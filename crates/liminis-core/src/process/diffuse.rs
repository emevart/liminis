//! The diffusion process: substeps, folded parameters, and the dispatch loop.
//!
//! Everything the kernel is not allowed to know (`kernels/diffuse.rs`). The
//! kernel gets `alpha` as one number; this file is where `D`, `dt` and `dx` are
//! turned into that number and into the count of substeps it belongs to.

use anyhow::{Result, bail};

use super::{Conservation, Invariant};
use crate::kernels::{DiffuseParams, diffuse_voxel_32, diffuse_voxel_64};
use crate::numeric::Q;
use crate::world::{Boundary, Face, Field32, Field64, Grid};

/// The stability limit of the explicit 7-point stencil, as a bound on `alpha`.
///
/// SPEC section 4.3 writes the condition as `dt*D*6/dx^2 <= 1`, which is the
/// same statement: `alpha = D*dt/dx^2` must not exceed a sixth. Everything about
/// substeps below exists to keep this true after the tick has been divided.
const STABILITY_LIMIT: f64 = 1.0 / 6.0;

/// The most substeps one field may cost per tick (ADR-061).
///
/// A build-time bound and deliberately not a config key. One substep is one pass
/// over the field — read `src`, write `dst` — so on the 128^3 base grid of SPEC
/// section 1.1 it is 16 MiB of traffic; sixty-four of them are exactly one GiB
/// per tick for one field. `N_MAX = 64` reads as "a field may not cost more than
/// a gibibyte a tick", where a round hundred would mean nothing and would let
/// through the two cases the bound exists for: a proton at `dx = 25 um` (90
/// substeps) and the enthalpy field at the default `lod = 0` (84).
///
/// A key that raised it would have exactly one effect — turning a refusal into a
/// run — because `n` is derived from `D`, `dt` and `dx` and nothing else, and
/// the loader has no tick budget to judge a raised value against. So there is
/// none.
///
/// Here rather than beside `S_MAX` and `R_MAX` in `world/registry.rs`: those
/// size a kernel's local arrays, this one sizes nothing at all and exists
/// because the tick budget is not in the config. "Beside `STABILITY_LIMIT`" is
/// by kind, and this is the kind (ADR-061).
pub const N_MAX: u32 = 64;

/// Diffusion of one substance on one lane of one field.
///
/// Holds what a tick needs: how many substeps, and the parameters of one of
/// them. Both are derived once, at construction, from the scenario — never
/// per tick, and never by hand (ADR-030).
///
/// One substance per instance. That is not a simplification, it is what the
/// buffers currently allow: substances get different substep counts from the
/// same tick (six for the proton, two for oxygen, one for most of the
/// registry), so after a diffusion phase two lanes of a shared field would have
/// changed buffers a different number of times. `world::Field` records the
/// choice that has to be made before lanes can advance independently, as
/// `TODO(swap-granularity)`, and says what this file relies on instead: a
/// process that writes every lane and swaps once needs none of it.
#[derive(Clone, Copy, Debug)]
pub struct Diffuse {
    substeps: u32,
    params: DiffuseParams,
}

impl Diffuse {
    /// Derive the substeps and fold the parameters for one substance.
    ///
    /// `diffusivity` is `D` in m^2/s, declared per substance
    /// (`CONFIG_SCHEMA.md` section 5); `dt` is the tick in seconds and `dx` the
    /// voxel edge in metres, both from the scenario.
    ///
    /// # Errors
    ///
    /// Returns an error if `D`, `dt` or `dx` is not a usable number, or if the
    /// scenario needs more substeps than a `u32` can count — see
    /// [`substeps_for`]. Also refuses a grid with an `exchange` face, which
    /// cannot be built today for a reason of its own (`world::Grid::new`).
    pub fn new(grid: &Grid, diffusivity: f64, dt: f64, dx: f64) -> Result<Self> {
        let (substeps, alpha) = substeps_and_alpha(diffusivity, dt, dx)?;

        // The one place `D`, `dt` and `dx` are visible at once. Past this line
        // they exist only as `alpha`, and the kernel cannot recombine them in
        // the wrong order because it never sees them (`ARCHITECTURE.md`).
        let params = DiffuseParams {
            nx: grid.nx(),
            ny: grid.ny(),
            nz: grid.nz(),
            periodic_mask: periodic_mask(grid)?,
            alpha: Q::from_f64(alpha),
        };

        debug_assert!(
            params.alpha <= Q::from_f64(STABILITY_LIMIT),
            "alpha {alpha} is over the stability limit after {substeps} substeps"
        );

        Ok(Self { substeps, params })
    }

