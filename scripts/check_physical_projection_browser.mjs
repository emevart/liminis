// Protected real-host LIVE3 QA. Run only after the pinned release host is built.
// No fake cells/API states, production helper exports, or view/hit-map access.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { mkdtemp, mkdir, readFile, writeFile, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = resolve(root, "target/qa/physical-projection-browser");
const sourceConfig = resolve(root, "configs/scenarios/cell-chamber-physical.toml");
const hostBinary = resolve(root, "target/release/liminis");
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const delay = (ms) => new Promise((done) => setTimeout(done, ms));
const axes = { xy: [0, 1, 2], xz: [0, 2, 1], yz: [1, 2, 0] };
const names = ["x", "y", "z"];
const timeouts = { run: 240_000, operation: 15_000, api: 5000, launch: 20_000, cdp: 5000, division: 90_000, historyHold: 20_000, trace: 15_000, cleanup: 5000 };
const report = {
  status: "NOT_RUN", startedAt: new Date().toISOString(), timeouts,
  scope: "Bounded real-host 2D projection acceptance; point-center markers are not bodies, collisions, spatial chemistry or science findings.",
  nodeVersion: process.version,
  workflowRun: process.env.GITHUB_RUN_ID ? `${process.env.GITHUB_SERVER_URL || "https://github.com"}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}` : null,
  workflowAttempt: process.env.GITHUB_RUN_ATTEMPT || null,
  runner: { os: process.env.RUNNER_OS || process.platform, arch: process.env.RUNNER_ARCH || process.arch, imageOS: process.env.ImageOS || null, imageVersion: process.env.ImageVersion || null },
  checks: [], screenshots: [], geometries: [], hosts: [], pageErrors: [], consoleErrors: [], resourceWarnings: [], cleanupErrors: [],
  launchArgumentCheck: { status: "NOT_RUN", method: "CDP Browser.getBrowserCommandLine" },
  trace: { status: "NOT_RUN", filename: "trace.zip" },
  limits: ["Chromium/Linux; desktop and 390/320px mobile viewports, not a full accessibility or device audit.", "30/60 are display targets; achieved FPS and native background scheduling are not asserted.", "Canvas methods observe paint arguments unchanged; geometry expectations use actual API coordinates and observed box edges, never production helpers or internal view.hits.", "Public-server counters, long-run motion/statistics and new science claims are outside this local real-host gate.", "An absent division parent is retained as an absent selection; absence is not a death assertion."],
};
let temporary, host, browser, context, page, tracing = false, stopRequested = false, releaseHistory;
let hostLog = "";
const identities = new Map();
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();

async function bounded(operation, timeout, label) {
  let timer;
  try { return await Promise.race([operation, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`Timed out: ${label}`)), timeout); })]); }
  finally { clearTimeout(timer); }
}
async function check(name, action) {
  const started = Date.now();
  try { assert.equal(stopRequested, false, "Acceptance was interrupted"); await action(); report.checks.push({ name, status: "PASS", elapsedMs: Date.now() - started }); }
  catch (error) { report.checks.push({ name, status: "FAIL", elapsedMs: Date.now() - started, error: error.stack || String(error) }); throw error; }
}
function fail(error, stage = "run") {
  stopRequested = true; report.status = "FAIL"; process.exitCode = 1;
  (report.failures ||= []).push({ stage, error: error.stack || String(error) });
}
async function eventually(predicate, label, timeout = timeouts.operation) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    assert.equal(stopRequested, false, "Acceptance was interrupted");
    if (await predicate()) return;
    if (host && (host.exitCode !== null || host.signalCode !== null)) throw new Error(`Host exited during ${label}: ${hostLog}`);
    await delay(40);
  }
  throw new Error(`Timed out: ${label}`);
}
async function api(origin, path, body) {
  const response = await fetch(`${origin}${path}`, { signal: AbortSignal.timeout(timeouts.api), cache: "no-store", ...(body ? { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) } : {}) });
  assert.equal(response.status, 200, `${path}: HTTP ${response.status}`);
  return response.json();
}
function validateState(state, physical) {
  assert.equal(state.kind, "cells"); assert.equal(state.seed, "42"); assert.equal(state.dt_seconds, 30);
  assert.equal(state.chamber_format, physical ? 2 : 1); assert.equal(state.model.spatial_positions, physical);
  assert.ok(Array.isArray(state.cells) && state.cells.length > 0 && state.cells.length <= 512);
  assert.ok(state.cells.every((cell) => typeof cell.id === "string" && /^\d+$/.test(cell.id)));
  if (physical) {
    const dimensions = state.model.dimensions_m;
    assert.ok(Array.isArray(dimensions) && dimensions.length === 3 && dimensions.every((value) => Number.isFinite(value) && value > 0));
    for (const cell of state.cells) assert.ok(Array.isArray(cell.position_m) && cell.position_m.length === 3 && cell.position_m.every((value, axis) => Number.isFinite(value) && value >= 0 && value <= dimensions[axis]), `Real API center invalid: ${cell.id}`);
  } else assert.ok(state.cells.every((cell) => !Object.hasOwn(cell, "position_m")));
  assert.equal(state.error, null);
  return state;
}
const positions = (state) => state.cells.map((cell) => ({ id: cell.id, position_m: cell.position_m }));
async function state(origin, physical = true) {
  const result = validateState(await api(origin, "/api/state"), physical);
  const identity = { seed: result.seed, dt_seconds: result.dt_seconds, config_hash: result.config_hash, world_format_version: result.world_format_version, chamber_format: result.chamber_format, model: result.model };
  if (identities.has(origin)) assert.deepEqual(identity, identities.get(origin), "Real host identity/config/model must remain unchanged"); else identities.set(origin, identity);
  return result;
}

