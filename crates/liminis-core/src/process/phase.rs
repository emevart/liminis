//! Phase transitions: line `g` of the tick order, and the one roster entry with
//! no operator behind it.
//!
//! There is no kernel here and no `apply`. ADR-065 counts nine records for S0
//! and this is one of them, so the module exists to hold the two things a roster
//! entry owes — its default of `enabled`, and its invariant — beside each other,
//! which is the rule the record states: "each process declares its own default
//! `enabled` next to its own invariant". Splitting line `g` off into a table of
//! booleans somewhere else would make that one rule into two.
//!
//! # A record that is never dispatched
//!
//! `tick::STEP_ORDER` has no step for this letter, so a roster entry for phase
//! transitions is materialised, hashed, printed in the canonical form — and
//! never reached. That is a state worth declaring out loud rather than
//! discovering: a user who writes `enabled = true` here gets a config that
//! loads, a `config_hash` that differs from his colleague's, and a world in
//! which nothing at all happens differently.
//!
//! Why the entry exists anyway: `config_hash` identifies the *configuration*,
//! and the day this line grows an operator every scenario that ran without one
//! has to be distinguishable from the scenarios that ran with it. Leaving the
//! record out until then would make those two worlds hash the same.
//!
//! # What is not decided
//!
//! Everything except the fact that the line is in SPEC section 8. What a phase
//! transition *is* in this model — condensation, precipitation of `MINERAL`,
//! something else — is named by no record, and neither is what it would read or
//! write. `docs/OPEN_QUESTIONS.md` is where that goes, not a plausible kernel
//! here.

use super::{Conservation, Invariant};

/// The default of `enabled` for phase transitions (ADR-065).
///
/// `false`, and this is the one of the nine where the argument is not about
/// physics at all: there is no operator. A default of `true` would enable a step
/// that cannot run, and the only observable consequence would be a different
/// `config_hash` for the same world — the failure mode ADR-065 rejected a
/// blanket `enabled = true` for, one line further along.
// TODO(CONFIG_SCHEMA.md section 13 item 23): no record assigns this value. The
// argument above is the argument for the value the code has to carry today, not
// a decision about what the default becomes once line `g` has an operator; the
// decision is a new entry in `DECISIONS.md`.
pub const ENABLED_BY_DEFAULT: bool = false;

/// What phase transitions do to the two ledgers.
///
/// Both conserved, and the claim is honest for an empty operator in exactly the
/// way it will stop being honest later: a phase change moves matter from one
/// substance to another and releases or absorbs a latent heat, so the energy arm
/// becomes the second arm of [`Conservation`] — the one that names a channel —
/// or the enthalpy field, the day there is something to declare. Written now
/// because a roster entry without an invariant is the field ADR-034 took out of
/// the config precisely so that it would not go unstated.
#[inline]
#[must_use]
pub fn invariant() -> Invariant {
    Invariant {
        matter: Conservation::Conserved,
        energy: Conservation::Conserved,
    }
}
