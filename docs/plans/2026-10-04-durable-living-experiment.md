# Durable Living Experiment: 2026-10-04

## Goal

Preserve and resume an actual local world, together with its long-term scalar
history. This stage changes observation and reliability, not biological rules.
The existing culture on 8081 has no full-state export route and must remain
untouched; the new durable experiment will run on 8082.

## Decisions

ADR-096 chooses host-owned immutable checkpoint generations around the existing
binary codec, bounded off-thread disk writing, measured in-memory capture,
explicit restore identity, and visible failures. Persistence defaults to the
gitignored `.liminis/runs` directory, never build output in `target`.

After measuring the copy size, ADR-098 bounds full checkpoints to the latest
three published generations per run. Cleanup occurs only after successful
publication, outside the simulation lock. Metrics and session lineage remain.

ADR-097 chooses exact NDJSON metrics in per-session append-only segments. Resume
links a new segment to the checkpoint's parent session/tick, preserving old
future observations without mixing them into the active history. The browser
loads a bounded overview of actual samples, not invented intermediate values.

## Ownership

- Root: persistence module, serve/main integration, launcher, operational docs,
  integration tests, runtime migration and browser QA.
- Astra: architecture/product decisions, append-only ADRs, acceptance plan,
  full-state deterministic replay tests and review.
- Viewer agent: only viewer.html; Save now/status and durable chart history.

## Acceptance

1. A paused or running checkpoint restores seed/config/world identity, tick,
   observer first_seen, run settings, both buffers and all channel counters.
2. After restore and further ticks, complete snapshot bytes equal an uninterrupted
   reference for seed 42 and the maximum u64 seed. Both ledgers are checked on
   every replayed tick in release as well as debug.
3. Corrupt, truncated, wrong-identity and trailing-byte checkpoint inputs are
   rejected before publishing a replacement world. A previous finished checkpoint
   is not overwritten by a failed save.
4. In-flight capture count is bounded. A persisted status appears only after
   writer acknowledgement and refers to the captured tick. Disk failure becomes
   visible and pauses progression.
5. Two processes cannot write one run namespace. Reset creates a new run and
   does not delete the old one. Resume explicitly loads saved config/seed.
6. Every NDJSON segment has its own exact header. A torn last line is tolerated;
   complete malformed records are not silently discarded. No synthetic residual
   zero is recorded at genesis or immediately after restore.
7. Resume history follows parent session boundaries and never merges an old
   future into the active branch. Browser refresh recovers the available whole
   horizon with at most 360 real samples, including first and last.
8. Manual Save, autosave, paused saving, launcher restart, preserved running/paused
   state, and exact maximum-u64 seed work through the real local server path.
9. Observer layouts at 420/590/1440 remain readable; saved/saving/error status is
   honest, and historical loading cannot change the current specimen or genotype
   first-detection facts.
10. The old 8081 culture survives unchanged. The new durable run on 8082 can be
    stopped after a confirmed save and genuinely resumed, not regenerated.

## Boundaries

No biological model change, cell objects, hot upgrade, browser checkpoint
upload, or in-process restore UI. Only the last three full checkpoints per run
are retained; manual saves are not pinned. History and old run namespaces are
not automatically deleted, so total disk use still grows. OS file sync
is not a guarantee against every hardware/power-loss failure. No push/deploy.

## Verification Status

`cargo test --release -p liminis-core --test acceptance_genetic_colony full_checkpoint -- --nocapture`
passed both full-state checkpoint tests (1.27 seconds). Resume after tick 137
and continuation for 89 ticks reproduced the entire uninterrupted snapshot for
seed 42 and `u64::MAX`, including both field buffers and exact channel counters.
Every replayed tick explicitly checked both ledgers in release mode.

One measured 24-cubed capture produced 1,110,407 bytes in 1,543 microseconds.
This is an observed in-memory encoding cost, not a guaranteed pause budget or a
measurement of disk sync. Keeping every minute's checkpoint would retain roughly
1.6 GB of binary state per day; this measurement motivated ADR-098's three-file
retention, roughly 3.3 MB of binary state per current 24-cubed run, plus the
temporary new publication, metadata and unbounded scalar history.

`cargo test --release -p liminis serve::persistence::tests -- --nocapture`
passed the initial 8 host tests in 0.32 seconds. After adding retention and
host-level observer/continuation checks,
`cargo test -p liminis serve::persistence::tests -- --nocapture`
passed all 10 in debug mode in 4.60 seconds. A concurrent 10-test release attempt
reached linking but was blocked by the separate running release test executable
on Windows. The final main-suite rerun subsequently passed all 10 (see below).

