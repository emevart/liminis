# LIVE mobile readout: a57 failure adjudication

**APPROVE_BOUNDED_QA_REPAIR; PR24 remains HOLD pending a new candidate and its gates.** The original run remains failed. This decision neither accepts the mobile CSS nor reclassifies skipped physical/projection/GL steps as successful.

Failed candidate `a57a6854b633c8e272eb4012ff11a0ec125e6fc2`, tree `2f9f0760ada0bbd1812a232bc6a30883242c76d3`; run `37282535160`, attempt 1. Its product/QA blobs are the same ones reviewed at 666/e2e; later binding changes were documentation-only. The previous acceptance-plan files remain frozen and historical.

## Observed evidence

The local original ZIP's independently recomputed SHA-256 is `c0b5cddbc1d5d520e80f079c93bbac2adffc446bff04d1b1bd1d4dac91352ba0`, matching the supplied original pin. This review read the original report, trace action/network records, pinned product/QA source and failure PNG. It is not the separate complete artifact/privacy review.

The report contains 12 completed legacy checks. The responsive phase stopped in its first, 1440 px case, at the lower-inspector call to `visibleTarget`. The error is `locator.scrollIntoViewIfNeeded: Element is not attached to the DOM`, while waiting for stability. The absent-ID Find and subsequent real Find already returned selected ID 240 at paused tick 2138, with primary inspector values matching the retained API cell. Upper inspector geometry passed. Mobile cases and later physical/projection/GL steps did not execute.

Trace `call@317` resolves `#cell-detail .object-grid dd >> nth=-1` to a visible `<dd>0.05</dd>` at 8210.152 ms and returns an ElementHandle at 8219.276 ms. The same trace records a `/api/state` GET beginning at 8218.488 ms with HTTP 200 and 4.128 ms duration. These are retained trace observations; the failing scroll is reported by the top-level error. The trace does not retain an initiating JavaScript mutation stack for that detachment.

The product source supplies a concrete lifecycle explanation for why this is an unsafe asynchronous QA target: `poll` calls `commitState`, which calls `renderInspector`; the latter invokes `#cell-detail.replaceChildren()` and constructs new descendants. The `#cell-detail` element itself persists. The current helper resolves a descendant for asynchronous native scrolling and then separately evaluates that descendant. Upper and lower inspector targets both share this lifecycle risk. The evidence supports removing that dependency from QA; it does not establish the particular poll as the sole proven cause or require a product polling change.

## Authorized repair

Change only `scripts/check_live_cells_browser.mjs` under its assigned owner. For the upper and lower inspector cases:

1. Record the existing scroll baseline and call native Playwright `scrollIntoViewIfNeeded()` once on persistent `#cell-detail`.
2. After the await, run one synchronous, read-only evaluation on that persistent wrapper. Re-query the current target inside it and capture the target's rectangle, style/visibility, ancestor clipping, native `elementFromPoint`, text/group and current selected ID/shown tick in that same evaluation. Do not retain a descendant or ElementHandle across an await.
3. Apply the existing strict field-specific visibility, complete viewport bounds, clipping, hit and API-derived value assertions. Wrapper visibility alone is insufficient. Keep upper/last-phenotype assertions, absent-to-present Find, actual scroll evidence, viewport/full-page captures and final paused-state checks. Any subsequent read should re-query from the stable wrapper rather than reuse a descendant handle.

If this one native wrapper scroll does not make the current field fully visible, fail. No JavaScript scroll fallback, retries, sleeps, retained DOM nodes, polling suspension, forced repaint, hidden/overlaid content, fabricated state, production changes or timeout increase is authorized. Existing model-control prohibition, request scope, sandbox, held-pixel, clock, polling, error and cleanup guards stay unchanged. IDs/ticks/values must continue to come from the current run's saved API snapshot, not the failure's ID 240/tick 2138.

## Subsequent gates

Finish the independent original-failure/source/privacy review and durable failure archive/readback. Review the new narrow QA diff independently, freeze a new exact candidate, then run one fresh full protected CI for that changed candidate under the existing budget. This does not authorize retrying the unchanged a57 candidate or repeating until green. A new detachment, visibility failure, altered guard or need for a broader repair returns to HOLD with the new evidence preserved.

The previously approved candidate/main/public acceptance plan otherwise applies, rebound explicitly to the new candidate after source review. LAB21, public LAB, LAB4/model work and causal LAB retries remain HOLD. This review performed no browser, test, model, build, CI, HTTP or Git mutation; it wrote only the two new adjudication sidecars.
