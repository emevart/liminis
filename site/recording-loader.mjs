import { selectExperiment, validateCatalog } from "./catalog.mjs";
import { validateRecording } from "./recording.mjs";

export async function loadRecording(search = "", fetcher = fetch, cryptoProvider = globalThis.crypto) {
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
  return { entry, data };
}
