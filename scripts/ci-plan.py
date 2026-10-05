#!/usr/bin/env python3
"""Which `pr.yml` lanes a change needs, and whether `verify` may pass.

`plan` classifies the paths a pull request changes into lanes; every job in
`pr.yml` runs when its lane is in the plan, and `aggregate` is `verify`'s one
step: a planned lane must have succeeded and an unplanned one must have been
skipped, so a lane whose `if:` is wrong fails `verify` instead of passing it.

A push to main, a comparison that cannot be computed, and a path no rule
claims all plan every lane. docs/TESTING.md, "Which lanes a pull request
runs", owns the rules and the reasons for them.
"""
import argparse
from fnmatch import fnmatchcase
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent

# The order is the order the summary prints.
LANES = (
    "policy",
    "rust",
    "web-checks",
    "desktop-checks",
    "os-contract",
    "windows-check",
    "windows-e2e",
    "web-e2e",
    "web-e2e-platform",
    "desktop-e2e",
)

# Crates whose change reaches what differs by operating system: the platform
# layer, the Herdr client's IPC, the install kit and hooks, and the daemon and
# core that `windows check` and the OS contract run on each system.
OS_CRATES = {
    "herdr-core",
    "hided",
    "hide-agent-hooks",
    "hide-herdr-client",
    "hide-host",
    "hide-kit",
    "hide-platform",
}
OS_LANES = ("os-contract", "windows-check", "windows-e2e", "web-e2e-platform", "desktop-e2e")

# Web files the desktop host imports or drives through native input: the host
# bridge, the shortcut registry the menu is built from, the store and
# snapshot, terminal input, and the focus and area code chords move.
SHARED_WEB = (
    "web/src/host.ts",
    "web/src/revealExternal.ts",
    "web/src/shortcuts.ts",
    "web/src/shortcutLabels.ts",
    "web/src/keyboard.ts",
    "web/src/keys.ts",
    "web/src/store.ts",
    "web/src/snapshot.ts",
    "web/src/ws.ts",
    "web/src/connection.ts",
    "web/src/terminals.ts",
    "web/src/terminalLinks.ts",
    "web/src/terminalLinkProvider.ts",
    "web/src/selection.ts",
    "web/src/areaCycle.ts",
    "web/src/areaFocus.ts",
    "web/src/viewFocus.ts",
    "web/src/browserViews.ts",
    "web/src/BrowserDisplay.tsx",
    "web/src/PaneView.tsx",
    "web/src/App.tsx",
    "web/src/main.tsx",
)
SHARED_WEB_LANES = ("desktop-checks", "desktop-e2e", "web-e2e-platform", "windows-e2e")

# Files a lane outside their own directory reads: a change to one also plans
# that lane, and the Rust crates whose tests read it. An entry ending in `/`
# is a folder. Keep this beside the test that reads the file.
READERS = (
    # herdr-core/src/runtime/tests/snapshot_delta.rs asserts the agent notes.
    ("AGENTS.md", {"rust"}, {"herdr-core"}),
    ("CLAUDE.md", {"rust"}, {"herdr-core"}),
    # herdr-core's structure tests scan the shell (runtime/tests.rs, sidebar.rs)
    # and the daemon (runtime/tests/snapshot_delta.rs).
    ("web/src/", {"rust"}, {"herdr-core"}),
    ("hided/src/", {"rust"}, {"herdr-core"}),
    # web/e2e/s2, s3 and s7 import the wire path conversion.
    ("desktop/src/main/wirePath.ts", {"web-checks", "web-e2e", "web-e2e-platform", "windows-e2e"}, set()),
    # web/src/settings.test.ts, theme.test.ts and web/e2e/theme.spec.ts read the tokens.
    ("design/tokens.json", {"web-checks", "web-e2e"}, set()),
)


# Paths named here are the only ones narrower than "every lane"; a path no rule
# names still plans every lane. Each entry says who reads the path, and
# `scripts/tests/test_ci_plan.py` checks the claim where it can be checked.
#
# Paths no `pr.yml` lane reads: `policy` alone. Its script suite and the
# repository invariants are what reads them.
POLICY_ONLY = (
    "agents/*", "site/*", "plugins/*", "tools/*", "spikes/*",
    ".gitignore", "web/.gitignore", "desktop/.gitignore",
    ".github/pull_request_template.md", ".github/dependabot.yml",
    # Workflows no `pr.yml` job calls; `package.yml` and `design-contract.yml`
    # have their own pull request triggers.
    ".github/workflows/nightly.yml", ".github/workflows/package.yml", ".github/workflows/release.yml",
    ".github/workflows/herdr-update.yml", ".github/workflows/design-contract.yml",
    "scripts/tests/*",
    # Scripts only `policy`, another workflow or nobody runs.
    "scripts/nightly-report.cjs",
    "scripts/check-agent-asset-committed.sh", "scripts/check-capability-readers-off-lock.sh",
    "scripts/check-harness-ignore-anchor.sh", "scripts/check-herdr-pin-single-source.sh",
    "scripts/check-no-workstation-identity.*", "scripts/check-worktree-removal-boundary.sh",
    "scripts/check-hide-full.sh", "scripts/check-hide-screens.mjs", "scripts/check-typed-live-remote.sh",
    "scripts/check-release-assets.mjs", "scripts/release-draft.mjs",
    "scripts/pen-*.mjs", "scripts/design-review.mjs", "scripts/web-shell-measure/*",
)

