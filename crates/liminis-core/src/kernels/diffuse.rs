//! Diffusion of one substance: a 7-point stencil in gather form.
//!
//! The first kernel of the project, and the shape every later one copies. It is
//! the skeleton of `ARCHITECTURE.md` made to compile, with the properties that
//! skeleton exists to protect restated next to the lines that carry them.
//!
//! # Gather, and why conservation is free
//!
//! A voxel computes its own new amount by summing the flux across its six faces
//! and writes **only its own cell** (ADR-034, SPEC section 4.2). It never writes
//! to a neighbour. ADR-005 states conservation as "subtract from the donor, add
//! to the acceptor", which reads like a kernel that touches both cells; that
//! form needs an atomic per face on the GPU and makes the result depend on the
//! traversal order.
//!
//! In gather form conservation follows from the flux function being
//! antisymmetric:
//!
//! ```text
//! flux(a, b) == -flux(b, a)   for every a and b
//! ```
//!
//! Voxel `i` adds `flux(here, there)` to itself and voxel `j`, in the same
//! substep and off the same state `N`, adds exactly the opposite. The sum over
//! the domain is identically zero — no atomics, no synchronisation, and no
//! tolerance. See [`flux_32`] for why the property holds by construction rather
//! than by test.
//!
//! # What is not here
//!
//! The number of substeps, the coefficient `D`, the timestep, the voxel edge,
//! the boundary conditions of the scenario, the buffers. All of it is folded or
//! owned on the host, in `process/diffuse.rs`; this file receives `alpha` as one
//! number and a mask of six bits, and does not know how either was made
//! (`ARCHITECTURE.md`, ADR-034).

use crate::numeric::{M32, M64, Q, q_conc_32, q_conc_64, q_round_32, q_round_64, qmul};

/// Parameters of one application. Scalars only: in WGSL this is a uniform
/// buffer, and every quantity in it is a place where a scale can drift apart
/// from the one the host meant.
///
/// Folded by the host, never derived here — a kernel that knew `D`, `dt` and
/// `dx` separately could combine them in the wrong order, and the wrong order
/// is not visible in the result (`ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug)]
pub struct DiffuseParams {
    /// Voxels along X.
    pub nx: u32,
    /// Voxels along Y.
    pub ny: u32,
    /// Voxels along Z.
    pub nz: u32,
    /// Bit `f` set: face `f` is periodic and wraps. Bit clear: the face is
    /// closed and its neighbour is the voxel itself, which carries no flux
    /// because an antisymmetric function of two equal arguments is zero
    /// (`world::Grid::neighbour`).
    ///
    /// The bit index is the discriminant of `world::Face`: `0 = x_min`,
    /// `1 = x_max`, `2 = y_min`, `3 = y_max`, `4 = z_min`, `5 = z_max`. A mask
    /// rather than an array of enums for the reason the reaction skeleton gives
    /// for `width_mask` (`ARCHITECTURE.md`): WGSL has no enums, and a mask
    /// indexed by a loop counter is a shift and an and.
    ///
    /// `exchange` has no encoding here on purpose. It moves matter into the
    /// `BOUNDARY_EXCHANGE` channel (SPEC section 7) and there are no channel
    /// counters yet, so the host refuses such a grid rather than letting it
    /// arrive as one of these two cases.
    pub periodic_mask: u32,
    /// `D*dt/dx^2` for **one substep**, folded on the host (ADR-030).
    ///
    /// Stability of the explicit 7-point stencil wants `6*alpha <= 1`; the host
    /// gets there by splitting the tick into `n = ceil(6*D*dt/dx^2)` substeps
    /// and dividing, and the kernel neither checks nor knows about it.
    pub alpha: Q,
}

