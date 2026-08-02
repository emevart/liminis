//! The world aggregate: every buffer of one run, and the rule about which of
//! them holds state `N`.
//!
//! [`Grid`], [`Field`] and [`Registry`] each answer one question and none of
//! them answers the fourth: **whose buffers survive a process boundary**. That
//! is ADR-057, and it is why this type exists rather than a tuple of the other
//! three.
//!
//! # Not every field lives on the same grid
//!
//! Amounts are on the fine grid of SPEC section 1.1. Enthalpy is on a coarse one
//! — 32^3 against 128^3 in the eco regime — because the substep count of a
//! diffusive field grows quadratically with the grid step and thermal
//! diffusivity is the fastest transport in the system (ADR-030, ADR-062). The
//! prescribed velocity field is on a third grid, coarser than the fine one and
//! finer than the enthalpy one (ADR-069). Light and the reaction energy
//! accumulator are on the fine grid.
//!
//! A coarse grid is **another [`Grid`]**, built by [`coarse_grid`], and not a
//! new mechanism: SPEC section 1.5 coarsens by a shift per axis, so the coarse
//! extents are `nx >> lod`, `ny >> lod`, `nz >> lod` and the boundary conditions
//! are inherited unchanged. What this module adds on top of that is the mapping
//! from a fine voxel to the coarse cell covering it, and it is deliberately
//! spelled once per **role** rather than once with a `lod` argument — see
//! [`World::enthalpy_cell_of`].
//!
//! # Which fields are double buffered, and which are not
//!
//! | field | grid | buffers | width |
//! |---|---|---|---|
//! | amounts | fine | two | `i32` and `i64`, derived (ADR-040) |
//! | enthalpy | coarse | two | `i64` (ADR-062) |
//! | reaction energy | fine | **one** | `i64` (ADR-045, ADR-062) |
//! | light | fine | one | `Q` |
//! | velocity, potential | coarse | one | `Q` (ADR-069) |
//! | heat capacity `C_cell` | coarse (enthalpy) | one | `Q` (ADR-062) |
//! | temperature `T` | coarse (enthalpy) | one | `Q` (ADR-044, ADR-062) |
//!
//! Two buffers are for gather transport: a kernel may not read the buffer it
//! writes (ADR-034). A quantity that no kernel gathers over needs one. The
//! reaction accumulator is the interesting entry: ADR-045 keeps it single
//! buffered on purpose, because the reaction kernel **overwrites** its cell
//! rather than adding to it, "and no separate clearing pass appears". A second
//! buffer would make a forgotten clear invisible — the other buffer holds last
//! tick's numbers rather than rubbish — cost 16.8 MB at 128^3, and quietly take
//! the voxel past the 230 bytes ADR-062 budgets for it. So this type never
//! zeroes it, at any call.
//!
//! # What is deliberately not here
//!
//! **Nothing indexed by lane leaves this type.** A snapshot is written in
//! substance order (ADR-037, ADR-056), so the accessors are either whole buffers
//! for a kernel or [`World::lane_of`] for one substance. There is no
//! `lane_amounts(lane)`.
//!
//! **No arithmetic over `M`.** This module addresses and copies, and that is the
//! whole of it. The rounding that antisymmetry rests on is not here, so nothing
//! here can fail loudly; every mistake it can make is one of the quiet ones
//! listed on the function that can make it.
//!
//! **One field the corpus asks for and no record gives an owner.** The overflow
//! field `V_occ/V_voxel - 1` of ADR-055 and ADR-067 (`Q`, fine grid, one buffer)
//! exists in the journal without a home. Putting it here is a decision, not a
//! detail, so it is named and not allocated.
//!
//! Its former companion, the temperature denominator `C_cell` of ADR-062, *is*
//! allocated now, and so is the temperature itself — see
//! [`World::heat_capacity`] and [`World::temperature`]. The second of those is a
//! divergence from the price ADR-062 printed: that record budgets "a `Q` field on
//! the coarse grid, 131 kB without a second buffer" for the denominator and says
//! nothing about a field for `T`, while `process::React::apply` takes
//! `temperature: &[Q]` and so presumes a buffer. Two of those is 262 kB on the
//! coarse grids rather than 131, and the coarse line of ADR-062 moves from
//! +393 kB to +524 kB. That is a divergence from a record rather than from a
//! frozen document, so it is settled by a record: **ADR-079**, which also says
//! why the denominator is not enough on its own — `VelocityField::apply` divides
//! the enthalpy by it cell for cell and never looks at `T`.

use anyhow::{Context, Result, bail};

use super::field::{Field, Field32, Field64};
use super::grid::{Axis, Face, Grid};
use super::registry::{Registry, Width};
use crate::numeric::{M32, M64, Q};

/// How many components the prescribed velocity field and its potential hold per
/// cell.
///
/// Three, because both are vector fields on the coarse grid: ADR-069 prices `A`
/// and `u` at `3 * 64^3 * 4 B` each.
// TODO(velocity-layout): three numbers about this field are undecided and none
// of them can be guessed at from the price. ADR-069 fixes the volume and the
// grid and leaves open (a) whether the storage is component-major or
// voxel-major, (b) whether `u` is three components per cell or one per face, and
// (c) who owns the buffer of Courant numbers on the **fine** faces that
// `kernels/advect.rs` needs after the trilinear interpolation. Until they are
// decided the buffers here are flat and long enough, and nothing indexes into
// them: an accessor that picked an order would settle (a) in code.
const VELOCITY_COMPONENTS: u32 = 3;

/// Which coarse grid each role lives on, in bits of coarsening relative to the
/// fine grid.
///
/// Two numbers rather than one, and they are genuinely different: at the eco
/// regime's 128^3 base grid the enthalpy field is 32^3 (`lod = 2`, SPEC
/// section 1.5) and the velocity field is 64^3 (`lod = 1`, ADR-069).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorldLayout {
    /// Coarsening of the enthalpy grid. Two in the eco regime.
    pub enthalpy_lod: u32,
    /// Coarsening of the prescribed velocity grid. One in the eco regime.
    // TODO(velocity-lod): no config key declares this. `CONFIG_SCHEMA.md`
    // section 7 declares `lod` on a `[[field]]` record, and there is no
    // `[[field]] id = "velocity"`; the four velocity keys of ADR-069
    // (`u_conv_max`, `l_c`, `stir_fraction`, `stir_period`) belong to the
    // *process*. The 64^3 grid is named in the journal in prose and by no key at
    // all, so it arrives here from the caller rather than being defaulted to
    // something plausible.
    pub velocity_lod: u32,
}

