//! The settling process: the Stokes velocity of a grain, folded into one signed
//! Courant number, and the lanes that are not dispatched at all.
//!
//! Everything `kernels/settle.rs` is not allowed to know. The kernel gets one
//! signed scalar; this file is where the radius, the two densities, the
//! viscosity, `g`, `dt` and `dx` become that scalar, and where a lane that does
//! not settle is dropped from the dispatch entirely.
//!
//! # The chain, and the two places it goes wrong quietly
//!
//! ```text
//! k    = 2*r^2/9                       [`settling_coefficient`]
//! rho  = molar_mass*1e-3/V_bar         [`grain_density`]
//! w    = k*(rho - rho_medium)*g/mu     [`settling_velocity`]   positive DOWN
//! c    = -(w*dt/dx)                    [`settling_courant`]    positive to +Z
//! ```
//!
//! `k = r^2/18` is the diameter form with a radius inside it and gives a
//! velocity exactly four times too small; a factor of four is quieter than a
//! factor of a thousand and rides out the calibration of the sedimentation rate
//! just as invisibly (ADR-067, and ADR-055 before it).
//!
//! The `1e-3` in the density is the conversion from grams, and it admits no
//! other unit (ADR-067). Read `molar_mass` in kg/mol and the error is a factor
//! of a thousand: one way round the Courant refusal is loud, the other way round
//! settling simply never happens, which is indistinguishable from an honest
//! `settling_radius = 0`.
//!
//! The negation in the last line is the whole of the sign convention and it is
//! silent. Invert it and mineral floats out through the lid while the floor
//! stays clean, with conservation, the per-face agreement, the undershoot bound
//! and both storage widths all green.
//!
//! # Its own condition, over the full step
//!
//! Settling is step `f` of the tick order, a separate operator, and under
//! Lie-Trotter splitting every operator is applied over the full step and gets
//! by on its own stability condition (ADR-036, ADR-067). So the condition here
//! is `|w|*dt/dx <= 1` — not a share of advection's six-face budget `dx/(6*dt)`,
//! and not a reason for substeps, which ADR-067 rejects by name: ADR-030 counts
//! substeps out of the *parabolic* condition, and `N_MAX = 64` stays the bound
//! of one diffusion.
//!
//! Transport runs along one axis, so a voxel has exactly one outgoing face, the
//! sum over outgoing faces in SPEC section 4.2 degenerates to a single term, and
//! the two inequalities of that section coincide.
//!
//! # What is not here
//!
//! Hindered settling. `rho_medium` is a declared constant of the medium rather
//! than the mixture density of a voxel, so a grain sinks through settled
//! sediment at the same speed as through clear water and matter piles up in the
//! bottom voxel until it meets `max_conc`. ADR-067 named that price and paid it;
//! `matter_accumulates_and_the_sediment_does_not_stop_itself` is where it is
//! written down as behaviour rather than as a footnote.
//!
//! Enthalpy. Settling does not carry it, and in that it is not an exception: no
//! transport process in the project does (ADR-062, ADR-067).
//!
//! `every_n_ticks`. Running this operator once every `n` ticks would multiply
//! the effective step and falsify a condition already checked at load. ADR-005
//! and ADR-030 forbid it for a diffusive field; whether the ban reaches a
//! hyperbolic operator is settled nowhere, and `kernels/settle.rs` says so in
//! its own words.

use anyhow::{Result, bail};

use super::{Conservation, Invariant};
use crate::kernels::settle::{SettleParams, settle_voxel_32, settle_voxel_64};
use crate::numeric::{Q, qsub};
use crate::world::{Boundary, Direction, Face, Field32, Field64, Grid, ParitySplit, Width};

/// The constants of the medium a grain settles through.
///
/// **Declared constants, not per-voxel state** (ADR-067). Making `rho_medium`
/// the mixture density of a voxel would give compaction for free — settled
/// sediment would stop itself — and was rejected at a price that is counted:
/// `w` becomes a per-voxel quantity, the kernel needs an input slice beyond its
/// `Copy` struct of scalars (ADR-034), and, decisively, `V_occ` has no lower
/// bound once negative partial molar volumes are legal, so `sum(m)/V_occ` has a
/// pole and "the velocity is known at load time" stops being true by
/// construction.
///
/// None of the three is reachable from a scenario today. `mu` is a declared key;
/// `rho_medium` has no ASCII name yet (`CONFIG_SCHEMA.md` section 7 prints
/// "TODO"), and `g` is assigned neither a value nor a place by any document —
/// ADR-069 says only that it does not become a key. `config/validate.rs`
/// therefore refuses a settling substance outright rather than invent either,
/// and this struct is how a caller who *has* both supplies them.
/// The default of `enabled` for settling, and it is `false` (ADR-065).
///
/// Two numbers of [`Medium`] are named by no document — `rho_medium` has no
/// ASCII key and `g` is assigned neither a value nor a place — so
/// `config/validate.rs` refuses a substance with `settling_radius > 0` outright
/// rather than invent them. A default of `true` would therefore enable a step
/// that either has nothing to settle (every radius is zero, by that refusal) or
/// cannot be built at all. Off is the honest reading of that pair, and the whole
/// of the argument.
///
/// The mirror argument: sediment that never settles under an entirely green test
/// suite is the failure `Substance::settling_radius` refuses a default for, and
/// the day `g` and `rho_medium` are declared, `true` is the reading that matches
/// it.
// TODO(CONFIG_SCHEMA.md section 13 item 23): assigned by no record, and settled
// together with the two numbers of `Medium`.
pub const ENABLED_BY_DEFAULT: bool = false;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Medium {
    /// Density of the medium, kg/m^3.
    pub rho_medium: f64,
    /// Gravitational acceleration, m/s^2.
    pub g: f64,
    /// Dynamic viscosity, Pa*s.
    pub mu: f64,
}

/// The declaration of one settling substance, taken **per lane**.
///
/// Per lane and never per substance: water is first in the registry of SPEC
/// section 2.3 and takes lane 0, so a substance-indexed array is right at
/// `s == 0` and off by one from there on — a set of lanes of exactly the right
/// size gets used, and it is the wrong set (ADR-056, ADR-057). Resolve with
/// `Registry::lane_of` before building one of these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    /// The **radius** of the grain, metres. Not the diameter — see
    /// [`settling_coefficient`].
    pub settling_radius: f64,
    /// Molar mass in **grams** per mole. The unit is not free: the `1e-3` in
    /// [`grain_density`] is the conversion from grams and admits no other
    /// (ADR-067).
    pub molar_mass: f64,
    /// Partial molar volume, m^3/mol. May be zero or negative in general —
    /// electrostriction makes it negative for three substances of the registry —
    /// but not zero for a grain that settles, because the density is derived by
    /// dividing by it.
    pub partial_molar_volume: f64,
}

