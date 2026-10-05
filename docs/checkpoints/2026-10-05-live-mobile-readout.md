# LIVE: мобильный readout без перекрытия инспектора

2026-10-05. Цель: устранить подтверждённое визуальное перекрытие legacy
chamber1 inspector/Population нижним sticky readout при320px. Источник finding:
accepted MAIN e2 live-cells-browser/viewer-320.png,145527bytes, SHA256
ca2d8136958a6c9c852c5ec9dc63c53c26c768e68b6b3fd98866b28bd86283aa.
Recorded HUD принят отдельным PR23; его исправление не закрывало этот finding.

База — fresh accepted MAIN813be1efad29569a3011f00697bd5d41b7397dc6,
treec5775c00a150768f2819e265a4165d8605bd48e3. PR23 candidate independent
source/actual0, fresh MAIN9SUCCESS+2expected PR-only SKIPPED и fresh public
independent actual0 приняты, оригиналы и selected evidence сохранены с
network readback. Старые PASS остаются привязаны к прежним HEAD/tree/run.

Product author frozen f939521c7999f5f2d226db462c4d0ad2900eb877,
tree0b1eff0a12213f24af050bcf81272da4d192b1a6, parent aadbb474….
Владение — только mobile CSS в crates/liminis/src/cell-viewer.html.
Прежние chamber2 normal-flow rows применяются также к chamber1:
masthead → скрытый offline → stage → readout → side. Readout static,
bottom0/z-index8 сохранены после narrow independent source finding;
stage heights/minima, desktop CSS, DOM/IDs/JS/API/model/dt/×N/FPS unchanged.
Physical computed declarations должны оставаться прежними; actual geometry
и screenshots ещё требуют новой приёмки. Legacy glyphs остаются schematic.

QA author frozen a973b25b47b7bf66330d280a199aee4b1544f07b,
treec4e389903eea073616eb33ea3d882236b456a1cd, parent813be1….
Владение — только scripts/check_live_cells_browser.mjs, прежняя responsive
paused phase. Сохранены старые viewport/PNG/gates, добавлен390px и viewport
captures stage/readout/upper+lower inspector/environment/events. Actual rects,
full readout text, clipping/elementFromPoint native hit tests и настоящий
scroll проверяют доступность. Find absent→present требует exact snapshot ID,
основных inspector полей/genome/generation и последнего нижнего phenotype
field; полные bounds и scroll подтверждают доступ к верхнему/нижнему inspector.
Это не отдельный assertion каждого physical xyz/phenotype поля;
resources/events сравниваются с actual API.
Request baseline ставится до resize/scroll; только прежние GET state/history
допускаются, model-control POST не допускается. Responsive counts/timestamps
сохраняются;650/4000ms обозначают прежние source constants, не новый measured
cadence assertion. Прежний actual30/60 polling guard остаётся неизменным.
Final actual snapshot должен
быть byte-equivalent по model fields. Никакие ответы/fixtures не фабрикуются.
Прежние held pixels/state/pacing/clock/Save/sandbox/error/cleanup/time budgets
не ослабляются. Node syntax/source checks не являются browserPASS.

Root интегрирует exact author blobs и владеет только этим checkpoint и
append-only operational acceptance. Checkout liminis-live-mobile-readout-
integration, codex/native-live-mobile-readout-integration. Независимый reviewer
отдельный от authors; frozen combined exact-source verdict **PENDING**.
Shared ADR/CI/navigation, core/config/raw, recorded observer и frozen
SPEC/NORTH_STAR не изменяются. Это реализация уже принятого mobile layout,
не новое решение о модели.

Следующие gates: независимый source0; один существующий protected full CI на
exact HEAD/tree; независимая actual chamber1 mobile320/390/590+desktop и
physical mobile regression. GL/projection regression должен реально PASS.
Общий cell-viewer.html изменён: whole-source identity bridge к e2 недопустим.
Change-impact bridge описывает shared mobile CSS delta, неизменённые HTML вне
style/3D renderer/vendor/host/GL QA/guards и свежие actual GL/projection gates.
Старый e2 independent GL artifact review остаётся историческим; новый полный
independent GL artifact review без отдельного concern здесь не заявляется.
Affected original ZIP
и reviews должны получить closed durable archive + normal network readback
до guarded merge. После merge — fresh MAIN regression и existing public gate.
Любой FAIL/drift/not-executed/new concern → HOLD/scoped review, без rerun ради
GREEN и без переноса старого PASS. PR/CI/actual/archive/merge пока **PENDING**.

Разрешения: explicit ownership/write-go пользователя, codex branches/PR/main
merge после GREEN+independent acceptance и существующий site deploy. Astra
одобрила этот bounded UX scope после HUD MAIN/public acceptance. Пользователь
вернулся; ранее выданные права сохраняются. LAB21/publicLAB/LAB4/models/causal
retries остаются HOLD. Нет новой infrastructure/hosting/integrations/secrets/
spend, native browser bypass, reset/clean/force-overwrite или удаления evidence.

Следующий шаг: freeze combined HEAD/tree, независимое source review, затем
один protected official pinned Playwright CI; никаких native browser/models.
