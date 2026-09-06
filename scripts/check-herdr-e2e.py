#!/usr/bin/env python3
"""Exercise bundled startup, protocol refusal and recovery on a private socket."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import pwd
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--bundle', default=str(next((ROOT / 'macos/build/assembled').glob('*.app'), ROOT / 'macos/build/assembled/hide.app')))
parser.add_argument('--output', default=str(Path(subprocess.check_output(['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'], cwd=ROOT, text=True).strip()).parent / 'agents/runs/herdr-runtime-release'))
parser.add_argument('--capture-human', action='store_true', help='Capture the selected pane for human review; not part of the machine oracle')
args = parser.parse_args()
bundle = Path(args.bundle).resolve()
out = Path(args.output).resolve()
out.mkdir(parents=True, exist_ok=True)
pin = json.loads((ROOT / 'macos/Sources/HerdrMacOS/Resources/herdr-bundle.json').read_text())
binary = bundle / 'Contents/Resources/herdr-runtime/herdr'
assert hashlib.sha256(binary.read_bytes()).hexdigest() == pin['sha256'], 'bundle digest differs'
app = bundle / 'Contents/MacOS/HerdrMacOS'
stable = out / 'stable-herdr'
subprocess.run(['/usr/bin/curl', '--fail', '--location', '--silent', '--show-error',
    'https://github.com/herdrdev/herdr/releases/download/v0.8.2/herdr-macos-aarch64', '--output', str(stable)], check=True)
assert hashlib.sha256(stable.read_bytes()).hexdigest() == 'a5d4f4d504d8b309c91f811050559300faba31258425f53c50852fc96f6ae574', 'stable digest differs'
stable.chmod(0o755)
processes = []
results = {}
with tempfile.TemporaryDirectory(prefix='he-', dir='/tmp') as directory:
    private = Path(directory)
    home = private / 'home'
    home.mkdir()
    checkout = home / 'checkout'
    checkout.mkdir()
    subprocess.run(['/usr/bin/git', 'init', '--quiet', str(checkout)], check=True)
    socket = private / 'h.sock'
    assert not socket.exists() and len(str(socket).encode()) < 100
    env = {'HOME': str(home), 'USER': pwd.getpwuid(os.getuid()).pw_name,
           'PATH': '/usr/bin:/bin', 'HERDR_SOCKET_PATH': str(socket)}
    def run(binary_path, *argv):
        return subprocess.run([str(binary_path), *argv], env=env, capture_output=True, text=True, timeout=10)
    def launch(stage):
        log = out / f'e2e-{stage}.log'
        with log.open('w') as stream:
            process = subprocess.Popen([str(app), '--state-path', str(home / 'state.json'), '--workspace-root', str(checkout)], env=env, stdout=stream, stderr=stream)
        processes.append(process)
        return process, log
    def stop_app(process):
        if process.poll() is None:
            process.terminate()
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill(); process.wait()
    def wait_for(label, predicate):
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                print(f'e2e.{label}', flush=True)
                return value
            time.sleep(.3)
        raise RuntimeError(f'e2e.{label} timed out; see {out}')
    def snapshot():
        result = run(binary, 'api', 'snapshot')
        if result.returncode: return None
        return json.loads(result.stdout).get('result', {}).get('snapshot')
    def connected(process, stage):
        snap = wait_for(f'{stage}.snapshot', lambda: snapshot())
        assert snap['version'] == pin['version'], snap.get('version')
        if not snap['panes']:
            created = run(binary, 'workspace', 'create', '--cwd', str(checkout), '--label', 'Runtime verification', '--focus')
            assert created.returncode == 0, created.stderr
            snap = wait_for(f'{stage}.pane', lambda: (value if (value := snapshot()) and value['panes'] else None))
        (out / f'e2e-{stage}-snapshot.json').write_text(json.dumps(snap, indent=2))
        def status():
            log = subprocess.run(['/usr/bin/log', 'show', '--last', '2m', '--style', 'ndjson', '--info',
                 '--predicate', f'processID == {process.pid} AND eventMessage CONTAINS "herdr.status"'], capture_output=True, text=True, timeout=15)
            (out / f'e2e-{stage}-status.log').write_text(log.stdout)
            return 'detail=connected' in log.stdout
        wait_for(f'{stage}.connected', status)
        results[stage] = {'pid': process.pid, 'version': snap['version'], 'panes': len(snap['panes'])}
    try:
        process, _ = launch('primary')
        connected(process, 'primary')
        if args.capture_human:
            # Only this run's PID can supply the human-review screenshot.
            swift = 'import CoreGraphics\nlet pid = Int32(CommandLine.arguments[1])!\nfor w in CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? [] { if (w[kCGWindowOwnerPID as String] as? Int32) == pid, (w[kCGWindowLayer as String] as? Int) == 0 { print(w[kCGWindowNumber as String]!); break } }'
            window = subprocess.check_output(['/usr/bin/swift', '-e', swift, str(process.pid)], text=True).strip()
            assert window, 'no window for isolated app PID'
            seen = subprocess.check_output(['/opt/homebrew/bin/peekaboo', 'see', '--pid', str(process.pid), '--window-id', window, '--json', '--path', str(out / 'before-select.png')], text=True)
            (out / 'before-select.json').write_text(seen)
            receipt = json.loads(seen)
            assert receipt['target_receipt']['pid'] == process.pid
            rows = [row for row in receipt['data']['ui_elements'] if row.get('identifier', '').startswith('hide-checkout-')]
            assert len(rows) == 1, 'isolated checkout row is ambiguous'
            subprocess.run(['/opt/homebrew/bin/peekaboo', 'click', '--pid', str(process.pid), '--window-id', window, '--snapshot', receipt['data']['snapshot_id'], '--on', rows[0]['id'], '--json'], check=True, stdout=subprocess.DEVNULL)
            after = subprocess.check_output(['/opt/homebrew/bin/peekaboo', 'see', '--pid', str(process.pid), '--window-id', window, '--json', '--path', str(out / 'after-select.png')], text=True)
            (out / 'after-select.json').write_text(after)
    
    
            subprocess.run(['/usr/sbin/screencapture', '-x', '-l', window, str(out / 'connected.png')], check=True)
        stop_app(process)
        assert run(binary, 'server', 'stop').returncode == 0
        wait_for('primary.stopped', lambda: not socket.exists())
        with (out / 'e2e-stable-server.log').open('w') as stream:
            old = subprocess.Popen([str(stable), 'server'], env=env, stdout=stream, stderr=stream)
        processes.append(old)
        wait_for('foreign.started', lambda: socket.exists())
        process, log = launch('failure')
        wait_for('failure.protocol_mismatch', lambda: 'protocol_mismatch' in log.read_text())
        assert run(stable, 'api', 'snapshot').returncode == 0, 'app stopped foreign server'
        results['failure'] = {'state': 'protocol_mismatch', 'foreign_server_survived': True}
        stop_app(process)
        assert run(stable, 'server', 'stop').returncode == 0
        wait_for('foreign.stopped', lambda: not socket.exists())
        process, _ = launch('recovery')
        connected(process, 'recovery')
        (out / 'e2e-result.json').write_text(json.dumps(results, indent=2))
    finally:
        for process in reversed(processes): stop_app(process)
        if socket.exists():
            stopped = run(binary, 'server', 'stop')
            if stopped.returncode:
                raise RuntimeError('isolated server cleanup failed: ' + stopped.stderr)