/// Where one substance's amounts live: a width class and a lane inside it.
///
/// **The single door from a substance index to a buffer address**, and its whole
/// job is to make the miss inexpressible. Two independent doors —
/// [`World::amounts_32_mut`] and [`World::amounts_64_mut`] — let a caller ask
/// the narrow field for a wide substance, and that call does not panic: a lane
/// of that number probably exists over there, so somebody else's amounts are
/// read and written. It is the host-side twin of `s * n_voxels + idx` inside a
/// kernel (ADR-056), except that the `kernel-lint` grep does not see it, because
/// the hook only reads `kernels/**`.
///
/// On the registry the project carries, water is first and takes lane 0, so
/// `amounts_32_mut().lane_pair_mut(s)` is **right** at `s == 0` and off by one
/// from there on. The ledger closes either way — nothing is lost, the wrong
/// thing is read.
///
/// The branch belongs to the caller, exactly as ADR-056 promises ("it is two
/// functions and two instantiations of the template").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LaneRef {
    /// Lane of the `i32` field.
    Narrow(u32),
    /// Lane of the `i64` field.
    Wide(u32),
}

/// Every buffer of one run, and who owns them across a process boundary.
///
/// Built once, from a grid and an already-derived [`Registry`]; see the module
/// header for the table of which fields are double buffered.
#[derive(Debug)]
pub struct World {
    grid: Grid,
    registry: Registry,
    /// `None` when the registry declared no 32-bit substance. Legal, and the
    /// mirror of the wide case: `Field::new` refuses a field of zero lanes, and
    /// allocating one dummy lane instead would shift the whole addressing of
    /// that class by one (`Registry::lanes`, ADR-056).
    amounts_32: Option<Field32>,
    /// `None` when the registry declared no 64-bit substance.
    amounts_64: Option<Field64>,
    enthalpy_grid: Grid,
    enthalpy_lod: u32,
    /// Double buffered: the field diffuses, and thermal diffusion is the fastest
    /// transport in the system (ADR-062).
    enthalpy: Field64,
    /// One buffer, on the fine grid, `i64`, never cleared here (ADR-045,
    /// ADR-062).
    energy_delta: Vec<M64>,
    /// What the fold of step `i'` owes `SOLAR_IN`, one `M64` per **coarse** cell
    /// (ADR-075). One buffer and never cleared, for the reason `energy_delta`
    /// is: the kernel overwrites its element rather than adding to it.
    solar_in: Vec<M64>,
    /// One buffer, on the fine grid: what leaves a voxel through its bottom face
    /// (`kernels/light.rs`).
    light: Vec<Q>,
    /// `sum(n_i * c_p_i)` of a coarse cell, J/K: one buffer, on the **enthalpy**
    /// grid, class `Q` (ADR-044, ADR-062).
    heat_capacity: Vec<Q>,
    /// `T_ref + H/C_cell`, kelvin: one buffer, on the enthalpy grid, class `Q`.
    temperature: Vec<Q>,
    velocity_grid: Grid,
    velocity_lod: u32,
    velocity: Vec<Q>,
    velocity_potential: Vec<Q>,
}

impl World {
    /// Allocate every buffer of a run.
    ///
    /// The registry arrives with widths, lanes and scales already derived
    /// (ADR-039, ADR-040): this constructor assigns nothing and checks nothing
    /// about chemistry. What it does check is that the grid can be coarsened by
    /// each declared `lod` — see [`coarse_grid`].
    ///
    /// # Errors
    ///
    /// Returns an error if either coarse grid cannot be built, naming the role,
    /// or if a field would be longer than a `u32` index can address.
    pub fn new(grid: Grid, registry: Registry, layout: &WorldLayout) -> Result<Self> {
        let enthalpy_grid = coarse_grid(&grid, layout.enthalpy_lod)
            .context("the enthalpy field lives on a coarse grid (ADR-062)")?;
        let velocity_grid = coarse_grid(&grid, layout.velocity_lod)
            .context("the prescribed velocity field lives on a coarse grid (ADR-069)")?;

        let amounts_32 = match registry.lanes(Width::Bits32) {
            0 => None,
            lanes => Some(Field::new(&grid, lanes).context("the 32-bit amount field")?),
        };
        let amounts_64 = match registry.lanes(Width::Bits64) {
            0 => None,
            lanes => Some(Field::new(&grid, lanes).context("the 64-bit amount field")?),
        };

        // `coarse_grid` inherits the six faces unchanged, so this cannot fail
        // today — and it is checked rather than assumed because the enthalpy
        // field trades through the lid on its own account (ADR-059): a coarse
        // grid that had quietly lost the `exchange` face would seal the top of
        // the world thermally while the amounts went on venting, and the energy
        // residual would close over it, because a sealed field conserves no worse
        // than a venting one.
        for (role, coarse) in [
            ("enthalpy", &enthalpy_grid),
            ("the prescribed velocity", &velocity_grid),
        ] {
            if coarse.exchange_mask() != grid.exchange_mask() {
                bail!(
                    "the coarse grid of {role} exchanges on faces {:#08b} and the \
                     fine grid on {:#08b}: a coarse field that lost the face would \
                     be sealed where the amounts vent, and the energy residual \
                     closes over a sealed field exactly (ADR-059)",
                    coarse.exchange_mask(),
                    grid.exchange_mask()
                );
            }
        }

        let enthalpy = Field::new(&enthalpy_grid, 1).context("the enthalpy field")?;

        let n_voxels = grid.n_voxels() as usize;
        let n_enthalpy_cells = enthalpy_grid.n_voxels() as usize;
        let n_velocity = velocity_grid
            .n_voxels()
            .checked_mul(VELOCITY_COMPONENTS)
            .with_context(|| {
                format!(
                    "a velocity field of {VELOCITY_COMPONENTS} components over \
                     {} cells is longer than a u32 index can address",
                    velocity_grid.n_voxels()
                )
            })? as usize;

        Ok(Self {
            grid,
            registry,
            amounts_32,
            amounts_64,
            enthalpy_grid,
            enthalpy_lod: layout.enthalpy_lod,
            enthalpy,
            energy_delta: vec![M64::ZERO; n_voxels],
            // On the coarse grid, because it is the fold's output and the fold is
            // dispatched per coarse cell. Sized off the enthalpy grid rather than
            // off the fine one: at `lod = 2` and 128 cubed the difference is
            // 262 kB against 16.8 MB (ADR-075).
            solar_in: vec![M64::ZERO; n_enthalpy_cells],
            light: vec![Q::ZERO; n_voxels],
            // Zeroed here because a `Vec` has to start somewhere, and **not**
            // maintained at zero afterwards. Both are rewritten in full by
            // `process::Temperature::apply` before anything reads them, once a
            // tick, which is the same argument ADR-045 uses to do without a
            // clearing pass over `energy_delta`. A zero heat capacity is not a
            // neutral initial value either — it is the one value the kernel
            // refuses to divide by — so "the operator has not run yet" and "this
            // cell has no temperature" are the same bits, and nothing tells them
            // apart: under ADR-079 both read `T = T_ref` in every profile. That
            // is why the operator runs before its consumer rather than after
            // (`process/tick.rs`), and not why the buffer starts at zero.
            heat_capacity: vec![Q::ZERO; n_enthalpy_cells],
            temperature: vec![Q::ZERO; n_enthalpy_cells],
            velocity_grid,
            velocity_lod: layout.velocity_lod,
            velocity: vec![Q::ZERO; n_velocity],
            velocity_potential: vec![Q::ZERO; n_velocity],
        })
    }

