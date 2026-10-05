# LIVE mobile readout: proportionate acceptance plan

Decision: **APPROVED_ACCEPTANCE_SCOPE**, conditional on the gates below. This is a source-based review plan, not browser or merge acceptance.

Final candidate: `e2e1aff2aa1632b25c04d2e7efe90cc5e9dc8f35`, tree `8401e6c41903bf63467bdfa8c23b28529327b24f`. Its parent `666f048074cf90454c7caa952e6c5c45a49f93ee` contains the reviewed product/QA change; the final commit changes only two documents to narrow inspector and cadence claims. Base: accepted HUD main `813be1efad29569a3011f00697bd5d41b7397dc6`.

## Change boundary

The base-to-candidate diff contains four paths: the shared `cell-viewer.html`, existing LIVE browser QA, ACCEPTANCE and the stage checkpoint. Product changes are confined to the mobile CSS block. HTML outside `<style>` is byte-identical to the base. The candidate HTML SHA-256 is `cf12bae4142980b513375a3a173c4249e457d58e760a366b90153c676446d148`.

The existing physical mobile grid rules now also apply to legacy chamber1. Static readout ordering replaces its sticky overlay; physical readout `bottom:0` and `z-index:8` declarations remain. Desktop rules, DOM, JavaScript, host/model code, 3D renderer/vendor and projection/GL QA remain unchanged. This is not whole-source identity with the earlier GL acceptance: the shared HTML has changed. The valid bridge combines unchanged-component identity, the explicit CSS delta and fresh regression results for the shared shell.

The added responsive QA checks actual rectangles, complete readout text, clipping, hit targets, scrolling and absent-to-present Find against one already confirmed paused population. Inspector claims cover primary fields, genome/generation and the last lower phenotype field; they do not individually assert every physical xyz or phenotype field. Responsive request timestamps/counts are evidence, while 650/4000 ms are source constants, not a new measured cadence result. The prior actual 30/60 polling guard remains separate and mandatory.

## Candidate before merge

- Complete independent source review and one fresh protected full CI on the exact final candidate. All mandatory browser steps must execute successfully; a workflow label without executed steps is insufficient.
- Independently review the new original LIVE and physical artifacts deeply: their exact source/body pins, actual paused state, five viewport layouts, viewport and full-page PNGs, trace-supported scroll/Find actions, primary inspector assertions, resource/event values, request coverage, absence of model-control POST during responsive actions and preserved final model fields. Preserve all old held-pixel, clock, save, polling, sandbox, error and cleanup checks without broadening their claims.
- Fresh projection and GL gates must actually pass. Their current source already requires the responsive physical geometry and controls, served-source checks, mandatory evidence sections and error/cleanup checks. Retain exact head/tree/run/job/step/artifact receipts and the unchanged-renderer/vendor/host/QA plus changed-CSS bridge. Review available original report summaries for the relevant responsive results. Transporting and independently re-reading the entire large GL ZIP is not required without a new concern. The historical e2 GL independent artifact review remains attributed to e2.
- Close the affected LIVE/physical original-artifact archives and verify readback before guarded merge. Preserve exact identities for the other fresh regression artifacts; do not describe them as newly independently reviewed full artifacts.

## Main follow-up

The guarded merge must reproduce the accepted candidate tree. Require fresh MAIN full CI with all applicable checks and six browser steps successful; only the two established PR-only guards may skip. Verify actual job/step completion and runtime/source bindings.

For fresh MAIN LIVE/physical evidence, independently check the new reports and affected artifacts: source/body binding, complete gate inventories, all five-width layout/Find/resource/event assertions, actual state equality, request/error/cleanup results, and new mobile viewport captures showing readout and upper/lower sidebar access. Retain and archive the affected original artifacts. This is an independent review of the changed mobile behavior and its accompanying regressions, not a promise to re-audit every unaffected desktop trace action. The deep candidate review remains candidate-bound and is connected by exact tree identity.

Projection/GL still require fresh MAIN executed regression gates and the same explicit change-impact bridge. A fresh failure, missing responsive result or source/runtime discrepancy requires targeted original-artifact review; prior success cannot fill the gap.

## Fresh MAIN public scope

The recorded public site does not serve this LIVE HTML. Its product assets and public checker remain identical to accepted HUD main. Nevertheless, complete the existing fresh MAIN public smoke after deployment readiness: all 19 HTTPS pins, 14 browser assets per checked viewport and four existing gates.

Independently review that new public report's exact source/metadata/body pins, redirect chain, complete gate inventory, raw HTML and strictly pinned insertion qualification, before-transmission analytics block acknowledgement, request/error/cleanup records, and both new desktop/mobile PNGs. Preserve the original public artifact, trace and source/run receipts with durable readback. Replaying every trace action is unnecessary when those checks agree and the source is unchanged; any inconsistency triggers the relevant deeper trace/body review.

The resulting claim is a fresh public regression for the unchanged recorded site. It is not public LIVE acceptance, full 2.42 GB HTTP verification, a complete accessibility audit or new independent GL acceptance.

## Stop conditions

Hold merge or final MAIN acceptance on any failed or unexecuted mandatory gate, unexpected skip, source/tree/runtime/guard drift, incomplete evidence, unexpected request or body mismatch, control occlusion, altered physical layout, changed paused model fields, or inconsistency between a report and its artifact. Preserve the failure and inspect the affected scope; no repeat-until-green or automatic product/QA expansion follows this plan. A required JavaScript, DOM, host, renderer, model, runtime or guard change needs a new concrete scope decision.

LAB21, public LAB, LAB4/model work and causal LAB retries remain on HOLD. This review executed no browser, tests, model, build, CI or external request and changed no product or QA files.
