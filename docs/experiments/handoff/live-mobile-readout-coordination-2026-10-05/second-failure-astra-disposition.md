# LIVE mobile readout: second failure adjudication

**HOLD_NO_SUPPORTED_REPAIR.** No further product or QA authoring, third full CI or diagnostic runtime is authorized by this decision. Preserve the failed candidate and complete its terminal CI receipt, independent artifact review and durable archive/readback. Accepted main `813be1efad29569a3011f00697bd5d41b7397dc6` remains the working baseline.

Candidate `000e2eb6b6d42b81f4cee19eaa23e3f09c917ee6`, tree `a8cdcc995fe3f6c6ec130ff5400b81d733c55286`; run `37284723150`, attempt 1. The local original ZIP independently matches SHA-256 `50020b72346f1902848a1a5b1f65dd84320c0da17dfd0598479df44e8ee08f79`.

This read-only review inspected the report, trace action records, current pinned QA/product source and failure PNG. It does not replace the separately assigned complete source/body/trace/PNG/privacy review.

## Established facts

- Twelve legacy checks completed before responsive testing. The first responsive case was desktop 1440; the already confirmed paused state was tick 1044, selected cell ID 116.
- The persistent-wrapper upper/lower inspector checks completed, including their exact current-field observations and screenshots. Environment access also completed. This establishes that the preceding repair reached those checks in this run; it is not acceptance of the unexecuted mobile cases.
- The next target was the persistent section containing `#events`. The failure occurred at `visibleTarget`'s strict `clipped === false` assertion. The preceding visible, enabled and native center-hit assertions completed. The geometry check returned `clipped=true` for some clipping ancestor/axis according to the helper, but the exact bounds and ancestor are unavailable.
- The helper throws before assigning its returned evidence to `responsive.cases[0].targets.events`. Thus the report has no Events measurement. Trace `call@349` records resolution of the Events section to an ElementHandle; the corresponding element evaluation result is not retained. A search of all trace records found no retained `clipped` field/result.
- The failure PNG shows the Recent events heading and rows. It does not establish the precise target/ancestor rectangles, whether clipping affected decoration or content, or the amount of clipping. A center hit also cannot establish visibility of the whole target.
- Mobile cases and physical/projection/GL steps did not execute. Final responsive state/network assertions were not reached. The original report's empty page/console/resource-warning arrays do not fill those missing checks.

## Decision

There is no basis to call this a rounding, border, scroll-limit or renderer issue. Nor is there evidence that a product layout change is necessary. Desktop CSS was outside the intended product delta, but unchanged source alone does not establish whether a newly added assertion is correct or exposes an existing limitation.

Do not add tolerance, suppress clipping, add a scroll retry/fallback, change desktop CSS, or switch from the Events section to inner content merely to pass. Changing target granularity could become a defensible acceptance contract, but the current artifact does not establish which boundary failed or whether such a change preserves the intended content-access check.

The missing evidence has a concrete future remedy: record every target's raw observation and complete clipping-ancestor chain in the current report **before** applying the unchanged strict assertions. Record target identity, current ID/tick, native scroll baseline/result and successful or failed observation status. Then retain the original failure rather than catching it as success. This would improve observability, not repair the clipping or predict GREEN. It is a specification for a separately bounded future decision, not authorization to write or run it now.

Another full protected workflow would repeat expensive unrelated checks only to recover that missing measurement. On the present evidence, stop at the preserved failure instead of launching a third such run. No alternative diagnostic workflow or runtime is opened implicitly. A future scope must explicitly choose the minimum protected measurement, its resource bound and stop rule before execution.

The a57 failure and its archive, the 000e failure, previous plans and the earlier inspector-repair disposition remain unchanged. No candidate/mobile/physical/GL PASS is transferred across them. LAB21, public LAB, LAB4/model work and causal LAB probes remain HOLD.

This review made no product/QA/Git edits, external requests, browser/test/model/build/CI runs. Only these two new adjudication sidecars were written.
