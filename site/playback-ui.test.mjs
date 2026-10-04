import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createHash, webcrypto } from "node:crypto";
import { gunzipSync, gzipSync } from "node:zlib";
import test from "node:test";
import { Script, createContext } from "node:vm";
import { DensePlaybackSession, PlaybackClock, defaultPlaybackRate, validPlaybackRate } from "./playback.mjs";
import { validateRecording } from "./recording.mjs";
import { createDenseRecordingLoader, decodeDenseChunk } from "./dense-recording.mjs";

// Node event-wiring coverage only: this DOM/canvas/RAF harness is not a browser
// and does not claim layout, native input validity, canvas pixels, or browser QA.
// Keep the application body intact; inject its actual clock imports and the
// schema-validated fixture at the recording-loader boundary. The real loader
// has separate request, byte-count, digest and schema tests.
const source = (await readFile(new URL("./observer.js", import.meta.url), "utf8"))
  .replace(/^import[^\n]+\n/gm, "");
const html = await readFile(new URL("./observe.html", import.meta.url), "utf8");
const script = new Script(source, { filename: "observer.js" });
const controls = ["play", "previous", "next", "scrub", "speed", "speed-preset", "draw-fps"];

function fixture() {
  const data = {
    schema_version: 1, kind: "recorded_cell_experiment",
    identity: { seed: "42", config_hash: "fixture", source_commit: "fixture", scenario: "fixture", world_format_version: 1 },
    model: { environment: "well_mixed", spatial_positions: false, volume_m3: 1, temperature_k: 300 },
    canonical_config: "fixture", limitations: [], genomes: { founder: {} },
    experiment: { steps: 23, sample_every: 10, dt_seconds: 1, checked_ticks: 23, matter_residual_max: "0", energy_residual_max: "0" },
    frames: [0, 10, 20, 23].map((tick, index) => ({
      tick, sim_time: tick,
      summary: { living_cells: 1, births: 0, deaths: 0, divisions: 0, generation_max: 0, total_biomass_mol: 1, total_cell_energy_j: 1 },
      cells: [{ id: "1", genome_key: "founder", birth_tick: 0, generation: 0, mass_mol: 1, energy_j: 1, division_mass_mol: 2, age_s: tick, starvation_s: 0 }],
      resources: [{ id: "food", concentration: 1, amount_mol: 1 }],
      residual: index ? { matter: "0", energy: "0" } : null,
      accounting: { matter_ids: ["C"], free_matter_units: ["0"], medium_matter_units: ["0"], bath_heat_units: "0", death_mass_units: "0", growth_extent_units: "0", medium_energy_units: "0" },
    })),
  };
  validateRecording(data);
  return data;
}

class EventTarget {
  listeners = new Map();
  addEventListener(type, callback) {
    const listeners = this.listeners.get(type) || [];
    listeners.push(callback); this.listeners.set(type, listeners);
  }
  dispatch(type, detail = {}) {
    for (const callback of this.listeners.get(type) || []) {
      callback({ type, target: this, currentTarget: this, preventDefault() {}, ...detail });
    }
  }
}

class Element extends EventTarget {
  constructor(tag, attributes = "") {
    super(); this.tagName = tag; this.attributes = new Map(); this.style = {}; this.children = [];
    this.disabled = /\bdisabled\b/.test(attributes); this.hidden = /\bhidden\b/.test(attributes);
    this.value = attributes.match(/\bvalue="([^"]*)"/)?.[1] || "";
    this.width = 0; this.height = 0; this.options = []; this.textWrites = 0; this.canvasCalls = [];
    this.context = new Proxy({ measureText: (text) => ({ width: String(text).length * 6 }) }, {
      get: (target, key) => key in target ? target[key] : (...args) => this.canvasCalls.push([key, ...args]),
      set: (target, key, value) => { target[key] = value; this.canvasCalls.push(["set", key, value]); return true; },
    });
  }
  get textContent() { return this.text || ""; }
  set textContent(value) { this.text = String(value); this.textWrites++; }
  get href() { return this.attributes.get("href"); }
  set href(value) { this.attributes.set("href", value); }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  removeAttribute(name) { this.attributes.delete(name); }
  append(...children) { this.children.push(...children); }
  before(...siblings) { this.beforeNodes = [...(this.beforeNodes || []), ...siblings]; }
  replaceChildren(...children) { this.children = [...children]; this.text = ""; }
  getBoundingClientRect() { return { left: 0, top: 0, width: 640, height: 360 }; }
  getContext(type) { assert.equal(type, "2d"); return this.context; }
}

