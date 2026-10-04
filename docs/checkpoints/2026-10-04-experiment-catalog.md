# Каталог Длинных Записей И Ручной Playback

- Дата и цель: 2026-10-04. Главная с настоящими записями 10k/100k/1M,
  отдельный observer и медленное ручное воспроизведение по запросу пользователя.
- Репозиторий: emevart/liminis, изолированный managed worktree
  `C:/Users/1/.codex/worktrees/local-living-world/liminis`, ветка
  `codex/experiment-catalog`. База `b2024cef08f0aa910a711a41c6a37fbc7b2ba35a`.
  PR #9 слит в main как `583b2ab253f3e8d8a36ce1c844c5e401bf38d551`;
  деревья этих SHA совпадают. CI main 37196499290 завершился успешно.
- Scope: только static site, manifest/generator/tests, CI-команда Node и
  сопровождающие документы. Core, config, exporter, world format и dt не менялись.
  Оригинальный dirty checkout `H:/liminis` и локальные культуры не включены.
- Координация: Sol реализует загрузчик/playback и ведёт Git; Astra согласовал
  каталог и сгенерировал длинные записи; viewer отвечает за HTML/CSS/главную;
  независимый cells_storage reviewer проверяет данные и fail-closed интеграцию.

## Сделано

- ADR-105 добавлен в конец DECISIONS. Каталог использует небольшой manifest
  с полным настоящим preview inventory, observer загружает только выбранный
  allowlisted файл. Проверяет размер, SHA-256 и schema до включения controls.
- 100k и 1M заново рассчитаны из genesis release Rust engine на source b2024.
  Все переходы проверены по обоим точным целым балансам, residual max = 0.
  Safety caps не снимались. Это одна общая seed42/config траектория,
  не три независимые биологические реплики. Общие sampled ticks сравниваются.
- По 201 кадру, cadence 50/500/5000. Ненаблюдённые события не дорисовываются,
  позиции display-only, данных о Brownian motion или связи с вокселем нет.
- Ручная скорость записи 0.05..60 frames/s, default 1; Previous/Next,
  Pause/scrub, Replay без автоматического зацикливания. Это не скорость engine.
  Entry points имеют версию `v=105`, чтобы обновить старые JS/CSS браузерного кэша.
- Первичный download anchor скрыт и не имеет href. Controls включаются после
  успешного verified load/render/bind; любая ошибка отключает их и скрывает ссылку.
- Apache-2.0 проекта сохранена, Lucide/Feather NOTICE дополнен chevron icons.

## Evidence И Проверки

Подробные команды воспроизведения, SHA-256 и BLAKE3 находятся в
`site/data/README.md`. Manifest freshness и shared trajectory проверяются тестами.

| Тики | Живых в конце | Рождений | Делений | Смертей | Поколение в конце |
| --- | --- | --- | --- | --- | --- |
| 10000 | 94 | 1044 | 522 | 436 | 9 |
| 100000 | 99 | 8960 | 4480 | 4389 | 51 |
| 1000000 | 88 | 89646 | 44823 | 44743 | 493 |

- `node --test site/recording.test.mjs site/catalog.test.mjs
  site/recording-loader.test.mjs site/playback.test.mjs`: 25 PASS.
- Независимый read-only review: 0 blockers. Единственная ранняя находка
  о download до завершения загрузки исправлена и повторно проверена.
- Browser QA на локальном HTTP 8090: главная 320/420/1440, observer
  320/420/590/1440 без горизонтального overflow. Все три маршрута; шаги
  50/500/5000; ручные 0.1 frames/s, Pause, Previous/Next, конец/Replay;
  keyboard cell inspector; unknown ID даёт явную ошибку, disabled controls
  и hidden download без href. Canvas непустой, JS console errors не обнаружены.
- Release повтор 100000 ticks занял 5.2107738 s (~19191 ticks/s), включая
  Cargo startup/export. `target/qa/cell-chamber-100k-benchmark.json` имеет тот же
  SHA-256 `b8db46c1dc83f8cd642ee2a3bd7913890e6868e265c61a8d390797399b37a4c9`.
  Это малая offline камера без HTTP, не throughput большого voxel world.
- Полный Rust CI для новой site-ветки и проверка опубликованного сайта ещё
  предстоят на момент этой записи; предыдущий source-identical main CI зелёный.

## Остаток И Разрешения

- Опубликовать отдельный PR, дождаться зелёного CI, слить и проверить
  liminis.dev. Пользователь разрешил публикацию в main, PR/merge и существующее
  выкатывание сайта сообщениями «публикуем в main ... мерджим» и
  «да ок, пусть выкатывается». Новую инфраструктуру не создавать.
- Cloud onboarding Liminis запущен через существующее GitHub connection;
  UI подтверждает GPT-6.1 Sol / Очень высокое. Это пока подготовка окружения,
  не завершённая coding-задача. Длинный debug-test ещё работает; приватность
  проверяется перед сохранением среды. Продуктовые правки onboarding запрещены.
- Следующий отдельный PR: manual/max engine pacing при неизменном dt,
  отзывчивых Pause/Step/save и measured TPS. Пользователь явно разрешил
  GPT-6.1 Sol xhigh, субагентов и Astra-советника. Поручение сохранено в
  `docs/plans/2026-10-04-cloud-continuation.md`; после merge передать cloud-сессии.
- После pacing: настоящие persisted coordinates/пассивный transport с numerics
  review, затем 2D/3D одного состояния, локальная micro-химия и дальше GRN/adhesion.
  Большое число тиков само по себе не означает приёмку S0/S1 или open-ended evolution.
