import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { validateRecording } from "./recording.mjs";

const source = JSON.parse(await readFile(new URL("./data/cell-chamber-seed-42.json", import.meta.url), "utf8"));
const changed = (mutate) => { const copy = structuredClone(source); mutate(copy); return copy; };

test("accepts the complete recorded experiment", () => assert.doesNotThrow(() => validateRecording(source)));
test("rejects a truncated final frame", () => assert.throws(() => validateRecording(changed((data) => data.frames.pop())), /incomplete/));
test("rejects a broken sample cadence", () => assert.throws(() => validateRecording(changed((data) => { data.frames[4].tick += 1; data.frames[4].sim_time += data.experiment.dt_seconds; })), /frame 5/i));
test("rejects a recording without tick-zero genesis", () => assert.throws(() => validateRecording(changed((data) => { data.frames[0].tick = 1; data.frames[0].sim_time = data.experiment.dt_seconds; })), /incomplete/));
test("rejects a living-cell inventory mismatch", () => assert.throws(() => validateRecording(changed((data) => { data.frames[8].summary.living_cells += 1; })), /summary/));
test("rejects malformed experiment metadata", () => assert.throws(() => validateRecording(changed((data) => { data.canonical_config = null; })), /metadata/));
test("rejects experiment bounds that cannot be safely iterated", () => assert.throws(() => validateRecording(changed((data) => { data.experiment.steps = 1e308; data.experiment.checked_ticks = 1e308; })), /bounds/));
test("rejects unparseable exact accounting integers", () => assert.throws(() => validateRecording(changed((data) => { data.frames[7].accounting.free_matter_units[0] = "garbage"; })), /accounting/));
