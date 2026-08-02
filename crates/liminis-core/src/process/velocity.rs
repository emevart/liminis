//! The prescribed velocity field: four dispatches, and every number the three
//! kernels are not allowed to know.
//!
//! Step `b` of the tick order (SPEC section 8), the operator ADR-069 introduces:
//!
//! ```text
//! A = A_conv + A_noise,    A_conv = L*(d_y T~, -d_x T~, 0),    u = curl A
//! ```
//!
//! ```text
//! 1. potential_voxel        over the enthalpy grid   (32^3, 2.6e5 reads)
//! 2. interpolate_potential  over the velocity grid   (64^3, 4.2e6)
//! 3. stir_potential         over the velocity grid   only if stir_fraction > 0
//! 4. curl_voxel             over the velocity grid   (2.1e6, 3.15e6 stirred)
//! 5. face_courant_voxel     over the fine grid       into the advection layout
//! ```
//!
//! # Where the five buffers live, and what the composition costs
//!
//! [`VelocityField::apply`] takes every buffer by argument and owns none. Four of
//! the five belong to `world::World` — the coarse potential on the enthalpy grid,
//! the potential interpolated onto the velocity grid, the stirred copy of it and
//! `u` — and come out of one call to `World::velocity_slices_mut` in this
//! function's argument order. The fifth, the Courant numbers of the fine faces,
//! belongs to `process::Scratch`, and the two owners borrow disjointly.
//!
//! ADR-069 priced step `b` at two buffers, `A` and `u`, `3 x 64^3 x 4 B` each:
//! 6.3 MB, 1.3% of the 482 MB of state. The composition that is implemented has
//! four, 9 830 400 B = 9.830 MB at the 128^3 base grid, and neither of the two
//! extra buffers is optional. The coarse potential exists because the closed form
//! of the record has no interpolation stage, and the wide difference has to be
//! taken on the grid the temperature lives on; the stirred copy exists because
//! `stir_potential` reads its source at every octave, and a kernel may not read
//! the buffer it writes (ADR-034). With the two coarse fields of ADR-079 beside
//! them the block is 10 092 544 B = 10.09 MB, 2.09% rather than 1.3%. The
//! arithmetic is written out on `world::World`'s `VELOCITY_COMPONENTS`; the
//! divergence from the record is a record of its own and not an edit to ADR-069
//! (ADR-032), and this paragraph is not that record.
//!
//! # The default is `false`, and it is arithmetic rather than caution
//!
//! [`VELOCITY_FIELD_ENABLED_BY_DEFAULT`]. `u_conv_max` is required when the
//! process is on and has no default of its own (ADR-069), and ADR-065 makes the
//! loader materialise the full list of processes **before** hashing — so the
//! record of this process appears in the canonical form of every scenario,
//! including the ones that never mention it. A default of `true` would therefore
//! refuse every such scenario, `configs/scenarios/hello.toml` first.
//!
//! # The estimate of `|u|` is derived at load, so there is no reduction in a tick
//!
//! `|u_conv| <= L * ||D||_1 * (t_max - t_min)`, and `L` is chosen to make that an
//! equality at `u_conv_max`. The whole point is that nothing measures the field at
//! run time: ADR-069 rejects the global normalisation with four separate reasons,
//! of which the operational one is that it puts a host readback and a barrier in
//! the middle of a tick, and turns a one-ULP disagreement in any voxel into a
//! disagreement of every field of class `M`.
//!
//! **`||D||_1` is convolved from the three stencils that are actually
//! implemented**, and this is the silent place of the whole file. The closed form
//! of ADR-069, `L = u_conv_max*h*s/(t_max - t_min)`, is written for the
//! composition "narrow difference after wide difference" and does **not** contain
//! the trilinear interpolation of the potential from 32^3 onto 64^3, which this
//! decision has. Take `L` from the formula rather than from the actual
//! composition and the estimate comes out wrong; the field exceeds the ceiling
//! `dx/(6*dt)`, the sum over outgoing faces passes one, and nothing fires — the
//! validator checked `u_conv_max`, not the real maximum; the debug assertion of
//! ADR-069 is required never to trip and is absent in release; and the amounts go
//! negative and read as the accepted undershoot of ADR-068. So
//! [`VelocityField::stencil_l1_norm`] composes the coefficient lists of the three
//! kernels and is checked against their measured impulse response in
//! `tests/acceptance_velocity.rs`.
//!
//! # It reads last tick's enthalpy, and that is the declared price
//!
//! Step `b` reads the enthalpy folded by step `i'` of the **previous** tick
//! (ADR-045, ADR-049), so the convective correction lags by one tick. That is the
//! ordinary price of first-order splitting, the same one ADR-050 named for the
//! catalysis fields, and ADR-069 puts a number on it: 0.1% of a transit at a
//! one-second tick. It is named here so that it is read rather than discovered.
//!
//! # Nothing here is added to the pressure velocity
//!
//! ADR-069 forbids it on three grounds and `process/pressure.rs` writes them out.
//! Steps `b`, `c`, `e` and `f` are separate factors of the Lie-Trotter splitting
//! (ADR-036), each with its own stability condition, and the displacement over a
//! tick is a sum of three separate terms — 0.167 from advection, at most one from
//! pressure, at most one from settling — rather than one shared budget.