    /// How many applications of the kernel make up one tick (ADR-030).
    #[inline]
    #[must_use]
    pub fn substeps(&self) -> u32 {
        self.substeps
    }

    /// The folded parameters of one substep, as the kernel receives them.
    #[inline]
    #[must_use]
    pub fn params(&self) -> DiffuseParams {
        self.params
    }

    /// Diffusion conserves matter, and does not touch energy.
    ///
    /// The first half is the point of the whole scheme: a substep moves amounts
    /// between voxels through an antisymmetric flux, so the sum over the domain
    /// is unchanged exactly, and `n` substeps are `n` applications of the same
    /// construction (ADR-005, ADR-030).
    ///
    /// The second half is a statement about this process, not about physics: it
    /// writes one lane of one substance field and nothing else. Enthalpy is its
    /// own field with its own transport (ADR-028), so nothing here reaches the
    /// energy ledger.
    ///
    /// That the enthalpy of a diffusing substance does *not* follow it is
    /// decided — ADR-062, which closed the `TODO(enthalpy-of-transport)` that
    /// stood here — and the decision was paid for rather than declared. The
    /// discarded term is bounded, `Sum D_i*c_i*c_p,i / (alpha*Sum c_j*c_p,j)`,
    /// and the bound is checked at load in `config::derive`: over 5% the
    /// scenario is refused. So this half of the invariant rests on that check
    /// and no longer on the absence of a document, which is what ADR-028 calls
    /// being half inside the ledger. On the corpus registry the term is 1.64%,
    /// and all of it is the self-diffusion of water, which in a homogeneous
    /// solvent transports no enthalpy at all.
    // TODO(diffusing-matter-does-not-move-enthalpy): `ACCEPTANCE.md` names the
    // assertion, and it cannot be written here. This process takes one field and
    // an enthalpy field is not something it can be handed, so a test would
    // assert that a buffer nobody passed in did not change; the statement
    // belongs to the orchestration that holds both fields at once, and there is
    // no tick loop yet (`ARCHITECTURE.md`, the host side).
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::Conserved,
        }
    }
}

/// Generates the per-width dispatch loop.
///
/// One text for both storage widths, for the reason ADR-040 gives on the kernel
/// side and `NUMERIC.md` section 5 gives for shaders: two hand-written copies
/// drift apart, and here they would drift in the substep loop, which is the one
/// place a difference would look like physics.
macro_rules! define_apply {
    ($name:ident, $field:ty, $voxel:ident) => {
        #[doc = concat!("Run one tick of diffusion over a `", stringify!($field), "`.")]
        ///
        /// The field must hold exactly one lane: this process advances both
        /// buffers of the whole field on every substep, and a lane it did not
        /// write would come back two steps stale rather than unchanged
        /// (`world::Field`).
        ///
        /// The loop is the shape of every transport phase and the part that does
        /// not survive the port: on the GPU the inner loop becomes a dispatch,
        /// the swap becomes a rebinding, and `diffuse_voxel` becomes WGSL.
        ///
        /// # Panics
        ///
        /// Panics if the field does not match the grid the process was folded
        /// against — a wrong lane count or a wrong voxel count. Both are
        /// programming errors rather than bad scenarios, and both would
        /// otherwise show up as a quietly wrong world.
        pub fn $name(&self, field: &mut $field) {
            assert_eq!(
                field.lanes(),
                1,
                "Diffuse advances the whole field per substep, so it takes a \
                 single-lane field (see TODO(swap-granularity) on world::Field)"
            );
            let n_voxels = field.n_voxels();
            assert_eq!(
                n_voxels,
                self.params.nx * self.params.ny * self.params.nz,
                "the field does not have the shape this process was folded for"
            );

            for _ in 0..self.substeps {
                let (src, dst) = field.lane_pair_mut(0);
                // Every voxel, in one pass, reading state N and writing state
                // N+1. Nothing else may go in this loop: a second pass over the
                // same buffers would be a kernel reading what it wrote.
                for idx in 0..n_voxels {
                    $voxel(src, dst, &self.params, idx);
                }
                field.swap();
            }
        }
    };
}

