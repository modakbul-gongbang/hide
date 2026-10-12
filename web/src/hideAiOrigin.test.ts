import { describe, expect, it } from "vitest";
import { hideAiCheckoutOf, hideAiPlace, hideAiWords, originLine, splitByOrigin, taskLine } from "./hideAiOrigin";
import { rowAccessibleName } from "./agentRow";
import { legacyAgentRow } from "../test/legacyAgentRow";
import { createInterfaceI18n } from "./i18n/instance";
import type { Checkout, HideAiCheckout } from "./snapshot";

const MEMORY = { kind: "memory_promote", target: "m72" };
const device = { device_id: "mac" };
const checkout = (path: string) => ({ id: path, path }) as unknown as Checkout;

describe("what Hide AI started is named from the core's codes (PRD hide-ai-origin-mark D-02, B10)", () => {
  it("words a known task in the interface language and a task the build does not know as its code", async () => {
    const ko = (await createInterfaceI18n("ko")).t;
    const en = (await createInterfaceI18n("en")).t;
    expect(taskLine(ko, MEMORY)).toBe("Memory 올리기 · m72");
    expect(taskLine(en, MEMORY)).toBe("Promote Memory · m72");
    expect(taskLine(en, { kind: "label_repair" })).toBe("label_repair");
  });

  it("says Hide AI alone when the core names no task, and never invents one", async () => {
    const { t } = await createInterfaceI18n("en");
    expect(taskLine(t, undefined)).toBeNull();
    expect(originLine(t, {})).toBe("Hide AI");
    expect(hideAiPlace(t, "herdr-ide", {})).toBe("herdr-ide › Hide AI");
    expect(hideAiPlace(t, "herdr-ide", { task: MEMORY })).toBe("herdr-ide › Promote Memory · m72");
  });

  it("puts Hide AI and the task in the row's accessible name, and nothing in an operator's row (D-09, B9)", async () => {
    const { t } = await createInterfaceI18n("ko");
    expect(hideAiWords(t, { task: MEMORY })).toBe("Hide AI · Memory 올리기 · m72");
    expect(hideAiWords(t, {})).toBe("Hide AI");
    const base = { id: "p", pane_id: "p", identity_label: "m72 결정을 문서로 옮기기", agent_kind: "claude", status_code: "stopped", detail: null } as const;
    expect(rowAccessibleName(t, legacyAgentRow({ ...base, hide_ai: { task: MEMORY } }), null)).toContain("Hide AI · Memory 올리기 · m72");
    expect(rowAccessibleName(t, legacyAgentRow({ ...base }), null)).not.toContain("Hide AI");
  });
});

describe("a project's checkouts split by who made them (D-03, D-06, D-07, B13)", () => {
  const listed: HideAiCheckout[] = [{ device_id: "mac", path: "/repo.worktrees/hide-memory-m72", task: MEMORY }];

  it("gathers the listed checkouts behind the operator's and keeps their order", () => {
    const rows = [checkout("/repo"), checkout("/repo.worktrees/hide-memory-m72"), checkout("/repo.worktrees/feature")];
    const { operator, hideAi } = splitByOrigin(listed, device, rows);
    expect(operator.map((row) => row.path)).toEqual(["/repo", "/repo.worktrees/feature"]);
    expect(hideAi.map((row) => row.path)).toEqual(["/repo.worktrees/hide-memory-m72"]);
  });

  it("joins by device and path, never by a name that looks like Hide AI's", () => {
    const lookalike = checkout("/repo.worktrees/hide-memory-m1-abc");
    expect(hideAiCheckoutOf(listed, device, lookalike)).toBeNull();
    expect(hideAiCheckoutOf(listed, { device_id: "mini" }, checkout("/repo.worktrees/hide-memory-m72"))).toBeNull();
    expect(splitByOrigin(undefined, device, [lookalike]).hideAi).toEqual([]);
  });
});
