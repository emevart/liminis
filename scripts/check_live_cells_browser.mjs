// Real Chromium QA of the release cells host; no substituted API states.
import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { mkdtemp, mkdir, rm, writeFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

const modulePath = process.env.LIMINIS_PLAYWRIGHT_MODULE;
assert.ok(modulePath, 'LIMINIS_PLAYWRIGHT_MODULE must name the pinned Playwright module');
const resolvedModule = modulePath.startsWith('/') && (await stat(modulePath)).isDirectory() ? join(modulePath, 'index.mjs') : modulePath;
const { chromium } = await import(resolvedModule.startsWith('/') ? pathToFileURL(resolvedModule).href : resolvedModule);
const execute = promisify(execFile);
const head = (await execute('git', ['rev-parse', 'HEAD'])).stdout.trim();
const tree = (await execute('git', ['rev-parse', 'HEAD^{tree}'])).stdout.trim();
const trackedStatus = (await execute('git', ['status', '--porcelain', '--untracked-files=no'])).stdout.trim();
assert.equal(trackedStatus, '', 'browser QA requires a clean tracked checkout');
if (process.env.LIMINIS_EXPECTED_TREE) assert.equal(tree, process.env.LIMINIS_EXPECTED_TREE, 'browser QA must run the expected tree');
assert.ok(process.env.LIMINIS_EXPECTED_HEAD, 'LIMINIS_EXPECTED_HEAD must identify the exact PR head');
assert.equal(head, process.env.LIMINIS_EXPECTED_HEAD, 'browser QA must run the exact requested PR head');
const scenarioConfig = process.env.LIMINIS_LIVE_BROWSER_CONFIG;
if (scenarioConfig) {
  const scenarioPath = resolve(scenarioConfig);
  assert.ok(scenarioPath.startsWith(resolve('configs/scenarios') + sep) && scenarioPath.endsWith('.toml') && (await stat(scenarioPath)).isFile(), 'LIMINIS_LIVE_BROWSER_CONFIG must name a repository scenario TOML');
}
const output = resolve(scenarioConfig ? 'target/qa/live-browser-physical' : 'target/qa/live-browser');
await mkdir(output, { recursive: true });
const temporary = await mkdtemp(resolve(tmpdir(), 'liminis-browser-'));
const report = { gitHead: head, gitTree: tree, trackedClean: true, chromiumSandbox: true, playwright: '1.61.1 (pinned by CI)', status: 'running', checks: [], screenshots: [], consoleErrors: [], pageErrors: [], resourceWarnings: [] };
let host, browser, context, page;
let tracing = false;
let stopping = false;
let hostLog = '';
const check = name => report.checks.push(name);
const delay = ms => new Promise(done => setTimeout(done, ms));
async function bounded(operation, timeout, label) {
  let timer;
  try {
    return await Promise.race([operation, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`Timed out: ${label} (${timeout}ms)`)), timeout);
    })]);
  } finally { clearTimeout(timer); }
}
function fail(error, stage = 'run') {
  stopping = true;
  report.status = 'failed';
  report.failures ??= [];
  report.failures.push({ stage, error: error.stack || String(error) });
  process.exitCode = 1;
}
function appendHostLog(data) {
  // Keep a bounded diagnostic tail even if a failing host logs indefinitely.
  hostLog = (hostLog + data).slice(-1024 * 1024);
}
async function eventually(predicate, label, timeout = 15000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    if (host && (host.exitCode !== null || host.signalCode !== null)) throw new Error(`Host exited during ${label}: ${hostLog}`);
    await delay(50);
  }
  throw new Error(`Timed out: ${label}`);
}
async function run() {
  const reservation = createServer();
  reservation.listen(0, '127.0.0.1');
  await bounded(once(reservation, 'listening'), 5000, 'free port reservation');
  const port = reservation.address().port;
  await new Promise(done => reservation.close(done));
  const origin = `http://127.0.0.1:${port}`;
  const hostArguments = ['cells', '--seed', '42', '--port', String(port), '--data-dir', temporary];
  if (scenarioConfig) hostArguments.push('--config', resolve(scenarioConfig));
  host = spawn(resolve('target/release/liminis'), hostArguments, { stdio: ['ignore', 'pipe', 'pipe'] });
  host.on('error', error => appendHostLog(`${error.stack}\n`));
  host.stdout.on('data', appendHostLog);
  host.stderr.on('data', appendHostLog);
  let identity;
  const positions = state => state.cells.map(cell => ({ id: cell.id, position_m: cell.position_m }));
  const state = async () => {
    const response = await fetch(`${origin}/api/state`, { signal: AbortSignal.timeout(5000) });
    assert.equal(response.status, 200);
    const result = await response.json();
    const currentIdentity = { seed: result.seed, dt_seconds: result.dt_seconds, config_hash: result.config_hash, chamber_format: result.chamber_format, model: result.model };
    identity ??= currentIdentity;
    assert.deepEqual(currentIdentity, identity, 'seed, dt, config and chamber model must stay unchanged');
    assert.equal(result.seed, '42');
    assert.equal(result.dt_seconds, 30);
    if (scenarioConfig) {
      assert.equal(result.chamber_format, 2, 'opt-in browser config must be physical chamber format 2');
      assert.equal(result.model.spatial_positions, true);
      const dimensions = result.model.dimensions_m;
      assert.ok(Array.isArray(dimensions) && dimensions.length === 3 && dimensions.every(length => Number.isFinite(length) && length > 0), 'physical box dimensions must be finite and positive');
      for (const cell of result.cells) {
        assert.ok(Array.isArray(cell.position_m) && cell.position_m.length === 3, `physical position missing for ${cell.id}`);
        cell.position_m.forEach((coordinate, axis) => assert.ok(Number.isFinite(coordinate) && coordinate >= 0 && coordinate <= dimensions[axis], `cell ${cell.id} position outside reported physical box`));
      }
    } else {
      assert.equal(result.chamber_format, 1, 'default browser run must retain legacy chamber format 1');
      assert.equal(result.model.spatial_positions, false);
      assert.ok(result.cells.every(cell => !Object.hasOwn(cell, 'position_m')), 'legacy state must not invent physical positions');
    }
    return result;
  };
  await eventually(async () => { try { return (await state()).kind === 'cells'; } catch { return false; } }, 'real host readiness');
  browser = await chromium.launch({ headless: true, chromiumSandbox: true, args: ['--enable-automation'], timeout: 20000 });
  report.browserVersion = browser.version();
  const browserSession = await bounded(browser.newBrowserCDPSession(), 5000, 'browser command-line session');
  try {
    const commandLine = await bounded(browserSession.send('Browser.getBrowserCommandLine'), 5000, 'protected browser command-line evidence');
    report.browserArguments = commandLine.arguments;
    const forbidden = ['--no-sandbox', '--disable-setuid-sandbox', '--disable-seccomp-filter-sandbox', '--disable-namespace-sandbox', '--disable-gpu-sandbox'];
    assert.ok(Array.isArray(report.browserArguments), 'Chromium must disclose command-line evidence');
    for (const flag of forbidden) assert.ok(!report.browserArguments.some(argument => argument === flag || argument.startsWith(`${flag}=`)), `protected launch must not contain ${flag}`);
    check('Chromium command line contains no sandbox-disabling flags');
  } finally { await bounded(browserSession.detach(), 3000, 'browser command-line session close'); }
  context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1 });
  context.setDefaultTimeout(10000);
  context.setDefaultNavigationTimeout(15000);
  await context.tracing.start({ screenshots: true, snapshots: true, sources: true });
  tracing = true;
  page = await context.newPage();
  let stateRequests = 0;
  const requests = [];
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname === '/api/state') stateRequests++;
    if (requests.length >= 1024) { fail(new Error('Browser request evidence exceeded 1024 records'), 'request evidence'); return; }
    requests.push({ method: request.method(), origin: url.origin, path: url.pathname, atMs: Date.now() });
  });
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
  report.chamberFormat = initial.chamber_format;
  report.scenarioConfig = scenarioConfig || 'default';
  report.model = initial.model;
  report.canvasMeaning = scenarioConfig ? 'orthographic physical point centers from saved coordinates; marker size is visual, not body radius' : 'inventory schematic; glyph placement is not physical coordinates';
  if (scenarioConfig) check('format-2 API reports finite in-box persisted coordinates; seed/dt/model identity unchanged');
  else check('default format-1 API has no fabricated physical coordinates');
  check('real Rust cells response rendered in Chromium');

  await page.locator('#play').click();
  await page.waitForFunction(() => document.querySelector('#play').getAttribute('aria-label') === 'Run simulation');
  const paused = await state();
  assert.equal(paused.running, false);
  await delay(300);
  const heldPaused = await state();
  assert.equal(heldPaused.tick, paused.tick);
  if (scenarioConfig) assert.deepEqual(positions(heldPaused), positions(paused), 'physical coordinates cannot move while paused');
  check('Pause acknowledgment has no subsequent accumulated ticks');

  await page.locator('#step').click();
  await eventually(async () => (await state()).tick === paused.tick + 1, 'one manual Step');
  const stepped = await state();
  assert.equal(stepped.running, false);
  if (scenarioConfig) {
    const previous = new Map(paused.cells.map(cell => [cell.id, cell.position_m]));
    const moved = stepped.cells.filter(cell => previous.has(cell.id) && cell.position_m.some((coordinate, axis) => coordinate !== previous.get(cell.id)[axis]));
    assert.ok(moved.length > 0, 'a real Step must advance at least one surviving physical cell position');
    report.physicalStep = { fromTick: paused.tick, toTick: stepped.tick, movedSurvivingCells: moved.length };
    check('one real model tick advances persisted positions for surviving IDs');
  }
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
  const heldAfterFps = await state();
  assert.equal(heldAfterFps.tick, manual.tick);
  if (scenarioConfig) assert.deepEqual(positions(heldAfterFps), positions(manual), '30/60 FPS must not move persisted physical coordinates');
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
  const layoutSnapshot = await state();
  assert.equal(layoutSnapshot.tick, afterMaximum.tick);
  check('Save acknowledges persisted checkpoint without advancing paused model');

  // All responsive actions use this already confirmed paused population. Find,
  // scrolling and resizing must not submit a model control or alter its state.
  assert.ok(layoutSnapshot.cells.length > 0, 'responsive Find needs a real living cell');
  const selectedCell = layoutSnapshot.cells[0];
  const absentId = '18446744073709551615';
  assert.ok(!layoutSnapshot.cells.some(cell => cell.id === absentId), 'absent-ID UI query must be absent in the actual API snapshot');
  const displayNumber = value => {
    if (value === 0) return '0';
    const magnitude = Math.abs(value);
    return magnitude >= 1e4 || magnitude < 1e-3 ? value.toExponential(2) : value.toLocaleString('en-US', { maximumSignificantDigits: 3 });
  };
  const expectedInspector = {
    id: selectedCell.id, 'shown snapshot tick': String(layoutSnapshot.tick), parent: selectedCell.parent_id ?? 'founder',
    'birth tick': String(selectedCell.birth_tick), age: `${displayNumber(selectedCell.age_s)} s`,
    mass: `${displayNumber(selectedCell.mass_mol)} mol`, 'division mass': `${displayNumber(selectedCell.division_mass_mol)} mol`, energy: `${displayNumber(selectedCell.energy_j)} J`,
  };
  const inspector = () => page.locator('#cell-detail').evaluate(host => {
    const list = host.querySelector('dl');
    const fields = Object.fromEntries([...list.querySelectorAll('dt')].map(label => [label.textContent, label.nextElementSibling.textContent]));
    return { fields, genomeKey: host.querySelector('.selected-code b').textContent, selection: document.querySelector('#selection-state').textContent };
  });
  const assertInspector = current => {
    for (const [key, value] of Object.entries(expectedInspector)) assert.equal(current.fields[key], value, `Find inspector ${key} must match the actual paused API cell`);
    assert.equal(current.genomeKey, selectedCell.genome_key);
    assert.equal(current.selection, `generation ${selectedCell.generation}`);
  };
  const geometry = () => page.evaluate(() => {
    const rect = element => {
      const box = element.getBoundingClientRect();
      return { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height };
    };
    const stage = document.querySelector('.stage'), readout = document.querySelector('.readout'), side = document.querySelector('.side');
    const text = [];
    const walker = document.createTreeWalker(readout, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const node = walker.currentNode;
      if (!node.textContent.trim()) continue;
      const range = document.createRange(); range.selectNodeContents(node);
      for (const box of range.getClientRects()) if (box.width > 0 && box.height > 0) text.push({ value: node.textContent.trim(), left: box.left, top: box.top, right: box.right, bottom: box.bottom });
    }
    return {
      stage: rect(stage), canvas: rect(document.querySelector('#chamber')), readout: rect(readout), side: rect(side),
      readoutPosition: getComputedStyle(readout).position, workspaceDisplay: getComputedStyle(document.querySelector('.workspace')).display,
      rows: { stage: getComputedStyle(stage).gridRowStart, readout: getComputedStyle(readout).gridRowStart, side: getComputedStyle(side).gridRowStart },
      stageMinHeight: getComputedStyle(stage).minHeight, readoutText: text,
      readoutOverflow: readout.scrollWidth > readout.clientWidth, horizontalOverflow: document.documentElement.scrollWidth > innerWidth,
      tick: document.querySelector('#tick').textContent, dt: document.querySelector('#dt').textContent,
      target: document.querySelector('#target-speed').textContent, live: document.querySelector('#live-label').textContent,
      generation: document.querySelector('#generation').textContent, matter: document.querySelector('#matter').textContent, energy: document.querySelector('#energy').textContent,
    };
  });
  const assertGeometry = (current, width) => {
    assert.equal(current.horizontalOverflow, false, `horizontal overflow at ${width}px`);
    assert.equal(current.readoutOverflow, false, `readout horizontal clipping at ${width}px`);
    assert.ok(current.stage.width > 0 && current.stage.height > 0 && current.readout.height > 0 && current.side.height > 0);
    assert.ok(current.stage.bottom <= current.readout.top || current.readout.bottom <= current.stage.top, 'readout must not cover the stage or its canvas');
    assert.ok(current.side.bottom <= current.readout.top || current.readout.bottom <= current.side.top, 'readout must not cover the sidebar');
    for (const text of current.readoutText) assert.ok(text.left >= current.readout.left && text.right <= current.readout.right && text.top >= current.readout.top && text.bottom <= current.readout.bottom, `readout text must be completely in its normal-flow rectangle: ${text.value}`);
    assert.equal(current.tick, layoutSnapshot.tick.toLocaleString('en-US'));
    assert.equal(current.dt, 'dt 30 s');
    assert.equal(current.target, 'Maximum');
    assert.equal(current.live, 'paused');
    assert.equal(current.generation, String(layoutSnapshot.summary.generation_max));
    assert.equal(current.matter, String(layoutSnapshot.residual?.matter ?? '—'));
    assert.equal(current.energy, String(layoutSnapshot.residual?.energy ?? '—'));
    if (width <= 800) {
      assert.equal(current.readoutPosition, 'static', 'mobile readout must use normal flow');
      assert.equal(current.workspaceDisplay, 'contents');
      assert.deepEqual(current.rows, { stage: '3', readout: '4', side: '5' });
      assert.ok(current.stage.bottom <= current.readout.top && current.readout.bottom <= current.side.top, 'mobile order must be stage, complete readout, sidebar');
      const minimum = scenarioConfig ? 560 : width <= 520 ? 470 : 500;
      assert.equal(current.stageMinHeight, `${minimum}px`, 'existing chamber-specific mobile stage minimum must be preserved');
      assert.equal(current.stage.height, width <= 520 ? minimum : Math.max(minimum, 620), 'existing 470px/62dvh stage rule and physical minimum must be preserved');
    }
  };
  // Only real browser rectangles and native hit tests are observed. No CSS,
  // production state, layout helpers, or hit maps are installed by this gate.
  const visibleTarget = async (locator, inspectorField = null) => {
    const beforeScroll = await page.evaluate(() => ({ scrollY, sideScrollTop: document.querySelector('.side').scrollTop }));
    await locator.scrollIntoViewIfNeeded();
    // Inspector descendants are replaced on ordinary polls. Scroll their
    // persistent host, then observe the current field and snapshot together in
    // one synchronous read. The field, not the host, must pass every hit/bound.
    const evidence = await locator.evaluate((host, inspectorField) => {
      let element = host, currentInspector = null, field = null;
      if (inspectorField) {
        const primary = host.querySelector(':scope > dl');
        if (!primary) throw new Error('Current inspector primary fields are absent');
        if (inspectorField === 'upper') element = primary;
        else if (inspectorField === 'lower') {
          const fields = host.querySelectorAll('.object-grid dd');
          element = fields[fields.length - 1];
        } else throw new Error('Unknown inspector field target');
        if (!element) throw new Error(`Current ${inspectorField} inspector field is absent`);
        currentInspector = {
          fields: Object.fromEntries([...primary.querySelectorAll('dt')].map(label => [label.textContent, label.nextElementSibling.textContent])),
          genomeKey: host.querySelector('.selected-code b').textContent, selection: document.querySelector('#selection-state').textContent,
        };
        field = { target: inspectorField, text: element.textContent, group: element.closest('.object')?.querySelector('.micro')?.textContent ?? null,
          key: inspectorField === 'lower' ? element.previousElementSibling.textContent : null, value: element.textContent };
      }
      const box = element.getBoundingClientRect(), css = getComputedStyle(element);
      const x = (box.left + box.right) / 2, y = (box.top + box.bottom) / 2;
      const target = document.elementFromPoint(x, y);
      let clipped = false;
      const ancestorClipping = [];
      for (let ancestor = element.parentElement; ancestor; ancestor = ancestor.parentElement) {
        const style = getComputedStyle(ancestor), boundary = ancestor.getBoundingClientRect();
        const clipsX = ['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowX) && (box.left < boundary.left || box.right > boundary.right);
        const clipsY = ['auto', 'scroll', 'hidden', 'clip'].includes(style.overflowY) && (box.top < boundary.top || box.bottom > boundary.bottom);
        if (clipsX || clipsY) clipped = true;
        if (inspectorField) ancestorClipping.push({ tagName: ancestor.tagName, id: ancestor.id, overflowX: style.overflowX, overflowY: style.overflowY,
          left: boundary.left, top: boundary.top, right: boundary.right, bottom: boundary.bottom, clipsX, clipsY });
      }
      return { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height,
        viewport: { width: innerWidth, height: innerHeight }, scrollY, sideScrollTop: document.querySelector('.side').scrollTop,
        visible: css.display !== 'none' && css.visibility === 'visible' && box.width > 0 && box.height > 0,
        disabled: Boolean(element.disabled), unobscured: target === element || element.contains(target), clipped,
        ...(inspectorField ? { field, currentInspector, ancestorClipping, style: { display: css.display, visibility: css.visibility, position: css.position, overflowX: css.overflowX, overflowY: css.overflowY },
          hit: { tagName: target?.tagName ?? null, id: target?.id ?? null } } : {}) };
    }, inspectorField);
    assert.equal(evidence.visible, true, 'scroll target must be rendered');
    assert.equal(evidence.disabled, false, 'scroll target must remain usable');
    assert.equal(evidence.unobscured, true, 'native hit test must reach the requested control or content');
    assert.equal(evidence.clipped, false, 'scroll target must not be clipped by an ancestor');
    assert.ok(evidence.left >= 0 && evidence.right <= evidence.viewport.width && evidence.top >= 0 && evidence.bottom <= evidence.viewport.height, 'complete scroll target must fit in the real viewport');
    if (inspectorField) assertInspector(evidence.currentInspector);
    evidence.beforeScroll = beforeScroll;
    return evidence;
  };
  const captureViewport = async (width, label) => {
    const filename = `viewer-${width}-viewport-${label}.png`;
    await page.screenshot({ path: resolve(output, filename), fullPage: false });
    report.screenshots.push(filename);
    return filename;
  };
  report.responsive = { snapshot: { tick: layoutSnapshot.tick, dt_seconds: layoutSnapshot.dt_seconds, pacing: layoutSnapshot.pacing, cells: layoutSnapshot.cells.length },
    selectedCell, absentId, controls: 'Native hit tests only; no model control is clicked during responsive checks', cases: [] };
  for (const width of [1440, 590, 420, 390, 320]) {
    const requestsBeforeLayout = requests.length, layoutStartedMs = Date.now();
    await page.setViewportSize({ width, height: 1000 });
    await delay(150);
    await page.evaluate(() => { window.scrollTo(0, 0); document.querySelector('.side').scrollTop = 0; });
    const evidence = { width, height: 1000, before: await geometry(), targets: {}, screenshots: [] };
    report.responsive.cases.push(evidence);
    assertGeometry(evidence.before, width);
    evidence.targets.stageCanvas = await visibleTarget(chamber);
    evidence.screenshots.push(await captureViewport(width, 'stage'));
    const disclaimer = page.locator('#schematic-caption');
    assert.equal(await disclaimer.isVisible(), !scenarioConfig, `schematic caption visibility at ${width}px`);
    if (!scenarioConfig) assert.match(await disclaimer.textContent(), /Icon placement does not represent physical coordinates/);
    const physicalNote = page.locator('#physical-state-note');
    assert.equal(await physicalNote.isVisible(), Boolean(scenarioConfig), `physical-position note visibility at ${width}px`);
    if (scenarioConfig) {
      assert.match(await physicalNote.textContent(), /Physical centers from saved coordinates/);
      assert.match(await physicalNote.textContent(), /Marker size is visual, not cell body radius/);
      assert.match(await physicalNote.textContent(), /Chemical resources remain shared and well mixed/);
      assert.equal(await page.locator('#projection-controls').isVisible(), true);
    }
    evidence.targets.readout = await visibleTarget(page.locator('.readout'));
    for (const selector of ['#play', '#step', '#speed', '#speed-form button[type=submit]', '[data-speed="1"]', '[data-speed="10"]', '[data-speed="100"]', '[data-speed="1000"]', '#maximum', '#screen-fps', '#reset-seed', '#seed-form button[type=submit]', '#save']) {
      evidence.targets[selector] = await visibleTarget(page.locator(selector));
    }
    assertGeometry(await geometry(), width);
    evidence.screenshots.push(await captureViewport(width, 'readout'));
    evidence.targets.cellId = await visibleTarget(page.locator('#cell-id'));
    evidence.targets.find = await visibleTarget(page.locator('#cell-search button[type=submit]'));
    await page.locator('#cell-id').fill(absentId);
    await page.locator('#cell-search button[type=submit]').click();
    await page.waitForFunction(id => document.querySelector('#selection-state').textContent === 'not present' && document.querySelector('#cell-detail').textContent.includes(`ID ${id} is not present`), absentId);
    assert.equal(await page.locator('#cell-detail dl').count(), 0, 'absent-ID Find must clear the prior inspector');
    evidence.findAbsent = { id: absentId, status: await page.locator('#selection-state').textContent(), inspectorLists: 0 };
    await page.locator('#cell-id').fill(selectedCell.id);
    await page.locator('#cell-search button[type=submit]').click();
    await page.waitForFunction(id => document.querySelector('#cell-detail dl dt')?.nextElementSibling?.textContent === id, selectedCell.id);
    evidence.findPresent = await inspector();
    assertInspector(evidence.findPresent);
    evidence.targets.upperInspector = await visibleTarget(page.locator('#cell-detail'), 'upper');
    evidence.screenshots.push(await captureViewport(width, 'inspector-upper'));
    evidence.targets.lowerInspector = await visibleTarget(page.locator('#cell-detail'), 'lower');
    evidence.lowerInspector = evidence.targets.lowerInspector.field;
    assert.equal(evidence.lowerInspector.group, 'Phenotype');
    const rawPhenotypeValue = selectedCell.phenotype[evidence.lowerInspector.key];
    assert.equal(typeof rawPhenotypeValue, 'number');
    const phenotypeMagnitude = Math.abs(rawPhenotypeValue);
    const expectedPhenotype = Number.isInteger(rawPhenotypeValue) ? String(rawPhenotypeValue) : phenotypeMagnitude !== 0 && (phenotypeMagnitude < 1e-4 || phenotypeMagnitude >= 1e6) ? rawPhenotypeValue.toExponential(3) : rawPhenotypeValue.toLocaleString('en-US', { maximumSignificantDigits: 4 });
    assert.equal(evidence.lowerInspector.value, expectedPhenotype, 'lower inspector must match a real paused API phenotype value');
    evidence.screenshots.push(await captureViewport(width, 'inspector-lower'));
    evidence.targets.environment = await visibleTarget(page.locator('#resources').locator('..'));
    evidence.resources = await page.locator('#resources').evaluate(host => [...host.querySelectorAll('.resource')].map(row => ({ id: row.children[0].textContent, concentration: row.children[1].textContent })));
    assert.deepEqual(evidence.resources, layoutSnapshot.resources.map(resource => ({ id: resource.id, concentration: `${displayNumber(resource.concentration)} mol/m³` })));
    evidence.screenshots.push(await captureViewport(width, 'environment'));
    evidence.targets.events = await visibleTarget(page.locator('#events').locator('..'));
    evidence.events = await page.locator('#events').evaluate(host => [...host.querySelectorAll('.event')].map(row => [...row.children].map(item => item.textContent)));
    const shownEvents = layoutSnapshot.events.slice(-6).reverse().map(event => [String(event.tick), event.kind, event.children?.length ? `${event.parent_id || event.cell_id || '—'} → ${event.children.join(', ')}${event.mutated ? ' · mutated' : ''}` : event.cell_id || event.parent_id || '—']);
    assert.deepEqual(evidence.events, shownEvents);
    if (!shownEvents.length) assert.equal(await page.locator('#events .empty').textContent(), 'No events recorded.');
    evidence.screenshots.push(await captureViewport(width, 'events'));
    const filename = `viewer-${width}.png`;
    await page.screenshot({ path: resolve(output, filename), fullPage: true });
    report.screenshots.push(filename);
    evidence.after = await geometry();
    assertGeometry(evidence.after, width);
    assertInspector(await inspector());
    evidence.elapsedMs = Date.now() - layoutStartedMs;
    evidence.requests = requests.slice(requestsBeforeLayout);
    for (const request of evidence.requests) assert.ok(request.origin === origin && request.method === 'GET' && ['/api/state', '/api/history'].includes(request.path), `resize/scroll/Find must not add a model control or asset request: ${request.method} ${request.path}`);
    evidence.pollRequests = { state: evidence.requests.filter(request => request.path === '/api/state').length, history: evidence.requests.filter(request => request.path === '/api/history').length, stateCadenceMs: 650, historyMinimumMs: 4000 };
    assert.ok(Object.values(evidence.targets).some(target => target.scrollY !== target.beforeScroll.scrollY || target.sideScrollTop !== target.beforeScroll.sideScrollTop), 'responsive phase must produce actual document or sidebar scrolling');
  }
  const afterLayout = await state();
  for (const key of ['tick', 'running', 'dt_seconds', 'pacing', 'target_tps', 'sim_time', 'model', 'summary', 'cells', 'resources', 'residual', 'events']) assert.deepEqual(afterLayout[key], layoutSnapshot[key], `responsive actions must preserve paused API ${key}`);
  check('mobile static stage/readout/sidebar order; complete readout text, accessible controls and real upper/lower inspector/environment/event scrolls');
  check('real absent-ID then present-ID Find changes selection and matches the already confirmed paused API; resize/scroll/Find make no control POST or model change');
  check('desktop/mobile captions distinguish legacy schematic inventory from physical point-center projection');
  check('real full-page and viewport screenshots with no horizontal overflow at 1440/590/420/390/320');
  assert.deepEqual(report.pageErrors, []);
  assert.deepEqual(report.consoleErrors, []);
  check('no Chromium page/console errors');
  assert.equal((await execute('git', ['rev-parse', 'HEAD'])).stdout.trim(), head, 'HEAD changed during browser QA');
  assert.equal((await execute('git', ['rev-parse', 'HEAD^{tree}'])).stdout.trim(), tree, 'tree changed during browser QA');
  assert.equal((await execute('git', ['status', '--porcelain', '--untracked-files=no'])).stdout.trim(), '', 'tracked checkout changed during browser QA');
  assert.equal(stopping, false, 'browser QA was interrupted or timed out');
  report.status = 'passed';
  report.finalState = { tick: afterMaximum.tick, dt_seconds: afterMaximum.dt_seconds, pacing: afterMaximum.pacing };
}
try {
  await bounded(run(), 120000, 'complete protected browser QA');
} catch (error) {
  fail(error);
} finally {
  if (report.status === 'failed' && page && !page.isClosed()) {
    try {
      await bounded(page.screenshot({ path: resolve(output, 'failure.png'), fullPage: true, timeout: 3000 }), 4000, 'failure screenshot');
      report.screenshots.push('failure.png');
    } catch (error) { report.failureScreenshotError = String(error); }
  }
  if (tracing && context) {
    try {
      await bounded(context.tracing.stop({ path: resolve(output, 'trace.zip') }), 5000, 'trace publication');
      report.trace = 'trace.zip';
    } catch (error) { fail(error, 'trace cleanup'); }
  }
  if (browser) {
    try { await bounded(browser.close(), 5000, 'protected browser close'); }
    catch (error) { fail(error, 'browser cleanup'); }
  }
  if (host && host.exitCode === null && host.signalCode === null) {
    try {
      const exited = once(host, 'exit');
      host.kill('SIGTERM');
      try { await bounded(exited, 3000, 'host SIGTERM'); }
      catch {
        if (host.exitCode === null && host.signalCode === null) {
          host.kill('SIGKILL');
          await bounded(exited, 3000, 'host SIGKILL');
        }
      }
    } catch (error) { fail(error, 'host cleanup'); }
  }
  try { await bounded(rm(temporary, { recursive: true, force: true }), 5000, 'temporary data cleanup'); }
  catch (error) { fail(error, 'data cleanup'); }
  // Serialize only after cleanup so a teardown failure cannot retain a green report.
  await writeFile(resolve(output, 'host.log'), hostLog);
  await writeFile(resolve(output, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
}
