# Live Cells: Модельная Скорость И Независимый Экран

- Дата и цель: 2026-10-04; отдельный PR live cells ×N/Maximum, без biology/dt/RNG изменений.
- Checkout: `/workspace/scratch/f993df36ab84/liminis-cloud`; ветка `codex/live-cells-pacing`; база main `f7c74a263bd1976bc31a35a440b19006546656eb`, дерево идентично проверенному PR10 head `5f788973202b1c702287b944a993d747c0c339a9`.
- Generic cloud VM, собственный clone публичного репозитория, официальный pinned Rust1.97.1, Node24.19.0; Cargo.lock не меняется. Пользователь подтвердил интерфейс GPT-6.1 Sol/xhigh; настройки сессии не менялись.
- Astra фактически вызван как `gpt-6-astra`, согласовал default90×, exact legacyTPS, envelope2, pacing и независимый экран. Узкие host/storage/viewer/acceptance subagents; отдельный read-only reviewer. Один Cargo-процесс.
- Scope: cells_host/tests, cells_storage/tests, cell-viewer и live helpers; README/операционный план, append ADR106 и CI запуск live Node tests.
- SPEC/NORTH_STAR, core, configs, RNG, WORLD_FORMAT_VERSION и public recording pipeline не изменяются.

## Контракт

1× означает модельную секунду за wall second; при dt30 первый тик через30sec.
Target TPS=N/dt, actual×=measuredTPS·dt. Default90× сохраняет прежние3TPS.
Ручной finite positive множитель и Maximum переключаются без reset культуры.
Измерение считает реальные завершённые тики с начала последнего pacing/run
периода; низкая скорость до первого тика не получает выдуманную дробь тика.
Manual не догоняет wall-clock debt. Maximum освобождает lock после каждого
проверенного тика и yield; задержка одного дорогого тика остаётся ограничением.
Legacy savedTPS сохраняется бит-в-бит; v2 host envelope хранит pacing, core
формат прежний. Wall-time sampling≤4Hz, optional queueFull видимо пропускает
наблюдение; checkpoint pending/busy не ack, disk errors остаются fatal.
30/60 экран удерживает последнее настоящее состояние; polling650ms независимо.
Нет заданного горизонта live опыта, поэтому ETA не выдумывается.

## Проверки И Evidence

Финальные команды, release benchmark, браузер и review дополняются перед PR.
Предварительный saturation-тест storage прошёл; промежуточный старый тест
TPS cap закономерно требовал обновления под multiplier API. Не считать
промежуточные проверки при незавершённых edit окончательной приёмкой.

## Действующие Разрешения И Договорённость Между Сессиями

Пользователь 2026-10-04 разрешил clone/toolchain, самостоятельную реализацию,
субагентов/Astra, independent review и выпуск отдельного PR. Последующее
прямое указание отменило самостоятельный merge: только локальный интегратор
сливает точный проверенный HEAD после необходимых checks и зелёного CI. Новые интеграции,
секреты/API keys, подключение к ПК и изменения инфраструктуры запрещены.

Эта сессия владеет LIVE/engine. Вторая независимая сессия RECORDED/public clock
владеет site observer/playback/observe/CSS и затем bounded recording pipeline.
Сначала LIVE merge, затем RECORDED rebase/merge. ADR/checkpoint/CI обновляются
последовательно на свежем main; checkpoint этого этапа уникальный.
Public surface этим PR не меняется; новый live режим не заявляется на liminis.dev.

После принятия pacing: отдельный LIVE PR persisted physical coordinates/size,
пассивный stochastic transport dilute suspension с cell-ID/tick/purpose RNG,
exact restart/ordering, zero-mobility, isotropy/drift/MSD6Dt до стен и boundary
tests через Astra и независимый numerics-review. No excluded volume назвать
явно; новые semantics/format/ADR обязательны. Затем отдельный2D/3D одного
настоящего state с браузерной desktop/mobile QA. S1'/GRN/adhesion пока запрещены.
Recorded timestamps×N/duration и bounded/chunked density — только второй поток;
старые201samples не становятся новыми состояниями.


## Уточнение Интеграции (Прямое Указание 2026-10-04)