    /// The fine grid: amounts, light and the reaction energy accumulator.
    #[inline]
    #[must_use]
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// The substance registry this world was built from.
    #[inline]
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The grid the enthalpy field lives on.
    #[inline]
    #[must_use]
    pub fn enthalpy_grid(&self) -> &Grid {
        &self.enthalpy_grid
    }

    /// The grid the prescribed velocity field lives on.
    #[inline]
    #[must_use]
    pub fn velocity_grid(&self) -> &Grid {
        &self.velocity_grid
    }

    /// Where substance `s` lives: a width class and a lane inside it.
    ///
    /// The one door. See [`LaneRef`].
    ///
    /// # Panics
    ///
    /// If `s` is not a substance index of this world's registry.
    #[inline]
    #[must_use]
    pub fn lane_of(&self, s: u32) -> LaneRef {
        let slot = self.registry.slot(s);
        match slot.width {
            Width::Bits32 => LaneRef::Narrow(slot.lane),
            Width::Bits64 => LaneRef::Wide(slot.lane),
        }
    }

    /// The 32-bit amount field, or `None` if the registry declared no 32-bit
    /// substance.
    #[inline]
    #[must_use]
    pub fn amounts_32(&self) -> Option<&Field32> {
        self.amounts_32.as_ref()
    }

    /// The 64-bit amount field, or `None` if the registry declared no 64-bit
    /// substance.
    #[inline]
    #[must_use]
    pub fn amounts_64(&self) -> Option<&Field64> {
        self.amounts_64.as_ref()
    }

    /// The 32-bit amount field, mutably.
    #[inline]
    pub fn amounts_32_mut(&mut self) -> Option<&mut Field32> {
        self.amounts_32.as_mut()
    }

    /// The 64-bit amount field, mutably.
    #[inline]
    pub fn amounts_64_mut(&mut self) -> Option<&mut Field64> {
        self.amounts_64.as_mut()
    }

    /// State `N` of both widths at once: the flat snapshot ADR-041 hands the
    /// reaction kernel.
    ///
    /// An absent width class gives an **empty slice**, not a field of one dummy
    /// lane. `width_mask == 0` (or its mirror) is what guarantees nobody looks
    /// into it (ADR-056).
    #[inline]
    #[must_use]
    pub fn amount_slices(&self) -> (&[M32], &[M64]) {
        (
            self.amounts_32.as_ref().map_or(&[][..], Field::read),
            self.amounts_64.as_ref().map_or(&[][..], Field::read),
        )
    }

    /// The snapshot of state `N` and the receiver of state `N+1`, both widths:
    /// `(src32, src64, dst32, dst64)`, in the argument order of
    /// `kernels::react::react_voxel`.
    // TODO(one-borrow-per-dispatch): the reaction kernel takes these four slices
    // *and* `energy_delta` in one call, and these are two `&mut self` borrows of
    // one `World`, so a caller cannot hold both. Until then a host copies the
    // accumulator in and out around the dispatch — `tests/acceptance_world.rs`
    // does exactly that and says so.
    //
    // Half of what this TODO used to say is answered. It said that a combined
    // accessor could not be designed yet, because the kernel also wants a
    // temperature field derived from enthalpy by an operator nothing in `process/`
    // performed — so a signature invented before that operator existed would be
    // wrong by the time it had a caller. The operator exists
    // (`process::Temperature`), and [`World::temperature_slices_mut`] is the
    // accessor it wanted. What is left is this one: the reaction dispatch needs
    // the amounts *mutably* beside the accumulator, which is a different set of
    // borrows, and the temperature it reads is by then an ordinary shared slice.
    #[inline]
    pub fn amount_slices_mut(&mut self) -> (&[M32], &[M64], &mut [M32], &mut [M64]) {
        let (src32, dst32) = match self.amounts_32.as_mut() {
            Some(field) => field.pair_mut(),
            None => (&[][..], &mut [][..]),
        };
        let (src64, dst64) = match self.amounts_64.as_mut() {
            Some(field) => field.pair_mut(),
            None => (&[][..], &mut [][..]),
        };
        (src32, src64, dst32, dst64)
    }

    /// The enthalpy field: coarse grid, `i64`, double buffered.
    ///
    /// `i64` is derived and not chosen: the window for `k_E` at `i32` is empty,
    /// and the miss is twenty-six binary orders (ADR-062). Three places in the
    /// corpus still print `i32` for this quantity.
    #[inline]
    #[must_use]
    pub fn enthalpy(&self) -> &Field64 {
        &self.enthalpy
    }

    /// The enthalpy field, mutably.
    #[inline]
    pub fn enthalpy_mut(&mut self) -> &mut Field64 {
        &mut self.enthalpy
    }

    /// The reaction energy accumulator: fine grid, `i64`, **one** buffer.
    ///
    /// Measured in the same joules as the enthalpy field, because the fold of
    /// ADR-045 is a sum and a unit conversion in the middle of it would make it
    /// a rounding (ADR-062).
    #[inline]
    #[must_use]
    pub fn energy_delta(&self) -> &[M64] {
        &self.energy_delta
    }

