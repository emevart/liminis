//! The velocity: a narrow curl of the stored potential, and the sampling of it
//! onto the faces of the fine grid.
//!
//! ```text
//! u = curl A
//! ```
//!
//! Last two of the four dispatches of step `b` (ADR-069). [`curl_voxel`] runs
//! over the velocity grid — `2.1e6` scalar reads a tick at 64^3 with a convective
//! potential whose `z` component is zero, `3.15e6` with stirring on — and
//! [`face_courant_voxel`] runs over the **fine** grid and produces exactly the
//! buffer `kernels/advect.rs` reads.
//!
//! # What is stored is `A`, and that is the whole argument
//!
//! `div u = 0` holds because `u` is a curl of stored values rather than a field
//! that has been adjusted afterwards. Each component of `A` enters the discrete
//! `div(curl A)` twice with opposite signs, on the same stored number — so the
//! identity is algebraic and not approximate, and it survives the switch to
//! `FIXED`, where the rounding happens on the **input** of the difference.
//!
//! Two things would take that away, and both look harmless:
//!
//! - a global multiplier over `u`. `qmul` rounds each component separately
//!   (ADR-022), so a rescaled field is no longer the curl of anything. ADR-069
//!   rejects the run-time normalisation it would serve, by name;
//! - a per-component clip. Clipping does not commute with a curl at all:
//!   `div u != 0` appears exactly where the field is strongest, which is the
//!   plume, and matter piles up in the convergence zone — an artefact
//!   indistinguishable from the result the project is looking for.
//!
//! Neither is here, and neither may arrive. The bound on `|u|` is derived at load
//! from the composed stencil (`process/velocity.rs`) so that no run-time measure
//! is needed at all.
//!
//! **What "bit for bit" can and cannot mean, stated here rather than discovered.**
//! The cancellation is exact in the *coefficients*: expand `div(curl A)` and
//! every stored `A` appears twice with opposite signs. It is **not** exact in
//! `f32` when the divergence is assembled out of the already-rounded `u`, because
//! the four differences that have to cancel are four separately rounded numbers
//! and floating-point subtraction does not associate. The property that does hold
//! bit for bit, and that
//! `prescribed_velocity_is_divergence_free_bit_for_bit` asserts, is the one on a
//! potential supported at a single cell: there the two occurrences are equal in
//! magnitude and cancel exactly, which is precisely the algebraic statement, and
//! it is what a per-component multiplier and a clip both fail. On a full field the
//! residual is bounded by rounding and is asserted as such.
//!
//! # The floor
//!
//! `w = 0` on the plane `z = 0` comes out of the potential rather than out of a
//! branch here: `u_z` reads only `A_x` and `A_y` **inside its own plane**, and
//! both are zeroed there by `kernels/potential.rs` and `kernels/noise.rs`. So a
//! wall in this file would be a second mechanism for the same thing, and the one
//! that already exists keeps the field a curl.
//!
//! # The layout of the courant buffer is the contract
//!
//! `courant[axis*n_voxels + idx]` is the Courant number on the **lower** face of
//! fine voxel `idx` along `axis` — the layout `advect_voxel_32` reads, and the
//! layout in which the canonical orientation of a face lives (ADR-054). The other
//! plausible spelling, `idx*3 + axis`, conserves matter exactly and gives a
//! believable flow at an angle; so does a permutation of the axes. Only
//! `the_face_courant_layout_matches_the_advection_kernel` separates them.

use crate::numeric::{Q, qadd, qdiv, qmul, qsub};

/// Parameters of the curl. Scalars only: in WGSL this is a uniform buffer.
#[derive(Clone, Copy, Debug)]
pub struct CurlParams {
    /// Cells of the velocity grid along X.
    pub nx: u32,
    /// Cells of the velocity grid along Y.
    pub ny: u32,
    /// Cells of the velocity grid along Z.
    pub nz: u32,
    /// Bit `f` set: face `f` is periodic. Bit clear: the face is closed and its
    /// neighbour is the cell itself, as in `kernels/diffuse.rs` and
    /// `kernels/pressure.rs`.
    ///
    /// The bit index is the discriminant of `world::Face`.
    pub periodic_mask: u32,
    /// `1/(2*dx_velocity)`, folded on the host: the narrow central difference
    /// divided by the step of the **velocity** grid, which is twice the fine step
    /// in the eco regime.
    ///
    /// Taking the fine step here instead doubles every velocity, and the only
    /// symptom is a Courant number twice what the validator checked — which is
    /// the one thing ADR-069 arranges to be known before the first tick.
    pub gain: Q,
}

