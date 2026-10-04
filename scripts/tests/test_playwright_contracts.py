"""Real reporter/selection controls, with no browser or product daemon."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import signal
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('quarantine', ROOT / 'scripts/ci-quarantine.py')
quarantine = importlib.util.module_from_spec(spec)
spec.loader.exec_module(quarantine)


class PlaywrightContracts(unittest.TestCase):
    def test_killed_reporter_preserves_completed_and_in_flight_results(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/reporter-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            root = Path(directory)
            tests = root / 'web/e2e'
            tests.mkdir(parents=True)
            package = (ROOT / 'web/node_modules/@playwright/test').as_posix()
            config = root / 'playwright.config.ts'
            config.write_text(f"""import {{ defineConfig }} from {json.dumps(package)};
export default defineConfig({{testDir:{json.dumps(str(tests))},workers:1,retries:0,
reporter:[[{json.dumps(str(ROOT / 'scripts/ci-reporter.ts'))}]]}});
""")
            (tests / 'interrupted.spec.ts').write_text(f"""import {{ test }} from {json.dumps(package)};
test('completed before termination', () => {{}});
test('in flight at termination', async () => {{ await new Promise(() => {{}}); }});
test('never started', () => {{}});
""")
            for termination in (signal.SIGKILL, signal.SIGINT):
                filename = root / f'{termination.name}.json'
                with (artifacts / f'{termination.name}.log').open('w') as output:
                    process = subprocess.Popen(['bash', 'scripts/verify-web.sh', 'web', 'e2e', '--config', str(config)], cwd=ROOT,
                        env={**os.environ, 'CI_LEDGER_PATH': str(filename)}, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
                    try:
                        deadline = time.monotonic() + 30
                        while time.monotonic() < deadline:
                            if filename.exists():
                                value = json.loads(filename.read_text())
                                phases = {r['test']: (r['status'], r['phase']) for r in value['records']}
                                if phases.get('completed before termination') == ('passed', 'completed') and phases.get('in flight at termination') == ('unknown', 'in-flight'):
                                    break
                            if process.poll() is not None:
                                self.fail('Playwright exited before the controlled interruption')
                            time.sleep(.01)
                        else:
                            self.fail('Playwright did not reach the controlled in-flight boundary')
                        os.killpg(process.pid, termination)
                        self.assertNotEqual(process.wait(timeout=10), 0)
                    finally:
                        if process.poll() is None:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait(timeout=5)
                value = json.loads(filename.read_text())
                (artifacts / f'{termination.name}.ledger.json').write_text(filename.read_text())
                rows = {r['test']: r for r in value['records']}
                self.assertEqual(rows['completed before termination']['status'], 'passed')
                self.assertIn(rows['in flight at termination']['status'], ('unknown', 'interrupted'))
                self.assertNotEqual(rows['never started']['status'], 'passed')
                self.assertEqual(value['summary']['firstPass'], 1)
                if termination == signal.SIGKILL:
                    self.assertEqual(value['collection'], 'partial-or-unknown')
                    self.assertEqual(rows['in flight at termination']['phase'], 'in-flight')
                    self.assertEqual(rows['never started']['phase'], 'scheduled')

    def test_serialized_primary_and_cleanup_and_exact_selection(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/reporter-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            root = Path(directory)
            tests = root / 'web/e2e'
            tests.mkdir(parents=True)
            package = (ROOT / 'web/node_modules/@playwright/test').as_posix()
            worker = (ROOT / 'web/e2e/worker-owned.ts').as_posix()
            config = root / 'playwright.config.ts'
            config.write_text(f"""import {{ defineConfig }} from {json.dumps(package)};
export default defineConfig({{testDir:{json.dumps(str(tests))},workers:1,retries:0,
reporter:[['json'],[{json.dumps(str(ROOT / 'scripts/ci-reporter.ts'))}]],
outputDir:{json.dumps(str(root / 'results'))}}});
""")
            entry = {'id': 'control', 'suite': 'web', 'file': 'web/e2e/registered.spec.ts', 'title': 'registered scenario', 'oses': ['Linux']}
            (tests / 'registered.spec.ts').write_text(f"""import {{ test }} from {json.dumps(package)};
test('registered scenario', () => {{}});
test('registered scenario with extra words', () => {{}});
""")
            (tests / 'unregistered.spec.ts').write_text(f"""import {{ test }} from {json.dumps(package)};
test('registered scenario', () => {{}});
""")
            selected = quarantine.selection([entry], 'web', 'Linux', 'required', {'quarantine_required': ['control']})
            listing = root / 'selected.txt'
            listing.write_text(quarantine.test_list(selected))

            def run(label, *args, expected=0):
                ledger = root / f'{label}.ledger.json'
                report = root / f'{label}.report.json'
                process = subprocess.run(['bash', 'scripts/verify-web.sh', 'web', 'e2e', '--config', str(config), *args], cwd=ROOT,
                    env={**os.environ, 'CI_LEDGER_PATH': str(ledger), 'PLAYWRIGHT_JSON_OUTPUT_FILE': str(report), 'RUNNER_OS': 'Linux'},
                    capture_output=True, text=True, timeout=60)
                (artifacts / f'{label}.log').write_text(process.stdout + process.stderr)
                self.assertEqual(process.returncode, expected, process.stdout + process.stderr)
                value = json.loads(ledger.read_text())
                # Keep actual serialized output and ledger as local evidence.
                (artifacts / f'{label}.ledger.json').write_text(ledger.read_text())
                (artifacts / f'{label}.report.json').write_text(report.read_text())
                return value, json.loads(report.read_text())

            excluded, _ = run('excluded', '--test-list-invert', str(listing))
            self.assertEqual({(r['suite'], r['test']) for r in excluded['records']}, {
                ('web/e2e/unregistered.spec.ts', 'registered scenario'),
                ('web/e2e/registered.spec.ts', 'registered scenario with extra words')})
            for mode in ('required', 'advisory'):
                observed, _ = run(mode, '--test-list', str(listing))
                self.assertEqual(len(observed['records']), 1)
                quarantine.results(selected, observed, mode == 'required', 'Linux')
                with self.assertRaisesRegex(ValueError, 'missing'):
                    quarantine.results(selected, excluded, mode == 'required', 'Linux')

            # Exercise thirty identities through the installed reporter. A grep
            # group with only its first member must not satisfy the controls.
            contract = json.loads((ROOT / 'contracts/ci-failure-controls.json').read_text())
            focus = contract['scenarios']['focus']['tests']
            (tests / 'pane-focus-ordering.spec.ts').write_text(
                f'import {{ test }} from {json.dumps(package)};\n' +
                ''.join(f'test({json.dumps(title)}, () => {{}});\n' for _, title in focus))
            observed, _ = run('repeat-identities', 'pane-focus-ordering.spec.ts', '--repeat-each=30')
            check = """const fs=require('node:fs'), c=require('./scripts/ci-controls.cjs');
