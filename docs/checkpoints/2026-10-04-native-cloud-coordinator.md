# Native Cloud: принятие координации Liminis

Дата: 2026-10-04. Цель: продолжение проекта после сохранённой передачи Work Cloud.

## Ownership и разрешения

OWNERSHIP ACCEPTED: native Cloud coordinator — единственный coordinator/integrator/release owner emevart/liminis.
Прямое поручение пользователя снимает read-only hold; публичный источник:
https://github.com/emevart/liminis/issues/11#issuecomment-5981221010 .
Разрешены scoped ветки codex/, commit/push/PR, independent review/numerics, необходимые тесты,
guarded merge после exact-head green CI/приёмки и существующий site deployment.
Новые infrastructure/integrations/secrets/расходы/crate publish и ослабление защиты не разрешены.
Frozen SPEC/NORTH_STAR не править; ADR append-only с «Отвергнуто». Shared ADR/CI/navigation сериализует coordinator.
Старые потоки не возобновляются. Исключение: старый LAB сохраняет пять исторических LAB-1 QA
файлов evidence-only в собственной архивной ветке; эта архивация не дублируется coordinator.

STOP ACK:
- LIVE: https://github.com/emevart/liminis/issues/11#issuecomment-5980993811
- RECORDED: https://github.com/emevart/liminis/issues/11#issuecomment-5980956890
- LAB: https://github.com/emevart/liminis/issues/11#issuecomment-5981053826

## Полученные refs

| Поток | HEAD | Tree | Состояние передачи |
|---|---|---|---|
| main | c5be8fc8652433c67378a689fbb151ea4bad510a | caff90194bb66a0aa34c696feb149113c70afba2 | LIVE-1 #12 MERGED |
| LIVE-2 | d5362346a3a02f0b0503fece5b7f30efc8c20efc | 51ab508bd61c110f75649f94fef08baad7fb6492 | WIP, НЕ ПРИНЯТ; fixture fix NOT RETESTED |
| RECORDED-1 #13 | c79a5a4678e1950429d749fb4c950ab304377776 | 6bd6989b75285eae5de4f22900a0480524301339 | exact run37208566873 completed SUCCESS; artifact acceptance coordinator pending |
| LAB-1 #14 | 21cbe90732128edee61963544e414d29a7420577 | 713751fb97b2f53206fc0a86c368e63c28d1da65 | ACCEPTED old head; fresh-main integration pending |
| LAB-2 #15 | aa5ecb24c99546e3f0f6c05983961476139b8726 | 339073efc563156c0c5fccc2dfa69e7bfe560a21 | exact run37209123599 completed SUCCESS; retarget/integration after #14 |

Локальный startup checkout /workspace/liminis, branch work, f7c74a был чистым;
origin/main fetched до c5be8fc. Далее отдельные coordinator/integration worktrees.
Точные новые heads/CI и результаты этапов публикуются в issue #11; старые PASS не переносятся на изменённый head.

## Evidence и научный provenance

Постоянный evidence-only prerelease, не product release/deploy:
https://github.com/emevart/liminis/releases/tag/qa-handoff-2026-10-04 .
Четыре исходных ZIP LIVE8874 PASS / RECf9 FAIL / RECf72 PASS / RECc79 PASS
и bundle complete published history: 48,225,607 bytes <64MiB; ZIP <20MiB.
Root перед передачей повторно скачал все пять бинарных assets и проверил SHA;
git bundle verify PASS. Здесь metadata Release повторно прочитаны, prerelease=true,
asset digests/размеры совпадают с переданным inventory.
Manifest SHA256 0298fc25fc0bfa66c6449fe18c3e79baefc6ee100e28a06f36262636a948f32a.
Bundle SHA256 1f4d12b12dfb5c05dd1c21361e63dcb2484c75700d4b41a069a9cd8a43e888aa.

LAB-2 raw results.json 5,294,742 bytes:
SHA256 6fd944f6fe2e25ca5418ac357dcb97ce68620afee569b2ecd6dc684c22f7d37a.
Numerical source immutable 21cbe90732128edee61963544e414d29a7420577 / world30/chamber1.
24 runs и full24 repeat уже сохранены; перенос базы не меняет science bytes/source identity.
Последующий итоговый ref исторического LAB-1 evidence-only архива добавить после публикации старым LAB.

## Ограничения и очередь

Shell GitHub API возвращает Forbidden; GitHub connector branch/blob/tree/commit/PR/merge доступен.
Публичные Git fetch/raw paths доступны. Не обходить network/security ограничения.
Настоящая UI приёмка — existing official pinned Playwright Actions с chromiumSandbox:true,
actual argv guard, trace/PNG/report. Node checks не являются browser PASS.
Один Cargo-процесс за раз. Новые scoped авторы/reviewers независимы, старые stopped authors не возобновляются.
Автопробуждение после окончания активного turn инструментально не подтверждено.

Следующий шаг: независимая code/artifact приёмка exact #13 c79 + all-checks/main guard,
ready/merge existing PR, проверить existing site. Затем fresh-main #14 и retarget/rebase #15
без rerun/переписывания raw24; далее завершить LIVE-2 physics/legacy eco30/numerics/browser/ADR gates.

## Вехи после принятия ownership

### REC-1 и архив LAB-1

PR #13 ACCEPTED/MERGED: head c79a5a4678e1950429d749fb4c950ab304377776,
CI run37208566873 и 11 checks PASS; независимые code/evidence/8 PNG review — 0 blockers.
Merge main 3b66c2cb680870568e54573fc85df477ef978e28, tree6bd6989b75285eae5de4f22900a0480524301339,
parents c5be8fc + c79a5a4. Native readback проверил original REC ZIP размер/SHA и manifest SHA.
Публичный liminis.dev ещё NOT VERIFIED: native shell policy CONNECT403; проверка через existing Actions pending.

Исторический LAB-1 архив принят отдельно от scientific source:
ref codex/cloud-handoff-lab-evidence; commit522b8cee4629316d9f1d68a80dae114bcde040c4;
tree69970ab4dd8de704764c4e0b57d6c519f7b7a7ea; source parent21cbe907 unchanged.
docs/experiments/handoff/lab-1/lab-1-qa-evidence.zip:169880 bytes,
SHA256 e37d3364efcdc1c63fe6c026902c6554a80565652aa34c4e118824480d43db75.
Manifest прочитан из опубликованного ref. Четыре JSON оригинала побайтно сохранены;
публичный negative-control log содержит одну redacted compile-path строку.
Исходный 1244-byte log не опубликован и не удалён; byte-exact сохранность всех пяти originals НЕ заявляется.
ACK https://github.com/emevart/liminis/issues/11#issuecomment-5981282024; old LAB снова STOPPED.

LIVE-2 baseline d536234: native focused micro43PASS/0FAIL/1ignored, host31PASS/0FAIL.
Полная приёмка pending. Автор добавил только независимый SI oracle и second-save/resume eco30 fixture;
они ещё требуют root serial tests и independent final numerics review.

Текущий следующий шаг: интеграция fresh main в существующую LAB-1 ветку без force overwrite,
новый exact-head CI/review; затем LAB-2 retarget и 23 Python tests без пересчёта 24 runs.


### LAB-1 принят; LAB-2 и public verification в работе

LAB #14 guarded MERGED после independent integration review0blockers и
всех11checks SUCCESS run37211688454 на head78e897bc77d5a9715bf6627af931e8cdf1b35701.
Main5652dc344c0d2e5aed3f7b17b0a919434ca66822/tree02e313be851f6369d5608340d025f01861f5242c.
Все4LAB1blobs и source21cbe907 сохранены. Новые recorded artifact bytes
12,556,828/SHA256370c77443a24d793de17232eb12e39fa43f62b9975ecf0f3e1979b7cfc0ae79f
скачаны штатным GitHubartifact→file_id→download_file path в native workspace,
CRC/report source/tree/22browser+17HTTPchecks PASS. SignedURLs не являются durable refs.
MainREC CI и existingCloudflarecheck SUCCESS; productionplayback по-прежнему
отдельный gate. QA-only PR #16 head4839e5c43b9db2e93e0d9380a79f7e0220581231
проверяет fixedpublicliminis.dev actualbytes+Chromium throughexistingprotectedActions.
Independent code/CI review0blockers; actualrun/artifact review pending.

LAB #15 retarget main, head4f8208afb2b164d0ef1d9119ae7c5bd2f7629f94,
tree1b41aabb1ce3faccd948648b88fdeff6c16b1463; parentsaa5ecb24 +5652dc3.
All9LAB2blobs unchanged. На этом head23PythonfunctionalPASS; actualstaticCLI
24orderedrowsPASS; summary/comparisons byte-exact; rawSHA6fd944 неизменён.
FreshCI37213082140/independent integration acceptance pending; no scientificrerun.

LIVE2 продолжение опубликовано37ea4d39647f225eafed1912799fd904e39ed3f1.
Preservedbaseline d536 и releasebenchmarksource3489 остаются ancestors.
Micro44PASS/0FAIL/1ignored; cells_host/storage focused32PASS; fmt/clippyPASS.
AbsoluteSIoracle и eco30secondsave/resume PASS; fullhostpackage ещё выполняется.
ADR108Astraapproved append-only; independent numerics/code0blockers, browser
Save/resume claim исправлен локально до finalhead. D0/Dpositive real releasebenchmark
source/script/raw в Git, binarySHAbe14eca5dbcb44fc1c773f38cc145b59b0ceaaac5b768e0eee7c10129aac35ee;
rawSHA9bf8d946ade33724430909b2ac18e79d4edad125753ed8b7fa5f49c513b40ec1.
Finalfreshmain/default+physicalbrowser/fullCI/merge pending.

Normal authenticated Git push проверен доступным без изменения сетевой политики;
GitHub shell RESTAPI всё ещё Forbidden, connector остаётся штатным API path.
Cargo в native workspace сериализуется; expensive suite уже запущена один раз.
Следующий шаг: exacthead #15/#16 gates и guarded integration; затем freshmain
LIVE2 candidate, protectedphysicalQA и durable evidence до releaseacceptance.

### LAB-2 принят; LIVE2 native gates завершены

LAB #15 guarded MERGED: exact head4f8208afb2b164d0ef1d9119ae7c5bd2f7629f94,
все11checks SUCCESS run37213082140; independent fresh-integration/code/numerics
review0blockers, 1160 direct paired arithmetic checks. Main теперь
 de2b52cd08dc3772800725a5eca64619f6bb33c6/tree1b41aabb1ce3faccd948648b88fdeff6c16b1463.
Все9LAB2blobs и source21cbe907/world30/chamber1 сохранены; rawSHA6fd944 unchanged.
23Pythonfunctional tests PASS, static24row CLI и derived byte-exact checks PASS.
Это интеграция ранее выполненных24runs, не новый эксперимент или science rerun.

LIVE2 native local candidate a72f3f05a1861b4be026f1a24068af70e8a09df1 включает
принятый LAB2 main, solver/runtime unchanged относительно independently reviewed4a2daca.
Native micro44PASS/0FAIL/1ignored; cells focused32PASS; fullhost94PASS/0FAIL
за786.29s; Node49PASS; fmt/clippyPASS. Fullhost log8436bytes,
SHA256c2c4c4e07d08b4aa210940c70945fd945f9ae587e089df2af77b9db183ecbe98.
Final CI, protected default/physical browsers, actual artifact review и merge pending.
API positions не выдаются за spatial visualization: current glyph layout schematic.

