// On Windows, a failed test attempt logs what holds TCP connections at that
// moment (`windowsTcpSummary`); a passing attempt runs nothing. The config
// adds this reporter only on a Windows CI run.
import type { Reporter, TestCase, TestResult } from "@playwright/test/reporter";
import { windowsTcpSummary } from "./platform-fixture";

export default class WindowsSocketsReporter implements Reporter {
  onTestEnd(test: TestCase, result: TestResult): void {
    if (result.status === test.expectedStatus) return;
    let lines: string[];
    try { lines = windowsTcpSummary(); } catch (error) { lines = [`summary unavailable: ${String(error)}`]; }
    for (const line of lines) console.log(`[windows sockets] ${test.title} (attempt ${result.retry + 1}): ${line}`);
  }
}
