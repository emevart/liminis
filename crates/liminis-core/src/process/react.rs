//! The chemistry step: the derived tables flattened once, and one call of the
//! reaction kernel per voxel per tick.
//!
//! Everything `kernels/react.rs` is not allowed to know. The kernel gets
//! thirteen read-only arrays and a struct of scalars; this file is where a
//! loaded scenario becomes those arrays, where the seed becomes a `run_key`, and
//! where the loop over the voxels lives (ADR-034, ADR-041).
//!
//! # One step for the whole of the chemistry
//!
//! ADR-050, in as many words: abiotic, microbial and cellular reactions "are
//! applied by one step of the tick, by one call of the reaction kernel". So
//! [`React::apply`] runs **one** loop over the voxels and **one** `react_voxel`
//! per voxel, over every reaction the scenario declares. Splitting it — abiotic
//! first, then the rest — is the arrangement ADR-050 was written to overturn,
//! and it fails twice in ways nothing here would notice:
//!
//! - the shared competition coefficient stops being shared, so whichever group
//!   ran first gets its substrate at full demand and the rest divide what is
//!   left. Every element still balances exactly: conservation is a property of
//!   `nu`, not of the extent (ADR-027);
//! - the second call **erases** the energy increment of the first. The kernel
//!   writes its cell of the accumulator rather than adding to it, which is
//!   precisely what lets ADR-045 do without a clearing pass. The matter ledger
//!   closes; the energy one does not — and nothing computes the energy residual
//!   today, so nothing goes red.
//!
//! `all_chemistry_is_applied_by_one_call` holds both halves, and it holds the
//! second one by fixing the *discrepancy* as expected: it runs the split
//! arrangement beside the single call and demands that the two disagree, so that
//! restoring the split cannot be a silent edit.
//!
//! # The tables are the derivation, not a second one
//!
//! `e_r`, `k`, `nu`, `nu_E`, the storage widths and `V_voxel` are read from
//! [`Derived`] and are not computed again here (ADR-039, ADR-040). A second
//! derivation would agree with `config/derive.rs` to the bit today and part from
//! it on the first scenario where the overflow ceiling lost to the raise to
//! `e_r` — that is, on the first scenario carrying water (ADR-040). What this
//! file does is flatten: one entry per participant, the enthalpy record appended
//! to each reaction's block (ADR-041), and `begin`/`len` as a running prefix.
//!
//! Three indexing conventions meet here and none is interchangeable with another
//! (ADR-056):
//!
//! ```text
//! nu, nu_sub, km                    by entry
//! begin, len, e_r, cat, rid,
//! vmax, q10, t_vmax                 by reaction
//! lane, conc_per_unit               by substance
//! ```
//!
//! and the amount buffers are addressed by `lane[s]`, never by `s`.
//!
//! # What this file refuses to load
//!
//! A reaction with a non-empty `catalyst`, a non-empty `requires` or a non-empty
//! `energy_from`. All three parse, all three validate, and all three reach a
//! kernel that implements none of them — so passing them through would be a
//! reaction running outside its declared window, or energy debited from a
//! channel nobody counted, or (ADR-063 spells this one out) a catalysis column
//! of a zero buffer, which stops the whole chemistry of the scenario with both
//! halves of the invariant closing on `0 == 0`.
//!
//! # What is not here
//!
//! **The invariant.** [`super::Invariant`] has one arm, `Conserved`, and it is
//! false for chemistry in the sense the ledger measures: `ledger::residual_matter`
//! is taken per substance, and a reaction turns substances into one another. See
//! the TODO on [`React`].
//!
//! **The process id.** The roster of ADR-065 holds every spelling in one place,
//! `process::ProcessId::id`, because an id reaches `config_hash` through the
//! canonical form and a second copy here would be a second thing to rename. It
//! spells this step `reactions`, and not `reactions_abiotic` as
//! `CONFIG_SCHEMA.md` section 12 prints — a name ADR-050 made wrong by merging
//! the three steps into one.
//!
//! The **default** of `enabled` does live here, next to the invariant, which is
//! where ADR-065 puts it: [`ENABLED_BY_DEFAULT`].
//!
//! **`every_n_ticks`.** ADR-030 forbids `> 1` for a process touching a diffusive
//! field, because skipping ticks multiplies the effective `dt`. Whether the ban
//! reaches chemistry is settled by no record, and "ignore it" and "fold it into
//! `dt`" look equally like decisions — so this process reads the key not at all,
//! and one [`React::apply`] is one tick.
//!
//! **The temperature.** No operator turns enthalpy into `T` (ADR-062 gives the
//! scale and the grid, not the operator), so the field arrives as an argument
//! and who fills it is undecided.
//!
//! **The fold of step `i'`.** The increment stays on the fine grid; collecting
//! it into enthalpy is `kernels/fold.rs` and a step of its own (ADR-045).

use anyhow::{Context, Result, bail};

use crate::config::{Config, Derived, Reaction};
use crate::kernels::react::{NO_CATALYST, R_MAX, ReactParams, Rx, S_MAX, react_voxel};
use crate::numeric::{M32, M64, Q, run_key};
use crate::world::{Field32, Field64, Grid, MAX_SUBSTANCES, Registry};

/// The default of `enabled` for the reactions, and it is `false` (ADR-065).
///
/// The module header above says the id and the default do not live in this file
/// because ADR-050 merged three steps into one; ADR-065 then put the default
/// beside the invariant, which is here, so this is where it went.
///
/// `false`, and the reason is the temperature. [`React::apply`] wants `T` on the
/// enthalpy grid, no operator derives it from enthalpy and `C_cell` (ADR-062
/// gives the scale and the grid, not the operator), and `world::World` does not
/// allocate the buffer. Step `h` cannot be dispatched at all, so `true` would
/// make every scenario refuse to build a tick.
///
/// The mirror argument, and it is the strong one: a world with matter and no
/// chemistry is not the world this project is for, so the day the temperature
/// operator exists, `true` is the reading of SPEC section 8 — and it will be a
/// change of semantics for every scenario, under the guard of ADR-020.
// TODO(CONFIG_SCHEMA.md section 13 item 23): assigned by no record. It is
// settled together with the temperature operator, in `DECISIONS.md`.
pub const ENABLED_BY_DEFAULT: bool = false;

/// The three extents of the world this process needs, and no fourth.
///
/// None of the three follows from either of the others, which is why they arrive
/// together rather than as a `&World`: the fine grid carries the amounts, the
/// enthalpy grid carries the temperature, and the coarsening between them is the
/// shift the kernel takes per axis (SPEC section 1.5).
///
/// **The coarse grid is the enthalpy one and never the velocity one.** A world
/// has two, they differ (`32^3` against `64^3` over a `128^3` base), and
/// `cnx`/`cny` taken from the velocity grid give the reaction the temperature of
/// a plausible, neighbouring, wrong cell — which appears in no invariant at all,
/// because temperature is class `Q` (ADR-062, `World::enthalpy_cell_of`).
pub struct ReactShape<'a> {
    /// The fine grid: amounts, the energy accumulator, the catalysis columns.
    pub grid: &'a Grid,
    /// The grid the enthalpy field — and so the temperature — lives on.
    pub enthalpy_grid: &'a Grid,
    /// How many binary orders coarser that grid is than the fine one.
    pub enthalpy_lod: u32,
}

/// The two columns of a reaction that the corpus declares nowhere in a form this
/// file could read.
///
/// They arrive from the caller rather than being defaulted to something
/// plausible, by the precedent `WorldLayout::velocity_lod` sets for exactly this
/// situation: a number named in prose and by no key does not get invented at the
/// point of use, because an invention outlives the task that made it.
pub struct Undeclared<'a> {
    /// The reference temperature of each reaction's Q10 factor, kelvin
    /// (ADR-048).
    ///
    /// `CONFIG_SCHEMA.md` section 6 declares no such key, and
    /// `reaction_without_t_vmax_is_rejected` stands `#[ignore]`d in
    /// `config/validate.rs` with that same sentence on it. Substituting `T_ref`
    /// is not available and ADR-048 says why: `T_ref` is an arbitrary scenario
    /// zero of enthalpy *storage*, and tying kinetics to it would make the speed
    /// of the whole chemistry a function of where that zero was put — "not a
    /// saving of one key but a mistake".
    pub t_vmax: &'a [f64],
    /// The identifier of each reaction, folded from its **name** (ADR-027).
    ///
    /// It is the third counter of every draw the kernel takes. ADR-027 fixes
    /// "from the name and never from the position in the file" and names no
    /// mixer; `TODO(reaction-id)` in `kernels/react.rs` records that choosing one
    /// is world semantics of the same standing as `numeric/rng.rs`. A plausible
    /// FNV written here would outlive this file and become irreversible, so the
    /// column comes from the caller.
    pub reaction_id: &'a [u32],
}

/// One chemistry step: the flat tables of ADR-041, owned, plus the scalars the
/// kernel receives.
///
/// Built once per run and applied once per tick. Everything in it is a fold of
/// the scenario and of the derivation; nothing in it is state.
// TODO(matter-invariant): `React::invariant()` is not written, and what is
// missing is not the code. `process::Conservation` has one arm, `Conserved`, and
// `ledger::residual_matter` is computed **per substance** — so declaring
// `Invariant { matter: Conserved, energy: Conserved }` here would be a lie in
// exactly the sense the ledger measures, and it would stay green until the first
// tick with chemistry and the first call of `assert_closed`. The statement
// chemistry actually satisfies is "every conserved quantity balances while the
// substances convert", and no record says how that is worded as an arm of the
// enum. The energy half has a second hole under it: the increment lands in the
// accumulator and reaches enthalpy only through step `i'`, and
// `ledger::DomainSums` has no door for the accumulator at all.
#[derive(Clone, Debug)]
pub struct React {
    // --- by entry ---
    nu: Vec<i32>,
    nu_sub: Vec<u32>,
    km: Vec<Q>,
    // --- by reaction ---
    begin: Vec<u32>,
    len: Vec<u32>,
    e_r: Vec<u32>,
    cat: Vec<u32>,
    rid: Vec<u32>,
    vmax: Vec<Q>,
    q10: Vec<Q>,
    t_vmax: Vec<Q>,
    // --- by substance ---
    lane: Vec<u32>,
    conc_per_unit: Vec<Q>,
    /// Every scalar the kernel takes except the tick, which [`React::params`]
    /// fills in. `run_key` is folded from the seed here, once (ADR-058).
    params: ReactParams,
}

