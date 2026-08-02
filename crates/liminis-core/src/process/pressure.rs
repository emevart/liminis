//! The pressure process: one overflow field, every lane relaxed against it, one
//! swap.
//!
//! Step `e` of the tick order (SPEC section 8). Everything `kernels/pressure.rs`
//! is not allowed to know: the partial molar volumes, the volume of a voxel, the
//! limiting overflow the mobility is derived from, and which lane of which width
//! class a substance lives in.
//!
//! # The shape of one application, and why it is three phases and not one loop
//!
//! ```text
//! 1. overflow_voxel over the whole domain, out of snapshot N of both fields
//! 2. relax_voxel_32 / relax_voxel_64 over every lane, against that one field
//! 3. one swap per field
//! ```
//!
//! **The overflow is taken once.** Recomputing it after each lane is the natural
//! reaction to "pressure should see fresh state", and it makes the order of the
//! lanes part of the semantics of the world: every lane still conserves exactly,
//! both axes of the invariant of ADR-028 still close, and the ledger says nothing
//! at all. It is the same class of error ADR-041 closed for reactions —
//! "recomputing availability after each reaction makes the order of the rows in
//! the TOML part of the semantics of the world, invisibly, because the balance
//! goes on closing".
//!
//! **Every lane, in both widths.** The concrete way to get this wrong is to skip
//! water, because it is the single `i64` substance and goes down the other branch
//! (`kernels/pressure.rs` names it). The composition of a voxel then drifts while
//! every lane conserves separately. The branch on storage width lives here, which
//! is where ADR-056 puts it: "it is two functions and two instantiations of the
//! template".
//!
//! **One swap, and the copy is empty.** Pressure runs exactly one substep per
//! lane, so every lane is odd, so the repair of ADR-057 is a swap of the field and
//! a copy of the *even* group — which is empty. The repair is still asked for by
//! name rather than open-coded as `field.swap()`, because "all lanes are odd" is
//! a property of this process today and a `ParitySplit` is where such a property
//! is written down; a lane left out of the restoration comes back holding the
//! write buffer, that is the previous phase's state, and zeroes on the first tick.
//! Zero is a legal amount and nothing falls over.
//!
//! # One iteration per tick, and the stiffness that cancels
//!
//! ADR-055 rejects several relaxation iterations per tick by name — "the same as
//! raising `k`, by a detour" — because one voxel per tick is the **Courant
//! ceiling** and not slowness. So [`Pressure::apply`] is one pass and there is no
//! loop to tune.
//!
//! The number the kernel receives is `1/theta_max`, and the declared stiffness `k`
//! takes no part in the arithmetic at all: deriving the mobility from "one voxel
//! per tick at `theta_max`" fixes the whole product `L*k*dt/dx^2`. `k` is
//! therefore not an argument of [`Pressure::new`], and that absence is the
//! decision — a wrong `k`, bars for pascals, would change not one bit of any run.
//!
//! # What is not here
//!
//! No velocity of any kind is added to the prescribed field of step `b`. ADR-069
//! forbids it on three independent grounds, and the first is arithmetic: ADR-055
//! derives this mobility so that pressure already spends the **whole** Courant
//! budget of its own step. Under Lie-Trotter splitting each operator gets by on
//! its own condition; a sum of two fields that each satisfy one satisfies
//! neither. The second ground is that this flux is divergent by construction —
//! that is its entire purpose — so adding it would take from the prescribed field
//! the one property it is built as a curl to have, while
//! `prescribed_velocity_is_divergence_free_bit_for_bit` went on being green,
//! because it looks at the buffer `u` and not at a sum.

use anyhow::{Result, bail};

use super::{Conservation, Invariant};
use crate::kernels::pressure::{
    Occupants, PressureParams, overflow_voxel, relax_voxel_32, relax_voxel_64,
};
use crate::numeric::{M32, M64, Q};
use crate::world::{Boundary, Face, Field32, Field64, Grid, LaneRef, ParitySplit};

