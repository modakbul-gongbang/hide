#!/usr/bin/env python3
"""Local authenticated-agent measurement of letters delivered while the agent works. See docs/PERFORMANCE_TESTING.md."""

import sys

from agent_live_check.mid_turn import main
from agent_live_check.protection import ProtectionError

if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ProtectionError as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(2)
