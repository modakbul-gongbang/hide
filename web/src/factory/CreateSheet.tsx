import { useEffect, useMemo, useState } from "react";
import { LoaderCircleIcon } from "lucide-react";
import type { Actions } from "../actions";
import { Button } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../components/ui/dialog";
import { RadioGroup, RadioGroupItem } from "../components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../components/ui/select";
import { useInterfaceTranslation } from "../i18n/client";
import { catalogWorkspaces } from "../snapshot";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import type { MergeMode, VerificationChoice } from "./commands";
import { Refusal } from "./MyTurn";
import { useFactoryRequest } from "./request";

/** What `hide factory init` answers before `--confirm` (docs/factory.md, Creating a Factory). */
export type InitPreview = {
  preview: true;
  project: string;
  source: "github" | "local";
  candidates: { kind: "ci" | "verify"; value: string }[];
  merge_mode: MergeMode;
  /** Set when the chosen verification leaves auto unavailable. */
  auto_unavailable: string | null;
  default_runtime: string;
  /** A GitHub Factory's account, repository and everything it reads and writes there; null for a local project (D-62). */
  github: GithubPlan | null;
};

/** The engine words each line of the lists; they carry no code yet (Review question). */
type GithubPlan = { account: string; repo: string; reads: string[]; writes: string[] };

type Verification = { kind: "ci" } | { kind: "commands"; commands: string[] } | { kind: "none" };

function verificationChoice(verification: Verification): VerificationChoice {
  return verification.kind === "ci" ? { kind: "ci", checks: [] } : verification;
}

/** The answer to an init as the sheet reads it: a preview, a Factory already there, or one just made. */
function initFactory(answer: Record<string, unknown> | null): string | null {
  const factory = answer?.factory as { id?: string } | undefined;
  return (answer?.existing === true || answer?.created === true) && typeof factory?.id === "string" ? factory.id : null;
}

/**
 * + Factory 만들기 (PRD software-factory-ui D-09, B3-B6): the project, the
 * verification and the merge mode, nothing else. The engine probes the
 * project and answers a preview, which the sheet shows as it is: the
 * candidates it detected, whether auto is available, and every GitHub read
 * and write before anything is written. The create button sends the same
 * choices with the approval; Cancel leaves nothing behind.
 */
