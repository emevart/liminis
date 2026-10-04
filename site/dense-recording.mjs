// REC2 admission/transport only: copy retained observations, never run biology.
export const DENSE_LIMITS = Object.freeze({ index: 64 * 1024, manifest: 16 * 1024 * 1024, gzip: 1024 * 1024, decoded: 4 * 1024 * 1024, frames: 256, chunks: 8192, cells: 512, requestMs: 15_000 });
const ENGINE = "b2024cef08f0aa910a711a41c6a37fbc7b2ba35a", LEGACY = "43f4a2df25c356ff387b71ac1961b516975f999b";
const CANONICAL_SHA = "4e72889e4cc2857a7f259a359b8a9bda349861f8bb72d6c9dd74fe6511ca03de";
const FROZEN = { "crates/liminis-core": "bd2a0a0d55dcfff052e5573ddfc863f4d918d489", "configs/scenarios/cell-chamber.toml": "896f3415f24676e35e36431b20a0f9f304fb2259", "Cargo.toml": "4b7bef5047fafbdd10526a90de4a18952678a6a6", "Cargo.lock": "224f065c1bbf103a5cfec139a2f3313e2bf80559", "rust-toolchain.toml": "1fd6f1afaf9c17073863c2b9a9f50f90572a7edb", "crates/liminis/Cargo.toml": "d234294cf3b92e2ee1979017d5d2f212c82abace", "crates/liminis/examples/export_cell_replay.rs": "ebfed98f122b9a05b5c7d95ef70d9f9f81045c27" };
const STATIC = ["id", "parent_id", "generation", "birth_tick", "genome_key", "division_mass_mol"], DYNAMIC = ["age_s", "mass_mol", "energy_j", "mass_units", "energy_units", "starvation_s"];
const FRAME = ["tick", "sim_time", "summary", "residual", "accounting", "resources"], HEX = /^[0-9a-f]{64}$/, SHA1 = /^[0-9a-f]{40}$/;
const integerKinds = new WeakMap(); // Lexical integer/float distinction, without boxing retained numbers.
function requireThat(value, message) { if (!value) throw new Error(`Dense recording: ${message}`); }
const own = (value, key) => Object.hasOwn(value, key);
function object(value) { return value !== null && typeof value === "object" && !Array.isArray(value); }
function fields(value, required, optional = []) {
  requireThat(object(value) && required.every((key) => own(value, key)) && Object.keys(value).every((key) => required.includes(key) || optional.includes(key)), "unknown/missing fields");
}
function natural(value, maximum = Number.MAX_SAFE_INTEGER, container, key) {
  requireThat(Number.isSafeInteger(value) && !Object.is(value, -0) && value >= 0 && value <= maximum && (!container || integerKinds.get(container)?.get(String(key)) !== false), "invalid unsigned integer"); return value;
}
function number(value) { requireThat(typeof value === "number" && Number.isFinite(value), "nonfinite numeric value"); }
function id(value) {
  requireThat(typeof value === "string" && value.length <= 20 && /^(0|[1-9][0-9]*)$/.test(value) && BigInt(value) < (1n << 64n), "invalid exact u64 ID"); return value;
}
function quantity(value) {
  requireThat(typeof value === "string" && value.length <= 40 && /^(0|-?[1-9][0-9]*)$/.test(value), "invalid exact i128 string");
  const integer = BigInt(value); requireThat(integer >= -(1n << 127n) && integer < (1n << 127n), "i128 out of range");
}
export function compareDenseCellIds(left, right) { const a = BigInt(id(left)), b = BigInt(id(right)); return a < b ? -1 : a > b ? 1 : 0; }
function exact(left, right) {
  if (typeof left !== typeof right) return false;
  if (typeof left === "number") return Object.is(left, right);
  if (Array.isArray(left)) return Array.isArray(right) && left.length === right.length && left.every((item, i) => exact(item, right[i]));
  if (object(left)) return object(right) && Object.keys(left).length === Object.keys(right).length && Object.keys(left).every((key) => own(right, key) && exact(left[key], right[key]));
  return left === right;
}
function copy(value) {
  if (Array.isArray(value)) return value.map(copy);
  if (object(value)) return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, copy(item)]));
  return value; // Includes the binary64 sign of zero.
}
function freeze(value) { if (value && typeof value === "object" && !Object.isFrozen(value)) { Object.values(value).forEach(freeze); Object.freeze(value); } return value; }
function bytes(value) { requireThat(value instanceof Uint8Array, "expected bytes"); return value; }
async function sha(value, provider = globalThis.crypto) {
  requireThat(provider?.subtle, "SHA-256 needs a secure-context crypto provider");
  return Array.from(new Uint8Array(await provider.subtle.digest("SHA-256", value)), (part) => part.toString(16).padStart(2, "0")).join("");
}