function harness({ data = fixture(), dense, sessionClass = DensePlaybackSession, loadError, deferred = false, hidden = false } = {}) {
  const nodes = new Map();
  for (const match of html.matchAll(/<([a-z]+)\b([^>]*\bid="([^"]+)"[^>]*)>/g)) {
    nodes.set(match[3], new Element(match[1], match[2]));
  }
  for (const match of html.matchAll(/<select\b[^>]*id="([^"]+)"[^>]*>([\s\S]*?)<\/select>/g)) {
    const select = nodes.get(match[1]);
    select.options = [...match[2].matchAll(/<option\b([^>]*)>/g)].map((option) => new Element("option", option[1]));
    select.value = select.options[0].value;
  }
  const document = new EventTarget();
  document.hidden = hidden;
  document.getElementById = (id) => {
    assert.ok(nodes.has(id), `Harness has no element #${id}`); return nodes.get(id);
  };
  document.createElement = (tag) => new Element(tag);
  document.querySelector = (selector) => {
    assert.equal(selector, ".download"); return nodes.get("download");
  };
  const window = new EventTarget(), rafs = new Map();
  let now = 0, nextRaf = 1, requested = 0, loadCalls = 0, fetchCalls = 0, reloads = 0, release;
  const barrier = deferred ? new Promise((resolve) => { release = resolve; }) : Promise.resolve();
  const context = createContext({
    document, location: { search: "?experiment=fixture", reload: () => { reloads++; } }, devicePixelRatio: 1,
    performance: { now: () => now }, PlaybackClock, DensePlaybackSession: sessionClass, defaultPlaybackRate, validPlaybackRate,
    loadRecording: async (search) => {
      assert.equal(search, "?experiment=fixture"); loadCalls++;
      await barrier;
      if (loadError) throw loadError;
      return { entry: { title: "Fixture", recording: "./data/fixture.json" }, data, mode: dense ? "dense" : "archive", dense };
    },
    fetch: () => { fetchCalls++; throw new Error("Playback must not fetch again."); },
    addEventListener: window.addEventListener.bind(window),
    requestAnimationFrame: (callback) => { const id = nextRaf++; requested++; rafs.set(id, callback); return id; },
    cancelAnimationFrame: (id) => rafs.delete(id),
  });
  const loaded = script.runInContext(context);
  const element = (id) => document.getElementById(id);
  return {
    loaded, release, element, document,
    at: (time) => { now = time; },
    input: (id, value, type = "change") => {
      const target = element(id); assert.equal(target.disabled, false, `#${id} must be enabled`);
      target.value = String(value); target.dispatch(type);
    },
    click: (id = "play") => {
      const target = element(id); assert.equal(target.disabled, false, `#${id} must be enabled`); target.dispatch("click");
    },
    frame: (time) => {
      now = time; const pending = [...rafs.entries()]; rafs.clear();
      for (const [, callback] of pending) callback(time);
    },
    visibility: (value) => { document.hidden = value; document.dispatch("visibilitychange"); },
    pagehide: (persisted = false) => window.dispatch("pagehide", { persisted }),
    pageshow: (persisted = false) => window.dispatch("pageshow", { persisted }),
    get layoutCount() { return new Script("view.layout.size").runInContext(context); },
    get pending() { return rafs.size; }, get requested() { return requested; },
    get loadCalls() { return loadCalls; }, get fetchCalls() { return fetchCalls; },
    get reloads() { return reloads; },
    get playhead() { return Number(element("scrub").value); },
    get stateTime() { return Number(element("time").textContent.replaceAll(",", "")); },
  };
}

function assertNoExtraIO(ui) {
  assert.equal(ui.loadCalls, 1); assert.equal(ui.fetchCalls, 0); assert.ok(ui.pending <= 1, "At most one RAF is queued");
}

function assertTime(ui, playhead, stateTime) {
  assert.ok(Math.abs(ui.playhead - playhead) < 1e-9, `Playhead ${ui.playhead} should be ${playhead}`);
  assert.equal(ui.stateTime, stateTime);
}

