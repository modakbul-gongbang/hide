// The device switcher at the bottom of the sidebar, the web form of the
// native `SidebarUtilityBar` chip and `HideDevicePicker` (docs/UI_BEHAVIOR.md "Device
// picker"): a compact chip naming the selected device opens one flat list,
// each row the device's name over Local or Remote, its connection and its
// agent count. The snapshot owns connection and selection; the list is a
// pick-one menu, so DropdownMenu's own roving focus and Escape already give
// it the arrow-key and dismiss behavior the native picker has.

import { CheckIcon, ChevronsUpDownIcon, LaptopIcon, ServerIcon } from "lucide-react";
import { useRef, useState } from "react";
import type { Actions } from "./actions";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "./components/ui/dropdown-menu";
import { Hint } from "./components/ui/tooltip";
import { deviceDetail } from "./remote";
import type { Device } from "./snapshot";
import { useShellStore } from "./store";
import { restoreFocus } from "./terminals";

const NO_DEVICES: Device[] = [];

function DeviceIcon({ remote }: { remote: boolean }) {
  const Icon = remote ? ServerIcon : LaptopIcon;
  return <Icon aria-hidden="true" className="size-(--size-icon-sm) shrink-0" />;
}

export function DevicePicker({ actions }: { actions: Actions }) {
  const devices = useShellStore((s) => s.rest?.navigator?.devices ?? NO_DEVICES);
  const selectedId = useShellStore((s) => s.rest?.navigator?.focused_device_id ?? "local");
  const [open, setOpen] = useState(false);
  const selected = devices.find((device) => device.id === selectedId) ?? devices[0] ?? null;
  // Choosing a device returns the keyboard to the terminal, the way a closed
  // sheet does; dismissing without choosing (Escape, outside click) leaves it
  // on the trigger, which is Radix's own default. Only a choice flips this.
  const chose = useRef(false);

  const choose = (device: Device) => {
    chose.current = true;
    if (device.id !== selectedId) actions.focusDevice(device.id);
    setOpen(false);
  };

  const label = selected?.label ?? "No device";
  return (
    <div className="min-w-0">
      <DropdownMenu open={open} onOpenChange={setOpen}>
        <Hint label={`Choose device, ${label}`}>
          <DropdownMenuTrigger
            data-device-picker-trigger={selected?.id ?? ""}
            className="inline-flex h-(--size-control-sm) max-w-full items-center gap-xs rounded-sm px-xs text-caption text-subtle-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring data-[state=open]:bg-accent data-[state=open]:text-foreground"
          >
            <DeviceIcon remote={selected?.kind === "remote"} />
            <span className="min-w-0 truncate">{label}</span>
            <ChevronsUpDownIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-muted-foreground" />
          </DropdownMenuTrigger>
        </Hint>
        <DropdownMenuContent
          align="start"
          side="top"
          aria-label="Devices"
          data-device-picker="true"
          className="max-h-(--size-relationship-list-max) w-(--size-tooltip-max-width) max-w-[calc(100vw-var(--spacing-lg))]"
          onCloseAutoFocus={(event) => {
            if (!chose.current) return;
            chose.current = false;
            event.preventDefault();
            // Back to the terminal the way a sheet returns it; the next
            // snapshot's focused pane takes it once the context has switched.
            restoreFocus(document.querySelector<HTMLElement>(".xterm textarea"));
          }}
        >
          <DropdownMenuLabel>Devices</DropdownMenuLabel>
          {devices.length === 0 ? <p className="px-sm py-sm text-body text-subtle-foreground">No devices available</p> : null}
          {devices.map((device) => {
            const isSelected = device.id === selectedId;
            const detail = deviceDetail(device);
            return (
              <DropdownMenuItem
                key={device.id}
                data-device-option={device.id}
                aria-label={[device.label, detail, isSelected ? "Selected" : null].filter(Boolean).join(", ")}
                className="items-start gap-md"
                onSelect={() => choose(device)}
              >
                <DeviceIcon remote={device.kind === "remote"} />
                <span className="flex min-w-0 flex-1 flex-col gap-xxs">
                  <span className="truncate text-subhead font-semibold text-foreground">{device.label}</span>
                  <span className="text-caption text-subtle-foreground">{detail}</span>
                </span>
                {isSelected ? <CheckIcon aria-hidden="true" className="size-(--size-icon-sm) shrink-0 text-foreground" /> : null}
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
