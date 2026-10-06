import { useEffect, useState, type ReactNode } from "react";
import type { Actions } from "../actions";
import { Group, Row } from "../components/settings-rows";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { Switch } from "../components/ui/switch";
import { useInterfaceTranslation } from "../i18n/client";
import type { MessageKey } from "../i18n/catalogs";
import type { FactoryCommand } from "./commands";
import type { FactoryView } from "./model";
import { Refusal } from "./MyTurn";
import { useFactoryRequest } from "./request";

/** A Factory's settings as `hide factory config` answers them (docs/factory.md, Settings). */
export type FactoryConfig = {
  verification: { kind: "ci"; checks: string[] } | { kind: "commands"; commands: string[] } | { kind: "none" };
  merge_mode: "auto" | "manual";
  merge_method: "merge" | "squash" | "rebase";
  quick_check: string | null;
  question_deadline_ms: number;
  stall_ms: number;
  no_report_ms: number;
  watch_interval_ms: number;
  watch_daily_limit: number;
  outside_read_ms: number;
  cancel_keep_ms: number;
  done_fold_ms: number;
  archive_fold_ms: number;
  new_task_limit: number;
  verify_failure_limit: number;
  verify_timeout_ms: number;
  disk_floor_bytes: number;
  default_runtime: "claude" | "codex";
  harness: { name: string; instructions: string } | null;
  autonomy: { id: string; description: string; enabled: boolean }[];
  autonomy_diff_limit: number;
  recovery: string[];
  risk_paths: string[];
  checks: { at: "intake" | "after_done" | "periodic"; instruction: string }[];
  prd_in_issue: boolean;
  macos_notifications: boolean;
  worker_args: Record<string, string[]>;
};

type ConfigAnswer = { config: FactoryConfig; machine: { max_workers: number } };

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const GIB = 1024 * 1024 * 1024;

/** A number setting: its key, the unit the engine takes it in, and how it reads from the config. */
type NumberSetting = { key: string; label: MessageKey; read: (answer: ConfigAnswer) => number };

const per = (unit: number, field: keyof FactoryConfig) => (answer: ConfigAnswer) => Math.round((answer.config[field] as number) / unit);
const plain = (field: keyof FactoryConfig) => (answer: ConfigAnswer) => answer.config[field] as number;

const RUN: NumberSetting[] = [{ key: "max_workers", label: "factory.settings.maxWorkers", read: (answer) => answer.machine.max_workers }];
const VERIFY: NumberSetting[] = [
  { key: "verify_failure_limit", label: "factory.settings.verifyFailureLimit", read: plain("verify_failure_limit") },
  { key: "verify_timeout_minutes", label: "factory.settings.verifyTimeout", read: per(MINUTE, "verify_timeout_ms") },
];
const THRESHOLDS: NumberSetting[] = [
  { key: "question_deadline_hours", label: "factory.settings.questionDeadline", read: per(HOUR, "question_deadline_ms") },
  { key: "stall_minutes", label: "factory.settings.stall", read: per(MINUTE, "stall_ms") },
  { key: "no_report_minutes", label: "factory.settings.noReport", read: per(MINUTE, "no_report_ms") },
  { key: "new_task_limit", label: "factory.settings.newTaskLimit", read: plain("new_task_limit") },
];
const WATCH: NumberSetting[] = [
  { key: "watch_interval_minutes", label: "factory.settings.watchInterval", read: per(MINUTE, "watch_interval_ms") },
  { key: "watch_daily_limit", label: "factory.settings.watchDailyLimit", read: plain("watch_daily_limit") },
  { key: "outside_read_minutes", label: "factory.settings.outsideRead", read: per(MINUTE, "outside_read_ms") },
];
const KEEP: NumberSetting[] = [
  { key: "done_fold_days", label: "factory.settings.doneFold", read: per(DAY, "done_fold_ms") },
  { key: "archive_fold_days", label: "factory.settings.archiveFold", read: per(DAY, "archive_fold_ms") },
  { key: "cancel_keep_days", label: "factory.settings.cancelKeep", read: per(DAY, "cancel_keep_ms") },
];
const AUTONOMY: NumberSetting[] = [{ key: "autonomy_diff_limit", label: "factory.settings.autonomyDiffLimit", read: plain("autonomy_diff_limit") }];
const ADVANCED: NumberSetting[] = [{ key: "disk_floor_gb", label: "factory.settings.diskFloor", read: per(GIB, "disk_floor_bytes") }];

