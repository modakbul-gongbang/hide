// The real area renderer and View tabs, with a bounded read-only content
// fixture for the focus design comparison. Native pages are verified in the app.
import { useEffect, useMemo } from "react";
import { createActions } from "../actions";
import { createAreaTree, type AreaAdapter } from "../AreaTree";
import { TooltipProvider } from "../components/ui/tooltip";
import type { ViewDisplaySnapshot, ViewLayoutSnapshot } from "../snapshot";
import { DisplayTab } from "../ViewAreas";
import { installKeyboardOwner, noteKeyboardOwner, useKeyboardOwner } from "../viewFocus";
import { VIEW_WORDS } from "../viewLayout";

const Tree = createAreaTree<ViewDisplaySnapshot>("view");
const CONTENT = "# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다.\n다른 영역의 원래 선택과 내용은 읽을 수 있습니다.\n\nfixture % echo 한글 확인\n한글 확인";

export function AreaFocusScene({ theme }: { theme: "light" | "dark" }) {
  const owner = useKeyboardOwner();
  const actions = useMemo(() => createActions(() => {}), []);
  const layout = useMemo<ViewLayoutSnapshot>(() => {
    const area = (id: string) => ({ id, active: `${id}-1`, displays: ["한글 노트.md", "검증 결과.md"].map((label, i) => ({ id: `${id}-${i + 1}`, label, kind: "file", state: "open", tab_id: null, preview: false, path: `/fixture/${label}` }) as ViewDisplaySnapshot) });
    return { root: { split: { id: "s1", axis: "row", ratio: 0.5, first: { area: area("a1") }, second: { area: area("a2") } } }, active_area: "a1", display_count: 4, limits: { areas: 6, depth: 5, displays: 64 } };
  }, []);
  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
    document.documentElement.classList.toggle("light", theme === "light");
    const remove = installKeyboardOwner();
    noteKeyboardOwner({ kind: "view", workspace: "fixture", areaId: "a1" });
    return remove;
  }, [theme]);
  const adapter: AreaAdapter<ViewDisplaySnapshot> = {
    words: VIEW_WORDS,
    keyboardArea: owner.kind === "view" && owner.workspace === "fixture" ? owner.areaId : null,
    label: (item) => item.label,
    sameContent: (a, b) => a.path === b.path,
    tab: (item, interaction) => <DisplayTab display={item} interaction={interaction} actions={actions} />,
    body: () => <textarea aria-label="Readable Korean content" readOnly value={CONTENT} className="min-h-0 min-w-0 flex-1 resize-none bg-background p-sm font-mono text-caption text-foreground outline-none" />,
    empty: () => null, floating: (item) => item.label,
    menu: () => [], runMenu: () => {}, focus: () => {}, focusArea: () => {}, move: () => {}, split: () => {}, resize: () => {}, newTab: () => {},
    newTabLabel: "New tab", tabListLabel: "View tabs", actionsLabel: "View actions",
  };
  return <TooltipProvider><div data-gallery-scene="area-focus" data-workspace-screen="fixture" className="flex h-full bg-background text-foreground"><Tree layout={layout} adapter={adapter} /></div></TooltipProvider>;
}
