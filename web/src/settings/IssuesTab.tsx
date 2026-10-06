import { TriangleAlertIcon } from "lucide-react";
import { useState } from "react";
import type { Actions } from "../actions";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Switch } from "../components/ui/switch";
import { Hint } from "../components/ui/tooltip";
import { Group, Note, Row, Status } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import { githubAccess, githubAccessLine, issueSourceChoices, type IssueSourceChoice } from "../settings";
import type { IssueSettings } from "../snapshot";
import { useShellStore } from "../store";
import { useErrorSince } from "./useErrorSince";

/**
 * Where each local project's issues live and how work starts from one. The
 * core resolves every source; this tab names what it resolved and sends the
 * operator's choice back as one event.
 */
export function IssuesTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const workspaces = useShellStore((s) => s.rest?.navigator?.workspaces);
  const stored = useShellStore((s) => s.rest?.ui_state?.project_issue_sources);
  const settings = useShellStore((s) => s.rest?.ui_state?.issue_settings);
  const [sourceAt, setSourceAt] = useState<number | null>(null);
  const sourceError = useErrorSince(sourceAt, ["issue_source."]);
  const [settingsAt, setSettingsAt] = useState<number | null>(null);
  const settingsError = useErrorSince(settingsAt, ["issue_settings."]);
  // The projects the boards draw issues for: this Mac's, each with its source;
  // a device's projects keep their issues on that device, and the Home is no project.
  const projects = (workspaces ?? []).filter((workspace) => !workspace.remote_target_id && !workspace.is_home && workspace.tasks?.source != null);
  const access = githubAccess(projects);
  const accessLine = access ? githubAccessLine(access, t) : null;
  const change = (patch: Partial<IssueSettings>) => {
    setSettingsAt(Date.now());
    actions.setIssueSettings(patch);
  };
  return (
    <div data-settings-issues="true">
      <Group title={t("issueSettings.sources")} data-settings-group="issue-sources">
        <Row
          label="GitHub"
          detail={access?.state === "failed" && access.reason ? <Note tone="warn" data-issue-github-reason="true">{access.reason}</Note> : null}
        >
          <span className="text-body text-muted-foreground">{t("issueSettings.githubAccess")}</span>
          {accessLine ? (
            <Status tone={accessLine.tone} data-issue-source-github={access?.state === "failed" ? access.category : "connected"}>
              {accessLine.text}
            </Status>
          ) : null}
        </Row>
        <Row label={t("issueSettings.local")}>
          <span className="text-body text-muted-foreground">{t("issueSettings.localDescription")}</span>
        </Row>
      </Group>
      <Group
        title={t("issueSettings.projects")}
        note={t("issueSettings.projectsDescription")}
        data-settings-group="project-issue-sources"
      >
        {projects.length === 0 ? <Row label={<Note>{t("issueSettings.noProjects")}</Note>} /> : null}
        {projects.map((workspace) => {
          const { value, options } = issueSourceChoices(workspace, stored?.[workspace.path], t);
          const failure = workspace.tasks?.source?.failure ?? null;
          return (
            <Row key={workspace.id} label={<span className="break-words">{workspace.label}</span>}>
              {failure ? (
                <Hint label={failure}>
                  <span className="text-warning" tabIndex={0} data-issue-source-failure={workspace.path}>
                    <TriangleAlertIcon aria-hidden="true" className="size-(--size-icon)" />
                  </span>
                </Hint>
              ) : null}
              <Select
                value={value}
                onValueChange={(next) => {
                  if (next === value) return;
                  setSourceAt(Date.now());
                  actions.setIssueSource(workspace.path, next as IssueSourceChoice);
                }}
              >
                <SelectTrigger
                  aria-label={t("issueSettings.projectSource", { name: workspace.label })}
                  className="w-auto min-w-(--size-settings-control-w)"
                  data-issue-source-project={workspace.path}
                  data-issue-source={value}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {options.map((option) => (
                    <SelectItem key={option.id} value={option.id} data-issue-source-option={option.id}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Row>
          );
        })}
        {sourceError ? <Row label={<Note tone="error" data-issue-source-error="true">{t("settings.notSaved", { reason: sourceError })}</Note>} /> : null}
      </Group>
      <Group title={t("issueSettings.startWork")} data-settings-group="issue-start">
        {settings ? (
          <>
            <Row label={t("issueSettings.aiNames")}>
              <Switch
                checked={settings.ai_worktree_name}
                onCheckedChange={(checked) => change({ ai_worktree_name: checked })}
                aria-label={t("issueSettings.aiNames")}
                data-issue-ai-worktree-name={String(settings.ai_worktree_name)}
              />
            </Row>
            <Row label={t("issueSettings.closesInstruction")}>
              <Switch
                checked={settings.closes_instruction}
                onCheckedChange={(checked) => change({ closes_instruction: checked })}
                aria-label={t("issueSettings.closesInstruction")}
                data-issue-closes-instruction={String(settings.closes_instruction)}
              />
            </Row>
          </>
        ) : (
          <Row label={<Note>{t("settings.notRead")}</Note>} />
        )}
        {settingsError ? <Row label={<Note tone="error" data-issue-settings-error="true">{t("settings.notSaved", { reason: settingsError })}</Note>} /> : null}
      </Group>
    </div>
  );
}
