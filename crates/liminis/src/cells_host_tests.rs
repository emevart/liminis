use super::*;

struct Temp(std::path::PathBuf);

impl Temp {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "liminis-cells-host-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SCENARIO: &str = include_str!("../../../configs/scenarios/cell-chamber.toml");

fn fixture() -> Sim {
    build(SCENARIO, u64::MAX).expect("cell scenario")
}

fn request(shared: &Arc<Mutex<Sim>>, method: Method, path: &str, body: &str) -> Response {
    route(
        shared,
        &Request {
            method,
            path: path.into(),
            query: String::new(),
            body: body.as_bytes().to_vec(),
        },
    )
}

#[test]
fn cell_routes_are_separate_and_controls_publish_checked_ticks() {
    let shared = Arc::new(Mutex::new(fixture()));
    let response = request(&shared, Method::Get, "/api/state", "");
    let state: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(state["kind"], "cells");
    assert_eq!(state["seed"], u64::MAX.to_string());
    assert!(state["residual"].is_null());
    assert_eq!(state["model"]["environment"], "well_mixed");
    assert_eq!(
        request(&shared, Method::Get, "/api/volume/BIO", "").status,
        404
    );
    assert_eq!(
        request(&shared, Method::Post, "/api/state", "{}").status,
        405
    );
    assert_eq!(
        request(
            &shared,
            Method::Post,
            "/api/control",
            r#"{"action":"speed","value":0}"#
        )
        .status,
        400
    );
    let response = request(
        &shared,
        Method::Post,
        "/api/control",
        r#"{"action":"step"}"#,
    );
    assert_eq!(response.status, 200);
    let state: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(state["tick"], 1);
    assert_eq!(state["running"], false);
    assert_eq!(state["residual"], json!({"matter":"0","energy":"0"}));
}

#[test]
fn exact_cell_snapshot_and_observer_restore_the_same_future() {
    let mut original = fixture();
    for _ in 0..137 {
        advance_one(&mut original).unwrap();
    }
    validate_observation(&original).unwrap();
    let captured = capture(&original);
    let saved: SavedState = serde_json::from_value(captured.state).unwrap();
    let mut restored = fixture();
    restored.state = MicroState::from_snapshot(&restored.config, saved.core).unwrap();
    restored.observation = saved.observation;
    restored.residual = None;
    validate_observation(&restored).unwrap();
    assert!(state_json(&restored)["residual"].is_null());
    for _ in 0..89 {
        advance_one(&mut original).unwrap();
        advance_one(&mut restored).unwrap();
        validate_observation(&restored).unwrap();
        assert_eq!(capture(&original).state, capture(&restored).state);
        assert_eq!(metric(&original), metric(&restored));
    }
    restored.observation.medium_matter[0] += 1;
    assert!(validate_observation(&restored).is_err());
    restored.observation.medium_matter[0] -= 1;
    restored.state.next_cell_id += 1;
    assert!(
        validate_observation(&restored)
            .unwrap_err()
            .to_string()
            .contains("next cell ID")
    );
}

#[test]
fn cell_seed_controls_preserve_all_64_bits() {
    assert_eq!(
        control_seed(&json!({"seed":u64::MAX.to_string()}), 0).unwrap(),
        u64::MAX
    );
    assert!(control_seed(&json!({"seed":42}), 0).is_err());
    assert!(control_seed(&json!({"seed":"-1"}), 0).is_err());
}

#[test]
fn observer_overflow_does_not_publish_a_partially_advanced_cell_state() {
    let mut sim = fixture();
    let cell = &mut sim.state.cells[0];
    cell.mass = cell.genome.division_mass;
    cell.energy =
        cell.genome.division_energy_cost + cell.genome.maintenance_energy_per_tick * 2 + 1;
    sim.observation.births = u64::MAX;
    let before = capture(&sim).state;
    assert!(
        advance_one(&mut sim)
            .unwrap_err()
            .to_string()
            .contains("birth counter exhausted")
    );
    assert_eq!(capture(&sim).state, before);
    assert_eq!(sim.state.tick, 0);
    assert!(sim.residual.is_none());
}

#[test]
fn inherited_float_physiology_survives_the_actual_json_checkpoint_codec() {
    let mut sim = fixture();
    sim.config.founder.genome.max_growth_rate_per_second = 1.491_542_492_935_830_4e-9;
    sim.state = MicroState::new(&sim.config).unwrap();
    let bytes = serde_json::to_vec(&sim.state.snapshot()).unwrap();
    let snapshot: micro::MicroSnapshot = serde_json::from_slice(&bytes).unwrap();
    let mut restored = MicroState::from_snapshot(&sim.config, snapshot).unwrap();
    assert_eq!(sim.state.snapshot(), restored.snapshot());
    for _ in 0..30 {
        assert_eq!(
            micro::step(&sim.config, &mut sim.state).unwrap(),
            micro::step(&sim.config, &mut restored).unwrap()
        );
        assert_eq!(sim.state.snapshot(), restored.snapshot());
    }
}

#[test]
fn cell_world_29_resume_step_and_resave_preserve_its_identity() {
    let root = Temp::new();
    let mut original = fixture();
    original.world_format_version = 29;
    start_storage(&mut original, &root.0).unwrap();
    advance_one(&mut original).unwrap();
    save(&mut original).unwrap();
    original.storage.as_mut().unwrap().wait_for_save().unwrap();
    let run_id = original.storage.as_ref().unwrap().status()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    drop(original.storage.take());

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut resumed = loop {
        match resume_sim(&root.0, &run_id, None) {
            Ok(sim) => break sim,
            Err(error) if error.to_string().contains("writer") && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("world 29 resume failed: {error:#}"),
        }
    };
    assert_eq!(resumed.world_format_version, 29);
    assert_eq!(state_json(&resumed)["world_format_version"], 29);
    assert_eq!(capture(&resumed).world_format_version, 29);
    advance_one(&mut resumed).unwrap();
    save(&mut resumed).unwrap();
    resumed.storage.as_mut().unwrap().wait_for_save().unwrap();
    let latest: Value =
        serde_json::from_slice(&std::fs::read(root.0.join(&run_id).join("latest.json")).unwrap())
            .unwrap();
    assert_eq!(latest["world_format_version"], 29);
    drop(resumed.storage.take());
}

#[test]
fn multipliers_validate_without_old_tps_clamps_and_maximum_retains_manual_speed() {
    let shared = Arc::new(Mutex::new(fixture()));
    for body in [
        r#"{"action":"speed","value":0}"#,
        r#"{"action":"speed","value":-1}"#,
        r#"{"action":"speed","value":"NaN"}"#,
        r#"{"action":"speed","value":1e999}"#,
        r#"{"action":"speed","value":null}"#,
    ] {
        assert_eq!(
            request(&shared, Method::Post, "/api/control", body).status,
            400
        );
    }
    assert!(Pacing::manual(f64::NAN, 30.0).is_err());
    assert!(Pacing::manual(f64::INFINITY, 30.0).is_err());
    assert!(Pacing::manual(f64::from_bits(1), 30.0).is_err());
    for multiplier in [1.0e-200, 1.0, 100_000.0, f64::MAX] {
        let body = json!({"action":"speed", "value":multiplier}).to_string();
        assert_eq!(
            request(&shared, Method::Post, "/api/control", &body).status,
            200
        );
        let sim = lock(&shared);
        assert_eq!(sim.state.tick, 0);
        assert_eq!(sim.pacing.multiplier, multiplier);
        assert_eq!(sim.target_tps, multiplier / sim.config.dt_seconds);
    }
    assert_eq!(
        request(
            &shared,
            Method::Post,
            "/api/control",
            r#"{"action":"maximum"}"#
        )
        .status,
        200
    );
    let sim = lock(&shared);
    assert_eq!(sim.pacing.mode, PaceMode::Maximum);
    assert_eq!(sim.pacing.multiplier, f64::MAX);
    assert!(state_json(&sim)["target_tps"].is_null());
    assert_eq!(sim.config.dt_seconds, 30.0);
}

#[test]
fn new_pacing_envelopes_restore_and_legacy_tick_rates_remain_exact() {
    let mut sim = fixture();
    for mode in [PaceMode::Manual, PaceMode::Maximum] {
        sim.pacing = Pacing {
            mode,
            multiplier: 0.125,
        };
        sim.target_tps = sim.pacing.tps(sim.config.dt_seconds);
        let saved: SavedState = serde_json::from_value(capture(&sim).state).unwrap();
        assert_eq!(
            restore_pacing(&saved, sim.target_tps, 30.0).unwrap(),
            sim.pacing
        );
        assert!(restore_pacing(&saved, sim.target_tps * 2.0, 30.0).is_err());
    }
    // This rate does not necessarily survive tps*dt/dt bit for bit.
    for tps in [
        0.5339,
        0.500_000_000_000_000_1,
        std::f64::consts::PI,
        99.123_456_789,
    ] {
        let mut legacy = capture(&sim).state;
        legacy["format"] = json!(1);
        legacy.as_object_mut().unwrap().remove("pacing");
        let saved: SavedState = serde_json::from_value(legacy).unwrap();
        sim.pacing = restore_pacing(&saved, tps, 30.0).unwrap();
        sim.target_tps = tps;
        assert_eq!(capture(&sim).tps.to_bits(), tps.to_bits());
        let migrated: SavedState = serde_json::from_value(capture(&sim).state).unwrap();
        assert_eq!(restore_pacing(&migrated, tps, 30.0).unwrap(), sim.pacing);
    }
    let mut bad = capture(&sim).state;
    bad["format"] = json!(1);
    assert!(restore_pacing(&serde_json::from_value(bad).unwrap(), sim.target_tps, 30.0).is_err());
    let mut bad = capture(&sim).state;
    bad.as_object_mut().unwrap().remove("pacing");
    assert!(restore_pacing(&serde_json::from_value(bad).unwrap(), sim.target_tps, 30.0).is_err());
}

fn start_worker(sim: Sim) -> (Arc<Mutex<Sim>>, std::thread::JoinHandle<()>) {
    let shared = Arc::new(Mutex::new(sim));
    let worker_shared = Arc::clone(&shared);
    let worker = std::thread::spawn(move || run_loop(&worker_shared));
    (shared, worker)
}

fn stop_worker(shared: &Arc<Mutex<Sim>>, worker: std::thread::JoinHandle<()>) {
    lock(shared).alive = false;
    worker.join().unwrap();
}

#[test]
fn one_x_waits_the_real_dt_and_pause_step_speed_changes_discard_debt() {
    let mut sim = fixture();
    sim.pacing = Pacing::manual(1.0, sim.config.dt_seconds).unwrap();
    sim.target_tps = sim.pacing.tps(sim.config.dt_seconds);
    let (shared, worker) = start_worker(sim);
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(
        lock(&shared).state.tick,
        0,
        "1x at dt30 must not tick immediately"
    );
    assert_eq!(lock(&shared).measured_tps, 0.0);
    assert_eq!(
        request(
            &shared,
            Method::Post,
            "/api/control",
            r#"{"action":"pause"}"#
        )
        .status,
        200
    );
    assert_eq!(
        request(
            &shared,
            Method::Post,
            "/api/control",
            r#"{"action":"step"}"#
        )
        .status,
        200
    );
    assert_eq!(lock(&shared).state.tick, 1);
    assert_eq!(
        request(&shared, Method::Post, "/api/control", r#"{"action":"run"}"#).status,
        200
    );
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(lock(&shared).state.tick, 1);
    assert_eq!(
        request(
            &shared,
            Method::Post,
            "/api/control",
            r#"{"action":"maximum"}"#
        )
        .status,
        200
    );
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        request(
            &shared,
            Method::Post,
            "/api/control",
            r#"{"action":"pause"}"#
        )
        .status,
        200
    );
    let tick = lock(&shared).state.tick;
    assert!(tick > 1);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(lock(&shared).state.tick, tick);
    assert_eq!(
        request(
            &shared,
            Method::Post,
            "/api/control",
            r#"{"action":"speed","value":1}"#
        )
        .status,
        200
    );
    assert_eq!(
        request(&shared, Method::Post, "/api/control", r#"{"action":"run"}"#).status,
        200
    );
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        lock(&shared).state.tick,
        tick,
        "paused Maximum must not leave catchup debt"
    );
    stop_worker(&shared, worker);
}

#[test]
fn actual_maximum_and_manual_workers_match_manual_step_core_rng_ids_and_balances() {
    for pacing in [
        Pacing {
            mode: PaceMode::Maximum,
            multiplier: 90.0,
        },
        Pacing {
            mode: PaceMode::Manual,
            multiplier: 30_000.0,
        },
    ] {
        let mut sim = fixture();
        sim.pacing = pacing;
        sim.target_tps = pacing.tps(sim.config.dt_seconds);
        let (shared, worker) = start_worker(sim);
        std::thread::sleep(Duration::from_millis(40));
        let started = Instant::now();
        assert_eq!(request(&shared, Method::Get, "/api/state", "").status, 200);
        assert_eq!(
            request(
                &shared,
                Method::Post,
                "/api/control",
                r#"{"action":"pause"}"#
            )
            .status,
            200
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "read/pause should acquire between checked ticks"
        );
        let actual = lock(&shared);
        let mut stepped = fixture();
        assert!(actual.state.tick > 0);
        for _ in 0..actual.state.tick {
            advance_one(&mut stepped).unwrap();
        }
        assert_eq!(actual.state.snapshot(), stepped.state.snapshot());
        assert_eq!(
            serde_json::to_value(&actual.observation).unwrap(),
            serde_json::to_value(&stepped.observation).unwrap()
        );
        assert_eq!(metric(&actual), metric(&stepped));
        validate_observation(&actual).unwrap();
        drop(actual);
        stop_worker(&shared, worker);
    }
}

fn resume_after_writer(root: &Path, run: &str) -> Sim {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match resume_sim(root, run, None) {
            Ok(sim) => return sim,
            Err(error) if error.to_string().contains("writer") && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("resume failed: {error:#}"),
        }
    }
}

