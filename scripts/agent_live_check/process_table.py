"""Process identities and RSS, without subprocesses or inspecting argv.

Darwin layouts follow sys/proc_info.h in the installed SDK. Linux uses proc(5).
"""

import ctypes
import errno
import json
from dataclasses import dataclass
import os
from pathlib import Path
import sys

MAX_SYSTEM_PROCESSES = 65_536
MAX_PROC_CONTEXT_BYTES = 1024 * 1024


class ProcessTable(dict):
    """Keep readable peers when some subjects cannot be inspected."""

    def __init__(self):
        super().__init__()
        self.unavailable = []
        self.vanished = []


def require_complete(table):
    unavailable = getattr(table, "unavailable", ())
    if unavailable:
        raise RuntimeError("process_table_subjects_unavailable:" + json.dumps(
            {"count": len(unavailable), "subjects": unavailable[:4]},
            sort_keys=True, separators=(",", ":")))


def validate_linux_procfs(status: str, mountinfo: str, own_pid: int):
    # fs/proc/array.c emits one NSpid per namespace, beginning at procfs's
    # namespace. Even coincident PID numbers cannot hide an extra namespace.
    identities = [line.split()[1:] for line in status.splitlines()
                  if line.startswith("NSpid:")]
    if identities != [[str(own_pid)]]:
        raise RuntimeError("procfs_pid_namespace_unconfirmed")
    roots = []
    for line in mountinfo.splitlines():
        before, separator, after = line.partition(" - ")
        fields, filesystem = before.split(), after.split()
        if not separator or len(fields) < 6 or len(filesystem) < 3:
            raise RuntimeError("procfs_mount_context_unavailable")
        point = fields[4]
        if point == "/proc":
            roots.append((fields, filesystem))
        elif point.startswith("/proc/"):
            component = point.split("/")[2]
            if component.isdecimal() or component in ("self", "thread-self"):
                raise RuntimeError("procfs_process_view_overmounted")
    if len(roots) != 1:
        raise RuntimeError("procfs_mount_context_unavailable")
    fields, filesystem = roots[0]
    if fields[3] != "/" or filesystem[0] != "proc":
        raise RuntimeError("procfs_mount_context_unavailable")
    options = fields[5].split(",") + filesystem[2].split(",")
    if any(option.startswith("hidepid=") and option not in ("hidepid=0", "hidepid=off")
           for option in options):
        # hidepid can omit live processes from readdir or return ENOENT.
        raise RuntimeError("procfs_process_visibility_restricted")


def linux_procfs_context():
    contents = []
    for name in ("status", "mountinfo"):
        with (Path("/proc/self") / name).open("rb") as stream:
            data = stream.read(MAX_PROC_CONTEXT_BYTES + 1)
        if len(data) > MAX_PROC_CONTEXT_BYTES:
            raise RuntimeError("procfs_mount_context_over_budget")
        contents.append(data.decode("utf-8", errors="strict"))
    validate_linux_procfs(*contents, os.getpid())


@dataclass(frozen=True)
class Process:
    pid: int
    parent: int
    group: int
    birth: int
    rss: int
    zombie: bool
    uid: int
    traced: bool = False


class BsdInfo(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint32) for name in (
        "flags", "status", "xstatus", "pid", "ppid", "uid", "gid",
        "ruid", "rgid", "svuid", "svgid", "reserved")]
    _fields_ += [("comm", ctypes.c_char * 16), ("name", ctypes.c_char * 32)]
    _fields_ += [(name, ctypes.c_uint32) for name in (
        "nfiles", "pgid", "jobc", "tdev", "tpgid")]
    _fields_ += [("nice", ctypes.c_int32), ("sec", ctypes.c_uint64),
                ("usec", ctypes.c_uint64)]


class ShortBsdInfo(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint32) for name in ("pid", "ppid", "pgid", "status")]
    _fields_ += [("comm", ctypes.c_char * 16)]
    _fields_ += [(name, ctypes.c_uint32) for name in (
        "flags", "uid", "gid", "ruid", "rgid", "svuid", "svgid", "reserved")]


class TaskInfo(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in (
        "virtual", "resident", "total_user", "total_system", "threads_user",
        "threads_system")]
    _fields_ += [("counts", ctypes.c_int32 * 12)]


