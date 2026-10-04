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
//! Velocity, advection, diffusion, settling and reactions are dispatched, with
//! the energy fold paired with reactions. Advection and diffusion include all
//! three transported fields. The remaining steps are refused by [`Tick::new`], each
//! naming the number or the operator that is missing, rather than being skipped
//! quietly:
//!
//! | step | what is missing |
//! |---|---|
//! | `a` light | the attenuator measure: `Light::new` wants a table of `Attenuator`, and what `conc_per_unit` multiplies is settled by no document (`TODO(attenuation-measure)`). Both locks of ADR-076 are gone — ADR-084 ratifies the `exchange` face as the sink, ADR-083 the counter width |
//! | `e` pressure | five locks, and `theta_max` is no longer one of them (ADR-082 declares it): no owner for the overflow field and no `partial_molar_volume` or `Occupant` table in `Derived`; no behaviour on an `exchange` face; one-sidedness undecided; `energy: Conserved` standing on `TODO(enthalpy-of-transport)` |
//! | `j` channels | three, and not the four this used to read: there are no events, `RADIATIVE_OUT` has no emissivity and `GEOTHERMAL_IN` no heat flux or vent composition. The sign of a counter left the list with ADR-084 |
//!
//! A refusal rather than a skip, because a skipped step is a different world that
//! looks like the same one. The one thing a refusal costs is that the roster's
//! defaults have to be `false` wherever the operator is blocked — which is the
//! reading ADR-065 already argued for when it rejected a blanket `enabled = true`
//! ("it enables processes that do not exist yet").
//!
//! # The Courant buffers, and the one hole that is left in them
//!
//! Both are filled by step `b` and read by step `c` of the same tick, which is
//! what puts them in [`Scratch`] (ADR-086). `VelocityField::apply` writes
//! `face_courant` with its fourth stage and folds `enthalpy_courant` out of it
//! with its fifth (ADR-087): the flux through the `2^(2*lod)` fine faces tiling a
//! coarse face, divided by the area of the coarse face — never the sum, and never
//! the mean, of the Courant numbers, which are intensive. So the tick that
//! dispatches step `b` is the tick step `c` starts moving both the amounts and
//! the enthalpy on, and `TODO(velocity-interpolation)` in `process/advect.rs` is
//! stale for both halves.
//!
//! On a roster that leaves the velocity field off, both buffers stay at `Q::ZERO`
//! and every application of advection is an application of a zero flux: a
//! dispatched step that moves nothing, which is a different thing from a skipped
//! step (ADR-057 wants the parity of a lane to come from the applications the
//! phase *ran*) and an indistinguishable thing by its result.
//!
//! TODO(exchange-courant): neither buffer's **ghost** cell is written by anybody.
//! The last element of each lane is the Courant number of the face of the
//! *domain* (ADR-059), and what belongs in it is undecided; both kernels
//! deliberately stop one short, so this is one hole and not two.
//!
//! What this costs, said out loud because ADR-087 requires it to be: in a release
//! build there is now **no run-time check of the actual field on either grid**.
//! [`Advect::fold_courant`](super::Advect::fold_courant) — the only place in the
//! project that checks both inequalities of SPEC section 4.2 against the velocity
//! a face actually has rather than against the declared `u_conv_max` — is bypassed
//! by both kernels and is called from nowhere in this crate. ADR-087 neither
//! deletes it nor calls it: deleting it belongs to the record that assigns
//! run-time control or refuses it outright. Debug keeps
//! `VelocityField::within_speed_bound` on the field and `within_coarse_courant` on
//! the folded buffer, and the second of those names its own blind spot — a sum
//! with no divisor trips it only above `dx/(32*dt)`, 18.75% of the validator's
//! ceiling.
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
    AdvectPhase, DiffusePhase, Fold, ProcessId, ROSTER_LEN, RosterEntry, Temperature, advect,
    diffuse, fold, light, pressure, react, settle, velocity,
};
use crate::config::{Config, Derived};
use crate::ledger::{DomainSums, Ledger, Nu};
use crate::numeric::{M32, M64, Q};
use crate::process::react::React;
use crate::world::{LaneRef, OwnedBuffersMut, Width, World};

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

/// The steps the temperature is recomputed immediately before, one at each of
/// the denominator's two readers (ADR-086).
///
/// A named constant for the reason [`STEP_ORDER`] is one, and here the reason
/// used to be sharper: while step `b` did not dispatch, the head pass had no
/// behavioural witness at all — both recomputations write the same two buffers,
/// so two passes over an unchanged state are bit-identical to one, and deleting
/// the head pass left the whole suite green. Since ADR-087 the consumer standing
/// between them dispatches, and `the_denominator_is_fresh_for_step_b_and_for_step_h`
/// is a claim about a step that runs. The array stays: `Tick::advance` consults
/// it rather than matching on the two steps inline, so that "how many
/// recomputations there are and where" is a datum instead of a shape of code.
///
/// No tenth letter appears in `STEP_ORDER` for them: the temperature is a step of
/// the tick and not a letter of the spec (ADR-079).
const TEMPERATURE_SLOTS: [Step; 2] = [Step::Velocity, Step::Reactions];

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
    /// Step `f` over the narrow field, absent when the scenario has no narrow
    /// substance or settling is off (ADR-067, ADR-085).
    ///
    /// No enthalpy twin beside these two, and the absence is the decision:
    /// **no** transport process in this project carries enthalpy along with the
    /// matter it moves, and the field is transported on its own grid by steps
    /// `c` and `d` (ADR-062, ADR-067).
    settle_32: Option<settle::SettlePhase>,
    /// Step `f` over the wide field.
    settle_64: Option<settle::SettlePhase>,
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
    /// `w_s` for every substance, in **substance** order (ADR-081).
    ///
    /// Folded once here, out of `Derived::chemical_weights`, and read twice a
    /// tick: by [`Tick::domain_sums`], which weighs the left side of the energy
    /// invariant with it, and by the two transport phases, which owe
    /// `BOUNDARY_EXCHANGE` the chemical energy of whatever crossed the lid.
    /// Fourteen `i64` on the registry of SPEC section 2.3 — 112 bytes, and
    /// nothing per voxel.
    ///
    /// By substance and not by lane, and reached from a lane through
    /// `substance_of_lane_*` above (ADR-056).
    chemical_weight: Vec<i64>,
    /// Step `b`, folded when the roster enables it (ADR-069, ADR-087).
    ///
    /// `None` on every roster that leaves the velocity field off, and `Some` on
    /// every roster that turns it on: the two locks of the old refusal are gone —
    /// `Derived::velocity` carries the four keys of ADR-069 through, and the fold
    /// onto the faces of the enthalpy grid is the fifth stage of the operator.
    velocity: Option<velocity::VelocityField>,
    /// Voxels of the fine grid, for the shape of the Courant buffer.
    n_voxels: u32,
    /// Cells of the enthalpy grid, for the shape of the other one.
    n_enthalpy_cells: u32,
    /// Step `h`, with catalysts gathered from current registry biomass.
    react: Option<React>,
    /// Step `i'`, folded on every run.
    ///
    /// Unconditional because [`Fold::new`] refuses nothing a loaded scenario can
    /// present, and because `i_surface` has to be `Q::ZERO` exactly when the
    /// light process is off — a fact of the *fold* and not of the roster, which
    /// is why it is decided once here rather than at each dispatch.
    fold: Fold,
    /// The temperature operator, folded on every run and dispatched twice a tick.
    temperature: Temperature,
    /// How many reactions the **scenario** declares, whatever the roster says.
    ///
    /// The second factor of the extent slice's length (ADR-080). Off the config
    /// and never off the folded `React`: a buffer whose length depended on a
    /// process being enabled would move every address in the world when it was
    /// switched off (ADR-086).
    n_reactions: u32,
    /// How many of those reactions name a catalyst.
    catalyst_columns: u32,
    ceilings: Vec<(String, LaneRef, i128)>,
}

/// How many components the prescribed velocity field and its potentials hold per
/// cell.
///
/// Three, because all four are vector fields on a coarse grid: ADR-069 prices `A`
/// and `u` at `3 * 64^3 * 4 B` each.
// TODO(velocity-layout): two numbers about this field are undecided and neither
// can be guessed at from the price. ADR-069 fixes the volume and the grid and
// leaves open (a) whether the storage is component-major or voxel-major, and
// (b) whether `u` is three components per cell or one per face. Until they are
// decided the buffers here are flat and long enough, and nothing indexes into any
// of the four: an accessor that picked an order would settle (a) in code.
const VELOCITY_COMPONENTS: u32 = 3;

