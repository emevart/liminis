// Read-only production smoke on the existing public site. Run in protected CI
// with its official playwright@1.61.1 install, never with a sandbox opt-out.
// This is separate from site/playback.browser.mjs and its local HTTP gate.
import assert from "node:assert/strict";
import { createHash, webcrypto } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFile, writeFile, mkdir, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { selectExperiment, validateCatalog } from "../site/catalog.mjs";
import { validateRecording } from "../site/recording.mjs";
import { createDenseRecordingLoader } from "../site/dense-recording.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const siteRoot = resolve(repoRoot, "site");
const evidenceDir = resolve(repoRoot, "target/qa/public-playback");
const publicBase = new URL("https://liminis.dev/");
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
// Reviewed production-only Cloudflare insertion, including its preceding LF.
// Its digest is pinned before execution, never learned from a new response.
const knownEdgeInsertion = {
  sourceBytes: 6894, sourceSha256: "6a0428e4ce738ada8cfd7e3f3586d1978dbe3f4c70fe2cd9d9dd8b8a002d640d",
  offset: 6878, bytes: 367, sha256: "bbba70d1fbb140fe2cff2d40386e726bfe911227760ad6e69e29644e42b6f40a",
  scriptUrl: "https://static.cloudflareinsights.com/beacon.min.js/v31edd6df95cf4e85bb4c19e7a9bdbcba1788362987495",
};
const controls = ["play", "previous", "next", "scrub", "speed", "speed-preset", "draw-fps"];
const sourceNames = ["observe.html", "observer.js", "observer.css", "playback.mjs", "recording-loader.mjs", "recording.mjs", "dense-recording.mjs", "catalog.mjs", "data/catalog.json"];
const timeouts = { deployment: 180_000, retry: 5000, fetch: 15_000, operation: 15_000, viewport: 60_000, launch: 20_000, cdp: 5000, traceStop: 15_000, cleanup: 5000 };
const evidence = {
  scope: "Read-only deployed recording smoke; does not replace local recorded-browser acceptance.",
  publicBase: publicBase.href, status: "NOT_RUN", startedAt: new Date().toISOString(),
  nodeVersion: process.version,
  runner: { os: process.env.RUNNER_OS || process.platform, arch: process.env.RUNNER_ARCH || process.arch, imageOS: process.env.ImageOS || null, imageVersion: process.env.ImageVersion || null },
  workflowRun: process.env.GITHUB_RUN_ID ? `${process.env.GITHUB_SERVER_URL || "https://github.com"}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}` : null,
  workflowAttempt: process.env.GITHUB_RUN_ATTEMPT || null,
  timeouts, checks: [], screenshots: [], publicAssetAttempts: [], browserObservations: [], blockedRequests: [], analyticsRequests: [],
  hostingHtmlQualification: { knownEdgeInsertion, analytics_execution: "not_tested", policy: "Actual browser HTML is retained; only this exact pinned insertion may differ. Its analytics Script GET is intentionally blocked before transmission." },
  publicAssets: { status: "NOT_RUN", measurement: "Node HTTPS GET response bytes, size and SHA-256 against the clean local checkout" },
  launchArgumentCheck: { status: "NOT_RUN", method: "CDP Browser.getBrowserCommandLine" },
  trace: { status: "NOT_RUN", filename: "trace.zip", coverage: "Actual public browser actions, responses, snapshots and screenshots; Node fetch observations are in evidence.json." },
  httpCoverage: { publicAssetBytes: "Node HTTPS responses and actual Playwright response bodies are verified separately.", playbackRequests: "Playwright Page.request events only; no public-server request counter is available.", fullLocalHttpAcceptance: "NOT_ASSERTED: remains the separate site/playback.browser.mjs gate." },
  limits: ["Chromium/Linux and desktop plus 390px mobile viewport only.", "Input assignment plus dispatched change verifies adapter semantics; native typing/range/touch and full accessibility are not asserted.", "Native background scheduling, achieved FPS, native download transport and kernel isolation are not asserted.", "The 30/60 controls are draw targets; held sample pixels and physical playhead are checked, not achieved frame rate."],
};
let browser, context, tracing = false;
const viewportFinalizers = [];

