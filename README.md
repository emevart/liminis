# Liminis

Liminis is a voxel simulator of a biological ecosystem. Environment and organisms are
made of the same substance registry and driven by the same reaction engine; nothing
appears or disappears without passing through a named, counted channel. It exists to
ask one question: do major evolutionary transitions arise in such a system, and under
what conditions?

The name is the genitive of Latin *limen*, "threshold" — the voxel threshold that
separates a field from an object, and the evolutionary threshold of a transition.

## Status

The local observer runs a microbial culture in Rust and serves an offline web
application. A new genetic-colony experiment begins with a single founder
population. Two binary loci encode growth speed/affinity and allocation between
food and detritus. Reproduction creates single-locus mutant offspring; resource
competition changes genotype shares, turnover supplies detritus, and diffusion
and growth spread the colony. Biomass uses the same integer chemistry,
transport and matter/energy accounting as the environment. The earlier
five-ecotype `living-world` scenario remains available.

This is finite mutation and selection among four possible genotypes in biomass
fields. The capsule marks represent voxel aggregates, not individual cells.
Open-ended genomes and multicellular development are future stages. The full
research acceptance criteria of S0/S1 are not claimed complete. Scope and
verification are recorded in [the genetic-colony plan](docs/plans/2026-10-04-genetic-colony.md)
and [the earlier living-world plan](docs/plans/2026-10-04-local-living-world.md).
The observer now saves full restart checkpoints and exact scalar history to
local disk; see [the durable-experiment plan](docs/plans/2026-10-04-durable-living-experiment.md).

Наличие кода не означает завершение приёмки S0/S1: текущие критерии
и расхождения находятся в [ACCEPTANCE](docs/ACCEPTANCE.md). Перед продолжением
сверьте статус с текущей веткой и результатами проверок.

A separate individual-cell mode now runs an ideal well-mixed, isothermal chamber.
Every visible capsule is a real cell with an ID, parent, generation, inherited
genome, structural mass and energy reserve. Cells grow, divide into two offspring,
mutate a bounded kinetics locus and die from energy starvation. Finite reservoir
exchange feeds the chamber through counted matter/energy channels. This is not a
spatial microsolver: screen positions are presentation only, and there is no
GRN, adhesion or multicellular development yet. See the
[cell-chamber plan](docs/plans/2026-10-04-cell-chamber.md).

## Expectations

This is a research project. Issues and questions are answered when there is time,
scenarios and analysis tooling are welcome, and pull requests against the numerical
kernels are accepted rarely — see `docs/COMMUNITY.md` for why the boundary sits there.

## Quickstart

For the new individual-cell chamber, start a separate local observer:

```powershell
.\scripts\start-local.ps1 -Cells
```

Or:

```sh
cargo run --release -p liminis -- cells
```

It defaults to port 8083, seed 42 and `configs/scenarios/cell-chamber.toml`.
Its own storage is `.liminis/cells`; after stopping its server, continue with
`.\scripts\start-local.ps1 -Cells -Resume latest`. Pause and Save commit the
exact cell table and counters, not the browser picture. Autosave is every minute,
history samples every ten completed ticks, and the last three checkpoints remain.
Selecting a cell shows its actual identity and decoded traits. A hard safety
limit stops the experiment visibly; it never silently suppresses births or resets.
The old culture servers and `.liminis/runs` remain independent.

On Windows, start a detached local server (builds the release binary and chooses
a free loopback port):

```powershell
.\scripts\start-local.ps1
```

Or run the server in a terminal on any supported platform:

```sh
cargo run --release -p liminis -- serve
```

Open the printed localhost URL. The default experiment is `genetic-colony`
with seed 42; the viewer can pause,
step, change speed, choose a slice, start a new experiment with a seed, save a
checkpoint and export the current observation. A paused reset exposes tick 0
in a new experiment and preserves the previous one. The observer includes actual food,
detritus and oxygen fields, durable histories and decoded genotype traits with
first-detection ticks, not a reconstructed ancestry tree. Nothing is deployed
or sent to an external service.

Choose an experiment explicitly:

```powershell
.\scripts\start-local.ps1 -Config configs/scenarios/genetic-colony.toml -Seed 42 -Port 8081
.\scripts\start-local.ps1 -Config configs/scenarios/living-world.toml
```