test("load gates every control and download until the trusted loader result resolves", async () => {
  const ui = harness({ deferred: true });
  for (const id of controls) assert.equal(ui.element(id).disabled, true, id);
  assert.equal(ui.element("download").hidden, true); assert.equal(ui.pending, 0);
  ui.release(); await ui.loaded;
  for (const id of ["play", "next", "scrub", "speed", "speed-preset", "draw-fps"]) assert.equal(ui.element(id).disabled, false, id);
  assert.equal(ui.element("previous").disabled, true);
  assert.equal(ui.element("download").href, "./data/fixture.json"); assert.equal(ui.element("download").hidden, false);
  assert.equal(ui.element("workspace").getAttribute("aria-busy"), "false");
  assert.equal(ui.element("sample-cadence").textContent, "4 real samples · every 10 ticks / 10 model s · final interval 3 model s.");
  assertNoExtraIO(ui);
});

test("actual pause, rate-change and resume handlers preserve a fractional playhead without extra schedules or requests", async () => {
  const ui = harness(); await ui.loaded;
  ui.at(1000); ui.input("speed", 2); ui.click(); assert.equal(ui.pending, 1);
  ui.frame(1217); assertTime(ui, .434, 0);
  const schedules = ui.requested;
  ui.at(1300); ui.input("speed", 3); assertTime(ui, .6, 0);
  assert.equal(ui.requested, schedules); assert.equal(ui.pending, 1);
  ui.at(1375); ui.click(); assertTime(ui, .825, 0); assert.equal(ui.pending, 0);
  assert.equal(ui.element("play").getAttribute("aria-label"), "Play recording");
  ui.at(1_000_000); ui.click(); ui.frame(1_000_500); assertTime(ui, 2.325, 0);
  assert.equal(ui.element("play").getAttribute("aria-label"), "Pause recording");
  assertNoExtraIO(ui);
});

test("manual invalid multiplier feedback retains the previous speed; presets and huge finite input use the same clock", async () => {
  const ui = harness(); await ui.loaded;
  ui.input("speed", 2.5);
  for (const invalid of ["", "0", "-1", "NaN", "Infinity", "1e309"]) {
    ui.input("speed", invalid);
    assert.equal(ui.element("speed").value, "2.5"); assert.equal(ui.element("rate-error").hidden, false);
    assert.match(ui.element("rate-error").textContent, /positive finite.*previous speed/i);
    assertTime(ui, 0, 0);
  }
  ui.input("speed-preset", "1000"); assert.equal(ui.element("speed").value, "1000");
  assert.equal(ui.element("rate-error").hidden, true);
  ui.input("speed", "1e100"); assert.equal(ui.element("speed").value, "1e+100");
  assert.equal(ui.element("speed-preset").value, "custom");
  ui.input("speed-preset", "overview"); assert.equal(ui.element("speed").value, String(23 / 120));
  assert.equal(ui.element("duration").textContent, "2 min"); assert.equal(ui.requested, 0);
  assertNoExtraIO(ui);
});

test("time seek holds the real floor sample, previous/next pause, and the irregular end offers replay", async () => {
  const ui = harness(); await ui.loaded;
  ui.input("speed", 1); ui.click(); assert.equal(ui.pending, 1);
  ui.at(3000); ui.input("scrub", 19.75, "input"); assertTime(ui, 19.75, 10);
  assert.match(ui.element("scrub").getAttribute("aria-valuetext"), /19.75 model seconds; shown sample 10 model seconds/);
  assert.equal(ui.pending, 0); assert.equal(ui.element("play").getAttribute("aria-label"), "Play recording");
  ui.click("previous"); assertTime(ui, 0, 0); assert.equal(ui.element("previous").disabled, true);
  ui.click("next"); assertTime(ui, 10, 10);
  ui.click("next"); assertTime(ui, 20, 20);
  ui.input("scrub", 22.999, "input"); assertTime(ui, 22.999, 20);
  ui.click("next"); assertTime(ui, 23, 23);
  assert.equal(ui.element("next").disabled, true); assert.equal(ui.element("remaining").textContent, "0 s");
  assert.equal(ui.element("play").getAttribute("aria-label"), "Replay recording");
  assert.equal(ui.requested, 1, "Seeking and stepping do not schedule animation");
  ui.at(5000); ui.click(); assertTime(ui, 0, 0); assert.equal(ui.pending, 1);
  ui.frame(28000); assertTime(ui, 23, 23); assert.equal(ui.pending, 0);
  assert.match(ui.element("playback-status").textContent, /Recording ended/);
  assertNoExtraIO(ui);
});

