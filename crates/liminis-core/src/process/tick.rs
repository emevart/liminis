//! The tick: the order of SPEC section 8, the Lie-Trotter splitting between the
//! operators, and the phase where both residuals are closed.
//!
//! Everything else under `process/` folds one operator. This file decides *when*
//! each of them runs, and that decision is world semantics rather than
//! orchestration: ADR-036 makes the order of the list of SPEC section 8 the order
//! of a first-order splitting, so swapping two lines changes the result and has
//! to move `WORLD_FORMAT_VERSION` (ADR-020) even though no config and no kernel
//! changed.
//!
//! # The order, and the only thing that can see it
//!
//! [`STEP_ORDER`] is the list, and [`Step::letter`] is its transcription of the
//! letters SPEC section 8 prints:
//!
//! ```text
//! a   light            the field, out of state N of the amounts
//! b   velocity         the prescribed field, out of last tick's enthalpy
//! c   advection        matter *and enthalpy* carried by the field b built
//! d   diffusion        explicit, n substeps per field, enthalpy among them
//! e   pressure         matter moved by its own mobility
//! f   settling         matter moved by its own velocity
//! h   reactions        the whole chemistry in one call (ADR-050)
//! i'  energy fold      128^3 -> 32^3: reactions and absorbed light (ADR-045, ADR-049)
//! j   channels         external channels and events
//! ```
//!
//! Line `g` — phase transitions — has no step here and does have a roster entry
//! (`process::phase`); the fold `i'` has a step here and no roster entry
//! (ADR-065 keeps it out on purpose, it is part of the energy path of the
//! reactions). The two lists are the same length by coincidence.
//!
//! **Nothing but `the_step_order_is_the_one_spec_section_8_prints` can see a
//! permutation of this array.** Both residuals close under any order — every
//! operator conserves separately — every "alone" test stays green, every kernel
//! test stays green, and there is no golden run in the repository to disagree
//! with. Swapping pressure ahead of diffusion is a different world and a green
//! suite. So the order is a named constant with a test that spells the letters
//! out, and not a sequence of calls in the body of [`Tick::advance`].
//!
//! **The light stands before the fold.** ADR-049 makes absorption a derived
//! quantity that the fold computes from the stored light field, so the field has
//! to belong to the *current* tick. The other order lags it by one, and the
//! energy residual stays at exactly zero while it does — the fold credits
//! `SOLAR_IN` from the field it read, so the counter and the enthalpy move
//! together. The mechanism built to catch "energy appeared out of nowhere" is
//! precisely the one that cannot catch this (ADR-049 says so itself).
//!
//! **The enthalpy is transported by `c` and `d` like everything else.** SPEC
//! section 8 names it on both lines — "адвекция … энтальпия", "диффузия …
//! энтальпия на 32³ — шесть" — and says outright that it is an ordinary
//! diffusive field, standing in those two steps beside the substances and
//! updated every tick. So each of the two dispatched steps runs over **three**
//! fields, not two: the narrow amounts, the wide amounts, and the enthalpy.
//!
//! It costs a second pair of folded operators rather than a third lane, because
//! the field is not on the amounts' grid: ADR-062 puts it on `32³` while the
//! amounts are on `128³`, and everything a phase is folded from — the extents in
//! `AdvectParams`, `alpha`, the substep count, the length of a Courant buffer —
//! is a function of that grid. Its own `alpha` and its own `n` are the ones
//! `config/derive.rs` already derived for the record (`Derived::enthalpy_field`),
//! refolded here from the same two inputs and checked against them, so that a
//! `dt` that disagreed with the derivation's is a refusal rather than a field
//! stepping at a rate nobody chose.
//!
//! Omitting it is the failure this file is otherwise built to prevent, and it is
//! invisible from every side: transport conserves, so both residuals stay at
//! exactly zero for ever, and the temperature ADR-062 derives from the field goes
//! on looking entirely plausible while heat neither conducts nor is carried by
//! the flow — in a system where thermal diffusion is the *fastest* transport
//! there is, two orders above the molecular one (ADR-028).
//!
//! **No shared Courant budget.** ADR-036: under splitting each operator is
//! applied over a full step and gets by on its own stability condition. This
//! file therefore never adds the budgets of steps `b`, `e` and `f` together, and
//! `config/validate.rs` keeps them as a sequence of `SpeedBound`s for the same
//! reason.
//!
//! # Which steps run today, and why the rest refuse
//!
//! Two of the nine are dispatched: advection and diffusion, each over all three
//! transported fields of S0 (see above). The other seven are refused by
//! [`Tick::new`] when a roster enables them, each naming the number or the
//! operator that is missing, rather than being skipped quietly:
//!
//! | step | what is missing |
//! |---|---|
//! | `a` light | a lit scenario is refused by two locks at once: energy has no sink, and the width of a channel counter is A-20 (A-19 in ADR-075 and ADR-076, drafted while that number was free) |
//! | `b` velocity | the four keys of ADR-069 reach no folder, and the Courant fold onto the enthalpy faces is `TODO(courant-fold)`; the buffers and the denominator no longer block it |
//! | `e` pressure | `theta_max` is declared nowhere (`TODO(theta-max)`) |
//! | `f` settling | `g` and `rho_medium` are named by no document (ADR-067, ADR-069) |
//! | `h` reactions | both arches of the invariant under chemistry, and step `i'` with it |
//! | `i'` fold | step `h`, which it is dispatched with or not at all |
//! | `j` channels | there are no events, and no emissivity or vent composition |
//!
//! A refusal rather than a skip, because a skipped step is a different world that
//! looks like the same one. The one thing a refusal costs is that the roster's
//! defaults have to be `false` wherever the operator is blocked — which is the
//! reading ADR-065 already argued for when it rejected a blanket `enabled = true`
//! ("it enables processes that do not exist yet").
//!
//! # The Courant buffers, which nothing fills
//!
//! Step `c` is dispatched and its Courant numbers are zeros, and that is a hole
//! rather than a property. [`Advect::fold_courant`](super::Advect::fold_courant)
//! is the door — and the only place in the project that checks both inequalities
//! of SPEC section 4.2 against the velocity a face actually has rather than
//! against the declared `u_conv_max` — and **nothing in this crate calls it**.
//! [`Scratch`] allocates the two buffers and leaves them at `Q::ZERO`, so every
//! application of advection today is an application of a zero flux: a dispatched
//! step that moves nothing, which is a different thing from a skipped step
//! (ADR-057 wants the parity of a lane to come from the applications the phase
//! *ran*) and an indistinguishable thing by its result.
//!
//! TODO(courant-fold): the two halves of this are no longer the same size, and
//! saying so is the point. The **fine** half is written: `kernels/curl.rs` has
//! `face_courant_voxel`, it reads the `64³` velocity onto the `128³` faces in the
//! layout `kernels/advect.rs` expects, and `VelocityField::apply` dispatches it as
//! its last stage — so `TODO(velocity-interpolation)` in `process/advect.rs` is
//! stale for that half and true only for this one. The **coarse** half is not:
//! `face_courant_voxel` debug-asserts that its target grid is a *refinement* of
//! the velocity grid (`nx == vnx << ratio_log2`), the enthalpy grid is coarser
//! than the velocity grid rather than finer (`lod = 2` against `lod = 1`), and
//! reading a `64³` field onto `32³` faces is an averaging that no record writes.
//! ADR-045 gives a precedent for fine-to-coarse folding and step `c` over the
//! enthalpy wants exactly that direction — but for a *flux* and not for a
//! quantity, and inventing it here would put a second, silent addressing scheme
//! underneath the only advective speed the Courant condition has.
//!
//! So `Scratch::enthalpy_courant` stays zero even once step `b` runs, and that is
//! the quiet version this refusal exists to prevent: matter advected by a field
//! and heat standing still, with both residuals closing exactly — transport
//! conserves either way — every "alone" test green, the temperature field
//! plausible, and the symptom a plume that carries substance without the heat that
//! raised it. The honest form is a refusal and not a half-dispatch.
//!
//! # Where the temperature is recomputed, and why it is a decision
//!
//! `process::Temperature` is a step of this tick and not a process of the roster
//! (ADR-065 closed the roster at nine; `process/mod.rs` says why), so no
//! `[[process]]` record can move it and this file is where its place in the order
//! is written down. **It runs immediately before step `h`.**
//!
//! No document named the step and ADR-079 does. `T` has two consumers and they
//! stand on opposite
//! sides of the transport: step `b` reads the heat capacity `C_cell` — the same
//! sum, before advection and diffusion have moved anything — and step `h` reads
//! `T` after all four transport steps. One recomputation a tick serves exactly
//! one of them correctly, and ADR-044 allows only one ("the composition changes
//! once per tick, so recomputing once per tick *is* recomputing", ADR-062).
//!
//! Recomputing before `h` gives the chemistry the composition it is actually
//! running on, and leaves step `b` with the previous tick's capacity — which is
//! what ADR-069 asks for in as many words, "out of last tick's enthalpy". The
//! other placement, at the top of the tick, would serve `b` and hand `h` a
//! temperature four transport steps stale. Neither is visible from any test that
//! is not this sentence: `T` is class `Q`, it enters no invariant, and both
//! halves of the ledger close exactly under either.
//!
//! That makes the placement world semantics of the same standing as the order of
//! the operators (ADR-036), and it moves `WORLD_FORMAT_VERSION` with them
//! (ADR-020). It is not in [`STEP_ORDER`] and ADR-079 says it stays out: that
//! array is the nine letters SPEC section 8 prints and is guarded by a test that
//! spells them out, and this operator is a step of the tick rather than a letter
//! of the spec. Nothing dispatches it today, because step `h` does not dispatch.
//!
//! # The stale accumulator, said out loud
//!
//! ADR-045 removed the clearing pass over `energy_delta`: the reaction kernel
//! **overwrites** its cell rather than adding to it. That makes a skipped step
//! `h` dangerous in a way no invariant can see — the fold of step `i'` would
//! credit the previous tick's increment a second time, and a stale number is
//! indistinguishable from a fresh one. What this tick does about it: **steps `h`
//! and `i'` are dispatched together or not at all.** Today neither is dispatched,
//! `energy_delta` is never written and never read, and the day one of them is
//! wired the other has to arrive in the same edit. The rule is written here
//! because it is the host's and nothing in either kernel can check it.
//!
//! # Phase 5 LEDGER
//!
//! [`Tick::advance`] closes both residuals after APPLY and before OBSERVE, in the
//! debug build, which is where ADR-059 puts them. Three things are deliberately
//! not folded into one `cfg`:
//!
//! - `Ledger::begin_tick` runs in **both** profiles. The counters tick in both
//!   (`ledger/mod.rs` says why), and a release build whose snapshot never moved
//!   would make `matter_this_tick` return the whole run's total instead of the
//!   tick's increment — and the metric stream of ADR-037 would print it as the
//!   residual;
//! - the reduction and the assertion run in the debug build. Two full passes over
//!   every lane once a tick is not a release cost anybody asked for;
//! - the residual is checked **per tick**. A leak on tick 7 compensated on tick
//!   900 leaves the difference of the two ends at zero, and a test built out of
//!   two reductions around a run is a much weaker statement wearing the same name.
//!
//! And the prohibition ADR-059 states: after the LEDGER phase no channel is
//! written. Everything that credits credits inside `advance`, above the phase.

