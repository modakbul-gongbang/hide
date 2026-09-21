export type S0Frame = { t: number; dt: number };

declare global {
  interface Window {
    __s0Echo: string;
    __s0EchoAt: number | null;
    __s0WaitFor: (text: string, timeoutMs?: number) => Promise<number>;
    __s0Frames: S0Frame[];
    __s0ReplayDone: boolean;
    __s0PaneId: string | null;
  }
}

export function installS0Hooks(): void {
  window.__s0Echo = "";
  window.__s0EchoAt = null;
  window.__s0Frames = [];
  window.__s0ReplayDone = false;
  window.__s0PaneId = null;
  window.__s0WaitFor = (text: string, timeoutMs = 2000) =>
    new Promise((resolve, reject) => {
      const started = performance.now();
      const timer = setInterval(() => {
        if (window.__s0Echo.includes(text)) {
          clearInterval(timer);
          resolve(window.__s0EchoAt ?? Date.now());
          return;
        }
        if (performance.now() - started > timeoutMs) {
          clearInterval(timer);
          reject(new Error(`echo timeout waiting for ${JSON.stringify(text)}`));
        }
      }, 2);
    });
}

export function noteEcho(chunk: string): void {
  window.__s0Echo += chunk;
  if (window.__s0Echo.length > 8192) {
    window.__s0Echo = window.__s0Echo.slice(-4096);
  }
  window.__s0EchoAt = Date.now();
}

export function bytesToBase64(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

export function decodeBase64(value: string): string {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
}
