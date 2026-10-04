//! A separate observer for real cells in an ideal well-mixed chamber.
//! The display has no authority over biology or physical coordinates.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use liminis_core::micro::{self, MicroConfig, MicroState};
use liminis_core::version::WORLD_FORMAT_VERSION;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::http::{Method, Request, Response};

#[path = "cells_storage.rs"]
mod storage;

const VIEWER: &str = include_str!("cell-viewer.html");
const DEFAULT_MULTIPLIER: f64 = 90.0;
const CONTROL_POLL: Duration = Duration::from_millis(10);
const SAMPLE_PERIOD: Duration = Duration::from_millis(250);
const EVENTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PaceMode {
    Manual,
    Maximum,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pacing {
    mode: PaceMode,
    multiplier: f64,
}

impl Pacing {
    fn manual(multiplier: f64, dt: f64) -> Result<Self> {
        ensure!(
            multiplier.is_finite() && multiplier > 0.0,
            "speed multiplier must be positive and finite"
        );
        let tps = multiplier / dt;
        ensure!(
            tps.is_finite() && tps > 0.0,
            "speed multiplier produces an unrepresentable tick rate for this dt"
        );
        Ok(Self {
            mode: PaceMode::Manual,
            multiplier,
        })
    }

    fn tps(self, dt: f64) -> f64 {
        self.multiplier / dt
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    births: u64,
    deaths: u64,
    divisions: u64,
    events: VecDeque<Value>,
    medium_matter: Vec<i128>,
    medium_energy: i128,
    growth_extent: i128,
    death_mass: i128,
}

struct Sim {
    config: MicroConfig,
    config_text: String,
    config_hash: String,
    seed: u64,
    world_format_version: u32,
    state: MicroState,
    observation: Observation,
    running: bool,
    alive: bool,
    error: Option<String>,
    residual: Option<(i128, i128)>,
    pacing: Pacing,
    target_tps: f64,
    pace_generation: u64,
    last_sample: Instant,
    measured_tps: f64,
    storage: Option<storage::CellsStorage>,
    transitioning: bool,
    save_requested: bool,
}

fn lock(shared: &Arc<Mutex<Sim>>) -> MutexGuard<'_, Sim> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

fn control_seed(command: &Value, previous: u64) -> Result<u64> {
    match command.get("seed") {
        None => Ok(previous),
        Some(Value::String(seed)) => seed.parse().context("seed must be a decimal u64 string"),
        _ => anyhow::bail!("seed must be a string to preserve its exact value"),
    }
}

fn event(sim: &mut Sim, value: Value) {
    sim.observation.events.push_back(value);
    while sim.observation.events.len() > EVENTS {
        sim.observation.events.pop_front();
    }
}

fn genome_key(cell: &micro::Cell) -> String {
    format!("K{:+}", cell.genome.kinetics)
}

fn resources(sim: &Sim) -> BTreeMap<String, f64> {
    sim.config
        .matter_ids
        .iter()
        .enumerate()
        .filter(|(s, _)| *s != sim.config.growth.biomass_substance)
        .map(|(s, id)| {
            (
                id.clone(),
                sim.state.matter[s] as f64
                    / sim.config.units_per_mol[s] as f64
                    / sim.config.volume_m3,
            )
        })
        .collect()
}

fn metric(sim: &Sim) -> storage::CellMetric {
    let bio = sim.config.growth.biomass_substance;
    let mass: i128 = sim.state.cells.iter().map(|c| c.mass).sum();
    let energy: i128 = sim.state.cells.iter().map(|c| c.energy).sum();
    let mut genomes = BTreeMap::new();
    for cell in &sim.state.cells {
        *genomes.entry(genome_key(cell)).or_insert(0) += 1;
    }
    let mut raw = BTreeMap::from([
        ("biomass_units".into(), mass.to_string()),
        ("cell_energy_units".into(), energy.to_string()),
        ("bath_heat_units".into(), sim.state.heat.to_string()),
        (
            "reaction_extent".into(),
            sim.observation.growth_extent.to_string(),
        ),
        (
            "death_mass_units".into(),
            sim.observation.death_mass.to_string(),
        ),
        (
            "medium_energy_units".into(),
            sim.observation.medium_energy.to_string(),
        ),
        ("next_cell_id".into(), sim.state.next_cell_id.to_string()),
    ]);
    for (s, id) in sim.config.matter_ids.iter().enumerate() {
        raw.insert(format!("pool.{id}"), sim.state.matter[s].to_string());
        raw.insert(
            format!("medium.{id}"),
            sim.observation.medium_matter[s].to_string(),
        );
    }
    storage::CellMetric {
        tick: sim.state.tick,
        sim_time: sim.state.tick as f64 * sim.config.dt_seconds,
        living_cells: sim.state.cells.len() as u64,
        births: sim.observation.births,
        deaths: sim.observation.deaths,
        biomass_mol: mass as f64 / sim.config.units_per_mol[bio] as f64,
        cell_energy_j: energy as f64 / sim.config.energy_units_per_joule as f64,
        resources: resources(sim),
        genomes,
        residual: sim.residual.map(|(matter, energy)| storage::CellResidual {
            matter: sim
                .config
                .matter_ids
                .iter()
                .map(|id| (id.clone(), matter.to_string()))
                .collect(),
            energy: energy.to_string(),
        }),
        raw_integer_statistics: raw,
    }
}

fn state_json(sim: &Sim) -> Value {
    let summary = metric(sim);
    let bio = sim.config.growth.biomass_substance;
    let limiting = sim.config.growth.limiting_substance;
    let bio_units = sim.config.units_per_mol[bio] as f64;
    let energy_units = sim.config.energy_units_per_joule as f64;
    let affinity_units = sim.config.units_per_mol[limiting] as f64;
    let cells: Vec<_> = sim.state.cells.iter().map(|cell| {
        let decoded = micro::config::decode_genome(&sim.config, &cell.genome);
        json!({"id":cell.id.to_string(), "parent_id":cell.parent_id.map(|id|id.to_string()),
            "generation":cell.generation,"birth_tick":cell.birth_tick,
            "age_s":cell.age as f64 * sim.config.dt_seconds,"genome_key":genome_key(cell),
            "mass_mol":cell.mass as f64 / bio_units,"energy_j":cell.energy as f64 / energy_units,
            "division_mass_mol":decoded.division_mass as f64 / bio_units,
            "starvation_s":f64::from(cell.starvation_ticks) * sim.config.dt_seconds,
            "genome":{"kinetics":cell.genome.kinetics,
                "base_growth_per_s":cell.genome.max_growth_rate_per_second,
                "base_km_mol_m3":cell.genome.affinity_km_amount as f64 / affinity_units / sim.config.volume_m3,
                "division_mass_mol":cell.genome.division_mass as f64 / bio_units,
                "maintenance_w":cell.genome.maintenance_energy_per_tick as f64 / energy_units / sim.config.dt_seconds,
                "capture_fraction":f64::from(cell.genome.capture_numerator)/f64::from(cell.genome.capture_denominator),
                "division_cost_j":cell.genome.division_energy_cost as f64 / energy_units,
                "starvation_tolerance_s":f64::from(cell.genome.starvation_tolerance_ticks)*sim.config.dt_seconds},
            "phenotype":{"growth_per_s":decoded.max_growth_rate_per_second,
                "km_mol_m3":decoded.affinity_km_amount / affinity_units / sim.config.volume_m3}
        })
    }).collect();
    let resources: Vec<_> = summary.resources.iter().map(|(id, concentration)| {
        json!({"id":id,"concentration":concentration,"amount_mol":concentration*sim.config.volume_m3})
    }).collect();
    json!({"kind":"cells","tick":sim.state.tick,"sim_time":summary.sim_time,
        "seed":sim.seed.to_string(),"scenario":sim.config.name,"config_hash":sim.config_hash,
        "world_format_version":sim.world_format_version,"chamber_format":sim.config.chamber_format,
        "running":sim.running,"alive":sim.alive,"pacing":sim.pacing,
        "dt_seconds":sim.config.dt_seconds,
        "target_tps":if sim.pacing.mode == PaceMode::Manual {Some(sim.target_tps)} else {None},
        "save_pending":sim.save_requested,
        "measured_tps":sim.measured_tps,
        "measured_multiplier":sim.measured_tps*sim.config.dt_seconds,"error":sim.error,
        "residual":sim.residual.map(|(matter,energy)|json!({"matter":matter.to_string(),"energy":energy.to_string()})),
        "persistence":sim.storage.as_ref().map(|s|s.status()),
        "model":{"environment":"well_mixed","volume_m3":sim.config.volume_m3,
            "temperature_k":sim.config.temperature_kelvin},
        "summary":{"living_cells":summary.living_cells,"births":summary.births,"deaths":summary.deaths,
            "divisions":sim.observation.divisions,
            "generation_max":sim.state.cells.iter().map(|c|c.generation).max().unwrap_or(0),
            "total_biomass_mol":summary.biomass_mol,"total_cell_energy_j":summary.cell_energy_j},
        "resources":resources,"cells":cells,"events":sim.observation.events})
}

fn build(text: &str, seed: u64) -> Result<Sim> {
    let scenario = micro::config::parse(text)?;
    let config_text = micro::config::canonical(&scenario)?;
    let config_hash = micro::config::config_hash(&scenario)?;
    let config = micro::config::derive(&scenario, seed)?;
    let state = MicroState::new(&config)?;
    let observation = Observation {
        medium_matter: vec![0; config.matter_ids.len()],
        ..Observation::default()
    };
    let pacing = Pacing::manual(DEFAULT_MULTIPLIER, config.dt_seconds)?;
    let target_tps = pacing.tps(config.dt_seconds);
    Ok(Sim {
        config,
        config_text,
        config_hash,
        seed,
        world_format_version: WORLD_FORMAT_VERSION,
        state,
        observation,
        running: true,
        alive: true,
        error: None,
        residual: None,
        pacing,
        target_tps,
        pace_generation: 0,
        last_sample: Instant::now(),
        measured_tps: 0.0,
        storage: None,
        transitioning: false,
        save_requested: false,
    })
}

struct Alive(Arc<Mutex<Sim>>);

impl Drop for Alive {
    fn drop(&mut self) {
        let mut sim = lock(&self.0);
        sim.alive = false;
        sim.running = false;
        sim.residual = None;
    }
}

fn run_loop(shared: &Arc<Mutex<Sim>>) {
    let _alive = Alive(Arc::clone(shared));
    let mut generation = None;
    let mut last_tick = Instant::now();
    let mut mark = Instant::now();
    let mut ticks = 0u64;
    loop {
        let sleep = {
            let mut sim = lock(shared);
            if !sim.alive {
                return;
            }
            service_storage(&mut sim);
            if generation != Some(sim.pace_generation) || !sim.running || sim.transitioning {
                generation = Some(sim.pace_generation);
                last_tick = Instant::now();
                mark = last_tick;
                ticks = 0;
                sim.measured_tps = 0.0;
            }
            if !sim.running || sim.transitioning {
                CONTROL_POLL
            } else {
                // Compare progress instead of constructing an astronomical
                // Duration or overflowing Instant for valid very slow rates.
                let due = sim.pacing.mode == PaceMode::Maximum
                    || last_tick.elapsed().as_secs_f64() * sim.target_tps >= 1.0;
                if due && safe_advance(&mut sim) {
                    ticks = ticks.saturating_add(1);
                    // Schedule from completion: missed wall time is never debt.
                    last_tick = Instant::now();
                }
                let elapsed = mark.elapsed().as_secs_f64();
                if !sim.running {
                    sim.measured_tps = 0.0;
                } else if elapsed > 0.0 {
                    sim.measured_tps = ticks as f64 / elapsed;
                }
                if sim.pacing.mode == PaceMode::Maximum {
                    Duration::ZERO
                } else {
                    let remaining =
                        (1.0 / sim.target_tps - last_tick.elapsed().as_secs_f64()).max(0.0);
                    Duration::from_secs_f64(remaining.min(CONTROL_POLL.as_secs_f64()))
                }
            }
        }; // Release the mutex after exactly one complete checked tick.
        if sleep.is_zero() {
            std::thread::yield_now();
        } else {
            std::thread::sleep(sleep);
        }
    }
}

fn advance_one(sim: &mut Sim) -> Result<()> {
    let parents: BTreeMap<_, _> = sim
        .state
        .cells
        .iter()
        .map(|cell| (cell.id, cell.clone()))
        .collect();
    let mut candidate = sim.state.clone();
    let report = micro::step(&sim.config, &mut candidate)?;
    let matter = report
        .matter_residual
        .iter()
        .copied()
        .max_by_key(|value| value.unsigned_abs())
        .unwrap_or(0);
    ensure!(
        matter == 0 && report.energy_residual == 0,
        "cell conservation ledger did not close"
    );
    let mut observation = sim.observation.clone();
    observation.births = observation
        .births
        .checked_add(u64::from(report.births))
        .context("birth counter exhausted")?;
    observation.deaths = observation
        .deaths
        .checked_add(u64::from(report.deaths))
        .context("death counter exhausted")?;
    observation.divisions = observation
        .divisions
        .checked_add(u64::from(report.fissions))
        .context("division counter exhausted")?;
    observation.medium_energy = observation
        .medium_energy
        .checked_add(report.medium_energy)
        .context("medium energy counter exhausted")?;
    observation.growth_extent = observation
        .growth_extent
        .checked_add(report.extent)
        .context("reaction extent counter exhausted")?;
    observation.death_mass = observation
        .death_mass
        .checked_add(report.death_mass)
        .context("death mass counter exhausted")?;
    for (total, delta) in observation
        .medium_matter
        .iter_mut()
        .zip(&report.medium_matter)
    {
        *total = total
            .checked_add(*delta)
            .context("medium matter counter exhausted")?;
    }
    sim.state = candidate;
    sim.observation = observation;
    sim.residual = Some((matter, report.energy_residual));
    let mut children: BTreeMap<u64, Vec<(u64, bool)>> = BTreeMap::new();
    for cell in &sim.state.cells {
        if !parents.contains_key(&cell.id)
            && let Some(parent) = cell.parent_id
        {
            let mutated = parents
                .get(&parent)
                .is_some_and(|p| p.genome != cell.genome);
            children.entry(parent).or_default().push((cell.id, mutated));
        }
    }
    let living: std::collections::BTreeSet<_> = sim.state.cells.iter().map(|c| c.id).collect();
    for parent in parents.values() {
        if let Some(child) = children.get(&parent.id) {
            event(
                sim,
                json!({"tick":sim.state.tick,"kind":"division","parent_id":parent.id.to_string(),
                "children":child.iter().map(|(id,_)|id.to_string()).collect::<Vec<_>>(),
                "mutated":child.iter().any(|(_,m)|*m)}),
            );
        } else if !living.contains(&parent.id) {
            event(
                sim,
                json!({"tick":sim.state.tick,"kind":"death","cell_id":parent.id.to_string()}),
            );
        }
    }
    Ok(())
}

fn safe_advance(sim: &mut Sim) -> bool {
    if sim.storage.as_ref().is_some_and(|s| s.error().is_some()) {
        sim.running = false;
        return false;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| advance_one(sim)));
    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(format!("{error:#}")),
        Err(error) => Some(
            error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "cell simulation stopped during a tick".into()),
        ),
    };
    if let Some(error) = error {
        sim.error = Some(error);
        sim.running = false;
        sim.alive = false;
        sim.residual = None;
        sim.measured_tps = 0.0;
        return false;
    }
    if let Some(mut storage) = sim.storage.take() {
        let result = if !sim.save_requested && storage.autosave_due() {
            storage.save(capture(sim))
        } else if !sim.save_requested && sim.last_sample.elapsed() >= SAMPLE_PERIOD {
            sim.last_sample = Instant::now();
            storage.sample(metric(sim)).map(|_| ())
        } else {
            Ok(())
        };
        sim.storage = Some(storage);
        if let Err(error) = result {
            if error.is::<storage::QueueBusy>() {
                sim.save_requested = true;
                return true;
            }
            sim.running = false;
            sim.measured_tps = 0.0;
            return false;
        }
    }
    true
}

