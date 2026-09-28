// The phone's one socket to hided (PRD mobile-companion D-16): pair with the
// QR's code or authenticate with the stored credential, then keep the list
// live and retry forever after a loss (B23). Nothing here calls any HTTP path
// but `/ws`; the page, its script and the service worker are the static shell.

import {
  credentialHash,
  inputFailure,
  macNameOf,
  parseFragment,
  refusalOf,
  replyProblem,
  type AgentKey,
  type PhoneMessage,
  type QuickKey,
  type ServerFrame,
} from "./protocol";
import { applyFrame, patch, usePhone } from "./store";
import { forgetPush, syncPush } from "./push";

const CREDENTIAL_KEY = "hide.phone.credential";
const RETRY_MS = [1000, 2000, 4000, 8000, 10000];

function storedCredential(): string | null {
  try {
    return window.localStorage.getItem(CREDENTIAL_KEY);
  } catch {
    return null;
  }
}

function storeCredential(credential: string | null): void {
  try {
    if (credential) window.localStorage.setItem(CREDENTIAL_KEY, credential);
    else window.localStorage.removeItem(CREDENTIAL_KEY);
  } catch {
    // A private tab refuses storage; the address still carries the credential.
  }
  const hash = credential ? credentialHash(credential) : "";
  window.history.replaceState(null, "", `${window.location.pathname}${hash}`);
}

let socket: WebSocket | null = null;
let credential: string | null = null;
let pairCode: string | null = null;
let attempt = 0;
let retryTimer: number | null = null;
/** Set when hided refused the phone for good: no retry until the page reloads. */
let stopped = false;

