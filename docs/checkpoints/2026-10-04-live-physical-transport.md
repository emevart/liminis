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
