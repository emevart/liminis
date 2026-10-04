//! A bounded, well-mixed individual-cell engine.
//!
//! This is deliberately a separate operator from the voxel world.  The chamber
//! has real integer chemical pools and real individual cells, but no positions:
//! any arrangement used by an observer is presentation, not simulated motion.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::numeric::{Q, qadd, qdiv, qmul, rand, stochastic_round};

pub mod config;
pub use config::{
    DeathSpec, FounderSpec, Genome, GrowthSpec, MediumExchange, MicroConfig, MicroNu, MutationSpec,
};

const DEMAND_PURPOSE: u32 = 0x6d69_6301;
const COMPETITION_PURPOSE: u32 = 0x6d69_6302;
const MUTATION_PURPOSE: u32 = 0x6d69_6304;
const MUTATION_SIGN_PURPOSE: u32 = 0x6d69_6305;

/// One simulated cell.  Mass is structural `BIO` in that substance's integer
/// storage units; energy is in the same integer units as chamber heat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub genome: Genome,
    pub mass: i128,
    pub energy: i128,
    pub age: u64,
    pub birth_tick: u64,
    pub generation: u32,
    pub starvation_ticks: u32,
}

impl MicroConfig {
    pub fn validate(&self) -> Result<()> {
        config::validate_runtime(self)?;
        ensure!(
            self.dt_seconds.is_finite() && self.dt_seconds > 0.0,
            "micro dt_seconds must be finite and positive"
        );
        ensure!(self.max_cells > 0, "micro max_cells must be positive");
        ensure!(
            !self.matter_ids.is_empty(),
            "micro chamber needs matter pools"
        );
        ensure!(
            self.matter_ids.len() == self.initial_matter.len(),
            "micro matter_ids and initial_matter lengths differ"
        );
        ensure!(
            self.matter_ids.len() == self.chemical_weights.len(),
            "micro matter_ids and chemical_weights lengths differ"
        );
        ensure!(
            self.initial_matter.iter().all(|amount| *amount >= 0),
            "micro initial matter must be nonnegative"
        );
        ensure!(
            self.temperature_kelvin.is_finite() && self.temperature_kelvin > 0.0,
            "micro bath temperature must be finite and positive"
        );
        ensure!(
            self.initial_bath_heat >= 0,
            "micro initial cumulative bath heat must be nonnegative"
        );
        let mut ids = BTreeSet::new();
        ensure!(
            self.matter_ids
                .iter()
                .all(|id| !id.is_empty() && ids.insert(id)),
            "micro matter ids must be nonempty and unique"
        );

        ensure!(
            !self.growth.nu.is_empty(),
            "micro growth stoichiometry is empty"
        );
        ensure!(
            self.growth.biomass_substance < self.matter_ids.len(),
            "micro BIO index is outside the matter registry"
        );
        ensure!(
            self.growth.limiting_substance < self.matter_ids.len(),
            "micro limiting-substance index is outside the matter registry"
        );
        ensure!(
            self.death.detritus_substance < self.matter_ids.len(),
            "micro DET index is outside the matter registry"
        );
        ensure!(
            self.growth.biomass_substance != self.death.detritus_substance,
            "micro BIO and DET must be distinct substances"
        );

        let mut participants = BTreeSet::new();
        let mut has_input = false;
        let mut bio_nu = None;
        let mut chemical_delta = 0i128;
        for nu in &self.growth.nu {
            ensure!(
                nu.substance < self.matter_ids.len(),
                "micro growth participant {} is outside the matter registry",
                nu.substance
            );
            ensure!(
                nu.value != 0,
                "micro growth contains a zero stoichiometric coefficient"
            );
            ensure!(
                participants.insert(nu.substance),
                "micro growth repeats substance {}",
                nu.substance
            );
            has_input |= nu.value < 0;
            if nu.substance == self.growth.biomass_substance {
                bio_nu = Some(nu.value);
            }
            chemical_delta = chemical_delta
                .checked_add(
                    i128::from(nu.value)
                        .checked_mul(i128::from(self.chemical_weights[nu.substance]))
                        .context("micro growth chemical-energy coefficient overflows i128")?,
                )
                .context("micro growth chemical-energy sum overflows i128")?;
        }
        ensure!(
            has_input,
            "micro growth must consume at least one chamber substance"
        );
        ensure!(
            bio_nu.is_some_and(|nu| nu > 0),
            "micro growth must produce positive BIO"
        );
        ensure!(
            self.growth
                .nu
                .iter()
                .any(|nu| nu.substance == self.growth.limiting_substance && nu.value < 0),
            "micro limiting substance must be a consumed growth participant"
        );
        ensure!(
            self.growth.energy_per_extent == -chemical_delta,
            "micro growth energy_per_extent must equal -sum(nu * chemical_weight)"
        );
        ensure!(
            self.growth.energy_per_extent >= 0,
            "micro first-cell growth reaction must release nonnegative energy"
        );

        let expected_death_heat = i128::from(self.chemical_weights[self.growth.biomass_substance])
            - i128::from(self.chemical_weights[self.death.detritus_substance]);
        ensure!(
            i128::from(self.death.chemical_heat_per_mass) == expected_death_heat,
            "micro death heat correction differs from BIO minus DET chemical weight"
        );
        ensure!(
            self.death.chemical_heat_per_mass >= 0,
            "micro format 1 does not support endothermic lysis"
        );
        ensure!(
            self.mutation.min_kinetics <= self.mutation.max_kinetics,
            "micro genome bounds are reversed"
        );
        probability("micro mutation probability", self.mutation.probability)?;
        ensure!(
            self.mutation.speed_factor_per_step.is_finite()
                && self.mutation.speed_factor_per_step >= 1.0,
            "micro speed factor per step must be finite and at least one"
        );
        ensure!(
            self.mutation.affinity_cost_per_step.is_finite()
                && self.mutation.affinity_cost_per_step >= 1.0,
            "micro affinity cost per step must be finite and at least one"
        );
        ensure!(
            self.founder.count > 0 && self.founder.count <= self.max_cells,
            "micro founder count is outside 1..=max_cells"
        );
        ensure!(self.founder.mass > 0, "micro founder mass must be positive");
        ensure!(
            self.founder.energy >= 0,
            "micro founder energy must be nonnegative"
        );
        self.validate_genome(&self.founder.genome)?;
        ensure!(
            self.medium.len() == self.matter_ids.len(),
            "micro medium must declare every free matter pool"
        );
        for exchange in &self.medium {
            ensure!(
                exchange.target_amount >= 0 && exchange.max_delta_per_tick >= 0,
                "micro medium target and maximum delta must be nonnegative"
            );
        }
        Ok(())
    }

