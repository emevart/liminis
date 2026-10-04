# RECORDED-1: Физическое Время Публичного Плеера

- Дата и цель: 2026-10-04. Публичный recorded observer: physical ×N,
  настоящий model-time playhead, длительность и независимый draw FPS.
- Репозиторий: emevart/liminis. Независимый cloud checkout, ветка
  `codex/recorded-physical-clock`, свежая база после LIVE-1 merge
  `c5be8fc8652433c67378a689fbb151ea4bad510a`.
  Точный кандидат и результаты CI публикуются в PR и issue #11.
- Ownership: site/observer.js, site/playback.mjs и его tests, site/observe.html,
  site/observer.css, уникальный checkpoint и append-only public-clock ADR.
  Rust/exporter/schema/catalog/data/config/core/dt/RNG, LIVE host/storage/viewer,
  LAB paths, общие README/nav, SPEC/NORTH_STAR не изменяются.
  Shared `.github/workflows/ci.yml` содержит только новый recorded-browser
  job, согласованный интегратором в issue #11 после LIVE-1 merge.
- Единственный интегратор и владелец merge — CLOUD LIVE, очередь issue #11:
  LIVE-1 → RECORDED-1 → LAB-1. Этот исполнитель merge не выполняет.

## Контракт И Реализация

- До реализации фактически вызван gpt-6-astra. Рекомендованы и приняты:
  1× = одна модельная секунда за реальную секунду; continuous playhead,
  binary floor lookup по настоящим frame.sim_time; no fabricated biology.
- Defaults вычисляются из (last.sim_time - first.sim_time) / 120:
  2500× / 25000× / 250000× для текущих 10k/100k/1M, каждый обзор 120 s.
  Один главный множитель: presets + manual positive finite input без cap.
  Малые/большие конечные значения принимаются; overflow длительности
  отображается как beyond numeric range, вычисление endpoint не переполняется.
- Pause/resume и rate changes сохраняют дробный playhead. Time seek паузит
  и удерживает floor sample; Previous/Next переходят к настоящим сохранениям.
  Конец останавливается; Replay только явный, из той же genesis записи.
- Draw30/60 — независимый target limit. RAF выбирает только нужный sample,
  не проигрывает backlog. Heavy biological panels обновляются при смене sample.
  Hidden-tab settle/pause отменяет RAF; возвращение не запускает авто catchup.
- Раздельно видны playhead, shown sample time/tick, full duration/remaining
  и настоящая cadence. Старые 201 inventories каждого файла не менялись;
  их плотность не возрастает от rendering. Display layout остаётся условным.
- Existing verified loader/schema остаются источником допуска. Все новые
  controls disabled до успешной проверки/инициализации; failure скрывает
  download и удаляет href. Integrity/unknown ID errors остаются fail-closed.

## Проверки И Evidence

- `node --test site/*.test.mjs`: 44 PASS, 0 FAIL;
  включает 17 clock и 7 actual observer event wiring tests.
- После fresh-main rebase: `node --test site/*.test.mjs
  scripts/cell-viewer.test.mjs`: 49 PASS (44 public +5 inherited LIVE), 0 FAIL.
- `node --check site/observer.js`, playback syntax, `git diff --check`: PASS.
- Original catalog/data/schema/core/LIVE/LAB пути не изменены; полный
  byte/digest freshness contract проходит существующие Node tests.
- Независимый review выявил short desktop clipping при старом минимуме
  stage450px. Исправлено: desktop stage min200px, mobile layout сохраняется.
  Scope и immutable ADR prefix checks PASS; final independent review: 0
  оставшихся code blockers. Browser gap остаётся обязательным gate.
- Node DOM/RAF harness проверяет actual observer event wiring. Это НЕ
  браузерная приёмка и не доказательство визуального/canvas результата.