test("30/60 draw targets refresh independently while model duration and selected samples stay identical", async () => {
  const slow = harness(), fast = harness(); await Promise.all([slow.loaded, fast.loaded]);
  for (const ui of [slow, fast]) { ui.input("speed", 1); ui.click(); }
  const schedules = fast.requested; fast.input("draw-fps", 60);
  assert.equal(fast.requested, schedules, "Changing draw FPS reuses the existing RAF");
  const slowWrites = slow.element("playhead").textWrites, fastWrites = fast.element("playhead").textWrites;
  for (let frame = 1; frame <= 1380; frame++) {
    const now = frame * 1000 / 60; slow.frame(now); fast.frame(now);
    if (frame % 12 === 0) {
      assertTime(slow, now / 1000, now >= 23000 ? 23 : now >= 20000 ? 20 : now >= 10000 ? 10 : 0);
      assert.equal(slow.playhead, fast.playhead); assert.equal(slow.stateTime, fast.stateTime);
      assert.equal(slow.element("duration").textContent, "23 s"); assert.equal(fast.element("duration").textContent, "23 s");
    }
  }
  assert.ok(fast.element("playhead").textWrites - fastWrites > (slow.element("playhead").textWrites - slowWrites) * 1.9);
  for (const ui of [slow, fast]) { assertTime(ui, 23, 23); assert.equal(ui.pending, 0); assertNoExtraIO(ui); }
});

test("visibilitychange cancels RAF and freezes fractional time until explicit resume, with no hidden-tab backlog", async () => {
  const ui = harness(); await ui.loaded;
  ui.input("speed", 1); ui.at(1000); ui.click(); ui.frame(6250); assertTime(ui, 5.25, 0);
  ui.at(6375); ui.visibility(true); assertTime(ui, 5.375, 0); assert.equal(ui.pending, 0);
  assert.match(ui.element("playback-status").textContent, /Paused while tab was hidden/);
  const schedules = ui.requested; ui.click(); ui.frame(600000);
  assertTime(ui, 5.375, 0); assert.equal(ui.requested, schedules); assert.equal(ui.pending, 0);
  ui.visibility(false); assertTime(ui, 5.375, 0); assert.equal(ui.pending, 0);
  ui.click(); ui.frame(600625); assertTime(ui, 6, 0);
  assert.equal(ui.pending, 1); assertNoExtraIO(ui);
});

test("loader rejection and malformed clock times keep controls disabled and download unavailable", async () => {
  const failures = [
    { loadError: new Error("Dataset integrity verification failed") },
    { data: { ...fixture(), frames: fixture().frames.map((sample, index) => ({ ...sample, sim_time: index === 2 ? NaN : sample.sim_time })) } },
    { data: { ...fixture(), frames: fixture().frames.map((sample, index) => ({ ...sample, sim_time: index === 2 ? 10 : sample.sim_time })) } },
  ];
  for (const failure of failures) {
    const ui = harness({ ...failure, deferred: true });
    ui.element("download").href = "./data/stale.json";
    ui.release(); await ui.loaded;
    for (const id of controls) {
      assert.equal(ui.element(id).disabled, true, id);
      assert.equal(ui.element(id).listeners.size, 0, `${id} must not bind before clock validation`);
    }
    assert.equal(ui.element("download").href, undefined); assert.equal(ui.element("download").hidden, true);
    assert.equal(ui.element("error").hidden, false);
    assert.match(ui.element("error").textContent, /integrity|sample times/);
    assert.equal(ui.element("workspace").getAttribute("aria-busy"), "false");
    assert.equal(ui.pending, 0); assert.equal(ui.requested, 0); assertNoExtraIO(ui);
  }
});

