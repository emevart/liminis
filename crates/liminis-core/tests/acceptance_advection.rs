//! Acceptance criteria of the second transport kernel: advection with a van
//! Leer limiter (`ACCEPTANCE.md`, S0).
//!
//! Four names, and the document fixes them — the stage is accepted by these and
//! not by a reading of the code:
//!
//! ```text
//! flux_is_antisymmetric
//! flux_limiter_falls_back_to_first_order_at_zero_gradient
//! advection_undershoot_stays_within_the_derived_bound
//! advection_of_a_step_produces_no_new_extrema
//! ```
//!
//! **The third name was `advection_never_produces_negative_amount` and is not
//! any more.** ADR-068 renamed it, and `ACCEPTANCE.md` already carries the new
//! name. The old one promised non-negativity, which no limiter can deliver in an
//! integer flux-form scheme: positivity is a property of exact arithmetic, every
//! face flux rounds to a whole storage unit, and van Leer does not change that
//! because TVD is a statement about the reals. What replaced the promise is a
//! bound — and a bound that depends on the pool, three units below `2^23`, six
//! below `2^24`, fifty-one at the ceiling ADR-039 allows — so this file computes
//! it from the buffer under test and never names a constant.
//!
//! Two more names of `ACCEPTANCE.md` are answered here, and they are not
//! advection's own:
//!
//! ```text
//! flux_is_the_two_named_crossings_composed
//! a_small_gradient_in_a_large_pool_still_carries_flux
//! ```
//!
//! ADR-060 reserved them for the shape of a transport flux — `q_conc` down, one
//! `q_round` back up, the differences taken in `M` before either — while the
//! only flux in the project was diffusive. Advection is the second flux built to
//! that shape and the first with a limiter between the terms, so the pair is
//! answered on it here. Nothing in this file claims them for advection alone: a
//! diffusive instance is still owed, in `tests/acceptance_diffusion.rs`.
//!
//! They live in an integration test rather than beside the kernel on purpose,
//! and `tests/acceptance_diffusion.rs` states the reason: an acceptance test is
//! the outside view, may use only what a scenario can use — the public surface
//! of `liminis-core` — and therefore keeps its meaning when the inside is
//! rearranged and cannot be quietly shaped around a private helper it was
//! supposed to be judging.
//!
//! The unit tests under `src/kernels/advect.rs` overlap some of this. That is
//! not duplication to be tidied away: those check the kernel from the inside,
//! with hand-picked stencils and one grid, including the neighbourhood lookup
//! which is private and has to be. These check the properties the criteria
//! actually name, over random arguments and over whole sweeps.

use liminis_core::kernels::advect::{
    AdvectParams, advect_voxel_32, advect_voxel_64, flux_32, flux_64, limited_slope_32,
    limited_slope_64,
};
use liminis_core::numeric::{
    M32, M64, Q, m_delta_64, q_conc_32, q_conc_64, q_round_32, q_round_64, qadd, qdiv, qmul, qsub,
};
use liminis_core::world::{Boundary, Face, Grid};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

/// The mask `AdvectParams` wants, read off a grid rather than written by hand.
///
/// Bit `f` is the discriminant of `world::Face`, and going through a `Grid`
/// means the boundary set has already been through `Grid::new` — which is what
/// refuses a half-periodic axis, an invariant `AdvectParams` cannot carry and
/// only asserts.
fn mask_of(grid: &Grid) -> u32 {
    let mut mask = 0u32;
    for face in Face::ALL {
        if grid.boundary(face) == Boundary::Periodic {
            mask |= 1 << (face as u32);
        }
    }
    mask
}

/// A strip of `n` voxels along X, with whatever the boundary set says.
fn strip(n: u32, boundary: [Boundary; 6]) -> (Grid, AdvectParams) {
    let grid = Grid::new(n, 1, 1, boundary).unwrap();
    let p = AdvectParams {
        exchange_mask: 0,
        nx: n,
        ny: 1,
        nz: 1,
        periodic_mask: mask_of(&grid),
        axis: 0,
    };
    (grid, p)
}

const PERIODIC: [Boundary; 6] = [Boundary::Periodic; 6];
const CLOSED: [Boundary; 6] = [Boundary::Closed; 6];
/// Periodic in X and Y, solid floor and lid in Z — the eco-regime default of
/// SPEC section 1.6, minus the face `Grid::new` refuses.
const FLOORED: [Boundary; 6] = [
    Boundary::Periodic,
    Boundary::Periodic,
    Boundary::Periodic,
    Boundary::Periodic,
    Boundary::Closed,
    Boundary::Closed,
];

/// The velocity budget of ADR-069: `dx/(6*dt) = 16.7 um/s`, a Courant number of
/// 0.167 at `dx = 100 um` and `dt = 1 s`.
const BUDGET: f64 = 0.167;

fn sweep_32(src: &[M32], courant: &[Q], p: &AdvectParams) -> Vec<M32> {
    let mut dst = vec![M32::ZERO; src.len()];
    for idx in 0..src.len() as u32 {
        advect_voxel_32(src, courant, &mut dst, p, idx);
    }
    dst
}

fn sweep_64(src: &[M64], courant: &[Q], p: &AdvectParams) -> Vec<M64> {
    let mut dst = vec![M64::ZERO; src.len()];
    for idx in 0..src.len() as u32 {
        advect_voxel_64(src, courant, &mut dst, p, idx);
    }
    dst
}

fn total_32(buffer: &[M32]) -> i64 {
    buffer.iter().map(|v| v.to_i64()).sum()
}

fn total_64(buffer: &[M64]) -> i64 {
    buffer.iter().map(|v| v.to_i64()).sum()
}