#[test]
fn legacy_checkpoint_migrates_without_rounding_speed_and_keeps_exact_future() {
    let root = Temp::new();
    let mut original = fixture();
    for _ in 0..137 {
        advance_one(&mut original).unwrap();
    }
    original.target_tps = 0.5339;
    original.pacing =
        Pacing::manual(original.target_tps * original.config.dt_seconds, 30.0).unwrap();
    start_storage(&mut original, &root.0).unwrap();
    let mut legacy = capture(&original);
    legacy.state["format"] = json!(1);
    legacy.state.as_object_mut().unwrap().remove("pacing");
    original.storage.as_mut().unwrap().save(legacy).unwrap();
    original.storage.as_mut().unwrap().wait_for_save().unwrap();
    let run = original.storage.as_ref().unwrap().status()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    drop(original.storage.take());
    let mut resumed = resume_after_writer(&root.0, &run);
    assert_eq!(resumed.target_tps.to_bits(), original.target_tps.to_bits());
    assert_eq!(resumed.state.snapshot(), original.state.snapshot());
    resumed.pacing.mode = PaceMode::Maximum;
    save(&mut resumed).unwrap();
    resumed.storage.as_mut().unwrap().wait_for_save().unwrap();
    drop(resumed.storage.take());
    let mut second = resume_after_writer(&root.0, &run);
    assert_eq!(second.target_tps.to_bits(), original.target_tps.to_bits());
    assert_eq!(second.pacing.mode, PaceMode::Maximum);
    assert_eq!(second.state.snapshot(), original.state.snapshot());
    for _ in 0..89 {
        advance_one(&mut original).unwrap();
        advance_one(&mut second).unwrap();
        assert_eq!(second.state.snapshot(), original.state.snapshot());
        assert_eq!(metric(&second), metric(&original));
    }
    drop(second.storage.take());
}

