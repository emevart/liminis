//! Acceptance criteria of the prescribed velocity field (`ACCEPTANCE.md`,
//! "Observable behaviour"):
//!
//! ```text
//! heated_bottom_produces_net_vertical_transport
//! horizontally_uniform_temperature_produces_no_velocity
//! prescribed_velocity_is_divergence_free_bit_for_bit
//! ```
//!
//! Outside the crate rather than beside the kernels, for the reason
//! `acceptance_light.rs` states: an acceptance test is the outside view. It may
//! use only what a scenario can use — the public surface of `liminis-core` — so
//! that it keeps its meaning when the inside is rearranged.
//!
//! The unit tests under `src/kernels/` do not overlap these. They pin the stencils
//! and the layouts; these pin that the mechanism moves matter, and by how much
//! against what.

use liminis_core::kernels::advect::{AdvectParams, advect_voxel_32};
use liminis_core::kernels::diffuse::{DiffuseParams, diffuse_voxel_32};
use liminis_core::numeric::{M32, M64, Q, run_key};
use liminis_core::process::substeps_and_alpha;
use liminis_core::process::velocity::{VelocityConfig, VelocityField};
use liminis_core::world::{Boundary, Grid};

/// The fine grid. Sixteen voxels an axis rather than the 128 of SPEC section 1.1,
/// with the three grids keeping the relation ADR-069 fixes: velocity at
/// `lod = 1`, enthalpy at `lod = 2`.
const NX: u32 = 16;
const N_VOXELS: u32 = NX * NX * NX;
const VN: u32 = NX / 2;
const CN: u32 = NX / 4;
const N_COARSE: u32 = CN * CN * CN;

/// A tick of a second, and a voxel that keeps the domain the size the arithmetic
/// of ADR-069 is written for: `16 * 800 um = 12.8 mm`. The eco regime reaches the
/// same domain with `dx = 100 um` on 128^3, and the numbers that matter here — the
/// Peclet number over the domain, the crossing time against `H^2/D` — are
/// properties of the **domain**, not of how many voxels it is cut into.
const DT: f64 = 1.0;
const DX: f64 = 8.0e-4;
const DOMAIN: f64 = NX as f64 * DX;

/// The declared temperature range of ADR-062, and the source amplitude: the
/// estimate of `|u|` is derived at `|T~| <= t_max - t_min`, so a source of exactly
/// that size is the one that reaches the bound.
const T_MIN: f64 = 273.15;
const T_MAX: f64 = 323.15;
const SPAN: f64 = T_MAX - T_MIN;

/// The diffusivity of oxygen, m^2/s (SPEC section 1.7). Taken from the corpus
/// rather than invented, because the threshold of `ACCEPTANCE.md` — "an order of
/// magnitude stronger than diffusion" — is a comparison against a real number.
const D_OXYGEN: f64 = 2.0e-9;

/// One unit of tracer per voxel of the initial layer, scaled so that a fraction
/// of `1e-5` of it is still a whole number of units.
const TRACER: i32 = 1_000_000;

/// The initial layer: the bottom quarter of the domain.
const LAYER_TOP: u32 = NX / 4;

fn floored(n: u32) -> Grid {
    Grid::new(
        n,
        n,
        n,
        [
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Closed,
            Boundary::Closed,
        ],
    )
    .unwrap()
}

fn config(l_c: f64, stir_fraction: f64) -> VelocityConfig {
    VelocityConfig {
        // The ceiling of ADR-069: the conservative outgoing-flux condition
        // `dx/(6*dt)`, that is a Courant number of 0.167. The factor of three that
        // a divergence-free field would seem to buy is not taken: the divergence
        // is exact on the velocity grid and not on the fine faces.
        u_conv_max: DX / (6.0 * DT),
        l_c,
        stir_fraction,
        stir_period: if stir_fraction > 0.0 {
            Some(60.0)
        } else {
            None
        },
        dt: DT,
        dx: DX,
        t_min: T_MIN,
        t_max: T_MAX,
        units_per_joule: 1.0,
        every_n_ticks: 1,
    }
}

fn field(cfg: &VelocityConfig) -> VelocityField {
    VelocityField::new(&floored(NX), &floored(VN), &floored(CN), cfg).unwrap()
}

fn coarse_index(x: u32, y: u32, z: u32) -> u32 {
    x + y * CN + z * CN * CN
}

fn fine_index(x: u32, y: u32, z: u32) -> u32 {
    x + y * NX + z * NX * NX
}

/// A unit heat capacity on the coarse grid, so that one storage unit of enthalpy
/// is one kelvin of anomaly and the source below is declared in kelvins.
fn unit_capacity() -> Vec<Q> {
    vec![Q::ONE; N_COARSE as usize]
}

