import { describe, expect, it } from "vitest";
import { windowStrip } from "./connection";
import { stepMarks, type MoveView } from "./coreMove";
import { createInterfaceI18n } from "./i18n/instance";
import { moveForm } from "./settings/CoreMoveDialog";

const view = (over: Partial<MoveView>): MoveView => ({ state: "idle", direction: "forward", device: "mini", intent: null, sent: 0, total: 0, failed: [], step: null, cause: null, node: null, ...over });

describe("the move's steps as the dialog marks them (B3, B4)", () => {
  it("marks the steps before the current one done, the current running and the rest to do", () => {
    expect(stepMarks(view({ state: "copying", step: "copy" })).map((row) => row.mark)).toEqual(["done", "done", "run", "todo", "todo"]);
  });

  it("marks the step that failed when the move is undone", () => {
    expect(stepMarks(view({ state: "rolled_back", step: "start_target" })).map((row) => row.mark)).toEqual(["done", "done", "done", "fail", "todo"]);
  });
});

describe("which form the dialog shows", () => {
  const request = { direction: "forward" as const, device: "mini" };

  it("waits on its checks until a frame newer than the one it asked beside answers, and for a frame of another move", () => {
    const before = view({ state: "rolled_back", step: "copy" });
    expect(moveForm(before, before, request)).toBe("checking");
    expect(moveForm(view({ state: "ready", device: "box" }), before, request)).toBe("checking");
    expect(moveForm(view({ state: "ready", direction: "back" }), before, request)).toBe("checking");
  });

  it("follows the supervisor's state once it answers", () => {
    const before = view({});
    expect(moveForm(view({ state: "checks_failed" }), before, request)).toBe("checks_failed");
    expect(moveForm(view({ state: "ready" }), before, request)).toBe("confirm");
    expect(moveForm(view({ state: "waiting" }), before, request)).toBe("moving");
    expect(moveForm(view({ state: "rolled_back" }), before, request)).toBe("failed");
    expect(moveForm(view({ state: "done" }), before, request)).toBe("done");
  });
});

describe("the strip above the window (W1 to W3)", () => {
  const machines = { from: "This Mac", to: "Mac mini" };

  it("names the move's step while a move holds the window", async () => {
    const { t } = await createInterfaceI18n("ko");
    const strip = windowStrip({ connection: "moving", refused: false, move: view({ state: "copying", step: "copy" }), link: null, machines: { from: "이 Mac", to: "Mac mini" }, coreMachine: "이 Mac" }, t);
    expect(strip).toEqual({ kind: "moving", mark: "pending", text: "core를 Mac mini로 옮기는 중 · 3/5 프로젝트와 기록 복사 · 다시 연결될 때까지 입력을 받지 않아요" });
  });

  it("names the node's update of its core and its failed link while the screen waits, and nothing once live", async () => {
    const { t } = await createInterfaceI18n("ko");
    const base = { refused: false, move: null, machines, coreMachine: "Mac mini" };
    expect(windowStrip({ ...base, connection: "connecting", link: { phase: "updating", machine: null, alias: "mini" } }, t)?.text).toBe("Mac mini의 core를 이 앱의 빌드로 바꾸는 중 · 에이전트는 계속 돌아요");
    expect(windowStrip({ ...base, connection: "reconnecting", link: { phase: "waiting", machine: "Mac mini", alias: "mini" } }, t)).toEqual({ kind: "unreachable", mark: "warn", text: "Mac mini의 core에 연결할 수 없어요 · 다시 연결하는 중" });
    expect(windowStrip({ ...base, connection: "live", link: { phase: "waiting", machine: "Mac mini", alias: "mini" } }, t)).toBeNull();
    // The operator ended the link (B16): the strip offers it back.
    expect(windowStrip({ ...base, connection: "connecting", link: { phase: "disconnected", machine: null, alias: "mini" } }, t)).toEqual({ kind: "disconnected", mark: "warn", text: "Mac mini core와 연결을 끊었어요", action: "reconnect" });
    expect(windowStrip({ ...base, connection: "reconnecting", link: { phase: "connecting", machine: null, alias: "mini" } }, t)?.kind).toBe("reconnecting");
    // A window opened while its link is down has drawn no core: it names the machine by the alias it is dialed by.
    const fresh = { ...base, coreMachine: "" };
    expect(windowStrip({ ...fresh, connection: "reconnecting", link: { phase: "waiting", machine: null, alias: "mini" } }, t)?.text).toBe("mini의 core에 연결할 수 없어요 · 다시 연결하는 중");
    expect(windowStrip({ ...fresh, connection: "connecting", link: { phase: "disconnected", machine: null, alias: "mini" } }, t)?.text).toBe("mini core와 연결을 끊었어요");
  });
});
