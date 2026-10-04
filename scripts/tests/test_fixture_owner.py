"""Native supervisor controls; kill only the worker, never its process group."""
import ctypes
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / 'target/debug/examples' / ('fixture-owner.exe' if os.name == 'nt' else 'fixture-owner')


def until(predicate, seconds=5):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.005)
    raise AssertionError('native fixture control did not reach its bounded observation')


def receipt(file):
    if not file.exists():
        return None
    version, phase, pid, birth, code, survivors, detail = file.read_text().rstrip('\n').split('\t')
    return {'version': version, 'phase': phase, 'pid': int(pid), 'birth': birth,
        'code': int(code), 'survivors': int(survivors), 'error': bytes.fromhex(detail).decode()}


def publish(file, content):
    temporary = file.with_suffix('.tmp')
    with temporary.open('x') as output:
        output.write(content)
    temporary.replace(file)


class ProcessObservation:
    def __init__(self, pid):
        self.pid = pid
        self.handle = None
        if os.name == 'nt':
            from ctypes import wintypes
            self.kernel = ctypes.WinDLL('kernel32', use_last_error=True)
            self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
            self.kernel.OpenProcess.restype = wintypes.HANDLE
            self.kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
            self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
            self.handle = self.kernel.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
            if not self.handle:
                raise ctypes.WinError(ctypes.get_last_error())

    def exited(self):
        if self.handle:
            result = self.kernel.WaitForSingleObject(self.handle, 0)
            if result == 0xFFFFFFFF:
                raise ctypes.WinError(ctypes.get_last_error())
            return result == 0
        result = subprocess.run(['ps', '-p', str(self.pid), '-o', 'stat='], capture_output=True, text=True, timeout=1)
        if result.returncode not in (0, 1):
            raise RuntimeError(result.stderr)
        return not result.stdout.strip() or result.stdout.strip().startswith('Z')

    def close(self):
        if self.handle and not self.kernel.CloseHandle(self.handle):
            raise ctypes.WinError(ctypes.get_last_error())


class OwnedControlRoot:
    def __init__(self, artifacts):
        self.path = Path(tempfile.mkdtemp(dir=artifacts))
        self.confirmed = False

    def __enter__(self):
        return self

    def __exit__(self, _type, failure, _traceback):
        if self.confirmed:
            shutil.rmtree(self.path)
        elif failure:
            failure.add_note(f'Control child exit unconfirmed; retained root: {self.path}')
        else:
            raise RuntimeError(f'Control child exit unconfirmed; retained root: {self.path}')