/// A **localised** source on the floor: a two-by-two patch of coarse columns,
/// hottest at `z = 0` at the full declared temperature span and decaying linearly
/// to nothing at the lid.
///
/// Localised **horizontally**, because a hot plane is horizontally uniform and
/// drives nothing at all — that is the other test in this file. Spread
/// **vertically**, because the field is prescribed from the enthalpy as it stands
/// and nothing here evolves it: a source confined to the bottom coarse layer gives
/// a potential confined to that layer and therefore a flow four fine voxels deep,
/// which cannot carry anything anywhere. The linear decay is the steady profile
/// conduction leaves above a floor source under a cold lid, and conduction is fast
/// — ADR-062 puts `H^2/alpha` at 1170 s against the 768 s the flow needs to cross
/// the domain — so this is the shape the enthalpy field actually presents to step
/// `b`, not a convenience.
///
/// The enthalpy is set directly rather than through the `VENT_BURST` machinery of
/// SPEC section 7, which does not exist: what is under test is the velocity field,
/// and the source is its input.
fn heated_bottom() -> Vec<M64> {
    let mut enthalpy = vec![M64::ZERO; N_COARSE as usize];
    let bump = |coord: u32| -> f64 {
        // One full wavelength across the domain, peaking at the centre and zero at
        // the wrap. The horizontal scale is the domain, because `l_c` is half of it
        // and the transfer function of the composed stencil peaks near `4*l_c`: a
        // patch two coarse cells wide sits far down the flank of that curve and
        // reaches a fifth of the ceiling.
        0.5 * (1.0 - (2.0 * std::f64::consts::PI * (f64::from(coord) + 0.5) / f64::from(CN)).cos())
    };
    for z in 0..CN {
        let amplitude = SPAN * f64::from(CN - z) / f64::from(CN);
        for y in 0..CN {
            for x in 0..CN {
                let value = amplitude * bump(x) * bump(y);
                enthalpy[coarse_index(x, y, z) as usize] = M64::new(value as i64);
            }
        }
    }
    enthalpy
}

/// A source that depends on `z` alone: the control. Stratified, and stratified is
/// not a defect — the point is that stratification on its own drives nothing.
fn stratified() -> Vec<M64> {
    let mut enthalpy = vec![M64::ZERO; N_COARSE as usize];
    for z in 0..CN {
        for y in 0..CN {
            for x in 0..CN {
                enthalpy[coarse_index(x, y, z) as usize] = M64::new(SPAN as i64 - 4 * i64::from(z));
            }
        }
    }
    enthalpy
}

/// One application of the whole of step `b`, from the enthalpy to the Courant
/// numbers on the faces of the fine grid.
fn face_courant_of(field: &VelocityField, enthalpy: &[M64], tick: u32, seed: u64) -> Vec<Q> {
    let mut coarse_potential = vec![Q::ZERO; (3 * N_COARSE) as usize];
    let mut potential = vec![Q::ZERO; (3 * VN * VN * VN) as usize];
    let mut stirred = vec![Q::ZERO; potential.len()];
    let mut velocity = vec![Q::ZERO; potential.len()];
    let mut face_courant = vec![Q::ZERO; (3 * N_VOXELS) as usize];

    field.apply(
        enthalpy,
        &unit_capacity(),
        &mut coarse_potential,
        &mut potential,
        &mut stirred,
        &mut velocity,
        &mut face_courant,
        tick,
        run_key(seed),
    );
    face_courant
}

/// The velocity field itself, for the assertions that are about `u` rather than
/// about what it carries.
fn velocity_of(field: &VelocityField, enthalpy: &[M64], tick: u32, seed: u64) -> Vec<Q> {
    let mut coarse_potential = vec![Q::ZERO; (3 * N_COARSE) as usize];
    let mut potential = vec![Q::ZERO; (3 * VN * VN * VN) as usize];
    let mut stirred = vec![Q::ZERO; potential.len()];
    let mut velocity = vec![Q::ZERO; potential.len()];
    let mut face_courant = vec![Q::ZERO; (3 * N_VOXELS) as usize];

    field.apply(
        enthalpy,
        &unit_capacity(),
        &mut coarse_potential,
        &mut potential,
        &mut stirred,
        &mut velocity,
        &mut face_courant,
        tick,
        run_key(seed),
    );
    velocity
}

/// A layer of tracer along the floor.
fn initial_tracer() -> Vec<M32> {
    let mut tracer = vec![M32::ZERO; N_VOXELS as usize];
    for z in 0..LAYER_TOP {
        for y in 0..NX {
            for x in 0..NX {
                tracer[fine_index(x, y, z) as usize] = M32::new(TRACER);
            }
        }
    }
    tracer
}

