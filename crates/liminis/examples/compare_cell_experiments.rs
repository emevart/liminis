//! Bounded observations of the existing well-mixed cell model, not a new engine.
//!
//! Reproducible manifest (the TOML is materialized, never patched after derive):
//! ```sh
//! python3 - <<'PY'
//! import json, pathlib
//! scenario = pathlib.Path('configs/scenarios/cell-chamber.toml').read_text()
//! pathlib.Path('/tmp/cell-lab.json').write_text(json.dumps({
//!   'schema_version': 1, 'batch_id': 'baseline', 'runs': [{
//!     'run_id': 'baseline-42', 'condition': 'baseline', 'seed': '42',
//!     'steps': 1000, 'sample_every': 10, 'scenario_toml': scenario}]}))
//! PY
//! cargo run --release -p liminis --example compare_cell_experiments -- \
//!   --manifest /tmp/cell-lab.json --source-commit "$(git rev-parse HEAD)" \
//!   --output /tmp/cell-lab-results.json
//! ```
//! Source verification describes the checkout, not an attestation of a standalone
//! binary. Watchdogs are disabled by default; enabling one can change the stopping
//! boundary. A finite horizon is censored, not evidence of indefinite survival.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use liminis_core::micro::{self, MicroConfig, MicroState, MicroStepReport};
use liminis_core::version::WORLD_FORMAT_VERSION;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOML_BYTES: usize = 100 * 1024;
const MAX_RUNS: usize = 64;
const MAX_STEPS: u64 = 1_000_000;
const MAX_BATCH_STEPS: u64 = 2_000_000;
const MAX_CELLS: u32 = 4096;
const MAX_CELL_TICKS: u64 = 100_000_000;
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SAMPLES: u64 = 201;
const PROVENANCE_INPUTS: [&str; 7] = [
    "crates/liminis-core",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "crates/liminis/Cargo.toml",
    "crates/liminis/examples/compare_cell_experiments.rs",
    "crates/liminis/tests/compare_cell_experiments.rs",
];

#[derive(Parser)]
struct Args {
    #[arg(long)]
    manifest: PathBuf,
    /// Full SHA of HEAD; all provenance inputs must be clean and tracked.
    #[arg(long)]
    source_commit: String,
    #[arg(long)]
    output: PathBuf,
    #[command(flatten)]
    limits: Limits,
}

#[derive(Clone, Debug, Parser, Serialize)]
struct Limits {
    #[arg(long, default_value_t = MAX_RUNS)]
    max_runs: usize,
    #[arg(long, default_value_t = MAX_STEPS)]
    max_steps_per_run: u64,
    #[arg(long, default_value_t = MAX_BATCH_STEPS)]
    max_batch_steps: u64,
    #[arg(long, default_value_t = MAX_CELLS)]
    max_cells: u32,
    #[arg(long, default_value_t = MAX_CELL_TICKS)]
    max_cell_ticks: u64,
    #[arg(long, default_value_t = MAX_OUTPUT_BYTES)]
    max_output_bytes: usize,
    /// Optional wall-clock watchdog, 0 means disabled; at most 60 seconds.
    #[arg(long, default_value_t = 0)]
    max_run_seconds: u64,
    /// Optional wall-clock watchdog, 0 means disabled; at most 300 seconds.
    #[arg(long, default_value_t = 0)]
    max_batch_seconds: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_runs: MAX_RUNS,
            max_steps_per_run: MAX_STEPS,
            max_batch_steps: MAX_BATCH_STEPS,
            max_cells: MAX_CELLS,
            max_cell_ticks: MAX_CELL_TICKS,
            max_output_bytes: MAX_OUTPUT_BYTES,
            max_run_seconds: 0,
            max_batch_seconds: 0,
        }
    }
}

