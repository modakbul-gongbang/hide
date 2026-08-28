from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .model import ContractError, load_manifest
from .runtime import RegressionRunner, SubprocessCommandRunner


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(prog="t15-regression", description="T15 verification matrix")
    result.add_argument("--root", type=Path, default=Path("."))
    result.add_argument("--manifest", required=True, type=Path)
    result.add_argument("--mode", choices=("contract", "dry-run", "run"), required=True)
    return result


def main(argv: list[str] | None = None) -> int:
    arguments = parser().parse_args(argv)
    try:
        root = arguments.root.resolve(strict=True)
        manifest_path = arguments.manifest if arguments.manifest.is_absolute() else root / arguments.manifest
        manifest = load_manifest(manifest_path, root=root)
        if arguments.mode == "contract":
            print(json.dumps({
                "schema": "herdr.ide.t15-regression.contract.v1",
                "status": "PASS",
                "run_id": manifest.run_id,
                "verification_profile": manifest.verification_profile,
                "manifest_sha256": manifest.digest,
                "output_dir": str(manifest.output_dir),
                "checks": [check.as_dict() for check in manifest.checks],
            }, ensure_ascii=False, indent=2))
            return 0
        runner = RegressionRunner(manifest, command_runner=SubprocessCommandRunner())
        result = runner.run(execute=arguments.mode == "run")
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0 if result.get("status") in {"PASS", "PARTIAL"} else 5
    except ContractError as error:
        print(json.dumps(error.as_dict(), ensure_ascii=False, sort_keys=True), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
