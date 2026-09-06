import {execFileSync} from 'node:child_process';
import fs from 'node:fs';
const result = execFileSync('npx', ['@google/design.md', 'lint', 'DESIGN.md'], {encoding: 'utf8'});
process.stdout.write(result);
const verdict = JSON.parse(result);
if (verdict.summary.errors !== 0 || verdict.summary.warnings !== 0) {
  throw new Error('DESIGN.md requires zero errors and zero warnings');
}
const document = fs.readFileSync('DESIGN.md', 'utf8');
for (const required of ['## In-Product Components', 'Do not use native `.help()`', 'essence:']) {
  if (!document.includes(required)) throw new Error('Missing design contract: ' + required);
}
if (document.includes('**No hover states documented**') || document.includes('属于:')) {
  throw new Error('Obsolete design policy remains');
}