impl Limits {
    fn validate(&self) -> Result<()> {
        ensure!((1..=MAX_RUNS).contains(&self.max_runs), "invalid run cap");
        ensure!(
            (1..=MAX_STEPS).contains(&self.max_steps_per_run),
            "invalid step cap"
        );
        ensure!(
            (1..=MAX_BATCH_STEPS).contains(&self.max_batch_steps),
            "invalid batch step cap"
        );
        ensure!(
            (1..=MAX_CELLS).contains(&self.max_cells),
            "invalid cell cap"
        );
        ensure!(
            (1..=MAX_CELL_TICKS).contains(&self.max_cell_ticks),
            "invalid work cap"
        );
        ensure!(
            (1..=MAX_OUTPUT_BYTES).contains(&self.max_output_bytes),
            "invalid output cap"
        );
        ensure!(
            self.max_run_seconds <= 60,
            "run watchdog exceeds 60 seconds"
        );
        ensure!(
            self.max_batch_seconds <= 300,
            "batch watchdog exceeds 300 seconds"
        );
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    batch_id: String,
    runs: Vec<Run>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    run_id: String,
    condition: String,
    /// Decimal string preserves all 64 bits in JavaScript and JSON consumers.
    seed: String,
    steps: u64,
    sample_every: u64,
    scenario_toml: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    args.limits.validate()?;
    verify_source(&args.source_commit)?;
    let mut bytes = Vec::new();
    File::open(&args.manifest)?
        .take((MAX_MANIFEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_MANIFEST_BYTES, "manifest exceeds 2 MiB");
    let manifest = parse_manifest(&bytes, &args.limits)?;
    let artifact = compare(
        &manifest,
        &args.source_commit,
        &args.limits,
        runtime_provenance()?,
    )?;
    // Serialize through a capped writer, never allocate an unlimited byte vector.
    let mut output = CappedWriter::new(args.limits.max_output_bytes);
    serde_json::to_writer(&mut output, &artifact).context("bounded result serialization")?;
    if let Some(parent) = args.output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, &output.bytes)?;
    println!(
        "{} results, {} bytes, blake3:{}",
        manifest.runs.len(),
        output.bytes.len(),
        blake3::hash(&output.bytes)
    );
    Ok(())
}

fn parse_manifest(bytes: &[u8], limits: &Limits) -> Result<Manifest> {
    limits.validate()?;
    ensure!(bytes.len() <= MAX_MANIFEST_BYTES, "manifest exceeds 2 MiB");
    let manifest: Manifest = serde_json::from_slice(bytes).context("parse strict JSON manifest")?;
    ensure!(manifest.schema_version == 1, "unsupported manifest schema");
    ensure!(valid_id(&manifest.batch_id), "invalid batch_id");
    ensure!(
        !manifest.runs.is_empty() && manifest.runs.len() <= limits.max_runs,
        "run count exceeds budget or is zero"
    );
    let mut ids = BTreeSet::new();
    for run in &manifest.runs {
        ensure!(
            valid_id(&run.run_id) && ids.insert(&run.run_id),
            "invalid or duplicate run_id"
        );
        ensure!(
            !run.condition.is_empty()
                && run.condition.len() <= 128
                && !run.condition.chars().any(char::is_control),
            "invalid condition label"
        );
    }
    Ok(manifest)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn parse_seed(seed: &str) -> Result<u64> {
    ensure!(
        !seed.is_empty()
            && (seed == "0" || !seed.starts_with('0'))
            && seed.bytes().all(|b| b.is_ascii_digit()),
        "seed must be a canonical decimal u64 string"
    );
    seed.parse().context("seed exceeds u64")
}

fn verify_source(commit: &str) -> Result<()> {
    let root = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    ensure!(root.status.success(), "runner requires a Git checkout");
    let root = PathBuf::from(String::from_utf8(root.stdout)?.trim());
    let head = Command::new("git")
        .current_dir(&root)
        .args(["rev-parse", "HEAD"])
        .output()?;
    ensure!(head.status.success(), "cannot inspect HEAD");
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
        "cannot inspect untracked provenance inputs"
    );
    validate_source(
        commit,
        String::from_utf8(head.stdout)?.trim(),
        clean.success(),
        untracked.stdout.is_empty(),
    )
}

fn validate_source(commit: &str, head: &str, clean: bool, no_untracked: bool) -> Result<()> {
    ensure!(
        commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "source-commit must be a full 40-character Git SHA"
    );
    ensure!(
        commit.eq_ignore_ascii_case(head),
        "source-commit differs from HEAD"
    );
    ensure!(
        clean && no_untracked,
        "core, runner, tests, Cargo and toolchain must match HEAD; commit before producing results"
    );
    Ok(())
}

fn runtime_provenance() -> Result<Value> {
    let rustc = Command::new("rustc").arg("--version").output()?;
    ensure!(rustc.status.success(), "cannot inspect runtime rustc");
    Ok(json!({
        "source_verification": "git_head_and_clean_provenance_inputs",
        "runtime_rustc": String::from_utf8(rustc.stdout)?.trim(),
        "runner_version": env!("CARGO_PKG_VERSION"),
        "host_os": std::env::consts::OS,
        "host_arch": std::env::consts::ARCH,
        "numerical_mode": "FLOAT/native f32 Q and f64 genome decoding; exact i128 ledgers",
        "profile": if cfg!(debug_assertions) {"debug"} else {"release"},
        "build_attestation": false,
        "build_requirement": "Use cargo run in this checkout. Runtime rustc and profile are observations, not proof of how a standalone binary was built."
    }))
}

struct Prepared {
    result: Value,
    config: Option<MicroConfig>,
}

fn prepare(run: &Run, limits: &Limits, requested_remaining: &mut u64) -> Prepared {
    let mut result = json!({
        "run_id":run.run_id,"condition":run.condition,"seed":run.seed,
        "requested_steps":run.steps,"sample_every":run.sample_every,
        "input_toml_digest":full_hash(run.scenario_toml.as_bytes()),
        "status":"refused","stop_reason":"not_started","error":null,
        "checked_ticks":0,"attempted_tick":null,"attempt_residuals":null,
        "attempted_cell_ticks":"0","committed_cell_ticks":"0",
        "summary":null,"samples":[],"samples_truncated":false,
        "canonical_config":null,"identity":null
    });
    let preparation = (|| -> Result<MicroConfig> {
        ensure!(
            run.scenario_toml.len() <= MAX_TOML_BYTES,
            "scenario TOML exceeds 100 KiB"
        );
        ensure!(
            (1..=limits.max_steps_per_run).contains(&run.steps),
            "requested steps exceed per-run budget or are zero"
        );
        ensure!(
            run.sample_every > 0 && run.steps.div_ceil(run.sample_every) < MAX_SAMPLES,
            "sampling exceeds 201 samples or has zero interval"
        );
        ensure!(
            run.steps <= *requested_remaining,
            "requested steps exceed remaining batch budget"
        );
        let seed = parse_seed(&run.seed)?;
        // All scenario changes precede parse/canonical/derive. No runtime config is patched.
        let scenario = micro::config::parse(&run.scenario_toml)?;
        let canonical = micro::config::canonical(&scenario)?;
        result["canonical_config"] = json!(canonical);
        result["identity"] = json!({
            "config_hash":micro::config::config_hash(&scenario)?,
            "canonical_digest":full_hash(canonical.as_bytes()),
            "derived":false,
            "validation":"parsed; chemistry derivation has not succeeded"
        });
        ensure!(
            scenario.chamber.max_cells <= limits.max_cells,
            "declared max_cells exceeds runner budget"
        );
        ensure!(
            (run.steps as f64 * scenario.dt).is_finite(),
            "requested physical horizon is not finite"
        );
        let config = micro::config::derive(&scenario, seed)?;
        result["identity"] = json!({
            "config_hash":config.config_hash,"canonical_digest":full_hash(canonical.as_bytes()),
            "derived":true,
            "world_format_version":WORLD_FORMAT_VERSION,"chamber_format":config.chamber_format,
            "scenario":config.name,"dt_seconds":config.dt_seconds,"volume_m3":config.volume_m3,
            "temperature_k":config.temperature_kelvin,"declared_max_cells":config.max_cells,
            "model":"well_mixed_individual_cells","spatial_positions":false,
            "units":{"concentration":"mol/m^3","structural_mass":"mol structural biomass","energy":"J","time":"s","temperature":"K"},
            "biomass_substance":config.matter_ids[config.growth.biomass_substance],
            "matter_scales":config.matter_ids.iter().zip(&config.units_per_mol).map(|(id,scale)| json!({"id":id,"units_per_mol":scale.to_string()})).collect::<Vec<_>>(),
            "energy_units_per_joule":config.energy_units_per_joule.to_string()
        });
        *requested_remaining -= run.steps;
        Ok(config)
    })();
    match preparation {
        Ok(config) => Prepared {
            result,
            config: Some(config),
        },
        Err(error) => {
            result["stop_reason"] = json!("input_or_admission_refused");
            result["error"] = json!(short_error(&error));
            Prepared {
                result,
                config: None,
            }
        }
    }
}

fn compare(manifest: &Manifest, source: &str, limits: &Limits, provenance: Value) -> Result<Value> {
    limits.validate()?;
    ensure!(
        source.len() == 40 && source.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid source SHA"
    );
    let batch_start = Instant::now();
    let mut requested_remaining = limits.max_batch_steps;
    let mut prepared: Vec<_> = manifest
        .runs
        .iter()
        .map(|run| prepare(run, limits, &mut requested_remaining))
        .collect();
    let mut artifact = json!({
        "schema_version":1,"kind":"comparative_cell_lab","source_commit":source.to_lowercase(),
        "provenance":provenance,"budgets":limits,"batch_id":manifest.batch_id,
        "batch":{"admitted_requested_steps":limits.max_batch_steps-requested_remaining,"attempted_cell_ticks":"0","committed_cell_ticks":"0"},
        "determinism":{"watchdogs_enabled":limits.max_run_seconds>0 || limits.max_batch_seconds>0,
            "claim":"Exact repeat requires identical source/config/seed/profile, platform/toolchain and stopping boundary. FLOAT transcendental results are not attested across hardware. Wall-clock watchdogs may stop at different boundaries."},
        "limitations":["Uncalibrated prototype chemistry and bounded inherited kinetics.","No temperature dependence, spatial dynamics, GRN or open-ended evolution.","Finite horizons and resource stops are censored observations, not completed biological histories.","Samples contain aggregates, not resumable states or event reconstructions."],
        "runs":[]
    });
    // Reserve a conservative final summary/footer before any simulation. The
    // 64 KiB/run covers bounded allele history, errors, lifecycle and integers;
    // registry-dependent storage is reserved separately. Identity already holds
    // the full canonical TOML. Arbitrarily long declared IDs need their actual
    // JSON-escaped byte count in the final summary, not a per-entry estimate.
    // Samples spend only the remaining byte allowance.
    let mut reservation = bounded_size(&artifact, limits.max_output_bytes)? + 1024;
    for item in &prepared {
        reservation = reservation.saturating_add(bounded_size(&item.result, MAX_OUTPUT_BYTES)? + 1);
        if let Some(config) = &item.config {
            let registry_bytes = bounded_size(&json!(config.matter_ids), MAX_OUTPUT_BYTES)?;
            reservation = reservation
                .saturating_add(64 * 1024 + config.matter_ids.len() * 512 + registry_bytes);
        }
    }
    if reservation > limits.max_output_bytes {
        for item in &mut prepared {
            if item.config.take().is_some() {
                item.result["status"] = json!("refused");
                item.result["stop_reason"] = json!("output_reservation_budget");
                item.result["error"] = json!(
                    "Canonical identity plus reserved summary would exceed the output budget; no simulation started."
                );
            }
            if item.result["canonical_config"].is_string() {
                item.result["canonical_config"] = Value::Null;
                item.result["canonical_config_omitted"] = json!("output_reservation_budget");
            }
            if item.result["identity"].is_object() {
                let identity = &item.result["identity"];
                let minimal = json!({
                    "config_hash":identity["config_hash"],
                    "canonical_digest":identity["canonical_digest"],
                    "derived":identity["derived"],
                    "runtime_identity_omitted":"output_reservation_budget"
                });
                item.result["identity"] = minimal;
            }
        }
        artifact["runs"] = json!(prepared.into_iter().map(|p| p.result).collect::<Vec<_>>());
        bounded_size(&artifact, limits.max_output_bytes).context(
            "output cap cannot hold even the complete refusal envelope; no simulation started",
        )?;
        return Ok(artifact);
    }
    let mut sample_bytes = limits.max_output_bytes - reservation;
    let mut work = Work::default();
    for (run, item) in manifest.runs.iter().zip(&mut prepared) {
        if let Some(config) = &item.config {
            observe(
                run,
                config,
                limits,
                &batch_start,
                &mut work,
                &mut sample_bytes,
                &mut item.result,
            )?;
        }
    }
    artifact["batch"]["attempted_cell_ticks"] = json!(work.attempted.to_string());
    artifact["batch"]["committed_cell_ticks"] = json!(work.committed.to_string());
    artifact["runs"] = json!(prepared.into_iter().map(|p| p.result).collect::<Vec<_>>());
    bounded_size(&artifact, limits.max_output_bytes)
        .context("reserved output envelope exceeded")?;
    Ok(artifact)
}

#[derive(Default)]
struct Work {
    attempted: u64,
    committed: u64,
}

#[derive(Clone, Default)]
struct Counters {
    births: u64,
    fissions: u64,
    deaths: u64,
    extent: i128,
    death_mass: i128,
    medium_energy: i128,
    medium_matter: Vec<i128>,
    peak_cells: usize,
    max_generation: u32,
    first_seen: BTreeMap<i8, u64>,
}

impl Counters {
    fn account(&self, report: &MicroStepReport, state: &MicroState) -> Result<Self> {
        let mut next = self.clone();
        next.births = next
            .births
            .checked_add(u64::from(report.births))
            .context("birth counter overflow")?;
        next.fissions = next
            .fissions
            .checked_add(u64::from(report.fissions))
            .context("fission counter overflow")?;
        next.deaths = next
            .deaths
            .checked_add(u64::from(report.deaths))
            .context("death counter overflow")?;
        next.extent = next
            .extent
            .checked_add(report.extent)
            .context("extent counter overflow")?;
        next.death_mass = next
            .death_mass
            .checked_add(report.death_mass)
            .context("death mass counter overflow")?;
        next.medium_energy = next
            .medium_energy
            .checked_add(report.medium_energy)
            .context("medium energy counter overflow")?;
        for (sum, delta) in next.medium_matter.iter_mut().zip(&report.medium_matter) {
            *sum = sum
                .checked_add(*delta)
                .context("medium matter counter overflow")?;
        }
        next.observe(state);
        Ok(next)
    }

