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

CLI source-guard acceptance/evidence выполняются на committed clean source;
результат будет добавлен перед окончательной передачей PR. Browser QA:
N/A, UI в LAB-1 отсутствует. CI candidate ещё NOT RUN до создания PR.
Checkpoint не подменяет проверку exact remote head перед merge.

## Следующий этап

После ACCEPTED/MERGED LAB-1: отдельная paired матрица LAB-2, затем LAB-3.
Никакая работа этого этапа не доказывает полную приёмку S0/S1/S2.