use anyhow::{Result, bail};
use std::collections::BTreeMap;

use super::{Conservation, Invariant};
use crate::kernels::curl::{CurlParams, SampleParams, curl_voxel, face_courant_voxel};
use crate::kernels::noise::{NOISE_OCTAVE_MAX, NoiseParams, stir_potential};
use crate::kernels::potential::{
    InterpolateParams, PotentialParams, interpolate_potential, potential_voxel,
};
use crate::numeric::{M64, Q};
use crate::world::{Boundary, Face, Grid};

/// The default of `enabled` for the velocity field (ADR-065, ADR-069).
///
/// **The process declares its own default**, and `config/validate.rs` reads it
/// from here rather than holding a second copy: two constants of one meaning drift
/// apart, and the direction that drifts silently is a validator that treats an
/// enabled process as disabled — the Courant condition then loses its only live
/// input and the config loads with an advection speed nobody checked.
///
/// `false`, for the reason in the module header.
pub const VELOCITY_FIELD_ENABLED_BY_DEFAULT: bool = false;

/// The three components of a vector field on the velocity grid.
const COMPONENTS: u32 = 3;

/// What a scenario declares about the velocity field, plus the four numbers of
/// the world it needs to derive the rest.
///
/// Four keys of ADR-069 — `u_conv_max`, `l_c`, `stir_fraction`, `stir_period` —
/// and none of them knows about a voxel (SPEC section 10). The mobility `L`, the
/// stencil radius `r`, the octave count and the estimate of `|u|` are all derived
/// from them here.
#[derive(Clone, Copy, Debug)]
pub struct VelocityConfig {
    /// The convective speed the mobility is calibrated to, m/s. Required, no
    /// default (ADR-069).
    pub u_conv_max: f64,
    /// The structure length `l_c`, m. The half-width of the wide difference is
    /// `r = round(l_c/(2*dx_coarse))` cells of the **enthalpy** grid.
    pub l_c: f64,
    /// The stirring amplitude as a fraction of `u_conv_max`. Zero means the noise
    /// kernel is not dispatched at all.
    pub stir_fraction: f64,
    /// The stirring period, s. Required when `stir_fraction > 0`.
    // TODO(stir-period): how the period enters the draw is not decided. ADR-069
    // lays the counters out with `b` as the tick number, and a period has nowhere
    // to act in that layout unless the draw is interpolated in time between two
    // pulls — a mechanism the record does not name. The key is carried here and
    // validated, and it reaches no kernel; inventing an interpolation would be a
    // decision about the dynamics made in a struct field.
    pub stir_period: Option<f64>,
    /// The tick, s.
    pub dt: f64,
    /// The edge of a **fine** voxel, m.
    pub dx: f64,
    /// The lower end of the declared temperature range, K (ADR-062).
    pub t_min: f64,
    /// The upper end of the declared temperature range, K (ADR-062).
    pub t_max: f64,
    /// How many storage units of enthalpy make one joule (ADR-062, ADR-039).
    pub units_per_joule: f64,
    /// How many ticks pass between applications. Only one is admissible.
    // Not one of the four keys of ADR-069: it is the generic `every_n_ticks` of
    // SPEC section 4.1, and it is here so that the refusal below has something to
    // refuse. See [`VelocityField::new`].
    pub every_n_ticks: u32,
}

/// The prescribed velocity field: the folded parameters of the four dispatches,
/// and the numbers derived at load.
#[derive(Clone, Debug)]
pub struct VelocityField {
    potential: PotentialParams,
    interpolate: InterpolateParams,
    curl: CurlParams,
    sample: SampleParams,
    /// Everything the noise kernel needs except the tick and the run key, which
    /// arrive per application.
    noise: NoiseParams,
    stirs: bool,
    mobility: f64,
    stencil_l1_norm: f64,
    speed_bound: f64,
    n_coarse: u32,
    n_velocity: u32,
    n_voxels: u32,
}