use anyhow::{Context, Result, bail};

use super::{
    AdvectPhase, DiffusePhase, ProcessId, ROSTER_LEN, RosterEntry, advect, diffuse, light,
    pressure, react, settle, velocity,
};
use crate::config::Derived;
use crate::ledger::{DomainSums, Ledger};
use crate::numeric::Q;
use crate::world::{LaneRef, Width, World};

/// One factor of the Lie-Trotter splitting, in the order of SPEC section 8.
///
/// Not the same list as [`ProcessId`]: see the module header for the two lines
/// where they differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    /// `a`
    Light,
    /// `b`
    Velocity,
    /// `c`
    Advection,
    /// `d`
    Diffusion,
    /// `e`
    Pressure,
    /// `f`
    Settling,
    /// `h`
    Reactions,
    /// `i'`
    EnergyFold,
    /// `j`
    ExternalChannels,
}

/// The splitting order of the world, written once.
///
/// A named constant and not a sequence of statements, because a permutation of a
/// sequence of statements is invisible to every test in this repository (see the
/// module header). `the_step_order_is_the_one_spec_section_8_prints` compares
/// this array letter by letter with what SPEC section 8 prints, and it is the
/// only thing that does.
pub const STEP_ORDER: [Step; 9] = [
    Step::Light,
    Step::Velocity,
    Step::Advection,
    Step::Diffusion,
    Step::Pressure,
    Step::Settling,
    Step::Reactions,
    Step::EnergyFold,
    Step::ExternalChannels,
];

