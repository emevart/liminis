//! Acceptance criteria of the reaction kernel (`ACCEPTANCE.md`, S0).
//!
//! Thirteen names, and the document fixes them — the stage is accepted by these
//! and not by a reading of the code:
//!
//! ```text
//! reaction_alone_conserves_each_element_exactly
//! competition_scaling_conserves_each_element_exactly
//! all_reactions_share_one_competition_coefficient
//! reaction_result_is_independent_of_order_in_toml
//! mixed_width_storage_matches_uniform_width_storage
//! a_negative_pool_caps_the_extent_at_zero
//! a_negative_pool_caps_the_competition_coefficient_at_zero
//! abiotic_reaction_proceeds_with_empty_catalyst
//! abiotic_rate_is_independent_of_every_catalyst_field
//! catalyzed_rate_equals_abiotic_rate_times_catalyst_concentration
//! the_matter_residual_closes_across_a_tick_with_chemistry_in_it
//! a_reaction_written_to_the_wrong_lane_breaks_the_matter_residual
//! a_dropped_reaction_write_breaks_the_matter_residual
//! ```
//!
//! The last three arrived with ADR-080 and they are the first tests in this file
//! that judge the chemistry through the **ledger** rather than through the
//! amounts. That is a different instrument: the ten above compare the kernel's
//! output against arithmetic done in the test, and these three compare two
//! independent witnesses of the same tick — the fields, and the kernel's own
//! report of how far each reaction ran.
//!
//! They live in an integration test rather than beside the kernel on purpose,
//! and `tests/acceptance_diffusion.rs` states the reason: an acceptance test is
//! the outside view, may use only what a scenario can use — the public surface
//! of `liminis-core` — and so keeps its meaning when the inside is rearranged.
//!
//! Three of these names are load-bearing in a way that is easy to miss, because
//! **every failure they catch keeps the ledger closing**. Matter is conserved by
//! the stoichiometric vector, not by anything this kernel decides (ADR-027), so
//! a kernel that read the wrong lane, applied the reactions one after another,
//! or scaled only the consumers of an oversold pool balances exactly as well as
//! a correct one:
//!
//! - `mixed_width_storage_matches_uniform_width_storage` (ADR-056) is the only
//!   thing that sees `s * n_voxels + idx` in place of `lane[s] * n_voxels + idx`.
//!   Nothing is lost — the wrong thing is read;
//! - `reaction_result_is_independent_of_order_in_toml` (ADR-041) is the only
//!   thing that sees availability recomputed after each reaction. What changes
//!   is who ate the substrate first;
//! - `all_reactions_share_one_competition_coefficient` (ADR-041, ADR-068) is the
//!   only thing that separates "one coefficient per voxel" from "a plausible
//!   coefficient". Both conserve every element exactly.
//!
//! # The fixture
//!
//! Six substances over five conserved quantities, and one of them 64-bit, so
//! that lanes and substance indices cannot coincide (ADR-056). The chemistry is
//! invented for this file — no scenario carries reactions yet — but its shape is
//! the shape of the corpus: integer molar coefficients, storage coefficients
//! `nu_i = s_i * 2^(k_i - e_r)` (ADR-039), enthalpy as a participant of the
//! stoichiometry vector at a reserved index (ADR-041).

use liminis_core::config;
use liminis_core::kernels::fold::{FoldParams, fold_energy};
use liminis_core::kernels::react::{NO_CATALYST, R_MAX, ReactParams, Rx, react_voxel};
use liminis_core::kernels::temperature::{Heat, TemperatureParams, temperature_cell};
use liminis_core::ledger::{Channel, DomainSums, Ledger, Nu};
use liminis_core::numeric::{M32, M64, Q, run_key};
use liminis_core::world::Width;

/// A deliberately non-cubic grid, small enough to run whole in a test and large
/// enough that a mistaken index lands somewhere visible.
const NX: u32 = 3;
const NY: u32 = 4;
const NZ: u32 = 2;
const N_VOXELS: u32 = NX * NY * NZ;

/// Substance indices of the fixture.
const A: u32 = 0;
const B: u32 = 1;
const C: u32 = 2;
const D: u32 = 3;
const E: u32 = 4;
const F: u32 = 5;
const N_SUBSTANCES: u32 = 6;

/// The reserved index of enthalpy in the stoichiometry vector (ADR-041). It is
/// past the last substance, which is what `world::Registry::MAX_SUBSTANCES`
/// exists to guarantee: an energy index inside the substance range lands the
/// enthalpy delta in the lane of a substance, and the *matter* ledger still
/// closes, because matter and energy are counted apart (ADR-028).
const S_ENERGY: u32 = N_SUBSTANCES;

/// The conserved quantities of `CLAUDE.md` and ADR-064, in their usual order.
const N_ELEMENTS: usize = 5;

/// `composition[s][e]`: atoms of conserved quantity `e` per formula unit of
/// substance `s`, in the order C, N, P, S, Fe.
///
/// The reactions below balance on it, and `reaction_alone_conserves_each_element_exactly`
/// checks that they still balance after the kernel has applied them in storage
/// units — which is the whole question, since the storage coefficients differ
/// from the molar ones by a per-substance power of two (ADR-039).
const COMPOSITION: [[i64; N_ELEMENTS]; N_SUBSTANCES as usize] = [
    [1, 0, 0, 0, 0], // A: one carbon
    [0, 1, 0, 0, 0], // B: one nitrogen
    [1, 2, 0, 0, 0], // C: A + 2 B
    [0, 0, 1, 0, 0], // D: one phosphorus
    [0, 2, 1, 0, 0], // E: 2 B + D
    [1, 0, 1, 0, 0], // F: A + D
];

/// `log2(units_per_mol)` per substance (ADR-039). Deliberately not all equal:
/// `nu_i = s_i * 2^(k_i - e_r)`, so a fixture with one scale would let a kernel
/// that ignored the exponent pass.
const K: [u32; N_SUBSTANCES as usize] = [8, 6, 6, 6, 6, 6];

/// The extent exponent, one per reaction and the same for all three here
/// (ADR-039). Every `k_i` above is at or above it, which is the condition
/// ADR-039 makes the validator check.
const E_R: u32 = 4;

/// One reaction as a scenario would write it: molar coefficients, signed, net
/// per substance (`config/derive.rs` folds a substance standing on both sides
/// into one entry).
struct Recipe {
    /// The identifier the host folded from the reaction's **name**, never from
    /// its position in the file (ADR-027). The fixture keeps it attached to the
    /// recipe for that reason: reordering the recipes must carry it along.
    rid: u32,
    /// `(substance, molar coefficient)`, inputs negative.
    molar: &'static [(u32, i64)],
    /// The molar enthalpy coefficient of the reaction, in the direction of the
    /// **field** (ADR-081): positive means the reaction gives heat up, so an
    /// exothermic record looks like an output from inside the vector. The three
    /// recipes below are exothermic, and their numbers are `-Sum_s s_s*w_s` over
    /// the weights in [`W`] — see
    /// `the_fixture_weights_agree_with_its_energy_coefficients`.
    molar_energy: i64,
    /// Turnovers per second per cubic metre, or per mole of catalyst when
    /// `catalyst` is set (ADR-063).
    vmax: f64,
    /// The reference temperature of the Q10 factor — the reaction's own, never
    /// the scenario's zero of enthalpy storage (ADR-048).
    t_vmax: f64,
    /// The catalysis column, or [`NO_CATALYST`] (ADR-063).
    catalyst: u32,
}

/// The flat tables of ADR-041, plus the two later columns: `lane` (ADR-056) and
/// `cat` (ADR-063). Owned here so that `Rx` can borrow them.
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

/// `k_i - e_r`: how many binary orders separate the molar coefficient from the
/// storage one for substance `s`.
fn shift(s: u32) -> u32 {
    K[s as usize] - E_R
}

/// The storage coefficient of ADR-039, exactly.
fn nu_of(s: u32, molar: i64) -> i32 {
    let scaled = molar << shift(s);
    i32::try_from(scaled).expect("the fixture keeps nu inside an i32 (ADR-039)")
}

/// Build the tables for a set of recipes, over a given lane assignment.
///
/// `lane_of` is passed in rather than derived so that
/// `mixed_width_storage_matches_uniform_width_storage` can hand the same
/// chemistry two different registries.
fn tables(recipes: &[Recipe], lane_of: &[u32], km: f64) -> Tables {
    let mut t = Tables {
        lane: lane_of.to_vec(),
        ..Default::default()
    };

    for s in 0..N_SUBSTANCES {
        // One storage unit as a concentration. The fixture makes it a power of
        // two so that nothing in the kinetics rounds by accident.
        t.conc_per_unit
            .push(q(1.0 / f64::from(1u32 << K[s as usize])));
    }

    for recipe in recipes {
        t.begin.push(t.nu.len() as u32);
        for &(s, molar) in recipe.molar {
            t.nu.push(nu_of(s, molar));
            t.nu_sub.push(s);
            // Parallel to `nu`, one entry per participant, meaningful only where
            // `nu < 0`. Zero means "no half-saturation", which keeps the
            // Michaelis term at exactly one and the extent exactly predictable —
            // which is what every test here wants except
            // `mixed_width_storage_matches_uniform_width_storage`, where a term
            // of exactly one would cancel `conc_per_unit` out of the kinetics
            // and hide the second half of what that test is for.
            t.km.push(q(km));
        }
        // Enthalpy travels in the same vector as the substances (ADR-041), with
        // a `km` entry of its own so that the two tables stay parallel.
        t.nu.push(
            i32::try_from(recipe.molar_energy << shift(A))
                .expect("the fixture keeps nu_E inside an i32"),
        );
        t.nu_sub.push(S_ENERGY);
        t.km.push(Q::ZERO);

        t.len.push(t.nu.len() as u32 - t.begin[t.begin.len() - 1]);
        t.e_r.push(E_R);
        t.cat.push(recipe.catalyst);
        t.rid.push(recipe.rid);
        t.vmax.push(q(recipe.vmax));
        // One flat value: at `T == t_vmax` the factor is one whatever it is, and
        // `the_q10_factor_is_measured_from_t_vmax` beside the kernel is what
        // exercises the other case.
        t.q10.push(q(2.0));
        t.t_vmax.push(q(recipe.t_vmax));
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

    /// The four tables the residual multiplies the reduced extent by (ADR-080).
    ///
    /// The same borrows `Rx` hands the kernel and never a second copy: the
    /// coefficient the ledger multiplies `Xi_r` by has to be the coefficient the
    /// kernel applied, and here that is the storage `nu`, not the molar `s` — the
    /// two differ by `2^(k_i - e_r)`, which is a factor of four on `A` in this
    /// fixture and exactly one on the other five.
    fn nu(&self) -> Nu<'_> {
        Nu {
            nu: &self.nu,
            nu_sub: &self.nu_sub,
            begin: &self.begin,
            len: &self.len,
        }
    }
}

