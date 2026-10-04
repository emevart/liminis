# LIVE-3a: physical2D observer

Дата: 2026-10-04. Dependent этап после LIVE2, NOT ACCEPTED до final-head gates.

## Scope и разрешения

Пользователь передал единственному native Cloud coordinator commit/push/PR,
independentreview/numerics и guardedmerge/deploy после green acceptance:
https://github.com/emevart/liminis/issues/11#issuecomment-5981221010 .
Старые product authors STOPPED; новые scopednative authors работают в отдельных
ветках, Cargo сериализуется. SharedADR/CI/navigation пишет только coordinator.
Frozen SPEC/NORTH_STAR, core biology, version/config, LAB24runs/source и прежние
publicJSON/SHA не меняются. Новые infra/secrets/расходы не разрешены.

Base физического runtime: LIVE2 PR17 exact33272bc452aca6a7e8716483da5d66941213cebf,
run37219223184 все11checksSUCCESS, independent code/numerics/artifact0blockers;
guarded mergedmain703085e76760924436396030dfd6f0a404daaadd.
Scopedviewer source d975bf224140f48728a3946e60e71517546d4390 плюс
955bb885d561e5d2535f379a786e542b02fb4434. Rootintegration normalcherry-picks,
никакого force/reset/clean чужой работы. Отдельный браузерный QA автор владеет
только новымscripts/check_physical_projection_browser.mjs; coordinator CI.

## Сделано и проверено

RealXY/XZ/YZ orthographic centers, equalunitscale/letterbox и µm/ruler,
fullprojectiondefault, clippedinclusive center-slice/visibletotal, exactID
search/xyz/tick inspector, coincidentcandidates и keyboardvisibleIDs.
State/readouts/canvas/hitmap/inspector synchronous commit до await history.
Absentunknown/vanishedID сохраняется как not-present snapshot, не смерть/lysis.
Sliceinterval виден вµm с17significantdigits,0thickness exactcenterplane.
Chamber1 остаётся schematic. Нет Brownian/biologyinterpolation, дорисованных
дочерей, physicalcellbodies или изменения modelticks от viewcontrols.

Independent sourcegeometryreview d975:0sourceblockers; factual absentIDfinding
исправлен955. Astra одобрил fullADR109draft+точные исправленияпрозы; rootappend
после108. Не менять старыеADR. NativeNode15PASS на955/rootintegration,
gitdiffcheckPASS; rootintegration Node59PASS. Actual final source/QA/CI review:
0 blockers; независимая проверка keyboard дополнения и final cleanup gates0.
Новый protected QA проверяет real SI coordinates/observed canvas paint/pixels,
pointer и реальные focus/arrow events с независимо отсортированными exact IDs;
unknown/outside/empty slice, delayed original history response/atomic snapshot,
реальные paused Steps до division и coincident daughters, семь screenshot.
Existing official pinned Chromium sandbox/CDP argv guard сохраняется; третий
вызов последовательный в существующем job, отдельный evidence artifact.
Actual browser/full CI на final HEAD пока pending; Node не browser PASS.

Первый actual protected run37220117918 на581acd5 — FAIL: первые8 checks
sourcecube/atomic state/planes/keyboard/boundaries/empty-slice PASS, но mobile390
sticky readout перекрыл ID input и physical ruler. Ошибка не ослаблялась в QA.
Astra одобрил physical-only ≤800px обычный поток stage → static readout →
inspector; отдельная grid row для offline notice, hidden не занимает места.
Scoped source fix3ebb9f/rootd7df82e добавляет только8CSSlines; chamber1 и desktop
не затронуты. Author Node15PASS; narrow independent review и новый exact CI
обязательны, первый FAIL сохранён как диагностическое evidence.

## Следующий шаг

Fresh main LIVE2 интегрирован без force/reset. Следующий шаг — onefinalhead
CI/PNG/trace acceptance и guardedmerge. Нельзя считать VMNodefixture
доказательством фактического renderedcanvas или повторять staleheadPASS.
После этого следующийприоритет по прямомууказаниюпользователя — каждыйтик
во всех трёх publicrecordings; сначала storageestimate, immutableoldarchive.
Three.js3D остаётсяследующимотдельнымstage, не выдуманнойготовностьюэтогоPR.
