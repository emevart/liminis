//! The world: where a voxel is, who its neighbours are, and where its amounts
//! live.
//!
//! This is the orchestration side of the one boundary that matters
//! (`ARCHITECTURE.md`). Kernels are free functions over flat slices and a linear
//! index; everything about *which* slices, how long they are and what the index
//! means lives here, and none of it ever travels into WGSL.
//!
//! Four types, and they answer four questions.
//!
//! [`Grid`] answers "where". One linear index, `idx = x + y*NX + z*NX*NY`, six
//! faces, one boundary condition per face (SPEC section 1.1, section 1.6). It
//! owns no memory and holds no cells.
//!
//! [`Field`] answers "where do the numbers live". A flat, substance-major,
//! double buffer over a grid (ADR-041, ADR-034).
//!
//! [`Registry`] answers "whose numbers are these". A `Field` addresses lanes; a
//! reaction, a snapshot and a genome's input vector address substances, and
//! since storage is compact the two are not the same number (ADR-056).
//!
//! [`World`] answers "whose buffers survive a process boundary". It holds the
//! grid, the registry and both amount fields together, and it is the one place
//! that turns a substance index into a buffer address ([`World::lane_of`], and
//! only through it). The rule it exists to hold up is ADR-057's: on completion
//! of a process the front buffer holds state `N` for **every** lane, so the six
//! readers of a coherent snapshot — reactions, pressure, settling, phase
//! change, `ledger/`, `observe/` — never learn that lanes advance at different
//! speeds. See [`ParitySplit`] for what a process pays for that.
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
//! # The coarse grids are grids
//!
//! A field on a coarser LOD is not a new mechanism. SPEC section 1.5 coarsens by
//! a shift **per axis**, so the coarse enthalpy grid is another [`Grid`] with
//! `nx >> lod` voxels along X and the same six boundary conditions, and the
//! field on it is another [`Field`]. What `World` adds is the mapping from a
//! fine voxel to the coarse cell that covers it, named by **role** —
//! [`World::enthalpy_cell_of`] and [`World::velocity_cell_of`] — because the two
//! coarse grids differ and a temperature read from the wrong one is plausible,
//! neighbouring and in no invariant. The fold kernel between the grids
//! (ADR-045) stays in `kernels/` and keeps taking `lod`, `nx`, `ny`, `nz` as
//! plain numbers.
//!
//! # What is not here
//!
//! No loader. The widths, the scales and the substep counts a [`World`] is built
//! from are derived at load time (ADR-039, ADR-040, ADR-030) by `config/`, and
//! `World::new` takes them already derived rather than deriving a second set.
//!
//! No ledger, no metrics, no snapshots. Those are their own modules.

mod field;
mod grid;
mod registry;
// `world::world`, and the repetition is the point: the aggregate is the module's
// namesake, one file per type beside `grid.rs`, `field.rs` and `registry.rs`.
// The alternative clippy is asking for — flattening it into this file — would
// put nine hundred lines of the one type that ties the other three together in
// the module header, where every reader looking for the map has to scroll past
// it.
#[allow(clippy::module_inception)]
mod world;

pub use field::{Direction, Field, Field32, Field64, ParitySplit};
pub use grid::{Axis, Boundary, Face, Grid};
pub use registry::{MAX_SUBSTANCES, R_MAX, Registry, S_MAX, SubstanceDecl, SubstanceSlot, Width};
pub use world::{LaneRef, OwnedBuffers, OwnedBuffersMut, World, WorldLayout, coarse_grid};
