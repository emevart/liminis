//! Counter-based randomness: no state, no sequence, no order (SPEC section 9).
//!
//! `rand(a, b, c)` is a pure function of its three counters. It is not a stream
//! and there is nothing to advance, seed or synchronise: two voxels drawing at
//! the same tick do not share anything, and a voxel drawing twice at the same
//! tick with the same purpose gets the same number twice — which is why the
//! third counter exists.
//!
//! This started as a convenience for cell behaviour and became load-bearing with
//! ADR-027: the turnover of every reaction is rounded stochastically, so this
//! function is now part of the chemistry. Two consequences follow, and both are
//! easy to break without noticing.
//!
//! - **The counters have to be stable.** `reaction_id` comes from the *name* of
//!   the reaction, never from its position in the file (ADR-027), or reordering
//!   two lines of TOML changes the stream and therefore the run, at unchanged
//!   semantics. Same for `cell_id` at division (SPEC section 9, open question
//!   B-8).
//! - **This function is world semantics.** Change the mixer and every run in the
//!   project changes with it, the same way a changed kernel does. The ADR-020
//!   guard in CI watches `configs/`, `kernels/` and `process/` and does not
//!   watch this file; the obligation to bump `WORLD_FORMAT_VERSION` applies all
//!   the same.
//!
//! # Why this mixer
//!
//! An avalanche of xorshift-and-multiply rounds, in the family of the MurmurHash3
//! finaliser; the constants are the ones published as `triple32` in Wellons's
//! hash-function prospector, chosen there for lowest measured bias among
//! three-round 32-bit mixers.
//!
//! Chosen over the obvious alternatives for one property: it needs nothing but
//! 32-bit xor, shift and wrapping multiply. Base WGSL has no 64-bit integers
//! (ADR-040 says the same thing about storage), so Philox and squares — the
//! two counter-based generators one would reach for first — would have to
//! emulate their 64-bit or high-half multiplies with pairs of `u32` on the GPU,
//! and the emulation is where a CPU/GPU divergence would hide. This mixer ports
//! to WGSL line by line, which is the same standard every kernel is held to.
//!
//! Each counter is folded in through its own round, so the three arguments are
//! not interchangeable: `rand(1, 2, 3)` and `rand(3, 2, 1)` are unrelated.

/// A nonzero start for the chain.
///
/// Every operation in [`mix`] maps zero to zero, so without this constant
/// `rand(0, 0, 0)` would be zero — and the first voxel of the first tick would
/// draw a guaranteed zero for every purpose, forever. The value is the usual
/// golden-ratio odd constant; nothing depends on which nonzero constant it is,
/// only on it being nonzero and fixed.
const START: u32 = 0x9e37_79b9;

/// One 32-bit avalanche round. A bijection: every step is invertible, so no
/// two inputs collide.
#[inline(always)]
const fn mix(mut x: u32) -> u32 {
    x ^= x >> 17;
    x = x.wrapping_mul(0xed5a_d4bb);
    x ^= x >> 11;
    x = x.wrapping_mul(0xac4c_1b51);
    x ^= x >> 15;
    x = x.wrapping_mul(0x3184_8bab);
    x ^= x >> 14;
    x
}

/// A uniform `u32` from three counters.
///
/// The canonical call sites are `(voxel_idx, tick, reaction_id)` for the
/// stochastic rounding of reaction extent (ADR-027) and `(cell_id, tick,
/// purpose)` for cell behaviour (SPEC section 9).
///
/// ```
/// use liminis_core::numeric::rand;
///
/// // A pure function of its arguments: no state, no order, no history.
/// assert_eq!(rand(7, 9, 11), rand(7, 9, 11));
/// assert_ne!(rand(7, 9, 11), rand(11, 9, 7));
/// ```
#[inline(always)]
pub const fn rand(a: u32, b: u32, c: u32) -> u32 {
    let mut h = mix(START ^ a);
    h = mix(h ^ b);
    h = mix(h ^ c);
    h
}

// TODO(seed): how the run seed reaches this generator is not decided anywhere,
// and it is not decided here either.
//
// SPEC section 9 and QUANTITIES.md section 9 both give the generator three
// counters and no seed, while `seed` is one third of a run's identity and
// ACCEPTANCE.md asks for `different_seed_gives_different_state`. Both hold at
// once under at least three different arrangements — fold the seed into a
// fourth counter here; fold it into `purpose` on the host; or leave the
// chemistry unseeded and let the seed enter only through worldgen — and they
// are not equivalent: the first two make two runs of one scenario differ in
// their chemistry, the third makes them differ only in their initial state.
//
// That is a decision about what a seed means, so it belongs in DECISIONS.md and
// not in this file. Until it is made, callers pass three counters and the seed
// does not reach the chemistry at all.

