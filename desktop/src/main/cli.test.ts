import { describe, expect, it } from "vitest";
import { parseConnect, parseLoginPath, parseStatus, resolveCli, type FileProbe } from "./cli";
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
  const input = { override: null, worktreeRoot: "/w", searchPath: "/usr/local/bin:/opt/bin" };

  it("takes the override first and never searches past an unusable one", () => {
    expect(resolveCli({ ...input, override: "/x/hide" }, probe({ "/x/hide": 1, "/w/target/debug/hide": 1 })).found).toEqual({ path: "/x/hide", source: "env" });
    const missing = resolveCli({ ...input, override: "/x/hide" }, probe({ "/w/target/debug/hide": 1 }));
    expect(missing).toEqual({ found: null, tried: ["/x/hide"] });
  });

  it("takes the newest worktree build before PATH", () => {
    const files = { "/w/target/debug/hide": 10, "/w/target/release/hide": 20, "/opt/bin/hide": 30 };
    expect(resolveCli(input, probe(files)).found).toEqual({ path: "/w/target/release/hide", source: "worktree" });
  });

  it("searches PATH in order when no build exists, and reports every path it tried", () => {
    const resolved = resolveCli(input, probe({ "/opt/bin/hide": 1 }));
    expect(resolved.found).toEqual({ path: "/opt/bin/hide", source: "path" });
    expect(resolved.tried).toEqual(["/w/target/debug/hide", "/w/target/release/hide", "/usr/local/bin/hide", "/opt/bin/hide"]);
  });

  it("skips the worktree when packaged and finds nothing on an empty PATH", () => {
    expect(resolveCli({ override: null, worktreeRoot: null, searchPath: "" }, probe({}))).toEqual({ found: null, tried: [] });
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
});
