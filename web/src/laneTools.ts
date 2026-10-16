// The lane's tools, in the rail's order (PRD right-lane D-01; PRD
// sidebar-task-lens D-16, D-17: the Project's Memory and PRs, then the
// checkout's Files and Changes). Kept apart
// from `workspace.ts` so the shortcut registry, which the desktop host's main
// process also reads, can name them without that module's screen graph.

export const TOOLS = ["memory", "prs", "files", "changes"] as const;
export type Tool = (typeof TOOLS)[number];
