import { useState } from "react";
import type { Actions } from "../actions";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Switch } from "../components/ui/switch";
import { Group, Note, Row, Status } from "../components/settings-rows";
import { useInterfaceTranslation } from "../i18n/client";
import { offeredModels } from "../settings";
import { useShellStore } from "../store";
import { useAgentsDemand } from "./useAgentsDemand";
import { useErrorSince } from "./useErrorSince";

/**
 * Hide AI: the agent Hide uses behind the scenes and the features that use it
 * (PRD settings-cleanup D-14, D-24). The "Let AI name worktrees" value keeps
 * its owner, the issue settings; this tab reads and writes the same value.
 */
export function HideAiTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const ai = useShellStore((s) => s.rest?.status?.background_ai);
  const issueSettings = useShellStore((s) => s.rest?.ui_state?.issue_settings);
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const aiError = useErrorSince(changedAt, ["ai_settings."]);
  const [namesAt, setNamesAt] = useState<number | null>(null);
  const namesError = useErrorSince(namesAt, ["issue_settings."]);
  useAgentsDemand(actions, false);
  const selected = ai?.providers.find((provider) => provider.id === ai.provider) ?? null;
  return (
    <>
      <Group
        title={t("settings.backgroundAi")}
        note={ai?.unavailable_reason ?? t("settings.backgroundAiDescription")}
      >
        <Row label={t("common.agent")} detail={aiError ? <Note tone="error" data-ai-error="true">{t("settings.notSaved", { reason: aiError })}</Note> : null}>
          <Select
            value={ai?.provider ?? undefined}
            disabled={!ai || ai.providers.length === 0}
            onValueChange={(value) => {
              setChangedAt(Date.now());
              actions.chooseAi(value);
            }}
          >
            <SelectTrigger aria-label={t("settings.backgroundAgent")} data-ai-provider="true">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(ai?.providers ?? []).map((provider) => (
                <SelectItem key={provider.id} value={provider.id}>
                  {provider.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Status tone="muted">{ai?.chosen ? t("settings.chosen") : t("settings.defaultChoice")}</Status>
        </Row>
        <Row
          label={t("common.model")}
          detail={selected?.models_unavailable_reason ? <Note>{t("settings.modelsUnavailable", { reason: selected.models_unavailable_reason })}</Note> : null}
        >
          <Select
            value={selected?.model ?? undefined}
            disabled={!selected || offeredModels(selected).length < 2}
            onValueChange={(value) => {
              if (!selected) return;
              setChangedAt(Date.now());
              actions.chooseAi(selected.id, value);
            }}
          >
            <SelectTrigger aria-label={t("settings.backgroundModel")} data-ai-model="true">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(selected ? offeredModels(selected) : []).map((model) => (
                <SelectItem key={model} value={model}>
                  {model}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Row>
        {selected && selected.state !== "ready" && selected.state !== "unread" ? (
          <Row label={<Note tone="warn" data-ai-degraded="true">{t("settings.backgroundDegraded", { agent: selected.label, status: selected.headline || selected.state })}</Note>} />
        ) : null}
      </Group>
      <Group title={t("settings.features")} data-settings-group="hide-ai-features">
        <Row label={t("settings.agentSummary")} detail={<Note>{t("settings.agentSummaryDescription")}</Note>}>
          <Switch
            checked={ai?.agent_summary ?? true}
            disabled={!ai}
            onCheckedChange={(checked) => {
              setChangedAt(Date.now());
              actions.setAgentSummary(checked);
            }}
            aria-label={t("settings.agentSummary")}
            data-ai-agent-summary={String(ai?.agent_summary ?? true)}
          />
        </Row>
        <Row label={t("issueSettings.aiNames")} detail={namesError ? <Note tone="error" data-issue-settings-error="true">{t("settings.notSaved", { reason: namesError })}</Note> : null}>
          <Switch
            checked={issueSettings?.ai_worktree_name ?? true}
            disabled={!issueSettings}
            onCheckedChange={(checked) => {
              setNamesAt(Date.now());
              actions.setIssueSettings({ ai_worktree_name: checked });
            }}
            aria-label={t("issueSettings.aiNames")}
            data-issue-ai-worktree-name={String(issueSettings?.ai_worktree_name ?? true)}
          />
        </Row>
      </Group>
    </>
  );
}