impl Step {
    /// The letter SPEC section 8 prints for this step.
    ///
    /// The spelling matters as much as the order: ADR-049 calls `i'` the fold of
    /// *all* the energy that arrived during the tick and not "the fold of the
    /// reaction energy", and the name of a step is part of the world semantics
    /// exactly as far as its content is.
    #[must_use]
    pub const fn letter(self) -> &'static str {
        match self {
            Step::Light => "a",
            Step::Velocity => "b",
            Step::Advection => "c",
            Step::Diffusion => "d",
            Step::Pressure => "e",
            Step::Settling => "f",
            Step::Reactions => "h",
            Step::EnergyFold => "i'",
            Step::ExternalChannels => "j",
        }
    }

    /// The roster entry that switches this step on, or `None`.
    ///
    /// `None` for the fold: ADR-065 keeps `i'` out of the roster because it is
    /// part of the energy path of the reactions (ADR-045) rather than a process,
    /// and a scenario cannot switch it off separately. That is the same statement
    /// as the rule in the module header — `h` and `i'` are dispatched together or
    /// not at all — seen from the config side.
    #[must_use]
    pub const fn process(self) -> Option<ProcessId> {
        match self {
            Step::Light => Some(ProcessId::Light),
            Step::Velocity => Some(ProcessId::VelocityField),
            Step::Advection => Some(ProcessId::Advection),
            Step::Diffusion => Some(ProcessId::Diffusion),
            Step::Pressure => Some(ProcessId::Pressure),
            Step::Settling => Some(ProcessId::Settling),
            Step::Reactions => Some(ProcessId::Reactions),
            Step::EnergyFold => None,
            Step::ExternalChannels => Some(ProcessId::ExternalChannels),
        }
    }
}

/// The row of a roster entry, for the two flat arrays a tick indexes by process.
///
/// `ProcessId::ALL` is in declaration order, so the position of a variant in it
/// *is* its discriminant; taking the row by search rather than by `as usize`
/// keeps that from being a second thing to know.
fn row(id: ProcessId) -> usize {
    ProcessId::ALL
        .iter()
        .position(|&p| p == id)
        .expect("ProcessId::ALL holds every variant")
}

/// One run's folded operators and its roster.
///
/// Built once, before the first tick, out of a world and a derivation. Holding
/// the folded processes rather than rebuilding them per tick is not only a cost
/// question: `Diffuse::new` refuses a substep count over `N_MAX` and
/// `Advect::new` refuses an exchange face, and a refusal that arrives on tick
/// 4 000 is a crash rather than a load error.
#[derive(Clone, Debug)]
pub struct Tick {
    /// Step `c` over the narrow field, absent when the scenario has no narrow
    /// substance or advection is off.
    advect_32: Option<AdvectPhase>,
    /// Step `c` over the wide field.
    advect_64: Option<AdvectPhase>,
    /// Step `c` over the enthalpy field, on its own coarse grid.
    advect_h: Option<AdvectPhase>,
    /// Step `d` over the narrow field.
    diffuse_32: Option<DiffusePhase>,
    /// Step `d` over the wide field.
    diffuse_64: Option<DiffusePhase>,
    /// Step `d` over the enthalpy field: one lane, its own `alpha`, its own
    /// substep count (ADR-062).
    diffuse_h: Option<DiffusePhase>,
    /// The roster, flattened by [`row`]: whether each process runs at all.
    enabled: [bool; ROSTER_LEN],
    /// And how often. `1` is every tick.
    every_n: [u32; ROSTER_LEN],
    /// How many substances the domain sums account for. Taken from the registry
    /// at construction so that [`Tick::domain_sums`] cannot be handed accumulators
    /// of another world's width — `Ledger::residual_matter` catches the narrow
    /// case loudly and the wide case not at all.
    n_substances: u32,
    /// The substance each lane of the narrow field carries, in **lane** order.
    ///
    /// Built once through `World::lane_of`, which is the one door from a
    /// substance to a buffer address (ADR-056), and handed to both transport
    /// phases so that what crosses the lid is credited to the right substance. A
    /// substance-indexed table here would be right for lane 0 and off by one
    /// from there on, and the ledger would close against somebody else's flow.
    substance_of_lane_32: Vec<u32>,
    /// The same for the wide field.
    substance_of_lane_64: Vec<u32>,
    /// Voxels of the fine grid, for the shape of the Courant buffer.
    n_voxels: u32,
    /// Cells of the enthalpy grid, for the shape of the other one.
    n_enthalpy_cells: u32,
}

/// The buffers a tick needs and the world does not own.
///
/// Separate from [`World`] because they hold nothing between ticks: every one of
/// them is written before it is read inside a single [`Tick::advance`]. Separate
/// from [`Tick`] because `Tick` is shared and these are `&mut`.
#[derive(Clone, Debug)]
pub struct Scratch {
    /// Three Courant numbers per **fine** voxel, in the layout
    /// `kernels/advect.rs` reads.
    ///
    /// Owned here rather than in `World`, and that is not settled:
    /// `TODO(courant-buffer-owner)` in `process/advect.rs` prices it at 25.2 MB
    /// for a 128-cubed run and no record puts it in the per-voxel budget. Here
    /// it is at least visible — a scratch buffer of one tick, allocated once.
    face_courant: Vec<Q>,
    /// Three more per cell of the **enthalpy** grid, for the same step over the
    /// field of ADR-062.
    ///
    /// A second buffer and not a second reading of the first: the two grids
    /// differ by `lod`, so a fine face has no cell here at all and the layout
    /// `axis * n_cells + idx` addresses a different thing in each. At `lod = 2`
    /// it is a sixty-fourth of the buffer above, which is why it is allocated
    /// rather than argued over.
    enthalpy_courant: Vec<Q>,
    /// The left-hand side of the invariant before the tick.
    before: DomainSums,
    /// And after it. Two accumulators reused across ticks rather than allocated
    /// twice a tick (`ledger/mod.rs`).
    after: DomainSums,
}

impl Scratch {
    /// Allocate everything one tick needs.
    ///
    /// # Errors
    ///
    /// Returns an error if the domain sums cannot be built — a world with no
    /// substances, which closes against anything (ADR-003).
    pub fn new(world: &World, tick: &Tick) -> Result<Scratch> {
        let n_voxels = world.grid().n_voxels() as usize;
        Ok(Scratch {
            // Three per voxel whether or not advection runs. A buffer sized by
            // the roster would change length when a process is switched off, and
            // `a_disabled_process_leaves_every_buffer_bit_for_bit` compares
            // buffers.
            // `lane_len` per axis and not `n_voxels`: the ghost cell owns the
            // Courant number of the face of the domain (ADR-059).
            face_courant: vec![Q::ZERO; 3 * (n_voxels + 1)],
            enthalpy_courant: vec![Q::ZERO; 3 * (world.enthalpy_grid().n_voxels() as usize + 1)],
            before: DomainSums::new(tick.n_substances).context("the domain sums before a tick")?,
            after: DomainSums::new(tick.n_substances).context("the domain sums after a tick")?,
        })
    }

