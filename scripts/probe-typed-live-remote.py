"""Capture pinned Herdr responses using only an owned server and local fixture process."""

import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    os.chdir(Path(__file__).resolve().parent.parent)
    binary = subprocess.check_output(
        ["zsh", "scripts/fetch-herdr-runtime.sh"], text=True
    ).strip()
    with tempfile.TemporaryDirectory(prefix="ht-", dir="/tmp") as directory:
        root = Path(directory)
        home = root / "home"
        home.mkdir()
        bindir = root / "bin"
        bindir.mkdir()
        config = root / "config.toml"
        config.write_text(
            'onboarding = false\n[terminal]\ndefault_shell = "/bin/bash"\n'
            'shell_mode = "non_login"\n[update]\nversion_check = false\n'
            'manifest_check = false\n'
        )
        # A local readiness fixture, never a provider call or implementation worker.
        fixture = bindir / "codex"
        fixture.write_text('''#!/usr/bin/python3
import os, socket, json, time
print("OpenAI Codex\\n> ", flush=True)
with socket.socket(socket.AF_UNIX) as stream:
    stream.connect(os.environ["HERDR_SOCKET_PATH"])
    stream.sendall((json.dumps({"id": "fixture", "method": "pane.report_agent",
        "params": {"pane_id": os.environ["HERDR_PANE_ID"], "source": "herdr:codex",
                   "agent": "codex", "state": "idle"}}) + "\\n").encode())
    stream.recv(65536)
while True:
    time.sleep(1)
''')
        fixture.chmod(0o755)
        environment = {
            "HOME": str(home),
            "PATH": str(bindir) + ":/usr/bin:/bin",
            "SHELL": "/bin/bash",
            "HERDR_SOCKET_PATH": str(root / "h.sock"),
            "HERDR_CONFIG_PATH": str(config),
            "USER": os.environ.get("USER", "fixture"),
        }

        def cli(*arguments):
            result = subprocess.run(
                [binary, *arguments], env=environment, capture_output=True,
                text=True, timeout=45, check=True,
            )
            return json.loads(result.stdout)

        def capture(name, response):
            (output / (name + ".json")).write_text(json.dumps(response, indent=2))

        def request(method, params):
            with socket.socket(socket.AF_UNIX) as stream:
                stream.settimeout(10)
                stream.connect(environment["HERDR_SOCKET_PATH"])
                stream.sendall((json.dumps({
                    "id": method, "method": method, "params": params,
                }) + "\n").encode())
                with stream.makefile() as reader:
                    reply = json.loads(reader.readline())
            if "error" in reply:
                raise RuntimeError(reply)
            capture(method, reply)
            return reply["result"]

        with (output / "probe-server.log").open("w") as log:
            server = subprocess.Popen(
                [binary, "server"], env=environment, stdout=log, stderr=log,
            )
            try:
                for _ in range(100):
                    if (root / "h.sock").exists():
                        break
                    if server.poll() is not None:
                        raise RuntimeError("isolated server exited before creating its socket")
                    time.sleep(0.05)
                else:
                    raise TimeoutError("isolated server did not create its socket")
                workspace = cli("workspace", "create", "--cwd", str(home), "--no-focus")
                capture("workspace.create", workspace)
                pane = workspace["result"]["root_pane"]["pane_id"]
                tab = request("tab.create", {
                    "workspace_id": workspace["result"]["workspace"]["workspace_id"],
                    "cwd": str(home), "focus": False,
                })
                request("tab.move", {"tab_id": tab["tab"]["tab_id"], "insert_index": 0})
                request("pane.split", {"target_pane_id": pane, "direction": "right", "focus": False})
                request("pane.layout", {"pane_id": pane})
                request("pane.read", {"pane_id": pane, "source": "visible", "format": "text"})
                request("session.snapshot", {})
                created = cli(
                    "agent", "new", "fixture-child", "--kind", "codex", "--pane", pane,
                    "--idempotency-key", "typed-probe", "--cwd", str(home),
                    "--no-focus", "--timeout", "10000",
                )
                capture("agent-new", created)
                print("captured all seven socket methods and real agent new output")
            finally:
                try:
                    subprocess.run(
                        [binary, "server", "stop"], env=environment,
                        capture_output=True, text=True, timeout=10, check=True,
                    )
                    server.wait(timeout=10)
                    print("owned server stopped: exit 0")
                finally:
                    # Only the process created above can be terminated on cleanup failure.
                    if server.poll() is None:
                        server.terminate()
                        server.wait(timeout=10)


if __name__ == "__main__":
    main()