/// The settling coefficient `k = 2*r^2/9`, in m^2.
///
/// **Derived from the radius, never declared.** ADR-067 rejected a `k` key
/// because `2.2e-11` has no readable preimage: a wrong one is indistinguishable
/// from a right one, is absorbed by the calibration of the sedimentation rate,
/// and is never found — the direct moral of ADR-055.
///
/// The number comes from balancing Stokes drag against the weight in the fluid:
/// `6*pi*mu*r*w = (4/3)*pi*r^3*d_rho*g`, so `w = 2*r^2*d_rho*g/(9*mu)`. Through
/// the diameter the same coefficient is `d^2/18`. Written as `r^2/18` — the
/// diameter form with a radius inside it — it gives a velocity exactly four
/// times too small.
///
/// `r = 0` is not a magic value: `2*0^2/9` is an exact zero, so the kernel needs
/// no branch and the host simply builds no process for it.
#[inline]
#[must_use]
pub fn settling_coefficient(settling_radius: f64) -> f64 {
    2.0 * settling_radius * settling_radius / 9.0
}

/// The density of the grain itself, `rho = molar_mass*1e-3/V_bar`, in kg/m^3.
///
/// The `1e-3` is the conversion from **grams** per mole and admits no other unit
/// (ADR-067). It is also the only place in the model where moles become
/// kilograms: `V_occ` lost its copy when the declared key became the partial
/// molar volume, which narrowed the blast radius of a wrong unit from "pressure,
/// settling and buoyancy" down to `w` alone — where the Courant condition
/// catches a factor of a thousand with a loud refusal instead of a quiet
/// calibration.
///
/// # Errors
///
/// Returns an error if either input is not finite, if `partial_molar_volume` is
/// zero — the conditional domain rule of ADR-067: legal in general, and an error
/// exactly where the division happens — or if the quotient overflows.
pub fn grain_density(molar_mass_g_per_mol: f64, partial_molar_volume: f64) -> Result<f64> {
    if !molar_mass_g_per_mol.is_finite() || molar_mass_g_per_mol <= 0.0 {
        bail!("molar_mass {molar_mass_g_per_mol} g/mol is not a usable molar mass");
    }
    if !partial_molar_volume.is_finite() {
        bail!("partial_molar_volume {partial_molar_volume} m^3/mol is not finite");
    }
    if partial_molar_volume == 0.0 {
        bail!(
            "a substance that settles declares partial_molar_volume = 0 m^3/mol: \
             the grain density is derived by division, \
             rho_bar = molar_mass*1e-3/partial_molar_volume, and does not exist \
             at zero (ADR-067). Zero stays legal for a substance that does not \
             settle — V_bar(H+) is exactly zero on the conventional scale"
        );
    }

    let rho = molar_mass_g_per_mol * 1.0e-3 / partial_molar_volume;
    if !rho.is_finite() {
        bail!(
            "the grain density overflowed at molar_mass = {molar_mass_g_per_mol} \
             g/mol and partial_molar_volume = {partial_molar_volume} m^3/mol"
        );
    }
    Ok(rho)
}

/// The Stokes settling velocity `w = k*(rho_bar - rho_medium)*g/mu`, in m/s,
/// **positive downward**.
///
/// Positive downward is the host's convention and it is the physical one: a
/// grain denser than the medium sinks. The kernel's convention is the opposite —
/// positive toward `+Z` — and exactly one negation lies between them, in
/// [`settling_courant`].
///
/// A grain with `settling_radius = 0` gets `w = 0` without the density being
/// derived at all. That is not a shortcut: ADR-067 keeps `V_bar = 0` legal for a
/// substance that does not settle (the proton has exactly that), and deriving
/// `rho_bar` for it would divide by zero on a perfectly valid declaration.
///
/// # Errors
///
/// Returns an error if the medium is not usable — a non-positive viscosity, a
/// non-finite `g` or `rho_medium` — if the radius is negative or not finite, or
/// if the derivation of the grain density fails.
pub fn settling_velocity(grain: &Grain, medium: &Medium) -> Result<f64> {
    if !medium.mu.is_finite() || medium.mu <= 0.0 {
        bail!("mu {} Pa*s is not a usable viscosity", medium.mu);
    }
    if !medium.g.is_finite() {
        bail!("g {} m/s^2 is not a usable acceleration", medium.g);
    }
    if !medium.rho_medium.is_finite() {
        bail!(
            "the medium density {} kg/m^3 is not a usable density",
            medium.rho_medium
        );
    }
    if !grain.settling_radius.is_finite() || grain.settling_radius < 0.0 {
        bail!(
            "settling_radius {} m is not a usable radius",
            grain.settling_radius
        );
    }

    // The one branch, and it is on the radius rather than on the velocity,
    // because it exists to avoid a division that has no answer rather than to
    // save the arithmetic.
    if grain.settling_radius == 0.0 {
        return Ok(0.0);
    }

    let rho_bar = grain_density(grain.molar_mass, grain.partial_molar_volume)?;
    let w = settling_coefficient(grain.settling_radius) * (rho_bar - medium.rho_medium) * medium.g
        / medium.mu;
    if !w.is_finite() {
        bail!(
            "the settling velocity overflowed at r = {} m, rho_bar = {rho_bar} \
             kg/m^3, rho_medium = {} kg/m^3, g = {} m/s^2, mu = {} Pa*s",
            grain.settling_radius,
            medium.rho_medium,
            medium.g,
            medium.mu
        );
    }
    Ok(w)
}

