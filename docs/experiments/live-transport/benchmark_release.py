#!/usr/bin/env python3
"""Bounded occupied release-host measurements for chamber2 D0/Dpositive.

Uses temporary storage/configs and owns only its child hosts. All timestamps
include HTTP polling and Pause acknowledgement. Counts are observations, not
an exact integrated cell-work denominator or a controlled timing comparison.
No source/config/public recording is overwritten; no build is launched.
"""

import argparse
import hashlib
import importlib.util
import json
import platform
import socket
import subprocess
import tempfile
import time
import urllib.error
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location(
    "pacing_acceptance", ROOT / "scripts/check_live_cells_pacing.py")
PACING = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACING)
HTTP_BUDGET_SECONDS = 2.0


class ConfigHost(PACING.Host):
    def __init__(self, binary, data, config):
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        self.url = f"http://127.0.0.1:{port}"
        self.log = tempfile.TemporaryFile()
        self.process = subprocess.Popen(
            [str(binary), "cells", "--port", str(port), "--data-dir", str(data),
             "--config", str(config), "--seed", "42"], cwd=ROOT,
            stdout=self.log, stderr=self.log)
        try:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                if self.process.poll() is not None:
                    self.log.seek(0)
                    raise AssertionError(self.log.read().decode())
                try:
                    self.state()
                    return
                except (urllib.error.URLError, TimeoutError):
                    time.sleep(0.02)
            raise AssertionError("host startup deadline")
        except BaseException:
            self.close()
            raise


def verify(state):
    assert state["kind"] == "cells" and state["chamber_format"] == 2
    assert state["world_format_version"] == 31 and state["seed"] == "42"
    assert state["dt_seconds"] == 30 and not state["error"]
    assert not state["persistence"]["error"]
    if state["tick"]:
        assert state["residual"] == {"matter": "0", "energy": "0"}
    lengths = state["model"]["dimensions_m"]
    for cell in state["cells"]:
        assert all(0 <= x <= length for x, length in
                   zip(cell["position_m"], lengths))


def measure(binary, template, mobility, ticks):
    assert template.count("mobility_scale = 1.0") == 1
    text = template.replace("mobility_scale = 1.0", f"mobility_scale = {mobility:.1f}")
    with tempfile.TemporaryDirectory(prefix="liminis-transport-benchmark-") as tmp:
        temporary = Path(tmp)
        config = temporary / "scenario.toml"
        config.write_text(text)
        host = ConfigHost(binary, temporary / "data", config)
        try:
            host.control("pause")
            host.settled()
            host.control("reset")
            initial = host.settled()
            verify(initial)
            assert initial["tick"] == 0
            positions = {cell["id"]: cell["position_m"] for cell in initial["cells"]}
            stepped, _ = host.control("step")
            verify(stepped)
            changes = sum(cell["position_m"] != positions[cell["id"]]
                          for cell in stepped["cells"] if cell["id"] in positions)
            if mobility == 0:
                assert changes == 0
            else:
                assert changes > 0
            host.settled()
            host.control("reset")
            initial = host.settled()
            host.control("maximum")
            assert host.state()["target_tps"] is None
            samples = [{"tick": 0, "living_cells": initial["summary"]["living_cells"]}]
            latencies = []
            started = time.monotonic()
            host.control("run")
            deadline = started + 8
            while time.monotonic() < deadline:
                state, latency = host.request("/api/state")
                verify(state)
                samples.append({"tick": state["tick"],
                                "living_cells": state["summary"]["living_cells"]})
                latencies.append(latency)
                if state["tick"] >= ticks:
                    break
                time.sleep(0.005)
            else:
                raise AssertionError("bounded occupied benchmark deadline")
            paused, pause_latency = host.control("pause")
            elapsed = time.monotonic() - started
            verify(paused)
            samples.append({"tick": paused["tick"],
                            "living_cells": paused["summary"]["living_cells"]})
            assert pause_latency < HTTP_BUDGET_SECONDS
            assert max(latencies) < HTTP_BUDGET_SECONDS
            time.sleep(0.05)
            assert host.state()["tick"] == paused["tick"], "Pause ack permits no next tick"
            saved = host.settled()
            counts = [sample["living_cells"] for sample in samples]
            assert min(counts) > 0, "empty-world speed cannot attest occupied budget"
            return {
                "name": f"live_cells_chamber2_seed42_release_D{'0' if mobility == 0 else 'positive'}_target{ticks}",
                "mobility_scale": mobility, "scenario_sha256": hashlib.sha256(config.read_bytes()).hexdigest(),
                "canonical_config_hash": paused["config_hash"], "model": paused["model"],
                "seed": "42", "dt_seconds": 30, "requested_target_ticks": ticks,
                "actual_ticks": paused["tick"], "actual_tps": paused["tick"] / elapsed,
                "actual_multiplier": paused["tick"] * 30 / elapsed,
                "wall_seconds_including_http_and_pause": elapsed,
                "observed_living_min": min(counts), "observed_living_max": max(counts),
                "all_observed_occupied": True, "first_step_changed_survivor_positions": changes,
                "http_max_ms": max(latencies) * 1000, "pause_ms": pause_latency * 1000,
                "http_and_pause_budget_seconds": HTTP_BUDGET_SECONDS,
                "save_ack_tick": saved["persistence"]["saved_tick"], "samples": samples,
            }
        finally:
            host.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ticks", type=int, default=1000)
    parser.add_argument("--output", default="target/qa/live-transport-benchmark.json")
    args = parser.parse_args()
    assert 1 <= args.ticks <= 2000, "bounded measurement target required"
    binary = ROOT / "target/release/liminis"
    assert binary.is_file(), "build pinned release host first"
    source = ROOT / "configs/scenarios/cell-chamber-physical.toml"
    cpu = next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                if line.startswith("model name")), None) if Path("/proc/cpuinfo").exists() else None
    report = {
        "status": "PASS", "platform": platform.platform(), "cpu": cpu,
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "source_tree": subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], cwd=ROOT, text=True).strip(),
        "tracked_diff": subprocess.check_output(["git", "diff", "HEAD", "--name-only"], cwd=ROOT, text=True).splitlines(),
        "runtime_rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "build_attestation": False,
        "build_requirement": "cargo build --locked --release -p liminis at source; runtime rustc and binary digest alone do not attest build origin",
        "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "base_scenario_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "limits": "single native build/hardware, sampled living inventory; no integrated cell-work or controlled D timing comparison",
        "benchmarks": [measure(binary, source.read_text(), mobility, args.ticks) for mobility in (0.0, 1.0)],
    }
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n")
    for row in report["benchmarks"]:
        print(json.dumps({key: value for key, value in row.items() if key != "samples"}))


if __name__ == "__main__":
    main()