# Paths whose readers are a known set of lanes. `web/e2e` helpers are imported
# by the desktop suites too, and `desktop/e2e` unit tests run in `windows
# check`, so neither is web-only or desktop-only.
WEB_E2E_LANES = {"web-checks", "web-e2e", "web-e2e-platform", "windows-e2e", "desktop-checks", "desktop-e2e", "windows-check"}
DESKTOP_E2E_LANES = {"desktop-checks", "desktop-e2e", "windows-check"}
NAMED_LANES = (
    ("web/e2e/*", WEB_E2E_LANES),
    ("desktop/e2e/*", DESKTOP_E2E_LANES),
    ("web/playwright.config.ts", {"web-checks", "web-e2e", "web-e2e-platform", "windows-e2e"}),
    ("desktop/playwright.config.ts", DESKTOP_E2E_LANES),
    ("desktop/vitest.config.ts", DESKTOP_E2E_LANES),
    # `web` and `desktop` lint share the e2e rules and the size check.
    ("web/eslint.config.js", {"web-checks", "desktop-checks"}),
    ("web/eslint.e2e.mjs", {"web-checks", "desktop-checks"}),
    ("web/eslint-rules/*", {"web-checks", "desktop-checks"}),
    ("web/scripts/check-e2e-test-size.mjs", {"web-checks", "desktop-checks"}),
    ("web/scripts/gen-types.mjs", {"web-checks", "web-e2e"}),
    ("desktop/eslint.config.mjs", {"desktop-checks"}),
    ("desktop/eslint.globals.mjs", {"desktop-checks"}),
    # `build.mjs` is the e2e's build; `package.mjs` and `smoke-package.mjs` run
    # in `package.yml`, which has its own trigger on this folder.
    ("desktop/scripts/build.mjs", {"desktop-checks", "desktop-e2e"}),
    ("desktop/scripts/package.mjs", {"desktop-checks"}),
    ("desktop/scripts/smoke-package.mjs", {"desktop-checks"}),
)


def named_lanes(path):
    """The lanes a named path needs, or None when no rule names it."""
    name = path.as_posix()
    if any(fnmatchcase(name, pattern) for pattern in POLICY_ONLY):
        return set(), f"named path, no lane reads it: {name}"
    # Documentation below a folder no rule claims (a crate's own README is
    # claimed by its crate, which may include it in a doc test).
    if path.suffix == ".md":
        return set(), f"documentation: {name}"
    for pattern, lanes in NAMED_LANES:
        if fnmatchcase(name, pattern):
            return set(lanes), f"named path read by a known set of lanes: {name}"
    return None


def cargo_crates(root=ROOT):
    """Each workspace crate's directory, mapped to the crates that consume it."""
    raw = subprocess.check_output(
        ["bash", "scripts/verify-cargo.sh", "metadata", "--format-version", "1", "--no-deps"],
        cwd=root,
        stderr=subprocess.PIPE,
    )
    metadata = json.loads(raw)
    workspace = PurePosixPath(Path(metadata["workspace_root"]).as_posix())
    directory = {}
    for package in metadata["packages"]:
        manifest = PurePosixPath(Path(package["manifest_path"]).as_posix())
        directory[package["name"]] = manifest.parent.relative_to(workspace).as_posix()
    consumers = {name: set() for name in directory}
    for package in metadata["packages"]:
        for dependency in package["dependencies"]:
            # Normal, build and dev edges alike: a dev-dependency's tests run
            # against the changed crate too.
            if dependency.get("path") and dependency["name"] in directory:
                consumers[dependency["name"]].add(package["name"])
    return {directory[name]: {"name": name, "consumers": consumers[name]} for name in directory}


def reverse_closure(crates, start):
    """The crate named `start` and every crate that depends on it, transitively."""
    by_name = {crate["name"]: crate for crate in crates.values()}
    seen, pending = {start}, [start]
    while pending:
        for consumer in by_name[pending.pop()]["consumers"]:
            if consumer not in seen:
                seen.add(consumer)
                pending.append(consumer)
    return seen


def is_docs(path):
    parts = path.parts
    if parts[0] == "docs":
        return True
    if path.name in ("AGENTS.md", "CLAUDE.md"):
        return True
    return len(parts) == 1 and path.suffix == ".md"