async function stopHost() {
  if (!host) return;
  if (host.exitCode === null && host.signalCode === null) {
    const exited = once(host, "exit"); host.kill("SIGTERM");
    try { await bounded(exited, 3000, "host SIGTERM"); }
    catch { if (host.exitCode === null && host.signalCode === null) { host.kill("SIGKILL"); await bounded(exited, 3000, "host SIGKILL"); } }
  }
  host = null;
}
async function startHost(label, config, physical) {
  assert.equal(stopRequested, false, "Acceptance was interrupted before host creation");
  const reservation = createServer(); reservation.listen(0, "127.0.0.1");
  await bounded(once(reservation, "listening"), 5000, "free port reservation");
  const port = reservation.address().port;
  await bounded(new Promise((done, reject) => reservation.close((error) => error ? reject(error) : done())), 3000, "free port release");
  const dataDir = join(temporary, label); await mkdir(dataDir);
  const args = ["cells", "--seed", "42", "--port", String(port), "--data-dir", dataDir];
  if (config) args.push("--config", config);
  const origin = `http://127.0.0.1:${port}`;
  assert.equal(stopRequested, false, "Acceptance was interrupted before host spawn");
  report.hosts.push({ label, binary: hostBinary, args, origin });
  host = spawn(hostBinary, args, { cwd: root, stdio: ["ignore", "pipe", "pipe"] });
  const append = (data) => { hostLog = (hostLog + `[${label}] ${data}`).slice(-1024 * 1024); };
  host.on("error", (error) => append(error.stack || String(error))); host.stdout.on("data", append); host.stderr.on("data", append);
  await eventually(async () => { try { return !!(await state(origin, physical)); } catch { return false; } }, `${label} real release host readiness`);
  await api(origin, "/api/control", { action: "pause" });
  const paused = await state(origin, physical); assert.equal(paused.running, false);
  const served = await fetch(origin, { signal: AbortSignal.timeout(timeouts.api), cache: "no-store" });
  assert.equal(served.status, 200);
  const viewerBytes = Buffer.from(await served.arrayBuffer()), localViewer = await readFile(resolve(root, "crates/liminis/src/cell-viewer.html"));
  assert.deepEqual(viewerBytes, localViewer, "Real release host must serve the exact candidate viewer bytes");
  Object.assign(report.hosts.at(-1), { viewerBytes: viewerBytes.byteLength, viewerSha256: digest(viewerBytes), identity: { seed: paused.seed, dt_seconds: paused.dt_seconds, config_hash: paused.config_hash, world_format_version: paused.world_format_version, chamber_format: paused.chamber_format, model: paused.model }, initialTick: paused.tick });
  return { origin, paused };
}

