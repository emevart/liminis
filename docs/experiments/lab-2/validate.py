#!/usr/bin/env python3
"""Validate/extract immutable LAB-2 observations using Python stdlib.

This is structural validation, not simulation, independent per-tick numerical
verification, BLAKE3 recomputation, or build attestation. Raw input integrity is
independently hashed with SHA-256. Comparisons use actual same-tick samples;
there is no interpolation, carry-forward, or significance claim.
"""

from __future__ import annotations

import argparse
import copy
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import re
import sys
import tempfile
import tomllib

SOURCE_COMMIT = "21cbe90732128edee61963544e414d29a7420577"
SEEDS = ("1", "7", "42", "2026")
CONDITIONS = ("baseline", "mutation_off", "starvation", "oxygen_low", "exchange_half", "founder_k2")
LANDMARKS = (100, 500, 1000, 2000, 5000, 10000, 20000)
STEPS, SAMPLE_EVERY, DT = 20000, 100, 30.0
MANIFEST_LIMIT, RESULT_LIMIT = 2 * 1024 * 1024, 16 * 1024 * 1024
INTEGER = re.compile(r"(?:0|-[1-9][0-9]*|[1-9][0-9]*)\Z")
ID = re.compile(r"[A-Za-z0-9_-]{1,64}\Z")
SPECIES = ("WATER", "FOOD", "O2", "CO2", "DET", "BIO")
METRIC_UNITS = {
    "living_cells": "cells", "structural_mass_mol": "mol structural biomass",
    "internal_energy_j": "J", "bath_heat_j": "J cumulative bath heat",
    "observed_allele_richness": "observed kinetics alleles", "max_generation_observed": "generation",
    "daughters_born": "daughters", "fissions": "fissions", "deaths": "deaths",
    "growth_extent": "derived integer extent units", "structural_mass_units": "derived structural-mass units",
    "internal_energy_units": "derived energy units", "bath_heat_units": "derived energy units",
    "medium_energy_units": "derived signed energy units",
    "food_free_mol": "mol FOOD", "oxygen_free_mol": "mol O2", "detritus_free_mol": "mol DET",
    "food_free_units": "derived FOOD units", "oxygen_free_units": "derived O2 units",
    "detritus_free_units": "derived DET units", "death_mass_units": "derived structural-mass units",
}
METRIC_UNITS.update({f"medium_matter_{species}_units": f"derived signed {species} units" for species in SPECIES})
EXACT_STRING_METRICS = {
    "daughters_born", "fissions", "deaths", "growth_extent", "structural_mass_units",
    "internal_energy_units", "bath_heat_units", "medium_energy_units",
    "food_free_units", "oxygen_free_units", "detritus_free_units", "death_mass_units",
}
EXACT_STRING_METRICS.update(f"medium_matter_{species}_units" for species in SPECIES)


class ValidationError(ValueError):
    """A declared input or observation is structurally inconsistent."""


def require(test, message):
    if not test:
        raise ValidationError(message)


def fields(value, required, context, optional=()):
    require(isinstance(value, dict) and set(required) <= set(value)
            and set(value) <= set(required) | set(optional), f"{context}: unknown/missing fields")


def natural(value, context, maximum=None):
    require(type(value) is int and value >= 0, f"{context}: expected nonnegative JSON integer")
    require(maximum is None or value <= maximum, f"{context}: exceeds limit")
    return value


def integer_string(value, context, signed=False):
    require(isinstance(value, str) and INTEGER.fullmatch(value), f"{context}: expected decimal integer string")
    number = int(value)
    require(signed or number >= 0, f"{context}: negative amount/count")
    require(-(1 << 127) <= number < (1 << 127), f"{context}: exceeds i128")
    return number


def close(actual, expected, context):
    require(type(actual) in (int, float) and math.isfinite(actual), f"{context}: expected finite number")
    require(math.isclose(actual, expected, rel_tol=1e-12, abs_tol=1e-30), f"{context}: physical/integer value mismatch")


def digest(value, context, short=False):
    size = 16 if short else 64
    require(isinstance(value, str) and re.fullmatch(r"blake3:[0-9a-f]{" + str(size) + r"}", value),
            f"{context}: malformed engine-produced BLAKE3 evidence")


def scenario_for(baseline, condition):
    value = copy.deepcopy(baseline)
    if condition == "mutation_off":
        value["genome"]["mutation_probability"] = 0.0
    elif condition == "starvation":
        for section in ("initial", "medium"):
            value[section]["concentration"]["FOOD"] = 0.0
    elif condition == "oxygen_low":
        for section in ("initial", "medium"):
            value[section]["concentration"]["O2"] = 0.05
    elif condition == "exchange_half":
        value["chamber"]["medium_exchange_per_s"] = 1.0e-6
    elif condition == "founder_k2":
        value["founder"]["genome"]["kinetics"] = 2
    return value


