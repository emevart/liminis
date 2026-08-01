//! `FLOAT` mode: `Q` is an `f32`, and every operation over it is a wrapper.
//!
//! The mode is chosen in `mod.rs` and this is the only implementation that
//! exists today (ADR-022, NUMERIC.md section 8). `FIXED` — `Q` as an `i32` with
//! a declared scale per quantity — is a sibling module offering the same names
//! and the same signatures; nothing outside this file may know which one it is
//! talking to.
//!
//! Two things here are load-bearing and easy to lose in a cleanup:
//!
//! - the inner `f32` is private and there is no `Deref`, so `a * b` over two
//!   `Q` does not compile. This is the first line of defence in ADR-022, and it
//!   is worth more than any grep because it holds without being run;
//! - the transcendentals go through wrappers even though `FLOAT` implements
//!   them with one library call. IEEE-754 requires bit-exact results only for
//!   `+`, `-`, `*`, `/` and square root; `exp`, `log` and `pow` differ between
//!   vendors in the last bits, so `FLOAT` is *not* bit-reproducible across
//!   platforms for those three, and `FIXED` replaces them with tables
//!   (NUMERIC.md section 2). A kernel written through the wrappers gets that
//!   fix for free; one written through `f32::exp` does not.

/// An intensive or derived quantity: concentration, temperature, reaction rate,
/// expression, any intermediate result inside a kernel.
///
/// The representation is private on purpose. Going through the wrappers is the
/// rule (ADR-022) and this is what makes it checkable by the compiler rather
/// than by a reviewer:
///
/// ```
/// use liminis_core::numeric::{Q, qmul};
///
/// let a = Q::from_f64(2.0);
/// let b = Q::from_f64(3.0);
/// assert_eq!(qmul(a, b), Q::from_f64(6.0));
/// ```
///
/// A bare operator over two `Q` does not compile:
///
/// ```compile_fail
/// use liminis_core::numeric::Q;
///
/// let a = Q::from_f64(2.0);
/// let b = Q::from_f64(3.0);
/// let _ = a * b;
/// ```
///
/// Neither does reaching around the type for the representation:
///
/// ```compile_fail
/// use liminis_core::numeric::Q;
///
/// let a = Q::from_f64(2.0);
/// let _: f32 = a.0;
/// ```
///
/// Comparisons are allowed and needed — `rand < frac(x)` in ADR-027 is one —
/// and they are not arithmetic, so they carry no mode risk.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Q(f32);

impl Q {
    /// Zero. Not a wrapper call, so it is available in const context.
    pub const ZERO: Self = Self(0.0);

    /// One.
    pub const ONE: Self = Self(1.0);

    /// Build a `Q` from a host-side number.
    ///
    /// Config parameters are `f64` (QUANTITIES.md), and this is where they stop
    /// being `f64`. It is a host-side call: a kernel never converts, it receives
    /// its numbers already folded into a `Params` struct (ARCHITECTURE.md).
    #[inline(always)]
    pub fn from_f64(value: f64) -> Self {
        Self(value as f32)
    }

    /// The value as `f64`, for diagnostics, metrics and test assertions.
    ///
    /// Explicit and named, so that reading the representation is visible at the
    /// call site. `Deref` would make it invisible and bring bare arithmetic back
    /// with it, which is the whole point of the type.
    ///
    /// The return type is `f64` rather than the representation on purpose: this
    /// signature has to survive the switch to `FIXED`, where the representation
    /// is an `i32` and a scale, and where the honest answer to "what number is
    /// this" is still a `f64`.
    #[inline(always)]
    pub fn debug_f64(self) -> f64 {
        f64::from(self.0)
    }
}

/// Debug-only guard. In release it compiles to nothing, which is what lets the
/// wrappers be free (NUMERIC.md section 2: "the compiler inlines them away").
#[inline(always)]
fn expect_finite(x: Q) {
    debug_assert!(
        x.0.is_finite(),
        "a non-finite Q reached a wrapper: {x:?}. In FLOAT mode this is what \
         overflow looks like; in FIXED mode it would be a wrapped integer and \
         no test would see it."
    );
}

/// `a + b`.
#[inline(always)]
pub fn qadd(a: Q, b: Q) -> Q {
    expect_finite(a);
    expect_finite(b);
    let r = Q(a.0 + b.0);
    expect_finite(r);
    r
}

