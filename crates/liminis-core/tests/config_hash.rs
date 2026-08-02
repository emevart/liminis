//! The config hash has to identify the configuration, not the file.

use liminis_core::config;
use std::collections::BTreeSet;
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

/// Rewrite one passage of a fixture, refusing to be a no-op.
///
/// `str::replace` against a passage the fixture no longer contains rewrites
/// nothing and hands back a copy — and every test below would stay green while
/// comparing a fixture with itself, which is the one failure they cannot see on
/// their own: both sides remain valid scenarios. Exactly one occurrence, or the
/// fixture has drifted away from what the test means to say.
fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(
        text.matches(from).count(),
        1,
        "the fixture must contain `{from}` exactly once",
    );
    text.replace(from, to)
}

const PLAIN: &str = r#"
name = "hello"
dt = 1.0
T_ref = 298.15

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
    dt   =    1e0
name="hello"

  T_ref  =  2.9815e2

[grid]
    dx = 0.0001   # 100 um
  nz  =  16
  nx  =  64
     ny = 32
"#;

/// The scenario of `CONFIG_SCHEMA.md` section 12, whole.
///
/// Whole is a requirement rather than thoroughness.
/// `the_projection_covers_every_simulated_key` is only as complete as this
/// text: an empty array of tables prints as `substance = []` and yields the
/// root path and no nested one (ADR-066), and a `None` yields no path on
/// *either* side — so an optional key the projection forgot would pass unseen.
/// Hence at least one entry of every array of tables, and every optional key
/// spelled out: `[boundary.reservoir]`, the three enthalpy keys of `[[field]]`,
/// `requires` on the reaction, and every process parameter of section 7.
///
/// Four records here are past section 12 — `requires`, and `[[process]]` for
/// light and for settling — and they exist for that reason alone: the example
/// does not write them, and without them `k_w … k_m`, `mu`, the five light keys
/// of ADR-076 and `requires.{field,min,max}` are invisible to the test.
///
/// **Nothing here meets the validator, and that is a decision rather than an
/// oversight of `load`.** Two of these passages are refused by it outright since
/// this wave: a non-empty `requires` is a load error until the reaction gate
/// exists (ADR-073), and `i_surface > 0` is refused by two locks at once
/// (ADR-076). They stay, because the fixture exists so that
/// `the_projection_covers_every_simulated_key` can see those keys at all, and a
/// key nobody may legally write is exactly the key a projection forgets. The
/// compiler is no help here: `Hashed<'a>` borrows `process: &'a [Process]` whole,
/// so exhaustive destructuring catches a new field of `Config` and not a new key
/// of `Process` (E0027 is about a struct, not about its element). This fixture is
/// the only thing that catches them.
///
/// `charge` is past section 12 too, and for a second reason. The example leaves
/// it out and says so — the balance closes as `0 = 0` either way — while
/// ADR-064 gives it exactly this spelling: `charge = 0.0` in `conserved`
/// against a signed `charge = -2` in a `composition`. Written out it makes
/// `composition` a map with two entries, and a map with one entry cannot be
/// reordered, so without it `whitespace_and_key_order_do_not_change_the_hash`
/// could not reach the sixth of the six maps at all.
///
/// **The numbers marked below are placeholders. This fixture checks the shape,
/// not the physics; the corpus names none of them (`CONFIG_SCHEMA.md` section
/// 13 item 23). Do not copy them into `configs/`.**
const SCENARIO: &str = r#"
name = "h2s-oxidation"
dt = 1.0
beta = 0.015625
T_ref = 298.15

[conserved]
C  = 12.01070
N  = 14.00670
P  = 30.97376
S  = 32.06500
Fe = 55.84500
charge = 0.0

[grid]
nx = 64
ny = 64
nz = 64
dx = 1.0e-4

[boundary]
z_min = "closed"
z_max = "exchange"

[boundary.reservoir]
t_out = 298.15
k_ex = 1.0e-5
conc_out = { H2S = 0.0, O2 = 0.25, SO4 = 28.0, H_ION = 1.0e-4 }

[[substance]]
id = "H2S"
molar_mass = 34.1
typical_conc = 0.1
max_conc = 10.0
partial_molar_volume = 1.0e-5   # placeholder, see the note on this fixture
settling_radius = 0.0
diffusivity = 1.6e-9
c_p = 100.0                     # placeholder
enthalpy_formation = -2.0e4     # placeholder
composition = { S = 1 }

