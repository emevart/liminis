//! The fold of step `i'`: one tick's energy, from the fine grid onto the coarse
//! one.
//!
//! ```text
//! H[coarse] = H[coarse] + sum over the 2^(3*lod) fine cells of
//!                 (reaction energy) + (light absorbed there)
//! ```
//!
//! Enthalpy lives on 32^3 and the processes that produce energy live on 128^3
//! (SPEC section 1.5), so sixty-four fine voxels fall on one coarse cell. ADR-045
//! settles that split for reactions — the reaction kernel writes its energy
//! increment into a field of its own on the fine grid, into its own cell and no
//! other, and this kernel gathers it — and ADR-049 hangs the light on the same
//! hook rather than on a second one.
//!
//! # Two sources, one sum
//!
//! The second source is not a field. The light field is already stored on 128^3,
//! so what a voxel absorbed is `I[z] - I[z-1]`, a **derived** quantity computed
//! here on the fly (ADR-049); no second accumulator on the fine grid exists, and
//! `kernels/light.rs` says in its own header that adding one would reverse that
//! decision without saying so. The terms come out of the convention that file
//! stores: a cell holds what *leaves* the voxel through its bottom face, so what
//! voxel `z` absorbed is `light[z+1] - light[z]`, and for the topmost voxel the
//! upper term is `p.i_surface`, the same folded scalar the light kernel is
//! handed. See [`absorbed_here`], which is the only place either fact is written
//! as code.
//!
//! # Pure gather: not one atomic, not one write into another cell
//!
//! A coarse cell reads its own sixty-four fine cells plus their `z`-neighbours in
//! the light field, and writes exactly one cell of each output — its own, at the
//! same index in both. The obvious
//! alternative is shorter by a field and by a pass: let the fine voxel add its
//! energy into the covering coarse cell atomically. ADR-045 rejected it in as
//! many words, and the reason is not contention: it would widen the one exception
//! of ADR-034 — "the only cells a kernel may write besides its own are the
//! ledger's channel counters" — from counters onto a field of state, and that
//! exception was kept narrow on purpose. So there is no atomic here, no channel
//! counter here, and no write into anybody else's cell.
//!
//! There are **two** outputs since ADR-075, and that revokes two words of
//! ADR-034 — "slices in, a slice out" becomes "slices out" — without touching the
//! gather property it was written to protect: both are addressed by the same
//! `coarse`, so there is still no atomic, no dependence on traversal order, and
//! no sight of a channel counter from in here. In WGSL it is a second storage
//! binding and the line-by-line translation survives. What the second output
//! carries is the subject of the section below;
//! `the_fold_writes_only_its_own_coarse_index_in_every_output` is the operational
//! form of SPEC section 4.6, whose sentence counts cells while the test counts
//! buffers.
//!
//! The other half of ADR-045 that has to survive contact with this file: the fine
//! field is **not** cleared. The reaction kernel overwrites its cell rather than
//! adding to it, which is exactly why no clearing pass exists — a fold that
//! zeroed `energy_delta` after reading it would be writing into a buffer it does
//! not own.
//!
//! # The covering cell is a shift per axis, and this is the quiet mistake
//!
//! SPEC section 1.5, quoted because the spec is frozen (ADR-032):
//!
//! ```text
//! coarse_idx = (x >> lod)
//!            + (y >> lod) * (NX >> lod)
//!            + (z >> lod) * (NX >> lod) * (NY >> lod)
//! ```
//!
//! "The shift goes **per axis**, not on the linear index: `idx = x + y*NX +
//! z*NX*NY` (section 1.1) interleaves the bit fields of X, Y and Z, and
//! `fine_idx >> lod` would mix them up — and run past the end of the coarse field
//! besides."
//!
//! Both wrong forms — `fine_idx >> lod` and `coarse * 64 + f` — partition the
//! index space into disjoint pieces exactly the way the right one does, so *every
//! conservation test stays green under them*: the totals close, and the heat is
//! merely credited to the wrong cells — a stripe of sixty-four along X instead of
//! a 4x4x4 cube. What is lost is the zoning that ADR-045 rejected the scalar
//! LEDGER-phase reduction to keep, and the picture stays plausible. The corpus
//! pushes towards the mistake, too: `coarse_idx = fine_idx >> lod` is printed as a
//! *definition* in `docs/CONFIG_SCHEMA.md` (lines 190 and 504), in
//! `config/schema.rs` and in `config/validate.rs` in three places. Only SPEC
//! section 1.5 is right, and only `the_covering_cell_is_the_per_axis_shift`
//! catches it: `world::Grid` knows nothing about `lod` at all, so for *this* half
//! of the mapping there is no authority in `world/` to check against the way
//! `diffuse.rs` checks its neighbourhood. The fine half does have one — SPEC
//! section 1.1, alive in `world::Grid::index` — and
//! `the_fine_indexing_agrees_with_the_grid` holds this file to it.
//!
//! The mirror of that mistake is [`coarse_coords`]: a decode of the coarse index
//! by the **fine** extents, `cy = coarse / nx` instead of `coarse / (nx >> lod)`.
//! It is partly masked on a cubic grid, which is why every test below runs on a
//! grid whose three coarse extents are pairwise different.
//!
//! # Both fields are `i64`, and the corpus will tell you otherwise
//!
//! The skeleton in `docs/ARCHITECTURE.md` printed `energy_delta: &[i32]`,
//! `QUANTITIES.md` section 3 printed `i32` for both quantities, and ADR-045 priced
//! "one `i32` field on 128^3". All of that is revoked by ADR-062, and revoked in
//! prose one has to read to the end of: the window for `k_E` at `i32` is *empty*,
//! and the miss is twenty-six binary orders, not one. The same record forbids the
//! tempting repair — giving the two fields different units and converting here —
//! because then the fold stops being a sum and starts rounding every tick. So
//! both roles are `M64`, the integer half of this kernel is addition of `i64` and
//! nothing else, and no conversion of units happens between them.
//!
//! What a narrower type would do is worth stating, because it is quiet:
//! `M32::from_i64_clamping` asserts in debug and **saturates silently in
//! release**, so a release run would come out with clipped enthalpy and a broken
//! energy ledger without a single panic.
//!
//! # The previous enthalpy comes from `src_h`
//!
//! `dst_h[coarse] = src_h[coarse] + ...`, never `dst_h[coarse] += ...`. On a
//! zeroed output buffer the two produce identical numbers, and they part company
//! at the first swap or the first reused buffer. Neither the compiler nor a grep
//! can tell them apart; `the_previous_enthalpy_is_read_from_src_not_from_dst`
//! can.
//!
//! # The light is rounded once per coarse cell, and read twice
//!
//! The sixty-four absorption terms are summed in `Q` and cross into storage units
//! **once**, through a single `m_delta_64` — one crossing between `M` and `Q` per
//! invocation, in the spirit of ADR-060 and `NUMERIC.md` section 1. Rounding each
//! of the sixty-four fine voxels separately would be just as defensible, and no
//! record in the journal chooses between them. The choice is declared here because
//! it moves bits, and because this number has a second consumer: ADR-059 credits
//! `SOLAR_IN` on this step, and ADR-075 settles how — the kernel writes the very
//! integer it added to the enthalpy into a second output slice, one element per
//! coarse cell, and the host reduces that slice and credits the counter as the
//! second half of the same dispatch.
//!
//! **One number read twice, not two expressions agreeing.** The identity ADR-075
//! asks for is syntactic: `let from_light = ...` once, two readers. Any variant
//! that forms the quantity again — a host-side reduction over the light field, or
//! `(dst_h - src_h) - sum(energy_delta)` — rounds on its own, and the rule of
//! `NUMERIC.md` section 3 is not additive: `round(a) + round(b) != round(a+b)`.
//! The two would then disagree by up to half a unit per coarse cell, on exactly
//! the scenarios where the roundings failed to cancel, and the residual is
//! compared as an integer and required to be an exact zero — so half a unit is as
//! fatal as a joule and much rarer.
//!
//! The slice is single buffered and is not cleared: this kernel **overwrites** its
//! element rather than adding to it, which is what ADR-045 says about
//! `energy_delta` and for the same reason. It is not state — no kernel reads it,
//! so the process-boundary invariant of ADR-057 does not reach it — and a second
//! buffer would only add a way to reduce the wrong one.
//!
//! # What this kernel cannot check, and the host must
//!
//! **The order of the tick.** ADR-049 requires the light to be computed at step
//! `a` and folded at step `i'`, so that the field belongs to the *current* tick;
//! moving the light after the fold would lag it by one and is itself world
//! semantics. Nothing here can see that, and there is no `process/` entry for
//! either step yet.
//!
//! **Stale inputs credited twice.** Neither `energy_delta` nor `light` is double
//! buffered, and both are rewritten every tick by their own kernel. A tick with
//! reactions switched off — `enabled = false` is legal (ADR-065) — hands this
//! kernel the *previous* tick's increment, and it will credit it again; a column
//! no light dispatch visited hands over yesterday's beam. A stale number is
//! indistinguishable from a fresh one, which is the same paragraph `light.rs`
//! writes about a column and `diffuse.rs` about a voxel.
//!
//! **A negative absorption is credited as it stands.** `light.rs` already pins
//! that a negative amount gives `tau < 0` and amplifies the beam; this kernel then
//! credits a negative absorption. The energy residual stays at zero while it
//! happens, because the counter and the enthalpy move together — the mechanism
//! built to catch "energy appeared out of nowhere" is precisely the one that will
//! not catch it. Clamping here would be a decision about the physics, and the same
//! `TODO(positivity)` is open in `diffuse.rs` and in `light.rs`.

