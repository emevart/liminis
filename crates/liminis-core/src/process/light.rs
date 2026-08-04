//! The light process: who owns the column dispatch, and what the four
//! coefficients of Beer-Lambert are folded into.
//!
//! Step `a` of the tick order (SPEC section 8). Everything `kernels/light.rs` is
//! not allowed to know: the physical `k` of each attenuator, the depth of a
//! voxel, what one storage unit is worth, and which lane of which width class a
//! substance lives in.
//!
//! # It credits nothing, and that is the decision
//!
//! Light falls, matter absorbs it, and the energy has an address — the enthalpy
//! of the voxel (ADR-028). This process does not credit it, does not accumulate
//! it and does not touch a channel counter. ADR-049 settles who does: the fold of
//! step `i'` computes `I[z+1] - I[z]` **from the stored field** on the fly, and
//! no second field on the fine grid exists.
//!
//! Adding a credit to `SOLAR_IN` here would reverse that record without saying
//! so, and the failure would be an exact doubling of the solar term — the fold
//! would go on crediting the same joules out of the same field, and the energy
//! residual would stay at zero while it happened, because the counter and the
//! enthalpy move together. `the_light_process_credits_nothing` is what stands in
//! the way, and the signature is the other half of it: [`Light::apply`] takes the
//! amounts as shared slices and can write nothing of class `M` at all.
//!
//! # The order is world semantics
//!
//! Light is step `a` and the fold is step `i'`, so the field the fold reads
//! belongs to the **current** tick (ADR-049). Swapping the two lags the light by
//! one tick with the energy ledger still closing exactly, and the discrepancy is
//! visible only in a transient and only on a plot. That ordering is why this file
//! moves `WORLD_FORMAT_VERSION` even though it changes no formula.
//!
//! # One number, one source
//!
//! `i_surface` is folded once, here, and handed to both consumers: the kernel
//! through `LightParams` and the fold of step `i'` through [`Light::i_surface`].
//! Folded twice, the two would differ in the last bit; that moves the absorption
//! of the topmost voxel and only that voxel, and `SOLAR_IN` is computed by the
//! same fold out of the same number, so the energy ledger closes exactly while
//! the two disagree.
//!
//! Modulation (ADR-076) makes that a statement about the **tick** and not only
//! about the constructor. [`Light::set_tick`] is the one place the day and the
//! season are evaluated: it rewrites `params.i_surface`, and both consumers read
//! it from there. The tick number has to reach step `a` and step `i'` and both
//! have to take one number — a disagreement of one tick lands on the absorption
//! of the topmost voxel and nowhere else, with `SOLAR_IN` credited by the same
//! fold out of the same number, so the residual stays at zero while they drift.
//! `the_incident_irradiance_has_one_source` is what stands there, and it is a
//! statement about a sequence of ticks for that reason.
//!
//! The multiplier is normalised to unit mean **over the samples the run actually
//! takes**, not over the continuous envelope, so that `i_surface` means "the mean
//! irradiance over a period" exactly rather than approximately. The continuous
//! `1/pi` would run 5.19% low at a period of eight ticks and 0.574% low at
//! twenty-four — the choice of period would move the energy budget of the world
//! by percent, with the profile staying a plausible sine (ADR-076).

use anyhow::{Result, bail};

use super::{Conservation, Invariant};
use crate::kernels::light::{Attenuators, LightParams, light_column};
use crate::numeric::{M32, M64, Q};
use crate::world::{Grid, LaneRef};

