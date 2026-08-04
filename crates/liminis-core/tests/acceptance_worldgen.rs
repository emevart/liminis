//! Acceptance criteria of the initial conditions (`ACCEPTANCE.md`, S0).
//!
//! ```text
//! same_seed_gives_the_same_initial_state
//! different_seed_gives_a_different_initial_state
//! worldgen_respects_declared_max_conc
//! layer_sides_are_anticorrelated_across_the_boundary
//! the_layer_side_does_not_change_with_the_seed
//! a_substance_declared_uniform_has_no_layer_step
//! the_initial_band_keeps_every_substance_strictly_above_zero
//! the_written_default_and_the_omitted_section_give_one_world
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
use liminis_core::worldgen::{WorldgenReport, generate};

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
    derived_from(SCENARIO)
}

/// The same for a scenario the caller has rewritten.
fn derived_from(text: &str) -> config::Derived {
    let config = config::parse(text).expect("the fixture must parse");
    config::validate(&config).expect("the fixture must validate")
}

/// The fixture with an `[initial.layer]` table appended (ADR-077).
///
/// Appended and not woven in: the section is a root table, so it may stand after
/// the last `[[field]]` record, and a fixture that spelled it into the middle
/// would have to be moved again every time the scenario grows.
fn with_layers(sides: &str) -> String {
    format!("{SCENARIO}\n[initial.layer]\n{sides}")
}

/// The run key ADR-077 measured the two branches of `amount_at` on.
///
/// A run key and not a seed: `generate` takes the fourth counter already folded
/// (ADR-058), so the number that identifies a world here is this one.
const LAYER_KEY: u32 = 99;

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
fn band(report: &WorldgenReport, s: u32) -> (i128, i128) {
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
    // Three buffers and not seven: `energy_delta`, the light field, `u` and its
    // potential left `World` with ADR-086, because none of them has a reader
    // standing earlier in the tick than its writer. `worldgen` never wrote any of
    // the four, so comparing them here was comparing two zeroed buffers.

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
        // The clearance is half of the headroom rather than a quarter, and it is
        // the ratified rule rather than a threshold chosen here: ADR-077 turns
        // `excursion = min(typical, max - typical)/2` into a consequence, and out
        // of the single inequality `2*excursion <= min(typical, max - typical)`
        // the peak obeys `2*(typical + excursion) <= typical + max` exactly. The
        // quarter this line used to carry was written twice in the corpus and
        // decided by nobody; the unit form of the same bound is
        // `the_band_is_bounded_by_both_headrooms`.
        let headroom = fill.amount_at_max - fill.amount_at_typical;
        assert!(
            highest <= fill.amount_at_typical + headroom / 2,
            "{}: {highest} is within a rounding of the ceiling {}",
            fill.id,
            fill.amount_at_max
        );
    }
}

// ---------------------------------------------------------------------------
// the layer side of a substance
// ---------------------------------------------------------------------------

/// How much more of substance `s` the water column holds than the sediment, in
/// units, as the difference of the two group means.
///
/// The two groups are told apart by `H2S` and never by the relief, which is
/// private to `worldgen/` — a test that recomputed it would agree with a wrong
/// implementation by construction. For a substance on a side, the sign of
/// `amount - amount_at_typical` **is** the sign of the layer term: the blend is
/// `(layer + noise)/2` with a layer term of amplitude `UNIT` and `|noise| <=
/// UNIT`, so a voxel above its typical amount cannot be a water voxel. The
/// voxels sitting exactly on the typical amount are the ties that bound allows,
/// and they belong to neither group.
///
/// Every fixture below therefore leaves `H2S` on the default side, and the
/// caller that wants to know whether the partition means anything asks for the
/// step of `H2S` itself.
fn layer_step(world: &World, report: &WorldgenReport, s: u32) -> f64 {
    let marker = front(world, H2S);
    let typical = report.per_substance[H2S as usize].amount_at_typical;
    let amounts = front(world, s);

    let (mut sediment, mut n_sediment) = (0i128, 0i128);
    let (mut water, mut n_water) = (0i128, 0i128);
    for (idx, &here) in marker.iter().enumerate() {
        match here.cmp(&typical) {
            std::cmp::Ordering::Greater => {
                sediment += amounts[idx];
                n_sediment += 1;
            }
            std::cmp::Ordering::Less => {
                water += amounts[idx];
                n_water += 1;
            }
            std::cmp::Ordering::Equal => {}
        }
    }
    assert!(
        n_sediment > 0 && n_water > 0,
        "the partition put every voxel on one side: {n_sediment} sediment, \
         {n_water} water"
    );
    water as f64 / n_water as f64 - sediment as f64 / n_sediment as f64
}

