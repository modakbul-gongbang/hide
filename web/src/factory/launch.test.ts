// Until the Factory opens (`launch.ts`), no way the operator has reaches its
// screen: the chord is unclaimed, and the open action every row, menu item,
// palette command and notification goes through leaves the screen as it is.

import { describe, expect, it } from "vitest";
import { createActions } from "../actions";
import { REGISTRY } from "../shortcuts";
import { useUiStore } from "../ui";
import { FACTORY_OPEN } from "./launch";

describe.skipIf(FACTORY_OPEN)("the Factory before it opens", () => {
  it("has no chord, and opening it leaves the screen where it is", () => {
    expect(REGISTRY.map((command) => command.id)).not.toContain("factory_open");
    useUiStore.setState({ screen: { kind: "main" }, overlay: "none", pendingClose: null });
    createActions(() => true).openFactory();
    expect(useUiStore.getState().screen?.kind).toBe("main");
  });
});
