import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';
import { webcrypto } from 'node:crypto';
import { loadLab, metricValue, selectLab, selectionURL, SEEDS } from './lab-data.mjs';

const script = readFileSync(new URL('./lab.js', import.meta.url), 'utf8');
const raw = JSON.parse(readFileSync(new URL('./data/lab-2/results.json', import.meta.url)));
const comparisons = JSON.parse(readFileSync(new URL('./data/lab-2/comparisons.json', import.meta.url)));
const source = name => (script.match(new RegExp(`function ${name}\\([^\\n]*\\) \\{[^\\n]*\\}$`, 'm')) ?? script.match(new RegExp(`function ${name}\\([^\\n]*\\) \\{[\\s\\S]*?\\n\\}`)))[0];
function harness(selection) {
  // Минимальный DOM для exact shipped render functions; не browser/layout PASS.
  let width = 300;
  const element = tag => ({ tag, children: [], attributes: {}, dataset: {}, style: { setProperty() {} }, textContent: '', value: 'latest',
    setAttribute(key, value) { this.attributes[key] = String(value); }, addEventListener() {}, append(...values) { this.children.push(...values); for (const value of values) value.parent = this; }, replaceChildren(...values) { this.children = [...values]; for (const value of values) value.parent = this; }, get lastElementChild() { return this.children.at(-1); },
    getBoundingClientRect() { assert.ok(this.parent?.parent, 'chart must be attached before measurement'); return { width }; }, querySelectorAll() { return this.children.flatMap(child => child.children.filter(value => value.tag === 'svg')); } });
  const elements = new Map(); const get = key => { if (!elements.has(key)) { const value = element(key); if (key.endsWith('summary') || key === 'pair-comparison') value.tBodies = [element('tbody')]; elements.set(key, value); } return elements.get(key); };
  const context = vm.createContext({ selected: selection, ready: true, plotWidths: [], data: { comparisons, runs: new Map(raw.runs.map(run => [run.run_id, run])) }, SEEDS, PLOT_METRICS: ['living_cells', 'structural_mass_mol', 'food_free_mol', 'oxygen_free_mol'], labels: {}, colors: ['#347a58', '#6a67a2', '#b15e38', '#267f98'], $: get, document: { createElement: element, createElementNS: (_, tag) => element(tag) }, metricValue });
  vm.runInContext(`${source('node')}\n${source('svgNode')}\n${source('display')}\n${source('pair')}\n${source('axisNumber')}\n${source('plotFrame')}\n${source('renderSample')}\n${source('renderComparisons')}\n${source('renderPlots')}\n${source('resizePlots')};globalThis.sample=renderSample;globalThis.compare=renderComparisons;globalThis.plots=renderPlots;globalThis.resize=resizePlots;globalThis.axis=axisNumber;globalThis.frame=plotFrame`, context);
  return { context, elements, get, setWidth(value) { width = value; } };
}
test('actual sample renderer keeps large exact strings and unknown genesis residuals distinct from zero', () => {
  const run = raw.runs.find(value => value.run_id === 'seed-1-baseline'), h = harness({ condition: run.condition, seed: run.seed, run, sample: run.samples[0] }); h.context.sample();
  const fields = new Map(h.get('exact-counters').children.filter(node => node.tag === 'dd').map(node => [node.attributes['data-field'], node]));
  assert.equal(fields.get('energy_units_per_joule').textContent, '147573952589676412928'); assert.match(fields.get('energy_residual').textContent, /unknown.*genesis/); assert.equal(fields.get('energy_residual').attributes['data-value'], 'null');
  assert.equal(h.get('living-alleles').dataset.count, 1); assert.equal(h.get('allele-history').dataset.richness, 1);
});
test('actual extinct renderer shows empty living histogram independently of historical richness', () => {
  const run = raw.runs.find(value => value.run_id === 'seed-42-starvation'), h = harness({ condition: run.condition, seed: run.seed, run, sample: run.samples.at(-1) }); h.context.sample();
  assert.equal(h.get('living-alleles').dataset.count, 0); assert.equal(h.get('allele-history').dataset.richness, run.summary.observed_allele_richness); assert.match(h.get('allele-histogram').children[0].textContent, /No living cells/);
  assert.equal(h.get('sample-stamp').dataset.tick, 454);
});
test('published comparison renderer preserves common400, terminal454 and landmark500 unavailable ranges', () => {
  const run = raw.runs.find(value => value.run_id === 'seed-42-starvation'), h = harness({ condition: run.condition, seed: run.seed, run, sample: run.samples.at(-1) });
  h.context.compare(); assert.match(h.get('comparison-status').textContent, /common sample: tick 400.*terminal tick 454/);
  assert.equal(h.get('landmark-summary').tBodies[0].children.length, 7); assert.equal(h.get('latest-common-summary').tBodies[0].children.length, 4);
  h.get('comparison-select').value = '500'; h.context.compare(); assert.match(h.get('comparison-status').textContent, /0 of 4.*unavailable/);
  for (const row of h.get('pair-comparison').tBodies[0].children) { assert.equal(row.children[2].textContent, 'unavailable'); assert.equal(row.children[3].textContent, 'unavailable'); assert.equal(row.children[4].attributes['data-min'], 'null'); assert.equal(row.children[4].attributes['data-count'], '0'); }
});

