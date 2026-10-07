"""One child-start boundary, with an EOF guardian for external processes."""

import ctypes
import hashlib
import json
import os
from pathlib import Path
import selectors
import secrets
import signal
import subprocess
import sys
import threading
import time

if __package__:
    from .process_table import darwin_candidate_current, descendants, marked_descendants, require_complete, snapshot
else:
    from process_table import darwin_candidate_current, descendants, marked_descendants, require_complete, snapshot

MAX_CHILDREN = 16
MAX_DESCENDANTS = 128
MAX_RSS_BYTES = 2 * 1024 * 1024 * 1024
MAX_OUTPUT = 1024 * 1024
MAX_FAILURE_REASON = 512
MAX_COMMANDS = 4096
COMMAND_SECONDS = 15
RUN_SECONDS = 30 * 60
POLL_SECONDS = 0.1


class ProcessError(RuntimeError):
    pass


def linux_children_remain():
    # After Popen's direct child has been waited, only owned adopted children
    # remain. __WALL includes clone children with a non-SIGCHLD exit signal.
    # ECHILD is the kernel's no-child proof; an empty sampled table is not.
    for _ in range(MAX_DESCENDANTS + 1):
        try:
            pid, _ = os.waitpid(-1, os.WNOHANG | 0x40000000)
        except ChildProcessError:
            return False
        if pid == 0:
            return True
    raise ProcessError("adopted_child_reap_over_budget")


class OwnedProcesses:
    """Guardians end children on pipe EOF, including abrupt owner death.

    The driver still owns the corresponding protocol close. A guardian is the
    final containment boundary, not a substitute for stopping a server.
    """

    def __init__(self, cancelled: threading.Event | None = None, diagnostics: Path | None = None):
        self.children = {}
        self.diagnostics = diagnostics
        self.sequence = 0
        if diagnostics is not None:
            diagnostics.mkdir(mode=0o700)
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
            diagnostic = ""
            if self.diagnostics is not None:
                if self.sequence >= MAX_COMMANDS:
                    os.close(reader)
                    os.close(writer)
                    raise ProcessError("command_count_over_budget")
                self.sequence += 1
                diagnostic = str(self.diagnostics / (str(self.sequence) + ".json"))
            command = [sys.executable, str(Path(__file__).resolve()),
                       "--guard", str(reader), diagnostic, "--", *map(str, argv)]
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
            try:
                try:
                    self.end(child)
                except ProcessError as error:
                    reasons = [line[:MAX_FAILURE_REASON] for line in output[1].decode("utf-8", errors="replace").splitlines()
                               if line.startswith(("guardian_failure:", "guardian_cleanup_failure:",
                                                   "guardian_orphan_scan_failure:", "guardian_signal_failure:"))]
                    raise ProcessError(str(error) + (": " + "; ".join(reasons[:4]) if reasons else "")) from error
            finally:
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
        # Group guardians enforce caps; a host-wide partial view is only a
        # sampled resource summary, never an ownership or cleanup verdict.
        current = {}
        for child in self.children:
            if child.poll() is None:
                current.update(descendants(table, child.pid))
        if any(item.rss < 0 and not item.zombie for item in current.values()):
            raise ProcessError("descendant_rss_unavailable")
        measured = {"descendants": len(current),
                    "rss_bytes": sum(max(0, item.rss) for item in current.values()),
                    "in_flight": len(self.children), "sampled": True}
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

    def attribution_report(self):
        records = {}
        omitted = False
        if self.diagnostics is not None:
            for path in self.diagnostics.iterdir():
                with path.open("rb") as stream:
                    data = stream.read(MAX_OUTPUT + 1)
                if len(data) > MAX_OUTPUT:
                    raise ProcessError("process_diagnostic_over_budget")
                for item in json.loads(data)["unattributed"]:
                    key = (item["pid"], item["birth"])
                    if key not in records and len(records) >= MAX_DESCENDANTS:
                        omitted = True
                    else:
                        records[key] = item
        return {"status": "출처 확인 못 함", "processes": list(records.values()),
                "limit": MAX_DESCENDANTS, "additional_records_omitted": omitted,
                "limitation": "An unseen double-fork descendant that clears its marker and leaves the owned group may escape attribution."}

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def group_exists(group):
    try:
        os.killpg(group, 0)
        return True
    except ProcessLookupError:
        return False


