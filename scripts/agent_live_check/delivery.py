"""Controller-originated real mail; only hided's own bell and its assistant reply prove receipt."""

import secrets

from .authentication import require_no_login
from .conversation import bell_turn, messages
from .processes import ProcessError
from .scenes import transcript
from .timing import Deadline, ObservationTimeout


SUMMARY = "@src/main.rs $HOME /review !ls #881 `cargo test` 진행 상황 C:\\"


def measure(runtime, pane, recipe, home, cwd, seconds, quiet_seconds, overlay):
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
    # The first line is what the bell types, so it carries the characters a
    # composer acts on: the bell must still arrive as one submitted prompt.
    letter_id = runtime.send_letter(pane, "live-proof-" + secrets.token_hex(8),
                                    SUMMARY + "\nReply with this marker in your own assistant text: " + marker)
    # The controller types nothing: only the line hided's doorbell typed and
    # kept on the letter opens the turn its hook hands the letter to, and the
    # doorbell waits for the pane to be quiet first.
    deadline = Deadline.observation(runtime.owner, seconds + quiet_seconds)
    bell = None
    try:
        while not deadline.expired():
            screen = runtime.screen(pane, seconds=deadline.command_seconds(), deadline=deadline.end)
            deadline.remaining()
            require_no_login(screen)
            current = runtime.agent(pane, seconds=deadline.command_seconds(), deadline=deadline.end)
            deadline.remaining()
            if not current or current.get("agent_session") != session:
                return {"outcome": "unknown", "reason": "recipient_identity_changed"}
            if bell is None:
                bell = runtime.bell_line(pane, letter_id, seconds=deadline.command_seconds(),
                                         deadline=deadline.end)
                deadline.remaining()
            fresh = messages(file, recipe["kind"])[offset:]
            deadline.remaining()
            if bell is not None and bell_turn(fresh, bell, marker):
                deadline.remaining()
                return {"outcome": "verified", "reason": "private_hided_letter_marker_in_hided_bell_assistant_reply",
                        "letter_id": letter_id, "bell": bell, "session_source": session["source"]}
            if current["agent_status"] == "blocked":
                return {"outcome": "unknown", "reason": "inbox_needs_approval", "letter_id": letter_id}
            if runtime.owner.cancelled.wait(0.15):
                raise ProcessError("run_cancelled")
    except ObservationTimeout:
        pass
    return {"outcome": "unknown", "reason": "delivery_observation_timeout", "letter_id": letter_id,
            "bell": bell}