const value=JSON.parse(fs.readFileSync(process.argv[1]));
for(const row of value.records) row.os='Windows';
const source={sha:value.records[0].sha,run:value.records[0].run,runAttempt:value.records[0].runAttempt};
c.results('focus','Windows',value,source);
const partial={...value,records:value.records.filter(row=>row.test===value.records[0].test)};
try { c.results('focus','Windows',partial,source); throw Error('partial group accepted'); }
catch(error) { if(!error.message.includes('incomplete controls')) throw error; }
"""
            checked = subprocess.run(['node', '-e', check, str(root / 'repeat-identities.ledger.json')], cwd=ROOT, capture_output=True, text=True, timeout=10)
            self.assertEqual(checked.returncode, 0, checked.stderr)

            (tests / 'errors.spec.ts').write_text(f"""import {{ test }} from {json.dumps(package)};
import {{ cleanupAfterFailure, finishFixture, ownUntilWorkerExit }} from {json.dumps(worker)};
for (const value of ['A', 'B']) test('primary ' + value, () => {{
  cleanupAfterFailure(new Error('Expected: primary ' + value + ' setup assertion'), () => {{ throw new Error('cleanup EBUSY owned executable'); }});
}});
test('cleanup only', () => {{
  let fail = true;
  const owner = ownUntilWorkerExit(() => {{ if(fail) throw new Error('cleanup-only owned exit unconfirmed'); }});
  try {{ owner.stop(); }} finally {{ fail = false; owner.stop(); }}
}});
test('primary native cause and every release', async () => {{
  const original = new Error('Expected: primary fixture setup', {{cause:new Error('clang.exe ETIMEDOUT')}});
  const released: string[] = [];
  try {{
    await finishFixture(original, [
      () => {{ released.push('daemon'); throw new Error('daemon exit unconfirmed'); }},
      () => {{ released.push('server'); throw new Error('server cleanup EBUSY'); }},
      () => {{ released.push('socket'); }},
    ]);
  }} finally {{
    if (released.join(',') !== 'daemon,server,socket') throw new Error('not all owned resources were released');
  }}
}});
test.describe('separate caller phases', () => {{
  test.afterEach(() => {{ throw new Error('secondary teardown exit unconfirmed'); }});
  test('separate primary and teardown', () => {{ throw new Error('Expected: separate primary assertion'); }});
}});
""")
            failures, report = run('errors', 'errors.spec.ts', expected=1)
            rows = failures['records']
            self.assertEqual(len(rows), 5)
            self.assertTrue(all(r['status'] == 'failed' for r in rows))
            primaries = [r for r in rows if r['test'] in ('primary A', 'primary B')]
            self.assertEqual(len({r['signature'] for r in primaries}), 2)
            for row in primaries:
                self.assertEqual(row['category'], 'assertion')
                self.assertIn('Expected: primary', row['assertion'])
                self.assertIn('cleanup EBUSY owned executable', json.dumps(row['causes']))
            def results_in(suites):
                return [s['tests'][0]['results'][0] for suite in suites for s in suite.get('specs', [])] + [result for suite in suites for result in results_in(suite.get('suites', []))]
            actual_results = results_in(report['suites'])
            errors = [result['error'] for result in actual_results]
            for error in errors[:2]:
                self.assertIn('Expected: primary', error['message'])
                self.assertIn('Expected: primary', error['stack'])
                self.assertIn('cleanup EBUSY owned executable', json.dumps(error['cause']))
            self.assertIn('cleanup-only owned exit unconfirmed', rows[2]['assertion'])
            native = next(r for r in rows if r['test'] == 'primary native cause and every release')
            self.assertIn('Expected: primary fixture setup', native['assertion'])
            actual = errors[3]
            self.assertIn('Expected: primary fixture setup', actual['stack'])
            for detail in ('clang.exe ETIMEDOUT', 'daemon exit unconfirmed', 'server cleanup EBUSY'):
                self.assertIn(detail, json.dumps(actual['cause']))
                self.assertIn(detail, json.dumps(native['causes']))
            separate = next(r for r in rows if r['test'] == 'separate primary and teardown')
            self.assertEqual(separate['assertion'], 'Error: Expected: separate primary assertion')
            self.assertIn('secondary teardown exit unconfirmed', json.dumps(separate['causes']))
            self.assertIn('secondary teardown exit unconfirmed', json.dumps(actual_results[4]['errors']))


if __name__ == '__main__':
    unittest.main()
