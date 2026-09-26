import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import {
  loginPathCommand,
  parseConnect,
  parseLoginPath,
  parseRememberedCli,
  parseStatus,
  rememberedCliPath,
  rememberedCliValue,
  resolveCli,
  type CliSearch,
  type FileProbe,
} from "./cli";
import { readJsonFile, writeJsonFile } from "./jsonFile";
import { ChildRunner } from "./spawn";
import type { ChildResult } from "./spawn";

const probe = (files: Record<string, number>): FileProbe => ({
  isExecutable: (file) => file in files,
  mtimeMs: (file) => files[file] ?? null,
});

const result = (stdout: string, extra: Partial<ChildResult> = {}): ChildResult => ({
  code: 0,
  signal: null,
  stdout,
  stderr: "",
  timedOut: false,
  spawnError: null,
  ...extra,
});

describe("resolveCli (B2)", () => {
  const input: CliSearch = { override: null, worktreeRoot: "/w", searchPath: "/usr/local/bin:/opt/bin", remembered: null, loginPath: null, home: "/h" };
  const packaged: CliSearch = { ...input, worktreeRoot: null, searchPath: "/usr/bin:/bin" };
  const login = (answer: string | null) => {
    const asked = { count: 0 };
    return { asked, loginPath: async () => (asked.count++, answer) };
  };

  it("takes the override first and never searches past an unusable one", async () => {
    expect((await resolveCli({ ...input, override: "/x/hide" }, probe({ "/x/hide": 1, "/w/target/debug/hide": 1 }))).found).toEqual({ path: "/x/hide", source: "env" });
    const missing = await resolveCli({ ...input, override: "/x/hide", remembered: "/r/hide" }, probe({ "/w/target/debug/hide": 1, "/r/hide": 1 }));
    expect(missing).toEqual({ found: null, tried: ["/x/hide"] });
  });

  it("takes the newest worktree build before PATH", async () => {
    const files = { "/w/target/debug/hide": 10, "/w/target/release/hide": 20, "/opt/bin/hide": 30 };
    expect((await resolveCli(input, probe(files))).found).toEqual({ path: "/w/target/release/hide", source: "worktree" });
  });

  it("searches PATH in order when no build exists, and reports every path it tried", async () => {
    const resolved = await resolveCli(input, probe({ "/opt/bin/hide": 1 }));
    expect(resolved.found).toEqual({ path: "/opt/bin/hide", source: "path" });
    expect(resolved.tried).toEqual(["/w/target/debug/hide", "/w/target/release/hide", "/usr/local/bin/hide", "/opt/bin/hide"]);
  });

  it("prefers PATH to the remembered CLI, and the remembered CLI to asking the login shell", async () => {
    const shell = login("/l");
    const onPath = await resolveCli({ ...packaged, searchPath: "/opt/bin", remembered: "/r/hide", ...shell }, probe({ "/opt/bin/hide": 1, "/r/hide": 1 }));
    expect(onPath.found).toEqual({ path: "/opt/bin/hide", source: "path" });
    const remembered = await resolveCli({ ...packaged, remembered: "/r/hide", ...shell }, probe({ "/r/hide": 1, "/l/hide": 1 }));
    expect(remembered).toEqual({ found: { path: "/r/hide", source: "remembered" }, tried: ["/usr/bin/hide", "/bin/hide", "/r/hide"] });
    expect(shell.asked.count).toBe(0);
  });

  it("asks the login shell once the remembered CLI is gone, skipping what PATH already tried", async () => {
    const shell = login("/usr/bin:/l");
    const resolved = await resolveCli({ ...packaged, remembered: "/r/hide", ...shell }, probe({ "/l/hide": 1 }));
    expect(resolved).toEqual({ found: { path: "/l/hide", source: "login" }, tried: ["/usr/bin/hide", "/bin/hide", "/r/hide", "/l/hide"] });
    expect(shell.asked.count).toBe(1);
  });

  it("looks in the usual install directories when the login shell does not answer", async () => {
    const resolved = await resolveCli({ ...packaged, ...login(null) }, probe({ "/opt/homebrew/bin/hide": 1 }));
    expect(resolved).toEqual({
      found: { path: "/opt/homebrew/bin/hide", source: "well-known" },
      tried: ["/usr/bin/hide", "/bin/hide", "/h/.local/bin/hide", "/opt/homebrew/bin/hide"],
    });
    const missing = await resolveCli({ ...packaged, ...login(null) }, probe({}));
    expect(missing.found).toBeNull();
    expect(missing.tried.at(-1)).toBe("/usr/local/bin/hide");
  });

  it("never asks a login shell when there is none to ask (unpackaged)", async () => {
    expect(await resolveCli({ ...input, worktreeRoot: null, searchPath: "" }, probe({}))).toEqual({
      found: null,
      tried: ["/h/.local/bin/hide", "/opt/homebrew/bin/hide", "/usr/local/bin/hide"],
    });
  });
});

