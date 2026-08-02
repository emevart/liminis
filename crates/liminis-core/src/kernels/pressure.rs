//! Pressure: artificial compressibility, one voxel of signal per tick.
//!
//! Step `e` of the tick order (SPEC section 8). A voxel holding more matter than
//! fits in it pushes the surplus at its neighbours:
//!
//! ```text
//! P     = k*(V_occ/V_voxel - 1)
//! V_occ = sum_i (amount_i / units_per_mol_i) * V_bar_i        (ADR-067)
//! ```
//!
//! ADR-006 hangs biofilm growth, sediment compaction, rising gas bubbles and the
//! capacity of a voxel on that one mechanism, and this file may not know which
//! of the four it is computing. There is no biology here and no branch on what
//! the matter is: a difference of occupancies drives a flux, and that is all.
//!
//! # What is folded, and the one thing that cancels
//!
//! ADR-055 declares `k` in pascals — the bulk modulus of the artificial medium —
//! and **derives** the mobility at load time from the Courant condition, so that
//! at the declared limiting overflow the displacement over one tick is exactly
//! one voxel. Two things follow at once: no number in the config knows about a
//! voxel (SPEC section 10), and the response is always the fastest of the stable
//! ones rather than quietly slower.
//!
//! So the kernel receives one number, [`PressureParams::courant_per_overflow`],
//! and neither the stiffness, nor the mobility, nor `dt`, nor `dx` appears in
//! this file (ADR-034, ADR-015).
//!
//! **The stiffness cancels, and that has to be said out loud.** With
//! `u = -L*grad P` the courant number of a face is
//! `L*k*dt/dx^2 * (theta_low - theta_high)`; deriving `L` from "one voxel per
//! tick at `theta_max`" fixes the whole product `L*k*dt/dx^2` at `1/theta_max`.
//! The folded number is therefore `1/theta_max` and `k` takes no part in the
//! arithmetic whatsoever. The consequence is uncomfortable and belongs in this
//! file rather than in a commit message: a wrong `k` — bars instead of pascals, a
//! factor of `1e5` — changes not one bit of any run, the only declared pressure
//! key turns out to be inert, and calibrating it calibrates nothing. Guarding
//! that is the host's job; naming it is this file's, because ADR-055 closed the
//! unit of `k` precisely against "for years compensated by calibration", and a
//! quantity that cancels is the same disease approached from the other side.
//!
//! # `V_occ` is amounts times partial molar volumes, and nothing else
//!
//! ADR-067 replaced density by `partial_molar_volume` as the declared key, so
//! `V_occ` is `sum (amount/units_per_mol)*V_bar` — **no molar mass and no
//! `1e-3`**. SPEC section 3 still prints both, and the record says outright that
//! it is stale on this point (ADR-067, "the discrepancy with the frozen spec, in
//! three places"). Putting them back would leave a field of entirely plausible
//! shape and would reverse half of ADR-067 without saying so.
//!
//! Two more consequences of that record are load-bearing here:
//!
//! - `V_bar` may be **negative**. Electrostriction is not exotic — an ion orders
//!   the water around it more tightly than water packs itself, so `PO4^3-` has
//!   `V_bar ~ -4.0e-5 m^3/mol` and `Fe^2+` about `-2.4e-5` — and `V_bar = 0` is
//!   legal too. `V_occ` is therefore **unbounded below**, and there is no
//!   `V_occ > 0` check in this file and must not be one;
//! - `1/V_voxel` lives inside [`Occupants::coeff`]. Leaving it out is the natural
//!   folding mistake, and it is the same one `light.rs` names about `dz`: at
//!   `dx = 100 um` it is a factor of `1e12`, the occupancy is monstrous
//!   everywhere, every face saturates, and the picture reads as "the stiffness is
//!   not calibrated yet".
//!
//! # Two kernels, and why they are not one
//!
//! [`overflow_voxel`] fills a field over the whole domain; [`relax_voxel_32`]
//! then reads it. Merging them — computing the neighbour's occupancy on the fly
//! inside the relaxation — is the tempting shortcut and is forbidden twice over.
//! It is either a read from the buffer being written, or `7*n_occupants`
//! operations per voxel in place of one pass (ADR-067 prices a single `V_occ` at
//! 55 to 69 operations, so this is not a rounding error in the budget). The worst
//! variant is the one in between, where the neighbour's overflow comes from `src`
//! and the voxel's own from the `dst` it has just written: the two sides of a
//! face then disagree and matter flows.
//!
//! # One voxel per tick is a ceiling, not slowness
//!
//! ADR-055 answers the objection that local relaxation is too slow by measuring
//! against the source of volume rather than against a Poisson projection: the
//! signal crosses the domain in 128 ticks against 1200 ticks per bacterial
//! generation (SPEC section 1.7). One voxel per tick is **the Courant ceiling** —
//! pressure cannot drive a flux faster without leaving stability — so several
//! relaxation iterations per tick buy nothing, and ADR-055 rejects them by name
//! ("the same as raising `k`, by a detour"). The honest price the same record
//! names: while the signal settles, an overflow of order `128/1200 ~ 11%`
//! accumulates, and measuring it is the job of a golden test with a piston.
//!
//! The kernel cannot know it is being run once every `n` ticks, and that would
//! multiply the effective step of a number already derived at the ceiling.
//! ADR-030 forbids `every_n_ticks > 1` for a diffusive field for exactly this
//! reason; whether the ban extends to this operator is settled nowhere, and
//! `settle.rs` already records the same gap for its own.
//!
//! # Conservation rests on the layout, and the rounding trap is inverted
//!
//! Gather form: a voxel sums the flux across its six faces and writes only its
//! own cell (ADR-034). A face is evaluated **once**, in the canonical orientation
//! of ADR-054 — from the smaller linear index to the larger — and both sides
//! apply that one number with opposite signs.
//!
//! What is antisymmetric here is [`face_courant`], not the flux. The diffusive
//! `flux(a, b) == -flux(b, a)` is **false** and must stay false — the donor is
//! picked by the sign of the courant, not by which side is fuller — while the
//! mirror form `flux(l, h, c) == -flux(h, l, -c)` holds bit for bit, exactly as
//! in `settle.rs`.
//!
//! **The canonical orientation is not what conservation rests on here, and
//! claiming otherwise is how the real gap stayed hidden.** In `advect.rs` and in
//! `settle.rs` the orientation is load-bearing, because a limited flux reads four
//! cells and that tuple is not symmetric: evaluate it from your own end and `Q`
//! arithmetic diverges in the last bit. This kernel has no limiter yet, its flux
//! reads the two cells of the face, and a voxel evaluating every face from its
//! own end would get the negated courant and, by the mirror identity above,
//! exactly the negated flux — so it would subtract precisely what the canonical
//! form adds. The two spellings are the same *numbers*, not merely the same
//! mathematics: IEEE-754 subtraction is exactly antisymmetric, `qmul` carries the
//! sign exactly, and rounding halves away from zero is an odd function. No test
//! in this file separates them and none can be written that does.
//!
//! The canonical pair is written anyway, and the reason is not taste: the day
//! `TODO(limiter)` is answered, the flux reads two cells beyond the face, the
//! tuple stops being symmetric, and the orientation becomes load-bearing exactly
//! as it is in the other two transport kernels. What conservation does rest on is
//! weaker and belongs to the layout rather than to the arithmetic — both voxels
//! of a face form the same pair of addresses and apply the one number with
//! opposite signs — and the failure modes with teeth are the sign lost on one of
//! the two faces of an axis and a clamp of the result at zero.
//!
//! The rule that is genuinely fragile is the **donor**, and it is fragile in the
//! way this file diagnoses for lane-index addressing. Picking the fuller of the
//! two pools instead of the side the courant points from conserves exactly, obeys
//! the mirror identity, leaves a closed face carrying nothing and a uniform
//! occupancy a fixed point, and closes both axes of the ledger — while taking the
//! matter that leaves a voxel out of its *neighbour's* pool. It is not an exotic
//! case: it is the case ADR-006 hangs on this mechanism, a voxel crowded by
//! another substance pushing this one out, where the fuller voxel and the fuller
//! pool are different voxels. `the_donor_is_the_side_the_courant_points_from` is
//! the whole of what stands in the way.
//!
//! The rounding rule is therefore *not* what conservation rests on, and the trap
//! is the inverted one `settle.rs` describes in its own header. Both sides of a
//! face apply the same rounded number, so `floor` would conserve perfectly and
//! `pressure_relaxation_conserves_exactly` would stay green. What `floor` breaks
//! is direction: `floor(c*a)` understates a positive flux and overstates the
//! magnitude of a negative one, which is a systematic drift of matter toward the
//! origin of the grid on every face where `|c*a|` is small. No conservation test,
//! no per-face test and no undershoot bound can see that.
//!
//! One corollary is **restored** here after being false twice: a closed face
//! carries nothing without a branch. In diffusion that follows from
//! `flux(a, a) == 0`; in advection and settling it is false, because a directed
//! flux is `c*a` whatever the neighbour holds. Here it holds again, and for a
//! third reason — the velocity itself is a difference, so a face whose neighbour
//! is the voxel itself has a zero courant. It is restored rather than inherited,
//! and the distinction is operational: the moment a limiter is added to the flux
//! (see `TODO(limiter)`), the antidiffusive term stops being proportional to the
//! difference and the wall starts to leak.
//!
//! # What is not here
//!
//! Channel counters: matter leaving through an `exchange` face belongs to
//! `BOUNDARY_EXCHANGE` (ADR-059), `periodic_mask` encodes two boundary states,
//! and the host is required to refuse such a grid — the same refusal `diffuse.rs`
//! and `settle.rs` already record.
//!
//! Cells. ADR-006 hangs cell motility and predation-by-engulfment on this same
//! mechanism, and SPEC section 3 adds the structural mass of cells to `V_occ`.
//! That is S2, the cell table does not exist, and `TODO(occupants)` says so
//! rather than leaving it to be inferred from an absence.
//!
//! The process. There is no `process/pressure.rs`, no overflow field in `world/`,
//! and no scenario with `[[process]] id = "pressure"` enabled (CONFIG_SCHEMA
//! section 12). The host owns the `Q` field the way it owns the light field —
//! `kernels/light.rs` did not touch `world/` either.

