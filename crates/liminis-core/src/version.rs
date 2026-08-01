//! World semantics version (ADR-020).
//!
//! Bumped on any change that affects dynamics: a constant in a reaction, the
//! order of operations in a kernel, a formula. Refactoring that preserves
//! behaviour changes the code version and leaves this number alone.
//!
//! This constant is the only place the number lives. CI fails a pull request
//! that touches `configs/**`, `crates/liminis-core/src/kernels/**` or
//! `crates/liminis-core/src/process/**` without changing it — the order of
//! processes is semantics too (ADR-036), even when no kernel changed.

/// Version of the world semantics. A run is identified by
/// `(seed, config_hash, world_format_version)`.
///
/// Version 2 is the first one that simulates anything: diffusion of one
/// substance, in gather form, over the substeps ADR-030 derives from the
/// coefficient. Version 1 had no kernels at all, so no run of it is comparable
/// with a run of this one.
pub const WORLD_FORMAT_VERSION: u32 = 2;
