import assert from 'node:assert/strict';
import { webcrypto } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { CONDITIONS, DESCRIPTOR_SHA256, RAW_FILES, digest, loadLab, selectLab, selectionURL, validateDescriptor, validateLab } from './lab-data.mjs';

const bytes = name => readFileSync(new URL(`./data/lab-2/${name}.json`, import.meta.url));
const original = { descriptor: JSON.parse(bytes('descriptor')), ...Object.fromEntries(Object.keys(RAW_FILES).map(name => [name, JSON.parse(bytes(name))])) };
const clone = value => structuredClone(value);
const validated = await validateLab(original.descriptor, original.results, original.comparisons, original.manifest, webcrypto);

test('exact descriptor and all three original JSON byte pins bind historical source and 24 joined runs', async () => {
  assert.equal(await digest(bytes('descriptor'), webcrypto), DESCRIPTOR_SHA256);
  for (const [name, pin] of Object.entries(RAW_FILES)) { assert.equal(bytes(name).length, pin.bytes); assert.equal(await digest(bytes(name), webcrypto), pin.sha256); }
  assert.equal(validated.runs.size, 24); assert.equal(validated.results.source_commit, '21cbe90732128edee61963544e414d29a7420577'); assert.equal(validated.descriptor.source.provenance.build_attestation, false);
  assert.equal(validated.runs.get('seed-1-baseline').identity.energy_units_per_joule, '147573952589676412928');
  assert.equal(typeof validated.runs.get('seed-1-baseline').identity.energy_units_per_joule, 'string');
});

test('descriptor rejects URL escape, digest coercion, duplicate IDs/indexes, false capabilities and source drift', () => {
  const edits = [d => { d.files.results.url = 'https://example.com/results.json'; }, d => { d.files.results.sha256 = [d.files.results.sha256]; }, d => { d.runs[1].run_id = d.runs[0].run_id; }, d => { d.runs[1].results_index = d.runs[0].results_index; }, d => { d.capabilities.full_genomes = true; }, d => { d.source.world_format_version = 31; }, d => { d.conditions[0].declared_parameters.initial.concentration.FOOD = '10'; }, d => { d.delivery.extra = true; }];
  for (const edit of edits) { const d = clone(original.descriptor); edit(d); assert.throws(() => validateDescriptor(d), /Invalid published LAB/); }
});

test('strict raw joins reject stale run IDs, numeric exact strings, missing samples and altered pair availability', async () => {
  const edits = [d => { d.results.runs[0].run_id = 'other'; }, d => { d.results.runs[0].samples[0].structural_mass_units = 2213608; }, d => { d.results.runs[0].samples[0].lifecycle.founders = ['8']; }, d => { d.results.runs[0].samples[1].ledger.energy_residual_last_checked = null; }, d => { d.results.runs[0].samples[1].allele_histogram = { 0: 1 }; }, d => { d.results.runs[0].samples[2].tick = 201; }, d => { d.comparisons.latest_common_samples.find(row => row.condition === 'starvation').tick = 454; }, d => { d.comparisons.landmark_comparisons.find(row => row.condition === 'starvation' && row.tick === 500).pairs[0].delta = { living_cells: 0 }; }, d => { d.comparisons.landmark_comparisons[0].ranges.delta.living_cells.min = [0]; }, d => { d.comparisons.landmark_comparisons[0].ranges.delta.growth_extent.width = 0; }];
  for (const edit of edits) { const d = clone(original); edit(d); await assert.rejects(validateLab(d.descriptor, d.results, d.comparisons, d.manifest, webcrypto), /Invalid published LAB/); }
});

