"""Lossless archived frame codec. No simulation or recomputed float fields."""
from __future__ import annotations

import copy
import hashlib
import json
import math
from pathlib import Path
import re
import struct
import zlib

MAX_TICKS = 256
MAX_DECODED = 4 * 1024 * 1024
MAX_GZIP = 1024 * 1024
MAX_CHUNKS = 8192
MAX_TOTAL = 512 * 1024 * 1024
PILOT_TOTAL = 64 * 1024 * 1024
GZIP_HEADER = b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\xff"
CANONICAL_SHA = "4e72889e4cc2857a7f259a359b8a9bda349861f8bb72d6c9dd74fe6511ca03de"
ENGINE_REF = "b2024cef08f0aa910a711a41c6a37fbc7b2ba35a"
RECORDING_REF = "43f4a2df25c356ff387b71ac1961b516975f999b"
FROZEN_OBJECTS = {
    "crates/liminis-core": "bd2a0a0d55dcfff052e5573ddfc863f4d918d489",
    "configs/scenarios/cell-chamber.toml": "896f3415f24676e35e36431b20a0f9f304fb2259",
    "Cargo.toml": "4b7bef5047fafbdd10526a90de4a18952678a6a6",
    "Cargo.lock": "224f065c1bbf103a5cfec139a2f3313e2bf80559",
    "rust-toolchain.toml": "1fd6f1afaf9c17073863c2b9a9f50f90572a7edb",
    "crates/liminis/Cargo.toml": "d234294cf3b92e2ee1979017d5d2f212c82abace",
    "crates/liminis/examples/export_cell_replay.rs": "ebfed98f122b9a05b5c7d95ef70d9f9f81045c27",
}
STATIC = ("id", "parent_id", "generation", "birth_tick", "genome_key", "division_mass_mol")
DYNAMIC = ("age_s", "mass_mol", "energy_j", "mass_units", "energy_units", "starvation_s")
FRAME_FIELDS = {"tick", "sim_time", "summary", "residual", "accounting", "resources", "cells"}
HEX = re.compile(r"[0-9a-f]{64}\Z")
SHA1 = re.compile(r"[0-9a-f]{40}\Z")
INTEGER = re.compile(r"(?:0|-[1-9][0-9]*|[1-9][0-9]*)\Z")


def require(value, message):
    if not value:
        raise ValueError(message)


def fields(value, expected, optional=()):
    require(type(value) is dict and set(expected) <= set(value) <= set(expected) | set(optional), "unknown/missing fields")


def natural(value, maximum=(1 << 53) - 1):
    require(type(value) is int and 0 <= value <= maximum, "invalid unsigned integer")
    return value


def cell_id(value):
    require(type(value) is str and INTEGER.fullmatch(value) and 0 <= int(value) < 1 << 64, "invalid exact u64 cell ID")
    return value


def exact_integer(value):
    require(type(value) is str and INTEGER.fullmatch(value) and -(1 << 127) <= int(value) < 1 << 127, "invalid exact i128 string")


def finite(value):
    require(type(value) in (float, int) and (type(value) is not int or abs(value) < 1 << 53)
            and math.isfinite(value), "nonfinite/unsafe numeric value")


def exact(a, b):
    if type(a) is not type(b):
        return False
    if type(a) is float:
        return struct.pack("!d", a) == struct.pack("!d", b)
    if type(a) is dict:
        return a.keys() == b.keys() and all(exact(a[k], b[k]) for k in a)
    if type(a) is list:
        return len(a) == len(b) and all(exact(x, y) for x, y in zip(a, b))
    return a == b


def parse(data):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def number(value):
        result = float(value)
        finite(result)
        return result
    def constant(value):
        raise ValueError("nonfinite JSON constant " + value)
    return json.loads(data, object_pairs_hook=pairs, parse_float=number, parse_constant=constant)


