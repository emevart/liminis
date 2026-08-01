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
/// Version 4 is the scenario schema. The loader knew three keys — `name`, `dt`
/// and `[grid]` — and now knows the whole of `CONFIG_SCHEMA.md` sections 2 to 8:
/// substances, reactions, fields, processes, boundaries and the conserved
/// quantities. The bump is mechanically required, because `configs/**` moved
/// (ADR-020), and it is required on the merits too. `config_hash` is now taken
/// over an explicit projection (ADR-066), and the projection has eleven root
/// keys where the whole struct had three, so the canonical bytes of every
/// scenario in existence have changed. No run of version 3 is comparable with a
/// run of this one, and the reason is the identity rather than the dynamics:
/// nothing about a tick changed, because nothing yet reads the new keys.
///
/// Version 5 is a bound. `process/diffuse.rs` gained `N_MAX = 64` (ADR-061), so
/// there is now a boundary past which a field is refused instead of being
/// counted out: a scenario whose `D`, `dt` and `lod` ask for more than
/// sixty-four substeps used to load and quietly cost up to a gibibyte of traffic
/// per tick per field, and now does not load at all. The set of admissible
/// worlds shrank, which is what this number is for; every world that loaded
/// under version 4 *and* still loads runs bit for bit as it did.
///
/// Version 6 is the validator, and it is the same kind of bump as version 5, on
/// the same grounds and by the same precedent. `config/validate.rs` turns the
/// forty-odd refusals of `CONFIG_SCHEMA.md` section 10 plus referential
/// integrity, the domains of definition and grid divisibility from prose into
/// checks, where before them nothing at all was checked. The set of admissible
/// worlds shrank again; no world that loaded before *and* loads now changed by a
/// single bit, because a validator computes nothing a tick reads.
///
/// The caveat belongs in the comment rather than in a commit message: **CI does
/// not require this increment.** The guard of ADR-020 watches `configs/**`,
/// `crates/liminis-core/src/kernels/**`, `crates/liminis-core/src/process/**`
/// and `numeric/rng.rs`, and it does not watch `config/**` — `CONFIG_SCHEMA.md`
/// section 11 names that blind spot outright. So this number moved because the
/// author moved it, and the reason it should have moved is above.
pub const WORLD_FORMAT_VERSION: u32 = 6;
