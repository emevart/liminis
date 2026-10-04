# Individual Cell Chamber

## Scope

Deliver a separate local, durable experiment containing actual individual cells:
growth, energy maintenance, binary fission, inherited bounded kinetic variation,
starvation death and exact matter/energy accounting. The environment is an ideal
well-mixed, cell-retaining supplied chamber. This is not the spatial S1-prime
solver, morphogenesis, GRN, adhesion or open-ended genome evolution.

Approved under the user's delegated architectural authority to Astra. Frozen
SPEC and NORTH_STAR are unchanged. ADR-099 and ADR-100 define the boundary and
compatibility; the exact cell-cycle accounting is recorded separately.

## Ownership

- Micro core: individual state, deterministic competition, cell cycle, exact
  per-tick ledgers, transactional failure and full snapshot replay.
- Micro config: standalone schema, inherited physiology, existing chemistry
  derivation, canonical identity and supplied scenario calibration.
- Cells storage: host-only bounded persistence worker, full-state publication,
  lock, checksums, three-generation retention and durable history lineage.
- Viewer: dedicated individual-cell microscope and inspector, real history,
  responsive controls, no simulated positions or ambient motion.
- Root: CLI/launcher, cells host/API, compatibility reader, browser verification
  and final integration. Astra: decisions, review, acceptance and this plan.

## Model Contract

One visible capsule corresponds to one actual living cell ID. Its screen
position and orientation are display layout, never physical state. Individual
mass, energy, age, parent ID, generation and inherited genome are inspectable.
The mutable locus couples uptake speed and resource affinity; all physiological
values are inherited rather than separate species-level overrides.

Free chemical pools are shared. Every tick gathers demand, fairly apportions
the limiting substrate and applies integer extents. Structural BIO belongs to
the cell table; it must be included in chemical matter and energy totals.
Fission retires the parent, partitions exact mass and remaining reserve into
two children and transfers its explicit energy cost into the thermal bath.
Starvation death moves structural BIO into equal-scale, equal-composition DET
and transfers remaining reserve plus chemical-energy correction into the bath.

The ideal isothermal bath is inside the accounting boundary. Its cumulative
received heat is not a derived chamber temperature or eco enthalpy field.
Chemical medium inputs/outputs carry exact named matter and energy credits.
There is no automatic reseeding or reset. The object limit is a visible safety
failure, never a silent physical carrying capacity.

## Delivery Gates

- Config canonical roundtrip and hash sensitivity; invalid stoichiometry,
  unrepresentable scales, genome bounds and structural exchange rejected.
- Exact matter and energy closure in release on every completed tick.
- Cell-table permutation cannot change allocation or child lineage.
- Fission, maintenance and lysis have exact independently checked transfers.
- Mutation-zero preserves genotype; mutations occur only at birth and remain
  in bounds; no-food and sterile controls do not create life.
- Snapshot restore reproduces exact future, including IDs, starvation clocks,
  genomes, counters and medium accounting.
- Supplied scenario runs at least 10,000 steps with divisions, deaths and
  living survivors without hitting the object safety limit.
- Full checkpoint corruption, wrong identity/kind, second writer, lineage
  clipping, three-generation retention and restart tested.
- Existing eco 28 load/advance/save preserves identity and exact config hash;
  other unsupported versions and cross-kind snapshots fail closed.
- Real 420/590/1440 browser observation, canvas pixels, inspector, run/pause,
  step, speed, reset, save and process restart verified on new port 8083.
- Existing eco regression tests, formatting and clippy pass.

## Runtime Safety

Existing 8080, 8081 and 8082 processes must not be reset or killed. New cells
experiments use `.liminis/cells` and port 8083; failed integration does not
invalidate the running eco experience. No deployment or push is authorized.

The active world-28 executable was preserved as ignored local fallback
`.liminis/legacy/eco-world28.exe`, SHA-256:

`B423DCBE37259B0A9508ACB24E3DDC46BEECDEBD5EFEBC99CBA570CFDF29F2B3`

After the original writer is deliberately stopped, it can resume its existing
run on another port with `serve --resume <run-id> --data-dir .liminis/runs
--port 8084`. A running process cannot be migrated from observation JSON.

## Verification Status

Core and scenario acceptance completed; host/browser and final workspace gates
are recorded separately below. The earlier accepted eco experiences remain
independent of this milestone.

### Biology And Replay

