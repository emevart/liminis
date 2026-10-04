import { loadRecording } from "./recording-loader.mjs?v=dense-1";
import { PlaybackClock, DensePlaybackSession, defaultPlaybackRate, validPlaybackRate } from "./playback.mjs?v=dense-1";

const $ = (id) => document.getElementById(id);
const finite = (value, fallback = 0) => Number.isFinite(Number(value)) ? Number(value) : fallback;
const palette = ["#67c99c", "#65c7d1", "#ee8177", "#d98ec0", "#dfbd64", "#82a9e8", "#a8cd6c", "#ce8f72"];
const icons = {
  play: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/></svg>',
  pause: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="14" y="3" width="5" height="18" rx="1"/><rect x="5" y="3" width="5" height="18" rx="1"/></svg>',
  replay: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>',
};
const view = { data: null, metadata: null, dense: null, session: null, ready: false, index: 0, clock: null, drawFps: 30, lastDraw: 0, selected: null, hits: [], layout: new Map(), raf: 0 };

function hash(text) { let value = 2166136261; for (const char of String(text)) { value ^= char.charCodeAt(0); value = Math.imul(value, 16777619); } return value >>> 0; }
function color(key) { return palette[hash(key) % palette.length]; }
function numberText(value, digits = 4) {
  const number = finite(value); if (number === 0) return "0"; if (Number.isInteger(number)) return number.toLocaleString("en-US");
  const magnitude = Math.abs(number); return magnitude < 1e-4 || magnitude >= 1e6 ? number.toExponential(digits - 1) : number.toLocaleString("en-US", { maximumSignificantDigits: digits });
}
function inspectText(value) {
  if (typeof value === "number") return numberText(value);
  if (Array.isArray(value)) return `[${value.map(inspectText).join(", ")}]`;
  if (value && typeof value === "object") return `{ ${Object.entries(value).map(([key, item]) => `${key}: ${inspectText(item)}`).join(", ")} }`;
  return String(value);
}
function frame() { return view.dense ? view.session?.current ?? null : view.data?.frames?.[view.index] ?? null; }
function timeline() { return view.dense ? view.metadata.experiment : view.data.frames.map((sample) => sample.sim_time); }
function prepare(canvas) {
  const rect = canvas.getBoundingClientRect(), ratio = Math.min(devicePixelRatio || 1, 2), width = Math.max(1, Math.round(rect.width)), height = Math.max(1, Math.round(rect.height));
  if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) { canvas.width = Math.round(width * ratio); canvas.height = Math.round(height * ratio); }
  const ctx = canvas.getContext("2d"); ctx.setTransform(ratio, 0, 0, ratio, 0, 0); return { ctx, width, height };
}
function buildLayout(data) {
  const ids = [...new Set(data.frames.flatMap((sample) => sample.cells.map((cell) => cell.id)))].sort((a, b) => hash(a) - hash(b) || a.localeCompare(b));
  const used = new Set(), capacity = 65536;
  ids.forEach((id) => { let slot = hash(id) % capacity, attempts = 0; while (used.has(slot) && attempts++ < capacity) slot = (slot + 1) % capacity; used.add(slot); view.layout.set(id, slot); });
}
function denseLayout(sample) {
  const living = new Set(sample.cells.map((cell) => cell.id));
  for (const id of view.layout.keys()) if (!living.has(id)) view.layout.delete(id);
  const used = new Set(view.layout.values());
  for (const cell of sample.cells) if (!view.layout.has(cell.id)) { let slot = hash(cell.id) % 65536; while (used.has(slot)) slot = (slot + 1) % 65536; used.add(slot); view.layout.set(cell.id, slot); }
}

