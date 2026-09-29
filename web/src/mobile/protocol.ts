// The phone's side of `/ws` (PRD mobile-companion D-01, D-04, D-16, D-17,
// D-22): the frames hided sends a phone (hided/src/mobile/phone.rs), the
// ones a phone may send back, how the page address carries a pairing code,
// a credential or a deep link, and the words each refusal and pane state
// shows. Pure, so every rule is tested without a socket or a browser.

export type Tone = "error" | "warning" | "working" | "success" | "subtle";

export type PhoneAgent = {
  device_id: string;
  pane_id: string;
  root_pane_id: string;
  group: GroupId;
  symbol: string;
  tone: Tone;
  agent_kind: string;
  title: string;
  place: string | null;
  device_label: string | null;
  elapsed: string;
  line: { text: string; tone: "error" | "warning" | "news" } | null;
  status_label: string;
  demand: string;
};

export type GroupId = "needs_you" | "done" | "working" | "seen";

export type AgentGroup = { group: GroupId; agents: PhoneAgent[] };

export type Notifications = "on" | "off" | "unasked";

export type PushMode = "off" | "app_closed" | "always";

export type RowsState = "ok" | "gone" | "device_unreachable" | "unavailable";

export type ServerFrame =
  | { type: "paired"; credential: string; phone_id: string; name: string }
  | { type: "hello"; mac_name: string; phone_id: string; name: string | null; vapid_public_key: string; notifications: Notifications }
  | { type: "meta"; push_mode: PushMode; other_phones: number }
  | { type: "agents"; groups: AgentGroup[] }
  | { type: "rows"; device_id: string; pane_id: string; state: RowsState; text?: string; lines?: number; more?: boolean }
  | { type: "input_result"; request_id?: string; ok: boolean; reason: string | null }
  | { type: "push_state"; notifications: "on" | "refused" }
  | { type: "refused"; reason: string }
  | { type: "refused_request"; request: string; reason: string };

/** The only messages a phone sends after its first frame. */
export type PhoneMessage =
  | { type: "open"; device_id: string; pane_id: string }
  | { type: "more" }
  | { type: "close" }
  | ({ type: "input"; request_id: string; device_id: string; pane_id: string } & ({ text: string } | { key: QuickKey }))
  | { type: "push_subscription"; subscription: { endpoint: string; keys: { p256dh: string; auth: string } } }
  | { type: "push_permission"; state: "denied" | "default" };

export type AgentKey = { device_id: string; pane_id: string };

export function sameKey(a: AgentKey | null, b: AgentKey | null): boolean {
  return !!a && !!b && a.device_id === b.device_id && a.pane_id === b.pane_id;
}

export const GROUP_TITLE: Record<GroupId, string> = {
  needs_you: "내 확인 대기",
  done: "끝",
  working: "진행 중",
  seen: "확인함",
};

export const GROUP_ORDER: readonly GroupId[] = ["needs_you", "done", "working", "seen"];

/** The five quick keys (B26, B38): what hided sends Herdr, the face, and the accessible name. */
export type QuickKey = "enter" | "escape" | "up" | "down" | "ctrl_c";

export const QUICK_KEYS: readonly { key: QuickKey; label: string; name: string }[] = [
  { key: "enter", label: "⏎", name: "Enter" },
  { key: "escape", label: "Esc", name: "Escape" },
  { key: "up", label: "↑", name: "위 화살표" },
  { key: "down", label: "↓", name: "아래 화살표" },
  { key: "ctrl_c", label: "^C", name: "Ctrl-C" },
];

/** The longest reply hided writes in one input (hided/src/mobile/pane.rs MAX_REPLY_CHARS). */
export const MAX_REPLY_CHARS = 2000;

/** Why a reply cannot be sent yet, checked before it leaves the phone (B26, B27). */
export function replyProblem(text: string): string | null {
  if (text.trim().length === 0) return "empty";
  if ([...text].length > MAX_REPLY_CHARS) return "too_long";
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(text)) return "control_characters";
  return null;
}

/** The line under the reply bar for a refused or failed input. */
export function inputFailure(reason: string | null): string {
  switch (reason) {
    case "uncertain":
      return "보냈는지 확인하지 못했어요. 터미널을 확인하고 필요하면 내용을 바꿔 다시 보내세요.";
    case "in_flight":
      return "같은 답장을 아직 보내는 중이에요. 잠시 뒤 다시 확인하세요.";
    case "empty":
      return "빈 답장은 보낼 수 없어요.";
    case "too_long":
      return `답장은 한 번에 ${MAX_REPLY_CHARS.toLocaleString("ko-KR")}자까지 보낼 수 있어요.`;
    case "control_characters":
      return "답장에는 한 줄 텍스트만 보낼 수 있어요.";
    case "gone":
      return "이 pane은 더 이상 열려 있지 않아요.";
    case "device_unreachable":
      return "기기가 연결돼 있지 않아 보내지 못했어요.";
    case "offline":
      return "맥의 hide에 닿지 않아 보내지 못했어요. 다시 보내세요.";
    default:
      return "보내지 못했어요. 다시 보내세요.";
  }
}

/** A refused connection's screen (B13, B14, B16). */
export type Refusal = "code_expired" | "phone_limit" | "revoked" | "mobile_off" | "no_credential";

export function refusalOf(reason: string): Refusal {
  switch (reason) {
    case "code_expired":
    case "phone_limit":
    case "revoked":
    case "mobile_off":
      return reason;
    default:
      return "revoked";
  }
}

