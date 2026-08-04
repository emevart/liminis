//! Temperature: the heat capacity of a coarse cell, gathered from the fine
//! grid, and the division ADR-044 writes.
//!
//! ```text
//! C_cell[coarse] = sum over the 2^(3*lod) fine cells of sum over substances of
//!                      n[s] * c_p[s]
//! T[coarse]      = T_ref + H[coarse] / C_cell[coarse]
//! ```
//!
//! Two output cells, one invocation, one coarse cell. ADR-044 gives the formula
//! and one sentence that decides the shape of this file: "the denominator depends
//! on the composition of the voxel, so it is **recomputed** rather than cached".
//! A folded `C_cell` already exists — `Derived::energy().c_cell`, summed at load
//! out of `typical_conc` — and it is there to derive `k_E` and for nothing else.
//! Using it here would reverse that sentence without saying so, and the symptom
//! would be a temperature field that does not depend on what is in the voxels:
//! plausible everywhere, wrong wherever the composition moved, and in no
//! invariant at all, because temperature is class `Q` and conserves nothing.
//!
//! # Temperature is a quantity of the coarse grid
//!
//! ADR-062, in as many words: "temperature is a quantity of the coarse cell,
//! sixty-four fine voxels share one `T`, and the `q10` factor of a reaction reads
//! the covering cell". So the amounts are on `128^3`, the enthalpy is on `32^3`,
//! and `sum(n_i * c_p_i)` is a fold from the first onto the second — "the same
//! shape as the fold of the energy in ADR-045, and the technique there is
//! declared **general**". This kernel is that technique used a second time, and
//! `kernels/fold.rs` is the first.
//!
//! # Pure gather: not one atomic, not one write into another cell
//!
//! A coarse cell reads its own `2^(3*lod)` fine voxels of every lane, one cell of
//! the enthalpy field, and writes two cells — its own in `C_cell` and its own in
//! `T`. ADR-045 rejected the shorter arrangement, a fine voxel adding its
//! capacity into the covering coarse cell atomically, and the reason was not
//! contention: it would widen the one exception of ADR-034 — "the only cells a
//! kernel may write besides its own are the ledger's channel counters" — from
//! counters onto a field of state. So there is no atomic here and no dependence
//! on the traversal order.
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
//! The shift goes **per axis**. `fine_idx >> lod` and `coarse * 2^(3*lod) + f`
//! cut the index space into disjoint pieces exactly the way the right formula
//! does, so any check of the form "the totals closed" is green under both — and
//! here there is not even that, because a temperature is conserved by nothing at
//! all. The corpus pushes towards the mistake: `coarse_idx = fine_idx >> lod` is
//! printed as a *definition* in `docs/CONFIG_SCHEMA.md` (lines 190 and 504), in
//! `config/schema.rs` and in `config/validate.rs` in three places. Only SPEC
//! section 1.5 is right, and `the_denominator_gathers_the_per_axis_cube` is what
//! holds this file to it. Its mirror is [`coarse_coords`]: a decode of the coarse
//! linear index by the **fine** extents, which is partly masked on a cubic grid
//! and is why every fixture below runs on a grid whose three coarse extents are
//! pairwise different.
//!
//! # The amounts are summed as integers, once per substance
//!
//! The inner loops sum `i64` and cross into `Q` exactly once per substance,
//! through `q_conc_64`. A kernel that converted each of the sixty-four fine
//! voxels separately and added them with `qadd` would drop every bit below the
//! twenty-fourth of each term: on the registry the project carries, a voxel of
//! water holds far more than `2^24` storage units, so the denominator would stay
//! entirely plausible while the contribution of a trace substance vanished
//! outright. That is the argument `kernels/fold.rs` makes about its own two
//! accumulators, and it is the same argument.
//!
//! # A negative `c_p` is physics, and "repairing" it is the quiet mistake
//!
//! Aqueous sulfate has a partial molar heat capacity of about
//! `-293 J/(mol K)` — `configs/scenarios/h2s-oxidation.toml` explains at length
//! why the solvent has to be present for that to be safe. So a term of this sum
//! is legitimately negative, and `max(0, .)` or `abs()` over a term (or over the
//! folded per-unit coefficient on the host) gives a positive, plausible
//! denominator: 0.2% off on the corpus composition and qualitatively wrong on a
//! composition without the solvent. Nothing but
//! `a_negative_heat_capacity_is_summed_and_not_dropped` can see it.
//!
//! # What happens when the denominator is not positive
//!
//! A cell whose composition sums to zero or below has no temperature to derive,
//! and ADR-079 settles what it answers instead: `T := T_ref`, the zero of
//! enthalpy storage, in **every** profile, and nothing is divided. Two things
//! that answer is not. It is not a `debug_assert`: a cell empty of solvent is a
//! legal state of a legal world — `[initial]` puts a substance on one side of a
//! layer (ADR-077), and the domain above the layer holds none of it — so a
//! panicking kernel would make a legal world unrunnable in the profile the tests
//! run in and runnable in the one the science runs in. And it is not a division:
//! `qdiv` by zero is an infinity, `expect_finite` compiles to nothing in release,
//! `qpow(q10, inf)` is an infinity, `xi` saturates at `i32::MAX` and is then
//! trimmed by the substrate actually present — so from the outside the reaction
//! merely "runs at full speed" while both halves of the invariant close exactly.
//!
//! The price ADR-079 names: nothing counts how many cells answered this way, so
//! a world that has lost its solvent everywhere is a world running its whole
//! chemistry at `T_ref` in silence.
//!
//! # `T_ref` is the zero of storage and is never a reference of the kinetics
//!
//! [`TemperatureParams::t_ref`] is the only place in `kernels/` where the
//! scenario's zero of enthalpy storage exists at all. ADR-044 keeps two reference
//! states apart and unrelated; ADR-048 keeps a third one, `t_vmax`, per reaction,
//! and calls substituting `T_ref` for it "not a saving of one key but a mistake".
//! `kernels/react.rs` states the other half — `T_ref` reaches no field of
//! `ReactParams` — and now that the name exists in this directory, the two
//! statements are one grep apart.

