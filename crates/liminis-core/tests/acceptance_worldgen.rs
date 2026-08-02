//! Acceptance criteria of the initial conditions (`ACCEPTANCE.md`, S0).
//!
//! ```text
//! same_seed_gives_the_same_initial_state
//! different_seed_gives_a_different_initial_state
//! worldgen_respects_declared_max_conc
//! ```
//!
//! Plus three that no record names and that nothing else can see: that the front
//! buffer holds the generated state at the end (ADR-057), that a lane is not a
//! substance index (ADR-056), and that a derivation out of step with the registry
//! is refused rather than silently swapping two substances.
//!
//! An integration test rather than a unit one, for the reason
//! `tests/acceptance_diffusion.rs` gives: the outside view may use only what a
//! scenario can use — the public surface of `liminis-core` — so it keeps its
//! meaning when the inside is rearranged. Here it also decides what can be
//! tested at all: `config::Derived` is built by `config::derive` and by nothing
//! else, so every claim about *declared* quantities needs a scenario that
//! actually loads.
//!
//! # The fixture, and the one way it differs from the tick fixture
//!
//! No file under `configs/` will do — `hello.toml` declares no substance and
//! does not survive its own derivation (`config::load` says so at length) — and
//! the worked example of `CONFIG_SCHEMA.md` section 12 lives as a private string
//! inside `config/validate.rs`. So the scenario is written out here, on the model
//! of `tests/acceptance_tick.rs`: section 12 plus `WATER`, declared **second**,
//! so that a lane is never a substance index (ADR-056).
//!
//! What is different here is the declared concentrations, and it is not
//! decoration. `every_lane_is_written_and_none_is_addressed_by_substance_index`
//! reads an amount and asks *whose* it is, so the amount a substance is filled
//! with has to identify it. The band of a substance is centred on
//! `2^28 * c_typ/c_max` (ADR-039), and in the tick fixture `H2S` and `H_ION`
//! declare the same ratio and land on the same band — under which the test would
//! be green with the two lanes exchanged. The five ratios here are a decade
//! apart, so the five bands are pairwise disjoint, and the test asserts that
//! before it asserts anything else.
//!
//! **The numbers marked as placeholders are placeholders.** `c_p`,
//! `enthalpy_formation` and `partial_molar_volume` are declared for no substance
//! anywhere in the corpus (`CONFIG_SCHEMA.md` section 13 item 23). Do not copy
//! this into `configs/`.

use liminis_core::config;
use liminis_core::numeric::run_key;
use liminis_core::world::{Boundary, Grid, LaneRef, Registry, SubstanceDecl, World, WorldLayout};
use liminis_core::worldgen::generate;

/// Substance indices of the fixture, in declaration order.
const H2S: u32 = 0;
const WATER: u32 = 1;
const O2: u32 = 2;
const SO4: u32 = 3;
const H_ION: u32 = 4;
const N_SUBSTANCES: u32 = 5;

/// The edge of the test domain.
///
/// Sixteen and not four. The lattice of octave `k` has `2^(k+1)` nodes per axis
/// and a spacing of `n >> (k+1)` voxels, so an edge of four carries exactly one
/// octave and an edge of sixteen carries three — and a single octave is the one
/// state under which "the field has structure larger than a voxel" cannot fail.
/// It is also divisible by `2^2`, which the enthalpy grid at `lod = 2` needs.
const N: u32 = 16;

/// A scenario with matter, chemistry and an enthalpy field, on the model of
/// `tests/acceptance_tick.rs`. See the module header for what the
/// concentrations are chosen for.
const SCENARIO: &str = r#"
name = "worldgen-fixture"
dt = 1.0
beta = 0.015625
T_ref = 298.15

[conserved]
C = 12.01070
N = 14.00670
P = 30.97376
S = 32.06500
Fe = 55.84500

[grid]
nx = 16
ny = 16
nz = 16
dx = 1.0e-4

[boundary]
x_min = "closed"
x_max = "closed"
y_min = "closed"
y_max = "closed"
z_min = "closed"
z_max = "closed"

[[substance]]
id = "H2S"
molar_mass = 34.08088
typical_conc = 0.1
max_conc = 0.2
partial_molar_volume = 3.5e-5
settling_radius = 0.0
diffusivity = 1.6e-9
c_p = 100.0
enthalpy_formation = 0.0
composition = { S = 1 }

