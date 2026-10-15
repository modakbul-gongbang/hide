import { AgentMark, HideMark, type BadgeGround } from "./AgentMark";
import { useAgentLogosShown } from "./agentLogoMode";

// Kept apart from AgentMark.tsx, which the phone bundle also imports: the phone
// has no shell store, and this reads it (and the keyboard's hold) to decide.

/**
 * The mark a list row or an agent tab draws before its title, when the
 * operator's view mode shows logos or an ⌥ hold peeks at them (PRD
 * agent-logo-view-mode D-04, D-05); hidden, the row draws nothing here, and a
 * Hide AI-started row stands on its owl alone, as before. `hideAi` is set
 * only where the row stands alone (D-09), never inside a project's Hide AI
 * group, whose head already says who started it.
 */
export function AgentRowMark({ kind, hideAi = null }: { kind: string | null | undefined; hideAi?: BadgeGround | null }) {
  const shown = useAgentLogosShown();
  if (shown) return <AgentMark kind={kind} hideAi={hideAi} />;
  return hideAi ? <HideMark className="size-(--size-agent-badge-compact)" /> : null;
}
