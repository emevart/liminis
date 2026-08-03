//! Acceptance criteria of the ledger: channel counters and the two residuals
//! (`ACCEPTANCE.md`, section "Conservation"; ADR-059, ADR-003, ADR-028).
//!
//! ```text
//! channel_counters_do_not_overflow_at_1e7_ticks
//! a_channel_counter_holds_ten_million_lit_ticks_at_full_sun
//! a_boundary_flux_of_one_percent_of_the_pool_survives_the_declared_horizon
//! a_closed_domain_leaves_every_channel_counter_at_zero
//! domain_sums_do_not_overflow_for_water_at_256_cubed
//! ```
//!
//! They live in an integration test rather than beside the module for the
//! reason already written at the top of `acceptance_diffusion.rs`: an acceptance
//! test is the outside view, it may use only the public surface of
//! `liminis-core`, and so it cannot be quietly shaped around a private helper it
//! was supposed to be judging.
//!
//! The unit tests under `src/ledger/` are the inside view, and they check things
//! these five cannot: the sign convention, the tick delta against the running
//! total, the shape of the counter table, and the width of the counter against
//! the width of the domain sum it closes against. Neither set subsumes the other.

use liminis_core::ledger::{CHANNEL_COUNT, Channel, DomainSums, Ledger, Nu};
use liminis_core::numeric::{M32, M64};
use liminis_core::process::DiffusePhase;
use liminis_core::world::{Boundary, Field, Field32, Field64, Grid};

/// A grid with no exchanging face: `k_ex` multiplies a flux nobody gathers.
///
/// The venting cases are the outside view of ADR-059 and live in
/// `tests/acceptance_boundary.rs`.
const SEALED: f64 = 0.0;

/// One substance per lane, and a scratch ledger. On a sealed grid no counter is
/// ever touched, so which substance a lane stands for cannot matter here.
fn run_diffusion_32(phase: &DiffusePhase, field: &mut Field32) {
    let table: Vec<u32> = (0..field.lanes()).collect();
    phase.apply_32(field, &table, &[0; 32], &mut Ledger::new(32).unwrap());
}

fn run_diffusion_64(phase: &DiffusePhase, field: &mut Field64) {
    let table: Vec<u32> = (0..field.lanes()).collect();
    phase.apply_64(field, &table, &[0; 32], &mut Ledger::new(32).unwrap());
}