[[substance]]
id = "WATER"
molar_mass = 18.01528
typical_conc = 55000.0
max_conc = 55600.0
partial_molar_volume = 1.8e-5
settling_radius = 0.0
diffusivity = 2.3e-9
c_p = 75.3
enthalpy_formation = -285830.0
composition = {}

[[substance]]
id = "O2"
molar_mass = 31.99880
typical_conc = 0.05
max_conc = 1.0
partial_molar_volume = 3.1e-5
settling_radius = 0.0
diffusivity = 2.1e-9
c_p = 101.0
enthalpy_formation = 0.0
composition = {}

[[substance]]
id = "SO4"
molar_mass = 96.06260
typical_conc = 0.5
max_conc = 100.0
partial_molar_volume = 1.4e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 102.0
enthalpy_formation = -846000.0
composition = { S = 1 }

[[substance]]
id = "H_ION"
molar_mass = 1.007940
typical_conc = 1.0e-4
max_conc = 0.2
partial_molar_volume = 0.0
settling_radius = 0.0
diffusivity = 9.3e-9
c_p = 103.0
enthalpy_formation = 0.0
composition = {}

[[reaction]]
id = "h2s_oxidation"
enthalpy = -846000.0
catalyst = ""
energy_from = ""
inputs = { H2S = 1, O2 = 2 }
outputs = { SO4 = 1, H_ION = 2 }

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = { H2S = 0.01, O2 = 0.01 }

[[reaction]]
id = "water_formation"
enthalpy = -571660.0
catalyst = ""
energy_from = ""
inputs = { H_ION = 4, O2 = 1 }
outputs = { WATER = 2 }

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = { H_ION = 0.01, O2 = 0.01 }

