//! Step `i'`: everything the tick's energy arrived as, folded from the fine grid
//! onto the coarse one, and the ledger's half of it.
//!
//! Everything `kernels/fold.rs` is not allowed to know. The kernel gets five
//! slices and a struct of scalars; this file is where the irradiance of the light
//! process, the derived `units_per_intensity` and `dt` become those scalars, and
//! where the loop over the coarse cells lives (ADR-034, ADR-045, ADR-049).
//!
//! # This is a step of the tick and not a process of the roster
//!
//! There is no [`super::ProcessId`] entry here, and ADR-065 says why: the fold is
//! part of the energy path of the reactions rather than a process a scenario can
//! switch off. `Step::EnergyFold::process()` answers `None` for exactly that
//! reason, so its dispatch condition is assigned by a record and not by a roster
//! entry — **`i'` runs if and only if `h` runs** (ADR-045, restated by ADR-086).
//!
//! The alternative, "`i'` runs on `h` **or** light", is rejected by the ownership
//! criterion of ADR-086 rather than by taste: with `h` off the fold would read an
//! `energy_delta` nobody wrote this tick, which is a `Scratch` buffer acquiring a
//! reader earlier than its writer. The price of keeping the gate has to be said
//! out loud, because it is silent in every other way: a scenario with the light
//! on and the reactions off gets no fold at all, absorbed light reaches neither
//! the enthalpy nor `SOLAR_IN`, and the world does not warm from illumination.
//! Today that state is unreachable — `refuse_if_blocked` rejects the light
//! process outright while energy has no sink (ADR-076) — and the day it becomes
//! reachable the gate opens by a record, not by an edit to the condition.
//!
//! # The one number this file must not invent
//!
//! [`FoldParams::i_surface`] is the upper absorption term of the topmost layer,
//! `i_surface - light[nz-1]`, and it is the only term of that difference which is
//! not in the field. Over a zeroed light field it is `i_surface` entire: a
//! nonzero value with step `a` switched off is a perfect silent heater whose
//! **energy residual stays at exactly zero**, because `SOLAR_IN` is credited the
//! same number. `FoldParams` is a `Copy` struct mirroring a WGSL uniform
//! (ADR-015), so no type can prevent it; what stands there is
//! `react::fold_params`, which takes an `Option<&Light>`, and
//! `the_fold_credits_no_solar_energy_when_the_light_step_does_not_run`.

use anyhow::{Result, bail};

use super::{Conservation, Invariant, coarse_shape_agrees, light::Light, react::fold_params};
use crate::config::Derived;
use crate::kernels::fold::{FoldParams, fold_energy};
use crate::ledger::Channel;
use crate::numeric::{M64, Q};
use crate::world::Grid;

/// One energy fold: the scalars the kernel receives, and the count of cells to
/// dispatch it over.
///
/// Built once per run and applied once per tick. Everything in it is a fold of
/// the scenario and of the derivation; nothing in it is state.
#[derive(Clone, Copy, Debug)]
pub struct Fold {
    params: FoldParams,
    n_cells: u32,
}

impl Fold {
    /// Fold the scenario into the scalars of `kernels::fold::fold_energy`.
    ///
    /// `light` is the folded light process, or `None` when step `a` does not run;
    /// it is what decides [`FoldParams::i_surface`], and passing `Some` for a
    /// scenario whose light is off is the mistake the module header describes.
    ///
    /// # Errors
    ///
    /// Returns an error if the coarse grid is not the fine one at the declared
    /// `lod`, or if `dt` is not a usable tick.
    pub fn new(
        grid: &Grid,
        enthalpy_grid: &Grid,
        enthalpy_lod: u32,
        light: Option<&Light>,
        derived: &Derived,
        dt: f64,
    ) -> Result<Self> {
        coarse_shape_agrees(
            grid,
            enthalpy_grid,
            enthalpy_lod,
            "the energy fold of step `i'`",
        )?;
        if !dt.is_finite() || dt <= 0.0 {
            bail!("the tick {dt} s is not a usable step for the energy fold");
        }

        // Zero when the scenario declares no light, and the pair is consistent by
        // construction: `fold_params` puts `Q::ZERO` in `i_surface` for the same
        // `None`, so the two halves of the top layer's absorption term cannot
        // disagree. Derived and never folded again here — `units_per_intensity`
        // is `dx^2 * 2^k_E`, and its two mistakes, the coarse face and a `dt`
        // folded in, move the whole solar input while leaving the shape of `I(z)`
        // and both residuals untouched (ADR-076).
        let units_per_intensity = derived
            .light()
            .map_or(Q::ZERO, |l| Q::from_f64(l.units_per_intensity));

        Ok(Self {
            params: fold_params(
                light,
                units_per_intensity,
                Q::from_f64(dt),
                grid.nx(),
                grid.ny(),
                grid.nz(),
                enthalpy_lod,
            ),
            n_cells: (grid.nx() >> enthalpy_lod)
                * (grid.ny() >> enthalpy_lod)
                * (grid.nz() >> enthalpy_lod),
        })
    }