    /// The reaction energy accumulator, mutably. Never cleared by this type: the
    /// reaction kernel overwrites its own cell, which is why ADR-045 has no
    /// clearing pass.
    #[inline]
    pub fn energy_delta_mut(&mut self) -> &mut [M64] {
        &mut self.energy_delta
    }

    /// What the fold owes `SOLAR_IN`: **coarse** grid, `i64`, one buffer
    /// (ADR-075).
    ///
    /// Not state, which is why one buffer suffices and why the process-boundary
    /// invariant of ADR-057 does not reach it: no kernel reads it, the writer is
    /// the fold of step `i'` and the reader is the host reduction standing
    /// immediately behind it, on the same step. A second buffer would cost
    /// another 262 kB at 128 cubed and would add the one failure it looks like
    /// insurance against — reducing the wrong one, and so losing or doubling a
    /// tick's credit.
    #[inline]
    #[must_use]
    pub fn solar_in(&self) -> &[M64] {
        &self.solar_in
    }

    /// Everything one dispatch of the fold of step `i'` touches, in the argument
    /// order of `kernels::fold::fold_energy`: `(energy_delta, light,
    /// enthalpy_read, enthalpy_write, solar)`.
    ///
    /// The other half of the pattern [`World::temperature_slices_mut`] set: five
    /// borrows of one aggregate that a caller could not otherwise hold at once,
    /// handed over as slices rather than as a `&mut World`, so that no accumulator
    /// has to be copied around the dispatch (`TODO(one-borrow-per-dispatch)`).
    ///
    /// The two outputs are the pair ADR-075 requires to be addressed by the same
    /// `coarse`, and they are handed out together for that reason: a caller that
    /// obtained them separately would be a caller that could dispatch one without
    /// the other.
    #[inline]
    #[allow(clippy::type_complexity)]
    // Five slices against a clippy threshold that would rather see a struct, and
    // the same trade `temperature_slices_mut` makes: the shape is the kernel's
    // argument list, and naming a type for it hides the dispatch contract from
    // whoever reads the accessor.
    pub fn fold_slices_mut(&mut self) -> (&[M64], &[Q], &[M64], &mut [M64], &mut [M64]) {
        // `pair_mut` and not `lane(0)`/`write_mut` separately: the fold reads
        // state `N` of the enthalpy and writes state `N+1`, and the two have to
        // come out of one borrow or the kernel would be reading the buffer it
        // writes. The lane carries the ghost cell after the coarse cells
        // (ADR-059); the fold never indexes past `n_coarse`, and the reservoir is
        // not a coarse cell.
        let (src_h, dst_h) = self.enthalpy.pair_mut();
        (
            &self.energy_delta,
            &self.light,
            src_h,
            dst_h,
            &mut self.solar_in,
        )
    }

    /// The light field: fine grid, one buffer. A cell holds what **leaves** the
    /// voxel through its bottom face (`kernels/light.rs`).
    #[inline]
    #[must_use]
    pub fn light(&self) -> &[Q] {
        &self.light
    }

    /// The light field, mutably.
    #[inline]
    pub fn light_mut(&mut self) -> &mut [Q] {
        &mut self.light
    }

    /// The heat capacity of each coarse cell, J/K: **enthalpy** grid, one buffer,
    /// class `Q`.
    ///
    /// The denominator of `T = T_ref + H/C_cell` (ADR-044), recomputed once a tick
    /// from the composition the voxels actually hold and never cached — the
    /// folded `Derived::energy().c_cell` is a different number for a different
    /// purpose, the derivation of `k_E`, and using it in a tick would reverse
    /// ADR-044 without saying so.
    ///
    /// One buffer, because no kernel gathers over it: `kernels/temperature.rs`
    /// writes a cell it does not read.
    #[inline]
    #[must_use]
    pub fn heat_capacity(&self) -> &[Q] {
        &self.heat_capacity
    }

    /// The heat capacity field, mutably.
    #[inline]
    pub fn heat_capacity_mut(&mut self) -> &mut [Q] {
        &mut self.heat_capacity
    }

    /// The temperature of each coarse cell, kelvin: enthalpy grid, one buffer,
    /// class `Q`.
    ///
    /// **A quantity of the coarse cell** — `2^(3*lod)` fine voxels share one `T`,
    /// and the Q10 factor of a reaction reads the covering one (ADR-062, and
    /// [`World::enthalpy_cell_of`] is the mapping). The enthalpy grid and never
    /// the velocity one: they differ, and the wrong one hands a reaction the
    /// temperature of a plausible neighbour, which appears in no invariant because
    /// this is class `Q`.
    #[inline]
    #[must_use]
    pub fn temperature(&self) -> &[Q] {
        &self.temperature
    }

    /// The temperature field, mutably.
    #[inline]
    pub fn temperature_mut(&mut self) -> &mut [Q] {
        &mut self.temperature
    }

    /// Everything one dispatch of `process::Temperature::apply` touches, in its
    /// argument order: `(src32, src64, enthalpy_read, heat_capacity, temperature)`.
    ///
    /// Half of `TODO(one-borrow-per-dispatch)` on [`World::amount_slices_mut`],
    /// closed. That TODO said outright that a combined accessor could not be
    /// designed before the temperature operator existed, because nobody knew what
    /// it would want; it exists now, and this is the signature it wants. Five
    /// borrows of one aggregate that a caller could not otherwise hold at once —
    /// two shared reads of the amount fields, one shared read of the enthalpy
    /// field's **front** buffer, and the two coarse outputs.
    ///
    /// `enthalpy().read()` and not `write_mut()`, and the difference is invisible
    /// to the compiler and to a grep. The back buffer holds state `N+1` or
    /// whatever the last swap left there; a temperature derived from it is
    /// entirely plausible, is in no invariant, and is caught only by
    /// `the_enthalpy_is_read_from_the_front_buffer`.
    ///
    /// The other half of the TODO is still open: the reaction kernel wants these
    /// amount slices *mutably* together with `energy_delta`, and that is a
    /// different set of borrows and a different accessor.
    #[inline]
    #[allow(clippy::type_complexity)]
    // Five slices against a clippy threshold that would rather see a struct. The
    // shape is the kernel's argument list, and naming a type for it would buy a
    // lint and cost the property that a reader can see the dispatch contract at
    // the accessor — the same trade `amount_slices_mut` makes one function up.
    pub fn temperature_slices_mut(&mut self) -> (&[M32], &[M64], &[M64], &mut [Q], &mut [Q]) {
        (
            self.amounts_32.as_ref().map_or(&[][..], Field::read),
            self.amounts_64.as_ref().map_or(&[][..], Field::read),
            // `lane(0)` and not `read()`: the enthalpy field is one lane and its
            // buffer carries the ghost cell after it (ADR-059). The operator
            // walks cells of the coarse grid and the reservoir is not one.
            self.enthalpy.lane(0),
            &mut self.heat_capacity,
            &mut self.temperature,
        )
    }