// JSON.parse silently accepts duplicate keys and rounds large integer tokens.
// Parse structural tokens here, using JSON.parse only for one quoted string.
export function parseDenseJson(input) {
  const text = typeof input === "string" ? input : new TextDecoder("utf-8", { fatal: true }).decode(bytes(input));
  requireThat(text.length <= DENSE_LIMITS.manifest, "JSON byte/text cap");
  const numericToken = /-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/y;
  let offset = 0;
  function space() { while (/[\x20\t\r\n]/.test(text[offset] || "\0")) offset++; }
  function string() {
    const start = offset++; let escaped = false;
    while (offset < text.length) {
      const code = text.charCodeAt(offset++);
      if (!escaped && code === 34) return JSON.parse(text.slice(start, offset));
      if (!escaped && code === 92) escaped = true; else escaped = false;
    }
    throw new Error("Dense recording: unterminated JSON string");
  }
  function value(depth = 0) {
    requireThat(depth <= 64, "JSON nesting cap"); space(); const ch = text[offset];
    if (ch === "\"") return { result: string() };
    if (ch === "{" || ch === "[") {
      const array = ch === "[", result = array ? [] : {}; offset++; space();
      if (text[offset] === (array ? "]" : "}")) { offset++; return { result }; }
      for (;;) {
        space(); let key = String(result.length);
        if (!array) { requireThat(text[offset] === "\"", "expected JSON key"); key = string(); requireThat(!own(result, key), "duplicate JSON key"); space(); requireThat(text[offset++] === ":", "expected colon"); }
        const item = value(depth + 1); Object.defineProperty(result, key, { value: item.result, enumerable: true, configurable: true, writable: true });
        // Only float tokens that look like safe integers need a lexical marker.
        // Dynamic age/starvation rows need none; avoiding a Map per cell/delta
        // keeps the payload cache bounded without excessive metadata overhead.
        if (item.integer === false && Number.isSafeInteger(item.result) && (!array || ["1", "2", "3"].includes(key))) {
          if (!integerKinds.has(result)) integerKinds.set(result, new Map()); integerKinds.get(result).set(key, false);
        }
        space(); const delimiter = text[offset++]; if (delimiter === (array ? "]" : "}")) break; requireThat(delimiter === ",", "expected JSON delimiter");
      }
      return { result };
    }
    for (const [literal, result] of [["null", null], ["true", true], ["false", false]]) if (text.startsWith(literal, offset)) { offset += literal.length; return { result }; }
    numericToken.lastIndex = offset; const token = numericToken.exec(text);
    requireThat(token, "invalid JSON token"); offset += token[0].length;
    const result = Number(token[0]), integer = !/[.eE]/.test(token[0]);
    requireThat(Number.isFinite(result) && (!integer || Number.isSafeInteger(result)), "nonfinite/unsafe JSON number");
    return { result, integer };
  }
  const result = value().result; space(); requireThat(offset === text.length, "trailing JSON data"); return result;
}