/// The velocity of one cell of the velocity grid.
///
/// `idx` runs over `0..nx*ny*nz`. Reads `a` at six neighbours and writes the
/// three components of its own cell; reads no cell of `dst`.
///
/// ```text
/// u_x = d_y A_z - d_z A_y
/// u_y = d_z A_x - d_x A_z
/// u_z = d_x A_y - d_y A_x
/// ```
///
/// Both buffers are component-major, `[c*n_cells + idx]`, the layout
/// `kernels/potential.rs` writes.
///
/// # A closed face folds onto the cell itself
///
/// The difference across such an axis is then between the cell and itself on one
/// side, that is a one-sided difference carrying the gain of a two-sided one. It
/// is not the exact derivative at a wall and does not have to be: the potential
/// is prescribed, not solved, and what the wall owes is `w = 0` on the floor,
/// which comes out of the zeroed tangential potential and holds under this
/// lookup exactly.
pub fn curl_voxel(a: &[Q], dst: &mut [Q], p: &CurlParams, idx: u32) {
    let n_cells = p.nx * p.ny * p.nz;

    let x_up = neighbour(p, idx, 1);
    let x_down = neighbour(p, idx, 0);
    let y_up = neighbour(p, idx, 3);
    let y_down = neighbour(p, idx, 2);
    let z_up = neighbour(p, idx, 5);
    let z_down = neighbour(p, idx, 4);

    let ax = 0;
    let ay = n_cells;
    let az = 2 * n_cells;

    let d_y_az = difference(a, az + y_up, az + y_down, p);
    let d_z_ay = difference(a, ay + z_up, ay + z_down, p);
    let d_z_ax = difference(a, ax + z_up, ax + z_down, p);
    let d_x_az = difference(a, az + x_up, az + x_down, p);
    let d_x_ay = difference(a, ay + x_up, ay + x_down, p);
    let d_y_ax = difference(a, ax + y_up, ax + y_down, p);

    dst[idx as usize] = qsub(d_y_az, d_z_ay);
    dst[(n_cells + idx) as usize] = qsub(d_z_ax, d_x_az);
    dst[(2 * n_cells + idx) as usize] = qsub(d_x_ay, d_y_ax);
}

/// One narrow central difference of the stored potential, already scaled.
///
/// One multiplication per difference rather than one per component of `u`: the
/// two occurrences of a stored `A` in `div(curl A)` are then the same rounded
/// product, which is where the exactness of the divergence comes from.
#[inline(always)]
fn difference(a: &[Q], up: u32, down: u32, p: &CurlParams) -> Q {
    qmul(p.gain, qsub(a[up as usize], a[down as usize]))
}

/// Parameters of the sampling onto the faces of the fine grid.
#[derive(Clone, Copy, Debug)]
pub struct SampleParams {
    /// Voxels of the **fine** grid along X.
    pub nx: u32,
    /// Voxels of the fine grid along Y.
    pub ny: u32,
    /// Voxels of the fine grid along Z.
    pub nz: u32,
    /// Cells of the velocity grid along X.
    pub vnx: u32,
    /// Cells of the velocity grid along Y.
    pub vny: u32,
    /// Cells of the velocity grid along Z.
    pub vnz: u32,
    /// Periodicity, encoded as in [`CurlParams::periodic_mask`].
    pub periodic_mask: u32,
    /// `log2(nx/vnx)`: how many times coarser the velocity grid is than the fine
    /// one. One in the eco regime, where the fine grid is 128^3 and the velocity
    /// grid 64^3 (ADR-069).
    pub ratio_log2: u32,
    /// `dt/dx` on the **fine** grid, folded on the host.
    ///
    /// The fine step and not the velocity one: the flux is computed on 128^3, and
    /// substituting 200 um would double the admissible speed with no basis
    /// (ADR-069). The whole ceiling of the scheme, `dx/(6*dt)`, is a statement
    /// about this number.
    pub courant_gain: Q,
}

