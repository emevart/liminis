import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import * as THREE from '../crates/liminis/src/vendor/three-r180/three.module.js';
import { CENTER_PIXELS, createPhysical3D, physicalScale, physicalSlice, projectCenter, tapGesture, validateCenter, visibilityOf } from '../crates/liminis/src/cell-viewer-3d.mjs';

const html = readFileSync(new URL('../crates/liminis/src/cell-viewer.html', import.meta.url), 'utf8');
const script = html.match(/<script type="module">([\s\S]*?)<\/script>/)[1];
new vm.Script(script);
const source = name => (script.match(new RegExp(`(?:async )?function ${name}\\([^\\n]*\\)\\{[^\\n]*\\}$`, 'm')) ?? script.match(new RegExp(`(?:async )?function ${name}\\([^\\n]*\\)\\{[\\s\\S]*?\\n      \\}`)))[0];
const dimensions = [40e-6, 20e-6, 10e-6];
const viewport = { left: 24, top: 180, width: 700, height: 340, canvasWidth: 748, canvasHeight: 580, pixelRatio: 1 };
const palette = ['#86dfb7', '#72d5df', '#ff8f83', '#dfbd64', '#bb9bdf', '#dfe7da', '#90aeed', '#e0a9c5'];
const cell = (id, position = dimensions.map(value => value / 2)) => ({ id, position_m: position, genome_key: 'K+1' });
const state = (cells, tick = 7) => ({ chamber_format: 2, tick, model: { dimensions_m: dimensions }, cells });
const options = selected => ({ axis: 2, slice: { enabled: false }, layers: { centers: true, box: true, slice: true, opacity: 1 }, selected });
const close = (actual, expected, tolerance = 1e-9) => assert.ok(Math.abs(actual - expected) < tolerance, `${actual} != ${expected}`);

// Инъекция заменяет только native allocation/input. Камера, matrices, sprites,
// shared materials и disposal events — настоящие vendored Three.js objects.
function fixture() {
  class Canvas extends EventTarget {
    getBoundingClientRect() { return { left: 10, top: 20, width: 748, height: 580 }; }
  }
  class Controls extends THREE.EventDispatcher {
    constructor(camera) { super(); this.camera = camera; this.target = new THREE.Vector3(); this.disposeCount = 0; controls = this; }
    update() { this.camera.lookAt(this.target); }
    saveState() {}
    dispose() { this.disposeCount++; }
  }
  let controls, camera, scene, changes = 0, losses = 0, sizes = 0;
  const renderer = {
    info: { memory: { geometries: 0, textures: 0 } }, disposeCount: 0,
    setClearColor() {}, setPixelRatio(value) { this.ratio = value; }, getPixelRatio() { return this.ratio; },
    setSize() { sizes++; }, setViewport(...values) { this.viewport = values; }, clear() {},
    render(value, currentCamera) { scene = value; camera = currentCamera; },
    getContext() { throw new Error('Unit test has no native WebGL'); },
    dispose() { this.disposeCount++; },
  };
  const canvas = new Canvas();
  const view = createPhysical3D({ THREE, OrbitControls: Controls, canvas, palette, colorIndex: () => 0,
    onChange: () => changes++, onLost: () => losses++, rendererFactory: () => renderer });
  return { view, canvas, renderer, get controls() { return controls; }, get camera() { return camera; }, get scene() { return scene; }, get changes() { return changes; }, get losses() { return losses; }, get sizes() { return sizes; } };
}