/// `a - b`.
#[inline(always)]
pub fn qsub(a: Q, b: Q) -> Q {
    expect_finite(a);
    expect_finite(b);
    let r = Q(a.0 - b.0);
    expect_finite(r);
    r
}

/// `a * b`.
#[inline(always)]
pub fn qmul(a: Q, b: Q) -> Q {
    expect_finite(a);
    expect_finite(b);
    let r = Q(a.0 * b.0);
    expect_finite(r);
    r
}

/// `a / b`.
#[inline(always)]
pub fn qdiv(a: Q, b: Q) -> Q {
    expect_finite(a);
    expect_finite(b);
    debug_assert!(b.0 != 0.0, "qdiv by zero: {a:?} / {b:?}");
    let r = Q(a.0 / b.0);
    expect_finite(r);
    r
}

/// `exp(x)`. A table in `FIXED`; see the note at the top of this file about
/// what that buys.
#[inline(always)]
pub fn qexp(x: Q) -> Q {
    expect_finite(x);
    Q(x.0.exp())
}

/// `ln(x)`, natural logarithm.
#[inline(always)]
pub fn qlog(x: Q) -> Q {
    expect_finite(x);
    debug_assert!(x.0 > 0.0, "qlog outside its domain: {x:?}");
    Q(x.0.ln())
}

/// `x^y`. `FIXED` builds it as `qexp(qmul(y, qlog(x)))`, so the domain here is
/// the same as `qlog`'s: `x > 0`.
#[inline(always)]
pub fn qpow(x: Q, y: Q) -> Q {
    expect_finite(x);
    expect_finite(y);
    debug_assert!(x.0 > 0.0, "qpow outside its domain: {x:?} ^ {y:?}");
    Q(x.0.powf(y.0))
}

/// `sqrt(x)`.
#[inline(always)]
pub fn qsqrt(x: Q) -> Q {
    expect_finite(x);
    debug_assert!(x.0 >= 0.0, "qsqrt outside its domain: {x:?}");
    Q(x.0.sqrt())
}

/// `1 / x`. Separate from `qdiv` because `FIXED` computes it from a table
/// rather than by dividing.
#[inline(always)]
pub fn qrcp(x: Q) -> Q {
    expect_finite(x);
    debug_assert!(x.0 != 0.0, "qrcp of zero");
    let r = Q(1.0 / x.0);
    expect_finite(r);
    r
}

/// `1 / (1 + exp(-x))`.
///
/// Written in the naive form on purpose: it is the form NUMERIC.md section 2
/// gives for `FLOAT`, it translates to WGSL line by line, and it has no hole at
/// either end — a large positive `x` gives `exp(-x) == 0` and a result of one, a
/// large negative `x` gives `exp(-x) == inf` and a result of zero.
#[inline(always)]
pub fn qsigmoid(x: Q) -> Q {
    expect_finite(x);
    Q(1.0 / (1.0 + (-x.0).exp()))
}

/// Round to a whole number, halves away from zero. One rule for the whole
/// system (NUMERIC.md section 3).
///
/// It has two purposes, and the second one is the one that will get this
/// function replaced by mistake.
///
/// **Drift.** Truncation toward zero is biased downward. Over the 1e6..1e7 ticks
/// this project is built for, the bias accumulates into exactly the drift that
/// integer accounting was introduced to avoid (ADR-004).
///
/// **Antisymmetry, and this matters more.** Kernels are written in gather form,
/// and conservation there does not rest on a test — it rests on the flux
/// function being antisymmetric, `f(a, b) == -f(b, a)` (ADR-034). Halves away
/// from zero is symmetric about zero: `1.5 -> 2` and `-1.5 -> -2`. Rounding down
/// is not: `floor(1.5) = 1` but `floor(-1.5) = -2`. Reach for `floor` — the
/// natural way to round an integer shift, and the one every numeric library
/// makes cheapest — and mass leaks one unit per face per substep, with no
/// failing test, a compiling kernel, a clean grep, and a ledger that diverges a
/// few thousand ticks later.
///
/// So if you are here to make this cheaper: the property to preserve is
/// `round(-x) == -round(x)` for every `x`. Nothing else about the function is
/// sacred.
///
/// Two traps for whoever ports this to WGSL:
///
/// - WGSL's `round()` is *not* this rule. It rounds halves to even, which is a
///   different function and breaks nothing visibly;
/// - the obvious branchless replacement, `trunc(x + copysign(0.5, x))`, is not
///   this rule either. At `x = 0.49999997` (the largest `f32` below a half) the
///   sum rounds up to exactly `1.0` and the result is one, where this function
///   gives zero.
#[inline(always)]
pub(in crate::numeric) fn round_half_away_from_zero(x: Q) -> i64 {
    expect_finite(x);
    // `f32::round` is exactly "halves away from zero", and it is an exact
    // operation: no double rounding, no library table, the same answer on every
    // platform. The cast to an integer saturates in Rust rather than wrapping,
    // so a value out of range clamps instead of changing sign; whoever hands a
    // value that big to this function has a bug upstream, and `q_round_32`
    // asserts about it.
    x.0.round() as i64
}

