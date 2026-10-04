//! Durable host storage for the standalone, well-mixed cell chamber.
//!
//! The cell engine owns the meaning and validation of `Capture::state`. This
//! module only owns crash-safe publication, lineage history, and run identity.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const FORMAT: u32 = 1;
const CHAMBER_FORMAT: u32 = 1;
const KIND: &str = "cells";
const HISTORY_LIMIT: usize = 360;
const RETAIN_CHECKPOINTS: usize = 3;
pub(crate) const AUTOSAVE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CellResidual {
    pub matter: BTreeMap<String, String>,
    pub energy: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CellMetric {
    pub tick: u64,
    pub sim_time: f64,
    pub living_cells: u64,
    pub births: u64,
    pub deaths: u64,
    pub biomass_mol: f64,
    pub cell_energy_j: f64,
    pub resources: BTreeMap<String, f64>,
    pub genomes: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residual: Option<CellResidual>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raw_integer_statistics: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Capture {
    pub tick: u64,
    #[serde(default = "legacy_cell_world_format")]
    pub world_format_version: u32,
    pub running: bool,
    pub tps: f64,
    pub state: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<CellMetric>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct StoredRun {
    pub run_id: String,
    pub session_id: String,
    pub config_text: String,
    pub seed: String,
    pub config_hash: String,
    pub world_format_version: u32,
    pub chamber_format: u32,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ResumeState {
    pub stored: StoredRun,
    pub checkpoint_id: String,
    pub parent_tick: u64,
    pub capture: Capture,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct HistoryContract {
    pub run_id: String,
    pub session_id: String,
    pub samples: Vec<CellMetric>,
    pub total_samples: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunInfo {
    format: u32,
    kind: String,
    chamber_format: u32,
    run_id: String,
    seed: String,
    config_hash: String,
    world_format_version: u32,
    created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionInfo {
    format: u32,
    kind: String,
    session_id: String,
    parent_session: Option<String>,
    parent_tick: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointInfo {
    format: u32,
    kind: String,
    chamber_format: u32,
    checkpoint_id: String,
    session_id: String,
    seed: String,
    config_hash: String,
    world_format_version: u32,
    tick: u64,
    running: bool,
    tps: f64,
    saved_at: u64,
    state_bytes: u64,
    state_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryHeader {
    record: String,
    format: u32,
    kind: String,
    chamber_format: u32,
    run_id: String,
    session_id: String,
    seed: String,
    config_hash: String,
    world_format_version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HistoryRecord {
    record: String,
    metric: CellMetric,
}

struct Lease {
    root: PathBuf,
    path: PathBuf,
    info: RunInfo,
    config_text: String,
    // An OS lease is released by the kernel after a crash.
    _lock: File,
}

#[derive(Default)]
struct History {
    anchors: Vec<(u64, CellMetric)>,
    last: Option<CellMetric>,
    count: u64,
    stride: u64,
}

impl History {
    fn push(&mut self, sample: CellMetric) -> Result<bool> {
        if let Some(last) = &self.last {
            ensure!(
                sample.tick >= last.tick,
                "cell history ticks went backwards within a lineage"
            );
            if sample.tick == last.tick {
                return Ok(false);
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
        Ok(true)
    }

    fn samples(&self) -> Vec<CellMetric> {
        let mut out: Vec<_> = self
            .anchors
            .iter()
            .map(|(_, sample)| sample.clone())
            .collect();
        if let Some(last) = &self.last
            && out.last().is_none_or(|sample| sample.tick != last.tick)
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
    skipped_observations: u64,
    history: History,
}

/// Temporary backpressure: no checkpoint was accepted or acknowledged.
#[derive(Debug)]
pub(crate) struct QueueBusy;

impl std::fmt::Display for QueueBusy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("cell storage queue is busy")
    }
}

impl std::error::Error for QueueBusy {}

enum Job {
    Sample(CellMetric),
    Checkpoint(Vec<u8>, Box<CheckpointInfo>, Option<CellMetric>),
    #[cfg(test)]
    Barrier(mpsc::SyncSender<()>),
    #[cfg(test)]
    Block(mpsc::SyncSender<()>, mpsc::Receiver<()>),
}

pub(crate) struct CellsStorage {
    lease: Arc<Lease>,
    session: SessionInfo,
    sender: mpsc::SyncSender<Job>,
    state: Arc<Mutex<State>>,
    last_capture: Instant,
    last_enqueued_sample: Option<u64>,
}

/// A checked resume that has not yet written a new session. The cell engine
/// must validate `state().capture.state` before calling `start`.
pub(crate) struct PreparedResume {
    lease: Arc<Lease>,
    checkpoint: CheckpointInfo,
    state: ResumeState,
    history: History,
    repair_published_marker: bool,
}

impl PreparedResume {
    pub(crate) fn state(&self) -> &ResumeState {
        &self.state
    }

    pub(crate) fn start(self) -> Result<CellsStorage> {
        if self.repair_published_marker {
            let generation = self
                .lease
                .path
                .join("checkpoints")
                .join(&self.checkpoint.checkpoint_id);
            repair_published_marker(&generation)?;
        }
        CellsStorage::start(self.lease, Some(&self.checkpoint), self.history, false)
    }
}

impl CellsStorage {
    pub(crate) fn prepare_resume(
        root: &Path,
        run: &str,
        checkpoint: Option<&str>,
    ) -> Result<PreparedResume> {
        prepare_resume(root, run, checkpoint)
    }

    pub(crate) fn new_run(
        root: &Path,
        config_text: String,
        config_hash: String,
        seed: String,
        world_format_version: u32,
    ) -> Result<Self> {
        validate_identity(&config_text, &config_hash, &seed, world_format_version)?;
        fs::create_dir_all(root)
            .with_context(|| format!("creating cell experiment storage {}", root.display()))?;
        let root = fs::canonicalize(root)?;
        let (run_id, path) = create_directory(&root, "run")?;
        let lock = acquire(&path)?;
        let info = RunInfo {
            format: FORMAT,
            kind: KIND.into(),
            chamber_format: CHAMBER_FORMAT,
            run_id,
            seed,
            config_hash,
            world_format_version,
            created_at: now(),
        };
        let lease = Arc::new(Lease {
            root,
            path,
            info,
            config_text,
            _lock: lock,
        });
        Self::start(lease, None, History::default(), true)
    }

    fn start(
        lease: Arc<Lease>,
        parent: Option<&CheckpointInfo>,
        history: History,
        new_run: bool,
    ) -> Result<Self> {
        let sessions = lease.path.join("sessions");
        fs::create_dir_all(&sessions)?;
        let (session_id, session_path) = create_directory(&sessions, "session")?;
        let session = SessionInfo {
            format: FORMAT,
            kind: KIND.into(),
            session_id,
            parent_session: parent.map(|checkpoint| checkpoint.session_id.clone()),
            parent_tick: parent.map(|checkpoint| checkpoint.tick),
        };
        let last_enqueued_sample = history.last.as_ref().map(|sample| sample.tick);
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let state = Arc::new(Mutex::new(State {
            saved: parent.cloned(),
            history,
            ..State::default()
        }));
        let worker_lease = Arc::clone(&lease);
        let worker_state = Arc::clone(&state);
        let worker_session = session.clone();
        std::thread::Builder::new()
            .name("liminis-cells-storage".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    if new_run {
                        write_new(
                            &worker_lease.path.join("config.toml"),
                            worker_lease.config_text.as_bytes(),
                        )?;
                        write_json_new(&worker_lease.path.join("run.json"), &worker_lease.info)?;
                    }
                    write_json_new(&session_path.join("session.json"), &worker_session)?;
                    let header = HistoryHeader {
                        record: "header".into(),
                        format: FORMAT,
                        kind: KIND.into(),
                        chamber_format: CHAMBER_FORMAT,
                        run_id: worker_lease.info.run_id.clone(),
                        session_id: worker_session.session_id.clone(),
                        seed: worker_lease.info.seed.clone(),
                        config_hash: worker_lease.info.config_hash.clone(),
                        world_format_version: worker_lease.info.world_format_version,
                    };
                    let mut stream = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(session_path.join("metrics.ndjson"))?;
                    write_line(&mut stream, &header)?;
                    stream.sync_all()?;
                    while let Ok(job) = receiver.recv() {
                        match job {
                            Job::Sample(metric) => {
                                append_metric(&mut stream, &worker_state, metric)?;
                            }
                            Job::Checkpoint(bytes, mut info, metric) => {
                                if let Some(metric) = metric {
                                    append_metric(&mut stream, &worker_state, metric)?;
                                }
                                let checkpoints = worker_lease.path.join("checkpoints");
                                fs::create_dir_all(&checkpoints)?;
                                let (id, path) = create_directory(&checkpoints, "checkpoint")?;
                                info.checkpoint_id = id;
                                info.saved_at = now();
                                info.state_hash = blake3::hash(&bytes).to_hex().to_string();
                                write_new(&path.join("state.json"), &bytes)?;
                                write_json_new(&path.join("metadata.json"), &info)?;
                                // `latest.json` is the commit point and is synced before ack.
                                atomic_json(&worker_lease.path.join("latest.json"), &info)?;
                                write_new(&path.join("published"), b"")?;
                                prune_checkpoints(&checkpoints, &info.checkpoint_id)?;
                                let mut state = state_lock(&worker_state);
                                state.saved = Some(*info);
                                state.saving = false;
                            }
                            #[cfg(test)]
                            Job::Barrier(done) => {
                                let _ = done.send(());
                            }
                            #[cfg(test)]
                            Job::Block(started, release) => {
                                let _ = started.send(());
                                let _ = release.recv();
                            }
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    let mut state = state_lock(&worker_state);
                    state.error = Some(format!("cell storage failed: {error:#}"));
                    state.saving = false;
                }
            })
            .context("starting the cell experiment storage worker")?;
        Ok(Self {
            lease,
            session,
            sender,
            state,
            last_capture: Instant::now(),
            last_enqueued_sample,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.lease.root
    }

    #[cfg(test)]
    fn run_id(&self) -> &str {
        &self.lease.info.run_id
    }

    #[cfg(test)]
    fn session_id(&self) -> &str {
        &self.session.session_id
    }

    pub(crate) fn busy(&self) -> bool {
        state_lock(&self.state).saving
    }

    pub(crate) fn error(&self) -> Option<String> {
        state_lock(&self.state).error.clone()
    }

    pub(crate) fn autosave_due(&self) -> bool {
        !self.busy() && self.last_capture.elapsed() >= AUTOSAVE
    }

    pub(crate) fn status(&self) -> Value {
        let state = state_lock(&self.state);
        json!({
            "enabled": true,
            "kind": KIND,
            "run_id": self.lease.info.run_id,
            "session_id": self.session.session_id,
            "saved_tick": state.saved.as_ref().map(|checkpoint| checkpoint.tick),
            "saved_at": state.saved.as_ref().map(|checkpoint| checkpoint.saved_at),
            "saving": state.saving,
            "error": state.error,
            "autosave_seconds": AUTOSAVE.as_secs(),
            "retained_checkpoints": RETAIN_CHECKPOINTS,
            "capture_bytes": state.capture_bytes,
            "capture_ms": state.capture_ms,
            "history_samples": state.history.count,
            "skipped_observations": state.skipped_observations,
        })
    }

    pub(crate) fn history(&self) -> Value {
        let state = state_lock(&self.state);
        let samples = state.history.samples();
        serde_json::to_value(HistoryContract {
            run_id: self.lease.info.run_id.clone(),
            session_id: self.session.session_id.clone(),
            total_samples: state.history.count,
            truncated: state.history.count > samples.len() as u64,
            samples,
        })
        .expect("serializing cell history contract")
    }

    /// Enqueues one optional actual observation. A saturated bounded queue skips
    /// it visibly without stopping the culture; history counts persisted samples.
    /// Checkpoint requests and real storage failures retain their stronger contract.
    pub(crate) fn sample(&mut self, metric: CellMetric) -> Result<()> {
        validate_metric(&metric)?;
        if let Some(error) = self.error() {
            bail!(error);
        }
        if self
            .last_enqueued_sample
            .is_some_and(|tick| metric.tick <= tick)
        {
            ensure!(
                self.last_enqueued_sample == Some(metric.tick),
                "cell history ticks went backwards"
            );
            return Ok(());
        }
        let tick = metric.tick;
        match self.sender.try_send(Job::Sample(metric)) {
            Ok(()) => {
                self.last_enqueued_sample = Some(tick);
                Ok(())
            }
            Err(mpsc::TrySendError::Full(_)) => {
                let mut state = state_lock(&self.state);
                state.skipped_observations = state.skipped_observations.saturating_add(1);
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                let message = "cell storage worker is unavailable".to_string();
                state_lock(&self.state).error = Some(message.clone());
                bail!(message)
            }
        }
    }

    pub(crate) fn save(&mut self, capture: Capture) -> Result<()> {
        ensure!(!self.busy(), "a cell checkpoint is already being saved");
        if let Some(error) = self.error() {
            bail!(error);
        }
        validate_capture(&capture)?;
        ensure!(
            capture.world_format_version == self.lease.info.world_format_version,
            "cell capture world identity differs from run"
        );
        let started = Instant::now();
        let metric = capture
            .metric
            .clone()
            .filter(|metric| self.last_enqueued_sample != Some(metric.tick));
        let bytes = serde_json::to_vec(&capture)?;
        let info = CheckpointInfo {
            format: FORMAT,
            kind: KIND.into(),
            chamber_format: CHAMBER_FORMAT,
            checkpoint_id: String::new(),
            session_id: self.session.session_id.clone(),
            seed: self.lease.info.seed.clone(),
            config_hash: self.lease.info.config_hash.clone(),
            world_format_version: self.lease.info.world_format_version,
            tick: capture.tick,
            running: capture.running,
            tps: capture.tps,
            saved_at: 0,
            state_bytes: bytes.len() as u64,
            state_hash: String::new(),
        };
        {
            let mut state = state_lock(&self.state);
            state.saving = true;
            state.capture_bytes = bytes.len();
            state.capture_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        match self
            .sender
            .try_send(Job::Checkpoint(bytes, Box::new(info), metric.clone()))
        {
            Ok(()) => {
                if let Some(metric) = metric {
                    self.last_enqueued_sample = Some(metric.tick);
                }
                self.last_capture = Instant::now();
                Ok(())
            }
            Err(mpsc::TrySendError::Full(_)) => {
                state_lock(&self.state).saving = false;
                Err(QueueBusy.into())
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                let message = "cell storage worker is unavailable".to_string();
                let mut state = state_lock(&self.state);
                state.saving = false;
                state.error = Some(message.clone());
                bail!(message)
            }
        }
    }

    pub(crate) fn wait_for_save(&mut self) -> Result<()> {
        wait_for_state(&self.state)
    }

    #[cfg(test)]
    pub(crate) fn test_make_autosave_due(&mut self) {
        self.last_capture = Instant::now() - AUTOSAVE;
    }

    /// Holds the writer and fills its single pending slot until released.
    #[cfg(test)]
    pub(crate) fn test_block_worker(&self) -> mpsc::SyncSender<()> {
        self.flush().expect("flush before holding storage worker");
        let (started, ready) = mpsc::sync_channel(0);
        let (release, blocked) = mpsc::sync_channel(0);
        self.sender
            .send(Job::Block(started, blocked))
            .expect("enqueue test storage hold");
        ready
            .recv_timeout(Duration::from_secs(5))
            .expect("storage worker entered test hold");
        let (done, receiver) = mpsc::sync_channel(0);
        drop(receiver);
        self.sender
            .try_send(Job::Barrier(done))
            .expect("fill held storage queue");
        release
    }

    #[cfg(test)]
    fn flush(&self) -> Result<()> {
        if let Some(error) = self.error() {
            bail!(error);
        }
        let (sender, receiver) = mpsc::sync_channel(0);
        self.sender
            .send(Job::Barrier(sender))
            .map_err(|_| anyhow::anyhow!("cell storage worker is unavailable"))?;
        receiver
            .recv_timeout(Duration::from_secs(30))
            .context("cell storage flush did not finish within 30 seconds")?;
        if let Some(error) = self.error() {
            bail!(error);
        }
        Ok(())
    }
}

fn prepare_resume(root: &Path, run: &str, checkpoint: Option<&str>) -> Result<PreparedResume> {
    let root = fs::canonicalize(root).context("opening local cell experiment storage")?;
    let run_id = if run == "latest" {
        latest_run(&root)?
    } else {
        checked_prefixed_id(run, "run-")?.to_string()
    };
    let path = root.join(&run_id);
    let lock = acquire(&path)?;
    let info: RunInfo = read_json(&path.join("run.json"))?;
    validate_run_info(&info, &run_id)?;
    let config_text = fs::read_to_string(path.join("config.toml"))?;
    validate_identity(
        &config_text,
        &info.config_hash,
        &info.seed,
        info.world_format_version,
    )?;
    let lease = Arc::new(Lease {
        root,
        path,
        info,
        config_text,
        _lock: lock,
    });
    let repair_published_marker = checkpoint.is_none();
    let latest: CheckpointInfo = if let Some(id) = checkpoint {
        let metadata: CheckpointInfo = read_json(
            &lease
                .path
                .join("checkpoints")
                .join(checked_prefixed_id(id, "checkpoint-")?)
                .join("metadata.json"),
        )?;
        ensure!(
            metadata.checkpoint_id == id,
            "requested cell checkpoint identifier differs from its metadata"
        );
        metadata
    } else {
        read_json(&lease.path.join("latest.json"))?
    };
    validate_checkpoint_info(&latest, &lease.info)?;
    let generation = lease
        .path
        .join("checkpoints")
        .join(checked_prefixed_id(&latest.checkpoint_id, "checkpoint-")?);
    let published: CheckpointInfo = read_json(&generation.join("metadata.json"))?;
    ensure!(
        serde_json::to_vec(&latest)? == serde_json::to_vec(&published)?,
        "cell checkpoint manifest and generation disagree"
    );
    let bytes = fs::read(generation.join("state.json"))?;
    ensure!(
        bytes.len() as u64 == latest.state_bytes,
        "cell checkpoint state length differs"
    );
    ensure!(
        blake3::hash(&bytes).to_hex().as_str() == latest.state_hash,
        "cell checkpoint state checksum differs"
    );
    let mut decoder = serde_json::Deserializer::from_slice(&bytes);
    let capture = Capture::deserialize(&mut decoder).context("decoding cell checkpoint state")?;
    decoder
        .end()
        .context("cell checkpoint state has trailing bytes")?;
    validate_capture(&capture)?;
    ensure!(
        capture.tick == latest.tick
            && capture.running == latest.running
            && capture.tps == latest.tps
            && capture.world_format_version == latest.world_format_version,
        "cell checkpoint state differs from metadata"
    );
    let history = load_history(&lease, &latest)?;
    let session_id = latest.session_id.clone();
    let state = ResumeState {
        stored: stored_run(&lease, &session_id),
        checkpoint_id: latest.checkpoint_id.clone(),
        parent_tick: latest.tick,
        capture,
    };
    Ok(PreparedResume {
        lease,
        checkpoint: latest,
        state,
        history,
        repair_published_marker,
    })
}

fn stored_run(lease: &Lease, session_id: &str) -> StoredRun {
    StoredRun {
        run_id: lease.info.run_id.clone(),
        session_id: session_id.to_string(),
        config_text: lease.config_text.clone(),
        seed: lease.info.seed.clone(),
        config_hash: lease.info.config_hash.clone(),
        world_format_version: lease.info.world_format_version,
        chamber_format: lease.info.chamber_format,
        kind: lease.info.kind.clone(),
    }
}

fn validate_identity(
    config_text: &str,
    config_hash: &str,
    seed: &str,
    world_format_version: u32,
) -> Result<()> {
    ensure!(!config_text.is_empty(), "cell canonical config is empty");
    ensure!(!seed.is_empty(), "cell seed identity is empty");
    let digest = blake3::hash(config_text.as_bytes()).to_hex();
    let expected_hash = format!("blake3:{}", &digest.as_str()[..16]);
    ensure!(
        config_hash == expected_hash,
        "cell canonical config hash differs"
    );
    ensure!(
        supports_cell_version(world_format_version),
        "cell storage requires world format version 29 or 30"
    );
    Ok(())
}

fn legacy_cell_world_format() -> u32 {
    29
}

fn supports_cell_version(version: u32) -> bool {
    matches!(version, 29 | 30)
}

fn validate_run_info(info: &RunInfo, run_id: &str) -> Result<()> {
    checked_prefixed_id(run_id, "run-")?;
    ensure!(
        info.format == FORMAT
            && info.kind == KIND
            && info.chamber_format == CHAMBER_FORMAT
            && info.run_id == run_id,
        "unsupported or mismatched cell run manifest"
    );
    Ok(())
}

fn validate_checkpoint_info(info: &CheckpointInfo, run: &RunInfo) -> Result<()> {
    ensure!(
        info.format == FORMAT && info.kind == KIND && info.chamber_format == CHAMBER_FORMAT,
        "unsupported cell checkpoint envelope"
    );
    checked_prefixed_id(&info.checkpoint_id, "checkpoint-")?;
    checked_prefixed_id(&info.session_id, "session-")?;
    ensure!(
        info.seed == run.seed
            && info.config_hash == run.config_hash
            && info.world_format_version == run.world_format_version,
        "cell checkpoint identity differs from run manifest"
    );
    ensure!(
        info.tps.is_finite() && info.tps > 0.0,
        "cell checkpoint rate is invalid"
    );
    Ok(())
}

fn validate_capture(capture: &Capture) -> Result<()> {
    ensure!(
        supports_cell_version(capture.world_format_version),
        "cell capture has an unsupported world format version"
    );
    ensure!(
        capture.tps.is_finite() && capture.tps > 0.0,
        "cell capture rate is invalid"
    );
    ensure!(
        capture.state.is_object(),
        "cell core snapshot must be a JSON object"
    );
    if let Some(metric) = &capture.metric {
        validate_metric(metric)?;
        ensure!(
            metric.tick == capture.tick,
            "cell capture metric tick differs from state tick"
        );
    }
    Ok(())
}

fn validate_metric(metric: &CellMetric) -> Result<()> {
    ensure!(
        metric.sim_time.is_finite()
            && metric.sim_time >= 0.0
            && metric.biomass_mol.is_finite()
            && metric.biomass_mol >= 0.0
            && metric.cell_energy_j.is_finite()
            && metric.cell_energy_j >= 0.0,
        "cell history metric contains an invalid scalar"
    );
    ensure!(
        metric
            .resources
            .values()
            .all(|value| value.is_finite() && *value >= 0.0),
        "cell history resource concentration is invalid"
    );
    let genome_cells = metric
        .genomes
        .values()
        .try_fold(0_u64, |total, count| total.checked_add(*count))
        .context("cell history genome count overflow")?;
    ensure!(
        genome_cells == metric.living_cells,
        "cell history genome counts differ from living cell count"
    );
    for (name, value) in &metric.raw_integer_statistics {
        value
            .parse::<i128>()
            .with_context(|| format!("cell history statistic {name} is not an exact i128"))?;
    }
    if let Some(residual) = &metric.residual {
        for (name, value) in &residual.matter {
            let parsed = value
                .parse::<i128>()
                .with_context(|| format!("cell matter residual {name} is not an exact i128"))?;
            ensure!(parsed == 0, "cell matter residual {name} is nonzero");
        }
        let energy = residual
            .energy
            .parse::<i128>()
            .context("cell energy residual is not an exact i128")?;
        ensure!(energy == 0, "cell energy residual is nonzero");
    }
    Ok(())
}

fn append_metric(stream: &mut File, state: &Arc<Mutex<State>>, metric: CellMetric) -> Result<()> {
    let record = HistoryRecord {
        record: "sample".into(),
        metric: metric.clone(),
    };
    write_line(stream, &record)?;
    stream.sync_data()?;
    state_lock(state).history.push(metric)?;
    Ok(())
}

fn load_history(lease: &Lease, checkpoint: &CheckpointInfo) -> Result<History> {
    let mut chain = Vec::new();
    let mut visited = BTreeSet::new();
    let mut session_id = Some(checkpoint.session_id.clone());
    let mut through = checkpoint.tick;
    while let Some(id) = session_id {
        ensure!(
            visited.len() < 10_000 && visited.insert(id.clone()),
            "cell history session lineage is cyclic or too long"
        );
        let path = lease
            .path
            .join("sessions")
            .join(checked_prefixed_id(&id, "session-")?);
        let session: SessionInfo = read_json(&path.join("session.json"))?;
        ensure!(
            session.format == FORMAT && session.kind == KIND && session.session_id == id,
            "cell session manifest differs"
        );
        ensure!(
            session.parent_session.is_some() == session.parent_tick.is_some(),
            "cell session parent is incomplete"
        );
        chain.push((path, id, through, session.parent_tick));
        session_id = session.parent_session;
        if let Some(tick) = session.parent_tick {
            ensure!(tick <= through, "cell history parent tick is in the future");
            through = tick;
        }
    }
    let mut history = History::default();
    for (path, expected_session_id, through, after) in chain.into_iter().rev() {
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
            }
            if header {
                let value: HistoryHeader = serde_json::from_slice(&line)?;
                ensure!(
                    value.record == "header"
                        && value.format == FORMAT
                        && value.kind == KIND
                        && value.chamber_format == CHAMBER_FORMAT
                        && value.run_id == lease.info.run_id
                        && value.session_id == expected_session_id
                        && value.seed == lease.info.seed
                        && value.config_hash == lease.info.config_hash
                        && value.world_format_version == lease.info.world_format_version,
                    "cell history header belongs to another experiment"
                );
                header = false;
            } else {
                let record: HistoryRecord = serde_json::from_slice(&line)
                    .context("invalid interior cell history record")?;
                ensure!(record.record == "sample", "unknown cell history record");
                validate_metric(&record.metric)?;
                ensure!(
                    after.is_none_or(|tick| record.metric.tick > tick),
                    "resumed cell session metrics do not follow its parent checkpoint"
                );
                ensure!(
                    previous.is_none_or(|tick| record.metric.tick > tick),
                    "cell session metrics are not strictly ordered"
                );
                previous = Some(record.metric.tick);
                if record.metric.tick <= through {
                    history.push(record.metric)?;
                }
            }
        }
        ensure!(!header, "cell history segment has no complete header");
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
        if !name.starts_with("run-") || !entry.path().join("latest.json").exists() {
            continue;
        }
        // Once latest exists, corruption is an error rather than a fallback.
        let info: RunInfo = read_json(&entry.path().join("run.json"))?;
        validate_run_info(&info, &name)?;
        let candidate = (info.created_at, name);
        if newest.as_ref().is_none_or(|current| candidate > *current) {
            newest = Some(candidate);
        }
    }
    newest
        .map(|(_, id)| id)
        .context("there are no saved local cell experiments")
}

fn checked_id(id: &str) -> Result<&str> {
    ensure!(
        !id.is_empty()
            && id.len() <= 100
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "invalid local cell experiment identifier"
    );
    Ok(id)
}

fn checked_prefixed_id<'a>(id: &'a str, prefix: &str) -> Result<&'a str> {
    checked_id(id)?;
    ensure!(
        id.starts_with(prefix) && id.len() > prefix.len(),
        "invalid local cell experiment identifier prefix"
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
        .context("this cell experiment already has a writer; stop that server before resuming")?;
    Ok(file)
}

fn create_directory(root: &Path, prefix: &str) -> Result<(String, PathBuf)> {
    for counter in 0..1000 {
        let id = format!("{prefix}-{}-{}-{counter}", now(), std::process::id());
        let path = root.join(&id);
        match fs::create_dir(&path) {
            Ok(()) => return Ok((id, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    bail!("cannot allocate a unique local cell experiment directory")
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

fn repair_published_marker(generation: &Path) -> Result<()> {
    let marker = generation.join("published");
    if marker.exists() {
        ensure!(
            marker.is_file(),
            "cell checkpoint publication marker is not a file"
        );
        return Ok(());
    }
    write_new(&marker, b"")
}

fn write_line(file: &mut File, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let pending = path.with_extension("json.pending");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&pending)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&pending, path).context("publishing the complete cell checkpoint manifest")?;
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
        checked_prefixed_id(&id, "checkpoint-")?;
        let info: CheckpointInfo = read_json(&entry.path().join("metadata.json"))?;
        ensure!(
            info.kind == KIND && info.checkpoint_id == id,
            "cell retention generation metadata differs"
        );
        previous.push((info.saved_at, id, entry.path()));
    }
    previous.sort_by(|left, right| right.cmp(left));
    for (_, _, path) in previous.into_iter().skip(RETAIN_CHECKPOINTS - 1) {
        let resolved = fs::canonicalize(&path)?;
        ensure!(
            resolved.parent() == Some(resolved_root.as_path()),
            "cell retention target escaped checkpoint directory"
        );
        fs::remove_dir_all(&resolved).with_context(|| {
            format!("retaining the latest {RETAIN_CHECKPOINTS} cell checkpoints")
        })?;
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
        let current = state_lock(state);
        if let Some(error) = &current.error {
            bail!(error.clone());
        }
        if !current.saving {
            return Ok(());
        }
        drop(current);
        ensure!(
            Instant::now() < deadline,
            "cell checkpoint write did not finish within 30 seconds"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
#[path = "cells_storage_tests.rs"]
mod tests;