fn mass_above_midplane(tracer: &[M32]) -> i64 {
    let mut total = 0i64;
    for z in NX / 2..NX {
        for y in 0..NX {
            for x in 0..NX {
                total += tracer[fine_index(x, y, z) as usize].to_i64();
            }
        }
    }
    total
}

fn total_mass(tracer: &[M32]) -> i64 {
    tracer.iter().map(|v| v.to_i64()).sum()
}

/// `ticks` ticks of advection: three axis applications a tick, in a fixed order,
/// which is world semantics in its own right (ADR-036).
fn advect(tracer: &[M32], face_courant: &[Q], ticks: u32) -> Vec<M32> {
    let mut src = tracer.to_vec();
    let mut dst = vec![M32::ZERO; N_VOXELS as usize];
    for _ in 0..ticks {
        for axis in 0..3u32 {
            let params = AdvectParams {
                nx: NX,
                ny: NX,
                nz: NX,
                periodic_mask: 0b00_1111,
                axis,
            };
            for idx in 0..N_VOXELS {
                advect_voxel_32(&src, face_courant, &mut dst, &params, idx);
            }
            std::mem::swap(&mut src, &mut dst);
        }
    }
    src
}

/// `ticks` ticks of pure diffusion at the same `D`, on the same grid.
fn diffuse(tracer: &[M32], ticks: u32) -> Vec<M32> {
    let (substeps, alpha) = substeps_and_alpha(D_OXYGEN, DT, DX).unwrap();
    let params = DiffuseParams {
        nx: NX,
        ny: NX,
        nz: NX,
        periodic_mask: 0b00_1111,
        alpha: Q::from_f64(alpha),
    };
    let mut src = tracer.to_vec();
    let mut dst = vec![M32::ZERO; N_VOXELS as usize];
    for _ in 0..ticks * substeps {
        for idx in 0..N_VOXELS {
            diffuse_voxel_32(&src, &mut dst, &params, idx);
        }
        std::mem::swap(&mut src, &mut dst);
    }
    src
}

/// How many ticks the flow of a given field needs to cross the domain twice.
///
/// Measured from the field rather than assumed, and the difference is the whole
/// reason this is a function. `u_conv_max` is the ceiling the mobility is
/// **calibrated** to — it is reached only by the worst-case sign pattern of the
/// composed stencil, and a smooth source of domain scale runs at a fifth of it.
/// A tick count taken from the ceiling would therefore stop the run at a fifth of
/// a transit, and the threshold would be missed for a reason that has nothing to
/// do with the mechanism.
///
/// Two transits and not one: the flow is a closed circulation — it is a curl —
/// so what crosses the midplane is the correlation `<w'c'>`, and one pass up the
/// plume is the first half of a cycle rather than a steady state.
fn transits(field: &VelocityField, enthalpy: &[M64]) -> u32 {
    let fastest = velocity_of(field, enthalpy, 0, 42)
        .iter()
        .fold(0.0f64, |m, v| m.max(v.debug_f64().abs()));
    assert!(fastest > 0.0, "the field is identically zero");
    let ticks = (2.0 * DOMAIN / (fastest * DT)).ceil() as u32;
    assert!(
        ticks <= 4000,
        "two transits would take {ticks} ticks at {fastest} m/s, which is slower \
         than this test is meant to be: the mobility is out by orders"
    );
    ticks
}

