use super::*;

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
            r#"{"action":"speed","value":101}"#
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
