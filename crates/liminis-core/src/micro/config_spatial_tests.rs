use super::*;

const CHAMBER: &str = include_str!("../../../../configs/scenarios/cell-chamber.toml");
const LEGACY_CANONICAL: &str = include_str!("test-data/cell-chamber-v1-canonical.toml");

fn spatial_scenario() -> MicroScenario {
    let mut scenario = parse(CHAMBER).unwrap();
    scenario.chamber_format = SPATIAL_CHAMBER_FORMAT_VERSION;
    scenario.chamber.volume_m3 = None;
    scenario.chamber.spatial = Some(SpatialChamber {
        dimensions_m: [1.0e-4; 3],
        viscosity_pa_s: 1.0e-3,
    });
    scenario.founder.genome.spatial = Some(SpatialGenome {
        radius_at_division_m: 1.0e-6,
        mobility_scale: 1.0,
    });
    scenario
}

#[test]
fn legacy_canonical_bytes_hash_and_absent_spatial_declarations_are_preserved() {
    let scenario = parse(CHAMBER).unwrap();
    assert_eq!(
        canonical(&scenario).unwrap().as_bytes(),
        LEGACY_CANONICAL.as_bytes()
    );
    assert_eq!(config_hash(&scenario).unwrap(), "blake3:1802c0f129855749");
    let config = derive(&scenario, u64::MAX).unwrap();
    assert!(config.spatial.is_none());
    assert!(config.founder.genome.spatial.is_none());
    validate_runtime(&config).unwrap();
    assert_eq!(derive_live(&scenario, u64::MAX).unwrap(), config);
}

#[test]
fn spatial_requires_explicit_entry_points_and_preserves_biology_derivation() {
    let spatial = spatial_scenario();
    let text = canonical(&spatial).unwrap();
    assert!(parse(&text).is_err());
    assert!(validate(&spatial).is_err());
    assert!(derive(&spatial, 42).is_err());
    assert!(derive_spatial(&parse(CHAMBER).unwrap(), 42).is_err());
    let reloaded = parse_live(&text).unwrap();
    assert_eq!(reloaded, spatial);
    let mut config = derive_spatial(&reloaded, 42).unwrap();
    assert_eq!(config, derive_live(&reloaded, 42).unwrap());
    let old = derive(&parse(CHAMBER).unwrap(), 42).unwrap();
    assert_eq!(config.volume_m3, old.volume_m3);
    // Format/hash and opt-in declarations are the only runtime differences.
    config.chamber_format = old.chamber_format;
    config.config_hash = old.config_hash.clone();
    config.spatial = None;
    config.founder.genome.spatial = None;
    assert_eq!(config, old);
}

#[test]
fn spatial_runtime_retains_every_seed_bit() {
    let scenario = spatial_scenario();
    for seed in [0, 1, 1_u64 << 32, u64::MAX] {
        let config = derive_spatial(&scenario, seed).unwrap();
        assert_eq!(config.spatial.unwrap().transport_seed, seed);
        assert_eq!(
            config.run_key,
            run_key(seed),
            "legacy biology seed folding must not change"
        );
        validate_runtime(&config).unwrap();
    }
}

#[test]
fn chamber_declarations_are_complete_and_do_not_hide_a_second_volume_source() {
    for volume in [0.0, -1.0, 1.0e-12, f64::NAN, f64::INFINITY] {
        let mut scenario = spatial_scenario();
        scenario.chamber.volume_m3 = Some(volume);
        assert!(
            validate_live(&scenario).is_err(),
            "format2 rejects presence, not only positive volume"
        );
    }
    let mut scenario = spatial_scenario();
    scenario.chamber.spatial = None;
    assert!(validate_live(&scenario).is_err());
    let mut legacy = parse(CHAMBER).unwrap();
    legacy.chamber.volume_m3 = None;
    assert!(validate_live(&legacy).is_err());
    legacy.chamber.volume_m3 = Some(1.0e-12);
    legacy.chamber.spatial = spatial_scenario().chamber.spatial;
    assert!(validate_live(&legacy).is_err());
    let mut unsupported = spatial_scenario();
    unsupported.chamber_format = 3;
    assert!(validate_live(&unsupported).is_err());
    assert!(derive_live(&unsupported, 42).is_err());
}

#[test]
fn dimensions_and_product_and_viscosity_reject_nonfinite_or_unrepresentable_values() {
    for dimensions in [
        [0.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
        [f64::NAN, 1.0, 1.0],
        [f64::INFINITY, 1.0, 1.0],
        [1.0e200; 3],
        [1.0e-200; 3],
        // Intermediate operations follow the declared (x*y)*z order.
        [1.0e200, 1.0e200, 1.0e-300],
        [1.0e-200, 1.0e-200, 1.0e200],
    ] {
        let mut scenario = spatial_scenario();
        scenario.chamber.spatial.as_mut().unwrap().dimensions_m = dimensions;
        assert!(validate_live(&scenario).is_err());
    }
    for viscosity in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut scenario = spatial_scenario();
        scenario.chamber.spatial.as_mut().unwrap().viscosity_pa_s = viscosity;
        assert!(validate_live(&scenario).is_err());
    }
}

