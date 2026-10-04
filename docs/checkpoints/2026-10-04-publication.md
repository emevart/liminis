# Публикация локального наблюдателя жизни

## Цель и разрешение

Дата: 2026-10-04. Довести проверенную локальную работу до PR и main,
сохранив действующие опыты. Пользователь прямо разрешил: «доделываем работу
локально, публикуем в main ветку, мерджим и все такое». Это разрешение на commit,
push, PR и merge после зелёного CI/review; не на deploy, удаление опытов или
перезапуск чужих процессов. Архитектурные и биологические решения согласует Astra.

## Git и Владение

- Репозиторий: `emevart/liminis`, private.
- Checkout: `C:\Users\1\.codex\worktrees\local-living-world\liminis`.
- Ветка: `codex/local-living-world`; исходная база `d21fb41`.
- Main `580fddb` согласован merge-коммитом `3d8fe94` с сохранением AGENTS.
- Завершённые этапы: `491bfd4`, `a198ba2`, `191cb8e`, `b5ed83d`.
- Проверенный correction-код: `5a1ec9e`. После полного прогона окончательно
  уточнён только текст новой ADR-102; равенство исходников/config/Cargo manifest
  проверено Git diff. Последующий handoff-коммит меняет только документацию.
- Координатор владеет micro validation, inoculum validation и этим checkpoint;
  Astra согласует численную коррекцию и ADR, отдельный reviewer проверяет
  реакционное ядро, storage reviewer исправляет хранилище/совместимость.

Dirty-файлы исходного `H:\liminis` относятся к прежней ADR-090 работе над
стабильными ID реакций. Она уже включена и развита в этой ветке; отдельно
переносить её заново не нужно. Исходный checkout не очищался и не перезаписывался.
Ignored `.liminis`, бинарники, локальные логи и QA изображения в Git не публикуются.

## Что Готово

Локальные наблюдатели пространственной популяционной экологии и отдельная
однородная клеточная камера. В камере есть реальные ID, наследуемая физиология,
рост, деление, родители, поколения, голодание и лизис. Есть управление временем,
инспектор, графики, точный учёт, полные checkpoint и история нескольких сессий.
Экранное размещение клеток не физическое: Brownian motion, плавание, контакты,
адгезия, GRN, тела и 3D пока отсутствуют.

Предпубликационное независимое ревью выявило округление конкуренции реакций
сверх запаса вещества. ADR-102/103 задают коррекцию и границу world 30:
новая eco семантика не продолжает world 28/29 под прежней identity. Старые
локальные eco процессы и `.liminis/legacy/eco-world28.exe` сохраняются.
Клеточная биология не меняется; narrow resume cells world 29 сохраняет identity.

Усилены проверки стоимости кинетики, положительных физических цен энергии,
крайних фенотипов, starvation clock, неизвестных nested полей и почти касающихся
одновещественных инокулюмов. Они не меняют поставляемую клеточную динамику.

## Проверки

Исторические доказательства этапов и browser QA:

- `docs/plans/2026-10-04-local-living-world.md`;
- `docs/plans/2026-10-04-genetic-colony.md`;
- `docs/plans/2026-10-04-durable-living-experiment.md`;
- `docs/plans/2026-10-04-cell-chamber.md`.

Итоговые локальные проверки correction-кода `5a1ec9e`:

- `cargo test --workspace --release`: 722 passed, 9 явно ignored;
  `target/qa/publication-release-tests.log`. Среди них новые precision/oracle,
  accumulator/negative-pool, marker/history и cells29 resume-step-resave регрессии.
- `cargo test -p liminis-core micro:: -- --nocapture`: 20 passed, 1 ignored.
- Явный release cell-chamber 10k: PASS, final 94, warm minimum 76, maximum 248,
  births 1044, deaths 436, observed kinetics -1/0/+1, final kinetics 0;
  `target/qa/publication-cell-10k.log`.
