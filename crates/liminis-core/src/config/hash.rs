//! The canonical form and the hash taken over it.
//!
//! ADR-038 excluded `[[calibration]]` from the hash and generalised the case
//! into a rule: **the hash covers what the simulator reads, and nothing beyond
//! it.** ADR-066 gives that rule a mechanism instead of a discipline. The
//! canonical form is the serialization of a separate borrowing type,
//! [`Hashed`], assembled from a [`Config`] by a function whose destructuring is
//! exhaustive — no `..` — so an unsimulated key is excluded by construction, and
//! a new schema key fails to *compile* until somebody has decided about it.
//!
//! That is strictly louder than a test: a failure on `cargo build`, not on
//! `cargo test`. What the compiler cannot do is tell `field: x` from `field: _`,
//! and that is what `the_projection_covers_every_simulated_key` is for.
//!
//! Two things the projection deliberately does not buy, both worth knowing
//! before relying on it.
//!
//! **It localizes the decision about what is hashed, not the bytes.** Nested
//! types are borrowed whole — `&[Substance]`, `&Grid` — rather than mirrored, so
//! the order of nested keys lives in `config/schema.rs`. Adding a simulated key
//! with a default moves the `config_hash` of every existing scenario, and it
//! should, because what the simulator reads has changed. The one gain is the one
//! that was wanted: adding an *un*simulated key no longer moves any hash.
//!
//! **The `[[process]]` array it prints is the materialised roster, not what the
//! file wrote.** `config::materialise` runs inside `parse`, so by the time the
//! projection is taken the array holds all nine records of ADR-065, each with an
//! explicit `enabled`, sorted into the order of SPEC section 8. Two things follow
//! and both are the point of that record: a scenario that omitted a record hashes
//! exactly like one that spelled the default out (`CONFIG_SCHEMA.md` section 11
//! item 2), and two files listing the same processes in different orders hash the
//! same. The order of the records is fixed *there* rather than here — this
//! module serializes `&[Process]` whole, so it has no say in it — which is why
//! `the_process_order_in_the_canonical_form_does_not_depend_on_the_file` is a
//! test of the loader and not of the projection.
//!
//! **The compiler checks the names and types of these fields, and not their
//! order** (ADR-066 says so outright). A field moved within [`Hashed`] compiles,
//! changes every `config_hash`, and turns nothing red — the path difference
//! stays empty, the round trip still agrees, and both existing tests compare
//! hashes only against each other. The defence is this paragraph and the
//! discipline it asks for. A snapshot test of a literal hash is not the answer:
//! it would be the first recorded value in the project and would itself become
//! the thing that has to be kept true.

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;

use super::schema::{
    Boundary, Config, Field, Genetics, Grid, Initial, Physics, Process, Reaction, Substance,
};

/// Root sections of the schema that the canonical form leaves out.
///
/// Public because `the_projection_covers_every_simulated_key` reads it from
/// `tests/`, where it asserts both directions: every path missing from the
/// canonical form belongs to a section named here, and every section named here
/// accounts for at least one missing path. A key struck out with an underscore
/// and not listed here fails that test by name.
///
/// One entry today (ADR-038). Were ADR-058 ever to decide that a scenario may
/// carry a `seed`, excluding it would cost one line `seed: _` in [`project`] and
/// one line here (ADR-066).
pub const NOT_HASHED: &[&str] = &["calibration"];

/// The hashed projection of a [`Config`]: the thirteen root keys the simulator
/// reads.
///
/// Private on purpose. The test takes its paths from the output of [`canonical`]
/// parsed back into a `toml::Value` rather than from a second serialization of
/// this type, so that what it inspects is the bytes that are actually hashed.
///
/// Borrows throughout: no copies, about a hundred and thirty bytes of stack and
/// no allocation. The one allocation is the canonical string itself, and that
/// one exists today too.
///
/// Field order must mirror [`Config`], scalars ahead of tables — see the note in
/// the module header about what does *not* enforce that.
#[derive(Serialize)]
struct Hashed<'a> {
    name: &'a str,
    dt: &'a f64,
    beta: &'a f64,
    #[serde(rename = "T_ref")]
    t_ref: &'a f64,
    conserved: &'a BTreeMap<String, f64>,
    grid: &'a Grid,
    boundary: &'a Boundary,
    /// Among the tables and between `[boundary]` and `[[substance]]`, which is
    /// where `Config` puts it and where `CONFIG_SCHEMA.md` numbers it (ADR-085).
    /// The position is a decision of the schema and not of this file — see the
    /// note in the module header about what does *not* enforce it.
    physics: &'a Physics,
    substance: &'a [Substance],
    reaction: &'a [Reaction],
    field: &'a [Field],
    process: &'a [Process],
    /// Last, and among the tables rather than among the scalars (section 11
    /// item 3). The initial state determines every subsequent tick, so a run
    /// that starts elsewhere is a different run and the section is hashed like
    /// any other thing the simulator reads (ADR-077).
    initial: &'a Initial,
    #[serde(skip_serializing_if = "Option::is_none")]
    genetics: &'a Option<Genetics>,
}

/// Build the projection.
///
/// The destructuring is exhaustive and has no `..`: a new field on [`Config`]
/// breaks this line with E0027, and it stays broken until its author writes down
/// whether the simulator reads it. That refusal is the whole point of the
/// function — it does no work otherwise.
///
/// `#[serde(skip_serializing)]` on the excluded field was the obvious
/// alternative and was rejected outright (ADR-066): `Config` has one
/// `Serialize` implementation, and the attribute would break it for every
/// consumer at once, so `check --dump` and every future config output would lose
/// a section the search driver has to read.
fn project(config: &Config) -> Hashed<'_> {
    let Config {
        name,
        dt,
        beta,
        t_ref,
        conserved,
        grid,
        boundary,
        physics,
        substance,
        reaction,
        field,
        process,
        initial,
        genetics,
        // ADR-038: read by the search driver, never by a tick. Listed in
        // `NOT_HASHED`, which is what keeps this underscore honest.
        calibration: _,
    } = config;

    Hashed {
        name,
        dt,
        beta,
        t_ref,
        conserved,
        grid,
        boundary,
        physics,
        substance,
        reaction,
        field,
        process,
        initial,
        genetics,
    }
}

/// Canonical serialization of a config: the exact bytes that get hashed.
///
/// Round-trip lives on the type, not on these bytes: `toml::to_string(config)`
/// remains a complete, lossless serialization of a `Config`. The canonical form
/// drops `[[calibration]]` on purpose — its job is to be hashed, not to be read
/// back as a scenario. What is still required of it,
/// `the_canonical_form_reloads_to_the_same_hash` checks.
///
/// # Errors
///
/// Returns an error if the projection cannot be written as TOML.
pub fn canonical(config: &Config) -> Result<String> {
    toml::to_string(&project(config)).context("serializing config canonically")
}

/// Hash of the canonical form, as `blake3:<16 hex chars>`.
///
/// One of the three coordinates of a run's identity, alongside the seed and
/// `WORLD_FORMAT_VERSION` (ADR-016, ADR-020).
///
/// # Errors
///
/// Returns an error if the canonical form cannot be produced.
pub fn config_hash(config: &Config) -> Result<String> {
    let canonical = canonical(config)?;
    let hex = blake3::hash(canonical.as_bytes()).to_hex();
    Ok(format!("blake3:{}", &hex.as_str()[..16]))
}
