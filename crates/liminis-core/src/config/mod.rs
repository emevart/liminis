//! Scenario configuration: parse, apply defaults, hash canonically.
//!
//! A run is identified by `(seed, config_hash, world_format_version)`
//! (ADR-016, ADR-020). For that identity to mean anything, the hash has to
//! depend on the *configuration* and not on how the file was typed. Two
//! properties give that:
//!
//! - Deserializing into a typed struct discards whitespace, comments and key
//!   order, and fills in every omitted field with its default.
//! - The canonical form is the serialization of an explicit projection of that
//!   struct — `config/hash.rs` — which is what makes the exclusion of a key a
//!   decision the compiler checks rather than a habit (ADR-066).
//!
//! `deny_unknown_fields` closes the other half: a typo in a key name is an
//! error, not a silently ignored line that changes nothing and hashes the same.
//!
//! The module splits three ways. [`schema`] holds the serde types — the shape of
//! a scenario file, section by section of `CONFIG_SCHEMA.md`. [`hash`] holds the
//! projection and the hash taken over it. This file holds the two entry points,
//! and nothing else: what a loaded config still lacks is named in the TODOs on
//! [`load`]. [`validate`] holds every rule of `CONFIG_SCHEMA.md` section 10 and
//! is a separate entry point, for the reason written on [`load`].

mod derive;
mod hash;
mod schema;
mod validate;

pub use derive::{
    Derived, DerivedEnergy, DerivedField, DerivedReaction, DerivedSubstance, Nu, derive,
};
pub use hash::{NOT_HASHED, canonical, config_hash};
pub use schema::{
    Boundary, Calibration, Config, Face, Field, Grid, Process, Rate, Reaction, Requirement,
    Reservoir, Scale, Substance,
};
pub use validate::validate;

use anyhow::{Context, Result};
use std::path::Path;

/// Read a config file and apply defaults.
///
/// Nothing else yet. Two things a caller may reasonably expect are absent, and
/// their absence is deliberate rather than forgotten.
///
/// TODO(scales): [`derive`] is not called from here. It computes every scale of
/// `CONFIG_SCHEMA.md` section 9 — `k[i]`, `e_r`, `ν`, `ν_E`, `k_E`, the
/// `i32`/`i64` choice, the mass tolerance and the substeps — and it returns a
/// `Result`, so calling it would make loading refuse configs that parse. Whether
/// it belongs inside `load`, or beside it in a validator, is settled by no
/// record: `ARCHITECTURE.md` lists "schema, validator, derivation of scales and
/// substeps" for `config/` in one line and names no file. Deciding it here would
/// also decide where the refusals of section 10 live, which is a larger question
/// than this line.
///
/// What wiring it would break today is worth naming, so that the day it is
/// wired is not the day it is discovered: `configs/scenarios/hello.toml` does
/// not survive its own derivation. It declares no `[[substance]]` at all, so the
/// heat capacity of a cell is zero, there is no temperature, and the energy
/// scale of ADR-062 has nothing to be derived from — and the missing
/// `[[field]] id = "enthalpy"` record is only the first of the two refusals it
/// meets. Adding the record does not fix it and neither does materialising the
/// field from defaults: the scenario has no matter, and the matter cannot be
/// invented here, because `c_p` is declared for no substance anywhere in the
/// corpus (`CONFIG_SCHEMA.md` section 13 item 23) while `C_cell` is a sum over
/// exactly that key. So `derive` is defined on a scenario with matter, the only
/// scenario in the repository has none, and the two are reconciled by whoever
/// closes item 23 — not by a placeholder registry written into `configs/`.
///
/// The validator, on the other hand, exists — and it is deliberately **not**
/// called from here. [`validate`] is a second entry point, and the reason is the
/// paragraph above rather than a preference: `hello.toml` does not survive
/// `derive`, so calling the validator from `load` would turn
/// `every_scenario_in_the_repository_loads` in `tests/config_hash.rs` red and
/// break the command printed in `README.md` and `CLAUDE.md`. Giving `hello.toml`
/// a registry is not available while `CONFIG_SCHEMA.md` section 13 item 23 keeps
/// `c_p` undeclared for every substance in the corpus. So `load` parses,
/// `validate` checks, and the day item 23 is closed the two can meet.
///
/// TODO(ADR-065 roster): the full list of processes is *not* materialized before
/// hashing, and this one breaks a documented property in silence. A config
/// without `enabled` and a config with `enabled = true` describe the same world
/// and get **different** `config_hash`: `Option::None` does not serialize at
/// all, so the key is simply absent from the canonical form. That is
/// `CONFIG_SCHEMA.md` section 11 item 2 violated, and no test in this crate
/// catches it — `an_omitted_field_hashes_as_its_default` looks at root scalars.
/// The fix is the materialization ADR-065 asks for, and its input is missing:
/// the default `enabled` of eight of the nine S0 processes is assigned by
/// nothing at all (section 13 item 23). Until then `an_omitted_process_section_hashes_as_the_full_default_roster`
/// and `the_canonical_form_names_every_process_in_the_roster` cannot be written.
///
/// # Errors
///
/// Returns an error if the file cannot be read, or if its contents are not a
/// valid scenario.
pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    parse(&text).with_context(|| format!("parsing config {}", path.display()))
}

/// Parse config text and apply defaults.
///
/// # Errors
///
/// Returns an error if the text is not valid TOML or does not match the schema
/// — including an unknown key, which is an error by decision and not by
/// accident (`CONFIG_SCHEMA.md` section 11 item 1).
pub fn parse(text: &str) -> Result<Config> {
    Ok(toml::from_str(text)?)
}
