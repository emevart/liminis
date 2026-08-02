//! Acceptance criterion of the pressure **process** (`ACCEPTANCE.md`,
//! "Conservation"):
//!
//! ```text
//! pressure_relaxation_reaches_hydrostatic_equilibrium
//! ```
//!
//! # Two tests of one name, and what each of them is for
//!
//! A test of this name already exists in `src/kernels/pressure.rs`. That one runs
//! **one lane of one width** through the two kernels by hand and asserts three
//! things: every step conserves, the spread of the overflow field never grows,
//! and the chain reaches a fixed point after moving. It is a statement about the
//! arithmetic of the scheme.
//!
//! This one is a statement about the **process boundary**. It runs
//! `process::pressure::Pressure::apply` over a world with lanes in *both* storage
//! widths and adds two assertions the kernel-level test cannot make:
//!
//! - every lane of every field is relaxed against **one** overflow field, taken
//!   once from the snapshot `N` — so the result does not depend on the order the
//!   lanes are visited in (ADR-041's "recomputing availability after every
//!   reaction" wearing another hat);
//! - after the call the **front** buffer holds the new state for every lane
//!   (ADR-057). Pressure runs exactly one substep per lane, so every lane is odd,
//!   so the repair is one swap and a copy of length zero — and a lane forgotten
//!   there comes back holding the write buffer: the previous phase's state, and
//!   zeroes on the first tick, which is a legal amount no invariant objects to.
//!
//! `cargo test pressure_relaxation_reaches_hydrostatic_equilibrium` runs both.
//! Neither subsumes the other, and the weaker one must not be read as the
//! criterion: it does not know that lanes exist.
//!
//! Outside the crate rather than beside the process, for the reason
//! `acceptance_light.rs` states: an acceptance test is the outside view and may
//! use only the public surface of `liminis-core`.

use liminis_core::numeric::{M32, M64, Q};
use liminis_core::process::pressure::{Occupant, Pressure};
use liminis_core::world::{Boundary, Field, Field32, Field64, Grid, LaneRef};

/// A deliberately non-cubic grid: on a cubic one every bug that swaps two axes is
/// invisible, because all three strides are equal.
const NX: u32 = 3;
const NY: u32 = 4;
const NZ: u32 = 5;
const N_VOXELS: u32 = NX * NY * NZ;

/// Two narrow lanes and one wide one. The wide lane is what makes the branch on
/// storage width load-bearing: skipping water because it is the one `i64`
/// substance and goes down the other branch is the concrete way to relax "every
/// lane" and mean "most of them" (`kernels/pressure.rs`).
const LANES_32: u32 = 2;
const LANES_64: u32 = 1;

/// The limiting overflow ADR-055 derives the mobility from.
///
/// Not a number from the corpus, and it cannot be: `theta_max` is declared
/// nowhere (`TODO(theta-max)` in `kernels/pressure.rs`). A test may pick one
/// precisely because the process takes it as an argument rather than reading it
/// from a config that has no key for it. The value is picked against the
/// linearised stability limit the kernel writes out —
/// `6*courant_per_overflow*(theta + 1) <= 1`, the third condition of
/// `TODO(courant-condition)`, which neither of the two conditions of SPEC section
/// 4.2 implies. With the occupancies below the occupancy of a voxel stays under
/// two, so `6*2/24 = 0.5 <= 1` with a factor of two in hand. Above that limit
/// neighbouring voxels swap pools every tick, conserving matter exactly and
/// leaving every other assertion in this file green.
const THETA_MAX: f64 = 24.0;

/// The voxel volume, m^3. `V_bar/(units_per_mol*V_voxel)` is what the process
/// folds, so any consistent triple gives an occupancy of the size this test
/// wants; these are chosen so that one storage unit is exactly `2^-13` of a
/// voxel, every occupancy below is a small dyadic number, and the total stays
/// inside the linearised limit of [`THETA_MAX`].
const V_VOXEL: f64 = 1.0;
const UNITS_PER_MOL: f64 = 8192.0;
const V_BAR: f64 = 1.0;

