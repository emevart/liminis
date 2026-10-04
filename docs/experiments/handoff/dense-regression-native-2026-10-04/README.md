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
