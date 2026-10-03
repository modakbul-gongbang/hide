#!/usr/bin/env python3
"""Compare the pinned binary's own schema on every operating system.

This command never reads a server or changes an operator's configuration.
The zsh full contract check delegates its binary comparison here as well.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def command(binary, *arguments):
    environment = {key: value for key, value in os.environ.items() if not key.startswith("HERDR_")}
    return subprocess.run(
        [binary, *arguments], check=True, capture_output=True, text=True,
        encoding="utf-8", env=environment, timeout=30,
    ).stdout


def check(binary):
    root = Path(__file__).resolve().parent.parent
    contract = json.loads((root / "contracts/herdr-api.schema.json").read_text(encoding="utf-8"))
    pin = json.loads((root / "contracts/herdr-bundle.json").read_text(encoding="utf-8"))
    received = json.loads(command(binary, "api", "schema", "--json"))
    if not isinstance(received, dict):
        raise ValueError("Herdr CLI schema must be a JSON object")
    protocol = contract["protocol"]
    if not isinstance(protocol, int) or isinstance(protocol, bool):
        raise ValueError("the contract protocol must be an integer")
    canonical = lambda value: json.dumps(value, sort_keys=True, ensure_ascii=False, indent=2) + "\n"
    if canonical(received) != canonical(contract):
        raise ValueError(f"Herdr CLI schema differs: expected protocol={protocol}, received={received.get('protocol')}")
    version_fields = command(binary, "--version").split()
    if len(version_fields) != 2 or version_fields[0] != "herdr" or version_fields[1] != pin["version"]:
        raise ValueError(f"Herdr CLI version differs from contracts/herdr-bundle.json: {version_fields}")
    normalized = canonical(contract)
    return {
        "status": "pass", "scope": "schema-only", "herdr_bin": binary,
        "version": version_fields[1], "protocol": protocol,
        "schema_sha256": hashlib.sha256(normalized.encode("utf-8")).hexdigest(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--herdr-bin", default=shutil.which("herdr"))
    arguments = parser.parse_args()
    try:
        if not arguments.herdr_bin or not Path(arguments.herdr_bin).is_file():
            raise ValueError("executable Herdr CLI was not found")
        print(json.dumps(check(arguments.herdr_bin), ensure_ascii=False))
        return 0
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
