use super::*;

use super::super::{MicroSnapshot, config, step};

fn legacy_config(seed: u64) -> MicroConfig {
    let raw = config::parse(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../configs/scenarios/cell-chamber.toml"
    )))
    .unwrap();
    config::derive(&raw, seed).unwrap()
}

fn spatial_config(seed: u64, mobility: f64) -> MicroConfig {
    let mut config = legacy_config(seed);
    config.chamber_format = config::SPATIAL_CHAMBER_FORMAT_VERSION;
    config.spatial = Some(config::SpatialConfig {
        dimensions_m: [1e-4; 3],
        viscosity_pa_s: 0.001,
        transport_seed: seed,
    });
    config.founder.genome.spatial = Some(config::SpatialGenome {
        radius_at_division_m: 1e-6,
        mobility_scale: mobility,
    });
    config.validate().unwrap();
    config
}

fn projected(mut snapshot: MicroSnapshot) -> MicroSnapshot {
    for cell in &mut snapshot.cells {
        cell.position_m = None;
        cell.genome.spatial = None;
    }
    snapshot
}

#[test]
fn transport_draw_anchors_full_ordered_le_tuple_and_strict_midpoint_domain() {
    let seed = 0xfedc_ba98_7654_3210u64;
    let id = 0x8877_6655_4433_2211u64;
    let tick = 0x0123_4567_89ab_cdefu64;
    let purpose = 0x1020_3040u32;
    let pair = 0x5060_7080u32;
    let attempt = 0x90a0_b0c0u32;
    let expected_tuple = [
        0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
        0x88, 0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01, 0x40, 0x30, 0x20, 0x10, 0x80, 0x70,
        0x60, 0x50, 0xc0, 0xb0, 0xa0, 0x90,
    ];
    let digest = blake3::derive_key("liminis/cells/transport/v1", &expected_tuple);
    let actual = draw(seed, id, tick, purpose, pair, attempt);
    for (axis, value) in actual.iter().enumerate() {
        let offset = axis * 8;
        let top = u64::from_le_bytes(digest[offset..offset + 8].try_into().unwrap()) >> 12;
        assert_eq!(
            value.to_bits(),
            ((top as f64 + 0.5) * 2.0f64.powi(-52)).to_bits()
        );
        assert!(*value > 0.0 && *value < 1.0);
    }
    let baseline = draw(seed, id, tick, BROWNIAN, 0, 0);
    for changed in [
        draw(seed ^ (1 << 63), id, tick, BROWNIAN, 0, 0),
        draw(seed, id ^ (1 << 63), tick, BROWNIAN, 0, 0),
        draw(seed, id, tick ^ (1 << 63), BROWNIAN, 0, 0),
        draw(seed, id, tick, FOUNDER, 0, 0),
        draw(seed, id, tick, BROWNIAN, 1, 0),
        draw(seed, id, tick, BROWNIAN, 0, 1),
    ] {
        assert_ne!(changed, baseline);
    }
    // The exact two extrema of the top-52 midpoint map cannot produce 0 or 1.
    assert_eq!(0.5 * 2.0f64.powi(-52), 2.0f64.powi(-53));
    assert_eq!(
        (4_503_599_627_370_495.0 + 0.5) * 2.0f64.powi(-52),
        1.0 - 2.0f64.powi(-53)
    );
}

#[test]
fn gaussian_rejection_has_fixed_bound_and_uses_distinct_xy_z_pairs() {
    let mut calls = 0;
    assert!(
        normal_pair_from(|attempt| {
            assert_eq!(attempt, calls);
            calls += 1;
            [0.0, 0.0] // Always outside the polar unit disk.
        })
        .is_err()
    );
    assert_eq!(calls, ATTEMPTS);
    let mut calls = 0;
    let pair = normal_pair_from(|_| {
        calls += 1;
        if calls == 1 { [0.5, 0.5] } else { [0.75, 0.5] }
    })
    .unwrap();
    assert_eq!(calls, 2); // Polar origin also refuses, without ln(0).
    assert!(pair[0].is_finite() && pair[0] > 0.0);
    assert_eq!(pair[1], 0.0);
    let actual = normal(u64::MAX, u64::MAX - 1, u64::MAX - 2).unwrap();
    let xy = normal_pair(u64::MAX, u64::MAX - 1, u64::MAX - 2, 0).unwrap();
    let z = normal_pair(u64::MAX, u64::MAX - 1, u64::MAX - 2, 1).unwrap();
    assert_eq!(actual, [xy[0], xy[1], z[0]]);
}

