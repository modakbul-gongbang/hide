import { useShellStore } from "../store";
import { useFollowInterfaceLanguage } from "./translator";

export { translate, useInterfaceTranslation } from "./translator";

/** Follows confirmed snapshots, including reconnects and another client's edit. */
export function InterfaceLanguageBoundary() {
  useFollowInterfaceLanguage(useShellStore((state) => state.rest?.ui_state?.interface_language));
  return null;
}
