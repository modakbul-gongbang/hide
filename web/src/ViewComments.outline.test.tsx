// @vitest-environment jsdom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { setComments } from "./editor/comments";
import { fileModel, useLineComments } from "./editor/useLineComments";
import { TooltipProvider } from "./components/ui/tooltip";
import { initializeInterfaceI18n } from "./i18n/instance";
import type { ViewDisplaySnapshot } from "./snapshot";
import { useShellStore } from "./store";
import { PageNoteLayer, RegionLayer, type RegionSource } from "./ViewComments";
import { closeNote, openNote, setRegionMode, useFeedbackStore } from "./viewFeedback";
import type { ViewWorkspace } from "./viewLayout";
import type { EditorView } from "@codemirror/view";

vi.hoisted(() => { HTMLCanvasElement.prototype.getContext = () => null; });
const savedShell = useShellStore.getState();
const savedFeedback = useFeedbackStore.getState();
const workspace = {} as ViewWorkspace;
const actions = createActions(() => true);

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  initializeInterfaceI18n("en");
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => 400 });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 200 });
});
afterEach(() => {
  delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
  delete (HTMLElement.prototype as { clientHeight?: number }).clientHeight;
  useShellStore.setState(savedShell, true);
  useFeedbackStore.setState(savedFeedback, true);
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

async function mount(node: ReactNode) {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(<TooltipProvider>{node}</TooltipProvider>));
  return { container, unmount: () => act(async () => root.unmount()) };
}

const source: RegionSource = { pages: [{ left: 0, top: 0, width: 400, height: 200, page: null }], crop: async () => null, revision: null };
const outlines = (container: HTMLElement) => [...container.querySelectorAll<HTMLElement>("[data-view-area-outline]")];

it("keeps a picked image area outlined while its note is open and drops it with the note", async () => {
  const { container, unmount } = await mount(<RegionLayer displayId="d1" path="/repo/a.png" source={source} workspace={workspace} actions={actions} />);
  await act(async () => setRegionMode("d1", true));
  await act(async () => openNote({ displayId: "d1", anchor: { kind: "region", rect: { x: 0.25, y: 0.5, width: 0.5, height: 0.25 } }, commentId: null, note: "" }));
  const [outline] = outlines(container);
  expect(outlines(container)).toHaveLength(1);
  expect(parseFloat(outline!.style.left)).toBe(100);
  expect(parseFloat(outline!.style.top)).toBe(100);
  expect(parseFloat(outline!.style.width)).toBe(200);
  expect(parseFloat(outline!.style.height)).toBe(50);
  expect(container.querySelector("[data-view-note-field]")).not.toBeNull();
  await act(async () => closeNote("d1"));
  expect(outlines(container)).toHaveLength(0);
  await unmount();
});

it("keeps a browser page's picked place outlined over its still while the note is open", async () => {
  const key = "mac:d1";
  const { container, unmount } = await mount(<PageNoteLayer commentKey={key} displayId="d1" workspace={workspace} actions={actions} add={() => {}} done={() => {}} />);
  await act(async () => useFeedbackStore.setState({ browserNotes: { [key]: { kind: "pick", pick: 1, anchor: { kind: "element", selector: "#a" } as never, at: { left: 0.25, top: 0.5, width: 0.5, height: 0.25 } } } }));
  const [outline] = outlines(container);
  expect(outlines(container)).toHaveLength(1);
  expect(parseFloat(outline!.style.left)).toBe(100);
  expect(parseFloat(outline!.style.top)).toBe(100);
  expect(parseFloat(outline!.style.width)).toBe(200);
  expect(parseFloat(outline!.style.height)).toBe(50);
  await act(async () => useFeedbackStore.setState({ browserNotes: {} }));
  expect(outlines(container)).toHaveLength(0);
  await unmount();
});

function Lines({ marks }: { marks: (rows: number[]) => void }) {
  const view = { dispatch: (tr: { effects: { is: (e: unknown) => boolean; value: { marked: number[] } } }) => { if (tr.effects.is(setComments)) marks(tr.effects.value.marked); } } as unknown as EditorView;
  const display = { id: "d1", kind: "file", path: "/repo/a.ts" } as unknown as ViewDisplaySnapshot;
  useLineComments({ view, display, workspace, actions, model: fileModel("a\nb\nc\nd\n"), rows: 4, saved: "a\nb\nc\nd\n" });
  return null;
}

it("keeps the rows dragged in the gutter marked while their note is open", async () => {
  let marked: number[] = [];
  const { unmount } = await mount(<Lines marks={(rows) => { marked = rows; }} />);
  expect(marked).toEqual([]);
  await act(async () => openNote({ displayId: "d1", anchor: { kind: "lines", start: 2, end: 3, lines: ["b", "c"] }, commentId: null, note: "" }));
  expect(marked.sort()).toEqual([2, 3]);
  await act(async () => closeNote("d1"));
  expect(marked).toEqual([]);
  await unmount();
});
