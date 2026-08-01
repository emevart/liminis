//! Counter-based randomness: no state, no sequence, no order (SPEC section 9).
//!
//! `rand(a, b, c, run_key)` is a pure function of its four counters. It is not a
//! stream and there is nothing to advance, seed or synchronise: two voxels
//! drawing at the same tick do not share anything, and a voxel drawing twice at
//! the same tick with the same purpose gets the same number twice — which is why
//! the third counter exists.
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
//!   guard in CI now says so itself: ADR-058 widened its pattern from `configs/`,
//!   `kernels/` and `process/` to cover this file, so a change here that forgets
//!   `WORLD_FORMAT_VERSION` fails the pull request instead of passing quietly.
//!
//! # The fourth counter
//!
//! `run_key` is the run seed, folded to 32 bits once on the host by [`run_key`]
//! and folded into the chain by a fourth round (ADR-058). Before it, the seed
//! was a third of a run's identity that reached nothing: two runs of one
//! scenario were bit-identical, because the only conversion from `Q` to an
//! integer in the whole of chemistry — the stochastic rounding of extent
//! (ADR-027) — drew from an unseeded generator. The seed either arrives here or
//! it does not reach the chemistry at all.
//!
//! The canonical order is `rand(voxel_idx, tick, reaction_id, run_key)`, and it
//! is worth writing down because the counters do not commute. The reaction
//! skeleton in `ARCHITECTURE.md` writes its wrapper as
//! `rng_key(p.run_key, p.tick, idx, r)` — the reverse order — so a wrapper that
//! forwards its arguments as they arrive produces a *different* stream: uniform,
//! unbiased, passing every statistical test below, and incomparable with the
//! reference run. Nothing but the anchor test can see that mistake.
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
//! to WGSL line by line, which is the same standard every kernel is held to, and
//! the fourth round does not take that away.
//!
//! Each counter is folded in through its own round, so the four arguments are
//! not interchangeable: `rand(1, 2, 3, 4)` and `rand(4, 3, 2, 1)` are unrelated.

/// A nonzero start for the chain.
///
/// Every operation in [`mix`] maps zero to zero, so without this constant
/// `rand(0, 0, 0, 0)` would be zero — and the first voxel of the first tick
/// would draw a guaranteed zero for every purpose, forever. The value is the
/// usual golden-ratio odd constant; nothing depends on which nonzero constant it
/// is, only on it being nonzero and fixed.
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

/// The run seed as a counter: a `u64` seed folded into one `u32` (ADR-058).
///
/// The seed is 64 bits on the command line and the generator counts in 32, so
/// the two halves are mixed here, on the host, exactly once. That placement is
/// the requirement: base WGSL has no 64-bit integers (ADR-040, NUMERIC.md
/// section 5), so no 64-bit operation may survive into a kernel.
///
/// The nesting is asymmetric on purpose. `mix(lo) ^ mix(hi)` is shorter, reads
/// more neutrally, and collapses every pair of seeds whose halves are swapped:
/// `0x2_0000_0001` and `0x1_0000_0002` would both come out as 4_126_714_175 —
/// one run under two names, and nothing short of comparing state byte for byte
/// would reveal it. Folding `hi` through a round and into `lo` costs the same
/// and does not do that.
///
/// The fold is 2^32-to-1, and the collisions it leaves are the declared price of
/// ADR-058 rather than a defect to patch: about 1.2% somewhere in a gallery of
/// ten thousand seeds. The mitigation is that the host prints this key next to
/// the seed in the run's identity line, so a collision is visible instead of
/// silent.
///
/// `run_key(0) == 0`, and that is left exactly as it is. Forcing the key nonzero
/// — `| 1`, or a substitute constant — maps some seeds onto each other for real,
/// and there is nothing to fix: the fourth round is not the identity at a zero
/// key, so `rand(0, 0, 0, 0)` is not zero either (see
/// `zero_counters_do_not_give_zero`).
#[inline(always)]
pub const fn run_key(seed: u64) -> u32 {
    let hi = (seed >> 32) as u32;
    let lo = seed as u32;
    mix(lo ^ mix(hi))
}

