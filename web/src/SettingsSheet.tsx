// The web Settings sheet (PRD S5 B1-B10, B18-B22; settings-left-nav B1-B4):
// a list of pages on the left and the chosen page on the right, each row
// reading a value the core or the daemon reported and each edit sent as the
// one core event that owns it. Nothing here decides a value; a pending edit
// shows as pending until the snapshot says it landed. The sheet is the shell:
// each page is its own module under `settings/`.

import { BellIcon, BotIcon, createLucideIcon, InfoIcon, KeyboardIcon, MonitorSmartphoneIcon, SlidersHorizontalIcon, SmartphoneIcon, SparklesIcon, XIcon, type LucideIcon } from "lucide-react";
import { useState } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Dialog, DialogContent, DialogTitle } from "./components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./components/ui/tabs";
import { Hint } from "./components/ui/tooltip";
import { useInterfaceTranslation } from "./i18n/client";
import { MobileTab } from "./MobileTab";
import { frontDeviceId } from "./devices";
import { registeredName, thisMachineKey } from "./screenMachine";
import { SETTINGS_GROUPS, type SettingsTab } from "./settings";
import { AboutTab } from "./settings/AboutTab";
import { AgentsTab } from "./settings/AgentsTab";
import { DevicesTab } from "./settings/DevicesTab";
import { GeneralTab } from "./settings/GeneralTab";
import { GithubTab } from "./settings/GithubTab";
import { HideAiTab } from "./settings/HideAiTab";
import { NotificationsTab } from "./settings/NotificationsTab";
import { ShortcutsTab } from "./settings/ShortcutsTab";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

export function SettingsGate({ actions }: { actions: Actions }) {
  const open = useUiStore((s) => s.overlay === "settings");
  return open ? <SettingsSheet actions={actions} /> : null;
}

/**
 * The GitHub page's mark: Lucide's own `github` outline, which Lucide 1.0
 * dropped with every brand icon, kept so the list draws the approved board's
 * glyph at the same stroke as its neighbours.
 */
const GithubIcon = createLucideIcon("github", [
  ["path", { d: "M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4", key: "tonef" }],
  ["path", { d: "M9 18c-4.51 2-5-2-7-2", key: "9comsn" }],
]);

/** Each page's mark in the list. */
const TAB_ICONS: Record<SettingsTab, LucideIcon> = {
  general: SlidersHorizontalIcon,
  notifications: BellIcon,
  devices: MonitorSmartphoneIcon,
  shortcuts: KeyboardIcon,
  agents: BotIcon,
  hideAi: SparklesIcon,
  mobile: SmartphoneIcon,
  github: GithubIcon,
  about: InfoIcon,
};