def validate_manifest(manifest, baseline):
    require(isinstance(manifest, dict) and set(manifest) == {"schema_version", "batch_id", "runs"},
            "manifest: unknown/missing fields")
    require(manifest["schema_version"] == 1 and isinstance(manifest["batch_id"], str)
            and ID.fullmatch(manifest["batch_id"]), "manifest: schema/batch_id mismatch")
    require(baseline["dt"] == DT and baseline["chamber"]["max_cells"] == 512
            and baseline["genome"]["mutation_probability"] == 0.02
            and baseline["founder"]["genome"]["kinetics"] == 0
            and baseline["chamber"]["medium_exchange_per_s"] == 2.0e-6
            and all(baseline[s]["concentration"]["FOOD"] == 10.0
                    and baseline[s]["concentration"]["O2"] == 2.0 for s in ("initial", "medium")),
            "baseline: preregistered control values changed")
    runs = manifest["runs"]
    require(isinstance(runs, list) and len(runs) == 24, "manifest: expected 24 runs")
    scenarios, ids = [], set()
    for index, (run, pair) in enumerate(zip(runs, ((s, c) for s in SEEDS for c in CONDITIONS))):
        context = f"manifest run {index}"
        require(isinstance(run, dict) and set(run) == {
            "run_id", "condition", "seed", "steps", "sample_every", "scenario_toml"
        }, f"{context}: unknown/missing fields")
        require((run["seed"], run["condition"]) == pair, f"{context}: seed-major pairing/order mismatch")
        require(isinstance(run["run_id"], str) and ID.fullmatch(run["run_id"]) and run["run_id"] not in ids,
                f"{context}: invalid/duplicate run_id")
        ids.add(run["run_id"])
        require(type(run["steps"]) is int and run["steps"] == STEPS
                and type(run["sample_every"]) is int and run["sample_every"] == SAMPLE_EVERY,
                f"{context}: horizon/sampling changed")
        require(isinstance(run["scenario_toml"], str) and len(run["scenario_toml"].encode()) <= 100 * 1024,
                f"{context}: invalid TOML size")
        scenario = tomllib.loads(run["scenario_toml"])
        require(scenario == scenario_for(baseline, run["condition"]),
                f"{context}: parameter leakage outside declared condition")
        scenarios.append(scenario)
    return scenarios


def round_positive(value):
    floor = math.floor(value)
    return floor + (value - floor >= 0.5)


def check_identity(identity, scenario, context):
    fields(identity, ("biomass_substance", "canonical_digest", "chamber_format", "config_hash",
                     "declared_max_cells", "derived", "dt_seconds", "energy_units_per_joule", "matter_scales",
                     "model", "scenario", "spatial_positions", "temperature_k", "units", "volume_m3",
                     "world_format_version"), context + ".identity")
    require(identity["world_format_version"] == 30 and identity["chamber_format"] == 1,
            f"{context}: engine/chamber version mismatch")
    require(identity["scenario"] == scenario["name"] and identity["dt_seconds"] == DT
            and identity["volume_m3"] == scenario["chamber"]["volume_m3"]
            and identity["temperature_k"] == scenario["chamber"]["temperature_k"]
            and identity["declared_max_cells"] == 512, f"{context}: physical identity mismatch")
    require(identity["model"] == "well_mixed_individual_cells" and identity["spatial_positions"] is False,
            f"{context}: unsupported model")
    require(identity["units"] == {"concentration": "mol/m^3", "structural_mass": "mol structural biomass",
                                  "energy": "J", "time": "s", "temperature": "K"}
            and identity["biomass_substance"] == scenario["growth"]["biomass_substance"],
            f"{context}: unit/biomass mismatch")
    scales = identity["matter_scales"]
    require(isinstance(scales, list) and [x["id"] for x in scales] == [x["id"] for x in scenario["substance"]],
            f"{context}: storage registry mismatch")
    for item in scales:
        fields(item, ("id", "units_per_mol"), context + ".scale")
        scale = integer_string(item["units_per_mol"], context + ".scale")
        require(scale > 0 and scale & (scale - 1) == 0, f"{context}: invalid power-of-two scale")
    scale = integer_string(identity["energy_units_per_joule"], context + ".energy scale")
    require(scale > 0 and scale & (scale - 1) == 0, f"{context}: invalid energy scale")


