"""Letters while the agent works, measured on the private runtime with this build's hooks.

One probe per agent: the agent runs a multi-call shell job in an owned pane with
Hide's hooks of this build installed into the agent's disposable configuration,
a letter is sent while Herdr reports it working, and the run records when the
hook handed the letter over and when the agent's own text repeated its marker.
A hook wrapper keeps each hook call's input and answer under the probe, so the
session id a hook names and the moment it answered are read from files rather
than guessed. The per-tool cost is measured from the controller with no letter
waiting, as docs/PERFORMANCE_TESTING.md, Letters at a tool event, asks.
"""

import argparse
import datetime
import json
import os
from pathlib import Path
import secrets
import shlex
import shutil
import signal
import statistics
import sys
import time

from .cli import default_herdr, operator_routing, wrapper as native_wrapper
from .contracts import recipes, source_contract
from .conversation import messages
from .integration import prepare as prepare_integration, project_args
from .overlay import prepare as prepare_overlay
from .processes import COMMAND_SECONDS, OwnedProcesses, ProcessError
from .protection import ProtectionError, beneath, private_directory, write_private
from .runtime import Runtime, clean_env
from .sandbox import WriteSandbox
from .scenes import transcript
from .setup import configure, prepare_startup

PLACEHOLDER = "/kit path/it's/hide-agent-hooks"
CLAUDE_EVENTS = ("SessionStart", "UserPromptSubmit", "SubagentStart", "SubagentStop", "Stop", "PreToolUse")
CLAUDE_MATCHER = "Bash|AskUserQuestion|ExitPlanMode|Edit|Write|MultiEdit|NotebookEdit|Read"
CODEX_MATCHER = "Bash|request_user_input|request_user_input_async|apply_patch|Edit|Write"
GROK_MATCHER = "Bash|ask_user_question|exit_plan_mode|Read|Edit|Write|read_file|search_replace|write"
CURSOR_MATCHER = "Shell|Read|Write|Delete"
# Approval-free flags per agent, so a tool call never waits on a menu the
# doorbell would refuse to type over (PRD D-09). Each is the agent's own
# documented switch, read from its --help on 2026-10-17.
FREE_ARGS = {"claude": ["--dangerously-skip-permissions"],
             "codex": ["--ask-for-approval", "never", "--dangerously-bypass-hook-trust"],
             "grok": ["--always-approve"], "cursor": ["--force", "--trust"],
             "omp": ["--approval-mode=yolo"], "pi": [], "opencode": []}


def parser():
    value = argparse.ArgumentParser(description="Measure letters delivered while an agent works.")
    value.add_argument("--agents", help="Comma-separated adapter IDs; default is all current adapters")
    value.add_argument("--model", action="append", default=[], metavar="ID=MODEL")
    value.add_argument("--run-dir", type=Path)
    value.add_argument("--herdr-bin", type=Path)
    value.add_argument("--runs", type=int, default=1, help="--now letters per agent")
    value.add_argument("--patience", action="store_true",
                       help="Also one letter without --now per agent, with the agent kept working past the wait")
    value.add_argument("--calls", type=int, default=12, help="shell calls the job makes")
    value.add_argument("--call-seconds", type=int, default=4)
    value.add_argument("--cost-samples", type=int, default=40)
    value.add_argument("--observe-seconds", type=float, default=150)
    return value


def replace_private(path, data):
    """Write a private file the integration step may already have made."""
    path.unlink(missing_ok=True)
    write_private(path, data)


def helper_wrapper(runtime, calls):
    """A shell wrapper at a plain path that keeps each hook call's input, answer and arguments."""
    target = runtime.bin / "hide-agent-hooks"
    real = runtime.checkout / "target/debug/hide-agent-hooks"
    if not real.is_file():
        raise ProtectionError("build_this_worktrees_hide_agent_hooks_first")
    text = ("#!/bin/sh\n"
            f"d={shlex.quote(str(calls))}\n"
            'n="$(/bin/date +%s)-$$"\n'
            'printf \'%s\\n\' "$*" > "$d/$n.args"\n'
            f'/usr/bin/tee "$d/$n.in" | {shlex.quote(str(real))} "$@" | /usr/bin/tee "$d/$n.out"\n')
    replace_private(target, text.encode())
    target.chmod(0o700)
    return target


