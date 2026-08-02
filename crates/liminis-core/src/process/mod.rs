//! Orchestration: who owns the buffers, how many substeps a field needs, in
//! what order the tick runs.
//!
//! The other half of the boundary of `ARCHITECTURE.md`. A kernel knows nothing
//! about config, time, or who called it; everything it does not know lives
//! here, and none of it ever travels into WGSL. The arrow points one way — a
//! process calls a kernel, a kernel does not know a process exists (ADR-034).
//!
//! # What a process owes
//!
//! SPEC section 4.1 gives the contract: `id`, `reads`, `writes`, `lod`,
//! `every_n_ticks`, `invariant`, `enabled`. Two of those are load-bearing here
//! and the rest are bookkeeping.
//!
//! **The invariant, on two axes.** Every process declares what it does to
//! matter and, separately, what it does to energy: conserves it, or moves it
//! through a named channel (ADR-003, ADR-028). One axis would not be enough —
//! light conserves matter while crediting energy through `SOLAR_IN`, and the
//! upkeep of an expression channel debits energy while conserving matter. See
//! [`Invariant`].
//!
//! **The order of processes is semantics, not implementation.** Splitting is
//! Lie-Trotter in the order of SPEC section 8 (ADR-036), so swapping two
//! processes changes the world and has to move `WORLD_FORMAT_VERSION`
//! (ADR-020), even though no config and no kernel changed.
//!
//! # The order, and the roster
//!
//! Both live under `process/` and neither is left to a caller. The splitting
//! order is [`tick::STEP_ORDER`] — a named constant with a test that spells out
//! the letters SPEC section 8 prints, because a permutation of a sequence of
//! calls is invisible to every other test in the repository. The list of
//! processes is [`ProcessId`], closed, nine long, with each default of `enabled`
//! declared by the process's own module and materialised into every scenario
//! before hashing (ADR-065).
//!
//! Both belong here rather than in `config/` for the same mechanical reason:
//! `process/**` is under the CI guard of ADR-020 and `config/**` is not, so a
//! default or an order living there could change the semantics of every run
//! without moving `WORLD_FORMAT_VERSION`.
//!
//! **`b` and `e` are separate steps and their velocities are never summed.**
//! ADR-069 forbids the sum on three independent grounds; the first is that
//! ADR-055 derives the pressure mobility so that step `e` already spends the whole
//! Courant budget of its own step. Under splitting each operator gets by on its
//! own condition, and the displacement over a tick is a sum of three separate
//! terms — 0.167 from advection, at most one from pressure, at most one from
//! settling — rather than one shared budget.
//!
//! # What is not here
//!
//! Every file here holds an operator or, for the two lines of SPEC section 8
//! that have none, the two things a roster entry owes — `phase.rs` and
//! `channels.rs`. What is left in this section is the trait none of them
//! implements.
//!
//! No `Process` trait, and not for lack of a decision about its shape — ADR-034
//! settled that. It is missing three types, and each of them is blocked on
//! something that has to be decided before it can be written:
//!
//! - `FieldRef`, for `reads` and `writes`. A field reference is a substance and
//!   a buffer address; `world::World::lane_of` now resolves that (ADR-056), and
//!   what is still missing is the loader that would say which fields a process
//!   named in TOML actually reads.
//! - The second arm of [`Conservation`]. It names a channel, and `ledger/` now
//!   has the channel registry — but `process/**` is under the guard of ADR-020,
//!   and that arm is not what this wave moved the version for.
//! - The `world` argument of `apply`. `world::World` exists as of ADR-057, and
//!   nothing constructs one outside tests: the loader that would derive the
//!   widths, the scales and the substep counts a `World` is built from does not
//!   build one yet.
//!
//! Writing the trait against invented versions of those three would put a
//! second, silent addressing scheme underneath every process. So for now a
//! process is a plain type with an `apply` that takes exactly the fields it
//! touches — which is the half of ADR-034 that matters anyway: a process cannot
//! reach a buffer it did not name.
//!
//! # One file here is not a process
//!
//! [`temperature`] folds an operator and has **no** entry in [`ProcessId`], and
//! that is deliberate rather than pending. ADR-065 closed the roster at nine and
//! counted them out one line at a time; a tenth record would be materialised into
//! every scenario and would move `config_hash` for every config that exists, so
//! adding one is an overturning of that record and not a convenience. The
//! precedent is already in the tick: step `i'`, the energy fold, is a step of
//! [`tick::STEP_ORDER`] with no roster entry, which ADR-065 keeps out on purpose
//! because it belongs to the energy path of the reactions.
//!
//! The temperature operator is the same kind of thing seen from the other side.
//! It has no `enabled`, because a run with the chemistry on and the temperature
//! off is not a cheaper world but an undefined one — `T` is an input of the Q10
//! factor of every reaction (ADR-048) — and it has no `every_n_ticks`, because
//! ADR-044 says the denominator is recomputed rather than cached and ADR-062
//! prices that at exactly one recomputation per tick.
//! `the_temperature_operator_is_not_a_roster_process` holds the count at nine.

