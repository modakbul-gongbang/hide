import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SidebarHeader } from "./components/sidebar-header";
import { TooltipProvider } from "./components/ui/tooltip";
import { useUiStore, type SidebarMode } from "./ui";

function header({
  mode = "projects" as SidebarMode,
  rail = true,
  title = { name: "This Mac", note: null as string | null },
  addProject = true,
  tabs = true,
  switchChord = null as string | null,
  canAdd = true,
} = {}) {
  return renderToStaticMarkup(
    createElement(
      TooltipProvider,
      null,
      createElement(SidebarHeader, {
        rail,
        title,
        addProject,
        tabs,
        mode,
        deviceMenu: {
          devices: [
            { id: "local", label: "This Mac", remote: false, connected: true },
            { id: "mini", label: "mini", remote: true, connected: true },
          ],
          frontId: "local",
          onSelect: () => undefined,
          onAddDevice: () => undefined,
          onShowRail: () => undefined,
        },
        switchChord,
        searchChord: "⌘K",
        newWorkspaceChord: "⇧⌘N",
        onMode: () => undefined,
        onSearch: () => undefined,
        onNewWorkspace: canAdd ? () => undefined : null,
      }),
    ),
  );
}

/** The opening tag of the first element carrying `attribute`. */
function tag(html: string, attribute: string): string | null {
  return html.match(new RegExp(`<[a-z]+[^>]*\\s${attribute}(?=[\\s=>])[^>]*>`))?.[0] ?? null;
}

/** The text of every element carrying `attribute`, in document order. */
function texts(html: string, attribute: string): string[] {
  return [...html.matchAll(new RegExp(`<([a-z]+)[^>]*\\s${attribute}(?=[\\s=>])[^>]*>(.*?)</\\1>`, "g"))].map((match) => match[2]!.replace(/<[^>]+>/g, ""));
}

describe("sidebar header (PRD sidebar-shell)", () => {
  it("opens on the Projects tab, Projects left of Agents (B3)", () => {
    expect(useUiStore.getState().sidebarMode).toBe("projects");
    const html = header();
    expect(texts(html, "data-sidebar-mode")).toEqual(["Projects", "Agents"]);
    expect(tag(html, 'data-sidebar-mode="projects"')).toContain('aria-pressed="true"');
    expect(tag(html, 'data-sidebar-mode="agents"')).toContain('aria-pressed="false"');
  });

  it("is the device's name with Add project and Search, then the tab strip, for every device (B3)", () => {
    const html = header({ title: { name: "mini", note: "Remote" } });
    expect(texts(html, "data-sidebar-title-name")).toEqual(["mini"]);
    expect(html).toContain("Remote");
    expect(html.indexOf("data-sidebar-title")).toBeLessThan(html.indexOf("data-sidebar-strip"));
    expect(html.indexOf("data-sidebar-new-workspace")).toBeLessThan(html.indexOf("data-sidebar-search"));
    expect(html).not.toContain("data-home-row");
    expect(html).not.toContain("data-sidebar-device-menu");
  });

  it("draws no tab strip and no Add project for a device that cannot be read (B3)", () => {
    const html = header({ title: { name: "mini", note: "Remote" }, tabs: false, addProject: false });
    expect(html).not.toContain("data-sidebar-strip");
    expect(html).not.toContain("data-sidebar-mode");
    expect(html).not.toContain("data-sidebar-new-workspace");
    expect(tag(html, "data-sidebar-search")).toContain('aria-label="Search"');
  });

  it("turns the name into the device menu while the rail is hidden (B6)", () => {
    const html = header({ rail: false });
    expect(tag(html, "data-sidebar-device-menu")).toContain('aria-label="This Mac, 기기 전환"');
    expect(texts(html, "data-sidebar-title-name")).toEqual(["This Mac"]);
    expect(header({ rail: true })).not.toContain("data-sidebar-device-menu");
  });

  it("puts Search at the line's end, and Add project before it where the caller offers it, on Projects only (B4, B5)", () => {
    const projects = header();
    const newWorkspace = tag(projects, "data-sidebar-new-workspace");
    const search = tag(projects, "data-sidebar-search");
    expect(newWorkspace).toContain('aria-label="Add project"');
    // A browser tab has no folder picker, so it offers no Add project.
    expect(header({ canAdd: false })).not.toContain("data-sidebar-new-workspace");
    expect(search).toContain('aria-label="Search"');
    expect(projects.indexOf("data-sidebar-new-workspace")).toBeLessThan(projects.indexOf("data-sidebar-search"));
    const agents = header({ mode: "agents", addProject: false });
    expect(agents).not.toContain("data-sidebar-new-workspace");
    expect(tag(agents, "data-sidebar-search")).toContain('aria-label="Search"');
    // No Search field: the chord is the hint's, never typed in the strip, and nothing takes text.
    expect(projects).not.toContain("⌘K");
    expect(projects).not.toContain("<input");
  });

  it("names a tab by a hint only when the switch has a chord", () => {
    expect(tag(header(), 'data-sidebar-mode="projects"')).not.toContain("aria-label");
    const bound = header({ switchChord: "⌃⌘S" });
    expect(tag(bound, 'data-sidebar-mode="projects"')).toContain('aria-label="Projects"');
    expect(bound).not.toContain("⌃⌘S");
  });
});
