#!/usr/bin/env python3
"""Require listed delivery library contracts to actually pass on each OS.

Four list/run pairs use the existing cargo wrapper, never a file-presence skip.
Logs stay in this checkout's target/delivery-os-contract (32 retained runs max).
The 570s shared deadline reserves cleanup time inside the workflow's 10 minutes.
Windows uses an atomic creation-time kill-on-close job; POSIX uses an EOF
guardian and an owned process group. Neither backend reaches an operator server.
The final delivery source and real host dispatcher regression must be reviewed
before this prospective lane is integrated into a branch carrying the modules.
"""
import ctypes
from dataclasses import dataclass
import errno
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time


ROOT = Path(__file__).resolve().parents[1]
MAX_NAMES = 256
MAX_NAME_BYTES = 512
MAX_OUTPUT_BYTES = 8 * 1024 * 1024
MAX_DIAGNOSTIC_BYTES = 64 * 1024
DIAGNOSTIC_TAIL_BYTES = 8 * 1024
DIAGNOSTIC_RECORD_BYTES = 32 * 1024
MAX_RETAINED_RUNS = 32
MAX_PROCESSES = 32
MAX_PROC_ENTRIES = 65536
MAX_RSS_BYTES = 3 * 1024 ** 3
BUILD_JOBS = 2
TOTAL_SECONDS = 570
CLEANUP_SECONDS = 5
NAME = r"[A-Za-z_][A-Za-z_0-9]*(?:::[A-Za-z_][A-Za-z_0-9]*)*"
LIST_NAME = re.compile(rf"({NAME}): test")
LIST_TOTAL = re.compile(r"(\d+) tests?, (\d+) benchmarks?")
RUN_NAME = re.compile(rf"test ({NAME}) \.\.\. (ok|FAILED|ignored)(?:, .*)?")
RUN_COUNT = re.compile(r"running (\d+) tests?")
RUN_TOTAL = re.compile(
    r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"(\d+) measured; (\d+) filtered out; finished in [0-9]+(?:\.[0-9]+)?s"
)


@dataclass(frozen=True)
class Group:
    package: str
    selection: str
    exact: bool
    required: tuple[str, ...]
    additional_prefixes: tuple[str, ...] = ()

    def accepts(self, name):
        if self.exact:
            return name == self.selection
        return name.startswith((self.selection, *self.additional_prefixes))

    def command(self, listing):
        command = ["bash", "scripts/verify-cargo.sh", "test-scoped", "-p",
                   self.package, "--lib", self.selection, "--"]
        if listing:
            command.append("--list")
        command.extend(["--color", "never"])
        if self.exact:
            command.append("--exact")
        return command


# Names come from committed delivery source, not a successful empty cargo run.
# Module selections also require every additional listed test to pass.
GROUPS = (
    Group("herdr-core", "delivery::", False, (
        "delivery::doorbell::tests::composer_refuses_draft_menu_and_unknown_layout",
        "delivery::doorbell::tests::styled_placeholder_and_bounded_footer_distinguish_actual_composer_states",
        "delivery::ledger::tests::durable_ledger_is_private_and_corruption_is_preserved",
        "delivery::ledger::tests::total_doorbell_reservations_include_success_and_survive_restart_before_confirmation",
        "delivery::ledger::tests::legacy_success_does_not_infer_spare_attempts_and_invalid_budget_is_rejected",
        "delivery::mailbox::tests::same_intent_converges_across_confirmation_cancel_and_restart",
        "delivery::mailbox::tests::intake_is_oldest_five_and_bounded_utf8_without_losing_full_body",
        "delivery::mailbox::tests::expiry_and_capacity_are_explicit_without_eviction",
        "delivery::mailbox::tests::repeated_intent_does_not_require_a_live_target_or_close_another_request",
        "delivery::mailbox::tests::open_and_retained_limits_reject_without_removing_existing_letters",
        "delivery::mailbox::tests::missing_native_identity_cannot_send_read_or_confirm_previous_occupant_mail",
        "delivery::watch::tests::two_warnings_keep_the_first_deadline_across_restart_and_reset_on_activity",
        "delivery::watch::tests::bootstrap_of_unchanged_status_keeps_the_durable_activity_clock",
        "delivery::watch::tests::reply_does_not_stop_watch_and_failed_reads_do_not_suppress_it",
        "delivery::watch::tests::warning_capacity_preserves_other_target_exit_and_activity_reset",
        "delivery::watch::tests::file_only_activity_reset_and_failed_read_start_a_distinct_durable_warning_episode",
        "delivery::watch::tests::missing_target_reference_keeps_status_only_watch_without_mailbox_authority",
        "delivery::worker::tests::success_is_durable_and_a_failed_save_never_publishes_the_letter",
        "delivery::worker::tests::stale_new_recipient_is_refused_while_retained_intent_replays_after_exit",
        "delivery::worker::tests::two_parent_watches_share_one_target_sample_and_refresh_it_next_tick",
        "delivery::worker::tests::uncertain_native_acquisition_or_loss_preserves_unbound_watch_and_original_clocks",
        "delivery::worker::tests::same_native_metadata_acquisition_is_accepted_but_positive_replacement_or_absence_ends_watch",
        "delivery::ledger::tests::validated_startup_reestablishes_the_installed_version_without_repairing_corruption",
        "delivery::ledger::tests::startup_bootstraps_private_sibling_state_and_preserves_refused_or_corrupt_bytes",
        "delivery::worker::tests::unavailable_store_refuses_all_intake_and_effects_until_validated_restart",
        "runtime::delivery::tests::prepared_command_refuses_changed_pane_or_checkout_capability_context_without_saving",
    ), additional_prefixes=("runtime::delivery::",)),
    Group("herdr-core", "wire::tests::delivery_requires_positive_readiness_and_preserves_visible_styling", True, (
        "wire::tests::delivery_requires_positive_readiness_and_preserves_visible_styling",
    )),
    Group("hide-session", "session_activity::", False, (
        "session_activity::tests::both_providers_return_only_mtime_and_size_without_parsing_conversation",
        "session_activity::tests::missing_unsupported_and_outside_references_return_path_free_errors",
        "session_activity::tests::a_reference_without_native_owner_is_not_activity",
        "session_activity::tests::id_lookup_counts_skipped_entries_across_directories_and_refuses_capacity",
        "session_activity::tests::below_capacity_id_lookup_preserves_reported_native_owner_and_fresh_activity",
    )),
    Group("hide-host", "serve::tests::activity_helper_answers_metadata_only_and_refuses_unsupported_references", True, (
        "serve::tests::activity_helper_answers_metadata_only_and_refuses_unsupported_references",
    )),
)


