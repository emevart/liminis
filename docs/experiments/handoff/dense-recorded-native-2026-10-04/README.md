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
