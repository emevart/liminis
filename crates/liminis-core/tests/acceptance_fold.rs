//! Acceptance criteria of the fold kernel (`ACCEPTANCE.md`, "Conservation"):
//!
//! ```text
//! energy_fold_from_fine_to_coarse_conserves_exactly
//! absorbed_light_appears_in_enthalpy
//! ```
//!
//! Outside the crate rather than beside the kernel, for the reason
//! `acceptance_diffusion.rs` states: an acceptance test is the outside view. It
//! may use only what a scenario can use — the public surface of `liminis-core` —
//! so that it keeps its meaning when the inside is rearranged, and so that it
//! cannot be quietly shaped around a private helper it was supposed to judge.
//!
//! Here that is not a placement nicety, it is the whole point. The fold is two
//! index mappings and a sum: SPEC section 1.1 for the fine voxel, section 1.5 for
//! the coarse cell that covers it. `kernels/fold.rs` carries private copies of
//! both, and every fixture of its unit tests places its inputs through the same
//! private copy — so a transposed one is consistent with itself, the totals still
//! close, and only a reader would notice. This file writes both mappings out from
//! the spec and never asks the kernel where it thinks a voxel is.
//!
//! The unit tests under `src/kernels/fold.rs` overlap these two names. That is not
//! duplication to be cleaned up: those check the kernel from the inside — that the
//! previous enthalpy comes from `src_h`, that the reaction sum stays in `i64`, that
//! the floor beam is credited to nobody — on one grid built with the kernel's own
//! helpers. These check the property the criterion names, on a different grid, with
//! the index written from the document. Either can fail while the other passes.
//!
//! The grid is deliberately not the one the unit tests use: `12 x 8 x 16` against
//! their `8 x 12 x 16`, so `nx` and `ny` are swapped as well as different. Coarse
//! `3 x 2 x 4` — three pairwise different extents, none of them one, because a
//! coarse extent of one erases its axis from the shift silently.

use liminis_core::kernels::fold::{FoldParams, fold_energy};
use liminis_core::kernels::light::{Attenuators, LightParams, light_column};
use liminis_core::numeric::{M32, M64, Q};

const NX: u32 = 12;
const NY: u32 = 8;
const NZ: u32 = 16;
const LOD: u32 = 2;

const N_FINE: u32 = NX * NY * NZ;
const N_COARSE: u32 = (NX >> LOD) * (NY >> LOD) * (NZ >> LOD);

/// `2^(3*lod)`: how many fine voxels one coarse cell owns.
const PER_COARSE: i64 = 1 << (3 * LOD);

/// Irradiance on the top face of the topmost voxel, W/m^2. Arbitrary on purpose:
/// no key in `CONFIG_SCHEMA.md` section 7 and no row in `QUANTITIES.md` section 5
/// names the incident irradiance, so any number here would be invented. The kernel
/// takes it folded, so a test may use whatever it likes and the criterion is
/// unaffected. A power of two, so the products below are exact and the assertions
/// are about the kernel rather than about `f32`.
const INCIDENT: f64 = 128.0;

/// The tick, seconds. Deliberately not one: a tick of one second would hide a
/// kernel that forgot to multiply by it.
const DT: f64 = 2.0;

/// What one unit of stored intensity is worth as energy per second, folded on the
/// host (`FoldParams::joules_per_intensity`). A power of two, so `qmul` by it is
/// exact and the only inexactness left on the light path is the single rounding
/// into storage units.
const JOULES_PER_INTENSITY: f64 = 1024.0;

/// The whole conversion factor from stored intensity to storage units.
const PER_INTENSITY: f64 = JOULES_PER_INTENSITY * DT;

fn params() -> FoldParams {
    FoldParams {
        nx: NX,
        ny: NY,
        nz: NZ,
        lod: LOD,
        i_surface: Q::from_f64(INCIDENT),
        joules_per_intensity: Q::from_f64(JOULES_PER_INTENSITY),
        dt: Q::from_f64(DT),
    }
}

/// The linear index of a fine voxel, from SPEC section 1.1: `x + y*NX + z*NX*NY`.
///
/// Written out here rather than borrowed from anywhere. `world::Grid::index` says
/// the same thing and the kernel's private copy is checked against it, but an
/// acceptance test that took its index from either of them would be agreeing with
/// the code under test by construction.
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

