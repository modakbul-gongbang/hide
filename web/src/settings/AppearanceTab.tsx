import { useEffect, useState } from "react";
import type { Actions } from "../actions";
import { RadioGroup, RadioGroupItem } from "../components/ui/radio-group";
import { Slider } from "../components/ui/slider";
import { ToggleGroup, ToggleGroupItem } from "../components/ui/toggle-group";
import { Hint } from "../components/ui/tooltip";
import { Group, Note, Row, Value } from "../components/settings-rows";
import { ACCENT_STORED_HEX } from "../generated/accents";
import { useInterfaceTranslation } from "../i18n/client";
import { ACCENT_CHOICES, FONT_SIZE_BASE, FONT_SIZE_MAX, FONT_SIZE_MIN, usableAccent, usableFontSize } from "../settings";
import { commandLabel } from "../shortcutLabels";
import { useShellStore } from "../store";
import { THEME_CHOICES, accentNameOf, readTheme, type AccentName, type ThemeChoice } from "../theme";
import { useErrorSince } from "./useErrorSince";

export function AppearanceTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const accent = useShellStore((s) => usableAccent(s.rest?.ui_state?.accent_hex));
  const theme = useShellStore((s) => readTheme(s.rest?.ui_state?.theme).choice);
  const fontSize = useShellStore((s) => usableFontSize(s.rest?.ui_state?.font_size)) ?? FONT_SIZE_BASE;
  const textLarger = useShellStore((s) => commandLabel("text_larger", s.rest?.ui_state));
  const textSmaller = useShellStore((s) => commandLabel("text_smaller", s.rest?.ui_state));
  const [changedAt, setChangedAt] = useState<number | null>(null);
  const error = useErrorSince(changedAt, ["ui_state."]);
  const [draftSize, setDraftSize] = useState(fontSize);
  useEffect(() => setDraftSize(fontSize), [fontSize]);
  const chosenAccent = accentNameOf(accent);
  const commitSize = (size: number) => {
    if (size === fontSize) return;
    setChangedAt(Date.now());
    actions.setFontSize(size);
  };
  return (
    <>
      <Group title={t("settings.theme")} note={t("settings.themeDescription")}>
        <Row label={t("settings.tabs.appearance")}>
          <ToggleGroup
            type="single"
            value={theme}
            aria-label={t("settings.theme")}
            data-theme-choice={theme}
            onValueChange={(value) => {
              // A second press on the chosen item would clear it; a theme is always chosen.
              if (!value || value === theme) return;
              setChangedAt(Date.now());
              actions.setTheme(value as ThemeChoice);
            }}
          >
            {THEME_CHOICES.map((choice) => (
              <ToggleGroupItem key={choice.id} value={choice.id} aria-label={t(`settings.theme.${choice.id}`)} data-theme-option={choice.id}>
                {t(`settings.theme.${choice.id}`)}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Row>
        <Row label={t("settings.accent")} detail={error ? <Note tone="error" data-appearance-error="true">{t("settings.notSaved", { reason: error })}</Note> : null}>
          <RadioGroup
            aria-label={t("settings.accent")}
            value={chosenAccent ?? ""}
            className="flex items-center gap-sm"
            onValueChange={(name) => {
              const hex = ACCENT_STORED_HEX[name as AccentName];
              setChangedAt(Date.now());
              actions.setAccent(hex.toUpperCase());
            }}
          >
            {ACCENT_CHOICES.map((choice) => (
              <Hint key={choice.id} label={t("settings.accentName", { name: t(`settings.accent.${choice.id}`) })}>
                <RadioGroupItem
                  value={choice.id}
                  data-accent={choice.id}
                  className={`border-0 ${choice.swatch} ring-1 ring-border data-[state=checked]:ring-2 data-[state=checked]:ring-foreground [&_svg]:hidden`}
                />
              </Hint>
            ))}
          </RadioGroup>
          <Value>{accent ? accent.toUpperCase() : t("settings.defaultChoice")}</Value>
        </Row>
      </Group>
      <Group
        title={t("settings.density")}
        note={t("settings.densityDescription", { larger: textLarger, smaller: textSmaller })}
      >
        <Row label={t("settings.interfaceFont")}>
          <Slider
            min={FONT_SIZE_MIN}
            max={FONT_SIZE_MAX}
            step={1}
            value={[draftSize]}
            aria-label={t("settings.interfaceFontSize")}
            data-font-size="true"
            className="w-(--size-settings-control-w)"
            onValueChange={([size]) => size !== undefined && setDraftSize(size)}
            onValueCommit={([size]) => size !== undefined && commitSize(size)}
          />
          <Value>{t("settings.fontPoints", { size: draftSize })}</Value>
        </Row>
      </Group>
    </>
  );
}
