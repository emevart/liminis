import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createHash, webcrypto } from "node:crypto";
import { spawnSync } from "node:child_process";
import { gunzipSync, gzipSync, constants } from "node:zlib";
import { admitDenseRecording, compareDenseCellIds, createDenseRecordingLoader, decodeDenseChunk, DENSE_LIMITS, parseDenseJson } from "./dense-recording.mjs";

const fixture = new URL("../scripts/fixtures/dense-recording/smoke-100/", import.meta.url);
const indexBytes = new Uint8Array(await readFile(new URL("index.json", fixture))), manifestBytes = new Uint8Array(await readFile(new URL("horizon-100.json", fixture)));
const compressed = new Uint8Array(await readFile(new URL("chunk-0000000-0000100.jsonl.gz", fixture)));
const INDEX_SHA = "d3d102bcb4233900416ba0532c9cdd83b41b353963b15c47a9c66e4852b260aa";
const digest = (value) => createHash("sha256").update(value).digest("hex"), clone = structuredClone;
const sourceIndex = JSON.parse(new TextDecoder().decode(indexBytes)), sourceManifest = JSON.parse(new TextDecoder().decode(manifestBytes));
const raw = gunzipSync(compressed), realRecords = raw.toString("utf8").trimEnd().split("\n").map(JSON.parse);
const legacy = JSON.parse(await readFile(new URL("./data/cell-chamber-seed-42.json", import.meta.url), "utf8"));
const STATIC = ["id", "parent_id", "generation", "birth_tick", "genome_key", "division_mass_mol"], DYNAMIC = ["age_s", "mass_mol", "energy_j", "mass_units", "energy_units", "starvation_s"];

