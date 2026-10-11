// A screen's one-line introduction (PRD first-run-onboarding D-18, B43): what
// the screen does and its first step, shown the first time the screen opens
// and never again. The core keeps which screens were introduced
// (`ui_state.screen_intros_seen`), so another window or a restart does not
// show it twice; this visit keeps the line it opened with.

import { useEffect, useState } from "react";
import type { Actions } from "./actions";
import { useInterfaceTranslation } from "./i18n/client";
import type { MessageKey } from "./i18n/catalogs";
import type { ScreenIntro as Screen } from "./snapshot";
import { useShellStore } from "./store";

const WORDS: Record<Screen, MessageKey> = {
  factory: "firstRun.intro.factory",
  devices: "firstRun.intro.devices",
};

export function ScreenIntro({ screen, actions }: { screen: Screen; actions: Actions }) {
  const { t } = useInterfaceTranslation();
  // Unknown until the first snapshot says which screens were introduced.
  const seen = useShellStore((s) => (s.rest?.ui_state ? (s.rest.ui_state.screen_intros_seen ?? []).includes(screen) : null));
  const [shown, setShown] = useState<boolean | null>(null);
  useEffect(() => {
    if (shown !== null || seen === null) return;
    setShown(!seen);
    if (!seen) actions.markScreenIntroSeen(screen);
  }, [actions, screen, seen, shown]);
  return shown ? (
    <p className="px-lg pb-sm text-body text-muted-foreground" data-screen-intro={screen}>
      {t(WORDS[screen])}
    </p>
  ) : null;
}
