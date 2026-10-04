# LAB-2: сохранение парной матрицы chamber1

- Дата: 2026-10-04. Цель: отдельные данные, методика и приёмка существующей
  модели; затем сравнительный LAB-3 после принятия LAB-2.
- Ветка: `codex/lab-paired-cell-matrix-1`; численный source/base
  `21cbe90732128edee61963544e414d29a7420577`, world30/chamber1,
  Rust1.97.1/release/native FLOAT. Это принятый LAB-1 из PR #14.
- LAB-1 ACCEPTED (code+CI), queued, без самостоятельного merge:
  https://github.com/emevart/liminis/issues/11#issuecomment-5980403029 .
- Владелец интеграции остаётся основная CLOUD LIVE-сессия. Возможная миграция
  координатора человеком не выполнена. Автор LAB не выполняет merge/reset/clean.
- Прямое уточнение человека: сохранить scoped code/docs и законченные публичные
  scientific artifacts перед возможной передачей, затем продолжать очередь.
- Владение LAB-2: только новые docs/experiments/lab-2/* и этот checkpoint.
  Engine, configs, formats, replay, site, shared/frozen paths не меняются.
- Среда: generic Work Cloud VM; ПК не является зависимостью. Новые интеграции,
  секреты, платный API и изменение инфраструктуры не используются.
- Full remote HEAD/tree и PR фиксируются в issue #11 после публикации;
  этот файл не содержит круговой ссылки на собственный commit.
- Draft PR #15: https://github.com/emevart/liminis/pull/15 . Base — LAB-1 branch
  для отдельного data diff; после интеграции #14 целевая база main.
- Safe snapshot `ff402c988b5d4db79566b2c43d621d0b99892613` сохранил все результаты
  и scoped drafts перед продолжением final guards. Файлы не потеряны.

## Предрегистрация и законченные результаты

Remote preregistration `9f49a2438be187b446556b1291b6264f0e3ea5c8`
создан до первого запуска: 2026-10-04 13:30:30 UTC. Содержит только manifest
и метод; оба файла после запуска побайтно совпадают с этим commit.

- manifest.json: 82 880 bytes, SHA256
  `f7a669548650754864a410a46e86593ab722116e0a01057e17f6efc84efcb2f3`.
- method.md: 15 371 bytes, SHA256
  `ae35da91554e011344e1bd4971165ff866a13a254a837aa8c083691410458eb3`.
- results.json: 5 294 742 bytes, SHA256
  `6fd944f6fe2e25ca5418ac357dcb97ce68620afee569b2ecd6dc684c22f7d37a`;
  BLAKE3 `a58697006ef0256c1a81ad9c5ab2cdc6d0529a9da144e855125a938234aa6236`.
- Результат уже загружен как Git blob
  `65df63d0331c00203f216b4561ac32b1e2322ed0`, совпавший с локальным git hash-object;
  snapshot commit закрепляет его в собственной ветке.

Матрица закончена полностью: seeds1/7/42/2026, по шесть заранее объявленных
условий, 20k requested steps, dt30s, sample every100, stock max_cells512.
Все 24 декларации сохранены. 20 trajectories достигли горизонта20k и имеют
status=censored/requested_horizon_reached; четыре starvation controls имеют
extinct/extinction на tick454 (13 620s). Refused, numerical failure и ранних
resource stops в этой фактической серии нет. Это не доказательство их отсутствия
в других постановках. Watchdogs отключены, guards/ledger сохранены.

Actual release CLI выполнен дважды. Весь второй JSON побайтно совпал с первым,
включая source/provenance, stop reasons, summaries и digests всех 24 rows.
Проверено 401 816 ticks; attempted=committed=36 770 916 cell-ticks.
Все tick_reports_hashed равны checked_ticks. Максимум популяции279 ниже guard512;
guard не трактуется как биологическая вместимость.

Единицы сохраняются в runtime identity: mol/m3, mol structural biomass, J, s, K;
точные quantities/counters/scales — decimal strings. Материализованные сценарии
получили штатный canonical/config hash, canonical TOML равен входной постановке.
Разные условия не изменяют derived config после derive.

Mutation-off действительно включает 977/967/981/978 fissions и один исторический
kinetics allele. Starvation имеет нулевой growth extent/fissions и восемь deaths.
Не создаются 20k samples после extinction; последний общий sample с baseline
для этой пары — tick400, отдельно от основного фиксированного landmark100.

## Проверки и открытые gates этой точки сохранения

PASS:

- Обе actual bounded CLI матрицы и побайтный full24 repeat.
- Root semantic diff всех 24 materialized inputs: ровно объявленные изменения.
- Независимый numerics review сверил все 4044 экспортированных samples:
  lifecycle/histogram/counters и sampled cumulative matter/energy ledger exact.
  Это аудит экспортов; полную проверку неэкспортированных ticks выполняет runner.
- Независимо сверены preregistration bytes и source stock через Git.
- Текущий Python validator принимает actual24; root создал summary.csv и
  comparisons.json (35 fixed landmarks, 20 latest-common diagnostics).
- Финальные 23 functional Python tests PASS у root (6.824s), final CLI PASS actual24.
- Независимые code и numerics review blockers0 на frozen validator SHA256
  `ae9171a6759ae49b58177fb6840bd8b2cde73ec303127c19c5facef6a1a32651` и tests SHA256
  `22f22b37ae064721928ff7e647aa8cfd8f34663c7516fcea82d886b2f3496e89`.
- CLI SHA anchors frozen baseline/preregistered manifest, strict schemas,
  treatment/canonical consistency, starvation control, output alias safeguards
  и corruption fixtures завершены. Unknown identities у refusals сохранены.
- Sampled cumulative energy audit использует source formation enthalpies,
  exported scales и Fraction; не реконструирует скрытые промежуточные ticks.
- Source LAB-1: 16 runner tests debug/release, code/numerics review и exact-head
  CI приняты интегратором. Это source evidence, не CI нового LAB-2 head.

Открытые gates после завершения реализации:

- Candidate CI и явный ACCEPTED/MERGED финального exact head LAB-2 у интегратора.
  Snapshot CI не подменяет CI следующего commit.
- Последовательный merge LIVE→RECORDED→LAB остаётся у soleowner.
- Browser QA N/A: этот этап не содержит UI. LAB-3 потребует реального browser gate.

Итоговая приёмка/commands/наблюдения/ограничения —
[validation.md](../experiments/lab-2/validation.md). Итоговый comparisons.json:
1 116 928 bytes, SHA256
`e2faeda80c9f47ff23e5aff14879f3780627d867fae51aa655b77d4ad52a8ae3`.
Summary.csv: 17 059 bytes, SHA256
`a8a6e8ae5356b42bb52ecc18d9b70869ca32df8b57ff9560e06ff6b01769fab0`.

Python static validation не пересчитывает BLAKE3 и не восстанавливает полный
state или ledger stream из aggregate samples. Raw file SHA256, структурный аудит,
независимый sampled numerical review и actual repeat разделены явно.
Native FLOAT не аттестуется между hardware; runtime rustc не является build attestation.

## Storage и следующая очередь

Законченный оригинальный scientific artifact находится в assigned
docs/experiments/lab-2/results.json и закрепляется snapshot commit в GitHub.
Manifest/method, scoped Python validator/tests и извлечённые summary/comparisons
сохраняются в той же собственной ветке, отдельно от LAB-1.

Ignored target/qa/lab-2/repeat-results.json остаётся локальным duplicate evidence:
его bytes равны опубликованному results.json, отдельно в Git не stage.
Оngoing batch отсутствует; private artifacts нет. Git-ignored build/QA cache
не stage автоматически. Незавершённой simulation или unsaved original data нет.
Открыты только candidate CI/integration gates, перечисленные выше.

Фактически вызван Astra до метода и после результатов: рекомендует зафиксировать
эти 24 опыта, завершить review/приёмку и перейти к LAB-3 после ACCEPTED.
Нет статистического discovery, победителя allele или доказанной открытой эволюции.
Число клеток и BIO — разные показатели; exchange-half BIO differences имеют
смешанные знаки. Итоговый extent одинаков у baseline/mutation_off/oxygen_low/
founder_k2, поэтому низкий O2 не называется подавлением суммарного20k роста.

Следующий шаг: опубликовать финальный exact head в явно dependent PR #15,
проверить candidate CI и получить интеграционную приёмку без собственного merge.
После его ACCEPTED/MERGED — отдельные site/lab* и site/data/lab/**, actual browser
QA через existing Actions у интегратора при cloud sandbox gap. Автоматический
новый turn/create_goal недоступен; активное cloud продолжение не зависит от ПК.