Public QA-only PR #16: реальные production Node GET всех9assets PASS,
но первые browser gates FAIL сохранены как диагностические результаты.
Официальный CDN делает same-origin307 /observe.html→/observe и добавляет
известный367-byte CloudflareAnalytics script с precedingLF в HTML.
Actual observedHTML7102bytes/SHA394a8119a40734203a494f1b206d735d10969412d4dff947340e9004e4da2345;
source6735bytes/SHA9670bd6ab110e363a01c060f885464dc3f8e03abaf73b5e416b86254275bdeba.
После удаления ровно pinned insertion offset6719/length367/
SHAbbba70d1fbb140fe2cff2d40386e726bfe911227760ad6e69e29644e42b6f40a
получаются source bytes. Astra согласовал только эту точную квалификацию,
не произвольное stripping HTML: raw browser HTML сохраняется, criticalresources
и recording byte-exact, аналитика не исполняется и не проверяется.
CDP Fetch Request-stage guard проверяет каждый redirect hop и блокирует
этот точный внешний Script GET до передачи с подтверждённым ACK; всё остальное
вне same-originGET — FAIL. Independent final lifecycle review pending;
PASS ещё не объявлен. Native production network policy не обходится.

Следующий bounded LIVE3a автором подготовлен отдельно: realXY/XZ/YZ projection
с µm/равным scale, slice, exactID inspector/overlap selection, atomic state+tick.
Commitd975bf224140f48728a3946e60e71517546d4390; только viewer и Node tests.
Dependent independent review/ADR/finalbrowser acceptance pending после LIVE2;
source preparation не является acceptance и не меняет frozen SPEC или science.

Следующий шаг: final public QA exacthead run/artifact acceptance и guarded merge;
затем включить принятый QA main в LIVE2 и выполнить один final-head CI с
protected physical browser. Исторические архивация/refs сохранены; old authors STOPPED.

### Public QA принят; LIVE2 exact CI и everytick очередь

PR16 guardedMERGED85974e3b7eb886ec7d8df70966d91db08778ba54 после exact09a2b5c/
run37215665903/11checksSUCCESS и независимого actual bytes/trace/2PNG review0blockers.
Qualification ровно одной Cloudflare HTML вставки и намеренного analyticsblock
сохранена; application playback PASS, analytics execution NOT TESTED.
Durable evidence-only branch codex/native-cloud-evidence-20261004 commit
 ec28c71b4832ee5177cb7db0d9cebc704bea357e/tree95b81ca72954e62fc00f8e31ab0c0b28f9c9da27;
3originalZIP,4592189bytes, publishedrefreadback size/SHA/CRC3/3PASS;
manifestSHA33587c59d01ed19881cba04ed3558a8425a6b1c12ce5dd1093ffaab5427bc6f6.
21textentries bounded patternnohits и обаPNG viewed, неfullsecretguarantee.

LIVE2 freshmain finalcandidate f9197b8493672304dae99abe190cdae664774de0/
treea5370d4d7177cf89adccf5a92a32f1616f32d17e, PR17draft,
CI37217188695pending. Root mainintegration нормальным Gitmerge/push,
solver unchanged относительноreviewed4a2; finaldelta/artifactsreview pending.
Default и physical browser последовательно в existingprotectedjob,
publicproductioncheck skipнаproductPR и выполняется наmainpush.

Новое прямое поручение: пересобрать прежние publicrecordings с everytick на всём
10k/100k/1M горизонте, сначала оценить хранение. Root+Astra независимо измерили
existing201samples: currentJSON≈301MB/2.66GB/26.4GB; gzip≈40.4/348/3592MB,
не adjacenttick measurement/upperbound. Sourcebacked расчёт сохранён в
 docs/experiments/dense-recording/storage-preflight.json и README.
Первыйbounded10k streamingcodec на архивной базеb202 готовит scopedauthor,
безmodelsteps/Cargo у автора; rootbuild/run после coherentcommit/independentreview.
Никаких новыхLAB24runs, переписыванияoldJSON/SHA, fakeworld30source или infra.
LIVE3a rootdependentintegration подготовлен отдельно, его actualQA ещё в работе;
после приёмки2D nextpriority REC2everytick; Three3D потомотдельнымstage.

### LIVE2 full-CI blocker исправлен; источники записей сохранены

Main85974e3b7eb886ec7d8df70966d91db08778ba54 push run37216990692
завершён SUCCESS, включая реальную publicproduction playback проверку.

LIVE2 run37217188695 на f9197b8 завершён FAILURE: общий core integration test
`every_scenario_in_the_repository_loads` выбирал chamber loader только по имени
`cell-chamber.toml` и ошибочно подавал `cell-chamber-physical.toml` voxel loader.
Protected default15/physical16 browser checks и recorded22browser/17HTTP прошли,
но это частичные результаты failed CI, НЕ приёмка LIVE2.

Новый PR17 candidate33272bc452aca6a7e8716483da5d66941213cebf/
tree5d8dd0426d4970908c739bc7e902b6f9e07b8334 исправляет только inventory test:
все TOML проверяются по declared family, chamber_format через строгий
micro_config::parse_live; historical loader/core/config/runtime не изменены.
Focused config_hash16PASS, fmt и focusedclippyPASS; independent read-only
review0blockers. Публикация normal nonforce Gitpush; новый exacthead full CI
и actual browser artifacts обязательны до merge, прежний PASS не переносится.

Дополненный durable evidence-only ref codex/native-cloud-evidence-20261004,
commitebb79be4ed1ed8ea97fa235dd018e3d53df0db2e:
6 original ZIP без перепаковки, 29,807,240bytes; manifestSHA
b37b9c86c96e6a8b3e13e5a16d495ca98d63ace63145fcad6deaf4ae1550af0e.
Published-ref readback size/SHA/CRC6/6PASS. Это включает f919 browser partial
evidence, статус failed full CI выше; не productrelease/deploy.
104textentries bounded privacy pattern0hits,14uniquePNG viewed;
не fullsecretguarantee, recorded regression PNG не повторно просмотрены полностью.

REC2 author подготовил streaming producer/packer/independent decoder на
archival b2024ce, Python codec fixtures12PASS без Cargo/modelsteps.
Root actual clean-source build и bounded10k measurement ещё pending.
Каждый тик остаётся целью для всех трёх горизонтов; windows не подставляются.
LIVE3 physical2D QA дополнен реальными keyboard событиями с independently
ordered visible IDs и final post-cleanup error gates; final review/CI pending.


### LIVE2/LIVE3 приняты; dense пилоты и разрешённый объём

2026-10-04, 18:08 UTC. OWNERSHIP ACCEPTED сохраняется; единственный
coordinator/integrator/release owner — native Cloud. Старые product authors
STOPPED, архивные refs/VM не удаляются. Автопробуждение после окончания active
turn инструментально не подтверждено.

PR17 guardedMERGED `703085e76760924436396030dfd6f0a404daaadd` после exact
`33272bc452aca6a7e8716483da5d66941213cebf`, tree
`5d8dd0426d4970908c739bc7e902b6f9e07b8334`, run37219223184:
все11checksPASS; независимые code/numerics/actual-artifact reviews0blockers.
Полный CI f919 был FAIL и остаётся диагностикой, его browserPASS не переносился.
Original9ZIP в evidence branch `codex/native-cloud-evidence-20261004`,
commit`4d063a86ebcc397070cab03d9c2a37b4eb709c01`, 53,494,487bytes;
manifestSHA`a872b6277af1966703fe360ad5974a58462a5bb95866bc21e6104623ecb883c3`.

PR18 guardedMERGED `61d5c22edb3d1da13029c9faa6b506253d674f91` после exact
`65d3a85dcc2050e6b01552abe3549c1ed4aad8eb`, tree
`d9c69f71c50bc741f65e29dd72a050505faef550`, run37221244788:
все11checksPASS; независимые final source/code/numerics/UX и actual artifacts0blockers.
Это наблюдение сохранённых point centres вXY/XZ/YZ, равный SIscale/µm/slice,
exactID/overlap inspector, atomic state+tick и mobile normal-flow controls.
Protected pinned sandboxed Chromium: projection12checks/7PNG, independent
20geometries/134markers max1.61e-13px,43keyboardevents, delayed originalhistory,
actual divisiontick47. Остальные fresh reports15/16live и22browser/17HTTPrecorded.
Все8 held/physicalPNG дополнительно просмотрены; held byte/RGBA equal.
Нет claims collisions/spatialchemistry/nativebackground/achievedFPS/fullaccessibility.
Recorded trace не сохранил JSresponsebodies: HTTPsourcebyte attestation не
называется browserresponsebodyattestation.

Original5ZIP включая mobileFAIL сохранены безперепаковки в
`codex/native-physical-observer-evidence`, commit
`8eb63a98f505b8bf9a2d7b10f43194d7cca264b1`, 59,674,823bytes;
manifestSHA`df6b0507ae1c5cf7ec63ace700538135b80c23cc1fe0b9d2842dfd7f344658f8`.
Отдельная НОВАЯ GitobjectDB скачала ref по сети: все5 размер/SHA256/ZIPCRC PASS.
Archive manifest фиксирует более раннюю pending границу, этот checkpoint
фиксирует окончательную приёмку. Сайт на новом main ещё нужно проверять:
PRgreen не является productiondeploymentPASS.

Пользователь подтвердил «Каждый тик на всём горизонте — сначала оценить объём
хранения», затем «Объём около3ГБ допустим — проверить существующее размещение».
Everytick вместо окон; прежние3JSON/URL/SHA неизменны; LAB24 не пересчитывается.
Codec уже использует deltas/gzip, независимые keyframechunks≤256ticks,
≤1MiBgzip/4MiBdecoded, максимум8192chunks. Числа не округляются, точные
u64/i128 строки и f64bits/-0 сохраняются, cadence не снижается при capFAIL.
Источник oldengine world30/chamber1/seed42/dt30 `b2024ce`, actual producer
отдельно `bb70c9eb448b55ef3f50dfd6cd2a70213157bd8d`/
tree`93975cb83da179e15c60e937772bf9b656e5279b`; 7frozenGitobjects
совпадают со старым исходником. Runtime не является binary/kernelattestation.

Реальные native замеры:
| Horizon | Frames | Chunks | Total bytes | Export seconds | Full decode seconds |
|---|---:|---:|---:|---:|---:|
|100|101|1|61355|0.202|0.223|
|10000|10001|41|28166588|35.708|17.767|
|100000|100001|393|244639227|308.601|154.209|

Everytick/закрытый учёт0/oldframebit equality и known genotype dictionary PASS.
10k:225oldframe comparisons;100k:423. Full1M ожидает603comparisons/561uniqueoldticks.
Pilot bytes опубликованы `codex/dense-recording-pilot-evidence`,
commit`82d6dead4a518f70de340440e62623455e02e448`;
442datasetfiles,244,720,653unique bytes, manifestSHA
`4e2a1f448cb7220c1b4950c228cbf18706fca4335f4fb5d88227d0878ee7097c`.
Fresh networkfetch в отдельнуюobjectDB проверил все442 original sizes/SHA PASS.
Публичные команды имеют нормализованные cwdrelative paths; научные значения
не изменены, original rawreportSHA записаны, исходныеchunks/index/manifests exact.

Полный1M export + bounded independent validation запущен один раз с hardcap
3,000,000,000bytes, без удаления тиков, без миллионаframes array/fullJSONLfile.
Последний observed progress>350k ticks, процесс ещё работает; завершённым
dataset/PASS или actual2.45GB его пока не называем. 100k даёт прогноз≈2.45GB.
Caps: H≤10k64MiB;H≤100k512MiB;H>100k3e9; historic256MiBtarget не forecast.

