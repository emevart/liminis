# Независимая приёмка regression originals83fa

**Qualified actual PASS, 0 blockers** для двух loopback regression artifacts из run37260833244/job111607516475, exact `83fa1e52e478e2a33602a24bf83b5456e55e70a3` / tree `47ce45646315b09493d58d87ca8fce4dc867aa8d`. Это не overallCI GREEN и не приёмка отдельного actual3D gate.

Original recorded ZIP: 12,519,814 B, SHA `97564a5031f8488f8f2b5a6bf7412489d42d7dd16ff5251dca8584529a13e3c5`, closed10/CRC0. Dense ZIP: 16,594,545 B, SHA `f46a89196890f683d771d6310fd4e12c411993dd190b3a6fb681f588944e4bbc`, closed7/CRC0. Extracted bytes exact originals. Nested traces closed218/135 members, CRC0, expanded54,392,736/34,295,778 B; их source QA bytes совпадают с committed objects. Все 8 legacy и20 dense source pins сверены с exact head. Traces показывают paired421/574 actions,14/5 page closes, только loopback GET, без action errors.

**Все 13 actual PNG просмотрены через view_image**:8 legacy и5 dense. Legacy три overview/short/mobile390/320 показывают исходные saved frames, controls и ограничения. Held30/60 original PNG byte-exact и whole RGBA SHA `fa97cfb15a1e082f0879000243be8d06e0fc17ae8f9beb5d3bc5f8680af1225f`. Dense desktop/mobile показывают actual tick992, explicit missing ID, schematic inventory и sparse201 plots; endpoint PNG показывает tick1,000,000 и recording ended. Spatial projection не заявляется.

Legacy22 checks и17 HTTP checks PASS:3 actual archive downloads совпали с pins, controls/hold/endpoints не добавляют HTTP, unknown experiment/digest corruption/unsupported schema отказывают. Page/console error checks и trace clean; отдельный raw browser-unhandled array этот legacy gate не записывает, такой zero count не выводится.

Dense14 substeps PASS:101 original frames, реальные native gzip corruption/abort controls, one-payload cache, paused atomic255→256 с delayed original chunk, реальная UI всех трёх горизонтов и endpoints100k/1M. Retained original data bodies сверены с Git objects; delayed fulfillment917,524 B exact SHA `24e05e658b8658d276a4af43d8e03c76c7398a2a08fd5da5640db73a57819de1`. 1× advance1.523 s согласуется с measured1523.3 ms (delta−0.0003 s); это bounded runner measurement, не hardwareFPS/peak heap claim.

Все9 dense ERR_ABORTED сохранены и независимо проверены:4 explicit signal cancellations и5 exact full-body EOF/Content-Length-before-default-abort; уникальные native/CDP/PW IDs, совпадающие occurrences и final bijection,1 ms precision allowance явно раскрыт. После cleanup final classification PASS, native/page/console/Node/cleanup errors0. Fresh signal-only phase не ставит observer Promise handlers/reader wrappers; productionUnhandled0 и ровно1 object-identical labelled synthetic sensitivity event сохранены. Generic aborted-request exemptions нет.

Bounded privacy: legacy14 UTF8/46,618,687 B и dense28 UTF8/39,922,293 B, включая8 gzip resources с16 MiB cap каждый. Obvious credential/private-context/signedURL/privateIP/negative-chatID patterns0; actual network cookies/auth headers0. Это ограниченная проверка известных признаков, не полная гарантия отсутствия секретов; trace JPEG frames отдельно не просматривались.

Reviewer не запускал browser/HTTP/models/Cargo/tests и не менял originals/repository. Принята только текущая bounded loopback regression scope: не full public2.42GB readback, не новая science/geometry, не перенос старых PASS. Fresh fullCI/Rust, остальные LIVE/actual3D originals reviews и durable archive/readback обязательны. LAB PR21/publicLAB/models остаются HOLD; historical main5bea FAIL неизвестной причины.

[Полные pins/inventory/observations](review.json).