/// The lowest and the highest amount of substance `s` over one plane of `z`.
///
/// A plane and not a group, because the two callers below want the one thing
/// [`layer_step`] cannot give: a region of space named by the axis rather than by
/// the field. Every measurement taken through the partition is relative — the
/// groups are read off `H2S`'s own deviation — and relative measurements are
/// invariant under swapping the two ends of the world.
fn plane_extremes(world: &World, s: u32, plane: u32) -> (i128, i128) {
    let grid = *world.grid();
    let amounts = front(world, s);

    let mut lowest = i128::MAX;
    let mut highest = i128::MIN;
    for (idx, &amount) in amounts.iter().enumerate() {
        let (_, _, z) = grid.coords(idx as u32);
        if z == plane {
            lowest = lowest.min(amount);
            highest = highest.max(amount);
        }
    }
    assert!(lowest <= highest, "the plane z = {plane} is empty");
    (lowest, highest)
}

/// The floor of the domain: the one plane that is sediment in every column, under
/// every key, on every grid the generator accepts.
///
/// The relief is confined so that each of the two layers keeps at least a voxel
/// of the domain — a boundary sitting on the floor or on the lid leaves one layer
/// out of the world entirely, and `worldgen/` says so where it draws the surface.
/// So the boundary is at height one or more and `z = 0` is under it, whatever the
/// noise did. The weakest form of that promise on purpose: a test that assumed
/// the two planes this fixture actually has to spare would go red on a change to
/// the confinement that breaks nothing.
const FLOOR_PLANE: u32 = 0;

/// And the lid, by the same argument from the other end: the boundary is at
/// `nz - 2` or lower, so the topmost plane is water in every column.
const LID_PLANE: u32 = N - 1;

