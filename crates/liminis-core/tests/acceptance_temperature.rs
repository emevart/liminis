//! Acceptance criterion of the temperature operator (`ACCEPTANCE.md`, "Physics"):
//!
//! ```text
//! temperature_from_enthalpy_round_trips
//! ```
//!
//! Outside the crate rather than beside the kernel, for the reason
//! `acceptance_fold.rs` states: an acceptance test is the outside view. It may use
//! only what a scenario can use — the public surface of `liminis-core` — so that
//! it keeps its meaning when the inside is rearranged, and so that it cannot be
//! quietly shaped around a private helper it was supposed to judge.
//!
//! # It runs on the coarse grid, and ADR-062 says so in as many words
//!
//! "Temperature is a quantity of the coarse cell, sixty-four fine voxels share one
//! `T`, and the `q10` factor of a reaction reads the covering cell. That is what
//! `temperature_from_enthalpy_round_trips` has to exercise, or it is not clear
//! which composition it is closing over." So the enthalpy this file plants is
//! enthalpy of a **cell**, the denominator is gathered from the `2^(3*lod)` fine
//! voxels that cell covers, and the round trip is asserted per cell and then
//! re-read through every fine voxel that shares it.
//!
//! The grid is deliberately not the one the kernel's own tests use: `12 x 8 x 16`
//! against their `8 x 12 x 16`, so `nx` and `ny` are swapped as well as different.
//! Coarse `3 x 2 x 4` — three pairwise different extents, none of them one,
//! because a coarse extent of one erases its axis from the shift in silence. Both
//! index mappings below are written out from SPEC sections 1.1 and 1.5 rather than
//! borrowed from anywhere: the kernel keeps private copies of both, every fixture
//! of its unit tests places its inputs through those same copies, and a transposed
//! one is consistent with itself.
//!
//! # What the fixture is built out of
//!
//! Powers of two throughout, so that the arithmetic of the expectation is exact
//! and an assertion is about the operator rather than about `f32`. Two of the
//! three substances are there to make the denominator a sum of **different
//! signs**: aqueous sulfate declares about `-293 J/(mol K)` and that is physics
//! (`configs/scenarios/h2s-oxidation.toml`), while the proton declares exactly
//! zero by the same single-ion convention. A fixture of three positive heat
//! capacities would pass over an operator that took `abs()` somewhere, and the
//! error that hides behind is 0.2% on the corpus composition and qualitative
//! without the solvent.
//!
//! # The composition varies along each axis separately, and it has to
//!
//! Every amount below is a function of `x`, `y` and `z` with a **different
//! weight on each**, and the expectation is not a table: it is the same amounts
//! summed a second time, one fine voxel at a time, through the covering map
//! written out in this file. So "coarse cell `c` holds `C`" is the statement
//! "these sixty-four voxels and no others were gathered into `c`", and it is the
//! only form of the statement that has content.
//!
//! An earlier version of this fixture alternated two compositions on
//! `coarse % 2`, and it certified nothing about the map. The coarse `z` stride is
//! `cnx*cny = 6`, an even number, so two cells differing only in `cz` carried the
//! same composition, the same capacity and the same target temperature: a kernel
//! gathering the wrong `z` slab — `z0 = cz << (lod-1)`, say — came out green
//! through the whole file. That is the exact mistake ADR-062 assigns this
//! criterion, and a wrong covering map is silent everywhere else: `T` is class
//! `Q`, it enters no invariant, both halves of the ledger close, and every
//! reaction of the world runs at the temperature of the wrong slab.

use liminis_core::kernels::temperature::{Heat, TemperatureParams, temperature_cell};
use liminis_core::numeric::{M32, M64, Q};

const NX: u32 = 12;
const NY: u32 = 8;
const NZ: u32 = 16;
const LOD: u32 = 2;

const N_FINE: u32 = NX * NY * NZ;
const N_COARSE: u32 = (NX >> LOD) * (NY >> LOD) * (NZ >> LOD);

/// `2^(3*lod)`: how many fine voxels one coarse cell owns.
const PER_COARSE: i64 = 1 << (3 * LOD);

