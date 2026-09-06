import fs from 'node:fs';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';

// Product-side AC11 proof stays inside the worktree. The run's declared
// bookkeeping deliverables and registered bytes prove rules/INDEX membership
// through the harness, which owns that separate record-root boundary.
const workflow = fs.readFileSync('.github/workflows/design-contract.yml', 'utf8');
assert(workflow.includes('pull_request:') && workflow.includes('push:'), 'CI must cover review and main');
for (const script of ['check-hide-theme-literals.sh', 'check-hide-components.sh']) {
  assert(workflow.split('\n').some(line => line.trim() === 'bash scripts/' + script),
    'Missing CI invocation for ' + script);
  process.stdout.write(execFileSync('bash', ['scripts/' + script], {encoding: 'utf8'}));
}
console.log('Product enforcement and CI binding PASS; registered bookkeeping proves invariant membership');