function socketUrl(): string {
  const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${window.location.host}/ws`;
}

export function send(message: PhoneMessage): boolean {
  if (!socket || socket.readyState !== WebSocket.OPEN || !usePhone.getState().connected) return false;
  socket.send(JSON.stringify(message));
  return true;
}

function scheduleRetry(): void {
  if (stopped || retryTimer !== null) return;
  const delay = RETRY_MS[Math.min(attempt, RETRY_MS.length - 1)];
  attempt += 1;
  retryTimer = window.setTimeout(() => {
    retryTimer = null;
    connect();
  }, delay);
}

/** Retries at once when the phone comes back (network, foreground). */
function retryNow(): void {
  if (stopped || usePhone.getState().connected) return;
  if (retryTimer !== null) {
    window.clearTimeout(retryTimer);
    retryTimer = null;
  }
  attempt = 0;
  connect();
}

function connect(): void {
  if (stopped) return;
  if (socket && socket.readyState <= WebSocket.OPEN) return;
  // Nothing to present: the unpaired screen stays, no socket is opened.
  if (!pairCode && !credential) return;
  const first = pairCode ? { client_kind: "phone", pair: pairCode } : { client_kind: "phone", token: credential };
  let refused = false;
  const ws = new WebSocket(socketUrl());
  socket = ws;
  ws.onopen = () => ws.send(JSON.stringify(first));
  ws.onmessage = (event) => {
    let frame: ServerFrame;
    try {
      frame = JSON.parse(String(event.data)) as ServerFrame;
    } catch {
      return;
    }
    if (frame.type === "refused") {
      refused = true;
      onRefused(frame.reason);
      return;
    }
    if (frame.type === "paired") {
      pairCode = null;
      credential = frame.credential;
      storeCredential(frame.credential);
    }
    if (frame.type === "hello") {
      attempt = 0;
      applyFrame(frame);
      reopenDetail();
      return;
    }
    if (frame.type === "input_result") {
      onInputResult(frame.ok, frame.reason);
      return;
    }
    applyFrame(frame);
    if (frame.type === "agents") openPending();
    // hided sends the push mode right after hello; the subscription follows it.
    if (frame.type === "meta") void syncPush();
  };
  ws.onclose = () => {
    if (socket === ws) socket = null;
    const state = usePhone.getState();
    if (state.pendingInput) patch({ pendingInput: null, inputError: inputFailure("offline") });
    patch({ connected: false, unreachable: !refused || state.refusal === "mobile_off" });
    if (!refused || state.refusal === "mobile_off") scheduleRetry();
  };
}

function onRefused(reason: string): void {
  const refusal = refusalOf(reason);
  if (refusal === "mobile_off") {
    // Mobile was switched off on the Mac: the phone stays paired and keeps retrying (B7).
    patch({ refusal: "mobile_off", connected: false, unreachable: true });
    return;
  }
  stopped = true;
  if (refusal === "revoked") {
    credential = null;
    storeCredential(null);
    void forgetPush();
  }
  patch({ screen: "refused", refusal, connected: false, unreachable: false, pair: null });
}

function openPending(): void {
  const pending = usePhone.getState().pendingOpen;
  if (!pending) return;
  patch({ pendingOpen: null });
  openDetail(pending);
}

function reopenDetail(): void {
  const detail = usePhone.getState().detail;
  if (detail) send({ type: "open", device_id: detail.key.device_id, pane_id: detail.key.pane_id });
}

export function openDetail(key: AgentKey): void {
  const state = usePhone.getState();
  if (!state.groups) {
    patch({ pendingOpen: key });
    return;
  }
  patch({ detail: { key, rows: null, moreAskedAt: null }, inputError: null, pendingInput: null });
  send({ type: "open", device_id: key.device_id, pane_id: key.pane_id });
}

export function closeDetail(): void {
  patch({ detail: null, inputError: null, pendingInput: null });
  send({ type: "close" });
}

/** Asks for the next page of older rows once per page (B25). */
export function loadMore(): void {
  const detail = usePhone.getState().detail;
  if (!detail?.rows?.more || detail.moreAskedAt === detail.rows.lines) return;
  patch({ detail: { ...detail, moreAskedAt: detail.rows.lines } });
  send({ type: "more" });
}

let requestCounter = 0;
/** A reply that failed keeps its request id, so a resend of the same text lands once (B27). */
let lastReply: { text: string; requestId: string } | null = null;

function newRequestId(): string {
  requestCounter += 1;
  const random = new Uint8Array(8);
  crypto.getRandomValues(random);
  return `${Date.now().toString(36)}-${requestCounter}-${Array.from(random, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

/** Sends a reply with Enter after it; the caller clears its field only on success (B26, B27). */
export function sendReply(text: string): boolean {
  const state = usePhone.getState();
  const detail = state.detail;
  if (!detail || state.pendingInput) return false;
  const problem = replyProblem(text);
  if (problem) {
    patch({ inputError: inputFailure(problem) });
    return false;
  }
  const requestId = lastReply?.text === text ? lastReply.requestId : newRequestId();
  lastReply = { text, requestId };
  const sent = send({ type: "input", request_id: requestId, device_id: detail.key.device_id, pane_id: detail.key.pane_id, text });
  patch(sent ? { pendingInput: { requestId, kind: "text" }, inputError: null } : { inputError: inputFailure("offline") });
  return sent;
}

export function sendKey(key: QuickKey): void {
  const state = usePhone.getState();
  const detail = state.detail;
  if (!detail || state.pendingInput) return;
  const requestId = newRequestId();
  const sent = send({ type: "input", request_id: requestId, device_id: detail.key.device_id, pane_id: detail.key.pane_id, key });
  patch(sent ? { pendingInput: { requestId, kind: "key" }, inputError: null } : { inputError: inputFailure("offline") });
}

type ReplyListener = (ok: boolean) => void;
const replyListeners = new Set<ReplyListener>();

/** The reply bar clears its field when its reply landed. */
export function onReplyResult(listener: ReplyListener): () => void {
  replyListeners.add(listener);
  return () => replyListeners.delete(listener);
}

function onInputResult(ok: boolean, reason: string | null): void {
  const pending = usePhone.getState().pendingInput;
  patch({ pendingInput: null, inputError: ok ? null : inputFailure(reason) });
  if (pending?.kind === "text") {
    if (ok) lastReply = null;
    for (const listener of replyListeners) listener(ok);
  }
}

/** The user tapped 연결 on the pairing screen (B11). */
export function pairNow(): void {
  const pair = usePhone.getState().pair;
  if (!pair) return;
  pairCode = pair.code;
  stopped = false;
  connect();
}

/** Reads the page address and starts: pair, reconnect, or show how to pair (B11-B13). */
export function start(): void {
  const fragment = parseFragment(window.location.hash);
  window.addEventListener("online", retryNow);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") retryNow();
  });
  if (fragment.open) patch({ pendingOpen: fragment.open });
  if (fragment.pair) {
    // The code stays out of storage and out of the address once read.
    window.history.replaceState(null, "", window.location.pathname);
    patch({ screen: "pair", pair: fragment.pair, macName: macNameOf(fragment.pair.endpoint) });
    return;
  }
  credential = fragment.credential ?? storedCredential();
  if (!credential) {
    patch({ screen: "unpaired" });
    return;
  }
  storeCredential(credential);
  patch({ screen: "app", unreachable: false });
  // Opened without a network, the list is not blank: the unreachable line shows at once (B23).
  if (!navigator.onLine) patch({ unreachable: true });
  connect();
}