    /// The left-hand side as it stood at the top of the last tick.
    ///
    /// Filled by the LEDGER phase of [`Tick::advance`], which runs in the debug
    /// build only (ADR-059) — so in a release build these are whatever they were
    /// initialised to. A caller that wants the sums in both profiles calls
    /// [`Tick::domain_sums`] itself, which is what the acceptance tests do.
    #[must_use]
    pub fn before(&self) -> &DomainSums {
        &self.before
    }

    /// The left-hand side after the last tick. See [`Scratch::before`].
    #[must_use]
    pub fn after(&self) -> &DomainSums {
        &self.after
    }
}

impl Tick {
    /// Fold every enabled operator of the roster, or refuse and say what is
    /// missing.
    ///
    /// `dt` and `dx` arrive as arguments rather than being read out of a config,
    /// on the precedent of every other constructor under `process/`: a folded
    /// parameter is the host's to compute, and a process that read the scenario
    /// would be a second place where a coefficient is formed.
    ///
    /// # Errors
    ///
    /// Returns an error if a folded operator refuses — a substep count over
    /// `N_MAX`, an exchange face, a Courant number over one — and, naming what is
    /// undecided, if the roster enables one of the seven processes this tick
    /// cannot dispatch. See the table in the module header.
    pub fn new(
        world: &World,
        derived: &Derived,
        roster: &[RosterEntry; ROSTER_LEN],
        dt: f64,
        dx: f64,
    ) -> Result<Tick> {
        let registry = world.registry();
        if derived.substances().len() != registry.n_substances() as usize {
            bail!(
                "the derivation carries {} substances and the world's registry {}: \
                 the two came from different scenarios, and a diffusivity would be \
                 folded for the wrong lane (ADR-056)",
                derived.substances().len(),
                registry.n_substances()
            );
        }

        let mut enabled = [false; ROSTER_LEN];
        let mut every_n = [super::DEFAULT_EVERY_N_TICKS; ROSTER_LEN];
        let mut written = [false; ROSTER_LEN];
        for entry in roster {
            let row = row(entry.id);
            // Nine entries are nine *distinct* processes, and the array's length
            // does not say so. A repeated id leaves some other process at the
            // `false` this array was initialised with, so the roster would come
            // out one process short with nothing missing from it — the failure
            // `config::materialise` refuses on the loader's side, on a path that
            // does not reach here: nothing in the crate turns a `Config` into a
            // `[RosterEntry; ROSTER_LEN]`.
            if written[row] {
                bail!(
                    "the roster names process `{}` twice. Nine entries are nine \
                     distinct processes; a repeat silently leaves whichever \
                     process is missing disabled, and a disabled process is a \
                     different world (ADR-065)",
                    entry.id.id()
                );
            }
            written[row] = true;
            enabled[row] = entry.enabled;
            every_n[row] = entry.every_n_ticks;
            if entry.every_n_ticks == 0 {
                bail!(
                    "process `{}` declares every_n_ticks = 0, which is not \
                     'every tick' but 'never': `tick % 0` has no answer",
                    entry.id.id()
                );
            }
            // ADR-030, on the door that executes rather than on the one that
            // parses. `config/derive.rs` refuses this over the config text, and
            // that check protects nobody here for the same reason as above. It
            // is diffusion and not every transport process, because that is the
            // extent of the ban: skipping ticks multiplies the effective `dt` of
            // the *substepped* operator, which is the one thing the substep count
            // exists to bound. And nothing sees it afterwards — diffusion
            // conserves at any `alpha`, so both residuals close over an unstable
            // field for as long as it takes to overflow.
            if entry.id == ProcessId::Diffusion && entry.every_n_ticks > 1 {
                bail!(
                    "process `{}` declares every_n_ticks = {}, and it moves \
                     diffusive fields. Skipping ticks multiplies the effective \
                     dt, which is what the substep count exists to prevent: a \
                     rarely updated diffusive field is unstable by definition \
                     rather than by oversight (ADR-030)",
                    entry.id.id(),
                    entry.every_n_ticks
                );
            }
        }

        for id in ProcessId::ALL {
            if enabled[row(id)] {
                refuse_if_blocked(id)?;
            }
        }

        let grid = world.grid();
        let lanes_32 = registry.lanes(Width::Bits32);
        let lanes_64 = registry.lanes(Width::Bits64);

        // The third transported field of S0, and the one that is on neither
        // amount grid (SPEC section 8 `c` and `d`, ADR-062). Its coefficient and
        // its coarse step are the loader's, refolded here rather than recomputed:
        // `dx * 2^lod` and `thermal_diffusivity` are the two inputs
        // `config/derive.rs` derived the record's `n` and `alpha` from.
        let enthalpy = derived.enthalpy_field();
        let enthalpy_grid = world.enthalpy_grid();
        let refolded = diffuse::substeps_and_alpha(enthalpy.diffusivity, dt, enthalpy.coarse_dx)?;
        if refolded != (enthalpy.substeps, enthalpy.alpha) {
            // Reachable on exactly one input: a `dt` here that is not the `dt`
            // the scenario was derived under. The field would then step at a rate
            // nobody chose while every test of transport stayed green, since
            // diffusion conserves at any `alpha`.
            bail!(
                "the enthalpy field was derived at {} substeps and alpha = {}, \
                 and folds here at {} and {}: dt = {dt} s is not the dt the \
                 scenario was derived under (ADR-030, ADR-062)",
                enthalpy.substeps,
                enthalpy.alpha,
                refolded.0,
                refolded.1
            );
        }

        let (advect_32, advect_64, advect_h) = if enabled[row(ProcessId::Advection)] {
            (
                fold_per_width(lanes_32, |lanes| AdvectPhase::new_32(grid, lanes, dt, dx))?,
                fold_per_width(lanes_64, |lanes| AdvectPhase::new_64(grid, lanes, dt, dx))?,
                // One lane, the coarse grid and the coarse step. `dx` here is not
                // the `dx` above and the compiler cannot tell them apart: the
                // Courant number of a face is `u*dt/dx`, so the fine step would
                // make every coarse face four times as fast as it is.
                Some(AdvectPhase::new_64(
                    enthalpy_grid,
                    1,
                    dt,
                    enthalpy.coarse_dx,
                )?),
            )
        } else {
            (None, None, None)
        };

        // One exchange velocity per reservoir, and zero when the scenario
        // declares none — which the validator ties to "no face is `exchange`"
        // in both directions, so a grid that vents always has a number here.
        let k_ex = derived.reservoir().map_or(0.0, |r| r.k_ex);

        let (diffuse_32, diffuse_64, diffuse_h) = if enabled[row(ProcessId::Diffusion)] {
            // Per lane and never per substance: on the registry the project
            // carries, water is first and takes lane 0, so a substance-indexed
            // array is right at `s == 0` and off by one from there on (ADR-056).
            let d32 = diffusivity_by_lane(world, derived, Width::Bits32);
            let d64 = diffusivity_by_lane(world, derived, Width::Bits64);
            (
                fold_per_width(lanes_32, |lanes| {
                    DiffusePhase::new_32(grid, lanes, &d32, dt, dx, k_ex)
                })?,
                fold_per_width(lanes_64, |lanes| {
                    DiffusePhase::new_64(grid, lanes, &d64, dt, dx, k_ex)
                })?,
                Some(DiffusePhase::new_64(
                    enthalpy_grid,
                    1,
                    &[enthalpy.diffusivity],
                    dt,
                    enthalpy.coarse_dx,
                    k_ex,
                )?),
            )
        } else {
            (None, None, None)
        };

        Ok(Tick {
            advect_32,
            advect_64,
            advect_h,
            diffuse_32,
            diffuse_64,
            diffuse_h,
            enabled,
            every_n,
            substance_of_lane_32: substance_by_lane(world, Width::Bits32),
            substance_of_lane_64: substance_by_lane(world, Width::Bits64),
            n_substances: registry.n_substances(),
            n_voxels: grid.n_voxels(),
            n_enthalpy_cells: enthalpy_grid.n_voxels(),
        })
    }