test('20 censored horizons and four extinct terminals retain observed454, common400 and unavailable500 separately', () => {
  const rows = [...validated.runs.values()]; assert.equal(rows.filter(run => run.status === 'censored').length, 20); assert.equal(rows.filter(run => run.status === 'extinct').length, 4);
  for (const run of rows.filter(run => run.condition === 'starvation')) { assert.deepEqual(run.samples.map(sample => sample.tick), [0, 100, 200, 300, 400, 454]); assert.equal(run.summary.living_cells, 0); assert.equal(run.stop_reason, 'extinction'); assert.equal(run.requested_steps, 20000); }
  for (const row of validated.comparisons.latest_common_samples.filter(row => row.condition === 'starvation')) assert.equal(row.tick, 400);
  const missing = validated.comparisons.landmark_comparisons.find(row => row.condition === 'starvation' && row.tick === 500); assert.equal(missing.available_pairs, 0); assert.equal(missing.expected_pairs, 4);
  for (const row of missing.pairs) { assert.equal(row.condition_metrics, null); assert.equal(row.delta, null); assert.equal(row.missing.condition.reason, 'stopped_before_landmark'); }
  assert.deepEqual(missing.ranges.delta.living_cells, { count: 0, min: null, max: null, width: null });
});

test('deep URLs select exact retained ticks and reject unknown or unobserved samples without floor or carry', () => {
  const selected = selectLab(validated, '?condition=starvation&seed=42&sample=454'); assert.equal(selected.sample.tick, 454);
  assert.equal(selectLab(validated, '?condition=starvation&seed=42').sample.tick, 454);
  const url = selectionURL(selected, 'https://liminis.dev/lab.html?retained=yes'); assert.equal(url.searchParams.get('condition'), 'starvation'); assert.equal(url.searchParams.get('seed'), '42'); assert.equal(url.searchParams.get('sample'), '454'); assert.equal(url.searchParams.get('retained'), 'yes');
  for (const query of ['?condition=starvation&sample=500', '?condition=baseline&sample=454', '?seed=9007199254740993', '?sample=1.5', '?sample=00100', '?sample=1e2', '?condition=unknown']) assert.throws(() => selectLab(validated, query), /Invalid published LAB/);
  for (const condition of CONDITIONS) assert.equal(selectLab(validated, `?condition=${condition}&seed=2026&sample=0`).sample.ledger.energy_residual_last_checked, null);
});

function transport({ corrupt, delayed } = {}) {
  const requested = []; let finish;
  const pending = new Promise(resolve => { finish = resolve; });
  const fetchImpl = async url => {
    requested.push(url.pathname); const name = url.pathname.match(/\/(descriptor|results|comparisons|manifest)\.json$/)?.[1]; assert.ok(name);
    if (name === delayed) await pending;
    const payload = new Uint8Array(bytes(name)); if (name === corrupt) payload[payload.length - 5] ^= 1;
    return new Response(payload, { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  return { requested, finish, fetchImpl };
}
test('loader exposes only the completed byte-verified joined dataset after a delayed genuine raw response', async () => {
  const net = transport({ delayed: 'manifest' }); let completed = false;
  const pending = loadLab({ baseURL: 'https://liminis.dev/lab.html', fetchImpl: net.fetchImpl, cryptoImpl: webcrypto }).then(value => { completed = true; return value; });
  for (let index = 0; index < 20 && net.requested.length < 4; index++) await new Promise(resolve => setTimeout(resolve, 1));
  assert.equal(net.requested.length, 4); assert.equal(completed, false); net.finish(); assert.equal((await pending).runs.size, 24);
  assert.deepEqual(net.requested.sort(), ['comparisons', 'descriptor', 'manifest', 'results'].map(name => `/data/lab-2/${name}.json`).sort());
});
test('genuine modified descriptor/raw bytes fail integrity instead of exposing a partial dataset', async () => {
  for (const name of ['descriptor', 'results', 'comparisons', 'manifest']) { const net = transport({ corrupt: name }); await assert.rejects(loadLab({ baseURL: 'https://liminis.dev/lab.html', fetchImpl: net.fetchImpl, cryptoImpl: webcrypto }), /SHA-256/); if (name === 'descriptor') assert.equal(net.requested.length, 1); }
});
