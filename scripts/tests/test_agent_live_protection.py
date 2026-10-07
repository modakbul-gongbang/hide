"""PRD B3/B6/B8: refuse routing, preserve concurrent bytes, end real children.

No provider, Herdr, or hided build is needed for these guard contracts. The
real-pinned-server lane owns the measurement tool's protocol acceptance.
"""

import json
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
from agent_live_check.process_table import Process, marked_descendants, snapshot
from agent_live_check.processes import OwnedProcesses, ProcessError, guard
from agent_live_check.protection import ConfigGuard, ProtectionError, stamp, validate_isolation
from agent_live_check.sandbox import WriteSandbox


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
                data = b"HIDE_LIVE_CHECK_OWNER=fixture-run\0"
                ctypes.memmove(buffer, data, len(data))
                size._obj.value = len(data)
                return 0
            ctypes.set_errno(errno.EPERM)
            return -1
        library = Mock()
        library.sysctl.side_effect = query
        with patch("agent_live_check.process_table.ctypes.CDLL", return_value=library), \
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