use crate::numeric::{M64, Q, m_delta_64, qadd, qmul, qsub};

/// Parameters of one application. Scalars only: in WGSL this is a uniform buffer,
/// and every number in it is a place where the host's scale can drift away from
/// the kernel's (`ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug)]
pub struct FoldParams {
    /// Voxels along X **of the fine grid**. The coarse extents are never passed
    /// in: they are `nx >> lod` and so on, derived per axis (SPEC section 1.5),
    /// and a second set of three numbers would be a second place for them to
    /// disagree with the first.
    pub nx: u32,
    /// Voxels along Y of the fine grid.
    pub ny: u32,
    /// Voxels along Z of the fine grid.
    pub nz: u32,
    /// How many bits coarser the enthalpy grid is: `2^(3*lod)` fine voxels to one
    /// coarse cell, sixty-four at `lod = 2` (SPEC section 1.5).
    ///
    /// A `u32` and a parameter, not a `u8` and not the constant 2. `u32` because
    /// WGSL has no eight-bit scalar at all; a parameter because
    /// `config/validate.rs` reads the enthalpy `lod` out of the `[[field]]`
    /// record and defaults it to zero, so `lod = 0` — a pointless but legal 1:1
    /// fold — has to work here rather than trip an assertion.
    pub lod: u32,
    /// Irradiance falling on the top face of the topmost voxel, W/m^2, already
    /// modulated by the day and the season on the host (SPEC section 7).
    ///
    /// The same folded scalar `LightParams::i_surface` is, and it has to be the
    /// same *number*: it is the upper term of the absorption of every voxel at
    /// `z = nz-1`, the one term of that difference which is not in the field. Two
    /// values drifting apart would create or destroy a layer of energy with the
    /// profile `I(z)` staying perfectly right.
    pub i_surface: Q,
    /// What one unit of stored intensity is worth as energy per second: the area
    /// of a **fine** face times `units_per_joule`, folded into one number on the
    /// host (ADR-015 — the kernel knows neither `dx` nor `units_per_joule`).
    ///
    /// Fine, not coarse, and that distinction is the whole of this comment. The
    /// sum below runs over the `2^(3*lod)` fine voxels of the cell, so every term
    /// already carries one fine face. Folding the coarse face `(dx*2^lod)^2` in
    /// here would multiply the entire solar input by `2^(2*lod)` — sixteen at
    /// `lod = 2` — and a forgotten [`FoldParams::dt`] would divide it by `dt`,
    /// which at a tick of one second is invisible outright. Both read as "the
    /// coefficients are not calibrated yet", and calibrating `i_surface` against
    /// them hides them forever; `Attenuators::coeff` in `kernels/light.rs` names
    /// the same trap for `dz`. Neither is checkable from inside a kernel — it is
    /// the host's obligation, and this is where it is written down. The two names
    /// that hold it are `the_solar_term_does_not_depend_on_the_enthalpy_lod` and
    /// `the_solar_term_scales_with_the_tick`.
    ///
    /// Both halves are named now (ADR-076): `i_surface` is a key of the light
    /// process in W/m^2, and `units_per_intensity = dx^2 * units_per_joule` is
    /// derived at load out of the grid step and the energy scale of ADR-062. It
    /// is **not** a key: the product `1.4757e12` has no readable preimage, so a
    /// wrong one is indistinguishable from a right one, and `units_per_joule` is
    /// already derived — a key would be a second independent source for a derived
    /// number.
    ///
    /// The field is called `units_per_intensity` and not `joules_per_intensity`,
    /// by the decision of ADR-076 rather than by preference: it measures storage
    /// units per (W/m^2) per second, and the old name taught the wrong unit in
    /// the one place the unit is assigned.
    pub units_per_intensity: Q,
    /// The tick, in seconds. Separate from [`FoldParams::units_per_intensity`]
    /// rather than folded into it, because that product is a *rate* and this
    /// crossing is `m_delta`, whose second argument is `dt` by its own
    /// documentation (`numeric/convert.rs`).
    pub dt: Q,
}

