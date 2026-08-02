//! The stirred half of the velocity potential: octaves of lattice noise added to
//! the convective potential.
//!
//! ```text
//! A = A_conv + amplitude * sum_k 2^-k * n_k
//! ```
//!
//! Third of the four dispatches of step `b` (ADR-069), and **the expensive one**:
//! three components times five octaves times eight corners is 120 draws per
//! velocity cell, `3.1e7` hashes a tick at 64^3 — twice the cost of a full
//! seven-point pass over 128^3. That is why it is not dispatched at all at
//! `stir_fraction = 0`, and why the branch is on the **host** rather than in this
//! file: a branch here would still pay for the dispatch, and the error would be
//! invisible in the result and visible only in the price.
//!
//! # It reads its input and writes a different buffer
//!
//! `dst = a_conv + noise`, never `dst += noise`. Adding in place is a read from
//! the buffer being written (`.claude/rules/kernels.md`), and the cost of not
//! doing it is one more `3*64^3*4 B = 3.1 MB` buffer — ADR-069 priced `A` and `u`
//! at 6.3 MB between them and did not count this one, which is a gap this file
//! records rather than papers over.
//!
//! # Why the lattice is periodic for nothing
//!
//! Octave `k` has a wavelength of `L_domain/2^k`, so its lattice has `2^(k+1)`
//! nodes per axis and a spacing of `extent >> (k+1)` cells. Every wavelength
//! divides the domain, so the field wraps in X and Y with no seam on the torus
//! and without a single line of code about it (ADR-069). The price is that the
//! extents have to be divisible: the host derives [`NoiseParams::n_octaves`] from
//! the extents it actually has and refuses the rest.
//!
//! The octave count runs from the domain down to a wavelength of four velocity
//! cells — five octaves at 64^3. Below four cells the lattice is the mesh, and a
//! stirring whose structure is one cell wide is the artefact ADR-069 rejected the
//! bare Laplacian for.
//!
//! # One stream per pair, and what a collision does
//!
//! The third counter of the draw is `NOISE_BASE + 3*octave + component`
//! (ADR-069), and the injectivity of that map is not tidiness. Two octaves of one
//! component sharing a `purpose` draw **the same number** at the lattice nodes
//! they share, the octaves become correlated, and the sum degenerates into a
//! single octave with a different multiplier — silently, with the whole corpus
//! green. `each_octave_and_component_draws_its_own_stream` is the only thing that
//! sees it.
//!
//! The fourth counter is the run key, and it is not optional: the three-counter
//! `rand` was revoked by ADR-058 precisely because a generator without it gives a
//! noise field identical under every seed, which fails
//! `different_seed_gives_different_state` in the one way that test cannot be
//! failed by anything else.

use crate::numeric::{M32, Q, q_conc_32, qadd, qdiv, qmul, qsub, rand};

/// The largest number of octaves a build supports.
///
/// A build-time bound in the manner of `N_MAX` (ADR-061) and `S_MAX`/`R_MAX`
/// (ADR-041), and it sizes the same kind of thing: the loop below is bounded, and
/// so is the slice of the `purpose` counter this kernel occupies —
/// `NOISE_BASE .. NOISE_BASE + 3*NOISE_OCTAVE_MAX`. Without a bound that slice is
/// open-ended and cannot be reserved against anything at all.
///
/// Eight is four octaves past what a 64^3 velocity grid can carry (five), and one
/// past what a 128^3 one could.
pub const NOISE_OCTAVE_MAX: u32 = 8;

/// The base of the `purpose` counter space the noise draws from.
// TODO(noise-base): nobody has decided this number. ADR-069 lays out the
// arithmetic `NOISE_BASE + 3*octave + component`, requires the pair to be
// injective, and sends the *value* to `OPEN_QUESTIONS.md` with its reason: there
// is no registry of `purpose` in the corpus, `reaction_id` is taken from the name
// of a reaction (ADR-027), and nothing keeps the two spaces apart. A collision
// between an octave and a reaction gives the two one stream — under a chemistry
// that looks entirely plausible.
//
// Taken here the same way `VELOCITY_FIELD_PROCESS` is taken in
// `config/validate.rs`: a constant with a TODO, not as knowledge. What the value
// buys is that the twenty-four counters this kernel uses are far from the small
// integers a hand-written `purpose` would pick, so a collision needs a reaction
// name whose hash lands in a 24-wide window. That is a probability, not a
// guarantee, and the guarantee is the registry that does not exist.
pub const NOISE_BASE: u32 = 0x4E4F_4953;