fn q(v: f64) -> Q {
    Q::from_f64(v)
}

/// The mixed-width registry: `A` is the wide substance, so lanes and substance
/// indices disagree from index 1 onwards — the shape of the registry the project
/// actually carries, where water stands first and takes lane 0 (ADR-056).
const MIXED_LANES: [u32; N_SUBSTANCES as usize + 1] = [0, 0, 1, 2, 3, 4, 99];
const MIXED_MASK: u32 = 1 << A;

/// The same six substances, all narrow. Here `lane == s`, which is the one
/// registry where the wrong addressing is right.
const UNIFORM_LANES: [u32; N_SUBSTANCES as usize + 1] = [0, 1, 2, 3, 4, 5, 99];

/// The world as the kernel sees it: two amount buffers, an energy accumulator,
/// a coarse temperature field and one catalysis column.
struct World {
    src32: Vec<M32>,
    src64: Vec<M64>,
    dst32: Vec<M32>,
    dst64: Vec<M64>,
    energy: Vec<M64>,
    /// The extent report of ADR-080: `n_voxels * n_reactions` cells, voxel-major.
    /// Sized for `R_MAX` so that one fixture serves every recipe below; the
    /// **stride** is always the scenario's `n_reactions`, which is what the
    /// kernel writes on and what [`World::xi`] reads with.
    xi: Vec<M32>,
    temperature: Vec<Q>,
    catalyst: Vec<Q>,
    lanes: [u32; N_SUBSTANCES as usize + 1],
    mask: u32,
}

impl World {
    /// `amounts[s]` in every voxel of the domain.
    fn uniform(
        amounts: [i64; N_SUBSTANCES as usize],
        lanes: [u32; N_SUBSTANCES as usize + 1],
        mask: u32,
    ) -> Self {
        let n64 = (0..N_SUBSTANCES).filter(|s| mask & (1 << s) != 0).count() as u32;
        let n32 = N_SUBSTANCES - n64;

        let mut world = World {
            src32: vec![M32::ZERO; (n32 * N_VOXELS) as usize],
            src64: vec![M64::ZERO; (n64 * N_VOXELS) as usize],
            dst32: vec![M32::ZERO; (n32 * N_VOXELS) as usize],
            dst64: vec![M64::ZERO; (n64 * N_VOXELS) as usize],
            energy: vec![M64::ZERO; N_VOXELS as usize],
            xi: vec![M32::ZERO; N_VOXELS as usize * R_MAX],
            // One coarse cell at lod 0 per fine voxel: the mapping of a fine
            // index to a coarse one is exercised beside the kernel, and putting
            // a coarse grid here would only test the fixture.
            temperature: vec![q(300.0); N_VOXELS as usize],
            catalyst: vec![q(3.0); N_VOXELS as usize],
            lanes,
            mask,
        };
        for s in 0..N_SUBSTANCES {
            for idx in 0..N_VOXELS {
                world.put(s, idx, amounts[s as usize]);
            }
        }
        world
    }

    fn at(&self, s: u32, idx: u32) -> usize {
        (self.lanes[s as usize] * N_VOXELS + idx) as usize
    }

    fn put(&mut self, s: u32, idx: u32, value: i64) {
        let at = self.at(s, idx);
        if self.mask & (1 << s) != 0 {
            self.src64[at] = M64::new(value);
        } else {
            self.src32[at] = M32::new(i32::try_from(value).unwrap());
        }
    }

    fn put_everywhere(&mut self, s: u32, value: i64) {
        for idx in 0..N_VOXELS {
            self.put(s, idx, value);
        }
    }

    fn before(&self, s: u32, idx: u32) -> i64 {
        let at = self.at(s, idx);
        if self.mask & (1 << s) != 0 {
            self.src64[at].to_i64()
        } else {
            self.src32[at].to_i64()
        }
    }

    fn after(&self, s: u32, idx: u32) -> i64 {
        let at = self.at(s, idx);
        if self.mask & (1 << s) != 0 {
            self.dst64[at].to_i64()
        } else {
            self.dst32[at].to_i64()
        }
    }

    fn delta(&self, s: u32, idx: u32) -> i64 {
        self.after(s, idx) - self.before(s, idx)
    }

