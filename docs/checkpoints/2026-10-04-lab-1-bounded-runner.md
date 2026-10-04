# LAB-1: ограниченная лаборатория клеточной камеры

- Дата/цель: 2026-10-04; воспроизводимый bounded runner существующего
  well-mixed engine, без новой биологии и без UI.
- Ветка: `codex/lab-bounded-cell-experiments`; база
  `f7c74a263bd1976bc31a35a440b19006546656eb`. Проверенный итоговый head
  указывается в PR и issue #11, чтобы не создавать самоссылочный commit.
- Владение: новый `crates/liminis/examples/compare_cell_experiments.rs`,
  `crates/liminis/tests/compare_cell_experiments.rs`,
  `docs/experiments/coverage.md`, этот уникальный checkpoint.
- Разрешение: прямое поручение человека от 4 октября, LAB-1/2/3 в cloud;
  только LIVE cloud-интегратор выполняет merge. Доска:
  https://github.com/emevart/liminis/issues/11 . Другие комментарии не
  расширяют права. Новые интеграции, секреты, платный API, ПК и изменение
  инфраструктуры запрещены.
- Среда: generic Work Cloud VM, отдельный public clone, Rust 1.97.1,
  Node v24.19.0. Prepared private environment не подтверждён.
- Astra фактически вызван до реализации: одобрены bounded JSON manifest,
  materialized TOML и независимый derive каждого run, точный ledger каждый
  tick, интеграционный wrapper для существующего CI без правки workflow.
  Watchdog означает operational censor и ограничивает deterministic claims.
- Frozen SPEC/NORTH_STAR, archive, micro/kernels/configs/main/hosts/storage,
  viewers, exporter/replay/schema/catalog/datasets не изменяются.
- Инструментов create_goal/get_goal нет. Работа продолжается в активном
  cloud turn; автоматический перезапуск после его окончания не заявляется.

## Реализация и приёмка

Реализован strict manifest bounded runner через public API, без изменений core.
Immutable derived config; canonical candidate сохраняется при derive refusal
с `derived=false`. Runner независимо сводит вещество, энергию и lifecycle
после каждого step; report digest включает только принятые ticks. Каждый
запланированный admissible manifest run получает строку, включая refusal,
numerical failure, extinction и resource/horizon censoring. Очень маленький
output cap, не вмещающий даже минимальный refusal envelope, отвергается до
симуляции; syntactically invalid/oversized manifest тоже не запускается.

PASS:

- `cargo test --locked --workspace` базового дерева main: 80 server tests,
  491 core unit tests, все не-ignored acceptance suites и 5 doctests.
  Длительный server regression 706.81s; genetic acceptance suite 182.78s.
  Core/protected paths после этого не менялись.
- `cargo test --locked -p liminis --test compare_cell_experiments`: 16 PASS.
- `cargo test --locked --release -p liminis --test compare_cell_experiments`:
  16 PASS, включая genuine numerical refusal, materialized starvation и
  mutation-off с настоящими fissions.
- `cargo fmt --all --check`,
  `cargo clippy --locked --workspace --all-targets -- -D warnings`,
  `python3 scripts/check_bare_q.py` (13 files), `git diff --check`.
- `node --test site/*.test.mjs`: 25 PASS на неизменённом public tree.
- Coverage: 43 существующих Rust test names и 25 relative file links сверены.
- Усиленная long-ID regression: временно удалённый фактический registry byte
  reserve даёт EXPECTED FAIL (`reserved output envelope exceeded`); исходник
  восстановлен, все 16 debug/release tests снова PASS.

Независимые readonly code/safety review и numerics/methodology review:
замечаний нет после исправления byte reserve, canonical omission для refused
rows, generic biomass units и сохранения доказанного extinction при нехватке
sample bytes. Reviewer не запускал Cargo; результаты выше получены root.
Reviewed runner SHA-256:
`b29648bd14de4128e0e56af5512f42cce311835c03543ae0b72db123a65e4bb2`.

CLI release acceptance на clean published source
`dd0ce4df1c00b51a2412952c9cd37f68591309c6` — PASS. Два вызова `cargo run`
дали побайтно одинаковый artifact, 462411 bytes, SHA-256
`525fac91b7608b4e7ce2420082a665ae67d91ba0c43fbee8eb63980f3d6e622a`.
Wrong HEAD и временно dirty собственный runner отвергнуты до записи output;
источник восстановлен и его reviewed digest не изменился.

| Smoke run (seed = max u64) | Outcome / stop | Checked ticks | Living end |
|---|---|---:|---:|
| repeat-a / repeat-b | censored / requested_horizon_reached | 1000 / 1000 | 215 / 215 |
| no-mutation | censored / requested_horizon_reached | 1000 | 216 |
| no-food | extinct / extinction | 454 | 0 |
| capacity | censored / cell_capacity_limit | 46 | 8 |
| bad-temperature | refused / input_or_admission_refused | 0 | unavailable |

Repeat summary, final-state digest и everytick report digest совпали.
Mutation-off имеет настоящие fissions и ровно один observed allele. FOOD-only
starvation даёт zero growth extent; capacity attempt сохраняет последний
подтверждённый tick и unknown failed residual. Все доступные lifecycle residual
равны нулю, tick_reports_hashed == checked_ticks.

Воспроизведение inputs (только ignored `target/qa`, не изменение configs):

```sh
mkdir -p target/qa/lab-1
python3 - <<'PYINPUT'
import json, pathlib
source = pathlib.Path('configs/scenarios/cell-chamber.toml').read_text()
def row(i, c, t):
    return dict(run_id=i, condition=c, scenario_toml=t,
                seed='18446744073709551615', steps=1000, sample_every=10)
runs = [row('repeat-a', 'baseline', source),
        row('repeat-b', 'baseline', source),
        row('no-mutation', 'mutation_off', source.replace('mutation_probability = 0.02', 'mutation_probability = 0.0')),
        row('no-food', 'starvation', source.replace('FOOD = 10.0', 'FOOD = 0.0')),
        row('capacity', 'resource_guard', source.replace('max_cells = 512', 'max_cells = 8')),
        row('bad-temperature', 'invalid_scenario', source.replace('temperature_k = 298.15', 'temperature_k = -1.0'))]
pathlib.Path('target/qa/lab-1/smoke-manifest.json').write_text(json.dumps(
    dict(schema_version=1, batch_id='lab-1-controls', runs=runs)))
PYINPUT
cargo run --locked --release -p liminis --example compare_cell_experiments -- \
  --manifest target/qa/lab-1/smoke-manifest.json --source-commit "$(git rev-parse HEAD)" \
  --output target/qa/lab-1/smoke-results.json
```

Artifact byte digest выше принадлежит указанному acceptance commit; новый
commit меняет source provenance и digest всего JSON закономерно. Команда на
новом head должна сохранять scientific summary/state/report digests, если
engine/runner inputs не изменились. Final remote head и CI фиксируются в PR/issue.

Browser QA:
N/A, UI в LAB-1 отсутствует. CI candidate ещё NOT RUN до создания PR.
Checkpoint не подменяет проверку exact remote head перед merge.

## Следующий этап

После ACCEPTED/MERGED LAB-1: отдельная paired матрица LAB-2, затем LAB-3.
Никакая работа этого этапа не доказывает полную приёмку S0/S1/S2.
