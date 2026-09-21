import { describe, expect, it } from "vitest";
import { COMMAND_DELETE, LINE_END, LINE_START, SHIFT_ENTER, mapModifiedKey } from "./keys";

function event(partial: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "a",
    shiftKey: false,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    isComposing: false,
    ...partial,
  } as KeyboardEvent;
}

describe("mapModifiedKey", () => {
  it("maps Shift+Enter to ESC CR", () => {
    expect(mapModifiedKey(event({ key: "Enter", shiftKey: true }))).toEqual(SHIFT_ENTER);
  });

  it("maps meta Backspace to ^U", () => {
    expect(mapModifiedKey(event({ key: "Backspace", metaKey: true }))).toEqual(COMMAND_DELETE);
  });

  it("maps meta arrows to line start and end", () => {
    expect(mapModifiedKey(event({ key: "ArrowLeft", metaKey: true }))).toEqual(LINE_START);
    expect(mapModifiedKey(event({ key: "ArrowRight", metaKey: true }))).toEqual(LINE_END);
  });

  it("skips mappings while composing", () => {
    expect(
      mapModifiedKey(event({ key: "Enter", shiftKey: true, isComposing: true }), true),
    ).toBeNull();
  });

  it("leaves plain keys alone", () => {
    expect(mapModifiedKey(event({ key: "Enter" }))).toBeNull();
    expect(mapModifiedKey(event({ key: "a" }))).toBeNull();
  });
});