def posix_entry(helper, arguments, timeout=8):
    quoted = shlex.quote(str(helper))
    return {"hooks": [{"type": "command",
                       "command": f"if [ -x {quoted} ]; then exec {quoted} {arguments}; fi",
                       "timeout": timeout}]}


def merge_hooks(path, groups):
    document = json.loads(path.read_text()) if path.is_file() else {}
    hooks = document.setdefault("hooks", {})
    for event, group in groups:
        hooks.setdefault(event, []).append(group)
    private_directory(path.parent)
    replace_private(path, (json.dumps(document, indent=2) + "\n").encode())
    return path


def install_hooks(runtime, recipe, integration, overlay, helper, cwd):
    """This build's hooks, in the disposable configuration the probe agent reads."""
    kind = recipe["kind"]
    if kind == "claude":
        groups = [(event, posix_entry(helper, f"hook --runtime claude-code --event {event} --memory-injection --source hide-subagents@8"))
                  for event in CLAUDE_EVENTS]
        groups[-1][1]["matcher"] = CLAUDE_MATCHER
        return [merge_hooks(integration["root"] / "settings.json", groups)]
    if kind == "codex":
        groups = [(event, posix_entry(helper, f"hook --runtime codex --event {event} --memory-injection --source hide-subagents@8"))
                  for event in CLAUDE_EVENTS]
        groups[-1][1]["matcher"] = CODEX_MATCHER
        home = Path(overlay["env"]["CODEX_HOME"])
        config = home / "config.toml"
        text = config.read_text() if config.is_file() else ""
        if "codex_hooks" not in text:
            replace_private(config, (text + "\n[features]\ncodex_hooks = true\n").encode())
        return [merge_hooks(home / "hooks.json", groups), config]
    if kind == "grok":
        groups = [(event, posix_entry(helper, f"hook --runtime grok --event {event} --source hide-guidance@5"))
                  for event in ("SessionStart", "PreToolUse", "SubagentStart", "SubagentStop", "Stop")]
        groups[1][1]["matcher"] = GROK_MATCHER
        return [merge_hooks(Path(overlay["env"]["GROK_HOME"]) / "hooks/hide.json", groups)]
    if kind == "cursor":
        path = cwd / ".cursor/hooks.json"
        document = json.loads(path.read_text()) if path.is_file() else {"version": 1, "hooks": {}}
        hooks = document.setdefault("hooks", {})
        hooks.setdefault("sessionStart", []).append(
            {"command": f"{helper} hook --runtime cursor --event SessionStart --source hide-guidance@5", "timeout": 8})
        hooks.setdefault("preToolUse", []).append(
            {"command": f"{helper} hook --runtime cursor --event PreToolUse --source hide-guidance@5", "timeout": 8,
             "matcher": CURSOR_MATCHER})
        private_directory(path.parent)
        replace_private(path, (json.dumps(document, indent=2) + "\n").encode())
        return [path]
    fixtures = runtime.checkout / "hide-agent-hooks/tests/fixtures/hook-bytes"
    if kind == "opencode":
        source, target = fixtures / "opencode-plugin.js", Path(overlay["env"]["XDG_CONFIG_HOME"]) / "opencode/plugins/hide.js"
        # The job's shell calls run without a menu (OpenCode's permission config).
        project = cwd / "opencode.json"
        document = json.loads(project.read_text()) if project.is_file() else {}
        document.setdefault("$schema", "https://opencode.ai/config.json")
        document.setdefault("permission", {})["bash"] = "allow"
        replace_private(project, (json.dumps(document, indent=2) + "\n").encode())
    else:
        source, target = fixtures / (kind + "-extension.ts"), Path(overlay["env"]["PI_CODING_AGENT_DIR"]) / "extensions/hide.ts"
    text = source.read_text().replace(json.dumps(PLACEHOLDER), json.dumps(str(helper)))
    if str(helper) not in text:
        raise ProtectionError("fixture_helper_placeholder_missing")
    private_directory(target.parent)
    replace_private(target, text.encode())
    return [target]