def lines(output):
    if len(output) > MAX_OUTPUT_BYTES:
        raise ValueError("libtest output exceeds the 8 MiB input cap")
    return output.decode("utf-8", errors="strict").splitlines()


def validate_names(names, group):
    if not names or len(names) > MAX_NAMES:
        raise ValueError("selected test count must be between 1 and 256")
    if len(set(names)) != len(names):
        raise ValueError("duplicate selected test name")
    for name in names:
        if not re.fullmatch(NAME, name) or len(name.encode("utf-8")) > MAX_NAME_BYTES:
            raise ValueError("invalid or over-budget test name")
        if not group.accepts(name):
            raise ValueError("listed test is outside the selected contract")
    missing = sorted(set(group.required) - set(names))
    if missing:
        raise ValueError("required tests were not listed: " + ", ".join(missing))
    return tuple(names)


def parse_listing(output, group):
    names, totals = [], []
    for line in lines(output):
        if line.endswith(": benchmark"):
            raise ValueError("benchmarks cannot satisfy a named library test contract")
        match = LIST_NAME.fullmatch(line)
        if match:
            names.append(match[1])
        match = LIST_TOTAL.fullmatch(line)
        if match:
            totals.append((int(match[1]), int(match[2])))
    selected = validate_names(names, group)
    if totals != [(len(selected), 0)]:
        raise ValueError("listing must have one matching test total and zero benchmarks")
    return selected


def parse_execution(output, selected, group):
    selected = validate_names(selected, group)
    statuses, counts, totals = {}, [], []
    for line in lines(output):
        match = RUN_NAME.fullmatch(line)
        if match:
            name, status = match[1], match[2]
            if name in statuses:
                raise ValueError("duplicate execution result")
            statuses[name] = status
        match = RUN_COUNT.fullmatch(line)
        if match:
            counts.append(int(match[1]))
        match = RUN_TOTAL.fullmatch(line)
        if match:
            totals.append((match[1], *(int(value) for value in match.groups()[1:])))
    if set(statuses) != set(selected) or any(value != "ok" for value in statuses.values()):
        raise ValueError("every listed test must appear exactly once with status ok")
    if counts != [len(selected)] or len(totals) != 1:
        raise ValueError("execution must have one matching running count and result summary")
    status, passed, failed, ignored, measured, _filtered = totals[0]
    if (status, passed, failed, ignored, measured) != ("ok", len(selected), 0, 0, 0):
        raise ValueError("result counts differ from listed tests or include failure/ignore")
    return len(selected)


def group_resources(pgid, ignored_pid=None):
    """Sample the group's process count and resident bytes from the kernel.

    RSS is sampled, not a claim about unsampled instantaneous peaks. This
    measurement does not fix the separate detached-descendant ownership gap.
    No process is started to inspect another process.
    """
    if sys.platform == "darwin":
        library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        # proc_listpids returns bytes; proc_listpgrppids returns a PID count.
        query = library.proc_listpids
        query.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_int]
        query.restype = ctypes.c_int
        buffer = (ctypes.c_int * (MAX_PROCESSES + 1))()
        ctypes.set_errno(0)
        size = query(2, pgid, buffer, ctypes.sizeof(buffer))  # PROC_PGRP_ONLY
        if ctypes.get_errno() or size < 0 or size % ctypes.sizeof(ctypes.c_int):
            raise OSError("cannot count owned process group")
        members = [pid for pid in buffer[:size // ctypes.sizeof(ctypes.c_int)]
                   if pid != ignored_pid]
        if len(members) > MAX_PROCESSES:
            return len(members), 0
        info = library.proc_pidinfo
        info.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                         ctypes.c_void_p, ctypes.c_int]
        info.restype = ctypes.c_int
        rss = 0
        for pid in members:
            # proc_taskinfo: six uint64 values followed by twelve int32 values.
            # pti_resident_size is the second uint64, in bytes.
            task = (ctypes.c_uint64 * 12)()
            ctypes.set_errno(0)
            written = info(pid, 4, 0, task, ctypes.sizeof(task))  # PROC_PIDTASKINFO
            if written != ctypes.sizeof(task):
                error = ctypes.get_errno()
                if error == errno.ESRCH:
                    continue  # This sampled member exited during the query.
                raise OSError(error, "cannot measure owned group resident bytes")
            rss += task[1]
        return len(members), rss
    if sys.platform != "linux":
        raise ValueError("unsupported POSIX contract runner")
    count, rss = 0, 0
    page_size = os.sysconf("SC_PAGE_SIZE")
    if page_size <= 0:
        raise ValueError("invalid native page size")
    with os.scandir("/proc") as entries:
        for index, entry in enumerate(entries):
            if index >= MAX_PROC_ENTRIES:
                raise ValueError("process table exceeds bounded query capacity")
            if not entry.name.isdecimal():
                continue
            if int(entry.name) == ignored_pid:
                continue
            try:
                with open(Path(entry.path) / "stat", "rb") as source:
                    data = source.read(4097)
            except FileNotFoundError:
                continue
            if len(data) > 4096:
                raise ValueError("process stat exceeds bounded query capacity")
            fields = data.rsplit(b")", 1)[-1].split()
            if len(fields) < 22:
                raise ValueError("invalid process group information")
            if int(fields[2]) == pgid:
                count += 1
                pages = int(fields[21])
                if pages < 0:
                    raise ValueError("invalid resident page count")
                rss += pages * page_size
                if count > MAX_PROCESSES:
                    return count, rss
    return count, rss