/// The default of `enabled` for the light, and it is `false` (ADR-065, ADR-076).
///
/// **Assigned by a record, and not a placeholder.** ADR-076 declares `i_surface`
/// and derives the multiplier the fold wanted, so the reason this value used to
/// carry — "the fold cannot be built" — is gone.
///
/// So are the two that replaced it, and the value is `false` all the same. Both
/// locks of ADR-076 were lifted in one pack: ADR-084 ratifies the built
/// `exchange` face as the energy sink of S0, and ADR-083 makes the counter
/// `i128`. The blanket refusal those two fed is gone from `config/validate.rs`,
/// and two narrow ones — a lit sealed box, and a steady state past the declared
/// range — stand where it did.
///
/// The reason that is left is the one that survives all of that: **step `a` is
/// still not dispatched**. `Tick::new` refuses an enabled light process because
/// the attenuator table this module's `Light::new` wants can be built from
/// nothing — the measure `Attenuator::conc_per_unit` multiplies is settled by no
/// document (`TODO(attenuation-measure)` below). A default of `true` here would
/// therefore refuse every scenario in the repository at tick assembly,
/// `hello.toml` first, which is exactly the failure ADR-065 argued against when
/// it rejected a blanket `enabled = true`.
///
/// And the repair that suggests itself for that — crediting `SOLAR_IN` from
/// inside this process, so it needs no fold — would reverse ADR-049, which
/// `the_light_process_credits_nothing` exists to prevent. That has not changed
/// with any of the above.
pub const ENABLED_BY_DEFAULT: bool = false;

/// The daily sample of the modulation at a tick: `max(0, sin(2*pi*n/N))`.
///
/// **One implementation, two callers**, and that is the point of it being here
/// rather than inside either. `config/derive.rs` sums these to get the discrete
/// normalisation `A_N` at load; [`Modulation`] takes one of them per tick. Two
/// transcriptions of one formula would put the normalisation and the multiplier
/// slightly out of step, and the symptom would be an `i_surface` that no longer
/// means "the mean over a period" — by a hair rather than by a percent, and so
/// invisible.
///
/// Exact at the quarter turns, and that is not decoration: `f64::sin` of the
/// argument that stands for `pi` returns `1.22e-16`, not zero. Left as it comes,
/// the normalisation would depend on the noise of the library — and at `N = 2`,
/// where both samples are meant to be zero, the sum would be `1.22e-16` instead
/// of zero and `A_2` would come out at `1.6e16` rather than dividing by zero.
/// A silently enormous multiplier is worse than a division that fails loudly, and
/// the minimum period of three ticks is argued from the division (ADR-076).
///
/// Zero at `period_ticks == 0`, which is the no-modulation case: the fraction is
/// then zero as well and the multiplier is identically one.
#[must_use]
pub fn daily_sample(tick: u32, period_ticks: u32) -> f64 {
    sample(tick, period_ticks).max(0.0)
}

/// The seasonal sample at a tick: `sin(2*pi*n/N)`, signed.
///
/// It needs no normalisation of its own — `sum sin` over a whole number of ticks
/// is an exact zero, so `1 + f_s*sin` has unit mean by construction — which is
/// why the daily half has an `A_N` and this one has none.
#[must_use]
pub fn seasonal_sample(tick: u32, period_ticks: u32) -> f64 {
    sample(tick, period_ticks)
}

/// `sin(2*pi*(tick mod period)/period)`, exact at the quarter turns.
fn sample(tick: u32, period_ticks: u32) -> f64 {
    if period_ticks == 0 {
        return 0.0;
    }
    let step = tick % period_ticks;
    // The quarter turns, written out: `sin` of the nearest `f64` to a multiple of
    // `pi/2` is not the exact value, and the two that matter here are the zeros.
    let quarters = 4 * u64::from(step);
    if quarters % u64::from(period_ticks) == 0 {
        return match (quarters / u64::from(period_ticks)) % 4 {
            0 | 2 => 0.0,
            1 => 1.0,
            _ => -1.0,
        };
    }
    (std::f64::consts::TAU * f64::from(step) / f64::from(period_ticks)).sin()
}

/// The daily and seasonal modulation of the incident irradiance (ADR-076).
///
/// Folded by the loader, never by this process: `daily_norm` is the discrete
/// `A_N = N / sum_{n<N} max(0, sin(2*pi*n/N))`, and recomputing it here would be
/// the second construction of a number whose whole purpose is to make one
/// statement exact.
///
/// Phase zero — dawn at tick zero — and there is no phase key: a phase is
/// equivalent to starting the scenario earlier, and a key for it would be a
/// second way to spell one world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modulation {
    /// Amplitude of the daily term as a fraction of the mean, `[0, 1]`.
    pub daily_fraction: f64,
    /// The daily period in whole ticks, or zero for no daily modulation.
    pub daily_period_ticks: u32,
    /// `A_N`, computed by the loader. One when there is no daily modulation.
    pub daily_norm: f64,
    /// Amplitude of the seasonal term as a fraction of the mean, `[0, 1]`.
    pub seasonal_fraction: f64,
    /// The seasonal period in whole ticks, or zero for no seasonal modulation.
    pub seasonal_period_ticks: u32,
}

