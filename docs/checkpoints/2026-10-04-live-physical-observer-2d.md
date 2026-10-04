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

Base физического runtime: LIVE2 a72f3f05a1861b4be026f1a24068af70e8a09df1,
acceptedmainLAB2 de2b52cd08dc3772800725a5eca64619f6bb33c6. LIVE2 ещё pending CI.
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
gitdiffcheckPASS. Actualbrowser/codefinaldelta/fullCI ещё pending.

## Следующий шаг

Дописать/независимо проверить actualprotectedbrowserQA, интегрировать принятый
freshmainLIVE2, актуализировать existingcaptionassertions/report meaning,
onefinalhead CI/PNG/trace acceptance и guardedmerge. Нельзя считать VMNodefixture
доказательством фактического renderedcanvas или повторять staleheadPASS.
После этого следующийприоритет по прямомууказаниюпользователя — каждыйтик
во всех трёх publicrecordings; сначала storageestimate, immutableoldarchive.
Three.js3D остаётсяследующимотдельнымstage, не выдуманнойготовностьюэтогоPR.