const RECOVERY: readonly { id: string; label: MessageKey }[] = [
  { id: "remove_finished_worktrees", label: "factory.settings.recovery.remove_finished_worktrees" },
  { id: "restart_worker", label: "factory.settings.recovery.restart_worker" },
  { id: "sleep_wake_worker", label: "factory.settings.recovery.sleep_wake_worker" },
  { id: "switch_runtime", label: "factory.settings.recovery.switch_runtime" },
  { id: "retry_reads_and_reconnect", label: "factory.settings.recovery.retry_reads_and_reconnect" },
];

const CHECK_POINTS = ["intake", "after_done", "periodic"] as const;
const CHECK_LABEL: Record<(typeof CHECK_POINTS)[number], MessageKey> = {
  intake: "factory.settings.checkAt.intake",
  after_done: "factory.settings.checkAt.after_done",
  periodic: "factory.settings.checkAt.periodic",
};

/**
 * The settings tab (PRD software-factory-ui B22): every engine default of one
 * Factory in the groups the PRD names, read from and written through the
 * stage-1 `config` command, so a change applies from the engine's next
 * judgment. Closing the Factory waits until no Task runs.
 */
export function FactorySettings({ factories, actions }: { factories: FactoryView[]; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const [picked, setPicked] = useState<string | null>(null);
  const factory = factories.find((view) => view.id === picked) ?? factories[0] ?? null;
  if (!factory) return null;
  return (
    <div className="flex max-w-(--size-settings-sheet-w) flex-col gap-sm px-lg pb-xl" data-factory-settings={factory.id}>
      {factories.length > 1 ? (
        <Select value={factory.id} onValueChange={setPicked}>
          <SelectTrigger size="sm" className="w-auto self-start" aria-label={t("factory.projectFilter")} data-factory-settings-factory="true">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {factories.map((view) => (
              <SelectItem key={view.id} value={view.id}>
                {view.project_name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : null}
      <SettingsBody key={factory.id} factory={factory} actions={actions} />
    </div>
  );
}

function SettingsBody({ factory, actions }: { factory: FactoryView; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const read = useFactoryRequest(actions);
  const write = useFactoryRequest(actions);
  const [answer, setAnswer] = useState<ConfigAnswer | null>(null);
  useEffect(() => {
    read.send({ verb: "config", project: factory.project, set: [] });
  }, [factory.project]);
  useEffect(() => {
    if (read.state.phase === "taken") setAnswer(read.state.answer as unknown as ConfigAnswer);
  }, [read.state]);
  useEffect(() => {
    if (write.state.phase === "taken") setAnswer(write.state.answer as unknown as ConfigAnswer);
  }, [write.state]);
  if (answer === null) {
    return read.state.phase === "refused" ? <Refusal state={read.state} /> : <div className="h-(--size-control-lg) rounded-md bg-muted" aria-busy="true" aria-label={t("factory.loading")} data-factory-settings-loading="true" />;
  }
  const config = answer.config;
  const set = (key: string, value: string) => write.send({ verb: "config", project: factory.project, set: [[key, value]] });
  const send = (command: FactoryCommand) => write.send(command);
  const numbers = (rows: NumberSetting[]) => rows.map((row) => <NumberRow key={row.key} label={t(row.label)} value={row.read(answer)} onCommit={(value) => set(row.key, String(value))} data={row.key} />);
  const verify = config.verification.kind === "commands" ? config.verification.commands.join(" &&& ") : "";
  const running = factory.columns.some((column) => column.cards.some((card) => card.state === "running"));
  return (
    <>
      <Refusal state={write.state} />
      <Group title={t("factory.settings.run")} data-factory-settings-group="run">
        {numbers(RUN)}
        <Row label={t("factory.settings.defaultRuntime")}>
          <Choice value={config.default_runtime} options={[["claude", "Claude Code"], ["codex", "Codex"]]} onChange={(value) => set("default_runtime", value)} data="default_runtime" />
        </Row>
        <Row label={t("factory.settings.harness")} detail={t("factory.settings.harnessDetail")}>
          <TextField value={config.harness ? `${config.harness.name}: ${config.harness.instructions}` : ""} placeholder={t("factory.settings.harnessPlaceholder")} onCommit={(value) => set("harness", value)} data="harness" />
        </Row>
      </Group>
      <Group title={t("factory.settings.verification")} data-factory-settings-group="verification">
        <Row label={t("factory.settings.verificationKind")}>
          <span className="text-body text-subtle-foreground" data-factory-settings-verification={config.verification.kind}>
            {t(config.verification.kind === "ci" ? "factory.create.ci" : config.verification.kind === "commands" ? "factory.create.commands" : "factory.create.none")}
          </span>
        </Row>
        {config.verification.kind === "ci" ? (
          <Row label={t("factory.settings.ciChecks")}>
            <TextField value={config.verification.checks.join(", ")} onCommit={(value) => set("ci", value)} data="ci" />
          </Row>
        ) : (
          <Row label={t("factory.settings.verifyCommands")} detail={t("factory.settings.verifyCommandsDetail")}>
            <TextField value={verify} onCommit={(value) => set("verify", value)} data="verify" />
          </Row>
        )}
        {numbers(VERIFY)}
      </Group>
      <Group title={t("factory.settings.merge")} data-factory-settings-group="merge">
        <Row label={t("factory.create.merge")}>
          <Choice value={config.merge_mode} options={[["auto", t("factory.create.auto")], ["manual", t("factory.create.manual")]]} onChange={(value) => set("merge_mode", value)} data="merge_mode" />
        </Row>
        <Row label={t("factory.settings.mergeMethod")}>
          <Choice value={config.merge_method} options={[["merge", t("factory.settings.method.merge")], ["squash", t("factory.settings.method.squash")], ["rebase", t("factory.settings.method.rebase")]]} onChange={(value) => set("merge_method", value)} data="merge_method" />
        </Row>
        <Row label={t("factory.settings.quickCheck")}>
          <TextField value={config.quick_check ?? ""} onCommit={(value) => set("quick_check", value)} data="quick_check" />
        </Row>
        <Row label={t("factory.settings.riskPaths")} detail={t("factory.settings.riskPathsDetail")}>
          <TextField value={config.risk_paths.join(", ")} onCommit={(value) => set("risk_paths", value)} data="risk_paths" />
        </Row>
      </Group>
      <Group title={t("factory.settings.thresholds")} data-factory-settings-group="thresholds">
        {numbers(THRESHOLDS)}
      </Group>
      <Group title={t("factory.settings.checks")} data-factory-settings-group="checks">
        {numbers(WATCH)}
        {config.checks.map((check, at) => (
          <Row key={at} label={t(CHECK_LABEL[check.at])}>
            <span className="min-w-0 text-body text-subtle-foreground [overflow-wrap:anywhere]">{check.instruction}</span>
          </Row>
        ))}
        <AddCheck onAdd={(at, instruction) => send({ verb: "check", project: factory.project, at, instruction })} />
      </Group>
      <Group title={t("factory.settings.notifyKeep")} data-factory-settings-group="keep">
        <Row label={t("factory.settings.macosNotifications")} detail={t("factory.settings.macosNotificationsDetail")}>
          <Switch checked={config.macos_notifications} aria-label={t("factory.settings.macosNotifications")} data-factory-setting="macos_notifications" onCheckedChange={(on) => set("macos_notifications", on ? "on" : "off")} />
        </Row>
        {numbers(KEEP)}
      </Group>
      <Group title={t("factory.settings.autonomy")} data-factory-settings-group="autonomy">
        {config.autonomy.map((scope) => (
          <Row key={scope.id} label={scope.description}>
            <Switch checked={scope.enabled} aria-label={scope.description} data-factory-setting={`autonomy:${scope.id}`} onCheckedChange={(on) => set("autonomy", `${scope.id}=${on ? "on" : "off"}`)} />
          </Row>
        ))}
        {numbers(AUTONOMY)}
        {RECOVERY.map((action) => (
          <Row key={action.id} label={t(action.label)}>
            <Switch checked={config.recovery.includes(action.id)} aria-label={t(action.label)} data-factory-setting={`recovery:${action.id}`} onCheckedChange={(on) => set("recovery", `${action.id}=${on ? "on" : "off"}`)} />
          </Row>
        ))}
      </Group>
      <Group title={t("factory.settings.advanced")} data-factory-settings-group="advanced">
        {numbers(ADVANCED)}
        <Row label={t("factory.settings.prdInIssue")}>
          <Switch checked={config.prd_in_issue} aria-label={t("factory.settings.prdInIssue")} data-factory-setting="prd_in_issue" onCheckedChange={(on) => set("prd_in_issue", on ? "on" : "off")} />
        </Row>
        {(["claude", "codex"] as const).map((runtime) => (
          <Row key={runtime} label={t("factory.settings.workerArgs", { runtime: runtime === "claude" ? "Claude Code" : "Codex" })}>
            <TextField value={(config.worker_args[runtime] ?? []).join(" ")} onCommit={(value) => set("worker_args", `${runtime}=${value}`)} data={`worker_args:${runtime}`} />
          </Row>
        ))}
        <Row label={t("factory.settings.close")} detail={running ? t("factory.settings.closeRunning") : t("factory.settings.closeDetail")}>
          <Button variant="destructive" size="sm" disabled={running || write.state.phase === "sending"} data-factory-close="true" onClick={() => send({ verb: "close", project: factory.project })}>
            {t("factory.settings.close")}
          </Button>
        </Row>
      </Group>
    </>
  );
}

/** A number the engine takes in a unit; it is sent when the field is left or Enter is pressed, and only when it changed. */
function NumberRow({ label, value, onCommit, data }: { label: string; value: number; onCommit: (value: number) => void; data: string }) {
  return (
    <Row label={label}>
      <TextField value={String(value)} numeric onCommit={(text) => { const next = Number(text); if (Number.isInteger(next) && next >= 0) onCommit(next); }} data={data} />
    </Row>
  );
}

function TextField({ value, onCommit, placeholder, numeric = false, data }: { value: string; onCommit: (value: string) => void; placeholder?: string; numeric?: boolean; data: string }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft.trim());
  };
  return (
    <Input
      value={draft}
      type={numeric ? "number" : "text"}
      className={numeric ? "w-(--size-control-lg)" : "w-(--size-settings-control-w)"}
      placeholder={placeholder}
      aria-label={placeholder}
      data-factory-setting={data}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit();
        if (event.key === "Escape") setDraft(value);
      }}
    />
  );
}

function Choice({ value, options, onChange, data }: { value: string; options: [string, ReactNode][]; onChange: (value: string) => void; data: string }) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger size="sm" className="w-auto" data-factory-setting={data}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map(([option, label]) => (
          <SelectItem key={option} value={option}>
            {label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function AddCheck({ onAdd }: { onAdd: (at: (typeof CHECK_POINTS)[number], instruction: string) => void }) {
  const { t } = useInterfaceTranslation();
  const [at, setAt] = useState<(typeof CHECK_POINTS)[number]>("after_done");
  const [instruction, setInstruction] = useState("");
  return (
    <Row label={t("factory.settings.addCheck")}>
      <Choice value={at} options={CHECK_POINTS.map((point) => [point, t(CHECK_LABEL[point])])} onChange={(value) => setAt(value as (typeof CHECK_POINTS)[number])} data="check_at" />
      <Input value={instruction} className="w-(--size-settings-control-w)" placeholder={t("factory.settings.checkInstruction")} aria-label={t("factory.settings.checkInstruction")} data-factory-setting="check_instruction" onChange={(event) => setInstruction(event.target.value)} />
      <Button size="sm" variant="secondary" disabled={!instruction.trim()} onClick={() => { onAdd(at, instruction.trim()); setInstruction(""); }}>
        {t("factory.settings.add")}
      </Button>
    </Row>
  );
}
