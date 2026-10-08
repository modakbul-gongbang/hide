"""Issue 828: the guardian wakes on its child's exit and never reaps it.

The child's unreaped zombie is what keeps its process group reserved, so the
watch must report the exit and leave the reaping to the guardian.
"""

from pathlib import Path
import subprocess
import sys
import threading
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_live_check.processes import POLL_SECONDS, ExitWatch

# A hang guard, not a measured time: the child exits as soon as it is let go.
HANG_GUARD = 10


@unittest.skipUnless(sys.platform == "darwin" or sys.platform.startswith("linux"),
                     "kqueue and pidfd are the systems the guardian watches with")
class ExitWatchContract(unittest.TestCase):
    def watch_until_exit(self, watch):
        cancelled = threading.Event()
        end = time.monotonic() + HANG_GUARD
        while not watch.wait(POLL_SECONDS, cancelled):
            self.assertLess(time.monotonic(), end, "the exit was never reported")

    def test_an_exit_is_reported_and_the_child_is_left_to_be_reaped(self):
        child = subprocess.Popen(["/bin/sh", "-c", "read line; exit 7"], stdin=subprocess.PIPE)
        watch = ExitWatch(child.pid)
        try:
            self.assertFalse(watch.wait(0, threading.Event()), "a running child has not exited")
            child.stdin.close()
            self.watch_until_exit(watch)
            self.assertIsNone(child.returncode)
            self.assertEqual(child.wait(timeout=HANG_GUARD), 7)
        finally:
            watch.close()

    def test_a_child_that_exited_before_the_watch_began_reads_as_exited(self):
        child = subprocess.Popen(["/bin/sh", "-c", "exit 5"])
        first = ExitWatch(child.pid)
        try:
            self.watch_until_exit(first)
        finally:
            first.close()
        late = ExitWatch(child.pid)
        try:
            self.assertTrue(late.wait(0, threading.Event()))
        finally:
            late.close()
        self.assertEqual(child.wait(timeout=HANG_GUARD), 5)


if __name__ == "__main__":
    unittest.main()
