# LIVE mobile readout: итог передачи

2026-10-05. PR #24 завершён в `closed`, без merge. Exact candidate
`000e2eb6b6d42b81f4cee19eaa23e3f09c917ee6` / tree
`a8cdcc995fe3f6c6ec130ff5400b81d733c55286`, run `37284723150`, окончился
FAIL: 10/11 checks и 9/10 CI jobs прошли, LIVE браузерная приёмка остановилась
на desktop 1440 Events clipping assertion (`clipped=true`). Exact geometry
target/ancestor, axis/amount и сохранённый результат evaluation отсутствуют.
Mobile, physical viewer, projection и Three.js actual gates не исполнены.
Причина и безопасный repair не установлены. Astra постановила
`HOLD_NO_SUPPORTED_REPAIR`; третий full CI или новый diagnostic runtime для
этой границы сейчас не разрешены.

Оригинальная трасса второго отказа и отчёт сохранены без перепаковки в
`codex/live-mobile-second-failure-evidence-20261005`, commit
`95eacbbff7d7c3ef7b88250a92d297859ae18ff8`, tree
`4ee3e3ca747a0937e06a2b0ba306dec857474a6e`. Закрытая опись включает 13 файлов
и manifest `32fa5a6dcfccb5e3491538175843a024a898d50976db9bdcfdc2dfe077f727ad`.
Отдельная Git network readback прошла: каждый опубликованный размер/SHA и
оригинальный outer/nested ZIP CRC проверены. Первый run сохранён независимо
в `codex/live-mobile-failure-evidence-20261005`, commit
`052c8fa285b54a0fc6b1228e6c5e0438550476cd`, с уже пройденной readback.
Оба архивных commit, manifest, run receipt, readback receipts и Astra decisions
закреплены sidecar-файлами в
`docs/experiments/handoff/live-mobile-readout-coordination-2026-10-05/`.

После закрытия PR документальный checkpoint опубликован на
`codex/live-mobile-readout-integration`: HEAD
`8a8d5a857e586dc1ffe2177ac977644caff44388`, tree
`27cbb0f3662c3fc599fc019de6910a2e6bba7f5f`. Проверенный product candidate
остаётся родителем `000e2eb6…`; финальный checkpoint не является новой
приёмкой продукта. `main` и deployed site остаются на принятом
`813be1efad29569a3011f00697bd5d41b7397dc6`. Не было merge, deploy, новой CI,
модельного запуска, пересчёта записей каждого тика или изменения raw24.

Любое продолжение требует отдельного bounded scope: сначала сохранить actual
Events target/ancestor geometry до assertion, затем независимый review. На
сегодня mobile readout остаётся HOLD; физическая 2D/Three.js функциональность
на принятом main не менялась.
