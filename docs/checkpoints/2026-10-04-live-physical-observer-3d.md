# LIVE-3b: Three.js observer реальных центров

- Дата/цель: 2026-10-04. После принятых LIVE2/LIVE-3a/REC-2 добавить
  bounded orthographic3D observer chamber2 по пользовательской очереди и
  решению GPT-6 Astra. NOT ACCEPTED до новых exact-head gates.
- Base main: `239ec8bde3d2f15e2aa5b50e9294e65eab5c0926`,
  tree `27c506c9b4aed0fbdaaa576f36bb1b768af8ae3a` после PR19.
  Root checkout `/workspace/liminis-live-3d-integration`, published branch
  `codex/live-cells-3d-observer`; source `codex/live-cells-3d-author`,
  browser QA `codex/live-cells-3d-browser-qa` от того же main.
- Владение: scoped author — viewer HTML/new3D module, exact pinned vendor,
  literal host assets/tests и focused Node tests; другой QA author — только
  new `scripts/check_physical_3d_browser.mjs`. Root alone shared ADR111,
  ACCEPTANCE/CI/checkpoints; reviewers не authors. Отдельные worktrees,
  author/QA sparse исключают только immutable dense chunks, integration full.
- Scope: real xyz centers/common scale, 2D default/3D toggle, ortho camera,
  opacity/centers-box-slice layers, прежний exact-ID inspector/atomic snapshot.
  Один context, current-population resources, explicit current2D fallback;
  pinned local Three.js0.180.0 MIT import closure. Ни model/core/config/version,
  ни LAB/raw/science/legacyJSON/dense data/frozen SPEC/NORTH_STAR не меняются.
- Сделано: actual Astra архитектура принята, ветки/checkouts и владельцы
  заведены, source/QA работают; coordinator append-only ADR111 и operational
  acceptance записаны. Это начало реализации, не source/browser PASS.
- Проверено: base clean exact239/tree27c; REC-2 exact11CI/independent review
  и qualified actual public accepted, durable ZIP/data network readback PASS.
  Upstream r180 stable tag target `0af9729d0c143a86a1d725d6e2c3ad83301f3f34`;
  final vendored byte/import-closure verification pending.
- Не проверено: final3D source/Node/host tests, independent geometry/source
  review, actual protected WebGL2/pixels/controls/mobile/fallback/lifecycle,
  exact-head fullCI, artifacts, merge. Node checks не browser PASS.
- Риски/зависимости: первый actual WebGL2 check — реальная доступность
  existing protected Chromium. Нельзя ослаблять sandbox/argv/network guards;
  observed backend не доказывает hardwareFPS. New gate bounded, old chamber1/
  physical/2D remain regressions; Cargo builds сериализуются.
- Действующие разрешения: ownership/write-go
  https://github.com/emevart/liminis/issues/11#issuecomment-5981221010 ;
  codex commit/push/PR, independent review/numerics, guarded green merge и
  existing deployment разрешены. Новые infra/integrations/secrets/расходы
  не разрешены. Старые product authors STOPPED; old VM не используется.
- Следующий шаг: получить clean scoped source/QA commits, serial focused
  Cargo/Node checks, integrate на fresh main и принять independent source
  review перед первым mandatory protected browser/fullCI run.

## Bounded public delivery follow-up, 21:19 UTC

Root-owned existing `scripts/check_public_recording_browser.mjs` дополнен
Node HTTPS metadata/final-chunk byte pins всех3dense horizons. Default10k
browser scope сохранён; никаких новых model runs/full publicGBdownload.
Native syntax/local metadata+finalgzipSHA проверка PASS:7uniquefiles,
2357392bytes, final gzip10k288747/100k275423/1M414483bytes. Это локальные
сохранённые bytes, не network/browser PASS. Existing main-only protected
gate выполнит новые чтения после green3Dmerge; independent source review
pending вместе с3D integration. Полный REC-2 accepted scope не расширяется
задним числом; предыдущие qualifiers и evidence остаются неизменны.
