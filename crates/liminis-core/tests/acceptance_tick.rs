//! Acceptance criteria of the tick loop (`ACCEPTANCE.md`, S0).
//!
//! ```text
//! ledger_residual_is_zero_over_10k_ticks
//! energy_ledger_residual_is_zero_over_10k_ticks
//! same_seed_and_config_give_byte_identical_state
//! different_seed_gives_different_state
//! ```
//!
//! Plus `diffusing_matter_does_not_move_enthalpy`, which `ACCEPTANCE.md` names
//! and which could not be written until something held both fields at once
//! (`TODO(diffusing-matter-does-not-move-enthalpy)` in `process/diffuse.rs`
//! deferred it to exactly this file), and the tests that hold the order of the
//! splitting, the transport of the enthalpy field, the shape of the left side of
//! the invariant and the four refusals of [`Tick::new`] in place — none of which
//! `ACCEPTANCE.md` names, because the document predates the roster of ADR-065.
//!
//! An integration test rather than a unit one, for the reason
//! `tests/acceptance_diffusion.rs` gives: the outside view may use only what a
//! scenario can use — the public surface of `liminis-core` — so it keeps its
//! meaning when the inside is rearranged.
//!
//! # The fixture, and why it is written here
//!
//! No file under `configs/` will do. `hello.toml` declares no substance at all,
//! so it has no heat capacity, no temperature and no energy scale, and it does
//! not survive its own derivation (`config::load` says so at length). The worked
//! example of `CONFIG_SCHEMA.md` section 12 lives as a private string inside
//! `config/validate.rs`. So the scenario is written out here, and the numbers
//! that the corpus does not name are marked as placeholders and must not be
//! copied into `configs/`.
//!
//! The grid is **not** the one the scenario declares: a `World` is built by this
//! file, and it is built closed on all six faces. That is deliberate and it is
//! what makes `ledger_residual_is_zero_over_10k_ticks` able to fail — on a closed
//! domain every channel counter stays at zero, so the right-hand side of the
//! invariant is exactly zero and any matter created or destroyed shows up as a
//! non-zero residual on the tick it happened.
//!
//! The registry is arranged so that a lane is never a substance index: `WATER` is
//! declared second, is the only 64-bit substance (its `k` is raised to the extent
//! exponent of a reaction, ADR-039), and therefore takes lane 0 of the wide field
//! while the four narrow substances take lanes 0..3 of the narrow one. On the
//! registry the project carries, water is *first* and takes lane 0, so the wrong
//! expression `lane == s` is right at `s == 0` and off by one from there on
//! (ADR-056).

use liminis_core::config;
use liminis_core::ledger::{Channel, DomainSums, Ledger};
use liminis_core::numeric::{M32, M64};
use liminis_core::process::{
    ProcessId, ROSTER_LEN, RosterEntry, STEP_ORDER, Scratch, Step, Tick, default_roster,
};
use liminis_core::world::{Boundary, Grid, LaneRef, Registry, Width, World, WorldLayout};

/// The eco regime of SPEC section 1.7: a one-second tick and a 100 um voxel.
const DT: f64 = 1.0;
const DX: f64 = 1.0e-4;

/// Small enough that ten thousand ticks run in a test, and divisible by four so
/// that the enthalpy grid at `lod = 2` exists at all.
const N: u32 = 4;

/// The edge the tests about the enthalpy field use instead.
///
/// At `N` the enthalpy grid is a single cell, and a single cell is the second
/// state under which diffusion is a no-op: every face of it is a closed wall, the
/// kernel's neighbour lookup answers with the cell itself, and no flux exists to
/// be wrong. Eight gives `2³` cells, which is the smallest grid on which the
/// transport of the field is a statement about anything.
const N_HEAT: u32 = 8;

/// Substance indices of the fixture, in declaration order.
const H2S: u32 = 0;
const WATER: u32 = 1;
const O2: u32 = 2;
const SO4: u32 = 3;
const H_ION: u32 = 4;
const N_SUBSTANCES: u32 = 5;

