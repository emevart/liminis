//! Advection of one substance along one axis: a van Leer limited flux in
//! gather form.
//!
//! The second transport kernel, and it is written against the first
//! (`kernels/diffuse.rs`) rather than beside it — same shape, same gather form,
//! same single rounding rule. Everything below that is *not* a copy of diffusion
//! is called out where it appears, because every one of those places fails
//! silently rather than loudly.
//!
//! # One axis per application, with the whole step
//!
//! `p.axis` selects the axis and the kernel does that axis only. Advection is
//! split component-wise (ADR-036), and the reason is the Courant condition SPEC
//! section 4.2 actually writes: `max(|u|*dt/dx) <= 1`. An unsplit
//! three-dimensional donor scheme would need the *sum* over the axes, which is
//! three times stricter. There are no substeps here: one application is one
//! axis with the full step, and the host applies the three axes in an order that
//! is part of the world semantics (ADR-036) and therefore lives in `process/`.
//!
//! Branching on `p.axis` is branching on a parameter that is the same for every
//! voxel of the dispatch, so on the GPU it is a uniform jump and no warp
//! diverges — the argument ADR-040 already made for `width_mask`.
//!
//! # The canonical orientation of a face (ADR-054)
//!
//! A face is computed **once**, from the lower linear index to the higher, and
//! the two voxels that share it apply the same number with opposite signs. In
//! this file that is not a convention anybody has to remember: it is expressed
//! in the layout of `courant`, one cell per face, read by both neighbours at the
//! same index, and in [`flux_32`] taking four cells in a fixed order rather than
//! "here" and "there".
//!
//! It has to be that way because the limiter's stencil is wider than the pair —
//! it looks at the cell upwind of the donor. If each voxel computed the flux
//! "from its own side", the two sides would run one expression with a different
//! operand order *and* a different stencil, `Q` arithmetic would disagree in the
//! last bit, and antisymmetry would break silently, exactly the way `floor`
//! breaks it. `flux_is_antisymmetric` is the only thing that would notice.
//!
//! The fallback at a zero gradient is a **consequence** of that orientation
//! rather than a second measure: the ratio `r` is undefined there and both sides
//! have to fall back to first order identically, which they do because the
//! argument they branch on is one argument, computed once (ADR-054).
//!
//! # What is not here
//!
//! The velocity field, `dt`, `dx`, the interpolation of `u` from the coarse grid
//! onto a fine face (ADR-069), the order of the axes, the buffer swap between
//! them. All of it is folded or owned on the host; this file receives one
//! Courant number per face and a mask of six bits.
//!
//! There is no clamp, no renormalisation and no velocity trimming, and all three
//! are refusals rather than omissions. A clamp would print matter, because the
//! matter is already in the neighbour by the time the result is negative and
//! conservation in gather form is a property of the flux function rather than of
//! the result (ADR-068). Trimming the velocity turns a config error into quietly
//! distorted physics (ADR-069). What the kernel owes instead is a *bounded*
//! undershoot, and the bound is stated on [`advect_voxel_32`].

use crate::numeric::{
    M32, M64, Q, q_conc_32, q_conc_64, q_round_32, q_round_64, qadd, qdiv, qmul, qsub,
};

/// Parameters of one application. Scalars only, for the reason `DiffuseParams`
/// gives: in WGSL this is a uniform buffer.
///
/// Neither `dt`, nor `dx`, nor the velocity appears here. They are folded into
/// the Courant number on the host, one per face, and a kernel that knew them
/// separately could combine them in the wrong order (`ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug)]
pub struct AdvectParams {
    /// Voxels along X.
    pub nx: u32,
    /// Voxels along Y.
    pub ny: u32,
    /// Voxels along Z.
    pub nz: u32,
    /// Bit `f` set: face `f` is periodic and wraps. Bit clear: the face is
    /// closed.
    ///
    /// Same encoding as `DiffuseParams::periodic_mask` — the bit index is the
    /// discriminant of `world::Face`, `0 = x_min` … `5 = z_max` — and it is the
    /// same encoding on purpose: two transport kernels reading one host-side
    /// mask differently is a class of bug with no symptom other than matter
    /// going the wrong way at one wall.
    ///
    /// **A closed face is not handled the way diffusion handles it.** There the
    /// neighbour across a closed face is the voxel itself and the face carries
    /// nothing without a branch, because `flux(a, a) == 0` for an antisymmetric
    /// flux. That corollary is *false* here: the donor term is proportional to
    /// the amount rather than to a difference, so `flux(a, a, a, a, c)` is
    /// `round(c*a)` and a wall built out of it leaks. The branch is in
    /// [`advect_voxel_32`], and `a_closed_face_carries_nothing_at_any_velocity`
    /// asserts both halves of the statement.
    ///
    /// Periodicity is a property of an *axis*, not of a face: `world::Grid::new`
    /// refuses a half-periodic axis outright. This struct carries no such
    /// guarantee, so [`axis_is_periodic`] asserts it in debug. A mask with bit
    /// `2a` and not bit `2a+1` would wrap the lower face of the axis and fold
    /// the upper one onto itself, and matter would vanish at one end of the axis
    /// with no symptom other than a ledger residual.
    ///
    /// An `exchange` face is the **third** state and lives in
    /// [`AdvectParams::exchange_mask`]. Folded in here as "not periodic" it
    /// would seal the lid; left out of both masks it would be closed just the
    /// same, which is the mistake with no symptom at all — step `d` would vent
    /// and step `c` would not, and the difference would read as physics.
    pub periodic_mask: u32,
    /// Bit `f` set: face `f` trades with the reservoir, and the neighbour across
    /// it is the ghost cell at `nx*ny*nz` (ADR-059).
    ///
    /// Same encoding as [`AdvectParams::periodic_mask`] and as
    /// `DiffuseParams::exchange_mask`: two transport kernels reading one
    /// host-side mask differently is a class of bug whose only symptom is matter
    /// going the wrong way at one wall.
    ///
    /// **The limiter switches itself off on inflow for free.** The cell upwind of
    /// the ghost is the ghost — [`neighbour_along`] collapses it onto itself — so
    /// `du` is exactly zero, `limited_slope` answers `Q::ZERO` at any `df`, and
    /// the face is the pure donor term. That is precisely the fallback ADR-054
    /// prescribes wherever the ratio `r` is undefined, arrived at by the address
    /// rather than by a branch (ADR-059).
    pub exchange_mask: u32,
    /// The axis this application advects along: `0 = X`, `1 = Y`, `2 = Z`.
    ///
    /// One application is one axis with the **full** step (ADR-036). Advection
    /// has no substeps: the Courant condition is a condition on the folded
    /// number, checked by the host at load time, and nothing here divides it.
    pub axis: u32,
}