function drawChamber() {
  const canvas = $("chamber"), { ctx, width, height } = prepare(canvas), sample = frame(); ctx.fillStyle = "#080d0b"; ctx.fillRect(0, 0, width, height); view.hits = [];
  if (!sample) return;
  const cells = sample.cells, padding = 36, usableWidth = Math.max(1, width - padding * 2), usableHeight = Math.max(1, height - padding * 2);
  const base = Math.max(2.4, Math.min(12, Math.sqrt(usableWidth * usableHeight / Math.max(1, cells.length)) * .24));
  const previousTick = view.dense ? sample.tick - 1 : view.index ? view.data.frames[view.index - 1].tick : -1;
  for (const cell of cells) {
    const slot = view.layout.get(cell.id) ?? hash(cell.id) % 65536, x = padding + ((slot % 256 + .5) / 256) * usableWidth, y = padding + ((Math.floor(slot / 256) + .5) / 256) * usableHeight;
    const ratio = Math.max(0, Math.min(1, finite(cell.mass_mol) / Math.max(finite(cell.division_mass_mol, 1), 1e-30)));
    const length = base * (1.35 + 1.45 * Math.sqrt(ratio)), thickness = base * (.58 + .58 * Math.sqrt(ratio)), angle = (hash(`${cell.id}:angle`) % 6283) / 1000;
    const selected = cell.id === view.selected, born = view.index > 0 && (view.dense ? cell.birth_tick === sample.tick : cell.birth_tick > previousTick && cell.birth_tick <= sample.tick);
    ctx.save(); ctx.translate(x, y); ctx.rotate(angle); ctx.fillStyle = color(cell.genome_key); ctx.globalAlpha = .86; ctx.shadowColor = color(cell.genome_key); ctx.shadowBlur = selected ? 11 : 3;
    ctx.beginPath(); ctx.roundRect(-length / 2, -thickness / 2, length, thickness, thickness / 2); ctx.fill(); ctx.shadowBlur = 0; ctx.globalAlpha = .32; ctx.strokeStyle = "#f0fff7"; ctx.lineWidth = .7; ctx.stroke();
    if (selected || born) { ctx.globalAlpha = 1; ctx.strokeStyle = selected ? "#fff" : "#efbc77"; ctx.lineWidth = selected ? 1.8 : 1.2; ctx.beginPath(); ctx.roundRect(-length / 2 - 3, -thickness / 2 - 3, length + 6, thickness + 6, (thickness + 6) / 2); ctx.stroke(); }
    ctx.restore(); view.hits.push({ id: cell.id, x, y, radius: Math.max(8, length * .65) });
  }
  canvas.setAttribute("aria-label", `${cells.length} actual living cells at recorded tick ${sample.tick}. Schematic well-mixed inventory; glyph positions are not spatial coordinates. Select a cell to inspect it.`);
}

function addPair(list, key, value) { const term = document.createElement("dt"), detail = document.createElement("dd"); term.textContent = key; detail.textContent = value; list.append(term, detail); }
function addObject(host, title, object) {
  if (!object || !Object.keys(object).length) return; const section = document.createElement("div"); section.className = "subobject"; const label = document.createElement("span"); label.className = "eyebrow"; label.textContent = title; const list = document.createElement("dl"); list.className = "kv";
  Object.entries(object).forEach(([key, value]) => addPair(list, key, inspectText(value))); section.append(label, list); host.append(section);
}
function renderInspector() {
  const sample = frame(), cell = sample?.cells.find((item) => item.id === view.selected), host = $("cell-detail"); host.replaceChildren();
  if (!cell) { host.className = "empty"; host.textContent = view.selected ? view.dense ? `Cell ${view.selected} is not present at shown tick ${sample.tick}.` : "This cell is not alive in the selected frame." : "Select a living cell in the chamber."; $("selection-label").textContent = view.selected ? view.dense ? "not present" : "not alive" : "none selected"; return; }
  host.className = ""; $("selection-label").textContent = `generation ${cell.generation}`; const heading = document.createElement("div"); heading.className = "selected"; const dot = document.createElement("span"); dot.className = "swatch"; dot.style.background = color(cell.genome_key); const code = document.createElement("b"); code.textContent = `${cell.genome_key} · cell ${cell.id}`; heading.append(dot, code); host.append(heading);
  const list = document.createElement("dl"); list.className = "kv"; addPair(list, "id", cell.id); addPair(list, "parent", cell.parent_id ?? "founder"); addPair(list, "birth tick", String(cell.birth_tick)); addPair(list, "age", `${numberText(cell.age_s)} s`); addPair(list, "mass", `${numberText(cell.mass_mol)} mol`); if (cell.mass_units != null) addPair(list, "mass units", cell.mass_units); addPair(list, "energy", `${numberText(cell.energy_j)} J`); if (cell.energy_units != null) addPair(list, "energy units", cell.energy_units); addPair(list, "division mass", `${numberText(cell.division_mass_mol)} mol`); addPair(list, "starvation", `${numberText(cell.starvation_s)} s`); host.append(list);
  const physiology = view.metadata.genomes?.[cell.genome_key]; addObject(host, "Genome", physiology?.genome); addObject(host, "Phenotype", physiology?.phenotype);
}