    fn observe(&mut self, state: &MicroState) {
        self.peak_cells = self.peak_cells.max(state.cells.len());
        for cell in &state.cells {
            self.max_generation = self.max_generation.max(cell.generation);
            self.first_seen
                .entry(cell.genome.kinetics)
                .or_insert(state.tick);
        }
    }
}

fn observe(
    run: &Run,
    config: &MicroConfig,
    limits: &Limits,
    batch_start: &Instant,
    work: &mut Work,
    sample_bytes: &mut usize,
    result: &mut Value,
) -> Result<()> {
    let initial_attempted_work = work.attempted;
    let initial_committed_work = work.committed;
    if work.attempted >= limits.max_cell_ticks || watchdog(batch_start, limits.max_batch_seconds) {
        result["stop_reason"] = json!("batch_budget_before_start");
        return Ok(());
    }
    let mut state = match MicroState::new(config) {
        Ok(state) => state,
        Err(error) => {
            result["stop_reason"] = json!("initialization_refused");
            result["error"] = json!(short_error(&error));
            return Ok(());
        }
    };
    let start = Instant::now();
    let mut counters = Counters {
        medium_matter: vec![0; config.matter_ids.len()],
        ..Counters::default()
    };
    counters.observe(&state);
    let mut samples = Vec::new();
    let mut reason = "requested_horizon_reached";
    let mut status = "censored";
    let mut error = None;
    let mut attempted_tick = None;
    let mut failed_attempt = false;
    let mut reports = HashWriter(blake3::Hasher::new());
    if !push_sample(config, &state, &counters, &mut samples, sample_bytes)? {
        reason = "output_sample_budget";
    } else {
        for _ in 0..run.steps {
            if watchdog(batch_start, limits.max_batch_seconds) {
                reason = "batch_watchdog";
                break;
            }
            if watchdog(&start, limits.max_run_seconds) {
                reason = "run_watchdog";
                break;
            }
            let cells = state.cells.len() as u64;
            if cells > limits.max_cell_ticks - work.attempted {
                reason = "batch_cell_work_budget";
                break;
            }
            // Failed attempts consume work; only accepted ticks count as committed.
            work.attempted += cells;
            attempted_tick = Some(state.tick + 1);
            let mut candidate = state.clone();
            let advance = micro::step(config, &mut candidate).and_then(|report| {
                check_report(config, &state, &candidate, &report)?;
                let next = counters.account(&report, &candidate)?;
                serde_json::to_writer(&mut reports, &report)?;
                Ok(next)
            });
            match advance {
                Ok(next) => {
                    counters = next;
                    state = candidate;
                    work.committed += cells;
                }
                Err(failure) => {
                    let message = short_error(&failure);
                    reason = if message.contains("max_cells") {
                        "cell_capacity_limit"
                    } else {
                        "numerical_failure"
                    };
                    status = if reason == "cell_capacity_limit" {
                        "censored"
                    } else {
                        "numerical_failure"
                    };
                    error = Some(message);
                    failed_attempt = true;
                    break;
                }
            }
            if state.cells.is_empty() {
                reason = "extinction";
                status = "extinct";
            }
            if (state.tick.is_multiple_of(run.sample_every)
                || state.tick == run.steps
                || state.cells.is_empty())
                && !push_sample(config, &state, &counters, &mut samples, sample_bytes)?
            {
                result["samples_truncated"] = json!(true);
                if !state.cells.is_empty() {
                    reason = "output_sample_budget";
                    status = "censored";
                }
                break;
            }
            if state.cells.is_empty() {
                break;
            }
        }
    }
    if samples
        .last()
        .is_none_or(|sample| sample["tick"] != state.tick)
        && !push_sample(config, &state, &counters, &mut samples, sample_bytes)?
    {
        result["samples_truncated"] = json!(true);
    }
    if reason == "output_sample_budget" {
        result["samples_truncated"] = json!(true);
    }
    result["status"] = json!(status);
    result["stop_reason"] = json!(reason);
    result["error"] = json!(error);
    result["checked_ticks"] = json!(state.tick);
    result["attempted_tick"] = json!(attempted_tick);
    result["attempted_cell_ticks"] = json!((work.attempted - initial_attempted_work).to_string());
    result["committed_cell_ticks"] = json!((work.committed - initial_committed_work).to_string());
    result["attempt_residuals"] = if failed_attempt || state.tick == 0 {
        Value::Null
    } else {
        json!({"matter":vec!["0";config.matter_ids.len()],"energy":"0"})
    };
    result["summary"] = summary(config, &state, &counters)?;
    result["final_state_digest"] = state_digest(&state)?;
    result["tick_reports_digest"] = json!(format!("blake3:{}", reports.0.finalize()));
    result["tick_reports_hashed"] = json!(state.tick);
    result["tick_report_digest_encoding"] = json!(
        "BLAKE3 of concatenated serde_json MicroStepReport values for committed ticks, in tick order"
    );
    result["samples"] = json!(samples);
    Ok(())
}

fn watchdog(start: &Instant, seconds: u64) -> bool {
    seconds != 0 && start.elapsed().as_secs() >= seconds
}

fn check_report(
    config: &MicroConfig,
    before: &MicroState,
    after: &MicroState,
    report: &MicroStepReport,
) -> Result<()> {
    let n = config.matter_ids.len();
    ensure!(
        report.matter_residual.len() == n && report.medium_matter.len() == n,
        "ledger report vector length mismatch"
    );
    ensure!(
        report.matter_residual.iter().all(|r| *r == 0) && report.energy_residual == 0,
        "reported integer ledger is nonzero"
    );
    ensure!(
        report.tick == after.tick
            && after.tick == before.tick + 1
            && report.cell_count as usize == after.cells.len(),
        "report state boundary mismatch"
    );
    let mut expected = report.medium_matter.clone();
    for nu in &config.growth.nu {
        expected[nu.substance] = expected[nu.substance]
            .checked_add(
                i128::from(nu.value)
                    .checked_mul(report.extent)
                    .context("growth delta overflow")?,
            )
            .context("matter delta overflow")?;
    }
    let bio = config.growth.biomass_substance;
    let det = config.death.detritus_substance;
    expected[bio] = expected[bio]
        .checked_sub(report.death_mass)
        .context("death BIO delta overflow")?;
    expected[det] = expected[det]
        .checked_add(report.death_mass)
        .context("death DET delta overflow")?;
    for ((a, b), delta) in combined_matter(config, after)?
        .iter()
        .zip(combined_matter(config, before)?)
        .zip(expected)
    {
        ensure!(
            a.checked_sub(b) == Some(delta),
            "independent matter ledger is nonzero"
        );
    }
    let energy_before = total_energy(config, before)?;
    let energy_after = total_energy(config, after)?;
    ensure!(
        energy_after.checked_sub(energy_before) == Some(report.medium_energy)
            && report.energy_before == energy_before
            && report.energy_after == energy_after,
        "independent energy ledger is nonzero"
    );
    let living = before.cells.len() as i128 + i128::from(report.births)
        - i128::from(report.fissions)
        - i128::from(report.deaths);
    ensure!(
        living == after.cells.len() as i128
            && report.births
                == report
                    .fissions
                    .checked_mul(2)
                    .context("lifecycle overflow")?,
        "lifecycle ledger did not close"
    );
    Ok(())
}

fn combined_matter(config: &MicroConfig, state: &MicroState) -> Result<Vec<i128>> {
    let mut amounts = state.matter.clone();
    for cell in &state.cells {
        amounts[config.growth.biomass_substance] = amounts[config.growth.biomass_substance]
            .checked_add(cell.mass)
            .context("structural mass sum overflow")?;
    }
    Ok(amounts)
}

fn total_energy(config: &MicroConfig, state: &MicroState) -> Result<i128> {
    let mut total = state.heat;
    for cell in &state.cells {
        total = total
            .checked_add(cell.energy)
            .context("cell energy sum overflow")?;
    }
    for (amount, weight) in combined_matter(config, state)?
        .into_iter()
        .zip(&config.chemical_weights)
    {
        total = total
            .checked_add(
                amount
                    .checked_mul(i128::from(*weight))
                    .context("chemical energy product overflow")?,
            )
            .context("total energy sum overflow")?;
    }
    Ok(total)
}

fn summary(config: &MicroConfig, state: &MicroState, counters: &Counters) -> Result<Value> {
    let mut alleles = BTreeMap::<String, usize>::new();
    let mut mass = 0_i128;
    let mut energy = 0_i128;
    for cell in &state.cells {
        *alleles.entry(cell.genome.kinetics.to_string()).or_default() += 1;
        mass = mass
            .checked_add(cell.mass)
            .context("summary mass overflow")?;
        energy = energy
            .checked_add(cell.energy)
            .context("summary energy overflow")?;
    }
    let living_identity = i128::from(config.founder.count) + i128::from(counters.births)
        - i128::from(counters.fissions)
        - i128::from(counters.deaths)
        - state.cells.len() as i128;
    ensure!(
        living_identity == 0,
        "cumulative lifecycle ledger did not close"
    );
    Ok(json!({
        "tick":state.tick,"time_seconds":state.tick as f64*config.dt_seconds,"living_cells":state.cells.len(),
        "peak_cells":counters.peak_cells,"max_generation_observed":counters.max_generation,
        "allele_histogram":alleles,"observed_allele_richness":counters.first_seen.len(),
        "allele_first_seen_tick":counters.first_seen.iter().map(|(allele,tick)| (allele.to_string(),*tick)).collect::<BTreeMap<_,_>>(),
        "structural_mass_units":mass.to_string(),"structural_mass_mol":mass as f64/config.units_per_mol[config.growth.biomass_substance] as f64,
        "internal_energy_units":energy.to_string(),"internal_energy_j":energy as f64/config.energy_units_per_joule as f64,
        "bath_heat_units":state.heat.to_string(),"bath_heat_j":state.heat as f64/config.energy_units_per_joule as f64,
        "matter":config.matter_ids.iter().zip(&config.units_per_mol).zip(&state.matter).map(|((id,scale),amount)| json!({"id":id,"free_units":amount.to_string(),"free_mol":*amount as f64 / *scale as f64})).collect::<Vec<_>>(),
        "lifecycle":{"founders":config.founder.count.to_string(),"daughters_born":counters.births.to_string(),"fissions":counters.fissions.to_string(),"deaths":counters.deaths.to_string(),"living_identity_residual":living_identity.to_string()},
        "accounting":{"growth_extent":counters.extent.to_string(),"death_mass_units":counters.death_mass.to_string(),"medium_energy_units":counters.medium_energy.to_string(),"medium_matter_units":counters.medium_matter.iter().map(ToString::to_string).collect::<Vec<_>>()},
        "ledger":{"checked_ticks":state.tick,"matter_residual_last_checked":if state.tick==0 {Value::Null} else {json!(vec!["0";config.matter_ids.len()])},"energy_residual_last_checked":if state.tick==0 {Value::Null} else {json!("0")}}
    }))
}

fn push_sample(
    config: &MicroConfig,
    state: &MicroState,
    counters: &Counters,
    samples: &mut Vec<Value>,
    remaining: &mut usize,
) -> Result<bool> {
    if samples
        .last()
        .is_some_and(|sample| sample["tick"] == state.tick)
    {
        return Ok(true);
    }
    let sample = summary(config, state, counters)?;
    let Ok(bytes) = bounded_size(&sample, *remaining) else {
        return Ok(false);
    };
    if bytes.saturating_add(1) > *remaining {
        return Ok(false);
    }
    *remaining -= bytes + 1;
    samples.push(sample);
    Ok(true)
}

fn full_hash(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes))
}