    /// The whole domain, the way a host runs one step of reactions.
    fn run(&mut self, rx: &Rx, p: &ReactParams) {
        for idx in 0..N_VOXELS {
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
    }

    /// The extent reaction `r` reported in voxel `idx` (ADR-080).
    ///
    /// Off the report, not off the amounts. The whole content of the record is
    /// that these are two independent witnesses, so a helper that derived one
    /// from the other would make every test below an identity.
    fn xi(&self, p: &ReactParams, r: usize, idx: u32) -> i64 {
        self.xi[(idx as usize) * (p.n_reactions as usize) + r].to_i64()
    }

    /// What the host reduces in phase 5 LEDGER: `Xi_r` over the whole domain.
    fn xi_total(&self, p: &ReactParams, r: usize) -> i128 {
        (0..N_VOXELS)
            .map(|idx| i128::from(self.xi(p, r, idx)))
            .sum()
    }
}

fn params() -> ReactParams {
    ReactParams {
        lane_len: N_VOXELS,
        nx: NX,
        ny: NY,
        n_voxels: N_VOXELS,
        n_substances: N_SUBSTANCES,
        n_reactions: 0,
        tick: 7,
        run_key: run_key(42),
        width_mask: MIXED_MASK,
        s_energy: S_ENERGY,
        lod: 0,
        cnx: NX,
        cny: NY,
        volume: Q::ONE,
        dt: Q::ONE,
    }
}

/// `A + 2 B -> C`, and `x = vmax * dt * V * 2^e_r = 0.25 * 16 = 4` quanta —
/// whole, so the stochastic draw of ADR-027 cannot change the answer and the
/// tests below can name the extent.
fn r1() -> Recipe {
    Recipe {
        rid: 0x5eed_0001,
        molar: &[(A, -1), (B, -2), (C, 1)],
        molar_energy: 2,
        vmax: 0.25,
        t_vmax: 300.0,
        catalyst: NO_CATALYST,
    }
}

/// `2 B + D -> E`, the second claimant on `B`.
fn r2() -> Recipe {
    Recipe {
        rid: 0x5eed_0002,
        molar: &[(B, -2), (D, -1), (E, 1)],
        molar_energy: 12,
        vmax: 0.25,
        t_vmax: 300.0,
        catalyst: NO_CATALYST,
    }
}

/// `A + D -> F`: touches neither of the two claimants' scarce substrate and
/// stands on abundant pools. Twice the `vmax` of the other two, so that
/// `all_reactions_share_one_competition_coefficient` compares a ratio rather
/// than two equal numbers.
fn r3() -> Recipe {
    Recipe {
        rid: 0x5eed_0003,
        molar: &[(A, -1), (D, -1), (F, 1)],
        molar_energy: 4,
        vmax: 0.5,
        t_vmax: 300.0,
        catalyst: NO_CATALYST,
    }
}

/// The extent applied to reaction `r`, read back out of the amounts.
///
/// Read rather than returned: the kernel writes state, not diagnostics, and a
/// test that asked it for `xi` would be testing a different function. Each
/// recipe here has a product no other recipe touches, which is what makes the
/// division exact.
fn extent_of(world: &World, product: u32, idx: u32) -> i64 {
    let nu = i64::from(nu_of(product, 1));
    let delta = world.delta(product, idx);
    assert_eq!(delta % nu, 0, "a product moved by a non-multiple of its nu");
    delta / nu
}

/// The per-element imbalance of one voxel, in moles.
///
/// `delta_s >> (k_s - e_r)` is exact by construction, because `nu_s` is
/// `s_s * 2^(k_s - e_r)` and every change is `nu_s * xi` (ADR-039, ADR-027). A
/// remainder here would mean a coefficient was rounded at run time.
fn element_residual(world: &World, element: usize, idx: u32) -> i64 {
    let mut total = 0i64;
    for s in 0..N_SUBSTANCES {
        let delta = world.delta(s, idx);
        let scale = 1i64 << shift(s);
        assert_eq!(
            delta % scale,
            0,
            "substance {s} moved by {delta}, which is not a whole number of \
             molar units at 2^{}: a coefficient was rounded at run time",
            shift(s)
        );
        total += (delta / scale) * COMPOSITION[s as usize][element];
    }
    total
}

fn assert_every_element_balances(world: &World) {
    for idx in 0..N_VOXELS {
        for element in 0..N_ELEMENTS {
            assert_eq!(
                element_residual(world, element, idx),
                0,
                "conserved quantity {element} does not balance in voxel {idx}"
            );
        }
    }
}

fn assert_something_happened(world: &World) {
    let moved = (0..N_SUBSTANCES).any(|s| (0..N_VOXELS).any(|idx| world.delta(s, idx) != 0));
    assert!(
        moved,
        "nothing moved at all: a kernel that does nothing conserves everything"
    );
}

// ---------------------------------------------------------------------------
// The matter residual with chemistry in it (ADR-080)
// ---------------------------------------------------------------------------

/// The left side of the matter invariant over one of the fixture's two states.
///
/// `after` picks the write buffer over the read one. A residual is
/// `after - before` over one tick, and this fixture holds both states at once,
/// which is what lets a whole tick be judged without a `Tick`.
///
/// Through the lane table and never through the substance index: the same wrong
/// mapping applied on both sides gives a residual of exactly zero for ever
/// (ADR-056), which is the failure `Tick::domain_sums` has its own paragraph
/// about.
fn domain_sums(world: &World, after: bool) -> DomainSums {
    let mut sums = DomainSums::new(N_SUBSTANCES).unwrap();
    for s in 0..N_SUBSTANCES {
        let at = (world.lanes[s as usize] * N_VOXELS) as usize;
        let span = at..at + N_VOXELS as usize;
        if world.mask & (1 << s) != 0 {
            sums.add_field_lane_64(
                s,
                if after {
                    &world.dst64[span]
                } else {
                    &world.src64[span]
                },
            );
        } else {
            sums.add_field_lane_32(
                s,
                if after {
                    &world.dst32[span]
                } else {
                    &world.src32[span]
                },
            );
        }
    }
    sums
}

/// The three-reaction fixture every test in this section runs, plus the state it
/// starts from.
///
/// Three reactions rather than one, and two of them competing for `B`: with a
/// single reaction the extent term is one product of two numbers, and a residual
/// that closes says nothing about whether the term is indexed by reaction at all.
fn reacting_tick() -> (Tables, ReactParams, World) {
    let t = tables(&[r1(), r2(), r3()], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 3;
    let world = World::uniform([100_000, 1_000, 0, 100_000, 0, 0], MIXED_LANES, MIXED_MASK);
    (t, p, world)
}

/// Write a value into the **write** buffer, which is where a corruption of the
/// kernel's output has to land.
fn poke_dst(world: &mut World, s: u32, idx: u32, value: i64) {
    let at = world.at(s, idx);
    if world.mask & (1 << s) != 0 {
        world.dst64[at] = M64::new(value);
    } else {
        world.dst32[at] = M32::new(i32::try_from(value).unwrap());
    }
}

/// `ACCEPTANCE.md`, section "Conservation" (ADR-080).
///
/// The identity chemistry actually satisfies, end to end:
/// `Delta n_s == Sum_c credited(c, s) + Sum_r nu_(r,s) * Xi_r`, with the left
/// side summed off the fields and `Xi_r` reduced off the slice the kernel wrote.
/// Before this record `Ledger::assert_closed` panicked on the first tick with any
/// reaction in it, which is the matter half of the lock on step `h`.
///
/// Three things are asserted and the second is the one that keeps the first
/// honest.
///
/// (1) the residual is an exact zero for every substance — not a tolerance;
/// (2) **without the extent term the same tick does not close.** A ledger with no
///     room for an extent is off by the whole of what the chemistry moved, which
///     is what makes the zero above evidence of a term rather than of a quiet
///     tick;
/// (3) not one channel counter moved. `Xi` is not a channel and the registry did
///     not grow a seventh name (ADR-059, ADR-080): the domain is closed, and
///     `a_closed_domain_leaves_every_channel_counter_at_zero` stays true on a
///     domain where chemistry is running.
///
/// What this cannot see is written down where it belongs, on
/// `Ledger::residual_matter`: the extent is credited with the same `xi` that
/// applied `Delta n = nu * xi`, so a wrong, negative or unbounded `xi` cancels
/// between the two sides and is invisible here for ever. That class is
/// `a_negative_pool_caps_the_extent_at_zero` and its neighbours, and it is not
/// the ledger's.
#[test]
fn the_matter_residual_closes_across_a_tick_with_chemistry_in_it() {
    let (t, p, mut world) = reacting_tick();
    let before = domain_sums(&world, false);
    world.run(&t.rx(), &p);
    let after = domain_sums(&world, true);

    assert_something_happened(&world);
    // The substances genuinely convert, which is the reason a per-substance
    // residual needed a second term at all.
    assert_ne!(
        after.matter(C),
        before.matter(C),
        "no reaction ran, so the residual below closes on 0 == 0"
    );

    let mut ledger = Ledger::with_reactions(N_SUBSTANCES, p.n_reactions).unwrap();
    ledger.begin_tick();
    ledger.reduce_extent(&world.xi, N_VOXELS);

    for r in 0..p.n_reactions as usize {
        assert_eq!(
            ledger.extent(r as u32),
            world.xi_total(&p, r),
            "the reduction of reaction {r} is not the sum of its slice"
        );
    }
    assert!(
        ledger.extent(0) > 0,
        "reaction 0 did not run, so its term is zero whatever the code does"
    );

    for s in 0..N_SUBSTANCES {
        assert_eq!(
            ledger.residual_matter(t.nu(), s, &before, &after),
            0,
            "substance {s} does not close"
        );
    }
    ledger.assert_closed(t.nu(), &before, &after);

    // (2) The same tick judged without the term. `Ledger::new` leaves the extent
    //     table empty, so `Sum_r nu * Xi` is identically zero — which is exactly
    //     the state of the ledger before ADR-080, and exactly what made step `h`
    //     undispatchable.
    let mut blind = Ledger::new(N_SUBSTANCES).unwrap();
    blind.begin_tick();
    let broken: Vec<i128> = (0..N_SUBSTANCES)
        .map(|s| blind.residual_matter(Nu::EMPTY, s, &before, &after))
        .collect();
    assert!(
        broken.iter().any(|&r| r != 0),
        "a ledger with no extent term closed a reacting tick: {broken:?}"
    );

    // (3) Not a channel.
    for channel in Channel::ALL {
        for s in 0..N_SUBSTANCES {
            assert_eq!(
                ledger.matter(channel, s),
                0,
                "{} moved for substance {s} on a closed domain",
                channel.name()
            );
        }
    }
}

/// `ACCEPTANCE.md`, section "Conservation" (ADR-080).
///
/// One of the two load-bearing names of the record, and this is the one that
/// answers the variant it rejected. A residual taken over the **conserved
/// quantities** instead of over the substances cannot see this at all on a
/// registry whose composition matrix has a kernel — and the shipped registry's
/// has rank 1 out of five names — because moving an amount between two substances
/// of equal composition projects to zero. Per substance, with the kernel's own
/// report on the other side, it is a hard failure.
///
/// **The corruption is applied to the kernel's output, and that is stated rather
/// than hidden.** A test cannot make `react_voxel` commit the bug from outside;
/// what it can do is take the real output and move the delta of one reaction's
/// product into the lane of another substance, which is byte for byte the state
/// a kernel addressing by substance index instead of by `lane[s]` would leave
/// (ADR-056). What is under test is that the residual is sensitive to the class,
/// not that the kernel has the defect.
#[test]
fn a_reaction_written_to_the_wrong_lane_breaks_the_matter_residual() {
    let (t, p, mut world) = reacting_tick();
    let before = domain_sums(&world, false);
    world.run(&t.rx(), &p);

    let mut ledger = Ledger::with_reactions(N_SUBSTANCES, p.n_reactions).unwrap();
    ledger.begin_tick();
    ledger.reduce_extent(&world.xi, N_VOXELS);

    // The control, and it is not decoration: without it a residual that had lost
    // its extent term entirely would be non-zero below and this test would be
    // green on the wrong evidence. The uncorrupted tick has to close first.
    ledger.assert_closed(t.nu(), &before, &domain_sums(&world, true));

    // Everything reaction 0 produced lands on `E` instead of on `C`. Both are
    // narrow, both are products of a reaction that ran, and both are made of the
    // same conserved quantities as their neighbours — so nothing about the
    // arrangement is convenient.
    for idx in 0..N_VOXELS {
        let moved = world.delta(C, idx);
        assert_ne!(moved, 0, "voxel {idx} produced no C to misplace");
        let kept = world.before(C, idx);
        let landed = world.after(E, idx) + moved;
        poke_dst(&mut world, C, idx, kept);
        poke_dst(&mut world, E, idx, landed);
    }
    let after = domain_sums(&world, true);

    // Both ends of the misplacement, and both have to be loud: the lane that did
    // not receive what its reaction made, and the lane that received what was not
    // made for it.
    assert_ne!(
        ledger.residual_matter(t.nu(), C, &before, &after),
        0,
        "the substance whose product went elsewhere still closes"
    );
    assert_ne!(
        ledger.residual_matter(t.nu(), E, &before, &after),
        0,
        "the substance that received a foreign product still closes"
    );

    // And the whole check fires, naming the substance rather than saying only
    // that the tick did not close.
    let refusal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ledger.assert_closed(t.nu(), &before, &after);
    }))
    .expect_err("a misplaced write closed the ledger");
    let message = refusal
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        message.contains("the matter ledger did not close"),
        "the refusal has to say what failed, and says: {message}"
    );
}

