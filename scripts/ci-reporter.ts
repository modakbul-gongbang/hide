import type { FullResult, Reporter, TestCase, TestResult } from '@playwright/test/reporter';
// Shared by both Playwright suites; no product/runtime instrumentation.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const ledger = require('./ci-ledger.cjs');
const registry = require('../contracts/ci-quarantine.json');
export default class LedgerReporter implements Reporter {
  private rows: unknown[] = [];
  onTestEnd(test: TestCase, result: TestResult) {
    if (this.rows.length >= ledger.MAX_ROWS) throw new Error('ledger row cap exceeded');
    const assertion = result.errors.map(error => error.message || '').join('\n');
    const row = { ...ledger.identity(), suite: test.location.file.replace(/\\/g, '/').replace(/^.*\/(web|desktop)\//, '$1/'),
      test: test.title, repeat: test.repeatEachIndex, retry: result.retry, worker: result.workerIndex,
      status: result.status, expected: test.expectedStatus, durationMs: result.duration,
      startedAt: result.startTime.toISOString(), completedAt: new Date(result.startTime.getTime() + result.duration).toISOString(), seed: null,
      category: result.status === 'skipped' ? 'skipped' : ledger.category(assertion),
      assertion: assertion.slice(0, 16000), signature: assertion ? ledger.signature(assertion) : null,
      causes: result.errors.flatMap(error => error.cause ? [error.cause] : []) };
    this.rows.push({...row, quarantine: ledger.quarantine(row, registry)});
  }
  onEnd(result: FullResult) {
    const filename = process.env.CI_LEDGER_PATH || '../agents/runs/ci-ledger/' + process.pid + '.json';
    ledger.write(filename, { ...ledger.merge(this.rows), outcome: result.status, collection: this.rows.length ? 'observed' : 'unknown-no-tests' });
    if (!this.rows.length) return { status: 'failed' as const };
  }
}
