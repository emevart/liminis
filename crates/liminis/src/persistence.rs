//! Local experiment storage. Capture is between ticks; all large file writes
//! happen on one bounded worker, never under the simulation mutex (ADR-096).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use liminis_core::observe::{
    self, FieldStat, MetricsWriter, RunHeader, SnapshotIdentity, TickMetrics,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{LaneRef, Sim, WORLD_FORMAT_VERSION, build};

const FORMAT: u32 = 1;
pub(super) const AUTOSAVE: Duration = Duration::from_secs(60);
const SAMPLE_EVERY: u32 = 30;
const HISTORY_LIMIT: usize = 360;
const RETAIN_CHECKPOINTS: usize = 3;

#[derive(Clone, Serialize, Deserialize)]
struct RunInfo {
    format: u32,
    run_id: String,
    scenario: String,
    seed: String,
    config_hash: String,
    world_format_version: u32,
    created_at: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct SessionInfo {
    format: u32,
    session_id: String,
    parent_session: Option<String>,
    parent_tick: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CheckpointInfo {
    format: u32,
    checkpoint_id: String,
    session_id: String,
    seed: String,
    config_hash: String,
    world_format_version: u32,
    tick: u32,
    running: bool,
    target_tps: f64,
    first_seen: BTreeMap<String, Option<u32>>,
    saved_at: u64,
    state_bytes: u64,
    state_hash: String,
}

struct Lease {
    root: PathBuf,
    path: PathBuf,
    info: RunInfo,
    // OS lock, not a PID sentinel: a crash releases it automatically.
    _lock: File,
}

#[derive(Clone, Serialize)]
struct Sample {
    tick: u32,
    biomass: f64,
    shares: Vec<f64>,
    resources: BTreeMap<String, f64>,
}

#[derive(Default)]
struct History {
    anchors: Vec<(u64, Sample)>,
    last: Option<Sample>,
    count: u64,
    stride: u64,
}

impl History {
    fn push(&mut self, sample: Sample) -> Result<()> {
        if let Some(last) = &self.last {
            ensure!(
                sample.tick >= last.tick,
                "history ticks went backwards within a lineage"
            );
            if sample.tick == last.tick {
                return Ok(());
            }
        }
        self.stride = self.stride.max(1);
        let ordinal = self.count;
        self.count += 1;
        if ordinal.is_multiple_of(self.stride) {
            self.anchors.push((ordinal, sample.clone()));
        }
        if self.anchors.len() >= HISTORY_LIMIT {
            self.stride *= 2;
            self.anchors.retain(|(n, _)| n.is_multiple_of(self.stride));
        }
        self.last = Some(sample);
        Ok(())
    }

    fn samples(&self) -> Vec<Sample> {
        let mut out: Vec<_> = self.anchors.iter().map(|(_, s)| s.clone()).collect();
        if let Some(last) = &self.last
            && out.last().is_none_or(|s| s.tick != last.tick)
        {
            out.push(last.clone());
        }
        out
    }
}

#[derive(Default)]
struct State {
    saving: bool,
    saved: Option<CheckpointInfo>,
    error: Option<String>,
    capture_bytes: usize,
    capture_ms: f64,
    history: History,
}

struct Job {
    metric: Option<Vec<u8>>,
    checkpoint: Option<(Vec<u8>, CheckpointInfo)>,
}

struct Model {
    fields: Vec<(String, u8)>,
    ecotypes: Vec<String>,
    volume: f64,
    columns: Vec<String>,
}

impl Model {
    fn of(sim: &Sim) -> Self {
        let ids: Vec<_> = sim.fields.iter().map(|f| f.id.as_str()).collect();
        Self {
            fields: sim.fields.iter().map(|f| (f.id.clone(), f.k)).collect(),
            ecotypes: sim.ecology.ids(),
            volume: f64::from(sim.world.grid().n_voxels()) * sim.v_voxel,
            columns: observe::columns(&ids, &ids),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<Sample> {
        let record: BTreeMap<String, Value> = serde_json::from_slice(bytes)?;
        ensure!(
            record.get("record").and_then(Value::as_str) == Some("tick"),
            "history record is not a tick"
        );
        // arbitrary_precision preserves the decimal token. Only after exact
        // i128 parsing do derived chart values take the permitted float door.
        let integer = |key: &str| -> Result<i128> {
            record
                .get(key)
                .context(format!("missing metric {key}"))?
                .to_string()
                .parse::<i128>()
                .with_context(|| format!("metric {key} is not an exact integer"))
        };
        ensure!(
            record.len() == self.columns.len() + 1,
            "history metric roster differs"
        );
        for column in &self.columns {
            integer(column)?;
        }
        let tick = u32::try_from(integer("tick")?)?;
        let mut mol = BTreeMap::new();
        for (id, k) in &self.fields {
            let total = integer(&format!("total.{id}"))?;
            ensure!(total >= 0, "negative substance total in history");
            integer(&format!("residual.{id}"))?;
            mol.insert(id.clone(), total as f64 / f64::from(*k).exp2());
        }
        integer("residual.energy")?;
        let biomass: f64 = self.ecotypes.iter().map(|id| mol[id]).sum();
        let shares = self
            .ecotypes
            .iter()
            .map(|id| {
                if biomass > 0.0 {
                    mol[id] / biomass
                } else {
                    0.0
                }
            })
            .collect();
        let resources = mol
            .into_iter()
            .filter(|(id, _)| !self.ecotypes.contains(id))
            .map(|(id, total)| (id, total / self.volume))
            .collect();
        Ok(Sample {
            tick,
            biomass,
            shares,
            resources,
        })
    }
}

pub(super) struct Persistence {
    lease: Arc<Lease>,
    session: SessionInfo,
    sender: mpsc::SyncSender<Job>,
    state: Arc<Mutex<State>>,
    last_capture: Instant,
    last_sample: Option<u32>,
}

impl Persistence {
    pub(super) fn create(root: &Path, sim: &Sim) -> Result<Self> {
        fs::create_dir_all(root)
            .with_context(|| format!("creating experiment storage {}", root.display()))?;
        let root = fs::canonicalize(root)?;
        let (id, path) = create_directory(&root, "run")?;
        let lock = acquire(&path)?;
        let info = RunInfo {
            format: FORMAT,
            run_id: id,
            scenario: sim.identity.scenario.clone(),
            seed: sim.identity.seed.to_string(),
            config_hash: sim.identity.config_hash.clone(),
            world_format_version: WORLD_FORMAT_VERSION,
            created_at: now(),
        };
        let lease = Arc::new(Lease {
            root,
            path,
            info,
            _lock: lock,
        });
        let mut storage = Self::start(lease, sim, None, History::default(), true)?;
        storage.save(sim, false)?;
        Ok(storage)
    }

    fn start(
        lease: Arc<Lease>,
        sim: &Sim,
        parent: Option<&CheckpointInfo>,
        history: History,
        new_run: bool,
    ) -> Result<Self> {
        let sessions = lease.path.join("sessions");
        fs::create_dir_all(&sessions)?;
        let (session_id, session_path) = create_directory(&sessions, "session")?;
        let session = SessionInfo {
            format: FORMAT,
            session_id,
            parent_session: parent.map(|p| p.session_id.clone()),
            parent_tick: parent.map(|p| p.tick),
        };
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let state = Arc::new(Mutex::new(State {
            saved: parent.cloned(),
            history,
            ..State::default()
        }));
        let header = metrics(sim, false)?;
        let canonical = liminis_core::config::canonical(&sim.scenario)?;
        let worker_lease = Arc::clone(&lease);
        let worker_state = Arc::clone(&state);
        let worker_session = session.clone();
        let model = Model::of(sim);
        std::thread::Builder::new()
            .name("liminis-storage".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    if new_run {
                        write_new(&worker_lease.path.join("config.toml"), canonical.as_bytes())?;
                        write_json_new(&worker_lease.path.join("run.json"), &worker_lease.info)?;
                    }
                    write_json_new(&session_path.join("session.json"), &worker_session)?;
                    let mut stream = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(session_path.join("metrics.ndjson"))?;
                    stream.write_all(&header)?;
                    stream.sync_all()?;
                    while let Ok(job) = receiver.recv() {
                        if let Some(metric) = job.metric {
                            let sample = model.decode(&metric)?;
                            stream.write_all(&metric)?;
                            stream.sync_data()?;
                            state_lock(&worker_state).history.push(sample)?;
                        }
                        if let Some((bytes, mut info)) = job.checkpoint {
                            let checkpoints = worker_lease.path.join("checkpoints");
                            fs::create_dir_all(&checkpoints)?;
                            let (id, path) = create_directory(&checkpoints, "checkpoint")?;
                            info.checkpoint_id = id;
                            info.saved_at = now();
                            info.state_hash = blake3::hash(&bytes).to_hex().to_string();
                            write_new(&path.join("state.limsnap"), &bytes)?;
                            write_json_new(&path.join("metadata.json"), &info)?;
                            // A manifest is the commit point; incomplete generations
                            // are never candidates for implicit recovery.
                            atomic_json(&worker_lease.path.join("latest.json"), &info)?;
                            write_new(&path.join("published"), b"")?;
                            prune_checkpoints(&checkpoints, &info.checkpoint_id)?;
                            let mut s = state_lock(&worker_state);
                            s.saved = Some(info);
                            s.saving = false;
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    let mut s = state_lock(&worker_state);
                    s.error = Some(format!("local storage failed: {error:#}"));
                    s.saving = false;
                }
            })
            .context("starting the experiment storage worker")?;
        Ok(Self {
            lease,
            session,
            sender,
            state,
            last_capture: Instant::now(),
            last_sample: None,
        })
    }

    pub(super) fn root(&self) -> &Path {
        &self.lease.root
    }

    pub(super) fn busy(&self) -> bool {
        state_lock(&self.state).saving
    }

    pub(super) fn error(&self) -> Option<String> {
        state_lock(&self.state).error.clone()
    }

    pub(super) fn waiter(&self) -> impl FnOnce() -> Result<()> + use<> {
        let state = Arc::clone(&self.state);
        move || wait_for_state(&state)
    }

    pub(super) fn status(&self) -> Value {
        let s = state_lock(&self.state);
        json!({"enabled": true, "run_id": self.lease.info.run_id, "session_id": self.session.session_id,
            "saved_tick": s.saved.as_ref().map(|p| p.tick), "saved_at": s.saved.as_ref().map(|p| p.saved_at),
            "saving": s.saving, "error": s.error, "autosave_seconds": AUTOSAVE.as_secs(),
            "retained_checkpoints": RETAIN_CHECKPOINTS,
            "capture_bytes": s.capture_bytes, "capture_ms": s.capture_ms, "history_samples": s.history.count})
    }

    pub(super) fn history(&self) -> Value {
        let s = state_lock(&self.state);
        let samples = s.history.samples();
        json!({"run_id": self.lease.info.run_id, "session_id": self.session.session_id,
            "total_samples": s.history.count, "truncated": s.history.count > samples.len() as u64,
            "samples": samples})
    }

    pub(super) fn save(&mut self, sim: &Sim, observe: bool) -> Result<()> {
        ensure!(!self.busy(), "a checkpoint is already being saved");
        if let Some(error) = self.error() {
            bail!("{error}");
        }
        let started = Instant::now();
        let mut bytes = Vec::new();
        observe::snapshot_write(&mut bytes, &snapshot_identity(sim), &sim.world, &sim.ledger)?;
        let info = CheckpointInfo {
            format: FORMAT,
            checkpoint_id: String::new(),
            session_id: self.session.session_id.clone(),
            seed: sim.identity.seed.to_string(),
            config_hash: sim.identity.config_hash.clone(),
            world_format_version: WORLD_FORMAT_VERSION,
            tick: sim.ticks,
            running: sim.running && sim.alive,
            target_tps: sim.target_tps,
            first_seen: sim.ecology.first_seen(),
            saved_at: 0,
            state_bytes: bytes.len() as u64,
            state_hash: String::new(),
        };
        let metric = if observe && sim.last.is_some() && self.last_sample != Some(sim.ticks) {
            Some(metrics(sim, true)?)
        } else {
            None
        };
        {
            let mut s = state_lock(&self.state);
            s.saving = true;
            s.capture_bytes = bytes.len();
            s.capture_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if let Err(error) = self.sender.try_send(Job {
            metric,
            checkpoint: Some((bytes, info)),
        }) {
            let mut s = state_lock(&self.state);
            s.saving = false;
            s.error = Some(format!("storage queue unavailable: {error}"));
            bail!("{}", s.error.as_deref().unwrap());
        }
        if observe && sim.last.is_some() {
            self.last_sample = Some(sim.ticks);
        }
        self.last_capture = Instant::now();
        Ok(())
    }

    pub(super) fn sample(&mut self, sim: &Sim) -> Result<()> {
        if sim.last.is_none() || self.last_sample == Some(sim.ticks) {
            return Ok(());
        }
        if let Err(error) = self.sender.try_send(Job {
            metric: Some(metrics(sim, true)?),
            checkpoint: None,
        }) {
            let message = format!("the bounded history queue is unavailable: {error}");
            state_lock(&self.state).error = Some(message.clone());
            bail!("{message}");
        }
        self.last_sample = Some(sim.ticks);
        Ok(())
    }

    pub(super) fn after_tick(&mut self, sim: &Sim) -> Result<()> {
        if let Some(error) = self.error() {
            bail!("{error}");
        }
        if !self.busy() && self.last_capture.elapsed() >= AUTOSAVE {
            self.save(sim, true)?;
        } else if sim.ticks.is_multiple_of(SAMPLE_EVERY) {
            self.sample(sim)?;
        }
        Ok(())
    }

    pub(super) fn wait_for_save(&mut self) -> Result<()> {
        wait_for_state(&self.state)
    }
}

pub(super) fn resume(root: &Path, run: &str, checkpoint: Option<&str>) -> Result<Sim> {
    let root = fs::canonicalize(root).context("opening local experiment storage")?;
    let run_id = if run == "latest" {
        latest_run(&root)?
    } else {
        checked_id(run)?.to_string()
    };
    let path = root.join(&run_id);
    let lock = acquire(&path)?;
    let info: RunInfo = read_json(&path.join("run.json"))?;
    ensure!(
        info.format == FORMAT && info.run_id == run_id,
        "unsupported or mismatched run manifest"
    );
    let lease = Arc::new(Lease {
        root,
        path,
        info,
        _lock: lock,
    });
    let latest: CheckpointInfo = if let Some(id) = checkpoint {
        let info: CheckpointInfo = read_json(
            &lease
                .path
                .join("checkpoints")
                .join(checked_id(id)?)
                .join("metadata.json"),
        )?;
        ensure!(
            info.checkpoint_id == id,
            "requested checkpoint identifier differs from its metadata"
        );
        info
    } else {
        read_json(&lease.path.join("latest.json"))?
    };
    ensure!(latest.format == FORMAT, "unsupported checkpoint envelope");
    let generation = lease
        .path
        .join("checkpoints")
        .join(checked_id(&latest.checkpoint_id)?);
    let published: CheckpointInfo = read_json(&generation.join("metadata.json"))?;
    ensure!(
        serde_json::to_vec(&latest)? == serde_json::to_vec(&published)?,
        "checkpoint manifest and generation disagree"
    );
    ensure!(
        latest.seed == lease.info.seed
            && latest.config_hash == lease.info.config_hash
            && latest.world_format_version == lease.info.world_format_version,
        "checkpoint identity differs from run manifest"
    );
    ensure!(
        latest.world_format_version == WORLD_FORMAT_VERSION,
        "saved world version is not supported by this build"
    );
    ensure!(
        latest.target_tps.is_finite()
            && (super::MIN_TPS..=super::MAX_TPS).contains(&latest.target_tps),
        "checkpoint rate is invalid"
    );
    let config = liminis_core::config::load(&lease.path.join("config.toml"))?;
    let seed = lease
        .info
        .seed
        .parse::<u64>()
        .context("saved seed is not u64")?;
    let mut sim = build(&config, seed)?;
    let bytes = fs::read(generation.join("state.limsnap"))?;
    ensure!(
        bytes.len() as u64 == latest.state_bytes,
        "checkpoint state length differs"
    );
    ensure!(
        blake3::hash(&bytes).to_hex().as_str() == latest.state_hash,
        "checkpoint state checksum differs"
    );
    let mut cursor = Cursor::new(&bytes);
    sim.ticks = observe::snapshot_read_into(
        &mut cursor,
        &snapshot_identity(&sim),
        &mut sim.world,
        &mut sim.ledger,
    )?;
    ensure!(
        cursor.position() == bytes.len() as u64,
        "checkpoint has trailing bytes"
    );
    ensure!(
        sim.ticks == latest.tick,
        "checkpoint tick metadata differs from binary state"
    );
    sim.ecology
        .restore_first_seen(&latest.first_seen, sim.ticks)?;
    sim.target_tps = latest.target_tps;
    sim.running = latest.running;
    sim.last = None;
    let history = load_history(&lease, &latest, &Model::of(&sim))?;
    let mut persistence = Persistence::start(lease, &sim, Some(&latest), history, false)?;
    // A fresh segment is durable before accepting requests. This checkpoint has
    // the same physics and tick but identifies the new observer session.
    persistence.save(&sim, false)?;
    persistence.wait_for_save()?;
    sim.persistence = Some(persistence);
    Ok(sim)
}

fn snapshot_identity(sim: &Sim) -> SnapshotIdentity {
    SnapshotIdentity {
        seed: sim.identity.seed,
        config_hash: sim.identity.config_hash.clone(),
        world_format_version: sim.identity.world_format_version,
        tick: sim.ticks,
    }
}

fn metrics(sim: &Sim, tick_record: bool) -> Result<Vec<u8>> {
    let ids: Vec<_> = sim.fields.iter().map(|f| f.id.as_str()).collect();
    let columns = observe::columns(&ids, &ids);
    let columns: Vec<_> = columns.iter().map(String::as_str).collect();
    let mut bytes = Vec::new();
    let mut writer = MetricsWriter::new(
        &mut bytes,
        &RunHeader {
            seed: sim.identity.seed,
            config_hash: &sim.identity.config_hash,
            world_format_version: sim.identity.world_format_version,
            toolchain: concat!("pinned: ", include_str!("../../../rust-toolchain.toml")),
            code_version: env!("CARGO_PKG_VERSION"),
            metrics_schema_version: observe::METRICS_SCHEMA_VERSION,
            columns: &columns,
        },
    )?;
    if tick_record {
        ensure!(
            sim.last.is_some(),
            "cannot invent metrics for a tick that was not checked"
        );
        let totals: Vec<_> = (0..sim.after.n_substances())
            .map(|s| sim.after.matter(s))
            .collect();
        let residuals: Vec<_> = (0..sim.after.n_substances())
            .map(|s| {
                sim.ledger.residual_matter(
                    sim.tick.reaction_nu(sim.ticks.wrapping_sub(1)),
                    s,
                    &sim.before,
                    &sim.after,
                )
            })
            .collect();
        let stats: Vec<_> = sim
            .fields
            .iter()
            .enumerate()
            .map(|(s, f)| {
                let n = sim.world.grid().n_voxels() as usize;
                let extrema = match f.lane {
                    LaneRef::Narrow(lane) => sim.world.amounts_32().unwrap().lane(lane)[..n]
                        .iter()
                        .map(|v| i128::from(v.to_i64()))
                        .fold((i128::MAX, i128::MIN), |(min, max), v| {
                            (min.min(v), max.max(v))
                        }),
                    LaneRef::Wide(lane) => sim.world.amounts_64().unwrap().lane(lane)[..n]
                        .iter()
                        .map(|v| i128::from(v.to_i64()))
                        .fold((i128::MAX, i128::MIN), |(min, max), v| {
                            (min.min(v), max.max(v))
                        }),
                };
                FieldStat {
                    name: &f.id,
                    min: extrema.0,
                    max: extrema.1,
                    sum: totals[s],
                    count: n as u64,
                }
            })
            .collect();
        writer.tick(&TickMetrics {
            tick: sim.ticks,
            substance_id: &ids,
            substance_total: &totals,
            energy_total: sim.after.energy(),
            channel_matter: &observe::channel_matter_totals(&sim.ledger),
            channel_energy: &observe::channel_energy_totals(&sim.ledger),
            residual_matter: &residuals,
            residual_energy: sim.ledger.residual_energy(&sim.before, &sim.after),
            n_cells: 0,
            n_organisms: 0,
            field: &stats,
            tick_nanos: sim.last_tick_nanos,
        })?;
    }
    drop(writer);
    if tick_record {
        let split = bytes
            .iter()
            .position(|b| *b == b'\n')
            .context("metric header has no newline")?;
        Ok(bytes.split_off(split + 1))
    } else {
        Ok(bytes)
    }
}

fn load_history(lease: &Lease, checkpoint: &CheckpointInfo, model: &Model) -> Result<History> {
    let mut chain = Vec::new();
    let mut visited = BTreeSet::new();
    let mut session_id = Some(checkpoint.session_id.clone());
    let mut through = checkpoint.tick;
    while let Some(id) = session_id {
        ensure!(
            visited.len() < 10_000 && visited.insert(id.clone()),
            "history session lineage is cyclic or too long"
        );
        let path = lease.path.join("sessions").join(checked_id(&id)?);
        let session: SessionInfo = read_json(&path.join("session.json"))?;
        ensure!(
            session.format == FORMAT && session.session_id == id,
            "session manifest differs"
        );
        ensure!(
            session.parent_session.is_some() == session.parent_tick.is_some(),
            "session parent is incomplete"
        );
        chain.push((path, through, session.parent_tick));
        session_id = session.parent_session;
        if let Some(tick) = session.parent_tick {
            ensure!(tick <= through, "history parent tick is in the future");
            through = tick;
        }
    }
    let mut history = History::default();
    for (path, through, after) in chain.into_iter().rev() {
        let mut reader = BufReader::new(File::open(path.join("metrics.ndjson"))?);
        let mut line = Vec::new();
        let mut header = true;
        let mut previous = None;
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            if line.last() != Some(&b'\n') {
                break;
            } // Only an interrupted tail is ignorable.
            if header {
                let value: Value = serde_json::from_slice(&line)?;
                ensure!(
                    value["record"] == "header"
                        && value["config_hash"] == lease.info.config_hash
                        && value["seed"].as_u64() == Some(lease.info.seed.parse::<u64>()?)
                        && value["world_format_version"] == lease.info.world_format_version,
                    "history header belongs to another experiment"
                );
                ensure!(
                    value["metrics_schema_version"] == observe::METRICS_SCHEMA_VERSION
                        && value["columns"] == json!(model.columns),
                    "history header metric roster differs"
                );
                header = false;
            } else {
                let sample = model
                    .decode(&line)
                    .context("invalid interior history record")?;
                ensure!(
                    after.is_none_or(|t| sample.tick > t),
                    "resumed session metrics do not follow its parent checkpoint"
                );
                ensure!(
                    previous.is_none_or(|t| sample.tick > t),
                    "session metrics are not strictly ordered"
                );
                previous = Some(sample.tick);
                if sample.tick <= through {
                    history.push(sample)?;
                }
            }
        }
        ensure!(!header, "history segment has no complete header");
    }
    Ok(history)
}

fn latest_run(root: &Path) -> Result<String> {
    let mut newest: Option<(u64, String)> = None;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("run-") {
            continue;
        }
        // A crash can leave a directory before the first manifest commit. It
        // is not a saved experiment; corruption of a committed one still fails.
        if !entry.path().join("latest.json").exists() {
            continue;
        }
        let info: RunInfo = read_json(&entry.path().join("run.json"))?;
        let candidate = (info.created_at, name);
        if newest.as_ref().is_none_or(|n| candidate > *n) {
            newest = Some(candidate);
        }
    }
    newest
        .map(|(_, id)| id)
        .context("there are no saved local experiments")
}

fn checked_id(id: &str) -> Result<&str> {
    ensure!(
        !id.is_empty()
            && id.len() <= 100
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "invalid local experiment identifier"
    );
    Ok(id)
}

fn acquire(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.join("writer.lock"))?;
    file.try_lock()
        .context("this experiment already has a writer; stop that server before resuming")?;
    Ok(file)
}

fn create_directory(root: &Path, prefix: &str) -> Result<(String, PathBuf)> {
    for counter in 0..1000 {
        let id = format!("{prefix}-{}-{}-{counter}", now(), std::process::id());
        let path = root.join(&id);
        match fs::create_dir(&path) {
            Ok(()) => return Ok((id, path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    bail!("cannot allocate a unique local experiment directory")
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    serde_json::from_reader(
        File::open(path).with_context(|| format!("opening {}", path.display()))?,
    )
    .with_context(|| format!("reading {}", path.display()))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_json_new(path: &Path, value: &impl Serialize) -> Result<()> {
    write_new(path, &serde_json::to_vec(value)?)
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temp = path.with_extension("json.pending");
    // One OS-locked writer owns this fixed publication path. A crash may leave
    // the pending file; it is never considered a published checkpoint.
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path).context("publishing the complete checkpoint manifest")?;
    Ok(())
}

fn prune_checkpoints(root: &Path, latest: &str) -> Result<()> {
    let resolved_root = fs::canonicalize(root)?;
    let mut previous = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if !id.starts_with("checkpoint-")
            || id == latest
            || !entry.path().join("published").is_file()
        {
            continue;
        }
        checked_id(&id)?;
        let info: CheckpointInfo = read_json(&entry.path().join("metadata.json"))?;
        ensure!(
            info.checkpoint_id == id,
            "retention generation metadata differs"
        );
        previous.push((info.saved_at, id, entry.path()));
    }
    previous.sort_by(|a, b| b.cmp(a));
    for (_, _, path) in previous.into_iter().skip(RETAIN_CHECKPOINTS - 1) {
        // Only direct, committed child generations of this OS-locked run.
        let resolved_target = fs::canonicalize(&path)?;
        ensure!(
            resolved_target.parent() == Some(resolved_root.as_path()),
            "retention target escaped checkpoint directory"
        );
        fs::remove_dir_all(&resolved_target)
            .with_context(|| format!("retaining the latest {RETAIN_CHECKPOINTS} checkpoints"))?;
    }
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn state_lock(state: &Arc<Mutex<State>>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait_for_state(state: &Arc<Mutex<State>>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let s = state_lock(state);
        if let Some(error) = &s.error {
            bail!("{error}");
        }
        if !s.saving {
            return Ok(());
        }
        drop(s);
        ensure!(
            Instant::now() < deadline,
            "checkpoint write did not finish within 30 seconds"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
#[test]
fn overview_is_bounded_and_preserves_actual_first_and_last_samples() {
    let mut history = History::default();
    for tick in 1..=10_000 {
        history
            .push(Sample {
                tick,
                biomass: f64::from(tick),
                shares: vec![1.0],
                resources: BTreeMap::new(),
            })
            .unwrap();
        assert!(history.samples().len() <= HISTORY_LIMIT);
    }
    let samples = history.samples();
    assert_eq!(history.count, 10_000);
    assert_eq!(samples.first().unwrap().tick, 1);
    assert_eq!(samples.last().unwrap().tick, 10_000);
    assert!(samples.windows(2).all(|pair| pair[0].tick < pair[1].tick));
    assert!(
        samples
            .iter()
            .all(|sample| sample.biomass == f64::from(sample.tick))
    );
    history.push(samples.last().unwrap().clone()).unwrap();
    assert_eq!(history.count, 10_000);
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