def snapshot() -> dict[int, Process]:
    result = ProcessTable()
    if sys.platform == "darwin":
        library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        library.proc_listallpids.argtypes = [ctypes.c_void_p, ctypes.c_int]
        library.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int,
                                        ctypes.c_uint64, ctypes.c_void_p,
                                        ctypes.c_int]
        pids = (ctypes.c_int * MAX_SYSTEM_PROCESSES)()
        count = library.proc_listallpids(pids, ctypes.sizeof(pids))
        if count <= 0 or count >= MAX_SYSTEM_PROCESSES:
            raise RuntimeError("process_table_unavailable_or_over_budget")
        for pid in pids[:count]:
            info, task = BsdInfo(), TaskInfo()
            ctypes.set_errno(0)
            if library.proc_pidinfo(pid, 3, 1, ctypes.byref(info), ctypes.sizeof(info)) != ctypes.sizeof(info):
                error = ctypes.get_errno()
                # Full BSD info requires the same UID; short BSD info does
                # not. Exclude a positively identified foreign UID, rather
                # than confusing its expected refusal with our ownership.
                short = ShortBsdInfo()
                ctypes.set_errno(0)
                read = library.proc_pidinfo(pid, 13, 1, ctypes.byref(short), ctypes.sizeof(short))
                short_error = ctypes.get_errno()
                if error == errno.ESRCH:
                    # A later foreign UID may be a reused PID. Preserve the
                    # earlier disappearance before excluding that replacement.
                    result.vanished.append(pid)
                if read == ctypes.sizeof(short) and short.uid != os.getuid():
                    continue
                if error == errno.ESRCH and (read == ctypes.sizeof(short) or short_error == errno.ESRCH):
                    continue
                else:
                    result.unavailable.append({"pid": pid, "errno": error})
                continue
            read = library.proc_pidinfo(pid, 4, 0, ctypes.byref(task), ctypes.sizeof(task))
            result[pid] = Process(pid, info.ppid, info.pgid,
                                  info.sec * 1_000_000 + info.usec,
                                  task.resident if read == ctypes.sizeof(task) else -1,
                                  info.status == 5, info.uid, bool(info.flags & 2))
    elif sys.platform.startswith("linux"):
        linux_procfs_context()
        with os.scandir("/proc") as entries:
            for index, entry in enumerate(entries):
                if index >= MAX_SYSTEM_PROCESSES:
                    raise RuntimeError("process_table_over_budget")
                if not entry.name.isdecimal():
                    continue
                try:
                    raw = (Path(entry.path) / "stat").read_text()
                    fields = raw[raw.rindex(")") + 2:].split()
                    pid = int(entry.name)
                    result[pid] = Process(pid, int(fields[1]), int(fields[2]),
                                          int(fields[19]),
                                          int(fields[21]) * os.sysconf("SC_PAGE_SIZE"),
                                          fields[0] == "Z", os.stat(entry.path).st_uid)
                except (FileNotFoundError, ProcessLookupError):
                    continue
                except PermissionError as error:
                    result.unavailable.append({"pid": int(entry.name), "errno": error.errno})
    else:
        raise RuntimeError("process_supervision_requires_darwin_or_linux")
    if os.getpid() not in result:
        if not any(subject["pid"] == os.getpid() for subject in result.unavailable):
            raise RuntimeError("own_process_missing_from_table")
    return result


def descendants(table: dict[int, Process], root: int, known=None) -> dict[int, Process]:
    # A token-proven orphan remains an owned ancestry root. A recycled PID
    # cannot carry that proof into a different process's subtree.
    selected = {root} | {pid for pid, identity in (known or {}).items()
                         if pid in table and table[pid].birth == identity.birth}
    while True:
        added = {pid for pid, item in table.items() if item.parent in selected}
        if added <= selected:
            return {pid: table[pid] for pid in selected if pid in table}
        selected |= added


def darwin_candidate_current(process: Process) -> bool:
    """Bind a procargs answer back to the sampled birth without another scan."""
    library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    library.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                                    ctypes.c_void_p, ctypes.c_int]
    info = BsdInfo()
    ctypes.set_errno(0)
    read = library.proc_pidinfo(process.pid, 3, 1, ctypes.byref(info), ctypes.sizeof(info))
    if read != ctypes.sizeof(info):
        error = ctypes.get_errno()
        if error == errno.ESRCH:
            return False
        raise RuntimeError("owned_process_identity_unavailable_" + str(error) + ":" + str(process.pid))
    return (info.sec * 1_000_000 + info.usec == process.birth
            and info.uid == process.uid and info.status != 5)