def retained_exit_status(process):
    """Observe exit without releasing the PID that pins our process group.

    Reaping before group cleanup lets that number be reused by another session.
    This protects the group identity; it does not contain detached descendants.
    """
    if sys.platform == "darwin":
        # macOS runner Python builds can lack os.waitid. The native waitid and
        # siginfo_t contract is in the macOS SDK's sys/wait.h and sys/signal.h.
        class SigInfo(ctypes.Structure):
            _fields_ = [("signo", ctypes.c_int), ("error", ctypes.c_int),
                        ("code", ctypes.c_int), ("pid", ctypes.c_int32),
                        ("uid", ctypes.c_uint32), ("status", ctypes.c_int),
                        ("address", ctypes.c_void_p), ("value", ctypes.c_void_p),
                        ("band", ctypes.c_long), ("reserved", ctypes.c_ulong * 7)]
        wait = ctypes.CDLL(None, use_errno=True).waitid
        wait.argtypes = [ctypes.c_int, ctypes.c_uint32,
                         ctypes.POINTER(SigInfo), ctypes.c_int]
        wait.restype = ctypes.c_int
        result = SigInfo()
        if wait(1, process.pid, ctypes.byref(result), 4 | 1 | 32):
            raise OSError(ctypes.get_errno(), "cannot observe owned wrapper exit")
        pid, code, status = result.pid, result.code, result.status
        exited, killed, dumped = 1, 2, 3
    elif sys.platform == "linux":
        result = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        if result is None:
            return None
        pid, code, status = result.si_pid, result.si_code, result.si_status
        exited, killed, dumped = os.CLD_EXITED, os.CLD_KILLED, os.CLD_DUMPED
    else:
        raise RuntimeError("unsupported POSIX exit observation")
    if pid == 0:  # WNOHANG has no exit state yet; the child stays waitable.
        return None
    if pid != process.pid:
        raise RuntimeError("exit observation did not name the owned wrapper")
    if code == exited:
        return status
    if code in (killed, dumped):
        return -status
    raise RuntimeError("unknown owned wrapper exit state")


def end_group(process):
    """Keep the primary unreaped until its group is empty, then reap it."""
    deadline = time.monotonic() + CLEANUP_SECONDS
    while True:
        status = retained_exit_status(process)
        count, _rss = group_resources(process.pid,
                                     process.pid if status is not None else None)
        if status is not None and count == 0:
            break
        # The primary's unreaped PID still belongs to this guardian, so the
        # group number cannot become another session's group during this call.
        os.killpg(process.pid, signal.SIGKILL)
        if time.monotonic() >= deadline:
            raise RuntimeError("owned process group survived cleanup deadline")
        time.sleep(0.05)
    process.wait(timeout=max(0.01, deadline - time.monotonic()))