impl VelocityField {
    /// Derive everything from the three grids and the scenario.
    ///
    /// The three grids are the fine one (amounts, and the faces the Courant
    /// numbers land on), the velocity one (`lod = 1` in the eco regime) and the
    /// enthalpy one (`lod = 2`). Both coarsenings are read off the extents rather
    /// than taken as arguments, so the relation between the grids cannot be
    /// declared one way and be another.
    ///
    /// # Errors
    ///
    /// Returns an error if a number is unusable, if the temperature range is
    /// empty, if the grids are not related by a whole power of two, if
    /// `r = round(l_c/(2*dx_coarse))` is under one, if `every_n_ticks` is over
    /// one, or if stirring is asked for on extents whose octave lattice cannot
    /// wrap.
    pub fn new(
        fine: &Grid,
        velocity: &Grid,
        enthalpy: &Grid,
        cfg: &VelocityConfig,
    ) -> Result<Self> {
        for (name, value) in [
            ("u_conv_max", cfg.u_conv_max),
            ("l_c", cfg.l_c),
            ("dt", cfg.dt),
            ("dx", cfg.dx),
            ("units_per_joule", cfg.units_per_joule),
        ] {
            if !value.is_finite() || value <= 0.0 {
                bail!("the velocity field declares {name} = {value}, which is not a usable number");
            }
        }
        if !(cfg.stir_fraction.is_finite() && (0.0..=1.0).contains(&cfg.stir_fraction)) {
            bail!(
                "the velocity field declares stir_fraction = {}, and the rule is \
                 stir_fraction in [0, 1]: it is a fraction of u_conv_max (ADR-069)",
                cfg.stir_fraction
            );
        }
        if cfg.stir_fraction > 0.0 && cfg.stir_period.is_none() {
            bail!(
                "the velocity field declares stir_fraction = {} above zero and no \
                 stir_period, which is required in that case and has no default \
                 (ADR-069)",
                cfg.stir_fraction
            );
        }
        // ADR-069: "`every_n_ticks > 1` is not given to the velocity field". The
        // field is not integrated, so the ban of ADR-030 does not formally reach
        // it — but the saving is nil on a step that costs a fifth of diffusion,
        // and the plume lags by `n` ticks. Without a refusal that lag is a quietly
        // late flow under a ledger that closes exactly.
        if cfg.every_n_ticks != 1 {
            bail!(
                "the velocity field declares every_n_ticks = {}, and only 1 is \
                 admissible (ADR-069). SPEC section 1.5 prints \"medium velocity | \
                 64^3 | every tick\", and the reason is not the stability argument \
                 of ADR-030: the field is not integrated, so skipping ticks costs \
                 nothing in stability and buys nothing in time — the step is a \
                 fifth the price of diffusion — while the plume lags by {} ticks \
                 with every invariant still closing",
                cfg.every_n_ticks,
                cfg.every_n_ticks
            );
        }

        let span = cfg.t_max - cfg.t_min;
        if !span.is_finite() || span <= 0.0 {
            bail!(
                "the declared temperature range is t_min = {} K to t_max = {} K \
                 (ADR-062), which spans {span} K: the mobility is derived from it, \
                 so an empty range gives no calibration at all",
                cfg.t_min,
                cfg.t_max
            );
        }

        let velocity_lod = lod_between(fine, velocity, "velocity")?;
        let enthalpy_lod = lod_between(fine, enthalpy, "enthalpy")?;
        if enthalpy_lod < velocity_lod {
            bail!(
                "the enthalpy grid (lod {enthalpy_lod}) is finer than the velocity \
                 grid (lod {velocity_lod}): the potential is taken on the first and \
                 interpolated onto the second (ADR-069), and this way round the \
                 interpolation would be a coarsening"
            );
        }
        let ratio_log2 = enthalpy_lod - velocity_lod;

        // `dx_coarse` is the step of the **enthalpy** grid, because that is the
        // grid the wide difference is taken on. Taken from the velocity grid
        // instead, `r` is out by exactly a factor of two, that is by one octave in
        // the selected wavelength — and nothing sees it (`kernels/potential.rs`).
        let dx_coarse = cfg.dx * f64::from(1u32 << enthalpy_lod);
        let dx_velocity = cfg.dx * f64::from(1u32 << velocity_lod);
        let radius = (cfg.l_c / (2.0 * dx_coarse)).round();
        if radius < 1.0 {
            bail!(
                "the velocity field declares l_c = {} m, giving a wide-stencil \
                 radius of r = round(l_c/(2*dx_coarse)) = {radius} cells, under the \
                 limit of 1. `dx_coarse` here is the step of the *enthalpy* grid, \
                 {dx_coarse} m, because that is the grid the wide temperature \
                 difference is taken on. Below one cell the wide difference \
                 degenerates into a narrow one and the wavelength selection goes \
                 with it (ADR-069)",
                cfg.l_c
            );
        }
        let r = radius as u32;
        if 2 * r >= enthalpy.nx().min(enthalpy.ny()) {
            bail!(
                "l_c = {} m gives r = {r} cells against an enthalpy grid {}x{} \
                 wide: at half the extent the two arms of the wide difference wrap \
                 onto one cell on a periodic axis and the difference is identically \
                 zero. ADR-069 bounds the search for l_c at half the domain, which \
                 is a quarter of the extent in cells",
                cfg.l_c,
                enthalpy.nx(),
                enthalpy.ny()
            );
        }

        let periodic_mask = periodic_mask(fine)?;
        let floor_is_closed = u32::from(fine.boundary(Face::ZMinus) == Boundary::Closed);

        // The gain of the wide difference with `L` factored out, and the gain of
        // the narrow curl. `||D||_1` is convolved from these two and from the
        // interpolation weights — the three stencils the kernels implement, and
        // not the closed form of ADR-069, which has no interpolation stage in it.
        let wide_gain = 1.0 / (2.0 * f64::from(r) * dx_coarse);
        let curl_gain = 1.0 / (2.0 * dx_velocity);
        let stencil_l1_norm = composed_l1_norm(1u32 << ratio_log2, r as i32, wide_gain, curl_gain);
        if !(stencil_l1_norm.is_finite() && stencil_l1_norm > 0.0) {
            bail!(
                "the composed stencil has an l1 norm of {stencil_l1_norm}, so the \
                 mobility cannot be derived from it"
            );
        }
        // ADR-069: `L` is taken so that `|u_conv| <= u_conv_max` holds exactly at
        // the declared temperature span.
        let mobility = cfg.u_conv_max / (stencil_l1_norm * span);
        let speed_bound = (1.0 + cfg.stir_fraction) * cfg.u_conv_max;

        let stirs = cfg.stir_fraction > 0.0;
        let n_octaves = if stirs {
            octaves_of(velocity)?
        } else {
            // Not dispatched, so the count is not derived either: an extent that
            // cannot carry an octave lattice is only an error for a scenario that
            // asked for stirring.
            0
        };
        // The noise potential is bounded by `stir_fraction*u_conv_max*dx_v/2`, and
        // the derivation is one line: `u_noise` is a narrow curl, so each component
        // reads four values of `A` at the gain `1/(2*dx_v)`, giving
        // `|u_noise| <= 4*|A|/(2*dx_v)`. The kernel weights octave `k` by `2^-k`
        // and does not normalise, so the sum of those weights is divided out here.
        let octave_weights: f64 = (0..n_octaves).map(|k| 0.5f64.powi(k as i32)).sum();
        let amplitude = if stirs {
            cfg.stir_fraction * cfg.u_conv_max * dx_velocity / (2.0 * octave_weights)
        } else {
            0.0
        };

        Ok(Self {
            potential: PotentialParams {
                cnx: enthalpy.nx(),
                cny: enthalpy.ny(),
                cnz: enthalpy.nz(),
                periodic_mask,
                r,
                // The one place `L`, the arm of the difference and the scale of the
                // enthalpy field are visible at once.
                conv_gain: Q::from_f64(mobility * wide_gain / cfg.units_per_joule),
            },
            interpolate: InterpolateParams {
                cnx: enthalpy.nx(),
                cny: enthalpy.ny(),
                cnz: enthalpy.nz(),
                vnx: velocity.nx(),
                vny: velocity.ny(),
                vnz: velocity.nz(),
                periodic_mask,
                ratio_log2,
                floor_is_closed,
            },
            curl: CurlParams {
                nx: velocity.nx(),
                ny: velocity.ny(),
                nz: velocity.nz(),
                periodic_mask,
                gain: Q::from_f64(curl_gain),
            },
            sample: SampleParams {
                nx: fine.nx(),
                ny: fine.ny(),
                nz: fine.nz(),
                vnx: velocity.nx(),
                vny: velocity.ny(),
                vnz: velocity.nz(),
                periodic_mask,
                ratio_log2: velocity_lod,
                courant_gain: Q::from_f64(cfg.dt / cfg.dx),
            },
            noise: NoiseParams {
                vnx: velocity.nx(),
                vny: velocity.ny(),
                vnz: velocity.nz(),
                n_octaves,
                tick: 0,
                run_key: 0,
                amplitude: Q::from_f64(amplitude),
                floor_is_closed,
            },
            stirs,
            mobility,
            stencil_l1_norm,
            speed_bound,
            n_coarse: enthalpy.n_voxels(),
            n_velocity: velocity.n_voxels(),
            n_voxels: fine.n_voxels(),
        })
    }

