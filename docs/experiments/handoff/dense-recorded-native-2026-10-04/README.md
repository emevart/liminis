# Native dense recorded browser evidence

Evidence-only branch, не product release/deploy. ZIP сохранён в оригинальных
байтах, без перепаковки. Exact candidate97aed6f/treef6feff8/run37228121227
имеет **overall FAIL** и не принят к merge.

Все11 отдельных dense checks и5actualPNG прошли: реальные исходные frames,
original gzip/negative controls, measured1×, buffering255→256 и horizons до
1M. Итоговый gate выявил3неатрибутированных `net::ERR_ABORTED` при lifecycle
reader/prefetch. Полный успешный браузерный verdict из этих subchecks не
выводится. Нужны narrow source fix, новый exact-head protected run и новая
независимая artifact acceptance. Старый FAIL остаётся диагностикой.

Независимый reviewer проверил source pins, report/trace hashes/CRC, весь5PNG
и bounded text privacy scan:20UTF8textfiles/22335115bytes, obvious credentials/
private-chat/old-VM markers0hits;111binary members не проверены как текст.
Это не полная гарантия отсутствия секретов в binary content. Public runner
paths не являются private Work VM context; raw job logs сюда не добавлены.
Сохранены честные ограничения heap/performance/native background/source
response-body coverage. Trace не сохранил26responsebodies, их byte attestation
не заявляется.

Original size/SHA и overall verdict — в `archive-manifest.json`.
Manifest фиксирует границу записи, окончательный fresh network readback
фиксируется отдельным coordinator checkpoint. Будущие successful ZIP имеют
свои exact HEAD/run/verdict и не заменяют этот файл.

## Свежий e2451e3 browser artifact

Exact `e2451e3142def791eb44aba4fcb19ada1aa1ed78`, tree27c506c,
run37231067824: dense14checks PASS, независимая actual acceptance
**QUALIFIED PASS,0 blockers**. Проверены original ZIP/source/trace/5PNG,
signal-only native rejection control и post-cleanup request aggregate.
Own review и квалификации сохранены exact в e245-independent-review.md/json
и двух малых validation JSON. Original новый dense ZIP и отдельный22-check
legacy ZIP сохранены без перепаковки; original97 FAIL остаётся отдельным.

Full exact-head CI/Rust, independent legacy/regression review и публичный
dense deployment требуют отдельных итогов; эти PASS не переносятся на main.
Fresh network readback новых original ZIP ещё pending. Manifest содержит
точные sizes/SHA и разные acceptance boundaries, общий ZIP budget45.3MB.
