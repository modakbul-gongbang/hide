"""One local command; native and synthetic measurements are explicitly distinct."""

import argparse
import datetime
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import sys
import time

from .contracts import SCENES, recipes, source_contract
from .delivery import measure as measure_delivery
from .processes import OwnedProcesses, ProcessError
from .protection import ConfigGuard, ProtectionError, beneath, private_directory, stamp, write_private
from .report import save
from .runtime import Runtime, clean_env
from .sandbox import WriteSandbox
from .scenes import observe


def parser():
    value = argparse.ArgumentParser(description="Measure bell safety on private pinned Herdr and this checkout's hided.")
    value.add_argument("--agents", help="Comma-separated adapter IDs; default is all current adapters")
    value.add_argument("--model", action="append", default=[], metavar="ID=MODEL",
                       help="Account's cheapest suitable model, passed only on the command line")
    value.add_argument("--run-dir", type=Path, help="New directory below this checkout's agents/runs")
    value.add_argument("--herdr-bin", type=Path, help="Previously fetched pinned Herdr; bytes are checked")
    value.add_argument("--socket", type=Path, help="Private socket; operator/default/existing paths are refused")
    value.add_argument("--state-dir", type=Path, help="Private state directory inside the run")
    value.add_argument("--scene-seconds", type=float, default=30)
    value.add_argument("--fixture-bin", type=Path, help="CI only: compiled Claude/Codex shim directory; uses private HOME")
    return value


def default_herdr(checkout, home):
    pin = json.loads((checkout / "contracts/herdr-bundle.json").read_text())
    if sys.platform == "darwin":
        cache, digest = home / "Library/Caches/hide/herdr-runtime", pin["sha256"]
    elif sys.platform.startswith("linux"):
        cache, digest = home / ".cache/hide/herdr-runtime", pin["linux_x86_64"]["sha256"]
    else:
        raise ProtectionError("live_check_host_not_supported")
    return cache / digest / "herdr"


def wrapper(runtime, recipe, executable, sandbox):
    directory = runtime.bin / recipe["id"]
    private_directory(directory)
    target = directory / recipe["executable"]
    command = [str(executable)]
    if sandbox:
        command = sandbox.command(command)
    # Herdr owns integration arguments. This wrapper neither replaces nor
    # installs those integrations and keeps the measured HOME for login.
    fixture_env = "export HIDE_E2E_LIVE_CHECK=1\n" if runtime.fixture_bin else ""
    write_private(target, ("#!/bin/sh\n" + fixture_env + "exec " + " ".join(shlex.quote(v) for v in command) + ' "$@"\n').encode())
    target.chmod(0o700)
    return target