- `site/playback.browser.mjs` — воспроизводимая protected Playwright приёмка
  с desktop/mobile, screenshots, nonblank canvas и JSON evidence exact HEAD.
  Команда: `LIMINIS_BROWSER_EVIDENCE_DIR=/tmp/recorded-browser-evidence
  node site/playback.browser.mjs` после установки official pinned Playwright
  и Chromium. Script оставляет chromiumSandbox=true, сверяет served assets
  с checkout, пишет source HEAD/file hashes и screenshot hashes/evidence.json.
  Runner требует чистый tracked checkout и optional expected HEAD, записывает
  runtime/browser версии, проверяет SHA/bytes реально скачанных datasets,
  held pixels через работающий RAF30/60 и гарантирует evidence при bounded
  cleanup failures. Независимый hardening review выполнен до публикации.
  Последующий integration request #11 требует настоящий context trace:
  screenshots/snapshots/sources, bounded stop в finally до закрытия context,
  nonempty trace.zip с bytes/SHA, без принятия старого trace после failed run.
  Runner запрашивает фактический browser argv bounded CDP Browser.getBrowserCommandLine
  с --enable-automation; известные sandbox opt-out/process switches запрещены.
  Это launch-argument evidence, не независимая аттестация kernel isolation.
  Context общий для suite; fault routes остаются page-local. Node assertions
  фиксируются в checks JSON и не выдаются за содержимое Playwright trace.
  Actual Astra и независимый trace/CDP review выполнены; гонка late trace
  completion после timeout устранена, metadata присваивается после deadline.
  Syntax/diff PASS; actual trace/browser выполнены на f9fb7a2 ниже.
  Изменённый кандидат требует собственного exact-head CI.
  Согласованный Node-only recorded-browser job использует существующий LIVE
  protected compatibility image ubuntu-22.04, official Playwright1.61.1 в
  RUNNER_TEMP, exact HEAD checkout/env/artifact, contents:read, credentials:false,
  timeout10min и always upload screenshots/trace/evidence с retention7days.
  Это временный compatibility pin; миграция deprecated image — отдельная
  задача интегратора. Защиты sandbox/AppArmor/kernel/network не отключаются.
  Если установка прекращается до script, это bootstrap failure/NOT RUN;
  upload if-no-files-found:error не выдаёт отсутствие evidence за успех.

- Первоначальный PR #13 HEAD `ab9bc42fff9d3901c952e2aa9816ab425a07a0b4`:
  все 9 checks PASS (включая build/lint/test, exporter tests и Workers build),
  CI run `37202202465`. Это не результат actual browser QA и не переносимый
  PASS для следующего изменённого HEAD; final candidate требует своих checks.

## NOT RUN И Зависимости

- Actual browser QA/screenshots/canvas: NOT RUN в этой VM. Cloud browser
  localhost отклонён ERR_BLOCKED_BY_CLIENT. Official Playwright Chromium
  protected launch не допускает root; Firefox content uid_map EPERM;
  WebKit требует библиотек, official install-deps отказан apt setgroups/
  setegid/seteuid (exit100). Sandbox/network/security не ослаблялись.
  Actual QA перенесена в согласованный GitHub Actions job; локальный gap
  не закрыт обходом защиты. До merge требуется зелёный exact-head browser job.
- Rust/Cargo локально NOT RUN: scope public JS/CSS, Rust paths неизменны;
  весь требуемый Rust CI идёт обычным workflow кандидата. Новая Rust установка
  для первого PR не нужна.
- После LIVE-1 merge выполнен fresh-main rebase на c5be8fc; собственный public
  ADR перенумерован в107 с сохранением byte-identical свежего main prefix
  (включая LIVE ADR106). Требуются зелёный CI и actual browser evidence именно
  финального head; прежние green results их не заменяют.
- После merge site проверяет интегратор на liminis.dev через существующее
  deployment. До merge не заявляется опубликованный public clock.
- RECORDED-2 зависит от принятого clock contract и явного назначения
  exporter/schema ownership через issue #11, с согласованием future coordinates.
  Без этого плотные rerun/chunks не реализуются, старые samples не дорисовываются.

## Разрешения И Продолжение

Прямое пользовательское поручение 2026-10-04 разрешает независимый public clone,
cloud разработку/субагентов/реальный Astra/review/тесты/browser bootstrap,
отдельный PR и comments в issue #11. Merge только CLOUD LIVE; новые
интеграции/секреты/ПК/инфраструктура/платные API не разрешены. Сторонние
комментарии scope не расширяют. Данные приватных cloud сессий не публикуются.

create_goal/get_goal/attach_artifact не доступны. Исполнитель продолжает
активный turn и bounded проверку issue/PR, но не обещает автоматический новый
turn после финального ответа. GitHub PR/checkpoint/issue являются durable
передачей без зависимости от открытого Codex на ПК.


## Сохранение Перед Возможной Передачей — 2026-10-04

