//! Acceptance criteria of the world aggregate: the process boundary, the LOD
//! mapping and host-side lane resolution.
//!
//! ```text
//! every_lane_is_state_n_at_a_process_boundary
//! a_coarse_cell_covers_exactly_its_fine_cells
//! mixed_width_storage_matches_uniform_width_storage
//! ```
//!
//! Outside the crate rather than beside the types, for the reason
//! `tests/acceptance_diffusion.rs` states: an acceptance test is the outside
//! view, it may use only what a scenario can use — the public surface of
//! `liminis-core` — and so it keeps its meaning when the inside is rearranged.
//! Here that matters twice over, because both of the first two names are about
//! an *address*, and a test that asked the code where it thought a lane or a
//! coarse cell was would agree with it by construction.
//!
//! # Two of these names are not in `ACCEPTANCE.md`, and that is not an oversight
//!
//! The journal fixes different names for the same two statements, and acceptance
//! names are assigned by a record rather than by whoever writes the code:
//!
//! | here | fixed by |
//! |---|---|
//! | `every_lane_is_state_n_at_a_process_boundary` | ADR-057 calls it `a_process_returns_with_state_n_in_the_front_buffer_for_every_lane` |
//! | `a_coarse_cell_covers_exactly_its_fine_cells` | nothing; the nearest is `the_covering_cell_is_the_per_axis_shift` in `kernels/fold.rs`, which is not in the acceptance list either |
//!
//! Both journal names are written and green — the first inside
//! `process/diffuse.rs`, the second inside `kernels/fold.rs` — and the two here
//! are the outside view of the same claims on a different fixture. Adding them
//! to `ACCEPTANCE.md` would be a decision about that document, so it is not made
//! here.
//!
//! `mixed_width_storage_matches_uniform_width_storage` deliberately duplicates a
//! name in `tests/acceptance_reactions.rs`, on the precedent of
//! `energy_fold_from_fine_to_coarse_conserves_exactly` (inside view in
//! `kernels/fold.rs`, outside view in `tests/acceptance_fold.rs`). That one runs
//! the reaction kernel over hand-written lane tables and catches the addressing
//! mistake *inside* a kernel. This one builds two `World`s and catches it on the
//! **host**: `world.amounts_32_mut()` reached with an `s` where a lane belongs.
//! Neither sees the other's failure, and both stay green when a lane is wrong in
//! the way that matters — the ledger closes, because nothing is lost, and the
//! wrong thing is read.

use liminis_core::kernels::fold::{FoldParams, fold_energy};
use liminis_core::kernels::react::{NO_CATALYST, ReactParams, Rx, react_voxel};
use liminis_core::ledger::Ledger;
use liminis_core::numeric::{M32, M64, Q, run_key};
use liminis_core::process::DiffusePhase;
use liminis_core::world::{
    Boundary, Face, Field32, Field64, Grid, LaneRef, Registry, SubstanceDecl, Width, World,
    WorldLayout,
};

/// A grid with no exchanging face: `k_ex` multiplies a flux nobody gathers.
///
/// The venting cases are the outside view of ADR-059 and live in
/// `tests/acceptance_boundary.rs`.
const SEALED: f64 = 0.0;

/// One substance per lane, and a scratch ledger. On a sealed grid no counter is
/// ever touched, so which substance a lane stands for cannot matter here.
fn run_diffusion_32(phase: &DiffusePhase, field: &mut Field32) {
    let table: Vec<u32> = (0..field.lanes()).collect();
    phase.apply_32(field, &table, &mut Ledger::new(32).unwrap());
}

fn run_diffusion_64(phase: &DiffusePhase, field: &mut Field64) {
    let table: Vec<u32> = (0..field.lanes()).collect();
    phase.apply_64(field, &table, &mut Ledger::new(32).unwrap());
}

/// The eco regime of SPEC section 1.7: a one-second tick and a 100 um voxel.
const DT: f64 = 1.0;
const DX: f64 = 1.0e-4;

