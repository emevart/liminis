//! The crossings between `M` and `Q`, and the only rounding in the system.
//!
//! NUMERIC.md section 1 allows exactly three, and the count is the point: every
//! place where an exact integer becomes an approximate number, or the other way
//! round, is a place where the ledger can lose a unit. Three named functions can
//! be read, tested and argued about; rounding sprinkled through the kernels
//! cannot.
//!
//! ```text
//! q_conc(amount: M, ...)            -> Q    amount to concentration
//! m_delta(rate: Q, dt)              -> M    rate to change in amount
//! xi(rate: Q, dt, volume, e_r, rng) -> i32  rate to whole quanta of extent
//! ```
//!
//! The first two are the throat for transport and for anything debited
//! continuously, and they round with one deterministic rule: halves away from
//! zero, the rule that keeps flux functions antisymmetric (ADR-034). The third
//! rounds stochastically, and that is a deliberate exception with a reason of
//! its own — see [`xi`].
//!
//! The two width-dependent crossings are generated from one text, for the reason
//! spelled out in `m.rs`: two hand-written copies drift apart, and the drift
//! shows up as "the 64-bit substance somehow behaves differently".

use super::m::{M32, M64};
use super::mode::{
    self, Q, exp2_from_u8, floor_i64, frac, qmul, round_half_away_from_zero, unit_from_u32,
};

// TODO(m_scale): the transport crossing is missing, and it is not invented here.
//
// The diffusion skeleton in ARCHITECTURE.md writes its flux as
// `m_scale(b - a, alpha)` and imports `m_scale` from this module — an
// `M`-times-`Q`-to-`M` function that every transport kernel needs. NUMERIC.md
// section 1 lists exactly three crossings and `m_scale` is not one of them; the
// two documents disagree about how many there are, and the missing one is
// precisely the one that carries mass across every face of the grid.
//
// Adding a crossing is a decision about where rounding is allowed to happen, and
// those live in DECISIONS.md. Everything such a function would need is already
// here: the rounding rule is `q_round_32` / `q_round_64`, and it is the rule the
// antisymmetry of the flux depends on.

/// Generates the two width-dependent crossings and the rounding entry point for
/// one storage width.
macro_rules! define_conversions {
    ($m:ident, $repr:ty, $q_round:ident, $q_conc:ident, $m_delta:ident) => {
        #[doc = concat!("Round a `Q` into an `", stringify!($m), "`, halves away from zero.")]
        ///
        /// The single rounding rule of the system (NUMERIC.md section 3). Its
        /// two purposes, and the reason `floor` must not replace it, are written
        /// out on the mode-level primitive this calls
        /// (`round_half_away_from_zero`); read that before touching this.
        ///
        /// Out of range clamps, and asserts in debug: an amount that does not
        /// fit means the storage width the loader derived (ADR-040) came from a
        /// wrong `max_conc`.
        #[inline(always)]
        pub fn $q_round(x: Q) -> $m {
            $m::from_i64_clamping(round_half_away_from_zero(x))
        }

        #[doc = concat!("An amount in `", stringify!($m), "` as a concentration, mol/m^3.")]
        ///
        /// The one way down from the counted world into the intensive one
        /// (QUANTITIES.md section 2). Kinetics works in concentrations and
        /// touches amounts only at the boundary, which is what keeps the voxel
        /// out of the config (SPEC section 10).
        ///
        /// `conc_per_unit` is what one storage unit is worth as a concentration:
        /// `1 / (units_per_mol * V_voxel)`, in mol/m^3 per unit. Both factors
        /// arrive folded into one number because the host folds parameters
        /// before a kernel sees them (ARCHITECTURE.md) — a kernel that knew
        /// `units_per_mol` and `V_voxel` separately would be a kernel that could
        /// combine them in the wrong order.
        ///
        /// A note for whoever checks this against the documents: NUMERIC.md
        /// section 1 writes the second argument as `volume`, while
        /// QUANTITIES.md section 2 defines concentration as
        /// `amount / (units_per_mol * V_voxel)`. One folded factor is the only
        /// reading that satisfies both.
        #[inline(always)]
        pub fn $q_conc(amount: $m, conc_per_unit: Q) -> Q {
            qmul(mode::from_i64(amount.to_i64()), conc_per_unit)
        }

        #[doc = concat!("A rate over one step as a change in amount, in `", stringify!($m), "`.")]
        ///
        /// The one way up from the intensive world into the counted one, for
        /// transport and for everything debited continuously — a flux through a
        /// face, the upkeep of an expression channel (NUMERIC.md section 1).
        ///
        /// `rate` is in storage units per second: the host folds `units_per_mol`
        /// and the voxel volume into it, by the same rule and for the same
        /// reason as in `q_conc`. Reactions do **not** come through here. There
        /// the rounding happens once, over the extent, and never over individual
        /// substances (ADR-027); use `xi`.
        ///
        /// Rounding is halves away from zero, so `m_delta(-rate, dt)` is exactly
        /// `-m_delta(rate, dt)`. Anything built on this function inherits the
        /// antisymmetry it needs (ADR-034).
        #[inline(always)]
        pub fn $m_delta(rate: Q, dt: Q) -> $m {
            $q_round(qmul(rate, dt))
        }
    };
}

