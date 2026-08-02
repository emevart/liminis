//! Downwelling light: Beer-Lambert attenuation, one pass down Z.
//!
//! ```text
//! I[z-1] = I[z] * exp(-(k_w*water + k_b*biomass + k_d*detritus + k_m*mineral)*dz)
//! ```
//!
//! One pass from the top of the column to the floor, `O(N)` over the domain
//! (SPEC section 4.6). The four coefficients of that formula are not here: the
//! host folds each of them with `dz` and with what one storage unit is worth,
//! and hands the kernel one number per attenuator ([`Attenuators::coeff`]).
//!
//! # The absorbed light is not stored, and must not be
//!
//! The intensity falls by `I[z] - I[z-1]` across voxel `z`, and that energy has
//! an address — the enthalpy of the voxel (ADR-028). This kernel does not credit
//! it, does not accumulate it, and does not touch a channel counter. ADR-049
//! settles who does: the same fold kernel that collects the energy of reactions
//! computes `I[z] - I[z-1]` **from the stored light field**, on the fly, and no
//! second field on the fine grid exists. Absorption is a *derived* quantity, not
//! a state of its own; storing it beside the field it is computed from would be
//! two sources of truth about one thing.
//!
//! Being derived puts one obligation on this kernel and only one: every term of
//! that difference has to be reachable. It is — see the convention on
//! [`light_column`], which is chosen for exactly that and for nothing else.
//!
//! This paragraph is here because a missing accumulator reads as an oversight.
//! It is a decision, and adding `absorbed[idx] = ...` below would reverse
//! ADR-049 without saying so.
//!
//! # What leaves through the floor is nobody's
//!
//! The beam that reaches `z = 0` and passes out under it is credited to nothing
//! and needs no channel (ADR-059: `SOLAR_IN` counts what was absorbed, not what
//! fell). It is stored — it is `dst[z = 0]`, and it has to be stored, because it
//! is also the lower term of the absorption of voxel `0` — but no term of the
//! fold's sum is it. A kernel that clamped the beam to zero at the floor, or
//! dumped the remainder into the bottom voxel, would be undoing that decision.
//!
//! # Shape on the GPU: dispatch over XY, loop over Z
//!
//! This is the only kernel in the project where a voxel needs a result computed
//! for its neighbour, and the dependency runs along one axis. It maps onto a
//! **two-dimensional dispatch over `(nx, ny)`**: `idx` comes from
//! `global_invocation_id.xy`, the loop over `z` runs inside one invocation, and
//! the running intensity lives in a register. No `workgroupBarrier`, no
//! `storageBarrier`, no atomics; `nz` iterations per thread, which is the `O(N)`
//! SPEC section 4.6 claims.
//!
//! ADR-034 is not bent by this. What that decision buys is two things — no
//! atomics, and no dependence on traversal order — and both hold here word for
//! word: an invocation owns `nz` cells, the write sets of two invocations are
//! disjoint, and no invocation reads anything another invocation wrote. Only
//! "one invocation, one cell" is relaxed, and that is a statement about
//! granularity, not about conservation. Conservation is not at stake at all: `I`
//! belongs to class `Q` (QUANTITIES.md section 5), it is not in the ledger, and
//! it conserves nothing.
//!
//! The tempting shortcut is to read the neighbour above as
//! `dst[index(x, y, z + 1)]`. On the CPU, and inside a single GPU invocation,
//! that produces the same numbers — and it is a read from the buffer being
//! written, forbidden by `.claude/rules/kernels.md`, which breaks the day the
//! pass is split across two dispatches. The running scalar is the rule, not an
//! optimisation, and no result can tell the two apart.
//!
//! # The dispatch domain is part of the contract
//!
//! `idx` runs over `0..nx*ny`, **not** over `0..n_voxels`. The two ways to get
//! that wrong fail differently, and the difference is worth stating because only
//! one of them is quiet.
//!
//! A host that dispatched per **voxel** does not merely pay `nz` times the work.
//! Past `idx = nx*ny` the decode below gives `y >= ny`, and [`index`] then
//! addresses past the end of the field: on the CPU a panic, in WGSL an
//! out-of-bounds store, which the specification leaves free to land on another
//! binding. That case is loud, and `debug_assert!` in [`light_column`] makes it
//! loud at the first invocation past the domain rather than at the first one
//! whose column happens to run off the end.
//!
//! The quiet case is the other one: a host that dispatches per column and
//! decodes `idx` as a **voxel** index gets the identical field, because the
//! first `nx*ny` voxel indices are exactly the plane `z = 0`. No assertion over
//! values can see that, and nothing here can — which is why `x` and `y` come out
//! of `idx` by `nx` alone and never by dividing out a plane, and why the domain
//! is named in words as well.
//!
//! # Addressing amounts: lanes, not substance indices
//!
//! The reaction skeleton in `ARCHITECTURE.md` writes `s * n_voxels + idx`.
//! ADR-056 revoked that consequence: a substance resolves to a *pair* — storage
//! width, and a lane index **within its own width class** — and the world holds
//! exactly two compact buffers. So the address here is
//! `lane[a] * n_voxels + at`, with `lane[a]` handed down by the host through
//! `Registry::lane_of`. The skeleton is stale on this point; a kernel that
//! copied it reads another substance's amounts and stays green on its own,
//! because nothing about light is conserved and the profile `I(z)` stays
//! monotone and plausible either way.