// Public modules rather than one private module per process re-exported here,
// for the reason `kernels/mod.rs` gives: these files are written in parallel,
// and a shared re-export list is the one kind of conflict that overwrites
// instead of announcing itself. The path also earns its keep at the call site —
// every process has an `apply`, and `process::react::apply` says which.
pub mod advect;
pub mod channels;
pub mod diffuse;
pub mod light;
pub mod phase;
pub mod pressure;
pub mod react;
pub mod settle;
pub mod temperature;
pub mod tick;
pub mod velocity;

pub use advect::{Advect, AdvectPhase};
pub use diffuse::{Diffuse, DiffusePhase, N_MAX, substeps_and_alpha, substeps_for};
pub use settle::{Grain, Medium, Settle, SettlePhase};
pub use temperature::Temperature;
pub use tick::{STEP_ORDER, Scratch, Step, Tick};

/// A coarse grid is the fine one at the declared `lod` — checked once, for every
/// caller that hands a kernel both.
///
/// The check exists because the mistake it catches is a **neighbouring** cell
/// rather than an out-of-range one. A world has two coarse grids and they differ:
/// the enthalpy field is `32^3` and the prescribed velocity field is `64^3` over
/// the same `128^3` base (ADR-062, ADR-069). Read with the extents of the wrong
/// one, the per-axis shift of SPEC section 1.5 lands on a plausible cell — and
/// what travels on that grid is temperature, which is class `Q` and enters no
/// invariant at all, so no residual can ever be nonzero because of it.
///
/// One copy and not one per caller. It was `check_shape` inside
/// `process/react.rs` when the reaction step was the only consumer; the
/// temperature operator is the second, and two copies of a refusal drift in the
/// direction that matters — one of them loosens, and the message that stops
/// saying "enthalpy" is the one nobody reads.
///
/// `role` names the field whose grid this is, so that a reader who meets the
/// refusal knows which of the two is at fault.
///
/// # Errors
///
/// Returns an error naming both shapes and the `lod` if they do not agree, and if
/// `lod` is past the width of a `u32` index.
pub(crate) fn coarse_shape_agrees(
    fine: &crate::world::Grid,
    coarse: &crate::world::Grid,
    lod: u32,
    role: &str,
) -> anyhow::Result<()> {
    if lod >= u32::BITS {
        anyhow::bail!(
            "a lod of {lod} shifts a u32 extent out of existence; the declared \
             range is 0..=2 (QUANTITIES.md section 1)"
        );
    }
    let expected = (fine.nx() >> lod, fine.ny() >> lod, fine.nz() >> lod);
    let got = (coarse.nx(), coarse.ny(), coarse.nz());
    if expected != got {
        anyhow::bail!(
            "the coarse grid given for {role} is {}x{}x{} while the fine grid \
             {}x{}x{} at lod {lod} covers {}x{}x{}. Temperature is a quantity of \
             the covering *enthalpy* cell (ADR-062), and the extents of the \
             velocity grid — the other coarse grid of a world — give a plausible \
             neighbouring cell instead, which appears in no invariant at all",
            got.0,
            got.1,
            got.2,
            fine.nx(),
            fine.ny(),
            fine.nz(),
            expected.0,
            expected.1,
            expected.2
        );
    }
    Ok(())
}