/// The whole dispatch, the way a host would run it: one invocation per coarse
/// cell, over the coarse grid and not the fine one.
fn dispatch(energy_delta: &[M64], light: &[Q], src_h: &[M64], p: &FoldParams) -> Vec<M64> {
    let mut dst_h = vec![M64::ZERO; N_COARSE as usize];
    for coarse in 0..N_COARSE {
        fold_energy(energy_delta, light, src_h, &mut dst_h, p, coarse);
    }
    dst_h
}

/// A sign-alternating reaction increment with no period any stride of the grid
/// shares: a pattern agreeing with `nx` or with a plane would cancel inside a
/// coarse cell and hide a voxel gathered twice.
fn reaction_pattern(x: u32, y: u32, z: u32) -> i64 {
    let n = i64::from(index(x, y, z));
    let magnitude = (n * 7919) % 1013 + 1;
    if n % 3 == 0 { -magnitude } else { magnitude }
}

/// A light field flat at the incident irradiance: every voxel absorbs an exact
/// zero, both at the top face and everywhere below it.
fn transparent() -> Vec<Q> {
    vec![Q::from_f64(INCIDENT); N_FINE as usize]
}

/// A light field in which every voxel absorbs exactly one unit of intensity:
/// `light[z] = i_surface - (nz - z)`.
///
/// Hand-built, and that is what lets the assertion below be an equality with no
/// tolerance: every one of the sixty-four terms of a coarse cell is `1.0`, so the
/// sum is `64.0` in whatever order it is taken, and the cell's share of the light
/// is one exactly known integer.
fn stepped() -> Vec<Q> {
    let mut light = vec![Q::ZERO; N_FINE as usize];
    for z in 0..NZ {
        for y in 0..NY {
            for x in 0..NX {
                light[index(x, y, z) as usize] = Q::from_f64(INCIDENT - f64::from(NZ - z));
            }
        }
    }
    light
}

fn total(cells: &[M64]) -> i64 {
    cells.iter().map(|h| h.to_i64()).sum()
}

#[test]
fn energy_fold_from_fine_to_coarse_conserves_exactly() {
    // Exactly, and without a tolerance: both sides are sums of `i64`.
    //
    // Two claims, and the second is the one that needs an outside index. A fold
    // that mapped fine voxels onto coarse cells by `fine_idx >> lod`, or by
    // `coarse * 64 + f`, or that transposed the fine index, partitions the index
    // space just as completely — the domain total closes under every one of them
    // and the heat lands in the wrong cell. So the per-cell sum is asserted too,
    // against a covering map written from SPEC section 1.5.
    let p = params();

    let mut energy_delta = vec![M64::ZERO; N_FINE as usize];
    for z in 0..NZ {
        for y in 0..NY {
            for x in 0..NX {
                energy_delta[index(x, y, z) as usize] = M64::new(reaction_pattern(x, y, z));
            }
        }
    }

    // What each coarse cell is owed, and what the domain is owed, both accumulated
    // here rather than read back from the kernel.
    let mut owed = vec![0i64; N_COARSE as usize];
    for z in 0..NZ {
        for y in 0..NY {
            for x in 0..NX {
                owed[covering(x, y, z) as usize] += reaction_pattern(x, y, z);
            }
        }
    }
    let owed_total: i64 = owed.iter().sum();

    // An enthalpy that is already something: a kernel that overwrote instead of
    // adding, or that read the previous value out of the buffer it writes, parts
    // from this here and not on a zeroed field.
    let before: Vec<M64> = (0..N_COARSE)
        .map(|c| M64::new(i64::from(c) * 1_000_003 - 7))
        .collect();

    // Light off. Every term of the right side is a reaction increment.
    let dark = dispatch(&energy_delta, &transparent(), &before, &p);
    assert_eq!(total(&dark) - total(&before), owed_total);
    for coarse in 0..N_COARSE {
        let at = coarse as usize;
        assert_eq!(
            dark[at].to_i64() - before[at].to_i64(),
            owed[at],
            "coarse cell {coarse} did not gather exactly its own {PER_COARSE} voxels"
        );
    }

    // Light on, one unit of intensity absorbed per fine voxel. The light is worth
    // an exact integer per coarse cell, so this stays an equality.
    let per_cell_light = PER_COARSE * (JOULES_PER_INTENSITY as i64) * (DT as i64);
    let lit = dispatch(&energy_delta, &stepped(), &before, &p);
    assert_eq!(
        total(&lit) - total(&before),
        owed_total + i64::from(N_COARSE) * per_cell_light
    );
    for coarse in 0..N_COARSE {
        let at = coarse as usize;
        assert_eq!(
            lit[at].to_i64() - before[at].to_i64(),
            owed[at] + per_cell_light,
            "coarse cell {coarse} is short of its reactions or of its light"
        );
    }
}