/// The undershoot bound of ADR-068, computed from the buffer rather than named:
/// `floor(f/2) + 3*ceil(spread/2^24)`, subtracted from the stencil minimum.
///
/// `f` is the number of **open** faces on the axis. A closed face carries
/// nothing and contributes no rounding error, so it is not in the count.
fn undershoot_allowance(open_faces: u32, spread: i64) -> i64 {
    const ULP: i64 = 1 << 24;
    i64::from(open_faces / 2) + 3 * ((spread + ULP - 1) / ULP).max(1)
}

// ---------------------------------------------------------------------------
// flux_is_antisymmetric
// ---------------------------------------------------------------------------

/// Amounts for the narrow width, across three magnitudes.
///
/// The bands and their reasons are the ones `acceptance_diffusion.rs` derives:
/// small, where the arithmetic can land on an exact half at all; middle and
/// large, where the `f32` runs out of mantissa and the flux comes back coarse.
///
/// The bound is 1e9 rather than `i32::MAX` because the kernel takes
/// `acceptor - donor` in the storage width, and a pair straddling the whole
/// range overflows that subtraction — a fact about `M32`, not about the flux.
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

/// Courant numbers, weighted toward the ones that can ask the rounding rule a
/// question.
///
/// The named values first, because they are the ones the corpus contains and not
/// ones invented for a test: zero, where the fallback and the sign of a signed
/// zero meet; `+-1`, where `(1 - |c|)` annihilates the antidiffusive term
/// (ADR-054); `+-0.167`, the velocity budget of ADR-069; `+-0.5`, the number
/// ADR-054 computes its numerical diffusion at. Halves and quarters land the
/// product on an exact half whenever the amounts have the matching parity, which
/// is the case a rounding rule that is not symmetric about zero fails. The rest
/// of the draws are uniform over the admissible range, which is where anything
/// else would hide.
fn courant() -> impl Strategy<Value = f64> {
    prop_oneof![
        3 => prop::sample::select(vec![
            0.0, 1.0, -1.0, BUDGET, -BUDGET, 0.5, -0.5, 0.25, -0.25, 0.75, -0.75,
        ]),
        1 => -1.0f64..=1.0f64,
    ]
}

