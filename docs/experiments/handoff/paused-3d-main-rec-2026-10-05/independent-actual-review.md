# MAIN REC+dense+PUBLIC: independent actual review

Qualified PASS, **0 blockers**, для трёх новых оригинальных artifacts run `37263650472`, recorded job `111615835795`, MAIN `e2fd620baa492b2c2ff4316d2f083ac3e78bc252`, tree `47ce45646315b09493d58d87ca8fce4dc867aa8d`. MAIN tree независимо совпал с 83fa. Старые actual PASS не переносились; общий CI, Rust/LIVE/GL и LAB сюда не входят.

Побайтово перечитаны три original ZIP. Сумма — 34,929,122 bytes.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| REC | 12,616,518 | e6a5d33458fff1594349ca08c2fb1a22de5f93e8b4abff4a87e9daa8566cca53 |
| Dense | 16,608,101 | c81d16c90616d57d6da404abc127082b4b3bb3a876714b34530c462def6633b2 |
| PUBLIC | 5,704,503 | a0af87ecffc69d9138a5f0cd97a886d1ca24c8a46d4e1669adea44d4b597a4ff |

Outer и nested CRC, inventory/path/duplicate/symlink guards, extraction byte equality и report/source/run/tree/script/trace/PNG pins прошли. Все 15 PNG просмотрены. QA source внутри каждой trace побайтово равен точному MAIN Git blob; завершённые trace actions без ошибок.

REC: 22 checks PASS, 8 PNG. Проверены реальные SHA/size downloads всех трёх архивов и отдельный локальный server request counter. Evidence подтверждает физическое 1× playback, pause/rates/seek/steps/replay и held pixels при draw targets 30/60. Две held PNG побайтово одинаковы. Три negative trace bodies независимо совпали с точной last-byte XOR corruption и schema 999 с соответствующим integrity-valid catalog. Unknown experiment не запрашивал recording.

Dense: 14 checks PASS, 5 PNG. Native результаты всех 101 ticks 0..100 видны в trace. Контрольные 0/50/100 whole frames и известные genomes независимо сравнены с original Git JSON по exact strings и binary64. Genesis residual — null; следующие 100 frames содержат точные строки matter="0", energy="0". Largest saved fixture честно обозначен как PARTIAL; retained chunks=1. SHA/CRC/truncated/trailing-member corruptions отказали.

Семь ERR_ABORTED имеют уникальное соответствие CDP/native fetch/Playwright: 4 explicit cancel и 3 полного EOF/count до finally cleanup, с allowance≤1ms. Предшествующих fetch/body failures нет. Trace подтверждает superseded seek→AbortError/current992/notfailed и close→AbortError/currentnull/retained0. Uninstrumented production unhandled=0; отдельный exact sensitivity control=1. Atomic 255→256 сохраняет canvas/inspector/playhead до release оригинального chunk. На tick 992 при 1×: 1.573 model seconds за 1.573 wall seconds, без extra HTTP и смены sample. Local 100k/1M endpoint checks ограничены; 33,393,079 served bytes ниже 96 MiB cap.

PUBLIC: 4 checks PASS, 2 PNG, 19 Node HTTPS byte pins на точный source, включая metadata/final gzip всех трёх horizons. Browser проверяет только default 14 assets на viewport. Canonical 307 observe.html→observe проверен отдельно от browser HTML byte qualification. Все пять gzip assets имеют bounded canonical header/CRC/single member/decoded size; inflated digests совпали с manifest. Desktop 1440 / mobile 390 реально показали every-tick 0→1, 30 s. При draw targets 30/60 physical playhead продвигается, initial pixels и tick0 сохраняются. Через page close/global drain control requests отсутствуют; в каждом viewport body reads settled 14/14, pending guard actions=0.

Browser raw HTML: 7261 bytes, SHA 47d1c84479b19c2025ddb8d273bf85d824c8aaf28f46a9043e475e015dea396a. Удалена **только** точная insertion из 367 bytes @6878, SHA bbba70d1fbb140fe2cff2d40386e726bfe911227760ad6e69e29644e42b6f40a. Остаток побайтово равен Git HTML: 6894 bytes, SHA 6a0428e4ce738ada8cfd7e3f3586d1978dbe3f4c70fe2cd9d9dd8b8a002d640d. Оба retained HTML и captured trace HTML совпали.

Точный analytics Script GET блокирован CDP Request stage before-transmission ACK, по одному на viewport. Единственные expected errors — соответствующие same-URL ERR_BLOCKED_BY_CLIENT.Inspector и console error. Unexpected request/error/cleanup отсутствуют. Guard обеспечивает origin/method; наблюдавшиеся same-origin paths независимо равны declared assets. Strict per-path source allowlist и hosting deployment Git attestation не утверждаются.

Есть неблокирующее визуальное ограничение: при 216 cells / tick 992 schematic glyphs пересекают HUD heading/counter text, особенно 320/390 px. Controls остаются видимы и unobscured. Общую чистоту плотного observer этими PNG подтвердить нельзя.

В review не запускались browser/HTTP/model/Cargo/test/CI и не менялись production/LAB files. Trace не содержит каждый JS response body: недостающие bindings подкреплены точными protected QA served/response SHA observations, а не полным trace-only body reconstruction. Наблюдавшиеся requests — GET без body/cookies/credential headers/signed URLs; public analytics metadata в точном known HTML сохранены как public evidence.

Analytics execution, native background/kernel networking, achieved hardware FPS, universal performance/peak heap, full accessibility/native input и kernel isolation не утверждаются. Нет полного 2.42 GB public download, public 100k/1M render или свежего full 1M decode/model run. LAB 21 HOLD; interoperability audit не начат. Все immutable pins и подробный scope — в review.json.