- `cargo fmt --all --check`, Clippy всех targets с `-D warnings`, bare-Q lint
  (13 файлов), `git diff --check`: PASS. `publication-clippy.log` хранится локально.
- `scripts/test-hooks.sh` через Git Bash с native Windows TMPDIR: 25 PASS,
  `target/qa/publication-hooks.log`.
- Независимое повторное численное ревью полных kernel/react и kernel/fold с
  вызывающими guards: blockers 0, два прежних остаточных риска записаны ниже.
- `cargo build --release -p liminis`: PASS; новый isolated cells30 HTTP process
  вернул viewer 200, исполнил 10 шагов с точными ledger, сохранил tick 11 и после
  настоящего process restart восстановил paused tick/identity30. Residual был
  unknown до первого нового проверенного шага, затем нулевой. Тестовый процесс
  остановлен; `target/qa/publication-smoke.log`.
- Замороженные документы unchanged относительно main; старый ADR-prefix
  сохранён. Исторические eco 10k не выдаются за новый world30 soak.

Публикационный diff: [main...codex/local-living-world](https://github.com/emevart/liminis/compare/main...codex/local-living-world).
Проверки/факт merge отражаются в GitHub PR этой ветки, а не предсказываются этим
pre-merge checkpoint. Перед продолжением сверить актуальные main и PR state.
Локальная Windows проверка не является проверкой Linux/cloud; merge разрешён
только после зелёного CI. SPEC acceptance 10^6 тиков остаётся отдельной работой.

## Локальный Запуск

```powershell
./scripts/start-local.ps1 -Cells -OpenBrowser
./scripts/start-local.ps1 -Cells -Resume latest -OpenBrowser
```

Скрипт выбирает свободный порт; занятые окна не закрывает. Новая камера по
умолчанию использует `.liminis/cells` и порт 8083. Resume использует сохранённые
config/seed; не передавать их заново. Рабочие 8080-8083 не перезапускались в
публикационном follow-up. На последней read-only проверке каждый отвечал,
ошибок не было, оба последних проверенных residual были нулевыми.

## Передача и Следующий Этап

Для cloud нужен опубликованный Git checkout, `rust-toolchain.toml` (1.97.1),
Cargo.lock, Python 3 и доступ установки зависимостей. Команды:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_bare_q.py
cargo test --workspace
cargo test --release -p liminis-core micro::config::tests::shipped_cell_chamber_sustains_real_turnover_for_10k_steps -- --ignored --nocapture
```

Cloud workspace предназначен для кодирования и headless проверок, не для
бессрочного размещения аквариума. Локальные живые данные сами через Git туда
не попадут; их перенос требует отдельного проверенного полного checkpoint.
Новая cloud-задача в этом follow-up не создаётся, deploy не выполняется.

Следующий конкретный этап: согласовать с Astra минимальную пространственную
камеру с физическими координатами/движением, затем единый переключатель
срез/3D и прозрачность веществ. Не выдавать декоративную анимацию за физику.
Далее S1-prime локальная химия, S1 экологическая калибровка/C-N циклы,
S2 GRN и адгезия с воспроизводимым развитием без мутаций, S3 разнообразие и
отбор коллективов. S4 изменения среды и S5 GPU только по измеренной потребности.
Цель NORTH_STAR: переход от самостоятельных клеток к новой единице отбора.

Длительная диверсификация, 10^6 тиков среды, много seed и морфогенез не приняты.
Известные отложенные риски: u32 горизонт RNG камеры, wall-clock порядок retention
и legacy eco history без run/session binding. Прежний `numeric::xi` может
насыщаться в release при недостаточном доказательстве kinetic envelope;
публичный Fold constructor не имеет полной самостоятельной проверки Q ingress
(нынешний light dispatch заблокирован, dt проверяется reaction ingress).
Источник этих двух замечаний: независимое повторное numerics-review; сейчас
не нарушают доказанную integer closure, дальнейший аудит kinetic/Fold envelopes
проводится отдельным узким этапом. SPEC/NORTH_STAR/archive не меняются.
