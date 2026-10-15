// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, expect, it, vi } from "vitest";
import { ProjectSettingsScene } from "./gallery/ProjectSettingsScene";
import { clientI18n } from "./i18n/translator";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

const browserLayout = vi.hoisted(() => {
  const canvas = HTMLCanvasElement.prototype.getContext;
  const scroll = Element.prototype.scrollIntoView;
  HTMLCanvasElement.prototype.getContext = () => null;
  // jsdom has no layout. Radix scrolls the selected option into its viewport.
  Element.prototype.scrollIntoView = () => {};
  return { restore: () => {
    HTMLCanvasElement.prototype.getContext = canvas;
    Element.prototype.scrollIntoView = scroll;
  } };
});
afterAll(() => browserLayout.restore());

it("keeps an acknowledged change settled when another window replaces its receipt", async () => {
  const savedShell = useShellStore.getState();
  const savedUi = useUiStore.getState();
  const language = clientI18n.language;
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => {
      await clientI18n.changeLanguage("en");
      root.render(<ProjectSettingsScene params={new URLSearchParams("lang=en")} />);
    });
    const control = () => document.querySelector<HTMLButtonElement>('[role="combobox"][aria-label="Issue source"]')!;
    await act(async () => control().dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
    const local = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find((option) => option.textContent === "Local")!;
    expect(local).toBeDefined();
    await act(async () => local.click());
    expect(document.querySelector("[data-project-settings]")?.textContent).toContain("Saved");
    await act(async () => useShellStore.setState((shell) => {
      const record = shell.rest!.ui_state!.project_settings!.example!;
      return { rest: { ...shell.rest, ui_state: { ...shell.rest!.ui_state, project_settings_writes: {
        example: { device_id: record.device_id, root: record.root, request_id: "another-window", state: "saved", error: null, patch: { new_work: "worktree" } },
      } } } };
    }));
    expect(control().disabled).toBe(false);
    expect(document.querySelector("[data-project-settings]")?.textContent).not.toContain("Saving");
  } finally {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(savedShell, true);
    useUiStore.setState(savedUi, true);
    await clientI18n.changeLanguage(language);
    vi.unstubAllGlobals();
  }
});
