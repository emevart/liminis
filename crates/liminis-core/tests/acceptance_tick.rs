//! Acceptance criteria of the tick loop (`ACCEPTANCE.md`, S0).
//!
//! ```text
//! ledger_residual_is_zero_over_10k_ticks
//! energy_ledger_residual_is_zero_over_10k_ticks
//! same_seed_and_config_give_byte_identical_state
//! different_seed_gives_different_state
//! poisoning_the_scratch_before_a_tick_changes_no_buffer_of_the_world
//! the_denominator_is_fresh_for_step_b_and_for_step_h
//! a_restart_continues_the_run_bit_for_bit_from_the_tick_it_was_taken_at
//! the_snapshot_holds_no_buffer_of_class_q
//! every_buffer_the_scratch_owns_is_absent_from_the_snapshot
//! the_fold_credits_no_solar_energy_when_the_light_step_does_not_run
//! load_reports_the_per_voxel_footprint_of_the_world_and_the_scratch
//! ```
//!
//! The last seven arrived with ADR-086, which is where the owner of every buffer
//! is decided; the eighth name of that record,
//! `every_n_ticks_on_the_light_process_is_rejected`, is a refusal of *load* and
//! lives in `config/validate.rs` beside the other sixty-four.
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
use liminis_core::ledger::{Channel, DomainSums, Ledger, Nu};
use liminis_core::numeric::{M32, M64, Q, qadd};
use liminis_core::observe::{SnapshotIdentity, snapshot_read_into, snapshot_write};
use liminis_core::process::{
    Footprint, ProcessId, ROSTER_LEN, RosterEntry, STEP_ORDER, Scratch, ScratchBuffers, Step,
    Temperature, Tick, default_roster,
};
use liminis_core::world::{
    Boundary, Grid, LaneRef, OwnedBuffers, Registry, Width, World, WorldLayout,
};

/// The run identity a snapshot of this fixture carries (ADR-020).
///
/// The hash is a literal here on purpose: nothing in these tests re-derives it,
/// and what is being asserted is the *length* of the file and the classification
/// of the buffers, not the provenance.
fn identity() -> SnapshotIdentity {
    SnapshotIdentity {
        seed: 42,
        config_hash: "blake3:0000000000000000".to_string(),
        world_format_version: liminis_core::version::WORLD_FORMAT_VERSION,
        tick: 0,
    }
}

/// The eco regime of SPEC section 1.7: a one-second tick and a 100 um voxel.
const DT: f64 = 1.0;
const DX: f64 = 1.0e-4;

/// Small enough that ten thousand ticks run in a test, and divisible by four so
/// that the enthalpy grid at `lod = 2` exists at all.
const N: u32 = 4;

/// The coarsening of the enthalpy grid this fixture builds, in bits (ADR-062).
const ENTHALPY_LOD: u32 = 2;

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

/// Parse the fixture.
///
/// `Tick::new` takes the `Config` as well as the `Derived` since ADR-086: three
/// of the operators it folds — the chemistry, the temperature and the fold —
/// read keys that never reach a derivation.
fn config() -> config::Config {
    config::parse(SCENARIO).expect("the fixture must parse")
}