/// A scenario with matter, chemistry and an enthalpy field — everything
/// `config::derive` needs, and nothing `configs/` carries.
///
/// **The numbers marked as placeholders are placeholders.** `c_p`,
/// `enthalpy_formation` and `partial_molar_volume` are declared for no substance
/// anywhere in the corpus (`CONFIG_SCHEMA.md` section 13 item 23); the rest is
/// section 12 and SPEC sections 1.7 and 2.3. Do not copy this into `configs/`.
///
/// `water_formation` exists for one reason and it is not chemistry: it is the
/// reaction whose extent exponent raises `WATER` past its own overflow ceiling,
/// which is what makes water the 64-bit substance (ADR-039, ADR-040). Its
/// enthalpy is the sum over the enthalpies of formation of its participants,
/// because ADR-044 makes that a load error rather than a rounding.
const SCENARIO: &str = r#"
name = "tick-fixture"
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
nx = 4
ny = 4
nz = 4
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
max_conc = 10.0
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
typical_conc = 0.25
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
typical_conc = 28.0
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
max_conc = 1.0e-2
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

/// A world on a domain closed on all six faces, with the amounts seeded into the
/// front buffer.
///
/// Closed and not periodic: on a torus a loss through a face is invisible because
/// there is no face, and the residual would close over it.
fn world(derived: &config::Derived) -> World {
    world_sized(derived, N, Heat::Varying)
}

/// How the fixture seeds the enthalpy field.
///
/// [`Heat::Uniform`] is not "a simpler fixture": a uniform field is the one state
/// the diffusion of the field itself cannot move, so it is what separates "the
/// enthalpy of diffusing matter followed it" from "the enthalpy diffused on its
/// own". Both are transport of energy and only the first is forbidden (ADR-062).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Heat {
    Varying,
    Uniform,
}

/// The fixture at a chosen edge. See [`world`].
fn world_sized(derived: &config::Derived, n: u32, heat: Heat) -> World {
    let grid = Grid::new(n, n, n, [Boundary::Closed; 6]).expect("the grid");
    let registry = Registry::new(&derived.decls()).expect("the registry");
    let mut world = World::new(
        grid,
        registry,
        &WorldLayout {
            enthalpy_lod: 2,
            velocity_lod: 1,
        },
    )
    .expect("the world");

    seed(&mut world, n, heat);
    world
}

/// A profile that varies along every axis, so that no operator sees a uniform
/// field.
///
/// A uniform field is the one initial state under which diffusion is a no-op:
/// every face carries the same amount both ways, the residual is zero because
/// nothing moved, and the test would pass with the whole tick loop deleted.
fn profile(n: u32, idx: u32, offset: i64) -> i64 {
    let (x, y, z) = (idx % n, (idx / n) % n, idx / (n * n));
    offset + i64::from(z) * 4_099 + i64::from(x) * 131 + i64::from(y) * 17
}

/// Fill both amount fields and the enthalpy field, and promote them into the
/// front buffer — which is where a process expects state `N` (ADR-057).
fn seed(world: &mut World, n: u32, heat: Heat) {
    let n_voxels = world.grid().n_voxels();
    let base: Vec<i64> = (0..N_SUBSTANCES)
        .map(|s| 1_000_000 + i64::from(s) * 7_919)
        .collect();

    for s in 0..N_SUBSTANCES {
        let offset = base[s as usize];
        match world.lane_of(s) {
            LaneRef::Narrow(lane) => {
                let field = world.amounts_32_mut().expect("a narrow field");
                let buffer = field.write_mut();
                for idx in 0..n_voxels {
                    // The fixture's amounts stay far inside `i32`, so the cast
                    // below cannot wrap; a scenario's would not, which is why
                    // `M32::from_i64_clamping` exists and why this is a test.
                    buffer[(lane * n_voxels + idx) as usize] =
                        M32::from_i64_clamping(profile(n, idx, offset));
                }
            }
            LaneRef::Wide(lane) => {
                let field = world.amounts_64_mut().expect("a wide field");
                let buffer = field.write_mut();
                for idx in 0..n_voxels {
                    buffer[(lane * n_voxels + idx) as usize] = M64::new(profile(n, idx, offset));
                }
            }
        }
    }
    if let Some(field) = world.amounts_32_mut() {
        field.swap();
    }
    if let Some(field) = world.amounts_64_mut() {
        field.swap();
    }

    // Enthalpy. Transported by the same two steps as the amounts (SPEC section 8
    // `c` and `d`) and on its own coarse grid, so it is seeded here for two
    // reasons: so that the energy half of the left side is a real number rather
    // than a structural zero, and so that step `d` has a gradient to work on.
    //
    // `Heat::Uniform` is the state that gradient is absent in, and it is the
    // fixture of `diffusing_matter_does_not_move_enthalpy`.
    let cells = world.enthalpy_grid().n_voxels();
    {
        let field = world.enthalpy_mut();
        let buffer = field.write_mut();
        for cell in 0..cells {
            buffer[cell as usize] = match heat {
                Heat::Varying => M64::new(50_000 + i64::from(cell) * 101),
                Heat::Uniform => M64::new(50_000),
            };
        }
        field.swap();
    }
}