proptest! {
    // Four thousand stencils a run, and a named place to keep a counterexample.
    // The path has to be given: the default is derived from the source file and
    // there is nothing to derive it from in an integration test.
    #![proptest_config(ProptestConfig {
        cases: 4096,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/acceptance_advection.proptest-regressions",
        ))),
        ..ProptestConfig::default()
    })]

    /// `ACCEPTANCE.md` calls this the most important test in the whole list and
    /// the cheapest. For advection it carries two statements, and the second is
    /// the one ADR-054 introduced the canonical orientation of a face for.
    ///
    /// **One: the mirror form.** A face read from the other end is the same face
    /// with the axis reversed, so
    ///
    /// ```text
    /// flux(ll, l, r, rr, c) == -flux(rr, r, l, ll, -c)
    /// ```
    ///
    /// bit for bit, in both widths, and both widths give the same integer. This
    /// is the property `floor` destroys: it rounds `1.5` to `1` and `-1.5` to
    /// `-2`, so a flux written with it compiles, satisfies the type system,
    /// passes `kernel-lint`, reads correctly, and leaks a unit per face per
    /// pass.
    ///
    /// **Two: the two-sided identity.** Run a whole strip and the domain sum
    /// does not move by a single unit. That is the assertion that fails when
    /// each voxel computes the flux "from its own side" — the defect ADR-054
    /// names — because then the two sides of a face have *different stencils*
    /// (each one's upwind cell is its own) and a different association order in
    /// one expression, which no sign symmetry can cancel in `f32`. It also fails
    /// if the two face fluxes of a voxel are summed and rounded once per voxel
    /// rather than once per face, because the neighbour would then round its own
    /// sum rather than the opposite of this one.
    #[test]
    fn flux_is_antisymmetric(
        ll in amount_32(),
        l in amount_32(),
        r in amount_32(),
        rr in amount_32(),
        wide_ll in amount_64(),
        wide_l in amount_64(),
        wide_r in amount_64(),
        wide_rr in amount_64(),
        value in courant(),
    ) {
        let forward = Q::from_f64(value);
        let mirrored = Q::from_f64(-value);

        let narrow = flux_32(M32::new(ll), M32::new(l), M32::new(r), M32::new(rr), forward);
        let narrow_back =
            flux_32(M32::new(rr), M32::new(r), M32::new(l), M32::new(ll), mirrored);
        prop_assert_eq!(
            narrow, -narrow_back,
            "flux_32({}, {}, {}, {}, {}) is not the negation of the mirrored face",
            ll, l, r, rr, value
        );

        // The same four amounts in the other width: one macro, one text, one
        // integer.
        let same_amounts = flux_64(
            M64::new(ll.into()),
            M64::new(l.into()),
            M64::new(r.into()),
            M64::new(rr.into()),
            forward,
        );
        prop_assert_eq!(narrow.to_i64(), same_amounts.to_i64());

        // And the width water lives in, at amounts no i32 could hold.
        let wide = flux_64(
            M64::new(wide_ll),
            M64::new(wide_l),
            M64::new(wide_r),
            M64::new(wide_rr),
            forward,
        );
        let wide_back = flux_64(
            M64::new(wide_rr),
            M64::new(wide_r),
            M64::new(wide_l),
            M64::new(wide_ll),
            mirrored,
        );
        prop_assert_eq!(
            wide, -wide_back,
            "flux_64({}, {}, {}, {}, {}) is not the negation of the mirrored face",
            wide_ll, wide_l, wide_r, wide_rr, value
        );

        // Two: the same four amounts laid out along a strip, swept, and summed.
        // The amounts are folded into a band that cannot overflow the sum of a
        // strip; the property under test is the pairing of faces, not the range
        // of the storage.
        const N: u32 = 8;
        let fold = |v: i32| v.rem_euclid(2_000_001) - 1_000_000;
        let src: Vec<M32> = (0..N)
            .map(|i| M32::new(fold([ll, l, r, rr][(i % 4) as usize] + i as i32)))
            .collect();

        // Two Courant fields, and the second is not decoration. Under a uniform
        // field `courant[base + idx]` and `courant[base + up]` hold the same
        // number, so the layout that *expresses* the canonical orientation —
        // a voxel reading its upper face at its neighbour's index, because that
        // cell is the neighbour's lower face — is unobserved: a voxel reading
        // its own index for both faces would still conserve, and this half of
        // the test would be blind to the very pairing it is named for. The
        // varying field takes each factor from a period that does not divide
        // `N`, so no two adjacent faces of the strip agree; `|value| <= 1` and
        // the factors are in `[-1, 1]`, so every number stays admissible.
        let varying: Vec<Q> = (0..3 * N)
            .map(|i| Q::from_f64(value * (f64::from(i % 5) - 2.0) / 2.0))
            .collect();
        let uniform = vec![forward; (3 * N) as usize];

        for boundary in [PERIODIC, FLOORED, CLOSED] {
            let (_, p) = strip(N, boundary);
            for courant in [&uniform, &varying] {
                let dst = sweep_32(&src, courant, &p);
                prop_assert_eq!(
                    total_32(&src), total_32(&dst),
                    "a sweep at c = {} moved the domain total", value
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// flux_limiter_falls_back_to_first_order_at_zero_gradient
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md`, section "Numerical verification": the special case that
/// would otherwise be found as a division by zero in production.
///
/// The ratio `r = du/df` is undefined at a zero gradient, and **both sides of
/// the face** have to fall back the same way (ADR-054). They do, and not because
/// anything here arranges it: the two sides call one function with one pair of
/// arguments, so the branch is not merely "the same expression" on both sides,
/// it is the same evaluation. The identical fallback is a consequence of the
/// canonical orientation rather than a second measure, which is why the mirror
/// identity is asserted again below, inside the fallback.
///
/// This test has to run in debug, and that is the point of it. An implementation
/// that computed `r = qdiv(du, df)` and branched afterwards would hit
/// `qdiv`'s `debug_assert!(b != 0)` here; in release it would produce an
/// infinity that some later wrapper reports as a non-finite `Q` in a different
/// function entirely. The branch therefore stands **before** the division and is
/// taken on exact integers.
#[test]
fn flux_limiter_falls_back_to_first_order_at_zero_gradient() {
    // --- The limiter itself returns exactly zero. --------------------------
    for magnitude in [1i64, 3, 1000, 1_000_000] {
        for sign in [1i64, -1] {
            let du = M32::new((sign * magnitude) as i32);
            let df = M32::new((-sign * magnitude) as i32);

            // df == 0: the ratio is undefined.
            assert_eq!(limited_slope_32(du, M32::ZERO), Q::ZERO);
            assert_eq!(
                limited_slope_64(M64::new(sign * magnitude), M64::ZERO),
                Q::ZERO
            );
            // du == 0: the flow comes off a flat patch or off a wall.
            assert_eq!(limited_slope_32(M32::ZERO, du), Q::ZERO);
            assert_eq!(
                limited_slope_64(M64::ZERO, M64::new(sign * magnitude)),
                Q::ZERO
            );
            // Opposite signs: an extremum, where a TVD limiter must be zero.
            assert_eq!(limited_slope_32(du, df), Q::ZERO);
            assert_eq!(
                limited_slope_64(M64::new(sign * magnitude), M64::new(-sign * magnitude)),
                Q::ZERO
            );
        }
    }
    assert_eq!(limited_slope_32(M32::ZERO, M32::ZERO), Q::ZERO);
    assert_eq!(limited_slope_64(M64::ZERO, M64::ZERO), Q::ZERO);

    // --- And the flux is then exactly the first-order donor flux. ----------
    //
    // Separately for a positive and a negative Courant number, because the donor
    // is a different cell in the two cases and a scheme that fell back only on
    // one side would still look right on half the domain.
    let first_order = |donor: M32, speed: Q| q_round_32(qmul(speed, q_conc_32(donor, Q::ONE)));

    // (far_low, low, high, far_high, description of which difference vanishes)
    let cases = [
        // df == 0 for either sign of c: the two cells across the face are equal.
        (100i32, 400, 400, 900),
        // du == 0 with c > 0 (far_low == low) and an extremum with c < 0.
        (400, 400, 900, 400),
        // du == 0 with c < 0 (far_high == high).
        (100, 400, 900, 900),
        // Opposite signs on both sides: a strict local extremum at the face.
        (100, 400, 200, 500),
    ];

    for (ll, l, r, rr) in cases {
        for value in [BUDGET, -BUDGET, 0.5, -0.5, 0.9, -0.9] {
            let forward = Q::from_f64(value);
            let (ll, l, r, rr) = (M32::new(ll), M32::new(l), M32::new(r), M32::new(rr));
            let downstream = value >= 0.0;
            let donor = if downstream { l } else { r };
            let acceptor = if downstream { r } else { l };
            let upwind = if downstream { ll } else { rr };

            let df = acceptor - donor;
            let du = donor - upwind;
            if limited_slope_32(du, df) != Q::ZERO {
                // Not a fallback case for this sign of c; the other sign covers
                // it. Asserting the limiter is zero here would be asserting the
                // table rather than the kernel.
                continue;
            }

            assert_eq!(
                flux_32(ll, l, r, rr, forward),
                first_order(donor, forward),
                "the fallback at ({ll:?}, {l:?}, {r:?}, {rr:?}) with c = {value} \
                 is not the first-order donor flux"
            );

            // The mirror identity holds inside the fallback too — the halves of
            // ADR-054 are one statement.
            let mirrored = Q::from_f64(-value);
            assert_eq!(
                flux_32(ll, l, r, rr, forward),
                -flux_32(rr, r, l, ll, mirrored)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// flux_is_the_two_named_crossings_composed
// a_small_gradient_in_a_large_pool_still_carries_flux
// ---------------------------------------------------------------------------

/// The water band the corpus tests the wide width in — the same literal as
/// `numeric/m.rs`, `world/field.rs`, `tests/acceptance_ledger.rs` and
/// `process/diffuse.rs`. It is 5.1e12 units, an order above the 5.1e11 ADR-040
/// derives for the registry's water, and one `f32` step across it is
/// `2^19 = 524 288`.
const WATER: i64 = 5_100_000_000_000;

/// The flux of ADR-060 written out a second time: the two named crossings
/// composed, the differences taken in `M`, one rounding on the face.
///
/// A second text of one expression is a poor test of most things and the only
/// possible test of this one. ADR-060 says so itself: both orders of the
/// composition are antisymmetric, both conserve exactly, both leave every ledger
/// at zero, and the property tests of this file cannot tell them apart. What the
/// comparison pins is three statements that have no other witness — that `df`
/// and `du` reach the limiter as **integer** subtractions, that the crossing
/// down into `Q` is `q_conc` and the crossing back up is one `q_round`, and that
/// the rounding happens once over the whole face rather than once per term.
fn composed_flux_64(far_low: M64, low: M64, high: M64, far_high: M64, courant: Q) -> M64 {
    let downstream = courant >= Q::ZERO;
    let donor = if downstream { low } else { high };
    let acceptor = if downstream { high } else { low };
    let upwind = if downstream { far_low } else { far_high };

    // The order under test. Integers first; `Q` after.
    let df = acceptor - donor;
    let du = donor - upwind;

    let magnitude = if downstream {
        courant
    } else {
        qsub(Q::ZERO, courant)
    };
    let antidiffusive = qdiv(qmul(courant, qsub(Q::ONE, magnitude)), qadd(Q::ONE, Q::ONE));

    q_round_64(qadd(
        qmul(courant, q_conc_64(donor, Q::ONE)),
        qmul(antidiffusive, limited_slope_64(du, df)),
    ))
}

/// `ACCEPTANCE.md`, section "Conservation", from ADR-060.
///
/// The advective instance of the name. ADR-060 settled the shape of a flux for
/// transport generally — no fourth crossing, no `m_scale`, the face is
/// `q_conc` down and one rounding back up — and it named this test while the
/// only flux in the project was diffusive. Advection is the second flux
/// function built to that shape, and it is the one where the shape is easiest to
/// lose, because it has three terms instead of one and a limiter between them.
///
/// The quads run from single digits to the water band, and the water ones are
/// the ones that carry the test: below `2^24` both orders of the composition
/// give the same integer and the comparison is vacuous.
#[test]
fn flux_is_the_two_named_crossings_composed() {
    let quads = [
        (0i64, 0, 0, 0),
        (100, 400, 900, 1500),
        (1500, 900, 400, 100),
        (-500, 200, 900, 1500),
        (7, 7, 7, 7),
        (1_000_000, 3, -4, 999),
        // Water. Gradients of a few hundred thousand units on a pool of 5.1e12
        // are the band where an `f32` cannot hold the difference but an `i64`
        // can, which is exactly the band ADR-060 is about.
        (
            WATER - 1_000_000_000,
            WATER,
            WATER + 250_000,
            WATER + 500_000,
        ),
        (
            WATER + 1_000_000_000,
            WATER,
            WATER - 250_000,
            WATER - 500_000,
        ),
        (
            WATER - 500_000,
            WATER - 250_000,
            WATER,
            WATER + 1_000_000_000,
        ),
        (
            WATER,
            WATER + 1_000_000,
            WATER + 3_000_000,
            WATER + 6_000_000,
        ),
        (
            WATER - 2_000_000,
            WATER - 1_000_000,
            WATER,
            WATER + 1_000_000,
        ),
    ];

    for (ll, l, r, rr) in quads {
        for value in [
            BUDGET, -BUDGET, 0.2, -0.2, 0.5, -0.5, 0.9, -0.9, 1.0, -1.0, 0.0,
        ] {
            let c = Q::from_f64(value);
            let (a, b, x, y) = (M64::new(ll), M64::new(l), M64::new(r), M64::new(rr));
            assert_eq!(
                flux_64(a, b, x, y, c),
                composed_flux_64(a, b, x, y, c),
                "the face ({ll}, {l}, {r}, {rr}) at c = {value} is not the two \
                 named crossings composed in the order ADR-060 fixes"
            );
        }
    }

    // The narrow width is the same macro body and needs no second text: what it
    // owes is that it answers the same integer wherever the amounts fit in it,
    // which is what carries the statement above across the two expansions.
    for (ll, l, r, rr) in [(100i32, 400, 900, 1500), (-500, 200, 900, 1500)] {
        for value in [BUDGET, -BUDGET, 0.5, -0.5] {
            let c = Q::from_f64(value);
            assert_eq!(
                flux_32(M32::new(ll), M32::new(l), M32::new(r), M32::new(rr), c).to_i64(),
                flux_64(
                    M64::new(ll.into()),
                    M64::new(l.into()),
                    M64::new(r.into()),
                    M64::new(rr.into()),
                    c
                )
                .to_i64()
            );
        }
    }
}

/// `ACCEPTANCE.md`, section "Conservation", and the load-bearing half of the
/// pair: the consequence of the composition order, on the one substance the
/// registry stores in 64 bits (ADR-040).
///
/// The stencil below is one face of water with a gradient of 250 000 units
/// across it — `4.9e-8` of the pool, and less than half of the `524 288` an
/// `f32` step spans there. So:
///
/// - after `q_conc` the two cells across the face are **one number**, asserted
///   below rather than assumed. The rejected order,
///   `qsub(q_conc(acceptor), q_conc(donor))`, gets exactly zero from them, hands
///   the limiter a zero difference, and returns `Q::ZERO`: the scheme has
///   quietly dropped to first order and every conservation test in this file
///   still passes, because a first-order flux is antisymmetric too;
/// - taken in `M` first the same difference is exact, the limiter is alive, and
///   the flux is *not* the first-order donor flux. Today it differs by 65 536
///   units — one `f32` step of the donor term, which is the granularity at which
///   anything can differ at this pool at all.
///
/// The regime is narrow, and that is worth saying rather than hiding. Advection
/// is not diffusion: its flux is `c*donor` plus a correction, so the donor term
/// carries the pool's own magnitude into the sum and its rounding nearly
/// swallows a correction proportional to the gradient. The band where the two
/// orders differ at the face at all is bounded on one side by the `f32` step of
/// the pool and on the other by the `f32` step of `c*pool`, which is why the
/// Courant number here is the velocity budget of ADR-069 rather than a round
/// number: at `|c| >= 0.5` the two steps meet and the band closes. On an `i32`
/// pool the same band is a handful of units wide and lies inside the ADR-068
/// rounding allowance, so the statement belongs to the wide width, exactly as
/// ADR-060 states it.
#[test]
fn a_small_gradient_in_a_large_pool_still_carries_flux() {
    const GRADIENT: i64 = 250_000;
    const UPWIND: i64 = 1_000_000_000;

    // `c < 0`, so the donor is `high` and the cell upwind of it is `far_high`.
    let c = Q::from_f64(-BUDGET);
    let far_low = M64::new(WATER - 2 * GRADIENT);
    let low = M64::new(WATER - GRADIENT);
    let high = M64::new(WATER);
    let far_high = M64::new(WATER + UPWIND);

    // The premise. One number after the crossing, an exact difference before it.
    assert_eq!(
        q_conc_64(low, Q::ONE),
        q_conc_64(high, Q::ONE),
        "the gradient is meant to be invisible after q_conc; pick a smaller one"
    );
    assert_eq!(
        qsub(q_conc_64(low, Q::ONE), q_conc_64(high, Q::ONE)),
        Q::ZERO
    );
    assert_eq!((low - high).to_i64(), -GRADIENT);

    // The limiter is alive on the integer differences.
    assert_ne!(limited_slope_64(high - far_high, low - high), Q::ZERO);

    // And the consequence: the face carries more than the donor term. The
    // first-order flux is written as the two named crossings and nothing else —
    // `q_conc` down, `m_delta` back up — which is what ADR-060 says a face is
    // when the limiter is off.
    let donor_only = m_delta_64(q_conc_64(high, Q::ONE), c);
    let limited = flux_64(far_low, low, high, far_high, c);
    assert_ne!(
        limited, donor_only,
        "a gradient of {GRADIENT} units on a pool of {WATER} carried nothing \
         beyond the donor term: the differences were taken after the crossing \
         into Q, not before it (ADR-060)"
    );

    // Not a property of one sign. The mirrored face is the same face read from
    // the other end (ADR-054), and it carries the same integer negated.
    assert_eq!(
        flux_64(far_high, high, low, far_low, Q::from_f64(BUDGET)),
        -limited
    );
}

// ---------------------------------------------------------------------------
// advection_undershoot_stays_within_the_derived_bound
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md`, section "Conservation". Renamed from
/// `advection_never_produces_negative_amount` by ADR-068, which cancelled the
/// operational form of that promise: an integer flux-form scheme has no limiter
/// that keeps amounts non-negative, because every face flux rounds to a whole
/// unit and TVD is a property of the reals.
///
/// What is asserted instead is the bound ADR-068 derives, computed **from the
/// buffer under test**:
///
/// ```text
/// new >= (minimum over the stencil) - floor(f/2) - 3*ceil(spread/2^24)
/// ```
///
/// No absolute number appears here and none may: the bound is three units below
/// `2^23`, six below `2^24`, fifty-one at the ceiling ADR-039 allows and 91 198
/// for water. A test promising a constant would be right about one registry and
/// wrong about the next legal one.
///
/// The other half of the criterion is on the same line: the domain total before
/// and after is the same integer. The clamp ADR-068 rejects would have made the
/// first half pass and this one fail — silently, because the matter it removed
/// is already in the neighbours.
// TODO(advective-undershoot-bound): ADR-068 declares the bound covers advection
// and derives it on the diffusive chain, and two places do not carry over.
//
// The anchor: the diffusive derivation reaches "not below the stencil minimum"
// through a convex combination of the voxel and its neighbours. Advection's
// convex-combination argument (Sweby's incremental form) needs a **uniform**
// Courant number along the axis; under a divergent face velocity field a uniform
// field legitimately falls below its stencil minimum, and what holds there is
// positivity relative to zero, out of the outflow condition of SPEC section 4.2.
//
// The `f32` term: `3*ceil(spread/2^24)` was derived from a chain whose argument
// is a difference. Advection's donor term crosses into `Q` as an amount, so the
// term understates the error wherever the spread is much smaller than the pool —
// for water it is the whole of the second-order correction.
//
// So this run stays inside the regime where the two forms agree: uniform `c`,
// pool below `2^23`. The advective form of the bound is a journal entry, and
// there is no number for it in the corpus.
#[test]
fn advection_undershoot_stays_within_the_derived_bound() {
    const N: u32 = 32;

    // Non-negative, spiky, and well below 2^23 = 8 388 608 — the band inside
    // which ADR-068's `f32` term and the advective one coincide.
    let seed: Vec<i64> = (0..N)
        .map(|i| match i % 7 {
            0 => 4_000_000,
            1 => 0,
            2 => 12,
            3 => 3,
            4 => 1_000_000,
            _ => 250_000,
        })
        .collect();

    for boundary in [PERIODIC, CLOSED] {
        let (_, p) = strip(N, boundary);
        let periodic = boundary[0] == Boundary::Periodic;

        for value in [BUDGET, -BUDGET, 0.5, -0.5, 1.0, -1.0, 0.9] {
            let courant = vec![Q::from_f64(value); (3 * N) as usize];
            let src: Vec<M32> = seed.iter().map(|&v| M32::new(v as i32)).collect();
            let dst = sweep_32(&src, &courant, &p);

            let amounts: Vec<i64> = src.iter().map(|v| v.to_i64()).collect();
            let spread = amounts.iter().max().unwrap() - amounts.iter().min().unwrap();

            for i in 0..N {
                // The stencil this voxel actually read: two cells down, itself,
                // two cells up, folded at a wall exactly as the kernel folds
                // them.
                let step = |from: u32, up: bool| -> u32 {
                    if up {
                        if from + 1 < N {
                            from + 1
                        } else if periodic {
                            0
                        } else {
                            from
                        }
                    } else if from > 0 {
                        from - 1
                    } else if periodic {
                        N - 1
                    } else {
                        from
                    }
                };
                let down = step(i, false);
                let up = step(i, true);
                let stencil = [step(down, false), down, i, up, step(up, true)];
                let floor_of_stencil = stencil.iter().map(|&j| amounts[j as usize]).min().unwrap();

                let open = u32::from(periodic || i > 0) + u32::from(periodic || i + 1 < N);
                let bound = floor_of_stencil - undershoot_allowance(open, spread);
                assert!(
                    dst[i as usize].to_i64() >= bound,
                    "voxel {i} came out at {} against a derived floor of {bound} \
                     (stencil minimum {floor_of_stencil}, {open} open faces, \
                     spread {spread}) at c = {value}",
                    dst[i as usize].to_i64()
                );
            }

            // Exactly. A clamp would have printed the difference.
            assert_eq!(
                total_32(&src),
                total_32(&dst),
                "the sweep at c = {value} moved the domain total"
            );
            if value != 0.0 {
                assert_ne!(src, dst, "nothing moved at c = {value}");
            }
        }
    }

    // The same statement in the width water is stored in, at a pool where the
    // `f32` term is no longer a formality.
    let (_, p) = strip(N, PERIODIC);
    let src: Vec<M64> = (0..N)
        .map(|i| M64::new(if i % 5 == 0 { 5_100_000_000_000 } else { 100 }))
        .collect();
    let courant = vec![Q::from_f64(BUDGET); (3 * N) as usize];
    let dst = sweep_64(&src, &courant, &p);
    assert_eq!(total_64(&src), total_64(&dst));

    let amounts: Vec<i64> = src.iter().map(|v| v.to_i64()).collect();
    let spread = amounts.iter().max().unwrap() - amounts.iter().min().unwrap();
    let allowance = undershoot_allowance(2, spread);
    for i in 0..N as usize {
        let neighbourhood = [
            amounts[(i + N as usize - 2) % N as usize],
            amounts[(i + N as usize - 1) % N as usize],
            amounts[i],
            amounts[(i + 1) % N as usize],
            amounts[(i + 2) % N as usize],
        ];
        let floor_of_stencil = *neighbourhood.iter().min().unwrap();
        assert!(
            dst[i].to_i64() >= floor_of_stencil - allowance,
            "water voxel {i} came out at {} against {}",
            dst[i].to_i64(),
            floor_of_stencil - allowance
        );
    }
}

// ---------------------------------------------------------------------------
// advection_of_a_step_produces_no_new_extrema
// ---------------------------------------------------------------------------

/// The flux of a scheme with a *fixed* limiter value, for the two reference
/// schemes this test needs. `phi == 0` is the first-order donor scheme, `phi ==
/// 1` is Lax — Wendroff.
///
/// Written out here rather than parameterised into the kernel on purpose: the
/// kernel has no `phi`, and giving it one so that a test could reach it would be
/// a parameter that only a test uses (ADR-015 forbids exactly that kind of
/// generality inside a kernel). Everything except the limiter is the same
/// composition in the same order, so the comparison below is about the limiter
/// and about nothing else.
fn reference_flux(low: M32, high: M32, speed: Q, second_order: bool) -> M32 {
    let downstream = speed >= Q::ZERO;
    let donor = if downstream { low } else { high };
    let acceptor = if downstream { high } else { low };
    let magnitude = if downstream {
        speed
    } else {
        qsub(Q::ZERO, speed)
    };
    let antidiffusive = qdiv(qmul(speed, qsub(Q::ONE, magnitude)), qadd(Q::ONE, Q::ONE));
    let slope = if second_order {
        q_conc_32(acceptor - donor, Q::ONE)
    } else {
        Q::ZERO
    };
    q_round_32(qadd(
        qmul(speed, q_conc_32(donor, Q::ONE)),
        qmul(antidiffusive, slope),
    ))
}

/// One pass of a reference scheme over a periodic strip, in gather form, with
/// the same sign convention as the kernel.
fn reference_pass(src: &[M32], speed: Q, second_order: bool) -> Vec<M32> {
    let n = src.len();
    (0..n)
        .map(|i| {
            let down = (i + n - 1) % n;
            let up = (i + 1) % n;
            let lower = reference_flux(src[down], src[i], speed, second_order);
            let upper = reference_flux(src[i], src[up], speed, second_order);
            src[i] + lower - upper
        })
        .collect()
}

/// The number of voxels strictly inside the two plateaux: how wide the front has
/// been smeared.
fn transition_band(buffer: &[M32], low: i64, high: i64) -> usize {
    buffer
        .iter()
        .filter(|v| v.to_i64() > low && v.to_i64() < high)
        .count()
}

/// How far the worst voxel lies outside the initial range.
fn excursion(buffer: &[M32], low: i64, high: i64) -> i64 {
    buffer
        .iter()
        .map(|v| (low - v.to_i64()).max(v.to_i64() - high).max(0))
        .max()
        .unwrap_or(0)
}

/// Total variation along the periodic axis.
fn total_variation(buffer: &[M32]) -> i64 {
    let n = buffer.len();
    (0..n)
        .map(|i| (buffer[(i + 1) % n].to_i64() - buffer[i].to_i64()).abs())
        .sum()
}

/// `ACCEPTANCE.md`, section "Numerical verification".
///
/// A monotone step on a periodic axis, advected many times, must not grow a new
/// extremum beyond the rounding bound of ADR-068, and its total variation must
/// not grow.
///
/// # The half without which the test is unfalsifiable
///
/// A pure first-order donor scheme passes the statement above with room to
/// spare: it produces no new extrema at all because it is monotone, it just
/// smears the front across the domain. So a limiter stuck at zero — the very
/// failure mode `flux_limiter_falls_back_to_first_order_at_zero_gradient` is
/// designed around — would sail through, and this test would be evidence of
/// nothing.
///
/// The test therefore computes the two neighbouring schemes on the same data and
/// asserts that van Leer is strictly between them: a **narrower** transition band
/// than the donor scheme, which is the numerical diffusion ADR-054 exists to
/// remove, and **no overshoot** where Lax — Wendroff has one, which is the
/// positivity a second-order scheme without a limiter gives up. A limiter stuck
/// at zero fails the first half; a limiter that is not applied at all fails the
/// second.
///
/// What is *not* here is the order of convergence: that is
/// `mms_advection_converges_at_order_2_on_monotone_data`, which needs a
/// manufactured solution and is a different job.
#[test]
fn advection_of_a_step_produces_no_new_extrema() {
    const N: u32 = 64;
    const LOW: i64 = 0;
    const HIGH: i64 = 1_000_000;

    let (_, p) = strip(N, PERIODIC);
    let initial: Vec<M32> = (0..N)
        .map(|i| {
            M32::new(if (16..48).contains(&i) {
                HIGH as i32
            } else {
                LOW as i32
            })
        })
        .collect();
    let variation_before = total_variation(&initial);
    assert_eq!(variation_before, 2 * (HIGH - LOW));

    // The whole spread is far below 2^24, so the `f32` term of the bound is at
    // its floor and the allowance is one unit of rounding per open face plus
    // three (ADR-068). It accumulates over passes: `min_k >= min_{k-1} - B`, so
    // over `k` passes the proved statement is `k*B`.
    let per_pass = undershoot_allowance(2, HIGH - LOW);

    for (value, passes) in [(BUDGET, 24usize), (0.5, 16), (-0.5, 16)] {
        let speed = Q::from_f64(value);
        let courant = vec![speed; (3 * N) as usize];

        let mut limited = initial.clone();
        let mut donor = initial.clone();
        let mut lax_wendroff = initial.clone();

        for pass in 0..passes {
            let before = limited.clone();
            limited = sweep_32(&limited, &courant, &p);
            donor = reference_pass(&donor, speed, false);
            lax_wendroff = reference_pass(&lax_wendroff, speed, true);

            // The proved per-application statement, checked where it is proved:
            // against the previous state rather than against the initial one.
            let floor_before = before.iter().map(|v| v.to_i64()).min().unwrap();
            let ceiling_before = before.iter().map(|v| v.to_i64()).max().unwrap();
            let spread = ceiling_before - floor_before;
            let allowance = undershoot_allowance(2, spread);
            for (i, v) in limited.iter().enumerate() {
                assert!(
                    v.to_i64() >= floor_before - allowance
                        && v.to_i64() <= ceiling_before + allowance,
                    "pass {pass} at c = {value}: voxel {i} left \
                     [{floor_before}, {ceiling_before}] by more than {allowance}"
                );
            }

            assert_eq!(
                total_32(&limited),
                total_32(&initial),
                "pass {pass} at c = {value} moved the domain total"
            );
        }

        // No new extrema over the whole run, within the accumulated bound.
        let drift = excursion(&limited, LOW, HIGH);
        assert!(
            drift <= per_pass * passes as i64,
            "the limited scheme left [{LOW}, {HIGH}] by {drift} at c = {value}"
        );

        // The total variation of a monotone profile does not grow under a TVD
        // scheme, and here it does not grow at all.
        assert!(
            total_variation(&limited) <= variation_before,
            "total variation grew from {variation_before} to {} at c = {value}",
            total_variation(&limited)
        );

        // --- And the two reference schemes, on the same data. --------------
        let limited_band = transition_band(&limited, LOW, HIGH);
        let donor_band = transition_band(&donor, LOW, HIGH);
        assert!(
            limited_band < donor_band,
            "van Leer smeared the front over {limited_band} voxels and the \
             first-order donor scheme over {donor_band} at c = {value}: the \
             limiter is not doing anything"
        );

        // Lax — Wendroff is the same expression with the limiter removed, and it
        // overshoots. If it ever stops, this comparison has stopped being
        // evidence and the assertion above about van Leer is unfalsifiable.
        let unlimited_drift = excursion(&lax_wendroff, LOW, HIGH);
        assert!(
            unlimited_drift > per_pass * passes as i64,
            "Lax — Wendroff did not overshoot at c = {value}, so this test \
             cannot tell a limiter from no limiter"
        );
        assert!(
            drift < unlimited_drift,
            "van Leer overshot as far as the unlimited scheme at c = {value}"
        );
    }
}

// ---------------------------------------------------------------------------
// exchange_inflow_falls_back_to_first_order
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md`, from ADR-059 and ADR-054.
///
/// At the face of the domain the limiter switches itself off on **inflow**, and
/// it costs no branch at all: the cell upwind of the ghost is the ghost, so the
/// numerator `du` of the ratio is exactly zero and van Leer answers zero at any
/// `df`. That is precisely the fallback ADR-054 prescribes wherever `r` is
/// undefined, reached by the address rather than by a special case — the flux is
/// the same four-cell expression every interior face runs through.
///
/// # Asserted on the limiter and not only on its consequence
///
/// `kernels/advect.rs` makes the limiter public for this reason and says why: a
/// test that could see only the flux would pass on a limiter stuck at zero
/// everywhere. So the first assertion is about `limited_slope_32` itself, over a
/// spread of `df`, and the second is the flux it produces.
///
/// # The mirror is half the test
///
/// On **outflow** through the same face the donor is the voxel below the lid, its
/// upwind cell is inside the domain, the stencil is full, and the limiter is
/// alive. Without that half a `neighbour_along` that collapsed the ghost onto
/// itself in *both* directions would pass — and it would have killed the limiter
/// on outflow too.
#[test]
fn exchange_inflow_falls_back_to_first_order() {
    const NZ: u32 = 5;
    let grid = Grid::new(
        1,
        1,
        NZ,
        [
            Boundary::Closed,
            Boundary::Closed,
            Boundary::Closed,
            Boundary::Closed,
            Boundary::Closed,
            Boundary::Exchange,
        ],
    )
    .unwrap();
    let ghost = grid.ghost_index();
    let lane_len = grid.lane_len();
    let top = grid.index(0, 0, NZ - 1);
    assert_eq!(grid.neighbour(top, Face::ZPlus), ghost);
    assert_eq!(grid.neighbour(ghost, Face::ZPlus), ghost);
    assert_eq!(grid.neighbour(ghost, Face::ZMinus), ghost);

    let p = AdvectParams {
        nx: 1,
        ny: 1,
        nz: NZ,
        periodic_mask: 0,
        exchange_mask: 1 << (Face::ZPlus as u32),
        axis: 2,
    };

    // The column and the reservoir are chosen so that the test can fail.
    //
    // A limiter that read **any other cell** as the one upwind of the ghost —
    // which is what a `neighbour_along` that did not collapse the ghost onto
    // itself would hand it — must come out nonzero, or the first-order answer
    // and the second-order one coincide and the assertion below is vacuous. So
    // the profile is not monotone: it rises into the domain and falls at the
    // lid, and the reservoir sits between the second voxel and the top one.
    const COLUMN: [i32; NZ as usize] = [1_000, 1_100, 3_000, 5_000, 2_200];
    const RESERVOIR: i32 = 1_500;
    let mut src: Vec<M32> = COLUMN.iter().map(|&a| M32::new(a)).collect();
    src.push(M32::new(RESERVOIR));
    assert_eq!(src.len(), lane_len as usize);
    let reservoir = src[ghost as usize];
    // The two differences a wrongly-addressed upwind cell would produce have the
    // same sign, so the limiter would be alive on them.
    assert_ne!(
        limited_slope_32(reservoir - src[1], src[top as usize] - reservoir),
        Q::ZERO,
        "the fixture cannot tell a live limiter from a dead one"
    );

    // --- one: the limiter itself ------------------------------------------

    // `du == 0` is what the address buys, and van Leer is zero there at **any**
    // `df` — including the ones where the two differences would have the same
    // sign and a live limiter would answer something large.
    for df in [-1_000_000i32, -9_000, -1, 0, 1, 9_000, 1_000_000] {
        assert_eq!(
            limited_slope_32(M32::ZERO, M32::new(df)),
            Q::ZERO,
            "the limiter is alive at a zero upwind difference, df = {df}"
        );
    }
    assert_eq!(
        reservoir - reservoir,
        M32::ZERO,
        "du at the face of the domain"
    );

    // --- two: the flux is the bare donor term -----------------------------

    // Inflow: the Courant number on the lid points toward the smaller index, so
    // the donor is the ghost and the cell upwind of it is the ghost.
    let inflow = Q::from_f64(-BUDGET);
    let donor_only = q_round_32(qmul(inflow, q_conc_32(reservoir, Q::ONE)));
    assert_ne!(donor_only, M32::ZERO, "the fixture moves nothing at all");
    assert_eq!(
        flux_32(
            src[top as usize],
            src[top as usize],
            reservoir,
            reservoir,
            inflow
        ),
        donor_only,
        "the face of the domain is not first order on inflow"
    );

    // And through the kernel, which is what actually reads the address: the top
    // voxel gains exactly the negative of that face's flux, because a positive
    // flux points toward the larger index.
    let mut courant = vec![Q::ZERO; (3 * lane_len) as usize];
    let base = 2 * lane_len;
    courant[(base + ghost) as usize] = inflow;
    let mut dst = vec![M32::ZERO; lane_len as usize];
    for idx in 0..grid.n_voxels() {
        advect_voxel_32(&src, &courant, &mut dst, &p, idx);
    }
    assert_eq!(
        dst[top as usize] - src[top as usize],
        M32::ZERO - donor_only,
        "the kernel did not apply the first-order flux of the lid"
    );
    // Nothing below the top layer moved: the lid is the only open face here.
    for z in 0..NZ - 1 {
        let idx = grid.index(0, 0, z) as usize;
        assert_eq!(dst[idx], src[idx], "voxel at z = {z} moved");
    }

    // --- three: the mirror, on outflow ------------------------------------

    // The same face, the other sign. The donor is now the voxel under the lid,
    // its upwind cell is the one below **that**, the stencil is full, and on a
    // column with a gradient the limiter is not zero.
    let below = grid.index(0, 0, NZ - 2);
    let du = src[top as usize] - src[below as usize];
    let df = reservoir - src[top as usize];
    assert_ne!(du, M32::ZERO, "the fixture's column is flat");
    assert_ne!(
        limited_slope_32(du, df),
        Q::ZERO,
        "the limiter is dead on outflow too: the ghost collapses onto itself in \
         both directions"
    );

    let outflow = Q::from_f64(BUDGET);
    let bare = q_round_32(qmul(outflow, q_conc_32(src[top as usize], Q::ONE)));
    assert_ne!(
        flux_32(
            src[below as usize],
            src[top as usize],
            reservoir,
            reservoir,
            outflow
        ),
        bare,
        "the face of the domain fell back to first order on outflow as well"
    );
}
