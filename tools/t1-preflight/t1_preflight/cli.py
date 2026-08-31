from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any

from .macos import inspect_bundle
from .model import PHASES, ContractError, OutputStore, load_manifest
from .runtime import RuntimeHarness


def emit(event: str, **fields: Any) -> None:
    payload = {"event": event, "monotonic_ns": time.monotonic_ns(), **fields}
    print(
        json.dumps(payload, ensure_ascii=False, sort_keys=True),
        file=sys.stderr,
        flush=True,
    )


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(
        prog="t1-preflight",
        description="Fail-closed, manifest-driven macOS native app T1 verification",
    )
    result.add_argument(
        "--app", required=True, type=Path, help="release .app bundle path"
    )
    result.add_argument(
        "--manifest", required=True, type=Path, help="fixture manifest JSON path"
    )
    result.add_argument(
        "--mode",
        required=True,
        choices=("dry-run", "static", "run"),
        help="plan only, inspect release bundle, or collect full native runtime evidence",
    )
    return result


def dry_run_plan(app_path: Path, manifest: Any) -> dict[str, Any]:
    actions = []
    scenario = json.loads(manifest.scenario_path.read_text(encoding="utf-8"))
    for action in scenario["actions"]:
        name = action["action"]
        driver = "observation"
        if name in {
            "zoom.shortcut",
            "terminal.native_input",
            "terminal.plain_key_control",
            "terminal.option_meta",
            "ime.physical_keys",
        }:
            driver = "CGEvent"
        elif name == "focus.click":
            driver = "CGEvent mouse"
        elif name == "window.resize":
            driver = "macOS Accessibility action"
        elif name == "ax.snapshot":
            driver = "macOS Accessibility query"
        elif name == "screenshot.capture":
            driver = "screencapture window id"
        elif name.startswith("profile."):
            driver = "libproc and ps process-tree sampling"
        actions.append({"action": name, "driver": driver})
    return {
        "schema": "herdr.t1-preflight.dry-run-plan.v1",
        "status": "DRY_RUN",
        "run_id": manifest.run_id,
        "app_path": str(app_path.resolve(strict=False)),
        "manifest_sha256": manifest.digest,
        "output_dir": str(manifest.output_dir),
        "phases": [
            {
                "phase": phase,
                "argv": [
                    str(
                        app_path.resolve(strict=False)
                        / manifest.bundle["main_executable"]
                    ),
                    *manifest.render_arguments(
                        phase,
                        manifest.output_dir / "runtime" / f"{phase}.events.jsonl",
                        action_socket=Path("/tmp")
                        / f"herdr-t1-action-{manifest.digest[:12]}-{phase}.sock",
                    ),
                ],
            }
            for phase in PHASES
        ],
        "actions": actions,
        "native_action_policy": "The app emits readiness and state telemetry only; the harness drives every interaction through CGEvent, Accessibility, or screencapture.",
        "cleanup_policy": "Refuse pre-existing instances; terminate only the exact PID launched by this run after re-checking its executable path; never escalate SIGTERM automatically.",
    }


def main(argv: list[str] | None = None) -> int:
    arguments = parser().parse_args(argv)
    store: OutputStore | None = None
    try:
        manifest = load_manifest(arguments.manifest)
        store = OutputStore(manifest, arguments.app, arguments.mode)
        reused = store.prepare()
        if reused is not None:
            emit(
                "t1_preflight.reused",
                status=reused["status"],
                result=str(store.result_path),
            )
            print(json.dumps(reused, ensure_ascii=False, indent=2))
            return 0 if reused["status"] in {"PASS", "STATIC_PASS", "DRY_RUN"} else 5

        emit(
            "t1_preflight.started",
            mode=arguments.mode,
            run_id=manifest.run_id,
            output_dir=str(manifest.output_dir),
        )
        if arguments.mode == "dry-run":
            result = dry_run_plan(arguments.app, manifest)
            store.write_json("dry-run-plan.json", result)
            store.finish(result)
            emit(
                "t1_preflight.completed",
                status="DRY_RUN",
                result=str(store.result_path),
            )
            print(json.dumps(result, ensure_ascii=False, indent=2))
            return 0

        bundle_evidence = inspect_bundle(arguments.app, manifest)
        store.write_json("bundle-evidence.json", bundle_evidence)
        if arguments.mode == "static":
            result = {
                "schema": "herdr.t1-preflight.result.v1",
                "status": "STATIC_PASS",
                "run_id": manifest.run_id,
                "manifest_sha256": manifest.digest,
                "bundle_evidence": "bundle-evidence.json",
            }
            store.finish(result)
            emit(
                "t1_preflight.completed",
                status="STATIC_PASS",
                result=str(store.result_path),
            )
            print(json.dumps(result, ensure_ascii=False, indent=2))
            return 0

        runtime_evidence = RuntimeHarness(arguments.app, manifest).run()
        store.write_json("runtime-evidence.json", runtime_evidence)
        result = {
            "schema": "herdr.t1-preflight.result.v1",
            "status": runtime_evidence["status"],
            "run_id": manifest.run_id,
            "manifest_sha256": manifest.digest,
            "bundle_evidence": "bundle-evidence.json",
            "runtime_evidence": "runtime-evidence.json",
        }
        store.finish(result)
        emit(
            "t1_preflight.completed",
            status=result["status"],
            result=str(store.result_path),
        )
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0 if result["status"] == "PASS" else 5
    except ContractError as error:
        failure = error.as_dict()
        if store is not None and store.manifest.output_dir.is_dir():
            try:
                store.write_json("failure.json", failure)
            except Exception as write_error:
                failure["failure_artifact_error"] = repr(write_error)
        print(json.dumps(failure, ensure_ascii=False, sort_keys=True), file=sys.stderr)
        return 2
    except Exception as error:
        failure = {
            "event": "t1_preflight.failed",
            "code": "harness.unexpected",
            "message": "unexpected harness failure",
            "details": {"error": repr(error)},
        }
        if store is not None and store.manifest.output_dir.is_dir():
            try:
                store.write_json("failure.json", failure)
            except Exception as write_error:
                failure["failure_artifact_error"] = repr(write_error)
        print(json.dumps(failure, ensure_ascii=False, sort_keys=True), file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())