[[substance]]
id = "O2"
molar_mass = 32.0
typical_conc = 0.25
max_conc = 1.0
partial_molar_volume = 1.0e-5   # placeholder
settling_radius = 0.0
diffusivity = 2.1e-9
c_p = 100.0                     # placeholder
enthalpy_formation = 0.0        # placeholder
composition = {}

[[substance]]
id = "SO4"
molar_mass = 96.1
typical_conc = 28.0
max_conc = 100.0
partial_molar_volume = 1.4e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 100.0                     # placeholder
enthalpy_formation = -9.0e5     # placeholder
composition = { S = 1, charge = -2 }

[[substance]]
id = "H_ION"
molar_mass = 1.0
typical_conc = 1.0e-4
max_conc = 1.0e-2
partial_molar_volume = 0.0
settling_radius = 0.0
diffusivity = 9.3e-9
c_p = 100.0                     # placeholder
enthalpy_formation = 0.0        # placeholder
composition = { charge = 1 }

[[reaction]]
id = "h2s_oxidation"
enthalpy = -8.46e5
catalyst = ""
energy_from = ""
inputs  = { H2S = 1, O2 = 2 }
outputs = { SO4 = 1, H_ION = 2 }

[[reaction.requires]]
field = "enthalpy"
min = 273.15
max = 323.15

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km  = { H2S = 0.01, O2 = 0.01 }

