# Exact-head browser regression evidence

Evidence-only branch, не product release/deploy. Три оригинальных ZIP
сохранены без перепаковки: live cells, physical transport и physical2D
projection на exact e2451e3/tree27c506c/run37231067824. Protected Chromium
report verdict PASS; независимая numerics/regression acceptance qualified
PASS,0 blockers. Exact own review e245-numerics-review.json проверяет все79
checks пяти reports, source pins, timings и неизменность archive/LAB/biology.
Full exact-head CI/Rust и публичный dense deployment пока pending, с отдельным
coordinator checkpoint. Эти regressions не подменяют новую dense QA.

Размеры/SHA — archive-manifest.json. Бounded outer/nested ZIP CRC и text
privacy scan PASS; binary members не проверены как текст, полной гарантии
отсутствия секретов нет. Новые original ZIP network readback ещё pending.

## Public dense main239

Original public-239ec8b-dense-original.zip сохранён без перепаковки:
5601441bytes/SHA688833bc16b805f6433ecd5fb296a3b8e0ec10c6e3481f423b785ae4e9889b48,
exact main239ec8b/tree27c506c/run37232682967. Independent actual artifact
**QUALIFIED APPLICATION PASS,0 blockers**,4checks/2PNG viewed. Exact own
review и3small validations включены рядом, с sizes/SHA в manifest.
Проверены14NodeHTTPS и28browser response pins, actual Next0→1/1×/held state.
Scope: default10k first/prefetched chunks; полный2.42GB public byte readback,
public100k/1M endpoints и analytics execution не заявляются. Известная pinned
edge insertion сохранена в rawHTML;2analytics Script GET заблокированы с ACK.
Observed initial stale/404 responses и один5s deployment retry сохранены.

Четыре original ZIP теперь45622306bytes<64MiB. Первые три имеют завершённый
fresh network readback на0541137; добавленный public original/reviews требуют
нового incremental network readback в ту же независимую Git database.
Main Rust ещё automatic pending на момент записи; PR19 exact fullCI принят.
