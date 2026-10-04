import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const html = readFileSync(new URL('../crates/liminis/src/cell-viewer.html', import.meta.url), 'utf8');
const script = html.match(/<script type="module">([\s\S]*?)<\/script>/)[1];
// Parse the complete shipped module, then exercise its actual control/frame functions.
new vm.Script(script);
const speedSource = script.match(/function speedMultiplier\(value\)\{[^\n]+\}/)[0];
const frameSource = script.match(/function screenFrame\(now\)\{[\s\S]*?\n      \}/)[0];
const controlSource = script.match(/async function control\(action,value\) \{[\s\S]*?\n      \}/)[0];
const physicalSource = script.match(/\/\/ BEGIN PHYSICAL HELPERS[^\n]*\n([\s\S]*?)\/\/ END PHYSICAL HELPERS/)[1];

function physical() {
  const context = vm.createContext({});
  vm.runInContext(`${physicalSource};globalThis.geometry={projectionAxes,projectionFrame,projectPhysicalCells,sliceInterval,sliceIncludes,hitCandidates,navigateVisibleIds}`, context);
  return context.geometry;
}

function inspector(view) {
  const inspectorSource = script.match(/function renderInspector\(\)\{[\s\S]*?\n      \}/)[0];
  const pairSource = script.match(/function addPair\(list,key,value\)\{[^\n]+\}/)[0];
  const micrometerSource = script.match(/function micrometers\(value,digits=6\)\{[\s\S]*?\n      \}/)[0];
  const element = () => ({ children: [], style: {}, textContent: '', append(...nodes) { this.children.push(...nodes); }, replaceChildren() { this.children = []; this.textContent = ''; } });
  const elements = new Map(['cell-detail', 'selection-state'].map(id => [id, element()]));
  const context = vm.createContext({ view, $: key => elements.get(key), document: { createElement: element }, color: () => '#86dfb7', format: String, objectPairs: () => {} });
  vm.runInContext(`${physicalSource}\n${pairSource}\n${micrometerSource}\n${inspectorSource};globalThis.inspect=renderInspector`, context);
  return { elements, render: context.inspect };
}