class PosixOwned:
    def __init__(self, pid, control, stdout):
        self.pid, self.control, self.stdout = pid, control, stdout
        self.control.setblocking(False)
        self.buffer = b""
        self.child_pid = None
        self.returncode = None
        self.error = None
        self.peak = None
        self.peak_rss_bytes = None
        self.measurement_error = None
        self.result_received = False
        self.control_eof = False
        self.cleanup_error = None
        self.memory_scope = "sampled_posix_process_group"
        self.closed = False

    def collect_result(self):
        while True:
            try:
                chunk = self.control.recv(4096)
            except BlockingIOError:
                break
            if not chunk:
                self.control_eof = True
                break
            self.buffer += chunk
            if len(self.buffer) > 4096:
                raise RuntimeError("process guardian response exceeds cap")
            while b"\n" in self.buffer:
                row, self.buffer = self.buffer.split(b"\n", 1)
                record = json.loads(row)
                if "pid" in record:
                    self.child_pid = record["pid"]
                else:
                    self.returncode = record["returncode"]
                    self.error, self.peak = record["error"], record["peak"]
                    self.peak_rss_bytes = record["peak_rss_bytes"]
                    self.cleanup_error = record["cleanup_error"]
                    self.result_received = True

    def poll(self):
        self.collect_result()
        if self.control_eof and not self.result_received:
            raise RuntimeError("process guardian exited without a result")
        if self.error:
            raise RuntimeError(self.error)
        if self.cleanup_error:
            raise RuntimeError(self.cleanup_error)
        return self.returncode

    def close(self):
        if self.closed:
            return
        self.closed = True
        deadline = time.monotonic() + CLEANUP_SECONDS + 1
        failure = None
        collecting = True

        def collect():
            nonlocal collecting
            if collecting:
                try:
                    self.collect_result()
                except BaseException as error:
                    # Measurement failure must not interrupt process release.
                    self.measurement_error = str(error)[:512]
                    collecting = False

        try:
            collect()
            try:
                self.control.shutdown(socket.SHUT_WR)
            except OSError as error:
                # A received terminal result means the guardian has completed
                # cleanup and may already have closed its end of this socket.
                # Reap it below; every other shutdown failure remains visible.
                if error.errno != errno.ENOTCONN:
                    failure = error
            while True:
                collect()
                ended, status = os.waitpid(self.pid, os.WNOHANG)
                if ended:
                    # The final frame can arrive between the last read and
                    # waitpid. Read it after reaping, before closing the pipe.
                    collect()
                    if status != 0:
                        failure = RuntimeError("process guardian failed; command release is unconfirmed")
                    break
                if time.monotonic() >= deadline:
                    # The child's PID may have been reaped by a failing
                    # guardian. Never signal a stale numeric process group.
                    os.kill(self.pid, signal.SIGKILL)
                    os.waitpid(self.pid, 0)
                    raise RuntimeError("process guardian exceeded cleanup deadline; command release is unconfirmed")
                time.sleep(0.05)
            if not self.result_received:
                self.measurement_error = self.measurement_error or "guardian exited without final resource report"
                failure = RuntimeError(self.measurement_error + "; command release is unconfirmed")
            elif self.cleanup_error:
                failure = RuntimeError(self.cleanup_error)
        finally:
            self.control.close()
        if failure is not None:
            raise failure


