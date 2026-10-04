# Записанный опыт

`cell-chamber-seed-42.json` создан существующим Rust micro engine, не взят
из работающего локального сервера. Это наблюдения, не checkpoint.

Проверенный commit движка и экспортёра: `43f4a2df25c356ff387b71ac1961b516975f999b`.
Численное ядро и конфигурация совпадают с parent `5ffb313` без изменений.
World format: `30`, chamber format: `1`, seed: `42`, config hash:
`blake3:1802c0f129855749`.

```powershell
cargo run --release -p liminis --example export_cell_replay -- --config configs/scenarios/cell-chamber.toml --seed 42 --steps 10000 --sample-every 50 --source-commit 43f4a2df25c356ff387b71ac1961b516975f999b --output site/data/cell-chamber-seed-42.json
```

Команда выполнена дважды до commit сайта. Оба запуска дали 201 кадр,
10000 проверенных переходов, 6047570 байт и одинаковый BLAKE3:
`d033dd8f012997fd050feb9f3ef7d9f41946b15b4decdabf30a1091037b1468a`.
SHA-256: `6d17cac2d95fe9d3d007866d7cf72ddf14e945bbe4c060591f961f305656a1f1`.
Экспортёр отказывается от несовпадающего HEAD, изменённого engine/exporter/Cargo/toolchain
или их untracked sources. Это проверка исходников, не аттестация произвольного
standalone binary; используйте сборку через `cargo run`.

После изменения HEAD при новом экспорте указывается его полный SHA из
`git rev-parse HEAD`. Metadata и digest всего файла тогда изменятся, даже если
движок не изменён. Динамическую воспроизводимость проверяют сравнением `frames`
и canonical config при одинаковых engine semantics, seed и конфигурации.

В финальном кадре: 94 живые клетки, 1044 рождённых дочерних клетки,
522 деления, 436 смертей, максимальное поколение 9. Оба целых баланса замкнуты
на каждом переходе. Это не доказывает биологическую калибровку или бессрочную жизнь.

Структура JSON:

- `identity`: seed строкой, конфигурация/hash и версии/commit.
- `canonical_config`: полный воспроизводимый сценарий.
- `genomes`: общая таблица наследуемой физиологии и декодированных признаков.
- `frames`: все живые клетки на шагах 0, 50, ..., 10000; нет восстановленных
  событий между samples. `births` считает обоих детей; `divisions` считает деления.
- `accounting`: точные целые количества и накопленные каналы строками.
- `residual`: null у genesis, реальные нули только после проверенного перехода.

Экранных координат в данных нет. Well-mixed модель не моделирует перенос,
Brownian motion, пространственное строение или локальные градиенты.

## Каталог горизонтов

Два следующих прогона созданы свежими процессами из genesis, но с теми же
config/seed. Это разные горизонты одной детерминированной траектории, **не
независимые биологические реплики**. Общие sampled кадры всех пар сравниваются
генератором manifest и совпадают, включая точные накопленные accounting поля.

```powershell
cargo run --release -p liminis --example export_cell_replay -- --config configs/scenarios/cell-chamber.toml --seed 42 --steps 100000 --sample-every 500 --source-commit b2024cef08f0aa910a711a41c6a37fbc7b2ba35a --output site/data/cell-chamber-seed-42-100k.json
cargo run --release -p liminis --example export_cell_replay -- --config configs/scenarios/cell-chamber.toml --seed 42 --steps 1000000 --sample-every 5000 --source-commit b2024cef08f0aa910a711a41c6a37fbc7b2ba35a --output site/data/cell-chamber-seed-42-1m.json
node scripts/build_experiment_catalog.mjs
node --test site/catalog.test.mjs site/recording.test.mjs
```

Оба длинных прогона завершились без safety refusal. Численное ядро,
экспортёр, config и world version не изменялись между source commits.
Оба целых residual проверены на каждом переходе, а не только на 201 samples.

| Горизонт | Cadence | Живых в конце | Рождений | Делений | Смертей | Поколение max в конце |
| --- | --- | --- | --- | --- | --- | --- |
| 10000 | 50 | 94 | 1044 | 522 | 436 | 9 |
| 100000 | 500 | 99 | 8960 | 4480 | 4389 | 51 |
| 1000000 | 5000 | 88 | 89646 | 44823 | 44743 | 493 |

`cell-chamber-seed-42-100k.json`: 5356325 байт,
SHA-256 `b8db46c1dc83f8cd642ee2a3bd7913890e6868e265c61a8d390797399b37a4c9`,
BLAKE3 `ddeeea345add04e8b9fd266fddb14a7db635705654b80f1db99ce6cd3fd47021`.

`cell-chamber-seed-42-1m.json`: 5312311 байт,
SHA-256 `ee236189e9b6891599bc428aefb0e4b892c7397e712bcf045f7db14a5bc4061d`,
BLAKE3 `9aad47b0dc8013b88d22b1499cfca218fabb390a0af9ced483749be29ceded78`.

`catalog.json` генерируется из этих файлов, содержит identity/digest/summary
и один полный preview inventory каждой записи. Preview выбран по максимальному
числу sampled вариантов генома, затем sampled количеству клеток; tick указан
явно. Он не изображает финальное или обязательно типичное состояние.
Редкая выборка длинных прогонов может пропускать короткоживущие варианты.
Поколение 493 и миллион проверенных шагов сами по себе не свидетельствуют
о неограниченной эволюции или полной приёмке S0/S1.