impl React {
    /// Flatten a loaded scenario into the tables of ADR-041.
    ///
    /// # Errors
    ///
    /// Returns an error if the derivation and the registry describe different
    /// registries, if either column of [`Undeclared`] is not one entry per
    /// reaction, if the shape's coarse grid is not the fine one at the declared
    /// `lod`, if a storage coefficient does not fit the `i32` the kernel table
    /// holds, if a folded scalar comes out non-finite or zero — and if any
    /// reaction declares a `catalyst`, a `requires` window or an `energy_from`
    /// channel, none of which the kernel implements.
    pub fn new(
        shape: &ReactShape<'_>,
        registry: &Registry,
        derived: &Derived,
        config: &Config,
        undeclared: &Undeclared<'_>,
        seed: u64,
    ) -> Result<Self> {
        let n_reactions = derived.reactions().len();
        let n_substances = derived.substances().len();

        check_registry(registry, derived)?;
        check_undeclared(undeclared, n_reactions)?;
        check_shape(shape)?;
        if config.reaction.len() != n_reactions {
            bail!(
                "the scenario declares {} reactions and the derivation carries \
                 {n_reactions}: the two came from different configs, and the \
                 kinetics of one would be applied to the stoichiometry of the \
                 other",
                config.reaction.len()
            );
        }
        if n_reactions > R_MAX {
            bail!(
                "{n_reactions} reactions declared, the reaction kernel sizes its \
                 local arrays at R_MAX = {R_MAX} (ADR-041)"
            );
        }
        if n_substances > MAX_SUBSTANCES as usize {
            bail!(
                "{n_substances} substances declared, at most {MAX_SUBSTANCES} can \
                 be addressed once the reserved index of enthalpy is taken out of \
                 S_MAX = {S_MAX} (ADR-041)"
            );
        }
        for reaction in &config.reaction {
            check_unimplemented_keys(reaction)?;
        }

        let v_voxel = derived.v_voxel();
        if !v_voxel.is_finite() || v_voxel <= 0.0 {
            bail!("the derived voxel volume {v_voxel} m^3 is not a volume");
        }
        if !config.dt.is_finite() || config.dt <= 0.0 {
            bail!("dt {} s is not a usable timestep", config.dt);
        }

        // The reserved index of enthalpy in the stoichiometry vector: one past
        // the last substance (ADR-041). Inside the substance range it would land
        // the enthalpy delta in the lane of a substance, which would then be
        // created and destroyed at the pace of the reactions while the *matter*
        // ledger closed, because matter and energy are counted apart (ADR-028).
        // `MAX_SUBSTANCES` is `S_MAX - 1` exactly so that this index exists.
        let s_energy = n_substances as u32;

        let conc_per_unit = concentration_per_unit(derived, v_voxel)?;

        // `lane_of` and never a hand-built `0..n`: `lane == s` is true nowhere by
        // rule since ADR-056. The one extra slot is the sentinel for `s_energy` —
        // the kernel never reads it, and it is there so that an off-by-one lands
        // on `u32::MAX` rather than out of bounds.
        let mut lane = registry.lane_of().to_vec();
        lane.push(u32::MAX);

        let mut this = Self {
            nu: Vec::new(),
            nu_sub: Vec::new(),
            km: Vec::new(),
            begin: Vec::with_capacity(n_reactions),
            len: Vec::with_capacity(n_reactions),
            e_r: Vec::with_capacity(n_reactions),
            cat: Vec::with_capacity(n_reactions),
            rid: undeclared.reaction_id.to_vec(),
            vmax: Vec::with_capacity(n_reactions),
            q10: Vec::with_capacity(n_reactions),
            t_vmax: Vec::with_capacity(n_reactions),
            lane,
            conc_per_unit,
            params: ReactParams {
                nx: shape.grid.nx(),
                ny: shape.grid.ny(),
                n_voxels: shape.grid.n_voxels(),
                n_substances: n_substances as u32,
                n_reactions: n_reactions as u32,
                // Filled per call by `params`. The template carries a zero so
                // that a mistake there is the first tick rather than a plausible
                // other one.
                tick: 0,
                // Folded once, here, out of the 64-bit seed (ADR-058). The seed
                // itself stays a run parameter and out of `config_hash`.
                run_key: run_key(seed),
                width_mask: registry.width_mask(),
                s_energy,
                lod: shape.enthalpy_lod,
                cnx: shape.enthalpy_grid.nx(),
                cny: shape.enthalpy_grid.ny(),
                volume: Q::from_f64(v_voxel),
                dt: Q::from_f64(config.dt),
            },
        };

        for (r, reaction) in derived.reactions().iter().enumerate() {
            let record = &config.reaction[r];
            if record.id != reaction.id {
                bail!(
                    "reaction {r} is `{}` in the scenario and `{}` in the \
                     derivation: the kinetics of one reaction would be applied to \
                     the stoichiometry of another, and every element would still \
                     balance",
                    record.id,
                    reaction.id
                );
            }

            // A running prefix, and never `r * len`. Blocks differ in length —
            // that is why `begin` and `len` exist rather than a stride (ADR-041)
            // — and a reaction reading somebody else's block balances by elements
            // anyway, because that block is balanced too. What the world gets is
            // then not the chemistry written in the TOML.
            this.begin.push(this.nu.len() as u32);

            for entry in &reaction.nu {
                // `try_from` and never `as`. In release a bare cast turns a
                // coefficient past `i32::MAX` into one of the opposite sign: the
                // reaction runs backwards, matter is created, and the element
                // balance is taken over the molar `s` and does not see it.
                let value = i32::try_from(entry.value).with_context(|| {
                    format!(
                        "reaction `{}`, substance `{}`: the storage coefficient \
                         nu = {} does not fit the i32 of the kernel table \
                         (ADR-039, ADR-041)",
                        reaction.id,
                        derived.substances()[entry.substance as usize].id,
                        entry.value
                    )
                })?;
                this.nu.push(value);
                this.nu_sub.push(entry.substance);
                // `km` is parallel to `nu` and is meaningful only where `nu` is
                // negative — the same set the input cap is taken over. It is
                // looked up **by the substance of this entry** and is never laid
                // out in the order the map is walked, which is alphabetical and
                // is not the declaration order the entries follow. A mismatch
                // gives the limiting term the half-saturation of another
                // substance; a rate enters no invariant, so both ledgers close
                // exactly on the wrong extent.
                this.km.push(if value < 0 {
                    let id = &derived.substances()[entry.substance as usize].id;
                    Q::from_f64(record.rate.km.get(id).copied().unwrap_or(0.0))
                } else {
                    Q::ZERO
                });
            }

            // Enthalpy travels in the same vector as the substances, as one more
            // entry at a reserved index (ADR-041). Left out, `energy_delta` is
            // identically zero; given a `nu_sub` inside the substance range, the
            // increment lands in the lane of a substance and the matter ledger
            // still closes. `len` is taken *after* it, for the same reason.
            let nu_energy = i32::try_from(reaction.nu_energy).with_context(|| {
                format!(
                    "reaction `{}`: the enthalpy coefficient nu_E = {} does not \
                     fit the i32 of the kernel table (ADR-041, ADR-062)",
                    reaction.id, reaction.nu_energy
                )
            })?;
            this.nu.push(nu_energy);
            this.nu_sub.push(s_energy);
            this.km.push(Q::ZERO);

            this.len
                .push(this.nu.len() as u32 - this.begin[this.begin.len() - 1]);
            this.e_r.push(u32::from(reaction.e_r));
            // No catalysis column exists, so the only legal value is the
            // sentinel — and a sentinel rather than column 0 of a zero buffer,
            // which is the accident ADR-063 names outright. A non-empty
            // `catalyst` was refused above.
            this.cat.push(NO_CATALYST);
            this.vmax
                .push(finite(record.rate.vmax, "rate.vmax", &reaction.id)?);
            this.q10
                .push(finite(record.rate.q10, "rate.q10", &reaction.id)?);
            this.t_vmax
                .push(finite(undeclared.t_vmax[r], "t_vmax", &reaction.id)?);
        }

        Ok(this)
    }

