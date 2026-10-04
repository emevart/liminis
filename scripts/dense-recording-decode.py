#!/usr/bin/env python3
"""Decode a dense manifest incrementally to stdout JSONL, without biology."""
import argparse
import hashlib
import importlib.util
from pathlib import Path
import sys
import zlib

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("dense_codec", Path(__file__).with_name("dense-recording-codec.py"))
codec = importlib.util.module_from_spec(spec)
spec.loader.exec_module(codec)


def frames(manifest_path, first=0, last=None):
    path = Path(manifest_path)
    with path.open("rb") as stream:
        raw = stream.read(16 * 1024 * 1024 + 1)
    codec.require(len(raw) <= 16 * 1024 * 1024, "manifest byte cap exceeded")
    manifest = codec.parse(raw)
    with (path.parent / "index.json").open("rb") as stream:
        index_bytes = stream.read(64 * 1024 + 1)
    codec.require(len(index_bytes) <= 64 * 1024, "index byte cap exceeded")
    index = codec.parse(index_bytes)
    codec.fields(index, ("schema_version", "kind", "identity", "provenance", "manifests", "unique_chunks", "unique_gzip_bytes", "decoded_chunk_bytes"))
    codec.require(type(index["schema_version"]) is int and index["schema_version"] == 1 and index["kind"] == "dense_recording_index"
                  and type(index["manifests"]) is list and 1 <= len(index["manifests"]) <= 3, "unsupported index")
    prior_horizon = 0
    for entry in index["manifests"]:
        codec.fields(entry, ("path", "bytes", "sha256", "first_tick", "last_tick", "frames"))
        horizon = codec.natural(entry["last_tick"], 1_000_000)
        codec.require(horizon > prior_horizon and entry["path"] == f"horizon-{horizon}.json" and entry["first_tick"] == 0
                      and type(entry["first_tick"]) is int and codec.natural(entry["frames"]) == horizon + 1
                      and 0 < codec.natural(entry["bytes"], 16 * 1024 * 1024)
                      and type(entry["sha256"]) is str and codec.HEX.fullmatch(entry["sha256"]), "invalid index manifest descriptor")
        prior_horizon = horizon
    full_cap = codec.total_cap(index["manifests"][-1]["last_tick"])
    codec.require(1 <= codec.natural(index["unique_chunks"], codec.MAX_CHUNKS)
                  and 1 <= codec.natural(index["unique_gzip_bytes"], full_cap)
                  and 1 <= codec.natural(index["decoded_chunk_bytes"], codec.MAX_CHUNKS * codec.MAX_DECODED), "invalid index counts")
    matches = [entry for entry in index["manifests"] if entry["path"] == path.name]
    codec.require(len(matches) == 1 and matches[0]["bytes"] == len(raw)
                  and matches[0]["sha256"] == hashlib.sha256(raw).hexdigest(), "manifest/index integrity mismatch")
    codec.fields(manifest, ("schema_version", "kind", "codec", "identity", "provenance", "model", "canonical_config",
                            "limitations", "experiment", "bounds", "packer", "genomes", "chunks"))
    codec.require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1 and manifest["kind"] == "dense_recorded_cell_experiment"
                  and manifest["codec"] == "dense-cell-jsonl-mask-v1+gzip", "unsupported dense manifest")
    horizon = codec.natural(manifest["experiment"]["steps"], 1_000_000)
    codec.require(matches[0]["first_tick"] == 0 and matches[0]["last_tick"] == horizon and matches[0]["frames"] == horizon + 1
                  and codec.exact(index["identity"], manifest["identity"]) and codec.exact(index["provenance"], manifest["provenance"]), "index identity/range mismatch")
    codec.validate_header({"type": "header", "schema_version": 1, "kind": "dense_cell_frame_stream",
                           **{k: manifest[k] for k in ("identity", "provenance", "model", "experiment", "canonical_config", "limitations")}})
    codec.require(manifest["experiment"]["sample_every"] == 1 and manifest["experiment"]["frames"] == horizon + 1
                  and type(manifest["experiment"]["frames"]) is int and manifest["experiment"]["checked_ticks"] == horizon
                  and type(manifest["experiment"]["checked_ticks"]) is int
                  and manifest["experiment"]["matter_residual_max"] == manifest["experiment"]["energy_residual_max"] == "0", "invalid dense cadence/counts")
    bounds = manifest["bounds"]
    codec.fields(bounds, ("frames_per_chunk", "decoded_bytes_per_chunk", "gzip_bytes_per_chunk", "max_chunks",
                          "total_artifact_bytes_cap", "engineering_target_bytes", "provider_limits_verified"))
    codec.require(bounds["frames_per_chunk"] == codec.MAX_TICKS and bounds["decoded_bytes_per_chunk"] == codec.MAX_DECODED
                  and bounds["gzip_bytes_per_chunk"] == codec.MAX_GZIP and bounds["max_chunks"] == codec.MAX_CHUNKS
                  and bounds["engineering_target_bytes"] == 256 * 1024 * 1024 and bounds["provider_limits_verified"] is False
                  and 0 < codec.natural(bounds["total_artifact_bytes_cap"], full_cap), "unsupported bounds")
    codec.fields(manifest["packer"], ("python", "zlib", "gzip_mtime", "gzip_filename", "gzip_os_byte"))
    codec.require(type(manifest["packer"]["python"]) is str and type(manifest["packer"]["zlib"]) is str
                  and type(manifest["packer"]["gzip_mtime"]) is int and manifest["packer"]["gzip_mtime"] == 0
                  and manifest["packer"]["gzip_filename"] == "" and manifest["packer"]["gzip_os_byte"] == 255, "unsupported packer metadata")
    codec.validate_genomes(manifest["genomes"])
    chunks = manifest["chunks"]
    codec.require(type(chunks) is list and 1 <= len(chunks) <= codec.MAX_CHUNKS, "unbounded manifest chunks")
    expected = 0
    for chunk in chunks:
        codec.validate_descriptor(chunk)
        codec.require(chunk["first_tick"] == expected, "manifest chunk gap/overlap")
        expected = chunk["last_tick"] + 1
    codec.require(expected == horizon + 1, "manifest horizon mismatch")
    gzip_bytes, decoded_bytes = sum(c["gzip_bytes"] for c in chunks), sum(c["decoded_bytes"] for c in chunks)
    codec.require(index["unique_chunks"] >= len(chunks) and index["unique_gzip_bytes"] >= gzip_bytes
                  and index["decoded_chunk_bytes"] >= decoded_bytes
                  and index["unique_gzip_bytes"] + len(index_bytes) + sum(e["bytes"] for e in index["manifests"]) <= bounds["total_artifact_bytes_cap"], "index/budget count mismatch")
    if horizon == index["manifests"][-1]["last_tick"]:
        codec.require(index["unique_chunks"] == len(chunks) and index["unique_gzip_bytes"] == gzip_bytes
                      and index["decoded_chunk_bytes"] == decoded_bytes, "index/full horizon count mismatch")
    last = horizon if last is None else last
    codec.require(type(first) is int and type(last) is int and 0 <= first <= last <= horizon, "invalid requested range")
    for chunk in chunks:
        if chunk["last_tick"] < first or chunk["first_tick"] > last:
            continue
        for frame, genomes in codec.read_chunk(path.parent, chunk):
            codec.require(all(k in manifest["genomes"] and codec.exact(v, manifest["genomes"][k]) for k, v in genomes.items()), "chunk/manifest genotype mismatch")
            if first <= frame["tick"] <= last:
                yield frame, genomes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--first-tick", type=int, default=0)
    parser.add_argument("--last-tick", type=int)
    args = parser.parse_args()
    try:
        for frame, genomes in frames(args.manifest, args.first_tick, args.last_tick):
            sys.stdout.buffer.write(codec.encode({"frame": frame, "genomes": genomes}))
    except (OSError, ValueError, TypeError, KeyError, IndexError, RecursionError, zlib.error) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