def check_summary(summary, scenario, identity, context):
    fields(summary, ("accounting", "allele_first_seen_tick", "allele_histogram", "bath_heat_j", "bath_heat_units",
                     "internal_energy_j", "internal_energy_units", "ledger", "lifecycle", "living_cells", "matter",
                     "max_generation_observed", "observed_allele_richness", "peak_cells", "structural_mass_mol",
                     "structural_mass_units", "tick", "time_seconds"), context)
    tick = natural(summary["tick"], context + ".tick", STEPS)
    close(summary["time_seconds"], tick * DT, context + ".time")
    living = natural(summary["living_cells"], context + ".living", 512)
    peak = natural(summary["peak_cells"], context + ".peak", 512)
    require(peak >= max(living, scenario["founder"]["count"]), f"{context}: invalid historical peak")
    generation = natural(summary["max_generation_observed"], context + ".generation", tick)
    hist, seen = summary["allele_histogram"], summary["allele_first_seen_tick"]
    require(isinstance(hist, dict) and isinstance(seen, dict), f"{context}: invalid allele maps")
    bounds = range(scenario["genome"]["kinetics_min"], scenario["genome"]["kinetics_max"] + 1)
    for allele, count in hist.items():
        require(INTEGER.fullmatch(allele) and int(allele) in bounds, f"{context}: allele outside bounds")
        require(natural(count, context + ".histogram", 512) > 0, f"{context}: empty histogram bin")
    require(sum(hist.values()) == living, f"{context}: histogram sum mismatch")
    for allele, first in seen.items():
        require(INTEGER.fullmatch(allele) and int(allele) in bounds, f"{context}: observed allele outside bounds")
        natural(first, context + ".first seen", tick)
    require(set(hist) <= set(seen) and type(summary["observed_allele_richness"]) is int
            and summary["observed_allele_richness"] == len(seen), f"{context}: richness mismatch")
    founder_allele = str(scenario["founder"]["genome"]["kinetics"])
    require(seen.get(founder_allele) == 0, f"{context}: missing founder allele")
    if scenario["genome"]["mutation_probability"] == 0:
        require(set(seen) == {founder_allele}, f"{context}: mutation-off allele changed")
    life = summary["lifecycle"]
    fields(life, ("deaths", "daughters_born", "fissions", "founders", "living_identity_residual"), context + ".lifecycle")
    founders = integer_string(life["founders"], context + ".founders")
    births = integer_string(life["daughters_born"], context + ".births")
    fissions = integer_string(life["fissions"], context + ".fissions")
    deaths = integer_string(life["deaths"], context + ".deaths")
    require(founders == scenario["founder"]["count"] and births == 2 * fissions
            and living == founders + births - fissions - deaths, f"{context}: lifecycle identity does not close")
    require(integer_string(life["living_identity_residual"], context + ".life residual", True) == 0,
            f"{context}: nonzero lifecycle residual")
    ledger = summary["ledger"]
    fields(ledger, ("checked_ticks", "energy_residual_last_checked", "matter_residual_last_checked"), context + ".ledger")
    require(type(ledger["checked_ticks"]) is int and ledger["checked_ticks"] == tick,
            f"{context}: checked-tick evidence mismatch")
    ids = [x["id"] for x in identity["matter_scales"]]
    expected_matter = None if tick == 0 else ["0"] * len(ids)
    expected_energy = None if tick == 0 else "0"
    require(ledger["matter_residual_last_checked"] == expected_matter
            and ledger["energy_residual_last_checked"] == expected_energy, f"{context}: invalid last-checked ledger evidence")
    scales = {x["id"]: int(x["units_per_mol"]) for x in identity["matter_scales"]}
    energy_scale = int(identity["energy_units_per_joule"])
    mass = integer_string(summary["structural_mass_units"], context + ".mass")
    energy = integer_string(summary["internal_energy_units"], context + ".energy")
    heat = integer_string(summary["bath_heat_units"], context + ".heat")
    require((mass == energy == 0) if living == 0 else mass >= living, f"{context}: structural units/population mismatch")
    close(summary["structural_mass_mol"], float(mass) / scales[identity["biomass_substance"]], context + ".mass mol")
    close(summary["internal_energy_j"], float(energy) / energy_scale, context + ".energy J")
    close(summary["bath_heat_j"], float(heat) / energy_scale, context + ".heat J")
    matter = summary["matter"]
    require(isinstance(matter, list) and [x["id"] for x in matter] == ids, f"{context}: matter registry mismatch")
    for pool in matter:
        fields(pool, ("id", "free_units", "free_mol"), context + ".pool")
        amount = integer_string(pool["free_units"], context + ".free " + pool["id"])
        close(pool["free_mol"], float(amount) / scales[pool["id"]], context + ".mol " + pool["id"])
    accounting = summary["accounting"]
    fields(accounting, ("growth_extent", "death_mass_units", "medium_energy_units", "medium_matter_units"), context + ".accounting")
    for key in ("growth_extent", "death_mass_units", "medium_energy_units"):
        integer_string(accounting[key], context + "." + key, key == "medium_energy_units")
    require(isinstance(accounting["medium_matter_units"], list) and len(accounting["medium_matter_units"]) == len(ids),
            f"{context}: medium counter registry mismatch")
    for value in accounting["medium_matter_units"]:
        integer_string(value, context + ".medium matter", True)
    if scenario["initial"]["concentration"]["FOOD"] == scenario["medium"]["concentration"]["FOOD"] == 0:
        food = next(pool for pool in matter if pool["id"] == "FOOD")
        require(int(accounting["growth_extent"]) == 0 and int(food["free_units"]) == 0
                and births == fissions == generation == 0, f"{context}: starvation control claims growth/fission")
    if tick == 0:
        require(living == peak == founders and generation == births == deaths == fissions == heat == 0
                and hist == {founder_allele: founders} and seen == {founder_allele: 0}, f"{context}: invalid genesis")
        require(mass == founders * round_positive(scenario["founder"]["structural_mass_mol"] * float(scales[identity["biomass_substance"]]))
                and energy == founders * round_positive(scenario["founder"]["internal_energy_j"] * float(energy_scale)),
                f"{context}: genesis founder amounts changed")
        for pool in matter:
            amount = scenario["initial"]["concentration"].get(pool["id"], 0.0) * scenario["chamber"]["volume_m3"] * float(scales[pool["id"]])
            require(int(pool["free_units"]) == round_positive(amount), f"{context}: genesis medium changed")
        require(all(int(x) == 0 for x in accounting["medium_matter_units"])
                and all(int(accounting[x]) == 0 for x in ("growth_extent", "death_mass_units", "medium_energy_units")),
                f"{context}: genesis counters changed")