impl Diffuse {
    define_apply!(apply_32, Field32, diffuse_voxel_32);
    define_apply!(apply_64, Field64, diffuse_voxel_64);
}

/// How many explicit substeps one tick of diffusion needs:
/// `n = ceil(6*D*dt/dx^2)` (ADR-030, SPEC sections 1.7 and 4.3).
///
/// Derived, never declared. The registry of SPEC section 1.7 comes out of this
/// formula and not out of a table: at `dx = 100 um` and a one-second tick the
/// proton needs six substeps because of the Grotthuss mechanism, oxygen and CO2
/// need two, and the rest of the registry passes in one. A substance added by a
/// scenario through TOML (ADR-018) gets its own count the same way, which is the
/// point — a hand-written list of fast fields works today and breaks on the
/// first substance someone else adds.
///
/// A substep of a flux scheme conserves matter exactly, so dividing the tick is
/// `n` applications of the same construction rather than a different scheme
/// (ADR-005).
///
/// `D = 0` is allowed and gives one substep with `alpha = 0`: a substance that
/// does not diffuse is a no-op, and refusing it would be a policy no document
/// states.
///
/// # Errors
///
/// Returns an error if `D` is negative or not finite, if `dt` or `dx` is not
/// finite and positive, or if the count is over [`N_MAX`].
pub fn substeps_for(diffusivity: f64, dt: f64, dx: f64) -> Result<u32> {
    Ok(substeps_and_alpha(diffusivity, dt, dx)?.0)
}

/// The substep count and the `alpha` that belongs to it.
///
/// One function because the two must agree: `alpha` is the coefficient of a
/// substep, `D*dt/(n*dx^2)`, and computing it anywhere else is how the two come
/// apart. Written as `ratio / (6n)` so that `alpha <= 1/6` follows from
/// `n >= ratio` in the same arithmetic that produced `n`, rather than from a
/// separate calculation that could round the other way.
///
/// Public because the loader needs both halves for a field it is not building a
/// [`Diffuse`] for yet (`config/derive.rs`): taking only `n` from here and
/// recomputing `alpha` there is exactly the second source of truth this function
/// exists to prevent.
///
/// # Errors
///
/// The errors of [`substeps_for`], of which the substep bound is the one a
/// scenario meets.
pub fn substeps_and_alpha(diffusivity: f64, dt: f64, dx: f64) -> Result<(u32, f64)> {
    if !diffusivity.is_finite() || diffusivity < 0.0 {
        bail!("diffusivity {diffusivity} m^2/s is not a usable coefficient");
    }
    if !dt.is_finite() || dt <= 0.0 {
        bail!("dt {dt} s is not a usable timestep");
    }
    if !dx.is_finite() || dx <= 0.0 {
        bail!("dx {dx} m is not a usable voxel edge");
    }

    // 6*D*dt/dx^2: the number of substeps before rounding up, and the left-hand
    // side of the stability condition of SPEC section 4.3 at one substep.
    let ratio = 6.0 * diffusivity * dt / (dx * dx);
    if !ratio.is_finite() {
        bail!(
            "6*D*dt/dx^2 overflowed at D = {diffusivity} m^2/s, dt = {dt} s, \
             dx = {dx} m"
        );
    }

    // A field that needs more than `N_MAX` substeps has to coarsen its LOD or be
    // solved to steady state, and this is the one place where either can be
    // asked for (ADR-030, ADR-061). The count is printed before the cast: at
    // dx = 1 um a proton needs 55 801 substeps and the micro regime is meant to
    // meet this refusal rather than to run.
    let substeps = ratio.ceil().max(1.0);
    if substeps > f64::from(N_MAX) {
        // Coarsening divides `n` by `4^dlod`, so the message can name the one
        // exit that applies instead of listing both. The lod range is 0..=2
        // (`QUANTITIES.md` section 1), so past `64*4^2 = 1024` only the solver
        // is left — and the solver is not written: S0 has none and ADR-030 says
        // it should not. Promising a stage is honest, promising a solver is not.
        let dlod = (substeps / f64::from(N_MAX)).log2() / 2.0;
        let dlod = dlod.ceil();
        let fix = if dlod <= 2.0 {
            format!("coarsen its lod by {dlod} (each step divides n by four)")
        } else {
            format!(
                "coarsening would need {dlod} steps of lod against a range of \
                 0..=2, so this field has to wait for the steady-state solver of \
                 stage S1'"
            )
        };
        bail!(
            "diffusion needs {substeps} substeps per tick at D = {diffusivity} \
             m^2/s, dt = {dt} s, dx = {dx} m, over the bound of N_MAX = {N_MAX} \
             substeps: {fix} (ADR-030, ADR-061)"
        );
    }
    debug_assert!(
        substeps <= f64::from(u32::MAX),
        "the N_MAX bound above refuses long before a u32 runs out"
    );

    let alpha = ratio / (6.0 * substeps);
    debug_assert!(
        alpha <= STABILITY_LIMIT,
        "alpha {alpha} over the stability limit after {substeps} substeps"
    );

    // Exact: `substeps` came from `ceil` and was just bounded by u32::MAX.
    Ok((substeps as u32, alpha))
}