use crate::numeric::{M32, M64, Q, q_conc_32, q_conc_64, q_round_32, q_round_64, qadd, qmul, qsub};

/// Parameters of one application. Scalars only: in WGSL this is a uniform
/// buffer, and every number in it is a place where the host's scale can drift
/// away from the kernel's (`ARCHITECTURE.md`).
///
/// The same `Copy` struct of scalars as `DiffuseParams` and `SettleParams`
/// (ADR-034, ADR-015).
#[derive(Clone, Copy, Debug)]
pub struct PressureParams {
    /// Voxels along X.
    pub nx: u32,
    /// Voxels along Y.
    pub ny: u32,
    /// Voxels along Z.
    pub nz: u32,
    /// `nx*ny*nz`, the stride between two lanes of the same amount buffer. It
    /// arrives rather than being recomputed inside the shader, by the argument
    /// `LightParams` gives: the host knows it, and a product of three `u32` in a
    /// kernel is one more place for an overflow that shows up only on the
    /// largest grid anybody runs.
    pub n_voxels: u32,
    /// The stride of an amount lane. On a `world::Field` it is `n_voxels + 1`
    /// — the voxels and the ghost cell after them (ADR-059) — and the host is
    /// the only place that knows so; this kernel is handed the number.
    ///
    /// Separate from [`PressureParams::n_voxels`] because the two mean different things
    /// and only one of them is an address. A dispatch runs over `0..n_voxels`;
    /// an amount lives at `lane[s] * lane_len + idx`. Using the voxel count as
    /// the stride reads one lane short of where the substance is, and on the
    /// registry the project carries that is **right** for the first lane of a
    /// width class and wrong by one element per lane after it — which is the
    /// same silent shape ADR-056 warns about for `s * n_voxels + idx`.
    pub lane_len: u32,
    /// How many entries of [`Occupants`] to sum. The table may be longer; the
    /// scenario decides how many substances take up room.
    pub n_occupants: u32,
    /// Bit `a` set: entry `a` of the occupancy table reads the 64-bit slice
    /// (ADR-040, ADR-056).
    ///
    /// The bit indexes the **table entry**, not the substance and above all not
    /// the voxel: the branch is then the same for every voxel of the dispatch,
    /// so on the GPU it is a uniform jump and the warp holds together. Branching
    /// on the width inside the loop over voxels instead is invisible on the CPU
    /// and diverges the warp on the GPU — an error in the price rather than in
    /// the result, and one that surfaces only after the port.
    pub width_mask: u32,
    /// Bit `f` set: face `f` is periodic and wraps. Bit clear: the face is closed
    /// and its neighbour is the voxel itself, which carries no flux because the
    /// courant number of that face is then a difference of one value with itself
    /// (`world::Grid::neighbour`).
    ///
    /// The bit index is the discriminant of `world::Face`: `0 = x_min`,
    /// `1 = x_max`, `2 = y_min`, `3 = y_max`, `4 = z_min`, `5 = z_max`.
    pub periodic_mask: u32,
    /// The courant number per unit of overflow difference across a face, folded
    /// on the host (ADR-055).
    ///
    /// The only number in this file that knows any physics, and it is the whole
    /// of it: mobility, stiffness, `dt` and `dx` collapse into it, and by the
    /// derivation of ADR-055 it equals `1/theta_max` — see the module header for
    /// why `k` cancels out of it entirely.
    ///
    /// `w` in `settle.rs` arrives the same way and for the same reason, and
    /// ADR-067 states the form word for word: the number is folded on the host in
    /// `f64` and arrives as one `Q`, with no bare operators over `Q` inside
    /// (ADR-022).
    // TODO(theta-max): the limiting overflow ADR-055 derives the mobility from is
    // **declared nowhere**. `CONFIG_SCHEMA.md` section 7 carries only the
    // stiffness `k` on the pressure row — and not even an ASCII name for it,
    // section 13 item 23 — while `QUANTITIES.md` section 4 has `k` and the
    // mobility as derived. Without `theta_max` this number has no value at all,
    // and a plausible one invented here would be indistinguishable from a
    // decision. It is settled by a record in the journal plus a key in
    // `CONFIG_SCHEMA.md` section 7, together with the question this work raises
    // anew: since `k` cancels, is it still a key of the scenario, and what does
    // it observe.
    pub courant_per_overflow: Q,
}

/// The occupancy table: flat, read-only, `n_occupants` long.
///
/// Built on the pattern of `Attenuators` in `kernels/light.rs`, which is the
/// pattern of `Rx<'a>` in `ARCHITECTURE.md`: a named set of bindings, each of
/// which becomes one `var<storage, read>` in WGSL. Not an interface — no trait,
/// no `dyn`, no behaviour inside.
// TODO(occupants): which lanes belong in this table is not decided. The whole
// registry is the only reading that matches ADR-006 — every substance takes up
// room — and a table assembled from "the ones that occupy an appreciable volume"
// makes `V_occ` smaller than the truth, the occupancy nearly uniform and the
// mechanism of ADR-006 a no-op, while reading as "the coefficients are not
// calibrated yet". Whether a scenario may declare a subset, and whether the
// displacement of cells joins this same step (SPEC section 6; ADR-006 hangs
// motility and predation on pressure), belongs in the journal. Cells are S2, but
// that has to be declared rather than passed over in silence.
pub struct Occupants<'a> {
    /// `lane[a]`: the index of the lane **within its own width class**, resolved
    /// by the host through `Registry::lane_of` (ADR-056). The kernel does no
    /// `s -> lane` resolution of its own; the address is `lane[a]*n_voxels + at`
    /// and never the stale `s*n_voxels + idx` of the skeleton in
    /// `ARCHITECTURE.md`.
    ///
    /// `lane == s` is not true anywhere, and a table built out of substance
    /// indices reads some other substance's amounts while nothing falls over: the
    /// occupancy stays plausible, pressure still looks like a field, and
    /// conservation is not touched at all — the flux is antisymmetric whatever
    /// the courant was computed from. Only a test with **different** amounts in
    /// two lanes can see it.
    pub lane: &'a [u32],
    /// `coeff[a]`: `V_bar_a / (units_per_mol_a * V_voxel)`, the dimensionless
    /// fraction of a voxel taken by one storage unit, folded into one number
    /// (ADR-067).
    ///
    /// Neither a molar mass nor a factor of `1e-3` is in this product, and
    /// neither may return: `V_occ` is amounts times partial molar volumes (see
    /// the module header). The sign is the sign of `V_bar` and carries
    /// electrostriction, so a negative entry is a declaration and not a defect.
    pub coeff: &'a [Q],
}

