// Literal core wire facts pin what the operator sees; no web state ladder.
import { expect, it } from "vitest";
import { emptyScope } from "../test/legacyAgentScope";
import { legacyAgentRow } from "../test/legacyAgentRow";
import type { AgentScope } from "./agentScope";
import { sessionsModel } from "./sessionPanel";
import type { AgentRow, Checkout, SessionGroup, SnapshotRest, Workspace } from "./snapshot";

function fixture() {
  const row = (id: string, group: SessionGroup): AgentRow => ({
    ...legacyAgentRow({ id, pane_id: id, identity_label: `긴 한국어 작업 제목 ${id}`, agent_kind: "codex", symbol: "○", group: "seen", demand: "question", unread: false, status_code: "question", emphasized: false, changed_at_unix_ms: 1 }),
    state: { ...legacyAgentRow({}).state, session: { group, line: "확인할 결과 한 줄", unfinished: false } },
    request: { verb: group === "needs_you" ? "fix" : "working", verb_since_unix_ms: 1, line: "확인할 결과 한 줄", request: null, later_by: null, reply: null, pull_requests: [] },
  });
  const agents = [row("first", "needs_you"), row("second", "working")];
  const scope = (members: string[]): AgentScope => ({ ...emptyScope(),
    members: members.map((pane_id) => ({ pane_id, project_id: "project", checkout_id: pane_id === "first" ? "main" : "feature" })),
    sessions: { groups: members.map((id, member) => ({ group: id === "first" ? "needs_you" as const : "working" as const, members: [member], more: [] })) },
  });
  const checkouts = ["main", "feature"].map((id, index) => ({ id, workspace_id: "project", label: id, branch: id, path: `/fixture/${id}`, agent_scope: scope([agents[index]!.pane_id]), tabs: [{ id: `tab-${id}`, panes: [{ id: agents[index]!.pane_id }] }] })) as Checkout[];
  const project = { id: "project", device_id: "local", label: "프로젝트", path: "/fixture", is_git: true, agent_scope: scope(["first", "second"]), checkouts } as Workspace;
  const rest = { navigator: { devices: [{ id: "local", kind: "local", label: "Local", agent_scope: project.agent_scope }], focused_device_id: "local", focused_checkout_id: "main", workspaces: [project] } } as unknown as SnapshotRest;
  return { rest, agents, project };
}

it("resolves project and checkout members using the core's groups", () => {
  const { rest, agents } = fixture();
  const all = sessionsModel(rest, agents, null);
  expect(all.project?.id).toBe("project");
  expect(all.members.map((row) => row.agent.pane_id)).toEqual(["first", "second"]);
  expect(all.scope?.sessions.groups.map((group) => group.group)).toEqual(["needs_you", "working"]);
  const front = sessionsModel(rest, agents, "main");
  expect(front.members.map((row) => row.agent.pane_id)).toEqual(["first"]);
  expect(front.scope?.sessions.groups.map((group) => group.group)).toEqual(["needs_you"]);
});

