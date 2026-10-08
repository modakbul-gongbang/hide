"""Controller-originated real mail; only an assistant bell reply proves receipt."""

import secrets

from .authentication import require_no_login
from .conversation import bell_turn, messages
from .processes import ProcessError
from .scenes import transcript
from .timing import Deadline, ObservationTimeout


def measure(runtime, pane, recipe, home, cwd, bell, seconds, overlay):
    initial = runtime.agent(pane)
    session = initial.get("agent_session") if initial else None
    file = transcript(home, recipe["kind"], session, overlay["session_root"])
    if file is None:
        return {"outcome": "unknown", "reason": "no_supported_positive_native_session_reader"}
    require_no_login(runtime.screen(pane))
    if initial["agent_status"] == "blocked":
        return {"outcome": "unknown", "reason": "recipient_blocked"}
    # Generated after the prior prompt, held only in controller memory. There
    # is no native helper, marker receipt or pre-bell marker-bearing prompt.
    offset = len(messages(file, recipe["kind"]))
    marker = "LIVE_MAIL_" + secrets.token_hex(16)
    letter_id = runtime.send_letter(pane, "live-proof-" + secrets.token_hex(8),
                                    "Reply with this marker in your own assistant text: " + marker)
    deadline = Deadline(runtime.owner, seconds)
    sent = False
    try:
        while not deadline.expired():
            screen = runtime.screen(pane, seconds=deadline.command_seconds(), deadline=deadline.end)
            deadline.remaining()
            require_no_login(screen)
            current = runtime.agent(pane, seconds=deadline.command_seconds(), deadline=deadline.end)
            deadline.remaining()
            if not current or current.get("agent_session") != session:
                return {"outcome": "unknown", "reason": "recipient_identity_changed"}
            fresh = messages(file, recipe["kind"])[offset:]
            deadline.remaining()
            if bell_turn(fresh, bell, marker):
                deadline.remaining()
                return {"outcome": "verified", "reason": "private_hided_letter_marker_in_bell_started_assistant_reply",
                        "letter_id": letter_id, "session_source": session["source"]}
            if current["agent_status"] == "blocked":
                return {"outcome": "unknown", "reason": "inbox_needs_approval", "letter_id": letter_id}
            if not sent and current["agent_status"] in ("idle", "done"):
                if not any(role == "user" and (text == bell or text.startswith(bell + "\n")) for role, text in fresh):
                    runtime.send(pane, bell, deadline=deadline)
                sent = True
            if runtime.owner.cancelled.wait(0.15):
                raise ProcessError("run_cancelled")
    except ObservationTimeout:
        pass
    return {"outcome": "unknown", "reason": "delivery_observation_timeout", "letter_id": letter_id}
