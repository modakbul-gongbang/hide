// What the Agents tab shows for each machine (PRD settings-cleanup D-06 to
// D-13, B8 to B20, B67): the seven supported agents in the kit's order, which
// of them the machine has, and the one line each row owes the operator. Every
// value is the kit snapshot's; nothing here decides what an agent supports.

import type { Device, KitAgent, KitPiece } from "../snapshot";
import { kitPartNeedsReinstall } from "../settings";

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
 * Whether the machine has the agent: its program was found (D-07), or the
 * agent is on, which keeps its row and its switch so it can be turned off
 * after its program went away.
 */
export function agentInstalled(agent: KitAgent): boolean {
  return agent.availability === "available" || agent.enabled;
}

/** One machine of the switch at the top of the tab: This Mac first, then each device in the Devices order. */
export type AgentMachine = {
  device: Device;
  installed: KitAgent[];
  notInstalled: KitAgent[];
  /** Why the machine shows no list: its kit does not run, or the device cannot be reached (B11). */
  blocked: "unreachable" | "unavailable" | null;
  /** The kit has not answered for this machine yet. */
  unread: boolean;
};

export function agentMachines(devices: readonly Device[]): AgentMachine[] {
  return devices
    .filter((device) => device.kind === "remote" || device.id === "local")
    .map((device) => {
      const agents = supportedAgents(device.kit?.agents);
      // A device that is not ready answers nothing, so its last list is not a fact to act on (B11).
      const unreachable = device.kind === "remote" && device.state !== "ready";
      return {
        device,
        installed: agents.filter(agentInstalled),
        notInstalled: agents.filter((agent) => !agentInstalled(agent)),
        blocked: unreachable ? "unreachable" : device.kit?.unavailable ? "unavailable" : null,
        unread: agents.length === 0 && !device.kit?.unavailable,
      };
    });
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
 * The status an agent that is on wears (B16, B19): what is connected, what is
 * not, or Ready when it is set up and has no session. A Partial agent, one
 * that is off and one whose sessions were not read yet wear none.
 */
export type AgentStatus =
  | { kind: "none" }
  | { kind: "ready" }
  | { kind: "sessions"; connected: number; notConnected: number };

export function agentStatus(agent: KitAgent): AgentStatus {
  if (!agent.enabled || agent.partial || !agent.sessions) return { kind: "none" };
  const { connected, not_connected, not_connected_hidden } = agent.sessions;
  const notConnected = not_connected.length + not_connected_hidden;
  if (connected === 0 && notConnected === 0) return { kind: "ready" };
  return { kind: "sessions", connected, notConnected };
}
