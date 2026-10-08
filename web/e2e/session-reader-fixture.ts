import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import type { HerdrFixture } from "./herdr-fixture";
import { copyFixtureShim } from "./shims/build";
import { fixtureExecutable } from "./platform-fixture";

export const PI_ID = "fixture-pi-native";
export const PI_TITLE = "요청 보기 - Pi native title";

/** A seed consumed and written by the fake CLI. No session file exists yet. */
export function preparePiWriter(herdr: HerdrFixture, checkout = path.join(herdr.root, "fixture")): string {
  copyFixtureShim("claude-shim", path.join(herdr.root, "bin", fixtureExecutable("pi")));
  const cwd = fs.realpathSync(checkout);
  const encoded = `--${cwd.replace(/^[/\\]/, "").replace(/[/\\:]/g, "-")}--`;
  const session = path.join(herdr.env.HOME!, ".pi", "agent", "sessions", encoded, "timestamp_fixture.jsonl");
  fs.mkdirSync(path.dirname(session), { recursive: true });
  fs.writeFileSync(path.join(herdr.root, "pi-session-path.config"), session);
  const timestamp = "2026-10-03T01:00:00Z";
  const records = [
    { type: "session", version: 3, id: PI_ID, timestamp, cwd },
    { type: "message", id: "a0000001", parentId: null, timestamp, message: { role: "user", content: "요청 보기를 만들어줘" } },
    { type: "message", id: "a0000002", parentId: "a0000001", timestamp, message: { role: "assistant", content: [{ type: "text", text: "HIDE_E2E_LABEL {\"goal\":\"Generated Pi goal\",\"goal_changed\":true,\"line\":\"완료\",\"end\":\"done\"}" }], stopReason: "stop" } },
    { type: "session_info", id: "a0000003", parentId: "a0000002", timestamp, name: PI_TITLE },
  ];
  fs.writeFileSync(path.join(herdr.root, "pi-session-seed.jsonl"), records.map(record => `${JSON.stringify(record)}\n`).join(""));
  return session;
}

/** Explicit synthetic integration provenance, never a native discovery claim. */
export function reportPiWriter(herdr: HerdrFixture, pane: string, session: string): void {
  execFileSync(herdr.bin, ["pane", "report-agent-session", pane, "--source", "herdr:pi", "--agent", "pi", "--agent-session-path", session, "--seq", "2", "--session-start-source", "clear"], { env: herdr.env, timeout: 30_000 });
}