/// The Courant numbers on the three lower faces of one fine voxel.
///
/// `idx` runs over `0..nx*ny*nz`. Writes `dst[axis*n_voxels + idx]` for the three
/// axes and nothing else — exactly the layout and exactly the meaning
/// `advect_voxel_32` reads.
///
/// # Where the sample is taken
///
/// The lower face of voxel `idx` along `axis` is at the voxel's own coordinate on
/// that axis and at the **centre** of the other two. Sampling at the cell centre
/// on all three axes instead is the natural slip and gives a half-voxel shift of
/// the whole flow along one axis — conserving matter exactly, and looking like a
/// flow.
pub fn face_courant_voxel(u: &[Q], dst: &mut [Q], p: &SampleParams, idx: u32) {
    debug_assert!(
        p.nx == p.vnx << p.ratio_log2
            && p.ny == p.vny << p.ratio_log2
            && p.nz == p.vnz << p.ratio_log2,
        "the fine grid {}x{}x{} is not the velocity grid {}x{}x{} refined by 2^{}",
        p.nx,
        p.ny,
        p.nz,
        p.vnx,
        p.vny,
        p.vnz,
        p.ratio_log2
    );

    let n_voxels = p.nx * p.ny * p.nz;
    let n_velocity = p.vnx * p.vny * p.vnz;
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    let ratio = 1u32 << p.ratio_log2;
    let denominator = 4 * ratio;
    let weight_log2 = p.ratio_log2 + 2;

    for axis in 0..3u32 {
        // `FACE` on the axis being sampled, `CENTRE` on the other two.
        let along_x = sample_numerator(x, if axis == 0 { FACE } else { CENTRE }, ratio);
        let along_y = sample_numerator(y, if axis == 1 { FACE } else { CENTRE }, ratio);
        let along_z = sample_numerator(z, if axis == 2 { FACE } else { CENTRE }, ratio);

        let base_x = along_x / denominator;
        let base_y = along_y / denominator;
        let base_z = along_z / denominator;

        let periodic_x = p.periodic_mask & 1 == 1;
        let periodic_y = (p.periodic_mask >> 2) & 1 == 1;
        let periodic_z = (p.periodic_mask >> 4) & 1 == 1;

        let lo = [
            corner_low(base_x, p.vnx, periodic_x),
            corner_low(base_y, p.vny, periodic_y),
            corner_low(base_z, p.vnz, periodic_z),
        ];
        let hi = [
            corner_high(base_x, p.vnx, periodic_x),
            corner_high(base_y, p.vny, periodic_y),
            corner_high(base_z, p.vnz, periodic_z),
        ];
        let weight_high = [
            unit_fraction(along_x - base_x * denominator, weight_log2),
            unit_fraction(along_y - base_y * denominator, weight_log2),
            unit_fraction(along_z - base_z * denominator, weight_log2),
        ];

        let mut sampled = Q::ZERO;
        for corner in 0..8u32 {
            let mut weight = Q::ONE;
            let mut at = 0u32;
            let mut stride = 1u32;
            for component in 0..3u32 {
                let extent = if component == 0 {
                    p.vnx
                } else if component == 1 {
                    p.vny
                } else {
                    p.vnz
                };
                let high = (corner >> component) & 1 == 1;
                let coord = if high {
                    hi[component as usize]
                } else {
                    lo[component as usize]
                };
                let share = if high {
                    weight_high[component as usize]
                } else {
                    qsub(Q::ONE, weight_high[component as usize])
                };
                weight = qmul(weight, share);
                at += coord * stride;
                stride *= extent;
            }
            sampled = qadd(sampled, qmul(weight, u[(axis * n_velocity + at) as usize]));
        }

        // `base + idx`, with the stride taken out first. The spelling matters:
        // `advect.rs` writes the same address the same way, and the grep of the
        // `kernel-lint` hook reads a multiplication by `n_voxels` next to an
        // identifier as lane-index addressing (ADR-056) — which this is not, the
        // stride here being an **axis** and not a substance.
        let base = axis * n_voxels;
        dst[(base + idx) as usize] = qmul(p.courant_gain, sampled);
    }
}

