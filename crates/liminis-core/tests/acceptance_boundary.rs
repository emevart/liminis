//! Acceptance criteria of the exchanging face (`ACCEPTANCE.md`, S0, ADR-059).
//!
//! ```text
//! boundary_outflow_appears_in_channel_counter
//! exchange_face_carries_enthalpy_into_the_energy_counter
//! the_ghost_cell_is_the_same_value_in_both_buffers
//! a_domain_reduction_skips_the_ghost_element
//! ```
//!
//! The third name of `exchange_inflow_falls_back_to_first_order` is not here: it
//! is a statement about the van Leer limiter and lives beside the rest of them in
//! `tests/acceptance_advection.rs`.
//!
//! An integration test rather than a unit one, for the reason
//! `tests/acceptance_diffusion.rs` gives: the outside view may use only what a
//! scenario can use — the public surface of `liminis-core` — so it keeps its
//! meaning when the inside is rearranged, and cannot be quietly shaped around a
//! private helper it was supposed to be judging.
//!
//! # What makes these tests able to fail
//!
//! Every one of them stands on a choice that no other test in the project can
//! see, because the failures are silent by construction:
//!
//! - **the sign of the counter.** On a closed domain both conventions give zero
//!   (`TODO(counter-sign)` in `ledger/mod.rs`), so this file is the only place a
//!   backwards counter can show up at all — and it shows up as a residual of
//!   twice the flow rather than as a small error;
//! - **the substance the flow is credited to.** The table is indexed by lane, and
//!   a lane is not a substance index (ADR-056). Credited to the wrong one, two
//!   substances fail to close at once and nothing points at the mapping, so the
//!   fixture below puts the venting substance at an index that is **not** its
//!   lane;
//! - **once a tick against once a substep.** ADR-059 credits `BOUNDARY_EXCHANGE`
//!   on every substep of steps `c` and `d`. Crediting once is wrong by exactly
//!   the factor `n`, and `n` is **one** for most of the registry — so the
//!   substance here is the proton, which takes six (SPEC section 1.7). On H2S,
//!   CH4, PO4, Fe or CO2 the mistake is invisible;
//! - **which buffer the ghost is in.** A run of substeps alternates direction
//!   (ADR-057), so a ghost seeded into `front` alone is a zero on every odd
//!   substep. Both residuals still close exactly, because the counter records
//!   what actually left;
//! - **whether a reduction over the domain includes the reservoir.** It cancels
//!   in `after - before`, so no residual can see it and only the absolute sums
//!   lie.

use liminis_core::config;
use liminis_core::ledger::{Channel, DomainSums, Ledger};
use liminis_core::numeric::{M32, M64};
use liminis_core::process::{
    DiffusePhase, N_MAX, ProcessId, ROSTER_LEN, RosterEntry, Scratch, Tick, default_roster,
};
use liminis_core::world::{
    Boundary, Face, Field, Field32, Grid, LaneRef, Registry, World, WorldLayout,
};

/// The eco regime of SPEC section 1.7: a one-second tick and a 100 um voxel.
const DT: f64 = 1.0;
const DX: f64 = 1.0e-4;

/// The exchange velocity of the reservoir, m/s.
///
/// The order ADR-059 names: the piston velocity of oxygen at the surface of the
/// sea is about `1e-5 m/s`, against a limit of `1e-4` at this `dx` and `dt`. Ten
/// times of headroom, and the record assigns no working value, so this is the
/// order and not a decision.
const K_EX: f64 = 1.0e-5;

/// The diffusivity of the proton, m^2/s: the Grotthuss mechanism, and the only
/// substance of the registry that takes six substeps (SPEC section 1.7).
const D_PROTON: f64 = 9.3e-9;

/// Periodic in X and Y, a solid floor, and a lid that vents. The eco-regime
/// default of SPEC section 1.6 in full (`CONFIG_SCHEMA.md` section 4).
fn vented(nx: u32, ny: u32, nz: u32) -> Grid {
    Grid::new(
        nx,
        ny,
        nz,
        [
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Closed,
            Boundary::Exchange,
        ],
    )
    .expect("a vented grid")
}