#[test]
fn founders_are_uniform_in_box_full_seed_and_biology_projection_stays_exact() {
    let old = legacy_config(u64::MAX);
    let spatial = spatial_config(u64::MAX, 1.0);
    let mut legacy_state = MicroState::new(&old).unwrap();
    let mut spatial_state = MicroState::new(&spatial).unwrap();
    assert_eq!(legacy_state.snapshot(), projected(spatial_state.snapshot()));
    for cell in &spatial_state.cells {
        let position = cell.position_m.unwrap();
        assert!(position.iter().all(|&x| x > 0.0 && x < 1e-4));
        for (axis, &x) in position.iter().enumerate() {
            assert_eq!(
                x,
                draw(u64::MAX, cell.id, 0, FOUNDER, axis as u32, 0)[0] * 1e-4
            );
        }
    }
    let other = MicroState::new(&spatial_config(u64::MAX ^ (1 << 63), 1.0)).unwrap();
    assert_ne!(spatial_state.cells[0].position_m, other.cells[0].position_m);
    let mut saw_fission = false;
    for _ in 0..150 {
        let legacy_report = step(&old, &mut legacy_state).unwrap();
        let spatial_report = step(&spatial, &mut spatial_state).unwrap();
        assert_eq!(legacy_report, spatial_report); // Full ledger, IDs and RNG effects.
        saw_fission |= spatial_report.fissions > 0;
        assert_eq!(legacy_state.snapshot(), projected(spatial_state.snapshot()));
    }
    assert!(saw_fission);
}

#[test]
fn legacy_positions_stay_absent_and_spatial_snapshot_refuses_bad_coordinates() {
    let old = legacy_config(42);
    let state = MicroState::new(&old).unwrap();
    assert!(state.cells.iter().all(|cell| cell.position_m.is_none()));
    let spatial = spatial_config(42, 1.0);
    let snapshot = MicroState::new(&spatial).unwrap().snapshot();
    for invalid in [
        None,
        Some([f64::NAN, 0.0, 0.0]),
        Some([f64::INFINITY, 0.0, 0.0]),
        Some([-f64::MIN_POSITIVE, 0.0, 0.0]),
        Some([1e-4f64.next_up(), 0.0, 0.0]),
    ] {
        let mut bad = snapshot.clone();
        bad.cells[0].position_m = invalid;
        assert!(MicroState::from_snapshot(&spatial, bad).is_err());
    }
    let mut bad = state.snapshot();
    bad.cells[0].position_m = Some([0.0; 3]);
    assert!(MicroState::from_snapshot(&old, bad).is_err());
}

#[test]
fn zero_mobility_preserves_coordinate_bits_at_walls_and_subnormal_exactly() {
    let config = spatial_config(42, 0.0);
    let mut state = MicroState::new(&config).unwrap();
    state.cells[0].position_m = Some([-0.0, f64::from_bits(1), 1e-4]);
    let positions: Vec<_> = state
        .cells
        .iter()
        .map(|cell| (cell.id, cell.position_m.unwrap().map(f64::to_bits)))
        .collect();
    advance(&config, &mut state).unwrap();
    for (id, bits) in positions {
        assert_eq!(
            state
                .cells
                .iter()
                .find(|cell| cell.id == id)
                .unwrap()
                .position_m
                .unwrap()
                .map(f64::to_bits),
            bits
        );
    }
    // A tiny-but-positive mobility must not silently become the D=0 fast path.
    let mut underflow = config.clone();
    underflow
        .founder
        .genome
        .spatial
        .as_mut()
        .unwrap()
        .mobility_scale = f64::from_bits(1);
    let mut positive_state = MicroState::new(&underflow).unwrap();
    let before = positive_state.clone();
    assert!(step(&underflow, &mut positive_state).is_err());
    assert_eq!(positive_state, before);
}