    /// Whether this run dispatches `id` at all.
    #[inline]
    #[must_use]
    pub fn enabled(&self, id: ProcessId) -> bool {
        self.enabled[row(id)]
    }

    /// How many ticks between two applications of `id`.
    #[inline]
    #[must_use]
    pub fn every_n_ticks(&self, id: ProcessId) -> u32 {
        self.every_n[row(id)]
    }

    /// The splitting order this tick walks. Always [`STEP_ORDER`]; the accessor
    /// exists so that a caller can print the order it *ran* rather than the order
    /// it believes was run.
    #[inline]
    #[must_use]
    pub fn steps(&self) -> &'static [Step; 9] {
        &STEP_ORDER
    }

    /// Whether step `step` runs on tick number `tick`.
    ///
    /// The fold of `i'` has no roster entry, so it runs exactly when the
    /// reactions do — the rule of the module header, in code.
    fn runs(&self, step: Step, tick: u32) -> bool {
        let id = match step.process() {
            Some(id) => id,
            None => ProcessId::Reactions,
        };
        let row = row(id);
        self.enabled[row] && tick.is_multiple_of(self.every_n[row])
    }

    /// One tick: phases 0 to 5 of SPEC section 8.
    ///
    /// The body walks [`STEP_ORDER`] rather than calling the operators in
    /// sequence, so that the order lives in the constant and the constant is
    /// what a test can read.
    ///
    /// # Panics
    ///
    /// In a debug build, if either residual comes out non-zero — that is phase 5
    /// LEDGER (ADR-059), and the message names the substance, both sides and the
    /// per-channel breakdown. Also if a buffer of `world` or `scratch` does not
    /// have the shape this tick was folded for.
    pub fn advance(
        &self,
        world: &mut World,
        ledger: &mut Ledger,
        scratch: &mut Scratch,
        tick: u32,
        run_key: u32,
    ) {
        assert_eq!(
            world.grid().n_voxels(),
            self.n_voxels,
            "the world does not have the shape this tick was folded for"
        );
        assert_eq!(
            world.enthalpy_grid().n_voxels(),
            self.n_enthalpy_cells,
            "the enthalpy grid does not have the shape this tick was folded for"
        );
        assert_eq!(
            scratch.face_courant.len(),
            3 * (self.n_voxels as usize + 1),
            "the Courant buffer does not have the shape this tick was folded for"
        );
        assert_eq!(
            scratch.enthalpy_courant.len(),
            3 * (self.n_enthalpy_cells as usize + 1),
            "the enthalpy Courant buffer does not have the shape this tick was \
             folded for"
        );

        // Outside any `cfg`, and that is the point: the counters tick in both
        // profiles, so the snapshot the tick's increment is measured from has to
        // move in both. Folded into the `cfg` below, a release build would report
        // the whole run's total as this tick's flow (`ledger/mod.rs`).
        ledger.begin_tick();

        #[cfg(debug_assertions)]
        self.domain_sums(world, &mut scratch.before);

        // TODO(stochastic-dispatch): `run_key` is carried through the tick and
        // consumed by nothing. Its two consumers are `React::params(tick)`
        // (ADR-058) and `NoiseParams` in the velocity field (ADR-069), and
        // neither step is dispatchable — see the table in the module header. It
        // stays in the signature rather than being added later because a tick
        // that forwards `tick` and forgets `run_key` gives every seed the same
        // world, `same_seed_and_config_give_byte_identical_state` stays green,
        // and only `different_seed_gives_different_state` goes red.
        //
        // The velocity half is a consumer only on the scenarios that stir, and
        // that is worth writing down beside the dispatch rather than discovering
        // from a green test. `VelocityField::apply` branches on `stirs` on the
        // host (ADR-069), and `run_key` enters `NoiseParams` and nothing else: at
        // `stir_fraction = 0` a step `b` that dispatched perfectly would still
        // leave this line true, because the convective half of `A` is a function
        // of the enthalpy and the heat capacity alone. So the day the criterion
        // above stops being ignored is the day a *stirring* fixture exists, and
        // not the day step `b` builds.
        let _ = run_key;

        for &step in &STEP_ORDER {
            if !self.runs(step, tick) {
                continue;
            }
            match step {
                // The seven steps `Tick::new` refuses to build. Unreachable
                // rather than silently skipped: `runs` can only be true for them
                // if `refuse_if_blocked` let them through.
                Step::Light
                | Step::Velocity
                | Step::Pressure
                | Step::Settling
                | Step::Reactions
                | Step::EnergyFold
                | Step::ExternalChannels => unreachable!(
                    "step {} was enabled and Tick::new did not refuse it",
                    step.letter()
                ),
                Step::Advection => {
                    // Both Courant buffers are zeros, and nothing folds them:
                    // `Advect::fold_courant` is called from nowhere in the crate
                    // because step `b` cannot be dispatched and no stencil maps a
                    // velocity onto either grid's faces. See "The Courant
                    // buffers, which nothing fills" in the module header — this
                    // is therefore one application of a zero flux and not a
                    // skipped step, and the parity of a lane still has to come
                    // from the applications the phase *ran* (ADR-057).
                    let courant = &scratch.face_courant;
                    if let (Some(phase), Some(field)) = (&self.advect_32, world.amounts_32_mut()) {
                        phase.apply_32(field, courant, &self.substance_of_lane_32, ledger);
                    }
                    if let (Some(phase), Some(field)) = (&self.advect_64, world.amounts_64_mut()) {
                        phase.apply_64(field, courant, &self.substance_of_lane_64, ledger);
                    }
                    // The enthalpy, which SPEC section 8 names on this line
                    // beside the substances. Its own grid, so its own buffer,
                    // and its own half of the ledger (ADR-028, ADR-067).
                    if let Some(phase) = &self.advect_h {
                        phase.apply_enthalpy(
                            world.enthalpy_mut(),
                            &scratch.enthalpy_courant,
                            ledger,
                        );
                    }
                }
                Step::Diffusion => {
                    if let (Some(phase), Some(field)) = (&self.diffuse_32, world.amounts_32_mut()) {
                        phase.apply_32(field, &self.substance_of_lane_32, ledger);
                    }
                    if let (Some(phase), Some(field)) = (&self.diffuse_64, world.amounts_64_mut()) {
                        phase.apply_64(field, &self.substance_of_lane_64, ledger);
                    }
                    // "энтальпия на 32³ — шесть": the six substeps of ADR-062 are
                    // the record's own, derived from `thermal_diffusivity` at the
                    // coarse step and never from a substance's `D`. Thermal
                    // diffusion is the fastest transport in the system (ADR-028),
                    // and this is the one line that performs it.
                    if let Some(phase) = &self.diffuse_h {
                        phase.apply_enthalpy(world.enthalpy_mut(), ledger);
                    }
                }
            }
        }

        // Phase 5 LEDGER: after APPLY, before OBSERVE, in the debug build
        // (ADR-059). Below every credit and above nothing — a channel written
        // after this point would make the *next* tick's residual wrong and name
        // no culprit.
        #[cfg(debug_assertions)]
        {
            self.domain_sums(world, &mut scratch.after);
            ledger.assert_closed(&scratch.before, &scratch.after);
        }
    }

    /// The left-hand side of the invariant: everything the domain holds.
    ///
    /// **The one place it is gathered.** Two failures live here and both are
    /// silent, which is why they are one function rather than a habit:
    ///
    /// **A lane where a substance was meant.** `DomainSums` is indexed by
    /// substance and `Field::lane` by lane, and the two coincide only for the
    /// first substance of a width class (ADR-056). The same wrong mapping applied
    /// before and after gives a residual of exactly zero on every tick for ever,
    /// while the channel credits land on somebody else's substance — and on a
    /// closed domain every counter is zero, so that does not show either. Hence
    /// `World::lane_of` and never a hand-built index.
    ///
    /// **A left side truncated to `amount[]`.** The guild fields and the cell
    /// table are part of it (ADR-059), and in S0 there are none of either. The
    /// doors below are therefore called with empty constants — and the call is
    /// **not** the protection it looks like. A door fed `&[]` adds zero and is
    /// byte for byte a door not called at all, so the day S1 allocates the guild
    /// buffers the sum goes on closing and nothing announces anything. What the
    /// calls buy is one grep away from `TODO(s1-guilds)` below, which names the
    /// buffers that have to be threaded through here; what they do not buy is a
    /// failing test, and reading them as one is the trap.
    ///
    /// # Panics
    ///
    /// If `out` accounts for a different number of substances than the registry
    /// this tick was folded from.
    pub fn domain_sums(&self, world: &World, out: &mut DomainSums) {
        assert_eq!(
            out.n_substances(),
            self.n_substances,
            "the domain sums account for {} substances and this world has {}",
            out.n_substances(),
            self.n_substances
        );
        out.clear();

        for s in 0..self.n_substances {
            match world.lane_of(s) {
                LaneRef::Narrow(lane) => {
                    let field = world
                        .amounts_32()
                        .expect("a narrow lane exists only if the narrow field does");
                    out.add_field_lane_32(s, field.lane(lane));
                }
                LaneRef::Wide(lane) => {
                    let field = world
                        .amounts_64()
                        .expect("a wide lane exists only if the wide field does");
                    out.add_field_lane_64(s, field.lane(lane));
                }
            }
        }

        // Energy: the enthalpy field, and nothing else in S0. `i64` since
        // ADR-062; the narrow door of `ledger/mod.rs` is the one that predates
        // that record.
        // `lane(0)` and not `read()`: the second carries the ghost cell, which
        // is the reservoir and not part of the domain (ADR-059, ADR-068). The
        // residual cannot see the difference — a constant cancels in
        // `after - before` — so only the absolute sum would lie.
        out.add_enthalpy_lane_64(world.enthalpy().lane(0));

        // The doors S0 has no data for. Three empty constants, and two of them
        // behind a lookup that answers `None` on every registry that exists — so
        // these lines execute nothing and prove nothing. They are here as the
        // shape of the left side rather than as a check of it; the check is
        // `TODO(s1-guilds)` below.
        //
        // The guild and cell doors take the substance the biomass is measured in
        // (`QUANTITIES.md` section 7), so they can only be reached on a registry
        // that declares it — which no S0 scenario does. The energy door takes no
        // substance and is called unconditionally.
        if let Some(biomass) = world.registry().index_of(BIOMASS) {
            out.add_guild_lane_32(biomass, GUILD_FIELDS);
            out.add_cell_masses_32(biomass, CELL_STRUCT_MASS);
        }
        out.add_cell_energy_64(CELL_ENERGY);
    }
}

