"""Use the pinned installer's assets, with only disposable config routes."""

import hashlib
import os
from pathlib import Path
import re

from .protection import (MAX_BACKUP_BYTES, ProtectionError, beneath, inventory_directory,
                         private_directory, stamp, write_private)


def prepare(runtime, recipe, overlay):
    if runtime.fixture_bin:
        return {"args": [], "artifacts": [], "sole_route": False, "synthetic": True}
    kind = recipe["kind"]
    home = runtime.probe / ("integration-" + recipe["id"])
    relative = {"claude": ".claude", "codex": ".codex", "pi": ".pi/agent",
                "omp": ".omp/agent", "opencode": ".config/opencode",
                "cursor": ".cursor", "grok": ".grok"}[kind]
    root = home / relative
    private_directory(root)
    # Only the install command has this disposable HOME. The authenticated
    # provider still uses operator HOME and its supported per-command roots.
    env = {**runtime.env, "HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"),
           "XDG_DATA_HOME": str(home / "data"), "XDG_STATE_HOME": str(home / "state"),
           "XDG_CACHE_HOME": str(home / "cache")}
    runtime.owner.run([str(runtime.herdr_bin), "integration", "install", kind], env=env, cwd=home)
    artifacts, total, pending, visited = [], 0, [root], 0
    while pending:
        file = pending.pop()
        visited += 1
        if visited > 128:
            raise ProtectionError("generated_integration_count_over_budget")
        if file.is_symlink():
            raise ProtectionError("generated_integration_alias_refused")
        if file.is_dir():
            pending.extend(file.iterdir())
            continue
        if not file.is_file():
            raise ProtectionError("generated_integration_not_regular")
        original = stamp(file)
        total += original.size
        if total > MAX_BACKUP_BYTES:
            raise ProtectionError("generated_integration_bytes_over_budget")
        content = file.read_bytes()
        version = re.search(rb"HERDR_INTEGRATION_ID=(" + kind.encode() + rb"(?:-[a-z0-9-]+)?)\r?\n[^\n]*HERDR_INTEGRATION_VERSION=([0-9]+)", content)
        artifacts.append({"file": file, "content": content, "stamp": original,
                          "integration_id": version[1].decode() if version else None,
                          "version": int(version[2]) if version else None})
    if not artifacts or not any(item["version"] is not None for item in artifacts):
        raise ProtectionError("generated_integration_version_missing")
    args, destination = [], None
    if kind == "claude":
        args = ["--settings", str(root / "settings.json"), "--setting-sources", "project,local"]
    elif kind == "codex":
        destination = Path(overlay["env"]["CODEX_HOME"])
    elif kind in ("pi", "omp"):
        destination = Path(overlay["env"]["PI_CODING_AGENT_DIR"])
    elif kind == "opencode":
        destination = Path(overlay["env"]["XDG_CONFIG_HOME"]) / "opencode"
    elif kind == "grok":
        destination = Path(overlay["env"]["GROK_HOME"])
    # Cursor's documented project hooks use the installer's absolute command
    # path. They are placed per scene by project_args, never in operator HOME.
    if destination:
        if not beneath(destination, runtime.probe):
            raise ProtectionError("integration_destination_outside_probe")
        copies = []
        for item in artifacts:
            target = destination / item["file"].relative_to(root)
            private_directory(target.parent)
            write_private(target, item["content"])
            copies.append({**item, "file": target, "stamp": stamp(target)})
        artifacts.extend(copies)
    overlay["settings"].extend(item["file"] for item in artifacts)
    copied_config = kind == "opencode" and any(
        item["source"].endswith(("/opencode.json", "/opencode.jsonc")) for item in overlay["copies"])
    return {"args": args, "artifacts": artifacts, "root": root,
            "sole_route": kind != "cursor" and not copied_config, "synthetic": False,
            "route_reason": "copied_configuration_may_load_external_plugins" if copied_config
                            else "global_hooks_not_excluded" if kind == "cursor" else "isolated_configuration"}


def project_args(plan, recipe, cwd):
    if recipe["kind"] == "cursor" and not plan["synthetic"]:
        destination = cwd / ".cursor/hooks.json"
        if not destination.exists():
            private_directory(destination.parent)
            write_private(destination, (plan["root"] / "hooks.json").read_bytes())
    return plan["args"]


def observe(runtime, pane, recipe, plan):
    rows = []
    changes = plan.setdefault("integrity_changes", {})
    for item in plan["artifacts"]:
        file = item["file"]
        if (not file.is_relative_to(runtime.probe) or not beneath(file, runtime.probe)
                or any(parent.is_symlink() for parent in file.parents
                       if parent.is_relative_to(runtime.probe))):
            raise ProtectionError("prepared_integration_path_refused", path=file)
        # Anchor the read through directories opened without following links;
        # checking a pathname alone cannot exclude an ancestor replacement.
        directory = inventory_directory(file.parent)
        try:
            current = stamp(Path(file.name), dir_fd=directory)
        finally:
            os.close(directory)
        if current is None:
            raise ProtectionError("prepared_integration_missing", path=file)
        if current != item["stamp"]:
            name = str(file.relative_to(runtime.probe))
            fields = ("digest", "size", "mode", "identity")
            changes[name] = {"name": name, "reason": "ordinary_private_artifact_changed",
                             "changed_fields": [field for field in fields
                                                if getattr(current, field) != getattr(item["stamp"], field)]}
        if item["version"] is not None:
            rows.append({"name": str(item["file"].relative_to(runtime.probe)),
                         "integration_id": item["integration_id"],
                         "version": item["version"], "sha256": hashlib.sha256(item["content"]).hexdigest()})
    current = runtime.agent(pane)
    session = current.get("agent_session") if current else None
    native = bool(session and session.get("source") == "herdr:" + recipe["kind"])
    versions = {row["version"] for row in rows}
    loaded = native and plan["sole_route"] and len(versions) == 1 and not changes
    return {"status": "integrity_unproven" if changes else
                      "loaded_version_observed" if loaded else "native_session_observed" if native else "not_observed",
            "loaded_version": sorted(versions) if loaded else None,
            "prepared_artifacts": rows, "native_source": session.get("source") if native else None,
            "evidence": "isolated_configuration_and_common_native_emitter_version" if loaded else "loaded_version_unproven",
            "route_reason": plan.get("route_reason", "synthetic"),
            "integrity": "unproven" if changes else "prepared_bytes_unchanged",
            "integrity_changes": list(changes.values()),
            "synthetic": plan["synthetic"]}