/// What a process does to one of the two ledgers over one tick.
///
/// The pair of enums of SPEC section 4.1, one arm short. `Conserved` means the
/// process moves the quantity around and the sum over the domain comes out bit
/// for bit equal — not "equal to within a tolerance": ADR-003 makes the per-tick
/// residual an exact integer comparison, and transport conserves by construction
/// rather than by accuracy (ADR-005).
///
/// The second arm names a channel, and it is what a process that vents declares
/// (ADR-028, ADR-059). Nothing in the corpus compares the declaration against
/// the behaviour, so both mistakes are quiet — but they are not equally bad.
/// Over-declaring costs a reader's trust; **under**-declaring, `Conserved` on a
/// grid whose lid trades with the reservoir, makes the promise of closed
/// accounting false while every test in the project is green. That is why the
/// transport processes take the arm from `Grid::has_exchange` rather than from a
/// constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Conservation {
    /// The quantity is neither created nor destroyed by this process.
    Conserved,
    /// The quantity crosses the boundary of the domain, and every unit of it is
    /// counted into this channel (ADR-003, ADR-059).
    ChangedThrough(crate::ledger::Channel),
}

/// The invariant of a process: what it does to matter, and separately what it
/// does to energy (ADR-028, SPEC section 4.1).
///
/// Two axes rather than one, because a process that conserves matter while
/// moving energy is inexpressible with a single answer, and the spec has at
/// least two of those.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Invariant {
    /// What happens to the amounts of substance. "Matter", not "mass": the unit
    /// of account has been molar since ADR-026.
    pub matter: Conservation,
    /// What happens to energy — enthalpy in the fields, and whatever the cells
    /// hold (ADR-028).
    pub energy: Conservation,
}

/// How many processes the roster of S0 holds. Nine, and closed (ADR-065).
///
/// The record counts them out: of the thirteen lines of step 0 of SPEC section 8
/// eleven can be switched off and leave the world defined, guild trait selection
/// (`i`) arrives in S1 and geology (`k`) in S4, cell sorting (`l`) turns
/// determinism off rather than a process, and the energy fold (`i'`) is part of
/// the energy path of the reactions rather than a process of its own (ADR-045).
/// "Nine records are materialised in S0."
///
/// Eight will not do, and the entry that makes it nine is the surprising one:
/// phase transitions, line `g`, which has no operator at all. See
/// [`phase`](crate::process::phase).
pub const ROSTER_LEN: usize = 9;

/// How often a process runs when the scenario does not say. Every tick.
///
/// The schema carries this default too (`config/schema.rs`), and the two are not
/// a duplicate of the kind ADR-065 warns about: that record moves `enabled` out
/// of `config/` because a default there could change the semantics of every run
/// without moving `WORLD_FORMAT_VERSION`. `every_n_ticks` cannot — ADR-030
/// forbids `> 1` on the process that moves diffusive fields, and both the
/// validator and [`tick::Tick::new`] refuse it — so the value is stated here for
/// the roster to build on and there for serde to fill in, and
/// `the_default_every_n_ticks_is_one_on_both_sides` holds them together.
pub const DEFAULT_EVERY_N_TICKS: u32 = 1;

/// The closed registry of processes (ADR-065).
///
/// In the order of SPEC section 8, which is also the order of the Lie-Trotter
/// splitting (ADR-036) and the order the canonical form prints. Reordering the
/// variants therefore does three things at once: it moves `config_hash` for
/// every scenario, it changes the world if [`tick::STEP_ORDER`] is rebuilt from
/// it, and it renumbers nothing else — so it is guarded the same way the tick
/// order is, by `the_roster_is_the_nine_of_adr_065` and by the guard of ADR-020
/// over `process/**`.
///
/// # Not a list of steps
///
/// One of the nine has no step in [`tick::STEP_ORDER`]:
/// [`PhaseTransitions`](ProcessId::PhaseTransitions), line `g`, which has no
/// operator at all. [`ExternalChannels`](ProcessId::ExternalChannels) is not the
/// second — it has step `j` and nothing to write yet, which is the ordinary case
/// of a step whose operator is blocked and which `tick::Tick::new` refuses when a
/// roster enables it. And the tick has one step that is no process: the energy
/// fold of `i'`, which ADR-065 keeps out of the roster on purpose. So the two
/// lists are the same length by coincidence — nine each, one entry apiece that
/// the other does not carry — and are not the same list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProcessId {
    /// Step `a`: the light field, swept down Z.
    Light,
    /// Step `b`: the prescribed velocity field (ADR-069).
    VelocityField,
    /// Step `c`: matter carried by the field step `b` built.
    Advection,
    /// Step `d`: explicit diffusion, `n` substeps per field.
    Diffusion,
    /// Step `e`: pressure and displacement (ADR-055).
    Pressure,
    /// Step `f`: settling and rising (ADR-067).
    Settling,
    /// Step `g`: phase transitions, which have no operator. See
    /// [`phase`](crate::process::phase).
    PhaseTransitions,
    /// Step `h`: the whole chemistry in one call (ADR-050).
    Reactions,
    /// Step `j`: external channels and events. See
    /// [`channels`](crate::process::channels).
    ExternalChannels,
}

