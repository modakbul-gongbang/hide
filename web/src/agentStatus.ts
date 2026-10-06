import type { TFunction } from "i18next";
import type { AgentStatusCode } from "./snapshot";

/** The word for a status code in the operator's language. */
export function statusText(t: TFunction<"translation">, code: AgentStatusCode): string {
  return t(`agents.status.${code}`);
}
