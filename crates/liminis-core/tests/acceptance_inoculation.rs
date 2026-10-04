//! Acceptance of declared initial concentrations and spatial inocula.
//!
//! The fixture uses ordinary chemical substances from the shipped ecological
//! scenario. Their declaration indices differ from their lanes, so a test that
//! addresses a substance index as a lane writes the wrong field while still
//! producing plausible values. The mixed-width form lives beside the private
//! fill in `worldgen`, where a deliberate narrow/wide registry can be built.

use liminis_core::config;
use liminis_core::world::{Boundary, Grid, LaneRef, Registry, World, WorldLayout};
use liminis_core::worldgen::generate;

const SCENARIO: &str = include_str!("../../../configs/scenarios/living-world.toml");
const GENETIC_SCENARIO: &str = include_str!("../../../configs/scenarios/genetic-colony.toml");
const N: u32 = 24;
const DX: f64 = 1.0e-4;

fn with_initial(initial: &str) -> String {
    let prefix = SCENARIO
        .find("\n[initial.")
        .map_or(SCENARIO, |at| &SCENARIO[..at]);
    format!("{prefix}\n{initial}\n")
}

fn derive(text: &str) -> config::Derived {
    let parsed = config::parse(text).expect("the inoculation fixture must parse");
    config::validate(&parsed).expect("the inoculation fixture must validate")
}

fn world(derived: &config::Derived) -> World {
    let grid = Grid::new(N, N, N, [Boundary::Closed; 6]).expect("the grid");
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

fn substance(derived: &config::Derived, id: &str) -> u32 {
    derived
        .substances()
        .iter()
        .position(|candidate| candidate.id == id)
        .expect("the declared substance") as u32
}

fn front(world: &World, substance: u32) -> Vec<i128> {
    match world.lane_of(substance) {
        LaneRef::Narrow(lane) => world
            .amounts_32()
            .expect("the narrow field")
            .lane(lane)
            .iter()
            .map(|amount| i128::from(amount.raw()))
            .collect(),
        LaneRef::Wide(lane) => world
            .amounts_64()
            .expect("the wide field")
            .lane(lane)
            .iter()
            .map(|amount| i128::from(amount.raw()))
            .collect(),
    }
}

#[test]
fn declared_founders_overwrite_the_closed_ball_on_their_own_lanes() {
    let scenario = with_initial(
        r#"
[initial.concentration]
WATER = 0.0
FOOD = 0.0

[[initial.inoculum]]
substance = "WATER"
center = [0.00015, 0.00015, 0.00015]
radius = 0.0001
concentration = 55000.0

[[initial.inoculum]]
substance = "FOOD"
center = [0.00015, 0.00015, 0.00015]
radius = 0.0001
concentration = 1.0
"#,
    );
    let derived = derive(&scenario);
    assert_eq!(derived.initial().dx, DX);

    let water = substance(&derived, "WATER");
    let food = substance(&derived, "FOOD");
    assert!(matches!(world(&derived).lane_of(food), LaneRef::Narrow(_)));

    let water_seed = derived
        .initial()
        .inocula
        .iter()
        .find(|inoculum| inoculum.substance == water)
        .expect("the water inoculum")
        .amount;
    let food_seed = derived
        .initial()
        .inocula
        .iter()
        .find(|inoculum| inoculum.substance == food)
        .expect("the food inoculum")
        .amount;

    let mut first = world(&derived);
    let report = generate(&mut first, &derived, 17).expect("the world must generate");
    let water_amounts = front(&first, water);
    let food_amounts = front(&first, food);

    let occupied: Vec<usize> = water_amounts
        .iter()
        .enumerate()
        .filter_map(|(idx, &amount)| (amount != 0).then_some(idx))
        .collect();
    assert_eq!(
        occupied.len(),
        7,
        "the centre and its six face neighbours lie in the closed radius-dx ball"
    );
    for (idx, (&water_amount, &food_amount)) in water_amounts.iter().zip(&food_amounts).enumerate()
    {
        if occupied.contains(&idx) {
            assert_eq!(water_amount, water_seed, "water at voxel {idx}");
            assert_eq!(food_amount, food_seed, "food at voxel {idx}");
        } else {
            assert_eq!(water_amount, 0, "water background at voxel {idx}");
            assert_eq!(food_amount, 0, "food background at voxel {idx}");
        }
    }

    for (substance, peak) in [(water, water_seed), (food, food_seed)] {
        let fill = &report.per_substance[substance as usize];
        assert_eq!(fill.floor, 0);
        assert_eq!(fill.peak, peak);
        assert!(!fill.uniform);
    }
    let printed = report.report();
    assert!(
        printed.contains(&format!("substance FOOD: actual [0, {food_seed}], typical"))
            && printed.contains("uniform false"),
        "the load report must describe the final overwritten field: {printed}"
    );

    let mut second = world(&derived);
    generate(&mut second, &derived, 9001).expect("the replay must generate");
    assert_eq!(front(&first, water), front(&second, water));
    assert_eq!(front(&first, food), front(&second, food));

    let oxygen = substance(&derived, "O2");
    assert_ne!(
        front(&first, oxygen),
        front(&second, oxygen),
        "an undeclared lane must retain the stochastic run-key path"
    );
}

#[test]
fn the_genetic_colony_starts_with_one_founder_and_three_empty_genotypes() {
    let derived = derive(GENETIC_SCENARIO);
    let genotypes = ["G00", "G01", "G10", "G11"];
    let indices = genotypes.map(|id| substance(&derived, id));

    for &substance in &indices {
        let background = derived
            .initial()
            .concentration
            .iter()
            .find(|declared| declared.substance == substance)
            .expect("every genotype declares its zero background");
        assert_eq!(background.amount, 0);
    }
    assert_eq!(derived.initial().inocula.len(), 1);
    assert_eq!(derived.initial().inocula[0].substance, indices[0]);

    let mut colony = world(&derived);
    let report = generate(&mut colony, &derived, 42).expect("the colony must generate");
    let founder_amount = derived.initial().inocula[0].amount;
    let founder = front(&colony, indices[0]);
    assert!(founder.contains(&founder_amount));
    assert!(
        founder
            .iter()
            .all(|&amount| amount == 0 || amount == founder_amount),
        "the founder lane contains only its zero background and declared inoculum"
    );
    let founder_report = &report.per_substance[indices[0] as usize];
    assert_eq!(
        (founder_report.floor, founder_report.peak),
        (0, founder_amount)
    );
    assert!(!founder_report.uniform);

    for (&id, &substance) in genotypes[1..].iter().zip(&indices[1..]) {
        assert!(
            front(&colony, substance).iter().all(|&amount| amount == 0),
            "{id} must be absent before mutation creates it"
        );
        let fill = &report.per_substance[substance as usize];
        assert_eq!((fill.floor, fill.peak), (0, 0), "{id}");
        assert!(fill.uniform, "{id}");
    }
}