/// The scenario's zero of enthalpy storage, K (ADR-044).
///
/// Exactly representable and deliberately not 298.15: a `T_ref` that looked like
/// the thermochemical reference of ADR-044 would make the two indistinguishable
/// in every assertion here, and they are unrelated by that record's whole point.
const T_REF: f64 = 256.0;

/// `2^-k_E`: what one storage unit of enthalpy is worth in joules (ADR-062).
///
/// Not the derived `k_E = 67` of the corpus, and it does not have to be: the
/// kernel receives this folded, so an acceptance test may pick a scale on which
/// its own arithmetic is exact. A power of two, so the crossing rounds nothing.
const JOULES_PER_UNIT: f64 = 1.0 / 1024.0;

/// Substance indices, and the lanes they are **not** equal to.
///
/// `WATER` is the wide one and takes lane 0 of the wide field; `SULFATE` takes
/// lane 0 of the narrow one and `PROTON` lane 1. So `lane != s` from the second
/// substance onwards, which is the shape of the registry the project carries and
/// the shape in which `s * n_voxels + idx` is *right* at `s == 0` and wrong after
/// (ADR-056).
const WATER: usize = 0;
const SULFATE: usize = 1;
const PROTON: usize = 2;
const N_SUBSTANCES: u32 = 3;

const LANES: [u32; N_SUBSTANCES as usize] = [0, 0, 1];
const WIDTH_MASK: u32 = 1 << WATER;

/// `c_p[s] * 2^-k[s]`, J/(K * storage unit), indexed by substance.
///
/// Sulfate's is negative and the proton's is exactly zero — both legal, both
/// declared that way in the corpus, and both fatal to an operator that "repairs"
/// the sign or refuses a non-positive coefficient at load.
const CAPACITY: [f64; N_SUBSTANCES as usize] = [
    1.0 / 256.0, // WATER
    -1.0 / 64.0, // SULFATE
    0.0,         // PROTON
];

/// How much of the solvent one fine voxel holds.
///
/// A separate weight on each axis, and the weights are spread far enough apart
/// that no two cubes can collide: a cube sums to
/// `318976 + 4096*cx + 16384*cy + 131072*cz` units, strictly increasing in each
/// coarse coordinate over the extents of this grid. That is what a covering map
/// gathering the wrong slab of any axis runs into, and it is exactly what the
/// old fixture — one composition per parity of the linear index — did not have
/// along `z`.
///
/// A single voxel's amount is not a multiple of 256, and the sum of a cube is:
/// each of the four terms above divides by it. So the cube of amounts is a whole
/// multiple of `1/CAPACITY[WATER] = 2^8` and the one crossing into `Q` rounds
/// nothing.
fn water(x: u32, y: u32, z: u32) -> i64 {
    4096 + 16 * i64::from(x) + 64 * i64::from(y) + 512 * i64::from(z)
}

/// How much of the substance with the **negative** `c_p` one fine voxel holds.
///
/// Weighted per axis as well, and with weights that are not the solvent's, so
/// that a kernel confusing the two lanes cannot land back on the same
/// denominator. It takes between 2.1% and 3.2% off the solvent's term across
/// this grid — an order more than the 0.2% the corpus composition gives it
/// (ADR-062), deliberately, because a term worth 0.2% of a denominator no
/// invariant contains is a term a fixture can drop without noticing.
fn sulfate(x: u32, y: u32, z: u32) -> i64 {
    16 + i64::from(x) + 2 * i64::from(y) + 4 * i64::from(z)
}

/// How much of the proton every voxel holds. Large on purpose and the same
/// everywhere: its `c_p` is exactly zero, so it has to move the denominator by
/// exactly nothing.
const PROTONS: i64 = 1 << 20;

/// How much extra solvent one fine voxel receives when the covering map is put
/// under a probe. `256 * CAPACITY[WATER]` is one whole joule per kelvin, so the
/// cell that receives it cannot come back unchanged through any rounding.
const PROBE: i64 = 256;