// Малый fixture границы loader для actual observer body. Он проверяет UI,
// не объявляет синтетические frames записью Rust или браузерной приёмкой.
function denseUIFixture({ delayed = false } = {}) {
  const data = fixture(); data.identity.world_format_version = 30; data.identity.chamber_format = 1;
  data.identity.source_commit = "4".repeat(40); data.experiment.dt_seconds = 30;
  data.frames.forEach((sample) => { sample.sim_time *= 30; sample.cells[0].age_s *= 30; });
  validateRecording(data);
  const manifest = { ...data, identity: { ...data.identity, source_commit: "b".repeat(40) }, provenance: { producer_commit: "c".repeat(40) }, experiment: { ...data.experiment, sample_every: 1, frames: 24 }, genomes: { founder: { genome: { kinetics: 4 }, phenotype: { growth_per_s: .004 } }, newcomer: { genome: { kinetics: -2 }, phenotype: { growth_per_s: .002 } } } };
  delete manifest.frames;
  const sampleAt = (tick) => {
    const sample = structuredClone(data.frames[0]); sample.tick = tick; sample.sim_time = tick * 30;
    sample.cells[0].age_s = tick * 30; sample.cells[0].energy_j = tick + 1;
    if (tick >= 17) sample.cells.push({ ...sample.cells[0], id: "9007199254740993", genome_key: "newcomer", parent_id: "1", birth_tick: 17, generation: 1, age_s: (tick - 17) * 30 });
    if (tick === 23) sample.cells.shift();
    sample.summary = { ...sample.summary, living_cells: sample.cells.length, births: tick >= 17 ? 1 : 0, deaths: tick === 23 ? 1 : 0, generation_max: tick >= 17 ? 1 : 0 };
    sample.resources[0].concentration = tick + 1; sample.residual = tick ? { matter: "0", energy: "0" } : null;
    return sample;
  };
  const requests = [], dense = {
    manifest, closed: false,
    seek: (tick, { signal }) => {
      if (dense.closed) return Promise.reject(new Error("loader is closed"));
      if (!delayed || tick === 0) { requests.push({ tick, signal }); return Promise.resolve({ frame: sampleAt(tick), genomes: manifest.genomes }); }
      return new Promise((resolve, reject) => requests.push({ tick, signal, resolve: () => resolve({ frame: sampleAt(tick), genomes: manifest.genomes }), reject }));
    },
    subscribeFailure(callback) { this.subscriber = callback; return () => { if (this.subscriber === callback) this.subscriber = null; }; },
    close() { this.closed = true; this.subscriber = null; },
  };
  return { data, dense, requests };
}
const settleDenseUI = async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); };
function treeText(element) { return [element.textContent, ...element.children.map(treeText)].join(" ").replace(/\s+/g, " ").trim(); }

test("dense UI показывает manifest facts/full genomes, один tick Next и только sparse графики/download", async () => {
  const source = denseUIFixture(), ui = harness(source); await ui.loaded;
  assert.equal(ui.element("world").textContent, "30"); assert.equal(ui.element("frame-count").textContent, "1 / 24");
  assert.equal(ui.element("scrub").max, "690"); assert.match(ui.element("sample-cadence").textContent, /24 real states.*every tick \/ 30 model s.*4 sparse archive frames/);
  const facts = treeText(ui.element("experiment-facts")); assert.match(facts, /frames 24 sample interval 1 ticks/);
  assert.match(facts, new RegExp(`source commit ${"b".repeat(40)} dense producer ${"c".repeat(40)}`));
  assert.match(facts, /archive source 4444/);
  for (const id of ["population-chart", "genome-chart", "resource-chart"]) assert.match(ui.element(id).beforeNodes[0].textContent, /Sparse archive overview · 4 frames/);
  assert.equal(ui.element("download").href, "./data/fixture.json"); assert.match(ui.element("download").textContent, /sparse archive \(4 frames\)/);
  ui.click("next"); await settleDenseUI(); assertTime(ui, 30, 30); assert.equal(ui.element("tick").textContent, "1");
  assert.match(ui.element("frame-change").textContent, /Cumulative counters/); assert.doesNotMatch(ui.element("frame-change").textContent, /since prior/);
  ui.element("chamber").dispatch("keydown", { key: "ArrowRight" });
  assert.match(treeText(ui.element("cell-detail")), /kinetics 4/);
  ui.input("scrub", 510, "input"); await settleDenseUI();
  assert.equal(ui.element("tick").textContent, "17"); assert.equal(ui.element("living").textContent, "2"); assert.equal(ui.element("births").textContent, "1");
  ui.element("chamber").dispatch("keydown", { key: "ArrowRight" });
  assert.match(treeText(ui.element("cell-detail")), /9007199254740993/); assert.match(treeText(ui.element("cell-detail")), /kinetics -2/);
  assert.equal(ui.layoutCount, 2);
  ui.input("scrub", 480, "input"); await settleDenseUI();
  assert.equal(ui.element("tick").textContent, "16"); assert.equal(ui.element("living").textContent, "1"); assert.equal(ui.layoutCount, 1);
  assert.equal(ui.element("selection-label").textContent, "not present"); assert.match(ui.element("cell-detail").textContent, /9007199254740993.*shown tick 16/);
  assertNoExtraIO(ui);
});