/// A roster with exactly the named processes enabled and every other one off.
///
/// Not `default_roster()` with edits: the point of the helper is that a test
/// says out loud which of the nine it runs, which is the "alone" arrangement
/// ADR-018 asks `enabled` to make possible.
fn roster(on: &[ProcessId]) -> [RosterEntry; ROSTER_LEN] {
    let mut roster = default_roster();
    for entry in &mut roster {
        entry.enabled = on.contains(&entry.id);
    }
    roster
}

/// The two steps of the tick order that have an operator today. See the table in
/// the header of `process/tick.rs` for what blocks the other seven.
const DISPATCHABLE: [ProcessId; 2] = [ProcessId::Advection, ProcessId::Diffusion];

/// How many ticks the two ledger criteria run for.
///
/// A shortened form of the criterion of SPEC section 13, which asks for `10^6`
/// ticks with both residuals at zero. The full form is a run of its own and does
/// not belong in `cargo test`: at this grid it is a hundred times this test.
const TICKS: u32 = 10_000;

/// Every buffer of a world, flattened, for a byte-for-byte comparison.
///
/// The **front** buffer of every lane, never `write_mut`: ADR-057 promises state
/// `N` in the front after every process, and comparing the write buffers would
/// compare whatever the last operator happened to leave behind. Buffers and not
/// a checksum, because a checksum hides a permutation of the lanes.
fn snapshot(world: &World) -> Vec<u8> {
    let mut bytes = amounts_snapshot(world);
    bytes.extend_from_slice(&heat_snapshot(world));
    for value in world.energy_delta() {
        bytes.extend_from_slice(&value.raw().to_le_bytes());
    }
    // The three `Q` fields go in by their bits and not by their value: `Q` is a
    // wrapper over an `f32` and `debug_f64` is the only way out of it, and the
    // widening is exact, so the bits of the `f64` distinguish exactly what the
    // bits of the `f32` do.
    for value in world.light() {
        bytes.extend_from_slice(&value.debug_f64().to_bits().to_le_bytes());
    }
    for value in world.velocity() {
        bytes.extend_from_slice(&value.debug_f64().to_bits().to_le_bytes());
    }
    for value in world.velocity_potential() {
        bytes.extend_from_slice(&value.debug_f64().to_bits().to_le_bytes());
    }
    bytes
}

/// The two amount fields' front buffers: the half of [`snapshot`] that carries
/// matter.
fn amounts_snapshot(world: &World) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(field) = world.amounts_32() {
        for value in field.read() {
            bytes.extend_from_slice(&value.raw().to_le_bytes());
        }
    }
    if let Some(field) = world.amounts_64() {
        for value in field.read() {
            bytes.extend_from_slice(&value.raw().to_le_bytes());
        }
    }
    bytes
}

/// The enthalpy field's front buffer: the half that carries energy.
///
/// Split out of [`snapshot`] because the two statements about the enthalpy that
/// steps `c` and `d` owe are about it *alone* — one says it moved, the other says
/// it did not — and a comparison of the whole world would answer neither.
fn heat_snapshot(world: &World) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in world.enthalpy().read() {
        bytes.extend_from_slice(&value.raw().to_le_bytes());
    }
    bytes
}

/// The energy the enthalpy field holds, summed the way `DomainSums` sums it.
fn heat_total(world: &World) -> i128 {
    world
        .enthalpy()
        .read()
        .iter()
        .map(|value| i128::from(value.to_i64()))
        .sum()
}

