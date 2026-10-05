# Фактические границы чтения LAB данных

Сохранён original GitHub Actions ZIP без перепаковки и исходный report.json.
Run37252287604, job111582214941, artifact11321850249 завершились **FAIL**.
Diagnostic HEAD `0b52aeec6c6dd07bc2a7b73fe506adcbb12cbf7a`, tree
`e4a1fcca97f7d5f4bbef915858dba49b5c3f0233`; product candidate
`1be0d0940ef75aa00d6152e5b718df0cc5fb446f`, tree
`64bdb5f4794f48907f99886c540f6391571b1914` неизменён.

Четыре conditional false Debugger points на исходном lab-data.mjs подтверждены
по compiled source SHA, native engineHash и possible/resolved positions:
131:25,139:24,142:23,144:4 (строки1-based, columns0-based). Реальных debugger
pauses0. Сохранены16primitive records: entry/finally/pre-release/post-release
каждого из четырёх JSON. Все достигли actual native read done===true, прочли
точное количество байтов, primaryFailedfalse; lock освобождён и cleanupfalse.
Adapter cancel на этом positive path не выполнялся. Это наблюдение под
Debugger; оно не доказывает прежний EOF или внутреннюю C++/GC причину.

Все восемь original HTTP bodies и unchanged continuation ACK точны;
восемь server responses finished. Descriptor50708 B получил native
loadingFailed, canceled:true, ERR_ABORTED; семь остальных loadingFinished.
По проверенному clock interval все три finally/pre/postrelease отношения
к descriptor failure — UNKNOWN_OVERLAPPING_INTERVALS. Same document,
Timestamp/timeTicks alignment и declared1ms precision allowance — предпосылки,
midpoint и Node arrival не превращаются в причинный порядок.

Независимое actual review проверило bytes/SHA/CRC/closed2members, источник,
четыре locations/conditions,16records, clock arithmetic, mapping/guards и
единственный фактический PNG. На нём четыре графика,24matrix,baseline seed1/
sample0; initial/final snapshots ready/unhandled0. PostPNG wait не выполнялся
при actionsPassedfalse; итоговый status остаётся FAIL. Это не остальные
LAB endpoints/layout/keyboard/negative gates и не fullCI/public acceptance.

OriginalZIP399750 B SHA256
`c628ee972f4ea13bc095d5decb9c8c77c542145e693c133ebd83733642b727c0`.
Expanded480156 B,nestedZIP0. Bounded privacy1UTF8member50483 B+1PNG, patterns0;
полной secret guarantee нет. Rawjoblogs/private WorkCloud context не включены.
Raw LAB2/scientific producer21cbe907 не менялись; Cargo/models не запускались.
Manifest задаёт закрытый список размеров/SHA и group cap64MiB. Сетевой Git
readback публикуется отдельным receipt координатора; старые FAIL сохраняются.
