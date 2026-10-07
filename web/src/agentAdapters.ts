// Read the generated Rust contract. No agent support table lives here.
import contract from "../../contracts/agent-adapters.json";

export const AGENT_ADAPTERS = contract;

export function agentAdapter(id: string) {
  return AGENT_ADAPTERS.find((row) => row.id === id);
}
