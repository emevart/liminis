# LAB-2: предрегистрация парного сравнения условий

Метод и [manifest.json](manifest.json) фиксируются отдельным remote commit
до первого матричного запуска. Это постановка опыта, без результатов и выводов.
Изменение метода после просмотра данных должно быть обозначено явно; расширение
матрицы требует фактически вызванного Astra и отдельного этапа.

Численный источник: `21cbe90732128edee61963544e414d29a7420577`, принятый
LAB-1 runner `crates/liminis/examples/compare_cell_experiments.rs`.
Исходный сценарий — `configs/scenarios/cell-chamber.toml` на этом commit,
world format 30, chamber format 1, Rust 1.97.1, release, native FLOAT.
Данный commit двигателя предшествует файлам LAB-2. Commit предрегистрации
и commit итоговых данных указываются отдельно в evidence PR; они не подменяют
source commit и не вписываются в собственные файлы как круговой self-hash.

SHA-256 точных байтов manifest:
`f7a669548650754864a410a46e86593ab722116e0a01057e17f6efc84efcb2f3`;
размер 82 880 bytes, 24 run declarations. Seed хранится десятичной строкой.
Каждый run содержит полный материализованный TOML. Имя `cell-chamber`,
вещественный реестр, генетическая программа и все неназванные параметры
остаются исходными. Condition — метка сравнения, а не подмена имени сценария.
Настоящие canonical config/hash вычисляет public parse/canonical/derive;
runtime config после derive не патчится.

## Матрица и бюджет

Порядок фиксирован: seed-major, seeds `1`, `7`, `42`, `2026`, а внутри каждого
seed следующие шесть условий в указанном порядке. Поэтому исчерпание batch
budget может влиять на доступность поздних runs; этот эффект показывается явно.

| Condition | Единственная декларативная правка относительно baseline | Единица и смысл |
|---|---|---|
| `baseline` | Нет | Исходная камера |
| `mutation_off` | `genome.mutation_probability = 0.0` | Вероятность мутации на рождении; остальные правила наследования сохраняются |
| `starvation` | `initial.concentration.FOOD = 0.0`, `medium.concentration.FOOD = 0.0` | mol/m³; питание отсутствует изначально и в резервуаре, запас энергии основателей сохраняется |
| `oxygen_low` | `initial.concentration.O2 = 0.05`, `medium.concentration.O2 = 0.05` | mol/m³; меньше кислорода в начальной среде и резервуаре |
| `exchange_half` | `chamber.medium_exchange_per_s = 1.0e-6` | s⁻¹; половина исходной независимо ограниченной скорости обмена pools |
| `founder_k2` | `founder.genome.kinetics = 2` | Безразмерный наследуемый kinetics allele основателей; последующая мутация остаётся включённой |

Для всех runs: 20 000 запрошенных steps, `dt = 30 s`, запрошенный горизонт
600 000 s модельного времени, sample every 100 ticks, не более 201 samples
включая genesis. Это горизонт исследования, а не обещание завершения каждого run.
Stock `chamber.max_cells = 512` — вычислительный safety guard, не вместимость
биологической среды. Достижение предела не лечится запретом деления или reset.

Runner сохраняет принятые жёсткие пределы LAB-1:

| Предел | Значение |
|---|---:|
| Число runs | 64; в матрице 24 |
| Steps на run | 1 000 000; запрошено 20 000 |
| Сумма admitted requested steps | 2 000 000; запрошено 480 000 |
| Runner admission max_cells | 4096; декларация каждого сценария 512 |
| Batch attempted cell-ticks | 100 000 000 |
| Итоговый JSON | 16 MiB |
| Manifest / отдельный TOML | 2 MiB / 100 KiB |
| Samples на run | 201 |
| Run/batch wall-clock watchdog | 0 / 0, отключены |

Attempted cell-ticks считают число живых объектов перед попыткой tick;
неуспешная попытка тоже расходует бюджет. Committed cell-ticks и checked ticks
считаются отдельно. 480 000 requested steps не доказывают, что лимит cell-work
не будет исчерпан. Не увеличиваются `dt`, caps или output budget ради удобного
результата; не отключаются numerical guards и оба exact integer ledger.

## Сравнения, объявленные до данных

Единица парного сравнения — один seed и одно условие против baseline того же
seed. Одинаковый seed задаёт воспроизводимую пару исходных постановок;
после разных событий деления это не синхронизированная родословная, не одни
и те же потомки и не 24 независимые биологические реплики. Отдельно показываются
четыре seed, без выбора наиболее красивого результата.

Основные landmarks: ticks `100`, `500`, `1000`, `2000`, `5000`, `10000`,
`20000` — соответственно 3 000, 15 000, 30 000, 60 000, 150 000, 300 000,
600 000 s. Разница всегда `condition − baseline` только на точном одинаковом
sample tick. Нет интерполяции, переноса последнего значения или продолжения
вымершего состояния за последний экспортированный tick.

Показатели: число живых клеток, структурная масса, накопленные fissions/deaths,
состав allele histogram, observed allele richness, максимальное наблюдённое
поколение, свободные FOOD/O2 и накопленный growth extent. Число клеток,
lifecycle, histogram и accounting integers сравниваются как целые, без float
округления. Для разности массовых units сначала требуется равенство соответствующих
`matter_scales`; физические mol/J в summaries являются display-производными,
а не основанием точного ledger. Observed richness включает ранее исчезнувшие
аллели; размер текущего histogram отражает только живые варианты.

