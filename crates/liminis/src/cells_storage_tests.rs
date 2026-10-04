use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "liminis-cells-storage-test-{}-{}-{}",
            std::process::id(),
            now(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("unique cell storage test directory");
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let (Ok(root), Ok(path)) = (
            fs::canonicalize(std::env::temp_dir()),
            fs::canonicalize(&self.0),
        ) else {
            return;
        };
        if self
            .0
            .file_name()
            .is_some_and(|name| path == root.join(name))
        {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn identity() -> (String, String) {
    let config = "format = 1\nname = \"test chamber\"\n".to_string();
    let digest = blake3::hash(config.as_bytes()).to_hex();
    let hash = format!("blake3:{}", &digest.as_str()[..16]);
    (config, hash)
}

fn metric(tick: u64) -> CellMetric {
    CellMetric {
        tick,
        sim_time: tick as f64 * 1.491_542_492_935_830_4e-9,
        living_cells: 2,
        births: tick,
        deaths: tick / 3,
        biomass_mol: 4.473_002_697_402_695e-12,
        cell_energy_j: 7.806_255_641_895_632e-6,
        resources: BTreeMap::from([("FOOD".into(), f64::from_bits(0.000_5_f64.to_bits() - 1))]),
        genomes: BTreeMap::from([("kinetics=0".into(), 2)]),
        residual: Some(CellResidual {
            matter: BTreeMap::from([("C".into(), "0".into())]),
            energy: "0".into(),
        }),
        raw_integer_statistics: BTreeMap::from([(
            "total_cell_mass_units".into(),
            "9007199254740993".into(),
        )]),
    }
}

fn capture(tick: u64) -> Capture {
    let exact = serde_json::from_str::<Value>(
        r#"{"format":1,"core":{"tick":0,"matter":900719925474099312345},"observation":{"births":0,"deaths":0,"divisions":0,"events":[]}}"#,
    )
    .unwrap();
    let mut state = exact;
    state["core"]["tick"] = json!(tick);
    Capture {
        tick,
        world_format_version: 29,
        running: tick.is_multiple_of(2),
        tps: 3.0,
        state,
        metric: (tick > 0).then(|| metric(tick)),
    }
}

fn new_storage(root: &Temp) -> CellsStorage {
    let (config, hash) = identity();
    CellsStorage::new_run(&root.0, config, hash, "42".into(), 29).expect("new cell experiment")
}

fn save(storage: &mut CellsStorage, tick: u64) -> CheckpointInfo {
    storage.save(capture(tick)).expect("accepted checkpoint");
    storage.wait_for_save().expect("committed checkpoint");
    state_lock(&storage.state)
        .saved
        .clone()
        .expect("saved metadata")
}

fn stop(storage: CellsStorage) {
    storage.flush().expect("flush storage worker");
    let lease = Arc::downgrade(&storage.lease);
    drop(storage);
    let deadline = Instant::now() + Duration::from_secs(5);
    while lease.upgrade().is_some() {
        assert!(
            Instant::now() < deadline,
            "cell storage worker did not release its lease"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn prepare_error(root: &Temp, run: &str, checkpoint: Option<&str>) -> String {
    match prepare_resume(&root.0, run, checkpoint) {
        Err(error) => format!("{error:#}"),
        Ok(_) => panic!("invalid cell checkpoint unexpectedly prepared"),
    }
}

fn rewrite_envelope(run: &Path, info: &CheckpointInfo) {
    let bytes = serde_json::to_vec(info).unwrap();
    fs::write(run.join("latest.json"), &bytes).unwrap();
    fs::write(
        run.join("checkpoints")
            .join(&info.checkpoint_id)
            .join("metadata.json"),
        bytes,
    )
    .unwrap();
}

#[test]
fn exact_json_round_trips_and_resume_is_two_phase() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    let info = save(&mut storage, 7);
    let run_id = storage.run_id().to_string();
    assert!(prepare_error(&root, &run_id, None).contains("writer"));
    stop(storage);

    let prepared = prepare_resume(&root.0, &run_id, None).unwrap();
    assert_eq!(prepared.state().capture, capture(7));
    assert_eq!(prepared.state().checkpoint_id, info.checkpoint_id);
    assert_eq!(prepared.state().stored.kind, KIND);
    let sessions_before = fs::read_dir(root.0.join(&run_id).join("sessions"))
        .unwrap()
        .count();
    assert_eq!(sessions_before, 1, "prepare must not create a session");
    let resumed = prepared.start().unwrap();
    resumed.flush().unwrap();
    assert_eq!(
        fs::read_dir(root.0.join(&run_id).join("sessions"))
            .unwrap()
            .count(),
        2
    );
    stop(resumed);
}

#[test]
fn cell_world_compatibility_is_exactly_29_or_30() {
    assert!(supports_cell_version(29));
    assert!(supports_cell_version(30));
    assert!(!supports_cell_version(28));
    assert!(!supports_cell_version(31));

    let mut legacy = serde_json::to_value(capture(7)).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("world_format_version");
    let decoded: Capture = serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.world_format_version, 29);

    let root = Temp::new();
    let mut storage = new_storage(&root);
    let mut wrong = capture(1);
    wrong.world_format_version = 30;
    assert!(
        storage
            .save(wrong)
            .unwrap_err()
            .to_string()
            .contains("world identity")
    );
    stop(storage);
}

#[test]
fn validated_implicit_latest_repairs_only_its_missing_marker() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    let info = save(&mut storage, 1);
    let run_id = storage.run_id().to_string();
    let marker = storage
        .lease
        .path
        .join("checkpoints")
        .join(&info.checkpoint_id)
        .join("published");
    stop(storage);
    fs::remove_file(&marker).unwrap();
    let prepared = prepare_resume(&root.0, &run_id, None).unwrap();
    assert!(!marker.exists(), "prepare is still read-only");
    let resumed = prepared.start().unwrap();
    assert!(
        marker.is_file(),
        "validated implicit latest repairs its marker"
    );
    stop(resumed);

    let root = Temp::new();
    let mut storage = new_storage(&root);
    let info = save(&mut storage, 1);
    let run_id = storage.run_id().to_string();
    let marker = storage
        .lease
        .path
        .join("checkpoints")
        .join(&info.checkpoint_id)
        .join("published");
    stop(storage);
    fs::remove_file(&marker).unwrap();
    let prepared = prepare_resume(&root.0, &run_id, Some(&info.checkpoint_id)).unwrap();
    let resumed = prepared.start().unwrap();
    assert!(
        !marker.exists(),
        "explicit orphan recovery does not publish the generation"
    );
    stop(resumed);
}

#[test]
fn corrupt_truncated_trailing_and_wrong_identity_state_are_rejected() {
    for case in 0..4 {
        let root = Temp::new();
        let mut storage = new_storage(&root);
        let mut info = save(&mut storage, 1);
        let run = storage.lease.path.clone();
        let run_id = storage.run_id().to_string();
        stop(storage);
        let marker = run
            .join("checkpoints")
            .join(&info.checkpoint_id)
            .join("published");
        fs::remove_file(&marker).unwrap();
        let path = run
            .join("checkpoints")
            .join(&info.checkpoint_id)
            .join("state.json");
        let mut bytes = fs::read(&path).unwrap();
        let expected = match case {
            0 => {
                bytes[0] ^= 1;
                fs::write(&path, &bytes).unwrap();
                "checksum"
            }
            1 => {
                bytes.pop();
                fs::write(&path, &bytes).unwrap();
                "length"
            }
            2 => {
                bytes.extend_from_slice(b"{}");
                fs::write(&path, &bytes).unwrap();
                info.state_bytes = bytes.len() as u64;
                info.state_hash = blake3::hash(&bytes).to_hex().to_string();
                rewrite_envelope(&run, &info);
                "trailing"
            }
            3 => {
                info.kind = "eco".into();
                rewrite_envelope(&run, &info);
                "envelope"
            }
            _ => unreachable!(),
        };
        let error = prepare_error(&root, &run_id, None);
        assert!(error.contains(expected), "case {case}: {error}");
        assert!(!marker.exists(), "invalid latest must remain unmarked");
    }
}

#[test]
fn valid_json_whitespace_and_equivalent_float_lexemes_restore() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    let mut info = save(&mut storage, 7);
    let run = storage.lease.path.clone();
    let run_id = storage.run_id().to_string();
    stop(storage);

    let path = run
        .join("checkpoints")
        .join(&info.checkpoint_id)
        .join("state.json");
    let compact = fs::read_to_string(&path).unwrap();
    let expanded = compact.replacen("\"tps\":3.0", "\"tps\":3.0000000000000000", 1);
    assert_ne!(expanded, compact, "fixture must contain the typed float");
    let bytes = format!(" \n{expanded}\n ").into_bytes();
    fs::write(&path, &bytes).unwrap();
    info.state_bytes = bytes.len() as u64;
    info.state_hash = blake3::hash(&bytes).to_hex().to_string();
    rewrite_envelope(&run, &info);

    let prepared = prepare_resume(&root.0, &run_id, None).unwrap();
    assert_eq!(prepared.state().capture, capture(7));
}

#[test]
fn latest_ignores_only_unpublished_runs_and_never_loads_an_eco_run() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    save(&mut storage, 1);
    let run_id = storage.run_id().to_string();
    stop(storage);

    let unfinished = root.0.join("run-unfinished");
    fs::create_dir(&unfinished).unwrap();
    fs::write(unfinished.join("run.json"), b"{broken").unwrap();
    let prepared = prepare_resume(&root.0, "latest", None).unwrap();
    assert_eq!(prepared.state().stored.run_id, run_id);
    drop(prepared);

    fs::write(unfinished.join("latest.json"), b"{}").unwrap();
    assert!(prepare_error(&root, "latest", None).contains("reading"));
}