function identity(value) {
  fields(value, ["seed", "config_hash", "world_format_version", "chamber_format", "source_commit", "scenario"]);
  requireThat(value.seed === "42" && natural(value.world_format_version, 30, value, "world_format_version") === 30 && natural(value.chamber_format, 1, value, "chamber_format") === 1 && value.source_commit === ENGINE && value.config_hash === "blake3:1802c0f129855749" && value.scenario === "cell-chamber", "wrong archived identity; world 31 cannot be relabeled");
}
function provenance(value) {
  fields(value, ["producer_commit", "producer_tree", "engine_ref", "legacy_recording_ref", "frozen_git_objects", "source_verification", "runtime_rustc", "host_os", "host_arch", "profile", "build_attestation", "build_requirement"]);
  requireThat(typeof value.producer_commit === "string" && SHA1.test(value.producer_commit) && typeof value.producer_tree === "string" && SHA1.test(value.producer_tree) && value.engine_ref === ENGINE && value.legacy_recording_ref === LEGACY && exact(value.frozen_git_objects, FROZEN) && value.profile === "release" && value.build_attestation === false && value.source_verification === "clean_HEAD_and_archived_Git_object_equality" && typeof value.runtime_rustc === "string" && value.runtime_rustc.startsWith("rustc 1.97.1 ") && ["host_os", "host_arch", "build_requirement"].every((key) => typeof value[key] === "string" && value[key].length > 0), "wrong producer/archived/compiler anchors");
}
function genomes(value) {
  requireThat(object(value) && Object.keys(value).length >= 1 && Object.keys(value).length <= 1024, "invalid genome dictionary");
  for (const entry of Object.values(value)) {
    fields(entry, ["genome", "phenotype"]); const genome = entry.genome;
    fields(genome, ["kinetics", "base_growth_per_s", "base_km_mol_m3", "division_mass_mol", "maintenance_w", "capture_fraction", "division_cost_j", "starvation_tolerance_s"]);
    fields(entry.phenotype, ["growth_per_s", "km_mol_m3"]);
    requireThat(Number.isInteger(genome.kinetics) && !Object.is(genome.kinetics, -0) && genome.kinetics >= -128 && genome.kinetics <= 127 && integerKinds.get(genome)?.get("kinetics") !== false, "invalid archived kinetics integer");
    Object.values(genome).forEach(number); Object.values(entry.phenotype).forEach(number);
  }
}
function descriptor(value, ordinal) {
  fields(value, ["path", "first_tick", "last_tick", "frames", "gzip_bytes", "decoded_bytes", "gzip_sha256", "decoded_sha256"]);
  const first = natural(value.first_tick, 1_000_000, value, "first_tick"), last = natural(value.last_tick, 1_000_000, value, "last_tick");
  const flat = `chunk-${String(first).padStart(7, "0")}-${String(last).padStart(7, "0")}.jsonl.gz`;
  const sharded = ordinal !== undefined && Number.isSafeInteger(ordinal) && ordinal >= 0 && ordinal < DENSE_LIMITS.chunks ? `chunks/${String(Math.floor(ordinal / 256)).padStart(2, "0")}/${flat}` : null;
  requireThat((value.path === flat || (sharded !== null && value.path === sharded)) && last >= first && natural(value.frames, DENSE_LIMITS.frames, value, "frames") === last - first + 1, "unsafe chunk path/range/shard ordinal");
  requireThat(natural(value.gzip_bytes, DENSE_LIMITS.gzip, value, "gzip_bytes") > 0 && natural(value.decoded_bytes, DENSE_LIMITS.decoded, value, "decoded_bytes") > 0 && typeof value.gzip_sha256 === "string" && HEX.test(value.gzip_sha256) && typeof value.decoded_sha256 === "string" && HEX.test(value.decoded_sha256), "invalid chunk lengths/digests");
}
function cap(horizon) { return horizon <= 10_000 ? 64 * 1024 * 1024 : horizon <= 100_000 ? 512 * 1024 * 1024 : 3_000_000_000; }
function validateIndex(value) {
  fields(value, ["schema_version", "kind", "identity", "provenance", "manifests", "unique_chunks", "unique_gzip_bytes", "decoded_chunk_bytes"]);
  requireThat(natural(value.schema_version, 1, value, "schema_version") === 1 && value.kind === "dense_recording_index" && Array.isArray(value.manifests) && value.manifests.length >= 1 && value.manifests.length <= 3, "unsupported index"); identity(value.identity); provenance(value.provenance);
  let prior = 0;
  for (const entry of value.manifests) {
    fields(entry, ["path", "bytes", "sha256", "first_tick", "last_tick", "frames"]); const last = natural(entry.last_tick, 1_000_000, entry, "last_tick");
    requireThat(last > prior && entry.path === `horizon-${last}.json` && natural(entry.first_tick, 0, entry, "first_tick") === 0 && natural(entry.frames, 1_000_001, entry, "frames") === last + 1 && natural(entry.bytes, DENSE_LIMITS.manifest, entry, "bytes") > 0 && typeof entry.sha256 === "string" && HEX.test(entry.sha256), "invalid manifest descriptor"); prior = last;
  }
  requireThat(natural(value.unique_chunks, DENSE_LIMITS.chunks, value, "unique_chunks") > 0 && natural(value.unique_gzip_bytes, cap(prior), value, "unique_gzip_bytes") > 0 && natural(value.decoded_chunk_bytes, DENSE_LIMITS.chunks * DENSE_LIMITS.decoded, value, "decoded_chunk_bytes") > 0, "invalid index counts");
}
export async function admitDenseRecording(indexBytes, manifestBytes, { indexSha256, manifestPath, cryptoProvider = globalThis.crypto } = {}) {
  bytes(indexBytes); bytes(manifestBytes); requireThat(indexBytes.length <= DENSE_LIMITS.index && manifestBytes.length <= DENSE_LIMITS.manifest, "metadata byte cap");
  requireThat(typeof indexSha256 === "string" && HEX.test(indexSha256) && await sha(indexBytes, cryptoProvider) === indexSha256, "externally pinned index SHA-256 mismatch");
  const index = parseDenseJson(indexBytes); validateIndex(index);
  const entry = index.manifests.find((item) => item.path === manifestPath); requireThat(entry && entry.bytes === manifestBytes.length && await sha(manifestBytes, cryptoProvider) === entry.sha256, "manifest/index integrity mismatch");
  const manifest = parseDenseJson(manifestBytes);
  fields(manifest, ["schema_version", "kind", "codec", "identity", "provenance", "model", "canonical_config", "limitations", "experiment", "bounds", "packer", "genomes", "chunks"]);
  requireThat(natural(manifest.schema_version, 1, manifest, "schema_version") === 1 && manifest.kind === "dense_recorded_cell_experiment" && manifest.codec === "dense-cell-jsonl-mask-v1+gzip" && exact(index.identity, manifest.identity) && exact(index.provenance, manifest.provenance), "unsupported manifest/identity mismatch");
  const model = manifest.model, experiment = manifest.experiment;
  fields(model, ["environment", "volume_m3", "temperature_k", "spatial_positions"]);
  requireThat(model.environment === "well_mixed" && model.spatial_positions === false && model.volume_m3 === 1e-12 && model.temperature_k === 298.15, "wrong archived model");
  requireThat(typeof manifest.canonical_config === "string" && await sha(new TextEncoder().encode(manifest.canonical_config), cryptoProvider) === CANONICAL_SHA && Array.isArray(manifest.limitations) && manifest.limitations.every((item) => typeof item === "string"), "wrong canonical bytes/limitations");
  fields(experiment, ["steps", "sample_every", "dt_seconds", "checked_ticks", "frames", "matter_residual_max", "energy_residual_max"]);
  const horizon = natural(experiment.steps, 1_000_000, experiment, "steps");
  requireThat(horizon >= 1 && horizon === entry.last_tick && natural(experiment.sample_every, 1, experiment, "sample_every") === 1 && experiment.dt_seconds === 30 && natural(experiment.checked_ticks, horizon, experiment, "checked_ticks") === horizon && natural(experiment.frames, 1_000_001, experiment, "frames") === horizon + 1 && experiment.matter_residual_max === "0" && experiment.energy_residual_max === "0", "invalid dense cadence/counts/residuals");
  const bounds = manifest.bounds;
  fields(bounds, ["frames_per_chunk", "decoded_bytes_per_chunk", "gzip_bytes_per_chunk", "max_chunks", "total_artifact_bytes_cap", "engineering_target_bytes", "provider_limits_verified"]);
  for (const [key, expected] of [["frames_per_chunk", DENSE_LIMITS.frames], ["decoded_bytes_per_chunk", DENSE_LIMITS.decoded], ["gzip_bytes_per_chunk", DENSE_LIMITS.gzip], ["max_chunks", DENSE_LIMITS.chunks], ["engineering_target_bytes", 256 * 1024 * 1024]]) requireThat(natural(bounds[key], expected, bounds, key) === expected, "unsupported bounds");
  requireThat(bounds.provider_limits_verified === false && natural(bounds.total_artifact_bytes_cap, cap(index.manifests.at(-1).last_tick), bounds, "total_artifact_bytes_cap") > 0, "unsupported full-horizon total cap");
  fields(manifest.packer, ["python", "zlib", "gzip_mtime", "gzip_filename", "gzip_os_byte"]);
  requireThat(typeof manifest.packer.python === "string" && typeof manifest.packer.zlib === "string" && natural(manifest.packer.gzip_mtime, 0, manifest.packer, "gzip_mtime") === 0 && manifest.packer.gzip_filename === "" && natural(manifest.packer.gzip_os_byte, 255, manifest.packer, "gzip_os_byte") === 255, "unsupported packer metadata");
  genomes(manifest.genomes); requireThat(Array.isArray(manifest.chunks) && manifest.chunks.length >= 1 && manifest.chunks.length <= DENSE_LIMITS.chunks, "unbounded chunks");
  let next = 0, compressed = 0, decoded = 0;
  const sharded = typeof manifest.chunks[0].path === "string" && manifest.chunks[0].path.startsWith("chunks/");
  for (const [ordinal, chunk] of manifest.chunks.entries()) { descriptor(chunk, ordinal); requireThat(chunk.path.startsWith("chunks/") === sharded, "mixed flat/sharded paths"); requireThat(chunk.first_tick === next, "chunk gap/overlap"); next = chunk.last_tick + 1; compressed += chunk.gzip_bytes; decoded += chunk.decoded_bytes; }
  requireThat(next === horizon + 1 && index.unique_chunks >= manifest.chunks.length && index.unique_gzip_bytes >= compressed && index.decoded_chunk_bytes >= decoded && index.unique_gzip_bytes + indexBytes.length + index.manifests.reduce((total, item) => total + item.bytes, 0) <= bounds.total_artifact_bytes_cap, "index/range/budget mismatch");
  if (horizon === index.manifests.at(-1).last_tick) requireThat(index.unique_chunks === manifest.chunks.length && index.unique_gzip_bytes === compressed && index.decoded_chunk_bytes === decoded, "full-horizon index counts mismatch");
  return freeze({ index, manifest, indexSha256 });
}