Для каждого condition × landmark ожидается **4 пары**. Показываются available
count, отсутствующие seed и причина отсутствия: refusal, numerical failure,
extinction до landmark, censor/resource stop либо отсутствующий sample.
Range — min/max и их разность по доступным paired differences на одном landmark;
при отсутствии пар значения неизвестны, а не равны нулю. Меньшее число пар
сохраняется рядом с range. Не объединяются различающиеся last-common ticks.
`comparisons.json` может дополнительно показывать последний общий sample
каждой пары с явным tick; это диагностический срез, не общий endpoint матрицы.

Для всех 24 declarations итоговая строка содержит requested steps, checked
ticks, status, stop reason, attempted tick/work, ошибку при наличии, lifecycle
и фактический горизонт. Refused results не исключаются из таблиц. Финальные
состояния на разных горизонтах не сравниваются как полные 20k результаты.
В терминологии runner `censored` с `requested_horizon_reached` означает
достижение конечного исследовательского горизонта; `censored` с safety/budget
reason означает раннюю остановку. `extinct`, `refused`, `numerical_failure`
сохраняются раздельно; итоговый ledger нулевого tick неизвестен.

## Контроли и границы интерпретации

`mutation_off` проверяет отсутствие новых kinetics alleles и сохранение
founder allele 0 в наблюдённых живых клетках. Наследование через деление
содержательно проверяется лишь при реально наблюдённых fissions. Summary
не экспортирует полную физиологию каждой клетки; полное genome inheritance
дополнительно покрывают принятые tests LAB-1/core, а не histogram сама по себе.

`starvation` проверяет отсутствие FOOD-driven роста и наблюдает расход
сохранённого внутреннего резерва, starvation tolerance и возможный лизис.
Основателям не обнуляется энергия и не вводится искусственная мгновенная смерть.
В обоих контролях отсутствие подходящего события обозначается пределом опыта.

Кинетический локус оплачивает ускорение потерей affinity: stock factors 1.35
для скорости и 1.8 для Km на allele step. `founder_k2` меняет наследуемую
физиологию основателя. У роста declared limiting substance — FOOD; O2 входит
в stoichiometric inputs и ограничивает доступный общий extent, но отдельного
O2-Monod множителя здесь нет. Обмен приближает каждый free pool к его target
с собственным cap; это не связанный fixed-volume chemostat flow и не вымывание
клеток. DET возвращается при смерти, но этим сценарием не вводится новый
пищевой путь, GRN, адгезия, пространственный solver или новый genome engine.

Камера well-mixed, изотермическая. Температурных эффектов кинетики,
пространственных координат и gradient selection метод не предполагает.
Поколения, мутации, turnover и различия между seed не доказывают открытую
эволюцию, статистическое открытие, появление вида или «победителя».
Выводы ограничиваются наблюдённой траекторией, controls и указанным горизонтом.

## Воспроизведение и проверяемый provenance

Нужны checkout итогового data PR и отдельный собственный clean worktree
численного source. Manifest берётся абсолютным путём из data checkout,
поскольку численный source commit не содержит файлов LAB-2. Пример команд:

```bash
data_checkout="/absolute/path/to/data-pr-checkout"
source_checkout="/absolute/path/to/lab-2-source"
git -C "$data_checkout" worktree add --detach "$source_checkout" 21cbe90732128edee61963544e414d29a7420577
mkdir -p "$source_checkout/target/lab-2"
cd "$source_checkout"
cargo run --locked --release -p liminis --example compare_cell_experiments -- \
  --manifest "$data_checkout/docs/experiments/lab-2/manifest.json" \
  --source-commit 21cbe90732128edee61963544e414d29a7420577 \
  --max-run-seconds 0 --max-batch-seconds 0 \
  --output "$source_checkout/target/lab-2/results.json"
```

Выход находится в ignored `target/`, не переписывает исходные данные PR.
Повтор выполняется той же командой в том же source/worktree/profile/toolchain
и на той же платформе с другим output filename. Сравниваются точные JSON
bytes, final-state/report digests, checked ticks и stop reasons. Не требуется
различие digests разных seeds: некоторые controls могут дать одинаковое state
или одинаковые reports. Native FLOAT не аттестуется побитно между hardware.
Runtime `rustc` и release profile — наблюдения provenance, не криптографическая
аттестация самостоятельного исполняемого файла.

Опубликованные [results.json](results.json), [summary.csv](summary.csv),
[comparisons.json](comparisons.json) и [validate.py](validate.py) должны сохранять
все попытки и команды получения. Python validator проверяет доступные exports,
структуру, pairing, summaries, целые counters и фиксированные landmarks.
Он не может независимо пересчитать ledger неэкспортированных ticks,
восстановить полный state или пересчитать BLAKE3 config/state/report hashes
средствами стандартной библиотеки. Поэтому static validation, release tests
runner с независимой свёрткой каждого tick и точный повтор запуска — разные
части evidence; ни одна не выдаётся за остальные. Digest точных файлов,
remote preregistration commit, tests/review и фактические команды сохраняются
в checkpoint/evidence этапа после выполнения, без преждевременного PASS.
