//! The convective half of the velocity potential: a **wide** horizontal
//! difference of temperature, and the interpolation of the result onto the
//! velocity grid.
//!
//! ```text
//! A_conv = L * ( d_y T~, -d_x T~, 0 )
//! ```
//!
//! Step `b` of the tick order (SPEC section 8), first two of its four dispatches
//! (ADR-069). The velocity itself is not here: what is stored is the potential
//! `A`, and `u = curl A` is taken by `kernels/curl.rs` out of the stored values.
//! That is the whole of why `div u = 0` holds exactly rather than approximately —
//! the rounding happens on the **input** of the curl and not on its output.
//!
//! # Why a wide difference and not the Laplacian it looks like
//!
//! `u_z = d_x A_y - d_y A_x = -L * (d_xx + d_yy) T~`, so this is a horizontal
//! Laplacian wearing a potential's clothes, and the difference between the two
//! spellings is the point of ADR-069 rather than a matter of taste.
//!
//! The bare `grad^2_h T` has a transfer function `L*k^2`: unbounded, largest at
//! the grid wavelength, and damped by conduction as `alpha*k^2` — the same power,
//! so there is no wavelength selection at any calibration whatsoever. Either
//! nothing grows or everything does, fastest at the mesh, and
//! `heated_bottom_produces_net_vertical_transport` would pass on a grid artefact:
//! word for word the outcome ADR-054 rejected superbee for.
//!
//! The composition "narrow difference of step `h` after a wide difference of
//! half-step `s = l_c`" has the transfer function
//!
//! ```text
//! L * sin(k*h) * sin(k*s) / (h*s)
//! ```
//!
//! which is **bounded** by `L/(h*s)`, vanishes at the grid wavelength (`k*h =
//! pi`), changes sign past `k*s > pi` — so modes shorter than `2*l_c` are damped
//! rather than amplified — and peaks near `lambda = 4*l_c`. That is what makes
//! `l_c` set the size of a cell instead of merely appearing to, and it is what
//! `the_wide_difference_does_not_degenerate_to_a_narrow_one` measures.
//!
//! `r` is therefore load-bearing and is counted in cells of the **enthalpy**
//! grid, the grid the difference is taken on (32^3 against a 128^3 base). Counted
//! in cells of the velocity grid it is out by exactly a factor of two, that is by
//! one octave in the selected wavelength, and nothing sees it: the field stays
//! smooth, the divergence stays an exact zero, the speed bound stays satisfied,
//! and `l_c` in the config stays what it was. The load-time refusal
//! `structure_length_below_the_temperature_cell_is_rejected` computes `r` from
//! the coarse step correctly and therefore cannot notice either — it checks the
//! input, not what the kernel did with it.
//!
//! # `T_ref` is not here and must not arrive
//!
//! `T = T_ref + H/C_cell` (ADR-044), and only the second term survives a
//! difference. Adding the first is algebraically free and numerically ruinous: at
//! `T_ref = 298.15 K` and an anomaly of hundredths of a kelvin, `f32` has spent
//! four decimal digits before the subtraction happens. The field stays smooth,
//! the divergence stays exact, the flow is simply quieter than it should be — and
//! that reads as "the calibration is not done yet". There is no reference
//! temperature in [`PotentialParams`], and that absence is the decision.
//!
//! `C_cell` arrives as a **field** rather than as a folded scalar, because it is
//! one (ADR-062: `sum(n_i*c_p,i)` on the coarse grid). A constant denominator, or
//! the solvent's taken from the registry, is worth 0.2% by ADR-062's own
//! measurement — and temperature is class `Q`, in no ledger, so nothing objects.
//!
//! # Two dispatches, not one
//!
//! [`potential_voxel`] runs over the **enthalpy** grid (2.6e5 scalar reads a tick
//! at 32^3); [`interpolate_potential`] runs over the **velocity** grid (4.2e6).
//! Merging them would recompute the wide difference eight times per velocity cell
//! for the eight corners it needs, which is the whole cost of the first pass
//! multiplied by the ratio of the two grids.
//!
//! What is interpolated is the **potential**, never the velocity. Interpolating
//! `u` would give a field that is no longer the curl of anything, and `div u`
//! would stop being an exact zero on the finer grid — for the same reason a
//! global rescaling would (ADR-069): `qmul` rounds each component separately.
//!
//! # The floor
//!
//! An impermeable floor is the tangential components of the potential zeroed on
//! the plane `z = 0` (ADR-069). `w = 0` there comes out exactly, the field stays a
//! curl, and `div u = 0` is untouched. One branch, here — and one more in
//! `kernels/noise.rs`, which is the trap: applied in only one of the two kernels
//! that write `A`, the floor holds at `stir_fraction = 0` and leaks the moment
//! stirring is switched on, with the divergence still exactly zero and the ledger
//! still closed. Matter simply goes through the floor and comes back through the
//! periodic lid.