// Locate the end of the single DEFLATE stream without inflating it. Native
// DecompressionStream remains the only inflater; this scanner rejects trailing
// members even on implementations which accept concatenated gzip streams.
function gzipBoundary(input) {
  const header = [31, 139, 8, 0, 0, 0, 0, 0, 2, 255];
  requireThat(input.length >= 18 && input.length <= DENSE_LIMITS.gzip && header.every((value, i) => input[i] === value), "unsupported gzip header/length");
  let bit = 80, produced = 0;
  function read(count) { requireThat(bit + count <= (input.length - 8) * 8, "truncated deflate"); let value = 0; for (let i = 0; i < count; i++, bit++) value += ((input[bit >> 3] >> (bit & 7)) & 1) * 2 ** i; return value; }
  function tree(lengths) {
    const counts = Array(16).fill(0), next = Array(16).fill(0), tables = Array.from({ length: 16 }, () => new Map()); let remaining = 1;
    for (const length of lengths) { requireThat(length >= 0 && length <= 15, "invalid Huffman length"); counts[length]++; }
    for (let width = 1; width <= 15; width++) { remaining = remaining * 2 - counts[width]; requireThat(remaining >= 0, "oversubscribed Huffman tree"); next[width] = ((next[width - 1] || 0) + (width > 1 ? counts[width - 1] : 0)) * 2; }
    lengths.forEach((width, symbol) => { if (!width) return; let code = next[width]++, reversed = 0; for (let i = 0; i < width; i++) { reversed = reversed * 2 + (code & 1); code >>= 1; } tables[width].set(reversed, symbol); });
    return () => { let code = 0; for (let width = 1; width <= 15; width++) { code += read(1) * 2 ** (width - 1); if (tables[width].has(code)) return tables[width].get(code); } throw new Error("Dense recording: invalid Huffman symbol"); };
  }
  const lengthBase = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258], lengthExtra = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
  const distanceBase = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577], distanceExtra = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
  let final, fixedLiteral, fixedDistance;
  do {
    final = read(1); const type = read(2); requireThat(type !== 3, "reserved deflate block");
    if (type === 0) {
      bit = Math.ceil(bit / 8) * 8; const count = read(16), inverse = read(16); requireThat((count ^ inverse) === 65535, "invalid stored block");
      requireThat(bit + count * 8 <= (input.length - 8) * 8, "truncated stored block"); bit += count * 8; produced += count;
    } else {
      let literal, distance;
      if (type === 1) { fixedLiteral ||= tree(Array.from({ length: 288 }, (_, i) => i < 144 ? 8 : i < 256 ? 9 : i < 280 ? 7 : 8)); fixedDistance ||= tree(Array(32).fill(5)); literal = fixedLiteral; distance = fixedDistance; }
      else {
        const literals = read(5) + 257, distances = read(5) + 1, count = read(4) + 4;
        // RFC1951 code-length order (written separately to keep indices explicit).
        const codeOrder = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15], codeLengths = Array(19).fill(0);
        for (let i = 0; i < count; i++) codeLengths[codeOrder[i]] = read(3);
        const code = tree(codeLengths), lengths = [];
        while (lengths.length < literals + distances) {
          const symbol = code();
          if (symbol < 16) lengths.push(symbol);
          else { requireThat(symbol !== 16 || lengths.length > 0, "invalid length repeat"); const repeat = symbol === 16 ? read(2) + 3 : symbol === 17 ? read(3) + 3 : read(7) + 11, item = symbol === 16 ? lengths.at(-1) : 0; requireThat(lengths.length + repeat <= literals + distances, "Huffman repeat overflow"); for (let i = 0; i < repeat; i++) lengths.push(item); }
        }
        requireThat(literals <= 286 && lengths[256] > 0, "missing end-of-block symbol"); literal = tree(lengths.slice(0, literals)); distance = tree(lengths.slice(literals));
      }
      for (;;) {
        const symbol = literal(); if (symbol === 256) break;
        if (symbol < 256) produced++;
        else { requireThat(symbol >= 257 && symbol <= 285, "invalid length symbol"); const index = symbol - 257, length = lengthBase[index] + read(lengthExtra[index]), d = distance(); requireThat(d < 30, "invalid distance symbol"); const back = distanceBase[d] + read(distanceExtra[d]); requireThat(back <= produced, "distance precedes payload"); produced += length; }
        requireThat(produced <= DENSE_LIMITS.decoded, "inflated byte cap exceeded");
      }
    }
    requireThat(produced <= DENSE_LIMITS.decoded, "inflated byte cap exceeded");
  } while (!final);
  requireThat(Math.ceil(bit / 8) === input.length - 8, "trailing bytes or multiple gzip members");
  const trailer = new DataView(input.buffer, input.byteOffset + input.length - 8, 8); requireThat(trailer.getUint32(4, true) === produced, "gzip ISIZE mismatch"); return produced;
}
function abortError() { return new DOMException("Dense recording request superseded or aborted", "AbortError"); }
function interrupted(signal) { if (signal?.aborted) throw signal.reason || abortError(); }
async function withSignal(operation, signal) {
  interrupted(signal); if (!signal) return operation(); let listener;
  try {
    const canceled = new Promise((_, reject) => { listener = () => reject(signal.reason || abortError()); signal.addEventListener("abort", listener, { once: true }); });
    // Attach race handlers before invoking the thunk. An already-aborted read
    // is never created; a synchronous abort/rejection inside fetch stays caught.
    const pending = Promise.resolve().then(() => { interrupted(signal); return operation(); });
    return await Promise.race([pending, canceled]);
  }
  finally { signal.removeEventListener("abort", listener); }
}
async function collect(stream, maximum, signal) {
  requireThat(stream?.getReader, "bounded response body is required"); const reader = stream.getReader(), parts = []; let length = 0;
  try {
    for (;;) { const part = await withSignal(() => reader.read(), signal); if (part.done) break; requireThat(part.value instanceof Uint8Array && part.value.byteLength <= maximum - length, "response/inflated byte cap exceeded"); parts.push(part.value); length += part.value.byteLength; }
    const result = new Uint8Array(length); let offset = 0; for (const part of parts) { result.set(part, offset); offset += part.byteLength; } return result;
  } catch (error) { void reader.cancel(error).catch(() => {}); throw error; }
  finally { reader.releaseLock(); }
}

