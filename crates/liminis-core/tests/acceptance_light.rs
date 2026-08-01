//! Acceptance criterion of the light kernel (`ACCEPTANCE.md`, "Numerical
//! verification"):
//!
//! ```text
//! light_attenuation_matches_beer_lambert
//! ```
//!
//! Outside the crate rather than beside the kernel, for the reason
//! `acceptance_diffusion.rs` states: an acceptance test is the outside view. It
//! may use only what a scenario can use — the public surface of `liminis-core` —
//! so that it keeps its meaning when the inside is rearranged, and so that it
//! cannot be quietly shaped around a private helper it was supposed to judge.
//!
//! The unit tests under `src/kernels/light.rs` do not overlap this one. They pin
//! the convention of the field, the write set of an invocation and the two
//! storage widths; this pins the number.

use liminis_core::kernels::light::{Attenuators, LightParams, light_column};
use liminis_core::numeric::{M32, M64, Q};

/// A tall, deliberately non-square column stack. Sixty-four voxels of depth is
/// what makes the tolerance below mean anything: one multiplication is exact to
/// half an ulp and says nothing.
const NX: u32 = 3;
const NY: u32 = 2;
const NZ: u32 = 64;
const N_VOXELS: u32 = NX * NY * NZ;
const N_COLUMNS: u32 = NX * NY;

/// The voxel edge along Z, metres. Neither one nor the eco-regime `dx` of
/// `1e-4 m`: `dz` lives inside the folded coefficient and nowhere else, so a
/// host that forgot it produces exactly the same shape of profile with a
/// different total optical depth — which reads as "the coefficients are not
/// calibrated" rather than as a bug. Only comparing against the analytic profile
/// in **physical** depth `j*dz`, with `dz != 1`, can tell the two apart.
const DZ: f64 = 0.05;

/// Irradiance on the top face of the topmost voxel, W/m^2. Arbitrary on purpose:
/// no key in `CONFIG_SCHEMA.md` section 7 and no row in `QUANTITIES.md`
/// section 5 names the incident irradiance, so any number here would be invented
/// (see `LightParams::i_surface`). The kernel takes it folded, so the test may
/// use whatever it likes and the criterion is unaffected.
const INCIDENT: f64 = 137.0;

/// Two attenuators with **different** amounts, in two different storage widths.
///
/// Different on purpose. `lane[a]` carries the lane inside its width class and
/// not the substance index (ADR-056), and a kernel that resolved the lane wrong
/// would read the other attenuator's amounts. With equal amounts that swap is
/// invisible; with these it changes `k_total` and the profile misses.
///
/// Both are exactly representable in `f32` — `12e6 < 2^24`, and `2e9` is
/// `15_625_000 * 2^7` — so the tolerance below is about the recurrence and not
/// about the inputs.
const AMOUNT_32: i32 = 12_000_000;
const AMOUNT_64: i64 = 2_000_000_000;

/// The physical attenuation coefficients, 1/m (`QUANTITIES.md` section 5).
const K_32: f64 = 1.5;
const K_64: f64 = 0.8;

/// What one storage unit of each attenuator is worth as the dimensionless
/// measure `k` multiplies. Which measure that is has not been settled by any
/// record — `QUANTITIES.md` section 5 gives `k` in 1/m, which forces the other
/// factor to be dimensionless, and the candidate consistent with the corpus is a
/// volume fraction through the partial molar volumes of ADR-067. The kernel is
/// not blocked by the question, because the host folds the product; this test is
/// not blocked either, because it assembles `k_total` from the same two numbers
/// the kernel never sees.
const X_PER_UNIT_32: f64 = 4.0e-8;
const X_PER_UNIT_64: f64 = 1.75e-10;

/// One invocation per column, the way the host runs it.
fn dispatch(src32: &[M32], src64: &[M64], att: &Attenuators, p: &LightParams) -> Vec<Q> {
    let mut dst = vec![Q::ZERO; N_VOXELS as usize];
    for idx in 0..N_COLUMNS {
        light_column(src32, src64, &mut dst, att, p, idx);
    }
    dst
}

