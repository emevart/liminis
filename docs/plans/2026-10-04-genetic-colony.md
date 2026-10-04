# Genetic Colony: 2026-10-04

## Goal

A founder population develops a spatial colony, produces previously absent
two-locus genotypes through reproduction mutation, and supports a second
metabolic niche through detritus from biomass turnover.

The existing `living-world` run and scenario remain usable. This is a bounded
population-genetics experiment, not individual cellular development or
open-ended genome evolution.

## Decisions

ADR-093 compiles a two-bit genome into four ordinary biomass lanes and checked
reaction branches. Bit 0 trades growth speed for substrate affinity. Bit 1
allocates a fixed catalytic budget between fresh food and detritus. Only one
locus can mutate per birth, and branch weights sum to the parent's total rate.

ADR-094 adds explicit initial concentrations and physical spherical inocula.
The scenario starts with G00 in one sphere; other genotypes and DET start at
exactly zero. Diffusion and chemistry, not rendering, create subsequent spread.

## Ownership

- Genetics agent: config schema, canonical hashing, idempotent genome expansion,
  decoder metadata and load validation.
- Inoculation agent: world generation and focused initial-state acceptance.
- Astra: biological decisions, scenario, ADRs and causal acceptance experiments.
- Root: API/state integration, version bump, local startup and browser QA.
- Viewer agent: existing observer UI, real fields and genome inspection.

## Acceptance

1. Genetics compiles deterministically, rejects conflicting generated rows,
   preserves canonical round trips, and hashes all dynamic parameters.
2. Initial G01/G10/G11 amounts are exactly zero; G00 is confined to the declared
   sphere. Mutation creates neighbouring genotypes. With mutation disabled,
   unseeded genotypes stay exactly zero.
3. A single initial G00 cannot produce the two-bit-different G11 during the
   first reaction tick. It can emerge through subsequent single-bit births.
4. Decoded speed/affinity and substrate-allocation tradeoffs cause different
   resource-dependent growth. The shared resource budget cannot increase merely
   because a genotype can consume two substrates.
5. DET originates from biomass turnover in the shipped scenario. A DET-preferring
   population grows on provided producer detritus and cannot obtain the same
   growth from its weak FOOD pathway alone.
6. A real concentration contour expands beyond the inoculum under growth and
   diffusion. Threshold and comparison are stated in the acceptance test;
   nonzero diffusion tails are not counted as occupied colonies.
7. Replay from the same seed/config is exact; both integer ledgers close every
   checked tick and declared concentration ceilings hold.
8. A full 24-cubed 10,000-tick release run passes before switching the default.
   The established finite-five scenario retains its regression checks.
9. The observer displays actual two-bit codes, decoded traits, and first detected
   ticks. A genotype-neighbour graph never claims individual ancestry.
10. Food, detritus, oxygen and biomass views use real concentration data. Sparse
    founder samples stay visible even when the full-plane 95th percentile is zero.
    Pause, step, reset, speed, export and 420/590/1440 layouts remain verified.

## Boundaries

The model still has four possible genotypes and no individual cell table.
Genome mutation changes inherited metabolic allocation and kinetic strategy;
it cannot invent arbitrary pathways. There is no GRN, morphology, adhesion,
phylogenetic tree or claim of a major evolutionary transition. Coefficients
remain uncalibrated.

## Verification Status

`cargo test --release -p liminis-core config::genetics -- --nocapture`
passed 8 compiler tests after replacing position-based generated reaction IDs
with template/parent/offspring names. Coverage includes canonical reload,
parameter hashing, collision rejection, and stale mutation branches. A final
load-only guard also rejects non-compiler chemistry involving generated lanes;
`cargo test --release -p liminis-core --lib config::genetics -- --nocapture`
passed all 8 again with that regression included.

`cargo test --release -p liminis-core --test acceptance_genetic_colony -- --nocapture`
passed 4 causal tests (1 deliberate long-soak ignore) in 34.01 seconds on the
final name-based reaction RNG keys. Matter and energy integer ledgers were
asserted every tick, including release mode.

- Poor FOOD, 30 ticks: slow biomass change -0.00027719 mol/m3; fast
  -0.00040702 mol/m3. Rich FOOD: slow +0.00070465; fast +0.00306011.
- Producer turnover made 0.00810254 mol/m3 mean DET. Over 300 consumer ticks,
  initial biomass 0.01000003 became 0.01139347 with that detritus and
  0.00830442 without it. Disabling producer turnover left DET exactly zero.
- At 16 cubed after 2,000 ticks, the maximum radius of voxel centres above
  0.0003 mol/m3 biomass grew from 0.295804 to 0.876071 mm. This threshold is
  1% of founder concentration; diffusion tails alone do not define the front.
- Only G00 starts present; the other three emerge through mutation. With
  mutation disabled they stay exactly zero. G11 cannot appear on tick 1.
- Same-seed substance and enthalpy fields replay exactly.

`cargo test -p liminis-core --test acceptance_genetic_colony -- --skip the_colony_expands --nocapture`
also passed the 3 short causal experiments in debug mode (24.80 seconds), with
the same printed fitness and cross-feeding values. The 2,000-tick spatial
experiment was verified in release rather than repeated in debug.

