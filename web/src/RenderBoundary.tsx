// A part of the window that fails to draw takes no other part with it. A
// render error React does not catch unmounts the whole page, the core's
// socket with it, and leaves the window blank until a reload that draws
// the same snapshot and fails again. Each boundary keeps its siblings and
// the connection, records the failure as a diagnostic once per error until
// the part draws again (design 13: no banner), and draws the part again on
// the next change to the core's snapshot or the page's own state, since a
// render reads nothing else. It listens to the stores only while its part
// is down, and a change to the diagnostic list alone is not a change: a
// part's own diagnostic would otherwise draw it, and two failing parts
// would draw each other, again and again.

import { Component, type ReactNode } from "react";
import { AreaEmpty } from "./AreaEmpty";
import { useInterfaceTranslation } from "./i18n/client";
import { useSidebarColumns } from "./sidebar";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

/** A failure's message kept in the diagnostic, which may quote a whole value (engineering 15). */
const MESSAGE_CAP = 300;

/** The shell store's fields no part draws from; a change to them alone draws nothing again. */
const DIAGNOSTIC_FIELDS: ReadonlySet<string> = new Set(["diagnostics", "diagnosticsDropped"]);

function changedBeyondDiagnostics(state: object, previous: object): boolean {
  const before = previous as Record<string, unknown>;
  return Object.entries(state).some(([key, value]) => !DIAGNOSTIC_FIELDS.has(key) && value !== before[key]);
}

type Props = {
  /** Names the part in the diagnostic. */
  region: string;
  /** What stands in the part's place while it cannot draw. */
  fallback: ReactNode;
  children: ReactNode;
};

export class RenderBoundary extends Component<Props, { failed: boolean }> {
  override state = { failed: false };
  /** The message last recorded, so a part failing the same way on every change logs once. */
  private recorded: string | null = null;
  private stopListening: (() => void) | null = null;

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  override componentDidCatch(error: unknown): void {
    const message = (error instanceof Error ? error.message : String(error)).slice(0, MESSAGE_CAP);
    if (message === this.recorded) return;
    this.recorded = message;
    useShellStore.getState().noteDiagnostic(`render_failed region=${this.props.region}: ${message}`);
  }

  // The listening follows `failed`, so a remount (StrictMode's included) picks it up again.
  override componentDidMount(): void {
    this.listenWhileFailed();
  }

  override componentDidUpdate(_props: Props, previous: { failed: boolean }): void {
    if (previous.failed && !this.state.failed) this.recorded = null;
    this.listenWhileFailed();
  }

  override componentWillUnmount(): void {
    this.stopListening?.();
    this.stopListening = null;
  }

  private listenWhileFailed(): void {
    if (!this.state.failed || this.stopListening) return;
    const retry = () => {
      this.stopListening?.();
      this.stopListening = null;
      this.setState({ failed: false });
    };
    const shell = useShellStore.subscribe((state, previous) => {
      if (changedBeyondDiagnostics(state, previous)) retry();
    });
    const page = useUiStore.subscribe(retry);
    this.stopListening = () => {
      shell();
      page();
    };
  }

  override render(): ReactNode {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

/** The line a column or the center shows in place of what it could not draw. */
export function RegionFailed() {
  const { t } = useInterfaceTranslation();
  return <AreaEmpty state="render_failed" text={t("shell.renderFailed")} />;
}

/** The sidebar's place at its own width, so the center keeps its room. */
export function SidebarFailed() {
  const columns = useSidebarColumns();
  const visible = useShellStore((s) => s.rest?.ui_state?.left_sidebar_visible ?? true);
  if (!visible) return null;
  return (
    <div className="flex h-full w-(--sidebar-total) shrink-0 border-r border-border bg-sidebar" style={columns} data-render-failed="sidebar">
      <RegionFailed />
    </div>
  );
}

/** The top row's place: an empty strip of its height, so the window can still be dragged. */
export function TitleRowFailed() {
  return <header className="window-drag h-(--size-tab-strip) shrink-0 border-b border-border bg-sidebar" data-render-failed="title" />;
}

/** The whole window's last resort: the drag strip over the line. */
export function ShellFailed() {
  return (
    <div className="flex h-full flex-col bg-background text-foreground" data-render-failed="shell">
      <TitleRowFailed />
      <RegionFailed />
    </div>
  );
}
