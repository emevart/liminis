//! Acceptance criteria of the first kernel: diffusion (`ACCEPTANCE.md`, S0).
//!
//! Three names, and the document fixes them — the stage is accepted by these
//! and not by a reading of the code:
//!
//! ```text
//! flux_is_antisymmetric
//! diffusion_alone_conserves_exactly
//! diffusion_matches_analytic_gaussian_spread
//! ```
//!
//! They live in an integration test rather than beside the kernel on purpose.
//! An acceptance test is the outside view: it may use only what a scenario can
//! use — the public surface of `liminis-core` — so that it keeps its meaning
//! when the inside is rearranged, and so that it cannot be quietly shaped
//! around a private helper it was supposed to be judging.
//!
//! The unit tests under `src/` overlap two of these names. That is not
//! duplication to be cleaned up: those check the kernel from the inside, with
//! hand-picked pairs and one grid, while these check the property that the
//! criterion actually names — over random pairs, over a whole tick's worth of
//! substeps, and against the analytic solution. Either one can fail while the
//! other passes.

use liminis_core::kernels::diffuse::{
    DiffuseParams, diffuse_voxel_32, diffuse_voxel_64, flux_32, flux_64,
};
use liminis_core::numeric::{M32, M64, Q};
use liminis_core::process::{Diffuse, DiffusePhase};
use liminis_core::world::{Boundary, Field, Field32, Field64, Grid};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

/// The eco regime of SPEC section 1.7: a one-second tick and a 100 um voxel.
const DT: f64 = 1.0;
const DX: f64 = 1.0e-4;

/// H+, the fastest thing in the registry (SPEC section 1.7). Six substeps per
/// tick at this `dx` because of the Grotthuss mechanism, which is what makes it
/// the worst case here: six chances a tick for a rounding rule to lose a unit.
const D_PROTON: f64 = 9.3e-9;

/// A grid with every face periodic. Conservation is asserted against a single
/// number for the whole domain, so nothing may leave it — and `exchange`, the
/// one boundary that would let something leave through a counted channel,
/// cannot be built until `ledger/` exists (`world::Grid::new`).
fn torus(nx: u32, ny: u32, nz: u32) -> Grid {
    Grid::new(nx, ny, nz, [Boundary::Periodic; 6]).unwrap()
}

fn total_32(field: &Field32) -> i64 {
    field.read().iter().map(|v| v.to_i64()).sum()
}

fn total_64(field: &Field64) -> i64 {
    field.read().iter().map(|v| v.to_i64()).sum()
}

/// Put a state into a field: write state `N+1` in full, then promote it.
///
/// In full, because a swap promotes whatever the write buffer holds — a voxel
/// this closure skipped would not be left alone, it would be left stale
/// (`world::Field`).
fn seed_32(field: &mut Field32, grid: &Grid, amount: impl Fn(u32, u32, u32) -> i32) {
    for idx in 0..field.n_voxels() {
        let (x, y, z) = grid.coords(idx);
        field.write_mut()[idx as usize] = M32::new(amount(x, y, z));
    }
    field.swap();
}

fn seed_64(field: &mut Field64, grid: &Grid, amount: impl Fn(u32, u32, u32) -> i64) {
    for idx in 0..field.n_voxels() {
        let (x, y, z) = grid.coords(idx);
        field.write_mut()[idx as usize] = M64::new(amount(x, y, z));
    }
    field.swap();
}

/// One substep over the whole lane — the inner loop of `process::Diffuse`,
/// reproduced here so that the total can be checked *between* substeps rather
/// than between ticks.
fn substep_32(field: &mut Field32, p: &DiffuseParams) {
    let n_voxels = field.n_voxels();
    let (src, dst) = field.lane_pair_mut(0);
    for idx in 0..n_voxels {
        diffuse_voxel_32(src, dst, p, idx);
    }
    field.swap();
}

fn substep_64(field: &mut Field64, p: &DiffuseParams) {
    let n_voxels = field.n_voxels();
    let (src, dst) = field.lane_pair_mut(0);
    for idx in 0..n_voxels {
        diffuse_voxel_64(src, dst, p, idx);
    }
    field.swap();
}

