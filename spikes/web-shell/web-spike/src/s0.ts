export type S0Frame = { t: number; dt: number };
export type HopTiming = {
  notified_ms: number | null;
  requested_ms: number;
  owner_started_ms: number;
  owner_finished_ms: number;
  ws_send_ms: number;
};
export type EchoSample = HopTiming & { arrival_ms: number; write_ms: number };

declare global {
  interface Window {
    __s0Arm: (text: string) => void;
    __s0WaitFor: () => Promise<EchoSample>;
    __s0Frames: S0Frame[];
    __s0ReplayDone: boolean;
    __s0ReplayReady: boolean;
    __s0StartReplay: () => Promise<void>;
    __s0ReplayWindow: { start: number; end: number; duration_ms: number } | null;
    __s0PaneId: string | null;
  }
}

export const epochMs = () => performance.timeOrigin + performance.now();
let armed: { marker: string; resolve: (s: EchoSample) => void; timer: ReturnType<typeof setTimeout> } | null = null;

export function installS0Hooks(): void {
  window.__s0Frames = [];
  window.__s0ReplayDone = false;
  window.__s0ReplayReady = false;
  window.__s0ReplayWindow = null;
  window.__s0PaneId = null;
  let promise: Promise<EchoSample> | null = null;
  window.__s0Arm = (marker) => {
    if (armed) throw new Error("echo sample already armed");
    promise = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        armed = null;
        reject(new Error("echo timeout"));
      }, 3000);
      armed = { marker, resolve, timer };
    });
    // A timeout is retrieved through __s0WaitFor even if the driver is delayed.
    void promise.catch(() => {});
  };
  window.__s0WaitFor = () => {
    if (!promise) throw new Error("echo sample not armed");
    return promise;
  };
}

export function noteWriteComplete(screen: () => string, timing: HopTiming, arrival_ms: number): void {
  if (!armed) return;
  const write_ms = epochMs();
  // Match the parsed terminal buffer: ANSI may encode repeated characters or
  // cursor movement, so matching raw WS bytes would miss a valid echo.
  if (!screen().includes(armed.marker)) return;
  clearTimeout(armed.timer);
  armed.resolve({ ...timing, arrival_ms, write_ms });
  armed = null;
}

export function bytesToBase64(value: string): string {
  let binary = "";
  for (const byte of new TextEncoder().encode(value)) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function decodeBase64(value: string): string {
  return new TextDecoder().decode(Uint8Array.from(atob(value), (c) => c.charCodeAt(0)));
}
