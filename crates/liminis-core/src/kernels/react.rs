//! All reactions of one voxel: flat tables, one snapshot, one shared
//! competition coefficient.
//!
//! The second skeleton of `ARCHITECTURE.md` made to compile. Diffusion showed
//! the form on the easiest possible kernel — one substance, one scalar, a fixed
//! stencil — and everything that is not obvious about the form is only visible
//! here: every substance of the voxel at once, a stoichiometry vector of
//! variable length, a variable number of reactions, a coefficient that couples
//! them, and all of it inside the same signature, without an allocation and
//! without a trait (ADR-041).
//!
//! # Three things are load-bearing, and all three are easy to lose
//!
//! **One snapshot for all reactions.** Demand is computed off state `N` and the
//! whole of it is applied into state `N+1`. Recomputing availability after each
//! reaction is cheaper and looks natural, and it makes the order of the rows in
//! TOML part of the semantics of the world — invisibly, because the balance goes
//! on closing and what changes is who ate the substrate first (ADR-041,
//! rejected).
//!
//! **Deltas accumulate locally and are written once.** `dst[at] += ...` inside
//! the loop over reactions is a read from the buffer being written. On the CPU
//! and inside one dispatch it gives the same numbers; it breaks on the day the
//! pass is split in two, and neither the compiler nor a grep sees it.
//!
//! **`xi_max` is computed over the inputs only.** The ceiling on an output is a
//! guarantee of the validator through `max_conc` (ADR-039), not a clamp here: a
//! product past its declared limit means a wrong declaration, and the run has to
//! fail loudly rather than saturate quietly (ADR-041).
//!
//! # What conservation rests on
//!
//! Nothing in this file. Changes in amount are `nu_i * xi` with integer `nu_i`,
//! so the balance is a property of the stoichiometric vector and not of the
//! extent (ADR-027). That is what makes rounding, capping and competition
//! scaling all safe to apply to `xi` — and it is also why every mistake this
//! kernel can make is silent: a wrong lane, a sequential application, a
//! coefficient applied to the wrong set all conserve every element exactly. The
//! acceptance tests in `tests/acceptance_reactions.rs` are the whole of the
//! defence, and they are named in `ACCEPTANCE.md` for that reason.
//!
//! # What is not here
//!
//! `D`, `dx`, `units_per_mol`, `T_ref`, the config, the cell table. The host
//! folds `volume` and `dt` and hands over `conc_per_unit` per substance; the
//! reference temperature of the Q10 factor arrives per reaction as `t_vmax`
//! (ADR-048) and the scenario's zero of enthalpy storage does not arrive at all,
//! which is the only structural defence against confusing the two. Cells are
//! invisible: their contribution to catalysis arrives as a field of
//! concentration, like a guild's (ADR-050).
//!
//! The `requires` gate of SPEC section 5 is not here either, and that is not an
//! omission by inattention. `schema::Requirement` is parsed and validated, but no
//! record says whether the gate applies in this kernel, which fields it reads, or
//! how it behaves at the edge of a window — a hard cut-off or a factor — and the
//! tables of ADR-041 do not carry it at all. Implementing it would be deciding
//! all three in code.

use crate::numeric::{M32, M64, Q, q_conc_64, q_round_64, qadd, qdiv, qmul, qpow, qsub, rand, xi};

/// How many substances the local arrays of this kernel are sized for.
///
/// A build-time bound, and the honest price of the ban on allocation: the demand
/// vector lives on the stack, so its size has to be named somewhere (ADR-041).
/// Thirty-two is also what makes `width_mask` a `u32` — the two numbers are one
/// decision, not two. The validator refuses a config that exceeds it, naming
/// both quantities, and "the registry is data" (ADR-018) means "data within a
/// declared bound" after that.
///
/// This is the home ADR-041 and `ARCHITECTURE.md` give it. It lived in
/// `world/registry.rs` until this file existed, because `kernels/` may not
/// depend on `world/`; the dependency runs the other way now, and the number is
/// spelled once.
pub const S_MAX: usize = 32;

/// How many reactions the local `xi[R_MAX]` is sized for (ADR-041).
pub const R_MAX: usize = 64;

/// `cat[r]` of a reaction with no catalyst at all.
///
/// The fork of ADR-063 costs one column and one branch. At the sentinel `vmax`
/// is in turnovers per second per cubic metre and there is **no** catalyst
/// factor in the kinetics — not a factor of one, and not a catalyst at zero
/// concentration. The second of those is the accident ADR-063 spells out: the
/// whole of the S0 chemistry stands still, both halves of the invariant close on
/// `0 == 0`, and nothing fails.
pub const NO_CATALYST: u32 = u32::MAX;

/// The flat read-only reaction tables. In WGSL each field is one
/// `var<storage, read>`.
///
/// A named set of bindings, not an interface: no trait, no `dyn`, no behaviour
/// inside. "Variable length" stops being a problem because the length lives in a
/// neighbouring array rather than in the type (ADR-041).
///
/// Two indexing conventions meet in here and they are not interchangeable.
/// `nu`, `nu_sub` and `km` are indexed by **entry**, `lane`, `conc_per_unit` by
/// **substance**, and the amount buffers by **lane**. Confusing `conc_per_unit`
/// with a lane-indexed table shifts a concentration by `2^(k_i - k_j)` — tens of
/// binary orders in the rate — with the balance closing exactly, because a rate
/// enters no invariant.
pub struct Rx<'a> {
    /// Storage coefficients, all reactions end to end: `nu_i = s_i * 2^(k_i -
    /// e_r)`, signed, one entry per participant (ADR-039). Net per substance: a
    /// substance standing on both sides of a reaction is folded into a single
    /// entry by the loader, and two entries would be applied twice.
    pub nu: &'a [i32],
    /// Which substance `nu[j]` belongs to. The reserved index `s_energy` appears
    /// here like any other participant (ADR-041).
    pub nu_sub: &'a [u32],
    /// Where reaction `r` starts in `nu`.
    pub begin: &'a [u32],
    /// How many entries reaction `r` has.
    pub len: &'a [u32],
    /// The extent exponent of reaction `r`, derived at load from its scarcest
    /// participant (ADR-039). The kernel does not compute it.
    pub e_r: &'a [u32],
    /// `lane_of[s]`: which lane of its width class substance `s` occupies
    /// (ADR-056). The sixth table, and the reason it is here rather than in
    /// [`ReactParams`] is that `Params` holds scalars only — in WGSL it is a
    /// uniform buffer.
    pub lane: &'a [u32],
    /// The catalysis column of reaction `r`, or [`NO_CATALYST`] (ADR-063).
    pub cat: &'a [u32],
    /// The identifier of reaction `r`, folded on the host from the reaction's
    /// **name** and never from its position in the file (ADR-027). It is the
    /// third counter of every draw this kernel takes, so an identifier taken
    /// from a row index makes reordering two lines of TOML a different run at
    /// unchanged semantics.
    ///
    /// The mixer is `numeric::name_key`, a round of the same `mix` per UTF-8
    /// byte, and it stays on the host by necessity and not by taste: WGSL has
    /// no strings, and the kernel form forbids a loop over host data of unknown
    /// length (ADR-015, ADR-090). What crosses the boundary is this `u32`
    /// column. Which counter of `rand` a future kernel may take for itself is
    /// still open — there is no registry of `purpose` — so the validator can
    /// only keep `rid` out of the windows that are declared as constants today
    /// (`NOISE_BASE`, `WORLDGEN_BASE`), and those two bases are themselves
    /// still `TODO`.
    pub rid: &'a [u32],
    /// Turnovers per second per cubic metre at `t_vmax`, or per mole of catalyst
    /// when `cat[r]` is not the sentinel (ADR-063).
    pub vmax: &'a [Q],
    /// The Q10 factor of reaction `r`: how much faster it runs ten kelvin above
    /// its own reference temperature.
    pub q10: &'a [Q],
    /// The reference temperature of the Q10 factor, in kelvin, per reaction
    /// (ADR-048). **Not** `T_ref`: that is an arbitrary scenario zero of
    /// enthalpy storage, and tying kinetics to it would make the speed of the
    /// whole chemistry a function of where the storage zero was put.
    pub t_vmax: &'a [Q],
    /// Half-saturation constants in mol/m^3, parallel to `nu`, one entry per
    /// participant and meaningful only where `nu < 0` (SPEC section 5).
    pub km: &'a [Q],
    /// What one storage unit of substance `s` is worth as a concentration:
    /// `1 / (units_per_mol * V_voxel)`. Indexed by **substance**, not by lane.
    pub conc_per_unit: &'a [Q],
}