fn route(shared: &Arc<Mutex<Sim>>, request: &Request) -> Response {
    match (request.path.as_str(), request.method) {
        ("/" | "/index.html", Method::Get) => Response::html(VIEWER),
        ("/api/state", Method::Get) => Response::json(state_json(&lock(shared)).to_string()),
        ("/api/history", Method::Get) => {
            let sim = lock(shared);
            match &sim.storage {
                Some(storage) => Response::json(
                    serde_json::to_string(&storage.history()).expect("finite cell history"),
                ),
                None => Response::error(503, "local history is not enabled"),
            }
        }
        ("/api/control", Method::Post) => control(shared, &request.body),
        ("/" | "/index.html" | "/api/state" | "/api/history" | "/api/control", _) => {
            Response::error(405, "unsupported method for this route")
        }
        _ => Response::error(404, "no such cell chamber route"),
    }
}

fn capture(sim: &Sim) -> storage::Capture {
    storage::Capture {
        tick: sim.state.tick,
        world_format_version: sim.world_format_version,
        running: sim.running && sim.alive,
        tps: sim.target_tps,
        state: json!({"format":2,"pacing":sim.pacing,"core":sim.state.snapshot(),"observation":sim.observation}),
        metric: sim.residual.map(|_| metric(sim)),
    }
}