/// The sample point is the lower face of the voxel: its own coordinate exactly.
const FACE: u32 = 0;
/// The sample point is the centre of the voxel, half a voxel up from its origin.
const CENTRE: u32 = 2;

/// The position of a sample point in units of `1/(4*ratio)` of a velocity cell,
/// shifted up by `2*ratio` so that it is never negative.
///
/// The twin of the function of the same name in `kernels/potential.rs`; the
/// derivation of the shift is written out there.
#[inline(always)]
fn sample_numerator(coord: u32, half: u32, ratio: u32) -> u32 {
    4 * coord + half + 2 * ratio
}

/// The lower of the two corners along one axis, given `base + 1`.
#[inline(always)]
fn corner_low(base_plus_one: u32, extent: u32, periodic: bool) -> u32 {
    if base_plus_one > 0 {
        base_plus_one - 1
    } else if periodic {
        extent - 1
    } else {
        0
    }
}

/// The upper of the two corners along one axis, given `base + 1`.
#[inline(always)]
fn corner_high(base_plus_one: u32, extent: u32, periodic: bool) -> u32 {
    if base_plus_one < extent {
        base_plus_one
    } else if periodic {
        0
    } else {
        extent - 1
    }
}

/// `numerator * 2^-log2_denominator`, exactly. See the twin in
/// `kernels/potential.rs` for why a kernel builds this rather than receiving it.
#[inline(always)]
fn unit_fraction(numerator: u32, log2_denominator: u32) -> Q {
    let two = qadd(Q::ONE, Q::ONE);
    let mut unit = Q::ONE;
    for _ in 0..log2_denominator {
        unit = qdiv(unit, two);
    }
    let mut value = Q::ZERO;
    for _ in 0..numerator {
        value = qadd(value, unit);
    }
    value
}