def allow_hook_route(sandbox, runtime):
    """Let a hook inside the sandbox find the private daemon the way the shipped hook does.

    The hook reads the bootstrap socket record under the state folder and connects to the
    socket it names (hide-node pane_proof); the bell check hides the whole run from the
    agent, so this opens exactly that record and the socket's folder, nothing else of
    the controller's state.
    """
    record = runtime.state / "pane-capabilities/bootstrap-socket"
    socket = Path(record.read_text().strip())
    # The daemon binds it in a private folder of its own under /tmp (hide-node pane_proof,
    # bootstrap_parent), named by a random token; the record under this run's own state
    # folder is what ties the folder to this run's daemon and no other.
    if (not socket.is_absolute() or socket.parent.parent != Path("/tmp")
            or not socket.parent.name.startswith("hide-pane") or socket.parent.is_symlink()):
        raise ProtectionError("bootstrap_socket_not_this_daemons")
    quote = lambda value: json.dumps(str(value), ensure_ascii=True)
    sandbox.rules.extend([f"(allow file-read* (literal {quote(record)}))",
                          f"(allow file-read-metadata (literal {quote(runtime.state)}) (literal {quote(record.parent)}))"])
    # The sandbox matches a resolved path, and /tmp is a link to /private/tmp.
    for folder in dict.fromkeys((socket.parent, socket.parent.resolve())):
        sandbox.rules.extend([f"(allow file-read* (subpath {quote(folder)}))",
                              f"(allow network-outbound (remote unix-socket (subpath {quote(folder)})))"])
    replacement = sandbox.profile.with_suffix(".next")
    write_private(replacement, ("\n".join(sandbox.rules) + "\n").encode())
    replacement.replace(sandbox.profile)


def job_prompt(calls, seconds):
    return (f"Run the shell command `sleep {seconds}` exactly {calls} times, one tool call after another, never in "
            "parallel and never combined. A message from Hide may arrive in between: a prompt line beginning with 🔔, "
            "a <system-reminder> block, or additional context attached to a tool result. The moment you see one, "
            "write its marker word (it looks like LIVE_MAIL_ followed by hex) in your own reply text, then go on "
            "with the remaining sleep calls. When all calls are done reply with the single word DONE.")


