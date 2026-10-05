# Независимое source review LIVE mobile readout

**QUALIFIED SOURCE PASS, 0 блокеров.** Проверен frozen HEAD `a57a6854b633c8e272eb4012ff11a0ec125e6fc2`, tree `2f9f0760ada0bbd1812a232bc6a30883242c76d3`, база `813be1efad29569a3011f00697bd5d41b7397dc6`. Checkout чистый. Этот verdict допускает один protected exact-head CI; browser acceptance и merge ещё не приняты.

Изменены ровно четыре согласованных пути: HTML viewer, существующий LIVE QA script, append-only ACCEPTANCE и новый checkpoint. Final product/QA файлы побайтно совпадают с frozen author commits `f939521c7999f5f2d226db462c4d0ad2900eb877` и `a973b25b47b7bf66330d280a199aee4b1544f07b`. Полные исходники и diff были прочитаны; финальная docs-only поправка a57 проверена отдельно. Reviewer не автор продукта, QA или root docs.

| Файл | Bytes | SHA256 |
|---|---:|---|
| `crates/liminis/src/cell-viewer.html` | 62157 | `cf12bae4142980b513375a3a173c4249e457d58e760a366b90153c676446d148` |
| `docs/ACCEPTANCE.md` | 161664 | `209d1d7b74594d23adfe0515fb18a40e7b338a9fd41f6f4039ba955c74d0fc9e` |
| `docs/checkpoints/2026-10-05-live-mobile-readout.md` | 6006 | `05dc7f2d716105e9145ca8222911ef2e121a1e9794d961e58f479b3b14cd9ae9` |
| `scripts/check_live_cells_browser.mjs` | 35636 | `691aa72ec7145a21b9c22df22394f5c47ac59d192254157902960ac56bd81ac0` |

Изменение продукта находится только в mobile media block <=800px. Уже существовавшие physical normal-flow rows теперь применяются и chamber1: stage, static readout, затем side. Narrow finding о прежних `bottom:0; z-index:8` закрыт author follow-up; обе декларации сохранены. Stage height/minimum rules прежние, включая physical minimum560. Desktop CSS, <=520px специальные rules и весь DOM/IDs/JavaScript вне блока побайтно прежние.

Это доказано сравнением неизменного prefix8990B и suffix52090B. Core/config/site, CI, frozen SPEC/NORTH_STAR/DECISIONS, host, 3D renderer/vendor и projection/GL QA имеют прежние Git objects. ACCEPTANCE сохраняет точный старый prefix159102B и добавляет2562B. Shared HTML целиком изменён: применим change-impact bridge с explicit CSS delta, прежними dependencies/guards и свежими actual GL/projection gates. Whole-HTML identity к старому e2 или новый independent GL artifact PASS не заявляются.

QA сохраняет прежние gates/budgets и расширяет paused responsive phase. Cases1440/590/420/390/320 используют настоящие rects, полный readout text, clipping ancestors, native elementFromPoint и scroll. Find absent→present проверяет exact saved API ID, восемь общих primary inspector fields, genome/generation и последний нижний phenotype field; все xyz/phenotype values по отдельности здесь не assert-ятся. Полные bounds и scroll проверяют доступность верхнего/нижнего inspector. Resources/events совпадают с actual API с принятой display precision.

Request baseline ставится до resize. Новые окна допускают только same-origin GET state/history; дополнительные POST запрещены. Bounded1024 request capture даёт sticky failure при переполнении. Final raw model fields, включая cells/coordinates/quantities, глубоко сравниваются с paused API snapshot. Responsive650/4000ms — source constants и сохранённые timestamps, не новый measured-cadence assertion; прежний actual30/60 polling guard сохранён.

Прежние atomic Step, pacing/manual speed/invalid speed, held state/pixels, Maximum, Save acknowledgement, official launch/sandbox/source/error/cleanup guards и120s budget не ослаблены. Новая нормальная phase ожидает35 layout/full-page PNG и2 старых held PNG; это ожидание исходника, изображения ещё не приняты. Сохранённым guards не приписываются новый eight-option assertion, native unhandled sampler, comprehensive post-cleanup aggregate или новый browser-body SHA oracle. Save acknowledgement не доказывает restart/resume.

Три замечания закрыты: сохранение physical stacking, честные inspector/cadence claims и qualification общего HTML/GL bridge. Последние docs корректно оставляют CI/actual/archive/merge/main/public PENDING. Проверка была read-only: browser, HTTP, tests, CI, Cargo и модели не запускались; source/Git не изменялись.

Остаются обязательными:

- Один full protected exact-head CI со всеми mandatory browser steps.
- Новая независимая actual chamber1 mobile/desktop и physical regression с original PNG/trace/request/state/error/cleanup proofs.
- Fresh actual GL/projection gates под квалифицированным change-impact bridge.
- Closed durable affected originals/reviews archive и normal network readback до guarded merge.
- После merge отдельные fresh MAIN regression и existing public gate.

Предыдущие PASS остаются привязаны к прежним HEAD/tree/run. Исторический5bea encoded-PNG FAIL сохраняет неизвестную причину; LAB21/publicLAB/LAB4/models/causal retries сохраняют HOLD. Нет claims о hardware FPS, новой научной валидации, полной public dataset delivery или exhaustive secret scan.
