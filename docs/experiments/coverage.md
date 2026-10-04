# Карта покрытия экспериментальной лаборатории

Это карта узких проверок существующей модели, а не приёмка стадий S0/S1′/S1/S2.
Исходная сверка: `f7c74a263bd1976bc31a35a440b19006546656eb`, world format 30,
chamber format 1. Замороженные [SPEC](../SPEC.md) и [NORTH_STAR](../NORTH_STAR.md)
не изменяются; действующие ограничения названы в [ADR](../DECISIONS.md) и
[ACCEPTANCE](../ACCEPTANCE.md).

Статусы относятся только к утверждению в своей строке:

- **verified** — есть конкретная исполняемая проверка и записанная прежняя
  приёмка; это не отметка повторного запуска в этой cloud-сессии.
- **partial** — часть требования проверяется, но названный более широкий
  результат не доказан.
- **missing** — требуемого механизма или соответствующей проверки на исходном
  commit нет. Запланированная работа не считается проверкой.

Существующие тесты ниже прочитаны, но этим документом не объявляются заново
пройденными. Новые команды, head, результаты и review LAB-1 фиксируются отдельно
в checkpoint этапа; отсутствие запуска означает **NOT RUN**, а не PASS.

## Существующая биология и учёт

| Требование SPEC / действующее решение | Статус | Точная проверка и граница вывода |
|---|---|---|
| §2.1, §6.4; ADR-101: оба целых баланса клетки на каждом завершённом tick | verified | `micro::tests::every_tick_closes_matter_and_energy`; независимая накопленная свёртка: `restored_future_matches_exact_cells_and_independent_cumulative_ledgers` в [acceptance_cell_chamber.rs](../../crates/liminis-core/tests/acceptance_cell_chamber.rs). Горизонт проверки конечен. |
| §6.4; ADR-101: деление и лизис не создают вещества или энергии | verified | `micro::tests::fission_only_partitions_parent_matter_and_energy`, `death_returns_structure_and_internal_energy`; `starvation_lyses_actual_cells_without_losing_matter_or_energy` в [acceptance_cell_chamber.rs](../../crates/liminis-core/tests/acceptance_cell_chamber.rs). |
| §6.2, §9; ADR-099/101: наследование, ID и отсутствие преимущества порядка хранения | verified | `zero_mutation_preserves_the_entire_inherited_genome` в [acceptance_cell_chamber.rs](../../crates/liminis-core/tests/acceptance_cell_chamber.rs); `micro::tests::competition_is_independent_of_cell_storage_order`, `division_lineage_is_independent_of_cell_storage_order`. |
| §6.2; ADR-099/102: ограниченный локус kinetics и оплаченный speed/affinity tradeoff | partial | `micro::tests::phenotype_tradeoff_reverses_with_resource_abundance`; `micro::config::tests::kinetics_program_requires_a_real_speed_affinity_tradeoff`, `all_mutable_endpoint_phenotypes_must_be_positive_and_representable`. Это ограниченная кинетика, без нового метаболического пути и открытой эволюции. |
| §6.1; ADR-099: жизнь не возникает из стерильной среды | verified | `sterile_supplied_medium_does_not_create_cells` в [acceptance_cell_chamber.rs](../../crates/liminis-core/tests/acceptance_cell_chamber.rs): 500 переходов, без подсевов и автоматического reset. |
| §2, §9; ADR-099/101/102: отказ safety guard атомарен | verified | `micro::tests::capacity_error_is_transactional`; snapshot guards: `completed_snapshots_reject_impossible_starvation_states`, `snapshot_and_nested_cell_genome_reject_unknown_fields`, `invalid_snapshot_is_refused`. `max_cells` — предел ресурса, не физическая вместимость. |
| §9, §11.3; ADR-100/103: точное продолжение state | verified | `micro::tests::snapshot_restores_the_exact_future`; `restored_future_matches_exact_cells_and_independent_cumulative_ledgers` в [acceptance_cell_chamber.rs](../../crates/liminis-core/tests/acceptance_cell_chamber.rs). Core replay не заменяет отдельную проверку host/storage compatibility. Eco28/29 новым world30 не продолжаются. |
| §2, §10; ADR-101/102: физические декларации выводят целые runtime units | verified | `micro::config::tests::shipped_cell_chamber_derives_exact_runtime_units`, `positive_declared_energy_costs_cannot_round_to_free_upkeep_or_fission`, `free_structural_biomass_is_rejected`, `lysis_requires_matching_bio_and_det_composition`, `runtime_restore_rechecks_the_energy_identity`. Конечность параметров не доказывает бессрочный безопасный горизонт. |
| §9, §11.3; ADR-100: canonical chamber config/hash | partial | `micro::config::tests::canonical_chamber_round_trip_keeps_its_separate_hash` проверяет round trip и отделение eco-loader. Сам по себе он не проверяет hash каждого варьируемого параметра LAB и provenance executable. |