The current local genetic culture on port 8081 was retained across the final
loader-only refusal guard so its accumulated growth is not lost. Its valid
scenario, hash and dynamics are unchanged; the freshly rebuilt release binary
includes that guard for subsequent launches. The earlier culture on 8080 is
paused with its state preserved.

## Save And Continue

Every new server writes to `.liminis/runs` by default (Git-ignored, not in
`target`). It saves at tick 0, every 60 wall-clock seconds while advancing, on
pause, and with the viewer's Save button. The status changes to "saved" only
after the complete checkpoint is committed. Checkpoints include both world
buffers, ledger counters, exact seed/config identity and observer metadata.
The browser's Export button remains an observation, not a restart file.

Continue the most recently created saved experiment after stopping its server:

```powershell
.\scripts\start-local.ps1 -Resume latest
```

Or select the run ID printed at startup:

```sh
cargo run --release -p liminis -- serve --resume run-<id>
```

Resume uses the saved canonical config and seed; passing `--config` or `--seed`
at the same time is an error. `--data-dir PATH` (launcher `-DataDir PATH`) selects
another storage folder. A running experiment continues running; a paused one
remains paused. Its residual is unreported until the next genuinely checked tick.
Only one process may write a particular experiment.

The latest three committed checkpoints per experiment are retained, including
manual saves. Older binary generations are pruned only after a new commit;
the sampled metric history and experiment namespaces are not deleted. A corrupt
latest checkpoint fails clearly, never silently starts over. An explicit retained
generation can be selected with `--resume run-<id> --checkpoint checkpoint-<id>`
(launcher `-Resume run-<id> -Checkpoint checkpoint-<id>`).

History samples are written every 30 completed ticks and at save/pause boundaries.
Each restart opens a new append-only segment. Earlier segments are clipped at the
chosen checkpoint for display, so an abandoned future is not merged into the new
trajectory. The chart is a bounded overview of actual samples, not a fabricated
continuous recording; exact integer metrics remain on disk.

The new durable observer is a separate experiment on port 8082. The pre-existing
8081 process cannot export its full state and was not reset or killed. Closing
the browser does not stop a detached server; pausing it commits a checkpoint,
and after stopping it the resume command continues from that checkpoint.

The existing scenario identity command remains available:

```
cargo build --workspace
cargo run -p liminis -- run --config configs/scenarios/hello.toml --seed 42
```

Команда run печатает идентичность конфигурации, не запускает симуляцию.
Для локальной симуляции и viewer:

~~~text
cargo run --release -p liminis -- serve --config configs/scenarios/h2s-oxidation.toml --seed 42
~~~

В release нет debug-фазы LEDGER ядра полей. Локальные живые наблюдатели
проверяют свои балансы отдельно; индивидуальная камера проверяет обе
целочисленные невязки на каждом завершённом тике также в release.

Пример формы идентичности, значения зависят от checkout:

```
seed=42 config_hash=blake3:<hex16> world_format_version=<current> code_version=0.1.0
```

`config_hash` is computed after defaults are applied and over a canonical
serialization, so whitespace, key order, and comments in the TOML do not change it.
Changing a parameter value does.

## Documents

- [docs/NORTH_STAR.md](docs/NORTH_STAR.md) — what is being built and why. Wins over
  the spec if the two disagree.
- [docs/SPEC.md](docs/SPEC.md) — the design. It is already frozen because implementation has begun in `crates/liminis-core/src/kernels/` (ADR-032). A hook reads that directory, so the
  trigger is a check and not a promise.
- [docs/DECISIONS.md](docs/DECISIONS.md) — the decision log. Append-only; a reversal is
  a new entry, not an edit.
- [docs/CONFIG_SCHEMA.md](docs/CONFIG_SCHEMA.md) — the scenario schema: what a config
  declares and what the validator rejects at load time.

The remaining documents (`ARCHITECTURE.md`, `NUMERIC.md`, `QUANTITIES.md`,
`ACCEPTANCE.md`, `COMMUNITY.md`, `OPEN_QUESTIONS.md`, `PRIOR_ART.md`,
`LEARNING.md`) are indexed in [docs/README.md](docs/README.md).
Правила агентов: [AGENTS.md](AGENTS.md). Everything under `docs/archive/` is a
superseded draft and contradicts the current spec.

## License

Apache-2.0. See [LICENSE](LICENSE) and [CITATION.cff](CITATION.cff).