class Probe:
    def __init__(self, runtime, recipe, agent_home, launch, overlay, integration, label):
        self.runtime, self.recipe, self.agent_home = runtime, recipe, agent_home
        self.launch, self.overlay, self.integration, self.label = launch, overlay, integration, label
        self.workspace = self.pane = self.cwd = None
        self.session = None
        self.calls = runtime.probe / ("hook-calls-" + label)
        private_directory(self.calls)
        self.record = {"agent": recipe["id"], "label": label}

    def start(self, calls, seconds, run):
        runtime, recipe = self.runtime, self.recipe
        self.workspace, self.pane, self.cwd = runtime.new_workspace(recipe, "rest", self.agent_home, self.launch, self.overlay)
        history = self.cwd / "history"
        private_directory(history)
        self.scene_overlay = {**self.overlay, "settings": list(self.overlay["settings"])}
        if recipe["kind"] in ("pi", "omp"):
            self.scene_overlay["session_root"] = history
        extra = configure(runtime, self.launch, recipe, "rest", self.cwd) + project_args(self.integration, recipe, self.cwd)
        helper = helper_wrapper(runtime, self.calls)
        self.record["hook_files"] = [str(p) for p in install_hooks(runtime, recipe, self.integration, self.overlay, helper, self.cwd)]
        values = dict(model=recipe["model"], sockets=str(runtime.short), history=str(history))
        native = [value.format_map(values) for value in recipe["argv"]] + extra + FREE_ARGS[recipe["kind"]]
        if recipe["kind"] == "codex":
            # The recipe's on-request approval is replaced by never.
            cleaned, skip = [], False
            for value in native:
                if skip:
                    skip = False
                    continue
                if value == "--ask-for-approval":
                    skip = True
                    continue
                cleaned.append(value)
            native = cleaned + ["--ask-for-approval", "never"]
        code, output, err = runtime.command(["agent", "start", "live-" + recipe["id"] + "-rest", "--kind", recipe["kind"],
                                             "--pane", self.pane, "--timeout", "5000", "--", *native],
                                            check=False, seconds=10)
        if code:
            write_private(run / (self.label + "-start.json"),
                          json.dumps({"exit_code": code, "stdout": output, "stderr": err}).encode())
        prepare_startup(runtime, self.pane, recipe, "rest", self.cwd, self.workspace, 30,
                        run / (self.label + "-startup-preparation.json"))
        agent = runtime.wait(lambda: (a := runtime.agent(self.pane)) and a.get("agent_status") in ("idle", "done")
                             and not a.get("launch_pending") and a, 60)
        self.session = agent.get("agent_session")
        self.record["herdr_session"] = self.session
        self.file = transcript(self.agent_home, recipe["kind"], self.session, self.scene_overlay.get("session_root"))
        self.offset = len(messages(self.file, recipe["kind"])) if self.file else 0
        self.record["transcript"] = str(self.file) if self.file else None
        runtime.send(self.pane, job_prompt(calls, seconds))
        self.record["prompt_sent_unix"] = time.time()

    def screen(self):
        return self.runtime.command(["pane", "read", self.pane, "--source", "recent-unwrapped", "--lines", "300"])[1]

    def hook_calls(self):
        rows = []
        for file in sorted(self.calls.glob("*.args")):
            stem = file.with_suffix("")
            answer = stem.with_suffix(".out")
            rows.append({"args": file.read_text().strip(), "in": stem.with_suffix(".in"),
                         "out": answer if answer.exists() else None,
                         "at": file.stat().st_mtime, "answered_at": answer.stat().st_mtime if answer.exists() else None})
        return rows

    def letter_show(self, letter_id):
        environment = {**self.runtime.daemon_env, "HERDR_PANE_ID": self.pane,
                       "HIDE_CAP_REF": str(self.runtime.pane_credentials[self.pane])}
        _, output, _ = self.runtime.owner.run([str(self.runtime.hide), "request", "show", letter_id], env=environment)
        return json.loads(output).get("result", {})


def send_letter(runtime, pane, intent, body, now):
    environment = {**runtime.daemon_env, "HERDR_PANE_ID": pane, "HIDE_CAP_REF": str(runtime.pane_credentials[pane])}
    args = [str(runtime.hide), "request", "send", pane, "--intent", intent, "--kind", "report", "--body", body]
    if now:
        args.append("--now")
    _, output, _ = runtime.owner.run(args, env=environment)
    answer = json.loads(output)
    if answer.get("ok") is not True:
        raise ProcessError("private_mailbox_send_refused:" + json.dumps(answer)[:200])
    return answer["result"]["id"]


