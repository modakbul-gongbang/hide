"""Process identities and RSS, without subprocesses or inspecting argv.

Darwin layouts follow sys/proc_info.h in the installed SDK. Linux uses proc(5).
"""

import ctypes
from dataclasses import dataclass
import os
from pathlib import Path
import sys

MAX_SYSTEM_PROCESSES = 65_536


@dataclass(frozen=True)
class Process:
    pid: int
    parent: int
    group: int
    birth: int
    rss: int
    zombie: bool


class BsdInfo(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint32) for name in (
        "flags", "status", "xstatus", "pid", "ppid", "uid", "gid",
        "ruid", "rgid", "svuid", "svgid", "reserved")]
    _fields_ += [("comm", ctypes.c_char * 16), ("name", ctypes.c_char * 32)]
    _fields_ += [(name, ctypes.c_uint32) for name in (
        "nfiles", "pgid", "jobc", "tdev", "tpgid")]
    _fields_ += [("nice", ctypes.c_int32), ("sec", ctypes.c_uint64),
                ("usec", ctypes.c_uint64)]


class TaskInfo(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in (
        "virtual", "resident", "total_user", "total_system", "threads_user",
        "threads_system")]
    _fields_ += [("counts", ctypes.c_int32 * 12)]


def snapshot() -> dict[int, Process]:
    result = {}
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
            if library.proc_pidinfo(pid, 3, 0, ctypes.byref(info), ctypes.sizeof(info)) != ctypes.sizeof(info):
                continue
            read = library.proc_pidinfo(pid, 4, 0, ctypes.byref(task), ctypes.sizeof(task))
            result[pid] = Process(pid, info.ppid, info.pgid,
                                  info.sec * 1_000_000 + info.usec,
                                  task.resident if read == ctypes.sizeof(task) else -1,
                                  info.status == 5)
    elif sys.platform.startswith("linux"):
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
                                          fields[0] == "Z")
                except (FileNotFoundError, ProcessLookupError, PermissionError):
                    continue
    else:
        raise RuntimeError("process_supervision_requires_darwin_or_linux")
    if os.getpid() not in result:
        raise RuntimeError("own_process_missing_from_table")
    return result


def descendants(table: dict[int, Process], root: int) -> dict[int, Process]:
    selected = {root}
    while True:
        added = {pid for pid, item in table.items() if item.parent in selected}
        if added <= selected:
            return {pid: table[pid] for pid in selected if pid in table}
        selected |= added
