# LAB-2: фактическая приёмка 24 опытов

Численный source — `21cbe90732128edee61963544e414d29a7420577`, принятый LAB-1,
world30/chamber1, Rust1.97.1/release/native FLOAT. Data commit и target PR —
отдельные идентичности, указанные в PR #15 и issue #11. Файлы опыта отсутствуют
в старом source; [методика](method.md) содержит воспроизведение через собственный
detached worktree и абсолютный путь к manifest из data checkout.

## Входы и результаты

Предрегистрация `9f49a2438be187b446556b1291b6264f0e3ea5c8` опубликована
2026-10-04 13:30:30 UTC до первого запуска. Независимый reviewer сравнил
method/manifest с этим Git object: байты не изменились. Это evidence процесса,
не криптографическая аттестация времени исполнения.

| Файл | Bytes | SHA-256 |
|---|---:|---|
| [manifest.json](manifest.json) | 82 880 | `f7a669548650754864a410a46e86593ab722116e0a01057e17f6efc84efcb2f3` |
| [method.md](method.md) | 15 371 | `ae35da91554e011344e1bd4971165ff866a13a254a837aa8c083691410458eb3` |
| [results.json](results.json) | 5 294 742 | `6fd944f6fe2e25ca5418ac357dcb97ce68620afee569b2ecd6dc684c22f7d37a` |
| [summary.csv](summary.csv) | 17 059 | `a8a6e8ae5356b42bb52ecc18d9b70869ca32df8b57ff9560e06ff6b01769fab0` |
| [comparisons.json](comparisons.json) | 1 116 928 | `e2faeda80c9f47ff23e5aff14879f3780627d867fae51aa655b77d4ad52a8ae3` |

Результат — неизменённый полный output bounded runner, без фильтрации или
компактного переписывания JSON. Второй actual release запуск с тем же manifest,
source, профилем, toolchain, платформой и stopping boundaries дал побайтно
одинаковый output всех 24 runs. BLAKE3 всего output:
`a58697006ef0256c1a81ad9c5ab2cdc6d0529a9da144e855125a938234aa6236`.

Серия выполнена полностью: 20 runs достигли requested tick20k, сохранив
finite-horizon status=censored/requested_horizon_reached; четыре starvation
runs вымерли на tick454 / 13 620s. Refusals, numerical failures и ранних
resource stops в этой фактической серии нет; соответствующие ветки результата
не удалены из runner/validator. Все 24 входят в planned denominator.

20×20 000 + 4×454 = **401 816 checked ticks**. Attempted и committed work
совпадают: **36 770 916 cell-ticks**. Все tick_reports_hashed равны checked_ticks.
Сохранено **4044 aggregate samples**: 20×201 + 4×6. Максимум наблюдённой
популяции279 ниже вычислительного guard512. Guard не является physical capacity.

## Описательные наблюдения

Seeds перечислены в фиксированном порядке **1, 7, 42, 2026**. Следующая таблица
использует только общий tick20k, а разности имеют направление condition−baseline.
Это четыре наблюдения и их диапазон, без статистического открытия.

| Condition | Living cells на20k | Парные разности | Min–max разностей |
|---|---|---|---|
| baseline | 92, 90, 91, 101 | — | — |
| mutation_off | 101, 98, 96, 100 | +9, +8, +5, −1 | −1…+9 |
| oxygen_low | 96, 96, 96, 92 | +4, +6, +5, −9 | −9…+6 |
| exchange_half | 47, 50, 51, 46 | −45, −40, −40, −55 | −55…−40 |
| founder_k2 | 93, 100, 94, 96 | +1, +10, +3, −5 | −5…+10 |

Starvation не подставляется в эту таблицу как состояние20k: последние
реальные observations — extinction454, нулевой growth extent/fissions и восемь
deaths в каждой траектории. Landmark100 имеет четыре пары; 500/1000/2000/5000/
10000/20000 — ноль. Последний общий sample с baseline — tick400; каждый такой
диагностический endpoint хранится отдельно. Carry-forward и interpolation нет.

Mutation-off сохраняет один исторический kinetics allele при реальных
977/967/981/978 fissions. Это содержательный контроль отсутствия мутаций,
а не доказательство большей эффективности такого режима.

