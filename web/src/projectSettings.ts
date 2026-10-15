import type { KitAgent, SnapshotRest, Workspace } from "./snapshot";
import { agentInstalled, agentUnsupportedHere, supportedAgents } from "./settings/agentRows";

export type RepositoryConnection = { mode: "auto" } | { mode: "repository"; repository: string };
export type BaseBranchChoice = { mode: "auto" } | { mode: "branch"; branch: string };
/** The protections of Working style (`hide_agent_hooks::guardrails::Rules`); every one starts off. */
export type Guardrails = {
  worktree_folder: boolean; base_branch: boolean; agent_merge: boolean; git_commands: boolean; files: boolean; paths: string[];
};
export type ProjectSettings = {
  version: number; device_id: string; root: string;
  repository?: RepositoryConnection; issue_source?: "auto" | "github" | "local";
  new_work?: "checkout" | "worktree"; base_branch?: BaseBranchChoice; guardrails?: Guardrails;
};
export type ProjectSettingsPatch = Partial<Pick<ProjectSettings, "repository" | "issue_source" | "new_work" | "base_branch" | "guardrails">>;

/** The newest record this build reads and writes; version 1 is the same record without guardrails (PRD agent-guardrails D-23). */
export const PROJECT_SETTINGS_VERSION = 2;
export const readableSettings = (record: ProjectSettings | null) => record === null || (record.version >= 1 && record.version <= PROJECT_SETTINGS_VERSION);

/** The paths a new list starts with (B15), and the limits the core refuses past (B16). */
export const DEFAULT_GUARD_PATHS = [".env", ".env.*", "*.pem", "*.key", "~/.ssh/"];
export const GUARD_PATH_LIMIT = 32;
export const GUARD_PATH_LENGTH = 200;

export function projectGuardrails(record: ProjectSettings | null): Guardrails {
  return record?.guardrails ?? { worktree_folder: false, base_branch: false, agent_merge: false, git_commands: false, files: false, paths: [...DEFAULT_GUARD_PATHS] };
}

/** Why `path` cannot join `paths`, as the core would answer, or null; the pattern itself is the core's to judge. */
export function guardPathRefusal(paths: readonly string[], path: string): "project_settings_paths_limit" | "project_settings_path_too_long" | "project_settings_path_duplicate" | "project_settings_path_invalid" | null {
  if (!path.trim()) return "project_settings_path_invalid";
  if (paths.length >= GUARD_PATH_LIMIT) return "project_settings_paths_limit";
  if (path.length > GUARD_PATH_LENGTH) return "project_settings_path_too_long";
  if (paths.includes(path)) return "project_settings_path_duplicate";
  return null;
}

/** The branch the base protection guards: the chosen one, or the remote's default for Auto; null when the project names none (B17). */
export function guardedBase(rest: SnapshotRest | null, workspace: Workspace, record: ProjectSettings | null): string | null {
  const choice = record?.base_branch;
  if (choice?.mode === "branch") return choice.branch;
  const legacy = choice?.mode === "auto" ? undefined : rest?.ui_state?.project_base_branches?.[workspace.path];
  return legacy ?? workspace.default_branch ?? null;
}

/**
 * The agents of the project's machine the guardrails do not reach (B25): installed there, and switched off in Hide
 * or with a hook that is not in place, Codex's untrusted hook included. An agent the system cannot run is no gap.
 */
export function unguardedAgents(agents: readonly KitAgent[] | undefined): KitAgent[] {
  return supportedAgents(agents).filter((agent) => agentInstalled(agent) && !agentUnsupportedHere(agent) && !(agent.enabled && agent.hook?.state === "installed"));
}
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