/// Generates one storage width of the advection kernel.
///
/// Invoked twice, immediately below, for the reason `diffuse.rs` and `m.rs` both
/// give at length: the storage width of an amount is derived at load time
/// (ADR-040), so both widths have to exist, and two hand-written texts drift
/// apart within a month. Here the argument is sharper than it was for diffusion,
/// because the limiter has a branch in it, and a branch is exactly the thing
/// that gets "fixed" in one copy.
macro_rules! define_advect {
    ($voxel:ident, $flux:ident, $slope:ident, $m:ty, $q_conc:ident, $q_round:ident) => {
        #[doc = concat!("The van Leer limited slope, in `", stringify!($m), "`.")]
        ///
        /// `2*du*df / (du + df)` when `du` and `df` have **strictly** the same
        /// sign, and `Q::ZERO` otherwise. Both differences arrive already taken
        /// in integers and already oriented along the flow: `df` is
        /// `acceptor - donor` across the face, `du` is `donor - upwind` behind
        /// it.
        ///
        /// # Why the harmonic form and not `psi(r) * df`
        ///
        /// The textbook limiter is `psi(r) = (r + |r|) / (1 + r)` with
        /// `r = du/df`, and the two forms are the same number wherever both are
        /// defined. They are not the same *program*:
        ///
        /// - `r` needs a division by `df`, and `df == 0` is not a rare case but
        ///   the common one — every uniform patch of every field. In debug that
        ///   division trips `qdiv`'s assertion; in release it produces an
        ///   infinity, which the next wrapper reports as a non-finite `Q`
        ///   somewhere else entirely (`numeric/float.rs`);
        /// - branching on `r` after the division branches on a `Q`. Today, in
        ///   `FLOAT`, that gives the same answer; in `FIXED` the division rounds
        ///   and the sign can move. The branch below is taken on exact integers,
        ///   which is the difference between the two sides of a face agreeing
        ///   "by the shape of the expression" and agreeing *identically*.
        ///
        /// So the branch stands **before** any division, and the harmonic form
        /// never divides by zero at all: past the branch both differences are
        /// nonzero and of one sign, so their sum is nonzero too.
        ///
        /// # Why the differences are taken in `M`
        ///
        /// Because ADR-060 says the order of the composition is load-bearing,
        /// and this is where losing it costs the most. `df` and `du` are integer
        /// subtractions of amounts; the crossing into `Q` happens after. Written
        /// the other way round — `qsub(q_conc(acceptor), q_conc(donor))` — a
        /// difference smaller than half an `f32` step of the pool collapses to
        /// zero, the limiter is dead, the scheme quietly drops to first order,
        /// and the ledger closes **exactly**, because both forms are
        /// antisymmetric. That step is `2^15 = 32 768` units at the `5.1e11`
        /// ADR-040 derives for water and `2^19 = 524 288` at the `5.1e12` the
        /// tests of the corpus stand water at, so the band that vanishes is tens
        /// of thousands of units wide either way.
        ///
        /// Nothing in a sweep notices, which is why the order is pinned by two
        /// tests of its own rather than by this comment:
        /// `flux_is_the_two_named_crossings_composed` recomputes the face from
        /// the integer differences and compares integers, and
        /// `a_small_gradient_in_a_large_pool_still_carries_flux` asserts the
        /// consequence on water.
        ///
        /// # The public surface is the test
        ///
        /// This is public because
        /// `flux_limiter_falls_back_to_first_order_at_zero_gradient` has to
        /// assert about the fallback itself and not only about its consequence.
        /// A test that could see only the flux would pass on a limiter stuck at
        /// zero everywhere.
        #[inline(always)]
        pub fn $slope(upwind: $m, face: $m) -> Q {
            // Strictly the same sign, in exact integers. "Strictly" excludes
            // both zeros, which is where the ratio is undefined and where van
            // Leer is zero anyway: `psi(0) = 0`, and the harmonic form tends to
            // zero as either difference does.
            let rising = upwind > <$m>::ZERO && face > <$m>::ZERO;
            let falling = upwind < <$m>::ZERO && face < <$m>::ZERO;
            if !(rising || falling) {
                return Q::ZERO;
            }

            let du = $q_conc(upwind, Q::ONE);
            let df = $q_conc(face, Q::ONE);
            // `2*du*df/(du+df)` associated as `2 * (du*df/(du+df))` rather than
            // `(2*du*df)/(du+df)`: doubling is exact in every mode, and this
            // keeps the largest intermediate at `du*df` instead of twice that.
            // Both sides of a face reach this line with identical arguments, so
            // any association would be antisymmetric — this one is merely the
            // one that overflows a decade later.
            let harmonic = qdiv(qmul(du, df), qadd(du, df));
            qadd(harmonic, harmonic)
        }

        #[doc = concat!("The flux across one face, in `", stringify!($m), "`.")]
        ///
        /// The face is the one between `low` and `high`, in the canonical
        /// orientation of ADR-054: `low` is the smaller linear index, `high` the
        /// larger, and a positive result moves matter toward the larger index.
        /// `far_low` is the cell below `low` and `far_high` the cell above
        /// `high` — four cells rather than two, which is the price ADR-054 names
        /// for the limiter.
        ///
        /// `courant` is `u*dt/dx` **on this face**, signed along the same axis:
        /// positive means toward the larger index. That convention is the one
        /// silent failure of the whole file that no conservation test can see.
        /// If the host folds `u` as an *outgoing* face velocity instead, matter
        /// travels upstream, both ledgers stay green, antisymmetry holds, and
        /// the only symptom is a plume going the wrong way across a field nobody
        /// has looked at yet. `matter_moves_downstream_not_upstream` is the only
        /// test that catches it.
        ///
        /// # The scheme
        ///
        /// ```text
        /// donor  = low     if c >= 0 else high
        /// upwind = far_low if c >= 0 else far_high
        /// df     = acceptor - donor      (in M)
        /// du     = donor - upwind        (in M)
        /// F = round( c*conc(donor) + c*(1-|c|)/2 * limited_slope(du, df) )
        /// ```
        ///
        /// One branch, not two expressions written out. The donor is chosen by
        /// the **sign of the velocity**, never by the canonical orientation: an
        /// upwind difference taken downwind is an unconditionally unstable
        /// scheme that conserves matter exactly and reads as turbulence on a
        /// volume render.
        ///
        /// The `(1 - |c|)` factor is what makes the second-order correction
        /// vanish at a Courant number of one, where a whole voxel of transfer is
        /// the exact answer. Losing it is nearly invisible at small `c` and
        /// removes the TVD property entirely;
        /// `at_courant_one_the_flux_is_pure_donor_transfer` pins it.
        ///
        /// # One rounding, on the face
        ///
        /// Exactly one `q_round` per face, over the whole expression (ADR-060).
        /// Not one per voxel over the sum of its two faces: the neighbour would
        /// round *its* sum rather than the opposite of this one, and the two
        /// would stop being exact negations. And not `floor`, for the reason the
        /// rounding rule exists — halves away from zero is the symmetric rule,
        /// and the leak here would be a unit per face per pass, three times
        /// faster than the diffusive one because there are three axes.
        ///
        /// # Antisymmetry, in the form this function can have it
        ///
        /// Diffusion's `flux(a, b) == -flux(b, a)` has a mirror image here:
        ///
        /// ```text
        /// flux(ll, l, r, rr, c) == -flux(rr, r, l, ll, -c)
        /// ```
        ///
        /// and it holds bit for bit, by construction, because the mirrored call
        /// picks the *same* donor, the same upwind cell and therefore the same
        /// two differences — only the sign of every multiplication by `c`
        /// changes, and negation is exact in IEEE-754 and in fixed point alike.
        /// What conservation actually needs is weaker and comes for free from
        /// the layout: both voxels sharing a face call this function with
        /// identical arguments, so they see the same integer, and one adds what
        /// the other subtracts.
        ///
        /// The corollary diffusion leans on is **false** here:
        /// `flux(a, a, a, a, c)` is `round(c*a)`, not zero. A closed face
        /// therefore needs a branch, and it has one in [`advect_voxel_32`].
        #[inline(always)]
        pub fn $flux(far_low: $m, low: $m, high: $m, far_high: $m, courant: Q) -> $m {
            let downstream = courant >= Q::ZERO;
            let donor = if downstream { low } else { high };
            let acceptor = if downstream { high } else { low };
            let upwind = if downstream { far_low } else { far_high };

            // Both differences in integers, and only then across into `Q`
            // (ADR-060). See the note on the limiter for what the other order
            // costs and why the ledger would not notice.
            let df = acceptor - donor;
            let du = donor - upwind;

            // `|c|` by comparison and `qsub(ZERO, c)`, because the ten wrappers
            // of NUMERIC.md section 2 do not include an absolute value and
            // comparisons are explicitly not arithmetic (`numeric/float.rs`).
            // `qsqrt(qmul(c, c))` would be the same number and three
            // mode-dependent operations instead of none.
            let magnitude = if downstream {
                courant
            } else {
                qsub(Q::ZERO, courant)
            };
            // `c*(1 - |c|)/2`. The halving is a division by an exact two rather
            // than a multiplication by a literal: building a `Q` out of a
            // host-side number is the host's job (`numeric/float.rs`), and
            // division by two is exact wherever the value is.
            let antidiffusive = qdiv(qmul(courant, qsub(Q::ONE, magnitude)), qadd(Q::ONE, Q::ONE));

            $q_round(qadd(
                qmul(courant, $q_conc(donor, Q::ONE)),
                qmul(antidiffusive, $slope(du, df)),
            ))
        }

        #[doc = concat!("One application of advection along `p.axis` in one voxel, in `", stringify!($m), "`.")]
        ///
        /// Reads `src` and `courant`, writes `dst[idx]` and nowhere else. Two
        /// input slices rather than one is as legitimate here as it is in the
        /// reaction skeleton of `ARCHITECTURE.md`: the shape ADR-034 fixes is
        /// "slices in, one slice out, `Copy` scalars, a linear index", not a
        /// count of arguments.
        ///
        /// # The layout of `courant`, where the canonical orientation lives
        ///
        /// `courant[axis * lane_len + idx]` is the Courant number on the
        /// **lower** face of voxel `idx` along `axis` — one cell per face. The
        /// indexing follows the only precedent in the corpus, `s * lane_len +
        /// idx` in the reaction skeleton.
        ///
        /// A voxel reads its lower face at `idx` and its upper face at the index
        /// of its upper neighbour, which is that neighbour's lower face. The two
        /// voxels sharing a face read **the same cell** and build the same
        /// four-cell stencil, so they compute the same integer. That is ADR-054
        /// expressed as a layout rather than as a discipline nobody can check.
        ///
        /// **The stride is `lane_len` and not `n_voxels`**, and the extra cell is
        /// the one the face of the domain needs: the upper face of the last voxel
        /// of an axis is the *ghost's* lower face, so it is read at
        /// `axis*lane_len + ghost` and a buffer of `3*n_voxels` has no cell for
        /// it at all. Three such cells exist, one per axis, and a grid that
        /// exchanged on both faces of one axis would have them collide — there is
        /// one ghost per lane, because `[boundary.reservoir]` is one section.
        // TODO(exchange-courant): what number belongs in that cell is settled
        // nowhere. ADR-059 gives one `alpha_ex = k_ex*dt/dx` for the *diffusive*
        // face and no advective speed for the domain face at all; whether it is
        // `+-alpha_ex`, or the prescribed `u` interpolated onto the lid
        // (ADR-069), and which way the sign points, is named by no record. The
        // host leaves it at whatever it was given, and `Advect::fold_courant`
        // checks it like any other face.
        ///
        /// # Gather, and the sign
        ///
        /// ADR-005 states transport as "subtract from the donor, add to the
        /// acceptor". The gather twin of that is: a voxel adds the flux of its
        /// lower face and subtracts the flux of its upper face, because a
        /// positive flux points toward the larger index in both cases.
        ///
        /// # Undershoot, and why there is no clamp
        ///
        /// A voxel can come out below its stencil minimum, and ADR-068 both
        /// permits it and bounds it:
        ///
        /// ```text
        /// new >= (minimum over the stencil) - floor(f/2) - 3*ceil(spread/2^24)
        /// ```
        ///
        /// with `f` the number of open faces — two or zero on one axis. No
        /// absolute number can stand in for that expression: it is three units
        /// below `2^23`, six below `2^24`, fifty-one at the ceiling ADR-039
        /// allows. Clamping the result would destroy matter the neighbours have
        /// already been given and break the one property the scheme is built on,
        /// silently (ADR-068).
        // TODO(advective-undershoot-bound): ADR-068 declares that this bound
        // covers advection too, and derives it on the diffusive chain. Two
        // places do not carry over, and neither has a number in the corpus:
        //
        // - the anchor. The diffusive derivation gives "not below the stencil
        //   minimum" from a convex combination. Under a *divergent* face
        //   velocity field a uniform field legitimately falls below its stencil
        //   minimum, and positivity there is relative to zero and follows from
        //   the outflow condition of SPEC section 4.2 instead;
        // - the `f32` term. `3*ceil(spread/2^24)` was derived from a chain whose
        //   argument is a difference. Advection's donor term crosses into `Q` as
        //   an *amount*, so the term understates the error wherever the spread
        //   is much smaller than the pool.
        //
        // The acceptance test is therefore restricted to the regime where both
        // forms agree — uniform `c`, pool below `2^23`. Settling the advective
        // form is a journal entry, not a change to this file.
        pub fn $voxel(src: &[$m], courant: &[Q], dst: &mut [$m], p: &AdvectParams, idx: u32) {
            debug_assert!(p.axis <= 2, "advection axis {} is not 0, 1 or 2", p.axis);

            let axis = p.axis;
            // `lane_len`, not `n_voxels`: the ghost cell carries the Courant
            // number of the face of the domain, and there is no other cell for
            // it (see the note on the layout above).
            let base = axis * (p.nx * p.ny * p.nz + 1);
            let periodic = axis_is_periodic(p, axis);
            let coord = coord_on_axis(p, idx, axis);
            let extent = extent_on_axis(p, axis);
            // The two faces of this axis, in the third state. A face left out of
            // both masks is closed, and a closed face carries nothing — so an
            // exchanging face that never reached this line would make step `c`
            // seal a lid that step `d` vents.
            let exchange_below = (p.exchange_mask >> (2 * axis)) & 1 == 1;
            let exchange_above = (p.exchange_mask >> (2 * axis + 1)) & 1 == 1;

            let here = src[idx as usize];
            let down = neighbour_along(p, idx, axis, 0);
            let up = neighbour_along(p, idx, axis, 1);
            // The second neighbour, which diffusion never needed: the limiter
            // looks one cell upwind of the donor, and the donor may be either
            // side of either face.
            let far_down = neighbour_along(p, down, axis, 0);
            let far_up = neighbour_along(p, up, axis, 1);

            let mut net = <$m>::ZERO;

            // The lower face: between `down` and `here`, canonically oriented,
            // and its Courant cell is this voxel's own.
            if periodic || coord > 0 || exchange_below {
                net += $flux(
                    src[far_down as usize],
                    src[down as usize],
                    here,
                    src[up as usize],
                    courant[(base + idx) as usize],
                );
            }

            // The upper face: between `here` and `up`, whose lower face it is.
            //
            // The branch is what a wall costs here. Copy diffusion's trick — the
            // neighbour across a closed face is the voxel itself, no branch —
            // and the wall leaks, because `flux(a, a, a, a, c)` is `round(c*a)`
            // rather than zero. One outcome of that mistake is loud (matter
            // appears at voxel zero out of nothing and the ledger says so) and
            // one is silent: on a closed axis the upper face of the last voxel
            // folds onto its own lower Courant cell, the net comes out near
            // zero, and the voxel at the wall simply never advects while every
            // sum still closes.
            if periodic || coord + 1 < extent || exchange_above {
                net -= $flux(
                    src[down as usize],
                    here,
                    src[up as usize],
                    src[far_up as usize],
                    courant[(base + up) as usize],
                );
            }

            dst[idx as usize] = here + net;
        }
    };
}