/// The overflow of one voxel: `V_occ/V_voxel - 1`, dimensionless.
///
/// `sum_a coeff[a] * amount[lane[a], idx]`, less one. Writes `dst[idx]` and
/// nothing else, reads no neighbour and does not read `dst`; the result does not
/// depend on the order the voxels are visited in (ADR-034).
///
/// One named crossing from `M` into `Q` and not one crossing back (ADR-060) — the
/// same composition `optical_depth` uses in `kernels/light.rs`, with `Q::ONE` as
/// the per-unit factor saying "this amount, as a number", because everything the
/// amount has to be multiplied by is already inside `coeff`. **Nothing here
/// rounds**, because nothing here goes back to an integer.
///
/// # The result may be negative, and the kernel will not stop it
///
/// Two independent reasons, and neither is a defect. A voxel may hold less than
/// fills it, and `V_bar` may itself be negative (ADR-067: electrostriction, and
/// `V_occ` "is unbounded below"). A check that `V_occ > 0`, or a clamp of the
/// result at zero, is the most natural edit the next reader will make, and it
/// would silently reverse half of ADR-067;
/// `a_negative_partial_molar_volume_lowers_the_occupancy` is what stands in the
/// way.
// TODO(one-sided): whether pressure is one-sided is not decided, and the
// difference is not cosmetic. SPEC section 3 writes "at `V_occ > V_voxel` a
// pressure arises", which reads as `P = max(0, k*theta)`; ADR-055 says the
// mechanism "stays as written" and does not distinguish. Under the one-sided
// reading an underfilled domain draws nothing in at all, and the project's own
// example — four substances and no solvent, occupancy about `4e-4` (ADR-067) —
// gives an identically zero field, so the whole of step `e` is an expensive
// no-op indistinguishable from "the scenario is not populated yet". Under the
// two-sided reading emptiness sucks its neighbours in, which negative `V_bar`
// makes legal in its own right. This kernel writes the two-sided value because it
// is the one from which either can be formed; clamping it here would destroy the
// information and is exactly the edit the journal has to authorise first.
//
// TODO(overflow-field): where this field lives and what it costs is nobody's
// decision yet. The precedent for the price is ADR-045 — four bytes per voxel, on
// a base that ADR-062 and ADR-067 have already moved to 234 bytes and 490 MB at
// 128^3 — and no record has budgeted it; `[[field]]` in `CONFIG_SCHEMA.md`
// section 6 does not know it. The same place has to answer whether the stored
// field holds pascals or the dimensionless overflow, because the observability of
// `k` depends on the answer.
pub fn overflow_voxel(
    src32: &[M32],
    src64: &[M64],
    dst: &mut [Q],
    occ: &Occupants,
    p: &PressureParams,
    idx: u32,
) {
    let mut occupied = Q::ZERO;

    for a in 0..p.n_occupants {
        // ADR-056, not the stale `s*n_voxels + idx` of the skeleton: the lane is
        // an index inside a width class and the host resolved it.
        let cell = (occ.lane[a as usize] * p.lane_len + idx) as usize;

        // The branch is on the table entry, the same value for every voxel of
        // the dispatch, so on the GPU it is a uniform jump (ADR-040).
        let amount = if p.width_mask & (1 << a) != 0 {
            q_conc_64(src64[cell], Q::ONE)
        } else {
            q_conc_32(src32[cell], Q::ONE)
        };

        occupied = qadd(occupied, qmul(occ.coeff[a as usize], amount));
    }

    dst[idx as usize] = qsub(occupied, Q::ONE);
}

/// The courant number of one face, in the canonical orientation of ADR-054: from
/// the smaller linear index to the larger.
///
/// `(theta_low - theta_high) * courant_per_overflow`, so a **positive** value
/// means transport toward the larger index — matter leaves the fuller side, and
/// the direction is a property of the difference rather than of a stored
/// velocity.
///
/// This is a function of a **face**, not of a voxel, and that is the whole of its
/// contract: the two voxels sharing a face call it with one pair in one order and
/// get back one number, which they then apply with opposite signs. Evaluating it
/// "from your own side" is algebraically the same and bitwise different, and the
/// temptation is stronger here than in settling precisely because the function is
/// antisymmetric — see the module header.
///
/// Public for the reason `limited_slope_32` is public in `advect.rs`: a test that
/// could only see the flux would pass against a ceiling quietly set six times too
/// low, and the ceiling is what ADR-055 asserts.
///
/// # Antisymmetry, exactly
///
/// `face_courant(a, b) == -face_courant(b, a)` for every pair, because `qsub` is
/// a subtraction and `qmul` carries the sign in IEEE-754 and in fixed point
/// alike. Two corollaries lean on it: a closed face, whose neighbour is the voxel
/// itself, has a courant of exactly zero and therefore carries nothing without a
/// branch; and a uniform occupancy is a fixed point of the whole scheme, whatever
/// the amounts behind it are.
// TODO(courant-condition): which condition of SPEC section 4.2 the derived
// mobility satisfies is not decided, and the gap is sixfold. `max over faces
// |c| <= 1` and the strict `sum over outgoing faces |c| <= 1` differ by exactly
// the six faces of a voxel at a local maximum, which at `c = 1` on each of them
// gives away six of its pools. ADR-055 says "the displacement over a tick equals
// one voxel"; ADR-069 counts "at most 1 voxel from pressure" inside a sum of
// 2.17; neither picks.
//
// Worse, and separately: for pressure the courant number is a function of the
// **state**, not of the config, so the load-time check that guards advection and
// settling does not exist here and cannot be built. And a third condition, which
// neither of the two named ones implies, is the one under which this relaxation
// actually relaxes: the donor factor of the flux is the whole pool, so
// linearising about a uniform state gives an effective `alpha` of
// `courant_per_overflow * (theta + 1)` — the **occupancy**, not the difference —
// and the seven-point stability limit is `6*alpha <= 1`. Above it neighbouring
// voxels swap pools every tick, conserving matter exactly, with antisymmetry, the
// closed face, the uniform fixed point and both axes of the ledger all green.
// `pressure_relaxation_reaches_hydrostatic_equilibrium` is the only test in this
// file that can see it, and it picks its number from this third condition because
// the corpus does not offer it one.
#[inline(always)]
pub fn face_courant(low: Q, high: Q, p: &PressureParams) -> Q {
    qmul(qsub(low, high), p.courant_per_overflow)
}

/// Generates one storage width of the pressure kernel.
///
/// Invoked twice, immediately below. ADR-040 derives the storage width of an
/// amount from its declared concentrations, so both widths have to exist, and
/// `NUMERIC.md` section 5 already settled how a two-width axis is expressed:
/// generate both from one text, as `define_diffuse!` and `define_settle!` do. Two
/// hand-written copies drift apart within a month, and the drift surfaces as
/// "water somehow has different dynamics".
macro_rules! define_pressure {
    ($voxel:ident, $flux:ident, $m:ty, $q_conc:ident, $q_round:ident) => {
        #[doc = concat!("The pressure flux across one face, in `", stringify!($m), "`.")]
        ///
        /// The signed amount crossing **from `low` to `high`**, that is, toward
        /// the larger linear index. Donor-acceptor upwinding, first order, with
        /// the donor picked by the **sign of the courant** and never by which
        /// side is fuller.
        ///
        /// The two rules are not told apart by conservation, and it is worth
        /// saying so where the choice is made rather than leaving it to be
        /// discovered: both sides of a face see one pair and would pick one donor
        /// under either rule, so the fuller-pool version conserves just as
        /// exactly. What it gets wrong is whose pool the matter comes out of,
        /// which is the whole content of upwinding — matter leaves the voxel the
        /// pressure is pushing it out of, and the two voxels differ in occupancy
        /// for reasons that have nothing to do with how much of *this* substance
        /// they hold (ADR-006, ADR-067). Held by
        /// `the_donor_is_the_side_the_courant_points_from`, and by nothing else
        /// in this file.
        ///
        /// The name is `flux` rather than `pressure_flux` because
        /// `kernels/mod.rs` already declared that every transport kernel has a
        /// function called `flux` and that the module is what tells them apart.
        ///
        /// # Where the classes cross
        ///
        /// Once, at the end, through the two named crossings composed (ADR-060):
        /// `q_conc` down with a unit factor, `q_round` up. One rounding per
        /// **face**, not one per voxel over the sum of its six: the neighbour
        /// would round its own sum rather than the opposite of this one, and the
        /// two would stop being exact negations.
        ///
        /// # Antisymmetry: read the module header before "fixing" this
        ///
        /// `flux(a, b, c) == -flux(b, a, c)` is **false** here and has to be —
        /// `flux(a, b, c) = round(c*a)` while `flux(b, a, c) = round(c*b)`.
        /// Whoever repairs that by making the flux a function of the difference
        /// has written diffusion with a different name for `alpha`. What holds,
        /// bit for bit, is the mirror form
        ///
        /// ```text
        /// flux(l, h, c) == -flux(h, l, -c)
        /// ```
        ///
        /// the same one `settle.rs` has, because the mirrored call picks the same
        /// donor and only the sign of the multiplication changes. And what
        /// conservation rests on is weaker still and comes from the layout: both
        /// voxels of a face call this with the identical pair in the identical
        /// order.
        // TODO(limiter): first order or a van Leer limiter is undecided. ADR-054
        // introduces the limiter for advection and ADR-067 takes it whole into
        // settling; nobody has said anything about pressure. The choice is not
        // cosmetic in either direction: with a limiter the corollary "a closed
        // face carries nothing without a branch" breaks, because the
        // antidiffusive term is proportional to the slope rather than to the
        // difference across the face, and the TVD argument of van Leer is derived
        // for a divergence-free field whereas the pressure flux is divergent by
        // construction — that is its whole purpose (ADR-069).
        #[inline(always)]
        pub fn $flux(low: $m, high: $m, courant: Q) -> $m {
            // Toward the larger index, hence `low` is the donor. `>=` and not
            // `>`: at a zero courant either choice gives zero, and a comparison
            // is not arithmetic (NUMERIC.md section 2).
            let toward_high = courant >= Q::ZERO;
            let donor = if toward_high { low } else { high };

            $q_round(qmul(courant, $q_conc(donor, Q::ONE)))
        }

        #[doc = concat!("One application of pressure in one voxel, in `", stringify!($m), "`.")]
        ///
        /// Sums the flux across the six faces and writes `dst[idx]` and nowhere
        /// else. `overflow` is an **input**, computed by [`overflow_voxel`] off
        /// the same snapshot `N` as `src`; nothing is read from `dst`, so the
        /// result does not depend on the order the voxels are visited in
        /// (ADR-034).
        ///
        /// Every voxel of the lane must be visited before the buffers are
        /// swapped: a skipped voxel is not left unchanged, it is left a tick
        /// stale.
        ///
        /// # A two-deep periodic axis carries the flux twice, and that is right
        ///
        /// With an extent of two and a periodic boundary, both faces of the axis
        /// lead to the same neighbour and both resolve to the same canonical
        /// pair, so the flux is applied twice in the same direction. That is
        /// diffusion's behaviour, not settling's: there really are two faces
        /// between the two cells, and a gradient drives matter through both,
        /// whereas a *directed* velocity crosses one of them each way and cancels
        /// (`settle.rs`, `an_axis_one_voxel_deep_carries_nothing`). Conservation
        /// is untouched either way; what doubles on such a grid is the courant
        /// budget of that axis. An axis one voxel deep is its own neighbour and
        /// carries nothing at all.
        ///
        /// # The snapshot the overflow was taken from is part of the contract
        ///
        /// ADR-057 requires the front buffer of every lane to hold state `N` at a
        /// process boundary. An overflow field taken before somebody else's step
        /// and relaxed after it makes half the domain relax against last tick,
        /// and conservation is exact either way — the difference shows up only as
        /// "pressure lags a little".
        ///
        /// Applying pressure to some lanes and not others has the same shape of
        /// silence: the composition of a voxel drifts while every lane conserves
        /// separately, so both axes of the invariant of ADR-028 close and the
        /// ledger says nothing. Skipping water because it is the one `i64`
        /// substance and goes down the other branch is the concrete way to do it.
        ///
        /// # Amounts may go negative, and this kernel will not stop them
        ///
        /// Each face rounds its flux to a whole unit, and past the declared
        /// limiting overflow a single face asks for more than the pool holds.
        /// Clamping is not an option and not an oversight: the neighbour has
        /// already been given the matter in the same application, so `max(0, .)`
        /// over the result would print units and break the exact equality of sums
        /// the whole scheme is built on (ADR-068).
        ///
        /// The undershoot bound of ADR-068 is **not** inherited here, and copying
        /// it from `settle.rs` would be worse than having none. That bound is
        /// derived at a constant `alpha`, through the step "at `f*alpha <= 1` the
        /// first term is non-negative"; here the courant is a function of the
        /// state and exceeds one on any gradient steeper than the declared limit,
        /// so the bound stops existing — while a test copied with a small courant
        /// stays green and promises a guarantee that is not there.
        // TODO(overshoot): what the scheme should do when the runtime overflow
        // exceeds the declared limit is not decided. ADR-055 names the transient
        // of about 11% itself and requires a golden test with a piston to measure
        // it, but says nothing about `c > 1`. The three honest answers are the
        // ones ADR-068 lists — accept with a proven bound, tighten at load time,
        // or a limiter — and the choice is about the physics, so its place is the
        // journal. What must **not** happen meanwhile is a clamp of the courant
        // at `+-1`, which is the first edit anyone will make on seeing a negative
        // amount: ADR-069 rejects exactly that ("trimming the speed turns a
        // config error into quietly distorted physics"), and it would make a
        // violation of the limit invisible.
        pub fn $voxel(src: &[$m], overflow: &[Q], dst: &mut [$m], p: &PressureParams, idx: u32) {
            let here = src[idx as usize];
            let mut net = <$m>::ZERO;

            for face in 0..6u32 {
                let n = neighbour(p, idx, face);

                // The canonical orientation of ADR-054, assembled the same way
                // from either side of the face: the smaller linear index first.
                // A closed face has `n == idx`, so both ends are the same cell,
                // the courant is a difference of one value with itself, and the
                // face carries nothing without a branch.
                let at_low = if n < idx { n } else { idx };
                let at_high = if n < idx { idx } else { n };

                let c = face_courant(overflow[at_low as usize], overflow[at_high as usize], p);
                let carried = $flux(src[at_low as usize], src[at_high as usize], c);

                // One number, two signs (ADR-005). The flux points toward the
                // larger index, so the low side loses exactly what the high side
                // gains, and the sum over the domain is identically zero without
                // an atomic anywhere.
                if idx == at_low {
                    net -= carried;
                } else {
                    net += carried;
                }
            }

            dst[idx as usize] = here + net;
        }
    };
}