fn state_digest(state: &MicroState) -> Result<Value> {
    let mut writer = HashWriter(blake3::Hasher::new());
    serde_json::to_writer(&mut writer, &state.snapshot())?;
    Ok(json!(format!("blake3:{}", writer.0.finalize())))
}

struct HashWriter(blake3::Hasher);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn short_error(error: &anyhow::Error) -> String {
    format!("{error:#}").chars().take(1024).collect()
}

struct CappedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl CappedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}
impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            return Err(io::Error::other("output byte budget exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn bounded_size(value: &Value, limit: usize) -> Result<usize> {
    struct Counter {
        count: usize,
        limit: usize,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit - self.count {
                return Err(io::Error::other("output byte budget exceeded"));
            }
            self.count += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { count: 0, limit };
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.count)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = include_str!("../../../configs/scenarios/cell-chamber.toml");
    const COMMIT: &str = "0123456789012345678901234567890123456789";

    fn run(text: String, steps: u64) -> Run {
        Run {
            run_id: "test".into(),
            condition: "test".into(),
            seed: u64::MAX.to_string(),
            steps,
            sample_every: 10,
            scenario_toml: text,
        }
    }
    fn batch(runs: Vec<Run>, limits: Limits) -> Value {
        compare(
            &Manifest {
                schema_version: 1,
                batch_id: "test".into(),
                runs,
            },
            COMMIT,
            &limits,
            json!({"profile":"test","build_attestation":false}),
        )
        .unwrap()
    }
    fn scenario_text(edit: impl FnOnce(&mut micro::config::MicroScenario)) -> String {
        let mut scenario = micro::config::parse(SOURCE).unwrap();
        edit(&mut scenario);
        micro::config::canonical(&scenario).unwrap()
    }

    #[test]
    fn seed_max_repeat_has_identical_results_and_exact_every_tick_ledgers() {
        let a = batch(vec![run(SOURCE.into(), 300)], Limits::default());
        let b = batch(vec![run(SOURCE.into(), 300)], Limits::default());
        assert_eq!(a, b);
        let result = &a["runs"][0];
        assert_eq!(result["seed"], u64::MAX.to_string());
        assert_eq!(result["checked_ticks"], 300);
        assert_eq!(result["tick_reports_hashed"], 300);
        assert_eq!(result["stop_reason"], "requested_horizon_reached");
        assert_eq!(result["status"], "censored");
        assert_eq!(
            result["summary"]["ledger"]["energy_residual_last_checked"],
            "0"
        );
        assert_eq!(
            result["summary"]["lifecycle"]["living_identity_residual"],
            "0"
        );
        assert_eq!(
            result["samples"][0]["ledger"]["energy_residual_last_checked"],
            Value::Null
        );
    }

    #[test]
    fn canonical_hashes_ignore_comments_and_change_with_materialized_parameters() {
        let a = batch(vec![run(SOURCE.into(), 10)], Limits::default());
        let b = batch(
            vec![run(format!("# different spelling\n{SOURCE}"), 10)],
            Limits::default(),
        );
        let c = batch(
            vec![run(scenario_text(|s| s.founder.genome.kinetics = 1), 10)],
            Limits::default(),
        );
        assert_eq!(a["runs"][0]["identity"], b["runs"][0]["identity"]);
        assert_ne!(
            a["runs"][0]["identity"]["config_hash"],
            c["runs"][0]["identity"]["config_hash"]
        );
        assert_ne!(
            a["runs"][0]["identity"]["canonical_digest"],
            c["runs"][0]["identity"]["canonical_digest"]
        );
        assert_eq!(
            c["runs"][0]["identity"]["canonical_digest"]
                .as_str()
                .unwrap()
                .len(),
            71
        );
    }

    #[test]
    fn mutation_off_control_includes_fission_and_one_inherited_allele() {
        let artifact = batch(
            vec![run(
                scenario_text(|s| s.genome.mutation_probability = 0.0),
                1000,
            )],
            Limits::default(),
        );
        let result = &artifact["runs"][0];
        assert_eq!(result["checked_ticks"], 1000);
        assert!(
            result["summary"]["lifecycle"]["fissions"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert_eq!(result["summary"]["observed_allele_richness"], 1);
        assert!(result["samples"].as_array().unwrap().iter().all(|sample| {
            sample["allele_histogram"]
                .as_object()
                .unwrap()
                .keys()
                .all(|allele| allele == "0")
        }));
    }

    #[test]
    fn food_starvation_control_is_materialized_and_extinction_closes_ledgers() {
        let text = scenario_text(|s| {
            s.initial.concentration.insert("FOOD".into(), 0.0);
            s.medium.concentration.insert("FOOD".into(), 0.0);
        });
        let artifact = batch(vec![run(text, 1000)], Limits::default());
        let result = &artifact["runs"][0];
        assert_eq!(result["status"], "extinct");
        assert!(result["checked_ticks"].as_u64().unwrap() < 1000);
        assert_eq!(result["summary"]["lifecycle"]["deaths"], "8");
        assert_eq!(result["summary"]["accounting"]["growth_extent"], "0");
        assert_eq!(
            result["summary"]["ledger"]["energy_residual_last_checked"],
            "0"
        );
    }

    #[test]
    fn capacity_failure_retains_last_committed_state_and_charges_attempt() {
        let text = scenario_text(|s| s.chamber.max_cells = s.founder.count);
        let config =
            micro::config::derive(&micro::config::parse(&text).unwrap(), u64::MAX).unwrap();
        let mut expected = MicroState::new(&config).unwrap();
        let failed_tick = loop {
            let before = expected.clone();
            if micro::step(&config, &mut expected).is_err() {
                assert_eq!(expected, before);
                break expected.tick + 1;
            }
        };
        let artifact = batch(vec![run(text, 1000)], Limits::default());
        let result = &artifact["runs"][0];
        assert_eq!(result["stop_reason"], "cell_capacity_limit");
        assert_eq!(result["checked_ticks"], failed_tick - 1);
        assert_eq!(result["attempted_tick"], failed_tick);
        assert_eq!(result["attempt_residuals"], Value::Null);
        assert_eq!(
            result["final_state_digest"],
            state_digest(&expected).unwrap()
        );
        assert_eq!(
            artifact["batch"]["attempted_cell_ticks"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap(),
            failed_tick * 8
        );
    }

    #[test]
    fn refusal_and_unstarted_runs_are_preserved_under_batch_and_work_budgets() {
        let mut second = run(SOURCE.into(), 10);
        second.run_id = "second".into();
        let mut third = run(SOURCE.into(), 10);
        third.run_id = "third".into();
        let artifact = batch(
            vec![run("invalid TOML".into(), 10), second, third],
            Limits {
                max_cell_ticks: 8,
                ..Limits::default()
            },
        );
        assert_eq!(artifact["runs"][0]["status"], "refused");
        assert_eq!(artifact["runs"][0]["attempt_residuals"], Value::Null);
        assert_eq!(artifact["runs"][1]["checked_ticks"], 1);
        assert_eq!(artifact["runs"][1]["stop_reason"], "batch_cell_work_budget");
        assert_eq!(artifact["runs"][2]["status"], "refused");
        assert_eq!(
            artifact["runs"][2]["stop_reason"],
            "batch_budget_before_start"
        );
        assert_eq!(artifact["runs"][2]["attempted_tick"], Value::Null);
        let mut second = run(SOURCE.into(), 10);
        second.run_id = "second".into();
        let artifact = batch(
            vec![run(SOURCE.into(), 10), second],
            Limits {
                max_batch_steps: 10,
                ..Limits::default()
            },
        );
        assert_eq!(artifact["runs"][1]["status"], "refused");
        assert_eq!(artifact["runs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn semantic_guards_seed_and_declared_cell_budget_are_not_bypassed() {
        for text in [
            scenario_text(|s| s.dt = 1e9),
            scenario_text(|s| s.chamber.max_cells = 4097),
            SOURCE.replace("chamber_format = 1", "chamber_format = 999"),
        ] {
            let result = batch(vec![run(text, 10)], Limits::default());
            assert_eq!(result["runs"][0]["status"], "refused");
            assert_eq!(result["runs"][0]["checked_ticks"], 0);
        }
        assert!(parse_seed("18446744073709551616").is_err());
        assert!(parse_seed("01").is_err());
        assert!(parse_seed("-1").is_err());
        assert!(parse_seed("1e2").is_err());
        assert_eq!(parse_seed(&u64::MAX.to_string()).unwrap(), u64::MAX);
    }

    #[test]
    fn corrupt_report_is_rejected_even_if_it_claims_zero_residuals() {
        let config = micro::config::derive(&micro::config::parse(SOURCE).unwrap(), 42).unwrap();
        let before = MicroState::new(&config).unwrap();
        let mut after = before.clone();
        let report = micro::step(&config, &mut after).unwrap();
        check_report(&config, &before, &after, &report).unwrap();
        let mut corrupt = report.clone();
        corrupt.matter_residual.pop();
        assert!(check_report(&config, &before, &after, &corrupt).is_err());
        let mut corrupt = report.clone();
        corrupt.medium_energy += 1;
        assert!(check_report(&config, &before, &after, &corrupt).is_err());
        let mut corrupt = report.clone();
        corrupt.matter_residual[0] = 1;
        assert!(check_report(&config, &before, &after, &corrupt).is_err());
        let mut corrupt = report.clone();
        corrupt.energy_residual = 1;
        assert!(check_report(&config, &before, &after, &corrupt).is_err());
        let mut corrupt = report;
        corrupt.medium_matter[0] += 1;
        assert!(check_report(&config, &before, &after, &corrupt).is_err());
    }

    #[test]
    fn real_numerical_failure_retains_genesis_and_does_not_claim_a_checked_tick() {
        let text = scenario_text(|s| s.founder.genome.max_growth_rate_per_s = 1e20);
        let scenario = micro::config::parse(&text).unwrap();
        let config = micro::config::derive(&scenario, u64::MAX).unwrap();
        let genesis = MicroState::new(&config).unwrap();
        let artifact = batch(vec![run(text, 10)], Limits::default());
        let result = &artifact["runs"][0];
        assert_eq!(result["status"], "numerical_failure");
        assert_eq!(result["attempted_tick"], 1);
        assert_eq!(result["checked_ticks"], 0);
        assert_eq!(result["tick_reports_hashed"], 0);
        assert_eq!(result["tick_reports_digest"], full_hash(&[]));
        assert_eq!(result["attempt_residuals"], Value::Null);
        assert_eq!(
            result["summary"]["ledger"]["matter_residual_last_checked"],
            Value::Null
        );
        assert_eq!(
            result["final_state_digest"],
            state_digest(&genesis).unwrap()
        );
        assert_eq!(artifact["batch"]["attempted_cell_ticks"], "8");
        assert_eq!(artifact["batch"]["committed_cell_ticks"], "0");
    }

    #[test]
    fn derive_refusal_keeps_materialized_canonical_source_without_runtime_identity() {
        let text = scenario_text(|s| s.growth.enthalpy_j_per_extent = -1.0);
        let artifact = batch(vec![run(text, 10)], Limits::default());
        let result = &artifact["runs"][0];
        assert_eq!(result["status"], "refused");
        assert!(result["canonical_config"].is_string());
        assert_eq!(result["identity"]["derived"], false);
        assert_eq!(result["identity"]["matter_scales"], Value::Null);
        assert_eq!(result["summary"], Value::Null);
    }

    #[test]
    fn extinction_is_preserved_when_final_sample_does_not_fit() {
        let text = scenario_text(|s| {
            s.initial.concentration.insert("FOOD".into(), 0.0);
            s.medium.concentration.insert("FOOD".into(), 0.0);
        });
        let mut request = run(text, 1000);
        request.sample_every = 1000;
        let mut remaining = 1000;
        let mut prepared = prepare(&request, &Limits::default(), &mut remaining);
        let config = prepared.config.as_ref().unwrap();
        let genesis = MicroState::new(config).unwrap();
        let mut counters = Counters {
            medium_matter: vec![0; config.matter_ids.len()],
            ..Counters::default()
        };
        counters.observe(&genesis);
        let mut sample_bytes = bounded_size(
            &summary(config, &genesis, &counters).unwrap(),
            MAX_OUTPUT_BYTES,
        )
        .unwrap()
            + 1;
        observe(
            &request,
            config,
            &Limits::default(),
            &Instant::now(),
            &mut Work::default(),
            &mut sample_bytes,
            &mut prepared.result,
        )
        .unwrap();
        assert_eq!(prepared.result["status"], "extinct");
        assert_eq!(prepared.result["stop_reason"], "extinction");
        assert_eq!(prepared.result["samples_truncated"], true);
        assert_eq!(prepared.result["summary"]["living_cells"], 0);
    }

    #[test]
    fn output_sample_pressure_stops_explicitly_and_retains_bounded_summary() {
        let artifact = batch(
            vec![run(SOURCE.into(), 1000)],
            Limits {
                max_output_bytes: 100 * 1024,
                ..Limits::default()
            },
        );
        let result = &artifact["runs"][0];
        assert_eq!(result["stop_reason"], "output_sample_budget");
        assert_eq!(result["samples_truncated"], true);
        assert!(result["checked_ticks"].as_u64().unwrap() < 1000);
        assert!(result["summary"].is_object());
        assert!(bounded_size(&artifact, 100 * 1024).is_ok());
    }

    #[test]
    fn long_registry_ids_are_reserved_before_samples_spend_the_output_budget() {
        let text = scenario_text(|scenario| {
            let mut unused = scenario.substance[0].clone();
            unused.id = "unused".repeat(11_667);
            scenario.substance.push(unused);
        });
        assert!(text.len() <= MAX_TOML_BYTES);
        let config =
            micro::config::derive(&micro::config::parse(&text).unwrap(), u64::MAX).unwrap();
        assert_eq!(config.matter_ids.len(), 7);
        // Derive the constrained cap from the actual artifact shape, not the
        // reservation formula under test. Two retained samples plus the final
        // row just exceed it; a correct reserve must stop before spending that
        // final row's long registry ID on another sample.
        let mut roomy = batch(vec![run(text.clone(), 1000)], Limits::default());
        assert_eq!(roomy["runs"][0]["checked_ticks"], 1000);
        roomy["runs"][0]["samples"]
            .as_array_mut()
            .unwrap()
            .truncate(2);
        let constrained_bytes = bounded_size(&roomy, MAX_OUTPUT_BYTES).unwrap() - 512;
        let artifact = batch(
            vec![run(text, 1000)],
            Limits {
                max_output_bytes: constrained_bytes,
                ..Limits::default()
            },
        );
        assert_eq!(artifact["runs"].as_array().unwrap().len(), 1);
        let result = &artifact["runs"][0];
        assert_eq!(result["stop_reason"], "output_sample_budget");
        assert!(result["checked_ticks"].as_u64().unwrap() > 0);
        assert!(result["summary"].is_object());
        assert_eq!(result["samples_truncated"], true);
        assert!(bounded_size(&artifact, constrained_bytes).is_ok());
    }

    #[test]
    fn source_guard_rejects_short_wrong_dirty_or_untracked_evidence() {
        validate_source(COMMIT, COMMIT, true, true).unwrap();
        assert!(validate_source("0123456", COMMIT, true, true).is_err());
        assert!(
            validate_source(
                COMMIT,
                "abcdefabcdefabcdefabcdefabcdefabcdefabcd",
                true,
                true
            )
            .is_err()
        );
        assert!(validate_source(COMMIT, COMMIT, false, true).is_err());
        assert!(validate_source(COMMIT, COMMIT, true, false).is_err());
    }

    #[test]
    fn output_refusal_omits_canonical_text_for_derive_refusals_too() {
        let text = scenario_text(|scenario| {
            let mut unused = scenario.substance[0].clone();
            unused.id = "unused".repeat(11_667);
            scenario.substance.push(unused);
            scenario.growth.enthalpy_j_per_extent = -1.0;
        });
        let mut refusal = run(text, 10);
        refusal.run_id = "derive-refused".into();
        let artifact = batch(
            vec![run(SOURCE.into(), 10), refusal],
            Limits {
                max_output_bytes: 16 * 1024,
                ..Limits::default()
            },
        );
        assert_eq!(artifact["runs"].as_array().unwrap().len(), 2);
        for result in artifact["runs"].as_array().unwrap() {
            assert_eq!(result["checked_ticks"], 0);
            assert_eq!(result["canonical_config"], Value::Null);
            assert_eq!(
                result["canonical_config_omitted"],
                "output_reservation_budget"
            );
            assert!(result["identity"]["canonical_digest"].is_string());
        }
        assert_eq!(artifact["runs"][1]["identity"]["derived"], false);
        assert_eq!(
            artifact["runs"][1]["stop_reason"],
            "input_or_admission_refused"
        );
        assert!(bounded_size(&artifact, 16 * 1024).is_ok());
    }

    #[test]
    fn hard_caps_strict_manifest_and_bounded_writer_refuse_unsafe_inputs() {
        assert!(
            Limits {
                max_output_bytes: MAX_OUTPUT_BYTES + 1,
                ..Limits::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Limits {
                max_run_seconds: 61,
                ..Limits::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Limits {
                max_cell_ticks: MAX_CELL_TICKS + 1,
                ..Limits::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            parse_manifest(
                br#"{"schema_version":1,"batch_id":"x","runs":[],"extra":1}"#,
                &Limits::default()
            )
            .is_err()
        );
        assert!(parse_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1], &Limits::default()).is_err());
        let mut writer = CappedWriter::new(3);
        assert!(serde_json::to_writer(&mut writer, &json!("oversized")).is_err());
        assert!(writer.bytes.len() <= 3);
        let artifact = batch(
            vec![run(SOURCE.into(), 10)],
            Limits {
                max_output_bytes: 16 * 1024,
                ..Limits::default()
            },
        );
        assert_eq!(
            artifact["runs"][0]["stop_reason"],
            "output_reservation_budget"
        );
        assert_eq!(artifact["runs"][0]["checked_ticks"], 0);
        assert!(bounded_size(&artifact, 16 * 1024).is_ok());
    }
}