define_advect!(
    advect_voxel_32,
    flux_32,
    limited_slope_32,
    M32,
    q_conc_32,
    q_round_32
);
define_advect!(
    advect_voxel_64,
    flux_64,
    limited_slope_64,
    M64,
    q_conc_64,
    q_round_64
);

/// The linear index of a voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
///
/// The kernel's own copy of `world::Grid::index`, for the reason `diffuse.rs`
/// gives: `kernels/` depends on `numeric/` and on nothing else. There are now
/// **three** copies of the neighbourhood in the project, and this is the first
/// with a second neighbour in it — which neither of the other two checks.
/// `the_axis_neighbourhood_agrees_with_the_grid` is where the three are held
/// together.
#[inline(always)]
fn index(p: &AdvectParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

/// One step down an axis, or what the boundary says instead.
///
/// A copy of `diffuse.rs`, deliberately, and a comparison rather than a modulo
/// for the same reason: an integer division is the expensive instruction of the
/// whole lookup on a GPU.
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

/// Voxels along one axis.
#[inline(always)]
fn extent_on_axis(p: &AdvectParams, axis: u32) -> u32 {
    if axis == 0 {
        p.nx
    } else if axis == 1 {
        p.ny
    } else {
        p.nz
    }
}

/// The coordinate of a voxel along one axis.
///
/// Separate from [`neighbour_along`] because the closed-face branch needs the
/// coordinate and not the neighbour: across a closed face the neighbour of the
/// first voxel *is* the first voxel, so "there is no neighbour" and "the
/// neighbour is me" are the same number, and only one of those two meanings
/// makes a wall.
#[inline(always)]
fn coord_on_axis(p: &AdvectParams, idx: u32, axis: u32) -> u32 {
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    if axis == 0 {
        x
    } else if axis == 1 {
        y
    } else {
        z
    }
}

/// Whether an axis wraps.
///
/// Periodicity is a property of an axis, and `world::Grid::new` refuses a grid
/// whose two faces on one axis disagree. `AdvectParams` carries no such
/// guarantee — it carries a raw mask — so the invariant is asserted here, in
/// debug, at the one place that reads it. Without the assertion a mask with one
/// bit of a pair set would wrap the lower face of the axis and fold the upper
/// one onto itself, and matter would vanish at one end with no symptom other
/// than a ledger residual.
#[inline(always)]
fn axis_is_periodic(p: &AdvectParams, axis: u32) -> bool {
    let lower = (p.periodic_mask >> (2 * axis)) & 1 == 1;
    let upper = (p.periodic_mask >> (2 * axis + 1)) & 1 == 1;
    debug_assert_eq!(
        lower, upper,
        "axis {axis} is periodic on one face and not on the other (mask {:#08b}): \
         periodicity is a property of an axis (world::Grid::new)",
        p.periodic_mask
    );
    lower
}

/// The neighbour of a voxel one step along an axis: `direction == 0` down,
/// anything else up.
///
/// An integer rather than a `bool` for the reason `Face` is an integer in
/// `diffuse.rs`: this is an argument the host and the shader have to agree
/// about, and WGSL indexes with integers.
///
/// A closed face returns the voxel itself. Unlike diffusion, that is **not**
/// enough to make the face carry nothing — see [`AdvectParams::periodic_mask`] —
/// but it is what makes the *limiter* fall back to first order beside a wall for
/// free: the cell upwind of a donor standing against the wall collapses onto the
/// donor, `du` is zero, and the limited slope is zero.
///
/// An **exchange** face returns the ghost cell, and the ghost cell is its own
/// neighbour in both directions. Both halves are load-bearing and they fail
/// apart. Without the first, step `c` never reads the reservoir and the lid is
/// sealed for advection while it vents for diffusion. Without the second, the
/// second neighbour of the stencil — `up(up)` on inflow — decodes `nx*ny*nz` as
/// a coordinate past the end of the axis and reads outside the lane. And it is
/// the second that gives `exchange_inflow_falls_back_to_first_order` its
/// mechanism: the cell upwind of the ghost *is* the ghost, so `du == 0` and van
/// Leer answers zero (ADR-054, ADR-059).
#[inline(always)]
fn neighbour_along(p: &AdvectParams, idx: u32, axis: u32, direction: u32) -> u32 {
    let ghost = p.nx * p.ny * p.nz;
    if idx == ghost {
        return ghost;
    }

    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    let periodic = axis_is_periodic(p, axis);
    let extent = extent_on_axis(p, axis);
    let coord = if axis == 0 {
        x
    } else if axis == 1 {
        y
    } else {
        z
    };
    let exchanging = if direction == 0 {
        (p.exchange_mask >> (2 * axis)) & 1 == 1
    } else {
        (p.exchange_mask >> (2 * axis + 1)) & 1 == 1
    };
    let against_the_face = if direction == 0 {
        coord == 0
    } else {
        coord + 1 == extent
    };
    if exchanging && against_the_face {
        return ghost;
    }
    let moved = if direction == 0 {
        step_down(coord, extent, periodic)
    } else {
        step_up(coord, extent, periodic)
    };

    let mut nbx = x;
    let mut nby = y;
    let mut nbz = z;
    if axis == 0 {
        nbx = moved;
    } else if axis == 1 {
        nby = moved;
    } else {
        nbz = moved;
    }

    index(p, nbx, nby, nbz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, Face, Grid};

    /// Deliberately non-cubic, and deliberately the extents the diffusion tests
    /// use: a cubic grid hides every bug that mixes up the axes, because all
    /// three strides are equal.
    const NX: u32 = 3;
    const NY: u32 = 4;
    const NZ: u32 = 5;
    const N_VOXELS: u32 = NX * NY * NZ;

    /// All six faces periodic.
    const TORUS: u32 = 0b11_1111;
    /// The eco-regime default of SPEC section 1.6, minus the face that cannot be
    /// built yet: periodic in X and Y, solid floor and lid in Z.
    const FLOORED: u32 = 0b00_1111;

    /// The velocity budget of ADR-069: `dx/(6*dt)` is 16.7 um/s, which is a
    /// Courant number of 0.167 at `dx = 100 um` and `dt = 1 s`. The working
    /// number of the project rather than a made-up one.
    const BUDGET: f64 = 0.167;

    /// The length of one lane, and of one axis's slice of a Courant buffer:
    /// the voxels and the ghost cell (ADR-059).
    const LANE_LEN: u32 = N_VOXELS + 1;

    fn params(periodic_mask: u32, axis: u32) -> AdvectParams {
        AdvectParams {
            nx: NX,
            ny: NY,
            nz: NZ,
            periodic_mask,
            exchange_mask: 0,
            axis,
        }
    }

    /// A Courant field with the same number on every face of every axis.
    fn uniform_courant(value: f64) -> Vec<Q> {
        vec![Q::from_f64(value); (3 * LANE_LEN) as usize]
    }

    /// A Courant field that differs from face to face, so that the divergence is
    /// nonzero nearly everywhere and conservation cannot come out right by
    /// symmetry.
    fn divergent_courant() -> Vec<Q> {
        (0..3 * LANE_LEN)
            .map(|i| Q::from_f64(((f64::from(i) * 37.0) % 19.0 - 9.0) / 20.0))
            .collect()
    }

    /// A value that depends on the index in a way no symmetry of the stencil can
    /// cancel by accident.
    fn pattern(idx: u32) -> i64 {
        (i64::from(idx) * 7919) % 1000 - 400
    }

    fn total_32(buffer: &[M32]) -> i64 {
        buffer.iter().map(|v| v.to_i64()).sum()
    }

    fn total_64(buffer: &[M64]) -> i64 {
        buffer.iter().map(|v| v.to_i64()).sum()
    }

    /// One lane: the voxels, then a ghost cell nobody reads.
    ///
    /// Every fixture in this module has `exchange_mask == 0`, so the ghost is
    /// unreachable through the neighbourhood — it is here because the *slice* is
    /// a lane and a lane is `n_voxels + 1` long (ADR-059), and a kernel handed a
    /// shorter one would index past its end on the first exchanging face.
    fn lane_32(f: impl Fn(u32) -> i64) -> Vec<M32> {
        let mut lane: Vec<M32> = (0..N_VOXELS).map(|i| M32::new(f(i) as i32)).collect();
        lane.push(M32::ZERO);
        lane
    }

    fn lane_64(f: impl Fn(u32) -> i64) -> Vec<M64> {
        let mut lane: Vec<M64> = (0..N_VOXELS).map(|i| M64::new(f(i))).collect();
        lane.push(M64::ZERO);
        lane
    }

    fn empty_32() -> Vec<M32> {
        vec![M32::ZERO; LANE_LEN as usize]
    }

    fn empty_64() -> Vec<M64> {
        vec![M64::ZERO; LANE_LEN as usize]
    }

    fn sweep_32(src: &[M32], dst: &mut [M32], courant: &[Q], p: &AdvectParams, n: u32) {
        for idx in 0..n {
            advect_voxel_32(src, courant, dst, p, idx);
        }
    }

    fn sweep_64(src: &[M64], dst: &mut [M64], courant: &[Q], p: &AdvectParams, n: u32) {
        for idx in 0..n {
            advect_voxel_64(src, courant, dst, p, idx);
        }
    }

    /// The first-order donor flux, written out, so that the tests asking "did the
    /// limiter switch itself off here" have something exact to compare against.
    fn donor_flux_32(donor: M32, courant: Q) -> M32 {
        q_round_32(qmul(courant, q_conc_32(donor, Q::ONE)))
    }

    /// The three copies of the neighbourhood in this project — `world/grid.rs`,
    /// `kernels/diffuse.rs` and this file — have to agree, and this is the first
    /// with a **second** neighbour in it. Nothing else looks at `down(down)` or
    /// `up(up)`, and a copy that disagreed there would leave the scheme
    /// conservative and stable and merely stop it being second order: no
    /// conservation test, no stability test and no ledger would move.
    #[test]
    fn the_axis_neighbourhood_agrees_with_the_grid() {
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
            (0, [Boundary::Closed; 6]),
        ];

        for (mask, boundary) in cases {
            let grid = Grid::new(NX, NY, NZ, boundary).unwrap();
            assert_eq!(grid.n_voxels(), N_VOXELS);

            for axis in 0..3u32 {
                let p = params(mask, axis);
                let (down_face, up_face) = match axis {
                    0 => (Face::XMinus, Face::XPlus),
                    1 => (Face::YMinus, Face::YPlus),
                    _ => (Face::ZMinus, Face::ZPlus),
                };

                for idx in 0..N_VOXELS {
                    let (x, y, z) = grid.coords(idx);
                    assert_eq!(index(&p, x, y, z), grid.index(x, y, z));
                    assert_eq!(
                        coord_on_axis(&p, idx, axis),
                        [x, y, z][axis as usize],
                        "voxel {idx}, axis {axis}"
                    );

                    let down = neighbour_along(&p, idx, axis, 0);
                    let up = neighbour_along(&p, idx, axis, 1);
                    assert_eq!(down, grid.neighbour(idx, down_face), "voxel {idx}");
                    assert_eq!(up, grid.neighbour(idx, up_face), "voxel {idx}");

                    // The second neighbour, which is new here.
                    assert_eq!(
                        neighbour_along(&p, down, axis, 0),
                        grid.neighbour(grid.neighbour(idx, down_face), down_face),
                        "voxel {idx}, axis {axis}, two steps down"
                    );
                    assert_eq!(
                        neighbour_along(&p, up, axis, 1),
                        grid.neighbour(grid.neighbour(idx, up_face), up_face),
                        "voxel {idx}, axis {axis}, two steps up"
                    );
                }
            }
        }
    }

    /// The most important difference from diffusion, written as a fact.
    ///
    /// Diffusion seals a wall without a branch, because an antisymmetric
    /// function of two equal arguments is zero. The same arrangement leaks here:
    /// the donor term is proportional to the amount, not to a difference. Both
    /// halves are asserted — the bare flux function through a wall is *not*
    /// zero, and the kernel through a wall moves nothing whatever velocity sits
    /// in the Courant cell behind it.
    #[test]
    fn a_closed_face_carries_nothing_at_any_velocity() {
        // One: the corollary diffusion rests on does not hold.
        for value in [BUDGET, -BUDGET, 0.5, 1.0, -1.0] {
            let speed = Q::from_f64(value);
            // Amounts large enough that `round(c*a)` is not zero for arithmetic
            // reasons: the claim is that a wall of equal arguments carries the
            // donor flux, not that any amount at all survives the rounding.
            for amount in [7i32, -13, 41, 1_000_003] {
                let a = M32::new(amount);
                assert_ne!(
                    flux_32(a, a, a, a, speed),
                    M32::ZERO,
                    "a wall built out of equal arguments would leak {amount} at {value}"
                );
                assert_eq!(flux_32(a, a, a, a, speed), donor_flux_32(a, speed));
            }
        }

        // Two: the kernel does seal it. On a closed axis the cell
        // `courant[base + idx]` of a floor voxel belongs to the wall face and to
        // nothing else, so putting any velocity at all into it must change
        // nothing anywhere.
        let p = params(FLOORED, 2);
        let base = 2 * N_VOXELS;
        let src = lane_32(pattern);

        let mut quiet = uniform_courant(BUDGET);
        let mut loud = uniform_courant(BUDGET);
        for wall in 0..(NX * NY) {
            quiet[(base + wall) as usize] = Q::ZERO;
            loud[(base + wall) as usize] = Q::from_f64(-0.9);
        }

        let mut with_quiet = empty_32();
        let mut with_loud = empty_32();
        sweep_32(&src, &mut with_quiet, &quiet, &p, N_VOXELS);
        sweep_32(&src, &mut with_loud, &loud, &p, N_VOXELS);

        assert_eq!(
            with_quiet, with_loud,
            "the velocity on the closed floor reached the domain"
        );
        assert_eq!(total_32(&src), total_32(&with_quiet));
        // And the run was not vacuous: the axis moved matter elsewhere, and the
        // voxel against the lid still advects. The silent version of this bug
        // leaves that voxel frozen with every sum closing.
        assert_ne!(with_quiet, src);
        let lid = index(&p, 1, 2, NZ - 1) as usize;
        assert_ne!(with_quiet[lid], src[lid]);
    }

    /// A periodic axis one voxel deep: the lower and the upper face are the same
    /// face. Both read the same Courant cell and build the same stencil, so the
    /// two fluxes are equal bit for bit and the net is exactly zero — not
    /// because the flux is zero, which it is not, but because there is only one
    /// of it. This is the road a flat two-dimensional scenario takes.
    #[test]
    fn an_axis_one_voxel_deep_moves_nothing() {
        const N: u32 = 16;
        let p = AdvectParams {
            nx: 4,
            ny: 4,
            nz: 1,
            periodic_mask: TORUS,
            exchange_mask: 0,
            axis: 2,
        };
        let src: Vec<M32> = (0..N + 1)
            .map(|i| M32::new(pattern(i) as i32 + 500))
            .collect();
        let courant: Vec<Q> = (0..3 * (N + 1))
            .map(|i| Q::from_f64(0.1 * f64::from(i % 7)))
            .collect();

        let mut dst = vec![M32::ZERO; (N + 1) as usize];
        for idx in 0..N {
            advect_voxel_32(&src, &courant, &mut dst, &p, idx);
        }
        assert_eq!(dst[..N as usize], src[..N as usize]);

        // And the flux across that face is not zero, so the assertion above is
        // about the pairing rather than about a dead velocity.
        assert_ne!(
            flux_32(
                src[0],
                src[0],
                src[0],
                src[0],
                courant[(2 * (N + 1)) as usize]
            ),
            M32::ZERO
        );
    }

    /// The sign convention, and the choice of donor. Two defects hide here, and
    /// both conserve matter exactly and leave every ledger green: a host that
    /// folded `u` as an outgoing face velocity sends matter upstream, and a
    /// donor picked by the canonical orientation instead of by the sign of `c`
    /// is an upwind difference taken downwind — an unconditionally unstable
    /// scheme that reads as mixing on a volume render.
    #[test]
    fn matter_moves_downstream_not_upstream() {
        const N: u32 = 24;
        let p = AdvectParams {
            nx: N,
            ny: 1,
            nz: 1,
            periodic_mask: TORUS,
            exchange_mask: 0,
            axis: 0,
        };

        let bump = |i: u32| {
            if (8..=10).contains(&i) {
                1_000_000i64
            } else {
                0
            }
        };
        let centre = |buffer: &[i64]| {
            let total: i64 = buffer.iter().sum();
            let moment: i64 = buffer.iter().enumerate().map(|(i, &a)| i as i64 * a).sum();
            moment as f64 / total as f64
        };
        let start = centre(&(0..N).map(bump).collect::<Vec<_>>());

        for (value, expected) in [(0.5f64, 1.0f64), (-0.5, -1.0)] {
            let courant = vec![Q::from_f64(value); (3 * (N + 1)) as usize];

            let mut narrow: Vec<M32> = (0..N + 1).map(|i| M32::new(bump(i) as i32)).collect();
            let mut wide: Vec<M64> = (0..N + 1).map(|i| M64::new(bump(i))).collect();
            for _ in 0..6 {
                let mut dst = vec![M32::ZERO; (N + 1) as usize];
                sweep_32(&narrow, &mut dst, &courant, &p, N);
                narrow = dst;

                let mut dst = vec![M64::ZERO; (N + 1) as usize];
                sweep_64(&wide, &mut dst, &courant, &p, N);
                wide = dst;
            }

            let end = centre(&narrow.iter().map(|v| v.to_i64()).collect::<Vec<_>>());
            assert!(
                (end - start) * expected > 0.0,
                "the bump moved from {start} to {end} at a Courant number of \
                 {value}: the velocity convention points upstream"
            );
            let end_wide = centre(&wide.iter().map(|v| v.to_i64()).collect::<Vec<_>>());
            assert!((end_wide - start) * expected > 0.0);
        }
    }

    /// A uniform field under a uniform velocity is a fixed point at **any**
    /// Courant number, including the water pool of ADR-040 where the flux itself
    /// is coarsened to thousands of units. The two faces of a voxel see the same
    /// four equal amounts and the same number, so they produce the same integer
    /// and cancel. This fails exactly when the two faces are computed by
    /// different expressions, or by one expression associated two ways.
    #[test]
    fn a_uniform_field_with_a_uniform_velocity_is_a_fixed_point() {
        for axis in 0..3u32 {
            let p = params(TORUS, axis);
            for value in [0.0, BUDGET, -BUDGET, 0.5, -0.5, 1.0, -1.0, 0.999] {
                let courant = uniform_courant(value);

                let src = lane_32(|_| 1_000_003);
                let mut dst = lane_32(|_| 1_000_003);
                sweep_32(&src, &mut dst, &courant, &p, N_VOXELS);
                assert_eq!(src, dst, "axis {axis}, c {value}");

                // Water, in the band the corpus tests it in — `numeric/m.rs`,
                // `world/field.rs`, `tests/acceptance_ledger.rs` and
                // `process/diffuse.rs` all use this same literal: 5.1e12 units,
                // an order above the 5.1e11 ADR-040 derives for the registry's
                // water, where one f32 step is 2^19 = 524 288 and the flux comes
                // back quantised to hundreds of thousands.
                let src = lane_64(|_| 5_100_000_000_000);
                let mut dst = lane_64(|_| 5_100_000_000_000);
                sweep_64(&src, &mut dst, &courant, &p, N_VOXELS);
                assert_eq!(src, dst, "axis {axis}, c {value}, water");
            }
        }
    }

    /// At `|c| = 1` the factor `(1 - |c|)` kills the antidiffusive term and the
    /// flux is exactly `c * donor`: a whole voxel of transfer, which is the exact
    /// answer. This is what pins that factor. Losing it is nearly invisible at
    /// the small Courant numbers of the velocity budget and removes the TVD
    /// property outright.
    #[test]
    fn at_courant_one_the_flux_is_pure_donor_transfer() {
        let quads = [
            (0i32, 0, 1000, 3000),
            (-500, 200, 900, 1500),
            (7, 7, 7, 7),
            (1_000_000, 3, -4, 999),
        ];
        for (ll, l, r, rr) in quads {
            for value in [1.0f64, -1.0] {
                let speed = Q::from_f64(value);
                let donor = if value > 0.0 { l } else { r };
                assert_eq!(
                    flux_32(M32::new(ll), M32::new(l), M32::new(r), M32::new(rr), speed),
                    donor_flux_32(M32::new(donor), speed),
                    "quad {ll} {l} {r} {rr} at {value}"
                );
                assert_eq!(
                    flux_64(
                        M64::new(ll.into()),
                        M64::new(l.into()),
                        M64::new(r.into()),
                        M64::new(rr.into()),
                        speed
                    )
                    .to_i64(),
                    donor_flux_32(M32::new(donor), speed).to_i64()
                );
            }
        }
    }

    /// The limiter is van Leer, and not either of the two limiters ADR-054
    /// rejects by name.
    ///
    /// The assertion that suggests itself here is `|phi| <= 2*min(|du|, |df|)`
    /// with the sign of `df` — Sweby's second-order TVD region — and on its own
    /// it is worth nothing at all. Saying why is half of what this test is for.
    /// That bound is the **upper envelope of the whole family**, not a fence
    /// around van Leer: minmod answers `min(|du|, |df|)` and sits well inside
    /// it, superbee is `psi = max(min(2r, 1), min(r, 2))` and therefore sits
    /// exactly *on* it, and van Leer runs between the two. Membership in the
    /// region is satisfied by construction by every limiter anyone would
    /// substitute, so a test that asserted only that would pass unchanged on
    /// either rejection — and both rejections in ADR-054 are scientific:
    /// superbee *manufactures* the sharp fronts the project is searching for,
    /// and minmod leaves about half the numerical diffusion the decision exists
    /// to remove.
    ///
    /// So the value is recomputed below in `f64`, independently of the harmonic
    /// form the kernel evaluates, and asserted to within the roundings of an
    /// `f32`. The three limiters are three different numbers everywhere except
    /// on the diagonal `|du| == |df|`, where all three answer `df` — which is
    /// why the exclusion is stated at asymmetric ratios, and why a table of
    /// equal ones would have proved nothing.
    #[test]
    fn the_limiter_is_van_leer_and_not_minmod_or_superbee() {
        // Five `f32` roundings stand between the two integers and `phi`: the two
        // conversions, the product, the sum and the quotient. The doubling is
        // exact. `4 * f32::EPSILON` is `2^-21`, comfortably above `5 * 2^-24`
        // and orders of magnitude below the distance to either neighbouring
        // limiter, which is a factor rather than an ulp.
        let slack = 4.0 * f64::from(f32::EPSILON);

        let values = [
            -1_000_000i64,
            -12345,
            -100,
            -7,
            -1,
            0,
            1,
            7,
            100,
            12345,
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

                // The number itself, recomputed. This is the assertion the two
                // substitutions fail: at every ratio other than one they answer
                // something else, by a factor.
                let (du_f, df_f) = (du as f64, df as f64);
                let van_leer = 2.0 * du_f * df_f / (du_f + df_f);
                let phi = narrow.debug_f64();
                assert!(
                    (phi - van_leer).abs() <= slack * van_leer.abs(),
                    "phi {phi} is not the van Leer slope {van_leer} at ({du}, {df})"
                );

                // The sign and the region are true of every limiter in the
                // family, and both are still worth asserting: the first is what
                // a sign slip in the fallback branch breaks, the second what a
                // second-order correction with no bound at all breaks.
                assert_eq!(
                    phi.signum(),
                    df.signum() as f64,
                    "phi {phi} does not follow the sign of df at ({du}, {df})"
                );
                let envelope = 2.0 * du_f.abs().min(df_f.abs()) * (1.0 + slack);
                assert!(
                    phi.abs() <= envelope,
                    "phi {phi} is outside the TVD region at ({du}, {df}), \
                     envelope {envelope}"
                );
            }
        }

        // The exclusion, spelled out at ratios where the three limiters
        // separate. At any ratio of two or more minmod answers exactly
        // `min(|du|, |df|)` and superbee exactly `2*min(|du|, |df|)`; van Leer
        // answers the harmonic mean, strictly between them. At `du = 3*df` the
        // three are `1.0`, `1.5` and `2.0` times `df`.
        for (du, df) in [
            (3i64, 1i64),
            (1, 3),
            (-3, -1),
            (-1, -3),
            (2, 1),
            (9, 3),
            (300_000, 100_000),
            (100_000, 300_000),
        ] {
            let phi = limited_slope_64(M64::new(du), M64::new(df))
                .debug_f64()
                .abs();
            let minmod = du.abs().min(df.abs()) as f64;
            let superbee = 2.0 * minmod;
            assert!(
                phi > minmod * (1.0 + slack) && phi < superbee * (1.0 - slack),
                "at ({du}, {df}) the limiter answered {phi}: minmod would answer \
                 {minmod} and superbee {superbee}, and ADR-054 rejects both"
            );
        }
    }

    /// Beside a wall the limiter switches itself off, and it costs nothing to
    /// arrange: the cell upwind of a donor standing against the wall folds onto
    /// the donor, `du` is zero, and van Leer is zero at a zero numerator. This
    /// is the mechanism `ACCEPTANCE.md` calls
    /// `exchange_inflow_falls_back_to_first_order`, checked on the boundary that
    /// can be built today — `Grid::new` refuses `exchange`.
    #[test]
    fn a_wall_adjacent_face_falls_back_to_first_order() {
        let p = params(FLOORED, 2);
        let first = index(&p, 1, 2, 0);
        let second = index(&p, 1, 2, 1);

        // The stencil of the face between z = 0 and z = 1 folds: the cell below
        // the floor voxel is the floor voxel.
        assert_eq!(neighbour_along(&p, first, 2, 0), first);
        assert_eq!(
            neighbour_along(&p, neighbour_along(&p, second, 2, 0), 2, 0),
            first
        );

        let speed = Q::from_f64(BUDGET);
        let low = M32::new(400);
        let high = M32::new(900);
        let far_high = M32::new(1500);
        assert_eq!(limited_slope_32(low - low, high - low), Q::ZERO);
        assert_eq!(
            flux_32(low, low, high, far_high, speed),
            donor_flux_32(low, speed),
            "the face against the floor is not first order"
        );
    }

    /// Exactly, not to within a tolerance. Both voxels of a face read one
    /// Courant cell and build one stencil, so they compute one integer, and one
    /// adds what the other subtracts (ADR-054, ADR-034).
    #[test]
    fn a_sweep_conserves_the_total_exactly_in_both_widths() {
        for mask in [TORUS, FLOORED, 0] {
            for axis in 0..3u32 {
                let p = params(mask, axis);
                let fields = [
                    uniform_courant(BUDGET),
                    uniform_courant(-0.5),
                    divergent_courant(),
                ];
                for courant in &fields {
                    let src = lane_32(pattern);
                    let mut dst = empty_32();
                    sweep_32(&src, &mut dst, courant, &p, N_VOXELS);
                    assert_eq!(
                        total_32(&src),
                        total_32(&dst),
                        "mask {mask:#08b}, axis {axis}"
                    );

                    let src = lane_64(|i| 5_100_000_000_000 + pattern(i));
                    let mut dst = empty_64();
                    sweep_64(&src, &mut dst, courant, &p, N_VOXELS);
                    assert_eq!(
                        total_64(&src),
                        total_64(&dst),
                        "mask {mask:#08b}, axis {axis}, water"
                    );
                }
            }
        }
    }

    /// Gather form buys this: nothing a voxel writes is visible to any other
    /// voxel in the same application. On the GPU there is no traversal order at
    /// all, so anything this test would catch is unfixable there.
    #[test]
    fn the_result_does_not_depend_on_the_traversal_order() {
        let p = params(TORUS, 1);
        let courant = divergent_courant();
        let src = lane_32(pattern);

        let mut forwards = empty_32();
        for idx in 0..N_VOXELS {
            advect_voxel_32(&src, &courant, &mut forwards, &p, idx);
        }

        let mut backwards = empty_32();
        for idx in (0..N_VOXELS).rev() {
            advect_voxel_32(&src, &courant, &mut backwards, &p, idx);
        }

        assert_eq!(forwards, backwards);
        assert_ne!(forwards, src);
    }
}