    /// The prescribed velocity field: coarse grid, one buffer,
    /// [`VELOCITY_COMPONENTS`] entries per cell.
    #[inline]
    #[must_use]
    pub fn velocity(&self) -> &[Q] {
        &self.velocity
    }

    /// The prescribed velocity field, mutably.
    #[inline]
    pub fn velocity_mut(&mut self) -> &mut [Q] {
        &mut self.velocity
    }

    /// The stored potential the velocity is the curl of (ADR-069): same grid,
    /// same shape, one buffer.
    #[inline]
    #[must_use]
    pub fn velocity_potential(&self) -> &[Q] {
        &self.velocity_potential
    }

    /// The stored potential, mutably.
    #[inline]
    pub fn velocity_potential_mut(&mut self) -> &mut [Q] {
        &mut self.velocity_potential
    }

    /// Fill the ghost cell of every lane of every field from the reservoir
    /// (ADR-059).
    ///
    /// `amount_out` is indexed by **substance**, in declaration order, in that
    /// substance's storage units — `round(conc_out_i * units_per_mol_i *
    /// V_voxel)`, rounded once by the loader and never in a tick. The narrowing
    /// to a lane happens here, through [`World::lane_of`], which is the one door
    /// (ADR-056).
    ///
    /// `enthalpy_out` is the enthalpy of a **coarse** cell of the reservoir at
    /// `t_out`, in the storage units of the energy scale. The field lives on the
    /// coarse grid, so `sum(n_i * c_p,i)` there is taken at `V_cell =
    /// (dx*2^lod)^3`; at `lod = 2` a `V_voxel` in its place is a factor of
    /// sixty-four, and nothing downstream notices — the sign of the flux is
    /// right, the counter records what moved, and the energy residual closes.
    ///
    /// Plain integers rather than a `config::DerivedReservoir`, because `world/`
    /// does not depend on `config/` and adding the dependency to save a caller
    /// two lines would put the loader underneath the buffers.
    ///
    /// # Errors
    ///
    /// Returns an error if `amount_out` is not one entry per substance, or if an
    /// entry does not fit the storage width the loader derived for that
    /// substance — which would mean the ghost overflows the field on the first
    /// exchange, and `config::validate` already refuses `conc_out > max_conc` for
    /// exactly that reason.
    pub fn seed_ghosts(&mut self, amount_out: &[i128], enthalpy_out: M64) -> Result<()> {
        let n_substances = self.registry.n_substances();
        if amount_out.len() != n_substances as usize {
            bail!(
                "the reservoir declares {} amounts and the registry holds {} \
                 substances: the array is indexed by substance, in declaration \
                 order (ADR-059)",
                amount_out.len(),
                n_substances
            );
        }

        for s in 0..n_substances {
            let units = amount_out[s as usize];
            let id = self.registry.id_of(s).to_string();
            match self.lane_of(s) {
                LaneRef::Narrow(lane) => {
                    let Ok(units) = i32::try_from(units) else {
                        bail!(
                            "the reservoir holds {units} units of `{id}` and the \
                             loader derived a 32-bit storage for it (ADR-040): the \
                             ghost cell would overflow the field on the first \
                             exchange"
                        );
                    };
                    let field = self
                        .amounts_32
                        .as_mut()
                        .expect("a narrow lane exists only if the narrow field does");
                    field.set_ghost(lane, M32::new(units));
                }
                LaneRef::Wide(lane) => {
                    let Ok(units) = i64::try_from(units) else {
                        bail!(
                            "the reservoir holds {units} units of `{id}`, which is \
                             past the 64-bit storage the loader derived for it \
                             (ADR-040)"
                        );
                    };
                    let field = self
                        .amounts_64
                        .as_mut()
                        .expect("a wide lane exists only if the wide field does");
                    field.set_ghost(lane, M64::new(units));
                }
            }
        }

        self.enthalpy.set_ghost(0, enthalpy_out);
        Ok(())
    }

    /// What the ghost cell holds for substance `s`, in its storage units.
    ///
    /// # Panics
    ///
    /// If `s` is not a substance index of this world's registry.
    #[must_use]
    pub fn ghost_of(&self, s: u32) -> i64 {
        match self.lane_of(s) {
            LaneRef::Narrow(lane) => self
                .amounts_32
                .as_ref()
                .expect("a narrow lane exists only if the narrow field does")
                .ghost(lane)
                .to_i64(),
            LaneRef::Wide(lane) => self
                .amounts_64
                .as_ref()
                .expect("a wide lane exists only if the wide field does")
                .ghost(lane)
                .to_i64(),
        }
    }

    /// What the ghost cell of the enthalpy field holds, in the storage units of
    /// the energy scale.
    #[inline]
    #[must_use]
    pub fn enthalpy_ghost(&self) -> M64 {
        self.enthalpy.ghost(0)
    }

