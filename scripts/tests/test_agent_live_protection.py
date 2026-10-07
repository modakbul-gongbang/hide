"""PRD B3/B6/B8: refuse routing, preserve concurrent bytes, end real children.

No provider, Herdr, or hided build is needed for these guard contracts. The
real-pinned-server lane owns the measurement tool's protocol acceptance.
"""

import json
import contextlib
import ctypes
import io
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_live_check.process_table import (Process, ProcessTable, descendants, marked_descendants,
                                           procargs_environment, require_complete, snapshot, validate_linux_procfs)
from agent_live_check.processes import OwnedProcesses, ProcessError, guard, linux_children_remain
from agent_live_check.protection import ConfigGuard, ProtectionError, stamp, validate_isolation
from agent_live_check.sandbox import WriteSandbox


def procargs(*environment):
    return (1).to_bytes(4, sys.byteorder, signed=True) + b"/fixture\0\0fixture\0" + b"\0".join(environment) + b"\0"


class ConfigurationProtection(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="aclp-", dir="/tmp")
        self.root = Path(self.temporary.name).resolve()
        self.run = self.root / "run"
        self.run.mkdir(mode=0o700)
        self.home = self.run / "fixture-home"
        self.home.mkdir(mode=0o700)
        self.config = self.home / ".config.json"
        self.config.write_bytes(b'{"secret":"fixture-only","model":"original"}\n')
        self.config.chmod(0o640)

    def tearDown(self):
        self.temporary.cleanup()

    def guard(self):
        return ConfigGuard(self.run / "backup", [self.config], [self.home],
                           exclusive_root=self.home)

    def test_operator_change_never_grants_recovery_ownership(self):
        guard = ConfigGuard(self.run / "backup", [self.config], [self.home])
        before = stamp(self.config)
        self.config.write_bytes(b"changed-concurrently")
        with self.assertRaisesRegex(ProtectionError, "no_exclusive_writer"):
            guard.record_write(self.config, before, stamp(self.config))
        self.assertTrue(guard.finish()["failures"])
        self.assertEqual(self.config.read_bytes(), b"changed-concurrently")

    def test_private_backup_and_owned_recovery_are_byte_exact(self):
        before = self.config.read_bytes()
        guard = self.guard()
        original = stamp(self.config)
        self.config.write_bytes(b'{"model":"temporary"}')
        guard.record_write(self.config, original, stamp(self.config))
        result = guard.finish()
        self.assertEqual(self.config.read_bytes(), before)
        self.assertEqual(self.config.stat().st_mode & 0o777, 0o640)
        self.assertEqual(result["restored"], [{"path": str(self.config), "result": "restored"}])
        self.assertEqual(result["failures"], [])
        self.assertNotIn("fixture-only", json.dumps(result))
        for path in (self.run / "backup").iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_concurrent_change_is_preserved_even_after_owned_write(self):
        guard = self.guard()
        original = stamp(self.config)
        self.config.write_bytes(b"owned")
        guard.record_write(self.config, original, stamp(self.config))
        replacement = self.config.with_suffix(".replacement")
        replacement.write_bytes(b"operator-concurrent")
        replacement.replace(self.config)
        result = guard.finish()
        self.assertEqual(self.config.read_bytes(), b"operator-concurrent")
        self.assertEqual(len(result["failures"]), 1)
        self.assertEqual(result["restored"], [])

    def test_unattributed_change_and_new_file_are_reported_without_overwrite(self):
        guard = self.guard()
        self.config.write_bytes(b"unattributed")
        (self.home / "unknown-settings.json").write_bytes(b"another-secret")
        result = guard.finish()
        self.assertEqual(self.config.read_bytes(), b"unattributed")
        self.assertTrue(result["failures"])
        self.assertIn({"path": str(self.home / "unknown-settings.json"), "kind": "added"}, result["directory_changes"])
        self.assertNotIn("another-secret", json.dumps(result))

    def test_alias_cannot_be_a_recovery_target(self):
        self.config.unlink()
        self.config.symlink_to(self.home / "other")
        with self.assertRaises(ProtectionError):
            self.guard()

    def test_refuses_operator_socket_state_home_and_unowned_short_directory(self):
        daemon_home = self.run / "home"
        state = self.run / "state"
        valid_socket = self.run / "herdr.sock"
        validate_isolation(self.run, daemon_home, valid_socket, state, self.home, None)
        cases = [
            (daemon_home, self.home / ".config/herdr/herdr.sock", state),
            (daemon_home, valid_socket, self.home / ".hide/state"),
            (self.home, valid_socket, state),
            (daemon_home, self.root / "short.sock", state),
            (daemon_home, valid_socket, Path("relative-state")),
        ]
        for home, sock, private_state in cases:
            with self.subTest(case=str(sock)):
                with self.assertRaises(ProtectionError):
                    validate_isolation(self.run, home, sock, private_state, self.home, None)


