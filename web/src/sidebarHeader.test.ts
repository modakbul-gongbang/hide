import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SidebarHeader } from "./components/sidebar-header";
import { TooltipProvider } from "./components/ui/tooltip";
import { useUiStore, type SidebarMode } from "./ui";

function header({ mode = "projects" as SidebarMode, overviewSelected = false, projectCount = 12 as number | null, switchChord = null as string | null, canAdd = true } = {}) {
  return renderToStaticMarkup(
    createElement(
      TooltipProvider,
      null,
      createElement(SidebarHeader, {
        mode,
        overviewSelected,
        projectCount,
        switchChord,
        searchChord: "⌘K",
        newWorkspaceChord: "⇧⌘N",
        onOverview: () => undefined,
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

  it("heads the pane with the Overview row and the project count, marked only while it is in front (B1, B2)", () => {
    const rest = header();
    expect(rest.indexOf("data-overview-destination")).toBeLessThan(rest.indexOf("data-sidebar-strip"));
    expect(rest).toContain(">Overview</span>");
    expect(texts(rest, "data-overview-count")).toEqual(["12 projects"]);
    expect(tag(rest, "data-overview-destination")).not.toContain("aria-current");
    expect(tag(rest, "data-overview-destination")).not.toContain("bg-secondary");
    const selected = tag(header({ overviewSelected: true }), "data-overview-destination");
    expect(selected).toContain('aria-current="page"');
    expect(selected).toContain("bg-secondary");
    expect(texts(header({ projectCount: 1 }), "data-overview-count")).toEqual(["1 project"]);
    // Before the first snapshot there is no count to show, rather than a zero.
    expect(header({ projectCount: null })).not.toContain("data-overview-count");
  });

  it("puts Search at the strip's end, and Add project before it on Projects only (B4, B5)", () => {
    const projects = header();
    const newWorkspace = tag(projects, "data-sidebar-new-workspace");
    const search = tag(projects, "data-sidebar-search");
    expect(newWorkspace).toContain('aria-label="Add project"');
    // A browser tab has no folder picker, so it offers no Add project.
    expect(header({ canAdd: false })).not.toContain("data-sidebar-new-workspace");
    expect(search).toContain('aria-label="Search"');
    expect(projects.indexOf("data-sidebar-new-workspace")).toBeLessThan(projects.indexOf("data-sidebar-search"));
    const agents = header({ mode: "agents" });
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