test('uniform SI scale and isometric orthographic centers agree with an independent analytic camera basis', () => {
  const f = fixture(), corners = Array.from({ length: 8 }, (_, index) => dimensions.map((length, axis) => index >> axis & 1 ? length : 0));
  const hits = f.view.draw(state(corners.map((point, index) => cell(String(index), point))), options(), viewport);
  const audit = f.view.audit(), scale = 1 / 40e-6;
  assert.equal(audit.sceneScale, scale); assert.equal(audit.camera.zoom, 1);
  const right = [1 / Math.sqrt(2), 0, -1 / Math.sqrt(2)], up = [-1 / Math.sqrt(6), 2 / Math.sqrt(6), -1 / Math.sqrt(6)];
  const center = dimensions.map(length => length / 2), dot = (point, basis) => point.reduce((sum, value, axis) => sum + (value - center[axis]) * scale * basis[axis], 0);
  for (let index = 0; index < corners.length; index++) {
    close(hits[index].x, viewport.left + viewport.width / 2 + dot(corners[index], right) * viewport.width / (audit.camera.right - audit.camera.left));
    close(hits[index].y, viewport.top + viewport.height / 2 - dot(corners[index], up) * viewport.height / (audit.camera.top - audit.camera.bottom));
  }
  // Common 1µm has the same scene length along each axis; glyph is CSS pixels.
  const centers = f.scene.children[2];
  for (const sprite of centers.children) close(sprite.scale.x * viewport.height / (audit.camera.top - audit.camera.bottom), CENTER_PIXELS);
  assert.equal(f.controls.enableDamping, false); assert.equal(f.controls.autoRotate, false);
  assert.equal(f.controls.listenToKeyEvents, undefined);
  f.view.dispose();
});

test('all three axis views and nonsquare viewport preserve equal units, zoom and clip edges', () => {
  const scale = physicalScale(dimensions), target = new THREE.Vector3(...dimensions.map(value => value * scale / 2));
  for (const [rightAxis, upAxis, back] of [[0, 1, [0, 0, 4]], [0, 2, [0, -4, 0]], [1, 2, [4, 0, 0]]]) {
    const camera = new THREE.OrthographicCamera(-2, 2, 1, -1, .01, 100);
    camera.position.copy(target).add(new THREE.Vector3(...back)); camera.up.set(0, 0, 0).setComponent(upAxis, 1); camera.lookAt(target); camera.updateMatrixWorld();
    for (const zoom of [1, 2]) {
      camera.zoom = zoom; camera.updateProjectionMatrix();
      const matrix = new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse).elements;
      const box = { left: 10, top: 20, width: 800, height: 400 }, center = dimensions.map(value => value / 2);
      const origin = projectCenter(center, scale, matrix, box), right = [...center], up = [...center]; right[rightAxis] += 1e-6; up[upAxis] += 1e-6;
      const x = projectCenter(right, scale, matrix, box), y = projectCenter(up, scale, matrix, box);
      close(origin.x, 410); close(origin.y, 220); close(x.x - origin.x, 5 * zoom); close(origin.y - y.y, 5 * zoom);
    }
  }
  const matrix = new THREE.Matrix4().identity().elements, box = { left: 10, top: 20, width: 800, height: 400 };
  for (const position of [[-1, -1, -1], [1, 1, 1]]) assert.equal(projectCenter(position, 1, matrix, box).inView, true);
  assert.equal(projectCenter([1.00001, 0, 0], 1, matrix, box).inView, false);
});

test('inclusive clipped slices, hidden layers and outside-view states never replace saved coordinates', () => {
  for (let axis = 0; axis < 3; axis++) {
    const length = dimensions[axis], center = length / 4;
    assert.deepEqual(physicalSlice(dimensions, axis, { enabled: true, center, thickness: length }), [0, center + length / 2]);
    const interval = physicalSlice(dimensions, axis, { enabled: true, center, thickness: 0 }), point = [0, 0, 0]; point[axis] = center;
    assert.deepEqual(interval, [center, center]);
    const layers = { centers: true, opacity: 1 };
    assert.equal(visibilityOf(point, axis, interval, layers, { inView: true }), 'visible');
    assert.equal(visibilityOf(point, axis, interval, { ...layers, opacity: 0 }, { inView: true }), 'centers layer hidden');
    assert.equal(visibilityOf(point, axis, interval, layers, { inView: false }), 'outside current view');
    point[axis] += length / 100;
    assert.equal(visibilityOf(point, axis, interval, layers, { inView: true }), 'outside slice');
  }
  for (const position of [null, [], [0, 0, NaN], ['0', 0, 0], [-1e-20, 0, 0], [0, 0, 11e-6]]) assert.throws(() => validateCenter(cell('1', position), dimensions), /position/);
  assert.throws(() => validateCenter(cell(9007199254740993), dimensions), /exact cell ID/);
  assert.throws(() => physicalScale([1, 0, 1]), /dimensions/);
});

