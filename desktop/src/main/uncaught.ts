// What the main process does with an exception or a rejection nothing caught:
// it records `host.uncaught` in the host log and keeps running, as Electron's
// default does, without Electron's error dialog. That dialog is a modal the
// operator can do nothing with (design 13), and while it is open a quit cannot
// finish, so a quit that met one waited forever (issue 675).

import { redact } from "../../../web/src/redact";
import type { HostLog } from "./log";

// A message can quote a page's address or the daemon URL, which carries the
// token: an address keeps only its scheme, and a secret-shaped run is redacted.
// A `file:` address is a stack frame's module and stays.
const ADDRESS = /\b(?!file:)([a-z][a-z0-9+.-]*):\/\/[^\s'"()<>]+/gi;

function scrub(text: string): string {
  return redact(text.replace(ADDRESS, "$1://[address]"));
}

/**
 * Listens on `target` (the process) for both. A listener on each replaces
 * Electron's dialog for an exception and Node's turning a rejection into one.
 */
export function recordUncaught(log: HostLog, target: NodeJS.EventEmitter = process): void {
  for (const kind of ["uncaughtException", "unhandledRejection"] as const) {
    target.on(kind, (reason: unknown) => {
      const error = reason instanceof Error ? reason : null;
      log.event("host.uncaught", {
        kind,
        message: scrub(error ? error.message : String(reason)),
        stack: error?.stack ? scrub(error.stack) : null,
      });
    });
  }
}
