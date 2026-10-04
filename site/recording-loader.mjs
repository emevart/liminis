import { selectExperiment, validateCatalog } from "./catalog.mjs";
import { validateRecording } from "./recording.mjs";
import { createDenseRecordingLoader } from "./dense-recording.mjs";

export async function loadRecording(search = "", fetcher = fetch, cryptoProvider = globalThis.crypto, baseUrl = globalThis.document?.baseURI ?? globalThis.location?.href) {
  const requestedMode = new URLSearchParams(search).get("recording");
  if (requestedMode !== null && !["archive", "dense"].includes(requestedMode)) throw new Error("Unknown recording mode. Use recording=archive for the sparse archive.");
  const catalogResponse = await fetcher("./data/catalog.json", { cache: "no-cache" });
  if (!catalogResponse.ok) throw new Error(`Catalog request failed (${catalogResponse.status})`);
  const catalog = validateCatalog(await catalogResponse.json());
  const entry = selectExperiment(catalog, search);
  const response = await fetcher(entry.recording, { cache: "no-cache" });
  if (!response.ok) throw new Error(`Dataset request failed (${response.status})`);
  const bytes = await response.arrayBuffer();
  if (bytes.byteLength !== entry.bytes) throw new Error("The recorded dataset is incomplete or out of date. Reload the page.");
  if (!cryptoProvider?.subtle) throw new Error("This browser cannot verify the recorded dataset. Use a current browser over HTTPS.");
  const digest = await cryptoProvider.subtle.digest("SHA-256", bytes);
  const sha256 = Array.from(new Uint8Array(digest), (value) => value.toString(16).padStart(2, "0")).join("");
  if (sha256 !== entry.sha256) throw new Error("The recorded dataset does not match the catalog. Reload the page.");
  const data = JSON.parse(new TextDecoder().decode(bytes));
  validateRecording(data);
  if (requestedMode === "dense" && !entry.dense) throw new Error("This experiment has no dense recording. Use recording=archive for its sparse archive.");
  if (!entry.dense || requestedMode === "archive") return { entry, data, mode: "archive" };
  const attachment = entry.dense;
  if (typeof attachment.index !== "string" || typeof attachment.manifest !== "string" || typeof attachment.index_sha256 !== "string" || !/^[a-f0-9]{64}$/.test(attachment.index_sha256) || typeof baseUrl !== "string") throw new Error("The dense recording attachment has no valid URL or external index digest.");
  const dense = await createDenseRecordingLoader({ indexUrl: new URL(attachment.index, baseUrl).href, indexSha256: attachment.index_sha256, manifestPath: attachment.manifest, baseUrl, fetcher, cryptoProvider });
  try {
    for (const key of ["seed", "config_hash", "world_format_version", "chamber_format"]) {
      if (dense.manifest.identity[key] !== entry.identity[key] || dense.manifest.identity[key] !== data.identity[key]) throw new Error(`Dense/archival identity differs (${key}).`);
    }
    for (const key of ["steps", "dt_seconds"]) {
      if (dense.manifest.experiment[key] !== entry.experiment[key] || dense.manifest.experiment[key] !== data.experiment[key]) throw new Error(`Dense/archival experiment differs (${key}).`);
    }
    return { entry, data, mode: "dense", dense };
  } catch (error) { dense.close(); throw error; }
}
