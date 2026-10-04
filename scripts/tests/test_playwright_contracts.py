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
    def test_step_inventory_overflow_retains_actual_failure_and_timeout(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/reporter-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            root = Path(directory)
            tests = root / 'web/e2e'
            tests.mkdir(parents=True)
            package = (ROOT / 'web/node_modules/@playwright/test').as_posix()
            (tests / 'step-cap.spec.ts').write_text(f"""import {{test}} from {json.dumps(package)};
test('independent completed result', () => {{}});
test.afterEach(({{}}, info) => {{
  if(info.title === 'timeout beyond step cap') throw new Error('secondary page-closed release');
}});
for(const title of ['failure beyond step cap','timeout beyond step cap']) test(title,async()=>{{
  test.setTimeout(15000);
  for(let i=0;i<10001;i++) await test.step('bounded observation '+i,()=>{{}});
  if(title.startsWith('failure')) throw new Error('Expected: original assertion after observations');
  await new Promise(()=>{{}});
}});
""")
            config = root / 'playwright.config.ts'
            config.write_text(f"""import {{defineConfig}} from {json.dumps(package)};
export default defineConfig({{testDir:{json.dumps(str(tests))},workers:1,retries:0,
reporter:[['json'],[{json.dumps(str(ROOT/'scripts/ci-reporter.ts'))}]]}});
""")
            filename = artifacts / 'step-cap.ledger.json'
            report = artifacts / 'step-cap.report.json'
            result = subprocess.run(['bash','scripts/verify-web.sh','web','e2e','--config',str(config)],cwd=ROOT,
                env={**os.environ,'CI_LEDGER_PATH':str(filename),'PLAYWRIGHT_JSON_OUTPUT_FILE':str(report)},
                capture_output=True,text=True,timeout=40)
            (artifacts/'step-cap.log').write_text(result.stdout+result.stderr)
            self.assertNotEqual(result.returncode,0)
            value = json.loads(filename.read_text())
            rows = {row['test']: row for row in value['records']}
            self.assertEqual(len(rows),3)
            self.assertEqual(rows['independent completed result']['status'],'passed')
            self.assertEqual(value['collection'],'partial-or-unknown')
            self.assertEqual(value['collectionErrors'],2)
            self.assertFalse(any(row['phase'] in ('in-flight','global-error') for row in rows.values()))
            actual = json.loads(report.read_text())['suites'][0]['specs']
            for scenario in actual[1:]:
                observed = scenario['tests'][0]['results'][0]
                row = rows[scenario['title']]
                self.assertEqual(row['status'],observed['status'])
                self.assertEqual(row['durationMs'],observed['duration'])
                # JSON's `errors` includes formatted source snippets, while
                # `error` retains Playwright's original serialized identity.
                self.assertEqual(row['assertion'],observed['error']['message'])
                self.assertEqual(row['failure'].get('stack'),observed['error'].get('stack'))
                self.assertIn('inventory cap exceeded',row['collectionErrors'][0]['message'])
                checked = subprocess.run(['node','-e',
                    "const c=require('./scripts/ci-ledger.cjs');process.stdout.write(c.signature(process.argv[1]));",
                    row['assertion']],cwd=ROOT,capture_output=True,text=True,timeout=5)
                self.assertEqual(checked.returncode,0,checked.stderr)
                self.assertEqual(row['signature'],checked.stdout)
            timeout = rows['timeout beyond step cap']
            self.assertEqual(timeout['status'],'timedOut')
            self.assertIn('15000ms exceeded',timeout['assertion'])
            self.assertIn('secondary page-closed release',json.dumps(timeout['causes']))

    def test_actual_reporter_keeps_primary_native_phase_and_refuses_same_message_in_other_phases(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/reporter-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        entry = json.loads((ROOT / 'contracts/ci-quarantine.json').read_text())['entries'][0]
        phases = ['native:blur-return-focus', 'native:rebound-focus', 'native:cancel-return-focus',
            'native:cancel-preview', 'native:blur-preview', None]
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            root = Path(directory)
            tests = root / 'desktop/e2e'
            tests.mkdir(parents=True)
            package = (ROOT / 'web/node_modules/@playwright/test').as_posix()
            (tests / 'browser.spec.ts').write_text(f"""import {{test,expect}} from {json.dumps(package)};
const phases={json.dumps(phases)};
test({json.dumps(entry['title'])},async()=>{{
  const phase=phases[test.info().repeatEachIndex];
  const fail=()=>{{
    if(phase?.includes('preview')) throw new Error("expect(locator).toBeVisible() failed: [data-cycle=area] element(s) not found");
    expect(false).toBe(true);
  }};
  if(phase) await test.step(phase,fail);else fail();
}});
""")
            config = root / 'playwright.config.ts'
            config.write_text(f"""import {{defineConfig}} from {json.dumps(package)};
export default defineConfig({{testDir:{json.dumps(str(tests))},workers:1,retries:0,repeatEach:6,
reporter:[['json'],[{json.dumps(str(ROOT/'scripts/ci-reporter.ts'))}]]}});
""")
            ledger = artifacts / 'native-phase.ledger.json'
            report = artifacts / 'native-phase.report.json'
            result = subprocess.run(['bash','scripts/verify-web.sh','web','e2e','--config',str(config)],cwd=ROOT,
                env={**os.environ,'RUNNER_OS':'macOS','CI_LEDGER_PATH':str(ledger),'PLAYWRIGHT_JSON_OUTPUT_FILE':str(report),'FORCE_COLOR':'1'},
                capture_output=True,text=True,timeout=30)
            (artifacts/'native-phase.log').write_text(result.stdout+result.stderr)
            self.assertNotEqual(result.returncode,0)
            rows=sorted(json.loads(ledger.read_text())['records'],key=lambda row:row['repeat'])
            self.assertEqual(len(rows),6)
            self.assertEqual([row['contractPhase'] for row in rows],phases)
            self.assertTrue(all(row['status']=='failed' and row['retry']==0 for row in rows))
            self.assertEqual([row['quarantine']['classification'] for row in rows],
                ['known-signature','outside-registered-signature','outside-registered-signature',
                 'known-signature','outside-registered-signature','outside-registered-signature'])
            self.assertIn('\x1b[',rows[0]['assertion'])
            self.assertEqual(rows[0]['assertion'],rows[1]['assertion'])
            self.assertNotEqual(rows[0]['signature'],rows[1]['signature'])
            self.assertIn('browser.spec.ts',rows[0]['failure']['stack'])
            actual=json.loads(report.read_text())
            self.assertEqual(actual['stats']['unexpected'],6)
            for row in rows[:-1]:
                self.assertIn(row['contractPhase'],[step['title'] for step in row['failure']['steps']])

    def test_ordinary_configs_execute_exact_file_and_title_path_and_fail_empty_selection(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/reporter-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            root = Path(directory)
            for package in ('web', 'desktop'):
                # No replacement config: these ephemeral basic tests run under
                # each ordinary package's installed runner and actual rootDir.
                prefix = 'ci-selection-' + root.name
                registered = ROOT / package / 'e2e' / (prefix + '-registered.spec.ts')
                other = ROOT / package / 'e2e' / (prefix + '-other.spec.ts')
                title = 'exact registered scenario'
                entry = {'id': 'control', 'suite': package, 'file': registered.relative_to(ROOT).as_posix(),
                    'title': title, 'title_path': ['registered phase', title], 'oses': ['Linux']}
                registered.write_text("import { test } from '@playwright/test';\n" +
                    "test.describe('registered phase', () => {\n" +
                    f"test({json.dumps(title)}, () => {{}});\n" +
                    f"test({json.dumps(title + ' with extra words')}, () => {{}});\n}});\n" +
                    "test.describe('another phase', () => {\n" + f"test({json.dumps(title)}, () => {{}});\n}});\n")
                other.write_text("import { test } from '@playwright/test';\n" +
                    "test.describe('registered phase', () => {\n" + f"test({json.dumps(title)}, () => {{}});\n}});\n")
                try:
                    listing = root / 'selection.txt'
                    script = "const fs=require('node:fs'),c=require('./scripts/ci-controls.cjs'); fs.writeFileSync(process.argv[1],c.testList(JSON.parse(process.argv[2])));"
                    result = subprocess.run(['node', '-e', script, str(listing), json.dumps([[entry['file'], entry['title_path']]])],
                        cwd=ROOT, capture_output=True, text=True, timeout=10)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(listing.read_text(), quarantine.test_list([entry]))
                    project = ['--project', 'background', '--no-deps'] if package == 'desktop' else []

                    def run(label, flag, expected=0):
                        filename = artifacts / f'ordinary-{package}-{label}.ledger.json'
                        report = artifacts / f'ordinary-{package}-{label}.report.json'
                        result = subprocess.run(['bash', 'scripts/verify-web.sh', package, 'e2e', prefix, *project,
                            '--retries=0', flag, str(listing), '--reporter=list,' + str(ROOT / 'scripts/ci-reporter.ts') + ',json'],
                            cwd=ROOT, env={**os.environ, 'CI_LEDGER_PATH': str(filename), 'PLAYWRIGHT_JSON_OUTPUT_FILE': str(report), 'RUNNER_OS': 'Linux'},
                            capture_output=True, text=True, timeout=60)
                        (artifacts / f'ordinary-{package}-{label}.log').write_text(result.stdout + result.stderr)
                        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                        return json.loads(filename.read_text()), json.loads(report.read_text())

                    included, report = run('controls', '--test-list')
                    self.assertEqual([(r['suite'], r['titlePath'], r['status']) for r in included['records']], [(entry['file'], entry['title_path'], 'passed')])
                    self.assertEqual(report['stats']['expected'], 1)
                    for mode in ('required', 'advisory'):
                        selected = quarantine.selection([entry], package, 'Linux', mode, {f'quarantine_{"required" if mode == "required" else "observe"}': ['control']})
                        listing.write_text(quarantine.test_list(selected))
                        observed, report = run(mode, '--test-list')
                        quarantine.results(selected, observed, mode == 'required', 'Linux')
                        self.assertEqual(report['stats']['expected'], 1)
                    excluded, report = run('excluded', '--test-list-invert')
                    self.assertEqual(report['stats']['expected'], 3)
                    self.assertEqual({(r['suite'], tuple(r['titlePath'])) for r in excluded['records']}, {
                        (entry['file'], ('registered phase', title + ' with extra words')),
                        (entry['file'], ('another phase', title)),
                        (other.relative_to(ROOT).as_posix(), ('registered phase', title))})
                    for mode in ('required', 'advisory'):
                        with self.assertRaisesRegex(ValueError, 'missing'):
                            quarantine.results([entry], excluded, mode == 'required', 'Linux')
                    # Reproduce the actual remote repository-relative selector
                    # failure and keep a failing, explicitly unknown receipt.
                    listing.write_text(entry['file'] + ' > ' + ' > '.join(entry['title_path']) + '\n')
                    empty, report = run('empty', '--test-list', expected=1)
                    self.assertEqual(report['stats']['expected'], 0)
                    self.assertEqual(empty['collection'], 'partial-or-unknown')
                    self.assertEqual(empty['summary']['firstPass'], 0)
                    self.assertGreaterEqual(empty['summary']['unknown'], 1)
                    self.assertTrue(any(r['phase'] == 'empty-selection' and r['status'] == 'unknown' for r in empty['records']))
                finally:
                    registered.unlink()
                    other.unlink()

    def test_full_web_inventory_includes_registered_macos_scenarios(self):
        entries = json.loads((ROOT / 'contracts/ci-quarantine.json').read_text())['entries']
        selected = quarantine.selection(entries, 'web', 'macOS', 'exclude')
        with tempfile.TemporaryDirectory(dir=ROOT / 'agents/runs/ci-test-refactor') as directory:
            listing = Path(directory) / 'excluded.txt'
            listing.write_text(quarantine.test_list(selected))
            def inventory(*args):
                result = subprocess.run(['bash', 'scripts/verify-web.sh', 'web', 'e2e', '--list', '--reporter=json', *args],
                    cwd=ROOT, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                # The wrapper emits pnpm's command before Playwright's JSON.
                report = json.loads(result.stdout[result.stdout.index('{'):])
                def identities(suites):
                    return {(s['file'].replace('\\', '/'), s['title']) for suite in suites for s in suite.get('specs', [])} | set().union(*(identities(suite.get('suites', [])) for suite in suites))
                return identities(report['suites'])
            full = inventory()
            pr = inventory('--test-list-invert', str(listing))
            for entry in selected:
                exact = (Path(entry['file']).name, entry['title'])
                self.assertTrue(exact in full, f'missing full-suite identity: {exact}')
                self.assertTrue(exact not in pr, f'registered identity leaked into ordinary PR: {exact}')

    def test_control_selection_discovers_every_exact_real_scenario(self):
        contract = json.loads((ROOT / 'contracts/ci-failure-controls.json').read_text())
        with tempfile.TemporaryDirectory(dir=ROOT / 'agents/runs/ci-test-refactor') as directory:
            listing = Path(directory) / 'selected.txt'
            for name, scenario in contract['scenarios'].items():
                selected = subprocess.run(['node', 'scripts/ci-controls.cjs', 'select', name, scenario['oses'][0], str(listing)],
                    cwd=ROOT, capture_output=True, text=True, timeout=10)
                self.assertEqual(selected.returncode, 0, selected.stderr)
                package = scenario['tests'][0][0].split('/')[0]
                project = ['--project', 'needs-focus' if name == 'native' else 'background', '--no-deps'] if package == 'desktop' else []
                result = subprocess.run(['bash', 'scripts/verify-web.sh', package, 'e2e', '--list', '--reporter=json',
                    '--test-list', str(listing), *project], cwd=ROOT, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                report = json.loads(result.stdout[result.stdout.index('{'):])
                def identities(suites):
                    return {(s['file'].replace('\\', '/'), s['title']) for suite in suites for s in suite.get('specs', [])} | set().union(*(identities(suite.get('suites', [])) for suite in suites))
                observed = identities(report['suites'])
                wanted = {(Path(file).relative_to(f'{package}/e2e').as_posix(), title) for file, title in scenario['tests']}
                self.assertEqual(observed, wanted, name)

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

            setup = root / 'global-setup.ts'
            setup.write_text("export default () => { throw new Error('global setup native preparation denied'); };\n")
            global_config = root / 'global.config.ts'
            global_config.write_text(config.read_text().replace('testDir:', f'globalSetup:{json.dumps(str(setup))},testDir:'))
            global_result, _ = run('global-error', '--config', str(global_config), 'registered.spec.ts', expected=1)
            failures = [row for row in global_result['records'] if row['phase'] == 'global-error']
            self.assertEqual(len(failures), 1)
            self.assertIn('global setup native preparation denied', failures[0]['assertion'])
            self.assertIn('global-setup.ts', failures[0]['failure']['stack'])
            self.assertEqual(global_result['summary']['firstPass'], 0)
            self.assertEqual(global_result['collection'], 'partial-or-unknown')


    def test_desktop_focus_guard_retains_primary_and_report_io_failure(self):
        artifacts = ROOT / 'agents/runs/ci-test-refactor/reporter-controls'
        artifacts.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=artifacts) as directory:
            root = Path(directory)
            tests = root / 'desktop/e2e'
            tests.mkdir(parents=True)
            temporary = root / 'private-tmp'
            temporary.mkdir()
            package = (ROOT / 'web/node_modules/@playwright/test').as_posix()
            fixture = (ROOT / 'desktop/e2e/fixture.ts').as_posix()
            (tests / 'guard.spec.ts').write_text(f"""import {{ test }} from {json.dumps(fixture)};
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
test('primary desktop assertion with native report read failure', () => {{
  const guards=fs.readdirSync(os.tmpdir()).filter(name=>name.startsWith('hide-e2e-focus-'));
  if(guards.length!==1) throw new Error('private focus guard identity missing');
  fs.mkdirSync(path.join(os.tmpdir(),guards[0],'directory-is-not-a-report'));
  throw new Error('Expected: primary desktop assertion');
}});
""")
            config = root / 'playwright.config.ts'
            config.write_text(f"""import {{ defineConfig }} from {json.dumps(package)};
export default defineConfig({{testDir:{json.dumps(str(tests))},workers:1,retries:0,
reporter:[['json'],[{json.dumps(str(ROOT / 'scripts/ci-reporter.ts'))}]]}});
""")
            ledger = artifacts / 'desktop-guard.ledger.json'
            report = artifacts / 'desktop-guard.report.json'
            result = subprocess.run(['bash', 'scripts/verify-web.sh', 'web', 'e2e', '--config', str(config)], cwd=ROOT,
                env={**os.environ, 'TMPDIR': str(temporary), 'TMP': str(temporary), 'TEMP': str(temporary),
                    'CI_LEDGER_PATH': str(ledger), 'PLAYWRIGHT_JSON_OUTPUT_FILE': str(report)},
                capture_output=True, text=True, timeout=30)
            (artifacts / 'desktop-guard.log').write_text(result.stdout + result.stderr)
            self.assertNotEqual(result.returncode, 0)
            value = json.loads(ledger.read_text())
            self.assertEqual(len(value['records']), 1)
            row = value['records'][0]
            self.assertEqual(row['status'], 'failed')
            self.assertIn('Expected: primary desktop assertion', row['assertion'])
            self.assertIn('EISDIR', json.dumps(row['causes']))
            actual = json.loads(report.read_text())['suites'][0]['specs'][0]['tests'][0]['results'][0]
            self.assertIn('Expected: primary desktop assertion', actual['error']['message'])
            self.assertIn('guard.spec.ts', actual['error']['stack'])
            self.assertIn('EISDIR', json.dumps(actual['errors'][1:]))
            self.assertFalse(any(entry.name.startswith('hide-e2e-focus-') for entry in temporary.iterdir()))


if __name__ == '__main__':
    unittest.main()
