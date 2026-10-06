#!/usr/bin/env python3
"""Stand-in for `codex app-server` over stdio, used by the trust tests.

It reads the hooks the way Codex lists them from `$CODEX_HOME/hooks.json`,
keeps trust in `$CODEX_HOME/fake-trust.json` (a table per key, like
`hooks.state` in config.toml, so `enabled` is a field the upsert must keep),
and records what happened beside them: `fake-calls.log` (one method per line)
and `fake-pid` (its own pid). `$CODEX_HOME/fake-mode` selects the behaviour:

  ok            (default) answers everything
  unsupported   hooks/list is an unknown method, answered the way codex-cli
                0.160.0 answers one (-32600, "unknown variant")
  invalid       hooks/list is refused for its parameters (-32600, not unknown)
  hang          never answers hooks/list, and keeps a child running
  noisy         sends a notification and a request of its own (with the id
                of Hide's next request) before it answers hooks/list
  flood         makes more requests of the client than any answer needs
  big_line      prints one line longer than any caller reads
  errors        lists no hooks and reports hooks.json as unusable
  escape        answers normally and leaves a process outside its tree
                holding its output open
  exit          ends right after initialize
  ignore_write  accepts config/batchWrite and stores nothing
  refuse_write  answers config/batchWrite with an error
  chatty        prints far more than any caller reads
  project       also lists an identical project-layer hook
"""
import hashlib
import json
import os
import subprocess
import sys

HOME = os.environ["CODEX_HOME"]


def path(name):
    return os.path.join(HOME, name)


def mode():
    try:
        return open(path("fake-mode")).read().strip()
    except OSError:
        return "ok"


def load_trust():
    try:
        return json.load(open(path("fake-trust.json")))
    except OSError:
        return {}


def save_trust(tables):
    with open(path("fake-trust.json"), "w") as f:
        json.dump(tables, f)


SNAKE = {
    "SessionStart": "session_start",
    "UserPromptSubmit": "user_prompt_submit",
    "SubagentStart": "subagent_start",
    "SubagentStop": "subagent_stop",
    "Stop": "stop",
    "PreToolUse": "pre_tool_use",
}


def wire(event):
    return event[0].lower() + event[1:]


def listed():
    hooks_json = path("hooks.json")
    try:
        document = json.load(open(hooks_json))
    except OSError:
        return []
    trust = load_trust()
    out = []
    order = 0
    for event, groups in document.get("hooks", {}).items():
        for g, group in enumerate(groups):
            for h, hook in enumerate(group.get("hooks", [])):
                key = f"{hooks_json}:{SNAKE.get(event, event)}:{g}:{h}"
                current = "sha256:" + hashlib.sha256(
                    json.dumps([event, group.get("matcher"), hook.get("command")]).encode()
                ).hexdigest()
                held = trust.get(key, {}).get("trusted_hash")
                status = "untrusted" if held is None else ("trusted" if held == current else "modified")
                out.append({
                    "key": key, "eventName": wire(event), "handlerType": "command",
                    "command": hook.get("command"), "async": False,
                    "matcher": group.get("matcher"), "timeoutSec": hook.get("timeout", 600),
                    "statusMessage": None, "additionalContextLimit": None,
                    "sourcePath": hooks_json, "source": "user", "pluginId": None,
                    "displayOrder": order, "enabled": trust.get(key, {}).get("enabled", True),
                    "isManaged": False, "currentHash": current, "trustStatus": status,
                })
                order += 1
    return out


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def reply(rid, result):
    send({"jsonrpc": "2.0", "id": rid, "result": result})


def error(rid, code, message):
    send({"jsonrpc": "2.0", "id": rid, "error": {"code": code, "message": message}})


if sys.argv[1:] != ["app-server"]:
    sys.stderr.write("unexpected arguments: %r\n" % sys.argv[1:])
    sys.exit(2)

with open(path("fake-pid"), "w") as f:
    f.write(str(os.getpid()))
with open(path("fake-cwd"), "w") as f:
    f.write(os.getcwd())
MODE = mode()

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    message = json.loads(raw)
    method = message.get("method")
    rid = message.get("id")
    params = message.get("params") or {}
    with open(path("fake-calls.log"), "a") as log:
        log.write(f"{method if method else 'client-response'}\n")
    if method == "initialize":
        reply(rid, {"userAgent": "fake", "codexHome": HOME})
        if MODE == "exit":
            sys.exit(0)
    elif method == "initialized":
        pass
    elif method == "hooks/list":
        if MODE == "unsupported":
            error(rid, -32600, "Invalid request: unknown variant `hooks/list`, expected one of `initialize`, `thread/start`")
        elif MODE == "invalid":
            error(rid, -32600, "Invalid request: missing field `cwds`")
        elif MODE == "hang":
            child = subprocess.Popen(["sleep", "600"])
            with open(path("fake-child-pid"), "w") as f:
                f.write(str(child.pid))
            while True:
                sys.stdin.readline()
        elif MODE == "flood":
            for n in range(50):
                send({"jsonrpc": "2.0", "id": 1000 + n, "method": "item/tool/requestUserInput", "params": {}})
            while True:
                sys.stdin.readline()
        elif MODE == "big_line":
            sys.stdout.write("y" * (2 * 1024 * 1024) + "\n")
            sys.stdout.flush()
        elif MODE == "errors":
            reply(rid, {"data": [{"cwd": os.getcwd(), "hooks": [], "warnings": [],
                                  "errors": [{"path": path("hooks.json"), "message": "invalid JSON"}]}]})
        elif MODE == "chatty":
            for _ in range(80):
                sys.stdout.write("x" * 65536 + "\n")
            sys.stdout.flush()
        else:
            if MODE == "noisy":
                send({"jsonrpc": "2.0", "method": "remoteControl/status/changed", "params": {"status": "disabled"}})
                send({"jsonrpc": "2.0", "id": rid, "method": "item/tool/requestUserInput", "params": {}})
            if MODE == "escape":
                if os.fork() == 0:
                    os.setsid()
                    if os.fork() == 0:
                        with open(path("fake-escaped-pid"), "w") as f:
                            f.write(str(os.getpid()))
                        import time
                        time.sleep(60)
                    os._exit(0)
                os.wait()
                while not os.path.exists(path("fake-escaped-pid")):
                    pass
            hooks = listed()
            if MODE == "project":
                hooks += [dict(h, source="project", key=h["key"] + ":p") for h in hooks]
            reply(rid, {"data": [{"cwd": os.getcwd(), "hooks": hooks, "warnings": [], "errors": []}]})
    elif method == "config/batchWrite":
        if MODE == "refuse_write":
            error(rid, -32600, "config is read-only")
        else:
            if MODE != "ignore_write":
                tables = load_trust()
                for edit in params["edits"]:
                    assert edit["keyPath"] == "hooks.state" and edit["mergeStrategy"] == "upsert"
                    for key, table in edit["value"].items():
                        tables.setdefault(key, {}).update(table)
                save_trust(tables)
            reply(rid, {"status": "ok", "version": "v", "filePath": path("config.toml")})
    elif rid is not None:
        error(rid, -32601, "Method not found")
