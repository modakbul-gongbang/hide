import { XIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { frontDeviceId } from "./devices";
import { useInterfaceTranslation } from "./i18n/client";
import { boardProjects } from "./navigation";
import { cardOfPane, everyAgent } from "./overviewTasks";
import type { BoardProject } from "./projectBoard";
import { machineName } from "./screenMachine";
import { SessionRecord, type RecordFrame } from "./SessionRecord";
import { sessionOfAgent, workKey } from "./sessionWork";
import type { AgentRow, WorkSession } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore, type OverviewPanel } from "./ui";

/**
 * A task's record beside the Overview's tabs (PRD overview-device-tasks D-07,
 * B16-B21): the same record the lane draws, under the panel's own head of ✕
 * and the ways to the work. The root's record, or a member's when a pane's
 * tree button opened it (B59); a dashed card's record carries the note that
 * says whose task it is (B14).
 */
export function TaskRecordPanel({ panel, rows, actions }: { panel: Extract<OverviewPanel, { kind: "task" | "record" }>; projects: readonly BoardProject[]; rows: ReadonlyMap<string, AgentRow>; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const rest = useShellStore((s) => s.rest);
  const agents = useShellStore((s) => s.agents);
  const everyRow = useMemo(() => (rows.size > 0 ? rows : everyAgent(rest, agents)), [rows, rest, agents]);
  // A linked session's `View conversation` names the session; a card names its pane (B58).
  const agent = panel.kind === "task" ? (everyRow.get(panel.focus ?? panel.root) ?? null) : ([...everyRow.values()].find((row) => row.session_id === panel.session.session_id) ?? null);
  const session = useMemo(() => (panel.kind === "record" ? panel.session : agent ? sessionOfAgent(agent, rest) : null), [panel, agent, rest]);
  const key = session ? workKey(session) : null;
  const [trail, setTrail] = useState<readonly WorkSession[]>([]);
  // The task may live on another device than the one the Overview shows (a dashed card), so it is found among every device's projects.
  const root = panel.kind === "task" ? panel.root : (agent?.pane_id ?? null);
  const owner = useMemo(() => (root ? cardOfPane(boardProjects(rest, agents), root) : null), [rest, agents, root]);
  const from = panel.kind === "task" ? panel.from : undefined;
  const viewing = useUiStore((s) => s.overview.deviceId) ?? frontDeviceId(rest);
  useEffect(() => {
    if (session) actions.readSessionRecord(session);
    // The read follows the session's identity, not each snapshot's copy of it.
  }, [actions, key]);
  const close = () => useUiStore.getState().setOverview({ panel: null });
  const project = owner?.project ?? null;
  const checkout = project && owner ? (owner.card.checkouts.map((id) => project.checkouts.find((row) => row.id === id)).find((row) => row?.exists) ?? null) : null;
  const elsewhere = project !== null && project.device_id !== frontDeviceId(rest);
  const deviceName = project ? machineName(rest?.navigator?.devices, project.device_id, t) : "";
  const head = (
    <div className="flex shrink-0 items-center gap-xs px-sm py-xs" data-record-head="overview">
      <Hint label={t("record.close")}>
        <Button variant="ghost" size="icon-sm" aria-label={t("record.close")} onClick={close} data-record-close="true"><XIcon /></Button>
      </Hint>
      <span className="flex-1" />
      {project && checkout && agent ? (
        elsewhere ? (
          <Button size="sm" variant="secondary" onClick={() => { actions.closeOverview(); actions.focusDevice(project.device_id); }} data-record-go-device={project.device_id}>
            {t("record.goToDevice", { device: deviceName })}
          </Button>
        ) : (
          <>
            <Button size="sm" onClick={() => { actions.closeOverview(); actions.openAgent(agent.pane_id); }} data-record-open-workspace="true">{t("record.openWorkspace")}</Button>
            <Button size="sm" variant="outline" onClick={() => { if (session) actions.viewRecordInLane(project.device_id, project.id, checkout.id, session); }} data-record-in-lane="true">{t("record.viewInLane")}</Button>
          </>
        )
      ) : null}
    </div>
  );
  const note = from && from !== viewing
    ? t("record.incomingNote", { device: machineName(rest?.navigator?.devices, viewing, t), root: machineName(rest?.navigator?.devices, from, t) })
    : null;
  const frame = useMemo<RecordFrame>(() => ({
    kind: "overview",
    head,
    note,
    trail,
    onTrail: setTrail,
    onRead: () => { if (session) actions.readSessionRecord(session); },
    onLeave: () => actions.leaveOverviewRecord(),
  }), [head, note, trail, session, actions]);
  if (!session) {
    return <div className="flex min-h-0 flex-1 flex-col" data-session-record="overview">
      {head}
      {note ? <p className="px-md pb-xs text-caption text-muted-foreground" data-record-note="true">{note}</p> : null}
      <p role="status" className="px-md py-sm text-caption text-muted-foreground" data-record-gone="true">{t("record.gone")}</p>
    </div>;
  }
  return <SessionRecord key={key} actions={actions} target={session} frame={frame} />;
}