fn floored() -> Grid {
    Grid::new(
        NX,
        NY,
        NZ,
        [
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Periodic,
            Boundary::Closed,
            Boundary::Closed,
        ],
    )
    .unwrap()
}

/// The occupancy table: every lane of both fields, which is the only reading that
/// matches ADR-006 — every substance takes up room.
fn occupants() -> Vec<Occupant> {
    let mut table = Vec::new();
    for lane in 0..LANES_32 {
        table.push(Occupant {
            lane: LaneRef::Narrow(lane),
            partial_molar_volume: V_BAR,
            units_per_mol: UNITS_PER_MOL,
        });
    }
    for lane in 0..LANES_64 {
        table.push(Occupant {
            lane: LaneRef::Wide(lane),
            partial_molar_volume: V_BAR,
            units_per_mol: UNITS_PER_MOL,
        });
    }
    table
}

/// An initial state whose lanes hold **different** numbers and whose occupancy
/// varies along all three axes.
///
/// Both properties are load-bearing. Equal lanes make a process that relaxes one
/// lane and copies it to the others indistinguishable from a correct one; a state
/// constant along Z leaves every Z face with a zero courant, and the floor and
/// the lid — the only walls the project actually runs with — are then never
/// exercised at all.
fn initial() -> (Field32, Field64) {
    let grid = floored();
    let mut narrow = Field::new(&grid, LANES_32).unwrap();
    let mut wide = Field::new(&grid, LANES_64).unwrap();

    for lane in 0..LANES_32 {
        let base = (lane * N_VOXELS) as usize;
        for at in 0..N_VOXELS as usize {
            let amount = 1024 * (1 + (at as i32 * 7 + lane as i32 * 3) % 5);
            narrow.write_mut()[base + at] = M32::new(amount);
        }
    }
    for at in 0..N_VOXELS as usize {
        wide.write_mut()[at] = M64::new(1024 * i64::from((at as u32 * 3) % 7));
    }
    // The state was written into the back buffer; a swap puts it where state `N`
    // belongs. Writing `read()` directly is impossible by construction, which is
    // the point of the type.
    narrow.swap();
    wide.swap();
    (narrow, wide)
}

fn totals_32(field: &Field32) -> Vec<i64> {
    (0..field.lanes())
        .map(|lane| field.lane(lane).iter().map(|v| v.to_i64()).sum())
        .collect()
}

fn totals_64(field: &Field64) -> Vec<i64> {
    (0..field.lanes())
        .map(|lane| field.lane(lane).iter().map(|v| v.to_i64()).sum())
        .collect()
}

fn spread_of(field: &[Q]) -> f64 {
    let mut lowest = f64::INFINITY;
    let mut highest = f64::NEG_INFINITY;
    for value in field {
        lowest = lowest.min(value.debug_f64());
        highest = highest.max(value.debug_f64());
    }
    highest - lowest
}