// Test-only independent array fold, using Node zlib/JSON.parse rather than the
// production parser/inflater. The real bounded 101-frame fixture fits in tests.
function reference(records) {
  const output = []; let current, genomes;
  for (const record of records) {
    if (record.type === "keyframe") {
      current = { ...clone(record.frame), cells: record.definitions.map((row, i) => Object.fromEntries([...STATIC.map((key, j) => [key, row[j]]), ...DYNAMIC.map((key, j) => [key, record.values[i][j]])])) }; genomes = clone(record.genomes);
    } else {
      current = clone(current); current.cells = current.cells.filter((cell) => !record.removed.includes(cell.id));
      for (const [ident, mask, values] of record.changed) { const cell = current.cells.find((entry) => entry.id === ident); let offset = 0; DYNAMIC.forEach((key, bit) => { if ((mask >> bit) & 1) cell[key] = values[offset++]; }); }
      for (const [row, values] of record.born) current.cells.push(Object.fromEntries([...STATIC.map((key, j) => [key, row[j]]), ...DYNAMIC.map((key, j) => [key, values[j]])]));
      if (record.order) current.cells = record.order.map((ident) => current.cells.find((cell) => cell.id === ident));
      Object.assign(current, clone(record.set)); Object.assign(genomes, clone(record.genomes));
    }
    output.push({ frame: clone(current), genomes: clone(genomes) });
  }
  return output;
}
const expected = reference(realRecords);
// Test fixture encoder keeps -0; JSON.stringify alone would erase its sign.
function encode(value) {
  if (typeof value === "number") return Object.is(value, -0) ? "-0.0" : JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(encode).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.entries(value).map(([key, item]) => `${JSON.stringify(key)}:${encode(item)}`).join(",")}}`;
  return JSON.stringify(value);
}
function member(records, options = {}) {
  const payload = Buffer.from(records.map((record) => encode(record) + "\n").join("")), data = gzipSync(payload, { level: 9, ...options }); data[8] = 2; data[9] = 255;
  const first = records[0].frame.tick, last = first + records.length - 1;
  const descriptor = { path: `chunk-${String(first).padStart(7, "0")}-${String(last).padStart(7, "0")}.jsonl.gz`, first_tick: first, last_tick: last, frames: records.length, gzip_bytes: data.length, decoded_bytes: payload.length, gzip_sha256: digest(data), decoded_sha256: digest(payload) };
  return { data, descriptor, payload };
}
function keyframe(observation) {
  const { cells, ...rest } = clone(observation.frame);
  return { type: "keyframe", schema_version: 1, frame: rest, definitions: cells.map((cell) => STATIC.map((name) => cell[name])), values: cells.map((cell) => DYNAMIC.map((name) => cell[name])), genomes: clone(observation.genomes) };
}
async function decode(value) { return decodeDenseChunk(value.data, value.descriptor, sourceManifest.genomes, { cryptoProvider: webcrypto }); }
function metadata(mutator) {
  const manifest = clone(sourceManifest), index = clone(sourceIndex); mutator?.(manifest, index);
  const mb = Buffer.from(encode(manifest) + "\n"); index.manifests[0].bytes = mb.length; index.manifests[0].sha256 = digest(mb);
  const ib = Buffer.from(encode(index) + "\n"); return { indexBytes: ib, manifestBytes: mb, indexSha256: digest(ib), manifest, index };
}
async function admit(value) { return admitDenseRecording(value.indexBytes, value.manifestBytes, { indexSha256: value.indexSha256, manifestPath: "horizon-100.json", cryptoProvider: webcrypto }); }
function response(data, headers = {}) { return new Response(data, { status: 200, headers }); }
const origin = "https://example.test/observer.html", directory = "https://example.test/dense/";
function loaderOptions(files, fetcher) { return { indexUrl: directory + "index.json", indexSha256: digest(files.get("index.json")), manifestPath: "horizon-100.json", baseUrl: origin, cryptoProvider: webcrypto, fetcher: fetcher || (async (url) => response(files.get(new URL(url).pathname.slice(new URL(directory).pathname.length)))) }; }
function originalFiles() { return new Map([["index.json", indexBytes], ["horizon-100.json", manifestBytes], [sourceManifest.chunks[0].path, compressed]]); }
function threeChunks() {
  const chunks = [[0, 49], [50, 99], [100, 100]].map(([first, last]) => member([keyframe(expected[first]), ...realRecords.slice(first + 1, last + 1)]));
  const meta = metadata((manifest, index) => { manifest.chunks = chunks.map((chunk) => chunk.descriptor); index.unique_chunks = chunks.length; index.unique_gzip_bytes = chunks.reduce((sum, chunk) => sum + chunk.data.length, 0); index.decoded_chunk_bytes = chunks.reduce((sum, chunk) => sum + chunk.payload.length, 0); });
  const files = new Map([["index.json", meta.indexBytes], ["horizon-100.json", meta.manifestBytes], ...chunks.map((chunk) => [chunk.descriptor.path, chunk.data])]);
  return { files, chunks, meta };
}
async function until(predicate) { for (let i = 0; i < 100; i++) { if (predicate()) return; await new Promise((done) => setTimeout(done, 5)); } assert.fail("bounded async fixture did not settle"); }

test("real pilot has original pinned bytes and admits archived world 30 without relabeling", async () => {
  assert.equal(indexBytes.length + manifestBytes.length + compressed.length, 61_355); assert.equal(digest(indexBytes), INDEX_SHA);
  const admitted = await admitDenseRecording(indexBytes, manifestBytes, { indexSha256: INDEX_SHA, manifestPath: "horizon-100.json", cryptoProvider: webcrypto });
  assert.equal(admitted.manifest.identity.world_format_version, 30); assert.equal(admitted.manifest.provenance.producer_commit, "bd2a7f25e61abd713109171fc25b7c3de86003ab"); assert.equal(admitted.manifest.provenance.build_attestation, false);
  assert.ok(Object.isFrozen(admitted.manifest.chunks[0])); assert.equal(admitted.manifest.experiment.frames, 101);
});

test("all 101 real frames match independent retained-value fold; ticks 0/50/100 match old Rust recording", async () => {
  const chunk = await decodeDenseChunk(compressed, sourceManifest.chunks[0], sourceManifest.genomes, { cryptoProvider: webcrypto });
  assert.equal(chunk.retainedFrames, 0); assert.equal(chunk.records, 101);
  for (let tick = 0; tick <= 100; tick++) assert.deepStrictEqual(chunk.frameAt(tick), expected[tick], `real tick ${tick}`);
  for (const tick of [0, 50, 100]) {
    assert.deepStrictEqual(chunk.frameAt(tick).frame, legacy.frames.find((frame) => frame.tick === tick));
    for (const [key, definition] of Object.entries(chunk.frameAt(tick).genomes)) assert.deepStrictEqual(definition, legacy.genomes[key]);
  }
  assert.throws(() => chunk.frameAt(101), /integer|seek/); assert.throws(() => chunk.frameAt(-1), /integer/);
});

test("strict JSON rejects duplicate escaped keys, invalid Unicode/grammar, unsafe integer tokens and nonfinite floats", () => {
  for (const text of ['{"a":1,"a":2}', '{"a":1,"\\u0061":2}', '{"a":NaN}', '{"a":1e999}', '{"a":9007199254740993}', '{"a":01}', '{"a":1,}', '[1,]', '{}x', '"unclosed', '"\\x20"', '{"a":true false}']) assert.throws(() => parseDenseJson(text), undefined, text);
  assert.throws(() => parseDenseJson(new Uint8Array([0xff]))); assert.throws(() => parseDenseJson("[".repeat(66) + "0" + "]".repeat(66)), /nesting/);
  const value = parseDenseJson('{"__proto__":{"safe":true},"n":-0.0,"large":1.7976931348623157e308,"small":5e-324,"i":9007199254740991}');
  assert.ok(Object.is(value.n, -0)); assert.equal(value.small, Number.MIN_VALUE); assert.equal(value.large, Number.MAX_VALUE); assert.equal(value.i, Number.MAX_SAFE_INTEGER); assert.ok(Object.hasOwn(value, "__proto__")); assert.equal({}.safe, undefined);
});

test("binary64 edge values and u64/i128 strings survive decode and non-monotonic seek without recalculation", async () => {
  const records = [clone(realRecords[0]), clone(realRecords[1])], big = "18446744073709551615";
  records[0].definitions[0][0] = big; records[1].changed[0][0] = big;
  records[0].values[0][2] = 0; records[1].changed[0] = [big, 4 | 8 | 16, [-0, "170141183460469231731687303715884105727", "-170141183460469231731687303715884105728"]];
  records[0].values[1][2] = Number.MIN_VALUE; records[0].values[2][2] = Number.MAX_VALUE;
  records[1].set.accounting.medium_energy_units = "-170141183460469231731687303715884105728";
  const value = await decode(member(records));
  assert.ok(Object.is(value.frameAt(1).frame.cells[0].energy_j, -0)); assert.ok(Object.is(value.frameAt(0).frame.cells[0].energy_j, 0));
  assert.equal(value.frameAt(0).frame.cells[1].energy_j, Number.MIN_VALUE); assert.equal(value.frameAt(0).frame.cells[2].energy_j, Number.MAX_VALUE);
  assert.equal(value.frameAt(1).frame.cells[0].id, big); assert.equal(value.frameAt(1).frame.cells[0].energy_units, "-170141183460469231731687303715884105728");
  assert.deepStrictEqual([big, "9007199254740993", "9007199254740992", "2"].sort(compareDenseCellIds), ["2", "9007199254740992", "9007199254740993", big]);
  assert.throws(() => compareDenseCellIds("01", "2")); assert.ok(Object.isFrozen(value.frameAt(1).frame.cells[0]));
});

test("complete payload validation refuses malformed future deltas before exposing tick zero", async () => {
  for (const kind of ["mask0", "mask64", "maskFloat", "maskIntegralFloat", "arity", "order", "tick", "changedId", "duplicateChange", "duplicateRemoval", "unknownRemoval", "reusedBirth", "genotype", "missingField", "unknownField", "unsafeID", "wideQuantity", "fractionalGeneration", "generationIntegralFloat", "kineticsIntegralFloat", "floatTick"]) {
    const records = clone(realRecords.slice(0, 2)), delta = records[1];
    if (kind === "mask0") delta.changed[0][1] = 0;
    if (kind === "mask64") delta.changed[0][1] = 64;
    if (kind === "maskFloat") delta.changed[0][1] = 1.5;
    if (kind === "arity") delta.changed[0][2].push(0);
    if (kind === "order") delta.order = Array(records[0].definitions.length).fill(records[0].definitions[0][0]);
    if (kind === "tick") delta.set.tick = 3;
    if (kind === "changedId") delta.changed[0][0] = "18446744073709551615";
    if (kind === "duplicateChange") delta.changed.push(clone(delta.changed[0]));
    if (kind === "duplicateRemoval") delta.removed = [records[0].definitions[0][0], records[0].definitions[0][0]];
    if (kind === "unknownRemoval") delta.removed = ["18446744073709551615"];
    if (kind === "reusedBirth") delta.born = [[records[0].definitions[0], records[0].values[0]]];
    if (kind === "genotype") delta.genomes = { K0: sourceManifest.genomes.K0 };
    if (kind === "missingField") delete delta.changed;
    if (kind === "unknownField") delta.extra = true;
    if (kind === "unsafeID") records[0].definitions[0][0] = "18446744073709551616";
    if (kind === "wideQuantity") records[0].values[0][3] = "170141183460469231731687303715884105728";
    if (kind === "fractionalGeneration") records[0].definitions[0][2] = 1.1;
    const value = member(records);
    if (["floatTick", "maskIntegralFloat", "generationIntegralFloat", "kineticsIntegralFloat"].includes(kind)) {
      let text = value.payload.toString();
      if (kind === "floatTick") text = text.replace('"tick":0', '"tick":0.0');
      if (kind === "maskIntegralFloat") { const prefix = `[${JSON.stringify(delta.changed[0][0])},${delta.changed[0][1]},`; text = text.replace(prefix, prefix.slice(0, -1) + ".0,"); }
      if (kind === "generationIntegralFloat") { const definition = records[0].definitions[0], prefix = `[${JSON.stringify(definition[0])},${JSON.stringify(definition[1])},${definition[2]},`; text = text.replace(prefix, prefix.slice(0, -1) + ".0,"); }
      if (kind === "kineticsIntegralFloat") text = text.replace('"kinetics":0', '"kinetics":0.0');
      value.payload = Buffer.from(text); value.data = gzipSync(value.payload, { level: 9 }); value.data[9] = 255;
      Object.assign(value.descriptor, { gzip_bytes: value.data.length, decoded_bytes: value.payload.length, gzip_sha256: digest(value.data), decoded_sha256: digest(value.payload) });
    }
    await assert.rejects(() => decode(value), undefined, kind);
  }
});

test("removal/birth/reordering copies retained values, and a retired ID cannot reappear inside a chunk", async () => {
  const records = clone(realRecords.slice(0, 3)), oldId = records[0].definitions[0][0], newcomer = "9007199254740993";
  records[1].changed = records[1].changed.filter((row) => row[0] !== oldId); records[1].removed = [oldId];
  const definition = clone(records[0].definitions[0]); definition[0] = newcomer; definition[1] = oldId; definition[2] = 1; definition[3] = 1;
  records[1].born = [[definition, clone(records[0].values[0])]];
  records[1].order = [newcomer, ...records[0].definitions.slice(1).map((row) => row[0])].reverse();
  records[2].changed = records[2].changed.filter((row) => row[0] !== oldId); records[2].order = [newcomer, ...records[0].definitions.slice(1).map((row) => row[0])];
  const value = await decode(member(records)), folded = reference(records);
  assert.deepStrictEqual(value.frameAt(1), folded[1]); assert.deepStrictEqual(value.frameAt(2), folded[2]);
  const retired = clone(records); retired[2].born = [[records[0].definitions[0], records[0].values[0]]]; retired[2].order.push(oldId); retired[2].set.summary.living_cells++;
  await assert.rejects(() => decode(member(retired)), /reused cell ID/);
});

test("admission refuses identity, provenance, bounds, unread descriptor, index counts and external hash errors", async () => {
  for (const kind of ["world31", "source", "producer", "compiler", "attestation", "config", "unknown", "cadence", "residual", "path", "range", "decodedCap", "gzipCap", "chunkCount", "counts", "budget", "provider", "fullCounts", "schemaFloat"]) {
    const value = metadata((manifest, index) => {
      if (kind === "world31") { manifest.identity.world_format_version = 31; index.identity.world_format_version = 31; }
      if (kind === "source") { manifest.identity.source_commit = "f".repeat(40); index.identity.source_commit = "f".repeat(40); }
      if (kind === "producer") manifest.provenance.producer_tree = "bad";
      if (kind === "compiler") { manifest.provenance.runtime_rustc = "rustc 1.98.0"; index.provenance.runtime_rustc = "rustc 1.98.0"; }
      if (kind === "attestation") { manifest.provenance.build_attestation = true; index.provenance.build_attestation = true; }
      if (kind === "config") manifest.canonical_config += "\n";
      if (kind === "unknown") manifest.extra = true;
      if (kind === "cadence") manifest.experiment.sample_every = 50;
      if (kind === "residual") manifest.experiment.energy_residual_max = "1";
      if (kind === "path") manifest.chunks[0].path = "../chunk.jsonl.gz";
      if (kind === "range") manifest.chunks[0].first_tick = 1;
      if (kind === "decodedCap") manifest.chunks[0].decoded_bytes = DENSE_LIMITS.decoded + 1;
      if (kind === "gzipCap") manifest.chunks[0].gzip_bytes = DENSE_LIMITS.gzip + 1;
      if (kind === "chunkCount") manifest.bounds.max_chunks = 8193;
      if (kind === "counts") index.decoded_chunk_bytes--;
      if (kind === "budget") manifest.bounds.total_artifact_bytes_cap = 64 * 1024 * 1024 + 1;
      if (kind === "provider") manifest.bounds.provider_limits_verified = true;
      if (kind === "fullCounts") index.unique_chunks = 2;
    });
    if (kind === "schemaFloat") { value.manifestBytes = Buffer.from(value.manifestBytes.toString().replace('"schema_version":1', '"schema_version":1.0')); value.index.manifests[0].bytes = value.manifestBytes.length; value.index.manifests[0].sha256 = digest(value.manifestBytes); value.indexBytes = Buffer.from(encode(value.index)); value.indexSha256 = digest(value.indexBytes); }
    await assert.rejects(() => admit(value), undefined, kind);
  }
  await assert.rejects(() => admit({ ...metadata(), indexSha256: "0".repeat(64) }), /pinned index/);
  await assert.rejects(() => admitDenseRecording(indexBytes, Buffer.concat([manifestBytes, Buffer.from(" ")]), { indexSha256: INDEX_SHA, manifestPath: "horizon-100.json", cryptoProvider: webcrypto }), /integrity/);
  await assert.rejects(() => admitDenseRecording(indexBytes, manifestBytes, { manifestPath: "horizon-100.json", cryptoProvider: webcrypto }), /pinned index/);
});

test("digest/commit fields reject coherent one-element arrays, including unread descriptors", async () => {
  await admit(metadata());
  for (const key of ["producer_commit", "producer_tree"]) {
    const value = metadata((manifest, index) => { const coerced = [manifest.provenance[key]]; manifest.provenance[key] = coerced; index.provenance[key] = clone(coerced); });
    await assert.rejects(() => admit(value), /anchors/, key);
  }
  for (const key of ["gzip_sha256", "decoded_sha256"]) {
    const value = metadata((manifest) => { manifest.chunks[0][key] = [manifest.chunks[0][key]]; });
    await assert.rejects(() => admit(value), /digests/, key);
    const { files, meta } = threeChunks(); meta.manifest.chunks[2][key] = [meta.manifest.chunks[2][key]];
    const mb = Buffer.from(encode(meta.manifest)); meta.index.manifests[0].bytes = mb.length; meta.index.manifests[0].sha256 = digest(mb); const ib = Buffer.from(encode(meta.index)); files.set("horizon-100.json", mb); files.set("index.json", ib);
    const requests = [];
    await assert.rejects(() => createDenseRecordingLoader(loaderOptions(files, async (url) => { const name = new URL(url).pathname.split("/").at(-1); requests.push(name); return response(files.get(name)); })), /digests/);
    assert.deepStrictEqual(requests, ["index.json", "horizon-100.json"], `unread ${key} rejected before any chunk GET`);
  }
  const unread = metadata((_, index) => { index.manifests.push({ path: "horizon-10000.json", bytes: 1, sha256: ["1".repeat(64)], first_tick: 0, last_tick: 10_000, frames: 10_001 }); });
  await assert.rejects(() => admit(unread), /manifest descriptor/);
  await assert.rejects(() => admit({ ...metadata(), indexSha256: [digest(metadata().indexBytes)] }), /pinned index/);
  await assert.rejects(() => createDenseRecordingLoader({ ...loaderOptions(originalFiles()), indexSha256: [INDEX_SHA] }), /pinned index/);
});

test("prefix cap uses full horizon: 64MiB / 512MiB / decimal 3GB and smaller existing caps", async () => {
  for (const [horizon, total] of [[10_000, 64 * 1024 * 1024], [100_000, 512 * 1024 * 1024], [1_000_000, 3_000_000_000]]) {
    const value = metadata((manifest, index) => { index.manifests.push({ path: `horizon-${horizon}.json`, bytes: 1, sha256: "1".repeat(64), first_tick: 0, last_tick: horizon, frames: horizon + 1 }); manifest.bounds.total_artifact_bytes_cap = total; });
    await admit(value);
    const tooLarge = metadata((manifest, index) => { index.manifests = value.index.manifests; manifest.bounds.total_artifact_bytes_cap = total + 1; }); await assert.rejects(() => admit(tooLarge), /integer|cap/);
  }
  await admit(metadata((manifest, index) => { index.manifests.push({ path: "horizon-1000000.json", bytes: 1, sha256: "1".repeat(64), first_tick: 0, last_tick: 1_000_000, frames: 1_000_001 }); manifest.bounds.total_artifact_bytes_cap = 64 * 1024 * 1024; }));
});

test("gzip corruption, truncation, extra member, trailing bytes, hash mismatch and bomb refuse before frames", async () => {
  const original = member([clone(realRecords[0])]);
  for (const kind of ["truncated", "crc", "size", "extra", "trailing", "header", "gzipHash", "decodedHash", "noLF", "duplicateKeys"]) {
    const value = { data: Buffer.from(original.data), descriptor: { ...original.descriptor }, payload: original.payload };
    if (kind === "truncated") value.data = value.data.subarray(0, value.data.length - 1);
    if (kind === "crc") value.data[value.data.length - 8] ^= 1;
    if (kind === "size") value.data[value.data.length - 4] ^= 1;
    if (kind === "extra") value.data = Buffer.concat([value.data, value.data]);
    if (kind === "trailing") value.data = Buffer.concat([value.data, Buffer.from([0])]);
    if (kind === "header") value.data[9] = 3;
    if (kind === "noLF" || kind === "duplicateKeys") {
      value.payload = kind === "noLF" ? original.payload.subarray(0, original.payload.length - 1) : Buffer.from(original.payload.toString().replace('"type":"keyframe"', '"type":"keyframe","type":"keyframe"'));
      value.data = gzipSync(value.payload, { level: 9 }); value.data[9] = 255; value.descriptor.decoded_bytes = value.payload.length; value.descriptor.decoded_sha256 = digest(value.payload);
    }
    value.descriptor.gzip_bytes = value.data.length; value.descriptor.gzip_sha256 = digest(value.data);
    if (kind === "gzipHash") value.descriptor.gzip_sha256 = "0".repeat(64);
    if (kind === "decodedHash") value.descriptor.decoded_sha256 = "0".repeat(64);
    await assert.rejects(() => decode(value), undefined, kind);
  }
  const bomb = gzipSync(Buffer.alloc(DENSE_LIMITS.decoded + 1, 120), { level: 9 }); bomb[9] = 255;
  await assert.rejects(() => decode({ data: bomb, descriptor: { ...original.descriptor, gzip_bytes: bomb.length, gzip_sha256: digest(bomb), decoded_bytes: DENSE_LIMITS.decoded } }), /inflated byte cap/);
  for (const options of [{ strategy: constants.Z_FIXED }, { level: 0 }]) assert.deepStrictEqual((await decode(member([realRecords[0]], options))).frameAt(0), expected[0]);
});

test("loader loads only selected assets as same-origin GET, reuses payload and retains one current frame", async () => {
  const files = originalFiles(), requests = [];
  const loader = await createDenseRecordingLoader(loaderOptions(files, async (url, options) => { requests.push({ url, options }); return response(files.get(new URL(url).pathname.split("/").at(-1)), { etag: '"not-provenance"' }); }));
  for (const tick of [50, 0, 100, 1, 99]) assert.deepStrictEqual(await loader.seek(tick), expected[tick]);
  assert.equal(requests.length, 3); assert.equal(loader.stats.maxRetainedChunks, 1); assert.equal(loader.stats.cachedChunks, 1); assert.equal(loader.stats.retainedFrames, 1);
  for (const { url, options } of requests) { assert.equal(new URL(url).origin, new URL(origin).origin); assert.equal(options.method, "GET"); assert.equal(options.redirect, "error"); assert.equal(options.credentials, "omit"); }
  loader.close(); assert.equal(loader.stats.retainedChunks, 0); assert.equal(loader.current, null); await assert.rejects(() => loader.seek(0), /closed/);
});

test("independent three-chunk seek holds current plus one prefetch and never 256 reconstructed states", async () => {
  const { files } = threeChunks(), requests = [];
  const loader = await createDenseRecordingLoader(loaderOptions(files, async (url) => { requests.push(new URL(url).pathname.split("/").at(-1)); return response(files.get(requests.at(-1))); }));
  assert.deepStrictEqual(await loader.seek(49), expected[49]); await until(() => loader.stats.cachedChunks === 2);
  assert.equal(requests.length, 4); assert.ok(!requests.includes("chunk-0000100-0000100.jsonl.gz"));
  assert.deepStrictEqual(await loader.seek(50), expected[50]); await until(() => loader.stats.cachedChunks === 2); assert.equal(requests.length, 5);
  assert.deepStrictEqual(await loader.seek(100), expected[100]); assert.equal(loader.stats.retainedChunks, 1);
  assert.deepStrictEqual(await loader.seek(0), expected[0]); await until(() => loader.stats.cachedChunks === 2);
  assert.equal(loader.stats.maxRetainedChunks, 2); assert.equal(loader.stats.retainedFrames, 1); loader.close();
});

test("full 256-record chunk seeks its last frame with exactly 255 retained deltas", async () => {
  const first = keyframe(expected[0]); first.definitions = []; first.values = []; first.frame.summary.living_cells = 0;
  const records = [first];
  for (let tick = 1; tick < 256; tick++) records.push({ type: "delta", set: { tick, sim_time: tick * 30, residual: { matter: "0", energy: "0" } }, born: [], removed: [], changed: [], genomes: {} });
  const value = await decode(member(records)); assert.equal(value.records, 256); assert.equal(value.retainedFrames, 0); assert.equal(value.frameAt(255).frame.tick, 255); assert.deepStrictEqual(value.frameAt(255).frame.accounting, first.frame.accounting);
  records.push({ ...records.at(-1), set: { tick: 256, sim_time: 256 * 30 } }); await assert.rejects(() => decode(member(records)), /integer|range/);
});

test("an unread second descriptor is rejected before fetching any chunk", async () => {
  const { files, meta } = threeChunks(), requests = [];
  meta.manifest.chunks[2].path = "chunk-0000100-0000100.jsonl.gz?external=yes";
  const mb = Buffer.from(encode(meta.manifest)); meta.index.manifests[0].bytes = mb.length; meta.index.manifests[0].sha256 = digest(mb); const ib = Buffer.from(encode(meta.index)); files.set("horizon-100.json", mb); files.set("index.json", ib);
  await assert.rejects(() => createDenseRecordingLoader(loaderOptions(files, async (url) => { const name = new URL(url).pathname.split("/").at(-1); requests.push(name); return response(files.get(name)); })), /path/);
  assert.deepStrictEqual(requests, ["index.json", "horizon-100.json"]);
});

test("published shards retain payload bytes, accept both formats and reject mixed/wrong/unread shard paths", async () => {
  const { chunks } = threeChunks();
  const published = metadata((manifest, index) => { manifest.chunks = chunks.map((chunk, ordinal) => ({ ...chunk.descriptor, path: `chunks/${String(Math.floor(ordinal / 256)).padStart(2, "0")}/${chunk.descriptor.path}` })); index.unique_chunks = chunks.length; index.unique_gzip_bytes = chunks.reduce((sum, chunk) => sum + chunk.data.length, 0); index.decoded_chunk_bytes = chunks.reduce((sum, chunk) => sum + chunk.payload.length, 0); });
  const files = new Map([["index.json", published.indexBytes], ["horizon-100.json", published.manifestBytes], ...published.manifest.chunks.map((chunk, ordinal) => [chunk.path, chunks[ordinal].data])]);
  await admit(published); const loader = await createDenseRecordingLoader(loaderOptions(files));
  for (const tick of [49, 50, 100]) assert.deepStrictEqual(await loader.seek(tick), expected[tick]); assert.equal(loader.stats.maxRetainedChunks, 2); loader.close();
  await assert.rejects(() => decodeDenseChunk(chunks[0].data, published.manifest.chunks[0], sourceManifest.genomes, { cryptoProvider: webcrypto }), /ordinal/);
  assert.deepStrictEqual((await decodeDenseChunk(chunks[0].data, published.manifest.chunks[0], sourceManifest.genomes, { cryptoProvider: webcrypto, chunkOrdinal: 0 })).frameAt(0), expected[0]);
  for (const path of ["chunks/01/chunk-0000100-0000100.jsonl.gz", "chunks/32/chunk-0000100-0000100.jsonl.gz", "chunks/0/chunk-0000100-0000100.jsonl.gz", "chunks/00/../chunk-0000100-0000100.jsonl.gz", "chunks/00/chunk-0000100-0000100.jsonl.gz?x=1", "chunk-0000100-0000100.jsonl.gz"]) {
    const bad = metadata((manifest, index) => { Object.assign(manifest, clone(published.manifest)); Object.assign(index, clone(published.index)); manifest.chunks[2].path = path; }); await assert.rejects(() => admit(bad), /path|shard/);
  }
});

test("shard number comes from descriptor ordinal, including the 255/256 boundary", async () => {
  async function boundary(wrong) {
    // Metadata-only corruption fixture; no invented observations are emitted.
    const manifest = clone(sourceManifest), index = clone(sourceIndex);
    manifest.experiment.steps = manifest.experiment.checked_ticks = 256; manifest.experiment.frames = 257;
    manifest.chunks = Array.from({ length: 257 }, (_, ordinal) => ({ ...sourceManifest.chunks[0], first_tick: ordinal, last_tick: ordinal, frames: 1, path: `chunks/${String(Math.floor(ordinal / 256)).padStart(2, "0")}/chunk-${String(ordinal).padStart(7, "0")}-${String(ordinal).padStart(7, "0")}.jsonl.gz` }));
    if (wrong) manifest.chunks[256].path = manifest.chunks[256].path.replace("chunks/01/", "chunks/00/");
    const mb = Buffer.from(encode(manifest)); index.manifests = [{ path: "horizon-256.json", first_tick: 0, last_tick: 256, frames: 257, bytes: mb.length, sha256: digest(mb) }]; index.unique_chunks = 257; index.unique_gzip_bytes = manifest.chunks.reduce((sum, chunk) => sum + chunk.gzip_bytes, 0); index.decoded_chunk_bytes = manifest.chunks.reduce((sum, chunk) => sum + chunk.decoded_bytes, 0);
    const ib = Buffer.from(encode(index)); return admitDenseRecording(ib, mb, { indexSha256: digest(ib), manifestPath: "horizon-256.json", cryptoProvider: webcrypto });
  }
  const good = await boundary(false); assert.match(good.manifest.chunks[255].path, /^chunks\/00\//); assert.match(good.manifest.chunks[256].path, /^chunks\/01\//); await assert.rejects(() => boundary(true), /ordinal/);
});

test("seek serial prevents late fetch/late failure from committing after a newer seek", async () => {
  for (const lateFailure of [false, true]) {
    const { files, chunks } = threeChunks(); let release, started = false;
    const delayed = new Promise((resolve, reject) => { release = () => lateFailure ? reject(new Error("stale failure")) : resolve(response(files.get(chunks[0].descriptor.path))); });
    const loader = await createDenseRecordingLoader({ ...loaderOptions(files, async (url) => { const path = new URL(url).pathname.split("/").at(-1); if (path === chunks[0].descriptor.path) { started = true; return delayed; } return response(files.get(path)); }), prefetch: false });
    const oldSeek = loader.seek(0); void oldSeek.catch(() => {}); await until(() => started);
    const newer = loader.seek(100); await assert.rejects(() => oldSeek, /superseded|abort/i); assert.deepStrictEqual(await newer, expected[100]); release(); await new Promise((done) => setTimeout(done, 15));
    assert.equal(loader.stats.currentTick, 100); assert.equal(loader.stats.failed, false); assert.equal(loader.stats.maxRetainedChunks, 1); loader.close();
  }
});

test("caller/lifetime abort clears in-flight state and corrupt active prefetch fails closed", async () => {
  const { files, chunks } = threeChunks(); const lifetime = new AbortController(); let started = false;
  const loader = await createDenseRecordingLoader({ ...loaderOptions(files, async (url) => { const path = new URL(url).pathname.split("/").at(-1); if (path === chunks[0].descriptor.path) { started = true; return new Promise(() => {}); } return response(files.get(path)); }), prefetch: false, signal: lifetime.signal });
  const caller = new AbortController(), pending = loader.seek(0, { signal: caller.signal }); void pending.catch(() => {}); await until(() => started); caller.abort(); await assert.rejects(() => pending, /abort/i); assert.equal(loader.stats.retainedChunks, 0); assert.equal(loader.current, null);
  lifetime.abort(); assert.equal(loader.stats.closed, true);
  const bad = threeChunks(); bad.files.set(bad.chunks[1].descriptor.path, Buffer.from(bad.chunks[1].data).fill(0));
  const failed = await createDenseRecordingLoader(loaderOptions(bad.files)); await failed.seek(0); await until(() => failed.stats.failed); assert.equal(failed.current, null); await assert.rejects(() => failed.seek(1), /integrity/); failed.close();
});

test("paused real-frame loader immediately notifies a corrupt active prefetch without another seek", async () => {
  const { files, chunks } = threeChunks(); let release, started = false;
  const held = new Promise((done) => { release = done; });
  const loader = await createDenseRecordingLoader(loaderOptions(files, async (url) => {
    const path = new URL(url).pathname.split("/").at(-1);
    if (path === chunks[1].descriptor.path) { started = true; await held; return response(Buffer.from(chunks[1].data).fill(0)); }
    return response(files.get(path));
  }));
  const notifications = [], unsubscribe = loader.subscribeFailure((error) => notifications.push(error));
  assert.throws(() => loader.subscribeFailure(() => {}), /only one/);
  const validated = await loader.seek(0); assert.deepStrictEqual(validated, expected[0]); await until(() => started);
  assert.equal(loader.current, validated); assert.deepStrictEqual(notifications, []);
  release(); await until(() => notifications.length === 1);
  const original = notifications[0]; assert.match(original.message, /gzip integrity/); assert.equal(loader.stats.failed, true); assert.equal(loader.current, null);
  assert.deepStrictEqual(validated, expected[0], "the consumer's immutable last validated snapshot remains usable");
  await assert.rejects(() => loader.seek(1), (error) => error === original); assert.equal(notifications.length, 1);
  unsubscribe(); unsubscribe(); const late = [], removeLate = loader.subscribeFailure((error) => late.push(error));
  assert.deepStrictEqual(late, [original], "late subscription receives retained failure immediately"); unsubscribe();
  assert.throws(() => loader.subscribeFailure(() => {}), /only one/, "old cleanup cannot remove a later subscription");
  removeLate(); loader.close(); assert.throws(() => loader.subscribeFailure(() => {}), /closed/);
});

test("failure subscription removal and close suppress pending prefetch callbacks", async () => {
  for (const mode of ["unsubscribe", "close"]) {
    const { files, chunks } = threeChunks(); const original = new Error(`${mode} original transport failure`); let release, started = false;
    const held = new Promise((_, reject) => { release = () => reject(original); });
    const loader = await createDenseRecordingLoader(loaderOptions(files, async (url) => {
      const path = new URL(url).pathname.split("/").at(-1);
      if (path === chunks[1].descriptor.path) { started = true; return held; }
      return response(files.get(path));
    }));
    const notifications = [], unsubscribe = loader.subscribeFailure((error) => notifications.push(error));
    await loader.seek(0); await until(() => started); mode === "close" ? loader.close() : unsubscribe();
    release(); await new Promise((done) => setTimeout(done, 25)); assert.deepStrictEqual(notifications, []);
    if (mode === "unsubscribe") {
      assert.equal(loader.stats.failed, true); const late = []; loader.subscribeFailure((error) => late.push(error)); assert.deepStrictEqual(late, [original]);
    } else { assert.equal(loader.stats.closed, true); assert.equal(loader.stats.failed, false); }
    loader.close(); unsubscribe();
  }
});

test("stale prefetch rejection and caller abort never notify fatal failure", async () => {
  const { files, chunks } = threeChunks(); let rejectLate, started = false;
  const held = new Promise((_, reject) => { rejectLate = reject; });
  const loader = await createDenseRecordingLoader(loaderOptions(files, async (url) => {
    const path = new URL(url).pathname.split("/").at(-1);
    if (path === chunks[1].descriptor.path) { started = true; return held; }
    return response(files.get(path));
  }));
  const notifications = []; loader.subscribeFailure((error) => notifications.push(error));
  await loader.seek(0); await until(() => started); assert.deepStrictEqual(await loader.seek(100), expected[100]);
  rejectLate(new Error("stale background error")); await new Promise((done) => setTimeout(done, 25));
  assert.deepStrictEqual(notifications, []); assert.equal(loader.stats.failed, false); assert.equal(loader.current.frame.tick, 100); loader.close();
  const caller = new AbortController(); let callerStarted = false;
  const canceled = await createDenseRecordingLoader({ ...loaderOptions(files, async (url) => {
    const path = new URL(url).pathname.split("/").at(-1); if (path === chunks[0].descriptor.path) { callerStarted = true; return new Promise(() => {}); } return response(files.get(path));
  }), prefetch: false });
  canceled.subscribeFailure((error) => notifications.push(error));
  const pending = canceled.seek(0, { signal: caller.signal }); void pending.catch(() => {}); await until(() => callerStarted); caller.abort();
  await assert.rejects(() => pending, /abort/i); assert.deepStrictEqual(notifications, []); assert.equal(canceled.stats.failed, false); canceled.close();
});

test("throwing or rejecting failure callbacks preserve the original transport error and create no detached rejection", async () => {
  for (const asynchronous of [false, true]) {
    const files = originalFiles(), original = new Error("original chunk transport failure"), consumer = new Error("consumer failure");
    const loader = await createDenseRecordingLoader(loaderOptions(files, async (url) => {
      const path = new URL(url).pathname.split("/").at(-1); if (path.endsWith(".gz")) throw original; return response(files.get(path));
    }));
    assert.throws(() => loader.subscribeFailure(null), /callback/); const notifications = [];
    const unsubscribe = loader.subscribeFailure((error) => { notifications.push(error); if (asynchronous) return Promise.reject(consumer); throw consumer; });
    await assert.rejects(() => loader.seek(0), (error) => error === original); await assert.rejects(() => loader.seek(1), (error) => error === original);
    assert.deepStrictEqual(notifications, [original]); unsubscribe();
    loader.subscribeFailure(() => Promise.reject(consumer)); await new Promise((done) => setTimeout(done, 25)); loader.close();
  }
});

test("mid-body abort and synchronous fetch abort produce only caught errors, never unhandled rejections", () => {
  // Isolate process-level rejection observation from node:test's own listener.
  // Native Response/ReadableStream models a fetch body errored by AbortSignal.
  const source = `
    import { createDenseRecordingLoader } from ${JSON.stringify(new URL("./dense-recording.mjs", import.meta.url).href)};
    import { webcrypto } from "node:crypto";
    const input = Uint8Array.from(${JSON.stringify(Array.from(indexBytes))}), unhandled = [], caught = [];
    process.on("unhandledRejection", (error) => unhandled.push(error.name));
    for (const turns of [0, 1, 2, "synchronous"]) {
      const lifetime = new AbortController(); let count = 0;
      const stream = new ReadableStream({
        start(controller) { lifetime.signal.addEventListener("abort", () => controller.error(lifetime.signal.reason)); },
        pull(controller) {
          if (count++ === 0) {
            controller.enqueue(input.subarray(0, 16));
            const after = (left) => queueMicrotask(() => left ? after(left - 1) : lifetime.abort()); after(Number(turns));
          }
        },
      }, { highWaterMark: 0 });
      const fetcher = turns === "synchronous" ? () => { lifetime.abort(); return Promise.reject(lifetime.signal.reason); } : async () => new Response(stream);
      try {
        await createDenseRecordingLoader({ indexUrl: "https://example.test/dense/index.json", indexSha256: ${JSON.stringify(INDEX_SHA)}, manifestPath: "horizon-100.json", baseUrl: "https://example.test/", cryptoProvider: webcrypto, signal: lifetime.signal, fetcher });
        caught.push("unexpected success");
      } catch (error) { caught.push(error.name); }
      await new Promise((done) => setTimeout(done, 15));
    }
    console.log(JSON.stringify({ caught, unhandled }));
  `;
  const child = spawnSync(process.execPath, ["--input-type=module", "-e", source], { encoding: "utf8", timeout: 5000, maxBuffer: 64 * 1024 });
  assert.equal(child.status, 0, child.stderr || child.error?.message);
  assert.deepStrictEqual(JSON.parse(child.stdout.trim()), { caught: ["AbortError", "AbortError", "AbortError", "AbortError"], unhandled: [] });
});

test("transport refuses cross-origin/redirected paths, oversize declared and streamed bodies, and wrong byte lengths", async () => {
  const options = loaderOptions(originalFiles()); await assert.rejects(() => createDenseRecordingLoader({ ...options, indexUrl: "https://other.test/index.json" }), /unsafe/);
  await assert.rejects(() => createDenseRecordingLoader({ ...options, manifestPath: "../hidden.json" }), /allowlisted/);
  await assert.rejects(() => createDenseRecordingLoader({ ...options, fetcher: async () => response(indexBytes, { "content-length": String(DENSE_LIMITS.index + 1) }) }), /Content-Length/);
  let canceled = false;
  const stream = new ReadableStream({ start(controller) { controller.enqueue(new Uint8Array(DENSE_LIMITS.index + 1)); }, cancel() { canceled = true; } });
  await assert.rejects(() => createDenseRecordingLoader({ ...options, fetcher: async () => response(stream) }), /byte cap/); await until(() => canceled);
  await assert.rejects(() => createDenseRecordingLoader({ ...options, fetcher: async (url) => new Response(new URL(url).pathname.endsWith("index.json") ? indexBytes : manifestBytes.subarray(0, manifestBytes.length - 1)) }), /byte count/);
  await assert.rejects(() => createDenseRecordingLoader({ ...options, fetcher: async () => ({ status: 200, url: "https://other.test/index.json", headers: new Headers(), body: new Blob([indexBytes]).stream() }) }), /URL mismatch/);
});
