#!/usr/bin/env python3
"""One fail-closed changed-path contract for required CI and its aggregate."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tomllib
import sys
import fnmatch
import importlib.util
import signal

VERSION = 1
LANES = ("docs", "rust", "checks", "web-checks", "desktop-checks", "windows-check", "windows-e2e", "web-e2e", "web-e2e-platform", "desktop-e2e", "os-contract", "quarantine-fixes")
INTEGRATION = {"herdr-core", "hided", "hide-host", "hide-herdr-client"}


def cargo_graph(root):
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    members = {}
    for directory in workspace["members"]:
        manifest = tomllib.loads((root / directory / "Cargo.toml").read_text())
        members[directory] = manifest
    names = {m["package"]["name"]: directory for directory, m in members.items()}
    reverse = {directory: set() for directory in members}
    for directory, manifest in members.items():
        tables = [manifest]
        tables.extend(manifest.get("target", {}).values())
        for table in tables:
            for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
                for alias, dependency in table.get(kind, {}).items():
                    name = dependency.get("package", alias) if isinstance(dependency, dict) else alias
                    if name in names:
                        reverse[names[name]].add(directory)
    return members, reverse


def select(root, entries, source, full_reason=None):
    members, reverse = cargo_graph(root)
    registry_path = root / "contracts/ci-quarantine.json"
    registry = json.loads(registry_path.read_text())["entries"] if registry_path.exists() else []
    required = set()
    reasons = {lane: [] for lane in LANES}
    packages = set()
    full = False
    def lanes(names, reason):
        for lane in names:
            reasons[lane].append(reason)
    def all_lanes(reason):
        nonlocal full
        full = True
        for lane in LANES:
            reasons[lane].append(reason)
        required.update(entry["id"] for entry in registry)
    if full_reason:
        all_lanes(full_reason)
    elif not entries:
        all_lanes("empty comparison")
    for status, filename in entries:
        for entry in registry:
            if any(fnmatch.fnmatchcase(filename, pattern) for pattern in entry["fix_paths"]):
                required.add(entry["id"])
                reasons["quarantine-fixes"].append(f"fix scenario {entry['id']}: {filename}")
        path = Path(filename)
        parts = path.parts
        if not parts or path.is_absolute() or ".." in parts:
            raise ValueError("invalid changed path")
        if status not in ("M", "A"):
            all_lanes(f"{status}: {filename}")
            continue
        # New executable/configuration payload has no established owner yet.
        if status == "A" and path.suffix != ".md":
            all_lanes(f"new payload: {filename}")
            continue
        if (parts[0] == "docs" and path.suffix == ".md") or filename in ("README.md", "CONTRIBUTING.md", "AGENTS.md") or (filename.startswith("agents/prd/") and path.suffix == ".md"):
            reasons["docs"].append(filename)
        elif parts[0] in members:
            affected = {parts[0]}
            pending = list(affected)
            while pending:
                for consumer in reverse[pending.pop()]:
                    if consumer not in affected:
                        affected.add(consumer)
                        pending.append(consumer)
            packages.update(members[p]["package"]["name"] for p in affected)
            if affected & INTEGRATION or parts[0] in ("hide-platform", "hide-kit", "hide-agent-hooks"):
                all_lanes(f"Rust ownership/OS consumers: {filename}")
            else:
                for lane in ("rust", "checks"):
                    reasons[lane].append(f"Rust reverse dependencies: {filename}")
        elif parts[0] in ("web", "desktop"):
            shared_fixture = filename.startswith(("web/e2e/", "desktop/e2e/")) and not filename.endswith(".spec.ts")
            if shared_fixture or path.name in ("package.json", "tsconfig.json") or "playwright.config" in filename:
                all_lanes(f"shared fixture/toolchain: {filename}")
                continue
            if parts[0] == "web":
                lanes(("web-checks", "web-e2e"), f"web package: {filename}")
                # These are shared input/state boundaries consumed by the
                # desktop shell and all three native transports.
                shared = filename.startswith(("web/src/terminal", "web/src/store", "web/src/keyboard", "web/src/shortcuts", "web/src/host", "web/src/snapshot", "web/src/wire")) or path.name in ("Terminal.tsx", "TerminalArea.tsx", "AreaTree.tsx", "AgentTabGroups.tsx", "BrowserDisplay.tsx")
                platform_spec = filename.endswith(".spec.ts") and "@platform" in (root / filename).read_text()
                if shared or platform_spec:
                    lanes(("desktop-checks", "desktop-e2e", "web-e2e-platform", "windows-e2e", "windows-check"), f"shared input/OS consumer: {filename}")
            else:
                lanes(("desktop-checks", "desktop-e2e"), f"desktop package: {filename}")
                # Main-process code owns native paths/process/input and the
                # host-daemon bridge; keep Windows compile and browser proof.
                if filename.startswith("desktop/src/main/"):
                    lanes(("rust", "windows-check", "windows-e2e"), f"native host/daemon consumer: {filename}")
                    packages.update(members[p]["package"]["name"] for p in INTEGRATION if p in members)
        elif parts[0] in ("plugins", "design", "contracts", "scripts", ".github"):
            # Shared shell, fixtures, scripts, pin and workflow boundaries are
            # deliberately conservative until a narrower contract is proved.
            all_lanes(f"shared execution boundary: {filename}")
        else:
            all_lanes(f"unknown owner: {filename}")
    if full or (reasons["rust"] and not packages):
        packages = {m["package"]["name"] for m in members.values()}
    observed = [entry["id"] for entry in registry if reasons["web-e2e" if entry["suite"] == "web" else "desktop-e2e"]]
    if source.get("event") and source["event"] != "pull_request":
        # Full main integration already includes every scenario once.
        reasons["quarantine-fixes"] = []
    return {"version": VERSION, "source": source, "lanes": {k: bool(v) for k, v in reasons.items()}, "reasons": reasons, "excluded": {k: "outside changed package and its declared consumers" for k, v in reasons.items() if not v}, "full": full, "rust_packages": sorted(packages), "quarantine_required": sorted(required), "quarantine_observe": sorted(observed)}


def aggregate(plan, results):
    if plan.get("version") != VERSION or set(plan.get("lanes", {})) != set(LANES) or not any(plan["lanes"].values()):
        raise ValueError("unknown or empty CI plan")
    if set(results) != set(LANES) | {"plan"}:
        raise ValueError("missing or unknown lane report")
    errors = []
    for name, result in results.items():
        expected = "success" if name == "plan" or plan["lanes"][name] else "skipped"
        if result.get("result") != expected:
            errors.append(f"{name}: expected {expected}, received {result.get('result')}")
    if errors:
        raise ValueError("; ".join(errors))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("plan", "aggregate", "rust"))
    parser.add_argument("--base")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--event", default=os.environ.get("GITHUB_EVENT_NAME", "pull_request"))
    parser.add_argument("--plan", default="ci-plan.json")
    parser.add_argument("--results")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if args.command == "plan":
        spec = importlib.util.spec_from_file_location("quarantine", root / "scripts/ci-quarantine.py")
        policy = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(policy)
        policy.validate(root)
        head = subprocess.check_output(["git", "rev-parse", args.head], cwd=root, text=True).strip()
        source = {"event": args.event, "head": head, "base": args.base, "tested": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()}
        entries, reason = [], None
        if args.event != "pull_request":
            reason = "full integration event"
        else:
            try:
                if not args.base:
                    raise ValueError("missing base")
                base = subprocess.check_output(["git", "merge-base", args.base, head], cwd=root, text=True).strip()
                source["merge_base"] = base
                raw = subprocess.check_output(["git", "diff", "--name-status", "-z", "--no-renames", base, head], cwd=root)
                fields = raw.decode("utf-8", errors="strict").split("\0")
                if fields[-1] != "" or len(fields[:-1]) % 2:
                    raise ValueError("malformed diff")
                entries = list(zip(fields[:-1:2], fields[1:-1:2]))
            except (ValueError, subprocess.CalledProcessError, UnicodeError) as error:
                reason = f"comparison unavailable: {type(error).__name__}"
        plan = select(root, entries, source, reason)
        Path(args.plan).write_text(json.dumps(plan, indent=2) + "\n")
        print(json.dumps(plan, indent=2))
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as output:
                for name, selected in plan["lanes"].items():
                    output.write(f"{name}={str(selected).lower()}\n")
                output.write(f"advisory={str(bool(plan['quarantine_observe'])).lower()}\n")
    elif args.command == "aggregate":
        plan = json.loads(Path(args.plan).read_text())
        tested = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        if plan["source"]["tested"] != tested:
            raise ValueError("CI plan belongs to a different tested checkout")
        aggregate(plan, json.loads(Path(args.results).read_text()))
    else:
        plan = json.loads(Path(args.plan).read_text())
        if plan.get("version") != VERSION or not plan["lanes"]["rust"] or not plan["rust_packages"]:
            raise ValueError("invalid Rust selection")
        packages = [arg for name in plan["rust_packages"] for arg in ("-p", name)]
        for mode, rest in (("clippy", ["--all-targets", "--", "-D", "warnings"]), ("test-scoped", [])):
            command = ["bash", "scripts/verify-cargo.sh", mode, *packages, *(["--message-format=json-render-diagnostics"] if mode == "test-scoped" else []), *rest]
            if mode == "test-scoped" and os.environ.get("CI_RUST_LOG"):
                with open(os.environ["CI_RUST_LOG"], "wb") as log:
                    process = subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
                    total = 0
                    try:
                        while chunk := process.stdout.read(65536):
                            total += len(chunk)
                            if total > 16 * 1024 * 1024:
                                raise ValueError("Rust log collection overflow")
                            log.write(chunk)
                            sys.stdout.buffer.write(chunk)
                        code = process.wait()
                    finally:
                        if process.poll() is None:
                            os.killpg(process.pid, signal.SIGTERM)
                            try:
                                process.wait(timeout=5)
                            except subprocess.TimeoutExpired:
                                os.killpg(process.pid, signal.SIGKILL)
                                process.wait()
                subprocess.run(["node", "scripts/ci-ledger.cjs", "rust", os.environ["CI_RUST_LOG"], os.environ["CI_LEDGER_PATH"]], cwd=root, check=True)
                if code:
                    sys.exit(code)
            else:
                subprocess.run(command, cwd=root, check=True)


if __name__ == "__main__":
    main()
