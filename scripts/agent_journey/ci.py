"""CI mode: each agent in the run's disposable HOME, signed in with an API key
from the environment, no write sandbox (PRD agent-journey-check D-31).

The candidate daemon's HOME is the agents' HOME, so the kit's hooks and
Herdr's integration are where each agent reads its own configuration, as on
a new machine. This table is CI mode's only per-agent CLI knowledge, as the
route table is local mode's. With `--fixture-bin` the compiled stand-ins of
`web/e2e/shims` run in place of the CLIs and no key is used.
"""

import json
import os
from pathlib import Path
import shlex
import shutil

from agent_live_check.protection import ProtectionError, private_directory, write_private

from .local import QUIET

LOGINS = {
    # Claude Code reads the key from ANTHROPIC_API_KEY; a fresh HOME first
    # asks to finish onboarding, to accept that key and to trust the folder,
    # which the disposable ~/.claude.json answers for the measured repository
    # only, so the never-trusted cell still meets Claude's folder question.
    "claude": {"secret": "HIDE_CLAUDE_API_KEY", "key_env": "ANTHROPIC_API_KEY"},
    # Codex stores an API key login in its home with `codex login --with-api-key`.
    "codex": {"secret": "HIDE_CODEX_API_KEY", "key_env": None},
}

# A stand-in asks no folder question, so its start anywhere says nothing of one.
FIXTURE_UNTRUSTED = "not measured: a stand-in has no folder question; the login lane measures it"

# What a fixture stand-in cannot show, said in its report (PRD B27).
FIXTURE_LIMITS = [
    "A stand-in answers with the token its prompt or its hook context carries; no model reads the prompt.",
    "Herdr's integration runs from the hook commands the kit installed, fed the payload the stand-in writes, "
    "so a change in the real CLI's hook payload is not seen.",
    "The working and idle screens and titles and the session file format are the stand-in's copies of the CLI's.",
    "A stand-in waits 5 s before its first turn, as a CLI's start takes seconds, so a first turn that ends "
    "sooner than Hide learns the session is not exercised.",
]


def executable(fixture_bin, kind):
    """The program a pane's `claude` or `codex` runs, or None when it is not installed."""
    if fixture_bin:
        program = fixture_bin / (kind + (".exe" if os.name == "nt" else ""))
        if not program.is_file() or not os.access(program, os.X_OK):
            raise ProtectionError(f"fixture_bin_missing_{kind}: build web/e2e/shims/claude-shim.c into {fixture_bin}")
        return program
    found = shutil.which(kind)
    return Path(found) if found else None


def not_configured(kind, fixture_bin):
    """The secret this agent's login needs, when the lane has none."""
    if fixture_bin:
        return None
    secret = LOGINS[kind]["secret"]
    return None if os.environ.get(secret) else secret


def route_names(kind, fixture_bin):
    """The environment CI mode adds to an agent, by name only (its values are paths or a key)."""
    names = ["HOME", "HIDE_STATE_DIR", "PATH", *QUIET]
    if kind == "codex":
        names.append("CODEX_HOME")
    if fixture_bin:
        names.append("HIDE_E2E_JOURNEY")
    elif LOGINS[kind]["key_env"]:
        names.append(LOGINS[kind]["key_env"])
    return names


def key_file(runtime, kind):
    """Where the run keeps the key for the agent's wrapper to read; under the
    probe, which the cleanup removes, so no report or artifact carries it."""
    return runtime.probe / "keys" / kind


def wrapper(runtime, program, kind, path, journey_config=None):
    """The `claude` or `codex` a pane runs: the agent in the disposable HOME,
    with the runner's PATH behind the candidate's `hide` and, for Claude, its key."""
    private_directory(runtime.bin)
    target = runtime.bin / kind
    environment = {"HOME": str(runtime.home), "HIDE_STATE_DIR": str(runtime.state),
                   "PATH": os.pathsep.join((str(runtime.bin), path)), **QUIET}
    if kind == "codex":
        environment["CODEX_HOME"] = str(runtime.home / ".codex")
    if journey_config:
        environment["HIDE_E2E_JOURNEY"] = str(journey_config)
    lines = ["#!/bin/sh", "unset CLAUDE_CONFIG_DIR"]
    lines.append("export " + " ".join(f"{key}={shlex.quote(value)}" for key, value in environment.items()))
    key_env = LOGINS[kind]["key_env"]
    if key_env and not journey_config:
        lines.append(f"{key_env}=$(cat {shlex.quote(str(key_file(runtime, kind)))}) && export {key_env}")
    lines.append("exec " + shlex.quote(str(program)) + ' "$@"')
    write_private(target, ("\n".join(lines) + "\n").encode())
    target.chmod(0o700)
    return target


def login(runtime, kind, launch, root):
    """Signs the agent in with the lane's key; returns the refusal, or None."""
    secret = os.environ[LOGINS[kind]["secret"]]
    keys = runtime.probe / "keys"
    private_directory(keys)
    write_private(key_file(runtime, kind), secret.encode())
    if kind == "claude":
        claude_settings(runtime.home, root, secret)
        return None
    environment = {"HOME": str(runtime.home), "PATH": runtime.env["PATH"], "TMPDIR": runtime.env["TMPDIR"]}
    command = f"{shlex.quote(str(launch))} login --with-api-key < {shlex.quote(str(key_file(runtime, kind)))}"
    code, _, error = runtime.owner.run(["/bin/sh", "-c", command], env=environment, check=False)
    if code:
        return "codex login --with-api-key refused: " + error.strip()[:200]
    code, output, error = runtime.owner.run([str(launch), "login", "status"], env=environment, check=False)
    return None if not code else "codex login status after the key login: " + (output + error).strip()[:200]


def claude_settings(home, root, secret):
    """The disposable ~/.claude.json a signed-in Claude would hold for the measured repository."""
    write_private(home / ".claude.json", json.dumps({
        "hasCompletedOnboarding": True,
        "customApiKeyResponses": {"approved": [secret[-20:]], "rejected": []},
        "projects": {str(root): {"hasTrustDialogAccepted": True}},
    }).encode())


def fixture_config(runtime, kind):
    """The stand-in's configuration: the hook commands the kit installed for
    `kind`, in the order its file lists them (web/e2e/shims/live-check.h)."""
    files = {"claude": runtime.home / ".claude/settings.json", "codex": runtime.home / ".codex/hooks.json"}
    document = json.loads(files[kind].read_text())
    # Each hook's stderr and exit status, kept in the run folder as evidence.
    lines = ["shell\t/bin/sh", f"index\t{runtime.probe / 'fixture-sessions'}", f"log\t{runtime.run / f'fixture-{kind}-hooks.log'}"]
    for event, groups in document.get("hooks", {}).items():
        for group in groups:
            for hook in group.get("hooks", []):
                command = hook.get("command")
                if hook.get("type") == "command" and isinstance(command, str) and "\n" not in command:
                    lines.append(f"hook\t{event}\t{command}")
    target = runtime.probe / f"fixture-{kind}.config"
    write_private(target, ("\n".join(lines) + "\n").encode())
    return target
