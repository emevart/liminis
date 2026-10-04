// Read-only production smoke on the existing public site. Run in protected CI
// with its official playwright@1.61.1 install, never with a sandbox opt-out.
// This is separate from site/playback.browser.mjs and its local HTTP gate.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFile, writeFile, mkdir, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { selectExperiment, validateCatalog } from "../site/catalog.mjs";
import { validateRecording } from "../site/recording.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const siteRoot = resolve(repoRoot, "site");
const evidenceDir = resolve(repoRoot, "target/qa/public-playback");
const publicBase = new URL("https://liminis.dev/");
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const controls = ["play", "previous", "next", "scrub", "speed", "speed-preset", "draw-fps"];
const sourceNames = ["observe.html", "observer.js", "observer.css", "playback.mjs", "recording-loader.mjs", "recording.mjs", "catalog.mjs", "data/catalog.json"];
const timeouts = { deployment: 180_000, retry: 5000, fetch: 15_000, operation: 15_000, viewport: 60_000, launch: 20_000, cdp: 5000, traceStop: 15_000, cleanup: 5000 };
const evidence = {
  scope: "Read-only deployed recording smoke; does not replace local recorded-browser acceptance.",
  publicBase: publicBase.href, status: "NOT_RUN", startedAt: new Date().toISOString(),
  nodeVersion: process.version,
  runner: { os: process.env.RUNNER_OS || process.platform, arch: process.env.RUNNER_ARCH || process.arch, imageOS: process.env.ImageOS || null, imageVersion: process.env.ImageVersion || null },
  workflowRun: process.env.GITHUB_RUN_ID ? `${process.env.GITHUB_SERVER_URL || "https://github.com"}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}` : null,
  workflowAttempt: process.env.GITHUB_RUN_ATTEMPT || null,
  timeouts, checks: [], screenshots: [], publicAssetAttempts: [], browserObservations: [], blockedRequests: [],
  publicAssets: { status: "NOT_RUN", measurement: "Node HTTPS GET response bytes, size and SHA-256 against the clean local checkout" },
  launchArgumentCheck: { status: "NOT_RUN", method: "CDP Browser.getBrowserCommandLine" },
  trace: { status: "NOT_RUN", filename: "trace.zip", coverage: "Actual public browser actions, responses, snapshots and screenshots; Node fetch observations are in evidence.json." },
  httpCoverage: { publicAssetBytes: "Node HTTPS responses and actual Playwright response bodies are verified separately.", playbackRequests: "Playwright Page.request events only; no public-server request counter is available.", fullLocalHttpAcceptance: "NOT_ASSERTED: remains the separate site/playback.browser.mjs gate." },
  limits: ["Chromium/Linux and desktop plus 390px mobile viewport only.", "Input assignment plus dispatched change verifies adapter semantics; native typing/range/touch and full accessibility are not asserted.", "Native background scheduling, achieved FPS, native download transport and kernel isolation are not asserted.", "The 30/60 controls are draw targets; held sample pixels and physical playhead are checked, not achieved frame rate."],
};
let browser, context, tracing = false;

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
// externally, and redirects cannot move these read-only requests elsewhere.
async function fetchAsset(asset, timeout) {
  const url = new URL(asset.filename, publicBase);
  const observation = { filename: asset.filename, url: url.href, expectedBytes: asset.bytes, expectedSha256: asset.sha256, status: "FAIL" };
  const controller = new AbortController(), timer = setTimeout(() => controller.abort(), timeout);
  let reader;
  try {
    const response = await fetch(url, { signal: controller.signal, cache: "no-store", redirect: "error", headers: { "Cache-Control": "no-cache" } });
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
  } catch (error) { observation.error = error.message || String(error); }
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

async function viewportSmoke(name, viewport, assets, entry, data) {
  const page = await bounded(context.newPage(), timeouts.operation, "Public page creation");
  page.setDefaultTimeout(timeouts.operation); page.setDefaultNavigationTimeout(timeouts.operation);
  await bounded(page.setViewportSize(viewport), timeouts.operation, "Public viewport setup");
  const observation = { name, viewport, status: "RUNNING", sourceResponses: [], requests: [], errors: [] };
  evidence.browserObservations.push(observation);
  const expected = new Map(assets.map((asset) => [new URL(asset.filename, publicBase).pathname, asset]));
  const bodyReads = [];
  page.on("pageerror", (error) => observation.errors.push(error.message));
  page.on("console", (message) => { if (message.type() === "error") observation.errors.push(message.text()); });
  page.on("requestfailed", (request) => observation.errors.push(`Request failed: ${request.url()} (${request.failure()?.errorText})`));
  page.on("request", (request) => observation.requests.push({ method: request.method(), url: request.url() }));
  page.on("response", (response) => {
    const url = new URL(response.url()), asset = url.origin === publicBase.origin ? expected.get(url.pathname) : null;
    if (!asset) return;
    const result = { filename: asset.filename, url: response.url(), httpStatus: response.status(), measurement: "Playwright response.body", expectedBytes: asset.bytes, expectedSha256: asset.sha256, status: "FAIL" };
    observation.sourceResponses.push(result);
    bodyReads.push(bounded(response.body(), timeouts.operation, `Browser response body: ${asset.filename}`).then((bytes) => {
      Object.assign(result, { bytes: bytes.byteLength, sha256: digest(bytes) });
      assert.equal(response.status(), 200); assert.equal(result.bytes, asset.bytes); assert.equal(result.sha256, asset.sha256);
      result.status = "PASS";
    }).catch((error) => { result.error = error.message || String(error); }));
  });
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
    assert.ok(observation.requests.every((item) => item.method === "GET" && new URL(item.url).origin === publicBase.origin), "Smoke must only observe read-only requests to liminis.dev");

    const first = data.frames[0], next = data.frames[1], horizon = data.frames.at(-1).sim_time - first.sim_time;
    assert.equal(Number(await page.locator("#speed").inputValue()), horizon / 120);
    assert.equal(await page.locator("#duration").textContent(), "2 min");
    assert.equal(await shownNumber(page, "playhead"), first.sim_time);
    assert.equal(await shownNumber(page, "time"), first.sim_time);
    assert.equal(await shownNumber(page, "tick"), first.tick);
    assert.ok((await page.locator("#sample-cadence").textContent()).includes(`${entry.sample_count} real samples`));
    observation.initialCanvas = await canvasState(page); assert.ok(observation.initialCanvas.paintedPixels > 10);
    await layout(page, name === "desktop");
    const filename = `${name}-default.png`;
    await bounded(page.screenshot({ path: resolve(evidenceDir, filename), fullPage: true }), timeouts.operation, "Public screenshot");
    evidence.screenshots.push({ filename, sha256: digest(await readFile(resolve(evidenceDir, filename))) });

    // Page request events are a limited browser observation, not server counters.
    const requestBaseline = observation.requests.length;
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
    assert.equal(await shownNumber(page, "playhead"), next.sim_time); assert.equal(await shownNumber(page, "time"), next.sim_time); assert.equal(await shownNumber(page, "tick"), next.tick);
    assert.equal(await shownNumber(page, "living"), next.cells.length);
    assert.equal(await page.locator("#play").getAttribute("aria-label"), "Play recording");
    observation.nextSample = { time: next.sim_time, tick: next.tick, livingCells: next.cells.length };
    await page.locator("#previous").click(); assert.equal(await shownNumber(page, "playhead"), first.sim_time); assert.equal(await shownNumber(page, "time"), first.sim_time);
    observation.playbackPageRequests = observation.requests.slice(requestBaseline);
    assert.deepEqual(observation.playbackPageRequests, [], "Playback/draw controls must create no Page request events");
    assert.deepEqual(observation.errors, []);
    observation.status = "PASS";
  } catch (error) { observation.status = "FAIL"; throw error; }
  finally { await bounded(page.close(), timeouts.cleanup, "Public page cleanup"); }
}