#[test]
fn pressure_relaxation_reaches_hydrostatic_equilibrium() {
    // The name promises more than the mechanism has, and `kernels/pressure.rs`
    // says so at length: there is no `g` in any formula of ADR-055, gravity
    // enters only the settling velocity of step `f`, so "hydrostatic" here means
    // the fixed point of a uniform occupancy and nothing more.
    let grid = floored();
    let table = occupants();
    let pressure = Pressure::new(&grid, THETA_MAX, &table, V_VOXEL).unwrap();

    let (mut narrow, mut wide) = initial();
    let start_32 = totals_32(&narrow);
    let start_64 = totals_64(&wide);

    let mut overflow = vec![Q::ZERO; N_VOXELS as usize];
    pressure.overflow(narrow.read(), wide.read(), &mut overflow);
    let mut spread = spread_of(&overflow);
    let opening = spread;
    let mut settled = false;

    for step in 0..4096 {
        let before_32: Vec<M32> = narrow.read().to_vec();
        let before_64: Vec<M64> = wide.read().to_vec();

        pressure.apply(Some(&mut narrow), Some(&mut wide), &mut overflow);

        // (1) Every lane conserves exactly, on every step of the chain. Per lane
        // and not over the field: a scheme that moved matter between lanes would
        // keep the field's total and change the composition of every voxel.
        assert_eq!(totals_32(&narrow), start_32, "narrow lanes at step {step}");
        assert_eq!(totals_64(&wide), start_64, "wide lanes at step {step}");

        // (4) The front buffer holds the state this call produced, for every lane
        // of both fields (ADR-057). Pressure runs one substep per lane, so all
        // lanes are odd, so the repair is one swap and a copy of length zero — and
        // a lane left out of it comes back holding the write buffer, which is the
        // previous phase's state and zeroes on the first tick.
        //
        // Checked as "the front buffer is the *result*" rather than "the front
        // buffer changed": a lane whose net flux came out zero is exactly the case
        // a changed-or-not assertion cannot see, and it is the case that breaks.
        assert_eq!(
            narrow.read().len(),
            (LANES_32 * N_VOXELS) as usize,
            "step {step}"
        );
        let recomputed = replay(&pressure, &before_32, &before_64);
        assert_eq!(
            narrow.read(),
            &recomputed.0[..],
            "narrow front at step {step}"
        );
        assert_eq!(wide.read(), &recomputed.1[..], "wide front at step {step}");

        // (2) The spread of the overflow field never grows: the scheme relaxes
        // rather than oscillating. This is the only assertion here that can see a
        // checkerboard, where two neighbours swap pools every tick while matter is
        // conserved exactly and both axes of the ledger stay green.
        pressure.overflow(narrow.read(), wide.read(), &mut overflow);
        let after = spread_of(&overflow);
        assert!(
            after <= spread,
            "the spread of the overflow grew from {spread} to {after} at step \
             {step}: the scheme is oscillating rather than relaxing"
        );
        spread = after;

        if narrow.read() == &before_32[..] && wide.read() == &before_64[..] {
            settled = true;
            break;
        }
    }

    // (3) It came to a fixed point, and it got there by moving — the second half
    // being what keeps the first from holding over a chain that never started.
    assert!(settled, "the domain never came to a fixed point");
    assert!(
        spread < opening,
        "the overflow ended as spread as it started ({opening} to {spread}): \
         nothing relaxed"
    );
}

/// One application of pressure over a snapshot, assembled outside the process
/// from the process's own two public halves.
///
/// Not a second implementation of the scheme: it calls `overflow` and `apply` on
/// throwaway fields built from the snapshot. What it pins is the property those
/// two calls are supposed to have — that the answer is a function of state `N`
/// alone — and that the front buffer of the real field holds it afterwards.
fn replay(pressure: &Pressure, before_32: &[M32], before_64: &[M64]) -> (Vec<M32>, Vec<M64>) {
    let grid = floored();
    let mut narrow = Field::new(&grid, LANES_32).unwrap();
    let mut wide = Field::new(&grid, LANES_64).unwrap();
    narrow.write_mut().copy_from_slice(before_32);
    wide.write_mut().copy_from_slice(before_64);
    narrow.swap();
    wide.swap();

    let mut overflow = vec![Q::ZERO; N_VOXELS as usize];
    pressure.apply(Some(&mut narrow), Some(&mut wide), &mut overflow);
    (narrow.read().to_vec(), wide.read().to_vec())
}