/// Advance `world` for `ticks` ticks with `on` enabled and no others.
///
/// The `#[cfg(debug_assertions)]` LEDGER phase runs inside every one of these
/// ticks, so both residuals are already asserted per tick by the time any
/// assertion below is reached.
fn advance(world: &mut World, derived: &config::Derived, on: &[ProcessId], ticks: u32) {
    let tick = Tick::new(world, derived, &roster(on), DT, DX).expect("folding the tick");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut scratch = Scratch::new(world, &tick).expect("the scratch buffers");
    for t in 0..ticks {
        tick.advance(world, &mut ledger, &mut scratch, t, 0);
    }
}

/// One run of `ticks` ticks under one seed, returning the world it ended on.
fn run(ticks: u32, run_key: u32, on: &[ProcessId]) -> World {
    let derived = derived();
    let mut world = world(&derived);
    let tick = Tick::new(&world, &derived, &roster(on), DT, DX).expect("folding the tick");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch buffers");

    for t in 0..ticks {
        tick.advance(&mut world, &mut ledger, &mut scratch, t, run_key);
    }
    world
}

// --- the two ledger criteria ---------------------------------------------

#[test]
fn ledger_residual_is_zero_over_10k_ticks() {
    // The shortened form of the criterion of SPEC section 13 (`10^6` ticks); the
    // full one runs separately.
    //
    // Checked **per tick** and not as the difference of the two ends of the run.
    // A leak on tick 7 compensated on tick 900 leaves the difference of the ends
    // at zero, and a test built out of two reductions around a run is a much
    // weaker statement wearing this name.
    let derived = derived();
    let mut world = world(&derived);
    let tick = Tick::new(&world, &derived, &roster(&DISPATCHABLE), DT, DX).expect("folding");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch");

    let mut before = DomainSums::new(N_SUBSTANCES).expect("before");
    let mut after = DomainSums::new(N_SUBSTANCES).expect("after");

    for t in 0..TICKS {
        tick.domain_sums(&world, &mut before);
        tick.advance(&mut world, &mut ledger, &mut scratch, t, 0);
        tick.domain_sums(&world, &mut after);

        for s in 0..N_SUBSTANCES {
            assert_eq!(
                ledger.residual_matter(s, &before, &after),
                0,
                "substance {s} did not close on tick {t}"
            );
        }
    }

    // The right-hand side, asserted rather than assumed: the domain is closed on
    // all six faces, so nothing may have been credited anywhere. Without this the
    // residuals above would also be satisfied by a step that lost matter and
    // credited the loss to a channel.
    for channel in Channel::ALL {
        for s in 0..N_SUBSTANCES {
            assert_eq!(
                ledger.matter(channel, s),
                0,
                "{} credited substance {s} on a closed domain",
                channel.name()
            );
        }
    }
}

#[test]
#[ignore = "the last assertion cannot pass as it stands, and weakening it would \
            make the criterion unfailable. `SOLAR_IN` needs the fold of step \
            `i'`, which cannot be dispatched: `FoldParams::joules_per_intensity` \
            is named by no key (TODO(joules-per-intensity) in kernels/fold.rs) \
            and what the fold owes the ledger is open question A-16 — three \
            answers are named and none chosen. The per-tick half above is no \
            longer vacuous: steps `c` and `d` transport the enthalpy field, so \
            energy moves inside the domain and a transport that lost a joule \
            would show here. What is still missing is a *source* — the reaction \
            energy of an exothermic reaction has no channel and no door in \
            DomainSums, and enthalpy_formation enters no left-hand side (see the \
            module header of ledger/mod.rs) — so with the light off the \
            right-hand side is identically zero"]
fn energy_ledger_residual_is_zero_over_10k_ticks() {
    let derived = derived();
    let mut world = world(&derived);
    let tick = Tick::new(&world, &derived, &roster(&DISPATCHABLE), DT, DX).expect("folding");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch");

    let mut before = DomainSums::new(N_SUBSTANCES).expect("before");
    let mut after = DomainSums::new(N_SUBSTANCES).expect("after");

    for t in 0..TICKS {
        tick.domain_sums(&world, &mut before);
        tick.advance(&mut world, &mut ledger, &mut scratch, t, 0);
        tick.domain_sums(&world, &mut after);
        assert_eq!(
            ledger.residual_energy(&before, &after),
            0,
            "the energy ledger did not close on tick {t}"
        );
    }

    // The half that makes the test able to fail, and the half that is blocked.
    // With the light switched off both sides are identically zero and every
    // assertion above holds for a tick loop that does nothing at all.
    assert!(
        ledger.energy(Channel::SolarIn) > 0,
        "no solar energy entered the domain over {TICKS} ticks"
    );
}

