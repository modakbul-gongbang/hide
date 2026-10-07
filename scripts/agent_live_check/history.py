"""A real previous turn, never a fabricated provider session or identity."""

import time

from .authentication import require_no_login
from .conversation import messages
from .processes import ProcessError
from .scenes import matches, transcript

LABEL = "LIVE_PREVIOUS_SESSION"
PROMPT = "Reply with exactly " + LABEL + ". Do not use tools."


def seed(runtime, pane, recipe, home, overlay, seconds):
    if runtime.fixture_bin:
        return {"status": "fixture", "visible_tokens": ["fixture-previous"]}
    if recipe["kind"] not in ("claude", "codex", "pi", "omp"):
        return {"status": "unknown", "reason": "no_supported_owned_session_reader", "visible_tokens": []}
    deadline = time.monotonic() + seconds
    sent = False
    while time.monotonic() < deadline:
        screen = runtime.screen(pane)
        require_no_login(screen)
        agent = runtime.agent(pane)
        if agent and not sent and agent["agent_status"] in ("idle", "done") and matches(
                recipe["scenes"]["rest"]["arrived"], screen, ""):
            runtime.send(pane, PROMPT)
            sent = True
        session = agent.get("agent_session") if agent else None
        file = transcript(home, recipe["kind"], session, overlay["session_root"])
        if sent and file:
            rows = messages(file, recipe["kind"])
            for index, (role, text) in enumerate(rows):
                if role == "user" and text == PROMPT and any(
                        r == "assistant" and LABEL in t for r, t in rows[index + 1:]):
                    tokens = [LABEL, file.name]
                    if session["kind"] == "id":
                        tokens.append(session["value"][:8])
                    return {"status": "native_turn_observed", "session": session,
                            "visible_tokens": tokens}
        if runtime.owner.cancelled.wait(0.15):
            raise ProcessError("run_cancelled")
    return {"status": "unknown", "reason": "owned_previous_turn_not_observed", "visible_tokens": []}