def procargs_environment(data: bytes) -> list[bytes]:
    """Read KERN_PROCARGS2's environment, distinguishing omitted from absent owner."""
    argc = int.from_bytes(data[:4], sys.byteorder, signed=True)
    if len(data) < 4 or not 1 <= argc <= MAX_SYSTEM_PROCESSES:
        raise RuntimeError("owner_environment_unavailable")
    offset = data.find(b"\0", 4)
    if offset < 0:
        raise RuntimeError("owner_environment_unavailable")
    offset += 1
    while offset < len(data) and data[offset] == 0:
        offset += 1
    for _ in range(argc):
        end = data.find(b"\0", offset)
        if end < 0:
            raise RuntimeError("owner_environment_unavailable")
        offset = end + 1
    environment = [entry for entry in data[offset:].split(b"\0") if entry]
    # XNU may successfully return argv but omit ALL environment variables for
    # a restricted target. An empty/invalid environment cannot exclude ours.
    if not data.endswith(b"\0") or not environment or any(b"=" not in entry for entry in environment):
        raise RuntimeError("owner_environment_unavailable")
    return environment


def marked_descendants(table: dict[int, Process], marker: str, earliest: int, *, known=None, remember=None) -> dict[int, Process]:
    """Find same-run Darwin orphans by inherited owner token, never argv logs.

    The marker is installed before the native child exists, survives ordinary
    double-fork/setsid daemonization, and is not present in concurrent work.
    Intentionally hostile code that clears its environment is outside this
    trusted-CLI ownership mechanism; the write guard still applies to it.
    """
    library = ctypes.CDLL(None, use_errno=True)
    library.sysctl.argtypes = [ctypes.POINTER(ctypes.c_int), ctypes.c_uint,
                               ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t),
                               ctypes.c_void_p, ctypes.c_size_t]
    expected = ("HIDE_LIVE_CHECK_OWNER=" + marker).encode()
    result = {}
    for pid, process in table.items():
        if process.uid != os.getuid() or process.birth < earliest or process.zombie:
            continue
        identity = known.get(pid) if known else None
        if identity and identity.birth == process.birth:
            # An ancestry/token proof survives reparenting and exec. Reading
            # the argument stack again adds no ownership evidence and can race
            # with exec/exit. PID reuse still requires a new token proof.
            result[pid] = process
            if remember:
                remember(pid, process)
            continue
        # Ordinary Darwin orphans go to init. ptrace can instead reparent a
        # live child to its tracer: the public BSD flag makes that another
        # candidate, never ownership proof. Unrelated ordinary children need
        # no argument inspection.
        if process.parent != 1 and not process.traced:
            continue
        mib = (ctypes.c_int * 3)(1, 49, pid)  # CTL_KERN, KERN_PROCARGS2, pid (installed SDK).
        size = ctypes.c_size_t(1024 * 1024)
        buffer = ctypes.create_string_buffer(size.value)
        if library.sysctl(mib, 3, buffer, ctypes.byref(size), None, 0):
            error = ctypes.get_errno()
            if error == errno.ESRCH:
                if hasattr(table, "vanished"):
                    table.vanished.append(pid)
                continue
            fresh = snapshot()
            require_complete(fresh)
            current = fresh.get(pid)
            if current is None or current.birth != process.birth or current.zombie:
                if hasattr(table, "vanished"):
                    table.vanished.append(pid)
                continue
            raise RuntimeError("owned_process_arguments_unavailable_" + str(error) + ":" + json.dumps(
                {"pid": pid, "birth": process.birth, "parent": process.parent,
                 "guardian": os.getpid()}, sort_keys=True, separators=(",", ":")))
        if not darwin_candidate_current(process):
            if hasattr(table, "vanished"):
                table.vanished.append(pid)
            continue
        if expected in procargs_environment(buffer.raw[:size.value]):
            result[pid] = process
            if remember:
                # Retain proven ownership even if a later unrelated process
                # refuses inspection and the overall scan must fail closed.
                remember(pid, process)
    return result