use crate::numeric::{M64, Q, q_conc_64, qadd, qdiv, qmul, qsub};

/// Parameters of the wide difference. Scalars only: in WGSL this is a uniform
/// buffer, and every number in it is a place where the host's scale can drift
/// away from the kernel's (`ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug)]
pub struct PotentialParams {
    /// Cells of the **enthalpy** grid along X.
    pub cnx: u32,
    /// Cells of the enthalpy grid along Y.
    pub cny: u32,
    /// Cells of the enthalpy grid along Z.
    pub cnz: u32,
    /// Bit `f` set: face `f` is periodic and wraps. Bit clear: the face is closed
    /// and a step past it clamps to the wall.
    ///
    /// The bit index is the discriminant of `world::Face`: `0 = x_min`,
    /// `1 = x_max`, `2 = y_min`, `3 = y_max`, `4 = z_min`, `5 = z_max` — the same
    /// encoding `DiffuseParams` and `AdvectParams` use, because two kernels
    /// reading one host-side mask differently is a class of bug whose only
    /// symptom is matter going the wrong way at one wall.
    ///
    /// A closed face **clamps** rather than folding onto the voxel itself. The
    /// difference matters only within `r` cells of a wall, it can only shorten
    /// the arm of the difference, and it therefore cannot raise the `l1` norm of
    /// the composed stencil the speed bound is derived from — see
    /// `process::velocity::VelocityField::stencil_l1_norm`.
    pub periodic_mask: u32,
    /// The half-width of the wide difference, in cells of the **enthalpy** grid:
    /// `r = round(l_c/(2*dx_coarse))`, at least one (ADR-069).
    ///
    /// At `r = 0` the wide difference collapses into a narrow one, the bound on
    /// the gain goes with it, and so does the wavelength selection the whole
    /// scheme exists for. The host refuses that at load
    /// (`structure_length_below_the_temperature_cell_is_rejected`); this kernel
    /// asserts it in debug because a kernel cannot check a config.
    pub r: u32,
    /// The mobility, the reciprocal of the arm of the difference and the scale of
    /// the enthalpy field, folded into one number on the host:
    /// `L / (2*r*dx_coarse*units_per_joule)`.
    ///
    /// So the kernel knows neither `L`, nor `l_c`, nor `dx`, nor `k_E`
    /// (ADR-015, ADR-034). Leaving `1/(2*r*dx_coarse)` out is the natural folding
    /// mistake and the one `light.rs` names about `dz`: the difference stops
    /// being a derivative and the flow comes out `2*r*dx_coarse` times too fast,
    /// which at the eco regime is a factor of `1.6e-3` — and reads as an
    /// uncalibrated `u_conv_max`.
    pub conv_gain: Q,
}