/// `floor(x)`, as an integer. Used by stochastic rounding, which is a different
/// rule for a documented reason — see [`crate::numeric::xi`].
#[inline(always)]
pub(in crate::numeric) fn floor_i64(x: Q) -> i64 {
    expect_finite(x);
    x.0.floor() as i64
}

/// `x - floor(x)`, always in `[0, 1)`.
#[inline(always)]
pub(in crate::numeric) fn frac(x: Q) -> Q {
    expect_finite(x);
    Q(x.0 - x.0.floor())
}

/// An integer amount as a `Q`.
///
/// Lossy above 2^24 by the nature of `f32`, and that is not a defect to fix
/// here: the result is a concentration or a rate, which is a `Q` and therefore
/// approximate by construction. Exactness lives on the `M` side of the boundary.
#[inline(always)]
pub(in crate::numeric) fn from_i64(v: i64) -> Q {
    Q(v as f32)
}

/// A random `u32` as a uniform draw in `[0, 1)`.
///
/// Twenty-four bits, one per bit of `f32` mantissa, so every representable
/// value in the range comes out with equal probability and the conversion is
/// exact. The remaining bias is one part in 2^24 and it is upward: comparing
/// `u < frac` accepts `ceil(frac * 2^24)` of the 2^24 possible draws.
#[inline(always)]
pub(in crate::numeric) fn unit_from_u32(r: u32) -> Q {
    Q((r >> 8) as f32 * (1.0 / 16_777_216.0))
}