    /// The tables, as the kernel receives them.
    ///
    /// Borrowed rather than copied so that a test can compare them against
    /// [`Derived`] entry by entry instead of inferring them from the outcome of a
    /// run — every mistake these tables can carry conserves every element
    /// exactly, so the outcome is the one place they are not visible.
    #[inline]
    #[must_use]
    pub fn rx(&self) -> Rx<'_> {
        Rx {
            nu: &self.nu,
            nu_sub: &self.nu_sub,
            begin: &self.begin,
            len: &self.len,
            e_r: &self.e_r,
            lane: &self.lane,
            cat: &self.cat,
            rid: &self.rid,
            vmax: &self.vmax,
            q10: &self.q10,
            t_vmax: &self.t_vmax,
            km: &self.km,
            conc_per_unit: &self.conc_per_unit,
        }
    }

    /// The kernel's scalars for one tick.
    ///
    /// The one place the tick and the run key are put into their two `u32`
    /// fields, and the reason it is one place is that the compiler cannot tell
    /// them apart. Swapped, the generator still gives a uniform, unbiased stream
    /// that passes every test in `numeric/rng.rs` and is incomparable with the
    /// reference run; only a comparison of values at a fixed seed and tick sees
    /// it (ADR-058).
    #[inline]
    #[must_use]
    pub fn params(&self, tick: u32) -> ReactParams {
        ReactParams {
            tick,
            ..self.params
        }
    }

    /// How many reactions this step applies.
    #[inline]
    #[must_use]
    pub fn n_reactions(&self) -> u32 {
        self.params.n_reactions
    }

    /// How many planes of the fine grid the catalysis buffer has to hold.
    ///
    /// Zero today, and by refusal rather than by omission: a non-empty
    /// `catalyst` does not load (ADR-063, and `TODO(catalyst-class)` in
    /// `kernels/react.rs` for what is undecided about the buffer itself).
    #[inline]
    #[must_use]
    pub fn catalyst_columns(&self) -> u32 {
        self.cat.iter().filter(|&&c| c != NO_CATALYST).count() as u32
    }

    /// One tick of the whole chemistry: one pass over the voxels, one call of the
    /// kernel per voxel (ADR-050).
    ///
    /// Slices rather than `&mut World`, because `World::amount_slices_mut` and
    /// `World::energy_delta_mut` are two mutable borrows of one aggregate and a
    /// caller cannot hold both — `TODO(one-borrow-per-dispatch)` in
    /// `world/world.rs` names that gap, and this file does not close it. The
    /// price falls on the caller and is worth saying out loud: a host that copies
    /// the accumulator around the dispatch and forgets to copy it back leaves the
    /// previous tick's increment in place, and step `i'` credits a stale number
    /// that is indistinguishable from a fresh one.
    ///
    /// Afterwards the new state is in the write buffers; [`React::promote`]
    /// restores the process boundary of ADR-057.
    ///
    /// # Panics
    ///
    /// If a buffer is shorter than the shape this process was folded for.
    #[allow(clippy::too_many_arguments)]
    // Eight against a clippy threshold of seven, for the reason the kernel's own
    // signature gives: every slice here is one binding on the other side of the
    // call, and folding them into a struct would buy a lint and cost the shape.
    pub fn apply(
        &self,
        tick: u32,
        src32: &[M32],
        src64: &[M64],
        dst32: &mut [M32],
        dst64: &mut [M64],
        energy_delta: &mut [M64],
        temperature: &[Q],
        catalyst: &[Q],
    ) {
        let n_voxels = self.params.n_voxels;
        assert!(
            energy_delta.len() >= n_voxels as usize,
            "the energy accumulator holds {} cells against {n_voxels} voxels",
            energy_delta.len()
        );
        assert_eq!(
            src32.len(),
            dst32.len(),
            "the two narrow buffers are of different lengths"
        );
        assert_eq!(
            src64.len(),
            dst64.len(),
            "the two wide buffers are of different lengths"
        );

        let rx = self.rx();
        let params = self.params(tick);
        for idx in 0..n_voxels {
            react_voxel(
                src32,
                src64,
                dst32,
                dst64,
                energy_delta,
                temperature,
                catalyst,
                &rx,
                &params,
                idx,
            );
        }
    }

    /// Restore the process boundary: the front buffer holds state `N` for every
    /// lane (ADR-057).
    ///
    /// One swap per field and no copy at all, and that is a property of this
    /// process rather than a shortcut: the kernel writes **every** lane of every
    /// voxel in one pass, including the lanes whose delta came out zero, so the
    /// parity is uniform and there is no minority group to copy back. A lane
    /// skipped as "unchanged" would not be unchanged — it would be one tick
    /// stale, which is a legal state no invariant can see.
    ///
    /// Separate from [`React::apply`] because that one holds slices and this one
    /// needs the fields whose pointers are exchanged. A caller holding a `World`
    /// calls it twice, once per width: the two accessors are two `&mut self`
    /// borrows of one aggregate, which is the same gap
    /// `TODO(one-borrow-per-dispatch)` names.
    pub fn promote(&self, amounts_32: Option<&mut Field32>, amounts_64: Option<&mut Field64>) {
        if let Some(field) = amounts_32 {
            field.swap();
        }
        if let Some(field) = amounts_64 {
            field.swap();
        }
    }
}

/// What one storage unit of each substance is worth as a concentration:
/// `1 / (2^k * V_voxel)`, in mol/m^3.
///
/// **Indexed by substance**, and the whole file turns on that. Laid out by lane
/// the concentration moves by `2^(k_i - k_j)` — tens of binary orders in the
/// rate — with every balance closing exactly, and on a registry whose wide
/// substance stands first the wrong expression is *right* at `s == 0` and off by
/// one from there on (ADR-056).
fn concentration_per_unit(derived: &Derived, v_voxel: f64) -> Result<Vec<Q>> {
    let mut out = Vec::with_capacity(derived.substances().len());
    for substance in derived.substances() {
        let per_unit = 1.0 / (2f64.powi(i32::from(substance.k)) * v_voxel);
        let folded = Q::from_f64(per_unit);
        if !per_unit.is_finite() || folded <= Q::ZERO {
            bail!(
                "substance `{}` at k = {} in a voxel of {v_voxel} m^3 gives one \
                 storage unit a concentration of {per_unit} mol/m^3, which is not \
                 a positive number in `Q`. Underflow here is silent in the worst \
                 way: every concentration comes out zero, every Michaelis term \
                 with it, the whole chemistry of the scenario stands still, and \
                 both halves of the invariant close on 0 == 0",
                substance.id,
                substance.k
            );
        }
        out.push(folded);
    }
    Ok(out)
}

/// The registry and the derivation describe the same substances, in the same
/// order.
///
/// Cheap, and it guards the one mix-up that is invisible downstream: a registry
/// built from another scenario has lanes of a plausible size for a set of
/// substances that is not this one, so the amounts of the run are read and
/// written through the wrong addresses with nothing lost (ADR-056).
fn check_registry(registry: &Registry, derived: &Derived) -> Result<()> {
    if registry.n_substances() as usize != derived.substances().len() {
        bail!(
            "the registry holds {} substances and the derivation {}: the lane \
             table would address a set of lanes of the right size and the wrong \
             membership (ADR-056)",
            registry.n_substances(),
            derived.substances().len()
        );
    }
    for (s, substance) in derived.substances().iter().enumerate() {
        let s = s as u32;
        if registry.id_of(s) != substance.id {
            bail!(
                "substance {s} is `{}` in the registry and `{}` in the \
                 derivation: the scale of one substance would be used for the \
                 amounts of another",
                registry.id_of(s),
                substance.id
            );
        }
    }
    Ok(())
}

/// Both undeclared columns are one entry per reaction.
fn check_undeclared(undeclared: &Undeclared<'_>, n_reactions: usize) -> Result<()> {
    for (name, len) in [
        ("t_vmax", undeclared.t_vmax.len()),
        ("reaction_id", undeclared.reaction_id.len()),
    ] {
        if len != n_reactions {
            bail!(
                "{len} values of `{name}` were given for {n_reactions} reactions. \
                 The column is indexed by reaction and travels with the row, so \
                 one of the wrong length is read for a plausible set of reactions \
                 and it is the wrong set"
            );
        }
    }
    Ok(())
}

/// The coarse grid of the shape is the fine one at the declared `lod`.
///
/// The check exists because the mistake it catches is a *neighbouring* cell
/// rather than an out-of-range one: a world has two coarse grids, and the
/// velocity one is a binary order finer than the enthalpy one. Read with the
/// wrong extents, `coarse_of` inside the kernel lands on a plausible cell, and
/// temperature is class `Q` and enters no invariant (ADR-062).
fn check_shape(shape: &ReactShape<'_>) -> Result<()> {
    if shape.enthalpy_lod >= u32::BITS {
        bail!(
            "an enthalpy lod of {} shifts a u32 extent out of existence; the \
             declared range is 0..=2 (QUANTITIES.md section 1)",
            shape.enthalpy_lod
        );
    }
    let lod = shape.enthalpy_lod;
    let expected = (
        shape.grid.nx() >> lod,
        shape.grid.ny() >> lod,
        shape.grid.nz() >> lod,
    );
    let got = (
        shape.enthalpy_grid.nx(),
        shape.enthalpy_grid.ny(),
        shape.enthalpy_grid.nz(),
    );
    if expected != got {
        bail!(
            "the coarse grid given for the temperature is {}x{}x{} while the fine \
             grid {}x{}x{} at lod {lod} covers {}x{}x{}. A reaction reads the \
             temperature of the covering *enthalpy* cell (ADR-062), and the \
             extents of the velocity grid — the other coarse grid of a world — \
             give it a plausible neighbouring cell instead, which appears in no \
             invariant at all",
            got.0,
            got.1,
            got.2,
            shape.grid.nx(),
            shape.grid.ny(),
            shape.grid.nz(),
            expected.0,
            expected.1,
            expected.2
        );
    }
    Ok(())
}

