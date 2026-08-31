#!/usr/bin/env python3
"""Capture one owned release window on a selected Retina NSScreen.

The script is intentionally a small visual-evidence boundary. It does not
reuse an output directory, terminate anything it did not launch, or infer a
Retina capture from the host display inventory alone. The app-owned runtime
report must also record a backing scale of at least two after placement.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from types import SimpleNamespace
from typing import Any


def _packages(root: Path) -> None:
    for relative in ("tools/t14-e2e", "tools/t1-preflight"):
        path = root / relative
        if str(path) not in sys.path:
            sys.path.insert(0, str(path))


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="capture-visual-fidelity")
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--state", choices=("normal", "attention", "remote"), required=True
    )
    parser.add_argument("--autoclose-ms", type=int, default=5000)
    return parser.parse_args()


def _read_json(path: Path) -> dict[str, Any] | None:
    if not path.is_file():
        return None
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) else None


def _wait_for_window(driver: Any, process: Any, timeout_s: float) -> int:
    deadline = time.monotonic() + timeout_s
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"owned app exited before window appeared: {process.poll()}")
        try:
            return int(driver.owned_window_id(process.pid))
        except Exception as error:  # the window is an external AppKit boundary
            last_error = error
            time.sleep(0.1)
    raise RuntimeError(f"owned AppKit window did not appear: {last_error!r}")


def main() -> int:
    arguments = parse_args()
    root = arguments.root.resolve(strict=True)
    app = (root / arguments.app).resolve(strict=True)
    executable = app / "Contents/MacOS/herdr-integrated-preflight"
    output = (root / arguments.output).resolve(strict=False)
    if output.exists():
        raise RuntimeError(f"visual output identity already exists: {output}")
    if not executable.is_file() or not os.access(executable, os.X_OK):
        raise RuntimeError(f"release executable is missing or not executable: {executable}")
    output.mkdir(parents=True)
    report_path = output / "runtime.json"
    screenshot_path = output / "release-window.png"

    _packages(root)
    from t1_preflight.macos import (  # pylint: disable=import-outside-toplevel
        capture_window,
        frontmost_application_identity,
        screen_inventory,
        select_retina_screen,
    )
    from t14_e2e.runtime import (  # pylint: disable=import-outside-toplevel
        MacOSInstanceInventory,
        MacOSNativeDriver,
        SubprocessController,
    )
    from t14_e2e.model import sha256_tree  # pylint: disable=import-outside-toplevel

    manifest_stub = SimpleNamespace(telemetry_path=None)
    driver = MacOSNativeDriver(manifest_stub)
    inventory = MacOSInstanceInventory()
    controller = SubprocessController()
    existing = inventory.pids_for_executable(executable)
    if existing:
        raise RuntimeError(f"exact target already has running instances: {existing}")

    screens = screen_inventory()
    selected = select_retina_screen(screens)
    if float(selected["backing_scale_factor"]) < 2.0:
        raise RuntimeError("selected screen does not meet backingScaleFactor>=2")

    process = None
    window_id: int | None = None
    placement: dict[str, Any] | None = None
    activation: dict[str, Any] | None = None
    screenshot: dict[str, Any] | None = None
    frontmost_at_capture: dict[str, Any] | None = None
    cleanup: dict[str, Any] = {"status": "not-run"}
    status = "BLOCKED"
    failure: str | None = None
    started = time.monotonic()
    try:
        process = controller.launch(
            executable,
            cwd=root,
            arguments=(
                "--browser-closed",
                "--autoclose-ms",
                str(arguments.autoclose_ms),
                "--t1-visual-state",
                arguments.state,
                "--report",
                str(report_path),
            ),
        )
        if inventory.pids_for_executable(executable) != [process.pid]:
            raise RuntimeError("owned release app was not the exact single target instance")
        window_id = _wait_for_window(driver, process, 10.0)
        placement = driver.place_on_retina_screen(process.pid)
        activation = driver.activate(process.pid)
        # Moving across screens emits viewDidChangeBackingProperties. Allow that
        # event and one render tick to settle before taking the owned screenshot.
        time.sleep(0.8)
        window_id = driver.owned_window_id(process.pid)
        screenshot = capture_window(window_id, screenshot_path)
        frontmost_at_capture = frontmost_application_identity()
        frontmost_identity = frontmost_at_capture.get("identity", {})
        if frontmost_identity.get("process_identifier") != process.pid:
            raise RuntimeError(
                "owned app was not frontmost at screenshot boundary: "
                f"{frontmost_identity!r}"
            )
        time.sleep(0.2)
        runtime = _read_json(report_path)
        if runtime is None:
            raise RuntimeError("app-owned runtime report is missing or invalid")
        if float(runtime.get("scale_factor", 0.0)) < 2.0:
            raise RuntimeError(
                f"runtime report did not prove Retina backing scale: {runtime.get('scale_factor')!r}"
            )
        status = "CAPTURED_FOR_HUMAN_REVIEW"
    except Exception as error:  # preserve a machine-readable failure below
        failure = repr(error)
    finally:
        if process is not None:
            cleanup = controller.terminate_owned(process)
        remaining = inventory.pids_for_executable(executable)
        cleanup["remaining_target_pids"] = remaining

    runtime = _read_json(report_path)
    metadata = {
        "schema": "herdr.ide.visual-fidelity.v6",
        "status": status,
        "state": arguments.state,
        "build": {
            "kind": "release-bundle",
            "app": str(arguments.app),
            "executable": str(arguments.app / "Contents/MacOS/herdr-integrated-preflight"),
            "bundle_sha256": sha256_tree(app),
            "pid": process.pid if process is not None else None,
            "window_id": window_id,
        },
        "screens": screens,
        "selected_screen": selected,
        "placement": placement,
        "activation": activation,
        "runtime": runtime,
        "capture": screenshot,
        "frontmost_at_capture": frontmost_at_capture,
        "cleanup": cleanup,
        "elapsed_ms": (time.monotonic() - started) * 1000,
        "failure": failure,
        "human_review": {
            "references": [
                "docs/design-reference/orca-01-agent-editor-terminal.png",
                "docs/design-reference/orca-02-browser-grab.png",
                "docs/design-reference/orca-03-status-bar.png",
            ],
            "taste_verdict_required": True,
            "state_capture": arguments.state,
        },
    }
    metadata_path = output / "visual-fidelity.json"
    metadata_path.write_text(
        json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(metadata, ensure_ascii=False, indent=2))
    return 0 if status == "CAPTURED_FOR_HUMAN_REVIEW" else 5


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(json.dumps({"status": "BLOCKED", "error": repr(error)}), file=sys.stderr)
        raise SystemExit(2)
