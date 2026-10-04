const integer = (value) => Number.isSafeInteger(value) && value >= 0;
const recordingPath = /^\.\/data\/[a-z0-9-]+\.json$/;
const digest = (value) => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);

export function validateDenseAttachment(dense, horizon) {
  const keys = ["index", "index_sha256", "manifest", "publication", "publication_sha256"];
  if (!dense || typeof dense !== "object" || Array.isArray(dense) || Object.keys(dense).length !== keys.length || !keys.every((key) => Object.hasOwn(dense, key))
    || dense.index !== "./data/dense-cell-chamber/index.json" || dense.publication !== "./data/dense-cell-chamber/publication.json"
    || dense.manifest !== `horizon-${horizon}.json` || !digest(dense.index_sha256) || !digest(dense.publication_sha256)) throw new Error("The catalog has an invalid dense recording attachment.");
  return dense;
}

export function validateCatalog(catalog) {
  if (catalog?.schema_version !== 1 || catalog.kind !== "recorded_experiment_catalog" || !Array.isArray(catalog.entries) || !catalog.entries.length || catalog.entries.length > 100) throw new Error("The experiment catalog has an unsupported schema.");
  const ids = new Set();
  for (const entry of catalog.entries) {
    if (typeof entry.id !== "string" || !/^[a-z0-9-]+$/.test(entry.id) || ids.has(entry.id) || typeof entry.title !== "string" || typeof entry.description !== "string" || typeof entry.recording !== "string" || !recordingPath.test(entry.recording) || typeof entry.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(entry.sha256) || !integer(entry.bytes) || entry.bytes < 1 || entry.bytes > 64 * 1024 * 1024) throw new Error("The experiment catalog has an invalid entry.");
    ids.add(entry.id);
    const identity = entry.identity, experiment = entry.experiment;
    if (!identity || typeof identity.seed !== "string" || !/^[0-9]+$/.test(identity.seed) || typeof identity.config_hash !== "string" || typeof identity.source_commit !== "string" || !/^[a-f0-9]{40}$/.test(identity.source_commit) || !integer(identity.world_format_version) || !integer(identity.chamber_format)) throw new Error("A catalog experiment has no valid identity.");
    if (!experiment || !integer(experiment.steps) || experiment.steps < 1 || experiment.steps > 1_000_000 || experiment.checked_ticks !== experiment.steps || !integer(experiment.sample_every) || experiment.sample_every < 1 || !Number.isFinite(experiment.dt_seconds) || experiment.dt_seconds <= 0 || !integer(entry.sample_count) || entry.sample_count !== Math.ceil(experiment.steps / experiment.sample_every) + 1 || entry.sample_count > 2001) throw new Error("A catalog experiment has invalid duration or sampling.");
    if (Object.hasOwn(entry, "dense")) validateDenseAttachment(entry.dense, experiment.steps);
    const preview = entry.preview;
    if (!preview || !integer(preview.tick) || preview.tick > experiment.steps || !Number.isFinite(preview.sim_time) || preview.sim_time !== preview.tick * experiment.dt_seconds || !Array.isArray(preview.cells) || preview.living_cells !== preview.cells.length) throw new Error("A catalog preview is invalid.");
    const cells = new Set();
    for (const cell of preview.cells) {
      if (typeof cell.id !== "string" || cells.has(cell.id) || typeof cell.genome_key !== "string" || !Number.isFinite(cell.mass_mol) || cell.mass_mol <= 0 || !Number.isFinite(cell.division_mass_mol) || cell.division_mass_mol <= 0) throw new Error("A catalog preview has an invalid cell inventory.");
      cells.add(cell.id);
    }
    for (const summary of [entry.initial_summary, entry.final_summary]) {
      if (!summary || !integer(summary.living_cells) || !integer(summary.births) || !integer(summary.deaths) || !integer(summary.divisions)) throw new Error("A catalog experiment has an invalid summary.");
    }
  }
  if (!ids.has(catalog.default_experiment) || !ids.has(catalog.featured_experiment)) throw new Error("The catalog has no valid default experiment.");
  return catalog;
}

export function selectExperiment(catalog, search = "") {
  validateCatalog(catalog);
  const requested = new URLSearchParams(search).get("experiment") ?? catalog.default_experiment;
  const selected = catalog.entries.find((entry) => entry.id === requested);
  if (!selected) throw new Error("This recorded experiment is not in the catalog.");
  return selected;
}
