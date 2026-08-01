//! Kernels: free functions over flat slices, one voxel at a time.
//!
//! This is the half of `liminis-core` that goes to the GPU. Everything here is
//! written to be transcribed into WGSL **line by line**, from the first line,
//! not "cleaned up later" (ADR-015) — and the CPU version stays afterwards as
//! the permanent reference: a shader that disagrees with it in the integer
//! fields is a bug in the shader.
//!
//! # The one boundary
//!
//! A kernel takes slices, a `Copy` struct of scalars and a linear voxel index,
//! and returns nothing — what it has to say, it writes into the output slice
//! (ADR-034):
//!
//! ```text
//! fn kernel(src: &[..], dst: &mut [..], p: &Params, idx: u32)
//! ```
//!
//! Allowed inside: local scalars, fixed-size arrays, `for` over a range,
//! branches, calls to other functions of the same shape.
//!
//! Forbidden inside: `self`, traits, generics over behaviour, closures, `Vec`,
//! `Box`, `dyn`, `HashMap`, recursion, allocation of any kind, reading from the
//! buffer being written, and any dependence of the result on the order the
//! voxels are visited in.
//!
//! `kernels/` depends on `numeric/` and on nothing else. If a kernel needs
//! `config/`, a parameter was not folded on the host, and that is the error
//! (`ARCHITECTURE.md`). The consequence is visible in `diffuse.rs`: the kernel
//! carries its own copy of the neighbourhood lookup rather than borrowing
//! `world::Grid`, and a test keeps the two from drifting.
//!
//! Two mechanical guards watch this directory. The type system is the first:
//! `Q` exposes no representation, so bare arithmetic over it does not compile
//! (ADR-022). `scripts/check_bare_q.py`, run by the `kernel-lint` hook and by
//! CI, is the second, for the WGSL templates that have no type system at all.
//! Neither of them can see a broken flux function — only `flux_is_antisymmetric`
//! can (`ARCHITECTURE.md`).

mod diffuse;

pub use diffuse::{DiffuseParams, diffuse_voxel_32, diffuse_voxel_64, flux_32, flux_64};