/// One coarse cell of the enthalpy field: the energy of one tick, gathered.
///
/// `coarse` runs over `0..(nx>>lod)*(ny>>lod)*(nz>>lod)` — the **coarse** grid,
/// which is the whole of what makes this kernel different from every other one in
/// this directory. Reads its own sixty-four cells of `energy_delta`, their
/// `z`-neighbours in `light`, and one cell of `src_h`; writes exactly one cell of
/// `dst_h` and one cell of `solar`, both at `coarse`.
///
/// `solar` is what the ledger is owed for this cell — the same integer this
/// invocation added to the enthalpy, not a second computation of it (ADR-075).
/// The host reduces the slice and credits `SOLAR_IN` with it as the second half
/// of this dispatch; the kernel itself sees no counter, as ADR-034 requires and
/// as this file's header repeats. Overwritten and never accumulated: on a zeroed
/// slice `=` and `+=` are indistinguishable and part company on the second tick,
/// and `the_solar_slice_is_overwritten_not_accumulated` is what tells them apart.
///
/// The loud half of the dispatch contract is asserted below. The quiet half — a
/// host that dispatches over coarse cells and decodes the index by the fine
/// extents — no assertion over values can see; what defends against it is that
/// [`coarse_coords`] divides by the coarse extents, and that this paragraph
/// exists.
pub fn fold_energy(
    energy_delta: &[M64],
    light: &[Q],
    src_h: &[M64],
    dst_h: &mut [M64],
    solar: &mut [M64],
    p: &FoldParams,
    coarse: u32,
) {
    // The dispatch domain, asserted rather than only documented, on the precedent
    // of `a_per_voxel_dispatch_is_refused_rather_than_writing_past_the_field` in
    // `kernels/light.rs`: a host that dispatched this over the fine grid gets a
    // `cz` past the coarse field, and `index` below then reads past the end of
    // `energy_delta`. Here that is a panic; in WGSL it is an out-of-bounds access
    // the specification allows to land on another binding.
    debug_assert!(
        coarse < n_coarse_cells(p),
        "fold_energy is dispatched over coarse cells, not fine voxels: coarse \
         {coarse} is past their count {}",
        n_coarse_cells(p)
    );

    let (cx, cy, cz) = coarse_coords(p, coarse);

    // The origin of the covering cube, per axis. This is the inverse of the shift
    // of SPEC section 1.5, and it is written per axis for the same reason the
    // shift is: a linear `coarse * 2^(3*lod)` would name a stripe along X.
    let x0 = cx << p.lod;
    let y0 = cy << p.lod;
    let z0 = cz << p.lod;
    let span = 1u32 << p.lod;

    // Two accumulators, not one, and the split is load-bearing. The reactions are
    // summed as integers — exactly, in `M64` — because the declared span of one
    // voxel for one tick is `3.08e16` units (ADR-062), fifty-five significant bits
    // against the twenty-four of `Q`. Passing them through `qadd` "to match the
    // light" would erase thirty-one low bits and look like harmless rounding.
    let mut reactions = M64::ZERO;
    let mut absorbed = Q::ZERO;

    // A triple `for` over the axes, not `for fine in fine_cells_of(p, coarse)` as
    // the skeleton in `ARCHITECTURE.md` used to write it: an iterator is either a
    // closure or a type with behaviour, and ADR-015 forbids both inside a kernel.
    // This form also happens to be the one that cannot mix the axes up.
    for dz in 0..span {
        for dy in 0..span {
            for dx in 0..span {
                let x = x0 + dx;
                let y = y0 + dy;
                let z = z0 + dz;

                reactions += energy_delta[index(p, x, y, z) as usize];
                absorbed = qadd(absorbed, absorbed_here(light, p, x, y, z));
            }
        }
    }

    // The one crossing from `Q` into `M` on this path, taken once per coarse cell
    // rather than once per fine voxel (see the module header). `qmul` first, so
    // that what reaches `m_delta_64` is energy per second — which is what that
    // function documents its first argument to be.
    let from_light = m_delta_64(qmul(absorbed, p.units_per_intensity), p.dt);

    // What the ledger is owed for this cell, handed over as an integer rather
    // than as a second computation (ADR-075). The two lines below read one
    // binding: the identity between the counter and the field is syntactic, and
    // any arrangement in which the counter's number is *formed again* rounds a
    // second time and disagrees by up to half a unit per cell.
    //
    // Assigned and never accumulated, like the enthalpy line under it, and for
    // the same reason ADR-045 gave `energy_delta`: a kernel that overwrites needs
    // no clearing pass, and on a zeroed slice `+=` is indistinguishable from this
    // until the second tick.
    solar[coarse as usize] = from_light;

    // From `src_h`, never from `dst_h`: `dst_h[coarse] += ...` would be a read
    // from the buffer being written, and it is indistinguishable from this line on
    // a zeroed output.
    dst_h[coarse as usize] = src_h[coarse as usize] + reactions + from_light;
}

/// What one fine voxel absorbed, in `Q` and unrounded.
///
/// The stored convention of `kernels/light.rs` is that a cell holds the intensity
/// **leaving** the voxel through its bottom face, so the difference is between the
/// cell above and this one:
///
/// ```text
/// (z < nz-1)   light[z+1] - light[z]
/// (z == nz-1)  p.i_surface - light[z]
/// ```
///
/// Three ways to get this wrong, all of them quiet, all of them documented in
/// `light.rs` from the other side of the boundary.
///
/// **The literal transcription.** ADR-049 writes `I[z] - I[z-1]` in terms of
/// *levels*, while the field is indexed by *cells*, so
/// `light[fine] - light[fine - plane]` gives both a one-voxel shift and the
/// opposite sign: the domain cools instead of warming. Telescoped over a column
/// the magnitude is the same, and if `SOLAR_IN` is ever credited from the same
/// expression the energy residual stays at zero — the sign would stand on both
/// sides of the equality.
///
/// **The mirror trick at the top face.** For `z = nz-1` there is no neighbour in
/// the field, and the project's most natural reflex — "the neighbour past the
/// boundary is me", which is how `diffuse.rs` closes a face — gives zero here, and
/// zero *absorption*, not zero *flux*: a whole layer of absorbed energy stops
/// existing. The profile `I(z)` does not change, and both halves of the invariant
/// close over a term neither side ever saw. That is verbatim the warning
/// `light_column` writes about the mirror convention, and
/// `the_top_layer_absorbs_against_the_surface_irradiance` is the only thing
/// standing there. The second form of the same mistake is reading
/// `light[index(x, y, nz)]` without the branch: a panic on the CPU, and in WGSL an
/// access past the buffer.
///
/// **Repairing the floor.** `light[index(x, y, 0)]` is the beam that left under
/// the domain. It is legitimate here as the lower term of voxel `0` and nowhere
/// else; crediting it — or clamping the beam to zero at the floor — creates energy
/// with no channel, which is the mistake ADR-059 names outright with "`SOLAR_IN`
/// counts what was absorbed, not what fell".
///
/// Nothing is rounded here, on purpose: the rounding is one per coarse cell and
/// happens in the caller.
#[inline(always)]
fn absorbed_here(light: &[Q], p: &FoldParams, x: u32, y: u32, z: u32) -> Q {
    let entering = if z + 1 < p.nz {
        light[index(p, x, y, z + 1) as usize]
    } else {
        p.i_surface
    };

    // `qsub`, not a bare `-`: `Q` implements no arithmetic operator at all
    // (ADR-022), and no `Neg` either, which is why the difference is written in
    // this order rather than negated afterwards.
    qsub(entering, light[index(p, x, y, z) as usize])
}

