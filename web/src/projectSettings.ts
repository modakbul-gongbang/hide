import type { SnapshotRest, Workspace } from "./snapshot";

export type RepositoryConnection = { mode: "auto" } | { mode: "repository"; repository: string };
export type BaseBranchChoice = { mode: "auto" } | { mode: "branch"; branch: string };
export type ProjectSettings = {
  version: number; device_id: string; root: string;
  repository?: RepositoryConnection; issue_source?: "auto" | "github" | "local";
  new_work?: "checkout" | "worktree"; base_branch?: BaseBranchChoice;
};
export type ProjectSettingsPatch = Partial<Pick<ProjectSettings, "repository" | "issue_source" | "new_work" | "base_branch">>;
export type ProjectSettingsWrite = { request_id: string; state: "pending" | "saved" | "failed"; error: string | null; patch?: ProjectSettingsPatch };
export type ProjectSettingsTarget = { deviceId: string; root: string; workspaceId: string; label: string };

export const settingsTarget = (workspace: Workspace): ProjectSettingsTarget => ({ deviceId: workspace.device_id, root: workspace.path, workspaceId: workspace.id, label: workspace.label });

/** The durable identity belongs to the core. Match its tuple, never a visible checkout id. */
export function projectSettings(rest: SnapshotRest | null, target: ProjectSettingsTarget, requestId?: string): { record: ProjectSettings | null; write: ProjectSettingsWrite | null } {
  const entry = Object.entries(rest?.ui_state?.project_settings ?? {}).find(([, row]) => row.device_id === target.deviceId && row.root === target.root);
  const record = entry?.[1] ?? null;
  // A failed first write has no saved record yet; receipts carry their scope too.
  const write = Object.values(rest?.ui_state?.project_settings_writes ?? {}).find((row) => row.device_id === target.deviceId && row.root === target.root && (!requestId || row.request_id === requestId)) ?? null;
  return { record, write };
}

export function projectIssueSource(rest: SnapshotRest | null, workspace: Workspace): string | undefined {
  return projectSettings(rest, settingsTarget(workspace)).record?.issue_source ?? rest?.ui_state?.project_issue_sources?.[workspace.path];
}

/** New worktrees use the same confirmed override as Git comparison; a deleted branch falls back visibly. */
export function projectBaseBranch(rest: SnapshotRest | null, workspace: Workspace): { branch: string; missing: string | null } {
  const record = projectSettings(rest, settingsTarget(workspace)).record;
  const choice = record?.base_branch;
  const legacy = rest?.ui_state?.project_base_branches?.[workspace.path];
  const wanted = choice?.mode === "branch" ? choice.branch : choice?.mode === "auto" ? null : legacy;
  const branches = workspace.branches ?? [];
  const fallback = workspace.default_branch && branches.includes(workspace.default_branch) ? workspace.default_branch : branches[0] ?? "";
  return { branch: wanted && branches.includes(wanted) ? wanted : fallback, missing: wanted && !branches.includes(wanted) ? wanted : null };
}