#[test]
fn transport_order_is_id_counter_keyed_and_resume_restores_exact_future() {
    let config = spatial_config(u64::MAX, 1.0);
    let mut ordered = MicroState::new(&config).unwrap();
    let mut reversed = ordered.clone();
    reversed.cells.reverse();
    for _ in 0..30 {
        assert_eq!(
            step(&config, &mut ordered).unwrap(),
            step(&config, &mut reversed).unwrap()
        );
        assert_eq!(ordered, reversed);
    }
    // Host tests exercise the actual JSON/disk codec, including coordinate bits.
    let mut resumed = MicroState::from_snapshot(&config, ordered.snapshot()).unwrap();
    for _ in 0..30 {
        assert_eq!(
            step(&config, &mut ordered).unwrap(),
            step(&config, &mut resumed).unwrap()
        );
        for (original, restored) in ordered.cells.iter().zip(&resumed.cells) {
            assert_eq!(
                original.position_m.unwrap().map(f64::to_bits),
                restored.position_m.unwrap().map(f64::to_bits)
            );
        }
        assert_eq!(ordered, resumed);
    }
}

#[test]
fn stokes_einstein_absolute_si_oracle_and_parameter_proportionalities() {
    let mut config = spatial_config(42, 0.375);
    config.temperature_kelvin = 310.0;
    config.dt_seconds = 0.125;
    config.spatial.as_mut().unwrap().viscosity_pa_s = 0.00125;
    let mut genome = config.founder.genome;
    genome.division_mass = 800;
    genome.spatial.as_mut().unwrap().radius_at_division_m = 2.5e-6;
    let mass = 100; // mass/division_mass = 1/8, so current radius is 1.25e-6 m.

    // Independent 80-decimal-digit evaluation using the SI-defined exact
    // k_B = 1.380649e-23 J/K and an 80-digit decimal pi, without production
    // constants: D = mobility*k_B*T/(6*pi*eta*r), sigma = sqrt(2*D*dt).
    // Inputs are T=310 K, eta=0.00125 Pa s, r_division=2.5e-6 m,
    // mass fraction=1/8, mobility=0.375, dt=0.125 s.
    const EXPECTED_D_M2_PER_S: f64 = 5.449_480_403_017_079e-14;
    const EXPECTED_SIGMA_M: f64 = 1.167_206_108_943_176_2e-7;
    let assert_relative = |label: &str, actual: f64, expected: f64| {
        // Allow native cbrt/sqrt rounding, while rejecting unit/factor errors.
        assert!(
            actual.is_finite() && (actual / expected - 1.0).abs() <= 1e-12,
            "{label}: {actual:e}, expected {expected:e}"
        );
    };
    for (label, temperature, viscosity, dt, diffusion_factor, sigma_factor) in [
        ("absolute SI", 310.0, 0.00125, 0.125, 1.0, 1.0),
        ("fourfold T", 1240.0, 0.00125, 0.125, 4.0, 2.0),
        ("fourfold eta", 310.0, 0.005, 0.125, 0.25, 0.5),
        ("fourfold dt", 310.0, 0.00125, 0.5, 1.0, 2.0),
    ] {
        config.temperature_kelvin = temperature;
        config.spatial.as_mut().unwrap().viscosity_pa_s = viscosity;
        config.dt_seconds = dt;
        let actual_sigma = sigma(&config, &genome, mass).unwrap();
        assert_relative(label, actual_sigma, EXPECTED_SIGMA_M * sigma_factor);
        assert_relative(
            label,
            actual_sigma * actual_sigma / (2.0 * dt),
            EXPECTED_D_M2_PER_S * diffusion_factor,
        );
    }
}