impl ProcessId {
    /// Every process, in the order of SPEC section 8.
    pub const ALL: [ProcessId; ROSTER_LEN] = [
        ProcessId::Light,
        ProcessId::VelocityField,
        ProcessId::Advection,
        ProcessId::Diffusion,
        ProcessId::Pressure,
        ProcessId::Settling,
        ProcessId::PhaseTransitions,
        ProcessId::Reactions,
        ProcessId::ExternalChannels,
    ];

    /// The spelling a scenario writes in `[[process]] id = "…"`.
    ///
    /// The only place these strings exist. They reach `config_hash` through the
    /// canonical form (ADR-065), so renaming one here shifts the identity of
    /// every scenario in existence — which is correct, and which is why it
    /// happens under the guard of ADR-020 rather than in `config/`.
    // TODO(CONFIG_SCHEMA.md section 13 item 23): the corpus names four of these
    // nine and none of them as a decision. Section 12 prints `diffusion`,
    // `advection`, `pressure` and `velocity_field` in a worked example, and
    // `config/validate.rs` already takes the fourth as "a constant with a TODO,
    // not as knowledge". Section 12 also prints `reactions_abiotic`, and that one
    // is not merely unassigned but *contradicted*: ADR-050 makes step `h` one
    // call over the whole chemistry, so a name that says "abiotic" describes a
    // split the journal rejected. The remaining four — light, settling, phase
    // transitions, external channels — are named by nobody at all. Every one of
    // them is a decision that moves the hash of every scenario, so it belongs in
    // `DECISIONS.md` together with the seven undecided `enabled` defaults.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            ProcessId::Light => "light",
            ProcessId::VelocityField => "velocity_field",
            ProcessId::Advection => "advection",
            ProcessId::Diffusion => "diffusion",
            ProcessId::Pressure => "pressure",
            ProcessId::Settling => "settling",
            ProcessId::PhaseTransitions => "phase_transitions",
            ProcessId::Reactions => "reactions",
            ProcessId::ExternalChannels => "external_channels",
        }
    }

    /// The process with this exact spelling, or `None`.
    ///
    /// The loader's door, and the reason an unknown `id` is a refusal rather
    /// than a shrug: `deny_unknown_fields` sees struct fields and `id` is a
    /// value, so serde cannot catch a typo here (ADR-065). Exact match and no
    /// case folding, on the precedent of `ledger::Channel::from_name`.
    #[must_use]
    pub fn from_id(id: &str) -> Option<ProcessId> {
        ProcessId::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The process's own default of `enabled`.
    ///
    /// **Delegated, never held here.** ADR-065 puts the default beside the
    /// invariant of the process, and a second copy in this match would be two
    /// constants of one meaning: they drift, and the direction that drifts
    /// silently is the one `config/validate.rs` already writes out over
    /// `VELOCITY_FIELD_ENABLED_BY_DEFAULT` — a validator reading an enabled
    /// process as disabled, and a config loading with an advection speed nobody
    /// checked. `every_roster_default_comes_from_its_own_module` is what holds
    /// the delegation in place; without it the two forms are one edit apart.
    #[must_use]
    pub const fn enabled_by_default(self) -> bool {
        match self {
            ProcessId::Light => light::ENABLED_BY_DEFAULT,
            ProcessId::VelocityField => velocity::VELOCITY_FIELD_ENABLED_BY_DEFAULT,
            ProcessId::Advection => advect::ENABLED_BY_DEFAULT,
            ProcessId::Diffusion => diffuse::ENABLED_BY_DEFAULT,
            ProcessId::Pressure => pressure::ENABLED_BY_DEFAULT,
            ProcessId::Settling => settle::ENABLED_BY_DEFAULT,
            ProcessId::PhaseTransitions => phase::ENABLED_BY_DEFAULT,
            ProcessId::Reactions => react::ENABLED_BY_DEFAULT,
            ProcessId::ExternalChannels => channels::ENABLED_BY_DEFAULT,
        }
    }
}