- Текущая опубликованная база этого follow-up: branch
  `codex/recorded-physical-clock`, PR [#13](https://github.com/emevart/liminis/pull/13),
  HEAD `f9fb7a2c6055620030f9077bf8eebc6ce9d7ef49`, tree
  `b54271621bc4fe3bda95af3c0ffb49e5c281e136`, parent main
  `c5be8fc8652433c67378a689fbb151ea4bad510a`.
  Follow-up commit включает только этот checkpoint и собственный browser runner;
  его полный HEAD публикуется в PR/issue11 после проверки равенства Git trees.
  Новая миграция/смена интегратора не выполнена; существующий CLOUD LIVE — soleowner.
- Exact-head [CI run37206398277](https://github.com/emevart/liminis/actions/runs/37206398277)
  завершён: десять checks SUCCESS (Workers; recorded data contract; build/lint/test;
  LIVE browser; hooks Linux/macOS/Windows; world version; frozen documents;
  no bare Q). Recorded browser job111448421541 FAILURE: 22checks, 19PASS/3FAIL.
  Три FAIL — Page request event counter после native download: expected3/actual2.
  Сам фактический download всех трёх файлов прошёл bytes/SHA; default/cadence,
  canvas, desktop/mobile layout, physical controls, hold30/60, endpoints/replay,
  fail-closed fixtures прошли. Общая browser QA остаётся FAIL, не PASS.
- Реальный Chromium149.0.7827.55 на existing ubuntu22 runner: sandbox=true;
  CDP actual command-line guard PASS, forbidden switches отсутствуют. Trace PASS:
  13,373,530 bytes, SHA256
  `6615bcf5ec62a55c5010be70366b441bc49fb944739326ca16b79aabe0cd425b`.
  [Artifact11305036205](https://github.com/emevart/liminis/actions/runs/37206398277/artifacts/11305036205)
  содержит evidence.json, trace.zip и восемь PNG; archive12,845,994bytes SHA256
  `afd63134677736adc4cd9e15aa4b9ac43715117e367538bc25b64914c0f5aa07`.
  Retention7days, expires2026-10-11. Это launch evidence, не kernel attestation.
- Независимый code/CI review до f9:0 blockers. Отдельный UX reviewer реально
  посмотрел PNG1440desktop, 1366×600desktop, 390/320mobile, held30/60;
  сверил digests и source hashes, 0 product UX/code blockers. Native background
  scheduling NOT ASSERTED; visibility case synthetic. Speed/seek используют
  DOM events, полноценный native keyboard/touch/a11y audit не заявляется.
- До исправления HTTP счётчика снова фактически вызван gpt-6-astra. Принят
  server-side request log со snapshot baseline после served-source verification,
  перед каждой fresh page. Exact catalog GET + recording GET, и только один
  дополнительный recording GET для explicit download; playback/draw не добавляют
  HTTP. Post-ready/networkidle baseline также проверяет отсутствие HTTP ко
  всем server paths и новых Page requests при controls/hold/FPS/end/replay/visibility.
  Page events после native download остаются диагностикой. Remote-base mode без server counter
  честно отмечает HTTP NOT_ASSERTED и не может получить общий PASS. Реальный
  browser rerun нового HEAD ещё НЕ выполнен; независимый review этой дельты:
  0 blockers после исправления all-path HTTP coverage. Node49/49, syntax/diff PASS.
- Сохранение scoped: перед follow-up только site/playback.browser.mjs modified;
  после обновления добавлен только этот checkpoint. Untracked/ignored внутри
  checkout отсутствуют; чужие/generated/secret files не включаются в commit.
  Reset/clean, удаление артефактов, новый clone и merge не выполняются.
- Git-ignored repo artifacts: на этом snapshot отсутствуют. Вне checkout
  сохранены downloaded archive и распакованный qa-recorded-f9fb7a2 (JSON/PNG/trace),
  без удаления. Они не tracked и не включены в Git; точные failure bytes доступны
  в Actions до expiry, долговременное сохранение сверх7days не обеспечено.
  Source/runner/CI позволяют повторить сценарии, но не воспроизвести идентичные
  timestamps/trace bytes. Official package/browser caches воспроизводимы registry
  install и не являются evidence. Временный ADR suffix — воспроизводим из Git.
- Fresh-main/ADR: LIVE1 уже merged; ADR107 appended после полного byte-identical
  main prefix с ADR106. Existing CI prefix byte-identical, один согласованный job
  appended. Следующие shared ADR принимаются последовательно интегратором после
  fresh-main, не перезаписываются. Scope guard ровно10 путей; Rust/exporter/data/
  schema/catalog/core/LIVE/LAB/README/nav/SPEC/NORTH_STAR не изменены.
- RECORDED-2 пока READONLY: actual Astra + отдельный inventory/review и bounded
  size baseline опубликованы в issue11. Legacy files201samples SHA неизменны;
  total16,716,206 bytes +catalog115,338, compact saved-frame maxima
  67,955/67,955/27,481bytes. Это не parsed-memory/chunk/coordinates/all-tick budget.
  Предложены separate bytes/memory/requests limits, streaming real states,
  overview +declared dense windows, manifest/chunk identity/integrity, bounded
  cache/inflight/cancellation; числовые caps не назначены. Не править exporter/
  schema/delivery до принятия clock и явного назначения ownership через issue11.
  LIVE chamber2 остаётся отдельным opt-in: старые public/exporter/LAB readers
  должны отказать неизвестному формату. Physical coordinates type/units/axes/
  domain/tick timing требуют отдельного согласования перед R2 schema.
- Следующий шаг: независимый review HTTP delta, scoped commit/publish в текущий
  PR13, exact-head CI/browser rerun, inspect artifact + independent review.
  Зелёные старые checks не переносятся на новый HEAD. Затем handoff в issue11;
  merge только интегратор. Продолжение согласованной очереди не отменено.