/// A substance declared `uniform` has no layer term at all, and keeps its own
/// noise at full amplitude (ADR-077).
///
/// Four assertions, and only the last one sees the rejected `blended = noise/2`
/// — "the side is off, keep the amplitude". That form has no step across the
/// boundary either, so (b) does not separate it from the accepted one; and it
/// narrows the span exactly as removing the layer term does, so (c) is green
/// under it too, a halved band still being narrower than a sided one. What
/// separates them is (d), which compares the two branches where the layer term
/// is present but *constant*, and there the accepted form is twice the other by
/// construction.
///
/// No number of the fixture is written down here. ADR-077 refuses to print the
/// four bounds it measured and says why: they depend on the truncation towards
/// zero in `amount_at`, and a number printed from a model rather than from a run
/// disagreed with the generator by units. Printed here it would become a
/// blessing for today's code for ever. (d) is a ratio of two runs of the same
/// generator for the same reason: the factor is the one the branch divides by,
/// and neither spread is a number this file claims to know.
#[test]
fn a_substance_declared_uniform_has_no_layer_step() {
    let derived = derived_from(&with_layers("O2 = \"uniform\"\n"));
    let mut filled = world(&derived);
    let report = generate(&mut filled, &derived, LAYER_KEY).expect("must generate");

    // (a) The partition is worth something: the fixture's layered substance has
    // a step across it, of the order of its own band.
    let layered = layer_step(&filled, &report, H2S);
    let band_h2s = report.per_substance[H2S as usize].excursion as f64;
    assert!(
        layered.abs() >= band_h2s / 2.0,
        "H2S is layered and its step across the boundary is {layered}, under \
         half of its excursion {band_h2s} — the partition is measuring noise"
    );

    // (b) Oxygen, declared `uniform`, has none.
    //
    // A quarter of the band and not zero, and the threshold is taken from (a)
    // rather than fitted: a side has to show at least half of a band, so half of
    // that floor separates the two by a factor of two and no layer term can hide
    // under it. A substance without one still steps a little across this
    // partition, and it is not a leftover layer — the groups are regions of
    // space, the substance's own octaves carry structure at the scale of the
    // domain, and the mean of a smooth field over half a domain is not its mean
    // over the whole. What the assertion has to exclude is a term of the order
    // of the band itself.
    let flat = layer_step(&filled, &report, O2);
    let band_o2 = report.per_substance[O2 as usize].excursion as f64;
    assert!(
        flat.abs() <= band_o2 / 4.0,
        "O2 is declared uniform and still steps by {flat} across the boundary, \
         over a quarter of its excursion {band_o2}"
    );

    // (c) And it takes a narrower part of its band than the same substance takes
    // under a side, on the same key: the layer term is a square wave of
    // amplitude `UNIT` and the substance's own noise is not.
    let sided = derived_from(&with_layers("O2 = \"sediment\"\n"));
    let mut other = world(&sided);
    let with_side = generate(&mut other, &sided, LAYER_KEY).expect("must generate");
    let span = |report: &WorldgenReport| {
        let fill = &report.per_substance[O2 as usize];
        fill.peak - fill.floor
    };
    assert!(
        span(&report) < span(&with_side),
        "O2 spans {} under `uniform` and {} under `sediment`; the uniform branch \
         is supposed to take the smaller part of the band",
        span(&report),
        span(&with_side)
    );

    // (d) And the noise it is left with is the whole of its own, not half of it.
    //
    // Measured over the floor of the domain, where the two runs differ by exactly
    // one thing: the layered one carries a layer term of `+UNIT` in every voxel
    // of that plane and the uniform one carries no layer term at all. The
    // substance's noise is the same field in both — `rand` is a hash of four
    // counters, so the stream of a slot does not know which branch reads it, and
    // the two runs share the key, the slot and the octave count. What is left
    // varying is therefore the substance's own noise: halved in the layered run
    // by the `/2` of the blend, whole in the uniform one, so the two spreads
    // stand as two to one. Under the rejected `blended = noise/2` they stand as
    // one to one, and this is the only measurement in the file that sees it.
    let spread = |world: &World| {
        let (low, high) = plane_extremes(world, O2, FLOOR_PLANE);
        high - low
    };
    let sided_spread = spread(&other);
    let flat_spread = spread(&filled);
    assert!(
        sided_spread > 0,
        "the layered run is flat over the floor plane, which makes the ratio \
         below vacuous"
    );
    // A bracket around two rather than an equality: the blend divides twice and
    // both divisions truncate, which moves either spread by units out of
    // millions. Three halves and four, so the rejected form — sitting at one —
    // misses the lower end by half of itself.
    assert!(
        flat_spread >= 3 * sided_spread / 2 && flat_spread <= 4 * sided_spread,
        "over the floor plane O2 spreads by {flat_spread} under `uniform` and by \
         {sided_spread} under `sediment`; the layer term is constant there, so \
         the uniform branch is supposed to carry twice the layered one, and a \
         ratio of one is `blended = noise/2` (ADR-077, rejected)"
    );
}

