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
//! # Which buffers live here at all, and it is a criterion rather than a list
//!
//! **A buffer lives in `World` if and only if it has a reader standing earlier
//! in the tick than its writer — the reader of the next tick included** — which
//! is exactly when a snapshot has to carry it for a run to continue from the
//! middle bit for bit. Everything else lives in [`process::Scratch`]. That is
//! ADR-086, and it is the reason this table has three rows and not twelve: every
//! buffer of class `Q` the type used to own is written and read inside one
//! [`process::Tick::advance`], so none of them is state and none of them is
//! here.
//!
//! [`process::Scratch`]: crate::process::Scratch
//! [`process::Tick::advance`]: crate::process::Tick::advance
//!
//! | field | grid | buffers | width |
//! |---|---|---|---|
//! | amounts | fine | two | `i32` and `i64`, derived (ADR-040) |
//! | enthalpy | coarse | two | `i64` (ADR-062) |
//!
//! Two buffers are for gather transport: a kernel may not read the buffer it
//! writes (ADR-034). A quantity that no kernel gathers over needs one, and every
//! such quantity of this world is now `Scratch`'s.
//!
//! The consequence worth stating on its own, because it is what makes the
//! criterion worth having: **no buffer of class `Q` is left here**, so the
//! snapshot format needs no byte door for `Q` at all (`TODO(snapshot-q)` in
//! `observe/snapshot.rs`), and the promise of ADR-016 — restart from any
//! snapshot — becomes satisfiable for S0 rather than provably unsatisfiable.
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
//! Its former companion, the temperature denominator `C_cell` of ADR-062, is
//! not here either, and that is the decision rather than an omission: ADR-086
//! recomputes the denominator **at each of its two readers**, so
//! `process::Temperature` is dispatched twice a tick and both of its outputs are
//! written and read inside one `Tick::advance`. ADR-044 stands entire and is the
//! reason the arrangement is legal — two recomputations are two computations
//! from state, not one computation and one read of something stored.

use anyhow::{Context, Result, bail};

use super::field::{Field, Field32, Field64};
use super::grid::{Axis, Face, Grid};
use super::registry::{Registry, Width};
use crate::numeric::{M32, M64};

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
    /// The grid of the prescribed velocity field. Shape and not a buffer: the
    /// four buffers of step `b` are `process::Scratch`'s since ADR-086, and the
    /// extents they are sized by are still this world's.
    velocity_grid: Grid,
    velocity_lod: u32,
    /// Double buffered: the field diffuses, and thermal diffusion is the fastest
    /// transport in the system (ADR-062).
    ///
    /// The last field of this struct, and the last row of [`OwnedBuffers`]. The
    /// two destructurings have to be read together — see
    /// [`World::owned_buffers`].
    enthalpy: Field64,
}

/// Every buffer [`World`] owns, shared, as one value.
///
/// **The type is the enumeration.** Built by an exhaustive destructuring of
/// `World` with no rest pattern, so a tenth field added to the world is
/// `E0027` here rather than a buffer quietly missing from the snapshot — which
/// is the failure ADR-086 found twice in the file that writes it (`solar_in`
/// written nowhere and guarded by nothing, `energy_delta` written and dead).
///
/// What it cannot do is the other half, and saying so is the point: it catches a
/// new **field** of `World`, not a new **lane** inside a field, and it cannot
/// force the new field to be *written* — a `_`-binding compiles. It converts
/// "forgot" into "decided, in a visible diff line". `config/hash.rs`'s `Hashed`
/// carries the same limit, and ADR-076 states it in the same words.
#[derive(Debug)]
pub struct OwnedBuffers<'a> {
    /// The 32-bit amount field, or `None` where the registry declared no 32-bit
    /// substance.
    pub amounts_32: Option<&'a Field32>,
    /// The 64-bit amount field.
    pub amounts_64: Option<&'a Field64>,
    /// The enthalpy field.
    pub enthalpy: &'a Field64,
}

