#!/usr/bin/env python3
"""Installs agent CLIs on a CI runner at the versions contracts/agent-clis.json pins.

Usage: install-agent-clis.py <kind>... [--prefix DIR]

CI only (CI=true): on a workstation each CLI is the operator's, and this would
hand a test a second copy. Each CLI goes under DIR (default
$RUNNER_TEMP/agent-clis), and the script prints one `<kind>=<program>` line
per CLI; with GITHUB_PATH set it puts each program's folder on the PATH of
the later steps, and with GITHUB_OUTPUT set it writes the same lines as the
step's outputs. A CLI whose installer takes no version is installed at the
vendor's current release; a CLI that answers another version than the
table's is named in a warning here, and the journey's lane names it again
from its report.
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

CHECKOUT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CHECKOUT / "scripts"))
from agent_journey.clis import TABLE, matches, table as pins  # noqa: E402

WINDOWS = os.name == "nt"
# Each vendor's documented installer for a pinned release.
GROK_INSTALLER = "https://x.ai/cli/install.sh"
OMP_INSTALLER = "https://omp.sh/install"


def table():
    rows = pins()
    for kind, row in rows.items():
        if not row.get("version") or row.get("installer") not in INSTALLERS:
            raise SystemExit(f"{TABLE}: {kind} needs a version and one of the installers {sorted(INSTALLERS)}")
    return rows


def bash():
    """The bash on PATH: on Windows, Git Bash. A bare `bash` would be looked
    up in System32 first, where the WSL launcher is."""
    program = shutil.which("bash")
    if not program:
        raise SystemExit("bash is not on PATH")
    return program


def run(argv, env=None):
    """A command whose output goes to stderr, so stdout carries only the programs."""
    result = subprocess.run(argv, env={**os.environ, **(env or {})}, stdout=sys.stderr, timeout=600)
    if result.returncode:
        raise SystemExit(f"{argv[0]} exited {result.returncode}")


def npm(kinds, rows, prefix):
    """One npm install of every npm CLI into its own prefix, as the trust contract installs them."""
    folder = prefix / "npm"
    program = shutil.which("npm")
    if not program:
        raise SystemExit("npm is not on PATH: set up Node before this step")
    run([program, "install", "--prefix", str(folder), "--no-audit", "--no-fund",
         *(f"{rows[kind]['package']}@{rows[kind]['version']}" for kind in kinds)])
    # npm's shim on Windows is a `.cmd` beside the script it runs elsewhere;
    # a package that names its native program is reached by that path instead.
    return {kind: folder / rows[kind].get("program", f"node_modules/.bin/{kind}{'.cmd' if WINDOWS else ''}")
            for kind in kinds}


def grok(row, prefix):
    """Grok's installer takes the version as its argument and the folder as
    GROK_BIN_DIR; on Windows it runs under Git Bash in the same form."""
    folder = prefix / "grok"
    run([bash(), "-c", f'curl -fsSL {GROK_INSTALLER} | bash -s "$0"', row["version"]],
        {"GROK_BIN_DIR": folder.as_posix() if WINDOWS else str(folder)})
    return folder / ("grok.exe" if WINDOWS else "grok")


def omp(row, prefix):
    """omp's installer takes a release tag in its binary mode and the folder
    as PI_INSTALL_DIR."""
    if WINDOWS:
        # Its Windows form (install.ps1 -Binary -Ref) is set with the Windows journey (#1327).
        raise SystemExit("omp: the Windows installer form is not set yet")
    folder = prefix / "omp"
    run(["sh", "-c", f'curl -fsSL {OMP_INSTALLER} | sh -s -- --binary --ref "v$0"', row["version"]],
        {"PI_INSTALL_DIR": str(folder)})
    return folder / "omp"


def agy(row, prefix):
    """agy's installer takes no version (scripts/install-agy.sh)."""
    del row, prefix
    found = subprocess.run([bash(), str(CHECKOUT / "scripts/install-agy.sh")], capture_output=True, text=True,
                           timeout=600)
    sys.stderr.write(found.stderr)
    if found.returncode:
        raise SystemExit(f"install-agy.sh exited {found.returncode}")
    return Path(found.stdout.strip())


INSTALLERS = {"npm": None, "grok": grok, "omp": omp, "agy": agy}


def answered(program):
    """What the program says its version is."""
    result = subprocess.run([str(program), "--version"], capture_output=True, text=True, timeout=120,
                            stdin=subprocess.DEVNULL)
    return (result.stdout + result.stderr).strip().splitlines()[0][:120] if result.stdout + result.stderr else ""


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("kinds", nargs="+")
    parser.add_argument("--prefix", type=Path)
    args = parser.parse_args(argv)
    if os.environ.get("CI") != "true":
        print("install-agent-clis.py installs CLIs on a CI runner only", file=sys.stderr)
        return 2
    rows = table()
    unknown = [kind for kind in args.kinds if kind not in rows]
    if unknown:
        raise SystemExit(f"not in {TABLE.name}: {', '.join(unknown)}")
    prefix = (args.prefix or Path(os.environ["RUNNER_TEMP"]) / "agent-clis").resolve()
    prefix.mkdir(parents=True, exist_ok=True)
    kinds = list(dict.fromkeys(args.kinds))
    programs = npm([kind for kind in kinds if rows[kind]["installer"] == "npm"], rows, prefix) \
        if any(rows[kind]["installer"] == "npm" for kind in kinds) else {}
    for kind in kinds:
        if rows[kind]["installer"] != "npm":
            programs[kind] = INSTALLERS[rows[kind]["installer"]](rows[kind], prefix)
    lines, notes = [], []
    for kind in kinds:
        program = programs[kind]
        if not program.is_file():
            raise SystemExit(f"{kind}: the installer left no program at {program}")
        version = answered(program)
        notes.append(f"{kind} {version}")
        if not matches(rows[kind]["version"], version):
            pinned = rows[kind].get("pinned")
            print(f"::warning title=agent CLI version::{kind} answered {version!r}, the table pins "
                  f"{rows[kind]['version']}" + (f" ({pinned})" if pinned else ""))
        lines.append(f"{kind}={program}")
    print(f"::notice title=agent CLIs::{', '.join(notes)}")
    for name, value in (("GITHUB_PATH", "\n".join(dict.fromkeys(str(programs[kind].parent) for kind in kinds))),
                        ("GITHUB_OUTPUT", "\n".join(lines))):
        if os.environ.get(name):
            with open(os.environ[name], "a", encoding="utf-8") as stream:
                stream.write(value + "\n")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