use crate::numeric::{M32, M64, Q, q_conc_32, q_conc_64, qadd, qexp, qmul, qsub};

/// Parameters of one application. Scalars only: in WGSL this is a uniform
/// buffer, and every number in it is a place where the host's scale can drift
/// away from the kernel's (`ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug)]
pub struct LightParams {
    /// Voxels along X.
    pub nx: u32,
    /// Voxels along Y.
    pub ny: u32,
    /// Voxels along Z.
    pub nz: u32,
    /// `nx*ny*nz`, the stride between two lanes of the same amount buffer. It
    /// arrives rather than being recomputed from the extents, by the same rule
    /// that brings `alpha` to the diffusion kernel already folded: the host
    /// knows it, and a product of three `u32` inside a shader is one more place
    /// for an overflow that shows up only on the largest grid anybody runs.
    pub n_voxels: u32,
    /// The stride of an amount lane. On a `world::Field` it is `n_voxels + 1`
    /// — the voxels and the ghost cell after them (ADR-059) — and the host is
    /// the only place that knows so; this kernel is handed the number.
    ///
    /// Separate from [`LightParams::n_voxels`] because the two mean different things
    /// and only one of them is an address. A dispatch runs over `0..n_voxels`;
    /// an amount lives at `lane[s] * lane_len + idx`. Using the voxel count as
    /// the stride reads one lane short of where the substance is, and on the
    /// registry the project carries that is **right** for the first lane of a
    /// width class and wrong by one element per lane after it — which is the
    /// same silent shape ADR-056 warns about for `s * n_voxels + idx`.
    pub lane_len: u32,
    /// How many entries of [`Attenuators`] to sum. The table may be longer; the
    /// scenario decides how many substances darken the water.
    pub n_attenuators: u32,
    /// Bit `a` set: entry `a` of the attenuator table reads the 64-bit slice
    /// (ADR-040, ADR-056).
    ///
    /// The bit indexes the **table entry** — not the substance, and above all
    /// not the voxel. That is the same property ADR-040 asks of the reaction
    /// kernel and for the same reason: the branch is then uniform across every
    /// column of the dispatch, so on the GPU the warp does not diverge.
    pub width_mask: u32,
    /// Irradiance falling on the top face of the topmost voxel, W/m^2, already
    /// modulated by the day and the season on the host (SPEC section 7).
    ///
    /// A folded scalar rather than an amplitude and a period, and that is more
    /// than the usual folding rule here. There is no key for it anywhere:
    /// `CONFIG_SCHEMA.md` section 7 lists exactly `k_w`, `k_b`, `k_d` and `k_m`,
    /// and `QUANTITIES.md` section 5 has a row for `I` but none for `I0`, an
    /// amplitude or a period. Naming a number here would be inventing one, and a
    /// wrong constant is compensated by calibrating `k` forever.
    pub i_surface: Q,
}

/// The attenuator table: flat, read-only, `n_attenuators` long.
///
/// Built on the pattern of `Rx<'a>` in `ARCHITECTURE.md` — a named set of
/// bindings, each of which becomes one `var<storage, read>` in WGSL. Not an
/// interface: no trait, no `dyn`, no behaviour inside.
pub struct Attenuators<'a> {
    /// `lane[a]`: the index of the lane **within its own width class**, resolved
    /// by the host through `Registry::lane_of` (ADR-056). The kernel does no
    /// `s -> lane` resolution of its own and never sees the indirection at all.
    ///
    /// `lane == s` is not true anywhere, and that is a rule rather than an
    /// observation. A table built out of substance indices reads some other
    /// substance's amounts — attenuating by oxygen instead of by water — and
    /// nothing falls over: light is in no ledger, both halves of the invariant
    /// still close, and `I(z)` is still monotone and plausible. Only a test with
    /// *different* amounts in the two lanes can see it.
    pub lane: &'a [u32],
    /// `coeff[a]`: `k_a * dz * (concentration per storage unit)`, folded into one
    /// number, so the kernel knows neither `k`, nor `dz`, nor the derived scale
    /// `2^k_i` (ADR-039).
    ///
    /// `dz` is inside it, and leaving it out is the natural mistake to make when
    /// folding: in the eco regime `dx = 1e-4 m`, so the optical depth moves by
    /// four orders of magnitude and the beam either dies inside the first voxel
    /// or never dies at all. Both pictures read as "the coefficients are not
    /// calibrated yet" rather than as a bug, and calibrating `k_w ... k_m` hides
    /// it forever — the same class of silent error as the `1e-3` in `V_occ`
    /// (SPEC section 3). Only a test that takes `dz != 1` and compares against
    /// the analytic profile in **physical** depth `j*dz` will notice.
    pub coeff: &'a [Q],
}