test('coincident daughters have identical pixels and exact large IDs use distance then BigInt ordering', () => {
  const f = fixture(), ids = ['18446744073709551615', '9007199254740993', '9007199254740992'];
  const hits = f.view.draw(state(ids.map(id => cell(id))), options(ids[0]), viewport);
  for (const hit of hits) { close(hit.x, hits[0].x); close(hit.y, hits[0].y); }
  const helpers = script.match(/\/\/ BEGIN PHYSICAL HELPERS[^\n]*\n([\s\S]*?)\/\/ END PHYSICAL HELPERS/)[1], context = vm.createContext({});
  vm.runInContext(`${helpers};globalThis.hit=hitCandidates;globalThis.nav=navigateVisibleIds`, context);
  assert.deepEqual(Array.from(context.hit(hits, hits[0].x, hits[0].y), hit => hit.id), [...ids].reverse());
  assert.equal(context.nav(ids, ids[2], 1), ids[1]);
  assert.equal(f.view.audit().resources.selectionSprites, 1); f.view.dispose();
});

test('renderer hit map and selection halo follow actual slice/layer/camera visibility on the same tick', () => {
  const f = fixture(), current = state([cell('1'), cell('2', [0, 0, 0])]), settings = options('1');
  f.view.draw(current, settings, viewport);
  settings.layers.centers = false; assert.equal(f.view.draw(current, settings, viewport).length, 0); assert.equal(f.view.visibility('1'), 'centers layer hidden'); assert.equal(f.view.audit().resources.selectionSprites, 0);
  settings.layers.centers = true; settings.slice = { enabled: true, center: 0, thickness: 0 };
  assert.deepEqual(f.view.draw(current, settings, viewport).map(hit => hit.id), ['2']); assert.equal(f.view.visibility('1'), 'outside slice');
  settings.slice.enabled = false; f.camera.zoom = 100; f.camera.updateProjectionMatrix(); f.controls.target.set(0, 0, 0); f.camera.position.set(2, 2, 2); f.controls.update(); f.controls.dispatchEvent({ type: 'change' });
  f.view.draw(current, settings, viewport); assert.equal(f.view.visibility('1'), 'outside current view'); assert.equal(f.view.audit().resources.selectionSprites, 0);
  f.view.reset(); assert.equal(f.view.draw(current, settings, viewport).length, 2); assert.equal(f.view.audit().resources.selectionSprites, 1);
  assert.equal(current.tick, 7); assert.equal(settings.selected, '1'); f.view.dispose();
});

test('sprite map and shared resources follow only current population, survive toggles and dispose once', () => {
  const f = fixture(), first = state([cell('1'), cell('2')]);
  f.view.draw(first, options('1'), viewport);
  const attributes = f.scene.children[1].children.map(line => line.geometry.getAttribute('position'));
  const disposal = new Map();
  f.scene.traverse(object => { for (const item of [object.geometry, object.material, object.material?.map].filter(Boolean)) if (!disposal.has(item)) { disposal.set(item, 0); item.addEventListener('dispose', () => disposal.set(item, disposal.get(item) + 1)); } });
  const initialChanges = f.changes;
  f.view.draw(first, options('1'), viewport); assert.equal(f.changes, initialChanges); assert.equal(f.sizes, 1);
  for (let index = 0; index < 200; index++) {
    f.view.draw(state([cell(String(index + 100)), cell('2')]), options(), viewport);
    assert.equal(f.view.audit().resources.sprites, 2); assert.equal(f.view.visibility('1'), 'not present');
  }
  for (let index = 0; index < 10; index++) { f.view.clear(); assert.equal(f.view.audit().resources.sprites, 0); f.view.draw(first, options(), viewport); }
  assert.deepEqual(f.scene.children[1].children.map(line => line.geometry.getAttribute('position')), attributes);
  assert.deepEqual(Object.fromEntries(['sprites', 'materials', 'textures', 'geometries'].map(key => [key, f.view.audit().resources[key]])), { sprites: 2, materials: 15, textures: 2, geometries: 7 });
  assert.equal(f.view.audit().resources.selectionSprites, 0);
  f.view.draw(first, options(), { ...viewport, width: 350 }); assert.ok(f.changes > initialChanges, 'viewport change invalidates geometry without a model tick');
  f.view.dispose(); f.view.dispose();
  assert.equal(f.renderer.disposeCount, 1); assert.equal(f.controls.disposeCount, 1);
  for (const count of disposal.values()) assert.equal(count, 1);
  assert.equal(f.view.audit().resources.sprites, 0); assert.equal(f.view.audit().resources.materials, 0);
  assert.throws(() => f.view.draw(first, options(), viewport), /unavailable/);
});