/// Every buffer [`World`] owns, mutably. See [`OwnedBuffers`].
#[derive(Debug)]
pub struct OwnedBuffersMut<'a> {
    /// The 32-bit amount field.
    pub amounts_32: Option<&'a mut Field32>,
    /// The 64-bit amount field.
    pub amounts_64: Option<&'a mut Field64>,
    /// The enthalpy field.
    pub enthalpy: &'a mut Field64,
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

        Ok(Self {
            grid,
            registry,
            amounts_32,
            amounts_64,
            enthalpy_grid,
            enthalpy_lod: layout.enthalpy_lod,
            velocity_grid,
            velocity_lod: layout.velocity_lod,
            enthalpy,
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
    ///
    /// **Four of the eight slices `process::React::apply` takes, and never more
    /// than four.** `TODO(one-borrow-per-dispatch)` used to stand here: the
    /// reaction kernel wants these *and* the accumulator in one call, they were
    /// two `&mut` borrows of one aggregate, and a host had to copy the
    /// accumulator around the dispatch — leaving a class of mistake, "forgot to
    /// copy it back", whose symptom is step `i'` crediting last tick's energy
    /// again. ADR-086 closed it by moving the owner rather than by widening the
    /// accessor: `energy_delta`, `xi_out`, the temperature and the catalysis are
    /// `process::Scratch`'s, and the two owners borrow disjoint things.
    ///
    /// Named for the dispatch and no longer for the field, because that is what
    /// it now is: the other half of the same call is
    /// `process::Scratch::react_slices_mut`.
    #[inline]
    pub fn react_slices_mut(&mut self) -> (&[M32], &[M64], &mut [M32], &mut [M64]) {
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

    /// Every buffer this type owns, shared, as one value.
    ///
    /// **The body is an exhaustive destructuring with no `..`**, and that is the
    /// whole mechanism: a tenth field added to [`World`] fails to compile here,
    /// then in [`OwnedBuffers`], then in `observe::snapshot_write`,
    /// `observe::snapshot_read_into` and the acceptance helper — all of which
    /// destructure the struct rather than reach into it by field. A single `..`
    /// anywhere in that chain turns the whole thing back into decoration, and the
    /// failure is invisible, because everything still compiles (ADR-086).
    ///
    /// The criterion the chain guards is on the module header: a buffer is here
    /// if and only if the snapshot has to carry it.
    #[must_use]
    pub fn owned_buffers(&self) -> OwnedBuffers<'_> {
        let World {
            grid: _,
            registry: _,
            amounts_32,
            amounts_64,
            enthalpy_grid: _,
            enthalpy_lod: _,
            velocity_grid: _,
            velocity_lod: _,
            enthalpy,
        } = self;
        OwnedBuffers {
            amounts_32: amounts_32.as_ref(),
            amounts_64: amounts_64.as_ref(),
            enthalpy,
        }
    }

    /// Every buffer this type owns, mutably. See [`World::owned_buffers`] for why
    /// the body is a destructuring.
    pub fn owned_buffers_mut(&mut self) -> OwnedBuffersMut<'_> {
        let World {
            grid: _,
            registry: _,
            amounts_32,
            amounts_64,
            enthalpy_grid: _,
            enthalpy_lod: _,
            velocity_grid: _,
            velocity_lod: _,
            enthalpy,
        } = self;
        OwnedBuffersMut {
            amounts_32: amounts_32.as_mut(),
            amounts_64: amounts_64.as_mut(),
            enthalpy,
        }
    }

    /// How many bytes this world's buffers hold, for the footprint line of the
    /// load report (`process::Footprint`, ADR-086).
    ///
    /// Through [`World::owned_buffers`] and never through `size_of::<World>()`:
    /// the struct is two `Option`s and a `Field` header, and the bytes are behind
    /// them. **Both** buffers of every field, because both are allocated — the
    /// snapshot writes both for the same reason (ADR-057).
    #[must_use]
    pub fn footprint_bytes(&self) -> usize {
        let OwnedBuffers {
            amounts_32,
            amounts_64,
            enthalpy,
        } = self.owned_buffers();
        let narrow = amounts_32.map_or(0, |f| 2 * f.read().len() * size_of::<M32>());
        let wide = amounts_64.map_or(0, |f| 2 * f.read().len() * size_of::<M64>());
        narrow + wide + 2 * enthalpy.read().len() * size_of::<M64>()
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

        let (src32, src64, dst32, dst64) = world.react_slices_mut();
        assert!(src32.is_empty() && dst32.is_empty());
        assert_eq!(src64.len(), (2 * (NX * NY * NZ + 1)) as usize);
        assert_eq!(dst64.len(), src64.len());
    }

    #[test]
    fn the_enthalpy_field_is_coarse_wide_and_double_buffered() {
        // `i64` and not `i32`, because three places in the corpus say `i32` for
        // this quantity (ADR-045, `QUANTITIES.md` section 3, the old skeleton in
        // `ARCHITECTURE.md`) and ADR-062 revokes them in prose one has to read to
        // the end of. Double buffered because it diffuses, and thermal diffusion
        // is the fastest transport in the system.
        //
        // What this test used to assert about the reaction accumulator, the light
        // field and the four buffers of step `b` has moved with them, to
        // `process::Scratch` (ADR-086): none of the five has a reader standing
        // earlier in the tick than its writer, so none of them is state and none
        // of them is here.
        let mut world = world();

        // A value no `i32` holds: the declared span of one voxel for one tick is
        // 3.08e16 units (ADR-062).
        let span = M64::new(30_800_000_000_000_001);
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
    fn owned_buffers_hands_out_every_buffer_and_the_amount_doors_agree_with_it() {
        // The value form of the criterion: three buffers, and the same three the
        // field-by-field accessors reach. What the compiler guarantees here is the
        // *count* — `World::owned_buffers` destructures with no `..`, so a tenth
        // field is `E0027` there and in every consumer — and what it cannot
        // guarantee is that a new field is also **written** into the file. That
        // half is `every_buffer_the_scratch_owns_is_absent_from_the_snapshot` and
        // `snapshot_covers_every_buffer_the_world_owns` in `tests/`.
        let mut world = world();
        world.enthalpy_mut().write_mut()[3] = M64::new(77);

        let OwnedBuffers {
            amounts_32,
            amounts_64,
            enthalpy,
        } = world.owned_buffers();
        assert!(
            amounts_32.is_some(),
            "the fixture declares a narrow substance"
        );
        assert!(amounts_64.is_some(), "and a wide one");
        assert_eq!(enthalpy.n_voxels(), world.enthalpy_grid().n_voxels());
        assert_eq!(enthalpy.read()[3], M64::ZERO, "the front buffer");

        // The mutable door is the same three, and `react_slices_mut` — the rename
        // of `amount_slices_mut` — still hands out the pair of each amount field
        // out of one borrow.
        let (src32, src64, dst32, dst64) = world.react_slices_mut();
        assert_eq!(src32.len(), dst32.len());
        assert_eq!(src64.len(), dst64.len());

        let OwnedBuffersMut {
            amounts_32,
            amounts_64,
            enthalpy,
        } = world.owned_buffers_mut();
        assert!(amounts_32.is_some());
        assert!(amounts_64.is_some());
        // The front buffer, and the write buffer still holds what was put there:
        // nothing in this type swaps on its own.
        assert_eq!(enthalpy.read()[3], M64::ZERO);
        assert_eq!(enthalpy.write_mut()[3], M64::new(77));
    }
}
