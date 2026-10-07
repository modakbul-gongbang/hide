// Fixed pre-refactor example wire values. Gallery controls change text and
// clocks; status decisions are data, never another projection implementation.
import type { AgentState } from "../snapshot";
import states from "./agentStates.json";

export function galleryAgentState(pane: string, text: string | null | undefined, changed: number): AgentState {
  const preset = states.presets[states.panes[pane as keyof typeof states.panes]] as AgentState;
  if (!preset) throw new Error(`Missing gallery agent state: ${pane}`);
  return { ...preset, line: preset.line ? { ...preset.line, text: text ?? "" } : null, request_since: changed };
}