def check_run(row, request, scenario):
    context = request["run_id"]
    fields(row, ("attempt_residuals", "attempted_cell_ticks", "attempted_tick", "canonical_config", "checked_ticks",
                 "committed_cell_ticks", "condition", "error", "identity", "input_toml_digest", "requested_steps",
                 "run_id", "sample_every", "samples", "samples_truncated", "seed", "status", "stop_reason", "summary"),
           context, ("canonical_config_omitted", "final_state_digest", "tick_report_digest_encoding",
                     "tick_reports_digest", "tick_reports_hashed"))
    for actual, declared in (("run_id", "run_id"), ("condition", "condition"), ("seed", "seed"),
                             ("requested_steps", "steps"), ("sample_every", "sample_every")):
        require(row[actual] == request[declared], f"{context}: manifest/result {actual} mismatch")
    digest(row["input_toml_digest"], context + ".input digest")
    identity, canonical = row["identity"], row["canonical_config"]
    if canonical is not None:
        require(isinstance(canonical, str) and tomllib.loads(canonical) == scenario, f"{context}: canonical parameter mismatch")
    else:
        require(row.get("canonical_config_omitted") == "output_reservation_budget" or row["status"] == "refused",
                f"{context}: unexplained missing canonical source")
    if identity is not None:
        require(isinstance(identity, dict) and type(identity["derived"]) is bool, f"{context}: invalid identity")
        digest(identity["config_hash"], context + ".config hash", True)
        digest(identity["canonical_digest"], context + ".canonical digest")
        if "runtime_identity_omitted" in identity:
            fields(identity, ("config_hash", "canonical_digest", "derived", "runtime_identity_omitted"), context + ".identity")
            require(identity["runtime_identity_omitted"] == "output_reservation_budget" and row["status"] == "refused",
                    f"{context}: false omitted runtime identity")
        elif not identity["derived"]:
            fields(identity, ("config_hash", "canonical_digest", "derived", "validation"), context + ".identity")
        if identity["derived"] and "runtime_identity_omitted" not in identity:
            check_identity(identity, scenario, context)
    status, reason = row["status"], row["stop_reason"]
    checked = natural(row["checked_ticks"], context + ".checked", STEPS)
    attempted = integer_string(row["attempted_cell_ticks"], context + ".attempted")
    committed = integer_string(row["committed_cell_ticks"], context + ".committed")
    require(committed <= attempted and committed <= checked * 512, f"{context}: invalid work evidence")
    samples = row["samples"]
    require(isinstance(samples, list) and len(samples) <= 201 and type(row["samples_truncated"]) is bool,
            f"{context}: invalid bounded samples")
    if status == "refused":
        require(reason in {"input_or_admission_refused", "initialization_refused", "output_reservation_budget", "batch_budget_before_start"},
                f"{context}: false refusal stop")
        require(checked == attempted == committed == 0 and row["attempted_tick"] is None
                and row["attempt_residuals"] is None and row["summary"] is None and not samples, f"{context}: refusal claims completed science")
        if reason != "batch_budget_before_start":
            require(isinstance(row["error"], str) and row["error"], f"{context}: unexplained refusal")
        return
    require(status in {"censored", "numerical_failure", "extinct"} and identity is not None and identity["derived"]
            and "runtime_identity_omitted" not in identity and canonical is not None, f"{context}: missing observed-run identity")
    summary = row["summary"]
    check_summary(summary, scenario, identity, context + ".final")
    require(summary["tick"] == checked, f"{context}: final state boundary mismatch")
    digest(row["final_state_digest"], context + ".state digest")
    digest(row["tick_reports_digest"], context + ".report digest")
    require(type(row["tick_reports_hashed"]) is int and row["tick_reports_hashed"] == checked and row["tick_report_digest_encoding"] ==
            "BLAKE3 of concatenated serde_json MicroStepReport values for committed ticks, in tick order", f"{context}: report count/encoding mismatch")
    failed = reason in {"cell_capacity_limit", "numerical_failure"}
    if failed:
        require(row["attempted_tick"] == checked + 1 and row["attempt_residuals"] is None
                and attempted == committed + summary["living_cells"] and isinstance(row["error"], str) and row["error"],
                f"{context}: rejected tick evidence changed")
    else:
        require(row["attempted_tick"] == (checked if checked else None) and attempted == committed, f"{context}: successful boundary/work mismatch")
        residual = None if checked == 0 else {"matter": ["0"] * len(identity["matter_scales"]), "energy": "0"}
        require(row["attempt_residuals"] == residual, f"{context}: false attempted residual")
    require(committed >= checked, f"{context}: insufficient committed work")
    if reason == "requested_horizon_reached":
        require(status == "censored" and checked == STEPS and summary["living_cells"] > 0, f"{context}: false horizon completion")
    elif reason == "extinction":
        require(status == "extinct" and checked > 0 and summary["living_cells"] == 0, f"{context}: false extinction")
    elif reason == "cell_capacity_limit":
        require(status == "censored" and checked < STEPS and "max_cells" in row["error"], f"{context}: false capacity stop")
    elif reason == "numerical_failure":
        require(status == "numerical_failure" and checked < STEPS, f"{context}: false numerical failure")
    elif reason in {"batch_cell_work_budget", "output_sample_budget"}:
        require(status == "censored" and (checked < STEPS or reason == "output_sample_budget"), f"{context}: false resource stop")
        require(reason != "output_sample_budget" or row["samples_truncated"], f"{context}: unmarked omitted samples")
    else:
        raise ValidationError(f"{context}: unregistered/disabled stop {reason!r}")
    ticks, previous = [], None
    for sample in samples:
        check_summary(sample, scenario, identity, context + ".sample")
        tick = sample["tick"]
        require(tick <= checked and (tick % SAMPLE_EVERY == 0 or tick == checked), f"{context}: fabricated/off-grid sample")
        require(not ticks or tick > ticks[-1], f"{context}: samples repeated/out of order")
        if previous is not None:
            for field in ("peak_cells", "max_generation_observed", "observed_allele_richness"):
                require(sample[field] >= previous[field], f"{context}: historical metric decreased")
            require(all(sample["allele_first_seen_tick"].get(a) == t for a, t in previous["allele_first_seen_tick"].items()),
                    f"{context}: allele history rewritten")
            for field in ("daughters_born", "fissions", "deaths"):
                require(int(sample["lifecycle"][field]) >= int(previous["lifecycle"][field]), f"{context}: lifecycle counter decreased")
            for field in ("growth_extent", "death_mass_units"):
                require(int(sample["accounting"][field]) >= int(previous["accounting"][field]), f"{context}: cumulative counter decreased")
        previous = sample
        ticks.append(tick)
    expected = list(range(0, checked + 1, SAMPLE_EVERY))
    if checked % SAMPLE_EVERY:
        expected.append(checked)
    require(ticks == (expected[:len(ticks)] if row["samples_truncated"] else expected), f"{context}: missing/invented sample schedule")
    if samples and ticks[-1] == checked:
        require(samples[-1] == summary, f"{context}: final sample/summary mismatch")


