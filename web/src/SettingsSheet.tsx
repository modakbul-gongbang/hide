// The web Settings sheet (PRD S5 B1-B10, B18-B22): the native sheet's sections
// less Pet, each row reading a value the core or the daemon reported and each
// edit sent as the one core event that owns it. Nothing here decides a value;
// a pending edit shows as pending until the snapshot says it landed. The sheet
// is the shell: each tab is its own module under `settings/`.

import { XIcon } from "lucide-react";
import { useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { MobileTab } from "./MobileTab";
import { SETTINGS_TABS, type SettingsTab } from "./settings";
import { AgentsTab } from "./settings/AgentsTab";
import { AppearanceTab } from "./settings/AppearanceTab";
import { DevicesTab } from "./settings/DevicesTab";
import { GeneralTab } from "./settings/GeneralTab";
import { IssuesTab } from "./settings/IssuesTab";
import { PerformanceTab } from "./settings/PerformanceTab";
import { ShortcutsTab } from "./settings/ShortcutsTab";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

export function SettingsGate({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "settings");
  return open ? <SettingsSheet actions={actions} /> : null;
}

function SettingsSheet({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const close = () => useUiStore.getState().closeOverlay("settings");
  const [tab, setTab] = useState<SettingsTab>(() => useUiStore.getState().settingsTab);
  const subtitle = t(`settings.tabs.${tab}Description`);
  const daemon = useShellStore((s) => s.daemon);
  const selected = useShellStore((s) => {
    const id = s.rest?.navigator?.focused_device_id ?? "local";
    return s.rest?.navigator?.devices?.find((device) => device.id === id) ?? null;
  });
  return (
    <Dialog open onOpenChange={(next) => { if (!next) close(); }}>
      {/* A Shortcuts chip that is recording owns Escape: it cancels the recording, not the sheet (B61). */}
      <DialogContent
        data-settings="true"
        className="w-(--size-settings-sheet-w) h-(--size-settings-sheet-h-max)"
        onEscapeKeyDown={(event) => {
          if (useUiStore.getState().recordingShortcut) event.preventDefault();
        }}
      >
        <header className="flex items-start gap-md border-b border-border px-xl py-lg">
          <div className="min-w-0 flex-1">
            <DialogTitle className="text-headline">{t("common.settings")}</DialogTitle>
            <DialogDescription>{subtitle}</DialogDescription>
            <p className="mt-xxs text-caption text-muted-foreground" data-settings-owner={daemon?.host_name ?? "unknown"}>
              {t("settings.owner", { host: daemon?.host_name ?? t("settings.daemonMachine") })}
              {selected?.kind === "remote" ? ` ${t("settings.ownerRemote", { device: selected.label })}` : ""}
            </p>
          </div>
          <Hint label={t("settings.close")} shortcut="Esc">
            <Button variant="ghost" size="icon-sm" aria-label={t("settings.close")} onClick={close} data-settings-close="true">
              <XIcon />
            </Button>
          </Hint>
        </header>
        {/* Radix Tabs owns the roving tabindex and Left/Right (plus Home/End)
            arrow-key navigation the hand-built tablist used to implement. */}
        <Tabs value={tab} onValueChange={(value) => setTab(value as SettingsTab)} className="min-h-0 flex-1 flex-col gap-none">
          <TabsList aria-label={t("settings.section")} className="w-full flex-wrap justify-start gap-xs rounded-none border-b border-border bg-sidebar px-xl py-sm">
            {SETTINGS_TABS.map((row) => (
              <TabsTrigger key={row} value={row} data-settings-tab={row}>
                {t(`settings.tabs.${row}`)}
              </TabsTrigger>
            ))}
          </TabsList>
          <TabsContent value={tab} className="min-h-0 flex-1 overflow-auto px-xl py-lg">
            {tab === "general" ? <GeneralTab actions={actions} /> : null}
            {tab === "appearance" ? <AppearanceTab actions={actions} /> : null}
            {tab === "agents" ? <AgentsTab actions={actions} /> : null}
            {tab === "issues" ? <IssuesTab actions={actions} /> : null}
            {tab === "devices" ? <DevicesTab actions={actions} /> : null}
            {tab === "mobile" ? <MobileTab actions={actions} /> : null}
            {tab === "performance" ? <PerformanceTab actions={actions} /> : null}
            {tab === "shortcuts" ? <ShortcutsTab actions={actions} /> : null}
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
