//! External channels and events: line `j` of the tick order, and the second
//! roster entry with no operator behind it.
//!
//! ADR-059 assigns this step four of the six channels of `ledger::Channel` —
//! `GEOTHERMAL_IN`, `RADIATIVE_OUT`, `IMPACT` and `VENT_BURST` — and the other
//! two go elsewhere: `BOUNDARY_EXCHANGE` is credited by steps `c` and `d` on
//! every substep, `SOLAR_IN` by the fold of step `i'`. So the counters this step
//! would write already exist.
//!
//! **`BOUNDARY_EXCHANGE` is not among the four and no longer among the empty
//! ones.** The exchanging face is built (ADR-059): `world::Grid::new` accepts it,
//! `Grid::neighbour` answers with the ghost cell, and `process::DiffusePhase` and
//! `process::AdvectPhase` credit what crosses it on every substep of their own
//! steps. Nothing is left for this step to do about it, and that is the shape
//! ADR-059 chose deliberately — it rejected "treat the exchange as a separate
//! process at step `j`" by name, because the exchange would then be applied over
//! a full `dt` after diffusion had already run `n` substeps, and the flux would
//! be computed from `dst` rather than from `src`.
//!
//! What does not exist is anything to write into the four that are left:
//!
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
//! numbers), `RADIATIVE_OUT` needs an emissivity, and both events need a schedule
//! that `CONFIG_SCHEMA.md` does not declare. Three blockers and not four: the
//! heat flux and the vent composition are **one** of them, because they belong
//! to one channel and one record lifts them together.
//!
//! The sign convention of a counter is no longer on the list. ADR-084 ratifies
//! it as the increment of the domain, and `ledger/mod.rs` says which way.
//!
//! The temperature `sigma*T^4` is taken over is **not** on that list any more:
//! `process/temperature.rs` derives it from enthalpy and the composition of the
//! coarse cell (ADR-044, ADR-062) and `world::World` owns the buffer. What has to
//! be said instead is that the field is on the enthalpy grid and the radiating
//! surface is the top layer of the *fine* one, so `RADIATIVE_OUT` needs a rule
//! for which cells of a `32^3` field face the sky — and that rule is one more
//! thing no record writes.

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
/// the domain, which is the second arm of [`Conservation`] — the arm that names a
/// channel. That arm exists now (ADR-059) and the two transport steps already use
/// it; this step will name a different channel of the six on the day it has an
/// operator, and it has none, so what it does to both ledgers is nothing.
#[inline]
#[must_use]
pub fn invariant() -> Invariant {
    Invariant {
        matter: Conservation::Conserved,
        energy: Conservation::Conserved,
    }
}