def validate(manifest, results, baseline):
    """Return complete ordered rows after structural validation."""
    scenarios = validate_manifest(manifest, baseline)
    fields(results, ("batch", "batch_id", "budgets", "determinism", "kind", "limitations", "provenance",
                     "runs", "schema_version", "source_commit"), "results")
    require(isinstance(results, dict) and results["schema_version"] == 1 and results["kind"] == "comparative_cell_lab",
            "results: unsupported schema")
    require(results["source_commit"] == SOURCE_COMMIT, "results: source differs from accepted LAB-1 engine")
    require(results["batch_id"] == manifest["batch_id"], "results: batch_id mismatch")
    p = results["provenance"]
    fields(p, ("build_attestation", "build_requirement", "host_arch", "host_os", "numerical_mode", "profile",
               "runner_version", "runtime_rustc", "source_verification"), "results.provenance")
    require(p["source_verification"] == "git_head_and_clean_provenance_inputs" and p["profile"] == "release"
            and p["build_attestation"] is False and p["runner_version"] == "0.1.0"
            and isinstance(p["runtime_rustc"], str) and p["runtime_rustc"].startswith("rustc 1.97.1 ")
            and p["numerical_mode"] == "FLOAT/native f32 Q and f64 genome decoding; exact i128 ledgers"
            and all(isinstance(p[k], str) and p[k] for k in ("host_os", "host_arch")), "results: provenance/version/profile mismatch")
    b = results["budgets"]
    fields(b, ("max_batch_seconds", "max_batch_steps", "max_cell_ticks", "max_cells", "max_output_bytes",
               "max_run_seconds", "max_runs", "max_steps_per_run"), "results.budgets")
    fields(results["determinism"], ("claim", "watchdogs_enabled"), "results.determinism")
    for field, low, high in (("max_runs", 24, 64), ("max_steps_per_run", STEPS, 1_000_000),
                             ("max_batch_steps", 24 * STEPS, 2_000_000), ("max_cells", 512, 4096),
                             ("max_cell_ticks", 1, 100_000_000), ("max_output_bytes", 1, RESULT_LIMIT)):
        require(type(b[field]) is int and low <= b[field] <= high, f"results: invalid {field}")
    require(b["max_run_seconds"] == b["max_batch_seconds"] == 0 and results["determinism"]["watchdogs_enabled"] is False,
            "results: watchdog not preregistered")
    rows = results["runs"]
    require(isinstance(rows, list) and len(rows) == 24, "results: missing/extra rows")
    seen, storage = {}, None
    for row, request, scenario in zip(rows, manifest["runs"], scenarios):
        check_run(row, request, scenario)
        identity = row["identity"]
        if identity is not None:
            signature = (row["canonical_config"], identity["config_hash"], identity["canonical_digest"], row["input_toml_digest"])
            require(signature == seen.setdefault(row["condition"], signature), f"{row['condition']}: canonical/hash changes across seeds")
            if identity["derived"] and "runtime_identity_omitted" not in identity:
                scales = (identity["matter_scales"], identity["energy_units_per_joule"])
                if storage is None:
                    storage = scales
                require(scales == storage, "results: fixed-registry scales changed across conditions")
    require(len(seen) == 6 and len({value[1] for value in seen.values()}) == 6
            and len({value[2] for value in seen.values()}) == 6,
            "results: different materialized conditions share config/canonical hashes")
    batch = results["batch"]
    fields(batch, ("admitted_requested_steps", "attempted_cell_ticks", "committed_cell_ticks"), "results.batch")
    attempted = integer_string(batch["attempted_cell_ticks"], "batch attempted")
    committed = integer_string(batch["committed_cell_ticks"], "batch committed")
    require(attempted == sum(int(row["attempted_cell_ticks"]) for row in rows)
            and committed == sum(int(row["committed_cell_ticks"]) for row in rows)
            and committed <= attempted <= b["max_cell_ticks"], "results: work totals do not close")
    require(batch["admitted_requested_steps"] == STEPS * sum(bool(row["identity"] and row["identity"]["derived"]) for row in rows),
            "results: admitted requested steps mismatch")
    return rows