Unit-проверки `micro::tests` находятся в [micro/mod.rs](../../crates/liminis-core/src/micro/mod.rs),
`micro::config::tests` — в [micro/config.rs](../../crates/liminis-core/src/micro/config.rs).
[ACCEPTANCE, «Индивидуальные клетки»](../ACCEPTANCE.md#индивидуальные-клетки-в-однородной-камере-adr-099-adr-100-adr-101)
и [исторический план камеры](../plans/2026-10-04-cell-chamber.md) записывают
release-приёмку четырёх causal tests и отдельный ignored soak
`micro::config::tests::shipped_cell_chamber_sustains_real_turnover_for_10k_steps`.
Исторический seed42/10k run наблюдал -1/0/+1, но в конце только 0: это
временное разнообразие, не устойчивая диверсификация. Этот world29 evidence
не считается новым world30 cloud-прогоном; совместимость ограничена ADR-103.
Более поздний [publication checkpoint](../checkpoints/2026-10-04-publication.md)
записывает уже world30 release workspace, focused micro suite и повторный
10k soak после численного исправления. Это тоже прежняя приёмка,
а не утверждение о повторном запуске в данной сессии.

## Генетическая колония и пределы физики

| Требование SPEC / действующее решение | Статус | Проверка или отсутствующая часть |
|---|---|---|
| §2.5, §6.2; ADR-093: два популяционных локуса компилируются до hash | verified | В [config/genetics.rs](../../crates/liminis-core/src/config/genetics.rs): `materialisation_is_idempotent_and_canonical_reload_is_exact`, `every_top_level_genetics_number_changes_the_hash`, `a_mismatched_generated_collision_is_rejected`, `raw_templates_cannot_reference_generated_genotype_lanes`. Это четыре полосы популяций, не индивидуальные клетки. |
| §6.2; ADR-093: новые незаселённые сочетания только через однолокусное рождение | verified | `a_single_founder_creates_new_genotypes_only_through_single_locus_births` в [acceptance_genetic_colony.rs](../../crates/liminis-core/tests/acceptance_genetic_colony.rs); compiler control `zero_mutation_omits_zero_rate_branches`. |
| §2.5, §6.2; ADR-093/095: среда меняет преимущество, оборот создаёт пищу | partial | `inherited_speed_and_affinity_reverse_fitness_between_poor_and_rich_food`, `producer_turnover_supplies_detritus_that_changes_consumer_growth` в [acceptance_genetic_colony.rs](../../crates/liminis-core/tests/acceptance_genetic_colony.rs). Причинные проверки конечного набора; не evidence изменения уровня отбора. |
| §1, §10; ADR-094: физический инокулюм и измеряемый популяционный фронт | verified | `declared_founders_overwrite_the_closed_ball_on_their_own_lanes`, `same_substance_inocula_cannot_share_a_tolerant_boundary_voxel`, `the_genetic_colony_starts_with_one_founder_and_three_empty_genotypes` в [acceptance_inoculation.rs](../../crates/liminis-core/tests/acceptance_inoculation.rs); `the_colony_expands_a_measured_biomass_contour_and_replays_exactly` в [acceptance_genetic_colony.rs](../../crates/liminis-core/tests/acceptance_genetic_colony.rs). Фронт поля с явным порогом не является телом организма. |
| §4–5, §10; ADR-102: eco chemistry/transport и масштабная сходимость | partial | [acceptance_reactions.rs](../../crates/liminis-core/tests/acceptance_reactions.rs): `reaction_alone_conserves_each_element_exactly`, `competition_scaling_conserves_each_element_exactly`, `a_reacting_tick_closes_the_energy_ledger_with_no_channel`; [acceptance_diffusion.rs](../../crates/liminis-core/tests/acceptance_diffusion.rs): `diffusion_matches_analytic_gaussian_spread`. Именованных `mms_diffusion_converges_at_order_2`, `mms_advection_converges_at_order_2_on_monotone_data`, `integral_quantities_agree_between_dx_and_dx_half` из ACCEPTANCE в исполняемом корпусе исходного commit не найдено. |
| §0.3, §1, §6.1, §13 S1′; ADR-099/101: пространственная micro-физика | missing | Камера well-mixed: нет физических x/y/z, локальной chemistry, stationary micro solver, excluded volume, моторики или пространственного фронта отдельных клеток. Eco-фронт и display positions эту проверку не заменяют. |
| §4.7, §6.3; ADR-101: температура влияет на клеточную физиологию | missing | В камере изотермическая баня; `bath_heat` — накопленный приём энергии. Наличие eco-теста `temperature_from_enthalpy_round_trips` в [acceptance_temperature.rs](../../crates/liminis-core/tests/acceptance_temperature.rs) не доказывает temperature-response клетки. Температурный эффект в LAB не заявляется. |
| §6.2–6.5, §13 S2/S3: GRN, адгезия, морфогенез, существенные переходы | missing | В текущей камере таких механизмов нет. Наследование, поколения, мутации и смена долей не доказывают развитие многоклеточной формы, видообразование или открытую эволюцию. |
| §11, §13 S0; ADR-104/105: публичные записи и большой горизонт | partial | В [export_cell_replay.rs](../../crates/liminis/examples/export_cell_replay.rs): `recording_is_deterministic_exact_seed_and_never_invents_frames`, `source_binding_rejects_wrong_commit_and_modified_or_untracked_engine`; в [catalog.test.mjs](../../site/catalog.test.mjs): `rejects invented shared trajectory claims`. Один seed с разными горизонтами — одна траектория; 1M micro ticks не является миллионным S0 eco soak и полной приёмкой объёмного рендера. |

Исторические команды genetic release/10k находятся в
[плане колонии](../plans/2026-10-04-genetic-colony.md). Они не заменяют
повторную приёмку исправленной eco arithmetic world30. Задачи positions/transport
и 2D/3D принадлежат LIVE, clock/chunks — RECORDED; LAB не меняет их механизмы.

## LAB-1: отдельный bounded runner

Новый `crates/liminis/examples/compare_cell_experiments.rs` использует
public `MicroScenario → derive → MicroState → step`. Материализованный control
получает свой canonical config/hash до derive; runtime config после derive
не меняется молча. Существующие control tests выше иногда правят derived config
в тестовом fixture; это не является подходящим provenance для LAB artifact.

Проверка нулей в `MicroStepReport` подтверждает заявленные core residual.
Независимой проверкой считается только отдельный расчёт изменения state,
medium channels, reaction extents и лизиса с точным сравнением обеих сторон;
повторное чтение полей report такой свёрткой не является. Статус runner должен
описывать действительно выполненный способ проверки.

| Критерий LAB-1 | Статус при составлении карты | Evidence этапа |
|---|---|---|
| Hard budgets на run/batch/bytes/cells; refused/failed/censored, причина остановки | verified | `semantic_guards_seed_and_declared_cell_budget_are_not_bypassed`, `hard_caps_strict_manifest_and_bounded_writer_refuse_unsafe_inputs`, `refusal_and_unstarted_runs_are_preserved_under_batch_and_work_budgets`, `capacity_failure_retains_last_committed_state_and_charges_attempt`; актуальный результат — в checkpoint. |
| Независимый exact integer ledger каждый tick, включая release, lifecycle и summary | verified | `seed_max_repeat_has_identical_results_and_exact_every_tick_ledgers`, `corrupt_report_is_rejected_even_if_it_claims_zero_residuals`: runner отдельно сводит state/channel/extent/lysis и lifecycle после каждого core step. Debug/release: 16 tests PASS; команды — в checkpoint. |
| Canonical identity, seed без потери бит, units, commit/version/profile и provenance | verified | `canonical_hashes_ignore_comments_and_change_with_materialized_parameters`, `source_guard_rejects_short_wrong_dirty_or_untracked_evidence`; runtime compiler/profile не являются build attestation. CLI evidence — в checkpoint. |
| Deterministic repeat, mutation-off и starvation controls через материализованные scenarios | verified | `mutation_off_control_includes_fission_and_one_inherited_allele`, `food_starvation_control_is_materialized_and_extinction_closes_ledgers`; control меняется в MicroScenario до canonical/derive. Команды и результат — в checkpoint. |
| Paired seed × condition матрица, variability/range, инспектируемые результаты и UI | missing | LAB-2/3 — отдельные этапы после принятия LAB-1. Данные и интерфейс этим PR не выдаются за готовые. |

Для каждого опубликованного опыта необходимы проверенный commit, canonical
config/hash, seed, версии, dt и единицы, запрошенный и фактически проверенный
горизонт, lifecycle, stop reason и честный статус. Отказ в derivation,
численная ошибка и достижение исследовательского/resource budget сохраняются
раздельно. `dt` не увеличивается ради скорости, guards/ledger не отключаются.

## Воспроизведение runner

Из чистого checkout указанного commit создать manifest и запустить runner:

```sh
mkdir -p target/qa/lab
python3 - <<'PY'
import json, pathlib
source = pathlib.Path('configs/scenarios/cell-chamber.toml').read_text()
manifest = {'schema_version': 1, 'batch_id': 'baseline', 'runs': [{
    'run_id': 'baseline-42', 'condition': 'baseline', 'seed': '42',
    'steps': 1000, 'sample_every': 10, 'scenario_toml': source}]}
pathlib.Path('target/qa/lab/input.json').write_text(json.dumps(manifest))
PY
cargo run --locked --release -p liminis --example compare_cell_experiments -- \
  --manifest target/qa/lab/input.json --source-commit "$(git rev-parse HEAD)" \
  --output target/qa/lab/results.json
```

Manifest содержит материализованный TOML каждого опыта, точный seed строкой,
уникальный run_id, condition, steps и sample_every. Runner не правит сценарии
или derived config. Он сохраняет canonical TOML/hash, физические units/storage
scales, версии, source commit, observed compiler/profile и границы provenance.
Artifact — наблюдения агрегатов и digest state, не restart checkpoint.

Hard bounds: 64 runs, 1M steps/run, 2M admitted requested steps/batch,
100M attempted cell-ticks/batch, 4096 declared max_cells, 100 KiB TOML/run,
2 MiB manifest, 16 MiB result, до 201 samples/run. CLI разрешает только
снижение этих caps. Optional watchdogs — до 60s/run и 300s/batch, по умолчанию
выключены. Watchdog меняет stopping boundary: повторяемость обещается лишь
при одинаковой детерминированной границе, не для произвольного wall time.
Запрошенный горизонт означает censoring будущей истории популяции;
вымирание, capacity stop, отказ входа и numerical failure показываются отдельно.
Невыполненный/непроверенный tick не получает нулевой residual.