/// The three keys that parse, validate, and reach a kernel implementing none of
/// them.
///
/// Refused rather than ignored, one message each. Silence costs differently in
/// each case and in none of them is it visible: a `requires` window dropped is a
/// reaction running outside the conditions it declares; an `energy_from` channel
/// dropped is energy debited from a counter nobody incremented; a `catalyst`
/// dropped to column 0 of a zero buffer is a rate identically zero, which stops
/// the whole chemistry of the scenario with both halves of the invariant closing
/// on `0 == 0` — the accident ADR-063 names outright.
fn check_unimplemented_keys(reaction: &Reaction) -> Result<()> {
    if !reaction.catalyst.is_empty() {
        bail!(
            "reaction `{}` declares catalyst = \"{}\", and the catalysis field it \
             names does not exist. ADR-050 has the contribution of cells and \
             guilds arrive as concentration fields folded by a separate kernel \
             after the REGULATE phase; that kernel, those fields and the \
             numbering of their columns are unwritten, and `TODO(catalyst-class)` \
             in `kernels/react.rs` records that not even the class of the buffer \
             — `Q`, or an `M` with a `conc_per_unit` — is decided. Passing the \
             reaction through against column 0 of a zero buffer is the one thing \
             that must not happen: the rate is then identically zero, the whole \
             chemistry of the scenario stands still, and both halves of the \
             invariant close on 0 == 0 (ADR-063). Write catalyst = \"\" for an \
             abiotic reaction",
            reaction.id,
            reaction.catalyst
        );
    }
    if !reaction.requires.is_empty() {
        bail!(
            "reaction `{}` declares {} `requires` window(s), and the reaction \
             kernel implements no gate. SPEC section 5 gives the windows and no \
             record says whether the gate is a hard cut-off or a factor, which \
             fields it reads, or how it behaves at the edge of a window — and the \
             flat tables of ADR-041 do not carry it at all. Ignored, it is a \
             reaction running outside the window it declares, with nothing to \
             fail",
            reaction.id,
            reaction.requires.len()
        );
    }
    if !reaction.energy_from.is_empty() {
        bail!(
            "reaction `{}` declares energy_from = \"{}\", and no step of this \
             process debits a channel. A non-empty channel is legal only for a \
             reaction whose declared enthalpy does *not* agree with the \
             enthalpies of formation of its participants, and ADR-044 does not \
             load such a reaction (ADR-059) — so the branch is unreachable, and a \
             silently ignored channel takes energy out of nowhere. Write \
             energy_from = \"\", which means \"out of this voxel's own enthalpy\"",
            reaction.id,
            reaction.energy_from
        );
    }
    Ok(())
}