/// Generates one storage width of the diffusion kernel.
///
/// Invoked twice, immediately below. ADR-040 derives the storage width of an
/// amount from its declared concentrations, so both widths have to exist, and
/// `NUMERIC.md` section 5 already settled how a two-width axis is expressed:
/// generate both from one text. Two hand-written kernels drift apart within a
/// month and the drift surfaces as "the 64-bit substance somehow has different
/// dynamics". This is the same tool `numeric/m.rs` and `numeric/convert.rs` use,
/// one level up, and the same one the shader templater will use one level down.
macro_rules! define_diffuse {
    ($voxel:ident, $flux:ident, $m:ty, $q_conc:ident, $q_round:ident) => {
        #[doc = concat!("Diffusive flux into `here` from `there`, in `", stringify!($m), "`.")]
        ///
        /// `alpha * (there - here)`, rounded by the one rule of the system.
        ///
        /// # Antisymmetry is a property of this function, not of a test
        ///
        /// Conservation in gather form rests on `flux(a, b) == -flux(b, a)`
        /// holding for **every** pair, exactly (ADR-034). It does, and by
        /// construction, because every step of the chain commutes with negation:
        ///
        /// 1. `there - here` is integer subtraction, so swapping the arguments
        ///    negates it exactly (`numeric::M32`);
        /// 2. the amount becomes a `Q` through `q_conc`, which is a
        ///    multiplication by a folded constant — and multiplication carries
        ///    the sign, in IEEE-754 and in fixed point alike;
        /// 3. `qmul` by `alpha` likewise;
        /// 4. `q_round` rounds halves **away from zero**: `1.5 -> 2` and
        ///    `-1.5 -> -2` (`NUMERIC.md` section 3).
        ///
        /// Step 4 is the fragile one. `floor` is the natural way to round an
        /// integer shift and the one every numeric library makes cheapest, and
        /// it is not symmetric: `floor(1.5) = 1` but `floor(-1.5) = -2`. A flux
        /// written with it compiles, passes the grep in `kernel-lint`, looks
        /// right, and leaks one unit per face per substep until the ledger
        /// diverges a few thousand ticks later. `flux_is_antisymmetric` is the
        /// only thing in the project that would notice.
        ///
        /// A corollary the neighbourhood lookup depends on: `flux(a, a) == 0`
        /// for any `a`, because an antisymmetric function of two equal arguments
        /// is its own negation. That is what makes a closed face carry nothing
        /// without a branch.
        // TODO(m_scale): `ARCHITECTURE.md` writes this line as
        // `m_scale(b - a, alpha)` and imports `m_scale` from `numeric/`. That
        // function does not exist: `NUMERIC.md` section 1 lists exactly three
        // crossings between M and Q and `m_scale` is not one of them, so
        // `numeric/convert.rs` left it unwritten with a TODO of its own rather
        // than invent a fourth place where rounding may happen.
        //
        // The composition below is not that fourth place. It goes down through
        // `q_conc` and back up through `q_round`, both of them named crossings,
        // and it rounds once, in the one place the system allows — which is why
        // `world/grid.rs` already writes the diffusive flux exactly this way in
        // `a_closed_face_carries_no_flux`. `Q::ONE` as the per-unit factor says
        // "an amount difference, as a number": alpha is dimensionless and the
        // flux is in the same storage units as the amounts.
        //
        // If a decision ever adds `m_scale`, this is the single line that
        // changes, in both widths at once.
        #[inline(always)]
        pub fn $flux(here: $m, there: $m, alpha: Q) -> $m {
            $q_round(qmul($q_conc(there - here, Q::ONE), alpha))
        }

        #[doc = concat!("One substep of diffusion in one voxel, in `", stringify!($m), "`.")]
        ///
        /// Reads `src`, writes `dst[idx]` and nowhere else. The result does not
        /// depend on the order the kernel is called in for different `idx`,
        /// because nothing any voxel writes is visible to any other: `src` is
        /// state `N` and `dst` is state `N+1`, and they are different buffers
        /// (ADR-034, `world::Field`).
        ///
        /// Every voxel of the lane must be visited before the buffers are
        /// swapped — a skipped voxel is not left unchanged, it is left two
        /// steps stale. A dispatch over `0..n_voxels` satisfies that by
        /// construction, and on the GPU `idx` is `global_invocation_id`.
        ///
        /// # Amounts may go negative, and this kernel will not stop them
        ///
        /// At the stability limit `alpha = 1/6` a nearly empty voxel can be
        /// overdrawn by rounding: six faces each carrying `round(alpha * H)`
        /// take up to `H + 3` units out of a pool of `H`. The smallest case is
        /// concrete — `H = 3` at `alpha = 1/6` sends one unit across each of six
        /// faces and lands on `-3`.
        ///
        /// Clamping here would be worse than the symptom: it would destroy the
        /// mass the neighbours have already been given and break conservation
        /// silently, which is the one property the whole scheme is built on. So
        /// the kernel does the arithmetic it was asked for, and
        /// `a_starved_voxel_is_overdrawn_rather_than_clamped` pins that it does.
        ///
        /// The other end of the range is guarded elsewhere: `net` accumulates in
        /// the width of the amounts, so a field near the storage ceiling would
        /// overflow it — loudly in debug, and only if the scale derivation of
        /// ADR-039 was fed a wrong `max_conc`.
        // TODO(positivity): what to do about it is not decided. SPEC section 4.2
        // gives advection a positivity condition of its own, stricter than the
        // Courant condition, and `ACCEPTANCE.md` names
        // `advection_never_produces_negative_amount` — but there is no diffusive
        // counterpart in either document, and no ADR sets a bound on alpha
        // tighter than stability. The honest options are a stricter alpha bound
        // at load time, a positivity-preserving flux limiter, or accepting a
        // bounded undershoot; picking one is a decision about the physics, and
        // it belongs in the journal.
        pub fn $voxel(src: &[$m], dst: &mut [$m], p: &DiffuseParams, idx: u32) {
            let here = src[idx as usize];
            let mut net = <$m>::ZERO;

            for face in 0..6u32 {
                let n = neighbour(p, idx, face);
                net += $flux(here, src[n as usize], p.alpha);
            }

            dst[idx as usize] = here + net;
        }
    };
}

