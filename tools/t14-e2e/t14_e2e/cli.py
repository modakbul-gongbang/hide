from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .model import ContractError, load_manifest
from .runtime import (
    CommandSnapshotClient,
    E2ERunner,
    MacOSInstanceInventory,
    MacOSNativeDriver,
    SubprocessController,
    inspect_bundle,
)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(prog="t14-e2e", description="Owned exactly-one-instance native E2E contract")
    result.add_argument("--root", type=Path, default=Path("."), help="worktree root")
    result.add_argument("--manifest", required=True, type=Path, help="root-relative T14 manifest")
    result.add_argument("--mode", choices=("contract", "dry-run", "run"), required=True)
    return result


def main(argv: list[str] | None = None) -> int:
    arguments = parser().parse_args(argv)
    try:
        root = arguments.root.resolve(strict=True)
        manifest_path = arguments.manifest if arguments.manifest.is_absolute() else root / arguments.manifest
        manifest = load_manifest(manifest_path, root=root)
        if arguments.mode == "contract":
            value = {
                "schema": "herdr.ide.t14-e2e.contract.v1",
                "status": "PASS",
                "run_id": manifest.run_id,
                "verification_profile": manifest.verification_profile,
                "manifest_sha256": manifest.digest,
                "scenario_sha256": manifest.scenario_sha256,
                "exact_command": manifest.exact_command,
                "app": {
                    "path": str(manifest.app_path),
                    "executable": str(manifest.executable),
                    "bundle_kind": manifest.bundle_kind,
                    "bundle_identifier": manifest.bundle_identifier,
                },
                "output_dir": str(manifest.output_dir),
                "ports": list(manifest.ports),
                "owned_resources": [resource.as_dict() for resource in manifest.owned_resources],
            }
            print(json.dumps(value, ensure_ascii=False, indent=2))
            return 0
        if arguments.mode == "dry-run":
            print(json.dumps({
                "schema": "herdr.ide.t14-e2e.dry-run.v1",
                "status": "DRY_RUN",
                "run_id": manifest.run_id,
                "manifest_sha256": manifest.digest,
                "scenario_sha256": manifest.scenario_sha256,
                "exact_command": manifest.exact_command,
                "native_side_effects": "none",
                "cleanup": "only the exact PID returned by this run; fixture resources require explicit confirmation",
            }, ensure_ascii=False, indent=2))
            return 0
        runner = E2ERunner(
            manifest,
            inventory=MacOSInstanceInventory(),
            process=SubprocessController(),
            native=MacOSNativeDriver(manifest),
            snapshots=CommandSnapshotClient(manifest),
            bundle_inspector=inspect_bundle,
        )
        result = runner.run()
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0 if result.get("status") == "PASS" else 5
    except ContractError as error:
        print(json.dumps(error.as_dict(), ensure_ascii=False, sort_keys=True), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
