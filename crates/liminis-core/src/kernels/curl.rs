//! The velocity: a narrow curl of the stored potential, and the sampling of it
//! onto the faces of the fine grid.
//!
//! ```text
//! u = curl A
//! ```
//!
//! Last two of the four dispatches of step `b` (ADR-069). [`curl_voxel`] runs
//! over the velocity grid — `2.1e6` scalar reads a tick at 64^3 with a convective
//! potential whose `z` component is zero, `3.15e6` with stirring on — and
//! [`face_courant_voxel`] runs over the **fine** grid and produces exactly the
//! buffer `kernels/advect.rs` reads.
//!
//! # What is stored is `A`, and that is the whole argument
//!
//! `div u = 0` holds because `u` is a curl of stored values rather than a field
//! that has been adjusted afterwards. Each component of `A` enters the discrete
//! `div(curl A)` twice with opposite signs, on the same stored number — so the
//! identity is algebraic and not approximate, and it survives the switch to
//! `FIXED`, where the rounding happens on the **input** of the difference.
//!
//! Two things would take that away, and both look harmless:
//!
//! - a global multiplier over `u`. `qmul` rounds each component separately
//!   (ADR-022), so a rescaled field is no longer the curl of anything. ADR-069
//!   rejects the run-time normalisation it would serve, by name;
//! - a per-component clip. Clipping does not commute with a curl at all:
//!   `div u != 0` appears exactly where the field is strongest, which is the
//!   plume, and matter piles up in the convergence zone — an artefact
//!   indistinguishable from the result the project is looking for.
//!
//! Neither is here, and neither may arrive. The bound on `|u|` is derived at load
//! from the composed stencil (`process/velocity.rs`) so that no run-time measure
//! is needed at all.
//!
//! **What "bit for bit" can and cannot mean, stated here rather than discovered.**
//! The cancellation is exact in the *coefficients*: expand `div(curl A)` and
//! every stored `A` appears twice with opposite signs. It is **not** exact in
//! `f32` when the divergence is assembled out of the already-rounded `u`, because
//! the four differences that have to cancel are four separately rounded numbers
//! and floating-point subtraction does not associate. The property that does hold
//! bit for bit, and that
//! `prescribed_velocity_is_divergence_free_bit_for_bit` asserts, is the one on a
//! potential supported at a single cell: there the two occurrences are equal in
//! magnitude and cancel exactly, which is precisely the algebraic statement, and
//! it is what a per-component multiplier and a clip both fail. On a full field the
//! residual is bounded by rounding and is asserted as such.
//!
//! # The floor
//!
//! `w = 0` on the plane `z = 0` comes out of the potential rather than out of a
//! branch here: `u_z` reads only `A_x` and `A_y` **inside its own plane**, and
//! both are zeroed there by `kernels/potential.rs` and `kernels/noise.rs`. So a
//! wall in this file would be a second mechanism for the same thing, and the one
//! that already exists keeps the field a curl.
//!
//! # The layout of the courant buffer is the contract
//!
//! `courant[axis*n_voxels + idx]` is the Courant number on the **lower** face of
//! fine voxel `idx` along `axis` — the layout `advect_voxel_32` reads, and the
//! layout in which the canonical orientation of a face lives (ADR-054). The other
//! plausible spelling, `idx*3 + axis`, conserves matter exactly and gives a
//! believable flow at an angle; so does a permutation of the axes. Only
//! `the_face_courant_layout_matches_the_advection_kernel` separates them.
//!
//! # The fold onto the coarse faces adds **flux**, and that is the whole record
//!
//! [`coarse_face_courant_voxel`] is the fifth dispatch of step `b` (ADR-087). The
//! enthalpy field lives on a grid coarser than the velocity grid (`lod = 2`
//! against `lod = 1`, ADR-062), so [`face_courant_voxel`] cannot serve it: its own
//! `debug_assert` wants a target grid that is a *refinement* of the velocity grid.
//! What the coarse faces want is the direction ADR-045 already walks — fine to
//! coarse, gathered by the coarse cell — but for a **flux** rather than for a
//! quantity, and that one word is what SPEC section 1.5 does not cover.
//!
//! **A Courant number is intensive, and intensive numbers are not added.** Six
//! tenths and six tenths are not one and two tenths. What adds is the volumetric
//! flux through a face; the division comes afterwards:
//!
//! ```text
//! Phi_i = u_i * dx_fine^2                     flux through one fine face
//! A_coarse = 2^(2*lod) * dx_fine^2            the coarse face is tiled by them
//! u_bar = sum(Phi_i)/A_coarse = 2^(-2*lod) * sum(u_i)
//! C_coarse = u_bar*dt/dx_coarse = 2^(-3*lod) * sum(C_i)
//! ```
//!
//! At `lod = 2`: **add sixteen and divide by sixty-four.** The averaging is of the
//! *velocity*, never of the Courant number — the two differ by exactly `2^lod`
//! because the fine and the coarse face carry different `dx`, and that miss is the
//! one no residual can see: transport conserves at any Courant number, both
//! residuals stay at exactly zero, the temperature stays plausible, and the world
//! gets heat outrunning fourfold the matter that carries it. At the validator's own
//! ceiling `|C_fine| <= 1/6` the axial sum of the mean-of-Courant form comes out at
//! a third of the debug threshold, so only
//! `a_uniform_flow_has_the_same_speed_on_the_fine_and_the_coarse_faces` separates
//! them.
//!
//! Two exponents, and they are not the same one. Sixteen is `(2^lod)^2`, the two
//! transverse extents of a coarse **face**; sixty-four is `2^(3*lod)`, the fine
//! **cells** of a coarse cell — the number `kernels/fold.rs` runs its cube over.
//! The exponent is counted down to the grid of the faces being added, which is the
//! fine grid of the amounts (`128^3`), and not down to the velocity grid: whoever
//! adds the four faces of a velocity-grid Courant buffer owes a divisor of eight
//! instead — and no such buffer exists, so the fine one is the only source there
//! is. Nothing in the types says so.
//!
//! Two more traps live here, both quiet, both borrowed from `kernels/fold.rs` and
//! moved onto faces. The covering block is a shift **per axis** (SPEC section 1.5):
//! the linear form `fine = (coarse << (3*lod)) + f` partitions the index space just
//! as disjointly, passes every conservation test, and credits the coarse face a
//! stripe of sixteen along X instead of the 4x4 square lying on it. And the fold
//! runs over the faces **on** the coarse face, not over all the faces inside the
//! coarse cell: the latter are four times as many and belong to other positions —
//! a factor in the same direction as the mean-of-Courant miss, so the two would
//! cancel into a right-looking number for the wrong reason.
//!
//! The stability of the coarse step follows from the fine one and needs no third
//! `SpeedBound` at load, which ADR-087 refuses by name so that the next author does
//! not add one for safety. The derivation runs off the validator's ceiling and not
//! off `|C| <= 1`: `config/validate.rs` declares `outgoing_faces: 6`, so
//! `|C_fine| <= 1/6`, and the divisor grows with the number of terms —
//! `|C_coarse| <= (1/6)/2^lod = 1/24`, with `2/24 = 1/12` over the two outgoing
//! faces of an axis under the splitting of ADR-036. Twelve times inside the
//! condition; from the premise `max|C| <= 1` the derivation does **not** go
//! through. `a_fine_field_within_the_courant_bound_stays_within_it_after_the_fold`
//! asserts it directly rather than by argument, because a broken fold breaks it
//! before any validator sees anything.

use crate::numeric::{Q, qadd, qdiv, qmul, qsub};