def guard(reader: int, argv: list[str], diagnostic: str = "") -> int:
    cancelled = threading.Event()
    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(signum, lambda *_: cancelled.set())
    # Keep the native root unreaped until all group signals are finished. Its
    # reserved PID also reserves the PGID, including after the root exits.
    signal.signal(signal.SIGCHLD, signal.SIG_DFL)
    if sys.platform.startswith("linux"):
        libc = ctypes.CDLL(None, use_errno=True)
        if libc.prctl(36, 1, 0, 0, 0) != 0:
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
    group = None
    observed = {}
    unattributed = {}
    failed = False
    confirmed = False
    scope = hashlib.sha256(str(Path(__file__).resolve()).encode()).hexdigest()[:16]
    initial = snapshot()
    identity = initial[os.getpid()]
    marker = f"{scope}:{identity.pid}:{identity.birth}:{secrets.token_hex(32)}"
    deadline = time.monotonic() + RUN_SECONDS

    def unknown(process):
        key = (process.pid, process.birth)
        if key not in unattributed and len(unattributed) < MAX_DESCENDANTS:
            unattributed[key] = {"pid": process.pid, "birth": process.birth,
                                 "executable": process.name[:32]}

    def collect():
        table = snapshot(group)
        require_complete(table)
        # Never infer ownership from an unreadable host orphan. A readable
        # token from a dead guardian in this checkout proves an earlier run;
        # another live guardian's token belongs to concurrent work.
        if sys.platform == "darwin" or sys.platform.startswith("linux"):
            all_processes = snapshot()
            if any(subject["pid"] in observed for subject in all_processes.unavailable):
                raise ProcessError("known_owned_metadata_unavailable")

            def matches(entry):
                prefix = ("HIDE_LIVE_CHECK_OWNER=" + scope + ":").encode()
                if not entry.startswith(prefix):
                    return False
                value = entry.split(b"=", 1)[1].decode("ascii", errors="replace")
                if value == marker:
                    return True
                parts = value.split(":")
                if len(parts) != 4 or not parts[1].isdecimal() or not parts[2].isdecimal():
                    return False
                guardian_pid, birth = int(parts[1]), int(parts[2])
                guardian = all_processes.get(guardian_pid)
                if guardian is not None:
                    return guardian.birth != birth
                try:
                    os.kill(guardian_pid, 0)
                except ProcessLookupError:
                    return True
                return False

            extras = marked_descendants(all_processes, matches, known=observed,
                                        unknown=unknown)
            table.update(extras)
            # Retain an earlier token proof while that exact birth remains.
            for pid, previous in observed.items():
                actual = all_processes.get(pid)
                if actual and actual.birth == previous.birth:
                    table[pid] = actual
        observed.update(table)
        return table

    def signal_owned(signum):
        if group is not None:
            try:
                os.killpg(group, signum)
            except ProcessLookupError:
                pass
        # Group signals are atomic with respect to membership. Escaped marked
        # helpers require a fresh birth check before each individual signal.
        table = collect()
        for process in table.values():
            if process.group == group or process.zombie:
                continue
            actual = (process if sys.platform == "darwin" and darwin_candidate_current(process)
                      else snapshot().get(process.pid) if sys.platform.startswith("linux") else None)
            if actual and actual.birth == process.birth:
                try:
                    os.kill(process.pid, signum)
                except ProcessLookupError:
                    pass

    try:
        child = owner.spawn(argv, env={**os.environ, "HIDE_LIVE_CHECK_OWNER": marker},
                            stdin=None, stdout=None, stderr=None, _guarded=False)
        group = child.pid
        if group <= 1 or group == os.getpgrp() or os.getpgid(child.pid) != group:
            raise ProcessError("owned_group_unconfirmed")
        while not cancelled.is_set():
            table = collect()
            if (any(p.rss < 0 and not p.zombie for p in table.values())
                    or len(table) > MAX_DESCENDANTS
                    or sum(max(0, p.rss) for p in table.values()) > MAX_RSS_BYTES):
                raise ProcessError("owned_processes_over_budget")
            root = table.get(child.pid)
            if root is None:
                raise ProcessError("unreaped_owned_root_missing")
            if root.zombie:
                break
            if time.monotonic() >= deadline:
                raise ProcessError("guardian_run_timeout")
            cancelled.wait(POLL_SECONDS)
    except BaseException as error:
        sys.stderr.write("guardian_failure:" + type(error).__name__ + ":" + str(error) + "\n")
        failed = True
    finally:
        try:
            if child is not None:
                try:
                    current = collect()
                    active = any(not process.zombie for process in current.values())
                except BaseException:
                    active = True
                    failed = True
                if active:
                    # A table error must not prevent ending the reserved group.
                    for signum in (signal.SIGCONT, signal.SIGTERM):
                        try:
                            signal_owned(signum)
                        except BaseException as error:
                            failed = True
                            sys.stderr.write("guardian_signal_failure:" + type(error).__name__ + ":" + str(error) + "\n")
                    end = time.monotonic() + 0.5
                    while time.monotonic() < end:
                        try:
                            if not any(not p.zombie for p in collect().values()):
                                break
                        except BaseException:
                            failed = True
                            break
                        time.sleep(POLL_SECONDS)
                    try:
                        signal_owned(signal.SIGKILL)
                    except BaseException as error:
                        failed = True
                        sys.stderr.write("guardian_signal_failure:" + type(error).__name__ + ":" + str(error) + "\n")
                child.wait(timeout=1)
                # No group signal after wait: a future reused PGID is not ours.
                end = time.monotonic() + 2
                while True:
                    if sys.platform.startswith("linux"):
                        linux_children_remain()
                    remaining = collect()
                    live_extras = [p for p in remaining.values() if p.group != group and not p.zombie]
                    for process in live_extras:
                        actual = (process if sys.platform == "darwin" and darwin_candidate_current(process)
                                  else snapshot().get(process.pid) if sys.platform.startswith("linux") else None)
                        if actual and actual.birth == process.birth:
                            try:
                                os.kill(process.pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                    if not group_exists(group) and not any(p.group != group for p in remaining.values()):
                        confirmed = True
                        break
                    if time.monotonic() >= end:
                        raise ProcessError("owned_processes_remain")
                    time.sleep(POLL_SECONDS)
            else:
                confirmed = True
        except BaseException as error:
            sys.stderr.write("guardian_cleanup_failure:" + type(error).__name__ + ":" + str(error) + "\n")
            failed = True
        if diagnostic:
            with open(diagnostic, "x", encoding="utf-8", opener=lambda path, flags: os.open(path, flags, 0o600)) as stream:
                json.dump({"confirmed": confirmed, "unattributed": list(unattributed.values()),
                           "unattributed_limit": MAX_DESCENDANTS,
                           "limitation": "An unseen double-fork descendant that clears its marker and leaves the owned group may escape attribution."}, stream)
    return 125 if failed or not confirmed else (child.returncode if child is not None else 125)


if __name__ == "__main__":
    if len(sys.argv) < 6 or sys.argv[1] != "--guard" or sys.argv[4] != "--":
        raise SystemExit(2)
    raise SystemExit(guard(int(sys.argv[2]), sys.argv[5:], sys.argv[3]))
