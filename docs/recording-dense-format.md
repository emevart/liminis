# Плотная lossless-запись архивной клеточной камеры

Этот формат сохраняет наблюдение каждого завершённого тика существующего
Rust-движка: от начального состояния `0` до заявленного горизонта включительно.
`sample_every = 1`; экспортёр не выбирает более редкую сетку при нехватке места.
Здесь описан код предварительного экспорта REC2, а не уже выполненный новый
эксперимент или опубликованный набор данных. Сборку, реальный pilot и сравнение
с архивом выполняет координатор после отдельной проверки кода.

Граница модели следует ADR-104: well-mixed камера без физических координат,
с ограниченной наследуемой кинетикой и некалиброванной прототипной химией.
Наблюдения не являются checkpoint: из них нельзя возобновить внутреннее
состояние движка или восстановить события внутри тика. Они не доказывают
приёмку S2 или неограниченную эволюцию. Сжатие и декодирование не исполняют
биологию, не интерполируют и не вычисляют заново физические float-поля.
Прежний экспортёр и его предел `2001` кадров сохраняются без изменений.

## Источник и происхождение

Официальный producer — `crates/liminis/examples/export_dense_cell_replay.rs`,
добавленный в чистый checkout архивной базы. Его `provenance.producer_commit`
и `producer_tree` обозначают фактический новый HEAD и дерево producer.
`identity.source_commit` и `provenance.engine_ref` обозначают архивный движок
`b2024cef08f0aa910a711a41c6a37fbc7b2ba35a`.
`legacy_recording_ref = 43f4a2df25c356ff387b71ac1961b516975f999b` обозначает
источник прежних записей. Эти три роли не подменяют друг друга.

До первой записи в stdout producer проверяет полный переданный SHA против
Git HEAD, чистоту всего checkout, совпадение Git root с каталогом, из которого
собран пример, `WORLD_FORMAT_VERSION = 30` и равенство следующих Git objects
в HEAD, `engine_ref` и `legacy_recording_ref`:

| Путь | Закреплённый Git object |
|---|---|
| `crates/liminis-core` | `bd2a0a0d55dcfff052e5573ddfc863f4d918d489` |
| `configs/scenarios/cell-chamber.toml` | `896f3415f24676e35e36431b20a0f9f304fb2259` |
| `Cargo.toml` | `4b7bef5047fafbdd10526a90de4a18952678a6a6` |
| `Cargo.lock` | `224f065c1bbf103a5cfec139a2f3313e2bf80559` |
| `rust-toolchain.toml` | `1fd6f1afaf9c17073863c2b9a9f50f90572a7edb` |
| `crates/liminis/Cargo.toml` | `d234294cf3b92e2ee1979017d5d2f212c82abace` |
| `crates/liminis/examples/export_cell_replay.rs` | `ebfed98f122b9a05b5c7d95ef70d9f9f81045c27` |

Идентичность опыта: seed `"42"`, world `30`, chamber `1`, dt `30` секунд,
scenario `cell-chamber`, config hash `blake3:1802c0f129855749`. SHA-256 UTF-8
байтов canonical config —
`4e72889e4cc2857a7f259a359b8a9bda349861f8bb72d6c9dd74fe6511ca03de`.
Packer проверяет эти якоря независимо от заявлений producer. Мир `31` не
принимается и не переименовывается в `30`.

Официальная команда использует закреплённый Rust `1.97.1`, `--locked` и
`--release`. Metadata содержит наблюдаемую версию `rustc`, OS/architecture и
profile. `build_attestation = false`: runtime-проверка исходного checkout не
аттестует произвольный standalone binary. Команда сборки и SHA-256 итогового
`index.json` должны сохраняться отдельно как evidence настоящего запуска.

## Поток producer и пределы

Producer выдаёт UTF-8 JSONL в stdout: один `header`, затем ровно `H + 1`
records `frame` для тиков `0..H`, затем один `end` и EOF. `H` допустим от `1`
до `1000000`. Каждая входная строка ограничена `4 MiB`. Rust хранит только
текущее состояние, counters и накопленный словарь наблюдавшихся генотипов.
Полный JSONL не надо сохранять промежуточным файлом: pipe передаёт его packer
с обычным обратным давлением.