@unittest.skipUnless(sys.platform == "darwin" or sys.platform.startswith("linux"),
                     "process guardian supports macOS and Linux")
class ProcessProtection(unittest.TestCase):
    def test_linux_procfs_must_name_the_same_namespace_without_hiding_processes(self):
        mount = "1 0 0:5 / /proc rw - proc proc rw\n"
        validate_linux_procfs("NSpid:\t42\n", mount, 42)
        cases = [("NSpid:\t7000\t42\n", mount), ("NSpid:\t42\t42\n", mount),
                 ("Pid:\t42\n", mount), ("NSpid:\t42\nNSpid:\t42\n", mount)]
        for policy in ("1", "2", "4", "noaccess", "invisible", "ptraceable", "unknown"):
            cases.append(("NSpid:\t42\n", mount.replace("- proc proc rw", "- proc proc rw,hidepid=" + policy)))
        cases.extend([("NSpid:\t42\n", mount.replace("- proc", "- tmpfs")),
                      ("NSpid:\t42\n", mount + "2 1 0:6 / /proc/80/stat rw - tmpfs none rw\n")])
        for status, mounts in cases:
            with self.subTest(status=status, mounts=mounts), self.assertRaises(RuntimeError):
                validate_linux_procfs(status, mounts, 42)

    def test_darwin_metadata_denial_preserves_a_peer_without_proving_absence(self):
        import errno
        own = os.getpid()
        library = Mock()
        def list_pids(buffer, unused):
            buffer[0], buffer[1] = own, 111
            return 2
        def info(pid, flavor, unused, buffer, size):
            if pid == 111:
                ctypes.set_errno(errno.EPERM)
                return 0
            value = buffer._obj
            if flavor == 3:
                value.ppid, value.uid, value.sec, value.flags = 1, os.getuid(), 1, 2
            return size
        library.proc_listallpids.side_effect = list_pids
        library.proc_pidinfo.side_effect = info
        with patch("agent_live_check.process_table.sys.platform", "darwin"), \
                patch("agent_live_check.process_table.ctypes.CDLL", return_value=library):
            table = snapshot()
        self.assertIn(own, table)
        self.assertTrue(table[own].traced)
        with self.assertRaisesRegex(RuntimeError, "process_table_subjects_unavailable"):
            require_complete(table)
        self.assertEqual(table.unavailable, [{"pid": 111, "errno": errno.EPERM}])

    def test_uninspectable_subject_forces_failure_while_readable_owned_peer_is_ended(self):
        guardian = Process(os.getpid(), 1, os.getpid(), 1, 0, False, os.getuid())
        child = Process(111, guardian.pid, 111, 2, 0, False, os.getuid())
        before = {guardian.pid: guardian, child.pid: child}
        partial = ProcessTable()
        partial.update(before)
        partial.unavailable.append({"pid": 222, "errno": 13})
        ended = {guardian.pid: guardian}
        native = Mock(returncode=0)
        native.poll.return_value = 0
        library = Mock()
        library.prctl.return_value = 0
        with patch("agent_live_check.processes.sys.platform", "linux"), \
                patch("agent_live_check.processes.ctypes.CDLL", return_value=library), \
                patch("agent_live_check.processes.threading.Thread"), \
                patch("agent_live_check.processes.signal.signal"), \
                patch("agent_live_check.processes.time.sleep"), \
                patch("agent_live_check.processes.OwnedProcesses.spawn", return_value=native), \
                patch("agent_live_check.processes.snapshot", side_effect=[before] + [partial] * 5 + [ended]), \
                patch("agent_live_check.processes.os.waitpid", side_effect=ChildProcessError), \
                patch("agent_live_check.processes.os.kill") as kill, contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(guard(-1, ["fixture"]), 125)
        self.assertIn(unittest.mock.call(child.pid, signal.SIGKILL), kill.call_args_list)
        self.assertEqual({call.args[0] for call in kill.call_args_list}, {child.pid})

    def test_unknown_traced_child_requires_token_even_with_a_live_external_parent(self):
        child = Process(111, 999, 111, 10, 0, False, os.getuid(), traced=True)
        library = Mock()
        def query(mib, unused, buffer, size, *rest):
            data = procargs(b"HIDE_LIVE_CHECK_OWNER=fixture-run")
            ctypes.memmove(buffer, data, len(data))
            size._obj.value = len(data)
            return 0
        library.sysctl.side_effect = query
        with patch("agent_live_check.process_table.ctypes.CDLL", return_value=library), \
                patch("agent_live_check.process_table.darwin_candidate_current", return_value=True):
            self.assertEqual(marked_descendants({111: child}, "fixture-run", 1), {111: child})
        library.sysctl.assert_called_once()

    def test_successful_argument_only_read_cannot_exclude_an_owned_restricted_helper(self):
        self.assertEqual(procargs_environment(procargs(b"OTHER=fixture")), [b"OTHER=fixture"])
        with self.assertRaisesRegex(RuntimeError, "owner_environment_unavailable"):
            procargs_environment(procargs())
        orphan = Process(111, 1, 111, 10, 0, False, os.getuid())
        library = Mock()
        def query(mib, unused, buffer, size, *rest):
            data = procargs()
            ctypes.memmove(buffer, data, len(data))
            size._obj.value = len(data)
            return 0
        library.sysctl.side_effect = query
        with patch("agent_live_check.process_table.ctypes.CDLL", return_value=library), \
                patch("agent_live_check.process_table.darwin_candidate_current", return_value=True):
            with self.assertRaisesRegex(RuntimeError, "owner_environment_unavailable"):
                marked_descendants({111: orphan}, "fixture-run", 1)

    def test_successful_token_answer_is_rejected_if_sampled_pid_was_reused(self):
        orphan = Process(111, 1, 111, 10, 0, False, os.getuid())
        for environment in (b"OTHER=fixture", b"HIDE_LIVE_CHECK_OWNER=fixture-run"):
            table = ProcessTable()
            table[111] = orphan
            library = Mock()
            def query(mib, unused, buffer, size, *rest):
                data = procargs(environment)
                ctypes.memmove(buffer, data, len(data))
                size._obj.value = len(data)
                return 0
            def replacement(pid, flavor, unused, buffer, size):
                info = buffer._obj
                info.sec, info.usec, info.uid = 0, 11, os.getuid()
                return size
            library.sysctl.side_effect = query
            library.proc_pidinfo.side_effect = replacement
            with self.subTest(environment=environment), \
                    patch("agent_live_check.process_table.ctypes.CDLL", return_value=library):
                self.assertEqual(marked_descendants(table, "fixture-run", 1), {})
            self.assertEqual(table.vanished, [111])

    def test_linux_final_empty_view_needs_kernel_no_child_proof_including_clone_children(self):
        with patch("agent_live_check.processes.os.waitpid", side_effect=[(222, 0), (0, 0)]) as wait:
            self.assertTrue(linux_children_remain())
        self.assertTrue(all(call.args == (-1, os.WNOHANG | 0x40000000) for call in wait.call_args_list))
        with patch("agent_live_check.processes.os.waitpid", side_effect=[(222, 0), ChildProcessError]):
            self.assertFalse(linux_children_remain())

    def test_darwin_disappearing_parent_cannot_make_final_empty_view_succeed(self):
        guardian = Process(os.getpid(), 1, os.getpid(), 1, 0, False, os.getuid())
        child = Process(111, guardian.pid, 111, 2, 0, False, os.getuid())
        helper = Process(333, 1, 333, 3, 0, False, os.getuid())
        before = {guardian.pid: guardian, child.pid: child}
        ended = {guardian.pid: guardian}
        ambiguous = ProcessTable()
        ambiguous.update(ended)
        ambiguous.vanished.append(222)
        def discover(table, marker, earliest, *, known, remember):
            if helper.pid in table:
                remember(helper.pid, helper)
                return {helper.pid: helper}
            return {}
        native = Mock(returncode=0)
        native.poll.return_value = 0
        with patch("agent_live_check.processes.sys.platform", "darwin"), \
                patch("agent_live_check.processes.threading.Thread"), \
                patch("agent_live_check.processes.signal.signal"), \
                patch("agent_live_check.processes.time.sleep"), \
                patch("agent_live_check.processes.OwnedProcesses.spawn", return_value=native), \
                patch("agent_live_check.processes.marked_descendants", side_effect=discover), \
                patch("agent_live_check.processes.snapshot", side_effect=[before] * 6 + [ambiguous, {**ended, helper.pid: helper}, ended]), \
                patch("agent_live_check.processes.os.kill") as kill:
            self.assertEqual(guard(-1, ["fixture"]), 0)
        self.assertIn(unittest.mock.call(helper.pid, signal.SIGKILL), kill.call_args_list,
                      "a disappeared shutdown parent hid its live child behind an empty final view")

    def test_linux_reaping_cannot_invalidate_a_view_still_used_for_signalling(self):
        guardian = Process(os.getpid(), 1, os.getpid(), 1, 0, False, os.getuid())
        child = Process(111, guardian.pid, 111, 2, 0, False, os.getuid())
        before, ended = {guardian.pid: guardian, child.pid: child}, {guardian.pid: guardian}
        trace = []
        def wait(*unused):
            trace.append("wait")
            raise ChildProcessError
        native, library = Mock(returncode=0), Mock()
        native.poll.return_value, library.prctl.return_value = 0, 0
        with patch("agent_live_check.processes.sys.platform", "linux"), \
                patch("agent_live_check.processes.ctypes.CDLL", return_value=library), \
                patch("agent_live_check.processes.threading.Thread"), \
                patch("agent_live_check.processes.signal.signal"), \
                patch("agent_live_check.processes.time.sleep"), \
                patch("agent_live_check.processes.OwnedProcesses.spawn", return_value=native), \
                patch("agent_live_check.processes.snapshot", side_effect=[before] * 7 + [ended]), \
                patch("agent_live_check.processes.os.waitpid", side_effect=wait), \
                patch("agent_live_check.processes.os.kill", side_effect=lambda *unused: trace.append("kill")):
            self.assertEqual(guard(-1, ["fixture"]), 0)
        self.assertNotIn("kill", trace[trace.index("wait"):], "reaping invalidated the birth-checked signal view")

    def test_persistent_foreign_zombie_does_not_make_owned_cleanup_unconfirmed(self):
        own, alive = os.getpid(), []
        library = Mock()
        def list_pids(buffer, unused):
            pids = [own, 222] + ([111] if alive else [])
            for index, pid in enumerate(pids):
                buffer[index] = pid
            return len(pids)
        def info(pid, flavor, include_zombies, buffer, size):
            if pid == 222 and flavor == 3 and not include_zombies:
                ctypes.set_errno(3)  # ESRCH: arg=0 excludes a zombie.
                return 0
            value = buffer._obj
            if flavor in (3, 13):
                value.uid, value.ppid, value.status = os.getuid(), 1 if pid != 111 else own, 5 if pid == 222 else 2
                if flavor == 3:
                    value.sec = 1 if pid == own else pid
            return size
        def signal_owned(pid, signum):
            self.assertEqual(pid, 111)
            if signum == signal.SIGKILL:
                alive.clear()
        native = Mock(returncode=0)
        native.poll.return_value = 0
        library.proc_listallpids.side_effect, library.proc_pidinfo.side_effect = list_pids, info
        with patch("agent_live_check.processes.sys.platform", "darwin"), \
                patch("agent_live_check.process_table.ctypes.CDLL", return_value=library), \
                patch("agent_live_check.processes.threading.Thread"), \
                patch("agent_live_check.processes.signal.signal"), \
                patch("agent_live_check.processes.time.sleep"), \
                patch("agent_live_check.processes.OwnedProcesses.spawn", side_effect=lambda *a, **kw: (alive.append(111), native)[1]), \
                patch("agent_live_check.processes.os.kill", side_effect=signal_owned):
            self.assertEqual(guard(-1, ["fixture"]), 0)

    def test_owned_orphan_subtree_is_tracked_without_adopting_reused_or_external_roots(self):
        def item(pid, parent, birth):
            return Process(pid, parent, pid, birth, 1, False, os.getuid())
        table = {pid: item(pid, parent, birth) for pid, parent, birth in (
            (999, 1, 1), (111, 999, 2), (222, 1, 3), (333, 222, 4),
            (444, 333, 5), (555, 1, 6), (666, 555, 7), (777, 1, 9), (888, 777, 10))}
        proven = {222: table[222], 777: item(777, 1, 8)}
        current = descendants(table, 999, proven)
        self.assertEqual(set(current), {999, 111, 222, 333, 444})
        self.assertEqual(sum(process.rss for process in current.values()), 5)

    def test_live_non_orphan_needs_no_token_read_but_unknown_orphan_still_fails(self):
        import ctypes
        import errno
        ordinary = Process(111, 999, 111, 10, 0, False, os.getuid())
        orphan = Process(111, 1, 111, 10, 0, False, os.getuid())
        library = Mock()
        def unavailable(*unused):
            ctypes.set_errno(errno.EIO)
            return -1
        library.sysctl.side_effect = unavailable
        with patch("agent_live_check.process_table.ctypes.CDLL", return_value=library), \
                patch("agent_live_check.process_table.snapshot", return_value={111: ordinary}):
            self.assertEqual(marked_descendants({111: ordinary}, "fixture-run", 1), {})
            library.sysctl.assert_not_called()
        with patch("agent_live_check.process_table.ctypes.CDLL", return_value=library), \
                patch("agent_live_check.process_table.snapshot", return_value={111: orphan}):
            with self.assertRaisesRegex(RuntimeError, "owned_process_arguments_unavailable_5"):
                marked_descendants({111: orphan}, "fixture-run", 1)
        library.sysctl.assert_called_once()

    def test_shutdown_helper_discovered_after_term_is_killed_before_cleanup_succeeds(self):
        guardian = Process(os.getpid(), 1, os.getpid(), 1, 0, False, os.getuid())
        child = Process(111, guardian.pid, 111, 2, 0, False, os.getuid())
        helper = Process(222, guardian.pid, 222, 3, 0, False, os.getuid())
        before = {guardian.pid: guardian, child.pid: child}
        after_term = {**before, helper.pid: helper}
        ended = {guardian.pid: guardian}
        native = Mock(returncode=0)
        native.poll.return_value = 0
        library = Mock()
        library.prctl.return_value = 0
        for initial_reads in (5, 6):
            with self.subTest(helper_before_kill=initial_reads == 5), \
                    patch("agent_live_check.processes.sys.platform", "linux"), \
                    patch("agent_live_check.processes.ctypes.CDLL", return_value=library), \
                    patch("agent_live_check.processes.threading.Thread"), \
                    patch("agent_live_check.processes.signal.signal"), \
                    patch("agent_live_check.processes.time.sleep"), \
                    patch("agent_live_check.processes.OwnedProcesses.spawn", return_value=native), \
                    patch("agent_live_check.processes.snapshot", side_effect=[before] * initial_reads + [after_term, ended]), \
                    patch("agent_live_check.processes.os.waitpid", side_effect=ChildProcessError), \
                    patch("agent_live_check.processes.os.kill") as kill:
                self.assertEqual(guard(-1, ["fixture"]), 0)
            self.assertIn(unittest.mock.call(helper.pid, signal.SIGKILL), kill.call_args_list,
                          "cleanup declared success without ending the newly adopted helper")

    def test_proven_birth_identity_needs_no_argument_read_but_reused_pid_does(self):
        import ctypes
        import errno
        identity = Process(111, 1, 111, 10, 0, False, os.getuid())
        library = Mock()
        def unavailable(*unused):
            ctypes.set_errno(errno.EIO)
            return -1
        library.sysctl.side_effect = unavailable
        with patch("agent_live_check.process_table.ctypes.CDLL", return_value=library):
            result = marked_descendants({111: identity}, "fixture-run", 1,
                                        known={111: identity})
            self.assertEqual(result, {111: identity})
            library.sysctl.assert_not_called()
            replacement = Process(111, 1, 111, 11, 0, False, os.getuid())
            with patch("agent_live_check.process_table.snapshot", return_value={111: replacement}):
                with self.assertRaisesRegex(RuntimeError, "owned_process_arguments_unavailable_5"):
                    marked_descendants({111: replacement}, "fixture-run", 1,
                                       known={111: identity})
            library.sysctl.assert_called_once()

    def test_partial_orphan_identity_survives_later_unrelated_denial(self):
        import ctypes
        import errno
        table = {111: Process(111, 1, 111, 10, 0, False, os.getuid()),
                 222: Process(222, 1, 222, 11, 0, False, os.getuid())}
        known = {}
        def query(mib, _, buffer, size, *unused):
            if mib[2] == 111:
                data = procargs(b"HIDE_LIVE_CHECK_OWNER=fixture-run")
                ctypes.memmove(buffer, data, len(data))
                size._obj.value = len(data)
                return 0
            ctypes.set_errno(errno.EPERM)
            return -1
        library = Mock()
        library.sysctl.side_effect = query
        with patch("agent_live_check.process_table.ctypes.CDLL", return_value=library), \
                patch("agent_live_check.process_table.darwin_candidate_current", return_value=True), \
                patch("agent_live_check.process_table.snapshot", return_value=table):
            with self.assertRaisesRegex(RuntimeError, "owned_process_arguments_unavailable"):
                marked_descendants(table, "fixture-run", 1, remember=known.__setitem__)
        self.assertEqual(known, {111: table[111]}, "proven orphan was discarded on a later denial")

    @unittest.skipUnless(sys.platform == "darwin", "Darwin orphan discovery")
    def test_orphan_scan_failure_still_ends_known_child(self):
        with tempfile.TemporaryDirectory(prefix="agent-scan-") as name:
            receipt = Path(name) / "child.pid"
            child = ("import os,time; from pathlib import Path; "
                     f"Path({str(receipt)!r}).write_text(str(os.getpid())); time.sleep(5)")
            program = (
                "import os,sys,time; from pathlib import Path; from unittest.mock import patch; "
                "from agent_live_check.processes import guard; r,w=os.pipe(); "
                "\ndef unavailable(*args,**kwargs):"
                f"\n deadline=time.monotonic()+1; file=Path({str(receipt)!r})"
                "\n while not file.exists() and time.monotonic()<deadline: time.sleep(.01)"
                "\n raise RuntimeError('injected_unrelated_procargs_denial')"
                "\nwith patch('agent_live_check.processes.marked_descendants', side_effect=unavailable):"
                f"\n result=guard(r,[sys.executable,'-c',{child!r}])"
                "\nos.close(w); raise SystemExit(result)")
            with OwnedProcesses() as owner:
                with self.assertRaisesRegex(ProcessError, "guardian_cleanup_or_resource_failure"):
                    owner.run([sys.executable, "-c", program],
                              env={**os.environ, "PYTHONPATH": str(Path(__file__).resolve().parents[1])},
                              seconds=5, check=False)
            self.assertTrue(receipt.exists(), "injected denial never exercised a running child")
            item = snapshot().get(int(receipt.read_text()))
            self.assertTrue(item is None or item.zombie, "scan failure abandoned proven child")

    def test_short_lived_parent_cannot_leave_detached_child(self):
        with tempfile.TemporaryDirectory(prefix="agent-detach-") as name:
            receipt = Path(name) / "child.pid"
            script = ("import os,time; from pathlib import Path; "
                      "pid=os.fork(); "
                      "\nif pid: raise SystemExit(0)"
                      "\nos.setsid()"
                      "\nif os.fork(): raise SystemExit(0)"
                      "\nos.close(1); os.close(2)"
                      f"\nPath({str(receipt)!r}).write_text(str(os.getpid()))"
                      "\ntime.sleep(5)")
            with OwnedProcesses() as owner:
                code, _, _ = owner.run([sys.executable, "-c", script], env=dict(os.environ))
                self.assertEqual(code, 0)
            # A missing receipt means the kernel tracked and stopped the leaf
            # before it ran. A live receipt must name an already ended process.
            if receipt.exists():
                item = snapshot().get(int(receipt.read_text()))
                self.assertTrue(item is None or item.zombie, "detached child survived confirmed cleanup")

    def test_finite_command_and_repeated_units_leave_no_child(self):
        with OwnedProcesses() as owner:
            for _ in range(20):
                code, out, err = owner.run([sys.executable, "-c", "print('finished')"], env=dict(os.environ))
                self.assertEqual((code, out.strip(), err), (0, "finished", ""))
            self.assertLessEqual(len(owner.children), 1)
        self.assertEqual(owner.children, {})

    def test_timeout_and_output_limit_fail_without_leaving_child(self):
        with OwnedProcesses() as owner:
            with self.assertRaisesRegex(ProcessError, "command_timeout"):
                owner.run([sys.executable, "-c", "import time; time.sleep(30)"], env=dict(os.environ), seconds=0.3)
            with self.assertRaisesRegex(ProcessError, "command_output_over_budget"):
                owner.run([sys.executable, "-c", "import sys; sys.stdout.write('x'*2000000)"], env=dict(os.environ))
        self.assertEqual(owner.children, {})

    def test_killed_owner_leaves_no_child_or_grandchild(self):
        # This fixture deliberately has no outer guardian: otherwise the outer
        # cleanup could hide a defect in the production child's EOF guardian.
        with tempfile.TemporaryDirectory(prefix="agent-owner-") as name:
            path = Path(name) / "pids.json"
            leaf = "import os,time; from pathlib import Path; Path(%r).write_text(str(os.getpid())); time.sleep(30)" % str(path)
            script = "import os,sys,time; sys.path.insert(0,%r); from agent_live_check.processes import OwnedProcesses; o=OwnedProcesses(); o.spawn([sys.executable,'-c',%r],env=dict(os.environ)); time.sleep(30)" % (str(Path(__file__).resolve().parents[1]), leaf)
            fixture = subprocess.Popen([sys.executable, "-c", script], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            try:
                deadline = time.monotonic() + 5
                while not path.exists() and time.monotonic() < deadline:
                    time.sleep(0.02)
                self.assertTrue(path.exists(), "fixture child did not announce its pid")
                pid = int(path.read_text())
                table = snapshot()
                child = table[pid]
                guardian = table[child.parent]
                fixture.kill()
                fixture.wait(timeout=2)
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    table = snapshot()
                    remaining = [item.pid for item in (child, guardian)
                                 if item.pid in table and table[item.pid].birth == item.birth
                                 and not table[item.pid].zombie]
                    if not remaining:
                        break
                    time.sleep(0.05)
                self.assertEqual(remaining, [])
            finally:
                if fixture.poll() is None:
                    fixture.kill()
                    fixture.wait(timeout=2)
                fixture.stderr.close()


@unittest.skipUnless(sys.platform == "darwin", "authenticated write guard is macOS-only")
class NativeWriteProtection(unittest.TestCase):
    def test_other_process_control_arguments_and_mailbox_storage_are_denied(self):
        with tempfile.TemporaryDirectory(prefix="acl-", dir="/tmp") as name:
            root = Path(name).resolve()
            run, sockets, home = [root / key for key in ("run", "sockets", "operator")]
            for path in (run, sockets, home):
                path.mkdir(mode=0o700)
            state = run / "state"
            caps = state / "pane-capabilities"
            caps.mkdir(parents=True, mode=0o700)
            reference = caps / "own.json"
            reference.write_text("fixture-reference")
            reference.chmod(0o600)
            mailbox = state / "mailbox.json"
            mailbox.write_text("fresh-test-marker")
            guard = WriteSandbox(run, sockets, home, [])
            guard.allow_reference(reference)
            with OwnedProcesses() as owner:
                standin = owner.spawn([sys.executable, "-c", "import time; time.sleep(5)", "fixture-process-marker"],
                                     env=dict(os.environ), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                program = ("import ctypes,errno,os; from pathlib import Path"
                           f"\nassert Path({str(reference)!r}).read_text() == 'fixture-reference'"
                           f"\nPath({str(reference.with_suffix('.claimed'))!r}).write_text('claim')"
                           f"\ntry: Path({str(mailbox)!r}).read_text()"
                           "\nexcept PermissionError: pass"
                           "\nelse: raise SystemExit(31)"
                           f"\ntry: os.kill({standin.pid}, 0)"
                           "\nexcept PermissionError: pass"
                           "\nelse: raise SystemExit(32)"
                           "\nlib=ctypes.CDLL(None,use_errno=True)"
                           f"\nmib=(ctypes.c_int*3)(1,49,{standin.pid})"
                           "\nsize=ctypes.c_size_t(1024*1024); data=ctypes.create_string_buffer(size.value)"
                           "\nassert lib.sysctl(mib,3,data,ctypes.byref(size),None,0) == -1"
                           "\nassert ctypes.get_errno() == errno.EPERM"
                           "\nprint('all-denials-enforced')")
                code, output, error = owner.run(guard.command([sys.executable, "-c", program]), env=dict(os.environ), check=False)
                self.assertEqual((code, output.strip()), (0, "all-denials-enforced"), error)

    def test_real_sandbox_blocks_outside_writes_and_other_unix_sockets(self):
        with tempfile.TemporaryDirectory(prefix="acl-", dir="/tmp") as name:
            root = Path(name).resolve()
            run, sockets, outside, home = [root / key for key in ("run", "sockets", "outside", "operator")]
            for path in (run, sockets, outside, home):
                path.mkdir(mode=0o700)
            history = home / "sessions"
            history.mkdir()
            previous = history / "previous.json"
            previous.write_bytes(b"operator-session")
            sandbox = WriteSandbox(run, sockets, home, [history])
            with OwnedProcesses() as owner:
                sandbox.verify(owner, dict(os.environ), outside)
                # A broad session allowance must never make older sessions
                # writable. This is the /resume picker protection boundary.
                program = "from pathlib import Path; Path(%r).write_bytes(b'changed')" % str(previous)
                code, _, _ = owner.run(sandbox.command([sys.executable, "-c", program]), env=dict(os.environ), check=False)
                self.assertNotEqual(code, 0)
                self.assertEqual(previous.read_bytes(), b"operator-session")
                fresh = history / "new-session.json"
                program = "from pathlib import Path; Path(%r).write_bytes(b'new-run-session')" % str(fresh)
                code, _, _ = owner.run(sandbox.command([sys.executable, "-c", program]), env=dict(os.environ), check=False)
                self.assertEqual(code, 0)
                self.assertEqual(fresh.read_bytes(), b"new-run-session")


if __name__ == "__main__":
    unittest.main()