define_pressure!(relax_voxel_32, flux_32, M32, q_conc_32, q_round_32);
define_pressure!(relax_voxel_64, flux_64, M64, q_conc_64, q_round_64);

/// The linear index of a voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
///
/// The kernel's own copy of `world::Grid::index`, and it has to be its own:
/// `kernels/` depends on `numeric/` and on nothing else, and a `Grid` is a host
/// type with no meaning in WGSL. `world/grid.rs` is the authority the copy has to
/// agree with, and `the_neighbourhood_agrees_with_the_grid` keeps them together.
#[inline(always)]
fn index(p: &PressureParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

/// One step down an axis, or what the boundary says instead.
///
/// A comparison rather than a modulo: `(coord + extent - 1) % extent` is the same
/// answer and one integer division, the expensive instruction of the whole lookup
/// on a GPU.
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

/// The neighbour of a voxel across one of its six faces.
///
/// `face` is the discriminant of `world::Face`, because WGSL has no enums and the
/// kernel loops over a plain integer. A **closed** face returns the voxel itself:
/// that is not a sentinel for "no neighbour", it is the answer that makes the
/// face carry no flux without a branch, since the courant of a face is a
/// difference of the two overflows. The same is true of an axis one voxel deep,
/// periodic or not — the torus closes onto itself.
#[inline(always)]
fn neighbour(p: &PressureParams, idx: u32, face: u32) -> u32 {
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    let periodic = (p.periodic_mask >> face) & 1 == 1;

    let mut nbx = x;
    let mut nby = y;
    let mut nbz = z;
    if face == 0 {
        nbx = step_down(x, p.nx, periodic);
    } else if face == 1 {
        nbx = step_up(x, p.nx, periodic);
    } else if face == 2 {
        nby = step_down(y, p.ny, periodic);
    } else if face == 3 {
        nby = step_up(y, p.ny, periodic);
    } else if face == 4 {
        nbz = step_down(z, p.nz, periodic);
    } else {
        nbz = step_up(z, p.nz, periodic);
    }

    index(p, nbx, nby, nbz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, Face, Grid};

    /// A deliberately non-cubic grid: on a cubic one every bug that swaps two
    /// axes is invisible, because all three strides are equal.
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

    /// Two occupants. Entry 0 reads the narrow buffer, entry 1 the wide one.
    const N_OCCUPANTS: u32 = 2;
    const WIDTH_MASK: u32 = 0b10;

    /// The lanes are deliberately not the table entries (ADR-056), and each width
    /// class has a decoy lane the table must not read. Were `lane == a`, every
    /// test here would pass against a kernel with the entry index hard-coded into
    /// the address.
    const LANE_32: u32 = 1;
    const LANE_64: u32 = 0;
    const N_LANES_32: u32 = 2;
    const N_LANES_64: u32 = 2;
    const LANES: [u32; 2] = [LANE_32, LANE_64];

    /// Folded occupancy coefficients, one per table entry:
    /// `V_bar/(units_per_mol * V_voxel)`. Powers of two, so that every product
    /// below is exact and the assertions are about the kernel rather than about
    /// `f32`.
    const COEFF_32: f64 = 1.0 / 1024.0;
    const COEFF_64: f64 = 1.0 / 2048.0;

    /// The folded courant numbers the sweeps walk. The last two are past the
    /// ceiling on this pattern — a face carrying the whole donor pool and more —
    /// because conservation is a property of the tuple and must not care.
    const FOLDED: [f64; 6] = [0.0, 1.0e-3, 1.0 / 16.0, 1.0 / 6.0, 1.0 / 3.0, 1.0];

    fn params(periodic_mask: u32, folded: f64) -> PressureParams {
        PressureParams {
            lane_len: N_VOXELS,
            nx: NX,
            ny: NY,
            nz: NZ,
            n_voxels: N_VOXELS,
            n_occupants: N_OCCUPANTS,
            width_mask: WIDTH_MASK,
            periodic_mask,
            courant_per_overflow: Q::from_f64(folded),
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

    /// The lane the transport tests move, as the host hands it over: one
    /// contiguous slice, with the lane gone from the arithmetic (ADR-056).
    fn lane_of_32(src: &[M32]) -> Vec<M32> {
        let base = (LANE_32 * N_VOXELS) as usize;
        src[base..base + N_VOXELS as usize].to_vec()
    }

    fn lane_of_64(src: &[M64]) -> Vec<M64> {
        let base = (LANE_64 * N_VOXELS) as usize;
        src[base..base + N_VOXELS as usize].to_vec()
    }

    fn write_lane_32(src: &mut [M32], lane: &[M32]) {
        let base = (LANE_32 * N_VOXELS) as usize;
        src[base..base + N_VOXELS as usize].copy_from_slice(lane);
    }

    fn write_lane_64(src: &mut [M64], lane: &[M64]) {
        let base = (LANE_64 * N_VOXELS) as usize;
        src[base..base + N_VOXELS as usize].copy_from_slice(lane);
    }

    /// The overflow field over the whole domain, the way the host runs the first
    /// of the two kernels.
    fn overflow_field(src32: &[M32], src64: &[M64], p: &PressureParams) -> Vec<Q> {
        let coeff = coeffs();
        let occ = Occupants {
            lane: &LANES,
            coeff: &coeff,
        };
        let mut dst = vec![Q::ZERO; N_VOXELS as usize];
        for idx in 0..N_VOXELS {
            overflow_voxel(src32, src64, &mut dst, &occ, p, idx);
        }
        dst
    }

    /// One application over the whole lane, the way the host runs the second: the
    /// overflow field first, off the same snapshot, then the relaxation.
    fn apply_32(src32: &[M32], src64: &[M64], p: &PressureParams) -> Vec<M32> {
        let overflow = overflow_field(src32, src64, p);
        let lane = lane_of_32(src32);
        let mut dst = vec![M32::ZERO; N_VOXELS as usize];
        for idx in 0..N_VOXELS {
            relax_voxel_32(&lane, &overflow, &mut dst, p, idx);
        }
        dst
    }

    fn apply_64(src32: &[M32], src64: &[M64], p: &PressureParams) -> Vec<M64> {
        let overflow = overflow_field(src32, src64, p);
        let lane = lane_of_64(src64);
        let mut dst = vec![M64::ZERO; N_VOXELS as usize];
        for idx in 0..N_VOXELS {
            relax_voxel_64(&lane, &overflow, &mut dst, p, idx);
        }
        dst
    }

    fn as_i64_32(buffer: &[M32]) -> Vec<i64> {
        buffer.iter().map(|v| v.to_i64()).collect()
    }

    fn as_i64_64(buffer: &[M64]) -> Vec<i64> {
        buffer.iter().map(|v| v.to_i64()).collect()
    }

    fn total_32(buffer: &[M32]) -> i64 {
        buffer.iter().map(|v| v.to_i64()).sum()
    }

    fn total_64(buffer: &[M64]) -> i64 {
        buffer.iter().map(|v| v.to_i64()).sum()
    }

    /// The largest difference of the overflow field over the domain.
    fn spread_of(field: &[Q]) -> f64 {
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        for value in field {
            lowest = lowest.min(value.debug_f64());
            highest = highest.max(value.debug_f64());
        }
        highest - lowest
    }

    /// An occupancy that varies from voxel to voxel in a way no symmetry of the
    /// stencil can cancel by accident, and whose two lanes hold **different**
    /// numbers — the only shape in which a table addressed by substance index
    /// instead of lane is visible at all.
    ///
    /// Entry 0 contributes `1..5` voxel volumes, entry 1 contributes `0..6`, so
    /// the overflow runs over `0..10` and the steepest face is a difference of
    /// several.
    ///
    /// # Three properties of the periods, and every one of them is load-bearing
    ///
    /// The multipliers are not decoration, and a fixture that lost any of them
    /// would leave whole tests running on a state that cannot fail them.
    ///
    /// Both amounts vary along **all three axes**. `5` and `7` are coprime with
    /// the strides `1`, `NX = 3` and `NX*NY = 12` and with the wrapped steps of
    /// the torus, so no face of the domain has a zero courant by construction. A
    /// state constant along Z — which `(at/3) % 2` and `(at*7) % 3` both were,
    /// because a Z step adds `4` to `at/3` and `84` to `at*7` — makes every Z
    /// face carry nothing, and then `FLOORED` re-runs the `TORUS` arithmetic bit
    /// for bit: the floor and the lid of the eco-regime default (SPEC section
    /// 1.6) are the only walls the project actually runs with, and a sweep over
    /// the three masks would be testing two of them.
    ///
    /// The two lanes vary **independently**, with different periods. The moving
    /// lane is one of the two summands of the occupancy, so with a single lane —
    /// or with two in step — the order of the occupancies across a face and the
    /// order of the pools are the same order, and the donor rule cannot be seen
    /// (`the_donor_is_the_side_the_courant_points_from`, which counts the faces
    /// where the two disagree in this very state).
    ///
    /// And the amounts stay multiples of the storage units the coefficients
    /// invert, so every occupancy is a small integer and every assertion is about
    /// the kernel rather than about `f32`.
    fn filled() -> (Vec<M32>, Vec<M64>) {
        let mut src32 = narrow();
        let mut src64 = wide();
        for at in 0..N_VOXELS {
            put_32(&mut src32, at, 1024 * (1 + (at as i32 * 7) % 5));
            put_64(&mut src64, at, 2048 * i64::from((at * 3) % 7));
            // Decoys in the lanes the table must not read.
            src32[at as usize] = M32::new(1_000_000 + at as i32);
            src64[(N_VOXELS + at) as usize] = M64::new(-9_000_000 - i64::from(at));
        }
        (src32, src64)
    }

    /// The `n`-th face neighbourhood of a voxel, assembled by walking the
    /// kernel's own lookup: the set of voxels reachable in at most `n` face
    /// crossings.
    fn ball(p: &PressureParams, centre: u32, n: u32) -> Vec<bool> {
        let mut inside = vec![false; N_VOXELS as usize];
        inside[centre as usize] = true;
        for _ in 0..n {
            let mut grown = inside.clone();
            for idx in 0..N_VOXELS {
                if !inside[idx as usize] {
                    continue;
                }
                for face in 0..6u32 {
                    grown[neighbour(p, idx, face) as usize] = true;
                }
            }
            inside = grown;
        }
        inside
    }

    /// Every voxel that moved is inside the `n`-th neighbourhood, and at least
    /// one did — the second half being what keeps the first from holding
    /// vacuously over a lane nothing touched.
    ///
    /// Takes the two states already in `i64` so that one text serves both
    /// widths: a generic helper would need a trait bound, and a trait is the one
    /// shape this directory does not use even in its tests.
    fn assert_reached_at_most(was: &[i64], now: &[i64], inside: &[bool], n: u32, mask: u32) {
        let mut moved = 0;
        for at in 0..N_VOXELS as usize {
            if now[at] != was[at] {
                moved += 1;
                assert!(
                    inside[at],
                    "voxel {at} changed at step {n} and is more than {n} faces \
                     from the disturbance (mask {mask:#08b})"
                );
            }
        }
        assert!(
            moved > 0,
            "nothing moved at all at step {n} (mask {mask:#08b})"
        );
    }

    /// `ACCEPTANCE.md`, section "Conservation".
    ///
    /// Exactly, not to within a tolerance: conservation here is the property that
    /// the two sides of a face apply one number, so any drift at all means the
    /// property is broken rather than small.
    ///
    /// The sweep is deliberate in all three of its axes. The three masks put the
    /// closed-face branch under load — on a torus a kernel that destroys matter
    /// at a wall is invisible, because there is no wall — and the mask sweep is
    /// worth its runtime **only** because `filled()` varies along all three axes:
    /// over a state constant along Z every Z face has a zero courant, and
    /// `FLOORED`, the one mask that has the floor and the lid the project runs
    /// with, repeats the `TORUS` arithmetic bit for bit. Both widths, because
    /// they come out of one macro and a drift between them is silent. And the
    /// folded courant runs past the point where a face carries the whole donor
    /// pool, because conservation is a property of the tuple and not of the data:
    /// it has to hold where the scheme is already unstable.
    ///
    /// What this test fails on: the sign lost on one of the two faces of an axis,
    /// a face evaluated against a neighbour that does not have it, and any clamp
    /// of the result at zero.
    ///
    /// What it does **not** fail on, stated here because the natural reading of
    /// "conserves exactly" is that it covers everything: a flux computed "from
    /// your own side" rather than from the canonical pair of ADR-054 is bit for
    /// bit this same kernel and cannot be caught by anything (see the module
    /// header), and the fuller-of-the-two donor rule conserves just as exactly
    /// while moving the wrong matter — that one is held by
    /// `the_donor_is_the_side_the_courant_points_from` and by nothing here.
    #[test]
    fn pressure_relaxation_conserves_exactly() {
        for mask in [TORUS, FLOORED, CLOSED] {
            for folded in FOLDED {
                let p = params(mask, folded);
                let (src32, src64) = filled();

                let before = lane_of_32(&src32);
                let after = apply_32(&src32, &src64, &p);
                assert_eq!(
                    total_32(&before),
                    total_32(&after),
                    "mask {mask:#08b}, folded {folded}"
                );

                let before = lane_of_64(&src64);
                let after = apply_64(&src32, &src64, &p);
                assert_eq!(
                    total_64(&before),
                    total_64(&after),
                    "mask {mask:#08b}, folded {folded}, 64-bit"
                );
            }
        }
    }

    /// `ACCEPTANCE.md`, section "Conservation", and the operational form of the
    /// central claim of ADR-055. Two assertions, and neither alone is what that
    /// record requires.
    ///
    /// **The ceiling holds.** After `n` applications the set of voxels that differ
    /// from the initial state is contained in the `n`-th face neighbourhood of the
    /// perturbed voxel — at **any** overflow, including one far past the declared
    /// limit. Pressure does not act at a distance however much of it there is, and
    /// the two ways to break that are a second relaxation iteration inside one
    /// application (rejected by name in ADR-055) and reading a neighbour's
    /// overflow out of the buffer being written, which carries the signal across
    /// the whole domain in one tick on a forward CPU sweep and one voxel on a
    /// backward one.
    ///
    /// **The ceiling is reached.** ADR-055 derives the mobility so that the
    /// response is "always the fastest of the stable ones, rather than quietly
    /// slower by oversight", which means the folded number is `1/theta_max`: a
    /// face whose overflow difference equals the declared limit carries the
    /// donor's whole pool. This is the direct analogue of
    /// `at_courant_one_the_flux_is_pure_donor_transfer` in `advect.rs`, and it is
    /// what a mobility understated by a factor of six fails — the sixth being the
    /// gap between the two conditions of SPEC section 4.2 that nobody has chosen
    /// between (see `TODO(courant-condition)`).
    #[test]
    fn pressure_signal_crosses_the_domain_at_one_voxel_per_tick() {
        // The ceiling holds, at a courant the host would actually fold. A uniform
        // world with one voxel disturbed, relaxed repeatedly, with the overflow
        // field recomputed every time off the state it is about to move.
        for mask in [TORUS, FLOORED] {
            let p = params(mask, 1.0 / 16.0);
            let centre = index(&p, 1, 2, 2);

            let mut src32 = narrow();
            let src64 = wide();
            for at in 0..N_VOXELS {
                put_32(&mut src32, at, 4096);
            }
            put_32(&mut src32, centre, 4096 + 2048);

            for n in 1..=3u32 {
                let lane = apply_32(&src32, &src64, &p);
                assert_reached_at_most(
                    &as_i64_32(&lane_of_32(&src32)),
                    &as_i64_32(&lane),
                    &ball(&p, centre, n),
                    n,
                    mask,
                );
                write_lane_32(&mut src32, &lane);
            }
        }

        // And at a courant far past anything a scenario may declare, where the
        // arithmetic is nonsense and the ceiling is not: the reach of one
        // application is a property of the stencil, and no amount of overflow
        // buys a second voxel of it.
        //
        // This half runs on the **wide** lane, and the reason is worth a line
        // rather than a shrug. Past the ceiling the amounts do not merely grow,
        // they grow doubly exponentially: the courant is itself proportional to
        // the difference of occupancies, so each application multiplies both the
        // pool and the speed. `i32` overflows before the second step at any
        // courant worth calling extreme, and the claim under test is about the
        // support of the change rather than about the arithmetic surviving.
        for mask in [TORUS, FLOORED] {
            let p = params(mask, 8.0);
            let centre = index(&p, 1, 2, 2);

            let src32 = narrow();
            let mut src64 = wide();
            for at in 0..N_VOXELS {
                put_64(&mut src64, at, 4096);
            }
            put_64(&mut src64, centre, 4096 + 2048);

            for n in 1..=3u32 {
                let lane = apply_64(&src32, &src64, &p);
                assert_reached_at_most(
                    &as_i64_64(&lane_of_64(&src64)),
                    &as_i64_64(&lane),
                    &ball(&p, centre, n),
                    n,
                    mask,
                );
                write_lane_64(&mut src64, &lane);
            }
        }

        // The ceiling is reached. `theta_max = 4` here, so the folded number is a
        // quarter, and a face across which the overflow differs by exactly the
        // declared limit has a courant number of exactly one.
        let p = params(TORUS, 0.25);
        let c = face_courant(Q::from_f64(3.0), Q::from_f64(-1.0), &p);
        assert_eq!(c, Q::ONE, "the derived mobility does not reach the ceiling");

        let pool = M32::new(1234);
        assert_eq!(
            flux_32(pool, M32::new(7), c),
            pool,
            "at the declared limit the face does not carry the whole donor pool"
        );
        // A power of two for the wide pool, and the reason belongs here rather
        // than in a fix later: above `2^24` an amount is not exactly
        // representable in `f32`, so "the whole pool" is exact only where the
        // pool itself is. ADR-068 prices that term of the error for transport;
        // this assertion is about the ceiling, not about `f32`, so it is stated
        // where the two do not interfere.
        let wide_pool = M64::new(1 << 32);
        assert_eq!(flux_64(wide_pool, M64::new(7), c), wide_pool);

        // And in the other direction, where the donor is the high side.
        let back = face_courant(Q::from_f64(-1.0), Q::from_f64(3.0), &p);
        assert_eq!(flux_32(M32::new(7), pool, back), -pool);
    }

    /// `ACCEPTANCE.md`, section "Conservation". A closed domain with a
    /// disturbance comes to a fixed point and stays there: the spread of the
    /// overflow field never grows from one application to the next, and after a
    /// finite number of applications nothing changes at all.
    ///
    /// **The name promises more than the mechanism has, and that has to be said
    /// here rather than discovered later.** Hydrostatics needs gravity, and there
    /// is no `g` in any formula of this file or of ADR-055: gravity enters only
    /// the settling velocity (ADR-067) and is not a key of the schema at all
    /// (`CONFIG_SCHEMA.md` section 13 item 23 exists so that nobody hard-codes
    /// 9.81 in silence). Either the name means the fixed point of a uniform
    /// occupancy, which is what is asserted below, or it needs step `f` standing
    /// next to it — and the corpus does not say which.
    ///
    /// This is also the one test in the file that can see a checkerboard: a state
    /// where two neighbours swap pools every tick, conserving matter exactly and
    /// leaving conservation, antisymmetry, the uniform fixed point and both axes
    /// of the ledger green. The folded number below is picked against the
    /// linearised condition written out in `TODO(courant-condition)` —
    /// `6*courant_per_overflow*(theta + 1) <= 1` — and not against either of the
    /// two conditions of SPEC section 4.2, because neither of them implies it and
    /// nobody has chosen between them.
    #[test]
    fn pressure_relaxation_reaches_hydrostatic_equilibrium() {
        // Occupancy one everywhere, one interior voxel half again as full. The
        // folded number is a sixteenth: `6*(1/16)*1.5 = 0.5625 <= 1`.
        let p = params(CLOSED, 1.0 / 16.0);
        let src64 = wide();
        let mut src32 = narrow();
        for at in 0..N_VOXELS {
            put_32(&mut src32, at, 1024);
        }
        put_32(&mut src32, index(&p, 1, 2, 2), 1024 + 512);

        let start = lane_of_32(&src32);
        let mut spread = spread_of(&overflow_field(&src32, &src64, &p));
        let mut settled = false;

        for step in 0..4096 {
            let next = apply_32(&src32, &src64, &p);
            assert_eq!(
                total_32(&next),
                total_32(&start),
                "step {step} of the chain lost matter"
            );

            let state = lane_of_32(&src32);
            write_lane_32(&mut src32, &next);
            let after = spread_of(&overflow_field(&src32, &src64, &p));
            assert!(
                after <= spread,
                "the spread of the overflow grew from {spread} to {after} at step \
                 {step}: the scheme is oscillating rather than relaxing"
            );
            spread = after;

            if next == state {
                settled = true;
                break;
            }
        }

        assert!(settled, "the column never came to a fixed point");
        // And it got there by moving, or the assertions above hold over a chain
        // that never started.
        assert_ne!(lane_of_32(&src32), start);
    }

    /// Obligatory for every new flux function (`.claude/rules/kernels.md`) — and
    /// here it asserts **two different properties** that must not be confused.
    ///
    /// The first is about [`face_courant`], and it is exact: swapping the two
    /// sides of a face negates the courant, because the courant is a difference.
    /// That is what makes a closed face carry nothing without a branch, and it is
    /// what conservation would rest on if the flux were diffusive.
    ///
    /// The second is that the diffusive statement is **false** for the flux and
    /// obliged to stay false. The donor is picked by the sign, so
    /// `flux(a, b, c) = round(c*a)` and `flux(b, a, c) = round(c*b)`. What holds
    /// instead is the mirror form `flux(l, h, c) == -flux(h, l, -c)`, the same one
    /// `settle.rs` has, and it is what a flux written as a function of the
    /// difference silently replaces with diffusion.
    #[test]
    fn flux_is_antisymmetric() {
        let p = params(TORUS, 1.0 / 4.0);
        let thetas = [-4.0f64, -1.5, -0.25, 0.0, 0.25, 1.5, 4.0, 1024.0];
        let pools = [-1_000_000i64, -1001, -3, -1, 0, 1, 3, 1001, 1_000_000];

        for a in thetas {
            for b in thetas {
                let forward = face_courant(Q::from_f64(a), Q::from_f64(b), &p);
                let backward = face_courant(Q::from_f64(b), Q::from_f64(a), &p);
                assert_eq!(
                    forward,
                    qsub(Q::ZERO, backward),
                    "the courant of a face read from the other end is not its \
                     negation ({a}, {b})"
                );
            }
        }

        // The mirror identity of the flux, bit for bit, in both widths.
        for a in thetas {
            for b in thetas {
                let c = face_courant(Q::from_f64(a), Q::from_f64(b), &p);
                let mirrored = face_courant(Q::from_f64(b), Q::from_f64(a), &p);
                for one in pools {
                    for other in pools {
                        let narrow_way = flux_32(M32::new(one as i32), M32::new(other as i32), c);
                        let back = flux_32(M32::new(other as i32), M32::new(one as i32), mirrored);
                        assert_eq!(
                            narrow_way.to_i64(),
                            -back.to_i64(),
                            "flux_32({one}, {other}) does not mirror at ({a}, {b})"
                        );

                        let wide_way = flux_64(M64::new(one), M64::new(other), c);
                        let wide_back = flux_64(M64::new(other), M64::new(one), mirrored);
                        assert_eq!(wide_way.to_i64(), -wide_back.to_i64());
                    }
                }
            }
        }

        // And the diffusive statement is false, which is the half of this test
        // that a "repair" would make true.
        let c = face_courant(Q::from_f64(1.0), Q::ZERO, &p);
        assert_ne!(
            flux_32(M32::new(1000), M32::new(4000), c).to_i64(),
            -flux_32(M32::new(4000), M32::new(1000), c).to_i64(),
            "the flux became a function of the difference: this is diffusion"
        );

        // The kernel applies exactly the canonical face numbers, with opposite
        // signs, and nothing else.
        for mask in [TORUS, FLOORED, CLOSED] {
            for folded in FOLDED {
                let p = params(mask, folded);
                let (src32, src64) = filled();
                let overflow = overflow_field(&src32, &src64, &p);
                let lane = lane_of_32(&src32);
                let dst = apply_32(&src32, &src64, &p);

                for idx in 0..N_VOXELS {
                    let mut expected = lane[idx as usize];
                    for face in 0..6u32 {
                        let n = neighbour(&p, idx, face);
                        let at_low = if n < idx { n } else { idx };
                        let at_high = if n < idx { idx } else { n };
                        let c =
                            face_courant(overflow[at_low as usize], overflow[at_high as usize], &p);
                        let carried = flux_32(lane[at_low as usize], lane[at_high as usize], c);
                        if idx == at_low {
                            expected -= carried;
                        } else {
                            expected += carried;
                        }
                    }
                    assert_eq!(
                        dst[idx as usize], expected,
                        "voxel {idx} did not apply the canonical face fluxes (mask \
                         {mask:#08b}, folded {folded})"
                    );
                }
            }
        }
    }

    /// The donor is the side the courant points **from**, and never the side
    /// holding more of the substance being moved.
    ///
    /// The only test in the file that can see the difference, and the file needs
    /// one: the fuller-pool rule conserves exactly, obeys the mirror identity,
    /// leaves a closed face carrying nothing and a uniform occupancy a fixed
    /// point, and closes both axes of the ledger. It takes the matter that leaves
    /// a voxel out of the **neighbour's** pool, which is the same shape of silence
    /// this file diagnoses for a table addressed by substance index: nothing is
    /// lost, the wrong thing is read.
    ///
    /// The two rules can only disagree where the fuller **voxel** and the fuller
    /// **pool** are different voxels — which is not an exotic corner but the case
    /// ADR-006 hangs the whole mechanism on. Occupancy is a sum over every
    /// occupying lane (ADR-067), so a voxel crowded by *another* substance pushes
    /// this one out while holding less of it than its neighbour does. Under the
    /// wrong rule that neighbour's pool is what gets moved, and the more crowded
    /// the voxel the wronger the amount.
    #[test]
    fn the_donor_is_the_side_the_courant_points_from() {
        let p = params(CLOSED, 1.0 / 16.0);

        // The face the whole argument is about, as two numbers: the low side is
        // the fuller voxel, the high side holds twice the pool of the substance
        // that moves. `theta_low = 4` against `theta_high = 1` gives a courant of
        // `3/16`, and the matter has to come out of the low side's 1024.
        let c = face_courant(Q::from_f64(4.0), Q::from_f64(1.0), &p);
        assert_eq!(c, Q::from_f64(3.0 / 16.0));
        assert_eq!(
            flux_32(M32::new(1024), M32::new(2048), c).to_i64(),
            192,
            "the face carried the acceptor's pool instead of the donor's"
        );
        assert_eq!(flux_64(M64::new(1024), M64::new(2048), c).to_i64(), 192);

        // Mirrored, where the donor is the high side and the low side is the one
        // holding twice as much.
        let back = face_courant(Q::from_f64(1.0), Q::from_f64(4.0), &p);
        assert_eq!(flux_32(M32::new(2048), M32::new(1024), back).to_i64(), -192);
        assert_eq!(flux_64(M64::new(2048), M64::new(1024), back).to_i64(), -192);

        // And the same statement about the kernel, on the state ADR-006 is
        // written about. Voxel `(0,0,0)` holds 1024 of the moving lane and 8192
        // of the other one, so its occupancy is `1 + 4 - 1 = 4`; its neighbour
        // `(1,0,0)` holds 2048 of the moving lane and nothing else, occupancy
        // `2 - 1 = 1`; the rest of the closed domain is empty at `-1`.
        let mut src32 = narrow();
        let mut src64 = wide();
        let crowded = index(&p, 0, 0, 0);
        let stocked = index(&p, 1, 0, 0);
        put_32(&mut src32, crowded, 1024);
        put_64(&mut src64, crowded, 8192);
        put_32(&mut src32, stocked, 2048);

        let field = overflow_field(&src32, &src64, &p);
        assert_eq!(field[crowded as usize], Q::from_f64(4.0));
        assert_eq!(field[stocked as usize], Q::from_f64(1.0));

        let before = lane_of_32(&src32);
        let after = apply_32(&src32, &src64, &p);

        // Three walls at the corner, and three open faces: one to `stocked` at a
        // courant of `3/16`, two to empty voxels at `5/16`. All three take from
        // the corner's own 1024: `1024 - 192 - 320 - 320`. Under the fuller-pool
        // rule the first of them would take `round(3/16 * 2048) = 384` out of a
        // pool of 1024 and leave the corner empty.
        assert_eq!(
            after[crowded as usize].to_i64(),
            192,
            "the crowded voxel gave up matter measured on its neighbour's pool"
        );

        // Its neighbour gains those 192 and loses `round(1/8 * 2048) = 256`
        // through each of its own three open faces, all of them to empty voxels.
        assert_eq!(after[stocked as usize].to_i64(), 2048 + 192 - 768);
        assert_eq!(total_32(&after), total_32(&before));

        // And the fixture the sweeps run on keeps the two rules apart. Without
        // this the tests above are the only ones that can fail on the donor, and
        // a later edit to `filled()` could take even the sweeps' chance away in
        // silence. The wrong rule is spelled out for the same reason the decoy
        // lanes are spelled out in
        // `the_overflow_reads_lanes_not_substance_indices`: a fixture that has
        // stopped distinguishing two rules is invisible otherwise.
        let p = params(TORUS, 1.0 / 16.0);
        let (src32, src64) = filled();
        let overflow = overflow_field(&src32, &src64, &p);
        let pools = lane_of_32(&src32);
        let mut disagreed = 0;
        for idx in 0..N_VOXELS {
            for face in 0..6u32 {
                let n = neighbour(&p, idx, face);
                let at_low = if n < idx { n } else { idx };
                let at_high = if n < idx { idx } else { n };
                let low = pools[at_low as usize];
                let high = pools[at_high as usize];
                let c = face_courant(overflow[at_low as usize], overflow[at_high as usize], &p);

                let fuller = if low.to_i64() >= high.to_i64() {
                    low
                } else {
                    high
                };
                let by_pool = q_round_32(qmul(c, q_conc_32(fuller, Q::ONE)));
                if flux_32(low, high, c) != by_pool {
                    disagreed += 1;
                }
            }
        }
        assert!(
            disagreed > 0,
            "no face of `filled()` tells the two donor rules apart: every sweep \
             over it is green under both"
        );
    }

    /// A state of equal **occupancy** — not of equal amounts: two substances with
    /// different coefficients give the same occupancy out of different numbers —
    /// does not move by a single unit.
    ///
    /// A scheme that lost a unit here would erode an equilibrium domain over a
    /// million ticks while looking like "pressure is a bit noisy".
    #[test]
    fn a_uniform_occupancy_is_a_fixed_point() {
        let mut src32 = narrow();
        let mut src64 = wide();
        for at in 0..N_VOXELS {
            let share = (at as i32 * 7) % 4;
            // Entry 0 contributes `1 + share` voxel volumes, entry 1 the
            // remaining `4 - share`. Different amounts, one occupancy.
            put_32(&mut src32, at, 1024 * (1 + share));
            put_64(&mut src64, at, 2048 * i64::from(4 - share));
        }

        for mask in [TORUS, FLOORED, CLOSED] {
            for folded in FOLDED {
                let p = params(mask, folded);

                for (at, value) in overflow_field(&src32, &src64, &p).iter().enumerate() {
                    assert_eq!(*value, Q::from_f64(4.0), "voxel {at} is not uniform");
                }

                assert_eq!(
                    apply_32(&src32, &src64, &p),
                    lane_of_32(&src32),
                    "mask {mask:#08b}, folded {folded}"
                );
                assert_eq!(
                    apply_64(&src32, &src64, &p),
                    lane_of_64(&src64),
                    "mask {mask:#08b}, folded {folded}, 64-bit"
                );
            }
        }
    }

    /// The corollary this scheme **restores**: a closed face carries nothing, and
    /// without a branch, because the velocity is itself a difference and the
    /// neighbour across a closed face is the voxel itself.
    ///
    /// True in diffusion, false in advection and in settling — both of which need
    /// a branch, and `settle.rs` says so — and true again here. The test pins the
    /// restoration rather than inheriting it: the moment a limiter is added
    /// (`TODO(limiter)`), the antidiffusive term stops being proportional to the
    /// difference and the wall starts to leak.
    #[test]
    fn a_closed_face_carries_nothing_at_any_overflow() {
        let p = params(CLOSED, 1.0);

        // The statement about the functions, at overflows far past anything a
        // scenario should reach.
        for theta in [0.0f64, 1.0, -1.0, 1024.0, -1024.0, 1.0e6] {
            let c = face_courant(Q::from_f64(theta), Q::from_f64(theta), &p);
            assert_eq!(c, Q::ZERO, "a face against itself has a courant at {theta}");
            for pool in [-1_000_000i64, -1, 0, 1, 1_000_000] {
                assert_eq!(flux_32(M32::new(pool as i32), M32::new(7), c), M32::ZERO);
                assert_eq!(flux_64(M64::new(pool), M64::new(7), c), M64::ZERO);
            }
        }

        // And the statement about the kernel: a corner voxel of a fully closed
        // grid, alone in the domain with an enormous overflow, gives up exactly
        // what its three open faces took and not one unit through a wall.
        let mut src32 = narrow();
        let src64 = wide();
        let corner = index(&p, 0, 0, 0);
        put_32(&mut src32, corner, 100_000);

        let overflow = overflow_field(&src32, &src64, &p);
        let lane = lane_of_32(&src32);
        let dst = apply_32(&src32, &src64, &p);

        let mut walls = 0;
        let mut expected = lane[corner as usize];
        for face in 0..6u32 {
            let n = neighbour(&p, corner, face);
            if n == corner {
                walls += 1;
                continue;
            }
            let at_low = if n < corner { n } else { corner };
            let at_high = if n < corner { corner } else { n };
            let c = face_courant(overflow[at_low as usize], overflow[at_high as usize], &p);
            let carried = flux_32(lane[at_low as usize], lane[at_high as usize], c);
            if corner == at_low {
                expected -= carried;
            } else {
                expected += carried;
            }
        }
        assert_eq!(walls, 3, "the corner of a closed grid has three walls");
        assert_eq!(dst[corner as usize], expected);
        assert_ne!(dst[corner as usize], lane[corner as usize]);
        assert_eq!(total_32(&dst), total_32(&lane));
    }

    /// The occupancy is addressed by lane, not by substance index (ADR-056).
    ///
    /// The only thing in the file that catches a table assembled out of substance
    /// indices, and it catches it only because the lanes hold **different**
    /// amounts: with equal amounts the error is invisible, the overflow field
    /// stays entirely plausible, and conservation is not touched at all.
    #[test]
    fn the_overflow_reads_lanes_not_substance_indices() {
        let p = params(TORUS, 1.0 / 16.0);
        let (src32, src64) = filled();
        let field = overflow_field(&src32, &src64, &p);

        for at in 0..N_VOXELS {
            let narrow_amount = src32[(LANE_32 * N_VOXELS + at) as usize].to_i64() as f64;
            let wide_amount = src64[(LANE_64 * N_VOXELS + at) as usize].to_i64() as f64;
            let by_hand = COEFF_32 * narrow_amount + COEFF_64 * wide_amount - 1.0;
            assert_eq!(field[at as usize], Q::from_f64(by_hand), "voxel {at}");

            // And the decoy lanes would have given something else, or the
            // equality above says nothing about the address.
            let decoy = COEFF_32 * src32[at as usize].to_i64() as f64
                + COEFF_64 * src64[(N_VOXELS + at) as usize].to_i64() as f64
                - 1.0;
            assert_ne!(field[at as usize], Q::from_f64(decoy), "voxel {at}");
        }
    }

    /// A negative partial molar volume lowers the occupancy, the occupancy goes
    /// below zero legitimately, and the kernel neither refuses nor clamps
    /// (ADR-067: electrostriction is real, `PO4^3-` and `Fe^2+` have negative
    /// `V_bar`, and `V_occ` is unbounded below).
    ///
    /// The test exists because "check that the volume is positive" is the most
    /// natural edit the next reader will make, and it would silently reverse half
    /// of ADR-067.
    #[test]
    fn a_negative_partial_molar_volume_lowers_the_occupancy() {
        let p = params(TORUS, 1.0 / 16.0);
        let mut src32 = narrow();
        let mut src64 = wide();
        for at in 0..N_VOXELS {
            put_32(&mut src32, at, 1024);
            put_64(&mut src64, at, 2048 * 4);
        }

        let mut dst = vec![Q::ZERO; N_VOXELS as usize];
        let positive = [Q::from_f64(COEFF_32), Q::from_f64(COEFF_64)];
        let negative = [Q::from_f64(COEFF_32), Q::from_f64(-COEFF_64)];

        let occ = Occupants {
            lane: &LANES,
            coeff: &positive,
        };
        for idx in 0..N_VOXELS {
            overflow_voxel(&src32, &src64, &mut dst, &occ, &p, idx);
        }
        let with_positive = dst.clone();

        let occ = Occupants {
            lane: &LANES,
            coeff: &negative,
        };
        for idx in 0..N_VOXELS {
            overflow_voxel(&src32, &src64, &mut dst, &occ, &p, idx);
        }

        for at in 0..N_VOXELS as usize {
            assert!(
                dst[at] < with_positive[at],
                "voxel {at} did not lose occupancy to electrostriction"
            );
            // `1 - 4 - 1 = -4`: below zero, and nothing rounded it back up.
            assert_eq!(dst[at], Q::from_f64(-4.0), "voxel {at}");
            assert!(dst[at] < Q::ZERO);
        }
    }

    /// Gather form buys this: nothing a voxel writes is visible to any other in
    /// the same application. On the GPU there is no traversal order at all, so
    /// anything this test could catch is unfixable there.
    ///
    /// It is also what catches the two kernels being merged into one — the
    /// neighbour's overflow computed on the fly out of `dst` — which on a forward
    /// CPU sweep gives the very same numbers.
    #[test]
    fn the_result_does_not_depend_on_the_traversal_order() {
        let p = params(TORUS, 1.0 / 6.0);
        let (src32, src64) = filled();
        let coeff = coeffs();
        let occ = Occupants {
            lane: &LANES,
            coeff: &coeff,
        };

        let mut forwards = vec![Q::ZERO; N_VOXELS as usize];
        for idx in 0..N_VOXELS {
            overflow_voxel(&src32, &src64, &mut forwards, &occ, &p, idx);
        }
        let mut backwards = vec![Q::ZERO; N_VOXELS as usize];
        for idx in (0..N_VOXELS).rev() {
            overflow_voxel(&src32, &src64, &mut backwards, &occ, &p, idx);
        }
        assert_eq!(forwards, backwards);

        let lane = lane_of_32(&src32);
        let mut ahead = vec![M32::ZERO; N_VOXELS as usize];
        for idx in 0..N_VOXELS {
            relax_voxel_32(&lane, &forwards, &mut ahead, &p, idx);
        }
        let mut behind = vec![M32::ZERO; N_VOXELS as usize];
        for idx in (0..N_VOXELS).rev() {
            relax_voxel_32(&lane, &forwards, &mut behind, &p, idx);
        }
        assert_eq!(ahead, behind);
        assert_ne!(ahead, lane);
    }

    /// The kernel carries its own copy of the neighbourhood lookup, because
    /// `kernels/` may depend on `numeric/` and on nothing else. This is the fourth
    /// copy in the tree — `diffuse.rs` and `settle.rs` keep the same test for the
    /// same reason — and drift between them is caught only here.
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
            let p = params(mask, 1.0 / 16.0);
            assert_eq!(grid.n_voxels(), N_VOXELS);

            for idx in 0..N_VOXELS {
                let (x, y, z) = grid.coords(idx);
                assert_eq!(index(&p, x, y, z), grid.index(x, y, z));
                for face in Face::ALL {
                    assert_eq!(
                        neighbour(&p, idx, face as u32),
                        grid.neighbour(idx, face),
                        "voxel {idx}, face {face:?}, mask {mask:#08b}"
                    );
                }
            }
        }
    }

    /// The two widths come out of one macro so that they cannot drift; this says
    /// they have not. Drift is silent and surfaces as "water somehow has different
    /// dynamics" (ADR-040, `numeric/m.rs`).
    #[test]
    fn both_widths_are_one_text() {
        let p = params(TORUS, 1.0 / 4.0);
        for a in [-4.0f64, -0.25, 0.0, 0.25, 4.0] {
            let c = face_courant(Q::from_f64(a), Q::ZERO, &p);
            for one in [-4_000i64, -37, -1, 0, 1, 37, 4_000, 1_000_003] {
                for other in [-4_000i64, -1, 0, 1, 4_000] {
                    assert_eq!(
                        flux_32(M32::new(one as i32), M32::new(other as i32), c).to_i64(),
                        flux_64(M64::new(one), M64::new(other), c).to_i64(),
                        "pools {one}, {other} at overflow {a}"
                    );
                }
            }
        }

        // And the whole kernel over one state: the same amounts in the narrow
        // lane and in the wide one, relaxed against the same overflow field.
        let mut src32 = narrow();
        let mut src64 = wide();
        for at in 0..N_VOXELS {
            let amount = 1024 * (1 + (at as i32 * 7) % 3);
            put_32(&mut src32, at, amount);
            put_64(&mut src64, at, i64::from(amount));
        }
        for mask in [TORUS, FLOORED, CLOSED] {
            for folded in FOLDED {
                let p = params(mask, folded);
                let narrow_side = apply_32(&src32, &src64, &p);
                let wide_side = apply_64(&src32, &src64, &p);
                for at in 0..N_VOXELS as usize {
                    assert_eq!(
                        narrow_side[at].to_i64(),
                        wide_side[at].to_i64(),
                        "voxel {at}, mask {mask:#08b}, folded {folded}"
                    );
                }
            }
        }
    }
}
