//! The two streams of ADR-037: metrics for analysis, snapshots for restart.
//!
//! They are two because they are different in every dimension that matters, and
//! folding them into one costs whichever half is sacrificed. A snapshot is the
//! full state — 449 MB at the grid SPEC section 13 names — written rarely,
//! binary, read by exactly one program. A metric record is kilobytes, written
//! often, text, read by whoever comes along six months later. ADR-037 rejected
//! the single-stream design by arithmetic: analysing a hundred runs would mean
//! reading tens of gigabytes for a hundred numbers, and next to that sits the
//! temptation to snapshot less often and lose the time resolution exactly where
//! it was needed.
//!
//! | | metrics (`metrics.rs`) | snapshot (`snapshot.rs`) |
//! |---|---|---|
//! | what | scalars of one tick | every buffer |
//! | when | every observed tick | rarely |
//! | form | append-only NDJSON | binary |
//! | size | kilobytes a record | hundreds of megabytes |
//! | torn tail | discarded, rest survives | the file is lost |
//!
//! # Why NDJSON, and what it costs
//!
//! Three properties, in ADR-037's order: a torn last tail is discarded without
//! losing the rest, the schema grows by adding keys with no format version, and
//! the file reads with any tool without a library. The first is the reason
//! [`records`] exists — a property nothing checks is a property nobody has.
//!
//! The third is partly false and the falseness is worth writing down here rather
//! than being discovered later: Python's `json` keeps arbitrary integers, and
//! JavaScript's `JSON.parse` truncates past `2^53`. The domain sum of water at
//! 256 cubed is `8.59e18`, five orders past that, so the viewer of ADR-016 and
//! ADR-021 reads a number this crate wrote exactly and gets something else. The
//! writer cannot fix it and must not pretend about it.
//!
//! # The residual is written even when it is zero
//!
//! ADR-037: "a written zero is proof that the check ran; a missing line is
//! indistinguishable from a disabled check". Every optimisation that deletes it
//! — `if residual != 0`, a skip-zero list, a "compact" mode — leaves valid JSON
//! and every other column meaning what it meant, and nothing in the ledger
//! fails, because `assert_closed` lives elsewhere and only in a debug build.
//! Hence `the_residual_is_written_even_when_it_is_zero`, which feeds the writer
//! the closed S0 domain where all of those look harmless.
//!
//! The same requirement has a second door, and it is sharper. ADR-059 puts the
//! *reduction* behind `cfg(debug_assertions)`, and `Tick::advance` honours that:
//! `Scratch::before` says in as many words that "in a release build these are
//! whatever they were initialised to", which is zero. A writer that called
//! `ledger.residual_matter(s, scratch.before(), scratch.after())` in release
//! would get `0 - 0 - credited`, which is exactly zero on a closed domain and
//! silently wrong the moment a channel fires — the column ADR-037 demands *as
//! proof the check ran* would be a fabricated zero produced by a check that did
//! not run, in the profile every real run uses.
//!
//! This module cannot decide that, and it does the one thing available to it:
//! [`TickMetrics`] takes the residuals as values, so the caller has to have
//! obtained them from somewhere, and nothing here computes a residual, a domain
//! sum or a counter.
// TODO(residual-in-release): ADR-059 puts the reduction in the debug build and
// ADR-037 requires the residual in the stream unconditionally; nothing in the
// corpus reconciles them. The three candidate answers — run the reduction in
// release on observed ticks only, refuse to write a tick whose sums were never
// filled, drop the column in release — differ in exactly the way ADR-037
// forbids, and choosing one is a new entry in `DECISIONS.md` rather than a
// decision for this file.
//!
//! # What does not go into the metric stream
//!
//! Vector observables — vertical profiles, histograms — "would inflate it by
//! orders of magnitude. A separate stream with its own frequency" (ADR-037). The
//! boundary is not "no arrays": a per-substance array of at most thirty-one
//! entries is already in the required minimum. It is **per substance and per
//! channel yes, per voxel and per layer no**. The first `"o2_by_depth": [...]`
//! produces a stream that still parses and a file a hundred times its budget,
//! and there is no mechanical guard against it.
//!
//! Guild biomass is in ADR-037's required minimum and is not a column here.
//! `ledger/mod.rs` carries `TODO(bt-invariant)`: `BT[g][t]` belongs to neither
//! residual and its home is undecided, and there are no guilds in S0. A column
//! emitted now would fix a name for a quantity whose accounting is open, and
//! the roster only ever grows.
//!
//! # The roster is versioned separately from the world
//!
//! ADR-037: "the list of metrics only grows. Removing a column devalues every
//! past run, adding one does not; so the list is versioned separately and does
//! not affect the semantics of the world." Hence
//! [`METRICS_SCHEMA_VERSION`] here and `WORLD_FORMAT_VERSION` untouched by this
//! module — bumping the latter would declare every run before this commit
//! incomparable with every run after it, on evidence that nothing about the
//! dynamics changed. CI only checks that the number moved when a guarded path
//! did, never that it stayed put when none did.

pub mod json;
pub mod metrics;
pub mod snapshot;

pub use json::Record;
pub use metrics::{
    FieldStat, MetricsWriter, Records, RunHeader, TIMING_COLUMNS, TickMetrics,
    channel_energy_totals, channel_matter_totals, channel_slot, columns, records,
};
pub use snapshot::{
    SNAPSHOT_FORMAT_VERSION, SNAPSHOT_MAGIC, SnapshotIdentity, snapshot_read_into, snapshot_write,
};

/// The version of the metric list, and of nothing else.
///
/// ADR-037 requires the roster to be versioned apart from the world's semantics,
/// because the two move for different reasons and at different rates: adding a
/// column changes what a future run records and leaves every past run as
/// comparable as it was, while `WORLD_FORMAT_VERSION` moving means no run before
/// it can go on a plot beside a run after it.
///
/// **It grows when a column is added, and a column is never removed.** Removing
/// one devalues every run ever recorded; keeping a column that turned out
/// useless costs eight bytes a tick.
///
/// Version 1 is the first stream there is: the required minimum of ADR-037 less
/// guild biomass, which has no accounting yet (see the module header).
pub const METRICS_SCHEMA_VERSION: u32 = 1;
