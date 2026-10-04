import { readFile, realpath, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { validateRecording } from "../site/recording.mjs";
import { validateCatalog } from "../site/catalog.mjs";
import { attachDenseRecordings } from "./attach_dense_catalog.mjs";

const sources = [
  ["cell-chamber-10k", "10,000 ticks", "cell-chamber-seed-42.json"],
  ["cell-chamber-100k", "100,000 ticks", "cell-chamber-seed-42-100k.json"],
  ["cell-chamber-1m", "1,000,000 ticks", "cell-chamber-seed-42-1m.json"],
];

export function catalogEntry(id, title, filename, bytes) {
  const recording = JSON.parse(bytes.toString("utf8"));
  validateRecording(recording);
  let preview = recording.frames[0], diversity = -1;
  for (const frame of recording.frames) {
    const count = new Set(frame.cells.map((cell) => cell.genome_key)).size;
    if (count > diversity || (count === diversity && frame.cells.length > preview.cells.length)) {
      preview = frame; diversity = count;
    }
  }
  return {
    id, title, description: "Well-mixed cell chamber. Seed 42; the same trajectory over a longer horizon.",
    recording: `./data/${filename}`, sha256: createHash("sha256").update(bytes).digest("hex"), bytes: bytes.length,
    identity: recording.identity, experiment: recording.experiment, sample_count: recording.frames.length,
    initial_summary: recording.frames[0].summary, final_summary: recording.frames.at(-1).summary,
    preview: { tick: preview.tick, sim_time: preview.sim_time, living_cells: preview.cells.length,
      selection: "maximum_sampled_genotype_count_then_living_count",
      cells: preview.cells.map(({ id, genome_key, mass_mol, division_mass_mol }) => ({ id, genome_key, mass_mol, division_mass_mol })) },
  };
}

export function verifySharedSamples(recordings) {
  for (let i = 0; i < recordings.length; i++) {
    for (let j = i + 1; j < recordings.length; j++) {
      const a = recordings[i], b = recordings[j];
      if (a.canonical_config !== b.canonical_config || a.identity.seed !== b.identity.seed || a.identity.world_format_version !== b.identity.world_format_version) throw new Error("Catalog recordings do not share the declared deterministic experiment.");
      const frames = new Map(a.frames.map((frame) => [frame.tick, frame]));
      for (const frame of b.frames) {
        if (frames.has(frame.tick) && JSON.stringify(frames.get(frame.tick)) !== JSON.stringify(frame)) throw new Error(`Shared recorded tick ${frame.tick} diverges between horizons.`);
      }
    }
  }
}

export async function buildCatalog() {
  const entries = [], recordings = [];
  for (const [id, title, filename] of sources) {
    const bytes = await readFile(new URL(`../site/data/${filename}`, import.meta.url));
    entries.push(catalogEntry(id, title, filename, bytes));
    recordings.push(JSON.parse(bytes.toString("utf8")));
  }
  verifySharedSamples(recordings);
  return validateCatalog({
    schema_version: 1, kind: "recorded_experiment_catalog",
    default_experiment: "cell-chamber-10k", featured_experiment: "cell-chamber-1m",
    interpretation: "Same scenario and seed, different horizons of one deterministic trajectory. These are not independent biological replicates.",
    entries: await attachDenseRecordings(entries),
  });
}

if (process.argv[1] && await realpath(resolve(process.argv[1])) === await realpath(fileURLToPath(import.meta.url))) {
  const catalog = await buildCatalog();
  const bytes = `${JSON.stringify(catalog, null, 2)}\n`;
  await writeFile(new URL("../site/data/catalog.json", import.meta.url), bytes);
  console.log(`${catalog.entries.length} verified experiments, ${Buffer.byteLength(bytes)} catalog bytes`);
}
