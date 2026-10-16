// The caps of a drop from the file manager and the failure line each one
// becomes (docs/UI_BEHAVIOR.md, Explorer). Apart from `explorerDrop.ts` so the
// upload path, which hands refusals back here, imports no cycle.

import { translate } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";

/** Items, bytes per file and bytes per drop: hided's staging and the host's import caps. */
export const DROP_MAX_ITEMS = 500;
export const DROP_MAX_FILE_BYTES = 20 * 1024 * 1024;
export const DROP_MAX_BYTES = 40 * 1024 * 1024;

export type DropFailure = "too_many" | "too_large" | "batch_too_large" | "unreadable";

const MIB = 1024 * 1024;

/** The failure line a drop failure becomes. */
export function dropFailureText(failure: DropFailure | "busy" | "failed"): string {
  const key: Record<string, MessageKey> = {
    too_many: "explorer.dropTooMany",
    too_large: "explorer.dropTooLarge",
    batch_too_large: "explorer.dropBatchTooLarge",
    unreadable: "explorer.dropUnreadable",
    busy: "explorer.dropBusy",
    failed: "explorer.dropFailed",
  };
  const limit = failure === "too_many" ? DROP_MAX_ITEMS : failure === "too_large" ? DROP_MAX_FILE_BYTES / MIB : DROP_MAX_BYTES / MIB;
  return translate(key[failure] as MessageKey, { limit });
}

/** The failure line hided's refusal reason becomes. */
export function dropRefusalText(reason: string): string {
  switch (reason) {
    case "too_large":
      return dropFailureText("too_large");
    case "batch_too_large":
      return dropFailureText("batch_too_large");
    case "too_many_files":
      return dropFailureText("too_many");
    case "staging_full":
      return dropFailureText("busy");
    default:
      return dropFailureText("failed");
  }
}