- `cargo test -p liminis-core --release --test acceptance_cell_chamber -- --nocapture`:
  four tests passed (0.10 s final independent rerun). Sterile supplied medium
  creates no cells; food deprivation lyses founders and preserves exact energy;
  mutation probability zero preserves the entire genome through real fissions;
  snapshot restore at tick 300 reproduces the next 500 ticks exactly for seed
  `18446744073709551615`, including individual state and independently summed
  chemical/medium/lysis ledgers.
- Nine focused `micro::tests` unit tests passed: exact transfers, allocation
  and fission lineage under cell-table permutation, transactional capacity
  refusal, replay and altered-physiology rejection. Five focused config tests
  passed: units, canonical roundtrip, free BIO, lysis composition and energy
  identity. Core all-target clippy with `-D warnings` passed.
- `cargo test --release -p liminis-core micro::config::tests::shipped_cell_chamber_sustains_real_turnover_for_10k_steps -- --ignored --nocapture`:
  10,000 steps passed with seed 42, exact per-tick matter and energy residuals,
  94 final living cells, minimum 76 after tick 2000, maximum 248 of the safety
  limit 512, 1044 births and 436 deaths. The test took 0.60 s; its separate
  release compilation took 49.42 s. Observed kinetic alleles were -1, 0 and 1;
  final survivors had allele 0. Transient variation is demonstrated, not
  persistent diversification or open-ended evolution.
- Final calibrated operational source uses `medium_exchange_per_s = 2e-6`;
  canonical identity is `blake3:1802c0f129855749`, also verified from the live
  8083 state. This is numerical/operational calibration of a CH2O-equivalent
  prototype, not calibration against a measured biological species.
- The first supplied parameter draft hit the explicit capacity guard before
  1000 ticks. The response was to reduce declared reservoir supply, not raise
  the object limit or suppress fission. Food/energy budgets now bound the
  observed population. No indefinite-lifetime guarantee follows from 10k steps.

### Review And Storage

- Review corrected extent-versus-pool Km units, BIO-per-extent growth conversion,
  strict finite stochastic-rounding bounds and pre-fission ID sorting. The
  phenotype decoder is shared by engine and observer.
- The chemistry projection reuses scale/stoichiometry/energy derivation but
  neutralizes spatial diffusivities in its private clone. Its small validator
  enthalpy diffusivity is not a chamber transport coefficient: there is no
  spatial thermal field in the actual chamber.
- Storage focused suites passed nine tests in debug (9.86 s) and release
  (4.68 s): exact integer JSON, two-phase resume, single-writer lock, corruption
  refusal, branch clipping, torn-tail handling, session-header binding,
  publication failure, bounded history and retention of three generations.
- Host review required atomic commit of candidate core state and checked
  cumulative counters. A forced counter-overflow regression verifies that
  failure cannot advance the core while leaving saved observation accounting
  behind. Full-state restore additionally rechecks global matter/energy from
  founders plus cumulative external channels and actual reaction/lysis extents.

### Host And Browser

- `cargo test --workspace --release` passed the complete non-ignored suite:
  72 host tests, 473 core unit tests, all non-ignored integration tests and five
  doctests. The longest host regression took 178.86 s. Existing explicitly
  ignored research criteria remain unaccepted. The full local log is
  `target/qa/cell-workspace-tests.log` (ignored QA output).
