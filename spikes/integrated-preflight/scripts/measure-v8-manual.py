#!/usr/bin/env python3
"""Collect bounded packaged-app process-tree evidence without native input."""

from __future__ import annotations

import json
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "tools" / "t1-preflight"
sys.path.insert(0, str(TOOLS))

from t1_preflight.macos import (  # noqa: E402
    executable_path_for_pid,
    pids_for_executable,
    sample_process_tree,
)
from t1_preflight.model import ContractError, sha256_file, sha256_tree  # noqa: E402


RUN_ID = "herdr-v8-packaged-manual-v1"
APP = (
    ROOT
    / "spikes/integrated-preflight/target/bundle/herdr-integrated-preflight.app"
)
EXECUTABLE = APP / "Contents/MacOS/herdr-integrated-preflight"
MANIFEST = ROOT / "spikes/integrated-preflight/runtime-manifest-v8-packaged-manual-v1.json"
FIXTURE = ROOT / "spikes/integrated-preflight/fixture"
OUTPUT = ROOT / "spikes/integrated-preflight/evidence/v8-packaged-manual-v1"
HTTP_PORT = 45831
CDP_PORT = 45832
SAMPLE_COUNT = 10
SAMPLE_INTERVAL_SECONDS = 0.5
EVENT_TIMEOUT_SECONDS = 15.0