/// Fold `w`, `dt` and `dx` into the single signed number the kernel receives.
///
/// **The one place `w`, `dt` and `dx` are visible at once**, and the one
/// negation of the whole file: the host knows `w` as positive downward and
/// `SettleParams::courant` is positive toward `+Z` (ADR-067).
///
/// # Errors
///
/// Returns an error if `dt` or `dx` is not finite and positive, if `w` is not
/// finite, or if `|c| > 1` **after** the fold into `Q`. The comparison is on the
/// `Q` and not on the `f64` it came from — see [`courant_is_within_one`], and
/// `the_settling_courant_is_compared_as_a_q_not_as_an_f64` for why that is not a
/// nicety here: the grain sitting exactly on the radius limit ADR-067 derives
/// has an `f64` Courant of `1.0000000000000002`.
pub fn settling_courant(w: f64, dt: f64, dx: f64) -> Result<Q> {
    if !w.is_finite() {
        bail!("the settling velocity {w} m/s is not a usable speed");
    }
    if !dt.is_finite() || dt <= 0.0 {
        bail!("dt {dt} s is not a usable timestep");
    }
    if !dx.is_finite() || dx <= 0.0 {
        bail!("dx {dx} m is not a usable voxel edge");
    }

    // The negation. Sinking is positive `w` and negative Courant.
    let courant = -(w * dt / dx);
    let folded = Q::from_f64(courant);
    if !courant_is_within_one(folded) {
        bail!(
            "settling at w = {w} m/s gives |w|*dt/dx = {} at dt = {dt} s and \
             dx = {dx} m, over the one-axis Courant condition of ADR-067. \
             Settling is a separate operator of step f and is applied over the \
             full step, so the condition is its own `w*dt/dx <= 1` rather than a \
             share of advection's outgoing-face budget, and there are no \
             settling substeps to divide it with",
            (w * dt / dx).abs()
        );
    }
    Ok(folded)
}

/// Settling of one lane of one field: one application per tick, along Z.
///
/// Holds the folded parameters the kernel receives and the velocity they came
/// from. One instance per **lane**, because the velocity is a constant of the
/// substance rather than of the world (ADR-067).
#[derive(Clone, Copy, Debug)]
pub struct Settle {
    params: SettleParams,
    /// The settling velocity in m/s, positive **downward**. Kept so that a
    /// caller can report it and so that a phase can decide whether to dispatch
    /// this lane at all.
    w: f64,
    n_voxels: u32,
}

impl Settle {
    /// Derive `w` and fold the parameters for one grain in one medium.
    ///
    /// # Errors
    ///
    /// The errors of [`settling_velocity`] and [`settling_courant`], and an
    /// `exchange` face. A grain over the Courant condition is refused with the
    /// limit on the radius spelled out, because `r <= sqrt(9*mu*dx/(2*d_rho*g*dt))`
    /// is what the author of the scenario can act on and "the velocity is too
    /// large" is not.
    pub fn new(grid: &Grid, grain: &Grain, medium: &Medium, dt: f64, dx: f64) -> Result<Self> {
        let w = settling_velocity(grain, medium)?;

        let courant = match settling_courant(w, dt, dx) {
            Ok(courant) => courant,
            Err(error) => {
                // The bound, inverted from `|w|*dt/dx <= 1` through
                // `w = 2*r^2*d_rho*g/(9*mu)`. Computed here rather than in
                // `settling_courant`, which knows only a speed: the excess
                // density is what turns a speed back into a radius.
                let excess = (grain_density(grain.molar_mass, grain.partial_molar_volume)?
                    - medium.rho_medium)
                    .abs();
                let limit = (9.0 * medium.mu * dx / (2.0 * excess * medium.g * dt)).sqrt();
                // Micrometres beside metres, because the limit of the worked
                // example is 5.27 um and `5.272660166570372e-6` is a number
                // nobody compares against a declared radius by eye.
                bail!(
                    "{error}. At mu = {} Pa*s, dx = {dx} m, dt = {dt} s and an \
                     excess density of {excess} kg/m^3 the heaviest grain that \
                     loads has r <= sqrt(9*mu*dx/(2*d_rho*g*dt)) = {limit} m = \
                     {} um, against the declared settling_radius = {} m = {} um. \
                     The eco regime at a one-second tick represents silt and \
                     finer, not sand, and that is a statement about the working \
                     window of the model rather than a defect of the scheme \
                     (ADR-067)",
                    medium.mu,
                    limit * 1.0e6,
                    grain.settling_radius,
                    grain.settling_radius * 1.0e6
                );
            }
        };

        Ok(Self {
            params: SettleParams {
                nx: grid.nx(),
                ny: grid.ny(),
                nz: grid.nz(),
                periodic_mask: periodic_mask(grid)?,
                courant,
            },
            w,
            n_voxels: grid.n_voxels(),
        })
    }

    /// The settling velocity in m/s, positive **downward**.
    #[inline]
    #[must_use]
    pub fn velocity(&self) -> f64 {
        self.w
    }

    /// The folded parameters, as the kernel receives them.
    #[inline]
    #[must_use]
    pub fn params(&self) -> SettleParams {
        self.params
    }

    /// Settling conserves matter, and does not touch energy (ADR-067).
    ///
    /// The first half holds by construction: a face is one integer applied to
    /// both sides with opposite signs, so the sum over the domain is unchanged
    /// exactly (ADR-005, ADR-054).
    ///
    /// The second half is about this process rather than about physics. ADR-067
    /// states it in the general form and it is worth repeating: **no** transport
    /// process in the project carries enthalpy along with the matter. Enthalpy
    /// is transported, but as a field of its own on its own grid (ADR-062).
    // TODO(channels): ADR-067 writes this invariant as "conserves matter except
    // through the BOUNDARY_EXCHANGE channel", and that arm of `Conservation`
    // does not exist. `Conserved` is true today only because `world::Grid::new`
    // refuses an `exchange` face, so a buoyant grain has no lid to leave
    // through. The statement becomes false on the day the arm appears, and this
    // comment is what will be pointing at it — which is also why
    // `settling_out_of_the_top_face_appears_in_boundary_exchange` cannot be
    // written yet: the process sees no channel registry at all.
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::Conserved,
        }
    }
}

/// Generates the per-width application loop, for the reason ADR-040 gives on the
/// kernel side: both storage widths exist, and two hand-written copies drift.
macro_rules! define_advance {
    ($name:ident, $field:ty, $voxel:ident) => {
        #[doc = concat!("Run one tick of settling on one lane of a `", stringify!($field), "`.")]
        ///
        /// One application, over the full step. There are no substeps here and
        /// ADR-067 rejects them by name: a grain that needs more than one voxel
        /// per tick is refused at load instead.
        ///
        /// One application is an **odd** number of applications, so this lane's
        /// new state ends in the back buffer and [`SettlePhase`] restores the
        /// process boundary afterwards (ADR-057). The pointers of the field are
        /// not touched here: the exchange is one per field, and lanes of the
        /// same field are dispatched a different number of times.
        ///
        /// Every voxel of the lane is visited before anything swaps. A skipped
        /// voxel is not left unchanged — `dst` is the other buffer — it is left
        /// a tick stale, which is indistinguishable from physics.
        ///
        /// # Panics
        ///
        /// If the field does not have the shape this process was folded against,
        /// or if `lane` is not one of its lanes.
        pub fn $name(&self, field: &mut $field, lane: u32) {
            let n_voxels = field.n_voxels();
            assert_eq!(
                n_voxels, self.n_voxels,
                "the field does not have the shape this process was folded for"
            );
            assert!(
                lane < field.lanes(),
                "lane {lane} of a field holding {}",
                field.lanes()
            );

            let (src, dst) = field.lane_pair_dir_mut(lane, Direction::Forward);
            for idx in 0..n_voxels {
                $voxel(src, dst, &self.params, idx);
            }
        }
    };
}

