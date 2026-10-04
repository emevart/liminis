// Run with an officially installed Playwright Chromium on a non-root runner.
// Optional: LIMINIS_BASE_URL, LIMINIS_BROWSER_EVIDENCE_DIR,
// LIMINIS_PLAYWRIGHT_MODULE (official package path), LIMINIS_EXPECTED_HEAD.
// Chromium's sandbox remains enabled. No local acceptance is implied by this file.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFile, writeFile, mkdir, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { dirname, extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const siteRoot = dirname(fileURLToPath(import.meta.url));
const evidenceDir = resolve(process.env.LIMINIS_BROWSER_EVIDENCE_DIR || "playback-browser-evidence");
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const controls = ["play", "previous", "next", "scrub", "speed", "speed-preset", "draw-fps"];
const httpRequests = [];
const timeouts = { launch: 20_000, cdp: 5000, traceStop: 15_000, cleanup: 5000 };
const evidence = { nodeVersion: process.version, runner: { os: process.env.RUNNER_OS || process.platform, arch: process.env.RUNNER_ARCH || process.arch, imageOS: process.env.ImageOS || null, imageVersion: process.env.ImageVersion || null }, workflowRun: process.env.GITHUB_RUN_ID ? `${process.env.GITHUB_SERVER_URL || "https://github.com"}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}` : null, workflowAttempt: process.env.GITHUB_RUN_ATTEMPT || null, status: "NOT_RUN", startedAt: new Date().toISOString(), checks: [], screenshots: [], timeouts, trace: { status: "NOT_RUN", filename: "trace.zip", coverage: "Browser actions/network; Node assertion results are recorded in checks." }, launchArgumentCheck: { status: "NOT_RUN", method: "CDP Browser.getBrowserCommandLine" }, visibilityCoverage: "Synthetic document.hidden + visibilitychange tests adapter wiring only; native background scheduling is not asserted." };
let server, browser, context, tracing = false, baseUrl;