describe("the remembered CLI", () => {
  it("reads back what the host wrote and refuses anything else", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-cli-"));
    const file = rememberedCliPath(dir);
    expect(parseRememberedCli(readJsonFile(file))).toBeNull();
    writeJsonFile(file, rememberedCliValue("/h/.local/bin/hide"));
    expect(parseRememberedCli(readJsonFile(file))).toBe("/h/.local/bin/hide");
    fs.writeFileSync(file, "{");
    expect(parseRememberedCli(readJsonFile(file))).toEqual({ unreadable: "not JSON" });
    for (const other of [{ schema: 2, path: "/a/hide" }, { schema: 1, path: "relative/hide" }, { schema: 1 }, "text"]) {
      expect(parseRememberedCli(other), JSON.stringify(other)).toEqual({ unreadable: "unexpected shape" });
    }
    fs.rmSync(dir, { recursive: true, force: true });
  });
});

describe("the hide CLI's answers", () => {
  const url = "http://127.0.0.1:7001/#token=abc";

  it("reads an attach from the last JSON line", () => {
    expect(parseConnect(result(`noise\n{"ok":true,"url":"${url}","port":7001,"pid":42}\n`))).toEqual({
      kind: "attached",
      url,
      origin: "http://127.0.0.1:7001",
      port: 7001,
      pid: 42,
    });
  });

  it("keeps the CLI's failure category and turns everything unreadable into start_failed", () => {
    expect(parseConnect(result('{"ok":false,"reason":"no_response","detail":"not healthy"}', { code: 2 }))).toEqual({
      kind: "failed",
      reason: "no_response",
      detail: "not healthy",
    });
    expect(parseConnect(result("", { code: 2, stderr: "unknown command: connect" }))).toMatchObject({ kind: "failed", reason: "start_failed" });
    expect(parseConnect(result("", { spawnError: "spawn EACCES" }))).toMatchObject({ reason: "start_failed", detail: "spawn EACCES" });
    expect(parseConnect(result("", { timedOut: true, code: null }))).toMatchObject({ reason: "no_response" });
  });

  it("refuses a URL that is not the daemon's loopback origin with a token", () => {
    for (const bad of ["http://example.com:7001/#token=abc", "http://127.0.0.1:7002/#token=abc", "http://127.0.0.1:7001/"]) {
      expect(parseConnect(result(JSON.stringify({ ok: true, url: bad, port: 7001, pid: 1 }))), bad).toMatchObject({ kind: "failed" });
    }
  });

  it("reads the attach-only probe, and null when it did not answer", () => {
    expect(parseStatus(result('{"running":false}'))).toEqual({ running: false });
    expect(parseStatus(result(`{"running":true,"url":"${url}","port":7001,"pid":42}`))).toMatchObject({ running: true, pid: 42 });
    expect(parseStatus(result("", { timedOut: true }))).toBeNull();
    expect(parseStatus(result("garbage"))).toBeNull();
  });

  it("finds the login shell's PATH past rc-file output", () => {
    expect(parseLoginPath("welcome!\n__HIDE_LOGIN_PATH__/a:/b__HIDE_LOGIN_PATH__\n")).toBe("/a:/b");
    expect(parseLoginPath("no marks")).toBeNull();
  });

  it("asks a real login shell for its PATH", async () => {
    const command = loginPathCommand("/bin/sh");
    const answer = await new ChildRunner().run(command.file, command.args, 10_000);
    expect(parseLoginPath(answer.stdout)).toBeTruthy();
  });
});
