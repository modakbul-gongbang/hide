// The portable fixture executable must preserve the caller's data through
// the real program boundary, including the Windows launcher.
import { beforeAll, expect, test } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { fixtureProgram } from "./platform-fixture";
import { buildFixtureShims } from "./shims/build";

beforeAll(() => {
  if (process.platform === "win32") buildFixtureShims(fileURLToPath(new URL("..", import.meta.url)));
}, 120_000);

test("fixture programs preserve Unicode paths, arguments, quoting and the child exit status", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hide-program-"));
  try {
    const executable = fixtureProgram(path.join(root, "한국어 폴더 🙂"), "echo-arguments", `
const args = process.argv.slice(2);
if (args[0] === "exit") process.exit(Number(args[1]));
process.stdout.write(JSON.stringify(args));
`);
    const args = ["issue", "create", "--title", "한국어 제목 🙂", "--body", '첫 줄\n"본문"과 C:\\fixture\\path', "", "quote\"inside", "trailing\\"];
    const options = { encoding: "utf8" as const, timeout: 10_000, windowsHide: true };
    const answer = spawnSync(executable, args, options);
    expect(answer.error).toBeUndefined();
    expect(answer.status).toBe(0);
    expect(answer.stderr).toBe("");
    expect(JSON.parse(answer.stdout)).toEqual(args);
    const refused = spawnSync(executable, ["exit", "37"], options);
    expect(refused.error).toBeUndefined();
    expect(refused.status).toBe(37);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
