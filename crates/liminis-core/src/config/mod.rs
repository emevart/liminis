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
//! A third property joined the two above with ADR-065: the loader materialises
//! the whole process roster — see [`materialise`] — so a section whose full
//! membership is not in the file gets its defaults applied before the hash like
//! any other key.
//!
//! The module splits three ways. [`schema`] holds the serde types — the shape of
//! a scenario file, section by section of `CONFIG_SCHEMA.md`. [`hash`] holds the
//! projection and the hash taken over it. This file holds the entry points and
//! the materialisation: what a loaded config still lacks is named in the TODOs on
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

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::process::{DEFAULT_EVERY_N_TICKS, ProcessId, ROSTER_LEN};

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
    let mut config: Config = toml::from_str(text)?;
    materialise(&mut config).context("materialising the process roster (ADR-065)")?;
    Ok(config)
}

/// Fill in the process roster: every process of [`ProcessId::ALL`], each with the
/// `enabled` its own module declares, in the order of SPEC section 8.
///
/// **Called from [`parse`] and therefore before the hash, which is the whole
/// decision.** ADR-065 rejected the cheaper variant — hash what was written,
/// execute what was materialised — in as many words: it breaks
/// `CONFIG_SCHEMA.md` section 11 item 2 head on, a config spelling out a default
/// and a config omitting the record would get different `config_hash` for one
/// world, and the identity of a run would again depend on how the file was typed.
///
/// Inside `parse` and not inside [`load`], for a reason that is not symmetry: the
/// tests of this crate go through `parse` and a run goes through `load`, so a
/// materialisation living in `load` alone would give the two different hashes —
/// and not as a failure, but as "these two runs are incomparable", with nothing
/// falling over.
///
/// Idempotent: running it over a config it has already filled in changes
/// nothing, which is what `the_canonical_form_reloads_to_the_same_hash` needs,
/// since the canonical form carries the whole roster.
///
/// # The trap ADR-065 asks to be said out loud
///
/// **Deleting a record no longer switches a process off.** Whoever erased the
/// light block so that there would be no light gets light. The only way off is
/// `enabled = false`.
///
/// # Errors
///
/// Returns an error if a record names a process that is not in the roster, or if
/// two records name the same one. Neither is caught by `deny_unknown_fields` —
/// serde sees the fields of a struct and `id` is a value — and the second is not
/// pedantry: "the last one wins" would make the canonical form depend on the
/// order the records were typed in.
pub fn materialise(config: &mut Config) -> Result<()> {
    let mut written = [false; ROSTER_LEN];
    for record in &config.process {
        let Some(id) = ProcessId::from_id(&record.id) else {
            bail!(
                "process `{}` is not in the roster. The registry of processes is \
                 closed and lives under `process/` (ADR-065); the nine it holds \
                 are {}",
                record.id,
                ProcessId::ALL
                    .iter()
                    .map(|p| format!("`{}`", p.id()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        };
        let row = roster_row(id);
        if written[row] {
            bail!(
                "process `{}` is declared twice. Letting the last record win \
                 would make the canonical form — and so `config_hash` — depend on \
                 the order the records were typed in (`CONFIG_SCHEMA.md` section \
                 11)",
                record.id
            );
        }
        written[row] = true;
    }

    for record in &mut config.process {
        if record.enabled.is_none() {
            let id = ProcessId::from_id(&record.id).expect("checked in the loop above");
            record.enabled = Some(id.enabled_by_default());
        }
    }

    for id in ProcessId::ALL {
        if !written[roster_row(id)] {
            config.process.push(default_record(id));
        }
    }

    // The order of the records is the order of SPEC section 8 and never the order
    // of the file. ADR-065 settles that the schema has no `order` key and leaves
    // the rest open; appending the missing records to the tail and keeping the
    // file's order for the rest is the variant that had to be rejected here,
    // because two files describing one world would then hash differently — and
    // `whitespace_and_key_order_do_not_change_the_hash` looks at the order of the
    // keys in a table, not at the order of the entries in an array of tables, so
    // nothing would have caught it.
    config.process.sort_by_key(|record| {
        roster_row(ProcessId::from_id(&record.id).expect("checked in the first loop"))
    });

    Ok(())
}

/// The record a scenario that wrote nothing about `id` gets.
///
/// Every parameter is `None`: a parameter belongs to whoever folds the process,
/// and a default invented here would be a number in the hash of every scenario
/// that nobody decided. `stir_fraction` is the exception the schema already
/// names — ADR-069 assigns it `0` — and it comes from serde's default rather
/// than from this function.
fn default_record(id: ProcessId) -> Process {
    Process {
        id: id.id().to_string(),
        enabled: Some(id.enabled_by_default()),
        every_n_ticks: DEFAULT_EVERY_N_TICKS,
        mu: None,
        u_conv_max: None,
        l_c: None,
        stir_period: None,
        stir_fraction: 0.0,
        k_w: None,
        k_b: None,
        k_d: None,
        k_m: None,
    }
}

/// The position of a process in [`ProcessId::ALL`], which is its row in the two
/// flat arrays above and its place in the canonical form.
fn roster_row(id: ProcessId) -> usize {
    ProcessId::ALL
        .iter()
        .position(|&p| p == id)
        .expect("ProcessId::ALL holds every variant")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The minimum a scenario has to write.
    const BARE: &str = "name = \"roster\"\nT_ref = 298.15\n\n[grid]\n";

    #[test]
    fn an_omitted_process_section_materialises_the_whole_roster() {
        let config = parse(BARE).expect("parsing");
        assert_eq!(config.process.len(), ROSTER_LEN);
        for (record, id) in config.process.iter().zip(ProcessId::ALL) {
            assert_eq!(record.id, id.id(), "in the order of SPEC section 8");
            assert_eq!(record.enabled, Some(id.enabled_by_default()));
            assert_eq!(record.every_n_ticks, DEFAULT_EVERY_N_TICKS);
        }
    }

    #[test]
    fn the_default_every_n_ticks_is_one_on_both_sides() {
        // The schema fills the key in per record and `process/` states the number
        // the roster is built on. Two homes, one value — and unlike `enabled`
        // this pair is safe to keep, because ADR-030 makes `> 1` a refusal on any
        // process touching a diffusive field. Asserted so that the safety is a
        // fact rather than a belief.
        let config = parse(&format!("{BARE}\n[[process]]\nid = \"diffusion\"\n")).expect("parse");
        let written = config
            .process
            .iter()
            .find(|record| record.id == ProcessId::Diffusion.id())
            .expect("the record the fixture wrote");
        assert_eq!(written.every_n_ticks, DEFAULT_EVERY_N_TICKS);
    }

    #[test]
    fn materialising_twice_changes_nothing() {
        // The canonical form carries the whole roster, so it is read back through
        // `parse` by `the_canonical_form_reloads_to_the_same_hash`.
        let mut once = parse(BARE).expect("parsing");
        let twice = {
            let mut config = once.clone();
            materialise(&mut config).expect("materialising again");
            config
        };
        materialise(&mut once).expect("materialising again");
        assert_eq!(once, twice);
        assert_eq!(once.process.len(), ROSTER_LEN);
    }

    #[test]
    fn a_written_record_keeps_what_it_wrote() {
        // The trap ADR-065 asks to be named: the only way off is `enabled = false`,
        // and it has to survive materialisation.
        let text = format!("{BARE}\n[[process]]\nid = \"diffusion\"\nenabled = false\n");
        let config = parse(&text).expect("parsing");
        let diffusion = config
            .process
            .iter()
            .find(|record| record.id == ProcessId::Diffusion.id())
            .expect("the diffusion record");
        assert_eq!(diffusion.enabled, Some(false));
        assert!(
            ProcessId::Diffusion.enabled_by_default(),
            "the fixture is only meaningful while the default is the other way"
        );
    }
}