    fn genome_contains(&self, genome: &Genome) -> bool {
        (self.mutation.min_kinetics..=self.mutation.max_kinetics).contains(&genome.kinetics)
    }

    fn validate_genome(&self, genome: &Genome) -> Result<()> {
        ensure!(
            self.genome_contains(genome),
            "micro genome kinetics is outside the declared bounds"
        );
        ensure!(
            genome.max_growth_rate_per_second.is_finite()
                && genome.max_growth_rate_per_second >= 0.0,
            "micro genome maximum growth rate must be finite and nonnegative"
        );
        ensure!(
            genome.affinity_km_amount > 0,
            "micro genome affinity Km must be positive"
        );
        ensure!(
            genome.division_mass >= 2,
            "micro genome division mass must be at least two units"
        );
        ensure!(
            genome.maintenance_energy_per_tick >= 0,
            "micro genome maintenance must be nonnegative"
        );
        ensure!(
            genome.capture_denominator > 0
                && genome.capture_numerator <= genome.capture_denominator,
            "micro genome capture fraction is outside [0,1]"
        );
        ensure!(
            genome.division_energy_cost >= 0,
            "micro genome division cost must be nonnegative"
        );
        let mut inherited = self.founder.genome;
        inherited.kinetics = genome.kinetics;
        ensure!(
            *genome == inherited,
            "micro format 1 permits mutation only at the kinetics locus"
        );
        Ok(())
    }
}

fn probability(name: &str, value: f64) -> Result<()> {
    ensure!(
        value.is_finite() && (0.0..=1.0).contains(&value),
        "{name} must be finite and in [0, 1]"
    );
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct MicroState {
    pub tick: u64,
    pub next_cell_id: u64,
    pub matter: Vec<i128>,
    pub heat: i128,
    pub cells: Vec<Cell>,
}

/// Persistence DTO.  The host is responsible for representing its `i128`
/// values as decimal strings in JSON intended for JavaScript.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MicroSnapshot {
    pub tick: u64,
    pub next_cell_id: u64,
    pub matter: Vec<i128>,
    pub heat: i128,
    pub cells: Vec<Cell>,
}

impl MicroState {
    pub fn new(config: &MicroConfig) -> Result<Self> {
        config.validate()?;
        let cells = (0..config.founder.count)
            .map(|id| Cell {
                id: u64::from(id),
                parent_id: None,
                genome: config.founder.genome,
                mass: config.founder.mass,
                energy: config.founder.energy,
                age: 0,
                birth_tick: 0,
                generation: 0,
                starvation_ticks: 0,
            })
            .collect();
        let state = Self {
            tick: 0,
            next_cell_id: u64::from(config.founder.count),
            matter: config.initial_matter.clone(),
            heat: config.initial_bath_heat,
            cells,
        };
        state.validate(config)?;
        Ok(state)
    }

