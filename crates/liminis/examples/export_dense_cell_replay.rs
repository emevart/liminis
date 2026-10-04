//! Stream every checked archived chamber tick to the bounded dense packer.
//! This is an observation producer, not a checkpoint or a new biological engine.
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use liminis_core::micro::{self, MicroConfig, MicroState};
use liminis_core::version::WORLD_FORMAT_VERSION;
use serde_json::{Value, json};

const ENGINE_REF: &str = "b2024cef08f0aa910a711a41c6a37fbc7b2ba35a";
const RECORDING_REF: &str = "43f4a2df25c356ff387b71ac1961b516975f999b";
const CORE_TREE: &str = "bd2a0a0d55dcfff052e5573ddfc863f4d918d489";
const CONFIG_PATH: &str = "configs/scenarios/cell-chamber.toml";
const FROZEN_PATHS: [&str; 7] = [
    "crates/liminis-core",
    CONFIG_PATH,
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "crates/liminis/Cargo.toml",
    "crates/liminis/examples/export_cell_replay.rs",
];

#[derive(Parser)]
struct Args {
    /// Actual producer HEAD, separate from the unchanged archived engine ref.
    #[arg(long)]
    producer_commit: String,
    #[arg(long, default_value_t = 10_000)]
    steps: u64,
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

fn git(args: &[&str]) -> Result<String> {
    let result = Command::new("git").args(args).output()?;
    ensure!(result.status.success(), "Git provenance command failed");
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

fn verify_source(producer: &str) -> Result<Value> {
    ensure!(
        producer.len() == 40 && producer.bytes().all(|b| b.is_ascii_hexdigit()),
        "producer-commit must be a full SHA"
    );
    let root = PathBuf::from(git(&["rev-parse", "--show-toplevel"])?);
    ensure!(
        root == PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?,
        "run cargo from the producer checkout"
    );
    let head = git(&["rev-parse", "HEAD"])?;
    ensure!(
        producer.eq_ignore_ascii_case(&head),
        "producer-commit differs from HEAD"
    );
    ensure!(
        git(&["status", "--porcelain", "--untracked-files=normal"])?.is_empty(),
        "producer checkout has tracked changes or untracked files"
    );
    ensure!(WORLD_FORMAT_VERSION == 30, "archive world identity changed");
    let mut objects = BTreeMap::new();
    for path in FROZEN_PATHS {
        let archived = git(&["rev-parse", &format!("{ENGINE_REF}:{path}")])?;
        let legacy = git(&["rev-parse", &format!("{RECORDING_REF}:{path}")])?;
        let current = git(&["rev-parse", &format!("HEAD:{path}")])?;
        ensure!(
            archived == legacy && current == archived,
            "archived bytes changed: {path}"
        );
        if path == "crates/liminis-core" {
            ensure!(archived == CORE_TREE, "unexpected archived core tree");
        }
        objects.insert(path, current);
    }
    let rustc = Command::new("rustc").arg("--version").output()?;
    ensure!(rustc.status.success(), "cannot inspect runtime rustc");
    Ok(json!({
        "producer_commit": head, "producer_tree": git(&["rev-parse", "HEAD^{tree}"])?,
        "engine_ref": ENGINE_REF, "legacy_recording_ref": RECORDING_REF,
        "frozen_git_objects": objects, "source_verification": "clean_HEAD_and_archived_Git_object_equality",
        "runtime_rustc": String::from_utf8(rustc.stdout)?.trim(),
        "host_os": std::env::consts::OS, "host_arch": std::env::consts::ARCH,
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "build_attestation": false,
        "build_requirement": "Use cargo run --locked --release in this clean producer checkout. Runtime metadata is not standalone binary attestation."
    }))
}

fn write_line(output: &mut impl Write, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() < 4 * 1024 * 1024, "producer line exceeds 4 MiB");
    output.write_all(&bytes)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        (1..=1_000_000).contains(&args.steps),
        "steps must be 1..=1000000"
    );
    ensure!(
        !cfg!(debug_assertions),
        "official dense production requires release"
    );
    let provenance = verify_source(&args.producer_commit)?;
    let text = fs::read_to_string(CONFIG_PATH)?;
    let scenario = micro::config::parse(&text)?;
    let canonical = micro::config::canonical(&scenario)?;
    let config = micro::config::derive(&scenario, 42)?;
    ensure!(
        config.chamber_format == 1 && config.dt_seconds == 30.0 && config.max_cells == 512,
        "unexpected archived chamber admission"
    );
    let mut state = MicroState::new(&config)?;
    let mut counters = Counters {
        medium_matter: vec![0; config.matter_ids.len()],
        ..Counters::default()
    };
    let mut genomes = BTreeMap::new();
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    write_line(
        &mut output,
        &json!({
            "type":"header", "schema_version":1, "kind":"dense_cell_frame_stream",
            "provenance":provenance,
            "identity":{"seed":"42","config_hash":config.config_hash,"world_format_version":WORLD_FORMAT_VERSION,
                "chamber_format":config.chamber_format,"source_commit":ENGINE_REF,"scenario":config.name},
            "model":{"environment":"well_mixed","volume_m3":config.volume_m3,
                "temperature_k":config.temperature_kelvin,"spatial_positions":false},
            "experiment":{"steps":args.steps,"sample_every":1,"dt_seconds":config.dt_seconds},
            "canonical_config":canonical,
            "limitations":["Every checked tick is an observation, not a restart checkpoint.",
                "No spatial coordinates, interpolation, JavaScript biology, or reconstruction of within-tick events.",
                "Bounded inherited kinetics and uncalibrated prototype chemistry; no open-ended evolution claim."]
        }),
    )?;
    for tick in 0..=args.steps {
        if tick > 0 {
            let report = micro::step(&config, &mut state)?;
            ensure!(
                report.tick == tick && state.tick == tick,
                "tick boundary mismatch"
            );
            ensure!(
                report.matter_residual.len() == config.matter_ids.len()
                    && report.medium_matter.len() == config.matter_ids.len()
                    && report.matter_residual.iter().all(|n| *n == 0),
                "nonzero/malformed matter residual"
            );
            ensure!(report.energy_residual == 0, "nonzero energy residual");
            counters.births = counters
                .births
                .checked_add(u64::from(report.births))
                .context("birth counter overflow")?;
            counters.deaths = counters
                .deaths
                .checked_add(u64::from(report.deaths))
                .context("death counter overflow")?;
            counters.divisions = counters
                .divisions
                .checked_add(u64::from(report.fissions))
                .context("division counter overflow")?;
            counters.growth_extent = counters
                .growth_extent
                .checked_add(report.extent)
                .context("extent overflow")?;
            counters.death_mass = counters
                .death_mass
                .checked_add(report.death_mass)
                .context("death mass overflow")?;
            counters.medium_energy = counters
                .medium_energy
                .checked_add(report.medium_energy)
                .context("energy channel overflow")?;
            for (total, delta) in counters.medium_matter.iter_mut().zip(report.medium_matter) {
                *total = total
                    .checked_add(delta)
                    .context("matter channel overflow")?;
            }
        }
        ensure!(
            state.cells.len() <= 512,
            "cell inventory exceeds archived bound"
        );
        let observed = frame(&config, &state, &counters, &mut genomes);
        write_line(
            &mut output,
            &json!({"type":"frame","frame":observed,"genomes":genomes}),
        )?;
    }
    write_line(
        &mut output,
        &json!({"type":"end","checked_ticks":state.tick,"frames":args.steps+1,
        "matter_residual_max":"0","energy_residual_max":"0"}),
    )?;
    output.flush()?;
    Ok(())
}

// Frame construction is byte-for-byte the archived exporter function.
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
