#!/usr/bin/env python3
"""LAB-2: неизменяемые byte copies и декларативный descriptor, без симуляции.

Запуск: python3 scripts/build_lab_catalog.py [--check]
SHA фиксируют принятые исторические файлы. Проверки ниже относятся к доставке
и связям записей; они не повторяют расчёт comparisons или numerical validation.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile
import tomllib


REPO_ROOT = Path(__file__).resolve().parents[1]
SOURCE_DIRECTORY = Path("docs/experiments/lab-2")
OUTPUT_DIRECTORY = Path("site/data/lab-2")
SOURCE_COMMIT = "21cbe90732128edee61963544e414d29a7420577"
SOURCE_PINS = {
    "results": (5294742, "6fd944f6fe2e25ca5418ac357dcb97ce68620afee569b2ecd6dc684c22f7d37a"),
    "comparisons": (1116928, "e2faeda80c9f47ff23e5aff14879f3780627d867fae51aa655b77d4ad52a8ae3"),
    "manifest": (82880, "f7a669548650754864a410a46e86593ab722116e0a01057e17f6efc84efcb2f3"),
}
CONFIG_PINS = {
    # Точные materialized TOML и canonical UTF-8 из принятого LAB-2.
    "baseline": (3102, "d6a2d667317b0eca9837e96f4d8234d8be7e55e67927cd95ab67042b3906d526", "4e72889e4cc2857a7f259a359b8a9bda349861f8bb72d6c9dd74fe6511ca03de"),
    "mutation_off": (3101, "3b7e2f3421df1f09ecdeae2c3da57b907318d4106987cbb384a7165b25619cc7", "04e20919a97134ff7acfce8cc999a9f8b7523bf43f15744c9fdd2592a5b6a421"),
    "starvation": (3100, "469c8bc63dc191e8af594a883ab4f728744b342a1900e1c5f122f7965beffbef", "75e3184ae421d3840cacba0ef8ede5ae91fe02249aeca9a3a60e35270d9b0358"),
    "oxygen_low": (3104, "b479ffb1c0d0e08991090f3ae6aad08248d084a0520ef39a6831a976677fd147", "682204146ce53feeb0099eec99610053f6f108fbbd6a952987f2f4caab7853d7"),
    "exchange_half": (3102, "b5c12296d47b83ffcc919482134d995d85086b5dc335fa9a20d8c08831e07b8e", "2e66a44a5641d3317f3fc211c38b3e599214a2e1be7559c8bbb7e6c083a17881"),
    "founder_k2": (3102, "7788568a676fc6588ac4b47af585b7d9afa7f03fba7ace258192831abbe8440c", "a38587228f228cf3efd4f00eb0af19b8983594773742514aed4be36d8df33b53"),
}
CONDITIONS = list(CONFIG_PINS)
SEEDS = ["1", "7", "42", "2026"]
LANDMARKS = [100, 500, 1000, 2000, 5000, 10000, 20000]
BATCH_ID = "lab-2-chamber1-paired-v1"
DECIMAL_INTEGER = re.compile(r"-?(?:0|[1-9][0-9]*)\Z")


class CatalogError(ValueError):
    """Неизменяемый источник или delivery contract не совпал."""


def require(condition, message):
    if not condition:
        raise CatalogError(message)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read_regular(path, max_bytes):
    """До чтения отвергает symlink/FIFO и превышение размера; bounded FD read."""
    try:
        require(stat.S_ISREG(path.lstat().st_mode), f"Not a regular file: {path}")
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
        fd = os.open(path, flags)
        with os.fdopen(fd, "rb") as stream:
            metadata = os.fstat(stream.fileno())
            require(stat.S_ISREG(metadata.st_mode), f"Not a regular file: {path}")
            require(metadata.st_size <= max_bytes, f"File exceeds byte bound: {path}")
            data = stream.read(max_bytes + 1)
            require(len(data) <= max_bytes, f"File exceeds byte bound: {path}")
            return data
    except OSError as exc:
        raise CatalogError(f"Cannot read {path}: {exc}") from exc


def parse_json(data):
    def unique_fields(pairs):
        obj = {}
        for key, value in pairs:
            require(key not in obj, f"Duplicate JSON key: {key}")
            obj[key] = value
        return obj

    def nonfinite(value):
        raise CatalogError(f"Non-finite JSON value: {value}")

    try:
        return json.loads(data.decode("utf-8"), object_pairs_hook=unique_fields, parse_constant=nonfinite)
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise CatalogError(f"Invalid JSON: {exc}") from exc


def verify_pair(pair, runs, condition, tick, seed):
    """Проверка exact join/availability, без вычисления metric differences."""
    baseline = runs[(seed, "baseline")]
    altered = runs[(seed, condition)]
    require(pair["seed"] == seed and pair["condition"] == condition and pair["tick"] == tick,
            "Comparison seed/condition/tick join differs")
    require(pair["baseline_run_id"] == baseline["run_id"]
            and pair["condition_run_id"] == altered["run_id"], "Comparison run_id join differs")
    base_sample = next((x for x in baseline["samples"] if x["tick"] == tick), None)
    condition_sample = next((x for x in altered["samples"] if x["tick"] == tick), None)
    available = base_sample is not None and condition_sample is not None
    require(type(pair["available"]) is bool and pair["available"] == available,
            "Comparison availability differs from exact retained samples")
    require(pair["baseline_allele_histogram"] == (base_sample["allele_histogram"] if base_sample else None)
            and pair["condition_allele_histogram"] == (condition_sample["allele_histogram"] if condition_sample else None),
            "Comparison histogram differs from joined sample")
    if available:
        require(isinstance(pair["baseline"], dict) and isinstance(pair["condition_metrics"], dict)
                and isinstance(pair["delta"], dict) and isinstance(pair["delta_allele_histogram"], dict)
                and pair["missing"] == {}, "Available pair has missing metrics")
    else:
        require(pair["condition_metrics"] is None and pair["delta"] is None
                and pair["delta_allele_histogram"] is None, "Missing pair uses zero/carry-forward metrics")
        missing = pair["missing"].get("condition", {})
        require(missing.get("status") == altered["status"]
                and missing.get("stop_reason") == altered["stop_reason"]
                and missing.get("checked_ticks") == altered["checked_ticks"]
                and missing.get("reason") == "stopped_before_landmark", "Missing pair loses stop reason")


def make_descriptor(manifest, results, comparisons):
    """Адаптирует только pinned LAB-2; не создаёт новые научные показатели."""
    require(manifest["schema_version"] == results["schema_version"] == comparisons["schema_version"] == 1,
            "Unsupported LAB schema")
    require(manifest["batch_id"] == results["batch_id"] == BATCH_ID, "LAB batch_id differs")
    require(results["kind"] == "comparative_cell_lab"
            and comparisons["kind"] == "paired_cell_landmark_comparisons", "Unsupported LAB kind")
    require(results["source_commit"] == comparisons["source_commit"] == SOURCE_COMMIT,
            "Historical scientific source_commit differs")
    require(results["provenance"]["build_attestation"] is False, "Source provenance claims build attestation")
    matrix = {"seeds": SEEDS, "conditions": CONDITIONS, "expected_runs": 24,
              "requested_steps": 20000, "sample_every": 100, "dt_seconds": 30.0}
    require(comparisons["matrix"] == matrix, "LAB matrix differs")
    require(len(manifest["runs"]) == len(results["runs"]) == 24, "LAB must retain all 24 declarations")

    runs, configs, rows = {}, {}, []
    for index, (declaration, run) in enumerate(zip(manifest["runs"], results["runs"])):
        seed, condition = SEEDS[index // 6], CONDITIONS[index % 6]
        run_id = f"seed-{seed}-{condition}"
        require(all(declaration[key] == run[key] == value for key, value in
                    (("run_id", run_id), ("seed", seed), ("condition", condition))), "Run declaration/order join differs")
        require(type(run["seed"]) is str, "Seed must remain an exact decimal string")
        require(declaration["steps"] == run["requested_steps"] == 20000
                and declaration["sample_every"] == run["sample_every"] == 100,
                "Requested horizon/sampling differs")
        identity = run["identity"]
        require(identity["world_format_version"] == 30 and identity["chamber_format"] == 1
                and identity["dt_seconds"] == 30.0 and identity["spatial_positions"] is False
                and identity["model"] == "well_mixed_individual_cells"
                and identity["scenario"] == "cell-chamber", "Historical LAB identity differs")

        toml = declaration["scenario_toml"].encode("utf-8")
        canonical = run["canonical_config"].encode("utf-8")
        size, toml_digest, canonical_digest = CONFIG_PINS[condition]
        require((len(toml), sha256(toml)) == (size, toml_digest), f"Materialized TOML pin differs: {condition}")
        require(sha256(canonical) == canonical_digest, f"Canonical config pin differs: {condition}")
        try:
            parameters = tomllib.loads(declaration["scenario_toml"])
        except tomllib.TOMLDecodeError as exc:
            raise CatalogError(f"Invalid materialized TOML: {exc}") from exc
        config = {"condition": condition, "toml_bytes": size, "toml_sha256": toml_digest,
                  "canonical_config_sha256": canonical_digest, "input_toml_digest": run["input_toml_digest"],
                  "identity": identity, "declared_parameters": parameters}
        if condition in configs:
            require(config == configs[condition], f"Condition config/identity inconsistent across seeds: {condition}")
        else:
            configs[condition] = config

        extinct = condition == "starvation"
        checked = 454 if extinct else 20000
        ticks = [0, 100, 200, 300, 400, 454] if extinct else list(range(0, 20001, 100))
        require(run["status"] == ("extinct" if extinct else "censored")
                and run["stop_reason"] == ("extinction" if extinct else "requested_horizon_reached"),
                "Historical status/stop_reason differs")
        require(run["checked_ticks"] == run["attempted_tick"] == checked and run["samples_truncated"] is False,
                "Actual horizon/truncation differs")
        require([x["tick"] for x in run["samples"]] == ticks, "Exact retained sample ticks differ")
        require(all(x["time_seconds"] == x["tick"] * 30.0 for x in run["samples"]), "Sample time/tick identity differs")
        require(run["summary"] == run["samples"][-1], "Summary differs from retained final sample")
        for key in ("attempted_cell_ticks", "committed_cell_ticks"):
            require(type(run[key]) is str and DECIMAL_INTEGER.fullmatch(run[key]), "Cell-work counter is not an integer string")
        rows.append({"run_id": run_id, "condition": condition, "seed": seed, "results_index": index,
                     "requested_steps": run["requested_steps"], "checked_ticks": run["checked_ticks"],
                     "attempted_tick": run["attempted_tick"], "sample_every": run["sample_every"],
                     "sample_count": len(run["samples"]), "status": run["status"],
                     "stop_reason": run["stop_reason"], "samples_truncated": run["samples_truncated"]})
        runs[(seed, condition)] = run
    require(sum(x["sample_count"] for x in rows) == 4044, "Retained sample inventory differs")

    landmarks = comparisons["landmark_comparisons"]
    require(len(landmarks) == 35, "Landmark inventory differs")
    metric_names = set(comparisons["metrics"])
    require(len(metric_names) == 27, "Comparison metric inventory differs")
    for row, (condition, tick) in zip(landmarks, ((c, t) for c in CONDITIONS[1:] for t in LANDMARKS)):
        require(row["condition"] == condition and row["tick"] == tick
                and row["time_seconds"] == tick * 30.0 and row["expected_pairs"] == 4
                and len(row["pairs"]) == 4, "Landmark order/denominator differs")
        for pair, seed in zip(row["pairs"], SEEDS):
            verify_pair(pair, runs, condition, tick, seed)
        available = sum(pair["available"] for pair in row["pairs"])
        require(row["available_pairs"] == available, "Available pair count differs")
        require(set(row["ranges"]) == {"baseline", "condition_metrics", "delta"}
                and all(set(arm) == metric_names for arm in row["ranges"].values()),
                "Comparison range metric inventory differs")
        if available == 0:
            require(all(value == {"count": 0, "min": None, "max": None, "width": None}
                        for arm in row["ranges"].values() for value in arm.values()),
                    "Missing range must remain unknown, not zero")
    latest = comparisons["latest_common_samples"]
    require(len(latest) == 20, "Latest-common inventory differs")
    for pair, (condition, seed) in zip(latest, ((c, s) for c in CONDITIONS[1:] for s in SEEDS)):
        tick = 400 if condition == "starvation" else 20000
        verify_pair(pair, runs, condition, tick, seed)
        require(pair["available"] is True, "Latest-common sample is unavailable")

    return {
        "schema_version": 1, "kind": "aggregate_cell_lab_descriptor", "batch_id": BATCH_ID,
        "files": {name: {"url": f"./data/lab-2/{name}.json", "bytes": size, "sha256": digest}
                  for name, (size, digest) in SOURCE_PINS.items()},
        "source": {"source_commit": SOURCE_COMMIT, "world_format_version": 30, "chamber_format": 1,
                   "dt_seconds": 30.0, "provenance": results["provenance"], "limitations": results["limitations"]},
        "delivery": {"builder": "scripts/build_lab_catalog.py", "builder_schema_version": 1,
                     "numerical_recalculation": False, "total_raw_bytes": 6494550},
        "matrix": comparisons["matrix"],
        "capabilities": {"aggregate_samples": True, "individual_cells": False, "spatial_positions": False,
                         "full_genomes": False, "resumable_states": False, "event_reconstruction": False},
        "conditions": [configs[condition] for condition in CONDITIONS], "runs": rows,
    }


def build_artifacts(repo_root=REPO_ROOT):
    artifacts, parsed = {}, {}
    for name, (size, digest) in SOURCE_PINS.items():
        path = repo_root / SOURCE_DIRECTORY / f"{name}.json"
        data = read_regular(path, size)
        require(len(data) == size and sha256(data) == digest, f"Immutable source pin differs: {name}.json")
        artifacts[f"{name}.json"] = data
        parsed[name] = parse_json(data)
    try:
        descriptor = make_descriptor(parsed["manifest"], parsed["results"], parsed["comparisons"])
        artifacts["descriptor.json"] = (json.dumps(descriptor, ensure_ascii=False, sort_keys=True,
                                                    indent=2, allow_nan=False) + "\n").encode("utf-8")
    except (KeyError, TypeError, ValueError) as exc:
        if isinstance(exc, CatalogError):
            raise
        raise CatalogError(f"Invalid LAB descriptor input: {exc}") from exc
    return artifacts


def write_or_check(repo_root=REPO_ROOT, check=False):
    artifacts = build_artifacts(repo_root)  # Никаких output writes до всех source guards.
    directory = repo_root / OUTPUT_DIRECTORY
    require(not directory.is_symlink(), "LAB output directory must not be a symlink")
    if directory.exists():
        require(directory.is_dir(), "LAB output path is not a directory")
        require(set(x.name for x in directory.iterdir()) <= set(artifacts), "Unexpected LAB output files")
    if check:
        for name, expected in artifacts.items():
            actual = read_regular(directory / name, len(expected))
            require(actual == expected, f"Stale LAB delivery: {name}")
    else:
        directory.mkdir(parents=True, exist_ok=True)
        for name, data in artifacts.items():
            destination = directory / name
            require(not destination.is_symlink(), f"LAB output must not be a symlink: {name}")
            temporary = None
            try:
                with tempfile.NamedTemporaryFile(dir=directory, prefix=".lab-", delete=False) as stream:
                    temporary = Path(stream.name)
                    stream.write(data)
                os.replace(temporary, destination)
            finally:
                if temporary is not None:
                    temporary.unlink(missing_ok=True)
    return artifacts


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Проверить freshness, не изменяя файлы")
    args = parser.parse_args(argv)
    try:
        artifacts = write_or_check(check=args.check)
    except CatalogError as exc:
        print(f"LAB delivery FAIL: {exc}", file=sys.stderr)
        return 1
    print(f"LAB delivery {'CHECK PASS' if args.check else 'WRITE PASS'}: 24 runs, 4044 samples; "
          f"6494550 immutable raw bytes; descriptor SHA256 {sha256(artifacts['descriptor.json'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