use crate::numeric::{M32, M64, Q, q_conc_64, qadd, qdiv};

/// Parameters of one application. Scalars only: in WGSL this is a uniform buffer,
/// and every number in it is a place where the host's scale can drift away from
/// the kernel's (`ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug)]
pub struct TemperatureParams {
    /// Voxels along X of the **fine** grid. The coarse extents are never passed
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
    /// The enthalpy grid, and never the velocity one. A world has two coarse
    /// grids and they differ — `32^3` against `64^3` over a `128^3` base — and
    /// the wrong one gives every cell the composition of a plausible neighbour.
    /// The host checks that with `process::coarse_shape_agrees`; from in here
    /// nothing can.
    pub lod: u32,
    /// How many voxels the fine grid holds; the stride between lanes of the
    /// amount buffers.
    pub n_voxels: u32,
    /// The stride of an amount lane. On a `world::Field` it is `n_voxels + 1`
    /// — the voxels and the ghost cell after them (ADR-059) — and the host is
    /// the only place that knows so; this kernel is handed the number.
    ///
    /// Separate from [`TemperatureParams::n_voxels`] because the two mean different things
    /// and only one of them is an address. A dispatch runs over `0..n_voxels`;
    /// an amount lives at `lane[s] * lane_len + idx`. Using the voxel count as
    /// the stride reads one lane short of where the substance is, and on the
    /// registry the project carries that is **right** for the first lane of a
    /// width class and wrong by one element per lane after it — which is the
    /// same silent shape ADR-056 warns about for `s * n_voxels + idx`.
    pub lane_len: u32,
    /// How many substances the registry holds. The loop bound of the sum, and the
    /// length of both tables in [`Heat`].
    pub n_substances: u32,
    /// Bit `s` set: substance `s` is stored 64-bit (ADR-040). The branch is on the
    /// substance index, the same for every cell, so on the GPU it is a uniform
    /// jump rather than a diverging warp.
    pub width_mask: u32,
    /// The scenario's zero of enthalpy **storage**, in kelvin (ADR-044).
    ///
    /// Arbitrary by construction: the field holds enthalpy relative to it so that
    /// no bits are spent on a constant that takes no part in the dynamics. It is
    /// not the thermochemical 298.15 K, which exists only to check a declared
    /// reaction enthalpy against the enthalpies of formation, and it is not the
    /// reference temperature of a Q10 factor, which is `t_vmax` and lives per
    /// reaction (ADR-048).
    pub t_ref: Q,
    /// What one storage unit of enthalpy is worth in joules: `2^-k_E`, folded on
    /// the host (ADR-062).
    ///
    /// `2^-k_E` and not `2^k_E`. The two are each other's reciprocal and the
    /// compiler cannot tell them apart; at the derived `k_E = 67` the wrong one
    /// puts the temperature forty binary orders out, which is at least loud. The
    /// quiet neighbour of that mistake is a factor of `2^(3*lod)` from a
    /// denominator gathered on the wrong grid, and it is the one
    /// `temperature_from_enthalpy_round_trips` is built to catch.
    pub joules_per_unit: Q,
}

/// The two read-only tables, both indexed by **substance**. In WGSL each is one
/// `var<storage, read>`.
///
/// They are in a struct of their own and not in [`TemperatureParams`] for the
/// reason `Rx` gives in `kernels/react.rs`: `Params` holds scalars only, because
/// in WGSL it is a uniform buffer.
pub struct Heat<'a> {
    /// `lane_of[s]`: which lane of its width class substance `s` occupies
    /// (ADR-056).
    ///
    /// The address of an amount is `lane[s] * n_voxels + idx` and never
    /// `s * n_voxels + idx`. `lane == s` is true nowhere by rule, and breaking the
    /// rule is silent: the kernel reads a lane belonging to some other substance,
    /// nothing is lost so no balance moves, and on the registry the project
    /// carries water stands first and takes lane 0 — so the wrong expression is
    /// *right* at `s == 0` and off by one from there on.
    pub lane: &'a [u32],
    /// What one storage unit of substance `s` is worth as heat capacity:
    /// `c_p[s] * 2^-k[s]`, in J/(K * unit). Indexed by **substance**, like
    /// `Rx::conc_per_unit` and for the same reason (ADR-056): laid out by lane it
    /// shifts a contribution by `2^(k_i - k_j)`, tens of binary orders, with every
    /// balance closing exactly because a denominator of a temperature enters no
    /// invariant.
    ///
    /// **The sign is carried through.** Zero is legal — the proton declares
    /// exactly that by convention — and negative is legal and real; see the module
    /// header.
    pub capacity_per_unit: &'a [Q],
}