/// Parameters of one application. Scalars only: in WGSL this is a uniform
/// buffer, and every quantity in it is a place where a scale can drift apart
/// from the one the host meant.
#[derive(Clone, Copy, Debug)]
pub struct ReactParams {
    /// Voxels along X of the **fine** grid.
    pub nx: u32,
    /// Voxels along Y of the fine grid.
    pub ny: u32,
    /// How many voxels the fine grid holds; the stride between lanes.
    pub n_voxels: u32,
    /// The stride of an amount lane. On a `world::Field` it is `n_voxels + 1`
    /// — the voxels and the ghost cell after them (ADR-059) — and the host is
    /// the only place that knows so; this kernel is handed the number.
    ///
    /// Separate from [`ReactParams::n_voxels`] because the two mean different things
    /// and only one of them is an address. A dispatch runs over `0..n_voxels`;
    /// an amount lives at `lane[s] * lane_len + idx`. Using the voxel count as
    /// the stride reads one lane short of where the substance is, and on the
    /// registry the project carries that is **right** for the first lane of a
    /// width class and wrong by one element per lane after it — which is the
    /// same silent shape ADR-056 warns about for `s * n_voxels + idx`.
    pub lane_len: u32,
    /// How many substances the registry holds. The loop bound of the write-back,
    /// and never [`S_MAX`]: the lane table is exactly this long (ADR-056).
    pub n_substances: u32,
    /// How many reactions the scenario declares.
    pub n_reactions: u32,
    /// The tick, second counter of every draw (ADR-027, ADR-058).
    pub tick: u32,
    /// The run seed folded to 32 bits on the host, fourth counter of every draw
    /// (ADR-058). Without it the chemistry of two seeds is bit-identical,
    /// because the stochastic rounding of extent is the only conversion from `Q`
    /// to an integer in the whole of it.
    ///
    /// Two `u32` counters side by side, and the compiler cannot tell them apart:
    /// swapping them at a call site produces a stream that is uniform,
    /// unbiased, passes every test in `numeric/rng.rs` and is incomparable with
    /// the reference run. Only the anchor test sees that.
    pub run_key: u32,
    /// Bit `s` set: substance `s` is stored 64-bit (ADR-040). The branch is on
    /// the substance index, the same for every voxel, so on the GPU it is a
    /// uniform branch rather than a diverging warp.
    pub width_mask: u32,
    /// The index of enthalpy in the stoichiometry vector (ADR-041).
    ///
    /// It arrives from the host and is not named by any record; what *is* fixed
    /// is that it must lie past the last substance, which is what
    /// `world::Registry::MAX_SUBSTANCES` is one below [`S_MAX`] for. An index
    /// inside the substance range lands the enthalpy delta in the lane of the
    /// last substance: that substance is quietly created and destroyed at the
    /// pace of the reactions while the matter ledger closes, because matter and
    /// energy are counted apart (ADR-028).
    pub s_energy: u32,
    /// How many binary orders coarser the temperature field is (SPEC section
    /// 1.5). Temperature is a quantity of the coarse cell: sixty-four fine
    /// voxels share one `T`, and the Q10 factor reads the covering cell
    /// (ADR-062).
    pub lod: u32,
    /// Coarse cells along X.
    pub cnx: u32,
    /// Coarse cells along Y.
    pub cny: u32,
    /// The voxel volume in m^3, folded on the host.
    ///
    /// `V_voxel`, not its reciprocal. From `dn_i = s_i * rate * dt * V` and
    /// `dn_i = s_i * 2^-e_r * xi` follows `xi = rate * dt * V * 2^e_r`; with
    /// `1/V` here the extent would move by `V^2`, eighteen orders in the eco
    /// regime, and no conservation test would see it, since `d_i = nu_i * xi` is
    /// exact at any `xi` (ADR-047, `numeric/convert.rs`).
    pub volume: Q,
    /// The timestep in seconds, folded on the host.
    pub dt: Q,
}

