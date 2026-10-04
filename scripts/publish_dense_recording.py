#!/usr/bin/env python3
"""Publish a byte-preserving transport layout, without running the model."""
import argparse
import copy
import ctypes
import errno
import hashlib
import importlib.util
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
VERSION = "dense-publication-layout-v1"


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "scripts" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


decoder = load("dense-recording-decode")
codec = decoder.codec


def sha(data):
    return hashlib.sha256(data).hexdigest()


def digest(value):
    codec.require(type(value) is str and codec.HEX.fullmatch(value), "expected external SHA-256 is required")


DIRECTORY_FLAGS = (os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC) if sys.platform == "linux" else None
FILE_FLAGS = (os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC) if sys.platform == "linux" else None


def open_directory(directory):
    codec.require(sys.platform == "linux", "publisher requires Linux safe-FD access")
    path = Path(directory)
    parts = path.parts[1:] if path.is_absolute() else path.parts
    descriptor = os.open("/" if path.is_absolute() else ".", DIRECTORY_FLAGS)
    try:
        for component in parts:
            codec.require(component not in ("", ".", ".."), "unsafe directory path")
            next_descriptor = os.open(component, DIRECTORY_FLAGS, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def read_file(directory, relative, maximum):
    path = Path(relative)
    codec.require(not path.is_absolute() and path.parts
                  and all(part not in ("", ".", "..") for part in path.parts), "unsafe source path")
    directory_descriptor = open_directory(directory)
    file_descriptor = None
    try:
        for component in path.parts[:-1]:
            next_descriptor = os.open(component, DIRECTORY_FLAGS, dir_fd=directory_descriptor)
            os.close(directory_descriptor)
            directory_descriptor = next_descriptor
        file_descriptor = os.open(path.parts[-1], FILE_FLAGS, dir_fd=directory_descriptor)
        info = os.fstat(file_descriptor)
        codec.require(stat.S_ISREG(info.st_mode), "source must be a regular file")
        codec.require(info.st_size <= maximum, "source file byte cap exceeded")
        data = bytearray()
        while len(data) <= maximum:
            piece = os.read(file_descriptor, min(64 * 1024, maximum + 1 - len(data)))
            if not piece:
                break
            data.extend(piece)
        codec.require(len(data) <= maximum, "source file byte cap exceeded")
        return bytes(data)
    finally:
        if file_descriptor is not None:
            os.close(file_descriptor)
        os.close(directory_descriptor)


def linux_renameat2():
    codec.require(sys.platform == "linux", "publisher requires Linux renameat2(RENAME_NOREPLACE)")
    library = ctypes.CDLL(None, use_errno=True)
    try:
        operation = library.renameat2
    except AttributeError as error:
        raise OSError(errno.ENOSYS, "libc renameat2 is unavailable; no overwrite fallback") from error
    operation.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    operation.restype = ctypes.c_int
    return operation


def atomic_noreplace(source, destination, operation):
    source, destination = Path(source), Path(destination)
    codec.require(source.name and destination.name and source.name not in (".", "..")
                  and destination.name not in (".", ".."), "invalid rename basename")
    source_descriptor = open_directory(source.parent)
    try:
        destination_descriptor = open_directory(destination.parent)
        try:
            result = operation(source_descriptor, os.fsencode(source.name), destination_descriptor, os.fsencode(destination.name), 1)
            if result != 0:
                code = ctypes.get_errno()
                raise OSError(code, os.strerror(code), str(destination))
        finally:
            os.close(destination_descriptor)
    finally:
        os.close(source_descriptor)


def preflight_noreplace(parent):
    operation = linux_renameat2()
    with tempfile.TemporaryDirectory(prefix=".dense-publication-probe-", dir=parent) as temporary:
        source, existing, absent = (Path(temporary) / name for name in ("source", "existing", "absent"))
        source.mkdir()
        existing.mkdir()
        source_inode, existing_inode = source.stat().st_ino, existing.stat().st_ino
        try:
            atomic_noreplace(source, existing, operation)
        except OSError as error:
            if error.errno != errno.EEXIST:
                raise
        else:
            raise ValueError("filesystem no-replace probe overwrote an existing destination")
        codec.require(source.is_dir() and existing.is_dir() and source.stat().st_ino == source_inode
                      and existing.stat().st_ino == existing_inode and not list(existing.iterdir()), "no-replace probe changed existing directory")
        atomic_noreplace(source, absent, operation)
        codec.require(not os.path.lexists(source) and absent.is_dir() and absent.stat().st_ino == source_inode, "no-replace probe did not move absent destination")
    return operation


def validate_inventory(source, expected_files):
    expected_directories = {str(parent) for name in expected_files for parent in Path(name).parents if str(parent) != "."}
    observed_files, observed_directories = set(), set()
    for path in source.rglob("*"):
        codec.require(not path.is_symlink(), "source inventory contains a symlink")
        relative = str(path.relative_to(source))
        codec.require((path.is_file() and relative in expected_files)
                      or (path.is_dir() and relative in expected_directories), "undeclared source dataset file/directory")
        (observed_files if path.is_file() else observed_directories).add(relative)
    codec.require(observed_files == expected_files and observed_directories == expected_directories, "source inventory is incomplete")


def publisher_identity(expected_commit):
    codec.require(type(expected_commit) is str and codec.SHA1.fullmatch(expected_commit), "expected publisher HEAD is required")
    def git(*args):
        return subprocess.check_output(["git", "-C", str(ROOT), *args]).decode().strip()
    head = git("rev-parse", "HEAD")
    codec.require(head == expected_commit and not git("status", "--porcelain", "--untracked-files=normal"), "publisher requires its exact clean HEAD")
    paths = ("scripts/publish_dense_recording.py", "scripts/dense-recording-codec.py", "scripts/dense-recording-decode.py")
    objects = {path: git("rev-parse", "HEAD:" + path) for path in paths}
    codec.require(all(git("hash-object", "--", path) == objects[path] for path in paths), "publisher helper bytes differ from HEAD")
    return {"version": VERSION, "commit": head, "tree": git("rev-parse", "HEAD^{tree}"),
            "git_objects": objects,
            "source_verification": "clean_actual_publisher_HEAD", "build_attestation": False}


def file_descriptor(path, data):
    return {"path": path, "bytes": len(data), "sha256": sha(data)}


def validate_receipt(raw, expected_sha, source_sha, full):
    digest(expected_sha)
    codec.require(len(raw) <= 64 * 1024 and sha(raw) == expected_sha, "validation receipt digest/size mismatch")
    receipt = codec.parse(raw)
    codec.fields(receipt, ("kind", "schema_version", "status", "source_index_sha256", "horizon", "frames", "decoded_frames",
                           "producer_commit", "matter_residual_max", "energy_residual_max", "validation_report_sha256"))
    experiment = full["experiment"]
    digest(receipt["source_index_sha256"])
    digest(receipt["validation_report_sha256"])
    codec.require(receipt["kind"] == "full_dense_recording_validation" and type(receipt["schema_version"]) is int
                  and receipt["schema_version"] == 1 and receipt["status"] == "PASS" and receipt["source_index_sha256"] == source_sha
                  and codec.natural(receipt["horizon"], 1_000_000) == experiment["steps"]
                  and codec.natural(receipt["frames"], 1_000_001) == codec.natural(receipt["decoded_frames"], 1_000_001) == experiment["frames"]
                  and receipt["producer_commit"] == full["provenance"]["producer_commit"]
                  and receipt["matter_residual_max"] == receipt["energy_residual_max"] == "0", "receipt does not bind a complete source decode")
    return receipt


def boundary_genomes(raw):
    # Only the <=256 records of a prefix-ending chunk, no frame reconstruction.
    genomes = None
    for position, line in enumerate(raw.splitlines()):
        record = codec.parse(line)
        if position == 0:
            codec.require(record["type"] == "keyframe", "prefix boundary lacks keyframe")
            genomes = record["genomes"]
        else:
            codec.require(record["type"] == "delta" and type(record["genomes"]) is dict
                          and not (genomes.keys() & record["genomes"].keys()), "invalid boundary genotype additions")
            genomes = {**genomes, **record["genomes"]}
        codec.validate_genomes(genomes)
    return genomes


def publish(source, output, expected_source_sha, expected_publisher_commit, receipt_path=None, expected_receipt_sha=None):
    source, output = Path(source), Path(output)
    digest(expected_source_sha)
    codec.require(not os.path.lexists(output), "output directory already exists; never overwrite")
    codec.require((receipt_path is None) == (expected_receipt_sha is None), "receipt path and external receipt SHA must be paired")
    publisher = publisher_identity(expected_publisher_commit)
    output.parent.mkdir(parents=True, exist_ok=True)
    rename_operation = preflight_noreplace(output.parent)
    index_bytes = read_file(source, "index.json", 64 * 1024)
    codec.require(sha(index_bytes) == expected_source_sha, "source index differs from external SHA")
    index = codec.parse(index_bytes)
    codec.require(type(index.get("manifests")) is list and 1 <= len(index["manifests"]) <= 3, "invalid source index")
    manifests, originals = [], {"original/index.json": index_bytes}
    for entry in index["manifests"]:
        horizon = codec.natural(entry["last_tick"], 1_000_000)
        codec.require(entry["path"] == f"horizon-{horizon}.json", "unsafe manifest path")
        raw = read_file(source, entry["path"], 16 * 1024 * 1024)
        manifest, parsed_index, checked_raw, checked_index = decoder.metadata(source / entry["path"], raw=raw, index_bytes=index_bytes)
        codec.require(raw == checked_raw and index_bytes == checked_index and codec.exact(index, parsed_index), "source metadata changed during validation")
        manifests.append(manifest)
        originals["original/" + entry["path"]] = raw
    full = manifests[-1]
    stable = set(full) - {"experiment", "genomes", "chunks"}
    for manifest in manifests:
        codec.require(all(codec.exact(manifest[key], full[key]) for key in stable), "prefix metadata/provenance mismatch")
        codec.require(codec.exact(manifest["chunks"], full["chunks"][:len(manifest["chunks"])]), "prefix descriptors/path mismatch")
        codec.require(all(key in full["genomes"] and codec.exact(value, full["genomes"][key]) for key, value in manifest["genomes"].items()), "prefix genotype definition mismatch")
    validate_inventory(source, {"index.json", *(entry["path"] for entry in index["manifests"]), *(chunk["path"] for chunk in full["chunks"])})
    receipt = None
    if receipt_path is not None:
        receipt_path = Path(receipt_path)
        receipt_raw = read_file(receipt_path.parent, receipt_path.name, 64 * 1024)
        receipt = validate_receipt(receipt_raw, expected_receipt_sha, expected_source_sha, full)
        originals["original/validation-receipt.json"] = receipt_raw
    cap = full["bounds"]["total_artifact_bytes_cap"]
    codec.require(index["unique_gzip_bytes"] + sum(len(data) for data in originals.values()) <= cap, "publication metadata exceeds total budget")
    with tempfile.TemporaryDirectory(prefix=".dense-publication-", dir=output.parent) as temporary:
        staging = Path(temporary) / "complete"
        staging.mkdir()
        mapping, file_digests = [], []
        prefix_ends = {manifest["experiment"]["steps"]: manifest for manifest in manifests}
        previous, previous_genomes, highest_id, decoded_frames = None, None, -1, 0
        for ordinal, descriptor in enumerate(full["chunks"]):
            codec.validate_descriptor(descriptor, ordinal)
            data = read_file(source, descriptor["path"], codec.MAX_GZIP)
            codec.require(len(data) == descriptor["gzip_bytes"] and sha(data) == descriptor["gzip_sha256"], "source gzip digest/size mismatch")
            raw = codec.inflate(data)
            codec.require(len(raw) == descriptor["decoded_bytes"] and sha(raw) == descriptor["decoded_sha256"], "source inflated digest/size mismatch")
            codec.require(len(raw.splitlines()) == descriptor["frames"], "source payload record count mismatch")
            if receipt is None:
                for frame, genomes in codec.decode_chunk(data, descriptor):
                    if frame["tick"] == descriptor["first_tick"] and previous is not None:
                        codec.delta(previous, frame, previous_genomes, genomes)
                    old_ids = set() if previous is None else {cell["id"] for cell in previous["cells"]}
                    new_ids = [int(cell["id"]) for cell in frame["cells"] if cell["id"] not in old_ids]
                    codec.require(not new_ids or min(new_ids) > highest_id, "source reused a retired ID")
                    if new_ids:
                        highest_id = max(new_ids)
                    codec.require(all(key in full["genomes"] and codec.exact(value, full["genomes"][key]) for key, value in genomes.items()), "payload genotype differs from manifest")
                    if frame["tick"] in prefix_ends:
                        codec.require(codec.exact(genomes, prefix_ends[frame["tick"]]["genomes"]), "prefix observed genotype dictionary mismatch")
                    previous, previous_genomes = frame, genomes
                    decoded_frames += 1
            elif descriptor["last_tick"] in prefix_ends:
                codec.require(codec.exact(boundary_genomes(raw), prefix_ends[descriptor["last_tick"]]["genomes"]), "prefix boundary genotype dictionary mismatch")
            new_path = codec.sharded_path(ordinal, descriptor["first_tick"], descriptor["last_tick"])
            target = staging / new_path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            mapping.append({"original_path": descriptor["path"], "published_path": new_path,
                            "gzip_bytes": len(data), "gzip_sha256": descriptor["gzip_sha256"],
                            "decoded_bytes": descriptor["decoded_bytes"], "decoded_sha256": descriptor["decoded_sha256"]})
        if receipt is None:
            codec.require(decoded_frames == full["experiment"]["frames"], "full source decode is incomplete")
        published_index = copy.deepcopy(index)
        metadata_files = dict(originals)
        for manifest, entry in zip(manifests, published_index["manifests"]):
            changed = copy.deepcopy(manifest)
            for ordinal, descriptor in enumerate(changed["chunks"]):
                descriptor["path"] = mapping[ordinal]["published_path"]
            data = codec.encode(changed)
            metadata_files[entry["path"]] = data
            entry.update(bytes=len(data), sha256=sha(data))
        published_index_bytes = codec.encode(published_index)
        metadata_files["index.json"] = published_index_bytes
        for path, data in metadata_files.items():
            target = staging / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            file_digests.append(file_descriptor(path, data))
        validation = {"method": "full_unique_trajectory_schema_decode" if receipt is None else "externally_pinned_full_decode_receipt",
                      "all_gzip_size_sha_crc_inflated_sha_checked": True,
                      "decoded_frames_here": decoded_frames,
                      "external_receipt_sha256": expected_receipt_sha,
                      "validation_report_sha256": None if receipt is None else receipt["validation_report_sha256"]}
        publication = codec.encode({"schema_version": 1, "kind": "dense_recording_publication", "publisher": publisher,
                                    "identity": full["identity"], "provenance": full["provenance"],
                                    "original_index_sha256": expected_source_sha, "published_index_sha256": sha(published_index_bytes),
                                    "layout": {"kind": "ordinal_shards_v1", "files_per_directory": 256, "max_directories": 32},
                                    "source_validation": validation, "mapping": mapping, "metadata_files": file_digests,
                                    "gzip_bytes": index["unique_gzip_bytes"],
                                    "metadata_bytes_excluding_publication": sum(len(data) for data in metadata_files.values()),
                                    "total_artifact_bytes_cap": cap})
        total = index["unique_gzip_bytes"] + sum(len(data) for data in metadata_files.values()) + len(publication)
        codec.require(total <= cap, "publication exceeds complete artifact budget")
        (staging / "publication.json").write_bytes(publication)
        codec.require(read_file(source, "index.json", 64 * 1024) == index_bytes, "source index changed during publication")
        codec.require(not os.path.lexists(output), "output appeared during publication")
        atomic_noreplace(staging, output, rename_operation)
    return {"publication_sha256": sha(publication), "publication_bytes": len(publication), "artifact_bytes": total,
            "source_index_sha256": expected_source_sha, "published_index_sha256": sha(published_index_bytes), "chunks": len(mapping)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--expected-source-index-sha256", required=True)
    parser.add_argument("--expected-publisher-commit", required=True)
    parser.add_argument("--validation-receipt", type=Path)
    parser.add_argument("--expected-receipt-sha256")
    args = parser.parse_args()
    try:
        print(codec.encode(publish(args.source_dir, args.output_dir, args.expected_source_index_sha256,
                                 args.expected_publisher_commit, args.validation_receipt, args.expected_receipt_sha256)).decode().strip())
    except (OSError, ValueError, TypeError, KeyError, IndexError, RecursionError, zlib.error, subprocess.CalledProcessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
