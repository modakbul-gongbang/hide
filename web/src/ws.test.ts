// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { BACKOFF_START_MS, nextBackoff, type ConnectionState } from "./connection";
import { configureFileBytes, requestFileBytes } from "./fileBytes";
import { useShellStore } from "./store";
import { connectShell } from "./ws";

type Listener = (event: unknown) => void;

/** A server end the test drives: what the shell sent, and the events it hears. */
class FakeSocket {
  static readonly OPEN = 1;
  static made: FakeSocket[] = [];
  readyState = 0;
  binaryType = "";
  readonly sent: string[] = [];
  private readonly listeners = new Map<string, Listener[]>();

  constructor(readonly url: string) {
    FakeSocket.made.push(this);
  }

  addEventListener(type: string, listener: Listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  send(data: unknown) {
    this.sent.push(String(data));
  }

  close() {}

  open() {
    this.readyState = FakeSocket.OPEN;
    this.emit("open", {});
  }

  frame(frame: unknown) {
    this.emit("message", { data: JSON.stringify(frame) });
  }

  closeWith(code: number, reason: string) {
    this.readyState = 3;
    this.emit("close", { code, reason });
  }

  handshake(): { have_revision: number } {
    return JSON.parse(this.sent[0] ?? "{}") as { have_revision: number };
  }

  private emit(type: string, event: unknown) {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

const saved = useShellStore.getState();

function made(index: number): FakeSocket {
  const socket = FakeSocket.made[index];
  if (!socket) throw new Error(`no socket ${index}`);
  return socket;
}

beforeEach(() => {
  vi.useFakeTimers();
  FakeSocket.made = [];
  vi.stubGlobal("WebSocket", FakeSocket);
  vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true })));
  window.location.hash = "#token=t";
});

afterEach(() => {
  useShellStore.setState(saved, true);
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

// R3, B17: a node closes a screen that fell behind with 1013
// `screen_fell_behind`. The shell treats it as any dropped socket: the read
// it waited for fails, it shows reconnecting (never gone) and reattaches at
// its first backoff from the revision it applied, and is drawn whole from
// the snapshot it is answered.
it("a screen closed as fallen behind fails its read and reattaches at the first attempt", async () => {
  const states: ConnectionState[] = [];
  const stop = useShellStore.subscribe((state) => states.push(state.connection));
  const full: boolean[] = [];
  const shell = connectShell({ onChunks: (_chunks, isFull) => full.push(isFull) });
  configureFileBytes(shell.dispatch);

  const first = made(0);
  first.open();
  first.frame({ type: "snapshot", payload: { revision: 5, rest: {} } });
  expect(useShellStore.getState().connection).toBe("live");
  const read = requestFileBytes("/repo/big.bin");
  const failed = read.then(
    () => "answered",
    (error: Error) => error.message,
  );

  first.closeWith(1013, "screen_fell_behind");
  expect(await failed).toBe("socket_closed");
  const firstAttempt = nextBackoff(BACKOFF_START_MS);
  await vi.advanceTimersByTimeAsync(firstAttempt - 1);
  expect(FakeSocket.made).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(FakeSocket.made).toHaveLength(2);

  const again = made(1);
  again.open();
  expect(again.handshake().have_revision).toBe(5);
  again.frame({ type: "snapshot", payload: { revision: 9, rest: {} } });
  expect(full.at(-1)).toBe(true);
  expect(useShellStore.getState().revision).toBe(9);
  expect(useShellStore.getState().connection).toBe("live");
  expect(states).toContain("reconnecting");
  expect(states).not.toContain("gone");
  stop();
  shell.close();
});