def metrics(summary):
    values = {key: summary[key] for key in ("living_cells", "structural_mass_mol", "internal_energy_j", "bath_heat_j",
              "observed_allele_richness", "max_generation_observed", "structural_mass_units", "internal_energy_units", "bath_heat_units")}
    values.update({key: summary["lifecycle"][key] for key in ("daughters_born", "fissions", "deaths")})
    values.update({key: summary["accounting"][key] for key in ("growth_extent", "medium_energy_units", "death_mass_units")})
    pools = {x["id"]: x["free_mol"] for x in summary["matter"]}
    values.update(food_free_mol=pools["FOOD"], oxygen_free_mol=pools["O2"], detritus_free_mol=pools["DET"])
    units = {x["id"]: x["free_units"] for x in summary["matter"]}
    values.update(food_free_units=units["FOOD"], oxygen_free_units=units["O2"], detritus_free_units=units["DET"])
    values.update({f"medium_matter_{species}_units": value
                   for species, value in zip((x["id"] for x in summary["matter"]), summary["accounting"]["medium_matter_units"])})
    return values


def delta(condition, baseline):
    return {key: str(int(condition[key]) - int(baseline[key])) if key in EXACT_STRING_METRICS
            else condition[key] - baseline[key] for key in METRIC_UNITS}


