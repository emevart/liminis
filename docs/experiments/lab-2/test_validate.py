#!/usr/bin/env python3
"""Functional corruption checks using immutable engine results, never simulation."""

import copy
import contextlib
import csv
import io
import json
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest

sys.dont_write_bytecode = True
import validate as lab


class MatrixValidationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        directory = Path(__file__).resolve().parent
        cls.manifest = lab.parse_json((directory / "manifest.json").read_bytes())
        cls.results = lab.parse_json((directory / "results.json").read_bytes())
        cls.baseline = tomllib.loads((directory.parents[2] / "configs/scenarios/cell-chamber.toml").read_text())

    def altered(self):
        return copy.deepcopy(self.results)

    def reject(self, results, text=None):
        with self.assertRaises(lab.ValidationError) if text is None else self.assertRaisesRegex(lab.ValidationError, text):
            lab.validate(self.manifest, results, self.baseline)

    def test_complete_real_matrix_and_csv_preserve_all_statuses(self):
        rows = lab.validate(self.manifest, self.results, self.baseline)
        self.assertEqual(len(rows), 24)
        extracted = list(csv.DictReader(io.StringIO(lab.summary_csv(rows))))
        self.assertEqual([x["run_id"] for x in extracted], [x["run_id"] for x in rows])
        self.assertEqual(sum(x["status"] == "extinct" for x in extracted), 4)
        self.assertTrue(all(x["structural_mass_units"] and x["structural_mass_mol"] for x in extracted))
        self.assertTrue(all(x["medium_matter_FOOD_units"] and x["medium_energy_units"] for x in extracted))

    def test_fixed_landmarks_retain_missing_pairs_without_carry_forward(self):
        extracted = lab.comparisons(self.results["runs"], {})
        self.assertEqual(len(extracted["landmark_comparisons"]), 35)
        rows = [x for x in extracted["landmark_comparisons"] if x["condition"] == "starvation"]
        self.assertEqual([x["available_pairs"] for x in rows], [4, 0, 0, 0, 0, 0, 0])
        for item in rows[1:]:
            self.assertEqual(item["expected_pairs"], 4)
            self.assertTrue(all(pair["delta"] is None and pair["missing"]["condition"]["stop_reason"] == "extinction"
                                for pair in item["pairs"]))
            self.assertTrue(all(value == {"count": 0, "min": None, "max": None, "width": None}
                                for value in item["ranges"]["delta"].values()))
        self.assertEqual(len(extracted["latest_common_samples"]), 20)
        latest = [x for x in extracted["latest_common_samples"] if x["condition"] == "starvation"]
        self.assertEqual([x["tick"] for x in latest], [400, 400, 400, 400])
        self.assertTrue(all("ranges" not in item for item in latest))

    def test_composition_deltas_and_range_width_keep_metric_units(self):
        extracted = lab.comparisons(self.results["runs"], {})
        row = next(x for x in extracted["landmark_comparisons"] if x["condition"] == "mutation_off" and x["tick"] == 20000)
        for pair in row["pairs"]:
            self.assertEqual(set(pair["condition_allele_histogram"]), {"0"})
            self.assertEqual(sum(pair["delta_allele_histogram"].values()), pair["delta"]["living_cells"])
            self.assertEqual(pair["delta_allele_histogram"]["0"], pair["condition_allele_histogram"]["0"]
                             - pair["baseline_allele_histogram"].get("0", 0))
        scalar = row["ranges"]["delta"]["living_cells"]
        values = [pair["delta"]["living_cells"] for pair in row["pairs"]]
        self.assertEqual(scalar["width"], max(values) - min(values))
        exact = row["ranges"]["delta"]["structural_mass_units"]
        values = [int(pair["delta"]["structural_mass_units"]) for pair in row["pairs"]]
        self.assertEqual(exact["width"], str(max(values) - min(values)))

    def test_swapped_or_missing_results_are_rejected(self):
        value = self.altered()
        value["runs"][0], value["runs"][1] = value["runs"][1], value["runs"][0]
        self.reject(value, "manifest/result")
        value = self.altered()
        value["runs"].pop()
        self.reject(value, "missing/extra")

    def test_source_profile_and_units_must_match_declared_engine(self):
        value = self.altered()
        value["source_commit"] = "f" * 40
        self.reject(value, "source")
        value = self.altered()
        value["provenance"]["profile"] = "debug"
        self.reject(value, "provenance")
        value = self.altered()
        value["runs"][0]["identity"]["units"]["time"] = "tick"
        self.reject(value, "unit/biomass")

    def test_lifecycle_and_histogram_corruption_are_rejected(self):
        value = self.altered()
        value["runs"][0]["summary"]["lifecycle"]["deaths"] = str(int(value["runs"][0]["summary"]["lifecycle"]["deaths"]) + 1)
        self.reject(value, "lifecycle identity")
        value = self.altered()
        hist = value["runs"][0]["summary"]["allele_histogram"]
        hist[next(iter(hist))] += 1
        self.reject(value, "histogram sum")

    def test_fake_samples_and_sample_gaps_are_rejected(self):
        value = self.altered()
        sample = value["runs"][0]["samples"][1]
        sample["tick"], sample["time_seconds"], sample["ledger"]["checked_ticks"] = 150, 4500.0, 150
        self.reject(value, "off-grid")
        value = self.altered()
        value["runs"][0]["samples"].pop(1)
        self.reject(value, "sample schedule")

    def test_genesis_unknown_residuals_and_every_tick_counts_are_checked(self):
        value = self.altered()
        ledger = value["runs"][0]["samples"][0]["ledger"]
        ledger["matter_residual_last_checked"], ledger["energy_residual_last_checked"] = ["0"] * 6, "0"
        self.reject(value, "last-checked ledger")
        value = self.altered()
        value["runs"][0]["tick_reports_hashed"] -= 1
        self.reject(value, "report count")
        value = self.altered()
        value["runs"][0]["summary"]["ledger"]["energy_residual_last_checked"] = "1"
        self.reject(value, "last-checked ledger")

    def test_materialized_and_canonical_parameter_leakage_are_rejected(self):
        manifest = copy.deepcopy(self.manifest)
        manifest["runs"][1]["scenario_toml"] = manifest["runs"][1]["scenario_toml"].replace("temperature_k = 298.15", "temperature_k = 300.0")
        with self.assertRaisesRegex(lab.ValidationError, "parameter leakage"):
            lab.validate(manifest, self.results, self.baseline)
        value = self.altered()
        value["runs"][0]["canonical_config"] = value["runs"][0]["canonical_config"].replace('name = "cell-chamber"', 'name = "different"')
        self.reject(value, "canonical parameter")

    def test_same_condition_engine_hashes_are_equal_among_seeds(self):
        value = self.altered()
        value["runs"][6]["identity"]["canonical_digest"] = "blake3:" + "f" * 64
        self.reject(value, "hash changes across seeds")

    def test_different_canonical_conditions_cannot_share_every_engine_hash(self):
        value = self.altered()
        original = value["runs"][0]["identity"]
        for row in value["runs"]:
            row["identity"]["config_hash"] = original["config_hash"]
            row["identity"]["canonical_digest"] = original["canonical_digest"]
        self.reject(value, "conditions share")

    def test_starvation_cannot_claim_growth_or_reappear_as_food(self):
        value = self.altered()
        row = value["runs"][2]
        for state in [row["summary"], *row["samples"][1:]]:
            state["accounting"]["growth_extent"] = "1"
        self.reject(value, "starvation control")

    def test_unknown_result_row_summary_and_identity_fields_are_rejected(self):
        for section in ("root", "row", "summary", "identity"):
            with self.subTest(section=section):
                value = self.altered()
                target = value if section == "root" else value["runs"][0] if section == "row" else value["runs"][0][section]
                target["unsupported"] = True
                self.reject(value, "unknown/missing fields")

    def test_exported_cumulative_energy_and_channel_corruption_are_rejected(self):
        value = self.altered()
        value["runs"][0]["samples"][1]["accounting"]["medium_energy_units"] = str(
            int(value["runs"][0]["samples"][1]["accounting"]["medium_energy_units"]) + 1)
        self.reject(value, "sampled cumulative energy")
        value = self.altered()
        value["runs"][0]["samples"][1]["accounting"]["medium_matter_units"][0] = str(
            int(value["runs"][0]["samples"][1]["accounting"]["medium_matter_units"][0]) + 1)
        self.reject(value, "sampled medium")

    def test_all_refused_unknown_identity_condition_keeps_24_rows(self):
        value = self.altered()
        for row in value["runs"]:
            if row["condition"] != "starvation":
                continue
            row.update(status="refused", stop_reason="input_or_admission_refused", error="synthetic refused input; no identity",
                       identity=None, canonical_config=None, checked_ticks=0, attempted_tick=None,
                       attempt_residuals=None, attempted_cell_ticks="0", committed_cell_ticks="0",
                       summary=None, samples=[], samples_truncated=False)
            for field in ("final_state_digest", "tick_reports_digest", "tick_reports_hashed", "tick_report_digest_encoding"):
                row.pop(field)
        value["batch"]["attempted_cell_ticks"] = str(sum(int(row["attempted_cell_ticks"]) for row in value["runs"]))
        value["batch"]["committed_cell_ticks"] = value["batch"]["attempted_cell_ticks"]
        value["batch"]["admitted_requested_steps"] = 20 * lab.STEPS
        rows = lab.validate(self.manifest, value, self.baseline)
        self.assertEqual(len(rows), 24)
        self.assertEqual(sum(row["identity"] is None for row in rows), 4)
        extracted = lab.comparisons(rows, {})
        self.assertTrue(all(row["available_pairs"] == 0 for row in extracted["landmark_comparisons"]
                            if row["condition"] == "starvation"))

    def test_exact_integers_and_batch_work_totals_cannot_be_faked(self):
        value = self.altered()
        value["runs"][0]["summary"]["lifecycle"]["fissions"] = 7
        self.reject(value, "integer string")
        value = self.altered()
        value["batch"]["attempted_cell_ticks"] = str(int(value["batch"]["attempted_cell_ticks"]) + 1)
        self.reject(value, "work totals")

    def test_unstarted_final_refusal_remains_in_all_24_rows_and_pairs(self):
        value = self.altered()
        row = value["runs"][-1]
        row.update(status="refused", stop_reason="batch_budget_before_start", error=None, checked_ticks=0,
                   attempted_tick=None, attempt_residuals=None, attempted_cell_ticks="0", committed_cell_ticks="0",
                   summary=None, samples=[], samples_truncated=False)
        for field in ("final_state_digest", "tick_reports_digest", "tick_reports_hashed", "tick_report_digest_encoding"):
            row.pop(field)
        total = sum(int(x["attempted_cell_ticks"]) for x in value["runs"])
        value["batch"]["attempted_cell_ticks"] = value["batch"]["committed_cell_ticks"] = str(total)
        value["budgets"]["max_cell_ticks"] = total
        rows = lab.validate(self.manifest, value, self.baseline)
        self.assertEqual(len(rows), 24)
        csv_rows = list(csv.DictReader(io.StringIO(lab.summary_csv(rows))))
        self.assertEqual(csv_rows[-1]["status"], "refused")
        self.assertEqual(csv_rows[-1]["structural_mass_mol"], "")
        extracted = lab.comparisons(rows, {})
        pairs = [x for x in extracted["landmark_comparisons"] if x["condition"] == "founder_k2"]
        self.assertTrue(all(x["expected_pairs"] == 4 and x["available_pairs"] == 3 for x in pairs))
        self.assertTrue(all(x["pairs"][-1]["missing"]["condition"]["reason"] == "run_refused" for x in pairs))

    def test_failed_attempt_must_not_publish_a_zero_residual(self):
        value = self.altered()
        row = value["runs"][0]
        row.update(status="numerical_failure", stop_reason="numerical_failure", error="synthetic rejected attempt",
                   attempted_tick=row["checked_ticks"] + 1)
        row["attempted_cell_ticks"] = str(int(row["committed_cell_ticks"]) + row["summary"]["living_cells"])
        self.reject(value, "rejected tick evidence")

    def test_strict_json_rejects_duplicate_keys_and_nonfinite_literals(self):
        for text in ('{"runs":[],"runs":[]}', '{"value":NaN}', '{"value":Infinity}'):
            with self.assertRaises(lab.ValidationError):
                lab.parse_json(text)

    def cli_fixture(self, directory, manifest_name="manifest.json"):
        root = Path(directory)
        paths = [root / manifest_name, root / "results.json", root / "baseline.toml"]
        source_directory = Path(__file__).resolve().parent
        paths[0].write_bytes((source_directory / "manifest.json").read_bytes())
        paths[1].write_bytes((source_directory / "results.json").read_bytes())
        source = Path(__file__).resolve().parents[3] / "configs/scenarios/cell-chamber.toml"
        paths[2].write_bytes(source.read_bytes())
        return paths

    def cli(self, paths, directory):
        argv = ["--manifest", str(paths[0]), "--results", str(paths[1]),
                "--baseline-config", str(paths[2]), "--output-dir", str(directory)]
        with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
            return lab.main(argv)

    def test_direct_output_collision_does_not_mutate_raw_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = self.cli_fixture(directory, "comparisons.json")
            original = [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths]
            self.assertEqual(self.cli(paths, directory), 1)
            self.assertEqual(original, [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths])
            self.assertFalse((Path(directory) / "summary.csv").exists())

    def test_symlink_and_hardlink_outputs_cannot_overwrite_raw_results(self):
        for link in ("symlink", "hardlink"):
            with self.subTest(link=link), tempfile.TemporaryDirectory() as directory:
                paths = self.cli_fixture(directory)
                target = Path(directory) / "summary.csv"
                if link == "symlink":
                    target.symlink_to(paths[1])
                else:
                    os.link(paths[1], target)
                original = [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths]
                self.assertEqual(self.cli(paths, directory), 1)
                self.assertEqual(original, [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths])
                self.assertFalse((Path(directory) / "comparisons.json").exists())

    def test_safe_outputs_preserve_raw_bytes_and_are_deterministic(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = self.cli_fixture(directory)
            original = [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths]
            self.assertEqual(self.cli(paths, directory), 0)
            outputs = [Path(directory) / "summary.csv", Path(directory) / "comparisons.json"]
            extracted = [p.read_bytes() for p in outputs]
            self.assertEqual(self.cli(paths, directory), 0)
            self.assertEqual(extracted, [p.read_bytes() for p in outputs])
            self.assertEqual(original, [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths])
            self.assertFalse(list(Path(directory).glob(".lab2-validator-*.tmp")))

    def test_cli_sha_anchors_reject_changed_inputs_without_writes(self):
        for index, expected in ((0, "manifest"), (2, "baseline")):
            with self.subTest(input=expected), tempfile.TemporaryDirectory() as directory:
                paths = self.cli_fixture(directory)
                paths[index].write_bytes(paths[index].read_bytes() + b"\n")
                original = [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths]
                self.assertEqual(self.cli(paths, directory), 1)
                self.assertEqual(original, [hashlib.sha256(p.read_bytes()).hexdigest() for p in paths])
                self.assertFalse((Path(directory) / "comparisons.json").exists())
                self.assertFalse((Path(directory) / "summary.csv").exists())


if __name__ == "__main__":
    unittest.main()
