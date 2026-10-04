#!/usr/bin/env python3
"""Pack stdin from the clean archived Rust producer, never run simulation here."""
import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("dense_codec", Path(__file__).with_name("dense-recording-codec.py"))
codec = importlib.util.module_from_spec(spec)
spec.loader.exec_module(codec)


def read_record(stream):
    maximum = 4 * 1024 * 1024  # Source line and encoded chunk have independent caps.
    line = stream.readline(maximum + 1)
    codec.require(line and len(line) <= maximum and line.endswith(b"\n"), "missing/truncated/oversized producer record")
    return codec.parse(line)


def pack(stream, output, max_bytes=None):
    output = Path(output)
    codec.require(not output.exists(), "output directory already exists; never overwrite published data")
    header = read_record(stream)
    codec.validate_header(header)
    steps = header["experiment"]["steps"]
    cap = codec.total_cap(steps)
    max_bytes = cap if max_bytes is None else max_bytes
    codec.require(type(max_bytes) is int and 1 <= max_bytes <= cap, "invalid total byte budget")
    horizons = sorted({steps, *(h for h in (10_000, 100_000, 1_000_000) if h <= steps)})
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".dense-recording-", dir=output.parent) as temporary:
        staging = Path(temporary) / "complete"
        staging.mkdir()
        chunks, prefixes, chunk = [], {}, None
        previous, old_genomes, highest_id = None, None, -1
        total_bytes = 0

        def flush():
            nonlocal chunk, total_bytes
            data, descriptor = chunk.finish()
            codec.require(len(chunks) < codec.MAX_CHUNKS, "8192 chunk cap exceeded")
            codec.require(total_bytes + len(data) <= max_bytes, "total gzip byte budget exceeded")
            (staging / descriptor["path"]).write_bytes(data)
            total_bytes += len(data)
            chunks.append(descriptor)
            chunk = None

        for tick in range(steps + 1):
            record = read_record(stream)
            codec.fields(record, ("type", "frame", "genomes"))
            codec.require(record["type"] == "frame", "missing producer frame")
            frame, genomes = record["frame"], record["genomes"]
            codec.validate_frame(frame, genomes)
            codec.require(frame["tick"] == tick, "producer tick gap/repetition")
            prior_ids = set() if previous is None else {c["id"] for c in previous["cells"]}
            new_ids = [int(c["id"]) for c in frame["cells"] if c["id"] not in prior_ids]
            codec.require(not new_ids or min(new_ids) > highest_id, "producer reused a retired ID")
            if new_ids:
                highest_id = max(new_ids)
            # Validate invariants even when a new independent keyframe is needed.
            changed = None if previous is None else codec.delta(previous, frame, old_genomes, genomes)
            if chunk is None:
                chunk = codec.Chunk(tick)
                codec.require(chunk.append(codec.keyframe(frame, genomes), tick), "single keyframe exceeds decoded/gzip cap")
            elif not chunk.append(changed, tick):
                flush()
                chunk = codec.Chunk(tick)
                codec.require(chunk.append(codec.keyframe(frame, genomes), tick), "single keyframe exceeds decoded/gzip cap")
            previous, old_genomes = frame, genomes
            if tick in horizons:
                flush()
                prefixes[tick] = {"chunks": copy.deepcopy(chunks), "genomes": copy.deepcopy(genomes)}
        footer = read_record(stream)
        codec.fields(footer, ("type", "checked_ticks", "frames", "matter_residual_max", "energy_residual_max"))
        codec.natural(footer["checked_ticks"], 1_000_000)
        codec.natural(footer["frames"], 1_000_001)
        codec.require(footer == {"type": "end", "checked_ticks": steps, "frames": steps + 1,
                               "matter_residual_max": "0", "energy_residual_max": "0"}, "incomplete/unchecked producer footer")
        codec.require(stream.read(1) == b"", "trailing producer data")
        manifests = []
        for horizon in horizons:
            prefix = prefixes[horizon]
            manifest = {"schema_version": 1, "kind": "dense_recorded_cell_experiment", "codec": "dense-cell-jsonl-mask-v1+gzip",
                        "identity": header["identity"], "provenance": header["provenance"], "model": header["model"],
                        "canonical_config": header["canonical_config"], "limitations": header["limitations"],
                        "experiment": {**header["experiment"], "steps": horizon, "checked_ticks": horizon,
                                       "frames": horizon + 1, "matter_residual_max": "0", "energy_residual_max": "0"},
                        "bounds": {"frames_per_chunk": codec.MAX_TICKS, "decoded_bytes_per_chunk": codec.MAX_DECODED,
                                   "gzip_bytes_per_chunk": codec.MAX_GZIP, "max_chunks": codec.MAX_CHUNKS,
                                   "total_artifact_bytes_cap": max_bytes, "engineering_target_bytes": 256 * 1024 * 1024,
                                   "provider_limits_verified": False},
                        "packer": {"python": sys.version.split()[0], "zlib": zlib.ZLIB_RUNTIME_VERSION,
                                   "gzip_mtime": 0, "gzip_filename": "", "gzip_os_byte": 255},
                        "genomes": prefix["genomes"], "chunks": prefix["chunks"]}
            data = codec.encode(manifest)
            codec.require(total_bytes + len(data) <= max_bytes, "total artifact byte budget exceeded by manifests")
            name = f"horizon-{horizon}.json"
            (staging / name).write_bytes(data)
            total_bytes += len(data)
            manifests.append({"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                              "first_tick": 0, "last_tick": horizon, "frames": horizon + 1})
        index = codec.encode({"schema_version": 1, "kind": "dense_recording_index", "identity": header["identity"],
                              "provenance": header["provenance"], "manifests": manifests,
                              "unique_chunks": len(chunks), "unique_gzip_bytes": sum(c["gzip_bytes"] for c in chunks),
                              "decoded_chunk_bytes": sum(c["decoded_bytes"] for c in chunks)})
        codec.require(total_bytes + len(index) <= max_bytes, "total artifact byte budget exceeded by index")
        (staging / "index.json").write_bytes(index)
        total_bytes += len(index)
        # No final manifest/directory exists before a complete validated footer.
        codec.require(not output.exists(), "output appeared during packing")
        os.rename(staging, output)
    return {"frames": steps + 1, "chunks": len(chunks), "artifact_bytes": total_bytes, "manifests": manifests,
            "index_bytes": len(index), "index_sha256": hashlib.sha256(index).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--max-total-bytes", type=int)
    args = parser.parse_args()
    try:
        print(json.dumps(pack(sys.stdin.buffer, args.output_dir, args.max_total_bytes)))
    except (OSError, ValueError, TypeError, KeyError, IndexError, RecursionError, zlib.error) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
