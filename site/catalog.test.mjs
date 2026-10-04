import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { buildCatalog, catalogEntry, verifySharedSamples } from "../scripts/build_experiment_catalog.mjs";
import { validateCatalog, validateDenseAttachment, selectExperiment } from "./catalog.mjs";

const catalog = await buildCatalog();

test("all three real horizons validate and committed manifest is fresh", async () => {
  assert.deepEqual(catalog.entries.map((entry) => entry.experiment.steps), [10_000, 100_000, 1_000_000]);
  assert.deepEqual(JSON.parse(await readFile(new URL("./data/catalog.json", import.meta.url))), catalog);
});

test("selection is limited to the validated catalog", () => {
  assert.equal(selectExperiment(catalog).id, "cell-chamber-10k");
  assert.equal(selectExperiment(catalog, "?experiment=cell-chamber-1m").experiment.steps, 1_000_000);
  assert.throws(() => selectExperiment(catalog, "?experiment=https://other.invalid/data.json"));
});

test("dense pins are typed, same-origin, closed and tied to their horizon", () => {
  const valid = { index: "./data/dense-cell-chamber/index.json", index_sha256: "a".repeat(64), manifest: "horizon-10000.json", publication: "./data/dense-cell-chamber/publication.json", publication_sha256: "b".repeat(64) };
  assert.equal(validateDenseAttachment(valid, 10000), valid);
  for (const change of [
    { index: "https://elsewhere.invalid/index.json" }, { manifest: "horizon-100000.json" },
    { publication: "./data/dense-cell-chamber/../publication.json" }, { index_sha256: [valid.index_sha256] },
    { publication_sha256: "invented" }, { untrusted: true },
  ]) assert.throws(() => validateDenseAttachment({ ...valid, ...change }, 10000));
  const attached = structuredClone(catalog); attached.entries[0].dense = valid;
  assert.equal(validateCatalog(attached), attached);
  attached.entries[0].dense = null; assert.throws(() => validateCatalog(attached));
});

test("rejects unsafe recording paths and duplicate IDs", () => {
  for (const recording of ["https://other.invalid/file.json", "./data/../secret.json", "//other.invalid/data.json"]) {
    const value = structuredClone(catalog); value.entries[0].recording = recording;
    assert.throws(() => validateCatalog(value));
  }
  const value = structuredClone(catalog); value.entries[1].id = value.entries[0].id;
  assert.throws(() => validateCatalog(value));
});

test("rejects stale summaries, malformed bounds and preview inventories", () => {
  for (const mutate of [
    (value) => value.entries[0].experiment.steps = 1e308,
    (value) => value.entries[0].preview.living_cells++,
    (value) => value.entries[0].final_summary.births = -1,
    (value) => value.entries[0].sha256 = "invented",
    (value) => value.entries[0].id = 42,
    (value) => value.entries[0].identity.seed = 42,
  ]) {
    const value = structuredClone(catalog); mutate(value); assert.throws(() => validateCatalog(value));
  }
});

test("previews and digest derive from actual validated bytes", async () => {
  const bytes = await readFile(new URL("./data/cell-chamber-seed-42-100k.json", import.meta.url));
  const recording = JSON.parse(bytes), entry = catalogEntry("cell-chamber-100k", "100,000 ticks", "cell-chamber-seed-42-100k.json", bytes);
  const frame = recording.frames.find((sample) => sample.tick === entry.preview.tick);
  assert.equal(entry.bytes, bytes.length);
  assert.deepEqual(entry.preview.cells.map((cell) => cell.id), frame.cells.map((cell) => cell.id));
  assert.deepEqual(entry.final_summary, recording.frames.at(-1).summary);
  const truncated = structuredClone(recording); truncated.frames.pop();
  assert.throws(() => catalogEntry("bad", "bad", "bad.json", Buffer.from(JSON.stringify(truncated))));
});

test("rejects invented shared trajectory claims", () => {
  const a = { canonical_config: "same", identity: { seed: "42", world_format_version: 30 }, frames: [{ tick: 0, cells: [] }] };
  const b = structuredClone(a); b.frames[0].cells.push({ id: "invented" });
  assert.throws(() => verifySharedSamples([a, b]));
});