/// Parameters of the curl. Scalars only: in WGSL this is a uniform buffer.
#[derive(Clone, Copy, Debug)]
pub struct CurlParams {
    /// Cells of the velocity grid along X.
    pub nx: u32,
    /// Cells of the velocity grid along Y.
    pub ny: u32,
    /// Cells of the velocity grid along Z.
    pub nz: u32,
    /// Bit `f` set: face `f` is periodic. Bit clear: the face is closed and its
    /// neighbour is the cell itself, as in `kernels/diffuse.rs` and
    /// `kernels/pressure.rs`.
    ///
    /// The bit index is the discriminant of `world::Face`.
    pub periodic_mask: u32,
    /// `1/(2*dx_velocity)`, folded on the host: the narrow central difference
    /// divided by the step of the **velocity** grid, which is twice the fine step
    /// in the eco regime.
    ///
    /// Taking the fine step here instead doubles every velocity, and the only
    /// symptom is a Courant number twice what the validator checked — which is
    /// the one thing ADR-069 arranges to be known before the first tick.
    pub gain: Q,
}

/// The velocity of one cell of the velocity grid.
///
/// `idx` runs over `0..nx*ny*nz`. Reads `a` at six neighbours and writes the
/// three components of its own cell; reads no cell of `dst`.
///
/// ```text
/// u_x = d_y A_z - d_z A_y
/// u_y = d_z A_x - d_x A_z
/// u_z = d_x A_y - d_y A_x
/// ```
///
/// Both buffers are component-major, `[c*n_cells + idx]`, the layout
/// `kernels/potential.rs` writes.
///
/// # A closed face folds onto the cell itself
///
/// The difference across such an axis is then between the cell and itself on one
/// side, that is a one-sided difference carrying the gain of a two-sided one. It
/// is not the exact derivative at a wall and does not have to be: the potential
/// is prescribed, not solved, and what the wall owes is `w = 0` on the floor,
/// which comes out of the zeroed tangential potential and holds under this
/// lookup exactly.
pub fn curl_voxel(a: &[Q], dst: &mut [Q], p: &CurlParams, idx: u32) {
    let n_cells = p.nx * p.ny * p.nz;

    let x_up = neighbour(p, idx, 1);
    let x_down = neighbour(p, idx, 0);
    let y_up = neighbour(p, idx, 3);
    let y_down = neighbour(p, idx, 2);
    let z_up = neighbour(p, idx, 5);
    let z_down = neighbour(p, idx, 4);

    let ax = 0;
    let ay = n_cells;
    let az = 2 * n_cells;

    let d_y_az = difference(a, az + y_up, az + y_down, p);
    let d_z_ay = difference(a, ay + z_up, ay + z_down, p);
    let d_z_ax = difference(a, ax + z_up, ax + z_down, p);
    let d_x_az = difference(a, az + x_up, az + x_down, p);
    let d_x_ay = difference(a, ay + x_up, ay + x_down, p);
    let d_y_ax = difference(a, ax + y_up, ax + y_down, p);

    dst[idx as usize] = qsub(d_y_az, d_z_ay);
    dst[(n_cells + idx) as usize] = qsub(d_z_ax, d_x_az);
    dst[(2 * n_cells + idx) as usize] = qsub(d_x_ay, d_y_ax);
}

/// One narrow central difference of the stored potential, already scaled.
///
/// One multiplication per difference rather than one per component of `u`: the
/// two occurrences of a stored `A` in `div(curl A)` are then the same rounded
/// product, which is where the exactness of the divergence comes from.
#[inline(always)]
fn difference(a: &[Q], up: u32, down: u32, p: &CurlParams) -> Q {
    qmul(p.gain, qsub(a[up as usize], a[down as usize]))
}

/// Parameters of the sampling onto the faces of the fine grid.
#[derive(Clone, Copy, Debug)]
pub struct SampleParams {
    /// Voxels of the **fine** grid along X.
    pub nx: u32,
    /// Voxels of the fine grid along Y.
    pub ny: u32,
    /// Voxels of the fine grid along Z.
    pub nz: u32,
    /// Cells of the velocity grid along X.
    pub vnx: u32,
    /// Cells of the velocity grid along Y.
    pub vny: u32,
    /// Cells of the velocity grid along Z.
    pub vnz: u32,
    /// Periodicity, encoded as in [`CurlParams::periodic_mask`].
    pub periodic_mask: u32,
    /// `log2(nx/vnx)`: how many times coarser the velocity grid is than the fine
    /// one. One in the eco regime, where the fine grid is 128^3 and the velocity
    /// grid 64^3 (ADR-069).
    pub ratio_log2: u32,
    /// `dt/dx` on the **fine** grid, folded on the host.
    ///
    /// The fine step and not the velocity one: the flux is computed on 128^3, and
    /// substituting 200 um would double the admissible speed with no basis
    /// (ADR-069). The whole ceiling of the scheme, `dx/(6*dt)`, is a statement
    /// about this number.
    pub courant_gain: Q,
}

/// The Courant numbers on the three lower faces of one fine voxel.
///
/// `idx` runs over `0..nx*ny*nz`. Writes `dst[axis*lane_len + idx]` for the three
/// axes and nothing else — exactly the layout and exactly the meaning
/// `advect_voxel_32` reads, with `lane_len = nx*ny*nz + 1` because the ghost cell
/// of ADR-059 owns the Courant number of the face of the domain. This kernel
/// never writes that cell: it is a face of the *domain* and not of a voxel, and
/// what belongs in it is undecided (`TODO(exchange-courant)` in
/// `kernels/advect.rs`).
///
/// # Where the sample is taken
///
/// The lower face of voxel `idx` along `axis` is at the voxel's own coordinate on
/// that axis and at the **centre** of the other two. Sampling at the cell centre
/// on all three axes instead is the natural slip and gives a half-voxel shift of
/// the whole flow along one axis — conserving matter exactly, and looking like a
/// flow.
pub fn face_courant_voxel(u: &[Q], dst: &mut [Q], p: &SampleParams, idx: u32) {
    debug_assert!(
        p.nx == p.vnx << p.ratio_log2
            && p.ny == p.vny << p.ratio_log2
            && p.nz == p.vnz << p.ratio_log2,
        "the fine grid {}x{}x{} is not the velocity grid {}x{}x{} refined by 2^{}",
        p.nx,
        p.ny,
        p.nz,
        p.vnx,
        p.vny,
        p.vnz,
        p.ratio_log2
    );

    let n_voxels = p.nx * p.ny * p.nz;
    let n_velocity = p.vnx * p.vny * p.vnz;
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    let ratio = 1u32 << p.ratio_log2;
    let denominator = 4 * ratio;
    let weight_log2 = p.ratio_log2 + 2;

    for axis in 0..3u32 {
        // `FACE` on the axis being sampled, `CENTRE` on the other two.
        let along_x = sample_numerator(x, if axis == 0 { FACE } else { CENTRE }, ratio);
        let along_y = sample_numerator(y, if axis == 1 { FACE } else { CENTRE }, ratio);
        let along_z = sample_numerator(z, if axis == 2 { FACE } else { CENTRE }, ratio);

        let base_x = along_x / denominator;
        let base_y = along_y / denominator;
        let base_z = along_z / denominator;

        let periodic_x = p.periodic_mask & 1 == 1;
        let periodic_y = (p.periodic_mask >> 2) & 1 == 1;
        let periodic_z = (p.periodic_mask >> 4) & 1 == 1;

        let lo = [
            corner_low(base_x, p.vnx, periodic_x),
            corner_low(base_y, p.vny, periodic_y),
            corner_low(base_z, p.vnz, periodic_z),
        ];
        let hi = [
            corner_high(base_x, p.vnx, periodic_x),
            corner_high(base_y, p.vny, periodic_y),
            corner_high(base_z, p.vnz, periodic_z),
        ];
        let weight_high = [
            unit_fraction(along_x - base_x * denominator, weight_log2),
            unit_fraction(along_y - base_y * denominator, weight_log2),
            unit_fraction(along_z - base_z * denominator, weight_log2),
        ];

        let mut sampled = Q::ZERO;
        for corner in 0..8u32 {
            let mut weight = Q::ONE;
            let mut at = 0u32;
            let mut stride = 1u32;
            for component in 0..3u32 {
                let extent = if component == 0 {
                    p.vnx
                } else if component == 1 {
                    p.vny
                } else {
                    p.vnz
                };
                let high = (corner >> component) & 1 == 1;
                let coord = if high {
                    hi[component as usize]
                } else {
                    lo[component as usize]
                };
                let share = if high {
                    weight_high[component as usize]
                } else {
                    qsub(Q::ONE, weight_high[component as usize])
                };
                weight = qmul(weight, share);
                at += coord * stride;
                stride *= extent;
            }
            sampled = qadd(sampled, qmul(weight, u[(axis * n_velocity + at) as usize]));
        }

        // `base + idx`, with the stride taken out first. The spelling matters:
        // `advect.rs` writes the same address the same way, and the grep of the
        // `kernel-lint` hook reads a multiplication by `n_voxels` next to an
        // identifier as lane-index addressing (ADR-056) — which this is not, the
        // stride here being an **axis** and not a substance.
        let base = axis * (n_voxels + 1);
        dst[(base + idx) as usize] = qmul(p.courant_gain, sampled);
    }
}

