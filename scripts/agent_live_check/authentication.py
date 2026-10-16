"""Authentication menus are terminal outcomes, never bell scenes."""

import re

from .processes import ProcessError


class AuthenticationRequired(ProcessError):
    pass


_WALL = re.compile(r"\b(?:sign[ -]?in|log[ -]?in|login required|authenticate|authentication required|not logged in)\b",
                   re.IGNORECASE)
# A provider row an agent marks as signed in (omp: "● logged in (login)").
_SIGNED_IN_ROW = re.compile(r"●\s*logged in", re.IGNORECASE)
# A hint that names the login command without asking for it (Pi: "Use /login to add providers.").
_HINT = re.compile(r"/log[ -]?in\b", re.IGNORECASE)


def require_no_login(screen: str) -> None:
    # A provider chooser that lists an account as signed in is a setup menu the
    # scene deadline reports as it is, not a login wall: calling it
    # unauthenticated would skip every scene of an agent that is logged in.
    if _SIGNED_IN_ROW.search(screen):
        return
    text = "\n".join(line for line in screen.splitlines() if not _HINT.search(line))
    if _WALL.search(text):
        raise AuthenticationRequired("not_authenticated_no_login_attempted")