Official existing-hosting docs checked read-only: CloudflareWorkers staticassets
25MiB/file,20k freefiles/100kpaid,staticrequests free/unlimited/noadditionalstoragecost;
actualaccount/build/uploadcapacity ещё НЕ проверена. GitHub recommended
Gitondisk10GB/directorywidth3000, enforcedpush2GB/file100MB.
План без новой infra/secrets/spend: sharded immutable chunks
`chunks/{ordinal//256:02d}/chunk-...jsonl.gz`, ≤32dirs/256files;
compressbytes не меняются. Publication в два boundedGitpush≤1.5GBновыхchunks,
branchpartial не используетсяmain/catalog. Index/manifests path-only rewrite,
originals retained; allpublicationmetadata учитывается в3GB.
Default publisher full bounded decode; explicit externally pinned coordinator
full-validation receipt разрешает schema-pass reuse, но ВСЕchunkSHA/CRC/decodedSHA
и prefix guards всё равно перечитываются. Receipt создаётся только AFTERfull1MPASS.

Следующий этап: закончить единственный1Mexport/validator; принять независимыми
code/numerics browserdecoder и publisher; затем подготовить sharded delivery,
everytick async observer/clock безmilliontimestamps, existingprotected realbrowser,
freshmain exactCI и actualproduction acceptance. Old201samples допускаются
только как явно подписанный sparseoverview/archive, не denseplayback.

### Full миллион принят; bounded delivery в работе

2026-10-04, 19:15 UTC. Native OWNERSHIP ACCEPTED сохраняется, прежние авторы
STOPPED. Все перечисленные разрешения/immutable boundaries действуют.

После main61d существующий run37223230012 завершилсяSUCCESS: все10 обязательных
jobs, actualpublic4checksPASS и2PNG просмотрены. OriginalpublicZIP3440989bytes,
SHA256`7f8f98e8d4ec0f067dfcd83583e6df97204d3d4afafa438f938dc02a3cb0a87f`
сохранён в evidence ref `codex/native-physical-observer-evidence`, commit
`5432dc91bfc376e9700546c86b745952331f51a5`; все6 originalZIP/63115812bytes
перечитаны новой networkobjectDB, size/SHA/CRC PASS; manifestSHA
`e075b6bf06c51b0ea4c17b9b5dc302ddd86d0d6019274238de77714358906481`.
Это старое sparsepublicplayback на принятом main, не denseproductionPASS.

Единственный полный1M export и full independent decode завершились PASS:
1000001frames/3909chunks/2421968874originalbytes, gzip2420633565bytes;
maxgzip1046955/maxdecoded4135813bytes. Export3058.079s/full decode1545.721s.
Все603 архивных frame comparisons на561ticks,826 used genotype checks,
полные known archive dictionaries и bothledger0 на1000000ticks PASS.
Qualified sampled producer+packer RSS23429120bytes/waited max23621632bytes
не обозначают общий peak всех процессов. Producer actualclean bb70/sourceb202/
world30/chamber1/seed42/dt30 и binarySHA455101… сохранились без смены identity.

После полногоPASS root выпустил закрытый pinned receiptSHA
`0cff85b03a18a63efeea14bc617a13dee956ac69b84dfd305b492fba9e7db1bd`,
public normalized reportSHA
`af409a80ff61fecd4600c971fac21b3fbc1cb30cbfbdb24f43af225156f88f47`.
Independent numerics receipt/report/source review0blockers; это доверенное
root evidence, не cryptographic binary/kernelattestation. Числа не изменены,
нормализованы только private runtime command paths; rawreportSHA сохранён.

Cleanpublisher923ea079a8b26ce4a98a797ea582683708585655, tree59fc932…,
выполнил actualLinuxFS no-replace probe и полную path-only публикацию всех3909
gzip: всеsize/SHA/CRC/inflatedSHA повторно PASS, prefixgenotypesPASS,
source receipt reused, decoded_frames_here0. Published complete total
2424602694bytes со всей metadata <3000000000. SourceindexSHA44b142…,
publishedindexSHA`c7d78508ccaf00729c20decd595ea0a3256e053a5562d35a58d68748e20b015b`,
publicationSHA`48cf7367c19e3fed7669e0a26473f6ceb02b720d6d0e70d19f140a672aa6368b`.
Исходныеgzip bytes unchanged; originalsindex/manifests/receipt сохранены.
Нового modelrun или повторного full schema decode publisher не запускал.

Combinedsource reviews0blockers: decoder bounded caches/abort/SHAstrings/
failure diagnostics; asyncUI pausedprefetchstickyfailure/pagehide; Linuxpublisher
FIFO/race fixes; catalog closedinventory/sharedprefix guard; protectedQA real
measured1× wall/modeltime вместо hardcoded hold; official sandbox/pins сохранены.
Source109Node/29Python/focusedfreshmainexamplecompilePASS, не browserPASS.
Review выявил CIshallow missingarchivedhelperbd2; root23ce2dd добавил exact
publishedSHA boundedfetch до29Python, independentreview0. Core/config/version/
LAB/oldJSON/frozen unchanged, ADRmainprefix+110appendonly. Actualcombinedtree
с complete data/catalog, fullCI/browser/evidence ещё pending.

Remote staging branch создан connector от опубликованного main61d:
`codex/dense-recorded-observer`. Первый partial2259chunks1399591759gzipbytes,
commit`1db6d26c094ebf25be93fc7cca912a8a1137945a`/treebba18e9… загружается;
catalog ещё старый, PR/main/deploy не обновлены. Второй boundedpush:
1650chunks1021041806gzipbytes + metadata/catalog. Partialbranch не PASSdataset.
Source/evidence docs: docs/experiments/dense-recording/million-2026-10-04 и
docs/checkpoints/2026-10-04-dense-recorded-observer.md на staging candidate.

Следующий конкретный шаг: закончить оба boundedpush и completeinventory/
catalog checks; exacthead protectedbrowser/fullCI+independentactualartifacts,
guardedmerge затем existingpublicdeployment/rawgzipbytes/capacity verification.
Документированные hostinglimits не заменяют actualdeployment. СтарыеURL/SHA,
LAB24 и все historicalevidence сохраняются. Автопробуждение послеactiveCloudturn
всё ещё не подтверждено.

### Complete candidate PR19 и первый protected browser отказ

2026-10-04, 19:33 UTC. Обе boundedpush завершилисьSUCCESS. Полный remote
candidate `97aed6f1c0c0dba7886fb34d0febf4c189eedf29`, tree
`f6feff861be3da22c6166264fcdde65e9c11c362`, PR19
https://github.com/emevart/liminis/pull/19, base61d unchanged, draft.
Independent final code/catalog/inventory/source review0blockers; all3919
regular files/3909chunks/sharedmetadata/prefix/cap согласованы, nativehelper
проверил SHA каждого файла. Снятие толькоdenseattachments восстанавливает
все старые catalog fields exact. Complete-data109Node/RustfmtPASS.

Exact run37228121227: site/fullinventory/Python29, numeric, all3hooks,
frozen/world и live/physical/projection jobsSUCCESS; buildlint/test ещё работает.
Existing WorkersBuild exact97 SUCCESS, version
`6d08d119-d7ba-4e9d-9226-605a1c848394`, build68c766e4-c5b0-48a2-9dcc-52f5118e1ce9.
Это фактическая existingintegration сборка полного candidate, не отдельная
аттестация всех publicservedbytes. Новый hosting/секреты/расходы не созданы.
Source/run подтверждают Actionscheckout завершается; nativefreshdownload
скорость не переносится на runners, CI не ослабляем/не меняем без actualneed.

Recorded архивная QA22checksPASS. Новый densebrowser завершил все11subchecks:
101actualframes, corruptionSHA/CRC/truncation/multimember, exactfirst/largest
chunkcontrols, nativebodyabort/stale/close, atomic255→256 originaldelayedbody,
measured1×Δmodel1.540s/Δwall1.5404s, heldpixels/state/display30/60,
4viewport и actual100k/1Mendpoints. Однако finaloverallFAIL:
3 net::ERR_ABORTED отсутствуют в узкой intentional classification.
Ни page/console/unhandled/cleanup errors не наблюдались. Все11subchecksPASS
НЕ переносятся в общийbrowserPASS. Original denseFAIL ZIP сохранён native,
16581557bytes/SHA256`0fdff34b12e5304ad718980061e10ddeb9cffedc097cdc0add3176c0fc8f3b6a`,
artifact11312631748, включаетreport/trace/5PNG; bytes/ZIPCRCchecked.

QAauthor и Astra разбирают exact per-request nativeAbortSignal/reason/order/
bodydone correlation. Одна late signal.aborted недостаточна: getfinally abort
может быть cleanup послеfailure, TimeoutError не intentional. Нельзя разрешить
все site ERR_ABORTED URLpattern или снятьfinalgate; source/body/Responses
не подменять. Нужен новый protectedrun исправленногоexactHEAD; nativebrowser
в этой среде не запускается, миллионаmodelticks повторно не считать.
Independent actualFAILartifact review идёт отдельно отauthor.

Отдельная новая nativeGitobjectDB скачивает complete97 по сети, без restart;
последний observed500MiB progress. Full3919size/SHAreadback ещёpending,
не объявляется завершённым. Main неmerged; publicdense acceptance pending.
Следующийшаг: narrowQAfix+independentreview → freshfullCI/exactbrowser/artifacts,
завершить readback/архивoriginalFAIL+acceptedZIP, guardedmerge приgreen,
existingactualpublicsource/gzipbytes verification.

### Complete network readback и fresh exact-head CI

2026-10-04, 20:13 UTC. Fresh network fetch complete97 в отдельную Git object
database завершён, без shared-object reuse. Closed inventory3919files,
3909gzipchunks, size/SHA256 каждого файла и total2424602694bytes PASS.
Public small report находится в candidate docs/experiments/dense-recording/
million-2026-10-04/network-readback.json. Это сохранность Git bytes, не новый
model/decode/browser/publicdeployment claim.

Original97 browserFAIL archive опубликован отдельной evidence-only веткой
`codex/dense-recording-browser-evidence`, commit
`51323628a2e8cb0b2ab1ce29b8e4d070c7227c81`, tree
`f808cbb7a2d603558628f9d792b5302b62031f4a`. Original16581557-byteZIP SHA
`0fdff34b12e5304ad718980061e10ddeb9cffedc097cdc0add3176c0fc8f3b6a`.
Fresh network archive readback size/SHA/ZIPCRC PASS. Independent actual
report/trace/source review и5PNG viewed; initialoverallFAIL не меняется.
Остальные10checks exactrun37228121227 завершилисьSUCCESS, включая Rust.

Узкий QA fix author798f604 принят independent source reviewer0 после закрытия
двух обнаруженных blockers: native rejection observer masking и отсутствующий
post-cleanup aggregate. Actual signal/reason/bodyEOF/CDPidentity proof имеет
18 synthetic metadata self-checks. Отдельная signal-only фаза сохраняет
nativePromise/Reader semantics; real saved16-byte mid-body abort/preabort
проверяются без orphan masking. Exact-identity synthetic native orphan control
проверяет error-channel sensitivity, raw event сохранён, unknown/repeatedFAIL.
Эти source/Node checks не являются actualChromiumPASS; данные immutable.

Новый clean published PR19 candidate
`e2451e3142def791eb44aba4fcb19ada1aa1ed78`, tree
`27c506c9b4aed0fbdaaa576f36bb1b768af8ae3a`. Mandatory fresh protected full CI
run`37231067824` на exactHEAD IN_PROGRESS. Existingbranch normalpushSUCCESS,
PR body обновлён, draft/main61d unchanged. На каждом новом head приёмка заново;
accepted subchecks исходногоFAIL не переносятся.

Следующий шаг: exact-head11checks и independent новых protected artifacts;
durable bounded originals/readback, guardedmerge толькоgreen+review0,
существующий public dense gate с catalog/module/index/manifest/gzip bytes.
Сохранность архива подтверждена; publicdense acceptance покаpending.

### REC-2 принят к merge, публичное воспроизведение отдельно