/// The default of `enabled` for pressure, and it is `false` (ADR-065).
///
/// The limiting overflow `theta_max` is declared nowhere — `TODO(theta-max)` on
/// [`Pressure::new`] says so in full — and the mobility of ADR-055 is derived
/// from it and from nothing else. A default of `true` would mean every scenario
/// displacing matter at a rate whose only input was invented at the point of
/// use; and because the scheme conserves exactly, both halves of the invariant
/// would close over it for ever.
///
/// The mirror argument, so that this value is not read as "pressure is
/// optional": ADR-055 makes the overflow a property of the world rather than of
/// a scenario — matter has volume whether or not anybody switches it on — so the
/// natural default once `theta_max` exists is `true`.
// TODO(CONFIG_SCHEMA.md section 13 item 23): assigned by no record. It is
// settled together with `theta_max`, in `DECISIONS.md` and in section 7 of the
// schema.
pub const ENABLED_BY_DEFAULT: bool = false;

/// One substance that takes up room in a voxel, as the scenario declares it.
///
/// Three numbers rather than one folded coefficient, for the reason
/// `process::light::Attenuator` gives: `1/V_voxel` is the factor a hand-folded
/// coefficient loses, and at `dx = 100 um` that is `1e12` — the occupancy is
/// monstrous everywhere, every face saturates, and the picture reads as "the
/// stiffness is not calibrated yet".
#[derive(Clone, Copy, Debug)]
pub struct Occupant {
    /// Which buffer the amounts are in: a width class and a lane inside it
    /// (ADR-056).
    pub lane: LaneRef,
    /// The partial molar volume `V_bar`, m^3/mol (ADR-067).
    ///
    /// **May be negative, and zero is legal.** Electrostriction is not exotic —
    /// an ion orders the water around it more tightly than water packs itself, so
    /// `PO4^3-` is about `-4.0e-5` — so `V_occ` is unbounded below and there is no
    /// positivity check on it anywhere in this file.
    pub partial_molar_volume: f64,
    /// How many storage units make one mole of this substance (ADR-026).
    pub units_per_mol: f64,
}

/// The pressure process: the folded parameters of one tick, the occupancy table,
/// and the repair of the process boundary.
#[derive(Clone, Debug)]
pub struct Pressure {
    params: PressureParams,
    lane: Vec<u32>,
    coeff: Vec<Q>,
    /// How many lanes each width class must hold for the occupancy table to
    /// address, so that [`Pressure::apply`] can check the field it is handed
    /// against the table it was folded from rather than against nothing.
    lanes_32: u32,
    lanes_64: u32,
}

