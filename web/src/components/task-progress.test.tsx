// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { clientI18n } from "../i18n/translator";
import { TaskMark, TaskName } from "./task-progress";
import { TooltipProvider } from "./ui/tooltip";

vi.hoisted(() => { HTMLCanvasElement.prototype.getContext = () => null; });

// A checklist task's mark is the row's mark (status-marks D-23, B34): no ?, ⛔
// or ◐; a question or a block is the operator's `!`, work in progress the
// row's ring, work left and a PR-bound task an empty slot.
afterEach(async () => {
  await clientI18n.changeLanguage("en");
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

it("draws each task state as a row mark or an empty slot and names it", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  await clientI18n.changeLanguage("ko");
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const states = ["done", "working", "question", "blocked", "unfinished", "review", "ci_failed"] as const;
  await act(async () => root.render(<TooltipProvider>
    {states.map((state) => <div key={state} data-case={state}><TaskMark state={state} /><TaskName state={state} name="파서 나누기" /></div>)}
    <div data-case="waiting"><TaskMark state="working" waiting /></div>
  </TooltipProvider>));
  try {
    const at = (name: string) => container.querySelector(`[data-case="${name}"] [data-task-state]`)!;
    const mark = (name: string) => at(name).querySelector("[data-mark]")?.getAttribute("data-mark") ?? null;
    expect(at("done").querySelector("svg")).not.toBeNull();
    expect(mark("done")).toBeNull();
    expect(mark("working")).toBe("going");
    expect(mark("waiting")).toBe("waiting");
    expect([mark("question"), at("question").getAttribute("aria-label")]).toEqual(["turn", "내 차례 · 질문"]);
    expect([mark("blocked"), at("blocked").getAttribute("aria-label")]).toEqual(["turn", "내 차례 · 막힘"]);
    for (const empty of ["unfinished", "review", "ci_failed"]) expect(at(empty).childElementCount).toBe(0);
    expect(at("review").getAttribute("aria-label")).toBe("리뷰 대기");
    expect(container.querySelector('[data-case="unfinished"] [data-task-left]')?.textContent).toBe("일 남음");
    expect(container.querySelectorAll("[data-task-left]")).toHaveLength(1);
    expect(container.textContent).not.toMatch(/[?⛔◐]/);
  } finally {
    await act(async () => root.unmount());
  }
});
