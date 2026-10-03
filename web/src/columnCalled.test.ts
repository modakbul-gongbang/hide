// @vitest-environment jsdom
import { afterAll, describe, expect, it, vi } from "vitest";
import { columnCalled } from "./actions";

// Importing the actions loads xterm; answer its canvas probe at the browser boundary.
const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => browserCanvas.restore());

const reveal = (is_directory: boolean) => ({
  schema_version: 2 as const,
  kind: "reveal_path" as const,
  payload: { path: "/projects/studio/src", workspace_id: "project:studio", checkout_id: "checkout:studio", is_directory },
});

describe("columnCalled", () => {
  // PRD three-column-panel B4, D-07: a linked file reaches File Views through
  // the core's call number; a linked folder shows only in the Explorer, so a
  // narrow body shows Tools.
  it("calls Tools for a linked folder and nothing for a linked file", () => {
    expect(columnCalled(reveal(true))).toBe("tools");
    expect(columnCalled(reveal(false))).toBeNull();
  });
});
