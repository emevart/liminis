import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { Script, createContext } from "node:vm";
import { PlaybackClock, defaultPlaybackRate, validPlaybackRate } from "./playback.mjs";
import { validateRecording } from "./recording.mjs";

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
  replaceChildren(...children) { this.children = [...children]; }
  getBoundingClientRect() { return { left: 0, top: 0, width: 640, height: 360 }; }
  getContext(type) { assert.equal(type, "2d"); return this.context; }
}

function harness({ data = fixture(), loadError, deferred = false, hidden = false } = {}) {
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
  let now = 0, nextRaf = 1, requested = 0, loadCalls = 0, fetchCalls = 0, release;
  const barrier = deferred ? new Promise((resolve) => { release = resolve; }) : Promise.resolve();
  const context = createContext({
    document, location: { search: "?experiment=fixture" }, devicePixelRatio: 1,
    performance: { now: () => now }, PlaybackClock, defaultPlaybackRate, validPlaybackRate,
    loadRecording: async (search) => {
      assert.equal(search, "?experiment=fixture"); loadCalls++;
      await barrier;
      if (loadError) throw loadError;
      return { entry: { title: "Fixture", recording: "./data/fixture.json" }, data };
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
    get pending() { return rafs.size; }, get requested() { return requested; },
    get loadCalls() { return loadCalls; }, get fetchCalls() { return fetchCalls; },
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