/// The convective potential of one cell of the **enthalpy** grid.
///
/// `idx` runs over `0..cnx*cny*cnz`. Writes the three components of its own cell
/// and nothing else; reads no cell of `dst`.
///
/// # Layout
///
/// Component-major: `dst[c*n_cells + idx]`, `c` in `0..3`. The same shape as
/// `courant[axis*n_voxels + idx]` in `kernels/advect.rs`, which is the only
/// precedent in the corpus for a vector over a grid, and the only one the
/// advection kernel can read without a translation layer.
///
/// # `A_z` is identically zero, and it is written
///
/// `A_conv` has no `z` component by construction, and the cell is written with
/// `Q::ZERO` rather than left alone. Leaving it alone would make the buffer hold
/// whatever the previous tick put there — and with stirring on, that previous
/// value is a *noise* `A_z`, so a run with `stir_fraction > 0` for one tick would
/// go on stirring in `z` forever. A kernel that writes only part of its output is
/// the same failure `world::Field` warns about for a skipped voxel.
pub fn potential_voxel(
    enthalpy: &[M64],
    heat_capacity: &[Q],
    dst: &mut [Q],
    p: &PotentialParams,
    idx: u32,
) {
    debug_assert!(
        p.r >= 1,
        "the wide difference has a half-width of {} cells: at zero it is a narrow \
         difference and the wavelength selection of ADR-069 is gone",
        p.r
    );
    // `2*r < extent`, not `r < extent`: at exactly half the extent the two arms
    // of the difference wrap onto the **same** cell on a periodic axis, the
    // difference is identically zero, and the whole field comes out as a plausible
    // sheet of zeroes. ADR-069 bounds the search for `l_c` at `L_domain/2`, which
    // is `r = cn/4`, so a scenario inside the declared range never meets this.
    debug_assert!(
        2 * p.r < p.cnx && 2 * p.r < p.cny,
        "a half-width of {} cells does not fit a {}x{} horizontal grid: at {} the \
         two arms of the difference wrap onto one cell",
        p.r,
        p.cnx,
        p.cny,
        p.cnx.min(p.cny) / 2
    );

    let n_cells = p.cnx * p.cny * p.cnz;
    let plane = p.cnx * p.cny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.cnx;
    let x = within_plane - y * p.cnx;

    let periodic_x = p.periodic_mask & 1 == 1;
    let periodic_y = (p.periodic_mask >> 2) & 1 == 1;

    // The wide difference along Y gives `A_x`, along X gives `A_y` with the
    // opposite sign. Swapping the two, or losing the sign, leaves a field that is
    // still a curl, still divergence-free and still bounded — and turns every
    // plume upside down.
    let y_up = index(p, x, step_up_by(y, p.cny, periodic_y, p.r), z);
    let y_down = index(p, x, step_down_by(y, p.cny, periodic_y, p.r), z);
    let x_up = index(p, step_up_by(x, p.cnx, periodic_x, p.r), y, z);
    let x_down = index(p, step_down_by(x, p.cnx, periodic_x, p.r), y, z);

    let d_y = qsub(
        temperature(enthalpy, heat_capacity, y_up),
        temperature(enthalpy, heat_capacity, y_down),
    );
    let d_x = qsub(
        temperature(enthalpy, heat_capacity, x_up),
        temperature(enthalpy, heat_capacity, x_down),
    );

    dst[idx as usize] = qmul(p.conv_gain, d_y);
    dst[(n_cells + idx) as usize] = qmul(p.conv_gain, qsub(Q::ZERO, d_x));
    dst[(2 * n_cells + idx) as usize] = Q::ZERO;
}

/// The temperature **anomaly** of one coarse cell: `H/C_cell`, and not
/// `T_ref + H/C_cell`.
///
/// One named crossing from `M` into `Q` and not one back (ADR-060), the
/// composition `optical_depth` uses in `kernels/light.rs`: `q_conc` with `Q::ONE`
/// says "this amount, as a number", and the scale of the enthalpy field is inside
/// [`PotentialParams::conv_gain`] along with everything else.
#[inline(always)]
fn temperature(enthalpy: &[M64], heat_capacity: &[Q], at: u32) -> Q {
    qdiv(
        q_conc_64(enthalpy[at as usize], Q::ONE),
        heat_capacity[at as usize],
    )
}

/// Parameters of the interpolation from the enthalpy grid onto the velocity grid.
#[derive(Clone, Copy, Debug)]
pub struct InterpolateParams {
    /// Cells of the enthalpy grid along X.
    pub cnx: u32,
    /// Cells of the enthalpy grid along Y.
    pub cny: u32,
    /// Cells of the enthalpy grid along Z.
    pub cnz: u32,
    /// Cells of the velocity grid along X.
    pub vnx: u32,
    /// Cells of the velocity grid along Y.
    pub vny: u32,
    /// Cells of the velocity grid along Z.
    pub vnz: u32,
    /// Periodicity, encoded as in [`PotentialParams::periodic_mask`].
    pub periodic_mask: u32,
    /// `log2(vnx/cnx)`: how many times finer the velocity grid is. One in the eco
    /// regime, where enthalpy is 32^3 and velocity 64^3 (ADR-069).
    pub ratio_log2: u32,
    /// Non-zero: the floor is impermeable, so the tangential components of the
    /// potential are zeroed on the plane `z = 0` (ADR-069).
    ///
    /// A `u32` and not a `bool` because this struct becomes a WGSL uniform
    /// buffer, which has no booleans.
    pub floor_is_closed: u32,
}