/// The six boundary conditions of the grid, as the bit mask the kernel reads.
///
/// Bit `f` is set when face `f` is periodic. A closed face contributes no bit
/// and the kernel's lookup returns the voxel itself, which carries no flux
/// (`kernels::DiffuseParams`).
fn periodic_mask(grid: &Grid) -> Result<u32> {
    let mut mask = 0u32;
    for face in Face::ALL {
        match grid.boundary(face) {
            Boundary::Periodic => mask |= 1 << (face as u32),
            Boundary::Closed => {}
            // Unreachable today: `Grid::new` refuses an exchange face until the
            // channel counters exist. Refused again rather than folded into one
            // of the other two, because the closest neighbour of "exchange" is
            // "closed", and quietly sealing a face that is supposed to vent is
            // exactly the error the refusal upstream exists to prevent.
            Boundary::Exchange => bail!(
                "face {face:?} is an exchange face: matter crossing it belongs \
                 in the BOUNDARY_EXCHANGE channel (SPEC section 7), and there \
                 are no channel counters yet"
            ),
        }
    }
    Ok(mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernels::flux_32;
    use crate::numeric::{M32, M64};
    use crate::world::Field;

    /// The eco regime of SPEC section 1.7: a one-second tick and a 100 um voxel.
    const DT: f64 = 1.0;
    const DX: f64 = 1.0e-4;

    fn torus(nx: u32, ny: u32, nz: u32) -> Grid {
        Grid::new(nx, ny, nz, [Boundary::Periodic; 6]).unwrap()
    }

    /// Periodic in X and Y, solid floor and lid in Z — the eco-regime default of
    /// SPEC section 1.6, minus the face that cannot be built yet.
    fn floored(nx: u32, ny: u32, nz: u32) -> Grid {
        Grid::new(
            nx,
            ny,
            nz,
            [
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Closed,
                Boundary::Closed,
            ],
        )
        .unwrap()
    }

    fn total_32(field: &Field32) -> i64 {
        field.read().iter().map(|v| v.to_i64()).sum()
    }

    fn total_64(field: &Field64) -> i64 {
        field.read().iter().map(|v| v.to_i64()).sum()
    }

    /// Put a state into a field's read buffer.
    fn seed_32(field: &mut Field32, amounts: impl Fn(u32) -> i32) {
        let n_voxels = field.n_voxels();
        let buffer = field.write_mut();
        for idx in 0..n_voxels {
            buffer[idx as usize] = M32::new(amounts(idx));
        }
        field.swap();
    }

    #[test]
    fn the_substep_count_reproduces_the_registry_of_spec_1_7() {
        // The table of SPEC section 1.7, recomputed rather than copied. If the
        // formula ever stops being `ceil(6*D*dt/dx^2)`, these move.
        let cases = [
            (9.3e-9, 6u32), // H+, the Grotthuss mechanism and the worst case
            (2.1e-9, 2),    // O2
            (1.9e-9, 2),    // CO2
            (1.6e-9, 1),    // H2S, CH4
            (0.8e-9, 1),    // PO4(3-), Fe(2+)
        ];
        for (diffusivity, expected) in cases {
            assert_eq!(
                substeps_for(diffusivity, DT, DX).unwrap(),
                expected,
                "D = {diffusivity} m^2/s"
            );
        }
    }

    #[test]
    fn coarsening_the_grid_divides_the_substeps_quadratically() {
        // ADR-030's main lever: 128^3 -> 32^3 is four times the voxel edge and a
        // sixteenth of the substeps. Stated on the proton, where it matters.
        let fine = substeps_for(9.3e-9, DT, DX).unwrap();
        let coarse = substeps_for(9.3e-9, DT, DX * 4.0).unwrap();
        assert_eq!(fine, 6);
        assert_eq!(coarse, 1);
        assert!(f64::from(fine) / 16.0 <= f64::from(coarse));
    }

    #[test]
    fn alpha_stays_under_the_stability_limit() {
        // The reason substeps exist at all. Swept over five orders of magnitude
        // of D — from a tenth of the slowest thing in the registry to the top of
        // what `N_MAX` admits at this dx — including the values that make the
        // ratio land on an integer, where `ceil` gives back what it was given
        // and `alpha` sits exactly on the limit rather than under it.
        //
        // The sweep stops at 1e-7 because that is where it has to: at dx = 100 um
        // and a one-second tick, D above `N_MAX*dx^2/(6*dt) = 1.07e-7` is a
        // refusal rather than a longer loop (ADR-061), and that case belongs to
        // `field_over_n_max_substeps_is_rejected` below.
        for exponent in -12..-7 {
            for multiplier in [1.0, 1.5, 2.0, 3.0, 6.0, 9.99] {
                let diffusivity = multiplier * 10f64.powi(exponent);
                let (substeps, alpha) = substeps_and_alpha(diffusivity, DT, DX).unwrap();
                assert!(substeps >= 1);
                assert!(
                    alpha <= STABILITY_LIMIT,
                    "alpha {alpha} at D = {diffusivity} over {substeps} substeps"
                );
                // And the division was not more generous than it had to be:
                // `n` is the smallest count that stays inside the limit, so one
                // substep fewer does not.
                //
                // `>=` rather than `>`, because the recomputation can run out of
                // mantissa: where 6*D*dt/dx^2 lands a hair over an integer, `n`
                // is that integer plus one and dividing back lands on the
                // nearest f64 to a sixth. The inequality is strict in exact
                // arithmetic and this is what is left of it in f64 — tightening
                // the assertion would only be pinning the rounding.
                if substeps > 1 {
                    let coarser = alpha * f64::from(substeps) / f64::from(substeps - 1);
                    assert!(
                        coarser >= STABILITY_LIMIT,
                        "{substeps} substeps at D = {diffusivity} is one too many"
                    );
                }
            }
        }
    }

    #[test]
    fn a_scenario_that_cannot_be_stepped_is_refused() {
        assert!(substeps_for(-1.0e-9, DT, DX).is_err());
        assert!(substeps_for(f64::NAN, DT, DX).is_err());
        assert!(substeps_for(1.6e-9, 0.0, DX).is_err());
        assert!(substeps_for(1.6e-9, -1.0, DX).is_err());
        assert!(substeps_for(1.6e-9, DT, 0.0).is_err());
        assert!(substeps_for(1.6e-9, DT, f64::INFINITY).is_err());

        // A substance that does not diffuse is a no-op, not an error.
        let (substeps, alpha) = substeps_and_alpha(0.0, DT, DX).unwrap();
        assert_eq!(substeps, 1);
        assert_eq!(alpha, 0.0);
    }

    /// `ACCEPTANCE.md`, from ADR-061.
    #[test]
    fn field_over_n_max_substeps_is_rejected() {
        // The boundary itself, and both sides of it. At dx = 100 um and a
        // one-second tick, N_MAX admits everything up to D = 1.07e-7 m^2/s —
        // which is where the enthalpy field of ADR-062 sits at lod 2, and it
        // passes at 1.4e-7 only because its own grid step is four times coarser.
        let limit = f64::from(N_MAX) * DX * DX / (6.0 * DT);
        assert_eq!(substeps_for(limit, DT, DX).unwrap(), N_MAX);
        assert!(substeps_for(limit * 1.01, DT, DX).is_err());

        // The micro regime, which is meant to meet this refusal rather than run
        // (SPEC section 1.7, ADR-030): the proton at dx = 1 um needs 55 801
        // substeps, and until ADR-061 that config loaded in silence and simply
        // did not finish its arithmetic.
        let err = substeps_for(9.3e-9, DT, 1.0e-6).unwrap_err().to_string();
        assert!(err.contains("55801"), "the count has to be named: {err}");
        assert!(err.contains("64"), "the bound has to be named: {err}");
        assert!(err.contains("substeps"), "{err}");

        // And the representability bound it replaced: 64 refuses long before a
        // u32 runs out, so that message is gone and this one arrives instead.
        let err = substeps_for(1.0, DT, 1.0e-9).unwrap_err().to_string();
        assert!(err.contains("substeps"), "unhelpful message: {err}");
        assert!(
            !err.contains("u32"),
            "the u32 bound is unreachable now: {err}"
        );
    }

    /// `ACCEPTANCE.md`, from ADR-061. The refusal has to say which of the two
    /// exits of ADR-030 applies, not list both.
    #[test]
    fn n_max_refusal_names_the_field_and_the_lod_that_fixes_it() {
        // 84 substeps — the enthalpy field of ADR-062 on the fine grid — is
        // `ceil(log4(84/64)) = 1` step of coarsening away from loading. The
        // field's own name is added by the loader, which is the only place that
        // knows it (`config/derive.rs`).
        let err = substeps_for(1.4e-7, DT, DX).unwrap_err().to_string();
        assert!(err.contains("84"), "{err}");
        assert!(err.contains("lod by 1"), "the one exit that applies: {err}");

        // Past 64*4^2 = 1024 no lod in the range 0..=2 is enough, and the
        // message says so rather than promising a solver that S0 does not have.
        let err = substeps_for(9.3e-9, DT, 1.0e-6).unwrap_err().to_string();
        assert!(err.contains("S1'"), "{err}");
        assert!(!err.contains("lod by "), "{err}");
    }

    /// `ACCEPTANCE.md`, section "Conservation" — the name is fixed there.
    ///
    /// Exact equality, over a whole tick's worth of substeps, on both storage
    /// widths and on both boundary regimes that can be built. Conservation here
    /// is a property of the flux function, so any drift at all means the
    /// property is broken rather than that the error is small.
    #[test]
    fn diffusion_alone_conserves_exactly() {
        for grid in [torus(5, 6, 7), floored(5, 6, 7)] {
            // The proton: six substeps, the worst case of the registry, so the
            // rounding gets six chances per tick to lose a unit.
            let diffuse = Diffuse::new(&grid, 9.3e-9, DT, DX).unwrap();
            assert_eq!(diffuse.substeps(), 6);
            assert_eq!(
                diffuse.invariant(),
                Invariant {
                    matter: Conservation::Conserved,
                    energy: Conservation::Conserved,
                }
            );

            let mut field: Field32 = Field::new(&grid, 1).unwrap();
            seed_32(&mut field, |idx| (idx as i32 * 7919) % 100_000);
            let before = total_32(&field);

            for _ in 0..8 {
                diffuse.apply_32(&mut field);
                assert_eq!(total_32(&field), before, "a tick moved the total");
            }

            // And the run did something: a conserving no-op would also pass the
            // assertion above.
            assert!(field.read().iter().any(|&v| v != M32::new(0)));
            assert_ne!(
                field.read()[0].to_i64(),
                0,
                "nothing reached the first voxel"
            );
        }
    }

    /// `ACCEPTANCE.md` names `transport_of_a_64_bit_substance_conserves_exactly`
    /// for the width ADR-040 introduced. This is the diffusive half of it: the
    /// same text, instantiated on `i64`, carrying a pool no `i32` could hold.
    #[test]
    fn transport_of_a_64_bit_substance_conserves_exactly() {
        let grid = torus(4, 5, 6);
        let diffuse = Diffuse::new(&grid, 2.1e-9, DT, DX).unwrap();

        let mut field: Field64 = Field::new(&grid, 1).unwrap();
        {
            let n_voxels = field.n_voxels();
            let buffer = field.write_mut();
            for idx in 0..n_voxels {
                // Water, at the scale that made ADR-040 necessary.
                buffer[idx as usize] = M64::new(5_100_000_000_000 + i64::from(idx) * 977);
            }
        }
        field.swap();

        let before = total_64(&field);
        for _ in 0..4 {
            diffuse.apply_64(&mut field);
            assert_eq!(total_64(&field), before);
        }
    }

    /// The smoke test: does the thing actually diffuse.
    ///
    /// A point source spreads, the peak comes down, the support grows, and the
    /// total does not move by a single unit. Everything else in this file tests
    /// a property; this one tests that the kernel does its job at all.
    ///
    /// Phosphate, the slow end of the registry of SPEC section 1.7, and the
    /// choice is not decoration. At `alpha` above a seventh the peak of a point
    /// source stops falling monotonically: after one substep the centre holds
    /// `S*(1 - 6*alpha)` and each neighbour `S*alpha`, so the centre stops being
    /// the maximum once `1 - 6*alpha < alpha`, and the profile rings between the
    /// two sublattices for a while. That is the explicit scheme behaving as
    /// explicit schemes do near their stability limit, not a defect of this
    /// kernel — conservation is exact throughout it, which is what the other
    /// tests here are for. It just makes a bad smoke test, because "spreading"
    /// stops being visible in the height of the peak.
    #[test]
    fn a_point_source_spreads_without_changing_the_total() {
        let grid = torus(9, 9, 9);
        let diffuse = Diffuse::new(&grid, 0.8e-9, DT, DX).unwrap();
        assert_eq!(diffuse.substeps(), 1);

        let centre = grid.index(4, 4, 4) as usize;
        let source = 1_000_000_000i32;
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_32(
            &mut field,
            |idx| {
                if idx as usize == centre { source } else { 0 }
            },
        );

        let occupied = |field: &Field32| field.read().iter().filter(|&&v| v != M32::ZERO).count();
        assert_eq!(occupied(&field), 1);

        // One tick, and the whole stencil in three assertions: each of the six
        // faces carries the same amount out of the peak, each neighbour receives
        // exactly one face's worth, and nothing else in the domain has changed.
        //
        // The amount is 8e7 units — `alpha` is 0.08 and the source a billion,
        // and the product lands on a multiple of eight because an `f32` has no
        // bits left for anything finer up there. That coarseness costs nothing
        // that matters: it is `Q`, the approximate half of the boundary, and
        // conservation does not depend on its precision. It depends on the sign
        // symmetry of the rounding, which is exact at every magnitude.
        let per_face = flux_32(M32::ZERO, M32::new(source), diffuse.params().alpha);
        assert_eq!(per_face, M32::new(80_000_000));

        diffuse.apply_32(&mut field);
        assert_eq!(total_32(&field), i64::from(source));
        assert_eq!(
            field.read()[centre],
            M32::new(source) - per_face - per_face - per_face - per_face - per_face - per_face
        );
        for face in Face::ALL {
            let neighbour = grid.neighbour(centre as u32, face) as usize;
            assert_eq!(field.read()[neighbour], per_face);
        }
        assert_eq!(occupied(&field), 7);

        // And it keeps going: the peak falls, the support grows, the total is
        // the same integer it started as.
        let mut peak = field.read()[centre].to_i64();
        let mut support = occupied(&field);
        for tick in 0..6 {
            diffuse.apply_32(&mut field);

            let next_peak = field.read()[centre].to_i64();
            let next_support = occupied(&field);
            assert_eq!(total_32(&field), i64::from(source), "tick {tick}");
            assert!(next_peak < peak, "the peak did not fall at tick {tick}");
            assert!(next_support > support, "nothing new was reached at {tick}");
            assert!(
                field.read().iter().all(|&v| v <= M32::new(source)),
                "a voxel got more than the whole source at tick {tick}"
            );

            peak = next_peak;
            support = next_support;
        }

        // Seven ticks on a nine-voxel torus: spread wide, nowhere near uniform.
        assert!(support < grid.n_voxels() as usize);
    }

    #[test]
    fn a_uniform_field_survives_a_tick_unchanged() {
        // Nothing to diffuse: every face sees the same amount on both sides.
        // The interesting half is the floored grid, where the walls are also
        // asked to do nothing.
        let grid = floored(4, 4, 4);
        let diffuse = Diffuse::new(&grid, 9.3e-9, DT, DX).unwrap();
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_32(&mut field, |_| 1_000_003);

        let before = field.read().to_vec();
        diffuse.apply_32(&mut field);
        assert_eq!(field.read(), before.as_slice());
    }

    #[test]
    fn a_closed_domain_keeps_everything_it_started_with() {
        // Every face closed: a sealed box. Nothing may cross the walls, and the
        // corner voxels — three walls each — are where a lookup that fell off
        // the grid would show up.
        let grid = Grid::new(3, 4, 5, [Boundary::Closed; 6]).unwrap();
        let diffuse = Diffuse::new(&grid, 9.3e-9, DT, DX).unwrap();
        assert_eq!(diffuse.params().periodic_mask, 0);

        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_32(&mut field, |idx| if idx == 0 { 1_000_000 } else { 0 });
        let before = total_32(&field);

        for _ in 0..10 {
            diffuse.apply_32(&mut field);
            assert_eq!(total_32(&field), before);
        }
        // The corner is a corner: it kept more than the middle of the box.
        assert!(field.read()[0].to_i64() > field.read()[grid.index(1, 2, 2) as usize].to_i64());
    }

    #[test]
    fn the_boundary_mask_says_what_the_grid_says() {
        assert_eq!(periodic_mask(&torus(3, 3, 3)).unwrap(), 0b11_1111);
        assert_eq!(periodic_mask(&floored(3, 3, 3)).unwrap(), 0b00_1111);
        assert_eq!(
            periodic_mask(&Grid::new(3, 3, 3, [Boundary::Closed; 6]).unwrap()).unwrap(),
            0
        );
    }

    #[test]
    #[should_panic(expected = "single-lane")]
    fn a_multi_lane_field_is_refused() {
        let grid = torus(3, 3, 3);
        let diffuse = Diffuse::new(&grid, 1.6e-9, DT, DX).unwrap();
        let mut field: Field32 = Field::new(&grid, 2).unwrap();
        diffuse.apply_32(&mut field);
    }

    #[test]
    #[should_panic(expected = "shape")]
    fn a_field_of_the_wrong_shape_is_refused() {
        let diffuse = Diffuse::new(&torus(3, 3, 3), 1.6e-9, DT, DX).unwrap();
        let mut field: Field32 = Field::new(&torus(4, 4, 4), 1).unwrap();
        diffuse.apply_32(&mut field);
    }
}
