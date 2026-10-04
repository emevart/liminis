#!/usr/bin/env python3
"""Codec fixtures only: existing observations and synthetic values; no model steps."""
import copy
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import random
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


codec = load("dense-recording-codec")
packer = load("dense-recording-pack")
decoder = load("dense-recording-decode")
legacy = codec.parse((ROOT / "site/data/cell-chamber-seed-42.json").read_bytes())


def header(steps):
    return {"type": "header", "schema_version": 1, "kind": "dense_cell_frame_stream",
            "identity": {**legacy["identity"], "source_commit": codec.ENGINE_REF},
            "model": legacy["model"], "canonical_config": legacy["canonical_config"], "limitations": [],
            "experiment": {"steps": steps, "sample_every": 1, "dt_seconds": 30.0},
            "provenance": {"producer_commit": "a" * 40, "producer_tree": "b" * 40,
                           "engine_ref": codec.ENGINE_REF, "legacy_recording_ref": codec.RECORDING_REF,
                           "frozen_git_objects": codec.FROZEN_OBJECTS, "runtime_rustc": "rustc 1.97.1 (codec fixture)",
                           "source_verification": "clean_HEAD_and_archived_Git_object_equality",
                           "host_os": "fixture", "host_arch": "fixture", "build_requirement": "fixture only, not an actual producer run",
                           "profile": "release", "build_attestation": False}}


def fixture(tick):
    # Synthetic codec sequence, not a reconstruction of the unseen legacy ticks.
    frame = copy.deepcopy(legacy["frames"][0])
    frame["tick"] = tick
    frame["sim_time"] = tick * 30.0
    frame["residual"] = None if tick == 0 else {"matter": "0", "energy": "0"}
    for index, cell in enumerate(frame["cells"]):
        cell["age_s"] = tick * 30.0
        cell["energy_units"] = str(10**28 + tick + index)
        cell["energy_j"] = (tick + index) / 17.0
    return frame


def stream(frames, genomes=None, complete=True):
    genomes = legacy["genomes"] if genomes is None else genomes
    steps = len(frames) - 1
    records = [header(steps)] + [{"type": "frame", "frame": f, "genomes": genomes} for f in frames]
    if complete:
        records.append({"type": "end", "checked_ticks": steps, "frames": steps + 1,
                        "matter_residual_max": "0", "energy_residual_max": "0"})
    return io.BytesIO(b"".join(codec.encode(record) for record in records))


def member(records, first):
    chunk = codec.Chunk(first)
    for i, record in enumerate(records):
        assert chunk.append(record, first + i)
    return chunk.finish()


def mutated_member(records, first=0):
    data = gzip.compress(b"".join(codec.encode(r) for r in records), mtime=0)
    data = codec.GZIP_HEADER + data[10:]
    raw = gzip.decompress(data)
    return data, {"path": f"chunk-{first:07d}-{first+len(records)-1:07d}.jsonl.gz", "first_tick": first,
                  "last_tick": first + len(records) - 1, "frames": len(records), "gzip_bytes": len(data),
                  "decoded_bytes": len(raw), "gzip_sha256": hashlib.sha256(data).hexdigest(),
                  "decoded_sha256": hashlib.sha256(raw).hexdigest()}