    /// The mobility `L`, m^3/(K*s). Derived from `u_conv_max` and the declared
    /// temperature range, never declared (ADR-069).
    #[inline]
    #[must_use]
    pub fn mobility(&self) -> f64 {
        self.mobility
    }

    /// The half-width of the wide difference, in cells of the **enthalpy** grid.
    #[inline]
    #[must_use]
    pub fn stencil_radius(&self) -> u32 {
        self.potential.r
    }

    /// How many octaves of noise the field carries: every wavelength from the
    /// domain down to four velocity cells, five at 64^3. Zero when stirring is off,
    /// because the kernel is then not dispatched at all.
    #[inline]
    #[must_use]
    pub fn octaves(&self) -> u32 {
        self.noise.n_octaves
    }

    /// `(1 + stir_fraction)*u_conv_max`: the number the load-time Courant check
    /// was made against, and the number no run-time measurement replaces.
    #[inline]
    #[must_use]
    pub fn speed_bound(&self) -> f64 {
        self.speed_bound
    }

    /// The `l1` norm of the composed stencil: the sum of the absolute
    /// coefficients of the map from a coarse temperature anomaly to one component
    /// of `u`, with `L` factored out.
    ///
    /// Convolved from the three stencils the kernels implement — the wide
    /// difference, the trilinear interpolation of the potential and the narrow
    /// curl — and **not** from the closed form of ADR-069, which was written for a
    /// composition without an interpolation stage. See the module header for what
    /// taking the formula instead would cost and why nothing would report it.
    #[inline]
    #[must_use]
    pub fn stencil_l1_norm(&self) -> f64 {
        self.stencil_l1_norm
    }

