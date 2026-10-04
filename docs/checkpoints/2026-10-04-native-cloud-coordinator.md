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