#[cfg(test)]
mod tests {
    use super::*;

    /// Anchors the stream. These are not magic numbers to be re-blessed when
    /// they stop matching: a changed value here means every run in the project
    /// changed, which is a `WORLD_FORMAT_VERSION` event (ADR-020), not a test
    /// to update.
    #[test]
    fn the_stream_is_the_same_stream_it_was() {
        assert_eq!(rand(0, 0, 0), 2_849_889_213);
        assert_eq!(rand(1, 0, 0), 1_606_903_817);
        assert_eq!(rand(0, 1, 0), 859_215_229);
        assert_eq!(rand(0, 0, 1), 1_388_560_977);
        assert_eq!(rand(u32::MAX, u32::MAX, u32::MAX), 1_743_567_203);
    }

    #[test]
    fn zero_counters_do_not_give_zero() {
        // Every step of the mixer maps zero to zero; the start constant is what
        // keeps the first voxel of the first tick from drawing a certain zero.
        assert_ne!(rand(0, 0, 0), 0);
    }

    #[test]
    fn draws_are_reproducible_and_independent_of_call_order() {
        let keys = [(0u32, 0u32, 0u32), (3, 5, 7), (12345, 999, 1), (7, 5, 3)];

        let forward: Vec<u32> = keys.iter().map(|&(a, b, c)| rand(a, b, c)).collect();

        // Same keys, opposite order, with unrelated draws interleaved. A
        // generator with hidden state answers differently here.
        let mut backward: Vec<u32> = Vec::new();
        for &(a, b, c) in keys.iter().rev() {
            let _ = rand(a ^ 0xdead, b ^ 0xbeef, c);
            backward.push(rand(a, b, c));
        }
        backward.reverse();

        assert_eq!(forward, backward);
    }

    #[test]
    fn permuting_the_counters_changes_the_draw() {
        // The three arguments mean different things, so they must not commute:
        // otherwise voxel 3 at tick 5 and voxel 5 at tick 3 would round their
        // chemistry identically.
        assert_ne!(rand(3, 5, 7), rand(5, 3, 7));
        assert_ne!(rand(3, 5, 7), rand(3, 7, 5));
        assert_ne!(rand(3, 5, 7), rand(7, 5, 3));
    }

    #[test]
    fn neighbouring_counters_do_not_give_neighbouring_draws() {
        // Voxel index and tick both walk by one. If the mixer let that through,
        // adjacent voxels would round in lockstep and the noise of ADR-027
        // would become a pattern.
        for i in 0..1000u32 {
            let a = rand(i, 42, 3);
            let b = rand(i + 1, 42, 3);
            assert!(
                a.abs_diff(b) > 1024,
                "draws for adjacent voxels are adjacent at i = {i}: {a} vs {b}"
            );
        }
    }

    #[test]
    fn draws_are_uniform_over_the_range() {
        // Chi-square over 64 buckets, 65536 draws. Expected 1024 per bucket,
        // 63 degrees of freedom; the 99.9% critical value is about 115, and a
        // biased mixer overshoots it by orders of magnitude rather than by a
        // little.
        const BUCKETS: usize = 64;
        const DRAWS: u32 = 65_536;

        let mut counts = [0u32; BUCKETS];
        for i in 0..DRAWS {
            let r = rand(i, 1, 2);
            counts[(r >> 26) as usize] += 1;
        }

        let expected = f64::from(DRAWS) / BUCKETS as f64;
        let chi2: f64 = counts
            .iter()
            .map(|&n| {
                let d = f64::from(n) - expected;
                d * d / expected
            })
            .sum();

        assert!(chi2 < 115.0, "chi-square {chi2} over {BUCKETS} buckets");
    }

    #[test]
    fn every_bit_is_a_coin_flip() {
        // Uniformity over buckets only looks at the top bits. Stochastic
        // rounding uses the top 24, but the low bits are one `>>` away from
        // being used by something else.
        const DRAWS: u32 = 20_000;

        let mut ones = [0u32; 32];
        for i in 0..DRAWS {
            let r = rand(i, 7, 13);
            for (bit, count) in ones.iter_mut().enumerate() {
                *count += (r >> bit) & 1;
            }
        }

        for (bit, &count) in ones.iter().enumerate() {
            let share = f64::from(count) / f64::from(DRAWS);
            assert!(
                (share - 0.5).abs() < 0.02,
                "bit {bit} is set {share} of the time"
            );
        }
    }
}