class WindowsOwned:
    """A non-inheritable job handle owns the process tree from creation.

    PROC_THREAD_ATTRIBUTE_JOB_LIST avoids the spawn/assignment race; the only
    inherited handles are the two explicitly supplied standard-stream handles.
    See Microsoft's UpdateProcThreadAttribute and Job Objects references.
    """
    def __init__(self, api, job, process, stdout, accounting_type,
                 process_ids_type, memory_type):
        self.api, self.job, self.process, self.stdout = api, job, process, stdout
        self.accounting_type = accounting_type
        self.process_ids_type, self.memory_type = process_ids_type, memory_type
        self.peak = None
        self.peak_rss_bytes = None
        self.measurement_error = None
        self.memory_scope = "sampled_windows_job_working_set"

    @classmethod
    def create(cls, command, environment):
        import msvcrt
        c = ctypes
        handle, dword, size_t = c.c_void_p, c.c_uint32, c.c_size_t

        class BasicLimit(c.Structure):
            _fields_ = [("process_time", c.c_int64), ("job_time", c.c_int64),
                        ("flags", dword), ("min_working_set", size_t),
                        ("max_working_set", size_t), ("active_limit", dword),
                        ("affinity", size_t), ("priority", dword), ("scheduling", dword)]

        class ExtendedLimit(c.Structure):
            _fields_ = [("basic", BasicLimit), ("io", c.c_uint64 * 6),
                        ("process_memory", size_t), ("job_memory", size_t),
                        ("peak_process_memory", size_t), ("peak_job_memory", size_t)]

        class Accounting(c.Structure):
            _fields_ = [("times", c.c_int64 * 4), ("faults", dword),
                        ("total", dword), ("active", dword), ("limited", dword)]

        class ProcessIds(c.Structure):
            _fields_ = [("assigned", dword), ("listed", dword),
                        ("pids", size_t * (MAX_PROCESSES + 1))]

        class Memory(c.Structure):
            _fields_ = [("size", dword), ("faults", dword),
                        ("peak_working_set", size_t), ("working_set", size_t),
                        ("peak_paged_pool", size_t), ("paged_pool", size_t),
                        ("peak_nonpaged_pool", size_t), ("nonpaged_pool", size_t),
                        ("pagefile", size_t), ("peak_pagefile", size_t)]

        class Startup(c.Structure):
            _fields_ = [("size", dword), ("reserved", c.c_wchar_p),
                        ("desktop", c.c_wchar_p), ("title", c.c_wchar_p),
                        ("x", dword), ("y", dword), ("x_size", dword), ("y_size", dword),
                        ("x_chars", dword), ("y_chars", dword), ("fill", dword),
                        ("flags", dword), ("show", c.c_uint16), ("reserved_size", c.c_uint16),
                        ("reserved_bytes", c.c_void_p), ("stdin", handle),
                        ("stdout", handle), ("stderr", handle)]

        class StartupEx(c.Structure):
            _fields_ = [("startup", Startup), ("attributes", c.c_void_p)]

        class ProcessInfo(c.Structure):
            _fields_ = [("process", handle), ("thread", handle), ("pid", dword), ("tid", dword)]

        api = c.WinDLL("kernel32", use_last_error=True)
        signatures = {
            "CreateJobObjectW": ([c.c_void_p, c.c_wchar_p], handle),
            "SetInformationJobObject": ([handle, c.c_int, c.c_void_p, dword], c.c_int),
            "QueryInformationJobObject": ([handle, c.c_int, c.c_void_p, dword, c.c_void_p], c.c_int),
            "InitializeProcThreadAttributeList": ([c.c_void_p, dword, dword, c.POINTER(size_t)], c.c_int),
            "UpdateProcThreadAttribute": ([c.c_void_p, dword, size_t, c.c_void_p,
                                           size_t, c.c_void_p, c.c_void_p], c.c_int),
            "DeleteProcThreadAttributeList": ([c.c_void_p], None),
            "CreateProcessW": ([c.c_wchar_p, c.c_wchar_p, c.c_void_p, c.c_void_p,
                                c.c_int, dword, c.c_void_p, c.c_wchar_p,
                                c.c_void_p, c.POINTER(ProcessInfo)], c.c_int),
            "GetExitCodeProcess": ([handle, c.POINTER(dword)], c.c_int),
            "WaitForSingleObject": ([handle, dword], dword),
            "TerminateJobObject": ([handle, dword], c.c_int),
            "OpenProcess": ([dword, c.c_int, dword], handle),
            "IsProcessInJob": ([handle, handle, c.POINTER(c.c_int)], c.c_int),
            "K32GetProcessMemoryInfo": ([handle, c.c_void_p, dword], c.c_int),
            "CloseHandle": ([handle], c.c_int),
        }
        for name, (arguments, result) in signatures.items():
            function = getattr(api, name)
            function.argtypes, function.restype = arguments, result

        def require(value):
            if not value:
                raise c.WinError(c.get_last_error())
            return value

        job = require(api.CreateJobObjectW(None, None))
        info, attributes, initialized = ProcessInfo(), None, False
        read_fd = write_fd = stdin_fd = None
        try:
            limit = ExtendedLimit()
            limit.basic.flags = 0x2000 | 0x0008  # KILL_ON_JOB_CLOSE | ACTIVE_PROCESS
            limit.basic.active_limit = MAX_PROCESSES
            require(api.SetInformationJobObject(job, 9, c.byref(limit), c.sizeof(limit)))
            read_fd, write_fd = os.pipe()
            stdin_fd = os.open(os.devnull, os.O_RDONLY)
            os.set_inheritable(write_fd, True)
            os.set_inheritable(stdin_fd, True)
            inherited = (handle * 2)(msvcrt.get_osfhandle(stdin_fd), msvcrt.get_osfhandle(write_fd))
            jobs = (handle * 1)(job)
            length = size_t()
            api.InitializeProcThreadAttributeList(None, 2, 0, c.byref(length))
            if c.get_last_error() != 122 or not 0 < length.value <= 65536:
                raise RuntimeError("invalid Windows process attribute size")
            attributes = c.create_string_buffer(length.value)
            require(api.InitializeProcThreadAttributeList(attributes, 2, 0, c.byref(length)))
            initialized = True
            # ProcThreadAttributeValue(..., input=True): HANDLE_LIST=2, JOB_LIST=13.
            for key, value in ((0x20002, inherited), (0x2000D, jobs)):
                require(api.UpdateProcThreadAttribute(attributes, 0, key, c.byref(value),
                                                       c.sizeof(value), None, None))
            startup = StartupEx()
            startup.startup.size = c.sizeof(startup)
            startup.startup.flags = 0x100  # STARTF_USESTDHANDLES
            startup.startup.stdin = inherited[0]
            startup.startup.stdout = startup.startup.stderr = inherited[1]
            startup.attributes = c.cast(attributes, c.c_void_p)
            binary = shutil.which(command[0])
            if not binary:
                raise ValueError("verification wrapper shell was not found")
            argv = [binary, *command[1:]]
            text = subprocess.list2cmdline(argv)
            if len(text) >= 32767:
                raise ValueError("verification command exceeds Windows argument cap")
            block = "\0".join(f"{key}={value}" for key, value in sorted(environment.items(),
                                                                        key=lambda item: item[0].upper())) + "\0\0"
            if len(block) > 1024 * 1024:
                raise ValueError("verification environment exceeds 1 MiB character cap")
            environment_block = c.create_unicode_buffer(block)
            command_line = c.create_unicode_buffer(text)
            flags = 0x80000 | 0x400 | 0x08000000  # EXTENDED_STARTUPINFO | UNICODE_ENV | NO_WINDOW
            require(api.CreateProcessW(binary, command_line, None, None, True, flags,
                                       environment_block, str(ROOT), c.byref(startup), c.byref(info)))
            stdout = os.fdopen(read_fd, "rb", buffering=0)
            read_fd = None
            result = cls(api, job, info.process, stdout, Accounting, ProcessIds, Memory)
            job = info.process = None  # Ownership transfers only after the stream is ready.
            return result
        finally:
            if initialized:
                api.DeleteProcThreadAttributeList(attributes)
            for descriptor in (read_fd, write_fd, stdin_fd):
                if descriptor is not None:
                    os.close(descriptor)
            if job:
                api.TerminateJobObject(job, 1)
                api.CloseHandle(job)
            for owned_handle in (info.thread, info.process):
                if owned_handle:
                    api.CloseHandle(owned_handle)

    def accounting(self):
        result = self.accounting_type()
        if not self.api.QueryInformationJobObject(self.job, 1, ctypes.byref(result),
                                                 ctypes.sizeof(result), None):
            raise ctypes.WinError(ctypes.get_last_error())
        self.peak = result.active if self.peak is None else max(self.peak, result.active)
        return result

    def poll(self):
        if self.accounting().limited:
            raise RuntimeError("Windows job exceeded its process cap")
        self.measure_memory()
        status = self.api.WaitForSingleObject(self.process, 0)
        if status == 258:  # WAIT_TIMEOUT
            return None
        if status != 0:
            raise ctypes.WinError(ctypes.get_last_error())
        result = ctypes.c_uint32()
        if not self.api.GetExitCodeProcess(self.process, ctypes.byref(result)):
            raise ctypes.WinError(ctypes.get_last_error())
        return result.value

    def measure_memory(self):
        # Job memory/PeakJobMemoryUsed counts committed virtual memory, not RSS.
        # Use working-set bytes from pinned process handles in this exact job.
        pids = self.process_ids_type()
        if not self.api.QueryInformationJobObject(self.job, 3, ctypes.byref(pids),
                                                 ctypes.sizeof(pids), None):
            raise ctypes.WinError(ctypes.get_last_error())
        if pids.assigned > MAX_PROCESSES or pids.listed > MAX_PROCESSES:
            raise RuntimeError("Windows job process list exceeded its cap")
        if pids.assigned != pids.listed:
            raise RuntimeError("Windows job process list is incomplete")
        rss = 0
        for pid in pids.pids[:pids.listed]:
            # SYNCHRONIZE also permits the exit check after a failed read.
            process = self.api.OpenProcess(0x100000 | 0x1000 | 0x0010, False, pid)
            if not process:
                # A missing handle cannot silently mean zero resident bytes.
                raise ctypes.WinError(ctypes.get_last_error())
            try:
                belongs = ctypes.c_int()
                if not self.api.IsProcessInJob(process, self.job, ctypes.byref(belongs)):
                    raise ctypes.WinError(ctypes.get_last_error())
                if not belongs.value:
                    raise RuntimeError("Windows sampled PID no longer belongs to the owned job")
                memory = self.memory_type()
                memory.size = ctypes.sizeof(memory)
                if not self.api.K32GetProcessMemoryInfo(process, ctypes.byref(memory), memory.size):
                    if self.api.WaitForSingleObject(process, 0) == 0:
                        continue  # A pinned job member ended during this read.
                    raise ctypes.WinError(ctypes.get_last_error())
                rss += memory.working_set
            finally:
                if not self.api.CloseHandle(process):
                    raise ctypes.WinError(ctypes.get_last_error())
        self.peak_rss_bytes = rss if self.peak_rss_bytes is None else max(self.peak_rss_bytes, rss)
        if rss > MAX_RSS_BYTES:
            raise RuntimeError("owned Windows job exceeded the 3 GiB sampled RSS cap")

    def close(self):
        if not self.job:
            return
        try:
            try:
                # An output-reader failure can precede the caller's first
                # poll. Capture the owned job before terminating its members.
                self.accounting()
                self.measure_memory()
            except BaseException as error:
                self.measurement_error = str(error)[:512]
            if not self.api.TerminateJobObject(self.job, 1):
                raise ctypes.WinError(ctypes.get_last_error())
            deadline = time.monotonic() + CLEANUP_SECONDS
            if self.api.WaitForSingleObject(self.process, int(CLEANUP_SECONDS * 1000)) != 0:
                raise RuntimeError("Windows wrapper survived cleanup deadline")
            if not self.api.CloseHandle(self.process):
                raise ctypes.WinError(ctypes.get_last_error())
            self.process = None
            while self.accounting().active:
                if time.monotonic() >= deadline:
                    raise RuntimeError("Windows job survived cleanup deadline")
                time.sleep(0.05)
            if self.measurement_error:
                raise RuntimeError(self.measurement_error)
        finally:
            if self.process:
                self.api.CloseHandle(self.process)
            self.api.CloseHandle(self.job)
            self.process = self.job = None