def observe_letter(probe, letter_id, marker, seconds, run):
    """When the hook handed the letter over and when the agent's own text repeated the marker."""
    runtime, kind = probe.runtime, probe.recipe["kind"]
    sent = time.time()
    row = {"letter_id": letter_id, "sent_unix": sent, "hook_handover_s": None, "read_s": None,
           "status_at_read": None, "state_at_end": None, "hook_confirmed_at_end": None,
           "working_seen": False, "read_before_turn_end": None, "bell_line": None, "samples": []}
    end = min(runtime.owner.deadline, time.monotonic() + seconds)
    turn_ended_at = None
    while time.monotonic() < end:
        agent = runtime.agent(probe.pane) or {}
        status = agent.get("agent_status")
        if status == "working":
            row["working_seen"] = True
        elif row["working_seen"] and status in ("idle", "done") and turn_ended_at is None:
            turn_ended_at = time.time()
        shown = probe.letter_show(letter_id)
        row["state_at_end"], row["hook_confirmed_at_end"] = shown.get("state"), shown.get("hook_confirmed")
        row["bell_line"] = shown.get("bell_line")
        if row["hook_handover_s"] is None:
            for call in probe.hook_calls():
                if call["out"] and marker in call["out"].read_text(errors="replace") and call["answered_at"] >= sent - 1:
                    row["hook_handover_s"] = round(call["answered_at"] - sent, 2)
                    row["hook_args"] = call["args"]
                    try:
                        payload = json.loads(call["in"].read_text(errors="replace"))
                    except ValueError:
                        payload = {}
                    row["hook_session"] = payload.get("sessionId") or payload.get("conversation_id") or payload.get("session_id")
                    break
            if row["hook_handover_s"] is None and kind in ("claude", "codex") and shown.get("hook_confirmed") is True:
                row["hook_handover_s"] = round(time.time() - sent, 2)
        if probe.file is None:
            probe.file = transcript(probe.agent_home, kind, probe.session, probe.scene_overlay.get("session_root"))
            row["transcript"] = str(probe.file) if probe.file else None
        if row["read_s"] is None:
            seen = False
            if probe.file:
                fresh = messages(probe.file, kind)[probe.offset:]
                for index, (role, text) in enumerate(fresh):
                    if role == "assistant" and marker in text:
                        seen = True
                        row["tool_results_after_read"] = sum(1 for r, _ in fresh[index + 1:] if r == "tool")
                        break
            else:
                screen = probe.screen()
                seen = any(line.strip() and marker in line and "🔔" not in line and "sleep" not in line
                           for line in screen.splitlines())
            if seen:
                row["read_s"] = round(time.time() - sent, 2)
                row["status_at_read"] = status
                row["read_before_turn_end"] = turn_ended_at is None
        if row["read_s"] is not None and turn_ended_at is not None:
            break
        if row["read_s"] is None and turn_ended_at is not None and time.time() - turn_ended_at > 20:
            break
        if runtime.owner.cancelled.wait(0.5):
            raise ProcessError("run_cancelled")
    row["turn_ended_s"] = round(turn_ended_at - sent, 2) if turn_ended_at else None
    if probe.file:
        fresh = messages(probe.file, kind)[probe.offset:]
        for index, (role, text) in enumerate(fresh):
            if role == "assistant" and marker in text:
                row["tool_results_after_read"] = sum(1 for r, _ in fresh[index + 1:] if r == "tool")
                row["user_turns_after_read"] = sum(1 for r, _ in fresh[index + 1:] if r == "user")
                break
        row["transcript_tail"] = [(role, text[:300]) for role, text in fresh[-12:]]
    row["screen_tail"] = probe.screen()[-3000:]
    row["hook_calls"] = [{"args": c["args"], "at": c["at"], "answered_at": c["answered_at"]} for c in probe.hook_calls()][-40:]
    # The probe is removed at close; the hook calls are the evidence, so they move under the run.
    kept = run / (probe.label + "-hook-calls")
    shutil.copytree(probe.calls, kept, dirs_exist_ok=True)
    for file in kept.rglob("*"):
        if file.is_file():
            file.chmod(0o600)
    write_private(run / (probe.label + "-" + letter_id.replace("/", "_") + ".json"), json.dumps(row, indent=2, ensure_ascii=False).encode())
    return row