#[test]
fn lineage_clips_old_future_and_ignores_only_a_torn_last_line() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    let run_id = storage.run_id().to_string();
    let mut middle = String::new();
    for tick in 1..=3 {
        let info = save(&mut storage, tick);
        if tick == 2 {
            middle = info.checkpoint_id;
        }
    }
    let history_file = storage
        .lease
        .path
        .join("sessions")
        .join(storage.session_id())
        .join("metrics.ndjson");
    stop(storage);
    OpenOptions::new()
        .append(true)
        .open(&history_file)
        .unwrap()
        .write_all(b"{torn")
        .unwrap();

    let prepared = prepare_resume(&root.0, &run_id, Some(&middle)).unwrap();
    assert_eq!(
        prepared
            .history
            .samples()
            .iter()
            .map(|m| m.tick)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let mut resumed = prepared.start().unwrap();
    resumed.sample(metric(3)).unwrap();
    resumed.flush().unwrap();
    let value = resumed.history();
    let ticks: Vec<_> = value["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|sample| sample["tick"].as_u64().unwrap())
        .collect();
    assert_eq!(ticks, [1, 2, 3]);
    stop(resumed);

    OpenOptions::new()
        .append(true)
        .open(&history_file)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    assert!(prepare_error(&root, &run_id, None).contains("history"));
}

#[test]
fn retention_keeps_three_published_generations_and_unrelated_children() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    let checkpoint_root = storage.lease.path.join("checkpoints");
    fs::create_dir(&checkpoint_root).unwrap();
    let unrelated = checkpoint_root.join("user-notes");
    fs::create_dir(&unrelated).unwrap();
    fs::write(unrelated.join("published"), b"not a checkpoint").unwrap();
    let pending = checkpoint_root.join("checkpoint-unpublished");
    fs::create_dir(&pending).unwrap();
    let mut committed = Vec::new();
    for tick in 1..=6 {
        committed.push(save(&mut storage, tick).checkpoint_id);
    }
    assert_eq!(
        committed
            .iter()
            .filter(|id| checkpoint_root.join(id).is_dir())
            .count(),
        3
    );
    for id in committed.iter().rev().take(3) {
        assert!(checkpoint_root.join(id).join("state.json").is_file());
    }
    assert!(unrelated.is_dir());
    assert!(pending.is_dir());
    assert_eq!(storage.history()["total_samples"], 6);
    stop(storage);
}

