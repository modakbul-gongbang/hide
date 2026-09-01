#!/usr/bin/env python3
"""Serve a scripted Herdr session over a Unix socket.

Verification-only. The pet's poses, badge row, and attention queue are driven
by whatever the herdr server reports, and driving a real agent into an unseen
error on demand is not something the product can do. This speaks the same
`session.snapshot`, `events.subscribe`, and `agent.list` contracts (protocol
21) the real server does. The app under test therefore exercises its ordinary
event-sync path end to end, including the 1s agent refresh clock that drives
roam and sleep transitions.

The scenario is re-read from disk on every snapshot and agent-list request, so
a scenario file edited while the app is running changes what the next refresh
sees. Deleting the socket (or stopping this process) is how the "herdr went
away" case is produced.

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
VERSION = "0.8.2"
HOST = {"host_id": "pet-scenario", "session_id": "fixture"}
WORKSPACE_ID = "wF"
TAB_ID = "wF:t1"

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


def build_agents(scenario: dict) -> list[dict]:
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
            "terminal_id": f"fixture-term-{index}",
            "pane_id": pane_id,
            "workspace_id": entry.get("workspace_id", WORKSPACE_ID),
            "tab_id": entry.get("tab_id", TAB_ID),
            "focused": index == 0,
            "revision": 0,
            "agent": entry.get("agent", "codex"),
            "agent_status": entry.get("agent_status", "unknown"),
            "cwd": entry.get("cwd", "/tmp/herdr-ide-verify-pet"),
            "tokens": tokens,
        }
        if "ambient" in entry:
            agent["ambient"] = entry["ambient"]
        agents.append(agent)
    return agents


def build_snapshot(scenario: dict) -> dict:
    agents = build_agents(scenario)

    first_pane = agents[0]["pane_id"] if agents else "fixture:p0"
    area = {"x": 0, "y": 0, "width": 120, "height": 40}
    return {
        "version": VERSION,
        "protocol": PROTOCOL,
        "host": HOST,
        "event_sequence": 0,
        "focused_pane_id": first_pane,
        "focused_tab_id": TAB_ID,
        "focused_workspace_id": WORKSPACE_ID,
        "workspaces": [
            {
                "workspace_id": WORKSPACE_ID,
                "number": 1,
                "label": "Pet Scenario",
                "focused": True,
                "pane_count": 1,
                "tab_count": 1,
                "active_tab_id": TAB_ID,
                "agent_status": "idle",
            }
        ],
        "tabs": [
            {
                "workspace_id": WORKSPACE_ID,
                "tab_id": TAB_ID,
                "number": 1,
                "label": "1",
                "focused": True,
                "pane_count": 1,
                "agent_status": "idle",
            }
        ],
        "panes": [
            {
                "workspace_id": WORKSPACE_ID,
                "tab_id": TAB_ID,
                "pane_id": first_pane,
                "surface": {
                    "kind": "terminal",
                    "attach": {
                        "host": HOST,
                        "transport": "herdr_client",
                        "protocol": PROTOCOL,
                        "terminal_id": "fixture-term-0",
                    },
                },
                "focused": True,
                "agent_status": "idle",
                "revision": 0,
                "cwd": "/tmp/herdr-ide-verify-pet",
            }
        ],
        "layouts": [
            {
                "workspace_id": WORKSPACE_ID,
                "tab_id": TAB_ID,
                "zoomed": False,
                "area": area,
                "focused_pane_id": first_pane,
                "panes": [{"pane_id": first_pane, "rect": area}],
                "splits": [],
            }
        ],
        "agents": agents,
        "lineage": [],
    }


def read_scenario(scenario_path: str) -> dict:
    with open(scenario_path, encoding="utf-8") as handle_file:
        return json.load(handle_file)


def send_result(connection: socket.socket, request_id: object, result: dict) -> None:
    response = {"id": request_id, "result": result}
    connection.sendall((json.dumps(response) + "\n").encode())


def send_error(connection: socket.socket, request_id: object, code: str, message: str) -> None:
    response = {"id": request_id, "error": {"code": code, "message": message}}
    connection.sendall((json.dumps(response) + "\n").encode())


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
            method = request.get("method")
            if method == "events.subscribe":
                send_result(
                    connection,
                    request_id,
                    {
                        "type": "subscription_started",
                        "host": HOST,
                        "sequence": 0,
                        "oldest_available_sequence": 1,
                    },
                )
                try:
                    while connection.recv(1):
                        pass
                except OSError:
                    pass
                return
            if method not in {"session.snapshot", "agent.list"}:
                send_error(
                    connection,
                    request_id,
                    "unsupported",
                    f"scenario server does not serve {method}",
                )
                return
            try:
                scenario = read_scenario(scenario_path)
            except (OSError, json.JSONDecodeError) as error:
                send_error(connection, request_id, "scenario", str(error))
                return
            if method == "session.snapshot":
                send_result(
                    connection,
                    request_id,
                    {"type": "session_snapshot", "snapshot": build_snapshot(scenario)},
                )
            else:
                send_result(
                    connection,
                    request_id,
                    {"type": "agent_list", "agents": build_agents(scenario)},
                )

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