/// The temperatures the fixture asks for, as an excess over `T_ref`.
///
/// Dyadic and of both signs, so `H` comes out an exact integer and the round
/// trip is exact rather than merely within tolerance. The negative ones matter:
/// enthalpy is stored relative to an arbitrary zero (ADR-044) and is signed, and
/// a kernel that clamped it would still look right on every warm cell. None of
/// them is zero, so a cell whose denominator moved has a temperature that moved
/// with it.
///
/// Read cyclically, and the cycle is longer than one, so `c` and `c+1` never
/// draw the same entry — which is what the "the cell next door differs" loop
/// leans on. The largest of them keeps `H` inside the exact range: at
/// `C_cell <= 2787.5 J/K` and `2^-k_E = 2^-10 J`, `4 * C_cell / 2^-k_E` is
/// `1.14e7`, under the `2^24` past which an `i64` no longer crosses into an
/// `f32` exactly.
const EXCESS: [f64; 5] = [4.0, -1.0, 0.5, -2.0, 1.0];

/// The excess this cell is put at. Keyed on the linear coarse index, because it
/// is the test's own choice of target and not something the operator has to map;
/// what the operator has to map is the composition, and that is keyed on the
/// three coordinates of the voxel itself.
fn excess(coarse: u32) -> f64 {
    EXCESS[coarse as usize % EXCESS.len()]
}

/// The linear index of a fine voxel, from SPEC section 1.1: `x + y*NX + z*NX*NY`.
///
/// Written out here rather than borrowed. `world::Grid::index` says the same
/// thing and the kernel's private copy is checked against it, but an acceptance
/// test that took its index from either would be agreeing with the code under
/// test by construction.
fn index(x: u32, y: u32, z: u32) -> u32 {
    x + y * NX + z * NX * NY
}

/// The coarse cell covering a fine voxel, from SPEC section 1.5: the shift goes
/// **per axis**, never on the linear index.
fn covering(x: u32, y: u32, z: u32) -> u32 {
    let cnx = NX >> LOD;
    let cny = NY >> LOD;
    (x >> LOD) + (y >> LOD) * cnx + (z >> LOD) * cnx * cny
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
        width_mask: WIDTH_MASK,
        t_ref: Q::from_f64(T_REF),
        joules_per_unit: Q::from_f64(JOULES_PER_UNIT),
    }
}

/// The amounts of the whole domain, laid out through the lane table.
fn amounts() -> (Vec<M32>, Vec<M64>) {
    let mut narrow = vec![M32::ZERO; (2 * N_FINE) as usize];
    let mut wide = vec![M64::ZERO; N_FINE as usize];
    for z in 0..NZ {
        for y in 0..NY {
            for x in 0..NX {
                let idx = index(x, y, z);
                wide[(LANES[WATER] * N_FINE + idx) as usize] = M64::new(water(x, y, z));
                narrow[(LANES[SULFATE] * N_FINE + idx) as usize] =
                    M32::new(i32::try_from(sulfate(x, y, z)).unwrap());
                narrow[(LANES[PROTON] * N_FINE + idx) as usize] =
                    M32::new(i32::try_from(PROTONS).unwrap());
            }
        }
    }
    (narrow, wide)
}

/// What each coarse cell holds of each substance, accumulated **one fine voxel
/// at a time** through the covering map of this file.
///
/// The expectation of the round trip is this and not a table of per-cell
/// numbers, and the difference is the whole of ADR-062's half of the criterion:
/// a table says "cell `c` came out at `C`", which a kernel gathering somebody
/// else's cube can satisfy, while this says "the amounts planted in these
/// sixty-four voxels, and no others, arrived in `c`".
///
/// Summed as integers, so the crossing into `Q` happens once per substance here
/// exactly as it happens once per substance in the kernel — an expectation that
/// added sixty-four `f32` terms would be measuring `f32` and calling the
/// difference a bug.
fn gathered(narrow: &[M32], wide: &[M64]) -> Vec<[i64; N_SUBSTANCES as usize]> {
    let mut units = vec![[0i64; N_SUBSTANCES as usize]; N_COARSE as usize];
    for z in 0..NZ {
        for y in 0..NY {
            for x in 0..NX {
                let idx = index(x, y, z);
                let cell = covering(x, y, z) as usize;
                units[cell][WATER] += wide[(LANES[WATER] * N_FINE + idx) as usize].to_i64();
                units[cell][SULFATE] += narrow[(LANES[SULFATE] * N_FINE + idx) as usize].to_i64();
                units[cell][PROTON] += narrow[(LANES[PROTON] * N_FINE + idx) as usize].to_i64();
            }
        }
    }
    units
}