#[test]
fn failed_latest_publication_never_acknowledges_the_new_tick() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    let previous = save(&mut storage, 1);
    fs::create_dir(storage.lease.path.join("latest.json.pending")).unwrap();
    storage.save(capture(2)).unwrap();
    assert!(storage.wait_for_save().is_err());
    let saved = state_lock(&storage.state).saved.clone().unwrap();
    assert_eq!(saved.tick, previous.tick);
    let latest: CheckpointInfo = read_json(&storage.lease.path.join("latest.json")).unwrap();
    assert_eq!(latest.checkpoint_id, previous.checkpoint_id);
    drop(storage);
}

#[test]
fn bounded_history_preserves_real_first_and_last_samples() {
    let mut history = History::default();
    for tick in 1..=10_000 {
        history.push(metric(tick)).unwrap();
        assert!(history.samples().len() <= HISTORY_LIMIT);
    }
    let samples = history.samples();
    assert_eq!(history.count, 10_000);
    assert_eq!(samples.first().unwrap().tick, 1);
    assert_eq!(samples.last().unwrap().tick, 10_000);
    assert!(samples.windows(2).all(|pair| pair[0].tick < pair[1].tick));
}

#[test]
fn history_rejects_malformed_exact_values_nonzero_residuals_and_negative_energy() {
    let mut malformed = metric(1);
    malformed
        .raw_integer_statistics
        .insert("bad".into(), "1.25".into());
    assert!(
        validate_metric(&malformed)
            .unwrap_err()
            .to_string()
            .contains("i128")
    );

    let mut nonzero = metric(1);
    nonzero.residual.as_mut().unwrap().energy = "1".into();
    assert!(
        validate_metric(&nonzero)
            .unwrap_err()
            .to_string()
            .contains("nonzero")
    );

    let mut negative = metric(1);
    negative.cell_energy_j = -0.5;
    assert!(validate_metric(&negative).is_err());
}