/// The same grid with the lid sealed, for the assertions that need a reference
/// world where nothing crosses.
fn sealed(nx: u32, ny: u32, nz: u32) -> Grid {
    Grid::new(
        nx,
        ny,
        nz,
        [
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Closed,
            Boundary::Closed,
        ],
    )
    .expect("a sealed grid")
}

/// How many substances the fixtures below account for.
const N_SUBSTANCES: u32 = 5;

/// The substance the venting lane stands for.
///
/// **Not zero, and not the lane index.** The phases take a table indexed by lane
/// and the fixture has one lane, so a credit posted against "the lane" instead of
/// against the table would land on substance 0 — and every assertion about
/// substance 0 would then be green over the mistake.
const VENTING: u32 = 3;

/// Seed one lane's voxels and promote them into the front buffer.
fn seed_32(field: &mut Field32, amount: impl Fn(u32) -> i32) {
    let n_voxels = field.n_voxels();
    let (_, dst) = field.lane_pair_mut(0);
    for (idx, cell) in dst.iter_mut().take(n_voxels as usize).enumerate() {
        *cell = M32::new(amount(idx as u32));
    }
    field.swap();
}

fn domain_sum(field: &Field32) -> i128 {
    field.lane(0).iter().map(|v| i128::from(v.to_i64())).sum()
}

