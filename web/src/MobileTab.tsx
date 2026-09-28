// Settings > Mobile (PRD mobile-companion B1-B7, B10, B14, B15, B29, B39):
// the switch, the four-step Tailscale checklist with the first failing
// step's action, the QR once hided has confirmed its serve entry, the paired
// phones with Revoke, and the push mode. Every value is the daemon's `mobile`
// frame; every control sends one `mobile_*` event.

import { AlertTriangleIcon, CheckIcon, CircleAlertIcon, ExternalLinkIcon } from "lucide-react";
import qrcode from "qrcode-generator";
import { useEffect, useMemo, useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { RadioGroup, RadioGroupItem } from "./components/ui/radio-group";
import { Switch } from "./components/ui/switch";
import { Group, Note, Row } from "./components/settings-rows";
import {
  PUSH_CHOICES,
  SWITCH_DETAIL,
  SWITCH_LABEL,
  checklistRows,
  codeCountdown,
  exposureLine,
  phoneLine,
  phonesTitle,
  type ChecklistRow,
  type MobileState,
  type PushMode,
} from "./mobileSettings";
import { useShellStore } from "./store";

export function MobileTab({ actions }: { actions: Actions }) {
  const mobile = useShellStore((s) => s.mobile);
  const live = useShellStore((s) => s.connection === "live");
  // hided rechecks the checklist while this tab is open and shows a new code
  // each time it opens (B3, B10). A restarted daemon has forgotten the tab,
  // so every live connection says so again.
  useEffect(() => {
    if (!live) return;
    actions.observeMobile(true);
    return () => actions.observeMobile(false);
  }, [actions, live]);
  if (!mobile) return <Note tone="pending">hide에 연결하는 중…</Note>;
  return (
    <div data-mobile-tab="true" data-mobile-exposure={mobile.exposure}>
      <Group title="모바일">
        <Row
          label={
            <span className="flex min-w-0 flex-col gap-xxs">
              <span>{SWITCH_LABEL}</span>
              <span className="text-body text-muted-foreground">{SWITCH_DETAIL}</span>
            </span>
          }
        >
          <Switch checked={mobile.enabled} onCheckedChange={(next) => actions.setMobileEnabled(next)} aria-label={SWITCH_LABEL} data-mobile-switch="true" />
        </Row>
        {mobile.enabled ? checklistRows(mobile).map((row) => <ChecklistItem key={row.id} row={row} />) : null}
        {/* A removal that failed at switch-off stays on screen until it is finished (B7). */}
        {mobile.enabled || mobile.exposure === "failed" ? <Pairing state={mobile} actions={actions} /> : null}
      </Group>
      {mobile.phones.length > 0 ? <Phones state={mobile} actions={actions} /> : null}
      <Group title="푸시 알림">
        <RadioGroup value={mobile.push_mode} onValueChange={(value) => actions.setPushMode(value as PushMode)} aria-label="푸시 알림" className="gap-none divide-y divide-border">
          {PUSH_CHOICES.map((choice) => (
            <label key={choice.id} className="flex cursor-pointer items-start gap-md px-md py-sm" data-push-choice={choice.id}>
              <RadioGroupItem value={choice.id} aria-label={choice.label} className="mt-xxs" />
              <span className="flex min-w-0 flex-col gap-xxs">
                <span className="text-subhead text-foreground">{choice.label}</span>
                {choice.detail ? <span className="text-body text-muted-foreground">{choice.detail}</span> : null}
              </span>
            </label>
          ))}
        </RadioGroup>
      </Group>
    </div>
  );
}

function StepMark({ state }: { state: ChecklistRow["state"] }) {
  if (state === "ok") return <CheckIcon aria-hidden="true" className="size-(--size-icon-lg) shrink-0 text-success" />;
  if (state === "failed") return <AlertTriangleIcon aria-hidden="true" className="size-(--size-icon-lg) shrink-0 text-warning" />;
  return <CircleAlertIcon aria-hidden="true" className="size-(--size-icon-lg) shrink-0 text-muted-foreground" />;
}

const STEP_WORD: Record<ChecklistRow["state"], string> = { ok: "통과", failed: "조치 필요", waiting: "대기" };

function ChecklistItem({ row }: { row: ChecklistRow }) {
  return (
    <div className="flex items-start gap-md px-md py-sm" data-mobile-step={row.id} data-step-state={row.state}>
      <StepMark state={row.state} />
      <span className="sr-only">{STEP_WORD[row.state]}: </span>
      <div className="flex min-w-0 flex-1 flex-col gap-xxs">
        <span className={`text-subhead ${row.state === "waiting" ? "text-muted-foreground" : "text-foreground"}`}>{row.title}</span>
        {row.action ? <span className="text-body text-muted-foreground">{row.action}</span> : null}
      </div>
      {row.link ? (
        <a href={row.link.href} target="_blank" rel="noopener noreferrer" className="inline-flex shrink-0 items-center gap-xs text-subhead text-primary outline-none focus-visible:ring-1 focus-visible:ring-ring" data-mobile-step-link={row.id}>
          {row.link.label}
          <ExternalLinkIcon aria-hidden="true" className="size-(--size-icon)" />
        </a>
      ) : null}
    </div>
  );
}

/** The ms since this frame arrived, ticking every `tickMs` while `live`. */
function useSinceFrame(frame: MobileState, tickMs: number, live: boolean): number {
  const [arrived, setArrived] = useState(() => Date.now());
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const at = Date.now();
    setArrived(at);
    setNow(at);
  }, [frame]);
  useEffect(() => {
    if (!live) return;
    const timer = window.setInterval(() => setNow(Date.now()), tickMs);
    return () => window.clearInterval(timer);
  }, [tickMs, live]);
  return Math.max(0, now - arrived);
}