export function CreateSheet({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const open = useUiStore((s) => s.screen?.kind === "factory" && s.screen.place.create);
  const close = () => useUiStore.getState().setFactoryPlace({ create: false });
  return (
    <Dialog open={open} onOpenChange={(next) => (next ? undefined : close())}>
      {open ? (
        <DialogContent aria-describedby={undefined} data-factory-create="true">
          <DialogHeader>
            <DialogTitle>{t("factory.create.title")}</DialogTitle>
          </DialogHeader>
          <CreateForm actions={actions} onClose={close} />
        </DialogContent>
      ) : null}
    </Dialog>
  );
}

function CreateForm({ actions, onClose }: { actions: Actions; onClose: () => void }) {
  const { t } = useInterfaceTranslation();
  const workspaces = useShellStore((s) => s.rest);
  const projects = useMemo(() => catalogWorkspaces(workspaces).filter((workspace) => !workspace.is_home && workspace.device_id === "local"), [workspaces]);
  const [project, setProject] = useState<string | null>(null);
  const [verification, setVerification] = useState<Verification | null>(null);
  const [mode, setMode] = useState<MergeMode>("auto");
  const probe = useFactoryRequest(actions);
  const create = useFactoryRequest(actions);
  const [preview, setPreview] = useState<InitPreview | null>(null);
  const path = projects.find((row) => row.id === project)?.path ?? null;

  // Every change of project or verification asks the engine again; the sheet decides nothing itself.
  useEffect(() => {
    if (path === null) return;
    probe.send({ verb: "init", project: path, verification: verification ? verificationChoice(verification) : null, merge_mode: null, confirm: false });
  }, [path, verification]);
  useEffect(() => {
    if (probe.state.phase !== "taken") return;
    const answer = probe.state.answer as Record<string, unknown>;
    const existing = initFactory(answer);
    if (existing) {
      useUiStore.getState().setFactoryPlace({ create: false, factory: existing, tab: "turn" });
      return;
    }
    const next = answer as unknown as InitPreview;
    setPreview(next);
    // The first answer preselects what the engine detected, the checks before the commands (B3).
    if (verification === null) {
      const first = next.candidates[0];
      setVerification(first?.kind === "ci" ? { kind: "ci" } : first ? { kind: "commands", commands: next.candidates.filter((row) => row.kind === "verify").map((row) => row.value) } : { kind: "none" });
    }
  }, [probe.state, verification]);
  useEffect(() => {
    if (create.state.phase !== "taken") return;
    const made = initFactory(create.state.answer as Record<string, unknown>);
    if (made) useUiStore.getState().setFactoryPlace({ create: false, factory: made, tab: "turn" });
  }, [create.state]);

  const probing = probe.state.phase === "sending";
  const autoBlocked = preview?.auto_unavailable != null;
  const merge: MergeMode = autoBlocked ? "manual" : mode;
  const ready = path !== null && preview !== null && verification !== null && !probing && probe.state.phase !== "refused";
  const projectName = projects.find((row) => row.id === project)?.label ?? "";
  return (
    <>
      <DialogBody className="flex flex-col gap-lg">
        <DialogDescription className="sr-only">{t("factory.create.title")}</DialogDescription>
        <Step title={t("factory.create.project")}>
          <Select value={project ?? undefined} onValueChange={(value) => { setProject(value); setVerification(null); setPreview(null); }}>
            <SelectTrigger aria-label={t("factory.create.project")} data-factory-create-project="true">
              <SelectValue placeholder={t("factory.create.pickProject")} />
            </SelectTrigger>
            <SelectContent>
              {projects.map((row) => (
                <SelectItem key={row.id} value={row.id} data-factory-create-project-option={row.label}>
                  {row.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Step>
        {path !== null ? (
          <Step title={t("factory.create.verification")}>
            {preview === null && probing ? (
              <span className="flex items-center gap-xs text-caption text-muted-foreground" data-factory-create-detecting="true">
                <LoaderCircleIcon aria-hidden="true" className="size-(--size-icon-sm) animate-spin" />
                {t("factory.create.detecting")}
              </span>
            ) : preview ? (
              <VerificationStep preview={preview} value={verification} onChange={setVerification} />
            ) : null}
            <Refusal state={probe.state} />
          </Step>
        ) : null}
        {preview ? (
          <Step title={t("factory.create.merge")}>
            <RadioGroup value={merge} onValueChange={(value) => setMode(value as MergeMode)} aria-label={t("factory.create.merge")}>
              <label className="flex items-center gap-sm text-body">
                <RadioGroupItem value="auto" disabled={autoBlocked} data-factory-create-merge="auto" />
                {t("factory.create.auto")}
              </label>
              {autoBlocked ? <span className="pl-xl text-caption text-muted-foreground" data-factory-create-auto-blocked="true">{t("factory.create.autoBlocked")}</span> : null}
              <label className="flex items-center gap-sm text-body">
                <RadioGroupItem value="manual" data-factory-create-merge="manual" />
                {t("factory.create.manual")}
              </label>
            </RadioGroup>
          </Step>
        ) : null}
        {preview?.github ? <GithubStep plan={preview.github} /> : null}
        <Refusal state={create.state} />
        {create.state.phase === "refused" && typeof (create.state.answer?.detail as { stage?: unknown } | undefined)?.stage === "string" ? (
          <span className="text-caption text-muted-foreground" data-factory-create-stage="true">
            {t("factory.create.failedStage", { stage: (create.state.answer!.detail as { stage: string }).stage })}
          </span>
        ) : null}
      </DialogBody>
      <DialogFooter>
        <Button variant="ghost" data-factory-create-cancel="true" onClick={onClose}>
          {t("common.cancel")}
        </Button>
        <Button
          data-factory-create-confirm="true"
          disabled={!ready || create.state.phase === "sending"}
          aria-busy={create.state.phase === "sending"}
          onClick={() => {
            if (path === null || verification === null) return;
            create.send({ verb: "init", project: path, verification: verificationChoice(verification), merge_mode: merge, confirm: true });
          }}
        >
          {create.state.phase === "sending" ? <LoaderCircleIcon className="animate-spin" /> : null}
          {projectName ? t("factory.create.confirm", { project: projectName }) : t("factory.create.open")}
        </Button>
      </DialogFooter>
    </>
  );
}

function Step({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-sm">
      <h3 className="text-subhead font-semibold">{title}</h3>
      {children}
    </section>
  );
}

/** The candidates the engine detected (B3): the required checks, or a set of verify commands, or none. */
function VerificationStep({ preview, value, onChange }: { preview: InitPreview; value: Verification | null; onChange: (value: Verification) => void }) {
  const { t } = useInterfaceTranslation();
  const ci = preview.candidates.find((row) => row.kind === "ci");
  const commands = preview.candidates.filter((row) => row.kind === "verify").map((row) => row.value);
  const chosen = value?.kind === "commands" ? value.commands : [];
  return (
    <RadioGroup value={value?.kind ?? ""} onValueChange={(kind) => onChange(kind === "ci" ? { kind: "ci" } : kind === "commands" ? { kind: "commands", commands } : { kind: "none" })} aria-label={t("factory.create.verification")}>
      {ci ? (
        <label className="flex items-start gap-sm text-body">
          <RadioGroupItem value="ci" className="mt-xxs" data-factory-create-verification="ci" />
          <span className="flex min-w-0 flex-col">
            {t("factory.create.ci")}
            <span className="font-mono text-caption text-muted-foreground [overflow-wrap:anywhere]">{ci.value}</span>
          </span>
        </label>
      ) : null}
      {commands.length > 0 ? (
        <div className="flex flex-col gap-xs">
          <label className="flex items-center gap-sm text-body">
            <RadioGroupItem value="commands" data-factory-create-verification="commands" />
            {t("factory.create.commands")}
          </label>
          {value?.kind === "commands" ? (
            <div className="flex flex-col gap-xs pl-xl">
              {commands.map((command) => (
                <label key={command} className="flex items-center gap-sm font-mono text-caption">
                  <Checkbox
                    checked={chosen.includes(command)}
                    data-factory-create-command={command}
                    onCheckedChange={(checked) => {
                      const next = checked ? [...chosen, command] : chosen.filter((other) => other !== command);
                      onChange(next.length > 0 ? { kind: "commands", commands: commands.filter((other) => next.includes(other)) } : { kind: "none" });
                    }}
                  />
                  <span className="[overflow-wrap:anywhere]">{command}</span>
                </label>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}
      <label className="flex items-center gap-sm text-body">
        <RadioGroupItem value="none" data-factory-create-verification="none" />
        {t("factory.create.none")}
      </label>
    </RadioGroup>
  );
}

/** Everything the Factory will read and write on GitHub, written before the button (B5, D-62). */
function GithubStep({ plan }: { plan: GithubPlan }) {
  const { t } = useInterfaceTranslation();
  return (
    <Step title={t("factory.create.github")}>
      <dl className="grid grid-cols-[auto_1fr] gap-x-md gap-y-xs text-body" data-factory-create-github="true">
        <dt className="text-muted-foreground">{t("factory.create.account")}</dt>
        <dd className="font-mono [overflow-wrap:anywhere]" data-factory-create-account={plan.account}>{plan.account}</dd>
        <dt className="text-muted-foreground">{t("factory.create.repo")}</dt>
        <dd className="font-mono [overflow-wrap:anywhere]" data-factory-create-repo={plan.repo}>{plan.repo}</dd>
        <dt className="text-muted-foreground">{t("factory.create.reads")}</dt>
        <dd>
          <ul className="flex flex-col gap-xxs" data-factory-create-reads={plan.reads.length}>
            {plan.reads.map((line) => <li key={line} className="[overflow-wrap:anywhere]">{line}</li>)}
          </ul>
        </dd>
        <dt className="text-muted-foreground">{t("factory.create.writes")}</dt>
        <dd>
          <ul className="flex flex-col gap-xxs" data-factory-create-writes={plan.writes.length}>
            {plan.writes.map((line) => <li key={line} className="[overflow-wrap:anywhere]">{line}</li>)}
          </ul>
        </dd>
      </dl>
    </Step>
  );
}