2026-10-04, 20:35 UTC. Exact e2451e3142def791eb44aba4fcb19ada1aa1ed78 /
tree27c506c9b4aed0fbdaaa576f36bb1b768af8ae3a, run37231067824: все11checks
SUCCESS, включая полный Rust build/lint/test/exporter, оба protected browser
jobs и existing Workers Build. Final source/doc review0 blockers. Actual
dense artifact independent qualified PASS0:14checks,5PNG viewed,20exactsource
pins,11actualaborts independently correlated, finalaggregate0. Numerics и
регрессии independent qualified PASS0:79checks пяти reports, frozen/source/
original bytes, dt30/H+1, measured1×1.503model/1.5034wall и publication budget
согласованы. Full browser decode миллиона frames/peakheap/hardwareFPS/native
BFCache/full response-body coverage не заявлены; подробные qualifiers в reviews.

Original ZIPs и exact own reviews опубликованы evidence-only:
- `codex/dense-recording-browser-evidence`, commit
  `4a4ee92fd1bf0abe4dcfa3f1f9469a0e8719e537`, tree
  `5efa2cfef82b1dcf6600edf087d8b6891affca38`:3ZIP/45312641bytes,
  включая initial97FAIL, new dense и legacy;5small ownreview files.
- `codex/dense-recording-regression-evidence`, commit
  `0541137966aed15f350422ba9cc25a06e7324ddb`, tree
  `bc7d975635e626cd91d142d944e5a89576e799bb`:3ZIP/40020865bytes,
  LIVE/physical/projection плюс numerics и qualified privacy report.

Оба bounded archivegroups<64MiB; fresh network fetch в две новые отдельные
Git object databases подтвердил каждый original/review file size/SHA и ZIP
CRC PASS. Public readback report:
`docs/experiments/handoff/dense-e245-archives-network-readback.json`.
Это saved byte evidence, не новый model/decode/browser/deployment claim.
Old97 overallFAIL сохранён, original ZIP не перепакован/не заменён.

PR19 ready и guarded merge с expected exact e245 SUCCESS. Новый main
`239ec8bde3d2f15e2aa5b50e9294e65eab5c0926`; merge tree совпадает с candidate.
Main push run`37232682967` IN_PROGRESS. Existing public gate отдельно должен
подтвердить actual deployed catalog/module/index/manifest/gzip SHA и browser
Next0→1/1×/held state. Пока этот gate pending, publicdense PASS не объявлен.
Новый hosting/секреты/расходы не создавались. Все legacy URL/SHA/LAB сохранены.

Реальный Astra read-only выбрал следующий bounded LIVE-3b после public REC-2
приёмки: дополнительный ортографический Three.js3D view реальных chamber2
centers в существующем live UI, camera/layers/opacity/slice/exact inspector.
2D начальный; нет новых biology/coordinates/fields/interpolation/physicsbody.
Pinned vendored modules с MIT/provenance/полным import closure, explicit host
routes, отдельный WebGL canvas/lifecycle. Read-only проверен stable upstream
three@0.180.0/r180, tag9e8635e…→commit0af9729d0c143a86a1d725d6e2c3ad83301f3f34.
Files/ADR111/code/preflight пока не начаты. Protected WebGL2 availability —
реальная первая неопределённость; при отказе sandbox не ослаблять.

Следующий шаг: принять existing public main artifact/source/rawgzip bytes,
сохранить оригинал+readback, затем открыть scoped LIVE-3b и append-only ADR111
на fresh main. Автопробуждение после активного cloud turn не подтверждено.

### REC-2: существующий публичный сайт принят

2026-10-04, 21:00 UTC. Main239ec8b/tree27c506c, push run37232682967:
все выполняемые mandatory jobs SUCCESS, включая Rust и public browser;
world/frozen jobs штатно skipped на push, их exact PR gates ранее SUCCESS.
Existing Workers Build SUCCESS. Source tree не изменился после принятого PR19.

Actual public artifact11313789667: original5601441bytes/SHA256
`688833bc16b805f6433ecd5fb296a3b8e0ec10c6e3481f423b785ae4e9889b48`.
Independent **QUALIFIED APPLICATION PASS,0 blockers**:4checks/2PNG viewed,
14Node HTTPS source/metadata/gzip pins и28browser response pins, Next0→1,
measured1×/held state, cleanup и sandbox argv. Первый stale/404 attempt и
один5s deployment retry сохранены. Известная367byte Cloudflare insertion
строго закреплена; analytics Script GET2 заблокированы до отправки с ACK,
analytics execution не принят. Scope: default10k initial/prefetched chunks;
full2.42GB public readback и public100k/1M endpoints не заявляются. Полный
dataset проверен отдельно в Git и accepted existing provider build.

Original и5exact ownreview/validation files опубликованы в evidence-only
`codex/dense-recording-regression-evidence`, commit
`3b433f5d68ad024a6e0ff8a0c03c8d7f71387ccc`, tree
`9782bf77495adb302264d3e7d6f32dbac7e47457`. Группа4ZIP/45622306bytes<64MiB.
Incremental network fetch нового original/reviews в ранее свежую отдельную
Git database: все13archive files size/SHA и4original ZIP CRC PASS. Из-за
shallow ancestry использован новый verification ref, опубликованный remote
не менялся. Small readback report:
`docs/experiments/handoff/dense-public-239-archive-network-readback.json`.
Предыдущая группа dense/legacy/oldFAIL на4a4ee92 остаётся сохранённой.

REC-2 source/CI/numerics/actual artifacts/storage/public application gates
закрыты в описанном scope; старые записи, LAB24 и frozen identity сохранены.
Следующий разрешённый bounded этап OPEN: LIVE-3b по уже принятому решению
Astra. Root создаёт отдельные branches/worktrees и append-only ADR111,
автор renderer/host assets отделён от browser QA и независимых reviewers.
Protected WebGL2 availability проверяется actual gate; native Node не
объявляется WebGL/browser PASS. Никаких новых infra/secrets/spend или
ослабления sandbox. Cargo сериализует root; shared ADR/CI/checkpoints — root.

### LIVE-3b: source и exact CI зелёные; доставка original3D evidence

