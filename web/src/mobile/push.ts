// Notifications on the phone (PRD mobile-companion D-19, D-20, D-21, B30,
// B34): permission is asked only when the user taps 알림 켜기, the
// subscription is registered over /ws and never over another path, a phone
// that turned notifications back on in the OS subscribes again when the app
// opens, and a revoked phone drops its local subscription.

import { staleTags, type AgentGroup } from "./protocol";
import { patch, usePhone } from "./store";
import { send } from "./connection";

export function pushSupported(): boolean {
  return "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;
}

export function permission(): NotificationPermission | "unsupported" {
  return "Notification" in window ? Notification.permission : "unsupported";
}

function applicationServerKey(base64url: string): Uint8Array<ArrayBuffer> {
  const base64 = base64url.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

async function registration(): Promise<ServiceWorkerRegistration | null> {
  if (!("serviceWorker" in navigator)) return null;
  try {
    return await navigator.serviceWorker.ready;
  } catch {
    return null;
  }
}

async function subscribe(): Promise<boolean> {
  const key = usePhone.getState().vapidKey;
  const worker = await registration();
  if (!key || !worker) return false;
  try {
    const subscription =
      (await worker.pushManager.getSubscription()) ??
      (await worker.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: applicationServerKey(key) }));
    const json = subscription.toJSON();
    if (!json.endpoint || !json.keys?.p256dh || !json.keys.auth) return false;
    return send({ type: "push_subscription", subscription: { endpoint: json.endpoint, keys: { p256dh: json.keys.p256dh, auth: json.keys.auth } } });
  } catch {
    return false;
  }
}

/** The user tapped 알림 켜기 (B30). */
export async function enableNotifications(): Promise<void> {
  if (!pushSupported()) return;
  const answer = await Notification.requestPermission();
  if (answer === "granted") {
    if (!(await subscribe())) patch({ notifications: "unasked" });
    return;
  }
  send({ type: "push_permission", state: answer === "denied" ? "denied" : "default" });
  patch({ notifications: answer === "denied" ? "off" : "unasked" });
}

/**
 * After every hello: re-subscribe when the OS allows notifications but hided
 * holds no subscription (the OS setting came back on, or the push service
 * dropped the old one), and tell hided when the OS setting went off (B30, B35).
 */
export async function syncPush(): Promise<void> {
  const state = usePhone.getState();
  if (!pushSupported()) return;
  const current = permission();
  if (current === "granted" && state.notifications !== "on" && state.pushMode !== "off") {
    await subscribe();
  } else if (current === "denied" && state.notifications !== "off") {
    send({ type: "push_permission", state: "denied" });
    patch({ notifications: "off" });
  }
}

/** A revoked phone keeps no subscription of its own (D-19). */
export async function forgetPush(): Promise<void> {
  const worker = await registration();
  try {
    await (await worker?.pushManager.getSubscription())?.unsubscribe();
  } catch {
    // Nothing is sent to a revoked phone anyway; hided dropped its copy in the same write.
  }
}

/** Closes the notifications of agents no longer waiting or done, once the list is known (D-21). */
export async function closeStaleNotifications(groups: AgentGroup[]): Promise<void> {
  const worker = await registration();
  if (!worker) return;
  try {
    const shown = await worker.getNotifications();
    const stale = new Set(staleTags(shown.map((notification) => notification.tag), groups));
    for (const notification of shown) if (stale.has(notification.tag)) notification.close();
  } catch {
    // getNotifications is missing on some engines; the next push closes them instead.
  }
}