// --- determinism ----------------------------------------------------------

#[test]
fn same_seed_and_config_give_byte_identical_state() {
    // Buffers and not a checksum: a checksum over the whole field is equal under
    // a permutation of the lanes, which is exactly the failure ADR-056 and
    // ADR-057 are about.
    let left = run(64, 7, &DISPATCHABLE);
    let right = run(64, 7, &DISPATCHABLE);
    assert_eq!(
        snapshot(&left),
        snapshot(&right),
        "two runs of one seed and one config diverged"
    );
}

#[test]
#[ignore = "no dispatchable process consumes run_key. Its two consumers are \
            React::params (ADR-058) and NoiseParams in the velocity field \
            (ADR-069), and neither step can be built — the reactions want a \
            temperature field no operator derives, the velocity field wants the \
            heat capacity buffer world/world.rs names and does not allocate. \
            Until one of them is dispatchable the two runs below are equal, and \
            the honest form of this criterion is a red test that says why rather \
            than a weakened one that passes. Weakening it — comparing the run \
            keys, or asserting only that the tick forwards the argument — is the \
            temptation to name out loud: it would leave the actual failure, a \
            tick that forwards `tick` and drops `run_key`, invisible while its \
            neighbour above stays green"]
fn different_seed_gives_different_state() {
    // A statement about this pair of seeds and not a theorem: `run_key` collapses
    // a `u64` seed into a `u32` (ADR-058), so some pair of seeds collides by
    // construction.
    let left = run(64, 7, &DISPATCHABLE);
    let right = run(64, 8, &DISPATCHABLE);
    assert_ne!(
        snapshot(&left),
        snapshot(&right),
        "two seeds gave the same world"
    );
}

// --- the order of the splitting ------------------------------------------

#[test]
fn the_step_order_is_the_one_spec_section_8_prints() {
    // The only thing in the repository that can see two Lie-Trotter factors
    // swapped: both residuals close under any order, every "alone" test and every
    // kernel test stays green, and there is no golden run to disagree with.
    let letters: Vec<&str> = STEP_ORDER.iter().map(|step| step.letter()).collect();
    assert_eq!(letters, ["a", "b", "c", "d", "e", "f", "h", "i'", "j"]);
}

#[test]
fn light_stands_before_the_energy_fold() {
    // ADR-049: the fold derives absorption from the stored light field, so the
    // field has to belong to the *current* tick. The behavioural half of this
    // criterion — one tick on a domain with a zero light field absorbs a non-zero
    // amount of energy — cannot be written while the fold is unbuildable; see the
    // `#[ignore]` on `energy_ledger_residual_is_zero_over_10k_ticks`.
    let position = |want: Step| {
        STEP_ORDER
            .iter()
            .position(|&step| step == want)
            .expect("every step is in STEP_ORDER")
    };
    assert!(position(Step::Light) < position(Step::EnergyFold));
}

// --- the roster -----------------------------------------------------------

#[test]
fn the_roster_is_the_nine_of_adr_065() {
    assert_eq!(ProcessId::ALL.len(), ROSTER_LEN);
    assert_eq!(ROSTER_LEN, 9);

    // In the order of SPEC section 8, and the order is not decoration: it is the
    // order the canonical form prints and therefore part of `config_hash`.
    let ids: Vec<&str> = ProcessId::ALL.iter().map(|p| p.id()).collect();
    assert_eq!(
        ids,
        [
            "light",
            "velocity_field",
            "advection",
            "diffusion",
            "pressure",
            "settling",
            "phase_transitions",
            "reactions",
            "external_channels",
        ]
    );

    // Distinct, and a round trip through the loader's door. A duplicate spelling
    // would make two records of one process indistinguishable, which is the
    // refusal `a_duplicate_process_id_is_rejected` exists for.
    let mut seen = std::collections::BTreeSet::new();
    for id in ProcessId::ALL {
        assert!(seen.insert(id.id()), "`{}` is spelled twice", id.id());
        assert_eq!(ProcessId::from_id(id.id()), Some(id));
    }
    assert_eq!(ProcessId::from_id("reactions_abiotic"), None);

    // What is deliberately absent, each for its own reason (ADR-065): guild trait
    // selection (`i`) arrives in S1, geology (`k`) in S4, cell sorting (`l`)
    // turns determinism off rather than a process, and the energy fold (`i'`) is
    // part of the energy path of the reactions rather than a process.
    for absent in ["guild_selection", "geology", "cell_sort", "energy_fold"] {
        assert_eq!(ProcessId::from_id(absent), None);
    }
}

