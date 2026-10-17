"""The fork cell: Fork agent makes the operator's own copy of the agent's
conversation, and Hide names where it came from.

The operator chooses Fork agent from the pane menu, which sends `fork_pane`
with the tab placement; the journey sends the same event on its own screen.
Hide then shows a new row whose `forked_from` names the source's pane, with
a session of its own that Herdr's integration reported, and `hide agent list`
registers the fork as a root (no parent) whose origin is the source
(docs/status-model.md, Where a parent comes from). A fork starts no turn, so
it spends no model request.
"""

from agent_live_check.processes import ProcessError

from . import cells

CELL = "fork"
# The fork starts the agent again and waits for its session (Hide waits 5 s
# for it before it registers the fork); a CLI's start takes seconds.
FORK_SECONDS = 90


def forked_row(rest, source):
    """The row Hide shows as forked from pane `source`, or None."""
    return next((row for row in (rest.get("navigator") or {}).get("agents") or []
                 if (row.get("forked_from") or {}).get("pane_id") == source), None)


def verdict(row, source_session, herdr_session, kind, listed, source_agent):
    """`row` the forked row, `herdr_session` its pane's session as Herdr
    reports it, `listed` its `hide agent list` entry."""
    if not row:
        return cells.FAIL, "no row is shown as forked from the source"
    session = row.get("session_id")
    if not session or session == source_session:
        return cells.FAIL, f"the forked row's session is {session}, not one of its own"
    status, reason = cells.emitter_verdict(herdr_session, kind)
    if status != cells.PASS:
        return status, "the fork's " + reason[0].lower() + reason[1:]
    if (herdr_session or {}).get("value") != session:
        return cells.FAIL, f"Herdr reports session {(herdr_session or {}).get('value')} in the fork, Hide {session}"
    if not listed:
        return cells.FAIL, "hide agent list has no registration for the fork's pane"
    if listed.get("parent") is not None or listed.get("origin") != source_agent:
        return cells.FAIL, (f"the fork is registered with parent {listed.get('parent')} and origin "
                            f"{listed.get('origin')}, not as a root from {source_agent}")
    return cells.PASS, (f"forked from {row['forked_from']['pane_id']} into its own session {session}, a root "
                        f"whose origin is {source_agent}")


def measure(journey):
    if not cells.measurable(journey.table, journey.id, CELL):
        return
    record = {"source": {"pane": journey.pane, "agent": journey.agent}}
    journey.record[CELL] = record
    source_session = (journey.daemon.row(journey.pane) or {}).get("session_id")
    record["source"]["session"] = source_session
    journey.daemon.dispatch("fork_pane", {"pane_id": journey.pane, "placement": "tab"})

    def forked(rest):
        row = forked_row(rest, journey.pane)
        return row if row and row.get("session_id") and row["session_id"] != source_session else None
    try:
        journey.daemon.wait(forked, FORK_SECONDS, "a row forked from the source with a session of its own")
    except ProcessError as error:
        if journey.runtime.owner.cancelled.is_set():
            raise
        record["fork_wait"] = str(error)
    rest = journey.daemon.snapshot() or {}
    row = forked_row(rest, journey.pane)
    # Without the mark, the panes the fork may have opened anyway, for the evidence.
    record["row"] = row or [agent for agent in (rest.get("navigator") or {}).get("agents") or []
                            if agent.get("pane_id") != journey.pane and agent.get("forked_from") is not None]
    pane = (row or {}).get("pane_id")
    herdr_session = (journey.runtime.agent(pane) or {}).get("agent_session") if pane else None
    listing = journey.controller.hide(["agent", "list"])
    listed = next((item for item in (listing.get("value") or {}).get("items") or []
                   if pane and item.get("pane") == pane), None)
    record.update(herdr_session=herdr_session, listed=listed, errors=list(journey.daemon.errors))
    journey.settle(CELL, verdict(row, source_session, herdr_session, journey.kind, listed, journey.agent),
                   journey.evidence(CELL, record))
