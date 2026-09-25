import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { AgentMark } from "./AgentMark";
import { Status } from "./components/settings-rows";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { Hint } from "./components/ui/tooltip";
import { overviewProject } from "./navigation";
import {
  PROVIDER_FILTERS,
  conversationTurns,
  detailState,
  listState,
  naming,
  sessionAccessibleName,
  sessionCheckout,
  sessionTime,
  sessionTitle,
  type DetailState,
  type ListState,
  type ProviderFilter,
} from "./sessions";
import type { ArchiveEvent, ProjectSessionDetail, SessionRow, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

// A Project's Sessions (PRD S8 B1-B9, D-08): the session history of every
// Workspace the Project has, newest first, narrowed by provider and search,
// with one session read beside it. It is entered from the Project's Overview
// and stays on that Project whatever the operator focuses elsewhere; nothing
// here runs an agent or sends a session to a Workspace.

export function SessionsScreen({ projectId, actions }: { projectId: string; actions: Actions }) {
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const setScreen = useUiStore((s) => s.setScreen);
  const found = useMemo(() => overviewProject(rest, agents, projectId), [rest, agents, projectId]);
  if (!found) {
    return (
      <section className="flex flex-1 flex-col items-center justify-center gap-sm p-xl text-caption text-muted-foreground" data-sessions-missing={projectId}>
        <p>This project is no longer in the catalog.</p>
        <Button variant="secondary" onClick={() => setScreen({ kind: "main" })}>
          Back to Main
        </Button>
      </section>
    );
  }
  // Keyed by the Project, so another Project starts with its own filters and asks for itself.
  return (
    <ProjectSessions
      key={`${found.workspace.device_id}:${found.workspace.id}`}
      workspace={found.workspace}
      deviceLabel={found.device?.label ?? null}
      actions={actions}
    />
  );
}

function ProjectSessions({ workspace, deviceLabel, actions }: { workspace: Workspace; deviceLabel: string | null; actions: Actions }) {
  const setScreen = useUiStore((s) => s.setScreen);
  const sessions = useShellStore((s) => s.projectSessions);
  const live = useShellStore((s) => s.connection === "live");
  const [provider, setProvider] = useState<ProviderFilter>("all");
  const [query, setQuery] = useState("");
  // Whether this screen has seen its own Project named since it asked (A7).
  const [acknowledged, setAcknowledged] = useState(false);
  const project = useMemo(() => ({ id: workspace.id, deviceId: workspace.device_id }), [workspace.id, workspace.device_id]);
  const named = naming(sessions, project, acknowledged);

  const namedNow = useRef(named);
  namedNow.current = named;

  // Name the Project on arrival, which reads its history afresh, and again
  // when the connection comes back: an event sent while the socket is down is
  // dropped, and a daemon that restarted names nothing. A window whose Project
  // another window has replaced since waits for the operator instead (A7).
  // `live` turns true only once the reconnect's snapshot is applied, so the
  // naming read here is the daemon's current one.
  useEffect(() => {
    if (live && namedNow.current !== "replaced") actions.refreshProjectSessions(project.id, project.deviceId);
  }, [project, live, actions]);
  useEffect(() => {
    if (named === "ours" && !acknowledged) setAcknowledged(true);
  }, [named, acknowledged]);

  const retry = () => actions.refreshProjectSessions(project.id, project.deviceId);
  const list = listState(sessions, named, provider, query);
  const detail = detailState(sessions, named);
  const total = named === "ours" ? (sessions?.rows.length ?? 0) : 0;

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col bg-background" aria-label={`Sessions of ${workspace.label}`} data-sessions-screen={workspace.id}>
      <header className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-xs border-b border-border bg-sidebar px-sm text-caption">
        <button type="button" className="rounded-xs px-xs text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent" data-go-main="true" onClick={() => setScreen({ kind: "main" })}>
          Main
        </button>
        <span aria-hidden="true" className="text-muted-foreground">/</span>
        <button
          type="button"
          title={workspace.path}
          className="min-w-0 truncate rounded-xs px-xs text-subtle-foreground hover:bg-accent hover:text-foreground focus-visible:bg-accent"
          data-go-overview={workspace.id}
          onClick={() => setScreen({ kind: "overview", projectId: workspace.id })}
        >
          {workspace.label}
        </button>
        <span aria-hidden="true" className="text-muted-foreground">/</span>
        <h1 className="shrink-0 text-subhead font-semibold text-foreground" aria-current="page">
          Sessions
        </h1>
        {deviceLabel ? <span className="shrink-0 rounded-xs bg-secondary px-xs text-micro text-subtle-foreground">{deviceLabel}</span> : null}
      </header>
      <div className="flex min-h-0 flex-1">
        <section
          aria-label="Session history"
          className="flex min-h-0 min-w-[var(--size-panel-min)] shrink basis-[var(--size-panel-ideal)] flex-col border-r border-border bg-sidebar"
          data-sessions-list={list.kind}
        >
          <HistoryControls
            provider={provider}
            query={query}
            total={total}
            shown={list.kind === "rows" ? list.rows.length : 0}
            filtering={provider !== "all" || query.trim() !== ""}
            reading={named === "ours" && sessions?.loading === true && total > 0}
            onProvider={setProvider}
            onQuery={setQuery}
          />
          <HistoryList
            state={list}
            workspace={workspace}
            selected={detail.kind === "none" ? null : detail.detail.session_id}
            onOpen={(row) => actions.openProjectSession(project.id, row.id)}
            onRetry={retry}
            onClearFilters={() => {
              setProvider("all");
              setQuery("");
            }}
            onShowHere={retry}
          />
        </section>
        <section aria-label="Session" className="flex min-h-0 min-w-[var(--size-workspace-area-min)] flex-1 flex-col" data-session-detail={detail.kind}>
          <SessionDetail state={detail} rows={named === "ours" ? (sessions?.rows ?? []) : []} choosable={list.kind === "rows"} workspace={workspace} onRetry={retry} />
        </section>
      </div>
    </section>
  );
}