/// `sum(n_i * c_p_i)` of one cell, J/K, from the units gathered into it.
fn capacity_of(units: &[i64; N_SUBSTANCES as usize]) -> f64 {
    units[WATER] as f64 * CAPACITY[WATER]
        + units[SULFATE] as f64 * CAPACITY[SULFATE]
        + units[PROTON] as f64 * CAPACITY[PROTON]
}

/// The whole dispatch, the way a host runs it: one invocation per coarse cell of
/// the enthalpy grid, and never one per fine voxel.
fn dispatch(narrow: &[M32], wide: &[M64], enthalpy: &[M64]) -> (Vec<Q>, Vec<Q>) {
    let p = params();
    let heat = Heat {
        lane: &LANES,
        capacity_per_unit: &CAPACITY.map(Q::from_f64),
    };
    let mut capacity = vec![Q::ZERO; N_COARSE as usize];
    let mut temperature = vec![Q::ZERO; N_COARSE as usize];
    for coarse in 0..N_COARSE {
        temperature_cell(
            narrow,
            wide,
            enthalpy,
            &mut capacity,
            &mut temperature,
            &heat,
            &p,
            coarse,
        );
    }
    (capacity, temperature)
}

/// Halves away from zero, in `f64`, so the enthalpy of a target temperature is
/// formed **outside** the crate and not by calling the conversion the operator
/// calls.
fn round_half_away_from_zero(x: f64) -> i64 {
    if x < 0.0 {
        -((-x) + 0.5).floor() as i64
    } else {
        (x + 0.5).floor() as i64
    }
}

/// The enthalpy, in storage units, that puts cell `coarse` at `T_ref + excess`:
/// `H = round((T* - T_ref) * C_cell / joules_per_unit)`.
///
/// Formed here and not asked of anything: the round trip is only a round trip if
/// one direction is computed by the test. `C_cell` arrives as the units the
/// covering map gathered, so an operator that gathers a different cube is not
/// merely inexact here — it is answering a question the test did not ask.
fn enthalpy_for(coarse: u32, units: &[i64; N_SUBSTANCES as usize]) -> i64 {
    round_half_away_from_zero(excess(coarse) * capacity_of(units) / JOULES_PER_UNIT)
}

/// The enthalpy of the whole coarse field, cell by cell.
fn enthalpy_field(units: &[[i64; N_SUBSTANCES as usize]]) -> Vec<M64> {
    (0..N_COARSE)
        .map(|c| M64::new(enthalpy_for(c, &units[c as usize])))
        .collect()
}