def cost(runtime, probe, samples, run):
    """Wall time of one hook call with no letter waiting, from the controller, per docs/PERFORMANCE_TESTING.md."""
    kind = probe.recipe["kind"]
    session = (probe.session or {}).get("value", "none")
    real = runtime.checkout / "target/debug/hide-agent-hooks"
    env = {**clean_env(), "HOME": str(probe.agent_home), "HIDE_STATE_DIR": str(runtime.state),
           "HERDR_PANE_ID": probe.pane, "HERDR_ENV": "1", "HERDR_SOCKET_PATH": str(runtime.socket),
           "HIDE_CAP_REF": str(runtime.pane_credentials[probe.pane]), "TMPDIR": str(runtime.run / "agent-tmp")}
    if kind == "grok":
        argv = [str(real), "hook", "--runtime", "grok", "--event", "PreToolUse"]
        payload = {"hook_event_name": "PreToolUse", "sessionId": session, "toolName": "run_terminal_command",
                   "toolInput": {"command": "sleep 1"}, "cwd": str(probe.cwd)}
    elif kind == "cursor":
        argv = [str(real), "hook", "--runtime", "cursor", "--event", "PreToolUse"]
        payload = {"conversation_id": session, "generation_id": "g1", "hook_event_name": "preToolUse",
                   "tool_name": "Shell", "tool_input": {"command": "sleep 1", "working_directory": str(probe.cwd)},
                   "cwd": str(probe.cwd)}
    elif kind in ("opencode", "pi", "omp"):
        argv = [str(real), kind, "letters"]
        payload = {"session_id": session, "version": 3 if kind == "opencode" else 5, "working": 0, "done": 0,
                   "lost": 0, "first": False, "letters": [], "guard_only": False}
    else:
        return {"skipped": "bell_target_pays_nothing_at_a_tool"}
    data = json.dumps(payload).encode()
    load_before = os.getloadavg()
    times, outputs = [], []
    import subprocess
    for index in range(samples + 5):
        started = time.perf_counter()
        done = subprocess.run(argv, input=data, env=env, capture_output=True, timeout=10)
        elapsed = (time.perf_counter() - started) * 1000
        if index >= 5:
            times.append(elapsed)
        outputs.append(done.stdout.decode(errors="replace")[:200])
    times.sort()
    report = {"samples": len(times), "warm_up": 5, "p50_ms": round(statistics.median(times), 1),
              "p95_ms": round(times[int(len(times) * 0.95) - 1], 1), "max_ms": round(times[-1], 1),
              "load_before": load_before, "load_after": os.getloadavg(), "argv": argv[1:],
              "answers": sorted(set(outputs))[:5]}
    write_private(run / (probe.label + "-cost.json"), json.dumps(report, indent=2).encode())
    return report