def interrupted(_signum, _frame):
    raise RuntimeError("contract check cancelled")


def spawn_owned(command, environment, deadline):
    """The sole process-start boundary; no retries or concurrent cargo calls."""
    if os.name == "nt":
        return WindowsOwned.create(command, environment)
    parent, child = socket.socketpair()
    try:
        read_fd, write_fd = os.pipe()
    except BaseException:
        parent.close()
        child.close()
        raise
    try:
        pid = os.fork()
    except BaseException:
        parent.close()
        child.close()
        os.close(read_fd)
        os.close(write_fd)
        raise
    if pid:
        child.close()
        os.close(write_fd)
        stream = None
        try:
            stream = os.fdopen(read_fd, "rb", buffering=0)
            result = PosixOwned(pid, parent, stream)
        except BaseException:
            # No command starts before ownership of both streams is complete.
            # EOF releases the waiting guardian even if wrapping stdout failed.
            parent.close()
            if stream is not None:
                stream.close()
            else:
                os.close(read_fd)
            until = time.monotonic() + CLEANUP_SECONDS + 1
            while not os.waitpid(pid, os.WNOHANG)[0]:
                if time.monotonic() >= until:
                    os.kill(pid, signal.SIGKILL)
                    os.waitpid(pid, 0)
                    raise RuntimeError("failed ownership transfer; waiting guardian was killed and reaped")
                time.sleep(0.05)
            raise
        try:
            parent.sendall(b"S")
        except BaseException:
            result.close()
            stream.close()
            raise
        return result
    process = None
    error, cleanup_error, returncode, peak, peak_rss = None, None, 1, None, None
    try:
        parent.close()
        os.close(read_fd)
        signal.signal(signal.SIGINT, interrupted)
        signal.signal(signal.SIGTERM, interrupted)
        # The parent may fail while creating its stream or owner handle. Until
        # its start message arrives this guardian owns no external command.
        ready, _, _ = select.select([child], [], [], max(0, deadline - time.monotonic()))
        if not ready or child.recv(1) != b"S":
            raise RuntimeError("command ownership was not transferred")
        process = subprocess.Popen(command, cwd=ROOT, env=environment,
                                   stdin=subprocess.DEVNULL, stdout=write_fd,
                                   stderr=subprocess.STDOUT, start_new_session=True,
                                   close_fds=True)
        child.sendall(json.dumps({"pid": process.pid}).encode() + b"\n")
        while (status := retained_exit_status(process)) is None:
            count, rss = group_resources(process.pid)
            peak = count if peak is None else max(peak, count)
            peak_rss = rss if peak_rss is None else max(peak_rss, rss)
            if count > MAX_PROCESSES:
                raise RuntimeError("owned process group exceeded 32 processes")
            if rss > MAX_RSS_BYTES:
                raise RuntimeError("owned process group exceeded the 3 GiB sampled RSS cap")
            if time.monotonic() >= deadline:
                raise RuntimeError("contract check exceeded shared 570s deadline")
            ready, _, _ = select.select([child], [], [], 0.1)
            if ready and not child.recv(1):
                raise RuntimeError("contract check owner exited")
        returncode = status
        if group_resources(process.pid, process.pid)[0]:
            raise RuntimeError("wrapper exited with owned descendants still present")
    except BaseException as failure:
        error = str(failure)[:512]
    finally:
        # Cancellation cannot interrupt the cleanup that owns this group.
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        if process is not None:
            try:
                end_group(process)
            except BaseException as failure:
                cleanup_error = "cleanup failed: " + str(failure)[:512]
        try:
            try:
                child.sendall(json.dumps({"returncode": returncode, "error": error,
                                          "cleanup_error": cleanup_error,
                                          "peak": peak, "peak_rss_bytes": peak_rss}).encode() + b"\n")
            except OSError:
                pass  # The owner is gone; owned group cleanup was attempted above.
            child.close()
            os.close(write_fd)
        finally:
            # A forked guardian must never unwind into the parent's main/receipt.
            os._exit(1 if cleanup_error else 0)