[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15

[[process]]
id = "diffusion"
enabled = true

[[process]]
id = "reactions"
enabled = true

[[process]]
id = "advection"
enabled = true

[[process]]
id = "velocity_field"
enabled = true
u_conv_max = 1.0e-5             # placeholder
l_c = 3.2e-3
stir_fraction = 0.0
stir_period = 3600.0

[[process]]
id = "pressure"
enabled = false

[[process]]
id = "light"
enabled = true
k_w = 0.04                      # placeholder
k_b = 0.02                      # placeholder
k_d = 0.01                      # placeholder
k_m = 0.5                       # placeholder
i_surface = 300.0               # placeholder
daily_fraction = 0.5            # placeholder
daily_period = 86400.0          # placeholder
seasonal_fraction = 0.25        # placeholder
seasonal_period = 31536000.0    # placeholder

[[process]]
id = "settling"
enabled = true
mu = 1.0e-3                     # placeholder

[[calibration]]
path = "reaction.h2s_oxidation.rate.q10"
min = 1.5
max = 3.0
scale = "linear"
"#;

/// The same scenario with the entries of every map typed in another order.
///
/// Struct-valued keys cannot carry the claim this fixture is for: their order in
/// the canonical form comes from the Rust declaration whatever the file says, so
/// `PLAIN` against `SCRAMBLED` would pass against an insertion-ordered map just
/// as well. Six keys of the schema hold maps whose *keys are data* —
/// `conserved`, `substance.composition`, `reaction.inputs`, `reaction.outputs`,
/// `rate.km` and `reservoir.conc_out` — and every one of them is reordered here,
/// because the map type is chosen per field and one of them turning into an
/// `IndexMap` would be invisible from any other (`CONFIG_SCHEMA.md` section 2).
///
/// Values are left alone. Reordering `km`, whose two entries hold equal numbers,
/// still changes the printed sequence of keys, which is the whole quantity under
/// test.
fn scenario_with_maps_reordered() -> String {
    let text = swap(
        SCENARIO,
        "C  = 12.01070\nN  = 14.00670\nP  = 30.97376\nS  = 32.06500\nFe = 55.84500\ncharge = 0.0",
        "charge = 0.0\nFe = 55.84500\nS  = 32.06500\nP  = 30.97376\nN  = 14.00670\nC  = 12.01070",
    );
    let text = swap(
        &text,
        "conc_out = { H2S = 0.0, O2 = 0.25, SO4 = 28.0, H_ION = 1.0e-4 }",
        "conc_out = { H_ION = 1.0e-4, SO4 = 28.0, O2 = 0.25, H2S = 0.0 }",
    );
    let text = swap(
        &text,
        "composition = { S = 1, charge = -2 }",
        "composition = { charge = -2, S = 1 }",
    );
    let text = swap(
        &text,
        "inputs  = { H2S = 1, O2 = 2 }",
        "inputs  = { O2 = 2, H2S = 1 }",
    );
    let text = swap(
        &text,
        "outputs = { SO4 = 1, H_ION = 2 }",
        "outputs = { H_ION = 2, SO4 = 1 }",
    );
    swap(
        &text,
        "km  = { H2S = 0.01, O2 = 0.01 }",
        "km  = { O2 = 0.01, H2S = 0.01 }",
    )
}

#[test]
fn whitespace_and_key_order_do_not_change_the_hash() {
    assert_eq!(
        hash_of("plain.toml", PLAIN),
        hash_of("scrambled.toml", SCRAMBLED),
    );

    // The half of the claim the pair above cannot carry: the keys of a map are
    // data, so the order they are typed in reaches the canonical bytes unless
    // the map orders itself by the key.
    assert_eq!(
        hash_of("scenario.toml", SCENARIO),
        hash_of("scenario_reordered.toml", &scenario_with_maps_reordered()),
    );
}

#[test]
fn changing_a_value_changes_the_hash() {
    let nudged = swap(PLAIN, "nz = 16", "nz = 17");
    assert_ne!(
        hash_of("plain_again.toml", PLAIN),
        hash_of("nudged.toml", &nudged)
    );
}

#[test]
fn an_unknown_field_is_an_error() {
    let typo = swap(PLAIN, "nz = 16", "nz = 16\nnw = 16");
    let err = config::load(&fixture("typo.toml", &typo))
        .expect_err("an unknown field must not be ignored silently");
    assert!(
        format!("{err:#}").contains("nw"),
        "the error should name the offending key, got: {err:#}",
    );
}

/// A typo inside an array of tables or a nested table has to be an error too.
///
/// `deny_unknown_fields` on `Config` alone would let a misspelt key inside
/// `[reaction.rate]` through, and a silently dropped line hashes exactly like
/// a correct file — the failure `an_unknown_field_is_an_error` exists to
/// prevent, one level down.
///
/// One case per nested type, and the roster is the point rather than the
/// thoroughness: the attribute is written per type, so a type missing from this
/// list is a type whose line can be deleted with nothing turning red.
/// `[[substance]]` is where that costs most — a key dropped there is a missing
/// `settling_radius` or `c_p`, not a cosmetic typo.
///
/// `Face` and `Scale` are absent because they are unit enums with no fields to
/// misspell; `Config` itself is covered by `an_unknown_field_is_an_error`.
#[test]
fn an_unknown_field_inside_a_nested_table_is_an_error() {
    // The line the typo is grafted onto, and the misspelt line to graft.
    const TYPOS: &[(&str, &str)] = &[
        ("nz = 64", "nw = 64"),                            // Grid
        ("z_min = \"closed\"", "z_mid = \"closed\""),      // Boundary
        ("k_ex = 1.0e-5", "k_in = 1.0e-5"),                // Reservoir
        ("molar_mass = 34.1", "molar_masss = 34.1"),       // Substance
        ("enthalpy = -8.46e5", "entalpy = -8.46e5"),       // Reaction
        ("field = \"enthalpy\"", "fields = \"enthalpy\""), // Requirement
        ("q10 = 2.0", "q20 = 2.0"),                        // Rate
        ("lod = 2", "lod2 = 2"),                           // Field
        ("l_c = 3.2e-3", "l_d = 3.2e-3"),                  // Process
        ("scale = \"linear\"", "scal = \"linear\""),       // Calibration
    ];

    for (anchor, typo) in TYPOS {
        let text = swap(SCENARIO, anchor, &format!("{anchor}\n{typo}"));
        let key = typo.split(' ').next().expect("a typo line names a key");
        let Err(err) = config::parse(&text) else {
            panic!("`{key}` is not a key of the schema and must not be ignored");
        };
        assert!(
            format!("{err:#}").contains(key),
            "the error should name the offending key `{key}`, got: {err:#}",
        );
    }
}

/// The hash is taken after defaults are applied, so omitting a field and
/// spelling out its default value are the same configuration.
///
/// The `[[process]]` record carries the two defaults that live one level down —
/// `every_n_ticks` (ADR-030) and `stir_fraction` (ADR-069) — and it is written
/// out because the root scalars cannot stand in for them: a key inside an array
/// of tables is defaulted per entry.
///
/// `enabled` is absent from *both* sides, and since ADR-065 that is no longer
/// an exception but the strongest case of the rule: the loader materialises the
/// whole roster before the hash, so the key arrives filled in from the process's
/// own module. `an_omitted_process_section_hashes_as_the_full_default_roster`
/// carries the same property one level up, for the section rather than the key.
#[test]
fn an_omitted_field_hashes_as_its_default() {
    let spelled_out = r#"
name = "defaults"
dt = 1.0
beta = 0.015625
T_ref = 298.15

substance = []
reaction = []
field = []
calibration = []

[conserved]
C  = 12.01070
N  = 14.00670
P  = 30.97376
S  = 32.06500
Fe = 55.84500

[grid]
nx = 64
ny = 64
nz = 64
dx = 1.0e-4

[boundary]
x_min = "periodic"
x_max = "periodic"
y_min = "periodic"
y_max = "periodic"
z_min = "closed"
z_max = "exchange"

[[process]]
id = "diffusion"
every_n_ticks = 1
stir_fraction = 0.0
"#;
    let omitted =
        "name = \"defaults\"\nT_ref = 298.15\n\n[grid]\n\n[[process]]\nid = \"diffusion\"\n";
    assert_eq!(
        hash_of("spelled_out.toml", spelled_out),
        hash_of("omitted.toml", omitted),
    );
}

/// The seed is a run parameter, not a scenario key (ADR-058).
///
/// `deny_unknown_fields` already makes the line an error; the test exists so
/// that this is a decision rather than a coincidence
/// (`CONFIG_SCHEMA.md` section 2), and so that a later `seed` field could not
/// quietly become a hashed key.
#[test]
fn seed_in_the_scenario_file_is_rejected() {
    let with_seed = swap(PLAIN, "dt = 1.0", "dt = 1.0\nseed = 42");
    let err = config::parse(&with_seed).expect_err("a seed in the scenario file must be rejected");
    assert!(
        format!("{err:#}").contains("seed"),
        "the error should name `seed`, got: {err:#}",
    );
}

/// The search driver reads `[[calibration]]`; the tick loop does not, so the
/// section stays out of the hash (ADR-038).
///
/// Were it in, a sweep over a calibrated parameter would hand every probe its
/// own run identity, and the gallery would lose the identity it shares.
#[test]
fn config_hash_ignores_the_calibration_section() {
    let cut = SCENARIO
        .find("[[calibration]]")
        .expect("the fixture declares a calibration section");
    let with = config::parse(SCENARIO).expect("the fixture must parse");
    let without =
        config::parse(&SCENARIO[..cut]).expect("the fixture minus its calibration must parse");

    // Without this the test would pass just as well against a schema that has
    // no calibration section at all: `deny_unknown_fields` would have refused
    // the section outright, and the two hashes would agree for the wrong
    // reason.
    assert_eq!(with.calibration.len(), 1, "the section must be parsed");
    assert!(without.calibration.is_empty());

    assert_eq!(
        config::config_hash(&with).expect("hashing config"),
        config::config_hash(&without).expect("hashing config"),
    );
}

/// The side of the layer is printed for every substance, including the ones the
/// file never mentions (ADR-077, ADR-065).
///
/// The argument is ADR-065's, one section over: without the materialisation an
/// author who wrote nothing sees the side of no substance at all, while the
/// identity of the run carries a claim that is in no file. The price is a line
/// per substance in the canonical form.
#[test]
fn the_canonical_form_names_the_layer_side_of_every_substance() {
    let config = config::parse(SCENARIO).expect("the fixture must parse");
    let canonical = config::canonical(&config).expect("canonical form");

    assert!(
        canonical.contains("[initial.layer]"),
        "the canonical form has no layer table:\n{canonical}"
    );
    for id in ["H2S", "O2", "SO4", "H_ION"] {
        assert!(
            canonical.contains(&format!("{id} = \"sediment\"")),
            "the canonical form does not name the side of `{id}`:\n{canonical}"
        );
    }

    // The fixture writes none of them, which is what makes the four lines above
    // a statement about the materialisation rather than about the file.
    assert!(
        !SCENARIO.contains("[initial"),
        "the fixture must not declare the section it is used to check"
    );
}

/// A side outside the enumeration is a load error (ADR-077).
///
/// Serde makes the refusal for nothing; the test is here so that it is a
/// decision rather than a coincidence — the precedent is
/// `seed_in_the_scenario_file_is_rejected` one section up. This is also where
/// the divergence from the frozen SPEC section 12.4 is pinned: it names three
/// layers, sediment, water and air, and the enumeration knows two sides and
/// "uniform". The world has no air, by a field or by a boundary, and a side no
/// process reads would be a value with no addressee (ADR-077, ADR-032).
#[test]
fn initial_layer_side_outside_the_enumeration_is_rejected() {
    let text = format!("{SCENARIO}\n[initial.layer]\nO2 = \"air\"\n");
    let err = config::parse(&text).expect_err("a side outside the enumeration must be rejected");
    let message = format!("{err:#}");
    assert!(
        message.contains("air"),
        "the error should name the value it refused, got: {message}"
    );
    assert!(
        message.contains("O2"),
        "the error should name the key it refused it under, or the author hunts \
         for the typo through the whole file, got: {message}"
    );
}

/// Every key path of a TOML tree, with array entries carrying their index.
///
/// `substance[0].molar_mass`, not `substance.molar_mass`. Folding the index
/// away would make the comparison blind to an array entry whose key set differs
/// from entry zero's — which is exactly the shape a projection that dropped one
/// key has.
fn key_paths(value: &toml::Value) -> BTreeSet<String> {
    fn walk(value: &toml::Value, prefix: &str, out: &mut BTreeSet<String>) {
        match value {
            toml::Value::Table(table) => {
                for (key, child) in table {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    out.insert(path.clone());
                    walk(child, &path, out);
                }
            }
            toml::Value::Array(items) => {
                for (i, child) in items.iter().enumerate() {
                    walk(child, &format!("{prefix}[{i}]"), out);
                }
            }
            _ => {}
        }
    }

    let mut out = BTreeSet::new();
    walk(value, "", &mut out);
    out
}

/// The section a path belongs to: everything before the first `.` or `[`.
fn root_of(path: &str) -> &str {
    &path[..path.find(['.', '[']).unwrap_or(path.len())]
}

/// The exhaustive destructuring in `config/hash.rs` refuses to compile against
/// a schema key nobody has decided about, but it cannot tell `field: x` from
/// `field: _` (ADR-066). This is what tells them apart.
#[test]
fn the_projection_covers_every_simulated_key() {
    let config = config::parse(SCENARIO).expect("the fixture must parse");

    let whole = key_paths(&toml::Value::try_from(&config).expect("serializing the config"));
    // Read back from the canonical bytes rather than serializing the projection
    // a second time: what is compared is what blake3 actually sees.
    let canonical: toml::Value =
        toml::from_str(&config::canonical(&config).expect("canonical form"))
            .expect("the canonical form must be TOML");
    let hashed = key_paths(&canonical);

    let missing: Vec<&String> = whole.difference(&hashed).collect();

    // A flat `missing == NOT_HASHED` cannot be written: on this fixture the
    // difference is `calibration` plus its four leaves — five entries against
    // one entry of the constant. So the two directions are asserted apart.
    for path in &missing {
        assert!(
            config::NOT_HASHED.contains(&root_of(path)),
            "`{path}` is read by the simulator but absent from the canonical \
             form, and `{}` is not named by NOT_HASHED",
            root_of(path),
        );
    }
    for section in config::NOT_HASHED {
        assert!(
            missing.iter().any(|path| root_of(path) == *section),
            "NOT_HASHED names `{section}`, but the canonical form of the \
             fixture keeps every key under it",
        );
    }
}

/// The canonical form drops `[[calibration]]` on purpose, and stays readable.
#[test]
fn the_canonical_form_reloads_to_the_same_hash() {
    let config = config::parse(SCENARIO).expect("the fixture must parse");
    let canonical = config::canonical(&config).expect("canonical form");
    let reloaded = config::parse(&canonical).expect("the canonical form must load back");

    assert_eq!(
        config::config_hash(&config).expect("hashing config"),
        config::config_hash(&reloaded).expect("hashing config"),
    );

    // Stated rather than implied: the loss is the point (ADR-066). The
    // canonical form's job is to be hashed, not to be read back as a scenario.
    assert!(reloaded.calibration.is_empty());
}

/// Nothing else in the repository opens `configs/scenarios/*.toml`.
///
/// CI runs `cargo test` and no step runs the binary, so a newly required key
/// breaks the command printed in `README.md`, `CLAUDE.md` and the bug-report
/// template without failing a single check. This is that check.
#[test]
fn every_scenario_in_the_repository_loads() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../configs/scenarios");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("configs/scenarios must exist") {
        let path = entry.expect("reading configs/scenarios").path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            if let Err(err) = config::load(&path) {
                panic!("{} does not load: {err:#}", path.display());
            }
            seen += 1;
        }
    }
    assert!(seen > 0, "no scenario found in {}", dir.display());
}

