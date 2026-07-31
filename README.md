# Liminis

Liminis is a voxel simulator of a biological ecosystem. Environment and organisms are
made of the same substance registry and driven by the same reaction engine; nothing
appears or disappears without passing through a named, counted channel. It exists to
ask one question: do major evolutionary transitions arise in such a system, and under
what conditions?

The name is the genitive of Latin *limen*, "threshold" — the voxel threshold that
separates a field from an object, and the evolutionary threshold of a transition.

## Status

Pre-S0. Nothing is simulated yet.

There is no grid, no fields, no tick loop, no kernels. The repository currently holds
the design documents, a Cargo workspace, and a single command that reads a scenario
config, applies defaults, and prints the identity of the run it would have performed.
The roadmap starts at S0 ("dead world": grid, advection, diffusion, pressure, light,
temperature, ledger) and is laid out in `docs/SPEC.md` §13.

## Expectations

This is a research project. Issues and questions are answered when there is time,
scenarios and analysis tooling are welcome, and pull requests against the numerical
kernels are accepted rarely — see `docs/COMMUNITY.md` for why the boundary sits there.

## Quickstart

```
cargo build --workspace
cargo run -p liminis -- run --config configs/scenarios/hello.toml --seed 42
```

The second command prints the triple that identifies a run:

```
seed=42 config_hash=blake3:<hex16> world_format_version=0 code_version=0.1.0
```

`config_hash` is computed after defaults are applied and over a canonical
serialization, so whitespace, key order, and comments in the TOML do not change it.
Changing a parameter value does.

## Documents

- [docs/NORTH_STAR.md](docs/NORTH_STAR.md) — what is being built and why. Wins over
  the spec if the two disagree.
- [docs/SPEC.md](docs/SPEC.md) — the design, frozen until the first working run.
- [docs/DECISIONS.md](docs/DECISIONS.md) — the decision log. Append-only; a reversal is
  a new entry, not an edit.

The remaining documents (`NUMERIC.md`, `QUANTITIES.md`, `COMMUNITY.md`,
`OPEN_QUESTIONS.md`, `PRIOR_ART.md`, `LEARNING.md`) are indexed in `CLAUDE.md`.
Everything under `docs/archive/` is a superseded draft and contradicts the
current spec.

## License

Apache-2.0. See [LICENSE](LICENSE) and [CITATION.cff](CITATION.cff).
