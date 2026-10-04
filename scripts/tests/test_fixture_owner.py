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

    @unittest.skipUnless(os.name != 'nt', 'Unix ancestry-failure termination boundary')
    def test_native_ancestry_overflow_still_ends_the_original_private_group(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with OwnedControlRoot(artifacts) as owned:
            root = owned.path
            home = root / 'owned-home'
            home.mkdir()
            (home / 'sentinel').write_text('original home')
            identities = root / 'identities'
            state = root / 'depth.receipt'
            target = root / 'deep.py'
            target.write_text("""import os, pathlib, subprocess, sys, time
file = pathlib.Path(sys.argv[1])
with file.open('a') as output: output.write(str(os.getpid())+'\\n')
depth = int(sys.argv[2])
if depth:
    child = subprocess.Popen([sys.executable, __file__, str(file), str(depth-1)])
    child.wait()
else:
    file.with_suffix('.ready').write_text('ready')
    time.sleep(60)
""")
            observations = []
            with (artifacts / 'ancestry-overflow.log').open('w') as output:
                helper = subprocess.Popen([str(HELPER), str(state), str(home), sys.executable, str(target), str(identities), '33'],
                    stdin=subprocess.PIPE, stdout=output, stderr=subprocess.STDOUT)
                try:
                    until(lambda: identities.with_suffix('.ready').exists())
                    ids = [int(pid) for pid in identities.read_text().splitlines()]
                    self.assertEqual(len(ids), 34)
                    observations = [ProcessObservation(pid) for pid in ids]
                    self.assertTrue(all(not process.exited() for process in observations))
                    running = receipt(state)
                    self.assertEqual(running['phase'], 'running')
                    self.assertEqual(running['pid'], ids[0])
                    publish(state.with_suffix('.stop'), f"{running['pid']} {running['birth']}")
                    finished = until(lambda: (value if value['phase']=='exit-unconfirmed' else None) if (value:=receipt(state)) else None)
                    self.assertIn('process ancestry depth cap exceeded', finished['error'])
                    self.assertIn('owned termination also failed:', finished['error'])
                    self.assertEqual(finished['survivors'], -1)
                    self.assertNotEqual(helper.wait(timeout=5), 0)
                    until(lambda: all(process.exited() for process in observations))
                    self.assertEqual((home / 'sentinel').read_text(), 'original home')
                    (artifacts / 'ancestry-overflow.json').write_text(json.dumps({'targets':ids,'running':running,'finished':finished,'independentlyObservedSurvivors':0}, indent=2))
                    owned.confirmed = True
                finally:
                    primary = sys.exc_info()[1]
                    helper.stdin.close()
                    if helper.poll() is None: helper.wait(timeout=5)
                    if observations:
                        try:
                            until(lambda: all(process.exited() for process in observations))
                            owned.confirmed = True
                        except BaseException as cleanup:
                            if primary is None: raise
                            primary.add_note(f'Ancestry overflow cleanup also failed: {cleanup}')
                    for process in observations: process.close()

    def test_actual_playwright_worker_loss_ends_ordinary_fixture_consumers(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with OwnedControlRoot(artifacts) as owned:
            proof = owned.path / 'ordinary-worker.json'
            filename = ROOT / 'web/e2e' / ('ci-owner-' + owned.path.name + '.spec.ts')
            filename.write_text("import { test } from '@playwright/test';\n"
                "import fs from 'node:fs'; import path from 'node:path';\n"
                "import { startHerdr } from './herdr-fixture'; import { startHided } from './hided-fixture';\n"
                "test('ordinary fixture worker loss', async () => {\n"
                " const herdr = await startHerdr({agents:false}); const daemon = await startHided(herdr);\n"
                " const directory = path.resolve('../agents/runs/ci-fixture-owners');\n"
                " const native = fs.readdirSync(directory).filter(f=>f.endsWith('.receipt')).map(f=>({file:path.join(directory,f),fields:fs.readFileSync(path.join(directory,f),'utf8').trimEnd().split('\\t')})).filter(r=>r.fields[1]==='running');\n"
                " const targets = native.filter(r=>Number(r.fields[2])===daemon.pid || Number(r.fields[2])===herdr.pid);\n"
                " const shells = herdr.panes.map(pane=>(herdr.run(['pane','process-info','--pane',pane]) as {result:{process_info:{shell_pid:number}}}).result.process_info.shell_pid);\n"
                " const value = {worker:process.pid,roots:[herdr.root,path.dirname(daemon.stateDir)],native:targets,shells,supervisors:[herdr.supervisorPid,daemon.supervisorPid]};\n"
                " const file = " + json.dumps(str(proof)) + "; fs.writeFileSync(file+'.tmp',JSON.stringify(value)); fs.renameSync(file+'.tmp',file);\n"
                " await new Promise(()=>{});\n});\n")
            observations = []
            process = None
            try:
                with (artifacts / 'ordinary-worker.log').open('w') as output:
                    process = subprocess.Popen(['bash', 'scripts/verify-web.sh', 'web', 'e2e', filename.name,
                        '--retries=0', '--workers=1'], cwd=ROOT, stdout=output, stderr=subprocess.STDOUT)
                    data = until(lambda: json.loads(proof.read_text()) if proof.exists() else None, seconds=20)
                    self.assertEqual(len(data['native']), 2)
                    # Each root is emitted by its ordinary fixture, and both
                    # original target PIDs are observed alive before the kill.
                    observations = [ProcessObservation(int(row['fields'][2])) for row in data['native']]
                    observations += [ProcessObservation(pid) for pid in data['shells']]
                    observations += [ProcessObservation(pid) for pid in data['supervisors']]
                    self.assertTrue(all(not target.exited() for target in observations))
                    os.kill(data['worker'], 9) if os.name != 'nt' else subprocess.run(
                        ['taskkill', '/PID', str(data['worker']), '/F'], check=True, capture_output=True, timeout=2)
                    # Deliberately no /T or process-group signal: only the
                    # actual Playwright worker is ended, not runner/helpers.
                    finished = [until(lambda file=Path(row['file']): (value if value['phase']=='owner-lost-exited' else None)
                        if (value:=receipt(file)) else None) for row in data['native']]
                    self.assertTrue(all(row['survivors']==0 for row in finished))
                    until(lambda: all(target.exited() for target in observations))
                    self.assertTrue(all(not Path(root).exists() for root in data['roots']))
                    self.assertNotEqual(process.wait(timeout=10), 0)
                    (artifacts / 'ordinary-worker.json').write_text(json.dumps({**data, 'finished':finished}, indent=2))
                    owned.confirmed = True
            finally:
                primary = sys.exc_info()[1]
                filename.unlink(missing_ok=True)
                if process is not None and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=10)
                if observations:
                    try:
                        until(lambda: all(target.exited() for target in observations))
                        owned.confirmed = True
                    except BaseException as cleanup:
                        if primary is None: raise
                        primary.add_note(f'Ordinary fixture cleanup also failed: {cleanup}')
                for target in observations: target.close()


if __name__ == '__main__':
    unittest.main()
