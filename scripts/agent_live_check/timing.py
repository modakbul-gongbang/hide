"""One absolute observation deadline across reads and resulting input."""

import time

from .processes import COMMAND_SECONDS, ProcessError


class ObservationTimeout(ProcessError):
    """A completed transport no longer supplies timely phase evidence."""


class Deadline:
    def __init__(self, owner, seconds):
        self.end = min(owner.deadline, time.monotonic() + seconds)

    @classmethod
    def observation(cls, owner, seconds):
        # The original observation window excluded guarded transport. Keep a
        # fixed allowance for it, rather than renewing the end on each read.
        return cls(owner, COMMAND_SECONDS + seconds)

    def remaining(self):
        seconds = self.end - time.monotonic()
        if seconds <= 0:
            raise ObservationTimeout("scene_timeout")
        return seconds

    def command_seconds(self):
        return min(COMMAND_SECONDS, self.remaining())

    def expired(self):
        return time.monotonic() >= self.end
