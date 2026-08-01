//! The world: where a voxel is, who its neighbours are, and where its amounts
//! live.
//!
//! This is the orchestration side of the one boundary that matters
//! (`ARCHITECTURE.md`). Kernels are free functions over flat slices and a linear
//! index; everything about *which* slices, how long they are and what the index
//! means lives here, and none of it ever travels into WGSL.
//!
//! Two types, and they answer two questions.
//!
//! [`Grid`] answers "where". One linear index, `idx = x + y*NX + z*NX*NY`, six
//! faces, one boundary condition per face (SPEC section 1.1, section 1.6). It
//! owns no memory and holds no cells.
//!
//! [`Field`] answers "where do the numbers live". A flat, substance-major,
//! double buffer over a grid (ADR-041, ADR-034).
//!
//! # What the pair is for
//!
//! A substep of any transport process is the same three lines:
//!
//! ```
//! use liminis_core::numeric::M32;
//! use liminis_core::world::{Boundary, Face, Field, Field32, Grid};
//!
//! let grid = Grid::new(8, 8, 8, [Boundary::Periodic; 6])?;
//! let mut field: Field32 = Field::new(&grid, 1)?;
//!
//! // Gather: every voxel reads its neighbours in state N and writes its own
//! // cell of state N+1. No voxel writes anywhere but its own index, so the
//! // result cannot depend on the order the loop runs in (ADR-034).
//! let (src, dst) = field.lane_pair_mut(0);
//! for idx in 0..grid.n_voxels() {
//!     let mut sum = M32::ZERO;
//!     for face in Face::ALL {
//!         sum += src[grid.neighbour(idx, face) as usize];
//!     }
//!     dst[idx as usize] = sum;
//! }
//! field.swap();
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! The real thing differs in one place: the body of the loop is a call to a
//! kernel, which computes the neighbour itself from its own `Params` rather than
//! from a `Grid`, because `kernels/` may depend on `numeric/` and on nothing
//! else. See the note on [`Grid`] about what that costs and who pays it.
//!
//! # What is not here
//!
//! No `World` aggregate. The skeleton in `ARCHITECTURE.md` writes
//! `world.pair_mut(s)` and `world.swap(s)`, indexed by substance — and a
//! substance index only becomes a buffer address once the registry says which
//! width and which lane it got, which is derived at load time (ADR-039,
//! ADR-040) by a loader that does not exist yet. The open end is written up in
//! the `TODO(width-lanes)` on [`Field`].
//!
//! No LOD. SPEC section 1.5 makes a coarse field a coarsening of the same grid
//! by a shift per axis, so a coarse field is another [`Grid`] and another
//! [`Field`] rather than a new mechanism — but the fold kernel between them
//! (ADR-045) belongs to `kernels/`, and nothing in the first kernel needs it.
//!
//! No ledger, no metrics, no snapshots. Those are their own modules.

mod field;
mod grid;

pub use field::{Field, Field32, Field64};
pub use grid::{Axis, Boundary, Face, Grid};