#[test]
fn every_roster_default_comes_from_its_own_module() {
    // The outside half of the check `process/mod.rs` makes from the inside: the
    // roster a scenario gets when it writes no `[[process]]` section is the nine
    // defaults, in order.
    let roster = default_roster();
    for (entry, id) in roster.iter().zip(ProcessId::ALL) {
        assert_eq!(entry.id, id);
        assert_eq!(entry.enabled, id.enabled_by_default());
        assert_eq!(entry.every_n_ticks, 1);
    }
}

#[test]
fn an_enabled_process_without_an_operator_is_refused() {
    // The other half of "the roster is closed": a scenario may name any of the
    // nine, and seven of them cannot be dispatched. Refused at fold time, naming
    // what is missing — a silent skip would make "switched on" and "switched off"
    // the same world with different hashes.
    let derived = derived();
    let world = world(&derived);
    for id in ProcessId::ALL {
        let result = Tick::new(&world, &derived, &roster(&[id]), DT, DX);
        if DISPATCHABLE.contains(&id) {
            assert!(result.is_ok(), "`{}` should fold", id.id());
        } else {
            let message = format!("{:#}", result.expect_err("should refuse"));
            assert!(
                message.contains(id.id()),
                "the refusal has to name `{}`; it said:\n{message}",
                id.id()
            );
        }
    }
}

// --- the left side of the invariant ---------------------------------------

#[test]
fn the_left_side_of_the_invariant_is_gathered_in_one_place() {
    // Every door of `DomainSums` that S0 has data for is fed by `Tick::domain_sums`
    // and by nothing else. The guild and cell doors are fed empty slices — S0 has
    // neither — so what this can hold is the two that carry numbers: the amounts
    // of every substance, and the enthalpy.
    let derived = derived();
    let world = world(&derived);
    let tick = Tick::new(&world, &derived, &roster(&DISPATCHABLE), DT, DX).expect("folding");

    let mut sums = DomainSums::new(N_SUBSTANCES).expect("the sums");
    tick.domain_sums(&world, &mut sums);

    for s in [H2S, WATER, O2, SO4, H_ION] {
        assert!(sums.matter(s) > 0, "substance {s} is missing from the sums");
    }

    // Enthalpy, at the width it actually has: `world::World` stores it as an
    // `i64` field (ADR-062), and a left side that summed only `amount[]` would
    // report zero here while the energy residual went on closing.
    let enthalpy: i128 = world
        .enthalpy()
        .read()
        .iter()
        .map(|v| i128::from(v.to_i64()))
        .sum();
    assert!(enthalpy > 0, "the fixture seeds a non-zero enthalpy");
    assert_eq!(sums.energy(), enthalpy);
}

