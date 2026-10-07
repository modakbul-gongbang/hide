"""Reach a data-defined scene, ring only when unblocked, observe effects."""

import json
from pathlib import Path
import re
import time

from .conversation import bell_turn, messages
from .authentication import AuthenticationRequired, require_no_login
from .processes import ProcessError
from .protection import ProtectionError, beneath, write_private


def matches(pattern: str, screen: str, bell: str) -> bool:
    # Match active detection rows, never a provider's growing scrollback.
    return bool(re.search(pattern.replace("{bell}", re.escape(bell)), screen,
                          flags=re.MULTILINE | re.IGNORECASE))


def arrived(data: dict, screen: str, bell: str) -> bool:
    return matches(data["arrived"], screen, bell) and (
        not data.get("controls") or matches(data["controls"], screen, bell))


def startup_blocker(scene, recipe, pane, result, actual):
    code, _, error = result
    try:
        refusal = json.loads(error).get("error", {}).get("code")
    except (ValueError, AttributeError):
        return False
    return bool(code and scene == "startup" and refusal == "agent_not_ready" and actual
                and actual.get("pane_id") == pane and actual.get("agent") == recipe["kind"]
                and actual.get("name") == "live-" + recipe["id"] + "-" + scene
                and actual.get("agent_status") == "blocked")


def transcript(home: Path, kind: str, session: dict | None, session_root: Path | None = None) -> Path | None:
    if not session or session.get("source") != "herdr:" + kind:
        return None
    roots = {"claude": ".claude/projects", "codex": ".codex/sessions",
             "pi": ".pi/agent/sessions", "omp": ".omp/agent/sessions"}
    if kind not in roots:
        return None
    root = session_root or home / roots[kind]
    if session.get("kind") == "path":
        file = Path(session.get("value", ""))
        if not file.is_absolute() or not beneath(file, root) or file.is_symlink():
            raise ProtectionError("native_session_path_outside_owned_history")
        return file if file.is_file() else None
    if session.get("kind") != "id":
        return None
    identity = session.get("value", "")
    if not re.fullmatch(r"[A-Za-z0-9._-]{1,256}", identity):
        raise ProtectionError("invalid_native_session_identity")
    if not root.is_dir():
        return None
    pending, count, found = [root], 0, []
    while pending:
        path = pending.pop()
        count += 1
        if count > 50_000:
            raise ProtectionError("session_location_over_budget")
        if path.is_symlink():
            continue
        if path.is_dir():
            pending.extend(path.iterdir())
        elif path.name.endswith(identity + ".jsonl"):
            found.append(path)
    if len(found) > 1:
        raise ProtectionError("ambiguous_native_session_location")
    return found[0] if found else None


def observe(runtime, pane: str, recipe: dict, scene: str, bell: str,
            home: Path, seconds: float, evidence: Path, cwd: Path, overlay: dict,
            previous_session: dict | None = None) -> dict:
    data = recipe["scenes"][scene]
    row = {"scene": scene, "arrival": "unreached", "status": "unknown",
           "effect": "not_tested", "reason": "scene_not_observed", "evidence": evidence.name}
    samples = []
    before = None
    deadline = time.monotonic() + seconds
    try:
        if data["send"]:
            runtime.send(pane, data["send"])
        while time.monotonic() < deadline:
            screen = runtime.screen(pane)
            require_no_login(screen)
            agent = runtime.agent(pane)
            previous_visible = (scene != "resume_picker" or runtime.fixture_bin or
                                (previous_session and any(token in screen for token in previous_session["visible_tokens"])))
            if agent and arrived(data, screen, bell) and previous_visible:
                before = {"screen": screen, "agent": agent}
                break
            samples = [{"phase": "arrival", "screen": screen, "agent": agent}]
            runtime.wait(lambda: True, 0.01)
            runtime.owner.cancelled.wait(0.15)
            if runtime.owner.cancelled.is_set():
                raise ProcessError("run_cancelled")
        if before is None:
            row["arrival"] = "timeout"
            row["reason"] = "arrival_deadline"
            return row
        row.update(arrival="reached", status=before["agent"]["agent_status"])
        samples = [{"phase": "before", **before}]
        if row["status"] == "blocked":
            row.update(effect="guarded", reason="Herdr_blocked_no_bell_typed")
            return row
        # The deliberate unguarded input is the experiment; agent.prompt would
        # refuse the very menus this tool needs to measure.
        session = before["agent"].get("agent_session")
        file = transcript(home, recipe["kind"], session, overlay["session_root"])
        previous = messages(file, recipe["kind"]) if file else []
        from .protection import stamp
        configuration = {path: stamp(path) for path in overlay["settings"]}
        existing_probes = {path.name for path in cwd.glob("probe-*.txt")}
        runtime.send(pane, bell)
        deadline = time.monotonic() + seconds
        effect = "ambiguous"
        reason = "no_positive_effect_evidence"
        while time.monotonic() < deadline:
            screen = runtime.screen(pane)
            require_no_login(screen)
            agent = runtime.agent(pane)
            samples = [samples[0], {"phase": "after", "screen": screen, "agent": agent}]
            if any(stamp(path) != original for path, original in configuration.items()):
                effect, reason = "settings_write", "private_settings_or_trust_changed_after_bell"
                break
            if agent and session and agent.get("agent_session") != session:
                effect, reason = "resumed_session", "native_session_identity_changed"
                break
            if scene in ("shell_approval", "file_approval", "plan_approval") and (
                    {path.name for path in cwd.glob("probe-*.txt")} - existing_probes):
                effect, reason = "approval", "probe_file_created_after_bell"
                break
            # The fixture and some native CLIs report explicit completed
            # selection/approval actions; losing the menu alone proves nothing.
            if matches(data["unsafe"], screen, bell):
                effect, reason = ("selection" if scene.endswith("picker") or scene == "startup"
                                  else "approval"), "explicit_completed_action"
                break
            if matches(data["no_match"], screen, bell) and scene.endswith("picker"):
                effect, reason = "no_match", "picker_reports_no_matches"
                break
            if file:
                fresh = messages(file, recipe["kind"])[len(previous):]
                if bell_turn(fresh, bell):
                    effect, reason = "new_turn", "native_user_bell_then_assistant_reply"
                    break
            if runtime.owner.cancelled.wait(0.15):
                raise ProcessError("run_cancelled")
        # A draft must still be present at the end of the observation window;
        # seeing the compositor briefly while input is submitted is not a draft.
        if effect == "ambiguous" and samples and matches(data["draft"], samples[-1]["screen"], bell):
            effect, reason = "unsent_draft", "bell_remains_in_composer_at_deadline"
        row.update(effect=effect, reason=reason)
        return row
    except AuthenticationRequired:
        row.update(arrival="skipped", effect="not_tested", reason="not_authenticated_no_login_attempted")
        raise
    finally:
        write_private(evidence, (json.dumps({"observation": row, "samples": samples}, indent=2) + "\n").encode())
