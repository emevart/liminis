import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createHash, webcrypto } from "node:crypto";
import test from "node:test";
import { loadRecording } from "./recording-loader.mjs";

const catalog = JSON.parse(await readFile(new URL("./data/catalog.json", import.meta.url), "utf8"));
// Проверяем legacy fallback явно на архивном catalog, независимо от того,
// прикреплена ли уже complete dense publication к реальному catalog сайта.
const archiveCatalog = { ...catalog, entries: catalog.entries.map(({ dense, ...entry }) => entry) };
const bytes = await readFile(new URL(catalog.entries[0].recording, import.meta.url));
const mockFetch = (dataset = bytes, status = 200) => async (url) => url === "./data/catalog.json"
  ? Response.json(archiveCatalog)
  : new Response(dataset, { status });

test("loads and verifies the default archive when no dense attachment exists", async () => {
  const { entry, data } = await loadRecording("", mockFetch(), webcrypto);
  assert.equal(entry.id, catalog.default_experiment);
  assert.equal(data.experiment.steps, entry.experiment.steps);
});
test("unknown experiment never fetches a dataset", async () => {
  const calls = [];
  await assert.rejects(loadRecording("?experiment=../private", async (url) => {
    calls.push(url); return Response.json(catalog);
  }, webcrypto), /not in the catalog/);
  assert.deepEqual(calls, ["./data/catalog.json"]);
});
test("reports a failed catalog request", async () => {
  await assert.rejects(loadRecording("", async () => new Response("Unavailable", { status: 503 }), webcrypto), /Catalog request failed \(503\)/);
});
test("reports a failed recording request", async () => {
  await assert.rejects(loadRecording("", mockFetch("Not found", 404), webcrypto), /Dataset request failed \(404\)/);
});
test("rejects a truncated download before activation", async () => {
  await assert.rejects(loadRecording("", mockFetch(bytes.subarray(0, bytes.length - 1)), webcrypto), /incomplete or out of date/);
});
test("rejects a same-size stale or corrupted download", async () => {
  const corrupted = Buffer.from(bytes); corrupted[10] ^= 1;
  await assert.rejects(loadRecording("", mockFetch(corrupted), webcrypto), /does not match the catalog/);
});

// Настоящие первые 100 ticks dense fixture и соответствующие старые sparse
// frames 0/50/100. Смена metadata горизонта здесь не запускает модель.
async function denseFixture() {
  const data = JSON.parse(bytes.toString()); data.frames = data.frames.slice(0, 3);
  data.experiment = { ...data.experiment, steps: 100, checked_ticks: 100 };
  assert.deepEqual(data.frames.map((frame) => frame.tick), [0, 50, 100]);
  const archive = Buffer.from(JSON.stringify(data));
  const index = await readFile(new URL("../scripts/fixtures/dense-recording/smoke-100/index.json", import.meta.url));
  const entry = structuredClone(catalog.entries[0]);
  entry.bytes = archive.length; entry.sha256 = createHash("sha256").update(archive).digest("hex");
  entry.experiment = { steps: 100, checked_ticks: 100, sample_every: 50, dt_seconds: 30 }; entry.sample_count = 3;
  entry.preview = { tick: 100, sim_time: 3000, living_cells: data.frames[2].cells.length, cells: data.frames[2].cells };
  entry.final_summary = data.frames[2].summary;
  entry.dense = { index: "./data/dense-cell-chamber/index.json", index_sha256: createHash("sha256").update(index).digest("hex"), manifest: "horizon-100.json", publication: "./data/dense-cell-chamber/publication.json", publication_sha256: "a".repeat(64) };
  const admittedCatalog = { ...catalog, entries: [entry], default_experiment: entry.id, featured_experiment: entry.id }, calls = [];
  const fetcher = async (url) => {
    calls.push(String(url));
    if (url === "./data/catalog.json") return Response.json(admittedCatalog);
    if (url === entry.recording) return new Response(archive);
    const filename = new URL(url).pathname.split("/").at(-1);
    assert.ok(["index.json", "horizon-100.json", "chunk-0000000-0000100.jsonl.gz"].includes(filename), `Unexpected request: ${url}`);
    return new Response(await readFile(new URL(`../scripts/fixtures/dense-recording/smoke-100/${filename}`, import.meta.url)));
  };
  return { entry, admittedCatalog, calls, fetcher, base: "https://example.test/liminis/observe.html" };
}

test("выбирает pinned dense по умолчанию, сохраняя sparse archive только для обзора", async () => {
  const fixture = await denseFixture();
  const loaded = await loadRecording("", fixture.fetcher, webcrypto, fixture.base);
  try {
    assert.equal(loaded.mode, "dense"); assert.equal(loaded.data.frames.length, 3);
    assert.equal(loaded.dense.manifest.experiment.frames, 101); assert.equal(loaded.dense.manifest.identity.world_format_version, 30);
    assert.notEqual(loaded.dense.manifest.identity.source_commit, loaded.entry.identity.source_commit);
    const { frame } = await loaded.dense.seek(1);
    assert.equal(frame.tick, 1); assert.equal(frame.sim_time, 30);
    assert.ok(fixture.calls.includes("https://example.test/liminis/data/dense-cell-chamber/index.json"));
    assert.equal(fixture.calls.some((url) => url.endsWith("publication.json")), false);
  } finally { loaded.dense.close(); }
});

test("явный recording=archive не загружает dense index/chunks", async () => {
  const fixture = await denseFixture();
  const loaded = await loadRecording("?recording=archive", fixture.fetcher, webcrypto, fixture.base);
  assert.equal(loaded.mode, "archive"); assert.equal(loaded.dense, undefined);
  assert.deepEqual(fixture.calls, ["./data/catalog.json", fixture.entry.recording]);
});

test("не подменяет неверный dense pin или identity архивным playback", async () => {
  const corrupted = await denseFixture(); corrupted.entry.dense.index_sha256 = "0".repeat(64);
  await assert.rejects(loadRecording("", corrupted.fetcher, webcrypto, corrupted.base), /SHA-256|digest|hash/i);
  assert.equal(corrupted.calls.some((url) => url.endsWith("horizon-100.json")), false);
  const mismatched = await denseFixture(); mismatched.entry.identity.seed = "43";
  await assert.rejects(loadRecording("", mismatched.fetcher, webcrypto, mismatched.base), /identity differs \(seed\)/);
});

test("неизвестный mode и unavailable dense дают явную ошибку", async () => {
  let calls = 0;
  await assert.rejects(loadRecording("?recording=other", async () => { calls++; }, webcrypto), /Unknown recording mode/);
  assert.equal(calls, 0);
  await assert.rejects(loadRecording("?recording=dense", mockFetch(), webcrypto), /no dense recording/);
});