/// `2^e`, exactly.
///
/// Not `exp2` from the library: this is an exponent of extent (ADR-039) and it
/// has an exact answer at every input, so there is no reason to spend a
/// vendor-dependent transcendental on it. In `FIXED` this becomes a shift.
#[inline(always)]
pub(in crate::numeric) fn exp2_from_u8(e: u8) -> Q {
    debug_assert!(
        e <= 127,
        "2^{e} is outside the f32 exponent range; an extent exponent this large \
         means the scale derivation (ADR-039) produced something unusable"
    );
    // Bias 127, mantissa zero: the bit pattern of a power of two.
    Q(f32::from_bits((u32::from(e) + 127) << 23))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(v: f64) -> Q {
        Q::from_f64(v)
    }

    #[test]
    fn wrappers_do_what_float_arithmetic_does() {
        assert_eq!(qadd(q(2.5), q(0.25)).debug_f64(), 2.75);
        assert_eq!(qsub(q(2.5), q(0.25)).debug_f64(), 2.25);
        assert_eq!(qmul(q(2.5), q(4.0)).debug_f64(), 10.0);
        assert_eq!(qdiv(q(2.5), q(0.5)).debug_f64(), 5.0);
        assert_eq!(qsqrt(q(6.25)).debug_f64(), 2.5);
        assert_eq!(qrcp(q(4.0)).debug_f64(), 0.25);
        // Not an exact comparison, and the reason is the point of the wrapper:
        // IEEE-754 says nothing about `pow`, so its last bits belong to the
        // vendor (NUMERIC.md section 2).
        assert!((qpow(q(2.0), q(10.0)).debug_f64() - 1024.0).abs() < 1e-3);
    }

    #[test]
    fn transcendentals_agree_with_their_definitions() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;

        assert!(close(qexp(Q::ZERO).debug_f64(), 1.0));
        assert!(close(qexp(Q::ONE).debug_f64(), std::f64::consts::E));
        assert!(close(qlog(Q::ONE).debug_f64(), 0.0));
        assert!(close(qlog(qexp(q(2.0))).debug_f64(), 2.0));
        assert!(close(qsigmoid(Q::ZERO).debug_f64(), 0.5));
        assert!(close(
            qsigmoid(q(2.0)).debug_f64(),
            1.0 / (1.0 + (-2.0f64).exp())
        ));
    }

    #[test]
    fn sigmoid_saturates_without_producing_nan() {
        assert_eq!(qsigmoid(q(200.0)).debug_f64(), 1.0);
        assert_eq!(qsigmoid(q(-200.0)).debug_f64(), 0.0);
    }

    #[test]
    fn rounding_of_halves_is_symmetric_about_zero() {
        // The property everything else in this file exists to protect.
        assert_eq!(round_half_away_from_zero(q(0.5)), 1);
        assert_eq!(round_half_away_from_zero(q(-0.5)), -1);
        assert_eq!(round_half_away_from_zero(q(1.5)), 2);
        assert_eq!(round_half_away_from_zero(q(-1.5)), -2);
        assert_eq!(round_half_away_from_zero(q(2.5)), 3);
        assert_eq!(round_half_away_from_zero(q(-2.5)), -3);

        // Halves to even would give 2 here, and floor would give 1: both are
        // wrong for a different reason, and neither is visible in a kernel.
        assert_ne!(round_half_away_from_zero(q(2.5)), 2);
        assert_ne!(round_half_away_from_zero(q(1.5)), 1);
    }

    #[test]
    fn rounding_is_antisymmetric_over_a_sweep() {
        // 4001 values through and past the halves, both signs.
        for i in 0..=4000i64 {
            let x = i as f64 * 0.0025 - 5.0;
            let a = round_half_away_from_zero(q(x));
            let b = round_half_away_from_zero(q(-x));
            assert_eq!(a, -b, "round(-x) != -round(x) at x = {x}");
        }
    }

    #[test]
    fn rounding_does_not_fall_into_the_double_rounding_hole() {
        // The largest f32 below a half. `trunc(x + copysign(0.5, x))` answers
        // one here; the rule answers zero.
        let just_under_half = f64::from(0.5f32 - f32::EPSILON / 4.0);
        assert!(just_under_half < 0.5);
        assert_eq!(round_half_away_from_zero(q(just_under_half)), 0);
        assert_eq!(round_half_away_from_zero(q(-just_under_half)), 0);
    }

    #[test]
    fn frac_and_floor_split_a_value_without_losing_it() {
        for &x in &[0.0, 0.25, 3.75, -0.25, -3.75, 1e6 + 0.5] {
            let f = floor_i64(q(x));
            let r = frac(q(x));
            assert!(r >= Q::ZERO && r < Q::ONE, "frac out of range at {x}");
            assert_eq!(f as f64 + r.debug_f64(), x, "floor + frac != x at {x}");
        }
    }

    #[test]
    fn unit_draws_stay_inside_the_half_open_unit_interval() {
        for r in [0u32, 1, 255, 256, u32::MAX / 2, u32::MAX - 1, u32::MAX] {
            let u = unit_from_u32(r);
            assert!(u >= Q::ZERO, "unit draw below zero for {r}");
            assert!(u < Q::ONE, "unit draw reached one for {r}");
        }
        assert_eq!(unit_from_u32(0).debug_f64(), 0.0);
    }

    #[test]
    fn powers_of_two_are_exact() {
        assert_eq!(exp2_from_u8(0).debug_f64(), 1.0);
        assert_eq!(exp2_from_u8(1).debug_f64(), 2.0);
        assert_eq!(exp2_from_u8(10).debug_f64(), 1024.0);
        // ADR-040 derives e_r = 63 for the default registry, so this input is
        // not hypothetical.
        assert_eq!(exp2_from_u8(63).debug_f64(), 9_223_372_036_854_775_808.0);
        assert_eq!(exp2_from_u8(127).debug_f64(), 2.0f64.powi(127));
    }
}