/// The substance guild fields and the cell table's `struct_mass` are measured in
/// (`QUANTITIES.md` section 7, SPEC section 6.1).
// TODO(s1-guilds): the spelling is taken from `QUANTITIES.md`, which writes
// `BIOMASS`, and no scenario declares such a substance yet. Whether guild biomass
// is a substance of the registry at all — as against a quantity of its own with a
// door of its own — is decided by the wave that writes guild transport, together
// with `TODO(bt-invariant)` in `ledger/mod.rs`.
const BIOMASS: &str = "BIOMASS";

/// The guild fields of the domain. Empty in S0, and the emptiness is the whole
/// of it: this is a placeholder, not a protection.
// TODO(s1-guilds): three buffers have to be threaded into `Tick::domain_sums`
// with the wave that allocates them — the per-guild biomass field, the cell
// table's `struct_mass` column and its `energy` column (SPEC section 6.1) — and
// the honest form of the door is one that takes them off `&World`, so that
// emptiness comes from the world rather than from a constant here. Until then
// the left side of ADR-003 is the two amount fields and the enthalpy, and a
// guild that moved on the first tick of S1 would move inside a residual that
// closes. Nothing in this file can notice that, which is why it is written down
// twice: here, and in the doc comment of `Tick::domain_sums`.
const GUILD_FIELDS: &[crate::numeric::M32] = &[];