// This instrumentation calls each original Canvas method with the original
// this/arguments and result. It records only the latest chamber paint, bounded
// by the real max_cells=512 fixture, without observing private viewer objects.
function observeCanvas() {
  const proto = CanvasRenderingContext2D.prototype;
  const paint = { serial: 0, current: null }; window.__physicalQaPaint = paint;
  for (const method of ["clearRect", "strokeRect", "arc", "fillText", "beginPath", "moveTo", "lineTo", "stroke"]) {
    const original = proto[method];
    proto[method] = function (...args) {
      if (this.canvas.id === "chamber") {
        if (method === "clearRect") paint.current = { serial: ++paint.serial, tick: document.getElementById("tick")?.textContent, rects: [], arcs: [], texts: [], strokes: [], path: [] };
        const frame = paint.current;
        if (frame) {
          if (method === "strokeRect") frame.rects.push([...args]);
          if (method === "arc") frame.arcs.push([...args]);
          if (method === "fillText") frame.texts.push([...args]);
          if (method === "beginPath") frame.path = [];
          if (method === "moveTo" || method === "lineTo") frame.path.push([method, ...args]);
          if (method === "stroke") frame.strokes.push({ path: frame.path.map((part) => [...part]), style: this.strokeStyle });
        }
      }
      return Reflect.apply(original, this, args);
    };
  }
}
async function newPage(origin, viewport = { width: 1440, height: 900 }) {
  assert.equal(stopRequested, false, "Acceptance was interrupted before page creation");
  page = await bounded(context.newPage(), timeouts.operation, "page creation");
  assert.equal(stopRequested, false, "Acceptance was interrupted after page creation");
  await page.setViewportSize(viewport);
  const traffic = { stateRequests: 0, inFlight: 0, maxInFlight: 0, posts: [], requestFailures: [] };
  const active = new Set();
  page.on("request", (request) => {
    const path = new URL(request.url()).pathname;
    if (path === "/api/state") { active.add(request); traffic.stateRequests++; traffic.inFlight = active.size; traffic.maxInFlight = Math.max(traffic.maxInFlight, active.size); }
    if (request.method() === "POST") traffic.posts.push({ url: request.url(), body: request.postData() });
  });
  const finished = (request) => { active.delete(request); traffic.inFlight = active.size; };
  page.on("requestfinished", finished); page.on("requestfailed", (request) => { finished(request); traffic.requestFailures.push({ url: request.url(), error: request.failure()?.errorText }); });
  page.on("pageerror", (error) => report.pageErrors.push(error.stack || error.message));
  page.on("console", (message) => {
    if (message.type() !== "error") return;
    const item = { text: message.text(), location: message.location() };
    if (item.location.url === `${origin}/favicon.ico` && /404/.test(item.text)) report.resourceWarnings.push(item); else report.consoleErrors.push(item);
  });
  await page.goto(origin, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => document.getElementById("live-label").textContent === "paused");
  return traffic;
}
async function waitShown(tick) {
  await page.waitForFunction((expected) => Number(document.getElementById("tick").textContent.replaceAll(",", "")) === expected && window.__physicalQaPaint.current && Number(window.__physicalQaPaint.current.tick.replaceAll(",", "")) === expected, tick);
}
async function paint() { return page.evaluate(() => JSON.parse(JSON.stringify(window.__physicalQaPaint.current))); }
async function inspector() {
  return page.locator("#cell-detail").evaluate((host) => Object.fromEntries([...host.querySelectorAll("dt")].map((term) => [term.textContent, term.nextElementSibling.textContent])));
}
async function inspectCell(cell, tick) {
  await eventually(async () => (await inspector()).id === cell.id && (await inspector())["shown snapshot tick"] === String(tick), `exact inspector ID ${cell.id} at tick ${tick}`);
  const fields = await inspector();
  for (let axis = 0; axis < 3; axis++) {
    const shown = fields[`${names[axis]} center`]; assert.match(shown, / µm$/);
    const value = Number(shown.replace(" µm", "").replaceAll(",", "")), expected = cell.position_m[axis] * 1e6;
    assert.ok(Math.abs(value - expected) <= Math.max(1e-6, Math.abs(expected) * 1e-5), `${names[axis]} inspector µm mismatch`);
  }
}
async function search(id) { await page.locator("#cell-id").fill(id); await page.locator("#cell-search button[type=submit]").click(); }
function projected(state, plane, rect, cells = state.cells) {
  const [horizontal, vertical] = axes[plane], lengths = state.model.dimensions_m;
  return cells.map((cell) => ({ id: cell.id, cell, x: rect[0] + rect[2] * (cell.position_m[horizontal] / lengths[horizontal]), y: rect[1] + rect[3] * (1 - cell.position_m[vertical] / lengths[vertical]) }));
}
function near(actual, expected, label, tolerance = 1e-6) { assert.ok(Math.abs(actual - expected) <= tolerance, `${label}: ${actual} vs ${expected}`); }
async function geometry(snapshot, plane, cells = snapshot.cells, label = plane) {
  await waitShown(snapshot.tick);
  const frame = await paint(); assert.equal(frame.rects.length, 1, "One observed physical box must be drawn");
  const rect = frame.rects[0], [horizontal, vertical] = axes[plane], lengths = snapshot.model.dimensions_m;
  assert.ok(rect.every(Number.isFinite) && rect[2] > 0 && rect[3] > 0);
  near(rect[2] / rect[3], lengths[horizontal] / lengths[vertical], "observed physical box aspect");
  const scale = rect[2] / lengths[horizontal]; near(rect[3] / lengths[vertical] / scale, 1, "equal SI scale on both axes");
  const centers = projected(snapshot, plane, rect, cells), markers = frame.arcs.filter((arc) => Math.abs(arc[2] - 3.5) < 1e-9);
  assert.equal(markers.length, cells.length, "Visible center count must match real slice centers");
  const unused = new Set(markers.map((_, index) => index));
  for (const center of centers) {
    const found = [...unused].find((index) => Math.hypot(markers[index][0] - center.x, markers[index][1] - center.y) < 1e-6);
    assert.notEqual(found, undefined, `Actual arc missing at API center for exact ID ${center.id}`); unused.delete(found);
  }
  assert.equal(await page.locator("#cell-count").textContent(), `${cells.length.toLocaleString("en-US")} / ${snapshot.cells.length.toLocaleString("en-US")}`);
  assert.equal(await page.locator("#projection-axes").textContent(), `${names[horizontal]} → · ${names[vertical]} ↑`);
  const sliceState = await page.evaluate(() => ({ enabled: document.getElementById("slice-enabled").checked, center: document.getElementById("slice-center").valueAsNumber / 1e6, thickness: document.getElementById("slice-thickness").valueAsNumber / 1e6 }));
  const hidden = axes[plane][2], lower = sliceState.enabled ? Math.max(0, sliceState.center - sliceState.thickness / 2) : 0, upper = sliceState.enabled ? Math.min(lengths[hidden], sliceState.center + sliceState.thickness / 2) : lengths[hidden];
  const rangeText = await page.locator("#projection-range").textContent(), range = rangeText.match(/^([xyz]) ∈ \[([^,]+), ([^\]]+)\] µm · (.+)$/);
  assert.ok(range, "Displayed physical slice interval must be available"); assert.equal(range[1], names[hidden]);
  near(Number(range[2]) / 1e6, lower, "displayed clipped slice lower bound", 1e-18); near(Number(range[3]) / 1e6, upper, "displayed clipped slice upper bound", 1e-18);
  assert.equal(range[4], !sliceState.enabled ? "full projection" : sliceState.thickness === 0 ? "exact center plane" : "inclusive center slice");
  for (const axis of [horizontal, vertical]) assert.ok(frame.texts.some((text) => text[0].includes(`${names[axis]} `) && text[0].includes(`${lengths[axis] * 1e6} µm`)), `Canvas axis label must show real ${names[axis]} length in µm`);
  const ruler = frame.texts.find((text) => /^[\d,.]+ µm$/.test(text[0])); assert.ok(ruler, "Observed µm ruler label missing");
  const rulerLength = Number(ruler[0].replace(" µm", "").replaceAll(",", "")) / 1e6;
  const rulerPath = frame.strokes.map((stroke) => stroke.path).find((path) => path.length === 4 && path[0][0] === "moveTo" && path.slice(1).every((part) => part[0] === "lineTo") && Math.abs(path[0][1] - rect[0]) < 1e-6 && Math.abs(path[1][2] - path[2][2]) < 1e-6);
  assert.ok(rulerPath, "Observed ruler stroke missing"); near((rulerPath[2][1] - rulerPath[1][1]) / scale, rulerLength, "ruler physical length", 1e-12);
  const pixels = await page.locator("#chamber").evaluate((canvas, points) => {
    const context = canvas.getContext("2d");
    return points.map(({ id, x, y }) => ({ id, pixel: [...context.getImageData(Math.floor(x), Math.floor(y), 1, 1).data] }));
  }, centers.map(({ id, x, y }) => ({ id, x, y })));
  for (const item of pixels) assert.ok(item.pixel[3] > 0 && Math.max(Math.abs(item.pixel[0] - 8), Math.abs(item.pixel[1] - 13), Math.abs(item.pixel[2] - 11)) > 20, `Actual marker pixel missing for ${item.id}`);
  report.geometries.push({ label, tick: snapshot.tick, plane, box: rect, scalePixelsPerMeter: scale, visibleIds: centers.map((point) => point.id), markerArcs: markers, rulerMeters: rulerLength, markerPixels: pixels, slice: { ...sliceState, lower, upper, displayedRange: rangeText } });
  return { frame, rect, centers };
}
async function pointer(center) {
  await page.locator("#chamber").scrollIntoViewIfNeeded();
  const box = await page.locator("#chamber").boundingBox();
  await page.mouse.click(box.x + center.x, box.y + center.y);
}
async function canvasDigest() {
  return page.locator("#chamber").evaluate(async (canvas) => Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data)), (value) => value.toString(16).padStart(2, "0")).join(""));
}
async function step(origin, before) {
  await page.locator("#step").click();
  let next;
  await eventually(async () => { next = await state(origin); return next.tick === before.tick + 1; }, "one paused real Step");
  assert.equal(next.running, false); await waitShown(next.tick);
  assert.equal(await page.locator("#control-error").textContent(), "");
  return next;
}
async function slice(center, thickness) {
  await page.locator("#slice-center").fill(String(center * 1e6)); await page.locator("#slice-thickness").fill(String(thickness * 1e6));
  assert.equal(await page.locator("#slice-error").isVisible(), false);
  const input = await page.evaluate(() => ({ center: document.getElementById("slice-center").valueAsNumber / 1e6, thickness: document.getElementById("slice-thickness").valueAsNumber / 1e6 }));
  return input;
}
function slicedCells(snapshot, plane, settings) {
  const hidden = axes[plane][2]; return snapshot.cells.filter((cell) => cell.position_m[hidden] >= settings.center - settings.thickness / 2 && cell.position_m[hidden] <= settings.center + settings.thickness / 2);
}
async function screenshot(filename) {
  await bounded(page.screenshot({ path: join(output, filename), fullPage: true }), timeouts.operation, "full-page screenshot");
  const bytes = await readFile(join(output, filename)); report.screenshots.push({ filename, bytes: bytes.byteLength, sha256: digest(bytes) });
}
async function responsive(snapshot, plane) {
  const controls = ["#projection-plane", "#slice-enabled", "#slice-center", "#slice-thickness", "#cell-id", "#cell-search button[type=submit]", "#play", "#step", "#speed", "#screen-fps", "#maximum", "#save", "#reset-seed", "#seed-form button"];
  report.layouts = [];
  for (const [label, viewport] of [["desktop", { width: 1440, height: 900 }], ["desktop-short", { width: 1440, height: 720 }], ["mobile", { width: 390, height: 844 }], ["mobile-narrow", { width: 320, height: 844 }]]) {
    await page.setViewportSize(viewport); await delay(100);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `Horizontal overflow: ${label}`);
    const boxes = [];
    for (const selector of controls) {
      const control = page.locator(selector); await control.scrollIntoViewIfNeeded();
      const box = await control.evaluate((element) => {
        const r = element.getBoundingClientRect(), hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height, viewportWidth: innerWidth, viewportHeight: innerHeight, unobscured: hit === element || element.contains(hit) };
      });
      assert.ok(box.width > 0 && box.height > 0 && box.left >= -1 && box.right <= box.viewportWidth + 1 && box.top >= -1 && box.bottom <= box.viewportHeight + 1 && box.unobscured, `Clipped or overlaid ${selector} at ${label}: ${JSON.stringify(box)}`); boxes.push({ selector, ...box });
    }
    await page.evaluate(() => scrollTo(0, 0));
    const settings = await page.evaluate(() => ({ center: document.getElementById("slice-center").valueAsNumber / 1e6, thickness: document.getElementById("slice-thickness").valueAsNumber / 1e6 }));
    const result = await geometry(snapshot, plane, slicedCells(snapshot, plane, settings), label);
    const bounds = await page.evaluate(() => {
      const rect = (selector) => { const r = document.querySelector(selector).getBoundingClientRect(); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom }; };
      return { canvas: rect("#chamber"), head: rect(".stage-head"), range: rect("#projection-range"), controls: rect("#projection-controls"), caption: rect(".stage-caption"), meta: rect(".stage-meta"), footer: rect(".readout") };
    });
    assert.ok(bounds.caption.top >= bounds.head.bottom && bounds.controls.top >= bounds.caption.bottom, `Physical headings, range and controls overlap: ${label}`);
    assert.ok(bounds.range.left >= -1 && bounds.range.right <= viewport.width + 1 && bounds.range.bottom <= bounds.head.bottom + 1, `Physical range is clipped: ${label}`);
    const boxTop = bounds.canvas.top + result.rect[1], rulerBottom = bounds.canvas.top + result.rect[1] + result.rect[3] + 40;
    assert.ok(boxTop - 12 >= Math.max(bounds.controls.bottom, bounds.caption.bottom), `Physical box/axis overlays caption or projection controls: ${label}`);
    assert.ok(rulerBottom <= bounds.meta.top, `Ruler overlaps metadata: ${label}`);
    assert.ok(rulerBottom <= bounds.footer.top || bounds.footer.top >= bounds.canvas.bottom, `Sticky readout overlays physical geometry: ${label}`);
    report.layouts.push({ label, viewport, controls: boxes, bounds, physicalBoxTop: boxTop, rulerBottom });
    await screenshot(`${label}.png`);
  }
  await page.setViewportSize({ width: 1440, height: 900 }); await delay(100); await page.evaluate(() => scrollTo(0, 0));
}