The initializer's tangent-boundary regression and both inoculation acceptance
tests passed after validator and worldgen adopted the same closed-sphere
membership predicate.

`cargo test --release -p liminis-core --test acceptance_genetic_colony genetic_colony_10k -- --ignored --nocapture`
passed the full 24-cubed 10,000-tick seed-42 run in 568.10 seconds. Both integer
ledgers closed every tick and the runtime concentration ceilings held. Final
G00/G01/G10/G11 biomass shares were 0.0306904565 / 0.8137141892 /
0.0046548077 / 0.1509405466. Mean DET was 0.2706843280 mol/m3. The same
0.0003 mol/m3 biomass contour radius grew from 0.327872 to 1.991858 mm.
All four genotypes remained present; none was artificially replenished.

This is a verified 10,000-tick horizon, not a claim that every arbitrary
parameter choice or indefinitely long experiment remains viable. The ongoing
observer retains runtime guards and pauses a failed run with its error visible.

The biological gate is accepted. ADR-095 authorizes the new default after the
final responsive observer checks pass. Root owns that UI/operational evidence
and the final local commit. No push or deployment is authorized by this stage.

## Observer And Local Runtime

The responsive observer gate passed at 420x900, 590x900 and 1440x1000.
There was no horizontal overflow or clipped control. Resource mode originally
put the canvas in an auto-sized grid row when genotype chips were hidden;
explicit row placement fixed it. FOOD, DET and O2 now fill the observation
region and use actual per-voxel concentrations and distinct-tick histories.

Browser checks verified:

- A paused reset remains paused at tick 0: G00 is present, the other three
  genotypes are unobserved, and the DET field and its history scale are zero.
- One step advances exactly one tick and detects G01/G10, not G11. In the
  final-ID seed-42 browser run G11 was first detected at tick 17.
- Genotype inspection shows authoritative decoded factors and resource shares;
  unobserved genotypes remain explicitly unobserved. The graph is a Hamming-1
  neighbour graph, not an ancestry reconstruction.
- Slice endpoints, resource modes, genotype filters, pause/resume and speed
  controls work. Paused resource histories do not gain artificial time samples.
- The microscope was nonblank: 3,910 bright pixels in the checked crop.
  Two paused screenshots had zero changed microscope pixels. Resuming from
  tick 0 to tick 1,357 changed 29,852 pixels in the same region.
- A real browser download at tick 0 preserved the exact seed string
  `18446744073709551615`, scenario `genetic-colony`, world version 28,
  hash `blake3:fca3fa6d14223d59`, all four genome records and 576 slice cells.
  This exports an observation, not a restart checkpoint.
- The browser had no warning/error logs after the final rebuild.

Generated QA evidence is local and ignored by Git under `target/qa`:
`genetic-founder-mobile.jpg`, `genetic-founder-graph.jpg`,
`genetic-zero-det.jpg`, `genetic-food-narrow.jpg`, `genetic-desktop.jpg`,
`genetic-pause-a.jpg`, `genetic-pause-b.jpg`, `genetic-resumed.jpg`,
and `genetic-export.json`.

The accepted default is now `genetic-colony`, seed 42 (ADR-095), with a CLI
regression for the default path/seed/port. The final launcher was exercised
without `-Config` and served this scenario on `http://127.0.0.1:8081/`.
The original `living-world` process on 8080 is paused, not destroyed; its world
and accumulated simulation time are preserved. New culture remains running.

Verification included the full release workspace before the final load guards,
then the pre-hardening complete `liminis-core` release suite: 458 unit tests passed
(3 pre-existing ignored), all acceptance suites and 5 doctests passed. The
genetic causal suite passed 4 tests in 34.16 seconds; its deliberate long-soak
ignore was separately exercised above. Legacy living-world acceptance passed
5 tests. The full release server suite passed 42 tests, including the legacy
1,000-tick run in 216.81 seconds. After the observer identity/default edits,
all 42 short server tests passed again; only that already-passed long legacy
test was filtered out. Final compiler guard evidence is appended below.

Workspace clippy with all targets and warnings denied, formatting, the bare-Q
guard (13 files), and whitespace checks passed. No dependencies were added.
All source work remains on the isolated `codex/local-living-world` worktree;
the original checkout's unrelated local changes were not modified.

The final compiler guard rejects direct generated-genotype references in raw
growth/turnover templates' inputs, outputs and Km. This closes a mutation bypass
without changing the shipped valid program. All 9 compiler tests passed,
including all six forbidden map/template combinations. After this final guard,
the complete `liminis-core` release suite passed again: 459 unit tests
(3 pre-existing ignored), every acceptance suite and all 5 doctests. All 42
short server tests passed again, with only the already-passed long legacy test
filtered out. Release build, all-target clippy, formatting and the bare-Q guard
were repeated successfully.

The new release binary is rebuilt for future launches. The running 8081 process
was deliberately retained: it predates only this loader refusal guard, which
does not alter the valid materialization, hash, rates or state. Restarting it
would discard genuine accumulated development. Its identity remains seed 42,
`blake3:fca3fa6d14223d59`, world format 28. The process is detached and local;
stopping Codex does not intentionally terminate it.
