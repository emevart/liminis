//! External channels and events: line `j` of the tick order, and the second
//! roster entry with no operator behind it.
//!
//! ADR-059 assigns this step four of the six channels of `ledger::Channel` —
//! `GEOTHERMAL_IN`, `RADIATIVE_OUT`, `IMPACT` and `VENT_BURST` — and the other
//! two go elsewhere: `BOUNDARY_EXCHANGE` is credited by steps `c` and `d` on
//! every substep, `SOLAR_IN` by the fold of step `i'`. So the counters this step
//! would write already exist. What does not exist is anything to write into
//! them:
//!
//! - the exchange face is not buildable. `world::Grid::new` accepts
//!   `Boundary::Exchange`, and every process that has to look at a face —
//!   `advect`, `diffuse`, `settle` — refuses one, naming `BOUNDARY_EXCHANGE`;
//! - there are no events. `IMPACT` and `VENT_BURST` are described as rare events
//!   in SPEC section 7 and by no schedule, key or type anywhere;
//! - `GEOTHERMAL_IN` and `RADIATIVE_OUT` are boundary conditions on the enthalpy
//!   field — heat at `z = 0`, `sigma*T^4` off the top layer — and enthalpy has
//!   no transport operator in `process/` at all yet.
//!
//! # Why it is a roster entry rather than nothing
//!
//! Because the entry is where "no channel was written this tick" stops being an
//! accident. ADR-059 draws a line after phase 4 — "after phase 4 no channel is
//! written" — and a step that owns the credits is what makes that line
//! checkable: everything that credits, credits here or earlier, and the LEDGER
//! phase runs after. A tick loop that credited from wherever it felt like would
//! make the *next* tick's residual wrong without naming a culprit.
//!
//! # What is not decided
//!
//! The shape of every one of the four. `GEOTHERMAL_IN` needs a heat flux and a
//! composition at the vents (SPEC section 7 names the substances and no
//! numbers), `RADIATIVE_OUT` needs an emissivity and a temperature field derived
//! from enthalpy (ADR-062, and that operator is unwritten), and both events need
//! a schedule that `CONFIG_SCHEMA.md` does not declare. The sign convention of a
//! counter is open too — `TODO(counter-sign)` in `ledger/mod.rs`.

use super::{Conservation, Invariant};

/// The default of `enabled` for the external channels (ADR-065).
///
/// `false`, on the same argument the other blocked processes carry: the step has
/// nothing to dispatch, so `true` would buy a different `config_hash` for an
/// identical world and nothing else. It is the weakest of the nine arguments,
/// and worth saying so — a channel step that does nothing is indistinguishable
/// from a channel step that is off, which is exactly why the value here is a
/// `TODO` and not a decision.
// TODO(CONFIG_SCHEMA.md section 13 item 23): no record assigns this value. When
// the four channels grow operators the question becomes a real one — a world
// with the vents off is a different world, not a cheaper one — and it is settled
// by an entry in `DECISIONS.md`.
pub const ENABLED_BY_DEFAULT: bool = false;

/// What the external channels do to the two ledgers.
///
/// Both `Conserved` today, and both wrong the moment the step does anything: the
/// whole purpose of a channel is that matter or energy crosses the boundary of
/// the domain, which is the second arm of [`Conservation`] — the arm that names
/// a channel and that `process/mod.rs` explains is still missing. This is the
/// process that will need it first.
#[inline]
#[must_use]
pub fn invariant() -> Invariant {
    Invariant {
        matter: Conservation::Conserved,
        energy: Conservation::Conserved,
    }
}