/// One cell of the velocity grid, gathered from the eight coarse cells around it.
///
/// `idx` runs over `0..vnx*vny*vnz`; `src` is the coarse potential in the layout
/// [`potential_voxel`] writes, `dst` the same layout over the velocity grid.
///
/// # The corners come from the enthalpy grid
///
/// The mapping is to the grid the potential **was computed on**, not to the one
/// the reaction kernel uses and not to `World::velocity_cell_of`. A miss there
/// gives a neighbouring, entirely plausible, wrong cell; temperature is class `Q`
/// and enters no invariant at all, so nothing outside
/// `the_potential_reads_the_enthalpy_cell_not_the_velocity_cell` can see it. That
/// is the third of the three decisions `version.rs` records as arriving with
/// `World`.
///
/// # The floor
///
/// With `floor_is_closed`, `A_x` and `A_y` are zeroed on the whole plane `z = 0`
/// of the velocity grid, so `w = d_x A_y - d_y A_x` is an exact zero there — every
/// term of that difference lies in the same plane. `A_z` is left alone: it does
/// not enter `w`, and zeroing it would kill the horizontal flow along the floor
/// as well, which no record asks for.
pub fn interpolate_potential(src: &[Q], dst: &mut [Q], p: &InterpolateParams, idx: u32) {
    debug_assert!(
        p.vnx == p.cnx << p.ratio_log2
            && p.vny == p.cny << p.ratio_log2
            && p.vnz == p.cnz << p.ratio_log2,
        "the velocity grid {}x{}x{} is not the enthalpy grid {}x{}x{} refined by \
         2^{}",
        p.vnx,
        p.vny,
        p.vnz,
        p.cnx,
        p.cny,
        p.cnz,
        p.ratio_log2
    );

    let n_coarse = p.cnx * p.cny * p.cnz;
    let n_velocity = p.vnx * p.vny * p.vnz;
    let plane = p.vnx * p.vny;
    let vz = idx / plane;
    let within_plane = idx - vz * plane;
    let vy = within_plane / p.vnx;
    let vx = within_plane - vy * p.vnx;

    let ratio = 1u32 << p.ratio_log2;
    // The position of a velocity cell centre in continuous coarse-cell
    // coordinates is `(v + 0.5)/ratio - 0.5`, which is negative in the first half
    // cell of every axis. `sample_numerator` carries it as an integer over the
    // denominator `4*ratio` with the negative case already folded in — see there.
    let along_x = sample_numerator(vx, CENTRE, ratio);
    let along_y = sample_numerator(vy, CENTRE, ratio);
    let along_z = sample_numerator(vz, CENTRE, ratio);
    let denominator = 4 * ratio;
    let weight_log2 = p.ratio_log2 + 2;

    let base_x = along_x / denominator;
    let base_y = along_y / denominator;
    let base_z = along_z / denominator;

    let lo = [
        corner_low(base_x, p.cnx, p.periodic_mask & 1 == 1),
        corner_low(base_y, p.cny, (p.periodic_mask >> 2) & 1 == 1),
        corner_low(base_z, p.cnz, (p.periodic_mask >> 4) & 1 == 1),
    ];
    let hi = [
        corner_high(base_x, p.cnx, p.periodic_mask & 1 == 1),
        corner_high(base_y, p.cny, (p.periodic_mask >> 2) & 1 == 1),
        corner_high(base_z, p.cnz, (p.periodic_mask >> 4) & 1 == 1),
    ];
    let weight_high = [
        unit_fraction(along_x - base_x * denominator, weight_log2),
        unit_fraction(along_y - base_y * denominator, weight_log2),
        unit_fraction(along_z - base_z * denominator, weight_log2),
    ];

    for component in 0..3u32 {
        let base = component * n_coarse;
        let mut value = Q::ZERO;
        // The eight corners of the cube, as a three-bit counter: bit `a` picks
        // the high corner along axis `a`. Written as a loop rather than as eight
        // lines because eight lines is where one of them ends up reading `lo`
        // where it meant `hi`, and the result is a plausible field.
        for corner in 0..8u32 {
            let mut weight = Q::ONE;
            let mut at = 0u32;
            let mut stride = 1u32;
            for axis in 0..3u32 {
                let extent = if axis == 0 {
                    p.cnx
                } else if axis == 1 {
                    p.cny
                } else {
                    p.cnz
                };
                let high = (corner >> axis) & 1 == 1;
                let coord = if high {
                    hi[axis as usize]
                } else {
                    lo[axis as usize]
                };
                let share = if high {
                    weight_high[axis as usize]
                } else {
                    qsub(Q::ONE, weight_high[axis as usize])
                };
                weight = qmul(weight, share);
                at += coord * stride;
                stride *= extent;
            }
            value = qadd(value, qmul(weight, src[(base + at) as usize]));
        }

        let tangential = component < 2;
        let on_the_floor = vz == 0 && p.floor_is_closed != 0;
        dst[(component * n_velocity + idx) as usize] = if tangential && on_the_floor {
            Q::ZERO
        } else {
            value
        };
    }
}