/// The full roster of ADR-065, spelled out as a scenario would have to spell it.
///
/// Nine records, each with the `enabled` its module declares. Written by hand
/// rather than generated from `default_roster()`, because a fixture built out of
/// the thing under test asserts only that the thing agrees with itself: this text
/// is what a user would have to type, and it goes red when a default changes —
/// which is the moment ADR-065 wants visible, since it moves `config_hash` for
/// every scenario in existence.
const FULL_ROSTER: &str = r#"
[[process]]
id = "light"
enabled = false

[[process]]
id = "velocity_field"
enabled = false

[[process]]
id = "advection"
enabled = false

[[process]]
id = "diffusion"
enabled = true

[[process]]
id = "pressure"
enabled = false

[[process]]
id = "settling"
enabled = false

[[process]]
id = "phase_transitions"
enabled = false

[[process]]
id = "reactions"
enabled = false

[[process]]
id = "external_channels"
enabled = false
"#;

/// The minimum a scenario has to write to load at all.
const BARE: &str = r#"
name = "roster"
T_ref = 298.15

[grid]
"#;

#[test]
fn an_omitted_process_section_hashes_as_the_full_default_roster() {
    // Section 11 item 2 of `CONFIG_SCHEMA.md`, applied to a section whose full
    // membership is not in the file: defaults are applied *before* hashing, so
    // "wrote nothing" and "wrote every default out" are one configuration
    // (ADR-065). The variant that had to be rejected — hash what was written,
    // execute what was materialised — differs from this line and from nothing
    // else.
    assert_eq!(
        hash_of("roster_omitted.toml", BARE),
        hash_of("roster_spelled_out.toml", &format!("{BARE}{FULL_ROSTER}")),
    );
}

