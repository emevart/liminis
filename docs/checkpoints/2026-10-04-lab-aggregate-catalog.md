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

## Source integration и уточнение QA, 23:23 UTC

Clean published data43356e5, UI fa4ce68 и browserQA3baa2d3 объединены обычными
cherry-pick; собственные scoped authors завершены.15 Python,12 UI Node,
160 raw paired-record checks/6 synthetic integer controls прошли у авторов.
Native browser/model/Cargo не запускались. Root добавляет только Lab navigation
на existing homepage, LAB gate в unchanged protected recorded job и
builder freshness/focused checks в existing data job. Official Playwright1.61.1,
Ubuntu22/sandbox:true/observed argv и остальные mandatory CI gates сохранены.

Main5bea pushCI37241483046 завершился FAILURE только на paused3D check8
(first7GL PASS; Rust/recorded/dense/public jobs PASS). Original FAIL сохранён.
Independent diagnostics подтверждают22 неизменных API tick1, но original
per-canvas PNG/post-FPS audits отсутствуют; причина raster/encoding/timing
неизвестна. Public artifact независимо qualified PASS для19HTTPS pins всех
3 horizons и actual default10k playback; это не overallmainGREEN/full2.42GB
HTTP readback/public100k1M rendering.

Astra явно одобрил отдельный QA-only repair commit в этом integration PR
и один fresh complete CI для LAB+fixed3D. Strict dimensions/decodedRGBA всей
прежней области, all3 original PNG и pre/post API/camera/viewport/layers,
diffcount/bounds сохраняются до assert. Никаких masks/tolerance/retries,
production3D bytes main5bea-exact.4 meaningful syntheticPNG controls PASS;
source/numerics/UX independent review идёт, actual fixed3D/LAB браузер ещё
NOT_RUN. Существующий FAIL не превращается в PASS после смены oracle.

## First actual LAB FAIL и узкие исправления, 23:50 UTC

Exact79df352/tree0c9b9a/run37243706546 завершился FAILURE: LAB2checksPASS,
check3 histogram CSSOM96.7391% вместо raw89/92×100=96.7391304347 отвергнут
прежним1e-5. Original artifact11318651498/2210725bytes/SHA256
`bbd3f4dce56469c082be6e61f8d9abdd913d48987af6575620a6493e4bcb3a09`
сохранён без перепаковки, outerCRC PASS, report/PNG/trace проверены независимо.
Новых биологических данных это расхождение не устанавливает.

Astra и независимые reviewers одобрили exact detachedCSSOM reference из raw
counts/living, без broadtolerance и изменения product histogram arithmetic.
Actual PNG подтвердил мелкие оси: clean published UI follow-up a0e5461
cherry-picked02aeab1; dynamicSVG1unit1CSSpx,height210,axisfont11/3sig,
один width-change ResizeObserver.14 meaningful UI Node PASS у автора,
rootQA теперь независимо проверяет rawframes/axes,4actualtypography layouts,
clip/nonoverlap и exactcounts; source/numerics review0. Старый FAIL сохранён;
новый source требует полного fresh CI и original artifacts.

Fixed3D79 original11318935319/59183146bytes/SHA256
`8d773a29f6bef2443124a73c31917189dd4a921ae6972e6c08ba83de2d49aa60`
доставлен через unchanged bounded helper pattern: fb3a5b47/run37244552112,
4exact rawparts. Original SHA/size/CRC PASS;13checksPASS,all3PNG1130×752,
same raw+RGBA SHA,changedPixels0/statepoll3each. Independent actual review
и durablearchive ещё в работе. Overall79FAIL и прежний main5FAIL не relabel;
этот 3D verdict не переносится на следующий LAB candidate.

Подготовлен отдельный publicLAB evidence-only smoke по Astra scope:
8asset pins/productcandidate отдельно от QAhead; deployedProductHead:null
запрещает runtime. Preparation branch не запускает QA. После acceptedmerge/
existingdeployment root закрепит actualproductSHA и запустит protected smoke.
Новая infrastructure/secrets/spend/security bypass не создаётся.
