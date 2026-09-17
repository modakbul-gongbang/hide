#!/usr/bin/env python3
"""Stand-in for the installed `claude` CLI used by hide-ai tests.

Answers `auth status --json` and prints one print-mode result frame. The
frames are the shapes measured from claude 2.1.267 and recorded in
`agents/runs/hide-ai-claude-backend/claude-print-mode-measurements.md`; the
real CLI is never called from a test.

FAKE_MODE selects the frame: ok (default), no_structured_output,
null_structured_output, api_401, api_403, api_429, api_500, api_400,
structured_output_retries, context_limit, no_result_frame, init_frame_only,
slow, no_account, auth_broken, auth_without_field.
FAKE_ARGS_FILE records the argument vector, FAKE_STDIN_FILE the prompt body.

A `/usage` run receives a whitelisted environment, so it reads no FAKE_*
variable. It takes its mode from a `usage-mode` file in its working directory
(text, cost, slow, exit, is_error, not_json) and writes what it received -
argv to `usage-args.json`, the environment's key set to `usage-env.json` -
into that same directory.
"""
import json
import os
import sys
import time

MODE = os.environ.get("FAKE_MODE", "ok")
ARGS = sys.argv[1:]

if path := os.environ.get("FAKE_ARGS_FILE"):
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(ARGS, handle)


def emit(obj, code=0):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()
    sys.exit(code)


USAGE_TEXT = (
    "You are currently using your subscription to power your Claude Code usage\n\n"
    "Current session: 4% used · resets Sep 17 at 9pm (Asia/Seoul)\n"
    "Current week (all models): 1% used · resets Sep 24 at 1pm (Asia/Seoul)\n"
    "Current week (Fable): 0% used · resets Sep 24 at 1pm (Asia/Seoul)\n"
)
COST_TEXT = "Total cost:            $0.0000\nTotal duration (API):  0s\n"

if ARGS[:2] == ["-p", "/usage"]:
    with open("usage-args.json", "w", encoding="utf-8") as handle:
        json.dump(ARGS, handle)
    with open("usage-env.json", "w", encoding="utf-8") as handle:
        json.dump(sorted(os.environ), handle)
    try:
        with open("usage-mode", encoding="utf-8") as handle:
            usage_mode = handle.read().strip()
    except FileNotFoundError:
        usage_mode = "text"
    local = {"type": "result", "subtype": "success", "is_error": False,
             "duration_api_ms": 0, "num_turns": 0, "total_cost_usd": 0,
             "local_command": "usage", "session_id": "s-usage"}
    if usage_mode == "slow":
        time.sleep(60)
    if usage_mode == "exit":
        sys.stdout.write("fatal: something\n")
        sys.exit(2)
    if usage_mode == "not_json":
        sys.stdout.write("Loading...\n")
        sys.exit(0)
    if usage_mode == "is_error":
        emit({**local, "is_error": True, "result": "usage failed"})
    emit({**local, "result": COST_TEXT if usage_mode == "cost" else USAGE_TEXT})

if ARGS[:2] == ["auth", "status"]:
    if MODE == "auth_broken":
        sys.stdout.write("not json at all\n")
        sys.exit(3)
    if MODE == "auth_without_field":
        emit({"authMethod": "none"})
    emit({"loggedIn": MODE != "no_account", "authMethod": "claude.ai"})

# Print mode. The prompt body arrives on stdin, never in argv.
prompt = sys.stdin.read()
if path := os.environ.get("FAKE_STDIN_FILE"):
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(prompt)

if MODE == "slow":
    time.sleep(60)

if MODE == "no_result_frame":
    sys.stdout.write("Killed before the turn produced a frame\n")
    sys.exit(1)

if MODE == "init_frame_only":
    emit({"type": "system", "subtype": "init", "session_id": "s-1"}, code=1)

USAGE = {"input_tokens": 1188, "output_tokens": 468}
BASE = {
    "type": "result",
    "session_id": "s-1",
    "num_turns": 3,
    "duration_ms": 5100,
    "total_cost_usd": 0.0045,
    "usage": USAGE,
    "permission_denials": [],
}

API_ERRORS = {
    "api_401": (401, "Failed to authenticate. API Error: 401"),
    "api_403": (403, "Failed to authenticate. API Error: 403"),
    "api_429": (429, "You've hit your session limit · resets 6:42pm"),
    "api_500": (500, "API Error: 500 This is a server-side issue"),
    "api_400": (400, "API Error: 400 bad request"),
}

if MODE in API_ERRORS:
    status, message = API_ERRORS[MODE]
    emit({**BASE, "subtype": "success", "is_error": True,
          "terminal_reason": "api_error", "api_error_status": status,
          "result": message, "usage": {"input_tokens": 0, "output_tokens": 0}}, code=1)

if MODE == "structured_output_retries":
    emit({**BASE, "subtype": "error_max_structured_output_retries", "is_error": True,
          "terminal_reason": "structured_output_retry_exhausted", "errors": []}, code=1)

if MODE == "context_limit":
    emit({**BASE, "subtype": "success", "is_error": True,
          "terminal_reason": "prompt_too_long", "api_error_status": None,
          "result": "Context limit reached"}, code=1)

ANSWER = json.loads(os.environ.get(
    "FAKE_REPLY", '{"summary":"fixture","attention":"none"}'))
FRAME = {**BASE, "subtype": "success", "is_error": False,
         "api_error_status": None, "terminal_reason": "completed",
         "result": json.dumps(ANSWER)}

if MODE == "no_structured_output":
    # The answer text is there and schema-shaped; the schema-bound field is
    # not. Only the field counts.
    emit(FRAME)

if MODE == "null_structured_output":
    emit({**FRAME, "structured_output": None})

emit({**FRAME, "structured_output": ANSWER})