/// The `purpose` counter of one (octave, component) pair.
///
/// A `const fn` of two compile-time quantities and never a value derived from the
/// seed: ADR-058 rejected the variant that turns every `purpose` constant into a
/// run-time value, and the reason survives here — a `purpose` that depended on
/// the run would make `reaction_id_is_stable_under_reordering_in_toml` unable to
/// tell "taken from the position" from "taken from the seed".
#[inline(always)]
#[must_use]
pub const fn noise_purpose(octave: u32, component: u32) -> u32 {
    NOISE_BASE + 3 * octave + component
}

/// Parameters of the noise. Scalars only: in WGSL this is a uniform buffer.
#[derive(Clone, Copy, Debug)]
pub struct NoiseParams {
    /// Cells of the velocity grid along X.
    pub vnx: u32,
    /// Cells of the velocity grid along Y.
    pub vny: u32,
    /// Cells of the velocity grid along Z.
    pub vnz: u32,
    /// How many octaves to sum, at most [`NOISE_OCTAVE_MAX`]. Derived by the host
    /// from the extents: every octave from the domain down to four cells.
    pub n_octaves: u32,
    /// The tick number, the second counter of every draw. The field is a pure
    /// function of `(seed, tick, H)` and is therefore not part of a snapshot
    /// (ADR-037, ADR-069).
    pub tick: u32,
    /// The run key, the fourth counter, derived on the host from both halves of
    /// the seed (ADR-058).
    pub run_key: u32,
    /// The amplitude of the summed octaves, folded on the host so that the whole
    /// sum is bounded by the stirring share of the speed bound.
    ///
    /// The kernel weights octave `k` by `2^-k` and does not normalise, so the
    /// host divides by `sum_k 2^-k` before handing the number over — the usual
    /// division of labour, and the reason the kernel knows neither
    /// `stir_fraction` nor `u_conv_max`.
    // TODO(noise-spectrum): the *shape* of the spectrum is decided by nobody.
    // ADR-069 declares the overall estimate `(1 + stir_fraction)*u_conv_max` and
    // says nothing about how the noise potential is normalised for that
    // inequality to be true, so `stir_fraction` is today a fraction of something
    // the corpus has not named. `1/f` is the choice made in code here and in
    // `process/velocity.rs`, where the bound that follows from it is derived; a
    // different spectrum changes the flow and moves the world version.
    pub amplitude: Q,
    /// Non-zero: the floor is impermeable, so the tangential components are zeroed
    /// on the plane `z = 0` — the same branch `interpolate_potential` carries, and
    /// it has to be in **both**.
    ///
    /// This is the trap ADR-069's floor rule sets. Zeroed only in the
    /// interpolation, the floor holds at `stir_fraction = 0` and leaks the instant
    /// stirring is on: `A_x` and `A_y` come back on the plane, `w` is no longer
    /// zero there, the divergence is still an exact zero, the ledger still closes,
    /// and matter simply leaves through the floor and returns through the periodic
    /// lid.
    pub floor_is_closed: u32,
}

/// One cell of the velocity grid: the convective potential plus the noise.
///
/// `idx` runs over `0..vnx*vny*vnz`. Reads `a_conv` — its own cell only — and
/// writes the three components of its own cell of `dst`, which is a **different**
/// buffer.
pub fn stir_potential(a_conv: &[Q], dst: &mut [Q], p: &NoiseParams, idx: u32) {
    debug_assert!(
        p.n_octaves <= NOISE_OCTAVE_MAX,
        "{} octaves is past the build bound of NOISE_OCTAVE_MAX = {NOISE_OCTAVE_MAX}",
        p.n_octaves
    );

    let n_velocity = p.vnx * p.vny * p.vnz;
    let plane = p.vnx * p.vny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.vnx;
    let x = within_plane - y * p.vnx;

    // `1/2^31`, so that a `u32` draw read as an `i32` lands in `[-1, 1)`. Built
    // once per cell rather than per draw, and built rather than folded, because a
    // kernel has no constructor from a number (`numeric/float.rs`) and this one is
    // the same in both modes.
    let unit = negative_power_of_two(31);

    for component in 0..3u32 {
        let mut summed = Q::ZERO;
        let mut octave_weight = Q::ONE;

        for octave in 0..p.n_octaves {
            let purpose = noise_purpose(octave, component);
            summed = qadd(
                summed,
                qmul(
                    octave_weight,
                    lattice_value(p, x, y, z, octave, purpose, unit),
                ),
            );
            octave_weight = qdiv(octave_weight, qadd(Q::ONE, Q::ONE));
        }

        let tangential = component < 2;
        let on_the_floor = z == 0 && p.floor_is_closed != 0;
        dst[(component * n_velocity + idx) as usize] = if tangential && on_the_floor {
            // The floor, in the second of the two kernels that write `A`. See
            // `NoiseParams::floor_is_closed` for what happens when only the first
            // one has it.
            Q::ZERO
        } else {
            qadd(
                a_conv[(component * n_velocity + idx) as usize],
                qmul(p.amplitude, summed),
            )
        };
    }
}