#[test]
fn pause_and_step_checkpoint_requests_survive_a_full_optional_queue() {
    for action in ["pause", "step"] {
        let root = Temp::new();
        let mut sim = fixture();
        start_storage(&mut sim, &root.0).unwrap();
        let release = sim.storage.as_ref().unwrap().test_block_worker();
        let shared = Arc::new(Mutex::new(sim));
        let command = json!({"action": action}).to_string();
        assert_eq!(
            request(&shared, Method::Post, "/api/control", &command).status,
            200
        );
        {
            let mut sim = lock(&shared);
            assert!(sim.save_requested);
            assert!(state_json(&sim)["save_pending"].as_bool().unwrap());
            assert!(!sim.storage.as_ref().unwrap().busy());
            assert!(!sim.running);
            service_storage(&mut sim);
            assert!(
                sim.save_requested,
                "retry must retain the only bounded pending request"
            );
        }
        assert_eq!(
            request(
                &shared,
                Method::Post,
                "/api/control",
                r#"{"action":"save"}"#
            )
            .status,
            409
        );
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut sim = lock(&shared);
            service_storage(&mut sim);
            if !sim.save_requested {
                sim.storage.as_mut().unwrap().wait_for_save().unwrap();
                assert_eq!(
                    sim.storage.as_ref().unwrap().status()["saved_tick"],
                    sim.state.tick
                );
                assert!(!sim.running);
                break;
            }
            assert!(Instant::now() < deadline);
            drop(sim);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[test]
fn autosave_backpressure_does_not_stop_maximum_and_retries_checkpoint() {
    let root = Temp::new();
    let mut sim = fixture();
    sim.pacing.mode = PaceMode::Maximum;
    start_storage(&mut sim, &root.0).unwrap();
    let release = sim.storage.as_ref().unwrap().test_block_worker();
    sim.storage.as_mut().unwrap().test_make_autosave_due();
    assert!(safe_advance(&mut sim));
    assert!(sim.running);
    assert!(sim.save_requested);
    assert_eq!(sim.state.tick, 1);
    assert!(sim.storage.as_ref().unwrap().error().is_none());
    assert!(safe_advance(&mut sim));
    assert_eq!(sim.state.tick, 2);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while sim.save_requested {
        service_storage(&mut sim);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    sim.storage.as_mut().unwrap().wait_for_save().unwrap();
    assert!(sim.running);
    assert_eq!(sim.storage.as_ref().unwrap().status()["saved_tick"], 2);
}