class FixtureOwner(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = subprocess.run(['bash', 'scripts/verify-cargo.sh', 'build', '-p', 'hide-platform', '--example', 'fixture-owner'], cwd=ROOT)
        if result.returncode:
            raise RuntimeError('native fixture supervisor build failed')

    def exercise(self, mode):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with OwnedControlRoot(artifacts) as owned:
            root = owned.path
            home = root / 'owned-home'
            home.mkdir()
            (home / 'sentinel').write_text('original home')
            state = root / 'receipt.json'
            identities = root / 'targets.json'
            worker_identity = root / 'worker.json'
            target = root / 'target.py'
            target.write_text("""import json, os, pathlib, subprocess, sys, time
if len(sys.argv) == 2:
    time.sleep(60)
else:
    child = subprocess.Popen([sys.executable, __file__, 'leaf'], start_new_session=os.name != 'nt')
    file = pathlib.Path(sys.argv[2]); temporary = file.with_suffix('.tmp')
    temporary.write_text(json.dumps({'root':os.getpid(), 'child':child.pid})); temporary.replace(file)
    time.sleep(60)
""")
            worker = root / 'worker.py'
            worker.write_text("""import json, pathlib, subprocess, sys, time
child = subprocess.Popen([sys.argv[1], sys.argv[2], sys.argv[3], sys.executable, sys.argv[4], 'root', sys.argv[5]], stdin=subprocess.PIPE)
file = pathlib.Path(sys.argv[6]); temporary = file.with_suffix('.tmp')
def publish():
    temporary.write_text(json.dumps({'helper':child.pid, 'code':child.poll()})); temporary.replace(file)
publish()
deadline = time.monotonic() + 60
while time.monotonic() < deadline:
    if child.poll() is not None:
        publish()
        time.sleep(60)
    time.sleep(.005)
""")
            # The fixture receives a pipe owned only by this worker. Its target
            # has null stdin, and the worker's death never kills the supervisor.
            with (artifacts / f'{mode}.log').open('w') as output:
                process = subprocess.Popen([sys.executable, str(worker), str(HELPER), str(state), str(home),
                    str(target), str(identities), str(worker_identity)], stdout=output, stderr=subprocess.STDOUT)
                observations = []
                try:
                    running = until(lambda: receipt(state))
                    self.assertEqual(running['phase'], 'running')
                    ids = until(lambda: json.loads(identities.read_text()) if identities.exists() else None)
                    self.assertEqual(running['pid'], ids['root'])
                    owner_id = until(lambda: json.loads(worker_identity.read_text()) if worker_identity.exists() else None)
                    observations = [ProcessObservation(ids[key]) for key in ('root', 'child')] + [ProcessObservation(owner_id['helper'])]
                    self.assertTrue(all(not observed.exited() for observed in observations))
                    if mode == 'identity-refusal':
                        publish(state.with_suffix('.stop'), f"{running['pid']} {int(running['birth']) + 1}")
                        refused = until(lambda: (value if value['phase'] == 'stop-refused' else None) if (value := receipt(state)) else None)
                        self.assertIn('launch identity mismatch', refused['error'])
                        self.assertEqual(refused['survivors'], -1)
                        self.assertEqual((home / 'sentinel').read_text(), 'original home')
                        self.assertTrue(all(not observed.exited() for observed in observations))
                        publish(state.with_suffix('.stop'), f"{running['pid']} {running['birth']}")
                        finished = until(lambda: (value if value['phase'] == 'exited' else None) if (value := receipt(state)) else None)
                        self.assertTrue(home.exists())
                    elif mode == 'stop-directory':
                        self.assertEqual((home / 'sentinel').read_text(), 'original home')
                        state.with_suffix('.stop').mkdir()
                        finished = until(lambda: (value if value['phase'] == 'stop-request-read-failed' else None) if (value := receipt(state)) else None)
                        self.assertEqual(finished['survivors'], 0)
                        self.assertIn('stop-request-read-failed:', finished['error'])
                        self.assertIn('stop-request-cleanup-failed:', finished['error'])
                        self.assertIn('raw=Some(', finished['error'])
                        if os.name != 'nt':
                            self.assertIn('Is a directory (os error 21)', finished['error'])
                        self.assertEqual((home / 'sentinel').read_text(), 'original home')
                        exit_record = until(lambda: (value if value['code'] is not None else None) if (value := json.loads(worker_identity.read_text())) else None)
                        self.assertNotEqual(exit_record['code'], 0)
                    else:
                        started = time.monotonic()
                        process.kill()  # exactly the worker PID, no killpg or tree command
                        self.assertNotEqual(process.wait(timeout=5), 0)
                        finished = until(lambda: (value if value['phase'] == 'owner-lost-exited' else None) if (value := receipt(state)) else None)
                        self.assertLess(time.monotonic() - started, 2)
                        self.assertFalse(home.exists())
                    self.assertEqual(finished['survivors'], 0)
                    self.assertEqual(finished['pid'], running['pid'])
                    self.assertEqual(finished['birth'], running['birth'])
                    until(lambda: all(observed.exited() for observed in observations))
                    (artifacts / f'{mode}.json').write_text(json.dumps({'worker': process.pid, **json.loads(worker_identity.read_text()), 'targets': ids, 'running': running, 'finished': finished}, indent=2))
                finally:
                    primary = sys.exc_info()[1]
                    if process.poll() is None:
                        process.kill()
                        process.wait(timeout=5)
                    # EOF always belongs to our worker. Confirm cleanup before
                    # temporary-directory removal, also on an assertion failure.
                    if state.exists() and len(observations) == 3:
                        try:
                            until(lambda: all(observed.exited() for observed in observations))
                            owned.confirmed = True
                        except BaseException as cleanup:
                            if primary is not None:
                                primary.add_note(f'Control cleanup also failed: {cleanup}')
                            else:
                                raise
                    for observed in observations:
                        observed.close()

    def test_worker_only_hard_kill_confirms_zero_owned_survivors(self):
        self.exercise('worker-hard-kill')

    def test_wrong_launch_identity_refuses_a_real_live_target(self):
        self.exercise('identity-refusal')

    def test_native_stop_request_error_preserves_primary_cleanup_and_home(self):
        self.exercise('stop-directory')


if __name__ == '__main__':
    unittest.main()