Перед наблюдением каждого тика `1..H` producer вызывает настоящий
`micro::step`, проверяет номер границы, длину вектора matter residual,
нулевую невязку каждого вещества и отдельный нулевой energy residual.
Все накопленные целые каналы и lifecycle counters складываются с проверкой
переполнения. Footer содержит `checked_ticks = H`, `frames = H + 1` и оба
максимальных residual строкой `"0"`. Нулевой кадр имеет `residual = null`:
переход до genesis не исполнялся. Packer требует полный footer и EOF.

Packer `scripts/dense-recording-pack.py` использует только Python stdlib.
Он сохраняет предыдущий и текущий frame, ограниченный gzip buffer, словарь
генотипов и descriptors chunks. Ни raw JSONL на миллион тиков, ни массив всех
frames не строятся. Все преобразования только копируют retained values.

| Ограничение | Значение |
|---|---|
| Records состояний в одном chunk | не более `256` |
| Распакованные байты JSONL payload одного chunk | не более `4 MiB` |
| Полный gzip file одного chunk | не более `1 MiB` |
| Живых клеток в одном frame | не более `512`, архивный admission |
| Chunks во всём наборе | не более `8192` |
| Полный pilot при `H <= 10000` | не более `64 MiB` |
| Полный набор при большем `H` | hard cap `512 MiB` |
| Инженерная цель большого набора | `256 MiB`, пока не измерена |

`4 MiB` относится к inflated keyframe/delta payload, а не к сумме всех
полностью восстановленных снимков chunk. Decoder хранит только ограниченный
payload и текущее состояние; consumer должен также ограничить кеш состояний.
Provider limits этим документом не подтверждены:
`provider_limits_verified = false`. `--max-total-bytes` может уменьшить cap;
увеличить соответствующий hard cap им нельзя. В полный размер входят все
уникальные gzip files, manifests и index.

Chunk закрывается до нарушения любого лимита. Не поместившийся следующий
тик начинает новый chunk с полным keyframe того же тика. Если даже один
keyframe не помещается, запись завершается ошибкой. Смена cadence, потеря
клеток, quantization и обрезание хвоста не являются допустимым выходом.
Packer записывает во временный соседний каталог и переименовывает его в
итоговый только после полной проверки потока и budgets. При ошибке финальный
набор не появляется; существующий output directory не перезаписывается.
Это атомарная видимость каталога, а не гарантия `fsync` или crash durability.

## Файлы, индекс и общие префиксы

Codec identifier: `dense-cell-jsonl-mask-v1+gzip`, schema `1`.
Набор состоит из `index.json`, `horizon-H.json` и файлов
`chunk-FFFFFFF-LLLLLLL.jsonl.gz`, где `F/L` — включительные номера первого и
последнего тика с семью десятичными цифрами. Chunk содержит один gzip member,
mtime `0`, пустое filename, OS byte `255`, deflate level `9`.
Версии Python/zlib записаны в manifest; байтовая идентичность gzip между
разными версиями zlib не обещается.

Descriptor каждого chunk содержит `path`, `first_tick`, `last_tick`,
`frames`, `gzip_bytes`, `decoded_bytes`, `gzip_sha256`, `decoded_sha256`.
Оба SHA-256 относятся к точным file/payload bytes, включая завершающие LF.
Ranges смежные, не перекрываются и покрывают `0..H`; `frames = L - F + 1`.
Decoder проверяет все descriptors перед seek и digest/length payload каждого
фактически прочитанного chunk. Абсолютные пути и `..` не допускаются.

`index.json` связывает identity/provenance с manifest descriptors: `path`,
`bytes`, `sha256`, `first_tick`, `last_tick`, `frames`. Он также содержит
число уникальных chunks и суммы их gzip/decoded bytes. Digest самого index
выдаётся в итоговом stdout report packer и должен закрепляться снаружи.
Внутренние digests обнаруживают порчу и рассогласование; без внешнего якоря
они не удостоверяют происхождение набора, который кто-либо переписал целиком.

При единственном настоящем прогоне `H = 1000000` packer создаёт три manifests
для `0..10000`, `0..100000`, `0..1000000`. Chunk принудительно закрывается на
тиках `10000` и `100000`; следующий начинается соответственно на `10001` или
`100001`. Меньший manifest ссылается на тот же точный prefix descriptors и
те же физические files, а не на другую траекторию. Его genome dictionary
содержит только генотипы, уже наблюдавшиеся к его горизонту. Для pilot или
другого разрешённого `H` packer создаёт manifest `H` и стандартные меньшие
горизонты, которые уже достигнуты. Оборванный миллионный поток не считается
успешным pilot: у него нет согласованного footer `H = 10000`.