Covered failures and guarantees:

- Corrupted/truncated/checksummed-trailing-byte/wrong-identity state is refused.
- An OS lease refuses a second writer and is released after writer shutdown.
- Incomplete initial run publication does not hide the previous saved run;
  a corrupt committed manifest is not silently skipped.
- An explicit checkpoint identifier cannot redirect to another generation.
- Every complete metric has the full exact integer roster; i128 values above
  f64 precision remain intact, while fractional counted values are rejected.
- Resume at tick 60 keeps parent samples 30/60, excludes the old tick-90 future,
  and appends the resumed tick 61 in a new segment.
- An interrupted final line is tolerated; a completed malformed line is refused.
- Failed manifest publication leaves the previous checkpoint and its ack intact.
- Six saves retain only the last three committed generations without deleting
  non-checkpoint directories, unfinished generations, or the NDJSON records.
- Genetic host resume preserves both running/paused modes, target TPS 137,
  exact maximum-u64 seed and genotype first-seen values; 15 further ticks after
  saving tick 75 match the uninterrupted full binary state in both cases.

The focused review findings were fixed and covered above. The viewer's history
axis now uses actual tick spacing rather than sample index, and unavailable
history is visible instead of a silent console-only failure. Code comments for
the formerly open restart/mean/timing and off-thread snapshot questions point
to ADR-096/097; no core behavior changed.

Root owns final release gates, actual process stop/resume and browser evidence
below. No acceptance claim relies on reconstructing the old 8081 culture.

## Final Runtime And Observer Evidence

`cargo test --workspace --release` passed: 52 server tests including the
1,000-tick legacy run (222.11 seconds), 459 core unit tests with 3 pre-existing
ignores, all acceptance suites and 5 doctests. Genetic acceptance passed 6 tests
including both new checkpoint cases; the deliberate 10k soak ignore was not
rerun because this stage changes no biological dynamics. Its previous stage
evidence remains in the genetic-colony plan. After the last retention/path and
storage-failure guards and the bounded-history regression, the short release server suite passed all 54 tests
(only the already-passed legacy long run filtered out), including all 10 host
persistence tests. Full log: ignored `target/qa/durable-workspace-tests.log`.
The additional overview test fed 10,000 actual samples, checked the 360-point
bound after every insertion, preserved first/last and ordering, and verified
that a duplicate tick cannot inflate the history count. This is a chart-cache
test, not another biological 10k-tick simulation.
Release build, all-target workspace clippy with warnings denied, formatting,
bare-Q check (13 files), and whitespace checks passed.

The launcher was used through the actual executable path, not only fixtures:

- A real browser Save transitioned through pending and showed the worker's
  captured tick after acknowledgement, not the live tick at completion.
- Pausing at tick 1,457 committed `running=false`. Stopping that new server and
  launching `-Resume run-1791086311753-43292-0` restored the exact tick, seed,
  hash, ecology totals/shares and first-seen records. All 50 disk history points
  remained available. Residuals were honestly null until the first next step;
  that step advanced exactly once and both actual residuals were zero.
- Screenshots across this paused restart had 0 changed microscope pixels in
  the checked ROI; 4,741 bright microscope pixels proved it was not blank.
- Reset with exact seed `18446744073709551615` created a new namespace, kept
  the prior experiment and remained paused at tick 0 without synthetic history.
  A further seed-42 reset created the final run and resumed real growth.
- After rebuilding the final binary, the final seed-42 run was paused and
  committed at tick 6,991. `-Resume latest` restored that same world and all
  240 history points in a new parent-linked session. It was then resumed and
  left advancing. Autosaves continue with only 3 complete binary generations.

Browser QA passed 420x900, 590x900 and 1440x1000: no horizontal overflow or
clipped save controls. Biomass/resource modes and histories used real samples;
refresh restored the disk horizon rather than starting an empty chart. Final
browser warning/error logs were empty. Screenshots under ignored `target/qa`:
`durable-before-restart.jpg`, `durable-after-restart.jpg`, `durable-colony.jpg`.

Final process is detached `target/local-server-8082.exe`, PID 15036,
`http://127.0.0.1:8082/`, run `run-1791086684126-14912-0`, seed 42,
hash `blake3:fca3fa6d14223d59`, world format 28. A final health sample at tick
8,624 reported running/alive, no simulation/storage error and zero matter and
energy residuals. The 8081 culture remained running at tick 88,683; 8080
remained paused at tick 430,975. Neither was reset, killed or claimed migrated.
Source, docs and tests remain in the isolated local worktree. No push/deploy.