/// One attenuator reading the narrow slice: the smallest configuration of the
/// light kernel that still produces a real Beer-Lambert profile.
const OPACITY: f64 = 1.0 / 1024.0;

/// `tau = 0.5` for a voxel holding this much. Under `ln 2`, so every intensity in
/// a column stays within a factor of two of its neighbour and the differences the
/// fold takes are exact in `f32` by Sterbenz.
const OPAQUE: i32 = 512;

/// The light field, produced by the light kernel itself.
///
/// The two kernels meet here the way they will meet in a tick and nowhere else in
/// this file: `light.rs` decides that a cell holds what **leaves** the voxel
/// through its bottom face, and the fold takes the difference that turns it back
/// into absorption. Building the field by hand would test the fold against this
/// file's reading of that convention instead of against the kernel that owns it.
fn light_field(amounts: &[M32]) -> Vec<Q> {
    let lp = LightParams {
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
    // The energy the beam lost inside the domain has to turn up in the enthalpy of
    // the cells it lost it in — ADR-049, which exists because light was otherwise a
    // channel with an input and no destination.
    //
    // Three claims. The cells covering an absorber warm; every other cell stays at
    // an exact zero, because a transparent voxel attenuates by `qexp(0) == 1` and
    // "nearly zero" is not the claim; and the column's total is the beam it lost.
    //
    // The absorbers sit in one fine column rather than in a whole layer, so no
    // coarse cell has more than one nonzero term and the `Q` sums are exact
    // whatever order they are taken in.
    const X: u32 = 9;
    const Y: u32 = 3;
    const Z_MIDDLE: u32 = 6;

    let p = params();
    let mut amounts = vec![M32::ZERO; N_FINE as usize];
    amounts[index(X, Y, Z_MIDDLE) as usize] = M32::new(OPAQUE);
    amounts[index(X, Y, NZ - 1) as usize] = M32::new(OPAQUE);

    let light = light_field(&amounts);
    let energy_delta = vec![M64::ZERO; N_FINE as usize];
    let src_h = vec![M64::ZERO; N_COARSE as usize];
    let dst = dispatch(&energy_delta, &light, &src_h, &p);

    let warm_middle = covering(X, Y, Z_MIDDLE);
    let warm_top = covering(X, Y, NZ - 1);
    assert_ne!(warm_middle, warm_top);

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

    // The topmost fine layer contributed, and it can only have done so against
    // `i_surface`: it has no neighbour above it in the field. A fold that mirrored
    // the boundary the way `diffuse.rs` closes a face would put a zero here and
    // leave a whole layer of absorbed energy out of the world with the profile
    // `I(z)` staying perfectly right.
    assert!(dst[warm_top as usize].to_i64() > 0);

    // The column as a whole absorbed what the beam lost crossing it, up to the
    // roundings into storage units — one per coarse cell of the column, four of
    // them, each worth at most half a unit. Named, rather than hidden inside a
    // tolerance chosen until the test passed.
    const ROUNDINGS_PER_COLUMN: i64 = (NZ >> LOD) as i64;
    let tolerance = 0.5 * ROUNDINGS_PER_COLUMN as f64;

    let mut column_sum = 0i64;
    for cz in 0..NZ >> LOD {
        column_sum += dst[covering(X, Y, cz << LOD) as usize].to_i64();
    }

    // `light[index(x, y, 0)]` is the beam that left under the domain: the lower
    // term of the bottom voxel and nothing else. Crediting it would create energy
    // with no channel — ADR-059, "SOLAR_IN counts what was absorbed, not what
    // fell" — so it appears here only inside the telescoped difference.
    let leaving = light[index(X, Y, 0) as usize].debug_f64();
    let telescoped = (INCIDENT - leaving) * PER_INTENSITY;
    assert!(telescoped > 0.0, "the column absorbed nothing to speak of");
    assert!(
        (column_sum as f64 - telescoped).abs() <= tolerance,
        "the column absorbed {column_sum} against a telescoped {telescoped}, over \
         a tolerance of {tolerance} for {ROUNDINGS_PER_COLUMN} roundings"
    );
}