/// The `struct_mass` column of the cell table. Empty in S0; see
/// [`GUILD_FIELDS`].
const CELL_STRUCT_MASS: &[crate::numeric::M32] = &[];

/// The `energy` column of the cell table. Empty in S0; see
/// [`GUILD_FIELDS`].
const CELL_ENERGY: &[crate::numeric::M64] = &[];

/// Fold one width class, or `None` when the registry declared no substance of it.
///
/// A width class with no substance is not a field of zero lanes: `Field::new`
/// refuses one, and a dummy lane would shift the addressing of that class by one
/// (ADR-056).
fn fold_per_width<T>(lanes: u32, fold: impl FnOnce(u32) -> Result<T>) -> Result<Option<T>> {
    if lanes == 0 {
        return Ok(None);
    }
    fold(lanes).map(Some)
}

/// The substance index behind every lane of one width class, in lane order.
///
/// Through `World::lane_of`, which is the one door (ADR-056). What it feeds is
/// the channel credit of steps `c` and `d`: a table built the other way round
/// would post the lid's flow of one substance against another's counter, and
/// `Ledger::assert_closed` would then fail on **two** substances at once with
/// nothing pointing at the mapping.
fn substance_by_lane(world: &World, width: Width) -> Vec<u32> {
    let lanes = world.registry().lanes(width) as usize;
    let mut by_lane = vec![0u32; lanes];
    for s in 0..world.registry().n_substances() {
        match (world.lane_of(s), width) {
            (LaneRef::Narrow(lane), Width::Bits32) | (LaneRef::Wide(lane), Width::Bits64) => {
                by_lane[lane as usize] = s;
            }
            _ => {}
        }
    }
    by_lane
}

/// The diffusivity of every lane of one width class, in lane order.
///
/// Indexed by lane and filled through `World::lane_of`, which is the one door
/// from a substance to a buffer address (ADR-056). A lane of a width class the
/// registry has none of comes out an empty vector, which `fold_per_width` never
/// passes on.
fn diffusivity_by_lane(world: &World, derived: &Derived, width: Width) -> Vec<f64> {
    let lanes = world.registry().lanes(width) as usize;
    let mut by_lane = vec![0.0; lanes];
    for (s, substance) in derived.substances().iter().enumerate() {
        // `s` is the substance index the derivation and the registry share: both
        // are built in declaration order and `Tick::new` has just checked that
        // they are the same length.
        let s = s as u32;
        match (world.lane_of(s), width) {
            (LaneRef::Narrow(lane), Width::Bits32) | (LaneRef::Wide(lane), Width::Bits64) => {
                by_lane[lane as usize] = substance.diffusivity;
            }
            _ => {}
        }
    }
    by_lane
}