def resource_record(owned):
    measured = owned.peak is not None and owned.peak_rss_bytes is not None
    unavailable = owned.measurement_error
    if not measured and unavailable is None:
        unavailable = "no complete resource sample was collected"
    return {"peak_owned_processes": owned.peak,
            "peak_sampled_rss_bytes": owned.peak_rss_bytes,
            "memory_scope": owned.memory_scope,
            "measurement_status": ("available" if measured and not unavailable else
                                   "partial" if measured else "unavailable"),
            "measurement_error": unavailable}


def run_command(command, deadline, log):
    environment = {key: value for key, value in os.environ.items()
                   if key.upper() != "CARGO_BUILD_JOBS"}
    # This is a CI resource contract, not an operator setting or secret.
    environment["CARGO_BUILD_JOBS"] = str(BUILD_JOBS)
    owned = None
    reader = None
    finished = threading.Event()
    output, failures = bytearray(), []
    started = time.monotonic()
    destination = None
    cost = None
    original_failure = None
    cleanup_failures = []
    try:
        if time.monotonic() >= deadline:
            raise RuntimeError("contract check exceeded shared 570s deadline before spawn")
        destination = log.open("xb")
        owned = spawn_owned(command, environment, deadline)

        def read_output():
            try:
                while chunk := os.read(owned.stdout.fileno(), 32768):
                    room = MAX_OUTPUT_BYTES - len(output)
                    destination.write(chunk[:room])
                    output.extend(chunk[:room])
                    if len(chunk) > room:
                        raise ValueError("command output exceeds the 8 MiB cap")
            except BaseException as failure:
                failures.append(failure)
            finally:
                finished.set()

        reader = threading.Thread(target=read_output, daemon=True)
        reader.start()
        while True:
            if failures:
                raise failures[0]
            returncode = owned.poll()
            if returncode is not None and finished.is_set():
                if failures:
                    raise failures[0]
                if returncode:
                    raise RuntimeError(f"verification wrapper exited {returncode}")
                cost = {"output_bytes": len(output)}
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("contract check exceeded shared 570s deadline")
            time.sleep(0.05)
    except BaseException as failure:
        original_failure = failure
    finally:
        try:
            if owned is not None:
                owned.close()
        except BaseException as failure:
            cleanup_failures.append(str(failure)[:512])
        if reader is not None:
            reader.join(CLEANUP_SECONDS)
            if reader.is_alive():
                cleanup_failures.append("owned output reader survived cleanup deadline")
        for stream in (None if owned is None else owned.stdout, destination):
            if stream is not None:
                try:
                    stream.close()
                except BaseException as failure:
                    cleanup_failures.append(str(failure)[:512])
    if original_failure is not None or cleanup_failures:
        if owned is not None:
            print(json.dumps({"event": "ci.delivery_command_resources", "status": "fail",
                              "seconds": round(time.monotonic() - started, 3),
                              **resource_record(owned)}), file=sys.stderr)
        # Preserve the original failure even if release also fails. UTF-8
        # replacement can expand bytes threefold: 8 KiB raw + 32 KiB JSON
        # and the fixed outcome text stay below the total 64 KiB budget.
        print(output[-DIAGNOSTIC_TAIL_BYTES:].decode("utf-8", errors="replace"), file=sys.stderr)
        reasons = ([str(original_failure)[:512]] if original_failure is not None else [])
        reasons.extend("cleanup: " + value for value in cleanup_failures)
        raise RuntimeError("; ".join(reasons)) from original_failure
    cost.update(resource_record(owned))
    cost["seconds"] = round(time.monotonic() - started, 3)  # Includes exit cleanup.
    return bytes(output), cost