async function bounded(operation, timeout, label) {
  let timer;
  try {
    return await Promise.race([operation, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${label} timed out`)), timeout); })]);
  } finally { clearTimeout(timer); }
}

async function serveSite() {
  const mime = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".mjs": "text/javascript", ".json": "application/json" };
  server = createServer(async (request, response) => {
    try {
      const pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
      httpRequests.push({ method: request.method, path: pathname });
      if (pathname === "/favicon.ico") { response.writeHead(204); response.end(); return; }
      const filename = resolve(siteRoot, `.${pathname === "/" ? "/index.html" : pathname}`);
      if (!filename.startsWith(`${siteRoot}${sep}`)) { response.writeHead(403); response.end(); return; }
      const bytes = await readFile(filename);
      response.writeHead(200, { "Content-Type": `${mime[extname(filename)] || "application/octet-stream"}; charset=utf-8`, "Cache-Control": "no-store" });
      response.end(bytes);
    } catch { response.writeHead(404); response.end(); }
  });
  await new Promise((resolveReady, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolveReady); });
  return `http://127.0.0.1:${server.address().port}/`;
}

async function check(name, action) {
  const started = Date.now();
  try { await action(); evidence.checks.push({ name, status: "PASS", elapsedMs: Date.now() - started }); }
  catch (error) { evidence.checks.push({ name, status: "FAIL", elapsedMs: Date.now() - started, error: error.stack || String(error) }); }
}

async function screenshot(page, filename) {
  await page.screenshot({ path: resolve(evidenceDir, filename), fullPage: true });
  evidence.screenshots.push({ filename, sha256: digest(await readFile(resolve(evidenceDir, filename))) });
}

async function freshPage(viewport = { width: 1440, height: 900 }) {
  const httpBaseline = httpRequests.length;
  const page = await context.newPage();
  await page.setViewportSize(viewport);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(30_000);
  const errors = [], requests = [], allRequests = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
  page.on("request", (request) => { allRequests.push(request.url()); if (new URL(request.url()).pathname.includes("/data/")) requests.push(request.url()); });
  return { page, errors, requests, allRequests, httpBaseline };
}

function dataHttp(name, baseline, recording, downloads = 0, pageEvents = null) {
  if (!server) return; // Remote mode cannot attest actual HTTP with Page events.
  const actual = httpRequests.slice(baseline).filter((request) => request.path.startsWith("/data/"));
  const expected = [{ method: "GET", path: "/data/catalog.json" }];
  if (recording) for (let index = 0; index <= downloads; index++) expected.push({ method: "GET", path: new URL(recording, baseUrl).pathname });
  const check = { name, status: "FAIL", measurement: "local-http-server", baseline, requests: actual, expectedRequests: expected, pageRequestEvents: pageEvents };
  (evidence.httpChecks ||= []).push(check);
  assert.deepEqual(actual, expected, "Only catalog, selected recording and explicit downloads may reach the data server");
  check.status = "PASS";
}

async function idleHttpBaseline(page, allRequests) {
  await page.waitForLoadState("networkidle");
  return { server: httpRequests.length, page: allRequests.length };
}

function idleHttp(name, baseline, allRequests) {
  const pageRequests = allRequests.slice(baseline.page);
  const check = { name, status: "FAIL", measurement: server ? "local-http-server-and-page-events" : "page-events-only", baseline, requests: server ? httpRequests.slice(baseline.server) : null, pageRequests };
  (evidence.httpChecks ||= []).push(check);
  assert.deepEqual(pageRequests, [], "Playback and draw changes must not cause browser requests");
  if (server) assert.deepEqual(check.requests, [], "Playback and draw changes must not cause HTTP to any served path");
  check.status = "PASS";
}

async function open(page, experiment) {
  const url = new URL("observe.html", baseUrl);
  if (experiment) url.searchParams.set("experiment", experiment);
  await page.goto(url.href);
  await page.waitForFunction(() => document.getElementById("workspace").getAttribute("aria-busy") === "false");
}

const shownNumber = async (page, id) => Number((await page.locator(`#${id}`).textContent()).replaceAll(",", ""));
async function speed(page, value) {
  await page.locator("#speed").evaluate((input, next) => { input.value = String(next); input.dispatchEvent(new Event("change", { bubbles: true })); }, value);
}
async function seek(page, value) {
  await page.locator("#scrub").evaluate((input, next) => { input.value = String(next); input.dispatchEvent(new Event("input", { bubbles: true })); }, value);
}
async function ready(page) {
  assert.equal(await page.locator("#error").isVisible(), false);
  for (const id of ["play", "next", "scrub", "speed", "speed-preset", "draw-fps"]) assert.equal(await page.locator(`#${id}`).isEnabled(), true, id);
  assert.equal(await page.locator("#download").isVisible(), true);
  assert.ok(await page.locator("#download").getAttribute("href"));
}
async function canvasNonblank(page) {
  const painted = await page.locator("#chamber").evaluate((canvas) => {
    const pixels = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    let count = 0;
    for (let index = 0; index < pixels.length; index += 4) {
      if (pixels[index + 3] > 0 && (Math.abs(pixels[index] - 8) > 8 || Math.abs(pixels[index + 1] - 13) > 8 || Math.abs(pixels[index + 2] - 11) > 8)) count++;
    }
    return count;
  });
  assert.ok(painted > 10, `Expected real cell pixels, found ${painted}`);
  return painted;
}
async function canvasDigest(page) {
  return page.locator("#chamber").evaluate(async (canvas) => {
    const pixels = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    const bytes = await crypto.subtle.digest("SHA-256", pixels);
    return Array.from(new Uint8Array(bytes), (value) => value.toString(16).padStart(2, "0")).join("");
  });
}
async function layout(page, desktop) {
  const width = await page.evaluate(() => ({ viewport: innerWidth, page: document.documentElement.scrollWidth }));
  assert.ok(width.page <= width.viewport + 1, `Horizontal overflow: ${JSON.stringify(width)}`);
  for (const id of [...controls, "duration", "sample-cadence", "clock-help"]) {
    if (!desktop) await page.locator(`#${id}`).scrollIntoViewIfNeeded();
    const rect = await page.locator(`#${id}`).evaluate((element) => {
      const box = element.getBoundingClientRect();
      return { left: box.left, right: box.right, top: box.top, bottom: box.bottom, width: box.width, height: box.height, viewportWidth: innerWidth, viewportHeight: innerHeight };
    });
    assert.ok(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= rect.viewportWidth + 1 && rect.top >= -1 && rect.bottom <= rect.viewportHeight + 1, `Clipped ${id}: ${JSON.stringify(rect)}`);
  }
  await page.evaluate(() => scrollTo(0, 0));
}
async function errorDisabled(page, expectedMessage) {
  assert.equal(await page.locator("#error").isVisible(), true);
  assert.match(await page.locator("#error").textContent(), expectedMessage);
  for (const id of controls) assert.equal(await page.locator(`#${id}`).isDisabled(), true, id);
  assert.equal(await page.locator("#download").isVisible(), false);
  assert.equal(await page.locator("#download").getAttribute("href"), null);
}

await mkdir(evidenceDir, { recursive: true });
try {
  // A failed new run must never attest a previous trace in a reused directory.
  await rm(resolve(evidenceDir, "trace.zip"), { force: true });
  evidence.sourceCommit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: siteRoot, encoding: "utf8" }).trim();
  evidence.sourceTree = execFileSync("git", ["rev-parse", "HEAD^{tree}"], { cwd: siteRoot, encoding: "utf8" }).trim();
  if (process.env.LIMINIS_EXPECTED_HEAD) assert.equal(evidence.sourceCommit, process.env.LIMINIS_EXPECTED_HEAD, "Acceptance checkout must equal expected PR head");
  evidence.trackedWorkingTreeStatus = execFileSync("git", ["status", "--porcelain", "--untracked-files=no"], { cwd: siteRoot, encoding: "utf8" }).trim();
  assert.equal(evidence.trackedWorkingTreeStatus, "", "Acceptance requires a clean tracked candidate checkout");
  evidence.scriptSha256 = digest(await readFile(fileURLToPath(import.meta.url)));
  evidence.sourceFiles = [];
  for (const filename of ["observe.html", "observer.js", "observer.css", "playback.mjs", "recording-loader.mjs", "recording.mjs", "catalog.mjs", "data/catalog.json"]) {
    evidence.sourceFiles.push({ filename, sha256: digest(await readFile(resolve(siteRoot, filename))) });
  }
  const catalogBytes = await readFile(resolve(siteRoot, "data/catalog.json"));
  const catalog = JSON.parse(catalogBytes);
  evidence.recordings = catalog.entries.map(({ id, sha256, bytes }) => ({ id, sha256, bytes }));
  baseUrl = process.env.LIMINIS_BASE_URL ? `${process.env.LIMINIS_BASE_URL.replace(/\/+$/, "")}/` : await serveSite();
  evidence.baseUrl = baseUrl;
  evidence.httpRequestCoverage = server ? { status: "PENDING", measurement: "local-http-server" } : { status: "NOT_ASSERTED", reason: "Remote base URL has no built-in server counter; Page events do not attest native download HTTP." };
  // Evidence for an optional remote URL must still match this exact checkout.
  for (const source of evidence.sourceFiles) {
    const response = await fetch(new URL(source.filename, baseUrl), { signal: AbortSignal.timeout(10_000), cache: "no-store" });
    assert.ok(response.ok, `Cannot verify served source ${source.filename}: ${response.status}`);
    source.servedSha256 = digest(Buffer.from(await response.arrayBuffer()));
    assert.equal(source.servedSha256, source.sha256, `Served source differs from candidate HEAD: ${source.filename}`);
  }
  const require = createRequire(import.meta.url), moduleId = process.env.LIMINIS_PLAYWRIGHT_MODULE || "playwright";
  const { chromium } = require(moduleId);
  evidence.playwrightVersion = require(`${moduleId}/package.json`).version;
  browser = await chromium.launch({ headless: true, chromiumSandbox: true, args: ["--enable-automation"], timeout: timeouts.launch });
  evidence.browser = { engine: "Chromium", version: browser.version(), chromiumSandbox: true };
  const session = await bounded(browser.newBrowserCDPSession(), timeouts.cdp, "Browser command-line session");
  try {
    const commandLine = await bounded(session.send("Browser.getBrowserCommandLine"), timeouts.cdp, "Actual browser command-line evidence");
    evidence.launchArgumentCheck.status = "FAIL";
    evidence.launchArgumentCheck.arguments = commandLine.arguments;
    assert.ok(Array.isArray(commandLine.arguments) && commandLine.arguments.length > 0 && commandLine.arguments.every((argument) => typeof argument === "string"), "CDP must return actual browser arguments");
    const forbidden = new Set(["--no-sandbox", "--no-zygote-sandbox", "--disable-setuid-sandbox", "--disable-namespace-sandbox", "--disable-seccomp-filter-sandbox", "--disable-gpu-sandbox", "--allow-sandbox-debugging", "--single-process"]);
    evidence.launchArgumentCheck.forbiddenSwitches = commandLine.arguments.map((argument) => argument.split("=", 1)[0]).filter((name) => forbidden.has(name) || /^--disable-.*sandbox$/.test(name));
    assert.deepEqual(evidence.launchArgumentCheck.forbiddenSwitches, [], "Actual browser command line must not disable sandbox layers or the accepted process model");
    evidence.launchArgumentCheck.status = "PASS";
    evidence.checks.push({ name: "Protected launch has actual CDP arguments without sandbox opt-out switches", status: "PASS" });
  } finally { await bounded(session.detach(), timeouts.cdp, "Browser command-line session cleanup"); }
  context = await browser.newContext({ acceptDownloads: true });
  await bounded(context.tracing.start({ screenshots: true, snapshots: true, sources: true }), timeouts.cdp, "Context trace start");
  tracing = true;
  evidence.status = "RUNNING";

  for (const entry of catalog.entries) await check(`default physical overview, cadence, verified download: ${entry.id}`, async () => {
    const { page, errors, requests, httpBaseline } = await freshPage();
    try {
      await open(page, entry.id); await ready(page);
      const horizon = entry.experiment.steps * entry.experiment.dt_seconds;
      assert.equal(Number(await page.locator("#speed").inputValue()), horizon / 120);
      assert.equal(await page.locator("#duration").textContent(), "2 min");
      assert.equal(await shownNumber(page, "playhead"), 0);
      assert.equal(await shownNumber(page, "time"), 0);
      const cadence = await page.locator("#sample-cadence").textContent();
      assert.ok(cadence.includes(`${entry.sample_count} real samples`));
      assert.ok(cadence.includes(`${entry.experiment.sample_every.toLocaleString("en-US")} ticks / ${(entry.experiment.sample_every * entry.experiment.dt_seconds).toLocaleString("en-US")} model s`), cadence);
      assert.equal((await page.locator("#download").getAttribute("href")), entry.recording);
      evidence.checks.push({ name: `canvas pixels: ${entry.id}`, status: "PASS", paintedPixels: await canvasNonblank(page) });
      await layout(page, true);
      await screenshot(page, `${entry.id}-desktop.png`);
      assert.equal(requests.length, 2, "Only catalog and selected recording should load");
      dataHttp(`initial load HTTP: ${entry.id}`, httpBaseline, entry.recording, 0, requests.length);
      const [download] = await Promise.all([page.waitForEvent("download"), page.locator("#download").click()]);
      const downloaded = await readFile(await download.path());
      assert.equal(downloaded.byteLength, entry.bytes); assert.equal(digest(downloaded), entry.sha256);
      evidence.checks.push({ name: `actual download integrity: ${entry.id}`, status: "PASS", bytes: downloaded.byteLength, sha256: digest(downloaded) });
      dataHttp(`actual download HTTP: ${entry.id}`, httpBaseline, entry.recording, 1, requests.length);
      assert.deepEqual(errors, []);
    } finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });

  const entry = catalog.entries.find((item) => item.id === catalog.default_experiment);
  assert.ok(entry);
  for (const [name, viewport, desktop] of [
    ["desktop-short", { width: 1366, height: 600 }, true],
    ["mobile", { width: 390, height: 844 }, false],
    ["mobile-narrow", { width: 320, height: 760 }, false],
  ]) await check(`responsive controls and real canvas: ${name}`, async () => {
    const { page, errors } = await freshPage(viewport);
    try { await open(page); await ready(page); await layout(page, desktop); await canvasNonblank(page); await screenshot(page, `${name}.png`); assert.deepEqual(errors, []); }
    finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });

  await check("real wall-time 1× hold, pause/resume, rate change, invalid input, seek and sample stepping", async () => {
    const { page, errors, requests, allRequests, httpBaseline } = await freshPage();
    try {
      await open(page); await ready(page); const idleBaseline = await idleHttpBaseline(page, allRequests); await speed(page, 1);
      assert.equal(await page.locator("#duration").textContent(), "3.47 d");
      const heldPixels = await canvasDigest(page);
      await page.locator("#play").click(); await page.waitForTimeout(1100); await page.locator("#play").click();
      const paused = await shownNumber(page, "playhead");
      assert.ok(paused >= 1 && paused < 3, `1× playhead after about 1.1 wall seconds: ${paused}`);
      assert.equal(await shownNumber(page, "time"), 0);
      assert.equal(await shownNumber(page, "tick"), 0);
      assert.equal(await canvasDigest(page), heldPixels, "1× holding must preserve actual sample pixels");
      await page.waitForTimeout(200); assert.equal(await shownNumber(page, "playhead"), paused);
      await page.locator("#play").click(); await page.waitForTimeout(300); await speed(page, 2); await page.waitForTimeout(300); await page.locator("#play").click();
      const resumed = await shownNumber(page, "playhead");
      assert.ok(resumed > paused + .7 && resumed < paused + 3, `Resume/rate change: ${paused} → ${resumed}`);
      for (const invalid of ["0", "-1", "", "1e309", "Infinity", "NaN"]) {
        await speed(page, invalid); assert.equal(await page.locator("#speed").inputValue(), "2"); assert.equal(await page.locator("#rate-error").isVisible(), true);
      }
      await speed(page, 1); assert.equal(await page.locator("#rate-error").isVisible(), false);
      const gap = entry.experiment.sample_every * entry.experiment.dt_seconds;
      await seek(page, gap * 1.5);
      assert.equal(await shownNumber(page, "playhead"), gap * 1.5);
      assert.equal(await shownNumber(page, "time"), gap);
      assert.equal(await page.locator("#play").getAttribute("aria-label"), "Play recording");
      await page.locator("#next").click(); assert.equal(await shownNumber(page, "time"), gap * 2); assert.equal(await shownNumber(page, "playhead"), gap * 2);
      await page.locator("#previous").click(); assert.equal(await shownNumber(page, "time"), gap); assert.equal(await shownNumber(page, "playhead"), gap);
      assert.equal(requests.length, 2, "Playback controls must not fetch observations");
      dataHttp("physical playback controls HTTP", httpBaseline, entry.recording, 0, requests.length);
      idleHttp("physical controls issue no HTTP", idleBaseline, allRequests);
      assert.deepEqual(errors, []);
    } finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });

  await check("held real sample pixels remain identical through RAF at draw30/60", async () => {
    const { page, errors, requests, allRequests, httpBaseline } = await freshPage();
    try {
      await open(page); const idleBaseline = await idleHttpBaseline(page, allRequests); await speed(page, 1);
      const baseline = await canvasDigest(page), duration = await page.locator("#duration").textContent();
      for (const fps of [30, 60]) {
        await page.locator("#draw-fps").selectOption(String(fps));
        const before = await shownNumber(page, "playhead");
        await page.locator("#play").click(); await page.waitForTimeout(200); await page.locator("#play").click();
        assert.ok(await shownNumber(page, "playhead") > before + .15, "Physical playhead must advance while the real sample is held");
        assert.equal(await canvasDigest(page), baseline); assert.equal(await shownNumber(page, "time"), 0); assert.equal(await shownNumber(page, "tick"), 0);
        assert.equal(await page.locator("#duration").textContent(), duration);
        await page.locator("#chamber").screenshot({ path: resolve(evidenceDir, `held-${fps}fps.png`) });
        evidence.screenshots.push({ filename: `held-${fps}fps.png`, sha256: digest(await readFile(resolve(evidenceDir, `held-${fps}fps.png`))) });
      }
      evidence.checks.push({ name: "held sample canvas pixel SHA-256", status: "PASS", sha256: baseline });
      assert.equal(requests.length, 2); dataHttp("playback/draw target HTTP", httpBaseline, entry.recording, 0, requests.length); idleHttp("held draw30/60 issue no HTTP", idleBaseline, allRequests); assert.deepEqual(errors, []);
    } finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });

  for (const fps of [30, 60]) await check(`draw target ${fps} FPS preserves duration, reaches real endpoint, explicit replay`, async () => {
    const { page, errors, requests, allRequests, httpBaseline } = await freshPage();
    try {
      await open(page); const idleBaseline = await idleHttpBaseline(page, allRequests); await page.locator("#draw-fps").selectOption(String(fps));
      const horizon = entry.experiment.steps * entry.experiment.dt_seconds;
      await speed(page, horizon); assert.equal(await page.locator("#duration").textContent(), "1 s");
      const started = Date.now(); await page.locator("#play").click();
      await page.waitForFunction(() => document.getElementById("play").getAttribute("aria-label") === "Replay recording");
      assert.ok(Date.now() - started >= 900, "Endpoint should take one physical wall second at either draw target");
      assert.equal(await shownNumber(page, "time"), horizon); assert.equal(await shownNumber(page, "playhead"), horizon);
      assert.equal(await shownNumber(page, "tick"), entry.experiment.steps);
      assert.equal(await page.locator("#remaining").textContent(), "0 s");
      assert.equal(await page.locator("#next").isDisabled(), true);
      await page.locator("#play").click();
      assert.equal(await page.locator("#play").getAttribute("aria-label"), "Pause recording");
      assert.ok(await shownNumber(page, "playhead") < horizon);
      await page.locator("#play").click();
      assert.equal(requests.length, 2); dataHttp("playback/draw target HTTP", httpBaseline, entry.recording, 0, requests.length); idleHttp(`draw${fps} endpoint/replay issue no HTTP`, idleBaseline, allRequests); assert.deepEqual(errors, []);
    } finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });

  await check("synthetic visibility event cancels playback without auto-resume or hidden backlog (native scheduling NOT ASSERTED)", async () => {
    const { page, errors, requests, allRequests, httpBaseline } = await freshPage();
    try {
      await open(page); const idleBaseline = await idleHttpBaseline(page, allRequests); await speed(page, 1); await page.locator("#play").click(); await page.waitForTimeout(200);
      await page.evaluate(() => { Object.defineProperty(document, "hidden", { configurable: true, get: () => true }); document.dispatchEvent(new Event("visibilitychange")); });
      const paused = await shownNumber(page, "playhead");
      assert.equal(await page.locator("#play").getAttribute("aria-label"), "Play recording");
      await page.waitForTimeout(300); assert.equal(await shownNumber(page, "playhead"), paused);
      await page.evaluate(() => { delete document.hidden; document.dispatchEvent(new Event("visibilitychange")); });
      await page.waitForTimeout(100); assert.equal(await shownNumber(page, "playhead"), paused);
      await page.locator("#play").click(); await page.waitForTimeout(200); await page.locator("#play").click();
      assert.ok(await shownNumber(page, "playhead") < paused + 1, "Hidden time must not become a resumed burst");
      assert.equal(requests.length, 2); dataHttp("playback/draw target HTTP", httpBaseline, entry.recording, 0, requests.length); idleHttp("visibility pause/resume issue no HTTP", idleBaseline, allRequests); assert.deepEqual(errors, []);
    } finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });

  await check("unknown experiment fails closed without a dataset request", async () => {
    const { page, errors, requests, httpBaseline } = await freshPage();
    try { await open(page, "not-in-the-catalog"); await errorDisabled(page, /not in the catalog/i); assert.equal(requests.length, 1); dataHttp("unknown experiment HTTP", httpBaseline, null, 0, requests.length); assert.deepEqual(errors, []); }
    finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });

  const originalBytes = await readFile(resolve(siteRoot, entry.recording));
  await check("same-size corrupted recording fails digest before controls/download activate", async () => {
    const { page, errors } = await freshPage();
    try {
      const corrupted = Buffer.from(originalBytes); corrupted[corrupted.length - 1] ^= 1;
      await page.route(`**/${entry.recording.replace(/^\.\//, "")}`, (route) => route.fulfill({ status: 200, contentType: "application/json", body: corrupted }));
      await open(page); await errorDisabled(page, /does not match the catalog/i); assert.deepEqual(errors, []);
    } finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });

  await check("integrity-valid unsupported recording schema keeps every control/download disabled", async () => {
    const { page, errors } = await freshPage();
    try {
      const data = JSON.parse(originalBytes); data.schema_version = 999;
      const malformed = Buffer.from(JSON.stringify(data));
      const alteredCatalog = structuredClone(catalog);
      const alteredEntry = alteredCatalog.entries.find((item) => item.id === entry.id);
      alteredEntry.bytes = malformed.length; alteredEntry.sha256 = digest(malformed);
      await page.route("**/data/catalog.json", (route) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(alteredCatalog) }));
      await page.route(`**/${entry.recording.replace(/^\.\//, "")}`, (route) => route.fulfill({ status: 200, contentType: "application/json", body: malformed }));
      await open(page); await errorDisabled(page, /unsupported schema/i); assert.deepEqual(errors, []);
    } finally { await bounded(page.close(), timeouts.cleanup, "Page cleanup"); }
  });
  if (server) evidence.httpRequestCoverage.status = evidence.httpChecks?.length && evidence.httpChecks.every((item) => item.status === "PASS") ? "PASS" : "FAIL";
  evidence.status = evidence.checks.some((item) => item.status === "FAIL") ? "FAIL" : "PASS";
} catch (error) {
  evidence.status = browser ? "FAIL" : "NOT_RUN";
  evidence.blocker = error.stack || String(error);
} finally {
  async function cleanup(name, action, timeout = timeouts.cleanup) {
    try {
      return { ok: true, value: await bounded(action(), timeout, `${name} cleanup`) };
    } catch (error) {
      const message = error.stack || String(error);
      (evidence.cleanupErrors ||= []).push({ name, error: message }); evidence.status = "FAIL";
      return { ok: false, error: message };
    }
  }
  try {
    if (tracing && context) {
      const saved = await cleanup("trace", async () => {
        await context.tracing.stop({ path: resolve(evidenceDir, "trace.zip") });
        const bytes = await readFile(resolve(evidenceDir, "trace.zip"));
        assert.ok(bytes.byteLength > 0, "Required trace must be nonempty");
        return { bytes: bytes.byteLength, sha256: digest(bytes) };
      }, timeouts.traceStop);
      // A timed-out operation can finish late; it must not overwrite FAIL metadata.
      Object.assign(evidence.trace, saved.ok ? { status: "PASS", ...saved.value } : { status: "FAIL", error: saved.error });
    }
    if (context) await cleanup("context", () => context.close());
    if (browser) await cleanup("browser", () => browser.close());
    if (server) { await cleanup("server", () => new Promise((closed, reject) => server.close((error) => error ? reject(error) : closed()))); server.closeAllConnections(); }
  } finally {
    if (evidence.status === "PASS" && evidence.trace.status !== "PASS") { evidence.status = "FAIL"; evidence.blocker = "Required context trace was not published."; }
    if (evidence.status === "PASS" && evidence.httpRequestCoverage?.status !== "PASS") { evidence.status = "NOT_RUN"; evidence.blocker = "Full HTTP acceptance requires the built-in server counter; remote UI observations are retained."; }
    evidence.finishedAt = new Date().toISOString();
    await writeFile(resolve(evidenceDir, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);
    console.log(JSON.stringify({ status: evidence.status, checks: evidence.checks.length, failed: evidence.checks.filter((item) => item.status === "FAIL").length, evidenceDir }));
    if (evidence.status !== "PASS") process.exitCode = 1;
  }
}
