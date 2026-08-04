//! Settling of one substance: directed transport along **one** axis, Z.
//!
//! Step `f` of the tick order (SPEC section 8), and the first operator in the
//! project that moves matter in a direction rather than down a gradient. Grains
//! denser than the medium sink, lighter ones rise, and both come out of the same
//! formula with opposite signs (ADR-067).
//!
//! # What is folded into `courant`, because the reader will reconstruct it
//!
//! The Stokes velocity is
//!
//! ```text
//! w = k*(rho_grain - rho_medium)*g/mu,   k = 2*r^2/9
//! ```
//!
//! with `r` the **radius** of the grain. `k = d^2/18` is the same coefficient
//! written through the diameter; `k = r^2/18` is the diameter form with a radius
//! inside it and gives a velocity exactly four times too slow — a factor that
//! survives calibration of the sedimentation rate unnoticed forever, which is
//! why ADR-067 fixed both the formula and the fact that `r` is a radius.
//!
//! None of that is in this file. The host derives `w` once at load time, divides
//! by `dx/dt` and hands the kernel a single signed number, the way `alpha`
//! arrives at the diffusion kernel: radius, densities, viscosity, `g`, `dt` and
//! `dx` are not knowable here and must not become knowable (ADR-034, ADR-015).
//! The paragraph exists so that whoever needs the formula reads it here instead
//! of reconstructing it, because the reconstruction has a wrong branch.
//!
//! # One axis, and what that changes
//!
//! Transport runs along Z only, so a voxel has exactly one outgoing face. The
//! sum over outgoing faces in the positivity condition of SPEC section 4.2
//! degenerates to a single term and both inequalities of that section coincide;
//! settling is checked at load time by its own condition `w*dt/dx <= 1` rather
//! than out of somebody else's budget, because Lie-Trotter splitting applies
//! every operator over the full step (ADR-036, ADR-067). There are no substeps
//! here: ADR-030 counts them out of the *parabolic* condition `6*D*dt/dx^2`, and
//! `N_MAX = 64` (ADR-061) stays the bound of one diffusion.
//!
//! Two consequences for whoever calls this kernel, and neither is enforceable
//! from inside it:
//!
//! - the host must compare `params.courant` — the `Q`, not the `f64` it came
//!   from — against one. ADR-068 showed this on a live constant:
//!   `Q::from_f64(1/6)` rounds **up**, so `6*alpha` is `1 + 2^-25`. Above one the
//!   donor cell gives away more than it holds, the undershoot bound stops being
//!   derivable, and negative amounts arrive by construction rather than by
//!   rounding;
//! - the kernel cannot know it is being run once every `n` ticks. That would
//!   multiply the effective step and falsify a condition already checked at
//!   load. ADR-005 and ADR-030 forbid `every_n_ticks > 1` for a diffusive field
//!   for exactly this reason; whether the ban extends to a hyperbolic operator
//!   is not settled anywhere, and this comment is the only place that says so.
//!
//! # Conservation, and why the flux is not antisymmetric in the diffusive sense
//!
//! Gather form: a voxel sums the flux across its two Z faces and writes only its
//! own cell (ADR-034). Conservation is the property that the two sides of a face
//! apply **one number** with opposite signs (ADR-005) — and for a directed flux
//! that is not the same statement as `flux(a, b) == -flux(b, a)`, which is false
//! here and must stay false: the donor is chosen by the sign of the courant
//! number, not by which side is fuller, so `flux(a, b) = c*a` and
//! `flux(b, a) = c*b`. What makes the two sides agree is that both evaluate one
//! expression over one four-cell tuple with the operands in one order —
//! the canonical orientation of ADR-054, from the smaller linear index to the
//! larger. Compute the flux "from your own side" and `Q` arithmetic diverges in
//! the last bit; matter then leaks one unit per face, silently, exactly the way
//! `floor` leaks it in a diffusive flux.
//!
//! The rounding rule is therefore *not* load-bearing for conservation here, and
//! that inversion is worth stating — together with the sentence that follows
//! from it and is false. Both voxels apply the same rounded number, so `floor`
//! would conserve perfectly; it would **not** merely make settling slower. A
//! positive flux points toward `+Z` and `floor(F) <= F`, so `floor` biases every
//! face toward `-Z` whatever the sign of the courant: a sinking grain would
//! settle up to one unit per face per tick *faster*, only a buoyant one slower,
//! and a still column would creep downward everywhere `|c*a| < 1`. That is a
//! systematic drift in the one direction settling already moves, which is the
//! worst place for it to be, and no conservation test, no per-face test and no
//! undershoot bound can see it. The tuple is what conservation rests on; the
//! rounding rule is what the direction of the physics rests on, and this file
//! has no sentry for the second — `the_flux_mirrors_when_the_axis_is_reversed`
//! is the closest thing to one.
//!
//! # What is not here
//!
//! Channel counters. Matter leaving through `z_max = exchange` belongs to
//! `BOUNDARY_EXCHANGE` (ADR-067, ADR-059), `periodic_mask` encodes only two
//! boundary states, and the host is required to refuse a grid whose Z face is
//! `exchange` — the same refusal `diffuse.rs` already records. A kernel that
//! quietly treated the unencoded face as closed would pile every buoyant grain
//! under the lid and the ledger would still balance, because nothing was lost.
//!
//! Enthalpy. Settling does not carry enthalpy with matter, and in that it is not
//! an exception: no transport process in the project does (ADR-067, ADR-062).
//! This kernel touches one lane of amounts and reads no energy field.

use crate::numeric::{
    M32, M64, Q, q_conc_32, q_conc_64, q_round_32, q_round_64, qadd, qdiv, qmul, qsub,
};

/// Parameters of one application. Scalars only: in WGSL this is a uniform
/// buffer, and every number in it is a place where the host's scale can drift
/// away from the kernel's (`ARCHITECTURE.md`).
///
/// The same `Copy` struct of scalars as `DiffuseParams`, field for field
/// (ADR-067, ADR-034, ADR-015).
#[derive(Clone, Copy, Debug)]
pub struct SettleParams {
    /// Voxels along X.
    pub nx: u32,
    /// Voxels along Y.
    pub ny: u32,
    /// Voxels along Z.
    pub nz: u32,
    /// Bit `f` set: face `f` is periodic and wraps. Bit clear: the face is
    /// closed and carries nothing.
    ///
    /// The bit index is the discriminant of `world::Face`: `0 = x_min`,
    /// `1 = x_max`, `2 = y_min`, `3 = y_max`, `4 = z_min`, `5 = z_max`. The
    /// whole layout is kept even though this kernel reads only bits 4 and 5, so
    /// that a host folding one mask for every transport kernel does not have to
    /// remember which of them re-numbered the faces.
    ///
    /// Unlike in diffusion, a closed face here needs a **branch** rather than a
    /// self-referencing neighbour: see [`z_face_is_open`].
    pub periodic_mask: u32,
    /// `w*dt/dx` for the **whole tick**, signed, folded on the host.
    ///
    /// **Positive means toward `+Z`.** The host knows `w` as positive
    /// *downward* — a grain denser than the medium sinks — so exactly one
    /// negation lies between that number and this field, and it is silent.
    /// Invert it and mineral floats out through the lid while the floor stays
    /// clean, with conservation, the per-face agreement, the undershoot bound
    /// and both storage widths all green. `a_dense_substance_moves_toward_z_min`
    /// is the only thing in this file that would notice.
    ///
    /// `|courant| <= 1` is the load-time condition of ADR-067 and is neither
    /// checked nor known here; see the note on the module about comparing it as
    /// a `Q`.
    pub courant: Q,
}