function QrCode({ text }: { text: string }) {
  const cells = useMemo(() => {
    const code = qrcode(0, "M");
    code.addData(text, "Byte");
    code.make();
    const count = code.getModuleCount();
    const dark: string[] = [];
    for (let row = 0; row < count; row += 1) {
      for (let col = 0; col < count; col += 1) {
        if (code.isDark(row, col)) dark.push(`M${col} ${row}h1v1h-1z`);
      }
    }
    return { count, path: dark.join("") };
  }, [text]);
  const quiet = 2;
  return (
    <svg
      role="img"
      aria-label="페어링 QR 코드"
      viewBox={`${-quiet} ${-quiet} ${cells.count + quiet * 2} ${cells.count + quiet * 2}`}
      className="size-(--size-mobile-qr) shrink-0 rounded-lg bg-qr-background"
      data-mobile-qr={text}
      shapeRendering="crispEdges"
    >
      <path d={cells.path} className="fill-qr-foreground" />
    </svg>
  );
}

function Pairing({ state, actions }: { state: MobileState; actions: Actions }) {
  const since = useSinceFrame(state, 1000, state.code_expires_at_ms !== null);
  const line = exposureLine(state);
  if (!state.qr || line) {
    return (
      <div className="flex items-start gap-md px-md py-sm" data-mobile-pairing="waiting">
        {line?.tone === "error" || line?.tone === "warn" ? (
          <AlertTriangleIcon aria-hidden="true" className={`size-(--size-icon-lg) shrink-0 ${line.tone === "error" ? "text-destructive" : "text-warning"}`} />
        ) : (
          <CircleAlertIcon aria-hidden="true" className="size-(--size-icon-lg) shrink-0 text-muted-foreground" />
        )}
        <span
          role={line?.tone === "error" ? "alert" : undefined}
          className={`min-w-0 break-words text-subhead ${line?.tone === "error" ? "text-destructive" : line?.tone === "warn" ? "text-warning" : "text-muted-foreground"}`}
          data-mobile-exposure-line={state.exposure}
        >
          {line?.text}
        </span>
      </div>
    );
  }
  const countdown = codeCountdown(state.code_expires_at_ms, state.now_ms, since);
  return (
    <div className="flex flex-wrap items-center gap-xl px-md py-lg" data-mobile-pairing="ready">
      <QrCode text={state.qr} />
      <div className="flex min-w-0 flex-1 flex-col gap-sm">
        <span className="text-headline font-semibold text-foreground">폰 카메라로 찍으세요</span>
        <span className="text-subhead text-muted-foreground">열리는 페이지에서 연결을 누르고, 공유 › 홈 화면에 추가로 앱처럼 두세요.</span>
        <span className="break-all font-mono text-subhead text-subtle-foreground" data-mobile-url={state.url ?? ""}>
          {state.url}
        </span>
        <span className="flex items-center gap-md">
          <span className="font-mono text-subhead text-muted-foreground" data-mobile-countdown={countdown ?? "expired"}>
            {countdown ? `코드는 ${countdown} 후 만료` : "코드가 만료됐어요"}
          </span>
          <Button variant="secondary" size="sm" onClick={() => actions.newMobileCode()} data-mobile-new-code="true">
            새 코드
          </Button>
        </span>
      </div>
    </div>
  );
}

function Phones({ state, actions }: { state: MobileState; actions: Actions }) {
  // The daemon's clock decides "방금" and the revoke date; this only adds the time since its frame.
  const daemonNow = state.now_ms + useSinceFrame(state, 30_000, true);
  return (
    <Group title={phonesTitle(state)} data-mobile-phones={String(state.phones.length)}>
      {state.phones.map((phone) => (
        <Row
          key={phone.id}
          label={
            <span className="flex min-w-0 flex-col gap-xxs" data-mobile-phone={phone.id}>
              <span className="break-words">{phone.name}</span>
              <span className="text-body text-muted-foreground" data-mobile-phone-line={phone.id}>
                {phoneLine(phone, daemonNow)}
              </span>
            </span>
          }
        >
          <Button variant="secondary" size="sm" className="text-destructive" onClick={() => actions.revokePhone(phone.id)} data-mobile-revoke={phone.id}>
            해지
          </Button>
        </Row>
      ))}
    </Group>
  );
}
