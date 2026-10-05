# Independent Astra review of the LAB source audit

Reviewed at 2026-10-05 05:32:49 UTC. **AGREE — NO_SUPPORTED_FIX; LAB remains HOLD.** There are no blocking semantic corrections to the audit. This verdict means that the inspected evidence does not support a particular repair. It does not establish that no repair exists.

This review used the eight already delivered local source files, the four audit reports and the saved c02 browser-version fields. It made no external requests and ran no browser, model, build, CI job or reproducer. Only this review and its JSON companion were written.

## Source and scope verification

All eight files independently matched the receipts in byte length, SHA-256 and locally computed Git blob SHA-1: **233,506 bytes total**. The three report digests in `audit-summary.json` also matched. The audit records a conservative 05:20:00–05:28:14 UTC source-reading window, within the declared eight-file, 10 MiB, 60-minute limits. This review verifies the retained receipts and files; it does not independently reconstruct tool-call timing or upstream HTTP history.

The retained official metadata maps Playwright v1.61.1 to `39e3553a4f283a41134d75d7e404484bd9e6865a` and Chromium 149.0.7827.55 to `3188f8a607ae7e067593be8aab7f02d2451fec07`. Local `browsers.json` specifies chromium/headless-shell revision 1228 and version 149.0.7827.55. The saved c02 report independently records Playwright 1.61.1, browser 149.0.7827.55 and the headless-shell-1228 executable path. This is release/version correspondence, not attestation of the running binary's source Git revision. File hashes bind the delivered source text; they are not independent upstream blob attestations.

The recorded correction from a nonexistent platform `fetch_data_loader.cc` path to the same target under core/fetch is consistent with the scope. A wrong-path 404 is not an access denial. This review made no replacement GET.

## Semantic findings

1. **Playwright's finished promise is not proof of successful native transport.** In `crNetworkManager.ts`, both loading-finished and loading-failed paths call `response._requestFinished` when a response exists. `network.ts` resolves the finished promise there. `internalBody` subsequently invokes the body callback, which first calls `Network.getResponseBody`. Thus a failed request can reach that callback. This explains why the two failure observations can coexist; it does not identify the cause of either the native abort or missing CDP body data.
2. **The two body-access methods are distinct.** `FetchHandler::GetResponseBody` delegates to the interception job, whose eligibility check requires a response-stage paused request. That is not the Playwright callback's `Network.getResponseBody`. The saved response-stage diagnostic cannot substitute for the missing actual-results body pin in the c02 mandatory gate.
3. **Blink consumer cancellation is not uniquely an application abort.** `BodyStreamBuffer::Close`, as well as explicit cancel, context destruction and error paths, can call `CancelConsumer`. The data-pipe loader also calls its consumer's cancel operation on normal `kDone`. These source branches prohibit an inference from the method name alone to a JavaScript cancellation, GC event or observed network failure.
4. **Reader EOF and network completion are not interchangeable or totally ordered by these files.** `DataPipeBytesConsumer::EndRead` can signal completion when a known total size equals the bytes read; `SignalSize` can also complete an already fully read pipe. Other paths wait for explicit completion/error and pipe closure. The actual active consumer chain and size notifier for the recorded failure were not observed. The audit correctly avoids asserting either that EOF always waits for `Network.loadingFinished`, or that the known-size branch caused this failure.
5. **The interceptor source does not close the causal gap.** `SendResponse` delivers its response body and calls `CompleteRequest`, which sends `OnComplete` before `Shutdown`. Separate loader/client disconnect handlers also call `Shutdown`; `CancelRequest` resets handles. The saved evidence does not select one of these native paths or bind it to the failing request's initiating stack. Source order within one function is not a cross-process delivery-order measurement.

The cited client disconnect handler is specifically at lines 1651–1652, adjacent to the matrix's listed StartRequest/CancelRequest ranges. This is a citation precision note, not a semantic blocker.

## Decision and limits

The c02 mandatory LAB failure remains authoritative for that candidate: gate 2 failed, seven later gates did not execute, and the failing results response lacks the required actual body pin. A complete rendered page, native stream EOF in another diagnostic and the earlier single successful B arm do not repair that evidence. The A/B candidate was worth evaluating under its prior conditional scope; its later mandatory failure prevents accepting it as a demonstrated compatibility fix.

No additional product loader change, runtime update, cache increase, timing/retention workaround, native-error exemption or repeated causal run is supported by this audit. Keep PR21, public LAB acceptance and dependent model work on HOLD. Keep each original failure and the unsuccessful candidate available for review. The draft reproducer is a specification only, not launch authorization; its unresolved protection-equivalent plain-network control remains unresolved.

Further native ownership, cache or completion paths are outside these eight files. Neither GC, response lifetime, a size threshold, a Chromium defect, an interception defect nor a specific race has been established. Distinct accepted-main UI work can proceed under its own bounded scope and unchanged acceptance gates; it does not resolve or resume LAB.