/// One materialised roster record: the three keys of `[[process]]` that the
/// simulator reads.
///
/// This is what the loader hands the tick after it has filled in what the
/// scenario left out (ADR-065). The parameters of a process — `u_conv_max`,
/// `k_w`, `mu` — are not here: they are read by whoever folds that process, and
/// duplicating them into a roster record would give every one of them a second
/// home.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RosterEntry {
    /// Which process.
    pub id: ProcessId,
    /// Whether it runs at all.
    pub enabled: bool,
    /// How many ticks between two applications. `1` is every tick; `> 1` is
    /// refused for [`Diffusion`](ProcessId::Diffusion), which is what ADR-030's
    /// ban comes to — skipping ticks multiplies the effective `dt` of the
    /// substepped operator.
    ///
    /// Refused twice, and the second time is not redundant. `config/derive.rs`
    /// refuses it over the text of a scenario; [`tick::Tick::new`] refuses it
    /// over this array, which is the door a run actually goes through and which
    /// nothing in the crate builds from a `Config`.
    pub every_n_ticks: u32,
}

/// The roster a scenario that writes no `[[process]]` section gets (ADR-065).
///
/// Every entry, in the order of SPEC section 8, with each `enabled` taken from
/// the process's own module. The point of the record is that this is not "no
/// processes": a user who deleted the light block to turn the light off gets
/// light, and the only way off is `enabled = false`.
#[must_use]
pub fn default_roster() -> [RosterEntry; ROSTER_LEN] {
    ProcessId::ALL.map(|id| RosterEntry {
        id,
        enabled: id.enabled_by_default(),
        every_n_ticks: DEFAULT_EVERY_N_TICKS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_roster_default_comes_from_its_own_module() {
        // The delegation of ADR-065, asserted one process at a time rather than
        // in a loop over `ALL`: a loop could only compare `enabled_by_default`
        // with itself. Each line below names the constant the module declares, so
        // a second copy appearing inside `enabled_by_default` fails here.
        assert_eq!(
            ProcessId::Light.enabled_by_default(),
            light::ENABLED_BY_DEFAULT
        );
        assert_eq!(
            ProcessId::VelocityField.enabled_by_default(),
            velocity::VELOCITY_FIELD_ENABLED_BY_DEFAULT
        );
        assert_eq!(
            ProcessId::Advection.enabled_by_default(),
            advect::ENABLED_BY_DEFAULT
        );
        assert_eq!(
            ProcessId::Diffusion.enabled_by_default(),
            diffuse::ENABLED_BY_DEFAULT
        );
        assert_eq!(
            ProcessId::Pressure.enabled_by_default(),
            pressure::ENABLED_BY_DEFAULT
        );
        assert_eq!(
            ProcessId::Settling.enabled_by_default(),
            settle::ENABLED_BY_DEFAULT
        );
        assert_eq!(
            ProcessId::PhaseTransitions.enabled_by_default(),
            phase::ENABLED_BY_DEFAULT
        );
        assert_eq!(
            ProcessId::Reactions.enabled_by_default(),
            react::ENABLED_BY_DEFAULT
        );
        assert_eq!(
            ProcessId::ExternalChannels.enabled_by_default(),
            channels::ENABLED_BY_DEFAULT
        );
    }

    #[test]
    fn the_default_roster_is_the_defaults_of_the_nine_modules() {
        let roster = default_roster();
        assert_eq!(roster.len(), ROSTER_LEN);
        for (entry, id) in roster.iter().zip(ProcessId::ALL) {
            assert_eq!(entry.id, id, "the roster is in the order of SPEC section 8");
            assert_eq!(entry.enabled, id.enabled_by_default());
            assert_eq!(entry.every_n_ticks, DEFAULT_EVERY_N_TICKS);
        }
    }
}
