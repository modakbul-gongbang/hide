// The stand-in for `hide-agent-hooks <pi|omp> <operation>` the extension test
// runs: it logs the agent, the operation, its input and the pane it ran in,
// then answers from HIDE_EXTENSION_HELPER_ANSWERS. `delay_ms` holds the answer
// back, `exit` ends without one, `raw` prints it unparsed.

import { appendFileSync, readFileSync } from "node:fs";

const [agent, operation] = process.argv.slice(2);
let stdin = "";
process.stdin.setEncoding("utf8");
for await (const chunk of process.stdin) stdin += chunk;
appendFileSync(
  process.env.HIDE_EXTENSION_HELPER_LOG,
  `${JSON.stringify({ agent, operation, input: JSON.parse(stdin), env: { HERDR_PANE_ID: process.env.HERDR_PANE_ID } })}\n`,
);
const { delay_ms: delay, exit, raw, ...answer } =
  JSON.parse(readFileSync(process.env.HIDE_EXTENSION_HELPER_ANSWERS, "utf8"))[operation] ?? {};
if (delay) await new Promise((resolve) => setTimeout(resolve, delay));
if (exit !== undefined) process.exit(exit);
process.stdout.write(raw ?? JSON.stringify(answer));
