# Каждое состояние архивной камеры, 2026-10-04

Одна реальная траектория seed42, world30/chamber1, `dt=30 s`, от tick0 до
tick1000000 включительно. Три горизонта сайта используют общие префиксы;
это не три независимых опыта. Старая well-mixed биология и точные значения
сохранены; физические координаты chamber2 здесь не добавлялись.

Чистый producer `bb70c9eb448b55ef3f50dfd6cd2a70213157bd8d`, tree
`93975cb83da179e15c60e937772bf9b656e5279b`, исполнялся на архивном engine
`b2024cef08f0aa910a711a41c6a37fbc7b2ba35a`. Producer опубликован отдельно
в `codex/dense-recording-export`. Его проверка frozen Git objects/версии
предшествует первой записи; запуск этого примера на нынешнем main/world31
намеренно отказывается от генерации. Копия исходника на main нужна для
ревью и сборки, не разрешает переименовать engine identity.

Полный native export завершился один раз, затем независимый последовательный
reader восстановил **1000001** frames. Все **603** общих архивных frames
на **561** уникальных ticks совпали по точным типам/целым/битам binary64,
проверены **826** используемых genotype definitions и полные известные
архивные dictionaries. На каждом из миллиона модельных тиков producer
проверил оба закрытых учёта: максимальные matter/energy residual равны `0`.
Измерения и ограничения RSS — в `full-validation-report.json`;
source/build identity — в `build-provenance.json`. Public command paths
нормализованы, raw report digests сохранены, численные результаты не менялись.
Это observed provenance, не отдельная аттестация runtime binary/kernel.

| Набор | Frames | Chunks | Bytes |
|---|---:|---:|---:|
| Original full dataset |1000001|3909|2421968874|
| Published shared dataset, со всей metadata |1000001|3909|2424602694|

Дельта каждого тика уже записана в gzip; независимые keyframes находятся
в начале блоков не длиннее256ticks. Максимальные gzip/decoded payload:
1046955/4135813bytes. Полный export/pack занял3058.079s, full decode1545.721s
на этой native машине; это не обещание производительности других машин.
Точные u64/i128 строки, f64/-0 и порядок живых клеток не округлялись.

Публикация clean publisher `923ea079a8b26ce4a98a797ea582683708585655`
меняет только пути в index/manifests: `chunks/{ordinal//256:02d}/…`,
максимум256files/directory. Все3909 оригинальных gzip bytes сохранены.
Original index/manifests и coordinator receipt находятся в
`site/data/dense-cell-chamber/original/`; `publication.json` связывает
original/published SHA и полный mapping. Receipt выпущен только после
окончательного PASS, отдельно закреплён SHA и ссылается на полный report.
Publisher reuse не повторяет восстановление миллиона frames (`decoded_frames_here=0`),
но повторно проверяет все gzip sizes/SHA/CRC/inflated SHA и prefix genotype
boundaries. Receipt — доверенное утверждение координатора, не подпись.

External pins:

- Original index SHA256: `44b142721927011cef693ba75bb9867d928d6a1d9cd172f050f2bf54832685b9`.
- Published index SHA256: `c7d78508ccaf00729c20decd595ea0a3256e053a5562d35a58d68748e20b015b`.
- Publication SHA256: `48cf7367c19e3fed7669e0a26473f6ceb02b720d6d0e70d19f140a672aa6368b`.
- Full report SHA256: `af409a80ff61fecd4600c971fac21b3fbc1cb30cbfbdb24f43af225156f88f47`.
- Receipt SHA256: `0cff85b03a18a63efeea14bc617a13dee956ac69b84dfd305b492fba9e7db1bd`.

Старые три JSON, URL и SHA сохранены. `?recording=archive` открывает их
прежнее sparse воспроизведение; default dense observer восстанавливает
реальный запрошенный tick. Старые201samples остаются только явно подписанным
overview и отдельным download. LAB24 outputs/source identity не менялись.

Native source checks и file hashing не доказывают browser/deployment PASS.
Нужны exact-head protected Playwright, независимая приёмка его reports/PNG/
trace и проверка существующего Cloudflare deployment с raw gzip bytes.
Документированные provider limits сами по себе не подтверждают фактическую
загрузку2.42GB. Новая инфраструктура, секреты и платные ресурсы не создаются.

После обеих bounded Git pushes полный dataset скачан по сети в отдельную
Git object database. `network-readback.json` закрепляет опубликованный
candidate97/tree, closed inventory3919files и exact size/SHA256 каждого
файла:2424602694bytes PASS. Последующие QA-only изменения сохраняют dataset.
Existing Workers Build этого candidate SUCCESS; actual public dense served
bytes/playback требуют отдельной приёмки после green CI и merge.
