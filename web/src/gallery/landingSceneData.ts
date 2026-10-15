// The product shots on the introduction site (site/index.html, the three
// pillars): the story of its one-night timeline, drawn by the shell's own
// components. Everything is the gallery's frozen sidebar fixture with its
// invented text replaced, never a second projection: the groups, counts and
// tree shape are what the core wrote for that fixture, and only the words and
// agent kinds a visitor reads are English and match the page. No name here is
// a person, a machine or a path of anyone's.

import type { AgentRow, Checkout, Device, SnapshotRest, Workspace } from "../snapshot";
import type { MobileState } from "../mobileSettings";
import { EMPTY_GALLERY_SCOPE } from "./agentScopes";
import { REFERENCE_FOLDS, sidebarScene } from "./sceneData";

/** What one agent row is called and which CLI runs it, by the fixture's pane id. */
type Words = { title: string; kind: string; detail?: string; pull?: [number, string] };

const AGENT_WORDS: Record<string, Words> = {
  a3: { title: "Approve #431 and #432?", kind: "claude", detail: "Both checked by Claude Code. Tests pass." },
  q1: { title: "#433: CSV or XLSX for the export?", kind: "claude", detail: "A product call the lead won't make for you." },
  q2: { title: "Fix the test that broke the export", kind: "opencode" },
  e1: { title: "Task detail API", kind: "codex" },
  tn1: { title: "Tidy the meeting notes", kind: "claude" },
  a1: { title: "Split the invoice export in three", kind: "claude", pull: [431, "Export invoices as CSV"] },
  a1c1: { title: "Export invoices as CSV", kind: "codex" },
  a1c2: { title: "Check #431 and #432", kind: "claude" },
  a1c3: { title: "Sort the export by date", kind: "codex", pull: [432, "Sort the export by date"] },
  a1g1: { title: "Find why the test broke", kind: "opencode" },
  a1g2: { title: "Add a test for sorted output", kind: "opencode" },
  a1c4: { title: "Pick the CSV column order", kind: "codex", pull: [434, "Pick the CSV column order"] },
  a1c5: { title: "Name the export file", kind: "codex" },
  a1c6: { title: "Add the export button", kind: "codex", pull: [436, "Add the export button"] },
  a1c7: { title: "Handle an empty invoice list", kind: "codex", pull: [435, "Handle an empty invoice list"] },
  a1c8: { title: "Link the export guide", kind: "codex" },
  a2: { title: "Draft the release notes", kind: "claude" },
  a2c1: { title: "Which changes to mention?", kind: "codex", detail: "Should the CSV export lead the notes?" },
  a2c2: { title: "Collect the merged pull requests", kind: "codex" },
};

const PROJECT_LABELS: Record<string, string> = {
  hide: "billing-app",
  sasu: "docs-site",
  "team-notes": "notes",
  "codex-daemon-apply-spec": "mobile-app",
  "sticky-notes": "scratch",
  investment: "pricing-page",
  "landing-page": "blog",
  "modakbul-site": "status-page",
  "old-prototype": "prototype",
  dotfiles: "dotfiles",
  "research-notes": "reading-list",
};

const BRANCHES: Record<string, string> = {
  "hide:155": "feat/433-export-format",
  "hide:154": "fix/export-test",
  "hide:electron": "feat/task-detail-api",
  "hide:ux": "docs/export-guide",
  "hide:fix": "fix/empty-invoice-list",
  "hide:legacy": "export-v1",
  "hide:inactive-0": "feat/export-csv",
};

const PURPOSES: Record<string, string> = {
  "hide:main": "Split the invoice export in three",
  "hide:155": "#433 CSV or XLSX for the export",
  "hide:154": "#434 Fix the test that broke the export",
  "hide:electron": "Task detail API",
  "hide:ux": "Export guide",
  "team-notes:folder": "Tidy the meeting notes",
  "sasu:main": "Docs search",
};

/** Replaces the words of one agent row and leaves everything the core decided as it was. */
function reword(row: AgentRow): AgentRow {
  const words = AGENT_WORDS[row.pane_id];
  if (!words) return row;
  const request = row.request?.pull_requests?.length && words.pull
    ? { ...row.request, pull_requests: row.request.pull_requests.map((pull) => ({ ...pull, number: words.pull![0], title: words.pull![1], url: `https://example.invalid/billing-app/pull/${words.pull![0]}`, head_branch: `agents/${words.pull![0]}` })) }
    : row.request;
  const line = row.state.line && words.detail !== undefined ? { ...row.state.line, text: words.detail } : row.state.line;
  return {
    ...row,
    agent_kind: words.kind,
    identity_label: words.title,
    request,
    state: { ...row.state, line },
  };
}