#[test]
fn the_substance_of_a_domain_sum_is_not_its_lane() {
    // On this registry `WATER` is declared second and is the only 64-bit
    // substance, so it takes lane 0 of the wide field while `O2`, `SO4` and
    // `H_ION` sit on lanes 1..3 of the narrow one. `lane == s` is therefore right
    // for `H2S` and wrong for everything after it (ADR-056) — and a wrong mapping
    // applied to both `before` and `after` gives a residual of exactly zero on
    // every tick for ever.
    let derived = derived();
    let world = world(&derived);
    let tick = Tick::new(&world, &derived, &roster(&DISPATCHABLE), DT, DX).expect("folding");

    assert_eq!(world.lane_of(WATER), LaneRef::Wide(0));
    assert_eq!(world.lane_of(O2), LaneRef::Narrow(1));
    assert_eq!(world.registry().lanes(Width::Bits64), 1);

    let mut sums = DomainSums::new(N_SUBSTANCES).expect("the sums");
    tick.domain_sums(&world, &mut sums);

    for s in 0..N_SUBSTANCES {
        let by_hand: i128 = match world.lane_of(s) {
            LaneRef::Narrow(lane) => world
                .amounts_32()
                .expect("a narrow field")
                .lane(lane)
                .iter()
                .map(|v| i128::from(v.to_i64()))
                .sum(),
            LaneRef::Wide(lane) => world
                .amounts_64()
                .expect("a wide field")
                .lane(lane)
                .iter()
                .map(|v| i128::from(v.to_i64()))
                .sum(),
        };
        assert_eq!(sums.matter(s), by_hand, "substance {s}");
    }

    // And the sums are distinct, so that the equality above is not satisfied by
    // five copies of one number.
    let distinct: std::collections::BTreeSet<i128> =
        (0..N_SUBSTANCES).map(|s| sums.matter(s)).collect();
    assert_eq!(distinct.len(), N_SUBSTANCES as usize);
}

#[test]
fn a_disabled_process_leaves_every_buffer_bit_for_bit() {
    // What ADR-018 wants `enabled` for, and what catches "a skipped process left
    // its lanes not unchanged but one tick stale" (ADR-057): the comparison is
    // over the front buffers of every lane, which is where state `N` has to be
    // after any process, dispatched or not.
    let moved = run(4, 0, &[ProcessId::Diffusion]);
    let still = run(4, 0, &[ProcessId::Advection]);
    let untouched = run(4, 0, &[]);

    let start = {
        let derived = derived();
        world(&derived)
    };

    // Diffusion on: the world moved. Without this the two equalities below would
    // hold for a tick loop that never dispatched anything.
    assert_ne!(
        snapshot(&moved),
        snapshot(&start),
        "diffusion changed nothing, so the comparisons below prove nothing"
    );

    // Every process off: bit for bit the initial state, front buffers included.
    assert_eq!(snapshot(&untouched), snapshot(&start));

    // Advection on with the velocity field off: the Courant numbers are zero, so
    // this is a dispatched step that moves nothing — which is a different thing
    // from a skipped step, and has to leave the same bytes.
    assert_eq!(snapshot(&still), snapshot(&start));
}

// --- the enthalpy field ---------------------------------------------------

#[test]
fn diffusing_matter_does_not_move_enthalpy() {
    // ADR-062 decides that the enthalpy of a diffusing substance does *not*
    // follow it: the term is discarded, bounded at load by
    // `enthalpy_carried_by_diffusion_over_five_percent_is_rejected`, and 1.64% on
    // the corpus registry. `process/diffuse.rs` names this criterion in a TODO
    // and could not write it — a process handed one field can only assert that a
    // buffer nobody passed it did not change. The tick holds both fields at once,
    // so here it is.
    //
    // The enthalpy is seeded **uniform**, and that is the whole construction: a
    // uniform field is the one state its own diffusion cannot move, so what is
    // left for the assertion to see is transport that came from somewhere else —
    // which is exactly the discarded term. On a varying field the two are added
    // together and the test would say nothing.
    let derived = derived();
    let mut world = world_sized(&derived, N_HEAT, Heat::Uniform);

    let matter_before = amounts_snapshot(&world);
    let heat_before = heat_snapshot(&world);

    advance(&mut world, &derived, &[ProcessId::Diffusion], 4);

    // The matter moved. Without this the equality below holds for a tick loop
    // that dispatched nothing at all.
    assert_ne!(
        amounts_snapshot(&world),
        matter_before,
        "no substance diffused, so the enthalpy had nothing to be carried by"
    );
    assert_eq!(
        heat_snapshot(&world),
        heat_before,
        "diffusing matter carried enthalpy with it (ADR-062)"
    );
}