    /// The folded scalars, as the kernel receives them.
    #[inline]
    #[must_use]
    pub fn params(&self) -> FoldParams {
        self.params
    }

    /// How many invocations one application costs: the cells of the coarse grid.
    ///
    /// The extents are shifted per axis and never taken off the count of fine
    /// voxels: on a grid where an axis does not divide, `n_voxels >> (3*lod)` is
    /// larger than the number of covering cubes and the edge of the domain is
    /// covered by nothing.
    #[inline]
    #[must_use]
    pub fn n_cells(&self) -> u32 {
        self.n_cells
    }

    /// The fold conserves matter and changes energy through `SOLAR_IN`.
    ///
    /// Matter, because [`Fold::apply`] touches no buffer of class `M` that counts
    /// as an amount of substance. Energy, because it does two things at once and
    /// only one of them is a move: the reaction increment already inside the
    /// domain is added to the enthalpy — that half is conserved, ADR-081 weighs
    /// the chemical energy on the left side of the identity — while the absorbed
    /// light **enters** the domain and is owed to a channel. The slice the kernel
    /// writes for it is reduced by the host immediately behind this dispatch, and
    /// `SOLAR_IN` has exactly one writer (ADR-075).
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::ChangedThrough(Channel::SolarIn),
        }
    }

    /// One application: every coarse cell of the enthalpy field, plus the slice
    /// the ledger is owed.
    ///
    /// Dispatched over the **coarse** cells and never over the fine voxels: at
    /// `lod = 2` sixty-four fine voxels fold into one cell, and a host that
    /// dispatched per voxel would decode a `cz` past the end of the coarse field.
    ///
    /// `src_h` and `dst_h` come out of one `Field::pair_mut`, because the kernel
    /// reads state `N` and writes state `N+1` and may not read the buffer it
    /// writes (ADR-034).
    ///
    /// # Panics
    ///
    /// If a buffer does not have the shape this operator was folded for.
    pub fn apply(
        &self,
        energy_delta: &[M64],
        light: &[Q],
        src_h: &[M64],
        dst_h: &mut [M64],
        solar: &mut [M64],
    ) {
        let n_voxels = (self.params.nx * self.params.ny * self.params.nz) as usize;
        assert!(
            energy_delta.len() >= n_voxels,
            "the energy accumulator holds {} cells against {n_voxels} voxels",
            energy_delta.len()
        );
        assert!(
            light.len() >= n_voxels,
            "the light field holds {} cells against {n_voxels} voxels",
            light.len()
        );
        let n_cells = self.n_cells as usize;
        assert!(
            src_h.len() >= n_cells && dst_h.len() >= n_cells,
            "the enthalpy pair holds {} and {} cells against the {n_cells} this \
             fold was folded for",
            src_h.len(),
            dst_h.len()
        );
        assert_eq!(
            solar.len(),
            n_cells,
            "the solar slice holds {} cells against the {n_cells} this fold was \
             folded for: it is reduced whole into SOLAR_IN, so a longer one \
             credits cells that are not in the domain (ADR-075)",
            solar.len()
        );

        for coarse in 0..self.n_cells {
            fold_energy(
                energy_delta,
                light,
                src_h,
                dst_h,
                solar,
                &self.params,
                coarse,
            );
        }
    }
}