impl Settle {
    define_advance!(advance_lane_32, Field32, settle_voxel_32);
    define_advance!(advance_lane_64, Field64, settle_voxel_64);
}

/// One settling phase over a whole field: the settling lanes advanced, the
/// process-boundary invariant restored once.
///
/// # A lane that does not settle is not dispatched at all
///
/// And the predicate is `w == 0`, not `settling_radius > 0`. The two are not the
/// same: a grain with a radius but neutral buoyancy (`rho_bar == rho_medium`)
/// has `w == 0` and nothing to do, and that case arrives from physics rather
/// than from a refactor. If the dispatch branched on one and the parity count on
/// the other, the lane would land in the odd group and get the write buffer
/// promoted into its front — the previous phase's state, and zeroes on the first
/// tick. Zero is a legal amount and no invariant would notice (ADR-057).
#[derive(Clone, Debug)]
pub struct SettlePhase {
    /// One entry per lane, in lane order. `None` is a lane with `w = 0`, which
    /// is not dispatched.
    lanes: Vec<Option<Settle>>,
    split: ParitySplit,
    /// The width this phase was folded for.
    width: Width,
    n_voxels: u32,
}

impl SettlePhase {
    /// # Errors
    ///
    /// The errors of [`Settle::new`], and a `grains_by_lane` of the wrong
    /// length.
    pub fn new_32(
        grid: &Grid,
        lanes: u32,
        grains_by_lane: &[Grain],
        medium: &Medium,
        dt: f64,
        dx: f64,
    ) -> Result<Self> {
        Self::fold(grid, lanes, grains_by_lane, medium, dt, dx, Width::Bits32)
    }

    /// # Errors
    ///
    /// The errors of [`SettlePhase::new_32`].
    pub fn new_64(
        grid: &Grid,
        lanes: u32,
        grains_by_lane: &[Grain],
        medium: &Medium,
        dt: f64,
        dx: f64,
    ) -> Result<Self> {
        Self::fold(grid, lanes, grains_by_lane, medium, dt, dx, Width::Bits64)
    }

    fn fold(
        grid: &Grid,
        lanes: u32,
        grains_by_lane: &[Grain],
        medium: &Medium,
        dt: f64,
        dx: f64,
        width: Width,
    ) -> Result<Self> {
        if grains_by_lane.len() as u32 != lanes {
            bail!(
                "a field of {lanes} lanes was given {} grains: the array is \
                 indexed by lane, not by substance (ADR-056). Water is first in \
                 the registry and takes lane 0, so a substance-indexed array is \
                 right at s == 0 and off by one from there on — a set of lanes of \
                 exactly the right size gets used, and it is the wrong set",
                grains_by_lane.len()
            );
        }

        let mut folded = Vec::with_capacity(lanes as usize);
        // The parity comes from the applications the phase will actually run.
        // A lane counted as `n = 1` while no longer being dispatched lands in
        // the odd group and gets the write buffer promoted into its front
        // (ADR-057), and zero is a legal amount.
        let mut applications_by_lane = Vec::with_capacity(lanes as usize);

        for grain in grains_by_lane {
            // Built even for a lane that will not be dispatched, so that a bad
            // declaration is refused at load rather than ignored because it
            // happened to sit next to a zero.
            let settle = Settle::new(grid, grain, medium, dt, dx)?;
            if settle.velocity() == 0.0 {
                folded.push(None);
                applications_by_lane.push(0);
            } else {
                folded.push(Some(settle));
                applications_by_lane.push(1);
            }
        }

        Ok(Self {
            lanes: folded,
            split: ParitySplit::from_substeps(&applications_by_lane),
            width,
            n_voxels: grid.n_voxels(),
        })
    }

    /// How the field is put back together at the end of the phase.
    #[inline]
    #[must_use]
    pub fn parity_split(&self) -> &ParitySplit {
        &self.split
    }

    /// How many applications lane `lane` runs: zero or one, and this is exactly
    /// the number the parity of the split is computed from.
    ///
    /// # Panics
    ///
    /// If `lane` is not one of this phase's lanes.
    #[inline]
    #[must_use]
    pub fn applications_of(&self, lane: u32) -> u32 {
        u32::from(self.lanes[lane as usize].is_some())
    }

    /// The folded Courant number of a dispatched lane, or `None` for a lane
    /// this phase does not touch.
    ///
    /// # Panics
    ///
    /// If `lane` is not one of this phase's lanes.
    #[inline]
    #[must_use]
    pub fn courant_of(&self, lane: u32) -> Option<Q> {
        self.lanes[lane as usize].map(|settle| settle.params().courant)
    }

    /// Settling conserves matter and does not touch energy — see
    /// [`Settle::invariant`], which this phase repeats lane by lane, including
    /// the caveat about `BOUNDARY_EXCHANGE`.
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::Conserved,
        }
    }
}

/// Generates the per-width phase, for the reason [`define_advance`] gives: two
/// hand-written copies would drift in the restoration, which is the one place a
/// difference looks like physics.
macro_rules! define_phase_apply {
    ($name:ident, $field:ty, $advance:ident, $width:expr) => {
        #[doc = concat!("Run one tick of settling over every settling lane of a `", stringify!($field), "`.")]
        ///
        /// Returns with the process-boundary invariant of ADR-057 restored: the
        /// front buffer holds the current state for **every** lane, including
        /// the lanes this phase did not dispatch.
        ///
        /// # Panics
        ///
        /// If the field's lane count or shape is not the one this phase was
        /// folded for.
        pub fn $name(&self, field: &mut $field) {
            assert_eq!(
                self.width, $width,
                "this phase was folded for the other storage width"
            );
            assert_eq!(
                field.lanes() as usize,
                self.lanes.len(),
                "the field holds {} lanes and this phase was folded for {}",
                field.lanes(),
                self.lanes.len()
            );
            assert_eq!(
                field.n_voxels(),
                self.n_voxels,
                "the field does not have the shape this phase was folded for"
            );

            for (lane, folded) in self.lanes.iter().enumerate() {
                if let Some(settle) = folded {
                    settle.$advance(field, lane as u32);
                }
            }
            field.restore_boundary(&self.split);
        }
    };
}

