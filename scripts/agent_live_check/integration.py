"""Use the pinned installer's assets, with only disposable config routes."""

import hashlib
from pathlib import Path
import re

from .protection import MAX_BACKUP_BYTES, ProtectionError, beneath, private_directory, stamp, write_private


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
    env = {**runtime.env, "HOME": str(home), "XDG_STATE_HOME": str(home / "state")}
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
        version = re.search(rb"HERDR_INTEGRATION_ID=" + kind.encode() + rb"\r?\n[^\n]*HERDR_INTEGRATION_VERSION=([0-9]+)", content)
        artifacts.append({"file": file, "content": content, "stamp": original,
                          "version": int(version[1]) if version else None})
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
    return {"args": args, "artifacts": artifacts, "root": root,
            "sole_route": kind != "cursor", "synthetic": False}


def project_args(plan, recipe, cwd):
    if recipe["kind"] == "cursor" and not plan["synthetic"]:
        destination = cwd / ".cursor/hooks.json"
        if not destination.exists():
            private_directory(destination.parent)
            write_private(destination, (plan["root"] / "hooks.json").read_bytes())
    return plan["args"]


def observe(runtime, pane, recipe, plan):
    rows = []
    for item in plan["artifacts"]:
        if stamp(item["file"]) != item["stamp"]:
            raise ProtectionError("prepared_integration_changed_during_probe")
        if item["version"] is not None:
            rows.append({"name": str(item["file"].relative_to(runtime.probe)),
                         "version": item["version"], "sha256": hashlib.sha256(item["content"]).hexdigest()})
    current = runtime.agent(pane)
    session = current.get("agent_session") if current else None
    native = bool(session and session.get("source") == "herdr:" + recipe["kind"])
    loaded = native and plan["sole_route"] and bool(rows)
    return {"status": "loaded_version_observed" if loaded else "native_session_observed" if native else "not_observed",
            "loaded_version": sorted({row["version"] for row in rows}) if loaded else None,
            "prepared_artifacts": rows, "native_source": session.get("source") if native else None,
            "evidence": "isolated_configuration_and_native_session_report" if loaded else "loaded_version_unproven",
            "synthetic": plan["synthetic"]}
