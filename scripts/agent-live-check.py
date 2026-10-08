#!/usr/bin/env python3
"""Local authenticated-agent bell measurement. See docs/VERIFICATION.md."""

import sys

from agent_live_check.cli import main
from agent_live_check.protection import ProtectionError

if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ProtectionError as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(2)