function renameCheckout(checkout: Checkout): Checkout {
  const branch = BRANCHES[checkout.id];
  const purpose = PURPOSES[checkout.id];
  return {
    ...checkout,
    ...(branch ? { branch, label: branch } : {}),
    ...(purpose ? { purpose: { text: purpose, origin: "operator" as const } } : {}),
    worktree: branch && checkout.worktree ? { ...checkout.worktree, branch } : checkout.worktree,
    pull_request: checkout.pull_request ? { ...checkout.pull_request, title: purpose ?? checkout.pull_request.title } : checkout.pull_request,
  };
}

function renameProject(project: Workspace): Workspace {
  const label = PROJECT_LABELS[project.id];
  return { ...project, ...(label ? { label } : {}), pinned: false, checkouts: project.checkouts.map(renameCheckout) };
}

/** The window's projects and agents for the attention and agents shots: one machine, the page's story. */
export function landingSidebar(nowMs: number, expanded: string[], width: number): { rest: SnapshotRest; agents: AgentRow[] } {
  const base = sidebarScene("reference", { ...REFERENCE_FOLDS, expandedAgents: expanded }, nowMs);
  const navigator = base.rest.navigator!;
  // The one worktree name the core's frozen tiers and states carry is reworded where it sits.
  const reworded = <T,>(value: T): T => JSON.parse(JSON.stringify(value).replaceAll('"feat/ui"', '"feat/export-csv"'));
  const rest: SnapshotRest = reworded({
    ...base.rest,
    navigator: {
      ...navigator,
      workspaces: navigator.workspaces?.map(renameProject),
      sleeping_sessions: navigator.sleeping_sessions?.map((session) => ({ ...session, identity_label: "Sync the scratch notes" })),
    },
    ui_state: {
      ...base.rest.ui_state,
      interface_language: "en",
      agent_logo_mode: "shown",
      sidebar_width: width,
      workspace_registrations: base.rest.ui_state?.workspace_registrations?.map((registration) => ({ ...registration, pinned: false })),
    },
  });
  return { rest, agents: reworded(base.agents.map(reword)) };
}

const NOW_SKEW = 90_000;

const MACHINE = (platform: string): NonNullable<Device["host"]> => ({ consent: "granted", helper_root: "~/.hide/host-helper", cli_dir: "~/.local/bin", contract: 3, bound_identity: "SHA256:example", granted_at_unix_ms: 1, state: "ready", message: null, platform, helper_path: "~/.hide/host-helper/current/hided" });
const KIT: NonNullable<Device["kit"]> = { unavailable: null, busy: false, components: [{ id: "cli", label: "hide command", state: "installed", reason: null, location: "~/.local/bin/hide" }], agents: [], offers_reinstall: false, shares_account_with: null };

/** The sidebar of the attention shot with two more machines registered beside the core's, and one paired phone. */
export function landingDevices(nowMs: number, width: number): { rest: SnapshotRest; agents: AgentRow[]; mobile: MobileState } {
  const scene = landingSidebar(nowMs, [], width);
  const navigator = scene.rest.navigator!;
  const [own, ...others] = navigator.devices ?? [];
  const remote = (id: string, label: string, platform: string, extra: Partial<Device>): Device => ({ ...own!, agent_scope: EMPTY_GALLERY_SCOPE, id, label, kind: "remote", state: "ready", message: null, ssh_alias: id, agent_count: 0, host: MACHINE(platform), kit: KIT, machine_name: null, ...extra });
  const devices: Device[] = [
    { ...own!, machine_name: "Mac mini", host: MACHINE("macos-arm64"), kit: KIT },
    ...others,
    remote("macbook", "MacBook", "macos-arm64", { ssh_alias: null, dials_in: true }),
    remote("workbox", "workbox", "linux-x86_64", {}),
  ];
  const connected = (target: string) => ({ target_id: target, state: "connected", message: null, herdr_version: "0.9.3", session: null });
  const rest = {
    ...scene.rest,
    navigator: { ...navigator, devices },
    status: { ...scene.rest.status, remote: [connected("macbook"), connected("workbox")] },
  } as SnapshotRest;
  const mobile: MobileState = {
    enabled: true,
    exposure: "exposed",
    checklist: { installed: "ok", logged_in: "ok", https: "ok", host_name: "mini.example.ts.net" },
    download_url: "https://example.invalid/tailscale",
    admin_url: "https://example.invalid/admin",
    foreign_target: null,
    failure: null,
    url: "https://mini.example.ts.net/m/",
    qr: null,
    code_expires_at_ms: null,
    phones: [{ id: "phone-1", name: "Phone", last_seen_ms: nowMs - NOW_SKEW, connected: true, notifications: "on", revoke_at_ms: nowMs + 30 * 86_400_000 }],
    max_phones: 5,
    push_mode: "app_closed",
    now_ms: nowMs,
  };
  return { rest, agents: scene.agents, mobile };
}