Manifest сохраняет identity, provenance, canonical config, model и limitations
из header; добавляет experiment counts, bounds, packer versions, полный
словарь observed genomes своего префикса и descriptors chunks. Словарь может
включать кратковременно жившие генотипы, отсутствующие в прежней редкой сетке.
Добавление таких записей не меняет определений ранее известных генотипов.

## Keyframe и delta

Первая строка каждого независимо читаемого chunk — keyframe:

```json
{"type":"keyframe","schema_version":1,"frame":{},"definitions":[],"values":[],"genomes":{}}
```

Здесь `{}` для `frame` — обозначение структуры, не допустимый пустой frame.
Он содержит все прежние поля frame, кроме `cells`: `tick`, `sim_time`,
`summary`, `residual`, `accounting`, `resources`. Массивы `definitions` и
`values` имеют одинаковую длину и соответствуют клеткам в точном исходном
порядке. Нулевая клеточная популяция допустима. `genomes` — накопленный к
этому тику словарь с теми же `genome`/`phenotype` fields, что в прежнем JSON.

Порядок полей одной definition row:

| Индекс | Поле | Хранение |
|---|---|---|
| 0 | `id` | каноническая десятичная строка `u64` |
| 1 | `parent_id` | такая же строка или `null` |
| 2 | `generation` | точное неотрицательное JSON number |
| 3 | `birth_tick` | точное неотрицательное JSON number |
| 4 | `genome_key` | строка, ссылающаяся на `genomes` |
| 5 | `division_mass_mol` | сохранённое конечное binary64 значение |

Порядок одной values row и соответствующие bits mask:

| Индекс/bit | Поле | Хранение |
|---|---|---|
| 0 | `age_s` | сохранённое binary64 |
| 1 | `mass_mol` | сохранённое binary64 |
| 2 | `energy_j` | сохранённое binary64 |
| 3 | `mass_units` | каноническая десятичная строка `i128` |
| 4 | `energy_units` | каноническая десятичная строка `i128` |
| 5 | `starvation_s` | сохранённое binary64 |

Следующая строка описывает ровно следующий тик, без пропусков:

```json
{"type":"delta","set":{},"born":[],"removed":[],"changed":[],"genomes":{}}
```

- `set` заменяет целиком изменившиеся top-level fields frame, кроме `cells`.
  Accounting, resources и summary сохраняются непосредственно из producer.
  Незаявленные fields сохраняют предыдущее значение; из tick/units другие
  поля не выводятся.
- `removed` перечисляет существующие string IDs, исчезнувшие на границе.
  Сам этот список не классифицирует исчезновение как смерть или деление.
- `changed` содержит rows `[id, mask, values]` для сохранившихся клеток.
  `mask` — целое от `1` до `63`; values содержит ровно `popcount(mask)`
  элементов в возрастающем порядке bits из таблицы выше. Маска `0`,
  неизвестные IDs, дубли изменений или лишние values запрещены.
- `born` содержит rows `[definition_row, values_row]` для новых IDs.
  Definitions одного ID неизменны. Официальный producer выделяет IDs
  монотонно; packer отвергает возвращение ранее исчезнувшего ID, включая
  новую границу chunk.
- `genomes` содержит только новые definitions. Изменение, удаление или
  повторное определение известного genotype key запрещено.
- Необязательный `order` перечисляет все текущие IDs ровно один раз в
  исходном порядке. Он обязателен при изменении порядка/состава; при его
  отсутствии сохраняется прежний порядок. После удаления и рождения
  множество IDs должно точно совпадать с `order`.

Decoder применяет новые genomes, удаления, изменения, рождения и порядок;
затем заменяет `set`, проверяет весь полученный frame и ожидаемый tick.
Каждый chunk начинает собственные definitions и не зависит от предыдущего
chunk, включая генотипы родителей, уже отсутствующих в живой популяции.

## Точность и независимый decoder