test('actual context-loss listener invalidates 3D once and constructor failures release allocation', () => {
  const f = fixture(); f.view.draw(state([cell('1')]), options(), viewport);
  const event = new Event('webglcontextlost', { cancelable: true }); f.canvas.dispatchEvent(event);
  assert.equal(event.defaultPrevented, true); assert.equal(f.losses, 1); assert.equal(f.controls.enabled, false);
  f.canvas.dispatchEvent(new Event('webglcontextlost', { cancelable: true })); assert.equal(f.losses, 1);
  assert.equal(f.view.audit().resources.lost, true); assert.throws(() => f.view.draw(state([]), options(), viewport), /unavailable/);
  f.view.dispose(); f.canvas.dispatchEvent(new Event('webglcontextlost', { cancelable: true })); assert.equal(f.losses, 1);
  let released = 0;
  assert.throws(() => createPhysical3D({ THREE, OrbitControls: class { constructor() { throw new Error('controls failed'); } }, canvas: f.canvas, palette, rendererFactory: () => ({ setClearColor() {}, dispose() { released++; } }) }), /controls failed/);
  assert.equal(released, 1);
});

test('tap uses maximum travel and time; drag back to origin, camera change, cancellation and multitouch do not select', () => {
  const tap = tapGesture(), event = (x = 0, y = 0, pointerId = 1) => ({ button: 0, isPrimary: true, pointerId, clientX: x, clientY: y });
  tap.down(event(), 0, 3); assert.equal(tap.up(event(2), 100, 3), true);
  tap.down(event(), 0, 3); tap.move(event(10)); tap.move(event()); assert.equal(tap.up(event(), 100, 3), false);
  tap.down(event(), 0, 3); assert.equal(tap.up(event(), 451, 3), false);
  tap.down(event(), 0, 3); assert.equal(tap.up(event(), 100, 4), false);
  tap.down(event(), 0, 3); tap.changed(); assert.equal(tap.up(event(), 100, 3), false);
  tap.down(event(), 0, 3); tap.cancel(); assert.equal(tap.up(event(), 100, 3), false);
  tap.down(event(), 0, 3); tap.down(event(0, 0, 2), 10, 3); assert.equal(tap.up(event(), 100, 3), false);
  tap.down({ ...event(), button: 2 }, 0, 3); assert.equal(tap.up(event(), 100, 3), false);
});

function activationHarness() {
  const elements = new Map(), element = id => elements.get(id) ?? elements.set(id, { hidden: true, focus() {}, querySelector() { return {}; } }).get(id);
  const view = { state: state([cell('1')], 1), activationSerial: 0, requestedMode: '2d', mode: '2d', contextCreations: 0, geometryRevision: 0, candidates: [], three: null };
  let resolve, draws = [], creates = 0, lostCallback;
  const dependencies = new Promise(done => { resolve = done; });
  const module = { createPhysical3D(options) { creates++; lostCallback = options.onLost; return { clear() {}, dispose() { this.disposed = true; } }; }, tapGesture };
  const context = vm.createContext({ view, $: element, palette, hash: () => 0, load3DDependencies: () => dependencies,
    configureProjection() {}, renderInspector() {}, renderCandidates() {}, drawChamber() { draws.push(view.state); view.drawnTick = view.state.tick; },
  });
  vm.runInContext(`${source('invalidateGeometry')}\n${source('deactivate3D')}\n${source('fallback3D')}\n${source('activate3D')};globalThis.activate=activate3D;globalThis.deactivate=deactivate3D;globalThis.invalidate=invalidateGeometry`, context);
  return { view, context, elements, draws, resolve: () => resolve([module, THREE, { OrbitControls: {} }]), get creates() { return creates; }, get lost() { return lostCallback; } };
}

