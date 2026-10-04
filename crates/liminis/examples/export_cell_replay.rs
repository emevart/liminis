//! Produce a public observation recording, never a resumable checkpoint.
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use liminis_core::micro::{self, MicroConfig, MicroState};
use liminis_core::version::WORLD_FORMAT_VERSION;
use serde_json::{Value, json};

const PROVENANCE_INPUTS: [&str; 6] = [
    "crates/liminis-core",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "crates/liminis/Cargo.toml",
    "crates/liminis/examples/export_cell_replay.rs",
];

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "configs/scenarios/cell-chamber.toml")]
    config: PathBuf,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    #[arg(long, default_value_t = 10_000)]
    steps: u64,
    #[arg(long, default_value_t = 50)]
    sample_every: u64,
    /// Full Git commit identifying the unchanged engine used for this run.
    #[arg(long)]
    source_commit: String,
    #[arg(long)]
    output: PathBuf,
}

#[derive(Default)]
struct Counters {
    births: u64,
    deaths: u64,
    divisions: u64,
    growth_extent: i128,
    death_mass: i128,
    medium_energy: i128,
    medium_matter: Vec<i128>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    verify_engine_source(&args.source_commit)?;
    let text = fs::read_to_string(&args.config).context("read chamber scenario")?;
    let recording = record(&text, &args)?;
    let bytes = serde_json::to_vec(&recording)?;
    ensure!(bytes.len() <= 64 * 1024 * 1024, "recording exceeds 64 MiB");
    if let Some(parent) = args
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, &bytes)?;
    println!(
        "{} frames, {} checked ticks, {} bytes, blake3:{}",
        recording["frames"].as_array().unwrap().len(),
        args.steps,
        bytes.len(),
        blake3::hash(&bytes).to_hex()
    );
    Ok(())
}

fn verify_engine_source(commit: &str) -> Result<()> {
    let root = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    ensure!(
        root.status.success(),
        "export requires the source Git checkout"
    );
    let root = PathBuf::from(String::from_utf8(root.stdout)?.trim());
    let head = Command::new("git")
        .current_dir(&root)
        .args(["rev-parse", "HEAD"])
        .output()?;
    ensure!(head.status.success(), "cannot inspect source HEAD");
    let clean = Command::new("git")
        .current_dir(&root)
        .args(["diff", "--quiet", "HEAD", "--"])
        .args(PROVENANCE_INPUTS)
        .status()?;
    let untracked = Command::new("git")
        .current_dir(&root)
        .args(["ls-files", "--others", "--exclude-standard", "--"])
        .args(PROVENANCE_INPUTS)
        .output()?;
    ensure!(
        untracked.status.success(),
        "cannot inspect untracked engine sources"
    );
    validate_source_evidence(
        commit,
        String::from_utf8(head.stdout)?.trim(),
        clean.success(),
        untracked.stdout.is_empty(),
    )
}

fn validate_source_evidence(
    commit: &str,
    head: &str,
    clean: bool,
    no_untracked: bool,
) -> Result<()> {
    ensure!(
        head.eq_ignore_ascii_case(commit),
        "source-commit differs from the checkout HEAD"
    );
    ensure!(
        clean,
        "engine source differs from the declared commit; commit it before export"
    );
    ensure!(no_untracked, "engine has untracked source files");
    Ok(())
}