/// The three coarse coordinates of a coarse linear index.
///
/// Divided by the **coarse** extents, `nx >> lod` and `ny >> lod`. Dividing by the
/// fine ones is the mirror of the `fine_idx >> lod` mistake of the module header
/// and is partly masked on a cubic grid, which is why the tests below run on a
/// grid whose coarse extents are pairwise different.
#[inline(always)]
fn coarse_coords(p: &FoldParams, coarse: u32) -> (u32, u32, u32) {
    let cnx = p.nx >> p.lod;
    let cny = p.ny >> p.lod;

    let z = coarse / (cnx * cny);
    let rest = coarse - z * cnx * cny;
    let y = rest / cnx;
    let x = rest - y * cnx;

    (x, y, z)
}

/// How many cells the coarse field has: the product of the shifted extents.
#[inline(always)]
fn n_coarse_cells(p: &FoldParams) -> u32 {
    (p.nx >> p.lod) * (p.ny >> p.lod) * (p.nz >> p.lod)
}

/// The linear index of a **fine** voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
///
/// The kernel's own copy of `world::Grid::index`, for the reason `diffuse.rs` and
/// `light.rs` each keep one: `kernels/` depends on `numeric/` and on nothing else,
/// and a `Grid` is a host type with no meaning in WGSL. Like theirs, it is kept
/// from drifting by a test against that authority —
/// `the_fine_indexing_agrees_with_the_grid`, the twin of
/// `the_neighbourhood_agrees_with_the_grid` in `diffuse.rs` and of
/// `the_column_indexing_agrees_with_the_grid` in `light.rs`. Every fixture in this
/// file places its inputs through this same function, so a transposed copy would
/// be self-consistent inside the module and invisible to every other test here:
/// the fold would gather some other voxel's increment and some other voxel's
/// light, the totals would still close, and the heat would be credited to the
/// wrong place.
///
/// Only the *coarse* half of the mapping has no authority in `world/` to be
/// checked against — `Grid` knows nothing about `lod` — so SPEC section 1.5 is
/// transcribed into `the_covering_cell_is_the_per_axis_shift` instead.
#[inline(always)]
fn index(p: &FoldParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernels::light::{Attenuators, LightParams, light_column};
    use crate::numeric::M32;
    use crate::world::{Boundary, Grid};

    /// The one fine grid of this module; the coarse grid it folds onto is
    /// `2 x 3 x 4`. Three pairwise different coarse extents, and none of them one:
    /// on a cubic grid every bug that swaps two axes is invisible, because all
    /// three strides are equal, and the decode of the coarse index by the fine
    /// extents is masked besides.
    ///
    /// `nz = 16` rather than the four a `2 x 3 x 1` coarse grid would need, and
    /// that is not a spare factor. With `cnz == 1` the coarse `z` is identically
    /// zero: the `z` term of the per-axis shift vanishes from every expectation,
    /// the divide-out of a coarse plane in [`coarse_coords`] is never taken with a
    /// nonzero quotient, and `let z0 = cz;` in place of `let z0 = cz << p.lod;`
    /// leaves the whole mapping half of this module green. A column of four coarse
    /// cells is also what makes "the coarse cell that covers the opaque layer" a
    /// statement with content in the light tests below.
    const NX: u32 = 8;
    const NY: u32 = 12;
    const NZ: u32 = 16;
    const LOD: u32 = 2;

    const N_FINE: u32 = NX * NY * NZ;
    const N_COARSE: u32 = (NX >> LOD) * (NY >> LOD) * (NZ >> LOD);

    /// `2^(3*lod)`: how many fine voxels one coarse cell owns.
    const PER_COARSE: i64 = 1 << (3 * LOD);

    /// Arbitrary, and a fixture is entitled to be: the kernel takes the
    /// irradiance folded, so no scenario key is involved here at all (ADR-076
    /// declares `i_surface` and refuses every scenario that sets it above zero).
    /// A power of two so that the products below are exact and the assertions are
    /// about the kernel rather than about `f32`.
    const INCIDENT: f64 = 128.0;

    /// Also arbitrary, also a power of two, and deliberately not one: a tick of
    /// one second would hide a kernel that forgot to multiply by it.
    const DT: f64 = 2.0;

    /// A power of two, so `qmul` by it is exact and the only inexactness left on
    /// the light path is the single rounding into storage units.
    const UNITS_PER_INTENSITY: f64 = 1024.0;

    /// The whole conversion factor from stored intensity to storage units.
    const PER_INTENSITY: f64 = UNITS_PER_INTENSITY * DT;

    fn params() -> FoldParams {
        FoldParams {
            nx: NX,
            ny: NY,
            nz: NZ,
            lod: LOD,
            i_surface: Q::from_f64(INCIDENT),
            units_per_intensity: Q::from_f64(UNITS_PER_INTENSITY),
            dt: Q::from_f64(DT),
        }
    }

    /// A light field in which every voxel absorbs nothing: flat at the incident
    /// irradiance, so both branches of [`absorbed_here`] give an identical zero.
    fn transparent(n: u32) -> Vec<Q> {
        vec![Q::from_f64(INCIDENT); n as usize]
    }

    /// A light field in which every voxel absorbs exactly one unit of intensity:
    /// `light[z] = i_surface - (nz - z)`.
    ///
    /// Hand-built rather than taken from `light_column`, and that is what makes
    /// the conservation assertion exact without a tolerance: every one of the
    /// sixty-four terms is `1.0`, so the `Q` sum is `64.0` whatever order it is
    /// taken in, and the contribution of a coarse cell is one exactly known
    /// integer.
    fn stepped(p: &FoldParams) -> Vec<Q> {
        let mut light = vec![Q::ZERO; (NX * NY * p.nz) as usize];
        for z in 0..p.nz {
            for y in 0..NY {
                for x in 0..NX {
                    let at = index(p, x, y, z) as usize;
                    light[at] = Q::from_f64(INCIDENT - f64::from(p.nz - z));
                }
            }
        }
        light
    }

    /// What one coarse cell gains from [`stepped`], in storage units: sixty-four
    /// voxels absorbing one unit of intensity each.
    const LIGHT_PER_COARSE: i64 = PER_COARSE * (UNITS_PER_INTENSITY as i64) * (DT as i64);

    /// A sign-alternating reaction increment with no period any stride of the grid
    /// shares: a pattern agreeing with `nx` or with a plane would cancel inside a
    /// coarse cell and hide a voxel read twice.
    fn reaction_pattern(at: u32) -> i64 {
        let n = i64::from(at);
        let magnitude = (n * 7919) % 1013 + 1;
        if n % 3 == 0 { -magnitude } else { magnitude }
    }

    fn reactions_filled(n: u32) -> Vec<M64> {
        (0..n).map(|at| M64::new(reaction_pattern(at))).collect()
    }

    /// The whole dispatch, the way the host runs it: one invocation per coarse
    /// cell, over both outputs.
    fn dispatch(
        energy_delta: &[M64],
        light: &[Q],
        src_h: &[M64],
        p: &FoldParams,
        n_coarse: u32,
    ) -> Vec<M64> {
        dispatch_both(energy_delta, light, src_h, p, n_coarse).0
    }

    /// The same, keeping the solar slice as well: `(dst_h, solar)`.
    fn dispatch_both(
        energy_delta: &[M64],
        light: &[Q],
        src_h: &[M64],
        p: &FoldParams,
        n_coarse: u32,
    ) -> (Vec<M64>, Vec<M64>) {
        let mut dst_h = vec![M64::ZERO; n_coarse as usize];
        let mut solar = vec![M64::ZERO; n_coarse as usize];
        for coarse in 0..n_coarse {
            fold_energy(
                energy_delta,
                light,
                src_h,
                &mut dst_h,
                &mut solar,
                p,
                coarse,
            );
        }
        (dst_h, solar)
    }

    fn total(cells: &[M64]) -> i64 {
        cells.iter().map(|h| h.to_i64()).sum()
    }

    /// The coarse index of the cell covering a fine voxel, per axis
    /// (SPEC section 1.5).
    fn covering(x: u32, y: u32, z: u32) -> u32 {
        let cnx = NX >> LOD;
        let cny = NY >> LOD;
        (x >> LOD) + (y >> LOD) * cnx + (z >> LOD) * cnx * cny
    }

    /// Halves away from zero, in `f64`, so that an expectation can be built
    /// without calling the conversion the kernel calls (`numeric/convert.rs`).
    fn round_half_away_from_zero(x: f64) -> i64 {
        if x < 0.0 {
            -((-x) + 0.5).floor() as i64
        } else {
            (x + 0.5).floor() as i64
        }
    }

    #[test]
    fn energy_fold_from_fine_to_coarse_conserves_exactly() {
        // `ACCEPTANCE.md`, section "Conservation". The criterion itself is judged
        // from outside the crate, in `tests/acceptance_fold.rs`, on another grid
        // and with the two index mappings written out from the spec rather than
        // borrowed from here; this is the inside view of the same name, and the
        // overlap is deliberate — see that file's header.
        //
        // Exact and without a tolerance: the left side is a sum of `i64`, and both
        // light fields are built so that the right side is one too.
        //
        // What this test does *not* catch is worth as much as what it does: a fold
        // that mapped fine voxels onto coarse cells by `fine_idx >> lod`, or by
        // `coarse * 64 + f`, partitions the index space just as completely, so the
        // totals still close and this test stays green. See
        // `the_covering_cell_is_the_per_axis_shift`.
        let p = params();
        let energy_delta = reactions_filled(N_FINE);
        let sum_fine: i64 = (0..N_FINE).map(reaction_pattern).sum();

        // Light off. The right side is a pure integer sum, and every term of it is
        // a reaction increment.
        let zeroed = vec![M64::ZERO; N_COARSE as usize];
        let dark = dispatch(&energy_delta, &transparent(N_FINE), &zeroed, &p, N_COARSE);
        assert_eq!(total(&dark) - total(&zeroed), sum_fine);

        // Light on, over a zeroed enthalpy.
        let lit = dispatch(&energy_delta, &stepped(&p), &zeroed, &p, N_COARSE);
        assert_eq!(
            total(&lit) - total(&zeroed),
            sum_fine + i64::from(N_COARSE) * LIGHT_PER_COARSE
        );

        // Light on, over an enthalpy that is already something. The increment has
        // to be the same one: a kernel that overwrote instead of adding, or that
        // read the previous value out of the output buffer, parts from the line
        // above here.
        let before: Vec<M64> = (0..N_COARSE)
            .map(|c| M64::new(i64::from(c) * 1_000_003 - 7))
            .collect();
        let over = dispatch(&energy_delta, &stepped(&p), &before, &p, N_COARSE);
        assert_eq!(
            total(&over) - total(&before),
            sum_fine + i64::from(N_COARSE) * LIGHT_PER_COARSE
        );
    }

    #[test]
    fn the_covering_cell_is_the_per_axis_shift() {
        // SPEC section 1.5 transcribed into a test, because for the coarse half of
        // the mapping there is no authority in `world/` to check against: `Grid`
        // knows nothing about `lod`. (The fine half has one, and
        // `the_fine_indexing_agrees_with_the_grid` uses it.) This is
        // the only defence against `coarse * 64 + f` and against `fine_idx >> lod`
        // — both of them cut the index space into disjoint pieces, so every
        // conservation test above is green on them while the heat is credited to a
        // stripe of sixty-four along X.
        //
        // The grid has to be non-cubic, or a permutation of the axes is invisible.
        let p = params();
        let light = transparent(N_FINE);
        let src_h = vec![M64::ZERO; N_COARSE as usize];
        let cnx = NX >> LOD;
        let cny = NY >> LOD;

        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    let mut energy_delta = vec![M64::ZERO; N_FINE as usize];
                    energy_delta[index(&p, x, y, z) as usize] = M64::new(1);

                    let dst = dispatch(&energy_delta, &light, &src_h, &p, N_COARSE);

                    // Written out per axis, from SPEC section 1.5, rather than
                    // called through `coarse_coords`: a test that reused the
                    // kernel's own decode would agree with it by construction.
                    let expected = (x >> LOD) + (y >> LOD) * cnx + (z >> LOD) * cnx * cny;
                    for coarse in 0..N_COARSE {
                        let want = i64::from(coarse == expected);
                        assert_eq!(
                            dst[coarse as usize].to_i64(),
                            want,
                            "the unit at ({x}, {y}, {z}) landed wrong: coarse cell \
                             {coarse} should hold {want}, its covering cell is {expected}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_fine_indexing_agrees_with_the_grid() {
        // The kernel carries its own copy of the fine index, because `kernels/`
        // may not depend on `world/`. The same test and the same reason as
        // `the_neighbourhood_agrees_with_the_grid` in `diffuse.rs` and
        // `the_column_indexing_agrees_with_the_grid` in `light.rs`: a test may
        // depend on everything.
        //
        // It is needed here more than in either of them, not less. Every fixture
        // in this module — `stepped`, `reactions_filled` through the dispatch, the
        // `amounts` of the light tests — places its values through this same
        // private `index`, so a transposed copy is consistent with itself and
        // invisible: `y + x*ny + z*nx*ny` is still a bijection of the plane, so
        // the totals close, `energy_fold_from_fine_to_coarse_conserves_exactly`
        // stays green, and in a real run the fold gathers another voxel's reaction
        // increment and another voxel's absorbed light. That is the coarse half's
        // failure mode word for word, and here it is checkable.
        let grid = Grid::new(NX, NY, NZ, [Boundary::Closed; 6]).unwrap();
        let p = params();
        assert_eq!(grid.n_voxels(), N_FINE);
        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    assert_eq!(index(&p, x, y, z), grid.index(x, y, z));
                }
            }
        }
    }

    #[test]
    fn every_fine_cell_is_gathered_exactly_once() {
        // The local form of the conservation test: not "the totals closed" but
        // "this cell collected its own sixty-four". A fold that read one voxel
        // twice and another not at all can still close globally.
        let p = params();
        let energy_delta = vec![M64::new(1); N_FINE as usize];
        let src_h = vec![M64::ZERO; N_COARSE as usize];

        let dst = dispatch(&energy_delta, &transparent(N_FINE), &src_h, &p, N_COARSE);

        for coarse in 0..N_COARSE {
            assert_eq!(
                dst[coarse as usize].to_i64(),
                PER_COARSE,
                "coarse cell {coarse} did not gather its {PER_COARSE} fine voxels"
            );
        }
    }

    /// One attenuator, reading the narrow slice: the smallest configuration of
    /// `kernels/light.rs` that still produces a real Beer-Lambert profile.
    const OPACITY: f64 = 1.0 / 1024.0;

    /// `tau = 0.5` for a voxel holding this much. Under `ln 2`, so every intensity
    /// in a column stays within a factor of two of its neighbour and the
    /// differences `absorbed_here` takes are exact in `f32` by Sterbenz — which is
    /// what lets the assertions below be equalities.
    const OPAQUE: i32 = 512;

    /// The light field, built by the light kernel itself rather than by hand.
    /// That is the point of these tests: the convention "a cell holds what leaves
    /// through the bottom face" lives in `light.rs` and the difference is taken
    /// here, and today the only thing connecting them is prose.
    fn light_field(amounts: &[M32]) -> Vec<Q> {
        let lp = LightParams {
            lane_len: N_FINE,
            nx: NX,
            ny: NY,
            nz: NZ,
            n_voxels: N_FINE,
            n_attenuators: 1,
            width_mask: 0,
            i_surface: Q::from_f64(INCIDENT),
        };
        let lanes = [0u32];
        let coeff = [Q::from_f64(OPACITY)];
        let att = Attenuators {
            lane: &lanes,
            coeff: &coeff,
        };
        let wide: Vec<M64> = Vec::new();

        let mut light = vec![Q::ZERO; N_FINE as usize];
        for column in 0..NX * NY {
            light_column(amounts, &wide, &mut light, &att, &lp, column);
        }
        light
    }

    #[test]
    fn absorbed_light_appears_in_enthalpy() {
        // `ACCEPTANCE.md`, and the inside view of it: the criterion is judged from
        // outside the crate in `tests/acceptance_fold.rs`. Three claims, and each
        // of them fails to a different mistake.
        //
        // The opaque voxels sit in one fine column rather than in a whole layer,
        // so every coarse cell has at most one nonzero absorption term and its `Q`
        // sum is exact whatever order it is taken in. That keeps the equalities
        // below equalities.
        const X: u32 = 1;
        const Y: u32 = 2;
        const Z_MIDDLE: u32 = 6;

        let p = params();
        let mut amounts = vec![M32::ZERO; N_FINE as usize];
        amounts[index(&p, X, Y, Z_MIDDLE) as usize] = M32::new(OPAQUE);
        amounts[index(&p, X, Y, NZ - 1) as usize] = M32::new(OPAQUE);

        let light = light_field(&amounts);
        let energy_delta = vec![M64::ZERO; N_FINE as usize];
        let src_h = vec![M64::ZERO; N_COARSE as usize];
        let dst = dispatch(&energy_delta, &light, &src_h, &p, N_COARSE);

        // (1) The coarse cells covering an opaque voxel warmed; every other one is
        // at exactly zero. A transparent voxel attenuates by `qexp(0) == 1`, so
        // "nearly zero" is not the claim.
        let warm_middle = covering(X, Y, Z_MIDDLE);
        let warm_top = covering(X, Y, NZ - 1);
        for coarse in 0..N_COARSE {
            let got = dst[coarse as usize].to_i64();
            if coarse == warm_middle || coarse == warm_top {
                assert!(
                    got > 0,
                    "coarse cell {coarse} covers an absorber, got {got}"
                );
            } else {
                assert_eq!(got, 0, "transparent coarse cell {coarse} warmed by {got}");
            }
        }

        // (2) The column as a whole absorbed the beam it lost, up to the roundings
        // — one per coarse cell of the column, and the column has four of them;
        // each costs at most half a unit. Named, not hidden inside a tolerance.
        const ROUNDINGS_PER_COLUMN: i64 = (NZ >> LOD) as i64;
        let tolerance = 0.5 * ROUNDINGS_PER_COLUMN as f64;

        let mut column_sum = 0i64;
        for cz in 0..NZ >> LOD {
            column_sum += dst[covering(X, Y, cz << LOD) as usize].to_i64();
        }
        let leaving = light[index(&p, X, Y, 0) as usize].debug_f64();
        let telescoped = (INCIDENT - leaving) * PER_INTENSITY;
        assert!(
            (column_sum as f64 - telescoped).abs() <= tolerance,
            "the column absorbed {column_sum} against a telescoped {telescoped}, \
             over a tolerance of {tolerance} for {ROUNDINGS_PER_COLUMN} roundings"
        );

        // (3) The topmost fine layer contributed, which is only true if its upper
        // term is `i_surface`. A kernel that mirrored the boundary would put a
        // zero here and leave every other assertion of this file green.
        assert!(dst[warm_top as usize].to_i64() > 0);
    }

    #[test]
    fn the_top_layer_absorbs_against_the_surface_irradiance() {
        // Separately and head-on, because a silent failure here costs a whole
        // layer: the voxel at `z = nz-1` has the sky above it, and the reflex that
        // closes a face in `diffuse.rs` — "the neighbour past the boundary is me" —
        // credits zero. The profile `I(z)` stays right, both halves of the
        // invariant close, and nothing turns red.
        const X: u32 = 5;
        const Y: u32 = 9;

        let p = params();
        let mut amounts = vec![M32::ZERO; N_FINE as usize];
        amounts[index(&p, X, Y, NZ - 1) as usize] = M32::new(OPAQUE);

        let light = light_field(&amounts);
        let energy_delta = vec![M64::ZERO; N_FINE as usize];
        let src_h = vec![M64::ZERO; N_COARSE as usize];
        let dst = dispatch(&energy_delta, &light, &src_h, &p, N_COARSE);

        // Exactly, not approximately: one absorbing voxel in the cell, so the `Q`
        // sum is a single term; that difference is exact by Sterbenz, and
        // multiplying it by two powers of two is exact as well. The only inexact
        // step left is the rounding into whole units, which `f64` reproduces here.
        let top = light[index(&p, X, Y, NZ - 1) as usize].debug_f64();
        let expected = round_half_away_from_zero((INCIDENT - top) * PER_INTENSITY);
        assert!(expected > 0);

        let warm = covering(X, Y, NZ - 1);
        assert_eq!(dst[warm as usize].to_i64(), expected);
        for coarse in 0..N_COARSE {
            if coarse == warm {
                continue;
            }
            assert_eq!(
                dst[coarse as usize].to_i64(),
                0,
                "coarse cell {coarse} warmed with nothing to absorb"
            );
        }
    }

    #[test]
    fn the_beam_under_the_floor_is_credited_to_nobody() {
        // `light[index(x, y, 0)]` is the beam that left under the domain. It enters
        // the sum only as the lower term of voxel `0` and never as an absorbed
        // quantity. With no attenuation anywhere the whole incident beam passes
        // through, and the enthalpy of the domain moves by exactly nothing.
        //
        // A kernel that "fixed the floor" — credited the remainder to the bottom
        // voxel, or clamped the beam to zero there — creates energy with no
        // channel, which is what ADR-059 forbids with "`SOLAR_IN` counts what was
        // absorbed, not what fell".
        let p = params();
        let amounts = vec![M32::ZERO; N_FINE as usize];
        let light = light_field(&amounts);

        // The premise: the beam really does reach the floor undimmed, or this test
        // asserts nothing.
        assert_eq!(light[index(&p, 3, 7, 0) as usize].debug_f64(), INCIDENT);

        let energy_delta = vec![M64::ZERO; N_FINE as usize];
        let src_h = vec![M64::ZERO; N_COARSE as usize];
        let dst = dispatch(&energy_delta, &light, &src_h, &p, N_COARSE);

        assert_eq!(total(&dst), 0);
        for coarse in 0..N_COARSE {
            assert_eq!(dst[coarse as usize].to_i64(), 0, "coarse cell {coarse}");
        }
    }

    #[test]
    fn the_previous_enthalpy_is_read_from_src_not_from_dst() {
        // `dst_h[coarse] += ...` gives identical numbers on a zeroed output and
        // parts from this on the first reused buffer. Neither the compiler nor a
        // grep can see the difference; this can.
        let p = params();
        let energy_delta = reactions_filled(N_FINE);
        let light = stepped(&p);
        let src_h: Vec<M64> = (0..N_COARSE)
            .map(|c| M64::new(i64::from(c) * 500_009 + 11))
            .collect();

        let clean = dispatch(&energy_delta, &light, &src_h, &p, N_COARSE);

        // The same call over an output buffer full of something else entirely.
        let mut dirty: Vec<M64> = (0..N_COARSE)
            .map(|c| M64::new(-7_000_000_003 * i64::from(c) - 13))
            .collect();
        let mut solar = vec![M64::ZERO; N_COARSE as usize];
        for coarse in 0..N_COARSE {
            fold_energy(
                &energy_delta,
                &light,
                &src_h,
                &mut dirty,
                &mut solar,
                &p,
                coarse,
            );
        }
        assert_eq!(clean, dirty);

        // And running the same dispatch again over its own output changes nothing:
        // one dispatch is idempotent, which is the property `+=` does not have.
        for coarse in 0..N_COARSE {
            fold_energy(
                &energy_delta,
                &light,
                &src_h,
                &mut dirty,
                &mut solar,
                &p,
                coarse,
            );
        }
        assert_eq!(clean, dirty);
    }

    #[test]
    fn the_solar_slice_is_overwritten_not_accumulated() {
        // The twin of the test above, for the output ADR-075 added: the same
        // dispatch over a slice full of somebody else's numbers has to give the
        // same slice as over a zeroed one, and a second dispatch over its own
        // output has to change nothing.
        //
        // On a zeroed slice `=` and `+=` are indistinguishable, which is the whole
        // reason this exists. They part company on the second tick, and they part
        // in the direction of a monotonically growing credit sitting beside a
        // perfectly correct enthalpy — so the energy residual is wrong and the
        // field it is checked against is not.
        let p = params();
        let energy_delta = reactions_filled(N_FINE);
        let light = stepped(&p);
        let src_h = vec![M64::new(29); N_COARSE as usize];

        let (_, clean) = dispatch_both(&energy_delta, &light, &src_h, &p, N_COARSE);
        assert!(
            clean.iter().any(|c| c.to_i64() != 0),
            "the fixture credits nothing, so the claim below is vacuous"
        );

        let mut dst_h = vec![M64::ZERO; N_COARSE as usize];
        let mut dirty: Vec<M64> = (0..N_COARSE)
            .map(|c| M64::new(-3_000_000_019 * i64::from(c) - 5))
            .collect();
        for coarse in 0..N_COARSE {
            fold_energy(
                &energy_delta,
                &light,
                &src_h,
                &mut dst_h,
                &mut dirty,
                &p,
                coarse,
            );
        }
        assert_eq!(clean, dirty);

        for coarse in 0..N_COARSE {
            fold_energy(
                &energy_delta,
                &light,
                &src_h,
                &mut dst_h,
                &mut dirty,
                &p,
                coarse,
            );
        }
        assert_eq!(clean, dirty);
    }

    #[test]
    fn the_fold_writes_only_its_own_coarse_index_in_every_output() {
        // The operational form of SPEC section 4.6, "it stays a pure gather and
        // writes only into its own coarse cell". The sentence counts cells and
        // there are now two writes into the one cell (ADR-075); this counts
        // buffers, and the name survives any number of outputs.
        //
        // Both outputs are walked, and both start from values no invocation could
        // produce: an invocation that wrote a neighbour, or that decoded the index
        // of the second output by the fine extents, leaves those values disturbed
        // somewhere other than at `coarse`. The grid's coarse extents are pairwise
        // different (2 x 3 x 4), or a permutation of the axes is invisible.
        let p = params();
        let energy_delta = reactions_filled(N_FINE);
        let light = stepped(&p);
        let src_h = vec![M64::new(101); N_COARSE as usize];

        let (want_h, want_solar) = dispatch_both(&energy_delta, &light, &src_h, &p, N_COARSE);

        for coarse in 0..N_COARSE {
            const SENTINEL_H: i64 = -6_004_799_503_160_661;
            const SENTINEL_SOLAR: i64 = 4_611_686_018_427_387_847;
            let mut dst_h = vec![M64::new(SENTINEL_H); N_COARSE as usize];
            let mut solar = vec![M64::new(SENTINEL_SOLAR); N_COARSE as usize];

            fold_energy(
                &energy_delta,
                &light,
                &src_h,
                &mut dst_h,
                &mut solar,
                &p,
                coarse,
            );

            for other in 0..N_COARSE {
                let at = other as usize;
                if other == coarse {
                    assert_eq!(dst_h[at], want_h[at], "enthalpy of the cell dispatched");
                    assert_eq!(solar[at], want_solar[at], "solar of the cell dispatched");
                } else {
                    assert_eq!(
                        dst_h[at].to_i64(),
                        SENTINEL_H,
                        "the invocation for {coarse} wrote the enthalpy of {other}"
                    );
                    assert_eq!(
                        solar[at].to_i64(),
                        SENTINEL_SOLAR,
                        "the invocation for {coarse} wrote the solar slice of {other}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_dark_scenario_moves_no_enthalpy() {
        // `i_surface = 0` is the legal closed-box scenario, and it costs no branch
        // anywhere: the top term of the topmost voxel is `0 - 0`, every term below
        // it is a difference of two zeros, `qmul(0, units_per_intensity)` is zero
        // and `m_delta_64` rounds zero to zero (ADR-076).
        //
        // Said out loud, because it is a property of the allocation and not a
        // decision: this holds only because the light buffer is **created** zeroed.
        // A scenario whose light field held yesterday's beam with `i_surface` since
        // set to zero would absorb a whole layer of it, and nothing here would
        // know.
        let p = FoldParams {
            i_surface: Q::ZERO,
            ..params()
        };
        let energy_delta = vec![M64::ZERO; N_FINE as usize];
        let light = vec![Q::ZERO; N_FINE as usize];
        let src_h: Vec<M64> = (0..N_COARSE)
            .map(|c| M64::new(i64::from(c) * 700_001 - 3))
            .collect();

        let (dst_h, solar) = dispatch_both(&energy_delta, &light, &src_h, &p, N_COARSE);
        assert_eq!(dst_h, src_h);
        for coarse in 0..N_COARSE {
            assert_eq!(
                solar[coarse as usize].to_i64(),
                0,
                "coarse cell {coarse} credited light in the dark"
            );
        }
    }

    #[test]
    fn the_result_does_not_depend_on_the_traversal_order() {
        // Gather form buys this: an invocation reads only the inputs and writes
        // only its own cell. The same statement, and the same reason, as
        // `the_result_does_not_depend_on_the_dispatch_order` for light — on the GPU
        // there is no traversal order at all, so anything this test could catch is
        // unfixable there.
        let p = params();
        let energy_delta = reactions_filled(N_FINE);
        let light = stepped(&p);
        let src_h = vec![M64::new(17); N_COARSE as usize];

        let mut forwards = vec![M64::ZERO; N_COARSE as usize];
        let mut forwards_solar = vec![M64::ZERO; N_COARSE as usize];
        for coarse in 0..N_COARSE {
            fold_energy(
                &energy_delta,
                &light,
                &src_h,
                &mut forwards,
                &mut forwards_solar,
                &p,
                coarse,
            );
        }

        let mut backwards = vec![M64::ZERO; N_COARSE as usize];
        let mut backwards_solar = vec![M64::ZERO; N_COARSE as usize];
        for coarse in (0..N_COARSE).rev() {
            fold_energy(
                &energy_delta,
                &light,
                &src_h,
                &mut backwards,
                &mut backwards_solar,
                &p,
                coarse,
            );
        }

        assert_eq!(forwards, backwards);
        assert_eq!(forwards_solar, backwards_solar);
    }

    #[test]
    fn a_coarse_cell_writes_only_itself() {
        // The operational form of ADR-034 and of the rejection in ADR-045:
        // perturbing one fine increment moves exactly one coarse cell, and the cell
        // next door does not move by a single unit. This is the checkable wording
        // of "atomic addition into the covering cell was rejected".
        const X: u32 = 6;
        const Y: u32 = 5;
        const Z: u32 = 2;

        let p = params();
        let light = stepped(&p);
        let src_h = vec![M64::ZERO; N_COARSE as usize];
        let energy_delta = reactions_filled(N_FINE);
        let before = dispatch(&energy_delta, &light, &src_h, &p, N_COARSE);

        let mut perturbed = energy_delta.clone();
        let at = index(&p, X, Y, Z) as usize;
        perturbed[at] += M64::new(1_000_000);
        let after = dispatch(&perturbed, &light, &src_h, &p, N_COARSE);

        let moved = covering(X, Y, Z);
        for coarse in 0..N_COARSE {
            let delta = after[coarse as usize].to_i64() - before[coarse as usize].to_i64();
            if coarse == moved {
                assert_eq!(delta, 1_000_000, "the perturbation missed its own cell");
            } else {
                assert_eq!(delta, 0, "coarse cell {coarse} moved by {delta}");
            }
        }
    }

    #[test]
    fn reaction_energy_is_summed_as_integers() {
        // The declared span of one voxel for one tick is `3.08e16` units (ADR-062)
        // — fifty-five significant bits, against the twenty-four of `Q`. Adding one
        // makes it a number `f32` cannot represent at all. A kernel that pushed the
        // reaction increment through `qadd` "to match the light" loses the low
        // thirty-one bits and looks like it merely rounded.
        const SPAN: i64 = 30_800_000_000_000_001;

        let p = params();
        let energy_delta = vec![M64::new(SPAN); N_FINE as usize];
        let src_h = vec![M64::ZERO; N_COARSE as usize];

        let dst = dispatch(&energy_delta, &transparent(N_FINE), &src_h, &p, N_COARSE);

        // Sixty-four of them per cell: `1.97e18`, comfortably inside `i64` and
        // hopeless in `f32`.
        for coarse in 0..N_COARSE {
            assert_eq!(
                dst[coarse as usize].to_i64(),
                PER_COARSE * SPAN,
                "coarse cell {coarse} lost bits"
            );
        }
    }

    #[test]
    fn the_fold_credits_what_the_light_kernel_attenuated() {
        // A guard against drift between two files, the way
        // `the_neighbourhood_agrees_with_the_grid` guards the diffusion kernel
        // against `Grid`. The convention "a cell holds what leaves through the
        // bottom face" is decided in `light.rs`; the difference that turns it back
        // into absorption is taken here, and today the only thing connecting them
        // is prose.
        //
        // Compared against the telescoped `i_surface - light[0]` rather than
        // against a hand-built profile: what is at stake is the *pairing* of the
        // terms, not the values.
        let p = params();
        let mut amounts = vec![M32::ZERO; N_FINE as usize];
        for at in 0..N_FINE {
            // Thin voxels: `tau <= 99/1024 < ln 2`, so every difference below is
            // exact in `f32` by Sterbenz and the sum of them is exact in `f64`.
            amounts[at as usize] = M32::new((at as i32 * 7919) % 97 + 3);
        }
        let light = light_field(&amounts);

        for y in 0..NY {
            for x in 0..NX {
                // Top down, so that every partial sum is exactly
                // `INCIDENT - light[z]` and the equality needs no tolerance — the
                // same argument `what_leaves_through_the_floor_is_never_credited`
                // makes in `light.rs`.
                let mut absorbed = 0.0f64;
                for z in (0..NZ).rev() {
                    absorbed += absorbed_here(&light, &p, x, y, z).debug_f64();
                }
                let leaving = light[index(&p, x, y, 0) as usize].debug_f64();
                assert!(absorbed > 0.0, "column ({x}, {y}) absorbed nothing");
                assert_eq!(absorbed, INCIDENT - leaving, "column ({x}, {y})");
            }
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "dispatched over coarse cells")]
    fn a_dispatch_over_fine_voxels_is_refused_rather_than_reading_past_the_field() {
        // The loud half of the dispatch contract, on the precedent of
        // `a_per_voxel_dispatch_is_refused_rather_than_writing_past_the_field` in
        // `light.rs`. The quiet half — a host that dispatches over coarse cells and
        // decodes the index by the fine extents — no assertion over values can see
        // at all; against that there is only the decode by the coarse extents and
        // the words in the header.
        let p = params();
        let energy_delta = vec![M64::ZERO; N_FINE as usize];
        let light = transparent(N_FINE);
        let src_h = vec![M64::ZERO; N_COARSE as usize];
        let mut dst_h = vec![M64::ZERO; N_COARSE as usize];
        let mut solar = vec![M64::ZERO; N_COARSE as usize];

        fold_energy(
            &energy_delta,
            &light,
            &src_h,
            &mut dst_h,
            &mut solar,
            &p,
            N_COARSE,
        );
    }
}