impl Modulation {
    /// A world with neither a day nor a season: the multiplier is identically
    /// one.
    pub const NONE: Self = Self {
        daily_fraction: 0.0,
        daily_period_ticks: 0,
        daily_norm: 1.0,
        seasonal_fraction: 0.0,
        seasonal_period_ticks: 0,
    };

    /// `m_d(n) * m_s(n)` at a tick — the factor the declared mean irradiance is
    /// multiplied by.
    ///
    /// `m_d(n) = 1 - f_d + f_d*A_N*max(0, sin(2*pi*n/N))` and
    /// `m_s(n) = 1 + f_s*sin(2*pi*n/N_s)`, both with unit mean over their period.
    #[must_use]
    pub fn multiplier(&self, tick: u32) -> f64 {
        let daily = 1.0 - self.daily_fraction
            + self.daily_fraction * self.daily_norm * daily_sample(tick, self.daily_period_ticks);
        let seasonal =
            1.0 + self.seasonal_fraction * seasonal_sample(tick, self.seasonal_period_ticks);
        daily * seasonal
    }
}

/// One substance that darkens the water, as the scenario declares it.
///
/// Three numbers rather than one folded coefficient, because the folding is this
/// file's job and the point of the type is that the caller cannot do it wrongly:
/// `dz` is not here at all, and it is the factor a hand-folded coefficient loses.
#[derive(Clone, Copy, Debug)]
pub struct Attenuator {
    /// Which buffer the amounts are in: a width class and a lane inside it
    /// (ADR-056). Never a substance index — `World::lane_of` is the one door.
    pub lane: LaneRef,
    /// The attenuation coefficient, 1/m (`QUANTITIES.md` section 5).
    pub k: f64,
    /// What one storage unit of this substance is worth as the dimensionless
    /// measure `k` multiplies.
    // TODO(attenuation-measure): which measure that is has not been settled.
    // `QUANTITIES.md` section 5 gives `k` in 1/m, which forces the other factor to
    // be dimensionless, and the candidate consistent with the corpus is a volume
    // fraction through the partial molar volumes of ADR-067. Nothing here is
    // blocked by the question — the product is what the kernel sees — but the
    // caller has to know which number it is passing, and today no document says.
    pub conc_per_unit: f64,
}

/// The light process: the folded parameters of one tick, and the attenuator
/// table.
///
/// Built once, from the scenario. The four coefficients `k_w`, `k_b`, `k_d` and
/// `k_m` of SPEC section 4.6 are not a fixed list here: an attenuator is a row of
/// a table, so a substance added by TOML darkens the water without a line of Rust
/// (ADR-018).
#[derive(Clone, Debug)]
pub struct Light {
    params: LightParams,
    lane: Vec<u32>,
    coeff: Vec<Q>,
    /// The declared **mean** irradiance, W/m^2, kept unmodulated so that
    /// [`Light::set_tick`] can refold it. Folding the modulated value back into
    /// itself would drift the mean over a run.
    i_surface_mean: f64,
    modulation: Modulation,
}

