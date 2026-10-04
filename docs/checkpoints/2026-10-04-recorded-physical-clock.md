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


## Подтверждённая Приёмка HTTP Исправления

- Сохранённый follow-up HEAD `f72fe98e50b4e6dca216421998213ff978d6b35a`,
  tree `f75d48bce5870565483e96f049ce64c364456dad`, parent f9fb7a2 выше.
  Local/remote совпали; clean checkout, untracked/ignored отсутствуют. Git ref
  принят только после равенства index/worktree/remote tree, без reset/clean
  или замены файлов. Этот последующий commit меняет только checkpoint;
  финальный HEAD и его отдельные checks публикуются в PR13/issue11.
- [Exact CI run37207963950](https://github.com/emevart/liminis/actions/runs/37207963950):
  recorded browser job111453040333 SUCCESS, actual22/22 browser checks,
  17/17 HTTP checks PASS. До этой записи остальные10checks SUCCESS, кроме
  build/lint/test, ещё выполняющего Test; итог всего run не объявлен успешным.
- [Artifact11305057269](https://github.com/emevart/liminis/actions/runs/37207963950/artifacts/11305057269)
  archive12,770,516bytes SHA256
  `02d17a246c7c2568770d1c96e0fafd75026945c0b0a67a534db794eedee6759b`.
  Trace13,117,307bytes SHA256
  `80005e44c2a05d55a32f31e8818572cfb9583aded4a6c3dcbbbb51468016acac`,
  213 ZIP entries, CRC verified. Real actions/snapshots/network/source14pages,
  no API/trace/cleanup errors. Source HEAD/tree and all source/served/runner/
  PNG/download digests verified. Native download сервер видит как третий GET,
  даже когда Page request events остаются2; assertion не ослаблен.
  Все пять post-ready controls/hold/FPS/replay/visibility windows не создают
  server HTTP ко всем путям и Page requests. Unknown ID запрашивает лишь catalog.
- Actual Chromium149 protected launch +CDP guard PASS. Реально просмотрены
  восемь новых PNG (три desktop1440, desktop1366×600, mobile390/320, held30/60).
  Canvas nonblank:3823paintedPixels для каждого dataset; held pixels идентичны.
  Отдельные независимые technical artifact и UX/product reviews:0blockers,
  falsePASS не найден. Новый runtime reviewer launch не приписывается этим reviews.
- Limits: Chromium/Linux; mobile default10k; native background scheduling,
  native typing/range/touch, achievedFPS benchmark и полныйa11y audit NOTASSERTED.
  Syntheticvisibility +actual adapter Node wiring проверены, 30/60 duration,
  endpoints и heldpixels проверены. Sandbox argv не аттестует kernel isolation.
- Новые ZIP/evidence/8PNG/trace сохранены вне checkout в qa-recorded-f72fe98 и
  attachments, предыдущие F9 failure copies не удалены. Git-ignored artifacts
  внутри checkout по-прежнему отсутствуют. Actions retention до2026-10-11;
  сверх7days exactbytes долговременно не сохранены, scenarios воспроизводимы.
- Final gate: зелёный полный CI именно финального PR HEAD после этой docs-only
  записи, actual exact-head browser artifact и independent source/hash review;
  статус обновляется в issue11 без переноса старого green на новую ревизию.
  RECORDED2 по-прежнему требует назначения exporter/schema/delivery ownership;
  merge/release интегрирует только действующий CLOUD LIVE после приёмки.


## Migration Pause — ACK HANDOFF AUTHOR STOPPED

Прямое уточнение пользователя 2026-10-04 supersedes continuation выше:
Native Codex Cloud создан пользователем для передачи, пока read-only;
старый LIVE coordinator прекращает dispatch/merge/deploy. После сохранения
этой bounded HTTP instrumentation границы RECORDED author прекращает новые
coding/stage assignments, включая R2. Project не отменён. Merge/deploy здесь
не выполняются; никакой новой сессии/интеграции/ownership самостоятельно нет.

- HTTP runtime исправление уже опубликовано и реально проверено на f72fe98
  выше. Этот handoff commit меняет только уникальный checkpoint; source/runtime
  tree files остаются побайтно теми же. Его full HEAD/tree и автоматический CI
  run/status публикуются в PR13/issue11; новый head не объявляется green по
  старому run. Последний snapshot f72 CI:10checks SUCCESS, build/lint/test
  job111453040393 IN_PROGRESS/Test, exporter test step PENDING.
- Все шесть существующих subagents COMPLETED, running/queued0: Astra clock,
  clock module, browser bootstrap, independent review, UI event/UX review,
  RECORDED2 readonly inventory. Их полезный код включён в PR13, решения/reviews/
  measurements отражены здесь и в issue11; unpublished agent code отсутствует.
- Собственных persistent Node/Cargo/browser/HTTP8765 процессов нет. Только
  GitHub Actions текущего/следующего commit могут оставаться running/queued;
  точный статус передаётся без ложногоPASS, нового local long run нет.
- До сохранения dirty только этот checkpoint. После commit/publish ожидается
  clean exact local/remote checkout; status/untracked/ignored проверяется и
  публикуется. Generated evidence лежит вне repo и не удаляется/не добавляется
  в PR. Reset/clean/newclone/merge/deploy не выполняются.
- Остаток для единственного native coordinator: импортировать PR13 head,
  дождаться его exact-head CI/сверить runtime hashes с принятой QA f72,
  закрыть acceptance/release gate и сохранить permanent binary archive через
  существующий GitHub до Actions expiry. R2 только readonly inventory, ownership
  exporter/schema/delivery не назначен; никаких R2 coding assignments.

### Archive Inventory Для Existing GitHub

Готовые ZIP уже доступны через существующий Actions, не требуют повторной QA:

| Evidence | Run / artifact | Bytes | SHA256 |
|---|---|---:|---|
| HTTP corrected PASS f72fe98 | 37207963950 / 11305057269 | 12770516 | `02d17a246c7c2568770d1c96e0fafd75026945c0b0a67a534db794eedee6759b` |
| Original FAIL f9fb7a2 | 37206398277 / 11305036205 | 12845994 | `afd63134677736adc4cd9e15aa4b9ac43715117e367538bc25b64914c0f5aa07` |

Оба ZIP сохранить целиком: evidence.json, trace.zip и восемь PNG ниже.
PNGs побайтно одинаковы между двумя runs; reports/traces отличаются и нужны
оба для проверяемой истории исправления. SHA256 corrected members:

| Member | Bytes | SHA256 |
| cell-chamber-100k-desktop.png | 112446 | `4ff6ce5769022874a7ea63aa5e46a4b07608af8215e55037ffa5cf4ade64b2c2` |
| cell-chamber-10k-desktop.png | 104093 | `c60788f5cc1a7a4e8b2316b2411c5f5e0d37702380ddac3cb39f437c01bee829` |
| cell-chamber-1m-desktop.png | 113466 | `fe272dcd2b43b23e1a3a881c3260400f4a1ccb9894aa7c09b3c1a69e1a3c1525` |
| desktop-short.png | 85115 | `a9a8cc28be87e224f6f98dc50bcb9b7aca955f2dcdf75178d8a4b2d424245ed5` |
| evidence.json | 19796 | `6187a3a92d6c9a5931ce9e10560ff2e22c30a7db9bf1377f9ca0cc4a37552735` |
| held-30fps.png | 25742 | `6441785dc924ad2d74ce1ca7dfc60d3fe4f81af1473ee268bb3d077d2b7f0484` |
| held-60fps.png | 25742 | `6441785dc924ad2d74ce1ca7dfc60d3fe4f81af1473ee268bb3d077d2b7f0484` |
| mobile-narrow.png | 130536 | `4c449822845169b0f63915f3eddb4e91ce2b9cf7d12a8b8b44cbd0321e34a76a` |
| mobile.png | 134087 | `788dc75f08244430e8d7b9c3fe5ac73e9f60f0c6449363f16b33cd72483ec0de` |
| trace.zip | 13117307 | `80005e44c2a05d55a32f31e8818572cfb9583aded4a6c3dcbbbb51468016acac` |

Failure evidence.json SHA256
`6d8af5bf36b29febf18aa3c1e4e05ad3338017313f34a58ec10fac8f38a47c5c`;
failure trace SHA указан выше. Локальные QA dirs/ZIPs сохранены нетронутыми.
Actions retention7days/expiry2026-10-11 — это НЕ permanent archive. В callable
existing GitHub доступны artifact read/download, upload/release asset capability
не предоставлена; permanent binary archive этим author не создан. Durable Git
checkpoint/issue сохраняют точные ссылки/состав/digests для archive coordinator.
Пакеты/browser caches воспроизводимы, raw timestamps/trace bytes — нет.