/// The buffers a tick needs and the world does not own — which, since ADR-086, is
/// every buffer whose writer stands **earlier in the tick than its reader**.
///
/// Separate from [`World`] because they hold nothing between ticks: every one of
/// them is written before it is read inside a single [`Tick::advance`]. Separate
/// from [`Tick`] because `Tick` is shared and these are `&mut`.
///
/// That sentence used to be a description of this type and is now the criterion
/// that assigns every buffer of the project to one of the two owners. Its
/// operational form is `poisoning_the_scratch_before_a_tick_changes_no_buffer_of_the_world`:
/// fill everything here with rubbish before a tick and the world after the tick
/// must match a run whose scratch was zeroed, bit for bit. A buffer classified
/// here that in truth carries information across a tick boundary fails there and
/// nowhere else.
///
/// **Everything here is allocated on every run**, whatever the roster says, on
/// the precedent already standing for the Courant buffer ("Three per voxel
/// whether or not advection runs"): a buffer whose *length* depended on a process
/// being enabled would move every address in the world when it was switched off,
/// and `a_disabled_process_leaves_every_buffer_bit_for_bit` would compare buffers
/// of different lengths.
#[derive(Clone, Debug)]
pub struct Scratch {
    catalyst: Vec<Q>,
    /// Three Courant numbers per **fine** voxel, in the layout
    /// `kernels/advect.rs` reads. Written by the last stage of step `b`, read by
    /// step `c` of the same tick — which is what puts it here (ADR-086, closing
    /// `TODO(courant-buffer-owner)`).
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
    /// The reaction energy accumulator: fine grid, `i64`, one buffer. Step `h`
    /// writes it, step `i'` reads it, and the two stand in one tick.
    ///
    /// Never cleared, here or anywhere: the reaction kernel **overwrites** its
    /// cell rather than adding to it, which is the whole of ADR-045's argument
    /// against a clearing pass. That is also why steps `h` and `i'` are
    /// dispatched together or not at all — a fold over a stale accumulator
    /// credits last tick's energy again, and a stale number is indistinguishable
    /// from a fresh one.
    energy_delta: Vec<M64>,
    /// The extent of every reaction in every fine voxel: `n_voxels *
    /// n_reactions` cells of `M32`, one block per voxel (ADR-080). Step `h`
    /// writes it and the LEDGER phase reduces it.
    ///
    /// Allocated in **both** profiles although only the debug build reduces it,
    /// and ADR-086 keeps it that way on purpose: gating it on
    /// `cfg(debug_assertions)` would give the host two signatures and two
    /// implementations — the class of divergence ADR-015 made the CPU version the
    /// eternal reference against — and would make the loader's footprint report
    /// print bytes the release build does not allocate.
    xi_out: Vec<M32>,
    /// What the fold of step `i'` owes `SOLAR_IN`, one `M64` per **coarse** cell
    /// (ADR-075). The reader is the host reduction standing immediately behind
    /// the writer, on the same step.
    solar: Vec<M64>,
    /// The light field: fine grid, one buffer, what **leaves** a voxel through
    /// its bottom face (`kernels/light.rs`). Step `a` writes it and step `i'`
    /// reads it, in one tick — which is why `every_n_ticks > 1` on the light
    /// process is a load error (`config/derive.rs`): a schedule that separated
    /// the two would turn this into inter-tick state of class `Q` that no
    /// snapshot carries.
    light: Vec<Q>,
    /// The prescribed velocity field `u`, on the velocity grid.
    velocity: Vec<Q>,
    /// The potential `u` is the curl of, interpolated onto the velocity grid.
    velocity_potential: Vec<Q>,
    /// The wide difference's output, on the **enthalpy** grid: the only one of
    /// the four sized by the coarser of the two coarse grids (ADR-069).
    velocity_potential_coarse: Vec<Q>,
    /// The noised copy of the interpolated potential. A separate buffer and not
    /// an addition in place: `stir_potential` reads every octave's contribution
    /// off the source, and a kernel may not read the buffer it writes (ADR-034).
    velocity_potential_stirred: Vec<Q>,
    /// `sum(n_i * c_p_i)` of a coarse cell, J/K: enthalpy grid, class `Q`
    /// (ADR-044, ADR-062).
    ///
    /// Here and not in `World`, and that is the load-bearing half of ADR-086.
    /// Under one recomputation a tick this buffer had a reader — step `b` —
    /// standing earlier than its writer, so it was state a snapshot had to carry
    /// and class `Q` has no byte door: the criterion came out false on the day it
    /// was written. Recomputing at **each** reader removes the reader, not the
    /// rule. ADR-044 stands entire, and precisely because there is still no
    /// cache: two recomputations are two computations from state.
    heat_capacity: Vec<Q>,
    /// `T_ref + H/C_cell`, kelvin: enthalpy grid, class `Q`. See
    /// [`Scratch::heat_capacity`].
    temperature: Vec<Q>,
    /// The left-hand side of the invariant before the tick.
    before: DomainSums,
    /// And after it. Two accumulators reused across ticks rather than allocated
    /// twice a tick (`ledger/mod.rs`).
    after: DomainSums,
}

/// Every buffer [`Scratch`] owns, shared, as one value — the twin of
/// [`crate::world::OwnedBuffers`] on the other owner.
///
/// **The type is the enumeration**, and it exists for the reason that type does:
/// a thirteenth buffer added to `Scratch` and forgotten by a walker is `E0027`
/// here rather than a buffer quietly missing from a bit-for-bit comparison. That
/// is not a hypothetical shape of defect — it is the one ADR-086 was written
/// against (`solar_in`, the twelfth buffer of a guard array declared eleven
/// long), and the outside walkers of this owner are exactly the comparisons on
/// which `same_seed_and_config_give_byte_identical_state` and
/// `a_disabled_process_leaves_every_buffer_bit_for_bit` rest.
///
/// The two `DomainSums` are **not** here and their absence is the one judgement
/// this type makes: they are accumulators of the debug LEDGER phase, refilled
/// from the world before every use, and never a buffer of the run. Everything
/// else is.
#[derive(Debug)]
pub struct ScratchBuffers<'a> {
    pub catalyst: &'a [Q],
    /// Three Courant numbers per fine voxel. See [`Scratch::face_courant`].
    pub face_courant: &'a [Q],
    /// Three per cell of the enthalpy grid.
    pub enthalpy_courant: &'a [Q],
    /// The reaction energy accumulator.
    pub energy_delta: &'a [M64],
    /// The extent of every reaction in every voxel.
    pub xi_out: &'a [M32],
    /// What the fold owes `SOLAR_IN`.
    pub solar: &'a [M64],
    /// The light field.
    pub light: &'a [Q],
    /// The prescribed velocity field.
    pub velocity: &'a [Q],
    /// The potential it is the curl of.
    pub velocity_potential: &'a [Q],
    /// The wide difference's output on the enthalpy grid.
    pub velocity_potential_coarse: &'a [Q],
    /// The noised copy of the interpolated potential.
    pub velocity_potential_stirred: &'a [Q],
    /// The denominator of `T = T_ref + H/C_cell`.
    pub heat_capacity: &'a [Q],
    /// The temperature.
    pub temperature: &'a [Q],
}

/// Every buffer [`Scratch`] owns, mutably. See [`ScratchBuffers`].
///
/// The instrument of the poison and of the guards that perturb one buffer at a
/// time, and not a door around the dispatch accessors: an operator takes the
/// slices its kernel is written over — `react_slices_mut` and its neighbours —
/// so that a process cannot reach a buffer it did not name (ADR-034).
#[derive(Debug)]
pub struct ScratchBuffersMut<'a> {
    pub catalyst: &'a mut [Q],
    /// See [`ScratchBuffers::face_courant`].
    pub face_courant: &'a mut [Q],
    /// See [`ScratchBuffers::enthalpy_courant`].
    pub enthalpy_courant: &'a mut [Q],
    /// See [`ScratchBuffers::energy_delta`].
    pub energy_delta: &'a mut [M64],
    /// See [`ScratchBuffers::xi_out`].
    pub xi_out: &'a mut [M32],
    /// See [`ScratchBuffers::solar`].
    pub solar: &'a mut [M64],
    /// See [`ScratchBuffers::light`].
    pub light: &'a mut [Q],
    /// See [`ScratchBuffers::velocity`].
    pub velocity: &'a mut [Q],
    /// See [`ScratchBuffers::velocity_potential`].
    pub velocity_potential: &'a mut [Q],
    /// See [`ScratchBuffers::velocity_potential_coarse`].
    pub velocity_potential_coarse: &'a mut [Q],
    /// See [`ScratchBuffers::velocity_potential_stirred`].
    pub velocity_potential_stirred: &'a mut [Q],
    /// See [`ScratchBuffers::heat_capacity`].
    pub heat_capacity: &'a mut [Q],
    /// See [`ScratchBuffers::temperature`].
    pub temperature: &'a mut [Q],
}

/// How long every buffer of a [`Scratch`] is, before one of them is allocated.
///
/// Split out of [`Scratch::new`] for the footprint line of the load report
/// (ADR-086): a run at `R = 64` has to be able to read its gigabyte **before** it
/// asks for it, and a projection computed beside the allocation rather than out
/// of it is the only kind that can be printed first. The duplication that buys is
/// paid for by `load_reports_the_per_voxel_footprint_of_the_world_and_the_scratch`,
/// which holds the projection against what an allocated `Scratch` actually
/// carries, buffer by buffer through [`ScratchBuffers`].
struct ScratchShape {
    catalyst_cells: usize,
    n_voxels: usize,
    n_coarse: usize,
    n_velocity: usize,
    n_coarse_potential: usize,
    xi_cells: usize,
}