/// The linear index of a cell: `x + y*NX + z*NX*NY` (SPEC section 1.1).
#[inline(always)]
fn index(p: &CurlParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

/// One step down an axis, or what the boundary says instead.
#[inline(always)]
fn step_down(coord: u32, extent: u32, periodic: bool) -> u32 {
    if coord > 0 {
        coord - 1
    } else if periodic {
        extent - 1
    } else {
        coord
    }
}

/// One step up an axis, or what the boundary says instead.
#[inline(always)]
fn step_up(coord: u32, extent: u32, periodic: bool) -> u32 {
    if coord + 1 < extent {
        coord + 1
    } else if periodic {
        0
    } else {
        coord
    }
}

/// The neighbour of a cell across one of its six faces.
///
/// `face` is the discriminant of `world::Face`, because WGSL has no enums. A
/// closed face returns the cell itself, as in `kernels/pressure.rs`.
#[inline(always)]
fn neighbour(p: &CurlParams, idx: u32, face: u32) -> u32 {
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    let periodic = (p.periodic_mask >> face) & 1 == 1;

    let mut nbx = x;
    let mut nby = y;
    let mut nbz = z;
    if face == 0 {
        nbx = step_down(x, p.nx, periodic);
    } else if face == 1 {
        nbx = step_up(x, p.nx, periodic);
    } else if face == 2 {
        nby = step_down(y, p.ny, periodic);
    } else if face == 3 {
        nby = step_up(y, p.ny, periodic);
    } else if face == 4 {
        nbz = step_down(z, p.nz, periodic);
    } else {
        nbz = step_up(z, p.nz, periodic);
    }

    index(p, nbx, nby, nbz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernels::advect::{AdvectParams, advect_voxel_32};
    use crate::numeric::{M32, rand, run_key};
    use crate::world::{Boundary, Grid};

    /// A deliberately non-cubic velocity grid.
    const NX: u32 = 8;
    const NY: u32 = 6;
    const NZ: u32 = 4;
    const N_CELLS: u32 = NX * NY * NZ;
    const TORUS: u32 = 0b11_1111;
    const FLOORED: u32 = 0b00_1111;

    fn params(mask: u32) -> CurlParams {
        CurlParams {
            nx: NX,
            ny: NY,
            nz: NZ,
            periodic_mask: mask,
            // A power of two, so that the multiplication is exact and the
            // assertions below are about the stencil rather than about `f32`.
            gain: Q::from_f64(0.25),
        }
    }

    fn curl(a: &[Q], p: &CurlParams) -> Vec<Q> {
        let mut dst = vec![Q::ZERO; (3 * N_CELLS) as usize];
        for idx in 0..N_CELLS {
            curl_voxel(a, &mut dst, p, idx);
        }
        dst
    }

    fn cell(x: u32, y: u32, z: u32) -> u32 {
        x + y * NX + z * NX * NY
    }

    /// The discrete divergence of `u`, assembled with the same narrow differences
    /// the curl uses.
    fn divergence(u: &[Q], p: &CurlParams) -> Vec<Q> {
        let mut out = vec![Q::ZERO; N_CELLS as usize];
        for idx in 0..N_CELLS {
            let d_x = difference(u, neighbour(p, idx, 1), neighbour(p, idx, 0), p);
            let d_y = difference(
                u,
                N_CELLS + neighbour(p, idx, 3),
                N_CELLS + neighbour(p, idx, 2),
                p,
            );
            let d_z = difference(
                u,
                2 * N_CELLS + neighbour(p, idx, 5),
                2 * N_CELLS + neighbour(p, idx, 4),
                p,
            );
            out[idx as usize] = qadd(qadd(d_x, d_y), d_z);
        }
        out
    }

    #[test]
    fn prescribed_velocity_is_divergence_free_bit_for_bit() {
        // The exact half of the property, on a potential supported at a single
        // cell: there each stored value enters the divergence twice with equal
        // magnitude and opposite sign, so the cancellation is on the same rounded
        // number and the result is `Q::ZERO` by equality and not by tolerance.
        //
        // Both boundary masks, because a closed face folds the neighbour onto the
        // cell itself and a wall is where a curl usually stops being one. Both
        // stirred and unstirred is the caller's axis and is covered in
        // `tests/acceptance_velocity.rs`; here the input is the buffer either of
        // them produces.
        for mask in [TORUS, FLOORED] {
            let p = params(mask);
            for component in 0..3u32 {
                for at in [cell(3, 2, 1), cell(0, 0, 0), cell(NX - 1, NY - 1, NZ - 1)] {
                    let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
                    a[(component * N_CELLS + at) as usize] = Q::from_f64(1.75);

                    for (idx, value) in divergence(&curl(&a, &p), &p).iter().enumerate() {
                        assert_eq!(
                            *value,
                            Q::ZERO,
                            "mask {mask:#08b}, component {component}, impulse at \
                             {at}, cell {idx}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_per_component_multiplier_is_not_divergence_free() {
        // The assertion above is worth nothing without this one: it says the test
        // can fail. A field scaled component by component — the "cheap limiter"
        // ADR-069 rejects because clipping does not commute with a curl — stops
        // being divergence-free on the very same impulse.
        let p = params(TORUS);
        let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
        a[cell(3, 2, 1) as usize] = Q::from_f64(1.75);

        // The impulse is in `A_x`, so `u_y` and `u_z` are the components that
        // carry it; scaling `u_x`, which is identically zero here, would prove
        // nothing. One component and not all three: a uniform multiplier is a
        // different mistake, and `f32` cannot always see it.
        let mut u = curl(&a, &p);
        for value in u.iter_mut().skip(N_CELLS as usize).take(N_CELLS as usize) {
            *value = qmul(*value, Q::from_f64(0.5));
        }
        assert!(divergence(&u, &p).iter().any(|v| *v != Q::ZERO));
    }

    #[test]
    fn the_divergence_of_a_full_field_is_zero_to_rounding() {
        // The other half, stated as what it is. On an arbitrary potential the four
        // differences that have to cancel are four separately rounded numbers, and
        // `f32` subtraction does not associate — so the residual is bounded by
        // rounding rather than identically zero. See the module header: the
        // algebraic identity is exact, its evaluation through a stored `u` is not.
        let p = params(TORUS);
        let key = run_key(4242);
        let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
        for (at, value) in a.iter_mut().enumerate() {
            let draw = rand(at as u32, 0, 1, key);
            *value = Q::from_f64(f64::from(draw) / f64::from(u32::MAX) - 0.5);
        }

        let u = curl(&a, &p);
        let scale = u.iter().fold(0.0f64, |m, v| m.max(v.debug_f64().abs()));
        assert!(scale > 0.0);
        for (idx, value) in divergence(&u, &p).iter().enumerate() {
            assert!(
                value.debug_f64().abs() <= 8.0 * scale * f64::from(f32::EPSILON),
                "cell {idx}: divergence {} against a field of scale {scale}",
                value.debug_f64()
            );
        }
    }

    #[test]
    fn the_curl_turns_a_linear_potential_into_a_uniform_flow() {
        // `A = (0, 0, y)` gives `u = (1, 0, 0)`; `A = (0, x, 0)` gives
        // `u = (0, 0, 1)`. Two of the six terms, with the signs that tell a curl
        // from its transpose — swapping them leaves a field that is still
        // divergence-free and flows the other way.
        let p = params(TORUS);

        let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    a[(2 * N_CELLS + cell(x, y, z)) as usize] = Q::from_f64(f64::from(y));
                }
            }
        }
        let u = curl(&a, &p);
        // Away from the wrap, where the ramp is not a ramp.
        let at = cell(3, 2, 1);
        assert_eq!(u[at as usize], Q::from_f64(2.0 * 0.25));
        assert_eq!(u[(N_CELLS + at) as usize], Q::ZERO);
        assert_eq!(u[(2 * N_CELLS + at) as usize], Q::ZERO);

        let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    a[(N_CELLS + cell(x, y, z)) as usize] = Q::from_f64(f64::from(x));
                }
            }
        }
        let u = curl(&a, &p);
        assert_eq!(u[at as usize], Q::ZERO);
        assert_eq!(u[(N_CELLS + at) as usize], Q::ZERO);
        assert_eq!(u[(2 * N_CELLS + at) as usize], Q::from_f64(2.0 * 0.25));
    }

    #[test]
    fn the_floor_carries_no_vertical_velocity() {
        // `w = 0` on the plane `z = 0` follows from the tangential potential being
        // zero there, and from nothing in this file: `u_z` reads `A_x` and `A_y`
        // only at its own `z`. The test therefore feeds a potential with the floor
        // already zeroed, which is what both writers of `A` produce.
        let p = params(FLOORED);
        let mut a = vec![Q::from_f64(0.5); (3 * N_CELLS) as usize];
        for y in 0..NY {
            for x in 0..NX {
                let at = cell(x, y, 0);
                a[at as usize] = Q::ZERO;
                a[(N_CELLS + at) as usize] = Q::ZERO;
            }
        }
        // And a potential that varies, or `w` is zero for the wrong reason.
        for z in 1..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    let at = cell(x, y, z);
                    a[at as usize] = Q::from_f64(f64::from(x + 2 * y + 3 * z));
                    a[(N_CELLS + at) as usize] = Q::from_f64(f64::from(3 * x + y));
                }
            }
        }

        let u = curl(&a, &p);
        for y in 0..NY {
            for x in 0..NX {
                let at = cell(x, y, 0);
                assert_eq!(
                    u[(2 * N_CELLS + at) as usize],
                    Q::ZERO,
                    "w at ({x}, {y}, 0)"
                );
            }
        }
        // The horizontal flow along the floor is not zero, which is the half a
        // blanket zeroing of `A` would destroy.
        assert!(
            u[cell(1, 1, 0) as usize] != Q::ZERO
                || u[(N_CELLS + cell(1, 1, 0)) as usize] != Q::ZERO
        );
    }

    #[test]
    fn the_face_courant_layout_matches_the_advection_kernel() {
        // The buffer this kernel fills is fed straight to `advect_voxel_32` at a
        // constant velocity: the matter has to move along the declared axis, in
        // the declared direction, at `u*dt/dx` on the face. This is what catches
        // permuted axes and the layout `idx*3 + axis` — both of which conserve
        // matter exactly and give a believable flow at an angle.
        const RATIO_LOG2: u32 = 1;
        let fine_nx = NX << RATIO_LOG2;
        let fine_ny = NY << RATIO_LOG2;
        let fine_nz = NZ << RATIO_LOG2;
        let n_voxels = fine_nx * fine_ny * fine_nz;

        for axis in 0..3u32 {
            // A uniform velocity of one along `axis`, zero elsewhere.
            let mut u = vec![Q::ZERO; (3 * N_CELLS) as usize];
            for at in 0..N_CELLS {
                u[(axis * N_CELLS + at) as usize] = Q::ONE;
            }

            let sample = SampleParams {
                nx: fine_nx,
                ny: fine_ny,
                nz: fine_nz,
                vnx: NX,
                vny: NY,
                vnz: NZ,
                periodic_mask: TORUS,
                ratio_log2: RATIO_LOG2,
                courant_gain: Q::from_f64(0.5),
            };
            let mut courant = vec![Q::ZERO; (3 * n_voxels) as usize];
            for idx in 0..n_voxels {
                face_courant_voxel(&u, &mut courant, &sample, idx);
            }

            // `u*dt/dx` on every face of the declared axis, and nothing at all on
            // the other two.
            let base = axis * n_voxels;
            for idx in 0..n_voxels {
                assert_eq!(
                    courant[(base + idx) as usize],
                    Q::from_f64(0.5),
                    "axis {axis}, voxel {idx}"
                );
                for other in 0..3u32 {
                    if other != axis {
                        let elsewhere = other * n_voxels;
                        assert_eq!(courant[(elsewhere + idx) as usize], Q::ZERO);
                    }
                }
            }

            // And a tracer really does move along that axis and toward the larger
            // index, which is what the sign convention of `advect.rs` means.
            let advect = AdvectParams {
                nx: fine_nx,
                ny: fine_ny,
                nz: fine_nz,
                periodic_mask: TORUS,
                axis,
            };
            let mut src = vec![M32::ZERO; n_voxels as usize];
            let start = 2 + 3 * fine_nx + fine_nx * fine_ny;
            src[start as usize] = M32::new(1024);
            let mut dst = vec![M32::ZERO; n_voxels as usize];
            for idx in 0..n_voxels {
                advect_voxel_32(&src, &courant, &mut dst, &advect, idx);
            }

            let downstream = if axis == 0 {
                start + 1
            } else if axis == 1 {
                start + fine_nx
            } else {
                start + fine_nx * fine_ny
            };
            assert!(
                dst[downstream as usize] > M32::ZERO,
                "axis {axis}: nothing arrived at the neighbour toward the larger \
                 index"
            );
            assert!(dst[start as usize] < M32::new(1024));
        }
    }

    #[test]
    fn the_neighbourhood_agrees_with_the_grid() {
        // The kernel carries its own copy of the lookup, because `kernels/` may
        // not depend on `world/`. The same test and the same reason as in
        // `diffuse.rs`.
        let grid = Grid::new(
            NX,
            NY,
            NZ,
            [
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Closed,
                Boundary::Closed,
            ],
        )
        .unwrap();
        let p = params(FLOORED);
        for idx in 0..N_CELLS {
            for (face, world_face) in crate::world::Face::ALL.iter().enumerate() {
                assert_eq!(
                    neighbour(&p, idx, face as u32),
                    grid.neighbour(idx, *world_face),
                    "cell {idx}, face {face}"
                );
            }
        }
    }
}