/// `ACCEPTANCE.md`, section "Conservation" (ADR-080).
///
/// The second load-bearing name, and it answers the *strongest* rejected
/// alternative: reconstructing `Xi` on the host from the domain deltas
/// (`nu * Xi == Delta n_domain`) instead of taking a report from the kernel. That
/// reconstruction is blind on exactly `span{nu}` — a lost write, a doubled
/// application, a dispatch that missed a slab all move the domain by something
/// proportional to `nu` and pass silently. With two independent witnesses they do
/// not.
///
/// The lost write here is a whole voxel: its amounts stay at state `N` while its
/// block of the extent slice reports the reaction that ran. That is not an exotic
/// arrangement — it is what a dispatch one voxel short of the domain looks like,
/// and it is also what a stale extent slice looks like from the other side.
#[test]
fn a_dropped_reaction_write_breaks_the_matter_residual() {
    let (t, p, mut world) = reacting_tick();
    let before = domain_sums(&world, false);
    world.run(&t.rx(), &p);

    let mut ledger = Ledger::with_reactions(N_SUBSTANCES, p.n_reactions).unwrap();
    ledger.begin_tick();
    ledger.reduce_extent(&world.xi, N_VOXELS);

    // The same control as in the test above: the intact tick closes, so what the
    // assertions below observe is the lost write and not a residual that lost its
    // second term.
    ledger.assert_closed(t.nu(), &before, &domain_sums(&world, true));

    const LOST: u32 = 5;
    let mut dropped = 0i64;
    for s in 0..N_SUBSTANCES {
        dropped += world.delta(s, LOST).abs();
        let kept = world.before(s, LOST);
        poke_dst(&mut world, s, LOST, kept);
    }
    assert!(dropped > 0, "voxel {LOST} had no write to lose");
    let after = domain_sums(&world, true);

    let residuals: Vec<i128> = (0..N_SUBSTANCES)
        .map(|s| ledger.residual_matter(t.nu(), s, &before, &after))
        .collect();
    assert!(
        residuals.iter().any(|&r| r != 0),
        "a whole voxel's write was lost and every substance still closed: \
         {residuals:?}"
    );

    // Precisely, on the product of reaction 0: the domain is short by exactly one
    // voxel's worth of it, and the residual is that number and not merely
    // non-zero. A check that only says "something is off" cannot tell a lost
    // write from a rounding.
    assert_eq!(
        residuals[C as usize],
        -i128::from(i64::from(nu_of(C, 1)) * world.xi(&p, 0, LOST)),
        "the shortfall in C is not one voxel of reaction 0"
    );
}