impl Pressure {
    /// Fold the occupancy table and derive the mobility from the limiting
    /// overflow.
    ///
    /// `theta_max` is the overflow at which the displacement over one tick is
    /// exactly one voxel; `v_voxel` is the volume of a voxel in m^3. The stiffness
    /// `k` is **not** an argument: it cancels out of the derivation entirely (see
    /// the module header).
    ///
    /// # Errors
    ///
    /// Returns an error if `theta_max`, `v_voxel` or an entry of the table is not
    /// a usable number, or if the table is longer than the `width_mask` of the
    /// kernel can address.
    // TODO(theta-max): the limiting overflow is **declared nowhere**.
    // `CONFIG_SCHEMA.md` section 7 carries only the stiffness `k` on the pressure
    // row, and `QUANTITIES.md` section 4 has `k` and the mobility as derived. So it
    // arrives as an argument rather than being read from a config that has no key
    // for it, and a plausible number invented here would be indistinguishable from
    // a decision. Settling it is a record in the journal plus a key in
    // `CONFIG_SCHEMA.md` section 7 — together with the question this work raises
    // anew: since `k` cancels, is it still a key of the scenario, and what does it
    // observe.
    pub fn new(grid: &Grid, theta_max: f64, occupants: &[Occupant], v_voxel: f64) -> Result<Self> {
        if !theta_max.is_finite() || theta_max <= 0.0 {
            bail!(
                "the limiting overflow theta_max = {theta_max} is not a usable \
                 number: the mobility is derived from it as 1/theta_max (ADR-055), \
                 so at zero the scheme has no speed at all and below zero it runs \
                 uphill"
            );
        }
        if !v_voxel.is_finite() || v_voxel <= 0.0 {
            bail!("the voxel volume {v_voxel} m^3 is not a usable number");
        }
        if occupants.len() > u32::BITS as usize {
            bail!(
                "{} occupants against a width mask of {} bits: the bit indexes the \
                 table entry (ADR-040), so the table cannot be longer than the mask",
                occupants.len(),
                u32::BITS
            );
        }

        let mut lane = Vec::with_capacity(occupants.len());
        let mut coeff = Vec::with_capacity(occupants.len());
        let mut width_mask = 0u32;
        let mut lanes_32 = 0u32;
        let mut lanes_64 = 0u32;

        for (entry, occupant) in occupants.iter().enumerate() {
            if !occupant.partial_molar_volume.is_finite()
                || !occupant.units_per_mol.is_finite()
                || occupant.units_per_mol <= 0.0
            {
                bail!(
                    "occupant {entry} declares V_bar = {} m^3/mol over {} units per \
                     mole, which is not a usable pair. A negative V_bar is legal \
                     (ADR-067, electrostriction); a non-positive units_per_mol is \
                     not",
                    occupant.partial_molar_volume,
                    occupant.units_per_mol
                );
            }
            // The one place `V_bar`, `units_per_mol` and `V_voxel` are visible at
            // once: the dimensionless fraction of a voxel one storage unit takes.
            // Neither a molar mass nor a factor of `1e-3` is in this product, and
            // neither may return (ADR-067).
            coeff.push(Q::from_f64(
                occupant.partial_molar_volume / (occupant.units_per_mol * v_voxel),
            ));
            match occupant.lane {
                LaneRef::Narrow(index) => {
                    lane.push(index);
                    lanes_32 = lanes_32.max(index + 1);
                }
                LaneRef::Wide(index) => {
                    lane.push(index);
                    lanes_64 = lanes_64.max(index + 1);
                    width_mask |= 1 << entry;
                }
            }
        }

        Ok(Self {
            params: PressureParams {
                nx: grid.nx(),
                ny: grid.ny(),
                nz: grid.nz(),
                n_voxels: grid.n_voxels(),
                n_occupants: occupants.len() as u32,
                width_mask,
                periodic_mask: periodic_mask(grid)?,
                // ADR-055, and the whole of the physics this process carries:
                // deriving the mobility from "one voxel per tick at the declared
                // limiting overflow" fixes `L*k*dt/dx^2` at `1/theta_max`.
                courant_per_overflow: Q::from_f64(1.0 / theta_max),
            },
            lane,
            coeff,
            lanes_32,
            lanes_64,
        })
    }

    /// The Courant number per unit of overflow difference across a face:
    /// `1/theta_max`.
    ///
    /// The one number in the scheme that knows any physics, and the stiffness is
    /// not in it. See the module header for why that is uncomfortable and has to
    /// be said out loud.
    #[inline]
    #[must_use]
    pub fn courant_per_overflow(&self) -> Q {
        self.params.courant_per_overflow
    }

    /// The folded parameters, as the kernels receive them.
    #[inline]
    #[must_use]
    pub fn params(&self) -> PressureParams {
        self.params
    }