define_diffuse!(diffuse_voxel_32, flux_32, M32, q_conc_32, q_round_32);
define_diffuse!(diffuse_voxel_64, flux_64, M64, q_conc_64, q_round_64);

/// The linear index of a voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
///
/// The kernel's own copy of `world::Grid::index`, and it has to be its own:
/// `kernels/` depends on `numeric/` and on nothing else, and a `Grid` is a host
/// type that has no meaning in WGSL. `world/grid.rs` is the authority the copy
/// has to agree with, and `the_neighbourhood_agrees_with_the_grid` is what keeps
/// the two from drifting.
#[inline(always)]
fn index(p: &DiffuseParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

/// One step down an axis, or what the boundary says instead.
///
/// A comparison rather than a modulo: `(coord + extent - 1) % extent` is the
/// same answer and one integer division, the expensive instruction of the whole
/// lookup on a GPU.
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

/// The neighbour of a voxel across one of its six faces.
///
/// `face` is the discriminant of `world::Face`, because WGSL has no enums and
/// the kernel loops over a plain integer. A **closed** face returns the voxel
/// itself: that is not a sentinel for "no neighbour", it is the answer that
/// makes the face carry no flux without a branch, since `flux(a, a) == 0` for
/// any antisymmetric flux. The same is true of an axis one voxel deep, periodic
/// or not — the torus closes onto itself.
#[inline(always)]
fn neighbour(p: &DiffuseParams, idx: u32, face: u32) -> u32 {
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
    use crate::world::{Boundary, Face, Grid};

    /// A deliberately non-cubic grid: a cubic one hides every bug that mixes up
    /// the axes, because all three strides are equal.
    const NX: u32 = 3;
    const NY: u32 = 4;
    const NZ: u32 = 5;
    const N_VOXELS: u32 = NX * NY * NZ;

    /// All six faces periodic.
    const TORUS: u32 = 0b11_1111;
    /// The eco-regime default of SPEC section 1.6, minus the face that cannot be
    /// built yet: periodic in X and Y, solid floor and lid in Z.
    const FLOORED: u32 = 0b00_1111;

    fn params(periodic_mask: u32, alpha: f64) -> DiffuseParams {
        DiffuseParams {
            nx: NX,
            ny: NY,
            nz: NZ,
            periodic_mask,
            alpha: Q::from_f64(alpha),
        }
    }

    /// A value that depends on the index in a way no symmetry of the stencil can
    /// cancel by accident.
    fn pattern(idx: u32) -> i64 {
        (idx as i64 * 7919) % 1000 - 400
    }

    fn total_32(buffer: &[M32]) -> i64 {
        buffer.iter().map(|v| v.to_i64()).sum()
    }

    fn total_64(buffer: &[M64]) -> i64 {
        buffer.iter().map(|v| v.to_i64()).sum()
    }

    /// `ACCEPTANCE.md`, section "Conservation" — the name is fixed there, and
    /// the document calls it the most important test in the whole list.
    ///
    /// The sweep is deterministic rather than `proptest` because the crate has no
    /// dev-dependency on it; it walks the halves, the sign changes and the
    /// arguments an alpha of a sixth turns into exact halves, which is where a
    /// rounding rule that is not symmetric about zero shows itself.
    #[test]
    fn flux_is_antisymmetric() {
        let alphas = [1.0 / 6.0, 0.125, 0.5, 1.0, 1.0e-3, 0.0];
        let amounts = [
            -1_000_000i64,
            -1001,
            -100,
            -9,
            -3,
            -1,
            0,
            1,
            3,
            9,
            100,
            1001,
            1_000_000,
        ];

        for alpha in alphas {
            let alpha = Q::from_f64(alpha);
            for a in amounts {
                for b in amounts {
                    let narrow = flux_32(M32::new(a as i32), M32::new(b as i32), alpha);
                    let swapped = flux_32(M32::new(b as i32), M32::new(a as i32), alpha);
                    assert_eq!(narrow, -swapped, "flux_32 is not antisymmetric ({a}, {b})");

                    let wide = flux_64(M64::new(a), M64::new(b), alpha);
                    let swapped = flux_64(M64::new(b), M64::new(a), alpha);
                    assert_eq!(wide, -swapped, "flux_64 is not antisymmetric ({a}, {b})");

                    // And the two widths are one text, so they had better agree.
                    assert_eq!(narrow.to_i64(), wide.to_i64());
                }
            }
        }
    }

    #[test]
    fn a_face_with_nothing_across_it_carries_no_flux() {
        // The corollary the closed-face lookup rests on. Written as an
        // assertion about equal arguments rather than about a grid, because
        // that is the property: an antisymmetric function of two equal
        // arguments is zero.
        let alpha = Q::from_f64(1.0 / 6.0);
        for a in [-123_456i64, -1, 0, 1, 7, 123_456] {
            assert_eq!(
                flux_32(M32::new(a as i32), M32::new(a as i32), alpha),
                M32::ZERO
            );
            assert_eq!(flux_64(M64::new(a), M64::new(a), alpha), M64::ZERO);
        }
    }

    /// The kernel carries its own copy of the neighbourhood lookup, because
    /// `kernels/` may not depend on `world/`. `world/grid.rs` asks for exactly
    /// this test, and it lives here because a test may depend on everything.
    #[test]
    fn the_neighbourhood_agrees_with_the_grid() {
        let cases = [
            (TORUS, [Boundary::Periodic; 6]),
            (
                FLOORED,
                [
                    Boundary::Periodic,
                    Boundary::Periodic,
                    Boundary::Periodic,
                    Boundary::Periodic,
                    Boundary::Closed,
                    Boundary::Closed,
                ],
            ),
            (0, [Boundary::Closed; 6]),
        ];

        for (mask, boundary) in cases {
            let grid = Grid::new(NX, NY, NZ, boundary).unwrap();
            let p = params(mask, 1.0 / 6.0);
            assert_eq!(grid.n_voxels(), N_VOXELS);

            for idx in 0..N_VOXELS {
                let (x, y, z) = grid.coords(idx);
                assert_eq!(index(&p, x, y, z), grid.index(x, y, z));
                for face in Face::ALL {
                    assert_eq!(
                        neighbour(&p, idx, face as u32),
                        grid.neighbour(idx, face),
                        "voxel {idx}, face {face:?}, mask {mask:#08b}"
                    );
                }
            }
        }
    }

    /// A degenerate axis is its own neighbour in both directions even when
    /// periodic, so it carries no flux — the flat case a two-dimensional
    /// scenario would use.
    #[test]
    fn an_axis_one_voxel_deep_carries_nothing() {
        let p = DiffuseParams {
            nx: 4,
            ny: 4,
            nz: 1,
            periodic_mask: TORUS,
            alpha: Q::from_f64(1.0 / 6.0),
        };
        for idx in 0..16u32 {
            assert_eq!(neighbour(&p, idx, 4), idx);
            assert_eq!(neighbour(&p, idx, 5), idx);
        }
    }

    /// One substep over a whole lane, the way the host runs it.
    fn substep_32(src: &[M32], dst: &mut [M32], p: &DiffuseParams) {
        for idx in 0..N_VOXELS {
            diffuse_voxel_32(src, dst, p, idx);
        }
    }

    fn substep_64(src: &[M64], dst: &mut [M64], p: &DiffuseParams) {
        for idx in 0..N_VOXELS {
            diffuse_voxel_64(src, dst, p, idx);
        }
    }

    #[test]
    fn a_substep_conserves_the_total_exactly_in_both_widths() {
        // Exactly: not "to within a tolerance". Conservation here is a property
        // of the antisymmetry of the flux, so any drift at all means the
        // property is broken, not that the error is small.
        for mask in [TORUS, FLOORED, 0] {
            for alpha in [1.0 / 6.0, 0.125, 1.0e-4] {
                let p = params(mask, alpha);

                let src: Vec<M32> = (0..N_VOXELS).map(|i| M32::new(pattern(i) as i32)).collect();
                let mut dst = vec![M32::ZERO; N_VOXELS as usize];
                substep_32(&src, &mut dst, &p);
                assert_eq!(
                    total_32(&src),
                    total_32(&dst),
                    "mask {mask:#08b}, alpha {alpha}"
                );

                let src: Vec<M64> = (0..N_VOXELS).map(|i| M64::new(pattern(i))).collect();
                let mut dst = vec![M64::ZERO; N_VOXELS as usize];
                substep_64(&src, &mut dst, &p);
                assert_eq!(total_64(&src), total_64(&dst));
            }
        }
    }

    #[test]
    fn the_result_does_not_depend_on_the_traversal_order() {
        // Gather form buys this: nothing a voxel writes is visible to any other
        // voxel in the same substep. On the GPU there is no traversal order at
        // all, so anything this test would catch is unfixable there.
        let p = params(TORUS, 1.0 / 6.0);
        let src: Vec<M32> = (0..N_VOXELS).map(|i| M32::new(pattern(i) as i32)).collect();

        let mut forwards = vec![M32::ZERO; N_VOXELS as usize];
        for idx in 0..N_VOXELS {
            diffuse_voxel_32(&src, &mut forwards, &p, idx);
        }

        let mut backwards = vec![M32::ZERO; N_VOXELS as usize];
        for idx in (0..N_VOXELS).rev() {
            diffuse_voxel_32(&src, &mut backwards, &p, idx);
        }

        assert_eq!(forwards, backwards);
        assert_ne!(forwards, src);
    }

    #[test]
    fn a_uniform_field_is_a_fixed_point() {
        // Every face sees the same amount on both sides, so every flux is zero
        // by the corollary above. A scheme that lost a unit to rounding here
        // would erode a uniform field over a million ticks.
        let p = params(FLOORED, 1.0 / 6.0);
        let src = vec![M32::new(1_000_003); N_VOXELS as usize];
        let mut dst = vec![M32::ZERO; N_VOXELS as usize];
        substep_32(&src, &mut dst, &p);
        assert_eq!(src, dst);
    }

    #[test]
    fn a_starved_voxel_is_overdrawn_rather_than_clamped() {
        // The documented undershoot, as a fact rather than a warning. Three
        // units at the stability limit: every face rounds `-0.5` away from zero
        // to `-1`, six faces take six units out of a pool of three, and the
        // voxel lands on -3.
        //
        // The second assertion is why the first one is allowed to stand. The six
        // units did not come from nowhere — the neighbours have them — so the
        // total is untouched, and a clamp at zero here would be the thing that
        // broke conservation, not the thing that saved it. If this test ever
        // starts failing because a voxel came back non-negative, check the total
        // before celebrating.
        let p = params(TORUS, 1.0 / 6.0);
        let centre = index(&p, 1, 2, 2) as usize;
        let mut src = vec![M32::ZERO; N_VOXELS as usize];
        src[centre] = M32::new(3);

        let mut dst = vec![M32::ZERO; N_VOXELS as usize];
        substep_32(&src, &mut dst, &p);

        assert_eq!(dst[centre], M32::new(-3));
        for face in 0..6u32 {
            assert_eq!(
                dst[neighbour(&p, centre as u32, face) as usize],
                M32::new(1)
            );
        }
        assert_eq!(total_32(&src), total_32(&dst));
    }

    #[test]
    fn a_closed_domain_moves_nothing_across_its_walls() {
        // With every face closed, the corner voxels have three faces that fold
        // onto themselves. Nothing may leave, and the total is unchanged — the
        // two halves of the same statement.
        let p = params(0, 1.0 / 6.0);
        let src: Vec<M32> = (0..N_VOXELS).map(|i| M32::new(pattern(i) as i32)).collect();
        let mut dst = vec![M32::ZERO; N_VOXELS as usize];
        substep_32(&src, &mut dst, &p);
        assert_eq!(total_32(&src), total_32(&dst));
    }
}