/// One octave of noise at one cell: the eight lattice corners around it, faded
/// together.
///
/// Value noise rather than gradient noise, and the arithmetic of ADR-069 is what
/// says so: one draw per corner, eight corners, `3 * 5 * 8 = 120` draws per cell.
/// A gradient formulation needs three draws per corner and would price at 360.
///
/// The fade is `t*t*(3 - 2t)`, whose derivative vanishes at both ends, so the
/// summed potential is continuous in its first derivative and the curl of it is
/// continuous. Plain linear interpolation would leave `u` discontinuous across
/// every lattice plane — visible as a grid of sheets in the flow, and not visible
/// in any invariant at all.
#[inline(always)]
fn lattice_value(p: &NoiseParams, x: u32, y: u32, z: u32, octave: u32, purpose: u32, unit: Q) -> Q {
    // Octave 0 has two nodes per axis, that is one wavelength across the domain.
    // One node per axis would be a constant field, which is what a lattice of
    // `2^k` nodes gives at `k = 0` — the off-by-one that costs the coarsest octave
    // and is invisible, because the remaining octaves still look like noise.
    let spacing_x = p.vnx >> (octave + 1);
    let spacing_y = p.vny >> (octave + 1);
    let spacing_z = p.vnz >> (octave + 1);
    debug_assert!(
        spacing_x >= 2 && spacing_y >= 2 && spacing_z >= 2,
        "octave {octave} has a lattice spacing under two cells on a {}x{}x{} grid",
        p.vnx,
        p.vny,
        p.vnz
    );

    let nodes_x = p.vnx / spacing_x;
    let nodes_y = p.vny / spacing_y;
    let nodes_z = p.vnz / spacing_z;

    let jx = x / spacing_x;
    let jy = y / spacing_y;
    let jz = z / spacing_z;

    // The cell centre sits at `(rem + 0.5)/spacing` inside its lattice cell, which
    // over the denominator `2*spacing` is the integer `2*rem + 1`. `spacing` is a
    // power of two by construction, so the denominator is one too and the fraction
    // is exact.
    let fade_x = fade(unit_fraction(
        2 * (x - jx * spacing_x) + 1,
        (2 * spacing_x).trailing_zeros(),
    ));
    let fade_y = fade(unit_fraction(
        2 * (y - jy * spacing_y) + 1,
        (2 * spacing_y).trailing_zeros(),
    ));
    let fade_z = fade(unit_fraction(
        2 * (z - jz * spacing_z) + 1,
        (2 * spacing_z).trailing_zeros(),
    ));

    let mut value = Q::ZERO;
    for corner in 0..8u32 {
        // `& (nodes - 1)` is the modulo of a power of two, and it is where the
        // lattice wraps: every octave's wavelength divides the domain, so the
        // torus closes with no seam and no branch (ADR-069).
        let cx = if corner & 1 == 1 {
            (jx + 1) & (nodes_x - 1)
        } else {
            jx
        };
        let cy = if corner & 2 == 2 {
            (jy + 1) & (nodes_y - 1)
        } else {
            jy
        };
        let cz = if corner & 4 == 4 {
            (jz + 1) & (nodes_z - 1)
        } else {
            jz
        };

        let node = cx + cy * nodes_x + cz * nodes_x * nodes_y;
        let draw = rand(node, p.tick, purpose, p.run_key);
        // The one crossing from an integer into `Q` a kernel is allowed
        // (ADR-060): `q_conc` with the per-unit factor folded, which here is
        // `2^-31`. Read as an `i32`, a uniform `u32` is a uniform signed value.
        let corner_value = q_conc_32(M32::new(draw as i32), unit);

        let weight_x = if corner & 1 == 1 {
            fade_x
        } else {
            qsub(Q::ONE, fade_x)
        };
        let weight_y = if corner & 2 == 2 {
            fade_y
        } else {
            qsub(Q::ONE, fade_y)
        };
        let weight_z = if corner & 4 == 4 {
            fade_z
        } else {
            qsub(Q::ONE, fade_z)
        };

        value = qadd(
            value,
            qmul(qmul(qmul(weight_x, weight_y), weight_z), corner_value),
        );
    }

    value
}

