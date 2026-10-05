# Независимая source-проверка paused3D QA

**0 source-блокеров** для `83fa1e52e478e2a33602a24bf83b5456e55e70a3` / tree `47ce45646315b09493d58d87ca8fce4dc867aa8d`. Это разрешает следующий согласованный CI gate; actual browser/fullCI acceptance ещё не получена.

База — `5beaafb2c4f717862b62e67fd3e23e7d81860535` / tree `7f6b3ea5a67810708b6a39e5e78f418d6b2d1b2d`. Clean worktree, ровно пять согласованных paths. Оба QA файла byte-exact cherry `3d48f47d5929fc7d06c808c3845d8d440ba4d716`. CI добавляет только `scripts/physical-3d-png.test.mjs` к существующей Node command; ACCEPTANCE сохраняет exact prefix и добавляет 19 строк, unique checkpoint содержит 47 строк. Объекты `crates`, `site`, `configs`, SPEC и DECISIONS совпадают с базой. LAB assets/loader/workflow/nav/ADR112 не перенесены.

В [QA source](/workspace/liminis-paused-3d-qa/scripts/check_physical_3d_browser.mjs:67) bounded decoder сохраняет все RGB/alpha bytes; opaque RGB получает alpha255. Comparator требует exact dimensions и каждый RGBA byte прежней всей compositor области. Любой изменённый pixel, alpha или размер — FAIL с count/bounds. Масок, допусков, новых crop и capture retries нет.

Baseline/30/60 original PNG и raw/decoded SHA сохраняются до identity assertions. Pre/post API/camera/viewport/mode/revisions/drawnTick и layers/slice inputs сравниваются с frozen paused/baseline facts. Browser poll-count, single-flight и отсутствие лишних POST сохраняются. Дополнительные Node API audit GET отделены от browser request counter и не изменяют browser polling scheduling. Все 13 прежних checks и strict final lifecycle/error gate сохранены; official pinned Playwright1.61.1, sandbox/observed argv и stock software-renderer qualification не меняются.

[Четыре synthetic controls](/workspace/liminis-paused-3d-qa/scripts/physical-3d-png.test.mjs:8) выполняют actual decoder/comparator против independent known bytes: пять PNG filters/разные encoding/RGB-alpha, изменение RGB и alpha, dimensions, bounds и malformed/oversized input. Author сообщил один PASS run на Node24.19; reviewer прочитал controls и не повторял runtime. Node controls не означают browser PASS.

Historical main5bea/run37241483046 остаётся FAIL неизвестной причины. Нет утверждения, что отсутствующие прежние canvas captures имели одинаковые pixels. Старые 79/c02 PASS на этот head не переносятся. LAB PR21/publicLAB/models остаются HOLD.

Обязательны завершение старого Cargo run до нового, один fresh exact-head protected fullCI, независимая actual13GL/originals/trace/paused PNG приёмка и прежние recorded/dense/physical/2D regressions, durable archive/readback и guarded merge с существующим main/public follow-up. Reviewer не запускал браузер, Cargo, модели, Node tests или HTTP и не менял author/product files.

Точные five-file SHA/размеры, unchanged object identities и проверки привязки находятся в [JSON](source-review.json).
