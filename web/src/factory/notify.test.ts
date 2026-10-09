import { describe, expect, it } from "vitest";
import type { FactorySummary, FactoryView, InboxItem } from "./model";
import { newFactoryNotices } from "./notify";

function view(id: string, patch: Partial<FactoryView> = {}): FactoryView {
  return { id, project: `/${id}`, project_name: id, closed: false, main_broken: false, macos_notifications: true, ...patch } as FactoryView;
}

function item(factory: string, task: string | null, group: InboxItem["group"], question: string | null = null, resolve: string | null = null): InboxItem {
  return { factory, task, group, question, resolve, kind: group === "todo" ? "github" : "blocking", display_id: task, title: task ?? "" } as InboxItem;
}

function summary(factories: FactoryView[], inbox: InboxItem[]): FactorySummary {
  return { my_turn: 0, factories, inbox };
}

describe("the Factory's macOS notifications (B24)", () => {
  it("take the first summary as the baseline, then name only what newly needs a decision, a Factory's to-do included", () => {
    const first = newFactoryNotices(summary([view("a")], [item("a", "T-1", "answer", "q-1")]), null);
    expect(first.notices).toEqual([]);
    const next = newFactoryNotices(summary([view("a")], [item("a", "T-1", "answer", "q-1"), item("a", "T-2", "merge"), item("a", null, "todo", null, "github")]), first.seen);
    expect(next.notices.map((notice) => notice.id)).toEqual(["item:a/T-2/merge", "item:a/-/github"]);
  });

  it("stay silent for a Factory with the setting off, and say a main that newly broke once", () => {
    const quiet = view("b", { macos_notifications: false });
    const base = newFactoryNotices(summary([view("a"), quiet], []), null).seen;
    const broke = newFactoryNotices(summary([view("a", { main_broken: true }), { ...quiet, main_broken: true }], [item("b", "T-1", "stopped")]), base);
    expect(broke.notices.map((notice) => notice.id)).toEqual(["main:a"]);
    expect(newFactoryNotices(summary([view("a", { main_broken: true })], []), broke.seen).notices).toEqual([]);
  });
});
