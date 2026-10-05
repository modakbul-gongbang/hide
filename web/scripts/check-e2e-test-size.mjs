#!/usr/bin/env node
// A size budget for one e2e test, with a baseline that only shrinks for the tests already over it.
//
//   node check-e2e-test-size.mjs <e2e dir>                  check (exit 1 on a violation)
//   node check-e2e-test-size.mjs <e2e dir> --write-baseline rewrite <e2e dir>/test-size-baseline.json from the tree
//   node check-e2e-test-size.mjs <e2e dir> --report         print every test's size, largest first
//
// Run from the package that owns the directory (web or desktop); TypeScript is resolved from there.
// A test is `test("title", ...)` at any depth in a `*.spec.ts` of the directory.
// Size is the line span of that call and the number of `expect`, `expect.soft` and `expect.poll` calls inside it.
// Assertions made in a helper the test calls are not counted, which is why the line limit sits beside the expect limit.
//
// Rules (docs/TESTING.md, Writing a Playwright e2e test, step 5):
//   1. A test over the limit that is not in the baseline fails: split independent contracts into their own specs.
//   2. A baselined test that grew past its recorded size fails: the baseline is a ceiling, not an allowance.
//   3. A baseline entry whose test is gone, or fits the limit now, fails until the entry is removed.
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const ts = createRequire(path.resolve("package.json"))("typescript");
const LIMIT = { lines: 120, expect: 40 };
const [directory, mode] = process.argv.slice(2);
if (!directory) {
  console.error("usage: check-e2e-test-size.mjs <e2e dir> [--write-baseline|--report]");
  process.exit(2);
}
const BASELINE = path.join(directory, "test-size-baseline.json");

// The head call of an assertion: expect(x), expect.soft(x), expect.poll(fn). The matcher call after it is not counted again.
function isExpect(node) {
  if (!ts.isCallExpression(node)) return false;
  let head = node.expression;
  while (ts.isPropertyAccessExpression(head)) head = head.expression;
  if (!ts.isIdentifier(head) || head.text !== "expect") return false;
  return !ts.isPropertyAccessExpression(node.expression) || ["soft", "poll"].includes(node.expression.name.text);
}

function measure(file) {
  const source = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true);
  const found = [];
  const seen = new Map();
  const visit = (node) => {
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "test") {
      const [title] = node.arguments;
      if (title && (ts.isStringLiteral(title) || ts.isNoSubstitutionTemplateLiteral(title))) {
        const start = source.getLineAndCharacterOfPosition(node.getStart()).line + 1;
        const end = source.getLineAndCharacterOfPosition(node.getEnd()).line + 1;
        let expects = 0;
        const count = (inner) => {
          if (isExpect(inner)) expects += 1;
          ts.forEachChild(inner, count);
        };
        ts.forEachChild(node, count);
        const n = (seen.get(title.text) ?? 0) + 1;
        seen.set(title.text, n);
        found.push({ key: `${path.basename(file)}::${title.text}${n > 1 ? ` #${n}` : ""}`, line: start, lines: end - start + 1, expect: expects });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return found;
}

const tests = fs.readdirSync(directory).filter((name) => name.endsWith(".spec.ts")).sort().flatMap((name) => measure(path.join(directory, name)));
const over = (t) => t.lines > LIMIT.lines || t.expect > LIMIT.expect;

if (mode === "--report") {
  for (const t of [...tests].sort((a, b) => b.lines - a.lines)) console.log(`${String(t.lines).padStart(4)} lines ${String(t.expect).padStart(4)} expect  ${t.key}`);
  process.exit(0);
}
if (mode === "--write-baseline") {
  const entries = Object.fromEntries(tests.filter(over).map((t) => [t.key, { lines: t.lines, expect: t.expect }]));
  fs.writeFileSync(BASELINE, `${JSON.stringify({ limit: LIMIT, tests: entries }, null, 2)}\n`);
  console.log(`${Object.keys(entries).length} tests over the budget written to ${BASELINE}`);
  process.exit(0);
}

const baseline = fs.existsSync(BASELINE) ? JSON.parse(fs.readFileSync(BASELINE, "utf8")).tests : {};
const byKey = new Map(tests.map((t) => [t.key, t]));
const errors = [];
for (const t of tests.filter(over)) {
  const allowed = baseline[t.key];
  const size = `${t.lines} lines, ${t.expect} expects (limit ${LIMIT.lines} / ${LIMIT.expect})`;
  if (!allowed) errors.push(`${directory}/${t.key}:${t.line} is ${size}; split the independent contracts into their own specs`);
  else if (t.lines > allowed.lines || t.expect > allowed.expect) errors.push(`${directory}/${t.key}:${t.line} grew to ${size}; the baseline allows ${allowed.lines} / ${allowed.expect}`);
}
for (const key of Object.keys(baseline)) {
  const t = byKey.get(key);
  if (!t) errors.push(`${BASELINE}: ${key} has no test; remove the entry`);
  else if (!over(t)) errors.push(`${BASELINE}: ${key} now fits the budget (${t.lines} / ${t.expect}); remove the entry`);
}
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log(`ok: ${tests.length} tests in ${directory}, ${Object.keys(baseline).length} baselined over the budget`);