/// One column of the light field, top to bottom.
///
/// `idx` runs over `0..nx*ny` — see the module header for why that cannot be
/// inferred from the numbers afterwards. Writes the `nz` cells of its own column
/// and not one cell of anybody else's; never reads `dst`, because the running
/// intensity lives in a local scalar.
///
/// # The convention of the field
///
/// `dst[index(x, y, z)]` is the intensity **leaving** voxel `z` through its
/// bottom face — equivalently, the intensity entering voxel `z-1`, and at
/// `z = 0` the beam that leaves the domain under the floor.
///
/// The convention is picked for one reason: ADR-049 makes absorption a derived
/// quantity, computed by the fold of step `i'` out of the stored field and its
/// `z`-neighbours (SPEC section 4.6), so the field is only adequate if **every**
/// term of every difference is reachable. Under this one it is. What voxel `z`
/// absorbed is
///
/// ```text
/// (z < nz-1)   dst[z+1] - dst[z]
/// (z == nz-1)  p.i_surface - dst[z]
/// ```
///
/// and the one term that is not in the field is `p.i_surface`, a folded scalar
/// the fold kernel receives the same way this kernel receives it. Nothing the
/// loop computes is thrown away, which is the checkable form of the same
/// statement: the last attenuation lands in `dst[0]`.
///
/// The mirror convention — "intensity **entering** voxel `z` through its top
/// face" — is the one that reads more naturally and is wrong here. It gives
/// `dst[nz-1] == p.i_surface` identically, an exact and pleasing assertion, and
/// it loses the floor: the absorption of voxel `0` is `dst[0]` minus the beam
/// leaving under the floor, and that beam is per-column data that no scalar can
/// stand in for. A whole layer of absorbed energy would then exist nowhere, and
/// nothing would object — light is class `Q` (QUANTITIES.md section 5), it is in
/// no ledger, and both halves of the invariant of ADR-028 would close over a
/// term neither side ever saw.
///
/// The price is that the assertion which tells the two conventions apart is no
/// longer an identity: it is `dst[nz-1] == p.i_surface * exp(-tau[nz-1])`,
/// checked against a hand-assembled value in
/// `the_top_cell_holds_what_leaves_the_topmost_voxel`. That test is also the one
/// that catches the whole field shifted by one voxel — `dst[at] = beam` before
/// the attenuation instead of after — and a pass that ran bottom-up.
///
/// # A skipped column carries last tick's answer
///
/// The field is rewritten in full every tick out of state `N` of the amounts, so
/// it needs no double buffering. The price is that a column nobody visited is
/// not "left unchanged": it holds the value of the previous tick, and the fold
/// of step `i'` will credit energy off it without complaint — while ADR-049
/// requires the light to belong to the *current* tick. Stale light looks exactly
/// like fresh light. The same paragraph is written about a voxel in
/// `kernels/diffuse.rs`; here it is about a column.
///
/// # A negative amount amplifies the beam
///
/// Amounts do go negative: `kernels/diffuse.rs` documents the overdraw and
/// `a_starved_voxel_is_overdrawn_rather_than_clamped` pins it. A negative amount
/// gives `tau < 0`, `qexp(-tau) > 1`, and the beam grows brighter as it sinks;
/// the fold then credits a negative absorption into `SOLAR_IN`. The energy
/// residual stays at zero while it happens, because the counter and the enthalpy
/// move together — so the mechanism built to catch "energy appeared out of
/// nowhere" is precisely the one that will not catch this.
///
/// The kernel does the arithmetic it was asked for, and
/// `a_negative_amount_amplifies_the_beam_rather_than_being_clamped` pins that it
/// does.
// TODO(positivity): whether to clamp is not decided. No record bounds `tau`
// below by zero or forbids a negative amount from reaching the light, and the
// same question is open for transport (`TODO(positivity)` in
// `kernels/diffuse.rs`). The honest options are the same three — a clamp in the
// kernel, a stricter bound at load time, or acceptance with a proven bound
// (ADR-068 is the precedent for the shape of that argument) — and choosing one
// is a decision about the physics, so it belongs in the journal. The test exists
// before the decision because without it the opposite behaviour arrives as an
// obvious fix and silently reverses ADR-049.
pub fn light_column(
    src32: &[M32],
    src64: &[M64],
    dst: &mut [Q],
    att: &Attenuators,
    p: &LightParams,
    idx: u32,
) {
    // The dispatch domain, asserted rather than only documented: past `nx*ny`
    // the decode below gives `y >= ny` and `index` runs off the end of the field
    // (see the module header). Debug-only, and a `debug_assert` on a parameter
    // invariant is what `axis_is_periodic` in `kernels/advect.rs` does with the
    // same argument.
    debug_assert!(
        idx < p.nx * p.ny,
        "light_column is dispatched over columns, not voxels: idx {idx} is past \
         nx*ny = {}",
        p.nx * p.ny
    );

    // A two-dimensional decode, by `nx` alone and never by dividing out a plane:
    // `idx` is a column, and a decode that took a `z` out of it would read as if
    // `idx` were a voxel index — the one confusion of the module header that no
    // value can betray.
    let y = idx / p.nx;
    let x = idx - y * p.nx;

    // The running intensity, in a register. Not `dst[index(x, y, z + 1)]`, which
    // gives the same numbers and is a read from the buffer being written.
    let mut beam = p.i_surface;

    // Top face down. `step` counts rather than `z`, because a `u32` loop that
    // decrements past zero wraps, and WGSL has no signed loop counter to reach
    // for instead.
    for step in 0..p.nz {
        let z = p.nz - 1 - step;
        let at = index(p, x, y, z);

        // `qsub(Q::ZERO, tau)` rather than `-tau`: `Q` implements no `Neg`
        // (ADR-022, `numeric/m.rs` gives one only to `M32`/`M64`). `qexp` rather
        // than `f32::exp`, because IEEE-754 requires bit-exact results only for
        // the four operations and the square root, so `exp` differs between
        // vendors in the last bits (NUMERIC.md section 2) — and this is the one
        // transcendental in the project whose result lands in a stored field.
        let tau = optical_depth(src32, src64, att, p, at);
        beam = qmul(beam, qexp(qsub(Q::ZERO, tau)));

        // Written *after* the attenuation of this voxel: the cell holds what
        // leaves voxel `z` through its bottom face (see the convention above).
        // The other ordering shifts the whole field up by one voxel, drops the
        // last attenuation on the floor, and is invisible on a uniform column.
        dst[at as usize] = beam;
    }

    // Every attenuation the loop computed is now in the field, the last one as
    // `dst[index(x, y, 0)]` — which is at once the lower term of what voxel `0`
    // absorbed and the beam that leaves under the floor. The second reading is
    // credited to nothing (ADR-059) and needs no channel; the first is what
    // keeps a whole layer of absorbed energy from existing nowhere.
}