#[test]
fn spatial_genome_requires_both_fields_without_defaults_and_valid_ranges() {
    let text = canonical(&spatial_scenario()).unwrap();
    for field in [
        "radius_at_division_m",
        "mobility_scale",
        "viscosity_pa_s",
        "dimensions_m",
    ] {
        let incomplete = text
            .lines()
            .filter(|line| !line.starts_with(&format!("{field} =")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            parse_live(&incomplete).is_err(),
            "missing {field} must not acquire a default"
        );
    }
    let mut scenario = spatial_scenario();
    scenario.founder.genome.spatial = None;
    assert!(validate_live(&scenario).is_err());
    let mut legacy = parse(CHAMBER).unwrap();
    legacy.founder.genome.spatial = spatial_scenario().founder.genome.spatial;
    assert!(validate_live(&legacy).is_err());
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut scenario = spatial_scenario();
        scenario
            .founder
            .genome
            .spatial
            .as_mut()
            .unwrap()
            .radius_at_division_m = radius;
        assert!(validate_live(&scenario).is_err());
    }
    for mobility in [-1.0, 1.0001, f64::NAN, f64::INFINITY] {
        let mut scenario = spatial_scenario();
        scenario
            .founder
            .genome
            .spatial
            .as_mut()
            .unwrap()
            .mobility_scale = mobility;
        assert!(validate_live(&scenario).is_err());
    }
    for mobility in [0.0, 1.0] {
        let mut scenario = spatial_scenario();
        scenario
            .founder
            .genome
            .spatial
            .as_mut()
            .unwrap()
            .mobility_scale = mobility;
        validate_live(&scenario).unwrap();
        derive_spatial(&scenario, 42).unwrap();
    }
}

#[test]
fn runtime_rejects_contradictory_format_volume_and_spatial_genome() {
    let config = derive_spatial(&spatial_scenario(), 42).unwrap();
    let mut invalid = config.clone();
    invalid.spatial = None;
    assert!(validate_runtime(&invalid).is_err());
    let mut invalid = config.clone();
    invalid.founder.genome.spatial = None;
    assert!(validate_runtime(&invalid).is_err());
    let mut invalid = config.clone();
    invalid.volume_m3 = invalid.volume_m3.next_up();
    assert!(validate_runtime(&invalid).is_err());
    let mut invalid = config.clone();
    invalid.chamber_format = CHAMBER_FORMAT_VERSION;
    assert!(validate_runtime(&invalid).is_err());
    let mut invalid = derive(&parse(CHAMBER).unwrap(), 42).unwrap();
    invalid.founder.genome.spatial = config.founder.genome.spatial;
    assert!(validate_runtime(&invalid).is_err());
    let declaration =
        "dimensions_m = [0.0001, 0.0001, 0.0001]\nviscosity_pa_s = 0.001\ntransport_seed = 42\n";
    for field in ["dimensions_m", "viscosity_pa_s", "transport_seed"] {
        let incomplete = declaration
            .lines()
            .filter(|line| !line.starts_with(&format!("{field} =")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(toml::from_str::<SpatialConfig>(&incomplete).is_err());
    }
}

#[test]
fn reflection_period_must_be_finite_even_when_mobility_is_zero() {
    let dimensions: [f64; 3] = [1.0e308, 1.0e-154, 1.0e-154];
    assert!(((dimensions[0] * dimensions[1]) * dimensions[2]).is_finite());
    for mobility in [0.0, 1.0] {
        let mut scenario = spatial_scenario();
        scenario.chamber.spatial.as_mut().unwrap().dimensions_m = dimensions;
        scenario
            .founder
            .genome
            .spatial
            .as_mut()
            .unwrap()
            .mobility_scale = mobility;
        let error = validate_live(&scenario).unwrap_err().to_string();
        assert!(error.contains("reflection period 2L"), "{error}");
        assert!(derive_spatial(&scenario, 42).is_err());

        let mut runtime = derive_spatial(&spatial_scenario(), 42).unwrap();
        runtime.spatial.as_mut().unwrap().dimensions_m = dimensions;
        runtime.volume_m3 = (dimensions[0] * dimensions[1]) * dimensions[2];
        runtime
            .founder
            .genome
            .spatial
            .as_mut()
            .unwrap()
            .mobility_scale = mobility;
        let error = validate_runtime(&runtime).unwrap_err().to_string();
        assert!(error.contains("reflection period 2L"), "{error}");
    }
}