impl Light {
    /// Fold the attenuator table and the incident irradiance.
    ///
    /// `i_surface_mean` is the irradiance on the top face of the topmost voxel,
    /// W/m^2, and it is the **mean over a modulation period** rather than an
    /// instantaneous value: the day and the season are applied here, once per
    /// tick, by [`Light::set_tick`]. `dz` is the voxel edge along Z, in metres.
    ///
    /// `dz` is an argument rather than being assumed to be one, and it is the
    /// factor whose absence is invisible: at the eco regime's `dx = 1e-4 m` the
    /// optical depth moves by four orders of magnitude, so the beam either dies
    /// inside the first voxel or never dies at all — and both pictures read as
    /// "the coefficients are not calibrated yet".
    ///
    /// # Errors
    ///
    /// Returns an error if a coefficient or the irradiance is not a usable
    /// number, or if the table is longer than the `width_mask` of the kernel can
    /// address.
    /// The irradiance is the scenario's now (ADR-076: `i_surface`, W/m^2, on the
    /// light process), and the modulation is the loader's: `Modulation::daily_norm`
    /// is the discrete `A_N` and is never recomputed here.
    pub fn new(
        grid: &Grid,
        attenuators: &[Attenuator],
        i_surface_mean: f64,
        modulation: Modulation,
        dz: f64,
    ) -> Result<Self> {
        if !i_surface_mean.is_finite() {
            bail!("the incident irradiance {i_surface_mean} W/m^2 is not a usable number");
        }
        if !modulation.daily_norm.is_finite() || !modulation.multiplier(0).is_finite() {
            bail!(
                "the modulation folds to an unusable multiplier: A_N = {}, \
                 f_d = {} over {} ticks, f_s = {} over {} ticks",
                modulation.daily_norm,
                modulation.daily_fraction,
                modulation.daily_period_ticks,
                modulation.seasonal_fraction,
                modulation.seasonal_period_ticks
            );
        }
        if !dz.is_finite() || dz <= 0.0 {
            bail!("dz {dz} m is not a usable voxel edge");
        }
        if attenuators.len() > u32::BITS as usize {
            bail!(
                "{} attenuators against a width mask of {} bits: the bit indexes \
                 the table entry (ADR-040), so the table cannot be longer than the \
                 mask",
                attenuators.len(),
                u32::BITS
            );
        }

        let mut lane = Vec::with_capacity(attenuators.len());
        let mut coeff = Vec::with_capacity(attenuators.len());
        let mut width_mask = 0u32;

        for (entry, attenuator) in attenuators.iter().enumerate() {
            if !attenuator.k.is_finite() || !attenuator.conc_per_unit.is_finite() {
                bail!(
                    "attenuator {entry} declares k = {} 1/m and {} per unit, which \
                     is not a usable pair",
                    attenuator.k,
                    attenuator.conc_per_unit
                );
            }
            // The one place `k`, `dz` and the per-unit measure are visible at
            // once. Past this line they exist only as one number, and the kernel
            // cannot recombine them in the wrong order because it never sees them
            // (`ARCHITECTURE.md`).
            coeff.push(Q::from_f64(attenuator.k * dz * attenuator.conc_per_unit));
            match attenuator.lane {
                LaneRef::Narrow(index) => lane.push(index),
                LaneRef::Wide(index) => {
                    lane.push(index);
                    width_mask |= 1 << entry;
                }
            }
        }

        Ok(Self {
            params: LightParams {
                nx: grid.nx(),
                ny: grid.ny(),
                nz: grid.nz(),
                n_voxels: grid.n_voxels(),
                lane_len: grid.lane_len(),
                n_attenuators: attenuators.len() as u32,
                width_mask,
                // Tick zero, through the one door: `set_tick` below is the only
                // place the multiplier is ever evaluated, and the constructor is
                // not allowed to be a second one.
                i_surface: Q::from_f64(i_surface_mean * modulation.multiplier(0)),
            },
            lane,
            coeff,
            i_surface_mean,
            modulation,
        })
    }

    /// Move the process to a tick, refolding the incident irradiance.
    ///
    /// **The one place the modulation is evaluated.** Both consumers — the kernel
    /// through `LightParams` and the fold of step `i'` through
    /// [`Light::i_surface`] — read the number this writes, so there is no second
    /// path along which they could take different ticks or different last bits.
    /// ADR-076 rejected the alternative in as many words: a kernel handed the tick
    /// number and the period would compute the modulation twice, and the
    /// disagreement would land on the absorption of the topmost voxel alone, with
    /// the energy residual staying at zero throughout.
    ///
    /// Refolded from the declared **mean** every time, never from the previous
    /// tick's value: a multiplier applied to an already-modulated number would
    /// compound, and the run's mean would drift away from the declared one while
    /// every profile still looked like a day.
    pub fn set_tick(&mut self, tick: u32) {
        self.params.i_surface = Q::from_f64(self.i_surface_mean * self.modulation.multiplier(tick));
    }