impl ScratchShape {
    /// The lengths a `Scratch` for `world` under `tick` would have.
    fn of(world: &World, tick: &Tick) -> Result<ScratchShape> {
        let n_voxels = world.grid().n_voxels() as usize;
        let n_coarse = world.enthalpy_grid().n_voxels() as usize;
        let n_velocity = world
            .velocity_grid()
            .n_voxels()
            .checked_mul(VELOCITY_COMPONENTS)
            .with_context(|| {
                format!(
                    "a velocity field of {VELOCITY_COMPONENTS} components over \
                     {} cells is longer than a u32 index can address",
                    world.velocity_grid().n_voxels()
                )
            })? as usize;
        // Off the **enthalpy** grid and never off the velocity one: the wide
        // difference of ADR-069 is taken where the temperature lives. Sized off
        // the other coarse grid this is eight times too long at the eco layout —
        // a loud panic in `VelocityField::apply` — and on a layout whose two lods
        // coincide it is exactly the right length for the wrong grid.
        let n_coarse_potential = world
            .enthalpy_grid()
            .n_voxels()
            .checked_mul(VELOCITY_COMPONENTS)
            .with_context(|| {
                format!(
                    "a coarse velocity potential of {VELOCITY_COMPONENTS} \
                     components over {n_coarse} enthalpy cells is longer than a \
                     u32 index can address"
                )
            })? as usize;
        // `n_voxels * n_reactions`, and the second factor comes from the
        // *scenario* rather than from the roster: a length that depended on
        // whether step `h` was enabled would change when it was switched off
        // (ADR-080, ADR-086).
        let xi_cells = n_voxels
            .checked_mul(tick.n_reactions as usize)
            .with_context(|| {
                format!(
                    "an extent slice of {n_voxels} voxels times {} reactions is \
                     longer than an index can address",
                    tick.n_reactions
                )
            })?;

        Ok(ScratchShape {
            catalyst_cells: (n_voxels + 1)
                .checked_mul(tick.catalyst_columns as usize)
                .context("the catalyst columns exceed addressable storage")?,
            n_voxels,
            n_coarse,
            n_velocity,
            n_coarse_potential,
            xi_cells,
        })
    }

    /// How many bytes those lengths come to.
    ///
    /// Class by class and never `size_of::<Scratch>()`: the struct is twelve
    /// `Vec` headers and the bytes are behind them. `Q` is four bytes and `M64`
    /// eight by the same declarations `numeric/` makes, so the two are asked
    /// rather than written down.
    fn bytes(&self) -> usize {
        let q = size_of::<Q>();
        let m32 = size_of::<M32>();
        let m64 = size_of::<M64>();
        3 * (self.n_voxels + 1) * q
            + self.catalyst_cells * q
            + 3 * (self.n_coarse + 1) * q
            + self.n_voxels * m64
            + self.xi_cells * m32
            + self.n_coarse * m64
            + self.n_voxels * q
            + 2 * self.n_velocity * q
            + self.n_coarse_potential * q
            + self.n_velocity * q
            + 2 * self.n_coarse * q
    }
}

/// What a run is about to occupy, in both owners, before the second of them is
/// allocated.
///
/// The loader's answer to a memory ceiling, and ADR-086 chose it over one on
/// purpose: every factor of the product is bounded already — `S_MAX = 32`,
/// `R_MAX = 64`, `N_MAX = 64` — so a byte limit would have to come out of a
/// number the corpus does not name, and as a scenario key it would be raised to
/// whatever the scenario liked. What is left that helps is the number itself,
/// printed before the allocation: at `R = 64` and 128 cubed the scratch alone is
/// 597.95 MB.
///
/// **Per fine voxel and in bytes, both.** The per-voxel figure is what the
/// budget of SPEC section 1.2 is written in and therefore what a divergence from
/// it is visible in; the total is what the machine has to find.
#[derive(Clone, Copy, Debug)]
pub struct Footprint {
    /// What [`World`] holds: class `M` and nothing else since ADR-086.
    world: usize,
    /// What a [`Scratch`] for it would hold — a projection, not a measurement.
    scratch: usize,
    /// Fine voxels, the denominator of every per-voxel figure below.
    n_voxels: usize,
    /// The second factor of the extent slice, printed because it is the one
    /// input a scenario can move by a factor of sixty-four (ADR-080).
    n_reactions: u32,
}

impl Footprint {
    /// Measure the world and project the scratch.
    ///
    /// # Errors
    ///
    /// Returns an error if a buffer of the scratch would be longer than an index
    /// can address — the same failure [`Scratch::new`] reports, arriving one line
    /// earlier.
    pub fn of(world: &World, tick: &Tick) -> Result<Footprint> {
        Ok(Footprint {
            world: world.footprint_bytes(),
            scratch: ScratchShape::of(world, tick)?.bytes(),
            n_voxels: world.grid().n_voxels() as usize,
            n_reactions: tick.n_reactions,
        })
    }

    /// What the world's buffers occupy, in bytes.
    #[must_use]
    pub fn world_bytes(&self) -> usize {
        self.world
    }

    /// What the scratch buffers will occupy, in bytes.
    #[must_use]
    pub fn scratch_bytes(&self) -> usize {
        self.scratch
    }

    /// The line the loader prints.
    ///
    /// Three per-voxel figures and their totals, because the classification of
    /// ADR-086 is exactly what moves bytes between the first two: a buffer that
    /// drifts back into `World` shows up here as a snapshot that grew.
    ///
    /// **Two spaces of numbers meet on this line and only one of them is
    /// printed.** The per-voxel budget of the corpus — 230 B of ADR-062, 250 B
    /// with the three scratch buffers ADR-086 adds, against the 226 B SPEC
    /// section 1.2 prints and cannot be edited (ADR-032) — is a *plan*: it counts
    /// guild and trait fields that exist in no line of code yet. What this line
    /// prints is what this scenario allocates, at its own `R` and its own
    /// substance widths. Naming the difference in the line itself is cheaper than
    /// a reader deciding the budget moved.
    #[must_use]
    pub fn report(&self) -> String {
        let per_voxel = |bytes: usize| bytes as f64 / self.n_voxels as f64;
        let total = self.world + self.scratch;
        format!(
            "memory: world {:.1} B/voxel ({:.2} MB), scratch {:.1} B/voxel \
             ({:.2} MB), together {:.1} B/voxel ({:.2} MB) over {} voxels at \
             R = {} — what this scenario allocates, not the per-voxel budget\n",
            per_voxel(self.world),
            self.world as f64 / 1.0e6,
            per_voxel(self.scratch),
            self.scratch as f64 / 1.0e6,
            per_voxel(total),
            total as f64 / 1.0e6,
            self.n_voxels,
            self.n_reactions,
        )
    }
}

impl Scratch {
    /// Allocate everything one tick needs.
    ///
    /// # Errors
    ///
    /// Returns an error if the domain sums cannot be built — a world with no
    /// substances, which closes against anything (ADR-003) — or if a buffer would
    /// be longer than a `u32` index can address.
    pub fn new(world: &World, tick: &Tick) -> Result<Scratch> {
        let ScratchShape {
            catalyst_cells,
            n_voxels,
            n_coarse,
            n_velocity,
            n_coarse_potential,
            xi_cells,
        } = ScratchShape::of(world, tick)?;

        Ok(Scratch {
            catalyst: vec![Q::ZERO; catalyst_cells],
            // `lane_len` per axis and not `n_voxels`: the ghost cell owns the
            // Courant number of the face of the domain (ADR-059).
            face_courant: vec![Q::ZERO; 3 * (n_voxels + 1)],
            enthalpy_courant: vec![Q::ZERO; 3 * (n_coarse + 1)],
            energy_delta: vec![M64::ZERO; n_voxels],
            xi_out: vec![M32::ZERO; xi_cells],
            // On the coarse grid, because it is the fold's output and the fold is
            // dispatched per coarse cell. Sized off the enthalpy grid rather than
            // off the fine one: at `lod = 2` and 128 cubed the difference is
            // 262 kB against 16.8 MB (ADR-075).
            solar: vec![M64::ZERO; n_coarse],
            light: vec![Q::ZERO; n_voxels],
            velocity: vec![Q::ZERO; n_velocity],
            velocity_potential: vec![Q::ZERO; n_velocity],
            velocity_potential_coarse: vec![Q::ZERO; n_coarse_potential],
            velocity_potential_stirred: vec![Q::ZERO; n_velocity],
            // Zeroed because a `Vec` has to start somewhere, and not maintained
            // at zero afterwards: `Temperature::apply` rewrites both in full
            // before either is read, twice a tick. A zero heat capacity is not a
            // neutral initial value — it is the one value the kernel refuses to
            // divide by — so "the operator has not run yet" and "this cell has no
            // temperature" are the same bits, and under ADR-079 both read
            // `T = T_ref`. That is why the operator runs before each of its
            // consumers rather than after, and not why the buffer starts at zero.
            heat_capacity: vec![Q::ZERO; n_coarse],
            temperature: vec![Q::ZERO; n_coarse],
            before: DomainSums::new(tick.n_substances).context("the domain sums before a tick")?,
            after: DomainSums::new(tick.n_substances).context("the domain sums after a tick")?,
        })
    }

    /// The four slices of `React::apply` this owner holds: `(energy_delta,
    /// xi_out, temperature, catalyst)`.
    ///
    /// The other four — the two source and the two destination amount buffers —
    /// come from `World::react_slices_mut`, and the two owners borrow disjointly.
    /// That is how `TODO(one-borrow-per-dispatch)` was closed: by moving an owner
    /// rather than by widening an accessor, so no host copies the accumulator
    /// around the dispatch and no host can forget to copy it back (ADR-086).
    ///
    /// Catalysts are regenerated from post-transport amounts before step h.
    /// They carry no state across a tick and belong to Scratch (ADR-091).
    #[inline]
    pub fn react_slices_mut(&mut self) -> (&mut [M64], &mut [M32], &[Q], &mut [Q]) {
        (
            &mut self.energy_delta,
            &mut self.xi_out,
            &self.temperature,
            &mut self.catalyst,
        )
    }