[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15
"#;

/// Parse, validate and derive the fixture.
fn derived() -> config::Derived {
    let config = config::parse(SCENARIO).expect("the fixture must parse");
    config::validate(&config).expect("the fixture must validate")
}

/// A world on the fixture's registry: periodic in X and Y, closed floor and lid.
///
/// Periodic and not closed, unlike the tick fixture: the lattice of the noise
/// wraps in X and Y by construction, and a domain that did not would make the
/// wrap untestable from outside.
fn world(derived: &config::Derived) -> World {
    let grid = Grid::new(
        N,
        N,
        N,
        [
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Closed,
            Boundary::Closed,
        ],
    )
    .expect("the grid");
    let registry = Registry::new(&derived.decls()).expect("the registry");
    World::new(
        grid,
        registry,
        &WorldLayout {
            enthalpy_lod: 2,
            velocity_lod: 1,
        },
    )
    .expect("the world")
}

/// State `N` of one substance, widened so that both widths compare in one text.
///
/// Through [`World::lane_of`] and nowhere else: the substance index is not the
/// lane, and a test that computed the address itself would agree with a wrong
/// implementation by construction (ADR-056).
fn front(world: &World, s: u32) -> Vec<i128> {
    match world.lane_of(s) {
        LaneRef::Narrow(lane) => world
            .amounts_32()
            .expect("the narrow field")
            .lane(lane)
            .iter()
            .map(|m| i128::from(m.raw()))
            .collect(),
        LaneRef::Wide(lane) => world
            .amounts_64()
            .expect("the wide field")
            .lane(lane)
            .iter()
            .map(|m| i128::from(m.raw()))
            .collect(),
    }
}

/// Both bounds of the band a substance is filled inside, from the report.
fn band(report: &liminis_core::worldgen::WorldgenReport, s: u32) -> (i128, i128) {
    let fill = &report.per_substance[s as usize];
    (
        fill.amount_at_typical - fill.excursion,
        fill.amount_at_typical + fill.excursion,
    )
}

// ---------------------------------------------------------------------------
// same_seed_gives_the_same_initial_state
// ---------------------------------------------------------------------------

/// Two worlds, one derivation, one run key: equal to the byte.
///
/// Every buffer, not only the amounts. What this fails on is anything that
/// reached the fill other than the four counters of `rand` — an iteration over a
/// `HashMap`, an address, a clock, a buffer that was never written, a call into a
/// generator that carries state. None of those is visible in a single run; all of
/// them are visible in two.
#[test]
fn same_seed_gives_the_same_initial_state() {
    const SEED: u64 = 42;

    let derived = derived();
    let mut a = world(&derived);
    let mut b = world(&derived);

    // The seed is folded on the host, once, and arrives as the fourth counter
    // (ADR-058). That `generate` takes the key and not the seed is asserted by
    // this line and by the compiler: `run_key` returns a `u32`, and a `u64`
    // parameter would not accept it without a cast this file does not write.
    let key: u32 = run_key(SEED);
    generate(&mut a, &derived, key).expect("the fixture must generate");
    generate(&mut b, &derived, key).expect("the fixture must generate");

    assert_eq!(a.amounts_32(), b.amounts_32(), "the narrow amounts");
    assert_eq!(a.amounts_64(), b.amounts_64(), "the wide amounts");
    assert_eq!(a.enthalpy(), b.enthalpy(), "the enthalpy field");
    assert_eq!(a.energy_delta(), b.energy_delta(), "the energy accumulator");
    assert_eq!(a.light(), b.light(), "the light field");
    assert_eq!(a.velocity(), b.velocity(), "the velocity field");
    assert_eq!(
        a.velocity_potential(),
        b.velocity_potential(),
        "the velocity potential"
    );

    // And the fold happens on the host rather than inside: a world generated
    // from the low half of the seed is not the world generated from its key. If
    // `generate` folded the seed a second time, these two would agree.
    let mut folded_twice = world(&derived);
    generate(&mut folded_twice, &derived, SEED as u32).expect("must generate");
    assert_ne!(
        a.amounts_32(),
        folded_twice.amounts_32(),
        "`generate` folds the seed itself, which would put the fold in two places"
    );
}

// ---------------------------------------------------------------------------
// different_seed_gives_a_different_initial_state
// ---------------------------------------------------------------------------

/// A statement about **one pair** of keys, not a theorem.
///
/// ADR-058 collapses a 64-bit seed into 32 bits and prices the collisions: about
/// 1.2% somewhere in a gallery of ten thousand seeds. A quantifier over seeds
/// would therefore be false, and the honest form is the one this test takes.
///
/// Three parts, and the middle one is the reason the first is not enough.
#[test]
fn different_seed_gives_a_different_initial_state() {
    let derived = derived();
    let mut a = world(&derived);
    let mut b = world(&derived);

    let key_a = run_key(1);
    let key_b = run_key(2);
    generate(&mut a, &derived, key_a).expect("must generate");
    let report_b = generate(&mut b, &derived, key_b).expect("must generate");

    // (a) A share of the domain, not "the fields differ". A single disagreeing
    // voxel passes `assert_ne!`, and the draw is independent per voxel, so the
    // share is obliged to sit near one. The band is set the way
    // `different_seed_changes_the_rounding_on_identical_state` sets its own —
    // wide enough that nothing but a key which never arrives can fail it.
    // Observed on this fixture: 1.000.
    let left = front(&a, H2S);
    let right = front(&b, H2S);
    let diverged = left
        .iter()
        .zip(right.iter())
        .filter(|(x, y)| x != y)
        .count();
    let share = diverged as f64 / left.len() as f64;
    assert!(
        (0.90..=1.0).contains(&share),
        "{diverged} of {} voxels differ under two run keys",
        left.len()
    );

    // (b) The multisets differ, and this is the part that catches the variant
    // ADR-058 rejected in as many words: a key folded into the first counter
    // gives the same numbers with the voxels permuted, under which (a) is green
    // and the two runs are one run under two names.
    let mut sorted_left = left.clone();
    let mut sorted_right = right.clone();
    sorted_left.sort_unstable();
    sorted_right.sort_unstable();
    assert_ne!(
        sorted_left, sorted_right,
        "the same numbers with the voxels permuted is one run under two names \
         (ADR-058, rejected)"
    );

    // (c) And the second key is inside the ceiling too, or "different" would be
    // reachable by walking out of the declared range.
    for s in 0..N_SUBSTANCES {
        let ceiling = report_b.per_substance[s as usize].amount_at_max;
        for (idx, amount) in front(&b, s).into_iter().enumerate() {
            assert!(
                (0..=ceiling).contains(&amount),
                "substance {s}, voxel {idx}: {amount} outside [0, {ceiling}]"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// worldgen_respects_declared_max_conc
// ---------------------------------------------------------------------------

/// `max_conc` is the hard ceiling of a run (ADR-041, `CONFIG_SCHEMA.md`
/// section 5), not an estimate, and a world that starts above it is a wrong
/// start rather than an initial condition.
///
/// Nothing in `process/` checks the ceiling at run time yet — the debug assert
/// and the release check on the tick boundary ADR-041 promises are not written —
/// so this test is the only thing standing between a scenario and an overflow
/// that surfaces somewhere else entirely.
///
/// The three assertions after the ceiling are against a vacuous pass. A constant
/// field satisfies the ceiling, and so does a field that was clamped to it.
#[test]
fn worldgen_respects_declared_max_conc() {
    let derived = derived();
    let mut world = world(&derived);
    let report = generate(&mut world, &derived, run_key(7)).expect("must generate");

    for s in 0..N_SUBSTANCES {
        let fill = &report.per_substance[s as usize];
        let amounts = front(&world, s);
        assert_eq!(amounts.len(), (N * N * N) as usize);

        for (idx, &amount) in amounts.iter().enumerate() {
            assert!(
                (0..=fill.amount_at_max).contains(&amount),
                "{}: voxel {idx} holds {amount}, outside [0, {}]",
                fill.id,
                fill.amount_at_max
            );
        }

        let lowest = *amounts.iter().min().expect("a non-empty domain");
        let highest = *amounts.iter().max().expect("a non-empty domain");

        // (a) The field is not constant, or the ceiling was checked against
        // nothing.
        assert!(
            lowest < fill.amount_at_typical && highest > fill.amount_at_typical,
            "{}: the domain sits at [{lowest}, {highest}] around a typical of {}",
            fill.id,
            fill.amount_at_typical
        );

        // (b) No voxel sits exactly on the ceiling. A plateau there is the
        // signature of a clamp, and ADR-041 forbids that in as many words: a
        // product past the declared limit means a wrong declaration, and the run
        // is to fall over loudly rather than saturate quietly.
        assert!(
            highest < fill.amount_at_max,
            "{}: {highest} is the ceiling itself, which is what a clamp looks like",
            fill.id
        );

        // (c) And the ceiling is not merely missed by the width of a rounding.
        // The rule that makes this true is not stated here and must not be:
        // `TODO(worldgen-excursion)` in `worldgen/mod.rs` carries it, together
        // with the reason it is a `TODO` — no `[initial]` section declares how far
        // an initial condition may wander (`CONFIG_SCHEMA.md` section 13). What is
        // asserted here is the weaker consequence any such rule owes: a quarter of
        // the headroom of clearance, so that "inside the ceiling" cannot be
        // satisfied by landing next to it.
        let headroom = fill.amount_at_max - fill.amount_at_typical;
        assert!(
            highest <= fill.amount_at_typical + headroom * 3 / 4,
            "{}: {highest} is within a rounding of the ceiling {}",
            fill.id,
            fill.amount_at_max
        );
    }
}

// ---------------------------------------------------------------------------
// the process boundary and the lane
// ---------------------------------------------------------------------------

/// ADR-057: on completion the front buffer holds state `N` for **every** lane.
///
/// Two silent failures live here and no invariant sees either. A forgotten swap
/// leaves the generated world in the write buffer and the front buffer at zero:
/// the world is empty, both invariants hold exactly, the residual is zero
/// forever, and the picture is black in a way that explains itself. A swap
/// *inside* the loop over lanes leaves half the lanes at zero by parity, and
/// zero is a legal amount (`world/field.rs` says so), so nothing falls over — the
/// world simply does not contain half of its chemistry.
///
/// The assertion is exact rather than statistical because the fill has a floor:
/// every amount is at least `typical - excursion`, which is positive for every
/// substance of the fixture, so a zero in the front buffer can only be an
/// unwritten voxel.
#[test]
fn the_front_buffer_holds_the_generated_state() {
    let derived = derived();
    let mut world = world(&derived);
    let report = generate(&mut world, &derived, run_key(11)).expect("must generate");

    for s in 0..N_SUBSTANCES {
        let floor = report.per_substance[s as usize].floor;
        assert!(
            floor > 0,
            "the fixture must have a positive floor to assert on"
        );
        for (idx, amount) in front(&world, s).into_iter().enumerate() {
            assert!(
                amount >= floor,
                "substance {s}, voxel {idx}: the front buffer holds {amount}, \
                 below the floor {floor} of the generated field"
            );
        }
    }

    // And the write buffer is what it was: zeroed at construction, promoted
    // nowhere. If the fill had written there and swapped twice, this is where it
    // would show.
    for s in 0..N_SUBSTANCES {
        match world.lane_of(s) {
            LaneRef::Narrow(lane) => {
                let field = world.amounts_32_mut().expect("the narrow field");
                let n_voxels = field.n_voxels() as usize;
                let start = lane as usize * n_voxels;
                assert!(
                    field.write_mut()[start..start + n_voxels]
                        .iter()
                        .all(|m| m.raw() == 0),
                    "substance {s} left something in the write buffer"
                );
            }
            LaneRef::Wide(lane) => {
                let field = world.amounts_64_mut().expect("the wide field");
                let n_voxels = field.n_voxels() as usize;
                let start = lane as usize * n_voxels;
                assert!(
                    field.write_mut()[start..start + n_voxels]
                        .iter()
                        .all(|m| m.raw() == 0),
                    "substance {s} left something in the write buffer"
                );
            }
        }
    }
}

/// A lane is not a substance index (ADR-056), and on this fixture the difference
/// is visible in the numbers themselves.
///
/// `WATER` is declared second and is the only wide substance, so it takes lane 0
/// of the wide field while the four narrow substances take lanes 0..3 of the
/// narrow one — every narrow substance after `H2S` is off by one from its index.
/// The five bands are a decade apart (see the module header), so an amount says
/// whose it is.
///
/// What this fails on: `s * n_voxels + idx`, and `amounts_32_mut()` reached with
/// a substance index where a lane belongs. Neither is visible in a ledger — the
/// wrong thing is read and written, and nothing is lost.
#[test]
fn every_lane_is_written_and_none_is_addressed_by_substance_index() {
    let derived = derived();
    let mut world = world(&derived);
    let report = generate(&mut world, &derived, run_key(13)).expect("must generate");

    // The precondition the whole test rests on: no two substances could be
    // mistaken for one another by their amounts.
    for s in 0..N_SUBSTANCES {
        for t in (s + 1)..N_SUBSTANCES {
            let (low_s, high_s) = band(&report, s);
            let (low_t, high_t) = band(&report, t);
            assert!(
                high_s < low_t || high_t < low_s,
                "the bands of substances {s} and {t} overlap, so the fixture \
                 cannot tell one lane from the other"
            );
        }
    }

    for s in 0..N_SUBSTANCES {
        let (low, high) = band(&report, s);
        for (idx, amount) in front(&world, s).into_iter().enumerate() {
            assert!(
                (low..=high).contains(&amount),
                "substance {s}, voxel {idx}: {amount} is outside its own band \
                 [{low}, {high}] — it belongs to some other substance"
            );
        }
    }
}

/// Two orders that agree only because nothing has had a reason to reorder either.
///
/// `derive.rs` says as much about the substance order of a `Config` and of a
/// `Derived`. Without a check the disagreement is silent: the amounts of one
/// substance land on the lane of another, the ceiling is compared against the
/// wrong limit, and both invariants close.
#[test]
fn a_derivation_out_of_step_with_the_registry_is_refused() {
    let derived = derived();

    // The same substances, declared to the registry in the opposite order. Every
    // width and every scale is still right; only the order is not.
    let mut decls: Vec<SubstanceDecl> = derived.decls();
    decls.reverse();

    let grid = Grid::new(N, N, N, [Boundary::Closed; 6]).expect("the grid");
    let registry = Registry::new(&decls).expect("the registry");
    let mut world = World::new(
        grid,
        registry,
        &WorldLayout {
            enthalpy_lod: 2,
            velocity_lod: 1,
        },
    )
    .expect("the world");

    let error = generate(&mut world, &derived, run_key(17))
        .expect_err("a derivation in a different order must be refused");
    let text = format!("{error:#}");
    assert!(
        text.contains("H2S") && text.contains("H_ION"),
        "the message must name both identifiers: {text}"
    );
    assert!(
        text.contains('0'),
        "the message must name the index they disagree at: {text}"
    );
}

/// The report is printed at load, which is the only reason a degenerate fill is
/// distinguishable from a working one.
#[test]
fn the_report_names_every_substance_and_the_octaves() {
    let derived = derived();
    let mut world = world(&derived);
    let report = generate(&mut world, &derived, run_key(19)).expect("must generate");

    assert_eq!(report.per_substance.len(), N_SUBSTANCES as usize);
    assert_eq!(report.octaves, 3, "16 voxels an axis carry three octaves");

    let text = report.report();
    for s in [H2S, WATER, O2, SO4, H_ION] {
        let id = &report.per_substance[s as usize].id;
        assert!(text.contains(id.as_str()), "the report omits {id}: {text}");
    }
    assert!(
        text.contains("octaves"),
        "the report omits the octave count"
    );
}