    /// The irradiance on the top face of the topmost voxel **at the current
    /// tick**, as the kernel has it.
    ///
    /// **The same `Q` has to reach `FoldParams::i_surface`.** It is the one term
    /// of the absorption of the topmost voxel that is not in the field (ADR-049),
    /// so a second folding of the same physical number puts a last-bit difference
    /// into that voxel and nowhere else. Since ADR-076 the number depends on the
    /// tick as well as on the scenario, so "the same number" is a claim about a
    /// sequence and not about a constructor.
    #[inline]
    #[must_use]
    pub fn i_surface(&self) -> Q {
        self.params.i_surface
    }

    /// The modulation this process was folded with.
    #[inline]
    #[must_use]
    pub fn modulation(&self) -> Modulation {
        self.modulation
    }

    /// How many invocations one application costs: `nx*ny`, one per column.
    ///
    /// Public so that the dispatch domain is checkable from a test and not only
    /// from prose. `kernels/light.rs` names the quiet half of that mistake: a host
    /// that dispatches per column and decodes `idx` as a voxel index produces the
    /// **identical** field, because the first `nx*ny` voxel indices are exactly
    /// the plane `z = 0`. No assertion over values can see it; only the count can.
    #[inline]
    #[must_use]
    pub fn columns(&self) -> u32 {
        self.params.nx * self.params.ny
    }

    /// The folded parameters, as the kernel receives them.
    #[inline]
    #[must_use]
    pub fn params(&self) -> LightParams {
        self.params
    }