/// Parameters of the fold onto the faces of the coarse enthalpy grid (ADR-087).
#[derive(Clone, Copy, Debug)]
pub struct CoarseCourantParams {
    /// Voxels along X **of the fine grid**, the one whose faces are added. The
    /// coarse extents are never passed in: they are `nx >> lod` and so on,
    /// derived per axis (SPEC section 1.5), and a second set of three numbers
    /// would be a second place for them to disagree with the first — the rule
    /// `FoldParams` states and this file obeys.
    pub nx: u32,
    /// Voxels along Y of the fine grid.
    pub ny: u32,
    /// Voxels along Z of the fine grid.
    pub nz: u32,
    /// How many bits coarser the enthalpy grid is than the fine one: `2^(2*lod)`
    /// fine faces tile one coarse face, sixteen at `lod = 2` (ADR-062).
    ///
    /// Checked and never declared: `lod_between` in `process/velocity.rs` derives
    /// it by comparing the two grids, and this is its third caller.
    pub lod: u32,
    /// `2^(-3*lod)`, folded on the host beside `courant_gain`.
    ///
    /// One number and one `qmul`, so no bare operator over `Q` appears (ADR-022)
    /// and the power of two is exact in both modes. The kernel knows neither
    /// `dt`, nor `dx`, nor the `lod` as a length (ADR-015) — what it knows is
    /// that the sum of the covering faces has to be divided by this.
    ///
    /// `2^(-2*lod)` here is the arithmetic mean of the Courant numbers, which
    /// overstates the coarse flux by exactly `2^lod` and is seen by neither
    /// residual; the module header carries the derivation.
    pub fold_gain: Q,
}

/// The Courant numbers on the three lower faces of one **coarse** cell.
///
/// `coarse` runs over `0..(nx>>lod)*(ny>>lod)*(nz>>lod)` — the coarse grid, which
/// is what makes this kernel a fold rather than a sampling (ADR-045: "the same
/// kernel under the same signature, only its linear index runs over the coarse
/// grid and it reads the fine one"). Reads `2^(2*lod)` cells of `fine` per axis
/// and writes `dst[axis*(n_coarse + 1) + coarse]` for the three axes and nothing
/// else: pure gather, no atomic, no write into anybody else's cell (ADR-034).
///
/// The ghost cell of the coarse lane — index `n_coarse`, the face of the *domain*
/// (ADR-059) — is never written, exactly as [`face_courant_voxel`] never writes
/// the fine one. `TODO(exchange-courant)` stays one hole and does not become two:
/// a fold that wrote it would give the lid a flux no channel accounts for, and
/// the enthalpy would leave the domain outside the ledger.
///
/// # Which sixteen faces
///
/// The ones lying **on** the coarse face, that is the lower faces of the fine
/// voxels along the coarse cell's lower boundary on that axis: for X, the fine
/// voxels at `x = cx << lod` spanning the whole `2^lod x 2^lod` square in Y and Z.
/// Gathering the *upper* covering faces instead (`x0 = (cx + 1) << lod`) shifts
/// the entire coarse flow by one coarse cell along every axis while conserving
/// exactly, and nothing in the fine buffer's layout marks which end a face belongs
/// to except the convention of ADR-054.
///
/// # The two lane lengths are different `u32`s
///
/// The fine buffer is addressed `axis*(n_fine + 1) + fine`, the coarse one
/// `axis*(n_coarse + 1) + coarse`; both carry the ghost of ADR-059 and the
/// compiler cannot tell the two lengths apart. Reading the fine buffer with
/// `axis*n_fine` shifts the Y-axis Courant by one cell and the Z-axis by two, with
/// conservation exact and the flow plausible either way.
pub fn coarse_face_courant_voxel(fine: &[Q], dst: &mut [Q], p: &CoarseCourantParams, coarse: u32) {
    // The dispatch domain, asserted rather than only documented, on the precedent
    // of `fold_energy`: a host that dispatched this over the fine grid gets a `cz`
    // past the coarse field, and the addressing below then reads past the end of
    // `fine`. Here that is a panic; in WGSL it is an out-of-bounds access the
    // specification allows to land on another binding.
    debug_assert!(
        coarse < n_coarse_cells(p),
        "coarse_face_courant_voxel is dispatched over coarse cells, not fine \
         voxels: coarse {coarse} is past their count {}",
        n_coarse_cells(p)
    );

    let n_fine = p.nx * p.ny * p.nz;
    let n_coarse = n_coarse_cells(p);
    let (cx, cy, cz) = coarse_coords(p, coarse);

    // The origin of the covering block, per axis. The inverse of the shift of
    // SPEC section 1.5, written per axis for the reason `kernels/fold.rs` writes
    // its own that way: a linear `coarse << (3*lod)` names a stripe along X.
    let x0 = cx << p.lod;
    let y0 = cy << p.lod;
    let z0 = cz << p.lod;
    let span = 1u32 << p.lod;

    for axis in 0..3u32 {
        // A flat sum of the `2^(2*lod)` terms and one division at the end, and
        // that is the form ADR-087 asks for while the scales of `Q` in `FIXED`
        // are undecided: a pairwise tree is equally legal in `FLOAT` and has to
        // decompose its divisor as two per level on the first three levels and
        // **eight** at the root. Halving at every level gives exactly
        // `mean(C_i)` — the `2^lod` miss this whole record exists to refuse — so
        // the form whose divisor cannot be decomposed wrongly is the one written
        // until that decision is made.
        let mut flux = Q::ZERO;

        // Two loops over the plane of the face and none along the axis itself:
        // the faces are the ones lying **on** the coarse face, not the
        // `2^(3*lod)` faces inside the coarse cell.
        for b in 0..span {
            for a in 0..span {
                let (x, y, z) = if axis == 0 {
                    (x0, y0 + a, z0 + b)
                } else if axis == 1 {
                    (x0 + a, y0, z0 + b)
                } else {
                    (x0 + a, y0 + b, z0)
                };
                // The stride of the **fine** lane, spelled with the parenthesis
                // `face_courant_voxel` spells it with: the `kernel-lint` grep
                // reads a multiplication by a voxel count next to an identifier
                // as lane-index addressing (ADR-056), which an axis is not.
                let at = axis * (n_fine + 1) + fine_index(p, x, y, z);
                flux = qadd(flux, fine[at as usize]);
            }
        }

        // The one multiplication, on the folded gain. Its exponent is the whole
        // of the record: `2^(-3*lod)` and not `2^(-2*lod)`.
        dst[(axis * (n_coarse + 1) + coarse) as usize] = qmul(p.fold_gain, flux);
    }
}

