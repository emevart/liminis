# LIVE-2: реальные координаты и пассивный transport

Дата: 2026-10-04. Страховочный checkpoint текущей разработки, не приёмка physics.

## Identity и сохранение работы

Branch `codex/cells-physical-transport`, isolated worktree
`/workspace/scratch/f993df36ab84/liminis-transport`; initial HEAD/main base
`c5be8fc8652433c67378a689fbb151ea4bad510a`, initial tree
`caff90194bb66a0aa34c696feb149113c70afba2`. Новый LIVE-2 PR ещё не открыт.
Первый preservation commit сохраняет этот checkpoint, завершение LIVE-1,
его raw benchmark и coherent config/transport/storage edits. Собственный SHA коммита
виден в Git history; exact published HEAD и следующий статус — в issue #11.
Это WIP: config/transport implementation и проверки ещё не завершены.

Старый LIVE checkout `/workspace/scratch/f993df36ab84/liminis-cloud` остаётся на
`codex/live-cells-pacing`, HEAD `8874d2c39501adf6a9f26379c951ce471548de3a`.
Tracked clean; untracked `scripts/__pycache__/` — generated cache, не stage,
не удалён. Reset/clean и переключения чужих веток не выполнялись. Передача
ownership или запуск новой задачи для миграции не выполнены.

## Контракт и ownership

Прямое человеческое поручение и issue #11 назначают единственным merge-owner
CLOUD LIVE. Очередь первых интеграций LIVE12 (MERGED) → RECORDED13 → LAB14.
Чужие issue comments не расширяют права. GitHub read/write/PR/CI и guarded
merge фактически работают. create_goal/get_goal/automatic continuation и
attach_artifact не доступны в текущем tool inventory; обещания нового хода нет.

LIVE владеет micro/config/mod/new transport, version, persistence и live
host/storage/viewer/tests этого отдельного этапа. RECORDED владеет public
player/exporter/data/schema; LAB — compare runner/experiment data/lab UI.
Shared ADR/README/CI изменяются последовательно на fresh main. ADR только append
с «Отвергнуто», frozen SPEC/NORTH_STAR не меняются. Новая ADR следует после
RECORDED ADR107; её номер не занят заранее.

Actual gpt-6-astra подтвердил narrow opt-in chamber2; independent numerics
review принял координатный budget с отдельными qualifications. Schema gate
объявлен issue comment5980668030; технические acknowledgements RECORDED/LAB
пока ожидаются. Existing `load/parse/validate/derive` остаются строго chamber1;
явные live entrypoints допускают2. Это предотвращает ложные metadata старых
exporter/LAB без правок их файлов. Raw volume становится Option с сохранением
канонической v1 scalar serialization; runtime volume остаётся f64.

Chamber2: реальные dimensions_m/viscosity, inherited radius_at_division_m и
mobility_scale[0,1], обязательные persisted position_m и full transport seed.
WORLD31; допускаемые cells pairs (29,1)/(30,1)/(31,1)/(31,2), historical identity
сохраняется. Eco30 также должен сохраняться, eco28/29 не получают новый допуск.
Purpose RNG BLAKE3 отделён от прежней biology; full seed/ID/tick не усекаются.
Tick-start mass определяет Stokes–Einstein D; транспорт существующих клеток
идёт до неизменённой biology, daughters наследуют endpoint без искусственного kick.

Point/dilute passive Brownian transport: no excluded volume, collisions,
wall drag, local chemistry или adhesion. Среда остаётся общей well-mixed.
Это частично отменяет ADR099 только для opt-in2 и требует явных узких исключений
из ADR015/022 и SPEC12.2/12.3 в новой ADR; S1′ не объявляется принятым.

Numerics: FMA endpoint, triangular reflecting walls, outward S≥L+13σ,
B=upward_sum(ulp(S),ulp(2L)); B·2^32≤L и B·2^11≤σ. Unsupported finite range,
positive underflow или exhausted bounded Gaussian attempts отвергают целый tick;
dt/D не меняются и coordinate clamp отсутствует. D0 сохраняет coordinate bits.
Budget касается computed σ/z arithmetic; не доказывает native ln/cbrt accuracy
или long-term drift. Second-moment bound <0.1% относится к свободным increments
до walls, Gaussian variance проверяется отдельно. Exact resume принимается на
фактическом disk/to_bits/future evidence pinned build, не cross-platform claim.

