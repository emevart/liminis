import { CONDITIONS, SEEDS, LANDMARKS, SOURCE_COMMIT, DESCRIPTOR_SHA256, RAW_FILES, PLOT_METRICS, loadLab, metricValue, selectLab, selectionURL } from './lab-data.mjs';

const $ = id => document.getElementById(id);
const names = { baseline: 'Baseline', mutation_off: 'Mutation off', starvation: 'Starvation', oxygen_low: 'Low oxygen', exchange_half: 'Half exchange', founder_k2: 'Founder kinetics +2' };
const labels = { living_cells: 'Living cells', structural_mass_mol: 'Structural biomass · mol', food_free_mol: 'Free FOOD · mol', oxygen_free_mol: 'Free O2 · mol' };
const colors = ['#347a58', '#6a67a2', '#b15e38', '#267f98'];
let data = null, selected = null, ready = false, plotWidths = [];
function node(tag, text, attributes = {}) { const value = document.createElement(tag); if (text !== null) value.textContent = text; for (const [key, item] of Object.entries(attributes)) value.setAttribute(key, item); return value; }
function svgNode(tag, attributes, text = null) { const value = document.createElementNS('http://www.w3.org/2000/svg', tag); for (const [key, item] of Object.entries(attributes)) value.setAttribute(key, item); if (text !== null) value.textContent = text; return value; }
function display(value) { return value === null || value === undefined ? 'unavailable' : typeof value === 'number' ? Number.isInteger(value) ? value.toLocaleString('en-US') : value.toExponential(6) : String(value); }
function pair(host, field, label, value) { const dd = node('dd', display(value), { 'data-field': field, 'data-value': value === null ? 'null' : String(value) }); host.append(node('dt', label), dd); }
function choices(host, values, value) { host.replaceChildren(...values.map(([id, text]) => node('option', text, { value: id }))); host.value = String(value); }
function fatal(error) { ready = false; $('lab-error').textContent = error instanceof Error ? error.message : String(error); $('lab-error').hidden = false; $('lab-status').textContent = 'LAB evidence unavailable'; $('lab-workspace').hidden = true; $('lab-workspace').setAttribute('aria-busy', 'false'); for (const id of ['condition-select', 'seed-select', 'sample-select', 'comparison-select', 'plot-window']) $(id).disabled = true; }

// Только подписи осей сокращаются; points, domains и detail используют original.
function axisNumber(value) { const rounded = Number(value.toPrecision(3)); return rounded !== 0 && (Math.abs(rounded) < .01 || Math.abs(rounded) >= 10000) ? rounded.toExponential() : String(rounded); }
function plotFrame(width) { if (!Number.isFinite(width) || width <= 96) throw new Error('The chart is too narrow to display observation axes.'); return { width, height: 210, x0: 72, x1: width - 12, y0: 12, y1: 178 }; }