// ---------------------------------------------------------------------------
// flux_is_antisymmetric
// ---------------------------------------------------------------------------

/// Amounts for the narrow width, across three magnitudes.
///
/// One uniform band would not do, because the two ends of the range fail
/// differently and neither is rare in a real field.
///
/// **Small.** This is the only band where `alpha * (there - here)` can land on
/// an exact half at all: above 2^24 an `f32` has no bit left to put a half in,
/// so a run drawn only from large amounts would never once ask the rounding
/// rule the question this test exists to ask.
///
/// **Middle and large.** Where the `f32` runs out of mantissa and the flux
/// comes back coarse — a hundred units at a time. Coarse is not the problem;
/// asymmetric would be, and this is where an implementation that lost the sign
/// symmetry somewhere in the conversion would show it.
///
/// The two amounts are drawn independently, so mixed pairs come up too: a full
/// voxel beside a nearly empty one is both the interesting case and the common
/// one at the edge of a plume.
///
/// The bound is 1e9 rather than `i32::MAX` because the kernel computes
/// `there - here` in the storage width, and a pair straddling the whole range
/// overflows that subtraction — a fact about `M32`, not about the flux.
fn amount_32() -> impl Strategy<Value = i32> {
    prop_oneof![
        2 => -32i32..=32i32,
        1 => -100_000i32..=100_000i32,
        1 => -1_000_000_000i32..=1_000_000_000i32,
    ]
}

/// The same three bands for the width water is stored in (ADR-040), with the
/// large one at the scale that made that decision necessary.
fn amount_64() -> impl Strategy<Value = i64> {
    prop_oneof![
        2 => -32i64..=32i64,
        1 => -100_000i64..=100_000i64,
        1 => -5_100_000_000_000i64..=5_100_000_000_000i64,
    ]
}

/// Coefficients, weighted toward the ones that can expose a rounding rule.
///
/// A uniform draw almost never puts `alpha * (there - here)` on an exact half,
/// and the exact half is the whole question: `round(1.5) = 2` and
/// `round(-1.5) = -2` are symmetric, `floor(1.5) = 1` and `floor(-1.5) = -2` are
/// not. Negative powers of two land on halves whenever the difference has the
/// matching parity, so three draws in four come from that family; the fourth is
/// uniform over `[0, 1]`, which is where anything else would hide.
///
/// The range goes to one rather than stopping at the stability limit of a
/// sixth. Antisymmetry is a property of the function at every argument, and the
/// kernel is not the place where stability is enforced anyway — the host is
/// (`process::Diffuse`).
fn alpha() -> impl Strategy<Value = f64> {
    prop_oneof![
        3 => (1i32..=6i32).prop_map(|k| 0.5f64.powi(k)),
        1 => 0.0f64..=1.0f64,
    ]
}