Exchange-half имеет меньше клеток и fissions во всех четырёх парах. Парные
разности fissions: −543/−545/−529/−764. Однако paired structural BIO differences
имеют смешанные знаки: **меньше клеток не означает меньше биомассы**.

Founder-k2 имеет меньше fissions (разности −222/−212/−259/−351), а historical
maximum generation ниже на три во всех четырёх парах. Living differences смешаны.
Стартовый allele одновременно меняет speed и affinity; отдельные founder cultures
не являются прямым соревнованием alleles или доказанным преимуществом +2.

Итоговый cumulative growth extent равен **2 341 910 derived integer quanta**
во всех16 runs baseline/mutation_off/oxygen_low/founder_k2; у exchange_half —
**1 261 027** во всех четырёх. Общие matter scales позволяют сравнить эти integers.
Oxygen-low не называется подавлением суммарного роста на20k. Нулевой O2 наблюдается
в четырёх samples каждой из двух oxygen_low траекторий (seeds1/2026); у seeds7/42
нулевых сохранённых samples нет. Одинаковый конечный extent не доказывает отсутствие
промежуточного ограничения кислородом. FOOD остаётся объявленным Monod limiting
substance; O2 участвует в стехиометрическом ограничении extent.

Поколения и observed alleles подтверждают bounded inherited variation текущего
прототипа. Температурная кинетика, GRN, spatial evolution, видообразование,
statistical significance и открытая эволюция этими данными не установлены.

## Проверки и пределы evidence

Финальные [validate.py](validate.py) и [test_validate.py](test_validate.py):

- SHA256 validator `ae9171a6759ae49b58177fb6840bd8b2cde73ec303127c19c5facef6a1a32651`.
- SHA256 tests `22f22b37ae064721928ff7e647aa8cfd8f34663c7516fcea82d886b2f3496e89`.
- Root: **23 functional tests PASS**, 6.824s; final CLI PASS actual24.
- Strict schemas, source/baseline/prereg SHA anchors, treatment isolation,
  available canonical identities, raw-output alias protection и atomic writes.
- Corruption fixtures: pairing/missing rows, source/schema/parameters/falsehash,
  starvation, lifecycle/histogram, sampled energy и direct/symlink/hardlink aliases.
- Honest whole-condition refusals/unknown identities и missing landmarks сохраняются.

Независимый code review blockers0 на указанных frozen SHA. Независимый numerics
review сверил canonical/materialized inputs, prereg bytes и все4044 экспортированных
cumulative endpoints: lifecycle, allele composition, matter/energy и channels.
Финальный sampled energy audit validator использует formation enthalpies и
exported scales фиксированной матрицы, Fraction запрещает скрытое округление.
Paired integer differences, range widths и35 fixed/20 latest-common comparisons
сверены независимо.

Это разные уровни evidence: runner независимо проверяет оба integer ledger
**каждый принятый tick**, включая release; статический Python и независимый
sampled review проверяют доступные exports. Aggregate samples не позволяют
пересчитать скрытые промежуточные ticks, BLAKE3 hashes или восстановить restart
state. Native FLOAT не аттестуется между hardware; runtime rustc/profile не
доказывают сборку отдельно принесённого binary.

SourceLAB-1 exact-head CI/code/numerics/release приняты интегратором. Candidate
CI и integration acceptance текущего data PR фиксируются отдельно в PR/issue11.
Browser QA N/A для LAB-2 безUI; будущий LAB-3 требует реального protected browser
gate, screenshots и working controls, а не Node/stdout как замену QA.

После запуска source worktree извлечения можно воспроизвести без перезаписи
опубликованных данных (переменные paths — собственные каталоги из method.md):

```bash
python3 -B "$data_checkout/docs/experiments/lab-2/validate.py" \
  --manifest "$data_checkout/docs/experiments/lab-2/manifest.json" \
  --results "$source_checkout/target/lab-2/results.json" \
  --baseline-config "$source_checkout/configs/scenarios/cell-chamber.toml" \
  --output-dir "$source_checkout/target/lab-2/validated"
python3 -B -m unittest discover -s "$data_checkout/docs/experiments/lab-2" -p test_validate.py
```
