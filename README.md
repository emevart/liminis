# Liminis

Liminis is a voxel simulator of a biological ecosystem. Environment and organisms are
made of the same substance registry and driven by the same reaction engine; nothing
appears or disappears without passing through a named, counted channel. It exists to
ask one question: do major evolutionary transitions arise in such a system, and under
what conditions?

The name is the genitive of Latin *limen*, "threshold" — the voxel threshold that
separates a field from an object, and the evolutionary threshold of a transition.

## Status

На 2026-09-09 в коммите d21fb41 реализованы ядра, цикл тиков и локальный
viewer. Наличие кода не означает завершение приёмки S0: текущие критерии
и расхождения находятся в [ACCEPTANCE](docs/ACCEPTANCE.md). Перед продолжением
сверьте статус с текущей веткой и результатами проверок.

The roadmap is laid out in `docs/SPEC.md` §13 and starts at S0: infrastructure first
(pinned toolchain, config validator, golden tests, `world_format_version`), then the
grid, the fields, advection, diffusion with substeps, pressure, light, enthalpy, the
reaction engine with abiotic chemistry, a ledger for substance and a ledger for
energy, and a volume viewer. Chemistry is in S0 because transport conserves substance
by construction, so on a world without reactions the central invariant has almost
nothing to catch (ADR-031) — and with oxidation fronts and chemical zonation the
stage is no longer a dead world, though not yet a live one. Temperature is absent from
the list on purpose: it follows from stored enthalpy instead of being a field of its
own (ADR-028).

## Expectations

This is a research project. Issues and questions are answered when there is time,
scenarios and analysis tooling are welcome, and pull requests against the numerical
kernels are accepted rarely — see `docs/COMMUNITY.md` for why the boundary sits there.

## Quickstart

```
cargo build --workspace
cargo run -p liminis -- run --config configs/scenarios/hello.toml --seed 42
```

Команда run печатает идентичность конфигурации, не запускает симуляцию.
Для локальной симуляции и viewer:

~~~text
cargo run --release -p liminis -- serve --config configs/scenarios/h2s-oxidation.toml --seed 42
~~~

В release нет debug-проверки невязки LEDGER.

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