#[test]
fn the_overflow_is_taken_once_from_the_snapshot_every_lane_relaxes_against() {
    // The lane order must not be world semantics. A process that recomputed the
    // overflow field after every lane — the natural reaction to "pressure should
    // see fresh state" — gives a result that depends on the order, while every
    // lane still conserves exactly and both axes of the ledger close. It is the
    // same class of error ADR-041 closed for reactions.
    //
    // The check runs the process against a field whose lanes have been permuted:
    // relaxing lane 1 before lane 0 must give the permuted answer and nothing
    // else.
    let grid = floored();
    let pressure = Pressure::new(&grid, THETA_MAX, &occupants(), V_VOXEL).unwrap();

    let (mut narrow, mut wide) = initial();
    pressure.apply(
        Some(&mut narrow),
        Some(&mut wide),
        &mut vec![Q::ZERO; N_VOXELS as usize],
    );
    let straight: Vec<M32> = narrow.read().to_vec();

    // The same state with the two narrow lanes exchanged. The occupancy is a sum
    // over lanes and so is unchanged by the swap; therefore the relaxed state has
    // to be the straight answer with the same two lanes exchanged, exactly.
    let (mut swapped, mut wide) = initial();
    swap_lanes(&mut swapped, 0, 1);
    pressure.apply(
        Some(&mut swapped),
        Some(&mut wide),
        &mut vec![Q::ZERO; N_VOXELS as usize],
    );
    let mut expected = straight.clone();
    swap_lane_slices(&mut expected, 0, 1);
    assert_eq!(swapped.read(), &expected[..]);

    // And the two lanes really do differ, or the equality above is a tautology.
    let lane_0 = &straight[0..N_VOXELS as usize];
    let lane_1 = &straight[N_VOXELS as usize..2 * N_VOXELS as usize];
    assert_ne!(lane_0, lane_1);
}

fn swap_lanes(field: &mut Field32, a: u32, b: u32) {
    let mut state = field.read().to_vec();
    swap_lane_slices(&mut state, a, b);
    field.write_mut().copy_from_slice(&state);
    field.swap();
}

fn swap_lane_slices(state: &mut [M32], a: u32, b: u32) {
    let n = N_VOXELS as usize;
    for at in 0..n {
        state.swap(a as usize * n + at, b as usize * n + at);
    }
}

#[test]
fn pressure_touches_every_lane_of_both_fields() {
    // Every lane of both widths moves under a non-uniform occupancy. Skipping one
    // — water, because it is the single `i64` substance and goes down the other
    // branch — leaves every lane conserving separately, both axes of the ledger
    // closed, and the composition of a voxel drifting.
    let grid = floored();
    let pressure = Pressure::new(&grid, THETA_MAX, &occupants(), V_VOXEL).unwrap();

    let (mut narrow, mut wide) = initial();
    let before_32: Vec<M32> = narrow.read().to_vec();
    let before_64: Vec<M64> = wide.read().to_vec();
    pressure.apply(
        Some(&mut narrow),
        Some(&mut wide),
        &mut vec![Q::ZERO; N_VOXELS as usize],
    );

    for lane in 0..LANES_32 {
        let base = (lane * N_VOXELS) as usize;
        let range = base..base + N_VOXELS as usize;
        assert_ne!(
            narrow.read()[range.clone()],
            before_32[range],
            "narrow lane {lane} was not touched"
        );
    }
    for lane in 0..LANES_64 {
        let base = (lane * N_VOXELS) as usize;
        let range = base..base + N_VOXELS as usize;
        assert_ne!(
            wide.read()[range.clone()],
            before_64[range],
            "wide lane {lane} was not touched"
        );
    }
}

#[test]
fn the_folded_courant_is_the_reciprocal_of_the_limiting_overflow() {
    // ADR-055 derives the mobility so that at the declared limiting overflow the
    // displacement over one tick is exactly one voxel, which fixes the whole
    // product `L*k*dt/dx^2` at `1/theta_max`. The stiffness `k` therefore takes no
    // part in the arithmetic at all — it is not an argument of `Pressure::new`,
    // and that absence is the decision, not an omission.
    let grid = floored();
    let pressure = Pressure::new(&grid, THETA_MAX, &occupants(), V_VOXEL).unwrap();
    assert_eq!(
        pressure.courant_per_overflow(),
        Q::from_f64(1.0 / THETA_MAX)
    );
}