async function bounded(operation, timeout, label) {
  let timer;
  try { return await Promise.race([operation, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${label} timed out`)), timeout); })]); }
  finally { clearTimeout(timer); }
}

async function check(name, action) {
  const started = Date.now();
  try { await action(); evidence.checks.push({ name, status: "PASS", elapsedMs: Date.now() - started }); return true; }
  catch (error) { evidence.checks.push({ name, status: "FAIL", elapsedMs: Date.now() - started, error: error.stack || String(error) }); return false; }
}

// Bound both time and decoded response size. The target URL cannot be supplied
// externally. Bounded canonical HTML redirects remain same-origin HTTPS GETs.
async function fetchAsset(asset, timeout) {
  const url = new URL(asset.filename, publicBase);
  const observation = { filename: asset.filename, url: url.href, expectedBytes: asset.bytes, expectedSha256: asset.sha256, status: "FAIL" };
  const controller = new AbortController(), timer = setTimeout(() => controller.abort(), timeout);
  let reader;
  try {
    let current = url, response;
    observation.redirects = [];
    for (let hop = 0; ; hop++) {
      response = await fetch(current, { signal: controller.signal, cache: "no-store", redirect: "manual", headers: { "Cache-Control": "no-cache" } });
      if (![301, 302, 303, 307, 308].includes(response.status)) break;
      const location = response.headers.get("location");
      assert.ok(location && hop < 4, "Canonical redirect must have a bounded target");
      const target = new URL(location, current);
      assert.equal(target.origin, publicBase.origin, "Canonical redirect must remain on liminis.dev");
      assert.equal(target.protocol, "https:");
      observation.redirects.push({ from: current.href, status: response.status, to: target.href });
      await response.body?.cancel();
      current = target;
    }
    observation.finalUrl = current.href;
    Object.assign(observation, { httpStatus: response.status, contentLength: response.headers.get("content-length"), etag: response.headers.get("etag"), lastModified: response.headers.get("last-modified") });
    assert.equal(response.status, 200, `Public asset HTTP status: ${asset.filename}`);
    assert.ok(response.body, `Missing response body: ${asset.filename}`);
    reader = response.body.getReader();
    const hash = createHash("sha256"); let bytes = 0;
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      bytes += chunk.value.byteLength;
      observation.bytes = bytes;
      assert.ok(bytes <= asset.bytes, `Public asset exceeds expected size: ${asset.filename}`);
      hash.update(chunk.value);
    }
    Object.assign(observation, { bytes, sha256: hash.digest("hex") });
    assert.equal(bytes, asset.bytes, `Public asset size differs: ${asset.filename}`);
    assert.equal(observation.sha256, asset.sha256, `Public asset SHA-256 differs: ${asset.filename}`);
    observation.status = "PASS";
  } catch (error) { observation.error = error.message || String(error); observation.errorCause = error.cause?.message || null; }
  finally { controller.abort(); clearTimeout(timer); if (reader) await bounded(reader.cancel(), timeouts.cleanup, "Public response cleanup").catch(() => {}); }
  return observation;
}

async function waitForPublicAssets(assets) {
  const deadline = Date.now() + timeouts.deployment;
  const pending = new Map(assets.map((asset) => [asset.filename, asset]));
  evidence.publicAssets.verifiedResponses = [];
  do {
    const startedAt = new Date().toISOString();
    // Retry only mismatches, avoiding repeated large immutable recording GETs.
    // The subsequent browser checks independently verify all received bytes.
    const observations = await Promise.all([...pending.values()].map((asset) => fetchAsset(asset, Math.min(timeouts.fetch, Math.max(1, deadline - Date.now())))));
    evidence.publicAssetAttempts.push({ startedAt, finishedAt: new Date().toISOString(), observations });
    for (const item of observations) if (item.status === "PASS") { pending.delete(item.filename); evidence.publicAssets.verifiedResponses.push(item); }
    if (pending.size === 0) { evidence.publicAssets.status = "PASS"; return; }
    const remaining = deadline - Date.now();
    if (remaining <= 0) break;
    await new Promise((resume) => setTimeout(resume, Math.min(timeouts.retry, remaining)));
  } while (Date.now() < deadline);
  evidence.publicAssets.status = "FAIL";
  throw new Error("Public assets did not match this checkout within the bounded deployment window; see publicAssetAttempts.");
}

const shownNumber = async (page, id) => Number((await page.locator(`#${id}`).textContent()).replaceAll(",", ""));

async function canvasState(page) {
  return page.locator("#chamber").evaluate(async (canvas) => {
    const pixels = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    let paintedPixels = 0;
    for (let index = 0; index < pixels.length; index += 4) {
      if (pixels[index + 3] > 0 && (Math.abs(pixels[index] - 8) > 8 || Math.abs(pixels[index + 1] - 13) > 8 || Math.abs(pixels[index + 2] - 11) > 8)) paintedPixels++;
    }
    const sha256 = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", pixels)), (value) => value.toString(16).padStart(2, "0")).join("");
    return { paintedPixels, sha256 };
  });
}

async function layout(page, desktop) {
  const width = await page.evaluate(() => ({ viewport: innerWidth, page: document.documentElement.scrollWidth }));
  assert.ok(width.page <= width.viewport + 1, `Horizontal overflow: ${JSON.stringify(width)}`);
  for (const id of [...controls, "duration", "sample-cadence", "clock-help"]) {
    if (!desktop) await page.locator(`#${id}`).scrollIntoViewIfNeeded();
    const box = await page.locator(`#${id}`).evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom, width: rect.width, height: rect.height, viewportWidth: innerWidth, viewportHeight: innerHeight };
    });
    assert.ok(box.width > 0 && box.height > 0 && box.left >= -1 && box.right <= box.viewportWidth + 1 && box.top >= -1 && box.bottom <= box.viewportHeight + 1, `Clipped ${id}: ${JSON.stringify(box)}`);
  }
  await page.evaluate(() => scrollTo(0, 0));
}

async function viewportSmoke(name, viewport, assets, entry, control) {
  const page = await bounded(context.newPage(), timeouts.operation, "Public page creation");
  page.setDefaultTimeout(timeouts.operation); page.setDefaultNavigationTimeout(timeouts.operation);
  await bounded(page.setViewportSize(viewport), timeouts.operation, "Public viewport setup");
  const observation = { name, viewport, status: "RUNNING", sourceResponses: [], requests: [], errors: [], expectedErrors: [], analyticsRequests: [] };
  evidence.browserObservations.push(observation);
  // Chromium's Playwright route handler skips redirected requests. Intercept
  // every Request-stage hop directly, retaining actual public responses.
  const networkSession = await bounded(context.newCDPSession(page), timeouts.cdp, "Public network guard session");
  observation.requestGuard = { method: "CDP Fetch.requestPaused at Request stage", allowed: 0, blocked: 0, status: "RUNNING" };
  const guardActions = new Set();
  const documentUrl = evidence.publicAssets.verifiedResponses.find((item) => item.filename === "observe.html").finalUrl;
  networkSession.on("Fetch.requestPaused", (event) => {
    const action = (async () => {
      const request = event.request, url = new URL(request.url);
      if (request.method === "GET" && url.origin === publicBase.origin) {
        observation.requestGuard.allowed++;
        await bounded(networkSession.send("Fetch.continueRequest", { requestId: event.requestId }), timeouts.cdp, "Public GET guard acknowledgement");
      } else if (request.method === "GET" && url.href === knownEdgeInsertion.scriptUrl && event.resourceType === "Script") {
        const frameTree = await bounded(networkSession.send("Page.getFrameTree"), timeouts.cdp, "Analytics document attribution");
        assert.equal(event.frameId, frameTree.frameTree.frame.id, "Analytics exception belongs only to the checked main document");
        assert.equal(frameTree.frameTree.frame.url, documentUrl);
        const blocked = { url: url.href, method: "GET", resourceType: event.resourceType, documentUrl, measurement: "CDP Request stage before transmission", status: "PENDING_ACK" };
        evidence.analyticsRequests.push(blocked); observation.analyticsRequests.push(blocked);
        await bounded(networkSession.send("Fetch.failRequest", { requestId: event.requestId, errorReason: "BlockedByClient" }), timeouts.cdp, "Intentional analytics block acknowledgement");
        blocked.status = "BLOCKED_ACKNOWLEDGED";
      } else {
        observation.requestGuard.blocked++;
        evidence.blockedRequests.push({ method: request.method, url: request.url, measurement: "CDP Request-stage before transmission" });
        await bounded(networkSession.send("Fetch.failRequest", { requestId: event.requestId, errorReason: "BlockedByClient" }), timeouts.cdp, "Unexpected request block acknowledgement");
      }
    })().catch(async (error) => {
      observation.errors.push(`Public request guard failed: ${error.message || String(error)}`);
      observation.requestGuard.status = "FAIL";
      await bounded(networkSession.send("Fetch.failRequest", { requestId: event.requestId, errorReason: "BlockedByClient" }), timeouts.cdp, "Failed guard refusal").catch(() => {});
    });
    guardActions.add(action);
    action.finally(() => guardActions.delete(action));
  });
  await bounded(networkSession.send("Network.setCacheDisabled", { cacheDisabled: true }), timeouts.cdp, "Disable public browser cache");
  await bounded(networkSession.send("Fetch.enable", { patterns: [{ urlPattern: "*", requestStage: "Request" }] }), timeouts.cdp, "Enable every-hop public request guard");
  const expected = new Map(assets.map((asset) => [new URL(asset.filename, publicBase).pathname, asset]));
  const attestedRedirects = new Map();
  for (const verified of evidence.publicAssets.verifiedResponses) {
    const asset = assets.find((item) => item.filename === verified.filename);
    expected.set(new URL(verified.finalUrl).pathname, asset);
    for (const redirect of verified.redirects) {
      expected.set(new URL(redirect.from).pathname, asset);
      expected.set(new URL(redirect.to).pathname, asset);
      attestedRedirects.set(redirect.from, redirect);
    }
  }
  const bodyReads = [];
  let settledBodyReads = 0, pageClosed = false, actionsPassed = false;
  const consoleErrors = [], failedRequests = [];
  observation.consoleErrors = consoleErrors; observation.requestFailures = failedRequests;
  page.on("pageerror", (error) => observation.errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() !== "error") return;
    const item = { text: message.text(), location: message.location() };
    consoleErrors.push(item);
    if (!(item.location.url === knownEdgeInsertion.scriptUrl && /^Failed to load resource: net::ERR_BLOCKED_BY_CLIENT(?:\.Inspector)?$/.test(item.text))) observation.errors.push(`Console error: ${item.text}`);
  });
  page.on("requestfailed", (request) => {
    const item = { url: request.url(), method: request.method(), errorText: request.failure()?.errorText };
    failedRequests.push(item);
    if (!(item.url === knownEdgeInsertion.scriptUrl && item.method === "GET" && /^net::ERR_BLOCKED_BY_CLIENT(?:\.Inspector)?$/.test(item.errorText || ""))) observation.errors.push(`Request failed: ${item.url} (${item.errorText})`);
  });
  page.on("request", (request) => observation.requests.push({ method: request.method(), url: request.url() }));
  page.on("response", (response) => {
    const url = new URL(response.url()), asset = url.origin === publicBase.origin ? expected.get(url.pathname) : null;
    if (!asset) return;
    if ([301, 302, 303, 307, 308].includes(response.status())) {
      const result = { url: response.url(), httpStatus: response.status(), status: "FAIL", measurement: "Browser canonical redirect headers; no asset byte claim" };
      (observation.redirectResponses ||= []).push(result);
      try {
        const location = response.headers().location;
        assert.ok(location, "Browser canonical redirect must name a target");
        const target = new URL(location, response.url());
        const attested = attestedRedirects.get(response.url());
        assert.equal(target.origin, publicBase.origin);
        assert.ok(attested, "Browser redirect must match the verified Node chain");
        assert.equal(response.status(), attested.status);
        assert.equal(target.href, attested.to);
        Object.assign(result, { target: target.href, status: "PASS" });
      } catch (error) { result.error = error.message || String(error); }
      return;
    }
    const result = { filename: asset.filename, url: response.url(), httpStatus: response.status(), measurement: "Playwright response.body", expectedBytes: asset.bytes, expectedSha256: asset.sha256, status: "FAIL" };
    observation.sourceResponses.push(result);
    bodyReads.push(bounded(response.body(), timeouts.operation, `Browser response body: ${asset.filename}`).then(async (bytes) => {
      Object.assign(result, { bytes: bytes.byteLength, sha256: digest(bytes) });
      assert.equal(response.status(), 200);
      if (asset.filename === "observe.html") {
        result.rawHtmlMatchesSource = result.bytes === asset.bytes && result.sha256 === asset.sha256;
        result.knownEdgeInjectionVerified = false;
        let normalized = bytes;
        if (!result.rawHtmlMatchesSource) {
          assert.equal(asset.bytes, knownEdgeInsertion.sourceBytes);
          assert.equal(asset.sha256, knownEdgeInsertion.sourceSha256);
          assert.equal(bytes.byteLength, asset.bytes + knownEdgeInsertion.bytes);
          const insertion = bytes.subarray(knownEdgeInsertion.offset, knownEdgeInsertion.offset + knownEdgeInsertion.bytes);
          assert.equal(digest(insertion), knownEdgeInsertion.sha256, "Only the independently reviewed exact Cloudflare insertion is allowed");
          normalized = Buffer.concat([bytes.subarray(0, knownEdgeInsertion.offset), bytes.subarray(knownEdgeInsertion.offset + knownEdgeInsertion.bytes)]);
          result.knownEdgeInjectionVerified = true;
        }
        assert.equal(normalized.byteLength, asset.bytes); assert.equal(digest(normalized), asset.sha256);
        result.afterExactInsertionRemovalMatchesSource = true;
        result.normalizedBytes = normalized.byteLength; result.normalizedSha256 = digest(normalized);
        result.rawHtmlFilename = `${name}-observe-raw.html`;
        await writeFile(resolve(evidenceDir, result.rawHtmlFilename), bytes);
        observation.html = result;
      } else {
        assert.equal(result.bytes, asset.bytes); assert.equal(result.sha256, asset.sha256);
      }
      result.status = "PASS";
    }).catch((error) => { result.error = error.message || String(error); }).finally(() => { settledBodyReads++; }));
  });
  async function finalizeObservation() {
    // Page closure stops new requests. Drain all already delivered guard/body
    // callbacks before validating; repeat after context/browser cleanup below.
    await bounded((async () => {
      while (guardActions.size) await Promise.all([...guardActions]);
      let drained = 0;
      while (drained < bodyReads.length) {
        const batch = bodyReads.slice(drained); drained += batch.length;
        await Promise.all(batch);
      }
    })(), timeouts.operation, "Closed public page evidence drain");
    assert.equal(pageClosed, true, "Public evidence verdict requires page closure");
    assert.equal(guardActions.size, 0);
    assert.equal(settledBodyReads, bodyReads.length);
    assert.ok(actionsPassed, "All viewport controls must have completed");
    for (const asset of assets) assert.ok(observation.sourceResponses.some((item) => item.filename === asset.filename && item.status === "PASS"), `Browser did not receive verified ${asset.filename}`);
    assert.ok(observation.sourceResponses.every((item) => item.status === "PASS"), "Every final critical response must match the checkout");
    assert.ok((observation.redirectResponses || []).every((item) => item.status === "PASS"), "Every final redirect must match the attested canonical chain");
    assert.ok(observation.requestGuard.allowed > 0);
    assert.equal(observation.requestGuard.blocked, 0);
    assert.notEqual(observation.requestGuard.status, "FAIL");
    const expectedAnalyticsCount = observation.html.knownEdgeInjectionVerified ? 1 : 0;
    assert.equal(observation.analyticsRequests.length, expectedAnalyticsCount, "Only one pinned insertion's blocked analytics request is permitted");
    assert.ok(observation.analyticsRequests.every((item) => item.status === "BLOCKED_ACKNOWLEDGED"));
    assert.ok(observation.requests.every((item) => item.method === "GET" && (new URL(item.url).origin === publicBase.origin || (item.url === knownEdgeInsertion.scriptUrl && expectedAnalyticsCount === 1))), "Every final browser request must satisfy public read-only scope");
    observation.expectedErrors = [];
    for (const item of failedRequests) {
      if (expectedAnalyticsCount === 1 && item.url === knownEdgeInsertion.scriptUrl && item.method === "GET" && /^net::ERR_BLOCKED_BY_CLIENT(?:\.Inspector)?$/.test(item.errorText || "")) observation.expectedErrors.push({ kind: "requestfailed", ...item });
      else observation.errors.push(`Request failed: ${item.url} (${item.errorText})`);
    }
    for (const item of consoleErrors) {
      if (expectedAnalyticsCount === 1 && item.location.url === knownEdgeInsertion.scriptUrl && /^Failed to load resource: net::ERR_BLOCKED_BY_CLIENT(?:\.Inspector)?$/.test(item.text)) observation.expectedErrors.push({ kind: "console", ...item });
      else observation.errors.push(`Console error: ${item.text}`);
    }
    observation.playbackPageRequests = observation.requests.slice(observation.playbackRequestBaseline);
    assert.deepEqual(observation.playbackPageRequests, [], "Controls must produce no requests, including late requests through page closure");
    assert.deepEqual(observation.errors, []);
    observation.requestGuard.status = "PASS";
    observation.finalization = { pageClosed, pendingGuardActions: guardActions.size, settledBodyReads, totalBodyReads: bodyReads.length, expectedAnalyticsCount, finalAnalyticsCount: observation.analyticsRequests.length };
  }
  viewportFinalizers.push(finalizeObservation);
  let failure;
  try {
    await page.goto(new URL("observe.html", publicBase).href);
    await page.waitForFunction(() => document.getElementById("workspace")?.getAttribute("aria-busy") === "false");
    assert.equal(await page.locator("#error").isVisible(), false);
    for (const id of controls.filter((id) => id !== "previous")) assert.equal(await page.locator(`#${id}`).isEnabled(), true, id);
    assert.equal(await page.locator("#previous").isDisabled(), true);
    assert.equal(await page.locator("#download").isVisible(), true);
    assert.equal(await page.locator("#download").getAttribute("href"), entry.recording);
    await page.waitForLoadState("networkidle"); await Promise.all(bodyReads);
    for (const asset of assets) assert.ok(observation.sourceResponses.some((item) => item.filename === asset.filename && item.status === "PASS"), `Browser did not receive verified ${asset.filename}`);
    assert.ok(observation.sourceResponses.every((item) => item.status === "PASS"), "Every observed critical browser response must match the checkout");
    assert.ok((observation.redirectResponses || []).every((item) => item.status === "PASS"), "Browser redirects must match the verified canonical chain");
    assert.ok(observation.requests.every((item) => item.method === "GET" && (new URL(item.url).origin === publicBase.origin || (item.url === knownEdgeInsertion.scriptUrl && observation.html.knownEdgeInjectionVerified && observation.analyticsRequests.some((request) => request.status === "BLOCKED_ACKNOWLEDGED")))), "Only public same-origin GETs or the exact intentionally blocked analytics request may be observed");

    const { first, next, horizon } = control;
    assert.equal(Number(await page.locator("#speed").inputValue()), horizon / 120);
    assert.equal(await page.locator("#duration").textContent(), "2 min");
    assert.equal(await shownNumber(page, "playhead"), first.sim_time);
    assert.equal(await shownNumber(page, "time"), first.sim_time);
    assert.equal(await shownNumber(page, "tick"), first.tick);
    assert.ok((await page.locator("#sample-cadence").textContent()).includes(control.dense ? `${control.states.toLocaleString("en-US")} real states` : `${entry.sample_count} real samples`));
    if (control.dense) {
      assert.match(await page.locator("#sample-cadence").textContent(), /every tick.*sparse archive/);
      assert.equal(await page.locator("#archive-link").getAttribute("href"), `./observe.html?experiment=${encodeURIComponent(entry.id)}&recording=archive`);
    }
    observation.initialCanvas = await canvasState(page); assert.ok(observation.initialCanvas.paintedPixels > 10);
    await layout(page, name === "desktop");
    const filename = `${name}-default.png`;
    await bounded(page.screenshot({ path: resolve(evidenceDir, filename), fullPage: true }), timeouts.operation, "Public screenshot");
    evidence.screenshots.push({ filename, sha256: digest(await readFile(resolve(evidenceDir, filename))) });

    // Page request events are a limited browser observation, not server counters.
    const requestBaseline = observation.requests.length;
    observation.playbackRequestBaseline = requestBaseline;
    await page.locator("#speed").evaluate((input) => { input.value = "1"; input.dispatchEvent(new Event("change", { bubbles: true })); });
    assert.equal(Number(await page.locator("#speed").inputValue()), 1);
    await page.locator("#play").evaluate((button) => { button.__publicSmokeClickTimes = []; button.addEventListener("click", () => button.__publicSmokeClickTimes.push(performance.now())); });
    const durationAt1x = await page.locator("#duration").textContent();
    assert.notEqual(durationAt1x, "2 min");
    observation.heldSamples = [];
    for (const fps of [30, 60]) {
      await page.locator("#draw-fps").selectOption(String(fps));
      const before = await shownNumber(page, "playhead");
      await page.locator("#play").click(); await page.waitForTimeout(1100); await page.locator("#play").click();
      const paused = await shownNumber(page, "playhead"), delta = paused - before;
      const clickTimes = await page.locator("#play").evaluate((button) => button.__publicSmokeClickTimes.slice(-2));
      assert.equal(clickTimes.length, 2);
      const wallSeconds = (clickTimes[1] - clickTimes[0]) / 1000;
      assert.ok(delta >= 1 && Math.abs(delta - wallSeconds) < .1, `Physical 1× advance at draw${fps}: ${delta} model seconds / ${wallSeconds} wall seconds`);
      assert.ok(paused < next.sim_time, "This smoke window must remain inside the first real sample interval");
      assert.equal(await shownNumber(page, "time"), first.sim_time); assert.equal(await shownNumber(page, "tick"), first.tick);
      assert.equal(await page.locator("#play").getAttribute("aria-label"), "Play recording");
      const canvas = await canvasState(page); assert.equal(canvas.sha256, observation.initialCanvas.sha256);
      assert.equal(await page.locator("#duration").textContent(), durationAt1x);
      await page.waitForTimeout(200); assert.equal(await shownNumber(page, "playhead"), paused);
      observation.heldSamples.push({ drawTarget: fps, playhead: paused, modelSecondsAdvanced: delta, wallSeconds, shownTime: first.sim_time, shownTick: first.tick, canvasSha256: canvas.sha256, durationAt1x });
    }
    await page.locator("#next").click();
    if (control.dense) await page.waitForFunction((tick) => Number(document.getElementById("tick").textContent.replaceAll(",", "")) === tick && document.getElementById("workspace").getAttribute("aria-busy") === "false", next.tick);
    assert.equal(await shownNumber(page, "playhead"), next.sim_time); assert.equal(await shownNumber(page, "time"), next.sim_time); assert.equal(await shownNumber(page, "tick"), next.tick);
    assert.equal(await shownNumber(page, "living"), next.cells.length);
    assert.equal(await page.locator("#play").getAttribute("aria-label"), "Play recording");
    observation.nextSample = { time: next.sim_time, tick: next.tick, livingCells: next.cells.length };
    await page.locator("#previous").click();
    if (control.dense) await page.waitForFunction((tick) => Number(document.getElementById("tick").textContent.replaceAll(",", "")) === tick && document.getElementById("workspace").getAttribute("aria-busy") === "false", first.tick);
    assert.equal(await shownNumber(page, "playhead"), first.sim_time); assert.equal(await shownNumber(page, "time"), first.sim_time);
    observation.playbackPageRequests = observation.requests.slice(requestBaseline);
    assert.deepEqual(observation.playbackPageRequests, [], "Playback/draw controls must create no Page request events");
    actionsPassed = true;
  } catch (error) { failure = error; }
  finally {
    try { await bounded(page.close(), timeouts.cleanup, "Public page cleanup"); pageClosed = true; }
    catch (error) { failure ||= error; }
    try { await finalizeObservation(); }
    catch (error) { failure ||= error; }
    observation.status = failure ? "FAIL" : "PASS";
  }
  if (failure) throw failure;
}