/// The optical depth of one voxel: `sum_a coeff[a] * amount[lane[a], at]`.
///
/// One named crossing from `M` into `Q` and not one crossing back (ADR-060). The
/// composition is the one `flux_32` in `kernels/diffuse.rs` already uses and for
/// the same reason: `q_conc` with `Q::ONE` as the per-unit factor says "this
/// amount, as a number", and everything the amount has to be multiplied by is
/// folded into `coeff` already. Nothing here rounds, because nothing here goes
/// back to an integer — the one rounding on this path happens in the fold of
/// step `i'`, in another file, which is why `q_round` does not appear and why
/// the halves-away-from-zero rule answers for nothing here.
#[inline(always)]
fn optical_depth(src32: &[M32], src64: &[M64], att: &Attenuators, p: &LightParams, at: u32) -> Q {
    let mut tau = Q::ZERO;

    for a in 0..p.n_attenuators {
        // ADR-056, not the stale `s * n_voxels + idx` of the skeleton: the lane
        // is an index inside a width class and the host resolved it.
        let cell = (att.lane[a as usize] * p.lane_len + at) as usize;

        // The branch is on the table entry, the same value for every column of
        // the dispatch, so on the GPU it is a uniform jump and the warp holds
        // together (ADR-040).
        let amount = if p.width_mask & (1 << a) != 0 {
            q_conc_64(src64[cell], Q::ONE)
        } else {
            q_conc_32(src32[cell], Q::ONE)
        };

        tau = qadd(tau, qmul(att.coeff[a as usize], amount));
    }

    tau
}