define_conversions!(M32, i32, q_round_32, q_conc_32, m_delta_32);
define_conversions!(M64, i64, q_round_64, q_conc_64, m_delta_64);

/// Round a `Q` to a whole number stochastically: `floor(x) + [rand < frac(x)]`.
///
/// The exception to the rule above, and a deliberate one (ADR-027, NUMERIC.md
/// section 3). Deterministic rounding here would not add a bias — it would stop
/// the slow half of the chemistry outright. A reaction turning over less than
/// one quantum of extent per tick rounds to zero every tick, forever: the
/// turnover of refractory detritus and the whole background of abiotic chemistry
/// go to a standstill, and over a million ticks that is not noise but a missing
/// process.
///
/// This draw is unbiased in the mean, needs no state and no extra field, and
/// stays bit-reproducible because the generator has no state to diverge
/// ([`super::rand`]). The price is noise at low rates, which the chemistry of
/// small numbers has physically anyway.
///
/// Note for anyone tempted to make this symmetric like the other rule: it must
/// not be. Conservation in a reaction is a property of the stoichiometric vector
/// `nu`, not of the extent (ADR-027), so rounding the extent any way at all is
/// safe — and `floor` plus a Bernoulli draw is what makes it unbiased.
#[inline(always)]
pub fn stochastic_round(x: Q, rng: u32) -> i64 {
    let whole = floor_i64(x);
    let carry = i64::from(unit_from_u32(rng) < frac(x));
    whole + carry
}

