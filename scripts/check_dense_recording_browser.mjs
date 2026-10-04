// Protected REC2 browser acceptance. This file is not browser-run evidence.
// Use the existing officially installed Playwright 1.61.1 on its CI runner.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFile, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir, rm, stat } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, extname, resolve, sep, join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const root = resolve(dirname(fileURLToPath(import.meta.url)), ".."), site = join(root, "site"), output = join(root, "target/qa/dense-recording-browser");
const tiny = join(root, "scripts/fixtures/dense-recording/smoke-100"), partial = join(root, "scripts/fixtures/dense-recording/pilot-100000");
const digest = (value) => createHash("sha256").update(value).digest("hex"), execute = promisify(execFile);
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
const timeouts = { run: 240_000, operation: 15_000, launch: 20_000, cdp: 5000, hold: 20_000, trace: 15_000, cleanup: 5000 };
const controls = ["play", "previous", "next", "scrub", "speed", "speed-preset", "draw-fps"];
const pins = [
  ["index.json", 1798, "603b6f3b62448c2dc6cd6abe976c77ffbb9aad83f9c143813785137e4f71a370"],
  ["horizon-10000.json", 18449, "a08ecd054f85fbeb06c52b868ef604d39c1d8563231cec76dc33420a5e611c8d"],
  ["horizon-100000.json", 124764, "7da1efd07473e15af331c060341fcb75fdcc911e60a5be9347423f9791442bcd"],
  ["chunk-0000000-0000255.jsonl.gz", 312818, "6328ef9d314ba73acfb72e4b9ee710259255a3734a1461b5d5cf631ca71d0968"],
  ["chunk-0000768-0000992.jsonl.gz", 1046390, "8acb535328f123b618f0d9d86044a8d3a0c0cac66c85143b0bcfa5aeed468f4c"],
];
const report = {
  status: "NOT_RUN", startedAt: new Date().toISOString(), timeouts, checks: [], screenshots: [], sources: [], http: [], pageErrors: [], consoleErrors: [], unhandled: [], requestFailures: [], cleanupErrors: [],
  nodeVersion: process.version, runner: { os: process.env.RUNNER_OS || process.platform, arch: process.env.RUNNER_ARCH || process.arch, imageOS: process.env.ImageOS || null },
  workflowRun: process.env.GITHUB_RUN_ID ? `${process.env.GITHUB_SERVER_URL || "https://github.com"}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}` : null,
  workflowAttempt: process.env.GITHUB_RUN_ATTEMPT || null,
  launchArgumentCheck: { status: "NOT_RUN", method: "CDP Browser.getBrowserCommandLine" }, trace: { status: "NOT_RUN", filename: "trace.zip" },
  decoder: { status: "NOT_RUN" }, ui: { status: "NOT_RUN", requiredHorizons: [10_000, 100_000, 1_000_000] },
  limits: ["The 100k fixture is PARTIAL original metadata plus only two saved chunks, never a complete 100k recording.", "Chromium/Linux and four viewport sizes; no achieved-FPS, full accessibility, native background or universal performance claim.", "Elapsed time, a 10ms timer gap and heap observations describe this bounded workload/runner, not peak heap or every device.", "The actual final catalog/UI/data are mandatory; their absence fails this gate, without substituting a catalog or frames.", "No browser biology, interpolated states, full million-frame array or full 3GB browser download. High rates may skip displayed states."],
};
let server, browser, context, page, tracing = false, stopping = false, origin, running;
let servedBytes = 0; const releases = new Set(), references = new Map();
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
function fail(error, stage = "run") { stopping = true; report.status = "FAIL"; process.exitCode = 1; (report.failures ||= []).push({ stage, error: error.stack || String(error) }); }
process.on("unhandledRejection", (error) => { report.unhandled.push(error?.stack || String(error)); fail(error instanceof Error ? error : new Error(String(error)), "Node unhandled rejection"); });
async function bounded(operation, ms, label) { let timer; try { return await Promise.race([operation, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`Timed out: ${label}`)), ms); })]); } finally { clearTimeout(timer); } }
async function check(name, action) { const started = Date.now(); try { assert.equal(stopping, false); await action(); report.checks.push({ name, status: "PASS", elapsedMs: Date.now() - started }); } catch (error) { report.checks.push({ name, status: "FAIL", elapsedMs: Date.now() - started, error: error.stack || String(error) }); throw error; } }
async function eventually(predicate, label) { const end = Date.now() + timeouts.operation; while (Date.now() < end) { assert.equal(stopping, false); if (await predicate()) return; await sleep(40); } throw new Error(`Timed out: ${label}`); }
async function source(filename) {
  const value = await readFile(join(root, filename)); assert.deepEqual(value, execFileSync("git", ["show", `HEAD:${filename}`], { cwd: root, maxBuffer: 20 * 1024 * 1024 }));
  report.sources.push({ filename, bytes: value.length, sha256: digest(value) }); return value;
}
function localSitePath(filename) { assert.equal(typeof filename, "string"); assert.ok(/^\.\/data\/[a-z0-9./-]+$/.test(filename) && !filename.split("/").includes("..")); const path = resolve(site, filename); assert.ok(path.startsWith(site + sep)); return path; }
async function reference(manifest, tick) {
  assert.equal(stopping, false);
  const key = `${manifest}:${tick}`; if (references.has(key)) return references.get(key);
  const result = await execute("python3", ["-B", join(root, "scripts/dense-recording-decode.py"), manifest, "--first-tick", String(tick), "--last-tick", String(tick)], { cwd: root, encoding: "utf8", timeout: timeouts.operation, maxBuffer: 2 * 1024 * 1024 });
  assert.equal(result.stderr, ""); const lines = result.stdout.trimEnd().split("\n"); assert.equal(lines.length, 1); const value = JSON.parse(lines[0]); assert.equal(value.frame.tick, tick);
  references.set(key, value); assert.ok(references.size <= 128, "Bounded Node reference-state cache"); (report.references ||= []).push({ manifest: manifest.slice(root.length + 1), tick, method: "independent Python reference decoder", sha256: digest(Buffer.from(result.stdout)) }); return value;
}
async function tinyReferences() {
  const manifest = join(tiny, "horizon-100.json"), result = await execute("python3", ["-B", join(root, "scripts/dense-recording-decode.py"), manifest, "--first-tick", "0", "--last-tick", "100"], { cwd: root, encoding: "utf8", timeout: timeouts.operation, maxBuffer: 4 * 1024 * 1024 });
  assert.equal(result.stderr, ""); const lines = result.stdout.trimEnd().split("\n"); assert.equal(lines.length, 101);
  lines.forEach((line, tick) => { const value = JSON.parse(line); assert.equal(value.frame.tick, tick); references.set(`${manifest}:${tick}`, value); });
  (report.references ||= []).push({ manifest: manifest.slice(root.length + 1), range: [0, 100], frames: 101, method: "independent Python reference decoder; bounded tiny fixture only", sha256: digest(Buffer.from(result.stdout)) });
}
async function serve() {
  server = createServer(async (request, response) => {
    let item;
    try {
      const url = new URL(request.url, "http://127.0.0.1"), path = decodeURIComponent(url.pathname);
      item = { method: request.method, path, query: url.search, status: null, bytesWritten: 0 }; report.http.push(item); assert.ok(report.http.length <= 1500); assert.equal(request.method, "GET");
      if (path === "/favicon.ico") { item.status = 204; response.writeHead(204); response.end(); return; }
      if (path === "/__dense_qa__/harness.html") { const value = Buffer.from("<!doctype html><meta charset=utf-8><title>Real dense decoder QA</title><p>Bounded decoder QA of saved Rust observations. Partial fixtures are not complete recordings.</p>"); item.status = 200; response.writeHead(200, { "content-type": "text/html", "content-length": value.length, "cache-control": "no-store" }); item.bytesWritten = value.length; response.end(value); return; }
      let filename, slow = false;
      const mount = path.match(/^\/__dense_qa__\/(smoke|partial|stream|late)\/([a-z0-9.-]+)$/);
      if (mount) {
        const [_, mode, name] = mount, directory = ["smoke", "stream"].includes(mode) ? tiny : partial;
        const names = directory === tiny ? ["index.json", "horizon-100.json", "chunk-0000000-0000100.jsonl.gz"] : pins.map(([name]) => name);
        assert.ok(names.includes(name)); filename = join(directory, name); slow = (mode === "stream" && name === "index.json") || (mode === "late" && name === "chunk-0000000-0000255.jsonl.gz");
      } else { filename = resolve(site, `.${path === "/" ? "/index.html" : path}`); assert.ok(filename.startsWith(site + sep)); }
      const info = await stat(filename); assert.ok(info.isFile()); const extension = extname(filename), maximum = extension === ".gz" ? 1024 * 1024 : extension === ".json" ? 16 * 1024 * 1024 : 2 * 1024 * 1024; assert.ok(info.size <= maximum);
      const value = await readFile(filename); servedBytes += value.length; assert.ok(servedBytes <= 96 * 1024 * 1024, "Bounded QA HTTP byte budget exceeded");
      Object.assign(item, { status: 200, bytes: value.length, sha256: digest(value), filename: filename.slice(root.length + 1), contentEncoding: null });
      const mime = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".json": "application/json", ".gz": "application/gzip" };
      response.writeHead(200, { "content-type": mime[extension] || "application/octet-stream", "content-length": value.length, "cache-control": "no-store" });
      if (!slow) { item.bytesWritten = value.length; response.end(value); return; }
      item.delayed = "HELD"; item.bytesWritten = 16; response.write(value.subarray(0, 16));
      let release; const gate = new Promise((done) => { release = done; }); releases.add(release);
      response.once("close", () => { item.delayed = response.writableFinished ? "RELEASED" : "CANCELED"; release(); });
      try { await bounded(gate, timeouts.hold, "unchanged original HTTP body hold"); } finally { releases.delete(release); }
      if (!response.destroyed && !response.writableEnded) { item.bytesWritten = value.length; response.end(value.subarray(16)); item.delayed = "RELEASED"; }
    } catch (error) { if (item) Object.assign(item, { status: item.status || 404, error: error.message }); if (!response.headersSent) response.writeHead(404); response.end(); }
  });
  await bounded(new Promise((done, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", done); }), 5000, "loopback server start");
  origin = `http://127.0.0.1:${server.address().port}`; report.origin = origin;
}
function observePage() {
  window.__denseQaErrors = { unhandled: [], errors: [], timer: { intervalMs: 10, samples: 0, maxGapMs: 0 } };
  addEventListener("unhandledrejection", (event) => window.__denseQaErrors.unhandled.push(String(event.reason?.stack || event.reason)));
  addEventListener("error", (event) => window.__denseQaErrors.errors.push(event.message));
  let last = performance.now(); setInterval(() => { const now = performance.now(), sample = window.__denseQaErrors.timer; sample.samples++; sample.maxGapMs = Math.max(sample.maxGapMs, now - last); last = now; }, 10);
}
async function fresh(viewport = { width: 1440, height: 900 }) {
  assert.equal(stopping, false); page = await bounded(context.newPage(), timeouts.operation, "page creation"); await page.setViewportSize(viewport);
  page.on("pageerror", (error) => report.pageErrors.push(error.stack || error.message));
  page.on("console", (message) => { if (message.type() !== "error") return; const item = { text: message.text(), location: message.location() }; if (/net::ERR_ABORTED/.test(item.text) && /\/__dense_qa__\/(stream|late)\//.test(item.location.url)) return; report.consoleErrors.push(item); });
  page.on("requestfailed", (request) => { const intentional = /\/__dense_qa__\/(stream|late)\//.test(request.url()) && /ERR_ABORTED|aborted/i.test(request.failure()?.errorText || ""); report.requestFailures.push({ url: request.url(), error: request.failure()?.errorText, intentional }); });
  await page.addInitScript(observePage); return page;
}
async function evaluate(...args) { return bounded(page.evaluate(...args), timeouts.operation, "browser module operation"); }
async function pageEnd() { await page.waitForLoadState("networkidle"); await sleep(50); const observed = await evaluate(() => window.__denseQaErrors); (report.pageMeasurements ||= []).push(observed); assert.deepEqual(observed.unhandled, []); assert.deepEqual(observed.errors, []); await page.close(); page = null; }
async function screenshot(filename) { await bounded(page.screenshot({ path: join(output, filename), fullPage: true }), timeouts.operation, "screenshot"); const value = await readFile(join(output, filename)); report.screenshots.push({ filename, bytes: value.length, sha256: digest(value) }); }
const shown = async (id) => Number((await page.locator(`#${id}`).textContent()).replaceAll(",", ""));
async function waitTick(tick) { await page.waitForFunction((expected) => Number(document.getElementById("tick").textContent.replaceAll(",", "")) === expected && document.getElementById("workspace").getAttribute("aria-busy") === "false", tick); }
async function seekUi(tick) { await page.locator("#scrub").evaluate((input, seconds) => { input.value = String(seconds); input.dispatchEvent(new Event("input", { bubbles: true })); }, tick * 30); await waitTick(tick); }
async function pixelDigest() { return page.locator("#chamber").evaluate(async (canvas) => Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data)), (part) => part.toString(16).padStart(2, "0")).join("")); }
async function stateUi(expected) {
  assert.equal(await shown("tick"), expected.tick); assert.equal(await shown("time"), expected.sim_time);
  for (const [element, key] of [["living", "living_cells"], ["births", "births"], ["deaths", "deaths"], ["divisions", "divisions"], ["generation", "generation_max"]]) assert.equal(await shown(element), expected.summary[key]);
  assert.equal(await page.locator("#ledger").textContent(), expected.residual ? "M 0 · E 0" : "not checked");
  assert.match(await page.locator("#chamber").getAttribute("aria-label"), new RegExp(`at recorded tick ${expected.tick}\\.`));
  const nonblank = await page.locator("#chamber").evaluate((canvas) => { const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data; let count = 0; for (let i = 0; i < data.length; i += 4) if (data[i + 3] && Math.max(Math.abs(data[i] - 8), Math.abs(data[i + 1] - 13), Math.abs(data[i + 2] - 11)) > 8) count++; return count; });
  if (expected.cells.length) assert.ok(nonblank > 10, "Actual saved-cell canvas pixels missing");
  assert.equal(await page.locator("#error").isVisible(), false); return { pixelSha256: await pixelDigest(), nonblankPixels: nonblank };
}
async function inspectorUi(expected) {
  await page.locator("#chamber").focus(); await page.locator("#chamber").press("ArrowRight");
  const fields = await page.locator("#cell-detail > dl.kv").evaluate((list) => Object.fromEntries([...list.querySelectorAll("dt")].map((term) => [term.textContent, term.nextElementSibling.textContent])));
  const cell = expected.cells.find((entry) => entry.id === fields.id); assert.ok(cell, "Inspector exact ID must belong to the actual shown inventory"); assert.equal(fields["mass units"], cell.mass_units); assert.equal(fields["energy units"], cell.energy_units); assert.equal(fields.parent, cell.parent_id ?? "founder"); assert.equal(fields["birth tick"], String(cell.birth_tick)); return fields;
}
async function layout(filename, viewport) {
  await page.setViewportSize(viewport); await sleep(100); assert.ok(await evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), "Horizontal overflow");
  const observations = [];
  for (const id of [...controls, "duration", "sample-cadence", "clock-help"]) { const element = page.locator(`#${id}`); await element.scrollIntoViewIfNeeded(); const rect = await element.evaluate((node) => { const r = node.getBoundingClientRect(), hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2); return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height, viewportWidth: innerWidth, viewportHeight: innerHeight, unobscured: hit === node || node.contains(hit) }; }); assert.ok(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= rect.viewportWidth + 1 && rect.top >= -1 && rect.bottom <= rect.viewportHeight + 1 && (!controls.includes(id) || rect.unobscured), `Clipped or overlaid #${id}: ${JSON.stringify(rect)}`); observations.push({ id, ...rect }); }
  await evaluate(() => scrollTo(0, 0)); (report.layouts ||= []).push({ filename, viewport, controls: observations }); await screenshot(filename);
}

async function standalone() {
  await fresh(); await page.goto(`${origin}/__dense_qa__/harness.html`);
  await evaluate(async ({ origin, sha }) => { window.__denseQaModule = await import(`${origin}/dense-recording.mjs`); window.__denseQaLoader = await window.__denseQaModule.createDenseRecordingLoader({ indexUrl: `${origin}/__dense_qa__/smoke/index.json`, indexSha256: sha, manifestPath: "horizon-100.json", baseUrl: origin, prefetch: false }); }, { origin, sha: "d3d102bcb4233900416ba0532c9cdd83b41b353963b15c47a9c66e4852b260aa" });
  await check("Browser native gzip and retained-value seeks match all 101 original real frames", async () => {
    await tinyReferences();
    for (let tick = 0; tick <= 100; tick++) { const expected = await reference(join(tiny, "horizon-100.json"), tick), actual = await evaluate((tick) => window.__denseQaLoader.seek(tick), tick); assert.deepEqual(actual, expected); }
    const archive = JSON.parse(await readFile(join(site, "data/cell-chamber-seed-42.json"), "utf8"));
    for (const tick of [0, 50, 100]) assert.deepEqual((await evaluate((tick) => window.__denseQaLoader.seek(tick), tick)).frame, archive.frames.find((sample) => sample.tick === tick));
    report.decoder.tiny = { matchedRealFrames: 101, archivalControlTicks: [0, 50, 100], indexSha256: "d3d102bcb4233900416ba0532c9cdd83b41b353963b15c47a9c66e4852b260aa", stats: await evaluate(() => window.__denseQaLoader.stats) };
    assert.equal(report.decoder.tiny.stats.maxRetainedChunks, 1); assert.equal(report.decoder.tiny.stats.retainedFrames, 1);
  });
  await check("Browser SHA/CRC/truncated/trailing-member corruptions refuse before frame emission", async () => {
    const result = await evaluate(async (origin) => {
      const manifest = await (await fetch(`${origin}/__dense_qa__/smoke/horizon-100.json`)).json(), response = await fetch(`${origin}/__dense_qa__/smoke/${manifest.chunks[0].path}`);
      if (response.headers.get("content-encoding") !== null) throw new Error("Stored gzip must be raw HTTP bytes, without Content-Encoding");
      const bytes = new Uint8Array(await response.arrayBuffer()), hash = async (value) => Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", value)), (part) => part.toString(16).padStart(2, "0")).join("");
      const results = [];
      for (const name of ["sha", "crc", "truncated", "trailing-member"]) {
        let corrupted = bytes.slice(); const descriptor = { ...manifest.chunks[0] };
        if (name === "sha") descriptor.gzip_sha256 = "0".repeat(64);
        else { if (name === "crc") corrupted[corrupted.length - 8] ^= 1; if (name === "truncated") corrupted = corrupted.subarray(0, corrupted.length - 1); if (name === "trailing-member") { const doubled = new Uint8Array(corrupted.length * 2); doubled.set(corrupted); doubled.set(corrupted, corrupted.length); corrupted = doubled; } descriptor.gzip_bytes = corrupted.length; descriptor.gzip_sha256 = await hash(corrupted); }
        try { await window.__denseQaModule.decodeDenseChunk(corrupted, descriptor, manifest.genomes); throw new Error(`Corruption admitted: ${name}`); } catch (error) { if (error.message.startsWith("Corruption admitted:")) throw error; results.push({ name, refused: true, error: error.message }); }
      }
      return { rawGzipBytes: bytes.length, rawGzipSha256: await hash(bytes), contentEncoding: null, results };
    }, origin);
    assert.equal(result.rawGzipSha256, "4eb60f6866110a40314247613779e5a38308b2d240836f79040bf548525e9821"); report.decoder.corruption = result;
  });
  await evaluate(() => window.__denseQaLoader.close());
  await check("Actual first and largest saved 100k chunks seek independently with a one-payload cache", async () => {
    await evaluate(async (origin) => { window.__denseQaLoader = await window.__denseQaModule.createDenseRecordingLoader({ indexUrl: `${origin}/__dense_qa__/partial/index.json`, indexSha256: "603b6f3b62448c2dc6cd6abe976c77ffbb9aad83f9c143813785137e4f71a370", manifestPath: "horizon-100000.json", baseUrl: origin, prefetch: false, fetcher: async (url, options) => { const response = await fetch(url, options); if (new URL(url).pathname.endsWith(".gz") && response.headers.get("content-encoding") !== null) throw new Error("Stored gzip cannot use transparent HTTP content encoding"); return response; } }); }, origin);
    const started = Date.now();
    for (const tick of [0, 50, 100, 768, 992, 768]) assert.deepEqual(await evaluate((tick) => window.__denseQaLoader.seek(tick), tick), await reference(join(partial, "horizon-100000.json"), tick));
    const cdp = await bounded(context.newCDPSession(page), timeouts.cdp, "heap CDP session"); let heap; try { heap = await bounded(cdp.send("Runtime.getHeapUsage"), timeouts.cdp, "observed browser heap"); } finally { await bounded(cdp.detach(), timeouts.cdp, "heap CDP detach"); }
    report.decoder.largest = { elapsedMs: Date.now() - started, gzipBytes: 1046390, decodedBytes: 4135813, records: 225, controlTicks: [0, 50, 100, 768, 992], heapObservation: heap, heapQualification: "One CDP snapshot, not peak or universal device memory", timer: await evaluate(() => window.__denseQaErrors.timer), stats: await evaluate(() => window.__denseQaLoader.stats) };
    assert.equal(report.decoder.largest.stats.maxRetainedChunks, 1); assert.equal(report.decoder.largest.stats.retainedFrames, 1); await evaluate(() => window.__denseQaLoader.close());
  });
  await check("Mid-body real fetch abort is caught with no browser unhandled rejection", async () => {
    const result = await evaluate(async (origin) => { const controller = new AbortController(); let headers; const received = new Promise((done) => { headers = done; }); const pending = window.__denseQaModule.createDenseRecordingLoader({ indexUrl: `${origin}/__dense_qa__/stream/index.json`, indexSha256: "d3d102bcb4233900416ba0532c9cdd83b41b353963b15c47a9c66e4852b260aa", manifestPath: "horizon-100.json", baseUrl: origin, signal: controller.signal, fetcher: async (url, options) => { const response = await fetch(url, options); headers(); return response; } }); const caught = pending.then(() => "unexpected success", (error) => error.name); await received; await new Promise((done) => setTimeout(done, 100)); controller.abort(); const error = await caught; await new Promise((done) => setTimeout(done, 30)); return { error, unhandled: window.__denseQaErrors.unhandled }; }, origin);
    assert.equal(result.error, "AbortError"); assert.deepEqual(result.unhandled, []); assert.ok(report.http.some((item) => item.path === "/__dense_qa__/stream/index.json" && item.bytesWritten === 16)); report.decoder.bodyAbort = result;
  });
  await check("Superseded and closed real loader fetches cannot commit a late frame or failure", async () => {
    await evaluate(async (origin) => { window.__denseQaLoader = await window.__denseQaModule.createDenseRecordingLoader({ indexUrl: `${origin}/__dense_qa__/late/index.json`, indexSha256: "603b6f3b62448c2dc6cd6abe976c77ffbb9aad83f9c143813785137e4f71a370", manifestPath: "horizon-100000.json", baseUrl: origin, prefetch: false }); window.__denseQaOld = window.__denseQaLoader.seek(0).then(() => "unexpected success", (error) => error.name); }, origin);
    await eventually(() => report.http.some((item) => item.path === "/__dense_qa__/late/chunk-0000000-0000255.jsonl.gz" && item.delayed === "HELD"), "real delayed chunk");
    assert.deepEqual(await evaluate(() => window.__denseQaLoader.seek(992)), await reference(join(partial, "horizon-100000.json"), 992));
    assert.equal(await evaluate(() => window.__denseQaOld), "AbortError"); for (const release of [...releases]) release(); await sleep(100);
    assert.equal(await evaluate(() => window.__denseQaLoader.current.frame.tick), 992); assert.equal(await evaluate(() => window.__denseQaLoader.stats.failed), false);
    const closeBaseline = report.http.length; await evaluate(() => { window.__denseQaOld = window.__denseQaLoader.seek(0).then(() => "unexpected success", (error) => error.name); });
    await eventually(() => report.http.slice(closeBaseline).some((item) => item.path === "/__dense_qa__/late/chunk-0000000-0000255.jsonl.gz" && item.delayed === "HELD"), "close pending original fetch");
    await evaluate(() => window.__denseQaLoader.close()); assert.equal(await evaluate(() => window.__denseQaOld), "AbortError"); for (const release of [...releases]) release();
    assert.equal(await evaluate(() => window.__denseQaLoader.current), null); assert.equal(await evaluate(() => window.__denseQaLoader.stats.retainedChunks), 0); await sleep(100);
  });
  report.decoder.status = "PASS"; await pageEnd();
}

async function actualUi() {
  const catalogBytes = await source("site/data/catalog.json"), catalog = JSON.parse(catalogBytes);
  const entries = report.ui.requiredHorizons.map((horizon) => catalog.entries.find((entry) => entry.experiment.steps === horizon));
  if (entries.some((entry) => !entry?.dense)) { report.ui.reason = "Final catalog must contain all three real dense attachments; no substitute catalog is supplied"; throw new Error(report.ui.reason); }
  const cases = [];
  for (const entry of entries) {
    assert.equal(entry.dense.manifest, `horizon-${entry.experiment.steps}.json`); assert.equal(typeof entry.dense.index_sha256, "string"); assert.match(entry.dense.index_sha256, /^[0-9a-f]{64}$/);
    assert.equal(typeof entry.dense.publication_sha256, "string"); assert.match(entry.dense.publication_sha256, /^[0-9a-f]{64}$/); const publicationBytes = await readFile(localSitePath(entry.dense.publication)); assert.equal(digest(publicationBytes), entry.dense.publication_sha256);
    const indexPath = localSitePath(entry.dense.index), indexBytes = await readFile(indexPath); assert.equal(digest(indexBytes), entry.dense.index_sha256);
    const index = JSON.parse(indexBytes), manifestEntry = index.manifests.find((item) => item.path === entry.dense.manifest); assert.ok(manifestEntry);
    const manifestPath = join(dirname(indexPath), manifestEntry.path), manifestBytes = await readFile(manifestPath); assert.equal(digest(manifestBytes), manifestEntry.sha256); assert.equal(manifestBytes.length, manifestEntry.bytes);
    const manifest = JSON.parse(manifestBytes); assert.equal(manifest.identity.world_format_version, 30); assert.equal(manifest.experiment.sample_every, 1);
    const archiveBytes = await readFile(localSitePath(entry.recording)); assert.equal(digest(archiveBytes), entry.sha256); assert.equal(archiveBytes.length, entry.bytes); const archive = JSON.parse(archiveBytes);
    cases.push({ entry, manifest, manifestPath, archive, indexPath });
  }
  report.ui.attachments = cases.map(({ entry, manifest }) => ({ id: entry.id, horizon: entry.experiment.steps, indexSha256: entry.dense.index_sha256, publicationSha256: entry.dense.publication_sha256, identity: manifest.identity, frames: manifest.experiment.frames, chunks: manifest.chunks.length }));
  const first = cases[0]; await fresh(); let releaseUi, heldUi, delayedOnce = false;
  const captured = new Promise((done) => { heldUi = done; }), gate = new Promise((done) => { releaseUi = done; }); releases.add(releaseUi);
  const descriptor = first.manifest.chunks[1], chunkUrl = new URL(descriptor.path, new URL(first.entry.dense.index, origin + "/")).href;
  await page.route(chunkUrl, async (route) => {
    if (delayedOnce) return route.continue(); delayedOnce = true;
    try { const response = await route.fetch({ timeout: timeouts.operation }), value = await response.body(); assert.equal(response.status(), 200); assert.equal(response.headers()["content-encoding"], undefined); assert.equal(value.length, descriptor.gzip_bytes); assert.equal(digest(value), descriptor.gzip_sha256); report.ui.delayedOriginal = { url: chunkUrl, status: "HELD", bytes: value.length, sha256: digest(value), fabricatedBytes: false }; heldUi(); await bounded(gate, timeouts.hold, "actual UI original chunk hold"); await route.fulfill({ response, body: value }); report.ui.delayedOriginal.status = "RELEASED"; } catch (error) { heldUi(); fail(error, "unchanged UI response delay"); await route.abort().catch(() => {}); }
  });
  await check("Actual dense 10k UI commits one-tick Next atomically after unchanged buffering", async () => {
    await page.goto(`${origin}/observe.html?experiment=${encodeURIComponent(first.entry.id)}`); await waitTick(0); assert.match(await page.locator("#sample-cadence").textContent(), /every tick/); assert.equal(await page.locator("#scrub").getAttribute("max"), "300000");
    await bounded(captured, timeouts.operation, "real prefetch hold"); assert.equal(report.ui.delayedOriginal?.status, "HELD");
    const beforeTick = descriptor.first_tick - 1; await seekUi(beforeTick); const expectedBefore = (await reference(first.manifestPath, beforeTick)).frame; await stateUi(expectedBefore); const inspector = await inspectorUi(expectedBefore), pixels = await pixelDigest(), playhead = await shown("playhead");
    await page.locator("#next").click(); await page.waitForFunction(() => document.getElementById("workspace").getAttribute("aria-busy") === "true"); await sleep(250);
    assert.equal(await shown("tick"), beforeTick); assert.equal(await shown("playhead"), playhead); assert.equal(await pixelDigest(), pixels); assert.deepEqual(await page.locator("#cell-detail > dl.kv").evaluate((list) => Object.fromEntries([...list.querySelectorAll("dt")].map((term) => [term.textContent, term.nextElementSibling.textContent]))), inspector); await stateUi(expectedBefore);
    releaseUi(); releases.delete(releaseUi); await waitTick(descriptor.first_tick); const expectedAfter = (await reference(first.manifestPath, descriptor.first_tick)).frame; await stateUi(expectedAfter); assert.equal(await shown("playhead"), expectedAfter.sim_time); await inspectorUi(expectedAfter);
    report.ui.atomicBoundary = { fromTick: beforeTick, toTick: descriptor.first_tick, oldPixelSha256: pixels, oldInspectorId: inspector.id, heldDuringBuffer: true, delayExcludedFromPlayhead: true };
    await page.locator("#previous").click(); await waitTick(beforeTick); await page.locator("#next").click(); await waitTick(descriptor.first_tick);
    for (const tick of [768, 992, 10_000]) { await seekUi(tick); const expected = (await reference(first.manifestPath, tick)).frame; await stateUi(expected); if (tick === 10_000) assert.deepEqual(expected, first.archive.frames.at(-1)); }
  });
  await check("Actual dense pixels/counters are held at 1x; draw targets do not add HTTP or samples", async () => {
    await seekUi(992); await page.waitForLoadState("networkidle"); const baseline = report.http.length, pixels = await pixelDigest(), startTick = await shown("tick"), time = await shown("time");
    await page.locator("#speed-preset").selectOption("1"); assert.equal(Number(await page.locator("#speed").inputValue()), 1);
    await page.locator("#play").evaluate((button) => { button.__denseQaClickTimes = []; button.addEventListener("click", () => { if (button.__denseQaClickTimes.length < 3) button.__denseQaClickTimes.push(performance.now()); }); });
    const firstTime = await shown("playhead"); await page.locator("#play").click();
    for (const fps of [30, 60]) { await page.locator("#draw-fps").selectOption(String(fps)); await sleep(700); assert.equal(await shown("tick"), startTick); assert.equal(await shown("time"), time); assert.equal(await pixelDigest(), pixels); }
    await page.locator("#play").click(); const finalTime = await shown("playhead"), clickTimes = await page.locator("#play").evaluate((button) => button.__denseQaClickTimes);
    assert.equal(clickTimes.length, 2); const measuredWallMs = clickTimes[1] - clickTimes[0], modelSecondsAdvanced = finalTime - firstTime, toleranceSeconds = .1;
    report.ui.hold = { tick: startTick, rate: 1, targets: [30, 60], requestedHoldMs: 1400, measuredWallMs, wallMeasurement: "Browser performance.now() at actual Play and Pause click events; includes intervening commands", browserClickTimesMs: clickTimes, modelSecondsAdvanced, toleranceSeconds, fromPlayhead: firstTime, toPlayhead: finalTime, pixelSha256: pixels, httpRequests: report.http.length - baseline };
    assert.ok(Number.isFinite(measuredWallMs) && measuredWallMs > 0 && Number.isFinite(modelSecondsAdvanced) && modelSecondsAdvanced >= 1 && Math.abs(modelSecondsAdvanced - measuredWallMs / 1000) < toleranceSeconds, `Physical 1x: ${modelSecondsAdvanced} model seconds / ${measuredWallMs / 1000} measured wall seconds`);
    assert.ok(finalTime < (startTick + 1) * 30); assert.equal(await page.locator("#play").getAttribute("aria-label"), "Play recording"); assert.equal(report.http.length, baseline);
  });
  await check("Actual dense controls fit desktop, short desktop and 390/320px mobile", async () => {
    for (const [filename, viewport] of [["dense-desktop.png", { width: 1440, height: 900 }], ["dense-desktop-short.png", { width: 1440, height: 720 }], ["dense-mobile.png", { width: 390, height: 844 }], ["dense-mobile-narrow.png", { width: 320, height: 844 }]]) await layout(filename, viewport);
  });
  const chunkRequests = report.http.filter((item) => item.path.startsWith(new URL(first.entry.dense.index, origin).pathname.replace(/index\.json$/, "")) && item.path.endsWith(".gz")); assert.ok(chunkRequests.length <= 16); report.ui.tenKChunkRequests = chunkRequests.length; await pageEnd();
  for (const current of cases.slice(1)) await check(`Actual dense ${current.entry.experiment.steps} UI uses real endpoints without a full recording fetch`, async () => {
    const baseline = report.http.length; await fresh(); await page.goto(`${origin}/observe.html?experiment=${encodeURIComponent(current.entry.id)}`); await waitTick(0); await stateUi(current.archive.frames[0]);
    await page.locator("#next").click(); await waitTick(1); await stateUi((await reference(current.manifestPath, 1)).frame); await seekUi(current.entry.experiment.steps); await stateUi(current.archive.frames.at(-1));
    assert.equal(await page.locator("#next").isDisabled(), true); assert.equal(await page.locator("#play").getAttribute("aria-label"), "Replay recording"); assert.equal(await page.locator("#frame-count").textContent(), `${current.entry.experiment.steps + 1} / ${current.entry.experiment.steps + 1}`);
    if (current.entry.experiment.steps === 1_000_000) await screenshot("dense-million-end.png"); await page.waitForLoadState("networkidle");
    const requests = report.http.slice(baseline).filter((item) => item.path.endsWith(".gz")); assert.ok(requests.length <= 8); (report.ui.endpoints ||= []).push({ horizon: current.entry.experiment.steps, shownTick: await shown("tick"), chunkRequests: requests.length, method: "actual DOM/canvas and original archival endpoint values" }); await pageEnd();
  });
  report.ui.status = "PASS";
}

async function run() {
  report.sourceHead = git("rev-parse", "HEAD"); report.sourceTree = git("rev-parse", "HEAD^{tree}"); assert.ok(process.env.LIMINIS_EXPECTED_HEAD); assert.equal(report.sourceHead, process.env.LIMINIS_EXPECTED_HEAD); if (process.env.LIMINIS_EXPECTED_TREE) assert.equal(report.sourceTree, process.env.LIMINIS_EXPECTED_TREE); assert.equal(git("status", "--porcelain", "--untracked-files=no"), "");
  for (const filename of ["scripts/check_dense_recording_browser.mjs", "site/dense-recording.mjs", "site/observer.js", "site/recording-loader.mjs", "site/playback.mjs", "site/observe.html", "site/observer.css", "site/catalog.mjs", "scripts/dense-recording-decode.py", "scripts/dense-recording-codec.py"]) await source(filename);
  for (const filename of ["index.json", "horizon-100.json", "chunk-0000000-0000100.jsonl.gz"]) await source(`scripts/fixtures/dense-recording/smoke-100/${filename}`);
  const declared = JSON.parse(await source("scripts/fixtures/dense-recording/pilot-100000/fixture-pins.json")); assert.equal(declared.complete_recording, false); assert.match(declared.label, /^PARTIAL:/); assert.deepEqual(declared.files, pins.map(([path, bytes, sha256]) => ({ path, bytes, sha256 }))); assert.equal(declared.original_bytes, 1504219);
  for (const [filename, length, sha256] of pins) { const value = await source(`scripts/fixtures/dense-recording/pilot-100000/${filename}`); assert.equal(value.length, length); assert.equal(digest(value), sha256); }
  report.fixture = declared; await serve();
  const require = createRequire(import.meta.url), moduleId = process.env.LIMINIS_PLAYWRIGHT_MODULE; assert.ok(moduleId); const pkg = require(`${moduleId}/package.json`); assert.equal(pkg.name, "playwright"); assert.equal(pkg.version, "1.61.1"); const { chromium } = require(moduleId);
  browser = await chromium.launch({ headless: true, chromiumSandbox: true, args: ["--enable-automation"], timeout: timeouts.launch }); report.browser = { version: browser.version(), playwright: pkg.version, chromiumSandbox: true };
  await check("Actual Chromium argv preserves every protected sandbox layer", async () => {
    const session = await bounded(browser.newBrowserCDPSession(), timeouts.cdp, "CDP browser session"); try { const { arguments: argv } = await bounded(session.send("Browser.getBrowserCommandLine"), timeouts.cdp, "observed Chromium argv"); report.launchArgumentCheck.arguments = argv; assert.ok(Array.isArray(argv) && argv.length > 0 && argv.every((arg) => typeof arg === "string")); const forbidden = new Set(["--no-sandbox", "--no-zygote-sandbox", "--disable-setuid-sandbox", "--disable-namespace-sandbox", "--disable-seccomp-filter-sandbox", "--disable-gpu-sandbox", "--allow-sandbox-debugging", "--single-process"]); report.launchArgumentCheck.forbiddenSwitches = argv.map((arg) => arg.split("=", 1)[0]).filter((arg) => forbidden.has(arg) || /^--disable-.*sandbox$/.test(arg)); assert.deepEqual(report.launchArgumentCheck.forbiddenSwitches, []); report.launchArgumentCheck.status = "PASS"; } finally { await bounded(session.detach(), timeouts.cdp, "CDP detach"); }
  });
  assert.equal(stopping, false); context = await bounded(browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 }), timeouts.operation, "browser context"); context.setDefaultTimeout(timeouts.operation); context.setDefaultNavigationTimeout(timeouts.operation);
  await context.route("**/*", async (route) => { const request = route.request(), url = new URL(request.url()); if (url.origin !== origin || request.method() !== "GET") { (report.prohibitedRequests ||= []).push({ url: request.url(), method: request.method() }); await route.abort(); return; } await route.continue(); });
  await bounded(context.tracing.start({ screenshots: true, snapshots: true, sources: true }), timeouts.cdp, "trace start"); tracing = true; report.status = "RUNNING"; await standalone(); await actualUi();
  assert.deepEqual(report.pageErrors, []); assert.deepEqual(report.consoleErrors, []); assert.deepEqual(report.unhandled, []); assert.deepEqual(report.prohibitedRequests || [], []); assert.ok(report.requestFailures.every((request) => request.intentional));
  assert.ok(report.http.every((request) => [200, 204].includes(request.status) && !request.error), "Every actual QA HTTP request must succeed or be an explicit body cancellation");
  assert.equal(git("rev-parse", "HEAD"), report.sourceHead); assert.equal(git("rev-parse", "HEAD^{tree}"), report.sourceTree); assert.equal(git("status", "--porcelain", "--untracked-files=no"), ""); report.servedBytes = servedBytes; assert.equal(stopping, false); report.status = "PASS";
}