proptest! {
    // Four thousand pairs a run, and a named place to keep a counterexample.
    // `proptest` writes a failing case into that file and replays it first on
    // the next run, which is what separates "the pairs are random" from "the
    // failure cannot be reproduced". The path has to be given: the default is
    // derived from the source file and there is nothing to derive it from in an
    // integration test, so proptest says so on stderr and keeps nothing.
    #![proptest_config(ProptestConfig {
        cases: 4096,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/acceptance_diffusion.proptest-regressions",
        ))),
        ..ProptestConfig::default()
    })]

    /// `ACCEPTANCE.md` calls this the most important test in the whole list and
    /// the cheapest, and names the tool: random pairs, through `proptest`.
    ///
    /// Conservation in gather form is not arithmetic that happens to come out
    /// even. Voxel `i` adds `flux(here, there)` to itself and voxel `j` adds
    /// `flux(there, here)` to itself in the same substep, off the same state
    /// `N`; the sum over the domain is identically zero **if and only if** the
    /// two are exact negations of each other, at every pair of amounts and
    /// every coefficient (ADR-034).
    ///
    /// Nothing else in the project can see this. A flux written with `floor`
    /// compiles, satisfies the type system, passes `kernel-lint`, reads
    /// correctly, and leaks one unit per face per substep.
    #[test]
    fn flux_is_antisymmetric(
        a in amount_32(),
        b in amount_32(),
        wide_a in amount_64(),
        wide_b in amount_64(),
        alpha in alpha(),
    ) {
        let alpha = Q::from_f64(alpha);

        let narrow = flux_32(M32::new(a), M32::new(b), alpha);
        let narrow_back = flux_32(M32::new(b), M32::new(a), alpha);
        prop_assert_eq!(
            narrow, -narrow_back,
            "flux_32({}, {}) is not the negation of flux_32({}, {})", a, b, b, a
        );

        let wide = flux_64(M64::new(wide_a), M64::new(wide_b), alpha);
        let wide_back = flux_64(M64::new(wide_b), M64::new(wide_a), alpha);
        prop_assert_eq!(
            wide, -wide_back,
            "flux_64({}, {}) is not the negation of flux_64({}, {})",
            wide_a, wide_b, wide_b, wide_a
        );

        // The corollary a closed face rests on, and it is not a separate
        // property: an antisymmetric function of two equal arguments is its own
        // negation, so it is zero. `world::Grid::neighbour` returns the voxel
        // itself across a closed face precisely because of this, so if it ever
        // stops holding, sealed boundaries start leaking too.
        prop_assert_eq!(flux_32(M32::new(a), M32::new(a), alpha), M32::ZERO);
        prop_assert_eq!(flux_64(M64::new(wide_a), M64::new(wide_a), alpha), M64::ZERO);
    }
}

