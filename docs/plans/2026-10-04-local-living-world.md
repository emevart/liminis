# Local Living World: 2026-10-04

## Goal

Run a local native simulation with an adjacent browser window that shows real
biomass growth, resource competition, turnover and changing inherited ecotype
shares. The original checkout remains untouched; work is isolated on
`codex/local-living-world`.

## Architecture

- Native Rust core remains the only simulator. The existing integer amount,
  reaction extent, chemical energy and transport paths also carry biomass.
- ADR-091 chooses five finite ecotypes instead of the unimplemented BT/Lande
  representation of ADR-011. Mutation is offspring production in adjacent
  ecotypes, with total branch rates equal to the parent's growth rate.
- ADR-092 extends local HTTP polling with an atomic ecological slice and
  explicit reset/speed controls. A rendered voxel is not an individual cell.
- `living-world.toml` is an uncalibrated CH2O-equivalent culture. Individual
  genomes, morphogenesis, lineage discovery and calibration against micro-mode
  are not implemented by this milestone.

## Work

1. Preserve the incoming reaction-name-key changes in the isolated worktree.
2. Add a release-active concentration ceiling guard and wire reaction dispatch.
   Published matter residuals use actual reaction stoichiometry and extents.
3. Resolve guild catalysts to existing substance lanes and fold concentrations
   once into Scratch before the common reaction kernel.
4. Add the 24-cubed culture with food, oxygen, water, carbon dioxide, detritus and
   five ecotypes. Verify stoichiometry, rate branching and domain behaviour.
5. Add atomic ecology observation and explicit pause, step, speed and reset.
6. Build a compact live viewer with true spatial fields, abundance histories,
   biomass-weighted growth traits and readable error/connection states.
7. Verify tests, numerical guards, local startup and browser interaction.
   Record the last verified evidence; do not mark open-ended evolution complete.

## Acceptance

- A shipped scenario loads and runs with nonzero reaction extents.
- Matter and energy residuals close with the actual reaction matrix.
- Undeclared catalysts fail; catalyst zero gives zero biological growth.
- Identical seed/config produces identical amounts after the same tick count.
- Ecotype shares change from differential reproduction and the stated tradeoff.
- A mutation branch produces offspring of its declared destination; branching
  preserves chemistry and the parent's combined potential growth rate.
- Biomass loss reaches detritus, which can remineralise; no silent disappearance.
- A sustained release run does not violate declared concentration ceilings.
- Pause, single-step, reset with the same seed, reset with a different seed,
  target speed, slice selection and observation export work in the browser.
- Desktop and narrow-window layouts remain legible; field pixels and plots are
  generated from the API state. UI errors expose stopped or unreachable runs.

## Verification Status

Core/scenario verification completed on 2026-10-04:

- `cargo test --release -p liminis-core --test acceptance_living_world -- --nocapture`
  passed all five short acceptance tests; the separately invoked soak is ignored
  in the ordinary suite. The final short run took 0.91 seconds.
- `cargo test -p liminis-core --test acceptance_living_world -- --nocapture`
  passed the first four tests in debug (7.69 seconds); the subsequently added
  catalyst-poison/sterile-culture test passed separately with
  `cargo test -p liminis-core --test acceptance_living_world living_catalysts -- --nocapture`.
- `cargo test --release -p liminis-core --test acceptance_living_world living_world_10k -- --ignored --nocapture`
  passed 10,000 ticks of the complete 24-cubed scenario, seed 42, in 377.53
  seconds. Both integer ledgers were checked explicitly on every tick in release,
  and the runtime concentration guards remained satisfied.
- Same-seed replay compares every amount and the enthalpy buffer. A single
  GENERALIST founder produces both adjacent variants; removing cross-ecotype
  birth reactions leaves unseeded types at exactly zero. Poisoning catalyst
  scratch before a tick changes no amount or enthalpy; a sterile culture stays
  sterile despite available food.
- `git diff --check` passed for the scenario, new acceptance tests and these docs.

The full-grid run changed biomass shares as follows:

| Ecotype | Initial share | Share at tick 10,000 |
|---|---:|---:|
| HARVESTER | 20.4386% | 0.1147% |
| FORAGER | 18.9875% | 0.2718% |
| GENERALIST | 20.3594% | 1.2014% |
| OPPORTUNIST | 19.8966% | 26.7924% |
| BLOOMER | 20.3178% | 71.6197% |

All five ecotypes remain present. This is evidence of finite inherited strategy
selection with reproduction mutation, not evidence of new genes, new species,
morphogenesis, an evolutionary transition or indefinitely sustained diversity.
The culture starts with all five variants present; the single-founder mutation
witness is an acceptance fixture, not the default initial state.

The shipped culture deliberately uses diffusion and an exchange reservoir.
Velocity and advection are explicitly disabled because their exchange-face
behaviour is not implemented; the corresponding core refusal was preserved.
The 10,000-tick result is the verified duration, not a guarantee of arbitrary
overnight horizons. Model coefficients remain uncalibrated.

Integration and browser verification completed on 2026-10-04:

- `cargo test --workspace --release --quiet` passed, including all 40 binary
  tests and the shipped chemical scenario's 1,000-tick regression (182.29
  seconds for the binary suite). After the final viewer changes, the workspace
  suite passed again with only that already-passed long test filtered out:
  `cargo test --workspace --release --quiet -- --skip the_shipped_scenario_survives_a_thousand_ticks`.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
  `python scripts/check_bare_q.py` and `git diff --check` passed. The Q guard
  checked 13 kernel files.
- `scripts/start-local.ps1` built and started a detached loopback server at
  `http://127.0.0.1:8080/`. The executable is copied out of Cargo's output path
  so observation does not lock subsequent Windows builds.
- CUA browser interaction verified pause, one tick (1204 to 1205 while paused),
  speed 60, slice 20, ecotype visibility, seed reset and the exact unsigned
  64-bit seed `18446744073709551615`. The final run was returned to seed 42,
  target 30 ticks/second and running state.
- Same-origin observation export downloaded a valid JSON frame with 576
  samples and five ecotypes. Its final filename is
  `living-world-observation.json`; tick and seed are read from the file, not
  from an earlier screen frame. This is not a resumable simulation checkpoint.
- At 420x880, 590x900 and 1440x960 there were no elements extending beyond the
  viewport and no horizontal overflow. Canvas glyphs are fixed aggregate
  voxel samples, not separately simulated individuals. Hue comes from the
  dominant ecotype, area from biomass, and internal bands from local shares.
- Screenshot pixel checks sampled 23,250 field positions: 11,377 were coloured,
  none changed between two paused frames, and 22,836 changed after resuming.
  Browser error/warning logs were empty. Local screenshots are in
  `target/qa/viewer-420.jpg`, `viewer-590.jpg`, `viewer-1440.jpg` and
  `viewer-pane.jpg` (generated evidence, not tracked source).

The legacy 3D toggle is deliberately hidden for ecological cultures. The old
volume protocol quantises against declared maximum concentration (ADR-072),
which loses early biomass below approximately 0.0392 mol/m3 at max 20. A shader
gain cannot recover those zero bytes. Its camera also needs separate legacy
framing work. The chemistry-only fallback remains intact; no ecological 3D
claim is made and no numerical limit was lowered for a prettier picture.

The original `H:\liminis` checkout retained its 12 incoming modified files.
This isolated branch includes their reaction-name-key snapshot intentionally.
There is no deployment, push or external runtime service.