    /// The three slices of the fold of step `i'` this owner holds:
    /// `(energy_delta, light, solar)`.
    ///
    /// The remaining pair — state `N` of the enthalpy and the receiver of state
    /// `N+1` — comes out of `world.enthalpy_mut().pair_mut()`, and has to come
    /// from one borrow there or the kernel would read the buffer it writes.
    #[inline]
    pub fn fold_slices_mut(&mut self) -> (&[M64], &[Q], &mut [M64]) {
        (&self.energy_delta, &self.light, &mut self.solar)
    }

    /// The two outputs of `Temperature::apply`: `(heat_capacity, temperature)`.
    ///
    /// The three inputs are the world's — `World::amount_slices()` and
    /// `World::enthalpy().lane(0)` — so this accessor is two slices and not five.
    /// `lane(0)` and never `read()` on that last one: the enthalpy lane carries
    /// the ghost cell after the coarse cells (ADR-059), and the reservoir is not
    /// a coarse cell.
    #[inline]
    pub fn temperature_slices_mut(&mut self) -> (&mut [Q], &mut [Q]) {
        (&mut self.heat_capacity, &mut self.temperature)
    }

    /// Everything one dispatch of `VelocityField::apply` touches that this owner
    /// holds, in that function's argument order: `(heat_capacity,
    /// coarse_potential, potential, stirred, velocity, face_courant,
    /// enthalpy_courant)`.
    ///
    /// One shared borrow beside six exclusive ones out of one aggregate, which
    /// is why it is one accessor and not seven. The remaining argument, the
    /// enthalpy's front buffer, is the world's.
    ///
    /// The seventh arrived with ADR-087 and it grew this accessor rather than
    /// standing beside it: both Courant buffers come from `Scratch` by the
    /// criterion of ADR-086 — writer earlier in the tick than reader — and one
    /// borrow per dispatch is what keeps two `&mut` out of one aggregate.
    #[inline]
    #[allow(clippy::type_complexity)]
    // Seven slices against a clippy threshold that would rather see a struct, on
    // the precedent the accessors of `world/world.rs` set and for the same stated
    // reason: the shape *is* the kernel's argument list, and naming a type for it
    // hides the dispatch contract from whoever reads the accessor.
    pub fn velocity_slices_mut(
        &mut self,
    ) -> (
        &[Q],
        &mut [Q],
        &mut [Q],
        &mut [Q],
        &mut [Q],
        &mut [Q],
        &mut [Q],
    ) {
        (
            &self.heat_capacity,
            &mut self.velocity_potential_coarse,
            &mut self.velocity_potential,
            &mut self.velocity_potential_stirred,
            &mut self.velocity,
            &mut self.face_courant,
            &mut self.enthalpy_courant,
        )
    }

    /// The light field, mutably: the one output of step `a`.
    #[inline]
    pub fn light_mut(&mut self) -> &mut [Q] {
        &mut self.light
    }

    /// What the last fold owes `SOLAR_IN`, for the reduction standing behind it.
    #[inline]
    #[must_use]
    pub fn solar(&self) -> &[M64] {
        &self.solar
    }

    /// The extent slice, for the reduction of phase 5 (ADR-080).
    #[inline]
    #[must_use]
    pub fn xi_out(&self) -> &[M32] {
        &self.xi_out
    }

    /// The denominator of `T = T_ref + H/C_cell`, as the last recomputation left
    /// it.
    #[inline]
    #[must_use]
    pub fn heat_capacity(&self) -> &[Q] {
        &self.heat_capacity
    }

    /// The temperature, as the last recomputation left it.
    #[inline]
    #[must_use]
    pub fn temperature(&self) -> &[Q] {
        &self.temperature
    }

