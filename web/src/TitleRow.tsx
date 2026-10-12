// The window's top row (PRD sidebar-titlebar-refresh D-15 to D-18, B17 to
// B23): one row where macOS's title bar and the Workspace toolbar were. Over
// the rail and the sidebar it names the device in front, after the traffic
// lights' space on a macOS window that is not full screen; over the center it
// carries the Workspace's path and icons, and nothing on any other screen.
// The row is where the desktop window is dragged from (`.window-drag`), and
// every control in it takes a press instead.

import type { Actions } from "./actions";
import { DeviceTitle } from "./components/sidebar-header";
import { railShown } from "./devices";
import { useDeviceTitle, useSidebarColumns } from "./sidebar";
import { useShellStore } from "./store";
import { trafficLightSpace, useWindowChrome } from "./windowChrome";
import { WorkspaceTitle } from "./WorkspaceScreen";

export function TitleRow({ actions }: { actions: Actions }) {
  const chrome = useWindowChrome();
  const lights = trafficLightSpace(chrome);
  const sidebar = useShellStore((s) => s.rest?.ui_state?.left_sidebar_visible ?? true);
  const rail = useShellStore((s) => railShown(s.rest));
  const { title, deviceMenu } = useDeviceTitle(actions);
  // The device's part spans the rail and the sidebar under it, one edge for both.
  const columns = useSidebarColumns();
  // The device name starts a step after the traffic lights, or at the row's own inset without them.
  const nameStart = lights ? "calc(var(--size-traffic-light-inset) + var(--spacing-sm))" : "var(--spacing-md)";
  return (
    <header className="window-drag flex h-(--size-tab-strip) shrink-0 border-b border-border bg-sidebar" style={columns} data-title-row="true" data-traffic-lights={lights ? "true" : "false"}>
      {sidebar ? (
        <div className="flex w-(--sidebar-total) shrink-0 items-center gap-sm border-r border-border pr-xs" style={{ paddingLeft: nameStart }} data-sidebar-title="true">
          <DeviceTitle rail={rail} title={title} deviceMenu={deviceMenu} />
        </div>
      ) : lights ? (
        <span aria-hidden="true" className="w-(--size-traffic-light-inset) shrink-0" data-traffic-light-space="true" />
      ) : null}
      <div className="flex min-w-0 flex-1 items-stretch" data-title-center="true">
        <WorkspaceTitle actions={actions} />
      </div>
    </header>
  );
}