// ---------------------------------------------------------------------------
// boundary_outflow_appears_in_channel_counter
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md`, from ADR-059.
///
/// Matter leaves through the lid, the domain sum falls by exactly what the
/// counter says, and the residual is an exact zero rather than a small number.
///
/// The proton and not a convenient substance: it takes six substeps, and a
/// counter credited once a tick instead of once a substep is wrong by exactly
/// that factor. On the five substances of the registry that take one substep the
/// two implementations are indistinguishable.
#[test]
fn boundary_outflow_appears_in_channel_counter() {
    let grid = vented(4, 4, 5);
    let phase = DiffusePhase::new_32(&grid, 1, &[D_PROTON], DT, DX, K_EX).unwrap();
    assert_eq!(
        phase.substeps_of(0),
        6,
        "the fixture must take six substeps"
    );
    assert!(phase.substeps_of(0) > 1, "one substep hides the factor n");
    assert!(phase.substeps_of(0) <= N_MAX);

    let mut field: Field32 = Field::new(&grid, 1).unwrap();
    seed_32(&mut field, |_| 1_000_000);
    // The reservoir holds less than the domain, so the lid is a sink.
    field.set_ghost(0, M32::ZERO);

    let mut before = DomainSums::new(N_SUBSTANCES).unwrap();
    before.add_field_lane_32(VENTING, field.lane(0));
    let started_with = domain_sum(&field);

    let mut ledger = Ledger::new(N_SUBSTANCES).unwrap();
    ledger.begin_tick();
    phase.apply_32(&mut field, &[VENTING], &mut ledger);

    let mut after = DomainSums::new(N_SUBSTANCES).unwrap();
    after.add_field_lane_32(VENTING, field.lane(0));

    // (1) The domain lost something, and it is the lid that took it: nothing
    // else in this arrangement can move matter out of a closed floor and a
    // torus.
    let lost = started_with - domain_sum(&field);
    assert!(lost > 0, "nothing left through the lid at all");

    // (2) The counter holds exactly the negative of it. Positive when matter
    // *entered* the domain, which is the convention `ledger/mod.rs` picks
    // between the two ADR-059 states, and this is the only place in the project
    // where picking the other one can fail.
    let counted = i128::from(ledger.matter(Channel::BoundaryExchange, VENTING));
    assert_eq!(
        counted, -lost,
        "the counter is not the flow through the lid"
    );

    // (3) The residual is an exact zero. Under the opposite sign convention it
    // would be `2*lost` rather than a small error.
    assert_eq!(ledger.residual_matter(VENTING, &before, &after), 0);
    ledger.assert_closed(&before, &after);

    // (4) The other five channels were not touched, for any substance: what
    // crosses this face belongs to BOUNDARY_EXCHANGE and to nothing else
    // (SPEC section 7).
    for channel in Channel::ALL {
        if channel == Channel::BoundaryExchange {
            continue;
        }
        assert_eq!(ledger.energy(channel), 0, "{} moved energy", channel.name());
        for s in 0..N_SUBSTANCES {
            assert_eq!(
                ledger.matter(channel, s),
                0,
                "{} moved substance {s}",
                channel.name()
            );
        }
    }

    // (5) And no other substance's counter moved. The table is indexed by lane
    // and this fixture has one lane at index 0, so a credit posted against the
    // lane would have landed on substance 0.
    for s in 0..N_SUBSTANCES {
        if s == VENTING {
            continue;
        }
        assert_eq!(
            ledger.matter(Channel::BoundaryExchange, s),
            0,
            "substance {s} was credited with somebody else's flow"
        );
    }

    // The same field under a sealed lid keeps everything, so the loss above is
    // the face and not the scheme.
    let sealed_grid = sealed(4, 4, 5);
    let sealed_phase = DiffusePhase::new_32(&sealed_grid, 1, &[D_PROTON], DT, DX, K_EX).unwrap();
    let mut sealed_field: Field32 = Field::new(&sealed_grid, 1).unwrap();
    seed_32(&mut sealed_field, |_| 1_000_000);
    let mut quiet = Ledger::new(N_SUBSTANCES).unwrap();
    sealed_phase.apply_32(&mut sealed_field, &[VENTING], &mut quiet);
    assert_eq!(domain_sum(&sealed_field), started_with);
    assert_eq!(quiet.matter(Channel::BoundaryExchange, VENTING), 0);
}

/// The mechanism of the test above, taken apart: the credit happens on **every**
/// substep, so a run of `n` credits `n` times.
///
/// Stated as a comparison between two substep counts rather than as a number,
/// because the number depends on the flux and the flux depends on the state after
/// each substep. What cannot depend on anything is that the six-substep run
/// credits six times and the one-substep run once — and an implementation that
/// credited once a tick would make both counts one.
#[test]
fn the_channel_is_credited_on_every_substep_and_not_once_a_tick() {
    let grid = vented(3, 3, 4);

    let count_credits = |diffusivity: f64| -> u32 {
        let phase = DiffusePhase::new_32(&grid, 1, &[diffusivity], DT, DX, K_EX).unwrap();
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_32(&mut field, |_| 1_000_000);
        field.set_ghost(0, M32::ZERO);

        let mut ledger = Ledger::new(N_SUBSTANCES).unwrap();
        // One credit per substep, and each of them is a whole plane's worth, so
        // counting them means counting the substeps the phase ran. The proxy is
        // the ratio of the two totals, which is what the assertion below uses.
        phase.apply_32(&mut field, &[VENTING], &mut ledger);
        assert!(ledger.matter(Channel::BoundaryExchange, VENTING) < 0);
        phase.substeps_of(0)
    };

    // The proton against phosphate: six substeps against one, out of the same
    // formula and not out of a table (ADR-030).
    assert_eq!(count_credits(D_PROTON), 6);
    assert_eq!(count_credits(0.8e-9), 1);
}

// ---------------------------------------------------------------------------
// the_ghost_cell_is_the_same_value_in_both_buffers
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md`, from ADR-059 and ADR-057.
///
/// A run of substeps alternates direction rather than swapping, so substep 0
/// reads `front` and substep 1 reads `back`. A ghost seeded into one buffer holds
/// the reservoir on even substeps and a zero on odd ones — the lid becomes an
/// infinite sink every other substep — and **nothing else in the project can see
/// it**: zero is a legal amount, the flux stays antisymmetric, the counter
/// records exactly what left, and both residuals close.
#[test]
fn the_ghost_cell_is_the_same_value_in_both_buffers() {
    let grid = vented(3, 3, 4);
    const RESERVOIR: i32 = 777_000;

    let mut field: Field32 = Field::new(&grid, 1).unwrap();
    seed_32(&mut field, |_| 1_000_000);
    field.set_ghost(0, M32::new(RESERVOIR));
    assert_eq!(field.ghost(0), M32::new(RESERVOIR));

    // An **odd** number of substeps, so that the lane's state ends in the back
    // buffer and the restoration copies it forward. Six would put it back in the
    // front by parity and hide a ghost that only the front buffer holds.
    let phase = DiffusePhase::new_32(&grid, 1, &[0.8e-9], DT, DX, K_EX).unwrap();
    assert_eq!(phase.substeps_of(0), 1, "an odd substep count is the case");

    let mut ledger = Ledger::new(N_SUBSTANCES).unwrap();
    phase.apply_32(&mut field, &[VENTING], &mut ledger);

    // After the substeps and after `restore_lane`, which copies the whole lane
    // back to front and would carry an unseeded `back` over a seeded `front`.
    assert_eq!(
        field.ghost(0),
        M32::new(RESERVOIR),
        "the ghost cell did not survive the phase"
    );

    // And the flux went the way the reservoir says: the domain holds more than
    // the outside, so matter left.
    assert!(ledger.matter(Channel::BoundaryExchange, VENTING) < 0);

    // The other half of the statement, which is the one that fails on a
    // single-buffer seeding: three substeps of alternating direction end on an
    // odd parity, and the run reads the ghost from `back` on the second of them.
    let proton = DiffusePhase::new_32(&grid, 1, &[D_PROTON], DT, DX, K_EX).unwrap();
    let mut alternating: Field32 = Field::new(&grid, 1).unwrap();
    seed_32(&mut alternating, |_| 1_000_000);
    alternating.set_ghost(0, M32::new(RESERVOIR));
    let mut ledger = Ledger::new(N_SUBSTANCES).unwrap();
    proton.apply_32(&mut alternating, &[VENTING], &mut ledger);
    assert_eq!(alternating.ghost(0), M32::new(RESERVOIR));

    // The consequence, and the reason the assertion above is worth making: a lid
    // that read a zero on every other substep would be a far bigger sink than a
    // reservoir at 777 000 units against a domain at a million. Six substeps
    // against a reservoir this close cannot take more than the whole gap.
    let taken = -ledger.matter(Channel::BoundaryExchange, VENTING);
    let gap = i64::from(1_000_000 - RESERVOIR) * i64::from(grid.nx() * grid.ny());
    assert!(taken > 0, "nothing crossed a lid with a gradient across it");
    assert!(
        taken < gap,
        "the lid took {taken} units against a gap of {gap}: it is venting into \
         something emptier than the declared reservoir"
    );
}

