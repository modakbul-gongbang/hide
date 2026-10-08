"""Authentication menus are terminal outcomes, never bell scenes."""

import re

from .processes import ProcessError


class AuthenticationRequired(ProcessError):
    pass


def require_no_login(screen: str) -> None:
    if re.search(r"\b(?:sign[ -]?in|log[ -]?in|login required|authenticate|authentication required|not logged in)\b",
                 screen, flags=re.IGNORECASE):
        raise AuthenticationRequired("not_authenticated_no_login_attempted")
