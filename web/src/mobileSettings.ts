// The pure rules behind Settings > Mobile (PRD mobile-companion B1-B7,
// B10, B14, B15, B29): what each checklist step says and which one carries
// the action, how the code countdown and a phone's row read, and the push
// choices. hided owns every value; this only words what its `mobile` frame
// reported, so each rule is tested without a browser.

export type StepState = "ok" | "failed" | "waiting";

/** The daemon's `mobile` frame (hided/src/mobile/mod.rs `frame_value`). */
export type MobileState = {
  enabled: boolean;
  exposure: "off" | "checking" | "blocked" | "foreign" | "failed" | "exposed";
  checklist: { installed: StepState; logged_in: StepState; https: StepState; host_name: string | null } | null;
  download_url: string;
  admin_url: string;
  foreign_target: string | null;
  failure: { step: string; message: string } | null;
  url: string | null;
  qr: string | null;
  code_expires_at_ms: number | null;
  phones: PhoneRow[];
  max_phones: number;
  push_mode: PushMode;
  now_ms: number;
};

export type PhoneRow = {
  id: string;
  name: string;
  last_seen_ms: number;
  connected: boolean;
  notifications: "on" | "off" | "unasked";
  revoke_at_ms: number;
};

export type PushMode = "off" | "app_closed" | "always";

export const PUSH_CHOICES: readonly { id: PushMode; label: string; detail: string | null }[] = [
  { id: "off", label: "끔", detail: null },
  {
    id: "app_closed",
    label: "앱이 닫혀 있을 때만",
    detail: "데스크톱 hide가 연결돼 있지 않은 동안만 폰으로 보냅니다. 내 확인 대기와 끝 두 전이에서만.",
  },
  { id: "always", label: "항상", detail: null },
];

export const SWITCH_LABEL = "폰에서 hide 열기";
export const SWITCH_DETAIL = "맥의 Tailscale로 이 hide를 tailnet 안에서만 엽니다. hide가 tailscale serve를 켜고, 끄면 자기가 만든 항목만 지웁니다.";

export type ChecklistRow = {
  id: "installed" | "logged_in" | "https" | "phone";
  state: StepState;
  title: string;
  /** What to do, on the first failing step only. */
  action: string | null;
  link: { label: string; href: string } | null;
};

/**
 * The four steps in order (B2): passed steps read as done, the first failing
 * one carries the exact action, and everything after it waits. The phone
 * step is guidance hide cannot check; it reads as done once the QR shows (B4).
 */
export function checklistRows(state: MobileState): ChecklistRow[] {
  const steps = state.checklist ?? { installed: "waiting", logged_in: "waiting", https: "waiting", host_name: null };
  const host = steps.host_name ? ` · ${steps.host_name}` : "";
  const rows: ChecklistRow[] = [
    steps.installed === "failed"
      ? {
          id: "installed",
          state: "failed",
          title: "맥에 Tailscale이 설치돼 있지 않아요",
          action: "Tailscale을 설치하면 여기서 바로 이어집니다.",
          link: { label: "다운로드", href: state.download_url },
        }
      : { id: "installed", state: steps.installed, title: "맥에 Tailscale 설치됨", action: null, link: null },
    steps.logged_in === "failed"
      ? {
          id: "logged_in",
          state: "failed",
          title: "Tailscale에 로그인되지 않았어요",
          action: "Tailscale 앱에서 로그인하세요. 로그인하면 여기서 바로 이어집니다.",
          link: null,
        }
      : { id: "logged_in", state: steps.logged_in, title: `Tailscale에 로그인됨${steps.logged_in === "ok" ? host : ""}`, action: null, link: null },
    steps.https === "failed"
      ? {
          id: "https",
          state: "failed",
          title: "tailnet에 HTTPS가 꺼져 있어요",
          action: "Tailscale 관리 콘솔 › DNS에서 MagicDNS와 HTTPS Certificates를 켜세요. 켜면 여기서 바로 이어집니다.",
          link: { label: "관리 콘솔 열기", href: state.admin_url },
        }
      : { id: "https", state: steps.https, title: "tailnet에 MagicDNS와 HTTPS 켜짐", action: null, link: null },
    {
      id: "phone",
      state: state.qr ? "ok" : "waiting",
      title: "폰에도 Tailscale 앱을 설치하고 같은 계정으로 로그인",
      action: null,
      link: null,
    },
  ];
  return rows;
}

/** The one line under the checklist when there is no QR, or null when the QR shows. */
export function exposureLine(state: MobileState): { tone: "pending" | "warn" | "error"; text: string } | null {
  switch (state.exposure) {
    case "exposed":
      return state.qr ? null : { tone: "pending", text: "새 코드를 만들고 있어요" };
    case "checking":
      return { tone: "pending", text: "확인하는 중…" };
    case "foreign":
      return {
        tone: "warn",
        text: `노출 안 됨 · 이 맥의 tailscale serve HTTPS 443에 hide가 만들지 않은 항목이 있어요: ${state.foreign_target ?? "알 수 없는 대상"}. hide는 그 항목을 바꾸지 않습니다.`,
      };
    case "failed":
      return { tone: "error", text: `노출 안 됨 · ${failureStep(state.failure?.step)}: ${state.failure?.message ?? ""}`.trim() };
    default:
      return { tone: "pending", text: "위 항목이 모두 통과하면 QR이 여기 나타납니다." };
  }
}

function failureStep(step: string | undefined): string {
  switch (step) {
    case "add":
      return "tailscale serve를 켜지 못했어요";
    case "remove":
      return "tailscale serve 항목을 지우지 못했어요";
    case "funnel":
      return "Funnel이 켜져 있어요";
    default:
      return "tailscale serve 상태를 확인하지 못했어요";
  }
}

/** `m:ss` until the code expires, from the daemon's clock; null once expired. */
export function codeCountdown(expiresAtMs: number | null, daemonNowMs: number, elapsedSinceFrameMs: number): string | null {
  if (expiresAtMs === null) return null;
  const left = Math.ceil((expiresAtMs - daemonNowMs - elapsedSinceFrameMs) / 1000);
  if (left <= 0) return null;
  return `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** "방금", "5분 전", "3시간 전", "3일 전". */
export function lastSeen(lastSeenMs: number, nowMs: number, connected: boolean): string {
  if (connected) return "방금";
  const ago = Math.max(0, nowMs - lastSeenMs);
  if (ago < 60_000) return "방금";
  if (ago < 3_600_000) return `${Math.floor(ago / 60_000)}분 전`;
  if (ago < DAY_MS) return `${Math.floor(ago / 3_600_000)}시간 전`;
  return `${Math.floor(ago / DAY_MS)}일 전`;
}

/**
 * A phone row's second line (B15): last seen, the notification state when
 * the phone answered the permission question, and the days until the
 * seven-day revoke once it has been away a day.
 */
export function phoneLine(phone: PhoneRow, nowMs: number): string {
  const parts = [lastSeen(phone.last_seen_ms, nowMs, phone.connected)];
  if (phone.notifications === "on") parts.push("알림 받는 중");
  if (phone.notifications === "off") parts.push("알림 꺼짐");
  const away = nowMs - phone.last_seen_ms;
  if (!phone.connected && away >= DAY_MS) {
    const days = Math.max(1, Math.ceil((phone.revoke_at_ms - nowMs) / DAY_MS));
    parts.push(`${days}일 뒤 자동 해지`);
  }
  return parts.join(" · ");
}

export function phonesTitle(state: MobileState): string {
  return `연결된 폰 · ${state.phones.length} / ${state.max_phones}`;
}
