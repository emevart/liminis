//! Independent causal checks for the actual-cell chamber, not its renderer.
use liminis_core::micro::{self, MicroConfig, MicroState, config};

const SOURCE: &str = include_str!("../../../configs/scenarios/cell-chamber.toml");

fn scenario(seed: u64) -> MicroConfig {
    config::derive(&config::parse(SOURCE).expect("cell scenario"), seed)
        .expect("derived cell scenario")
}

fn matter(config: &MicroConfig, state: &MicroState) -> Vec<i128> {
    let mut amounts = state.matter.clone();
    amounts[config.growth.biomass_substance] +=
        state.cells.iter().map(|cell| cell.mass).sum::<i128>();
    amounts
}

fn energy(config: &MicroConfig, state: &MicroState) -> i128 {
    let chemical: i128 = matter(config, state)
        .iter()
        .zip(&config.chemical_weights)
        .map(|(amount, weight)| amount * i128::from(*weight))
        .sum();
    chemical + state.heat + state.cells.iter().map(|cell| cell.energy).sum::<i128>()
}

#[test]
fn sterile_supplied_medium_does_not_create_cells() {
    let config = scenario(42);
    let mut state = MicroState::new(&config).unwrap();
    state.cells.clear();
    let next_id = state.next_cell_id;
    for _ in 0..500 {
        let before = matter(&config, &state);
        let before_energy = energy(&config, &state);
        let report = micro::step(&config, &mut state).unwrap();
        assert_eq!(report.extent, 0);
        assert_eq!(report.births, 0);
        assert_eq!(report.deaths, 0);
        assert!(state.cells.is_empty());
        assert_eq!(state.next_cell_id, next_id);
        for ((after, before), exchange) in matter(&config, &state)
            .iter()
            .zip(&before)
            .zip(&report.medium_matter)
        {
            assert_eq!(after - before, *exchange);
        }
        assert_eq!(
            energy(&config, &state) - before_energy,
            report.medium_energy
        );
    }
}

#[test]
fn starvation_lyses_actual_cells_without_losing_matter_or_energy() {
    let mut config = scenario(42);
    config.initial_matter.fill(0);
    for pool in &mut config.medium {
        pool.max_delta_per_tick = 0;
    }
    config.founder.energy = 0;
    let mut state = MicroState::new(&config).unwrap();
    let initial_mass: i128 = state.cells.iter().map(|cell| cell.mass).sum();
    let initial_energy = energy(&config, &state);
    let mut deaths = 0;
    let mut death_mass = 0;
    for _ in 0..config.founder.genome.starvation_tolerance_ticks + 2 {
        let report = micro::step(&config, &mut state).unwrap();
        assert_eq!(report.extent, 0);
        assert_eq!(report.births, 0);
        assert_eq!(report.energy_residual, 0);
        deaths += report.deaths;
        death_mass += report.death_mass;
    }
    assert!(state.cells.is_empty());
    assert_eq!(deaths, config.founder.count);
    assert_eq!(death_mass, initial_mass);
    assert_eq!(state.matter[config.death.detritus_substance], initial_mass);
    assert_eq!(energy(&config, &state), initial_energy);
}

#[test]
fn zero_mutation_preserves_the_entire_inherited_genome() {
    let mut config = scenario(42);
    config.mutation.probability = 0.0;
    let mut state = MicroState::new(&config).unwrap();
    let mut divisions = 0;
    for _ in 0..1000 {
        let report = micro::step(&config, &mut state).unwrap();
        divisions += report.fissions;
        assert!(
            state
                .cells
                .iter()
                .all(|cell| cell.genome == config.founder.genome)
        );
    }
    assert!(divisions > 0, "heredity control must include real births");
    assert!(!state.cells.is_empty());
}

#[test]
fn restored_future_matches_exact_cells_and_independent_cumulative_ledgers() {
    let config = scenario(u64::MAX);
    let mut original = MicroState::new(&config).unwrap();
    for _ in 0..300 {
        micro::step(&config, &mut original).unwrap();
    }
    let mut restored = MicroState::from_snapshot(&config, original.snapshot()).unwrap();
    let initial_matter = matter(&config, &original);
    let initial_energy = energy(&config, &original);
    let mut expected_matter = vec![0_i128; config.matter_ids.len()];
    let mut expected_energy = 0;
    for _ in 0..500 {
        let report = micro::step(&config, &mut original).unwrap();
        assert_eq!(report, micro::step(&config, &mut restored).unwrap());
        assert_eq!(original, restored);
        expected_energy += report.medium_energy;
        for (total, delta) in expected_matter.iter_mut().zip(&report.medium_matter) {
            *total += delta;
        }
        for nu in &config.growth.nu {
            expected_matter[nu.substance] += i128::from(nu.value) * report.extent;
        }
        expected_matter[config.growth.biomass_substance] -= report.death_mass;
        expected_matter[config.death.detritus_substance] += report.death_mass;
        for ((after, initial), expected) in matter(&config, &original)
            .iter()
            .zip(&initial_matter)
            .zip(&expected_matter)
        {
            assert_eq!(after - initial, *expected);
        }
        assert_eq!(energy(&config, &original) - initial_energy, expected_energy);
    }
}
