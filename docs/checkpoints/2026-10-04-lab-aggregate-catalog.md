# LAB-3a: каталог сохранённых агрегатных наблюдений

- Дата и цель: 2026-10-04, 22:58 UTC. Показать на существующем сайте 24
  сохранённых LAB прогона, графики, параметры среды и living kinetics alleles.
  Никакого пересчёта биологии или реконструкции индивидуальных клеток.
- Основа: принятый main `5beaafb2c4f717862b62e67fd3e23e7d81860535`, tree
  `7f6b3ea5a67810708b6a39e5e78f418d6b2d1b2d`, после guarded merge PR20.
  LIVE-3b принят на exact 9d: 11 checks, 13 actual3D и 79 regression checks,
  независимые code/numerics verdicts 0 blockers. Original ZIP и собственные
  отчёты сохранены в трёх группах с закрытым Git network byte/CRC readback.
  Current main run `37241483046` и public all3horizon follow-up пока в работе;
  его PASS не переносится на новый LAB source.
- Integration: `codex/lab-aggregate-site`; отдельные data/view/browser-QA
  branches/worktrees основаны на том же main. Root владеет ADR112, shared
  ACCEPTANCE, navigation, CI и checkpoint. Data author владеет stdlib builder,
  focused tests и site/data/lab-2. UI author — lab.html/js/css/data adapter и
  узкие tests; QA author — отдельный actual browser script. Review независимо
  от авторов; Cargo остаётся сериализованным и для этого этапа не требуется.
- Разрешения: прежний user write-go/единственный coordinator сохраняется.
  Текущее user поручение делегирует вопросы и решения adviser Astra. Astra
  явно одобрил ADR112 и scoped coding; повторного согласования этого scope
  не требуется. Новые infrastructure/secrets/spend/security bypass не разрешены.
  Official pinned protected Actions используются для настоящего browser QA;
  native Node/source checks не называются browser PASS.
- Immutable source: `21cbe90732128edee61963544e414d29a7420577`, world30,
  chamber1, dt30s; current main/site commit не заменяет numerical provenance.
  results.json: 5294742 bytes, SHA256
  `6fd944f6fe2e25ca5418ac357dcb97ce68620afee569b2ecd6dc684c22f7d37a`;
  comparisons.json: 1116928 bytes, SHA256
  `e2faeda80c9f47ff23e5aff14879f3780627d867fae51aa655b77d4ad52a8ae3`;
  manifest.json: 82880 bytes, SHA256
  `f7a669548650754864a410a46e86593ab722116e0a01057e17f6efc84efcb2f3`.
  Всего 6494550 original bytes копируются без reserialization. Raw LAB files,
  source identity, frozen docs, old recordings/URLs/SHA остаются сохранёнными.
- Readiness подтверждена: 6 conditions × seeds1/7/42/2026, 24 run IDs,
  4044 samples; TOML одинаков внутри каждого условия, различается между ними.
  Существующий recorded catalog несовместим с агрегатными rows без cell
  inventory; его contract не ослабляется. Отдельный descriptor/adapter — ADR112.
- Обязательные ограничения: 20 horizon-censored runs на 20000; 4 starvation
  extinct на454 с samples0/100/200/300/400/454. Latest-common400, terminal454
  и missing landmark500 различаются; unknown 0/4/null не равен нулю. Detail
  selector выбирает retained sample; published paired records не пересчитываются.
  Historical allele richness не равна living/species/full-genome diversity;
  четыре seed пары не являются 24 независимыми репликами. Genesis ledger
  неизвестен. Линии графиков — визуальные направляющие между наблюдениями.
- Сделано: scoped authors started; append-only ADR112 и acceptance добавлены
  в integration. Actual source/data/UI/browser checks ещё не выполнялись;
  LAB PASS не объявлен. Sparse worktrees исключают только dense gzip chunks;
  full dense inventory checks в них не запускать.
- Следующий шаг: получить clean data/descriptor и UI/QA commits, принять
  независимый source/numerics review, провести один exact protected fullCI
  с actual LAB browser gate, сохранить original evidence/readback; после
  green acceptance guarded merge и existing public LAB delivery acceptance.

Автопробуждение после завершения active cloud turn не подтверждено.