#[test]
fn heated_bottom_produces_net_vertical_transport() {
    // The place where the prescribed field stops being decoration. The threshold
    // is `ACCEPTANCE.md`'s, word for word: convection either carries matter upward
    // an order of magnitude harder than diffusion, or it does not work.
    //
    // `l_c` is half the domain — `ACCEPTANCE.md` requires at least a quarter, and
    // the reason is not caution: at `l_c = 400 um` the flow closes into cells, the
    // enhancement goes as `Pe^(1/2)` and comes to 1.8 times diffusion, and the
    // threshold is missed for a reason that has nothing to do with the mechanism.
    //
    // `stir_fraction = 0`, so what is measured is the convective correction and
    // not the noise.
    let cfg = config(DOMAIN / 2.0, 0.0);
    let field = field(&cfg);
    let courant = face_courant_of(&field, &heated_bottom(), 0, 42);

    let ticks = transits(&field, &heated_bottom());
    let tracer = initial_tracer();
    let start = mass_above_midplane(&tracer);
    let carried = advect(&tracer, &courant, ticks);
    let diffused = diffuse(&tracer, ticks);

    // Transport conserves, in both runs and exactly. A ratio measured over a run
    // that printed matter would mean nothing.
    assert_eq!(
        total_mass(&carried),
        total_mass(&tracer),
        "advection lost matter"
    );
    assert_eq!(
        total_mass(&diffused),
        total_mass(&tracer),
        "diffusion lost matter"
    );

    let by_flow = mass_above_midplane(&carried) - start;
    let by_diffusion = mass_above_midplane(&diffused) - start;

    // The Peclet number over the **domain**, printed rather than asserted, so that
    // "the threshold was not reached on a small grid" reads differently from "the
    // mechanism does not work". ADR-069 computes it as the ratio of the diffusive
    // crossing time to the advective one.
    let peclet = cfg.u_conv_max * DOMAIN / D_OXYGEN;
    let fastest = velocity_of(&field, &heated_bottom(), 0, 42)
        .iter()
        .fold(0.0f64, |m, v| m.max(v.debug_f64().abs()));
    assert!(
        by_diffusion >= 0 && by_flow > 0,
        "nothing crossed the midplane at all: {by_flow} by flow, {by_diffusion} by \
         diffusion. Pe over the domain is {peclet}, and the fastest cell of the \
         field runs at {fastest} m/s against a ceiling of {}",
        cfg.u_conv_max
    );
    assert!(
        by_flow >= 10 * by_diffusion.max(1),
        "the flow carried {by_flow} units over the midplane against {by_diffusion} \
         by diffusion alone, a ratio of {:.2} under the threshold of ten. Pe over \
         the domain is {peclet}, and the run was {ticks} ticks, two crossings of \
         the domain by the flow: if Pe is of order ten the threshold is out of \
         reach on this grid, and if Pe is large the mechanism is not working",
        by_flow as f64 / by_diffusion.max(1) as f64
    );
}

#[test]
fn the_transport_is_the_flow_and_not_the_numerical_diffusion_of_the_scheme() {
    // The control run, and it is not optional. ADR-054 rejected superbee with
    // exactly this argument: a scheme that manufactures the sought-after result
    // produces it whether or not the physics does. With a horizontally uniform
    // temperature the field is identically zero, so the advection kernel runs the
    // same number of times over the same tracer with every Courant number at zero
    // — and the answer has to be the initial state, bit for bit.
    let cfg = config(DOMAIN / 2.0, 0.0);
    let field = field(&cfg);
    let courant = face_courant_of(&field, &stratified(), 0, 42);
    for (at, value) in courant.iter().enumerate() {
        assert_eq!(*value, Q::ZERO, "face {at} of a horizontally uniform world");
    }

    let tracer = initial_tracer();
    let carried = advect(&tracer, &courant, transits(&field, &heated_bottom()));
    assert_eq!(
        carried, tracer,
        "the advection scheme moved matter with no flow at all: the ratio of \
         heated_bottom_produces_net_vertical_transport would then be measuring \
         numerical diffusion"
    );
    assert_eq!(mass_above_midplane(&carried), mass_above_midplane(&tracer));
}

#[test]
fn horizontally_uniform_temperature_produces_no_velocity() {
    // Exact equality, in all three components. Non-vacuous only beside
    // `heated_bottom_produces_net_vertical_transport`: a kernel returning zeros
    // passes this one and fails that one.
    //
    // The stratification is deliberate. What is asserted is not only "a uniform
    // world is still" but "a temperature varying along `z` alone drives nothing" —
    // that is, that no vertical difference leaked into the potential.
    let cfg = config(DOMAIN / 2.0, 0.0);
    let field = field(&cfg);
    let velocity = velocity_of(&field, &stratified(), 0, 42);
    for (at, value) in velocity.iter().enumerate() {
        assert_eq!(*value, Q::ZERO, "velocity component cell {at}");
    }

    // And the heated bottom does drive something, or the assertion above holds
    // over a kernel that returns zeros.
    let hot = velocity_of(&field, &heated_bottom(), 0, 42);
    assert!(hot.iter().any(|v| *v != Q::ZERO));
}