#[test]
fn temperature_from_enthalpy_round_trips() {
    // `ACCEPTANCE.md`, "Physics". `T*` is chosen per coarse cell, `H` is computed
    // from it outside the crate, the operator is run, and the `T` it produces has
    // to be `T*` again.
    let (narrow, wide) = amounts();
    let units = gathered(&narrow, &wide);
    let enthalpy = enthalpy_field(&units);
    let (capacity, temperature) = dispatch(&narrow, &wide, &enthalpy);

    for coarse in 0..N_COARSE {
        let want_capacity = capacity_of(&units[coarse as usize]);
        let want = T_REF + excess(coarse);

        // The denominator first, because everything else is downstream of it and
        // because three mistakes of very different sizes land here. A `C_cell`
        // gathered on the **fine** grid is `2^(3*lod)` times too small, so `T`
        // comes out sixty-four times further from `T_ref` — a plausible-looking
        // number that no invariant contains. A `joules_per_unit` folded as `2^k_E`
        // instead of `2^-k_E` is absurd by forty binary orders and therefore the
        // easy one. And the third is the covering map itself: the right-hand side
        // is the amounts of *these* sixty-four voxels, summed one voxel at a time
        // through SPEC section 1.5, so a cell that gathered a neighbouring cube —
        // a slab of the wrong `z`, a stripe of sixty-four along X — fails here
        // rather than passing on a plausible number.
        assert_eq!(
            capacity[coarse as usize],
            Q::from_f64(want_capacity),
            "coarse cell {coarse} did not gather the composition of its \
             {PER_COARSE} fine voxels"
        );

        // One storage unit of enthalpy is worth this much temperature in this
        // cell: `2^-k_E / C_cell`. Named as a number rather than hidden inside an
        // epsilon, because it is the whole of the tolerance the criterion allows.
        let quantum = JOULES_PER_UNIT / want_capacity;
        let got = temperature[coarse as usize].debug_f64();
        assert!(
            (got - want).abs() <= quantum,
            "coarse cell {coarse} came back at {got} K against the {want} K it \
             was given, over a quantum of {quantum} K"
        );

        // And on this fixture the round trip is not merely inside the tolerance:
        // every weight in it is a power of two and every excess is dyadic, so `H`
        // is an exact integer and the division gives the excess back exactly.
        assert_eq!(got, want, "coarse cell {coarse} is not exact");
    }

    // The cell next door holds a different composition and comes out at a
    // different temperature. Without this the whole fixture is consistent with an
    // operator that ignores the amounts and returns one number.
    for coarse in 0..N_COARSE - 1 {
        assert_ne!(
            temperature[coarse as usize],
            temperature[coarse as usize + 1],
            "coarse cells {coarse} and {} agree, and their compositions do not",
            coarse + 1
        );
        assert_ne!(capacity[coarse as usize], capacity[coarse as usize + 1]);
    }

    // Sixty-four fine voxels share one `T`, and **which** sixty-four is the half
    // of ADR-062 that no assertion over whole fields can make: a covering map of
    // `fine_idx >> lod`, or of `coarse * 2^(3*lod) + f`, or one that shifts `z` by
    // `lod - 1`, partitions the voxels just as completely. So every fine voxel of
    // the domain in turn receives a little more solvent, and exactly one coarse
    // cell — the one this file's own per-axis shift names — is allowed to move in
    // either output.
    //
    // One dispatch per voxel, which is `nx*ny*nz` of them. The cost is the point:
    // it is the only statement that ties an *address in the amount buffer* to a
    // cell of the temperature field, and every cheaper form of it has been green
    // under a wrong map.
    let mut probed = wide.clone();
    for z in 0..NZ {
        for y in 0..NY {
            for x in 0..NX {
                let at = (LANES[WATER] * N_FINE + index(x, y, z)) as usize;
                probed[at] = M64::new(water(x, y, z) + PROBE);
                let (moved_capacity, moved_temperature) = dispatch(&narrow, &probed, &enthalpy);
                probed[at] = M64::new(water(x, y, z));

                let covering = covering(x, y, z);
                for coarse in 0..N_COARSE {
                    let at = coarse as usize;
                    if coarse == covering {
                        assert_ne!(
                            moved_capacity[at], capacity[at],
                            "the solvent added at ({x}, {y}, {z}) never reached \
                             coarse cell {coarse}, which is the cell covering it"
                        );
                        assert_ne!(moved_temperature[at], temperature[at]);
                    } else {
                        assert_eq!(
                            moved_capacity[at], capacity[at],
                            "the solvent added at ({x}, {y}, {z}) landed in coarse \
                             cell {coarse}; the cell covering it is {covering}"
                        );
                        assert_eq!(moved_temperature[at], temperature[at]);
                    }
                }
            }
        }
    }
}

