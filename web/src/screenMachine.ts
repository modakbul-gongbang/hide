import { localDeviceId, type SnapshotRest } from "./snapshot";

/** The longest node id a hash may name. */
const NODE_MAX = 128;

/**
 * The machine named by a page's hash as `node`: `hide connect` adds it when
 * this machine's hided runs in the node role, its core on another machine
 * (docs/ARCHITECTURE.md, A core on another machine). A value that cannot be
 * a node id is not one.
 */
export function screenNodeFromHash(hash: string): string | null {
  const node = new URLSearchParams(hash.replace(/^#/, "")).get("node");
  return node && node.length <= NODE_MAX && /^[A-Za-z0-9._-]+$/.test(node) ? node : null;
}

/**
 * The device this screen runs on: what only this machine can do (reveal or
 * open with an app, a terminal path, a page on its loopback) follows its
 * panes and checkouts. Without a node in the hash it is the core's own.
 */
export function screenDeviceId(rest: SnapshotRest | null): string {
  const hash = typeof window === "undefined" ? undefined : window.location?.hash;
  return (hash ? screenNodeFromHash(hash) : null) ?? localDeviceId(rest);
}
