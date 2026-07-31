//! World semantics version (ADR-020).
//!
//! Bumped on any change that affects dynamics: a constant in a reaction, the
//! order of operations in a kernel, a formula. Refactoring that preserves
//! behaviour changes the code version and leaves this number alone.
//!
//! This constant is the only place the number lives. CI fails a pull request
//! that touches `configs/**` or `crates/liminis-core/src/kernels/**` without
//! changing it.

/// Version of the world semantics. A run is identified by
/// `(seed, config_hash, world_format_version)`.
pub const WORLD_FORMAT_VERSION: u32 = 1;