/// The smoothstep fade `t*t*(3 - 2*t)`.
#[inline(always)]
fn fade(t: Q) -> Q {
    let two = qadd(Q::ONE, Q::ONE);
    let three = qadd(two, Q::ONE);
    qmul(qmul(t, t), qsub(three, qmul(two, t)))
}

/// `2^-exponent`, exactly, by repeated halving.
///
/// A `Q` out of an integer without `Q::from_f64`, which is a host-side call and
/// would put an `f64` inside a shader (`numeric/float.rs`).
#[inline(always)]
fn negative_power_of_two(exponent: u32) -> Q {
    let two = qadd(Q::ONE, Q::ONE);
    let mut unit = Q::ONE;
    for _ in 0..exponent {
        unit = qdiv(unit, two);
    }
    unit
}

/// `numerator * 2^-log2_denominator`, exactly. The twin of the function of the
/// same name in `kernels/potential.rs`; each kernel carries its own, by the rule
/// that `kernels/` files do not depend on one another.
#[inline(always)]
fn unit_fraction(numerator: u32, log2_denominator: u32) -> Q {
    let unit = negative_power_of_two(log2_denominator);
    let mut value = Q::ZERO;
    for _ in 0..numerator {
        value = qadd(value, unit);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const VNX: u32 = 16;
    const VNY: u32 = 16;
    const VNZ: u32 = 8;
    const N_VELOCITY: u32 = VNX * VNY * VNZ;
    /// From the domain down to four cells on the shortest axis: `8 >> (k+1) >= 2`
    /// gives `k` in `0..2`.
    const OCTAVES: u32 = 2;

    fn params(floor_is_closed: u32, tick: u32, run_key: u32) -> NoiseParams {
        NoiseParams {
            vnx: VNX,
            vny: VNY,
            vnz: VNZ,
            n_octaves: OCTAVES,
            tick,
            run_key,
            amplitude: Q::ONE,
            floor_is_closed,
        }
    }

    fn dispatch(a_conv: &[Q], p: &NoiseParams) -> Vec<Q> {
        let mut dst = vec![Q::ZERO; (3 * N_VELOCITY) as usize];
        for idx in 0..N_VELOCITY {
            stir_potential(a_conv, &mut dst, p, idx);
        }
        dst
    }

    fn zero_potential() -> Vec<Q> {
        vec![Q::ZERO; (3 * N_VELOCITY) as usize]
    }

    #[test]
    fn each_octave_and_component_draws_its_own_stream() {
        // Named by ADR-069 as a silent failure: two octaves of one component in
        // one `purpose` draw the same number at every lattice node they share, the
        // octaves correlate, and the sum degenerates into a single octave with a
        // different multiplier — with the whole corpus green.
        //
        // Every pair the build can produce, not only the five a 64^3 grid uses.
        let mut seen = BTreeSet::new();
        for octave in 0..NOISE_OCTAVE_MAX {
            for component in 0..3u32 {
                seen.insert(noise_purpose(octave, component));
            }
        }
        assert_eq!(seen.len(), (3 * NOISE_OCTAVE_MAX) as usize);
        // And the space is contiguous, which is what lets it be reserved against
        // the `reaction_id` space the day a registry of `purpose` exists.
        assert_eq!(*seen.iter().next().unwrap(), NOISE_BASE);
        assert_eq!(
            *seen.iter().next_back().unwrap(),
            NOISE_BASE + 3 * NOISE_OCTAVE_MAX - 1
        );
    }

    #[test]
    fn the_noise_changes_with_the_run_key_and_with_the_tick() {
        // Without the fourth counter the field would be the same under every seed
        // (ADR-058), and `different_seed_gives_different_state` would have nothing
        // to fail on. Without the second it would be frozen in time, which is what
        // `stir_period` is supposed to be about.
        let a_conv = zero_potential();
        let base = dispatch(&a_conv, &params(0, 0, 1));
        let other_seed = dispatch(&a_conv, &params(0, 0, 2));
        let other_tick = dispatch(&a_conv, &params(0, 1, 1));

        assert_ne!(base, other_seed);
        assert_ne!(base, other_tick);
        assert_ne!(other_seed, other_tick);
    }

    #[test]
    fn the_noise_is_added_to_the_convective_potential_and_never_in_place() {
        // `dst = a_conv + noise`. The input buffer is untouched, and the output is
        // the sum — an in-place `+=` would be a read from the buffer being written
        // and would double the noise on the second tick.
        let mut a_conv = zero_potential();
        for value in a_conv.iter_mut() {
            *value = Q::from_f64(0.25);
        }
        let before = a_conv.clone();
        let p = params(0, 3, 7);
        let stirred = dispatch(&a_conv, &p);

        assert_eq!(a_conv, before, "the input buffer moved");

        // The noise alone, taken over a zero convective potential, plus the
        // convective potential, is the stirred field exactly.
        let alone = dispatch(&zero_potential(), &p);
        for at in 0..stirred.len() {
            assert_eq!(stirred[at], qadd(before[at], alone[at]), "cell {at}");
        }
    }

    #[test]
    fn the_summed_octaves_stay_inside_the_declared_amplitude() {
        // The bound `process/velocity.rs` derives the noise amplitude from: the
        // kernel weights octave `k` by `2^-k` and the host divides the amplitude
        // by the sum of those weights, so the whole potential is inside
        // `amplitude` whatever the draws are.
        let p = params(0, 11, 13);
        let field = dispatch(&zero_potential(), &p);
        let ceiling: f64 = (0..OCTAVES).map(|k| 0.5f64.powi(k as i32)).sum();
        for (at, value) in field.iter().enumerate() {
            assert!(
                value.debug_f64().abs() <= ceiling,
                "cell {at} is {} over a ceiling of {ceiling}",
                value.debug_f64()
            );
        }
        // And it is not a field of zeroes.
        assert!(field.iter().any(|v| v.debug_f64().abs() > 0.05));
    }

    #[test]
    fn the_floor_carries_no_tangential_potential_with_stirring_on() {
        // The second half of the floor branch, and the one that is easy to leave
        // out: zeroed only in the interpolation, the floor holds at
        // `stir_fraction = 0` and leaks the instant stirring is on, with the
        // divergence still an exact zero and the ledger still closed.
        let p = params(1, 5, 9);
        let a_conv = vec![Q::from_f64(1.0); (3 * N_VELOCITY) as usize];
        let field = dispatch(&a_conv, &p);

        for vy in 0..VNY {
            for vx in 0..VNX {
                let at = (vx + vy * VNX) as usize;
                assert_eq!(field[at], Q::ZERO, "A_x at ({vx}, {vy}, 0)");
                assert_eq!(field[N_VELOCITY as usize + at], Q::ZERO);
            }
        }
        // One plane up the noise is there, or the assertion above holds over a
        // field of zeroes.
        let above = (VNX * VNY) as usize;
        assert_ne!(field[above], Q::ZERO);
    }

    #[test]
    fn the_lattice_wraps_without_a_seam_in_x_and_y() {
        // Every octave's wavelength divides the domain, so the field is periodic
        // in X and Y for nothing (ADR-069). Checked as smoothness across the wrap:
        // the step from the last column to the first is no larger than the largest
        // step anywhere inside the row.
        let p = params(0, 2, 3);
        let field = dispatch(&zero_potential(), &p);

        let row = |z: u32, y: u32| -> Vec<f64> {
            (0..VNX)
                .map(|x| field[(x + y * VNX + z * VNX * VNY) as usize].debug_f64())
                .collect()
        };

        for z in [1u32, 3] {
            for y in [0u32, 5] {
                let values = row(z, y);
                let inside = (1..VNX as usize)
                    .map(|x| (values[x] - values[x - 1]).abs())
                    .fold(0.0f64, f64::max);
                let seam = (values[0] - values[VNX as usize - 1]).abs();
                assert!(
                    seam <= inside * 1.5 + 1e-6,
                    "a seam of {seam} against the largest interior step {inside} \
                     at (z = {z}, y = {y})"
                );
            }
        }
    }
}