def diagnostic_record(record):
    # Full selected names stay in bounded logs/result.json. CLI receipts carry
    # their digest so success/failure output never grows with all selected names.
    result = {**record, "groups": [{key: value for key, value in group.items()
                                    if key != "names"} for group in record["groups"]]}
    text = json.dumps(result)
    if len(text.encode("utf-8")) > DIAGNOSTIC_RECORD_BYTES:
        raise ValueError("contract diagnostic record exceeds 32 KiB cap")
    return text


def run_directory():
    target = ROOT / "target"
    target.mkdir(exist_ok=True)
    if target.resolve() != target or target.is_symlink():
        raise ValueError("contract outputs must stay in this checkout's own target")
    parent = target / "delivery-os-contract"
    parent.mkdir(exist_ok=True)
    if parent.resolve() != parent or parent.is_symlink():
        raise ValueError("contract output directory must not redirect outside target")
    lock_path = parent / ".lock"
    if lock_path.is_symlink():
        raise ValueError("contract lock must not redirect outside target")
    descriptor = os.open(lock_path, os.O_CREAT | os.O_RDWR | getattr(os, "O_NOFOLLOW", 0), 0o600)
    fence = os.fdopen(descriptor, "r+b", buffering=0)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > 1:
            raise ValueError("invalid contract output lock")
        if metadata.st_size == 0:
            fence.write(b"\0")
            fence.seek(0)
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(descriptor, msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        # Hold the OS-released lock for the entire gate, including cleanup.
        # Concurrent invocations fail instead of racing past the retention cap.
        count = 0
        with os.scandir(parent) as entries:
            for entry in entries:
                if entry.name == ".lock":
                    continue
                if not entry.name.startswith("run-") or not entry.is_dir(follow_symlinks=False):
                    raise ValueError("unexpected entry in owned contract output directory")
                count += 1
                if count >= MAX_RETAINED_RUNS:
                    raise ValueError("32 contract runs retained; archive owned logs before another run")
        return Path(tempfile.mkdtemp(prefix="run-", dir=parent)), fence
    except BaseException:
        fence.close()
        raise


def main():
    if len(sys.argv) != 1:
        print("error: this gate accepts no test-filter or deadline overrides", file=sys.stderr)
        return 2
    previous = {}
    run = None
    fence = None
    record = {"event": "ci.delivery_os_contract", "status": "fail", "groups": [],
              "invocations": 0, "build_jobs": BUILD_JOBS,
              "deadline_seconds": TOTAL_SECONDS, "process_cap": MAX_PROCESSES,
              "sampled_rss_bytes_cap": MAX_RSS_BYTES,
              "input_bytes_per_command_cap": MAX_OUTPUT_BYTES,
              "selected_names_per_group_cap": MAX_NAMES}
    try:
        for sig in (signal.SIGINT, signal.SIGTERM):
            previous[sig] = signal.signal(sig, interrupted)
        if not shutil.which("bash"):
            raise ValueError("bash required by the existing verification wrapper was not found")
        run, fence = run_directory()
        record["correlation_id"] = run.name
        deadline = time.monotonic() + TOTAL_SECONDS
        for index, group in enumerate(GROUPS):
            record["invocations"] += 1
            listed, list_cost = run_command(group.command(True), deadline, run / f"{index}-list.log")
            names = parse_listing(listed, group)
            record["invocations"] += 1
            executed, run_cost = run_command(group.command(False), deadline, run / f"{index}-run.log")
            passed = parse_execution(executed, names, group)
            record["groups"].append({"subject_id": f"{group.package}:{group.selection}",
                                     "listed": len(names), "passed": passed,
                                     "names": names,
                                     "names_sha256": hashlib.sha256("\n".join(names).encode()).hexdigest(),
                                     "list_cost": list_cost, "run_cost": run_cost})
        record["status"] = "pass"
        print(diagnostic_record(record))
        return 0
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as failure:
        record["error"] = str(failure)[:1024]
        print(diagnostic_record(record), file=sys.stderr)
        print("error: delivery contracts failed; fix the source or inspect the owned run logs", file=sys.stderr)
        return 1
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)
        try:
            if run is not None:
                (run / "result.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
        finally:
            if fence is not None:
                fence.close()


if __name__ == "__main__":
    sys.exit(main())