/// The made solution: a uniform column of two attenuators, one narrow and one
/// wide, and the analytic Beer-Lambert profile it has to reproduce.
///
/// `k_total` is assembled here out of the physical `k` and the concentrations,
/// independently of the folded `coeff` the kernel is handed. Deriving it from
/// `coeff` would make the test agree with whatever the folding did, including
/// dropping `dz`.
#[test]
fn light_attenuation_matches_beer_lambert() {
    // Lane indices deliberately not equal to the table entry (ADR-056): the
    // narrow buffer has two lanes and the attenuator sits in the second.
    const LANE_32: u32 = 1;
    const LANE_64: u32 = 0;
    const N_LANES_32: u32 = 2;
    const N_LANES_64: u32 = 1;

    let p = LightParams {
        nx: NX,
        ny: NY,
        nz: NZ,
        n_voxels: N_VOXELS,
        n_attenuators: 2,
        // Bit 1: table entry 1 reads the wide slice. The bit indexes the entry,
        // not the substance and not the voxel.
        width_mask: 0b10,
        i_surface: Q::from_f64(INCIDENT),
    };

    // What the host folds: `k * dz * (measure per storage unit)`, one number per
    // attenuator. The kernel sees neither `k` nor `dz` nor the scale.
    let lane = [LANE_32, LANE_64];
    let coeff = [
        Q::from_f64(K_32 * DZ * X_PER_UNIT_32),
        Q::from_f64(K_64 * DZ * X_PER_UNIT_64),
    ];
    let att = Attenuators {
        lane: &lane,
        coeff: &coeff,
    };

    let mut src32 = vec![M32::ZERO; (N_LANES_32 * N_VOXELS) as usize];
    let mut src64 = vec![M64::ZERO; (N_LANES_64 * N_VOXELS) as usize];
    for at in 0..N_VOXELS {
        src32[(LANE_32 * N_VOXELS + at) as usize] = M32::new(AMOUNT_32);
        src64[(LANE_64 * N_VOXELS + at) as usize] = M64::new(AMOUNT_64);
    }

    let dst = dispatch(&src32, &src64, &att, &p);

    // Assembled from the physics, not from the kernel's numbers: 1/m times a
    // dimensionless measure, summed over the attenuators. Comes to 1.0 1/m here,
    // so the whole 3.2 m column is 3.2 optical depths deep and the beam leaves
    // the floor at about four per cent — attenuation that is neither invisible
    // nor saturated.
    let k_total =
        K_32 * X_PER_UNIT_32 * f64::from(AMOUNT_32) + K_64 * X_PER_UNIT_64 * AMOUNT_64 as f64;

    // Derived rather than picked: `nz` multiplications in single precision, each
    // of them a rounded `qexp` times a rounded product, so the relative error
    // accumulates linearly in the depth. Eight ulps per voxel is the slack over
    // that estimate.
    let tolerance = 8.0 * f64::from(NZ) * f64::from(f32::EPSILON);

    for y in 0..NY {
        for x in 0..NX {
            for j in 0..NZ {
                // `dst[nz-1-j]` is the intensity **leaving** the bottom face of
                // the voxel `j` layers below the surface, so the analytic profile
                // is taken at the physical depth `(j+1)*dz` — and at `j = 0` it
                // is the incident irradiance already attenuated by one voxel, not
                // the incident irradiance itself. The offset is the convention of
                // the field (`kernels::light::light_column`), which is what makes
                // the absorption of every voxel — the floor one included —
                // recoverable from the stored field alone (ADR-049).
                let z = NZ - 1 - j;
                let at = (x + y * NX + z * NX * NY) as usize;
                let computed = dst[at].debug_f64();
                let analytic = INCIDENT * (-k_total * f64::from(j + 1) * DZ).exp();

                assert!(
                    (computed / analytic - 1.0).abs() <= tolerance,
                    "column ({x}, {y}), depth {j}: {computed} against {analytic}, \
                     relative error {} over a tolerance of {tolerance}",
                    (computed / analytic - 1.0).abs()
                );
            }
        }
    }

    // The profile actually decays over the column. Without this the assertion
    // above is satisfied by a kernel that attenuates nothing whenever `k_total`
    // is small enough for the tolerance to swallow it.
    let top = dst[(NZ - 1) as usize * (NX * NY) as usize].debug_f64();
    let bottom = dst[0].debug_f64();
    assert!(bottom < 0.05 * top, "{bottom} against {top}");
}