    /// Pressure conserves matter and does not touch energy.
    ///
    /// The first half is by construction: both voxels of a face apply one rounded
    /// number with opposite signs, so the sum over the domain is unchanged exactly
    /// (ADR-005, ADR-034) — separately for every lane, which is the statement that
    /// has teeth, since a process that moved matter between lanes would keep the
    /// field's total.
    ///
    /// The second half is a statement about *this process* and rests on an absence
    /// the corpus has not measured.
    // TODO(enthalpy-of-transport): whether enthalpy follows the matter that
    // pressure moves is not settled. ADR-062 bounded and checked the *diffusive*
    // term at load — 5% threshold, 1.64% on the corpus registry — and there is no
    // such estimate for steps `c` and `e`. Declaring `energy: Conserved` here
    // therefore rests on the absence of a number rather than on one, which is
    // exactly what ADR-028 calls being half inside the ledger.
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::Conserved,
        }
    }

    /// The overflow field of the whole domain, out of one snapshot of both
    /// widths.
    ///
    /// Public because [`Pressure::apply`] is not the only caller that needs it: a
    /// test measuring whether the scheme relaxes has to see the field itself, and
    /// so will whoever writes the golden test with a piston that ADR-055 requires.
    ///
    /// The result is **two-sided**: it holds the signed `V_occ/V_voxel - 1` and
    /// not `max(0, .)` of it. `TODO(one-sided)` in `kernels/pressure.rs` records
    /// that the choice is not made in the journal; this is the reading from which
    /// either can be formed.
    ///
    /// # Panics
    ///
    /// Panics if `overflow` does not have the shape this process was folded for.
    pub fn overflow(&self, src32: &[M32], src64: &[M64], overflow: &mut [Q]) {
        assert_eq!(
            overflow.len(),
            self.params.n_voxels as usize,
            "the overflow field does not have the shape this process was folded for"
        );
        let occupants = Occupants {
            lane: &self.lane,
            coeff: &self.coeff,
        };
        for idx in 0..self.params.n_voxels {
            overflow_voxel(src32, src64, overflow, &occupants, &self.params, idx);
        }
    }

    /// One tick of pressure over every lane of both fields.
    ///
    /// `overflow` is scratch: it is filled here from state `N`, and its contents
    /// on entry are ignored. Returns with the process-boundary invariant of
    /// ADR-057 restored — the front buffer holds the new state for **every** lane
    /// of both fields.
    ///
    /// A width class the scenario has none of is passed as `None`, and that is not
    /// the same as an empty field: `Field::new` refuses a field of zero lanes, and
    /// a dummy lane would shift the addressing of that class by one (ADR-056).
    ///
    /// # Panics
    ///
    /// Panics if a field's shape or lane count is not the one the occupancy table
    /// was folded from. Both are programming errors rather than bad scenarios.
    pub fn apply(
        &self,
        amounts_32: Option<&mut Field32>,
        amounts_64: Option<&mut Field64>,
        overflow: &mut [Q],
    ) {
        // Phase one: the overflow field, once, out of the snapshot every lane is
        // about to be relaxed against. Both widths are read here and nowhere else,
        // which is what makes the answer independent of the order of the lanes.
        {
            let src32: &[M32] = match &amounts_32 {
                Some(field) => field.read(),
                None => &[],
            };
            let src64: &[M64] = match &amounts_64 {
                Some(field) => field.read(),
                None => &[],
            };
            self.overflow(src32, src64, overflow);
        }

        // Phase two: every lane of every field, against that one field. Phase
        // three: one swap per field, with an empty copy — see the module header.
        if let Some(field) = amounts_32 {
            assert_eq!(
                field.n_voxels(),
                self.params.n_voxels,
                "the 32-bit field does not have the shape this process was folded for"
            );
            assert!(
                field.lanes() >= self.lanes_32,
                "the occupancy table names lane {} of a 32-bit field holding {}",
                self.lanes_32.saturating_sub(1),
                field.lanes()
            );
            for lane in 0..field.lanes() {
                let (src, dst) = field.lane_pair_mut(lane);
                for idx in 0..self.params.n_voxels {
                    relax_voxel_32(src, overflow, dst, &self.params, idx);
                }
            }
            field.restore_boundary(&all_lanes_odd(field.lanes()));
        }
        if let Some(field) = amounts_64 {
            assert_eq!(
                field.n_voxels(),
                self.params.n_voxels,
                "the 64-bit field does not have the shape this process was folded for"
            );
            assert!(
                field.lanes() >= self.lanes_64,
                "the occupancy table names lane {} of a 64-bit field holding {}",
                self.lanes_64.saturating_sub(1),
                field.lanes()
            );
            for lane in 0..field.lanes() {
                let (src, dst) = field.lane_pair_mut(lane);
                for idx in 0..self.params.n_voxels {
                    relax_voxel_64(src, overflow, dst, &self.params, idx);
                }
            }
            field.restore_boundary(&all_lanes_odd(field.lanes()));
        }
    }
}