def main(argv=None):
    args = parser().parse_args(argv)
    checkout = Path(__file__).resolve().parents[2]
    operator = Path.home().resolve()
    if not 1 <= args.scene_seconds <= 120:
        raise ProtectionError("scene_seconds_out_of_bounds")
    contract = source_contract(checkout)
    available = recipes(Path(__file__).with_name("recipes"), contract["targets"])
    selected = args.agents.split(",") if args.agents else list(available)
    if not selected or len(set(selected)) != len(selected) or set(selected) - available.keys():
        raise ProtectionError("invalid_agent_selection")
    overrides = {}
    for entry in args.model:
        key, separator, model = entry.partition("=")
        if not separator or key not in selected or not model or key in overrides:
            raise ProtectionError("invalid_model_override")
        overrides[key] = model
    for key, model in overrides.items():
        available[key]["model"] = model
    if args.fixture_bin and (set(selected) - {"claude-code", "codex"}):
        raise ProtectionError("fixtures_only_support_claude_and_codex")
    slug = "live-check-" + datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S") + "-" + os.urandom(3).hex()
    run = (args.run_dir or checkout / "agents/runs" / slug).absolute()
    if run.exists() or run.is_symlink() or not beneath(run, checkout / "agents/runs"):
        raise ProtectionError("report_directory_must_be_new_and_local_only")
    private_directory(run)
    owner = OwnedProcesses()
    report = {"format": 1, "fixture": bool(args.fixture_bin), "herdr": {}, "agents": [],
              "source": contract["declaration_sha256"], "configuration": {},
              "cleanup": {"confirmed": False}, "failures": [], "resources": {}}
    runtime = guard = None
    previous_signals = {}
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        previous_signals[signum] = signal.signal(signum, lambda *_: owner.cancelled.set())
    try:
        runtime = Runtime(checkout, run, operator, owner,
                          herdr_bin=args.herdr_bin or default_herdr(checkout, operator),
                          fixture_bin=args.fixture_bin, socket=args.socket, state=args.state_dir)
        # Runtime rejects operator paths and wrong binaries before this point.
        agent_home = runtime.home if args.fixture_bin else operator
        all_recipes = [available[key] for key in selected]
        known = list(dict.fromkeys(agent_home / relative for recipe in all_recipes for relative in recipe["known"]))
        roots = list(dict.fromkeys(agent_home / relative for recipe in all_recipes for relative in recipe["roots"]))
        histories = list(dict.fromkeys(agent_home / relative for recipe in all_recipes for relative in recipe["histories"]))
        guard = ConfigGuard(run / "configuration-backup", known, roots, histories=histories,
                            exclusive_root=agent_home if args.fixture_bin else None)
        sandbox = None if args.fixture_bin else WriteSandbox(run, runtime.short, operator, histories)
        if sandbox:
            outside = runtime.short / "guard-outside"
            private_directory(outside)
            sandbox.verify(owner, {**clean_env(), "TMPDIR": str(run / "agent-tmp")}, outside)
        # Only the candidate CLI is reachable as `hide` in a probe workspace.
        write_private(runtime.bin / "hide", ("#!/bin/sh\nexec " + shlex.quote(str(runtime.hide)) + ' "$@"\n').encode())
        (runtime.bin / "hide").chmod(0o700)
        report["herdr"] = runtime.start()
        for recipe in all_recipes:
            provider = {"id": recipe["id"], "version": "unavailable", "model": recipe["model"],
                        "model_note": recipe["model_note"], "bell_target": contract["targets"][recipe["id"]],
                        "integration": {"status": "not_observed"}, "scenes": [], "login": recipe["login"],
                        "delivery": {"outcome": "unknown", "reason": "not_measured"}}
            report["agents"].append(provider)
            executable = ((args.fixture_bin / recipe["executable"]).resolve() if args.fixture_bin
                          else shutil.which(recipe["executable"]))
            if not executable or not Path(executable).is_file():
                provider["skipped"] = "not_installed"
            else:
                launch = wrapper(runtime, recipe, Path(executable), sandbox)
                metadata_env = {**clean_env(), "HOME": str(agent_home), "TMPDIR": str(run / "agent-tmp")}
                code, version, err = owner.run([str(launch), "--version"], env=metadata_env, check=False)
                provider["version"] = version.strip()[:256]
                if code:
                    provider["skipped"] = "version_probe_failed"
                    write_private(run / (recipe["id"] + "-version-error.txt"), err.encode())
                else:
                    for scene in SCENES:
                        workspace = None
                        evidence = run / (recipe["id"] + "-" + scene + ".json")
                        try:
                            workspace, pane, cwd = runtime.new_workspace(recipe, scene, agent_home, launch)
                            history = cwd / "history"
                            private_directory(history)
                            mcp = cwd / "live-mcp.json"
                            write_private(mcp, json.dumps({"mcpServers": {"live_probe": {
                                "command": sys.executable, "args": [str(checkout / "scripts/agent_live_check/mcp_fixture.py")]}}}).encode())
                            values = dict(model=recipe["model"], sockets=str(runtime.short),
                                          history=str(history), mcp=str(mcp))
                            native_args = [value.format_map(values) for value in recipe["argv"]]
                            runtime.command(["agent", "start", "live-" + recipe["id"] + "-" + scene,
                                             "--kind", recipe["kind"], "--pane", pane, "--timeout", "3000",
                                             "--", *native_args], check=False, seconds=8)
                            startup_screen = runtime.screen(pane)
                            if re.search(r"Authentication required|not logged in|Please (?:log|sign) in",
                                         startup_screen, flags=re.IGNORECASE):
                                provider["skipped"] = "not_authenticated"
                                write_private(evidence, json.dumps({"screen": startup_screen,
                                              "reason": "not_authenticated_no_login_attempted"}).encode())
                                break
                            native = runtime.agent(pane)
                            session = native.get("agent_session") if native else None
                            if session and session.get("source") == "herdr:" + recipe["kind"]:
                                provider["integration"] = {"status": "native_session_observed",
                                    "source": session["source"], "herdr_version": runtime.expected_version,
                                    "binary_sha256": report["herdr"]["sha256"]}
                            provider["scenes"].append(observe(runtime, pane, recipe, scene, contract["bell"],
                                                               agent_home, args.scene_seconds, evidence, cwd))
                            if scene == "rest" and provider["scenes"][-1]["arrival"] == "reached":
                                provider["delivery"] = measure_delivery(runtime, pane, recipe, agent_home,
                                                                        cwd, contract["bell"], args.scene_seconds)
                        except ProcessError as error:
                            if owner.cancelled.is_set():
                                raise
                            provider["scenes"].append({"scene": scene, "arrival": "timeout", "status": "unknown",
                                                       "effect": "not_tested", "reason": str(error), "evidence": evidence.name})
                            if not evidence.exists():
                                write_private(evidence, json.dumps({"reason": str(error)}).encode())
                        finally:
                            if workspace:
                                runtime.close_workspace(workspace)
            if provider.get("skipped"):
                provider["scenes"] = [{"scene": scene, "arrival": "skipped", "status": "unknown",
                                       "effect": "not_tested", "reason": provider["skipped"]} for scene in SCENES]
            report["resources"] = owner.usage()
    except Exception as error:
        report["failures"].append({"type": type(error).__name__, "reason": str(error)})
    finally:
        if runtime:
            report["cleanup"] = runtime.close()
        else:
            owner.close()
            report["cleanup"] = {"confirmed": True, "probe_removed": True}
        if guard:
            try:
                if args.fixture_bin:
                    # Only a sole-owned disposable HOME may claim all its
                    # writers. Native runs never infer operator ownership.
                    for path, before in guard.before.items():
                        current = stamp(path)
                        if current != before:
                            guard.record_write(path, before, current)
                report["configuration"] = guard.finish()
            except Exception as error:
                report["configuration"] = {"failures": [{"reason": str(error)}]}
        for signum, handler in previous_signals.items():
            signal.signal(signum, handler)
    code = save(run, report)
    print(f"{run / 'report.md'}\n{run / 'report.json'}\nexit={code}")
    return code