    /// Whether the noise kernel is dispatched at all.
    ///
    /// The branch is on the host and not in the kernel (ADR-069): stirring is 120
    /// draws per velocity cell, `3.1e7` hashes a tick, twice the price of a full
    /// seven-point pass over 128^3 — so a branch inside the kernel would leave the
    /// default costing everything it costs today. The error is invisible in the
    /// result and visible only in the price, which is why the flag is public.
    #[inline]
    #[must_use]
    pub fn stirs(&self) -> bool {
        self.stirs
    }

    /// The velocity field conserves matter and conserves energy.
    ///
    /// It follows from the signature rather than from physics: [`VelocityField::apply`]
    /// writes buffers of class `Q` only and takes the enthalpy as a shared slice.
    /// Not the only process of which that is true — diffusion, advection,
    /// settling, pressure and cell sorting conserve both axes as well (ADR-069).
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::Conserved,
        }
    }

    /// One application: the whole field, from the enthalpy of the previous tick to
    /// the Courant numbers on the faces of the fine grid.
    ///
    /// The buffers are the caller's, and there are five of them because the noise
    /// may not be added in place — that would be a read from the buffer being
    /// written (`.claude/rules/kernels.md`). `stirred` is untouched when stirring
    /// is off, which is how `the_noise_kernel_is_not_dispatched_at_zero_stir_fraction`
    /// sees the branch.
    ///
    /// # Panics
    ///
    /// Panics if a buffer does not have the shape this process was folded for.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &self,
        enthalpy: &[M64],
        heat_capacity: &[Q],
        coarse_potential: &mut [Q],
        potential: &mut [Q],
        stirred: &mut [Q],
        velocity: &mut [Q],
        face_courant: &mut [Q],
        tick: u32,
        run_key: u32,
    ) {
        assert_eq!(enthalpy.len(), self.n_coarse as usize, "the enthalpy field");
        assert_eq!(
            heat_capacity.len(),
            self.n_coarse as usize,
            "the heat capacity field lives on the same coarse grid as the enthalpy \
             (ADR-062), and the potential divides one by the other cell for cell"
        );
        assert_eq!(
            coarse_potential.len(),
            (COMPONENTS * self.n_coarse) as usize,
            "the coarse potential"
        );
        assert_eq!(
            potential.len(),
            (COMPONENTS * self.n_velocity) as usize,
            "the interpolated potential"
        );
        assert_eq!(stirred.len(), potential.len(), "the stirred potential");
        assert_eq!(velocity.len(), potential.len(), "the velocity field");
        assert_eq!(
            face_courant.len(),
            (COMPONENTS * (self.n_voxels + 1)) as usize,
            "the face Courant buffer is three numbers per **lane** of the fine \
             grid, in the layout kernels/advect.rs reads: `n_voxels` faces and \
             the face of the domain, which belongs to the ghost cell (ADR-059)"
        );

        for idx in 0..self.n_coarse {
            potential_voxel(
                enthalpy,
                heat_capacity,
                coarse_potential,
                &self.potential,
                idx,
            );
        }
        for idx in 0..self.n_velocity {
            interpolate_potential(coarse_potential, potential, &self.interpolate, idx);
        }

        // The branch of ADR-069, on the host. `A` is whichever buffer the last
        // writer used, and it is passed to the curl as a shared slice.
        let a: &[Q] = if self.stirs {
            let noise = NoiseParams {
                tick,
                run_key,
                ..self.noise
            };
            for idx in 0..self.n_velocity {
                stir_potential(potential, stirred, &noise, idx);
            }
            stirred
        } else {
            potential
        };

        for idx in 0..self.n_velocity {
            curl_voxel(a, velocity, &self.curl, idx);
        }

        debug_assert!(
            self.within_speed_bound(velocity),
            "the velocity field exceeded the bound {} m/s the validator checked. \
             This assertion is required never to fire (ADR-069): if it does, the \
             estimate ||D||_1 disagrees with the stencils the kernels implement, \
             and in release nothing at all would report it",
            self.speed_bound
        );

        for idx in 0..self.n_voxels {
            face_courant_voxel(velocity, face_courant, &self.sample, idx);
        }
    }

    /// The debug-only check ADR-069 puts **on the field**: three comparisons per
    /// velocity cell on the magnitude, plus the sum over the six outgoing faces.
    ///
    /// Not a reduction in the sense the record rejects: it computes nothing the
    /// tick then uses, it exists only in debug builds, and it is required never to
    /// fire. A firing assertion here is a bug in the derivation of `L`, not a mode
    /// of operation.
    #[cfg(debug_assertions)]
    fn within_speed_bound(&self, velocity: &[Q]) -> bool {
        let ceiling = self.speed_bound * (1.0 + 1e-3);
        let n = self.n_velocity as usize;
        for idx in 0..n {
            let mut outgoing = 0.0;
            for component in 0..COMPONENTS as usize {
                let value = velocity[component * n + idx].debug_f64().abs();
                if value > ceiling {
                    return false;
                }
                outgoing += value;
            }
            // The sum over the outgoing faces, which is what SPEC section 4.2 asks
            // for in its strict form; a divergence-free field has three of the six
            // faces outgoing, and each of the three components crosses one of them.
            if outgoing > 3.0 * ceiling {
                return false;
            }
        }
        true
    }

    #[cfg(not(debug_assertions))]
    fn within_speed_bound(&self, _velocity: &[Q]) -> bool {
        true
    }
}

