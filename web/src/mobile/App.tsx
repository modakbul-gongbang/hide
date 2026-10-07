// The phone app's screens (PRD mobile-companion D-08, B11-B16, B19-B23,
// B30, B38): pairing, the refused and unpaired guidance, and the list; the
// detail is `Detail.tsx`. Every value comes from the store the socket writes.

import { BellIcon, ChevronRightIcon, LaptopIcon, Loader2Icon, PlusIcon, XIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { useElapsed } from "../components/elapsed";
import { useInterfaceTranslation } from "../i18n/translator";
import { statusText } from "../agentStatus";
import { Button } from "../components/ui/button";
import { closeDetail, openDetail, openStartSheet, pairNow } from "./connection";
import { Detail } from "./Detail";
import { AgentHead, Place } from "./parts";
import { StartSheet } from "./StartSheet";
import {
  GROUP_TITLE,
  REFUSAL_NOTICE,
  UNREACHABLE_NOTICE,
  headerLine,
  noticeText,
  notificationRow,
  type PhoneAgent,
} from "./protocol";
import { closeStaleNotifications, enableNotifications, permission, pushSupported } from "./push";
import { patch, usePhone } from "./store";

export function App() {
  const screen = usePhone((s) => s.screen);
  const detail = usePhone((s) => s.detail);
  if (screen === "pair") return <PairScreen />;
  if (screen === "refused") return <Guidance refused />;
  if (screen === "unpaired") return <Guidance refused={false} />;
  return detail ? <Detail onBack={closeDetail} /> : <ListScreen />;
}

function HideIcon() {
  return (
    <img aria-hidden="true" alt="" src="/m/icon-192.png" className="size-(--size-mobile-app-icon) rounded-xl" />
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return <main className="flex min-h-full flex-col items-center justify-center gap-lg px-lg py-xxxl text-center">{children}</main>;
}

/** The page the QR opens (B11). */
function PairScreen() {
  const { t } = useInterfaceTranslation();
  const macName = usePhone((s) => s.macName);
  const [pairing, setPairing] = useState(false);
  return (
    <Centered>
      <HideIcon />
      <h1 className="text-headline font-semibold text-foreground">{t("mobile.pair.title", { name: macName || t("mobile.mac") })}</h1>
      <p className="max-w-(--size-mobile-copy) text-subhead text-muted-foreground">{t("mobile.pair.description")}</p>
      <Button
        className="h-(--size-touch-target) w-full max-w-(--size-mobile-copy) rounded-lg text-title font-semibold"
        disabled={pairing}
        data-phone-pair="true"
        onClick={() => {
          setPairing(true);
          pairNow();
        }}
      >
        {t("mobile.pair.connect")}
      </Button>
      <p className="text-body text-muted-foreground">{t("mobile.pair.expiry")}</p>
    </Centered>
  );
}

/** No credential, or one hided refused for good (B13, B14, B16). */
function Guidance({ refused }: { refused: boolean }) {
  const { t } = useInterfaceTranslation();
  const refusal = usePhone((s) => s.refusal);
  const text = noticeText(t, refused && refusal ? REFUSAL_NOTICE[refusal] : REFUSAL_NOTICE.no_credential);
  return (
    <Centered>
      <HideIcon />
      <p role={refused ? "alert" : undefined} className="max-w-(--size-mobile-copy) text-title text-foreground" data-phone-guidance={refused ? (refusal ?? "revoked") : "unpaired"}>
        {text}
      </p>
      {refused && refusal !== "phone_limit" && refusal !== "no_credential" ? <p className="text-subhead text-muted-foreground">{noticeText(t, REFUSAL_NOTICE.no_credential)}</p> : null}
    </Centered>
  );
}

function standalone(): boolean {
  const iosStandalone = (navigator as Navigator & { standalone?: boolean }).standalone === true;
  return iosStandalone || window.matchMedia("(display-mode: standalone)").matches;
}

function ListScreen() {
  const { t } = useInterfaceTranslation();
  const macName = usePhone((s) => s.macName);
  const otherPhones = usePhone((s) => s.otherPhones);
  const connected = usePhone((s) => s.connected);
  const unreachable = usePhone((s) => s.unreachable);
  const groups = usePhone((s) => s.groups);
  const installHint = usePhone((s) => s.installHint);
  useEffect(() => {
    if (groups && connected) void closeStaleNotifications(groups);
  }, [groups, connected]);
  const dim = unreachable && !connected;
  const agents = groups?.flatMap((group) => group.agents) ?? [];
  return (
    <main className="phone-safe-bottom flex min-h-full flex-col">
      <header className="phone-safe-top px-lg">
        <div className="flex items-center justify-between pt-lg">
          <h1 className="text-display font-semibold text-foreground">hide</h1>
          <div className="flex items-center gap-md">
            <span
              role="img"
              aria-label={connected ? t("mobile.connected") : t("mobile.disconnected")}
              data-phone-connected={connected ? "true" : "false"}
              className={`size-(--size-status-mark) rounded-full ${connected ? "bg-success" : "bg-muted-foreground"}`}
            />
            <button
              type="button"
              aria-label={t("mobile.start.title")}
              data-phone-start="true"
              className="-mr-sm flex size-(--size-touch-target) items-center justify-center rounded-lg text-foreground active:bg-accent"
              onClick={openStartSheet}
            >
              <PlusIcon aria-hidden="true" className="size-(--size-icon-lg)" />
            </button>
          </div>
        </div>
        {macName ? (
          <p className="mt-xs flex items-center gap-xs text-subhead text-muted-foreground" data-phone-header={headerLine(t, macName, otherPhones)}>
            <LaptopIcon aria-hidden="true" className="size-(--size-icon) shrink-0" />
            <span className="min-w-0 truncate">{headerLine(t, macName, otherPhones)}</span>
          </p>
        ) : null}
      </header>
      {dim ? (
        <div role="status" className="mt-md flex items-center gap-md bg-secondary px-lg py-md text-subhead text-subtle-foreground" data-phone-unreachable="true">
          <Loader2Icon aria-hidden="true" className="size-(--size-icon-lg) shrink-0 animate-spin" />
          <span>{noticeText(t, UNREACHABLE_NOTICE)}</span>
        </div>
      ) : null}
      {installHint && !standalone() ? <InstallHint /> : null}
      {connected ? <NotificationRow /> : null}
      <div className={dim ? "opacity-50" : undefined} aria-busy={dim}>
        {groups && agents.length === 0 ? (
          <p className="px-lg py-xxl text-center text-title text-muted-foreground" data-phone-empty="true">
            {t("mobile.noAgents")}
          </p>
        ) : null}
        {groups?.map((group) => {
          const id = group.group;
          if (group.count === 0) return null;
          return (
            <section key={id} className="px-lg pt-lg" data-phone-group={id}>
              <h2 className="flex gap-sm pb-xs text-subhead text-muted-foreground">
                <span>{t(GROUP_TITLE[id])}</span>
                <span className="font-mono" data-phone-group-count={group.count}>
                  {group.count}
                </span>
              </h2>
              <ul className="divide-y divide-border border-b border-border">
                {group.agents.map((agent) => (
                  <AgentRow key={`${agent.device_id}|${agent.pane_id}`} agent={agent} />
                ))}
              </ul>
            </section>
          );
        })}
      </div>
      <StartSheet />
    </main>
  );
}

const LINE_TONE = { error: "text-destructive", warning: "text-warning", news: "text-foreground" } as const;

function AgentRow({ agent }: { agent: PhoneAgent }) {
  const { t } = useInterfaceTranslation();
  const elapsed = useElapsed(agent.changed_at_unix_ms);
  const label = [statusText(t, agent.status_code), agent.title, agent.place, agent.device_label, elapsed].filter(Boolean).join(", ");
  return (
    <li>
      <button
        type="button"
        aria-label={label}
        data-phone-agent={`${agent.device_id}|${agent.pane_id}`}
        className="flex min-h-(--size-touch-target) w-full flex-col gap-xs py-md text-left outline-none focus-visible:bg-accent active:bg-accent"
        onClick={() => openDetail({ device_id: agent.device_id, pane_id: agent.pane_id })}
      >
        <span className="flex w-full items-center gap-sm">
          <span className="min-w-0 flex-1">
            <AgentHead agent={agent} large={false} />
          </span>
          <span className="shrink-0 font-mono text-body text-muted-foreground">{elapsed}</span>
          <ChevronRightIcon aria-hidden="true" className="size-(--size-icon) shrink-0 text-muted-foreground" />
        </span>
        <Place agent={agent} />
        {agent.line ? (
          <span className={`line-clamp-2 pl-(--size-mobile-row-inset) text-body ${LINE_TONE[agent.line.tone]}`} data-phone-line={agent.line.tone}>
            {agent.line.text}
          </span>
        ) : null}
      </button>
    </li>
  );
}

/** Shown once after pairing, in a browser tab: keep hide on the Home Screen (B11). */
function InstallHint() {
  const { t } = useInterfaceTranslation();
  return (
    <div className="mx-lg mt-md flex items-start gap-md rounded-lg border border-border bg-card px-md py-sm text-subhead text-card-foreground" data-phone-install-hint="true">
      <span className="min-w-0 flex-1">{t("mobile.installHint")}</span>
      <button type="button" aria-label={t("common.close")} className="flex size-(--size-touch-target) shrink-0 items-center justify-center text-muted-foreground" onClick={() => patch({ installHint: false })}>
        <XIcon aria-hidden="true" className="size-(--size-icon)" />
      </button>
    </div>
  );
}

/** Turn on notifications, the Home Screen first, or the OS setting that is off (B30). */
function NotificationRow() {
  const { t } = useInterfaceTranslation();
  const pushMode = usePhone((s) => s.pushMode);
  const notifications = usePhone((s) => s.notifications);
  const [asking, setAsking] = useState(false);
  const row = notificationRow({ pushMode, notifications, permission: permission(), supported: pushSupported() });
  if (!row) return null;
  if (row === "enable") {
    return (
      <button
        type="button"
        disabled={asking}
        data-phone-notifications="enable"
        className="mx-lg mt-md flex min-h-(--size-touch-target) items-center gap-md rounded-lg border border-border bg-card px-md text-left text-title text-card-foreground active:bg-accent"
        onClick={() => {
          setAsking(true);
          void enableNotifications().finally(() => setAsking(false));
        }}
      >
        <BellIcon aria-hidden="true" className="size-(--size-icon-lg) shrink-0 text-primary" />
        <span className="flex-1">{t("mobile.notifications.enable")}</span>
        <ChevronRightIcon aria-hidden="true" className="size-(--size-icon) text-muted-foreground" />
      </button>
    );
  }
  return (
    <p className="mx-lg mt-md flex items-start gap-md rounded-lg border border-border bg-card px-md py-sm text-subhead text-muted-foreground" data-phone-notifications={row}>
      <BellIcon aria-hidden="true" className="mt-xxs size-(--size-icon) shrink-0" />
      <span>{row === "denied" ? t("mobile.notifications.denied") : t("mobile.notifications.installFirst")}</span>
    </p>
  );
}