#[test]
fn history_header_is_bound_to_its_session_path() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    save(&mut storage, 1);
    let run_id = storage.run_id().to_string();
    let history_file = storage
        .lease
        .path
        .join("sessions")
        .join(storage.session_id())
        .join("metrics.ndjson");
    stop(storage);

    let text = fs::read_to_string(&history_file).unwrap();
    let mut lines = text.lines();
    let mut header: HistoryHeader = serde_json::from_str(lines.next().unwrap()).unwrap();
    header.session_id = "session-valid-but-wrong".into();
    let mut changed = serde_json::to_string(&header).unwrap();
    changed.push('\n');
    for line in lines {
        changed.push_str(line);
        changed.push('\n');
    }
    fs::write(history_file, changed).unwrap();
    assert!(prepare_error(&root, &run_id, None).contains("header"));
}

#[test]
fn optional_sample_saturation_is_visible_and_checkpoint_busy_is_not_acknowledged() {
    let root = Temp::new();
    let mut storage = new_storage(&root);
    storage.flush().unwrap();
    // Hold a bounded queue without its consumer to exercise backpressure
    // deterministically, independently of filesystem or scheduler timing.
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker_sender = std::mem::replace(&mut storage.sender, sender);
    storage.sample(metric(1)).unwrap();
    storage.sample(metric(2)).unwrap();
    assert_eq!(storage.status()["skipped_observations"], 1);
    assert_eq!(storage.status()["history_samples"], 0);
    assert_eq!(storage.last_enqueued_sample, Some(1));
    assert!(storage.error().is_none());
    let error = storage.save(capture(2)).unwrap_err();
    assert!(error.downcast_ref::<QueueBusy>().is_some());
    assert!(!storage.busy());
    assert!(storage.status()["saved_tick"].is_null());
    assert!(storage.error().is_none());
    assert_eq!(storage.last_enqueued_sample, Some(1));

    let queued = receiver.recv().unwrap();
    storage.sender = worker_sender;
    storage.sender.send(queued).unwrap();
    storage.flush().unwrap();
    let info = save(&mut storage, 2);
    assert_eq!(info.tick, 2);
    assert_eq!(storage.history()["total_samples"], 2);
    assert_eq!(storage.status()["skipped_observations"], 1);
    let run_id = storage.run_id().to_string();
    stop(storage);
    let prepared = prepare_resume(&root.0, &run_id, None).unwrap();
    assert_eq!(prepared.state().capture, capture(2));
    assert_eq!(prepared.history.count, 2);
}
