//! Acceptance criteria of the reaction kernel (`ACCEPTANCE.md`, S0).
//!
//! Ten names, and the document fixes them — the stage is accepted by these and
//! not by a reading of the code:
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
//! ```
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

use liminis_core::kernels::react::{NO_CATALYST, ReactParams, Rx, react_voxel};
use liminis_core::numeric::{M32, M64, Q, run_key};

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
    /// The molar enthalpy coefficient of the reaction, in the same signed sense:
    /// negative means the record looks like an input, which is what an
    /// exothermic reaction looks like from inside the vector.
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
                &self.temperature,
                &self.catalyst,
                rx,
                p,
                idx,
            );
        }
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
        molar_energy: -3,
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
        molar_energy: -5,
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
        molar_energy: -7,
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
