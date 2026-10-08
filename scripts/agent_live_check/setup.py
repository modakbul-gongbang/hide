"""Supported per-scene configuration, inside the disposable checkout only."""

import json
from pathlib import Path
import sys
import time

from .authentication import require_no_login
from .processes import COMMAND_SECONDS, ProcessError
from .protection import beneath, private_directory, write_private
from .scenes import matches, owned_launch


def prepare_startup(runtime, pane, recipe, scene, cwd, workspace, seconds, evidence):
    """Select only an observed owned-folder trust option before other scenes."""
    data = recipe.get("startup_preparation")
    if scene == "startup" or runtime.fixture_bin or not data:
        return False
    record = {"scene": scene, "purpose": "owned_folder_trust_preparation", "outcome": "unknown", "key_attempts": []}
    samples = {}
    deadline = min(runtime.owner.deadline, time.monotonic() + seconds)

    def remaining():
        value = deadline - time.monotonic()
        if value <= 0:
            raise ProcessError("scene_timeout")
        return value

    def command_budget():
        return min(COMMAND_SECONDS, remaining())

    def frame(phase):
        screen = runtime.screen(pane, seconds=command_budget(), deadline=deadline)
        samples[phase] = {"phase": phase, "screen": screen, "agent": None}
        actual = runtime.agent(pane, seconds=command_budget(), deadline=deadline)
        samples[phase]["agent"] = actual
        remaining()
        require_no_login(screen)
        if (workspace not in runtime.workspaces or pane not in runtime.pane_credentials
                or cwd not in runtime.checkout_directories or not beneath(cwd, runtime.probe)
                or not owned_launch(scene, recipe, pane, actual, cwd=cwd, workspace=workspace)):
            raise ProcessError("startup_preparation_identity_not_owned")
        return screen, actual

    def selected(phase, option, other):
        screen, _ = frame(phase)
        return (matches(data["prompt"], screen, "") and matches(data[option], screen, "")
                and not matches(data[other], screen, ""))

    try:
        screen = runtime.screen(pane, seconds=command_budget(), deadline=deadline)
        samples["before"] = {"phase": "before", "screen": screen, "agent": None}
        remaining()
        require_no_login(screen)
        if not matches(data["prompt"], screen, ""):
            record["outcome"] = "not_required"
            return False
        if not selected("before", "default", "selected"):
            raise ProcessError("startup_preparation_default_not_observed")
        remaining()
        record["key_attempts"].append("down")
        runtime.command(["pane", "send-keys", pane, "down"], seconds=command_budget(), deadline=deadline)
        runtime.wait(lambda: selected("selected", "selected", "default"), remaining())
        # Re-read immediately before Enter; never confirm a stale selection.
        if not selected("confirmation", "selected", "default"):
            raise ProcessError("startup_preparation_selection_changed")
        remaining()
        record["key_attempts"].append("enter")
        runtime.command(["pane", "send-keys", pane, "enter"], seconds=command_budget(), deadline=deadline)

        def ready():
            screen, actual = frame("ready")
            remaining()
            return (actual["agent_status"] in ("idle", "done") and not actual.get("launch_pending")
                    and not matches(data["prompt"], screen, "")
                    and matches(recipe["scenes"]["rest"]["arrived"], screen, ""))

        runtime.wait(ready, remaining())
        record["outcome"] = "ready"
        return True
    except Exception as error:
        record["reason"] = str(error)
        raise
    finally:
        record["samples"] = list(samples.values())
        write_private(evidence, json.dumps(record, indent=2).encode())


def configure(runtime, launch: Path, recipe: dict, scene: str, cwd: Path) -> list[str]:
    command = sys.executable
    arguments = [str(runtime.checkout / "scripts/agent_live_check/mcp_fixture.py")]
    server = {"type": "stdio", "command": command, "args": arguments}
    data = recipe["mcp"]
    extra = []
    # Claude's strict file is present on every launch, but only the MCP scene
    # registers a tool. No account/global server is imported by this file.
    if recipe["kind"] == "claude":
        file = cwd / ("live-mcp-" + scene + ".json")
        write_private(file, json.dumps({"mcpServers": {"live_probe": server}
                                       if scene == "mcp_approval" else {}}).encode())
        extra = ["--strict-mcp-config", "--mcp-config", str(file)]
    elif scene == "mcp_approval":
        if data["format"] == "codex-overrides":
            for key, value in {"command": command, "args": arguments,
                               "default_tools_approval_mode": "prompt"}.items():
                extra.extend(["-c", "mcp_servers.live_probe." + key + "=" + json.dumps(value)])
        elif data["format"] == "grok-project-command":
            runtime.owner.run([str(launch), "mcp", "add", "--scope", "project",
                              "--leader-socket", str(runtime.short / "grok.sock"),
                              "live_probe", "--", command, *arguments],
                              env=runtime.native_env, cwd=cwd)
        else:
            file = cwd / data["path"]
            private_directory(file.parent)
            if data["format"] == "opencode":
                content = {"mcp": {"live_probe": {"type": "local", "command": [command, *arguments],
                                                  "enabled": True}},
                           "permission": {"live_probe_*": "ask"}}
            else:
                if recipe["kind"] == "pi":
                    server["exposure"] = "direct"
                content = {"mcpServers": {"live_probe": server}}
            write_private(file, json.dumps(content).encode())
    if scene == "startup" and recipe["kind"] == "pi":
        # Pi trusts project resources, not a bare .pi directory. An empty
        # project settings file elicits its real trust dialog without bypass.
        private_directory(cwd / ".pi")
        write_private(cwd / ".pi/settings.json", b"{}\n")
    return extra