function renderPlots() {
  const limit = Number($('plot-window').value), arms = selected.condition === 'baseline' ? ['baseline'] : ['baseline', selected.condition], host = $('plots'); host.replaceChildren();
  const widths = [];
  for (const metric of PLOT_METRICS) {
    const curves = arms.flatMap(condition => SEEDS.map(seed => { const run = data.runs.get(`seed-${seed}-${condition}`); return { run, values: run.samples.filter(sample => sample.tick <= limit) }; }));
    const ymax = Math.max(...curves.flatMap(curve => curve.values.map(sample => metricValue(sample, metric)))) || 1, xmax = limit * 30;
    const svg = svgNode('svg', { height: 210, role: 'img', 'aria-label': `${labels[metric]}, four paired seeds; dots are observations`, class: 'metric-plot', 'data-metric': metric, 'data-x-max': xmax, 'data-y-max': ymax });
    const section = node('div', null, { class: 'plot' }); section.append(node('h3', labels[metric]), svg); host.append(section);
    const frame = plotFrame(svg.getBoundingClientRect().width); widths.push(frame.width);
    svg.setAttribute('viewBox', `0 0 ${frame.width} ${frame.height}`); for (const key of ['x0', 'x1', 'y0', 'y1']) svg.setAttribute(`data-${key}`, frame[key]);
    const x = time => frame.x0 + time / xmax * (frame.x1 - frame.x0), y = value => frame.y0 + (1 - value / ymax) * (frame.y1 - frame.y0);
    svg.append(svgNode('path', { d: `M${frame.x0} ${frame.y0} V${frame.y1} H${frame.x1}`, fill: 'none', stroke: '#b8c5bc' }));
    for (const fraction of [0, .5, 1]) { const yy = y(ymax * fraction); svg.append(svgNode('line', { x1: frame.x0, y1: yy, x2: frame.x1, y2: yy, stroke: '#e1e6df' }), svgNode('text', { class: 'axis-label', 'data-axis': 'y', 'data-value': ymax * fraction, x: frame.x0 - 6, y: yy + 4, 'text-anchor': 'end' }, axisNumber(ymax * fraction)), svgNode('text', { class: 'axis-label', 'data-axis': 'x', 'data-value': xmax * fraction / 3600, x: x(xmax * fraction), y: 201, 'text-anchor': fraction === 0 ? 'start' : fraction === 1 ? 'end' : 'middle' }, `${axisNumber(xmax * fraction / 3600)} h`)); }
    for (const { run, values } of curves) {
      const color = colors[SEEDS.indexOf(run.seed)], baseline = selected.condition !== 'baseline' && run.condition === 'baseline', active = run.seed === selected.seed;
      const attrs = { 'data-condition': run.condition, 'data-seed': run.seed, 'data-metric': metric, 'data-last-tick': values.at(-1).tick, 'data-sample-count': values.length, 'data-run-last-tick': run.summary.tick, 'data-run-sample-count': run.samples.length };
      svg.append(svgNode('path', { ...attrs, class: 'observed-curve', d: values.map((sample, index) => `${index ? 'L' : 'M'}${x(sample.time_seconds).toFixed(6)} ${y(metricValue(sample, metric)).toFixed(6)}`).join(' '), fill: 'none', stroke: color, 'stroke-width': active ? 2 : 1, 'stroke-dasharray': baseline ? '5 4' : 'none', opacity: active ? 1 : .6 }));
      for (const sample of values) svg.append(svgNode('circle', { 'data-condition': run.condition, 'data-seed': run.seed, 'data-tick': sample.tick, class: 'observed-dot', cx: x(sample.time_seconds), cy: y(metricValue(sample, metric)), r: active ? 1.8 : 1.1, fill: color, opacity: active ? 1 : .5 }));
    }
    if (selected.sample.tick <= limit) svg.append(svgNode('line', { class: 'sample-guide', x1: x(selected.sample.time_seconds), x2: x(selected.sample.time_seconds), y1: frame.y0, y2: frame.y1, stroke: '#84978a', 'stroke-dasharray': '2 4' }));
  }
  plotWidths = widths;
  const legend = $('seed-key'); legend.replaceChildren(...SEEDS.map((seed, index) => { const button = node('button', `seed ${seed}`, { type: 'button', 'aria-pressed': seed === selected.seed, 'data-seed': seed }); button.style.setProperty('--seed-color', colors[index]); button.addEventListener('click', () => changeSelection(selected.condition, seed, selected.sample.tick)); return button; }));
}
function resizePlots() {
  if (!ready || $('lab-workspace').hidden) return;
  const charts = Array.from($('plots').querySelectorAll('.metric-plot'));
  if (charts.length === 4 && charts.some((chart, index) => chart.getBoundingClientRect().width !== plotWidths[index])) renderPlots();
}
const plotObserver = new ResizeObserver(resizePlots); plotObserver.observe($('plots'));
addEventListener('pagehide', () => plotObserver.disconnect());
addEventListener('pageshow', event => { if (event.persisted) { plotObserver.observe($('plots')); resizePlots(); } });
function renderSample() {
  const { run, sample } = selected, host = $('run-detail'); host.replaceChildren();
  $('sample-stamp').textContent = `${run.run_id} · tick ${sample.tick} · ${display(sample.time_seconds)} model s`;
  $('sample-stamp').dataset.runId = run.run_id; $('sample-stamp').dataset.tick = sample.tick;
  $('run-status').textContent = `${run.status} · ${run.stop_reason}`;
  for (const [field, label, value] of [['run_id', 'Run', run.run_id], ['seed', 'Paired seed', run.seed], ['tick', 'Shown sample tick', sample.tick], ['time_seconds', 'Model seconds', sample.time_seconds], ['living_cells', 'Living cells', sample.living_cells], ['structural_mass_mol', 'Structural biomass · mol', sample.structural_mass_mol], ['food_free_mol', 'Free FOOD · mol', metricValue(sample, 'food_free_mol')], ['oxygen_free_mol', 'Free O2 · mol', metricValue(sample, 'oxygen_free_mol')], ['status', 'Run status', run.status], ['stop_reason', 'Stop reason', run.stop_reason], ['terminal_tick', 'Actual terminal tick', run.summary.tick], ['requested_steps', 'Requested horizon', run.requested_steps], ['sample_count', 'Retained observations', run.samples.length], ['peak_cells', 'Historical peak cells', sample.peak_cells], ['max_generation_observed', 'Historical max generation', sample.max_generation_observed]]) pair(host, field, label, value);
  const alleles = Object.entries(sample.allele_histogram).sort((a, b) => Number(a[0]) - Number(b[0])), histogram = $('allele-histogram'); histogram.replaceChildren();
  $('living-alleles').textContent = `${alleles.length} current living alleles`; $('living-alleles').dataset.count = alleles.length;
  for (const [allele, count] of alleles) { const row = node('div', null, { class: 'hist-row', 'data-allele': allele, 'data-count': count }), track = node('div', null, { class: 'hist-track' }), fill = node('div', null, { class: 'hist-fill' }); fill.style.width = `${count / sample.living_cells * 100}%`; track.append(fill); row.append(node('span', `K${Number(allele) >= 0 ? '+' : ''}${allele}`), track, node('span', String(count), { class: 'mono' })); histogram.append(row); }
  if (!alleles.length) histogram.append(node('p', 'No living cells; living allele histogram is empty.', { class: 'note' }));
  $('allele-history').textContent = `Historical observed kinetics-allele richness: ${sample.observed_allele_richness}. It includes previously seen alleles; it is not current living diversity or a species count.`; $('allele-history').dataset.richness = sample.observed_allele_richness;
  const exact = $('exact-counters'); exact.replaceChildren();
  for (const [field, value] of Object.entries({ ...sample.lifecycle, ...sample.accounting, structural_mass_units: sample.structural_mass_units, internal_energy_units: sample.internal_energy_units, bath_heat_units: sample.bath_heat_units, energy_units_per_joule: run.identity.energy_units_per_joule })) if (typeof value === 'string') pair(exact, field, field.replaceAll('_', ' '), value);
  for (const [index, value] of sample.accounting.medium_matter_units.entries()) pair(exact, `medium_matter_${sample.matter[index].id}`, `medium matter ${sample.matter[index].id} units`, value);
  pair(exact, 'ledger_checked_ticks', 'Runner ledger checked ticks', sample.ledger.checked_ticks);
  pair(exact, 'energy_residual', 'Last checked energy residual', sample.ledger.energy_residual_last_checked);
  if (sample.ledger.energy_residual_last_checked === null) exact.lastElementChild.textContent = 'unknown · no tick checked at genesis';
  pair(exact, 'matter_residual', 'Last checked matter residuals', sample.ledger.matter_residual_last_checked === null ? null : sample.ledger.matter_residual_last_checked.join(', '));
  if (sample.ledger.matter_residual_last_checked === null) exact.lastElementChild.textContent = 'unknown · no tick checked at genesis';
}
function renderComparisons() {
  const condition = selected.condition, latest = data.comparisons.latest_common_samples.filter(row => row.condition === condition), landmarks = data.comparisons.landmark_comparisons.filter(row => row.condition === condition);
  const summary = $('landmark-summary').tBodies[0], common = $('latest-common-summary').tBodies[0]; summary.replaceChildren(); common.replaceChildren();
  for (const row of landmarks) { const tr = node('tr', null, { 'data-kind': 'landmark', 'data-tick': row.tick, 'data-available-pairs': row.available_pairs, 'data-expected-pairs': row.expected_pairs }); tr.append(node('td', String(row.tick)), node('td', `${row.available_pairs} of ${row.expected_pairs}`), node('td', row.available_pairs ? 'published paired observations' : 'unavailable · stopped before landmark')); summary.append(tr); }
  for (const row of latest) { const tr = node('tr', null, { 'data-kind': 'latest-common', 'data-seed': row.seed, 'data-tick': row.tick }); tr.append(node('td', row.seed), node('td', String(row.tick)), node('td', String(data.runs.get(row.condition_run_id).summary.tick))); common.append(tr); }
  const choice = $('comparison-select').value, landmark = landmarks.find(row => String(row.tick) === choice), row = choice === 'latest' ? latest.find(value => value.seed === selected.seed) : landmark?.pairs.find(value => value.seed === selected.seed), tbody = $('pair-comparison').tBodies[0]; tbody.replaceChildren();
  if (condition === 'baseline') { $('comparison-status').textContent = 'Baseline is the reference arm. Choose another condition to inspect published paired differences.'; $('comparison-select').disabled = true; return; }
  $('comparison-select').disabled = false;
  $('comparison-status').textContent = choice === 'latest' ? `Latest common sample: tick ${row.tick} · seed ${row.seed} · condition terminal tick ${selected.run.summary.tick}. No pooled range across different endpoints.` : `Landmark tick ${row.tick} · ${landmark.available_pairs} of 4 pairs available · selected seed ${row.seed}${row.available ? '' : ' · unavailable: stopped_before_landmark'}`;
  $('comparison-status').dataset.tick = row.tick; $('comparison-status').dataset.kind = choice === 'latest' ? 'latest-common' : 'landmark';
  for (const metric of [...PLOT_METRICS, 'daughters_born', 'fissions', 'deaths', 'growth_extent', 'observed_allele_richness']) {
    const range = landmark?.ranges.delta[metric], tr = node('tr', null, { 'data-metric': metric, 'data-available': row.available });
    tr.append(node('th', labels[metric] ?? metric.replaceAll('_', ' ')));
    for (const arm of ['baseline', 'condition_metrics', 'delta']) tr.append(node('td', display(row[arm]?.[metric] ?? null), { 'data-arm': arm, 'data-value': row[arm]?.[metric] ?? 'null' }));
    tr.append(node('td', !range ? 'not pooled' : range.count === 0 ? 'unavailable · 0 of 4' : `[${display(range.min)}, ${display(range.max)}] · ${range.count} pairs`, { 'data-arm': 'range', 'data-min': range?.min ?? 'null', 'data-max': range?.max ?? 'null', 'data-width': range?.width ?? 'null', 'data-count': range?.count ?? 'null' })); tbody.append(tr);
  }
}
function renderEnvironment() {
  const condition = data.descriptor.conditions.find(value => value.condition === selected.condition), params = condition.declared_parameters, host = $('environment'); host.replaceChildren();
  for (const [field, label, value] of [['volume_m3', 'Volume · m³', params.chamber.volume_m3], ['temperature_k', 'Temperature · K', params.chamber.temperature_k], ['medium_exchange_per_s', 'Exchange · s⁻¹', params.chamber.medium_exchange_per_s], ['mutation_probability', 'Mutation probability', params.genome.mutation_probability], ['founder_kinetics', 'Founder kinetics', params.founder.genome.kinetics]]) pair(host, field, label, value);
  for (const arm of ['initial', 'medium']) for (const substance of ['FOOD', 'O2']) pair(host, `${arm}_${substance}`, `${arm === 'initial' ? 'Initial' : 'Reservoir target'} ${substance} · mol/m³`, params[arm].concentration[substance]);
  $('declared-parameters').textContent = JSON.stringify(params, null, 2);
}
function renderMatrix() {
  const host = $('run-matrix').tBodies[0]; host.replaceChildren();
  for (const condition of CONDITIONS) for (const seed of SEEDS) { const run = data.runs.get(`seed-${seed}-${condition}`), tr = node('tr', null, { 'data-condition': condition, 'data-seed': seed, 'data-run-id': run.run_id, 'data-selected': selected.condition === condition && selected.seed === seed, 'data-last-tick': run.summary.tick, 'data-living': run.summary.living_cells, 'data-status': run.status, 'data-stop-reason': run.stop_reason }); const link = node('a', names[condition], { href: selectionURL({ condition, seed, sample: run.summary }, location.href) }); link.addEventListener('click', event => { if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return; event.preventDefault(); changeSelection(condition, seed, run.summary.tick); }); const td = node('td', null); td.append(link); tr.append(td, ...[seed, run.summary.tick, run.summary.living_cells, run.status, run.stop_reason].map(value => node('td', String(value)))); host.append(tr); }
}
function renderProvenance() {
  const host = $('provenance'), source = data.descriptor.source; host.replaceChildren();
  for (const [field, label, value] of [['source_commit', 'Historical engine source', source.source_commit], ['world_format_version', 'World format', source.world_format_version], ['chamber_format', 'Chamber format', source.chamber_format], ['dt_seconds', 'Model dt · s', source.dt_seconds], ['numerical_mode', 'Numerical mode', source.provenance.numerical_mode], ['profile', 'Reported profile', source.provenance.profile], ['runtime_rustc', 'Reported runtime rustc', source.provenance.runtime_rustc], ['build_attestation', 'Build attestation', String(source.provenance.build_attestation)], ['source_verification', 'Runner source check', source.provenance.source_verification], ['descriptor_sha256', 'Delivery descriptor SHA-256', DESCRIPTOR_SHA256]]) pair(host, field, label, value);
  const links = $('source-links'); links.replaceChildren(node('a', 'Historical source tree', { href: `https://github.com/emevart/liminis/tree/${SOURCE_COMMIT}` }), node('a', 'Delivery descriptor', { href: './data/lab-2/descriptor.json' }));
  for (const [name, pin] of Object.entries(RAW_FILES)) links.append(node('a', `Original ${name} JSON · SHA-256 ${pin.sha256}`, { href: pin.url, download: '', 'data-source': name, 'data-sha256': pin.sha256 }));
  pair(host, 'run_input_toml_digest', 'Runner input TOML digest', selected.run.input_toml_digest); pair(host, 'run_final_digest', 'Runner final state digest', selected.run.final_state_digest); pair(host, 'tick_reports_digest', 'Runner tick reports digest', selected.run.tick_reports_digest);
}
function commit(selection) {
  selected = selection; choices($('condition-select'), CONDITIONS.map(value => [value, names[value]]), selected.condition); choices($('seed-select'), SEEDS.map(value => [value, value]), selected.seed); choices($('sample-select'), selected.run.samples.map(sample => [sample.tick, `tick ${sample.tick} · ${display(sample.time_seconds)} s`]), selected.sample.tick);
  renderSample(); $('lab-workspace').hidden = false; renderPlots(); renderComparisons(); renderEnvironment(); renderMatrix(); renderProvenance();
  $('lab-error').hidden = true; $('lab-workspace').hidden = false; $('lab-workspace').setAttribute('aria-busy', 'false'); $('lab-status').textContent = 'Verified published bytes · 24 runs · four paired seeds · aggregate observations'; ready = true;
  for (const id of ['condition-select', 'seed-select', 'sample-select', 'plot-window']) $(id).disabled = false;
}
function changeSelection(condition, seed, tick) {
  if (!ready) return;
  const run = data.runs.get(`seed-${seed}-${condition}`), sample = run.samples.find(value => value.tick === tick) ?? run.samples.at(-1);
  const next = selectLab(data, `?condition=${condition}&seed=${seed}&sample=${sample.tick}`); commit(next); history.pushState(null, '', selectionURL(next, location.href));
}
for (const id of ['condition-select', 'seed-select', 'sample-select']) $(id).addEventListener('change', () => changeSelection($('condition-select').value, $('seed-select').value, Number($('sample-select').value)));
$('comparison-select').addEventListener('change', () => { if (ready) renderComparisons(); }); $('plot-window').addEventListener('change', () => { if (ready) renderPlots(); });
addEventListener('popstate', () => { if (!data) return; try { commit(selectLab(data, location.search)); } catch (error) { fatal(error); } });
choices($('comparison-select'), [['latest', 'Latest common sample per seed'], ...LANDMARKS.map(tick => [tick, `Landmark tick ${tick}`])], 'latest');
try { const validated = await loadLab({ baseURL: document.baseURI }); const selection = selectLab(validated, location.search); data = validated; commit(selection); history.replaceState(null, '', selectionURL(selection, location.href)); }
catch (error) { fatal(error); }