#[test]
fn a_target_between_two_storage_quanta_round_trips_inside_one_quantum() {
    // The other half of the criterion, and the one that makes the tolerance more
    // than decoration: a target temperature that is *not* a whole number of
    // storage units. `H` is rounded once, outside the crate, and what comes back
    // has to be the target within that rounding.
    //
    // Two terms and both are named. The first is the quantum, `2^-k_E / C_cell`.
    // The second is one ulp of `Q` at the temperature itself: `Q` is an `f32` and
    // `T = T_ref + excess` adds a small number to a large one, so at 256 K nothing
    // in this system can resolve better than `2^-15` K — which is *larger* than
    // the quantum on this fixture, and would be larger still on the corpus scale,
    // where ADR-062 prices the resolution of the field at `2.5e-17` K and calls
    // that "an absurdity named as a price".
    const THIRD: f64 = 1.0 / 3.0;

    let (narrow, wide) = amounts();
    let units = gathered(&narrow, &wide);
    let enthalpy: Vec<M64> = (0..N_COARSE)
        .map(|c| {
            M64::new(round_half_away_from_zero(
                THIRD * capacity_of(&units[c as usize]) / JOULES_PER_UNIT,
            ))
        })
        .collect();
    let (_, temperature) = dispatch(&narrow, &wide, &enthalpy);

    for coarse in 0..N_COARSE {
        let quantum = JOULES_PER_UNIT / capacity_of(&units[coarse as usize]);
        // One ulp of an `f32` at 256: `2^(8-23)`.
        let ulp = 2f64.powi(-15);
        let want = T_REF + THIRD;
        let got = temperature[coarse as usize].debug_f64();
        assert!(
            (got - want).abs() <= quantum + ulp,
            "coarse cell {coarse} came back at {got} K against {want} K, over a \
             quantum of {quantum} K plus one ulp of {ulp} K"
        );
    }
}

#[test]
fn a_substance_with_zero_heat_capacity_moves_the_temperature_by_nothing() {
    // The proton declares `c_p = 0` by the same single-ion convention that gives
    // it a zero partial molar volume, and it is the most abundant thing in this
    // fixture after the solvent. Its whole pool has to be worth exactly nothing in
    // the denominator — an operator that treated a zero coefficient as "missing"
    // and substituted anything at all would move every temperature of the world.
    let (narrow, wide) = amounts();
    let enthalpy = enthalpy_field(&gathered(&narrow, &wide));
    let (with_protons, t_with) = dispatch(&narrow, &wide, &enthalpy);

    let mut without = narrow.clone();
    for idx in 0..N_FINE {
        without[(LANES[PROTON] * N_FINE + idx) as usize] = M32::ZERO;
    }
    let (without_protons, t_without) = dispatch(&without, &wide, &enthalpy);

    assert_eq!(with_protons, without_protons);
    assert_eq!(t_with, t_without);
}

#[test]
fn the_negative_heat_capacity_lowers_the_denominator() {
    // Aqueous sulfate at about `-293 J/(mol K)` is physics and not a typo, and an
    // `abs()` or a `max(0, .)` anywhere on its path gives a positive, plausible
    // denominator that no invariant contains. Stated as a direction rather than a
    // value, because the two repairs differ in magnitude and agree in sign.
    let (narrow, wide) = amounts();
    let enthalpy = vec![M64::ZERO; N_COARSE as usize];
    let (capacity, _) = dispatch(&narrow, &wide, &enthalpy);

    let mut more_sulfate = narrow.clone();
    for idx in 0..N_FINE {
        let at = (LANES[SULFATE] * N_FINE + idx) as usize;
        more_sulfate[at] = M32::new(more_sulfate[at].to_i64() as i32 * 2);
    }
    let (lowered, _) = dispatch(&more_sulfate, &wide, &enthalpy);

    for coarse in 0..N_COARSE {
        assert!(
            lowered[coarse as usize] < capacity[coarse as usize],
            "coarse cell {coarse}: doubling the substance with a negative c_p did \
             not lower the denominator"
        );
        assert!(
            lowered[coarse as usize] > Q::ZERO,
            "the fixture has to stay above zero, or it is testing the refusal \
             instead"
        );
    }
}
