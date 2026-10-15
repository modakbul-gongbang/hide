// The lane's tools, in the rail's order (PRD right-lane D-01). Kept apart
// from `workspace.ts` so the shortcut registry, which the desktop host's main
// process also reads, can name them without that module's screen graph.

export const TOOLS = ["work", "memory", "files", "changes"] as const;
export type Tool = (typeof TOOLS)[number];