- Final `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `python scripts/check_bare_q.py` (13 files) and
  `git diff --check` passed. After the final test-fixture float representation
  change, all ten cell storage release tests passed again (2.79 s).
- Focused debug verification also passed: `cargo test -p liminis cells_ --
  --nocapture` ran 16 host/storage/CLI tests (6.33 s), and `cargo test -p
  liminis-core micro:: -- --nocapture` ran 14 core/config tests (0.01 s), with
  the explicit calibration soak still correctly excluded from that command.
- The explicit 10k supplied-chamber soak passed independently again in release
  (0.50 s): final 94, mature minimum 76, maximum 248, births 1044, deaths 436,
  observed alleles {-1, 0, 1}, final allele {0}.
- Real CUA browser checks covered 420/590/1440 viewports, actual-cell inspection,
  keyboard selection, pause/run, one-tick advance, speed, seed reset, save and
  restart. At 420 px, the document and every inspector section had equal
  client/scroll widths; controls retained their fixed 34-by-32 px size. At
  1440-by-1000, the chamber was 1130-by-888 and the toolbar fit without page
  overflow. The viewport override was removed after testing.
- Offline pixel checks of CUA screenshots found 11,310 bright samples in the
  desktop chamber, exactly zero changed chamber samples between paused frames,
  and 1,804 changed samples after real steps resumed. Mobile full-page captures
  contained 4,063/5,775 bright chamber samples. These prove nonblank rendering
  and real state changes, not physical movement: screen packing remains an
  observer-only layout. Browser warning/error logs were empty.
- Local evidence is `target/qa/cell-chamber-desktop-a.jpg`,
  `cell-chamber-desktop-b.jpg`, `cell-chamber-running.jpg`,
  `cell-chamber-420-full.jpg`, `cell-chamber-590-full.jpg` and
  `cell-chamber-final.jpg` under the same QA directory. The screenshots and
  live `.liminis` data are not source-controlled deliverables.
- A real process stop/resume first exposed an invalid storage guard: typed JSON
  reserialization is not a lexical identity test for derived float values.
  The loader now consumes exactly one JSON value (`Deserializer::end`), while
  retaining byte length, checksum and semantic identity checks. The existing
  serde_json dependency enables `float_roundtrip`; no package was added.
  Regressions cover whitespace/equivalent number lexemes, a second JSON value,
  challenging inherited float physiology and identical future ticks.
- The repaired restart at tick 381 restored the exact core/observer JSON
  subtree (SHA-256
  `3DD01DB48E87CE3FF2335FAEA6653D76700E7614D94CE0595656FE380FD222AF`),
  124 living cells, 232 births and the saved paused mode.
- A second actual process restart at tick 47,802 restored 98 living cells,
  4,522 births, 2,171 deaths, 2,261 divisions, maximum generation 28 and target
  100 ticks/s. Full core/observer checkpoint trees were compared recursively:
  every integer matched exactly and every float had identical f64 bits.
  JSON byte spelling was deliberately not used as the semantic comparison.
  The resumed residual was unknown until one real tick executed, then both
  residuals were zero. The experiment was returned to running without reset.
- Live mode/speed changes observed before the root's explicit controls were
  not attributed to a source without evidence. The saved restart baseline was
  separately verified as paused. Existing 8080/8081/8082 hosts were not stopped
  or reset; the final health probe found all alive with zero residuals and no
  errors. New cells host 8083 was also running with zero residuals and no
  simulation/storage error.

## Continuation Boundary

This stage is a working local prototype, not acceptance of complete S0, S1 or
S1-prime research criteria. It does not have physical cell positions, Brownian
trajectories, spatial chemical fields, contact mechanics, adhesion, GRN, bodies
or full durable phylogeny. Its short event ring is not an ancestry database.
The apparent lines of capsules are a display-packing artifact, not evolved
chains or colonies.

Astra's recommended next bounded milestone is a new spatial chamber experiment:
saved physical positions and occupied volume, calibrated passive displacement,
boundaries, daughter placement and 3D/slice observation of the same state. The
first such chamber may keep its chemical medium explicitly well-mixed; no fake
gradients should be rendered. Decisions on displacement statistics, volume and
contact resolution must go through a new ADR before implementation.

Next comes S1-prime local chemistry and the stationary micro solver, then
S1 ecological calibration and C/N cycles. S2 adds genetic regulation and
adhesion, first proving reproducible development with mutation disabled. S3
adds richer mutation, durable lineage and selection at the collective level.
S4 environment changes and S5 GPU acceleration follow actual research and
performance needs. Every stage needs causal controls, several seeds, exact
accounting/restart and its own declared horizon; a successful 10k chamber run
does not replace the SPEC's million-tick environment criterion.

On 2026-10-04, GitHub `main` was freshly verified at `580fddb` (agent-documentation
PR #7 merged), while this isolated branch still starts from `d21fb41`; no open
GitHub PR existed. Before publication, reconcile the branch with current main,
preserve its new AGENTS instructions, rerun affected checks, then open a scoped
PR and wait for CI/review. No push or merge has been performed at this boundary.
The dirty files in the original `H:\liminis` checkout are the earlier ADR-090
reaction-name identity work, already incorporated and extended in this isolated
branch; they must not be separately reapplied or discarded. The original
checkout stays untouched. Source/config/tests/ADRs/plans can move through Git; live
experiments require a separate explicit validated-checkpoint transfer.

The publication follow-up is recorded in
`docs/checkpoints/2026-10-04-publication.md`. Its numerical world-30 correction
supersedes the world-29 eco compatibility described above; those earlier tests
remain historical evidence, not permission to continue old eco dynamics with
the corrected engine. Existing local eco executables/cultures are preserved.

Cloud continuation should start from published source with the pinned Rust
toolchain and Cargo.lock, Python 3, headless acceptance commands and these
scope limits. A cloud coding workspace is not the always-on aquarium: keep
local viewing/visual QA and live checkpoint ownership separate. Cloud execution
and Linux CI are not claimed verified by the local Windows test results.