#[test]
fn the_noise_kernel_is_not_dispatched_at_zero_stir_fraction() {
    // The branch is on the host (ADR-069): stirring costs 120 draws per velocity
    // cell, `3.1e7` hashes a tick, twice a full seven-point pass over 128^3. The
    // error is invisible in the result and visible only in the price, so what is
    // asserted is that the buffer the noise kernel writes is **untouched** — a
    // sentinel survives the call — and that the velocity is the curl of the
    // interpolated potential bit for bit.
    let cfg = config(DOMAIN / 2.0, 0.0);
    let field = field(&cfg);
    assert!(!field.stirs());

    let sentinel = Q::from_f64(-12345.0);
    let mut coarse_potential = vec![Q::ZERO; (3 * N_COARSE) as usize];
    let mut potential = vec![Q::ZERO; (3 * VN * VN * VN) as usize];
    let mut stirred = vec![sentinel; potential.len()];
    let mut velocity = vec![Q::ZERO; potential.len()];
    let mut face_courant = vec![Q::ZERO; (3 * N_VOXELS) as usize];

    field.apply(
        &heated_bottom(),
        &unit_capacity(),
        &mut coarse_potential,
        &mut potential,
        &mut stirred,
        &mut velocity,
        &mut face_courant,
        0,
        run_key(42),
    );

    assert!(
        stirred.iter().all(|v| *v == sentinel),
        "the noise kernel was dispatched at stir_fraction = 0"
    );
    assert!(velocity.iter().any(|v| *v != Q::ZERO));
}

#[test]
fn the_floor_carries_no_vertical_velocity_with_stirring_on_and_off() {
    // The trap ADR-069's floor rule sets, and the only test that can see it. The
    // tangential potential is zeroed on the plane `z = 0` in **two** kernels; done
    // in only one, the floor holds at `stir_fraction = 0` and leaks the instant
    // stirring is on — with the divergence still an exact zero, the ledger still
    // closed, and matter simply leaving through the floor and returning through
    // the periodic lid.
    let cells = (VN * VN * VN) as usize;
    for stir_fraction in [0.0, 0.5] {
        let cfg = config(DOMAIN / 2.0, stir_fraction);
        let field = field(&cfg);
        let velocity = velocity_of(&field, &heated_bottom(), 3, 42);

        for y in 0..VN {
            for x in 0..VN {
                let at = (x + y * VN) as usize;
                assert_eq!(
                    velocity[2 * cells + at],
                    Q::ZERO,
                    "w at ({x}, {y}, 0) with stir_fraction = {stir_fraction}"
                );
            }
        }
        // And the field is not zero everywhere, or the wall is held by an empty
        // buffer.
        assert!(velocity.iter().any(|v| *v != Q::ZERO));
    }
}

#[test]
fn the_speed_bound_is_the_one_the_validator_checked() {
    // Two assertions, and the second is the more important one.
    //
    // First: on the limiting input — a source at exactly the declared span — no
    // component of `u` is over `speed_bound()`, which is the number the validator
    // compared against `dx/(6*dt)`.
    //
    // Second: `stencil_l1_norm()` is the norm of the composition the three kernels
    // actually implement, measured here by their impulse response. The closed form
    // of ADR-069 is written for a composition **without** the interpolation stage;
    // taken from the formula, `L` comes out wrong, the field passes the ceiling,
    // and nothing reports it — the validator checked `u_conv_max`, the debug
    // assertion is required never to fire and is absent in release, and the amounts
    // go negative and read as the accepted undershoot of ADR-068.
    let cfg = config(DOMAIN / 2.0, 0.0);
    let field = field(&cfg);
    let velocity = velocity_of(&field, &heated_bottom(), 0, 42);
    for (at, value) in velocity.iter().enumerate() {
        assert!(
            value.debug_f64().abs() <= field.speed_bound() * (1.0 + 1e-6),
            "component cell {at} is {} m/s over a bound of {}",
            value.debug_f64(),
            field.speed_bound()
        );
    }

    // The impulse response, measured through the kernels. By linearity the
    // coefficient of coarse cell `j` in the velocity at a fixed output cell is the
    // velocity there when the temperature is a delta at `j`; the `l1` norm is the
    // sum of their magnitudes, worst over the output cell and the component.
    let cells = (VN * VN * VN) as usize;
    let mut measured = vec![0.0f64; 3 * cells];
    for j in 0..N_COARSE {
        let mut enthalpy = vec![M64::ZERO; N_COARSE as usize];
        enthalpy[j as usize] = M64::new(1);
        let response = velocity_of(&field, &enthalpy, 0, 42);
        for at in 0..3 * cells {
            measured[at] += response[at].debug_f64().abs();
        }
    }
    // `L` is folded into the kernel, and `stencil_l1_norm` has it factored out.
    let worst = measured.iter().fold(0.0f64, |m, v| m.max(*v)) / field.mobility();
    assert!(
        (worst / field.stencil_l1_norm() - 1.0).abs() < 1e-3,
        "the measured l1 norm of the composed stencil is {worst} against the \
         derived {}: the estimate of |u| and the kernels disagree",
        field.stencil_l1_norm()
    );
}
