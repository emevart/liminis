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

## Expectations

This is a research project. Issues and questions are answered when there is time,
scenarios and analysis tooling are welcome, and pull requests against the numerical
kernels are accepted rarely — see `docs/COMMUNITY.md` for why the boundary sits there.

## Quickstart

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
step, change speed, choose a slice, restart with a seed and export the current
observation. A paused restart exposes tick 0. The observer includes actual food,
detritus and oxygen fields, session histories and decoded genotype traits with
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

The existing scenario identity command remains available:

```
cargo build --workspace
cargo run -p liminis -- run --config configs/scenarios/hello.toml --seed 42
```

The second command prints the triple that identifies a run:

```
seed=42 config_hash=blake3:<hex16> world_format_version=28 code_version=0.1.0
```

`config_hash` is computed after defaults are applied and over a canonical
serialization, so whitespace, key order, and comments in the TOML do not change it.
Changing a parameter value does.

## Documents

- [docs/NORTH_STAR.md](docs/NORTH_STAR.md) — what is being built and why. Wins over
  the spec if the two disagree.
- [docs/SPEC.md](docs/SPEC.md) — the design. It freezes the moment the first file
  appears in `crates/liminis-core/src/kernels/`; until then it is brought in line with
  the decision log rather than extended (ADR-032). A hook reads that directory, so the
  trigger is a check and not a promise.
- [docs/DECISIONS.md](docs/DECISIONS.md) — the decision log. Append-only; a reversal is
  a new entry, not an edit.
- [docs/CONFIG_SCHEMA.md](docs/CONFIG_SCHEMA.md) — the scenario schema: what a config
  declares and what the validator rejects at load time.

The remaining documents (`ARCHITECTURE.md`, `NUMERIC.md`, `QUANTITIES.md`,
`ACCEPTANCE.md`, `COMMUNITY.md`, `OPEN_QUESTIONS.md`, `PRIOR_ART.md`,
`LEARNING.md`) are indexed in `CLAUDE.md`. Everything under `docs/archive/` is a
superseded draft and contradicts the current spec.

## License

Apache-2.0. See [LICENSE](LICENSE) and [CITATION.cff](CITATION.cff).
