import { describe, expect, it } from "vitest";
import { echoDecision, joinDocument, minimalChange, peerState, type PeerState, type TextChange } from "./sync";

function apply(text: string, change: TextChange | null): string {
  return change ? text.slice(0, change.from) + change.insert + text.slice(change.to) : text;
}

describe("the change another view applies", () => {
  it("replaces only what lies between the common prefix and suffix", () => {
    expect(minimalChange("hello world", "hello brave world")).toEqual({ from: 6, to: 6, insert: "brave " });
    expect(minimalChange("abc", "ac")).toEqual({ from: 1, to: 2, insert: "" });
    expect(minimalChange("same", "same")).toBeNull();
    expect(minimalChange("", "new")).toEqual({ from: 0, to: 0, insert: "new" });
  });

  it("inserts into a run of one character without replacing any of it", () => {
    const change = minimalChange("aaaa", "aaaaa");
    expect(change).toEqual({ from: 4, to: 4, insert: "a" });
    expect(apply("aaaa", change)).toBe("aaaaa");
  });

  it("never splits a surrogate pair and reproduces the text exactly", () => {
    const before = "note 😀 end";
    const after = "note 😃 end";
    const change = minimalChange(before, after);
    expect(change).toEqual({ from: 5, to: 7, insert: "😃" });
    expect(apply(before, change)).toBe(after);
  });

  it("reproduces Korean composed text", () => {
    const before = "메모: 한";
    const after = "메모: 한글";
    expect(apply(before, minimalChange(before, after))).toBe(after);
  });
});

describe("the echo rule for each view", () => {
  it("settles on its own text, drops an older echo while one is pending, and takes a real change", () => {
    expect(echoDecision("typed", "typed", "typed")).toBe("settle");
    expect(echoDecision("type", "typed", "typed")).toBe("drop");
    expect(echoDecision("typed", "type", "typed")).toBe("replace");
    expect(echoDecision("reloaded", "typed", null)).toBe("replace");
  });

  it("drops a stale echo in the view that received the edit as well as the one that made it", () => {
    const received: string[] = [];
    const typing = joinDocument("file:a", () => undefined, idle);
    const other = joinDocument("file:a", (contents) => received.push(contents), idle);
    typing.publish("ab");
    expect(received).toEqual(["ab"]);
    // Both views now hold "ab" with "ab" pending; the core's echo of "a" is older.
    const pending = received[0] ?? null;
    expect(echoDecision("a", "ab", "ab")).toBe("drop");
    expect(echoDecision("a", pending ?? "", pending)).toBe("drop");
    expect(echoDecision("ab", pending ?? "", pending)).toBe("settle");
    typing.leave();
    other.leave();
  });
});

describe("the channel", () => {
  it("reaches every other view of the same document and no one else", () => {
    const heard: string[] = [];
    const one = joinDocument("file:a", (contents) => heard.push(`one:${contents}`), idle);
    const two = joinDocument("file:a", (contents) => heard.push(`two:${contents}`), idle);
    const elsewhere = joinDocument("file:b", (contents) => heard.push(`b:${contents}`), idle);
    one.publish("x");
    expect(heard).toEqual(["two:x"]);
    two.leave();
    one.publish("y");
    expect(heard).toEqual(["two:x"]);
    one.leave();
    elsewhere.leave();
  });

  it("starts a view that mounts later from a sibling's text and pending echo, else from the core", () => {
    expect(peerState("file:c")).toBeNull();
    const ahead = joinDocument("file:c", () => undefined, () => ({ text: "typed", pending: "typed" }));
    expect(peerState("file:c")).toEqual({ text: "typed", pending: "typed" });
    ahead.leave();
    expect(peerState("file:c")).toBeNull();
  });
});

function idle(): PeerState {
  return { text: "", pending: null };
}
