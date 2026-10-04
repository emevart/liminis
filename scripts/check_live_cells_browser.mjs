// Real Chromium QA of the release cells host; no substituted API states.
import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, mkdir, rm, writeFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const modulePath = process.env.LIMINIS_PLAYWRIGHT_MODULE;
assert.ok(modulePath, 'LIMINIS_PLAYWRIGHT_MODULE must name the pinned Playwright module');
const resolvedModule = modulePath.startsWith('/') && (await stat(modulePath)).isDirectory() ? join(modulePath, 'index.mjs') : modulePath;
const { chromium } = await import(resolvedModule.startsWith('/') ? pathToFileURL(resolvedModule).href : resolvedModule);
const execute = promisify(execFile);
const head = (await execute('git', ['rev-parse', 'HEAD'])).stdout.trim();
const tree = (await execute('git', ['rev-parse', 'HEAD^{tree}'])).stdout.trim();
assert.ok(process.env.LIMINIS_EXPECTED_HEAD, 'LIMINIS_EXPECTED_HEAD must identify the exact PR head');
assert.equal(head, process.env.LIMINIS_EXPECTED_HEAD, 'browser QA must run the exact requested PR head');
const output = resolve('target/qa/live-browser');
await mkdir(output, { recursive: true });
const temporary = await mkdtemp(resolve(tmpdir(), 'liminis-browser-'));
const report = { gitHead: head, gitTree: tree, playwright: '1.61.1 (pinned by CI)', status: 'running', checks: [], screenshots: [], consoleErrors: [], pageErrors: [], resourceWarnings: [] };
let host, browser;
let hostLog = '';
const check = name => report.checks.push(name);
const delay = ms => new Promise(done => setTimeout(done, ms));
async function eventually(predicate, label, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    if (host && (host.exitCode !== null || host.signalCode !== null)) throw new Error(`Host exited during ${label}: ${hostLog}`);
    await delay(50);
  }
  throw new Error(`Timed out: ${label}`);
}
try {
  const reservation = createServer();
  reservation.listen(0, '127.0.0.1');
  await once(reservation, 'listening');
  const port = reservation.address().port;
  await new Promise(done => reservation.close(done));
  const origin = `http://127.0.0.1:${port}`;
  host = spawn(resolve('target/release/liminis'), ['cells', '--seed', '42', '--port', String(port), '--data-dir', temporary], { stdio: ['ignore', 'pipe', 'pipe'] });
  host.on('error', error => { hostLog += `${error.stack}\n`; });
  host.stdout.on('data', data => { hostLog += data; });
  host.stderr.on('data', data => { hostLog += data; });
  const state = async () => {
    const response = await fetch(`${origin}/api/state`);
    assert.equal(response.status, 200);
    return response.json();
  };
  await eventually(async () => { try { return (await state()).kind === 'cells'; } catch { return false; } }, 'real host readiness');
  browser = await chromium.launch({ headless: true });
  report.browserVersion = browser.version();
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1 });
  let stateRequests = 0;
  page.on('request', request => { if (new URL(request.url()).pathname === '/api/state') stateRequests++; });
  page.on('console', message => {
    if (message.type() !== 'error') return;
    const entry = { text: message.text(), location: message.location() };
    // Chromium may request a browser icon absent from this local observer.
    // Record that precise cosmetic 404 separately; all other errors fail QA.
    if (entry.location.url === `${origin}/favicon.ico` && /404/.test(entry.text)) report.resourceWarnings.push(entry);
    else report.consoleErrors.push(entry);
  });
  page.on('pageerror', error => report.pageErrors.push(error.stack || error.message));
  await page.goto(origin, { waitUntil: 'networkidle' });
  await page.waitForFunction(() => document.querySelector('#live-label').textContent === 'evolving');
  const initial = await state();
  assert.ok(initial.cells.length > 0);
  report.scenario = initial.scenario;
  report.configHash = initial.config_hash;
  report.seed = initial.seed;
  check('real Rust cells response rendered in Chromium');

  await page.locator('#play').click();
  await page.waitForFunction(() => document.querySelector('#play').getAttribute('aria-label') === 'Run simulation');
  const paused = await state();
  assert.equal(paused.running, false);
  await delay(300);
  assert.equal((await state()).tick, paused.tick);
  check('Pause acknowledgment has no subsequent accumulated ticks');

  await page.locator('#step').click();
  await eventually(async () => (await state()).tick === paused.tick + 1, 'one manual Step');
  assert.equal((await state()).running, false);
  check('Step advances exactly one real tick while paused');

  await page.locator('#speed').fill('1');
  await page.locator('#speed-form button[type=submit]').click();
  await eventually(async () => (await state()).pacing.multiplier === 1, 'manual multiplier');
  const manual = await state();
  assert.equal(manual.dt_seconds, 30);
  assert.equal(manual.target_tps, 1 / 30);
  await page.waitForFunction(() => document.querySelector('#target-speed').textContent.startsWith('×1 /'));
  check('manual 1× reports 1/30 TPS at unchanged dt=30s');
  for (const multiplier of [10, 100, 1000]) {
    await page.locator(`[data-speed="${multiplier}"]`).click();
    await eventually(async () => (await state()).pacing.multiplier === multiplier, `preset ${multiplier}`);
  }
  assert.equal((await state()).tick, manual.tick);
  check('presets switch multiplier without reset or ticks while paused');
  await page.locator('#speed').fill('-1');
  await page.locator('#speed-form button[type=submit]').click();
  await page.waitForFunction(() => document.querySelector('#control-error').textContent.includes('positive finite'));
  assert.equal((await state()).pacing.multiplier, 1000);
  await page.locator('[data-speed="1"]').click();
  await page.waitForFunction(() => document.querySelector('#control-error').textContent === '');
  check('invalid manual speed gives visible error and leaves pacing unchanged');

  const chamber = page.locator('#chamber');
  await chamber.focus();
  await page.keyboard.press('ArrowRight');
  await page.waitForFunction(() => !document.querySelector('#cell-detail').classList.contains('empty'));
  assert.match(await page.locator('#selection-state').textContent(), /generation/);
  const nonBlank = await chamber.evaluate(canvas => {
    const context = canvas.getContext('2d');
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
    let visible = 0;
    for (let i = 0; i < pixels.length; i += 4) if (pixels[i] > 40 || pixels[i + 1] > 40 || pixels[i + 2] > 40) visible++;
    return visible > 20;
  });
  assert.equal(nonBlank, true);
  check('canvas contains real cell glyphs; keyboard selection updates inspector');

  await page.locator('#screen-fps').selectOption('30');
  await page.locator('#screen-fps').evaluate(element => element.blur());
  const requestsBefore30 = stateRequests;
  await delay(1400);
  const requests30 = stateRequests - requestsBefore30;
  const held30 = await chamber.screenshot();
  await writeFile(resolve(output, 'held-30fps.png'), held30);
  await page.locator('#screen-fps').selectOption('60');
  await page.locator('#screen-fps').evaluate(element => element.blur());
  const requestsBefore60 = stateRequests;
  await delay(1400);
  const requests60 = stateRequests - requestsBefore60;
  const held60 = await chamber.screenshot();
  await writeFile(resolve(output, 'held-60fps.png'), held60);
  assert.ok(requests30 >= 1 && requests30 <= 3 && requests60 >= 1 && requests60 <= 3 && Math.abs(requests30 - requests60) <= 1, `screen FPS must not scale HTTP polling: ${requests30}/${requests60}`);
  report.pausedStateRequests = { fps30: requests30, fps60: requests60, intervalMs: 1400 };
  assert.ok(held30.equals(held60), 'paused chamber pixels must stay identical at 30/60 FPS');
  assert.equal((await state()).tick, manual.tick);
  check('30/60 FPS hold pixel-identical confirmed paused state without model advancement');

  await page.locator('#maximum').click();
  await page.waitForFunction(() => document.querySelector('#maximum').getAttribute('aria-pressed') === 'true');
  const beforeMaximum = await state();
  assert.equal(beforeMaximum.pacing.mode, 'maximum');
  assert.equal(beforeMaximum.target_tps, null);
  await page.locator('#play').click();
  await eventually(async () => (await state()).tick >= beforeMaximum.tick + 20, 'Maximum real ticks');
  await page.waitForFunction(() => document.querySelector('#play').getAttribute('aria-label') === 'Pause simulation');
  await page.locator('#play').click();
  await page.waitForFunction(() => document.querySelector('#play').getAttribute('aria-label') === 'Run simulation');
  const afterMaximum = await state();
  assert.equal(afterMaximum.running, false);
  await delay(300);
  assert.equal((await state()).tick, afterMaximum.tick);
  check('Maximum advances real ticks and remains responsive to Pause');

  await eventually(async () => !(await state()).save_pending && !(await state()).persistence.saving, 'writer available');
  await page.waitForFunction(() => !document.querySelector('#save').disabled);
  await page.locator('#save').click();
  await eventually(async () => {
    const s = await state();
    return !s.save_pending && !s.persistence.saving && s.persistence.saved_tick >= afterMaximum.tick;
  }, 'checkpoint persisted');
  await page.waitForFunction(() => document.querySelector('#save-status').textContent.startsWith('saved '));
  assert.equal((await state()).tick, afterMaximum.tick);
  check('Save acknowledges persisted checkpoint without advancing paused model');

  for (const width of [1440, 590, 420, 320]) {
    await page.setViewportSize({ width, height: 1000 });
    await delay(150);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `horizontal overflow at ${width}px`);
    const filename = `viewer-${width}.png`;
    await page.screenshot({ path: resolve(output, filename), fullPage: true });
    report.screenshots.push(filename);
  }
  check('real desktop/mobile screenshots and no horizontal overflow at 1440/590/420/320');
  assert.deepEqual(report.pageErrors, []);
  assert.deepEqual(report.consoleErrors, []);
  check('no Chromium page/console errors');
  report.status = 'passed';
  report.finalState = { tick: afterMaximum.tick, dt_seconds: afterMaximum.dt_seconds, pacing: afterMaximum.pacing };
  console.log(JSON.stringify(report, null, 2));
} catch (error) {
  report.status = 'failed';
  report.error = error.stack || String(error);
  process.exitCode = 1;
  console.error(report.error);
} finally {
  if (browser) {
    try { await Promise.race([browser.close(), delay(3000)]); }
    catch (error) { report.cleanupError = String(error); process.exitCode = 1; }
  }
  if (host && host.exitCode === null && host.signalCode === null) {
    host.kill('SIGTERM');
    await Promise.race([once(host, 'exit'), delay(3000)]);
    if (host.exitCode === null && host.signalCode === null) {
      host.kill('SIGKILL');
      await Promise.race([once(host, 'exit'), delay(3000)]);
    }
  }
  await writeFile(resolve(output, 'host.log'), hostLog);
  await writeFile(resolve(output, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  await rm(temporary, { recursive: true, force: true });
}