2026-10-04, 22:26 UTC. OWNERSHIP ACCEPTED сохраняется: native coordinator
продолжает разрешённую очередь, old Work Cloud authors не возобновлялись.
Main остаётся `239ec8bde3d2f15e2aa5b50e9294e65eab5c0926`.
Draft [PR20](https://github.com/emevart/liminis/pull/20): exact candidate
`9d9f70903939b93905beed9e0ad2be5c3f4d18ff`, tree
`7f6b3ea5a67810708b6a39e5e78f418d6b2d1b2d`; run
`37238281987`: все11checks SUCCESS, включая Rust build/lint/test/exporter,
оба protected browser jobs, все4LIVE сценария и existing Workers Build.
Новые real3D проверки используют unchanged official Playwright1.61.1,
chromiumSandbox:true и observedargv guard. Stock software SwiftShader
сохранён; hardwareFPS/kernel sandbox attestation не заявляются.

Scoped source/QA authors завершены, опубликованы и clean. Independent
source/numerics review0blockers, native30Node/32hostchecks PASS. First09
overallFAIL (caption regression,3DNOTRUN) сохранён; one-line accepted2D
caption fix не меняет geometry/vendor/biology/QA. Старый FAIL не переименован
в PASS. Shared append-only ADR111, ACCEPTANCE и phase checkpoint опубликованы
в candidate. Frozen docs, old recordings/LAB24/source identities сохранены.

Original diagnostic4.25MB и exact independent reviews опубликованы в
evidence-only `codex/native-3d-observer-evidence-20261004`, commit
`471993728dd81d5eb77e7d9d743122f7808611ba`, tree
`210001fcb40ca902e2724d830bf71e29589b09db`. Initial exact1f38 archive
fresh independent Git database readback:14files/1originalCRC PASS; новое
closed17file diagnostic471 readback выполняется. Группа пока4.34MB<64MiB.

Пять original9d regression ZIP уже скачаны через existing GitHub connector:
legacyREC12704737, dense16422044, LIVE5011027, physical6876222,
projection30501374bytes. Размер/SHA256 совпали с official artifact digest,
каждыйZIP CRC PASS. Actual79reportchecks independent acceptance в работе.
Это проверка bytes/reports, не повторная симуляция или million decode.

Реальный оставшийся blocker: original3D artifact`11316208815`,44944288bytes,
SHA256 `e417e0a26e61a28315d080e0ea4b42153bd033d090449e9c52958b9301097e98`.
Connector managed download успешен, но native download_file ограничен32MiB;
выданный managed read URL через inherited proxy/TLS заблокирован. Network
policy/proxy/CA/security не менялись; credentials и signed URL не публикуются.

Реальный Astra одобрил bounded official Actions companion read-job:
отдельная evidence-only push ветка, без PR/checkout/model/browser; только
стандартный actions:read token, fixed same-repo artifact/run/head/size/SHA.
Original ZIP делится без перепаковки на3raw parts16777216/16777216/11389856,
каждая в отдельном small companion artifact с receipt. Native соединит
части и повторно проверит originalSHA/size/CRC. Productcandidate9d и его
gates не меняются. Root-owned helper подготовлен в
`codex/evidence-transport-3d-20261004`; publication после independent review.
Новая infrastructure/hosting/secrets/spend не создаются.

Следующий шаг: доставить original3D bytes, independent screenshots/trace/
API geometry/resource/argv acceptance, bounded originalZIP archive с новым
Git network readback; только затем guarded PR20 merge и existing public
delivery follow-up. LAB-3a scope Astra выбран read-only, coding пока не открыт.
Никакой вечный auto-resume после завершения active turn не обещан.

### LIVE-3b принят и объединён; новый main FAIL сохранён, LAB-3a открыт

2026-10-04, 23:17 UTC. OWNERSHIP ACCEPTED. PR20 guarded merge выполнен
после exact candidate `9d9f70903939b93905beed9e0ad2be5c3f4d18ff`, всех11
SUCCESS checks run37238281987 и независимых actual13GL+79regression checks
с0 findings. Новый main `5beaafb2c4f717862b62e67fd3e23e7d81860535`, tree
`7f6b3ea5a67810708b6a39e5e78f418d6b2d1b2d` идентичен принятому tree.

Original3D44944288bytes доставлен fixed evidence-only Actions helper
ea6bba6a930e88715f7898a5da658a81f4d44ccd, run37240071604. Три raw slices
соединены без перепаковки; original size/SHA/CRC совпали. Все7 originalZIP,
собственные source/code/numerics/artifact reviews и receipts опубликованы
в трёх отдельных группах, каждая<64MiB, с независимым закрытым Git network
size/SHA/CRC readback: observer `e7bd2662aa76ab463a709cb4da08f2143d3e5608`,
regressions `ff5e3fec5647fbd7a723e9b98b20e0e50cd287fe`, recorded
`d3d707639e0860d69bb60f529956e94f86707576`. Итог121544108bytes/56files.
Результаты: `docs/experiments/handoff/live-3d-archives-network-readback.json`
и `live-3d-9d-exact-head-checks.json`. Распакованный объём отдельно от cap.

Новый main push run37241483046 завершён FAILURE: 3D check8 сравнил PNG SHA
после paused display60 с baseline и получил mismatch. Original18,078,055B,
artifact11317433841, SHA256
`71ab3e8085e7c13e49920b4b505e118c0df882d3ee89678c66b528a2e8e874ab`
скачан, CRC/source pins/trace проверены; FAIL не переименован. Independent
code/numerics подтверждают неизменные22API состояния tick1, но исходные
baseline/30/60 canvas PNG и post-FPS camera audits не сохранены. Причина
encoding/raster/timing неизвестна; production fix и flaky claim не обоснованы.
Astra явно одобрил узкое QA уточнение: same whole compositor region,
строго dimensions+decoded RGBA, никаких tolerance/masks/retries. Все три
original captures, encoded/decoded hashes, camera/viewport/API/layers и
pixel diff count/bounds должны сохраниться до assert. Нужен свежий exact CI;
прежний main3D FAIL остаётся историческим фактом.

Main public artifact11317418838: 5634585bytes, SHA256
`7686895bf55e00810d676aeaf2ebd948bc68678481c238eebe26fb1b82cc7d1f`.
Independent qualified PASS0:19 HTTPS source/metadata/data pins включают
final gzip всех3 horizons; default10k desktop/mobile Next0→1 и1×/hold
подтверждены actual protected browser/2PNG. Нет full2.42GB HTTP readback,
public100k/1M rendering или overallmainGREEN claim. Эти два новых originals
и собственные reviews готовятся в отдельный bounded durable archive.

LAB-3a по approved ADR112 открыт: data43356e5d674053d269290a988677062b7325d9a1,
UI fa4ce68e5896ada440eccfd84e64b8173551f0dd опубликованы clean; scoped
browser QA в работе. Root integration codex/lab-aggregate-site основан на
5bea и владеет общими ADR/ACCEPTANCE/navigation/CI. Raw24/source21cbe907
сохранены, новых model runs нет. Data15Python и UI12Node PASS означают
source checks; actual LAB browser/fullCI/independent acceptance ещё pending.
Поручение пользователя сохраняет write-go и делегирует решения Astra.
Следующий шаг: narrow QA repair/review, LAB source integration и один
fresh protected exact-head CI; merge только после green независимой приёмки.

### LAB-3a exact source опубликован, один fresh CI в работе

2026-10-04, 23:28 UTC. Draft PR21:
`79df352a045cf7dd0efc14a232e867365c7b75a7`, tree
`0c9b9a5530111e418e49a675a946b5f007367c21`,
https://github.com/emevart/liminis/actions/runs/37243706546.
Data/UI/QA authors clean published и завершены. Root integration объединён
normal cherry-pick; homepage Lab link, builder freshness/focused checks и
новый LAB gate в existing protected recorded job добавлены root. Observer
HTML/knownEdgeInsertion/source pins остаются byte-exact; production3D/core/
config/frozen/rawLAB/oldREC сохранены. Integratedpreflight160pairedrecords,
all24/4044/3raw pins PASS, actual browser ещё NOT_RUN.

Отдельный QA-only repair commit3d48f47 сохраняет whole exact dimensions+
RGBA/all3originals и pre/post actualAPI/camera/viewport/mode/revision/layers,
diffcount/bounds до assertions;4 focused PNG controls PASS, independent
source review0. Astra одобрил один полный CI без пропуска mandatoryjobs;
production3D не правился и oldmainFAIL не relabel. LAB numerical review
независимо проверил3672 differences+2835 range/count records по raw24,
без импортов producer/validator/UI; итоговый source verdict оформляется.
Astra source/UX architecture0; actual mobile PNG должны подтвердить
читаемость осей, source/Node её не доказывают. Merge/publicLAB pending.

Оба новых main originals и собственные diagnostic/public reviews сохранены
в evidence-only `codex/native-main-3d-followup-evidence-20261004`, commit
`7500fadc65aa8639f0c31820ed9a7b5654cea76f`, tree
`3dff3a5abc8707f5326d0482bcd81862833e7a90`. Original main3D FAIL и public
qualified PASS остаются раздельными. Новый независимый Git object database
без alternates: closed17files/2ZIP size/SHA/CRC PASS; published24,084,194B
сmanifest<64MiB, manifestSHA256
`03076ccab35263b905a29465649a6aada415bc45b294f71f80464bc9e9e4016d`.
Small readback: `docs/experiments/handoff/main-3d-followup-network-readback.json`.
Следующий шаг — exactCI и originalfixed3D/LAB artifact acceptance;
public LAB QA подготовка обсуждается с Astra отдельно от production candidate.

### LAB-3a реальные два FAIL сохранены; 3D принят отдельно, narrow repair в работе

2026-10-05, 00:17 UTC. OWNERSHIP ACCEPTED. Пользователь до утра отсутствует;
вопросы и спорные решения передаются реальному Astra. Active native turn
продолжается; подтверждённого механизма вечного auto-resume нет. Main остаётся
`5beaafb2c4f717862b62e67fd3e23e7d81860535`; draft PR21 не объединён.

Первый LAB candidate79 run37243706546 FAILURE: exact counters/histogram
прошли до проверки CSSOM bar96.7391% против unrounded89/92*100. Узкое
QA исправление сравнивает exact actual CSSOM с independently constructed
CSSOM, без расширения numerical tolerance; исходный FAIL сохранён. Original
LAB ZIP2210725bytes SHA256
`bbd3f4dce56469c082be6e61f8d9abdd913d48987af6575620a6493e4bcb3a09`
и собственные source/numerics/failure reviews сохранены в evidence branch
`codex/lab-aggregate-evidence-20261004`, commit
`41f38a5d1ca57f01ad21088bd14ce92117c906aa`, tree
`eb4c2b2682336d4c9b90042c93ca54c182c648bf`. Closed20files/1ZIP2310183B,
manifest `db972e9ff96862843b0ed683c7dfd8dacefa028cf26cd2e01177681df690632b`,
fresh independent Git object database size/SHA/CRC PASS.

Source/UX review actual первого PNG выявил мелкие оси; Astra одобрил
210CSSpx responsive SVG с actual measured width/11CSSpx axis-only rounded
labels. Root candidate `60015b28a42e222fcdafd33ed80da6c81f57c6db`, tree
`117583151afcc3e6326971f87c44f3e41bc56eac`, full CI run37245225939
FAILURE. Actual LAB first6gates PASS:24runs/48endpoints/all raw curves,
160published pair selections, controls, real URL/back/reload/keyboard.
Gate7 layout FAIL: plotFrame throws too-narrow after clearing plots;
ResizeObserver cannot recover from partial tree. Two sticky initial/reload
comparisons body aborts remain unresolved; initial abort precedes reload by
3minutes, so navigation-only explanation rejected. Run budget210s retained;
no unchanged retry, tolerance widening, skipped gate or source PASS transfer.

LAB600 original artifact11319250919:49316006bytes SHA256
`bdc0b53a6543c560b505737a83c1dcae214f74d79bdad05ecdb9793307b06b80`.
Native32MiB limit закрыт reviewed fixed evidence-only helper
`5148bfea758292f0d67babc941d38ba4ba993ac2`, tree
`459189b6661eda76c1e40807ee39492e3af65fd8`, run37246516009:3raw ranges
16777216/16777216/15761574 + separate original report companion.
Native original size/SHA/outerCRC и original report exactbytes PASS; сам
browser verdict остаётся FAIL. Independent failure/trace/privacy review
и durable publication этого второго FAIL ещё pending.

Candidate79 original physical3D artifact11318935319 отдельно accepted
qualified13gates PASS, independent code/numerics0. Original59183146B SHA
`8d773a29f6bef2443124a73c31917189dd4a921ae6972e6c08ba83de2d49aa60`,
22outer+1595nestedCRC, all12PNG viewed, all3paused captures encodedbytes
и decodedRGBA exact/no changedpixel, pre/post API/camera/viewport/layers
проверены independently. Oldmain5bea FAIL причина неизвестна и не relabel.
Software WebGL2/stock guarded SwiftShader qualification; no hardwareFPS
claim. Expanded traces131207138B выше64MiB; archive cap толькоpublishedbytes.
Evidence branch `codex/lab-3d-79-evidence-20261004`, commit
`e43cb056ba61f38822b1a3f21815bb73eda9d169`, tree
`ab86c981875bd8670961106e9d6988c71d6da4f3`:closed11files/1ZIP59546673B,
manifestSHA `4444c926a8eb800e88cea2e71057073068497b4c5f7e0c699067095e9dbd6a96`.
Fresh independent Git object database normal network size/SHA/CRC PASS,
readback `docs/experiments/handoff/lab-3d-79-network-readback.json`.

Astra approved narrow repair: permanent4plot shells, measureall usable
widths before replacingcurves, detached construction/synchronouscommit,
hidden/zero-size defer to next RO/pageshow without retry loop; unexpected
error visiblefatal. Separate QA batches DOMreads while retaining all
9gates/48endpoints/all24/all160pairs/210s. Unreached compactJSON transport
negative-control LF assumption replaced by approved single numeric e→E
spelling with exactoffset/sameparsedvalues/samelength/differentSHA.
Scientific source21cbe907/world30/chamber1/raw24SHA6fd944 remains unchanged;
no new modelrun during this delivery repair.

Public LAB QA preparation `codex/lab-public-smoke-preparation`, commit
`64bf82130e5992947c9d2ee999eb018dc92d0a51`, tree
`98a485613794cebcf7b7d5938094999f1168c0a1`:independent source review0,
NOT_RUN. Manifest deployed product SHA intentionally null; runtime refuses
before public requests. After accepted guardedmerge/deployment only: repin
accepted candidate/tree/deployedmerge and all8assets, source delta review,
existingofficial protected browser/public bytes/readback. No new hosting.

Следующий шаг: independent nativeFAIL600 review/archive, narrow UI/loader/QA
fixes и fresh exactfullCI; только GREEN/review/runtime acceptance разрешает
PR21 merge. После accepted/public LAB3a Astra выбрал отдельный bounded LAB4a
dt30/15/7.5s sensitivity plan; до этого новые model runs не запускаются.

### LAB600 durable FAIL закрыт; три narrow fixes на едином чистом candidate

2026-10-05, 00:32 UTC. Original LAB600 FAIL archive опубликован в
`codex/lab-600-failure-evidence-20261005`, commit
`8c8347764b6a4d2bd41386cff73ce00e14b3acc5`, tree
`750c4b18096338d2d513e8f3d0d5fdca07dc22fe`:closed14files/1originalZIP,
49,824,379bytes<64MiB, manifestSHA
`3d5b70b29f0a1fc8c1f2c2d38adc34b452e1dd81d07ec7f7b1b1a1bc3820d42a`.
Fresh independent Git database without alternates normal network size/SHA/CRC
PASS. Один lazy blob fetch получил HTTP503; повторная проверка прошла без
изменения сети/security/credential settings. Readback:
`docs/experiments/handoff/lab-600-failure-network-readback.json`.

Independent code confirmedclosed5outer/416nestedCRC/13Gitpins/3PNGFAIL;
independentnumerics reconstructed272reportedSVGpathSHA/39152points exact,
но actual raster blank и fullLABFAIL сохраняются. Два ROexceptions внутри
fullPage captures176446/192287ms, второй до shortresize192456ms. Actual
transient width и cancellation timing не записаны; причинность cleanup
исправления со старыми abort не заявляется.

Root integrated clean `1be0d0940ef75aa00d6152e5b718df0cc5fb446f`, tree
`64bdb5f4794f48907f99886c540f6391571b1914`. Narrow source authors:
loader4c0a31a/independentcode+numerics0; UI6a6dbdf5/permanent4shells/atomic
commit/finite0..96pending+visibilityhidden/no loop/fatalinvalid; QA41089782/
DOMbatching/exact e→E offset5496/no parsedvalue change/all9gates/210s.
Nativecombined30Node and15Python PASS; builderexact24/4044/6494550rawbytes
PASS, syntax/diffcheck clean. Rawscience/core/config/frozen/oldREC/production3D/
observerHTML/publicpins/CI unchanged vs prior approved source boundaries.
Independent final UI+QA source review pending; fresh CI NOT_RUN yet.

Public smoke preparation now `3788400ea3dbf9b7de7da555c6b8f6d86748f72f`, tree
`2327d27bb9cce58b590bd2f78894ffbba01ae402`:pending readiness plus repeated
32raw endpoint/frame/font assertions AFTER fullPage capture, independent
source review0,17preflight controlsPASS. HTTP/browserNOT_RUN, manifeststill
old79/null intentionallyblocks runtime. After final acceptedmerge/deployment
only: adoptaccepted8sitebytes, repin candidate/tree/deployedSHA, independent
source delta review, official existing protected public smoke.

Следующий шаг: finalsource reviews0, ordinarypush exact1be combined head,
freshfullCI/nativeoriginalartifactacceptance/durablebytearchive/readback;
PR21guardedmerge толькоGREEN. Пользователь не получает вопросы до утра;
спорные решения Astra. LAB4a пока только selected bounded futureplan, models
не запускались.

### Fresh1be LAB initial FAIL: transport diagnosis без speculative production fix

2026-10-05, 00:54 UTC. Exact1be0d0940ef75aa00d6152e5b718df0cc5fb446f/
tree64bdb5f4794f48907f99886c540f6391571b1914 freshCI37248220760: recorded
job111570284160FAIL, LIVEjob111570284210SUCCESS; Rust ещё running при
фиксации. OriginalLAB artifact11319619204:643462B/SHA256
21ff9f8d414b1f2346a9bdef92147eaadec2adaa95cd41a65d5b26ef404cfe4c
скачан native, outerCRC/sourceHEAD/tree PASS. Gate1 observedargvPASS, gate2
FAIL до sample/comparison/layout/negative gates: descriptor/results failed
net::ERR_ABORTED, две PWNetwork.getResponseBody NoData; comparisons/manifest
bodypins доступны и exact. Page/console/Nodeunhandled/cleanup0; browser
__labQaUnhandled array не прочитан из-за earlyexit, browser-unhandled0 не заявлен. ActualPNG сейчас
содержит4complete visible plots и24matrix, но это не9gateLAB acceptance.

Originaltrace: descriptorbody NoData941.006ms, results1197.849ms; ready
1197.387ms, firstfailurePNG1298.160ms. До этих ошибок нет navigation/reload/
screenshot/cleanup. EOFcleanup исправление не устранило наблюдаемые abort,
причинная связь не заявляется. Source0 остаётся source0, actualFAIL сохранён;
main5bea иdraftPR21 неизменны, public smoke/models не запускаются.

Astra explicitly одобрил ОДИН evidence-only transport diagnostic в existing
repo official protected runner безCargo/models/deploy. Candidate1be/tree64
и8siteassets immutable; отдельные diagnosticHEAD/tree/scriptSHA. Одинinitial
positive load, единственныйCDPFetchowner, Request-stage fixedloopback8GETguard
передпередачей и Response-stage originalHTTP200body size/SHA через
Fetch.getResponseBody→continueResponse безheader/body/status override,
безroute.fetch/fulfill/takeResponseBodyAsStream/secondHTTPfetch. Bounded
Network timeline/requestIDs/terminalerrors/PNG; любые errors diagnosticFAIL,
безURLallowances/retry. DiagnosticнеполнаяLABacceptance инепрежнийPASS.
Rootownsworkflow, scopedauthorowns2scripts; independentreview доsinglelaunch.
Preparebranch `codex/lab-transport-diagnostic-prepare-20261005` не trigger;
futureRUNbranchonly `codex/lab-transport-diagnostic-run-20261005`.

Source0 combined1be reports appended ordinarynewcommit to first79archive:
`35c590d588568e896661b3731c466a244732be21`, tree
`45959aa3d8045ded9894867773c7cad9ab929923`, closed30files2357319B,
manifest74781300ad700ace76281983482a81379cc7f3af9722b3b20a1f181f0c4b3c28.
Old files/41f38a5d originalmanifest preserved; freshindependentGitDB latest
size/SHA/CRC PASS afterone503 lazyfetch retry. Readback:
`docs/experiments/handoff/lab-aggregate-source1be-network-readback.json`.
Publicprep eeb0499 pinsall8candidate1be assets/source0/17controlsPASS;
deployedSHA=null и runtimeNOTRUN. NextLAB4a designindependently0, budgets
194000steps/99328000cellticks<unchanged100M; prereg/admission/models deferred
untilLAB3aaccepted/public.

Следующий шаг: independent original1be review/archive, diagnostic source0
иsingleofficialrun, затем обоснованнаяQAправка/freshfullCI/runtimeacceptance.
Не отменятьreadercontract/менятьproductstreamingнаугад и не ослаблять
responsepins/errorgates. Пользовательотсутствуетдоутра; решенияAstra.

### LAB1be original durable closed archive и diagnostic source gate

2026-10-05, 01:02 UTC. FullCI37248220760 completedFAILURE:9of10GitHub
ActionsjobsSUCCESS, recordedLABjobFAIL. Latest Workers check на том же
sourcehead1be ещёin_progress послеevidencebranchcreate; неGREENclaim.
OriginalsmallLABZIP643462B plus independent source/trace/PNG review appended
to first79archive withoutrepacking original bytes. Correct finalcommit
`d6c6db823c72022ff72e6890fa1c3542a7c94f83`, tree
`7cc28402166fe7c9970b1167e118bc7853cb1fbb`, manifestSHA
`2e087bedff465ece0393e113f062c4cff238babffd4ea919dd98b26c6eb291ed`,
closed35files/2ZIP3037715B<64MiB. Independent normalnetwork Git database
readback size/SHA/closedinventory/CRC PASS; report
`docs/experiments/handoff/lab-aggregate-earlyfail1be-network-readback.json`.
Intermediate2b9fb6f published newfiles beforemanifest regeneration due wrong
oldschema key; it is NOT acceptedclosedarchive. Ordinarynewcommitd6 repaired
manifest and newverificationPASS; previous35c/41originalversions unchanged.

Native diagnostic workflowprepared root-owned SHA
ade77f54266c73cef921fd198554fab1f469252b139bbabcdbda50e0dd5ed199.
OfficialUbuntu22/Node24/PW1.61.1, contentsread/exactgithub.sha/persistcredFALSE,
fixedcandidate1be fetch+8assetpreflight, noCargo/model/deploy. Independent
workflow0; script combinedreview found final-page-ready guardgap before
anylaunch. Authorfixing; singleofficialdiagnosticNOT_RUN. Candidateproduction
source unchanged and publicdeployedpinNULL, main5bea stable.

Следующий шаг: close diagnostic source0+finalafterPNGready guard, freeze
clean diagnostichead/tree иодинprotectedrun, independent actualtimeline
acceptance; only then justifiedQAchange/fullmandatoryCI beforemerge.


### Запущена одна отдельная диагностика LAB transport

2026-10-05, 01:16 UTC. Независимый source review: 0 blockers на чистом
`edca9b9fa16a11bb24317dead67362515b9d1e00`, tree
`5e2d67d3c43cbdb2e3f9992b7114f99a97f25cf5`. Отличие от product1be — ровно
три новых файла: два diagnostic scripts и root workflow. Все восемь site
assets совпали с исходным кандидатом по Git blobs, размерам и SHA: 6599049 B.
Source preflight PASS; HTTP/browser на native машине не запускались.

Astra согласовал один evidence-only запуск. Remote RUN branch сначала создан
из опубликованного product1be без диагностического workflow, затем выполнен
один обычный push проверенного edca9b9. Run
https://github.com/emevart/liminis/actions/runs/37250700549 — IN_PROGRESS.
Единственный initial load, pre-send Request guard, original Response body
через CDP и unchanged continuation. Любые ошибки остаются FAIL. Response-stage
pause меняет timing: диагностический PASS не установит причину прежнего
uninstrumented abort и не заменит девять LAB gates/fullCI/public acceptance.

Четыре отдельных независимых source reports сохранены с исходными байтами в
`docs/experiments/handoff/lab-transport-diagnostic-2026-10-05/`.
Issue11 обновлён: https://github.com/emevart/liminis/issues/11#issuecomment-5986459043.
Main5bea и draftPR21 не менялись; public smoke и LAB4a модели NOT_RUN.
Пользователь отсутствует до утра: вопросов ему нет, спорные решения Astra.

Следующий шаг: получить original diagnostic artifact, независимо проверить
Network timeline/body pins/PNG/error arrays и выбрать обоснованную QA правку;
после неё обязательна свежая полная приёмка кандидата перед merge.


### Original transport diagnostic FAIL сохранён и проверен из Git

2026-10-05, 01:26 UTC. Run37250700549/job111577533759 завершились FAILURE.
Artifact11320133411, originalZIP397681 B/SHA256
`b4a3fdb5ca151f839f7f8fc87adefae6cb1aa8fd1bf0c85b9aaff1faa8dd7036`
скачан через штатный GitHub connector; два members report.json/page.png,
CRC/extracted byte equality PASS. Independent actual review подтверждает
DIAGNOSTIC_FAIL и 0 дополнительных integrity/privacy blockers для архива.

Все восемь original HTTP200 Fetch bodies точны; unchanged continuations ACK,
восемь server finish/close-after-finish без server abort. Семь native Network
loadingFinished; results.json после полного dataReceived5294742 B —
loadingFailed, canceled:true, ERR_ABORTED. Chrome75.758739→75.763942=5.203 ms;
Node arrivals2371.133→2502.646=131.513 ms — разные часы и задержка наблюдения,
не единая causal timeline. Ошибка до cleanupPNG по source flow, но Reader
EOF/signal/lifetime/cancellation cause непосредственно не наблюдались.
Observed sandbox argv PASS; initial/final UI ready, fourvisibleplots/24matrix/
seed1sample0/unhandled0. ActualPNG просмотрен независимо, прежние и остальные
LAB gates не переименованы в PASS. actionsPassed:false; postPNG ready wait
не выполнялся, final snapshot прочитан. Guard остался RUNNING.

Архив `codex/lab-transport-diagnostic-evidence-20261005`:
`68419ae6b7a53fc707983491736a663f728f0dfb`, tree
`b98642ef38ef9ce55231f97d76a779798d4f854b`. Closed10files/originalZIP1,
479464 B включая manifest, SHA manifest
`5272cc2c3a857efa272c6dbe2f9345b635ff58096811cd2b83b66ccc3f020c46`.
Отдельная свежая Git object DB без alternates прочитала origin branch и
проверила все size/SHA/closedinventory/CRC. Receipt сохранён в
`docs/experiments/handoff/lab-transport-diagnostic-edca-network-readback.json`.
Bounded privacy1UTF8 member36031 B+1PNG, patterns0; полной secret guarantee нет.

Astra согласовал ещё один отдельный evidence-only CASE: четыре conditional
Debugger logpoints, всегда false, на неизменённом SHA-pinned lab-data.mjs.
Entry читает только параметры без TDZ locals; finally beforecleanup, перед
releaseLock и после awaitreleaseLock читают только primitives. Ожидаются
16records/4datarequests, cap32; exactsourceSHA/resolvedlocations/nopauses.
Нет monkeypatch/nativePromisehandlers/objectretention/forcedGC.
Performance.Timestamp sandwich с document performance.now задаёт интервалы
совмещения; смешение timeOrigin/Network epoch запрещено, пересечение UNKNOWN.
Любые errors остаются FAIL; исчезновение abort при instrumentation не равно
cause proof. Source author owns2scripts; root owns4WF name/branch edits;
independent review перед ONE future push. New CASE ещё NOT_RUN.
Future RUN branch `codex/lab-reader-lifecycle-run-20261005`; старый RUN больше
не используется. Product1be/main5bea, PR21, raw LAB2 и frozen docs неизменны;
models/Cargo/publicsmoke NOT_RUN. Вопросы пользователю не отправляются.

Следующий шаг: exact reader-diagnostic source freeze/review0/ONE protectedrun,
независимый разбор границ EOF/lock и причинных интервалов с Astra; затем только
обоснованная правка и свежие full LAB/CI/public gates перед product merge.


### Второй отдельный CASE Reader lifecycle: source0 и запуск

2026-10-05, 01:40 UTC. Финальный independent source review: qualified0blockers
на clean `0b52aeec6c6dd07bc2a7b73fe506adcbb12cbf7a`, tree
`e4a1fcca97f7d5f4bbef915858dba49b5c3f0233`.
Script285a32de1bb0f232d3c4ce6ba51bd3a520622cc48d7061406f2e87e4dd7d867d,
manifest3402878d7b33e08f92812ca25f33bcc5c8319a412cc67cd0ecb1f8752fcf1847,
rootWFd65f7b28ac44c69e501b2006fed031318fbc32c96ab5475258c2ad5422fdf58e.
Отличие от product1be — только прежние три diagnostic paths; все восемь
site assets6599049 B неизменны. Два новых independent source reports сохранены
в `docs/experiments/handoff/lab-reader-lifecycle-2026-10-05/`.

Четыре conditional false точки: entry131, finally139, pre_release142,
post_release144. Uppercolumn23 для pre_release включает вызов метода до его
исполнения. Node definition-only inspector проверил source feasibility, но
фактические Chrome sourceSHA/possible/resolved positions ещё обязательны.
Ожидаются16primitive records, cap32; TDZ locals в entry не читаются. Native
fetch/Response/Reader/Promise не переопределяются, новые Promise observers и
GC отсутствуют. Clock anchors только same document/timeOrigin; declared1ms
precision allowance, drift/overlap UNKNOWN, Node arrival не browser time.
Derived order annotations идемпотентны; исходные records сохраняются.

Remote RUN branch создан из опубликованного product1be без нового workflow,
один обычный push финального0b52. Фактический official protected run:
https://github.com/emevart/liminis/actions/runs/37252287604 — IN_PROGRESS.
Root не запускал native browser/Cargo/models/deploy. Предыдущий edca FAIL,
архив68419ae и product gates сохраняются. Даже diagnosticPASS не докажет
причину или исправление поведения без instrumentation и не заменит fullCI.

Следующий шаг: original artifact и independent actual source-location/EOF/
lock/timing review, решение Astra по доказательствам. Main5bea/PR21 не менялись,
public smoke/LAB4a NOT_RUN; вопросов отсутствующему пользователю нет.


### Reader EOF observed, native cancellation unresolved; last bounded A/B plan

2026-10-05, 01:51 UTC. Run37252287604/job111582214941 completedFAILURE.
Original artifact11321850249/ZIP399750 B SHA256
`c628ee972f4ea13bc095d5decb9c8c77c542145e693c133ebd83733642b727c0`.
Независимый actual review подтвердил compiled sourceSHA/engineHash b034…,
четыре exactChromepossible/resolved positions131:25,139:24,142:23,144:4,
16primitive records/paused0. Все четыре JSON достигли actual read done===true:
completedtrue/exactlength/primaryfalse, prelockedtrue→postfalse/cleanupfalse.
На наблюдённом positive path adapter cancel не исполнялся. Это не доказательство
прежнего path или внутренней C++/GC причины.

Все восемь body pins/ACK/serverfinish/decodedcounts точны. Native FAIL теперь
у descriptor50708 B; семь остальных loadingFinished. Все три clock relations
к failure148.526763: UNKNOWN_OVERLAPPING_INTERVALS. Initial/finalready4plots/
24matrix/seed1sample0/unhandled0, actualPNG просмотрен; не остальные LABgates.
Nativeerror и actionsPassedfalse сохраняют overallFAIL.

Архив `codex/lab-reader-lifecycle-evidence-20261005`:
`cd7fa2c103d1e273abe79e5efeedaecf468fcaa7`, tree
`fe0dd780d614afe0817ba26c14ed8f8ed6267a31`, closed8files506195 B,
manifestSHA `0a8985a5e4794d48992d29128e72515ad9a50896cf905db18dcd7931127709c3`.
Оригинал не перепакован. Separate network Git object DB без alternates
проверила refs/closedinventory/size/SHA/CRC PASS; receipt
`docs/experiments/handoff/lab-reader-lifecycle-network-readback.json`.
Boundedprivacy50483UTF8bytes/1PNG/patterns0, expanded480156B, не fullguarantee.

Astra выбрал ОДНУ ПОСЛЕДНЮЮ bounded A/B compatibility probe без production
правок: A exact1be manualreader; B только readBytes transport consumer через
native body.pipeTo(new WritableStream({write:boundedaccumulate})). Прежние
concat/SHA/parser/7siteassets/все научные JSON неизменны, B explicitvariantSHA
и точный functiondiff. Incrementalcap beforeaccumulate/backpressure/cleanup
и primaryerror identity обязательны. Без unboundedarrayBuffer, timers,
keepalive, retainedResponses, Debugger или nativePromise observers.

Один protected job, две свежие последовательные guarded contexts. Ошибки
каждого arm sticky; AFAIL не превращается в overallPASS при Bsuccess.
Только AFAIL/BстрогийPASS по восьми bodypins/finished/UI/errors0 даст будущий
scoped compatibility candidate; не C++causeproof и не productacceptance.
BFAIL или обаPASS/inconclusive → прекратить дальнейшие causal runs,
сохранить Chrome149/guard/stream blocker до утреннего разбора, не merge.

Author owns NEW3files check_lab_transport_ab.mjs, lab-transport-ab-assets.json,
fixtures/lab-transport-ab/lab-data-pipe-to.mjs; root ONLY NEW workflow
lab-transport-ab.yml. Future RUNcodex/lab-transport-ab-run-20261005,
10min/contentsread/Ubuntu22/Node24/officialPW1.61.1; same sandbox guards,
fixedcandidate1be/sparseassets, alwaysoriginalartifact. Source0/finalfreeze
обязательны до ONE launch. A/B ещё NOT_RUN; новые модели/Cargo/public/deploy
не запускались. Main5bea/product1be/PR21/raw/frozen docs сохраняются.

Следующий шаг: A/B focusedsourcechecks/independentreview0/exactfreeze,
единственнаяcomparativeprobe и independentactualreview. При candidateB
обязательны productionfocusedtests/reviews/fresh9LAB/fullCI/public доmerge;
при inconclusive — durableblockercheckpoint и остановка causal runs поAstra.
Вопросы отсутствующему пользователю не отправляются.


### Последняя A/B проверка: кандидат совместимости принят в работу

2026-10-05, 02:20 UTC. Единственный protected run37254028918,
job111587245283, artifact11322181713 завершён. Diagnostic HEAD
`3b174d480ac1be94bea543d08322f075a229ba44`, tree
`25d34dcaf7ef6962820b4e1ed26317f3e78739dd`. Source review и independent
actual integrity/scope/privacy review: 0 дополнительных blockers.
Итог **AB_PROBE_FAIL** сохраняется; product/LAB/fullCI не приняты.

A (исходный1be) получил comparisons.json native canceled ERR_ABORTED
во время main; семь запросов finished. Все восемь original body pins,
decoded counts, continuation ACK и server finish точны. B меняет только
readBytes на bounded native Response.body.pipeTo + WritableStream:
восемь native loadingFinished, все pins точны, четыре ready graphs,
24 matrix rows/seed1/tick0, initial/final unhandled0; main settled,
page closed, cleanup/pending paused0. Оба1440×2443 PNG просмотрены
независимо, byte-exact PNG/RGBA. Это отдельный transport arm PASS,
не девять LAB gates и не внутренняя причина/повторяемость Chromium.

Original ZIP805885 B SHA256
`f8905342189586970b4bbc5b6164b2c48cbcb34a9f013eb6bbe2637e92ad69d0`
сохранён без перепаковки в `codex/lab-transport-ab-evidence-20261005`:
commit `d9a78d218e06b65fa93de540bbf69f412b04fc9b`, tree
`42e33b9a3ae72b2947ea924526198d85f9f4b91a`. Closed8files939228 B;
manifest SHA256 `7e9f4ae35836743b7824be9691125d3013d52a397cd8574a85cb54631ad6df8b`.
Отдельная network Git object DB без alternates повторно проверила
все bytes/size/SHA/closed inventory/ZIP CRC: PASS. Receipt сохранён
в `docs/experiments/handoff/lab-transport-ab-network-readback.json`.
Bounded privacy:3UTF8 JSON147166 B, ZIP inventory, оба PNG, patterns0;
не полная secret guarantee. Original5members expanded1006512 B/nested0.

Astra явно одобрил EXACT B только как UA-compatibility candidate.
Новый автор работает в отдельном fresh1be worktree, владеет только
site/lab-data.mjs и lab-data.test.mjs. Function SHA256
`3129290bf9cbe4eccfd8b17bdf1628547d8fde5273a12f3a5e13cbe600f59a92`,
whole module20280 B SHA256
`51dac9a199089718fcd5624f7ca7998867e5f924d22badbe76d8473e2b19b38e`;
prefix/suffix исходного модуля неизменны. Incremental cap до накопления,
await native completion/cancel/unlock, сохранение primary write error;
нет unbounded arrayBuffer fallback, missing stream — явный отказ.
Поведенческие nativeStream tests заменяют manual reader spies.

Дополнительные причинные probes прекращены. Следующий шаг: clean
scoped author commit, independent code и numerical/raw binding, root
integration и один свежий full mandatory CI со всеми9 LAB gates.
Только после independent actual acceptance допускается guarded PR21
merge и отдельная existing deployment/public LAB приёмка. Main5bea,
product1be, PR21 draft и raw24/source21cbe907 пока неизменны.
LAB4a/models/Cargo/public smoke NOT_RUN. Пользователь отсутствует до
утра; решения передаются Astra, вопросы пользователю не отправляются.


### Последняя A/B probe: исходный FAIL сохранён; узкий compatibility candidate

2026-10-05, 02:22 UTC. Protected run37254028918/job111587245283 завершён
**overall AB_PROBE_FAIL** на exact diagnostic
`3b174d480ac1be94bea543d08322f075a229ba44`, tree
`25d34dcaf7ef6962820b4e1ed26317f3e78739dd`.
Independent source review0 и actual integrity/scope/privacy review0 завершены.
Original artifact11322181713/ZIP805885 B SHA256
`f8905342189586970b4bbc5b6164b2c48cbcb34a9f013eb6bbe2637e92ad69d0`.

A byte-exact product1be получил native comparisons1116928 ERR_ABORTED duringmain,
семь прочих запросов finished. Восемь body SHA/length/decodedcounts/serverfinish
точны. A FAIL остаётся FAIL. B меняет только readBytes на standard bounded
WritableStream + response.body.pipeTo: восемь native finished, все original
pins,4readyplots/24matrix/seed1/tick0 до и после PNG, unhandled/allerrors0,
mainSettled/pageClosed true/pendingpaused0. Два свежих браузера последовательно;
stock official Ubuntu22/Node24/PW1.61.1/Chromium149 и sandbox/argvguards сохранены.
Нет Debugger/Promise observers/GC/deliveryretry. Оба1440×2443 PNG реально
просмотрены reviewer; byte-exact PNG/RGBA. Это transport arm PASS, не9LABgates.

Astra явно APPROVE EXACT B как UA-compatibility candidate. Инкрементный cap до
накопления, native awaited completion/cancel/unlock, primary write error при
secondary cancel failure обязательны. Absent stream failclosed; arrayBuffer
fallback, timers/retainedResponses не разрешены. Source module20280 B SHA256
`51dac9a199089718fcd5624f7ca7998867e5f924d22badbe76d8473e2b19b38e`, functionSHA
`3129290bf9cbe4eccfd8b17bdf1628547d8fde5273a12f3a5e13cbe600f59a92`.
Prefix/suffix к original1be byte-exact. Новый scoped author owns ONLY
site/lab-data.mjs и site/lab-data.test.mjs на fresh isolated1be; patch/tests
IN_PROGRESS, publication/integration pending. Формулы/raw source/24runs/core/
CSS/UI/QA остаются прежними. Дополнительных causal probes не будет.

Durable evidence branch `codex/lab-transport-ab-evidence-20261005`:
`d9a78d218e06b65fa93de540bbf69f412b04fc9b`, tree
`42e33b9a3ae72b2947ea924526198d85f9f4b91a`; closed8files939228 B,
manifestSHA `7e9f4ae35836743b7824be9691125d3013d52a397cd8574a85cb54631ad6df8b`.
Original ZIP без перепаковки; safe own source/actual reports и общий JSON.
Отдельная Git object DB без alternates получила normal network ref/blob bytes:
closedinventory/size/SHA/CRC PASS. Receipt
`docs/experiments/handoff/lab-transport-ab-network-readback.json`.
Boundedprivacy3UTF8members147166 B+2PNG/patterns0, expanded1006512 B,
nestedZIP0; не fullsecretguarantee. Old FAIL и все прежние архивы сохраняются.

Main5bea стабилен, product1be/PR21draft пока FAIL; merge/deploy не выполнены.
C++ cause, repeatability/performance/science/fullCI этим CASE не доказаны.
Пользователь до утра отсутствует: вопросов нет, решения переданы Astra.
Следующий шаг: exact B production diff/native behavioral tests, independent
code/data-numerics binding, один новый exact-head mandatory9LAB/fullCI с
original artifacts/actual review. Только после green — guardedmerge,
existingdeployment и отдельная public LAB byte/browser acceptance. LAB4a
модели остаются NOT_RUN; frozen SPEC/NORTH_STAR не меняются.


### Native pipe production source0; один свежий mandatory CI

2026-10-05, 03:26 UTC. Final clean candidate
`c02bc689bf7014bda3bfdf346e3906a9bc536f14`, tree
`416655a9ef6f1ba91c0556eb35fb8f300249bb15` опубликован одним обычным push
в existing PR21 `codex/lab-aggregate-site`; draft сохраняется.
Source author e180e0d77ccd06989667780db9d784abccca8a3f опубликован отдельно
в `codex/lab-stream-pipe-to-20261005`. ONLY2sitefiles +root30lineACCEPTANCE/
51linecheckpoint отличаются от1be. Module20280B/function844B точны provenB,
prefix17530/suffix1906 literal unchanged. Root14nativeNode tests PASS;
independent code review0 и DATA/NUMERICS binding0 подтверждены на exactcombined.
Raw4pins/24runs/4044samples/20censored4extinct454/common400/missing500/genesisnull,
producer21cbe907/world30/chamber1/buildattestationfalse/formulas/SVG/oracles
и core/frozen/REC/3D bytes unchanged. No new science/performance/causeclaim.

Один свежий mandatory fullCI, attempt1/eventpull_request/headexactc02:
https://github.com/emevart/liminis/actions/runs/37259168567 — IN_PROGRESS.
Защищённый recorded job111602519476 включает all9LAB и прежние REC/dense;
LIVEjob111602519414 включает real13GL; Rustjob111602519427 идёт в штатнойCI.
NativeCargo/models/causalrepeat не запускаются. OldFAIL и A/BoverallFAIL
не переносятся вPASS; никакие gate/oracle/sandboxerror allowances не добавлены.

Safe source4reports включены в новый closed evidence ref
`codex/lab-transport-ab-evidence-20261005`,
`d7953ff0c7c9401e139d0c1a1e1a950ded94e52d`, tree
`509f31e84809438ef64f64d4cbefb0586e911c19`:12files971386B,
manifestSHAee1463fc7898187640d84d30520126f27e579c88193ccd3b9507391774bc13c2.
OriginalZIP/ABreport byte-exact retained; source0 не actualbrowserPASS.
Normal-network separate GitDB/noalternates closedsize/SHA/CRC PASS;
receipt `docs/experiments/handoff/lab-native-pipe-source-network-readback.json`.

Publicpreparation c02repin опубликован:
`4d78c8336e25ebb98e4e0a31db52ea2e47dfbd55`, tree
`ed6489d67c9d645fb072b2225705b247c09f9c6f`, branch
`codex/lab-public-smoke-preparation`. ONLYmanifest8pins +exactproductionmodule,
script/WF/HTMLhostingexception unchanged. Sourcepreflight17 PASS, deployedSHA
null и actualHTTP/browser refused; independentpinreview pending. Existing
publicrunbranch не запускается до acceptedmerge/actualdeploymentSHA.

Main5bea остаётся stable, productmerge/deploy NOT_DONE. Следующий шаг:
дождаться exacthead mandatoryCI без rerun, получитьoriginal browserbytes,
independent fullLAB/GL/regression actualreviews +boundedarchive/readback.
ПослеGREEN — guarded PR21merge и separateexistingpublicdelivery/browser.
Пользователь до утра отсутствует, решений от него не запрашиваем; Astra
сохраняет спорные решения. LAB4a models ещё NOT_RUN.


### Mandatory LAB native pipe FAIL сохранён; отдельный 3D QA scope

2026-10-05, 03:42 UTC. Exactc02 fullrun37259168567: recordedjob111602519476
FAIL наLABgate2 resultsERR_ABORTED/Response.bodyNoData; firstsandboxPASS,
remaining7NOT_EXECUTED. В trace failure доPNG/nav/reload/cleanup. ActualmoduleB
20280B/SHA51dac delivered; семь прочихbodypins точны. Results actualbodySHA
недоступен; server/sourcepins не заменяют actualdeliveryproof. FailurePNG
реально просмотрен independentreviewer,4plots24rowsbaseline1tick0; это не
fullLABPASS. BrowserUnhandled UNOBSERVED, serialized pendingSet{} и
page.closedfalse не finaldrainproof. Recordedpage/console/Node/cleanuparrays0
только их scope. Причина Chromium/repeatability/performance неизвестны.

Independentactual review подтвердил1mandatory acceptance blocker,
additionalarchiveintegrity/privacy0. Closedouter3/nested13CRC0,
expanded2902194B; boundedprivacy10UTF8members2093409B/explicitprivatepatterns0,
11broad-IDmatches классифицированы как scientifici128fields, не privatechat
refs. No fullsecretguarantee. Safeownreview reports иoriginal674755-byteZIP
безперепаковки опубликованы: branchcodex/lab-native-pipe-failure-evidence-20261005,
archivef76cb514eb4e25463da5e1422a0d2d11766e3a4d/tree
dce2fd7c8f10abfa12932c567d314a3021053ea4,closed6files733385B,
manifestSHAf7296c37bb74e10206332c429df911f9305c9ca4cd28d85b9447b77311671c2d.
Separate normal-network GitDB/noalternates closedsize/SHA/CRC PASS;
receipt `docs/experiments/handoff/lab-native-pipe-failure-network-readback.json`.

Astra HOLD PR21merge/deploy/publicLAB/LAB4models и дальнейшиеLABcausalprobes/
retries/productchanges до новогоразбора. Draftc02/WIP/code/tests/source0/
raw24/evidence сохранены; rollback/reset не выполняются. Publicprep4d78
независимоqualifiedpin0/17SOURCEchecks, deployedSHA=null; safeownreport
`docs/experiments/handoff/pre-public-c02-pin-review.json`. ActualpublicLAB
HTTP/browser NOT_RUN. Main5bea здесь unchanged; новыйPASS не объявлен.

После archive/readback Astra отдельно APPROVE QA-only paused3D наfreshmain.
Root clean source `83fa1e52e478e2a33602a24bf83b5456e55e70a3`, tree
`47ce45646315b09493d58d87ca8fce4dc867aa8d`, branchcodex/paused-3d-pixel-qa.
Exact3d48f47перенесён ONLY2QAscripts; root добавил PNGtest кoldCI Nodecommand
и2operationaldocs. Wholeproduction3D/site/core/config/raw/frozen/DECISIONS
objects main5bea-identical; LABworkflow/assets/nav/loader/ADR112 не включены.
Node4meaningful PNGcontrols PASS; source/code/data reviewPENDING.
HistoricalmainFAIL не переименован, causeunknown; old79/c02PASS notcarried.

Следующий шаг: source0 на83fa и завершениеужеrunningc02Rustjob перед ONEnew
mandatoryfullCI/PR. Cargo сериализуются; nativeCargo/models не запускаются.
Fresh13GL/allregressions/actualoriginals/archive/readback затемguardedmerge
QA-onlyPR/existingmainfollowup/publicscope. LAB21/models остаютсяHOLD.
РешенияAstra, пользовательдоутраотсутствует; auto-wakeup не обещается.


### PR22: exact83fa full CI GREEN; actual originals получены

2026-10-05, 04:13 UTC. Отдельный QA-only PR22 открыт draft на base5bea:
https://github.com/emevart/liminis/pull/22 . Head
83fa1e52e478e2a33602a24bf83b5456e55e70a3/tree
47ce45646315b09493d58d87ca8fce4dc867aa8d, production/site/core/raw/frozen
objects unchanged. Independent exact source/code и DATA/GEOMETRY0 завершены.
Safe4reports уже опубликованы source-only archive749b81e8af2d7b4a4453531268a8f403ad0e1e56,
treef7f0591bbabb7047d7b19dc2306afb30a029343a,
codex/paused-3d-qa-evidence-20261005; closed6files31280B,
manifest6fe2ab55dc8f82fd6aa635739a1271fd2fd8b70bada01421103a5c1b129f68b5,
separate GitDB/noalternates normal-network closedsize/SHA PASS.

Fresh mandatoryrun37260833244 attempt1/eventpull_request completedSUCCESS;
all10GHAjobs и Workers Builds:liminis check SUCCESS на exact83fa (11/11).
Rust/build/lint/tests SUCCESS; recordedjob111607516475 и LIVEjob111607516729
SUCCESS со всеми прежними защищёнными browser substeps. Старые FAIL не
переименовываются; source0/jobGREEN не заменяют independent actual acceptance.

Все6 новых original ZIP получены.3Dartifact11324254318 exact48498615B,
SHA413f5c6ddfb79df66135f4701fbfc4fbeca11a9aa1b65270bc9870351399f671:
approved read-actions-only fixed helper efc54aaaa753e79a91427f58d4eb316eff7aabd7,
run37262081937 SUCCESS, три rawparts16777216/16777216/14944183 получены
через existing app/native reads. ContainerSHA/CRC/receipts/offsets/rawSHA и
исходный size/SHA/CRC22members совпали; ZIP восстановлен без перепаковки.
Helper не запускает checkout/build/browser/models/deploy, HTTP auth только
fixedAPI и newNOauth officialstorage request.150s между reads/5min hardjobcap;
transport не заявляет product acceptance. Native sourcehelper review0.

Independent actual REC/dense qualifiedPASS0: legacy22+17HTTP и dense14,
все13PNG просмотрены; source/body/trace/SHA/CRC и boundedprivacy проверены.
Dense9ERR_ABORTED:5verified fullbodyEOF/count доdefaultabort +4explicitcancel,
PW/CDP/nativebijections/orphan0, не generic error allowance. Legacy отдельная
browserUnhandled array не измерена. Native1x1523.3ms/advanced1.523s;
loopback100k/1M finalgzip bodies сохранены и pinned. Full2.42GB publicHTTP/
hardwareFPS не измерены. Два originals и safe own reports опубликованы
codex/paused-3d-83-rec-evidence-20261005,
6c646b0ca9203ed8047c62f8f504effbfe7c24d3/tree32b0253600a06c16a71560e614513549aec0b335,
closed6files29172633B/manifest711933c9bbf95dcceecd63657e1525115fdb641c405aaefe2b8bbb2e5cc72c2c;
network byte readback IN_PROGRESS. Actual LIVE/physical/projection и две
независимые actual3D reviews IN_PROGRESS. Merge/product acceptance PENDING.

Следующий шаг: завершить actualreviews, bounded originals+reports archives
и normalnetwork closed readbacks; затем guardedmerge PR22 exact83fa,
fresh main CI/actual/public scope. LABPR21c02/publicLAB/LAB4models и новые
LABcausalprobes/retries остаются Astra HOLD. Пользователя до утра не спрашиваем.