/// The sample point is the centre of the cell, half a cell up from its origin.
const CENTRE: u32 = 2;

/// The position of a sample point in units of `1/(4*ratio)` of a coarse cell,
/// shifted up by `2*ratio` so that it is never negative.
///
/// The unshifted position of the centre of cell `coord` is
/// `(coord + half/4)/ratio - 1/2`; over the denominator `4*ratio` that is
/// `4*coord + half - 2*ratio`, which is negative in the first half cell of the
/// axis. Adding `4*ratio` — one whole coarse cell — makes it non-negative, so the
/// integer division below is a floor, and the base index that comes out is the
/// **true** base plus one. Every caller therefore works with `base + 1`, which is
/// why [`corner_low`] and [`corner_high`] take it in that form: a `u32` cannot
/// hold the `-1` the first half cell of every axis legitimately produces, and an
/// `i32` here would be one more thing to get wrong on the way to WGSL.
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

/// `numerator * 2^-log2_denominator`, exactly.
///
/// A `Q` out of two integers, without `Q::from_f64` — which is a host-side call
/// by the rule of `numeric/float.rs`, and would put a `f64` inside a shader.
/// Halving and adding are exact in `FLOAT` for a dyadic value of this size and
/// are the two operations `FIXED` is cheapest at, so the interpolation weights
/// are the same numbers in both modes.
///
/// The loops are short by construction: `log2_denominator` is `ratio_log2 + 2`
/// and `numerator` is under `4*ratio`, and the ratio between two neighbouring
/// grids of SPEC section 1.5 is two.
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

/// One step of `r` cells up an axis, or what the boundary says instead.
///
/// A comparison rather than a modulo, for the reason `kernels/pressure.rs` gives:
/// an integer division is the expensive instruction of the whole lookup on a GPU.
/// Valid because `r < extent` is asserted in [`potential_voxel`].
#[inline(always)]
fn step_up_by(coord: u32, extent: u32, periodic: bool, r: u32) -> u32 {
    if coord + r < extent {
        coord + r
    } else if periodic {
        coord + r - extent
    } else {
        extent - 1
    }
}

/// One step of `r` cells down an axis, or what the boundary says instead.
#[inline(always)]
fn step_down_by(coord: u32, extent: u32, periodic: bool, r: u32) -> u32 {
    if coord >= r {
        coord - r
    } else if periodic {
        coord + extent - r
    } else {
        0
    }
}