/// The two sides sit on opposite sides of the boundary, `uniform` on neither,
/// and `sediment` is the floor of the world.
///
/// One fixture declares all three at once, which is the only way to see the
/// failure this test exists for: a side resolved on the wrong substance index
/// puts the oxycline somewhere else entirely, both ledgers close, and every
/// other test in the corpus stays green.
///
/// The last assertion is against a failure of the same family and needs an
/// argument of its own, because everything measured through [`layer_step`] is
/// blind to it. That partition is read off `H2S`'s own deviation, so inverting
/// the sides globally — `sediment` meaning rich *above* the relief — relabels the
/// two groups along with the field, and every relative statement in this file
/// stays green while the default world is stratified upside down: a scenario
/// writing `O2 = "water"` would put its oxygen in the sediment, with both ledgers
/// closing exactly. What that costs is named in ADR-077 by its two consequences,
/// `oxidation_front_forms_at_predicted_depth` and
/// `density_stratification_persists_without_forcing`, and both are statements
/// about which end of the z axis is which.
#[test]
fn layer_sides_are_anticorrelated_across_the_boundary() {
    let derived = derived_from(&with_layers(
        "H2S = \"sediment\"\nO2 = \"water\"\nSO4 = \"uniform\"\n",
    ));
    let mut world = world(&derived);
    let report = generate(&mut world, &derived, LAYER_KEY).expect("must generate");

    let step = |s: u32| layer_step(&world, &report, s);
    let excursion = |s: u32| report.per_substance[s as usize].excursion as f64;

    let sediment = step(H2S);
    assert!(
        sediment <= -excursion(H2S) / 2.0,
        "H2S is enriched in the sediment and the water column holds {sediment} \
         more of it"
    );
    let water = step(O2);
    assert!(
        water >= excursion(O2) / 2.0,
        "O2 is enriched in the water column and it holds {water} more of it"
    );
    // A quarter, on the same argument as in
    // `a_substance_declared_uniform_has_no_layer_step`: half of what a declared
    // side has to show, so the two cannot be confused, and above the residual a
    // partition of space leaves in a field with structure at the scale of the
    // domain.
    let neither = step(SO4);
    assert!(
        neither.abs() <= excursion(SO4) / 4.0,
        "SO4 is declared uniform and steps by {neither} across the boundary"
    );

    // And which end of the axis each side is, measured on the two planes the
    // relief cannot cross. There the layer term is `+UNIT` or `-UNIT` with a
    // sign the declaration fixes, and `|noise| <= UNIT` means the blend cannot
    // carry a substance across its typical amount on the rich side or up to it
    // on the poor one. So the bound is exact rather than statistical, and it is
    // the declaration that is being read and not the field.
    for (s, id, rich_at_the_floor) in [(H2S, "H2S", true), (O2, "O2", false)] {
        let typical = report.per_substance[s as usize].amount_at_typical;
        let (floor_low, floor_high) = plane_extremes(&world, s, FLOOR_PLANE);
        let (lid_low, lid_high) = plane_extremes(&world, s, LID_PLANE);
        let ((rich_low, rich_high), (poor_low, poor_high)) = if rich_at_the_floor {
            ((floor_low, floor_high), (lid_low, lid_high))
        } else {
            ((lid_low, lid_high), (floor_low, floor_high))
        };
        let (rich, poor) = if rich_at_the_floor {
            ("sediment, the floor", "the lid")
        } else {
            ("water, the lid", "the floor")
        };
        assert!(
            rich_low >= typical,
            "{id} is declared on the side of {rich}, and a voxel of that plane \
             holds {rich_low}, under its typical {typical}"
        );
        assert!(
            poor_high <= typical,
            "{id} is declared on the side of {rich}, and a voxel of {poor} holds \
             {poor_high}, over its typical {typical}"
        );
        // Or a substance sitting flat on its typical amount everywhere would
        // satisfy both bounds and assert nothing about either end.
        assert!(
            rich_high > typical && poor_low < typical,
            "{id} does not step across the domain at all: [{rich_low}, \
             {rich_high}] on the rich plane and [{poor_low}, {poor_high}] on the \
             poor one, around a typical of {typical}"
        );
    }
}