function reconstruct(definition, values) {
  requireThat(Array.isArray(definition) && definition.length === STATIC.length && Array.isArray(values) && values.length === DYNAMIC.length, "invalid cell row");
  natural(definition[2], Number.MAX_SAFE_INTEGER, definition, 2); natural(definition[3], 1_000_000, definition, 3);
  return Object.fromEntries([...STATIC.map((name, i) => [name, definition[i]]), ...DYNAMIC.map((name, i) => [name, values[i]])]);
}
function frame(value, definitions) {
  fields(value, [...FRAME, "cells"]); const tick = natural(value.tick, 1_000_000, value, "tick"); number(value.sim_time);
  requireThat(value.sim_time === tick * 30 && exact(value.residual, tick === 0 ? null : { matter: "0", energy: "0" }), "wrong model time/unchecked boundary"); genomes(definitions);
  requireThat(Array.isArray(value.cells) && value.cells.length <= DENSE_LIMITS.cells, "unbounded cell inventory"); const ids = new Set();
  for (const cell of value.cells) {
    fields(cell, [...STATIC, ...DYNAMIC]); id(cell.id); requireThat(!ids.has(cell.id), "duplicate cell ID"); ids.add(cell.id); if (cell.parent_id !== null) id(cell.parent_id);
    natural(cell.generation); requireThat(natural(cell.birth_tick) <= tick && typeof cell.genome_key === "string" && own(definitions, cell.genome_key), "unknown birth/genotype");
    ["division_mass_mol", "age_s", "mass_mol", "energy_j", "starvation_s"].forEach((key) => number(cell[key])); quantity(cell.mass_units); quantity(cell.energy_units);
  }
  const summary = value.summary; fields(summary, ["living_cells", "births", "deaths", "divisions", "generation_max", "total_biomass_mol", "total_cell_energy_j"]);
  requireThat(natural(summary.living_cells, DENSE_LIMITS.cells, summary, "living_cells") === ids.size, "inventory count mismatch");
  ["births", "deaths", "divisions", "generation_max"].forEach((key) => natural(summary[key], Number.MAX_SAFE_INTEGER, summary, key)); number(summary.total_biomass_mol); number(summary.total_cell_energy_j);
  const accounting = value.accounting; fields(accounting, ["matter_ids", "free_matter_units", "bath_heat_units", "medium_matter_units", "medium_energy_units", "growth_extent_units", "death_mass_units"]);
  const registry = accounting.matter_ids; requireThat(Array.isArray(registry) && registry.length > 0 && registry.every((item) => typeof item === "string") && new Set(registry).size === registry.length, "invalid matter registry");
  for (const key of ["free_matter_units", "medium_matter_units"]) { requireThat(Array.isArray(accounting[key]) && accounting[key].length === registry.length, "invalid matter counters"); accounting[key].forEach(quantity); }
  ["bath_heat_units", "medium_energy_units", "growth_extent_units", "death_mass_units"].forEach((key) => quantity(accounting[key]));
  requireThat(Array.isArray(value.resources), "invalid resources"); const resourceIds = new Set();
  for (const resource of value.resources) { fields(resource, ["id", "amount_mol", "concentration"]); requireThat(registry.includes(resource.id) && !resourceIds.has(resource.id), "invalid resource registry"); resourceIds.add(resource.id); number(resource.amount_mol); number(resource.concentration); }
}
function start(record) {
  fields(record, ["type", "schema_version", "frame", "definitions", "values", "genomes"]);
  requireThat(record.type === "keyframe" && natural(record.schema_version, 1, record, "schema_version") === 1 && Array.isArray(record.definitions) && Array.isArray(record.values) && record.definitions.length === record.values.length && record.definitions.length <= DENSE_LIMITS.cells, "missing independent keyframe"); fields(record.frame, FRAME); natural(record.frame.tick, 1_000_000, record.frame, "tick");
  const state = { ...record.frame, cells: record.definitions.map((definition, i) => reconstruct(definition, record.values[i])) }; frame(state, record.genomes);
  return { frame: state, genomes: record.genomes, seen: new Set(state.cells.map((cell) => cell.id)) };
}
function patch(previous, record) {
  fields(record, ["type", "set", "born", "removed", "changed", "genomes"], ["order"]);
  requireThat(record.type === "delta" && object(record.set) && Object.keys(record.set).every((key) => FRAME.includes(key)), "invalid frame patch");
  if (own(record.set, "tick")) natural(record.set.tick, 1_000_000, record.set, "tick");
  requireThat(object(record.genomes) && Object.keys(record.genomes).every((key) => !own(previous.genomes, key)), "redefined genotype");
  const definitions = { ...previous.genomes, ...record.genomes }, cells = new Map(previous.frame.cells.map((cell) => [cell.id, { ...cell }]));
  requireThat(Array.isArray(record.changed) && record.changed.length <= DENSE_LIMITS.cells && Array.isArray(record.born) && record.born.length <= DENSE_LIMITS.cells && Array.isArray(record.removed) && record.removed.length <= DENSE_LIMITS.cells && new Set(record.removed).size === record.removed.length, "unbounded/duplicate changes or removals");
  for (const removed of record.removed) { id(removed); requireThat(cells.delete(removed), "unknown removal"); }
  const touched = new Set();
  for (const row of record.changed) {
    requireThat(Array.isArray(row) && row.length === 3, "invalid change row"); const [ident, mask, values] = row; id(ident); natural(mask, 63, row, 1);
    const keys = DYNAMIC.filter((_, i) => mask & (1 << i)); requireThat(mask > 0 && cells.has(ident) && !touched.has(ident) && Array.isArray(values) && values.length === keys.length, "invalid ID/mask/change arity"); touched.add(ident); keys.forEach((name, i) => { cells.get(ident)[name] = values[i]; });
  }
  for (const row of record.born) { requireThat(Array.isArray(row) && row.length === 2, "invalid birth row"); const cell = reconstruct(...row); id(cell.id); requireThat(!previous.seen.has(cell.id), "reused cell ID"); previous.seen.add(cell.id); cells.set(cell.id, cell); }
  const order = own(record, "order") ? record.order : previous.frame.cells.map((cell) => cell.id);
  requireThat(Array.isArray(order) && order.length === cells.size && new Set(order).size === order.length && order.every((ident) => typeof ident === "string" && cells.has(ident)), "incomplete/repeated cell order");
  const next = { ...previous.frame, ...record.set, cells: order.map((ident) => cells.get(ident)) }; frame(next, definitions);
  requireThat(next.tick === previous.frame.tick + 1, "decoded tick gap"); return { frame: next, genomes: definitions, seen: previous.seen };
}
export async function decodeDenseChunk(input, chunk, manifestGenomes, { cryptoProvider = globalThis.crypto, signal, chunkOrdinal } = {}) {
  descriptor(chunk, chunkOrdinal); genomes(manifestGenomes); bytes(input); interrupted(signal);
  requireThat(input.length === chunk.gzip_bytes && await sha(input, cryptoProvider) === chunk.gzip_sha256, "gzip integrity mismatch"); interrupted(signal);
  const predicted = gzipBoundary(input); requireThat(predicted === chunk.decoded_bytes, "decoded length mismatch");
  requireThat(typeof DecompressionStream === "function", "native gzip DecompressionStream is required");
  const deadline = AbortSignal.timeout(DENSE_LIMITS.requestMs), combined = signal ? AbortSignal.any([signal, deadline]) : deadline;
  const raw = await collect(new Blob([input]).stream().pipeThrough(new DecompressionStream("gzip")), DENSE_LIMITS.decoded, combined);
  requireThat(raw.length === chunk.decoded_bytes && await sha(raw, cryptoProvider) === chunk.decoded_sha256 && raw.at(-1) === 10, "decoded integrity/incomplete final record"); interrupted(signal);
  const lines = new TextDecoder("utf-8", { fatal: true }).decode(raw).split("\n"); lines.pop(); requireThat(lines.length === chunk.frames, "chunk record count mismatch");
  const records = lines.map(parseDenseJson); let current;
  // Validate the complete payload before exposing even its first frame.
  for (let i = 0; i < records.length; i++) {
    interrupted(signal); current = i ? patch(current, records[i]) : start(records[i]); requireThat(current.frame.tick === chunk.first_tick + i, "decoded tick/range mismatch");
    requireThat(object(manifestGenomes) && Object.entries(current.genomes).every(([key, definition]) => own(manifestGenomes, key) && exact(definition, manifestGenomes[key])), "chunk/manifest genotype mismatch");
  }
  freeze(records);
  return Object.freeze({ firstTick: chunk.first_tick, lastTick: chunk.last_tick, payloadBytes: raw.length, records: records.length, retainedFrames: 0,
    frameAt(tick) {
      natural(tick, chunk.last_tick); requireThat(tick >= chunk.first_tick, "seek outside chunk"); let selected = start(records[0]);
      for (let i = 1; i <= tick - chunk.first_tick; i++) selected = patch(selected, records[i]);
      return freeze({ frame: copy(selected.frame), genomes: copy(selected.genomes) });
    },
  });
}

