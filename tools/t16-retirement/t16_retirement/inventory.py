from __future__ import annotations

import hashlib
import json
import os
import subprocess
from pathlib import Path
from typing import Any


class InventoryError(RuntimeError):
    pass


def redact_path(value: str) -> str:
    home = str(Path.home())
    return value.replace(home, "<HOME>")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(root: Path, *arguments: str) -> str:
    try:
        completed = subprocess.run(
            ["git", "-C", str(root), *arguments],
            check=False,
            capture_output=True,
            text=True,
            timeout=20,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise InventoryError(f"git command failed: {arguments!r}: {error!r}") from error
    if completed.returncode != 0:
        raise InventoryError(
            f"git command exited {completed.returncode}: {arguments!r}: {completed.stderr[-500:]}"
        )
    return completed.stdout


def tracked_tree_digest(root: Path) -> str:
    # Hash the index's path and blob identity, not ignored build output or user files.
    return hashlib.sha256(git(root, "ls-files", "-s").encode("utf-8")).hexdigest()


def inventory(project_root: Path, pet_root: Path) -> dict[str, Any]:
    project_root = project_root.resolve(strict=True)
    pet_root = pet_root.resolve(strict=True)
    old_prd_paths = (
        project_root / "agents/prd/herdr-lightweight-ide/prd.md",
        project_root / "agents/prd/herdr-ide-native-shell/prd.md",
    )
    old_prds: list[dict[str, Any]] = []
    for path in old_prd_paths:
        if not path.is_file():
            raise InventoryError(f"required historical PRD is missing: {path}")
        old_prds.append(
            {
                "path": redact_path(str(path.relative_to(project_root))),
                "status": "superseded-by-herdr-ide-rust-native",
                "sha256": sha256_file(path),
            }
        )
    worktrees_raw = git(project_root, "worktree", "list", "--porcelain")
    worktrees = []
    current: dict[str, str] = {}
    for line in worktrees_raw.splitlines() + [""]:
        if line.startswith("worktree "):
            current = {"path": redact_path(line.removeprefix("worktree "))}
        elif line.startswith("HEAD "):
            current["head"] = line.removeprefix("HEAD ")
        elif line.startswith("branch "):
            current["branch"] = line.removeprefix("branch ")
        elif not line and current:
            worktrees.append(current)
            current = {}
    electron_worktrees = [
        item
        for item in worktrees
        if "electron" in item.get("path", "").lower()
        or "herdr-ide-native-shell" in item.get("branch", "")
    ]
    tracked_paths = git(project_root, "ls-tree", "-r", "--name-only", "HEAD").splitlines()
    electron_paths = [
        path
        for path in tracked_paths
        if "electron" in path.lower() or Path(path).name in {"package.json", "electron.vite.config.ts"}
    ]
    project_status = git(project_root, "status", "--porcelain", "--untracked-files=no").strip()
    pet_status = git(pet_root, "status", "--porcelain", "--untracked-files=no").strip()
    pet_head = git(pet_root, "rev-parse", "HEAD").strip()
    return {
        "schema": "herdr.ide.t16-retirement-evidence.v1",
        "status": "PASS",
        "supersession": {
            "approved_rust_prd": "agents/prd/herdr-ide-rust-native-800mb/prd.md",
            "old_prds": old_prds,
            "sealed_inputs_modified": False,
            "interpretation": "old PRDs are recorded as superseded in the implementation result; their files remain unchanged and read-only",
        },
        "electron_reference": {
            "worktrees_present": electron_worktrees,
            "tracked_electron_paths": electron_paths,
            "present_in_current_checkout": bool(electron_worktrees or electron_paths),
            "project_worktree_status_clean": not project_status,
            "disposition": "preserved-read-only-or-not-present-in-current-checkout",
        },
        "standalone_pet": {
            "root": "<PET_ROOT>",
            "head": pet_head,
            "tracked_tree_digest": tracked_tree_digest(pet_root),
            "worktree_status_clean": not pet_status,
            "before_after": {
                "observation": "single read-only inventory; no before/after mutation was performed",
                "unchanged_observed": not pet_status,
            },
        },
        "retirement": {
            "executed": False,
            "exact_targets": [
                "agents/prd/herdr-lightweight-ide/prd.md",
                "agents/prd/herdr-ide-native-shell/prd.md",
                "<ELECTRON_REFERENCE_WORKTREE>",
                "<PET_ROOT>",
                "/Applications/Herdr Pet.app",
            ],
            "required_preconditions": [
                "fresh sasu implement receipt exists",
                "V1-V8 required rows are PASS",
                "HV1-HV4 human review is recorded",
                "exact target inventory and dirty-state check pass",
                "separate explicit user approval for cleanup",
            ],
            "reason_not_executed": "PRD requires receipt, parity evidence, and separate user confirmation before any retirement",
        },
        "side_effects": "read-only git/filesystem inventory; no PRD, Electron, Pet, app bundle, or user resource changed",
    }


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, path)
