import { CopyIcon } from "lucide-react";
import { useState } from "react";
import type { Actions } from "../actions";
import { Button } from "../components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../components/ui/dialog";
import { checkCommand, moveCopy, stepMarks, type MoveView, type StepMark } from "../coreMove";
import { useInterfaceTranslation } from "../i18n/client";
import { takesEun, takesEuro } from "../i18n/koParticle";
import { cn } from "../lib/utils";
import { moveMachines } from "../screenMachine";
import { useShellStore } from "../store";

/** What the operator asked from Settings: the core toward a device, or back to this machine from a node's window. */
export type MoveRequest = { direction: "forward"; device: string } | { direction: "back" };

/** The dialog's five forms (PRD core-host-node-move B, approved round 3), and the wait before the first answer. */
export type MoveForm = "checking" | "checks_failed" | "confirm" | "moving" | "failed" | "done";

/**
 * Which form the move's frame puts the dialog in. Until the supervisor
 * answers this request (`asked` is the view the request was sent beside),
 * and for a frame about another move, the dialog waits on its checks.
 */
export function moveForm(view: MoveView | null, asked: MoveView | null, request: MoveRequest): MoveForm {
  if (!view || view === asked) return "checking";
  const direction = view.direction ?? "forward";
  if (direction !== request.direction || (request.direction === "forward" && view.device !== request.device)) return "checking";
  switch (view.state) {
    case "checks_failed":
      return "checks_failed";
    case "ready":
      return "confirm";
    case "stopping":
    case "copying":
    case "starting":
    case "linking":
    case "rolling_back":
    case "waiting":
      return "moving";
    case "rolled_back":
      return "failed";
    case "done":
      return "done";
    default:
      return "checking";
  }
}

/** Sends `request` and answers the frame it was sent beside, which the dialog waits past. */
export function askMove(actions: Actions, request: MoveRequest, start: boolean): MoveView | null {
  const asked = useShellStore.getState().coreMove;
  if (request.direction === "back") actions.coreMove({ action: start ? "back" : "check_back" });
  else actions.coreMove({ action: start ? "start" : "check", device: request.device });
  return asked;
}

// A step's line and its mark: only a done step's ✓ is green, its words stay the body's.
const MARK: Record<StepMark, { glyph: string; line: string; mark: string }> = {
  done: { glyph: "✓", line: "text-foreground", mark: "text-success" },
  run: { glyph: "…", line: "font-semibold text-foreground", mark: "text-foreground" },
  todo: { glyph: "-", line: "text-muted-foreground", mark: "" },
  fail: { glyph: "✕", line: "text-destructive", mark: "" },
};

/**
 * The move as one dialog over Settings (PRD core-host-node-move B, the
 * operator's choice): its body turns from the checks to the consequences to
 * the steps to the result, and every form names the reason first. The
 * dialog only asks; the supervisor's frame says where the move is, so a
 * window that closed it and opens Settings again finds the same state.
 */