def encode(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode("utf-8") + b"\n"


def validate_header(header):
    fields(header, ("type", "schema_version", "kind", "provenance", "identity", "model", "experiment", "canonical_config", "limitations"))
    require(header["type"] == "header" and type(header["schema_version"]) is int and header["schema_version"] == 1 and header["kind"] == "dense_cell_frame_stream", "unsupported producer stream")
    identity, experiment, provenance = header["identity"], header["experiment"], header["provenance"]
    fields(identity, ("seed", "config_hash", "world_format_version", "chamber_format", "source_commit", "scenario"))
    fields(experiment, ("steps", "sample_every", "dt_seconds"), ("checked_ticks", "frames", "matter_residual_max", "energy_residual_max"))
    fields(provenance, ("producer_commit", "producer_tree", "engine_ref", "legacy_recording_ref", "frozen_git_objects",
                        "source_verification", "runtime_rustc", "host_os", "host_arch", "profile", "build_attestation", "build_requirement"))
    fields(header["model"], ("environment", "volume_m3", "temperature_k", "spatial_positions"))
    require(identity["seed"] == "42" and identity["world_format_version"] == 30 and identity["chamber_format"] == 1
            and type(identity["world_format_version"]) is int and type(identity["chamber_format"]) is int and identity["source_commit"] == ENGINE_REF
            and identity["config_hash"] == "blake3:1802c0f129855749" and identity["scenario"] == "cell-chamber", "wrong archived identity")
    require(provenance["engine_ref"] == ENGINE_REF and provenance["legacy_recording_ref"] == RECORDING_REF
            and type(provenance["producer_commit"]) is str and SHA1.fullmatch(provenance["producer_commit"])
            and type(provenance["producer_tree"]) is str and SHA1.fullmatch(provenance["producer_tree"])
            and provenance["profile"] == "release" and provenance["build_attestation"] is False,
            "wrong producer provenance")
    require(provenance["frozen_git_objects"] == FROZEN_OBJECTS and type(provenance["runtime_rustc"]) is str
            and provenance["runtime_rustc"].startswith("rustc 1.97.1 ")
            and provenance["source_verification"] == "clean_HEAD_and_archived_Git_object_equality"
            and all(type(provenance[k]) is str and provenance[k] for k in ("host_os", "host_arch", "build_requirement")), "wrong archived byte/compiler anchors")
    require(type(header["canonical_config"]) is str and type(header["limitations"]) is list
            and all(type(item) is str for item in header["limitations"]), "invalid canonical/limitations metadata")
    require(hashlib.sha256(header["canonical_config"].encode()).hexdigest() == CANONICAL_SHA, "wrong archived canonical bytes")
    require(header["model"]["environment"] == "well_mixed" and header["model"]["spatial_positions"] is False
            and header["model"]["volume_m3"] == 1e-12 and header["model"]["temperature_k"] == 298.15, "wrong model")
    require(1 <= natural(experiment["steps"], 1_000_000) and natural(experiment["sample_every"]) == 1 and experiment["dt_seconds"] == 30.0, "wrong horizon/cadence")


def validate_genomes(genomes):
    require(type(genomes) is dict and 1 <= len(genomes) <= 1024, "invalid genome dictionary")
    for key, entry in genomes.items():
        require(type(key) is str, "invalid genotype key")
        fields(entry, ("genome", "phenotype"))
        fields(entry["genome"], ("kinetics", "base_growth_per_s", "base_km_mol_m3", "division_mass_mol", "maintenance_w",
                                 "capture_fraction", "division_cost_j", "starvation_tolerance_s"))
        fields(entry["phenotype"], ("growth_per_s", "km_mol_m3"))
        require(type(entry["genome"]["kinetics"]) is int and -128 <= entry["genome"]["kinetics"] <= 127, "invalid archived kinetics integer")
        for name, value in entry["genome"].items():
            if name != "kinetics":
                finite(value)
        for value in entry["phenotype"].values():
            finite(value)


def validate_frame(frame, genomes):
    fields(frame, FRAME_FIELDS)
    tick = natural(frame["tick"], 1_000_000)
    finite(frame["sim_time"])
    require(frame["sim_time"] == tick * 30.0, "wrong model time")
    require(frame["residual"] == (None if tick == 0 else {"matter": "0", "energy": "0"}), "unchecked ledger boundary")
    validate_genomes(genomes)
    require(type(frame["cells"]) is list and len(frame["cells"]) <= 512, "unbounded cell inventory")
    ids = []
    for cell in frame["cells"]:
        fields(cell, STATIC + DYNAMIC)
        ids.append(cell_id(cell["id"]))
        if cell["parent_id"] is not None:
            cell_id(cell["parent_id"])
        natural(cell["generation"])
        require(natural(cell["birth_tick"]) <= tick and cell["genome_key"] in genomes, "unknown birth/genotype")
        for key in ("division_mass_mol", "age_s", "mass_mol", "energy_j", "starvation_s"):
            finite(cell[key])
        exact_integer(cell["mass_units"])
        exact_integer(cell["energy_units"])
    require(len(set(ids)) == len(ids), "duplicate cell ID")
    summary = frame["summary"]
    fields(summary, ("living_cells", "births", "deaths", "divisions", "generation_max", "total_biomass_mol", "total_cell_energy_j"))
    require(natural(summary["living_cells"]) == len(ids), "inventory count mismatch")
    for key in ("births", "deaths", "divisions", "generation_max"):
        natural(summary[key])
    for key in ("total_biomass_mol", "total_cell_energy_j"):
        finite(summary[key])
    accounting = frame["accounting"]
    fields(accounting, ("matter_ids", "free_matter_units", "bath_heat_units", "medium_matter_units", "medium_energy_units", "growth_extent_units", "death_mass_units"))
    registry = accounting["matter_ids"]
    require(type(registry) is list and registry and len(set(registry)) == len(registry) and all(type(x) is str for x in registry), "invalid matter registry")
    for key in ("free_matter_units", "medium_matter_units"):
        require(type(accounting[key]) is list and len(accounting[key]) == len(registry), "invalid matter counters")
        for amount in accounting[key]:
            exact_integer(amount)
    for key in ("bath_heat_units", "medium_energy_units", "growth_extent_units", "death_mass_units"):
        exact_integer(accounting[key])
    require(type(frame["resources"]) is list, "invalid resources")
    resource_ids = set()
    for resource in frame["resources"]:
        fields(resource, ("id", "amount_mol", "concentration"))
        require(resource["id"] in registry and resource["id"] not in resource_ids, "invalid resource registry")
        resource_ids.add(resource["id"])
        finite(resource["amount_mol"])
        finite(resource["concentration"])


def static_row(cell):
    return [cell[k] for k in STATIC]


def dynamic_row(cell):
    return [cell[k] for k in DYNAMIC]


def keyframe(frame, genomes):
    return {"type": "keyframe", "schema_version": 1, "frame": {k: v for k, v in frame.items() if k != "cells"},
            "definitions": [static_row(c) for c in frame["cells"]], "values": [dynamic_row(c) for c in frame["cells"]], "genomes": genomes}


def delta(previous, frame, old_genomes, genomes):
    require(frame["tick"] == previous["tick"] + 1, "missing/repeated tick")
    require(all(k in genomes and exact(v, genomes[k]) for k, v in old_genomes.items()), "genome definition changed/disappeared")
    before = {c["id"]: c for c in previous["cells"]}
    after = {c["id"]: c for c in frame["cells"]}
    changed, born = [], []
    for cell in frame["cells"]:
        prior = before.get(cell["id"])
        if prior is None:
            born.append([static_row(cell), dynamic_row(cell)])
        else:
            require(exact(static_row(prior), static_row(cell)), "immutable cell definition changed")
            indexes = [i for i, name in enumerate(DYNAMIC) if not exact(prior[name], cell[name])]
            if indexes:
                changed.append([cell["id"], sum(1 << i for i in indexes), [cell[DYNAMIC[i]] for i in indexes]])
    removed = [c["id"] for c in previous["cells"] if c["id"] not in after]
    result = {"type": "delta", "set": {k: v for k, v in frame.items() if k != "cells" and not exact(previous[k], v)},
              "born": born, "removed": removed, "changed": changed,
              "genomes": {k: v for k, v in genomes.items() if k not in old_genomes}}
    order = [c["id"] for c in frame["cells"]]
    if order != [c["id"] for c in previous["cells"]]:
        result["order"] = order
    return result


class Chunk:
    """One incremental gzip member; trial copies bound final compressed bytes."""
    def __init__(self, first_tick):
        self.first_tick = first_tick
        self.last_tick = first_tick - 1
        self.count = self.decoded_bytes = self.crc = 0
        self.compressed = bytearray(GZIP_HEADER)
        self.compressor = zlib.compressobj(9, zlib.DEFLATED, -15)
        self.sha = hashlib.sha256()

    def append(self, record, tick):
        data = encode(record)
        trial = self.compressor.copy()
        emitted = trial.compress(data)
        final_size = len(self.compressed) + len(emitted) + len(trial.copy().flush()) + 8
        if self.count >= MAX_TICKS or self.decoded_bytes + len(data) > MAX_DECODED or final_size > MAX_GZIP:
            return False
        require(tick == self.last_tick + 1, "chunk tick gap")
        self.compressor = trial
        self.compressed.extend(emitted)
        self.sha.update(data)
        self.crc = zlib.crc32(data, self.crc)
        self.decoded_bytes += len(data)
        self.count += 1
        self.last_tick = tick
        return True

    def finish(self):
        require(self.count > 0, "empty chunk")
        data = bytes(self.compressed) + self.compressor.flush() + struct.pack("<II", self.crc & 0xffffffff, self.decoded_bytes)
        require(len(data) <= MAX_GZIP, "gzip cap exceeded")
        name = f"chunk-{self.first_tick:07d}-{self.last_tick:07d}.jsonl.gz"
        return data, {"path": name, "first_tick": self.first_tick, "last_tick": self.last_tick, "frames": self.count,
                      "gzip_bytes": len(data), "decoded_bytes": self.decoded_bytes,
                      "gzip_sha256": hashlib.sha256(data).hexdigest(), "decoded_sha256": self.sha.hexdigest()}


def inflate(data):
    require(len(data) <= MAX_GZIP, "compressed byte cap exceeded")
    require(data.startswith(GZIP_HEADER), "unsupported gzip header")
    decoder = zlib.decompressobj(31)
    raw = decoder.decompress(data, MAX_DECODED + 1)
    require(len(raw) <= MAX_DECODED and decoder.eof and not decoder.unconsumed_tail and not decoder.unused_data,
            "oversized, truncated, trailing or multiple gzip members")
    require(raw.endswith(b"\n"), "incomplete final record")
    return raw


def reconstruct(definition, values):
    require(type(definition) is list and len(definition) == len(STATIC) and type(values) is list and len(values) == len(DYNAMIC), "invalid cell row")
    return dict(zip(STATIC + DYNAMIC, definition + values))


def validate_descriptor(descriptor):
    fields(descriptor, ("path", "first_tick", "last_tick", "frames", "gzip_bytes", "decoded_bytes", "gzip_sha256", "decoded_sha256"))
    require(type(descriptor["path"]) is str and re.fullmatch(r"chunk-[0-9]{7}-[0-9]{7}\.jsonl\.gz", descriptor["path"]), "unsafe chunk path")
    first, last = natural(descriptor["first_tick"], 1_000_000), natural(descriptor["last_tick"], 1_000_000)
    require(descriptor["path"] == f"chunk-{first:07d}-{last:07d}.jsonl.gz", "path/range mismatch")
    require(last >= first and type(descriptor["frames"]) is int and 1 <= descriptor["frames"] == last - first + 1 <= MAX_TICKS, "invalid chunk range")
    require(type(descriptor["gzip_bytes"]) is int and 1 <= descriptor["gzip_bytes"] <= MAX_GZIP
            and type(descriptor["decoded_bytes"]) is int and 1 <= descriptor["decoded_bytes"] <= MAX_DECODED
            and type(descriptor["gzip_sha256"]) is str and HEX.fullmatch(descriptor["gzip_sha256"])
            and type(descriptor["decoded_sha256"]) is str and HEX.fullmatch(descriptor["decoded_sha256"]), "invalid chunk byte/digest fields")
    return first, last


def decode_chunk(data, descriptor):
    first, last = validate_descriptor(descriptor)
    require(len(data) == descriptor["gzip_bytes"] and hashlib.sha256(data).hexdigest() == descriptor["gzip_sha256"], "gzip integrity mismatch")
    raw = inflate(data)
    require(len(raw) == descriptor["decoded_bytes"] and hashlib.sha256(raw).hexdigest() == descriptor["decoded_sha256"], "decoded integrity mismatch")
    lines = raw.splitlines()
    require(len(lines) == descriptor["frames"], "chunk record count mismatch")
    state, genomes, seen = None, None, set()
    for index, line in enumerate(lines):
        record = parse(line)
        if index == 0:
            fields(record, ("type", "schema_version", "frame", "definitions", "values", "genomes"))
            require(record["type"] == "keyframe" and type(record["schema_version"]) is int and record["schema_version"] == 1
                    and type(record["definitions"]) is list and type(record["values"]) is list
                    and len(record["definitions"]) == len(record["values"]) <= 512, "missing independent keyframe")
            fields(record["frame"], FRAME_FIELDS - {"cells"})
            state = dict(record["frame"])
            state["cells"] = [reconstruct(a, b) for a, b in zip(record["definitions"], record["values"])]
            genomes = record["genomes"]
            seen = {c["id"] for c in state["cells"]}
        else:
            fields(record, ("type", "set", "born", "removed", "changed", "genomes"), ("order",))
            require(record["type"] == "delta" and type(record["set"]) is dict and set(record["set"]) <= FRAME_FIELDS - {"cells"}, "invalid frame patch")
            require(type(record["genomes"]) is dict and not (record["genomes"].keys() & genomes.keys()), "redefined genotype")
            genomes = {**genomes, **record["genomes"]}
            cells = {c["id"]: dict(c) for c in state["cells"]}
            require(type(record["changed"]) is list and type(record["born"]) is list
                    and len(record["changed"]) <= 512 and len(record["born"]) <= 512, "unbounded changes/births")
            require(type(record["removed"]) is list and len(set(record["removed"])) == len(record["removed"]), "duplicate removal")
            for removed in record["removed"]:
                require(removed in cells, "unknown removal")
                del cells[removed]
            touched = set()
            for row in record["changed"]:
                require(type(row) is list and len(row) == 3, "invalid change row")
                ident, mask, values = row
                require(ident in cells and ident not in touched and type(mask) is int and 0 < mask < (1 << len(DYNAMIC))
                        and type(values) is list and len(values) == mask.bit_count(), "invalid ID/mask/change arity")
                touched.add(ident)
                for name, value in zip((k for i, k in enumerate(DYNAMIC) if mask & (1 << i)), values):
                    cells[ident][name] = value
            for row in record["born"]:
                require(type(row) is list and len(row) == 2, "invalid birth row")
                cell = reconstruct(*row)
                ident = cell_id(cell["id"])
                require(ident not in seen, "reused cell ID")
                seen.add(ident)
                cells[ident] = cell
            order = record.get("order", [c["id"] for c in state["cells"]])
            require(type(order) is list and len(order) == len(cells) and len(set(order)) == len(order) and set(order) == cells.keys(), "incomplete/repeated cell order")
            state = {**state, **record["set"], "cells": [cells[ident] for ident in order]}
        require(state["tick"] == first + index, "decoded tick gap")
        validate_frame(state, genomes)
        yield copy.deepcopy(state), copy.deepcopy(genomes)


def read_chunk(directory, descriptor):
    # Validation rejects paths before any filesystem read; files are bounded too.
    validate_descriptor(descriptor)
    path = descriptor["path"]
    with (Path(directory) / path).open("rb") as stream:
        data = stream.read(MAX_GZIP + 1)
    return decode_chunk(data, descriptor)