/// How many bits of coarsening separate two grids, checked rather than declared.
fn lod_between(fine: &Grid, coarse: &Grid, role: &str) -> Result<u32> {
    for lod in 0..u32::BITS {
        if fine.nx() >> lod == coarse.nx()
            && fine.ny() >> lod == coarse.ny()
            && fine.nz() >> lod == coarse.nz()
            && coarse.nx() << lod == fine.nx()
            && coarse.ny() << lod == fine.ny()
            && coarse.nz() << lod == fine.nz()
        {
            return Ok(lod);
        }
    }
    bail!(
        "the {role} grid {}x{}x{} is not the fine grid {}x{}x{} coarsened by a \
         whole power of two on every axis (SPEC section 1.5 coarsens by a shift \
         **per axis**)",
        coarse.nx(),
        coarse.ny(),
        coarse.nz(),
        fine.nx(),
        fine.ny(),
        fine.nz()
    )
}

/// Every octave from the domain down to a wavelength of four cells.
///
/// Octave `k` has a lattice of `2^(k+1)` nodes per axis and a spacing of
/// `extent >> (k+1)`; both the wrap of the lattice and the exact fractions inside
/// a lattice cell need the extents to be powers of two, so a scenario that asks
/// for stirring on other extents is refused rather than given a seam.
fn octaves_of(velocity: &Grid) -> Result<u32> {
    let mut count = NOISE_OCTAVE_MAX;
    for (extent, axis) in [
        (velocity.nx(), "X"),
        (velocity.ny(), "Y"),
        (velocity.nz(), "Z"),
    ] {
        if !extent.is_power_of_two() || extent < 4 {
            bail!(
                "stirring is switched on and the velocity grid has {extent} cells \
                 along {axis}. The octave lattice of ADR-069 has 2^(k+1) nodes per \
                 axis, which is what makes every wavelength divide the domain and \
                 the field wrap in X and Y for nothing; on an extent that is not a \
                 power of two of at least four there is no such lattice and the \
                 torus grows a seam"
            );
        }
        // `extent >> n >= 2` for every octave used, so `n <= log2(extent) - 1`.
        count = count.min(extent.trailing_zeros() - 1);
    }
    Ok(count)
}

/// The `l1` norm of the composition "wide difference, trilinear interpolation,
/// narrow curl", convolved from the three coefficient lists.
///
/// The output is one component of `u` at one **phase** — the position of a
/// velocity cell inside the coarse cell covering it — and the norm is the worst
/// over the `ratio^3` phases and the three components. The worst phase is what the
/// bound has to hold at, and it is not the same phase for every component.
///
/// Computed on the periodic interior. A closed axis clamps the wide difference
/// (`kernels/potential.rs`) and folds the curl onto the cell itself, and both can
/// only merge coefficients of the same sign onto one cell or drop one — neither
/// of which raises the sum of absolute values. So the interior number is the
/// maximum.
fn composed_l1_norm(ratio: u32, r: i32, wide_gain: f64, curl_gain: f64) -> f64 {
    let mut worst = 0.0f64;

    for pz in 0..ratio as i32 {
        for py in 0..ratio as i32 {
            for px in 0..ratio as i32 {
                for component in 0..COMPONENTS {
                    // The narrow curl as (component of `A`, velocity offset,
                    // coefficient). `A_z` is identically zero for the convective
                    // potential, so the terms that read it are absent rather than
                    // written and multiplied by nothing.
                    let curl: &[(u32, [i32; 3], f64)] = match component {
                        0 => &[(1, [0, 0, 1], -curl_gain), (1, [0, 0, -1], curl_gain)],
                        1 => &[(0, [0, 0, 1], curl_gain), (0, [0, 0, -1], -curl_gain)],
                        _ => &[
                            (1, [1, 0, 0], curl_gain),
                            (1, [-1, 0, 0], -curl_gain),
                            (0, [0, 1, 0], -curl_gain),
                            (0, [0, -1, 0], curl_gain),
                        ],
                    };

                    let mut accumulated: BTreeMap<[i32; 3], f64> = BTreeMap::new();
                    for &(a_component, offset, coefficient) in curl {
                        let sample = [px + offset[0], py + offset[1], pz + offset[2]];
                        for (corner, weight) in interpolation_corners(sample, ratio as i32) {
                            // The wide difference: `A_x` reads `T` at `+-r` along
                            // Y, `A_y` at `+-r` along X with the opposite sign.
                            let wide: [([i32; 3], f64); 2] = if a_component == 0 {
                                [
                                    ([corner[0], corner[1] + r, corner[2]], wide_gain),
                                    ([corner[0], corner[1] - r, corner[2]], -wide_gain),
                                ]
                            } else {
                                [
                                    ([corner[0] + r, corner[1], corner[2]], -wide_gain),
                                    ([corner[0] - r, corner[1], corner[2]], wide_gain),
                                ]
                            };
                            for (cell, gain) in wide {
                                *accumulated.entry(cell).or_insert(0.0) +=
                                    coefficient * weight * gain;
                            }
                        }
                    }

                    let norm: f64 = accumulated.values().map(|c| c.abs()).sum();
                    worst = worst.max(norm);
                }
            }
        }
    }

    worst
}

