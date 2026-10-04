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
  Syntax/diff PASS; actual trace/browser результат остаётся NOT RUN до CI.
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
  Интегратор должен выполнить actual browser script и закрыть gap до merge.
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