impl SettlePhase {
    define_phase_apply!(apply_32, Field32, advance_lane_32, Width::Bits32);
    define_phase_apply!(apply_64, Field64, advance_lane_64, Width::Bits64);
}

fn courant_is_within_one(courant: Q) -> bool {
    let magnitude = if courant >= Q::ZERO {
        courant
    } else {
        qsub(Q::ZERO, courant)
    };
    magnitude <= Q::ONE
}

fn periodic_mask(grid: &Grid) -> Result<u32> {
    let mut mask = 0u32;
    for face in Face::ALL {
        match grid.boundary(face) {
            Boundary::Periodic => mask |= 1 << (face as u32),
            Boundary::Closed => {}
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
    use crate::numeric::{M32, M64};
    use crate::world::Field;

    /// The eco regime of SPEC section 1.7: a one-second tick and a 100 um voxel.
    const DT: f64 = 1.0;
    const DX: f64 = 1.0e-4;

    /// The medium of the worked example in ADR-067.
    ///
    /// **Every number here is a test fixture, and two of the three are not
    /// project constants.** `mu = 1e-3 Pa*s` is the viscosity of water and is a
    /// declared key. `rho_medium = 1000 kg/m^3` has no ASCII name yet
    /// (`CONFIG_SCHEMA.md` section 7 still prints "TODO"), and `g = 9.81` is
    /// assigned no value and no place by any document — ADR-069 says only that
    /// it does not become a key, and `config/validate.rs` refuses a settling
    /// substance outright rather than invent it. The pair is written here
    /// because the numeric examples of ADR-067 imply it: they give
    /// `w = 3.60e-4 m/s` at `r = 10 um` and `d_rho = 1650 kg/m^3`, which is
    /// `g = 9.81` and nothing else. Quoting it in a test is not deciding it.
    const MEDIUM: Medium = Medium {
        rho_medium: 1000.0,
        g: 9.81,
        mu: 1.0e-3,
    };

    /// The excess density of the worked example, `1650 kg/m^3`.
    ///
    /// A **reference** number, in ADR-067's own words: ADR-046 left the density
    /// of `MINERAL` undeclared, and this is a textbook value used for an
    /// estimate. It is repeated here so that the radius limit and the velocity
    /// below have one source, and it must not become a project constant by way
    /// of being cited.
    const DELTA_RHO: f64 = 1650.0;

    /// A grain of the given radius and grain density.
    ///
    /// `rho_bar = molar_mass*1e-3/V_bar`, so a partial molar volume of
    /// `1e-3 m^3/mol` makes the molar mass in g/mol numerically equal to the
    /// grain density in kg/m^3, and the derivation exact in `f64`.
    fn grain(settling_radius: f64, rho_bar: f64) -> Grain {
        Grain {
            settling_radius,
            molar_mass: rho_bar,
            partial_molar_volume: 1.0e-3,
        }
    }

    /// The heaviest grain the one-axis Courant condition admits:
    /// `r <= sqrt(9*mu*dx/(2*d_rho*g*dt))`, which is 5.27 um for the medium
    /// above (ADR-067).
    fn radius_limit(delta_rho: f64) -> f64 {
        (9.0 * MEDIUM.mu * DX / (2.0 * delta_rho * MEDIUM.g * DT)).sqrt()
    }

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

    /// A profile that varies **along Z**, which is the only variation settling
    /// can see and the one a torus would hide.
    ///
    /// Two degeneracies are being avoided at once, and neither can be relaxed:
    /// on a torus the loss of matter through the floor is invisible because
    /// there is no floor, and on a vertically uniform profile the outflow
    /// downward and the inflow from a cell that does not exist above cancel
    /// exactly.
    fn profile(grid: &Grid, idx: u32) -> i32 {
        let (x, y, z) = grid.coords(idx);
        1_000 + (z as i32) * 4_099 + (x as i32) * 13 + (y as i32) * 7
    }

    fn seed_lane_32(field: &mut Field32, lane: u32, amounts: impl Fn(u32) -> i32) {
        let n_voxels = field.n_voxels();
        let buffer = field.write_mut();
        for idx in 0..n_voxels {
            buffer[(lane * n_voxels + idx) as usize] = M32::new(amounts(idx));
        }
    }

    fn seed_lane_64(field: &mut Field64, lane: u32, amounts: impl Fn(u32) -> i64) {
        let n_voxels = field.n_voxels();
        let buffer = field.write_mut();
        for idx in 0..n_voxels {
            buffer[(lane * n_voxels + idx) as usize] = M64::new(amounts(idx));
        }
    }

    fn lane_total_32(field: &Field32, lane: u32) -> i64 {
        field.lane(lane).iter().map(|v| v.to_i64()).sum()
    }

    fn lane_total_64(field: &Field64, lane: u32) -> i64 {
        field.lane(lane).iter().map(|v| v.to_i64()).sum()
    }

    /// The first moment of a lane along Z: `sum z*amount`. Falls when matter
    /// sinks, rises when it floats.
    fn first_moment(grid: &Grid, field: &Field32, lane: u32) -> i64 {
        (0..grid.n_voxels())
            .map(|idx| {
                let (_, _, z) = grid.coords(idx);
                i64::from(z) * field.lane(lane)[idx as usize].to_i64()
            })
            .sum()
    }

    /// `ACCEPTANCE.md`, section "Conservation". The name is not new — ADR-067
    /// refuses to add a second one for the same check — and this is its process
    /// half: one operator, one axis, the exact antisymmetry of a face.
    ///
    /// A field of three lanes of which two settle, a Z-closed grid, a profile
    /// that varies along Z, ten ticks, and both storage widths. The sums are
    /// compared per lane as well as over the field: a phase that moved matter
    /// between lanes would keep the second sum and break the first.
    ///
    /// The last assertion of each half is what keeps the rest honest, and it is
    /// the same line the advective twin carries
    /// (`process/advect.rs::advection_alone_conserves_exactly`): a settling
    /// process that transports **nothing** conserves every sum here perfectly.
    /// ADR-067 deliberately did not add a second name for this check, on the
    /// ground that the existing one gains the content it did not have — "one
    /// operator, one axis, the exact antisymmetry of a face" — and a conserving
    /// no-op satisfies that sentence whole. So the movement is asserted here
    /// rather than left to the neighbouring tests, which would make this name
    /// green on their evidence rather than on its own.
    #[test]
    fn sedimentation_conserves_exactly() {
        let grid = floored(4, 5, 6);
        let grains = [
            grain(0.0, 2650.0),                // does not settle at all
            grain(3.0e-6, 1000.0 + DELTA_RHO), // sinks, |c| = 0.324
            grain(3.0e-6, 500.0),              // floats, |c| = 0.098
        ];
        let lanes = grains.len() as u32;

        let phase = SettlePhase::new_32(&grid, lanes, &grains, &MEDIUM, DT, DX).unwrap();
        assert_eq!(
            phase.invariant(),
            Invariant {
                matter: Conservation::Conserved,
                energy: Conservation::Conserved,
            }
        );

        let mut field: Field32 = Field::new(&grid, lanes).unwrap();
        for lane in 0..lanes {
            seed_lane_32(&mut field, lane, |idx| {
                profile(&grid, idx) * (1 + lane as i32)
            });
        }
        field.swap();
        let before: Vec<i64> = (0..lanes).map(|l| lane_total_32(&field, l)).collect();
        let seeded: Vec<Vec<M32>> = (0..lanes).map(|l| field.lane(l).to_vec()).collect();

        for tick in 0..10 {
            phase.apply_32(&mut field);
            for lane in 0..lanes {
                assert_eq!(
                    lane_total_32(&field, lane),
                    before[lane as usize],
                    "tick {tick} moved the total of lane {lane}"
                );
            }
        }
        // Lane 0 does not settle and must not have moved; the two that do must
        // have. Without the second half a process that transports nothing
        // passes every assertion above.
        assert_eq!(field.lane(0), seeded[0].as_slice());
        for lane in 1..lanes {
            assert_ne!(
                field.lane(lane),
                seeded[lane as usize].as_slice(),
                "lane {lane} settles and nothing moved: a conserving no-op \
                 passes every assertion above"
            );
        }

        // The same text on the wide storage width.
        let phase = SettlePhase::new_64(&grid, lanes, &grains, &MEDIUM, DT, DX).unwrap();
        let mut wide: Field64 = Field::new(&grid, lanes).unwrap();
        for lane in 0..lanes {
            seed_lane_64(&mut wide, lane, |idx| {
                5_100_000_000_000 + i64::from(profile(&grid, idx)) * i64::from(1 + lane)
            });
        }
        wide.swap();
        let before: Vec<i64> = (0..lanes).map(|l| lane_total_64(&wide, l)).collect();
        let seeded: Vec<Vec<M64>> = (0..lanes).map(|l| wide.lane(l).to_vec()).collect();
        for tick in 0..10 {
            phase.apply_64(&mut wide);
            for lane in 0..lanes {
                assert_eq!(
                    lane_total_64(&wide, lane),
                    before[lane as usize],
                    "tick {tick} moved the total of lane {lane} at 64 bits"
                );
            }
        }
        assert_eq!(wide.lane(0), seeded[0].as_slice());
        for lane in 1..lanes {
            assert_ne!(
                wide.lane(lane),
                seeded[lane as usize].as_slice(),
                "lane {lane} settles and nothing moved at 64 bits"
            );
        }
    }

    /// `ACCEPTANCE.md`, section "Deriving the scales", from ADR-067.
    ///
    /// `k = 2r^2/9`, and — the half the name is for — no type in this module
    /// accepts `k` in any form. That half is checked by the compiler rather
    /// than by an assertion: [`Grain`] and [`Medium`] are built here with every
    /// field named, so a `k` added to either stops this test compiling.
    /// ADR-067 rejected the key because `2.2e-11` has no readable preimage: a
    /// wrong one is indistinguishable from a right one and is absorbed by the
    /// calibration of the sedimentation rate forever.
    #[test]
    fn settling_coefficient_is_derived_from_radius_not_declared() {
        for r in [0.0, 1.0e-7, 3.0e-6, 1.0e-5, 5.0e-5] {
            assert_eq!(settling_coefficient(r), 2.0 * r * r / 9.0);
        }
        // r = 0 is not a magic value: 2*0^2/9 is an exact zero, so the kernel
        // needs no branch and the host simply does not build a process for it.
        assert_eq!(settling_coefficient(0.0), 0.0);

        // And the velocity of a folded process is that coefficient, not a
        // second number arriving from somewhere else.
        let r = 3.0e-6;
        let settle = Settle::new(
            &floored(3, 3, 4),
            &grain(r, 1000.0 + DELTA_RHO),
            &MEDIUM,
            DT,
            DX,
        )
        .unwrap();
        assert_eq!(
            settle.velocity(),
            settling_coefficient(r) * DELTA_RHO * MEDIUM.g / MEDIUM.mu
        );
    }

    /// `ACCEPTANCE.md`, section "Deriving the scales", from ADR-067.
    ///
    /// The worked example: `r = 10 um`, `d_rho = 1650 kg/m^3`, `mu = 1e-3 Pa*s`
    /// give `k = 2.22e-11 m^2` and `w = 3.60e-4 m/s`. Both numbers are
    /// reference values quoted for an estimate — `MINERAL` has no declared
    /// density (ADR-046) — and they are here to pin the *form*, not to become
    /// constants.
    ///
    /// The scale law is the half that survives any change of constants:
    /// `w(2r) == 4*w(r)`. `k = r^2/18` — the diameter form with a radius inside
    /// it — gives a velocity exactly four times too small, and a factor of four
    /// is quieter than a factor of a thousand and rides out the calibration of
    /// the sedimentation rate just as invisibly (ADR-067, ADR-055).
    #[test]
    fn stokes_velocity_uses_radius_not_diameter() {
        let r = 1.0e-5;
        assert!((settling_coefficient(r) - 2.22e-11).abs() < 0.01e-11);

        let dense = grain(r, 1000.0 + DELTA_RHO);
        let w = settling_velocity(&dense, &MEDIUM).unwrap();
        assert!((w - 3.60e-4).abs() < 0.01e-4, "w = {w}");

        // The diameter form with a radius inside it, which is what the record
        // names as the mistake.
        let wrong = r * r / 18.0 * DELTA_RHO * MEDIUM.g / MEDIUM.mu;
        assert!(
            (w - 4.0 * wrong).abs() < 1.0e-18,
            "w = {w}, wrong = {wrong}"
        );

        let doubled = settling_velocity(&grain(2.0 * r, 1000.0 + DELTA_RHO), &MEDIUM).unwrap();
        assert_eq!(doubled, 4.0 * w);
    }

    /// The one silent negation of this file.
    ///
    /// The host knows `w` as positive **downward** — a grain denser than the
    /// medium sinks — and `SettleParams::courant` is positive toward `+Z`.
    /// Exactly one negation lies between them. Invert it and mineral floats out
    /// through the lid while the floor stays clean, with conservation, the
    /// per-face agreement, the undershoot bound and both storage widths all
    /// green.
    #[test]
    fn a_dense_grain_sinks_and_a_buoyant_one_rises() {
        let grid = floored(3, 3, 6);

        let dense = grain(3.0e-6, 1000.0 + DELTA_RHO);
        let settle = Settle::new(&grid, &dense, &MEDIUM, DT, DX).unwrap();
        assert!(settle.velocity() > 0.0, "a denser grain must sink");
        assert!(
            settle.params().courant < Q::ZERO,
            "sinking is toward -Z, so the kernel's number is negative"
        );

        let buoyant = grain(3.0e-6, 500.0);
        let rising = Settle::new(&grid, &buoyant, &MEDIUM, DT, DX).unwrap();
        assert!(rising.velocity() < 0.0);
        assert!(rising.params().courant > Q::ZERO);

        // And the profile actually moves that way.
        for (grains, sinks) in [([dense], true), ([buoyant], false)] {
            let phase = SettlePhase::new_32(&grid, 1, &grains, &MEDIUM, DT, DX).unwrap();
            let mut field: Field32 = Field::new(&grid, 1).unwrap();
            seed_lane_32(&mut field, 0, |idx| profile(&grid, idx));
            field.swap();
            let before = first_moment(&grid, &field, 0);
            phase.apply_32(&mut field);
            let after = first_moment(&grid, &field, 0);
            if sinks {
                assert!(after < before, "the dense grain did not sink");
            } else {
                assert!(after > before, "the buoyant grain did not rise");
            }
        }
    }

    /// `ACCEPTANCE.md`, section "Observable behaviour", from ADR-067. This is
    /// the host half; the kernel half is `a_zero_courant_leaves_the_lane_untouched`.
    ///
    /// A lane at `settling_radius = 0` is not dispatched **at all**: no
    /// application, no Courant number, and its values are bit for bit what they
    /// were. Bitwise and not "barely moved": one unit per face per tick is
    /// `10^6` units over the S0 horizon.
    ///
    /// Zero applications is an even number of applications, so the lane lands in
    /// the even group of the split and the restoration leaves it alone.
    #[test]
    fn substance_with_zero_settling_radius_does_not_move_vertically() {
        let grid = floored(3, 4, 5);
        let grains = [grain(0.0, 2650.0), grain(3.0e-6, 1000.0 + DELTA_RHO)];
        let phase = SettlePhase::new_32(&grid, 2, &grains, &MEDIUM, DT, DX).unwrap();

        assert_eq!(phase.applications_of(0), 0);
        assert_eq!(phase.courant_of(0), None);
        assert_eq!(phase.applications_of(1), 1);
        assert!(phase.courant_of(1).is_some());
        assert_eq!(phase.applications_of(0) % 2, 0, "not in the even group");

        let mut field: Field32 = Field::new(&grid, 2).unwrap();
        seed_lane_32(&mut field, 0, |idx| profile(&grid, idx));
        seed_lane_32(&mut field, 1, |idx| profile(&grid, idx));
        field.swap();
        let before = field.lane(0).to_vec();

        phase.apply_32(&mut field);
        assert_eq!(field.lane(0), before.as_slice());
        // And the lane that does settle did move, or the assertion above is
        // about a phase that does nothing at all.
        assert_ne!(field.lane(1), before.as_slice());
    }

    /// The trap of ADR-057 in its settling form, and it is reachable from a
    /// plausible config.
    ///
    /// A lane with `settling_radius > 0` that is neutrally buoyant
    /// (`rho_bar == rho_medium`, so `w == 0`) is not dispatched either, and the
    /// parity has to be counted from the applications the phase **ran**, not
    /// from the predicate `radius > 0`. Let the two disagree and the lane lands
    /// in the odd group and gets the write buffer promoted into its front: the
    /// previous phase's state, and zeroes on the first tick. Zero is a legal
    /// amount and no invariant would notice.
    #[test]
    fn a_lane_that_was_not_dispatched_is_not_left_a_tick_stale() {
        let grid = floored(3, 4, 5);
        let neutral = grain(3.0e-6, MEDIUM.rho_medium);
        assert_eq!(
            grain_density(neutral.molar_mass, neutral.partial_molar_volume).unwrap(),
            MEDIUM.rho_medium
        );
        let grains = [grain(3.0e-6, 1000.0 + DELTA_RHO), neutral];
        let phase = SettlePhase::new_32(&grid, 2, &grains, &MEDIUM, DT, DX).unwrap();

        assert_eq!(settling_velocity(&neutral, &MEDIUM).unwrap(), 0.0);
        assert_eq!(phase.applications_of(1), 0);
        assert_eq!(phase.courant_of(1), None);

        let mut field: Field32 = Field::new(&grid, 2).unwrap();
        seed_lane_32(&mut field, 0, |idx| profile(&grid, idx));
        seed_lane_32(&mut field, 1, |idx| profile(&grid, idx));
        field.swap();
        let before = field.lane(1).to_vec();

        // A marker in the write buffer: it must not come out.
        const MARKER: i32 = -424_242;
        seed_lane_32(&mut field, 1, |_| MARKER);

        phase.apply_32(&mut field);
        assert_eq!(
            field.lane(1),
            before.as_slice(),
            "a lane the phase never dispatched came back holding something else"
        );
    }

    /// Settling is checked against **its own** one-axis condition.
    ///
    /// Three claims. The grain exactly on the derived limit loads. The grain
    /// past it is refused, and the refusal names both `w` and the limit on the
    /// radius, because "too fast" is not something a scenario author can act
    /// on. And a grain that passes its own condition is **not** cut down to a
    /// share of advection's six-face budget `dx/(6*dt)`: transport here runs
    /// along one axis, so the sum over outgoing faces in SPEC section 4.2
    /// degenerates to a single term and both inequalities of that section
    /// coincide (ADR-067, ADR-036).
    ///
    /// Both mistakes this forbids conserve matter exactly and stay stable:
    /// trimming settling to `0.167`, and splitting it into substeps — which
    /// ADR-067 rejects by name.
    #[test]
    fn the_courant_condition_is_settlings_own_and_not_a_share_of_advections() {
        let grid = floored(3, 3, 5);
        let limit = radius_limit(DELTA_RHO);
        assert!((limit - 5.27e-6).abs() < 0.01e-6, "limit = {limit}");

        // Exactly on the limit: accepted, and the kernel gets exactly -1.
        let at_the_limit =
            Settle::new(&grid, &grain(limit, 1000.0 + DELTA_RHO), &MEDIUM, DT, DX).unwrap();
        assert_eq!(at_the_limit.params().courant, qsub(Q::ZERO, Q::ONE));

        // Past it: refused, naming w and the bound on the radius.
        let error =
            Settle::new(&grid, &grain(5.3e-6, 1000.0 + DELTA_RHO), &MEDIUM, DT, DX).unwrap_err();
        let message = error.to_string();
        let w = settling_velocity(&grain(5.3e-6, 1000.0 + DELTA_RHO), &MEDIUM).unwrap();
        assert!(message.contains(&format!("w = {w}")), "{message}");
        assert!(message.contains("5.27"), "{message}");

        // Between advection's six-face share and one: legal, and folded whole.
        let r = 4.0e-6;
        let settle = Settle::new(&grid, &grain(r, 1000.0 + DELTA_RHO), &MEDIUM, DT, DX).unwrap();
        let c = settle.velocity() * DT / DX;
        assert!(c > 1.0 / 6.0 && c < 1.0, "c = {c}");
        assert_eq!(settle.params().courant, Q::from_f64(-c));
        assert_ne!(settle.params().courant, Q::from_f64(-c / 6.0));
    }

    /// The Courant number is compared as the `Q` the kernel receives, not as the
    /// `f64` it was computed from (ADR-068).
    ///
    /// Here the requirement is sharper than in advection, because the settling
    /// kernel gets the number as a single scalar and there is nowhere else to
    /// compare it. It also comes out the other way round from the way ADR-068
    /// reads: one is exactly representable in both `f64` and the `f32` behind
    /// `Q`, so no value at or below one can climb above it in the cast, and the
    /// failure ADR-068 shows on `1/6` cannot be reproduced at this bound.
    ///
    /// What is left is the same requirement seen from the other side, and it is
    /// not academic: the grain sitting exactly on the radius limit ADR-067
    /// derives has an `f64` Courant of `1.0000000000000002`. Compared as an
    /// `f64` it is refused — the record's own limit would be unreachable.
    /// Compared as the `Q` the kernel runs on it is exactly one, and legal.
    #[test]
    fn the_settling_courant_is_compared_as_a_q_not_as_an_f64() {
        let limit = radius_limit(DELTA_RHO);
        let w = settling_velocity(&grain(limit, 1000.0 + DELTA_RHO), &MEDIUM).unwrap();
        assert!(w * DT / DX > 1.0, "the f64 courant is over one");
        assert_eq!(Q::from_f64(-(w * DT / DX)), qsub(Q::ZERO, Q::ONE));
        assert_eq!(settling_courant(w, DT, DX).unwrap(), qsub(Q::ZERO, Q::ONE));

        // Over one after the fold as well: refused.
        let over = (1.0 + 2.0f64.powi(-20)) * DX / DT;
        assert!(settling_courant(over, DT, DX).is_err());
        assert!(settling_courant(-over, DT, DX).is_err());
    }

    /// Parameter arrays are indexed by **lane**, never by substance (ADR-056).
    ///
    /// Water is first in the registry of SPEC section 2.3 and takes lane 0, so a
    /// substance-indexed array is right at `s == 0` and off by one from there
    /// on: a set of lanes of exactly the right size gets copied, and it is the
    /// wrong set.
    #[test]
    fn the_parity_split_is_indexed_by_lane_and_not_by_substance() {
        let grid = floored(3, 3, 5);
        let grains = [grain(0.0, 2650.0), grain(3.0e-6, 1000.0 + DELTA_RHO)];

        let error = SettlePhase::new_32(&grid, 3, &grains, &MEDIUM, DT, DX).unwrap_err();
        assert!(error.to_string().contains("ADR-056"), "{error}");

        // Water first, the settling substance second: the phase moves lane 1 and
        // leaves lane 0 exactly where it was.
        let phase = SettlePhase::new_32(&grid, 2, &grains, &MEDIUM, DT, DX).unwrap();
        let mut field: Field32 = Field::new(&grid, 2).unwrap();
        seed_lane_32(&mut field, 0, |idx| profile(&grid, idx));
        seed_lane_32(&mut field, 1, |idx| profile(&grid, idx));
        field.swap();
        let before = field.lane(0).to_vec();

        phase.apply_32(&mut field);
        assert_eq!(
            field.lane(0),
            before.as_slice(),
            "the neighbouring lane moved"
        );
        assert_ne!(
            field.lane(1),
            before.as_slice(),
            "the settling lane did not"
        );
    }

    /// The price ADR-067 named and paid: `rho_medium` is a constant, there is no
    /// hindered settling, and matter piles up in the bottom voxel until it hits
    /// `max_conc`.
    ///
    /// So settling without compaction is a process that is **guaranteed to stop
    /// a long run**, and that has to be known in advance rather than found on
    /// the millionth tick. The chain is run to its fixed point and the fixed
    /// point is asserted to be a full bottom layer over an empty column.
    ///
    /// `|c| = 1` on purpose. There the antidiffusive term `c*(1 - |c|)/2`
    /// vanishes, every voxel hands its whole content to the one below, and the
    /// fixed point is reached in `nz - 1` ticks instead of being approached
    /// forever — at `|c| < 1` the rounding freezes the last unit and the
    /// statement would be a weaker one about a different state.
    #[test]
    fn matter_accumulates_and_the_sediment_does_not_stop_itself() {
        let grid = floored(3, 3, 5);
        let phase = SettlePhase::new_32(
            &grid,
            1,
            &[grain(radius_limit(DELTA_RHO), 1000.0 + DELTA_RHO)],
            &MEDIUM,
            DT,
            DX,
        )
        .unwrap();
        assert_eq!(phase.courant_of(0), Some(qsub(Q::ZERO, Q::ONE)));

        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_lane_32(&mut field, 0, |idx| profile(&grid, idx));
        field.swap();
        let total = lane_total_32(&field, 0);

        for _ in 0..grid.nz() {
            phase.apply_32(&mut field);
        }

        let mut floor = 0i64;
        for idx in 0..grid.n_voxels() {
            let (_, _, z) = grid.coords(idx);
            let amount = field.lane(0)[idx as usize].to_i64();
            if z == 0 {
                floor += amount;
            } else {
                assert_eq!(amount, 0, "voxel {idx} above the floor still holds matter");
            }
        }
        assert_eq!(floor, total, "the sediment lost or gained on the way down");

        // And it is a fixed point: nothing stops it, and nothing moves it on.
        let settled = field.lane(0).to_vec();
        for _ in 0..3 {
            phase.apply_32(&mut field);
            assert_eq!(field.lane(0), settled.as_slice());
        }
    }
}
