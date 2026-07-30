//! Scenario configuration: parse, apply defaults, hash canonically.
//!
//! A run is identified by `(seed, config_hash, world_format_version)`
//! (ADR-016, ADR-020). For that identity to mean anything, the hash has to
//! depend on the *configuration* and not on how the file was typed. Two
//! properties give that:
//!
//! - Deserializing into a typed struct discards whitespace, comments and key
//!   order, and fills in every omitted field with its default.
//! - Serializing that struct back walks the fields in declaration order, so
//!   the canonical form is fully determined by the values.
//!
//! `deny_unknown_fields` closes the other half: a typo in a key name is an
//! error, not a silently ignored line that changes nothing and hashes the same.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A scenario configuration, after defaults have been applied.
///
/// Field order here *is* the canonical serialization order. Scalars must stay
/// ahead of tables: TOML has no way to write a bare key after a table header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Scenario name. The one field without a default.
    pub name: String,
    /// Timestep, seconds.
    #[serde(default = "default_dt")]
    pub dt: f64,
    pub grid: Grid,
}

/// Grid extent and spacing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    #[serde(default = "default_extent")]
    pub nx: u32,
    #[serde(default = "default_extent")]
    pub ny: u32,
    #[serde(default = "default_extent")]
    pub nz: u32,
    /// Voxel edge, metres. Selects the regime: 1e-6 is micro, 1e-4 is eco
    /// (ADR-013).
    #[serde(default = "default_dx")]
    pub dx: f64,
}

fn default_dt() -> f64 {
    0.05
}

fn default_extent() -> u32 {
    64
}

fn default_dx() -> f64 {
    1.0e-4
}

/// Read a config file and apply defaults.
pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    parse(&text).with_context(|| format!("parsing config {}", path.display()))
}

/// Parse config text and apply defaults.
pub fn parse(text: &str) -> Result<Config> {
    Ok(toml::from_str(text)?)
}

/// Canonical serialization of a config: the exact bytes that get hashed.
pub fn canonical(config: &Config) -> Result<String> {
    toml::to_string(config).context("serializing config canonically")
}

/// Hash of the canonical form, as `blake3:<16 hex chars>`.
pub fn config_hash(config: &Config) -> Result<String> {
    let canonical = canonical(config)?;
    let hex = blake3::hash(canonical.as_bytes()).to_hex();
    Ok(format!("blake3:{}", &hex.as_str()[..16]))
}
