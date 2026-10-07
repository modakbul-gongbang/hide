"""One child-start boundary, with an EOF guardian for external processes."""

import ctypes
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import threading
import time

if __package__:
    from .process_table import descendants, snapshot
else:
    from process_table import descendants, snapshot

MAX_CHILDREN = 16
MAX_DESCENDANTS = 128
MAX_RSS_BYTES = 2 * 1024 * 1024 * 1024
MAX_OUTPUT = 1024 * 1024
COMMAND_SECONDS = 15
RUN_SECONDS = 30 * 60
POLL_SECONDS = 0.1


class ProcessError(RuntimeError):
    pass


class OwnedProcesses:
    """Guardians end children on pipe EOF, including abrupt owner death.

    The driver still owns the corresponding protocol close. A guardian is the
    final containment boundary, not a substitute for stopping a server.
    """

    def __init__(self, cancelled: threading.Event | None = None):
        self.children = {}
        self.cancelled = cancelled or threading.Event()
        self.deadline = time.monotonic() + RUN_SECONDS

    def spawn(self, argv, *, env, cwd=None, stdin=subprocess.DEVNULL,
              stdout=subprocess.PIPE, stderr=subprocess.PIPE, _guarded=True):
        for child in list(self.children):
            if child.poll() is not None:
                self.end(child)
        if len(self.children) >= MAX_CHILDREN:
            raise ProcessError("child_count_over_budget")
        if self.cancelled.is_set() or time.monotonic() >= self.deadline:
            raise ProcessError("run_cancelled_or_timed_out")
        reader = writer = None
        command = argv
        if _guarded:
            reader, writer = os.pipe()
            command = [sys.executable, str(Path(__file__).resolve()),
                       "--guard", str(reader), "--", *map(str, argv)]
        try:
            # The only raw spawn. Only the guardian uses the unguarded branch.
            child = subprocess.Popen(command, env=env, cwd=cwd, stdin=stdin,
                                     stdout=stdout, stderr=stderr,
                                     start_new_session=True,
                                     pass_fds=(() if reader is None else (reader,)))
        except BaseException:
            if writer is not None:
                os.close(writer)
            raise
        finally:
            if reader is not None:
                os.close(reader)
        self.children[child] = writer
        return child

    def run(self, argv, *, env, cwd=None, seconds=COMMAND_SECONDS, check=True):
        child = self.spawn(argv, env=env, cwd=cwd)
        output = [bytearray(), bytearray()]
        end = min(self.deadline, time.monotonic() + seconds)
        try:
            with selectors.DefaultSelector() as selector:
                for index, stream in enumerate((child.stdout, child.stderr)):
                    os.set_blocking(stream.fileno(), False)
                    selector.register(stream, selectors.EVENT_READ, index)
                while selector.get_map():
                    if self.cancelled.is_set():
                        raise ProcessError("run_cancelled")
                    if time.monotonic() >= end:
                        raise ProcessError("command_timeout")
                    for key, _ in selector.select(POLL_SECONDS):
                        data = os.read(key.fileobj.fileno(), 65536)
                        if not data:
                            selector.unregister(key.fileobj)
                        else:
                            output[key.data].extend(data)
                            if sum(map(len, output)) > MAX_OUTPUT:
                                raise ProcessError("command_output_over_budget")
            code = child.wait(timeout=max(0.01, end - time.monotonic()))
            if check and code:
                raise ProcessError(f"command_exit_{code}")
            return code, *(bytes(value).decode("utf-8", errors="replace") for value in output)
        finally:
            self.end(child)
            child.stdout.close()
            child.stderr.close()

    def end(self, child):
        writer = self.children.pop(child, None)
        if writer is not None:
            os.close(writer)
        if child.poll() is None:
            if writer is None:
                child.send_signal(signal.SIGCONT)
                child.terminate()
            try:
                child.wait(timeout=4)
            except subprocess.TimeoutExpired as error:
                # Killing the guardian here would discard its ownership proof.
                self.children[child] = None
                raise ProcessError("cleanup_unconfirmed") from error
        if writer is not None and (child.returncode == 125 or child.returncode < 0):
            raise ProcessError("guardian_cleanup_or_resource_failure")

    def usage(self):
        table = snapshot()
        current = {}
        for child in self.children:
            if child.poll() is None:
                current.update(descendants(table, child.pid))
        if any(item.rss < 0 and not item.zombie for item in current.values()):
            raise ProcessError("descendant_rss_unavailable")
        measured = {"descendants": len(current),
                    "rss_bytes": sum(max(0, item.rss) for item in current.values()),
                    "in_flight": len(self.children)}
        if (measured["descendants"] > MAX_CHILDREN * (MAX_DESCENDANTS + 1)
                or measured["rss_bytes"] > MAX_CHILDREN * MAX_RSS_BYTES):
            raise ProcessError("owned_processes_over_budget")
        return measured

    def close(self):
        failures = []
        for child in list(self.children)[::-1]:
            try:
                self.end(child)
            except (OSError, ProcessError) as error:
                failures.append(type(error).__name__)
        if failures:
            raise ProcessError("cleanup_unconfirmed: " + ",".join(failures))

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def guard(reader: int, argv: list[str]) -> int:
    cancelled = threading.Event()
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, lambda *_: cancelled.set())
    if sys.platform.startswith("linux"):
        libc = ctypes.CDLL(None, use_errno=True)
        if libc.prctl(36, 1, 0, 0, 0) != 0:  # PR_SET_CHILD_SUBREAPER
            return 125

    def watch_owner():
        try:
            os.read(reader, 1)
        finally:
            os.close(reader)
            cancelled.set()

    threading.Thread(target=watch_owner, daemon=True).start()
    owner = OwnedProcesses(cancelled)
    child = None
    observed = {}
    failed = False
    deadline = time.monotonic() + RUN_SECONDS
    try:
        child = owner.spawn(argv, env=dict(os.environ), stdin=None, stdout=None,
                            stderr=None, _guarded=False)
        while not cancelled.is_set():
            current = descendants(snapshot(), os.getpid())
            current.pop(os.getpid(), None)
            observed.update(current)
            if (any(p.rss < 0 and not p.zombie for p in current.values())
                    or len(current) > MAX_DESCENDANTS
                    or sum(max(0, p.rss) for p in current.values()) > MAX_RSS_BYTES):
                failed = True
                break
            if time.monotonic() >= deadline:
                failed = True
                break
            if child.poll() is not None:
                break
            cancelled.wait(POLL_SECONDS)
    except BaseException:
        failed = True
    finally:
        try:
            current = descendants(snapshot(), os.getpid())
            current.pop(os.getpid(), None)
            observed.update(current)
            for signum in (signal.SIGCONT, signal.SIGTERM, signal.SIGKILL):
                table = snapshot()
                for pid, identity in observed.items():
                    actual = table.get(pid)
                    if actual and actual.birth == identity.birth and not actual.zombie:
                        try:
                            os.kill(pid, signum)
                        except ProcessLookupError:
                            pass
                if signum != signal.SIGKILL:
                    time.sleep(POLL_SECONDS)
            if child is not None:
                child.wait(timeout=2)
            end = time.monotonic() + 2
            while True:
                table = snapshot()
                survivors = [pid for pid, item in observed.items()
                             if pid in table and table[pid].birth == item.birth
                             and not table[pid].zombie]
                if not survivors:
                    break
                if time.monotonic() >= end:
                    failed = True
                    break
                time.sleep(POLL_SECONDS)
        except BaseException:
            failed = True
    return 125 if failed else (child.returncode if child is not None else 125)


if __name__ == "__main__":
    if len(sys.argv) < 5 or sys.argv[1] != "--guard" or sys.argv[3] != "--":
        raise SystemExit(2)
    raise SystemExit(guard(int(sys.argv[2]), sys.argv[4:]))