test('lazy activation draws latest committed snapshot; cancellation creates no renderer and loss returns to latest 2D', async () => {
  const f = activationHarness(), pending = f.context.activate(), latest = state([cell('9007199254740993')], 9);
  f.view.state = latest; f.resolve(); await pending;
  assert.equal(f.view.mode, '3d'); assert.equal(f.draws[0], latest); assert.equal(f.view.contextCreations, 1);
  f.view.state = state([cell('2')], 10); f.lost(new Error('WebGL context lost'));
  assert.equal(f.view.mode, '2d'); assert.equal(f.draws.at(-1), f.view.state); assert.match(f.elements.get('view-3d-status').textContent, /showing 2D/);
  const cancelled = activationHarness(), waiting = cancelled.context.activate(); cancelled.context.deactivate(); cancelled.resolve(); await waiting;
  assert.equal(cancelled.creates, 0); assert.equal(cancelled.view.mode, '2d'); assert.equal(cancelled.draws.length, 0);
});

test('camera and setting revisions synchronously retire old candidates on a held tick', () => {
  const f = activationHarness(); f.view.drawnTick = 7; f.view.drawnRevision = 1; f.view.geometryRevision = 1; f.view.candidateRevision = 1; f.view.candidateTick = 7; f.view.candidates = [cell('1'), cell('2')]; f.view.hits = f.view.candidates;
  f.context.invalidate();
  assert.equal(f.view.geometryRevision, 2); assert.equal(f.view.drawnTick, 7); assert.equal(f.view.drawnRevision, 1);
  assert.equal(f.view.candidates.length, 0); assert.equal(f.view.hits.length, 0); assert.equal(f.view.candidateTick, null); assert.equal(f.view.candidateRevision, null);
});

test('persistence-disabled same-tick seed/config replacement retires candidates and an active tap before drawing new centers', () => {
  for (const [key, replacement, enabledBefore = false] of [['seed', '2'], ['config_hash', 'b'.repeat(64)], ['world_format_version', 31], ['chamber_format', 1], ['persistence', { enabled: false }, true]]) {
    const before = { ...state([cell('1'), cell('2')], 0), seed: '1', config_hash: 'a'.repeat(64), world_format_version: 30, persistence: enabledBefore ? { enabled: true, run_id: 'run', session_id: 'session' } : { enabled: false } };
    const after = { ...before, [key]: replacement, cells: [cell('1', [0, 0, 0]), cell('2', dimensions)] }, tap = tapGesture();
    const event = { button: 0, isPrimary: true, pointerId: 1, clientX: 100, clientY: 100 };
    const view = { state: before, identity: enabledBefore ? 'run\nsession' : null, geometryRevision: 9, drawnRevision: 9, drawnTick: 0, pollSerial: 0, tap,
      candidates: [cell('1'), cell('2')], candidateTick: 0, candidateRevision: 9, hits: [cell('1'), cell('2')], history: [], selected: '1', layout: new Map(), usedSlots: new Set(), slice: {} };
    tap.down(event, 0, 9);
    let draws = 0;
    const context = vm.createContext({ view, $: () => ({ dataset: {} }), retireLayout() {}, mergeHistory() {}, liveSample: value => value, configureProjection() {}, renderState() {}, renderInspector() {}, renderCandidates() {},
      drawChamber() { draws++; assert.equal(view.state, after); assert.equal(view.candidates.length, 0); assert.equal(view.hits.length, 0); assert.equal(view.geometryRevision, 10); view.drawnTick = after.tick; view.drawnRevision = view.geometryRevision; },
    });
    vm.runInContext(`${source('invalidateGeometry')}\n${source('commitState')};globalThis.commit=commitState`, context);
    context.commit(after);
    assert.equal(draws, 1); assert.equal(view.identity, null); assert.equal(view.candidateTick, null); assert.equal(view.candidateRevision, null); assert.equal(tap.up(event, 100, view.geometryRevision), false);
    const candidates = [cell('1'), cell('2')]; view.candidates = candidates; view.hits = candidates; view.candidateTick = 0; view.candidateRevision = 10;
    context.drawChamber = () => { assert.equal(view.candidates, candidates); };
    context.commit({ ...after }); assert.equal(view.geometryRevision, 10); assert.equal(view.candidates, candidates, 'unchanged paused snapshot retains current candidates');
  }
});

