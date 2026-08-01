//! World semantics version (ADR-020).
//!
//! Bumped on any change that affects dynamics: a constant in a reaction, the
//! order of operations in a kernel, a formula. Refactoring that preserves
//! behaviour changes the code version and leaves this number alone.
//!
//! This constant is the only place the number lives. CI fails a pull request
//! that touches `configs/**`, `crates/liminis-core/src/kernels/**`,
//! `crates/liminis-core/src/process/**` or `crates/liminis-core/src/numeric/
//! rng.rs` without changing it — the order of processes is semantics too
//! (ADR-036) even when no kernel changed, and so is the mixer every draw comes
//! out of (ADR-058).

/// Version of the world semantics. A run is identified by
/// `(seed, config_hash, world_format_version)`.
///
/// Version 2 is the first one that simulates anything: diffusion of one
/// substance, in gather form, over the substeps ADR-030 derives from the
/// coefficient. Version 1 had no kernels at all, so no run of it is comparable
/// with a run of this one.
///
/// Version 3 is where the seed arrives. `rand` takes a fourth counter, the run
/// key derived from the seed on the host (ADR-058), and a fourth round of the
/// mixer folds it in. No formula and no kernel changed, and that is exactly why
/// the bump is easy to miss: what changed is every draw. Stochastic rounding of
/// extent is the only conversion from `Q` to an integer in the chemistry
/// (ADR-027), so a different draw is a different `xi` for every reaction of
/// every voxel of every tick. Nothing from version 2 can go on a plot beside
/// anything from version 3 — including runs of the same seed, because under
/// version 2 the seed reached nothing at all.
pub const WORLD_FORMAT_VERSION: u32 = 3;