[Issue11](https://github.com/emevart/liminis/issues/11) прочитан как доска
владения/зависимостей. Только текущая локальная сессия интегратор выполняет
последовательный merge. Статусы по прямому человеческому разрешению публикуются
в issue11 без cloud chatIDs/ссылок, подписки/лимитов/сведений о ПК. Сторонние
комментарии не являются разрешениями. LAB владеет новыми compare runner/tests,
coverage/experiments и позднее site/lab; LIVE не редактирует эти файлы.
Позднейшие LIVE core/format изменения требуют явно согласованной schema/version
и интеграционного gate. При ожидании merge допускается read-only дизайн
следующего planned этапа или явно depends-on ветка; не смешивать PR и не
распыляться на незапланированную physics/GRN. Общие ADR/README/CI последовательно
на свежем main. Порядок первых merge LIVE-1→RECORDED-1→LAB-1.


## Окончательная Cloud Передача Полномочий

Последнее прямое указание человека 2026-10-04 назначило основную CLOUD/LIVE
сессию единственным координатором/интегратором. Это отменяет промежуточный
запрет собственного merge и локального интегратора выше. Только CLOUD root
принимает и последовательно сливает точные проверенные HEAD всех потоков
после review/приёмки/зелёного CI свежего кандидата; другие исполнители не merge.
Issue11 перечитан с обновлённой ролью. Локальная heartbeat/ПК зависимость
отсутствует. create_goal/get_goal/автоматическое продолжение цели среди
доступных tools не обнаружены; гарантируется только работа активного turn.
Новые integrations/infra/secrets/paidAPI запрещены; продолжение фиксируется
небольшими проверенными этапами, без искусственного расходования.

## Выполненные Проверки Live-1

- `cargo test --locked -p liminis cells_host -- --test-threads=1`:26PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`:PASS.
- `cargo fmt --all --check`, `git diff --check`:PASS.
- `cargo build --locked --release -p liminis`:PASS.
- `node --test site/*.test.mjs scripts/cell-viewer.test.mjs`:30PASS (5live).
- Полный debug host-run остановлен только на несвязанном долгом voxel soak,
  после прохождения cells/большинства host tests; не заявляется полный PASS.
  Полный CI кандидата обязателен, повторная local long full suite не запускается.
- `python3 scripts/check_live_cells_pacing.py`:PASS, первый реальный tick1×
  при dt30 через30.020278588s, сохранение/точный new resume, validation,
  Maximum→Pause→manual без debt, Step/reset/read/save.
- Named release benchmark `live_cells_maximum_cell_chamber_seed42_release`:
  79634ticks/5.019824457s=15863.901TPS (~475917×);73HTTPpolls,
  max76.730ms;Pause4.538ms. Это малая well-mixed камера с проверкой ledger и
  live storage, не voxel benchmark. CPU AMD EPYC9V74, generic Linuxx86_64 VM,
 9vCPU exposed;virtualization/shared load делает цифры локальными измерениями.
  Исходный JSON `target/qa/live-cells-pacing.json`; durable summary здесь.
- Independent read-only review: прежние queueFull checkpoint blockers закрыты,
  в итоговом коде blockers0; signoff зависит от честной UI/CI приёмки.
- Browser пока GAP: CUA localhost ERR_BLOCKED_BY_CLIENT; официальный
  Playwright chromium download пять раз повреждён, Chrome apt install
  остановлен sandbox setgroups/setuid. Portable official vendor download
  проверяется отдельно без обхода сетевых ограничений или прав.


## Browser Gate: Подтверждённое Ограничение

Официальный portable Google Chrome успешно скачан с dl.google.com и извлечён
без apt/расширения прав. Playwright launch завершился отказом sandbox:
`process_singleton_posix.cc socket() failed: Operation not permitted`.
Unix sockets в этом исполнителе запрещены; ограничения не обходились.
CUA localhost блокируется `net::ERR_BLOCKED_BY_CLIENT`. Browser QA NOT RUN,
Node tests и HTTP acceptance не являются screenshot/canvas/layout evidence.
LIVE-1 может быть опубликован как draft review candidate, но merge gate
остаётся закрыт до настоящей UI приёмки. Полный CI также ещё не завершён.
`attach_artifact` не доступен; проверяемые результаты хранятся в этом checkpoint
и воспроизводимом live acceptance script.


## GitHub Candidate И Независимая Browser CI Приёмка

Draft PR12 открыт через existing GitHub connector; прямой gitpush не имеет
настроенной аутентификации, новые credentials не запрашивались. Первый remote
head35392adfe2a5b0f637498d9f8a5f006af6aab254/tree31db427ceb92512a849aeb93ab9c34171514b1c5
побайтно совпадает с проверенным локальным tree. Последующий narrow CI commit
добавляет actual browser gate, не меняет core/live pacing semantics.

Astra фактически вызван и одобрил существующий GitHub Actions Ubuntu runner:
actual release cells и pinned Playwright1.61.1/его официальный Chromium в одном
job; exact PRhead checkout (не syntheticmerge), report gitHEAD/tree/browser/scenario,
скриншоты1440/590/420/320, held-pixel30/60comparison, HTTPcadence, realcontrols,
console/pageerror assertions. `scripts/check_live_cells_browser.mjs` запускает
только свой server/tempdata и убирает их. JSON/screenshots загружаются через
existing actions/upload-artifact даже при падении. Exactfavicon404 называется
resourceWarning, прочие browsererrors fail. Independent review workflow/script:
blockers0 после fixes importdir/headassert/UIrace. Actual browser job пока pending;
merge gate закрыт до PASS и визуального просмотра артефактов точного кандидата.