function chartBase(canvas) { const { ctx, width, height } = prepare(canvas), pad = { left: 34, right: 7, top: 7, bottom: 18 }; ctx.clearRect(0, 0, width, height); ctx.strokeStyle = "#d8ded9"; ctx.lineWidth = 1; for (let i = 0; i < 3; i++) { const y = pad.top + (height - pad.top - pad.bottom) * i / 2 + .5; ctx.beginPath(); ctx.moveTo(pad.left, y); ctx.lineTo(width - pad.right, y); ctx.stroke(); } return { ctx, width, height, pad, innerWidth: width - pad.left - pad.right, innerHeight: height - pad.top - pad.bottom }; }
function xAt(sample, chart) { const frames = view.data.frames, first = frames[0].tick, span = frames.at(-1).tick - first; return chart.pad.left + (span ? (sample.tick - first) / span * chart.innerWidth : chart.innerWidth / 2); }
function cursor(chart) { const x = xAt(frame(), chart); chart.ctx.strokeStyle = "#6f7c74"; chart.ctx.lineWidth = 1; chart.ctx.beginPath(); chart.ctx.moveTo(x + .5, chart.pad.top); chart.ctx.lineTo(x + .5, chart.height - chart.pad.bottom); chart.ctx.stroke(); }
function line(chart, values, stroke) { const max = Math.max(...values, 1e-30); chart.ctx.beginPath(); view.data.frames.forEach((sample, index) => { const x = xAt(sample, chart), y = chart.pad.top + chart.innerHeight * (1 - values[index] / max); index ? chart.ctx.lineTo(x, y) : chart.ctx.moveTo(x, y); }); chart.ctx.strokeStyle = stroke; chart.ctx.lineWidth = 1.45; chart.ctx.stroke(); }
function renderCharts() {
  if (!view.data) return; const frames = view.data.frames, population = chartBase($("population-chart")); line(population, frames.map((item) => finite(item.summary.living_cells)), "#489c75"); line(population, frames.map((item) => finite(item.summary.total_biomass_mol)), "#5c6861"); cursor(population);
  population.ctx.fillStyle = "#6c776f"; population.ctx.font = "9px ui-monospace,monospace"; population.ctx.fillText(String(frames[0].tick), population.pad.left, population.height - 4); const last = String(frames.at(-1).tick); population.ctx.fillText(last, population.width - population.pad.right - population.ctx.measureText(last).width, population.height - 4);
  const genome = chartBase($("genome-chart")), keys = Object.keys(view.data.genomes || {}).sort(), totals = frames.map((sample) => Math.max(1, sample.cells.length)), lower = frames.map(() => 0);
  keys.forEach((key) => { const counts = frames.map((sample) => sample.cells.reduce((sum, cell) => sum + (cell.genome_key === key ? 1 : 0), 0)); genome.ctx.beginPath(); genome.ctx.moveTo(xAt(frames[0], genome), genome.pad.top + genome.innerHeight); frames.forEach((sample, index) => { const top = lower[index] + counts[index] / totals[index]; genome.ctx.lineTo(xAt(sample, genome), genome.pad.top + genome.innerHeight * (1 - top)); }); for (let index = frames.length - 1; index >= 0; index--) genome.ctx.lineTo(xAt(frames[index], genome), genome.pad.top + genome.innerHeight * (1 - lower[index])); genome.ctx.closePath(); genome.ctx.fillStyle = color(key); genome.ctx.globalAlpha = .72; genome.ctx.fill(); genome.ctx.globalAlpha = 1; counts.forEach((count, index) => lower[index] += count / totals[index]); }); cursor(genome);
  const resource = chartBase($("resource-chart")), resourceIds = [...new Set(frames.flatMap((sample) => sample.resources.map((item) => item.id)))].sort(); resourceIds.forEach((id) => { const values = frames.map((sample) => finite(sample.resources.find((item) => item.id === id)?.concentration)), max = Math.max(...values, 1e-30); resource.ctx.beginPath(); frames.forEach((sample, frameIndex) => { const x = xAt(sample, resource), y = resource.pad.top + resource.innerHeight * (1 - values[frameIndex] / max); frameIndex ? resource.ctx.lineTo(x, y) : resource.ctx.moveTo(x, y); }); resource.ctx.strokeStyle = color(id); resource.ctx.lineWidth = 1.2; resource.ctx.stroke(); }); cursor(resource);
}

