// Исторический source и byte pins не зависят от текущего HEAD сайта.
export const SOURCE_COMMIT = '21cbe90732128edee61963544e414d29a7420577';
export const DESCRIPTOR_SHA256 = '457d0d92616472f4007539bb9b92d755057f7b7eb536493ddda90c77daef26bb';
export const CONDITIONS = ['baseline', 'mutation_off', 'starvation', 'oxygen_low', 'exchange_half', 'founder_k2'];
export const SEEDS = ['1', '7', '42', '2026'];
export const LANDMARKS = [100, 500, 1000, 2000, 5000, 10000, 20000];
export const RAW_FILES = {
  results: { url: './data/lab-2/results.json', bytes: 5294742, sha256: '6fd944f6fe2e25ca5418ac357dcb97ce68620afee569b2ecd6dc684c22f7d37a' },
  comparisons: { url: './data/lab-2/comparisons.json', bytes: 1116928, sha256: 'e2faeda80c9f47ff23e5aff14879f3780627d867fae51aa655b77d4ad52a8ae3' },
  manifest: { url: './data/lab-2/manifest.json', bytes: 82880, sha256: 'f7a669548650754864a410a46e86593ab722116e0a01057e17f6efc84efcb2f3' },
};
const fail = label => { throw new Error(`Invalid published LAB evidence: ${label}.`); };
const check = (value, label) => { if (!value) fail(label); };
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const uint = value => Number.isSafeInteger(value) && value >= 0;
const finite = value => typeof value === 'number' && Number.isFinite(value);
const integerString = value => typeof value === 'string' && /^(0|-?[1-9]\d*)$/.test(value);
const sha = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
function canonical(value) { return Array.isArray(value) ? value.map(canonical) : object(value) ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value; }
const same = (a, b) => JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
function keys(value, expected, label) { check(object(value) && same(Object.keys(value).sort(), [...expected].sort()), label); }
export async function digest(bytes, cryptoImpl = globalThis.crypto) {
  return Array.from(new Uint8Array(await cryptoImpl.subtle.digest('SHA-256', bytes)), value => value.toString(16).padStart(2, '0')).join('');
}
export function validateDescriptor(d) {
  keys(d, ['schema_version', 'kind', 'batch_id', 'files', 'source', 'delivery', 'matrix', 'capabilities', 'conditions', 'runs'], 'descriptor fields');
  check(d.schema_version === 1 && d.kind === 'aggregate_cell_lab_descriptor' && d.batch_id === 'lab-2-chamber1-paired-v1', 'descriptor schema');
  keys(d.files, Object.keys(RAW_FILES), 'file set');
  for (const [name, pin] of Object.entries(RAW_FILES)) { keys(d.files[name], ['url', 'bytes', 'sha256'], 'file fields'); check(same(d.files[name], pin), `${name} byte pin`); }
  keys(d.source, ['source_commit', 'world_format_version', 'chamber_format', 'dt_seconds', 'provenance', 'limitations'], 'source fields');
  keys(d.delivery, ['builder', 'builder_schema_version', 'numerical_recalculation', 'total_raw_bytes'], 'delivery fields');
  check(d.source?.source_commit === SOURCE_COMMIT && d.source.world_format_version === 30 && d.source.chamber_format === 1 && d.source.dt_seconds === 30 && d.source.provenance?.build_attestation === false && Array.isArray(d.source.limitations), 'historical source');
  check(same(d.matrix, { seeds: SEEDS, conditions: CONDITIONS, expected_runs: 24, requested_steps: 20000, sample_every: 100, dt_seconds: 30 }), 'fixed paired matrix');
  check(same(d.capabilities, { aggregate_samples: true, individual_cells: false, spatial_positions: false, full_genomes: false, resumable_states: false, event_reconstruction: false }), 'aggregate capabilities');
  check(d.delivery?.builder === 'scripts/build_lab_catalog.py' && d.delivery.builder_schema_version === 1 && d.delivery.numerical_recalculation === false && d.delivery.total_raw_bytes === 6494550, 'delivery metadata');
  check(Array.isArray(d.conditions) && d.conditions.length === 6 && d.conditions.every((condition, index) => condition.condition === CONDITIONS[index] && uint(condition.toml_bytes) && sha(condition.toml_sha256) && sha(condition.canonical_config_sha256) && object(condition.declared_parameters) && object(condition.identity) && typeof condition.input_toml_digest === 'string'), 'declared conditions');
  for (const condition of d.conditions) {
    keys(condition, ['condition', 'toml_bytes', 'toml_sha256', 'canonical_config_sha256', 'input_toml_digest', 'identity', 'declared_parameters'], 'condition fields');
    const params = condition.declared_parameters, identity = condition.identity;
    check(identity.dt_seconds === 30 && identity.world_format_version === 30 && identity.chamber_format === 1 && identity.spatial_positions === false && integerString(identity.energy_units_per_joule) && Array.isArray(identity.matter_scales) && identity.matter_scales.length === 6 && identity.matter_scales.every((scale, index) => scale.id === ['WATER', 'FOOD', 'O2', 'CO2', 'DET', 'BIO'][index] && integerString(scale.units_per_mol)), 'condition identity/scales');
    check(params.dt === 30 && params.chamber_format === 1 && finite(params.chamber?.volume_m3) && params.chamber.volume_m3 === identity.volume_m3 && finite(params.chamber.temperature_k) && params.chamber.temperature_k === identity.temperature_k && finite(params.chamber.medium_exchange_per_s) && finite(params.genome?.mutation_probability) && Number.isInteger(params.founder?.genome?.kinetics), 'declared environment');
    for (const arm of ['initial', 'medium']) for (const substance of ['FOOD', 'O2']) check(finite(params[arm]?.concentration?.[substance]), 'declared concentration');
  }
  check(Array.isArray(d.runs) && d.runs.length === 24, 'run descriptor count');
  const ids = new Set(), indices = new Set();
  for (const run of d.runs) {
    keys(run, ['run_id', 'condition', 'seed', 'results_index', 'requested_steps', 'checked_ticks', 'attempted_tick', 'sample_every', 'sample_count', 'status', 'stop_reason', 'samples_truncated'], 'run descriptor fields');
    check(CONDITIONS.includes(run.condition) && SEEDS.includes(run.seed) && run.run_id === `seed-${run.seed}-${run.condition}` && !ids.has(run.run_id) && uint(run.results_index) && run.results_index < 24 && !indices.has(run.results_index), 'unique run joins');
    check(run.requested_steps === 20000 && run.sample_every === 100 && run.samples_truncated === false && run.attempted_tick === run.checked_ticks && run.checked_ticks === (run.condition === 'starvation' ? 454 : 20000) && run.sample_count === (run.condition === 'starvation' ? 6 : 201) && run.status === (run.condition === 'starvation' ? 'extinct' : 'censored') && run.stop_reason === (run.condition === 'starvation' ? 'extinction' : 'requested_horizon_reached'), 'run stopping boundary');
    ids.add(run.run_id); indices.add(run.results_index);
  }
  return d;
}
function histogram(value, living) {
  check(object(value), 'living allele histogram');
  check(Object.entries(value).every(([key, count]) => /^(-[1-4]|[0-4])$/.test(key) && uint(count) && count > 0) && Object.values(value).reduce((sum, count) => sum + count, 0) === living, 'living histogram counts');
}
function validateSample(sample, tick, identity) {
  check(sample.tick === tick && sample.time_seconds === tick * 30 && uint(sample.living_cells) && sample.living_cells <= identity.declared_max_cells, 'retained sample identity');
  for (const key of ['structural_mass_mol', 'internal_energy_j', 'bath_heat_j']) check(finite(sample[key]), `sample ${key}`);
  for (const key of ['structural_mass_units', 'internal_energy_units', 'bath_heat_units']) check(integerString(sample[key]), `exact ${key}`);
  histogram(sample.allele_histogram, sample.living_cells);
  check(object(sample.allele_first_seen_tick) && uint(sample.observed_allele_richness) && Object.keys(sample.allele_first_seen_tick).length === sample.observed_allele_richness && Object.entries(sample.allele_first_seen_tick).every(([key, first]) => /^(-[1-4]|[0-4])$/.test(key) && uint(first) && first <= tick) && Object.keys(sample.allele_histogram).every(key => Object.hasOwn(sample.allele_first_seen_tick, key)), 'historical allele observations');
  check(uint(sample.max_generation_observed) && uint(sample.peak_cells), 'historical counts');
  check(Array.isArray(sample.matter) && sample.matter.length === 6 && sample.matter.every((pool, index) => pool.id === identity.matter_scales[index].id && finite(pool.free_mol) && pool.free_mol >= 0 && integerString(pool.free_units)), 'observed free pools');
  check(object(sample.lifecycle) && ['daughters_born', 'deaths', 'fissions', 'founders', 'living_identity_residual'].every(key => integerString(sample.lifecycle[key])), 'exact lifecycle');
  check(object(sample.accounting) && ['death_mass_units', 'growth_extent', 'medium_energy_units'].every(key => integerString(sample.accounting[key])) && Array.isArray(sample.accounting.medium_matter_units) && sample.accounting.medium_matter_units.length === 6 && sample.accounting.medium_matter_units.every(integerString), 'exact accounting');
  const ledger = sample.ledger;
  check(ledger?.checked_ticks === tick && (tick === 0 ? ledger.energy_residual_last_checked === null && ledger.matter_residual_last_checked === null : ledger.energy_residual_last_checked === '0' && Array.isArray(ledger.matter_residual_last_checked) && ledger.matter_residual_last_checked.length === 6 && ledger.matter_residual_last_checked.every(value => value === '0')), 'exported ledger boundary');
}
export const metricValue = (sample, metric) => metric === 'food_free_mol' || metric === 'oxygen_free_mol' ? sample.matter.find(pool => pool.id === (metric === 'food_free_mol' ? 'FOOD' : 'O2')).free_mol : sample[metric];
export const PLOT_METRICS = ['living_cells', 'structural_mass_mol', 'food_free_mol', 'oxygen_free_mol'];