test("dense buffering держит весь показанный state; superseded seek не меняет canvas/inspector/tick", async () => {
  const source = denseUIFixture({ delayed: true }), ui = harness(source); await ui.loaded;
  ui.element("chamber").dispatch("keydown", { key: "ArrowRight" });
  const originalInspector = treeText(ui.element("cell-detail")), draws = ui.element("chamber").canvasCalls.length;
  ui.input("scrub", 510, "input");
  assert.equal(ui.element("workspace").getAttribute("aria-busy"), "true"); assert.match(ui.element("playback-status").textContent, /Buffering recorded tick 17 · shown tick 0/);
  assertTime(ui, 0, 0); assert.equal(ui.element("living").textContent, "1"); assert.equal(treeText(ui.element("cell-detail")), originalInspector); assert.equal(ui.element("chamber").canvasCalls.length, draws);
  ui.input("scrub", 300, "input"); assert.equal(source.requests[1].signal.aborted, true);
  source.requests[2].resolve(); await settleDenseUI(); assertTime(ui, 300, 300);
  const shownInspector = treeText(ui.element("cell-detail")); source.requests[1].resolve(); await settleDenseUI();
  assertTime(ui, 300, 300); assert.equal(treeText(ui.element("cell-detail")), shownInspector);
  assert.equal(ui.element("workspace").getAttribute("aria-busy"), "false"); assert.equal(ui.pending, 0);
  ui.input("scrub", 510, "input"); assert.equal(ui.element("play").getAttribute("aria-label"), "Pause recording");
  ui.click(); assert.equal(source.requests[3].signal.aborted, true); source.requests[3].resolve(); await settleDenseUI();
  assertTime(ui, 300, 300); assert.equal(ui.element("playback-status").textContent, "Paused"); assert.equal(ui.pending, 0);
});

test("dense hidden-tab Pause отменяет fetch и поздний ответ не запускает playback", async () => {
  const source = denseUIFixture({ delayed: true }), ui = harness(source); await ui.loaded;
  ui.input("speed", 30); ui.click(); ui.frame(1100);
  assert.equal(source.requests[1].tick, 1); assert.match(ui.element("playback-status").textContent, /Buffering/); assert.equal(ui.pending, 0);
  ui.at(2000); ui.visibility(true); assert.equal(source.requests[1].signal.aborted, true);
  ui.frame(600_000); source.requests[1].resolve(); await settleDenseUI();
  assertTime(ui, 0, 0); assert.equal(ui.pending, 0); assert.match(ui.element("playback-status").textContent, /Paused while tab was hidden/);
  ui.visibility(false); assert.equal(ui.pending, 0); ui.click(); ui.frame(600_100);
  assertTime(ui, 3, 0); assert.equal(source.requests.length, 2); assert.equal(ui.pending, 1);
  ui.pagehide(); assert.equal(source.dense.closed, true); assert.equal(ui.pending, 0);
});

test("dense error сохраняет кадр и inspector, отключает controls без архивного fallback", async () => {
  const source = denseUIFixture({ delayed: true }), ui = harness(source); await ui.loaded;
  ui.element("chamber").dispatch("keydown", { key: "ArrowRight" }); const inspector = treeText(ui.element("cell-detail"));
  ui.click("next"); source.requests[1].reject(new Error("Chunk digest mismatch")); await settleDenseUI();
  assertTime(ui, 0, 0); assert.equal(ui.element("tick").textContent, "0"); assert.equal(treeText(ui.element("cell-detail")), inspector);
  for (const id of controls) assert.equal(ui.element(id).disabled, true, id);
  assert.equal(ui.element("error").hidden, false); assert.match(ui.element("error").textContent, /digest mismatch.*last validated state.*recording=archive/i);
  assert.equal(source.dense.closed, true); assert.equal(ui.pending, 0); assert.equal(ui.loadCalls, 1);
  assert.equal(ui.element("download").href, "./data/fixture.json"); assert.match(ui.element("download").textContent, /sparse archive/);
});

test("dense end и Replay используют подтверждённые endpoints и не создают RAF дубликаты", async () => {
  const source = denseUIFixture(), ui = harness(source); await ui.loaded;
  ui.input("scrub", 690, "input"); await settleDenseUI();
  assertTime(ui, 690, 690); assert.equal(ui.element("next").disabled, true); assert.equal(ui.element("play").getAttribute("aria-label"), "Replay recording");
  ui.click(); await settleDenseUI(); assertTime(ui, 0, 0); assert.equal(ui.pending, 1);
  assert.equal(ui.element("play").getAttribute("aria-label"), "Pause recording"); ui.click(); assert.equal(ui.pending, 0);
});

