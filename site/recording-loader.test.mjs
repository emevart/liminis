import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { webcrypto } from "node:crypto";
import test from "node:test";
import { loadRecording } from "./recording-loader.mjs";

const catalog = JSON.parse(await readFile(new URL("./data/catalog.json", import.meta.url), "utf8"));
const bytes = await readFile(new URL(catalog.entries[0].recording, import.meta.url));
const mockFetch = (dataset = bytes, status = 200) => async (url) => url === "./data/catalog.json"
  ? Response.json(catalog)
  : new Response(dataset, { status });

test("loads and verifies the default recorded dataset", async () => {
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