/// All reactions of one voxel: one snapshot in, one write out.
///
/// Reads `src32`/`src64`, writes its own cell of `dst32`/`dst64`, of
/// `energy_delta` and of `xi_out`, and nothing else. It touches no neighbour, no
/// channel counter and no atomic: a reaction is local, and the sums for the
/// ledger are taken by reduction in the LEDGER phase (ADR-041, ADR-080).
///
/// The three passes are the three passes of SPEC section 5 — demand, then the
/// coefficient, then application — and the order between them is the whole
/// point. Every substance is written back, including the ones whose delta came
/// out zero: the write buffer holds state `N+1` in full, and a skipped lane is
/// left not "unchanged" but one tick stale (ADR-057).
///
/// # The extent slice, and what it costs
///
/// `xi_out` is the report the matter half of the ledger closes against
/// (ADR-080): `Delta n_s == Sum_c credited(c, s) + Sum_r nu_(r,s) * Xi_r`, with
/// `Xi_r` reduced on the host from this slice. The extent is a local of the third
/// pass and there is no other way out of a kernel — ADR-034 fixes a free function
/// with no return value, and a WGSL compute entry point returns nothing at all,
/// so "hand the host a number" is this same slice with two host implementations
/// instead of one (ADR-075, rejected).
///
/// **`4 * R` bytes per voxel**, `R` being the scenario's reaction count and never
/// [`R_MAX`]. Against the 230 B per voxel of ADR-062: 234 B and 490 MB at 128
/// cubed for `R = 1`, 1.74% of the state; 270 B and 566 MB at the ten reactions
/// SPEC section 2.3 is heading for, 17.4%; and at `R_MAX = 64` it is 256 B per
/// voxel — 486 B and 1 019 MB, **111%** of the whole state, at which point the
/// debug mechanism outweighs the world. The cliff is real and the length of this
/// slice is the only thing holding it off.
#[allow(clippy::too_many_arguments)]
// Twelve arguments against a clippy threshold of seven. The signature is fixed
// by ADR-034 and every slice here is one binding in WGSL, so folding them into a
// struct would buy a lint and cost the shape the port depends on.
#[allow(clippy::needless_range_loop)]
// And the three loops over the reactions index `want` by the loop variable,
// which clippy would rather see as `want.iter().enumerate()`. An iterator is
// either a closure or a type with behaviour, and both are forbidden inside a
// kernel (ADR-015); `.take(n_reactions)` on a `[i32; R_MAX]` is also not the
// same statement — the array is the build-time bound and the loop bound is the
// scenario's count. `kernels/fold.rs` writes its triple loop by axis for the
// same reason and says so.
pub fn react_voxel(
    src32: &[M32],
    src64: &[M64],
    dst32: &mut [M32],
    dst64: &mut [M64],
    energy_delta: &mut [M64],
    xi_out: &mut [M32],
    temperature: &[Q],
    catalyst: &[Q],
    rx: &Rx,
    p: &ReactParams,
    idx: u32,
) {
    debug_assert!(idx < p.n_voxels, "voxel {idx} is past the grid");
    debug_assert!(
        p.n_substances as usize <= S_MAX && p.n_reactions as usize <= R_MAX,
        "the validator must refuse a config over S_MAX = {S_MAX} or R_MAX = {R_MAX} (ADR-041)"
    );
    debug_assert!(
        p.s_energy >= p.n_substances && (p.s_energy as usize) < S_MAX,
        "s_energy = {} overlaps the substance range: the enthalpy delta would \
         land in the lane of a substance, and the matter ledger would still \
         close (ADR-028, ADR-041)",
        p.s_energy
    );
    // One block of `n_reactions` cells per voxel, and the block of the **last**
    // voxel has to fit. A slice sized `n_voxels` — the shape of every other
    // per-voxel buffer in this signature, and so the shape a host reaches for —
    // is long enough for voxel zero of a one-reaction scenario and for nothing
    // else, and the reduction would read the extent of one voxel as the extent
    // of another (ADR-080).
    debug_assert!(
        xi_out.len() >= (p.n_voxels as usize) * (p.n_reactions as usize),
        "the extent slice holds {} cells against {} voxels times {} reactions",
        xi_out.len(),
        p.n_voxels,
        p.n_reactions
    );

    // 1. Demand. Every reaction works out how many quanta of extent it would
    //    like, off one and the same snapshot `N`, and is capped by its inputs
    //    right away. The order of the reactions does not matter here, and that
    //    is the goal.
    let mut want = [0i32; R_MAX];
    for r in 0..p.n_reactions as usize {
        let rate = rate_of(src32, src64, temperature, catalyst, rx, p, idx, r);
        let e_r = rx.e_r[r];
        // `xi` takes the exponent as a `u8` while the table is `u32` (ADR-041).
        // A junk value would narrow silently and move the quantum of extent by a
        // power of two.
        debug_assert!(
            e_r <= 127,
            "extent exponent {e_r} of reaction {r} is outside the range a scale \
             derivation can produce (ADR-039)"
        );
        // The canonical counter order is (voxel, tick, reaction, run key), and
        // the counters do not commute: the skeleton in `ARCHITECTURE.md` writes
        // the wrapper in the opposite order, and a stream produced that way is
        // uniform, unbiased, passes every test in `numeric/rng.rs` and is
        // incomparable with the reference run (ADR-058).
        let draw = rand(idx, p.tick, rx.rid[r], p.run_key);
        let raw = xi(rate, p.dt, p.volume, e_r as u8, draw);
        want[r] = extent_cap(src32, src64, rx, p, idx, r, raw);
    }

    // 2. The coefficient. Demand is summed per substance; if any substance is
    //    oversold, the shared multiplier is the minimum over the oversold ones.
    //    Scaling `xi` is safe: conservation is a property of the vector `nu`, not
    //    of the extent (ADR-027).
    let mut demand = [0i64; S_MAX];
    for r in 0..p.n_reactions as usize {
        accumulate_demand(&mut demand, rx, p, r, want[r]);
    }
    let scale = competition_scale(&demand, src32, src64, rx, p, idx);

    // 3. Application. Deltas accumulate locally and are written once — otherwise
    //    the kernel would be reading from the buffer it writes to.
    let mut delta = [0i64; S_MAX];
    for r in 0..p.n_reactions as usize {
        let extent = scale_extent(want[r], scale);
        // The report to the ledger, written **before** the early exit and
        // written on every reaction, including the ones that did nothing
        // (ADR-080). A write and not an addition, for the reason ADR-045 gives
        // about `energy_delta`: the slice lives inside one tick and there is no
        // clearing pass, so a `+=` here would let a reduction credit last tick's
        // extent a second time — and a stale quantum is indistinguishable from a
        // fresh one. `the_extent_slice_is_overwritten_not_accumulated` is the
        // guard, and it is the same failure `the_energy_delta_is_written_not_added`
        // guards one field over.
        //
        // Voxel-major, `idx * R + r`: this voxel's own block of `R` cells and no
        // other, so the rule of ADR-034 holds unchanged — a voxel writes only
        // where it lives.
        xi_out[(idx as usize) * (p.n_reactions as usize) + r] = M32::new(extent);
        if extent == 0 {
            continue;
        }
        for j in rx.begin[r]..rx.begin[r] + rx.len[r] {
            let s = rx.nu_sub[j as usize] as usize;
            // Exact: `nu` is integer and was rounded once, at load (ADR-041).
            delta[s] += i64::from(rx.nu[j as usize]) * i64::from(extent);
        }
    }
    for s in 0..p.n_substances {
        let after = amount_get(src32, src64, rx, p, idx, s) + delta[s as usize];
        amount_store(dst32, dst64, rx, p, idx, s, after);
    }

    // Enthalpy is a participant of `nu` like any substance (ADR-041), but it
    // lives on 32^3 while the reactions run on 128^3 (SPEC section 1.5). So the
    // increment goes here, onto the fine grid, into this cell and no other, and a
    // separate kernel folds it (ADR-045, `kernels/fold.rs`). A write and not an
    // addition: the field lives inside one tick, which is why no zeroing pass
    // exists — restore three reaction steps and `+=` would be green here, and
    // only `the_energy_delta_is_written_not_added` would notice.
    energy_delta[idx as usize] = M64::new(delta[p.s_energy as usize]);
}

/// The amount of substance `s` in this voxel, widened.
///
/// One of the two places where mixed-width storage is visible. The branch is on
/// the substance index, identical for every voxel, so on the GPU it is a uniform
/// jump (ADR-040, ADR-041).
///
/// The address goes through `lane[s]` and never through `s` itself. `lane == s`
/// stopped being true anywhere with ADR-056, and that is a rule rather than an
/// observation, because breaking it is silent: a kernel addressing by substance
/// index reads a lane belonging to some other substance, the ledger closes
/// because nothing was lost, and on the registry the project carries water
/// stands first and takes lane 0 — so the wrong expression is *right* at
/// `s == 0` and off by one after it.
#[inline(always)]
fn amount_get(src32: &[M32], src64: &[M64], rx: &Rx, p: &ReactParams, idx: u32, s: u32) -> i64 {
    let at = (rx.lane[s as usize] * p.lane_len + idx) as usize;
    if p.width_mask & (1 << s) != 0 {
        src64[at].to_i64()
    } else {
        src32[at].to_i64()
    }
}

/// Write the new amount of substance `s` back into state `N+1`.
///
/// The narrowing is the second half of the width branch and it goes through
/// `from_i64_clamping`, not through `as i32`. An amount out of range means the
/// width derivation of ADR-040 was fed a wrong `max_conc`; the assertion says so
/// in debug, and in release the clamp keeps a full pool from becoming a negative
/// one, which is precisely what a bare cast would do.
#[inline(always)]
fn amount_store(
    dst32: &mut [M32],
    dst64: &mut [M64],
    rx: &Rx,
    p: &ReactParams,
    idx: u32,
    s: u32,
    value: i64,
) {
    let at = (rx.lane[s as usize] * p.lane_len + idx) as usize;
    if p.width_mask & (1 << s) != 0 {
        dst64[at] = M64::from_i64_clamping(value);
    } else {
        dst32[at] = M32::from_i64_clamping(value);
    }
}

