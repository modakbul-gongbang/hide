import type { FullConfig, FullResult, Reporter, Suite, TestCase, TestError, TestResult, TestStep } from '@playwright/test/reporter';
// Shared by both Playwright suites; no product/runtime instrumentation.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const ledger = require('./ci-ledger.cjs');
const registry = require('../contracts/ci-quarantine.json');
function primarySteps(result: TestResult) {
  const primary = result.errors[0];
  if (!primary) return [];
  let visited = 0;
  function locate(steps: TestStep[], depth: number): TestStep | undefined {
    if (depth > 32) throw new Error('reporter step depth cap exceeded');
    for (const step of steps) {
      if (++visited > 10_000) throw new Error('reporter step inventory cap exceeded');
      const child = locate(step.steps, depth + 1);
      if (child) return child;
      if (step.error?.message === primary.message && step.error?.stack === primary.stack) return step;
    }
  }
  const found = locate(result.steps, 0), path: TestStep[] = [];
  for (let step = found; step; step = step.parent) {
    if (path.length >= 32) throw new Error('reporter step parent depth cap exceeded');
    path.unshift(step);
  }
  return path.map(step => ({ title: step.title, location: step.location, params: step.category === 'test.step' ? step.params : undefined }));
}
export default class LedgerReporter implements Reporter {
  private rows = new Map<string, Record<string, unknown>>();
  private source = ledger.identity();
  private filename = process.env.CI_LEDGER_PATH || '../agents/runs/ci-ledger/' + process.pid + '.json';
  private offset = Number(process.env.CI_REPEAT_OFFSET || 0);
  private globalErrors = 0;
  private collectionErrors = 0;
  private selected = 0;

  private subject(test: TestCase, retry: number) {
    return { ...this.source, suite: test.location.file.replace(/\\/g, '/').replace(/^.*\/(web|desktop)\//, '$1/'),
      project: test.parent.project()?.name || '', test: test.title,
      // Reporter suites include the empty root, project and file ancestors.
      titlePath: test.titlePath().slice(3),
      repeat: test.repeatEachIndex + this.offset, retry, expected: test.expectedStatus, seed: null };
  }
  private set(test: TestCase, retry: number, row: Record<string, unknown>) {
    const key = JSON.stringify([test.id, retry]);
    if (!this.rows.has(key) && this.rows.size >= ledger.MAX_ROWS) throw new Error('ledger row cap exceeded');
    this.rows.set(key, row);
  }
  private flush(outcome = 'unknown', complete = false) {
    ledger.write(this.filename, { ...ledger.merge([...this.rows.values()]), outcome,
      collectionErrors: this.collectionErrors,
      collection: complete && this.rows.size && !this.globalErrors && !this.collectionErrors && outcome !== 'interrupted' && ![...this.rows.values()].some(row => row.status === 'unknown')
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
    this.selected = suite.allTests().length;
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
    let steps: ReturnType<typeof primarySteps> = [];
    let collectionError: { phase: string; message: string; stack?: string } | undefined;
    try { steps = primarySteps(result); }
    catch (error) {
      this.collectionErrors++;
      collectionError = { phase: 'primary-steps',
        message: String(error instanceof Error ? error.message : error).slice(0, 16000),
        stack: error instanceof Error ? error.stack?.slice(0, 64000) : undefined };
    }
    const contractPhase = steps.slice().reverse().find(step => step.title.startsWith('native:'))?.title || null;
    const row = { ...this.subject(test, result.retry), worker: result.workerIndex,
      status: result.status, phase: 'completed', durationMs: result.duration,
      startedAt: result.startTime.toISOString(), completedAt: new Date(result.startTime.getTime() + result.duration).toISOString(),
      category: result.status === 'skipped' ? 'skipped' : result.status === 'interrupted' ? 'cancelled' : ledger.category(assertion),
      assertion: assertion.slice(0, 16000), signature: assertion ? ledger.signature(assertion, contractPhase) : null,
      contractPhase,
      failure: { location: result.errors[0]?.location, stack: result.errors[0]?.stack?.slice(0, 64000), steps },
      collectionErrors: collectionError ? [collectionError] : [],
      causes: [...result.errors.slice(1), ...result.errors.flatMap(error => error.cause ? [error.cause] : [])] };
    this.set(test, result.retry, { ...row, quarantine: ledger.quarantine(row, registry) });
    this.flush();
  }
  onEnd(result: FullResult) {
    if (!this.selected) this.rows.set('empty-selection', { ...this.source, suite: 'Playwright runner', project: '',
      test: 'empty selected inventory', repeat: 0, retry: 0, status: 'unknown', phase: 'empty-selection', category: 'collection',
      assertion: 'Playwright selected no scenarios', signature: ledger.signature('Playwright selected no scenarios') });
    this.flush(result.status, true);
    if (this.globalErrors || this.collectionErrors || !this.rows.size || [...this.rows.values()].some(row => row.status === 'unknown')) return { status: 'failed' as const };
  }
}