export const REFUSAL_TEXT: Record<Refusal, string> = {
  code_expired: "코드가 만료됐어요. 맥에서 QR을 다시 여세요.",
  phone_limit: "폰은 4대까지 연결할 수 있어요. 맥의 설정 > Mobile에서 하나를 해지하세요.",
  revoked: "이 폰의 연결이 해지됐어요. 맥에서 QR을 다시 여세요.",
  mobile_off: "연결 안 됨 · 맥의 hide가 꺼져 있거나 폰의 Tailscale이 꺼져 있어요. 다시 시도 중",
  no_credential: "맥의 설정 > Mobile에서 QR을 찍으세요.",
};

export const UNREACHABLE_TEXT = REFUSAL_TEXT.mobile_off;

/** The detail's line for a pane hided could not read (B28). */
export function rowsProblem(state: RowsState): string | null {
  switch (state) {
    case "ok":
      return null;
    case "gone":
      return "이 pane은 더 이상 열려 있지 않아요.";
    case "device_unreachable":
      return "이 에이전트의 기기가 연결돼 있지 않아요.";
    default:
      return "이 pane을 읽지 못했어요. 다시 시도 중";
  }
}

/**
 * A row drawn only with box-drawing characters, such as the rule a TUI draws
 * across the desktop pane: wrapped at a phone's width it becomes a stack of
 * rules, so the detail clips it to one line instead.
 */
export function boxDrawingRow(text: string): boolean {
  return /^[\s\u2500-\u257f]*[\u2500-\u257f][\s\u2500-\u257f]*$/.test(text);
}

/** The QR's payload (D-22): `{v:1, endpoint, code}` as base64url JSON. */
export type PairPayload = { v: 1; endpoint: string; code: string };

function fromBase64Url(text: string): string {
  const base64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
  return new TextDecoder().decode(Uint8Array.from(binary, (char) => char.charCodeAt(0)));
}

export function toBase64Url(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function parsePair(value: string): PairPayload | null {
  try {
    const parsed: unknown = JSON.parse(fromBase64Url(value));
    if (typeof parsed !== "object" || parsed === null) return null;
    const { v, endpoint, code } = parsed as Record<string, unknown>;
    if (v !== 1 || typeof endpoint !== "string" || typeof code !== "string" || code.length === 0) return null;
    return { v: 1, endpoint, code };
  } catch {
    return null;
  }
}

/** What the page address carries after `#` (D-22): a pairing code, a credential, a deep link. */
export type Fragment = { pair: PairPayload | null; credential: string | null; open: AgentKey | null };

export function parseFragment(hash: string): Fragment {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const pair = params.get("pair");
  const credential = params.get("k");
  const open = params.get("open");
  return {
    pair: pair ? parsePair(pair) : null,
    credential: credential && /^[0-9a-f]{64}$/.test(credential) ? credential : null,
    open: open ? openKey(open) : null,
  };
}

/** A deep link names one agent as `device|pane`, the notification tag's form (D-17). */
export function openKey(value: string): AgentKey | null {
  const bar = value.indexOf("|");
  if (bar <= 0 || bar === value.length - 1) return null;
  return { device_id: value.slice(0, bar), pane_id: value.slice(bar + 1) };
}

export function keyTag(key: AgentKey): string {
  return `${key.device_id}|${key.pane_id}`;
}

/**
 * The page address a paired phone keeps, so "Add to Home Screen" carries the
 * credential into the installed app, whose storage iOS may keep apart from
 * Safari's (PRD Risks).
 */
export function credentialHash(credential: string): string {
  return `#k=${credential}`;
}

/** The Mac's name before pairing: the first label of the ts.net address. */
export function macNameOf(endpoint: string): string {
  try {
    return new URL(endpoint).hostname.split(".")[0] || "맥";
  } catch {
    return "맥";
  }
}

/** The list header's second line (B19). */
export function headerLine(macName: string, otherPhones: number): string {
  return otherPhones > 0 ? `${macName} · 폰 ${otherPhones}대 더 연결됨` : macName;
}

/** Tags whose notifications the open app closes (D-21): agents no longer waiting or done. */
/** The demands that raise a push for their root (hided/src/mobile/push.rs). */
const HOLDING_DEMANDS: ReadonlySet<string> = new Set(["question", "approval", "error"]);

export function staleTags(tags: readonly string[], groups: readonly AgentGroup[]): string[] {
  const live = new Set<string>();
  for (const group of groups) {
    for (const agent of group.agents) {
      // A delegated child is only ever Working or Seen, yet its demand is
      // what raised its root's notification (D-21): that keeps it too.
      const holds = group.group === "needs_you" || group.group === "done" || HOLDING_DEMANDS.has(agent.demand);
      if (holds) live.add(keyTag({ device_id: agent.device_id, pane_id: agent.root_pane_id }));
    }
  }
  return tags.filter((tag) => !live.has(tag));
}

/** The notification row on the list (B30), or null when it has nothing to say. */
export type NotificationRow = "enable" | "install_first" | "denied" | null;

export function notificationRow({
  pushMode,
  notifications,
  permission,
  supported,
}: {
  pushMode: PushMode;
  notifications: Notifications;
  permission: NotificationPermission | "unsupported";
  /** Whether this page can subscribe at all: a Safari tab on iOS cannot, a Home Screen app can. */
  supported: boolean;
}): NotificationRow {
  if (pushMode === "off") return null;
  if (permission === "denied" || (notifications === "off" && permission !== "granted")) return "denied";
  if (notifications === "on" && permission === "granted") return null;
  // iOS offers push only to a Home Screen app; a Safari tab has no PushManager.
  if (!supported) return "install_first";
  return "enable";
}
