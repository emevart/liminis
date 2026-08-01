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
//! # What is not here
//!
//! No `Process` trait, and not for lack of a decision about its shape — ADR-034
//! settled that. It is missing three types, and each of them is blocked on
//! something that has to be decided before it can be written:
//!
//! - `FieldRef`, for `reads` and `writes`. A field reference is a substance and
//!   a buffer address, and a substance only becomes an address once the loader
//!   says which storage width and which lane it got (ADR-039, ADR-040). The
//!   open end is written up as `TODO(width-lanes)` on `world::Field`.
//! - The second arm of [`Conservation`]. It names a channel, the channel
//!   registry of SPEC section 7 is not in code, and `ledger/` does not exist.
//! - The `world` argument of `apply`. `world::Field` deliberately stops short of
//!   a `World` aggregate, for the same substance-to-lane reason.
//!
//! Writing the trait against invented versions of those three would put a
//! second, silent addressing scheme underneath every process. So for now a
//! process is a plain type with an `apply` that takes exactly the fields it
//! touches — which is the half of ADR-034 that matters anyway: a process cannot
//! reach a buffer it did not name.

mod diffuse;

pub use diffuse::{Diffuse, N_MAX, substeps_and_alpha, substeps_for};

/// What a process does to one of the two ledgers over one tick.
///
/// The pair of enums of SPEC section 4.1, one arm short. `Conserved` means the
/// process moves the quantity around and the sum over the domain comes out bit
/// for bit equal — not "equal to within a tolerance": ADR-003 makes the per-tick
/// residual an exact integer comparison, and transport conserves by construction
/// rather than by accuracy (ADR-005).
// TODO(channels): the other arm, `ChangedThrough(channel)`, is missing because
// the channel registry is not in code. SPEC section 7 names the channels in
// prose, `ledger/` does not exist, and `CONFIG_SCHEMA.md` section 13 item 7
// records that the legal set of channel names is not even fixed. A process that
// needs it — light, boundary exchange, maintenance upkeep — cannot be written
// until then anyway, so an invented enum would have no user and every chance of
// being wrong by the time it had one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Conservation {
    /// The quantity is neither created nor destroyed by this process.
    Conserved,
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