    /// The cell of the **enthalpy** grid that covers a fine voxel.
    ///
    /// SPEC section 1.5, quoted because the spec is frozen (ADR-032):
    ///
    /// ```text
    /// coarse_idx = (x >> lod)
    ///            + (y >> lod) * (NX >> lod)
    ///            + (z >> lod) * (NX >> lod) * (NY >> lod)
    /// ```
    ///
    /// **The shift goes per axis.** `fine_idx >> lod` and
    /// `coarse * 2^(3*lod) + f` cut the index space into disjoint pieces exactly
    /// the way the right formula does, so every conservation test in the project
    /// stays green under either: the sums close and the heat is merely credited
    /// to a stripe of sixty-four along X instead of a 4x4x4 cube. What is lost is
    /// the zoning that ADR-045 rejected the scalar LEDGER-phase reduction in
    /// order to keep. The corpus pushes towards the mistake, too:
    /// `coarse_idx = fine_idx >> lod` is printed as a *definition* in
    /// `CONFIG_SCHEMA.md` (lines 190 and 504), in `config/schema.rs` and in
    /// `config/validate.rs` in three places.
    ///
    /// # Why by role and not `coarse_cell_of(idx, lod)`
    ///
    /// Because there are two coarse grids and they are different (`lod = 2` for
    /// enthalpy, `lod = 1` for velocity in the eco regime). The reaction kernel
    /// reads the temperature of the covering **enthalpy** cell (ADR-062); a
    /// `cnx`/`cny` taken from the velocity grid gives it the temperature of a
    /// plausible, neighbouring, wrong cell — and temperature is class `Q`, so
    /// that error appears in no invariant at all. A single function taking a
    /// `lod` makes the mix-up expressible; two named ones do not.
    ///
    /// # Panics
    ///
    /// If `fine_idx` is outside the fine grid (debug builds).
    #[inline]
    #[must_use]
    pub fn enthalpy_cell_of(&self, fine_idx: u32) -> u32 {
        covering_cell(&self.grid, &self.enthalpy_grid, self.enthalpy_lod, fine_idx)
    }

    /// The cell of the **velocity** grid that covers a fine voxel. See
    /// [`World::enthalpy_cell_of`] for the formula and for why these are two
    /// functions.
    #[inline]
    #[must_use]
    pub fn velocity_cell_of(&self, fine_idx: u32) -> u32 {
        covering_cell(&self.grid, &self.velocity_grid, self.velocity_lod, fine_idx)
    }
}

/// The per-axis shift of SPEC section 1.5, written once.
///
/// `coarse.nx()` is `fine.nx() >> lod` by construction of [`coarse_grid`], and
/// it is read off the coarse grid rather than recomputed here so that the two
/// cannot disagree — the mirror of the mistake `kernels/fold.rs` names in
/// `coarse_coords`, a decode of the coarse index by the **fine** extents.
#[inline]
fn covering_cell(fine: &Grid, coarse: &Grid, lod: u32, fine_idx: u32) -> u32 {
    let (x, y, z) = fine.coords(fine_idx);
    (x >> lod) + (y >> lod) * coarse.nx() + (z >> lod) * coarse.nx() * coarse.ny()
}