#[test]
fn preexisting_mass_sets_radius_and_daughters_inherit_one_parent_endpoint() {
    let config = spatial_config(42, 1.0);
    let mut state = MicroState::new(&config).unwrap();
    for cell in &mut state.cells {
        cell.mass = cell.genome.division_mass;
        cell.energy =
            cell.genome.division_energy_cost + cell.genome.maintenance_energy_per_tick + 100;
        let at_division = sigma(&config, &cell.genome, cell.mass).unwrap();
        let at_eight = sigma(&config, &cell.genome, cell.mass * 8).unwrap();
        assert!((at_division / at_eight - 2.0f64.sqrt()).abs() < 1e-12);
    }
    let mut expected = state.clone();
    advance(&config, &mut expected).unwrap();
    let report = step(&config, &mut state).unwrap();
    assert!(report.fissions > 0);
    for child in &state.cells {
        let parent = expected
            .cells
            .iter()
            .find(|cell| Some(cell.id) == child.parent_id)
            .unwrap();
        assert_eq!(
            child.position_m.unwrap().map(f64::to_bits),
            parent.position_m.unwrap().map(f64::to_bits)
        );
        assert_eq!(child.genome.spatial, parent.genome.spatial);
        assert_eq!(child.age, 0);
    }
}

#[test]
fn unsupported_numeric_range_rejects_entire_tick_after_earlier_candidate_motion() {
    let config = spatial_config(42, 1e-14);
    let mut state = MicroState::new(&config).unwrap();
    // First cell can move; the later cell's positive coefficient is finite but
    // too small relative to box precision because of its large tick-start mass.
    // The inherited spatial genome remains unchanged for every cell.
    state.cells[1].mass = i128::MAX / 16;
    state.validate(&config).unwrap();
    let mut first_only = state.clone();
    first_only.cells.truncate(1);
    advance(&config, &mut first_only).unwrap();
    assert_ne!(first_only.cells[0].position_m, state.cells[0].position_m);
    let before = state.clone();
    let error = format!("{:#}", step(&config, &mut state).unwrap_err());
    assert!(error.contains("error budget"), "{error}");
    assert_eq!(state, before);
    let mut overflow = config.clone();
    overflow.spatial.as_mut().unwrap().viscosity_pa_s = f64::MAX;
    let mut state = MicroState::new(&overflow).unwrap();
    let before = state.clone();
    assert!(step(&overflow, &mut state).is_err());
    assert_eq!(state, before);
}

const STAT_N: usize = 65_536;
const STAT_SEED: u64 = 0x517a_94f0_0000_0000;

#[derive(Default)]
struct Moments {
    sum: [f64; 3],
    squared: [f64; 3],
    cross: [f64; 3],
    radius_squared: f64,
}

impl Moments {
    fn add(&mut self, value: [f64; 3]) {
        for (axis, &component) in value.iter().enumerate() {
            self.sum[axis] += component;
            self.squared[axis] += component * component;
        }
        self.cross[0] += value[0] * value[1];
        self.cross[1] += value[0] * value[2];
        self.cross[2] += value[1] * value[2];
        self.radius_squared += value.iter().map(|x| x * x).sum::<f64>();
    }

    fn assert_isotropic_gaussian(&self, label: &str) {
        // Preregistered n=65536 independent full seeds, fixed six-sigma bounds.
        // No fitted tolerances, rejected tails, wall filtering, or rerun seeds.
        let n = STAT_N as f64;
        let mean_bound = 6.0 / n.sqrt();
        let variance_bound = 6.0 * (2.0 / n).sqrt();
        let cross_bound = 6.0 / n.sqrt();
        let msd_relative_bound = 6.0 * (2.0 / (3.0 * n)).sqrt();
        for axis in 0..3 {
            assert!(
                (self.sum[axis] / n).abs() <= mean_bound,
                "{label} axis {axis} mean"
            );
            assert!(
                (self.squared[axis] / n - 1.0).abs() <= variance_bound,
                "{label} axis {axis} second moment"
            );
            assert!(
                (self.cross[axis] / n).abs() <= cross_bound,
                "{label} cross {axis}"
            );
        }
        assert!(
            (self.radius_squared / (3.0 * n) - 1.0).abs() <= msd_relative_bound,
            "{label} normalized free MSD"
        );
    }
}

