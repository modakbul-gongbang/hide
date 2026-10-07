// Read the generated Rust contract. No agent support table lives here.
import contract from "../../contracts/agent-adapters.json";

export const AGENT_ADAPTERS = contract;

export function agentAdapter(id: string) {
  const normalized = id.trim().toLowerCase();
  return AGENT_ADAPTERS.find((row) => row.id === normalized || row.aliases.some((alias) => alias === normalized));
}