export function CoreMoveDialog({ request, asked: first, actions, onClose, onRequest }: { request: MoveRequest; asked: MoveView | null; actions: Actions; onClose: () => void; onRequest: (request: MoveRequest) => void }) {
  const { t } = useInterfaceTranslation();
  const view = useShellStore((s) => s.coreMove);
  const refusal = useShellStore((s) => s.coreMoveRefusal);
  const devices = useShellStore((s) => s.rest?.navigator?.devices);
  // The frame the last request was sent beside: the dialog waits until a newer one answers it.
  const [asked, setAsked] = useState<MoveView | null>(first);
  const ask = (next: MoveRequest, start: boolean) => setAsked(askMove(actions, next, start));
  const form = moveForm(view, asked, request);
  const machines = moveMachines(devices, request.direction === "back" ? { direction: "back", device: null } : { direction: "forward", device: request.device }, t);
  const values = moveCopy(machines);
  const back = request.direction === "back";
  // Korean writes the particle after the machine's name by its last sound (`koParticle`).
  const euro = takesEuro(machines.to);
  const why = back ? null : takesEun(machines.to) ? t("coreMove.whyEun", values) : t("coreMove.why", values);
  const title = (() => {
    switch (form) {
      case "checking":
      case "checks_failed":
        return back ? t("coreMove.back.title", values) : euro ? t("coreMove.titleEuro", values) : t("coreMove.title", values);
      case "confirm":
        return back ? t("coreMove.back.confirmTitle", values) : euro ? t("coreMove.confirmTitleEuro", values) : t("coreMove.confirmTitle", values);
      case "moving":
        if (view?.state === "rolling_back") return t("coreMove.rollingBackTitle");
        return back ? t("coreMove.back.movingTitle", values) : euro ? t("coreMove.movingTitleEuro", values) : t("coreMove.movingTitle", values);
      case "failed":
        return t("coreMove.failedTitle");
      case "done":
        return back ? t("coreMove.back.doneTitle", values) : euro ? t("coreMove.doneTitleEuro", values) : t("coreMove.doneTitle", values);
    }
  })();
  const failed = view?.failed ?? [];
  const passed = view?.checked === undefined ? null : Math.max(0, view.checked - failed.length);
  const peer = back ? machines.from : machines.to;
  return (
    <Dialog open onOpenChange={(next) => { if (!next) onClose(); }}>
      {/* The move stops the core: no button is the default, whichever form opens (design 6). */}
      <DialogContent showCloseButton initialFocus="container" data-core-move-dialog={form} data-core-move-direction={request.direction}>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {why && (form === "checking" || form === "checks_failed" || form === "confirm") ? <DialogDescription>{why}</DialogDescription> : null}
        </DialogHeader>
        <DialogBody className="space-y-sm">
          {form === "checking" ? (
            refusal ? (
              <p className="text-body text-destructive" data-core-move-refused={refusal}>
                {refusal === "move_busy" || refusal === "core_pending" ? t("coreMove.refused.busy") : refusal === "move_unavailable" || refusal === "moving" ? t("coreMove.refused.unavailable") : t("coreMove.refused.other")}
              </p>
            ) : (
              <p className="text-body text-muted-foreground" data-core-move-checking="true">{t("coreMove.checking")}</p>
            )
          ) : null}
          {form === "checks_failed" ? (
            <>
              <p className="text-caption text-muted-foreground" data-core-move-count="true">
                {passed === null ? t("coreMove.toFix", { failed: failed.length }) : t("coreMove.toFixPassed", { failed: failed.length, passed })}
              </p>
              <ul className="space-y-xs">
                {failed.map((row) => (
                  <FailedCheck key={row.check} check={row.check} detail={row.detail} machine={machines.to} actions={actions} />
                ))}
              </ul>
            </>
          ) : null}
          {form === "confirm" ? (
            <>
              <p className="text-body text-foreground">{t("coreMove.consequence")}</p>
              <p className="text-body text-muted-foreground">{back ? t("coreMove.back.reversible") : t("coreMove.reversible")}</p>
            </>
          ) : null}
          {form === "moving" && view ? (
            <>
              <ol className="space-y-xs" data-core-move-steps="true">
                {stepMarks(view).map(({ step, mark }) => (
                  <li key={step} className={cn("flex items-baseline gap-sm text-body", MARK[mark].line)} data-core-move-step={step} data-core-move-mark={mark}>
                    <span aria-hidden="true" className={cn("w-(--size-icon) shrink-0 text-center", MARK[mark].mark)}>{MARK[mark].glyph}</span>
                    <span>{t(`coreMove.step.${step}`, values)}</span>
                  </li>
                ))}
              </ol>
              {view.state === "waiting" ? <p className="text-body text-warning" data-core-move-waiting="true">{t("coreMove.waiting", { machine: peer })}</p> : null}
            </>
          ) : null}
          {form === "failed" && view ? (
            <>
              <p className="text-body text-destructive" data-core-move-failed={view.step ?? "check"}>
                <span aria-hidden="true">× </span>
                {t(`coreMove.failedStep.${view.step ?? "check"}`, values)}
              </p>
              <p className="text-body text-success">
                <span aria-hidden="true">✓ </span>
                {t("coreMove.unchanged", values)}
              </p>
            </>
          ) : null}
          {form === "done" ? <p className="text-body text-subtle-foreground">{t("coreMove.rescan", values)}</p> : null}
        </DialogBody>
        {/* While it moves the dialog only reports; its header's × closes it. */}
        {form === "moving" ? null : (
        <DialogFooter>
          {form === "checking" ? (
            <Button variant="secondary" onClick={onClose}>{t("common.close")}</Button>
          ) : null}
          {form === "checks_failed" ? (
            <>
              <Button variant="secondary" onClick={onClose}>{t("common.close")}</Button>
              <Button data-core-move-recheck="true" onClick={() => ask(request, false)}>{t("coreMove.recheck")}</Button>
            </>
          ) : null}
          {form === "confirm" ? (
            <>
              <Button variant="secondary" onClick={onClose}>{t("common.cancel")}</Button>
              <Button data-core-move-start="true" onClick={() => ask(request, true)}>
                {back ? t("coreMove.back.start", values) : euro ? t("coreMove.startEuro", values) : t("coreMove.start", values)}
              </Button>
            </>
          ) : null}
          {form === "failed" ? (
            <>
              <Button variant="secondary" onClick={onClose}>{t("common.close")}</Button>
              <Button data-core-move-retry="true" onClick={() => ask(request, true)}>{t("common.retry")}</Button>
            </>
          ) : null}
          {form === "done" ? (
            <>
              {back ? null : (
                <Button
                  variant="secondary"
                  data-core-move-undo="true"
                  onClick={() => {
                    const next: MoveRequest = { direction: "back" };
                    onRequest(next);
                    ask(next, false);
                  }}
                >
                  {t("coreMove.undo")}
                </Button>
              )}
              <Button onClick={onClose}>{t("common.close")}</Button>
            </>
          ) : null}
        </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** One check that did not pass: its name, and the one command that fixes it where there is one; what the check found is its hint. */
function FailedCheck({ check, detail, machine, actions }: { check: MoveView["failed"][number]["check"]; detail: string; machine: string; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const command = checkCommand(check);
  return (
    <li className="flex min-w-0 items-center justify-between gap-md" data-core-move-check={check} title={detail}>
      <span className="min-w-0 text-body text-destructive">
        <span aria-hidden="true">× </span>
        {t(`coreMove.check.${check}`, { machine })}
      </span>
      {command ? (
        <span className="flex shrink-0 items-center gap-xxs rounded-sm bg-muted py-xxs pl-sm pr-xxs" data-core-move-command={check}>
          <code className="font-mono text-body text-foreground">{command}</code>
          <Button variant="ghost" size="icon" aria-label={t("coreMove.copy")} onClick={() => actions.copyText(command, "core move command")}>
            <CopyIcon aria-hidden="true" />
          </Button>
        </span>
      ) : (
        <span className="min-w-0 text-right text-caption text-muted-foreground">{t(`coreMove.fix.${check}`, { machine })}</span>
      )}
    </li>
  );
}