await mkdir(evidenceDir, { recursive: true });
try {
  for (const filename of ["trace.zip", "desktop-default.png", "mobile-default.png"]) await rm(resolve(evidenceDir, filename), { force: true });
  evidence.sourceCommit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim();
  evidence.sourceTree = execFileSync("git", ["rev-parse", "HEAD^{tree}"], { cwd: repoRoot, encoding: "utf8" }).trim();
  if (process.env.LIMINIS_EXPECTED_HEAD) assert.equal(evidence.sourceCommit, process.env.LIMINIS_EXPECTED_HEAD, "Public smoke checkout must match expected HEAD");
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
  // Same-origin GETs continue unchanged. Any other request fails this smoke
  // before transmission; this route also disables browser cache for fresh bytes.
  await bounded(context.route("**/*", (route) => {
    const request = route.request(), url = new URL(request.url());
    if (request.method() === "GET" && url.origin === publicBase.origin) return route.continue();
    evidence.blockedRequests.push({ method: request.method(), url: request.url() });
    return route.abort("blockedbyclient");
  }), timeouts.operation, "Read-only public request guard");
  await bounded(context.tracing.start({ screenshots: true, snapshots: true, sources: true }), timeouts.cdp, "Public trace start"); tracing = true;
  for (const [name, viewport] of [["desktop", { width: 1440, height: 900 }], ["mobile", { width: 390, height: 844 }]]) {
    await check(`Public default observer ready, real canvas, physical 1× hold and sample stepping: ${name}`, () => bounded(viewportSmoke(name, viewport, assets, entry, data), timeouts.viewport, `${name} public smoke`));
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
  if (evidence.status === "PASS" && (evidence.trace.status !== "PASS" || evidence.publicAssets.status !== "PASS" || evidence.launchArgumentCheck.status !== "PASS" || evidence.screenshots.length !== 2 || evidence.browserObservations.length !== 2 || evidence.browserObservations.some((item) => item.status !== "PASS") || evidence.blockedRequests.length)) { evidence.status = "FAIL"; evidence.blocker = "Required public smoke evidence is incomplete or a request violated read-only public scope."; }
  evidence.finishedAt = new Date().toISOString();
  await writeFile(resolve(evidenceDir, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(JSON.stringify({ scope: "public-recording-smoke", status: evidence.status, checks: evidence.checks.length, failed: evidence.checks.filter((item) => item.status === "FAIL").length, evidenceDir }));
  if (evidence.status !== "PASS") process.exitCode = 1;
}