/// The eight coarse corners a velocity cell interpolates from, and their weights.
///
/// The host-side twin of the arithmetic in `kernels/potential.rs`: the centre of
/// velocity cell `v` sits at `(v + 0.5)/ratio - 0.5` in continuous coarse-cell
/// coordinates, which is negative in the first half cell of every axis. Offsets
/// are relative and unbounded, because this function measures a stencil rather
/// than addressing a buffer.
fn interpolation_corners(sample: [i32; 3], ratio: i32) -> Vec<([i32; 3], f64)> {
    let mut base = [0i32; 3];
    let mut frac = [0f64; 3];
    for axis in 0..3 {
        let numerator = 2 * sample[axis] + 1 - ratio;
        let denominator = 2 * ratio;
        base[axis] = numerator.div_euclid(denominator);
        frac[axis] = f64::from(numerator.rem_euclid(denominator)) / f64::from(denominator);
    }

    let mut corners = Vec::with_capacity(8);
    for corner in 0..8u32 {
        let mut cell = [0i32; 3];
        let mut weight = 1.0f64;
        for axis in 0..3 {
            let high = (corner >> axis) & 1 == 1;
            cell[axis] = base[axis] + i32::from(high);
            weight *= if high { frac[axis] } else { 1.0 - frac[axis] };
        }
        corners.push((cell, weight));
    }
    corners
}