#[test]
fn gaussian_mean_axis_variance_cross_msd_and_tails_use_preregistered_seed_ensemble() {
    let mut moments = Moments::default();
    let mut tails = [0usize; 3];
    let mut signed_three_sigma = [0usize; 2];
    for index in 0..STAT_N {
        let value = normal(
            STAT_SEED + index as u64,
            0xf000_0000_0000_002a,
            0xe000_0000_0000_0123,
        )
        .unwrap();
        moments.add(value);
        // Tail counts use one axis per independent tuple, not three correlated
        // polar-pair components treated as extra independent samples.
        for (tail, cutoff) in tails.iter_mut().zip([1.0, 2.0, 3.0]) {
            *tail += usize::from(value[0].abs() > cutoff);
        }
        signed_three_sigma[0] += usize::from(value[0] < -3.0);
        signed_three_sigma[1] += usize::from(value[0] > 3.0);
    }
    moments.assert_isotropic_gaussian("one tick");
    let n = STAT_N as f64;
    // Two-sided N(0,1) tail probabilities at 1, 2 and 3 standard deviations.
    for (count, probability) in tails.into_iter().zip([
        0.317_310_507_862_914_15,
        0.045_500_263_896_358_42,
        0.002_699_796_063_260_19,
    ]) {
        let tolerance = 6.0 * (probability * (1.0 - probability) / n).sqrt();
        assert!(
            (count as f64 / n - probability).abs() <= tolerance,
            "Gaussian tail p={probability}, count={count}"
        );
    }
    for count in signed_three_sigma {
        let probability = 0.002_699_796_063_260_19 / 2.0;
        assert!(
            (count as f64 / n - probability).abs()
                <= 6.0 * (probability * (1.0 - probability) / n).sqrt()
        );
    }
}

#[test]
fn free_fixed_coefficient_multilag_msd_matches_six_dt_without_wall_filtering() {
    let config = spatial_config(STAT_SEED, 1.0);
    let genome = config.founder.genome;
    let sigma = sigma(&config, &genome, config.founder.mass).unwrap();
    let diffusion = sigma * sigma / (2.0 * config.dt_seconds);
    let mut moments: [Moments; 3] = std::array::from_fn(|_| Moments::default());
    let lags = [1, 4, 16];
    for sample in 0..STAT_N {
        let seed = STAT_SEED + sample as u64;
        let mut displacement = [0.0; 3];
        for tick in 0..16 {
            let z = normal(seed, u64::MAX - 13, (1u64 << 48) + tick).unwrap();
            for axis in 0..3 {
                displacement[axis] += sigma * z[axis];
            }
            if let Some(index) = lags.iter().position(|&lag| lag == tick + 1) {
                let expected_axis_sigma =
                    (2.0 * diffusion * config.dt_seconds * (tick + 1) as f64).sqrt();
                moments[index].add(displacement.map(|value| value / expected_axis_sigma));
            }
        }
    }
    for (moment, lag) in moments.into_iter().zip(lags) {
        moment.assert_isotropic_gaussian(&format!("free lag {lag}"));
    }
}

