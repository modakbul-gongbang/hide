// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { SnapshotRest } from "../snapshot";
import { useShellStore } from "../store";
import { InterfaceLanguageBoundary, translate } from "./client";

it("answers in the confirmed language at call time, outside any render", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  const root = createRoot(container);
  const choose = (language: string | null) => useShellStore.setState({ rest: { ui_state: { interface_language: language } } as unknown as SnapshotRest });
  try {
    choose("ko");
    await act(async () => root.render(<InterfaceLanguageBoundary />));
    expect(translate("common.close")).toBe("닫기");
    choose("ja");
    await act(async () => root.render(<InterfaceLanguageBoundary />));
    expect(translate("common.close")).toBe("閉じる");
    choose(null);
    await act(async () => root.render(<InterfaceLanguageBoundary />));
    expect(translate("common.close")).toBe("Close");
  } finally {
    await act(async () => root.unmount());
    useShellStore.setState({ rest: null });
  }
});