fn record(text: &str, args: &Args) -> Result<Value> {
    ensure!(
        args.source_commit.len() == 40 && args.source_commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "source-commit must be a full 40-character Git commit"
    );
    ensure!(
        (1..=1_000_000).contains(&args.steps),
        "steps must be 1..=1000000"
    );
    ensure!(args.sample_every > 0, "sample-every must be positive");
    ensure!(
        args.steps.div_ceil(args.sample_every) <= 2_000,
        "at most 2001 frames are allowed"
    );
    let scenario = micro::config::parse(text)?;
    let canonical = micro::config::canonical(&scenario)?;
    let config = micro::config::derive(&scenario, args.seed)?;
    let mut state = MicroState::new(&config)?;
    let mut counters = Counters {
        medium_matter: vec![0; config.matter_ids.len()],
        ..Counters::default()
    };
    let mut genomes = BTreeMap::new();
    let mut frames = vec![frame(&config, &state, &counters, &mut genomes)];
    for _ in 0..args.steps {
        let report = micro::step(&config, &mut state)?;
        ensure!(
            report.matter_residual.iter().all(|n| *n == 0),
            "nonzero matter residual"
        );
        ensure!(report.energy_residual == 0, "nonzero energy residual");
        counters.births += u64::from(report.births);
        counters.deaths += u64::from(report.deaths);
        counters.divisions += u64::from(report.fissions);
        counters.growth_extent = counters
            .growth_extent
            .checked_add(report.extent)
            .context("extent counter overflow")?;
        counters.death_mass = counters
            .death_mass
            .checked_add(report.death_mass)
            .context("death counter overflow")?;
        counters.medium_energy = counters
            .medium_energy
            .checked_add(report.medium_energy)
            .context("energy channel overflow")?;
        for (total, delta) in counters.medium_matter.iter_mut().zip(report.medium_matter) {
            *total = total
                .checked_add(delta)
                .context("matter channel overflow")?;
        }
        if state.tick.is_multiple_of(args.sample_every) || state.tick == args.steps {
            frames.push(frame(&config, &state, &counters, &mut genomes));
        }
    }
    Ok(json!({
        "schema_version":1,
        "kind":"recorded_cell_experiment",
        "provenance":{"source_verification":"git_head_and_clean_engine_and_exporter",
            "build_requirement":"Generate with cargo run from the verified checkout; standalone binaries are not build attestations."},
        "identity":{"seed":args.seed.to_string(),"config_hash":config.config_hash,
            "world_format_version":WORLD_FORMAT_VERSION,"chamber_format":config.chamber_format,
            "source_commit":args.source_commit.to_lowercase(),"scenario":config.name},
        "model":{"environment":"well_mixed","volume_m3":config.volume_m3,
            "temperature_k":config.temperature_kelvin,"spatial_positions":false},
        "experiment":{"steps":args.steps,"sample_every":args.sample_every,"dt_seconds":config.dt_seconds,
            "checked_ticks":args.steps,"matter_residual_max":"0","energy_residual_max":"0"},
        "limitations":["Recorded observations, not a live simulation or a restart checkpoint.",
            "Screen positions are display-only; the chamber has no spatial motion.",
            "Frames are sampled; events between them are not reconstructed.",
            "Bounded inherited kinetics, ideal thermal bath, and uncalibrated prototype chemistry."],
        "canonical_config":canonical,"genomes":genomes,"frames":frames
    }))
}

fn frame(
    config: &MicroConfig,
    state: &MicroState,
    counters: &Counters,
    genomes: &mut BTreeMap<String, Value>,
) -> Value {
    let bio_units = config.units_per_mol[config.growth.biomass_substance] as f64;
    let energy_units = config.energy_units_per_joule as f64;
    let affinity_units = config.units_per_mol[config.growth.limiting_substance] as f64;
    let mut total_mass = 0i128;
    let mut total_energy = 0i128;
    let cells: Vec<_> = state.cells.iter().map(|cell| {
        total_mass += cell.mass;
        total_energy += cell.energy;
        let key = format!("K{}", cell.genome.kinetics);
        let decoded = micro::config::decode_genome(config, &cell.genome);
        genomes.entry(key.clone()).or_insert_with(|| json!({
            "genome":{"kinetics":cell.genome.kinetics,
                "base_growth_per_s":cell.genome.max_growth_rate_per_second,
                "base_km_mol_m3":cell.genome.affinity_km_amount as f64 / affinity_units / config.volume_m3,
                "division_mass_mol":cell.genome.division_mass as f64 / bio_units,
                "maintenance_w":cell.genome.maintenance_energy_per_tick as f64 / energy_units / config.dt_seconds,
                "capture_fraction":f64::from(cell.genome.capture_numerator)/f64::from(cell.genome.capture_denominator),
                "division_cost_j":cell.genome.division_energy_cost as f64 / energy_units,
                "starvation_tolerance_s":f64::from(cell.genome.starvation_tolerance_ticks)*config.dt_seconds},
            "phenotype":{"growth_per_s":decoded.max_growth_rate_per_second,
                "km_mol_m3":decoded.affinity_km_amount / affinity_units / config.volume_m3}
        }));
        json!({"id":cell.id.to_string(),"parent_id":cell.parent_id.map(|id|id.to_string()),
            "generation":cell.generation,"birth_tick":cell.birth_tick,"age_s":cell.age as f64*config.dt_seconds,
            "genome_key":key,"mass_mol":cell.mass as f64/bio_units,"energy_j":cell.energy as f64/energy_units,
            "mass_units":cell.mass.to_string(),"energy_units":cell.energy.to_string(),
            "division_mass_mol":decoded.division_mass as f64/bio_units,
            "starvation_s":f64::from(cell.starvation_ticks)*config.dt_seconds})
    }).collect();
    let resources: Vec<_> = config
        .matter_ids
        .iter()
        .enumerate()
        .filter(|(s, _)| *s != config.growth.biomass_substance)
        .map(|(s, id)| {
            let amount_mol = state.matter[s] as f64 / config.units_per_mol[s] as f64;
            json!({"id":id,"concentration":amount_mol/config.volume_m3,"amount_mol":amount_mol})
        })
        .collect();
    json!({"tick":state.tick,"sim_time":state.tick as f64*config.dt_seconds,
        "summary":{"living_cells":cells.len(),"births":counters.births,"deaths":counters.deaths,
            "divisions":counters.divisions,"generation_max":state.cells.iter().map(|c|c.generation).max().unwrap_or(0),
            "total_biomass_mol":total_mass as f64/bio_units,"total_cell_energy_j":total_energy as f64/energy_units},
        "residual":if state.tick==0 { Value::Null } else {json!({"matter":"0","energy":"0"})},
        "accounting":{"matter_ids":config.matter_ids,"free_matter_units":state.matter.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "bath_heat_units":state.heat.to_string(),"medium_matter_units":counters.medium_matter.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "medium_energy_units":counters.medium_energy.to_string(),"growth_extent_units":counters.growth_extent.to_string(),
            "death_mass_units":counters.death_mass.to_string()},
        "resources":resources,"cells":cells})
}