function renderFrequencies() { const cells = frame().cells, counts = new Map(); cells.forEach((cell) => counts.set(cell.genome_key, (counts.get(cell.genome_key) || 0) + 1)); const host = $("frequencies"); host.replaceChildren(); [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).forEach(([key, count]) => { const row = document.createElement("div"); row.className = "frequency"; const label = document.createElement("span"); label.textContent = key; const value = document.createElement("span"); value.className = "mono"; value.textContent = `${count} · ${cells.length ? (count / cells.length * 100).toFixed(1) : "0.0"}%`; const track = document.createElement("div"); track.className = "track"; const fill = document.createElement("div"); fill.className = "fill"; fill.style.background = color(key); fill.style.width = `${cells.length ? count / cells.length * 100 : 0}%`; track.append(fill); row.append(label, value, track); host.append(row); }); $("genome-total").textContent = `${counts.size} variant${counts.size === 1 ? "" : "s"}`; }
function renderResources() { const resources = frame().resources, host = $("resources"), max = Math.max(...resources.map((item) => finite(item.concentration)), 1e-30); host.replaceChildren(); resources.forEach((resource) => { const row = document.createElement("div"); row.className = "resource"; const label = document.createElement("span"); label.textContent = resource.id; const value = document.createElement("span"); value.className = "mono"; value.textContent = `${numberText(resource.concentration)} mol/m³`; const track = document.createElement("div"); track.className = "track"; const fill = document.createElement("div"); fill.className = "fill"; fill.style.background = color(resource.id); fill.style.width = `${Math.max(0, Math.min(100, finite(resource.concentration) / max * 100))}%`; track.append(fill); row.append(label, value, track); host.append(row); }); }
function renderFacts() { const data = view.metadata, identity = data.identity, model = data.model, experiment = data.experiment, list = $("experiment-facts"); list.replaceChildren(); addPair(list, "scenario", identity.scenario); addPair(list, "environment", model.environment); addPair(list, "volume", `${numberText(model.volume_m3)} m³`); addPair(list, "temperature", `${numberText(model.temperature_k)} K`); addPair(list, "frames", (view.dense ? experiment.frames : data.frames.length).toLocaleString("en-US")); addPair(list, "sample interval", `${experiment.sample_every} ticks`); addPair(list, "time step", `${numberText(experiment.dt_seconds)} s`); addPair(list, "matter residual max", experiment.matter_residual_max); addPair(list, "energy residual max", experiment.energy_residual_max); addPair(list, "canonical config", typeof data.canonical_config === "string" ? "included in dataset" : "unavailable"); const term = document.createElement("dt"), detail = document.createElement("dd"), link = document.createElement("a"); term.textContent = "source commit"; link.href = `https://github.com/emevart/liminis/commit/${encodeURIComponent(identity.source_commit)}`; link.textContent = identity.source_commit; detail.append(link); list.append(term, detail); if (view.dense) { addPair(list, "dense producer", data.provenance.producer_commit); addPair(list, "overview plots", `${view.data.frames.length} sparse archive frames, every ${view.data.experiment.sample_every} ticks`); addPair(list, "archive source", view.data.identity.source_commit); } (data.limitations || []).forEach((limitation, index) => addPair(list, `limitation ${index + 1}`, limitation)); $("checks").textContent = `${experiment.checked_ticks} checked ticks`; }

