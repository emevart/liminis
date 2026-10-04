import test from "node:test";
import assert from "node:assert/strict";
import { cp, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { attachDenseRecordings } from "../scripts/attach_dense_catalog.mjs";

const fixture = new URL("../scripts/fixtures/dense-recording/publication-100/", import.meta.url);
const manifest = JSON.parse(await readFile(new URL("horizon-100.json", fixture)));
const entry = { identity: manifest.identity, experiment: manifest.experiment, archive: "unchanged" };

test("catalog attachment validates a real saved pilot publication and preserves archival fields", async () => {
  const [attached] = await attachDenseRecordings([entry], fixture);
  assert.equal(attached.archive, "unchanged");
  assert.equal(attached.identity, entry.identity);
  assert.equal(attached.experiment, entry.experiment);
  assert.equal(attached.dense.index_sha256, "d6c4cf074f981fe55979b8ab4853ad3f624b455ac2235e39e5d42618943e8738");
  assert.equal(attached.dense.publication_sha256, "1a54d47560d293169e7261bd53cb9dc46d4d1a73289aa9789bbd9f52bc261b01");
});

test("broken or partial dense delivery fails closed instead of reverting to the archive", async () => {
  const temp = await mkdtemp(join(tmpdir(), "liminis-catalog-delivery-"));
  try {
    for (const fault of ["corrupted_chunk", "missing_publication", "extra_file", "wrong_horizon"]) {
      const directory = join(temp, fault); await cp(fixture, directory, { recursive: true });
      let candidate = entry;
      if (fault === "corrupted_chunk") {
        const path = join(directory, "chunks/00/chunk-0000000-0000100.jsonl.gz"), bytes = await readFile(path); bytes[30] ^= 1; await writeFile(path, bytes);
      } else if (fault === "missing_publication") await rm(join(directory, "publication.json"));
      else if (fault === "extra_file") await writeFile(join(directory, "unexpected.json"), "{}");
      else candidate = { ...entry, experiment: { ...entry.experiment, steps: 101 } };
      const expected = { corrupted_chunk: /chunk digest mismatch/, missing_publication: /ENOENT/, extra_file: /unexpected publication file/, wrong_horizon: /archive\/dense experiment identity mismatch/ };
      await assert.rejects(attachDenseRecordings([candidate], directory), expected[fault], fault);
    }
  } finally { await rm(temp, { recursive: true, force: true }); }
});

test("a repository with no dense directory remains an explicit archival catalog", async () => {
  const temp = await mkdtemp(join(tmpdir(), "liminis-catalog-archive-"));
  const entries = [entry];
  try { assert.equal(await attachDenseRecordings(entries, join(temp, "absent")), entries); }
  finally { await rm(temp, { recursive: true, force: true }); }
});

test("coherently rehashed metadata cannot attach a prefix whose chunk is missing", async () => {
  const temp = await mkdtemp(join(tmpdir(), "liminis-catalog-prefix-"));
  try {
    const directory = join(temp, "dataset"); await cp(fixture, directory, { recursive: true });
    const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
    const encode = (object) => Buffer.from(JSON.stringify(object));
    const index = JSON.parse(await readFile(join(directory, "index.json")));
    const priorIndex = JSON.parse(await readFile(join(directory, "original/index.json")));
    const publication = JSON.parse(await readFile(join(directory, "publication.json")));
    const descriptor = (path, bytes) => ({ path, bytes: bytes.length, sha256: sha(bytes), first_tick: 0, last_tick: 1, frames: 2 });
    const prefix = structuredClone(manifest); Object.assign(prefix.experiment, { steps: 1, checked_ticks: 1, frames: 2 });
    Object.assign(prefix.chunks[0], { path: "chunks/00/chunk-0000000-0000001.jsonl.gz", first_tick: 0, last_tick: 1, frames: 2 });
    const publishedBytes = encode(prefix), prior = structuredClone(prefix); prior.chunks[0].path = "chunk-0000000-0000001.jsonl.gz";
    const priorBytes = encode(prior);
    index.manifests.unshift(descriptor("horizon-1.json", publishedBytes));
    priorIndex.manifests.unshift(descriptor("horizon-1.json", priorBytes));
    await writeFile(join(directory, "horizon-1.json"), publishedBytes);
    await writeFile(join(directory, "original/horizon-1.json"), priorBytes);
    const indexBytes = encode(index), originalBytes = encode(priorIndex);
    await writeFile(join(directory, "index.json"), indexBytes); await writeFile(join(directory, "original/index.json"), originalBytes);
    publication.published_index_sha256 = sha(indexBytes); publication.original_index_sha256 = sha(originalBytes);
    const paths = [...publication.metadata_files.map((entry) => entry.path), "horizon-1.json", "original/horizon-1.json"];
    publication.metadata_files = [];
    for (const path of paths) { const bytes = await readFile(join(directory, path)); publication.metadata_files.push({ path, bytes: bytes.length, sha256: sha(bytes) }); }
    publication.metadata_bytes_excluding_publication = publication.metadata_files.reduce((sum, entry) => sum + entry.bytes, 0);
    await writeFile(join(directory, "publication.json"), encode(publication));
    const entry = { identity: prefix.identity, experiment: prefix.experiment };
    await assert.rejects(attachDenseRecordings([entry], directory), /horizon chunks are not a shared full-trajectory prefix/);
  } finally { await rm(temp, { recursive: true, force: true }); }
});