/// One coarse cell: its heat capacity, and the temperature that follows from it.
///
/// `coarse` runs over `0..(nx>>lod)*(ny>>lod)*(nz>>lod)` — the **coarse** grid,
/// which is what this kernel has in common with `kernels/fold.rs` and with
/// nothing else in this directory. Reads its own `2^(3*lod)` fine voxels of every
/// lane and one cell of `enthalpy`; writes one cell of `heat_capacity` and one of
/// `temperature`, both its own.
///
/// `enthalpy` is state `N` — the **front** buffer of the field. The back buffer
/// holds state `N+1`, or whatever the last swap left there, and a temperature
/// derived from it is entirely plausible; nothing but
/// `the_enthalpy_is_read_from_the_front_buffer` in `process/temperature.rs` can
/// tell the two apart.
#[allow(clippy::too_many_arguments)]
// Eight arguments against a clippy threshold of seven. The signature is fixed by
// ADR-034 and every slice here is one binding in WGSL, so folding them into a
// struct would buy a lint and cost the shape the port depends on — the same
// trade `kernels/react.rs` makes and says so.
pub fn temperature_cell(
    amounts_32: &[M32],
    amounts_64: &[M64],
    enthalpy: &[M64],
    heat_capacity: &mut [Q],
    temperature: &mut [Q],
    heat: &Heat,
    p: &TemperatureParams,
    coarse: u32,
) {
    // The loud half of the dispatch contract, on the precedent of `fold_energy`:
    // a host that dispatched this over the fine grid gets a `cz` past the coarse
    // field, and reads past the end of `enthalpy`. Here that is a panic; in WGSL
    // it is an out-of-bounds access the specification allows to land on another
    // binding.
    debug_assert!(
        coarse < n_coarse_cells(p),
        "temperature_cell is dispatched over coarse cells, not fine voxels: \
         coarse {coarse} is past their count {}",
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

    let mut capacity = Q::ZERO;

    // Substance outermost, cube innermost, and the nesting is load-bearing twice
    // over. The width branch is taken once per substance rather than once per
    // voxel, which is what makes it a uniform jump on the GPU (ADR-040); and the
    // `2^(3*lod)` amounts of one substance are summed as integers before anything
    // crosses into `Q` (see the module header).
    for s in 0..p.n_substances {
        let lane = heat.lane[s as usize];
        let wide = p.width_mask & (1 << s) != 0;

        // TODO(capacity-sum-bound): nothing in the corpus bounds this sum. ADR-039
        // caps one voxel's amount at `2^(32-4)` for a substance whose scale came
        // from its own ceiling, but a substance raised to `e_r` (ADR-040 — water,
        // on the registry the project carries) is capped only by fitting an `i64`
        // at all, and `2^(3*lod)` of those do not. In release an overflow wraps
        // into a *negative* capacity, which reaches the branch below as "this cell
        // has no meaningful temperature"; in debug the addition panics, which is
        // the whole of the guard until a record supplies the inequality. The twin
        // of `TODO(demand-bound)` in `kernels/react.rs`, and open for the same
        // reason.
        let mut units = 0i64;
        for dz in 0..span {
            for dy in 0..span {
                for dx in 0..span {
                    let at = (lane * p.lane_len + index(p, x0 + dx, y0 + dy, z0 + dz)) as usize;
                    units += if wide {
                        amounts_64[at].to_i64()
                    } else {
                        amounts_32[at].to_i64()
                    };
                }
            }
        }

        // The one crossing from `M` into `Q` on this path, taken once per
        // substance. `q_conc_64` rather than a fourth named crossing: ADR-060
        // refused the fourth christening separately, and this is the third
        // crossing of NUMERIC.md section 1 used with a different per-unit factor —
        // the idiom `kernels/react.rs` already uses for "an amount, as a number".
        // The wide door for both width classes, because the *sum* of `2^(3*lod)`
        // narrow amounts does not fit an `M32` even when each term does.
        capacity = qadd(
            capacity,
            q_conc_64(M64::new(units), heat.capacity_per_unit[s as usize]),
        );
    }

    // A cell with no temperature to derive answers `T_ref` and divides nothing
    // (ADR-079). The case is reachable and is not a typo: a negative partial
    // molar heat capacity is physics — aqueous sulfate declares about
    // `-293 J/(mol K)` — so a composition whose solvent is gone sums to zero or
    // below. The validator does not close the door either: `thermal_transport` in
    // `config/derive.rs` refuses `c_v <= 0` for the **typical** composition and
    // says nothing about the composition a voxel actually has.
    //
    // No assertion guards this, and that is the decision rather than an omission.
    // A `debug_assert` would make the CPU reference — which ADR-015 keeps forever
    // — stop on an input the shader is required to survive, and it would stop only
    // in the profile the tests run in. See the module header for what the answer
    // costs.
    //
    // The shape of the branch is `rate_of`'s `if saturation > Q::ZERO`, and
    // deliberately so: the same question — "divide, or answer without dividing" —
    // gets the same shape twice rather than two idioms.
    let t = if capacity > Q::ZERO {
        // `q_conc_64` again, with joules per storage unit as the per-unit factor:
        // the enthalpy field is class `M` and the quotient is class `Q`, so the
        // crossing has to happen, and it happens once.
        qadd(
            p.t_ref,
            qdiv(
                q_conc_64(enthalpy[coarse as usize], p.joules_per_unit),
                capacity,
            ),
        )
    } else {
        p.t_ref
    };

    heat_capacity[coarse as usize] = capacity;
    temperature[coarse as usize] = t;
}

/// The three coarse coordinates of a coarse linear index.
///
/// Divided by the **coarse** extents, `nx >> lod` and `ny >> lod`. Dividing by
/// the fine ones is the mirror of the `fine_idx >> lod` mistake of the module
/// header: it is partly masked on a cubic grid, which is why the fixtures below
/// run on a grid whose three coarse extents are pairwise different, and it hands
/// a cell the composition of some other cube while every sum in sight stays
/// perfectly plausible.
#[inline(always)]
fn coarse_coords(p: &TemperatureParams, coarse: u32) -> (u32, u32, u32) {
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
fn n_coarse_cells(p: &TemperatureParams) -> u32 {
    (p.nx >> p.lod) * (p.ny >> p.lod) * (p.nz >> p.lod)
}

/// The linear index of a **fine** voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
///
/// The kernel's own copy of `world::Grid::index`, for the reason `diffuse.rs`,
/// `light.rs` and `fold.rs` each keep one: `kernels/` depends on `numeric/` and on
/// nothing else, and a `Grid` is a host type with no meaning in WGSL. Like theirs,
/// it is kept from drifting by a test against that authority —
/// `the_fine_indexing_agrees_with_the_grid`.
///
/// Needed here no less than there. Every fixture in this module lays its amounts
/// out through this same private function, so a transposed copy is consistent with
/// itself and invisible from inside: `y + x*ny + z*nx*ny` is still a bijection of
/// the plane, the denominator summed over the whole domain is the same number, and
/// the cell merely gets the composition of somebody else's voxel.
#[inline(always)]
fn index(p: &TemperatureParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, Grid};

    /// The one fine grid of this module; the coarse grid it folds onto is
    /// `2 x 3 x 4`. Three pairwise different coarse extents, and none of them
    /// one: on a cubic grid every mistake that swaps two axes is invisible,
    /// because all three strides are equal, and the decode of the coarse index by
    /// the fine extents is masked besides. `nz = 16` rather than four, so that
    /// the coarse `z` is not identically zero and `z0 = cz` in place of
    /// `z0 = cz << lod` cannot stay green.
    const NX: u32 = 8;
    const NY: u32 = 12;
    const NZ: u32 = 16;
    const LOD: u32 = 2;

    const N_FINE: u32 = NX * NY * NZ;
    const N_COARSE: u32 = (NX >> LOD) * (NY >> LOD) * (NZ >> LOD);

    /// `2^(3*lod)`: how many fine voxels one coarse cell owns.
    const PER_COARSE: i64 = 1 << (3 * LOD);

    /// Three substances. `X` is the wide one and it is deliberately **not** the
    /// only one in lane 0: `lane[Y] == 0` as well, so a kernel addressing by
    /// substance index is right at `s == 0`, reads `X`'s buffer for `Y`, and is
    /// off by one for `Z` — the registry shape ADR-056 describes.
    const X: usize = 0;
    const Y: usize = 1;
    const Z: usize = 2;
    const N_SUBSTANCES: u32 = 3;

    /// `lane_of[s]`: `X` takes lane 0 of the wide field, `Y` and `Z` lanes 0 and 1
    /// of the narrow one.
    const LANES: [u32; N_SUBSTANCES as usize] = [0, 0, 1];
    const MASK: u32 = 1 << X;

    /// `c_p[s] * 2^-k[s]`, J/(K * unit), indexed by substance. Powers of two, so
    /// every product and every sum below is exact in `f32` and an assertion is
    /// about the kernel rather than about rounding.
    ///
    /// `Y` is the sulfate of this fixture: a **negative** coefficient, which is
    /// physics (ADR-044, and the header of `configs/scenarios/h2s-oxidation.toml`).
    const CAPACITY: [f64; N_SUBSTANCES as usize] = [0.5, -0.25, 4.0];

    /// How much of the solvent every fine voxel holds in the fixtures that probe
    /// the index mapping.
    ///
    /// Not decoration and not a round number for its own sake: a coarse cell whose
    /// composition sums to zero has no temperature at all, and this kernel says so
    /// with an assertion, so a fixture that put one unit into one voxel and left
    /// the rest of the domain empty would be measuring the refusal instead of the
    /// mapping. Water settles the corpus denominator by five hundred to one
    /// (ADR-062), and this stands for it.
    const SOLVENT: i64 = 2;

    /// What [`SOLVENT`] alone gives a coarse cell: `2^(3*lod) * 2 * 0.5`.
    const BASELINE: f64 = PER_COARSE as f64 * SOLVENT as f64 * CAPACITY[X];

    /// The scenario's zero of enthalpy storage, K. Exactly representable, and
    /// deliberately not 298.15: a `T_ref` that looked like the thermochemical
    /// reference would make the two indistinguishable in every assertion here.
    const T_REF: f64 = 256.0;

    /// `2^-k_E`: joules per storage unit of enthalpy. A power of two, so the
    /// crossing is exact.
    const JOULES_PER_UNIT: f64 = 1.0 / 1024.0;

    fn q(v: f64) -> Q {
        Q::from_f64(v)
    }

    fn params() -> TemperatureParams {
        TemperatureParams {
            lane_len: N_FINE,
            nx: NX,
            ny: NY,
            nz: NZ,
            lod: LOD,
            n_voxels: N_FINE,
            n_substances: N_SUBSTANCES,
            width_mask: MASK,
            t_ref: q(T_REF),
            joules_per_unit: q(JOULES_PER_UNIT),
        }
    }

    /// The two amount buffers of the fixture: one wide lane, two narrow ones.
    struct Amounts {
        narrow: Vec<M32>,
        wide: Vec<M64>,
    }

    impl Amounts {
        fn empty() -> Self {
            Amounts {
                narrow: vec![M32::ZERO; (2 * N_FINE) as usize],
                wide: vec![M64::ZERO; N_FINE as usize],
            }
        }

        /// Put an amount of substance `s` into one fine voxel, through the lane
        /// table — the one door from a substance to a buffer address (ADR-056).
        fn put(&mut self, p: &TemperatureParams, s: usize, x: u32, y: u32, z: u32, value: i64) {
            let at = (LANES[s] * N_FINE + index(p, x, y, z)) as usize;
            if MASK & (1 << s) != 0 {
                self.wide[at] = M64::new(value);
            } else {
                self.narrow[at] = M32::new(value as i32);
            }
        }

        /// The same amount of substance `s` in every fine voxel.
        fn fill(&mut self, p: &TemperatureParams, s: usize, value: i64) {
            for z in 0..NZ {
                for y in 0..NY {
                    for x in 0..NX {
                        self.put(p, s, x, y, z, value);
                    }
                }
            }
        }
    }

    /// The whole dispatch, the way the host runs it: one invocation per coarse
    /// cell.
    fn dispatch(
        amounts: &Amounts,
        enthalpy: &[M64],
        heat: &Heat,
        p: &TemperatureParams,
    ) -> (Vec<Q>, Vec<Q>) {
        let mut capacity = vec![Q::ZERO; N_COARSE as usize];
        let mut temperature = vec![Q::ZERO; N_COARSE as usize];
        for coarse in 0..N_COARSE {
            temperature_cell(
                &amounts.narrow,
                &amounts.wide,
                enthalpy,
                &mut capacity,
                &mut temperature,
                heat,
                p,
                coarse,
            );
        }
        (capacity, temperature)
    }

    fn tables() -> (Vec<u32>, Vec<Q>) {
        (LANES.to_vec(), CAPACITY.iter().map(|&c| q(c)).collect())
    }

    fn heat<'a>(lane: &'a [u32], capacity: &'a [Q]) -> Heat<'a> {
        Heat {
            lane,
            capacity_per_unit: capacity,
        }
    }

    fn cold() -> Vec<M64> {
        vec![M64::ZERO; N_COARSE as usize]
    }

    /// The coarse cell covering a fine voxel, per axis (SPEC section 1.5),
    /// written out here rather than taken from [`coarse_coords`].
    fn covering(x: u32, y: u32, z: u32) -> u32 {
        let cnx = NX >> LOD;
        let cny = NY >> LOD;
        (x >> LOD) + (y >> LOD) * cnx + (z >> LOD) * cnx * cny
    }

    #[test]
    fn the_denominator_gathers_the_per_axis_cube() {
        // SPEC section 1.5 written out by axis in the test rather than borrowed
        // from the code: for the coarse half of the mapping there is no authority
        // in `world/` to check against, because `Grid` knows nothing about `lod`.
        // This is the only defence against `fine_idx >> lod` and against
        // `coarse * 2^(3*lod) + f`. Both cut the index space into disjoint pieces,
        // so any "the sum came out right" check is green under them while the heat
        // capacity is credited to a stripe of sixty-four along X — and here not
        // even that check exists, because a temperature conserves nothing.
        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);

        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    // A solvent everywhere, because a cell whose composition sums
                    // to zero answers `T_ref` without dividing (ADR-079, and the
                    // test that pins it), which would make every cell of this
                    // fixture agree for the wrong reason. The probe on top of it
                    // is one unit of `Z`, whose coefficient is a positive power of
                    // two.
                    let mut amounts = Amounts::empty();
                    amounts.fill(&p, X, SOLVENT);
                    amounts.put(&p, Z, x, y, z, 1);

                    let (capacity_out, _) = dispatch(&amounts, &cold(), &h, &p);

                    let expected = covering(x, y, z);
                    for coarse in 0..N_COARSE {
                        let want = if coarse == expected {
                            q(BASELINE + CAPACITY[Z])
                        } else {
                            q(BASELINE)
                        };
                        assert_eq!(
                            capacity_out[coarse as usize], want,
                            "the unit at ({x}, {y}, {z}) landed wrong: coarse cell \
                             {coarse} should hold {want:?}, its covering cell is \
                             {expected}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_coarse_index_is_decoded_by_the_coarse_extents() {
        // The mirror of the mistake above: `cy = coarse / nx` instead of
        // `coarse / (nx >> lod)`. Partly masked on a cubic grid, which is why the
        // fixture's coarse grid is `2 x 3 x 4`. The symptom is a cell holding the
        // composition of somebody else's cube with every sum in sight adding up.
        let p = params();
        let cnx = NX >> LOD;
        let cny = NY >> LOD;
        let cnz = NZ >> LOD;

        for cz in 0..cnz {
            for cy in 0..cny {
                for cx in 0..cnx {
                    let coarse = cx + cy * cnx + cz * cnx * cny;
                    assert_eq!(coarse_coords(&p, coarse), (cx, cy, cz));
                }
            }
        }

        // And the composition really does follow the decode: one unit in the
        // origin voxel of each cube must reach that cube's cell and no other.
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);
        for cz in 0..cnz {
            for cy in 0..cny {
                for cx in 0..cnx {
                    let mut amounts = Amounts::empty();
                    amounts.fill(&p, X, SOLVENT);
                    amounts.put(&p, Z, cx << LOD, cy << LOD, cz << LOD, 1);
                    let (out, _) = dispatch(&amounts, &cold(), &h, &p);
                    let coarse = cx + cy * cnx + cz * cnx * cny;
                    assert_eq!(out[coarse as usize], q(BASELINE + CAPACITY[Z]));
                }
            }
        }
    }

    #[test]
    fn the_amounts_are_summed_as_integers_before_they_cross_into_q() {
        // The direct twin of `reaction_energy_is_summed_as_integers` in
        // `kernels/fold.rs`. `Q` carries twenty-four significant bits, so a
        // kernel that converted each of the sixty-four fine voxels separately and
        // added them with `qadd` loses the small ones into the large one, every
        // time, and the denominator it comes out with is entirely plausible.
        //
        // The fixture makes that concrete: one voxel of the cube holds `2^25` and
        // the other sixty-three hold one apiece. Summed as integers first, the
        // cell holds `2^25 + 63` and a single crossing rounds it to `2^25 + 64`;
        // converted one at a time, `2^25 + 1` rounds back to `2^25` at every step
        // and the sixty-three trace voxels contribute *nothing at all*.
        //
        // `2^25` and not the `2^60` a voxel of water raised to `e_r` may actually
        // hold: `2^(3*lod)` terms of that size do not fit the `i64` accumulator,
        // which is a real hole and is recorded as `TODO(capacity-sum-bound)` on
        // the kernel rather than papered over in a fixture.
        const HEAVY: i64 = 1 << 25;
        const TRACE: i64 = 1;

        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);

        let mut amounts = Amounts::empty();
        amounts.fill(&p, X, TRACE);
        for cz in 0..NZ >> LOD {
            for cy in 0..NY >> LOD {
                for cx in 0..NX >> LOD {
                    amounts.put(&p, X, cx << LOD, cy << LOD, cz << LOD, HEAVY);
                }
            }
        }

        let (out, _) = dispatch(&amounts, &cold(), &h, &p);

        // `CAPACITY[X]` is a power of two, so scaling commutes with the rounding
        // into `Q` and this expectation is the exact integer sum crossed once.
        let units = HEAVY + (PER_COARSE - 1) * TRACE;
        for coarse in 0..N_COARSE {
            assert_eq!(
                out[coarse as usize],
                Q::from_f64(units as f64 * CAPACITY[X]),
                "coarse cell {coarse} lost the trace voxels before the sum"
            );
        }

        // And the fixture really can tell the two apart: without the trace voxels
        // the answer differs, which is exactly what a per-voxel crossing produces.
        let mut heavy_only = Amounts::empty();
        for cz in 0..NZ >> LOD {
            for cy in 0..NY >> LOD {
                for cx in 0..NX >> LOD {
                    heavy_only.put(&p, X, cx << LOD, cy << LOD, cz << LOD, HEAVY);
                }
            }
        }
        let (bare, _) = dispatch(&heavy_only, &cold(), &h, &p);
        assert_ne!(
            bare[0], out[0],
            "the trace voxels made no difference, so this fixture cannot see a \
             per-voxel crossing into Q"
        );
    }

    #[test]
    fn a_negative_heat_capacity_is_summed_and_not_dropped() {
        // Aqueous sulfate declares about -293 J/(mol K), and that is physics
        // rather than a typo — `configs/scenarios/h2s-oxidation.toml` explains why
        // the solvent has to be present for it to be safe. `max(0, .)` over a term
        // here, or `abs()` over the folded coefficient on the host, gives a
        // positive and plausible denominator: 0.2% off on the corpus composition
        // and qualitatively wrong without the solvent. No invariant sees either.
        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);

        let mut amounts = Amounts::empty();
        amounts.fill(&p, X, 8); // +4.0 per voxel
        amounts.fill(&p, Y, 4); // -1.0 per voxel
        let (out, _) = dispatch(&amounts, &cold(), &h, &p);

        let expected = PER_COARSE as f64 * (8.0 * CAPACITY[X] + 4.0 * CAPACITY[Y]);
        assert!(expected > 0.0, "the fixture has to stay above zero overall");
        for coarse in 0..N_COARSE {
            assert_eq!(
                out[coarse as usize],
                q(expected),
                "coarse cell {coarse}: the negative term was dropped or made \
                 positive"
            );
        }

        // Stated the other way round as well, because an `abs()` on the host and a
        // `max(0, .)` in the kernel are two different edits with one symptom: more
        // of the negative substance has to give *less* capacity.
        let mut more = Amounts::empty();
        more.fill(&p, X, 8);
        more.fill(&p, Y, 8);
        let (lower, _) = dispatch(&more, &cold(), &h, &p);
        assert!(
            lower[0] < out[0],
            "doubling the substance with a negative c_p did not lower the \
             denominator"
        );
    }

    #[test]
    fn a_nonpositive_denominator_answers_the_storage_zero_and_divides_nothing() {
        // The answer of ADR-079, and **no `cfg` on it**: the whole point of that
        // record is that the two profiles agree. An earlier version of this file
        // asserted in debug and answered in release, so `cargo test --workspace` —
        // which is the whole of CI — never once executed the branch a real run
        // takes. A behaviour tested in a profile nobody ships is a behaviour
        // nobody tested.
        //
        // Without the branch: `qdiv` by zero is an infinity, `expect_finite` is a
        // `debug_assert` and compiles to nothing in release, `qpow(q10, inf)` is an
        // infinity, `xi` saturates at `i32::MAX` and is then trimmed by the
        // substrate actually present. From the outside that is "the reaction runs
        // at full speed", with both halves of the invariant closing exactly.
        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);

        // Both signs of the case, and they are two different worlds: "zero" is a
        // voxel with nothing in it at all, which every scenario with a layer has
        // above the layer (ADR-077); "negative" is a voxel that kept the sulfate
        // and lost the solvent, which is the composition ADR-044 warns about.
        for (name, fill) in [("negative", 16i64), ("zero", 0)] {
            let mut amounts = Amounts::empty();
            if fill > 0 {
                amounts.fill(&p, Y, fill);
            }
            // A large enthalpy, so that "it answered `T_ref`" cannot be confused
            // with "it divided a zero numerator".
            let enthalpy = vec![M64::new(1 << 20); N_COARSE as usize];
            let (out, temperature) = dispatch(&amounts, &enthalpy, &h, &p);
            for coarse in 0..N_COARSE {
                assert!(
                    out[coarse as usize] <= Q::ZERO,
                    "the {name} fixture is not the case this test is about"
                );
                assert_eq!(
                    temperature[coarse as usize],
                    q(T_REF),
                    "coarse cell {coarse} of the {name} fixture divided anyway"
                );
                assert!(temperature[coarse as usize].debug_f64().is_finite());
            }
        }
    }

    #[test]
    fn the_amounts_are_addressed_by_lane_and_never_by_substance_index() {
        // `s * n_voxels + idx` in place of `lane[s] * n_voxels + idx` reads a lane
        // belonging to some other substance. Nothing is lost, so no balance moves;
        // and on a registry whose wide substance stands first the wrong expression
        // is *right* at `s == 0`, which is why the fixture puts `Y` in lane 0 of
        // the other width class and `Z` one further along.
        //
        // The same statement `mixed_width_storage_matches_uniform_width_storage`
        // makes for the reaction kernel: the mixed-width registry and an
        // artificially uniform one have to give the same denominator.
        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);

        let mut mixed = Amounts::empty();
        mixed.fill(&p, X, 6);
        mixed.fill(&p, Y, 12);
        mixed.fill(&p, Z, 3);
        let (from_mixed, _) = dispatch(&mixed, &cold(), &h, &p);

        // `6*0.5 + 12*(-0.25) + 3*4 = 12` per voxel. A kernel reading
        // `capacity_per_unit[lane[s]]` instead of `capacity_per_unit[s]` gets
        // `6*0.5 + 12*0.5 + 3*(-0.25) = 8.25`; one addressing amounts by `s`
        // reads `X`'s wide lane for `Y` and lane 2 of the narrow buffer — which
        // does not exist in this fixture — for `Z`. Neither is visible in any
        // balance, so the expectation is written out as a number.
        for coarse in 0..N_COARSE {
            assert_eq!(from_mixed[coarse as usize], q(PER_COARSE as f64 * 12.0));
        }

        // The same three substances, all narrow, `lane == s` throughout: three
        // lanes of the narrow buffer and an empty wide one.
        let uniform_p = TemperatureParams { width_mask: 0, ..p };
        let uniform_lanes = [0u32, 1, 2];
        let uniform_heat = heat(&uniform_lanes, &capacity);
        let mut uniform = Amounts {
            narrow: vec![M32::ZERO; (3 * N_FINE) as usize],
            wide: Vec::new(),
        };
        for (s, value) in [(0usize, 6i64), (1, 12), (2, 3)] {
            for idx in 0..N_FINE {
                uniform.narrow[(s as u32 * N_FINE + idx) as usize] = M32::new(value as i32);
            }
        }
        let mut capacity_out = vec![Q::ZERO; N_COARSE as usize];
        let mut temperature_out = vec![Q::ZERO; N_COARSE as usize];
        for coarse in 0..N_COARSE {
            temperature_cell(
                &uniform.narrow,
                &uniform.wide,
                &cold(),
                &mut capacity_out,
                &mut temperature_out,
                &uniform_heat,
                &uniform_p,
                coarse,
            );
        }
        assert_eq!(from_mixed, capacity_out);

        // And the second half: `capacity_per_unit` is indexed by **substance**.
        // Permuted into lane order it moves a contribution by `2^(k_i - k_j)` with
        // every balance closing, so the fixture's coefficients are chosen far
        // enough apart that the permutation cannot land on the same number.
        let by_lane: Vec<Q> = vec![q(CAPACITY[X]), q(CAPACITY[Z]), q(CAPACITY[Y])];
        let wrong = heat(&lanes, &by_lane);
        let (from_wrong, _) = dispatch(&mixed, &cold(), &wrong, &p);
        assert_ne!(
            from_mixed, from_wrong,
            "a table indexed by lane gave the same answer, so this fixture cannot \
             tell the two conventions apart"
        );
    }

    #[test]
    fn the_fine_indexing_agrees_with_the_grid() {
        // The kernel carries its own copy of SPEC section 1.1, because `kernels/`
        // may not depend on `world/`. The same test and the same reason as
        // `the_neighbourhood_agrees_with_the_grid` in `diffuse.rs` and
        // `the_fine_indexing_agrees_with_the_grid` in `fold.rs`: a test may depend
        // on everything.
        //
        // Needed here as much as there. Every fixture in this module lays its
        // amounts out through this same private function, so a transposed copy is
        // self-consistent, the denominator summed over the domain is unchanged,
        // and the cell simply gets somebody else's composition.
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
    fn a_coarse_cell_writes_only_itself() {
        // The operational form of ADR-034 and of the atomic ADR-045 rejected:
        // perturbing one fine amount moves exactly one coarse cell, in **both**
        // output buffers, and the cell next door does not move by a bit.
        const PX: u32 = 6;
        const PY: u32 = 5;
        const PZ: u32 = 2;

        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);
        let enthalpy: Vec<M64> = (0..N_COARSE)
            .map(|c| M64::new(i64::from(c) * 100_003 - 7))
            .collect();

        let mut amounts = Amounts::empty();
        amounts.fill(&p, X, 64);
        amounts.fill(&p, Z, 8);
        let (capacity_before, temperature_before) = dispatch(&amounts, &enthalpy, &h, &p);

        amounts.put(&p, Z, PX, PY, PZ, 8 + 4096);
        let (capacity_after, temperature_after) = dispatch(&amounts, &enthalpy, &h, &p);

        let moved = covering(PX, PY, PZ);
        for coarse in 0..N_COARSE {
            let at = coarse as usize;
            if coarse == moved {
                assert_ne!(
                    capacity_before[at], capacity_after[at],
                    "the perturbation missed its own cell"
                );
                assert_ne!(temperature_before[at], temperature_after[at]);
            } else {
                assert_eq!(
                    capacity_before[at], capacity_after[at],
                    "coarse cell {coarse} moved in the capacity buffer"
                );
                assert_eq!(
                    temperature_before[at], temperature_after[at],
                    "coarse cell {coarse} moved in the temperature buffer"
                );
            }
        }
    }

    #[test]
    fn the_result_does_not_depend_on_the_traversal_order() {
        // What the gather form buys: an invocation reads only inputs and writes
        // only its own two cells. The same statement `fold.rs` makes, and for the
        // same reason — on the GPU there is no traversal order at all, so anything
        // this test could catch would be unfixable there.
        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);
        let enthalpy: Vec<M64> = (0..N_COARSE)
            .map(|c| M64::new(i64::from(c) * 500_009 + 11))
            .collect();
        let mut amounts = Amounts::empty();
        amounts.fill(&p, X, 40);
        amounts.fill(&p, Y, 3);

        let mut forward_capacity = vec![Q::ZERO; N_COARSE as usize];
        let mut forward_temperature = vec![Q::ZERO; N_COARSE as usize];
        for coarse in 0..N_COARSE {
            temperature_cell(
                &amounts.narrow,
                &amounts.wide,
                &enthalpy,
                &mut forward_capacity,
                &mut forward_temperature,
                &h,
                &p,
                coarse,
            );
        }

        let mut backward_capacity = vec![Q::ZERO; N_COARSE as usize];
        let mut backward_temperature = vec![Q::ZERO; N_COARSE as usize];
        for coarse in (0..N_COARSE).rev() {
            temperature_cell(
                &amounts.narrow,
                &amounts.wide,
                &enthalpy,
                &mut backward_capacity,
                &mut backward_temperature,
                &h,
                &p,
                coarse,
            );
        }

        assert_eq!(forward_capacity, backward_capacity);
        assert_eq!(forward_temperature, backward_temperature);
    }

    #[test]
    fn the_offset_is_the_storage_zero_and_never_a_reaction_reference() {
        // At `H = 0` the temperature is exactly `T_ref` and nothing else. The
        // other half of the claim is structural and lives in the signatures:
        // `t_vmax` is not a parameter of this kernel, and `T_ref` is not a
        // parameter of `kernels/react.rs` — ADR-048 calls substituting one for the
        // other "not a saving of one key but a mistake", and now that `T_ref`
        // exists in this directory at all, the two absences are one grep apart.
        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);
        let mut amounts = Amounts::empty();
        amounts.fill(&p, X, 64);

        let (_, temperature) = dispatch(&amounts, &cold(), &h, &p);
        for coarse in 0..N_COARSE {
            assert_eq!(
                temperature[coarse as usize],
                q(T_REF),
                "coarse cell {coarse} at zero enthalpy is not at the storage zero"
            );
        }

        // And it is an offset rather than a scale: the same enthalpy under a
        // different `T_ref` moves every cell by exactly the difference.
        let enthalpy = vec![M64::new(1 << 16); N_COARSE as usize];
        let (_, warm) = dispatch(&amounts, &enthalpy, &h, &p);
        let shifted = TemperatureParams {
            t_ref: q(T_REF + 32.0),
            ..p
        };
        let (_, warm_shifted) = dispatch(&amounts, &enthalpy, &h, &shifted);
        for coarse in 0..N_COARSE {
            assert_eq!(
                warm_shifted[coarse as usize].debug_f64() - warm[coarse as usize].debug_f64(),
                32.0
            );
        }
    }

    #[test]
    fn the_denominator_is_recomputed_and_not_cached() {
        // ADR-044 word for word: "the denominator depends on the composition of
        // the voxel, so it is recomputed rather than cached". The failing
        // implementation is not hypothetical — `Derived::energy().c_cell` is a
        // heat capacity folded once at load out of `typical_conc`, it exists for
        // the derivation of `k_E` and for nothing else, and it is the most
        // plausible thing in the project to reuse here. Under it the temperature
        // stops depending on what the voxels hold, which no invariant can see.
        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);
        let enthalpy = vec![M64::new(1 << 18); N_COARSE as usize];

        let mut amounts = Amounts::empty();
        amounts.fill(&p, X, 64);
        let (first_capacity, first_temperature) = dispatch(&amounts, &enthalpy, &h, &p);

        // The composition moves, as it does once per tick when the chemistry is
        // applied (ADR-041), and the second application has to reflect it.
        amounts.fill(&p, X, 128);
        let (second_capacity, second_temperature) = dispatch(&amounts, &enthalpy, &h, &p);

        for coarse in 0..N_COARSE {
            let at = coarse as usize;
            assert_ne!(
                first_capacity[at], second_capacity[at],
                "coarse cell {coarse} kept the capacity of the previous \
                 composition"
            );
            assert_ne!(first_temperature[at], second_temperature[at]);
        }

        // Twice the capacity at the same enthalpy is half the excess over `T_ref`.
        for coarse in 0..N_COARSE {
            let at = coarse as usize;
            let first = first_temperature[at].debug_f64() - T_REF;
            let second = second_temperature[at].debug_f64() - T_REF;
            assert_eq!(second * 2.0, first);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "dispatched over coarse cells")]
    fn a_dispatch_over_fine_voxels_is_refused_rather_than_reading_past_the_field() {
        // The loud half of the dispatch contract, on the precedent of
        // `fold_energy`. The quiet half — a host that dispatches over coarse cells
        // and decodes the index by the fine extents — no assertion over values can
        // see; against that there is only the decode by the coarse extents and
        // `the_coarse_index_is_decoded_by_the_coarse_extents`.
        let p = params();
        let (lanes, capacity) = tables();
        let h = heat(&lanes, &capacity);
        let amounts = Amounts::empty();
        let mut capacity_out = vec![Q::ZERO; N_COARSE as usize];
        let mut temperature_out = vec![Q::ZERO; N_COARSE as usize];

        temperature_cell(
            &amounts.narrow,
            &amounts.wide,
            &cold(),
            &mut capacity_out,
            &mut temperature_out,
            &h,
            &p,
            N_COARSE,
        );
    }
}