function HistoryControls({
  provider,
  query,
  total,
  shown,
  filtering,
  reading,
  onProvider,
  onQuery,
}: {
  provider: ProviderFilter;
  query: string;
  total: number;
  shown: number;
  filtering: boolean;
  reading: boolean;
  onProvider: (provider: ProviderFilter) => void;
  onQuery: (query: string) => void;
}) {
  // ToggleGroup's own roving focus only moves the arrows; it never selects on
  // its own. A one-Tab-stop filter picks the neighbouring provider on the
  // same keystroke that moves to it (B9), so this still selects explicitly
  // and focuses the element that plays that choice now, ahead of Radix's own
  // roving-focus handler for the same keydown.
  const onProviderKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const index = PROVIDER_FILTERS.findIndex((choice) => choice.id === provider);
    const next = PROVIDER_FILTERS[(index + step + PROVIDER_FILTERS.length) % PROVIDER_FILTERS.length];
    if (!next) return;
    onProvider(next.id);
    event.currentTarget.querySelector<HTMLElement>(`[data-provider-choice="${next.id}"]`)?.focus();
  };
  const onSearchKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      // Into the list, at the row the list would give focus to on Tab.
      const row = event.currentTarget.closest("[data-sessions-list]")?.querySelector<HTMLElement>('[data-session-row][tabindex="0"]');
      if (row) {
        event.preventDefault();
        row.focus();
      }
    } else if (event.key === "Escape" && query !== "") {
      event.preventDefault();
      event.stopPropagation();
      onQuery("");
    }
  };
  return (
    <div className="flex shrink-0 flex-col gap-sm border-b border-border p-sm">
      <ToggleGroup
        type="single"
        aria-label="Provider"
        className="w-full"
        value={provider}
        onValueChange={(next) => {
          // Radix lets the active item toggle itself off (value becomes "");
          // a provider filter has no "none chosen" state, so that click is a
          // no-op and the current provider stays selected (matches a native
          // radiogroup, which cannot be deselected by re-choosing it).
          if (next) onProvider(next as ProviderFilter);
        }}
        data-sessions-provider={provider}
        onKeyDown={onProviderKey}
      >
        {PROVIDER_FILTERS.map((choice) => (
          <ToggleGroupItem key={choice.id} value={choice.id} data-provider-choice={choice.id} className="min-w-0 flex-1 truncate">
            {choice.label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <Input
        type="search"
        mono={false}
        value={query}
        placeholder="Search sessions"
        aria-label="Search sessions"
        className="w-full"
        data-sessions-search="true"
        onChange={(event) => onQuery(event.target.value)}
        onKeyDown={onSearchKey}
      />
      {total > 0 ? (
        <p className="flex items-center gap-xs text-micro text-muted-foreground" data-sessions-count={filtering ? `${shown}/${total}` : String(total)}>
          <span>{filtering ? `${shown} of ${total} sessions` : `${total} ${total === 1 ? "session" : "sessions"}`}</span>
          {reading ? <span role="status">Reading…</span> : null}
        </p>
      ) : null}
    </div>
  );
}

function HistoryList({
  state,
  workspace,
  selected,
  onOpen,
  onRetry,
  onClearFilters,
  onShowHere,
}: {
  state: ListState;
  workspace: Workspace;
  selected: string | null;
  onOpen: (row: SessionRow) => void;
  onRetry: () => void;
  onClearFilters: () => void;
  onShowHere: () => void;
}) {
  const listRef = useRef<HTMLUListElement>(null);
  if (state.kind !== "rows") {
    return (
      <div className="flex min-h-0 flex-1 flex-col gap-sm overflow-auto p-md" data-sessions-state={state.kind}>
        <ListNotice state={state} onRetry={onRetry} onClearFilters={onClearFilters} onShowHere={onShowHere} />
      </div>
    );
  }
  // One row takes Tab (the open one, else the first) and the arrows move
  // between rows, so a long history is one stop in the tab order (B9).
  const focusable = state.rows.some((row) => row.id === selected) ? selected : state.rows[0]?.id;
  const moveFocus = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp" && event.key !== "Home" && event.key !== "End") return;
    const rows = [...(listRef.current?.querySelectorAll<HTMLElement>("[data-session-row]") ?? [])];
    const index = rows.indexOf(event.currentTarget);
    const next = event.key === "Home" ? 0 : event.key === "End" ? rows.length - 1 : index + (event.key === "ArrowDown" ? 1 : -1);
    if (next < 0 || next >= rows.length) return;
    event.preventDefault();
    rows[next]?.focus();
  };
  return (
    <ul ref={listRef} className="flex min-h-0 flex-1 flex-col gap-xxs overflow-auto p-xs" role="list" data-sessions-rows={state.rows.length}>
      {state.rows.map((row) => (
        <SessionRowItem key={`${row.provider}:${row.id}`} row={row} workspace={workspace} selected={row.id === selected} focusable={row.id === focusable} onOpen={onOpen} onRetry={onRetry} onKeyDown={moveFocus} />
      ))}
    </ul>
  );
}

function ListNotice({ state, onRetry, onClearFilters, onShowHere }: { state: Exclude<ListState, { kind: "rows" }>; onRetry: () => void; onClearFilters: () => void; onShowHere: () => void }) {
  switch (state.kind) {
    case "loading":
      return (
        <p role="status">
          <Status tone="pending">Loading sessions…</Status>
        </p>
      );
    case "empty":
      return <Status tone="muted">No sessions yet</Status>;
    case "no_match":
      return (
        <>
          <Status tone="muted">No matching sessions</Status>
          <div>
            <Button variant="secondary" onClick={onClearFilters} data-sessions-clear="true">
              Clear filters
            </Button>
          </div>
        </>
      );
    case "failed":
      return (
        <div role="alert" className="flex flex-col gap-sm">
          <Status tone="error">Sessions could not be read</Status>
          <p className="break-words text-caption text-subtle-foreground">{state.reason}</p>
          <div>
            <Button variant="secondary" onClick={onRetry} data-sessions-retry="list">
              Retry
            </Button>
          </div>
        </div>
      );
    case "unavailable":
      return (
        <div role="status" className="flex flex-col gap-sm" data-sessions-unavailable="true">
          <Status tone="warn">Sessions unavailable</Status>
          <p className="break-words text-caption text-subtle-foreground">{state.reason}</p>
        </div>
      );
    case "replaced":
      return (
        <div role="status" className="flex flex-col gap-sm">
          <Status tone="muted">Another window is showing another project's sessions.</Status>
          <div>
            <Button variant="secondary" onClick={onShowHere} data-sessions-show-here="true">
              Show this project's sessions
            </Button>
          </div>
        </div>
      );
  }
}

function SessionRowItem({
  row,
  workspace,
  selected,
  focusable,
  onOpen,
  onRetry,
  onKeyDown,
}: {
  row: SessionRow;
  workspace: Workspace;
  selected: boolean;
  focusable: boolean;
  onOpen: (row: SessionRow) => void;
  onRetry: () => void;
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => void;
}) {
  const title = sessionTitle(row);
  const checkout = sessionCheckout(row, workspace);
  const time = sessionTime(row.updated_at_unix_ms);
  const unavailable = row.unavailable_reason;
  return (
    <li className={`rounded-sm ${selected ? "bg-secondary" : ""}`} data-session={row.id} data-session-available={unavailable ? "false" : "true"}>
      <button
        type="button"
        tabIndex={focusable ? 0 : -1}
        aria-current={selected ? "true" : undefined}
        aria-label={sessionAccessibleName(row, checkout.label, time)}
        title={`${title ?? "Untitled session"}\n${checkout.path}`}
        data-session-row={row.id}
        className="flex w-full flex-col gap-xxs rounded-sm px-sm py-xs text-left outline-none hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
        onClick={() => onOpen(row)}
        onKeyDown={onKeyDown}
      >
        <span className="flex items-center gap-xs text-micro">
          <AgentMark kind={row.provider} />
          <span className={unavailable ? "text-muted-foreground" : "text-subtle-foreground"}>{row.provider_label}</span>
          {time ? <span className="text-muted-foreground">{time}</span> : null}
          {unavailable ? <span className="ml-auto shrink-0 text-warning">! unavailable</span> : null}
        </span>
        <span className={`line-clamp-2 break-words break-keep text-body ${unavailable ? "text-muted-foreground" : title ? "text-foreground" : "italic text-muted-foreground"}`}>{title ?? "Untitled session"}</span>
        <span className={`truncate text-micro ${unavailable ? "text-muted-foreground" : "text-subtle-foreground"}`}>{checkout.label}</span>
      </button>
      {unavailable ? (
        <div className="flex flex-col gap-xs px-sm pb-xs">
          <p className="break-words text-caption text-warning" data-session-reason={row.id}>
            {unavailable}
          </p>
          <div className="flex flex-wrap items-center gap-xs">
            <Button variant="ghost" onClick={onRetry} data-session-retry={row.id}>
              Retry
            </Button>
            <CopySource locator={row.locator} id={row.id} />
          </div>
        </div>
      ) : null}
    </li>
  );
}

/** Copies the session's actual provider file path; says so, or why it could not. */
function CopySource({ locator, id }: { locator: string; id: string }) {
  const [copied, setCopied] = useState<"copied" | "failed" | null>(null);
  useEffect(() => {
    if (!copied) return undefined;
    const timer = window.setTimeout(() => setCopied(null), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const copy = () => {
    const clipboard = navigator.clipboard;
    if (!clipboard) {
      setCopied("failed");
      return;
    }
    void clipboard.writeText(locator).then(
      () => setCopied("copied"),
      () => setCopied("failed"),
    );
  };
  return (
    <>
      <Hint label={locator}>
        <Button variant="ghost" onClick={copy} data-session-copy={id}>
          Copy source location
        </Button>
      </Hint>
      {copied ? (
        <span role="status" className={`text-caption ${copied === "copied" ? "text-success" : "text-warning"}`}>
          {copied === "copied" ? "Copied" : "Could not copy"}
        </span>
      ) : null}
    </>
  );
}

function SessionDetail({
  state,
  rows,
  choosable,
  workspace,
  onRetry,
}: {
  state: DetailState;
  rows: SessionRow[];
  /** Whether the list shows a session to choose; the hint says nothing otherwise. */
  choosable: boolean;
  workspace: Workspace;
  onRetry: () => void;
}) {
  if (state.kind === "none") {
    return choosable ? <p className="m-auto p-lg text-caption text-muted-foreground">Choose a session to read it here.</p> : null;
  }
  const row = rows.find((candidate) => candidate.id === state.detail.session_id) ?? null;
  if (state.kind === "loading") {
    return (
      <>
        {row ? <DetailHeader detail={state.detail} row={row} workspace={workspace} reading /> : null}
        <p role="status" className="p-lg">
          <Status tone="pending">Reading session…</Status>
        </p>
      </>
    );
  }
  if (state.kind === "failed") {
    return (
      <>
        {row ? <DetailHeader detail={state.detail} row={row} workspace={workspace} /> : null}
        <div role="alert" className="flex flex-col gap-sm p-lg" data-session-failure={state.detail.session_id}>
          <Status tone="warn">This session cannot be opened</Status>
          <p className="break-words text-caption text-subtle-foreground">{state.reason}</p>
          <div>
            <Button variant="secondary" onClick={onRetry} data-session-detail-retry="true">
              Retry
            </Button>
          </div>
        </div>
      </>
    );
  }
  const turns = conversationTurns(state.archive);
  return (
    <>
      {row ? <DetailHeader detail={state.detail} row={row} workspace={workspace} reading={state.detail.loading} /> : null}
      {turns.length === 0 ? (
        <p className="p-lg text-caption text-muted-foreground">This session has no readable request or answer.</p>
      ) : (
        <ol
          tabIndex={0}
          className="flex min-h-0 flex-1 flex-col gap-md overflow-auto p-md outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          aria-label="Conversation"
          data-session-turns={turns.length}
        >
          {turns.map((turn, index) => (
            <Turn key={index} turn={turn} provider={state.archive.provider ?? row?.provider_label ?? "Agent"} />
          ))}
        </ol>
      )}
    </>
  );
}

/** The open session as its row names it: provider, checkout, time, the file to copy, and its request as the title. */
function DetailHeader({ detail, row, workspace, reading = false }: { detail: ProjectSessionDetail; row: SessionRow; workspace: Workspace; reading?: boolean }) {
  const title = sessionTitle(row);
  const checkout = sessionCheckout(row, workspace);
  const time = sessionTime(row.updated_at_unix_ms);
  return (
    <header className="flex shrink-0 flex-col gap-xxs border-b border-border bg-card px-md py-sm" data-session-header={detail.session_id}>
      <div className="flex items-center gap-xs text-micro text-subtle-foreground">
        <AgentMark kind={row.provider} />
        <span>{row.provider_label}</span>
        <span className="truncate text-muted-foreground" title={checkout.path}>
          {checkout.label}
        </span>
        {time ? <span className="shrink-0 text-muted-foreground">{time}</span> : null}
        {reading ? (
          <span role="status" className="shrink-0 text-muted-foreground">
            Reading…
          </span>
        ) : null}
        <span className="flex-1" />
        <CopySource key={detail.session_id} locator={detail.locator || row.locator} id="detail" />
      </div>
      <h2 className={`break-words break-keep text-subhead font-semibold ${title ? "text-foreground" : "italic text-muted-foreground"}`}>{title ?? "Untitled session"}</h2>
    </header>
  );
}

function Turn({ turn, provider }: { turn: ArchiveEvent; provider: string }) {
  const person = turn.role === "user";
  const interrupted = turn.kind === "interrupted";
  const time = sessionTime(turn.at_unix_ms);
  return (
    <li className={`flex flex-col gap-xxs ${person ? "rounded-sm bg-card px-sm py-xs" : "px-sm"}`} data-turn={turn.role}>
      <span className="flex items-baseline gap-xs text-micro text-muted-foreground">
        <span className="text-subtle-foreground">{person ? "Request" : provider}</span>
        {time ? <span>{time}</span> : null}
      </span>
      <p className={`whitespace-pre-wrap break-words break-keep text-body ${interrupted ? "italic text-muted-foreground" : "text-foreground"}`}>{turn.text}</p>
    </li>
  );
}
