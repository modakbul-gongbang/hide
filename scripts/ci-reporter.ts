import type { FullConfig, FullResult, Reporter, Suite, TestCase, TestError, TestResult } from '@playwright/test/reporter';
// Shared by both Playwright suites; no product/runtime instrumentation.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const ledger = require('./ci-ledger.cjs');
const registry = require('../contracts/ci-quarantine.json');
export default class LedgerReporter implements Reporter {
  private rows = new Map<string, Record<string, unknown>>();
  private source = ledger.identity();
  private filename = process.env.CI_LEDGER_PATH || '../agents/runs/ci-ledger/' + process.pid + '.json';
  private offset = Number(process.env.CI_REPEAT_OFFSET || 0);
  private globalErrors = 0;

  private subject(test: TestCase, retry: number) {
    return { ...this.source, suite: test.location.file.replace(/\\/g, '/').replace(/^.*\/(web|desktop)\//, '$1/'),
      project: test.parent.project()?.name || '', test: test.title,
      repeat: test.repeatEachIndex + this.offset, retry, expected: test.expectedStatus, seed: null };
  }
  private set(test: TestCase, retry: number, row: Record<string, unknown>) {
    const key = JSON.stringify([test.id, retry]);
    if (!this.rows.has(key) && this.rows.size >= ledger.MAX_ROWS) throw new Error('ledger row cap exceeded');
    this.rows.set(key, row);
  }
  private flush(outcome = 'unknown', complete = false) {
    ledger.write(this.filename, { ...ledger.merge([...this.rows.values()]), outcome,
      collection: complete && this.rows.size && !this.globalErrors && outcome !== 'interrupted' && ![...this.rows.values()].some(row => row.status === 'unknown')
        ? 'observed' : 'partial-or-unknown' });
  }
  onError(error: TestError) {
    if (this.rows.size >= ledger.MAX_ROWS) throw new Error('ledger row cap exceeded');
    const assertion = error.message || error.value || 'unidentified Playwright global error';
    const index = this.globalErrors++;
    this.rows.set('global:' + index, { ...this.source, suite: 'Playwright runner', project: '',
      test: 'global error ' + index, repeat: 0, retry: 0, expected: 'passed',
      status: 'failed', phase: 'global-error', category: ledger.category(assertion),
      assertion: assertion.slice(0, 16000), signature: ledger.signature(assertion),
      failure: { location: error.location, stack: error.stack?.slice(0, 64000) }, causes: error.cause ? [error.cause] : [] });
    this.flush('failed');
  }
  onBegin(_config: FullConfig, suite: Suite) {
    if (!Number.isInteger(this.offset) || this.offset < 0 || this.offset > 29) throw new Error('invalid repetition offset');
    for (const test of suite.allTests()) this.set(test, 0, { ...this.subject(test, 0),
      status: 'unknown', phase: 'scheduled', worker: null, durationMs: null,
      startedAt: null, completedAt: null, category: 'unknown', assertion: '', signature: null, causes: [] });
    this.flush();
  }
  onTestBegin(test: TestCase, result: TestResult) {
    this.set(test, result.retry, { ...this.subject(test, result.retry), status: 'unknown', phase: 'in-flight',
      worker: result.workerIndex, durationMs: null, startedAt: result.startTime.toISOString(), completedAt: null,
      category: 'unknown', assertion: '', signature: null, causes: [] });
    this.flush();
  }
  onTestEnd(test: TestCase, result: TestResult) {
    const assertion = result.errors[0]?.message || '';
    const row = { ...this.subject(test, result.retry), worker: result.workerIndex,
      status: result.status, phase: 'completed', durationMs: result.duration,
      startedAt: result.startTime.toISOString(), completedAt: new Date(result.startTime.getTime() + result.duration).toISOString(),
      category: result.status === 'skipped' ? 'skipped' : result.status === 'interrupted' ? 'cancelled' : ledger.category(assertion),
      assertion: assertion.slice(0, 16000), signature: assertion ? ledger.signature(assertion) : null,
      failure: { location: result.errors[0]?.location, stack: result.errors[0]?.stack?.slice(0, 64000) },
      causes: [...result.errors.slice(1), ...result.errors.flatMap(error => error.cause ? [error.cause] : [])] };
    this.set(test, result.retry, { ...row, quarantine: ledger.quarantine(row, registry) });
    this.flush();
  }
  onEnd(result: FullResult) {
    this.flush(result.status, true);
    if (this.globalErrors || !this.rows.size || [...this.rows.values()].some(row => row.status === 'unknown')) return { status: 'failed' as const };
  }
}