/// A deliberately non-cubic grid whose three **coarse** extents at `lod = 2` are
/// `2 x 3 x 4` — pairwise different, and none of them one.
///
/// Neither property is decoration. On a cubic grid every bug that permutes the
/// axes is invisible, because all three strides are equal; and with a coarse
/// extent of one, that axis drops out of every expectation, so `z0 = cz` in
/// place of `z0 = cz << lod` stays green.
const NX: u32 = 8;
const NY: u32 = 12;
const NZ: u32 = 16;
const ENTHALPY_LOD: u32 = 2;
const VELOCITY_LOD: u32 = 1;

fn layout() -> WorldLayout {
    WorldLayout {
        enthalpy_lod: ENTHALPY_LOD,
        velocity_lod: VELOCITY_LOD,
    }
}

/// Periodic in X and Y, closed floor and lid in Z — the eco-regime default of
/// SPEC section 1.6, minus the `exchange` face that cannot be built yet.
fn floored(nx: u32, ny: u32, nz: u32) -> Grid {
    Grid::new(
        nx,
        ny,
        nz,
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

fn decl(id: &str, width: Width) -> SubstanceDecl {
    SubstanceDecl {
        id: id.to_string(),
        width,
        k: K,
    }
}

/// A value that depends on both lane and voxel, so that a lane mix-up cannot
/// pass by accident.
fn pattern(lane: u32, idx: u32) -> i32 {
    (1 + lane as i32) * 1_000_003 + (idx as i32 * 7919) % 100_000
}

// ---------------------------------------------------------------------------
// every_lane_is_state_n_at_a_process_boundary
// ---------------------------------------------------------------------------

/// Substep counts of ADR-030 for the corpus registry, as diffusivities:
/// `[6, 2, 1, 1, 1]` and one lane at `D = 0`, which takes none at all.
///
/// The minority here is **even** — lanes 0 and 1 against three odd ones and the
/// untouched one — no: with the zero lane the even side is three and the odd
/// side three, which is the tie. Both branches of the restoration have to be
/// exercised or one of them never runs, so the test below uses two
/// configurations and asserts which branch each one took.
const SUBSTEPS_MINORITY_EVEN: [f64; 5] = [9.3e-9, 2.1e-9, 1.6e-9, 0.8e-9, 1.0e-9];
const SUBSTEPS_MINORITY_ODD: [f64; 6] = [9.3e-9, 2.1e-9, 1.9e-9, 2.1e-9, 1.6e-9, 0.0];

/// One lane of a multi-lane field, run alone in a field of its own, through the
/// same public process. The reference every assertion is made against.
fn diffused_alone_32(grid: &Grid, diffusivity: f64, lane: u32, ticks: u32) -> Field32 {
    let mut field = Field32::new(grid, 1).unwrap();
    {
        let n_voxels = field.n_voxels();
        let buffer = field.write_mut();
        for idx in 0..n_voxels {
            buffer[idx as usize] = M32::new(pattern(lane, idx));
        }
    }
    field.swap();

    let phase = DiffusePhase::new_32(grid, 1, &[diffusivity], DT, DX, SEALED).unwrap();
    for _ in 0..ticks {
        run_diffusion_32(&phase, &mut field);
    }
    field
}

/// `ADR-057`, and `ACCEPTANCE.md` under the name
/// `a_process_returns_with_state_n_in_the_front_buffer_for_every_lane`.
///
/// The whole claim in one sentence: a reader of state `N` — the reaction kernel,
/// pressure, settling, `ledger/`, `observe/` — does not have to know that lanes
/// advance at different speeds. So **no assertion here looks into the write
/// buffer**. Every lane is read back through `Field::read()` and compared with
/// the same lane run alone at its own substep count.
///
/// It fails if the restoration copied the wrong group, copied the wrong way
/// (`front -> back`), chose the parity by substance index instead of by lane, or
/// counted a lane nobody dispatched as odd.
#[test]
fn every_lane_is_state_n_at_a_process_boundary() {
    let grid = floored(5, 6, 7);
    const TICKS: u32 = 3;

    // Both branches of the restoration, because each one looks perfectly
    // workable on its own and only one of them runs per configuration.
    let mut swapped = 0;
    let mut unswapped = 0;

    for diffusivities in [&SUBSTEPS_MINORITY_EVEN[..], &SUBSTEPS_MINORITY_ODD[..]] {
        let lanes = diffusivities.len() as u32;
        let phase = DiffusePhase::new_32(&grid, lanes, diffusivities, DT, DX, SEALED).unwrap();
        if phase.parity_split().swaps() {
            swapped += 1;
        } else {
            unswapped += 1;
        }

        let mut field = Field32::new(&grid, lanes).unwrap();
        {
            let n_voxels = field.n_voxels();
            let lane_len = field.lane_len();
            let buffer = field.write_mut();
            for lane in 0..lanes {
                for idx in 0..n_voxels {
                    buffer[(lane * lane_len + idx) as usize] = M32::new(pattern(lane, idx));
                }
            }
        }
        field.swap();

        for _ in 0..TICKS {
            run_diffusion_32(&phase, &mut field);
        }

        for lane in 0..lanes {
            let reference = diffused_alone_32(&grid, diffusivities[lane as usize], lane, TICKS);
            assert_eq!(
                field.lane(lane),
                reference.lane(0),
                "lane {lane} of {lanes} is not state N in the front buffer"
            );
        }
    }

    assert_eq!(
        (swapped, unswapped),
        (1, 1),
        "both restoration branches have to run, or one of them is never exercised"
    );

    // The wide field of the same world, one lane, at the width where the copy is
    // twice as expensive (ADR-057: the one 64-bit lane stands on the odd side of
    // the corpus registry).
    let wide_phase = DiffusePhase::new_64(&grid, 1, &[9.3e-9], DT, DX, SEALED).unwrap();
    let mut wide = Field64::new(&grid, 1).unwrap();
    {
        let n_voxels = wide.n_voxels();
        let buffer = wide.write_mut();
        for idx in 0..n_voxels {
            buffer[idx as usize] = M64::new(5_100_000_000_000 + i64::from(idx) * 977);
        }
    }
    wide.swap();
    let before: i64 = wide.lane(0).iter().map(|v| v.to_i64()).sum();

    for _ in 0..TICKS {
        run_diffusion_64(&wide_phase, &mut wide);
        assert_eq!(
            wide.lane(0).iter().map(|v| v.to_i64()).sum::<i64>(),
            before,
            "the wide lane moved its total"
        );
    }
    // Six substeps is an even number of them, so this lane is in the front
    // buffer without a copy — and it had better hold something that moved.
    assert!(wide.lane(0).windows(2).any(|w| w[0] != w[1]));
}

// ---------------------------------------------------------------------------
// a_coarse_cell_covers_exactly_its_fine_cells
// ---------------------------------------------------------------------------

/// SPEC section 1.5, transcribed here from the document rather than borrowed
/// from the code: the shift goes **per axis**.
fn covering_cell_from_the_spec(x: u32, y: u32, z: u32) -> u32 {
    let cnx = NX >> ENTHALPY_LOD;
    let cny = NY >> ENTHALPY_LOD;
    (x >> ENTHALPY_LOD) + (y >> ENTHALPY_LOD) * cnx + (z >> ENTHALPY_LOD) * cnx * cny
}

/// The two wrong forms, so that the test can prove it is able to tell them
/// apart. Both cut the index space into disjoint pieces exactly the way the
/// right one does, so every conservation test in the project stays green under
/// either.
fn covering_cell_by_linear_shift(fine_idx: u32) -> u32 {
    fine_idx >> ENTHALPY_LOD
}

fn covering_cell_by_block(fine_idx: u32) -> u32 {
    fine_idx / (1 << (3 * ENTHALPY_LOD))
}

/// ADR-045, ADR-062 and SPEC section 1.5. The nearest journal name is
/// `the_covering_cell_is_the_per_axis_shift`, which lives beside the fold kernel.
///
/// Three claims. The mapping agrees with the spec written out by axis; the
/// preimage of every coarse cell is exactly the cube of `2^(3*lod)` fine voxels;
/// and the mapping agrees with the one the fold kernel carries privately — which
/// until now had no authority in `world/` to be checked against at all, because
/// `Grid` knew nothing about `lod`.
#[test]
fn a_coarse_cell_covers_exactly_its_fine_cells() {
    let world = World::new(floored(NX, NY, NZ), narrow_registry(), &layout()).unwrap();
    let cnx = NX >> ENTHALPY_LOD;
    let cny = NY >> ENTHALPY_LOD;
    let cnz = NZ >> ENTHALPY_LOD;
    let span = 1u32 << ENTHALPY_LOD;
    let n_coarse = cnx * cny * cnz;
    assert_eq!(world.enthalpy_grid().n_voxels(), n_coarse);
    assert!(cnx != cny && cny != cnz && cnx != cnz && cnz > 1);

    // (1) The mapping, per axis, and the witnesses that this fixture can see the
    //     two wrong forms. Without the witnesses the test only says "a partition
    //     is a partition", which both mistakes satisfy.
    let mut linear_shift_differs = false;
    let mut block_differs = false;
    let mut members: Vec<Vec<(u32, u32, u32)>> = vec![Vec::new(); n_coarse as usize];

    for z in 0..NZ {
        for y in 0..NY {
            for x in 0..NX {
                let fine_idx = world.grid().index(x, y, z);
                let expected = covering_cell_from_the_spec(x, y, z);
                assert_eq!(
                    world.enthalpy_cell_of(fine_idx),
                    expected,
                    "voxel ({x}, {y}, {z})"
                );
                linear_shift_differs |= covering_cell_by_linear_shift(fine_idx) != expected;
                block_differs |= covering_cell_by_block(fine_idx) != expected;
                members[expected as usize].push((x, y, z));
            }
        }
    }
    assert!(
        linear_shift_differs,
        "`fine_idx >> lod` would pass this test"
    );
    assert!(
        block_differs,
        "`coarse * 2^(3*lod) + f` would pass this test"
    );

    // (2) The preimage of a coarse cell is a cube, not a stripe of sixty-four
    //     along X.
    for coarse in 0..n_coarse {
        let cell = &members[coarse as usize];
        assert_eq!(
            cell.len(),
            (span * span * span) as usize,
            "coarse cell {coarse} covers the wrong number of fine voxels"
        );
        let x0 = cell.iter().map(|v| v.0).min().unwrap();
        let y0 = cell.iter().map(|v| v.1).min().unwrap();
        let z0 = cell.iter().map(|v| v.2).min().unwrap();
        assert_eq!((x0 % span, y0 % span, z0 % span), (0, 0, 0));
        for &(x, y, z) in cell {
            assert!(
                (x0..x0 + span).contains(&x)
                    && (y0..y0 + span).contains(&y)
                    && (z0..z0 + span).contains(&z),
                "coarse cell {coarse} is not the cube at ({x0}, {y0}, {z0}): ({x}, {y}, {z})"
            );
        }
    }

    // (3) The fold kernel and `World` have to agree, one fine voxel at a time.
    //     The kernel carries its own copy of both index mappings, because
    //     `kernels/` may depend on `numeric/` and on nothing else; this is the
    //     first time there is anything in `world/` for the coarse half of it to
    //     be held to.
    let fold = FoldParams {
        nx: NX,
        ny: NY,
        nz: NZ,
        lod: ENTHALPY_LOD,
        // A flat light field at the incident irradiance absorbs nothing, so the
        // only thing this dispatch moves is the reaction increment.
        i_surface: Q::from_f64(128.0),
        units_per_intensity: Q::from_f64(1024.0),
        dt: Q::from_f64(1.0),
    };
    let light = vec![Q::from_f64(128.0); (NX * NY * NZ) as usize];
    let src_h = vec![M64::ZERO; n_coarse as usize];

    for fine_idx in 0..world.grid().n_voxels() {
        let mut energy_delta = vec![M64::ZERO; (NX * NY * NZ) as usize];
        energy_delta[fine_idx as usize] = M64::new(1);

        let mut dst_h = vec![M64::ZERO; n_coarse as usize];
        let mut solar = vec![M64::ZERO; n_coarse as usize];
        for coarse in 0..n_coarse {
            fold_energy(
                &energy_delta,
                &light,
                &src_h,
                &mut dst_h,
                &mut solar,
                &fold,
                coarse,
            );
        }

        let expected = world.enthalpy_cell_of(fine_idx);
        for coarse in 0..n_coarse {
            assert_eq!(
                dst_h[coarse as usize].to_i64(),
                i64::from(coarse == expected),
                "the unit at fine voxel {fine_idx} landed in {coarse}, and \
                 World says its covering cell is {expected}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// mixed_width_storage_matches_uniform_width_storage
// ---------------------------------------------------------------------------

/// Four substances. `B` is the wide one and it is **not** at index zero, so
/// lanes and substance indices come apart from index two onwards — the shape of
/// the registry the project actually carries, where water stands first and takes
/// lane 0, and where the wrong expression is right at `s == 0` and off by one
/// after it.
///
/// The wide substance sits in the *middle* rather than at the end on purpose. A
/// registry whose only wide substance is last makes every wrong lane index run
/// off the end of the narrow field, and an out-of-bounds panic is not the
/// failure this test is about: with it here, reading substance `C` by its index
/// lands on `D`'s lane — in range, plausible, and silent.
const A: u32 = 0;
const B: u32 = 1;
const C: u32 = 2;
const D: u32 = 3;
const N_SUBSTANCES: u32 = 4;

/// The reserved index of enthalpy in the stoichiometry vector (ADR-041): past
/// the last substance, which is what `MAX_SUBSTANCES` being one below `S_MAX`
/// guarantees.
const S_ENERGY: u32 = N_SUBSTANCES;

/// `log2(units_per_mol)`, the same for every substance here so that the storage
/// coefficients equal the molar ones and the assertions are about addressing
/// rather than about scale (ADR-039).
const K: u8 = 4;
const E_R: u32 = 4;

/// Diffusivities **by substance index**. `B` does not diffuse, and that is not
/// laziness: it is the one substance whose storage width differs between the two
/// registries, so diffusing it would compare `diffuse_voxel_32` against
/// `diffuse_voxel_64` rather than comparing two addressings.
const DIFFUSIVITY: [f64; N_SUBSTANCES as usize] = [9.3e-9, 0.0, 2.1e-9, 1.6e-9];

/// Starting pools, distinct per substance: with equal ones a swapped lane reads
/// the right number by accident.
const START: [i64; N_SUBSTANCES as usize] = [100_000, 32_000, 7_000, 11_000];

fn narrow_registry() -> Registry {
    Registry::new(&[
        decl("A", Width::Bits32),
        decl("B", Width::Bits32),
        decl("C", Width::Bits32),
        decl("D", Width::Bits32),
    ])
    .unwrap()
}

fn mixed_registry() -> Registry {
    Registry::new(&[
        decl("A", Width::Bits32),
        decl("B", Width::Bits64),
        decl("C", Width::Bits32),
        decl("D", Width::Bits32),
    ])
    .unwrap()
}

/// Put an amount into the **write** buffer of whichever field owns substance
/// `s`, by substance index. This is the line the whole test is about, and
/// [`LaneRef`] is what stops it from being written with `s` where a lane goes.
fn seed(world: &mut World, s: u32, idx: u32, value: i64) {
    let lane_len = world.grid().lane_len();
    match world.lane_of(s) {
        LaneRef::Narrow(lane) => {
            let field = world.amounts_32_mut().expect("a narrow class was declared");
            field.write_mut()[(lane * lane_len + idx) as usize] =
                M32::new(i32::try_from(value).unwrap());
        }
        LaneRef::Wide(lane) => {
            let field = world.amounts_64_mut().expect("a wide class was declared");
            field.write_mut()[(lane * lane_len + idx) as usize] = M64::new(value);
        }
    }
}

/// Read an amount back out of state `N`, by substance index.
fn amount_of(world: &World, s: u32, idx: u32) -> i64 {
    let lane_len = world.grid().lane_len();
    let (narrow, wide) = world.amount_slices();
    match world.lane_of(s) {
        LaneRef::Narrow(lane) => narrow[(lane * lane_len + idx) as usize].to_i64(),
        LaneRef::Wide(lane) => wide[(lane * lane_len + idx) as usize].to_i64(),
    }
}

/// The flat reaction tables of ADR-041, owned so that `Rx` can borrow them.
///
/// One reaction, `A + 2 B -> C`, exothermic. Every `k_i` equals `e_r`, so the
/// storage coefficients are the molar ones and nothing here rounds.
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

impl Tables {
    /// Built from **this world's own registry**: the lane table the kernel gets
    /// is `Registry::lane_of()` and never a hand-written one.
    fn for_world(world: &World) -> Self {
        let mut lane = world.registry().lane_of().to_vec();
        // One slot past the last substance, for the enthalpy index. It is never
        // read — `extent_cap`, `accumulate_demand` and `rate_of` all skip
        // `s_energy` — and it is here so that an off-by-one would land on a
        // sentinel rather than out of bounds.
        lane.push(u32::MAX);

        Tables {
            nu: vec![-1, -2, 1, 3],
            nu_sub: vec![A, B, C, S_ENERGY],
            begin: vec![0],
            len: vec![4],
            e_r: vec![E_R],
            lane,
            cat: vec![NO_CATALYST],
            rid: vec![0x5eed_0001],
            // `xi = vmax * dt * V * 2^e_r = 0.25 * 16 = 4` quanta — a whole
            // number, so the stochastic rounding of ADR-027 cannot separate the
            // two runs by itself.
            vmax: vec![Q::from_f64(0.25)],
            q10: vec![Q::from_f64(2.0)],
            t_vmax: vec![Q::from_f64(300.0)],
            km: vec![Q::ZERO; 4],
            conc_per_unit: vec![Q::from_f64(1.0 / f64::from(1u32 << K)); N_SUBSTANCES as usize],
        }
    }

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

/// Build a world, seed the same amounts by substance index, and promote them
/// into state `N`.
fn seeded(registry: Registry) -> World {
    let mut world = World::new(floored(NX, NY, NZ), registry, &layout()).unwrap();
    let n_voxels = world.grid().n_voxels();
    for s in 0..N_SUBSTANCES {
        for idx in 0..n_voxels {
            // A gradient on top of the pool, or diffusion has nothing to move.
            seed(&mut world, s, idx, START[s as usize] + i64::from(idx % 97));
        }
    }
    if let Some(field) = world.amounts_32_mut() {
        field.swap();
    }
    if let Some(field) = world.amounts_64_mut() {
        field.swap();
    }
    world
}

/// One tick as a host runs it: a diffusion phase over each width, then one
/// dispatch of the reaction kernel.
fn run_a_tick(world: &mut World) {
    let grid = *world.grid();
    let n_voxels = grid.n_voxels();

    // Diffusivities are declared per substance and the phase takes them per
    // lane. Resolving with `s` in place of the lane is the mistake this whole
    // file exists for, and on this registry it is right for A and B and wrong
    // from C onwards.
    let mut narrow_by_lane = vec![0.0f64; world.registry().lanes(Width::Bits32) as usize];
    let mut wide_by_lane = vec![0.0f64; world.registry().lanes(Width::Bits64) as usize];
    for s in 0..N_SUBSTANCES {
        match world.lane_of(s) {
            LaneRef::Narrow(lane) => narrow_by_lane[lane as usize] = DIFFUSIVITY[s as usize],
            LaneRef::Wide(lane) => wide_by_lane[lane as usize] = DIFFUSIVITY[s as usize],
        }
    }

    if let Some(field) = world.amounts_32_mut() {
        let lanes = field.lanes();
        run_diffusion_32(
            &DiffusePhase::new_32(&grid, lanes, &narrow_by_lane, DT, DX, SEALED).unwrap(),
            field,
        );
    }
    if let Some(field) = world.amounts_64_mut() {
        let lanes = field.lanes();
        run_diffusion_64(
            &DiffusePhase::new_64(&grid, lanes, &wide_by_lane, DT, DX, SEALED).unwrap(),
            field,
        );
    }

    let tables = Tables::for_world(world);
    let params = ReactParams {
        nx: grid.nx(),
        ny: grid.ny(),
        n_voxels,
        lane_len: grid.lane_len(),
        n_substances: N_SUBSTANCES,
        n_reactions: 1,
        tick: 7,
        run_key: run_key(42),
        width_mask: world.registry().width_mask(),
        s_energy: S_ENERGY,
        lod: ENTHALPY_LOD,
        cnx: world.enthalpy_grid().nx(),
        cny: world.enthalpy_grid().ny(),
        volume: Q::ONE,
        dt: Q::ONE,
    };

    let temperature = vec![Q::from_f64(300.0); world.enthalpy_grid().n_voxels() as usize];
    let catalyst = vec![Q::ZERO; n_voxels as usize];

    // The energy increment goes into a local buffer and is copied into the
    // world's afterwards: `amount_slices_mut` and `energy_delta_mut` are two
    // mutable borrows of one `World` and cannot be held at once. That is a gap
    // in the aggregate rather than in the test — see the note in the summary of
    // this wave — and it does not touch what is being asserted here, which is
    // where the *amounts* were read from and written to.
    let mut energy = vec![M64::ZERO; n_voxels as usize];
    {
        let rx = tables.rx();
        let (src32, src64, dst32, dst64) = world.amount_slices_mut();
        for idx in 0..n_voxels {
            react_voxel(
                src32,
                src64,
                dst32,
                dst64,
                &mut energy,
                &temperature,
                &catalyst,
                &rx,
                &params,
                idx,
            );
        }
    }
    world.energy_delta_mut().copy_from_slice(&energy);

    // The reaction wrote state `N+1`; promote it, so that the next reader sees
    // one coherent snapshot (ADR-057).
    if let Some(field) = world.amounts_32_mut() {
        field.swap();
    }
    if let Some(field) = world.amounts_64_mut() {
        field.swap();
    }
}

/// ADR-056, and `ACCEPTANCE.md` under the same name — the host-side half of it.
///
/// Two worlds over the same four substances: one registry with `C` stored 64-bit
/// and one artificially uniform. Seeded identically **by substance index**, run
/// through the same sequence, and read back by substance index. The two must
/// agree bit for bit.
///
/// What it catches that the kernel-side test of the same name cannot:
/// `world.amounts_32_mut()` reached with an `s` where a lane belongs. That call
/// does not panic — a lane of that number probably exists over there — so
/// somebody else's amounts are read and written, the ledger closes because
/// nothing was lost, and on a registry whose wide substance stands first the
/// expression is even right for `s == 0`.
#[test]
fn mixed_width_storage_matches_uniform_width_storage() {
    let mut mixed = seeded(mixed_registry());
    let mut uniform = seeded(narrow_registry());

    // The premise: on one of the two registries a lane is not its substance
    // index, and on the other it is. Without this the test compares two copies
    // of the same addressing.
    assert_eq!(mixed.lane_of(B), LaneRef::Wide(0));
    assert_eq!(mixed.lane_of(C), LaneRef::Narrow(1));
    assert_eq!(mixed.lane_of(D), LaneRef::Narrow(2));
    assert_eq!(uniform.lane_of(C), LaneRef::Narrow(2));
    assert_eq!(uniform.lane_of(D), LaneRef::Narrow(3));
    assert_eq!(mixed.registry().width_mask(), 1 << B);
    assert_eq!(uniform.registry().width_mask(), 0);
    assert!(mixed.amounts_64().is_some());
    assert!(uniform.amounts_64().is_none());

    let total = |world: &World, s: u32| -> i64 {
        (0..world.grid().n_voxels())
            .map(|idx| amount_of(world, s, idx))
            .sum()
    };
    let before: Vec<i64> = (0..N_SUBSTANCES).map(|s| total(&mixed, s)).collect();

    for tick in 0..3 {
        run_a_tick(&mut mixed);
        run_a_tick(&mut uniform);

        for s in 0..N_SUBSTANCES {
            for idx in 0..mixed.grid().n_voxels() {
                assert_eq!(
                    amount_of(&mixed, s, idx),
                    amount_of(&uniform, s, idx),
                    "tick {tick}: substance {s} of voxel {idx} came out different \
                     under the two registries — a lane was resolved on the host \
                     from the substance index"
                );
            }
        }
    }

    // And the run did something, or the assertions above are about a world that
    // never moved. Stated on domain totals rather than on single voxels:
    // diffusion conserves exactly and flattens the seeded gradient, so a
    // per-voxel comparison against the starting pool measures the smoothing
    // rather than the chemistry.
    //
    // The extent is a whole number — `xi = vmax*dt*V*2^e_r = 4` — so the
    // stochastic rounding of ADR-027 does not enter, and the arithmetic is
    // exact: `A + 2 B -> C` at four turnovers, in every voxel, for three ticks.
    let n_voxels = i64::from(mixed.grid().n_voxels());
    const TURNOVERS: i64 = 4 * 3;
    assert_eq!(total(&mixed, A) - before[A as usize], -TURNOVERS * n_voxels);
    assert_eq!(
        total(&mixed, B) - before[B as usize],
        -2 * TURNOVERS * n_voxels
    );
    assert_eq!(total(&mixed, C) - before[C as usize], TURNOVERS * n_voxels);
    assert_eq!(total(&mixed, D) - before[D as usize], 0, "D takes no part");
    assert!(
        mixed.energy_delta().iter().any(|&e| e != M64::ZERO),
        "the reaction released no energy"
    );

    // Diffusion moved something too: the seeded gradient of A is flatter than it
    // started, which a phase that never dispatched a lane could not do.
    let spread = |world: &World, s: u32| -> i64 {
        let amounts: Vec<i64> = (0..world.grid().n_voxels())
            .map(|idx| amount_of(world, s, idx))
            .collect();
        amounts.iter().max().unwrap() - amounts.iter().min().unwrap()
    };
    assert!(spread(&mixed, A) < 96, "lane A never diffused");
    assert!(spread(&mixed, C) < 96, "lane C never diffused");
    assert_eq!(
        spread(&mixed, B),
        96,
        "B does not diffuse and must not have"
    );

    // The boundary conditions travelled to both coarse grids, and the two coarse
    // grids are different. Neither is asserted anywhere else from outside.
    for face in Face::ALL {
        assert_eq!(
            mixed.enthalpy_grid().boundary(face),
            mixed.grid().boundary(face)
        );
    }
    assert_ne!(mixed.enthalpy_grid(), mixed.velocity_grid());
}