LIVE2 viewer сохраняет inventory schematic с постоянной явной подписью,
что layout не physical coordinates. API публикует настоящие позиции и размеры2;
2D/Three.js views только отдельным PR после принятия physics.

## Активные авторы и текущие проверки

Внутренние существующие авторы (не продублированы): `host` — config/API;
`acceptance` — transport/mod/его tests; `storage` — live adapters/disk tests.
`astra_advisor` — фактический советник; `transport_design_review` — независимый
численный review; `review` — независимое code/artifact review; `viewer` — готовый
LIVE1 browser author. Root координирует и единолично запускает Cargo.
На preservation boundary собственных Cargo/rustc/live host/browser процессов нет;
чужие процессы не остановлены. Все три production автора paused после coherent
bounded edits для snapshot: config/API + v1 golden fixture; transport/mod +
начальные rational reflection/FMA/budget tests; actual chamber identity storage.
Dirty scope до stage: micro/config.rs, micro/mod.rs, live host/storage/storage_tests,
оба LIVE checkpoints; untracked new transport.rs/transport_tests.rs/test-data
fixture и docs/experiments/live-pacing/*. Все перечисленные scoped изменения
сохраняются; generated cache и ignored QA directory не stage.

LIVE2 tests/benchmark/browser/CI: NOT RUN, PR NOT OPEN, physics не PASS.
Pending: config golden compatibility, transport/statistical/high-precision/walls
tests, projected biology equivalence, exact actual old/new disk resume, API/save
responsiveness, honest named release D0/Dpositive benchmark, independent full
code/numerics review, ADR/version/operational docs, fresh-main final-head CI.

RECORDED13 fresh candidate `f9fb7a2c6055620030f9077bf8eebc6ce9d7ef49`, tree
`b54271621bc4fe3bda95af3c0ffb49e5c281e136`, base c5be8fc; run37206398277:
protected browser job111448421541 FAIL (22checks/3 download-request assertions),
Chromium sandbox successfully launched, trace saved. Full build test pending at
snapshot; previous green heads не переносятся. Автор исправляет; root не merge.
Ignored diagnostic evidence old LIVE checkout `target/qa/recorded-f9fb7a2/`:
artifact11305036205/report/screenshots/trace, not in Git, retention7days gap.
https://github.com/emevart/liminis/actions/runs/37206398277/artifacts/11305036205

LAB14 code/numerics accepted, exact `21cbe90732128edee61963544e414d29a7420577`,
tree `713751fb97b2f53206fc0a86c368e63c28d1da65`, old base f7c74a, run37203165206
green. Fresh-main rebase/final CI remain before merge after RECORDED. Authorized
dependent LAB2 preregistration `9f49a2438be187b446556b1291b6264f0e3ea5c8` pins
accepted chamber1 source; results/independent validation pending. No new format
admission silently granted. RECORDED2 bounded/chunks remains separate.

Ignored scientific benchmark raw JSON/script now copied unchanged to Git under
docs/experiments/live-pacing; original target/qa copies remain. Protected LIVE1
PNG/trace remain Actions artifact11303903150 and ignored local directory, not in
Git. Reproduction and all LIVE1 exact tested SHA/evidence recorded in its checkpoint.

## PAUSED FOR HANDOFF — физика не завершена

Последнее прямое человеческое указание: контролируемая передача coordinator/release
ownership в созданную пользователем native Cloud задачу. Новый координатор пока
READ-ONLY, write-go не получал. Этот checkpoint фиксирует остановку прежнего LIVE,
не принятие LIVE2. Новые этапы/назначения, merge, deploy, auto-merge запрещены
после этого pause до сообщения нового координатора. RECORDED/LAB — независимые
авторы, их полезные проверки этим pause не отменены.

Последний опубликованный WIP перед final snapshot:
HEAD `090dc94dc0eb25a8a0a9ff0fae81e960f9ba99cc`,
tree `23bd39cb3af3abf7ab948ed96503dd449add3bf9`,
branch `codex/cells-physical-transport`, base
`c5be8fc8652433c67378a689fbb151ea4bad510a`. Final preservation commit включает
весь scoped diff ниже. Его точный HEAD/tree публикуется в последней STOPPED
записи issue11 и виден в Git history; документ не содержит невозможную
самоссылку на свой будущий SHA. LIVE2 PR не открыт, branch push не запускает
CI (workflow реагирует на main push и PR), main не изменялся после LIVE12 merge.

Сохранённые изменения после090: config opt-in tests и unconditional finite2L
guard; transport tests/statistics/rational fixtures и atomic-refusal fixture fix;
WORLD31; eco30 admission + actual disk future test; physical scenario;
host/storage API/version/seed/disk tests; честная inventory schematic caption;
optional physical browser acceptance в существующем live script. SPEC/NORTH_STAR,
DECISIONS, CI, Cargo.lock, RECORDED/LAB owned files не изменены этим snapshot.
ADR ещё НЕ добавлена: обязательно append следующего свободного номера после
fresh-main RECORDED107, с «Отвергнуто» и уже названными исключениями/semantics.

### Реально выполненные проверки

Один последовательный Cargo-процесс, pinned Rust1.97.1, общий existing target dir
старого LIVE checkout. Команды выполнялись из нового physical worktree:

```
cargo fmt --all
CARGO_TARGET_DIR=/workspace/scratch/f993df36ab84/liminis-cloud/target cargo test --locked -p liminis-core micro:: -- --nocapture
CARGO_TARGET_DIR=/workspace/scratch/f993df36ab84/liminis-cloud/target cargo test --locked -p liminis cells_host -- --nocapture
```

Core micro: **42 PASS, 1 FAIL, 1 ignored**, 2.80s. Golden v1 bytes/hash,
opt-in/2L validation, full counters, projected biology/report balances,
zero-mobility bits, daughter inheritance, rational reflection/FMA/budget,
Gaussian/6Dt multilag before walls и reflected-uniform ensemble tests PASS.
Fail: `unsupported_numeric_range_rejects_entire_tick_after_earlier_candidate_motion`:
fixture изменял отдельной клетке неизменяемый inherited spatial genome, поэтому
state validation останавливал её до transport. Автор исправил fixture:
одинаковая mobility1e-14 для всех и большая масса поздней клетки; проверяются
valid initial state, успешное движение first-only candidate, полный cause
numeric-budget refusal и rollback. **Исправление NOT RETESTED** после human pause;
его фактический результат не выдумывается.

Live host/storage: **31 PASS, 0 FAIL**, 3.49s. Включены actual old/new disk
resume/to_bits (minsubnormal, -0, wall-adjacent), fullu64 seed, future после64ticks,
повторный resume, все cells29/30/31 identities1 без upgrade, chamber2 pair31,
реальные позиции API и старые pacing/backpressure/manual/max/step tests.
Новый отдельный eco30 actual disk future test **NOT RUN** (фильтр cells_host).

Final `git diff --check` PASS. Formatter выполнен до последнего atomic-fixture
исправления; final fmt check не повторялся. Browser script author syntax/diff
PASS; физический browser script НЕ запускался. Existing CI ещё НЕ wired для
второго physical invocation. Нет release benchmark D0/Dpositive, final independent
code/numerics review, full necessary final-candidate CI или physical browser QA.
Physics/UX acceptance **NOT PASS**. Полная локальная full suite не повторялась.

Actual Astra final design approved; independent numerical initial090 audit не
нашёл algorithm blockers, потребовал finite2L даже D0 — исправлено и core test PASS.
Final candidate после всех edits пока independently не проверен. Доказательство B
не относится к coefficient/transcendental/longtime error и не даёт post-wall6Dt.

### Остановка авторов, процессов и queued writes

Все production авторы закончены на coherent boundary. `acceptance` явно ACK
STOPPED с сохранёнными файлами и NOT RETESTED fix; `review` явно ACK STOPPED.
`host`, `storage`, `viewer`, `transport_design_review` уже completed, им направлен
STOP, interrupt подтвердил completed. Никаких их writes/processes не осталось.
Завершившийся actual Astra advisor отсутствует в active registry (`not_found`
на interrupt), новых вызовов не назначено. Root завершил только уже запущенный
host Cargo; exit0 получен. Собственных Cargo/rustc/livehost/Chromium/Playwright
процессов нет, queued writes0, future LIVE dispatch0, merge/deploy0. Auto-merge
не включался: PR12 closed/merged, PR13/14/15 `auto_merge:null`; LIVE2 PR отсутствует.
Goal/automatic continuation tools отсутствовали и ничего не создавалось.

Dirty before final stage: 13 modified scoped source/test/script files плюс этот
checkpoint; untracked `configs/scenarios/cell-chamber-physical.toml`,
`micro/config_spatial_tests.rs`, `docs/experiments/live-pacing/http-acceptance.json`.
Все полезные scoped изменения final snapshot опубликованы. После publication
новый worktree tracked/untracked clean. Старый LIVE checkout остаётся HEAD8874,
единственный untracked `scripts/__pycache__/` generated cache сохранён, не stage.
Private-only source/secret changes0; ignored build/QA и downloaded artifact ZIP
сохранены отдельно, не удалены. Чужие процессы/checkout не менялись.

### Artifacts: сохранность и bounded archive proposal

Raw supplemental benchmark уже в Git `occupied-seed42.json` (SHA256
`3cce6e6a62c4668b3fb999cf0d334fcc356fd9015cc303d5d7f460af3787c153`) и script
`benchmark_occupied_cells.py` (SHA256
`8f62689975e92722aea961c328b3c940dfbeecb46cde8817772c53c9493c78a4`).
Дополнительно raw HTTP acceptance592bytes сохранён в Git как
`docs/experiments/live-pacing/http-acceptance.json`, SHA256
`67ffbf21a136f8077b2cff227a02f5cef254bd6daeab1b37ecebf51deb862e97`.
Этот исторический raw report не содержит собственного HEAD/binary hash;
он не является приёмкой LIVE2 или нового final commit. Исходные target/qa copies
и старый release binary не удалены.

Старый LIVE checkout ignored inventory:

| Path under target/qa | Contents / bytes | Durable source / limit |
| --- | --- | --- |
| protected-8874d2c/ | accepted JSON/4PNG/trace/log, 9files / 7,090,095bytes | run37204679284 artifact11303903150, ZIP3,997,658bytes, expires2026-10-11T13:12:15Z |
| recorded-f9fb7a2/ | failed diagnostic report/8PNG/trace, 10files / 14,116,427bytes | run37206398277 artifact11305036205, ZIP12,845,994bytes, expires2026-10-11T13:40:42Z |
| actions-live-browser/ | superseded default-sandbox evidence, 8files / 956,968bytes | historical run37201843089/artifact11302379823; НЕ protected PASS |
| browser-cells/ | local blocked/diagnostic attempts, 15files / 643,019bytes | reproducible gap; НЕ browser PASS |
| occupied_cells_benchmark.json + benchmark_occupied_cells.py | 22,621 +3,056bytes | exact copied Git artifacts above |
| live-cells-pacing.json | 592bytes | exact copied Git artifact above |

Downloaded Actions ZIP also remains outside checkout in workspace attachments;
restore public bytes via artifact IDs rather than private workspace handles.
CI scripts reproduce scenarios, but screenshots/trace timestamps and byte hashes
не обещаны детерминированными. PNG/report/trace НЕ saved-to-Git: bounded retention
— явный durability gap. Ничего не удалялось и security protections не менялись.

Предложение новому координатору после write-go: один отдельный evidence-only
GitHub archive commit/PR в существующем repo, ≤32MiB total и ≤20MiB per artifact,
только exact-head reports/PNG/trace ZIP + INDEX с SHA256/source/expiry/status;
проверить отсутствие private identifiers/secrets, не включать caches/temp dirs.
Начать с accepted LIVE1 и corrected REC1, failure ZIP при необходимости сохранить
в оставшемся bound. Это постоянный Git archive без новой интеграции/infra,
без release/deploy и без изменения browser sandbox. **Сейчас archive не создан**:
остановка ownership/dispatch соблюдается, artifacts можно скачать до expiry.

### Следующая очередь для нового координатора

1. Read issue11 и exact candidates; REC13 теперь HEAD
   `f72fe98e50b4e6dca216421998213ff978d6b35a`, tree
   `f75d48bce5870565483e96f049ce64c364456dad`, basec5, draft. Author сообщает run
   `37207963950`, recorded job `111453040333` SUCCESS,22/22browser+17/17HTTP,
   artifact11305057269; root не выполнил independent final technical artifact
   acceptance, full build/test ещё pending по последнему author report.
   Independent root reviewer earlierf9 source/security/actualscreenshots:0extra
   blockers, но это не перенос приёмки на новый HEAD. Merge до всех exact gates нет.
2. LAB14 accepted oldhead21c/basef7; rebasefreshmain afterREC13 и finalCI.
   LAB15 dependent headff402c/tree9adc/baseLAB14, draft8newfiles; исправления
   review/validation pending. Independent authors НЕ остановлены этим LIVE pause.
3. LIVE2 восстановить branch+checkpoint, сначала retest исправленного atomic
   fixture, затем targeted suite/finalfmt/eco30/bench/browser/review/ADR gate;
   read shared schema acknowledgements (RECORDED ack5980834090, LAB pending).
   Rebase sequentially freshmain после интеграций, не смешивать чужие PR.
   Physics принять только с фактическими numerics/statistical/disk/HTTP/CI gates.
4. После physics отдельный LIVE3 actual2D/Three3D, recorded bounded chunks и LAB UI
   по issue dependencies; никакой concurrent chemistry/GRN/S1′ semantic rewrite.

Это final pause для handoff. Выполненные LIVE1 и WIP LIVE2 сохранены; завершение
физики, новый write-go, merge или deployment этим документом не объявляются.


## Native coordinator: продолжение LIVE-2

Сохранённый d536234 retested в native environment: focused micro43PASS/0FAIL/1ignored,
host31PASS/0FAIL. Затем только два acceptance fixture расширены: absolute SI
Stokes–Einstein oracle и eco30 advance/save/second resume exact future. Оба targeted
test PASS. Независимый numerics-review a4dfdf3 — 0 blockers; oracle literals
отдельно пересчитаны 90-digit Decimal, relative literal error <7e-17.

Fresh RECmain3b66c2cb интегрирован без переписывания WIP истории. ADR-108 append-only
после107 согласован Astra: узкое CPU/native-f64 исключение015/022, opt-in частичная
отмена099; strict chamber1 exporter/LAB и агрегатная NDJSON история остаются прежними.
SPEC/NORTH_STAR не менялись. Clippy выявил два equivalent range patterns и loop
в test; исправлены без numerical semantics изменения. fmt и полный
clippy --locked --workspace --all-targets -- -D warnings PASS.

Явная последовательная cargo build --locked --release -p liminis выполнена root
на чистых tracked source3489d1647e2cf1f27d382d3f6c61d24aec2e27a3.
Named occupied D0/Dpositive release benchmark сохранён без перерасчёта:
docs/experiments/live-transport/occupied-seed42-release.json и reproduce script.
D0:1182actualticks, observed living8–222, HTTPmax41.51ms/Pause6.71ms;
Dpositive:2092actualticks, observed living8–242, HTTPmax226.71ms/Pause80.68ms;
оба ниже объявленного2sbudget, после Pause tick не меняется, checkpoint ack exacttick.
Target1000 означает остановку после первого observed crossing, не exact horizon;
HTTP/control scheduling даёт overshoot. Это sampled inventory и одно hardware/build,
не integratedcellwork или controlled D timing comparison.
D0firstStep changedpositions0, Dpositive8; actual per-tick residuals exactzero.
Final necessary CI/full review/default+physical protected browser acceptance ещё PENDING.
Новый physical invocation добавлен последовательно в existing live-browser job;
official runtime/sandbox/CDP guard прежние, artifacts разделены по format.