/// Generates one storage width of the settling kernel.
///
/// Invoked twice, immediately below. ADR-040 derives the storage width of an
/// amount from its declared concentrations, so both widths have to exist, and
/// `NUMERIC.md` section 5 already settled how a two-width axis is expressed:
/// generate both from one text, as `define_diffuse!` and `define_conversions!`
/// do. Two hand-written copies drift apart within a month, and the drift
/// surfaces as "the 64-bit substance somehow behaves differently".
macro_rules! define_settle {
    ($voxel:ident, $flux:ident, $slope:ident, $m:ty, $q_conc:ident, $q_round:ident) => {
        #[doc = concat!("The van Leer limited slope, in `", stringify!($m), "`.")]
        ///
        /// `2*du*df/(du + df)` when `du` and `df` have **strictly** the same
        /// sign, and `Q::ZERO` otherwise. Both differences arrive already taken
        /// in integers and already oriented along the flow: `df` is
        /// `acceptor - donor` across the face, `du` is `donor - upwind` behind
        /// it.
        ///
        /// # This is `advect.rs` line for line, and it is a copy on purpose
        ///
        /// ADR-067 takes the limiter whole from ADR-054, so settling and
        /// advection are not merely the same *number* — they have to be the same
        /// *program*, or a substance that both advects and settles gets two
        /// limiters that round differently in `FIXED` and disagree on the sign
        /// of a face. The reasoning for every line of it — why the harmonic form
        /// and not `psi(r)*df`, why the branch stands before any division, why
        /// the differences are taken in `M` — is written out once, on
        /// `advect::limited_slope_32`, and is not repeated here.
        ///
        /// The copy follows the precedent this directory already set for the
        /// neighbourhood lookup: two texts held together by a test rather than
        /// one text shared across kernels, because a kernel becomes a WGSL
        /// entry point and reaching across into another one is a dependency the
        /// shader has to carry. `the_limiter_is_advections_limiter` is the
        /// sentry, and drift between the two is caught there and nowhere else.
        ///
        /// Public for the reason `advect.rs` gives: a test that could see only
        /// the flux would pass on a limiter stuck at zero everywhere, and five
        /// separate mutations of an inlined limiter once did exactly that.
        #[inline(always)]
        pub fn $slope(upwind: $m, face: $m) -> Q {
            // Strictly the same sign, in exact integers. "Strictly" excludes
            // both zeros — the undefined ratio and the extremum — where van Leer
            // is zero anyway.
            let rising = upwind > <$m>::ZERO && face > <$m>::ZERO;
            let falling = upwind < <$m>::ZERO && face < <$m>::ZERO;
            if !(rising || falling) {
                return Q::ZERO;
            }

            let du = $q_conc(upwind, Q::ONE);
            let df = $q_conc(face, Q::ONE);
            // `2 * (du*df/(du+df))` rather than `(2*du*df)/(du+df)`: doubling is
            // exact in every mode, and this keeps the largest intermediate at
            // `du*df`. The association is the one `advect.rs` chose, and it has
            // to stay the one `advect.rs` chose.
            let harmonic = qdiv(qmul(du, df), qadd(du, df));
            qadd(harmonic, harmonic)
        }

        #[doc = concat!("Settling flux across one Z face, in `", stringify!($m), "`.")]
        ///
        /// The signed amount that crosses **from `low` to `high`**, that is,
        /// toward `+Z`. `outer_low` is the cell one step below `low`,
        /// `outer_high` one step above `high`: the limiter looks one cell
        /// upstream of the donor, so the stencil of a face is four cells wide
        /// (ADR-054).
        ///
        /// The name is `flux` rather than `settle_flux` because `kernels/mod.rs`
        /// already declared that every transport kernel has a function called
        /// `flux` and that the module is what tells them apart.
        ///
        /// # The scheme
        ///
        /// Donor-acceptor upwinding with a van Leer limiter on top of it — the
        /// same form ADR-005 requires and ADR-054 sharpens, taken whole:
        ///
        /// ```text
        /// df = acceptor - donor      (in M)
        /// du = donor - upwind        (in M)
        /// F  = round( c*conc(donor) + c*(1 - |c|)/2 * limited_slope(du, df) )
        /// ```
        ///
        /// `psi(r)*df` with `r = du/df` is the same number and a different
        /// program; [`limited_slope_32`] says which one this is and why.
        ///
        /// The donor is picked by the **sign of the courant number** and never
        /// by comparing the two amounts: this is directed transport, not
        /// diffusion, and a donor chosen locally would make the two sides of a
        /// face disagree the moment the profile is not monotone.
        ///
        /// First order alone would be cheaper and is not enough: the numerical
        /// diffusion of a donor scheme is `D_num = (w*dx/2)*(1 - C)`, and at
        /// `dx = 100 um` that is the same order as the molecular diffusion the
        /// model resolves — the argument of ADR-054, word for word, with `w` in
        /// place of `u`.
        ///
        /// # Where the classes cross
        ///
        /// Once, at the end, through the two named crossings composed
        /// (ADR-060): `q_conc` down with a unit factor, `q_round` up. And the
        /// order inside matters — the differences of the stencil are taken in
        /// `M`, exactly, and cross into `Q` already as differences; only the
        /// donor amount itself crosses as an amount. Taking the differences in
        /// `Q` would lose everything below one ulp of the pool, which for water
        /// at `5.1e11` units is 32768 units.
        ///
        /// The unavoidable cost of that asymmetry is worth naming: the donor
        /// term is the *pool*, and in a large pool the antidiffusive correction
        /// `c*(1 - |c|)/2 * limited_slope(du, df)` can fall below its ulp and
        /// vanish. The scheme then degrades silently to first order — back to
        /// the numerical diffusion ADR-054 was adopted to remove. No
        /// conservation test, no per-face test and no undershoot bound can see
        /// that happen, which is why the correction has three sentries of its
        /// own: `the_antidiffusive_correction_is_the_van_leer_one`,
        /// `the_limiter_stays_inside_the_tvd_region` and
        /// `the_flux_mirrors_when_the_axis_is_reversed`.
        ///
        /// # Antisymmetry: read the module comment before "fixing" this
        ///
        /// `flux(a, b) == -flux(b, a)` is **false** here and has to be. What
        /// conservation rests on is that the two voxels sharing a face call this
        /// function with the identical four-cell tuple, so they get the
        /// identical number back; `flux_is_antisymmetric` states that and
        /// nothing weaker.
        ///
        /// The mirror image of the diffusive statement does hold, bit for bit,
        /// and it is the one property of this function that sees the *direction*
        /// of transport:
        ///
        /// ```text
        /// flux(ll, l, h, hh, c) == -flux(hh, h, l, ll, -c)
        /// ```
        ///
        /// because the mirrored call picks the same donor, the same upwind cell
        /// and therefore the same two differences — only the sign of every
        /// multiplication by `c` changes, and negation is exact in IEEE-754 and
        /// in fixed point alike. It is what a limiter reaching downwind breaks,
        /// what `(1 - c)` written for `(1 - |c|)` breaks, and what `floor`
        /// breaks.
        pub fn $flux(outer_low: $m, low: $m, high: $m, outer_high: $m, courant: Q) -> $m {
            let upward = courant >= Q::ZERO;
            let donor = if upward { low } else { high };
            let acceptor = if upward { high } else { low };
            let upwind = if upward { outer_low } else { outer_high };

            // Exact integer differences (ADR-060), and the flat gradient is
            // handled by the limiter rather than by a branch here: `df == 0` is
            // not exotic — it is the initial condition of half the scenarios —
            // and [`limited_slope_32`] answers `Q::ZERO` for it without
            // dividing. `du == 0` is the same statement one cell further back,
            // which is what a face against a wall looks like: the outer cell is
            // the boundary cell itself, so both sides fall back to first order,
            // and that is the rule ADR-054 gives for an undefined `r` arrived at
            // rather than chosen.
            let df = acceptor - donor;
            let du = donor - upwind;

            // `|c|` by comparison and `qsub(ZERO, c)`: `qabs` is not one of the
            // ten wrappers of NUMERIC.md section 2, and comparisons are
            // explicitly not arithmetic.
            let magnitude = if upward {
                courant
            } else {
                qsub(Q::ZERO, courant)
            };
            // `c*(1 - |c|)/2`, and the halving is a division by an exact two
            // rather than a multiplication by a literal: building a `Q` out of a
            // host-side number is the host's job, and a kernel that converts is
            // a kernel that could convert differently from the host.
            let antidiffusive = qdiv(qmul(courant, qsub(Q::ONE, magnitude)), qadd(Q::ONE, Q::ONE));

            $q_round(qadd(
                qmul(courant, $q_conc(donor, Q::ONE)),
                qmul(antidiffusive, $slope(du, df)),
            ))
        }

        #[doc = concat!("One application of settling in one voxel, in `", stringify!($m), "`.")]
        ///
        /// Reads `src`, writes `dst[idx]` and nowhere else. The result does not
        /// depend on the order the kernel is called in for different `idx`,
        /// because nothing any voxel writes is visible to any other: `src` is
        /// state `N`, `dst` is state `N+1`, and they are different buffers
        /// (ADR-034).
        ///
        /// The five-cell stencil along Z makes one shortcut especially tempting
        /// and it is forbidden: the flux through this voxel's bottom face has
        /// already been computed by the voxel below as its top face. Taking it
        /// ready-made is either a read from the buffer being written or a
        /// sequential dependency — correct on a forward CPU sweep, different on
        /// a backward one, undefined on a GPU where there is no order at all.
        ///
        /// Every voxel of the lane must be visited before the buffers are
        /// swapped: a skipped voxel is not left unchanged, it is left a tick
        /// stale.
        ///
        /// # Amounts may go negative, and this kernel will not stop them
        ///
        /// Each open face rounds its flux to a whole unit, so a voxel can be
        /// overdrawn by up to half a unit per face. ADR-068 bounds it:
        /// `new >= min(stencil) - floor(f/2) - 3*ceil(spread/2^24)` with `f` the
        /// number of open faces. One axis means `f <= 2` and an interior floor
        /// of `-1`, not the `-3` of the seven-point stencil; a voxel on a closed
        /// floor has `f = 1` and a floor of zero.
        ///
        /// Clamping is not an option and not an oversight: the neighbour has
        /// already been given the matter in the same application, so `max(0, .)`
        /// over the result would print units and break the exact equality of
        /// sums that the whole scheme is built on (ADR-068).
        pub fn $voxel(src: &[$m], dst: &mut [$m], p: &SettleParams, idx: u32) {
            let here = src[idx as usize];
            let mut net = <$m>::ZERO;

            // The bottom face, in canonical orientation: this voxel is the
            // `high` side of it. `above` is the outer cell on the far side of
            // the face — taken by stepping up from here, which is the same
            // expression the voxel below evaluates for its own top face.
            if z_face_is_open(p, idx, 4) {
                let below = z_neighbour(p, idx, 4);
                let below2 = z_neighbour(p, below, 4);
                let above = z_neighbour(p, idx, 5);
                net += $flux(
                    src[below2 as usize],
                    src[below as usize],
                    here,
                    src[above as usize],
                    p.courant,
                );
            }

            // The top face: this voxel is the `low` side, and the flux it
            // carries leaves.
            if z_face_is_open(p, idx, 5) {
                let above = z_neighbour(p, idx, 5);
                let above2 = z_neighbour(p, above, 5);
                let below = z_neighbour(p, idx, 4);
                net -= $flux(
                    src[below as usize],
                    here,
                    src[above as usize],
                    src[above2 as usize],
                    p.courant,
                );
            }

            dst[idx as usize] = here + net;
        }
    };
}

