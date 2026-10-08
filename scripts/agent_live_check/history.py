"""A real previous turn, never a fabricated provider session or identity."""

from .authentication import require_no_login
from .conversation import messages
from .processes import ProcessError
from .scenes import matches, transcript
from .timing import Deadline, ObservationTimeout

LABEL = "LIVE_PREVIOUS_SESSION"
PROMPT = "Reply with exactly " + LABEL + ". Do not use tools."


def seed(runtime, pane, recipe, home, overlay, seconds):
    if runtime.fixture_bin:
        return {"status": "fixture", "visible_tokens": ["fixture-previous"]}
    deadline = Deadline(runtime.owner, seconds)
    sent = False
    working = False
    try:
        while not deadline.expired():
            screen = runtime.screen(pane, seconds=deadline.command_seconds())
            deadline.remaining()
            require_no_login(screen)
            agent = runtime.agent(pane, seconds=deadline.command_seconds())
            deadline.remaining()
            if agent and not sent and agent["agent_status"] in ("idle", "done") and matches(
                    recipe["scenes"]["rest"]["arrived"], screen, ""):
                runtime.send(pane, PROMPT, deadline=deadline)
                sent = True
            session = agent.get("agent_session") if agent else None
            working |= bool(sent and agent and agent["agent_status"] == "working")
            if (recipe["kind"] not in ("claude", "codex", "pi", "omp") and sent and working
                    and session and session.get("source") == "herdr:" + recipe["kind"]
                    and agent["agent_status"] in ("idle", "done") and LABEL in screen
                    and matches(recipe["scenes"]["rest"]["arrived"], screen, "")):
                # A real submitted prompt on a positively identified native
                # session is enough to seed its own resume catalog. This is not
                # role-aware assistant evidence and cannot verify mail delivery.
                deadline.remaining()
                return {"status": "native_prompt_completed", "session": session,
                        "visible_tokens": [LABEL], "assistant_reply_verified": False}
            file = transcript(home, recipe["kind"], session, overlay["session_root"])
            if sent and file:
                rows = messages(file, recipe["kind"])
                for index, (role, text) in enumerate(rows):
                    if role == "user" and text == PROMPT and any(
                            r == "assistant" and LABEL in t for r, t in rows[index + 1:]):
                        tokens = [LABEL, file.name]
                        if session["kind"] == "id":
                            tokens.append(session["value"][:8])
                        deadline.remaining()
                        return {"status": "native_turn_observed", "session": session,
                                "visible_tokens": tokens}
            if runtime.owner.cancelled.wait(0.15):
                raise ProcessError("run_cancelled")
    except ObservationTimeout:
        pass
    return {"status": "unknown", "reason": "owned_previous_turn_not_observed", "visible_tokens": []}
