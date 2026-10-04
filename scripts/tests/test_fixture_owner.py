"""Native supervisor controls; kill only the worker, never its process group."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import shutil
import shlex
import signal
import sys
import tempfile
import time
import unittest
import traceback

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


@unittest.skipUnless(__name__ == '__main__' or os.environ.get('CI_FIXTURE_OWNER_CONTROLS') == '1',
    'native fixture controls run in their prepared three-OS workflow')
class FixtureOwner(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        record = {'version': 1, 'phase': 'verification-shell-lookup', 'status': 'in-flight',
            'sha': os.environ.get('GITHUB_SHA', 'local'), 'os': sys.platform}
        try:
            selected = os.environ.get('CI_FIXTURE_BASH')
            record['shellSource'] = 'CI_FIXTURE_BASH' if selected else 'PATH'
            if os.name == 'nt' and not selected:
                raise RuntimeError('Windows native controls require CI_FIXTURE_BASH from the Actions Git Bash shell')
            selected = selected or shutil.which('bash')
            if not selected or not Path(selected).is_absolute() or not Path(selected).is_file():
                raise RuntimeError('verification Bash is unavailable or not an absolute executable')
            cls.shell = str(Path(selected).resolve())
            record['shell'] = cls.shell
            version = subprocess.run([cls.shell, '--version'], check=True, capture_output=True, text=True, timeout=5)
            record['shellVersion'] = version.stdout.splitlines()[0]
            record['phase'] = 'native-supervisor-build'
            result = subprocess.run([cls.shell, 'scripts/verify-cargo.sh', 'build', '-p', 'hide-platform', '--example', 'fixture-owner'], cwd=ROOT)
            if result.returncode:
                raise RuntimeError(f'native fixture supervisor build failed: exit={result.returncode}')
            record['status'] = 'passed'
        except BaseException as error:
            record.update(status='failed', error=str(error), stack=traceback.format_exc())
            raise
        finally:
            (artifacts / 'setup.json').write_text(json.dumps(record, indent=2)+'\n')

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
                    if mode == 'process-inventory':
                        command = [str(HELPER), '--probe-tree', str(running['pid'])]
                        queried = subprocess.run(command, check=True, capture_output=True, text=True, timeout=5)
                        fields = queried.stdout.strip().split('\t')
                        self.assertEqual(fields[:3], ['v1', str(running['pid']), running['birth']])
                        self.assertEqual(int(fields[3]), 1)
                        self.assertGreater(int(fields[4]), 0)
                        again = subprocess.run(command + [running['birth']], check=True, capture_output=True, text=True, timeout=5)
                        self.assertEqual(again.stdout.strip().split('\t')[:4], fields[:4])
                        refused = subprocess.run(command + [str(int(running['birth']) + 1)], capture_output=True, text=True, timeout=5)
                        self.assertNotEqual(refused.returncode, 0)
                        self.assertIn('fixture probe launch identity changed', refused.stderr)
                        self.assertTrue(all(not observed.exited() for observed in observations))
                        (artifacts / 'process-inventory-probe.json').write_text(json.dumps({
                            'original': running, 'inventory': fields, 'refused': refused.stderr,
                            'status': refused.returncode, 'ownedProcessesStillLive': True}, indent=2)+'\n')
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

    def test_native_process_inventory_refuses_reused_identity_without_signalling(self):
        self.exercise('process-inventory')

    def test_worker_only_hard_kill_confirms_zero_owned_survivors(self):
        self.exercise('worker-hard-kill')

    def test_preparation_shell_output_does_not_substitute_for_owned_exit(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        facts = []
        for mode in ['complete', 'timeout', 'worker-hard-kill']:
            with OwnedControlRoot(artifacts) as owned:
                root = owned.path
                proof, launch, outcome = [root / name for name in ['targets.json', 'owner.json', 'outcome.json']]
                target = root / 'target.cjs'
                target.write_text("const fs=require('node:fs'),{spawn}=require('node:child_process');\n"
                    "const [mode,file]=process.argv.slice(2);\n"
                    "if(mode==='leaf')setInterval(()=>{},1000);\n"
                    "else if(mode==='complete'){process.stdout.write('version-output-before-exit\\n');}\n"
                    "else {const child=spawn(process.execPath,[__filename,'leaf'],{stdio:'ignore'});\n"
                    "fs.writeFileSync(file+'.tmp',JSON.stringify({target:process.pid,child:child.pid}));fs.renameSync(file+'.tmp',file);\n"
                    "process.stdout.write('version-output-before-exit\\n');setInterval(()=>{},1000);}\n")
                worker = root / 'worker.cjs'
                worker.write_text("const fs=require('node:fs'); const owned=require(" + json.dumps(str(ROOT/'scripts/ci-owned-command.cjs')) + ");\n"
                    "const [root,target,mode,proof,launch,outcome]=process.argv.slice(2);\n"
                    "const publish=(file,value)=>{fs.writeFileSync(file+'.tmp',JSON.stringify(value));fs.renameSync(file+'.tmp',file);};\n"
                    "owned.run(root,owned.shell(),['-c','exec \"$@\"','preparation-control',process.execPath,target,mode,proof],{\n"
                    "subject:'actual-tool-probe',onOwner:owner=>publish(launch,{worker:process.pid,...owner})}).then(\n"
                    "value=>publish(outcome,{status:'passed',...value}),\n"
                    "error=>{publish(outcome,{status:'failed',message:error.message,code:error.code,stdout:error.stdout,stderr:error.stderr,owner:error.owner,secondary:error.secondary||[]});process.exitCode=1;});\n")
                observations = []
                process = subprocess.Popen([shutil.which('node'),str(worker),str(ROOT),str(target),mode,str(proof),str(launch),str(outcome)],
                    cwd=ROOT,env={**os.environ,'CI_FIXTURE_BASH':self.shell},stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
                data = None
                try:
                    data = until(lambda: json.loads(launch.read_text()) if launch.exists() else None)
                    if mode != 'complete':
                        ids = until(lambda: json.loads(proof.read_text()) if proof.exists() else None)
                        running = until(lambda: value if (value:=receipt(Path(data['receipt']))) and value['phase']=='running' else None)
                        # On Windows, Git Bash can retain its own native PID
                        # while the tool is its child. Observe both identities.
                        original = list(dict.fromkeys([running['pid'],ids['target'],ids['child'],data['supervisorPid']]))
                        observations = [ProcessObservation(pid) for pid in original]
                        self.assertGreater(running['pid'],0)
                        self.assertGreater(int(running['birth']),0)
                        self.assertTrue(all(not target.exited() for target in observations))
                    if mode == 'worker-hard-kill':
                        process.kill()  # End only the Node caller; no /T or group signal.
                        process.wait(timeout=5)
                        final = until(lambda: value if (value:=receipt(Path(data['receipt']))) and value['phase']=='owner-lost-exited' else None)
                        self.assertEqual(final['survivors'],0)
                        self.assertFalse(Path(data['home']).exists())
                        self.assertFalse(outcome.exists())
                        facts.append({'mode':mode,'launch':data,'targets':ids,'finished':final,'ownedSurvivors':0})
                    else:
                        code = process.wait(timeout=10)
                        result = json.loads(outcome.read_text())
                        self.assertEqual(result['status'],'passed' if mode=='complete' else 'failed')
                        self.assertEqual(code,0 if mode=='complete' else 1)
                        self.assertIn('version-output-before-exit',result['stdout'])
                        self.assertTrue(result['owner']['supervisorExited'])
                        self.assertEqual(result['owner']['observed']['survivors'],0)
                        self.assertFalse(Path(data['home']).exists())
                        if mode=='timeout':
                            self.assertEqual(result['code'],'ETIMEDOUT')
                            self.assertIn('5000ms',result['message'])
                            self.assertEqual(result['secondary'],[])
                        facts.append({'mode':mode,'launch':data,'outcome':result,'ownedSurvivors':0})
                    until(lambda: all(target.exited() for target in observations))
                    owned.confirmed = True
                finally:
                    primary = sys.exc_info()[1]
                    if process.poll() is None: process.kill(); process.wait(timeout=5)
                    if data:
                        try:
                            final = until(lambda: value if (value:=receipt(Path(data['receipt']))) and value['survivors']==0 else None)
                            until(lambda: all(target.exited() for target in observations))
                            owned.confirmed = True
                        except BaseException as cleanup:
                            if primary is None: raise
                            primary.add_note(f'Preparation control cleanup also failed: {cleanup}')
                    for observed in observations: observed.close()
                    process.stdout.close(); process.stderr.close()
        (artifacts/'preparation-command.json').write_text(json.dumps(facts,indent=2)+'\n')

    def test_wrong_launch_identity_refuses_a_real_live_target(self):
        self.exercise('identity-refusal')

    def test_late_preparation_probe_preserves_primary_and_unknown_exit_receipts(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        facts = []
        modes = ['timeout'] + (['exit-unconfirmed'] if os.name != 'nt' else [])
        for mode in modes:
            with OwnedControlRoot(artifacts) as owned:
                root = owned.path
                for relative in ['scripts', 'contracts', 'tools', 'target/debug/examples']:
                    (root / relative).mkdir(parents=True, exist_ok=True)
                shutil.copy2(HELPER, root / 'target/debug/examples' / HELPER.name)
                compiled = []
                for name, kind in [('hided', 'bin'), ('hide', 'bin'), ('fixture-owner', 'example')]:
                    executable = root / 'target/debug' / ('examples' if kind == 'example' else '') / (name + ('.exe' if os.name == 'nt' else ''))
                    if kind == 'bin': executable.write_text('controlled external producer output')
                    compiled.append({'reason': 'compiler-artifact', 'target': {'name': name, 'kind': [kind]},
                        'profile': {'test': False}, 'features': [], 'executable': str(executable)})
                # Cargo's output is controlled at the external producer boundary;
                # the CLI, chosen shell and native supervisor remain real.
                (root / 'scripts/verify-cargo.sh').write_text("#!/usr/bin/env bash\ncat <<'ARTIFACTS'\n"+
                    '\n'.join(json.dumps(row) for row in compiled)+"\nARTIFACTS\n")
                for relative in ['Cargo.lock', 'pnpm-lock.yaml', 'contracts/herdr-bundle.json']:
                    (root / relative).write_text('unchanged control input')
                (root / '.gitignore').write_text('target/\nagents/\n')
                probe = root / 'tools/probe.cjs'
                probe.write_text("const fs=require('node:fs'),path=require('node:path'),{spawn}=require('node:child_process');\n"
                    "if(process.argv[2]==='leaf'){setInterval(()=>{},1000);}\n"
                    "else {const dir=path.resolve('agents/probe');fs.mkdirSync(dir,{recursive:true});\n"
                    "const counter=path.join(dir,'counter');const n=fs.existsSync(counter)?Number(fs.readFileSync(counter)):0;\n"
                    "fs.writeFileSync(counter,String(n+1));process.stdout.write('controlled-rust-version\\n');\n"
                    "if(n){const child=spawn(process.execPath,[__filename,'leaf'],{stdio:'ignore'});\n"
                    "const file=path.join(dir,'targets.json');fs.writeFileSync(file+'.tmp',JSON.stringify({target:process.pid,child:child.pid}));\n"
                    "fs.renameSync(file+'.tmp',file);setInterval(()=>{},1000);}}\n")
                shim = root / 'tools/rustc'
                shim.write_text('#!/usr/bin/env bash\nexec node '+shlex.quote(probe.as_posix())+' "$@"\n')
                shim.chmod(0o755)
                for args in [['init', '--quiet'], ['add', '.'], ['-c', 'core.hooksPath=/dev/null', '-c',
                        'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '--quiet', '-m', 'Initial control']]:
                    subprocess.run(['git', *args], cwd=root, check=True, capture_output=True, text=True)
                source = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
                environment = {**os.environ, 'CI_FIXTURE_BASH': self.shell,
                    'PATH': str(root / 'tools') + os.pathsep + os.environ['PATH']}
                environment.pop('GITHUB_SHA', None)
                state = root / 'agents/runs/ci-preparation/ledger-build.json'
                observations = []
                helper = None
                suspended = False
                with (artifacts / ('preparation-late-'+mode+'.log')).open('w') as output:
                    process = subprocess.Popen([shutil.which('node'), str(ROOT / 'scripts/ci-preparation.cjs'), 'build'],
                        cwd=root, env=environment, stdout=output, stderr=subprocess.STDOUT)
                    try:
                        ids = until(lambda: json.loads((root/'agents/probe/targets.json').read_text())
                            if (root/'agents/probe/targets.json').exists() else None)
                        data = until(lambda: value if (value:=json.loads(state.read_text()))['records'][0]['phase']=='identity:rustc:-vV'
                            and value['records'][0]['commandOwner'] else None)
                        launch = data['records'][0]['commandOwner']
                        running = until(lambda: value if (value:=receipt(Path(launch['receipt']))) and value['phase']=='running' else None)
                        helper = launch['supervisorPid']
                        observations = [ProcessObservation(pid) for pid in dict.fromkeys([running['pid'], ids['target'], ids['child'], helper])]
                        self.assertTrue(all(not observed.exited() for observed in observations))
                        if mode == 'exit-unconfirmed':
                            os.kill(helper, signal.SIGSTOP)  # Suspend only this control's original supervisor.
                            suspended = True
                        self.assertEqual(process.wait(timeout=10), 1)
                        actual = json.loads(state.read_text())
                        row = actual['records'][0]
                        self.assertEqual(row['sha'], source)
                        self.assertEqual(row['phase'], 'identity:rustc:-vV')
                        self.assertEqual(row['status'], 'failed')
                        self.assertEqual(row['failure']['code'], 'ETIMEDOUT')
                        self.assertEqual(row['assertion'], 'preparation command rustc timed out after 5000ms')
                        self.assertEqual(row['failure']['message'], row['assertion'])
                        self.assertIn('controlled-rust-version', row['failure']['stdout']['text'])
                        self.assertFalse((root/'agents/runs/ci-preparation/build.json').exists())
                        self.assertEqual(row['commandOwner']['supervisorPid'], helper)
                        if mode == 'exit-unconfirmed':
                            self.assertEqual(actual['collection'], 'partial-or-unknown')
                            self.assertFalse(row['commandOwner']['supervisorExited'])
                            self.assertEqual(row['commandOwner']['observed']['survivors'], -1)
                            self.assertTrue(any('owned exit unconfirmed' in item['message'] for item in row['failure']['secondary']))
                            self.assertTrue(Path(launch['home']).exists())
                            self.assertTrue(all(not observed.exited() for observed in observations))
                        else:
                            self.assertEqual(actual['collection'], 'complete')
                            self.assertTrue(row['commandOwner']['supervisorExited'])
                            self.assertEqual(row['commandOwner']['observed']['survivors'], 0)
                            self.assertEqual(row['failure']['secondary'], [])
                            self.assertFalse(Path(launch['home']).exists())
                        (artifacts / ('preparation-late-'+mode+'-ledger.json')).write_text(json.dumps(actual, indent=2)+'\n')
                        facts.append({'mode': mode, 'source': source, 'targets': ids, 'running': running, 'ledger': actual})
                    finally:
                        primary = sys.exc_info()[1]
                        if suspended: os.kill(helper, signal.SIGCONT)
                        if process.poll() is None: process.kill(); process.wait(timeout=5)
                        try:
                            if observations:
                                until(lambda: all(observed.exited() for observed in observations))
                                owned.confirmed = True
                        except BaseException as cleanup:
                            if primary is None: raise
                            primary.add_note(f'Late preparation control cleanup also failed: {cleanup}')
                        for observed in observations: observed.close()
        (artifacts/'preparation-late-probe.json').write_text(json.dumps(facts, indent=2)+'\n')

    def test_native_stop_request_error_preserves_primary_cleanup_and_home(self):
        self.exercise('stop-directory')

    @unittest.skipUnless(os.name != 'nt', 'Unix exec wrapper controls API admission; Windows ordinary consumers run separately')
    def test_ordinary_fixture_observes_initial_api_admission_and_retains_refusal(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        invocation = Path(tempfile.mkdtemp(dir=artifacts, prefix='api-admission-result-'))
        (invocation/'source.json').write_text(json.dumps({name: hashlib.sha256((ROOT/name).read_bytes()).hexdigest()
            for name in ['scripts/ci-owned-command.cjs', 'web/e2e/herdr-fixture.ts', 'scripts/tests/test_fixture_owner.py']}, indent=2)+'\n')
        actual = os.environ.get('HIDE_E2E_HERDR_BIN') or os.environ.get('HERDR_BIN_PATH') or shutil.which('herdr')
        self.assertTrue(actual)
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            root = Path(directory)
            proof = root / 'api-admission.jsonl'
            wrappers = {}
            for mode in ['pending', 'refused', 'malformed']:
                executable = root / ('herdr-'+mode)
                executable.write_text('#!'+sys.executable+'\n'+
                    'import json, os, pathlib, sys\n'+
                    'real='+repr(str(Path(actual).resolve()))+'\n'+
                    'mode='+repr(mode)+'\n'+
                    'counter=pathlib.Path('+repr(str(root/(mode+'.count')))+')\n'+
                    'if sys.argv[1:]==["server"]:\n'+
                    ' with pathlib.Path('+repr(str(proof))+').open("a") as out: out.write(json.dumps({"mode":mode,"serverPid":os.getpid(),"root":str(pathlib.Path(os.environ["HERDR_CONFIG_PATH"]).parent)})+"\\n")\n'+
                    'if sys.argv[1:]==["api","snapshot"]:\n'+
                    ' n=int(counter.read_text()) if counter.exists() else 0; counter.write_text(str(n+1))\n'+
                    ' with pathlib.Path('+repr(str(proof))+').open("a") as out: out.write(json.dumps({"mode":mode,"query":n,"pid":os.getpid()})+"\\n")\n'+
                    ' if mode=="malformed": print(json.dumps({"result":{}}));sys.exit(0)\n'+
                    ' if mode=="refused" or n==0:\n'+
                    '  print(json.dumps({"id":"cli:api:snapshot","error":{"code":"server_not_running" if mode=="pending" else "refused","message":"controlled API admission "+mode}}),file=sys.stderr);sys.exit(1)\n'+
                    'os.execv(real,[real,*sys.argv[1:]])\n')
                executable.chmod(0o755)
                wrappers[mode] = str(executable)
            filename = ROOT/'web/e2e'/('ci-api-admission-'+root.name+'.spec.ts')
            report = invocation/'report.json'
            ledger = invocation/'ledger.json'
            fixture_facts = invocation/'fixtures.jsonl'
            fixture_facts.write_text('')
            filename.write_text("import {test,expect} from '@playwright/test';import fs from 'node:fs';\n"
                "import {startHerdr} from './herdr-fixture';import {finishFixture} from './worker-owned';\n"
                "const wrappers="+json.dumps(wrappers)+";\n"
                "for(const mode of ['pending','refused','malformed'] as const)test('initial API '+mode,async()=>{\n"
                " process.env.HIDE_E2E_HERDR_BIN=wrappers[mode];let herdr:Awaited<ReturnType<typeof startHerdr>>|undefined;let failure:unknown;let primary:unknown;\n"
                " try {try{herdr=await startHerdr({agents:false});}catch(error){failure=error;}\n"
                " if(mode==='pending'){expect(failure).toBeUndefined();expect(herdr!.panes).toHaveLength(2);}\n"
                " else {expect(herdr).toBeUndefined();expect((failure as Error).message).toContain(mode==='refused'?'controlled API admission refused':'initial snapshot has no workspace inventory');}\n"
                " fs.appendFileSync("+json.dumps(str(fixture_facts))+",JSON.stringify({mode,pid:herdr?.pid,root:herdr?.root,message:(failure as Error)?.message,stack:(failure as Error)?.stack})+'\\n');\n"
                " }catch(error){primary=error;}finally{await finishFixture(primary,[()=>herdr?.stop()]);}\n});\n")
            try:
                result = subprocess.run([self.shell, 'scripts/verify-web.sh', 'web', 'e2e', filename.name,
                    '--retries=0', '--workers=1', '--reporter=list,'+str(ROOT/'scripts/ci-reporter.ts')+',json'], cwd=ROOT,
                    env={**os.environ,'PLAYWRIGHT_JSON_OUTPUT_FILE':str(report),'CI_LEDGER_PATH':str(ledger)},
                    capture_output=True, text=True, timeout=60)
                (invocation/'run.log').write_text(result.stdout+result.stderr)
                self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
                rows = [row for row in json.loads(ledger.read_text())['records'] if row['suite']==filename.relative_to(ROOT).as_posix()]
                self.assertEqual(len(rows), 3)
                self.assertTrue(all(row['status']=='passed' and row['retry']==0 for row in rows))
                evidence = [json.loads(line) for line in proof.read_text().splitlines()]
                queries = [row for row in evidence if 'query' in row]
                self.assertEqual([row['query'] for row in queries if row['mode']=='pending'], [0, 1])
                self.assertEqual([row['query'] for row in queries if row['mode']=='refused'], [0])
                self.assertEqual([row['query'] for row in queries if row['mode']=='malformed'], [0])
                servers = [row for row in evidence if 'serverPid' in row]
                self.assertEqual(len(servers), 3)
                self.assertTrue(all(ProcessObservation(row['serverPid']).exited() and not Path(row['root']).exists() for row in servers))
                (invocation/'queries.json').write_text(json.dumps(evidence, indent=2)+'\n')
                facts = [json.loads(line) for line in fixture_facts.read_text().splitlines()]
                self.assertEqual(len(facts), 3)
                self.assertFalse(Path(facts[0]['root']).exists())
                self.assertTrue(ProcessObservation(facts[0]['pid']).exited())
            finally:
                filename.unlink(missing_ok=True)

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
                    process = subprocess.Popen([self.shell, 'scripts/verify-web.sh', 'web', 'e2e', filename.name,
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

    def test_actual_consumers_release_confirmed_homes_and_serialize_primary_errors(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            proof = Path(directory) / 'consumer-release.json'
            filename = ROOT / 'web/e2e' / ('ci-owner-release-' + Path(directory).name + '.spec.ts')
            report = artifacts / 'consumer-release.report.json'
            normalized = artifacts / 'consumer-release.ledger.json'
            filename.write_text("import { test, expect } from '@playwright/test';\n"
                "import fs from 'node:fs'; import path from 'node:path';\n"
                "import { startHerdr } from './herdr-fixture'; import { startHided } from './hided-fixture';\n"
                "const proof=" + json.dumps(str(proof)) + ";\n"
                "test('Herdr evidence failure still releases confirmed root', async()=>{\n"
                " const herdr=await startHerdr({agents:false}); const saved=process.env.HIDE_E2E_SCREENSHOT_DIR;\n"
                " const blocked=proof+'.blocked'; fs.writeFileSync(blocked,'original'); process.env.HIDE_E2E_SCREENSHOT_DIR=blocked;\n"
                " let primary:unknown; try { herdr.stop(); } catch(error) {primary=error;} finally {if(saved===undefined)delete process.env.HIDE_E2E_SCREENSHOT_DIR;else process.env.HIDE_E2E_SCREENSHOT_DIR=saved;}\n"
                " expect(primary).toBeInstanceOf(Error); expect((primary as Error).message).toContain('EEXIST');\n"
                " expect(fs.existsSync(herdr.root)).toBe(false); fs.appendFileSync(proof,JSON.stringify({consumer:'herdr',rootRemoved:true,message:(primary as Error).message})+'\\n');\n"
                " throw primary;\n});\n"
                "test('Hided native stop failure still releases confirmed root', async()=>{\n"
                " const herdr=await startHerdr({agents:false}); const daemon=await startHided(herdr);\n"
                " const directory=path.resolve('../agents/runs/ci-fixture-owners');\n"
                " const receipt=fs.readdirSync(directory).filter(f=>f.endsWith('.receipt')).map(f=>path.join(directory,f)).find(f=>fs.readFileSync(f,'utf8').split('\\t')[2]===String(daemon.pid));\n"
                " expect(receipt).toBeDefined(); fs.mkdirSync(receipt!.replace(/\\.receipt$/,'.stop'));\n"
                " await expect.poll(()=>fs.readFileSync(receipt!,'utf8').split('\\t')[1]).toBe('stop-request-read-failed');\n"
                " let primary:unknown; try {daemon.stop();} catch(error){primary=error;}\n"
                " expect(primary).toBeInstanceOf(Error); expect((primary as Error).message).toContain('stop-request-read-failed');\n"
                " expect((primary as Error).message).toContain('stop-request-cleanup-failed');\n"
                " expect(fs.readFileSync(receipt!,'utf8').split('\\t')[5]).toBe('0');\n"
                " expect(fs.existsSync(path.dirname(daemon.stateDir))).toBe(false); herdr.stop();\n"
                " fs.appendFileSync(proof,JSON.stringify({consumer:'hided',rootRemoved:true,message:(primary as Error).message})+'\\n'); throw primary;\n});\n")
            if os.name != 'nt':
                with filename.open('a') as source:
                    source.write("test('unconfirmed native exit preserves a live consumer home',async()=>{\n"
                        " const herdr=await startHerdr({agents:false});const daemon=await startHided(herdr);\n"
                        " const directory=path.resolve('../agents/runs/ci-fixture-owners');\n"
                        " const receipt=fs.readdirSync(directory).filter(f=>f.endsWith('.receipt')).map(f=>path.join(directory,f)).find(f=>fs.readFileSync(f,'utf8').split('\\t')[2]===String(daemon.pid))!;\n"
                        " let primary:unknown;process.kill(daemon.supervisorPid,'SIGSTOP');\n"
                        " try {try {daemon.stop();}catch(error){primary=error;}\n"
                        " expect(primary).toBeInstanceOf(Error);expect((primary as Error).message).toContain('unconfirmed');\n"
                        " expect(fs.existsSync(path.dirname(daemon.stateDir))).toBe(true);expect((await fetch(daemon.origin+'/health')).ok).toBe(true);\n"
                        " fs.appendFileSync(proof,JSON.stringify({consumer:'unconfirmed',homePreservedWhileLive:true,message:(primary as Error).message})+'\\n');\n"
                        " }finally {process.kill(daemon.supervisorPid,'SIGCONT');}\n"
                        " await expect.poll(()=>fs.readFileSync(receipt,'utf8').split('\\t')[1]).toBe('owner-lost-exited');\n"
                        " expect(fs.readFileSync(receipt,'utf8').split('\\t')[5]).toBe('0');herdr.stop();throw primary;\n});\n")
            try:
                result = subprocess.run([self.shell, 'scripts/verify-web.sh', 'web', 'e2e', filename.name, '--retries=0', '--workers=1',
                    '--reporter=list,'+str(ROOT/'scripts/ci-reporter.ts')+',json'], cwd=ROOT,
                    env={**os.environ,'PLAYWRIGHT_JSON_OUTPUT_FILE':str(report),'CI_LEDGER_PATH':str(normalized)},
                    capture_output=True, text=True, timeout=60)
                (artifacts/'consumer-release.log').write_text(result.stdout+result.stderr)
                self.assertNotEqual(result.returncode, 0)
                facts=[json.loads(line) for line in proof.read_text().splitlines()]
                expected={'herdr','hided'} if os.name=='nt' else {'herdr','hided','unconfirmed'}
                self.assertEqual({row['consumer'] for row in facts},expected)
                self.assertTrue(all(row['rootRemoved'] for row in facts if row['consumer']!='unconfirmed'))
                self.assertTrue(all(row['homePreservedWhileLive'] for row in facts if row['consumer']=='unconfirmed'))
                actual=json.loads(report.read_text())
                self.assertEqual(actual['stats']['unexpected'],len(expected))
                rows=[row for row in json.loads(normalized.read_text())['records'] if row['suite']==filename.relative_to(ROOT).as_posix()]
                self.assertEqual(len(rows),len(expected))
                self.assertTrue(all(row['status']=='failed' for row in rows))
                self.assertTrue(any('EEXIST' in row['assertion'] for row in rows))
                self.assertTrue(any('stop-request-read-failed' in row['assertion'] and 'stop-request-cleanup-failed' in row['assertion'] for row in rows))
            finally:
                filename.unlink(missing_ok=True)

    def test_actual_native_receivers_refuse_missing_duplicate_and_wrong_pane_bytes(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/fixture-owner-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            proof = Path(directory) / 'native-input.jsonl'
            filename = ROOT / 'web/e2e' / ('ci-native-input-' + Path(directory).name + '.spec.ts')
            filename.write_text("import {test,expect} from '@playwright/test';\n"
                "import fs from 'node:fs'; import {execFileSync} from 'node:child_process'; import {startHerdr} from './herdr-fixture';\n"
                "import {nativeInputReceivers} from './native-input-receiver'; import {fixtureProcessTree} from './platform-fixture'; import {finishFixture} from './worker-owned';\n"
                "for(const mode of ['missing','duplicate','wrong-pane','exact']) test('native receipt '+mode,async()=>{\n"
                " const herdr=await startHerdr({agents:false});let primary:unknown;\n"
                " try {const inventory=await fixtureProcessTree(herdr.pid,5000,herdr.env);expect(inventory.descendants).toBeGreaterThanOrEqual(2);\n"
                " expect(await fixtureProcessTree(herdr.pid,5000,herdr.env,inventory.birth)).toEqual(inventory);\n"
                " if(mode==='wrong-pane')await expect(fixtureProcessTree(herdr.pid,5000,herdr.env,String(BigInt(inventory.birth)+1n))).rejects.toThrow('fixture probe launch identity changed');\n"
                " const receivers=await nativeInputReceivers(herdr,herdr.panes);\n"
                " const line='printf receiver-control'; const bytes=Buffer.from(line+'\\r');\n"
                " const received=new Map(herdr.panes.map(pane=>[pane,Buffer.alloc(0)]));\n"
                " const expected=new Map(received);expected.set(herdr.panes[0],bytes);\n"
                " if(mode!=='missing'){const pane=herdr.panes[mode==='wrong-pane'?1:0];\n"
                " const text=mode==='duplicate'?line+'\\r'+line+'\\r':line+'\\r';execFileSync(herdr.bin,['pane','send-text',pane,text],{env:herdr.env,timeout:30000});received.set(pane,Buffer.from(text));}\n"
                " await expect.poll(()=>receivers.read()).toEqual(received);\n"
                " if(mode==='exact')receivers.confirm(expected);else expect(()=>receivers.confirm(expected)).toThrow('native byte receipt mismatch');\n"
                " fs.appendFileSync("+json.dumps(str(proof))+",JSON.stringify({mode,panes:herdr.panes,receipt:receivers.receipt(),received:[...receivers.read()].map(([pane,bytes])=>({pane,hex:bytes.toString('hex')})),rejected:mode!=='exact'})+'\\n');\n"
                " }catch(error){primary=error;}finally{await finishFixture(primary,[()=>herdr.stop()]);}\n});\n")
            try:
                result = subprocess.run([self.shell,'scripts/verify-web.sh','web','e2e',filename.name,'--retries=0','--workers=1'],
                    cwd=ROOT,capture_output=True,text=True,timeout=60)
                (artifacts/'native-input-controls.log').write_text(result.stdout+result.stderr)
                self.assertEqual(result.returncode,0,result.stdout+result.stderr)
                facts=[json.loads(line) for line in proof.read_text().splitlines()]
                self.assertEqual({row['mode'] for row in facts},{'missing','duplicate','wrong-pane','exact'})
                self.assertTrue(all(row['rejected']==(row['mode']!='exact') for row in facts))
                (artifacts/'native-input-controls.json').write_text(json.dumps(facts,indent=2)+'\n')
            finally:
                filename.unlink(missing_ok=True)


if __name__ == '__main__':
    unittest.main()