class DenseCodecTests(unittest.TestCase):
    def test_total_caps_enforce_exact_decimal_budget_and_thresholds_before_frames(self):
        cases = ((1, 64 * 1024 * 1024), (10_000, 64 * 1024 * 1024),
                 (10_001, 512 * 1024 * 1024), (100_000, 512 * 1024 * 1024),
                 (100_001, 3_000_000_000), (1_000_000, 3_000_000_000))
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "data"
            for horizon, cap in cases:
                self.assertEqual(codec.total_cap(horizon), cap)
                for custom in (cap + 1, 3 * 1024**3, True, 0):
                    with self.subTest(horizon=horizon, custom=custom), self.assertRaisesRegex(ValueError, "budget"):
                        packer.pack(io.BytesIO(codec.encode(header(horizon))), output, custom)
                    self.assertFalse(output.exists())
                for custom in (None, cap, cap - 1):
                    with self.subTest(horizon=horizon, custom=custom), self.assertRaisesRegex(ValueError, "producer record"):
                        packer.pack(io.BytesIO(codec.encode(header(horizon))), output, custom)
                    self.assertFalse(output.exists())
            for invalid in (0, 1_000_001, True):
                with self.assertRaises(ValueError): codec.total_cap(invalid)

    def test_old_producer_artifact_decodes_without_metadata_or_byte_changes(self):
        # Execute only the old stdlib packer on synthetic codec values, no model.
        with tempfile.TemporaryDirectory() as temporary:
            old_scripts = Path(temporary) / "old"
            old_scripts.mkdir()
            for name in ("dense-recording-codec", "dense-recording-pack"):
                path = old_scripts / (name + ".py")
                path.write_bytes(subprocess.check_output(["git", "show", "bd2a7f25e61abd713109171fc25b7c3de86003ab:scripts/" + path.name], cwd=ROOT))
            spec = importlib.util.spec_from_file_location("old_dense_packer", old_scripts / "dense-recording-pack.py")
            old_packer = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(old_packer)
            output = Path(temporary) / "artifact"
            originals = [fixture(0), fixture(1)]
            old_packer.pack(stream(originals), output)
            before = {p.name: p.read_bytes() for p in output.iterdir()}
            actual = list(decoder.frames(output / "horizon-1.json"))
            self.assertTrue(all(codec.exact(frame, original) for (frame, _), original in zip(actual, originals)))
            self.assertEqual(before, {p.name: p.read_bytes() for p in output.iterdir()})

    def test_prefix_budget_uses_full_index_horizon_and_keeps_legacy_caps(self):
        # Metadata-only unread ranges: no 100k/million trajectory is constructed.
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "artifact"
            packer.pack(stream([fixture(0), fixture(1)]), output)
            manifest = codec.parse((output / "horizon-1.json").read_bytes())
            index = codec.parse((output / "index.json").read_bytes())
            manifest["experiment"].update(steps=10_000, checked_ticks=10_000, frames=10_001)
            for first in range(2, 10_001, 256):
                last = min(first + 255, 10_000)
                manifest["chunks"].append({"path": f"chunk-{first:07d}-{last:07d}.jsonl.gz", "first_tick": first,
                                           "last_tick": last, "frames": last - first + 1, "gzip_bytes": 100,
                                           "decoded_bytes": 1000, "gzip_sha256": "a" * 64, "decoded_sha256": "b" * 64})
            path = output / "horizon-10000.json"
            for full_horizon, cap, gzip_total in ((100_000, 512 * 1024 * 1024, 244_000_000),
                                                 (1_000_000, 512 * 1024 * 1024, 244_000_000),
                                                 (1_000_000, 3_000_000_000, 2_800_000_000),
                                                 (1_000_000, 2_900_000_000, 2_800_000_000)):
                manifest["bounds"]["total_artifact_bytes_cap"] = cap
                data = codec.encode(manifest)
                path.write_bytes(data)
                index["manifests"] = [{"path": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                                       "first_tick": 0, "last_tick": 10_000, "frames": 10_001},
                                      {"path": f"horizon-{full_horizon}.json", "bytes": 2000, "sha256": "c" * 64,
                                       "first_tick": 0, "last_tick": full_horizon, "frames": full_horizon + 1}]
                index.update(unique_chunks=4000, unique_gzip_bytes=gzip_total, decoded_chunk_bytes=10_000_000_000)
                (output / "index.json").write_bytes(codec.encode(index))
                selected = list(decoder.frames(path, 0, 0))
                self.assertTrue(codec.exact(selected[0][0], fixture(0)))
            for full_horizon, cap, gzip_total in ((10_000, 512 * 1024 * 1024, 1_000_000),
                                                 (100_000, 3_000_000_000, 1_000_000),
                                                 (1_000_000, 3_000_000_001, 2_800_000_000),
                                                 (1_000_000, 512 * 1024 * 1024, 2_800_000_000),
                                                 (1_000_000, 3_000_000_000, 3_000_000_000)):
                manifest["bounds"]["total_artifact_bytes_cap"] = cap
                data = codec.encode(manifest)
                path.write_bytes(data)
                index["manifests"][0].update(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
                if full_horizon == 10_000:
                    index["manifests"] = index["manifests"][:1]
                else:
                    index["manifests"] = [index["manifests"][0], {"path": f"horizon-{full_horizon}.json", "bytes": 2000,
                                          "sha256": "c" * 64, "first_tick": 0, "last_tick": full_horizon, "frames": full_horizon + 1}]
                index["unique_gzip_bytes"] = gzip_total
                (output / "index.json").write_bytes(codec.encode(index))
                with self.subTest(full_horizon=full_horizon, cap=cap, gzip_total=gzip_total), self.assertRaises(ValueError):
                    list(decoder.frames(path, 0, 0))

    def test_every_original_frame_and_known_genome_entry_roundtrips_exactly(self):
        for frame in legacy["frames"]:
            data, descriptor = member([codec.keyframe(frame, legacy["genomes"])], frame["tick"])
            decoded, genomes = next(codec.decode_chunk(data, descriptor))
            self.assertTrue(codec.exact(decoded, frame))
            self.assertTrue(codec.exact(genomes, legacy["genomes"]))
            self.assertEqual(gzip.decompress(data), codec.encode(codec.keyframe(frame, legacy["genomes"])))

    def test_changes_births_removals_reordering_and_transient_genotype(self):
        a, b = fixture(0), fixture(1)
        b["cells"].pop(0)
        child = copy.deepcopy(b["cells"][0])
        child.update(id="18446744073709551615", parent_id=a["cells"][0]["id"], birth_tick=1,
                     generation=4294967296, genome_key="transient")
        b["cells"].insert(0, child)
        b["cells"].reverse()
        b["summary"]["living_cells"] = len(b["cells"])
        genomes = {**legacy["genomes"], "transient": copy.deepcopy(legacy["genomes"]["K0"])}
        data, descriptor = member([codec.keyframe(a, legacy["genomes"]), codec.delta(a, b, legacy["genomes"], genomes)], 0)
        result = list(codec.decode_chunk(data, descriptor))
        self.assertTrue(codec.exact(result[-1][0], b))
        self.assertTrue(codec.exact(result[-1][1], genomes))
        self.assertEqual(result[-1][0]["cells"][-1]["id"], "18446744073709551615")
        self.assertEqual(result[-1][0]["cells"][-1]["generation"], 4294967296)

    def test_negative_zero_binary64_edges_and_exact_large_integer_strings(self):
        a, b = fixture(0), fixture(1)
        a["cells"][0]["energy_j"] = 0.0
        b["cells"][0]["energy_j"] = -0.0
        b["cells"][1]["energy_j"] = float.fromhex("0x1.0000000000001p-1022")
        b["cells"][2]["energy_j"] = 1.7976931348623157e308
        b["accounting"]["medium_energy_units"] = str(-(1 << 127))
        data, descriptor = member([codec.keyframe(a, legacy["genomes"]), codec.delta(a, b, legacy["genomes"], legacy["genomes"])], 0)
        decoded = list(codec.decode_chunk(data, descriptor))[-1][0]
        self.assertTrue(codec.exact(decoded, b))
        self.assertEqual(struct.pack("!d", decoded["cells"][0]["energy_j"]), struct.pack("!d", -0.0))

    def test_pack_bounded_chunks_seek_and_repeat_without_a_full_frame_array(self):
        frames = [fixture(tick) for tick in range(300)]
        with tempfile.TemporaryDirectory() as temporary:
            first, second = Path(temporary) / "first", Path(temporary) / "second"
            report = packer.pack(stream(frames), first)
            packer.pack(stream(frames), second)
            self.assertEqual(report["chunks"], 2)
            manifest = codec.parse((first / "horizon-299.json").read_bytes())
            self.assertEqual([(c["first_tick"], c["last_tick"]) for c in manifest["chunks"]], [(0, 255), (256, 299)])
            for path in first.iterdir():
                self.assertEqual(path.read_bytes(), (second / path.name).read_bytes())
            selected = list(decoder.frames(first / "horizon-299.json", 270, 272))
            self.assertEqual([f["tick"] for f, _ in selected], [270, 271, 272])
            for (observed, _), original in zip(selected, frames[270:273]):
                self.assertTrue(codec.exact(observed, original))

    def test_adaptive_decoded_and_gzip_caps_begin_new_independent_keyframes(self):
        frames = [fixture(tick) for tick in range(12)]
        rng = random.Random(1)
        for frame in frames:
            for cell in frame["cells"]:
                cell["energy_j"] = rng.random()
        size = max(len(codec.encode(codec.keyframe(f, legacy["genomes"]))) for f in frames)
        gz = max(len(member([codec.keyframe(f, legacy["genomes"])], f["tick"])[0]) for f in frames)
        for name, cap in [("MAX_DECODED", size + 100), ("MAX_GZIP", gz + 40)]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary, patch.object(packer.codec, name, cap), patch.object(decoder.codec, name, cap):
                output = Path(temporary) / "data"
                result = packer.pack(stream(frames), output)
                self.assertGreater(result["chunks"], 1)
                actual = list(decoder.frames(output / "horizon-11.json"))
                self.assertTrue(all(codec.exact(a, b) for (a, _), b in zip(actual, frames)))

    def test_incomplete_input_and_total_chunk_caps_never_publish_a_manifest(self):
        frames = [fixture(i) for i in range(3)]
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "data"
            for value, cap in [(stream(frames, complete=False), None), (stream(frames), 1)]:
                with self.assertRaises(ValueError):
                    packer.pack(value, output, cap)
                self.assertFalse(output.exists())
                self.assertFalse(list(Path(temporary).glob(".dense-recording-*")))
            with patch.object(packer.codec, "MAX_TICKS", 1), patch.object(packer.codec, "MAX_CHUNKS", 1):
                with self.assertRaisesRegex(ValueError, "chunk cap"):
                    packer.pack(stream(frames), output)
            self.assertFalse(output.exists())
            with patch.object(packer.codec, "MAX_DECODED", 10):
                with self.assertRaisesRegex(ValueError, "keyframe"):
                    packer.pack(stream(frames), output)
            self.assertFalse(output.exists())

    def test_retired_ids_cannot_reappear_at_an_independent_chunk_boundary(self):
        frames = [fixture(i) for i in range(3)]
        frames[1]["cells"].pop(0)
        frames[1]["summary"]["living_cells"] -= 1
        with tempfile.TemporaryDirectory() as temporary, patch.object(packer.codec, "MAX_TICKS", 1):
            output = Path(temporary) / "data"
            with self.assertRaisesRegex(ValueError, "retired ID"):
                packer.pack(stream(frames), output)
            self.assertFalse(output.exists())

    def test_seek_validates_unread_chunk_descriptors_and_complete_index_counts(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "data"
            packer.pack(stream([fixture(i) for i in range(300)]), output)
            path = output / "horizon-299.json"
            original = codec.parse(path.read_bytes())
            original_index = codec.parse((output / "index.json").read_bytes())
            for kind in ("unsafe_path", "wrong_frames", "wrong_bytes", "index_count"):
                manifest, index = copy.deepcopy(original), copy.deepcopy(original_index)
                if kind == "unsafe_path": manifest["chunks"][1]["path"] = "../hidden.jsonl.gz"
                if kind == "wrong_frames": manifest["chunks"][1]["frames"] = 1
                if kind == "wrong_bytes": manifest["chunks"][1]["decoded_bytes"] = codec.MAX_DECODED + 1
                if kind == "index_count": index["unique_chunks"] += 1
                data = codec.encode(manifest)
                index["manifests"][0].update(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
                path.write_bytes(data)
                (output / "index.json").write_bytes(codec.encode(index))
                with self.subTest(kind=kind), self.assertRaises(ValueError):
                    list(decoder.frames(path, 0, 0))

    def test_corruption_truncation_extra_member_and_decompression_bomb_fail(self):
        data, descriptor = member([codec.keyframe(fixture(0), legacy["genomes"])], 0)
        for broken in [data[:-1], data + data, data[:-8] + b"badcrc!!"]:
            with self.assertRaises(ValueError):
                list(codec.decode_chunk(broken, descriptor))
        with self.assertRaises(ValueError):
            bomb = gzip.compress(b"x" * (codec.MAX_DECODED + 1), mtime=0)
            codec.inflate(codec.GZIP_HEADER + bomb[10:])
        with self.assertRaises(ValueError):
            codec.inflate(data + data)
        with self.assertRaisesRegex(ValueError, "header"):
            codec.inflate(data[:9] + b"\x03" + data[10:])

    def test_valid_hashes_cannot_hide_bad_masks_order_ids_or_ticks(self):
        a, b = fixture(0), fixture(1)
        base = [codec.keyframe(a, legacy["genomes"]), codec.delta(a, b, legacy["genomes"], legacy["genomes"])]
        for kind in ("mask", "arity", "order", "tick", "changed_id", "duplicate_change", "genotype"):
            records = copy.deepcopy(base)
            change = records[1]
            if kind == "mask": change["changed"][0][1] = 1 << len(codec.DYNAMIC)
            if kind == "arity": change["changed"][0][2].append(0)
            if kind == "order": change["order"] = [a["cells"][0]["id"]] * len(a["cells"])
            if kind == "tick": change["set"]["tick"] = 2
            if kind == "changed_id": change["changed"][0][0] = "18446744073709551615"
            if kind == "duplicate_change": change["changed"].append(copy.deepcopy(change["changed"][0]))
            if kind == "genotype": change["genomes"] = {"K0": {}}
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                data, descriptor = mutated_member(records)
                list(codec.decode_chunk(data, descriptor))

    def test_unknown_keys_nonfinite_values_and_integer_narrowing_are_rejected(self):
        for text in (b'{"a":1,"a":2}', b'{"a":NaN}', b'{"a":1e999}'):
            with self.assertRaises(ValueError): codec.parse(text)
        for value in (42, "18446744073709551616", "01", "-1"):
            with self.assertRaises(ValueError): codec.cell_id(value)
        with self.assertRaises(ValueError): codec.natural(1 << 53)
        with self.assertRaises(ValueError): codec.finite(1 << 53)
        a, b = fixture(0), fixture(1)
        b["cells"][0]["generation"] += 1
        with self.assertRaisesRegex(ValueError, "immutable"):
            codec.delta(a, b, legacy["genomes"], legacy["genomes"])

    def test_wrong_archive_and_existing_output_are_refused_before_writes(self):
        wrong = header(1)
        wrong["identity"]["world_format_version"] = 31
        with self.assertRaises(ValueError): codec.validate_header(wrong)
        wrong = header(1)
        wrong["canonical_config"] += "\n"
        with self.assertRaises(ValueError): codec.validate_header(wrong)
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            sentinel = output / "old.json"
            sentinel.write_bytes(b"immutable")
            with self.assertRaises(ValueError): packer.pack(stream([fixture(0), fixture(1)]), output)
            self.assertEqual(sentinel.read_bytes(), b"immutable")

    def test_forced_horizon_prefix_shares_chunks_and_checks_manifest_integrity(self):
        steps = 10002
        def records():
            yield header(steps)
            for tick in range(steps + 1):
                frame = fixture(tick)
                frame["cells"] = []
                frame["summary"]["living_cells"] = 0
                yield {"type": "frame", "frame": frame, "genomes": legacy["genomes"]}
            yield {"type": "end", "checked_ticks": steps, "frames": steps + 1,
                   "matter_residual_max": "0", "energy_residual_max": "0"}
        class StreamingFixture:
            def __init__(self): self.records = iter(records())
            def readline(self, maximum): return codec.encode(next(self.records))
            def read(self, maximum): return b""
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "data"
            packer.pack(StreamingFixture(), output)
            short = codec.parse((output / "horizon-10000.json").read_bytes())
            long = codec.parse((output / "horizon-10002.json").read_bytes())
            self.assertEqual(short["chunks"], long["chunks"][:-1])
            self.assertEqual(short["chunks"][-1]["last_tick"], 10000)
            self.assertEqual(long["chunks"][-1]["first_tick"], 10001)
            self.assertEqual([f["tick"] for f, _ in decoder.frames(output / "horizon-10000.json", 9999, 10000)], [9999, 10000])
            manifest = output / "horizon-10000.json"
            manifest.write_bytes(manifest.read_bytes() + b" ")
            with self.assertRaisesRegex(ValueError, "integrity"):
                list(decoder.frames(manifest, 10000, 10000))

    def test_archived_frame_builder_is_unchanged_and_no_float_derivation_exists(self):
        old = (ROOT / "crates/liminis/examples/export_cell_replay.rs").read_text()
        new = (ROOT / "crates/liminis/examples/export_dense_cell_replay.rs").read_text()
        self.assertEqual(old[old.index("fn frame("):old.index("\n#[cfg(test)]")].strip(), new[new.index("fn frame("):].strip())


if __name__ == "__main__":
    unittest.main()
