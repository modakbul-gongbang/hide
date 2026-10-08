"""Letter 2787/B8: terminal races end signals, never the absence obligation.

Only kernel/filesystem boundaries are substituted. No helper is launched.
"""

import ctypes
from dataclasses import replace
import errno
import os
from pathlib import Path
import signal
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_live_check.process_table import BsdInfo, Process, ProcessState, ProcessTable, identity_state, snapshot
from agent_live_check.processes import ProcessError, group_exists, signal_identity, signal_reserved_group


class ProcessStateContract(unittest.TestCase):
    def test_expected_birth_distinguishes_terminal_and_unknown_metadata(self):
        subject = Process(123, 2, 123, 10, 0, False, os.getuid())
        for name, actual, expected in (
                ("live", subject, ProcessState.LIVE),
                ("zombie", replace(subject, zombie=True), ProcessState.ZOMBIE),
                ("gone", None, ProcessState.VANISHED),
                ("replacement", replace(subject, birth=11, uid=subject.uid + 1), ProcessState.VANISHED)):
            table = ProcessTable()
            if actual is not None:
                table[123] = actual
            with self.subTest(name=name):
                self.assertIs(identity_state(123, 10, uid=subject.uid, table=table), expected)
        for name in ("unavailable", "foreign", "uid", "width"):
            table = ProcessTable()
            if name == "unavailable":
                table.unavailable.append({"pid": 123, "errno": errno.EPERM})
            elif name == "foreign":
                table.foreign_uid_pids.add(123)
            else:
                table[123] = replace(subject, uid=subject.uid + 1) if name == "uid" else replace(subject, pointer_width=4)
            with self.subTest(name=name), self.assertRaises(RuntimeError):
                identity_state(123, 10, uid=subject.uid, pointer_width=8, table=table)
        # An exec can change argv width without ending established birth proof.
        self.assertIs(identity_state(123, 10, uid=subject.uid, table=table), ProcessState.LIVE)

    def test_darwin_fresh_identity_and_denied_signal_share_terminal_states(self):
        subject = Process(123, 2, 123, 10, 0, False, os.getuid())
        for outcome in ("live", "gone", "replacement", "zombie", "unavailable", "uid"):
            library = Mock()
            reads = 0
            def read_info(pid, flavor, unused, buffer, size):
                nonlocal reads
                reads += 1
                if reads > 1 and outcome in ("gone", "unavailable"):
                    ctypes.set_errno(errno.ESRCH if outcome == "gone" else errno.EPERM)
                    return 0
                info = ctypes.cast(buffer, ctypes.POINTER(BsdInfo)).contents
                info.uid, info.flags, info.usec, info.status = os.getuid(), 0x10, 10, 1
                if reads > 1:
                    info.usec += outcome == "replacement"
                    info.uid += outcome == "uid"
                    info.status = 5 if outcome == "zombie" else 1
                return size
            library.proc_pidinfo.side_effect = read_info
            with self.subTest(outcome=outcome), patch.object(sys, "platform", "darwin"), \
                    patch.object(ctypes, "CDLL", return_value=library), \
                    patch.object(os, "kill", side_effect=PermissionError(errno.EPERM, "denied")):
                if outcome in ("live", "unavailable", "uid"):
                    with self.assertRaises((PermissionError, RuntimeError)):
                        signal_identity(subject, signal.SIGTERM)
                else:
                    expected = ProcessState.ZOMBIE if outcome == "zombie" else ProcessState.VANISHED
                    self.assertIs(signal_identity(subject, signal.SIGTERM), expected)

    def test_darwin_terminal_identity_needs_no_signal(self):
        for outcome in ("gone", "replacement", "zombie"):
            library = Mock()
            def read_info(pid, flavor, unused, buffer, size):
                if outcome == "gone":
                    ctypes.set_errno(errno.ESRCH)
                    return 0
                info = buffer._obj
                info.uid, info.usec, info.status = os.getuid(), 11 if outcome == "replacement" else 10, 5
                return size
            library.proc_pidinfo.side_effect = read_info
            with self.subTest(outcome=outcome), patch.object(sys, "platform", "darwin"), \
                    patch.object(ctypes, "CDLL", return_value=library), \
                    patch.object(os, "kill", side_effect=AssertionError("terminal birth must not be signalled")):
                state = signal_identity(Process(123, 2, 123, 10, 0, False, os.getuid()), signal.SIGKILL)
                self.assertIs(state, ProcessState.ZOMBIE if outcome == "zombie" else ProcessState.VANISHED)

    def test_terminal_group_denial_keeps_presence_until_actual_disappearance(self):
        for outcome in ("zombie", "gone", "live", "unavailable"):
            library = Mock()
            def list_group(group, pids, size):
                if outcome == "gone":
                    ctypes.set_errno(errno.ESRCH)
                    return 0
                pids[0] = 123
                return 1
            def read_info(pid, flavor, unused, buffer, size):
                if outcome == "unavailable":
                    ctypes.set_errno(errno.EPERM)
                    return 0
                if flavor == 3:
                    info = buffer._obj
                    info.pid, info.pgid, info.uid, info.status = 123, 123, os.getuid(), 5 if outcome == "zombie" else 1
                return size
            library.proc_listpgrppids.side_effect = list_group
            library.proc_pidinfo.side_effect = read_info
            with self.subTest(outcome=outcome), patch.object(sys, "platform", "darwin"), \
                    patch.object(ctypes, "CDLL", return_value=library), \
                    patch.object(os, "killpg", side_effect=PermissionError(errno.EPERM, "denied")):
                if outcome in ("live", "unavailable"):
                    with self.assertRaises((PermissionError, RuntimeError)):
                        group_exists(123)
                else:
                    self.assertEqual(group_exists(123), outcome == "zombie")
                    # No signal is needed for terminal members, including
                    # an ESRCH group enumeration, but zombies stay present.
                    with patch.object(os, "killpg", side_effect=AssertionError("terminal group must not be signalled")):
                        signal_reserved_group(123, signal.SIGKILL)
            with patch.object(os, "killpg", side_effect=ProcessLookupError(errno.ESRCH, "gone")):
                self.assertFalse(group_exists(123))

    def test_darwin_group_enrollment_uses_the_current_identity_record(self):
        for group in (999, 123):
            library = Mock()
            def list_group(unused, pids, size):
                pids[0] = 123
                return 1
            def read_info(pid, flavor, unused, buffer, size):
                if flavor == 3:
                    info = buffer._obj
                    info.pid, info.pgid, info.uid, info.usec = 123, group, os.getuid(), 20
                return size
            library.proc_listpgrppids.side_effect = list_group
            library.proc_pidinfo.side_effect = read_info
            with self.subTest(group=group), patch.object(sys, "platform", "darwin"), \
                    patch.object(ctypes, "CDLL", return_value=library):
                table = snapshot(123)
            self.assertEqual(list(table), [123] if group == 123 else [])
            if table:
                self.assertEqual((table[123].group, table[123].birth), (123, 20))

    def test_unavailable_group_metadata_does_not_abandon_its_reserved_signal(self):
        library = Mock()
        def list_group(group, pids, size):
            ctypes.set_errno(errno.EPERM)
            return 0
        library.proc_listpgrppids.side_effect = list_group
        sent = []
        with patch.object(sys, "platform", "darwin"), patch.object(ctypes, "CDLL", return_value=library), \
                patch.object(os, "killpg", side_effect=lambda group, signum: sent.append((group, signum))):
            with self.assertRaisesRegex(ProcessError, "owned_group_metadata_unavailable"):
                signal_reserved_group(123, signal.SIGTERM)
        self.assertEqual(sent, [(123, signal.SIGTERM)])

    def test_linux_group_and_birth_come_from_the_same_stat_record(self):
        with tempfile.TemporaryDirectory(prefix="process-state-") as directory:
            root = Path(directory)
            (root / "self").mkdir()
            (root / "self/status").write_text("NSpid:\t" + str(os.getpid()) + "\n")
            (root / "self/mountinfo").write_text("1 0 0:5 / /proc rw - proc proc rw\n")
            (root / "123").mkdir()
            original_open, original_stat = Path.open, os.stat
            def redirect(path):
                path = Path(path)
                return root / path.relative_to("/proc") if path.is_relative_to("/proc") else path
            def proc_open(path, *args, **kwargs):
                return original_open(redirect(path), *args, **kwargs)
            def proc_stat(path, *args, **kwargs):
                return original_stat(redirect(path), *args, **kwargs)
            for group in (999, 123):
                fields = ["0"] * 22
                fields[0], fields[1], fields[2], fields[19], fields[21] = "S", "2", str(group), "20", "1"
                (root / "123/stat").write_text("123 (replacement) " + " ".join(fields))
                entries = Mock()
                entries.__enter__ = Mock(return_value=iter([SimpleNamespace(name="123", path="/proc/123")]))
                entries.__exit__ = Mock(return_value=False)
                with self.subTest(group=group), patch.object(sys, "platform", "linux"), \
                        patch.object(os, "scandir", return_value=entries), \
                        patch.object(os, "getpgid", return_value=123), \
                        patch.object(Path, "open", proc_open), patch.object(os, "stat", proc_stat):
                    table = snapshot(123)
                self.assertEqual(list(table), [123] if group == 123 else [])
                if table:
                    self.assertEqual((table[123].group, table[123].birth), (123, 20))


if __name__ == "__main__":
    unittest.main()