test('reset intent cancels current tap and candidates before a delayed control response, including same-seed reset', async () => {
  const f = activationHarness(), tap = tapGesture(), event = { button: 0, isPrimary: true, pointerId: 1, clientX: 100, clientY: 100 };
  Object.assign(f.view, { tap, candidates: [cell('1'), cell('2')], candidateTick: 0, candidateRevision: 0, hits: [cell('1'), cell('2')], selected: '1', history: [state([])] });
  tap.down(event, 0, 0);
  let finish, polls = 0;
  f.context.control = (action, seed) => { assert.equal(action, 'reset'); assert.equal(seed, '1'); assert.equal(f.view.candidates.length, 0); return new Promise(resolve => { finish = resolve; }); };
  f.context.poll = async () => { polls++; };
  vm.runInContext(`${source('resetSimulation')};globalThis.reset=resetSimulation`, f.context);
  const pending = f.context.reset('1');
  assert.equal(f.view.candidateTick, null); assert.equal(f.view.candidateRevision, null); assert.equal(f.view.hits.length, 0); assert.equal(polls, 0); assert.equal(tap.up(event, 100, f.view.geometryRevision), false);
  finish(); await pending; assert.equal(polls, 1); assert.equal(f.view.selected, null);
  assert.ok(script.includes('void resetSimulation(seed)'));
});

test('synthetic persisted-page wiring reloads instead of reusing a disposed renderer', () => {
  let disposed = 0, reloads = 0;
  const view = { activationSerial: 2, three: { dispose() { disposed++; } } }, context = vm.createContext({ view, location: { reload() { reloads++; } } });
  vm.runInContext(`${source('hidePage')}\n${source('restorePage')};globalThis.hide=hidePage;globalThis.restore=restorePage`, context);
  context.hide(); assert.equal(disposed, 1); assert.equal(view.activationSerial, 3);
  context.restore({ persisted: false }); assert.equal(reloads, 0);
  context.restore({ persisted: true }); assert.equal(reloads, 1);
  assert.ok(script.includes('addEventListener("pagehide",hidePage)')); assert.ok(script.includes('addEventListener("pageshow",restorePage)'));
});

test('vendored bytes pin the complete executable import closure and MIT license', () => {
  const root = new URL('../crates/liminis/src/vendor/three-r180/', import.meta.url), manifest = JSON.parse(readFileSync(new URL('provenance.json', root)));
  assert.equal(THREE.REVISION, '180'); assert.equal(manifest.commit, '0af9729d0c143a86a1d725d6e2c3ad83301f3f34'); assert.equal(manifest.tag_object, '9e8635e2031c25859dc47ba07e72230dccb2682a'); assert.equal(manifest.unmodified, true);
  for (const file of manifest.files) {
    const bytes = readFileSync(new URL(file.path, root));
    assert.equal(bytes.length, file.bytes); assert.equal(createHash('sha256').update(bytes).digest('hex'), file.sha256);
    assert.equal(createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex'), file.git_blob);
    const executable = bytes.toString().replace(/\/\*[\s\S]*?\*\//g, '');
    const imports = [...new Set(Array.from(executable.matchAll(/(?:from\s*|import\s*)['"]([^'"]+)['"]/g), match => match[1]))];
    assert.deepEqual(imports, file.imports);
    for (const path of imports) assert.ok(path === 'three' ? manifest.import_map[path] === '/assets/vendor/three-r180/three.module.js' : manifest.files.some(file => `./${file.path}` === path));
  }
  assert.match(readFileSync(new URL('LICENSE', root), 'utf8'), /Permission is hereby granted, free of charge/);
  assert.ok(html.includes('"three":"/assets/vendor/three-r180/three.module.js"'));
});