define_settle!(
    settle_voxel_32,
    flux_32,
    limited_slope_32,
    M32,
    q_conc_32,
    q_round_32
);
define_settle!(
    settle_voxel_64,
    flux_64,
    limited_slope_64,
    M64,
    q_conc_64,
    q_round_64
);

/// The linear index of a voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
#[inline(always)]
fn index(p: &SettleParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

/// One step down an axis, or what the boundary says instead.
#[inline(always)]
fn step_down(coord: u32, extent: u32, periodic: bool) -> u32 {
    if coord > 0 {
        coord - 1
    } else if periodic {
        extent - 1
    } else {
        coord
    }
}

/// One step up an axis, or what the boundary says instead.
#[inline(always)]
fn step_up(coord: u32, extent: u32, periodic: bool) -> u32 {
    if coord + 1 < extent {
        coord + 1
    } else if periodic {
        0
    } else {
        coord
    }
}

/// The neighbour of a voxel across face 4 (`z_min`) or face 5 (`z_max`).
#[inline(always)]
fn z_neighbour(p: &SettleParams, idx: u32, face: u32) -> u32 {
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    let periodic = (p.periodic_mask >> face) & 1 == 1;
    let nbz = if face == 4 {
        step_down(z, p.nz, periodic)
    } else {
        step_up(z, p.nz, periodic)
    };

    index(p, x, y, nbz)
}

/// Is the Z face of this voxel a face matter may cross at all?
#[inline(always)]
fn z_face_is_open(p: &SettleParams, idx: u32, face: u32) -> bool {
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let periodic = (p.periodic_mask >> face) & 1 == 1;
    if face == 4 {
        z > 0 || periodic
    } else {
        z + 1 < p.nz || periodic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, Face, Grid};

    /// A deliberately non-cubic grid: on a cubic one all three strides are
    /// equal, and a kernel that walked X while claiming to walk Z would pass
    /// every test in this file.
    const NX: u32 = 3;
    const NY: u32 = 4;
    const NZ: u32 = 5;
    const N_VOXELS: u32 = NX * NY * NZ;

    /// All six faces periodic.
    const TORUS: u32 = 0b11_1111;
    /// The eco-regime default of SPEC section 1.6, minus the face that cannot be
    /// encoded yet: periodic in X and Y, solid floor and lid in Z.
    const FLOORED: u32 = 0b00_1111;
    /// Every face closed.
    const CLOSED: u32 = 0;

    /// The courants the sweeps walk: zero, both signs, and the endpoint
    /// `|c| = 1` where the antidiffusive term vanishes identically.
    ///
    /// `+-0.6` is not decoration and not a round number: the outgoing flux of a
    /// voxel is `|c|*H + |c|(1 - |c|)/2 * slope` with `slope <= 2*H`, so a
    /// limiter twice too strong asks for `|c|(3 - 2|c|)*H`, and the worst of
    /// that sits at `|c| = 0.6`, where it wants `1.125*H` out of a pool of `H`.
    /// Endpoints and halves say nothing about the interior of that curve.
    const COURANTS: [f64; 11] = [0.0, 0.1, -0.1, 0.5, -0.5, 0.6, -0.6, 0.75, -0.75, 1.0, -1.0];

    fn params(periodic_mask: u32, courant: f64) -> SettleParams {
        SettleParams {
            nx: NX,
            ny: NY,
            nz: NZ,
            periodic_mask,
            courant: Q::from_f64(courant),
        }
    }

    /// A profile that varies **along Z** and is not symmetric in the plane.
    ///
    /// Vertical inhomogeneity is not decoration: see the comment on
    /// `sedimentation_conserves_exactly` for the two degeneracies that make a
    /// broken kernel look conservative.
    ///
    /// It is monotone in `z`, and that is a limitation rather than a feature:
    /// on monotone data the limiter is smooth and never switches off, so this
    /// profile can say nothing about an extremum. [`step_pattern`] is the other
    /// half, and the two are used together everywhere the limiter is at issue.
    fn pattern(idx: u32) -> i64 {
        let z = i64::from(idx / (NX * NY));
        let rest = i64::from(idx % (NX * NY));
        z * 1301 + (rest * 7919) % 97 + 40
    }

    /// The profile the limiter is dangerous on: a thin layer standing over an
    /// empty voxel with a thick bed underneath it, and a strict local maximum
    /// two cells up.
    ///
    /// Every number in it is chosen against a term of the scheme rather than
    /// picked to look irregular.
    ///
    /// - `z = 1` holds `1000` with `0` above and four million below, so the
    ///   two differences of the face beneath it are both positive and of very
    ///   different size: the harmonic mean saturates at `2*du = 2*H`, which is
    ///   the largest antidiffusive correction a pool of `H` can generate. This
    ///   is the voxel a limiter twice too strong overdraws (ADR-068);
    /// - `z = 3` holds two million between two empty cells: a strict local
    ///   maximum, the only place ADR-068 says undershoot is possible at all,
    ///   and the place a TVD limiter is obliged to switch itself off;
    /// - the whole spread stays far below `2^24`, where the `f32` term of the
    ///   ADR-068 bound is arithmetically zero rather than merely small, so the
    ///   assertions are about the scheme and not about `f32`.
    fn step_pattern(idx: u32) -> i64 {
        let z = idx / (NX * NY);
        let rest = i64::from(idx % (NX * NY));
        match z {
            0 => 4_000_000 + rest,
            1 => 1_000 + rest % 7,
            3 => 2_000_000,
            _ => 0,
        }
    }

    fn profile_32() -> Vec<M32> {
        (0..N_VOXELS).map(|i| M32::new(pattern(i) as i32)).collect()
    }

    fn step_profile_32() -> Vec<M32> {
        (0..N_VOXELS)
            .map(|i| M32::new(step_pattern(i) as i32))
            .collect()
    }

    fn step_profile_64() -> Vec<M64> {
        (0..N_VOXELS).map(|i| M64::new(step_pattern(i))).collect()
    }

    fn profile_64() -> Vec<M64> {
        (0..N_VOXELS).map(|i| M64::new(pattern(i))).collect()
    }

    fn total_32(buffer: &[M32]) -> i64 {
        buffer.iter().map(|v| v.to_i64()).sum()
    }

    fn total_64(buffer: &[M64]) -> i64 {
        buffer.iter().map(|v| v.to_i64()).sum()
    }

    /// One application over the whole lane, the way the host runs it.
    fn apply_32(src: &[M32], p: &SettleParams) -> Vec<M32> {
        let mut dst = vec![M32::ZERO; src.len()];
        for idx in 0..src.len() as u32 {
            settle_voxel_32(src, &mut dst, p, idx);
        }
        dst
    }

    fn apply_64(src: &[M64], p: &SettleParams) -> Vec<M64> {
        let mut dst = vec![M64::ZERO; src.len()];
        for idx in 0..src.len() as u32 {
            settle_voxel_64(src, &mut dst, p, idx);
        }
        dst
    }

    /// The flux through the **bottom** face of `idx`, assembled from the four
    /// cells of the stencil in canonical orientation: from the smaller linear
    /// index to the larger (ADR-054).
    fn flux_through_bottom_face(src: &[M32], p: &SettleParams, idx: u32) -> M32 {
        let below = z_neighbour(p, idx, 4);
        let below2 = z_neighbour(p, below, 4);
        let above = z_neighbour(p, idx, 5);
        flux_32(
            src[below2 as usize],
            src[below as usize],
            src[idx as usize],
            src[above as usize],
            p.courant,
        )
    }

    /// The flux through the **top** face of `idx`, same orientation.
    fn flux_through_top_face(src: &[M32], p: &SettleParams, idx: u32) -> M32 {
        let above = z_neighbour(p, idx, 5);
        let above2 = z_neighbour(p, above, 5);
        let below = z_neighbour(p, idx, 4);
        flux_32(
            src[below as usize],
            src[idx as usize],
            src[above as usize],
            src[above2 as usize],
            p.courant,
        )
    }

    /// How many Z faces of this voxel matter may cross — the `f` of ADR-068.
    fn open_z_faces(p: &SettleParams, idx: u32) -> u32 {
        u32::from(z_face_is_open(p, idx, 4)) + u32::from(z_face_is_open(p, idx, 5))
    }

    /// Obligatory for every new flux function (`.claude/rules/kernels.md`) — but
    /// the statement is **not** the diffusive one, and that is half the value of
    /// this test.
    ///
    /// `flux(a, b) == -flux(b, a)` is false for a donor scheme and must stay
    /// false: the donor is chosen by the sign of the courant number, not by
    /// which side is fuller, so `flux(a, b) = c*a` while `flux(b, a) = c*b`.
    /// Whoever "fixes" that by making the flux a function of the difference has
    /// written diffusion; whoever averages the two sides has written the central
    /// scheme, unconditionally unstable under explicit Euler. Both stay green on
    /// conservation and both destroy the physics of settling.
    ///
    /// What conservation actually rests on here is stated below: for every
    /// internal Z face, the number the lower voxel subtracts through its `z_max`
    /// face is **bitwise** the number the upper voxel adds through its `z_min`
    /// face — one expression, over one four-cell tuple, with the operands in one
    /// order (ADR-054: two sides computing "from their own side" diverge in the
    /// last bit of `Q`, and matter leaks one unit per face).
    #[test]
    fn flux_is_antisymmetric() {
        for mask in [TORUS, FLOORED, CLOSED] {
            for c in COURANTS {
                let p = params(mask, c);
                let src = profile_32();
                let dst = apply_32(&src, &p);

                for idx in 0..N_VOXELS {
                    if z_face_is_open(&p, idx, 5) {
                        let upper = z_neighbour(&p, idx, 5);
                        assert_eq!(
                            flux_through_top_face(&src, &p, idx),
                            flux_through_bottom_face(&src, &p, upper),
                            "the two sides of the face above voxel {idx} disagree \
                             (mask {mask:#08b}, courant {c})"
                        );
                    }

                    // And the kernel applies exactly those numbers, with
                    // opposite signs, and nothing else.
                    let mut expected = src[idx as usize];
                    if z_face_is_open(&p, idx, 4) {
                        expected += flux_through_bottom_face(&src, &p, idx);
                    }
                    if z_face_is_open(&p, idx, 5) {
                        expected -= flux_through_top_face(&src, &p, idx);
                    }
                    assert_eq!(
                        dst[idx as usize], expected,
                        "voxel {idx} did not apply the canonical face fluxes \
                         (mask {mask:#08b}, courant {c})"
                    );
                }
            }
        }
    }

    /// `ACCEPTANCE.md`, section "Conservation". ADR-067 gives the name its
    /// content: one operator, one axis, exact antisymmetry of the flux through
    /// the face.
    ///
    /// Exactly, not to within a tolerance: conservation here is a property of
    /// the two sides of a face applying one number, so any drift at all means
    /// the property is broken rather than small.
    ///
    /// **Two degeneracies this test is built to avoid, and neither may be
    /// weakened away by a later refactor.** On a torus a kernel that destroys
    /// matter at the floor is invisible, because there is no floor. On a
    /// vertically *uniform* profile a leak out of the bottom and an inflow from
    /// a non-existent cell above cancel each other exactly. The carrying case is
    /// therefore the Z-closed grid with a vertically inhomogeneous profile, and
    /// it is the only one that exercises the closed-face branch at all.
    #[test]
    fn sedimentation_conserves_exactly() {
        for mask in [TORUS, FLOORED, CLOSED] {
            for c in COURANTS {
                let p = params(mask, c);

                let src = profile_32();
                let dst = apply_32(&src, &p);
                assert_eq!(
                    total_32(&src),
                    total_32(&dst),
                    "mask {mask:#08b}, courant {c}"
                );

                let src = profile_64();
                let dst = apply_64(&src, &p);
                assert_eq!(
                    total_64(&src),
                    total_64(&dst),
                    "mask {mask:#08b}, courant {c}, 64-bit"
                );

                // And on the profile with the extrema in it, where the limiter
                // switches on and off from face to face. Conservation cannot
                // tell the two profiles apart — it is a property of the tuple,
                // not of the data — and that is exactly why running only the
                // smooth one would leave the impression that it can.
                let src = step_profile_32();
                let dst = apply_32(&src, &p);
                assert_eq!(
                    total_32(&src),
                    total_32(&dst),
                    "mask {mask:#08b}, courant {c}, stepped"
                );

                let src = step_profile_64();
                let dst = apply_64(&src, &p);
                assert_eq!(
                    total_64(&src),
                    total_64(&dst),
                    "mask {mask:#08b}, courant {c}, stepped, 64-bit"
                );
            }
        }
    }

    /// A closed face is handled by a **branch**, not by the identity
    /// `flux(a, a) == 0` — which a directed flux does not satisfy.
    ///
    /// `diffuse.rs` teaches the opposite reflex in the comment on its own
    /// `neighbour`: "a closed face returns the voxel itself... the answer that
    /// makes the face carry no flux without a branch". Copy that here and the
    /// bottom voxel hands matter to itself and it vanishes, while the top voxel
    /// receives an inflow from a cell that does not exist and matter is printed.
    #[test]
    fn a_closed_face_carries_no_settling_flux() {
        let p = params(FLOORED, -0.5);
        let column_x = 1;
        let column_y = 2;

        // Bottom voxel of a Z-closed grid, nothing above it: it has nowhere to
        // send matter and nothing to receive, so the amount may not fall by one.
        let mut src = vec![M32::ZERO; N_VOXELS as usize];
        let floor = index(&p, column_x, column_y, 0) as usize;
        src[floor] = M32::new(4_000);
        let dst = apply_32(&src, &p);
        assert_eq!(dst[floor], src[floor], "the closed floor swallowed matter");
        assert_eq!(total_32(&src), total_32(&dst));

        // Top voxel, nothing below it: it sends matter down and may not gain a
        // single unit from the lid above.
        let mut src = vec![M32::ZERO; N_VOXELS as usize];
        let lid = index(&p, column_x, column_y, NZ - 1) as usize;
        src[lid] = M32::new(4_000);
        let dst = apply_32(&src, &p);
        assert!(
            dst[lid] < src[lid],
            "matter under a downward courant did not leave the top voxel"
        );
        assert_eq!(total_32(&src), total_32(&dst));

        // And the statement about the function itself, which is exactly what
        // separates settling from diffusion: four equal arguments give a
        // non-zero flux, where a diffusive one gives zero by antisymmetry.
        //
        // The amounts are large enough that `|c*a| >= 1/2`. Below half a unit
        // the deterministic rounding of NUMERIC.md section 3 answers zero, and
        // that is a property of the rounding rule rather than of this scheme;
        // asserting non-zero there would be asserting something false.
        for a in [4i64, 7, 4_000] {
            for c in [0.25f64, -0.25, 1.0, -1.0] {
                let c = Q::from_f64(c);
                let a32 = M32::new(a as i32);
                assert_eq!(
                    flux_32(a32, a32, a32, a32, c),
                    q_round_32(qmul(q_conc_32(a32, Q::ONE), c))
                );
                assert_ne!(flux_32(a32, a32, a32, a32, c), M32::ZERO);
                let a64 = M64::new(a);
                assert_ne!(flux_64(a64, a64, a64, a64, c), M64::ZERO);
            }
        }
    }

    /// The consequence ADR-067 named and priced: "the sediment does not stop
    /// itself". There is no hindered settling, `rho_medium` is a constant, and
    /// matter piles up on a closed floor until it hits `max_conc`.
    ///
    /// This test exists so that a future change made "so that it stops piling
    /// up" is a visible change of behaviour rather than a quiet one.
    #[test]
    fn matter_accumulates_on_a_closed_floor() {
        // At |c| = 1 the antidiffusive term vanishes and the scheme moves the
        // whole content of a cell down one voxel per application, so `nz`
        // applications — `nz * ceil(1/|c|)` — put everything on the floor.
        let p = params(FLOORED, -1.0);
        let src = profile_32();
        let start = total_32(&src);

        let mut state = src.clone();
        for _ in 0..NZ {
            let next = apply_32(&state, &p);
            assert_eq!(total_32(&next), start, "a step of the chain lost matter");
            state = next;
        }

        for idx in (NX * NY)..N_VOXELS {
            assert_eq!(
                state[idx as usize],
                M32::ZERO,
                "voxel {idx} above the floor still holds matter"
            );
        }
        assert_eq!(total_32(&state), start);

        // Slower settling drains geometrically rather than one voxel per step,
        // so the chain runs to its fixed point instead of to a derived length.
        // At `|c| = 1/2` the fixed point is still an empty column: a cell of one
        // unit sends `round(1/2) = 1` — halves away from zero — and empties.
        for c in [-0.5f64] {
            let p = params(FLOORED, c);
            let mut state = profile_32();
            for _ in 0..4096 {
                let next = apply_32(&state, &p);
                assert_eq!(total_32(&next), start, "a step of the chain lost matter");
                if next == state {
                    break;
                }
                state = next;
            }
            for idx in (NX * NY)..N_VOXELS {
                assert_eq!(
                    state[idx as usize],
                    M32::ZERO,
                    "voxel {idx} above the floor still holds matter at courant {c}"
                );
            }
        }

        // Below half a voxel per tick the column does **not** empty, and that is
        // arithmetic rather than a defect: a cell holding `a` units sends
        // `round(|c|*a)`, which is zero once `a < 1/(2|c|)`, so a residue of up
        // to `ceil(1/(2|c|)) - 1` units per voxel stays where it is forever. At
        // `|c| = 1/4` that is one unit. Deterministic rounding is what transport
        // uses (ADR-060); the stochastic rule that would carry such a residue is
        // reserved for extent (ADR-027), and extending it here is a decision for
        // the journal, not a patch. The assertion pins the residue so that a
        // change to it is visible.
        let p = params(FLOORED, -0.25);
        let mut state = profile_32();
        for _ in 0..4096 {
            let next = apply_32(&state, &p);
            assert_eq!(total_32(&next), start, "a step of the chain lost matter");
            if next == state {
                break;
            }
            state = next;
        }
        for idx in (NX * NY)..N_VOXELS {
            assert!(
                state[idx as usize] <= M32::new(1),
                "voxel {idx} kept {:?} units above the floor, more than the \
                 rounding residue of one",
                state[idx as usize]
            );
        }
    }

    /// The sign convention, and nothing else in the file sees it.
    ///
    /// The host knows `w = k*(rho_grain - rho_medium)*g/mu` as positive
    /// **downward**; the canonical face flux of this file is positive **upward**.
    /// Exactly one negation lies between them and it is silent. Invert it and
    /// mineral floats out through the lid into `BOUNDARY_EXCHANGE` while the
    /// floor stays clean — with conservation, the per-face agreement, the
    /// undershoot bound and both widths all green.
    #[test]
    fn a_dense_substance_moves_toward_z_min() {
        fn moment(src: &[M32]) -> i64 {
            (0..src.len() as u32)
                .map(|idx| i64::from(idx / (NX * NY)) * src[idx as usize].to_i64())
                .sum()
        }

        // Matter starts in the middle, so neither direction is blocked by a wall
        // on the first step.
        let geometry = params(FLOORED, 0.0);
        let mut src = vec![M32::ZERO; N_VOXELS as usize];
        for x in 0..NX {
            for y in 0..NY {
                for z in 1..NZ - 1 {
                    src[index(&geometry, x, y, z) as usize] = M32::new(10_000 + (x + y + z) as i32);
                }
            }
        }

        let down = apply_32(&src, &params(FLOORED, -0.5));
        assert!(
            moment(&down) < moment(&src),
            "a dense substance did not move toward z_min"
        );

        let up = apply_32(&src, &params(FLOORED, 0.5));
        assert!(
            moment(&up) > moment(&src),
            "a buoyant substance did not move toward z_max"
        );
    }

    /// `ACCEPTANCE.md`, section "Conservation"; the bound is ADR-068's.
    ///
    /// ADR-068 derives **two** forms, and which of them transfers to directed
    /// transport is the whole content of this test.
    ///
    /// The universal one — "from a non-negative state `m >= 0`, and then
    /// `new >= -floor(f/2)`" — uses nothing but two facts: the flux of each face
    /// is rounded to a whole unit, and the scheme is positive in exact
    /// arithmetic. Both hold here, so with the `f32` term it reads
    /// `new >= -floor(f/2) - 3*ceil(spread/2^24)`, with `f` the number of
    /// **open** faces. One axis means `f <= 2`, so the floor is `-1` and not the
    /// `-3` of a seven-point stencil; a voxel on a closed floor has `f = 1` and
    /// a floor of zero.
    ///
    /// The stronger one, relative to the minimum over the stencil, does **not**
    /// transfer to a voxel with a closed face, and the reason is physics rather
    /// than a defect. ADR-068 derives it from `new = H + sum_f round(a*(n_f -
    /// H))` — a flux proportional to the *difference*, which vanishes when the
    /// neighbour equals the voxel. A directed flux is `c*H` whatever the
    /// neighbours hold, so the voxel on the inflow side of a wall drains toward
    /// zero regardless of them: on this grid, uniform 40 units over a closed
    /// floor at `c = +0.1` gives 36, four below the stencil minimum, with `f = 1`
    /// and a claimed floor of zero. Buoyant grains lifting off the floor is
    /// exactly what that is. Where both faces are open the TVD property of the
    /// limiter does hold and the stronger form is asserted too.
    ///
    /// The assertion is the inequality and never the literal `-1`: at
    /// `|c| <= 1` a pure donor scheme reaches zero, so `assert_eq!(.., -1)`
    /// would pin an artefact of the limiter as a contract.
    ///
    /// # The bound is never attained here, and that is a theorem rather than a
    /// gap in the data
    ///
    /// Every voxel of this test comes out at zero or above, on both profiles and
    /// at every courant, so the derived floor of `-1` sits a whole unit below
    /// anything observed. Looking for data that reaches it is wasted effort, and
    /// the reason has to be written down or somebody will spend a day on it:
    ///
    /// - **only one face of a settling voxel is ever outgoing.** The donor of a
    ///   face is chosen by the sign of `c`, which is one sign for the whole
    ///   lane, so a voxel gives matter away through its downstream face and
    ///   receives through the upstream one. ADR-068 counts `f` **open** faces
    ///   because a diffusive voxel loses through all of them at once; here the
    ///   error budget is one rounding, not two, and `floor(1/2) = 0`;
    /// - **the exact outflow never exceeds the pool.** It is
    ///   `|c|*H + (|c|/2)(1 - |c|)*slope` with `slope <= 2*min(du, df) <= 2*H`
    ///   from a non-negative state, hence at most `|c|(2 - |c|)*H <= H` for
    ///   `|c| <= 1`, with equality only at `|c| = 1` where the correction is
    ///   identically zero. Rounding a quantity that is `<= H` cannot exceed `H`.
    ///
    /// So the honest statement of this scheme is stronger than ADR-068's, and
    /// **both** are asserted below: the derived bound, which is the contract,
    /// and non-negativity, which is what the contract has slack against. The
    /// second is the assertion with teeth. A limiter twice too strong — the one
    /// mutation neither conservation nor the mirror identity can see — asks for
    /// as much as `|c|(3 - 2|c|)*H` out of a pool of `H`, which exceeds it for
    /// every `|c|` between a third and one. [`step_pattern`] is built for that
    /// mutation: the empty voxel over the thin layer receives a correction
    /// larger than the whole donor term and comes out at `-80`, past the derived
    /// bound of `-4` as well as past zero.
    #[test]
    fn transport_undershoot_stays_within_the_derived_bound() {
        for mask in [TORUS, FLOORED, CLOSED] {
            for c in COURANTS {
                let p = params(mask, c);
                for src in [profile_32(), step_profile_32()] {
                    // The bound is stated over a non-negative state; both are.
                    assert!(src.iter().all(|v| *v >= M32::ZERO));
                    let dst = apply_32(&src, &p);

                    for idx in 0..N_VOXELS {
                        let below = z_neighbour(&p, idx, 4);
                        let above = z_neighbour(&p, idx, 5);
                        let stencil = [
                            idx,
                            below,
                            z_neighbour(&p, below, 4),
                            above,
                            z_neighbour(&p, above, 5),
                        ];
                        let mut low = i64::MAX;
                        let mut high = i64::MIN;
                        for at in stencil {
                            let v = src[at as usize].to_i64();
                            low = low.min(v);
                            high = high.max(v);
                        }

                        let f = i64::from(open_z_faces(&p, idx));
                        let slack = f / 2 + 3 * (((high - low) + (1 << 24) - 1) / (1 << 24));
                        let new = dst[idx as usize].to_i64();

                        assert!(
                            new >= -slack,
                            "voxel {idx} fell to {new}, past the derived bound \
                             {} (f = {f}, mask {mask:#08b}, courant {c})",
                            -slack
                        );

                        if f == 2 {
                            assert!(
                                new >= low - slack,
                                "voxel {idx} fell to {new}, past the TVD bound {} \
                                 with both faces open (mask {mask:#08b}, courant {c})",
                                low - slack
                            );
                        }

                        // The sharper statement, and the one that fails first.
                        // Both spreads are far below `2^24`, so the `f32` term
                        // of ADR-068 is `3*(spread/2^24)` rounded up to whole
                        // units of *matter* — zero here — rather than the
                        // conservative `3*ceil(.)` of the acceptance form. What
                        // is left is the theorem above, with nothing subtracted
                        // from it.
                        assert!(
                            high - low < 1 << 24,
                            "the profile left the band where the f32 term is zero"
                        );
                        assert!(
                            new >= 0,
                            "voxel {idx} came out at {new} from a non-negative \
                             state: one outgoing face cannot overdraw a pool at \
                             |c| <= 1 (f = {f}, mask {mask:#08b}, courant {c})"
                        );
                    }
                }
            }
        }
    }

    /// The kernel half of
    /// `substance_with_zero_settling_radius_does_not_move_vertically`; the other
    /// half is the host branch, which starts no process for a substance whose
    /// `w` is zero.
    ///
    /// Bitwise equality and not "barely moved": one unit per face per tick is
    /// `10^6` units over the S0 horizon.
    #[test]
    fn a_zero_courant_leaves_the_lane_untouched() {
        for mask in [TORUS, FLOORED, CLOSED] {
            let p = params(mask, 0.0);

            let src = profile_32();
            assert_eq!(apply_32(&src, &p), src, "mask {mask:#08b}");

            let src = profile_64();
            assert_eq!(apply_64(&src, &p), src, "mask {mask:#08b}, 64-bit");
        }
    }

    /// The five-cell stencil makes the temptation stronger than in diffusion:
    /// the flux through a voxel's bottom face has already been computed by the
    /// voxel below as its top face, and reusing it is either a read from the
    /// buffer being written or a sequential dependency — invisible on a forward
    /// CPU sweep and fatal in WGSL, where there is no traversal order at all.
    #[test]
    fn the_result_does_not_depend_on_the_traversal_order() {
        let p = params(TORUS, -0.5);
        let src = profile_32();

        let mut forwards = vec![M32::ZERO; N_VOXELS as usize];
        for idx in 0..N_VOXELS {
            settle_voxel_32(&src, &mut forwards, &p, idx);
        }

        let mut backwards = vec![M32::ZERO; N_VOXELS as usize];
        for idx in (0..N_VOXELS).rev() {
            settle_voxel_32(&src, &mut backwards, &p, idx);
        }

        assert_eq!(forwards, backwards);
        assert_ne!(forwards, src);
    }

    /// The kernel carries its own copy of the neighbourhood lookup, because
    /// `kernels/` may depend on `numeric/` and on nothing else. This is the
    /// third copy in the tree — `diffuse.rs` keeps the same test for the same
    /// reason — and drift between them is caught only here.
    #[test]
    fn the_neighbourhood_agrees_with_the_grid() {
        let cases = [
            (TORUS, [Boundary::Periodic; 6]),
            (
                FLOORED,
                [
                    Boundary::Periodic,
                    Boundary::Periodic,
                    Boundary::Periodic,
                    Boundary::Periodic,
                    Boundary::Closed,
                    Boundary::Closed,
                ],
            ),
            (CLOSED, [Boundary::Closed; 6]),
        ];

        for (mask, boundary) in cases {
            let grid = Grid::new(NX, NY, NZ, boundary).unwrap();
            let p = params(mask, -0.5);
            assert_eq!(grid.n_voxels(), N_VOXELS);

            for idx in 0..N_VOXELS {
                let (x, y, z) = grid.coords(idx);
                assert_eq!(index(&p, x, y, z), grid.index(x, y, z));

                for (face, expected) in [(4u32, Face::ZMinus), (5u32, Face::ZPlus)] {
                    let once = z_neighbour(&p, idx, face);
                    assert_eq!(once, grid.neighbour(idx, expected), "voxel {idx}");
                    // The limiter reaches two cells out, so the doubled step is
                    // part of the contract, not an implementation detail.
                    assert_eq!(
                        z_neighbour(&p, once, face),
                        grid.neighbour(grid.neighbour(idx, expected), expected),
                        "voxel {idx}, doubled step across {expected:?}"
                    );
                }
            }
        }
    }

    /// Subtler than its diffusive twin: with Z periodic **both** faces of a
    /// one-voxel axis carry a non-zero flux `c*here`, and the result is zero
    /// only because they cancel. The assertion is therefore about the result.
    #[test]
    fn an_axis_one_voxel_deep_carries_nothing() {
        for mask in [TORUS, FLOORED] {
            let p = SettleParams {
                nx: 4,
                ny: 4,
                nz: 1,
                periodic_mask: mask,
                courant: Q::from_f64(-0.5),
            };
            let src: Vec<M32> = (0..16).map(|i| M32::new(1_000 + i)).collect();
            let mut dst = vec![M32::ZERO; 16];
            for idx in 0..16u32 {
                settle_voxel_32(&src, &mut dst, &p, idx);
            }
            assert_eq!(dst, src, "mask {mask:#08b}");
        }
    }

    /// At zero gradient the ratio `r` is undefined and both sides of the face
    /// must fall back the same way (ADR-054). The fallback is the first-order
    /// donor flux, and the division must not happen at all.
    ///
    /// Written the textbook way — `r = du/df`, then a branch on the sign of `r`
    /// — this is a `debug_assert` in `qdiv` on every uniform patch of every
    /// field and, in release, `inf` -> `NaN` through `q_round` -> an arbitrary
    /// integer through `from_i64_clamping`: matter appearing out of nothing with
    /// no red test. The harmonic form never reaches the division at all.
    ///
    /// The three zeros of the limiter are asserted on the limiter and not only
    /// through the flux, because they are the same statement at three distances
    /// from the face and only one of them is "flat": `df == 0` is the pair
    /// across the face, `du == 0` is the cell behind the donor — a face against
    /// a wall, where the outer cell folds onto the donor — and opposite signs
    /// are a strict local extremum, where a TVD limiter is obliged to be zero.
    #[test]
    fn the_limiter_falls_back_to_first_order_on_a_flat_gradient() {
        for magnitude in [1i64, 3, 1_000, 1_000_000] {
            for sign in [1i64, -1] {
                let one_way = M32::new((sign * magnitude) as i32);
                let other_way = M32::new((-sign * magnitude) as i32);
                assert_eq!(limited_slope_32(one_way, M32::ZERO), Q::ZERO);
                assert_eq!(limited_slope_32(M32::ZERO, one_way), Q::ZERO);
                assert_eq!(limited_slope_32(one_way, other_way), Q::ZERO);

                let wide = M64::new(sign * magnitude);
                let wide_other = M64::new(-sign * magnitude);
                assert_eq!(limited_slope_64(wide, M64::ZERO), Q::ZERO);
                assert_eq!(limited_slope_64(M64::ZERO, wide), Q::ZERO);
                assert_eq!(limited_slope_64(wide, wide_other), Q::ZERO);
            }
        }
        assert_eq!(limited_slope_32(M32::ZERO, M32::ZERO), Q::ZERO);
        assert_eq!(limited_slope_64(M64::ZERO, M64::ZERO), Q::ZERO);

        for c in COURANTS {
            let c = Q::from_f64(c);
            for v in [0i64, 1, 37, 4_000, -4_000] {
                let flat = M32::new(v as i32);
                let expected = q_round_32(qmul(q_conc_32(flat, Q::ONE), c));
                for outer in [-9_999i64, 0, 9_999] {
                    let outer32 = M32::new(outer as i32);
                    assert_eq!(
                        flux_32(outer32, flat, flat, outer32, c),
                        expected,
                        "flat gradient at {v}, outer {outer}, courant {}",
                        c.debug_f64()
                    );
                }
                let flat64 = M64::new(v);
                assert_eq!(
                    flux_64(flat64, flat64, flat64, flat64, c),
                    q_round_64(qmul(q_conc_64(flat64, Q::ONE), c))
                );
            }
        }
    }

    /// The limiter is van Leer and not something else wearing its name.
    ///
    /// `|phi| <= 2*min(|du|, |df|)` with the sign of `df` is Sweby's
    /// second-order TVD region, and it is what separates van Leer from the two
    /// substitutions that pass every conservation test in the project: minmod
    /// gives `min(|du|, |df|)`, which is half the numerical diffusion ADR-054
    /// exists to remove, and superbee reaches `2*max`, leaving the region on a
    /// step — the one ADR-054 rejected by name, for *manufacturing* the sharp
    /// fronts the project is looking for.
    ///
    /// The lower half of the region matters as much here and is asserted with
    /// it: `|phi| >= min(|du|, |df|)`. A limiter clamped to zero satisfies the
    /// upper bound and every other test in this file.
    #[test]
    fn the_limiter_stays_inside_the_tvd_region() {
        let values = [
            -1_000_000i64,
            -12_345,
            -100,
            -7,
            -1,
            0,
            1,
            7,
            100,
            12_345,
            1_000_000,
        ];

        for du in values {
            for df in values {
                let narrow = limited_slope_32(M32::new(du as i32), M32::new(df as i32));
                let wide = limited_slope_64(M64::new(du), M64::new(df));
                assert_eq!(narrow, wide, "the two widths disagree at ({du}, {df})");

                if du == 0 || df == 0 || du.signum() != df.signum() {
                    assert_eq!(narrow, Q::ZERO, "no fallback at ({du}, {df})");
                    continue;
                }

                let phi = narrow.debug_f64();
                assert_eq!(
                    phi.signum(),
                    df.signum() as f64,
                    "phi {phi} does not follow the sign of df at ({du}, {df})"
                );
                // The slack covers three f32 roundings of the harmonic form. A
                // relative 2^-20 is orders of magnitude too small to admit
                // either neighbour: superbee leaves the upper bound by a factor
                // and minmod misses the lower one by a factor, not by an ulp.
                let smaller = du.abs().min(df.abs()) as f64;
                assert!(
                    phi.abs() <= 2.0 * smaller * (1.0 + 2f64.powi(-20)),
                    "phi {phi} is above the TVD region at ({du}, {df})"
                );
                assert!(
                    phi.abs() >= smaller * (1.0 - 2f64.powi(-20)),
                    "phi {phi} is below the harmonic mean at ({du}, {df}): a \
                     limiter this weak is minmod or worse"
                );
            }
        }
    }

    /// Settling and advection have to be the same *program* and not merely the
    /// same number, and this is the only thing in the tree that says so.
    ///
    /// ADR-067 takes the limiter whole from ADR-054, so a substance that both
    /// advects and settles goes through one limiter twice. Two texts of it round
    /// alike in `FLOAT` by luck rather than by construction — in `FIXED` a
    /// different association order rounds differently, and a sign taken after a
    /// rounded division can move — so the copy in this file is held against the
    /// original by argument, over a sweep that includes the pairs where the
    /// harmonic mean is worst conditioned: two operands of wildly different
    /// size, and two that nearly cancel.
    #[test]
    fn the_limiter_is_advections_limiter() {
        use crate::kernels::advect;

        let values = [
            -5_100_000_000_000i64,
            -1_000_001,
            -999_999,
            -1_000,
            -3,
            -1,
            0,
            1,
            3,
            1_000,
            999_999,
            1_000_001,
            5_100_000_000_000,
        ];

        for du in values {
            for df in values {
                assert_eq!(
                    limited_slope_64(M64::new(du), M64::new(df)),
                    advect::limited_slope_64(M64::new(du), M64::new(df)),
                    "the settling limiter and the advective one differ at \
                     ({du}, {df})"
                );
                if du.abs() <= i64::from(i32::MAX) && df.abs() <= i64::from(i32::MAX) {
                    assert_eq!(
                        limited_slope_32(M32::new(du as i32), M32::new(df as i32)),
                        advect::limited_slope_32(M32::new(du as i32), M32::new(df as i32)),
                        "the narrow widths differ at ({du}, {df})"
                    );
                }
            }
        }
    }

    /// The mirror identity of ADR-054, which is the one property of the flux
    /// that sees the **direction** of transport:
    ///
    /// ```text
    /// flux(ll, l, h, hh, c) == -flux(hh, h, l, ll, -c)
    /// ```
    ///
    /// bit for bit. Reading a face from the other end picks the same donor, the
    /// same upwind cell and the same two differences, so only the sign of every
    /// multiplication by `c` changes — and negation is exact in IEEE-754 and in
    /// fixed point alike.
    ///
    /// Three defects live under this one assertion and nothing else in the file
    /// catches any of them: a limiter that looks downwind of the donor instead
    /// of upwind (the mirrored call would then read a different cell), the
    /// factor `(1 - c)` written where `(1 - |c|)` belongs (the correction would
    /// grow instead of vanishing for a sinking grain), and `floor` in place of
    /// the rounding rule (`floor(1.5) = 1` against `-floor(-1.5) = 2`).
    #[test]
    fn the_flux_mirrors_when_the_axis_is_reversed() {
        let quads = [
            (0i64, 0, 1_000, 3_000),
            (-500, 200, 900, 1_500),
            (7, 7, 7, 7),
            (1_000_000, 3, -4, 999),
            (0, 1_000, 4_000_000, 0),
            (3, 2, 1, 0),
            (0, 1, 2, 3),
        ];

        for (ll, l, h, hh) in quads {
            for c in COURANTS {
                let forward = Q::from_f64(c);
                let mirrored = Q::from_f64(-c);

                let narrow = flux_32(
                    M32::new(ll as i32),
                    M32::new(l as i32),
                    M32::new(h as i32),
                    M32::new(hh as i32),
                    forward,
                );
                let back = flux_32(
                    M32::new(hh as i32),
                    M32::new(h as i32),
                    M32::new(l as i32),
                    M32::new(ll as i32),
                    mirrored,
                );
                assert_eq!(
                    narrow.to_i64(),
                    -back.to_i64(),
                    "flux_32({ll}, {l}, {h}, {hh}, {c}) is not the negation of \
                     the same face read from the other end"
                );

                let wide = flux_64(
                    M64::new(ll),
                    M64::new(l),
                    M64::new(h),
                    M64::new(hh),
                    forward,
                );
                let wide_back = flux_64(
                    M64::new(hh),
                    M64::new(h),
                    M64::new(l),
                    M64::new(ll),
                    mirrored,
                );
                assert_eq!(wide.to_i64(), -wide_back.to_i64());
            }
        }
    }

    /// The second-order half of the scheme, recomputed in `f64` from ADR-054 and
    /// compared against the kernel.
    ///
    /// This is the test the file did not have, and the reason it is worth its
    /// length: the antidiffusive correction is invisible to everything else
    /// here. Conservation holds for any function of the four cells whatever;
    /// the mirror identity holds for any function symmetric in the way this one
    /// is; the undershoot bound has a unit of slack against it. Deleting the
    /// correction outright leaves a first-order donor scheme, which is exactly
    /// what ADR-054 says is not good enough — `D_num = (w*dx/2)(1 - C)` is the
    /// same order as the molecular diffusion the model resolves — and it is a
    /// deletion no other assertion in this file would notice.
    ///
    /// The reference is written in `f64` rather than in `Q` on purpose: a
    /// reference built out of the same wrappers in the same order would agree
    /// with the kernel by construction and would be checking the transcription
    /// rather than the scheme. The tolerance is therefore an honest one — half a
    /// unit for `q_round` plus the `f32` error of the two products — and the
    /// amounts are kept small enough that it stays below the term under test.
    #[test]
    fn the_antidiffusive_correction_is_the_van_leer_one() {
        /// ADR-054's scheme in `f64`: no `Q`, no rounding, no wrappers.
        fn reference(outer_low: i64, low: i64, high: i64, outer_high: i64, c: f64) -> f64 {
            let upward = c >= 0.0;
            let donor = if upward { low } else { high };
            let acceptor = if upward { high } else { low };
            let upwind = if upward { outer_low } else { outer_high };

            let df = (acceptor - donor) as f64;
            let du = (donor - upwind) as f64;
            let slope = if du * df > 0.0 {
                2.0 * du * df / (du + df)
            } else {
                0.0
            };
            c * donor as f64 + 0.5 * c * (1.0 - c.abs()) * slope
        }

        // Four-cell stencils, each with a reason. The first three are smooth and
        // monotone in the direction of the flow, which is where the correction
        // is largest and where a limiter that reaches the wrong way answers zero
        // instead. The last two are extrema, where van Leer must switch off.
        let quads = [
            (0i64, 1_000, 4_000, 9_000),
            (100, 1_100, 2_100, 3_100),
            (0, 1_000, 4_000_000, 8_000_000),
            (0, 4_000, 1_000, 0),
            (5_000, 1_000, 4_000, 500),
        ];

        let mut saw_a_live_correction = 0;
        for (ll, l, h, hh) in quads {
            for c in COURANTS {
                let expected = reference(ll, l, h, hh, c);
                let got = flux_32(
                    M32::new(ll as i32),
                    M32::new(l as i32),
                    M32::new(h as i32),
                    M32::new(hh as i32),
                    Q::from_f64(c),
                )
                .to_i64() as f64;

                // Half a unit for the rounding, plus four f32 ulps of the
                // largest intermediate for the chain that produced it.
                let scale = (ll.abs().max(l.abs()).max(h.abs()).max(hh.abs())) as f64;
                let tolerance = 0.5 + 4.0 * scale * 2f64.powi(-24);
                assert!(
                    (got - expected).abs() <= tolerance,
                    "flux_32({ll}, {l}, {h}, {hh}, {c}) came out at {got} \
                     against the van Leer value {expected} (tolerance \
                     {tolerance})"
                );

                // And the term under test is not a rounding artefact on this
                // stencil: the first-order flux is a different integer.
                let first_order = c * (if c >= 0.0 { l } else { h }) as f64;
                if (expected - first_order).abs() > 2.0 * tolerance {
                    saw_a_live_correction += 1;
                    assert_ne!(
                        got.round() as i64,
                        first_order.round() as i64,
                        "the antidiffusive correction is missing at \
                         ({ll}, {l}, {h}, {hh}) with c = {c}"
                    );
                }
            }
        }
        // The sweep has to have exercised the correction, or the loop above
        // asserts nothing at all.
        assert!(
            saw_a_live_correction >= 8,
            "only {saw_a_live_correction} of the stencils had a correction \
             large enough to see; the table has stopped testing anything"
        );
    }

    /// Settling has no fixed point as a whole — on a closed grid the top voxel
    /// empties and the bottom one fills — so the statement is about the
    /// interior, and a straight copy of `a_uniform_field_is_a_fixed_point` here
    /// would be red for physics rather than for a defect.
    #[test]
    fn a_uniform_column_does_not_erode_in_its_interior() {
        for c in COURANTS {
            let p = params(FLOORED, c);
            let src = vec![M32::new(1_000_003); N_VOXELS as usize];
            let dst = apply_32(&src, &p);
            for z in 1..NZ - 1 {
                for x in 0..NX {
                    for y in 0..NY {
                        let at = index(&p, x, y, z) as usize;
                        assert_eq!(dst[at], src[at], "voxel ({x}, {y}, {z}), courant {c}");
                    }
                }
            }
        }
    }

    /// The two widths come out of one macro so that they cannot drift; this says
    /// they have not. Drift is silent and surfaces as "the 64-bit substance
    /// somehow behaves differently" (`numeric/m.rs`).
    #[test]
    fn both_widths_are_one_text() {
        for c in COURANTS {
            let c_q = Q::from_f64(c);
            for a in [-4_000i64, -37, -1, 0, 1, 37, 4_000, 1_000_003] {
                for b in [-4_000i64, -1, 0, 1, 4_000] {
                    let narrow = flux_32(
                        M32::new(a as i32),
                        M32::new(b as i32),
                        M32::new((a + b) as i32),
                        M32::new((b - a) as i32),
                        c_q,
                    );
                    let wide = flux_64(
                        M64::new(a),
                        M64::new(b),
                        M64::new(a + b),
                        M64::new(b - a),
                        c_q,
                    );
                    assert_eq!(narrow.to_i64(), wide.to_i64(), "a {a}, b {b}, courant {c}");
                }
            }

            for mask in [TORUS, FLOORED, CLOSED] {
                let p = params(mask, c);
                let narrow = apply_32(&profile_32(), &p);
                let wide = apply_64(&profile_64(), &p);
                for idx in 0..N_VOXELS as usize {
                    assert_eq!(narrow[idx].to_i64(), wide[idx].to_i64(), "voxel {idx}");
                }
            }
        }
    }
}