/// The linear index of a coarse cell: `x + y*CNX + z*CNX*CNY` (SPEC section 1.1).
///
/// The kernel's own copy of `world::Grid::index`, for the reason `diffuse.rs`
/// keeps one: `kernels/` depends on `numeric/` and on nothing else.
#[inline(always)]
fn index(p: &PotentialParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.cnx + z * p.cnx * p.cny
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, Grid};

    /// A deliberately non-cubic coarse grid: on a cubic one every bug that swaps
    /// two axes is invisible, because all three strides are equal.
    const CNX: u32 = 8;
    const CNY: u32 = 6;
    const CNZ: u32 = 4;
    const N_COARSE: u32 = CNX * CNY * CNZ;
    const RATIO_LOG2: u32 = 1;
    const VNX: u32 = CNX << RATIO_LOG2;
    const VNY: u32 = CNY << RATIO_LOG2;
    const VNZ: u32 = CNZ << RATIO_LOG2;
    const N_VELOCITY: u32 = VNX * VNY * VNZ;

    /// Periodic in X and Y, closed floor and lid — the eco-regime default of SPEC
    /// section 1.6, minus the `exchange` face `Grid::new` refuses.
    const FLOORED: u32 = 0b00_1111;
    const TORUS: u32 = 0b11_1111;

    const GAIN: f64 = 1.0 / 64.0;

    fn params(mask: u32, r: u32) -> PotentialParams {
        PotentialParams {
            cnx: CNX,
            cny: CNY,
            cnz: CNZ,
            periodic_mask: mask,
            r,
            conv_gain: Q::from_f64(GAIN),
        }
    }

    fn interpolate_params(mask: u32, floor_is_closed: u32) -> InterpolateParams {
        InterpolateParams {
            cnx: CNX,
            cny: CNY,
            cnz: CNZ,
            vnx: VNX,
            vny: VNY,
            vnz: VNZ,
            periodic_mask: mask,
            ratio_log2: RATIO_LOG2,
            floor_is_closed,
        }
    }

    /// A unit heat capacity, so that the enthalpy of a cell *is* its temperature
    /// anomaly in storage units and every expectation below is exact.
    fn unit_capacity() -> Vec<Q> {
        vec![Q::ONE; N_COARSE as usize]
    }

    fn dispatch(enthalpy: &[M64], capacity: &[Q], p: &PotentialParams) -> Vec<Q> {
        let mut dst = vec![Q::ZERO; (3 * N_COARSE) as usize];
        for idx in 0..N_COARSE {
            potential_voxel(enthalpy, capacity, &mut dst, p, idx);
        }
        dst
    }

    fn interpolate(src: &[Q], p: &InterpolateParams) -> Vec<Q> {
        let mut dst = vec![Q::ZERO; (3 * N_VELOCITY) as usize];
        for idx in 0..N_VELOCITY {
            interpolate_potential(src, &mut dst, p, idx);
        }
        dst
    }

    fn coarse_index(x: u32, y: u32, z: u32) -> u32 {
        x + y * CNX + z * CNX * CNY
    }

    fn velocity_index(x: u32, y: u32, z: u32) -> u32 {
        x + y * VNX + z * VNX * VNY
    }

    #[test]
    fn horizontally_uniform_temperature_produces_no_velocity() {
        // A temperature that depends on `z` alone gives an identically zero
        // potential, exactly. The assertion is only non-vacuous beside
        // `heated_bottom_produces_net_vertical_transport`: a kernel returning
        // zeros passes this one and fails that one.
        //
        // The second half is the one worth writing down: stratification along `z`
        // on its own must produce nothing either, which is the checkable form of
        // "no vertical difference leaked into the potential".
        let p = params(FLOORED, 2);
        let mut enthalpy = vec![M64::ZERO; N_COARSE as usize];
        for z in 0..CNZ {
            for y in 0..CNY {
                for x in 0..CNX {
                    enthalpy[coarse_index(x, y, z) as usize] = M64::new(1000 - 250 * i64::from(z));
                }
            }
        }

        let dst = dispatch(&enthalpy, &unit_capacity(), &p);
        for (at, value) in dst.iter().enumerate() {
            assert_eq!(*value, Q::ZERO, "component cell {at} is not zero");
        }
    }

    #[test]
    fn the_wide_difference_reads_r_cells_of_the_enthalpy_grid() {
        // A single hot cell, and the response has to appear exactly `r` cells away
        // along Y in `A_x` and along X in `A_y` — never at the neighbour. This is
        // the assertion that separates `r` in coarse cells from `r` in velocity
        // cells: at half the radius the response lands on a different cell
        // entirely, while the field stays smooth and bounded.
        const HOT: i64 = 4096;
        for r in [1u32, 2] {
            let p = params(TORUS, r);
            let mut enthalpy = vec![M64::ZERO; N_COARSE as usize];
            let (hx, hy, hz) = (4u32, 3u32, 2u32);
            enthalpy[coarse_index(hx, hy, hz) as usize] = M64::new(HOT);

            let dst = dispatch(&enthalpy, &unit_capacity(), &p);
            let expected = Q::from_f64(GAIN * HOT as f64);

            // `A_x = gain*(T(y+r) - T(y-r))`, so the cell `r` **below** the hot
            // one sees `+`, and the cell `r` above sees `-`.
            let below = coarse_index(hx, (hy + CNY - r) % CNY, hz);
            let above = coarse_index(hx, hy + r, hz);
            assert_eq!(dst[below as usize], expected);
            assert_eq!(dst[above as usize], qsub(Q::ZERO, expected));

            // `A_y = -gain*(T(x+r) - T(x-r))`, mirrored.
            let left = coarse_index((hx + CNX - r) % CNX, hy, hz);
            let right = coarse_index(hx + r, hy, hz);
            assert_eq!(dst[(N_COARSE + left) as usize], qsub(Q::ZERO, expected));
            assert_eq!(dst[(N_COARSE + right) as usize], expected);

            // And the immediate neighbour at `r > 1` sees nothing at all.
            if r > 1 {
                let next_door = coarse_index(hx, hy + 1, hz);
                assert_eq!(dst[next_door as usize], Q::ZERO);
            }
        }
    }

    #[test]
    fn the_z_component_of_the_convective_potential_is_written_zero() {
        // Written rather than left alone: the buffer is shared with the noise
        // kernel across ticks, and a `A_z` left over from a stirred tick would go
        // on stirring in `z` forever.
        let p = params(TORUS, 1);
        let mut dst = vec![Q::from_f64(7.0); (3 * N_COARSE) as usize];
        let enthalpy = vec![M64::new(5); N_COARSE as usize];
        for idx in 0..N_COARSE {
            potential_voxel(&enthalpy, &unit_capacity(), &mut dst, &p, idx);
        }
        for at in 2 * N_COARSE..3 * N_COARSE {
            assert_eq!(dst[at as usize], Q::ZERO);
        }
    }

    #[test]
    fn the_interpolation_reproduces_a_constant_and_a_linear_ramp() {
        // Trilinear weights sum to one, so a constant potential survives exactly;
        // and on a ramp along X the interpolated value is the ramp evaluated at
        // the velocity cell centre. The second half is what catches the weights
        // being applied to the wrong corner — a swap of `lo` and `hi` reproduces a
        // constant perfectly and mirrors the ramp.
        let p = interpolate_params(TORUS, 0);

        let mut src = vec![Q::ZERO; (3 * N_COARSE) as usize];
        for at in 0..N_COARSE {
            src[at as usize] = Q::from_f64(3.5);
        }
        let dst = interpolate(&src, &p);
        for at in 0..N_VELOCITY {
            assert_eq!(dst[at as usize], Q::from_f64(3.5), "cell {at}");
        }

        // A ramp along X, away from the wrap so that the periodic corner does not
        // enter: coarse cell `cx` holds `cx`, and velocity cell `vx` sits at
        // `(vx + 0.5)/2 - 0.5` in coarse coordinates.
        let mut src = vec![Q::ZERO; (3 * N_COARSE) as usize];
        for z in 0..CNZ {
            for y in 0..CNY {
                for x in 0..CNX {
                    src[coarse_index(x, y, z) as usize] = Q::from_f64(f64::from(x));
                }
            }
        }
        let dst = interpolate(&src, &p);
        for vx in 2..VNX - 2 {
            let at = velocity_index(vx, 3, 2);
            let expected = (f64::from(vx) + 0.5) / 2.0 - 0.5;
            assert_eq!(
                dst[at as usize],
                Q::from_f64(expected),
                "velocity column {vx}"
            );
        }
    }

    #[test]
    fn the_closed_floor_zeroes_the_tangential_potential() {
        // The branch that makes `w = 0` on the floor exact. `A_z` is deliberately
        // untouched: it does not enter `w`, and zeroing it would stop the
        // horizontal flow along the floor as well.
        let p = interpolate_params(FLOORED, 1);
        let src = vec![Q::from_f64(2.0); (3 * N_COARSE) as usize];
        let dst = interpolate(&src, &p);

        for vy in 0..VNY {
            for vx in 0..VNX {
                let at = velocity_index(vx, vy, 0);
                assert_eq!(dst[at as usize], Q::ZERO, "A_x at ({vx}, {vy}, 0)");
                assert_eq!(dst[(N_VELOCITY + at) as usize], Q::ZERO);
                assert_eq!(dst[(2 * N_VELOCITY + at) as usize], Q::from_f64(2.0));
            }
        }
        // And one plane up the potential is untouched, or the assertion above is
        // about a buffer of zeroes.
        let at = velocity_index(1, 1, 1);
        assert_eq!(dst[at as usize], Q::from_f64(2.0));

        // With an open floor the same call leaves the plane alone.
        let open = interpolate(&src, &interpolate_params(TORUS, 0));
        assert_eq!(open[velocity_index(1, 1, 0) as usize], Q::from_f64(2.0));
    }

    #[test]
    fn the_reference_temperature_never_enters_the_difference() {
        // The kernel divides `H` by `C_cell` and never adds a reference
        // temperature — `PotentialParams` has no field for one, which is the whole
        // of the guarantee. What this test measures is the *cost* of the other
        // spelling, so that the absence reads as a decision rather than an
        // omission: the same anomaly carried on top of `T_ref = 298.15 K` in the
        // same `f32` gives a different potential, and the difference is the four
        // decimal digits `f32` has already spent.
        //
        // A run that made that mistake would be smooth, divergence-free, bounded
        // and simply quieter than it should be.
        const ANOMALY: f64 = 0.01;
        const T_REF: f64 = 298.15;
        let p = params(TORUS, 1);

        // The anomaly alone, as the kernel is given it.
        let capacity = unit_capacity();
        let mut anomaly = vec![Q::ZERO; N_COARSE as usize];
        for at in 0..N_COARSE {
            anomaly[at as usize] = Q::from_f64(if at % 3 == 0 { ANOMALY } else { 0.0 });
        }
        let straight = difference_of(&anomaly, &p);

        // The same field with the reference temperature carried through it.
        let mut absolute = vec![Q::ZERO; N_COARSE as usize];
        for at in 0..N_COARSE {
            absolute[at as usize] = Q::from_f64(T_REF + anomaly[at as usize].debug_f64());
        }
        let offset = difference_of(&absolute, &p);

        assert_ne!(
            straight, offset,
            "adding T_ref before the difference is not free in f32; if these are \
             equal the fixture no longer measures anything"
        );
        // The anomaly form is the exact one: the difference is a difference of two
        // exactly representable values.
        assert_eq!(straight, Q::from_f64(ANOMALY));
        assert!(
            (offset.debug_f64() - ANOMALY).abs() > ANOMALY * 1e-4,
            "T_ref cost less than the four decimal digits this test is about"
        );

        // And the kernel itself is a function of `H/C` alone: doubling `C` halves
        // the anomaly and halves the potential exactly.
        let mut enthalpy = vec![M64::ZERO; N_COARSE as usize];
        for at in 0..N_COARSE {
            enthalpy[at as usize] = M64::new(i64::from(at % 5) * 256);
        }
        let once = dispatch(&enthalpy, &capacity, &p);
        let twice = dispatch(
            &enthalpy,
            &vec![qadd(Q::ONE, Q::ONE); N_COARSE as usize],
            &p,
        );
        for at in 0..(2 * N_COARSE) as usize {
            assert_eq!(qadd(twice[at], twice[at]), once[at], "component cell {at}");
        }
    }

    /// The wide difference of a `Q` field along Y at one cell, assembled without
    /// the kernel — the two spellings of the temperature meet here.
    fn difference_of(temperature: &[Q], p: &PotentialParams) -> Q {
        let (x, y, z) = (4u32, 3u32, 2u32);
        let up = coarse_index(x, y + p.r, z);
        let down = coarse_index(x, y - p.r, z);
        qsub(temperature[up as usize], temperature[down as usize])
    }

    #[test]
    fn the_coarse_indexing_agrees_with_the_grid() {
        // The kernel carries its own copy of the index, because `kernels/` may not
        // depend on `world/`. The same test and the same reason as
        // `the_neighbourhood_agrees_with_the_grid` in `diffuse.rs`: a test may
        // depend on everything.
        let grid = Grid::new(CNX, CNY, CNZ, [Boundary::Closed; 6]).unwrap();
        let p = params(TORUS, 1);
        assert_eq!(grid.n_voxels(), N_COARSE);
        for z in 0..CNZ {
            for y in 0..CNY {
                for x in 0..CNX {
                    assert_eq!(index(&p, x, y, z), grid.index(x, y, z));
                }
            }
        }
    }
}