// ---------------------------------------------------------------------------
// channel_counters_do_not_overflow_at_1e7_ticks
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md` says in as many words that this test does not run ten
/// million ticks: it substitutes the boundary value. The horizon is the one
/// ADR-004 declares, `10^6`–`10^7` ticks, and the worst case is taken at the
/// upper end of it.
///
/// **The old placeholder is kept and is now labelled as one.** A thousand units
/// a tick over `10^7` ticks is `10^10`, and `QUANTITIES.md` section 3 used it to
/// argue that a counter must be sixty-four bits wide. Half of that argument was
/// always true and stayed true: `i32` is hopeless, silently and not loudly.
/// The other half — "and `i64` is roomy" — **is false by number**, and it is not
/// the arithmetic below that refutes it, it is the two tests underneath this one:
/// full sun fills an `i64` energy counter 2.62 times over in a *single* tick, and
/// a through-flow of one percent of a surface voxel's pool exhausts it on tick
/// `1.1e5`, eleven percent of the shortest declared horizon (ADR-083). This test
/// is therefore about the `i32` half only, and says so, rather than standing as a
/// green monument to the claim that was disproved.
///
/// What overflow would mean here is not a lost low bit. It is a counter that
/// comes back a plausible number seven times too small, or — a little further
/// along the same run — a number of the opposite sign, which reads as an
/// outflow where there was an inflow. The ledger would go on closing, against
/// the wrong right-hand side.
#[test]
fn channel_counters_do_not_overflow_at_1e7_ticks() {
    const PER_TICK: i128 = 1_000;
    const TICKS: i128 = 10_000_000;
    const HORIZON: i128 = PER_TICK * TICKS;

    let mut ledger = Ledger::new(2).unwrap();

    ledger.credit_matter(Channel::BoundaryExchange, 0, HORIZON);
    assert_eq!(
        ledger.matter(Channel::BoundaryExchange, 0),
        10_000_000_000,
        "the counter lost digits at the declared horizon"
    );

    // Not "it would be tight in an i32": it does not fit at all.
    assert!(i32::try_from(HORIZON).is_err());
    // And the failure of a 32-bit counter would be silent rather than loud — a
    // seventh of the true flow, still positive, still plausible.
    assert_eq!(HORIZON as i32, 1_410_065_408);
    // A little earlier in the same run, at three thousand units a tick over the
    // first 10^6 ticks, the wrap lands on the wrong side of zero and the
    // counter reports an outflow through a channel that only let matter in.
    assert!((3_000_000_000i64 as i32) < 0);

    // Energy behaves the same way. It is a separate table (ADR-028), so it
    // would have to be got wrong separately too.
    ledger.credit_energy(Channel::SolarIn, HORIZON);
    assert_eq!(ledger.energy(Channel::SolarIn), 10_000_000_000);
    assert!(i32::try_from(ledger.energy(Channel::SolarIn)).is_err());

    // Signed, and holding the net: a `u64` counter would be wide enough and
    // would refuse this credit.
    ledger.credit_matter(Channel::RadiativeOut, 1, -HORIZON);
    assert_eq!(ledger.matter(Channel::RadiativeOut, 1), -10_000_000_000);
    ledger.credit_energy(Channel::RadiativeOut, -HORIZON);
    assert_eq!(ledger.energy(Channel::RadiativeOut), -10_000_000_000);

    // A day of symmetric exchange looks like no exchange at all, and that is
    // the declared behaviour: the ledger wants the net, and whoever wants the
    // gross flows wants the metric stream (ADR-037, ADR-059).
    ledger.credit_matter(Channel::Impact, 0, HORIZON);
    ledger.credit_matter(Channel::Impact, 0, -HORIZON);
    assert_eq!(ledger.matter(Channel::Impact, 0), 0);
}

// ---------------------------------------------------------------------------
// a_channel_counter_holds_ten_million_lit_ticks_at_full_sun
// ---------------------------------------------------------------------------

/// The energy branch of ADR-083, at the worst case the corpus declares, and it
/// does not run ten million ticks either — it substitutes the boundary value and
/// demands the **exact number** rather than the absence of a panic.
///
/// The arithmetic, entirely from declared quantities: the upper face of a 128
/// cubed domain at `dx = 1e-4 m` is `128^2 * (1e-4)^2 = 1.6384e-4 m^2`; full sun
/// of `1e3 W/m^2` over a one-second tick is `0.16384 J`; and `k_E = 67` (ADR-062)
/// makes a joule `2^67` units, so one lit tick is `2^81/10^5 = 2.4179e19` units.
/// Over `10^7` ticks — the upper end of the horizon ADR-004 declares — that is
/// `2.4179e26`.
///
/// `SOLAR_IN` has no cancellation of any kind: it counts what was absorbed and
/// not a net (ADR-059), so it grows monotonically wherever absorption is
/// non-negative. This branch has no long horizon to hide behind, which is why it
/// is the one that fixes the width.
#[test]
fn a_channel_counter_holds_ten_million_lit_ticks_at_full_sun() {
    /// `floor(2^81 / 10^5)`, the units of storage one lit tick delivers.
    const PER_TICK: i128 = 24_178_516_392_292_583_494;
    const TICKS: i128 = 10_000_000;

    let mut ledger = Ledger::new(1).unwrap();
    ledger.credit_energy(Channel::SolarIn, PER_TICK * TICKS);
    assert_eq!(
        ledger.energy(Channel::SolarIn),
        241_785_163_922_925_834_940_000_000,
        "the counter lost digits at the declared horizon of full sun"
    );

    // The counterpoint, without which the assertion above says nothing: the same
    // number does not fit an `i64` at all, and neither does **one tick** of it.
    assert!(i64::try_from(ledger.energy(Channel::SolarIn)).is_err());
    assert!(i64::try_from(PER_TICK).is_err());

    // 2.62144 ceilings in a single tick, and the number is exact: `2^18 * 1e-5`.
    // Written as a ratio of integers so that no `f64` enters this file, and read
    // back off the counter so that it is the ledger being asserted about.
    let mut one_tick = Ledger::new(1).unwrap();
    one_tick.credit_energy(Channel::SolarIn, PER_TICK);
    let per_tick = one_tick.energy(Channel::SolarIn);
    assert!(per_tick * 100_000 > 262_143 * (1i128 << 63));
    assert!(per_tick * 100_000 < 262_145 * (1i128 << 63));

    // Thirty-nine free bits after the whole horizon: `2.4179e26 < 2^88 < 2^127`.
    let whole = ledger.energy(Channel::SolarIn);
    assert!(whole < 1i128 << 88);
    assert!(whole > 1i128 << 87);
}

// ---------------------------------------------------------------------------
// a_boundary_flux_of_one_percent_of_the_pool_survives_the_declared_horizon
// ---------------------------------------------------------------------------

/// The matter branch of ADR-083, on the number that branch is argued from.
///
/// **The honest form of the claim, and it is weaker than the draft's.** A
/// counter is signed and holds the *net* (ADR-059), one per `(channel,
/// substance)` pair over every face, so a lid that brings a percent of the pool
/// in and takes the same amount out does not move it at all: a day of symmetric
/// exchange looks exactly like no exchange. Monotonic growth needs a **through
/// flow** — matter entering by another channel (a vent, `GEOTHERMAL_IN`,
/// `VENT_BURST`) or being made by a reaction inside, and leaving through
/// `BOUNDARY_EXCHANGE`. So: this branch is reachable with a through flow running,
/// chemical or geothermal, and is not reachable by a lid on its own. Without one,
/// what bounds the net outflow is the capacity of the domain (ADR-006) and not
/// the width of the counter.
///
/// The number stands either way. Water holds `5.12e11` units per voxel (ADR-040)
/// and the face of a 128 cubed domain carries 16 384 voxels, so a through flow of
/// one percent of a surface voxel's pool is `1.6384e4 * 5.12e9 = 8.389e13` units
/// a tick.
#[test]
fn a_boundary_flux_of_one_percent_of_the_pool_survives_the_declared_horizon() {
    /// One percent of the water pool of a surface voxel, over the whole face.
    const PER_TICK: i128 = 83_886_080_000_000;
    const TICKS: i128 = 10_000_000;

    let mut ledger = Ledger::new(1).unwrap();
    ledger.credit_matter(Channel::BoundaryExchange, 0, -PER_TICK * TICKS);
    assert_eq!(
        ledger.matter(Channel::BoundaryExchange, 0),
        -838_860_800_000_000_000_000,
        "the counter lost digits at the declared horizon"
    );
    assert!(i64::try_from(ledger.matter(Channel::BoundaryExchange, 0)).is_err());

    // Where an `i64` would have died, counted rather than incanted: `i64::MAX`
    // divided by the per-tick flow is tick 109 951 — eleven percent of `10^6`,
    // the *shortest* horizon ADR-004 declares.
    let at_tick = i128::from(i64::MAX) / PER_TICK;
    assert_eq!(at_tick, 109_951);
    // Eleven percent of `10^6`, in whole numbers so that no `f64` enters this
    // file: `10.995%`, which is over a tenth and under an eighth of the horizon.
    assert!(at_tick * 100 > 10 * 1_000_000);
    assert!(at_tick * 100 < 12 * 1_000_000);

    // And it is not a rounding: the flow of the next tick does not fit an i64.
    assert!(i64::try_from(PER_TICK * (at_tick + 1)).is_err());

    // Signed and net, which is the whole reason the claim above needed weakening:
    // an equal exchange the other way puts the counter back at zero.
    ledger.credit_matter(Channel::BoundaryExchange, 0, PER_TICK * TICKS);
    assert_eq!(ledger.matter(Channel::BoundaryExchange, 0), 0);
}

// ---------------------------------------------------------------------------
// a_closed_domain_leaves_every_channel_counter_at_zero
// ---------------------------------------------------------------------------

/// The eco regime of SPEC section 1.7, and the fastest thing in the registry.
const DT: f64 = 1.0;
const DX: f64 = 1.0e-4;
const D_PROTON: f64 = 9.3e-9;

/// A grid with every face periodic — the only kind that can be built today,
/// and the only kind that is genuinely closed: `exchange` is refused at load
/// time and everything else carries no flux (`world::Grid::new`).
fn torus(nx: u32, ny: u32, nz: u32) -> Grid {
    Grid::new(nx, ny, nz, [Boundary::Periodic; 6]).unwrap()
}

fn seed_32(field: &mut Field32, grid: &Grid, amount: impl Fn(u32, u32, u32) -> i32) {
    for idx in 0..field.n_voxels() {
        let (x, y, z) = grid.coords(idx);
        field.write_mut()[idx as usize] = M32::new(amount(x, y, z));
    }
    field.swap();
}

fn seed_64(field: &mut Field64, grid: &Grid, amount: impl Fn(u32, u32, u32) -> i64) {
    for idx in 0..field.n_voxels() {
        let (x, y, z) = grid.coords(idx);
        field.write_mut()[idx as usize] = M64::new(amount(x, y, z));
    }
    field.swap();
}

/// `ACCEPTANCE.md`, section "Conservation". ADR-059 calls this one of the two
/// cheapest and nastiest tests in its list: it catches an **internal** process
/// crediting a channel.
///
/// Nothing in this run crosses the boundary of the domain, so after every tick
/// two things must hold at once, and they are not the same statement. Both
/// residuals are exact zeroes, and all ninety counters are still untouched. A
/// transport kernel that credited the flux across an interior face to
/// `BOUNDARY_EXCHANGE` would keep the first (the flux is symmetric, so the
/// credits would cancel over the domain) and break the second.
///
/// The run is the real `process::Diffuse` over the real fields, not a hand-made
/// delta: a ledger judged against a mock is a ledger judged against the same
/// assumption twice.
///
/// **And the domain sum is asserted to be a known non-zero number before
/// anything else happens.** A `DomainSums` nobody fed balances perfectly —
/// `0 - 0 == 0` — so a zero residual means nothing until the left side is shown
/// to be counting something. The same guard is already used in
/// `acceptance_diffusion.rs`.
#[test]
fn a_closed_domain_leaves_every_channel_counter_at_zero() {
    const L: u32 = 8;
    const TICKS: u32 = 100;
    /// Two substances: the narrow width and the wide one. The reduction walks
    /// them separately and a table indexed the wrong way round would mix them.
    const N_SUBSTANCES: u32 = 2;
    const NARROW: u32 = 0;
    const WIDE: u32 = 1;

    const NARROW_BACKGROUND: i32 = 1_000;
    const WIDE_BACKGROUND: i64 = 5_100_000_000_000;
    const ENTHALPY_PER_VOXEL: i32 = 4_000;
    /// A pool near the `i32` ceiling in one voxel, so that the flux across each
    /// of its faces is large enough to have no spare bits in the `f32` it passes
    /// through.
    const POOL: i32 = 2_000_000_000;

    let grid = torus(L, L, L);
    let n_voxels = i128::from(grid.n_voxels());
    let phase = DiffusePhase::new_32(&grid, 1, &[D_PROTON], DT, DX, SEALED).unwrap();
    let wide_phase = DiffusePhase::new_64(&grid, 1, &[D_PROTON], DT, DX, SEALED).unwrap();

    // The checkerboard, with a pool dropped into one voxel of it.
    //
    // The checkerboard is the one state that keeps every face of every voxel
    // carrying flux for the whole run, so that the counters are asked the
    // question on every tick rather than after the field has gone flat. On its
    // own, though, it is not enough here: at this alpha it cycles between
    // amplitudes 7 and -5 with a period of two substeps, and a tick is six of
    // them, so tick by tick the narrow field would come back **identical** — a
    // hundred ticks of perfect zeroes with the last assertion of this test
    // unable to tell that from a hundred ticks of nothing happening at all. The
    // pool spreads monotonically and is what makes the state at the end differ
    // from the state at the start.
    let mut narrow: Field32 = Field::new(&grid, 1).unwrap();
    seed_32(&mut narrow, &grid, |x, y, z| {
        if (x, y, z) == (4, 4, 4) {
            POOL
        } else if (x + y + z) % 2 == 0 {
            NARROW_BACKGROUND + 7
        } else {
            NARROW_BACKGROUND - 7
        }
    });

    let mut wide: Field64 = Field::new(&grid, 1).unwrap();
    seed_64(&mut wide, &grid, |x, y, z| {
        WIDE_BACKGROUND + i64::from(x * 100 + y * 10 + z)
    });

    // Enthalpy: no process in this run touches it, so the energy half of the
    // invariant closes with a delta of exactly zero — against a left side that
    // is emphatically not zero.
    let enthalpy = vec![M32::new(ENTHALPY_PER_VOXEL); grid.n_voxels() as usize];

    let mut ledger = Ledger::new(N_SUBSTANCES).unwrap();
    let mut before = DomainSums::new(N_SUBSTANCES).unwrap();
    let mut after = DomainSums::new(N_SUBSTANCES).unwrap();

    let reduce = |sums: &mut DomainSums, narrow: &Field32, wide: &Field64| {
        sums.clear();
        sums.add_field_lane_32(NARROW, narrow.lane(0));
        sums.add_field_lane_64(WIDE, wide.lane(0));
        sums.add_enthalpy_lane_32(&enthalpy);
    };

    // Before anything: the left side is a known, non-zero number on all three
    // accumulators.
    reduce(&mut before, &narrow, &wide);
    assert_eq!(
        before.matter(NARROW),
        // The checkerboard cancels exactly — 256 voxels each way — and the pool
        // replaced one voxel that held `background + 7`.
        i128::from(NARROW_BACKGROUND) * n_voxels - 1_007 + i128::from(POOL)
    );
    assert_eq!(
        before.matter(WIDE),
        // The gradient sums to `64 * (100 + 10 + 1) * (0 + ... + 7)`.
        i128::from(WIDE_BACKGROUND) * n_voxels + 198_912
    );
    assert_eq!(before.energy(), i128::from(ENTHALPY_PER_VOXEL) * n_voxels);
    let narrow_at_start = narrow.lane(0).to_vec();
    let wide_total = before.matter(WIDE);

    for tick in 0..TICKS {
        reduce(&mut before, &narrow, &wide);

        ledger.begin_tick();
        run_diffusion_32(&phase, &mut narrow);
        run_diffusion_64(&wide_phase, &mut wide);

        reduce(&mut after, &narrow, &wide);

        assert_eq!(
            ledger.residual_matter(Nu::EMPTY, NARROW, &before, &after),
            0,
            "tick {tick}: the narrow substance did not close"
        );
        assert_eq!(
            ledger.residual_matter(Nu::EMPTY, WIDE, &before, &after),
            0,
            "tick {tick}: the wide substance did not close"
        );
        assert_eq!(
            ledger.residual_energy(&before, &after),
            0,
            "tick {tick}: energy did not close"
        );
        ledger.assert_closed(Nu::EMPTY, &before, &after);

        // And nothing was credited to anything. Ninety numbers at fourteen
        // substances; six times two plus six here.
        for channel in Channel::ALL {
            for substance in 0..N_SUBSTANCES {
                assert_eq!(
                    ledger.matter(channel, substance),
                    0,
                    "tick {tick}: an interior flux was credited to {} for \
                     substance {substance}",
                    channel.name()
                );
                assert_eq!(ledger.matter_this_tick(channel, substance), 0);
            }
            assert_eq!(
                ledger.energy(channel),
                0,
                "tick {tick}: energy was credited to {}",
                channel.name()
            );
            assert_eq!(ledger.energy_this_tick(channel), 0);
        }
    }

    assert_eq!(Channel::ALL.len(), CHANNEL_COUNT);

    // The run did something. Without this the hundred ticks above could have
    // been a hundred ticks of nothing moving, and every zero would be honest
    // and empty.
    assert_ne!(
        narrow.lane(0),
        narrow_at_start.as_slice(),
        "the field never moved, so the residuals proved nothing"
    );
    assert_eq!(after.matter(WIDE), wide_total);
}

// ---------------------------------------------------------------------------
// domain_sums_do_not_overflow_for_water_at_256_cubed
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md`, section "Conservation". The name says the substance and the
/// grid on purpose, and this is the reason (ADR-059):
///
/// The worst case for a domain sum is not the 32-bit substance it looks like.
/// Water is the one 64-bit substance of the registry (ADR-040) and holds
/// `5.12e11` units per voxel; at 256 cubed — the target SPEC section 13 already
/// declares — its domain sum is `8.59e18`, which is 93% of `i64::MAX`. The
/// headroom does not run out at some theoretical limit, it runs out on the
/// roadmap. On a 32-bit substance at 128 cubed the same test passes with an
/// `i64` accumulator and with an `i128` one alike, i.e. checks nothing, and the
/// last assertion below is that statement in numbers.
///
/// The sum is accumulated in tiles rather than out of one buffer. What is under
/// test is the width of the accumulator, not the ability of CI to allocate
/// 134 MB — and `add_field_lane_64` is the real door either way.
#[test]
fn domain_sums_do_not_overflow_for_water_at_256_cubed() {
    /// The ceiling of water in storage units, per voxel (ADR-040).
    const WATER_PER_VOXEL: i64 = 512_000_000_000;
    const EDGE: i64 = 256;
    const VOXELS: i64 = EDGE * EDGE * EDGE;
    const TILE: usize = 65_536;
    const WATER: u32 = 0;

    let mut sums = DomainSums::new(1).unwrap();
    let tile = vec![M64::new(WATER_PER_VOXEL); TILE];
    for _ in 0..VOXELS / TILE as i64 {
        sums.add_field_lane_64(WATER, &tile);
    }

    // (1) Exact, to the last digit. `5.12e11 * 16 777 216`.
    let water = sums.matter(WATER);
    assert_eq!(water, 8_589_934_592_000_000_000);

    // (2) 93% of the width an `i64` accumulator would have had.
    assert_eq!(water * 100 / i128::from(i64::MAX), 93);
    assert!(i64::try_from(water).is_ok());

    // Past `2^53` by five orders of magnitude, which is why nothing on this
    // path may pass through an `f64`: a difference of two such sums would stop
    // being exact while still looking entirely reasonable.
    assert!(water > (1i128 << 53));

    // (3) And water is not all of the left side of even one substance. Guild
    // fields and the `struct_mass` of the cell table are moles of `BIOMASS`
    // (ADR-059) and land on their substance's accumulator the same way; fed
    // here at boundary values, they carry the sum past `i64::MAX`.
    //
    // The two values are substituted, not physical, and deliberately so: no
    // ceiling on `guild[g]` or on a cell's `struct_mass` is declared anywhere,
    // the widths are derived by a loader that does not exist yet (ADR-039,
    // ADR-040), and inventing a physical number here would be a fact nobody
    // decided. They are boundary values of the right order, in the same sense
    // as the 10^10 of `channel_counters_do_not_overflow_at_1e7_ticks`.
    //
    // TODO(ledger-ceilings): and they are load-bearing, which is why this
    // carries a marker rather than only a paragraph. Water at 256 cubed is
    // 8.59e18 and fits `i64` at 93%; what carries the total past the edge is
    // `CELL_STRUCT_MASS * CELLS = 6.5e17`. When the loader of ADR-039 and
    // ADR-040 derives the real ceiling for `BIOMASS` and it comes out smaller,
    // this test starts failing for a reason that has nothing to do with the
    // width of an accumulator. Whoever re-tunes these constants: the assertion
    // that must survive is `i64::try_from(total).is_err()` — the exact totals
    // below are arithmetic, that one is the point of the test.
    //
    // That water and biomass share an index here is an artefact of the same
    // choice: what is under test is that one substance's left side takes three
    // doors and the accumulator survives their sum.
    const GUILD_PER_VOXEL: i32 = 2_000_000_000;
    const GUILD_VOXELS: i64 = 4_194_304;
    const CELLS: usize = 500_000;
    const CELL_STRUCT_MASS: i64 = 1_300_000_000_000;

    let guild_tile = vec![M32::new(GUILD_PER_VOXEL); TILE];
    for _ in 0..GUILD_VOXELS / TILE as i64 {
        sums.add_guild_lane_32(WATER, &guild_tile);
    }
    sums.add_cell_masses_64(WATER, &vec![M64::new(CELL_STRUCT_MASS); CELLS]);

    let total = sums.matter(WATER);
    assert_eq!(total, 9_248_323_200_000_000_000);
    assert!(
        i64::try_from(total).is_err(),
        "an i64 accumulator wraps here, silently in release, and the phantom \
         residual that follows looks like a leak in transport"
    );
    assert_eq!(total - water, 658_388_608_000_000_000);

    // (4) The control, and the reason the name of this test carries both the
    // substance and the grid. A 32-bit substance at its ceiling of `2^31/8`
    // units per voxel over 128 cubed sums to `2^49`: it clears `i32` by exactly
    // `2^18`, which is what makes 64 bits look like the answer, and then stops
    // 16 383 times short of `i64::MAX`. Written against that case, this test is
    // green with an `i64` accumulator and with an `i128` one, and proves
    // nothing.
    const NARROW_PER_VOXEL: i32 = 268_435_456;
    const NARROW_VOXELS: i64 = 128 * 128 * 128;

    let mut control = DomainSums::new(1).unwrap();
    let narrow_tile = vec![M32::new(NARROW_PER_VOXEL); TILE];
    for _ in 0..NARROW_VOXELS / TILE as i64 {
        control.add_field_lane_32(0, &narrow_tile);
    }
    let control = control.matter(0);
    assert_eq!(control, 562_949_953_421_312);
    assert_eq!(control / (i128::from(i32::MAX) + 1), 262_144);
    assert_eq!(i128::from(i64::MAX) / control, 16_383);
    assert_eq!(water / control, 15_258);
}

// ---------------------------------------------------------------------------
// load_reports_the_chemical_energy_of_the_domain_in_joules
// ---------------------------------------------------------------------------

/// The shipped scenario, read from `configs/` rather than written here.
///
/// This test is about the numbers of a scenario that exists, so it has to load
/// the one that does. It goes through `config::derive` and not through
/// `config::validate` for the reason `acceptance_fold.rs` gives about its own
/// pair: the derivation is the one door to these quantities.
fn shipped_scenario() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../configs/scenarios/h2s-oxidation.toml"
    );
    std::fs::read_to_string(path).expect("the shipped scenario")
}

fn derived_at(text: &str) -> liminis_core::config::Derived {
    let config = liminis_core::config::parse(text).expect("the scenario parses");
    liminis_core::config::derive(&config).expect("the scenario derives")
}

/// `ACCEPTANCE.md`, section "Conservation" (ADR-081, and the divergence from
/// frozen SPEC section 2.1).
///
/// SPEC section 2.1 writes the left side of the invariant as
/// `Delta(sum fields + sum cells)` — an **unweighted** sum. After ADR-081 the
/// energy half of it is weighted: the domain holds `H_field + Sum_s w_s * n_s`,
/// and the chemical term is seventy-six times the whole declared span of the
/// enthalpy field. The spec is frozen and is not edited (ADR-032), so the
/// divergence lives here, as a number the loader prints.
///
/// The three numbers are `1 757 J`, `23.1 J` and `76`, and the middle one is the
/// **full** declared span `2 * H_max * n_cells`. Half of it — `H_max * n_cells`,
/// which is what "the field holds H_max" invites — prints a ratio of 152 and is
/// invisible without the literal.
#[test]
fn load_reports_the_chemical_energy_of_the_domain_in_joules() {
    let d = derived_at(&shipped_scenario());
    let energy = d.energy();

    // The weights the sum is taken over, so that a plausible total cannot come
    // out of implausible parts. Water carries 99.8% of it and appears in no
    // reaction at all, which is the price ADR-081 names: its
    // `enthalpy_formation` is now load-bearing and nothing checks it.
    let weights = d.chemical_weights();
    let named = |id: &str| -> i64 {
        let s = d
            .substances()
            .iter()
            .position(|s| s.id == id)
            .unwrap_or_else(|| panic!("no substance `{id}`"));
        weights[s]
    };
    assert_eq!(named("WATER"), -9_366_077_440, "-285830 J/mol at k = 52");
    assert_eq!(
        named("H2S"),
        -19_850,
        "-39700 J/mol at k = 68, halved exactly"
    );
    assert_eq!(named("O2"), -5_850);
    assert_eq!(named("SO4"), -29_096_640);
    assert_eq!(
        named("H_ION"),
        0,
        "zero by the single-ion convention, and the one substance above k_E"
    );

    let chemical = energy.chemical_energy_joules;
    let span = energy.field_span_joules;
    assert!(
        (1750.0..1765.0).contains(&chemical.abs()),
        "the domain holds {chemical:e} J of chemical energy, not 1757 J"
    );
    assert!(chemical < 0.0, "formation enthalpies are negative");
    assert!(
        (23.0..23.2).contains(&span),
        "the declared field span is {span:e} J, not 23.1 J"
    );
    let ratio = chemical.abs() / span;
    assert!(
        (75.0..77.0).contains(&ratio),
        "the ratio is {ratio}, and 152 means the half span was used"
    );

    let report = d.report();
    assert!(report.contains("chemical energy of the domain"), "{report}");
    assert!(report.contains("2*H_max*n_cells"), "{report}");

    // The ratio is a property of the registry and not of the grid: both terms
    // scale with the number of voxels, one through `n_voxels` and the other
    // through `n_cells`, so a scenario cannot make the chemical term look small
    // by shrinking the world.
    let bigger = derived_at(
        &shipped_scenario()
            .replace("nx = 48", "nx = 128")
            .replace("ny = 48", "ny = 128")
            .replace("nz = 48", "nz = 128"),
    );
    let big_ratio =
        bigger.energy().chemical_energy_joules.abs() / bigger.energy().field_span_joules;
    assert!(
        (big_ratio - ratio).abs() < 0.1,
        "the ratio moved from {ratio} to {big_ratio} with the grid"
    );
    assert!(
        (33_000.0..33_600.0).contains(&bigger.energy().chemical_energy_joules.abs()),
        "at 128 cubed the chemical term is {:e} J, not 33.3 kJ",
        bigger.energy().chemical_energy_joules
    );

    // And the domain fits the ledger with room: the shipped scenario stands at
    // seventy-eight bits of the hundred and twenty-seven the accumulator has.
    assert!(
        (77.0..79.0).contains(&energy.chemical_energy_bits),
        "the shipped scenario stands at {} bits",
        energy.chemical_energy_bits
    );
}