function close(actual, expected) {
  assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} != ${expected}`);
}

test('all physical planes letterbox a nonsquare box with equal units and right/up axes', () => {
  const geometry = physical();
  const dimensions = [40e-6, 20e-6, 10e-6];
  const viewport = { left: 10, top: 20, width: 300, height: 200 };
  for (const [plane, axes, width, height] of [
    ['xy', [0, 1, 2], 300, 150],
    ['xz', [0, 2, 1], 300, 75],
    ['yz', [1, 2, 0], 300, 150],
  ]) {
    const origin = [0, 0, 0];
    const right = [...origin]; right[axes[0]] = 1e-6;
    const up = [...origin]; up[axes[1]] = 1e-6;
    const result = geometry.projectPhysicalCells([
      { id: '1', position_m: origin },
      { id: '2', position_m: dimensions },
      { id: '3', position_m: dimensions.map(length => length / 2) },
      { id: '4', position_m: right },
      { id: '5', position_m: up },
    ], dimensions, plane, viewport);
    const frame = result.frame;
    assert.deepEqual(Array.from(frame.axes), axes);
    close(frame.width, width); close(frame.height, height);
    close(frame.left, 10 + (300 - width) / 2);
    close(frame.top, 20 + (200 - height) / 2);
    close(result.markers[0].x, frame.left); close(result.markers[0].y, frame.top + height);
    close(result.markers[1].x, frame.left + width); close(result.markers[1].y, frame.top);
    close(result.markers[2].x, 160); close(result.markers[2].y, 120);
    close(result.markers[3].x - result.markers[0].x, result.markers[0].y - result.markers[4].y);
    assert.ok(result.markers[3].x > result.markers[0].x);
    assert.ok(result.markers[4].y < result.markers[0].y);
  }
});

test('slices include both center bounds on each hidden axis and full projection is default', () => {
  const geometry = physical();
  const dimensions = [40e-6, 20e-6, 10e-6];
  const viewport = { left: 0, top: 0, width: 300, height: 200 };
  for (const plane of ['xy', 'xz', 'yz']) {
    const hidden = geometry.projectionAxes(plane)[2];
    const center = dimensions[hidden] / 2, thickness = dimensions[hidden] / 2;
    const low = center - thickness / 2, high = center + thickness / 2;
    const cells = [low, high, low - dimensions[hidden] / 16, high + dimensions[hidden] / 16].map((value, index) => {
      const position = dimensions.map(length => length / 2); position[hidden] = value;
      return { id: String(index + 1), position_m: position };
    });
    const sliced = geometry.projectPhysicalCells(cells, dimensions, plane, viewport, { enabled: true, center, thickness });
    assert.deepEqual(Array.from(sliced.markers, marker => marker.id), ['1', '2']);
    assert.equal(geometry.projectPhysicalCells(cells, dimensions, plane, viewport).markers.length, 4);
    const atCenter = dimensions.map(length => length / 2);
    assert.equal(geometry.sliceIncludes(atCenter, hidden, { enabled: true, center, thickness: 0 }, dimensions[hidden]), true);
    assert.throws(() => geometry.projectPhysicalCells(cells, dimensions, plane, viewport, { enabled: true, center, thickness: -1 }), /slice/);
  }
});

test('slice interval intersects the box and zero thickness filters the exact represented center plane', () => {
  const geometry = physical(), length = 20e-6;
  assert.deepEqual(Array.from(geometry.sliceInterval(length, { enabled: false })), [0, length]);
  const lowClipped = geometry.sliceInterval(length, { enabled: true, center: 2e-6, thickness: 10e-6 });
  assert.equal(lowClipped[0], 0); assert.ok(Math.abs(lowClipped[1] - 7e-6) < 1e-20);
  const highClipped = geometry.sliceInterval(length, { enabled: true, center: 18e-6, thickness: 10e-6 });
  assert.ok(Math.abs(highClipped[0] - 13e-6) < 1e-20); assert.equal(highClipped[1], length);
  const center = 7.123456789e-6, plane = { enabled: true, center, thickness: 0 };
  assert.deepEqual(Array.from(geometry.sliceInterval(length, plane)), [center, center]);
  assert.equal(geometry.sliceIncludes([0, center, 0], 1, plane, length), true);
  assert.equal(geometry.sliceIncludes([0, center - 1e-20, 0], 1, plane, length), false);
  assert.equal(geometry.sliceIncludes([0, center + 1e-20, 0], 1, plane, length), false);
  assert.throws(() => geometry.sliceInterval(length, { enabled: true, center: length * 2, thickness: 0 }), /slice/);
});

test('invalid positions in any axis are rejected instead of being drawn at a fallback zero', () => {
  const geometry = physical();
  const dimensions = [40e-6, 20e-6, 10e-6];
  const viewport = { left: 0, top: 0, width: 300, height: 200 };
  for (const invalid of [undefined, null, [], [0, 0], [0, 0, NaN], [0, Infinity, 0], ['0', 0, 0], [-1e-20, 0, 0], [0, 0, 11e-6]]) {
    assert.throws(() => geometry.projectPhysicalCells([{ id: '1', position_m: invalid }], dimensions, 'xy', viewport), /position/);
  }
  for (const invalid of [undefined, [0, 1, 1], [-1, 1, 1], [Infinity, 1, 1], ['1', 1, 1]]) {
    assert.throws(() => geometry.projectionFrame(invalid, 'xy', viewport), /dimensions/);
  }
  assert.throws(() => geometry.projectionFrame(dimensions, 'unknown', viewport), /plane/);
  assert.throws(() => geometry.projectPhysicalCells([{ id: 18446744073709551615, position_m: [0, 0, 0] }], dimensions, 'xy', viewport), /exact cell ID/);
});

test('coincident centers stay coincident and hit candidates sort by distance then exact large ID', () => {
  const geometry = physical();
  const dimensions = [40e-6, 20e-6, 10e-6];
  const cells = ['18446744073709551615', '9007199254740993', '9007199254740992'].map(id => ({ id, position_m: [20e-6, 10e-6, 5e-6] }));
  const { markers } = geometry.projectPhysicalCells(cells, dimensions, 'xy', { left: 0, top: 0, width: 300, height: 200 });
  for (const marker of markers) { close(marker.x, 150); close(marker.y, 100); }
  const ids = hits => Array.from(hits, hit => hit.id);
  assert.deepEqual(ids(geometry.hitCandidates(markers, 150, 100)), ['9007199254740992', '9007199254740993', '18446744073709551615']);
  const withNearer = [...markers, { id: '18446744073709551614', x: 154, y: 100, r: 8 }];
  assert.deepEqual(ids(geometry.hitCandidates(withNearer, 154, 100)), ['18446744073709551614', '9007199254740992', '9007199254740993', '18446744073709551615']);
  assert.equal(geometry.hitCandidates(markers, 159, 100).length, 0);
  assert.equal(geometry.hitCandidates(markers, 158, 100).length, 3);
});

test('keyboard navigation uses visible exact IDs and handles a retained absent selection', () => {
  const geometry = physical();
  const visible = ['18446744073709551615', '9007199254740993', '9007199254740992'];
  assert.equal(geometry.navigateVisibleIds(visible, 'outside-slice', 1), '9007199254740992');
  assert.equal(geometry.navigateVisibleIds(visible, 'not-present', -1), '18446744073709551615');
  assert.equal(geometry.navigateVisibleIds(visible, '9007199254740992', 1), '9007199254740993');
  assert.equal(geometry.navigateVisibleIds(visible, '18446744073709551615', 1), '9007199254740992');
  assert.equal(geometry.navigateVisibleIds([], 'selected', 1), null);
});

test('inspector retains exact selection outside a slice and after disappearance, with saved xyz and shown tick', () => {
  const id = '18446744073709551615';
  const view = { selected: id, drawnTick: 42, visibleIds: [], projectionError: null,
    state: { chamber_format: 2, tick: 42, model: { dimensions_m: [40e-6, 20e-6, 10e-6] },
      cells: [{ id, position_m: [10e-6, 5e-6, 2e-6], genome_key: 'genotype', generation: 1, birth_tick: 10 }] } };
  const { elements, render } = inspector(view);
  render();
  assert.equal(elements.get('selection-state').textContent, 'outside slice');
  const list = elements.get('cell-detail').children.find(node => node.className === 'kv object-grid');
  const pairs = new Map();
  for (let i = 0; i < list.children.length; i += 2) pairs.set(list.children[i].textContent, list.children[i + 1].textContent);
  assert.equal(pairs.get('id'), id); assert.equal(pairs.get('shown snapshot tick'), '42');
  assert.equal(pairs.get('x center'), '10 µm'); assert.equal(pairs.get('y center'), '5 µm'); assert.equal(pairs.get('z center'), '2 µm');
  view.state.cells = []; view.state.tick = 43; view.drawnTick = 43;
  render();
  assert.equal(view.selected, id);
  assert.equal(elements.get('selection-state').textContent, 'not present');
  assert.equal(elements.get('cell-detail').textContent, `ID ${id} is not present in the shown snapshot at tick 43.`);
});

test('unknown exact ID is not present without inferring that it was ever born or died', () => {
  const id = '18446744073709551615';
  const view = { selected: id, drawnTick: 9, visibleIds: ['9007199254740993'], projectionError: null,
    state: { chamber_format: 2, tick: 9, cells: [{ id: '9007199254740993' }] } };
  const { elements, render } = inspector(view);
  render();
  assert.equal(view.selected, id);
  assert.equal(elements.get('selection-state').textContent, 'not present');
  assert.equal(elements.get('cell-detail').textContent, `ID ${id} is not present in the shown snapshot at tick 9.`);
});

test('division removes a selected parent from the snapshot without claiming death or selecting a daughter', () => {
  const parent = '9007199254740993', daughters = ['9007199254740994', '9007199254740995'];
  const view = { selected: parent, drawnTick: 10, visibleIds: [parent], projectionError: null,
    state: { chamber_format: 1, tick: 10, cells: [{ id: parent, genome_key: 'genotype', generation: 1, birth_tick: 2 }] } };
  const { elements, render } = inspector(view);
  render();
  assert.equal(elements.get('selection-state').textContent, 'generation 1');
  view.state = { chamber_format: 1, tick: 11, cells: daughters.map(id => ({ id, parent_id: parent })),
    events: [{ kind: 'division', tick: 11, parent_id: parent, children: daughters }] };
  view.drawnTick = 11; view.visibleIds = daughters;
  render();
  assert.equal(view.selected, parent);
  assert.equal(elements.get('selection-state').textContent, 'not present');
  assert.equal(elements.get('cell-detail').textContent, `ID ${parent} is not present in the shown snapshot at tick 11.`);
});

test('state, displayed tick, hit map and inspector commit before a delayed independent history request', async () => {
  const commitSource = script.match(/function commitState\(state\)\{[\s\S]*?\n      \}/)[0];
  const pollSource = script.match(/async function poll\(\)\{[\s\S]*?\n      \}/)[0];
  const state = { kind: 'cells', tick: 8, cells: [{ id: '18446744073709551615' }], persistence: { enabled: true, run_id: 'run', session_id: 'session' } };
  const view = { state: { tick: 7 }, drawnTick: 7, hits: [{ id: 'old' }], selected: state.cells[0].id, pollBusy: false, pollSerial: 0, identity: 'run\nsession', slice: {}, layout: new Map(), usedSlots: new Set() };
  const elements = new Map(['offline', 'live', 'live-label'].map(id => [id, { dataset: {} }]));
  const order = [];
  let historyFinish;
  const history = new Promise(resolve => { historyFinish = resolve; });
  let displayedTick, inspectedTick;
  const context = vm.createContext({ view, $: id => elements.get(id), json: async () => state,
    retireLayout: () => {}, mergeHistory: () => {}, liveSample: value => value, configureProjection: () => {},
    renderState: () => { displayedTick = view.state.tick; order.push('readouts'); },
    drawChamber: () => { view.hits = view.state.cells; view.drawnTick = view.state.tick; order.push('canvas/hits'); },
    renderInspector: () => { inspectedTick = view.drawnTick; assert.equal(view.selected, view.hits[0].id); order.push('inspector'); },
    renderCandidates: () => {},
    loadHistory: () => { assert.equal(displayedTick, 8); assert.equal(inspectedTick, 8); assert.equal(view.hits[0].id, state.cells[0].id); order.push('history'); return history; },
  });
  vm.runInContext(`${commitSource}\n${pollSource};globalThis.pollState=poll`, context);
  await context.pollState();
  assert.equal(view.state, state); assert.equal(view.drawnTick, 8); assert.equal(view.pollBusy, false);
  assert.deepEqual(order, ['readouts', 'canvas/hits', 'inspector', 'history']);
  assert.equal(view.selected, '18446744073709551615');
  const candidates = [{ id: view.selected }];
  view.candidates = candidates; view.candidateTick = 8;
  await context.pollState();
  assert.equal(view.candidates, candidates, 'paused polls keep the candidate list for the same drawn tick');
  assert.equal(view.candidateTick, 8);
  historyFinish();
});

test('manual multiplier accepts positive finite values without the former TPS clamp', () => {
  const context = vm.createContext({});
  vm.runInContext(`${speedSource};globalThis.parseSpeed=speedMultiplier`, context);
  for (const value of ['0.00001', '1', '90', '1000', '1e100']) {
    assert.equal(context.parseSpeed(value), Number(value));
  }
  for (const value of ['', ' ', '0', '-1', 'NaN', 'Infinity', '-Infinity', '1e999']) {
    assert.throws(() => context.parseSpeed(value), /positive finite/);
  }
});

test('30/60 FPS redraws held state without requests, ticks, or queued snapshots', () => {
  for (const fps of [30, 60]) {
    const confirmed = Object.freeze({ tick: 7, cells: Object.freeze([{ id: '7', mass: 42 }]) });
    const view = { screenFps: fps, frameAt: null, state: confirmed };
    let draws = 0;
    let scheduled;
    const context = vm.createContext({ view,
      drawChamber: () => { draws++; assert.equal(view.state, confirmed); },
      requestAnimationFrame: (callback) => { scheduled = callback; },
      fetch: () => assert.fail('screen frames must not make HTTP requests'),
    });
    vm.runInContext(`${frameSource};globalThis.frame=screenFrame`, context);
    for (let now = 0; now < 1000; now += 1000 / 120) context.frame(now);
    assert.ok(draws >= fps - 1 && draws <= fps + 1, `${fps} FPS: ${draws} draws`);
    assert.equal(view.state.tick, 7);
    assert.equal(scheduled, context.frame);
    assert.deepEqual(Object.keys(view).sort(), ['frameAt', 'screenFps', 'state']);
  }
});

test('speed/maximum controls use multiplier contract, never screen FPS', async () => {
  const bodies = [];
  const context = vm.createContext({ fetch: async (url, options) => {
    assert.equal(url, '/api/control');
    bodies.push(JSON.parse(options.body));
    return { ok: true, json: async () => ({ ok: true }) };
  } });
  vm.runInContext(`${controlSource};globalThis.send=control`, context);
  await context.send('speed', 1);
  await context.send('maximum');
  assert.deepEqual(bodies, [{ action: 'speed', value: 1 }, { action: 'maximum' }]);
});

test('retired IDs release layout slots while living cells keep confirmed positions', () => {
  const source = script.match(/function retireLayout\(cells\)\{[\s\S]*?\n      \}/)[0];
  const view = { layout: new Map([['old', 1], ['living', 2]]), usedSlots: new Set([1, 2]) };
  const context = vm.createContext({ view });
  vm.runInContext(`${source};globalThis.retire=retireLayout`, context);
  context.retire([{ id: 'living' }, { id: 'new' }]);
  assert.deepEqual([...view.layout], [['living', 2]]);
  assert.deepEqual([...view.usedSlots], [2]);
  for (let i = 0; i < 1000; i++) {
    view.layout.set(`retired-${i}`, 3);
    view.usedSlots.add(3);
    context.retire([{ id: 'living' }]);
    assert.equal(view.layout.size, 1);
    assert.equal(view.usedSlots.size, 1);
    assert.equal(view.layout.get('living'), 2);
  }
});

test('queued save stays visible until writer confirms the requested checkpoint', () => {
  const source = script.match(/function renderSave\(\)\{[^\n]+\}/)[0];
  const elements = new Map(['save', 'save-status'].map(id => [id, { dataset: {} }]));
  const view = { state: { tick: 20, save_pending: true, persistence: { enabled: true, saving: false, saved_tick: 20 } },
    saveRequest: { tick: 20, serial: 1 }, pollSerial: 2, saveError: null };
  const context = vm.createContext({ view, $: id => elements.get(id), finite: Number });
  vm.runInContext(`${source};globalThis.render=renderSave`, context);
  context.render();
  assert.notEqual(view.saveRequest, null);
  assert.equal(elements.get('save').disabled, true);
  assert.equal(elements.get('save-status').textContent, 'queued 20');
  view.state.save_pending = false;
  context.render();
  assert.equal(view.saveRequest, null);
  assert.equal(elements.get('save').disabled, false);
  assert.equal(elements.get('save-status').textContent, 'saved 20');
});