test("dense newborn highlight относится только к birth_tick === shown tick, включая skipped/backward seek", async () => {
  const source = denseUIFixture(), ui = harness(source); await ui.loaded;
  const commands = ui.element("chamber").canvasCalls;
  for (const [tick, expected] of [[20, false], [17, true], [16, false], [18, false]]) {
    const start = commands.length; ui.input("scrub", tick * 30, "input"); await settleDenseUI();
    assert.equal(commands.slice(start).some(([operation, key, value]) => operation === "set" && key === "strokeStyle" && value === "#efbc77"), expected, `shown tick ${tick}`);
    assert.match(ui.element("frame-change").textContent, /Cumulative counters/);
  }
});

// Синтетический lifecycle event: не проверка native BFCache в браузере.
test("persisted pagehide закрывает reader; persisted pageshow reload не даёт Play использовать старый reader", async () => {
  const source = denseUIFixture(), ui = harness(source); await ui.loaded;
  ui.click(); assert.equal(ui.pending, 1); ui.pagehide(true);
  assert.equal(source.dense.closed, true); assert.equal(source.dense.subscriber, null); assert.equal(ui.pending, 0);
  await assert.rejects(source.dense.seek(1, { signal: new AbortController().signal }), /closed/);
  for (const id of controls) assert.equal(ui.element(id).disabled, true, id);
  ui.pageshow(true); assert.equal(ui.reloads, 1); assert.equal(ui.pending, 0);
  for (const id of controls) assert.equal(ui.element(id).disabled, true, id);
  const seeks = source.requests.length; ui.element("play").dispatch("click"); ui.frame(600_000);
  assert.equal(source.requests.length, seeks); assert.equal(ui.pending, 0);
  ui.pageshow(false); assert.equal(ui.reloads, 1);
});

// Два chunks из настоящих Rust frames 0..100. Первый содержит неизменённые
// строки исходного fixture; keyframe50 берётся из совпадающего старого архива.
// Повреждается только транспорт второго chunk, не биология или metadata.
async function realCorruptPrefetch() {
  const directory = new URL("../scripts/fixtures/dense-recording/smoke-100/", import.meta.url);
  const manifest = JSON.parse(await readFile(new URL("horizon-100.json", directory), "utf8"));
  const index = JSON.parse(await readFile(new URL("index.json", directory), "utf8"));
  const lines = gunzipSync(await readFile(new URL("chunk-0000000-0000100.jsonl.gz", directory))).toString().trimEnd().split("\n");
  const data = JSON.parse(await readFile(new URL("./data/cell-chamber-seed-42.json", import.meta.url), "utf8"));
  data.frames = data.frames.slice(0, 3); data.experiment = { ...data.experiment, steps: 100, checked_ticks: 100 }; validateRecording(data);
  const { cells, ...rest } = data.frames[1], genomes = {};
  for (const line of lines.slice(0, 51)) Object.assign(genomes, JSON.parse(line).genomes);
  const fixed = ["id", "parent_id", "generation", "birth_tick", "genome_key", "division_mass_mol"], varying = ["age_s", "mass_mol", "energy_j", "mass_units", "energy_units", "starvation_s"];
  const keyframe = { type: "keyframe", schema_version: 1, frame: rest, definitions: cells.map((cell) => fixed.map((key) => cell[key])), values: cells.map((cell) => varying.map((key) => cell[key])), genomes };
  const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
  const chunks = [[0, 49, lines.slice(0, 50)], [50, 100, [JSON.stringify(keyframe), ...lines.slice(51)]]].map(([first, last, records]) => {
    const raw = Buffer.from(records.join("\n") + "\n"), bytes = gzipSync(raw, { level: 9 }); bytes[8] = 2; bytes[9] = 255;
    return { bytes, descriptor: { path: `chunk-${String(first).padStart(7, "0")}-${String(last).padStart(7, "0")}.jsonl.gz`, first_tick: first, last_tick: last, frames: last - first + 1, gzip_bytes: bytes.length, decoded_bytes: raw.length, gzip_sha256: digest(bytes), decoded_sha256: digest(raw) } };
  });
  manifest.chunks = chunks.map((chunk) => chunk.descriptor);
  const validSecond = await decodeDenseChunk(chunks[1].bytes, chunks[1].descriptor, manifest.genomes, { cryptoProvider: webcrypto });
  assert.deepEqual(validSecond.frameAt(50).frame, data.frames[1], "second fixture chunk starts at the real retained Rust frame50");
  assert.deepEqual(validSecond.frameAt(100).frame, data.frames[2], "second fixture chunk ends at the real retained Rust frame100");
  const manifestBytes = Buffer.from(JSON.stringify(manifest)); index.manifests[0].bytes = manifestBytes.length; index.manifests[0].sha256 = digest(manifestBytes);
  index.unique_chunks = 2; index.unique_gzip_bytes = chunks.reduce((sum, chunk) => sum + chunk.bytes.length, 0); index.decoded_chunk_bytes = chunks.reduce((sum, chunk) => sum + chunk.descriptor.decoded_bytes, 0);
  const indexBytes = Buffer.from(JSON.stringify(index)), requests = []; let release, started = false;
  const held = new Promise((resolve) => { release = resolve; });
  const dense = await createDenseRecordingLoader({ indexUrl: "https://example.test/dense/index.json", indexSha256: digest(indexBytes), manifestPath: "horizon-100.json", baseUrl: "https://example.test/observe.html", cryptoProvider: webcrypto,
    fetcher: async (url) => {
      const path = new URL(url).pathname.split("/").at(-1); requests.push(path);
      if (path === "index.json") return new Response(indexBytes);
      if (path === "horizon-100.json") return new Response(manifestBytes);
      if (path === chunks[0].descriptor.path) return new Response(chunks[0].bytes);
      assert.equal(path, chunks[1].descriptor.path); started = true; await held;
      return new Response(Buffer.from(chunks[1].bytes).fill(0));
    },
  });
  return { data, dense, requests, release, started: () => started };
}
async function untilUI(predicate) {
  for (let attempt = 0; attempt < 100; attempt++) { if (predicate()) return; await new Promise((resolve) => setTimeout(resolve, 5)); }
  assert.fail("bounded async UI fixture did not settle");
}

