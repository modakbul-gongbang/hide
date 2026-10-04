"""Real reporter/selection controls, with no browser or product daemon."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('quarantine', ROOT / 'scripts/ci-quarantine.py')
quarantine = importlib.util.module_from_spec(spec)
spec.loader.exec_module(quarantine)


class PlaywrightContracts(unittest.TestCase):
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
import {{ cleanupAfterFailure, ownUntilWorkerExit }} from {json.dumps(worker)};
for (const value of ['A', 'B']) test('primary ' + value, () => {{
  cleanupAfterFailure(new Error('Expected: primary ' + value + ' setup assertion'), () => {{ throw new Error('cleanup EBUSY owned executable'); }});
}});
test('cleanup only', () => {{
  let fail = true;
  const owner = ownUntilWorkerExit(() => {{ if(fail) throw new Error('cleanup-only owned exit unconfirmed'); }});
  try {{ owner.stop(); }} finally {{ fail = false; owner.stop(); }}
}});
""")
            failures, report = run('errors', 'errors.spec.ts', expected=1)
            rows = failures['records']
            self.assertEqual(len(rows), 3)
            self.assertTrue(all(r['status'] == 'failed' for r in rows))
            primaries = [r for r in rows if r['test'].startswith('primary')]
            self.assertEqual(len({r['signature'] for r in primaries}), 2)
            for row in primaries:
                self.assertEqual(row['category'], 'assertion')
                self.assertIn('Expected: primary', row['assertion'])
                self.assertIn('cleanup EBUSY owned executable', json.dumps(row['causes']))
            errors = [s['tests'][0]['results'][0]['error'] for suite in report['suites'] for s in suite['specs']]
            for error in errors[:2]:
                self.assertIn('Expected: primary', error['message'])
                self.assertIn('Expected: primary', error['stack'])
                self.assertIn('cleanup EBUSY owned executable', json.dumps(error['cause']))
            self.assertIn('cleanup-only owned exit unconfirmed', rows[2]['assertion'])


if __name__ == '__main__':
    unittest.main()