/// The repair of ADR-057 for a phase that ran exactly one substep on every lane.
///
/// One substep is an odd number, so every lane is odd, so the split swaps the
/// field and copies the even group — which is empty. Written as a `ParitySplit`
/// rather than as a bare `field.swap()` because "one substep per lane" is a
/// property of this process today: the day pressure grows a second pass for one
/// lane and not another, a bare swap silently leaves half the field a tick stale,
/// while this expression starts copying.
fn all_lanes_odd(lanes: u32) -> ParitySplit {
    ParitySplit::from_substeps(&vec![1u32; lanes as usize])
}

/// The six boundary conditions of the grid, as the bit mask the kernels read.
///
/// Bit `f` is set when face `f` is periodic; a closed face contributes no bit and
/// the kernel's lookup returns the voxel itself, which carries no flux because
/// the courant of that face is then a difference of one value with itself. The
/// same function `process/diffuse.rs` carries, refusing the same third case for
/// the same reason.
fn periodic_mask(grid: &Grid) -> Result<u32> {
    let mut mask = 0u32;
    for face in Face::ALL {
        match grid.boundary(face) {
            Boundary::Periodic => mask |= 1 << (face as u32),
            Boundary::Closed => {}
            // Unreachable today: `Grid::new` refuses an exchange face until the
            // channel counters exist. Refused again rather than folded into one of
            // the other two, because the closest neighbour of "exchange" is
            // "closed", and quietly sealing a face that is supposed to vent is
            // exactly the error the refusal upstream exists to prevent.
            Boundary::Exchange => bail!(
                "face {face:?} is an exchange face: matter crossing it belongs in \
                 the BOUNDARY_EXCHANGE channel (ADR-059), and this process credits \
                 no channel at all"
            ),
        }
    }
    Ok(mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Field;

    const NX: u32 = 3;
    const NY: u32 = 4;
    const NZ: u32 = 5;
    const N_VOXELS: u32 = NX * NY * NZ;
    const THETA_MAX: f64 = 24.0;
    const V_VOXEL: f64 = 1.0;

    fn floored() -> Grid {
        Grid::new(
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
        .unwrap()
    }

    fn occupants() -> Vec<Occupant> {
        vec![
            Occupant {
                lane: LaneRef::Narrow(0),
                partial_molar_volume: 1.0,
                units_per_mol: 1024.0,
            },
            Occupant {
                lane: LaneRef::Wide(0),
                partial_molar_volume: 1.0,
                units_per_mol: 2048.0,
            },
        ]
    }

    fn built() -> Pressure {
        Pressure::new(&floored(), THETA_MAX, &occupants(), V_VOXEL).unwrap()
    }

    fn state() -> (Field32, Field64) {
        let grid = floored();
        let mut narrow = Field::new(&grid, 1).unwrap();
        let mut wide = Field::new(&grid, 1).unwrap();
        for at in 0..N_VOXELS as usize {
            narrow.write_mut()[at] = M32::new(1024 * (1 + (at as i32 * 7) % 5));
            wide.write_mut()[at] = M64::new(2048 * i64::from((at as u32 * 3) % 7));
        }
        narrow.swap();
        wide.swap();
        (narrow, wide)
    }

    #[test]
    fn a_pressure_phase_returns_state_n_in_the_front_buffer_for_every_lane() {
        // Pressure runs one substep per lane, so every lane is odd, so the repair
        // is one swap and a copy of length zero. A lane left out of it comes back
        // holding the write buffer — the previous phase's state, and zeroes on the
        // first tick, which is a legal amount no invariant objects to.
        //
        // Asserted as "the front buffer is what the kernel wrote", by running the
        // kernel by hand over the same snapshot. A "the front buffer changed"
        // assertion would be blind to exactly the lane whose net flux is zero.
        let pressure = built();
        let (mut narrow, mut wide) = state();
        let before_32: Vec<M32> = narrow.read().to_vec();
        let before_64: Vec<M64> = wide.read().to_vec();

        let mut overflow = vec![Q::ZERO; N_VOXELS as usize];
        pressure.overflow(&before_32, &before_64, &mut overflow);
        let mut expected_32 = vec![M32::ZERO; N_VOXELS as usize];
        let mut expected_64 = vec![M64::ZERO; N_VOXELS as usize];
        for idx in 0..N_VOXELS {
            relax_voxel_32(
                &before_32,
                &overflow,
                &mut expected_32,
                &pressure.params(),
                idx,
            );
            relax_voxel_64(
                &before_64,
                &overflow,
                &mut expected_64,
                &pressure.params(),
                idx,
            );
        }

        let mut scratch = vec![Q::ZERO; N_VOXELS as usize];
        pressure.apply(Some(&mut narrow), Some(&mut wide), &mut scratch);

        assert_eq!(narrow.read(), &expected_32[..]);
        assert_eq!(wide.read(), &expected_64[..]);
        // And the state moved, or the equality above holds over a no-op.
        assert_ne!(narrow.read(), &before_32[..]);

        // The repair is a swap and nothing else: one substep per lane is odd, and
        // the even group is empty.
        let split = all_lanes_odd(1);
        assert!(split.swaps());
        assert!(split.lanes_to_copy().is_empty());
    }

    #[test]
    fn an_absent_width_class_is_not_an_empty_field() {
        // `None` and not a field of zero lanes: `Field::new` refuses that, and a
        // dummy lane would shift the addressing of the class by one (ADR-056).
        let narrow_only = vec![Occupant {
            lane: LaneRef::Narrow(0),
            partial_molar_volume: 1.0,
            units_per_mol: 1024.0,
        }];
        let pressure = Pressure::new(&floored(), THETA_MAX, &narrow_only, V_VOXEL).unwrap();
        let (mut narrow, _) = state();
        let before: Vec<M32> = narrow.read().to_vec();

        let mut overflow = vec![Q::ZERO; N_VOXELS as usize];
        pressure.apply(Some(&mut narrow), None, &mut overflow);
        assert_ne!(narrow.read(), &before[..]);
        assert_eq!(pressure.params().width_mask, 0);
    }

    #[test]
    fn a_negative_partial_molar_volume_is_accepted_and_a_zero_units_per_mol_is_not() {
        // Electrostriction is not exotic, so `V_bar < 0` is a declaration and not
        // a defect (ADR-067) and `V_occ` is unbounded below. A `units_per_mol` of
        // zero is a different thing: it is a division by zero in the folding.
        let electrostrictive = vec![Occupant {
            lane: LaneRef::Narrow(0),
            partial_molar_volume: -4.0e-5,
            units_per_mol: 1024.0,
        }];
        assert!(Pressure::new(&floored(), THETA_MAX, &electrostrictive, V_VOXEL).is_ok());

        let broken = vec![Occupant {
            lane: LaneRef::Narrow(0),
            partial_molar_volume: 1.0,
            units_per_mol: 0.0,
        }];
        assert!(Pressure::new(&floored(), THETA_MAX, &broken, V_VOXEL).is_err());
        assert!(Pressure::new(&floored(), 0.0, &occupants(), V_VOXEL).is_err());
        assert!(Pressure::new(&floored(), THETA_MAX, &occupants(), 0.0).is_err());
    }

    #[test]
    fn the_stiffness_is_not_an_argument() {
        // ADR-055 derives the mobility so that `L*k*dt/dx^2 = 1/theta_max`, so `k`
        // cancels out of the arithmetic entirely. Its absence from the signature is
        // the decision, and this test is the only place that says so — a `k`
        // threaded through and multiplied in would change no result of any other
        // test in this file.
        let pressure = built();
        assert_eq!(
            pressure.courant_per_overflow(),
            Q::from_f64(1.0 / THETA_MAX)
        );
        assert_eq!(
            pressure.params().courant_per_overflow,
            pressure.courant_per_overflow()
        );
    }
}
