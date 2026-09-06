#!/usr/bin/env python3
import json
import re
import subprocess
import sys
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(f"worktree performance evidence: {message}")


if len(sys.argv) > 2:
    fail("expected at most one performance evidence directory")
if len(sys.argv) == 2:
    root = Path(sys.argv[1])
else:
    listing = subprocess.run(
        ["git", "worktree", "list", "--porcelain"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    candidates = []
    for line in listing.splitlines():
        if not line.startswith("worktree "):
            continue
        candidate = (
            Path(line.removeprefix("worktree "))
            / "agents/runs/hide-agent-tree-and-worktree-panel/verification/performance"
        )
        if (candidate / "analysis.json").is_file():
            candidates.append(candidate)
    if len(candidates) != 1:
        fail(f"expected one record-root evidence directory, found {len(candidates)}")
    root = candidates[0]
analysis_path = root / "analysis.json"
try:
    analysis = json.loads(analysis_path.read_text())
except (OSError, json.JSONDecodeError) as error:
    fail(f"cannot read {analysis_path}: {error}")

protocol = analysis.get("protocol", {})
if protocol != {"idle_seconds": 20, "sample_seconds": 3, "sample_windows": 5}:
    fail(f"unexpected protocol: {protocol}")
if analysis.get("verdict") != "pass":
    fail("analysis verdict is not pass")
if analysis.get("difference_percentage_points", 1) > 0.5:
    fail("shown mutex wait exceeds hidden mode by more than 0.5 percentage points")

for mode_name in ("shown", "hidden"):
    mode = analysis.get("modes", {}).get(mode_name, {})
    if not mode.get("load_under_14") or mode.get("maximum_load_1m", 14) >= 14:
        fail(f"{mode_name} load was not below 14")
    if mode.get("new_git_or_du_observations") != 0:
        fail(f"{mode_name} observed a new git or du child process")
    if mode.get("valid_sample_windows") != 5:
        fail(f"{mode_name} does not have five valid sample windows")

    context = (root / mode_name / "context.txt").read_text()
    if "started_at=" not in context or "finished_at=" not in context:
        fail(f"{mode_name} context lacks measurement bounds")

    children = (root / mode_name / "children-20s.txt").read_text()
    seconds = {int(value) for value in re.findall(r"^second=(\d+) ", children, re.MULTILINE)}
    if seconds != set(range(1, 21)):
        fail(f"{mode_name} does not contain every 1-second child-process observation")
    process_lines = [line for line in children.splitlines() if line and not line.startswith("second=")]
    if any(re.search(r"(?:^|[/ ])(?:git|du)(?: |$)", line) for line in process_lines):
        fail(f"{mode_name} contains a git or du child process")

    valid = [sample for sample in mode.get("samples", []) if sample.get("valid")]
    if len(valid) != 5:
        fail(f"{mode_name} analysis does not identify five valid samples")
    for sample in valid:
        sample_path = Path(sample["path"])
        text = sample_path.read_text()
        if "Main Thread" not in text or sample.get("main_samples", 0) <= 0:
            fail(f"{sample_path} is not a symbolicated main-thread sample")

print("worktree performance evidence satisfies AC25")
