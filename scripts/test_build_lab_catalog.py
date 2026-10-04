#!/usr/bin/env python3
"""Delivery/failure checks на исторических LAB-2 bytes, без model runs."""

import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest

sys.dont_write_bytecode = True
import build_lab_catalog as builder


class LabDeliveryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = Path(__file__).resolve().parents[1] / "docs/experiments/lab-2"
        cls.raw = {name: (cls.directory / f"{name}.json").read_bytes()
                   for name in ("manifest", "results", "comparisons")}
        cls.manifest = json.loads(cls.raw["manifest"])
        cls.results = json.loads(cls.raw["results"])
        cls.comparisons = json.loads(cls.raw["comparisons"])

    def descriptor(self, manifest=None, results=None, comparisons=None):
        return builder.make_descriptor(manifest or self.manifest, results or self.results,
                                       comparisons or self.comparisons)

    def temporary_source(self, root):
        directory = root / "docs/experiments/lab-2"
        directory.mkdir(parents=True)
        for name, data in self.raw.items():
            (directory / f"{name}.json").write_bytes(data)
        return directory

    def test_published_files_are_original_bytes_and_check_is_read_only(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = self.temporary_source(root)
            builder.write_or_check(root)
            published = root / "site/data/lab-2"
            before = {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in published.iterdir()}
            self.assertEqual(set(before), {"manifest.json", "results.json", "comparisons.json", "descriptor.json"})
            self.assertEqual(sum(len(before[f"{name}.json"][0]) for name in self.raw), 6494550)
            for name, original in self.raw.items():
                self.assertEqual(before[f"{name}.json"][0], original)
                self.assertEqual((source / f"{name}.json").read_bytes(), original)
            builder.write_or_check(root, check=True)
            self.assertEqual(before, {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in published.iterdir()})

    def test_descriptor_is_deterministic_and_source_is_distinct_from_delivery(self):
        first = builder.build_artifacts()["descriptor.json"]
        self.assertEqual(first, builder.build_artifacts()["descriptor.json"])
        descriptor = json.loads(first)
        self.assertEqual(descriptor["kind"], "aggregate_cell_lab_descriptor")
        self.assertEqual(descriptor["source"]["source_commit"], self.results["source_commit"])
        self.assertEqual(descriptor["source"]["provenance"], self.results["provenance"])
        self.assertIs(descriptor["source"]["provenance"]["build_attestation"], False)
        self.assertNotIn("source_commit", descriptor["delivery"])
        self.assertIs(descriptor["delivery"]["numerical_recalculation"], False)
        self.assertEqual(descriptor["capabilities"], {"aggregate_samples": True, "individual_cells": False,
                         "spatial_positions": False, "full_genomes": False, "resumable_states": False,
                         "event_reconstruction": False})
        for name, data in self.raw.items():
            self.assertEqual(descriptor["files"][name], {"url": f"./data/lab-2/{name}.json", "bytes": len(data),
                             "sha256": hashlib.sha256(data).hexdigest()})

    def test_full_declared_configs_match_original_toml_and_preserve_units(self):
        descriptor = self.descriptor()
        for config in descriptor["conditions"]:
            declaration = next(x for x in self.manifest["runs"] if x["condition"] == config["condition"])
            original = declaration["scenario_toml"].encode("utf-8")
            run = next(x for x in self.results["runs"] if x["condition"] == config["condition"])
            self.assertEqual(config["declared_parameters"], tomllib.loads(original.decode("utf-8")))
            self.assertEqual((config["toml_bytes"], config["toml_sha256"]),
                             (len(original), hashlib.sha256(original).hexdigest()))
            self.assertEqual(config["canonical_config_sha256"], hashlib.sha256(run["canonical_config"].encode()).hexdigest())
        baseline = descriptor["conditions"][0]
        self.assertEqual(baseline["declared_parameters"]["initial"]["concentration"]["FOOD"], 10.0)
        self.assertEqual(baseline["identity"]["units"]["concentration"], "mol/m^3")
        self.assertEqual(baseline["declared_parameters"]["chamber"]["max_cells"], 512)
        self.assertEqual(self.results["budgets"]["max_cells"], 4096)
        observed_food = next(x for x in self.results["runs"][0]["samples"][0]["matter"] if x["id"] == "FOOD")
        self.assertLess(observed_food["free_mol"], 1e-10)

    def test_run_ids_seeds_censors_and_actual_endpoints_are_retained(self):
        descriptor = self.descriptor()
        self.assertEqual([x["run_id"] for x in descriptor["runs"]], [x["run_id"] for x in self.manifest["runs"]])
        self.assertEqual(sum(x["sample_count"] for x in descriptor["runs"]), 4044)
        self.assertTrue(all(type(x["seed"]) is str for x in descriptor["runs"]))
        self.assertTrue(any("_" in x["run_id"] for x in descriptor["runs"]))
        for metadata in descriptor["runs"]:
            run = self.results["runs"][metadata["results_index"]]
            self.assertEqual(metadata["run_id"], run["run_id"])
            if metadata["condition"] == "starvation":
                self.assertEqual((metadata["status"], metadata["stop_reason"], metadata["checked_ticks"], metadata["sample_count"]),
                                 ("extinct", "extinction", 454, 6))
                self.assertEqual([x["tick"] for x in run["samples"]], [0, 100, 200, 300, 400, 454])
            else:
                self.assertEqual((metadata["status"], metadata["stop_reason"], metadata["checked_ticks"], metadata["sample_count"]),
                                 ("censored", "requested_horizon_reached", 20000, 201))

    def test_original_integer_strings_and_unknown_genesis_ledgers_survive_byte_copy(self):
        copied = json.loads(builder.build_artifacts()["results.json"])
        self.assertEqual(copied, self.results)
        scale = self.descriptor()["conditions"][0]["identity"]["energy_units_per_joule"]
        self.assertEqual(scale, "147573952589676412928")
        self.assertIsInstance(scale, str)
        genesis = copied["runs"][0]["samples"][0]
        self.assertIsNone(genesis["ledger"]["matter_residual_last_checked"])
        self.assertIsNone(genesis["ledger"]["energy_residual_last_checked"])
        self.assertEqual(genesis["lifecycle"]["founders"], "8")

    def test_each_source_pin_fails_before_output_even_for_valid_json(self):
        for name in self.raw:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = self.temporary_source(root)
                path = source / f"{name}.json"
                data = path.read_bytes()
                # Валидный JSON той же длины, включая compact results без newline.
                altered = data.replace(b'"schema_version"', b'"Schema_version"', 1)
                self.assertNotEqual(hashlib.sha256(altered).digest(), hashlib.sha256(data).digest())
                json.loads(altered)
                path.write_bytes(altered)
                with self.assertRaisesRegex(builder.CatalogError, "Immutable source pin differs"):
                    builder.write_or_check(root)
                self.assertFalse((root / "site/data/lab-2").exists())

    def test_check_rejects_stale_raw_and_descriptor_without_repair(self):
        for name in ("results.json", "descriptor.json"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.temporary_source(root)
                builder.write_or_check(root)
                path = root / "site/data/lab-2" / name
                stale = path.read_bytes().replace(b'"schema_version"', b'"Schema_version"', 1)
                path.write_bytes(stale)
                with self.assertRaisesRegex(builder.CatalogError, "Stale LAB delivery"):
                    builder.write_or_check(root, check=True)
                self.assertEqual(path.read_bytes(), stale)

    def test_missing_copy_and_unexpected_output_fail_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.temporary_source(root)
            builder.write_or_check(root)
            directory = root / "site/data/lab-2"
            (directory / "manifest.json").unlink()
            with self.assertRaises(builder.CatalogError):
                builder.write_or_check(root, check=True)
            (directory / "unexpected.json").write_bytes(b"{}")
            with self.assertRaisesRegex(builder.CatalogError, "Unexpected LAB output files"):
                builder.write_or_check(root)
            self.assertFalse((directory / "manifest.json").exists())

    def test_duplicate_and_nonfinite_json_are_rejected(self):
        for data in (b'{"seed":"1","seed":"7"}', b'{"value":NaN}'):
            with self.subTest(data=data), self.assertRaises(builder.CatalogError):
                builder.parse_json(data)

    def test_run_reordering_and_new_source_identity_are_rejected(self):
        results = copy.deepcopy(self.results)
        results["runs"][0], results["runs"][1] = results["runs"][1], results["runs"][0]
        with self.assertRaisesRegex(builder.CatalogError, "Run declaration/order join differs"):
            self.descriptor(results=results)
        results = copy.deepcopy(self.results)
        results["source_commit"] = "5beaafb2c4f717862b62e67fd3e23e7d81860535"
        with self.assertRaisesRegex(builder.CatalogError, "Historical scientific source_commit differs"):
            self.descriptor(results=results)
        results = copy.deepcopy(self.results)
        results["runs"][0]["identity"]["world_format_version"] = 31
        with self.assertRaisesRegex(builder.CatalogError, "Historical LAB identity differs"):
            self.descriptor(results=results)

    def test_toml_and_canonical_bytes_are_not_normalized(self):
        manifest = copy.deepcopy(self.manifest)
        manifest["runs"][6]["scenario_toml"] += "\n"
        with self.assertRaisesRegex(builder.CatalogError, "Materialized TOML pin differs"):
            self.descriptor(manifest=manifest)
        results = copy.deepcopy(self.results)
        results["runs"][6]["canonical_config"] += "\n"
        with self.assertRaisesRegex(builder.CatalogError, "Canonical config pin differs"):
            self.descriptor(results=results)

    def test_condition_identity_and_input_digest_must_match_across_seeds(self):
        for key in ("identity", "input_toml_digest"):
            with self.subTest(key=key):
                results = copy.deepcopy(self.results)
                if key == "identity":
                    results["runs"][6][key]["config_hash"] = "blake3:0000000000000000"
                else:
                    results["runs"][6][key] = "blake3:" + "0" * 64
                with self.assertRaisesRegex(builder.CatalogError, "inconsistent across seeds"):
                    self.descriptor(results=results)

    def test_extinction_final_sample_and_latest_common_tick_are_separate(self):
        results = copy.deepcopy(self.results)
        results["runs"][2]["samples"].pop()
        with self.assertRaisesRegex(builder.CatalogError, "Exact retained sample ticks differ"):
            self.descriptor(results=results)
        comparisons = copy.deepcopy(self.comparisons)
        pair = next(x for x in comparisons["latest_common_samples"] if x["condition"] == "starvation")
        self.assertEqual(pair["tick"], 400)
        pair["tick"] = 454
        with self.assertRaisesRegex(builder.CatalogError, "Comparison seed/condition/tick join differs"):
            self.descriptor(comparisons=comparisons)

    def test_missing_pairs_ranges_and_ids_cannot_be_fabricated(self):
        for corruption in ("availability", "delta", "range", "id"):
            with self.subTest(corruption=corruption):
                comparisons = copy.deepcopy(self.comparisons)
                row = next(x for x in comparisons["landmark_comparisons"] if x["condition"] == "starvation" and x["tick"] == 500)
                pair = row["pairs"][0]
                if corruption == "availability":
                    pair["available"] = True
                elif corruption == "delta":
                    pair["delta"] = {}
                elif corruption == "range":
                    row["ranges"]["delta"]["living_cells"] = {"count": 0, "min": 0, "max": 0, "width": 0}
                else:
                    pair["baseline_run_id"] = "seed-7-baseline"
                with self.assertRaises(builder.CatalogError):
                    self.descriptor(comparisons=comparisons)

    def test_cell_work_integer_strings_are_not_narrowed_or_coerced(self):
        results = copy.deepcopy(self.results)
        results["runs"][0]["attempted_cell_ticks"] = int(results["runs"][0]["attempted_cell_ticks"])
        with self.assertRaisesRegex(builder.CatalogError, "not an integer string"):
            self.descriptor(results=results)


if __name__ == "__main__":
    unittest.main()