BROWSER_EXECUTABLES = {
    str(
        (APP / relative).resolve(strict=False)
    )
    for relative in (
        "Contents/Frameworks/herdr-integrated-preflight Helper.app/Contents/MacOS/herdr-integrated-preflight Helper",
        "Contents/Frameworks/herdr-integrated-preflight Helper (Alerts).app/Contents/MacOS/herdr-integrated-preflight Helper (Alerts)",
        "Contents/Frameworks/herdr-integrated-preflight Helper (GPU).app/Contents/MacOS/herdr-integrated-preflight Helper (GPU)",
        "Contents/Frameworks/herdr-integrated-preflight Helper (Plugin).app/Contents/MacOS/herdr-integrated-preflight Helper (Plugin)",
        "Contents/Frameworks/herdr-integrated-preflight Helper (Renderer).app/Contents/MacOS/herdr-integrated-preflight Helper (Renderer)",
    )
}


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def read_events(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    raw = path.read_text(encoding="utf-8")
    lines = raw.splitlines()
    if raw and not raw.endswith("\n"):
        lines = lines[:-1]
    events: list[dict[str, Any]] = []
    for line in lines:
        events.append(json.loads(line))
    return events


def wait_for_event(
    path: Path,
    process: subprocess.Popen[bytes],
    name: str,
    timeout: float = EVENT_TIMEOUT_SECONDS,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        events = read_events(path)
        for event in events:
            if event.get("event") == name:
                return event, events
        exit_code = process.poll()
        if exit_code is not None:
            raise ContractError(
                "manual.runtime.early_exit",
                "packaged app exited before required telemetry",
                event=name,
                exit_code=exit_code,
                events=events,
            )
        time.sleep(0.05)
    raise ContractError(
        "manual.runtime.telemetry_timeout",
        "packaged app telemetry did not arrive before bounded timeout",
        event=name,
        timeout_seconds=timeout,
        events=read_events(path),
    )


def wait_port(port: int, timeout: float = 5.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.settimeout(0.2)
            try:
                probe.connect(("127.0.0.1", port))
                return
            except OSError:
                time.sleep(0.05)
    raise ContractError(
        "manual.fixture.server_timeout",
        "owned fixture server did not accept loopback connections",
        port=port,
    )


def terminate_owned(
    process: subprocess.Popen[bytes],
    member_pids: set[int],
) -> dict[str, Any]:
    attempted: list[dict[str, Any]] = []
    candidates = {process.pid, *member_pids}
    for pid in sorted(candidates, reverse=True):
        if pid <= 0:
            continue
        path = executable_path_for_pid(pid)
        if path is None:
            continue
        inside_bundle = False
        try:
            path.relative_to(APP.resolve(strict=True))
            inside_bundle = True
        except ValueError:
            pass
        if pid != process.pid and not inside_bundle:
            continue
        try:
            os.kill(pid, signal.SIGTERM)
            attempted.append({"pid": pid, "signal": "SIGTERM", "executable": str(path)})
        except ProcessLookupError:
            continue
        except PermissionError as error:
            attempted.append({"pid": pid, "error": repr(error), "executable": str(path)})
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        if process.poll() is None:
            path = executable_path_for_pid(process.pid)
            if path == EXECUTABLE.resolve(strict=False):
                os.kill(process.pid, signal.SIGKILL)
                attempted.append({"pid": process.pid, "signal": "SIGKILL", "executable": str(path)})
            process.wait(timeout=5)
    remaining = [pid for pid in pids_for_executable(EXECUTABLE) if pid == process.pid]
    return {"attempted": attempted, "exit_code": process.returncode, "remaining_exact_pids": remaining}


def launch_phase(mode: str, phase: str, output: Path) -> dict[str, Any]:
    events_path = output / "runtime" / f"{phase}.events.jsonl"
    log_path = output / "runtime" / f"{phase}.log"
    events_path.parent.mkdir(parents=True, exist_ok=True)
    profile = output / "browser-profile"
    profile.mkdir(parents=True, exist_ok=True)
    arguments = [
        str(EXECUTABLE),
        "--t1-preflight-phase",
        phase,
        "--t1-browser-mode",
        mode,
        "--t1-browser-url",
        f"http://127.0.0.1:{HTTP_PORT}/",
        "--t1-browser-profile",
        str(profile),
        "--t1-remote-debugging-port",
        str(CDP_PORT),
        "--t1-preflight-events",
        str(events_path),
        "--t1-preflight-run-id",
        RUN_ID,
    ]
    if pids_for_executable(EXECUTABLE):
        raise ContractError(
            "manual.runtime.preexisting_target",
            "exact packaged target executable already has a running instance",
            pids=pids_for_executable(EXECUTABLE),
        )
    started = time.monotonic()
    with log_path.open("wb") as log_handle:
        process = subprocess.Popen(
            arguments,
            cwd=EXECUTABLE.parent,
            stdin=subprocess.DEVNULL,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        member_pids: set[int] = set()
        result: dict[str, Any] = {
            "phase": phase,
            "mode": mode,
            "pid": process.pid,
            "command": arguments,
            "events_path": str(events_path),
            "log_path": str(log_path),
            "status": "BLOCKED",
        }
        try:
            usable, events = wait_for_event(events_path, process, "app.usable")
            result["usable"] = usable
            result["warm_usable_ms"] = round((time.monotonic() - started) * 1000, 3)
            if mode == "browser-included":
                ready, events = wait_for_event(events_path, process, "browser.profile.ready")
                result["browser_ready"] = ready
            samples: list[dict[str, Any]] = []
            sample_error: dict[str, Any] | None = None
            for _ in range(SAMPLE_COUNT):
                try:
                    sample = sample_process_tree(
                        process.pid,
                        app_path=APP,
                        allowed_external_processes=["/bin/zsh"],
                    )
                    samples.append(sample)
                    member_pids.update(
                        int(member["pid"])
                        for member in sample.get("members", [])
                    )
                except ContractError as error:
                    sample_error = error.as_dict()
                    break
                time.sleep(SAMPLE_INTERVAL_SECONDS)
            result["samples"] = samples
            result["sample_error"] = sample_error
            rss_values = [float(sample["rss_mb"]) for sample in samples]
            cpu_values = [float(sample["cpu_percent"]) for sample in samples]
            observed_paths = {
                member["executable"]
                for sample in samples
                for member in sample.get("members", [])
            }
            result["metrics"] = {
                "sample_count": len(samples),
                "max_rss_mb": max(rss_values) if rss_values else None,
                "avg_cpu_percent": (sum(cpu_values) / len(cpu_values)) if cpu_values else None,
                "browser_helper_observed": bool(BROWSER_EXECUTABLES.intersection(observed_paths)),
                "observed_executables": sorted(observed_paths),
            }
            result["events"] = events
            result["status"] = "MEASURED" if samples else "BLOCKED"
        except ContractError as error:
            result["error"] = error.as_dict()
            result["events"] = read_events(events_path)
        finally:
            result["cleanup"] = terminate_owned(process, member_pids)
        return result


def main() -> int:
    if OUTPUT.exists():
        raise ContractError(
            "manual.output_exists",
            "manual V8 output identity must be fresh and must not reuse prior evidence",
            output=str(OUTPUT),
        )
    if not EXECUTABLE.is_file() or not os.access(EXECUTABLE, os.X_OK):
        raise ContractError(
            "manual.bundle_missing",
            "signed release executable is missing or not executable",
            executable=str(EXECUTABLE),
        )
    OUTPUT.mkdir(parents=True, exist_ok=False)
    result: dict[str, Any] = {
        "schema": "herdr.t1-preflight.v8-manual.v1",
        "run_id": RUN_ID,
        "bundle": {
            "app": str(APP),
            "executable": str(EXECUTABLE),
            "identifier": "dev.herdr.integrated-preflight",
            "tree_sha256": sha256_tree(APP),
        },
        "manifest": {"path": str(MANIFEST), "sha256": sha256_file(MANIFEST)},
        "budgets": {
            "warm_usable_ms": 1000,
            "idle_cpu_percent": 1,
            "browser_closed_rss_mb": 200,
            "browser_included_rss_mb": 800,
            "terminal_input_to_present_p95_ms": 50,
        },
        "method": {
            "native_input": "NOT_MEASURED_BY_DESIGN",
            "terminal_input_to_present_p95_ms": "NOT_MEASURED",
            "sample_count": SAMPLE_COUNT,
            "sample_interval_ms": 500,
            "owned_cleanup": "exact bundle descendants and exact fixture server only",
        },
        "phases": {},
    }
    server = subprocess.Popen(
        [
            "/Library/Developer/CommandLineTools/usr/bin/python3",
            "-m",
            "http.server",
            str(HTTP_PORT),
            "--bind",
            "127.0.0.1",
            "--directory",
            str(FIXTURE),
        ],
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    try:
        wait_port(HTTP_PORT)
        result["fixture_server"] = {"pid": server.pid, "port": HTTP_PORT, "root": str(FIXTURE)}
        result["phases"]["warm_closed"] = launch_phase("browser-closed", "warm_closed", OUTPUT)
        result["phases"]["browser_included"] = launch_phase("browser-included", "browser_included", OUTPUT)
    finally:
        if server.poll() is None:
            os.kill(server.pid, signal.SIGTERM)
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            if server.poll() is None:
                os.kill(server.pid, signal.SIGKILL)
                server.wait(timeout=5)
        result["fixture_server_cleanup"] = {"pid": server.pid, "exit_code": server.returncode}
    statuses = [phase.get("status") for phase in result["phases"].values()]
    result["status"] = "PARTIAL_BLOCKED" if "BLOCKED" in statuses else "PARTIAL"
    result["blockers"] = [
        "native input, AX, screenshot, and terminal latency were intentionally not measured by this bounded process-tree discriminator",
        "full packaged runtime scenario remains blocked by the preserved input.invalid_point display-geometry failure",
    ]
    write_json(OUTPUT / "v8-manual-evidence.json", result)
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ContractError as error:
        print(json.dumps(error.as_dict(), ensure_ascii=False, indent=2), file=sys.stderr)
        raise SystemExit(2)
