// What the Agents tab shows for each machine (PRD settings-cleanup D-06 to
// D-13, B8 to B20, B67): the seven supported agents in the kit's order, which
// of them the machine has, and the one line each row owes the operator. Every
// value is the kit snapshot's; nothing here decides what an agent supports.

import type { Device, KitAgent, KitPiece, RemoteStatus } from "../snapshot";
import { deviceState, kitPartNeedsReinstall } from "../settings";

/** The agents Hide supports, in the order every machine lists them (D-06). */
export const SUPPORTED_AGENTS = ["claude-code", "codex", "gemini-cli", "grok", "opencode", "pi", "cursor"] as const;

/**
 * Where an agent's own installation guide lives, for the Install link of a row
 * whose program the machine does not have (B8). The kit's `doc_url` is its
 * skills page and the row's Docs link; these are the vendors' installation
 * pages, each read on 2026-10-06.
 */
const INSTALL_DOCS: Record<string, string> = {
  "claude-code": "https://code.claude.com/docs/en/setup",
  codex: "https://learn.chatgpt.com/docs/codex/cli",
  "gemini-cli": "https://geminicli.com/docs/get-started/installation/",
  grok: "https://github.com/xai-org/grok-build",
  opencode: "https://opencode.ai/docs/",
  pi: "https://pi.dev/",
  cursor: "https://cursor.com/docs/cli/installation",
};

/**
 * Where an agent's own documentation lives, for the row's Docs link. A table
 * of this build, never the kit's `doc_url`: a device's helper reports that
 * field, and a link the page draws must not come from a machine it does not
 * run on.
 */
const DOCS: Record<string, string> = {
  "claude-code": "https://code.claude.com/docs/en/skills",
  codex: "https://learn.chatgpt.com/docs/build-skills",
  "gemini-cli": "https://geminicli.com/docs/cli/skills/",
  grok: "https://docs.x.ai/build/features/skills-plugins-marketplaces",
  opencode: "https://opencode.ai/docs/skills/",
  pi: "https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md",
  cursor: "https://cursor.com/docs/context/skills",
};

/** The documentation page of an agent Hide supports, or null for any other id. */
export function docsUrl(agentId: string): string | null {
  return DOCS[agentId] ?? null;
}

/** The vendor's installation page for an agent, or null for one Hide does not support. */
export function installDocUrl(agentId: string): string | null {
  return INSTALL_DOCS[agentId] ?? null;
}

/**
 * The seven supported agents of one machine in the supported order. An older
 * helper that still lists agents Hide dropped shows none of them: the list is
 * the supported set, whatever a machine reports.
 */
export function supportedAgents(agents: readonly KitAgent[] | undefined): KitAgent[] {
  const byId = new Map((agents ?? []).map((agent) => [agent.id, agent]));
  return SUPPORTED_AGENTS.flatMap((id) => {
    const agent = byId.get(id);
    return agent ? [agent] : [];
  });
}

/**
 * Whether the machine has the agent: its program was found (D-07, B9). An
 * agent the operator switched on, whose program then went away, keeps its row
 * and its switch so it can be turned off. An agent that is on only because
 * Claude Code and Codex are on by default is not kept: its program is not
 * there, so it is not installed (`chosen` is the record holding the
 * operator's own choice).
 */
export function agentInstalled(agent: KitAgent): boolean {
  return agent.availability === "available" || (agent.enabled && agent.chosen === true);
}

/** One machine of the switch at the top of the tab: This Mac first, then each device in the Devices order. */
export type AgentMachine = {
  device: Device;
  installed: KitAgent[];
  notInstalled: KitAgent[];
  /**
   * Why the machine shows no list (B11): its kit does not run, the device is
   * still connecting, was switched off in Devices, or cannot be reached. The
   * words are the Devices tab's own (`deviceState`), so the tabs cannot disagree.
   */
  blocked: "unreachable" | "connecting" | "disabled" | "unavailable" | null;
  /** The kit has not answered for this machine yet. */
  unread: boolean;
};

export function agentMachines(devices: readonly Device[], remote?: readonly RemoteStatus[]): AgentMachine[] {
  return devices
    .map((device) => {
      const agents = supportedAgents(device.kit?.agents);
      // A device that is not ready answers nothing, so its last list is not a fact to act on (B11).
      const { state } = deviceState(device, remote?.find((row) => row.target_id === device.id));
      const gone = state === "connecting" || state === "disabled" ? state : state === "unavailable" ? "unreachable" : null;
      return {
        device,
        installed: agents.filter(agentInstalled),
        notInstalled: agents.filter((agent) => !agentInstalled(agent)),
        blocked: gone ?? (device.kit?.unavailable ? "unavailable" : null),
        unread: agents.length === 0 && !device.kit?.unavailable,
      };
    });
}

/**
 * What Check again says when the machine's last read failed (B10): the core
 * sends a code, never prose, and a code this build does not know reads as the
 * general line (the cause is in the diagnostic log, B68).
 */
export function checkFailedReason(code: string): "agents.checkReason.unreachable" | "agents.checkReason.timed_out" | "agents.checkReason.failed" {
  if (code === "unreachable") return "agents.checkReason.unreachable";
  if (code === "timed_out") return "agents.checkReason.timed_out";
  return "agents.checkReason.failed";
}

/** The pieces of one agent that a Reinstall would repair, the failed ones first (B13, B20). */
export type AgentProblem = { part: "skill" | "hook" | "herdr"; piece: KitPiece };

export function agentProblems(agent: KitAgent): AgentProblem[] {
  if (!agent.enabled) return [];
  const pieces: AgentProblem[] = [{ part: "skill", piece: agent.skill }];
  if (agent.hook) pieces.push({ part: "hook", piece: agent.hook });
  if (agent.herdr) pieces.push({ part: "herdr", piece: agent.herdr });
  const broken = pieces.filter(({ piece }) => kitPartNeedsReinstall(piece));
  return [...broken.filter(({ piece }) => piece.state === "failed"), ...broken.filter(({ piece }) => piece.state !== "failed")];
}

/**
 * A switched-off agent whose removal did not finish: something of Hide's is
 * still there, and its row says so instead of reading Off (B14).
 */
export function agentLeftover(agent: KitAgent): KitPiece | null {
  if (agent.enabled) return null;
  return [agent.skill, agent.hook, agent.herdr].find((piece): piece is KitPiece => piece?.state === "failed") ?? null;
}

/**
 * The status an agent that is on wears (B16, B19): how many of its sessions
 * run now, or Ready when it is set up and has none. A Partial agent, one that
 * is off and one whose sessions were not read yet wear none.
 */
export type AgentStatus = { kind: "none" } | { kind: "ready" } | { kind: "sessions"; count: number };

export function agentStatus(agent: KitAgent): AgentStatus {
  // An agent whose program is gone has no sessions and is never Ready (B9).
  if (!agent.enabled || agent.partial || agent.sessions == null || agent.availability !== "available") return { kind: "none" };
  if (agent.sessions === 0) return { kind: "ready" };
  return { kind: "sessions", count: agent.sessions };
}