/// The rate of reaction `r` in this voxel: turnovers per second per cubic metre.
///
/// ```text
/// rate = vmax * q10^((T - t_vmax)/10) * min over inputs S/(km + S) * [catalyst]
/// ```
///
/// Three things about the shape of it are decisions rather than transcription.
///
/// **The reference temperature is `t_vmax`, the reaction's own** (ADR-048).
/// `T_ref` — the scenario's zero of enthalpy storage — reaches no parameter of
/// this kernel, and that absence is the only defence there is: substituting it
/// would make the speed of the whole chemistry a function of an arbitrary
/// storage zero, and neither ledger would see it. ADR-048 calls that "not a
/// saving of one key but a mistake".
///
/// **The catalyst enters as a concentration, and at the sentinel there is no
/// factor at all** (ADR-047, ADR-063). Not a one, not a zero: a phantom unit
/// catalyst gives the same bits today and is unfalsifiable in principle, and a
/// zero stops the S0 chemistry with both halves of the invariant closing on
/// `0 == 0`.
///
/// **The Michaelis minimum is taken over the same entries the input cap is taken
/// over**: those with `nu < 0`, enthalpy excluded. `nu` is net and signed, while
/// `km` is declared per input in TOML; if the two sets ever come apart, the rate
/// and the ceiling stop describing the same reaction, silently.
///
/// One caveat that belongs next to the formula rather than after the first
/// divergence: `qpow` is a transcendental, IEEE-754 requires nothing of `pow`,
/// and vendors differ in the last bits (ADR-022, NUMERIC.md section 2). The
/// result feeds the stochastic rounding of extent, where one ulp flips a whole
/// quantum. Reproducibility from the seed is untouched — the draw is a pure
/// function of its counters — but reproducibility across platforms is not, and
/// `version.rs` already says the same about `qexp` in the light kernel.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn rate_of(
    src32: &[M32],
    src64: &[M64],
    temperature: &[Q],
    catalyst: &[Q],
    rx: &Rx,
    p: &ReactParams,
    idx: u32,
    r: usize,
) -> Q {
    // Ten kelvin is the definition of Q10, not a folded parameter: in WGSL this
    // is the literal `10.0` and there is nothing for a host to get wrong about
    // it. What is folded is `t_vmax`, and it arrives per reaction.
    let per_decade = Q::from_f64(10.0);
    let above_reference = qsub(temperature[coarse_of(p, idx) as usize], rx.t_vmax[r]);
    let mut rate = qmul(
        rx.vmax[r],
        qpow(rx.q10[r], qdiv(above_reference, per_decade)),
    );

    let mut limiting = Q::ONE;
    for j in rx.begin[r]..rx.begin[r] + rx.len[r] {
        let s = rx.nu_sub[j as usize];
        // The enthalpy record is not a substrate. In an **endothermic** reaction
        // `nu_E < 0`, so it looks exactly like an input from in here: the rate
        // would be limited by a pool that does not exist, and `amount_get` would
        // index the lane table past its end (ADR-041).
        //
        // Which sign this guard saves changed with ADR-081 and the guard did not.
        // `nu_E` is now stated in the direction of the field — `-Sum_s nu_s*w_s`
        // — so an *exothermic* record is positive and `nu >= 0` short-circuits it
        // on every scenario the repository carries. That makes this line look
        // dead: delete it and the whole corpus stays green, and the failure
        // arrives on the first endothermic reaction, as an index into
        // `lane[s_energy]` — a panic in debug, and in WGSL a read the
        // specification allows to land on another binding. The one thing standing
        // here is `the_energy_record_is_not_a_substrate` below, which keeps a
        // negative `nu_E` on purpose.
        if s == p.s_energy || rx.nu[j as usize] >= 0 {
            continue;
        }
        let amount = amount_get(src32, src64, rx, p, idx, s);
        // A pool the transport of ADR-068 left negative contributes no substrate
        // rather than a negative concentration.
        let pool = if amount > 0 { amount } else { 0 };
        let conc = q_conc_64(M64::new(pool), rx.conc_per_unit[s as usize]);
        let saturation = qadd(rx.km[j as usize], conc);
        // Both zero means an empty pool at zero half-saturation, and the term is
        // zero rather than a division by zero.
        let term = if saturation > Q::ZERO {
            qdiv(conc, saturation)
        } else {
            Q::ZERO
        };
        if term < limiting {
            limiting = term;
        }
    }
    rate = qmul(rate, limiting);

    if rx.cat[r] != NO_CATALYST {
        // The catalysis columns are laid out like the amount lanes: one plane of
        // the fine grid per column.
        //
        // TODO(catalyst-class): that this buffer is already a concentration is a
        // choice, not a quotation. ADR-050 says "fields of concentration", which
        // reads as `Q`; ADR-063 says the multiplier is "a concentration through
        // `q_conc`", which reads as an `M` plus a `conc_per_unit`. One of the two
        // crossings has to happen somewhere and no record says where, so the
        // fold sits on the host here. The numbering of the columns —
        // `guild:` against `expr:` — and the table that resolves it are undecided
        // in the same way.
        rate = qmul(rate, catalyst[(rx.cat[r] * p.lane_len + idx) as usize]);
    }
    rate
}

/// The demand of reaction `r`, capped by what its inputs can supply.
///
/// `xi_max = min over inputs floor(amount_i / |nu_i|)`, then
/// `xi = max(0, min(xi_raw, xi_max))` with the maximum **last** (ADR-027,
/// ADR-068). The order is not decoration: `min(max(0, xi_raw), xi_max)` lets a
/// negative cap straight through, and a reaction with a negative extent runs
/// backwards while conserving matter, so both ledgers close.
///
/// The division is written over non-negative operands on purpose. `nu` of an
/// input is negative and integer division in Rust truncates **toward zero**, so
/// `amount / nu` at `amount >= 0` is a ceiling, not a floor — one quantum more
/// than the substrate allows, and the pool goes negative with the balance exact.
///
/// The ceiling of an output is not here and must not be: it is a guarantee of
/// the validator through `max_conc` (ADR-039, ADR-041). A product past its
/// declared limit means the declaration is wrong, and saturating it here would
/// let a wrong declaration survive the run.
#[inline(always)]
fn extent_cap(
    src32: &[M32],
    src64: &[M64],
    rx: &Rx,
    p: &ReactParams,
    idx: u32,
    r: usize,
    raw: i32,
) -> i32 {
    // No input at all means no cap from the inputs.
    let mut cap = i64::MAX;
    for j in rx.begin[r]..rx.begin[r] + rx.len[r] {
        let s = rx.nu_sub[j as usize];
        let nu = i64::from(rx.nu[j as usize]);
        // The third of the three guards on `s_energy`, and the third to change
        // which sign it saves: after ADR-081 an exothermic `nu_E` is positive, so
        // what would reach a lane that does not exist is an **endothermic**
        // record. See `rate_of` for why the branch is not dead.
        if s == p.s_energy || nu >= 0 {
            continue;
        }
        let amount = amount_get(src32, src64, rx, p, idx, s);
        // `-nu` is positive, so at a non-negative amount the truncation of the
        // division is the floor this needs. At a negative amount the quotient is
        // negative either way and the saturation below is what answers it.
        let quanta = amount / -nu;
        if quanta < cap {
            cap = quanta;
        }
    }

    let capped = if i64::from(raw) < cap {
        i64::from(raw)
    } else {
        cap
    };
    if capped > 0 { capped as i32 } else { 0 }
}

/// Add what reaction `r` would take out of each substance at extent `want`.
///
/// Enthalpy is skipped for the reason `rate_of` gives: a record with `nu_E < 0`
/// looks like an input, and a demand accumulated against a pool that does not
/// exist would make the whole chemistry compete for it. Since ADR-081 that is the
/// **endothermic** case — `nu_E` is stated in the direction of the field, so an
/// exothermic record is positive and never reaches this branch at all.
#[inline(always)]
fn accumulate_demand(demand: &mut [i64; S_MAX], rx: &Rx, p: &ReactParams, r: usize, want: i32) {
    for j in rx.begin[r]..rx.begin[r] + rx.len[r] {
        let s = rx.nu_sub[j as usize] as usize;
        let nu = i64::from(rx.nu[j as usize]);
        if s == p.s_energy as usize || nu >= 0 {
            continue;
        }
        demand[s] += -nu * i64::from(want);
        // TODO(demand-bound): nothing in the corpus bounds this sum. It
        // accumulates `|nu| * want` over up to R_MAX = 64 reactions in an i64,
        // while the ceiling of a wide substance under ADR-039 and ADR-062 is
        // `2^(w-4)`; neither ADR-039 nor ADR-041 gives an inequality from which
        // non-overflow would follow. In release an overflow wraps into a
        // *negative* demand, that is a competition coefficient out of nowhere,
        // so the assertion below is the whole of the guard until a record
        // supplies the bound.
        debug_assert!(
            demand[s] >= 0,
            "the demand accumulator of substance {s} wrapped: the bound on \
             |nu| * want summed over the reactions of one voxel is not written \
             down anywhere"
        );
    }
}