/// One `f64` from the scenario crossing into `Q`, checked at the crossing.
///
/// The fold on the host is the only place a config number becomes a `Q`
/// (ADR-022, ADR-034), and it is where a non-finite one has to be caught: inside
/// a kernel `FLOAT` mode carries the infinity through the arithmetic and `FIXED`
/// mode would carry a wrapped integer that no assertion sees.
fn finite(value: f64, key: &str, reaction: &str) -> Result<Q> {
    if !value.is_finite() {
        bail!("reaction `{reaction}` declares {key} = {value}, which is not finite");
    }
    Ok(Q::from_f64(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{derive, parse, validate};
    use crate::numeric::{q_conc_64, qadd, qdiv, qmul, rand, xi};
    use crate::world::{Boundary, LaneRef, World, WorldLayout};

    // --- the fixture ------------------------------------------------------
    //
    // A TOML text assembled in the test rather than a file under `configs/`, for
    // the reason `config/derive.rs` gives about its own fixtures: a scenario file
    // is watched by the CI guard of ADR-020, and a fixture written for a test
    // would drag `WORLD_FORMAT_VERSION` along for something that is not a change
    // of semantics.
    //
    // **Numbers marked as placeholders are placeholders.** `c_p`,
    // `enthalpy_formation` and `partial_molar_volume` are declared for no
    // substance anywhere in the corpus (`CONFIG_SCHEMA.md` section 13 item 23);
    // they are written because the schema requires them. Do not copy them into
    // `configs/`.
    //
    // Three properties of the fixture are load-bearing and none is decoration:
    //
    // - **`dx` is a power of two**, `2^-13 m`, so `V_voxel = 2^-39 m^3` exactly
    //   and `conc_per_unit = 2^(39-k)` is an exact power of two in `Q`. Every
    //   rate below is then exact in `f32`, and an extent named as a number is a
    //   number rather than a rounding;
    // - **the substance declared first is the one stored 64-bit.** `ZED` is
    //   raised to `e_r` and comes out `i64`, so `lane != s` from the second
    //   substance onwards — the shape of the registry the project carries, where
    //   water stands first and takes lane 0 (ADR-056);
    // - **the declaration order of the substances is not their alphabetical
    //   order.** `rate.km` is a `BTreeMap` and is walked alphabetically, `nu` is
    //   in declaration order. A fixture where the two agree cannot see a `km`
    //   column laid out in the order of the walk.

    const HEADER: &str = r#"
name = "process-react-fixture"
dt = 1.0
beta = 0.015625
T_ref = 298.15

[conserved]
C = 12.01070
N = 14.00670

[grid]
nx = 8
ny = 8
nz = 8
dx = 1.220703125e-4

[boundary]
x_min = "closed"
x_max = "closed"
y_min = "closed"
y_max = "closed"
z_min = "closed"
z_max = "closed"

[[substance]]
id = "ZED"
molar_mass = 12.01070
typical_conc = 55500.0
max_conc = 55500.0
partial_molar_volume = 1.8e-5
settling_radius = 0.0
diffusivity = 2.3e-9
c_p = 75.3
enthalpy_formation = 0.0
composition = { C = 1 }

[[substance]]
id = "ACE"
molar_mass = 14.00670
typical_conc = 1.0e-4
max_conc = 1.0e-2
partial_molar_volume = 0.0
settling_radius = 0.0
diffusivity = 9.3e-9
c_p = 100.0
enthalpy_formation = 0.0
composition = { N = 1 }

[[substance]]
id = "ZEDACE"
molar_mass = 26.01740
typical_conc = 1.0e-3
max_conc = 1.0e-1
partial_molar_volume = 1.0e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 100.0
enthalpy_formation = -846000.0
composition = { C = 1, N = 1 }

[[substance]]
id = "DIACE"
molar_mass = 28.01340
typical_conc = 1.0e-3
max_conc = 1.0e-1
partial_molar_volume = 1.0e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 100.0
enthalpy_formation = -400000.0
composition = { N = 2 }

[[substance]]
id = "ZEDX"
molar_mass = 12.01070
typical_conc = 1.0e-3
max_conc = 1.0e-1
partial_molar_volume = 1.0e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 100.0
enthalpy_formation = -500000.0
composition = { C = 1 }
"#;

    const FIELD: &str = r#"
[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15
"#;

    /// Substance indices of the fixture, in declaration order.
    const ZED: u32 = 0;
    const ACE: u32 = 1;
    const ZEDACE: u32 = 2;
    const DIACE: u32 = 3;
    const ZEDX: u32 = 4;
    const N_SUBSTANCES: u32 = 5;

    /// The two conserved quantities of the fixture, and the composition of every
    /// substance over them. The reactions balance on this table, and
    /// `reaction_alone_conserves_each_element_exactly` checks that they still
    /// balance after the kernel has applied them in *storage* units — which is
    /// the whole question, since the storage coefficients differ from the molar
    /// ones by a per-substance power of two (ADR-039).
    const COMPOSITION: [[i64; 2]; N_SUBSTANCES as usize] = [
        [1, 0], // ZED:    one C
        [0, 1], // ACE:    one N
        [1, 1], // ZEDACE: C and N
        [0, 2], // DIACE:  two N
        [1, 0], // ZEDX:   one C, an isomer of ZED
    ];

    /// The reference temperature of every reaction's Q10 factor (ADR-048), and
    /// what the temperature field is filled with unless a test says otherwise.
    /// Exactly representable in `f32`, so `T - t_vmax` is an exact zero and the
    /// Q10 factor an exact one.
    const T_VMAX: f64 = 300.0;

    /// Ten kelvin above it: also exact in `f32`, so the difference is exactly ten
    /// and the Q10 factor exactly `q10`.
    const T_WARM: f64 = 310.0;

    /// A half-saturation so far below every concentration of the fixture that
    /// `km + S == S` in `f32` and the Michaelis term is exactly one. Not zero,
    /// because `config/validate.rs` requires `km > 0` for every input; not a
    /// plausible number, because a plausible one would make every extent below a
    /// rounding rather than a value.
    const KM_NEGLIGIBLE: &str = "1.0e-30";

    /// `vmax` giving the amination an extent of exactly four quanta:
    /// `x = vmax*dt*V*2^e_r = 2^-18 * 2^-39 * 2^59 = 4`.
    const VMAX_FOUR_QUANTA: &str = "3.814697265625e-6";

    const ENTHALPY_LOD: u32 = 2;
    const SEED: u64 = 42;
    const OTHER_SEED: u64 = 43;

    /// `ZED + ACE -> ZEDACE`, `e_r = 59`. The `extra` lines go inside the
    /// `[[reaction]]` table, before `[reaction.rate]` opens a new one.
    fn amination(vmax: &str, km: &str, extra: &str) -> String {
        format!(
            r#"
[[reaction]]
id = "amination"
enthalpy = -846000.0
inputs = {{ ZED = 1, ACE = 1 }}
outputs = {{ ZEDACE = 1 }}
{extra}

[reaction.rate]
vmax = {vmax}
t_vmax = 300.0
q10 = {Q10}
km = {km}
"#,
            Q10 = "2.0"
        )
    }

    /// The amination at negligible half-saturation, which is what every test but
    /// two wants.
    fn plain_amination(vmax: &str) -> String {
        amination(
            vmax,
            &format!("{{ ZED = {KM_NEGLIGIBLE}, ACE = {KM_NEGLIGIBLE} }}"),
            "",
        )
    }

    /// `2 ACE -> DIACE`, the second claimant on `ACE`. `e_r = 60`, so the same
    /// `vmax` gives it twice the extent of the amination.
    fn dimerisation(vmax: &str) -> String {
        format!(
            r#"
[[reaction]]
id = "dimerisation"
enthalpy = -400000.0
inputs = {{ ACE = 2 }}
outputs = {{ DIACE = 1 }}

[reaction.rate]
vmax = {vmax}
t_vmax = 300.0
q10 = 2.0
km = {{ ACE = {KM_NEGLIGIBLE} }}
"#
        )
    }

    /// `ZED -> ZEDX`, which touches neither claimant's scarce substrate.
    /// `e_r = 55`.
    fn isomerisation(vmax: &str) -> String {
        format!(
            r#"
[[reaction]]
id = "isomerisation"
enthalpy = -500000.0
inputs = {{ ZED = 1 }}
outputs = {{ ZEDX = 1 }}

[reaction.rate]
vmax = {vmax}
t_vmax = 300.0
q10 = 2.0
km = {{ ZED = {KM_NEGLIGIBLE} }}
"#
        )
    }

    /// `ZEDACE -> ZED + ACE`: a 32-bit input and a 64-bit output, which is what
    /// makes a half-promoted pair of fields visible at all.
    fn reduction() -> String {
        format!(
            r#"
[[reaction]]
id = "reduction"
enthalpy = 846000.0
inputs = {{ ZEDACE = 1 }}
outputs = {{ ZED = 1, ACE = 1 }}

[reaction.rate]
vmax = {VMAX_FOUR_QUANTA}
t_vmax = 300.0
q10 = 2.0
km = {{ ZEDACE = {KM_NEGLIGIBLE} }}
"#
        )
    }

    fn scenario(reactions: &[String]) -> String {
        format!("{HEADER}{}{FIELD}", reactions.concat())
    }

    /// The identifier of a reaction, folded from its **name** and never from its
    /// position in the file (ADR-027).
    ///
    /// A table rather than a hash, because the mixer is undecided (see
    /// [`Undeclared::reaction_id`]) and because a table is what makes the
    /// property visible: permuting the `[[reaction]]` blocks permutes this column
    /// with them.
    fn rid_of(id: &str) -> u32 {
        match id {
            "amination" => 0x5eed_0001,
            "dimerisation" => 0x5eed_0002,
            "isomerisation" => 0x5eed_0003,
            "reduction" => 0x5eed_0004,
            other => panic!("the fixture has no reaction called `{other}`"),
        }
    }

    /// Everything a test needs to run a step, in one value.
    #[derive(Debug)]
    struct Fixture {
        derived: Derived,
        world: World,
        react: React,
    }

    impl Fixture {
        fn build(text: &str, seed: u64) -> Result<Self> {
            let config = parse(text).unwrap();
            let derived = validate(&config)?;
            Self::assemble(config, derived, seed)
        }

        /// The same, past the validator. Used only where the validator refuses a
        /// scenario that this process has to refuse in its own words as well — a
        /// non-empty `energy_from` is the one such key (ADR-059).
        fn build_unvalidated(text: &str, seed: u64) -> Result<Self> {
            let config = parse(text).unwrap();
            let derived = derive(&config)?;
            Self::assemble(config, derived, seed)
        }

        fn assemble(config: Config, derived: Derived, seed: u64) -> Result<Self> {
            let registry = Registry::new(&derived.decls()).unwrap();
            let grid = Grid::new(
                config.grid.nx,
                config.grid.ny,
                config.grid.nz,
                [Boundary::Closed; 6],
            )
            .unwrap();
            let world = World::new(
                grid,
                registry,
                &WorldLayout {
                    enthalpy_lod: ENTHALPY_LOD,
                    // A binary order finer than the enthalpy grid, as ADR-069 has
                    // it in the eco regime. Different on purpose: it is the grid
                    // `cnx`/`cny` must *not* come from.
                    velocity_lod: 1,
                },
            )
            .unwrap();

            let t_vmax = vec![T_VMAX; derived.reactions().len()];
            let rid: Vec<u32> = derived.reactions().iter().map(|r| rid_of(&r.id)).collect();
            let react = React::new(
                &ReactShape {
                    grid: world.grid(),
                    enthalpy_grid: world.enthalpy_grid(),
                    enthalpy_lod: ENTHALPY_LOD,
                },
                world.registry(),
                &derived,
                &config,
                &Undeclared {
                    t_vmax: &t_vmax,
                    reaction_id: &rid,
                },
                seed,
            )?;

            Ok(Self {
                derived,
                world,
                react,
            })
        }

        fn n_voxels(&self) -> u32 {
            self.world.grid().n_voxels()
        }

        /// Put an amount into the **write** buffer of whichever field owns
        /// substance `s`, in every voxel. Through `lane_of`, the one door from a
        /// substance index to a buffer address (ADR-056).
        fn stage_everywhere(&mut self, s: u32, value: i64) {
            let n_voxels = self.n_voxels();
            match self.world.lane_of(s) {
                LaneRef::Narrow(lane) => {
                    let field = self.world.amounts_32_mut().expect("a narrow class");
                    let at = (lane * n_voxels) as usize;
                    field.write_mut()[at..at + n_voxels as usize]
                        .fill(M32::new(i32::try_from(value).unwrap()));
                }
                LaneRef::Wide(lane) => {
                    let field = self.world.amounts_64_mut().expect("a wide class");
                    let at = (lane * n_voxels) as usize;
                    field.write_mut()[at..at + n_voxels as usize].fill(M64::new(value));
                }
            }
        }

        /// Promote what was staged into state `N`, and leave a marker behind in
        /// the write buffers, so that a lane the step forgets to write shows up
        /// as the marker rather than as a plausible amount.
        fn commit(&mut self, marker: i64) {
            self.react.promote(self.world.amounts_32_mut(), None);
            self.react.promote(None, self.world.amounts_64_mut());
            if let Some(field) = self.world.amounts_32_mut() {
                field
                    .write_mut()
                    .fill(M32::new(i32::try_from(marker).unwrap()));
            }
            if let Some(field) = self.world.amounts_64_mut() {
                field.write_mut().fill(M64::new(marker));
            }
        }

        /// Amounts by substance and voxel, out of state `N`.
        fn snapshot(&self) -> Vec<Vec<i64>> {
            let n_voxels = self.n_voxels();
            let (narrow, wide) = self.world.amount_slices();
            (0..N_SUBSTANCES)
                .map(|s| {
                    (0..n_voxels)
                        .map(|idx| match self.world.lane_of(s) {
                            LaneRef::Narrow(lane) => {
                                narrow[(lane * n_voxels + idx) as usize].to_i64()
                            }
                            LaneRef::Wide(lane) => wide[(lane * n_voxels + idx) as usize].to_i64(),
                        })
                        .collect()
                })
                .collect()
        }

        /// One step, with the accumulator copied around the dispatch the way
        /// `tests/acceptance_world.rs` does and for the same reason: two mutable
        /// borrows of one `World` cannot be held at once
        /// (`TODO(one-borrow-per-dispatch)`).
        fn apply(&mut self, tick: u32, temperature: &[Q], catalyst: &[Q]) {
            let mut energy = self.world.energy_delta().to_vec();
            {
                let (src32, src64, dst32, dst64) = self.world.amount_slices_mut();
                self.react.apply(
                    tick,
                    src32,
                    src64,
                    dst32,
                    dst64,
                    &mut energy,
                    temperature,
                    catalyst,
                );
            }
            self.world.energy_delta_mut().copy_from_slice(&energy);
        }

        fn promote(&mut self) {
            self.react.promote(self.world.amounts_32_mut(), None);
            self.react.promote(None, self.world.amounts_64_mut());
        }

        /// A temperature field at `T_VMAX` in every coarse cell: the Q10 factor
        /// is then exactly one and the rate is exactly `vmax`.
        fn isothermal(&self) -> Vec<Q> {
            vec![Q::from_f64(T_VMAX); self.world.enthalpy_grid().n_voxels() as usize]
        }

        fn k_of(&self, s: u32) -> i32 {
            i32::from(self.derived.substances()[s as usize].k)
        }

        fn e_r_of(&self, r: usize) -> u32 {
            u32::from(self.derived.reactions()[r].e_r)
        }

        /// The storage coefficient of substance `s` in reaction `r`, from the
        /// derivation. Zero for a substance the reaction does not touch.
        fn nu_of(&self, r: usize, s: u32) -> i64 {
            self.derived.reactions()[r]
                .nu
                .iter()
                .find(|entry| entry.substance == s)
                .map_or(0, |entry| entry.value)
        }
    }

    /// The extent reaction `r` ran at in voxel `idx`, read out of the amounts of
    /// a product no other reaction of the fixture touches.
    ///
    /// Read rather than asked for: the step writes state, not diagnostics, and a
    /// test that asked it for `xi` would be testing a different function.
    fn extent_of(
        f: &Fixture,
        r: usize,
        product: u32,
        before: &[Vec<i64>],
        after: &[Vec<i64>],
    ) -> i64 {
        let nu = f.nu_of(r, product);
        assert!(
            nu > 0,
            "substance {product} is not a product of reaction {r}"
        );
        let delta = after[product as usize][0] - before[product as usize][0];
        assert_eq!(delta % nu, 0, "a product moved by a non-multiple of its nu");
        delta / nu
    }

    /// The extent in a named voxel.
    fn extent_at(
        f: &Fixture,
        r: usize,
        product: u32,
        idx: u32,
        before: &[Vec<i64>],
        after: &[Vec<i64>],
    ) -> i64 {
        let nu = f.nu_of(r, product);
        let delta = after[product as usize][idx as usize] - before[product as usize][idx as usize];
        assert_eq!(delta % nu, 0, "a product moved by a non-multiple of its nu");
        delta / nu
    }

    /// The imbalance of one conserved quantity over one voxel, in moles.
    ///
    /// `delta_s >> (k_s - e_r)` is exact by construction, because `nu_s` is
    /// `s_s * 2^(k_s - e_r)` and every change is `nu_s * xi` (ADR-039, ADR-027).
    /// A remainder here means a coefficient was rounded at run time.
    fn element_residual(
        f: &Fixture,
        element: usize,
        idx: u32,
        e_r: u32,
        before: &[Vec<i64>],
        after: &[Vec<i64>],
    ) -> i64 {
        let mut total = 0;
        for s in 0..N_SUBSTANCES {
            let delta = after[s as usize][idx as usize] - before[s as usize][idx as usize];
            let shift = f.k_of(s) - e_r as i32;
            assert!(shift >= 0, "k of substance {s} is below e_r");
            let scale = 1i64 << shift;
            assert_eq!(
                delta % scale,
                0,
                "substance {s} moved by {delta}, which is not a whole number of \
                 molar units at 2^{shift}: a coefficient was rounded at run time"
            );
            total += (delta / scale) * COMPOSITION[s as usize][element];
        }
        total
    }

    /// One reaction of a set of tables, with `begin` and `len` travelling with
    /// the row. The reason [`React::rx`] is public: the split arrangement of
    /// ADR-050 has to be runnable beside the single call.
    fn only_reaction<'a>(rx: &Rx<'a>, r: usize) -> Rx<'a> {
        Rx {
            nu: rx.nu,
            nu_sub: rx.nu_sub,
            km: rx.km,
            begin: &rx.begin[r..=r],
            len: &rx.len[r..=r],
            e_r: &rx.e_r[r..=r],
            lane: rx.lane,
            cat: &rx.cat[r..=r],
            rid: &rx.rid[r..=r],
            vmax: &rx.vmax[r..=r],
            q10: &rx.q10[r..=r],
            t_vmax: &rx.t_vmax[r..=r],
            conc_per_unit: rx.conc_per_unit,
        }
    }

    /// `ACCEPTANCE.md`, section "Conservation" — the process-level half of it.
    ///
    /// One reaction, substrate to spare, and the assertion is an exact zero per
    /// conserved quantity rather than a tolerance. It fails if the tables were
    /// rebuilt here instead of read out of [`Derived`], if a substance standing
    /// on both sides were applied as two entries rather than one net one, if a
    /// delta went into somebody else's lane, or if the net coefficient was
    /// narrowed from `i64` with a bare `as`.
    #[test]
    fn reaction_alone_conserves_each_element_exactly() {
        let mut f = Fixture::build(&scenario(&[plain_amination(VMAX_FOUR_QUANTA)]), SEED).unwrap();
        f.stage_everywhere(ZED, 1 << 20);
        f.stage_everywhere(ACE, 1 << 20);
        f.commit(0);

        let before = f.snapshot();
        let temperature = f.isothermal();
        f.apply(7, &temperature, &[]);
        f.promote();
        let after = f.snapshot();

        // Something moved at all. A step that does nothing conserves everything,
        // and a run whose `xi` came out zero would pass every assertion below.
        assert!(
            (0..N_SUBSTANCES).any(|s| before[s as usize] != after[s as usize]),
            "nothing moved at all"
        );

        let e_r = f.e_r_of(0);
        for idx in 0..f.n_voxels() {
            for element in 0..COMPOSITION[0].len() {
                assert_eq!(
                    element_residual(&f, element, idx, e_r, &before, &after),
                    0,
                    "conserved quantity {element} does not balance in voxel {idx}"
                );
            }
        }

        // And the extent is the one the formula names, so that the balance above
        // is a statement about a reaction that ran.
        let expected = 4;
        for idx in 0..f.n_voxels() {
            assert_eq!(
                extent_at(&f, 0, ZEDACE, idx, &before, &after),
                expected,
                "voxel {idx} did not run at vmax*dt*V*2^e_r = {expected} quanta"
            );
        }
    }

    /// `ACCEPTANCE.md`, section "Determinism" — the process-level half.
    ///
    /// The same three reactions as two TOML texts, the blocks in opposite orders.
    /// Every per-row column has to travel with its row: `begin`, `len`, `e_r`,
    /// `cat`, `rid`, `vmax`, `q10`, `t_vmax`, and the `km` entries inside the
    /// row's own block.
    ///
    /// What it catches that the kernel-level test of the same name cannot, because
    /// there the tables are written by hand:
    ///
    /// - `reaction_id` taken from the position of the record rather than from its
    ///   name (ADR-027);
    /// - `km` or `nu` laid out in an order that is a property of how the file was
    ///   typed;
    /// - `begin`/`len` computed as `r * len` instead of as a running prefix — the
    ///   blocks here differ in length, so the second text would read another
    ///   reaction's block whole.
    ///
    /// The fixture is non-trivial in two ways and neither is optional. `ACE` is
    /// oversold, so the rows can interact at all; and **every extent is
    /// fractional**, so the stochastic draw of ADR-027 decides the last quantum.
    /// With whole extents the draw reaches nothing and a `reaction_id` taken from
    /// the row index changes not one bit.
    #[test]
    fn reaction_result_is_independent_of_order_in_toml() {
        // `x = vmax*dt*V*2^e_r` comes out at 2.94, 3.15 and 3.60 quanta.
        let a = plain_amination("2.8e-6");
        let d = dimerisation("1.5e-6");
        let i = isomerisation("5.5e-5");

        let run = |text: String| {
            let mut f = Fixture::build(&text, SEED).unwrap();
            f.stage_everywhere(ZED, 1 << 20);
            f.stage_everywhere(ACE, 1 << 16);
            f.commit(0);
            let before = f.snapshot();
            let temperature = f.isothermal();
            f.apply(7, &temperature, &[]);
            f.promote();
            let after = f.snapshot();
            let energy = f.world.energy_delta().to_vec();
            (before, after, energy)
        };

        let (before, forwards, energy_forwards) = run(scenario(&[a.clone(), d.clone(), i.clone()]));
        let (_, backwards, energy_backwards) = run(scenario(&[i, d, a]));

        // All three reactions ran, or the permutation moved rows that did
        // nothing: the two that compete and the one that does not.
        for product in [ZEDACE, DIACE, ZEDX] {
            assert_ne!(
                before[product as usize], forwards[product as usize],
                "the reaction producing substance {product} did not run at all"
            );
        }
        assert_eq!(
            forwards, backwards,
            "the amounts depend on the order of the [[reaction]] blocks"
        );
        assert_eq!(
            energy_forwards, energy_backwards,
            "the energy increment depends on the order of the [[reaction]] blocks"
        );
    }

    /// The tables are the derivation flattened, not a second derivation.
    ///
    /// Checked entry by entry against [`Derived`], because the outcome of a run
    /// cannot see any of it: a `nu` computed here from `s`, `k` and `e_r` would
    /// agree with `config/derive.rs` to the bit on most scenarios and part from it
    /// on the first one where the overflow ceiling lost to the raise to `e_r`
    /// (ADR-039, ADR-040) — which this fixture has, in `ZED`.
    #[test]
    fn the_tables_are_the_derivation_and_not_a_second_one() {
        let f = Fixture::build(
            &scenario(&[
                plain_amination(VMAX_FOUR_QUANTA),
                dimerisation(VMAX_FOUR_QUANTA),
                isomerisation(VMAX_FOUR_QUANTA),
            ]),
            SEED,
        )
        .unwrap();

        // The scenario where a second derivation would part from the first.
        let zed = &f.derived.substances()[ZED as usize];
        assert!(
            zed.raised_to_e_r && zed.k > zed.k_ceiling,
            "the fixture no longer contains a substance whose scale was raised to \
             e_r, and it is the only case where two derivations disagree"
        );

        let rx = f.react.rx();
        let mut j = 0usize;
        for (r, reaction) in f.derived.reactions().iter().enumerate() {
            assert_eq!(
                rx.begin[r] as usize, j,
                "reaction {r} does not start where \
                 the running prefix says"
            );
            for entry in &reaction.nu {
                assert_eq!(rx.nu[j], i32::try_from(entry.value).unwrap());
                assert_eq!(
                    rx.nu_sub[j], entry.substance,
                    "entry {j} names a substance the derivation does not"
                );
                j += 1;
            }
            // The enthalpy record closes the block, at the reserved index, and it
            // is inside `len` (ADR-041).
            assert_eq!(rx.nu[j], i32::try_from(reaction.nu_energy).unwrap());
            assert_eq!(
                rx.nu_sub[j], N_SUBSTANCES,
                "the enthalpy record is not at \
                 the reserved index past the last substance"
            );
            j += 1;
            assert_eq!(rx.len[r] as usize, j - rx.begin[r] as usize);
            assert_eq!(rx.e_r[r], u32::from(reaction.e_r));
            assert_eq!(rx.rid[r], rid_of(&reaction.id));
            assert_eq!(rx.cat[r], NO_CATALYST);
        }
        assert_eq!(
            j,
            rx.nu.len(),
            "the tables carry entries no reaction claims"
        );

        assert_eq!(
            &rx.lane[..N_SUBSTANCES as usize],
            f.world.registry().lane_of(),
            "the lane table is not the registry's"
        );
        assert_eq!(
            f.react.params(0).width_mask,
            f.world.registry().width_mask()
        );
        assert_eq!(f.react.n_reactions(), 3);
        assert_eq!(f.react.catalyst_columns(), 0);
    }

    /// The `km` of an entry is the one declared for that entry's substance.
    ///
    /// `rate.km` is a `BTreeMap` and is walked alphabetically; `nu` is in the
    /// declaration order of the substances, and in this fixture the two disagree
    /// — `ZED` is declared first and sorts last. Laid out in the order of the
    /// walk, the limiting term takes the half-saturation of another substance,
    /// and a rate enters no invariant at all: both ledgers close exactly on the
    /// wrong extent.
    #[test]
    fn km_lands_on_the_entry_of_its_own_substance() {
        // An order of magnitude apart, and chosen so that the terms come out at
        // exact binary fractions: at `conc_ZED = 1` and `km_ZED = 1` the term is
        // 1/2, at `conc_ACE = 2^-7` and `km_ACE = 3*2^-7` it is 1/4. The minimum
        // is the second, so `x = vmax*dt*V*2^e_r / 4 = 1` quantum exactly.
        let km_zed = 1.0;
        let km_ace = 0.0234375;
        let text = scenario(&[amination(
            VMAX_FOUR_QUANTA,
            &format!("{{ ZED = {km_zed}, ACE = {km_ace} }}"),
            "",
        )]);
        let mut f = Fixture::build(&text, SEED).unwrap();

        // The column, entry by entry.
        {
            let rx = f.react.rx();
            for (j, &s) in rx.nu_sub.iter().enumerate() {
                let expected = if s == N_SUBSTANCES || rx.nu[j] >= 0 {
                    // A product, and the enthalpy record with it. `km` is
                    // meaningful only where `nu < 0`, and the enthalpy record of
                    // an exothermic reaction has `nu_E < 0` — it looks exactly
                    // like an input from inside the vector, which is why the
                    // reserved index is tested first here and skipped first in
                    // the kernel.
                    Q::ZERO
                } else if s == ZED {
                    Q::from_f64(km_zed)
                } else {
                    Q::from_f64(km_ace)
                };
                assert_eq!(rx.km[j], expected, "entry {j}, substance {s}");
            }
        }

        // And the value: the extent is the one the limiting substrate names.
        f.stage_everywhere(ZED, 1 << 20);
        f.stage_everywhere(ACE, 1 << 27);
        f.commit(0);
        let before = f.snapshot();
        let temperature = f.isothermal();
        f.apply(7, &temperature, &[]);
        f.promote();
        let after = f.snapshot();

        let rx = f.react.rx();
        let conc = |s: u32, pool: i64| q_conc_64(M64::new(pool), rx.conc_per_unit[s as usize]);
        let term = |c: Q, km: f64| qdiv(c, qadd(Q::from_f64(km), c));
        let limiting = {
            let a = term(conc(ZED, 1 << 20), km_zed);
            let b = term(conc(ACE, 1 << 27), km_ace);
            if a < b { a } else { b }
        };
        assert_eq!(
            limiting,
            Q::from_f64(0.25),
            "the fixture stopped limiting on ACE"
        );

        let rate = qmul(rx.vmax[0], limiting);
        for idx in 0..f.n_voxels() {
            let draw = rand(idx, 7, rx.rid[0], run_key(SEED));
            let expected = i64::from(xi(
                rate,
                f.react.params(7).dt,
                f.react.params(7).volume,
                f.e_r_of(0) as u8,
                draw,
            ));
            assert_eq!(expected, 1, "the fixture no longer names a whole extent");
            assert_eq!(
                extent_at(&f, 0, ZEDACE, idx, &before, &after),
                expected,
                "voxel {idx}"
            );
        }
    }

    /// `conc_per_unit` is indexed by substance and never by lane.
    ///
    /// On this registry the wide substance stands first and takes lane 0, so the
    /// wrong expression is *right* at `s == 0` and off by one from there on. What
    /// it costs is a factor of `2^(k_i - k_j)` in the rate — thirteen binary
    /// orders between `ZED` and `ACE` here — with every balance closing exactly.
    #[test]
    fn conc_per_unit_is_indexed_by_substance_and_not_by_lane() {
        let f = Fixture::build(&scenario(&[plain_amination(VMAX_FOUR_QUANTA)]), SEED).unwrap();

        // Lanes and substance indices disagree, or the test proves nothing.
        assert_ne!(
            f.world.registry().lane_of()[ACE as usize],
            ACE,
            "the fixture no longer has a registry where lane != s"
        );

        let rx = f.react.rx();
        assert_eq!(rx.conc_per_unit.len(), N_SUBSTANCES as usize);
        for s in 0..N_SUBSTANCES {
            let k = f.k_of(s);
            // `V_voxel = 2^-39` exactly, so this is an exact power of two and the
            // equality is a statement about the index rather than about rounding.
            let expected = Q::from_f64(2f64.powi(39 - k));
            assert_eq!(
                expected,
                Q::from_f64(1.0 / (2f64.powi(k) * f.derived.v_voxel())),
                "the fixture's dx is no longer a power of two"
            );
            assert_eq!(
                rx.conc_per_unit[s as usize], expected,
                "substance {s} at k = {k}"
            );
        }
    }

    /// ADR-050, word for word: one call for the whole of the chemistry.
    ///
    /// Two reactions sharing `ACE`. The demands are 4 and 8 quanta against a pool
    /// that covers three quarters of them, so the shared coefficient is exactly
    /// 3/4 and the extents are 3 and 6.
    ///
    /// Three assertions, and the second is the one that makes the split
    /// irreversible: it runs the split arrangement beside the single call and
    /// **fixes the discrepancy as expected**, so that restoring three steps cannot
    /// be a silent edit. The matter ledger closes either way; the energy one does
    /// not, and nothing computes the energy residual today.
    #[test]
    fn all_chemistry_is_applied_by_one_call() {
        let text = scenario(&[
            plain_amination(VMAX_FOUR_QUANTA),
            dimerisation(VMAX_FOUR_QUANTA),
        ]);
        let mut f = Fixture::build(&text, SEED).unwrap();
        f.stage_everywhere(ZED, 1 << 20);
        f.stage_everywhere(ACE, 147_456);
        f.commit(0);

        let before = f.snapshot();
        let temperature = f.isothermal();
        f.apply(7, &temperature, &[]);
        f.promote();
        let after = f.snapshot();

        // (3) One coefficient for both, so the extents are in the ratio of the
        //     demands. Checked as an integer cross-product rather than as a
        //     float, because that is what "one coefficient" means.
        let (want_amination, want_dimerisation) = (4, 8);
        let e_amination = extent_of(&f, 0, ZEDACE, &before, &after);
        let e_dimerisation = extent_of(&f, 1, DIACE, &before, &after);
        assert_eq!((e_amination, e_dimerisation), (3, 6));
        assert_eq!(
            e_amination * want_dimerisation,
            e_dimerisation * want_amination,
            "the two reactions were scaled by different coefficients"
        );

        // (1) The accumulator carries the enthalpy of both reactions, not of one.
        let expected = f.derived.reactions()[0].nu_energy * e_amination
            + f.derived.reactions()[1].nu_energy * e_dimerisation;
        for idx in 0..f.n_voxels() {
            assert_eq!(
                f.world.energy_delta()[idx as usize].to_i64(),
                expected,
                "voxel {idx} does not carry the enthalpy of both reactions"
            );
        }

        // (2) The arrangement ADR-050 overturned, run beside it. Two dispatches
        //     into one accumulator: the second overwrites the cell, so what
        //     survives is the enthalpy of the second reaction alone — and it is
        //     computed without the first reaction's demand, so even that number
        //     is not a term of the sum above.
        //
        //     A second world at the same state `N`, because the one above has
        //     already been promoted and its `ACE` is spent.
        let mut g = Fixture::build(&text, SEED).unwrap();
        g.stage_everywhere(ZED, 1 << 20);
        g.stage_everywhere(ACE, 147_456);
        g.commit(0);
        let mut split = vec![M64::ZERO; g.n_voxels() as usize];
        {
            let rx = g.react.rx();
            let mut params = g.react.params(7);
            params.n_reactions = 1;
            let (src32, src64, dst32, dst64) = g.world.amount_slices_mut();
            for r in 0..2 {
                let one = only_reaction(&rx, r);
                for idx in 0..params.n_voxels {
                    react_voxel(
                        src32,
                        src64,
                        dst32,
                        dst64,
                        &mut split,
                        &temperature,
                        &[],
                        &one,
                        &params,
                        idx,
                    );
                }
            }
        }
        assert_eq!(
            split[0].to_i64(),
            f.derived.reactions()[1].nu_energy * want_dimerisation,
            "the split arrangement no longer erases the first increment"
        );
        assert_ne!(
            split[0].to_i64(),
            expected,
            "two dispatches gave the same energy increment as one, which is what \
             ADR-050 says cannot be: the kernel overwrites its cell of the \
             accumulator rather than adding to it (ADR-045)"
        );
    }

    /// ADR-057 on the reaction side: the front buffer holds state `N+1` for every
    /// lane of both widths when the step returns.
    ///
    /// The fixture's reaction takes a 32-bit input and gives a 64-bit output, and
    /// that is the point: with one field of the two promoted, the inputs come from
    /// one state and the outputs land in another, and the element balance breaks
    /// loudly. On a single-width fixture it closes.
    ///
    /// The write buffers carry a marker beforehand, so that a copy made in the
    /// wrong direction — or a lane the step never wrote — shows up as the marker
    /// rather than as a plausible amount.
    #[test]
    fn the_process_returns_with_state_n_in_the_front_buffer_for_every_lane() {
        const MARKER: i64 = 777;
        let mut f = Fixture::build(&scenario(&[reduction()]), SEED).unwrap();
        f.stage_everywhere(ZEDACE, 1 << 20);
        f.stage_everywhere(ZED, 1 << 20);
        f.stage_everywhere(ACE, 1 << 20);
        f.commit(MARKER);

        let before = f.snapshot();
        let temperature = f.isothermal();
        f.apply(7, &temperature, &[]);
        f.promote();
        let after = f.snapshot();

        for s in 0..N_SUBSTANCES {
            for idx in 0..f.n_voxels() {
                assert_ne!(
                    after[s as usize][idx as usize], MARKER,
                    "substance {s} of voxel {idx} came back holding the marker: \
                     the lane was never written, or the buffers were exchanged \
                     the other way round (ADR-057)"
                );
            }
        }

        // The wide output grew and the narrow input shrank, so both fields took
        // part.
        assert!(after[ZED as usize][0] > before[ZED as usize][0]);
        assert!(after[ZEDACE as usize][0] < before[ZEDACE as usize][0]);

        let e_r = f.e_r_of(0);
        for idx in 0..f.n_voxels() {
            for element in 0..COMPOSITION[0].len() {
                assert_eq!(
                    element_residual(&f, element, idx, e_r, &before, &after),
                    0,
                    "conserved quantity {element} does not balance in voxel {idx}: \
                     one of the two amount fields is a tick behind the other"
                );
            }
        }
    }

    /// ADR-045: the increment lands on the fine grid, in its own cell, and the
    /// enthalpy field is not touched at all.
    ///
    /// Also that the cell is **written and not added to**, which is what lets
    /// ADR-045 do without a clearing pass — and what makes a second dispatch in
    /// one tick erase the first (ADR-050). A step that "just to be safe" folded
    /// the increment into enthalpy itself would have step `i'` credit it a second
    /// time, and the energy residual would come out doubled where nobody computes
    /// it today.
    #[test]
    fn the_energy_increment_lands_on_the_fine_grid_and_never_on_the_enthalpy_field() {
        const STALE: i64 = -999_999;
        let mut f = Fixture::build(&scenario(&[plain_amination(VMAX_FOUR_QUANTA)]), SEED).unwrap();
        f.stage_everywhere(ZED, 1 << 20);
        f.stage_everywhere(ACE, 1 << 20);
        f.commit(0);
        f.world.energy_delta_mut().fill(M64::new(STALE));

        let enthalpy_before = f.world.enthalpy().clone();
        let temperature = f.isothermal();
        f.apply(7, &temperature, &[]);

        assert_eq!(
            &enthalpy_before,
            f.world.enthalpy(),
            "the step wrote into the enthalpy field; the fold of step i' would \
             then credit the same joules a second time (ADR-045)"
        );

        let expected = f.derived.reactions()[0].nu_energy * 4;
        for idx in 0..f.n_voxels() {
            assert_eq!(
                f.world.energy_delta()[idx as usize].to_i64(),
                expected,
                "voxel {idx}: the accumulator was added to rather than written, \
                 or it was not written at all"
            );
        }

        // The same inputs a second time: the same increment, not twice it. No
        // clearing pass runs between the two, by decision (ADR-045).
        let first = f.world.energy_delta().to_vec();
        f.apply(7, &temperature, &[]);
        assert_eq!(first, f.world.energy_delta());
    }

    /// ADR-058: the tick and the run key are two neighbouring `u32`, and only a
    /// comparison of values can tell them apart.
    ///
    /// Four runs over one state. The first and the fourth are the same
    /// `(seed, tick)` and must agree bit for bit; the other two differ in one
    /// counter each and must not. Swapped, the stream stays uniform and unbiased,
    /// passes every test in `numeric/rng.rs`, and is incomparable with the
    /// reference run.
    #[test]
    fn the_tick_and_the_run_key_reach_the_draw_in_that_order() {
        // Fractional, or the draw decides nothing and all four runs agree.
        let text = scenario(&[plain_amination("3.0e-6")]);

        let run = |seed: u64, tick: u32| {
            let mut f = Fixture::build(&text, seed).unwrap();
            f.stage_everywhere(ZED, 1 << 20);
            f.stage_everywhere(ACE, 1 << 20);
            f.commit(0);
            let temperature = f.isothermal();
            f.apply(tick, &temperature, &[]);
            f.promote();
            f.snapshot()
        };

        let a0 = run(SEED, 0);
        let a1 = run(SEED, 1);
        let b0 = run(OTHER_SEED, 0);
        let a0_again = run(SEED, 0);

        assert_eq!(a0, a0_again, "one seed and one tick are not one world");
        assert_ne!(a0, a1, "the tick does not reach the draw");
        assert_ne!(a0, b0, "the run key does not reach the draw");

        // And the two counters are in their own fields, which is the half no
        // comparison of runs can see.
        let f = Fixture::build(&text, SEED).unwrap();
        for tick in [0, 1, 7, u32::MAX] {
            assert_eq!(f.react.params(tick).tick, tick);
            assert_eq!(f.react.params(tick).run_key, run_key(SEED));
        }
    }

    /// ADR-063: a catalysed reaction is refused, not passed through against
    /// column 0 of a zero buffer.
    ///
    /// The refusal is the assertion, and it is not pedantry. A zero concentration
    /// gives a rate of identically zero: the whole chemistry of the scenario
    /// stands still, and both halves of the invariant close on `0 == 0` — the
    /// accident ADR-063 names by name. An abiotic reaction has to keep loading,
    /// and its rate has to be independent of whatever the catalysis buffer
    /// happens to hold.
    #[test]
    fn a_catalysed_reaction_is_refused_until_the_catalysis_field_exists() {
        for catalyst in ["guild:M_PHOTO", "expr:7"] {
            let text = scenario(&[amination(
                VMAX_FOUR_QUANTA,
                &format!("{{ ZED = {KM_NEGLIGIBLE}, ACE = {KM_NEGLIGIBLE} }}"),
                &format!("catalyst = \"{catalyst}\""),
            )]);
            let refusal = format!("{:#}", Fixture::build(&text, SEED).unwrap_err());
            for expected in ["amination", catalyst, "ADR-050", "ADR-063"] {
                assert!(
                    refusal.contains(expected),
                    "the refusal does not name `{expected}`: {refusal}"
                );
            }
        }

        // The abiotic half: it loads, it runs, and the buffer changes nothing.
        let text = scenario(&[plain_amination(VMAX_FOUR_QUANTA)]);
        let run = |loaded: bool| {
            let mut f = Fixture::build(&text, SEED).unwrap();
            f.stage_everywhere(ZED, 1 << 20);
            f.stage_everywhere(ACE, 1 << 20);
            f.commit(0);
            let temperature = f.isothermal();
            let catalyst = if loaded {
                vec![Q::from_f64(3.0); f.n_voxels() as usize]
            } else {
                Vec::new()
            };
            f.apply(7, &temperature, &catalyst);
            f.promote();
            f.snapshot()
        };
        let empty = run(false);
        let loaded = run(true);
        assert_eq!(
            empty, loaded,
            "an abiotic reaction read the catalysis buffer: at the sentinel there \
             is no catalysis factor at all, not a factor of one (ADR-063)"
        );
    }

    /// A `requires` window and a named energy channel are refused, each with its
    /// own message.
    ///
    /// Both parse and both validate, and both reach a kernel that implements
    /// neither. A dropped `requires` is a reaction running outside the window it
    /// declares — SPEC section 5 gives the windows and no record says whether the
    /// gate is a hard cut-off or a factor. A dropped `energy_from` is energy taken
    /// from a channel nobody counted: the non-empty branch is legal only for a
    /// reaction whose enthalpy does not agree with the enthalpies of formation,
    /// and ADR-044 does not load such a reaction (ADR-059).
    #[test]
    fn a_requires_window_or_a_named_energy_channel_is_refused() {
        let km = format!("{{ ZED = {KM_NEGLIGIBLE}, ACE = {KM_NEGLIGIBLE} }}");

        let gated = scenario(&[amination(
            VMAX_FOUR_QUANTA,
            &km,
            "requires = [{ field = \"enthalpy\", min = 273.15, max = 323.15 }]",
        )]);
        let refusal = format!("{:#}", Fixture::build(&gated, SEED).unwrap_err());
        assert!(
            refusal.contains("requires") && refusal.contains("amination"),
            "{refusal}"
        );

        // Past the validator, which refuses this one first and for its own reason
        // (ADR-059). The process has to refuse it in its own words as well.
        let channelled = scenario(&[amination(
            VMAX_FOUR_QUANTA,
            &km,
            "energy_from = \"SOLAR_IN\"",
        )]);
        let refusal = format!(
            "{:#}",
            Fixture::build_unvalidated(&channelled, SEED).unwrap_err()
        );
        assert!(
            refusal.contains("energy_from") && refusal.contains("SOLAR_IN"),
            "{refusal}"
        );
    }

    /// ADR-062: the Q10 factor reads the temperature of the covering **enthalpy**
    /// cell, and `lod`, `cnx`, `cny` come from that grid and no other.
    ///
    /// A world carries two coarse grids and they are different — `lod = 2` for
    /// enthalpy, `lod = 1` for velocity. Extents taken from the velocity grid give
    /// the reaction a plausible, neighbouring, wrong cell; temperature is class
    /// `Q` and enters no invariant, so nothing else in the project can see it.
    #[test]
    fn the_temperature_is_read_from_the_covering_enthalpy_cell() {
        let mut f = Fixture::build(&scenario(&[plain_amination(VMAX_FOUR_QUANTA)]), SEED).unwrap();

        // The two coarse grids disagree, or `cnx` from the wrong one is the right
        // number by accident.
        assert_ne!(
            f.world.enthalpy_grid().nx(),
            f.world.velocity_grid().nx(),
            "the fixture no longer has two different coarse grids"
        );
        let params = f.react.params(0);
        assert_eq!(params.lod, ENTHALPY_LOD);
        assert_eq!(params.cnx, f.world.enthalpy_grid().nx());
        assert_eq!(params.cny, f.world.enthalpy_grid().ny());

        // Ten kelvin above `t_vmax` in half the cells: the Q10 factor is exactly
        // two there and exactly one elsewhere, so the extent doubles.
        let mut temperature = f.isothermal();
        for (cell, t) in temperature.iter_mut().enumerate() {
            if cell % 2 == 1 {
                *t = Q::from_f64(T_WARM);
            }
        }
        assert!(temperature.len() > 1, "the coarse grid holds one cell");

        f.stage_everywhere(ZED, 1 << 20);
        f.stage_everywhere(ACE, 1 << 20);
        f.commit(0);
        let before = f.snapshot();
        f.apply(7, &temperature, &[]);
        f.promote();
        let after = f.snapshot();

        for idx in 0..f.n_voxels() {
            let cell = f.world.enthalpy_cell_of(idx);
            let expected = if cell % 2 == 1 { 8 } else { 4 };
            assert_eq!(
                extent_at(&f, 0, ZEDACE, idx, &before, &after),
                expected,
                "voxel {idx} did not run at the temperature of enthalpy cell {cell}"
            );
        }
    }
}