def missing_reason(row, tick):
    return {"status": row["status"], "stop_reason": row["stop_reason"], "checked_ticks": row["checked_ticks"],
            "samples_truncated": row["samples_truncated"],
            "reason": "run_refused" if row["status"] == "refused" else
                      "stopped_before_landmark" if row["checked_ticks"] < tick else "sample_not_retained"}


def pair_record(seed, condition, tick, baseline, altered):
    pair = {"seed": seed, "condition": condition, "tick": tick,
            "baseline_run_id": baseline["run_id"], "condition_run_id": altered["run_id"]}
    a = next((x for x in baseline["samples"] if x["tick"] == tick), None)
    b = next((x for x in altered["samples"] if x["tick"] == tick), None)
    pair.update(baseline_allele_histogram=a["allele_histogram"] if a is not None else None,
                condition_allele_histogram=b["allele_histogram"] if b is not None else None,
                delta_allele_histogram=None)
    if a is None or b is None:
        pair.update(available=False, baseline=metrics(a) if a is not None else None,
                    condition_metrics=metrics(b) if b is not None else None, delta=None,
                    missing={key: missing_reason(row, tick or 0) for key, row, sample in
                             (("baseline", baseline, a), ("condition", altered, b)) if sample is None})
    else:
        x, y = metrics(a), metrics(b)
        pair.update(available=True, baseline=x, condition_metrics=y, delta=delta(y, x), missing={})
        pair["delta_allele_histogram"] = {str(allele): b["allele_histogram"].get(str(allele), 0) - a["allele_histogram"].get(str(allele), 0)
                                         for allele in range(-4, 5)}
    return pair


def ranges(pairs, field):
    available = [pair for pair in pairs if pair["available"]]
    result = {}
    for key in METRIC_UNITS:
        values = [int(pair[field][key]) if key in EXACT_STRING_METRICS else pair[field][key] for pair in available]
        lo, hi = (min(values), max(values)) if values else (None, None)
        width = hi - lo if values else None
        if values and key in EXACT_STRING_METRICS:
            lo, hi, width = str(lo), str(hi), str(width)
        result[key] = {"count": len(values), "min": lo, "max": hi, "width": width}
    return result


def comparisons(rows, integrity):
    lookup = {(row["seed"], row["condition"]): row for row in rows}
    landmarks, latest = [], []
    for condition in CONDITIONS[1:]:
        for tick in LANDMARKS:
            pairs = [pair_record(s, condition, tick, lookup[(s, "baseline")], lookup[(s, condition)]) for s in SEEDS]
            landmarks.append({"condition": condition, "tick": tick, "time_seconds": tick * DT, "expected_pairs": 4,
                              "available_pairs": sum(x["available"] for x in pairs), "pairs": pairs,
                              "ranges": {f: ranges(pairs, f) for f in ("baseline", "condition_metrics", "delta")}})
        for seed in SEEDS:
            a, b = lookup[(seed, "baseline")], lookup[(seed, condition)]
            common = {x["tick"] for x in a["samples"]} & {x["tick"] for x in b["samples"]}
            latest.append(pair_record(seed, condition, max(common) if common else None, a, b))
    return {
        "schema_version": 1, "kind": "paired_cell_landmark_comparisons", "source_commit": SOURCE_COMMIT,
        "validation": {"status": "PASS", "scope": "static structural validation of immutable runner evidence",
                       "independent_integrity": integrity,
                       "engine_produced_evidence": "BLAKE3 and every-tick ledger checks are runner evidence; Python checks structure/counts, not truth by re-execution.",
                       "not_performed": ["Independent BLAKE3 recomputation", "Core simulation/replay", "Build attestation",
                                         "Independent per-tick numerical ledger verification"]},
        "matrix": {"seeds": list(SEEDS), "conditions": list(CONDITIONS), "expected_runs": 24,
                   "requested_steps": STEPS, "sample_every": SAMPLE_EVERY, "dt_seconds": DT},
        "metrics": {k: {"unit": v, "encoding": "decimal integer string" if k in EXACT_STRING_METRICS else "JSON number"} for k, v in METRIC_UNITS.items()},
        "comparison_rule": "Condition minus baseline for the same seed and exact sampled tick; no interpolation or carry-forward.",
        "allele_composition_rule": "Sparse observed living-cell histograms are retained for both arms; signed per-allele count deltas cover kinetics -4..4 and sum to the living-cell delta.",
        "landmark_comparisons": landmarks, "latest_common_samples": latest,
        "latest_common_rule": "Each seed/condition pair keeps its own latest common sampled tick; differing endpoints are not pooled.",
        "limitations": ["Four paired seeds are bounded exploration, not a significance test or statistical discovery.",
                        "Horizon/resource stops censor future history; refused/numerical failures remain in the full matrix.",
                        "No cellular temperature response, spatial transport, new genome engine or open-ended evolution is inferred."]}


