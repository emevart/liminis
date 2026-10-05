# Полная LAB приёмка native pipe: исходный FAIL

Original GitHub Actions ZIP сохранён без перепаковки. Run37259168567,
recorded job111602519476, artifact11323803358; exact candidate
`c02bc689bf7014bda3bfdf346e3906a9bc536f14`, tree
`416655a9ef6f1ba91c0556eb35fb8f300249bb15`.
LAB статус **FAIL**, продукт не принят. Только sandboxgate1 PASS;
gate2 FAILED, оставшиеся7LABgates NOT_EXECUTED.

Observed module20280B/SHA51dac9a199089718fcd5624f7ca7998867e5f924d22badbe76d8473e2b19b38e
точно совпадает с B. results.json получил reported Chromium ERR_ABORTED
и Playwright response.body Network.getResponseBody NoData. Trace показывает
failure до PNG и до любой reload/history/navigation. Для results фактические
response body size/SHA отсутствуют: server/sourcepins не заменяют byteproof.
Семь других HTTP bodypins есть. Actual failurePNG просмотрен reviewer;
1440×2443 с четырьмя графиками/24matrixbaseline1sample0, byte-exact прежним
PNG. Видимый UI не отменяет request failure и не принимает остальные gates.

Registered page/console/Node/cleanup arrays пусты в их scope. Browser-side
__labQaUnhandled не прочитан после earlyexit; его значение UNKNOWN.
Serialized pending Set={} и page.closed:false не являются итоговым drain
или closePage proof. Trace publication PASS — только сохранность trace,
не overall browser PASS. Причина Chromium, repeatability/performance не
установлены. Scoped Node14/source0/data-binding0 и earlierA/B BtransportPASS
не перенесены на полную приёмку, oldFAIL/ABoverallFAIL сохраняются.

Astra HOLD PR21merge/deploy/publicLAB/LAB4models и дальнейшие LABcausalprobes/
retries/productchanges. Candidate остаётся сохранённым draft; main5bea здесь
не изменялся. Отдельный approved QA-only paused3D этап требует freshmain/
independentreview/fullCI/actualgates и не ослабляет этот LAB blocker.

Original674755B SHA256
`b89e457fa75407d86cf5e96788615c7090e74334083544b0bb24ce2cf3364b26`.
Closed ZIP3members: report17587B, failurePNG429673B, traceZIP280337B.
Safe own independent actual/privacy report включён явно, без rawjoblogs.
Boundedprivacy относится к originalreport/nestedtrace text и просмотренному
PNG; broad-ID совпадения классифицированы как decimal i128 scientific
counts в pinned originalcomparisons, не privatecontext. Нет полного secret
guarantee. Scientific raw/source21cbe907/world30/chamber1/24runs не менялись;
новые LABmodel runs не выполнялись. Manifest задаёт closed inventory/size/
SHA/groupcap64MiB; networkreadback публикуется только после реальной проверки.

Independent actual review:1mandatory acceptance blocker; additionalarchive integrity/privacy0. Outer3/nested13 CRC0; combinedexpanded2902194B. Boundedprivacy10UTF8members2093409B, explicitcredential/privatepatterns0;11broad-ID matches independentlyclassified scientifici128falsepositives. Это не fullsecretguarantee.