/// The one multiplier every reaction of this voxel is scaled by.
///
/// The minimum over the **oversold** substances, and one on a voxel where
/// nothing is oversold. Two neighbouring readings are wrong and neither
/// disturbs an invariant: a minimum over *every* substance is below one always,
/// so the whole of the chemistry is uniformly slowed by an unknown factor; and a
/// coefficient applied only to the consumers of the oversold pool conserves
/// every element exactly. `all_reactions_share_one_competition_coefficient` is
/// the only thing that separates the three (ADR-041, ADR-068).
///
/// `max(0, .)` is ADR-068: a negative pool must not turn the multiplier of every
/// reaction of the voxel negative, including the ones that never touched it.
/// With the input cap taken before the demand is summed, a negative pool
/// contributes no demand and never reaches this minimum — the saturation here is
/// the second lock on the same door, and it is the one that holds if the two
/// passes are ever reordered.
///
/// TODO(competition-rounding): the representation of the coefficient is not
/// decided by any record. As a `Q` the comparison and the division go through
/// `qdiv`, so the guarantee "the scaled demands do not exceed what is there"
/// rests on `f32`; as a pair of integers it would be exact and would need a
/// different signature. The corpus gives only "the shared multiplier is the
/// minimum over the oversold" (ADR-041) and "`scale = max(0, .)`" (ADR-068).
#[inline(always)]
fn competition_scale(
    demand: &[i64; S_MAX],
    src32: &[M32],
    src64: &[M64],
    rx: &Rx,
    p: &ReactParams,
    idx: u32,
) -> Q {
    let mut scale = Q::ONE;
    for s in 0..p.n_substances {
        let asked = demand[s as usize];
        if asked <= 0 {
            continue;
        }
        let have = amount_get(src32, src64, rx, p, idx, s);
        if have >= asked {
            continue;
        }
        let ratio = if have > 0 {
            qdiv(as_number(have), as_number(asked))
        } else {
            // Saturated at zero rather than allowed negative (ADR-068).
            Q::ZERO
        };
        if ratio < scale {
            scale = ratio;
        }
    }
    scale
}

/// `floor(want * scale)`, and the floor is the point.
///
/// Any other rule breaks the guarantee the coefficient exists for. Rounding to
/// nearest overshoots by up to half a quantum per reaction, so the sum of the
/// scaled demands exceeds what is there by one unit per reaction and the pool
/// goes negative with the balance exact. Rounding halves away from zero —
/// NUMERIC.md section 3, the rule of the whole system — is exactly that mistake
/// written in the house style: it is there to keep a *flux* antisymmetric
/// (ADR-034), and there is no flux here.
///
/// The floor is safe for the reason everything else about `xi` is safe:
/// conservation is a property of `nu`, not of the extent (ADR-027).
///
/// TODO(scale-rounding): which rule applies here is not assigned by any record.
/// NUMERIC.md section 3 knows two — halves away from zero, and stochastic — and
/// neither is named for this operation; a fourth rounding site in the chemistry
/// is not opened by any entry either. The exact integer form,
/// `want * have / asked`, is not available: at the ceiling `2^60` of a 64-bit
/// substance and a `want` up to `2^31` the product does not fit an i64, and WGSL
/// has no i128. This wants a line in the journal, not a decision here.
#[inline(always)]
fn scale_extent(want: i32, scale: Q) -> i32 {
    if want <= 0 {
        return 0;
    }
    // The common case, and it is not an optimisation: `f32` cannot hold a `want`
    // above 2^24 exactly, and a voxel where nothing is oversold must not lose a
    // quantum to a round trip through `Q`.
    if scale >= Q::ONE {
        return want;
    }

    let scaled = qmul(as_number(i64::from(want)), scale);
    // Floor, built out of the one rounding the numeric layer offers plus a step
    // down when it rounded up. `numeric/` exposes no floor of its own: a fourth
    // named crossing between `M` and `Q` was refused by ADR-060, and this is not
    // one — it is two calls of the same crossing.
    let nearest = q_round_64(scaled).to_i64();
    let floored = if as_number(nearest) > scaled {
        nearest - 1
    } else {
        nearest
    };
    if floored > 0 { floored as i32 } else { 0 }
}

/// An integer as a number of the same value, for a comparison or a ratio.
///
/// `q_conc` with a per-unit factor of one, which is the idiom `kernels/diffuse.rs`
/// already uses for "an amount difference, as a number": it is a named crossing
/// (NUMERIC.md section 1) rather than a new place where rounding may happen.
#[inline(always)]
fn as_number(value: i64) -> Q {
    q_conc_64(M64::new(value), Q::ONE)
}