def classify(path, status, crates, root):
    """The lanes one changed path needs, a reason, and the Rust packages it reaches.

    None for the lanes means the path needs every lane.
    """
    parts = path.parts
    if status not in ("A", "M", "D"):
        return None, f"{status} change: {path}", set()
    if is_docs(path):
        return set(), f"documentation: {path}", set()
    if parts[0] == "design":
        # design-contract.yml checks the library; no pr.yml lane reads it.
        return set(), f"design library: {path}", set()
    crate = crates.get(parts[0])
    if crate is not None:
        packages = reverse_closure(crates, crate["name"])
        # Every crate compiles into the Windows workspace `windows check` builds.
        lanes = {"rust", "windows-check"}
        # Every end-to-end lane drives hided, so a crate it links reaches them.
        if "hided" in packages:
            lanes.add("web-e2e")
        if crate["name"] in OS_CRATES:
            lanes.update(OS_LANES)
        return lanes, f"crate {crate['name']}: {path}", packages
    if parts[0] == "web" and len(parts) > 1:
        if parts[1] == "e2e" and len(parts) == 3 and path.name.endswith(".spec.ts"):
            lanes = {"web-checks", "web-e2e"}
            spec = root / path
            # A deleted spec cannot be read; keep the platform lanes for it.
            if status == "D" or "@platform" in spec.read_text(encoding="utf-8"):
                lanes.update(("web-e2e-platform", "windows-e2e"))
            return lanes, f"web e2e spec: {path}", set()
        if parts[1] in ("src", "public") or path.as_posix() in ("web/index.html", "web/mobile.html"):
            lanes = {"web-checks", "web-e2e"}
            if path.as_posix() in SHARED_WEB:
                lanes.update(SHARED_WEB_LANES)
                return lanes, f"web shared with the desktop host: {path}", set()
            return lanes, f"web shell: {path}", set()
    if parts[0] == "desktop" and len(parts) > 1:
        if parts[1] == "e2e" and len(parts) == 3 and path.name.endswith(".spec.ts"):
            return {"desktop-checks", "desktop-e2e"}, f"desktop e2e spec: {path}", set()
        if parts[1] in ("src", "static"):
            lanes = {"desktop-checks", "desktop-e2e"}
            if parts[1:3] == ("src", "main"):
                # The main process owns paths, child processes and the daemon
                # start; its unit suite runs on Windows in `windows check`.
                lanes.add("windows-check")
            return lanes, f"desktop host: {path}", set()
    named = named_lanes(path)
    if named is not None:
        return named[0], named[1], set()
    # Workflows, scripts, contracts and the Herdr pin, lockfiles, toolchain and
    # package configuration, and any path no rule above or in `named_lanes`
    # names: every lane.
    return None, f"shared or unclassified: {path}", set()


def select(entries, crates, root=ROOT, full_reason=None):
    """The plan for a list of (status, path) entries from `git diff --name-status`."""
    reasons = {lane: [] for lane in LANES}
    packages = set()
    full = []
    if full_reason:
        full.append(full_reason)
    elif not entries:
        full.append("the comparison listed no changed path")
    for status, name in entries:
        path = PurePosixPath(name)
        if not name or path.is_absolute() or ".." in path.parts:
            raise ValueError(f"invalid changed path: {name!r}")
        lanes, reason, reached = classify(path, status, crates, root)
        if lanes is None:
            full.append(reason)
            continue
        for reader, extra_lanes, extra_packages in READERS:
            if name == reader or (reader.endswith("/") and name.startswith(reader)):
                lanes |= extra_lanes
                reached = reached | extra_packages
        packages |= reached
        reasons["policy"].append(reason)
        for lane in lanes:
            reasons[lane].append(reason)
    if full:
        for lane in LANES:
            reasons[lane] = list(full)
        packages = {crate["name"] for crate in crates.values()}
    # The OS contract's macOS leg runs inside `desktop e2e`.
    if reasons["os-contract"] and not reasons["desktop-e2e"]:
        reasons["desktop-e2e"].append("the OS contract's macOS leg")
    planned = [lane for lane in LANES if reasons[lane]]
    return {
        "full": bool(full),
        "lanes": planned,
        "rust_packages": sorted(packages) if reasons["rust"] else [],
        "reasons": {lane: reasons[lane] for lane in planned},
    }


def changed_entries(base, head, root=ROOT):
    raw = subprocess.check_output(
        ["git", "diff", "--name-status", "-z", "--no-renames", base, head], cwd=root, stderr=subprocess.PIPE
    )
    fields = raw.decode("utf-8").split("\0")
    if fields[-1] != "" or len(fields[:-1]) % 2:
        raise ValueError("malformed diff output")
    return list(zip(fields[:-1:2], fields[1:-1:2]))