fn save(sim: &mut Sim) -> Result<()> {
    ensure!(
        sim.alive && sim.error.is_none(),
        "cannot save an incomplete cell tick"
    );
    let capture = capture(sim);
    sim.storage
        .as_mut()
        .context("local storage is not enabled")?
        .save(capture)?;
    Ok(())
}

fn service_storage(sim: &mut Sim) {
    if let Some(storage) = &sim.storage {
        if storage.error().is_some() {
            sim.running = false;
            sim.measured_tps = 0.0;
        } else if sim.save_requested && !storage.busy() && sim.alive && !sim.transitioning {
            match save(sim) {
                Ok(()) => sim.save_requested = false,
                Err(error) if error.is::<storage::QueueBusy>() => {}
                Err(_) => sim.running = false,
            }
        }
    }
}

pub fn run(
    port: u16,
    path: &Path,
    seed: u64,
    data_dir: &Path,
    resume: Option<&str>,
    checkpoint: Option<&str>,
) -> Result<()> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    let sim = if let Some(run) = resume {
        resume_sim(data_dir, run, checkpoint)?
    } else {
        let mut sim = build(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading cell scenario {}", path.display()))?,
            seed,
        )?;
        start_storage(&mut sim, data_dir)?;
        sim
    };
    println!("Liminis cells on http://127.0.0.1:{port}/");
    println!(
        "seed={} config_hash={} world_format_version={} chamber_format=1",
        sim.seed, sim.config_hash, sim.world_format_version
    );
    let shared = Arc::new(Mutex::new(sim));
    let ticking = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("liminis-cells".into())
        .spawn(move || run_loop(&ticking))
        .context("starting the cell chamber")?;
    crate::http::serve(listener, move |request| route(&shared, request))?;
    Ok(())
}