/// The linear index of a voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
///
/// The kernel's own copy of `world::Grid::index`, for the reason `diffuse.rs`
/// keeps one: `kernels/` depends on `numeric/` and on nothing else, and a `Grid`
/// is a host type with no meaning in WGSL. `world/grid.rs` is the authority the
/// copy has to agree with, and `the_column_indexing_agrees_with_the_grid` is
/// what keeps the two from drifting.
#[inline(always)]
fn index(p: &LightParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric::{qexp, qmul, qsub};
    use crate::world::{Boundary, Grid};

    /// A deliberately non-cubic grid: on a cubic one every bug that swaps two
    /// axes is invisible, because all three strides are equal.
    const NX: u32 = 3;
    const NY: u32 = 4;
    const NZ: u32 = 5;
    const N_VOXELS: u32 = NX * NY * NZ;
    const N_COLUMNS: u32 = NX * NY;

    /// Two attenuators. Entry 0 reads the narrow buffer, entry 1 the wide one.
    const WIDTH_MASK: u32 = 0b10;

    /// The lanes are deliberately not the table entries (ADR-056). Were they
    /// equal, every test here would pass against a kernel with `lane == a`
    /// hard-coded into it.
    const LANE_32: u32 = 1;
    const LANE_64: u32 = 0;
    const N_LANES_32: u32 = 2;
    const N_LANES_64: u32 = 1;

    /// Arbitrary, and it has to be: no key names the incident irradiance
    /// anywhere in the corpus (see [`LightParams::i_surface`]). A test may
    /// substitute any number precisely because the kernel takes it folded.
    const INCIDENT: f64 = 137.0;

    /// Folded coefficients, one per table entry. Powers of two, so that the
    /// products below are exact and the assertions are about the kernel rather
    /// than about `f32`.
    const COEFF_32: f64 = 1.0 / 1024.0;
    const COEFF_64: f64 = 1.0 / 2048.0;

    const LANES: [u32; 2] = [LANE_32, LANE_64];

    fn params() -> LightParams {
        LightParams {
            lane_len: N_VOXELS,
            nx: NX,
            ny: NY,
            nz: NZ,
            n_voxels: N_VOXELS,
            n_attenuators: 2,
            width_mask: WIDTH_MASK,
            i_surface: Q::from_f64(INCIDENT),
        }
    }

    fn coeffs() -> [Q; 2] {
        [Q::from_f64(COEFF_32), Q::from_f64(COEFF_64)]
    }

    fn narrow() -> Vec<M32> {
        vec![M32::ZERO; (N_LANES_32 * N_VOXELS) as usize]
    }

    fn wide() -> Vec<M64> {
        vec![M64::ZERO; (N_LANES_64 * N_VOXELS) as usize]
    }

    fn put_32(src: &mut [M32], at: u32, value: i32) {
        src[(LANE_32 * N_VOXELS + at) as usize] = M32::new(value);
    }

    fn put_64(src: &mut [M64], at: u32, value: i64) {
        src[(LANE_64 * N_VOXELS + at) as usize] = M64::new(value);
    }

    /// The whole dispatch, the way the host runs it: one invocation per column.
    fn dispatch(src32: &[M32], src64: &[M64], p: &LightParams) -> Vec<Q> {
        let coeff = coeffs();
        let att = Attenuators {
            lane: &LANES,
            coeff: &coeff,
        };
        let mut dst = vec![Q::ZERO; N_VOXELS as usize];
        for idx in 0..N_COLUMNS {
            light_column(src32, src64, &mut dst, &att, p, idx);
        }
        dst
    }

    /// A per-voxel amount that no symmetry of the column can cancel by accident.
    fn pattern(at: u32) -> i32 {
        (at as i32 * 7919) % 97 + 3
    }

    fn filled() -> (Vec<M32>, Vec<M64>) {
        let mut src32 = narrow();
        let mut src64 = wide();
        for at in 0..N_VOXELS {
            put_32(&mut src32, at, pattern(at));
            put_64(&mut src64, at, i64::from(pattern(at)) * 2);
        }
        (src32, src64)
    }

    /// The optical depth of one voxel of [`filled`], assembled without the
    /// kernel.
    fn depth_of(at: u32) -> f64 {
        COEFF_32 * f64::from(pattern(at)) + COEFF_64 * f64::from(pattern(at)) * 2.0
    }

    #[test]
    fn the_top_cell_holds_what_leaves_the_topmost_voxel() {
        // The assertion that tells the stored convention apart from its mirror,
        // and the only one that catches the whole field being shifted by one
        // voxel — `dst[at] = beam` before or after `beam = qmul(beam, t)`. On a
        // uniform column both orderings produce a perfect exponential, and any
        // relative-error test writes the missing factor `exp(-tau)` off against
        // the choice of `I0`.
        //
        // Exact rather than approximate, because the expected value is built out
        // of the same `qexp` and the same `qmul` the kernel uses; what is
        // independent here is the *depth*, assembled by `depth_of` without the
        // kernel. Under the mirror convention the top cell would be `i_surface`
        // untouched, which the second assertion rules out.
        //
        // It is also the only line in the kernel tied to `Face::z_max = 5` in
        // `world::Grid`: light enters through the top, and a pass that ran
        // bottom-up would light the world from the floor with every other test
        // in this file still green.
        let p = params();
        let (src32, src64) = filled();
        let dst = dispatch(&src32, &src64, &p);

        for y in 0..NY {
            for x in 0..NX {
                let at = index(&p, x, y, NZ - 1);
                let expected = qmul(p.i_surface, qexp(qsub(Q::ZERO, Q::from_f64(depth_of(at)))));
                assert_eq!(dst[at as usize], expected, "column ({x}, {y})");
                assert!(dst[at as usize] < p.i_surface, "column ({x}, {y})");
            }
        }
    }

    #[test]
    fn the_optical_depth_of_a_voxel_attenuates_the_light_leaving_its_bottom_face() {
        // A column with exactly one opaque voxel pins *which* voxel's optical
        // depth attenuates the light through *which* face. A uniform column does
        // not answer that question at all: every voxel looks like every other,
        // and the profile is a perfect exponential under either answer.
        const Z_STAR: u32 = 3;
        const OPAQUE: i32 = 5000;

        let p = params();
        let mut src32 = narrow();
        let src64 = wide();
        put_32(&mut src32, index(&p, 1, 2, Z_STAR), OPAQUE);

        let dst = dispatch(&src32, &src64, &p);

        // Leaving every voxel above the opaque one, nothing has absorbed
        // anything. `qexp(0) == 1` exactly, so these are exact equalities.
        for z in Z_STAR + 1..NZ {
            assert_eq!(
                dst[index(&p, 1, 2, z) as usize],
                p.i_surface,
                "above the opaque voxel, z = {z}"
            );
        }

        // The step falls on the bottom face of `z*` itself, which under the
        // stored convention means the step is between `dst[z*+1]` and `dst[z*]`:
        // the optical depth of voxel `z*` is what the beam pays leaving it.
        let expected = qmul(
            p.i_surface,
            qexp(qsub(Q::ZERO, Q::from_f64(COEFF_32 * f64::from(OPAQUE)))),
        );
        assert!(expected < p.i_surface);
        for z in 0..=Z_STAR {
            assert_eq!(
                dst[index(&p, 1, 2, z) as usize],
                expected,
                "at and below the opaque voxel, z = {z}"
            );
        }
    }

    #[test]
    fn the_result_does_not_depend_on_the_dispatch_order() {
        // Gather form buys this even here, where the pass has a dependency along
        // Z: an invocation reads only `src` — state `N` — and its own register,
        // and nothing another invocation wrote. On the GPU there is no traversal
        // order at all, so anything this test could catch is unfixable there.
        // The same statement, and the same reason, as
        // `the_result_does_not_depend_on_the_traversal_order` for diffusion.
        let p = params();
        let (src32, src64) = filled();
        let coeff = coeffs();
        let att = Attenuators {
            lane: &LANES,
            coeff: &coeff,
        };

        let mut forwards = vec![Q::ZERO; N_VOXELS as usize];
        for idx in 0..N_COLUMNS {
            light_column(&src32, &src64, &mut forwards, &att, &p, idx);
        }

        let mut backwards = vec![Q::ZERO; N_VOXELS as usize];
        for idx in (0..N_COLUMNS).rev() {
            light_column(&src32, &src64, &mut backwards, &att, &p, idx);
        }

        assert_eq!(forwards, backwards);
    }

    #[test]
    fn a_column_writes_its_own_column_and_nothing_else() {
        // The operational form of ADR-034 for a kernel whose unit of work is a
        // column: the write set of an invocation is exactly its own column, and
        // the column next door is neither read nor written. That is the checkable
        // wording of "the granularity is relaxed, the gather is not".
        const X: u32 = 1;
        const Y: u32 = 2;

        let p = params();
        let (src32, src64) = filled();
        let before = dispatch(&src32, &src64, &p);

        let mut perturbed = src32.clone();
        for z in 0..NZ {
            put_32(&mut perturbed, index(&p, X, Y, z), 4000);
        }
        let after = dispatch(&perturbed, &src64, &p);

        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    if x == X && y == Y {
                        continue;
                    }
                    let at = index(&p, x, y, z) as usize;
                    assert_eq!(before[at], after[at], "voxel ({x}, {y}, {z}) moved");
                }
            }
        }

        // And the perturbation did reach its own column, or the loop above
        // asserted nothing at all.
        let bottom = index(&p, X, Y, 0) as usize;
        assert_ne!(before[bottom], after[bottom]);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "dispatched over columns")]
    fn a_per_voxel_dispatch_is_refused_rather_than_writing_past_the_field() {
        // The loud half of the dispatch contract, and the reason the module
        // header no longer calls a per-voxel dispatch merely wasteful: at
        // `idx = nx*ny` the decode gives `y = ny`, and `index` then addresses
        // `nx*ny` — one past a field of exactly that many cells. Here that is a
        // panic either way; in WGSL it is an out-of-bounds store, which the
        // specification allows to land on another binding, and no test of the
        // light field could ever see it.
        let p = params();
        let (src32, src64) = filled();
        let coeff = coeffs();
        let att = Attenuators {
            lane: &LANES,
            coeff: &coeff,
        };
        let mut dst = vec![Q::ZERO; N_VOXELS as usize];

        light_column(&src32, &src64, &mut dst, &att, &p, N_COLUMNS);
    }

    /// One column of the field, in `f64`, at a column the tests agree on.
    fn column_at(dst: &[Q], p: &LightParams, x: u32, y: u32) -> Vec<f64> {
        (0..NZ)
            .map(|z| dst[index(p, x, y, z) as usize].debug_f64())
            .collect()
    }

    /// What voxel `z` absorbed, assembled the way the fold of step `i'` will
    /// assemble it (ADR-049) and out of nothing else: the stored field, its
    /// `z`-neighbour, and — for the topmost voxel, whose neighbour above is the
    /// sky — the same folded `i_surface` this kernel is handed.
    ///
    /// This function is the contract of the field convention written as code. If
    /// it needs an input the fold kernel will not have, the convention is wrong.
    fn absorbed_by(column: &[f64], z: usize) -> f64 {
        let entering = if z + 1 == NZ as usize {
            INCIDENT
        } else {
            column[z + 1]
        };
        entering - column[z]
    }

    #[test]
    fn the_absorption_of_every_voxel_is_recoverable_from_the_stored_field() {
        // The obligation ADR-049 puts on this kernel: absorption is derived, so
        // every term of every difference has to be reachable — for the floor
        // voxel as much as for the topmost one. Under the mirror convention the
        // term for `z = 0` cannot be formed at all, and nothing else in this file
        // would notice, because light is class `Q` and is in no ledger.
        //
        // Compared against Beer-Lambert per voxel rather than telescoped:
        // `sum (I[z+1] - I[z]) == I[nz-1] - I[0]` is an algebraic identity that
        // holds over any array whatsoever, kernel or no kernel, and asserting it
        // would test nothing.
        let p = params();
        let (src32, src64) = filled();
        let dst = dispatch(&src32, &src64, &p);
        let column = column_at(&dst, &p, 2, 1);

        for z in 0..NZ as usize {
            let at = index(&p, 2, 1, z as u32);
            let entering = if z + 1 == NZ as usize {
                INCIDENT
            } else {
                column[z + 1]
            };
            let computed = absorbed_by(&column, z);
            let analytic = entering * (1.0 - (-depth_of(at)).exp());

            // Derived, not picked. The profile itself is good to a few ulps per
            // voxel of `f32`; the difference of two neighbouring values of it
            // cancels, and the cancellation multiplies the relative error by
            // `entering/absorbed`, which is `1/(1 - exp(-tau))`. The thinnest
            // voxel of `filled` is `tau = 3/512`, so that factor reaches ~170 and
            // a fixed tolerance would either be vacuous at the top or fail at the
            // bottom.
            let tolerance =
                8.0 * f64::from(NZ) * f64::from(f32::EPSILON) / (1.0 - (-depth_of(at)).exp());
            assert!(
                (computed / analytic - 1.0).abs() <= tolerance,
                "voxel z = {z}: absorbed {computed} against {analytic}, relative \
                 error {} over a tolerance of {tolerance}",
                (computed / analytic - 1.0).abs()
            );
            assert!(computed > 0.0, "voxel z = {z} absorbed nothing");
        }
    }

    #[test]
    fn what_leaves_through_the_floor_is_never_credited() {
        // The whole incident beam has exactly two destinations, and the sum says
        // so exactly: absorbed by a named voxel, or gone under the floor. The
        // second is credited to nothing and needs no channel (ADR-059:
        // `SOLAR_IN` counts what was absorbed, not what fell), and anyone reading
        // the shortfall as a leak and adding a donor for the remainder to this
        // kernel would be reversing that decision.
        //
        // What this test does *not* pin, and where to look instead: that the
        // absorption terms are the right numbers, and that one exists per voxel,
        // are both
        // `the_absorption_of_every_voxel_is_recoverable_from_the_stored_field`.
        // The equality below survives the mirror convention — there the top term
        // is zero and `column[0]` is what *enters* voxel 0 rather than what
        // leaves the domain, and the two errors cancel exactly. Read alone it
        // would be the same kind of green as the identity it replaced.
        let p = params();
        let (src32, src64) = filled();
        let dst = dispatch(&src32, &src64, &p);
        let column = column_at(&dst, &p, 2, 1);

        // Top down, because that order makes every partial sum exactly
        // `INCIDENT - column[z]`: each term is a difference of two `f32` values
        // within a factor of two of each other and so exact by Sterbenz, and each
        // partial sum is a value `f64` represents outright. Summed bottom up the
        // equality below would hold only to a rounding.
        let mut absorbed = 0.0f64;
        for z in (0..NZ as usize).rev() {
            absorbed += absorbed_by(&column, z);
        }

        let leaving = column[0];
        assert!(leaving > 0.0);
        assert!(absorbed > 0.0);
        assert_eq!(absorbed + leaving, INCIDENT);

        // The strict inequality is the ADR-059 half, and it is what a kernel
        // that "fixed the leak" fails: clamp the floor beam to zero, or dump the
        // remainder into the bottom voxel, and the column absorbs the whole
        // incident beam. Both are the obvious repair for a shortfall that is not
        // a shortfall.
        assert!(absorbed < INCIDENT);
    }

    #[test]
    fn the_two_storage_widths_attenuate_alike() {
        // The branch on `width_mask` runs on the table entry, not on the voxel,
        // so the same amount behind the same folded coefficient has to give the
        // same optical depth out of either slice. This catches the two slices
        // being swapped and the mask bit being inverted, and no conservation test
        // can: light conserves nothing.
        const AMOUNT: i64 = 1234;

        let p = params();
        let coeff = [Q::from_f64(COEFF_32), Q::from_f64(COEFF_32)];
        let att = Attenuators {
            lane: &LANES,
            coeff: &coeff,
        };

        let mut in_narrow = narrow();
        let mut in_wide = wide();
        let empty_narrow = narrow();
        let empty_wide = wide();
        for at in 0..N_VOXELS {
            put_32(&mut in_narrow, at, AMOUNT as i32);
            put_64(&mut in_wide, at, AMOUNT);
        }

        let mut from_narrow = vec![Q::ZERO; N_VOXELS as usize];
        let mut from_wide = vec![Q::ZERO; N_VOXELS as usize];
        for idx in 0..N_COLUMNS {
            light_column(&in_narrow, &empty_wide, &mut from_narrow, &att, &p, idx);
            light_column(&empty_narrow, &in_wide, &mut from_wide, &att, &p, idx);
        }

        assert_eq!(from_narrow, from_wide);
        // And the amount did something, or the equality above is two fields of
        // untouched incident light.
        assert_ne!(from_narrow[index(&p, 0, 0, 0) as usize], p.i_surface);
    }

    #[test]
    fn the_column_indexing_agrees_with_the_grid() {
        // The kernel carries its own copy of the index, because `kernels/` may
        // not depend on `world/`. The same test and the same reason as
        // `the_neighbourhood_agrees_with_the_grid` in `diffuse.rs`: a test may
        // depend on everything.
        let grid = Grid::new(NX, NY, NZ, [Boundary::Closed; 6]).unwrap();
        let p = params();
        assert_eq!(grid.n_voxels(), N_VOXELS);
        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    assert_eq!(index(&p, x, y, z), grid.index(x, y, z));
                }
            }
        }
    }

    #[test]
    fn a_negative_amount_amplifies_the_beam_rather_than_being_clamped() {
        // The accepted behaviour, pinned as a fact rather than as a warning. See
        // the `TODO(positivity)` on `light_column`: choosing between a bound on
        // `tau`, a clamp and acceptance is a decision about the physics and
        // belongs in the journal. Until it is made, the opposite behaviour would
        // arrive as an obvious fix and nothing would object.
        let p = params();
        let mut src32 = narrow();
        let src64 = wide();
        for at in 0..N_VOXELS {
            put_32(&mut src32, at, -500);
        }

        let dst = dispatch(&src32, &src64, &p);

        for z in 0..NZ - 1 {
            assert!(
                dst[index(&p, 0, 0, z) as usize] > dst[index(&p, 0, 0, z + 1) as usize],
                "the beam did not grow crossing the bottom face of z = {}",
                z + 1
            );
        }
        assert!(dst[index(&p, 0, 0, 0) as usize] > p.i_surface);
    }
}