test("real corrupt prefetch при Paused сразу показывает fatal UI и сохраняет подтверждённый frame без Next", async () => {
  const source = await realCorruptPrefetch(), ui = harness(source);
  try {
    await ui.loaded; await untilUI(source.started); assertTime(ui, 0, 0); assert.equal(ui.pending, 0);
    assert.equal(ui.element("play").disabled, false); assert.match(ui.element("playback-status").textContent, /Paused/);
    ui.element("chamber").dispatch("keydown", { key: "ArrowRight" });
    const inspector = treeText(ui.element("cell-detail")), canvas = ui.element("chamber").canvasCalls.length;
    const living = ui.element("living").textContent; source.release(); await untilUI(() => !ui.element("error").hidden);
    assert.match(ui.element("error").textContent, /gzip integrity.*last validated state/i);
    assert.match(ui.element("playback-status").textContent, /Recording error/);
    for (const id of controls) assert.equal(ui.element(id).disabled, true, id);
    assertTime(ui, 0, 0); assert.equal(ui.element("living").textContent, living); assert.equal(treeText(ui.element("cell-detail")), inspector);
    assert.equal(ui.element("chamber").canvasCalls.length, canvas); assert.equal(source.dense.stats.failed, true); assert.equal(source.dense.stats.closed, true);
    assert.equal(source.requests.filter((path) => path.endsWith(".gz")).length, 2); assert.equal(ui.pending, 0); assert.equal(ui.loadCalls, 1);
  } finally { source.dense.close(); }
});

test("real prefetch failure между initial seek commit и await continuation не включает ready заново", async () => {
  const source = await realCorruptPrefetch();
  // Задержка только continuation initial seek: реальный frame уже committed,
  // реальный corrupt prefetch должен успеть уведомить до final ready gate.
  class DelayedInitialContinuation extends DensePlaybackSession {
    async seek(time) {
      const result = await super.seek(time);
      if (time === 0) { source.release(); await untilUI(() => source.dense.stats.failed); }
      return result;
    }
  }
  const ui = harness({ ...source, sessionClass: DelayedInitialContinuation });
  try {
    await ui.loaded; assertTime(ui, 0, 0); assert.equal(ui.element("error").hidden, false);
    assert.match(ui.element("error").textContent, /gzip integrity/);
    for (const id of controls) assert.equal(ui.element(id).disabled, true, id);
    assert.equal(ui.element("download").hidden, true); assert.equal(ui.pending, 0); assert.equal(source.dense.stats.closed, true);
    assert.equal(ui.element("play").listeners.size, 0, "fatal initial load must not bind ready controls");
  } finally { source.dense.close(); }
});
