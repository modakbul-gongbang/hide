#!/usr/bin/env python3
"""Stand-in for `codex app-server --listen stdio://` used by hide-ai tests.

Speaks the subset of the JSON-RPC protocol the backend uses. Behaviour is
selected with FAKE_MODE: ok (default), slow, usage_limit, garbage, exit,
no_account, no_model. FAKE_ARGS_FILE records the argument vector when set.
"""
import json
import os
import sys
import time

MODE = os.environ.get("FAKE_MODE", "ok")
REPLY = os.environ.get("FAKE_REPLY", '{"summary":"fixture","attention":"none"}')
if path := os.environ.get("FAKE_ARGS_FILE"):
    with open(path, "w", encoding="utf-8") as f:
        json.dump(sys.argv[1:], f)


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def result(rid, value):
    send({"jsonrpc": "2.0", "id": rid, "result": value})


def notify(method, params):
    send({"jsonrpc": "2.0", "method": method, "params": params})


active_turn = None
for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    rid = msg.get("id")
    method = msg.get("method")
    params = msg.get("params") or {}
    if method == "initialize":
        result(rid, {"userAgent": "fake"})
    elif method == "initialized":
        pass
    elif method == "account/read":
        account = None if MODE == "no_account" else {"type": "chatgpt", "planType": "pro"}
        result(rid, {"account": account, "requiresOpenaiAuth": True})
    elif method == "model/list":
        models = [] if MODE == "no_model" else [{"id": "gpt-5.6-luna", "model": "gpt-5.6-luna"}]
        result(rid, {"data": models, "nextCursor": None})
    elif method == "account/rateLimits/read":
        result(rid, {"rateLimits": {"primary": {"usedPercent": 100, "resetsAt": int(time.time()) + 120}}})
    elif method == "thread/start":
        assert params.get("ephemeral") is True and params.get("baseInstructions")
        result(rid, {"thread": {"id": "thread-1"}, "model": params.get("model"), "instructionSources": []})
    elif method == "turn/start":
        if MODE == "exit":
            sys.exit(3)
        assert params.get("clientUserMessageId") and params.get("outputSchema")
        active_turn = "turn-1"
        result(rid, {"turn": {"id": active_turn, "status": "inProgress", "items": []}})
        notify("turn/started", {"threadId": "thread-1", "turn": {"id": active_turn, "status": "inProgress", "items": []}})
        if MODE == "slow":
            continue
        if MODE == "usage_limit":
            notify("turn/completed", {"threadId": "thread-1", "turn": {"id": active_turn, "status": "failed", "items": [],
                    "error": {"message": "limit", "codexErrorInfo": "usageLimitExceeded"}}})
            continue
        text = "not json at all" if MODE == "garbage" else REPLY
        notify("item/completed", {"threadId": "thread-1", "turnId": active_turn,
                                  "item": {"type": "agentMessage", "id": "msg-1", "text": text, "phase": "final_answer"}})
        notify("thread/tokenUsage/updated", {"threadId": "thread-1", "turnId": active_turn,
               "tokenUsage": {"last": {"inputTokens": 1234, "outputTokens": 56, "cachedInputTokens": 0, "reasoningOutputTokens": 0, "totalTokens": 1290},
                              "total": {"inputTokens": 1234, "outputTokens": 56, "cachedInputTokens": 0, "reasoningOutputTokens": 0, "totalTokens": 1290}}})
        notify("turn/completed", {"threadId": "thread-1", "turn": {"id": active_turn, "status": "completed", "items": []}})
    elif method == "turn/interrupt":
        result(rid, {})
        notify("turn/completed", {"threadId": params.get("threadId"), "turn": {"id": params.get("turnId"), "status": "interrupted", "items": []}})
        active_turn = None
    elif rid is not None:
        send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "unknown"}})