async function run() {
  report.sourceHead = git("rev-parse", "HEAD"); report.sourceTree = git("rev-parse", "HEAD^{tree}");
  assert.ok(process.env.LIMINIS_EXPECTED_HEAD, "Expected exact HEAD is required"); assert.equal(report.sourceHead, process.env.LIMINIS_EXPECTED_HEAD);
  if (process.env.LIMINIS_EXPECTED_TREE) assert.equal(report.sourceTree, process.env.LIMINIS_EXPECTED_TREE);
  assert.equal(git("status", "--porcelain", "--untracked-files=no"), "", "Clean tracked source is required");
  const scriptBytes = await readFile(fileURLToPath(import.meta.url));
  assert.deepEqual(scriptBytes, execFileSync("git", ["show", "HEAD:scripts/check_physical_projection_browser.mjs"], { cwd: root })); report.scriptSha256 = digest(scriptBytes);
  const binaryStat = await stat(hostBinary); assert.ok(binaryStat.isFile() && binaryStat.size < 128 * 1024 * 1024); report.hostBinary = { bytes: binaryStat.size, sha256: digest(await readFile(hostBinary)), buildProvenance: "Existing CI cargo build --locked --release -p liminis; digest is not standalone cryptographic source attestation." };
  const original = await readFile(sourceConfig), text = original.toString("utf8"), pattern = /^dimensions_m = \[1\.0e-4, 1\.0e-4, 1\.0e-4\]$/m;
  assert.equal((text.match(/^dimensions_m\s*=/gm) || []).length, 1); assert.ok(pattern.test(text), "Bounded fixture requires the unchanged declared cube source");
  const fixture = Buffer.from(text.replace(pattern, "dimensions_m = [2.0e-4, 1.0e-4, 5.0e-5]"));
  // 2 * 1 * 0.5 = 1 preserves the mathematical volume. Report the actual host
  // floats separately; a one-ULP multiplication-order difference is not a fault.
  temporary = await mkdtemp(join(tmpdir(), "liminis-projection-browser-"));
  const fixtureConfig = join(temporary, "physical-nonsquare.toml"); await writeFile(fixtureConfig, fixture); await writeFile(join(output, "physical-nonsquare.toml"), fixture);
  report.configs = { source: { filename: "configs/scenarios/cell-chamber-physical.toml", bytes: original.byteLength, sha256: digest(original), dimensions_m: [1e-4, 1e-4, 1e-4] }, nonsquare: { filename: "physical-nonsquare.toml", bytes: fixture.byteLength, sha256: digest(fixture), dimensions_m: [2e-4, 1e-4, 5e-5], onlyChange: "dimensions_m; volume unchanged" } };
  const moduleId = process.env.LIMINIS_PLAYWRIGHT_MODULE; assert.ok(moduleId, "Supply the existing official pinned Playwright module");
  const require = createRequire(import.meta.url), pkg = require(`${moduleId}/package.json`); assert.equal(pkg.name, "playwright"); assert.equal(pkg.version, "1.61.1"); report.playwrightVersion = pkg.version;
  const { chromium } = require(moduleId); browser = await chromium.launch({ headless: true, chromiumSandbox: true, args: ["--enable-automation"], timeout: timeouts.launch });
  report.browser = { engine: "Chromium", version: browser.version(), chromiumSandbox: true };
  await check("Actual CDP argv retains the protected sandbox process model", async () => {
    const session = await bounded(browser.newBrowserCDPSession(), timeouts.cdp, "CDP session");
    try {
      const { arguments: argv } = await bounded(session.send("Browser.getBrowserCommandLine"), timeouts.cdp, "actual Chromium argv");
      report.launchArgumentCheck.status = "FAIL"; report.launchArgumentCheck.arguments = argv;
      assert.ok(Array.isArray(argv) && argv.length > 0 && argv.every((value) => typeof value === "string"));
      const forbidden = new Set(["--no-sandbox", "--no-zygote-sandbox", "--disable-setuid-sandbox", "--disable-namespace-sandbox", "--disable-seccomp-filter-sandbox", "--disable-gpu-sandbox", "--allow-sandbox-debugging", "--single-process"]);
      report.launchArgumentCheck.forbiddenSwitches = argv.map((argument) => argument.split("=", 1)[0]).filter((flag) => forbidden.has(flag) || /^--disable-.*sandbox$/.test(flag));
      assert.deepEqual(report.launchArgumentCheck.forbiddenSwitches, []); report.launchArgumentCheck.status = "PASS";
    } finally { await bounded(session.detach(), timeouts.cdp, "CDP cleanup"); }
  });
  context = await bounded(browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 }), timeouts.operation, "browser context");
  context.setDefaultTimeout(timeouts.operation); context.setDefaultNavigationTimeout(timeouts.operation);
  await context.addInitScript(observeCanvas);
  await bounded(context.tracing.start({ screenshots: true, snapshots: true, sources: true }), timeouts.cdp, "trace start"); tracing = true; report.status = "RUNNING";

  let sourceReportedVolume;
  await check("Unmodified source physical scenario has real SI centers and matching cube projection", async () => {
    const { origin, paused } = await startHost("source-cube", sourceConfig, true);
    assert.deepEqual(paused.model.dimensions_m, [1e-4, 1e-4, 1e-4]); sourceReportedVolume = paused.model.volume_m3;
    await newPage(origin); await geometry(paused, "xy", paused.cells, "source-cube");
    await writeFile(join(output, "source-cube-state.json"), JSON.stringify(paused, null, 2) + "\n"); await screenshot("source-cube.png");
    await page.close(); page = null; await stopHost();
  });

  const fixtureHost = await startHost("nonsquare", fixtureConfig, true), origin = fixtureHost.origin;
  let snapshot = fixtureHost.paused; assert.deepEqual(snapshot.model.dimensions_m, [2e-4, 1e-4, 5e-5]);
  const calculatedVolume = (2e-4 * 1e-4) * 5e-5;
  assert.ok(Number.isFinite(snapshot.model.volume_m3) && snapshot.model.volume_m3 > 0 && Math.abs(snapshot.model.volume_m3 - calculatedVolume) <= 4 * Number.EPSILON * calculatedVolume);
  report.configs.volumes = { mathematicallyEqual: true, sourceHost: sourceReportedVolume, nonsquareHost: snapshot.model.volume_m3, nonsquareCalculated: calculatedVolume, byteEqualityNotAsserted: true };
  await writeFile(join(output, "nonsquare-initial-state.json"), JSON.stringify(snapshot, null, 2) + "\n");
  let historySeen; const historyCaptured = new Promise((done) => { historySeen = done; });
  const held = new Promise((done) => { releaseHistory = done; });
  let heldOnce = false; const historyErrors = [];
  await context.route(`${origin}/api/history`, async (route) => {
    if (heldOnce) return route.continue(); heldOnce = true;
    try {
      const response = await route.fetch({ timeout: timeouts.operation }), bytes = await bounded(response.body(), timeouts.operation, "actual history body");
      assert.equal(response.status(), 200); report.delayedHistory = { status: "HELD", bytes: bytes.byteLength, sha256: digest(bytes), originalStatus: response.status(), originalHeaders: response.headers(), source: "route.fetch actual host response; fulfillment uses exactly these original bytes" };
      await writeFile(join(output, "held-history.json"), bytes); historySeen();
      await bounded(held, timeouts.historyHold, "intentional real history hold");
      await route.fulfill({ response, body: bytes }); report.delayedHistory.status = "RELEASED";
    } catch (error) { historyErrors.push(error.stack || String(error)); historySeen(); await route.abort().catch(() => {}); }
  });
  const traffic = await newPage(origin); report.traffic = traffic;
  await check("State/readouts/paint/inspector commit before an unchanged delayed real history response", async () => {
    await bounded(historyCaptured, timeouts.operation, "real history capture"); assert.deepEqual(historyErrors, []); assert.equal(report.delayedHistory.status, "HELD");
    const selected = snapshot.cells[0]; await search(selected.id); await inspectCell(selected, snapshot.tick);
    const before = snapshot; snapshot = await step(origin, before);
    await geometry(snapshot, "xy", snapshot.cells, "atomic-before-history-release");
    const survivor = snapshot.cells.find((cell) => cell.id === selected.id); assert.ok(survivor); await inspectCell(survivor, snapshot.tick);
    assert.equal(await page.locator("#history-state").textContent(), "history · loading"); assert.equal(report.delayedHistory.status, "HELD");
    assert.ok(snapshot.cells.some((cell) => before.cells.some((old) => old.id === cell.id && old.position_m.some((coordinate, axis) => coordinate !== cell.position_m[axis]))), "Real Step must change at least one surviving Brownian endpoint");
    report.atomicStep = { fromTick: before.tick, toTick: snapshot.tick, selectedId: selected.id, historyReleasedAtAssertion: false };
    releaseHistory(); await eventually(async () => report.delayedHistory.status === "RELEASED", "original history response release"); assert.deepEqual(historyErrors, []);
  });
  const viewOnlyState = await state(origin), viewOnlyPosts = traffic.posts.length;
  await check("All nonsquare planes map actual SI positions to marker pixels and pointer-selected exact IDs", async () => {
    for (const plane of Object.keys(axes)) {
      await page.locator("#projection-plane").selectOption(plane);
      const result = await geometry(snapshot, plane);
      const isolated = result.centers.find((candidate) => result.centers.every((other) => other.id === candidate.id || Math.hypot(candidate.x - other.x, candidate.y - other.y) > 12));
      assert.ok(isolated, `Actual fixture must have an isolated pointer target in ${plane}`); await pointer(isolated); await inspectCell(isolated.cell, snapshot.tick);
    }
  });
  await check("Exact unknown ID is not-present, not a fabricated death", async () => {
    const unknown = "18446744073709551615"; assert.ok(!snapshot.cells.some((cell) => cell.id === unknown)); await search(unknown);
    assert.match(await page.locator("#selection-state").textContent(), /not present/i);
    const text = await page.locator("#cell-detail").textContent(); assert.ok(text.includes(unknown)); assert.match(text, /not present/i); assert.doesNotMatch(text, /\bdead\b|\bdied\b|no longer living/i);
  });
  let slicePlane;
  await check("Inclusive upper/lower boundaries, zero-width center slices and retained outside-slice selection", async () => {
    const edge = snapshot.cells.flatMap((cell) => cell.position_m.map((value, axis) => ({ cell, value, axis }))).find(({ value, axis }) => value > 0 && value < snapshot.model.dimensions_m[axis] / 2 && (value * 1e6) / 1e6 === value && (2 * value * 1e6) / 1e6 === 2 * value);
    assert.ok(edge, "A real center must be exactly representable by the µm slice input");
    slicePlane = ["yz", "xz", "xy"][edge.axis]; await page.locator("#projection-plane").selectOption(slicePlane); await page.locator("#slice-enabled").check();
    for (const [label, center, thickness] of [["inclusive-upper", 0, edge.value * 2], ["inclusive-lower", edge.value * 2, edge.value * 2], ["zero-width", edge.value, 0]]) {
      const settings = await slice(center, thickness), visible = slicedCells(snapshot, slicePlane, settings);
      const boundary = label === "inclusive-upper" ? settings.center + settings.thickness / 2 : label === "inclusive-lower" ? settings.center - settings.thickness / 2 : settings.center;
      assert.equal(boundary, edge.value); assert.ok(visible.some((cell) => cell.id === edge.cell.id)); await geometry(snapshot, slicePlane, visible, label);
    }
    const outside = snapshot.cells.find((cell) => cell.position_m[edge.axis] !== edge.value); assert.ok(outside); await search(outside.id); await inspectCell(outside, snapshot.tick);
    assert.equal(await page.locator("#selection-state").textContent(), "outside slice"); assert.match(await page.locator("#cell-detail").textContent(), /outside the current slice/);
    report.sliceBoundary = { plane: slicePlane, axis: names[edge.axis], exactCenterMeters: edge.value, boundaryId: edge.cell.id, outsideSelectedId: outside.id };
  });
  await check("Desktop, short desktop and 390/320px layouts expose controls without geometry overlays", () => responsive(snapshot, slicePlane));
  await page.locator("#slice-enabled").uncheck(); await page.locator("#projection-plane").selectOption("xy");
  await check("Paused view-only controls preserve API state and single-flight poll cadence at draw30/60", async () => {
    const before = await state(origin), postCount = traffic.posts.length, pixels = await canvasDigest(), requestCounts = [];
    for (const fps of [30, 60]) {
      await page.locator("#screen-fps").selectOption(String(fps)); await page.locator("#screen-fps").evaluate((element) => element.blur());
      const requests = traffic.stateRequests; await delay(1400); requestCounts.push(traffic.stateRequests - requests);
      assert.equal(await canvasDigest(), pixels); const heldState = await state(origin); assert.equal(heldState.tick, before.tick); assert.equal(heldState.running, false); assert.deepEqual(positions(heldState), positions(before)); assert.deepEqual(heldState.pacing, before.pacing); assert.equal(heldState.dt_seconds, before.dt_seconds);
    }
    assert.equal(traffic.posts.length, postCount); assert.equal(traffic.maxInFlight, 1);
    assert.equal(traffic.posts.length, viewOnlyPosts, "Plane/slice/ID/display controls must not POST model control");
    assert.equal(before.tick, viewOnlyState.tick); assert.deepEqual(positions(before), positions(viewOnlyState)); assert.deepEqual(before.pacing, viewOnlyState.pacing); assert.deepEqual(before.model, viewOnlyState.model);
    assert.ok(requestCounts.every((count) => count >= 1 && count <= 3) && Math.abs(requestCounts[0] - requestCounts[1]) <= 1, `Draw must not scale HTTP polling: ${requestCounts}`);
    report.pausedDraw = { pixelSha256: pixels, fps30StateRequests: requestCounts[0], fps60StateRequests: requestCounts[1], intervalMs: 1400, maxStateInFlight: traffic.maxInFlight, tick: before.tick };
  });
  await check("Bounded real paused Steps reach first actual division; parent stays absent and coincident daughters remain individually selectable", async () => {
    assert.equal(snapshot.summary.divisions, 0, "This fresh bounded fixture must not have passed its first division before stepping");
    const deadline = Date.now() + timeouts.division; let event, selectedParent, steps = 0;
    for (; steps < 128 && snapshot.tick < 128 && Date.now() < deadline && !event; steps++) {
      selectedParent = [...snapshot.cells].sort((left, right) => right.mass_mol / right.division_mass_mol - left.mass_mol / left.division_mass_mol || (BigInt(left.id) < BigInt(right.id) ? -1 : 1))[0];
      await search(selectedParent.id); await inspectCell(selectedParent, snapshot.tick);
      snapshot = await step(origin, snapshot); event = snapshot.events.find((entry) => entry.kind === "division" && entry.tick === snapshot.tick);
    }
    assert.ok(event, "First actual division must be reached within 128 real Steps and the deadline");
    assert.ok(snapshot.tick <= 128 && Date.now() <= deadline, "Actual first division must stay within the tick and wall deadline");
    const selectedEvent = snapshot.events.find((entry) => entry.kind === "division" && entry.tick === snapshot.tick && entry.parent_id === selectedParent.id);
    assert.ok(selectedEvent, "Selected greatest-mass founder must be among first actual division parents in this unchanged biology fixture"); event = selectedEvent;
    assert.ok(!snapshot.cells.some((cell) => cell.id === event.parent_id));
    const absentText = await page.locator("#cell-detail").textContent(); assert.match(absentText, new RegExp(`\\b${event.parent_id}\\b`)); assert.doesNotMatch(absentText, /\bdead\b|\bdied\b/); assert.match(await page.locator("#selection-state").textContent(), /no longer living|not present/i);
    const daughters = event.children.map((id) => snapshot.cells.find((cell) => cell.id === id)); assert.equal(daughters.length, 2); assert.ok(daughters.every(Boolean)); assert.deepEqual(daughters[0].position_m, daughters[1].position_m);
    const result = await geometry(snapshot, "xy", snapshot.cells, "first-real-division"); const center = result.centers.find((point) => point.id === daughters[0].id); await pointer(center);
    assert.equal(await page.locator("#hit-candidates").isVisible(), true);
    const candidateIds = await page.locator("#candidate-list option").evaluateAll((options) => options.map((option) => option.value));
    assert.ok(daughters.every((cell) => candidateIds.includes(cell.id)));
    const ordered = [...daughters].sort((left, right) => BigInt(left.id) < BigInt(right.id) ? -1 : 1); assert.ok(candidateIds.indexOf(ordered[0].id) < candidateIds.indexOf(ordered[1].id), "Coincident exact IDs must retain integer ordering");
    for (const daughter of daughters) { await page.locator("#candidate-list").selectOption(daughter.id); await inspectCell(daughter, snapshot.tick); }
    report.division = { tick: snapshot.tick, realStepCount: steps, tickLimit: 128, deadlineMs: timeouts.division, parentId: event.parent_id, daughterIds: event.children, position_m: daughters[0].position_m, candidateIds, parentAbsenceIsNotDeath: true };
    await writeFile(join(output, "first-division-state.json"), JSON.stringify(snapshot, null, 2) + "\n"); await screenshot("coincident-daughters.png");
    await page.locator("#projection-plane").selectOption("xz"); assert.equal(await page.locator("#hit-candidates").isVisible(), false);
    const otherPlane = await geometry(snapshot, "xz"); await pointer(otherPlane.centers.find((point) => point.id === daughters[0].id)); assert.equal(await page.locator("#hit-candidates").isVisible(), true);
    await page.locator("#slice-enabled").check(); assert.equal(await page.locator("#hit-candidates").isVisible(), false); await page.locator("#slice-enabled").uncheck();
    await pointer((await geometry(snapshot, "xz")).centers.find((point) => point.id === daughters[0].id)); assert.equal(await page.locator("#hit-candidates").isVisible(), true);
    snapshot = await step(origin, snapshot); assert.equal(await page.locator("#hit-candidates").isVisible(), false); await geometry(snapshot, "xz", snapshot.cells, "stale-candidates-after-real-step");
  });
  assert.equal(traffic.maxInFlight, 1); assert.deepEqual(historyErrors, []); assert.deepEqual(traffic.requestFailures, []);
  await page.close(); page = null; await stopHost();
  await check("Real legacy chamber1 retains schematic inventory without physical controls or coordinates", async () => {
    const { origin: legacyOrigin, paused } = await startHost("legacy", null, false); await newPage(legacyOrigin); await waitShown(paused.tick);
    assert.equal(await page.locator("#projection-controls").isVisible(), false); assert.equal(await page.locator("#physical-state-note").isVisible(), false); assert.match(await page.locator("#schematic-caption").textContent(), /does not represent physical coordinates/);
    assert.equal((await paint()).rects.length, 0); await screenshot("legacy-chamber1.png"); await page.close(); page = null; await stopHost();
  });
  assert.deepEqual(report.pageErrors, []); assert.deepEqual(report.consoleErrors, []);
  assert.equal(git("rev-parse", "HEAD"), report.sourceHead); assert.equal(git("rev-parse", "HEAD^{tree}"), report.sourceTree); assert.equal(git("status", "--porcelain", "--untracked-files=no"), "");
  assert.deepEqual(await readFile(sourceConfig), original); assert.equal(stopRequested, false);
  report.status = "PASS";
}