    pub fn snapshot(&self) -> MicroSnapshot {
        MicroSnapshot {
            tick: self.tick,
            next_cell_id: self.next_cell_id,
            matter: self.matter.clone(),
            heat: self.heat,
            cells: self.cells.clone(),
        }
    }

    pub fn from_snapshot(config: &MicroConfig, snapshot: MicroSnapshot) -> Result<Self> {
        config.validate()?;
        let state = Self {
            tick: snapshot.tick,
            next_cell_id: snapshot.next_cell_id,
            matter: snapshot.matter,
            heat: snapshot.heat,
            cells: snapshot.cells,
        };
        state.validate(config)?;
        Ok(state)
    }

    pub fn validate(&self, config: &MicroConfig) -> Result<()> {
        ensure!(
            self.matter.len() == config.matter_ids.len(),
            "micro snapshot matter-pool count differs from config"
        );
        ensure!(
            self.matter.iter().all(|amount| *amount >= 0),
            "micro snapshot contains negative chamber matter"
        );
        ensure!(
            self.heat >= 0,
            "micro snapshot contains negative cumulative bath heat"
        );
        ensure!(
            self.cells.len() <= config.max_cells as usize,
            "micro snapshot exceeds max_cells"
        );
        let mut ids = BTreeSet::new();
        for cell in &self.cells {
            ensure!(
                ids.insert(cell.id),
                "micro snapshot repeats cell id {}",
                cell.id
            );
            ensure!(
                cell.id < self.next_cell_id,
                "micro snapshot cell id {} is not below next_cell_id",
                cell.id
            );
            ensure!(
                cell.parent_id.is_none_or(|parent| parent < cell.id),
                "micro snapshot cell {} has a non-ancestral parent id",
                cell.id
            );
            ensure!(
                config.genome_contains(&cell.genome),
                "micro snapshot cell {} has an out-of-bounds genome",
                cell.id
            );
            config
                .validate_genome(&cell.genome)
                .with_context(|| format!("micro snapshot cell {} genome", cell.id))?;
            ensure!(
                cell.mass > 0,
                "micro snapshot cell {} has nonpositive mass",
                cell.id
            );
            ensure!(
                cell.energy >= 0,
                "micro snapshot cell {} has negative energy",
                cell.id
            );
            ensure!(
                cell.birth_tick <= self.tick,
                "micro snapshot cell {} was born after the snapshot tick",
                cell.id
            );
            ensure!(
                cell.age == self.tick - cell.birth_tick,
                "micro snapshot cell {} age is inconsistent with birth_tick",
                cell.id
            );
            ensure!(
                (cell.generation == 0) == cell.parent_id.is_none(),
                "micro snapshot cell {} ancestry and generation disagree",
                cell.id
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicroStepReport {
    pub tick: u64,
    pub extent: i128,
    pub births: u32,
    pub fissions: u32,
    pub deaths: u32,
    pub death_mass: i128,
    pub medium_matter: Vec<i128>,
    pub medium_energy: i128,
    pub matter_residual: Vec<i128>,
    pub energy_before: i128,
    pub energy_after: i128,
    pub energy_residual: i128,
    pub cell_count: u32,
}

/// Advance one chamber tick.  Demand is collected before any pool changes;
/// every cell is then scaled by the same limiting extent fraction.
pub fn step(config: &MicroConfig, state: &mut MicroState) -> Result<MicroStepReport> {
    let mut candidate = state.clone();
    let report = step_in_place(config, &mut candidate)?;
    *state = candidate;
    Ok(report)
}

fn step_in_place(config: &MicroConfig, state: &mut MicroState) -> Result<MicroStepReport> {
    config.validate()?;
    state.validate(config)?;
    ensure!(state.tick < u64::MAX, "micro tick counter is exhausted");
    state.cells.sort_unstable_by_key(|cell| cell.id);

    let before_matter = combined_matter(config, state)?;
    let energy_before = total_energy(config, state)?;
    let (medium_matter, medium_energy) = apply_medium(config, state)?;
    let demands = demands(config, state)?;
    let allocations = allocate_competition(config, state, &demands)?;
    let extent = allocations.iter().try_fold(0i128, |sum, value| {
        sum.checked_add(*value)
            .context("micro total growth extent overflows i128")
    })?;

    apply_growth(config, state, &allocations)?;
    apply_upkeep(state)?;
    let (deaths, death_mass) = apply_death(config, state)?;
    let (births, fissions) = apply_fission(config, state)?;
    for cell in &mut state.cells {
        if cell.birth_tick != state.tick + 1 {
            cell.age = cell
                .age
                .checked_add(1)
                .context("micro cell age overflows u64")?;
        }
    }
    state.tick += 1;
    state.cells.sort_unstable_by_key(|cell| cell.id);
    state.validate(config)?;

    let after_matter = combined_matter(config, state)?;
    let mut expected_delta = vec![0i128; state.matter.len()];
    expected_delta.copy_from_slice(&medium_matter);
    for nu in &config.growth.nu {
        expected_delta[nu.substance] = expected_delta[nu.substance]
            .checked_add(
                i128::from(nu.value)
                    .checked_mul(extent)
                    .context("micro expected growth delta overflows i128")?,
            )
            .context("micro expected matter delta overflows i128")?;
    }
    let bio = config.growth.biomass_substance;
    let det = config.death.detritus_substance;
    expected_delta[bio] = expected_delta[bio]
        .checked_sub(death_mass)
        .context("micro expected BIO death delta overflows i128")?;
    expected_delta[det] = expected_delta[det]
        .checked_add(death_mass)
        .context("micro expected DET death delta overflows i128")?;
    let matter_residual = after_matter
        .iter()
        .zip(&before_matter)
        .zip(&expected_delta)
        .map(|((&after, &before), &expected)| {
            after
                .checked_sub(before)
                .and_then(|delta| delta.checked_sub(expected))
                .context("micro matter residual overflows i128")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        matter_residual.iter().all(|residual| *residual == 0),
        "micro matter ledger did not close: {matter_residual:?}"
    );

    let energy_after = total_energy(config, state)?;
    let energy_residual = energy_after
        .checked_sub(energy_before)
        .and_then(|delta| delta.checked_sub(medium_energy))
        .context("micro energy residual overflows i128")?;
    ensure!(
        energy_residual == 0,
        "micro energy ledger did not close: {energy_residual}"
    );
    Ok(MicroStepReport {
        tick: state.tick,
        extent,
        births,
        fissions,
        deaths,
        death_mass,
        medium_matter,
        medium_energy,
        matter_residual,
        energy_before,
        energy_after,
        energy_residual,
        cell_count: u32::try_from(state.cells.len())
            .context("micro cell count does not fit u32")?,
    })
}

fn apply_medium(config: &MicroConfig, state: &mut MicroState) -> Result<(Vec<i128>, i128)> {
    let mut changes = Vec::with_capacity(state.matter.len());
    let mut energy = 0i128;
    for (substance, exchange) in config.medium.iter().enumerate() {
        let gap = exchange
            .target_amount
            .checked_sub(state.matter[substance])
            .context("micro medium gap overflows i128")?;
        let magnitude = gap
            .checked_abs()
            .context("micro medium gap cannot be represented")?
            .min(exchange.max_delta_per_tick);
        let delta = magnitude * gap.signum();
        state.matter[substance] = state.matter[substance]
            .checked_add(delta)
            .context("micro medium pool overflows i128")?;
        energy = energy
            .checked_add(
                delta
                    .checked_mul(i128::from(config.chemical_weights[substance]))
                    .context("micro medium energy product overflows i128")?,
            )
            .context("micro medium energy credit overflows i128")?;
        changes.push(delta);
    }
    Ok((changes, energy))
}

fn combined_matter(config: &MicroConfig, state: &MicroState) -> Result<Vec<i128>> {
    let mut total = state.matter.clone();
    let biomass = state.cells.iter().try_fold(0i128, |sum, cell| {
        sum.checked_add(cell.mass)
            .context("micro biomass sum overflows i128")
    })?;
    total[config.growth.biomass_substance] = total[config.growth.biomass_substance]
        .checked_add(biomass)
        .context("micro combined BIO amount overflows i128")?;
    Ok(total)
}

fn total_energy(config: &MicroConfig, state: &MicroState) -> Result<i128> {
    let matter = combined_matter(config, state)?;
    let chemical = matter.iter().zip(&config.chemical_weights).try_fold(
        0i128,
        |sum, (&amount, &weight)| {
            sum.checked_add(
                amount
                    .checked_mul(i128::from(weight))
                    .context("micro chemical-energy product overflows i128")?,
            )
            .context("micro chemical-energy sum overflows i128")
        },
    )?;
    let internal = state.cells.iter().try_fold(0i128, |sum, cell| {
        sum.checked_add(cell.energy)
            .context("micro internal-energy sum overflows i128")
    })?;
    state
        .heat
        .checked_add(chemical)
        .and_then(|sum| sum.checked_add(internal))
        .context("micro total energy overflows i128")
}

fn demands(config: &MicroConfig, state: &MicroState) -> Result<Vec<i128>> {
    let biomass_per_extent = config
        .growth
        .nu
        .iter()
        .find(|nu| nu.substance == config.growth.biomass_substance)
        .expect("validated BIO participant")
        .value;
    let resource = Q::from_f64(state.matter[config.growth.limiting_substance] as f64);
    state
        .cells
        .iter()
        .map(|cell| {
            let phenotype = config::decode_genome(config, &cell.genome);
            ensure!(
                phenotype.max_growth_rate_per_second.is_finite()
                    && phenotype.affinity_km_amount.is_finite(),
                "micro genome phenotype is not finite"
            );
            let saturation = qdiv(
                resource,
                qadd(resource, Q::from_f64(phenotype.affinity_km_amount)),
            );
            let maximum_extent =
                cell.mass as f64 * phenotype.max_growth_rate_per_second * config.dt_seconds
                    / biomass_per_extent as f64;
            ensure!(
                maximum_extent.is_finite()
                    && maximum_extent >= 0.0
                    && maximum_extent < i64::MAX as f64,
                "micro cell {} demand is outside the stochastic-rounding range",
                cell.id
            );
            let demand = qmul(qmul(Q::from_f64(maximum_extent), saturation), Q::ONE);
            ensure!(
                demand.debug_f64().is_finite()
                    && demand.debug_f64() >= 0.0
                    && demand.debug_f64() < i64::MAX as f64,
                "micro cell {} folded demand is outside the stochastic-rounding range",
                cell.id
            );
            let draw = rand(
                fold_id(cell.id),
                state.tick as u32,
                DEMAND_PURPOSE,
                config.run_key,
            );
            Ok(i128::from(stochastic_round(demand, draw).max(0)))
        })
        .collect()
}

fn allocate_competition(
    config: &MicroConfig,
    state: &MicroState,
    demands: &[i128],
) -> Result<Vec<i128>> {
    let total_demand = demands.iter().try_fold(0i128, |sum, value| {
        sum.checked_add(*value)
            .context("micro total demand overflows i128")
    })?;
    if total_demand == 0 {
        return Ok(vec![0; demands.len()]);
    }
    let cap = config
        .growth
        .nu
        .iter()
        .filter(|nu| nu.value < 0)
        .map(|nu| state.matter[nu.substance] / -i128::from(nu.value))
        .min()
        .unwrap_or(0)
        .min(total_demand);
    let mut allocation = vec![0i128; demands.len()];
    let mut ranked = Vec::with_capacity(demands.len());
    let mut assigned = 0i128;
    for (index, (&demand, cell)) in demands.iter().zip(&state.cells).enumerate() {
        let product = demand
            .checked_mul(cap)
            .context("micro competition product overflows i128")?;
        allocation[index] = product / total_demand;
        assigned = assigned
            .checked_add(allocation[index])
            .context("micro competition allocation overflows i128")?;
        ranked.push((
            product % total_demand,
            rand(
                fold_id(cell.id),
                state.tick as u32,
                COMPETITION_PURPOSE,
                config.run_key,
            ),
            cell.id,
            index,
        ));
    }
    ranked.sort_unstable_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    let remainder = usize::try_from(cap - assigned)
        .context("micro competition remainder does not fit usize")?;
    ensure!(
        remainder <= ranked.len(),
        "micro Hamilton remainder exceeds cell count"
    );
    for &(_, _, _, index) in ranked.iter().take(remainder) {
        allocation[index] += 1;
    }
    Ok(allocation)
}

fn apply_growth(config: &MicroConfig, state: &mut MicroState, allocation: &[i128]) -> Result<()> {
    for (cell, &extent) in state.cells.iter_mut().zip(allocation) {
        if extent == 0 {
            continue;
        }
        for nu in &config.growth.nu {
            let delta = i128::from(nu.value)
                .checked_mul(extent)
                .context("micro growth matter delta overflows i128")?;
            if nu.substance == config.growth.biomass_substance {
                cell.mass = cell
                    .mass
                    .checked_add(delta)
                    .context("micro cell mass overflows i128")?;
            } else {
                state.matter[nu.substance] = state.matter[nu.substance]
                    .checked_add(delta)
                    .context("micro chamber pool overflows i128")?;
                ensure!(
                    state.matter[nu.substance] >= 0,
                    "micro growth overdraws {}",
                    config.matter_ids[nu.substance]
                );
            }
        }
        let released = config
            .growth
            .energy_per_extent
            .checked_mul(extent)
            .context("micro released growth energy overflows i128")?;
        let captured = released
            .checked_mul(i128::from(cell.genome.capture_numerator))
            .context("micro captured growth numerator overflows i128")?
            / i128::from(cell.genome.capture_denominator);
        let heat = released - captured;
        cell.energy = cell
            .energy
            .checked_add(captured)
            .context("micro cell energy overflows i128")?;
        state.heat = state
            .heat
            .checked_add(heat)
            .context("micro chamber heat overflows i128")?;
    }
    Ok(())
}

fn apply_upkeep(state: &mut MicroState) -> Result<()> {
    for cell in &mut state.cells {
        let required = cell.genome.maintenance_energy_per_tick;
        let paid = cell.energy.min(required);
        cell.energy -= paid;
        state.heat = state
            .heat
            .checked_add(paid)
            .context("micro upkeep heat overflows i128")?;
        if paid == required {
            cell.starvation_ticks = 0;
        } else {
            cell.starvation_ticks = cell
                .starvation_ticks
                .checked_add(1)
                .context("micro starvation counter overflows u32")?;
        }
    }
    Ok(())
}

fn apply_death(config: &MicroConfig, state: &mut MicroState) -> Result<(u32, i128)> {
    let mut survivors = Vec::with_capacity(state.cells.len());
    let mut deaths = 0u32;
    let mut death_mass = 0i128;
    for cell in state.cells.drain(..) {
        if cell.starvation_ticks > cell.genome.starvation_tolerance_ticks {
            deaths = deaths
                .checked_add(1)
                .context("micro death count overflows u32")?;
            death_mass = death_mass
                .checked_add(cell.mass)
                .context("micro death mass overflows i128")?;
            state.matter[config.death.detritus_substance] = state.matter
                [config.death.detritus_substance]
                .checked_add(cell.mass)
                .context("micro detritus pool overflows i128")?;
            let chemical_correction = i128::from(config.death.chemical_heat_per_mass)
                .checked_mul(cell.mass)
                .context("micro death energy correction overflows i128")?;
            state.heat = state
                .heat
                .checked_add(cell.energy)
                .and_then(|heat| heat.checked_add(chemical_correction))
                .context("micro death heat overflows i128")?;
        } else {
            survivors.push(cell);
        }
    }
    state.cells = survivors;
    Ok((deaths, death_mass))
}

fn apply_fission(config: &MicroConfig, state: &mut MicroState) -> Result<(u32, u32)> {
    let cells = std::mem::take(&mut state.cells);
    let fissions_needed = cells
        .iter()
        .filter(|cell| {
            cell.mass >= cell.genome.division_mass
                && cell.energy >= cell.genome.division_energy_cost
                && cell.starvation_ticks == 0
        })
        .count();
    ensure!(
        cells
            .len()
            .checked_add(fissions_needed)
            .is_some_and(|count| count <= config.max_cells as usize),
        "micro division would exceed max_cells; the run must pause rather than hide a capacity limit"
    );
    let mut out = Vec::with_capacity(cells.len().saturating_mul(2).min(config.max_cells as usize));
    let mut births = 0u32;
    for cell in cells {
        let can_divide = cell.mass >= cell.genome.division_mass
            && cell.energy >= cell.genome.division_energy_cost
            && cell.starvation_ticks == 0;
        if !can_divide {
            out.push(cell);
            continue;
        }
        let first_mass = cell.mass / 2;
        let second_mass = cell.mass - first_mass;
        let reserve = cell.energy - cell.genome.division_energy_cost;
        state.heat = state
            .heat
            .checked_add(cell.genome.division_energy_cost)
            .context("micro division heat overflows i128")?;
        let first_energy = reserve / 2;
        let second_energy = reserve - first_energy;
        let first_id = take_id(state)?;
        let second_id = take_id(state)?;
        let generation = cell
            .generation
            .checked_add(1)
            .context("micro cell generation overflows u32")?;
        out.push(Cell {
            id: first_id,
            parent_id: Some(cell.id),
            genome: mutate(config, &cell.genome, first_id, state.tick + 1),
            mass: first_mass,
            energy: first_energy,
            age: 0,
            birth_tick: state.tick + 1,
            generation,
            starvation_ticks: 0,
        });
        out.push(Cell {
            id: second_id,
            parent_id: Some(cell.id),
            genome: mutate(config, &cell.genome, second_id, state.tick + 1),
            mass: second_mass,
            energy: second_energy,
            age: 0,
            birth_tick: state.tick + 1,
            generation,
            starvation_ticks: 0,
        });
        births = births
            .checked_add(2)
            .context("micro birth count overflows u32")?;
    }
    state.cells = out;
    Ok((
        births,
        u32::try_from(fissions_needed).context("micro fission count does not fit u32")?,
    ))
}

fn take_id(state: &mut MicroState) -> Result<u64> {
    let id = state.next_cell_id;
    state.next_cell_id = state
        .next_cell_id
        .checked_add(1)
        .context("micro cell id space is exhausted")?;
    Ok(id)
}

fn mutate(config: &MicroConfig, genome: &Genome, id: u64, tick: u64) -> Genome {
    let draw = rand(fold_id(id), tick as u32, MUTATION_PURPOSE, config.run_key);
    if !draw_probability(draw, config.mutation.probability) {
        return *genome;
    }
    let sign = rand(
        fold_id(id),
        tick as u32,
        MUTATION_SIGN_PURPOSE,
        config.run_key,
    );
    let delta = if sign & 1 == 0 { -1 } else { 1 };
    Genome {
        kinetics: genome
            .kinetics
            .saturating_add(delta)
            .clamp(config.mutation.min_kinetics, config.mutation.max_kinetics),
        ..*genome
    }
}

fn draw_probability(draw: u32, probability: f64) -> bool {
    f64::from(draw) < probability * (f64::from(u32::MAX) + 1.0)
}

fn fold_id(id: u64) -> u32 {
    id as u32 ^ (id >> 32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genome() -> Genome {
        Genome {
            kinetics: 0,
            max_growth_rate_per_second: 0.25,
            affinity_km_amount: 10,
            division_mass: 1_000_000,
            maintenance_energy_per_tick: 1,
            capture_numerator: 2,
            capture_denominator: 5,
            division_energy_cost: 2,
            starvation_tolerance_ticks: 1,
        }
    }

    fn fixture() -> MicroConfig {
        // FOOD -> BIO, with ten units of chemical energy released per extent.
        MicroConfig {
            chamber_format: config::CHAMBER_FORMAT_VERSION,
            name: "unit chamber".into(),
            config_hash: "unit".into(),
            dt_seconds: 1.0,
            volume_m3: 1.0,
            temperature_kelvin: 298.15,
            run_key: 17,
            max_cells: 64,
            matter_ids: vec!["FOOD".into(), "BIO".into(), "DET".into()],
            units_per_mol: vec![1, 1, 1],
            initial_matter: vec![100_000, 0, 0],
            chemical_weights: vec![20, 10, 10],
            initial_bath_heat: 1_000,
            energy_units_per_joule: 1,
            growth: GrowthSpec {
                nu: vec![
                    MicroNu {
                        substance: 0,
                        value: -1,
                    },
                    MicroNu {
                        substance: 1,
                        value: 1,
                    },
                ],
                biomass_substance: 1,
                limiting_substance: 0,
                energy_per_extent: 10,
            },
            death: DeathSpec {
                detritus_substance: 2,
                chemical_heat_per_mass: 0,
            },
            mutation: MutationSpec {
                min_kinetics: -4,
                max_kinetics: 4,
                probability: 0.2,
                speed_factor_per_step: 2.0,
                affinity_cost_per_step: 4.0,
            },
            founder: FounderSpec {
                count: 4,
                mass: 8,
                energy: 8,
                genome: genome(),
            },
            medium: vec![
                MediumExchange {
                    target_amount: 100_000,
                    max_delta_per_tick: 0,
                },
                MediumExchange {
                    target_amount: 0,
                    max_delta_per_tick: 0,
                },
                MediumExchange {
                    target_amount: 0,
                    max_delta_per_tick: 0,
                },
            ],
        }
    }

    #[test]
    fn every_tick_closes_matter_and_energy() {
        let config = fixture();
        let mut state = MicroState::new(&config).unwrap();
        for _ in 0..100 {
            let report = step(&config, &mut state).unwrap();
            assert!(report.matter_residual.iter().all(|r| *r == 0));
            assert_eq!(report.energy_residual, 0);
        }
    }

    #[test]
    fn competition_is_independent_of_cell_storage_order() {
        let mut config = fixture();
        config.initial_matter[0] = 11;
        config.medium[0].target_amount = 11;
        config.founder.genome.division_mass = 10_000;
        let mut a = MicroState::new(&config).unwrap();
        let mut b = a.clone();
        b.cells.reverse();
        step(&config, &mut a).unwrap();
        step(&config, &mut b).unwrap();
        a.cells.sort_by_key(|cell| cell.id);
        b.cells.sort_by_key(|cell| cell.id);
        assert_eq!(a, b);
    }

    #[test]
    fn division_lineage_is_independent_of_cell_storage_order() {
        let mut config = fixture();
        config.founder.genome.division_mass = 10;
        config.mutation.probability = 1.0;
        let mut a = MicroState::new(&config).unwrap();
        for cell in &mut a.cells {
            cell.mass = 10;
        }
        let mut b = a.clone();
        b.cells.reverse();
        assert_eq!(
            step(&config, &mut a).unwrap(),
            step(&config, &mut b).unwrap()
        );
        assert_eq!(a, b);
    }

    #[test]
    fn capacity_error_is_transactional() {
        let mut config = fixture();
        config.max_cells = config.founder.count;
        config.founder.genome.division_mass = 10;
        let mut state = MicroState::new(&config).unwrap();
        for cell in &mut state.cells {
            cell.mass = 10;
        }
        let before = state.clone();
        let error = step(&config, &mut state).unwrap_err().to_string();
        assert!(error.contains("max_cells"), "{error}");
        assert_eq!(state, before);
    }

    #[test]
    fn fission_only_partitions_parent_matter_and_energy() {
        let mut config = fixture();
        config.initial_matter[0] = 0;
        config.medium[0].target_amount = 0;
        config.founder.genome.division_mass = 10;
        let mut state = MicroState::new(&config).unwrap();
        for cell in &mut state.cells {
            cell.mass = 10;
        }
        let before_mass: i128 = state.cells.iter().map(|cell| cell.mass).sum();
        let before_energy = total_energy(&config, &state).unwrap();
        let report = step(&config, &mut state).unwrap();
        assert_eq!(report.births, 8);
        assert_eq!(
            state.cells.iter().map(|cell| cell.mass).sum::<i128>(),
            before_mass
        );
        assert_eq!(total_energy(&config, &state).unwrap(), before_energy);
        assert!(state.cells.iter().all(|cell| cell.parent_id.is_some()));
    }

    #[test]
    fn death_returns_structure_and_internal_energy() {
        let mut config = fixture();
        config.initial_matter[0] = 0;
        config.medium[0].target_amount = 0;
        config.founder.genome.maintenance_energy_per_tick = 100;
        let mut state = MicroState::new(&config).unwrap();
        let before_energy = total_energy(&config, &state).unwrap();
        let biomass: i128 = state.cells.iter().map(|cell| cell.mass).sum();
        step(&config, &mut state).unwrap();
        let report = step(&config, &mut state).unwrap();
        assert_eq!(report.deaths, config.founder.count);
        assert!(state.cells.is_empty());
        assert_eq!(state.matter[config.death.detritus_substance], biomass);
        assert_eq!(total_energy(&config, &state).unwrap(), before_energy);
    }

    #[test]
    fn snapshot_restores_the_exact_future() {
        let config = fixture();
        let mut original = MicroState::new(&config).unwrap();
        for _ in 0..7 {
            step(&config, &mut original).unwrap();
        }
        let mut restored = MicroState::from_snapshot(&config, original.snapshot()).unwrap();
        for _ in 0..20 {
            assert_eq!(
                step(&config, &mut original).unwrap(),
                step(&config, &mut restored).unwrap()
            );
            assert_eq!(original, restored);
        }
    }

    #[test]
    fn phenotype_tradeoff_reverses_with_resource_abundance() {
        let config = fixture();
        let slow = Cell {
            id: 1,
            parent_id: None,
            genome: Genome {
                kinetics: -2,
                ..genome()
            },
            mass: 100,
            energy: 10,
            age: 0,
            birth_tick: 0,
            generation: 0,
            starvation_ticks: 0,
        };
        let fast = Cell {
            id: 2,
            genome: Genome {
                kinetics: 2,
                ..genome()
            },
            ..slow.clone()
        };
        let mut poor = MicroState {
            tick: 0,
            next_cell_id: 3,
            matter: vec![1, 0, 0],
            heat: 1_000,
            cells: vec![slow.clone(), fast.clone()],
        };
        let mut rich = MicroState {
            matter: vec![100_000, 0, 0],
            ..poor.clone()
        };
        let poor_demands = demands(&config, &poor).unwrap();
        let rich_demands = demands(&config, &rich).unwrap();
        assert!(poor_demands[0] >= poor_demands[1]);
        assert!(rich_demands[1] > rich_demands[0]);
        poor.cells.clear();
        rich.cells.clear();
    }

    #[test]
    fn invalid_snapshot_is_refused() {
        let config = fixture();
        let state = MicroState::new(&config).unwrap();
        let mut snapshot = state.snapshot();
        snapshot.cells[1].id = snapshot.cells[0].id;
        assert!(MicroState::from_snapshot(&config, snapshot).is_err());

        let mut snapshot = state.snapshot();
        snapshot.cells[0].age = 7;
        assert!(MicroState::from_snapshot(&config, snapshot).is_err());

        let mut snapshot = state.snapshot();
        snapshot.cells[0].genome.capture_numerator = 0;
        assert!(MicroState::from_snapshot(&config, snapshot).is_err());
    }
}