function updatePlayButton() {
  const button = $("play"), ended = view.index === view.clock.count - 1, playing = view.session ? view.session.playing || view.session.buffering : view.clock.playing;
  button.innerHTML = playing ? icons.pause : ended ? icons.replay : icons.play;
  button.setAttribute("aria-label", playing ? "Pause recording" : ended ? "Replay recording" : "Play recording"); button.title = playing ? "Pause" : ended ? "Replay" : "Play";
  button.disabled = !view.ready; $("previous").disabled = !view.ready || view.index === 0; $("next").disabled = !view.ready || ended;
}
function renderFrame() {
  const sample = frame(), summary = sample.summary, previous = !view.dense && view.index ? view.data.frames[view.index - 1].summary : null; $("living").textContent = summary.living_cells.toLocaleString("en-US"); $("births").textContent = summary.births.toLocaleString("en-US"); $("deaths").textContent = summary.deaths.toLocaleString("en-US"); $("divisions").textContent = summary.divisions.toLocaleString("en-US"); $("generation").textContent = summary.generation_max; $("ledger").textContent = sample.residual ? `M ${sample.residual.matter} · E ${sample.residual.energy}` : "not checked"; $("tick").textContent = sample.tick.toLocaleString("en-US"); $("time").textContent = numberText(sample.sim_time); $("frame-label").textContent = view.dense ? `recorded tick ${sample.tick} · ${view.index + 1} of ${view.clock.count} states` : `recorded frame ${view.index + 1} of ${view.clock.count}`; $("frame-count").textContent = `${view.index + 1} / ${view.clock.count}`;
  $("frame-change").textContent = view.dense ? "Cumulative counters at shown tick · no within-tick event reconstruction" : previous ? `+${summary.births - previous.births} births · +${summary.deaths - previous.deaths} deaths since prior frame` : "initial recorded frame"; drawChamber(); renderInspector(); renderFrequencies(); renderResources(); renderCharts(); updatePlayButton();
}
function durationText(seconds) {
  if (!Number.isFinite(seconds)) return "beyond numeric range";
  if (seconds === 0) return "0 s";
  if (seconds < .01) return "< 0.01 s";
  if (seconds < 60) return `${numberText(seconds, 3)} s`;
  if (seconds < 3600) return `${numberText(seconds / 60, 3)} min`;
  if (seconds < 86400) return `${numberText(seconds / 3600, 3)} h`;
  return `${numberText(seconds / 86400, 3)} d`;
}
function renderClock(snapshot) {
  $("playhead").textContent = numberText(snapshot.playhead, 8);
  $("scrub").value = String(snapshot.playhead);
  $("scrub").setAttribute("aria-valuetext", `${numberText(snapshot.playhead, 8)} model seconds; shown sample ${numberText(snapshot.stateTime)} model seconds`);
  $("duration").textContent = durationText(snapshot.durationSeconds); $("remaining").textContent = durationText(snapshot.remainingSeconds);
}
function showSnapshot(snapshot, force = false) {
  const changed = snapshot.index !== view.index; view.index = snapshot.index;
  if (changed || force) renderFrame();
  renderClock(snapshot); updatePlayButton();
}
function pause(message = "Paused") {
  if (view.session) { cancelAnimationFrame(view.raf); view.raf = 0; view.session.pause(message); return; }
  const snapshot = view.clock.pause(performance.now()); cancelAnimationFrame(view.raf); view.raf = 0;
  showSnapshot(snapshot); $("playback-status").textContent = snapshot.ended ? "Recording ended · Replay available" : message;
}
function start() {
  if (!view.ready || document.hidden) return;
  if (view.session) { cancelAnimationFrame(view.raf); view.raf = 0; view.lastDraw = 0; view.session.start(); return; }
  cancelAnimationFrame(view.raf); const snapshot = view.clock.start(performance.now()); view.lastDraw = 0;
  showSnapshot(snapshot); $("playback-status").textContent = snapshot.ended ? "Recording ended" : "Playing";
  if (snapshot.playing) view.raf = requestAnimationFrame(advance);
}
function advance(now) {
  view.raf = 0;
  if (view.session) { if (document.hidden) { pause("Paused while tab was hidden · press Play to resume"); return; } if (view.session.buffering || !view.session.playing) return; if (now - view.lastDraw >= 1000 / view.drawFps - .5) { view.lastDraw = now; view.session.advance(now); } if (view.session.playing && !view.session.buffering && !view.raf) view.raf = requestAnimationFrame(advance); return; }
  if (!view.clock.playing) return;
  if (document.hidden) { pause("Paused while tab was hidden · press Play to resume"); return; }
  const interval = 1000 / view.drawFps;
  if (now - view.lastDraw >= interval - .5) {
    view.lastDraw = now; const snapshot = view.clock.advance(now); showSnapshot(snapshot);
    if (snapshot.ended) { $("playback-status").textContent = "Recording ended · Replay available"; return; }
  }
  view.raf = requestAnimationFrame(advance);
}
function setSpeed(value) {
  if (!validPlaybackRate(value)) {
    $("speed").value = String(view.clock.rate); $("rate-error").textContent = "Enter a positive finite speed multiplier. The previous speed is retained."; $("rate-error").hidden = false; return;
  }
  $("rate-error").hidden = true; if (view.session) { view.session.setRate(value); $("speed").value = String(view.clock.rate); $("speed-preset").value = view.clock.rate === defaultPlaybackRate(timeline()) ? "overview" : [...$("speed-preset").options].some((option) => option.value === String(view.clock.rate)) ? String(view.clock.rate) : "custom"; return; } const snapshot = view.clock.setRate(value, performance.now());
  $("speed").value = String(view.clock.rate);
  const overview = defaultPlaybackRate(view.data.frames.map((sample) => sample.sim_time));
  $("speed-preset").value = view.clock.rate === overview ? "overview" : [...$("speed-preset").options].some((option) => option.value === String(view.clock.rate)) ? String(view.clock.rate) : "custom";
  showSnapshot(snapshot); if (snapshot.ended) $("playback-status").textContent = "Recording ended · Replay available";
}
function bind() {
  $("play").addEventListener("click", () => (view.session ? view.session.playing || view.session.buffering : view.clock.playing) ? pause() : start());
  for (const [id, offset] of [["previous", -1], ["next", 1]]) $(id).addEventListener("click", () => { cancelAnimationFrame(view.raf); view.raf = 0; if (view.session) { void view.session.step(offset); return; } showSnapshot(view.clock.step(offset, performance.now())); $("playback-status").textContent = "Paused at recorded sample"; });
  $("scrub").addEventListener("input", (event) => { cancelAnimationFrame(view.raf); view.raf = 0; if (view.session) { void view.session.seek(Number(event.target.value)); return; } showSnapshot(view.clock.seek(Number(event.target.value), performance.now())); $("playback-status").textContent = "Paused at playback time · shown state is held"; });
  $("speed").addEventListener("change", () => setSpeed($("speed").value));
  $("speed-preset").addEventListener("change", () => setSpeed($("speed-preset").value === "overview" ? defaultPlaybackRate(timeline()) : $("speed-preset").value));
  $("draw-fps").addEventListener("change", () => { const fps = Number($("draw-fps").value); if (fps === 30 || fps === 60) view.drawFps = fps; });
  document.addEventListener("visibilitychange", () => { if (document.hidden && (view.clock.playing || view.session?.buffering)) pause("Paused while tab was hidden · press Play to resume"); });
  $("chamber").addEventListener("pointerdown", (event) => { const rect = event.currentTarget.getBoundingClientRect(), x = event.clientX - rect.left, y = event.clientY - rect.top, hit = [...view.hits].reverse().find((item) => Math.hypot(x - item.x, y - item.y) <= item.radius); view.selected = hit?.id ?? null; drawChamber(); renderInspector(); });
  $("chamber").addEventListener("keydown", (event) => { if (!["ArrowLeft", "ArrowRight"].includes(event.key)) return; const cells = frame().cells; if (!cells.length) return; event.preventDefault(); const current = Math.max(0, cells.findIndex((cell) => cell.id === view.selected)), offset = event.key === "ArrowRight" ? 1 : -1; view.selected = cells[(current + offset + cells.length) % cells.length].id; drawChamber(); renderInspector(); });
  addEventListener("resize", () => { drawChamber(); renderCharts(); });
  if (view.session) addEventListener("pagehide", () => { cancelAnimationFrame(view.raf); view.raf = 0; view.session.close(); view.dense.close(); });
}

