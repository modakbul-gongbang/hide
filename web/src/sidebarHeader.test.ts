import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SidebarHeader } from "./components/sidebar-header";
import { TooltipProvider } from "./components/ui/tooltip";
import { useUiStore } from "./ui";

function header({
  rail = true,
  title = { name: "This Mac", note: null as string | null },
} = {}) {
  return renderToStaticMarkup(
    createElement(
      TooltipProvider,
      null,
      createElement(SidebarHeader, {
        rail,
        title,
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
        searchChord: "⌘K",
        onSearch: () => undefined,
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
  it("keeps the device heading and opens directly into the project tree (session-first-ui B8)", () => {
    expect(useUiStore.getState().sidebarMode).toBe("projects");
    const html = header({ title: { name: "mini", note: "Remote" } });
    expect(texts(html, "data-sidebar-title-name")).toEqual(["mini"]);
    expect(html).toContain("Remote");
    expect(html).not.toContain("data-sidebar-mode");
    expect(html).not.toContain("data-sidebar-strip");
  });

  it("turns the name into the device menu while the rail is hidden (B6)", () => {
    const html = header({ rail: false });
    expect(tag(html, "data-sidebar-device-menu")).toContain('aria-label="This Mac, switch device"');
    expect(texts(html, "data-sidebar-title-name")).toEqual(["This Mac"]);
    expect(header({ rail: true })).not.toContain("data-sidebar-device-menu");
  });

  it("ends the line with Search alone: Add project sits on the Projects title (B4, first-run-onboarding B52)", () => {
    const html = header();
    expect(tag(html, "data-sidebar-search")).toContain('aria-label="Search"');
    expect(html).not.toContain("data-sidebar-new-workspace");
    // No Search field: the chord is the hint's, never typed in the strip, and nothing takes text.
    expect(html).not.toContain("⌘K");
    expect(html).not.toContain("<input");
  });

});
