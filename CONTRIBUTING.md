# Contributing

Liminis is a research project. Issues and questions are answered when there is time.

## Where the boundary sits

The kernel is narrow and the config surface is wide. That is a structural decision
(ADR-018), not a rule in a document, and it decides what a pull request can be.

**Taken freely.** Scenarios — substances, reactions, initial conditions. Transfer
functions and visual presets. Analysis tooling and metrics. Documentation,
translations, examples. Packaging and platform ports. Tests.

**Taken with care.** A new environment process needs a golden test against an
analytical solution. Changes to the registries or to a file format move the world
semantics version, see below.

**Practically not taken.** The numerical kernels. The reason is not ownership: a
subtle error in a flux scheme gets past every reviewer. The physics degrades in
silence, runs keep looking plausible, and half a year later there is no way to
establish when the results stopped meaning anything. `docs/COMMUNITY.md` sets out the
same boundary at length.

One consequence is worth naming: adding a substance, a reaction or a scenario is a TOML
edit and a run. If it makes you touch Rust, that is a defect in the architecture — file
it as one instead of working around it.

## A run is a triple

```
(seed, config_hash, world_format_version)
```

Anything that reports a run — a bug, a scenario, a figure — carries all three. A bug
report without the triple cannot be reproduced and is closed with a request to complete
it; the issue template asks for each part. `config_hash` is taken after defaults are
applied and over a canonical serialization: whitespace, key order and comments do not
change it, a parameter value does. The toolchain pinned in `rust-toolchain.toml` belongs
to the same contract (ADR-024).

## WORLD_FORMAT_VERSION

The world semantics version is a separate counter from the code version (ADR-020).
Refactoring that preserves behaviour moves the code version and leaves this one alone.
Anything that changes what the numbers mean — a constant in a reaction, a formula, the
order of operations in a kernel — moves `WORLD_FORMAT_VERSION` in
`crates/liminis-core/src/version.rs`. Results produced under different values do not
belong on the same plot.

CI fails a pull request that touches `configs/**`,
`crates/liminis-core/src/kernels/**` or `crates/liminis-core/src/process/**` without
moving the constant. The third path counts because the order in which processes run
inside a tick is part of the world semantics: under Lie–Trotter splitting the result
depends on it, so swapping two processes moves the version even though no config and
no kernel changed (ADR-036). If a change there
genuinely cannot affect dynamics, say so in the pull request description and record
why; the template asks for exactly that.

## Checks

```
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/test-hooks.sh
python3 scripts/check_bare_q.py
```

Everything below the build line is what CI runs (`.github/workflows/ci.yml`). Running
it locally first saves a round trip. The last one is the grep for bare arithmetic over
`Q` inside kernels; it passes trivially while `kernels/` is still empty.

## Golden tests

There are none yet: the directory `tests/golden/` appears with the first environment
process, and this section describes the rule that will apply then.

A tool that can rewrite a reference to make a test green is not a reference, so an
update is a deliberate act:

```
LIMINIS_BLESS_GOLDEN=1 <command>
```

Be aware of what enforces this and what does not. The guard is a local hook that only
fires inside a Claude Code session; a shell redirect, or a plain editor, walks straight
past it. On a pull request the only thing standing between a rewritten reference and a
green build is a reviewer reading the diff. So if a golden moves, name it in the pull
request and say why the new answer is the correct one — that sentence is the whole
mechanism.

## Documents

`docs/SPEC.md` and `docs/NORTH_STAR.md` freeze when the first file appears in
`crates/liminis-core/src/kernels/` (ADR-032). While the window is open they are
brought in line with the decision log, not extended — a new idea goes to
`docs/OPEN_QUESTIONS.md`.

`docs/DECISIONS.md` is append-only. A decision is reversed by a new entry that names
the one it reverses, never by editing the old text; a log that can be rewritten
afterwards is not a log. Everything under `docs/archive/` is a superseded draft that
contradicts the current spec: read it if you like, do not cite it.

## Formalities

Apache-2.0 (ADR-019). Section 5 of the license already covers what you submit, so
there is no CLA and no DCO to sign. [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) applies.