/// Refuse a process whose operator cannot be dispatched, naming what is missing.
///
/// One message per process, and each of them names the record or the open
/// question that has to close before the step can run. A refusal rather than a
/// skip: a scenario that switched a process on and got a world without it would
/// be indistinguishable from a scenario that switched it off, and both halves of
/// the invariant would close over the difference.
fn refuse_if_blocked(id: ProcessId) -> Result<()> {
    match id {
        ProcessId::Advection | ProcessId::Diffusion => Ok(()),
        ProcessId::Light => bail!(
            "process `{}` is enabled and step `a` cannot be dispatched, and what \
             blocks it is no longer a missing number: ADR-076 declares `i_surface` \
             and derives `units_per_intensity`, and ADR-075 settles what the fold \
             owes the ledger. What blocks it is a **pair** of open questions, and \
             neither is enough on its own. Energy has no sink in any scenario — \
             `RADIATIVE_OUT` is unimplemented and step `j` has nothing to write — \
             so absorbed light accumulates without bound and crosses the declared \
             temperature range in about 42 ticks at full sun, taking the Courant \
             bound proved at load with it. And the width of a channel counter is \
             open question A-20 — A-19 in ADR-075 and ADR-076, which were drafted \
             while that number was free — and an i64 `SOLAR_IN` holds \
             2^63/2^k_E = 62.5 mJ at k_E = 67, against 0.16384 J for one lit tick \
             of a 128^3 domain — 2.62 ceilings in a tick, so lifting the sink \
             alone would trade a \
             silent overflow of the field for a loud panic in the ledger. \
             `config/validate.rs` refuses a scenario with i_surface > 0 by the \
             same two locks; a dark box, i_surface = 0, is legal there and \
             pointless here. Its default is enabled = {} (`process/light.rs`, \
             ADR-076)",
            id.id(),
            light::ENABLED_BY_DEFAULT
        ),
        ProcessId::VelocityField => bail!(
            "process `{}` is enabled and step `b` cannot be dispatched, and \
             neither of the two things that used to block it does so any longer. \
             The buffers: `world::World` owns all four — the coarse potential on \
             the enthalpy grid, the potential interpolated onto the velocity grid, \
             the stirred copy of it and `u` itself — and hands them out together as \
             `World::velocity_slices_mut` (ADR-069). The denominator: `C_cell` is \
             identically `Q::ZERO` in every run, because `process::Temperature` is \
             its only writer and is dispatched by nothing, but ADR-079 answers a \
             non-positive `C_cell` about the *cell* rather than about one kernel, \
             and `kernels/potential.rs` now implements that answer beside \
             `kernels/temperature.rs`: the cell contributes no temperature anomaly \
             and nothing is divided. Two things are left and only one of them is \
             code. **Code:** the four keys of ADR-069 — u_conv_max, l_c, \
             stir_fraction, stir_period — are `[[process]]` keys of the `Config`, \
             `config/validate.rs` checks them and keeps none, `Derived` has no \
             velocity section, and `Tick::new` takes a `&Derived`; so nothing \
             outside `tests/` can build a `VelocityConfig`, and the precedent for \
             carrying them through is `DerivedSubstance::diffusivity`, copied for \
             exactly this reason. **A decision:** `TODO(courant-fold)` in the \
             header of this file. The last stage of `VelocityField::apply` writes \
             `Scratch::face_courant`, so the tick that dispatches step `b` is the \
             tick step `c` starts moving the amounts on — while \
             `Scratch::enthalpy_courant` stays zero, because reading a velocity \
             onto the faces of the coarser enthalpy grid is a fine-to-coarse fold \
             of a *flux* that no record writes. Matter carried by the flow and heat \
             standing still, with both residuals closing exactly. Its default is \
             enabled = {} (ADR-069)",
            id.id(),
            velocity::VELOCITY_FIELD_ENABLED_BY_DEFAULT
        ),
        ProcessId::Pressure => bail!(
            "process `{}` is enabled and step `e` cannot be dispatched: the \
             limiting overflow theta_max, from which ADR-055 derives the whole \
             mobility, is declared by no key and no record \
             (`TODO(theta-max)` in `process/pressure.rs`). A plausible number put \
             here would be indistinguishable from a decision, and the scheme \
             conserves exactly, so both residuals would close over it for ever. \
             Its default is enabled = {} (`process/pressure.rs`)",
            id.id(),
            pressure::ENABLED_BY_DEFAULT
        ),
        ProcessId::Settling => bail!(
            "process `{}` is enabled and step `f` cannot be dispatched: \
             `Medium` wants g and rho_medium, and no document assigns either a \
             value or a place (ADR-067, ADR-069) — which is why \
             `config/validate.rs` refuses a substance with settling_radius > 0 \
             outright. Its default is enabled = {} (`process/settle.rs`)",
            id.id(),
            settle::ENABLED_BY_DEFAULT
        ),
        ProcessId::PhaseTransitions => bail!(
            "process `{}` is enabled and line `g` has no operator at all: it is \
             one of the nine records ADR-065 materialises and it appears in no \
             step of the tick order. Enabling it changes `config_hash` and \
             nothing else, which is a world that looks configured and is not. Its \
             default is enabled = {} (`process/phase.rs`)",
            id.id(),
            super::phase::ENABLED_BY_DEFAULT
        ),
        ProcessId::Reactions => bail!(
            "process `{}` is enabled and step `h` cannot be dispatched, and the \
             temperature is no longer what blocks it — `process/temperature.rs` \
             derives `T` from enthalpy and the actual composition (ADR-044, \
             ADR-062) and `world::World` owns both coarse buffers. What is left is \
             both arches of the invariant. Matter: `ledger::residual_matter` is \
             taken per substance and chemistry turns substances into one another, \
             so `Ledger::assert_closed` panics on the first tick with any reaction \
             in it, and the statement chemistry does satisfy is an arm of \
             `process::Conservation` that no record has worded. Energy: \
             `Tick::domain_sums` counts the enthalpy field alone, there is no door \
             for the chemical energy of the substances, and the sign of the \
             increment — `nu_E` or `-nu_E`, that is, whether an exothermic \
             reaction warms the cell — is chosen by nothing. Step `i'` is no longer \
             blocked in its own right — ADR-076 derives `units_per_intensity` and \
             ADR-075 settles what the fold owes the ledger (A-16 closed) — but \
             the two are dispatched together or not at all \
             (ADR-045: the reaction kernel overwrites its cell, so a fold over a \
             stale accumulator credits last tick's energy again). Its default is \
             enabled = {} (`process/react.rs`)",
            id.id(),
            react::ENABLED_BY_DEFAULT
        ),
        ProcessId::ExternalChannels => bail!(
            "process `{}` is enabled and step `j` has nothing to write: there are \
             no events — `IMPACT` and `VENT_BURST` are described by no schedule, \
             key or type anywhere. `BOUNDARY_EXCHANGE` is not among the blockers \
             and never was this step's to write: steps `c` and `d` credit it on \
             every substep (ADR-059). The temperature field is no longer among \
             them either, but the two heat channels ADR-059 does assign here \
             are: \
             `RADIATIVE_OUT` wants an emissivity nothing declares, `GEOTHERMAL_IN` \
             a heat flux and a vent composition, and the sign convention of a \
             counter is `TODO(counter-sign)` in `ledger/mod.rs`. Its default is \
             enabled = {} (`process/channels.rs`)",
            id.id(),
            super::channels::ENABLED_BY_DEFAULT
        ),
    }
}

// The delegation of ADR-065, checked at compile time for the two processes this
// file dispatches. `every_roster_default_comes_from_its_own_module` covers all
// nine at test time; these two are also a build failure, because they are the
// pair whose defaults this tick's refusal table depends on — a `Diffusion`
// default that stopped coming from `diffuse.rs` would change which scenarios fold
// at all.
const _: () = {
    assert!(advect::ENABLED_BY_DEFAULT == ProcessId::Advection.enabled_by_default());
    assert!(diffuse::ENABLED_BY_DEFAULT == ProcessId::Diffusion.enabled_by_default());
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_step_order_is_the_one_spec_section_8_prints() {
        // The only thing in the repository that can see a permutation of two
        // Lie-Trotter factors. Both residuals close under any order, every
        // "alone" test and every kernel test stays green, and there is no golden
        // run to disagree.
        let letters: Vec<&str> = STEP_ORDER.iter().map(|s| s.letter()).collect();
        assert_eq!(letters, ["a", "b", "c", "d", "e", "f", "h", "i'", "j"]);
    }

    #[test]
    fn light_stands_before_the_energy_fold() {
        // ADR-049: the fold computes absorption out of the stored light field, so
        // the field has to belong to the *current* tick. The other order lags it
        // by one with the energy residual staying at exactly zero — the counter
        // and the enthalpy move together.
        let position = |want: Step| {
            STEP_ORDER
                .iter()
                .position(|&s| s == want)
                .expect("every step is in STEP_ORDER")
        };
        assert!(position(Step::Light) < position(Step::EnergyFold));
    }

    #[test]
    fn every_step_but_the_fold_names_a_process() {
        // ADR-065 keeps `i'` out of the roster: it is part of the energy path of
        // the reactions (ADR-045), not a process a scenario can switch off.
        for step in STEP_ORDER {
            match step {
                Step::EnergyFold => assert_eq!(step.process(), None),
                other => assert!(
                    other.process().is_some(),
                    "{} names no process",
                    other.letter()
                ),
            }
        }
    }

    #[test]
    fn one_roster_entry_has_no_step_of_its_own() {
        // Line `g` — phase transitions — and only it. Line `j` has a step and
        // nothing to write, which is a different thing: a step whose operator is
        // blocked is refused by `Tick::new`, a process with no step is not
        // refused by anything, and the roster carries both (ADR-065). Stated as a
        // test so that giving `g` a step, or taking `j`'s away, is a visible edit.
        let with_a_step: Vec<ProcessId> = STEP_ORDER.iter().filter_map(|s| s.process()).collect();
        for id in ProcessId::ALL {
            let has = with_a_step.contains(&id);
            match id {
                ProcessId::PhaseTransitions => assert!(!has, "line g has no operator"),
                _ => assert!(has, "{} has no step", id.id()),
            }
        }
    }
}
