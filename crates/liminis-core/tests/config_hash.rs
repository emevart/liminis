//! The config hash has to identify the configuration, not the file.

use liminis_core::config;
use std::path::{Path, PathBuf};

/// Write a fixture into Cargo's per-test-binary temp directory and return its
/// path. Two configs mean two files, so these tests go through `config::load`
/// rather than `config::parse`.
fn fixture(name: &str, text: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, text).expect("writing fixture");
    path
}

fn hash_of(name: &str, text: &str) -> String {
    let config = config::load(&fixture(name, text)).expect("loading fixture");
    config::config_hash(&config).expect("hashing config")
}

const PLAIN: &str = r#"
name = "hello"
dt = 0.05

[grid]
nx = 64
ny = 32
nz = 16
dx = 1.0e-4
"#;

/// Same configuration, typed differently: keys reordered, blank lines and
/// indentation moved around, comments added, floats written another way.
const SCRAMBLED: &str = r#"
# the same scenario, typed by someone else
    dt   =    5.0e-2
name="hello"


[grid]
    dx = 0.0001   # 100 um
  nz  =  16
  nx  =  64
     ny = 32
"#;

#[test]
fn whitespace_and_key_order_do_not_change_the_hash() {
    assert_eq!(
        hash_of("plain.toml", PLAIN),
        hash_of("scrambled.toml", SCRAMBLED),
    );
}

#[test]
fn changing_a_value_changes_the_hash() {
    let nudged = PLAIN.replace("nz = 16", "nz = 17");
    assert_ne!(
        hash_of("plain_again.toml", PLAIN),
        hash_of("nudged.toml", &nudged)
    );
}

#[test]
fn an_unknown_field_is_an_error() {
    let typo = PLAIN.replace("nz = 16", "nz = 16\nnw = 16");
    let err = config::load(&fixture("typo.toml", &typo))
        .expect_err("an unknown field must not be ignored silently");
    assert!(
        format!("{err:#}").contains("nw"),
        "the error should name the offending key, got: {err:#}",
    );
}

/// The hash is taken after defaults are applied, so omitting a field and
/// spelling out its default value are the same configuration.
#[test]
fn an_omitted_field_hashes_as_its_default() {
    let spelled_out =
        "name = \"defaults\"\ndt = 0.05\n\n[grid]\nnx = 64\nny = 64\nnz = 64\ndx = 1.0e-4\n";
    let omitted = "name = \"defaults\"\n\n[grid]\n";
    assert_eq!(
        hash_of("spelled_out.toml", spelled_out),
        hash_of("omitted.toml", omitted),
    );
}