/// Parse, validate and derive the fixture.
fn derived() -> config::Derived {
    config::validate(&config()).expect("the fixture must validate")
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
            enthalpy_lod: ENTHALPY_LOD,
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
    // The stride of a lane, which is not the voxel count: a lane ends in the
    // ghost cell of ADR-059. Written with `n_voxels` instead, every lane after
    // the first lands one element short of where its substance is — and nothing
    // downstream notices, because a residual compares a field with itself.
    let lane_len = world.grid().lane_len();
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
                    buffer[(lane * lane_len + idx) as usize] =
                        M32::from_i64_clamping(profile(n, idx, offset));
                }
            }
            LaneRef::Wide(lane) => {
                let field = world.amounts_64_mut().expect("a wide field");
                let buffer = field.write_mut();
                for idx in 0..n_voxels {
                    buffer[(lane * lane_len + idx) as usize] = M64::new(profile(n, idx, offset));
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

/// The steps of the tick order **this fixture's scenario** can dispatch.
///
/// Three of the nine have an operator since ADR-087, and the third is missing
/// from this list on purpose: step `b` folds for a roster that enables it only if
/// the scenario also declares the four keys of ADR-069, and [`SCENARIO`] writes no
/// `[[process]]` record at all — which is the shape every file under `configs/`
/// has. The velocity fixture is [`SCENARIO_WITH_VELOCITY`], at the bottom of this
/// file. See the table in the header of `process/tick.rs` for what blocks the
/// other six.
const DISPATCHABLE: [ProcessId; 2] = [ProcessId::Advection, ProcessId::Diffusion];

/// How many ticks the two ledger criteria run for.
///
/// A shortened form of the criterion of SPEC section 13, which asks for `10^6`
/// ticks with both residuals at zero. The full form is a run of its own and does
/// not belong in `cargo test`: at this grid it is a hundred times this test.
const TICKS: u32 = 10_000;

/// Every buffer of a run, flattened, for a byte-for-byte comparison.
///
/// **Two owners and not one.** Since ADR-086 a run's buffers are split between
/// `World` — what has a reader standing earlier in the tick than its writer —
/// and `process::Scratch`, everything else. A helper that walked only the first
/// would compare a quarter of the world, and `same_seed_and_config_give_byte_identical_state`
/// would hold over a world it cannot see. That is exactly the argument
/// `world/world.rs` used to keep the step-`b` buffers where they were, and it is
/// the argument that survives the move: the helper follows the buffers.
///
/// The **front** buffer of every lane, never `write_mut`: ADR-057 promises state
/// `N` in the front after every process, and comparing the write buffers would
/// compare whatever the last operator happened to leave behind. Buffers and not
/// a checksum, because a checksum hides a permutation of the lanes.
fn snapshot(world: &World, scratch: &Scratch) -> Vec<u8> {
    let mut bytes = world_snapshot(world);
    bytes.extend_from_slice(&scratch_snapshot(scratch));
    bytes
}

/// The half of [`snapshot`] the file on disk carries: three buffers and nothing
/// else.
///
/// The list is **destructured out of `OwnedBuffers`** and never retyped. Retyping
/// it is the forbidden form and the reason is in the shape of the defect ADR-086
/// found: `solar_in` was the twelfth buffer of a guard array declared to hold
/// eleven, and the header of the file that should have written it had warned in
/// as many words that "an enumeration that has quietly gone short reads to the
/// next author as exhaustive". A destructuring cannot go short: a tenth field of
/// `World` is `E0027` here.
fn world_snapshot(world: &World) -> Vec<u8> {
    let OwnedBuffers {
        amounts_32,
        amounts_64,
        enthalpy,
    } = world.owned_buffers();

    let mut bytes = Vec::new();
    if let Some(field) = amounts_32 {
        for value in field.read() {
            bytes.extend_from_slice(&value.raw().to_le_bytes());
        }
    }
    if let Some(field) = amounts_64 {
        for value in field.read() {
            bytes.extend_from_slice(&value.raw().to_le_bytes());
        }
    }
    for value in enthalpy.lane(0) {
        bytes.extend_from_slice(&value.raw().to_le_bytes());
    }
    bytes
}

/// The other half: every buffer of the tick's own owner.
///
/// The `Q` buffers go in by their bits and not by their value: `Q` is a wrapper
/// over an `f32` and `debug_f64` is the only way out of it, and the widening is
/// exact, so the bits of the `f64` distinguish exactly what the bits of the `f32`
/// do.
///
/// **Destructured out of `ScratchBuffers`** and never retyped, for the reason
/// `world_snapshot` destructures `OwnedBuffers`: a thirteenth buffer of `Scratch`
/// is `E0027` here. The earlier form of this helper walked four of the twelve
/// through four read accessors and was silent about the other eight — the defect
/// ADR-086 was written against, reproduced in the commit that applied it, which
/// is why the type came first and the helper second.
fn scratch_snapshot(scratch: &Scratch) -> Vec<u8> {
    let ScratchBuffers {
        face_courant,
        enthalpy_courant,
        energy_delta,
        xi_out,
        solar,
        light,
        velocity,
        velocity_potential,
        velocity_potential_coarse,
        velocity_potential_stirred,
        heat_capacity,
        temperature,
    } = scratch.buffers();

    let mut bytes = Vec::new();
    for value in energy_delta.iter().chain(solar) {
        bytes.extend_from_slice(&value.raw().to_le_bytes());
    }
    for value in xi_out {
        bytes.extend_from_slice(&value.raw().to_le_bytes());
    }
    for value in face_courant
        .iter()
        .chain(enthalpy_courant)
        .chain(light)
        .chain(velocity)
        .chain(velocity_potential)
        .chain(velocity_potential_coarse)
        .chain(velocity_potential_stirred)
        .chain(heat_capacity)
        .chain(temperature)
    {
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
    for value in world.enthalpy().lane(0) {
        bytes.extend_from_slice(&value.raw().to_le_bytes());
    }
    bytes
}

/// The energy the enthalpy field holds, summed the way `DomainSums` sums it.
fn heat_total(world: &World) -> i128 {
    world
        .enthalpy()
        .lane(0)
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
    let tick =
        Tick::new(world, derived, &config(), &roster(on), DT, DX, 42).expect("folding the tick");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut scratch = Scratch::new(world, &tick).expect("the scratch buffers");
    for t in 0..ticks {
        tick.advance(world, &mut ledger, &mut scratch, t, 0);
    }
}

/// One run of `ticks` ticks under one seed, returning **both** owners it ended
/// on.
///
/// Both, because a run's buffers live in two of them since ADR-086 and a
/// comparison that saw one would be a comparison of a quarter of the world.
fn run(ticks: u32, run_key: u32, on: &[ProcessId]) -> (World, Scratch) {
    let derived = derived();
    let mut world = world(&derived);
    let tick =
        Tick::new(&world, &derived, &config(), &roster(on), DT, DX, 42).expect("folding the tick");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch buffers");

    for t in 0..ticks {
        tick.advance(&mut world, &mut ledger, &mut scratch, t, run_key);
    }
    (world, scratch)
}

/// A world and a scratch that no tick has touched, for the "unchanged" side of
/// the bit-for-bit comparisons.
fn fresh() -> (World, Scratch) {
    let derived = derived();
    let world = world(&derived);
    let tick =
        Tick::new(&world, &derived, &config(), &roster(&[]), DT, DX, 42).expect("folding the tick");
    let scratch = Scratch::new(&world, &tick).expect("the scratch buffers");
    (world, scratch)
}

#[test]
#[ignore = "this cannot fail as it stands, and the honest half of ADR-075 is why. \
            The record calls the configuration reachable — a roster with \
            `reactions` off runs neither `h` nor `i'` — and in the same breath \
            says it does not create the dispatch site of step `i'`: \
            step `i'` is dispatched with step `h` and step `h` cannot be folded \
            while the mixer of ADR-027 is unnamed. So no roster credits \
            `SOLAR_IN`, `Scratch::solar` is written by nobody, and both \
            assertions below hold against a reduction moved into phase 5 as \
            readily as against the one in step `i'` — which is the alternative \
            they exist to separate. It comes back on the day step `i'` is \
            dispatched, and until then it is a name for a guard that guards \
            nothing rather than a criterion; weakening the name instead would \
            leave the shape of a live test around it"]
fn a_tick_without_the_fold_leaves_solar_in_untouched() {
    // ADR-075 puts the reduction of the solar slice inside step `i'` — the second
    // half of the same dispatch — rather than in phase 5. The property that buys
    // is structural: no fold, no credit, with nothing to guard it and no tick
    // stamp to keep.
    //
    // The configuration is the one the record names: a roster with `reactions`
    // off runs neither `h` nor `i'` by the rule at the head of `process/tick.rs`,
    // and the counter has to stand still. Moved into phase 5, the reduction would
    // run on a tick the fold did not, and the symptom would be a wrong residual on
    // the *next* tick, naming nobody. What is missing for the test to tell those
    // two apart is the other half of the pair — a roster that *does* dispatch the
    // fold — and that roster does not exist: `Tick::new` refuses `reactions`
    // outright, so the counter is at zero under every roster that builds.
    let derived = derived();
    let mut world = world(&derived);
    let tick = Tick::new(
        &world,
        &derived,
        &config(),
        &roster(&DISPATCHABLE),
        DT,
        DX,
        42,
    )
    .expect("folding");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch");

    // The premise: reactions are off in this roster, so step `i'` is not among
    // the steps this tick dispatches.
    assert!(!DISPATCHABLE.contains(&ProcessId::Reactions));

    for t in 0..8 {
        tick.advance(&mut world, &mut ledger, &mut scratch, t, 0);
        assert_eq!(
            ledger.energy(Channel::SolarIn),
            0,
            "SOLAR_IN moved on tick {t} without a fold to move it"
        );
    }

    // And the buffer the reduction would read is still the zeroed one it was
    // allocated as, which is what makes the zero above a statement about the
    // dispatch rather than about the arithmetic. It belongs to `Scratch` since
    // ADR-086: its writer is step `i'` and its reader the reduction standing
    // immediately behind it, both inside one tick.
    assert!(scratch.solar().iter().all(|c| c.to_i64() == 0));
}

/// `ACCEPTANCE.md`, section "Conservation" (ADR-080).
///
/// The second term of the matter identity has to vanish on a tick that ran no
/// chemistry, and vanish *by construction* rather than because nothing wrote to
/// it. `Ledger::begin_tick` clears the extent table exactly as it snapshots the
/// counters, and the clear is the one line this name guards.
///
/// The extent is planted before the run rather than left at its initial zero,
/// and that is the whole test. A table that is cleared only by its constructor
/// passes every version of this written the easy way — allocate, advance, assert
/// zero — because the value it would have carried over is a value nothing put
/// there. What it fails is a tick after a tick that reacted: last tick's `Xi`
/// standing while the field does not move is a residual that reports a
/// conversion nobody performed, which is the stale-accumulator failure ADR-045
/// removed the clearing pass for, arriving through the door built to detect it.
#[test]
fn a_tick_without_chemistry_leaves_every_extent_total_at_zero() {
    const N_REACTIONS: u32 = 2;

    let derived = derived();
    let mut world = world(&derived);
    let tick = Tick::new(
        &world,
        &derived,
        &config(),
        &roster(&DISPATCHABLE),
        DT,
        DX,
        42,
    )
    .expect("folding");
    let mut ledger =
        Ledger::with_reactions(N_SUBSTANCES, N_REACTIONS).expect("a ledger with room for extent");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch");

    // The premise: this roster dispatches no chemistry. `Tick::new` refuses an
    // enabled `reactions` outright today, so there is no other kind of roster to
    // compare against — which is stated here rather than left for a reader to
    // infer from a passing test.
    assert!(!DISPATCHABLE.contains(&ProcessId::Reactions));

    // A previous tick's chemistry, reduced into the table the way phase 5 would
    // reduce it. Two reactions and different totals, so a clear that zeroed only
    // the first slot is visible.
    let planted = [M32::new(3), M32::new(11), M32::new(5), M32::new(0)];
    ledger.reduce_extent(&planted, 2);
    assert_eq!(ledger.extent(0), 8, "the fixture did not plant anything");
    assert_eq!(ledger.extent(1), 11);

    for t in 0..4 {
        tick.advance(&mut world, &mut ledger, &mut scratch, t, 0);
        for r in 0..N_REACTIONS {
            assert_eq!(
                ledger.extent(r),
                0,
                "reaction {r} carried an extent of {} into tick {t}, which no \
                 dispatch put there",
                ledger.extent(r)
            );
        }
    }
}

/// `ACCEPTANCE.md`, section "Conservation" (ADR-080).
///
/// The third arm of `Conservation` belongs to step `h` and to nothing else, on
/// **both** axes. Two mistakes are guarded and they are not the same mistake:
///
/// - a transport process, or the temperature operator, quietly declaring
///   `Transmutes` — which would be a claim that its changes are whole multiples
///   of load-checked vectors, and there are none;
/// - anything declaring it for **energy**. ADR-080 gives the arm content on the
///   matter axis only; on the energy axis ADR-081 leaves step `h` declaring
///   `Conserved`, because the weighted left side of that record does not move
///   under a reaction and there is nothing to report. The enum is one enum, so
///   the wrong axis type-checks.
///
/// Every operator of the roster is built here rather than looped over, because
/// there is no table from a `ProcessId` to an `Invariant` — the declarations live
/// on the operators, and a loop could only compare one of them with itself. The
/// two grids are both used: the transport arms depend on whether the lid vents,
/// and `ChangedThrough` is the arm that would hide a `Transmutes` written one
/// line above it.
#[test]
fn only_the_reaction_step_declares_transmutes() {
    use liminis_core::process::light::{Light, Modulation};
    use liminis_core::process::pressure::Pressure;
    use liminis_core::process::velocity::{VelocityConfig, VelocityField};
    use liminis_core::process::{
        AdvectPhase, Conservation, DiffusePhase, Grain, Invariant, Medium, SettlePhase,
        Temperature, channels, phase, react,
    };

    let config = config::parse(SCENARIO).expect("the fixture must parse");
    let derived = derived();
    let world = world(&derived);
    let sealed = Grid::new(N, N, N, [Boundary::Closed; 6]).expect("a sealed grid");
    let venting = Grid::new(
        N,
        N,
        N,
        [
            Boundary::Closed,
            Boundary::Closed,
            Boundary::Closed,
            Boundary::Closed,
            Boundary::Closed,
            Boundary::Exchange,
        ],
    )
    .expect("a venting grid");

    const K_EX: f64 = 1.0e-6;
    const D: f64 = 1.0e-9;

    let medium = Medium {
        rho_medium: 1_000.0,
        g: 9.81,
        mu: 1.0e-3,
    };
    // A grain that does not settle at all, so that no number here has to be one
    // the corpus refuses to name (ADR-067, `TODO` in `process/settle.rs`).
    let grain = Grain {
        settling_radius: 0.0,
        molar_mass: 2_650.0,
        partial_molar_volume: 1.0e-3,
    };
    let velocity_cfg = VelocityConfig {
        u_conv_max: DX / (6.0 * DT),
        // `r = round(l_c / (2 * dx_coarse))` cells of the enthalpy grid, and
        // `dx_coarse` is `4 * dx` at `lod = 2`, so this is exactly one cell.
        l_c: 8.0 * DX,
        stir_fraction: 0.0,
        stir_period: None,
        dt: DT,
        dx: DX,
        t_min: 273.15,
        t_max: 313.15,
        units_per_joule: 1.0,
        every_n_ticks: 1,
    };

    let mut declared: Vec<(&str, Invariant)> = Vec::new();
    // The two transport processes over both grids: theirs is the only arm in the
    // roster that depends on the boundary, and `ChangedThrough` on the venting
    // one is the neighbour a stray `Transmutes` would hide behind.
    for (name, grid) in [("sealed", &sealed), ("venting", &venting)] {
        declared.push((
            name,
            AdvectPhase::new_32(grid, 1, DT, DX)
                .expect("advection folds")
                .invariant(),
        ));
        declared.push((
            name,
            DiffusePhase::new_32(grid, 1, &[D], DT, DX, K_EX)
                .expect("diffusion folds")
                .invariant(),
        ));
    }
    // The other three take the sealed grid and no other: settling, pressure and
    // the velocity field **refuse** an exchange face outright, because no record
    // says what they do at one (ADR-059, ADR-067). That refusal is a fact about
    // those operators rather than an inconvenience of this fixture, and asking
    // for the venting grid here would only assert it a second time.
    declared.push((
        "settling",
        SettlePhase::new_32(&sealed, 1, &[grain], &medium, DT, DX)
            .expect("settling folds")
            .invariant(),
    ));
    declared.push((
        "light",
        Light::new(&sealed, &[], 0.0, Modulation::NONE, DX)
            .expect("light folds")
            .invariant(),
    ));
    // `theta_max` and `v_voxel` are a positive number each: the record that
    // assigns the first does not exist (`TODO(theta-max)`), and the arm below
    // does not depend on either.
    declared.push((
        "pressure",
        Pressure::new(&sealed, 1.0, &[], DX * DX * DX)
            .expect("pressure folds")
            .invariant(),
    ));
    // Three grids of its own rather than the world's: at `N = 4` and `lod = 2`
    // the enthalpy grid is a single cell, and ADR-069 bounds the half-width of
    // the wide difference at a quarter of that grid's extent — one cell has no
    // room for it. Nothing about the arm depends on the size.
    let fine = Grid::new(16, 16, 16, [Boundary::Closed; 6]).expect("a fine grid");
    let velocity_grid = Grid::new(8, 8, 8, [Boundary::Closed; 6]).expect("a velocity grid");
    let enthalpy_grid = Grid::new(4, 4, 4, [Boundary::Closed; 6]).expect("an enthalpy grid");
    declared.push((
        "velocity",
        VelocityField::new(&fine, &velocity_grid, &enthalpy_grid, &velocity_cfg)
            .expect("the velocity field folds")
            .invariant(),
    ));
    declared.push((
        "temperature",
        Temperature::new(
            world.grid(),
            world.enthalpy_grid(),
            2,
            world.registry(),
            &derived,
            &config,
        )
        .expect("the temperature operator folds")
        .invariant(),
    ));
    // The two roster entries with no operator at all (ADR-065).
    declared.push(("phase_transitions", phase::invariant()));
    declared.push(("external_channels", channels::invariant()));

    // Two transport processes over two grids, five more operators, and the two
    // roster entries that have none: eleven declarations. The count is written
    // out so that a process quietly dropped from the list above is a failure
    // here rather than a silently weaker claim.
    assert_eq!(declared.len(), 11);

    for (name, invariant) in &declared {
        assert_ne!(
            invariant.matter,
            Conservation::Transmutes,
            "{name} declares Transmutes for matter, and step `h` is the only step \
             with load-checked vectors to transmute by (ADR-080)"
        );
        assert_ne!(
            invariant.energy,
            Conservation::Transmutes,
            "{name} declares Transmutes for energy. The arm has content on the \
             matter axis only: on the energy axis ADR-081 leaves even step `h` \
             declaring Conserved, because a reaction does not move the weighted \
             left side at all"
        );
    }

    // And the one that does. Step `h` on the matter axis, from the module that
    // owns the declaration — and now on **both** axes, because `React::invariant`
    // finally exists (ADR-081). Until it did, the energy half of the assertion
    // above had nothing to read: every arm in the loop belonged to a process that
    // is not step `h`, so "no process declares Transmutes for energy" was true of
    // a list that did not contain the one process it was about.
    assert_eq!(react::matter_conservation(), Conservation::Transmutes);
    assert_eq!(
        react::invariant(),
        Invariant {
            matter: Conservation::Transmutes,
            energy: Conservation::Conserved,
        },
        "step `h` transmutes matter and conserves energy — literally, with no \
         second term on the right and no channel of its own (ADR-080, ADR-081)"
    );
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
    let tick = Tick::new(
        &world,
        &derived,
        &config(),
        &roster(&DISPATCHABLE),
        DT,
        DX,
        42,
    )
    .expect("folding");
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
                ledger.residual_matter(Nu::EMPTY, s, &before, &after),
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
            `i'`, and the two things that used to block it are gone: ADR-076 \
            declares i_surface and derives units_per_intensity, ADR-075 settles \
            what the fold owes the ledger and A-16 is closed. What blocks it now \
            is that no scenario may be lit at all — refused now by one lock, an \
            energy sink that exists nowhere; the second, the width of a channel \
            counter, was answered by ADR-083 — and that step `i'` is dispatched \
            with step `h` or not at \
            all, while step `h` reads a temperature no operator produces for it. \
            The per-tick half above is no longer vacuous: steps `c` and `d` \
            transport the enthalpy field, so energy moves inside the domain and a \
            transport that lost a joule would show here. What is still missing is \
            a *source* — the reaction energy of an exothermic reaction has no \
            channel and no door in DomainSums, and enthalpy_formation enters no \
            left-hand side (see the module header of ledger/mod.rs) — so with the \
            light off the right-hand side is identically zero"]
fn energy_ledger_residual_is_zero_over_10k_ticks() {
    let derived = derived();
    let mut world = world(&derived);
    let tick = Tick::new(
        &world,
        &derived,
        &config(),
        &roster(&DISPATCHABLE),
        DT,
        DX,
        42,
    )
    .expect("folding");
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
    let (left, left_scratch) = run(64, 7, &DISPATCHABLE);
    let (right, right_scratch) = run(64, 7, &DISPATCHABLE);
    assert_eq!(
        snapshot(&left, &left_scratch),
        snapshot(&right, &right_scratch),
        "two runs of one seed and one config diverged"
    );
}

#[test]
#[ignore = "no dispatchable process consumes run_key, and this note now owes two \
            statements rather than one. Its two consumers are React::params \
            (ADR-058) and NoiseParams in the velocity field (ADR-069). The \
            reactions are blocked by the two arches of the invariant under \
            chemistry and no longer by a temperature nobody derives: \
            `process/temperature.rs` derives it (ADR-079). The velocity field is \
            no longer blocked at all: `Derived` carries the four keys of \
            ADR-069 and the fold onto the faces of the enthalpy grid is the fifth \
            stage of the operator (ADR-087), so a roster that enables the field \
            over a scenario that declares `u_conv_max` dispatches step `b`. \
            The fixture in this file is not such a scenario — it writes no \
            `[[process]]` section at all — and that is deliberate, because the \
            second statement is what this criterion is about. \
            The second statement is about this criterion rather than about the \
            dispatch, and it is the one that was never checked: a step `b` that \
            dispatched perfectly would leave the two runs below *equal anyway*. \
            `VelocityField::apply` branches on `stirs` on the host, `run_key` \
            enters `NoiseParams` and nothing else, and the fixture in this file \
            writes no `[[process]]` section at all, so its `stir_fraction` is the \
            default zero and the whole field is a function of the enthalpy and the \
            heat capacity. So this criterion needs a stirring scenario as well as a \
            dispatch, and the honest form of it meanwhile is a red test that says \
            so rather than a weakened one that passes. Weakening it — comparing \
            the run keys, or asserting only that the tick forwards the argument — \
            is the temptation to name out loud: it would leave the actual failure, \
            a tick that forwards `tick` and drops `run_key`, invisible while its \
            neighbour above stays green"]
fn different_seed_gives_different_state() {
    // A statement about this pair of seeds and not a theorem: `run_key` collapses
    // a `u64` seed into a `u32` (ADR-058), so some pair of seeds collides by
    // construction.
    let (left, left_scratch) = run(64, 7, &DISPATCHABLE);
    let (right, right_scratch) = run(64, 8, &DISPATCHABLE);
    assert_ne!(
        snapshot(&left, &left_scratch),
        snapshot(&right, &right_scratch),
        "two seeds gave the same world"
    );
}

#[test]
fn snapshot_covers_every_buffer_the_world_owns() {
    // `world_snapshot` promises to hold every buffer of a `World`, and since
    // ADR-086 the compiler holds it to the *count*: the helper destructures
    // `OwnedBuffers`, whose constructor destructures `World` with no rest
    // pattern, so a tenth field is `E0027` in three places at once. What the
    // compiler still cannot do is make the new binding be **written** — a `_`
    // compiles — so the list below is what says each of the three actually
    // reaches the bytes.
    //
    // **Per buffer, and never by a byte total alone.** The mistake this guard
    // exists for is a copy-paste among near-identical loops — repeat one
    // accessor, drop another — and a total is blind to exactly that whenever two
    // buffers share a length.
    //
    // Three rows and not eleven, and they are **destructured out of
    // `OwnedBuffers`** rather than retyped. Retyping the array as `[(&str,
    // Bump); 3]` by deleting eight lines is the forbidden form: that is the exact
    // shape of the defect ADR-086 fixed, an enumeration that had quietly gone
    // short — `solar_in` was the twelfth buffer of an array declared to hold
    // eleven.
    let derived = derived();
    let fixture = world(&derived);
    let OwnedBuffers {
        amounts_32,
        amounts_64,
        enthalpy,
    } = fixture.owned_buffers();

    type Bump = fn(&mut World);
    let bumps: [(&str, bool, Bump); 3] = [
        ("amounts_32", amounts_32.is_some(), |world| {
            // Through the *back* buffer and a swap, because `snapshot` reads the
            // front one (ADR-057) and there is no door that writes it directly.
            let field = world.amounts_32_mut().expect("four narrow substances");
            let (src, dst) = field.pair_mut();
            dst.copy_from_slice(src);
            dst[0] = M32::from_i64_clamping(dst[0].to_i64() + 1);
            field.swap();
        }),
        ("amounts_64", amounts_64.is_some(), |world| {
            let field = world.amounts_64_mut().expect("WATER is the wide substance");
            let (src, dst) = field.pair_mut();
            dst.copy_from_slice(src);
            dst[0] = M64::new(dst[0].to_i64() + 1);
            field.swap();
        }),
        ("enthalpy", !enthalpy.lane(0).is_empty(), |world| {
            let field = world.enthalpy_mut();
            let (src, dst) = field.pair_mut();
            dst.copy_from_slice(src);
            dst[0] = M64::new(dst[0].to_i64() + 1);
            field.swap();
        }),
    ];

    for (name, present, bump) in bumps {
        assert!(present, "the fixture does not allocate `{name}`");
        let mut world = world(&derived);
        let before = world_snapshot(&world);
        bump(&mut world);
        assert_ne!(
            world_snapshot(&world),
            before,
            "`snapshot` does not cover `{name}`"
        );
    }

    // And the byte total on top, which catches the other half of the same
    // copy-paste: an accessor repeated rather than dropped. The loop above cannot
    // see that — a buffer covered twice still moves when it is perturbed.
    //
    // Counted in bytes because that is what the helper produces: `M32` goes in
    // four bytes wide and `M64` eight. There is no `Q` term left in it, and that
    // is the criterion seen from the format side (ADR-086).
    let world = world(&derived);
    let OwnedBuffers {
        amounts_32,
        amounts_64,
        enthalpy,
    } = world.owned_buffers();
    let mut expected = 0;
    if let Some(field) = amounts_32 {
        expected += field.read().len() * 4;
    }
    if let Some(field) = amounts_64 {
        expected += field.read().len() * 8;
    }
    expected += enthalpy.lane(0).len() * 8;

    assert_eq!(
        world_snapshot(&world).len(),
        expected,
        "a buffer of the world is counted twice by `world_snapshot`, or is \
         missing from both this list and the list above"
    );
}

#[test]
fn every_buffer_the_scratch_owns_is_absent_from_the_snapshot() {
    // The twin of the guard above, walking the other owner, and named from what
    // it is: a guard of the *classification*. `snapshot_covers_every_buffer_the_scratch_owns`
    // would read as the opposite of the decision.
    //
    // **What the second assertion of each round is worth, said plainly.**
    // `snapshot_write(sink, &SnapshotIdentity, &World, &Ledger)` takes no
    // `Scratch` at all, so no mutation of one can reach the file under any
    // implementation that compiles: today that equality is the parameter list
    // restated, and it cannot go red. It is kept, and kept honest by this
    // paragraph, because it is the round that arms itself the day `snapshot_write`
    // is handed a `Scratch` — and it would not have caught
    // `world.energy_delta()` at `snapshot.rs:182`, which was a field of `World`
    // when it was written and reached the file through the door for `World`. The
    // guard against *that* is `the_snapshot_holds_no_buffer_of_class_q`, which
    // computes the file's length out of the world's `M` buffers alone.
    //
    // **The first assertion of each round is the one that can fail**, and it is
    // what makes the promise "every buffer" true rather than stated: each of the
    // twelve is perturbed **alone**, through the exhaustive `ScratchBuffersMut`,
    // and `scratch_snapshot` has to move for each. A helper that walked four of
    // the twelve — which is what it did on the day ADR-086 was applied — goes red
    // here eight times over, and until this round existed the determinism
    // criteria resting on that helper said nothing about eight buffers of the run
    // while their comments named all of them.
    let derived = derived();
    let world = world(&derived);
    let tick =
        Tick::new(&world, &derived, &config(), &roster(&[]), DT, DX, 42).expect("folding the tick");

    let mut file = Vec::new();
    snapshot_write(
        &mut file,
        &identity(),
        &world,
        &Ledger::new(N_SUBSTANCES).unwrap(),
    )
    .expect("writing the snapshot");

    // One perturbation per buffer, in the order `ScratchBuffers` declares them.
    // `Q` moves through `qadd` and never through `Q::from_f64(x.debug_f64() +
    // 1.0)`: the debug door is mode dependent and the rule about bare operators
    // holds in a test as much as in a kernel (ADR-022).
    type Bump = fn(&mut Scratch);
    let bumps: [(&str, Bump); 12] = [
        ("face_courant", |s| {
            bump_q(&mut s.buffers_mut().face_courant[0]);
        }),
        ("enthalpy_courant", |s| {
            bump_q(&mut s.buffers_mut().enthalpy_courant[0]);
        }),
        ("energy_delta", |s| {
            let cell = &mut s.buffers_mut().energy_delta[0];
            *cell = M64::new(cell.to_i64() + 1);
        }),
        ("xi_out", |s| {
            let cell = &mut s.buffers_mut().xi_out[0];
            *cell = M32::from_i64_clamping(cell.to_i64() + 1);
        }),
        ("solar", |s| {
            let cell = &mut s.buffers_mut().solar[0];
            *cell = M64::new(cell.to_i64() + 1);
        }),
        ("light", |s| bump_q(&mut s.buffers_mut().light[0])),
        ("velocity", |s| bump_q(&mut s.buffers_mut().velocity[0])),
        ("velocity_potential", |s| {
            bump_q(&mut s.buffers_mut().velocity_potential[0]);
        }),
        ("velocity_potential_coarse", |s| {
            bump_q(&mut s.buffers_mut().velocity_potential_coarse[0]);
        }),
        ("velocity_potential_stirred", |s| {
            bump_q(&mut s.buffers_mut().velocity_potential_stirred[0]);
        }),
        ("heat_capacity", |s| {
            bump_q(&mut s.buffers_mut().heat_capacity[0]);
        }),
        ("temperature", |s| {
            bump_q(&mut s.buffers_mut().temperature[0]);
        }),
    ];

    // The count is held against the type and not against the literal above: a
    // thirteenth buffer is `E0027` in this destructuring, the author has to name
    // it in `lengths`, and the equality below then goes red until it is named in
    // `bumps` as well. The limit is the one `OwnedBuffers` states in the same
    // words — a `_`-binding compiles, so what the compiler converts is "forgot"
    // into "decided, in a visible diff line", and not into "impossible".
    let scratch = Scratch::new(&world, &tick).expect("the scratch buffers");
    let ScratchBuffers {
        face_courant,
        enthalpy_courant,
        energy_delta,
        xi_out,
        solar,
        light,
        velocity,
        velocity_potential,
        velocity_potential_coarse,
        velocity_potential_stirred,
        heat_capacity,
        temperature,
    } = scratch.buffers();
    let lengths = [
        face_courant.len(),
        enthalpy_courant.len(),
        energy_delta.len(),
        xi_out.len(),
        solar.len(),
        light.len(),
        velocity.len(),
        velocity_potential.len(),
        velocity_potential_coarse.len(),
        velocity_potential_stirred.len(),
        heat_capacity.len(),
        temperature.len(),
    ];
    assert_eq!(
        bumps.len(),
        lengths.len(),
        "a buffer of `Scratch` is perturbed by nobody below"
    );

    for ((name, bump), len) in bumps.into_iter().zip(lengths) {
        // The premise of the round: an empty buffer cannot witness anything, and
        // a fixture that allocated one would make the round green by vacancy.
        assert!(len > 0, "the fixture allocates `{name}` empty");

        let mut scratch = Scratch::new(&world, &tick).expect("the scratch buffers");
        let quiet = scratch_snapshot(&scratch);
        bump(&mut scratch);
        assert_ne!(
            scratch_snapshot(&scratch),
            quiet,
            "`scratch_snapshot` does not cover `{name}`"
        );

        let mut after = Vec::new();
        snapshot_write(
            &mut after,
            &identity(),
            &world,
            &Ledger::new(N_SUBSTANCES).unwrap(),
        )
        .expect("writing the snapshot");
        assert_eq!(
            file, after,
            "`{name}` is a buffer of `Scratch` and it reaches the snapshot file"
        );
    }

    // And the poison on top, which is the instrument the classification criterion
    // itself uses: it has to move every buffer the helper can see, or
    // `poisoning_the_scratch_before_a_tick_changes_no_buffer_of_the_world` is
    // green about buffers it never touched.
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch buffers");
    let quiet = scratch_snapshot(&scratch);
    scratch.poison(0x5A5A_5A5A);
    assert_ne!(
        scratch_snapshot(&scratch),
        quiet,
        "`Scratch::poison` moved no buffer this helper can see"
    );
}

#[test]
fn load_reports_the_per_voxel_footprint_of_the_world_and_the_scratch() {
    // What ADR-086 gives instead of a memory ceiling in bytes, and it is offered
    // *instead* rather than beside: every factor of the product is bounded
    // already — `S_MAX = 32`, `R_MAX = 64`, `N_MAX = 64` — so a limit would have
    // to be invented, and as a scenario key it would be raised to whatever the
    // scenario liked. The line is what a run at `R = 64` reads its gigabyte from
    // before it asks for it.
    //
    // **The projection is held against the allocation**, which is the half that
    // can rot: `Footprint` computes the scratch from the shapes so that it can be
    // printed before `Scratch::new` runs, and two computations of one number is
    // exactly the arrangement that drifts. The measured side goes through
    // `ScratchBuffers`, so a thirteenth buffer moves the measurement and fails
    // here unless the projection moved with it.
    let derived = derived();
    let world = world(&derived);
    let tick =
        Tick::new(&world, &derived, &config(), &roster(&[]), DT, DX, 42).expect("folding the tick");
    let footprint = Footprint::of(&world, &tick).expect("the footprint of the fixture");
    let scratch = Scratch::new(&world, &tick).expect("the scratch buffers");

    let ScratchBuffers {
        face_courant,
        enthalpy_courant,
        energy_delta,
        xi_out,
        solar,
        light,
        velocity,
        velocity_potential,
        velocity_potential_coarse,
        velocity_potential_stirred,
        heat_capacity,
        temperature,
    } = scratch.buffers();
    let allocated = size_of_val(face_courant)
        + size_of_val(enthalpy_courant)
        + size_of_val(energy_delta)
        + size_of_val(xi_out)
        + size_of_val(solar)
        + size_of_val(light)
        + size_of_val(velocity)
        + size_of_val(velocity_potential)
        + size_of_val(velocity_potential_coarse)
        + size_of_val(velocity_potential_stirred)
        + size_of_val(heat_capacity)
        + size_of_val(temperature);
    assert_eq!(
        footprint.scratch_bytes(),
        allocated,
        "the projected scratch footprint is not what `Scratch::new` allocates"
    );

    // The world's side, measured the way the snapshot measures it: both buffers
    // of every field, because both are allocated.
    let OwnedBuffers {
        amounts_32,
        amounts_64,
        enthalpy,
    } = world.owned_buffers();
    let mut expected = 2 * size_of_val(enthalpy.read());
    if let Some(field) = amounts_32 {
        expected += 2 * size_of_val(field.read());
    }
    if let Some(field) = amounts_64 {
        expected += 2 * size_of_val(field.read());
    }
    assert_eq!(footprint.world_bytes(), expected);

    // And the line itself carries three per-voxel figures and their sum, computed
    // rather than spelled: the fixture's own numbers are substituted into the
    // text, so a report that printed a literal or dropped a term goes red here.
    let n_voxels = f64::from(world.grid().n_voxels());
    let report = footprint.report();
    for (label, bytes) in [
        ("world", footprint.world_bytes()),
        ("scratch", footprint.scratch_bytes()),
        (
            "together",
            footprint.world_bytes() + footprint.scratch_bytes(),
        ),
    ] {
        let printed = format!("{label} {:.1} B/voxel", bytes as f64 / n_voxels);
        assert!(
            report.contains(&printed),
            "the report does not print `{printed}`; it said:\n{report}"
        );
    }
}

#[test]
fn the_snapshot_holds_no_buffer_of_class_q() {
    // The criterion seen from the format side, and what makes ADR-016's "restart
    // from any snapshot" provable rather than promised: `Q` has no
    // mode-independent byte door (`TODO(snapshot-q)`), so a file that held one
    // would be a file whose meaning depended on the build.
    //
    // The length is computed from the world's own `M` buffers and never from a
    // literal: a `Q` buffer written would make the file longer by a multiple of
    // eight bytes, and a literal would have to be edited to notice.
    let derived = derived();
    let world = world(&derived);
    let ledger = Ledger::new(N_SUBSTANCES).unwrap();

    let mut bytes = Vec::new();
    snapshot_write(&mut bytes, &identity(), &world, &ledger).expect("writing the snapshot");

    let OwnedBuffers {
        amounts_32,
        amounts_64,
        enthalpy,
    } = world.owned_buffers();

    // The header: magic, format, seed, world format, tick, and the length-prefixed
    // config hash, then four shape words.
    let header = 8 + 4 + 8 + 4 + 4 + (4 + identity().config_hash.len()) + 4 * 4;
    // Both buffers of every substance lane, in substance order, at that lane's
    // own width.
    let mut lanes = 0;
    for s in 0..N_SUBSTANCES {
        lanes += match world.lane_of(s) {
            LaneRef::Narrow(lane) => 2 * amounts_32.expect("a narrow field").lane(lane).len() * 4,
            LaneRef::Wide(lane) => 2 * amounts_64.expect("a wide field").lane(lane).len() * 8,
        };
    }
    // Both buffers of the enthalpy lane.
    let heat = 2 * enthalpy.lane(0).len() * 8;
    // The counter block: sixteen bytes per channel per substance, plus sixteen
    // per channel for energy (ADR-083).
    let counters = 16 * Channel::ALL.len() * (N_SUBSTANCES as usize + 1);

    assert_eq!(
        bytes.len(),
        header + lanes + heat + counters,
        "the snapshot is not exactly the header, the M buffers and the counters"
    );
}

#[test]
fn a_restart_continues_the_run_bit_for_bit_from_the_tick_it_was_taken_at() {
    // The promise of ADR-016, and since ADR-086 it is satisfiable rather than
    // provably unsatisfiable: the file holds only class `M`, and `Q` has no
    // mode-independent byte door.
    //
    // Live, and that is the whole difference from the variant ADR-086 rejected —
    // advection and diffusion dispatch today, so a mid-run snapshot can actually
    // be taken, poured and finished. It fails if any buffer with a reader
    // standing earlier than its writer was classified into `Scratch`.
    const HALF: u32 = 6;

    let derived = derived();

    // The continuous run: 2*HALF ticks, with the file taken at the halfway
    // boundary.
    let mut straight = world(&derived);
    let tick = Tick::new(
        &straight,
        &derived,
        &config(),
        &roster(&DISPATCHABLE),
        DT,
        DX,
        42,
    )
    .expect("folding the tick");
    let mut straight_ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut straight_scratch = Scratch::new(&straight, &tick).expect("the scratch buffers");
    for t in 0..HALF {
        tick.advance(
            &mut straight,
            &mut straight_ledger,
            &mut straight_scratch,
            t,
            0,
        );
    }

    let mut file = Vec::new();
    snapshot_write(&mut file, &identity(), &straight, &straight_ledger)
        .expect("writing the snapshot");

    for t in HALF..2 * HALF {
        tick.advance(
            &mut straight,
            &mut straight_ledger,
            &mut straight_scratch,
            t,
            0,
        );
    }

    // The restarted run: a world built from the same scenario, poured, and
    // finished. Its `Scratch` is a fresh one — nothing of it is carried, which is
    // the claim.
    let mut restarted = world(&derived);
    let mut restarted_ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let at = snapshot_read_into(
        &file[..],
        &identity(),
        &mut restarted,
        &mut restarted_ledger,
    )
    .expect("pouring the snapshot");
    assert_eq!(at, identity().tick);

    let mut restarted_scratch = Scratch::new(&restarted, &tick).expect("the scratch buffers");
    for t in HALF..2 * HALF {
        tick.advance(
            &mut restarted,
            &mut restarted_ledger,
            &mut restarted_scratch,
            t,
            0,
        );
    }

    // The premise: the run moved over the second half, or the equality below is
    // about two worlds that stood still.
    assert_ne!(
        world_snapshot(&straight),
        Vec::new(),
        "the fixture allocates no buffers"
    );

    assert_eq!(
        world_snapshot(&straight),
        world_snapshot(&restarted),
        "a restart from the middle of the run did not continue it bit for bit"
    );
    for channel in Channel::ALL {
        for s in 0..N_SUBSTANCES {
            assert_eq!(
                straight_ledger.matter(channel, s),
                restarted_ledger.matter(channel, s),
                "channel {} of substance {s} differs after the restart",
                channel.name()
            );
        }
        assert_eq!(
            straight_ledger.energy(channel),
            restarted_ledger.energy(channel),
            "the energy counter of channel {} differs after the restart",
            channel.name()
        );
    }
}

#[test]
fn poisoning_the_scratch_before_a_tick_changes_no_buffer_of_the_world() {
    // The operational form of the ownership criterion itself (ADR-086): fill
    // every buffer of `Scratch` with rubbish before a tick, and the world after
    // the tick must match a run whose scratch was zeroed, bit for bit. A buffer
    // classified into `Scratch` that in truth carries information across a tick
    // boundary fails here and nowhere else.
    //
    // **Advection is excluded, and the reason is a real hole rather than a
    // convenience.** Step `c` reads `face_courant`, whose writer is the last
    // stage of step `b`, and step `b` does not dispatch — so today that one
    // buffer is read without having been written this tick, and poisoning it
    // legitimately moves matter. The exclusion disappears the day step `b`
    // builds. The tempting fix, zeroing the Courant buffers at the top of
    // `Tick::advance`, is the clearing pass ADR-045 spent a decision removing, it
    // costs a 25.17 MB memset per tick at 128^3, and it would mask the precise
    // failure `every_n_ticks_on_the_velocity_field_is_rejected` exists to
    // prevent.
    const WITHOUT_ADVECTION: [ProcessId; 1] = [ProcessId::Diffusion];
    assert!(
        !WITHOUT_ADVECTION.contains(&ProcessId::Advection),
        "step `c` reads a Courant buffer step `b` does not write yet"
    );

    let derived = derived();
    let mut clean = world(&derived);
    let mut poisoned = world(&derived);
    let tick = Tick::new(
        &clean,
        &derived,
        &config(),
        &roster(&WITHOUT_ADVECTION),
        DT,
        DX,
        42,
    )
    .expect("folding the tick");

    let mut clean_scratch = Scratch::new(&clean, &tick).expect("the scratch buffers");
    let mut poisoned_scratch = Scratch::new(&poisoned, &tick).expect("the scratch buffers");
    poisoned_scratch.poison(0x0BAD_F00D);

    let mut clean_ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut poisoned_ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    for t in 0..4 {
        tick.advance(&mut clean, &mut clean_ledger, &mut clean_scratch, t, 0);
        tick.advance(
            &mut poisoned,
            &mut poisoned_ledger,
            &mut poisoned_scratch,
            t,
            0,
        );
    }

    assert_eq!(
        world_snapshot(&clean),
        world_snapshot(&poisoned),
        "a buffer classified into `Scratch` carries information across the tick \
         boundary"
    );
}

#[test]
fn the_denominator_is_fresh_for_step_b_and_for_step_h() {
    // ADR-086: the denominator is recomputed **at each of its two readers**, so
    // the value each of them sees is derived from the composition standing at
    // that reader rather than from one recomputation serving both. The name is
    // the record's own and is deliberately not
    // `the_denominator_is_recomputed_before_each_of_its_two_readers`, so that it
    // cannot be confused with the live unit test
    // `the_denominator_is_recomputed_and_not_cached` in `kernels/temperature.rs`:
    // that one asserts there is no cache, this one that there is freshness at
    // both readers.
    //
    // The tail half is what can be seen today. Poison the scratch, run one tick
    // of diffusion over a non-uniform field, and the denominator left in the
    // buffer has to be the one the operator gives over the **post-transport**
    // state — and not the one it gives over the state the tick started from.
    //
    // `N_HEAT` and not `N`: at the smaller edge the enthalpy grid is a single
    // cell, transport cannot move anything **between** coarse cells, and both
    // recomputations agree for a reason that has nothing to do with freshness.
    //
    // The head half needs a consumer standing between the two passes, and there
    // is none: step `b` does not dispatch, two passes over an unchanged state are
    // bit-identical to one, and over a changed state only the last survives in
    // the buffer. So **the head pass has no witness at all today**, and saying
    // that plainly is the point: `the_temperature_has_a_slot_before_step_b_and_before_step_h`
    // reads the table `Tick::advance` consults and says nothing about the tick.
    // Half of the record's criterion is checked here and the other half arrives
    // with step `b`, which is the reader that can tell the two passes apart.
    let derived = derived();
    let config = config();
    let mut world = world_sized(&derived, N_HEAT, Heat::Varying);

    let operator = Temperature::new(
        world.grid(),
        world.enthalpy_grid(),
        ENTHALPY_LOD,
        world.registry(),
        &derived,
        &config,
    )
    .expect("folding the temperature operator");
    let cells = operator.n_cells() as usize;
    assert!(
        cells > 1,
        "a single coarse cell cannot see transport at all"
    );

    // What the operator answers over the state the tick starts from.
    let mut before = vec![Q::ZERO; cells];
    let mut before_t = vec![Q::ZERO; cells];
    {
        let (src32, src64) = world.amount_slices();
        operator.apply(
            src32,
            src64,
            world.enthalpy().lane(0),
            &mut before,
            &mut before_t,
        );
    }

    let tick = Tick::new(
        &world,
        &derived,
        &config,
        &roster(&[ProcessId::Diffusion]),
        DT,
        DX,
        42,
    )
    .expect("folding the tick");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch buffers");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    scratch.poison(0x0BAD_F00D);
    tick.advance(&mut world, &mut ledger, &mut scratch, 0, 0);

    // And what it answers over the state the tick ended on.
    let mut after = vec![Q::ZERO; cells];
    let mut after_t = vec![Q::ZERO; cells];
    {
        let (src32, src64) = world.amount_slices();
        operator.apply(
            src32,
            src64,
            world.enthalpy().lane(0),
            &mut after,
            &mut after_t,
        );
    }

    // The premise: transport moved the composition between coarse cells, or the
    // equality below holds for a tick that recomputed nothing.
    assert_ne!(
        before, after,
        "diffusion moved no composition across a coarse boundary, so this \
         fixture cannot tell a fresh denominator from a stale one"
    );

    assert_eq!(
        scratch.heat_capacity(),
        &after[..],
        "the denominator step `h` reads was not computed on the composition the \
         voxels hold after transport"
    );
    assert_eq!(
        scratch.temperature(),
        &after_t[..],
        "the temperature step `h` reads is not the one the post-transport state \
         gives"
    );
}

#[test]
fn the_fold_credits_no_solar_energy_when_the_light_step_does_not_run() {
    // `FoldParams::i_surface` is the upper absorption term of the topmost layer,
    // and it is the only term of that difference which is not in the field. Over
    // a zeroed light field it is `i_surface` entire: a non-zero value with step
    // `a` off is a perfect silent heater whose energy residual stays at **exactly
    // zero**, because `SOLAR_IN` is credited the same number. `FoldParams` is a
    // `Copy` struct mirroring a WGSL uniform (ADR-015), so no type can prevent
    // it; this test and `react::fold_params` are what stand there.
    let derived = derived();
    let world = world(&derived);
    let tick = Tick::new(
        &world,
        &derived,
        &config(),
        &roster(&DISPATCHABLE),
        DT,
        DX,
        42,
    )
    .expect("folding the tick");

    // The premise: this roster does not run step `a`.
    assert!(!DISPATCHABLE.contains(&ProcessId::Light));
    assert_eq!(
        tick.fold_params().i_surface,
        Q::ZERO,
        "the fold would create a layer of energy out of a light field nobody wrote"
    );
}

/// One `Q` cell, moved by one. The wrapper exists because the rule about bare
/// operators over `Q` (ADR-022) holds outside `kernels/` as well: `*cell + 1.0`
/// does not compile, and reaching for `Q::from_f64(cell.debug_f64() + 1.0)`
/// instead would go out through the debug door and back, which is mode dependent.
fn bump_q(cell: &mut Q) {
    *cell = qadd(*cell, Q::ONE);
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
    // nine, and on **this** scenario seven of them refuse. Six have no operator;
    // the seventh, the velocity field, has one since ADR-087 and refuses here for
    // a different reason — [`SCENARIO`] declares none of the four keys of
    // ADR-069, so the derivation carries no velocity section. Refused at fold
    // time either way, naming what is missing: a silent skip would make "switched
    // on" and "switched off" the same world with different hashes.
    let derived = derived();
    let world = world(&derived);
    for id in ProcessId::ALL {
        let result = Tick::new(&world, &derived, &config(), &roster(&[id]), DT, DX, 42);
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
    let tick = Tick::new(
        &world,
        &derived,
        &config(),
        &roster(&DISPATCHABLE),
        DT,
        DX,
        42,
    )
    .expect("folding");

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
        .lane(0)
        .iter()
        .map(|v| i128::from(v.to_i64()))
        .sum();
    assert!(enthalpy > 0, "the fixture seeds a non-zero enthalpy");

    // And the chemical energy of what the domain holds, which is the other term
    // of the left side since ADR-081: `H_field + Sum_s w_s * n_s`. It is added by
    // the **last** door of `Tick::domain_sums`, after every matter door, because
    // it weighs the accumulators those filled — and nothing but an absolute
    // number like this one can see a door called too early, since the same short
    // sum taken before and after cancels in `after - before`.
    let weights = derived.chemical_weights();
    let chemical: i128 = (0..N_SUBSTANCES)
        .map(|s| i128::from(weights[s as usize]) * sums.matter(s))
        .sum();
    assert!(
        chemical < 0,
        "the fixture's registry declares negative formation enthalpies"
    );
    assert!(
        chemical.abs() > enthalpy.abs() * 10,
        "the chemical term is the larger of the two on any ordinary registry,          and a fixture where it is not cannot tell the sum from the field"
    );
    assert_eq!(sums.energy(), enthalpy + chemical);
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
    let tick = Tick::new(
        &world,
        &derived,
        &config(),
        &roster(&DISPATCHABLE),
        DT,
        DX,
        42,
    )
    .expect("folding");

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
    // **Both owners, and that is the changed half of this test.** Of the eleven
    // buffers this helper used to walk, eight left `World` with ADR-086 —
    // `energy_delta`, `light`, `heat_capacity`, `temperature`, `velocity`, the
    // two potentials and the stirred copy — and three stayed. Eight and not six:
    // nine fields left the world, and the ninth, `solar_in`, was never in the
    // helper at all. A helper that narrowed to the three would compare a quarter
    // of the run and say the same thing about it, which is precisely the
    // argument `world/world.rs` used to keep the step-`b` buffers. It follows all
    // eight: `scratch_snapshot` destructures `ScratchBuffers`, and
    // `every_buffer_the_scratch_owns_is_absent_from_the_snapshot` perturbs each
    // of the twelve alone to prove the helper sees it.
    let (moved, moved_scratch) = run(4, 0, &[ProcessId::Diffusion]);
    let (still, still_scratch) = run(4, 0, &[ProcessId::Advection]);
    let (untouched, untouched_scratch) = run(4, 0, &[]);
    let (start, start_scratch) = fresh();

    // Diffusion on: the run moved. Without this the two equalities below would
    // hold for a tick loop that never dispatched anything.
    assert_ne!(
        snapshot(&moved, &moved_scratch),
        snapshot(&untouched, &untouched_scratch),
        "diffusion changed nothing, so the comparisons below prove nothing"
    );

    // Advection on with the velocity field off: the Courant numbers are zero, so
    // this is a dispatched step that moves nothing — which is a different thing
    // from a skipped step, and has to leave the same bytes in **both** owners.
    assert_eq!(
        snapshot(&still, &still_scratch),
        snapshot(&untouched, &untouched_scratch)
    );

    // And with every process off the *world* is bit for bit the state it was
    // built in, front buffers included. Only the world: a tick with an empty
    // roster still recomputes the denominator and the temperature, twice, because
    // the temperature operator is a step of the tick and not a process of the
    // roster (ADR-079, ADR-086). Comparing the scratch against a world that never
    // ran a tick would be asserting that the operator does nothing, which is the
    // opposite of what this wave decided.
    assert_eq!(world_snapshot(&untouched), world_snapshot(&start));
    assert_eq!(
        scratch_snapshot(&start_scratch).len(),
        scratch_snapshot(&untouched_scratch).len()
    );
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
        Tick::new(&world, &derived, &config(), &roster, DT, DX, 42).expect_err("should refuse")
    );
    assert!(
        message.contains(ProcessId::VelocityField.id()),
        "the refusal has to name the repeated process; it said:\n{message}"
    );
}

#[test]
fn a_roster_enabling_the_velocity_field_without_its_keys_is_refused() {
    // A string assertion on purpose. Nothing else in the crate can see that a
    // `bail!` has gone stale, and `an_enabled_process_without_an_operator_is_refused`
    // only checks that *some* refusal happened and that it names the process.
    //
    // What this used to guard was the refusal of step `b` itself, and that
    // refusal is gone: ADR-087 folds the flux onto the faces of the enthalpy grid
    // and `Derived` carries the four keys of ADR-069, so the arm of
    // `refuse_if_blocked` was deleted rather than rewritten. One refusal is left
    // and it is about the *scenario* and not about the operator — a roster that
    // switches the field on over a scenario that declares no `u_conv_max` — and
    // it has to name the process and the keys, because the reader's next move is
    // to go and write them.
    let derived = derived();
    let world = world(&derived);
    let message = format!(
        "{:#}",
        Tick::new(
            &world,
            &derived,
            &config(),
            &roster(&[ProcessId::VelocityField]),
            DT,
            DX,
            42,
        )
        .expect_err("this scenario declares no velocity keys")
    );

    // The dead blockers, in the shape they used to take. A refusal that names a
    // lock somebody has since lifted is worse than no refusal, because the reader
    // stops at the first sentence: `qdiv` on a non-positive `C_cell` went with
    // ADR-079, and the fold onto the coarse faces went with ADR-087.
    for stale in ["qdiv", "TODO(courant-fold)"] {
        assert!(
            !message.contains(stale),
            "the refusal still names `{stale}`, which is lifted; it said:\n{message}"
        );
    }
    for wanted in [ProcessId::VelocityField.id(), "u_conv_max", "ADR-069"] {
        assert!(
            message.contains(wanted),
            "the refusal has to name `{wanted}`; it said:\n{message}"
        );
    }
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
        Tick::new(&world, &derived, &config(), &roster, DT, DX, 42).expect_err("should refuse")
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
        Tick::new(
            &world,
            &derived,
            &config(),
            &roster(&DISPATCHABLE),
            DT * 2.0,
            DX,
            42,
        )
        .expect_err("should refuse")
    );
    assert!(
        message.contains("enthalpy"),
        "the refusal has to name the field; it said:\n{message}"
    );
}

// --- step `b` dispatches (ADR-069, ADR-087) ---------------------------------

/// The fixture with a velocity field declared: the four keys of ADR-069 on the
/// process record ADR-065 materialises anyway.
///
/// A second string and not an edit to [`SCENARIO`], so that every test above goes
/// on running against a scenario with no `[[process]]` record at all — which is
/// the shape `configs/scenarios/hello.toml` has and the shape the roster defaults
/// are read under.
///
/// The numbers: `u_conv_max = 1e-5 m/s` is 60% of the ceiling
/// `dx/(6*dt) = 1.67e-5` the validator holds a `SpeedBound` at, and
/// `l_c = 8e-4 m` is `r = round(l_c/(2*dx_coarse)) = 1` cell of the enthalpy grid
/// at `lod = 2`. Both are properties of this fixture and neither is a
/// recommendation: `CONFIG_SCHEMA.md` section 13 leaves the value of `l_c` to the
/// scenario, and no file under `configs/` enables the field at all.
const SCENARIO_WITH_VELOCITY: &str = r#"
[[process]]
id = "velocity_field"
enabled = true
u_conv_max = 1.0e-5
l_c = 8.0e-4
stir_fraction = 0.0
"#;

/// The edge the velocity tests build their world at.
///
/// Sixteen and not [`N`] or [`N_HEAT`]: `VelocityField::new` refuses
/// `2*r >= min(cnx, cny)` — at half the extent the two arms of the wide
/// difference wrap onto one cell and the difference is identically zero — so at
/// `r = 1` the enthalpy grid has to be at least three cells wide, which at
/// `lod = 2` is a fine grid of sixteen.
const N_FLOW: u32 = 16;

fn config_with_velocity() -> config::Config {
    let mut text = SCENARIO.to_string();
    text.push_str(SCENARIO_WITH_VELOCITY);
    config::parse(&text).expect("the velocity fixture must parse")
}

fn derived_with_velocity() -> config::Derived {
    config::validate(&config_with_velocity()).expect("the velocity fixture must validate")
}

/// A world at [`N_FLOW`], seeded so that the enthalpy field carries a real
/// temperature anomaly rather than a handful of storage units.
///
/// The amplitude is measured off the world rather than declared, and that is the
/// whole of this function. `H = C_cell * dT * 2^k_E`, where `C_cell` is the
/// denominator the tick will actually divide by — the operator's answer over the
/// composition this fixture seeded, not `DerivedEnergy::c_cell`, which is the
/// declared `c_v` at `typical_conc` and is a different number on any world whose
/// amounts are not the typical ones. Taken from the declaration instead, the
/// anomaly comes out far past the declared span, `within_speed_bound` fires, and
/// the test reports a defect of the fixture as a defect of the derivation of `L`.
///
/// Half the declared span and not the whole of it: the mobility is calibrated so
/// that `|u_conv| <= u_conv_max` holds exactly at `t_max - t_min` (ADR-069), so
/// half of it leaves the field comfortably inside the bound the validator checked
/// while still driving a flow.
fn flowing_world(derived: &config::Derived) -> World {
    let mut world = world_sized(derived, N_FLOW, Heat::Varying);
    let config = config_with_velocity();
    let operator = Temperature::new(
        world.grid(),
        world.enthalpy_grid(),
        ENTHALPY_LOD,
        world.registry(),
        derived,
        &config,
    )
    .expect("folding the temperature operator");

    let cells = world.enthalpy_grid().n_voxels() as usize;
    let mut heat_capacity = vec![Q::ZERO; cells];
    let mut temperature = vec![Q::ZERO; cells];
    {
        let (src32, src64) = world.amount_slices();
        operator.apply(
            src32,
            src64,
            world.enthalpy().lane(0),
            &mut heat_capacity,
            &mut temperature,
        );
    }

    let energy = derived.energy();
    let units_per_joule = 2f64.powi(i32::from(energy.k_e));
    let half_span = (energy.t_max - energy.t_min) / 2.0;
    let cn = world.enthalpy_grid().nx();
    {
        let field = world.enthalpy_mut();
        let buffer = field.write_mut();
        for cell in 0..cells {
            let idx = cell as u32;
            let (x, y, z) = (idx % cn, (idx / cn) % cn, idx / (cn * cn));
            // Localised horizontally and decaying with height: a horizontally
            // uniform field drives nothing at all, which is what
            // `horizontally_uniform_temperature_produces_no_velocity` asserts
            // next door.
            let bump = |c: u32| {
                0.5 * (1.0
                    - (2.0 * std::f64::consts::PI * (f64::from(c) + 0.5) / f64::from(cn)).cos())
            };
            let share = bump(x) * bump(y) * f64::from(cn - z) / f64::from(cn);
            let joules_per_kelvin = heat_capacity[cell].debug_f64();
            buffer[cell] =
                M64::new((share * half_span * joules_per_kelvin * units_per_joule) as i64);
        }
        field.swap();
    }
    world
}

#[test]
fn the_velocity_field_dispatches_and_fills_both_courant_buffers() {
    // The witness that step `b` runs at all. Under version 21 a roster with
    // `velocity_field` enabled was refused by `Tick::new`, so two scenarios
    // differing by this flag produced a bit-identical world — which is why
    // `a_disabled_process_leaves_every_buffer_bit_for_bit` passed and said
    // nothing.
    //
    // **Both** buffers, and that is the half ADR-087 adds. The fine one has been
    // written by the last stage of `VelocityField::apply` since ADR-069; the
    // coarse one stayed at `Q::ZERO` while nothing folded a flux onto the faces of
    // the enthalpy grid, and step `c` over the enthalpy was therefore an
    // application of a zero flux with both residuals closing exactly.
    let derived = derived_with_velocity();
    assert!(derived.velocity().is_some());

    let mut world = flowing_world(&derived);
    let tick = Tick::new(
        &world,
        &derived,
        &config_with_velocity(),
        &roster(&[ProcessId::VelocityField, ProcessId::Advection]),
        DT,
        DX,
        42,
    )
    .expect("a roster that enables the velocity field folds");
    let mut ledger = Ledger::new(N_SUBSTANCES).expect("the ledger");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch buffers");
    tick.advance(&mut world, &mut ledger, &mut scratch, 0, 7);

    let ScratchBuffers {
        face_courant,
        enthalpy_courant,
        ..
    } = scratch.buffers();
    assert!(
        face_courant.iter().any(|v| *v != Q::ZERO),
        "step `b` left the fine Courant buffer at zero"
    );
    assert!(
        enthalpy_courant.iter().any(|v| *v != Q::ZERO),
        "step `b` left the enthalpy Courant buffer at zero: the fold of ADR-087 \
         did not run, and step `c` carried the amounts with a field the enthalpy \
         never saw"
    );
}

#[test]
fn an_enabled_velocity_field_with_every_n_ticks_above_one_is_refused_at_fold() {
    // `Tick::new` passes the roster's `every_n_ticks` into `VelocityConfig`, so
    // the refusal `VelocityField::new` already carries fires. Hard-coded to one it
    // becomes dead code, a roster with `every_n_ticks = 5` folds, and step `c`
    // moves matter on four ticks out of five over a field step `b` did not update
    // — the temporal form of transport without a velocity (ADR-074).
    let derived = derived_with_velocity();
    let world = flowing_world(&derived);

    let mut roster = roster(&[ProcessId::VelocityField]);
    for entry in &mut roster {
        if entry.id == ProcessId::VelocityField {
            entry.every_n_ticks = 5;
        }
    }
    let message = Tick::new(
        &world,
        &derived,
        &config_with_velocity(),
        &roster,
        DT,
        DX,
        42,
    )
    .expect_err("a velocity field on a schedule has to be refused")
    .to_string();
    assert!(message.contains("every_n_ticks"), "{message}");
    assert!(
        message.contains('5'),
        "the number has to be named: {message}"
    );
}

#[test]
fn the_four_velocity_keys_reach_the_fold_unchanged() {
    // `Derived` carries the four keys of ADR-069 byte for byte and derives
    // nothing from them: the mobility `L`, the radius `r`, the octave count and
    // the estimate of `|u|` come out of the three grids inside
    // `VelocityField::new`, and a second derivation here would be a second source
    // for numbers that would disagree wherever the world's enthalpy grid and the
    // `[[field]]` record's `lod` disagree.
    let with_velocity = derived_with_velocity();
    let declared = with_velocity
        .velocity()
        .expect("the fixture enables the velocity field");
    assert_eq!(declared.u_conv_max, 1.0e-5);
    assert_eq!(declared.l_c, 8.0e-4);
    assert_eq!(declared.stir_fraction, 0.0);
    assert_eq!(declared.stir_period, None);

    // And absent on the scenario that says nothing about the process, which is
    // every file under `configs/`: absence of a record means the process's own
    // default, and that default is `false` (ADR-065, ADR-069).
    assert!(derived().velocity().is_none());

    // The temperature range reaches a derivation too, and it had nowhere to live
    // before ADR-087: `energy_window` read `t_min` and `t_max` off the enthalpy
    // `[[field]]` record to derive `H_max` and then dropped them, while the
    // mobility is calibrated so that `|u_conv| <= u_conv_max` holds exactly at
    // that span.
    let energy = with_velocity.energy();
    assert_eq!(energy.t_min, 273.15);
    assert_eq!(energy.t_max, 323.15);
}