/// The side is a property of the scenario and not of the seed (ADR-077,
/// ADR-058).
///
/// The rejected variant — a per-substance sign taken from the run key — makes
/// `different_seed_gives_a_different_initial_state` *greener*: two runs of one
/// scenario would differ not by their noise but by where the oxygen is, and the
/// question "why is there no oxidation front at seed 7" would have no answer in
/// the file. This is the only name that tells the two forms apart.
#[test]
fn the_layer_side_does_not_change_with_the_seed() {
    let derived = derived_from(&with_layers("O2 = \"water\"\n"));

    let mut first = world(&derived);
    let a = generate(&mut first, &derived, run_key(3)).expect("must generate");
    let mut second = world(&derived);
    let b = generate(&mut second, &derived, run_key(4)).expect("must generate");

    // Or the test is green on a build where the seed reaches nothing at all.
    assert_ne!(
        front(&first, H2S),
        front(&second, H2S),
        "the two keys gave one world"
    );

    for s in 0..N_SUBSTANCES {
        let here = layer_step(&first, &a, s);
        let there = layer_step(&second, &b, s);
        assert_eq!(
            here.signum(),
            there.signum(),
            "substance {s} steps by {here} under one key and by {there} under \
             another: the side moved with the seed"
        );
    }
}

/// The floor of the band is positive for **every** substance of the registry
/// (ADR-077).
///
/// Over all five and not over one: the rejected form of the band — a share of
/// the headroom to the ceiling — is green on water, whose typical amount is 99%
/// of its maximum, and red on oxygen, `SO4` and the proton, which are exactly
/// the microcomponents the dynamic range of ADR-039 exists for.
#[test]
fn the_initial_band_keeps_every_substance_strictly_above_zero() {
    let derived = derived();
    let mut world = world(&derived);
    let report = generate(&mut world, &derived, run_key(23)).expect("must generate");

    for s in 0..N_SUBSTANCES {
        let fill = &report.per_substance[s as usize];
        let floor = fill.amount_at_typical - fill.excursion;
        assert!(
            floor > 0,
            "{}: the band starts at {floor}, and a substance absent from part of \
             the domain is indistinguishable from one the scenario declared away",
            fill.id
        );
        // The identity, not merely the sign: `2*excursion <= typical` gives
        // `2*(typical - excursion) >= typical`, which is what makes the floor
        // positive rather than a rounding away from zero (ADR-077).
        assert!(
            2 * floor >= fill.amount_at_typical,
            "{}: the floor {floor} is under half of the typical {}",
            fill.id,
            fill.amount_at_typical
        );
        // And what was written obeys the bound the construction promises.
        assert!(
            fill.floor > 0,
            "{}: the lowest amount written is {}",
            fill.id,
            fill.floor
        );
    }
}

/// A scenario that omits `[initial]` and one that writes today's default out are
/// one configuration and one world (ADR-077, ADR-065).
///
/// Both halves of version 14 at once: the default is applied before the hash, so
/// the two files hash alike (`CONFIG_SCHEMA.md` section 11 item 2), and the
/// arithmetic of `amount_at` under `sediment` is untouched, so the increment is
/// one of identity rather than of dynamics — the claim the comment in
/// `version.rs` makes and that nothing else in the repository can check, there
/// being no recorded world hash anywhere in it.
#[test]
fn the_written_default_and_the_omitted_section_give_one_world() {
    let written = with_layers(
        "H2S = \"sediment\"\nWATER = \"sediment\"\nO2 = \"sediment\"\n\
         SO4 = \"sediment\"\nH_ION = \"sediment\"\n",
    );

    let omitted_config = config::parse(SCENARIO).expect("the fixture must parse");
    let written_config = config::parse(&written).expect("the fixture must parse");
    assert_eq!(
        config::config_hash(&omitted_config).expect("hashing config"),
        config::config_hash(&written_config).expect("hashing config"),
    );

    let omitted = derived_from(SCENARIO);
    let spelled_out = derived_from(&written);
    let mut a = world(&omitted);
    let mut b = world(&spelled_out);
    generate(&mut a, &omitted, LAYER_KEY).expect("must generate");
    generate(&mut b, &spelled_out, LAYER_KEY).expect("must generate");

    assert_eq!(a.amounts_32(), b.amounts_32(), "the narrow amounts");
    assert_eq!(a.amounts_64(), b.amounts_64(), "the wide amounts");
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