/// The six boundary conditions of the grid, as the bit mask the kernels read.
///
/// The same function `process/diffuse.rs` and `process/pressure.rs` carry,
/// refusing the same third case for the same reason.
fn periodic_mask(grid: &Grid) -> Result<u32> {
    let mut mask = 0u32;
    for face in Face::ALL {
        match grid.boundary(face) {
            Boundary::Periodic => mask |= 1 << (face as u32),
            Boundary::Closed => {}
            Boundary::Exchange => bail!(
                "face {face:?} is an exchange face, and this process has no \
                 behaviour on one. The counters exist and the ghost cell exists \
                 (ADR-059) — what does not exist is a decision about what this \
                 operator does at the face of the domain: `settling_out_of_the_\
                 top_face_appears_in_boundary_exchange` (`ACCEPTANCE.md`) names \
                 an outcome for settling and no mechanism, and pressure and the \
                 velocity field are named by nothing at all. Refused rather than \
                 treated as closed, because a lid sealed for one operator and \
                 vented for the others is a difference in the flux that reads as \
                 physics"
            ),
        }
    }
    Ok(mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The eco regime of SPEC section 1.7 shrunk by two on every axis, so that the
    /// three grids keep their relation: fine 32^3, velocity 16^3, enthalpy 8^3.
    const NX: u32 = 32;
    const DT: f64 = 1.0;
    const DX: f64 = 1.0e-4;
    const T_MIN: f64 = 273.15;
    const T_MAX: f64 = 323.15;

    fn floored(n: u32) -> Grid {
        Grid::new(
            n,
            n,
            n,
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

    fn config() -> VelocityConfig {
        VelocityConfig {
            // The ceiling of ADR-069 on the fine grid: `dx/(6*dt)`.
            u_conv_max: DX / (6.0 * DT),
            // Half the domain, which is `r = 4` cells of the enthalpy grid.
            l_c: f64::from(NX) * DX / 2.0,
            stir_fraction: 0.0,
            stir_period: None,
            dt: DT,
            dx: DX,
            t_min: T_MIN,
            t_max: T_MAX,
            units_per_joule: 1.0,
            every_n_ticks: 1,
        }
    }

    fn built(cfg: &VelocityConfig) -> VelocityField {
        VelocityField::new(&floored(NX), &floored(NX / 2), &floored(NX / 4), cfg).unwrap()
    }

    #[test]
    fn every_n_ticks_above_one_is_refused_for_the_velocity_field() {
        // ADR-069 by name. The field is not integrated, so the ban of ADR-030 does
        // not formally reach it; without a refusal, skipping ticks gives a quietly
        // late flow with the ledger closing exactly and nothing to notice it.
        let mut cfg = config();
        cfg.every_n_ticks = 2;
        let message = VelocityField::new(&floored(NX), &floored(NX / 2), &floored(NX / 4), &cfg)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains('2'),
            "the number has to be named: {message}"
        );
        assert!(message.contains("every_n_ticks"), "{message}");

        cfg.every_n_ticks = 1;
        assert!(VelocityField::new(&floored(NX), &floored(NX / 2), &floored(NX / 4), &cfg).is_ok());
    }

    #[test]
    fn the_stencil_radius_counts_cells_of_the_enthalpy_grid() {
        // `r = round(l_c/(2*dx_coarse))` with `dx_coarse` the step of the enthalpy
        // grid. Counted on the velocity grid the answer is exactly twice as large,
        // that is one octave off in the selected wavelength, and the field stays
        // smooth, divergence-free and inside its speed bound.
        let field = built(&config());
        let dx_coarse = DX * 4.0;
        assert_eq!(
            field.stencil_radius(),
            (f64::from(NX) * DX / 2.0 / (2.0 * dx_coarse)).round() as u32
        );
        // Half the domain is a quarter of the extent in cells: 8/4 = 2.
        assert_eq!(field.stencil_radius(), 2);

        // And the refusal below one cell names the coarse step.
        let mut cfg = config();
        cfg.l_c = DX;
        let message = VelocityField::new(&floored(NX), &floored(NX / 2), &floored(NX / 4), &cfg)
            .unwrap_err()
            .to_string();
        assert!(message.contains("enthalpy"), "{message}");
    }

    #[test]
    fn the_mobility_is_derived_from_the_composed_stencil_and_not_from_the_formula() {
        // The closed form of ADR-069, `L = u_conv_max*h*s/(t_max - t_min)`, is
        // written for the composition without the interpolation stage. The two
        // differ, and this test pins that they do — if they agreed, taking either
        // would be safe and the module header would be about nothing.
        let cfg = config();
        let field = built(&cfg);
        let span = T_MAX - T_MIN;

        assert!(
            (field.mobility() * field.stencil_l1_norm() * span - cfg.u_conv_max).abs() <= 1e-18
        );

        let h = DX * 2.0;
        let s = cfg.l_c;
        let closed_form = cfg.u_conv_max * h * s / span;
        assert!(
            (field.mobility() / closed_form - 1.0).abs() > 0.05,
            "the closed form {closed_form} and the composed mobility {} agree to \
             within 5%, so this fixture cannot tell them apart",
            field.mobility()
        );
    }

    #[test]
    fn the_noise_is_not_derived_at_all_when_stirring_is_off() {
        // The branch is on the host, so at `stir_fraction = 0` nothing about the
        // lattice is derived — not even the octave count, which is why a grid that
        // could not carry a lattice is only an error for a scenario that asked for
        // one.
        let field = built(&config());
        assert!(!field.stirs());
        assert_eq!(field.octaves(), 0);
        assert_eq!(field.speed_bound(), config().u_conv_max);

        let mut cfg = config();
        cfg.stir_fraction = 0.25;
        cfg.stir_period = Some(60.0);
        let stirred = built(&cfg);
        assert!(stirred.stirs());
        // 16 cells per axis: octaves from the domain down to four cells.
        assert_eq!(stirred.octaves(), 3);
        assert_eq!(stirred.speed_bound(), 1.25 * cfg.u_conv_max);
    }

    #[test]
    fn a_stirring_fraction_without_a_period_is_refused() {
        let mut cfg = config();
        cfg.stir_fraction = 0.5;
        assert!(
            VelocityField::new(&floored(NX), &floored(NX / 2), &floored(NX / 4), &cfg).is_err()
        );
        cfg.stir_period = Some(30.0);
        assert!(VelocityField::new(&floored(NX), &floored(NX / 2), &floored(NX / 4), &cfg).is_ok());
    }

    #[test]
    fn the_grids_have_to_be_related_by_a_whole_power_of_two() {
        let cfg = config();
        assert!(VelocityField::new(&floored(NX), &floored(12), &floored(NX / 4), &cfg).is_err());
        // And the enthalpy grid may not be finer than the velocity grid: the
        // potential is taken on the first and interpolated onto the second.
        assert!(
            VelocityField::new(&floored(NX), &floored(NX / 4), &floored(NX / 2), &cfg).is_err()
        );
    }

    #[test]
    fn an_empty_temperature_range_is_refused() {
        let mut cfg = config();
        cfg.t_max = cfg.t_min;
        assert!(
            VelocityField::new(&floored(NX), &floored(NX / 2), &floored(NX / 4), &cfg).is_err()
        );
    }
}