await mkdir(evidenceDir, { recursive: true });
try {
  for (const filename of ["trace.zip", "desktop-default.png", "mobile-default.png"]) await rm(resolve(evidenceDir, filename), { force: true });
  evidence.sourceCommit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim();
  evidence.sourceTree = execFileSync("git", ["rev-parse", "HEAD^{tree}"], { cwd: repoRoot, encoding: "utf8" }).trim();
  assert.ok(process.env.LIMINIS_EXPECTED_HEAD, "LIMINIS_EXPECTED_HEAD must identify the reviewed checkout");
  assert.equal(evidence.sourceCommit, process.env.LIMINIS_EXPECTED_HEAD, "Public smoke checkout must match expected HEAD");
  evidence.trackedWorkingTreeStatus = execFileSync("git", ["status", "--porcelain", "--untracked-files=no"], { cwd: repoRoot, encoding: "utf8" }).trim();
  assert.equal(evidence.trackedWorkingTreeStatus, "", "Public smoke requires a clean tracked checkout");
  const scriptBytes = await readFile(fileURLToPath(import.meta.url));
  assert.deepEqual(scriptBytes, execFileSync("git", ["show", "HEAD:scripts/check_public_recording_browser.mjs"], { cwd: repoRoot }));
  evidence.scriptSha256 = digest(scriptBytes);
  const assets = [];
  for (const filename of sourceNames) {
    const bytes = await readFile(resolve(siteRoot, filename));
    assets.push({ filename, bytes: bytes.byteLength, sha256: digest(bytes) });
  }
  const catalog = validateCatalog(JSON.parse(await readFile(resolve(siteRoot, "data/catalog.json"), "utf8")));
  const entry = selectExperiment(catalog), recordingBytes = await readFile(resolve(siteRoot, entry.recording));
  assert.equal(recordingBytes.byteLength, entry.bytes); assert.equal(digest(recordingBytes), entry.sha256);
  const data = JSON.parse(recordingBytes.toString("utf8")); validateRecording(data);
  assert.ok(data.frames.length > 1 && data.frames[1].sim_time - data.frames[0].sim_time > 10, "Default public recording must have a real held-sample smoke window");
  assets.push({ filename: entry.recording.replace(/^\.\//, ""), bytes: entry.bytes, sha256: entry.sha256 });
  let control = { first: data.frames[0], next: data.frames[1], horizon: data.experiment.steps * data.experiment.dt_seconds, dense: false };
  if (entry.dense) {
    // Expected frames come from the already fully validated local publication;
    // this smoke checks deployed transport and rendering, not a second model.
    const indexUrl = new URL(entry.dense.index, publicBase);
    const localFetch = async (url) => {
      const target = new URL(url, publicBase); assert.equal(target.origin, publicBase.origin);
      assert.ok(target.pathname.startsWith("/data/dense-cell-chamber/") && !target.search && !target.hash);
      return new Response(await readFile(resolve(siteRoot, `.${target.pathname}`)));
    };
    const loader = await createDenseRecordingLoader({ indexUrl: indexUrl.href, indexSha256: entry.dense.index_sha256, manifestPath: entry.dense.manifest, baseUrl: publicBase.href, fetcher: localFetch, cryptoProvider: webcrypto, prefetch: false });
    try {
      const first = (await loader.seek(0)).frame, next = (await loader.seek(1)).frame;
      assert.deepEqual(first, data.frames[0], "Dense genesis must match the immutable sparse archive");
      assert.equal(loader.manifest.experiment.steps, entry.experiment.steps);
      control = { first, next, horizon: loader.manifest.experiment.steps * loader.manifest.experiment.dt_seconds, states: loader.manifest.experiment.frames, dense: true };
      const filenames = [indexUrl.pathname.slice(1), new URL(entry.dense.manifest, indexUrl).pathname.slice(1), ...loader.manifest.chunks.slice(0, 2).map((chunk) => new URL(chunk.path, indexUrl).pathname.slice(1))];
      for (const filename of filenames) { const bytes = await readFile(resolve(siteRoot, filename)); assets.push({ filename, bytes: bytes.length, sha256: digest(bytes) }); }
      evidence.denseAdmission = { indexSha256: entry.dense.index_sha256, manifest: entry.dense.manifest, actualStates: control.states, sourceValidation: "Local publication was independently validated before merge; expected states decoded by reviewed source module", browserScope: "Deployed initial and prefetched chunks plus exact every-tick Step; whole-horizon rendering is not claimed", publicationRuntimeVerification: false };
    } finally { loader.close(); }
  }
  evidence.sourceFiles = assets; evidence.selectedRecording = { id: entry.id, filename: entry.recording, bytes: entry.bytes, sha256: entry.sha256 };
  evidence.status = "RUNNING";
  assert.ok(await check("Public critical assets and one allowlisted recording match checkout bytes", () => waitForPublicAssets(assets)));

  const moduleId = process.env.LIMINIS_PLAYWRIGHT_MODULE;
  assert.ok(moduleId, "Supply LIMINIS_PLAYWRIGHT_MODULE from the official protected CI install");
  const require = createRequire(import.meta.url), pkg = require(`${moduleId}/package.json`);
  assert.equal(pkg.name, "playwright"); assert.equal(pkg.version, "1.61.1"); evidence.playwrightVersion = pkg.version;
  const { chromium } = require(moduleId);
  browser = await chromium.launch({ headless: true, chromiumSandbox: true, args: ["--enable-automation"], timeout: timeouts.launch });
  evidence.browser = { engine: "Chromium", version: browser.version(), chromiumSandbox: true };
  assert.ok(await check("Observed browser arguments retain protected sandbox process model", async () => {
    const session = await bounded(browser.newBrowserCDPSession(), timeouts.cdp, "Public CDP session");
    try {
      const commandLine = await bounded(session.send("Browser.getBrowserCommandLine"), timeouts.cdp, "Public actual browser command line");
      evidence.launchArgumentCheck.status = "FAIL"; evidence.launchArgumentCheck.arguments = commandLine.arguments;
      assert.ok(Array.isArray(commandLine.arguments) && commandLine.arguments.length > 0 && commandLine.arguments.every((argument) => typeof argument === "string"));
      const forbidden = new Set(["--no-sandbox", "--no-zygote-sandbox", "--disable-setuid-sandbox", "--disable-namespace-sandbox", "--disable-seccomp-filter-sandbox", "--disable-gpu-sandbox", "--allow-sandbox-debugging", "--single-process"]);
      evidence.launchArgumentCheck.forbiddenSwitches = commandLine.arguments.map((argument) => argument.split("=", 1)[0]).filter((name) => forbidden.has(name) || /^--disable-.*sandbox$/.test(name));
      assert.deepEqual(evidence.launchArgumentCheck.forbiddenSwitches, []);
      evidence.launchArgumentCheck.status = "PASS";
    } finally { await bounded(session.detach(), timeouts.cdp, "Public CDP cleanup"); }
  }));
  context = await bounded(browser.newContext({ acceptDownloads: false }), timeouts.operation, "Public context creation");
  await bounded(context.tracing.start({ screenshots: true, snapshots: true, sources: true }), timeouts.cdp, "Public trace start"); tracing = true;
  for (const [name, viewport] of [["desktop", { width: 1440, height: 900 }], ["mobile", { width: 390, height: 844 }]]) {
    await check(`Public default observer ready, real canvas, physical 1× hold and ${control.dense ? "every-tick" : "archival sample"} stepping: ${name}`, () => bounded(viewportSmoke(name, viewport, assets, entry, control), timeouts.viewport, `${name} public smoke`));
  }
  evidence.status = evidence.checks.every((item) => item.status === "PASS") ? "PASS" : "FAIL";
} catch (error) { evidence.status = "FAIL"; evidence.blocker = error.stack || String(error); }
finally {
  async function cleanup(name, action, timeout = timeouts.cleanup) {
    try { return { ok: true, value: await bounded(action(), timeout, `${name} cleanup`) }; }
    catch (error) { (evidence.cleanupErrors ||= []).push({ name, error: error.stack || String(error) }); evidence.status = "FAIL"; return { ok: false }; }
  }
  if (tracing && context) {
    const saved = await cleanup("public trace", async () => {
      await context.tracing.stop({ path: resolve(evidenceDir, "trace.zip") });
      const bytes = await readFile(resolve(evidenceDir, "trace.zip")); assert.ok(bytes.byteLength > 0);
      return { bytes: bytes.byteLength, sha256: digest(bytes) };
    }, timeouts.traceStop);
    Object.assign(evidence.trace, saved.ok ? { status: "PASS", ...saved.value } : { status: "FAIL" });
  }
  if (context) await cleanup("public context", () => context.close());
  if (browser) await cleanup("public browser", () => browser.close());
  // Revalidate the same final arrays after global cleanup. No verdict is based
  // on an earlier request count or response-body snapshot.
  for (const finalize of viewportFinalizers) await cleanup("public final evidence revalidation", finalize, timeouts.operation);
  if (evidence.status === "PASS" && (evidence.trace.status !== "PASS" || evidence.publicAssets.status !== "PASS" || evidence.launchArgumentCheck.status !== "PASS" || evidence.screenshots.length !== 2 || evidence.browserObservations.length !== 2 || evidence.browserObservations.some((item) => item.status !== "PASS" || item.requestGuard?.status !== "PASS" || item.errors.length || item.analyticsRequests.some((request) => request.status !== "BLOCKED_ACKNOWLEDGED")) || evidence.blockedRequests.length)) { evidence.status = "FAIL"; evidence.blocker = "Required public smoke evidence is incomplete or a request violated read-only public scope."; }
  evidence.browser_html_raw_matches_source = evidence.browserObservations.length === 2 && evidence.browserObservations.every((item) => item.html?.rawHtmlMatchesSource === true);
  evidence.browser_html_known_edge_injection_verified = evidence.browserObservations.some((item) => item.html?.knownEdgeInjectionVerified === true);
  evidence.browser_html_after_exact_insertion_removal_matches_source = evidence.browserObservations.length === 2 && evidence.browserObservations.every((item) => item.html?.afterExactInsertionRemovalMatchesSource === true);
  evidence.application_playback = evidence.status;
  evidence.finishedAt = new Date().toISOString();
  await writeFile(resolve(evidenceDir, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(JSON.stringify({ scope: "public-recording-smoke", status: evidence.status, checks: evidence.checks.length, failed: evidence.checks.filter((item) => item.status === "FAIL").length, evidenceDir }));
  if (evidence.status !== "PASS") process.exitCode = 1;
}
