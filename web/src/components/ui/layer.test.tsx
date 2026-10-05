// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { useReturnFocus } from "./layer";

type Handback = (event: Event) => void;

/** A layer's content part: mounted while open, it reports its hand-back the way Radix calls it after the close. */
function mountLayer(onHandback: (handback: Handback) => void) {
  function Content() {
    onHandback(useReturnFocus(true));
    return null;
  }
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  act(() => root.render(<Content />));
  return () => {
    act(() => root.unmount());
    container.remove();
  };
}

function holder(label: string) {
  const button = document.createElement("button");
  button.textContent = label;
  document.body.append(button);
  button.focus();
  return button;
}

function handBack(handback: Handback) {
  const event = new Event("focusScopeAutoFocusOnUnmount", { cancelable: true });
  handback(event);
  return event;
}

it("hands the keyboard back to the element that held it when its layer closed", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const before = holder("before");
  let handback: Handback = () => {};
  const close = mountLayer((next) => { handback = next; });
  (document.activeElement as HTMLElement).blur();
  close();
  const event = handBack(handback);
  expect(event.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(before);
  before.remove();
});

it("does not take the keyboard back once a newer layer opened and closed after its own closed", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const settingsButton = holder("settings");
  let older: Handback = () => {};
  const closeOlder = mountLayer((next) => { older = next; });
  closeOlder();
  // The older layer's timer is still queued behind input while the next layer takes the keyboard and gives it back.
  holder("terminal");
  const closeNewer = mountLayer(() => {});
  (document.activeElement as HTMLElement).blur();
  closeNewer();
  // Its own hand-back has not run yet, so nothing holds the keyboard when the older one does.
  expect(document.activeElement).toBe(document.body);
  const event = handBack(older);
  expect(event.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(document.body);
  settingsButton.remove();
});