export async function validateLab(d, results, comparisons, manifest, cryptoImpl = globalThis.crypto) {
  validateDescriptor(d);
  check(results.schema_version === 1 && results.kind === 'comparative_cell_lab' && results.source_commit === SOURCE_COMMIT && results.batch_id === d.batch_id && same(results.provenance, d.source.provenance) && same(results.limitations, d.source.limitations), 'results source');
  check(manifest.schema_version === 1 && manifest.batch_id === d.batch_id && Array.isArray(manifest.runs) && manifest.runs.length === 24 && Array.isArray(results.runs) && results.runs.length === 24, 'raw run counts');
  const runs = new Map(), samples = new Map();
  for (const metadata of d.runs) {
    const run = results.runs[metadata.results_index], input = manifest.runs[metadata.results_index], condition = d.conditions.find(value => value.condition === metadata.condition);
    check(object(run) && object(input) && run.run_id === metadata.run_id && input.run_id === run.run_id && run.condition === metadata.condition && run.seed === metadata.seed && input.condition === run.condition && input.seed === run.seed, 'run join');
    for (const key of ['requested_steps', 'checked_ticks', 'attempted_tick', 'sample_every', 'status', 'stop_reason', 'samples_truncated']) check(run[key] === metadata[key], `run ${key}`);
    check(input.steps === run.requested_steps && input.sample_every === run.sample_every && same(run.identity, condition.identity) && run.identity.world_format_version === 30 && run.identity.chamber_format === 1 && run.identity.dt_seconds === 30 && run.identity.spatial_positions === false && run.input_toml_digest === condition.input_toml_digest, 'historical run identity');
    check(typeof input.scenario_toml === 'string' && typeof run.canonical_config === 'string' && new TextEncoder().encode(input.scenario_toml).length === condition.toml_bytes && await digest(new TextEncoder().encode(input.scenario_toml), cryptoImpl) === condition.toml_sha256 && await digest(new TextEncoder().encode(run.canonical_config), cryptoImpl) === condition.canonical_config_sha256, 'declared configuration bytes');
    check(Array.isArray(run.samples) && run.samples.length === metadata.sample_count, 'sample count');
    const expected = run.condition === 'starvation' ? [0, 100, 200, 300, 400, 454] : Array.from({ length: 201 }, (_, index) => index * 100);
    run.samples.forEach((sample, index) => validateSample(sample, expected[index], run.identity));
    check(same(run.summary, run.samples.at(-1)) && (run.status !== 'extinct' || run.summary.living_cells === 0), 'terminal summary');
    runs.set(run.run_id, run); samples.set(run.run_id, new Map(run.samples.map(sample => [sample.tick, sample])));
  }
  check(comparisons.schema_version === 1 && comparisons.kind === 'paired_cell_landmark_comparisons' && comparisons.source_commit === SOURCE_COMMIT && same(comparisons.matrix, d.matrix) && comparisons.validation?.status === 'PASS' && comparisons.validation.independent_integrity?.results?.digest === RAW_FILES.results.sha256 && comparisons.validation.independent_integrity?.manifest?.digest === RAW_FILES.manifest.sha256, 'comparison source');
  check(object(comparisons.metrics) && [...PLOT_METRICS, 'daughters_born', 'fissions', 'deaths', 'growth_extent', 'observed_allele_richness'].every(key => Object.hasOwn(comparisons.metrics, key)) && Object.values(comparisons.metrics).every(value => object(value) && typeof value.unit === 'string' && ['JSON number', 'decimal integer string'].includes(value.encoding)), 'comparison metric schema');
  check(Array.isArray(comparisons.landmark_comparisons) && comparisons.landmark_comparisons.length === 35 && Array.isArray(comparisons.latest_common_samples) && comparisons.latest_common_samples.length === 20, 'published comparison counts');
  const seen = new Set();
  function pair(value, condition, tick, seed) {
    check(value.condition === condition && value.seed === seed && value.tick === tick && value.baseline_run_id === `seed-${seed}-baseline` && value.condition_run_id === `seed-${seed}-${condition}`, 'paired run join');
    const baseline = samples.get(value.baseline_run_id).get(tick), selected = samples.get(value.condition_run_id).get(tick), available = !!baseline && !!selected;
    check(value.available === available && same(value.baseline_allele_histogram, baseline?.allele_histogram ?? null) && same(value.condition_allele_histogram, selected?.allele_histogram ?? null), 'paired sample availability');
    for (const [metrics, sample] of [[value.baseline, baseline], [value.condition_metrics, selected]]) {
      check(sample ? object(metrics) && PLOT_METRICS.every(key => metrics[key] === metricValue(sample, key)) : metrics === null, 'paired metric observation');
    }
    if (!available) check(value.delta === null && value.delta_allele_histogram === null && value.missing?.condition?.reason === 'stopped_before_landmark' && value.missing.condition.status === runs.get(value.condition_run_id).status && value.missing.condition.stop_reason === runs.get(value.condition_run_id).stop_reason && value.missing.condition.checked_ticks === runs.get(value.condition_run_id).checked_ticks, 'unavailable pair values/reason');
    else {
      check(object(value.delta) && object(value.delta_allele_histogram), 'published differences');
      for (const [key, definition] of Object.entries(comparisons.metrics)) for (const metrics of [value.baseline, value.condition_metrics, value.delta]) check(definition.encoding === 'decimal integer string' ? integerString(metrics[key]) : finite(metrics[key]), 'comparison metric type');
    }
  }
  for (const row of comparisons.landmark_comparisons) {
    check(CONDITIONS.slice(1).includes(row.condition) && LANDMARKS.includes(row.tick) && row.time_seconds === row.tick * 30 && !seen.has(`${row.condition}:${row.tick}`) && row.expected_pairs === 4 && Array.isArray(row.pairs) && row.pairs.length === 4, 'landmark identity'); seen.add(`${row.condition}:${row.tick}`);
    row.pairs.forEach((value, index) => pair(value, row.condition, row.tick, SEEDS[index]));
    check(row.available_pairs === row.pairs.filter(value => value.available).length, 'available pair denominator');
    keys(row.ranges, ['baseline', 'condition_metrics', 'delta'], 'published range arms');
    for (const arm of ['baseline', 'condition_metrics', 'delta']) {
      keys(row.ranges[arm], Object.keys(comparisons.metrics), 'published range metrics');
      for (const [metric, range] of Object.entries(row.ranges[arm])) {
        keys(range, ['count', 'min', 'max', 'width'], 'published range fields');
        const exact = comparisons.metrics[metric].encoding === 'decimal integer string';
        check(range.count === row.available_pairs && (range.count === 0 ? range.min === null && range.max === null && range.width === null : [range.min, range.max, range.width].every(exact ? integerString : finite) && (exact ? BigInt(range.min) <= BigInt(range.max) && BigInt(range.width) >= 0n : range.min <= range.max && range.width >= 0)), 'published range availability/type');
      }
    }
  }
  seen.clear();
  for (const value of comparisons.latest_common_samples) {
    check(CONDITIONS.slice(1).includes(value.condition) && SEEDS.includes(value.seed) && !seen.has(`${value.condition}:${value.seed}`), 'latest common identity'); seen.add(`${value.condition}:${value.seed}`);
    const ticks = runs.get(`seed-${value.seed}-${value.condition}`).samples.map(sample => sample.tick).filter(tick => samples.get(`seed-${value.seed}-baseline`).has(tick));
    check(value.tick === ticks.at(-1) && value.available === true, 'latest common sampled boundary'); pair(value, value.condition, value.tick, value.seed);
  }
  return { descriptor: d, results, comparisons, manifest, runs };
}
async function readBytes(url, limit, fetchImpl) {
  const response = await fetchImpl(url);
  if (!response.ok) throw new Error(`LAB data request failed (${response.status}).`);
  if (!response.body?.pipeTo) throw new Error('LAB streamed response is unavailable.');
  const chunks = []; let length = 0, writeFailed = false, writeError;
  const sink = new WritableStream({
    write(value) {
      try { length += value.length; check(length <= limit, 'response size'); chunks.push(value); }
      catch (error) { writeFailed = true; writeError = error; throw error; }
    }
  });
  try { await response.body.pipeTo(sink); }
  catch (error) { if (writeFailed) throw writeError; throw error; }
  const bytes = new Uint8Array(length); let offset = 0; for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; } return bytes;
}
export async function loadLab({ baseURL = import.meta.url, fetchImpl = globalThis.fetch, cryptoImpl = globalThis.crypto } = {}) {
  check(sha(DESCRIPTOR_SHA256), 'descriptor release pin');
  const decode = bytes => JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
  const bytes = await readBytes(new URL('./data/lab-2/descriptor.json', baseURL), 256 * 1024, fetchImpl);
  check(await digest(bytes, cryptoImpl) === DESCRIPTOR_SHA256, 'descriptor SHA-256'); const d = validateDescriptor(decode(bytes));
  const payloads = await Promise.all(Object.entries(RAW_FILES).map(async ([name, pin]) => { const raw = await readBytes(new URL(pin.url, baseURL), pin.bytes, fetchImpl); check(raw.length === pin.bytes && await digest(raw, cryptoImpl) === pin.sha256, `${name} SHA-256`); return [name, decode(raw)]; }));
  const raw = Object.fromEntries(payloads); return validateLab(d, raw.results, raw.comparisons, raw.manifest, cryptoImpl);
}
export function selectLab(data, search = '') {
  const query = new URLSearchParams(search), condition = query.get('condition') ?? 'baseline', seed = query.get('seed') ?? '1';
  check(CONDITIONS.includes(condition) && SEEDS.includes(seed), 'URL condition or seed');
  const run = data.runs.get(`seed-${seed}-${condition}`), requested = query.get('sample');
  check(requested === null || /^(0|[1-9]\d*)$/.test(requested), 'URL sample');
  const tick = requested === null ? run.samples.at(-1).tick : Number(requested), sample = run.samples.find(value => value.tick === tick);
  check(!!sample, 'URL sample must be a retained observation'); return { condition, seed, run, sample };
}
export function selectionURL(selection, currentURL) {
  const url = new URL(currentURL); for (const [key, value] of Object.entries({ condition: selection.condition, seed: selection.seed, sample: String(selection.sample.tick) })) url.searchParams.set(key, value); return url;
}
