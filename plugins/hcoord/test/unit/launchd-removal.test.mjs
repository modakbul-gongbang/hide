import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { createRequire } from "node:module";
const { removeLaunchAgent } = createRequire(import.meta.url)("../../dist/support/launchd.js");

function agent(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "launchd-remove-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const plistPath = path.join(root, "owned.plist");
  fs.writeFileSync(plistPath, "original service");
  return { label: "com.hcoord.daemon.test", plistPath };
}

for (const status of [null, 77]) {
  test(`removal preserves the plist when launchctl cannot establish ownership state (${status})`, (t) => {
    const target = agent(t);
    const asked = [];
    const result = removeLaunchAgent(target, { uid: 123, launchctl: (args) => {
      asked.push(args);
      return { status, stdout: "", stderr: "launchctl refused the query" };
    } });
    assert.match(result.problem, /could not read/);
    assert.equal(result.plist, "kept");
    assert.equal(fs.readFileSync(target.plistPath, "utf8"), "original service");
    assert.deepEqual(asked, [["print", "gui/123/com.hcoord.daemon.test"]]);
  });
}

test("a refused bootout reports the failure and retains the service definition", (t) => {
  const target = agent(t);
  const result = removeLaunchAgent(target, { launchctl: ([verb]) => ({ status: verb === "print" ? 0 : 77, stdout: "", stderr: "bootout refused" }) });
  assert.match(result.problem, /bootout failed/);
  assert.equal(result.plist, "kept");
  assert.equal(fs.existsSync(target.plistPath), true);
});

test("a missing label and plist are already uninstalled", (t) => {
  const target = agent(t);
  fs.rmSync(target.plistPath);
  const result = removeLaunchAgent(target, { launchctl: () => ({ status: 113, stdout: "", stderr: "Could not find service" }) });
  assert.equal(result.problem, null);
  assert.equal(result.plist, "absent");
  assert.deepEqual(result.launchctl, []);
});
