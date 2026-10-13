import { describe, expect, it } from "vitest";
import type { SnapshotRest, Workspace } from "./snapshot";
import { projectBaseBranch, projectIssueSource, projectSettings, settingsTarget } from "./projectSettings";

const project = { id: "display-id", device_id: "local", path: "/project", label: "project", branches: ["main", "develop"], default_branch: "main" } as Workspace;
const snapshot = () => ({ ui_state: {
  project_issue_sources: { "/project": "local" }, project_base_branches: { "/project": "develop" },
  project_settings: { original: { version: 1, device_id: "local", root: "/project" },
    remote: { version: 1, device_id: "remote", root: "/project", issue_source: "github" } },
} }) as SnapshotRest;

describe("confirmed scoped project settings", () => {
  it("inherits unset legacy fields and lets explicit automatic choices shadow them", () => {
    const rest = snapshot();
    expect(projectIssueSource(rest, project)).toBe("local");
    expect(projectBaseBranch(rest, project)).toEqual({ branch: "develop", missing: null });
    Object.assign(rest.ui_state!.project_settings!.original!, { issue_source: "auto", base_branch: { mode: "auto" } });
    expect(projectIssueSource(rest, project)).toBe("auto");
    expect(projectBaseBranch(rest, project)).toEqual({ branch: "main", missing: null });
  });

  it("matches device plus main root despite display id or primary checkout changes", () => {
    const rest = snapshot();
    expect(projectSettings(rest, settingsTarget({ ...project, id: "another-visible-id" })).record?.device_id).toBe("local");
    expect(projectIssueSource(rest, { ...project, device_id: "remote" })).toBe("github");
    expect(projectSettings(rest, { ...settingsTarget(project), root: "/project.worktrees/feature" }).record).toBeNull();
  });

  it("reports a removed branch with an existing fallback", () => {
    const rest = snapshot();
    rest.ui_state!.project_settings!.original!.base_branch = { mode: "branch", branch: "deleted" };
    expect(projectBaseBranch(rest, project)).toEqual({ branch: "main", missing: "deleted" });
    expect(projectBaseBranch(rest, { ...project, default_branch: "deleted" })).toEqual({ branch: "main", missing: "deleted" });
  });

  it("finds a failed first-write receipt without a confirmed record", () => {
    const rest = snapshot();
    rest.ui_state!.project_settings = {};
    rest.ui_state!.project_settings_writes = { scope: { device_id: "local", root: "/project", request_id: "intent", state: "failed", error: "project_settings_save_failed", patch: { issue_source: "github" } } };
    expect(projectSettings(rest, settingsTarget(project))).toMatchObject({ record: null, write: { request_id: "intent", patch: { issue_source: "github" } } });
    expect(projectSettings(rest, { ...settingsTarget(project), deviceId: "remote" }).write).toBeNull();
  });
  it("finds a correlated busy refusal while another request is pending", () => {
    const rest = snapshot();
    rest.ui_state!.project_settings_writes = {
      active: { device_id: "local", root: "/project", request_id: "active", state: "pending", error: null },
      refusal: { device_id: "local", root: "/project", request_id: "second", state: "failed", error: "project_settings_busy" },
    };
    expect(projectSettings(rest, settingsTarget(project), "second").write).toMatchObject({ request_id: "second", state: "failed", error: "project_settings_busy" });
    expect(projectSettings(rest, settingsTarget(project), "active").write?.state).toBe("pending");
  });

});
