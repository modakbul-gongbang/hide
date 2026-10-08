import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { execFileSync } from "node:child_process";
import type { HerdrFixture } from "./herdr-fixture";
import { copyFixtureShim } from "./shims/build";
import { fixtureExecutable } from "./platform-fixture";

export const PI_ID = "fixture-pi-native";
export const PI_TITLE = "요청 보기 - Pi native title";
export const OMP_ID = "fixture-omp-native";
export const OMP_TITLE = "요청 보기 - omp native title";
export const OMP_UPDATED_TITLE = "요청 보기 - 현재 이름 日本語";
export type NativeFileReader = "pi" | "omp";

// Native omp's v1 title slot is exactly 256 bytes including its newline.
// This oracle comes from the pinned CLI format, independent of the reader.
function ompTitleSlot(title: string): Buffer {
  const record = { type: "title", v: 1, title, source: "user", updatedAt: "2026-10-03T01:00:00.500Z", pad: "" };
  const empty = JSON.stringify(record);
  const padding = 255 - Buffer.byteLength(empty);
  if (padding < 0) throw new Error("fixture title exceeds the native slot");
  return Buffer.from(`${JSON.stringify({ ...record, pad: " ".repeat(padding) })}\n`);
}

/** A native writer's in-place slot update, with all history bytes retained. */
export function updateOmpTitle(session: string, title: string): void {
  const file = fs.openSync(session, "r+");
  try {
    const slot = ompTitleSlot(title);
    if (fs.writeSync(file, slot, 0, slot.length, 0) !== slot.length) throw new Error("fixture title write was incomplete");
  } finally {
    fs.closeSync(file);
  }
}

/** Native recorded messages, not snapshot injection or question prose. */
export function appendOmpQuestion(session: string, answered = false): void {
  const timestamp = "2026-10-03T01:00:01Z";
  const message = answered
    ? { role: "toolResult", toolCallId: "omp-question", content: [{ type: "text", text: "미리보기" }], isError: true }
    : { role: "assistant", content: [{ type: "toolCall", id: "omp-question", name: "ask", arguments: { questions: [{ id: "target", question: "배포 대상을 골라주세요", options: [{ label: "미리보기" }, { label: "운영" }] }] } }], stopReason: "toolUse" };
  fs.appendFileSync(session, `${JSON.stringify({ type: "message", id: answered ? "answer" : "question", parentId: null, timestamp, message })}\n`);
}

/** A seed consumed and written by the fake CLI. No session file exists yet. */
export function preparePiWriter(herdr: HerdrFixture, checkout = path.join(herdr.root, "fixture")): string {
  return prepareNativeWriter(herdr, "pi", checkout);
}

export function prepareNativeWriter(herdr: HerdrFixture, kind: NativeFileReader, checkout = path.join(herdr.root, "fixture")): string {
  copyFixtureShim("claude-shim", path.join(herdr.root, "bin", fixtureExecutable(kind)));
  const cwd = fs.realpathSync(checkout);
  const relativeTemp = path.relative(fs.realpathSync(os.tmpdir()), cwd);
  if (kind === "omp" && (relativeTemp.startsWith("..") || path.isAbsolute(relativeTemp))) throw new Error("omp fixture requires an owned temporary checkout");
  const encoded = kind === "pi" ? `--${cwd.replace(/^[/\\]/, "").replace(/[/\\:]/g, "-")}--` : `-tmp-${relativeTemp.replace(/[/\\:]/g, "-")}`;
  const session = path.join(herdr.env.HOME!, `.${kind}`, "agent", "sessions", encoded, "timestamp_fixture.jsonl");
  fs.mkdirSync(path.dirname(session), { recursive: true });
  fs.writeFileSync(path.join(herdr.root, `${kind}-session-path.config`), session);
  const timestamp = "2026-10-03T01:00:00Z";
  const records = [
    { type: "session", version: 3, id: kind === "pi" ? PI_ID : OMP_ID, timestamp, cwd },
    { type: "message", id: "a0000001", parentId: null, timestamp, message: { role: "user", content: "요청 보기를 만들어줘" } },
    { type: "message", id: "a0000002", parentId: "a0000001", timestamp, message: { role: "assistant", content: [{ type: "text", text: "HIDE_E2E_LABEL {\"goal\":\"Generated Pi goal\",\"goal_changed\":true,\"line\":\"완료\",\"end\":\"done\"}" }], stopReason: "stop" } },
    ...(kind === "pi" ? [{ type: "session_info", id: "a0000003", parentId: "a0000002", timestamp, name: PI_TITLE }] : [{ type: "title_change", id: "a0000003", parentId: "a0000002", timestamp, title: "Old audit title", source: "user" }]),
  ];
  fs.writeFileSync(path.join(herdr.root, `${kind}-session-seed.jsonl`), Buffer.concat([
    kind === "omp" ? ompTitleSlot(OMP_TITLE) : Buffer.alloc(0),
    Buffer.from(records.map(record => `${JSON.stringify(record)}\n`).join("")),
  ]));
  return session;
}

/** Explicit synthetic integration provenance, never a native discovery claim. */
export function reportPiWriter(herdr: HerdrFixture, pane: string, session: string): void {
  reportNativeWriter(herdr, "pi", pane, session);
}

export function reportNativeWriter(herdr: HerdrFixture, kind: NativeFileReader, pane: string, session: string): void {
  execFileSync(herdr.bin, ["pane", "report-agent-session", pane, "--source", `herdr:${kind}`, "--agent", kind, "--agent-session-path", session, "--seq", "2", "--session-start-source", "clear"], { env: herdr.env, timeout: 30_000 });
}