await mkdir(output, { recursive: true });
let running;
try {
  for (const filename of ["trace.zip", "source-cube.png", "desktop.png", "desktop-short.png", "mobile.png", "mobile-narrow.png", "coincident-daughters.png", "legacy-chamber1.png", "failure.png"]) await rm(join(output, filename), { force: true });
  running = run(); await bounded(running, timeouts.run, "complete bounded physical projection QA");
} catch (error) {
  fail(error);
  // A timed-out action is allowed to observe stopRequested and settle before
  // teardown/report publication. Resource creation checks also stop late work.
  if (running) await bounded(running.catch(() => {}), timeouts.launch + timeouts.operation, "interrupted QA settlement").catch((settlementError) => fail(settlementError, "interrupted QA settlement"));
}
finally {
  releaseHistory?.();
  async function cleanup(name, action, timeout = timeouts.cleanup) {
    try { return { ok: true, value: await bounded(action(), timeout, name) }; }
    catch (error) { report.cleanupErrors.push({ name, error: error.stack || String(error) }); fail(error, name); return { ok: false }; }
  }
  if (report.status !== "PASS" && page && !page.isClosed()) await cleanup("failure screenshot", () => screenshot("failure.png"), 4000);
  if (tracing && context) {
    const saved = await cleanup("trace publication", async () => { await context.tracing.stop({ path: join(output, "trace.zip") }); const bytes = await readFile(join(output, "trace.zip")); assert.ok(bytes.byteLength > 0); return { bytes: bytes.byteLength, sha256: digest(bytes) }; }, timeouts.trace);
    Object.assign(report.trace, saved.ok ? { status: "PASS", ...saved.value } : { status: "FAIL" });
  }
  if (context) await cleanup("browser context cleanup", () => context.close());
  if (browser) await cleanup("browser cleanup", () => browser.close());
  await cleanup("real host cleanup", stopHost, 7000);
  if (temporary) await cleanup("temporary fixture/data cleanup", () => rm(temporary, { recursive: true, force: true }));
  if (report.status === "PASS" && (report.trace.status !== "PASS" || report.launchArgumentCheck.status !== "PASS" || report.cleanupErrors.length || !report.division || report.screenshots.length !== 7)) fail(new Error("Required final physical projection evidence is incomplete"), "final gate");
  report.finishedAt = new Date().toISOString();
  await writeFile(join(output, "host.log"), hostLog); await writeFile(join(output, "report.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify({ status: report.status, sourceHead: report.sourceHead, checks: report.checks.length, output }));
  if (report.status !== "PASS") process.exitCode = 1;
}
