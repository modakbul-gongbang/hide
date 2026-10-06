import { useEffect, useState, type KeyboardEvent } from "react";
import type { Actions } from "../actions";
import { Button } from "../components/ui/button";
import { Kbd } from "../components/ui/kbd";
import { Group, Note, Row, Status } from "../components/settings-rows";
import { commandTitle } from "../commandTitle";
import { hostKind, keySystem, type HostKind } from "../host";
import { useInterfaceTranslation } from "../i18n/client";
import {
  AREA_COMMANDS,
  EDITABLE_PANE_COMMANDS,
  bindingProblem,
  chordEquals,
  chordFromEvent,
  defaultChord,
  displayChord,
  hostChord,
  resolvedRegistry,
  serializeStoredChord,
  sheetRows,
  storedBindings,
  storedKey,
  type BindingProblem,
  type Chord,
  type CommandId,
  type KeySystem,
} from "../shortcuts";
import { bindingProblemText, sheetRowTitle } from "../shortcutLabels";
import { useShellStore } from "../store";
import { useUiStore } from "../ui";
import { useErrorSince } from "./useErrorSince";

export function ShortcutsTab({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  // Each host edits its own set: the desktop app the macOS set, the browser
  // its own (user decision 2026-09-26).
  const host = hostKind();
  const system = keySystem();
  const stored = useShellStore((s) => storedBindings(s.rest?.ui_state, host));
  const { registry, diagnostic } = resolvedRegistry(stored, host, system);
  const [sentAt, setSentAt] = useState<number | null>(null);
  const [sent, setSent] = useState<Record<string, string> | null>(null);
  const saveError = useErrorSince(sentAt, ["ui_state."]);
  const saving = sent !== null && JSON.stringify(sent) !== JSON.stringify(stored ?? {}) && saveError === null;
  const apply = (bindings: Record<string, string>) => {
    setSent(bindings);
    setSentAt(Date.now());
    actions.setPaneShortcuts(host, bindings);
  };
  const current = (): Record<string, string> => Object.fromEntries(Object.entries(diagnostic && !diagnostic.includes("retired and was ignored") ? {} : stored ?? {}).filter(([key]) => !["project_home", "toggle_sidebar_view"].includes(key)));
  const rowsFor = (ids: readonly CommandId[]) =>
    ids.map((id) => (
      <ShortcutRow
        key={id}
        id={id}
        host={host}
        system={system}
        registry={registry}
        overridden={!diagnostic && stored?.[storedKey(id, host)] !== undefined}
        onApply={(chord) => {
          const next = current();
          const fallback = defaultChord(id, host, system);
          const text = serializeStoredChord(chord, host, system);
          if ((fallback && chordEquals(fallback, chord)) || text === null) delete next[storedKey(id, host)];
          else next[storedKey(id, host)] = text;
          apply(next);
        }}
        onClear={() => {
          const next = current();
          next[storedKey(id, host)] = "none";
          apply(next);
        }}
        onReset={() => {
          const next = current();
          delete next[storedKey(id, host)];
          apply(next);
        }}
      />
    ));
  return (
    <>
      <Group
        title={t("settings.shortcuts.paneAndNavigation")}
        note={t(host === "electron" ? (system === "mac" ? "settings.shortcuts.desktopMac" : "settings.shortcuts.desktopPc") : system === "mac" ? "settings.shortcuts.browserMac" : "settings.shortcuts.browserPc")}
      >
        {rowsFor(EDITABLE_PANE_COMMANDS.filter((id) => !(AREA_COMMANDS as readonly CommandId[]).includes(id)))}
        <Row label={<span className="text-subtle-foreground">{t("settings.shortcuts.toggleConversation")}</span>}>
          <Status tone="muted">{t("settings.shortcuts.conversationUnavailable")}</Status>
        </Row>
      </Group>
      <Group
        title={t("settings.shortcuts.areaTitle")}
        note={t("settings.shortcuts.areaDescription")}
        data-settings-group="area-commands"
      >
        {rowsFor(AREA_COMMANDS)}
      </Group>
      <Group
        title={t("settings.shortcuts.numberedTitle")}
        note={
          host === "electron"
            ? t("settings.shortcuts.numberedDesktop", { modifiers: t(system === "mac" ? "settings.shortcuts.numberedModifiersMac" : "settings.shortcuts.numberedModifiersPc") })
            : t(system === "mac" ? "settings.shortcuts.numberedBrowserMac" : "settings.shortcuts.numberedBrowserPc")
        }
        data-settings-group="numbered-chords"
      >
        {sheetRows("Tabs", registry, host, system)
          .concat(sheetRows("Navigate", registry, host, system))
          .filter((row) => row.id.startsWith("select_"))
          .map((row) => (
            <Row key={row.id} label={sheetRowTitle(row, t)}>
              <Kbd data-shortcut-effective={row.id}>{row.chord ?? "-"}</Kbd>
              {row.chord === null ? <Status tone="muted">{t("settings.shortcuts.notOnHost")}</Status> : null}
            </Row>
          ))}
      </Group>
      {diagnostic ? <Note tone="warn" data-shortcut-diagnostic="true">{diagnostic}</Note> : null}
      {saving ? <Note tone="pending">{t("workspace.saving")}</Note> : null}
      {saveError ? <Note tone="error" data-shortcut-save-error="true">{t("settings.notSaved", { reason: saveError })}</Note> : null}
      <div className="mt-sm flex justify-end">
        <Button variant="secondary" disabled={!stored || Object.keys(stored).length === 0} onClick={() => apply({})} data-shortcut-reset-all="true">
          {t("settings.shortcuts.restoreDefaults")}
        </Button>
      </div>
    </>
  );
}

function ShortcutRow({
  id,
  host,
  system,
  registry,
  overridden,
  onApply,
  onReset,
  onClear,
}: {
  id: CommandId;
  host: HostKind;
  system: KeySystem;
  registry: ReturnType<typeof resolvedRegistry>["registry"];
  overridden: boolean;
  onApply: (chord: Chord) => void;
  onReset: () => void;
  onClear: () => void;
}) {
  const { t } = useInterfaceTranslation();
  const command = registry.find((row) => row.id === id);
  const [recording, setRecording] = useState(false);
  const [draft, setDraft] = useState<Chord | null>(null);
  const [problem, setProblem] = useState<BindingProblem | "altgr" | null>(null);
  const setRecordingFlag = useUiStore((s) => s.setRecordingShortcut);
  useEffect(() => {
    if (!recording) return;
    setRecordingFlag(true);
    return () => setRecordingFlag(false);
  }, [recording, setRecordingFlag]);
  if (!command) return null;
  const title = commandTitle(id, t);
  const effective = hostChord(command, host);
  const record = (event: KeyboardEvent<HTMLButtonElement>) => {
    // IME composition and lone modifiers are not chords; the recorder waits.
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (["Meta", "Alt", "Shift", "Control", "CapsLock"].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape" && !event.metaKey && !event.altKey && !event.ctrlKey) {
      setRecording(false);
      setProblem(null);
      return;
    }
    const chord = chordFromEvent(event.nativeEvent);
    // AltGr types a character on Windows and Linux layouts (Windows reports it
    // as Ctrl+Alt), so a key pressed with it is never a chord, as the window
    // listener already treats it.
    const reason: BindingProblem | "altgr" | null =
      system === "pc" && event.nativeEvent.getModifierState?.("AltGraph") ? "altgr" : bindingProblem(id, chord, registry, host, system);
    setRecording(false);
    setProblem(reason);
    setDraft(reason ? null : chord);
  };
  return (
    <Row
      label={title}
      detail={
        problem ? (
          <Note tone="error" data-shortcut-problem={id}>
            {problem === "altgr" ? t("settings.shortcuts.altGr") : bindingProblemText(problem, t)} {effective ? t("settings.shortcuts.previousChord", { chord: displayChord(effective, system) }) : ""}
          </Note>
        ) : null
      }
    >
      <Kbd data-shortcut-effective={id}>{effective ? displayChord(effective, system) : "-"}</Kbd>
      {draft ? (
        <>
          <span className="text-body text-subtle-foreground">→</span>
          <Kbd className="text-foreground" data-shortcut-draft={id}>
            {displayChord(draft, system)}
          </Kbd>
          <Button
            onClick={() => {
              onApply(draft);
              setDraft(null);
            }}
            data-shortcut-apply={id}
          >
            {t("common.apply")}
          </Button>
          <Button variant="ghost" onClick={() => setDraft(null)}>
            {t("common.cancel")}
          </Button>
        </>
      ) : (
        <Button
          variant={recording ? "default" : "secondary"}
          aria-label={recording ? t("settings.shortcuts.recordAria", { command: title }) : t("settings.shortcuts.changeAria", { command: title })}
          onKeyDown={recording ? record : undefined}
          onBlur={() => setRecording(false)}
          onClick={() => {
            setProblem(null);
            setRecording(true);
          }}
          data-shortcut-record={id}
        >
          {recording ? t("settings.shortcuts.pressChord") : t("settings.shortcuts.change")}
        </Button>
      )}
      {effective && !draft && !recording ? <Button variant="ghost" onClick={onClear} data-shortcut-clear={id}>{t("settings.shortcuts.clear")}</Button> : null}
      {overridden && !draft ? (
        <Button variant="ghost" onClick={onReset} data-shortcut-reset={id}>
          {t("common.default")}
        </Button>
      ) : null}
    </Row>
  );
}
