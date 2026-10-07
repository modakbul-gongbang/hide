"""Supported per-scene configuration, inside the disposable checkout only."""

import json
from pathlib import Path
import sys

from .protection import private_directory, write_private


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
