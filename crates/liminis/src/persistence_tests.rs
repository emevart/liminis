use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "liminis-persistence-test-{}-{}-{}",
            std::process::id(),
            now(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("unique test directory");
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

fn fixture() -> Sim {
    let config =
        liminis_core::config::parse(super::super::fixture::SCENARIO).expect("fixture config");
    build(&config, u64::MAX).expect("fixture world")
}

fn stored(root: &Temp) -> (Sim, Persistence) {
    let sim = fixture();
    let mut persistence = Persistence::create(&root.0, &sim).expect("new durable experiment");
    persistence.wait_for_save().expect("baseline committed");
    (sim, persistence)
}

fn saved(persistence: &Persistence) -> CheckpointInfo {
    state_lock(&persistence.state)
        .saved
        .clone()
        .expect("saved metadata")
}

fn stop(persistence: Persistence) {
    let lease = Arc::downgrade(&persistence.lease);
    drop(persistence);
    let deadline = Instant::now() + Duration::from_secs(5);
    while lease.upgrade().is_some() {
        assert!(
            Instant::now() < deadline,
            "storage worker did not release its lease"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn resume_error(root: &Temp, run: &str, checkpoint: Option<&str>) -> String {
    match resume(&root.0, run, checkpoint) {
        Err(error) => format!("{error:#}"),
        Ok(mut sim) => {
            stop(sim.persistence.take().expect("resumed persistence"));
            panic!("invalid checkpoint unexpectedly resumed");
        }
    }
}

fn rewrite_envelope(run: &Path, info: &CheckpointInfo) {
    let bytes = serde_json::to_vec(info).expect("metadata bytes");
    fs::write(run.join("latest.json"), &bytes).expect("replace test manifest");
    fs::write(
        run.join("checkpoints")
            .join(&info.checkpoint_id)
            .join("metadata.json"),
        bytes,
    )
    .expect("replace test generation metadata");
}

#[test]
fn published_checkpoint_rejects_corruption_truncation_trailing_data_and_wrong_identity() {
    for case in 0..4 {
        let root = Temp::new();
        let (_, persistence) = stored(&root);
        let run = persistence.lease.path.clone();
        let run_id = persistence.lease.info.run_id.clone();
        let mut info = saved(&persistence);
        stop(persistence);
        let path = run
            .join("checkpoints")
            .join(&info.checkpoint_id)
            .join("state.limsnap");
        let mut bytes = fs::read(&path).expect("saved bytes");
        let expected = match case {
            0 => {
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                fs::write(&path, &bytes).unwrap();
                "checksum"
            }
            1 => {
                bytes.pop();
                fs::write(&path, &bytes).unwrap();
                "length"
            }
            2 => {
                bytes.push(77);
                fs::write(&path, &bytes).unwrap();
                info.state_bytes = bytes.len() as u64;
                info.state_hash = blake3::hash(&bytes).to_hex().to_string();
                rewrite_envelope(&run, &info);
                "trailing"
            }
            3 => {
                info.seed = "1".into();
                rewrite_envelope(&run, &info);
                "identity"
            }
            _ => unreachable!(),
        };
        let error = resume_error(&root, &run_id, None);
        assert!(error.contains(expected), "case {case}: {error}");
    }
}

#[test]
fn an_os_lease_refuses_a_second_writer_and_releases_after_shutdown() {
    let root = Temp::new();
    let (_, persistence) = stored(&root);
    let id = persistence.lease.info.run_id.clone();
    assert!(resume_error(&root, &id, None).contains("writer"));
    stop(persistence);
    let mut resumed = resume(&root.0, &id, None).expect("OS lock released");
    assert_eq!(resumed.identity.seed, u64::MAX);
    stop(resumed.persistence.take().unwrap());
}

#[test]
fn latest_ignores_unpublished_runs_but_not_corrupt_published_manifests() {
    let root = Temp::new();
    let (_, persistence) = stored(&root);
    let id = persistence.lease.info.run_id.clone();
    stop(persistence);
    let unfinished = root.0.join("run-unfinished");
    fs::create_dir(&unfinished).unwrap();
    fs::write(unfinished.join("run.json"), b"{interrupted").unwrap();
    assert_eq!(latest_run(&root.0).expect("previous committed run"), id);
    fs::write(unfinished.join("latest.json"), b"{}").unwrap();
    assert!(
        latest_run(&root.0).is_err(),
        "published corruption is not silently skipped"
    );
}

#[test]
fn explicit_checkpoint_cannot_redirect_to_another_generation() {
    let root = Temp::new();
    let (_, persistence) = stored(&root);
    let run = persistence.lease.path.clone();
    let id = persistence.lease.info.run_id.clone();
    let info = saved(&persistence);
    stop(persistence);
    let alias = run.join("checkpoints").join("checkpoint-alias");
    fs::create_dir(&alias).unwrap();
    write_json_new(&alias.join("metadata.json"), &info).unwrap();
    assert!(resume_error(&root, &id, Some("checkpoint-alias")).contains("requested"));
}

#[test]
fn complete_history_records_require_every_exact_integer_column() {
    let mut sim = fixture();
    super::super::advance_one(&mut sim);
    let model = Model::of(&sim);
    let record = metrics(&sim, true).expect("real checked tick");
    model.decode(&record).expect("complete record");
    let mut value: Value = serde_json::from_slice(&record).unwrap();
    let channel = model
        .columns
        .iter()
        .find(|key| key.starts_with("channel."))
        .expect("channel column");
    value.as_object_mut().unwrap().remove(channel);
    assert!(model.decode(&serde_json::to_vec(&value).unwrap()).is_err());
    value[channel] = json!(0.25);
    assert!(model.decode(&serde_json::to_vec(&value).unwrap()).is_err());
    value[channel] = json!((1_i128 << 100) + 1);
    model
        .decode(&serde_json::to_vec(&value).unwrap())
        .expect("i128 beyond f64 stays exact");
}

#[test]
fn resume_history_clips_the_old_future_and_appends_a_new_segment() {
    let root = Temp::new();
    let (mut sim, mut persistence) = stored(&root);
    let id = persistence.lease.info.run_id.clone();
    let mut middle = String::new();
    for target in [30, 60, 90] {
        while sim.ticks < target {
            super::super::advance_one(&mut sim);
        }
        persistence
            .save(&sim, true)
            .expect("checkpoint with real metric");
        persistence.wait_for_save().unwrap();
        if target == 60 {
            middle = saved(&persistence).checkpoint_id;
        }
    }
    stop(persistence);
    let mut resumed = resume(&root.0, &id, Some(&middle)).expect("resume tick 60");
    assert_eq!(resumed.ticks, 60);
    assert!(resumed.last.is_none(), "no fabricated restored residual");
    let mut storage = resumed.persistence.take().unwrap();
    let ticks = |storage: &Persistence| -> Vec<u32> {
        state_lock(&storage.state)
            .history
            .samples()
            .iter()
            .map(|s| s.tick)
            .collect()
    };
    assert_eq!(ticks(&storage), [30, 60]);
    super::super::advance_one(&mut resumed);
    storage.save(&resumed, true).unwrap();
    storage.wait_for_save().unwrap();
    assert_eq!(ticks(&storage), [30, 60, 61]);
    stop(storage);
}

#[test]
fn an_interrupted_tail_is_ignored_but_a_complete_bad_record_is_refused() {
    for complete in [false, true] {
        let root = Temp::new();
        let (mut sim, mut persistence) = stored(&root);
        super::super::advance_one(&mut sim);
        persistence.save(&sim, true).unwrap();
        persistence.wait_for_save().unwrap();
        let id = persistence.lease.info.run_id.clone();
        let file = persistence
            .lease
            .path
            .join("sessions")
            .join(&persistence.session.session_id)
            .join("metrics.ndjson");
        stop(persistence);
        let mut stream = OpenOptions::new().append(true).open(file).unwrap();
        stream
            .write_all(if complete { b"{bad}\n" } else { b"{bad" })
            .unwrap();
        drop(stream);
        if complete {
            assert!(resume_error(&root, &id, None).contains("history"));
        } else {
            let mut resumed = resume(&root.0, &id, None).expect("complete prefix recovers");
            assert_eq!(resumed.ticks, 1);
            stop(resumed.persistence.take().unwrap());
        }
    }
}

#[test]
fn failed_manifest_publication_preserves_the_previous_checkpoint() {
    let root = Temp::new();
    let (mut sim, mut persistence) = stored(&root);
    let run = persistence.lease.path.clone();
    let previous = saved(&persistence);
    fs::create_dir(run.join("latest.json.pending")).expect("inject publication failure");
    super::super::advance_one(&mut sim);
    persistence
        .save(&sim, true)
        .expect("capture accepted before disk error");
    assert!(persistence.wait_for_save().is_err());
    assert!(persistence.error().is_some());
    let current: CheckpointInfo = read_json(&run.join("latest.json")).unwrap();
    assert_eq!(current.checkpoint_id, previous.checkpoint_id);
    assert_eq!(
        saved(&persistence).tick,
        previous.tick,
        "failure never acknowledges a newer tick"
    );
    assert!(
        run.join("checkpoints")
            .join(previous.checkpoint_id)
            .join("state.limsnap")
            .is_file()
    );
    stop(persistence);
}

#[test]
fn retention_keeps_three_committed_checkpoints_and_all_history() {
    let root = Temp::new();
    let (mut sim, mut persistence) = stored(&root);
    let checkpoint_root = persistence.lease.path.join("checkpoints");
    let unrelated = checkpoint_root.join("user-notes");
    fs::create_dir(&unrelated).unwrap();
    fs::write(unrelated.join("published"), b"not a checkpoint").unwrap();
    let pending = checkpoint_root.join("checkpoint-unpublished");
    fs::create_dir(&pending).unwrap();
    let first = saved(&persistence).checkpoint_id;
    let mut committed = vec![first.clone()];
    for _ in 0..5 {
        super::super::advance_one(&mut sim);
        persistence.save(&sim, true).unwrap();
        persistence.wait_for_save().unwrap();
        committed.push(saved(&persistence).checkpoint_id);
    }
    assert!(!checkpoint_root.join(first).exists());
    for id in committed.iter().rev().take(3) {
        assert!(checkpoint_root.join(id).join("state.limsnap").is_file());
    }
    assert_eq!(
        committed
            .iter()
            .filter(|id| checkpoint_root.join(id).is_dir())
            .count(),
        3
    );
    assert!(
        unrelated.is_dir(),
        "non-checkpoint files are never retention candidates"
    );
    assert!(
        pending.is_dir(),
        "unfinished generations are not pruned as published state"
    );
    assert_eq!(state_lock(&persistence.state).history.count, 5);
    let history = persistence
        .lease
        .path
        .join("sessions")
        .join(&persistence.session.session_id)
        .join("metrics.ndjson");
    assert_eq!(fs::read_to_string(history).unwrap().lines().count(), 6);
    stop(persistence);
}

#[test]
fn host_resume_preserves_genotype_observer_mode_and_exact_future() {
    for running in [false, true] {
        let root = Temp::new();
        let mut config = liminis_core::config::parse(include_str!(
            "../../../configs/scenarios/genetic-colony.toml"
        ))
        .expect("genetic source");
        config.grid.nx = 8;
        config.grid.ny = 8;
        config.grid.nz = 8;
        config.initial.inoculum[0].center = [0.0004; 3];
        config.initial.inoculum[0].radius = 0.00015;
        let mut continuous = build(&config, u64::MAX).expect("genetic world");
        for _ in 0..75 {
            super::super::advance_one(&mut continuous);
        }
        continuous.running = running;
        continuous.target_tps = 137.0;
        let first_seen = continuous.ecology.first_seen();
        assert!(first_seen.values().any(|tick| tick.is_some_and(|t| t > 0)));
        let mut persistence = Persistence::create(&root.0, &continuous).unwrap();
        persistence.wait_for_save().unwrap();
        let id = persistence.lease.info.run_id.clone();
        stop(persistence);
        let mut resumed = resume(&root.0, &id, None).expect("host resume");
        let resumed_storage = resumed.persistence.take().unwrap();
        assert_eq!(resumed.ticks, 75);
        assert_eq!(resumed.running, running);
        assert_eq!(resumed.target_tps, 137.0);
        assert_eq!(resumed.ecology.first_seen(), first_seen);
        assert!(resumed.last.is_none());
        for _ in 0..15 {
            super::super::advance_one(&mut continuous);
            super::super::advance_one(&mut resumed);
        }
        let mut expected = Vec::new();
        let mut actual = Vec::new();
        observe::snapshot_write(
            &mut expected,
            &snapshot_identity(&continuous),
            &continuous.world,
            &continuous.ledger,
        )
        .unwrap();
        observe::snapshot_write(
            &mut actual,
            &snapshot_identity(&resumed),
            &resumed.world,
            &resumed.ledger,
        )
        .unwrap();
        assert!(
            actual == expected,
            "host resume future must be byte-exact, running={running}"
        );
        assert_eq!(
            resumed.ecology.first_seen(),
            continuous.ecology.first_seen()
        );
        stop(resumed_storage);
    }
}