function SettingsSheet({ actions }: { actions: Actions }) {
  const { t } = useInterfaceTranslation();
  const close = () => useUiStore.getState().closeOverlay("settings");
  const [tab, setTab] = useState<SettingsTab>(() => useUiStore.getState().settingsTab);
  const selected = useShellStore((s) => {
    const id = frontDeviceId(s.rest);
    return s.rest?.navigator?.devices?.find((device) => device.id === id) ?? null;
  });
  const groupName = (id: (typeof SETTINGS_GROUPS)[number]["id"]) => (id === "app" ? t("settings.groups.app") : t(thisMachineKey(null)));
  return (
    <Dialog open onOpenChange={(next) => { if (!next) close(); }}>
      {/* A Shortcuts chip that is recording owns Escape: it cancels the recording, not the sheet (B61).
          A window narrower or shorter than the sheet keeps the window inset clear on every side (settings-left-nav B1, B13). */}
      <DialogContent
        data-settings="true"
        aria-describedby={undefined}
        className="h-(--size-settings-sheet-h-max) max-h-[calc(100%-2*var(--size-settings-sheet-window-inset))] w-(--size-settings-sheet-w) max-w-[calc(100%-2*var(--size-settings-sheet-window-inset))]"
        onEscapeKeyDown={(event) => {
          if (useUiStore.getState().recordingShortcut) event.preventDefault();
        }}
      >
        {/* Radix Tabs owns the roving tabindex: Up and Down (plus Home and End)
            move through the list and show each page, Tab enters the page (B3). */}
        <Tabs value={tab} onValueChange={(value) => setTab(value as SettingsTab)} orientation="vertical" className="min-h-0 flex-1 flex-row gap-none">
          <nav className="flex w-(--size-settings-nav-w) shrink-0 flex-col overflow-y-auto border-r border-border bg-sidebar px-sm pb-md pt-lg">
            <DialogTitle className="px-sm pb-xs text-title font-semibold">{t("common.settings")}</DialogTitle>
            <TabsList aria-label={t("settings.section")} className="w-full flex-1 flex-col items-stretch gap-xxs rounded-none bg-transparent p-none">
              {SETTINGS_GROUPS.map((group) => (
                <div key={group.id} className="flex flex-col gap-xxs" data-settings-nav-group={group.id}>
                  <div className="truncate px-sm pb-xs pt-md text-micro font-medium text-muted-foreground">{groupName(group.id)}</div>
                  {group.tabs.map((row) => (
                    <NavItem key={row} tab={row} label={t(`settings.tabs.${row}`)} />
                  ))}
                </div>
              ))}
              <div className="mt-auto flex flex-col pt-xs">
                <NavItem tab="about" label={t("settings.tabs.about")} />
              </div>
            </TabsList>
          </nav>
          <div className="flex min-w-0 flex-1 flex-col">
            <header className="flex items-center gap-sm px-xl pb-lg pt-lg">
              <div className="flex min-w-0 flex-1 items-center gap-sm">
                <h2 className="truncate text-headline font-semibold" data-settings-page-title={tab}>{t(`settings.tabs.${tab}`)}</h2>
                {/* A remote device's name shows only while it is the selected device (B6), beside the page title (B4). */}
                {selected?.kind === "remote" ? (
                  <Hint label={t("settings.ownerRemote", { device: registeredName(selected) })}>
                    <span className="inline-block min-w-0 max-w-full truncate rounded-sm border border-border px-xs text-caption text-muted-foreground outline-none focus-visible:ring-1 focus-visible:ring-ring" tabIndex={0} data-settings-device={selected.id}>
                      {registeredName(selected)}
                    </span>
                  </Hint>
                ) : null}
              </div>
              <Hint label={t("settings.close")} shortcut="Esc">
                <Button variant="ghost" size="icon-sm" aria-label={t("settings.close")} onClick={close} data-settings-close="true">
                  <XIcon />
                </Button>
              </Hint>
            </header>
            {/* The page is the tab panel and the one thing that scrolls; the list stays where it is (B12, B13). */}
            <TabsContent value={tab} className="min-h-0 overflow-auto px-xl pb-xl focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring" data-settings-page={tab}>
              {tab === "general" ? <GeneralTab actions={actions} /> : null}
              {tab === "notifications" ? <NotificationsTab actions={actions} /> : null}
              {tab === "devices" ? <DevicesTab actions={actions} /> : null}
              {tab === "shortcuts" ? <ShortcutsTab actions={actions} /> : null}
              {tab === "agents" ? <AgentsTab actions={actions} /> : null}
              {tab === "hideAi" ? <HideAiTab actions={actions} /> : null}
              {tab === "mobile" ? <MobileTab actions={actions} /> : null}
              {tab === "github" ? <GithubTab actions={actions} /> : null}
              {tab === "about" ? <AboutTab /> : null}
            </TabsContent>
          </div>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

/** One page in the list: its mark and its name, filled while it is the page shown. */
function NavItem({ tab, label }: { tab: SettingsTab; label: string }) {
  const Icon = TAB_ICONS[tab];
  return (
    <TabsTrigger
      value={tab}
      data-settings-tab={tab}
      className="h-auto min-h-(--size-control) w-full justify-start gap-sm rounded-sm px-sm py-xs text-subhead font-normal data-[state=active]:font-medium"
    >
      <Icon aria-hidden="true" />
      <span className="truncate">{label}</span>
    </TabsTrigger>
  );
}
