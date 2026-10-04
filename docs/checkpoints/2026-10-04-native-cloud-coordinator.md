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
