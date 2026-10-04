import { lstat, readFile, readdir } from "node:fs/promises";
import { createHash, webcrypto } from "node:crypto";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { admitDenseRecording, parseDenseJson, DENSE_LIMITS } from "../site/dense-recording.mjs";

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const hex = (value) => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
const natural = (value) => Number.isSafeInteger(value) && value >= 0 && !Object.is(value, -0);
const requireThat = (condition, message) => { if (!condition) throw new Error(`Dense catalog: ${message}`); };
const same = (left, right, message) => requireThat(isDeepStrictEqual(left, right), message);

// This build-time check hashes each immutable file, one bounded file at a time.
// Runtime trusts the catalog-pinned index chain; publication is audit evidence.
export async function attachDenseRecordings(entries, location = new URL("../site/data/dense-cell-chamber/", import.meta.url)) {
  const directory = resolve(location instanceof URL ? fileURLToPath(location) : location);
  let root;
  try { root = await lstat(directory); } catch (error) { if (error.code === "ENOENT") return entries; throw error; }
  requireThat(root.isDirectory() && !root.isSymbolicLink(), "unsafe publication directory");
  async function read(path, maximum) {
    requireThat(typeof path === "string" && /^(?:original\/)?(?:index\.json|horizon-[0-9]+\.json|validation-receipt\.json)$|^publication\.json$|^chunks\/[0-9]{2}\/chunk-[0-9]{7}-[0-9]{7}\.jsonl\.gz$/.test(path), "unsafe publication path");
    let target = directory;
    for (const component of path.split("/")) {
      target = join(target, component); const stat = await lstat(target);
      requireThat(!stat.isSymbolicLink(), "publication symlink");
    }
    const stat = await lstat(target);
    requireThat(stat.isFile() && stat.size > 0 && stat.size <= maximum, "publication file size/type");
    const bytes = await readFile(target); requireThat(bytes.length === stat.size, "file changed during read"); return bytes;
  }
  const publicationBytes = await read("publication.json", 4 * 1024 * 1024), publication = parseDenseJson(publicationBytes);
  requireThat(publication.schema_version === 1 && publication.kind === "dense_recording_publication"
    && hex(publication.original_index_sha256) && hex(publication.published_index_sha256), "unsupported publication");
  same(publication.layout, { kind: "ordinal_shards_v1", files_per_directory: 256, max_directories: 32 }, "wrong shard layout");
  const indexBytes = await read("index.json", DENSE_LIMITS.index), indexSha = sha(indexBytes), index = parseDenseJson(indexBytes);
  requireThat(indexSha === publication.published_index_sha256 && Array.isArray(index.manifests) && index.manifests.length >= 1 && index.manifests.length <= 3, "published index mismatch");
  const manifests = new Map();
  for (const descriptor of index.manifests) {
    const bytes = await read(descriptor.path, DENSE_LIMITS.manifest);
    const admitted = await admitDenseRecording(indexBytes, bytes, { indexSha256: indexSha, manifestPath: descriptor.path, cryptoProvider: webcrypto });
    manifests.set(descriptor.path, admitted.manifest);
  }
  const full = manifests.get(index.manifests.at(-1).path);
  same(publication.identity, index.identity, "publication identity mismatch");
  same(publication.provenance, index.provenance, "publication provenance mismatch");
  const validation = publication.source_validation;
  requireThat(validation && validation.all_gzip_size_sha_crc_inflated_sha_checked === true, "missing source validation");
  const reuse = validation.method === "externally_pinned_full_decode_receipt";
  requireThat(reuse || validation.method === "full_unique_trajectory_schema_decode", "unsupported validation method");
  requireThat(validation.decoded_frames_here === (reuse ? 0 : full.experiment.frames), "incomplete full validation");
  const expected = new Set(["index.json", "original/index.json"]);
  for (const descriptor of index.manifests) { expected.add(descriptor.path); expected.add(`original/${descriptor.path}`); }
  if (reuse) expected.add("original/validation-receipt.json");
  requireThat(Array.isArray(publication.metadata_files) && publication.metadata_files.length === expected.size, "metadata inventory mismatch");
  const original = new Map(); let metadataBytes = 0;
  for (const descriptor of publication.metadata_files) {
    requireThat(descriptor && expected.delete(descriptor.path) && natural(descriptor.bytes) && descriptor.bytes > 0 && hex(descriptor.sha256), "invalid/duplicate metadata descriptor");
    const bytes = await read(descriptor.path, DENSE_LIMITS.manifest);
    requireThat(bytes.length === descriptor.bytes && sha(bytes) === descriptor.sha256, "metadata digest mismatch");
    original.set(descriptor.path, bytes); metadataBytes += bytes.length;
  }
  requireThat(expected.size === 0 && sha(original.get("original/index.json")) === publication.original_index_sha256, "original index mismatch");
  const originalIndex = parseDenseJson(original.get("original/index.json"));
  requireThat(Array.isArray(originalIndex.manifests) && originalIndex.manifests.length === index.manifests.length, "original manifest inventory mismatch");
  for (let i = 0; i < index.manifests.length; i++) {
    const oldDescriptor = originalIndex.manifests[i], descriptor = index.manifests[i];
    requireThat(oldDescriptor.path === descriptor.path, "original horizon changed");
    const oldBytes = original.get(`original/${descriptor.path}`);
    requireThat(oldBytes.length === oldDescriptor.bytes && sha(oldBytes) === oldDescriptor.sha256, "original manifest digest mismatch");
    const prior = parseDenseJson(oldBytes), published = manifests.get(descriptor.path);
    requireThat(Array.isArray(prior.chunks) && prior.chunks.length === published.chunks.length, "original chunk inventory changed");
    for (let ordinal = 0; ordinal < prior.chunks.length; ordinal++) {
      requireThat(prior.chunks[ordinal].path === published.chunks[ordinal].path.split("/").at(-1), "original chunk name changed");
      prior.chunks[ordinal].path = published.chunks[ordinal].path;
    }
    same(prior, published, "publication changed observations or original metadata");
    oldDescriptor.bytes = descriptor.bytes; oldDescriptor.sha256 = descriptor.sha256;
  }
  same(originalIndex, index, "publication changed original index fields");
  if (reuse) {
    const bytes = original.get("original/validation-receipt.json"), receipt = parseDenseJson(bytes);
    requireThat(bytes.length <= 64 * 1024 && hex(validation.external_receipt_sha256) && sha(bytes) === validation.external_receipt_sha256
      && hex(validation.validation_report_sha256) && receipt.validation_report_sha256 === validation.validation_report_sha256
      && receipt.kind === "full_dense_recording_validation" && receipt.schema_version === 1 && receipt.status === "PASS"
      && receipt.source_index_sha256 === publication.original_index_sha256 && receipt.producer_commit === index.provenance.producer_commit
      && receipt.horizon === full.experiment.steps && receipt.frames === full.experiment.frames && receipt.decoded_frames === full.experiment.frames
      && receipt.matter_residual_max === "0" && receipt.energy_residual_max === "0", "validation receipt mismatch");
  } else requireThat(validation.external_receipt_sha256 === null && validation.validation_report_sha256 === null, "unexpected receipt metadata");
  requireThat(Array.isArray(publication.mapping) && publication.mapping.length === full.chunks.length, "chunk mapping mismatch");
  const inventory = new Set(["publication.json", ...original.keys()]); let gzipBytes = 0;
  for (const [ordinal, chunk] of full.chunks.entries()) {
    const mapping = publication.mapping[ordinal];
    same(mapping, { original_path: chunk.path.split("/").at(-1), published_path: chunk.path, gzip_bytes: chunk.gzip_bytes, gzip_sha256: chunk.gzip_sha256, decoded_bytes: chunk.decoded_bytes, decoded_sha256: chunk.decoded_sha256 }, "path-only mapping mismatch");
    requireThat(!inventory.has(chunk.path), "duplicate chunk path"); inventory.add(chunk.path);
    const bytes = await read(chunk.path, DENSE_LIMITS.gzip);
    requireThat(bytes.length === chunk.gzip_bytes && sha(bytes) === chunk.gzip_sha256, "chunk digest mismatch"); gzipBytes += bytes.length;
  }
  requireThat(metadataBytes === publication.metadata_bytes_excluding_publication && gzipBytes === publication.gzip_bytes
    && publication.total_artifact_bytes_cap === full.bounds.total_artifact_bytes_cap
    && gzipBytes + metadataBytes + publicationBytes.length <= full.bounds.total_artifact_bytes_cap, "complete publication budget mismatch");
  async function checkInventory(path = "") {
    for (const item of await readdir(join(directory, path), { withFileTypes: true })) {
      const child = path ? `${path}/${item.name}` : item.name;
      if (item.isDirectory()) { requireThat([...inventory].some((file) => file.startsWith(`${child}/`)), "unexpected directory"); await checkInventory(child); }
      else requireThat(item.isFile() && inventory.delete(child), "unexpected publication file/symlink");
    }
  }
  await checkInventory(); requireThat(inventory.size === 0, "publication inventory incomplete");
  return entries.map((entry) => {
    const path = `horizon-${entry.experiment.steps}.json`, manifest = manifests.get(path);
    requireThat(manifest && manifest.experiment.dt_seconds === entry.experiment.dt_seconds
      && ["seed", "config_hash", "world_format_version", "chamber_format", "scenario"].every((key) => manifest.identity[key] === entry.identity[key]), "archive/dense experiment identity mismatch");
    return { ...entry, dense: { index: "./data/dense-cell-chamber/index.json", index_sha256: indexSha, manifest: path, publication: "./data/dense-cell-chamber/publication.json", publication_sha256: sha(publicationBytes) } };
  });
}