/// The coarse grid at `lod` bits of coarsening: extents shifted **per axis**,
/// boundary conditions inherited.
///
/// The one place the coarse extents are formed. Two ways to get them wrong, and
/// both are quiet:
///
/// **From `n_voxels`.** `(nx*ny*nz) >> (3*lod)` equals the product of the
/// shifted extents only when every extent divides. On a grid where one does not,
/// the coarse field comes out with more cells than there are covering cubes, the
/// edge of the domain is covered by nothing, and the symptom is absorbed light
/// that landed nowhere — indistinguishable from "there is not much light".
///
/// **Boundary conditions defaulted.** `Grid::new` wants six faces and
/// `[Boundary::Periodic; 6]` is the shortest thing to write. Under it heat
/// leaves through a closed floor and comes back through the lid; the energy
/// invariant closes **exactly**, because periodic transport conserves no worse
/// than closed, and the temperature field looks plausible. No record declares
/// the inheritance — the choice is made here, and named here, which is what
/// `the_coarse_grid_keeps_the_boundary_conditions_of_the_fine_one` holds.
///
/// # Errors
///
/// Returns an error naming the axis and both numbers if an extent is not
/// divisible by `2^lod`, and if `lod` is past the width of the index.
pub fn coarse_grid(fine: &Grid, lod: u32) -> Result<Grid> {
    if lod >= u32::BITS {
        bail!(
            "a lod of {lod} shifts a u32 extent out of existence; the declared \
             range is 0..=2 (QUANTITIES.md section 1)"
        );
    }
    let span = 1u32 << lod;

    for (extent, axis) in [
        (fine.nx(), Axis::X),
        (fine.ny(), Axis::Y),
        (fine.nz(), Axis::Z),
    ] {
        if extent % span != 0 {
            bail!(
                "grid axis {axis:?} has {extent} voxels, which is not divisible \
                 by 2^{lod} = {span}: `{extent} >> {lod}` would silently drop \
                 the last {} voxels of the axis, and they would be covered by no \
                 coarse cell at all",
                extent % span
            );
        }
    }

    let boundary = Face::ALL.map(|face| fine.boundary(face));
    Grid::new(
        fine.nx() >> lod,
        fine.ny() >> lod,
        fine.nz() >> lod,
        boundary,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, SubstanceDecl, SubstanceSlot};

    /// A deliberately non-cubic fine grid whose three **coarse** extents at
    /// `lod = 2` are pairwise different and none of them one: `2 x 3 x 4`.
    ///
    /// Not decoration. With a coarse extent of one the term of that axis
    /// vanishes from every expectation, and `z0 = cz` in place of
    /// `z0 = cz << lod` stays green — the lesson `kernels/fold.rs` writes down
    /// about its own fixture.
    const NX: u32 = 8;
    const NY: u32 = 12;
    const NZ: u32 = 16;
    const ENTHALPY_LOD: u32 = 2;
    const VELOCITY_LOD: u32 = 1;

    fn layout() -> WorldLayout {
        WorldLayout {
            enthalpy_lod: ENTHALPY_LOD,
            velocity_lod: VELOCITY_LOD,
        }
    }

    /// Periodic in X and Y, closed floor and lid in Z — the eco-regime default
    /// of SPEC section 1.6, minus the `exchange` face `Grid::new` refuses.
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

    fn decl(id: &str, width: Width) -> SubstanceDecl {
        SubstanceDecl {
            id: id.to_string(),
            width,
            k: 0,
        }
    }

    /// A registry whose one wide substance is **not** at index zero, so the lane
    /// table is not the identity anywhere it could be mistaken for one.
    fn mixed_registry() -> Registry {
        Registry::new(&[
            decl("A", Width::Bits32),
            decl("B", Width::Bits32),
            decl("WATER", Width::Bits64),
            decl("C", Width::Bits32),
        ])
        .unwrap()
    }

    fn world() -> World {
        World::new(floored(NX, NY, NZ), mixed_registry(), &layout()).unwrap()
    }

    #[test]
    fn the_coarse_grid_keeps_the_boundary_conditions_of_the_fine_one() {
        // No ADR declares this inheritance, so the choice is made in code and
        // named here. `[Boundary::Periodic; 6]` is the shortest thing to write
        // and it sends heat out through a closed floor and back through the lid,
        // with the energy invariant closing exactly and the temperature field
        // looking plausible.
        let fine = floored(NX, NY, NZ);
        let world = world();

        for face in Face::ALL {
            assert_eq!(
                world.enthalpy_grid().boundary(face),
                fine.boundary(face),
                "face {face:?} of the enthalpy grid"
            );
            assert_eq!(
                world.velocity_grid().boundary(face),
                fine.boundary(face),
                "face {face:?} of the velocity grid"
            );
        }
        assert_eq!(
            world.enthalpy_grid().boundary(Face::ZMinus),
            Boundary::Closed
        );

        // And the extents are the shift per axis, not a division of n_voxels.
        assert_eq!(world.enthalpy_grid().nx(), NX >> ENTHALPY_LOD);
        assert_eq!(world.enthalpy_grid().ny(), NY >> ENTHALPY_LOD);
        assert_eq!(world.enthalpy_grid().nz(), NZ >> ENTHALPY_LOD);
    }

    #[test]
    fn a_grid_not_divisible_by_its_lod_is_refused() {
        // The host-side twin of the validator's
        // `a_grid_not_divisible_by_its_coarsest_lod_is_rejected`: `World` is
        // public and can be built without a validator, and `nx >> lod` on an
        // indivisible extent silently loses the edge of the domain — fewer
        // coarse cells than there are covering cubes, and fine voxels that fall
        // into none.
        let err = coarse_grid(&floored(9, 12, 16), 2).unwrap_err().to_string();
        assert!(err.contains('X'), "the axis has to be named: {err}");
        assert!(err.contains('9'), "the extent has to be named: {err}");
        assert!(err.contains('4'), "the divisor has to be named: {err}");

        assert!(coarse_grid(&floored(8, 6, 16), 2).is_err());
        assert!(coarse_grid(&floored(8, 12, 18), 2).is_err());
        assert!(coarse_grid(&floored(8, 12, 16), 2).is_ok());

        // Through the constructor, and per role: the same grid is legal at one
        // lod and refused at the other, so a message that named neither would
        // leave the reader guessing which field is at fault.
        let refused = World::new(
            floored(8, 12, 16),
            mixed_registry(),
            &WorldLayout {
                enthalpy_lod: 3,
                velocity_lod: 1,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(refused.contains("enthalpy"), "{refused}");
    }

    #[test]
    fn a_coarse_cell_covers_exactly_its_fine_cells() {
        // SPEC section 1.5 written out per axis in the test rather than borrowed
        // from the code, and then checked from the other side: the preimage of
        // each coarse cell is exactly the cube of `2^(3*lod)` fine voxels.
        let world = world();
        let cnx = NX >> ENTHALPY_LOD;
        let cny = NY >> ENTHALPY_LOD;
        let cnz = NZ >> ENTHALPY_LOD;
        let span = 1u32 << ENTHALPY_LOD;
        let n_coarse = cnx * cny * cnz;

        let mut members: Vec<Vec<(u32, u32, u32)>> = vec![Vec::new(); n_coarse as usize];
        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    let fine_idx = world.grid().index(x, y, z);
                    let expected = (x >> ENTHALPY_LOD)
                        + (y >> ENTHALPY_LOD) * cnx
                        + (z >> ENTHALPY_LOD) * cnx * cny;
                    assert_eq!(
                        world.enthalpy_cell_of(fine_idx),
                        expected,
                        "voxel ({x}, {y}, {z})"
                    );
                    members[expected as usize].push((x, y, z));
                }
            }
        }

        for coarse in 0..n_coarse {
            let cell = &members[coarse as usize];
            assert_eq!(
                cell.len(),
                (span * span * span) as usize,
                "coarse cell {coarse} covers the wrong number of fine voxels"
            );
            let x0 = cell.iter().map(|v| v.0).min().unwrap();
            let y0 = cell.iter().map(|v| v.1).min().unwrap();
            let z0 = cell.iter().map(|v| v.2).min().unwrap();
            for &(x, y, z) in cell {
                assert!(
                    x >= x0
                        && x < x0 + span
                        && y >= y0
                        && y < y0 + span
                        && z >= z0
                        && z < z0 + span,
                    "coarse cell {coarse} is not a cube: ({x}, {y}, {z}) is outside \
                     [{x0}, {y0}, {z0}) + {span}"
                );
            }
        }
    }

    #[test]
    fn the_two_coarse_grids_are_told_apart_by_role() {
        // The point is the door that does not exist: there is no
        // `coarse_cell_of(idx, lod)`, because the reaction kernel reads the
        // temperature of the covering *enthalpy* cell (ADR-062) and a `cnx`/`cny`
        // taken from the velocity grid gives it a plausible, neighbouring, wrong
        // cell — in no invariant, because temperature is class `Q`.
        let world = world();
        assert_ne!(world.enthalpy_grid(), world.velocity_grid());

        let mut differ = 0;
        for fine_idx in 0..world.grid().n_voxels() {
            assert!(world.enthalpy_cell_of(fine_idx) < world.enthalpy_grid().n_voxels());
            assert!(world.velocity_cell_of(fine_idx) < world.velocity_grid().n_voxels());
            if world.enthalpy_cell_of(fine_idx) != world.velocity_cell_of(fine_idx) {
                differ += 1;
            }
        }
        assert!(
            differ > 0,
            "the two mappings agreed everywhere, so this fixture cannot tell them apart"
        );
    }

    #[test]
    fn a_substance_resolves_to_a_lane_and_never_to_its_own_index() {
        // Pinned as a table rather than as `assert_ne!(lane, s)` in a loop: that
        // assertion is false at `s == 0` and vacuous on a single-width registry,
        // so it would fail honest code and pass the shortcut it is meant to
        // catch (the argument of `the_lane_table_is_not_the_identity`).
        let world = world();
        assert_eq!(world.lane_of(0), LaneRef::Narrow(0));
        assert_eq!(world.lane_of(1), LaneRef::Narrow(1));
        assert_eq!(world.lane_of(2), LaneRef::Wide(0));
        assert_eq!(world.lane_of(3), LaneRef::Narrow(2));

        // And the registry agrees, because there is one mapping and not two.
        for s in 0..world.registry().n_substances() {
            let SubstanceSlot { width, lane, .. } = world.registry().slot(s);
            let expected = match width {
                Width::Bits32 => LaneRef::Narrow(lane),
                Width::Bits64 => LaneRef::Wide(lane),
            };
            assert_eq!(world.lane_of(s), expected, "substance {s}");
        }
    }

    #[test]
    fn an_absent_width_class_hands_the_kernel_an_empty_slice() {
        // Both mirrors, because ADR-040's rule is general and not about water.
        // A dummy lane in place of the empty class would shift the addressing of
        // the *other* class by nothing and of this one by one — and `Field::new`
        // refusing zero lanes is exactly the nudge towards allocating it.
        let narrow_only =
            Registry::new(&[decl("A", Width::Bits32), decl("B", Width::Bits32)]).unwrap();
        let world = World::new(floored(NX, NY, NZ), narrow_only, &layout()).unwrap();
        assert!(world.amounts_64().is_none());
        assert!(world.amounts_32().is_some());
        assert_eq!(world.registry().width_mask(), 0);
        let (narrow, wide) = world.amount_slices();
        assert!(wide.is_empty());
        // Two lanes of `n_voxels + 1`: the buffer a kernel is pointed at carries
        // the ghost cell of every lane (ADR-059).
        assert_eq!(narrow.len(), (2 * (NX * NY * NZ + 1)) as usize);

        let wide_only =
            Registry::new(&[decl("A", Width::Bits64), decl("B", Width::Bits64)]).unwrap();
        let mut world = World::new(floored(NX, NY, NZ), wide_only, &layout()).unwrap();
        assert!(world.amounts_32().is_none());
        assert!(world.amounts_64().is_some());
        let (narrow, wide) = world.amount_slices();
        assert!(narrow.is_empty());
        assert_eq!(wide.len(), (2 * (NX * NY * NZ + 1)) as usize);

        let (src32, src64, dst32, dst64) = world.amount_slices_mut();
        assert!(src32.is_empty() && dst32.is_empty());
        assert_eq!(src64.len(), (2 * (NX * NY * NZ + 1)) as usize);
        assert_eq!(dst64.len(), src64.len());
    }

    #[test]
    fn the_energy_accumulator_is_single_buffered_and_sixty_four_bit() {
        // Both halves are pinned, because three places in the corpus say `i32`
        // for this quantity (ADR-045, `QUANTITIES.md` section 3, the old skeleton
        // in `ARCHITECTURE.md`) and ADR-062 revokes them in prose one has to read
        // to the end of. `M32::from_i64_clamping` asserts in debug and
        // **saturates silently in release**.
        let mut world = world();
        assert_eq!(world.energy_delta().len(), (NX * NY * NZ) as usize);
        assert_eq!(world.light().len(), (NX * NY * NZ) as usize);

        // A value no `i32` holds: the declared span of one voxel for one tick is
        // 3.08e16 units (ADR-062).
        let span = M64::new(30_800_000_000_000_001);
        world.energy_delta_mut()[7] = span;
        assert_eq!(world.energy_delta()[7], span);

        // One buffer, and this type never clears it: ADR-045 keeps it single
        // buffered precisely because the reaction kernel overwrites its cell, so
        // no clearing pass exists to be forgotten.
        let _ = world.amount_slices_mut();
        let _ = world.enthalpy_mut();
        assert_eq!(world.energy_delta()[7], span);

        // The enthalpy field is the other way round: coarse, wide and double
        // buffered, because it diffuses.
        assert_eq!(
            world.enthalpy().n_voxels(),
            world.enthalpy_grid().n_voxels()
        );
        assert_eq!(world.enthalpy().lanes(), 1);
        world.enthalpy_mut().write_mut()[3] = span;
        assert_eq!(world.enthalpy().read()[3], M64::ZERO);
        world.enthalpy_mut().swap();
        assert_eq!(world.enthalpy().read()[3], span);
    }

    #[test]
    fn the_two_derived_fields_are_single_buffered_on_the_enthalpy_grid() {
        // Both halves matter and neither follows from the other. The **grid** is
        // the enthalpy one and not the velocity one: a world has two coarse grids
        // and they differ, and a temperature buffer sized by the wrong one is
        // still a plausible length — here 96 cells against 384, which is a panic,
        // and on a cubic grid nothing at all. The **buffer count** is one, because
        // no kernel gathers over either: `kernels/temperature.rs` writes cells it
        // does not read.
        let mut world = world();
        let cells = world.enthalpy_grid().n_voxels() as usize;
        assert_eq!(world.heat_capacity().len(), cells);
        assert_eq!(world.temperature().len(), cells);
        assert_ne!(cells, world.velocity_grid().n_voxels() as usize);
        assert_ne!(cells, world.grid().n_voxels() as usize);

        // No swap, no second buffer, and nothing here clears them between ticks —
        // the operator rewrites both in full before anything reads them, which is
        // the argument ADR-045 makes for `energy_delta`.
        world.temperature_mut()[3] = Q::from_f64(310.0);
        world.heat_capacity_mut()[3] = Q::from_f64(2.5);
        let _ = world.amount_slices_mut();
        world.enthalpy_mut().swap();
        assert_eq!(world.temperature()[3], Q::from_f64(310.0));
        assert_eq!(world.heat_capacity()[3], Q::from_f64(2.5));

        // And the combined accessor hands out the same two buffers plus the
        // **front** of the enthalpy field, which is what the operator reads.
        world.enthalpy_mut().write_mut()[3] = M64::new(77);
        let (_, _, enthalpy, capacity, temperature) = world.temperature_slices_mut();
        assert_eq!(enthalpy.len(), cells);
        assert_eq!(enthalpy[3], M64::ZERO, "the read buffer, not the write one");
        assert_eq!(capacity.len(), cells);
        assert_eq!(temperature[3], Q::from_f64(310.0));
    }

    #[test]
    fn the_velocity_field_is_three_components_of_its_own_grid() {
        // ADR-069 prices `A` and `u` at `3 * 64^3 * 4 B` each. The order of the
        // components is undecided, so nothing here indexes into the buffer — see
        // `TODO(velocity-layout)`.
        let world = world();
        let cells = world.velocity_grid().n_voxels();
        assert_eq!(
            cells,
            (NX >> VELOCITY_LOD) * (NY >> VELOCITY_LOD) * (NZ >> VELOCITY_LOD)
        );
        assert_eq!(
            world.velocity().len(),
            (cells * VELOCITY_COMPONENTS) as usize
        );
        assert_eq!(world.velocity_potential().len(), world.velocity().len());
    }
}
