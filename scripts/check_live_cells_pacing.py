#!/usr/bin/env python3
"""Real release-host pacing/resume acceptance; stdlib only, no build or user data.

Run from the repository root after building target/release/liminis. This owns
only its child process, free loopback port and temporary storage. JSON output
includes a named five-second Maximum benchmark, not a browser QA claim.
Checkpoint equality covers actual temporarily persisted envelopes; deterministic
future and same-tick slow/manual/Maximum comparisons belong to host unit tests.
"""

import argparse
import json
import platform
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path


class Host:
    def __init__(self, binary, root, data, resume=False):
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        self.url = f"http://127.0.0.1:{port}"
        args = [str(binary), "cells", "--port", str(port), "--data-dir", str(data)]
        args += ["--resume", "latest"] if resume else ["--seed", "42"]
        self.log = tempfile.TemporaryFile()
        self.process = subprocess.Popen(args, cwd=root, stdout=self.log, stderr=self.log)
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
            raise AssertionError("host startup timed out")
        except BaseException:
            self.close()
            raise

    def request(self, path, payload=None, expected=200):
        body = None if payload is None else json.dumps(payload).encode()
        request = urllib.request.Request(self.url + path, data=body,
                                         headers={"Content-Type": "application/json"})
        start = time.monotonic()
        try:
            with urllib.request.urlopen(request, timeout=5) as response:
                status, raw = response.status, response.read()
        except urllib.error.HTTPError as error:
            status, raw = error.code, error.read()
        latency = time.monotonic() - start
        assert status == expected, (status, expected, raw.decode())
        return (json.loads(raw) if status == 200 else None), latency

    def state(self):
        return self.request("/api/state")[0]

    def control(self, action, **values):
        return self.request("/api/control", {"action": action, **values})

    def settled(self):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            state = self.state()
            assert not state["error"], state["error"]
            status = state["persistence"]
            assert not status["error"], status["error"]
            if (not status["saving"] and not state["save_pending"]
                    and status["saved_tick"] == state["tick"]):
                return state
            time.sleep(0.02)
        raise AssertionError("checkpoint acknowledgement timed out")

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self.log.close()


def saved_capture(data, state):
    run = data / state["persistence"]["run_id"]
    latest = json.loads((run / "latest.json").read_text())
    return json.loads((run / "checkpoints" / latest["checkpoint_id"] / "state.json").read_text())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="target/release/liminis")
    parser.add_argument("--benchmark-seconds", type=float, default=5.0)
    args = parser.parse_args()
    assert args.benchmark_seconds > 0
    root = Path(__file__).resolve().parents[1]
    binary = (root / args.binary).resolve()
    assert binary.is_file(), f"build release binary first: {binary}"
    metrics = {"benchmark": "live_cells_maximum_cell_chamber_seed42_release",
               "platform": platform.platform(), "processor": platform.processor(),
               "scenario": "configs/scenarios/cell-chamber.toml", "seed": 42}
    with tempfile.TemporaryDirectory(prefix="liminis-live-acceptance-") as temporary:
        data = Path(temporary)
        host = Host(binary, root, data)
        try:
            host.control("pause")
            initial = host.settled()
            assert initial["dt_seconds"] == 30
            for invalid in [0, -1, "NaN", "Inf", "-Inf", None, True, float("nan"), float("inf")]:
                host.request("/api/control", {"action": "speed", "value": invalid}, expected=400)
            for multiplier in [0.5, 1, 90, 1234.5]:
                state, _ = host.control("speed", value=multiplier)
                assert state["tick"] == initial["tick"]
                assert state["target_tps"] == multiplier / 30
            host.control("speed", value=1)
            clock_started = time.monotonic()
            host.control("run")
            deadline = clock_started + 33
            first_tick_wall_seconds = None
            while time.monotonic() < deadline:
                observed = host.state()
                wall_seconds = time.monotonic() - clock_started
                if observed["tick"] != initial["tick"]:
                    assert wall_seconds >= 29.9, "1x dt30 advanced before 30 wall seconds"
                    assert observed["tick"] == initial["tick"] + 1
                    first_tick_wall_seconds = wall_seconds
                    break
                time.sleep(0.05)
            assert first_tick_wall_seconds is not None, "1x dt30 did not tick within 33 seconds"
            state, _ = host.control("pause")
            assert state["tick"] == initial["tick"] + 1
            host.settled()
            state, _ = host.control("step")
            assert state["tick"] == initial["tick"] + 2
            host.settled()
            host.control("maximum")
            before, _ = host.control("run")
            started = time.monotonic()
            latencies = []
            measured = 0.0
            while time.monotonic() - started < args.benchmark_seconds:
                state, latency = host.request("/api/state")
                latencies.append(latency)
                measured = max(measured, state["measured_tps"])
                time.sleep(0.05)
            paused, pause_latency = host.control("pause")
            elapsed = time.monotonic() - started
            ticks = paused["tick"] - before["tick"]
            assert ticks > 0
            assert pause_latency < 2 and max(latencies) < 2, "HTTP/pause responsiveness exceeded 2s"
            time.sleep(0.15)
            assert host.state()["tick"] == paused["tick"], "ticks advanced after Pause ack"
            host.settled()
            host.control("speed", value=1)
            host.control("run")
            time.sleep(0.2)
            no_debt, _ = host.control("pause")
            assert no_debt["tick"] == paused["tick"], "Maximum accrued pacing debt"
            host.settled()
            host.control("speed", value=7.5)
            state, _ = host.control("reset")
            assert state["tick"] == 0 and state["pacing"] == {"mode": "manual", "multiplier": 7.5}
            host.settled()
            host.control("maximum")
            host.control("step")
            state = host.settled()
            host.control("save")
            state = host.settled()
            capture = saved_capture(data, state)
            host.close()
            host = Host(binary, root, data, resume=True)
            restored = host.state()
            assert restored["tick"] == state["tick"] and not restored["running"]
            assert restored["pacing"] == state["pacing"]
            host.control("save")
            restored = host.settled()
            resumed_capture = saved_capture(data, restored)
            assert resumed_capture["state"] == capture["state"], "core/IDs/RNG/observation/pacing changed on resume"
            assert resumed_capture["tps"] == capture["tps"]
            metrics.update(wall_seconds=elapsed, ticks=ticks, actual_tps=ticks / elapsed,
                           actual_multiplier=ticks / elapsed * 30,
                           reported_tps_peak=measured, http_polls=len(latencies),
                           first_tick_wall_seconds=first_tick_wall_seconds,
                           http_max_ms=max(latencies) * 1000, pause_ms=pause_latency * 1000,
                           acceptance="passed", exact_new_checkpoint_resume=True)
        finally:
            host.close()
    print(json.dumps(metrics, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