// ---------------------------------------------------------------------------
// a_domain_reduction_skips_the_ghost_element
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md`, from ADR-059 and ADR-068.
///
/// A reduction over the domain must not count the reservoir. **No residual can
/// see this**: the ghost is constant, so it cancels in `after - before` and
/// `ledger_residual_is_zero_over_1e6_ticks` stays green; what lies is the
/// absolute sum, and with it the volume export of `serve.rs` and the floor/peak
/// band of worldgen.
#[test]
fn a_domain_reduction_skips_the_ghost_element() {
    let grid = vented(3, 4, 5);

    let sums_of = |ghost: i32| -> i128 {
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_32(&mut field, |idx| 1_000 + idx as i32);
        field.set_ghost(0, M32::new(ghost));

        let mut sums = DomainSums::new(N_SUBSTANCES).unwrap();
        sums.add_field_lane_32(VENTING, field.lane(0));
        sums.matter(VENTING)
    };

    // A reservoir five orders above anything in the domain: if it were counted,
    // the sum would be dominated by it.
    let quiet = sums_of(0);
    let loud = sums_of(1_000_000_000);
    assert_eq!(quiet, loud, "the reduction counted the reservoir");
    assert!(quiet > 0, "the fixture summed nothing at all");

    // The lane view is the voxels and the buffer is the lane, and the difference
    // is exactly one element (ADR-059).
    let mut field: Field32 = Field::new(&grid, 1).unwrap();
    assert_eq!(field.lane(0).len(), grid.n_voxels() as usize);
    assert_eq!(field.lane_len(), grid.n_voxels() + 1);
    assert_eq!(field.read().len(), field.lane_len() as usize);
    field.set_ghost(0, M32::new(42));
    assert_eq!(field.lane(0).len(), grid.n_voxels() as usize);
    assert!(!field.lane(0).contains(&M32::new(42)));

    // The same statement for the enthalpy field, which the tick reduces through
    // a different door (`DomainSums::add_enthalpy_lane_64`).
    let derived = derived();
    let mut world = vented_world(&derived);
    let reservoir = derived.reservoir().expect("the fixture declares one");

    let mut before = DomainSums::new(derived.substances().len() as u32).unwrap();
    before.add_enthalpy_lane_64(world.enthalpy().lane(0));
    world
        .seed_ghosts(&reservoir.amount_out, M64::new(reservoir.enthalpy_out))
        .unwrap();
    let mut after = DomainSums::new(derived.substances().len() as u32).unwrap();
    after.add_enthalpy_lane_64(world.enthalpy().lane(0));

    assert_ne!(reservoir.enthalpy_out, 0, "the fixture seeds a real ghost");
    assert_eq!(
        before.energy(),
        after.energy(),
        "seeding the reservoir moved the energy of the domain"
    );
}

// ---------------------------------------------------------------------------
// exchange_face_carries_enthalpy_into_the_energy_counter
// ---------------------------------------------------------------------------

/// A scenario whose `T_ref` is **not** the thermochemical 298.15 K.
///
/// That is the whole reason it is written out here instead of being borrowed.
/// ADR-044 keeps the two reference states apart because calling them one word
/// made both questions unanswerable, and `configs/scenarios/h2s-oxidation.toml`
/// sets `T_ref = 298.15` "out of convenience and not by requirement" — so on
/// every scenario in the corpus the two are the same number and a ghost folded
/// through the wrong one is invisible. Here they are five kelvin apart.
///
/// **The numbers marked as placeholders are placeholders.** `c_p`,
/// `enthalpy_formation` and `partial_molar_volume` are declared for no substance
/// anywhere in the corpus (`CONFIG_SCHEMA.md` section 13 item 23). Do not copy
/// this into `configs/`.
///
/// `water_formation` exists for one reason and it is not chemistry: it is the
/// reaction whose extent exponent raises `WATER` past its own overflow ceiling,
/// which is what makes water the 64-bit substance (ADR-039, ADR-040). Without a
/// wide substance the fixture could not tell the two width classes apart, and
/// `World::seed_ghosts` has a branch for each.
const SCENARIO: &str = r#"
name = "boundary-fixture"
dt = 1.0
beta = 0.015625
T_ref = 300.0

[conserved]
C = 12.01070
N = 14.00670
P = 30.97376
S = 32.06500
Fe = 55.84500

[grid]
nx = 8
ny = 8
nz = 8
dx = 1.0e-4

[boundary]
x_min = "periodic"
x_max = "periodic"
y_min = "periodic"
y_max = "periodic"
z_min = "closed"
z_max = "exchange"

[boundary.reservoir]
t_out = 310.0
k_ex = 1.0e-5
conc_out = { H2S = 0.0, WATER = 55000.0, O2 = 0.25, SO4 = 28.0, H_ION = 1.0e-4 }

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

/// The scenario's `T_ref`, and it is not 298.15 (ADR-044).
const T_REF: f64 = 300.0;
/// The reservoir's temperature.
const T_OUT: f64 = 310.0;
/// The coarsening of the enthalpy field, as the scenario declares it.
const ENTHALPY_LOD: u32 = 2;

fn derived() -> config::Derived {
    let config = config::parse(SCENARIO).expect("the fixture must parse");
    config::validate(&config).expect("the fixture must validate")
}

/// A world on the fixture's registry, with a lid that vents.
fn vented_world(derived: &config::Derived) -> World {
    let registry = Registry::new(&derived.decls()).expect("the registry");
    World::new(
        vented(8, 8, 8),
        registry,
        &WorldLayout {
            enthalpy_lod: ENTHALPY_LOD,
            velocity_lod: 1,
        },
    )
    .expect("the world")
}

/// `sum(conc_out_i * c_p_i) * V_cell`, J/K, recomputed here from the scenario's
/// own numbers and independently of the loader.
///
/// `V_cell` of the **enthalpy** grid, `(dx*2^lod)^3`, and the whole point of
/// recomputing it is that number: at `lod = 2` a `V_voxel` in its place is a
/// factor of sixty-four, `t_out` stays inside its declared range, the load
/// passes, the sign of the flux is right and the energy residual closes — the
/// counter records whatever moved. Only the magnitude lies, so the magnitude has
/// to be asserted.
fn heat_capacity_of_the_reservoir() -> f64 {
    let coarse_dx = DX * f64::from(1u32 << ENTHALPY_LOD);
    let v_cell = coarse_dx * coarse_dx * coarse_dx;
    // (conc_out, c_p) of the five substances, in declaration order.
    let composition = [
        (0.0, 100.0),
        (55_000.0, 75.3),
        (0.25, 101.0),
        (28.0, 102.0),
        (1.0e-4, 103.0),
    ];
    composition
        .iter()
        .map(|(conc, c_p)| conc * c_p * v_cell)
        .sum()
}

/// `ACCEPTANCE.md`, from ADR-059.
///
/// Enthalpy is a diffusive field of its own (ADR-028, ADR-030) and goes through
/// the lid on its own account, so it needs a ghost cell of its own and it lands
/// in the **energy** half of the ledger. Without one the top of the world is
/// thermally sealed by a default, which is the outcome `Grid::new` used to refuse
/// the face in order to prevent.
#[test]
fn exchange_face_carries_enthalpy_into_the_energy_counter() {
    let derived = derived();
    let reservoir = derived.reservoir().expect("the fixture declares one");
    let field_record = derived.enthalpy_field();
    let k_e = derived.energy().k_e;
    let n_substances = derived.substances().len() as u32;

    // --- the ghost, before anything moves --------------------------------

    // Recomputed here out of the scenario, not read back out of the loader.
    let c_cell = heat_capacity_of_the_reservoir();
    let expected = ((T_OUT - T_REF) * c_cell * f64::from(1u64.wrapping_shl(0) as u32).max(1.0))
        .mul_add(0.0, 0.0) // keep the expression readable below
        + ((T_OUT - T_REF) * c_cell * 2f64.powi(i32::from(k_e))).round();
    assert_eq!(
        reservoir.enthalpy_out, expected as i64,
        "the reservoir's enthalpy is not (t_out - T_ref) * C_cell at the energy \
         scale"
    );

    // Two ways of getting it wrong, and both load: the thermochemical reference
    // in place of the scenario's `T_ref` (ADR-044), and `V_voxel` in place of the
    // coarse `V_cell` (ADR-062).
    let through_298 = ((T_OUT - 298.15) * c_cell * 2f64.powi(i32::from(k_e))).round() as i64;
    assert_ne!(
        reservoir.enthalpy_out, through_298,
        "the fixture cannot tell T_ref from the thermochemical reference"
    );
    let at_the_fine_voxel = reservoir.enthalpy_out / 64;
    assert_ne!(reservoir.enthalpy_out, at_the_fine_voxel);

    // --- the flux, in both directions ------------------------------------

    // The domain holds the composition of the reservoir at a temperature of its
    // own, so that the ordering of the two enthalpies is the ordering of the two
    // temperatures. The field transported is `H` and not `T` (ADR-062), and on
    // two different compositions the two orderings would come apart.
    let enthalpy_at = |t_domain: f64| -> i64 {
        ((t_domain - T_REF) * c_cell * 2f64.powi(i32::from(k_e))).round() as i64
    };

    for (t_domain, entering) in [(305.0, true), (315.0, false)] {
        let mut world = vented_world(&derived);
        world
            .seed_ghosts(&reservoir.amount_out, M64::new(reservoir.enthalpy_out))
            .unwrap();
        assert_eq!(world.enthalpy_ghost(), M64::new(reservoir.enthalpy_out));

        {
            let cells = world.enthalpy_grid().n_voxels();
            let field = world.enthalpy_mut();
            let (_, dst) = field.lane_pair_mut(0);
            for cell in dst.iter_mut().take(cells as usize) {
                *cell = M64::new(enthalpy_at(t_domain));
            }
            field.swap();
        }

        let mut before = DomainSums::new(n_substances).unwrap();
        before.add_enthalpy_lane_64(world.enthalpy().lane(0));

        let phase = DiffusePhase::new_64(
            world.enthalpy_grid(),
            1,
            &[field_record.diffusivity],
            DT,
            field_record.coarse_dx,
            reservoir.k_ex,
        )
        .unwrap();

        let mut ledger = Ledger::new(n_substances).unwrap();
        ledger.begin_tick();
        phase.apply_enthalpy(world.enthalpy_mut(), &mut ledger);

        let mut after = DomainSums::new(n_substances).unwrap();
        after.add_enthalpy_lane_64(world.enthalpy().lane(0));

        let credited = i128::from(ledger.energy(Channel::BoundaryExchange));
        let moved = after.energy() - before.energy();

        if entering {
            assert!(
                credited > 0,
                "the reservoir is warmer than the domain and no energy entered"
            );
        } else {
            assert!(
                credited < 0,
                "the domain is warmer than the reservoir and no energy left"
            );
        }
        assert_eq!(credited, moved, "the counter is not what the domain gained");
        assert_eq!(ledger.residual_energy(&before, &after), 0);

        // Enthalpy goes through the face on its own account and drags no matter
        // with it (ADR-067): not one substance counter moved, on any channel.
        for channel in Channel::ALL {
            for s in 0..n_substances {
                assert_eq!(
                    ledger.matter(channel, s),
                    0,
                    "{} moved substance {s} while the heat crossed the lid",
                    channel.name()
                );
            }
        }
    }
}

/// The coarse grids inherit the exchanging face, and the enthalpy field is on one
/// of them.
///
/// `coarse_grid` copies the six faces unchanged, so this cannot fail today. It is
/// asserted because the failure is the quiet one: a coarse grid that had lost the
/// face would be thermally sealed while the amounts went on venting, and the
/// energy residual would close over it exactly, a sealed field being conserved no
/// worse than a venting one.
#[test]
fn the_coarse_grids_inherit_the_exchanging_face() {
    let derived = derived();
    let world = vented_world(&derived);

    for grid in [world.grid(), world.enthalpy_grid(), world.velocity_grid()] {
        assert!(grid.has_exchange());
        assert_eq!(grid.exchange_mask(), 1 << (Face::ZPlus as u32));
        assert_eq!(grid.boundary(Face::ZPlus), Boundary::Exchange);
        assert_eq!(grid.boundary(Face::ZMinus), Boundary::Closed);
    }

    // And the three grids really are three different shapes, or the assertion
    // above is about one grid written three times.
    assert_ne!(world.grid().n_voxels(), world.enthalpy_grid().n_voxels());
    assert_ne!(
        world.velocity_grid().n_voxels(),
        world.enthalpy_grid().n_voxels()
    );
}

/// Every lane of every field gets its ghost, and the narrowing from a substance
/// to a lane goes through the one door (ADR-056).
#[test]
fn seeding_the_reservoir_fills_every_lane_of_every_field() {
    let derived = derived();
    let reservoir = derived.reservoir().expect("the fixture declares one");
    let mut world = vented_world(&derived);

    world
        .seed_ghosts(&reservoir.amount_out, M64::new(reservoir.enthalpy_out))
        .unwrap();

    let mut wide_seen = false;
    for s in 0..derived.substances().len() as u32 {
        assert_eq!(
            i128::from(world.ghost_of(s)),
            reservoir.amount_out[s as usize],
            "substance {s} did not get its reservoir amount"
        );
        if matches!(world.lane_of(s), LaneRef::Wide(_)) {
            wide_seen = true;
        }
    }
    assert!(
        wide_seen,
        "every substance came out narrow, so the fixture cannot tell the two \
         width classes apart"
    );

    // The reservoir of the corpus is mostly water, so at least one amount is
    // large and at least one is zero — a table of zeroes would pass the loop
    // above and prove nothing.
    assert!(reservoir.amount_out.iter().any(|&a| a > 0));
    assert!(reservoir.amount_out.contains(&0));

    // A table of the wrong length is refused rather than applied to a prefix.
    let short = reservoir.amount_out[..1].to_vec();
    assert!(world.seed_ghosts(&short, M64::ZERO).is_err());
}

/// The whole tick over a venting world, and the residual closing on every one of
/// them.
///
/// The unit tests above take one phase apart; this takes the orchestration. Two
/// things live only here and both are silent: the substance table `Tick` builds
/// per width class — a lane is not a substance index (ADR-056), and a table built
/// the other way round would post the lid's flow against another substance's
/// counter — and the fact that `Tick::advance` credits **before** the LEDGER
/// phase runs, which is the ordering ADR-059 states as "after phase 4 no channel
/// is written".
///
/// `Tick::advance` asserts both residuals itself in a debug build, so the body of
/// this test is mostly the setup; what it adds on top is that something actually
/// crossed the lid, or the assertion inside would be closing `0 == 0`.
#[test]
fn a_tick_over_a_venting_world_closes_both_residuals() {
    let derived = derived();
    let reservoir = derived.reservoir().expect("the fixture declares one");
    let mut world = vented_world(&derived);
    let n_substances = derived.substances().len() as u32;

    // Every voxel of every lane at a round number, and a reservoir that differs
    // from it in both directions across the registry — H2S at zero outside and a
    // domain that holds some, oxygen the other way round.
    for s in 0..n_substances {
        match world.lane_of(s) {
            LaneRef::Narrow(lane) => {
                let field = world.amounts_32_mut().expect("a narrow field");
                let n_voxels = field.n_voxels();
                let (_, dst) = field.lane_pair_mut(lane);
                for cell in dst.iter_mut().take(n_voxels as usize) {
                    *cell = M32::new(1_000_000);
                }
            }
            LaneRef::Wide(lane) => {
                let field = world.amounts_64_mut().expect("a wide field");
                let n_voxels = field.n_voxels();
                let (_, dst) = field.lane_pair_mut(lane);
                for cell in dst.iter_mut().take(n_voxels as usize) {
                    *cell = M64::new(1_000_000_000);
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
    world
        .seed_ghosts(&reservoir.amount_out, M64::new(reservoir.enthalpy_out))
        .unwrap();

    let mut roster: [RosterEntry; ROSTER_LEN] = default_roster();
    for entry in &mut roster {
        entry.enabled = entry.id == ProcessId::Diffusion;
    }

    let tick = Tick::new(&world, &derived, &roster, DT, DX).expect("folding the tick");
    let mut scratch = Scratch::new(&world, &tick).expect("the scratch buffers");
    let mut ledger = Ledger::new(n_substances).unwrap();

    let mut before = DomainSums::new(n_substances).unwrap();
    tick.domain_sums(&world, &mut before);

    // Ten ticks. `Tick::advance` closes both residuals per tick in a debug build
    // (ADR-059) — per tick, and not over the run, because a leak on tick 3
    // compensated on tick 7 leaves the two ends equal.
    for t in 0..10u32 {
        tick.advance(&mut world, &mut ledger, &mut scratch, t, 0);
    }

    let mut after = DomainSums::new(n_substances).unwrap();
    tick.domain_sums(&world, &mut after);

    // And the lid did something, or the assertions inside `advance` closed a
    // pair of zeroes ten times.
    let mut moved = 0;
    for s in 0..n_substances {
        let credited = i128::from(ledger.matter(Channel::BoundaryExchange, s));
        if credited != 0 {
            moved += 1;
            assert_eq!(
                after.matter(s) - before.matter(s),
                credited,
                "substance {s} moved by something other than what the lid counted"
            );
        }
    }
    assert!(moved >= 2, "only {moved} substances crossed the lid");

    // The energy half moved too: the enthalpy field is on the coarse grid and
    // vents on its own account (ADR-028, ADR-067).
    assert_ne!(ledger.energy(Channel::BoundaryExchange), 0);
    assert_eq!(
        after.energy() - before.energy(),
        i128::from(ledger.energy(Channel::BoundaryExchange))
    );

    // No other channel was written at all: everything that credits credits on
    // steps `c` and `d`, and this run dispatches only `d`.
    for channel in Channel::ALL {
        if channel == Channel::BoundaryExchange {
            continue;
        }
        assert_eq!(ledger.energy(channel), 0, "{}", channel.name());
        for s in 0..n_substances {
            assert_eq!(ledger.matter(channel, s), 0, "{}", channel.name());
        }
    }
}
