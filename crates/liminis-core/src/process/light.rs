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

use anyhow::{Result, bail};

use super::{Conservation, Invariant};
use crate::kernels::light::{Attenuators, LightParams, light_column};
use crate::numeric::{M32, M64, Q};
use crate::world::{Grid, LaneRef};

/// The default of `enabled` for the light, and it is `false` (ADR-065).
///
/// Not a statement about how interesting a dark world is. Step `a` computes the
/// field and step `i'` folds it into enthalpy and credits `SOLAR_IN` — and the
/// fold cannot be dispatched: `FoldParams::i_surface` and
/// `FoldParams::joules_per_intensity` are named by no key and no record
/// (`TODO(joules-per-intensity)` in `kernels/fold.rs`), and what the fold owes
/// the ledger is open question A-16. A default of `true` would put light into
/// the enthalpy of every scenario at a scale nobody chose, and calibrating `k`
/// against it would hide the invention for good.
///
/// The mirror argument is worth stating, because it is the one that will
/// overturn this value: `LightParams::i_surface` and `FoldParams::i_surface`
/// have to be the *same number* folded by the host, and the day they are, light
/// on by default is the natural reading of SPEC section 4.6.
// TODO(CONFIG_SCHEMA.md section 13 item 23): assigned by no record. See the
// module header of `kernels/fold.rs` for what has to be decided first, and
// `DECISIONS.md` for where the answer goes.
pub const ENABLED_BY_DEFAULT: bool = false;

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
}

impl Light {
    /// Fold the attenuator table and the incident irradiance.
    ///
    /// `i_surface` is the irradiance on the top face of the topmost voxel, W/m^2,
    /// already modulated by the day and the season by the caller; `dz` is the
    /// voxel edge along Z, in metres.
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
    // TODO(i-surface): the incident irradiance is a key of nothing.
    // `CONFIG_SCHEMA.md` section 7 lists exactly `k_w`, `k_b`, `k_d` and `k_m`;
    // `QUANTITIES.md` section 5 has a row for `I` and none for `I0`, an amplitude
    // or a period. So it arrives as an argument rather than being read from a
    // config that has no place for it, and a plausible number invented here would
    // be indistinguishable from a decision.
    pub fn new(grid: &Grid, attenuators: &[Attenuator], i_surface: f64, dz: f64) -> Result<Self> {
        if !i_surface.is_finite() {
            bail!("the incident irradiance {i_surface} W/m^2 is not a usable number");
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
                n_attenuators: attenuators.len() as u32,
                width_mask,
                i_surface: Q::from_f64(i_surface),
            },
            lane,
            coeff,
        })
    }

    /// The irradiance on the top face of the topmost voxel, as the kernel has it.
    ///
    /// **The same `Q` has to reach `FoldParams::i_surface`.** It is the one term
    /// of the absorption of the topmost voxel that is not in the field (ADR-049),
    /// so a second folding of the same physical number puts a last-bit difference
    /// into that voxel and nowhere else.
    #[inline]
    #[must_use]
    pub fn i_surface(&self) -> Q {
        self.params.i_surface
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

    fn amounts() -> (Vec<M32>, Vec<M64>) {
        let mut src32 = vec![M32::ZERO; (2 * N_VOXELS) as usize];
        let mut src64 = vec![M64::ZERO; N_VOXELS as usize];
        for at in 0..N_VOXELS as usize {
            src32[N_VOXELS as usize + at] = M32::new(12_000_000);
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
        let light = Light::new(&grid(), &table(), INCIDENT, DZ).unwrap();
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
        let light = Light::new(&grid(), &table(), INCIDENT, DZ).unwrap();
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

    #[test]
    fn the_incident_irradiance_has_one_source() {
        // The same `Q` reaches the kernel and whoever folds step `i'`. Two
        // foldings of one physical number differ in the last bit, and that
        // difference lands on the absorption of the topmost voxel and nowhere
        // else — with `SOLAR_IN` computed by the same fold out of the same number,
        // so the energy ledger closes exactly.
        let light = Light::new(&grid(), &table(), INCIDENT, DZ).unwrap();
        assert_eq!(light.i_surface(), light.params().i_surface);
        assert_eq!(light.i_surface(), Q::from_f64(INCIDENT));
    }

    #[test]
    fn the_voxel_depth_is_inside_the_folded_coefficient() {
        // `dz` is the factor a hand-folded coefficient loses, and losing it is
        // invisible in the shape of the profile: only the total optical depth
        // moves, which reads as an uncalibrated `k`.
        let shallow = Light::new(&grid(), &table(), INCIDENT, DZ).unwrap();
        let deep = Light::new(&grid(), &table(), INCIDENT, 2.0 * DZ).unwrap();
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
        let light = Light::new(&grid(), &table(), INCIDENT, DZ).unwrap();
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
        assert!(Light::new(&grid(), &bad, INCIDENT, DZ).is_err());
        assert!(Light::new(&grid(), &table(), INCIDENT, 0.0).is_err());
        assert!(Light::new(&grid(), &table(), f64::INFINITY, DZ).is_err());
    }
}
