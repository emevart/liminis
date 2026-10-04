# Открытие Репозитория И Публичный Observer

## Цель И Разрешение

2026-10-04. Пользователь просит открыть репозиторий, проверить лицензию и сайт,
показывать настоящую симуляцию либо проведённые эксперименты. Ранее прямо
разрешены PR/merge и существующий Cloudflare auto-build/deploy: «да ок, пусть
выкатывается». Это scope публичного исходного кода и отдельной записи, не
публикации управления локальным ПК или переноса живых сохранений.

Решение согласовано с Astra: ADR-104, статический replay реального Rust run.
Браузерного расчёта биологии, WASM, серверной симуляции и нового hosting нет.

## Состояние

- Репозиторий: `emevart/liminis`, теперь public; анонимный GitHub API вернул
  `visibility=public`, `private=false`. Homepage: `https://liminis.dev/`.
- Ветка: `codex/public-observer`, база main `5ffb313` (PR #8).
- Проверенный engine/exporter commit: `43f4a2df25c356ff387b71ac1961b516975f999b`.
  Первый отдельный commit содержит только новый экспортёр. Ядро, сценарии,
  Cargo manifest/lock и toolchain относительно базы не менялись.
- Следующий commit содержит сайт, запись, CI, README и этот checkpoint.
  Факт merge/выкладки проверяется в PR, не предсказывается этим документом.
- Исходный dirty checkout и процессы 8080-8083 не изменялись. Отдельный
  локальный статический preview: `http://127.0.0.1:8090/`.

Root владеет открытием/проверками, README, CI и checkpoint. Astra согласует
формат и ADR; exporter/viewer имеют отдельные владельцы и независимое ревью.

## Лицензия И Публичность

- Полная Apache-2.0 с владельцем `Pavel Artemev` уже есть в LICENSE;
  Cargo metadata и CITATION.cff согласованы. Лицензия не заменялась.
- Cargo metadata всех 71 пакетов содержит разрешительные варианты лицензий;
  LGPL у одного пакета является альтернативой MIT/Apache, не выбранной
  обязательной лицензией. Это проверка metadata, не юридическое заключение
  и не аудит будущих бинарных дистрибутивов.
- Локальные Lucide paths сопровождаются `site/NOTICE`: ISC и MIT для
  Feather-derived download, атрибуция сверена с `https://lucide.dev/license`.
- Gitleaks Git scan всех refs: 77 non-merge commits из 82 reachable,
  около 7.70 MB diff, findings 0. Отчёт: `target/qa/public-secrets.json`.
- Сканированы 24 завершённых CI-прогона и обсуждения 8 PR: 4.40 MB,
  findings 0; `target/qa/public-discussions-secrets.json`. GitHub artifacts
  отсутствовали. Это автоматическая проверка, не гарантия отсутствия любого
  чувствительного текста. Открытие включает обычную историю авторов,
  обсуждения и Actions-логи.
- GitHub secret scanning и push protection включены и проверены API.
- Gitleaks статического site payload: около 6.09 MB, findings 0;
  `target/qa/public-site-secrets.json`. Приватная `.liminis` не читается
  экспортёром и не входит в публикацию. Сайт не обращается к localhost,
  сторонней телеметрии или внешнему API.

## Реальный Опыт

`cell-chamber`, seed 42, world 30, chamber 1, config
`blake3:1802c0f129855749`. Отдельный headless run исполнил 10000 переходов
через существующий `micro::step`; оба integer residual проверены на каждом.
201 полный sampled cell frame: 0, 50, ..., 10000. У genesis residual unknown.

В конце 94 живые клетки, 1044 рождения дочерних клеток, 522 деления,
436 смертей, максимальное живое поколение 9. Развитие ограниченных вариантов
физиологии не доказывает открытую эволюцию или долговременное разнообразие.

Два `cargo run --release ...` дали одинаковые 6047570 байт:

- BLAKE3: `d033dd8f012997fd050feb9f3ef7d9f41946b15b4decdabf30a1091037b1468a`.
- SHA-256: `6d17cac2d95fe9d3d007866d7cf72ddf14e945bbe4c060591f961f305656a1f1`.

Команда и структура: [site/data/README.md](../../site/data/README.md).
Source guard проверяет HEAD, чистоту engine/exporter/build inputs и untracked
sources. Это проверка исходников при сборке через Cargo, не криптографическая
аттестация произвольного бинарника. Другой HEAD закономерно меняет metadata.

## Проверки

- `cargo fmt --all --check`, Clippy workspace/all-targets `-D warnings`: PASS.
- `cargo test -p liminis --example export_cell_replay --release`: 3 PASS.
- `node --test site/recording.test.mjs`: 8 PASS; целый dataset, обрезанный
  финал, cadence/genesis, inventory, metadata, чрезмерные bounds и испорченные
  exact integers. JS syntax checks: PASS. Оба suite добавлены в CI.
- Независимое ревью закрыло source-binding и completeness blockers: 0 остаётся.
- Browser QA 320/420/590/900/1440: реальная камера/графики, pause/play/4x,
  seek/end/replay, клеточные свойства и корректный not-alive при перемотке;
  нет горизонтального overflow. Два пересечения подписей исправлены CSS.
- Два paused screenshot buffer дали одинаковый hash. Воспроизведение
  естественно дошло до конца и остановилось. Browser download SHA-256 совпал
  с artifact. Console warn/error при рабочем пути отсутствовали.
- Browser fault-HTTP fixture отдельно не запускался: локальная команда
  создания тестового сервера отклонена инструментом. Отказ malformed data
  проверен unit suite; сетевой error path остаётся browser QA gap.
- Старый main `5ffb313` полный post-merge CI завершился success. Новый PR CI
  ещё должен быть проверен перед merge. Старые engine release/10k доказательства
  записаны в [предыдущем checkpoint](2026-10-04-publication.md).
- SPEC/NORTH_STAR/archive unchanged; прежний ADR prefix сохранён;
  `git diff --check`: PASS.

## Ограничения И Следующий Шаг

Это конечная запись наблюдений, не live simulation и не restart checkpoint.
Камера well-mixed; позиции display-only, нет Brownian motion/плавания/контактов.
Межкадровые события не реконструируются. Загрузка около 6 MB требует сети;
состояния loading/error явные. Повтор не запускает новый эксперимент.

Далее проверить PR/merge, фактический Cloudflare build и публичный сайт с
совпадающим dataset digest. Следующий биологический этап остаётся настоящими
пространственными координатами/движением, затем общий срез/3D observer.
Browser engine требует отдельной portability/reproducibility приёмки.
