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

## Source freeze и native host, 21:35 UTC

Source author clean/published `e9feb9aa7bc045f572d5155e2b8813eae5a78aa9`,
tree `c836ffbc7057c6507493e82bc23955a72d115ca6`:11ownedpaths,28/28Node
(16viewer+12new3D), syntax/fmt/diff PASS. Root normal cherry-pick
`d773598c3eb892947a201fb2a7e16bf0565f2832`,
tree `2873b83a65189d14c4de371bc5506800084be9e0`; provisioned pinned
`cargo test --locked -p liminis cells_host`:32PASS/0FAIL,63unit+16integration
filtered, compile12.85s/tests3.88s. New literal display routes serve exact
local bytes without changing state. Cargo sole root, now idle; никаких
новых production experiments, native browser или пересчёта dataset.

Astra independently проверил original accepted e245 CDP report: stock
Playwright1.61.1/Chromium149.0.7827.55 уже имел
`--enable-unsafe-swiftshader`, `--use-gl=angle`,
`--use-angle=swiftshader-webgl` при chromiumSandbox:true/oldguardPASS.
Новая draft blanket unsafe prohibition ошибочно отвергала тот же accepted
runtime; разрешено сохранить точный stock launch,8sandbox prohibitions,
`--disable-.*sandbox` и запреты ignore-gpu-blocklist/disable-web-security.
GPU flags/defaults не добавляются/меняются. Evidence явно запишет observed
flags и actualbackend; software geometry не hardwareFPS/kernelattestation.
Это решение о неизменной конфигурации, не новый WebGL/browser PASS.

Pending: clean QA freeze, independent exactcombinedsource/numerics review,
один fresh protected fullCI и actual3D artifacts; no merge до green gates.

## QA freeze и combined candidate, 21:39 UTC

QA author clean/published `336071358b1c4f64bdba2f43bc694ea5d00bfcfa`,
tree `44c5dec4e1c982e9b2c5b758695c6d1f50ff26c3`, только new script;
SHA256 `c3a6642f73c2a00fdfaea6c4b321e93a58e6c031dab2d220c3f044fe0c86de01`.
Syntax/diff и6arithmetic/5PNGfilter synthetic oracle checks PASS, не browser.
13mandatory checks/9PNG/trace,300s+boundedcleanup; old regressions сохранены.
Root normal cherry-pick combined `cfa5eac5bdeda75ef27a7610f4c953f82f2c3033`,
tree `7d8290afc003b90f1c0ccd865acc792f409a208e`, clean/published.

Independent numerical source review exacte9:0blockers.1440cases over144
camera/frustum/zoom/nonsquare fixtures согласуются с независимой проекцией
до3.49246e-10CSSpx при tolerance1e-6; u64ties/zero-slice/snapshot/revision
проверены. Source review JSON SHA256
`16921038c0c48a207dc5ebc1963be51ad78bc3c91533d165499d32bff220874d`.
Combined final source/QA review и fresh protected acceptance ещё pending.
Никакого nativeGL/productionmodel/datasetdecode repeat.

Следующий этап после принятого3D — Astra read-only scope LAB-3a:
отдельная плотная LAB страница existing site, матрица6conditions×4seeds,
парные агрегатные графики/sample selector/kinetics alleles/provenance.
Published24runs/4044samples не содержат индивидуальных клеток/xyz/full
genomes; их не реконструировать.20horizon-censored/4extinction454, missing
landmarks не нули/carry-forward. Новый этап пока не открыт для coding;
raw/source21cbe907 и все прежние scientific bytes сохраняются.
