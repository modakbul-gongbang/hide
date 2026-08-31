#!/usr/bin/env python3
"""Serve scripted herdr session snapshots over a Unix socket.

Verification-only. The pet's poses, badge row, and attention queue are driven
by whatever the herdr server reports, and driving a real agent into an unseen
error on demand is not something the product can do. This speaks the same
`session.snapshot` contract (protocol 21) the real server does, so the app
under test exercises its ordinary polling path end to end, including the
1s poll clock the roam and sleep transitions depend on.

The scenario is re-read from disk on every request, so a scenario file edited
while the app is running changes what the next poll sees. Deleting the socket
(or stopping this process) is how the "herdr went away" case is produced.

    ./pet_scenario_server.py --socket /tmp/pet.sock --scenario scenario.json

A scenario file is a JSON object: {"agents": [{"pane_id", "state", "summary",
"ambient"?}]}, where state is one of question, approval, error, working, done,
idle, or a raw token name.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import sys
import threading

PROTOCOL = 21

STATE_TOKENS = {
    "question": "status_question_new",
    "approval": "status_approval_new",
    "error": "status_error_new",
    "acknowledged": "status_question",
    "working": "status_working",
    "done": "status_done_new",
    "idle": "status_idle",
}

STATE_SYMBOLS = {
    "status_question_new": "?",
    "status_approval_new": "!",
    "status_error_new": "×",
    "status_question": "?",
    "status_working": "●",
    "status_done_new": "●",
    "status_idle": "○",
}

SORT_RANKS = {
    "status_error_new": "00",
    "status_question_new": "01",
    "status_approval_new": "02",
    "status_done_new": "04",
    "status_working": "05",
}


def build_snapshot(scenario: dict) -> dict:
    agents = []
    for index, entry in enumerate(scenario.get("agents", [])):
        pane_id = entry.get("pane_id") or f"fixture:p{index}"
        token = STATE_TOKENS.get(entry.get("state", "idle"), entry.get("state", "status_idle"))
        tokens = {
            token: STATE_SYMBOLS.get(token, "○"),
            "sort_rank": entry.get("sort_rank", SORT_RANKS.get(token, "10")),
            # 13 digits; ordering only matters within one snapshot.
            "activity": entry.get("activity", f"{1788000000000 + index:013d}"),
            "summary": entry.get("summary", "Scenario agent"),
            "elapsed": entry.get("elapsed", "1m"),
        }
        agent = {
            "pane_id": pane_id,
            "workspace_id": entry.get("workspace_id", "wF"),
            "agent": entry.get("agent", "codex"),
            "agent_status": entry.get("agent_status", "unknown"),
            "cwd": entry.get("cwd", "/tmp/herdr-ide-verify-pet"),
            "tokens": tokens,
        }
        if "ambient" in entry:
            agent["ambient"] = entry["ambient"]
        agents.append(agent)

    first_pane = agents[0]["pane_id"] if agents else "fixture:p0"
    area = {"x": 0, "y": 0, "width": 120, "height": 40}
    return {
        "protocol": PROTOCOL,
        "focused_pane_id": first_pane,
        "workspaces": [{"workspace_id": "wF", "label": "Pet Scenario"}],
        "layouts": [
            {
                "workspace_id": "wF",
                "tab_id": "tF",
                "zoomed": False,
                "area": area,
                "focused_pane_id": first_pane,
                "panes": [{"pane_id": first_pane, "rect": area}],
                "splits": [],
            }
        ],
        "agents": agents,
    }


def serve(socket_path: str, scenario_path: str) -> None:
    if os.path.exists(socket_path):
        os.unlink(socket_path)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(socket_path)
    server.listen(16)
    print(f"pet scenario server on {socket_path} <- {scenario_path}", flush=True)

    def handle(connection: socket.socket) -> None:
        with connection:
            data = b""
            while not data.endswith(b"\n"):
                chunk = connection.recv(65536)
                if not chunk:
                    return
                data += chunk
            try:
                request = json.loads(data.decode())
            except json.JSONDecodeError:
                return
            request_id = request.get("id")
            if request.get("method") != "session.snapshot":
                connection.sendall(
                    (
                        json.dumps(
                            {
                                "id": request_id,
                                "error": {
                                    "code": "unsupported",
                                    "message": f"scenario server serves session.snapshot only, got {request.get('method')}",
                                },
                            }
                        )
                        + "\n"
                    ).encode()
                )
                return
            # Re-read every request so editing the scenario changes the very
            # next poll.
            try:
                with open(scenario_path, encoding="utf-8") as handle_file:
                    scenario = json.load(handle_file)
            except (OSError, json.JSONDecodeError) as error:
                connection.sendall(
                    (
                        json.dumps(
                            {
                                "id": request_id,
                                "error": {"code": "scenario", "message": str(error)},
                            }
                        )
                        + "\n"
                    ).encode()
                )
                return
            response = {"id": request_id, "result": {"snapshot": build_snapshot(scenario)}}
            connection.sendall((json.dumps(response) + "\n").encode())

    try:
        while True:
            connection, _ = server.accept()
            threading.Thread(target=handle, args=(connection,), daemon=True).start()
    except KeyboardInterrupt:
        pass
    finally:
        server.close()
        if os.path.exists(socket_path):
            os.unlink(socket_path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--socket", required=True)
    parser.add_argument("--scenario", required=True)
    arguments = parser.parse_args()
    serve(arguments.socket, arguments.scenario)
    return 0


if __name__ == "__main__":
    sys.exit(main())