fn start_storage(sim: &mut Sim, root: &Path) -> Result<()> {
    let mut storage = storage::CellsStorage::new_run(
        root,
        sim.config_text.clone(),
        sim.config_hash.clone(),
        sim.seed.to_string(),
        sim.world_format_version,
        sim.config.chamber_format,
    )?;
    storage.save(capture(sim))?;
    storage.wait_for_save()?;
    sim.storage = Some(storage);
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedState {
    format: u32,
    core: micro::MicroSnapshot,
    observation: Observation,
    #[serde(default)]
    pacing: Option<Pacing>,
}

fn restore_pacing(saved: &SavedState, tps: f64, dt: f64) -> Result<Pacing> {
    ensure!(tps.is_finite() && tps > 0.0, "saved cell speed is invalid");
    match saved.format {
        1 => {
            ensure!(
                saved.pacing.is_none(),
                "legacy envelope cannot contain pacing"
            );
            Pacing::manual(tps * dt, dt)
        }
        2 => {
            let pacing = saved.pacing.context("saved pacing is missing")?;
            Pacing::manual(pacing.multiplier, dt)?;
            ensure!(
                pacing.tps(dt) == tps || pacing.multiplier == tps * dt,
                "saved pacing differs from checkpoint tick rate"
            );
            Ok(pacing)
        }
        _ => anyhow::bail!("unsupported cell state envelope"),
    }
}

fn resume_sim(root: &Path, run: &str, checkpoint: Option<&str>) -> Result<Sim> {
    let prepared = storage::CellsStorage::prepare_resume(root, run, checkpoint)?;
    let saved = prepared.state();
    ensure!(
        saved.parent_tick == saved.capture.tick,
        "saved session boundary differs from checkpoint"
    );
    ensure!(
        saved.stored.kind == "cells"
            && saved.stored.chamber_format == 1
            && matches!(saved.stored.world_format_version, 29 | 30),
        "this build cannot restore this cell chamber identity"
    );
    let mut sim = build(&saved.stored.config_text, saved.stored.seed.parse()?)?;
    ensure!(
        sim.config_hash == saved.stored.config_hash,
        "saved cell config hash differs"
    );
    ensure!(
        saved.capture.world_format_version == saved.stored.world_format_version,
        "saved cell state world identity differs"
    );
    sim.world_format_version = saved.stored.world_format_version;
    let snapshot: SavedState = serde_json::from_value(saved.capture.state.clone())?;
    sim.pacing = restore_pacing(&snapshot, saved.capture.tps, sim.config.dt_seconds)?;
    sim.target_tps = saved.capture.tps;
    sim.state = MicroState::from_snapshot(&sim.config, snapshot.core)?;
    ensure!(
        sim.state.tick == saved.capture.tick,
        "cell state tick differs from checkpoint"
    );
    ensure!(
        snapshot.observation.events.len() <= EVENTS,
        "saved observer event window exceeds its bound"
    );
    for event in &snapshot.observation.events {
        ensure!(
            event["tick"]
                .as_u64()
                .is_some_and(|tick| tick <= sim.state.tick),
            "saved event is outside the completed history"
        );
    }
    sim.observation = snapshot.observation;
    validate_observation(&sim)?;
    sim.running = saved.capture.running;
    sim.residual = None;
    println!(
        "restoring cell checkpoint {} at tick {}",
        saved.checkpoint_id, sim.state.tick
    );
    let mut storage = prepared.start()?;
    storage.save(capture(&sim))?;
    storage.wait_for_save()?;
    sim.storage = Some(storage);
    Ok(sim)
}

fn validate_observation(sim: &Sim) -> Result<()> {
    let observation = &sim.observation;
    let config = &sim.config;
    ensure!(
        observation.medium_matter.len() == config.matter_ids.len(),
        "saved medium ledger shape differs"
    );
    ensure!(
        observation.growth_extent >= 0 && observation.death_mass >= 0,
        "saved reaction extent is negative"
    );
    ensure!(
        observation.divisions.checked_mul(2) == Some(observation.births),
        "saved birth and division counters disagree"
    );
    ensure!(
        u64::from(config.founder.count).checked_add(observation.births)
            == Some(sim.state.next_cell_id),
        "saved next cell ID disagrees with actual birth history"
    );
    let expected_cells = i128::from(config.founder.count) + i128::from(observation.births)
        - i128::from(observation.divisions)
        - i128::from(observation.deaths);
    ensure!(
        expected_cells == sim.state.cells.len() as i128,
        "saved cell lifecycle counters disagree"
    );
    let bio = config.growth.biomass_substance;
    let det = config.death.detritus_substance;
    let founder_mass = config
        .founder
        .mass
        .checked_mul(i128::from(config.founder.count))
        .context("founder total overflows")?;
    let mut initial = config.initial_matter.clone();
    initial[bio] = initial[bio]
        .checked_add(founder_mass)
        .context("initial BIO sum overflows")?;
    let mut expected = initial.clone();
    for (total, credit) in expected.iter_mut().zip(&observation.medium_matter) {
        *total = total
            .checked_add(*credit)
            .context("medium ledger overflows")?;
    }
    for nu in &config.growth.nu {
        expected[nu.substance] = expected[nu.substance]
            .checked_add(
                i128::from(nu.value)
                    .checked_mul(observation.growth_extent)
                    .context("reaction ledger product overflows")?,
            )
            .context("reaction ledger sum overflows")?;
    }
    expected[bio] = expected[bio]
        .checked_sub(observation.death_mass)
        .context("lysis BIO ledger overflows")?;
    expected[det] = expected[det]
        .checked_add(observation.death_mass)
        .context("lysis DET ledger overflows")?;
    let mut actual = sim.state.matter.clone();
    for cell in &sim.state.cells {
        actual[bio] = actual[bio]
            .checked_add(cell.mass)
            .context("cell mass sum overflows")?;
    }
    ensure!(
        actual == expected,
        "saved cumulative matter ledger does not close"
    );
    let chemical = |matter: &[i128]| -> Result<i128> {
        matter
            .iter()
            .zip(&config.chemical_weights)
            .try_fold(0i128, |sum, (amount, weight)| {
                sum.checked_add(
                    amount
                        .checked_mul(i128::from(*weight))
                        .context("chemical energy product overflows")?,
                )
                .context("chemical energy sum overflows")
            })
    };
    let initial_energy = chemical(&initial)?
        .checked_add(config.initial_bath_heat)
        .and_then(|sum| {
            config
                .founder
                .energy
                .checked_mul(i128::from(config.founder.count))
                .and_then(|energy| sum.checked_add(energy))
        })
        .context("initial energy sum overflows")?;
    let internal = sim.state.cells.iter().try_fold(0i128, |sum, cell| {
        sum.checked_add(cell.energy)
            .context("cell energy sum overflows")
    })?;
    let actual_energy = chemical(&actual)?
        .checked_add(sim.state.heat)
        .and_then(|sum| sum.checked_add(internal))
        .context("total energy sum overflows")?;
    ensure!(
        initial_energy.checked_add(observation.medium_energy) == Some(actual_energy),
        "saved cumulative energy ledger does not close"
    );
    Ok(())
}

fn reset(shared: &Arc<Mutex<Sim>>, command: &Value) -> Response {
    let (config_text, seed, pacing, tps, generation, running, root) = {
        let mut sim = lock(shared);
        if sim.transitioning || sim.storage.as_ref().is_some_and(|s| s.busy()) {
            return Response::error(409, "wait for the pending checkpoint before resetting");
        }
        let seed = match control_seed(command, sim.seed) {
            Ok(seed) => seed,
            Err(error) => return Response::error(400, &error.to_string()),
        };
        let Some(storage) = &sim.storage else {
            return Response::error(503, "local storage is not enabled");
        };
        let root = storage.root().to_path_buf();
        let running = sim.running;
        sim.running = false;
        sim.measured_tps = 0.0;
        if sim.alive
            && let Err(error) = save(&mut sim)
        {
            return Response::error(503, &format!("saving previous chamber: {error:#}"));
        }
        sim.transitioning = true;
        (
            sim.config_text.clone(),
            seed,
            sim.pacing,
            sim.target_tps,
            sim.pace_generation.wrapping_add(1),
            running,
            root,
        )
    };
    // No large file writes or construction hold the active simulation lock.
    let replacement = (|| -> Result<Sim> {
        // The bounded worker completes the previous save before publishing the
        // new chamber. It owns the previous run, which remains intact on disk.
        loop {
            let status = {
                let sim = lock(shared);
                let storage = sim.storage.as_ref().unwrap();
                (storage.busy(), storage.error())
            };
            if let Some(error) = status.1 {
                anyhow::bail!("{error}");
            }
            if !status.0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut fresh = build(&config_text, seed)?;
        fresh.running = running;
        fresh.pacing = pacing;
        fresh.target_tps = tps;
        fresh.pace_generation = generation;
        start_storage(&mut fresh, &root)?;
        Ok(fresh)
    })();
    let mut sim = lock(shared);
    match replacement {
        Ok(fresh) => {
            *sim = fresh;
            Response::json(state_json(&sim).to_string())
        }
        Err(error) => {
            sim.transitioning = false;
            Response::error(
                503,
                &format!("new chamber failed; previous chamber is retained on pause: {error:#}"),
            )
        }
    }
}

fn control(shared: &Arc<Mutex<Sim>>, body: &[u8]) -> Response {
    let command: Value = match serde_json::from_slice(body) {
        Ok(command) => command,
        Err(_) => return Response::error(400, "control must be a JSON object"),
    };
    if command["action"] == "reset" {
        return reset(shared, &command);
    }
    let mut sim = lock(shared);
    if sim.transitioning {
        return Response::error(409, "a new experiment is being created");
    }
    if !sim.alive || sim.error.is_some() {
        return Response::error(503, "this chamber has stopped; start a new experiment");
    }
    if sim.storage.as_ref().is_some_and(|s| s.error().is_some()) {
        return Response::error(503, "local storage failed; simulation remains stopped");
    }
    match command.get("action").and_then(Value::as_str) {
        Some("run" | "play") => {
            sim.running = true;
            sim.measured_tps = 0.0;
            sim.pace_generation = sim.pace_generation.wrapping_add(1);
        }
        Some("pause") => {
            sim.running = false;
            sim.pace_generation = sim.pace_generation.wrapping_add(1);
            sim.measured_tps = 0.0;
            sim.save_requested = true;
            service_storage(&mut sim);
        }
        Some("step") => {
            sim.running = false;
            sim.pace_generation = sim.pace_generation.wrapping_add(1);
            sim.measured_tps = 0.0;
            if !safe_advance(&mut sim) {
                return Response::error(503, sim.error.as_deref().unwrap_or("storage failed"));
            }
            sim.save_requested = true;
            service_storage(&mut sim);
        }
        Some("speed") => {
            let Some(value) = command.get("value").and_then(Value::as_f64) else {
                return Response::error(400, "speed value must be a number");
            };
            let pacing = match Pacing::manual(value, sim.config.dt_seconds) {
                Ok(pacing) => pacing,
                Err(error) => return Response::error(400, &error.to_string()),
            };
            sim.pacing = pacing;
            sim.target_tps = pacing.tps(sim.config.dt_seconds);
            sim.pace_generation = sim.pace_generation.wrapping_add(1);
            sim.measured_tps = 0.0;
        }
        Some("maximum") => {
            sim.pacing.mode = PaceMode::Maximum;
            sim.pace_generation = sim.pace_generation.wrapping_add(1);
            sim.measured_tps = 0.0;
        }
        Some("save") => {
            if sim.storage.as_ref().is_some_and(|s| s.busy()) {
                return Response::error(409, "a checkpoint is already being saved");
            }
            if let Err(error) = save(&mut sim) {
                if error.is::<storage::QueueBusy>() {
                    return Response::error(409, "cell storage queue is busy; retry save");
                }
                return Response::error(503, &format!("saving the chamber: {error:#}"));
            }
        }
        _ => return Response::error(400, "unknown cell chamber command"),
    }
    Response::json(state_json(&sim).to_string())
}

#[cfg(test)]
#[path = "cells_host_tests.rs"]
mod tests;