await mkdir(output, { recursive: true });
try { for (const filename of ["trace.zip", "failure.png", "dense-desktop.png", "dense-desktop-short.png", "dense-mobile.png", "dense-mobile-narrow.png", "dense-million-end.png"]) await rm(join(output, filename), { force: true }); running = run(); await bounded(running, timeouts.run, "whole bounded dense browser QA"); }
catch (error) { fail(error); for (const release of [...releases]) release(); if (running) await bounded(running.catch(() => {}), timeouts.operation + timeouts.launch, "interrupted QA settlement").catch((error) => fail(error, "interrupted QA settlement")); }
finally {
  for (const release of [...releases]) release();
  async function cleanup(name, action, timeout = timeouts.cleanup) { try { return { ok: true, value: await bounded(action(), timeout, name) }; } catch (error) { report.cleanupErrors.push({ name, error: error.stack || String(error) }); fail(error, name); return { ok: false }; } }
  if (report.status !== "PASS" && page && !page.isClosed()) await cleanup("failure PNG", () => screenshot("failure.png"));
  if (tracing && context) { const saved = await cleanup("trace publication", async () => { await context.tracing.stop({ path: join(output, "trace.zip") }); const value = await readFile(join(output, "trace.zip")); assert.ok(value.length); return { bytes: value.length, sha256: digest(value) }; }, timeouts.trace); Object.assign(report.trace, saved.ok ? { status: "PASS", ...saved.value } : { status: "FAIL" }); }
  if (context) await cleanup("context cleanup", () => context.close()); if (browser) await cleanup("browser cleanup", () => browser.close());
  if (server) await cleanup("loopback server cleanup", () => new Promise((done, reject) => { server.close((error) => error ? reject(error) : done()); server.closeAllConnections(); }));
  await sleep(50);
  if (report.status === "PASS") try { assert.equal(git("rev-parse", "HEAD"), report.sourceHead); assert.equal(git("rev-parse", "HEAD^{tree}"), report.sourceTree); assert.equal(git("status", "--porcelain", "--untracked-files=no"), ""); } catch (error) { fail(error, "post-cleanup source guard"); }
  if (report.status === "PASS" && (report.trace.status !== "PASS" || report.launchArgumentCheck.status !== "PASS" || report.decoder.status !== "PASS" || report.ui.status !== "PASS" || report.cleanupErrors.length || report.unhandled.length || report.pageErrors.length || report.consoleErrors.length || (report.prohibitedRequests || []).length || report.requestFailures.some((request) => !request.intentional) || report.http.some((request) => ![200, 204].includes(request.status) || request.error) || report.screenshots.length !== 5)) fail(new Error("Required final browser evidence is incomplete"), "final gate");
  report.finishedAt = new Date().toISOString(); await writeFile(join(output, "report.json"), JSON.stringify(report, null, 2) + "\n"); console.log(JSON.stringify({ status: report.status, sourceHead: report.sourceHead, checks: report.checks.length, output })); if (report.status !== "PASS") process.exitCode = 1;
}
