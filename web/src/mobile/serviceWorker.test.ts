import { readFileSync } from "node:fs";
import { createContext, runInContext } from "node:vm";
import { expect, it } from "vitest";

// Only browser APIs are substituted. Run the shipped worker itself and
// inspect the notifications a phone receives, not a copy of its formatter.
function phoneWorker() {
  type Event = { data: unknown; waitUntil: (work: Promise<unknown>) => void };
  const listeners = new Map<string, (event: Event) => void>();
  const stored = new Map<string, Response>();
  const shown: { title: string; body: string; tag: string }[] = [];
  const closed: string[] = [];
  const self = {
    addEventListener: (kind: string, listener: (event: Event) => void) => listeners.set(kind, listener),
    registration: {
      showNotification: async (title: string, options: { body: string; tag: string }) => { shown.push({ title, body: options.body, tag: options.tag }); },
      getNotifications: async () => [{ tag: "old", close: () => closed.push("old") }],
    },
  };
  const caches = { open: async () => ({
    put: async (key: string, value: Response) => { stored.set(key, value); },
    match: async (key: string) => stored.get(key)?.clone(),
  }) };
  runInContext(readFileSync(new URL("../../public/m/sw.js", import.meta.url), "utf8"), createContext({ self, caches, Response, URL }));
  async function dispatch(kind: string, data: unknown) {
    const pending: Promise<unknown>[] = [];
    listeners.get(kind)!({ data, waitUntil: (work) => pending.push(work) });
    await Promise.all(pending);
  }
  return { shown, closed,
    words: (words: Record<string, string>) => dispatch("message", { type: "words", words }),
    push: (data: Record<string, unknown>) => dispatch("push", { json: () => data }),
  };
}

it("shows the row's status and exact sentence, replaces by the same tag and withdraws seen notices", async () => {
  const phone = phoneWorker();
  await phone.words({ needs_you: "Needs your attention", done: "Done", status_question: "Question", status_approval: "Approval", status_error: "Blocked", status_done: "Done", status_stopped: "Stopped" });
  for (const [status_code, detail] of [["question", "배포할 환경을 골라 주세요"], ["approval", "계획 승인 대기"], ["error", "로그인이 필요해요"], ["done", "배포 완료"], ["stopped", "테스트 세 개 남음"]]) {
    await phone.push({ title: "Add a dark mode switch", state: status_code === "done" || status_code === "stopped" ? "done" : "needs_you", status_code, detail, place: "my-app", tag: "node|w1:p1", clear: ["old"] });
  }
  expect(phone.shown).toEqual([
    { title: "Add a dark mode switch", body: "Question · 배포할 환경을 골라 주세요 · my-app", tag: "node|w1:p1" },
    { title: "Add a dark mode switch", body: "Approval · 계획 승인 대기 · my-app", tag: "node|w1:p1" },
    { title: "Add a dark mode switch", body: "Blocked · 로그인이 필요해요 · my-app", tag: "node|w1:p1" },
    { title: "Add a dark mode switch", body: "Done · 배포 완료 · my-app", tag: "node|w1:p1" },
    { title: "Add a dark mode switch", body: "Stopped · 테스트 세 개 남음 · my-app", tag: "node|w1:p1" },
  ]);
  expect(phone.closed).toEqual(["old", "old", "old", "old", "old"]);
});

it("keeps the original body with no detail, and works with older payloads and cached page words", async () => {
  const phone = phoneWorker();
  await phone.words({ needs_you: "내 확인 대기", done: "끝", status_done: "완료", status_question: "질문" });
  await phone.push({ title: "task", state: "needs_you", place: "project" });
  await phone.push({ title: "task", state: "done", status_code: "stopped", detail: null, place: "project" });
  await phone.push({ title: "task", state: "needs_you", status_code: "question", detail: "Choose staging?", place: "" });
  await phone.push({ title: "task", state: "done", status_code: "done", detail: "배포 완료", place: "project" });
  await phone.words({ needs_you: "내 확인 대기", done: "끝" });
  await phone.push({ title: "task", state: "needs_you", status_code: "question", detail: "Choose staging?", place: "project" });
  expect(phone.shown.map(({ body }) => body)).toEqual(["내 확인 대기 · project", "끝 · project", "질문 · Choose staging?", "완료 · 배포 완료 · project", "내 확인 대기 · Choose staging? · project"]);
  const newPhone = phoneWorker();
  await newPhone.push({ title: "task", state: "done", detail: "배포 완료", place: "project" });
  expect(newPhone.shown).toEqual([{ title: "task", body: "배포 완료 · project", tag: undefined }]);
});