def summary_csv(rows):
    output = io.StringIO(newline="")
    columns = ["run_id", "seed", "condition", "status", "stop_reason", "requested_steps", "checked_ticks", "attempted_tick",
               "attempted_cell_ticks", "committed_cell_ticks", "samples_truncated", "config_hash", "canonical_digest",
               "final_state_digest", "tick_reports_digest", "final_tick", "final_time_seconds", *METRIC_UNITS,
               "allele_histogram", "allele_first_seen_tick", "error"]
    writer = csv.DictWriter(output, fieldnames=columns, lineterminator="\n")
    writer.writeheader()
    for row in rows:
        values = {key: row.get(key) for key in columns if key in row}
        identity = row["identity"] or {}
        values.update(config_hash=identity.get("config_hash"), canonical_digest=identity.get("canonical_digest"))
        if row["summary"] is not None:
            values.update(metrics(row["summary"]))
            values.update(final_tick=row["summary"]["tick"], final_time_seconds=row["summary"]["time_seconds"])
            values.update({key: json.dumps(row["summary"][key], sort_keys=True, separators=(",", ":"))
                           for key in ("allele_histogram", "allele_first_seen_tick")})
        writer.writerow(values)
    return output.getvalue()


def read_bounded(path, maximum):
    with path.open("rb") as stream:
        data = stream.read(maximum + 1)
    require(len(data) <= maximum, f"{path.name}: byte cap exceeded")
    return data


def parse_json(data):
    def pairs(items):
        value = {}
        for key, item in items:
            require(key not in value, f"JSON duplicate key {key!r}")
            value[key] = item
        return value
    def constant(value):
        raise ValidationError(f"JSON non-finite constant {value}")
    return json.loads(data, object_pairs_hook=pairs, parse_constant=constant)


def aliases(first, second):
    if first.resolve() == second.resolve():
        return True
    return first.exists() and second.exists() and first.samefile(second)


def output_targets(directory, protected):
    targets = [directory / "summary.csv", directory / "comparisons.json"]
    require(not aliases(targets[0], targets[1]), "outputs: derived target paths alias one another")
    for target in targets:
        require(all(not aliases(target, path) for path in protected),
                f"outputs: {target.name} aliases immutable input")
    return targets


def write_derived(targets, texts):
    """Stage both derived files before replacing either; never follow target links."""
    staged = []
    try:
        targets[0].parent.mkdir(parents=True, exist_ok=True)
        for target, text in zip(targets, texts):
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="",
                                             dir=target.parent, prefix=".lab2-validator-", suffix=".tmp", delete=False) as stream:
                staged.append(Path(stream.name))
                stream.write(text)
        for temporary, target in zip(staged, targets):
            os.replace(temporary, target)
    finally:
        for temporary in staged:
            temporary.unlink(missing_ok=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--results", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--baseline-config", type=Path, default=Path(__file__).resolve().parents[3] / "configs/scenarios/cell-chamber.toml")
    args = parser.parse_args(argv)
    try:
        targets = output_targets(args.output_dir, (args.manifest, args.results, args.baseline_config))
        m = read_bounded(args.manifest, MANIFEST_LIMIT)
        r = read_bounded(args.results, RESULT_LIMIT)
        b = read_bounded(args.baseline_config, 100 * 1024)
        manifest, results = parse_json(m), parse_json(r)
        rows = validate(manifest, results, tomllib.loads(b.decode()))
        require(len(r) <= results["budgets"]["max_output_bytes"], "results: raw bytes exceed declared output cap")
        integrity = {key: {"algorithm": "SHA-256", "digest": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
                     for key, data in (("manifest", m), ("results", r), ("baseline_config", b))}
        encoded = json.dumps(comparisons(rows, integrity), ensure_ascii=False, allow_nan=False, indent=2) + "\n"
        write_derived(targets, (summary_csv(rows), encoded))
        print("PASS: 24 ordered rows; structural checks only; summary.csv and comparisons.json written")
        return 0
    except (OSError, ValueError, KeyError, TypeError, IndexError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