#[test]
fn founder_uniform_ensemble_has_declared_mean_variance_and_no_axis_correlation() {
    let mut config = spatial_config(STAT_SEED, 0.0);
    let mut moments = Moments::default();
    for index in 0..STAT_N {
        config.spatial.as_mut().unwrap().transport_seed = STAT_SEED + index as u64;
        let position = founder_position(&config, u64::MAX - 3).unwrap().unwrap();
        // Standardize uniform box coordinates to mean0/variance1.
        moments.add(position.map(|x| (x / 1e-4 - 0.5) * 12.0f64.sqrt()));
    }
    let n = STAT_N as f64;
    for axis in 0..3 {
        assert!((moments.sum[axis] / n).abs() <= 6.0 / n.sqrt());
        // Standardized uniform fourth moment=1.8: Var(Z²)=0.8.
        assert!((moments.squared[axis] / n - 1.0).abs() <= 6.0 * (0.8 / n).sqrt());
        assert!((moments.cross[axis] / n).abs() <= 6.0 / n.sqrt());
    }
}

#[test]
fn reflections_keep_all_corners_and_multiple_crossings_inside_without_clamps() {
    let lengths = [1.0, 2.0, 4.0];
    for corner in 0..8 {
        for (axis, &length) in lengths.iter().enumerate() {
            let coordinate = if corner & (1 << axis) == 0 {
                0.0
            } else {
                length
            };
            assert_eq!(reflect(coordinate, length).unwrap(), coordinate);
            for wraps in -17..=17 {
                let endpoint = coordinate + wraps as f64 * (2.0 * length);
                assert_eq!(reflect(endpoint, length).unwrap(), coordinate);
            }
        }
    }
    for endpoint in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(reflect(endpoint, 1.0).is_err());
    }
    assert!(reflect(1.0, f64::MAX).is_err());
}

#[test]
fn reflected_uniform_ensemble_preserves_box_distribution_at_small_and_multiple_wall_steps() {
    // Independent fixed full seed sets. Reflecting Brownian endpoints should
    // preserve the uniform stationary distribution; this is not post-wall MSD.
    let lengths = [1.0, 1.3, 0.7];
    let n = STAT_N as f64;
    for base_seed in [42, 0x1_0000_002a] {
        for step_sigma in [0.007, 0.175, 1.4] {
            let mut sum = [0.0; 3];
            let mut centered_squared = [0.0; 3];
            let mut cross = [0.0; 3];
            let mut bins = [[0usize; 32]; 3];
            let mut joint = [0usize; 64];
            for sample in 0..STAT_N {
                let seed = base_seed + sample as u64;
                let z = normal(seed, 0xf000_0000_0000_0001, 0xe000_0000_0000_0001).unwrap();
                let mut centered = [0.0; 3];
                let mut joint_index = 0;
                for axis in 0..3 {
                    let initial = draw(seed, 0xf000_0000_0000_0001, 0, FOUNDER, axis as u32, 0)[0]
                        * lengths[axis];
                    coordinate_budget(lengths[axis], step_sigma).unwrap();
                    let final_position =
                        reflect(step_sigma.mul_add(z[axis], initial), lengths[axis]).unwrap();
                    assert!(
                        final_position.is_finite()
                            && final_position >= 0.0
                            && final_position <= lengths[axis]
                    );
                    let unit = final_position / lengths[axis];
                    centered[axis] = unit - 0.5;
                    sum[axis] += unit;
                    centered_squared[axis] += centered[axis] * centered[axis];
                    bins[axis][((unit * 32.0) as usize).min(31)] += 1;
                    joint_index = joint_index * 4 + ((unit * 4.0) as usize).min(3);
                }
                cross[0] += centered[0] * centered[1];
                cross[1] += centered[0] * centered[2];
                cross[2] += centered[1] * centered[2];
                joint[joint_index] += 1;
            }
            for axis in 0..3 {
                assert!(
                    (sum[axis] / n - 0.5).abs() <= 6.0 / (12.0 * n).sqrt(),
                    "seed{base_seed} sigma{step_sigma} axis{axis} mean"
                );
                assert!(
                    (centered_squared[axis] / n - 1.0 / 12.0).abs() <= 6.0 / (180.0 * n).sqrt(),
                    "seed{base_seed} sigma{step_sigma} axis{axis} moment"
                );
                assert!(
                    (cross[axis] / n).abs() <= 6.0 / (12.0 * n.sqrt()),
                    "seed{base_seed} sigma{step_sigma} cross{axis}"
                );
                for count in bins[axis] {
                    assert!(
                        (count as f64 - n / 32.0).abs()
                            <= 6.0 * (n * (1.0 / 32.0) * (31.0 / 32.0)).sqrt(),
                        "seed{base_seed} sigma{step_sigma} axis{axis} marginal bin"
                    );
                }
            }
            for count in joint {
                assert!(
                    (count as f64 - n / 64.0).abs()
                        <= 6.0 * (n * (1.0 / 64.0) * (63.0 / 64.0)).sqrt(),
                    "seed{base_seed} sigma{step_sigma} joint bin"
                );
            }
        }
    }
}

