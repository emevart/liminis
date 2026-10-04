//! Configuration boundary for the well-mixed, individual-cell chamber.
//!
//! This is deliberately not an extension of [`crate::config::Config`]. The
//! chamber is a separate experiment and has a separate format identity, while
//! its chemical registry is folded through the existing scale, stoichiometry,
//! mass, and energy derivation. Physical quantities are declared in TOML;
//! integer storage quantities cross the boundary in [`MicroConfig`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::config::{
    self, Boundary, Config, Face, Field, Grid, Initial, Layer, Nu, Physics, Process, Rate,
    Reaction, Substance,
};
use crate::numeric::{Q, run_key};
use crate::process::ProcessId;

pub const CHAMBER_FORMAT_VERSION: u32 = 1;
pub const SPATIAL_CHAMBER_FORMAT_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicroScenario {
    pub chamber_format: u32,
    pub name: String,
    pub dt: f64,
    #[serde(default = "default_extent_fraction")]
    pub extent_fraction: f64,
    pub conserved: BTreeMap<String, f64>,
    pub chamber: Chamber,
    pub growth: GrowthDeclaration,
    pub death: DeathDeclaration,
    pub genome: GenomeProgram,
    pub founder: FounderDeclaration,
    pub initial: MediumComposition,
    pub medium: MediumComposition,
    pub substance: Vec<Substance>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chamber {
    /// Homogeneous liquid volume, m^3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_m3: Option<f64>,
    /// Fixed bath temperature, K. Format 1 does not evolve temperature.
    pub temperature_k: f64,
    pub max_cells: u32,
    /// Independent capped-reservoir actuator, 1/s. Each free pool may move by
    /// at most `max_conc * volume * units_per_mol * rate * dt` per tick.
    pub medium_exchange_per_s: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<SpatialChamber>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialChamber {
    pub dimensions_m: [f64; 3],
    pub viscosity_pa_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialGenome {
    pub radius_at_division_m: f64,
    pub mobility_scale: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialConfig {
    pub dimensions_m: [f64; 3],
    pub viscosity_pa_s: f64,
    pub transport_seed: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediumComposition {
    pub concentration: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrowthDeclaration {
    pub id: String,
    pub enthalpy_j_per_extent: f64,
    pub biomass_substance: String,
    pub limiting_substance: String,
    pub inputs: BTreeMap<String, i32>,
    pub outputs: BTreeMap<String, i32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeathDeclaration {
    pub detritus_substance: String,
}

/// The bounded mutation program. It changes only the signed kinetics locus.
/// Every physiological quantity itself remains in the inherited genome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenomeProgram {
    pub kinetics_min: i8,
    pub kinetics_max: i8,
    pub mutation_probability: f64,
    pub mutation_step: i8,
    pub speed_factor_per_step: f64,
    pub affinity_cost_per_step: f64,
}

/// A complete inherited physiology in physical units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenomeDeclaration {
    pub kinetics: i8,
    /// Maximum fractional structural-mass growth at saturation, 1/s.
    pub max_growth_rate_per_s: f64,
    pub uptake_km_mol_m3: f64,
    pub division_mass_mol: f64,
    pub maintenance_power_w: f64,
    pub capture_numerator: u32,
    pub capture_denominator: u32,
    pub division_energy_j: f64,
    pub starvation_tolerance_s: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<SpatialGenome>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FounderDeclaration {
    pub count: u32,
    pub structural_mass_mol: f64,
    pub internal_energy_j: f64,
    pub genome: GenomeDeclaration,
}

/// Runtime-only integer configuration consumed by the chamber step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicroConfig {
    pub chamber_format: u32,
    pub name: String,
    pub config_hash: String,
    pub dt_seconds: f64,
    pub run_key: u32,
    pub volume_m3: f64,
    pub temperature_kelvin: f64,
    pub max_cells: u32,
    pub matter_ids: Vec<String>,
    pub units_per_mol: Vec<i128>,
    pub initial_matter: Vec<i128>,
    pub chemical_weights: Vec<i64>,
    pub energy_units_per_joule: i128,
    pub initial_bath_heat: i128,
    pub growth: GrowthSpec,
    pub death: DeathSpec,
    pub mutation: MutationSpec,
    pub founder: FounderSpec,
    pub medium: Vec<MediumExchange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<SpatialConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicroNu {
    pub substance: usize,
    pub value: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrowthSpec {
    pub nu: Vec<MicroNu>,
    pub biomass_substance: usize,
    pub limiting_substance: usize,
    pub energy_per_extent: i128,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeathSpec {
    pub detritus_substance: usize,
    /// Heat added per structural storage unit when BIO becomes DET.
    pub chemical_heat_per_mass: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationSpec {
    pub min_kinetics: i8,
    pub max_kinetics: i8,
    pub probability: f64,
    pub speed_factor_per_step: f64,
    pub affinity_cost_per_step: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Genome {
    pub kinetics: i8,
    pub max_growth_rate_per_second: f64,
    pub affinity_km_amount: i128,
    pub division_mass: i128,
    pub maintenance_energy_per_tick: i128,
    pub capture_numerator: u32,
    pub capture_denominator: u32,
    pub division_energy_cost: i128,
    pub starvation_tolerance_ticks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial: Option<SpatialGenome>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FounderSpec {
    pub count: u32,
    pub mass: i128,
    pub energy: i128,
    pub genome: Genome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediumExchange {
    pub target_amount: i128,
    pub max_delta_per_tick: i128,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecodedGenome {
    pub max_growth_rate_per_second: f64,
    pub affinity_km_amount: f64,
    pub division_mass: i128,
    pub maintenance_energy_per_tick: i128,
    pub capture_numerator: u32,
    pub capture_denominator: u32,
    pub division_energy_cost: i128,
    pub starvation_tolerance_ticks: u32,
}

const fn default_extent_fraction() -> f64 {
    1.0 / 64.0
}

pub fn load(path: &Path) -> Result<MicroScenario> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading cell chamber config {}", path.display()))?;
    parse(&text).with_context(|| format!("parsing cell chamber config {}", path.display()))
}

pub fn parse(text: &str) -> Result<MicroScenario> {
    let scenario: MicroScenario = toml::from_str(text)?;
    validate(&scenario)?;
    Ok(scenario)
}

pub fn canonical(scenario: &MicroScenario) -> Result<String> {
    toml::to_string(scenario).context("serializing cell chamber config canonically")
}

pub fn config_hash(scenario: &MicroScenario) -> Result<String> {
    let canonical = canonical(scenario)?;
    let hex = blake3::hash(canonical.as_bytes()).to_hex();
    Ok(format!("blake3:{}", &hex.as_str()[..16]))
}

/// Historical well-mixed entry point. Spatial scenarios require opt-in.
pub fn validate(scenario: &MicroScenario) -> Result<()> {
    ensure!(
        scenario.chamber_format == CHAMBER_FORMAT_VERSION,
        "the legacy cell entry point requires chamber format 1"
    );
    validate_live(scenario)
}

/// Explicit live dispatch; public recording and existing callers stay strict1.
pub fn parse_live(text: &str) -> Result<MicroScenario> {
    let scenario: MicroScenario = toml::from_str(text)?;
    validate_live(&scenario)?;
    Ok(scenario)
}

pub fn validate_live(scenario: &MicroScenario) -> Result<()> {
    chamber_volume(scenario)?;
    validate_spatial_genome(scenario.chamber_format, scenario.founder.genome.spatial)?;
    ensure!(
        !scenario.name.trim().is_empty(),
        "chamber name must not be empty"
    );
    positive_finite("dt", scenario.dt)?;
    ensure!(
        scenario.extent_fraction.is_finite()
            && scenario.extent_fraction > 0.0
            && scenario.extent_fraction <= 1.0,
        "extent_fraction must be finite and in (0, 1]"
    );
    positive_finite("chamber.temperature_k", scenario.chamber.temperature_k)?;
    ensure!(
        scenario.chamber.max_cells > 0,
        "chamber.max_cells must be positive"
    );
    ensure!(
        scenario.chamber.medium_exchange_per_s.is_finite()
            && scenario.chamber.medium_exchange_per_s >= 0.0,
        "chamber.medium_exchange_per_s must be finite and non-negative"
    );
    ensure!(
        scenario.chamber.medium_exchange_per_s * scenario.dt <= 1.0,
        "medium exchange moves more than a whole free pool per tick"
    );
    ensure!(
        !scenario.substance.is_empty(),
        "the chamber needs a chemical registry"
    );

    let mut ids = BTreeSet::new();
    for substance in &scenario.substance {
        ensure!(
            !substance.id.trim().is_empty(),
            "substance id must not be empty"
        );
        ensure!(
            ids.insert(substance.id.as_str()),
            "substance `{}` is declared twice",
            substance.id
        );
        ensure!(
            substance.diffusivity.is_finite() && substance.diffusivity >= 0.0,
            "substance `{}` diffusivity must be finite and non-negative",
            substance.id
        );
    }
    let biomass = named_substance(scenario, &scenario.growth.biomass_substance)?;
    let detritus = named_substance(scenario, &scenario.death.detritus_substance)?;
    ensure!(
        biomass.id != detritus.id,
        "BIO and DET must be different substances"
    );
    ensure!(
        biomass.composition == detritus.composition,
        "BIO and DET must have identical conserved composition for exact lysis"
    );
    ensure!(
        scenario
            .growth
            .inputs
            .contains_key(&scenario.growth.limiting_substance),
        "growth limiting_substance `{}` is not an input",
        scenario.growth.limiting_substance
    );
    ensure!(
        scenario
            .growth
            .outputs
            .get(&biomass.id)
            .copied()
            .unwrap_or(0)
            > 0,
        "growth must produce the structural substance `{}`",
        biomass.id
    );
    ensure!(
        !scenario.growth.inputs.contains_key(&biomass.id),
        "growth cannot consume its structural product `{}`",
        biomass.id
    );
    validate_stoich(&scenario.growth.inputs, &ids, "growth input")?;
    validate_stoich(&scenario.growth.outputs, &ids, "growth output")?;

    validate_composition(&scenario.initial, &ids, "initial")?;
    validate_composition(&scenario.medium, &ids, "medium")?;
    ensure!(
        concentration(&scenario.initial, &biomass.id) == 0.0,
        "initial free BIO must be zero; cells live in the cell table"
    );
    ensure!(
        concentration(&scenario.medium, &biomass.id) == 0.0,
        "medium free BIO must be zero; exchange cannot create cells"
    );

    let program = &scenario.genome;
    ensure!(
        program.kinetics_min <= program.kinetics_max,
        "genome kinetics bounds are reversed"
    );
    ensure!(
        program.mutation_probability.is_finite()
            && (0.0..=1.0).contains(&program.mutation_probability),
        "genome.mutation_probability must be finite and in [0, 1]"
    );
    ensure!(
        program.mutation_step == 1,
        "format 1 mutates the kinetics locus by exactly one step"
    );
    ensure!(
        program.speed_factor_per_step.is_finite() && program.speed_factor_per_step > 1.0,
        "genome.speed_factor_per_step must be finite and greater than one for the speed/affinity tradeoff"
    );
    ensure!(
        program.affinity_cost_per_step.is_finite()
            && program.affinity_cost_per_step > program.speed_factor_per_step,
        "genome.affinity_cost_per_step must be finite and greater than speed_factor_per_step for the speed/affinity tradeoff"
    );

    let founder = &scenario.founder;
    ensure!(
        founder.count > 0 && founder.count <= scenario.chamber.max_cells,
        "founder count must be in 1..=chamber.max_cells"
    );
    positive_finite("founder.structural_mass_mol", founder.structural_mass_mol)?;
    ensure!(
        founder.internal_energy_j.is_finite() && founder.internal_energy_j >= 0.0,
        "founder.internal_energy_j must be finite and non-negative"
    );
    validate_genome(&founder.genome, program)?;
    ensure!(
        founder.structural_mass_mol < founder.genome.division_mass_mol,
        "founder structural mass must start below its inherited division threshold"
    );
    Ok(())
}

pub fn derive(scenario: &MicroScenario, seed: u64) -> Result<MicroConfig> {
    validate(scenario)?;
    derive_checked(scenario, seed)
}

pub fn derive_spatial(scenario: &MicroScenario, seed: u64) -> Result<MicroConfig> {
    ensure!(
        scenario.chamber_format == SPATIAL_CHAMBER_FORMAT_VERSION,
        "spatial derivation requires chamber format 2"
    );
    validate_live(scenario)?;
    derive_checked(scenario, seed)
}

pub fn derive_live(scenario: &MicroScenario, seed: u64) -> Result<MicroConfig> {
    match scenario.chamber_format {
        CHAMBER_FORMAT_VERSION => derive(scenario, seed),
        SPATIAL_CHAMBER_FORMAT_VERSION => derive_spatial(scenario, seed),
        _ => anyhow::bail!("unsupported chamber format"),
    }
}

fn derive_checked(scenario: &MicroScenario, seed: u64) -> Result<MicroConfig> {
    let volume_m3 = chamber_volume(scenario)?;
    let chemistry = chemistry_config(scenario, volume_m3)?;
    config::validate(&chemistry).context("validating chamber chemistry")?;
    let derived = config::derive(&chemistry).context("deriving chamber chemistry")?;
    let growth = derived
        .reactions()
        .first()
        .context("growth reaction was not derived")?;
    ensure!(
        growth.nu_energy > 0,
        "growth must release positive chemical energy, got {}",
        growth.nu_energy
    );

    let ids: Vec<String> = derived.substances().iter().map(|s| s.id.clone()).collect();
    let biomass = index_of(&ids, &scenario.growth.biomass_substance)?;
    let detritus = index_of(&ids, &scenario.death.detritus_substance)?;
    let limiting = index_of(&ids, &scenario.growth.limiting_substance)?;
    ensure!(
        derived.substances()[biomass].k == derived.substances()[detritus].k,
        "BIO and DET must derive the same storage scale (got k={} and k={})",
        derived.substances()[biomass].k,
        derived.substances()[detritus].k
    );

    let units_per_mol: Vec<i128> = derived
        .substances()
        .iter()
        .map(|s| {
            1_i128
                .checked_shl(u32::from(s.k))
                .context("matter scale does not fit i128")
        })
        .collect::<Result<_>>()?;
    let energy_units_per_joule = 1_i128
        .checked_shl(u32::from(derived.energy().k_e))
        .context("energy scale does not fit i128")?;
    let initial_matter = fold_composition(&scenario.initial, &ids, &units_per_mol, volume_m3)?;
    let medium_matter = fold_composition(&scenario.medium, &ids, &units_per_mol, volume_m3)?;
    let genome = derive_genome(
        scenario,
        biomass,
        limiting,
        &units_per_mol,
        energy_units_per_joule,
        volume_m3,
    )?;
    let structural_mass = physical_to_int(
        "founder structural mass",
        scenario.founder.structural_mass_mol * units_per_mol[biomass] as f64,
    )?;
    ensure!(
        structural_mass > 0,
        "founder structural mass rounds to zero storage units"
    );
    let internal_energy = physical_to_int(
        "founder internal energy",
        scenario.founder.internal_energy_j * energy_units_per_joule as f64,
    )?;
    let chemical_weights = derived.chemical_weights();
    ensure!(
        chemical_weights[biomass] >= chemical_weights[detritus],
        "format-1 lysis must be non-endothermic: BIO weight {} is below DET weight {}",
        chemical_weights[biomass],
        chemical_weights[detritus]
    );
    let stoich = growth.nu.iter().map(micro_nu).collect::<Result<Vec<_>>>()?;

    let config = MicroConfig {
        chamber_format: scenario.chamber_format,
        name: scenario.name.clone(),
        config_hash: config_hash(scenario)?,
        dt_seconds: scenario.dt,
        run_key: run_key(seed),
        volume_m3,
        temperature_kelvin: scenario.chamber.temperature_k,
        max_cells: scenario.chamber.max_cells,
        matter_ids: ids,
        units_per_mol,
        initial_matter,
        chemical_weights: chemical_weights.clone(),
        energy_units_per_joule,
        initial_bath_heat: 0,
        growth: GrowthSpec {
            nu: stoich,
            biomass_substance: biomass,
            limiting_substance: limiting,
            energy_per_extent: i128::from(growth.nu_energy),
        },
        death: DeathSpec {
            detritus_substance: detritus,
            chemical_heat_per_mass: chemical_weights[biomass] - chemical_weights[detritus],
        },
        mutation: MutationSpec {
            min_kinetics: scenario.genome.kinetics_min,
            max_kinetics: scenario.genome.kinetics_max,
            probability: scenario.genome.mutation_probability,
            speed_factor_per_step: scenario.genome.speed_factor_per_step,
            affinity_cost_per_step: scenario.genome.affinity_cost_per_step,
        },
        founder: FounderSpec {
            count: scenario.founder.count,
            mass: structural_mass,
            energy: internal_energy,
            genome,
        },
        medium: medium_matter
            .into_iter()
            .zip(derived.substances())
            .map(|(target_amount, substance)| MediumExchange {
                target_amount,
                max_delta_per_tick: ((substance.amount_at_max as f64)
                    * scenario.chamber.medium_exchange_per_s
                    * scenario.dt)
                    .ceil() as i128,
            })
            .collect(),
        spatial: scenario.chamber.spatial.map(|spatial| SpatialConfig {
            dimensions_m: spatial.dimensions_m,
            viscosity_pa_s: spatial.viscosity_pa_s,
            transport_seed: seed,
        }),
    };
    validate_runtime(&config)?;
    Ok(config)
}

/// Decode the one mutable locus without inventing phenotype in the observer.
#[must_use]
pub fn decode_genome(config: &MicroConfig, genome: &Genome) -> DecodedGenome {
    let step = i32::from(genome.kinetics);
    DecodedGenome {
        max_growth_rate_per_second: genome.max_growth_rate_per_second
            * config.mutation.speed_factor_per_step.powi(step),
        affinity_km_amount: genome.affinity_km_amount as f64
            * config.mutation.affinity_cost_per_step.powi(step),
        division_mass: genome.division_mass,
        maintenance_energy_per_tick: genome.maintenance_energy_per_tick,
        capture_numerator: genome.capture_numerator,
        capture_denominator: genome.capture_denominator,
        division_energy_cost: genome.division_energy_cost,
        starvation_tolerance_ticks: genome.starvation_tolerance_ticks,
    }
}

/// Recheck every structural invariant after deserializing a runtime snapshot.
/// This validates configuration, not the evolving cumulative bath heat held by
/// `MicroState`; format 1 separately requires non-endothermic lysis.
pub fn validate_runtime(config: &MicroConfig) -> Result<()> {
    match (config.chamber_format, config.spatial) {
        (CHAMBER_FORMAT_VERSION, None) => {}
        (SPATIAL_CHAMBER_FORMAT_VERSION, Some(spatial)) => {
            let volume = spatial_volume(spatial.dimensions_m, spatial.viscosity_pa_s)?;
            ensure!(
                volume == config.volume_m3,
                "runtime volume differs from spatial dimensions"
            );
        }
        _ => anyhow::bail!("runtime chamber format and spatial configuration disagree"),
    }
    positive_finite("runtime dt_seconds", config.dt_seconds)?;
    positive_finite("runtime volume_m3", config.volume_m3)?;
    positive_finite("runtime temperature_kelvin", config.temperature_kelvin)?;
    ensure!(config.max_cells > 0, "runtime max_cells must be positive");
    let n = config.matter_ids.len();
    ensure!(n > 0, "runtime matter registry is empty");
    ensure!(
        config.units_per_mol.len() == n,
        "runtime matter scale length mismatch"
    );
    ensure!(
        config.initial_matter.len() == n,
        "runtime initial matter length mismatch"
    );
    ensure!(
        config.chemical_weights.len() == n,
        "runtime chemical weight length mismatch"
    );
    ensure!(config.medium.len() == n, "runtime medium length mismatch");
    ensure!(
        config.energy_units_per_joule > 0,
        "runtime energy scale must be positive"
    );
    let mut names = BTreeSet::new();
    for (index, id) in config.matter_ids.iter().enumerate() {
        ensure!(
            !id.is_empty() && names.insert(id),
            "runtime matter ids must be non-empty and unique"
        );
        ensure!(
            config.units_per_mol[index] > 0,
            "runtime matter scale must be positive"
        );
        ensure!(
            config.initial_matter[index] >= 0,
            "runtime initial matter must be non-negative"
        );
        ensure!(
            config.medium[index].target_amount >= 0 && config.medium[index].max_delta_per_tick >= 0,
            "runtime medium exchange must be non-negative"
        );
    }
    ensure!(
        config.growth.biomass_substance < n,
        "runtime biomass index is out of range"
    );
    ensure!(
        config.growth.limiting_substance < n,
        "runtime limiting-substance index is out of range"
    );
    ensure!(
        config.death.detritus_substance < n,
        "runtime detritus index is out of range"
    );
    ensure!(
        config.growth.biomass_substance != config.death.detritus_substance,
        "runtime BIO and DET indexes must differ"
    );
    ensure!(
        config.units_per_mol[config.growth.biomass_substance]
            == config.units_per_mol[config.death.detritus_substance],
        "runtime BIO and DET scales must match"
    );
    ensure!(
        config.death.chemical_heat_per_mass >= 0,
        "runtime lysis must be non-endothermic"
    );
    let mut energy_identity = config.growth.energy_per_extent;
    let mut biomass_nu = 0_i64;
    for nu in &config.growth.nu {
        ensure!(
            nu.substance < n,
            "runtime growth stoichiometry index is out of range"
        );
        ensure!(
            nu.value != 0,
            "runtime growth stoichiometry contains a zero row"
        );
        energy_identity = energy_identity
            .checked_add(i128::from(nu.value) * i128::from(config.chemical_weights[nu.substance]))
            .context("runtime growth energy identity overflows")?;
        if nu.substance == config.growth.biomass_substance {
            biomass_nu = biomass_nu
                .checked_add(nu.value)
                .context("runtime BIO coefficient overflows")?;
        }
    }
    ensure!(biomass_nu > 0, "runtime growth must produce BIO");
    ensure!(
        energy_identity == 0,
        "runtime growth violates the exact chemical-energy identity by {energy_identity}"
    );
    ensure!(
        config.mutation.min_kinetics <= config.mutation.max_kinetics,
        "runtime mutation bounds are reversed"
    );
    ensure!(
        config.mutation.probability.is_finite()
            && (0.0..=1.0).contains(&config.mutation.probability),
        "runtime mutation probability must be in [0, 1]"
    );
    ensure!(
        config.mutation.speed_factor_per_step.is_finite()
            && config.mutation.speed_factor_per_step > 1.0,
        "runtime speed factor must be finite and greater than one for the speed/affinity tradeoff"
    );
    ensure!(
        config.mutation.affinity_cost_per_step.is_finite()
            && config.mutation.affinity_cost_per_step > config.mutation.speed_factor_per_step,
        "runtime affinity cost must be finite and greater than the speed factor for the speed/affinity tradeoff"
    );
    ensure!(
        config.founder.count > 0 && config.founder.count <= config.max_cells,
        "runtime founder count is outside capacity"
    );
    ensure!(
        config.founder.mass > 0 && config.founder.energy >= 0,
        "runtime founder mass must be positive and energy non-negative"
    );
    validate_runtime_genome(config, &config.founder.genome)?;
    for kinetics in [config.mutation.min_kinetics, config.mutation.max_kinetics] {
        let genome = Genome {
            kinetics,
            ..config.founder.genome
        };
        let phenotype = decode_genome(config, &genome);
        positive_finite(
            "runtime decoded growth rate",
            phenotype.max_growth_rate_per_second,
        )?;
        positive_finite("runtime decoded affinity Km", phenotype.affinity_km_amount)?;
        positive_finite(
            "runtime Q-folded affinity Km",
            Q::from_f64(phenotype.affinity_km_amount).debug_f64(),
        )?;
    }
    ensure!(
        config.founder.mass < config.founder.genome.division_mass,
        "runtime founder starts at or above its division threshold"
    );
    Ok(())
}

fn validate_runtime_genome(config: &MicroConfig, genome: &Genome) -> Result<()> {
    validate_spatial_genome(config.chamber_format, genome.spatial)?;
    ensure!(
        (config.mutation.min_kinetics..=config.mutation.max_kinetics).contains(&genome.kinetics),
        "runtime genome kinetics is outside mutation bounds"
    );
    positive_finite("runtime max growth rate", genome.max_growth_rate_per_second)?;
    ensure!(
        genome.affinity_km_amount > 0 && genome.division_mass > 0,
        "runtime genome Km and division mass must be positive"
    );
    ensure!(
        genome.maintenance_energy_per_tick >= 0 && genome.division_energy_cost >= 0,
        "runtime genome energy costs must be non-negative"
    );
    ensure!(
        genome.capture_denominator > 0 && genome.capture_numerator <= genome.capture_denominator,
        "runtime capture ratio is invalid"
    );
    ensure!(
        genome.starvation_tolerance_ticks > 0,
        "runtime starvation tolerance must be positive"
    );
    Ok(())
}

/// Resolve the declaration without adding a second volume source to format2.
fn chamber_volume(scenario: &MicroScenario) -> Result<f64> {
    match (
        scenario.chamber_format,
        scenario.chamber.volume_m3,
        scenario.chamber.spatial,
    ) {
        (CHAMBER_FORMAT_VERSION, Some(volume), None) => {
            positive_finite("chamber.volume_m3", volume)?;
            Ok(volume)
        }
        (SPATIAL_CHAMBER_FORMAT_VERSION, None, Some(spatial)) => {
            spatial_volume(spatial.dimensions_m, spatial.viscosity_pa_s)
        }
        _ => anyhow::bail!(
            "chamber format 1 requires volume and no spatial declaration; format 2 requires spatial dimensions and forbids volume"
        ),
    }
}

fn spatial_volume(dimensions_m: [f64; 3], viscosity_pa_s: f64) -> Result<f64> {
    for dimension in dimensions_m {
        positive_finite("spatial dimension", dimension)?;
        positive_finite("spatial reflection period 2L", 2.0 * dimension)?;
    }
    positive_finite("spatial viscosity_pa_s", viscosity_pa_s)?;
    let volume = (dimensions_m[0] * dimensions_m[1]) * dimensions_m[2];
    positive_finite("spatial dimensions volume product", volume)?;
    Ok(volume)
}

fn validate_spatial_genome(format: u32, genome: Option<SpatialGenome>) -> Result<()> {
    match (format, genome) {
        (CHAMBER_FORMAT_VERSION, None) => Ok(()),
        (SPATIAL_CHAMBER_FORMAT_VERSION, Some(spatial)) => {
            positive_finite("spatial radius_at_division_m", spatial.radius_at_division_m)?;
            ensure!(
                spatial.mobility_scale.is_finite() && (0.0..=1.0).contains(&spatial.mobility_scale),
                "spatial mobility_scale must be finite and in [0, 1]"
            );
            Ok(())
        }
        _ => anyhow::bail!(
            "spatial genome must be absent in chamber format 1 and present in format 2"
        ),
    }
}

fn chemistry_config(scenario: &MicroScenario, volume_m3: f64) -> Result<Config> {
    let mut km = BTreeMap::new();
    for id in scenario.growth.inputs.keys() {
        km.insert(id.clone(), scenario.founder.genome.uptake_km_mol_m3);
    }
    let reaction = Reaction {
        id: scenario.growth.id.clone(),
        enthalpy: scenario.growth.enthalpy_j_per_extent,
        catalyst: String::new(),
        energy_from: String::new(),
        inputs: scenario.growth.inputs.clone(),
        outputs: scenario.growth.outputs.clone(),
        requires: Vec::new(),
        rate: Rate {
            vmax: 1.0,
            t_vmax: scenario.chamber.temperature_k,
            q10: 1.0,
            km,
        },
    };
    let off = ProcessId::ALL
        .into_iter()
        .map(|id| Process {
            id: id.id().to_string(),
            enabled: Some(false),
            every_n_ticks: 1,
            u_conv_max: None,
            l_c: None,
            stir_period: None,
            stir_fraction: 0.0,
            k_w: None,
            k_b: None,
            k_d: None,
            k_m: None,
            i_surface: None,
            daily_fraction: 0.0,
            daily_period: None,
            seasonal_fraction: 0.0,
            seasonal_period: None,
            theta_max: None,
        })
        .collect();
    let closed = Boundary {
        x_min: Face::Closed,
        x_max: Face::Closed,
        y_min: Face::Closed,
        y_max: Face::Closed,
        z_min: Face::Closed,
        z_max: Face::Closed,
        reservoir: None,
    };
    let mut substances = scenario.substance.clone();
    for substance in &mut substances {
        // Transport is absent in the homogeneous chamber. Retaining the
        // registry's documentary diffusivities in this one-cell projection
        // would make the spatial validator constrain an operator that does not
        // exist; scales, stoichiometry, heat capacities, and chemistry remain
        // unchanged.
        substance.diffusivity = 0.0;
    }
    let mut config = Config {
        name: format!("{} chemistry", scenario.name),
        dt: scenario.dt,
        beta: scenario.extent_fraction,
        t_ref: scenario.chamber.temperature_k,
        conserved: scenario.conserved.clone(),
        grid: Grid {
            nx: 1,
            ny: 1,
            nz: 1,
            dx: volume_m3.cbrt(),
        },
        boundary: closed,
        physics: Physics::default(),
        substance: substances,
        reaction: vec![reaction],
        field: vec![Field {
            id: "enthalpy".to_string(),
            lod: 0,
            // A derivation-only one-cell registry. The existing scale
            // validator requires a positive value, while the chamber evolves
            // no spatial thermal field. This bounded shim costs one imaginary
            // substep in validation and is never used by the chamber runtime.
            thermal_diffusivity: Some(1.0e-12),
            t_min: Some(scenario.chamber.temperature_k - 10.0),
            t_max: Some(scenario.chamber.temperature_k + 10.0),
        }],
        process: off,
        initial: Initial {
            layer: scenario
                .substance
                .iter()
                .map(|s| (s.id.clone(), Layer::Water))
                .collect(),
            concentration: scenario.initial.concentration.clone(),
            inoculum: Vec::new(),
        },
        genetics: None,
        calibration: Vec::new(),
    };
    config::materialise(&mut config)?;
    Ok(config)
}

fn derive_genome(
    scenario: &MicroScenario,
    biomass: usize,
    limiting: usize,
    units_per_mol: &[i128],
    energy_units_per_joule: i128,
    volume_m3: f64,
) -> Result<Genome> {
    let source = &scenario.founder.genome;
    let division_mass = physical_to_int(
        "genome division mass",
        source.division_mass_mol * units_per_mol[biomass] as f64,
    )?;
    let uptake_km_amount = physical_to_int(
        "genome uptake Km",
        source.uptake_km_mol_m3 * volume_m3 * units_per_mol[limiting] as f64,
    )?;
    ensure!(
        uptake_km_amount > 0,
        "genome uptake Km rounds to zero storage units"
    );
    let division_energy = physical_to_int(
        "genome division energy",
        source.division_energy_j * energy_units_per_joule as f64,
    )?;
    ensure!(
        source.division_energy_j == 0.0 || division_energy > 0,
        "positive genome division energy rounds to zero storage units"
    );
    let maintenance_energy = physical_to_int(
        "genome maintenance energy",
        source.maintenance_power_w * scenario.dt * energy_units_per_joule as f64,
    )?;
    ensure!(
        source.maintenance_power_w == 0.0 || maintenance_energy > 0,
        "positive genome maintenance energy rounds to zero storage units"
    );
    let starvation_tolerance_ticks = (source.starvation_tolerance_s / scenario.dt).ceil();
    ensure!(
        starvation_tolerance_ticks.is_finite()
            && starvation_tolerance_ticks >= 1.0
            && starvation_tolerance_ticks <= f64::from(u32::MAX),
        "genome starvation tolerance is outside the tick range"
    );
    Ok(Genome {
        kinetics: source.kinetics,
        max_growth_rate_per_second: source.max_growth_rate_per_s,
        affinity_km_amount: uptake_km_amount,
        division_mass,
        maintenance_energy_per_tick: maintenance_energy,
        capture_numerator: source.capture_numerator,
        capture_denominator: source.capture_denominator,
        division_energy_cost: division_energy,
        starvation_tolerance_ticks: starvation_tolerance_ticks as u32,
        spatial: source.spatial,
    })
}

fn fold_composition(
    composition: &MediumComposition,
    ids: &[String],
    units_per_mol: &[i128],
    volume_m3: f64,
) -> Result<Vec<i128>> {
    ids.iter()
        .zip(units_per_mol)
        .map(|(id, units)| {
            physical_to_int(
                &format!("concentration of {id}"),
                concentration(composition, id) * volume_m3 * *units as f64,
            )
        })
        .collect()
}

fn validate_genome(genome: &GenomeDeclaration, program: &GenomeProgram) -> Result<()> {
    ensure!(
        (program.kinetics_min..=program.kinetics_max).contains(&genome.kinetics),
        "founder genome kinetics lies outside the declared bounds"
    );
    positive_finite("genome.max_growth_rate_per_s", genome.max_growth_rate_per_s)?;
    positive_finite("genome.uptake_km_mol_m3", genome.uptake_km_mol_m3)?;
    positive_finite("genome.division_mass_mol", genome.division_mass_mol)?;
    ensure!(
        genome.maintenance_power_w.is_finite() && genome.maintenance_power_w >= 0.0,
        "genome.maintenance_power_w must be finite and non-negative"
    );
    ensure!(
        genome.capture_denominator > 0 && genome.capture_numerator <= genome.capture_denominator,
        "genome capture ratio must satisfy numerator <= denominator and denominator > 0"
    );
    ensure!(
        genome.division_energy_j.is_finite() && genome.division_energy_j >= 0.0,
        "genome.division_energy_j must be finite and non-negative"
    );
    positive_finite(
        "genome.starvation_tolerance_s",
        genome.starvation_tolerance_s,
    )
}

fn validate_composition(
    composition: &MediumComposition,
    ids: &BTreeSet<&str>,
    label: &str,
) -> Result<()> {
    for (id, value) in &composition.concentration {
        ensure!(
            ids.contains(id.as_str()),
            "{label} concentration names unknown substance `{id}`"
        );
        ensure!(
            value.is_finite() && *value >= 0.0,
            "{label} concentration of `{id}` must be finite and non-negative"
        );
    }
    Ok(())
}

fn validate_stoich(side: &BTreeMap<String, i32>, ids: &BTreeSet<&str>, label: &str) -> Result<()> {
    ensure!(!side.is_empty(), "{label} table must not be empty");
    for (id, coefficient) in side {
        ensure!(
            ids.contains(id.as_str()),
            "{label} names unknown substance `{id}`"
        );
        ensure!(
            *coefficient > 0,
            "{label} coefficient for `{id}` must be positive"
        );
    }
    Ok(())
}

fn named_substance<'a>(scenario: &'a MicroScenario, id: &str) -> Result<&'a Substance> {
    scenario
        .substance
        .iter()
        .find(|s| s.id == id)
        .with_context(|| format!("unknown substance `{id}`"))
}

fn index_of(ids: &[String], id: &str) -> Result<usize> {
    ids.iter()
        .position(|candidate| candidate == id)
        .with_context(|| format!("unknown substance `{id}`"))
}

fn concentration(composition: &MediumComposition, id: &str) -> f64 {
    composition.concentration.get(id).copied().unwrap_or(0.0)
}

fn micro_nu(nu: &Nu) -> Result<MicroNu> {
    Ok(MicroNu {
        substance: usize::try_from(nu.substance).context("substance index does not fit usize")?,
        value: nu.value,
    })
}

fn positive_finite(name: &str, value: f64) -> Result<()> {
    ensure!(
        value.is_finite() && value > 0.0,
        "{name} must be finite and positive"
    );
    Ok(())
}

fn physical_to_int(name: &str, value: f64) -> Result<i128> {
    ensure!(
        value.is_finite() && value >= 0.0 && value <= i128::MAX as f64,
        "{name} is outside the integer storage range"
    );
    Ok(value.round() as i128)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAMBER: &str = include_str!("../../../../configs/scenarios/cell-chamber.toml");

    #[test]
    fn shipped_cell_chamber_derives_exact_runtime_units() {
        let scenario = parse(CHAMBER).expect("shipped chamber parses");
        let config = derive(&scenario, 42).expect("shipped chamber derives");
        validate_runtime(&config).expect("derived runtime config validates");

        assert_eq!(config.chamber_format, CHAMBER_FORMAT_VERSION);
        assert_eq!(config.matter_ids.len(), config.units_per_mol.len());
        assert_eq!(config.matter_ids.len(), config.medium.len());
        assert!(config.growth.energy_per_extent > 0);
        assert!(config.founder.mass > 0);
        assert!(config.founder.mass < config.founder.genome.division_mass);
        assert_eq!(
            config.units_per_mol[config.growth.biomass_substance],
            config.units_per_mol[config.death.detritus_substance]
        );
    }

    #[test]
    fn canonical_chamber_round_trip_keeps_its_separate_hash() {
        let scenario = parse(CHAMBER).expect("shipped chamber parses");
        let text = canonical(&scenario).expect("canonical form");
        let reloaded = parse(&text).expect("canonical form reloads");
        assert_eq!(
            config_hash(&scenario).unwrap(),
            config_hash(&reloaded).unwrap()
        );
        assert!(
            config::parse(&text).is_err(),
            "micro format must not enter eco loader"
        );
    }

    #[test]
    fn kinetics_program_requires_a_real_speed_affinity_tradeoff() {
        let scenario = parse(CHAMBER).expect("shipped chamber parses");
        let config = derive(&scenario, 42).expect("shipped chamber derives");
        for (speed, affinity) in [(1.0, 1.8), (1.35, 1.0), (1.35, 1.2), (1.35, 1.35)] {
            let mut invalid_scenario = scenario.clone();
            invalid_scenario.genome.speed_factor_per_step = speed;
            invalid_scenario.genome.affinity_cost_per_step = affinity;
            let error = validate(&invalid_scenario)
                .expect_err("unpriced faster growth must be rejected")
                .to_string();
            assert!(error.contains("tradeoff"), "{error}");

            let mut invalid_runtime = config.clone();
            invalid_runtime.mutation.speed_factor_per_step = speed;
            invalid_runtime.mutation.affinity_cost_per_step = affinity;
            let error = validate_runtime(&invalid_runtime)
                .expect_err("runtime must retain the same tradeoff guard")
                .to_string();
            assert!(error.contains("tradeoff"), "{error}");
        }
    }

    #[test]
    fn positive_declared_energy_costs_cannot_round_to_free_upkeep_or_fission() {
        let scenario = parse(CHAMBER).unwrap();
        let mut invalid = scenario.clone();
        invalid.founder.genome.maintenance_power_w = 1.0e-100;
        let error = derive(&invalid, 42).unwrap_err().to_string();
        assert!(
            error.contains("maintenance energy rounds to zero"),
            "{error}"
        );

        let mut invalid = scenario.clone();
        invalid.founder.genome.division_energy_j = 1.0e-100;
        let error = derive(&invalid, 42).unwrap_err().to_string();
        assert!(error.contains("division energy rounds to zero"), "{error}");

        let mut explicit_zero = scenario;
        explicit_zero.founder.genome.maintenance_power_w = 0.0;
        explicit_zero.founder.genome.division_energy_j = 0.0;
        let config = derive(&explicit_zero, 42).unwrap();
        assert_eq!(config.founder.genome.maintenance_energy_per_tick, 0);
        assert_eq!(config.founder.genome.division_energy_cost, 0);
    }

    #[test]
    fn all_mutable_endpoint_phenotypes_must_be_positive_and_representable() {
        let scenario = parse(CHAMBER).unwrap();
        let config = derive(&scenario, 42).unwrap();
        for (speed, affinity, min, max) in [
            (1.0e99, 1.0e100, -127, 4),
            (1.0e99, 1.0e100, -4, 127),
            (2.0, 1.0e10, -4, 4),
            (2.0, 1.0e30, -4, 0),
        ] {
            let mut invalid = config.clone();
            invalid.mutation.speed_factor_per_step = speed;
            invalid.mutation.affinity_cost_per_step = affinity;
            invalid.mutation.min_kinetics = min;
            invalid.mutation.max_kinetics = max;
            let error = validate_runtime(&invalid).unwrap_err().to_string();
            assert!(
                error.contains("decoded") || error.contains("Q-folded"),
                "{error}"
            );
        }
    }

    #[test]
    fn free_structural_biomass_is_rejected() {
        let text = CHAMBER.replacen("BIO = 0.0\n", "BIO = 0.1\n", 1);
        let error = parse(&text).expect_err("free BIO must fail").to_string();
        assert!(error.contains("initial free BIO"), "{error}");
    }

    #[test]
    fn lysis_requires_matching_bio_and_det_composition() {
        let text = CHAMBER.replacen(
            "id = \"DET\"\ncomposition = { C = 1, H = 2, O = 1 }",
            "id = \"DET\"\ncomposition = { C = 1, H = 1, O = 1 }",
            1,
        );
        let error = parse(&text)
            .expect_err("composition mismatch must fail")
            .to_string();
        assert!(error.contains("identical conserved composition"), "{error}");
    }

    #[test]
    fn runtime_restore_rechecks_the_energy_identity() {
        let scenario = parse(CHAMBER).expect("shipped chamber parses");
        let mut config = derive(&scenario, 42).expect("shipped chamber derives");
        config.growth.energy_per_extent += 1;
        let error = validate_runtime(&config)
            .expect_err("energy corruption must fail")
            .to_string();
        assert!(error.contains("energy identity"), "{error}");
    }

    #[test]
    #[ignore = "explicit 10k-step shipped-scenario calibration soak"]
    fn shipped_cell_chamber_sustains_real_turnover_for_10k_steps() {
        let scenario = parse(CHAMBER).expect("shipped chamber parses");
        let config = derive(&scenario, 42).expect("shipped chamber derives");
        let mut state = crate::micro::MicroState::new(&config).expect("initial chamber");
        let mut births = 0_u64;
        let mut deaths = 0_u64;
        let mut maximum = state.cells.len();
        let mut warm_minimum = usize::MAX;
        let mut observed_kinetics = BTreeSet::from([0_i8]);
        for step in 0..10_000 {
            let report = crate::micro::step(&config, &mut state).expect("chamber step");
            assert_eq!(report.matter_residual, vec![0; config.matter_ids.len()]);
            assert_eq!(report.energy_residual, 0);
            births += u64::from(report.births);
            deaths += u64::from(report.deaths);
            maximum = maximum.max(state.cells.len());
            observed_kinetics.extend(state.cells.iter().map(|cell| cell.genome.kinetics));
            if step >= 2_000 {
                warm_minimum = warm_minimum.min(state.cells.len());
            }
        }
        let kinetics: BTreeSet<i8> = state
            .cells
            .iter()
            .map(|cell| cell.genome.kinetics)
            .collect();
        println!(
            "cell-chamber 10k: final={}, min_after_2k={}, max={}, births={}, deaths={}, observed_kinetics={observed_kinetics:?}, final_kinetics={kinetics:?}",
            state.cells.len(),
            warm_minimum,
            maximum,
            births,
            deaths
        );
        assert!(
            births > 0 && deaths > 0,
            "the shipped culture must divide and die"
        );
        assert!(
            (20..=250).contains(&state.cells.len()),
            "final culture is outside the calibrated observational range"
        );
        assert!(
            (10..=300).contains(&warm_minimum),
            "the mature culture leaves its calibrated range"
        );
        assert!(
            maximum < config.max_cells as usize,
            "capacity must not regulate biology"
        );
        assert!(
            observed_kinetics.len() > 1,
            "the mutating run must produce inherited variation"
        );
    }
}

#[cfg(test)]
#[path = "config_spatial_tests.rs"]
mod spatial_tests;