    /// Every buffer this owner holds, shared, as one value.
    ///
    /// The door every outside walker of this owner goes through — the bit-for-bit
    /// comparisons of `tests/acceptance_tick.rs` and the footprint line of the
    /// load report. Through [`ScratchBuffers`] and never through twelve
    /// accessors, so that the enumeration is the compiler's rather than each
    /// walker's own (ADR-086).
    #[must_use]
    pub fn buffers(&self) -> ScratchBuffers<'_> {
        let Scratch {
            catalyst,
            face_courant,
            enthalpy_courant,
            energy_delta,
            xi_out,
            solar,
            light,
            velocity,
            velocity_potential,
            velocity_potential_coarse,
            velocity_potential_stirred,
            heat_capacity,
            temperature,
            before: _,
            after: _,
        } = self;
        ScratchBuffers {
            catalyst,
            face_courant,
            enthalpy_courant,
            energy_delta,
            xi_out,
            solar,
            light,
            velocity,
            velocity_potential,
            velocity_potential_coarse,
            velocity_potential_stirred,
            heat_capacity,
            temperature,
        }
    }

    /// The same, mutably. See [`ScratchBuffersMut`].
    #[must_use]
    pub fn buffers_mut(&mut self) -> ScratchBuffersMut<'_> {
        let Scratch {
            catalyst,
            face_courant,
            enthalpy_courant,
            energy_delta,
            xi_out,
            solar,
            light,
            velocity,
            velocity_potential,
            velocity_potential_coarse,
            velocity_potential_stirred,
            heat_capacity,
            temperature,
            before: _,
            after: _,
        } = self;
        ScratchBuffersMut {
            catalyst,
            face_courant,
            enthalpy_courant,
            energy_delta,
            xi_out,
            solar,
            light,
            velocity,
            velocity_potential,
            velocity_potential_coarse,
            velocity_potential_stirred,
            heat_capacity,
            temperature,
        }
    }

    /// Fill every buffer this type owns with a non-zero pattern.
    ///
    /// The instrument of the criterion, and it exists in the library rather than
    /// in a test file because it has to be **exhaustive**: it goes through
    /// [`Scratch::buffers_mut`], whose body destructures `Scratch` with no `..`,
    /// so a buffer added there and forgotten stops both from compiling. A poison
    /// that had quietly gone short would make
    /// `poisoning_the_scratch_before_a_tick_changes_no_buffer_of_the_world`
    /// green about a buffer it never touched — the same failure ADR-086 found in
    /// the snapshot guard, in the same shape.
    ///
    /// `Q::from_f64` and never a bare cast: `Q` has no arithmetic outside its
    /// wrappers (ADR-022), and the pattern is turned into a value through the one
    /// door there is.
    pub fn poison(&mut self, pattern: u32) {
        let ScratchBuffersMut {
            catalyst,
            face_courant,
            enthalpy_courant,
            energy_delta,
            xi_out,
            solar,
            light,
            velocity,
            velocity_potential,
            velocity_potential_coarse,
            velocity_potential_stirred,
            heat_capacity,
            temperature,
        } = self.buffers_mut();

        // Non-zero in both classes, and distinct per class so that a buffer read
        // as the wrong type shows up as a value rather than as a zero. `M64` gets
        // the pattern itself; `Q` gets it as a magnitude a float can hold exactly.
        let m = M64::new(i64::from(pattern) + 1);
        let q = Q::from_f64(f64::from(pattern) + 1.0);

        for cell in energy_delta.iter_mut().chain(solar) {
            *cell = m;
        }
        for cell in xi_out {
            *cell = M32::from_i64_clamping(i64::from(pattern) + 1);
        }
        for cell in face_courant
            .iter_mut()
            .chain(catalyst)
            .chain(enthalpy_courant)
            .chain(light)
            .chain(velocity)
            .chain(velocity_potential)
            .chain(velocity_potential_coarse)
            .chain(velocity_potential_stirred)
            .chain(heat_capacity)
            .chain(temperature)
        {
            *cell = q;
        }
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
    /// **`dt` and `dx` stay separate arguments although the `Config` now arrives
    /// too**, and that is deliberate: the check `refolded != (enthalpy.substeps,
    /// enthalpy.alpha)` below catches exactly a `dt` that is not the one the
    /// scenario was derived under. Read out of the same `Config`, it would be
    /// comparing the derivation with itself — a check that passes always and
    /// looks identical. Seven parameters is exactly the `too_many_arguments`
    /// threshold (ADR-086).
    ///
    /// `&Config` and `seed` arrive because three operators need them and none of
    /// them may be folded by a caller: `React::new` wants the kinetics and the
    /// run key (ADR-058), `Temperature::new` wants `t_ref` and `c_p`, and
    /// `Fold::new` wants the light. Folding them outside and passing them in was
    /// rejected for the reason this constructor already carries — an operator is
    /// folded once before the first tick so that a refusal arrives at load rather
    /// than on tick 4 000, and fifteen call sites would have to keep that
    /// synchronous for ever.
    ///
    /// # Errors
    ///
    /// Returns an error if a folded operator refuses — a substep count over
    /// `N_MAX`, an exchange face, a Courant number over one, a `T_ref` that is
    /// not a number, an energy scale that makes one storage unit worthless — and,
    /// naming what is undecided, if the roster enables one of the seven processes
    /// this tick cannot dispatch. See the table in the module header.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        world: &World,
        derived: &Derived,
        config: &Config,
        roster: &[RosterEntry; ROSTER_LEN],
        dt: f64,
        dx: f64,
        seed: u64,
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

        // Step `f`, folded when the roster asks for it (ADR-067, ADR-085). Every
        // number comes through `Derived` and none out of the `Config` standing
        // right here, for the reason step `b` gives one screen down: a second
        // reader of `[physics]` beside the validator's agrees with it on every
        // scenario that validated and parts company on the first one that did
        // not.
        let (settle_32, settle_64) = if enabled[row(ProcessId::Settling)] {
            let Some(declared) = derived.medium() else {
                bail!(
                    "the roster enables process `{}` and the derivation carries \
                     no medium: the scenario's `[[process]]` record is absent or \
                     disabled, or `[physics]` declares no mu — and the viscosity \
                     is required when the process is on, with no default \
                     (ADR-085). A roster and a config that disagree about which \
                     processes run is not a world with a default; \
                     `config::materialise` builds the two from one another and \
                     nothing in this crate turns a `Config` into a roster",
                    ProcessId::Settling.id()
                );
            };
            let medium = settle::Medium {
                rho_medium: declared.rho_medium,
                g: declared.g,
                mu: declared.mu,
            };
            // Per lane and never per substance (ADR-056). A substance-indexed
            // array here is the quiet failure: the wrong lane sinks, matter is
            // conserved exactly, both residuals stay at zero, and the only
            // witness is which lane moved.
            let g32 = grains_by_lane(world, derived, Width::Bits32);
            let g64 = grains_by_lane(world, derived, Width::Bits64);
            (
                fold_per_width(lanes_32, |lanes| {
                    settle::SettlePhase::new_32(grid, lanes, &g32, &medium, dt, dx)
                })?,
                fold_per_width(lanes_64, |lanes| {
                    settle::SettlePhase::new_64(grid, lanes, &g64, &medium, dt, dx)
                })?,
            )
        } else {
            (None, None)
        };

        // Step `a` is folded by nobody, so step `i'` folds with `i_surface`
        // exactly `Q::ZERO`, and that is the pairing
        // `the_fold_credits_no_solar_energy_when_the_light_step_does_not_run`
        // guards. `refuse_if_blocked` above rejects an enabled light process
        // outright while the attenuator table `Light::new` wants reaches no
        // folder — the measure `conc_per_unit` multiplies is settled nowhere —
        // which is the same gap that keeps step `a` out of the dispatch. The sink
        // is no longer part of that gap: ADR-084 ratifies the `exchange` face as
        // it. The `Some` arrives here on the day step `a` builds,
        // and it has to be the *same* `Light`: `i_surface` is the one term of the
        // topmost layer's absorption which is not in the field, and two foldings
        // of one physical number put a last-bit difference into that voxel and
        // nowhere else.
        let light: Option<&light::Light> = None;
        let fold = fold::Fold::new(
            grid,
            enthalpy_grid,
            u32::from(enthalpy.lod),
            light,
            derived,
            dt,
        )?;

        // Dispatched twice a tick and folded once, unconditionally.
        //
        // **This cancels a placement consequence of ADR-079** — "the
        // recomputation stands immediately before step `h`" — because there are
        // two recomputations, one at each reader; the decision itself, two coarse
        // fields, the answer `T_ref` at a non-positive denominator, no assertion
        // and the coarse grid, stands entire.
        //
        // Unconditionally, **and that cancels a consequence of ADR-086 by the
        // record ADR-089**, not by this comment. ADR-086 folds the operator only
        // on a roster that enables the reactions or the velocity field, so that
        // `Temperature::new` cannot become a new load-time refusal; but both of
        // those processes are refused at construction, so under that condition
        // the operator would be folded on no buildable scenario, dispatched
        // never, and `the_denominator_is_fresh_for_step_b_and_for_step_h` — which
        // the same record names and which runs a diffusion-only roster — could
        // not pass. ADR-089 carries the argument and the price.
        //
        // The worry the condition was raised against is answered by a test rather
        // than by assumption, and by the right one: `the_shipped_scenario_survives_a_thousand_ticks`
        // (`liminis/src/serve.rs`) goes through `build`, which is what calls
        // `Tick::new`. `every_scenario_in_the_repository_loads` cannot answer it —
        // `config::load` is `parse` and does not fold a tick.
        let temperature = Temperature::new(
            grid,
            enthalpy_grid,
            u32::from(enthalpy.lod),
            registry,
            derived,
            config,
        )?;

        let n_reactions =
            u32::try_from(config.reaction.len()).context("more reactions than a u32 can count")?;

        // Step `b`, folded when the roster asks for it. Every number comes
        // through `Derived` and none out of the `Config` standing right here:
        // reading `[[process]]` keys again would put a second reader of the
        // process records beside the validator's, which is the thing
        // `DerivedSubstance::diffusivity` exists to prevent — the two agree on
        // every scenario that validates and part company on the first one that
        // does not.
        let velocity = if enabled[row(ProcessId::VelocityField)] {
            let Some(declared) = derived.velocity() else {
                bail!(
                    "the roster enables process `{}` and the derivation carries no \
                     velocity section: the scenario's `[[process]]` record is \
                     absent or disabled, so the four keys of ADR-069 — u_conv_max, \
                     l_c, stir_fraction, stir_period — have nowhere to come from. \
                     A roster and a config that disagree about which processes run \
                     is not a world with a default; `config::materialise` builds \
                     the two from one another and nothing in this crate turns a \
                     `Config` into a roster",
                    ProcessId::VelocityField.id()
                );
            };
            let energy = derived.energy();
            Some(velocity::VelocityField::new(
                grid,
                world.velocity_grid(),
                enthalpy_grid,
                &velocity::VelocityConfig {
                    u_conv_max: declared.u_conv_max,
                    l_c: declared.l_c,
                    stir_fraction: declared.stir_fraction,
                    stir_period: declared.stir_period,
                    dt,
                    dx,
                    t_min: energy.t_min,
                    t_max: energy.t_max,
                    // `2^k_E` and never `k_E`: the exponent is what ADR-062
                    // derives and the multiplier is what the potential divides
                    // the enthalpy by. Passed as the exponent, the convective
                    // gain is off by tens of binary orders — caught by
                    // `within_speed_bound` in debug, and in release the amounts
                    // go negative and read as the accepted undershoot of ADR-068.
                    units_per_joule: 2f64.powi(i32::from(energy.k_e)),
                    // From the roster and never the literal 1: hard-coded, the
                    // refusal inside `VelocityField::new` becomes dead code and a
                    // scenario with `every_n_ticks = 5` loads, with step `c`
                    // moving matter on four ticks out of five over a field step
                    // `b` did not update (ADR-074).
                    every_n_ticks: every_n[row(ProcessId::VelocityField)],
                },
            )?)
        } else {
            None
        };

        Ok(Tick {
            ceilings: derived
                .substances()
                .iter()
                .enumerate()
                .map(|(s, meta)| (meta.id.clone(), world.lane_of(s as u32), meta.amount_at_max))
                .collect(),
            velocity,
            advect_32,
            advect_64,
            advect_h,
            diffuse_32,
            diffuse_64,
            diffuse_h,
            settle_32,
            settle_64,
            react: fold_react(
                world,
                derived,
                config,
                seed,
                enabled[row(ProcessId::Reactions)],
            )?,
            fold,
            temperature,
            n_reactions,
            catalyst_columns: config
                .reaction
                .iter()
                .filter(|r| !r.catalyst.is_empty())
                .count() as u32,
            enabled,
            every_n,
            chemical_weight: derived.chemical_weights(),
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

    /// How many reactions the scenario declares, whatever the roster says.
    ///
    /// The length of the extent slice is `n_voxels * n_reactions`, and
    /// [`Scratch::new`] takes the second factor from here so that it does not
    /// have to read the config a second time.
    #[inline]
    #[must_use]
    pub fn n_reactions(&self) -> u32 {
        self.n_reactions
    }

    /// How many reactions of the scenario name a catalyst.
    ///
    /// Zero on every scenario that builds — `React::new` refuses a non-empty
    /// `catalyst` in `check_unimplemented_keys`, because the folding kernel of
    /// ADR-050 does not exist. It is not a column count for a buffer: neither the
    /// class of the catalysis field nor the number of columns is decided, and
    /// `Scratch::react_slices_mut` says where.
    #[inline]
    #[must_use]
    pub fn catalyst_columns(&self) -> u32 {
        self.catalyst_columns
    }

    /// The folded parameters of step `i'`.
    ///
    /// Public so that `i_surface` is checkable from a test:
    /// `the_fold_credits_no_solar_energy_when_the_light_step_does_not_run` reads
    /// it, and it is the only way that claim can be made from outside the crate.
    #[inline]
    #[must_use]
    pub fn fold_params(&self) -> crate::kernels::fold::FoldParams {
        self.fold.params()
    }

    /// Recompute the denominator and the temperature out of the state as it
    /// stands right now.
    ///
    /// Two computations from state and never one computation plus one read of
    /// something stored, which is why ADR-044 stands entire under ADR-086: what
    /// that record forbids is a **cache**, and there is none. Nothing between
    /// these two calls stores `C_cell` anywhere.
    fn recompute_temperature(&self, world: &World, scratch: &mut Scratch) {
        let (amounts_32, amounts_64) = world.amount_slices();
        // `lane(0)` and not `read()`: the enthalpy lane carries the ghost cell
        // after the coarse cells (ADR-059), and the operator walks cells of the
        // coarse grid. The front buffer and not the back one — the back holds
        // state `N+1` or whatever the last swap left there, and a temperature
        // derived from it is entirely plausible and in no invariant.
        let enthalpy = world.enthalpy().lane(0);
        let (heat_capacity, temperature) = scratch.temperature_slices_mut();
        self.temperature
            .apply(amounts_32, amounts_64, enthalpy, heat_capacity, temperature);
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
        self.assert_ceilings(world, tick);
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
        // Every buffer this tick reaches into, and one of them is the reason the
        // list is not "the two Courant buffers" any more. `xi_out` is the one
        // whose length is not `n_voxels`: a slice of `n_voxels` cells is long
        // enough for the first voxel of a one-reaction scenario, and past that
        // the per-voxel blocks overlap and the phase-5 reduction reads one
        // voxel's extent as another's (ADR-080).
        assert_eq!(
            scratch.energy_delta.len(),
            self.n_voxels as usize,
            "the reaction energy accumulator does not have the shape this tick \
             was folded for"
        );
        assert_eq!(
            scratch.xi_out.len(),
            self.n_voxels as usize * self.n_reactions as usize,
            "the extent slice holds {} cells against {} voxels times {} reactions",
            scratch.xi_out.len(),
            self.n_voxels,
            self.n_reactions
        );
        assert_eq!(
            scratch.light.len(),
            self.n_voxels as usize,
            "the light field does not have the shape this tick was folded for"
        );
        assert_eq!(
            scratch.solar.len(),
            self.n_enthalpy_cells as usize,
            "the solar slice does not have the shape this tick was folded for"
        );
        assert_eq!(
            scratch.heat_capacity.len(),
            self.n_enthalpy_cells as usize,
            "the heat capacity field does not have the shape this tick was folded \
             for"
        );
        assert_eq!(
            scratch.temperature.len(),
            self.n_enthalpy_cells as usize,
            "the temperature field does not have the shape this tick was folded \
             for"
        );
        assert_eq!(
            scratch.velocity_potential_coarse.len(),
            3 * self.n_enthalpy_cells as usize,
            "the coarse velocity potential lives on the enthalpy grid (ADR-069) \
             and does not have the shape this tick was folded for"
        );
        assert_eq!(
            scratch.velocity.len(),
            scratch.velocity_potential.len(),
            "the velocity field and its potential are the same shape"
        );
        assert_eq!(
            scratch.velocity_potential_stirred.len(),
            scratch.velocity_potential.len(),
            "the stirred potential is a copy of the interpolated one"
        );

        // Outside any `cfg`, and that is the point: the counters tick in both
        // profiles, so the snapshot the tick's increment is measured from has to
        // move in both. Folded into the `cfg` below, a release build would report
        // the whole run's total as this tick's flow (`ledger/mod.rs`).
        ledger.begin_tick();

        #[cfg(debug_assertions)]
        self.domain_sums(world, &mut scratch.before);

        // TODO(stochastic-dispatch): `run_key` now reaches step `b` and still
        // reaches nothing else. Its other consumer is `React::params(tick)`
        // (ADR-058), and step `h` is not dispatchable — see the table in the
        // module header.
        //
        // And step `b` is a consumer only on the scenarios that **stir**, which
        // is worth writing down beside the dispatch rather than discovering from
        // a green test. `VelocityField::apply` branches on `stirs` on the host
        // (ADR-069) and `run_key` enters `NoiseParams` and nothing else: at
        // `stir_fraction = 0` the convective half of `A` is a function of the
        // enthalpy and the heat capacity alone, so a perfectly dispatched step
        // `b` leaves every seed the same world. `different_seed_gives_different_state`
        // therefore goes red for a stirring fixture and for no other, and until
        // one exists a tick that forwarded `tick` and forgot `run_key` would look
        // exactly like this one.

        for &step in &STEP_ORDER {
            // The two recomputations of ADR-086, one at each reader of the
            // denominator, and they stand **outside** `runs` on purpose. Step `b`
            // reads `C_cell` and runs every tick; step `h` reads `T` and may run
            // every third. Gating the tail pass on `runs(Step::Reactions, tick)`
            // is the tempting saving of 2.94e7 mul-adds on every skipped tick and
            // is exactly the variant ADR-086 rejects: at `every_n_ticks = 3` step
            // `b` would divide the enthalpy by a denominator three ticks old,
            // computed on a composition the cell no longer has, and nothing would
            // fail — `T` and `C_cell` are class `Q` and enter no invariant.
            //
            // No tenth letter appears in `STEP_ORDER` for it: the temperature is a
            // step of the tick and not a letter of the spec (ADR-079), so both
            // calls stand in the body of this loop.
            //
            // The tail pass is guarded behaviourally by
            // `the_denominator_is_fresh_for_step_b_and_for_step_h`. The head pass
            // is guarded by nothing and cannot be: both passes write the same two
            // buffers, so deleting it leaves the whole suite green and both
            // residuals at exactly zero (`C_cell` and `T` are class `Q` and enter
            // no invariant). `the_temperature_has_a_slot_before_step_b_and_before_step_h`
            // reads the array below — which is why the array exists — and that is
            // a statement about the table this line consults, not about the tick.
            // The behavioural witness arrives with step `b`, which is the reader
            // standing between the two passes.
            if TEMPERATURE_SLOTS.contains(&step) {
                self.recompute_temperature(world, scratch);
            }
            if !self.runs(step, tick) {
                continue;
            }
            match step {
                // The three steps `Tick::new` refuses to build. Unreachable
                // rather than silently skipped: `runs` can only be true for them
                // if `refuse_if_blocked` let them through.
                Step::Light | Step::Pressure | Step::ExternalChannels => {
                    unreachable!(
                        "step {} was enabled and Tick::new did not refuse it",
                        step.letter()
                    )
                }
                Step::Velocity => {
                    // `expect` and not `unreachable!`: `runs` is true here only
                    // if the roster enabled the velocity field, and that is
                    // exactly the condition `Tick::new` folds the operator under.
                    let field = self
                        .velocity
                        .as_ref()
                        .expect("Tick::new folds step `b` for every roster that enables it");
                    // Seven slices from this owner and one from the world, and
                    // the two borrow disjointly (ADR-086). The enthalpy's
                    // **front** buffer, because it is the one input here that is
                    // state — and `lane(0)` and never `read()`, because the lane
                    // carries the ghost cell after the coarse cells (ADR-059)
                    // and the reservoir is not a coarse cell.
                    let enthalpy = world.enthalpy().lane(0);
                    let (
                        heat_capacity,
                        coarse_potential,
                        potential,
                        stirred,
                        velocity,
                        face_courant,
                        enthalpy_courant,
                    ) = scratch.velocity_slices_mut();
                    field.apply(
                        enthalpy,
                        heat_capacity,
                        coarse_potential,
                        potential,
                        stirred,
                        velocity,
                        face_courant,
                        enthalpy_courant,
                        tick,
                        run_key,
                    );
                }
                Step::Reactions => {
                    self.assert_ceilings(world, tick);
                    // `expect` and not `unreachable!`: `runs` is true here only if
                    // the roster enabled the reactions, and `fold_react` refuses
                    // that roster — so this is the same statement seen from the
                    // dispatch, and it names the lock rather than the arm.
                    let react = self
                        .react
                        .as_ref()
                        .expect("Tick::new refuses an enabled `reactions` entry it could not fold");
                    // Four slices from each owner, and the two owners borrow
                    // disjoint things — which is how `TODO(one-borrow-per-dispatch)`
                    // was closed. Nothing is copied around this call, so the class
                    // of mistake "forgot to copy the accumulator back", whose
                    // symptom is step `i'` crediting last tick's energy again, has
                    // no place left to happen (ADR-086).
                    let (energy_delta, xi_out, temperature, catalyst) = scratch.react_slices_mut();
                    let (src32, src64, dst32, dst64) = world.react_slices_mut();
                    react.gather_catalysts(src32, src64, catalyst);
                    react.apply(
                        tick,
                        src32,
                        src64,
                        dst32,
                        dst64,
                        energy_delta,
                        xi_out,
                        temperature,
                        catalyst,
                    );
                    // The process boundary of ADR-057: the front buffer holds
                    // state `N` again. Through the exhaustive door, because two
                    // separate `_mut` accessors would be two exclusive borrows of
                    // one aggregate.
                    let OwnedBuffersMut {
                        amounts_32,
                        amounts_64,
                        enthalpy: _,
                    } = world.owned_buffers_mut();
                    react.promote(amounts_32, amounts_64);
                }
                Step::EnergyFold => {
                    // `runs` answers this step with the roster row of the
                    // reactions (see `Tick::runs`), so `i'` runs if and only if
                    // `h` runs — the gate ADR-045 assigned and ADR-086 restated.
                    let (energy_delta, light, solar) = scratch.fold_slices_mut();
                    let (src_h, dst_h) = world.enthalpy_mut().pair_mut();
                    self.fold.apply(energy_delta, light, src_h, dst_h, solar);
                    world.enthalpy_mut().swap();
                    // In **both** profiles, and immediately behind its writer.
                    // `SOLAR_IN` has exactly one writer, and crediting it anywhere
                    // else would be a second reduction with a rounding of its own
                    // (ADR-075). Above the LEDGER phase, like every other credit.
                    react::credit_solar(scratch.solar(), ledger);
                }
                Step::Advection => {
                    // Both Courant buffers come from step `b` of this same tick
                    // (ADR-086, ADR-087), and on a roster that leaves the
                    // velocity field off both are zeros — one application of a
                    // zero flux and not a skipped step, because the parity of a
                    // lane has to come from the applications the phase *ran*
                    // (ADR-057). See "The Courant buffers" in the module header.
                    let courant = &scratch.face_courant;
                    if let (Some(phase), Some(field)) = (&self.advect_32, world.amounts_32_mut()) {
                        phase.apply_32(
                            field,
                            courant,
                            &self.substance_of_lane_32,
                            &self.chemical_weight,
                            ledger,
                        );
                    }
                    if let (Some(phase), Some(field)) = (&self.advect_64, world.amounts_64_mut()) {
                        phase.apply_64(
                            field,
                            courant,
                            &self.substance_of_lane_64,
                            &self.chemical_weight,
                            ledger,
                        );
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
                Step::Settling => {
                    // One application over the full step and no substeps:
                    // settling is a factor of the splitting in its own right, so
                    // it gets by on its own condition, and a grain that would
                    // need more than one voxel a tick is refused at load rather
                    // than divided into pieces (ADR-036, ADR-067).
                    //
                    // No credit to any channel and no enthalpy: the process
                    // conserves matter by construction — a face is one integer
                    // applied to both sides with opposite signs — and carries no
                    // energy, which is not an exception but the rule for every
                    // transport process here (ADR-062, ADR-067). What crosses an
                    // `exchange` face is not handled at all and cannot be
                    // reached: `settle::periodic_mask` refuses such a face when
                    // the phase is folded, which is `TODO(channels)` in
                    // `process/settle.rs` seen from this side.
                    if let (Some(phase), Some(field)) = (&self.settle_32, world.amounts_32_mut()) {
                        phase.apply_32(field);
                    }
                    if let (Some(phase), Some(field)) = (&self.settle_64, world.amounts_64_mut()) {
                        phase.apply_64(field);
                    }
                }
                Step::Diffusion => {
                    if let (Some(phase), Some(field)) = (&self.diffuse_32, world.amounts_32_mut()) {
                        phase.apply_32(
                            field,
                            &self.substance_of_lane_32,
                            &self.chemical_weight,
                            ledger,
                        );
                    }
                    if let (Some(phase), Some(field)) = (&self.diffuse_64, world.amounts_64_mut()) {
                        phase.apply_64(
                            field,
                            &self.substance_of_lane_64,
                            &self.chemical_weight,
                            ledger,
                        );
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
        //
        // The reduction of the extent belongs here too, and it is the one thing
        // in this phase that is *not* a channel (ADR-080). ADR-041 puts it here
        // in as many words — "the reaction kernel does not touch the channel
        // counters ... the sums for the ledger are taken by reduction in the
        // LEDGER phase" — and the rule transcribed at the top of this file,
        // "every credit happens above the LEDGER phase", is about channels and
        // does not reach it.
        //
        // Structurally, and this is the half a comment has to carry: the
        // reduction runs **if and only if** step `h` ran in this tick,
        // exactly as `i'` is dispatched together with `h` and never apart from it
        // (ADR-045). Today it runs in no tick — `Tick::new` refuses an
        // enabled `reactions` — so there is no reduction to run and no
        // stoichiometry to multiply by, and `Nu::EMPTY` is what the other eight
        // processes of the roster report. The day step `h` dispatches, three
        // lines arrive in one edit: the buffer, `Ledger::reduce_extent` over it
        // immediately above this block, and `React::nu()` in place of the
        // constant below. Forgetting the middle one is not silent — the extent
        // stays zero while the field moved, and the residual breaks by the whole
        // of what the chemistry did; forgetting the last one is not silent
        // either, because `residual_matter` refuses a reduced extent it has no
        // vector for.
        self.assert_ceilings(world, tick.wrapping_add(1));
        let nu = self.reaction_nu(tick);
        if self.n_reactions > 0 && self.runs(Step::Reactions, tick) {
            ledger.reduce_extent(scratch.xi_out(), self.n_voxels);
        }
        #[cfg(debug_assertions)]
        {
            // The reduction of the extent, and it runs **if and only if step `h`
            // ran in this tick** — the same condition step `h` itself is under
            // and not the weaker "was step `h` folded". The two differ by
            // `every_n_ticks`, which ADR-086 leaves the reactions ("реакциям и
            // адвекции расписание остаётся"), and the difference is not academic:
            // `xi_out` is never cleared (ADR-045, and the field's own comment
            // says why), so on a skipped tick a reduction would credit the
            // previous dispatch's extents into `Xi_r` while no amount moved, and
            // `assert_closed` would break the matter residual naming a reaction
            // that did not run. `Ledger::begin_tick` zeroes `Xi_r`, so the
            // skipped tick reduces nothing and reports `Nu::EMPTY` — which is
            // consistent, because `residual_matter` refuses a reduced extent it
            // has no vector for and there is none to refuse.
            self.domain_sums(world, &mut scratch.after);
            ledger.assert_closed(nu, &scratch.before, &scratch.after);
        }
        #[cfg(not(debug_assertions))]
        let _ = nu;
    }

    pub fn reaction_nu(&self, tick: u32) -> Nu<'_> {
        match &self.react {
            Some(react) if self.runs(Step::Reactions, tick) => react.nu(),
            _ => Nu::EMPTY,
        }
    }

    fn assert_ceilings(&self, world: &World, tick: u32) {
        for (id, lane, ceiling) in &self.ceilings {
            let invalid = match *lane {
                LaneRef::Narrow(lane) => world.amounts_32().expect("narrow field").lane(lane)
                    [..self.n_voxels as usize]
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (i, i128::from(n.to_i64())))
                    .find(|(_, n)| *n < 0 || *n > *ceiling),
                LaneRef::Wide(lane) => world.amounts_64().expect("wide field").lane(lane)
                    [..self.n_voxels as usize]
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (i, i128::from(n.to_i64())))
                    .find(|(_, n)| *n < 0 || *n > *ceiling),
            };
            if let Some((idx, amount)) = invalid {
                assert!(
                    amount >= 0,
                    "substance `{id}` is negative at tick {tick}, voxel {:?}: {amount}",
                    world.grid().coords(idx as u32)
                );
                assert!(
                    amount <= *ceiling,
                    "substance `{id}` exceeds declared ceiling at tick {tick}, voxel {:?}: {amount} > {ceiling}",
                    world.grid().coords(idx as u32)
                );
            }
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

        // The chemical energy of everything the domain holds, and it goes
        // **last** (ADR-081). It weighs the `matter` accumulators the doors above
        // filled, so one line earlier it would weigh a half-filled table — and no
        // residual can see that, because the same short sum taken before and
        // after cancels in `after - before` and stays zero for ever. The same
        // class as "a lane where a substance was meant", two paragraphs up, and
        // the same kind of witness: only an absolute number can fail, which is
        // `load_reports_the_chemical_energy_of_the_domain_in_joules` on the
        // loader's side.
        out.add_chemical_energy(&self.chemical_weight);
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

/// Fold step `h`, or say why it cannot be folded.
///
/// Build the common chemistry operator when the roster enables reactions.
fn fold_react(
    world: &World,
    derived: &Derived,
    config: &Config,
    seed: u64,
    enabled: bool,
) -> Result<Option<React>> {
    if !enabled {
        return Ok(None);
    }
    React::new(
        &react::ReactShape {
            grid: world.grid(),
            enthalpy_grid: world.enthalpy_grid(),
            enthalpy_lod: u32::from(derived.enthalpy_field().lod),
        },
        world.registry(),
        derived,
        config,
        seed,
    )
    .map(Some)
}

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

/// The grain of every lane of one width class, in lane order.
///
/// The mirror of [`diffusivity_by_lane`], and it exists for the same reason
/// (ADR-056): the index is a **lane** and never a substance. Water is first in
/// the registry of SPEC section 2.3 and takes lane 0, so a substance-indexed
/// array is right at `s == 0` and off by one from there on — and
/// `SettlePhase::fold` checks only the length, so the wrong lane would sink with
/// matter conserved exactly, both residuals at zero and every "alone" test green.
///
/// A lane no substance of this width class occupies keeps a grain of zero
/// radius, which `settling_velocity` answers with `w = 0` and the phase does not
/// dispatch at all.
fn grains_by_lane(world: &World, derived: &Derived, width: Width) -> Vec<settle::Grain> {
    let lanes = world.registry().lanes(width) as usize;
    let mut by_lane = vec![
        settle::Grain {
            settling_radius: 0.0,
            molar_mass: 0.0,
            partial_molar_volume: 0.0,
        };
        lanes
    ];
    for (s, substance) in derived.substances().iter().enumerate() {
        // `s` is the substance index the derivation and the registry share: both
        // are built in declaration order and `Tick::new` has just checked that
        // they are the same length.
        let s = s as u32;
        match (world.lane_of(s), width) {
            (LaneRef::Narrow(lane), Width::Bits32) | (LaneRef::Wide(lane), Width::Bits64) => {
                by_lane[lane as usize] = settle::Grain {
                    settling_radius: substance.settling_radius,
                    molar_mass: substance.molar_mass,
                    partial_molar_volume: substance.partial_molar_volume,
                };
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
        // Rewritten and not deleted, and both halves of that matter. The two
        // locks this message used to name are gone — ADR-084 ratifies the built
        // `exchange` face as the energy sink of S0, ADR-083 answers the counter
        // width — so a message that went on naming them would send the next
        // author hunting holes that are not there. What is left is a different
        // lock and a real one, and **deleting the arm because the old reasons
        // expired would be the quiet disaster this function exists to prevent**:
        // `Light::new` takes `&[Attenuator]`, an attenuator carries
        // `conc_per_unit`, and what that multiplies is settled by no document
        // (`TODO(attenuation-measure)` in `process/light.rs`; `CONFIG_SCHEMA.md`
        // says outright that `k_w`…`k_m` reach no table). `Derived` therefore
        // carries no such table, so the only way to build a tick would be
        // `Light::new(grid, &[], ..)` — a lit world that absorbs nothing, with
        // step `i'` dispatched alongside step `h` and so not running at all,
        // `SOLAR_IN` never credited, and both residuals zero. A world that looks
        // configured and is not, green all the way down and without a panic.
        ProcessId::Light => bail!(
            "process `{}` is enabled and step `a` cannot be dispatched, and what \
             blocks it is neither of the two locks it used to be. ADR-084 \
             ratifies the sink: absorbed energy leaves through the built \
             `exchange` face, credited to BOUNDARY_EXCHANGE by steps `c` and `d`, \
             and `config/validate.rs` now refuses a lit scenario only for a \
             sealed box or a steady state past the declared range. ADR-083 \
             answers the width: the counter is i128 and holds \
             2^127/2^k_E = 1.15e18 J at k_E = 67, against 0.16384 J for one lit \
             tick of a 128^3 domain. What is left is the **attenuation measure**: \
             `Light::new` wants a table of attenuators, an attenuator is a `k` per \
             unit of some dimensionless measure of storage, and which measure that \
             is no document decides (`TODO(attenuation-measure)` in \
             `process/light.rs`, A-22 in `OPEN_QUESTIONS.md`) — so `Derived` \
             carries no such table and there is nothing to build the operator \
             from. Building it with an empty table instead would give a lit world \
             that absorbs nothing and reports no error. Its default is \
             enabled = {} (`process/light.rs`, ADR-076)",
            id.id(),
            light::ENABLED_BY_DEFAULT
        ),
        // Step `b` dispatches (ADR-087). The arm is **deleted** and not
        // rewritten, which is the other of the two shapes this function takes:
        // the reactions keep an arm because one lock of theirs is still shut,
        // and the velocity field has none left. The four keys of ADR-069 reach
        // `Tick::new` through `Derived::velocity`, and the fold onto the faces
        // of the enthalpy grid is the fifth stage of the operator, so
        // `Scratch::enthalpy_courant` is written by the same dispatch that
        // writes `Scratch::face_courant` — matter and heat carried by one field.
        ProcessId::VelocityField => Ok(()),
        // Rewritten and not deleted, and the rewrite is the point. ADR-082
        // declares the limiting overflow, so the lock this message used to name is
        // lifted — and a refusal that goes on naming a lifted lock sends the next
        // author hunting a hole that is not there, which is exactly what ADR-081
        // forced to be rewritten in the reactions arm below. Five locks are left
        // and the message names all five, because the reader's next move is to go
        // and shut one.
        //
        // The key's spelling is deliberately absent from the text. The `light` arm
        // above does name `i_surface`, and that is the shape this one used to
        // copy; here the name is left out so that
        // `an_enabled_pressure_scenario_is_refused_until_step_e_has_an_owner_and_a_boundary`
        // can assert the absence mechanically rather than by reading the sentence
        // it sits in. Whoever adds it back will find out which of the two
        // properties they cared about.
        ProcessId::Pressure => bail!(
            "process `{}` is enabled and step `e` cannot be dispatched, and what \
             blocks it is no longer a missing number: ADR-082 makes the limiting \
             overflow a required key of the pressure record, with a floor of \
             6*Theta_sup and a ceiling from the declared run horizon, and \
             `Derived::pressure` carries it. Five things are left. (1) This \
             dispatch: `Tick::run` holds `Step::Pressure` in an unreachable arm. \
             (2) The overflow field has no owner — nothing allocates it — and \
             `Derived` carries neither partial_molar_volume nor an Occupant \
             table, so the occupancy table cannot be assembled at all. (3) What \
             pressure does at an `exchange` face is decided by nothing, and \
             `periodic_mask` in `process/pressure.rs` refuses one outright while \
             the shipped scenario declares z_max = \"exchange\". (4) Whether the \
             field is one-sided is undecided (`TODO(one-sided)`), and on four \
             substances with no solvent the one-sided reading is identically \
             zero. (5) `energy: Conserved` on this step rests on the absence of a \
             number rather than on one (`TODO(enthalpy-of-transport)`). Its \
             default is enabled = {} (`process/pressure.rs`, ADR-082)",
            id.id(),
            pressure::ENABLED_BY_DEFAULT
        ),
        // Step `f` dispatches (ADR-085). The arm is **deleted** and not
        // rewritten, which is the shape the velocity field took one screen up
        // and the opposite of the shape the light and pressure keep: `Medium`
        // wanted `g` and `rho_medium`, `[physics]` declares both with defaults,
        // and `Derived::medium` carries them to `Tick::new` through the door
        // `DerivedSubstance::diffusivity` goes through. The grain itself arrives
        // the same way — `settling_radius`, `molar_mass` and
        // `partial_molar_volume` are fields of a derived substance now — so
        // there is nothing left for this arm to name.
        //
        // Deleting it and leaving `Step::Settling` in the unreachable arm of
        // `Tick::advance` is the half-lift that panics on the first tick;
        // keeping it and writing the dispatch is the half-lift that is silent —
        // dead code, and an enabled settling process refused by a message that
        // has become false.
        ProcessId::Settling => Ok(()),
        ProcessId::PhaseTransitions => bail!(
            "process `{}` is enabled and line `g` has no operator at all: it is \
             one of the nine records ADR-065 materialises and it appears in no \
             step of the tick order. Enabling it changes `config_hash` and \
             nothing else, which is a world that looks configured and is not. Its \
             default is enabled = {} (`process/phase.rs`)",
            id.id(),
            super::phase::ENABLED_BY_DEFAULT
        ),
        // Rewritten and not deleted, for the **second** time. The first rewrite
        // cancelled a consequence of ADR-081; this one cancels a consequence of
        // ADR-088 — the part asking for the ceiling guard and the flipped flag in
        // the *same commit* as the last lock of step `h`. The decision of ADR-088
        // stands entire: two preconditions, both still owed, and
        // `react::ENABLED_BY_DEFAULT` is not flipped here (ADR-090).
        //
        // What is guarded is the *content* of the refusal and not the fact of it.
        // A message naming a lock that has been lifted sends the next author
        // looking for it — and a message that names the locks short sends them
        // the same way, only worse, because it goes on firing and goes on looking
        // exhaustive. Whoever writes the ceiling guard would read "one thing
        // left", delete this arm, flip the flag, and make `Nu::EMPTY` a lie in
        // three places of `serve.rs` while phase 5 stays silent in release. So
        // both remaining pieces of work are named below, and the difference
        // between them is said out loud rather than left to be inferred.
        ProcessId::Reactions => Ok(()),
        ProcessId::ExternalChannels => bail!(
            "process `{}` is enabled and step `j` has nothing to write: there are \
             no events — `IMPACT` and `VENT_BURST` are described by no schedule, \
             key or type anywhere. `BOUNDARY_EXCHANGE` is not among the blockers \
             and never was this step's to write: steps `c` and `d` credit it on \
             every substep (ADR-059). The temperature field is no longer among \
             them either, but the two heat channels ADR-059 does assign here \
             are: \
             `RADIATIVE_OUT` wants an emissivity nothing declares, and \
             `GEOTHERMAL_IN` a heat flux and a vent composition — one blocker and \
             not two, because it is one channel and one record lifts both. That \
             makes three, where there used to be four: the sign of a counter left \
             the list with ADR-084, which ratifies it as the increment of the \
             domain. The shape of the radiative boundary is A-25. Its default is \
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
    fn the_temperature_has_a_slot_before_step_b_and_before_step_h() {
        // **Named for what it checks and not for what the tick does**, and the
        // difference is the whole reason for the name. What is checked is
        // `TEMPERATURE_SLOTS`: two slots, at `Step::Velocity` and
        // `Step::Reactions`, in the order `STEP_ORDER` walks them. What is *not*
        // checked, here or anywhere, is that `Tick::advance` recomputes at each
        // of them — both passes write the same two buffers, so a tick with the
        // head pass deleted leaves this test, every acceptance test and both
        // residuals exactly as they are. Calling this test the witness of the
        // head pass would be the failure ADR-086 opens on: a reader told to stop
        // checking by a name.
        //
        // The array is worth reading all the same, and for the reason
        // `the_step_order_is_the_one_spec_section_8_prints` reads `STEP_ORDER`: a
        // permutation or a missing entry in a table the loop consults is
        // invisible to everything else in the repository.
        let slots: Vec<&str> = STEP_ORDER
            .iter()
            .filter(|step| TEMPERATURE_SLOTS.contains(step))
            .map(|step| step.letter())
            .collect();
        assert_eq!(
            slots,
            ["b", "h"],
            "the two recomputations stand immediately before step `b` and \
             immediately before step `h`, one at each reader of the denominator"
        );
        assert_eq!(
            TEMPERATURE_SLOTS.len(),
            slots.len(),
            "a slot names a step the tick does not walk"
        );

        // And the gate they must **not** be under. Tying the tail pass to
        // `runs(Step::Reactions, tick)` looks like a saving of 2.94e7 mul-adds on
        // every skipped tick and is the variant ADR-086 rejects by name: at
        // `every_n_ticks = 3` on the reactions, step `b` would divide the
        // enthalpy by a denominator three ticks old. The source is what carries
        // that, and this line is the reminder beside it.
        assert_eq!(Step::Velocity.process(), Some(ProcessId::VelocityField));
        assert_eq!(Step::Reactions.process(), Some(ProcessId::Reactions));
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