#[test]
fn the_canonical_form_names_every_process_in_the_roster() {
    // The only thing backing ADR-065's promise that "it is visible what actually
    // came out". Without it the user who forgot a record sees nothing at all —
    // which is the same record's argument for `--print-canonical` on the CLI.
    let config = config::load(&fixture("roster_named.toml", BARE)).expect("loading");
    let canonical = config::canonical(&config).expect("canonical form");
    for id in [
        "light",
        "velocity_field",
        "advection",
        "diffusion",
        "pressure",
        "settling",
        "phase_transitions",
        "reactions",
        "external_channels",
    ] {
        assert!(
            canonical.contains(&format!("id = \"{id}\"")),
            "the canonical form does not name `{id}`:\n{canonical}"
        );
    }
}

#[test]
fn the_process_order_in_the_canonical_form_does_not_depend_on_the_file() {
    // Two files naming the same processes in different orders describe one world.
    // Nothing else in this file sees it: `whitespace_and_key_order_do_not_change_the_hash`
    // reorders the keys *inside* a table and never the entries of an array of
    // tables, and the natural materialisation — append the missing records to the
    // tail, keep the file's order for the rest — passes that test and fails this
    // one.
    let forwards =
        format!("{BARE}\n[[process]]\nid = \"diffusion\"\n\n[[process]]\nid = \"light\"\n");
    let backwards =
        format!("{BARE}\n[[process]]\nid = \"light\"\n\n[[process]]\nid = \"diffusion\"\n");
    assert_eq!(
        hash_of("roster_forwards.toml", &forwards),
        hash_of("roster_backwards.toml", &backwards),
    );
}