    /// Light conserves matter and conserves energy — as a statement about *this
    /// process*, and it follows from the signature rather than from physics.
    ///
    /// [`Light::apply`] writes one buffer of class `Q` and takes the amounts as
    /// shared slices: it cannot change a quantity either ledger counts. The energy
    /// the beam loses is credited by the fold of step `i'` and by nothing here
    /// (ADR-049), so `SOLAR_IN` does not appear in this file.
    ///
    /// The second arm of [`Conservation`] would be needed the day this process
    /// credited a channel, and it is exactly because it does not that the arm is
    /// still missing (`process/mod.rs`).
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::Conserved,
        }
    }

    /// One application: the whole light field, rewritten out of state `N` of the
    /// amounts.
    ///
    /// Dispatched over **columns**, `0..nx*ny`, and never over voxels — see
    /// [`Light::columns`]. The field is single buffered and needs no swap: it is
    /// rewritten in full every tick, and the price of that is the one
    /// `kernels/light.rs` names — a column nobody visited holds the previous
    /// tick's value, and the fold of step `i'` credits energy off it without
    /// complaint.
    ///
    /// # Panics
    ///
    /// Panics if the light field does not have the shape this process was folded
    /// for. That is a programming error rather than a bad scenario, and it would
    /// otherwise show up as a quietly wrong world.
    pub fn apply(&self, src32: &[M32], src64: &[M64], light: &mut [Q]) {
        assert_eq!(
            light.len(),
            self.params.n_voxels as usize,
            "the light field does not have the shape this process was folded for"
        );

        let attenuators = Attenuators {
            lane: &self.lane,
            coeff: &self.coeff,
        };
        for idx in 0..self.columns() {
            light_column(src32, src64, light, &attenuators, &self.params, idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{Channel, Ledger};
    use crate::world::Boundary;

    const NX: u32 = 3;
    const NY: u32 = 4;
    const NZ: u32 = 5;
    const N_VOXELS: u32 = NX * NY * NZ;
    const DZ: f64 = 0.05;
    const INCIDENT: f64 = 137.0;

    fn grid() -> Grid {
        Grid::new(NX, NY, NZ, [Boundary::Closed; 6]).unwrap()
    }

    /// Two attenuators in two width classes, with lanes that are deliberately not
    /// the table entries (ADR-056).
    fn table() -> Vec<Attenuator> {
        vec![
            Attenuator {
                lane: LaneRef::Narrow(1),
                k: 1.5,
                conc_per_unit: 4.0e-8,
            },
            Attenuator {
                lane: LaneRef::Wide(0),
                k: 0.8,
                conc_per_unit: 1.75e-10,
            },
        ]
    }

    /// The lane stride of a `world::Field` over this grid: the voxels and the
    /// ghost cell after them (ADR-059).
    const LANE_LEN: u32 = N_VOXELS + 1;

    fn amounts() -> (Vec<M32>, Vec<M64>) {
        let mut src32 = vec![M32::ZERO; (2 * LANE_LEN) as usize];
        let mut src64 = vec![M64::ZERO; LANE_LEN as usize];
        for at in 0..N_VOXELS as usize {
            src32[LANE_LEN as usize + at] = M32::new(12_000_000);
            src64[at] = M64::new(2_000_000_000);
        }
        (src32, src64)
    }

    #[test]
    fn light_is_dispatched_over_columns_not_voxels() {
        // The count and nothing but the count. `kernels/light.rs` names the pair
        // of mistakes: dispatching per voxel is loud (past `nx*ny` the decode runs
        // off the end of the field), and dispatching per column while decoding
        // `idx` as a voxel index gives the **identical** field, because the first
        // `nx*ny` voxel indices are exactly the plane `z = 0`.
        let light = Light::new(&grid(), &table(), INCIDENT, Modulation::NONE, DZ).unwrap();
        assert_eq!(light.columns(), NX * NY);
        assert_ne!(light.columns(), N_VOXELS);

        // And every voxel is written, which is the half the count alone does not
        // give: a dispatch over `nx*ny` that wrote only its own cell would leave
        // the field at its previous value above `z = 0`.
        let (src32, src64) = amounts();
        let mut field = vec![Q::ZERO; N_VOXELS as usize];
        light.apply(&src32, &src64, &mut field);
        for (at, value) in field.iter().enumerate() {
            assert_ne!(*value, Q::ZERO, "voxel {at} was never visited");
        }
    }

    #[test]
    fn the_light_process_credits_nothing() {
        // ADR-049: absorption is derived and is credited by the fold of step `i'`
        // out of this same field. A counter incremented here would credit the same
        // joules twice, and the energy residual would stay at zero while it
        // happened, because the counter and the enthalpy move together.
        let light = Light::new(&grid(), &table(), INCIDENT, Modulation::NONE, DZ).unwrap();
        let (src32, src64) = amounts();
        let before_32 = src32.clone();
        let before_64 = src64.clone();

        let mut ledger = Ledger::new(2).unwrap();
        ledger.begin_tick();
        let mut field = vec![Q::ZERO; N_VOXELS as usize];
        light.apply(&src32, &src64, &mut field);

        assert_eq!(src32, before_32);
        assert_eq!(src64, before_64);
        for channel in Channel::ALL {
            assert_eq!(ledger.energy(channel), 0, "channel {channel:?}");
            for substance in 0..2 {
                assert_eq!(ledger.matter(channel, substance), 0);
            }
        }

        assert_eq!(
            light.invariant(),
            Invariant {
                matter: Conservation::Conserved,
                energy: Conservation::Conserved,
            }
        );
    }

    /// A day of four ticks at full amplitude: `A_4 = 4`, samples `0, 1, 0, 0`.
    ///
    /// Four rather than something rounder because it is the sharpest legal case —
    /// the peak is exactly `4.0`, 27% above `pi` — and because three quarters of
    /// its ticks are dark, so a modulation that never ran is not mistakable for
    /// one that did.
    fn four_tick_day() -> Modulation {
        Modulation {
            daily_fraction: 1.0,
            daily_period_ticks: 4,
            daily_norm: 4.0,
            ..Modulation::NONE
        }
    }

    #[test]
    fn the_incident_irradiance_has_one_source() {
        // The same `Q` reaches the kernel and whoever folds step `i'`. Two
        // foldings of one physical number differ in the last bit, and that
        // difference lands on the absorption of the topmost voxel and nowhere
        // else — with `SOLAR_IN` computed by the same fold out of the same number,
        // so the energy ledger closes exactly.
        //
        // A statement about a **sequence of ticks** since ADR-076, not about the
        // constructor: modulation adds a second way for the two to come apart,
        // because the tick number has to travel to step `a` and to step `i'` and
        // both have to take one number. A disagreement of one tick is invisible in
        // both residuals for exactly the reason above.
        let mut light = Light::new(&grid(), &table(), INCIDENT, four_tick_day(), DZ).unwrap();

        let mut seen = Vec::new();
        for tick in 0..12 {
            light.set_tick(tick);
            // The kernel's copy and the fold's copy, at the same tick.
            assert_eq!(light.i_surface(), light.params().i_surface, "tick {tick}");
            seen.push(light.i_surface());
        }

        // And the sequence is not a constant, or the equality above would hold on
        // a process that ignored the tick entirely.
        assert!(
            seen.iter().any(|q| *q != seen[0]),
            "the irradiance never moved; the claim is vacuous"
        );

        // The period is the period: tick `n` and tick `n + N` agree exactly.
        for tick in 0..8 {
            assert_eq!(
                seen[tick],
                seen[tick + 4],
                "tick {tick} against tick {}",
                tick + 4
            );
        }
    }

    #[test]
    fn the_modulation_is_evaluated_in_one_place() {
        // `set_tick` is the only door, and the constructor goes through it too:
        // a freshly built process is at tick zero and not at the unmodulated mean.
        // The two differ whenever tick zero is not a mean sample — at a four-tick
        // day, dawn is dark, and a constructor that folded the bare mean would
        // hand the first tick four times too much light.
        let mut light = Light::new(&grid(), &table(), INCIDENT, four_tick_day(), DZ).unwrap();
        let built = light.i_surface();
        light.set_tick(0);
        assert_eq!(built, light.i_surface());
        assert_ne!(built, Q::from_f64(INCIDENT));

        // Refolded from the declared mean each time, never compounded: coming back
        // to a tick gives the same number however many ticks were visited between.
        light.set_tick(1);
        let peak = light.i_surface();
        for tick in [2, 3, 7, 100, 1] {
            light.set_tick(tick);
        }
        assert_eq!(peak, light.i_surface());
        assert_eq!(light.modulation(), four_tick_day());
    }

    #[test]
    fn daily_modulation_preserves_the_period_mean_irradiance_exactly() {
        // `ACCEPTANCE.md`, ADR-076. The mean of the multiplier over one period is
        // one, which is what makes `i_surface` mean "the mean irradiance over a
        // period" rather than "roughly that". Only the **discrete** normalisation
        // `A_N` gives it: with the continuous `1/pi` the mean runs 5.19% low at a
        // period of eight ticks and 0.574% low at twenty-four — the choice of
        // period would move the energy budget of the world by percent.
        //
        // Short periods, deliberately: at 86 400 ticks the continuous
        // normalisation is off by `4.4e-10` and any tolerance swallows it. Three
        // is the shortest legal period, four is the sharpest.
        //
        // **Named honestly: this is not a bit-exact equality, and it cannot be.**
        // `A_N` is `N/sum` in `f64` and the mean multiplies it back by `sum/N`, so
        // the round trip is exact only when the two roundings cancel; and the
        // process hands the result to `Q`, which is `f32` in FLOAT mode. What the
        // tolerance below has to do is separate the discrete normalisation from
        // the continuous one, and `1e-12` is nine orders below the smallest
        // deviation `1/pi` produces at these periods.
        const TOLERANCE: f64 = 1e-12;

        for period in [3u32, 4, 8, 24] {
            let norm = discrete_norm(period);
            let modulation = Modulation {
                daily_fraction: 1.0,
                daily_period_ticks: period,
                daily_norm: norm,
                ..Modulation::NONE
            };
            let mean: f64 =
                (0..period).map(|n| modulation.multiplier(n)).sum::<f64>() / f64::from(period);
            assert!(
                (mean - 1.0).abs() < TOLERANCE,
                "a day of {period} ticks has period mean {mean}, not 1"
            );

            // And the continuous normalisation would fail this by percent, which
            // is what the tolerance above is there to tell apart.
            let continuous = Modulation {
                daily_norm: std::f64::consts::PI,
                ..modulation
            };
            let off: f64 =
                (0..period).map(|n| continuous.multiplier(n)).sum::<f64>() / f64::from(period);
            assert!(
                (off - 1.0).abs() > 1e-3,
                "1/pi is indistinguishable from A_N at {period} ticks, so this \
                 test proves nothing there"
            );
        }
    }

    #[test]
    fn the_seasonal_mean_needs_no_normalisation() {
        // `sum sin` over a whole number of ticks is an exact zero, so
        // `1 + f_s*sin` has unit mean by construction. That asymmetry with the
        // daily half is the reason only one of them carries an `A_N`.
        for period in [3u32, 4, 8, 24] {
            let modulation = Modulation {
                seasonal_fraction: 1.0,
                seasonal_period_ticks: period,
                ..Modulation::NONE
            };
            let mean: f64 =
                (0..period).map(|n| modulation.multiplier(n)).sum::<f64>() / f64::from(period);
            assert!(
                (mean - 1.0).abs() < 1e-12,
                "a season of {period} ticks: {mean}"
            );
        }
    }

    #[test]
    fn a_two_tick_day_has_no_normalisation_at_all() {
        // The arithmetic behind the three-tick minimum, checked rather than
        // asserted in prose: at `N = 2` the samples are taken at phases `0` and
        // `pi`, both exactly zero, so the sum is zero and `A_2` divides by it.
        //
        // Exactly zero matters. `f64::sin` of the argument standing for `pi`
        // returns `1.22e-16`, and taken as it comes the sum would be that instead
        // — `A_2` would be `1.6e16` rather than an infinity, and a silently
        // enormous multiplier is worse than a division that fails loudly.
        assert_eq!(daily_sample(0, 2), 0.0);
        assert_eq!(daily_sample(1, 2), 0.0);
        assert!(!discrete_norm(2).is_finite());

        // Three ticks is degenerate and alive: `A_3 = 3/0.8660 = 3.464`.
        assert!((discrete_norm(3) - 3.4641016151377544).abs() < 1e-12);
    }

    /// `A_N` the way the loader computes it, reproduced here from the definition
    /// so that these tests do not agree with `config/derive.rs` by construction.
    fn discrete_norm(period: u32) -> f64 {
        let sum: f64 = (0..period).map(|n| daily_sample(n, period)).sum();
        f64::from(period) / sum
    }

    #[test]
    fn the_voxel_depth_is_inside_the_folded_coefficient() {
        // `dz` is the factor a hand-folded coefficient loses, and losing it is
        // invisible in the shape of the profile: only the total optical depth
        // moves, which reads as an uncalibrated `k`.
        let shallow = Light::new(&grid(), &table(), INCIDENT, Modulation::NONE, DZ).unwrap();
        let deep = Light::new(&grid(), &table(), INCIDENT, Modulation::NONE, 2.0 * DZ).unwrap();
        let (src32, src64) = amounts();

        let mut thin = vec![Q::ZERO; N_VOXELS as usize];
        let mut thick = vec![Q::ZERO; N_VOXELS as usize];
        shallow.apply(&src32, &src64, &mut thin);
        deep.apply(&src32, &src64, &mut thick);

        assert!(thick[0] < thin[0]);
    }

    #[test]
    fn the_width_mask_indexes_the_table_entry() {
        // Bit `a` is set when entry `a` reads the wide buffer, so the branch is
        // uniform across the dispatch (ADR-040). Indexed by lane or by substance
        // instead, the mask points at the wrong entry and the kernel attenuates by
        // whatever the other slice holds — with `I(z)` staying monotone and
        // entirely plausible.
        let light = Light::new(&grid(), &table(), INCIDENT, Modulation::NONE, DZ).unwrap();
        assert_eq!(light.params().width_mask, 0b10);
        assert_eq!(light.params().n_attenuators, 2);
    }

    #[test]
    fn an_unusable_coefficient_is_refused_at_load() {
        let bad = vec![Attenuator {
            lane: LaneRef::Narrow(0),
            k: f64::NAN,
            conc_per_unit: 1.0,
        }];
        assert!(Light::new(&grid(), &bad, INCIDENT, Modulation::NONE, DZ).is_err());
        assert!(Light::new(&grid(), &table(), INCIDENT, Modulation::NONE, 0.0).is_err());
        assert!(Light::new(&grid(), &table(), f64::INFINITY, Modulation::NONE, DZ).is_err());
    }
}