// External index SHA is required. Same-origin GETs only; ETags are deliberately
// not treated as provenance. Observer integration/selection is a separate owner.
export async function createDenseRecordingLoader({ indexUrl, indexSha256, manifestPath, baseUrl = globalThis.location?.href, fetcher = globalThis.fetch, cryptoProvider = globalThis.crypto, signal: lifetimeSignal, prefetch = true } = {}) {
  requireThat(typeof baseUrl === "string" && typeof fetcher === "function", "same-origin base and fetch are required");
  const base = new URL(baseUrl), url = new URL(indexUrl, base); requireThat(["https:", "http:"].includes(base.protocol) && url.origin === base.origin && url.pathname.endsWith("/index.json") && !url.search && !url.hash && !url.username && !url.password, "unsafe index URL");
  async function get(target, maximum, expected, signal) {
    requireThat(target.origin === base.origin && !target.username && !target.password && !target.search && !target.hash, "cross-origin or unsafe asset URL");
    const controller = new AbortController(), deadline = AbortSignal.timeout(DENSE_LIMITS.requestMs), combined = AbortSignal.any([controller.signal, deadline, ...(signal ? [signal] : [])]); interrupted(combined);
    try {
      const response = await withSignal(() => fetcher(target.href, { method: "GET", mode: "same-origin", credentials: "omit", redirect: "error", cache: "no-store", signal: combined }), combined);
      requireThat(response.status === 200 && (!response.url || response.url === target.href), "asset response status/URL mismatch");
      const declared = response.headers?.get("content-length");
      if (declared !== null && declared !== undefined) requireThat(/^(0|[1-9][0-9]*)$/.test(declared) && Number(declared) <= maximum && (expected === undefined || Number(declared) === expected), "asset Content-Length mismatch/cap");
      const result = await collect(response.body, expected ?? maximum, combined); requireThat(expected === undefined || result.length === expected, "asset byte count mismatch"); return result;
    } finally { controller.abort(); }
  }
  const indexBytes = await get(url, DENSE_LIMITS.index, undefined, lifetimeSignal);
  requireThat(typeof indexSha256 === "string" && HEX.test(indexSha256) && await sha(indexBytes, cryptoProvider) === indexSha256, "externally pinned index SHA-256 mismatch");
  const initialIndex = parseDenseJson(indexBytes); validateIndex(initialIndex); const manifestEntry = initialIndex.manifests.find((entry) => entry.path === manifestPath); requireThat(manifestEntry, "manifest must be index-allowlisted");
  const manifestBytes = await get(new URL(manifestEntry.path, url), DENSE_LIMITS.manifest, manifestEntry.bytes, lifetimeSignal);
  const admitted = await admitDenseRecording(indexBytes, manifestBytes, { indexSha256, manifestPath, cryptoProvider }); interrupted(lifetimeSignal);
  const entries = new Map(); let serial = 0, activeSeek, closed = false, current = null, failure = null, maximumEntries = 0;
  const live = (entry) => !closed && entries.get(entry.index) === entry && !entry.controller.signal.aborted;
  function evict(index) { const entry = entries.get(index); if (entry) { entries.delete(index); entry.controller.abort(abortError()); } }
  function obtain(index) {
    if (entries.has(index)) return entries.get(index);
    const chunk = admitted.manifest.chunks[index], controller = new AbortController(), entry = { index, controller, payload: null, error: null };
    entries.set(index, entry); maximumEntries = Math.max(maximumEntries, entries.size); requireThat(entries.size <= 2, "chunk cache cap");
    const combined = lifetimeSignal ? AbortSignal.any([lifetimeSignal, controller.signal]) : controller.signal;
    entry.promise = (async () => {
      const compressed = await get(new URL(chunk.path, url), DENSE_LIMITS.gzip, chunk.gzip_bytes, combined);
      const payload = await decodeDenseChunk(compressed, chunk, admitted.manifest.genomes, { cryptoProvider, signal: combined, chunkOrdinal: index });
      requireThat(live(entry), "stale chunk must not commit"); entry.payload = payload; return payload;
    })().catch((error) => { entry.error = error; if (live(entry)) { failure = error; current = null; } throw error; });
    void entry.promise.catch(() => {}); return entry;
  }
  function close() { if (closed) return; closed = true; serial++; activeSeek?.abort(abortError()); for (const index of [...entries.keys()]) evict(index); current = null; lifetimeSignal?.removeEventListener("abort", close); }
  lifetimeSignal?.addEventListener("abort", close, { once: true });
  return Object.freeze({ index: admitted.index, manifest: admitted.manifest, indexSha256,
    get current() { return current; },
    get stats() { return Object.freeze({ retainedChunks: entries.size, cachedChunks: [...entries.values()].filter((entry) => entry.payload).length, loadingChunks: [...entries.values()].filter((entry) => !entry.payload && !entry.error).length, maxRetainedChunks: maximumEntries, retainedFrames: current ? 1 : 0, currentTick: current?.frame.tick ?? null, closed, failed: !!failure }); },
    async seek(tick, { signal } = {}) {
      requireThat(!closed, "loader is closed"); if (failure) throw failure; interrupted(lifetimeSignal); interrupted(signal); natural(tick, admitted.manifest.experiment.steps);
      const requested = ++serial; activeSeek?.abort(abortError()); activeSeek = new AbortController(); current = null;
      let low = 0, high = admitted.manifest.chunks.length - 1;
      while (low < high) { const middle = Math.floor((low + high) / 2); if (admitted.manifest.chunks[middle].last_tick < tick) low = middle + 1; else high = middle; }
      const index = low; for (const old of [...entries.keys()]) if (old !== index && (!prefetch || old !== index + 1)) evict(old);
      const entry = obtain(index), signals = [activeSeek.signal, ...(lifetimeSignal ? [lifetimeSignal] : []), ...(signal ? [signal] : [])], combined = AbortSignal.any(signals);
      try {
        const payload = await withSignal(() => entry.promise, combined); interrupted(combined); requireThat(requested === serial && !closed && !failure, "stale seek must not emit");
        current = payload.frameAt(tick);
        if (prefetch && index + 1 < admitted.manifest.chunks.length) obtain(index + 1);
        return current;
      } catch (error) {
        if (requested === serial) { current = null; if (combined.aborted) { evict(index); for (const old of [...entries.keys()]) evict(old); } }
        throw error;
      }
    }, close,
  });
}
