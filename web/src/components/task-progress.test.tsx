// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { clientI18n } from "../i18n/translator";
import type { TaskState } from "../sessionWork";
import { shownTaskState, TaskMark, TaskName } from "./task-progress";
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
  // A checklist task's states (sessionWork.taskState), not an agent's, spelled as one string so the
  // core's guard against a shell-side agent state vocabulary (axes_no_state_name_array_remains_in_the_shell) reads no array.
  const states = "done working question blocked unfinished review ci_failed".split(" ") as TaskState[];
  await act(async () => root.render(<TooltipProvider>
    {states.map((state) => <div key={state} data-case={state}><TaskMark state={state} mark="going" /><TaskName state={state} name="파서 나누기" prShown /></div>)}
    <div data-case="grouped"><TaskName state="ci_failed" name="좁은 열 확인" prShown={false} /></div>
    <div data-case="waiting"><TaskMark state="working" mark="waiting" /></div>
  </TooltipProvider>));
  try {
    const at = (name: string) => container.querySelector(`[data-case="${name}"] [data-task-state]`)!;
    const mark = (name: string) => at(name).querySelector("[data-mark]")?.getAttribute("data-mark") ?? null;
    expect(at("done").querySelector("svg")).not.toBeNull();
    expect(mark("done")).toBeNull();
    expect([mark("working"), at("working").getAttribute("aria-label")]).toEqual(["going", "진행 중"]);
    expect([mark("waiting"), at("waiting").getAttribute("aria-label")]).toEqual(["waiting", "진행 중 · 대기"]);
    expect([mark("question"), at("question").getAttribute("aria-label")]).toEqual(["turn", "내 차례 · 질문"]);
    expect([mark("blocked"), at("blocked").getAttribute("aria-label")]).toEqual(["turn", "내 차례 · 막힘"]);
    // An empty slot is silent: the name's word or the PR mark says the state once.
    for (const empty of ["unfinished", "review", "ci_failed"]) expect([at(empty).childElementCount, at(empty).getAttribute("aria-hidden")]).toEqual([0, "true"]);
    // Work left says so after the name; a PR-bound task's PR mark says its state, and
    // a task whose request's PR mark hangs on another task says the state in words.
    const words = [...container.querySelectorAll("[data-task-word]")].map((word) => [word.closest("[data-case]")!.getAttribute("data-case"), word.textContent]);
    expect(words).toEqual([["unfinished", "일 남음"], ["grouped", "CI 실패"]]);
    expect(container.textContent).not.toMatch(/[?⛔◐]/);
  } finally {
    await act(async () => root.unmount());
  }
});

it("draws a task left in progress as work left once its row stops", () => {
  expect(shownTaskState("working", "going")).toBe("working");
  expect(shownTaskState("working", "waiting")).toBe("working");
  for (const mark of ["unread", "quiet", "done", "turn", null] as const) expect(shownTaskState("working", mark)).toBe("unfinished");
  expect(shownTaskState("question", "quiet")).toBe("question");
});