IDs никогда не превращаются в JavaScript Number. Канонические строки не
имеют `+`, ведущих нулей или `-0`; ID находится в диапазоне `u64`, quantities
и accounting — в диапазоне `i128`. Для них допустимы string comparison и
BigInt, если consumer действительно нуждается в арифметике. Остальные целые
JSON numbers явно ограничены `0..2^53-1`, без сужения до `u32`. При выходе за
границу формат отказывается от записи. Signed kinetics остаётся точным `i8`
исходного архивного schema; новое биологическое ограничение не вводится.

Конечные float-поля retained как исходные binary64 values. Выбор changed
fields сравнивает биты, включая знак нуля: `+0.0` и `-0.0` различаются.
JSON decimal round-trip сохраняет эти binary64; NaN, Infinity и переполнение
при parse отвергаются. Python codec также различает integer и float tokens.
Независимый JS decoder должен сохранять `-0` (`Object.is`), не округлять и
не пересчитывать display values. Повторный `JSON.stringify` превращает `-0`
в `0`, поэтому он не является проверкой сохранности знака. Equality чисел
при приёмке сравнивается по binary64 bits, а exact strings — побуквенно.
Дубли JSON keys и неизвестные/отсутствующие schema fields запрещены; обычный
`JSON.parse` без отдельной проверки duplicate keys сам по себе этого не
гарантирует.

Совпадение с архивом означает совпадение всех значений каждого прежнего frame
на том же tick и всех ранее известных genome definitions. Оно не означает
байтовую идентичность целого прежнего JSON file: здесь другой metadata,
cadence и контейнер. Новые transient genotype definitions разрешены.
`sim_time == tick * 30` проверяется как admission, но decoder берёт значение
из retained payload, а не реконструирует его умножением.

Python reference decoder `scripts/dense-recording-decode.py` проверяет index,
manifest, ranges, lengths, SHA-256, один gzip member, bounds, masks, порядок,
IDs и schema. Он читает только chunks, пересекающие запрошенный диапазон,
и выдаёт по одному frame в JSONL. На стороне браузера достаточно загрузить
index и выбранный manifest, проверить внешне закреплённый index SHA, найти
chunk по включительному tick range, проверить/inflate его и применить не
более `255` delta records после keyframe. Держать весь миллион состояний
в памяти не требуется. Consumer обязан отклонять truncated payload,
trailing bytes, дополнительные gzip members и oversize до выдачи состояний.

## Команды и граница приёмки

Из чистого producer checkout координатор сначала выполняет bounded `100`
тиков; пример команды, не свидетельство её выполнения:

```bash
set -o pipefail
producer_ref="$(git rev-parse HEAD)"
cargo run --locked --release -p liminis --example export_dense_cell_replay -- \
  --producer-commit "$producer_ref" --steps 100 \
  | python3 -B scripts/dense-recording-pack.py \
      --output-dir target/dense/pilot-100 --max-total-bytes 67108864
```

После успешного source guard, сборки, independent review, ledger checks и
сравнения архивных общих кадров тот же producer HEAD допускается к `10000`
тикам с тем же cap `64 MiB`. До решения о `100000/1000000` фиксируются bytes,
chunk count, максимальные decoded/gzip sizes, elapsed time и peak memory
pilot. Два отдельно выполненных pilot не выдаются за общий миллионный
прогон. Большой прогон создаёт свои общие префиксы описанным выше способом.

Проверить сохранённый chunk диапазоном, не загружая весь опыт:

```bash
python3 -B scripts/dense-recording-decode.py target/dense/pilot-100/horizon-100.json \
  --first-tick 50 --last-tick 50
python3 -B scripts/dense-recording-tests.py
```

Codec tests используют все `201` уже записанных frames короткого архива и
синтетические последовательности значений: exact round-trip genome table,
binary64 edges/negative zero, большие integer strings, рождения/удаления,
новые генотипы, reordering, adaptive chunks, shared prefix, seek, порчу,
ошибки masks/IDs/descriptor/index, незавершённый поток и превышение budgets.
Синтетические records не являются новыми модельными тиками. Отдельный static
guard сравнивает функцию построения frame с прежним экспортёром без изменений.

Author preflight не запускает Cargo или модель. Финальная приёмка требует
от координатора реальной сборки exact clean producer HEAD, проверки source
guard до записи, обоих ledger residual на всех реальных тиках, измеренного
pilot и независимого сравнения всех общих архивных frames/known genomes.
Python codec round-trip сам по себе этих требований не закрывает. Публикация,
поддержка browser decoder и provider budgets находятся вне этого preflight.