#[test]
fn the_enthalpy_field_is_transported_by_the_steps_that_name_it() {
    // SPEC section 8 puts the enthalpy on line `c` and on line `d` beside the
    // substances — "адвекция … энтальпия", "диффузия … энтальпия на 32³ — шесть"
    // — and says outright that it is an ordinary diffusive field updated every
    // tick. Nothing else in this file can see that it is: the field conserves
    // under transport, so a tick that never touched it keeps both residuals at
    // exactly zero for ever, and the temperature ADR-062 derives from it goes on
    // looking plausible in a world where heat does not conduct.
    let derived = derived();
    let mut world = world_sized(&derived, N_HEAT, Heat::Varying);

    let start = heat_snapshot(&world);
    let energy = heat_total(&world);
    assert!(energy > 0, "the fixture seeds a non-zero enthalpy");

    // Step `d`. Six substeps at the coarse step, from the record's own
    // `thermal_diffusivity` and never from a substance's `D` (ADR-062).
    advance(&mut world, &derived, &[ProcessId::Diffusion], 1);
    assert_ne!(
        heat_snapshot(&world),
        start,
        "step `d` did not touch the enthalpy field"
    );
    assert_eq!(
        heat_total(&world),
        energy,
        "step `d` conserves energy exactly, or it is not a flux scheme (ADR-005)"
    );

    // Step `c`, alone and from the seeded state again. It moves nothing today and
    // has to leave the bytes alone rather than leave them stale: the Courant
    // buffer of the coarse grid is zeros because step `b` cannot be dispatched
    // (see "The Courant buffers, which nothing fills" in `process/tick.rs`), and
    // a phase that ran three applications still owes the front buffer state `N`
    // (ADR-057).
    //
    // Said out loud: this half cannot fail while the buffer is zeros, and
    // deleting the dispatch of the enthalpy from step `c` would keep it green.
    // It is written down anyway because the failure it will catch is the one that
    // arrives with the fold — a phase that runs and leaves the write buffer in
    // the front — and because the assertion above it is what fails if the field
    // stops being transported at all.
    let mut world = world_sized(&derived, N_HEAT, Heat::Varying);
    advance(&mut world, &derived, &[ProcessId::Advection], 1);
    assert_eq!(
        heat_snapshot(&world),
        start,
        "step `c` moved the enthalpy field under a zero Courant buffer"
    );
}

// --- what the fold refuses ------------------------------------------------

#[test]
fn a_roster_naming_one_process_twice_is_refused() {
    // `config::materialise` refuses this over the text of a scenario, and that
    // check is on a path nothing reaches: no door in the crate turns a `Config`
    // into a roster array. A repeat leaves whichever process is missing at the
    // `false` the array was initialised with, so the world silently loses a
    // process while the roster still has nine entries.
    let derived = derived();
    let world = world(&derived);
    let mut roster = roster(&DISPATCHABLE);
    roster[0] = roster[1];

    let message = format!(
        "{:#}",
        Tick::new(&world, &derived, &roster, DT, DX).expect_err("should refuse")
    );
    assert!(
        message.contains(ProcessId::VelocityField.id()),
        "the refusal has to name the repeated process; it said:\n{message}"
    );
}

#[test]
fn diffusion_every_other_tick_is_refused() {
    // ADR-030, on the door that executes. Skipping ticks multiplies the effective
    // `dt`, which is what the substep count exists to bound — and nothing after
    // this point could see it: diffusion conserves at any `alpha`, so both
    // residuals close over an unstable field until it overflows.
    let derived = derived();
    let world = world(&derived);
    let mut roster = roster(&DISPATCHABLE);
    for entry in &mut roster {
        if entry.id == ProcessId::Diffusion {
            entry.every_n_ticks = 2;
        }
    }

    let message = format!(
        "{:#}",
        Tick::new(&world, &derived, &roster, DT, DX).expect_err("should refuse")
    );
    assert!(
        message.contains(ProcessId::Diffusion.id()),
        "the refusal has to name the process; it said:\n{message}"
    );
}

#[test]
fn a_dt_the_scenario_was_not_derived_under_is_refused() {
    // The enthalpy field's substeps and `alpha` come from the derivation, and the
    // tick refolds them from the same two inputs to check that it agrees. It can
    // only disagree on `dt`, and a disagreement there is a field stepping at a
    // rate nobody chose, with every test of transport still green because
    // diffusion conserves at any `alpha`.
    let derived = derived();
    let world = world(&derived);
    let message = format!(
        "{:#}",
        Tick::new(&world, &derived, &roster(&DISPATCHABLE), DT * 2.0, DX)
            .expect_err("should refuse")
    );
    assert!(
        message.contains("enthalpy"),
        "the refusal has to name the field; it said:\n{message}"
    );
}