#[cfg(test)]
mod tests {
    use super::*;
    const CONFIG: &str = include_str!("../../../configs/scenarios/cell-chamber.toml");

    fn args() -> Args {
        Args {
            config: PathBuf::new(),
            seed: u64::MAX,
            steps: 125,
            sample_every: 50,
            source_commit: "a".repeat(40),
            output: PathBuf::new(),
        }
    }

    #[test]
    fn recording_is_deterministic_exact_seed_and_never_invents_frames() {
        let a = record(CONFIG, &args()).unwrap();
        assert_eq!(a, record(CONFIG, &args()).unwrap());
        assert_eq!(a["identity"]["seed"], u64::MAX.to_string());
        let frames = a["frames"].as_array().unwrap();
        assert_eq!(
            frames
                .iter()
                .map(|f| f["tick"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [0, 50, 100, 125]
        );
        assert!(frames[0]["residual"].is_null());
        for frame in &frames[1..] {
            assert_eq!(frame["residual"]["matter"], "0");
            assert_eq!(frame["residual"]["energy"], "0");
            assert_eq!(
                frame["summary"]["living_cells"].as_u64().unwrap() as usize,
                frame["cells"].as_array().unwrap().len()
            );
            for cell in frame["cells"].as_array().unwrap() {
                assert!(cell["id"].is_string());
                assert!(
                    cell["mass_units"]
                        .as_str()
                        .unwrap()
                        .parse::<i128>()
                        .unwrap()
                        > 0
                );
                assert!(
                    a["genomes"]
                        .get(cell["genome_key"].as_str().unwrap())
                        .is_some()
                );
            }
        }
        assert_eq!(a["experiment"]["checked_ticks"], 125);
    }

    #[test]
    fn rejects_invalid_identity_and_unbounded_sampling() {
        let mut input = args();
        input.source_commit = "main".into();
        assert!(record(CONFIG, &input).is_err());
        input = args();
        input.sample_every = 0;
        assert!(record(CONFIG, &input).is_err());
        input = args();
        input.steps = 2001;
        input.sample_every = 1;
        assert!(record(CONFIG, &input).is_err());
    }

    #[test]
    fn source_binding_rejects_wrong_commit_and_modified_or_untracked_engine() {
        let head = "a".repeat(40);
        validate_source_evidence(&head, &head, true, true).unwrap();
        assert!(validate_source_evidence(&"b".repeat(40), &head, true, true).is_err());
        assert!(validate_source_evidence(&head, &head, false, true).is_err());
        assert!(validate_source_evidence(&head, &head, true, false).is_err());
    }
}