test('actual plot renderer draws only observed samples and ends starvation at454 for all four pairs', () => {
  const run = raw.runs.find(value => value.run_id === 'seed-42-starvation'), h = harness({ condition: run.condition, seed: run.seed, run, sample: run.samples.at(-1) });
  h.get('plot-window').value = '20000';
  h.context.plots();
  for (const plot of h.get('plots').children) {
    const svg = plot.children[1], curves = svg.children.filter(value => value.attributes.class === 'observed-curve'); assert.equal(curves.length, 8);
    for (const curve of curves) {
      const starvation = curve.attributes['data-condition'] === 'starvation'; assert.equal(curve.attributes['data-last-tick'], starvation ? '454' : '20000'); assert.equal(curve.attributes['data-sample-count'], starvation ? '6' : '201');
      assert.equal((curve.attributes.d.match(/[ML]/g) ?? []).length, starvation ? 6 : 201);
      const lastX = Number(curve.attributes.d.trim().split(/\s+/).at(-2).replace(/^[ML]/, '')); assert.ok(Math.abs(lastX - (72 + (starvation ? 454 : 20000) / 20000 * 216)) < 1e-6);
    }
  }
});

test('axis-only formatter gives short significant labels and pixel-sized chart frames at desktop/mobile widths', () => {
  const h = harness(null);
  for (const [value, expected] of [[0, '0'], [278, '278'], [166.6666667, '167'], [1.178612345e-11, '1.18e-11'], [5.853099e-12, '5.85e-12'], [100000, '1e+5']]) assert.equal(h.context.axis(value), expected);
  for (const width of [300, 370, 620.5]) { const frame = h.context.frame(width); assert.equal(frame.width, width); assert.equal(frame.height, 210); assert.equal(frame.x0, 72); assert.equal(frame.x1, width - 12); assert.equal(frame.y1, 178); }
  assert.throws(() => h.context.frame(0), /too narrow/);
  const css = readFileSync(new URL('./lab.css', import.meta.url), 'utf8'); assert.match(css, /\.plot svg\{[^}]*height:210px/); assert.match(css, /\.plot text\{[^}]*font:11px/);
});

test('measured chart widths change only projection geometry; observer ignores unchanged width, hidden state and drawing itself', () => {
  const run = raw.runs.find(value => value.run_id === 'seed-42-starvation'), h = harness({ condition: run.condition, seed: run.seed, run, sample: run.samples.at(-1) }); h.get('plot-window').value = '20000';
  h.context.plots(); let current = h.get('plots').children[0], oldSVG = current.children[1];
  h.context.resize(); assert.equal(h.get('plots').children[0], current, 'unchanged width must not redraw');
  h.setWidth(370); h.context.resize(); assert.notEqual(h.get('plots').children[0], current); current = h.get('plots').children[0];
  const svg = current.children[1]; assert.equal(svg.attributes.viewBox, '0 0 370 210'); assert.equal(svg.attributes['data-x0'], '72'); assert.equal(svg.attributes['data-x1'], '358'); assert.equal(svg.attributes['data-y0'], '12'); assert.equal(svg.attributes['data-y1'], '178'); assert.equal(svg.attributes['data-x-max'], oldSVG.attributes['data-x-max']); assert.equal(svg.attributes['data-y-max'], oldSVG.attributes['data-y-max']);
  const curves = chart => chart.children.filter(value => value.attributes.class === 'observed-curve'); assert.deepEqual(curves(svg).map(value => [value.attributes['data-last-tick'], value.attributes['data-sample-count']]), curves(oldSVG).map(value => [value.attributes['data-last-tick'], value.attributes['data-sample-count']]));
  h.context.resize(); assert.equal(h.get('plots').children[0], current, 'observer after own redraw has stable widths');
  h.get('lab-workspace').hidden = true; h.setWidth(300); h.context.resize(); assert.equal(h.get('plots').children[0], current, 'hidden charts are not measured/redrawn');
  assert.match(script, /new ResizeObserver\(resizePlots\)/); assert.match(script, /plotObserver\.disconnect\(\)/);
});

test('actual startup wiring waits for byte-validated data and never commits a corrupted or invalid-URL dataset', async () => {
  const init = script.slice(script.lastIndexOf('try { const validated = await loadLab'));
  for (const scenario of ['delayed', 'corrupt', 'invalid-url']) {
    let finish, commits = 0;
    const pending = new Promise(resolve => { finish = resolve; });
    const fetchImpl = async url => { const name = url.pathname.match(/\/(descriptor|results|comparisons|manifest)\.json$/)[1]; if (scenario === 'delayed' && name === 'results') await pending; const bytes = new Uint8Array(readFileSync(new URL(`./data/lab-2/${name}.json`, import.meta.url))); if (scenario === 'corrupt' && name === 'results') bytes[bytes.length - 2] ^= 1; return new Response(bytes); };
    const h = harness(null); Object.assign(h.context, { ready: false, data: null, document: { baseURI: 'https://liminis.dev/lab.html' }, location: { href: 'https://liminis.dev/lab.html', search: scenario === 'invalid-url' ? '?condition=starvation&sample=500' : '' }, history: { replaceState() {} }, loadLab: options => loadLab({ ...options, fetchImpl, cryptoImpl: webcrypto }), selectLab, selectionURL, commit() { commits++; } });
    vm.runInContext(`${source('fatal')};globalThis.initialize=(async()=>{${init}})()`, h.context);
    if (scenario === 'delayed') { await new Promise(resolve => setTimeout(resolve, 20)); assert.equal(commits, 0); assert.equal(h.context.data, null); finish(); }
    await h.context.initialize;
    assert.equal(commits, scenario === 'delayed' ? 1 : 0);
    if (scenario !== 'delayed') { assert.equal(h.get('lab-workspace').hidden, true); assert.equal(h.get('condition-select').disabled, true); assert.equal(h.get('lab-error').hidden, false); assert.equal(h.context.data, null); }
  }
});
