// The device switcher at the bottom of the sidebar, the web form of the
// native `SidebarUtilityBar` chip and `HideDevicePicker` (DESIGN.md "Device
// picker"): a compact chip naming the selected device opens one flat list,
// each row the device's name over Local or Remote, its connection and its
// agent count. The snapshot owns connection and selection; arrow keys move
// only the keyboard focus, and a device is chosen by activating its row.

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { Actions } from "./actions";
import { deviceDetail } from "./remote";
import type { Device } from "./snapshot";
import { useShellStore } from "./store";
import { restoreFocus } from "./terminals";
import { useUiStore } from "./ui";

const NO_DEVICES: Device[] = [];

function DeviceIcon({ remote }: { remote: boolean }) {
  // Line drawings of the native `laptopcomputer` and `server.rack` symbols.
  return (
    <svg aria-hidden="true" viewBox="0 0 16 16" className="h-[1em] w-[1em] shrink-0" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      {remote ? (
        <>
          <rect x="2.5" y="2.5" width="11" height="4.5" rx="1" />
          <rect x="2.5" y="9" width="11" height="4.5" rx="1" />
          <path d="M5 4.75h.01M5 11.25h.01" />
        </>
      ) : (
        <>
          <rect x="3" y="3.5" width="10" height="7" rx="1" />
          <path d="M1.5 12.5h13" />
        </>
      )}
    </svg>
  );
}

export function DevicePicker({ actions }: { actions: Actions }) {
  const devices = useShellStore((s) => s.rest?.navigator?.devices ?? NO_DEVICES);
  const selectedId = useShellStore((s) => s.rest?.navigator?.focused_device_id ?? "local");
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const selected = devices.find((device) => device.id === selectedId) ?? devices[0] ?? null;

  useEffect(() => {
    if (!open) return;
    const remove = useUiStore.getState().pushEscape(() => {
      setOpen(false);
      trigger.current?.focus();
    });
    // Focus starts on the selected row, like the native picker's reconcileFocus.
    const rows = [...(list.current?.querySelectorAll<HTMLButtonElement>("button[data-device-option]") ?? [])];
    (rows.find((row) => row.dataset.deviceOption === selectedId) ?? rows[0])?.focus();
    const outside = (event: PointerEvent) => {
      if (!list.current?.contains(event.target as Node) && !trigger.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", outside, true);
    return () => {
      remove();
      window.removeEventListener("pointerdown", outside, true);
    };
    // The focus is placed when the list opens, not again on every snapshot,
    // so the effect runs on `open` alone.
  }, [open]);

  const move = (event: KeyboardEvent<HTMLUListElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const rows = [...(list.current?.querySelectorAll<HTMLButtonElement>("button[data-device-option]") ?? [])];
    const index = rows.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === "ArrowDown" ? Math.min(index + 1, rows.length - 1) : Math.max(index - 1, 0);
    rows[next]?.focus();
  };

  const choose = (device: Device) => {
    setOpen(false);
    if (device.id !== selectedId) actions.focusDevice(device.id);
    // Back to the terminal the way a sheet returns it (`restoreFocus`); the
    // next snapshot's focused pane takes it once the context has switched.
    restoreFocus(document.querySelector<HTMLElement>(".xterm textarea"));
  };

  const label = selected?.label ?? "No device";
  return (
    <div className="relative shrink-0 border-t border-border px-md py-xs">
      <button
        ref={trigger}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`Choose device, ${label}`}
        title="Choose device"
        data-device-picker-trigger={selected?.id ?? ""}
        className="inline-flex h-[var(--size-control-compact)] max-w-full items-center gap-xs rounded-sm px-xs text-caption text-subtle-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring"
        onClick={() => setOpen(!open)}
      >
        <DeviceIcon remote={selected?.kind === "remote"} />
        <span className="min-w-0 truncate">{label}</span>
        <span aria-hidden="true" className="text-muted-foreground">
          ▾
        </span>
      </button>
      {open ? (
        <div className="absolute bottom-full left-sm z-30 mb-xxs w-[var(--size-tooltip-max-width)] max-w-[calc(100vw-var(--spacing-lg))] rounded-md border border-border bg-secondary p-md shadow-lg" data-device-picker="true">
          <p className="mb-sm px-sm text-caption font-bold text-muted-foreground">DEVICES</p>
          {devices.length === 0 ? (
            <p className="p-sm text-body text-subtle-foreground">No devices available</p>
          ) : (
            <ul ref={list} role="listbox" aria-label="Devices" onKeyDown={move} className="max-h-[var(--size-relationship-list-max)] space-y-xxs overflow-auto">
              {devices.map((device) => {
                const isSelected = device.id === selectedId;
                const detail = deviceDetail(device);
                return (
                  <li key={device.id} role="option" aria-selected={isSelected}>
                    <button
                      type="button"
                      data-device-option={device.id}
                      aria-label={[device.label, detail, isSelected ? "Selected" : null].filter(Boolean).join(", ")}
                      className="relative flex w-full items-center gap-md rounded-md px-md py-sm text-left outline-none hover:bg-popover focus-visible:ring-1 focus-visible:ring-ring"
                      onClick={() => choose(device)}
                    >
                      {isSelected ? <span aria-hidden="true" className="pointer-events-none absolute inset-0 rounded-md bg-primary opacity-[var(--opacity-selected-fill)]" /> : null}
                      <span className="w-[var(--size-checkout-icon)] text-title text-subtle-foreground">
                        <DeviceIcon remote={device.kind === "remote"} />
                      </span>
                      <span className="flex min-w-0 flex-1 flex-col gap-xxs">
                        <span className="truncate text-subhead font-semibold text-foreground">{device.label}</span>
                        <span className="text-caption text-subtle-foreground">{detail}</span>
                      </span>
                      {isSelected ? (
                        <span aria-hidden="true" className="text-body font-semibold text-foreground">
                          ✓
                        </span>
                      ) : null}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  );
}