function denseStatus(status, detail) {
  cancelAnimationFrame(view.raf); view.raf = 0;
  $("workspace").setAttribute("aria-busy", status === "buffering" ? "true" : "false");
  if (status === "error") {
    view.ready = false; view.dense.close(); $("error").hidden = false;
    $("error").textContent = `Dense recording stopped: ${detail instanceof Error ? detail.message : String(detail)}. The last validated state remains shown. Use recording=archive to choose the sparse archive.`;
    ["play", "previous", "next", "scrub", "speed", "speed-preset", "draw-fps"].forEach((id) => $(id).disabled = true);
    $("playback-status").textContent = "Recording error · last validated state held";
  } else {
    $("playback-status").textContent = status === "buffering" ? `Buffering recorded tick ${detail} · shown tick ${frame()?.tick ?? "none"}` : status === "ended" ? "Recording ended · Replay available" : status === "playing" ? "Playing" : status === "paused" ? "Paused at validated recorded tick" : status;
    if (status === "buffering") renderClock(view.session.snapshot);
    updatePlayButton();
    if (view.ready && view.session.playing && !view.session.buffering && !document.hidden) view.raf = requestAnimationFrame(advance);
  }
}

async function load() {
  try {
    const { entry, data, mode, dense } = await loadRecording(location.search);
    const archiveLink = $("archive-link"); if (archiveLink) { archiveLink.href = `./observe.html?experiment=${encodeURIComponent(entry.id)}&recording=archive`; archiveLink.hidden = mode !== "dense"; }
    view.data = data; view.dense = mode === "dense" ? dense : null; view.metadata = view.dense ? dense.manifest : data;
    view.clock = new PlaybackClock(timeline()); if (!view.dense) buildLayout(data);
    $("seed").textContent = view.metadata.identity.seed; $("config").textContent = view.metadata.identity.config_hash; $("world").textContent = view.metadata.identity.world_format_version; $("scrub").min = String(view.clock.firstTime); $("scrub").max = String(view.clock.endTime); $("speed").value = String(view.clock.rate);
    document.title = `Liminis · ${entry.title}`;
    if (view.dense) {
      $("sample-cadence").textContent = `Schematic well-mixed inventory; no spatial coordinates. ${view.clock.count.toLocaleString("en-US")} real states · every tick / ${numberText(view.metadata.experiment.dt_seconds)} model s. Overview plots: ${data.frames.length} sparse archive frames, every ${numberText(data.experiment.sample_every)} ticks.`;
      $("previous").setAttribute("aria-label", "Previous recorded tick"); $("previous").title = "Previous recorded tick"; $("next").setAttribute("aria-label", "Next recorded tick"); $("next").title = "Next recorded tick";
      for (const id of ["population-chart", "genome-chart", "resource-chart"]) {
        const label = document.createElement("span"); label.className = "eyebrow"; label.textContent = `Sparse archive overview · ${data.frames.length} frames · every ${numberText(data.experiment.sample_every)} ticks`;
        $(id).before(label); $(id).setAttribute("aria-label", `${$(id).getAttribute("aria-label")} · sparse archive overview, not every-tick history`);
      }
      view.session = new DensePlaybackSession(view.clock, dense.seek.bind(dense), {
        now: () => performance.now(), status: denseStatus,
        commit: (sample, snapshot) => {
          const changed = snapshot.index !== view.index || !view.ready; view.index = snapshot.index;
          if (changed) { denseLayout(sample); renderFrame(); }
          renderClock(snapshot); updatePlayButton();
        },
      });
      if (!await view.session.seek(0)) throw view.session.failure ?? new Error("The initial dense state could not be loaded.");
    } else {
      const times = timeline(), gaps = times.slice(1).map((time, index) => time - times[index]);
      const shortest = Math.min(...gaps), longest = Math.max(...gaps);
      $("sample-cadence").textContent = `${data.frames.length} real samples · every ${numberText(data.experiment.sample_every)} ticks / ${numberText(data.experiment.sample_every * data.experiment.dt_seconds)} model s${shortest !== longest ? ` · final interval ${numberText(gaps.at(-1))} model s` : ""}.`;
      showSnapshot(view.clock.advance(performance.now()), true);
    }
    renderFacts(); bind();
    const download = document.querySelector(".download"); download.href = entry.recording; download.hidden = false;
    if (view.dense) download.textContent = `Download sparse archive (${data.frames.length} frames)`;
    view.ready = true; ["scrub", "speed", "speed-preset", "draw-fps"].forEach((id) => $(id).disabled = false); updatePlayButton(); $("loading").hidden = true; $("workspace").setAttribute("aria-busy", "false");
  } catch (error) { view.session?.close(); view.dense?.close(); const download = document.querySelector(".download"); download.removeAttribute("href"); download.hidden = true; ["play", "previous", "next", "scrub", "speed", "speed-preset", "draw-fps"].forEach((id) => $(id).disabled = true); $("loading").hidden = true; $("error").hidden = false; $("error").textContent = error instanceof Error ? error.message : String(error); $("workspace").setAttribute("aria-busy", "false"); }
}
load();
