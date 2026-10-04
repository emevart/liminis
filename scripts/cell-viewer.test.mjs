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