#[test]
fn represented_reflection_fixtures_include_walls_multiwrap_and_negative_roundoff() {
    // Independent exact-rational modulo of the represented y/L inputs.
    let rows: [(u64, u64, u64, bool); 12] = [
        (
            0x0000000000000000,
            0x3ff0000000000000,
            0x0000000000000000,
            true,
        ),
        (
            0x8000000000000001,
            0x3ff0000000000000,
            0x0000000000000001,
            false,
        ),
        (
            0xbc90000000000000,
            0x3ff0000000000000,
            0x3c90000000000000,
            false,
        ),
        (
            0x3fefffffffffffff,
            0x3ff0000000000000,
            0x3fefffffffffffff,
            true,
        ),
        (
            0x3ff0000000000001,
            0x3ff0000000000000,
            0x3feffffffffffffe,
            true,
        ),
        (
            0x3fffffffffffffff,
            0x3ff0000000000000,
            0x3cb0000000000000,
            true,
        ),
        (
            0x4000000000000001,
            0x3ff0000000000000,
            0x3cc0000000000000,
            true,
        ),
        (
            0xc000000000000001,
            0x3ff0000000000000,
            0x3cc0000000000000,
            true,
        ),
        (
            0x4130000040000000,
            0x3ff0000000000000,
            0x3fd0000000000000,
            true,
        ),
        (
            0x40f9999a1999999a,
            0x3fb999999999999a,
            0x3fa0000000000000,
            false,
        ),
        (
            0xc0f9999a1999999a,
            0x3fb999999999999a,
            0x3fa0000000000000,
            false,
        ),
        (
            0x4000000000000001,
            0x4000000000000000,
            0x3ffffffffffffffe,
            true,
        ),
    ];
    for (y, length, expected, exact) in rows {
        let length = f64::from_bits(length);
        let actual = reflect(f64::from_bits(y), length).unwrap();
        if exact {
            assert_eq!(actual.to_bits(), expected, "y={y:016x}");
        } else {
            let allowance = (2.0 * length).next_up() - 2.0 * length;
            assert!(
                (actual - f64::from_bits(expected)).abs() <= allowance,
                "y={y:016x}"
            );
        }
    }
}

#[test]
fn fma_preserves_cancellation_and_numeric_budget_refuses_unsupported_scales() {
    let x = 1.0;
    let sigma = f64::from_bits(0x3ff0000000000001);
    let z = f64::from_bits(0xbfefffffffffffff);
    assert_eq!(sigma.mul_add(z, x).to_bits(), 0xbc9ffffffffffffe);
    assert_eq!(sigma * z + x, 0.0);
    assert!(coordinate_budget(1.0, 2.0f64.powi(-40)).is_err());
    assert_eq!(
        coordinate_budget(1.0, 2.0f64.powi(-39)).unwrap().to_bits(),
        0x3cc8000000000001
    );
    assert_eq!(
        coordinate_budget(1.0, 32768.0).unwrap().to_bits(),
        0x3dd0000800000001
    );
    assert!(coordinate_budget(1.0, 131072.0).is_err());
    for invalid in [0.0, -1.0, f64::INFINITY, f64::NAN, f64::MAX] {
        assert!(coordinate_budget(invalid, 1.0).is_err());
        assert!(coordinate_budget(1.0, invalid).is_err());
    }
}