/// The coarse cell covering a fine voxel.
///
/// Temperature is a quantity of the coarse grid — sixty-four fine voxels share
/// one `T`, and the Q10 factor of a reaction reads the covering cell (ADR-062).
///
/// The shift is taken **per axis**, exactly as in `kernels/fold.rs` and for the
/// same reason: `idx >> (3 * lod)` is the same expression with the axes confused
/// and names a stripe along X, which no conservation test can see because
/// temperature conserves nothing.
///
/// TODO(temperature-transport): how the temperature reaches this kernel is not
/// settled. ADR-062 fixes that `T` belongs to the coarse cell and that the Q10
/// factor reads the covering one, but no record names the buffer, its producer,
/// or this mapping; `lod`, `cnx` and `cny` in [`ReactParams`] are this file's
/// proposal, not a quotation. The alternative — the host expanding `T` onto the
/// fine grid — costs a field at 128^3 and is not weighed in the journal either.
#[inline(always)]
fn coarse_of(p: &ReactParams, idx: u32) -> u32 {
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    (x >> p.lod) + (y >> p.lod) * p.cnx + (z >> p.lod) * p.cnx * p.cny
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A non-cubic grid: with equal extents every mistake that confuses the axes
    /// is hidden, because all three strides agree.
    const NX: u32 = 2;
    const NY: u32 = 3;
    const NZ: u32 = 2;
    const N_VOXELS: u32 = NX * NY * NZ;

    /// Four substances, `X` the wide one, and enthalpy past the last of them.
    const X: u32 = 0;
    const Y: u32 = 1;
    const Z: u32 = 2;
    const W: u32 = 3;
    const N_SUBSTANCES: u32 = 4;
    const S_ENERGY: u32 = N_SUBSTANCES;

    /// `lane_of[s]`: `X` is wide and takes lane 0 of the wide field, so every
    /// narrow substance sits one below its index (ADR-056). The last entry is
    /// the enthalpy index, and it points **past both fields on purpose**: a
    /// kernel that treated the energy record as a substrate would index a lane
    /// that does not exist, and this fixture makes that a panic rather than a
    /// quietly borrowed number.
    const LANES: [u32; N_SUBSTANCES as usize + 1] = [0, 0, 1, 2, 4096];
    const MASK: u32 = 1 << X;

    /// `k_i - e_r` per substance: the storage coefficient is the molar one
    /// shifted by this (ADR-039).
    const SHIFT: [u32; N_SUBSTANCES as usize] = [4, 2, 2, 2];
    const E_R: u32 = 4;

    fn q(v: f64) -> Q {
        Q::from_f64(v)
    }

    fn nu_of(s: u32, molar: i64) -> i32 {
        (molar << SHIFT[s as usize]) as i32
    }

    /// One reaction as the loader would hand it over.
    struct Recipe {
        rid: u32,
        molar: &'static [(u32, i64)],
        molar_energy: i64,
        vmax: f64,
        t_vmax: f64,
        catalyst: u32,
        /// The half-saturation constant of every input of this recipe, mol/m^3.
        /// Zero is the value most of the fixtures below want — the Michaelis
        /// term is then exactly one at any non-empty pool and the extent is the
        /// formula rather than an approximation of it — and
        /// `the_rate_is_limited_by_its_scarcest_substrate` is the one that needs
        /// it nonzero.
        km: f64,
    }

    fn recipe(molar: &'static [(u32, i64)], vmax: f64) -> Recipe {
        Recipe {
            rid: 0x1234,
            molar,
            molar_energy: -3,
            vmax,
            t_vmax: 300.0,
            catalyst: NO_CATALYST,
            km: 0.0,
        }
    }

    #[derive(Default)]
    struct Tables {
        nu: Vec<i32>,
        nu_sub: Vec<u32>,
        begin: Vec<u32>,
        len: Vec<u32>,
        e_r: Vec<u32>,
        lane: Vec<u32>,
        cat: Vec<u32>,
        rid: Vec<u32>,
        vmax: Vec<Q>,
        q10: Vec<Q>,
        t_vmax: Vec<Q>,
        km: Vec<Q>,
        conc_per_unit: Vec<Q>,
    }

    fn tables(recipes: &[Recipe]) -> Tables {
        let mut t = Tables {
            lane: LANES.to_vec(),
            ..Default::default()
        };
        for s in 0..N_SUBSTANCES {
            t.conc_per_unit
                .push(q(1.0 / f64::from(1u32 << (SHIFT[s as usize] + E_R))));
        }
        for r in recipes {
            t.begin.push(t.nu.len() as u32);
            for &(s, molar) in r.molar {
                t.nu.push(nu_of(s, molar));
                t.nu_sub.push(s);
                // Parallel to `nu`, one entry per participant. At the zero most
                // recipes carry the Michaelis term is exactly one at any
                // non-empty pool, so the extent is the formula and not an
                // approximation of it.
                t.km.push(q(r.km));
            }
            t.nu.push(nu_of(X, r.molar_energy));
            t.nu_sub.push(S_ENERGY);
            t.km.push(Q::ZERO);
            t.len.push(t.nu.len() as u32 - t.begin[t.begin.len() - 1]);
            t.e_r.push(E_R);
            t.cat.push(r.catalyst);
            t.rid.push(r.rid);
            t.vmax.push(q(r.vmax));
            t.q10.push(q(2.0));
            t.t_vmax.push(q(r.t_vmax));
        }
        t
    }

    impl Tables {
        fn rx(&self) -> Rx<'_> {
            Rx {
                nu: &self.nu,
                nu_sub: &self.nu_sub,
                begin: &self.begin,
                len: &self.len,
                e_r: &self.e_r,
                lane: &self.lane,
                cat: &self.cat,
                rid: &self.rid,
                vmax: &self.vmax,
                q10: &self.q10,
                t_vmax: &self.t_vmax,
                km: &self.km,
                conc_per_unit: &self.conc_per_unit,
            }
        }
    }

    struct World {
        src32: Vec<M32>,
        src64: Vec<M64>,
        dst32: Vec<M32>,
        dst64: Vec<M64>,
        energy: Vec<M64>,
        /// The extent report of ADR-080, sized for `R_MAX` reactions so that the
        /// fixture does not have to be rebuilt per scenario. The **stride** is
        /// still `p.n_reactions`, which is what the kernel writes on, so
        /// [`World::xi`] takes the params rather than assuming the allocation.
        xi: Vec<M32>,
        temperature: Vec<Q>,
        catalyst: Vec<Q>,
    }

    impl World {
        fn new(amounts: [i64; N_SUBSTANCES as usize]) -> Self {
            let mut world = World {
                src32: vec![M32::ZERO; (3 * N_VOXELS) as usize],
                src64: vec![M64::ZERO; N_VOXELS as usize],
                dst32: vec![M32::ZERO; (3 * N_VOXELS) as usize],
                dst64: vec![M64::ZERO; N_VOXELS as usize],
                energy: vec![M64::ZERO; N_VOXELS as usize],
                xi: vec![M32::ZERO; N_VOXELS as usize * R_MAX],
                temperature: vec![q(300.0); N_VOXELS as usize],
                catalyst: vec![q(1.0); N_VOXELS as usize],
            };
            for s in 0..N_SUBSTANCES {
                for idx in 0..N_VOXELS {
                    let at = (LANES[s as usize] * N_VOXELS + idx) as usize;
                    if MASK & (1 << s) != 0 {
                        world.src64[at] = M64::new(amounts[s as usize]);
                    } else {
                        world.src32[at] = M32::new(amounts[s as usize] as i32);
                    }
                }
            }
            world
        }

        fn amount(&self, s: u32, idx: u32) -> i64 {
            let at = (LANES[s as usize] * N_VOXELS + idx) as usize;
            if MASK & (1 << s) != 0 {
                self.dst64[at].to_i64()
            } else {
                self.dst32[at].to_i64()
            }
        }

        fn before(&self, s: u32, idx: u32) -> i64 {
            let at = (LANES[s as usize] * N_VOXELS + idx) as usize;
            if MASK & (1 << s) != 0 {
                self.src64[at].to_i64()
            } else {
                self.src32[at].to_i64()
            }
        }

        fn voxel(&mut self, rx: &Rx, p: &ReactParams, idx: u32) {
            react_voxel(
                &self.src32,
                &self.src64,
                &mut self.dst32,
                &mut self.dst64,
                &mut self.energy,
                &mut self.xi,
                &self.temperature,
                &self.catalyst,
                rx,
                p,
                idx,
            );
        }

        /// The extent reaction `r` reported in this voxel (ADR-080).
        ///
        /// Read off the report rather than off the amounts, unlike
        /// [`World::extent`] one function down: the whole point of the slice is
        /// that it is a **second** witness, so a reader that derived it from the
        /// field would be the first witness wearing the second one's name.
        fn xi(&self, p: &ReactParams, r: usize, idx: u32) -> i64 {
            self.xi[(idx as usize) * (p.n_reactions as usize) + r].to_i64()
        }

        fn run(&mut self, rx: &Rx, p: &ReactParams) {
            for idx in 0..N_VOXELS {
                self.voxel(rx, p, idx);
            }
        }

        /// The extent of the reaction whose product is `s`, read back out of the
        /// amounts: the kernel writes state, not diagnostics.
        fn extent(&self, s: u32, idx: u32) -> i64 {
            (self.amount(s, idx) - self.before(s, idx)) / i64::from(nu_of(s, 1))
        }
    }

    fn params(n_reactions: u32) -> ReactParams {
        ReactParams {
            lane_len: N_VOXELS,
            nx: NX,
            ny: NY,
            n_voxels: N_VOXELS,
            n_substances: N_SUBSTANCES,
            n_reactions,
            tick: 3,
            run_key: 0x2b_2b_2b,
            width_mask: MASK,
            s_energy: S_ENERGY,
            lod: 0,
            cnx: NX,
            cny: NY,
            volume: Q::ONE,
            dt: Q::ONE,
        }
    }

    /// `X + 2 Y -> Z` at `x = 0.25 * 1 * 1 * 2^4 = 4` quanta, whole, so the
    /// stochastic draw cannot change the answer.
    fn one_reaction() -> Tables {
        tables(&[recipe(&[(X, -1), (Y, -2), (Z, 1)], 0.25)])
    }

    #[test]
    fn the_energy_record_is_not_a_substrate() {
        // Endothermic: `nu_E` is negative, so from inside the loops the enthalpy
        // record looks exactly like an input. If either the cap or the demand
        // pass treats it as one, the chemistry is limited by a pool that does not
        // exist while the vector `nu` is applied in full — and both halves of the
        // invariant close.
        //
        // **Relabelled by ADR-081, not re-signed, and the difference is the whole
        // value of this fixture.** That record states `nu_E` in the direction of
        // the field, so an exothermic reaction now has `nu_E > 0` — and a
        // positive coefficient is short-circuited by `nu >= 0` before the guard
        // on `s_energy` is ever consulted. Rewriting the sign here to match the
        // new convention would leave the three guards without a single test in
        // the repository; the reaction stays endothermic instead.
        //
        // The fixture points `lane[s_energy]` past both fields, so the mistake is
        // a panic here rather than a quietly borrowed number.
        let t = one_reaction();
        let p = params(1);
        let mut world = World::new([100_000, 100_000, 0, 0]);
        world.run(&t.rx(), &p);

        for idx in 0..N_VOXELS {
            assert_eq!(world.extent(Z, idx), 4, "the unconstrained demand is 4");
            assert_eq!(
                world.energy[idx as usize],
                M64::new(i64::from(nu_of(X, -3)) * 4)
            );
        }
    }

    #[test]
    fn the_energy_delta_is_written_not_added() {
        // ADR-045 word for word: the cell is overwritten, which is why there is
        // no zeroing pass. This is not a test about today — it is about the day
        // somebody restores the three reaction steps ADR-050 abolished. With `+=`
        // it is green, and with an external zeroing pass it is green too; the
        // difference shows up only here.
        let t = one_reaction();
        let p = params(1);
        let mut world = World::new([100_000, 100_000, 0, 0]);
        for (i, cell) in world.energy.iter_mut().enumerate() {
            *cell = M64::new(-(i as i64) - 7_777);
        }

        let target = 3u32;
        world.voxel(&t.rx(), &p, target);

        assert_eq!(
            world.energy[target as usize],
            M64::new(i64::from(nu_of(X, -3)) * 4),
            "the increment was added to what was in the cell instead of replacing it"
        );
        for idx in 0..N_VOXELS {
            if idx != target {
                assert_eq!(
                    world.energy[idx as usize],
                    M64::new(-(i64::from(idx)) - 7_777),
                    "a voxel wrote into a cell that is not its own"
                );
            }
        }
    }

    #[test]
    fn the_extent_slice_is_overwritten_not_accumulated() {
        // The twin of the test above, one slice over, and it exists because the
        // consequence is worse here. `energy_delta` feeds step `i'`; the extent
        // slice feeds `Ledger::residual_matter`, which is the one thing in the
        // project that decides whether a tick is trustworthy. A `+=` in the write
        // would make a reduction over a voxel the dispatch skipped credit last
        // tick's chemistry against a field that did not move — the
        // stale-accumulator failure ADR-045 removed the clearing pass for, arriving
        // through the door built to detect it (ADR-080).
        //
        // Three claims, and each fails to a different mistake:
        //
        // (1) the cell is **replaced**. Junk is planted in every cell first, so a
        //     `+=` shows up as junk plus four rather than as four;
        // (2) a reaction whose extent came out zero still reports. The write sits
        //     above the `continue` in the third pass, and moved below it a
        //     reaction that stopped running would leave its previous quantum in
        //     place — which is the same stale value in a slower disguise;
        // (3) a voxel writes only its own block of `R` cells. `idx * R + r`
        //     transposed to `r * n_voxels + idx` stays inside the slice, gives
        //     plausible numbers, and is a different reaction's extent.
        let t = tables(&[
            recipe(&[(X, -1), (Y, -2), (Z, 1)], 0.25),
            // No `X` left to work on once the first reaction is fed a pool of
            // zero, so this one's extent is zero in every voxel — and it has to
            // say so rather than say nothing.
            recipe(&[(W, -1), (Z, 1)], 0.5),
        ]);
        let p = params(2);
        let mut world = World::new([100_000, 100_000, 0, 0]);
        for (i, cell) in world.xi.iter_mut().enumerate() {
            *cell = M32::new(-(i as i32) - 555);
        }

        let target = 3u32;
        world.voxel(&t.rx(), &p, target);

        assert_eq!(
            world.xi(&p, 0, target),
            4,
            "the extent was added to what was in the cell instead of replacing it"
        );
        assert_eq!(
            world.xi(&p, 1, target),
            0,
            "a reaction that did not run left the previous value standing"
        );
        for idx in 0..N_VOXELS {
            if idx == target {
                continue;
            }
            for r in 0..2 {
                let at = (idx as usize) * 2 + r;
                assert_eq!(
                    world.xi[at],
                    M32::new(-(at as i32) - 555),
                    "voxel {target} wrote into the block of voxel {idx}"
                );
            }
        }
    }

    #[test]
    fn reaction_never_produces_negative_amount() {
        // Not a statement about a clamp — there is none — but about the input cap
        // and the shared coefficient being enough together. It fails if the
        // scaled extent is rounded up or to nearest: the sum of the scaled
        // demands then exceeds what is there by one unit per reaction, and the
        // pool goes negative with the balance exactly closed.
        let t = tables(&[
            recipe(&[(X, -1), (Y, -2), (Z, 1)], 0.25),
            recipe(&[(Y, -2), (W, 1)], 0.4375),
        ]);
        let p = params(2);

        for pool in [1i64, 3, 7, 8, 15, 24, 31, 32, 33] {
            let mut world = World::new([100_000, pool, 0, 0]);
            world.run(&t.rx(), &p);
            for idx in 0..N_VOXELS {
                for s in 0..N_SUBSTANCES {
                    assert!(
                        world.amount(s, idx) >= 0,
                        "substance {s} went to {} at a starting pool of {pool}",
                        world.amount(s, idx)
                    );
                }
            }
        }
    }

    #[test]
    fn the_q10_factor_is_measured_from_t_vmax() {
        // Half of the claim is structural and lives in the signatures: `T_ref`
        // reaches no parameter of this kernel, so the scenario's zero of enthalpy
        // storage cannot be substituted for the reference temperature of the
        // kinetics (ADR-048). The other half is arithmetic.
        let mut cold = recipe(&[(X, -1), (Y, -2), (Z, 1)], 0.25);
        cold.t_vmax = 290.0;
        let t = tables(&[recipe(&[(X, -1), (Y, -2), (Z, 1)], 0.25), cold]);
        // The second reaction shares the product of the first, so read the two
        // one at a time.
        let p_first = params(1);

        let mut world = World::new([1_000_000, 1_000_000, 0, 0]);
        world.run(&t.rx(), &p_first);
        for idx in 0..N_VOXELS {
            // T == t_vmax: the factor is exactly one and the rate is vmax.
            assert_eq!(world.extent(Z, idx), 4);
        }

        let only_cold = tables(&[{
            let mut r = recipe(&[(X, -1), (Y, -2), (Z, 1)], 0.25);
            r.t_vmax = 290.0;
            r
        }]);
        let mut warmer = World::new([1_000_000, 1_000_000, 0, 0]);
        warmer.run(&only_cold.rx(), &p_first);
        for idx in 0..N_VOXELS {
            // Ten kelvin above its own reference, at Q10 = 2: twice as fast.
            assert_eq!(
                warmer.extent(Z, idx),
                8,
                "two reactions with one vmax and different t_vmax must run at \
                 different extents"
            );
        }
    }

    #[test]
    fn the_rate_is_limited_by_its_scarcest_substrate() {
        // The other half of the rate law. Every fixture in this file and in
        // `tests/acceptance_reactions.rs` carries `km == 0`, and at zero the
        // Michaelis term is `conc/conc == 1` for any non-empty pool: the
        // concentration cancels out of the kinetics entirely, and with it both
        // `km` and `conc_per_unit`. Deleting `km` from `rate_of` and reading
        // `conc_per_unit` by lane instead of by substance are then two mutations
        // nothing sees. This test is where they die, and it has to pin an exact
        // extent for that — "the rate went down" is satisfied by both.
        //
        // The second one is the mistake the doc comment on `Rx` is about:
        // `conc_per_unit` is indexed by **substance** while the amount buffers
        // are indexed by lane, and here `lane[Y] == 0` against `Y == 1`, so a
        // lane-indexed read hands `Y` the per-unit value of the wide `X` and
        // shifts its concentration by `2^(k_X - k_Y)`. Nothing else notices: a
        // rate enters no invariant, so both ledgers close on the wrong number.
        let mut r = recipe(&[(X, -1), (Y, -2), (Z, 1)], 0.25);
        // Of the order of the pool concentrations below, which is what makes the
        // term a fraction rather than a rounding of one.
        r.km = 1.0;
        let t = tables(&[r]);
        let p = params(1);

        // `conc_Y = pool * 2^-6` and `conc_X = 100_000 * 2^-8 = 390.6`, so `Y`
        // is the scarce one and the term is `conc_Y / (1 + conc_Y)`:
        // `x = 0.25 * term * 2^4`, whole in every row and so beyond the reach of
        // the stochastic draw.
        for (pool, extent) in [(0i64, 0i64), (64, 2), (192, 3)] {
            let mut world = World::new([100_000, pool, 0, 0]);
            world.run(&t.rx(), &p);
            for idx in 0..N_VOXELS {
                assert_eq!(
                    world.extent(Z, idx),
                    extent,
                    "at a pool of {pool} the substrate term is not the one the \
                     rate law names"
                );
            }
        }
    }

    #[test]
    fn the_coefficient_saturates_at_zero_against_a_negative_pool() {
        // ADR-068 asks for `scale = max(0, .)`, and this is the only test that
        // can fail without it — the acceptance test that carries the ADR's name
        // cannot, and its own doc comment says why. `extent_cap` runs before the
        // demand pass, so a reaction consuming a negative pool comes out at
        // `want == 0`, contributes no demand, and `competition_scale` skips the
        // substance at `asked <= 0` before ever dividing by what is there. The
        // negative branch is unreachable through `react_voxel` today.
        //
        // Which is exactly why it is worth a test of its own rather than a
        // deletion. The saturation is the second lock on a door ADR-068 wants
        // locked twice, and it is the one that holds on the day the two passes
        // are reordered — the day the first lock stops closing. A test that can
        // only reach it through the kernel would go green on that day and stay
        // green.
        let t = one_reaction();
        let p = params(1);
        let world = World::new([100_000, -24, 0, 0]);

        // Hand-built: a demand against `Y` that the input cap would never let
        // through, so that the minimum reaches a negative `have`.
        let mut demand = [0i64; S_MAX];
        demand[Y as usize] = 32;
        let scale = competition_scale(&demand, &world.src32, &world.src64, &t.rx(), &p, 0);
        assert_eq!(
            scale,
            Q::ZERO,
            "a pool of -24 against a demand of 32 turned the multiplier of every \
             reaction of the voxel negative, including the ones that never \
             touched it (ADR-068)"
        );

        // And the innocent substance is still the ordinary case: nothing
        // oversold leaves the multiplier at one.
        let healthy = World::new([100_000, 100_000, 0, 0]);
        let mut none = [0i64; S_MAX];
        none[Y as usize] = 32;
        assert_eq!(
            competition_scale(&none, &healthy.src32, &healthy.src64, &t.rx(), &p, 0),
            Q::ONE
        );
    }

    #[test]
    fn the_result_does_not_depend_on_the_traversal_order() {
        // The same test diffusion has, and here it is the second line against a
        // read from the write buffer inside one voxel: `dst[at] += ...` in the
        // loop over reactions gives the same numbers as a local accumulator on
        // the CPU, and differs only when the pass is split in two dispatches.
        let t = tables(&[
            recipe(&[(X, -1), (Y, -2), (Z, 1)], 0.25),
            recipe(&[(Y, -2), (W, 1)], 0.25),
        ]);
        let p = params(2);

        let mut forwards = World::new([100_000, 32, 0, 0]);
        for idx in 0..N_VOXELS {
            forwards.voxel(&t.rx(), &p, idx);
        }

        let mut backwards = World::new([100_000, 32, 0, 0]);
        for idx in (0..N_VOXELS).rev() {
            backwards.voxel(&t.rx(), &p, idx);
        }

        assert_eq!(forwards.dst32, backwards.dst32);
        assert_eq!(forwards.dst64, backwards.dst64);
        assert_eq!(forwards.energy, backwards.energy);
        assert_ne!(forwards.dst32, forwards.src32);
    }

    #[test]
    fn an_output_over_its_declared_ceiling_is_not_clamped() {
        // `xi_max` is computed over the inputs only. The ceiling of an output is
        // a guarantee of the validator through `max_conc` (ADR-039), which
        // ADR-042 puts at 2^28, and this product starts four binary orders above
        // it. Saturating here would let a wrong declaration survive the run
        // quietly, which is the outcome ADR-041 refuses.
        let t = one_reaction();
        let p = params(1);
        let mut world = World::new([100_000, 100_000, 1 << 30, 0]);
        world.run(&t.rx(), &p);

        for idx in 0..N_VOXELS {
            assert_eq!(
                world.amount(Z, idx),
                (1 << 30) + i64::from(nu_of(Z, 1)) * 4,
                "the product was capped at a ceiling this kernel must not know"
            );
        }
    }

    #[test]
    fn a_coarse_cell_covers_its_own_eight_voxels() {
        // The mapping of the temperature field, per axis. `idx >> (3 * lod)` is
        // the same expression with the axes confused and names a stripe along X;
        // nothing conserves temperature, so no balance would notice.
        let mut p = params(0);
        p.nx = 4;
        p.ny = 4;
        p.n_voxels = 4 * 4 * 4;
        p.lod = 1;
        p.cnx = 2;
        p.cny = 2;

        // The eight fine voxels of the corner cube all map to coarse cell 0.
        for z in 0..2u32 {
            for y in 0..2u32 {
                for x in 0..2u32 {
                    assert_eq!(coarse_of(&p, x + y * 4 + z * 16), 0);
                }
            }
        }
        // One step along each axis lands on the neighbouring coarse cell, and on
        // a different one for each axis.
        assert_eq!(coarse_of(&p, 2), 1);
        assert_eq!(coarse_of(&p, 2 * 4), 2);
        assert_eq!(coarse_of(&p, 2 * 16), 4);
        assert_eq!(coarse_of(&p, 3 + 3 * 4 + 3 * 16), 7);
    }

    #[test]
    fn a_scaled_extent_rounds_down() {
        // Any other rule breaks the guarantee the coefficient exists for.
        assert_eq!(scale_extent(4, q(0.5)), 2);
        assert_eq!(scale_extent(5, q(0.5)), 2);
        assert_eq!(scale_extent(3, q(0.5)), 1);
        assert_eq!(scale_extent(1, q(0.5)), 0);
        assert_eq!(scale_extent(7, q(0.999)), 6);
        assert_eq!(scale_extent(0, q(0.5)), 0);
        assert_eq!(scale_extent(-3, q(0.5)), 0);
        // A coefficient of one leaves the extent alone exactly, including above
        // the 2^24 where an `f32` stops being able to hold it.
        assert_eq!(scale_extent(1 << 25, Q::ONE), 1 << 25);
        assert_eq!(scale_extent(4, Q::ZERO), 0);
    }

    #[test]
    fn the_last_voxel_of_the_grid_is_addressable() {
        // A guard against the mistake `kernels/light.rs` names: a host that
        // dispatched over the wrong domain reads past the end of a field. Here
        // the last voxel of the last lane is the far end of both buffers.
        let t = one_reaction();
        let p = params(1);
        let mut world = World::new([5, 5, 0, 0]);
        world.voxel(&t.rx(), &p, N_VOXELS - 1);
        assert_eq!(world.amount(Z, N_VOXELS - 1), 0, "5 units cap the extent");
    }
}