/// A reaction rate as a whole number of quanta of extent (SPEC section 5,
/// ADR-027, ADR-039).
///
/// The third and last crossing, and the only rounding in the whole of chemistry:
///
/// ```text
/// x  = rate * dt * V_voxel * 2^e_r
/// xi = floor(x) + [rand(voxel_idx, tick, reaction_id) < frac(x)]
/// ```
///
/// What comes back counts quanta of extent of size `2^-e_r` of a turnover, not
/// turnovers. The difference is not cosmetic: a turnover of the Redfield formula
/// unit is 106 mol of carbon, an eco-regime voxel holds about 1e-11 mol of it,
/// so a whole number of turnovers per voxel per tick would be zero at every
/// rate and every scale. Quanta are of order one, which is where stochastic
/// rounding does anything at all.
///
/// Changes in amount are then `nu_i * xi` with integer `nu_i` — exact, which is
/// why rounding, capping and competition scaling may all be applied to `xi`
/// without touching the balance (ADR-027).
///
/// `rate`, `dt` and `volume` are the quantities of the formula: turnovers per
/// second, seconds, and the voxel volume in m^3, folded on the host.
///
/// Note for whoever wires this into the reaction kernel: the skeleton in
/// ARCHITECTURE.md passes `p.inv_volume` here, while SPEC section 5 multiplies
/// by `V_voxel`. This function follows SPEC — one of the two documents has the
/// factor upside down, and it is not this file's place to decide which.
#[inline(always)]
pub fn xi(rate: Q, dt: Q, volume: Q, e_r: u8, rng: u32) -> i32 {
    let x = qmul(qmul(qmul(rate, dt), volume), exp2_from_u8(e_r));
    let quanta = stochastic_round(x, rng);
    // The extent is an i32 (QUANTITIES.md section 6). Capping by available
    // substrate is the kernel's job and happens after this, on the same value;
    // an extent that does not fit an i32 before any capping means `e_r` was
    // derived from a wrong declaration (ADR-039).
    const LOW: i64 = i32::MIN as i64;
    const HIGH: i64 = i32::MAX as i64;
    debug_assert!(
        (LOW..=HIGH).contains(&quanta),
        "extent {quanta} does not fit an i32 before capping"
    );
    quanta.clamp(LOW, HIGH) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric::{qdiv, qsub, rand};

    fn q(v: f64) -> Q {
        Q::from_f64(v)
    }

    #[test]
    fn rounding_into_both_widths_takes_halves_away_from_zero() {
        assert_eq!(q_round_32(q(1.5)), M32::new(2));
        assert_eq!(q_round_32(q(-1.5)), M32::new(-2));
        assert_eq!(q_round_32(q(0.5)), M32::new(1));
        assert_eq!(q_round_32(q(-0.5)), M32::new(-1));
        assert_eq!(q_round_32(q(0.49)), M32::ZERO);

        assert_eq!(q_round_64(q(1.5)), M64::new(2));
        assert_eq!(q_round_64(q(-1.5)), M64::new(-2));
        assert_eq!(q_round_64(q(2.5)), M64::new(3));
        assert_eq!(q_round_64(q(-2.5)), M64::new(-3));
    }

    #[test]
    fn amount_to_concentration_and_back_agree() {
        // A pool of 2^20 units at 1e-4 m voxel edge and 1e6 units per mol: the
        // shape of the numbers the scale derivation aims at (ADR-039).
        let volume = 1.0e-4f64.powi(3);
        let units_per_mol = 1.0e6;
        let conc_per_unit = q(1.0 / (units_per_mol * volume));
        let units_per_conc = q(units_per_mol * volume);

        for &units in &[0i32, 1, 1024, 1 << 20, -(1 << 20)] {
            let amount = M32::new(units);
            let conc = q_conc_32(amount, conc_per_unit);
            // Back up the same way the host would come down: one folded factor,
            // multiplied rather than divided.
            let back = q_round_32(qmul(conc, units_per_conc));
            assert_eq!(back, amount, "round trip lost {units}");

            // And the same trip written as a division, which is what a reader
            // checking the units will try.
            assert_eq!(q_round_32(qdiv(conc, conc_per_unit)), amount);
        }
    }

    #[test]
    fn concentration_is_the_same_number_in_both_widths() {
        // The macro exists so that these two cannot drift apart.
        let conc_per_unit = q(3.5e-7);
        for &units in &[0i64, 1, 4095, 1 << 20] {
            let narrow = q_conc_32(M32::new(units as i32), conc_per_unit);
            let wide = q_conc_64(M64::new(units), conc_per_unit);
            assert_eq!(narrow, wide);
        }
    }

    #[test]
    fn a_rate_becomes_an_amount_and_survives_the_trip_back() {
        let dt = q(1.0);
        for &rate in &[0.0f64, 1.0, 7.0, 1234.0, -56.0] {
            let delta = m_delta_32(q(rate), dt);
            assert_eq!(f64::from(delta.raw()), rate);
        }

        // Half a unit per second for two seconds is one unit, and the rounding
        // is the same rounding as everywhere else.
        assert_eq!(m_delta_32(q(0.5), q(2.0)), M32::new(1));
        assert_eq!(m_delta_32(q(0.5), q(1.0)), M32::new(1));
        assert_eq!(m_delta_32(q(-0.5), q(1.0)), M32::new(-1));
        assert_eq!(m_delta_64(q(2.5), q(1.0)), M64::new(3));
    }

    #[test]
    fn m_delta_is_antisymmetric() {
        // Transport is built out of this: `flux(a, b) == -flux(b, a)` has to
        // hold exactly, or conservation in gather form stops holding (ADR-034).
        for i in -2000..=2000i64 {
            let rate = q(i as f64 * 0.0005);
            let dt = q(0.75);
            assert_eq!(m_delta_64(rate, dt), -m_delta_64(qsub(Q::ZERO, rate), dt));
        }
    }

    #[test]
    fn stochastic_rounding_only_ever_returns_a_neighbour() {
        for i in 0..1000u32 {
            let x = q(7.3);
            let v = stochastic_round(x, rand(i, 0, 0));
            assert!(v == 7 || v == 8, "stochastic rounding produced {v}");
        }
    }

    #[test]
    fn stochastic_rounding_leaves_whole_numbers_alone() {
        // frac == 0 means the Bernoulli draw can never fire: an exact amount
        // must not acquire noise.
        for i in 0..1000u32 {
            assert_eq!(stochastic_round(q(4.0), rand(i, 1, 1)), 4);
            assert_eq!(stochastic_round(q(-4.0), rand(i, 2, 2)), -4);
            assert_eq!(stochastic_round(Q::ZERO, rand(i, 3, 3)), 0);
        }
    }

    /// ACCEPTANCE.md, section "Determinism" — the name of this test is fixed
    /// there.
    ///
    /// Deterministic rounding of the extent would be biased toward zero and
    /// would stop everything slower than one quantum per tick (ADR-027). This is
    /// the test that says the replacement is actually unbiased, and it is the
    /// only one that would notice if the low bits of the generator were skewed.
    #[test]
    fn stochastic_rounding_is_unbiased_over_1e6_draws() {
        const DRAWS: u32 = 1_000_000;

        for &fraction in &[0.1f64, 0.5, 0.9, 0.375] {
            let x = 7.0 + fraction;
            let mut total = 0i64;
            for i in 0..DRAWS {
                total += stochastic_round(q(x), rand(i, 4242, 3));
            }

            let mean = total as f64 / f64::from(DRAWS);
            // One standard error is about 0.0005 at fraction = 0.5, so this is
            // a six-sigma band. The test is deterministic: it either passes
            // always or fails always.
            assert!(
                (mean - x).abs() < 3.0e-3,
                "mean {mean} over {DRAWS} draws, expected {x}"
            );
        }
    }

    #[test]
    fn extent_follows_the_spec_formula() {
        // rate * dt * V * 2^e_r with an exact power of two and no fractional
        // part: the draw cannot change the answer.
        let rate = q(4.0);
        let dt = q(0.5);
        let volume = q(0.25);

        // 4 * 0.5 * 0.25 * 2^1 = 1.0 exactly: whole, so the draw cannot matter.
        assert_eq!(xi(rate, dt, volume, 1, 0), 1);
        assert_eq!(xi(rate, dt, volume, 1, u32::MAX), 1);
        assert_eq!(xi(rate, dt, volume, 4, 0), 8);

        // At 2^0 the product is half a quantum and the draw decides. Both
        // outcomes are reachable; that they come up in the right proportion is
        // the unbiasedness test above.
        assert_eq!(xi(rate, dt, volume, 0, 0), 1);
        assert_eq!(xi(rate, dt, volume, 0, u32::MAX), 0);
    }

    #[test]
    fn extent_of_a_dead_reaction_is_zero() {
        assert_eq!(xi(Q::ZERO, q(1.0), q(1.0), 20, rand(1, 2, 3)), 0);
    }
}