/// A uniform `u32` from four counters.
///
/// The canonical call sites are `(voxel_idx, tick, reaction_id, run_key)` for
/// the stochastic rounding of reaction extent (ADR-027, ADR-058) and
/// `(cell_id, tick, purpose, run_key)` for cell behaviour (SPEC section 9).
///
/// The first three counters are positional: what they mean is decided by the
/// call site. The fourth is always the run key, which is why it is named and not
/// `d` — and why the three-counter form was deleted rather than kept alongside.
/// Two entry points into one generator are two streams, and one day somebody
/// picks the wrong one; an arity error is loud, a silently unseeded draw is not.
///
/// ```
/// use liminis_core::numeric::{rand, run_key};
///
/// // A pure function of its arguments: no state, no order, no history.
/// assert_eq!(rand(7, 9, 11, run_key(42)), rand(7, 9, 11, run_key(42)));
/// assert_ne!(rand(7, 9, 11, run_key(42)), rand(11, 9, 7, run_key(42)));
/// ```
#[inline(always)]
pub const fn rand(a: u32, b: u32, c: u32, run_key: u32) -> u32 {
    let mut h = mix(START ^ a);
    h = mix(h ^ b);
    h = mix(h ^ c);
    // Last, and through a round of its own. Folding the key in before the first
    // round instead — `mix(START ^ a ^ run_key)`, or in place of `START` — is
    // free and wrong: two seeds one bit apart then produce the same multiset of
    // draws with the voxels permuted, so the noise field of run B is the noise
    // field of run A with its cells shuffled, and every statistical test in this
    // file is happy with that (ADR-058, rejected).
    h = mix(h ^ run_key);
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric::{Q, stochastic_round};

    /// Anchors the stream. These are not magic numbers to be re-blessed when
    /// they stop matching: a changed value here means every run in the project
    /// changed, which is a `WORLD_FORMAT_VERSION` event (ADR-020), not a test
    /// to update.
    ///
    /// They were re-blessed exactly once, by ADR-058, in the commit that moved
    /// `WORLD_FORMAT_VERSION` from 2 to 3 — because the fourth round is not the
    /// identity even at a zero key: `mix` is a bijection and maps zero to zero
    /// only as a whole word, not on top of an already nonzero `h`. Both halves
    /// of that change belong to one commit; a revision carrying the new numbers
    /// under version 2 would be a run nobody can identify afterwards.
    ///
    /// The last line is not part of the re-blessing but an addition. The five
    /// probes above all pass `run_key = 0` and therefore cannot tell "the key is
    /// folded in by a fourth round" from "the key is not folded in at all",
    /// which is precisely the mistake this file is one line away from.
    #[test]
    fn the_stream_is_the_same_stream_it_was() {
        assert_eq!(rand(0, 0, 0, 0), 4_024_642_630);
        assert_eq!(rand(1, 0, 0, 0), 55_943_592);
        assert_eq!(rand(0, 1, 0, 0), 173_636_613);
        assert_eq!(rand(0, 0, 1, 0), 2_993_682_749);
        assert_eq!(rand(u32::MAX, u32::MAX, u32::MAX, 0), 2_792_197_155);
        assert_eq!(rand(0, 0, 0, 1), 4_015_628_648);
    }

    #[test]
    fn zero_counters_do_not_give_zero() {
        // Every step of the mixer maps zero to zero; the start constant is what
        // keeps the first voxel of the first tick from drawing a certain zero.
        //
        // Since ADR-058 that has a second reading. `run_key(0) == 0`, so the
        // all-zero call is not a hypothetical corner: it is the run of the seed
        // a person types first. The start constant covers it in the
        // four-counter form too, which is why no seed has to be rejected and no
        // key has to be forced nonzero.
        assert_eq!(run_key(0), 0);
        assert_ne!(rand(0, 0, 0, 0), 0);
    }

    /// ACCEPTANCE.md, section "Determinism" — one of the three names ADR-058
    /// sends there.
    ///
    /// What ADR-058 claims is about the rounding of chemistry, not about the
    /// mixer, so the assertion is made on `stochastic_round` and not on raw
    /// draws. Comparing draws would repeat `permuting_the_counters_changes_the_draw`
    /// and leave untested the one path the decision was taken for: the seed
    /// reaching `xi` through the only `Q`-to-integer conversion in the
    /// chemistry (ADR-027, NUMERIC.md sections 1 and 3).
    #[test]
    fn different_seed_changes_the_rounding_on_identical_state() {
        // One state, two runs: a thousand voxels of one tick of one reaction,
        // every one of them with the same extent to round. Seeds 1 and 2 are
        // arbitrary and deliberately short — that both halves of a wide seed
        // reach the key is the next test's job, this one only needs two keys
        // that differ.
        let a = run_key(1);
        let b = run_key(2);

        // Half a quantum: the draw decides every voxel, so the two runs
        // disagree on each with probability 1/2. A value with a small
        // fractional part would weaken the claim into uselessness — at 0.001
        // the two fields agree on 99.8% of voxels and the threshold would have
        // to sit at zero, where "the key arrives" and "the key arrives
        // sometimes" look the same.
        let x = Q::from_f64(0.5);

        let diverged = (0..1000u32)
            .filter(|&i| {
                stochastic_round(x, rand(i, 7, 3, a)) != stochastic_round(x, rand(i, 7, 3, b))
            })
            .count();

        // A thousand independent coin flips: 500 expected, sigma about 15.8,
        // and this band is six of them — the same style as the chi-square
        // threshold below. Observed: 527. Zero means the key never reaches the
        // draw at all, which is the failure this test exists for; a key that
        // permutes voxels instead of changing values would still land near 500
        // and is caught by the anchor test, not by this one.
        assert!(
            (400..=600).contains(&diverged),
            "{diverged} of 1000 voxels rounded differently under two run keys"
        );
    }

    /// ACCEPTANCE.md, section "Determinism" — the second of the three names.
    #[test]
    fn run_key_is_derived_from_both_halves_of_the_seed() {
        // `seed as u32` is the line written in a hurry, and the CLI takes a
        // u64: under it, seeds 1 and 1 + 2^32 would run bit-identically under
        // two names, and whoever spreads a gallery out over the high bits gets
        // one trajectory back.
        assert_ne!(run_key(1), run_key(1 + (1 << 32)));
        assert_ne!(run_key(0), run_key(1 << 32));

        // And the halves must not be interchangeable, which is what the
        // asymmetric nesting buys. Under `mix(lo) ^ mix(hi)` these two seeds
        // both come out as 4_126_714_175.
        assert_ne!(run_key(0x2_0000_0001), run_key(0x1_0000_0002));
    }

    #[test]
    fn draws_are_reproducible_and_independent_of_call_order() {
        // A nonzero key throughout, here and in every statistical test below:
        // on key 0 the whole distribution of the generator would keep being
        // measured on the one key no real run has except seed 0, and a fourth
        // round that ruins the spread at a nonzero key would pass unnoticed.
        let key = run_key(1);
        let keys = [(0u32, 0u32, 0u32), (3, 5, 7), (12345, 999, 1), (7, 5, 3)];

        let forward: Vec<u32> = keys.iter().map(|&(a, b, c)| rand(a, b, c, key)).collect();

        // Same keys, opposite order, with unrelated draws interleaved. A
        // generator with hidden state answers differently here.
        let mut backward: Vec<u32> = Vec::new();
        for &(a, b, c) in keys.iter().rev() {
            let _ = rand(a ^ 0xdead, b ^ 0xbeef, c, key);
            backward.push(rand(a, b, c, key));
        }
        backward.reverse();

        assert_eq!(forward, backward);
    }

    #[test]
    fn permuting_the_counters_changes_the_draw() {
        // The four arguments mean different things, so they must not commute:
        // otherwise voxel 3 at tick 5 and voxel 5 at tick 3 would round their
        // chemistry identically.
        let key = run_key(1);
        assert_ne!(rand(3, 5, 7, key), rand(5, 3, 7, key));
        assert_ne!(rand(3, 5, 7, key), rand(3, 7, 5, key));
        assert_ne!(rand(3, 5, 7, key), rand(7, 5, 3, key));

        // The run key is not interchangeable with a voxel index or a reaction
        // identifier either. If it were, a run seeded to key K and a reaction
        // whose identifier happens to be K would share one stream.
        assert_ne!(rand(3, 5, 7, 11), rand(11, 5, 7, 3));
        assert_ne!(rand(3, 5, 7, 11), rand(3, 5, 11, 7));
    }

    #[test]
    fn neighbouring_counters_do_not_give_neighbouring_draws() {
        // Voxel index and tick both walk by one. If the mixer let that through,
        // adjacent voxels would round in lockstep and the noise of ADR-027
        // would become a pattern.
        let key = run_key(1);
        for i in 0..1000u32 {
            let a = rand(i, 42, 3, key);
            let b = rand(i + 1, 42, 3, key);
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

        let key = run_key(1);
        let mut counts = [0u32; BUCKETS];
        for i in 0..DRAWS {
            let r = rand(i, 1, 2, key);
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

        let key = run_key(1);
        let mut ones = [0u32; 32];
        for i in 0..DRAWS {
            let r = rand(i, 7, 13, key);
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