/// The three coarse coordinates of a coarse linear index.
///
/// Divided by the **coarse** extents, `nx >> lod` and `ny >> lod`. Dividing by the
/// fine ones is the mirror of the linear-covering mistake and is partly masked on
/// a cubic grid, which is why every fixture below runs on a grid whose three
/// coarse extents are pairwise different. The twin of the function of the same
/// name in `kernels/fold.rs`.
#[inline(always)]
fn coarse_coords(p: &CoarseCourantParams, coarse: u32) -> (u32, u32, u32) {
    let cnx = p.nx >> p.lod;
    let cny = p.ny >> p.lod;

    let z = coarse / (cnx * cny);
    let rest = coarse - z * cnx * cny;
    let y = rest / cnx;
    let x = rest - y * cnx;

    (x, y, z)
}

/// How many cells the coarse grid has: the product of the shifted extents.
#[inline(always)]
fn n_coarse_cells(p: &CoarseCourantParams) -> u32 {
    (p.nx >> p.lod) * (p.ny >> p.lod) * (p.nz >> p.lod)
}

/// The linear index of a **fine** voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
///
/// This file's own copy, for the reason every kernel keeps one: `kernels/` depends
/// on `numeric/` and on nothing else. Held to its authority by
/// `the_fine_indexing_of_the_fold_agrees_with_the_grid`, the twin of the test of
/// the same shape in `fold.rs` — a transposed copy would be self-consistent inside
/// this module and invisible to every other test in it.
#[inline(always)]
fn fine_index(p: &CoarseCourantParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

/// The sample point is the lower face of the voxel: its own coordinate exactly.
const FACE: u32 = 0;
/// The sample point is the centre of the voxel, half a voxel up from its origin.
const CENTRE: u32 = 2;

/// The position of a sample point in units of `1/(4*ratio)` of a velocity cell,
/// shifted up by `2*ratio` so that it is never negative.
///
/// The twin of the function of the same name in `kernels/potential.rs`; the
/// derivation of the shift is written out there.
#[inline(always)]
fn sample_numerator(coord: u32, half: u32, ratio: u32) -> u32 {
    4 * coord + half + 2 * ratio
}

/// The lower of the two corners along one axis, given `base + 1`.
#[inline(always)]
fn corner_low(base_plus_one: u32, extent: u32, periodic: bool) -> u32 {
    if base_plus_one > 0 {
        base_plus_one - 1
    } else if periodic {
        extent - 1
    } else {
        0
    }
}

/// The upper of the two corners along one axis, given `base + 1`.
#[inline(always)]
fn corner_high(base_plus_one: u32, extent: u32, periodic: bool) -> u32 {
    if base_plus_one < extent {
        base_plus_one
    } else if periodic {
        0
    } else {
        extent - 1
    }
}

/// `numerator * 2^-log2_denominator`, exactly. See the twin in
/// `kernels/potential.rs` for why a kernel builds this rather than receiving it.
#[inline(always)]
fn unit_fraction(numerator: u32, log2_denominator: u32) -> Q {
    let two = qadd(Q::ONE, Q::ONE);
    let mut unit = Q::ONE;
    for _ in 0..log2_denominator {
        unit = qdiv(unit, two);
    }
    let mut value = Q::ZERO;
    for _ in 0..numerator {
        value = qadd(value, unit);
    }
    value
}

/// The linear index of a cell: `x + y*NX + z*NX*NY` (SPEC section 1.1).
#[inline(always)]
fn index(p: &CurlParams, x: u32, y: u32, z: u32) -> u32 {
    x + y * p.nx + z * p.nx * p.ny
}

/// One step down an axis, or what the boundary says instead.
#[inline(always)]
fn step_down(coord: u32, extent: u32, periodic: bool) -> u32 {
    if coord > 0 {
        coord - 1
    } else if periodic {
        extent - 1
    } else {
        coord
    }
}

/// One step up an axis, or what the boundary says instead.
#[inline(always)]
fn step_up(coord: u32, extent: u32, periodic: bool) -> u32 {
    if coord + 1 < extent {
        coord + 1
    } else if periodic {
        0
    } else {
        coord
    }
}

/// The neighbour of a cell across one of its six faces.
///
/// `face` is the discriminant of `world::Face`, because WGSL has no enums. A
/// closed face returns the cell itself, as in `kernels/pressure.rs`.
#[inline(always)]
fn neighbour(p: &CurlParams, idx: u32, face: u32) -> u32 {
    let plane = p.nx * p.ny;
    let z = idx / plane;
    let within_plane = idx - z * plane;
    let y = within_plane / p.nx;
    let x = within_plane - y * p.nx;

    let periodic = (p.periodic_mask >> face) & 1 == 1;

    let mut nbx = x;
    let mut nby = y;
    let mut nbz = z;
    if face == 0 {
        nbx = step_down(x, p.nx, periodic);
    } else if face == 1 {
        nbx = step_up(x, p.nx, periodic);
    } else if face == 2 {
        nby = step_down(y, p.ny, periodic);
    } else if face == 3 {
        nby = step_up(y, p.ny, periodic);
    } else if face == 4 {
        nbz = step_down(z, p.nz, periodic);
    } else {
        nbz = step_up(z, p.nz, periodic);
    }

    index(p, nbx, nby, nbz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernels::advect::{AdvectParams, advect_voxel_32};
    use crate::numeric::{M32, rand, run_key};
    use crate::world::{Boundary, Grid};
    use proptest::prelude::*;

    /// A deliberately non-cubic velocity grid.
    const NX: u32 = 8;
    const NY: u32 = 6;
    const NZ: u32 = 4;
    const N_CELLS: u32 = NX * NY * NZ;
    const TORUS: u32 = 0b11_1111;
    const FLOORED: u32 = 0b00_1111;

    fn params(mask: u32) -> CurlParams {
        CurlParams {
            nx: NX,
            ny: NY,
            nz: NZ,
            periodic_mask: mask,
            // A power of two, so that the multiplication is exact and the
            // assertions below are about the stencil rather than about `f32`.
            gain: Q::from_f64(0.25),
        }
    }

    fn curl(a: &[Q], p: &CurlParams) -> Vec<Q> {
        let mut dst = vec![Q::ZERO; (3 * N_CELLS) as usize];
        for idx in 0..N_CELLS {
            curl_voxel(a, &mut dst, p, idx);
        }
        dst
    }

    fn cell(x: u32, y: u32, z: u32) -> u32 {
        x + y * NX + z * NX * NY
    }

    /// The discrete divergence of `u`, assembled with the same narrow differences
    /// the curl uses.
    fn divergence(u: &[Q], p: &CurlParams) -> Vec<Q> {
        let mut out = vec![Q::ZERO; N_CELLS as usize];
        for idx in 0..N_CELLS {
            let d_x = difference(u, neighbour(p, idx, 1), neighbour(p, idx, 0), p);
            let d_y = difference(
                u,
                N_CELLS + neighbour(p, idx, 3),
                N_CELLS + neighbour(p, idx, 2),
                p,
            );
            let d_z = difference(
                u,
                2 * N_CELLS + neighbour(p, idx, 5),
                2 * N_CELLS + neighbour(p, idx, 4),
                p,
            );
            out[idx as usize] = qadd(qadd(d_x, d_y), d_z);
        }
        out
    }

    #[test]
    fn prescribed_velocity_is_divergence_free_bit_for_bit() {
        // The exact half of the property, on a potential supported at a single
        // cell: there each stored value enters the divergence twice with equal
        // magnitude and opposite sign, so the cancellation is on the same rounded
        // number and the result is `Q::ZERO` by equality and not by tolerance.
        //
        // Both boundary masks, because a closed face folds the neighbour onto the
        // cell itself and a wall is where a curl usually stops being one. Both
        // stirred and unstirred is the caller's axis and is covered in
        // `tests/acceptance_velocity.rs`; here the input is the buffer either of
        // them produces.
        for mask in [TORUS, FLOORED] {
            let p = params(mask);
            for component in 0..3u32 {
                for at in [cell(3, 2, 1), cell(0, 0, 0), cell(NX - 1, NY - 1, NZ - 1)] {
                    let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
                    a[(component * N_CELLS + at) as usize] = Q::from_f64(1.75);

                    for (idx, value) in divergence(&curl(&a, &p), &p).iter().enumerate() {
                        assert_eq!(
                            *value,
                            Q::ZERO,
                            "mask {mask:#08b}, component {component}, impulse at \
                             {at}, cell {idx}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_per_component_multiplier_is_not_divergence_free() {
        // The assertion above is worth nothing without this one: it says the test
        // can fail. A field scaled component by component — the "cheap limiter"
        // ADR-069 rejects because clipping does not commute with a curl — stops
        // being divergence-free on the very same impulse.
        let p = params(TORUS);
        let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
        a[cell(3, 2, 1) as usize] = Q::from_f64(1.75);

        // The impulse is in `A_x`, so `u_y` and `u_z` are the components that
        // carry it; scaling `u_x`, which is identically zero here, would prove
        // nothing. One component and not all three: a uniform multiplier is a
        // different mistake, and `f32` cannot always see it.
        let mut u = curl(&a, &p);
        for value in u.iter_mut().skip(N_CELLS as usize).take(N_CELLS as usize) {
            *value = qmul(*value, Q::from_f64(0.5));
        }
        assert!(divergence(&u, &p).iter().any(|v| *v != Q::ZERO));
    }

    #[test]
    fn the_divergence_of_a_full_field_is_zero_to_rounding() {
        // The other half, stated as what it is. On an arbitrary potential the four
        // differences that have to cancel are four separately rounded numbers, and
        // `f32` subtraction does not associate — so the residual is bounded by
        // rounding rather than identically zero. See the module header: the
        // algebraic identity is exact, its evaluation through a stored `u` is not.
        let p = params(TORUS);
        let key = run_key(4242);
        let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
        for (at, value) in a.iter_mut().enumerate() {
            let draw = rand(at as u32, 0, 1, key);
            *value = Q::from_f64(f64::from(draw) / f64::from(u32::MAX) - 0.5);
        }

        let u = curl(&a, &p);
        let scale = u.iter().fold(0.0f64, |m, v| m.max(v.debug_f64().abs()));
        assert!(scale > 0.0);
        for (idx, value) in divergence(&u, &p).iter().enumerate() {
            assert!(
                value.debug_f64().abs() <= 8.0 * scale * f64::from(f32::EPSILON),
                "cell {idx}: divergence {} against a field of scale {scale}",
                value.debug_f64()
            );
        }
    }

    #[test]
    fn the_curl_turns_a_linear_potential_into_a_uniform_flow() {
        // `A = (0, 0, y)` gives `u = (1, 0, 0)`; `A = (0, x, 0)` gives
        // `u = (0, 0, 1)`. Two of the six terms, with the signs that tell a curl
        // from its transpose — swapping them leaves a field that is still
        // divergence-free and flows the other way.
        let p = params(TORUS);

        let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    a[(2 * N_CELLS + cell(x, y, z)) as usize] = Q::from_f64(f64::from(y));
                }
            }
        }
        let u = curl(&a, &p);
        // Away from the wrap, where the ramp is not a ramp.
        let at = cell(3, 2, 1);
        assert_eq!(u[at as usize], Q::from_f64(2.0 * 0.25));
        assert_eq!(u[(N_CELLS + at) as usize], Q::ZERO);
        assert_eq!(u[(2 * N_CELLS + at) as usize], Q::ZERO);

        let mut a = vec![Q::ZERO; (3 * N_CELLS) as usize];
        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    a[(N_CELLS + cell(x, y, z)) as usize] = Q::from_f64(f64::from(x));
                }
            }
        }
        let u = curl(&a, &p);
        assert_eq!(u[at as usize], Q::ZERO);
        assert_eq!(u[(N_CELLS + at) as usize], Q::ZERO);
        assert_eq!(u[(2 * N_CELLS + at) as usize], Q::from_f64(2.0 * 0.25));
    }

    #[test]
    fn the_floor_carries_no_vertical_velocity() {
        // `w = 0` on the plane `z = 0` follows from the tangential potential being
        // zero there, and from nothing in this file: `u_z` reads `A_x` and `A_y`
        // only at its own `z`. The test therefore feeds a potential with the floor
        // already zeroed, which is what both writers of `A` produce.
        let p = params(FLOORED);
        let mut a = vec![Q::from_f64(0.5); (3 * N_CELLS) as usize];
        for y in 0..NY {
            for x in 0..NX {
                let at = cell(x, y, 0);
                a[at as usize] = Q::ZERO;
                a[(N_CELLS + at) as usize] = Q::ZERO;
            }
        }
        // And a potential that varies, or `w` is zero for the wrong reason.
        for z in 1..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    let at = cell(x, y, z);
                    a[at as usize] = Q::from_f64(f64::from(x + 2 * y + 3 * z));
                    a[(N_CELLS + at) as usize] = Q::from_f64(f64::from(3 * x + y));
                }
            }
        }

        let u = curl(&a, &p);
        for y in 0..NY {
            for x in 0..NX {
                let at = cell(x, y, 0);
                assert_eq!(
                    u[(2 * N_CELLS + at) as usize],
                    Q::ZERO,
                    "w at ({x}, {y}, 0)"
                );
            }
        }
        // The horizontal flow along the floor is not zero, which is the half a
        // blanket zeroing of `A` would destroy.
        assert!(
            u[cell(1, 1, 0) as usize] != Q::ZERO
                || u[(N_CELLS + cell(1, 1, 0)) as usize] != Q::ZERO
        );
    }

    #[test]
    fn the_face_courant_layout_matches_the_advection_kernel() {
        // The buffer this kernel fills is fed straight to `advect_voxel_32` at a
        // constant velocity: the matter has to move along the declared axis, in
        // the declared direction, at `u*dt/dx` on the face. This is what catches
        // permuted axes and the layout `idx*3 + axis` — both of which conserve
        // matter exactly and give a believable flow at an angle.
        const RATIO_LOG2: u32 = 1;
        let fine_nx = NX << RATIO_LOG2;
        let fine_ny = NY << RATIO_LOG2;
        let fine_nz = NZ << RATIO_LOG2;
        let n_voxels = fine_nx * fine_ny * fine_nz;

        for axis in 0..3u32 {
            // A uniform velocity of one along `axis`, zero elsewhere.
            let mut u = vec![Q::ZERO; (3 * N_CELLS) as usize];
            for at in 0..N_CELLS {
                u[(axis * N_CELLS + at) as usize] = Q::ONE;
            }

            let sample = SampleParams {
                nx: fine_nx,
                ny: fine_ny,
                nz: fine_nz,
                vnx: NX,
                vny: NY,
                vnz: NZ,
                periodic_mask: TORUS,
                ratio_log2: RATIO_LOG2,
                courant_gain: Q::from_f64(0.5),
            };
            let lane_len = n_voxels + 1;
            let mut courant = vec![Q::ZERO; (3 * lane_len) as usize];
            for idx in 0..n_voxels {
                face_courant_voxel(&u, &mut courant, &sample, idx);
            }

            // `u*dt/dx` on every face of the declared axis, and nothing at all on
            // the other two.
            let base = axis * lane_len;
            for idx in 0..n_voxels {
                assert_eq!(
                    courant[(base + idx) as usize],
                    Q::from_f64(0.5),
                    "axis {axis}, voxel {idx}"
                );
                for other in 0..3u32 {
                    if other != axis {
                        let elsewhere = other * lane_len;
                        assert_eq!(courant[(elsewhere + idx) as usize], Q::ZERO);
                    }
                }
            }

            // And a tracer really does move along that axis and toward the larger
            // index, which is what the sign convention of `advect.rs` means.
            let advect = AdvectParams {
                exchange_mask: 0,
                nx: fine_nx,
                ny: fine_ny,
                nz: fine_nz,
                periodic_mask: TORUS,
                axis,
            };
            let mut src = vec![M32::ZERO; lane_len as usize];
            let start = 2 + 3 * fine_nx + fine_nx * fine_ny;
            src[start as usize] = M32::new(1024);
            let mut dst = vec![M32::ZERO; lane_len as usize];
            for idx in 0..n_voxels {
                advect_voxel_32(&src, &courant, &mut dst, &advect, idx);
            }

            let downstream = if axis == 0 {
                start + 1
            } else if axis == 1 {
                start + fine_nx
            } else {
                start + fine_nx * fine_ny
            };
            assert!(
                dst[downstream as usize] > M32::ZERO,
                "axis {axis}: nothing arrived at the neighbour toward the larger \
                 index"
            );
            assert!(dst[start as usize] < M32::new(1024));
        }
    }

    #[test]
    fn the_neighbourhood_agrees_with_the_grid() {
        // The kernel carries its own copy of the lookup, because `kernels/` may
        // not depend on `world/`. The same test and the same reason as in
        // `diffuse.rs`.
        let grid = Grid::new(
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
        .unwrap();
        let p = params(FLOORED);
        for idx in 0..N_CELLS {
            for (face, world_face) in crate::world::Face::ALL.iter().enumerate() {
                assert_eq!(
                    neighbour(&p, idx, face as u32),
                    grid.neighbour(idx, *world_face),
                    "cell {idx}, face {face}"
                );
            }
        }
    }

    // --- the fold onto the coarse faces (ADR-087) ---------------------------

    /// The coarse grid every fold fixture below runs on: three **pairwise
    /// different** extents, none of them one.
    ///
    /// Pairwise different for the reason `kernels/fold.rs` states about its own
    /// `2 x 3 x 4`: on a cubic grid every bug that swaps two axes is invisible,
    /// because all three strides are equal, and a decode of the coarse index by
    /// the fine extents is masked besides.
    const CNX: u32 = 2;
    const CNY: u32 = 3;
    const CNZ: u32 = 4;
    const N_COARSE: u32 = CNX * CNY * CNZ;

    /// A fine step that is a power of two, and a tick of one second.
    ///
    /// Dyadic on purpose: the tests below recover a velocity out of a stored
    /// Courant number and form the flux back out of it, so every intermediate is
    /// exact in `f32` and the assertions are about the fold rather than about
    /// rounding. The eco regime's `100 um` is not a power of two and would put a
    /// tolerance where an equality belongs.
    const DX_FINE: f64 = 1.0 / 1024.0;
    const DT: f64 = 1.0;

    fn fold_params(lod: u32) -> CoarseCourantParams {
        CoarseCourantParams {
            nx: CNX << lod,
            ny: CNY << lod,
            nz: CNZ << lod,
            lod,
            // `2^(-3*lod)`, built by halving so that the exponent is written once
            // and the value is exact in both modes.
            fold_gain: Q::from_f64(0.5f64.powi(3 * lod as i32)),
        }
    }

    fn n_fine_cells(p: &CoarseCourantParams) -> u32 {
        p.nx * p.ny * p.nz
    }

    /// A zeroed fine Courant buffer of the layout `face_courant_voxel` writes:
    /// three lanes of `n_fine + 1`, the last cell of each being the ghost of
    /// ADR-059.
    fn fine_buffer(p: &CoarseCourantParams) -> Vec<Q> {
        vec![Q::ZERO; (3 * (n_fine_cells(p) + 1)) as usize]
    }

    fn coarse_buffer(p: &CoarseCourantParams) -> Vec<Q> {
        vec![Q::ZERO; (3 * (n_coarse_cells(p) + 1)) as usize]
    }

    fn fine_at(p: &CoarseCourantParams, axis: u32, x: u32, y: u32, z: u32) -> usize {
        (axis * (n_fine_cells(p) + 1) + fine_index(p, x, y, z)) as usize
    }

    fn coarse_at(p: &CoarseCourantParams, axis: u32, coarse: u32) -> usize {
        (axis * (n_coarse_cells(p) + 1) + coarse) as usize
    }

    /// The linear index of a coarse cell, by the **coarse** extents.
    fn coarse_index(cx: u32, cy: u32, cz: u32) -> u32 {
        cx + cy * CNX + cz * CNX * CNY
    }

    /// Dispatch the fold over the whole coarse grid.
    fn fold(fine: &[Q], p: &CoarseCourantParams) -> Vec<Q> {
        let mut dst = coarse_buffer(p);
        for coarse in 0..n_coarse_cells(p) {
            coarse_face_courant_voxel(fine, &mut dst, p, coarse);
        }
        dst
    }

    #[test]
    fn the_coarse_face_courant_is_the_fine_flux_over_the_coarse_area() {
        // The divisor, and it is computed here from the *definition of the flux*
        // rather than from `2^(-3*lod)`: a velocity is recovered out of every
        // stored Courant number, multiplied by the area of a fine face, summed,
        // divided by the area of the coarse face and turned back into a Courant
        // number at the coarse step. If the kernel divides by `2^(2*lod)` — the
        // arithmetic mean of the Courant numbers — this comes out a factor of
        // `2^lod` too large; if it does not divide at all, `2^(3*lod)`.
        //
        // Both `lod = 1` and `lod = 2`, so that a divisor hard-coded to four or to
        // sixty-four fails on one of them.
        for lod in [1u32, 2] {
            let p = fold_params(lod);
            let mut fine = fine_buffer(&p);
            // Pairwise distinct dyadic values: the sum of any sixteen of them has
            // an integer numerator under 2^24 and is exact in `f32`.
            for (at, value) in fine.iter_mut().enumerate() {
                *value = Q::from_f64((at as f64 + 1.0) / 4096.0);
            }

            let dst = fold(&fine, &p);
            let span = 1u32 << lod;
            let area_fine = DX_FINE * DX_FINE;
            let dx_coarse = DX_FINE * f64::from(span);
            let area_coarse = dx_coarse * dx_coarse;

            for coarse in 0..n_coarse_cells(&p) {
                let (cx, cy, cz) = coarse_coords(&p, coarse);
                for axis in 0..3u32 {
                    let mut flux = 0.0f64;
                    for b in 0..span {
                        for a in 0..span {
                            let (x, y, z) = match axis {
                                0 => ((cx << lod), (cy << lod) + a, (cz << lod) + b),
                                1 => ((cx << lod) + a, cy << lod, (cz << lod) + b),
                                _ => ((cx << lod) + a, (cy << lod) + b, cz << lod),
                            };
                            let courant = fine[fine_at(&p, axis, x, y, z)].debug_f64();
                            // `C = u*dt/dx_fine`, so `u = C*dx_fine/dt`.
                            let u = courant * DX_FINE / DT;
                            flux += u * area_fine;
                        }
                    }
                    let u_bar = flux / area_coarse;
                    let expected = u_bar * DT / dx_coarse;

                    assert_eq!(
                        dst[coarse_at(&p, axis, coarse)],
                        Q::from_f64(expected),
                        "lod {lod}, coarse cell {coarse}, axis {axis}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_uniform_flow_has_the_same_speed_on_the_fine_and_the_coarse_faces() {
        // The one miss both inequalities of SPEC section 4.2 are blind to. Under a
        // constant `u` every fine face carries `C_fine = u*dt/dx_fine` and the
        // coarse face has to carry `C_fine/2^lod`, which is the *same physical
        // speed* at the coarse step. The arithmetic mean of the Courant numbers
        // gives `C_fine` here — a flow `2^lod` times faster, with both residuals
        // still exactly zero and the temperature entirely plausible.
        for lod in [1u32, 2] {
            let p = fold_params(lod);
            let c_fine = Q::from_f64(0.125);
            let mut fine = fine_buffer(&p);
            for value in fine.iter_mut() {
                *value = c_fine;
            }

            let dst = fold(&fine, &p);
            let expected = Q::from_f64(0.125 / f64::from(1u32 << lod));
            assert_ne!(
                expected, c_fine,
                "the fixture cannot tell the fold of the flux from the mean of the \
                 Courant numbers"
            );
            for coarse in 0..n_coarse_cells(&p) {
                for axis in 0..3u32 {
                    assert_eq!(
                        dst[coarse_at(&p, axis, coarse)],
                        expected,
                        "lod {lod}, coarse cell {coarse}, axis {axis}"
                    );
                }
            }
        }
    }

    #[test]
    fn shear_across_a_coarse_face_folds_below_either_fine_face() {
        // The maximum of `|C_i|` and the sum of the magnitudes, both refused by
        // ADR-087 and both plausible to whoever is thinking about stability. A
        // maximum over magnitudes has no sign at all, and under a shear it would
        // carry heat in a direction the flux does not have.
        let lod = 2u32;
        let p = fold_params(lod);
        let span = 1u32 << lod;
        let c = 0.0625f64;

        // Balanced: half the covering faces one way, half the other. The flux
        // through the coarse face is zero and so is its Courant number, while
        // `max|C_i|` is `c` and `sum|C_i|` is sixteen times it.
        let mut fine = fine_buffer(&p);
        for coarse in 0..n_coarse_cells(&p) {
            let (cx, cy, cz) = coarse_coords(&p, coarse);
            for b in 0..span {
                for a in 0..span {
                    let sign = if a < span / 2 { 1.0 } else { -1.0 };
                    let at = fine_at(&p, 0, cx << lod, (cy << lod) + a, (cz << lod) + b);
                    fine[at] = Q::from_f64(sign * c);
                }
            }
        }
        for (coarse, value) in fold(&fine, &p).iter().enumerate().take(N_COARSE as usize) {
            assert_eq!(
                *value,
                Q::ZERO,
                "coarse cell {coarse} under a balanced shear"
            );
        }

        // Asymmetric: twelve one way, four the other. The folded number is
        // strictly smaller in magnitude than the largest covering face and than
        // the sum of the magnitudes.
        let mut fine = fine_buffer(&p);
        for coarse in 0..n_coarse_cells(&p) {
            let (cx, cy, cz) = coarse_coords(&p, coarse);
            for b in 0..span {
                for a in 0..span {
                    let sign = if a < 3 { 1.0 } else { -1.0 };
                    let at = fine_at(&p, 0, cx << lod, (cy << lod) + a, (cz << lod) + b);
                    fine[at] = Q::from_f64(sign * c);
                }
            }
        }
        let dst = fold(&fine, &p);
        for coarse in 0..n_coarse_cells(&p) {
            let folded = dst[coarse_at(&p, 0, coarse)].debug_f64();
            assert!(folded > 0.0, "coarse cell {coarse}: the net flux is upward");
            assert!(
                folded < c,
                "coarse cell {coarse}: the fold gave {folded} against a largest \
                 covering face of {c} — that is the maximum of the magnitudes"
            );
            assert!(
                folded < 16.0 * c,
                "coarse cell {coarse}: the fold gave the sum of the magnitudes"
            );
        }
    }

    #[test]
    fn the_covering_faces_are_the_per_axis_shift() {
        // The trap `kernels/fold.rs` holds in its header, here on faces. The
        // linear form `fine = (coarse << (3*lod)) + f` partitions the index space
        // just as disjointly and passes every conservation test; what it credits
        // is a stripe of sixteen along X instead of the 4x4 square lying on the
        // coarse face. Only this test separates them, because `world::Grid` knows
        // nothing about `lod` and there is no authority in `world/` to check
        // against.
        let lod = 2u32;
        let p = fold_params(lod);
        let span = 1u32 << lod;
        // An interior coarse cell, so that a stripe running off the marked square
        // has somewhere to land.
        let marked = coarse_index(1, 1, 2);
        let (cx, cy, cz) = coarse_coords(&p, marked);

        for axis in 0..3u32 {
            let mut fine = fine_buffer(&p);
            for b in 0..span {
                for a in 0..span {
                    let (x, y, z) = match axis {
                        0 => (cx << lod, (cy << lod) + a, (cz << lod) + b),
                        1 => ((cx << lod) + a, cy << lod, (cz << lod) + b),
                        _ => ((cx << lod) + a, (cy << lod) + b, cz << lod),
                    };
                    fine[fine_at(&p, axis, x, y, z)] = Q::ONE;
                }
            }

            let dst = fold(&fine, &p);
            let expected = Q::from_f64(16.0 * 0.5f64.powi(3 * lod as i32));
            for coarse in 0..n_coarse_cells(&p) {
                let want = if coarse == marked { expected } else { Q::ZERO };
                assert_eq!(
                    dst[coarse_at(&p, axis, coarse)],
                    want,
                    "axis {axis}: coarse cell {coarse} against the marked cell \
                     {marked}"
                );
                // And nothing at all reached the other two axes.
                for other in 0..3u32 {
                    if other != axis {
                        assert_eq!(
                            dst[coarse_at(&p, other, coarse)],
                            Q::ZERO,
                            "axis {axis} leaked into axis {other} at coarse cell \
                             {coarse}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_courant_fold_writes_only_its_own_coarse_faces() {
        // ADR-034 in its operational form: three cells per invocation and not one
        // more, the coarse ghost cells left alone, and a result independent of the
        // order the coarse grid is walked in.
        let p = fold_params(2);
        let mut fine = fine_buffer(&p);
        for (at, value) in fine.iter_mut().enumerate() {
            *value = Q::from_f64((at as f64 + 1.0) / 4096.0);
        }

        let poison = Q::from_f64(-12345.0);
        let coarse = coarse_index(1, 2, 1);
        let mut dst = vec![poison; (3 * (n_coarse_cells(&p) + 1)) as usize];
        coarse_face_courant_voxel(&fine, &mut dst, &p, coarse);

        let touched: Vec<usize> = dst
            .iter()
            .enumerate()
            .filter(|(_, value)| **value != poison)
            .map(|(at, _)| at)
            .collect();
        assert_eq!(
            touched,
            vec![
                coarse_at(&p, 0, coarse),
                coarse_at(&p, 1, coarse),
                coarse_at(&p, 2, coarse),
            ],
            "the fold wrote cells other than its own three coarse faces"
        );
        // The ghost of each lane is the face of the *domain* (ADR-059), and the
        // fine kernel does not write its own either: `TODO(exchange-courant)` is
        // one hole and not two.
        for axis in 0..3u32 {
            assert_eq!(
                dst[coarse_at(&p, axis, n_coarse_cells(&p))],
                poison,
                "the fold wrote the coarse ghost cell of axis {axis}"
            );
        }

        // And the dispatch is independent of the traversal order, bit for bit.
        let mut forwards = coarse_buffer(&p);
        for idx in 0..n_coarse_cells(&p) {
            coarse_face_courant_voxel(&fine, &mut forwards, &p, idx);
        }
        let mut backwards = coarse_buffer(&p);
        for idx in (0..n_coarse_cells(&p)).rev() {
            coarse_face_courant_voxel(&fine, &mut backwards, &p, idx);
        }
        assert_eq!(forwards, backwards);
    }

    #[test]
    fn the_coarse_face_divergence_is_the_sum_of_the_fine_ones() {
        // The identity a point sample onto the coarse face destroys, and the
        // reason it matters: a conservative scheme under a divergent field does
        // not lose matter, it **compresses** — heat piles up in an imaginary
        // convergence zone with both residuals at exactly zero.
        //
        // Stated in flux and not in Courant numbers, because that is where the
        // cancellation lives: the interior fine faces of a coarse block enter the
        // sum of the fine divergences twice with opposite signs (antisymmetry,
        // ADR-034), leaving exactly the faces the six coarse ones tile.
        let lod = 2u32;
        let p = fold_params(lod);
        let span = 1u32 << lod;
        let mut fine = fine_buffer(&p);
        for (at, value) in fine.iter_mut().enumerate() {
            // Signed and dyadic, so that the divergence is not zero by symmetry.
            let sign = if at % 3 == 0 { -1.0 } else { 1.0 };
            *value = Q::from_f64(sign * ((at % 37) as f64 + 1.0) / 1024.0);
        }
        let dst = fold(&fine, &p);

        let dx_coarse = DX_FINE * f64::from(span);
        // The flux through a face of area `A` at Courant number `C`:
        // `u*A = (C*dx/dt)*A`.
        let fine_flux = |c: f64| c * DX_FINE / DT * DX_FINE * DX_FINE;
        let coarse_flux = |c: f64| c * dx_coarse / DT * dx_coarse * dx_coarse;

        // An interior coarse cell on every axis, so that both its upper coarse
        // face and the upper fine faces of its block exist inside the domain.
        let coarse = coarse_index(0, 1, 1);
        let (cx, cy, cz) = coarse_coords(&p, coarse);
        assert!(cx + 1 < CNX && cy + 1 < CNY && cz + 1 < CNZ);

        let mut from_coarse = 0.0f64;
        for axis in 0..3u32 {
            let up = match axis {
                0 => coarse + 1,
                1 => coarse + CNX,
                _ => coarse + CNX * CNY,
            };
            from_coarse += coarse_flux(dst[coarse_at(&p, axis, up)].debug_f64());
            from_coarse -= coarse_flux(dst[coarse_at(&p, axis, coarse)].debug_f64());
        }

        let mut from_fine = 0.0f64;
        for dz in 0..span {
            for dy in 0..span {
                for dx in 0..span {
                    let (x, y, z) = ((cx << lod) + dx, (cy << lod) + dy, (cz << lod) + dz);
                    for axis in 0..3u32 {
                        let (ux, uy, uz) = match axis {
                            0 => (x + 1, y, z),
                            1 => (x, y + 1, z),
                            _ => (x, y, z + 1),
                        };
                        from_fine += fine_flux(fine[fine_at(&p, axis, ux, uy, uz)].debug_f64());
                        from_fine -= fine_flux(fine[fine_at(&p, axis, x, y, z)].debug_f64());
                    }
                }
            }
        }

        // A relative tolerance and not an equality, for the reason the module
        // header already gives about `div(curl A)`: the identity is exact in the
        // *coefficients*, and both sides here are assembled out of numbers `Q`
        // has already rounded.
        let scale = from_fine.abs().max(from_coarse.abs());
        assert!(scale > 0.0, "the fixture has no divergence to compare");
        assert!(
            (from_coarse - from_fine).abs() <= 1e-5 * scale,
            "the divergence of the coarse cell is {from_coarse} against \
             {from_fine} summed over its {} fine cells",
            span * span * span
        );
    }

    #[test]
    fn the_fine_indexing_of_the_fold_agrees_with_the_grid() {
        // The file's own copy of SPEC section 1.1, held to the authority that
        // owns it. Every fixture above places its inputs through this same
        // function, so a transposed copy would be self-consistent inside the
        // module and invisible to all of them.
        let p = fold_params(2);
        let grid = Grid::new(p.nx, p.ny, p.nz, [Boundary::Periodic; 6]).unwrap();
        for z in 0..p.nz {
            for y in 0..p.ny {
                for x in 0..p.nx {
                    assert_eq!(fine_index(&p, x, y, z), grid.index(x, y, z));
                }
            }
        }
    }

    proptest! {
        /// The stability of the coarse step, asserted directly instead of by
        /// argument (ADR-087).
        ///
        /// `config/validate.rs::speed_bounds` declares `outgoing_faces: 6`, so a
        /// scenario that loads has `|u| <= dx/(6*dt)` and every fine face carries
        /// `|C| <= 1/6`. The fold then owes `|C_coarse| <= (1/6)/2^lod = 1/24` and
        /// `<= 1/12` over the two outgoing faces of an axis — twelve times inside
        /// the axis-split condition of ADR-036, four times inside the conservative
        /// six-face reading. This is the property that makes ADR-087's refusal of
        /// a third `SpeedBound` sound, and a broken fold breaks it before any
        /// validator sees anything.
        #[test]
        fn a_fine_field_within_the_courant_bound_stays_within_it_after_the_fold(
            draws in proptest::collection::vec(-1.0f64..=1.0, 3 * (8 * 12 * 16 + 1)),
        ) {
            const CEILING: f64 = 1.0 / 6.0;
            let p = fold_params(2);
            let mut fine = fine_buffer(&p);
            prop_assert_eq!(fine.len(), draws.len());
            for (cell, draw) in fine.iter_mut().zip(&draws) {
                // At the ceiling and just under it: the draw is a fraction of the
                // validator's own bound.
                *cell = Q::from_f64(draw * CEILING);
            }

            let dst = fold(&fine, &p);
            // One part in a million of slack, and it is `f32` and not physics: the
            // fold is fifteen `qadd` over already-rounded numbers.
            let per_face = CEILING / 4.0 * (1.0 + 1e-6);
            let per_axis = 2.0 * CEILING / 4.0 * (1.0 + 1e-6);
            for coarse in 0..n_coarse_cells(&p) {
                let (cx, cy, cz) = coarse_coords(&p, coarse);
                for axis in 0..3u32 {
                    let lower = dst[coarse_at(&p, axis, coarse)].debug_f64();
                    prop_assert!(
                        lower.abs() <= per_face,
                        "coarse cell {} axis {} carries {} over {}",
                        coarse, axis, lower, per_face
                    );

                    // The upper face of the cell is the lower face of the cell
                    // above it, and the sum over the two outgoing faces of the
                    // axis is what `Advect::fold_courant` compares with one.
                    let (up, exists) = match axis {
                        0 => (coarse + 1, cx + 1 < CNX),
                        1 => (coarse + CNX, cy + 1 < CNY),
                        _ => (coarse + CNX * CNY, cz + 1 < CNZ),
                    };
                    let mut outgoing = 0.0f64;
                    if lower < 0.0 {
                        outgoing -= lower;
                    }
                    if exists {
                        let upper = dst[coarse_at(&p, axis, up)].debug_f64();
                        if upper > 0.0 {
                            outgoing += upper;
                        }
                    }
                    prop_assert!(
                        outgoing <= per_axis,
                        "coarse cell {} gives away {} along axis {} against {}",
                        coarse, outgoing, axis, per_axis
                    );
                }
            }
        }
    }
}