def main(argv=None):
    args = parser().parse_args(argv)
    checkout = Path(__file__).resolve().parents[2]
    operator = Path.home().resolve()
    contract = source_contract(checkout)
    available = recipes(Path(__file__).with_name("recipes"), contract["targets"])
    selected = args.agents.split(",") if args.agents else list(available)
    if set(selected) - available.keys():
        raise ProtectionError("invalid_agent_selection")
    for entry in args.model:
        key, _, model = entry.partition("=")
        available[key]["model"] = model
    slug = "mid-turn-" + datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S") + "-" + os.urandom(2).hex()
    run = (args.run_dir or checkout / "agents/runs/letters-to-busy-agents" / slug).absolute()
    if run.exists() or not beneath(run, checkout / "agents/runs"):
        raise ProtectionError("report_directory_must_be_new_and_local_only")
    private_directory(run)
    owner = OwnedProcesses(diagnostics=run / "process-diagnostics")
    # Seven agents with a five-minute wait each outlast the bell check's half hour.
    owner.deadline = time.monotonic() + 3 * 3600
    report = {"format": 1, "agents": [], "failures": []}
    runtime = None
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, lambda *_: owner.cancelled.set())
    try:
        runtime = Runtime(checkout, run, operator, owner, herdr_bin=args.herdr_bin or default_herdr(checkout, operator))
        chosen = [available[key] for key in selected]
        known = list(dict.fromkeys(operator / rel for r in chosen for rel in r["known"]))
        shared = {operator / rel for r in chosen for rel in r.get("shared", {})}
        executables = [runtime.herdr_bin, Path(sys.executable)]
        executables.extend(Path(v) for r in chosen if (v := shutil.which(r["executable"])))
        sandbox = WriteSandbox(run, runtime.short, operator, runtime.state, checkout=checkout,
                               routing=operator_routing(os.environ), executables=executables,
                               protected=[p for p in known if p not in shared])
        runtime.sandbox = sandbox
        write_private(runtime.bin / "hide", ("#!/bin/sh\nexec " + shlex.quote(str(runtime.hide)) + ' "$@"\n').encode())
        (runtime.bin / "hide").chmod(0o700)
        report["herdr"] = runtime.start()
        allow_hook_route(sandbox, runtime)
        report["hided_sha256"] = report["herdr"].pop("hided_sha256", None)
        for recipe in chosen:
            provider = {"id": recipe["id"], "model": recipe["model"], "bell_target": contract["targets"][recipe["id"]],
                        "runs": [], "cost": None}
            report["agents"].append(provider)
            executable = shutil.which(recipe["executable"])
            if not executable:
                provider["skipped"] = "not_installed"
                continue
            launch = native_wrapper(runtime, recipe, Path(executable), sandbox)
            if recipe.get("overlay"):
                runtime.credential_roots.add(runtime.probe / ("config-" + recipe["id"]))
            overlay = prepare_overlay(recipe, runtime.probe, operator, owner=owner, env=clean_env())
            integration = prepare_integration(runtime, recipe, overlay)
            if recipe["kind"] == "codex" and overlay["session_root"]:
                private_directory(overlay["session_root"])
                link = runtime.home / ".codex/sessions"
                if link.is_symlink():
                    link.unlink()
                link.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                link.symlink_to(overlay["session_root"], target_is_directory=True)
            metadata_env = {**clean_env(), **overlay["env"], "HOME": str(operator), "TMPDIR": str(run / "agent-tmp")}
            _, version, _ = owner.run([str(launch), "--version"], env=metadata_env, check=False)
            provider["version"] = version.strip()[:120]
            plan = [("now", True)] * args.runs + ([("patience", False)] if args.patience else [])
            for index, (label, now) in enumerate(plan):
                probe = Probe(runtime, recipe, operator, launch, overlay, integration, f"{recipe['id']}-{label}-{index}")
                row = {"label": label, "now": now}
                provider["runs"].append(row)
                try:
                    calls, seconds = (args.calls, args.call_seconds) if now else (16, 25)
                    probe.start(calls, seconds, run)
                    row.update(probe.record)
                    runtime.wait(lambda: (runtime.agent(probe.pane) or {}).get("agent_status") == "working", 90)
                    row["working_at_s"] = round(time.time() - probe.record["prompt_sent_unix"], 1)
                    # The first tool call has started before the letter goes, so the letter is
                    # handed over at a later tool event, never read with the prompt.
                    runtime.wait(lambda: True, 3)
                    marker = "LIVE_MAIL_" + secrets.token_hex(6)
                    letter = send_letter(runtime, probe.pane, "mid-turn-" + secrets.token_hex(4),
                                         marker + " please repeat this marker\nSent while you work; reply with the marker word.", now)
                    observe = args.observe_seconds if now else 420
                    row.update(observe_letter(probe, letter, marker, observe, run))
                    if label == "now" and index == 0:
                        provider["cost"] = cost(runtime, probe, args.cost_samples, run)
                except (ProcessError, ProtectionError) as error:
                    if owner.cancelled.is_set():
                        raise
                    row["failure"] = str(error)
                    try:
                        row["screen_tail"] = probe.screen()[-3000:] if probe.pane else None
                    except Exception:
                        pass
                finally:
                    if probe.workspace:
                        try:
                            runtime.close_workspace(probe.workspace)
                        except Exception as error:
                            report["failures"].append({"agent": recipe["id"], "phase": "close", "reason": str(error)})
    except Exception as error:
        report["failures"].insert(0, {"type": type(error).__name__, "reason": str(error)})
    finally:
        try:
            report["cleanup"] = runtime.close() if runtime else {"confirmed": False}
        except Exception as error:
            report["cleanup"] = {"confirmed": False, "failures": [str(error)]}
            try:
                owner.close()
            except Exception:
                pass
    write_private(run / "report.json", json.dumps(report, indent=2, ensure_ascii=False, default=str).encode())
    print(run / "report.json")
    return 0 if not report["failures"] else 1