def plan(event, base, head, root=ROOT, crates=None, draft=False):
    if draft:
        # A draft is not a merge candidate: no lane runs, `aggregate` fails
        # `verify` for it, and marking the pull request ready for review
        # starts the run that plans lanes.
        return {"full": False, "lanes": [], "rust_packages": [], "reasons": {}, "draft": True}
    if crates is None:
        try:
            crates = cargo_crates(root)
        except (subprocess.CalledProcessError, ValueError, KeyError) as error:
            # Without the crate graph no Rust change can be narrowed; the full
            # Rust lane runs the whole workspace and needs no package list.
            return select([], {}, root, f"crate graph unavailable: {error}")
    if event != "pull_request":
        return select([], crates, root, f"{event}: every lane runs on main")
    try:
        # A pull request's checkout is the merge commit GitHub made; anything
        # else would compare against a parent that is not the base.
        parents = subprocess.check_output(
            ["git", "rev-list", "--parents", "-n", "1", head], cwd=root, text=True, stderr=subprocess.PIPE
        ).split()
        if len(parents) != 3:
            raise ValueError(f"{head} is not a merge commit")
        entries = changed_entries(base, head, root)
    except subprocess.CalledProcessError as error:
        detail = error.stderr.strip() if isinstance(error.stderr, str) else (error.stderr or b"").decode(errors="replace").strip()
        return select([], crates, root, f"comparison unavailable: {detail or error}")
    except (UnicodeDecodeError, ValueError) as error:
        return select([], crates, root, f"comparison unavailable: {error}")
    return select(entries, crates, root)


def aggregate(needs):
    """Raise unless every planned lane succeeded and every other lane was skipped."""
    if set(needs) != set(LANES) | {"plan"}:
        missing = sorted((set(LANES) | {"plan"}) - set(needs))
        unknown = sorted(set(needs) - set(LANES) - {"plan"})
        raise ValueError(f"verify's needs do not match the lanes: missing {missing}, unknown {unknown}")
    if needs["plan"]["result"] != "success":
        raise ValueError(f"plan: {needs['plan']['result']}")
    # A skipped required check counts as passed, so a draft's `verify` fails
    # instead; the run that follows marking it ready replaces this one.
    if needs["plan"]["outputs"].get("draft") == "true":
        raise ValueError("draft: lanes not run, mark ready for review")
    planned = json.loads(needs["plan"]["outputs"]["lanes"])
    if not planned or not set(planned) <= set(LANES):
        raise ValueError(f"plan named no lane or an unknown one: {planned}")
    errors = []
    for lane in LANES:
        expected = "success" if lane in planned else "skipped"
        result = needs[lane]["result"]
        if result != expected:
            errors.append(f"{lane}: {'planned' if lane in planned else 'not planned'}, expected {expected}, got {result}")
    if errors:
        raise ValueError("; ".join(errors))


def summary(result):
    if result.get("draft"):
        return "A draft pull request plans no lane; marking it ready for review runs them.\n"
    lines = ["| Lane | Runs | Why |", "| --- | --- | --- |"]
    for lane in LANES:
        reasons = result["reasons"].get(lane, [])
        why = "; ".join(reasons[:3]) + (f" (+{len(reasons) - 3} more)" if len(reasons) > 3 else "")
        lines.append(f"| {lane} | {'yes' if reasons else 'no'} | {why} |")
    if result["rust_packages"]:
        lines.append("")
        lines.append("Rust packages: " + ", ".join(result["rust_packages"]))
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    plan_parser = sub.add_parser("plan", help="classify base..head and write the job outputs")
    plan_parser.add_argument("--event", required=True)
    plan_parser.add_argument("--base", default="HEAD^1")
    plan_parser.add_argument("--head", default="HEAD")
    plan_parser.add_argument("--draft", choices=("true", "false"), default="false", help="the pull request is a draft")
    sub.add_parser("aggregate", help="check toJSON(needs), read from NEEDS")
    args = parser.parse_args()

    if args.command == "plan":
        result = plan(args.event, args.base, args.head, draft=args.draft == "true")
        print(json.dumps(result, indent=2))
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
                output.write(f"lanes={json.dumps(result['lanes'])}\n")
                output.write(f"full={str(result['full']).lower()}\n")
                output.write(f"draft={str(result.get('draft', False)).lower()}\n")
                output.write(f"rust-packages={json.dumps(result['rust_packages'])}\n")
        if os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
                output.write(summary(result))
    else:
        needs = json.loads(os.environ["NEEDS"])
        try:
            aggregate(needs)
        except ValueError as error:
            print(f"verify failed: {error}", file=sys.stderr)
            print(json.dumps(needs, indent=2), file=sys.stderr)
            sys.exit(1)
        print("every planned lane succeeded and every other lane was skipped")


if __name__ == "__main__":
    main()
