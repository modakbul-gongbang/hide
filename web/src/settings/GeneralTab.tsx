import type { Actions } from "../actions";
import { AppearanceGroup } from "./AppearanceGroup";

/** Language and look, and nothing else (PRD settings-left-nav B5). */
export function GeneralTab({ actions }: { actions: Actions }) {
  return <AppearanceGroup actions={actions} />;
}
