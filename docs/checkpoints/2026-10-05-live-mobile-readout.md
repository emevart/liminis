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

## Первый actual FAIL и узкая QA-правка

Run37282535160 attempt1 на a57a6854b633c8e272eb4012ff11a0ec125e6fc2/
tree2f9f0760ada0bbd1812a232bc6a30883242c76d3: legacy LIVE actual FAIL при
native scroll нижнего inspector field на1440px — Element is not attached
to the DOM. Mobile cases/physical/projection/GL actual steps NOT_EXECUTED.
Source qualified0 не заменяет actual приёмку; PR24 merge HOLD.
Original artifact11332948215,8544954B,
SHAc0b5cddbc1d5d520e80f079c93bbac2adffc446bff04d1b1bd1d4dac91352ba0
сохранён без перепаковки, outer/nested CRC PASS. Independent failure review
прочитал четыре report PNG и два held PNG, source/body/trace: paused tick2138/
ID240 unchanged, первых12gates PASS, complete responsive gate не достигнут.
Privacy35UTF8/994970B obviouspatterns/auth/cookies0, bounded qualification.

Trace call317 resolved transient lastdd0.05 около8210.152ms, stateGET
8218.488ms→HTTP200 примерно8222.616ms; последующий inspector сериализован
заново с тем же ID/tick. Source commitState→renderInspector.replaceChildren
на обычном poll согласуется с lifecycle finding; direct mutation callback/
единственный инициатор не записаны, точная causality не заявляется.

Astra APPROVE_BOUNDED_QA_REPAIR: только прежний browser QA helper upper/lower.
Автор frozen d61f56d9490a38221bdf4ea046f732619c114dc9,
tree6932c58eaaa35a32990447fd43652ca7a8b6ad32, parenta973b25b….
Native scroll теперь обращён к persistent #cell-detail; после await одна
синхронная read-only evaluation заново находит current конкретный upperDL/
lowerfield и снимает rect/clipping/hit/text/group +exact ID/tick/inspector.
Нельзя удерживать descendant/ElementHandle через await. Именно field, а не
wrapper, обязан пройти все прежние strict bounds/hit/value oracles и PNG.
Если wrapper scroll не выводит field целиком в viewport — FAIL: нет fallback,
JS scroll/retry/timer/retention/poll pause или product renderer change.
Все прочие viewport/Find/noPOST/model-state/oldguards/budgets сохранены.

Root cherry-pick exact QA blob; product unchanged. Node --check/diff-check
только source syntax PASS. Нужны новый frozen combined independent source0,
durable original failure archive/readback и только затем один fresh full CI
нового candidate. Старый a57 FAIL/skip/evidence/ref сохраняется. Fresh actual/
merge/MAIN/public acceptance всё ещё PENDING; LAB и остальные HOLD неизменны.