// ---------------------------------------------------------------------------
// diffusion_alone_conserves_exactly
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md`, section "Conservation". Exactly: the sum over the domain
/// before and after is the same integer, not the same number to within a
/// tolerance (ADR-003, ADR-005).
///
/// Two runs, and they fail differently.
///
/// **One, the substep.** The initial state is the checkerboard, whose
/// eigenvalue under the 7-point stencil is `1 - 12*alpha`: at the stability
/// limit that is exactly `-1`, so the mode flips sign every substep and does not
/// decay. It is the one state that keeps every face of every voxel carrying flux
/// for a whole long run — a diffusing blob goes flat within a few hundred
/// substeps, and a test that kept asserting after that would be asserting that
/// nothing had moved. The total is checked after **every substep**, so a leak of
/// one unit per face is caught where it happens rather than as a number that no
/// longer adds up two thousand substeps later.
///
/// **Two, the tick.** The same statement one level up, through the process that
/// owns the substep count, on both storage widths, over hundreds of ticks. This
/// is the one that would notice if the substep loop dropped or repeated a
/// buffer swap: the kernel would still be conserving and the tick would not.
#[test]
fn diffusion_alone_conserves_exactly() {
    // --- One: the substep, at the stability limit. -------------------------
    const L: u32 = 6; // even, or the checkerboard does not close on the torus
    const SUBSTEPS: u32 = 2_000;

    let grid = torus(L, L, L);
    // 6*D*dt/dx^2 is 5.994 at this diffusivity, so the tick divides into six
    // substeps and `alpha` lands one part in a thousand under the stability
    // limit. Not *on* it: `n = ceil(6*D*dt/dx^2)` puts alpha exactly on the
    // limit only when that ratio is an integer, and asking a decimal
    // diffusivity for an integer ratio asks which side of it the last bit of a
    // f64 falls on — `D = 1.0e-8` gives seven substeps, not six, because
    // `1.0e-4 * 1.0e-4` is a hair above `1.0e-8`.
    //
    // One part in a thousand under the limit is what the checkerboard mode
    // wants anyway: its eigenvalue is `1 - 12*alpha = -0.998`, so it flips sign
    // every substep and comes out of two thousand of them with a fiftieth of the
    // amplitude it started with — decay slow enough that every face is still
    // carrying flux at the end. The assertion after the loop is what checks
    // that, rather than this arithmetic.
    //
    // The parameters come from the process rather than from a hand-written
    // struct: a test that folds its own `alpha` is testing a kernel nobody runs.
    let at_the_limit = Diffuse::new(&grid, 9.99e-9, DT, DX).unwrap();
    assert_eq!(at_the_limit.substeps(), 6);
    assert!(at_the_limit.params().alpha <= Q::from_f64(1.0 / 6.0));
    assert!(at_the_limit.params().alpha > Q::from_f64(0.166));
    let p = at_the_limit.params();

    for (label, background, amplitude) in [
        // Amounts above 2^24, where an f32 has no mantissa left for the low
        // bits of the flux and the rounding is coarse.
        ("large", 1_000_000_000i32, 900_000_001i32),
        // And amounts where `alpha * (there - here)` has a fractional part
        // every time. This one cycles between amplitudes 7 and -5 forever, so
        // the rounding never gets to go quiet.
        ("small", 1_000i32, 7i32),
    ] {
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_32(&mut field, &grid, |x, y, z| {
            if (x + y + z) % 2 == 0 {
                background + amplitude
            } else {
                background - amplitude
            }
        });

        let total = total_32(&field);
        assert_eq!(total, i64::from(background) * i64::from(grid.n_voxels()));

        for substep in 0..SUBSTEPS {
            substep_32(&mut field, &p);
            assert_eq!(
                total_32(&field),
                total,
                "{label}: substep {substep} moved the total"
            );
        }

        // And the run was not asserting about a field that had stopped moving:
        // one more substep still changes it.
        let settled = field.read().to_vec();
        substep_32(&mut field, &p);
        assert_ne!(
            field.read(),
            settled.as_slice(),
            "{label}: the field went quiet, so the substeps above proved nothing"
        );
        assert_eq!(total_32(&field), total);
    }

    // The same mode in the width water is stored in (ADR-040), at an amplitude
    // no i32 could hold. Both widths come out of one macro, so this is not a
    // second implementation — but the macro is exactly the place where an
    // instantiation could go wrong for one width only, and nothing above would
    // have seen it.
    let mut wide_mode: Field64 = Field::new(&grid, 1).unwrap();
    seed_64(&mut wide_mode, &grid, |x, y, z| {
        let amplitude = 5_100_000_000_000i64;
        if (x + y + z) % 2 == 0 {
            10_000_000_000_000 + amplitude
        } else {
            10_000_000_000_000 - amplitude
        }
    });
    let wide_mode_total = total_64(&wide_mode);
    for substep in 0..SUBSTEPS {
        substep_64(&mut wide_mode, &p);
        assert_eq!(
            total_64(&wide_mode),
            wide_mode_total,
            "i64: substep {substep} moved the total"
        );
    }

    // --- Two: whole ticks, through the process. ----------------------------
    const TICKS: u32 = 300;

    let grid = torus(12, 10, 8);
    let diffuse = Diffuse::new(&grid, D_PROTON, DT, DX).unwrap();
    assert_eq!(diffuse.substeps(), 6);
    let phase = DiffusePhase::new_32(&grid, 1, &[D_PROTON], DT, DX).unwrap();
    let wide_phase = DiffusePhase::new_64(&grid, 1, &[D_PROTON], DT, DX).unwrap();

    let mut narrow: Field32 = Field::new(&grid, 1).unwrap();
    seed_32(&mut narrow, &grid, |x, y, z| {
        if (x, y, z) == (6, 5, 4) {
            // A pool near the i32 ceiling, so the flux across each face is
            // large enough that the f32 it passes through has no spare bits.
            2_000_000_000
        } else if (x + y + z) % 5 == 0 {
            // Four units against an empty neighbourhood is the documented
            // overdraw at this alpha: each of the six faces rounds `0.62` up to
            // one and takes six units out of a pool of four. The voxel goes to
            // -2, and the total must not notice, because the neighbours have
            // them. A kernel that clamped at zero here would look kinder and
            // would be creating matter.
            4
        } else {
            0
        }
    });

    let mut wide: Field64 = Field::new(&grid, 1).unwrap();
    seed_64(&mut wide, &grid, |x, y, z| {
        // Water, at the scale that made ADR-040 necessary, with a gradient of a
        // few hundred units on top of it. The small difference on the large pool
        // is the case that separates `q_conc(there - here)` from
        // `q_conc(there) - q_conc(here)`: at 5.1e12 an f32 step is
        // `2^19 = 524 288` units, so the second form would round the whole
        // gradient away and this field would sit there looking perfectly
        // conserved and perfectly still.
        5_100_000_000_000 + i64::from(x * 100 + y * 10 + z)
    });

    let narrow_total = total_32(&narrow);
    let wide_total = total_64(&wide);
    let wide_spread = |field: &Field64| {
        let amounts = field.read().iter().map(|v| v.to_i64());
        amounts.clone().max().unwrap() - amounts.min().unwrap()
    };
    let wide_spread_before = wide_spread(&wide);

    for tick in 0..TICKS {
        phase.apply_32(&mut narrow);
        assert_eq!(total_32(&narrow), narrow_total, "i32: tick {tick}");

        wide_phase.apply_64(&mut wide);
        assert_eq!(total_64(&wide), wide_total, "i64: tick {tick}");
    }

    // And both runs did something. The narrow pool spread out of its voxel and
    // reached every corner of the domain; the wide gradient flattened, which it
    // could not have done if the differences had been rounded away.
    assert!(narrow.read()[grid.index(6, 5, 4) as usize].to_i64() < 2_000_000_000);
    assert!(narrow.read().iter().all(|&v| v != M32::ZERO));
    assert!(wide_spread(&wide) < wide_spread_before / 10);
}

// ---------------------------------------------------------------------------
// diffusion_matches_analytic_gaussian_spread
// ---------------------------------------------------------------------------

/// The analytic solution, sampled at the centre of one voxel.
///
/// A point source of `total` in an infinite three-dimensional medium is
/// `total * (4*pi*D*t)^(-3/2) * exp(-r^2/(4*D*t))`: a Gaussian of variance
/// `2*D*t` on each axis. Summed over the other two axes, which integrate to
/// one, the marginal along an axis is the one-dimensional Gaussian of the same
/// variance, and one slab of voxels holds that density times `dx`.
///
/// **Sampled at the centre of the voxel, not integrated over it**, and that is
/// not a shortcut — the integral is the wrong reference here. Grouping a
/// Gaussian into cells of width `dx` inflates its variance by `dx^2/12`, which
/// is Sheppard's correction, and at `dx = 0.37*sigma` that is a relative 1.1e-2:
/// an order of magnitude larger than everything else this comparison is trying
/// to see, and shaped exactly like a wrong diffusivity. The discrete solution's
/// own variance is `2*D*t` exactly — that is the identity the tolerance note
/// below rests on — so the analytic profile it has to be held against is the one
/// whose *samples* carry that variance. Sampling adds nothing to it: by Poisson
/// summation the correction is of order `exp(-2*pi^2*sigma^2/dx^2)`, which is
/// e-147 at this resolution.
fn gaussian_marginal(total: f64, x: f64, sigma: f64, dx: f64) -> f64 {
    total * dx * (-0.5 * (x / sigma).powi(2)).exp() / (sigma * std::f64::consts::TAU.sqrt())
}

/// The fourth probabilists' Hermite polynomial, the shape of the leading
/// correction to a Gaussian for a distribution with excess kurtosis.
fn hermite_4(u: f64) -> f64 {
    u.powi(4) - 6.0 * u * u + 3.0
}

/// `ACCEPTANCE.md`, section "Numerical verification".
///
/// A point source in a three-dimensional medium spreads into a Gaussian of
/// variance `2*D*t` on each axis. Checked against the analytic solution, with a
/// tolerance that is derived rather than fitted — a fitted tolerance measures
/// whatever the code currently does.
///
/// # Where the tolerance comes from
///
/// Two of the three error sources are exactly zero here, and that is what makes
/// a derived tolerance possible at all.
///
/// **Discretisation: none, in this quantity.** Multiply one substep by `x^2`
/// and sum over the domain. Every voxel `j` appears once as a neighbour of each
/// of its six neighbours, whose `x^2` sum to `6*x_j^2 + 2*dx^2`, so
///
/// ```text
/// M2' - M2 = alpha * (sum_j n_j * (6*x_j^2 + 2*dx^2) - 6*M2) = 2*alpha*dx^2*S
/// ```
///
/// exactly, at every state and every substep. After `n` substeps the variance
/// is `2*alpha*n*dx^2`, and `alpha = D*dt_sub/dx^2` with `n*dt_sub = t`, so it
/// is `2*D*t` — the analytic answer, with no truncation term to bound. The
/// second moment is the one quantity of this scheme that is not approximate,
/// which is why the acceptance criterion is stated on it.
///
/// **Wrap-around: none, and asserted rather than estimated.** The domain is a
/// torus, and mass that has been round it is counted at the wrong distance. The
/// only faces that cross the wrap belong to voxels in the outermost shell, and
/// the flux across a face between two empty voxels is zero, so it is enough
/// that the shell is empty at the start of every substep. That is checked, not
/// estimated — with integer amounts the far tail is not merely small, it is
/// identically zero, because a transfer below half a unit rounds to nothing.
///
/// **Rounding: the only term left.** Write the integer transfer across a face
/// as the exact one plus an error `e`. Two things bound `e`: the rounding
/// itself, `|e| <= 1/2` per face, and the `f32` the coefficient passes through,
/// which contributes a *relative* `2^-23`. A transfer across an x-face changes
/// the second moment by `e * (x_i^2 - x_j^2) = e * dx * (x_i + x_j)`, at most
/// `e * 2*R*dx` because nothing leaves the box the shell assertion pins. There
/// is one x-face per voxel, so per substep the second moment can be knocked off
/// by at most `V * (1/2) * 2*R*dx` from the rounding, and by
/// `2^-23 * 2*alpha*S * 2*R*dx` from the `f32` — the second uses
/// `sum|n_j - n_i| <= 2*S` over the faces of one axis, which holds for any
/// non-negative state. Over `n` substeps, against `M2 = 2*alpha*n*dx^2*S`, `n`
/// cancels out of both and what is left is a relative bound in run parameters
/// alone, with `R` in voxels:
///
/// ```text
/// rounding:  V*R / (2*alpha*S)
/// f32:       2^-23 * 2*R
/// ```
///
/// Both are worst cases in which every face errs in the same direction at once;
/// the real error comes out three orders of magnitude smaller, and that is the
/// right way round for a criterion. The last thing `alpha` could hide is itself:
/// the kernel holds it as an `f32`, so the variance it produces is the analytic
/// one to a relative `2^-24`, which is inside the second term.
///
/// # And the shape, not only the width
///
/// A distribution with the right variance need not be a Gaussian, so the
/// marginal profile is compared to the analytic one bin by bin. The marginal is
/// the sharper comparison and not merely the cheaper one: summing the update
/// over `y` and `z` kills the `y` and `z` faces exactly, so the marginal obeys
/// the one-dimensional scheme — a lazy random walk with step probability
/// `alpha` — while the three-dimensional profile carries cross terms of
/// relative order `1/n` that have nothing to do with the Gaussian.
///
/// A lattice walk of twenty-four steps is not exactly a Gaussian either, and it
/// misses by a knowable amount rather than by a negligible one: its excess
/// kurtosis is exact and derived where the tolerance is built, and the Edgeworth
/// expansion turns it into a relative deviation of `gamma_2 * He_4(u) / 24` —
/// two parts in a thousand at the edge of the window compared here, and past a
/// percent just outside it. Which is why the reference profile has to be right
/// to a part in ten thousand before any of this can be read at all, and why the
/// note on [`gaussian_marginal`] is part of the derivation rather than a
/// footnote to it.
#[test]
fn diffusion_matches_analytic_gaussian_spread() {
    /// Half the extent of the grid: the source sits at the centre and the
    /// furthest voxel is `R` away on each axis.
    const R: u32 = 21;
    const N: u32 = 2 * R + 1;
    const TICKS: u32 = 4;
    /// 2^38 units. Large enough that the rounding term of the tolerance is
    /// small, and small enough that the tail is exactly zero well before the
    /// wrap — the two ends of the same trade, since both scale with the source.
    const SOURCE: i64 = 1 << 38;

    let grid = torus(N, N, N);
    let diffuse = Diffuse::new(&grid, D_PROTON, DT, DX).unwrap();
    let substeps = diffuse.substeps();
    assert_eq!(substeps, 6);
    let p = diffuse.params();
    let alpha = p.alpha.debug_f64();

    let mut field: Field64 = Field::new(&grid, 1).unwrap();
    seed_64(&mut field, &grid, |x, y, z| {
        if (x, y, z) == (R, R, R) { SOURCE } else { 0 }
    });
    assert_eq!(total_64(&field), SOURCE);

    // The outermost shell: every voxel that owns a face across the wrap.
    let radius = |idx: u32| {
        let (x, y, z) = grid.coords(idx);
        x.abs_diff(R).max(y.abs_diff(R)).max(z.abs_diff(R))
    };
    let shell: Vec<u32> = (0..grid.n_voxels())
        .filter(|&idx| radius(idx) == R)
        .collect();
    assert_eq!(
        shell.len(),
        (N * N * N - (N - 2) * (N - 2) * (N - 2)) as usize
    );

    // Substep by substep rather than tick by tick, and the reason is the check
    // inside the loop: the flux across a wrap face is `flux(a, b)` between the
    // two ends of the shell, so if the whole shell is empty at the start of a
    // substep, that substep moves nothing across the wrap. Checking after every
    // substep is what makes "empty shell" enough; checking once a tick would
    // need a guard band six voxels deep, and the tail is not empty that far in.
    //
    // The loop is `process::Diffuse::apply_64` with an assertion between the
    // substeps — same buffers, same order, same parameters, which are the
    // process's own and not this test's.
    let assert_shell_is_empty = |field: &Field64, step: u32| {
        for &idx in &shell {
            assert_eq!(
                field.read()[idx as usize],
                M64::ZERO,
                "substep {step}: voxel {idx} sits on the wrap and is not empty, \
                 so the profile is about to come round the torus and the moments \
                 below would be measuring the box rather than the medium"
            );
        }
    };

    for step in 0..TICKS * substeps {
        assert_shell_is_empty(&field, step);
        substep_64(&mut field, &p);
        assert_eq!(total_64(&field), SOURCE, "substep {step} moved the total");
    }
    assert_shell_is_empty(&field, TICKS * substeps);

    // Moments, in voxels and in exact integers: nothing has to be rounded to
    // measure a distribution of integers over a lattice.
    let mut first = [0i64; 3];
    let mut second = [0i64; 3];
    let mut marginal = [[0i64; N as usize]; 3];
    for idx in 0..grid.n_voxels() {
        let amount = field.read()[idx as usize].to_i64();
        if amount == 0 {
            continue;
        }
        let (x, y, z) = grid.coords(idx);
        for (axis, coord) in [x, y, z].into_iter().enumerate() {
            let d = i64::from(coord) - i64::from(R);
            first[axis] += d * amount;
            second[axis] += d * d * amount;
            marginal[axis][coord as usize] += amount;
        }
    }

    let t = f64::from(TICKS) * DT;
    // The analytic solution, in the form the criterion names it: variance
    // 2*D*t on each axis.
    let variance = 2.0 * D_PROTON * t;
    let sigma = variance.sqrt();
    let n_voxels = f64::from(grid.n_voxels());
    let source = SOURCE as f64;

    let rounding = n_voxels * f64::from(R) / (2.0 * alpha * source);
    let f32_relative = 2.0f64.powi(-23) * 2.0 * f64::from(R);
    let tolerance = rounding + f32_relative;

    for axis in 0..3 {
        // The scheme and the rounding rule are both symmetric under reflection
        // about the source, and so is the initial state, so the numerical
        // solution is symmetric voxel for voxel and its first moment is
        // *exactly* zero. Nothing else in this file would notice a rounding
        // rule that treated the two directions differently while still being
        // odd — and such a rule would show up in the field as a drift.
        assert_eq!(first[axis], 0, "axis {axis}: the profile is off centre");

        let measured = (second[axis] as f64 / source) * DX * DX;
        let deviation = (measured / variance - 1.0).abs();
        assert!(
            deviation <= tolerance,
            "axis {axis}: variance {measured:e} m^2 against the analytic \
             2*D*t = {variance:e} m^2 is off by {deviation:e}, over the derived \
             tolerance {tolerance:e} (rounding {rounding:e}, f32 {f32_relative:e})"
        );
    }

    // The three axes are the same computation on a cubic grid, so they are not
    // merely close: they are equal, integer for integer.
    assert_eq!(marginal[0], marginal[1]);
    assert_eq!(marginal[0], marginal[2]);

    // The shape, bin by bin, against the analytic profile.
    //
    // `gamma_2` is how far from Gaussian the scheme is, and it is not an
    // estimate: run the same moment argument as above one order up. With
    // `X' = X + step`, the step independent of the state and odd, the marginal's
    // fourth moment obeys `M4(k) = M4(k-1) + 12*alpha*M2(k-1) + 2*alpha`, and
    // `M2(k) = 2*alpha*k`, so
    //
    //     M4(n) = 12*alpha^2*n*(n-1) + 2*alpha*n
    //     M4(n) - 3*sigma^4 = 2*alpha*n*(1 - 6*alpha)
    //     gamma_2 = (1 - 6*alpha) / (2*alpha*n)
    //
    // in units of the voxel. That is the excess kurtosis of the profile, exactly,
    // at every n — it vanishes as the run lengthens, and it also vanishes at
    // alpha = 1/6, which is the sense in which the stability limit is the sharp
    // timestep rather than merely the largest one.
    let n_substeps = f64::from(TICKS * substeps);
    let gamma_2 = (1.0 - 6.0 * alpha) / (2.0 * alpha * n_substeps);
    // Half a unit per face on each of the two planes bounding a slab, over every
    // substep: the whole of what rounding can do to one bin of the marginal.
    let bin_rounding = n_substeps * f64::from(N * N);

    let window = (2.0 * sigma / DX) as i64;
    for offset in -window..=window {
        let bin = (i64::from(R) + offset) as usize;
        let measured = marginal[0][bin] as f64;
        let expected = gaussian_marginal(source, offset as f64 * DX, sigma, DX);
        let u = offset as f64 * DX / sigma;

        // The Edgeworth expansion turns an excess kurtosis into a deviation from
        // the Gaussian of `gamma_2 * He_4(u) / 24`, relative. After it come a
        // term in `gamma_2^2 * He_8` and one in the sixth cumulant, both an
        // order of magnitude smaller than this one across the window; the
        // factor of two is their allowance, and `He_4(0) = 3` is a floor,
        // because without it the bound would vanish at the two zeros of `He_4`
        // where the next order is all that is left.
        //
        // The measured deviations come out at about a third of the bound at
        // every bin. That is a consequence and not an input: every number in
        // the expression is either a run parameter or a coefficient of the
        // expansion.
        let edgeworth = gamma_2 * hermite_4(u).abs() / 24.0;
        let next_order = gamma_2 * 3.0 / 24.0;
        let tolerance = 2.0 * (edgeworth + next_order) + bin_rounding / expected;

        let deviation = (measured / expected - 1.0).abs();
        assert!(
            deviation <= tolerance,
            "marginal at {offset} voxels ({u:.2} sigma): {measured:e} against \
             the analytic {expected:e} is off by {deviation:e}, over the derived \
             tolerance {tolerance:e} (Edgeworth {edgeworth:e}, rounding {:e})",
            bin_rounding / expected
        );
    }
}
