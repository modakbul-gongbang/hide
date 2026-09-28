import { describe, expect, it } from "vitest";
import { githubAccess, githubAccessLine, issueSourceChoices } from "./settings";
import type { Checkout, GithubStatus, TaskSource, Workspace } from "./snapshot";

const status = (patch: Partial<GithubStatus>): GithubStatus => ({
  failure_category: null,
  available: true,
  loading: false,
  stale: false,
  last_success_at_unix_ms: null,
  unavailable_reason: null,
  ...patch,
});

const source = (patch: Partial<TaskSource>): TaskSource => ({
  kind: "github",
  label: "GitHub",
  name: "acme/app",
  reading: false,
  failure: null,
  last_read_at_unix_ms: 1,
  chosen: false,
  ...patch,
});

const project = (patch: Partial<Workspace> & { github?: GithubStatus }): Workspace => {
  const { github, ...rest } = patch;
  return {
    id: "w1",
    label: "app",
    path: "/p/app",
    device_id: "local",
    remote_target_id: null,
    is_git: true,
    registered: true,
    temporary: false,
    pinned: false,
    checkouts: github ? [{ id: "c1", github } as Checkout] : [],
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    ...rest,
  };
};

describe("Settings › Issues", () => {
  it("reads gh as connected once any project's read succeeded, and otherwise names gh's own refusal first", () => {
    const loggedOut = project({ github: status({ available: false, failure_category: "not logged in", unavailable_reason: "run gh auth login" }) });
    const offline = project({ id: "w2", github: status({ stale: true, failure_category: "network or rate limit", unavailable_reason: "timeout" }) });
    const noRemote = project({ id: "w3", github: status({ stale: true, failure_category: "no GitHub remote", unavailable_reason: "none of the git remotes" }) });
    const healthy = project({ id: "w4", github: status({ last_success_at_unix_ms: 5 }) });

    expect(githubAccess([offline, healthy])).toEqual({ state: "connected" });
    expect(githubAccess([offline, loggedOut])).toEqual({ state: "failed", category: "not logged in", reason: "run gh auth login" });
    expect(githubAccessLine({ state: "failed", category: "not logged in", reason: null }).text).toBe("gh 로그인 안 됨");
    // A repository with no GitHub remote says nothing about gh, and a device's project is not read here.
    expect(githubAccess([noRemote])).toBeNull();
    expect(githubAccess([project({ remote_target_id: "mini", github: status({ last_success_at_unix_ms: 5 }) })])).toBeNull();
  });

  it("names what Auto resolves to only where the core has shown it, and offers GitHub only to a Git project", () => {
    const onDefault = project({ tasks: { source: source({}), tasks: [], overflow: false } });
    expect(issueSourceChoices(onDefault, undefined)).toEqual({
      value: "auto",
      options: [
        { id: "auto", label: "자동 (GitHub)" },
        { id: "github", label: "GitHub · acme/app" },
        { id: "local", label: "Local" },
      ],
    });

    // Chosen Local: the core has not said what Auto would be, so it is not guessed; the repository still names GitHub.
    const chosenLocal = project({
      tasks: { source: source({ kind: "local", label: "Local", name: null, chosen: true }), tasks: [], overflow: false },
      home_issues: { repository: "acme/app", issues: [], overflow: false },
    });
    const local = issueSourceChoices(chosenLocal, "local");
    expect(local.value).toBe("local");
    expect(local.options.map((option) => option.label)).toEqual(["자동", "GitHub · acme/app", "Local"]);

    // A folder is always Local; a GitHub choice it cannot take reads as Auto.
    const folder = project({ is_git: false, tasks: { source: source({ kind: "local", label: "Local", name: null, chosen: true }), tasks: [], overflow: false } });
    expect(issueSourceChoices(folder, "github")).toEqual({
      value: "auto",
      options: [
        { id: "auto", label: "자동 (Local)" },
        { id: "local", label: "Local" },
      ],
    });
  });
});
