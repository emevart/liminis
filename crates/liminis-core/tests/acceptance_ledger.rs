//! Acceptance criteria of the ledger: channel counters and the two residuals
//! (`ACCEPTANCE.md`, section "Conservation"; ADR-059, ADR-003, ADR-028).
//!
//! ```text
//! channel_counters_do_not_overflow_at_1e7_ticks
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
//! these three cannot: the sign convention, the tick delta against the running
//! total, the shape of the counter table. Neither set subsumes the other.

use liminis_core::ledger::{CHANNEL_COUNT, Channel, DomainSums, Ledger};
use liminis_core::numeric::{M32, M64};
use liminis_core::process::Diffuse;
use liminis_core::world::{Boundary, Field, Field32, Field64, Grid};

// ---------------------------------------------------------------------------
// channel_counters_do_not_overflow_at_1e7_ticks
// ---------------------------------------------------------------------------

/// `ACCEPTANCE.md` says in as many words that this test does not run ten
/// million ticks: it checks the width of the counter and substitutes the
/// boundary value. `QUANTITIES.md` section 3 supplies both numbers — a thousand
/// units a tick over the horizon of 10^7 ticks that SPEC section 13 declares as
/// the readiness criterion of S0.
///
/// What overflow would mean here is not a lost low bit. It is a counter that
/// comes back a plausible number seven times too small, or — a little further
/// along the same run — a number of the opposite sign, which reads as an
/// outflow where there was an inflow. The ledger would go on closing, against
/// the wrong right-hand side.
#[test]
fn channel_counters_do_not_overflow_at_1e7_ticks() {
    const PER_TICK: i64 = 1_000;
    const TICKS: i64 = 10_000_000;
    const HORIZON: i64 = PER_TICK * TICKS;

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
    let diffuse = Diffuse::new(&grid, D_PROTON, DT, DX).unwrap();

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
    let narrow_at_start = narrow.read().to_vec();
    let wide_total = before.matter(WIDE);

    for tick in 0..TICKS {
        reduce(&mut before, &narrow, &wide);

        ledger.begin_tick();
        diffuse.apply_32(&mut narrow);
        diffuse.apply_64(&mut wide);

        reduce(&mut after, &narrow, &wide);

        assert_eq!(
            ledger.residual_matter(NARROW, &before, &after),
            0,
            "tick {tick}: the narrow substance did not close"
        );
        assert_eq!(
            ledger.residual_matter(WIDE, &before, &after),
            0,
            "tick {tick}: the wide substance did not close"
        );
        assert_eq!(
            ledger.residual_energy(&before, &after),
            0,
            "tick {tick}: energy did not close"
        );
        ledger.assert_closed(&before, &after);

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
        narrow.read(),
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