/// `ACCEPTANCE.md`, section "Conservation".
///
/// One reaction, substrate to spare, and the assertion is an exact zero per
/// conserved quantity — not a tolerance. It fails if a substance standing on
/// both sides was applied as two entries instead of one net one, if any `nu` was
/// rounded at run time, or if a delta was written into somebody else's lane.
#[test]
fn reaction_alone_conserves_each_element_exactly() {
    let t = tables(&[r1()], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 1;

    let mut world = World::uniform([100_000, 100_000, 0, 0, 0, 0], MIXED_LANES, MIXED_MASK);
    world.run(&t.rx(), &p);

    assert_something_happened(&world);
    assert_every_element_balances(&world);

    // And the extent is the one the formula names, so that the balance above is
    // a statement about a reaction that ran rather than about arithmetic.
    for idx in 0..N_VOXELS {
        assert_eq!(extent_of(&world, C, idx), 4);
    }
}

/// `ACCEPTANCE.md`, section "Conservation".
///
/// The same claim, element by element, with the shared pool of `B` chosen so
/// that the two reactions together ask for twice what is there. It fails if the
/// scaling is applied to the substance deltas instead of to `xi`: the balance
/// then drifts by the rounding of each substance separately, which is exactly
/// the construction ADR-026 and ADR-027 rejected.
#[test]
fn competition_scaling_conserves_each_element_exactly() {
    let t = tables(&[r1(), r2()], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 2;

    let mut world = World::uniform([100_000, 0, 0, 100_000, 0, 0], MIXED_LANES, MIXED_MASK);
    // Two reactions want 4 quanta each, at 8 storage units of B per quantum:
    // 64 demanded against 32 available, so the coefficient is exactly a half.
    world.put_everywhere(B, 32);
    world.run(&t.rx(), &p);

    // Competition actually engaged — asserted before the balance, because the
    // balance holds just as exactly when it did not.
    for idx in 0..N_VOXELS {
        let e1 = extent_of(&world, C, idx);
        let e2 = extent_of(&world, E, idx);
        assert!(e1 > 0 && e1 < 4, "reaction 1 ran at {e1}, wanted 4");
        assert!(e2 > 0 && e2 < 4, "reaction 2 ran at {e2}, wanted 4");
        // And the scaled demand did not overdraw the pool it competed for.
        assert!(world.after(B, idx) >= 0);
    }

    assert_something_happened(&world);
    assert_every_element_balances(&world);
}

/// `ACCEPTANCE.md`, section "Conservation".
///
/// The only test that separates "one coefficient per voxel" from "a plausible
/// coefficient", because every candidate conserves matter exactly.
///
/// Three reactions: two compete for a scarce `B`, the third touches it not at
/// all and stands on abundant pools. The third must come out reduced, and
/// reduced by the *same* factor. The `vmax` values make the wants 4, 4 and 8, so
/// the ratios are checked as exact integer cross-products rather than as a
/// float comparison.
///
/// It fails three ways, and each of them is a defensible-looking kernel:
/// a coefficient computed per substance, a coefficient applied only to the
/// consumers of the oversold pool, and a minimum taken over every substance
/// instead of over the oversold ones — the last being below one always, which
/// slows the whole of the chemistry by an unknown factor with no invariant
/// disturbed.
#[test]
fn all_reactions_share_one_competition_coefficient() {
    let t = tables(&[r1(), r2(), r3()], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 3;

    let mut world = World::uniform([100_000, 0, 0, 100_000, 0, 0], MIXED_LANES, MIXED_MASK);
    world.put_everywhere(B, 32);
    world.run(&t.rx(), &p);

    for idx in 0..N_VOXELS {
        let e1 = extent_of(&world, C, idx);
        let e2 = extent_of(&world, E, idx);
        let e3 = extent_of(&world, F, idx);

        assert!(e1 > 0 && e1 < 4, "the first consumer ran at {e1}");
        assert!(
            e3 > 0 && e3 < 8,
            "the reaction that touches no scarce substrate ran at {e3} of 8: \
             the competition coefficient is not shared"
        );

        // want_1 = want_2 = 4, want_3 = 8. One coefficient means
        // e_i * want_j == e_j * want_i for every pair, in integers.
        assert_eq!(e1 * 4, e2 * 4, "the two consumers disagree");
        assert_eq!(e1 * 8, e3 * 4, "the third reaction got its own coefficient");
    }

    assert_every_element_balances(&world);
}

/// `ACCEPTANCE.md`, section "Determinism".
///
/// The same reactions, assembled twice with the rows permuted — the `nu` window
/// travels with `begin`/`len`, and so does every per-row column: `rid`, `e_r`,
/// `cat`, `vmax`, `q10`, `t_vmax`. The result must be identical byte for byte.
///
/// The fixture has to be non-trivial in two separate ways, and neither of them
/// is optional.
///
/// **`B` is oversold**, so the order could decide who eats first. Without
/// competition the rows cannot interact at all and the test is green under any
/// implementation, including the sequential application ADR-041 rejected.
///
/// **Every extent is fractional**, so the stochastic draw of ADR-027 decides it.
/// This is what makes the test able to see a `reaction_id` taken from the row's
/// position: with `x` a whole number the Bernoulli draw never fires, the stream
/// reaches nothing, and permuting the identifiers changes not one bit. That was
/// the first shape of this fixture and it passed against a kernel that drew on
/// `r` — which is why the pool of `B` is 60 rather than 32 here: at 32 the input
/// cap binds at four quanta for both consumers and flattens the randomness back
/// out.
///
/// Both failures are invisible otherwise: the balance goes on closing, and what
/// changes is who ate the substrate first.
#[test]
fn reaction_result_is_independent_of_order_in_toml() {
    let mut p = params();
    p.n_reactions = 3;

    // `x = vmax * dt * V * 2^e_r` comes out at 4.48, 4.8 and 8.8 quanta: the
    // draw decides the last one in every voxel of every reaction.
    let fractional = |mut recipe: Recipe, vmax: f64| -> Recipe {
        recipe.vmax = vmax;
        recipe
    };
    let recipes = || {
        [
            fractional(r1(), 0.28),
            fractional(r2(), 0.30),
            fractional(r3(), 0.55),
        ]
    };
    let [ra, rb, rc] = recipes();
    let forwards = tables(&[ra, rb, rc], &MIXED_LANES, 0.0);
    let [ra, rb, rc] = recipes();
    let backwards = tables(&[rc, rb, ra], &MIXED_LANES, 0.0);

    let mut a = World::uniform([100_000, 0, 0, 100_000, 0, 0], MIXED_LANES, MIXED_MASK);
    a.put_everywhere(B, 60);
    let mut b = World::uniform([100_000, 0, 0, 100_000, 0, 0], MIXED_LANES, MIXED_MASK);
    b.put_everywhere(B, 60);

    a.run(&forwards.rx(), &p);
    b.run(&backwards.rx(), &p);

    assert_eq!(
        a.dst32, b.dst32,
        "the narrow amounts depend on the row order"
    );
    assert_eq!(a.dst64, b.dst64, "the wide amounts depend on the row order");
    assert_eq!(
        a.energy, b.energy,
        "the energy delta depends on the row order"
    );

    // And the run did something worth comparing.
    assert_something_happened(&a);
}

/// `ACCEPTANCE.md`, section "Scale derivation" — where ADR-056 sent it, and
/// where `world/registry.rs` says outright that it is not written because there
/// is no reaction kernel yet. There is one now.
///
/// The same chemistry twice: once over the mixed-width registry, where `A` is
/// wide and every other substance sits one lane below its index, and once over
/// an artificially uniform one, where `lane == s`. The amounts must agree
/// substance by substance.
///
/// It fails on two mistakes, and both are addressing by substance index where
/// the table says lane. Nothing else catches either: the ledger closes, because
/// nothing was lost, and on the registry the project carries, water stands first
/// and takes lane 0, so the wrong call is *right* at `s == 0`.
///
/// The first is `s * n_voxels + idx` in place of `lane[s] * n_voxels + idx`,
/// which is the amount buffers.
///
/// The second is `conc_per_unit[lane[s]]` in place of `conc_per_unit[s]`, and
/// **it is the reason the `km` here is not zero**. The two tables meet in
/// `rate_of` with opposite indexing conventions, and at `km == 0` the Michaelis
/// term is `conc/conc == 1` for any non-empty pool: the concentration cancels
/// out of the kinetics, and with it the only read of `conc_per_unit` in the
/// kernel. With a `km` of the order of the pool concentrations the term is a
/// fraction, the two registries disagree by `2^(k_A - k_s)` under the wrong
/// read, and this is the only outside-view test that can see it — the extents
/// themselves stay whole, so nothing here has to name a number.
#[test]
fn mixed_width_storage_matches_uniform_width_storage() {
    let mut p = params();
    p.n_reactions = 3;

    let mixed_tables = tables(&[r1(), r2(), r3()], &MIXED_LANES, 0.5);
    let uniform_tables = tables(&[r1(), r2(), r3()], &UNIFORM_LANES, 0.5);

    // Distinct starting pools per substance: with equal ones a swapped lane
    // reads the right number by accident.
    let start = [100_000, 32, 7, 100_001, 11, 13];

    let mut mixed = World::uniform(start, MIXED_LANES, MIXED_MASK);
    mixed.run(&mixed_tables.rx(), &p);

    let mut uniform = World::uniform(start, UNIFORM_LANES, 0);
    p.width_mask = 0;
    uniform.run(&uniform_tables.rx(), &p);

    assert_something_happened(&mixed);
    for s in 0..N_SUBSTANCES {
        for idx in 0..N_VOXELS {
            assert_eq!(
                mixed.after(s, idx),
                uniform.after(s, idx),
                "substance {s} of voxel {idx} came out different under the two \
                 registries: the kernel addressed by substance index, not by lane"
            );
        }
    }
    assert_eq!(mixed.energy, uniform.energy);
}

/// `ACCEPTANCE.md`, section "Transport undershoot and what divides by an amount".
///
/// A pool the transport of ADR-068 is allowed to leave negative. The reaction
/// that consumes it must come out at zero extent, and nothing may move.
///
/// It fails if the saturation is missing — `xi_max = floor(-24/8)` is `-3`, the
/// reaction runs *backwards*, matter is conserved and both ledgers close — and
/// it fails if the order of `min` and `max` is swapped: ADR-068 asks for
/// `xi = max(0, min(xi_raw, xi_max))` with the maximum **last**, and
/// `min(max(0, xi_raw), xi_max)` lets the negative cap straight through.
#[test]
fn a_negative_pool_caps_the_extent_at_zero() {
    let t = tables(&[r1()], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 1;

    // Two shapes of the same state. -3 is the undershoot ADR-068 measured on
    // diffusion; -24 is a whole number of quanta below zero, where a floor and a
    // truncation toward zero disagree and only the final saturation saves it.
    for pool in [-3i64, -24] {
        let mut world = World::uniform([100_000, 0, 0, 0, 0, 0], MIXED_LANES, MIXED_MASK);
        world.put_everywhere(B, pool);
        world.run(&t.rx(), &p);

        for idx in 0..N_VOXELS {
            assert_eq!(
                extent_of(&world, C, idx),
                0,
                "a pool of {pool} let the reaction run"
            );
            for s in 0..N_SUBSTANCES {
                assert_eq!(
                    world.after(s, idx),
                    world.before(s, idx),
                    "substance {s} moved at a zero extent"
                );
            }
            assert_eq!(world.energy[idx as usize], M64::ZERO);
        }
    }

    // The same fixture with a healthy pool, so that the assertions above are
    // about a reaction that the negative pool stopped rather than about a
    // reaction that was never going to run. Without this a kernel that does
    // nothing at all passes this test and only this one.
    let mut healthy = World::uniform([100_000, 100_000, 0, 0, 0, 0], MIXED_LANES, MIXED_MASK);
    healthy.run(&t.rx(), &p);
    for idx in 0..N_VOXELS {
        assert_eq!(extent_of(&healthy, C, idx), 4);
    }
}

/// `ACCEPTANCE.md`, section "Transport undershoot and what divides by an amount".
///
/// A negative pool belonging to a substance one of the reactions does not touch
/// at all. The reaction standing on healthy substrate must run its full want:
/// whatever the starved one does about the pool that starves it, it must not
/// reach out of its own row and hold the rest of the voxel back.
///
/// **What this test does not check, and where that lives instead.** ADR-068 asks
/// for two locks on the same door, and only one of them is inside reach from
/// out here. The saturation `scale = max(0, .)` in `competition_scale` cannot be
/// made to fail by any fixture this file can build: the input cap runs before
/// the demand pass, so a reaction consuming a negative pool arrives at
/// `want == 0`, contributes no demand to it, and the coefficient skips the
/// substance before dividing by what is there. Delete the saturation and this
/// test stays green, which was true of it from the day it was written.
///
/// So the claim it does carry is the outer one — a negative pool of `B` leaves
/// `r3` at its full extent of 8 — and the saturation itself is guarded beside
/// the kernel by `the_coefficient_saturates_at_zero_against_a_negative_pool`,
/// which calls `competition_scale` with a demand the input cap would never let
/// through. That is the lock that matters on the day the two passes are
/// reordered, and on that day this test goes on passing.
#[test]
fn a_negative_pool_caps_the_competition_coefficient_at_zero() {
    let t = tables(&[r1(), r3()], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 2;

    let mut world = World::uniform([100_000, 0, 0, 100_000, 0, 0], MIXED_LANES, MIXED_MASK);
    world.put_everywhere(B, -24);
    world.run(&t.rx(), &p);

    for idx in 0..N_VOXELS {
        assert_eq!(extent_of(&world, C, idx), 0, "the starved reaction ran");
        assert_eq!(
            extent_of(&world, F, idx),
            8,
            "a substance the third reaction never touches held it back: the \
             competition coefficient went negative"
        );
    }
    assert_every_element_balances(&world);
}

/// `ACCEPTANCE.md`, section "Analytic and manufactured solutions" — one of the
/// three names of ADR-063.
///
/// A reaction with `cat[r] == NO_CATALYST` and a lawful `vmax` must turn over.
/// It catches the outcome ADR-063 calls an accident rather than an option: an
/// empty catalyst read as a catalyst at zero concentration. Then `rate == 0`,
/// the whole of the S0 chemistry stands still, both halves of the invariant
/// close on `0 == 0` and there is nothing to fail.
#[test]
fn abiotic_reaction_proceeds_with_empty_catalyst() {
    let t = tables(&[r1()], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 1;

    let mut world = World::uniform([100_000, 100_000, 0, 0, 0, 0], MIXED_LANES, MIXED_MASK);
    world.run(&t.rx(), &p);

    for idx in 0..N_VOXELS {
        assert!(
            extent_of(&world, C, idx) > 0,
            "an abiotic reaction with a lawful vmax did not turn over"
        );
    }
}

/// `ACCEPTANCE.md`, section "Analytic and manufactured solutions".
///
/// The same run under two different catalysis buffers: the extent must be
/// identical. Catches the phantom multiplier — an implementation that reads the
/// catalysis column unconditionally and substitutes a one or a zero at the
/// sentinel diverges here.
#[test]
fn abiotic_rate_is_independent_of_every_catalyst_field() {
    let t = tables(&[r1()], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 1;

    let start = [100_000, 100_000, 0, 0, 0, 0];

    let mut low = World::uniform(start, MIXED_LANES, MIXED_MASK);
    low.catalyst = vec![Q::ZERO; N_VOXELS as usize];
    low.run(&t.rx(), &p);

    let mut high = World::uniform(start, MIXED_LANES, MIXED_MASK);
    high.catalyst = (0..N_VOXELS).map(|i| q(f64::from(i) + 17.0)).collect();
    high.run(&t.rx(), &p);

    assert_eq!(low.dst32, high.dst32);
    assert_eq!(low.dst64, high.dst64);
    assert_eq!(low.energy, high.energy);
    assert_something_happened(&low);
}

/// `ACCEPTANCE.md`, section "Analytic and manufactured solutions".
///
/// Two reactions with the same number in `vmax`, one with an empty catalyst and
/// one with a full column: the catalysed rate is the abiotic one times the
/// catalyst concentration, which is what pins the conditional unit of `vmax`
/// from both sides (ADR-063).
///
/// It cannot be checked by a ledger even in principle: the fork stands before
/// `rate`, and conservation is a property of the vector `nu`.
#[test]
fn catalyzed_rate_equals_abiotic_rate_times_catalyst_concentration() {
    let mut catalysed = r2();
    catalysed.catalyst = 0;
    catalysed.vmax = r1().vmax;

    let t = tables(&[r1(), catalysed], &MIXED_LANES, 0.0);
    let mut p = params();
    p.n_reactions = 2;

    // Everything abundant: with nothing oversold the competition coefficient is
    // one, and the two extents are the two rates.
    let mut world = World::uniform(
        [1_000_000, 1_000_000, 0, 1_000_000, 0, 0],
        MIXED_LANES,
        MIXED_MASK,
    );
    world.catalyst = vec![q(3.0); N_VOXELS as usize];
    world.run(&t.rx(), &p);

    for idx in 0..N_VOXELS {
        let abiotic = extent_of(&world, C, idx);
        let catalysed = extent_of(&world, E, idx);
        assert!(abiotic > 0);
        assert_eq!(
            catalysed,
            abiotic * 3,
            "the catalysed reaction ran at {catalysed} against {abiotic} at a \
             catalyst concentration of 3"
        );
    }
}

// ---------------------------------------------------------------------------
// The energy half of the invariant (ADR-081)
// ---------------------------------------------------------------------------

/// A closed box carrying the chemistry of `configs/scenarios/h2s-oxidation.toml`,
/// with formation enthalpies that agree with the declared reaction enthalpies to
/// the last digit (ADR-044).
///
/// Written out rather than read off the shipped file, for the reason every other
/// fixture in `tests/` gives: an acceptance test that loaded the repository's own
/// scenario would go red the day somebody calibrated it, and would go red about
/// calibration rather than about the sign of `nu_E`.
///
/// Sulfate keeps its **negative** heat capacity, which is physics and not a typo
/// (`configs/scenarios/h2s-oxidation.toml`): the denominator of the temperature
/// is a sum of different signs, and the solvent is what makes it positive. See
/// `an_exothermic_reaction_warms_its_cell` for why the cell has to hold water.
const EXOTHERMIC_SCENARIO: &str = r#"
name = "exothermic-fixture"
dt = 1.0
beta = 0.015625
T_ref = 300.0

[conserved]
C = 12.01070
N = 14.00670
P = 30.97376
S = 32.06500
Fe = 55.84500

[grid]
nx = 8
ny = 8
nz = 8
dx = 1.0e-4

[boundary]
x_min = "periodic"
x_max = "periodic"
y_min = "periodic"
y_max = "periodic"
z_min = "closed"
z_max = "closed"

[[substance]]
id = "H2S"
molar_mass = 34.08088
typical_conc = 0.1
max_conc = 10.0
partial_molar_volume = 3.5e-5
settling_radius = 0.0
diffusivity = 1.6e-9
c_p = 179.0
enthalpy_formation = 0.0
composition = { S = 1 }

[[substance]]
id = "WATER"
molar_mass = 18.01528
typical_conc = 55000.0
max_conc = 55600.0
partial_molar_volume = 1.8e-5
settling_radius = 0.0
diffusivity = 2.3e-9
c_p = 75.3
enthalpy_formation = -285830.0
composition = {}

[[substance]]
id = "O2"
molar_mass = 31.99880
typical_conc = 0.25
max_conc = 1.0
partial_molar_volume = 3.1e-5
settling_radius = 0.0
diffusivity = 2.1e-9
c_p = 234.0
enthalpy_formation = 0.0
composition = {}

[[substance]]
id = "SO4"
molar_mass = 96.06260
typical_conc = 28.0
max_conc = 100.0
partial_molar_volume = 1.4e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = -293.0
enthalpy_formation = -846000.0
composition = { S = 1 }

[[substance]]
id = "H_ION"
molar_mass = 1.007940
typical_conc = 1.0e-4
max_conc = 1.0e-2
partial_molar_volume = 0.0
settling_radius = 0.0
diffusivity = 9.3e-9
c_p = 0.0
enthalpy_formation = 0.0
composition = {}

[[reaction]]
id = "h2s_oxidation"
enthalpy = -846000.0
catalyst = ""
energy_from = ""
inputs = { H2S = 1, O2 = 2 }
outputs = { SO4 = 1, H_ION = 2 }

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = { H2S = 0.01, O2 = 0.01 }

[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15
"#;

/// The fine grid of [`EXOTHERMIC_SCENARIO`], and the coarse grid the enthalpy
/// field folds onto: `2 x 2 x 2` cells of sixty-four fine voxels each.
const FINE: u32 = 8;
const N_FINE: u32 = FINE * FINE * FINE;
const ENTHALPY_LOD: u32 = 2;
const COARSE_SIDE: u32 = FINE >> ENTHALPY_LOD;
const N_COARSE: u32 = COARSE_SIDE * COARSE_SIDE * COARSE_SIDE;
/// `2^(3*lod)`: how many fine voxels one coarse cell owns.
const PER_COARSE: i64 = 1 << (3 * ENTHALPY_LOD);

/// The scenario's zero of enthalpy storage, K, as the fixture declares it.
const T_REF_SCENARIO: f64 = 300.0;

/// How far the reaction is made to run in every fine voxel of the warm cell, in
/// quanta of `2^-e_r` turnovers.
///
/// `2^24`, and deliberately not the `xi ~ 2` a tick of the shipped scenario
/// actually produces (ADR-081). One tick of real chemistry warms a coarse cell
/// by `1.8e-7 K`, and `Q` is an `f32`: at 300 K its unit in the last place is
/// `3e-5 K`, two hundred times larger, so `T_ref + H/C` answers `T_ref` exactly
/// and a strict inequality would be red under **either** sign. The fixture
/// therefore runs a million ticks' worth of extent in one dispatch, which is the
/// smallest change that makes the sign observable at all. What is under test is
/// the sign of `nu_E`, and the sign does not depend on how far the reaction ran.
const XI: i64 = 1 << 24;

/// The lane of every substance of [`EXOTHERMIC_SCENARIO`] within its width class,
/// and the mask that says which class that is (ADR-040, ADR-056).
///
/// Derived from the loader's own widths rather than written out: which substance
/// ends up wide is a conclusion of `config/derive.rs` from `max_conc`, and a
/// hand-written table here would be a second source for it.
fn lanes_of(d: &config::Derived) -> (Vec<u32>, u32) {
    let mut lane = vec![0u32; d.substances().len()];
    let mut mask = 0u32;
    let (mut n32, mut n64) = (0u32, 0u32);
    for (s, decl) in d.decls().iter().enumerate() {
        match decl.width {
            Width::Bits64 => {
                lane[s] = n64;
                mask |= 1 << s;
                n64 += 1;
            }
            Width::Bits32 => {
                lane[s] = n32;
                n32 += 1;
            }
        }
    }
    (lane, mask)
}

/// `ACCEPTANCE.md`, section "Physics" (ADR-081, ADR-044, ADR-079).
///
/// **The one thing in this repository that can tell the two signs of `nu_E`
/// apart.** On the shipped scenario the old definition — `round(dH * 2^(k_E -
/// e_r))` — and the new one — `-Sum_s nu_s * w_s` — give the same modulus and
/// differ only in sign, so every conservation test, every "alone" test and every
/// residual is green under either: the same `nu_E` stands on both sides of the
/// energy ledger. Only a temperature can see it, and a temperature has only
/// existed since ADR-079.
///
/// The chain is the one a tick runs: the loader fixes `nu_E`, `kernels/fold.rs`
/// adds it to the enthalpy field, and `kernels/temperature.rs` divides by the
/// composition of the cell. Nothing here flips a sign of its own — that is the
/// whole content of ADR-081, "the sign lives in one place, in the derivation at
/// load".
///
/// **The denominator has to be positive, and it is not positive by nature.**
/// `T = T_ref + H/Sum(n*c_p)` is warming only where the sum is above zero, and
/// aqueous sulfate declares `c_p = -293 J/(mol K)`, which is real. ADR-077 lets a
/// scenario put a substance on one side of the layer, so a voxel holding sulfate
/// and no solvent is a legal state of a legal world; there ADR-079 answers about
/// the **cell** rather than about the kernel — `C_cell <= 0` gives `T := T_ref`,
/// divides nothing and panics in no build profile. Such a cell would make this
/// test compare `T_ref` against `T_ref` and pass under either sign, so the cell
/// below holds water and the assertion on the capacity is written out first.
#[test]
fn an_exothermic_reaction_warms_its_cell() {
    let config = config::parse(EXOTHERMIC_SCENARIO).expect("the fixture must parse");
    let d = config::validate(&config).expect("the fixture must validate");

    let rx = d
        .reactions()
        .iter()
        .find(|r| r.id == "h2s_oxidation")
        .expect("the fixture declares it");

    // (1) The sign, at the one place it is decided. The declared enthalpy is
    //     negative — the reaction gives heat up — and `nu_E` is stated in the
    //     direction of the **field**, so it is positive: the fold adds it, and
    //     what the field gains the chemical form lost.
    assert!(
        rx.nu_energy > 0,
        "an exothermic reaction has nu_E = {} in the direction of the field \
         (ADR-081)",
        rx.nu_energy
    );
    assert_eq!(
        rx.nu_energy, 54_144_000,
        "846000 J/turnover at k_E = 67 and e_r = 61 is 846000 * 2^6"
    );

    // The composition of the warm cell: the solvent at its typical concentration,
    // and sulfate — whose `c_p` is negative — beside it.
    let (lane, mask) = lanes_of(&d);
    let n32 = d
        .decls()
        .iter()
        .filter(|s| s.width == Width::Bits32)
        .count() as u32;
    let n64 = d.decls().len() as u32 - n32;
    let lane_len = N_FINE + 1;
    let mut amounts_32 = vec![M32::ZERO; (n32 * lane_len) as usize];
    let mut amounts_64 = vec![M64::ZERO; (n64 * lane_len) as usize];

    let mut capacity_per_unit = vec![Q::ZERO; d.substances().len()];
    for (s, sub) in d.substances().iter().enumerate() {
        // `c_p[s] * 2^-k[s]`, J/(K * storage unit) — what the host folds for
        // `Heat::capacity_per_unit`, computed here out of the scenario's own
        // `c_p` and the loader's own `k`.
        let per_unit = config.substance[s].c_p / (2f64).powi(i32::from(sub.k));
        capacity_per_unit[s] = Q::from_f64(per_unit);
        let units = i64::try_from(sub.amount_at_typical).expect("a typical amount fits an i64");
        for idx in 0..N_FINE {
            let at = (lane[s] * lane_len + idx) as usize;
            if mask & (1 << s) != 0 {
                amounts_64[at] = M64::new(units);
            } else {
                amounts_32[at] = M32::new(i32::try_from(units).expect("a narrow amount fits"));
            }
        }
    }

    let t_p = TemperatureParams {
        nx: FINE,
        ny: FINE,
        nz: FINE,
        lod: ENTHALPY_LOD,
        n_voxels: N_FINE,
        lane_len,
        n_substances: d.substances().len() as u32,
        width_mask: mask,
        t_ref: Q::from_f64(T_REF_SCENARIO),
        // `2^-k_E`, the joules one storage unit of enthalpy is worth (ADR-062).
        joules_per_unit: Q::from_f64((2f64).powi(-i32::from(d.energy().k_e))),
    };
    let heat = Heat {
        lane: &lane,
        capacity_per_unit: &capacity_per_unit,
    };

    let mut src_h = vec![M64::ZERO; N_COARSE as usize];
    let mut dst_h = vec![M64::ZERO; N_COARSE as usize];
    let mut heat_capacity = vec![Q::ZERO; N_COARSE as usize];
    let mut temperature = vec![Q::ZERO; N_COARSE as usize];

    for coarse in 0..N_COARSE {
        temperature_cell(
            &amounts_32,
            &amounts_64,
            &src_h,
            &mut heat_capacity,
            &mut temperature,
            &heat,
            &t_p,
            coarse,
        );
    }
    let before = temperature[0];
    let c_cell = heat_capacity[0];

    // (2) The denominator, before anything is asserted about the numerator.
    //     A cell answering `T_ref` because it has no heat capacity (ADR-079)
    //     would pass the comparison below under either sign of `nu_E`.
    assert!(
        c_cell > Q::ZERO,
        "the cell has C_cell = {c_cell:?} and answers T_ref without dividing \
         (ADR-079), so it cannot tell the two signs apart"
    );
    assert_eq!(before, Q::from_f64(T_REF_SCENARIO), "H = 0 means T = T_ref");

    // The reaction runs in every fine voxel of coarse cell 0 and nowhere else.
    let mut energy_delta = vec![M64::ZERO; N_FINE as usize];
    for z in 0..(1u32 << ENTHALPY_LOD) {
        for y in 0..(1u32 << ENTHALPY_LOD) {
            for x in 0..(1u32 << ENTHALPY_LOD) {
                energy_delta[(x + y * FINE + z * FINE * FINE) as usize] =
                    M64::new(rx.nu_energy * XI);
            }
        }
    }

    let light = vec![Q::ZERO; N_FINE as usize];
    let mut solar = vec![M64::ZERO; N_COARSE as usize];
    let f_p = FoldParams {
        nx: FINE,
        ny: FINE,
        nz: FINE,
        lod: ENTHALPY_LOD,
        // A dark box: the solar term is identically zero, so what reaches the
        // field is the chemistry and nothing else.
        i_surface: Q::ZERO,
        units_per_intensity: Q::ZERO,
        dt: Q::ONE,
    };
    for coarse in 0..N_COARSE {
        fold_energy(
            &energy_delta,
            &light,
            &src_h,
            &mut dst_h,
            &mut solar,
            &f_p,
            coarse,
        );
    }
    assert_eq!(
        dst_h[0].to_i64(),
        PER_COARSE * rx.nu_energy * XI,
        "the fold gathers sixty-four fine voxels into one cell"
    );

    src_h.copy_from_slice(&dst_h);
    for coarse in 0..N_COARSE {
        temperature_cell(
            &amounts_32,
            &amounts_64,
            &src_h,
            &mut heat_capacity,
            &mut temperature,
            &heat,
            &t_p,
            coarse,
        );
    }
    let after = temperature[0];

    // (3) Strictly warmer, and by the predicted amount. "Not colder" would be
    //     satisfied by a cell that did nothing.
    assert!(
        after > before,
        "burning sulfide cooled the water: {before:?} -> {after:?} (ADR-081)"
    );
    let predicted = PER_COARSE as f64 * XI as f64 * rx.nu_energy as f64
        / ((2f64).powi(i32::from(d.energy().k_e)) * c_cell.debug_f64());
    let got = after.debug_f64() - before.debug_f64();
    assert!(
        (got - predicted).abs() <= 1.0e-5 * predicted.abs(),
        "the cell warmed by {got} K against the predicted {predicted} K"
    );
    // And the magnitude written out, so that a scale slipped by a power of two
    // is visible without recomputing the formula above.
    assert!(
        (1.48..1.49).contains(&got),
        "64 * 2^24 quanta at nu_E = 54144000 and C_cell = 2.645e-4 J/K is 1.486 K, \
         and this cell warmed by {got} K"
    );

    // (4) Every other coarse cell stood still: the fold reads its own sixty-four
    //     voxels, and a cell warmed by somebody else's chemistry would make the
    //     assertion above true for the wrong reason.
    for coarse in 1..N_COARSE {
        assert_eq!(
            temperature[coarse as usize],
            Q::from_f64(T_REF_SCENARIO),
            "coarse cell {coarse} moved without any chemistry in it"
        );
    }
}

/// What one storage unit of each fixture substance is worth as chemical energy,
/// in the storage units of the enthalpy field (ADR-081).
///
/// `w_s = round(enthalpy_formation_s * 2^(k_E - k_s))`, and here the numbers are
/// chosen rather than derived, for the reason every other table in this file is
/// chosen: no scenario carries this chemistry. What they are chosen **for** is
/// the identity the record rests on — `Sum_s nu_s * w_s + nu_E == 0` for every
/// one of the three reactions — and `the_fixture_weights_agree_with_its_energy_coefficients`
/// is what keeps the three `molar_energy` numbers above and this table in step.
///
/// Every one of them is negative, as a formation enthalpy usually is, and the
/// three reactions come out exothermic — `nu_E > 0`, in the direction of the
/// field.
const W: [i64; N_SUBSTANCES as usize] = [-4, -8, -40, -6, -70, -38];

/// The left side of the **energy** invariant over one of the fixture's two
/// states: the enthalpy of the field plus the chemical energy of what the domain
/// holds (ADR-081).
///
/// `add_chemical_energy` last, after every matter door, and the order is not
/// stylistic: the door reads `DomainSums::matter`, so called earlier it weighs a
/// half-filled table. No residual can see that — the same short sum taken before
/// and after cancels in `after - before` — which is why the rule lives in a
/// comment here and in `Tick::domain_sums`, and why the absolute number is
/// asserted by `load_reports_the_chemical_energy_of_the_domain_in_joules`
/// instead.
fn energy_sums(world: &World, enthalpy: &[M64], after: bool) -> DomainSums {
    let mut sums = domain_sums(world, after);
    sums.add_enthalpy_lane_64(enthalpy);
    sums.add_chemical_energy(&W);
    sums
}

/// The identity the fixture is built on, asserted rather than trusted.
///
/// Without it the two tables below drift apart at the first edit and every
/// energy assertion in this file becomes a statement about whatever they drifted
/// to. The kernel is not involved: this is arithmetic over the tables the host
/// hands it.
#[test]
fn the_fixture_weights_agree_with_its_energy_coefficients() {
    let t = tables(&[r1(), r2(), r3()], &MIXED_LANES, 0.0);
    for r in 0..3usize {
        let begin = t.begin[r] as usize;
        let len = t.len[r] as usize;
        let mut sum = 0i64;
        let mut nu_energy = None;
        for i in begin..begin + len {
            let s = t.nu_sub[i];
            if s == S_ENERGY {
                nu_energy = Some(i64::from(t.nu[i]));
            } else {
                sum += i64::from(t.nu[i]) * W[s as usize];
            }
        }
        let nu_energy = nu_energy.expect("every recipe carries an energy record");
        assert_eq!(
            nu_energy, -sum,
            "reaction {r} has nu_E = {nu_energy} against -Sum nu_s*w_s = {}",
            -sum
        );
        assert!(
            nu_energy > 0,
            "the fixture's reactions are exothermic, so nu_E is positive in the \
             direction of the field (ADR-081)"
        );
    }
}

/// `ACCEPTANCE.md`, section "Conservation" (ADR-081, ADR-080, ADR-028).
///
/// The energy twin of `the_matter_residual_closes_across_a_tick_with_chemistry_in_it`,
/// and it closes by a **different** mechanism, which is the whole of why the two
/// records are two. Matter gets a second term on the right — `Sum_r nu_(r,s) *
/// Xi_r` — because chemistry turns substances into one another. Energy gets
/// nothing on the right at all: the chemical energy of the substances joins the
/// **left** side, so a reaction stops being a source and becomes a transfer
/// between two forms of one quantity, and the identity `Sum_s nu_s*w_s + nu_E ==
/// 0` makes the residual zero by construction rather than by check.
///
/// Three things, and the third is what keeps the first honest.
///
/// (1) the residual is an exact zero, with the left side taken as
///     `enthalpy field + Sum_s w_s*n_s`;
/// (2) not one energy counter moved. No `CHEMICAL_HEAT` was invented: a channel
///     is a door **out** of the domain and a reaction is not a door, which is the
///     one argument ADR-081 rejects the channel on;
/// (3) the negative control. The same tick judged with a left side that omits the
///     chemical term does **not** close, and is off by exactly `nu_E * Xi`
///     summed over the reactions.
///
/// **The pair `(h, i')` is dispatched together or not at all** (ADR-045), and
/// this test is why that matters here. `nu_E * Xi` reaches the enthalpy field
/// only through the fold, while `Sum w_s*n_s` moves the instant the reaction
/// kernel writes; step `h` run without `i'` therefore leaves the left side short
/// by exactly `nu_E * Xi`, and the plausible repair — adding `Sum_r nu_(E,r)*Xi_r`
/// to the right — is the double credit ADR-080 forbids in as many words. Neither
/// the compiler nor any other test in this repository marks that boundary; (3)
/// does.
#[test]
fn a_reacting_tick_closes_the_energy_ledger_with_no_channel() {
    let (t, p, mut world) = reacting_tick();

    // The enthalpy field of this fixture is one cell per voxel — `lod = 0`, the
    // legal 1:1 fold `kernels/fold.rs` documents — so the fold is a copy of the
    // accumulator into the field and the mapping of fine voxels to coarse cells
    // is exercised where it belongs, beside that kernel.
    let mut src_h = vec![M64::ZERO; N_VOXELS as usize];
    let mut dst_h = vec![M64::ZERO; N_VOXELS as usize];
    let before = energy_sums(&world, &src_h, false);

    world.run(&t.rx(), &p);

    let light = vec![Q::ZERO; N_VOXELS as usize];
    let mut solar = vec![M64::ZERO; N_VOXELS as usize];
    let f_p = FoldParams {
        nx: NX,
        ny: NY,
        nz: NZ,
        lod: 0,
        i_surface: Q::ZERO,
        units_per_intensity: Q::ZERO,
        dt: Q::ONE,
    };
    for coarse in 0..N_VOXELS {
        fold_energy(
            &world.energy,
            &light,
            &src_h,
            &mut dst_h,
            &mut solar,
            &f_p,
            coarse,
        );
    }
    src_h.copy_from_slice(&dst_h);
    let after = energy_sums(&world, &src_h, true);

    assert_something_happened(&world);
    assert_ne!(
        after.matter(C),
        before.matter(C),
        "no reaction ran, so the residual below closes on 0 == 0"
    );
    assert_ne!(
        src_h.iter().map(|h| h.to_i64()).sum::<i64>(),
        0,
        "the enthalpy field did not move, so the chemical term has nothing to \
         cancel against"
    );

    let mut ledger = Ledger::with_reactions(N_SUBSTANCES, p.n_reactions).unwrap();
    ledger.begin_tick();
    ledger.reduce_extent(&world.xi, N_VOXELS);

    // (1)
    assert_eq!(
        ledger.residual_energy(&before, &after),
        0,
        "the energy ledger did not close on a reacting tick"
    );

    // (2)
    for channel in Channel::ALL {
        assert_eq!(
            ledger.energy(channel),
            0,
            "{} credited energy on a closed domain",
            channel.name()
        );
    }

    // (3) The same tick judged without the chemical term on the left.
    let mut blind_before = domain_sums(&world, false);
    blind_before.add_enthalpy_lane_64(&vec![M64::ZERO; N_VOXELS as usize]);
    let mut blind_after = domain_sums(&world, true);
    blind_after.add_enthalpy_lane_64(&src_h);
    let short = ledger.residual_energy(&blind_before, &blind_after);
    let heat: i128 = (0..p.n_reactions as usize)
        .map(|r| {
            let begin = t.begin[r] as usize;
            let len = t.len[r] as usize;
            let nu_energy = (begin..begin + len)
                .find(|&i| t.nu_sub[i] == S_ENERGY)
                .map(|i| i128::from(t.nu[i]))
                .expect("every recipe carries an energy record");
            nu_energy * ledger.extent(r as u32)
        })
        .sum();
    assert!(heat > 0, "the fixture's chemistry released no heat at all");
    assert_eq!(
        short, heat,
        "a left side without the chemical term is off by exactly nu_E * Xi, and \
         adding that to the right would credit one transformation twice \
         (ADR-080)"
    );
}

/// `ACCEPTANCE.md`, section "Детерминизм" (ADR-027, ADR-090).
///
/// The name it carries belongs to a different section of the document than the
/// thirteen at the head of this file, for the same reason the energy tests above
/// do: this is where the only fixture in `tests/` that puts a real reaction
/// registry through `parse` -> `validate` already lives.
///
/// **It has to go through `validate`, not through `load`.** `config::load` is
/// `read_to_string` plus `parse` and never calls the derivation, so
/// `every_scenario_in_the_repository_loads` sees neither `rid` nor either
/// refusal of ADR-090 — `config/validate.rs` says so in its own head comment,
/// and says it is a decision rather than an omission.
///
/// **Why four assertions and not one.** Each kills a defect the other three let
/// through, and every one of them leaves both residuals closing: `rid` is only
/// the third counter of `rand`, conservation is a property of the vector `nu`
/// and not of the extent `xi` (ADR-027), so a world drawing the wrong stream
/// goes on balancing exactly as well as a correct one.
///
/// - (1) the two derivations disagree on `reactions()[0].id`. Without it the day
///   `derive` starts normalising the order of the registry — a sort by name is
///   the obvious way to do it — both sides become the same registry and the test
///   compares a config against itself forever. `config_hash.rs` keeps a `swap`
///   helper asserting "exactly one occurrence" against the same failure;
/// - (2) `rid` is looked up **by name**, never by index. Comparing
///   `reactions()[0].rid` across the two would be green under a positional
///   `rid`, which is the one defect the name is about;
/// - (3) the two `rid` differ from each other. A constant `rid` — zero for every
///   reaction — is stable under reordering and is not taken from the position,
///   so it passes (1) and (2); what it does to a world is give two reactions in
///   one voxel on one tick a single draw. Today the collision refusal of ADR-090
///   fires first and this fixture never reaches (3), which is exactly why (3) is
///   written down: the two arrived in one record, and a later author relaxing
///   the refusal must not also be the author who removes its last witness;
/// - (4) the two numbers are written out as literals. A dense index assigned
///   after sorting the names lexicographically — the variant ADR-090 rejected —
///   would give 0 and 1 and pass (1) through (3). Calling `numeric::name_key`
///   here instead would fold the name on both sides of the equality and stay
///   green against a `derive` that folded a **normalised** name (trim, case,
///   NFC), which the doc comment of `DerivedReaction::rid` forbids by name.
///
/// **What it cannot see, and ADR-090 leaves open.** It permutes rows, it does
/// not add them, so a `rid` that depended on the whole registry — `name_key(id)
/// ^ mix(n_reactions)` — is stable here and drifts the day a scenario gains a
/// reaction. The literals close the dense index and most rebasings, not the
/// class.
///
/// **If it goes red, suspect the scales first.** `k_E`, `e_r` and the mass
/// tolerance are functions of the whole registry; both texts declare the same
/// *set* of reactions, so a min/max accumulation gives the same numbers, but an
/// accumulation sensitive to traversal order would split the two derivations and
/// report it here as a wrong identifier. Compare `e_r`, `scarcest` and
/// `nu_energy` of the two before suspecting the fold — and do not weaken these
/// assertions if they differ, because scales that depend on the order of TOML
/// rows violate ADR-027 in the same way `rid` would.
#[test]
fn reaction_id_is_stable_under_reordering_in_toml() {
    // Two `split_once` cut [`EXOTHERMIC_SCENARIO`] into head, the whole reaction
    // record, and the tail. The second reaction is that same record under a
    // second name: identical chemistry means the element, mass and energy
    // balances hold by construction and not one number is invented here. The
    // pair is the one `config/derive.rs` already derives — neither key lands in
    // a reserved `purpose` window, and neither collides with the other.
    let (head, rest) = EXOTHERMIC_SCENARIO
        .split_once("\n[[reaction]]")
        .expect("the fixture declares a reaction");
    let (body, rest) = rest
        .split_once("\n[[field]]")
        .expect("the reaction record is followed by the enthalpy field");
    let first = format!("\n[[reaction]]{body}");
    let tail = format!("\n[[field]]{rest}");
    assert_eq!(
        first.matches("h2s_oxidation").count(),
        1,
        "the rename below has nothing to rename, so both texts would be the \
         same registry and this test would never be able to go red"
    );
    let second = first.replace("h2s_oxidation", "rxn_138249");

    // The same three pieces in the two orders — a permutation, not two editions.
    let ab = format!("{head}{first}{second}{tail}");
    let ba = format!("{head}{second}{first}{tail}");

    let derive = |text: &str| {
        let config = config::parse(text).expect("the permutation must parse");
        config::validate(&config).expect("the permutation must validate")
    };
    let d_ab = derive(&ab);
    let d_ba = derive(&ba);

    let rid_of = |d: &config::Derived, id: &str| {
        d.reactions()
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("the registry declares {id}"))
            .rid
    };

    // (1) The permutation reached the derivation at all.
    assert_eq!(d_ab.reactions()[0].id, "h2s_oxidation");
    assert_eq!(
        d_ba.reactions()[0].id,
        "rxn_138249",
        "the two derivations agree on the first row, so nothing was permuted"
    );

    // (2) The identifier follows the name across the swap, not the row.
    assert_eq!(
        rid_of(&d_ab, "h2s_oxidation"),
        rid_of(&d_ba, "h2s_oxidation"),
        "moving a reaction down the file moved its `rid` (ADR-027)"
    );
    assert_eq!(
        rid_of(&d_ba, "rxn_138249"),
        rid_of(&d_ab, "rxn_138249"),
        "moving a reaction up the file moved its `rid` (ADR-027)"
    );

    // (3) Two names, two counters.
    assert_ne!(
        rid_of(&d_ab, "h2s_oxidation"),
        rid_of(&d_ab, "rxn_138249"),
        "one `rid` for two reactions is one draw for two reactions in a voxel, \
         and both residuals close through it (ADR-090)"
    );

    // (4) The counters are the fold of the names and of nothing else.
    assert_eq!(rid_of(&d_ab, "h2s_oxidation"), 603_427_705);
    assert_eq!(
        rid_of(&d_ab, "rxn_138249"),
        148_503_722,
        "`rid` is `name_key` of the name as TOML wrote it, so a canonicalised \
         position or a normalised name is a different number (ADR-090)"
    );
}
