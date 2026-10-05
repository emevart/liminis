# Отдельная диагностика исходных LAB ответов

Сохранён оригинальный ZIP GitHub Actions без перепаковки и исходный report.json.
Диагностика завершилась **FAIL**, продукт не принят. Run37250700549,
job111577533759, artifact11320133411. Diagnostic HEAD
`edca9b9fa16a11bb24317dead67362515b9d1e00`, tree
`5e2d67d3c43cbdb2e3f9992b7114f99a97f25cf5`; product candidate
`1be0d0940ef75aa00d6152e5b718df0cc5fb446f`, tree
`64bdb5f4794f48907f99886c540f6391571b1914`.

Все восемь original response bodies совпали с неизменёнными Git assets по
размеру и SHA. После чтения CDP продолжил исходные ответы без overrides.
Все восемь серверных ответов завершились; семь native Network requests
loadingFinished, results.json после полного dataReceived — loadingFailed,
net::ERR_ABORTED, canceled:true. Причина отмены и Reader EOF не наблюдались.
Chrome timestamp и Node arrival различаются; их нельзя смешивать в причинной
шкале. Response-stage pause меняет timing.

Независимое ревью проверило закрытые два ZIP members, CRC, extracted bytes,
source/head/tree, восьмерку body pins и server/network/Fetch соответствий,
observed sandbox argv и единственный фактический PNG. На нём четыре графика,
24 строки matrix, baseline seed1/sample0; начальный и финальный snapshots
ready, unhandled0. Это не оставшиеся LAB endpoints/layouts/negative gates.
Post-PNG ready wait не выполнялся при actionsPassed:false; итоговый proof
сохранил FAIL. Guard остался RUNNING, не переименован в PASS.

Оригинальный ZIP397681 B SHA256
`b4a3fdb5ca151f839f7f8fc87adefae6cb1aa8fd1bf0c85b9aaff1faa8dd7036`.
Privacy review ограничен одним UTF8 report36031 B и просмотром одного PNG;
credential/private-context patterns0, полной гарантии секретов нет. Raw joblogs
и private WorkCloud context не включены. Восемь immutable assets и workflow
остаются доступны в diagnostic ref; raw LAB2/scientific producer21cbe907
не менялись и модели не запускались.

Manifest задаёт закрытый список, точные sizes/SHA и group cap64MiB. ZIP expanded
465704 B, nested ZIP0. Независимый сетевой Git readback фиксируется отдельным
receipt координатора; сохранённые FAIL не заменяются последующими результатами.
