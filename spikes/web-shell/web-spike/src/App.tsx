import { useEffect, useMemo, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";
import { bytesToBase64, decodeBase64, installS0Hooks, noteEcho, type S0Frame } from "./s0";

type Mode = "live" | "replay" | "ime";

type WireMessage = {
  type: string;
  payload?: {
    terminal?: { pane_id?: string; chunks?: Chunk[] };
    chunks?: Chunk[];
    rest?: { focused?: { pane_id?: string } };
  };
};

type Chunk = { pane_id?: string; bytes_base64?: string };

type CaptureLine = { t_ms: number; type: string; payload: WireMessage["payload"] };

function modeFromLocation(): Mode {
  const value = new URLSearchParams(window.location.search).get("mode");
  if (value === "replay" || value === "ime") return value;
  return "live";
}

export function App() {
  const mode = useMemo(modeFromLocation, []);
  if (mode === "ime") return <ImeProcedure />;
  return <Pane mode={mode} />;
}

function Pane({ mode }: { mode: Exclude<Mode, "ime"> }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [banner, setBanner] = useState(
    mode === "live" ? "connecting…" : "loading capture…",
  );

  useEffect(() => {
    installS0Hooks();
    const term = new Terminal({
      cols: 80,
      rows: 24,
      fontFamily: "SF Mono, Menlo, Monaco, monospace",
      fontSize: 13,
      theme: { background: "#111111", foreground: "#dddddd" },
      scrollback: 2000,
    });
    term.open(hostRef.current!);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      setBanner((current) => `${current} (webgl fallback to dom renderer)`);
    }

    const feedChunks = (chunks: Chunk[] | undefined) => {
      if (!chunks) return;
      for (const chunk of chunks) {
        if (!chunk.bytes_base64) continue;
        const text = decodeBase64(chunk.bytes_base64);
        term.write(text);
        noteEcho(text);
      }
    };

    let closed = false;
    if (mode === "live") {
      const socket = new WebSocket(`ws://${window.location.host}/ws`);
      socket.addEventListener("open", () => setBanner("live"));
      socket.addEventListener("close", () => {
        if (!closed) setBanner("disconnected");
      });
      socket.addEventListener("message", (event) => {
        const message = JSON.parse(String(event.data)) as WireMessage;
        const paneId =
          message.payload?.rest?.focused?.pane_id ??
          message.payload?.terminal?.pane_id ??
          message.payload?.rest?.tab?.panes?.[0]?.id;
        if (paneId && !window.__s0PaneId) {
          window.__s0PaneId = paneId;
          socket.send(
            JSON.stringify({
              schema_version: 2,
              kind: "terminal_resize",
              payload: { pane_id: paneId, cols: 80, rows: 24, new_view: true },
            }),
          );
        }
        feedChunks(message.payload?.chunks ?? message.payload?.terminal?.chunks);
      });
      const dataSub = term.onData((data) => {
        const paneId = window.__s0PaneId;
        if (!paneId || socket.readyState !== WebSocket.OPEN) return;
        socket.send(
          JSON.stringify({
            schema_version: 2,
            kind: "key",
            payload: {
              pane_id: paneId,
              bytes_base64: bytesToBase64(data),
            },
          }),
        );
      });
      return () => {
        closed = true;
        dataSub.dispose();
        socket.close();
        term.dispose();
      };
    }

    const frames: S0Frame[] = [];
    let last = performance.now();
    let raf = 0;
    const onFrame = (t: number) => {
      frames.push({ t, dt: t - last });
      last = t;
      window.__s0Frames = frames;
      raf = requestAnimationFrame(onFrame);
    };
    raf = requestAnimationFrame(onFrame);

    const abort = { stopped: false };
    void (async () => {
      const response = await fetch("/capture.jsonl");
      if (!response.ok) {
        setBanner("capture.jsonl missing");
        return;
      }
      const text = await response.text();
      const lines: CaptureLine[] = text
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line) as CaptureLine);
      if (abort.stopped) return;
      setBanner(`replay ${lines.length} deltas`);
      const origin = performance.now();
      const t0 = lines[0]?.t_ms ?? 0;
      for (const line of lines) {
        if (abort.stopped) return;
        const wait = line.t_ms - t0 - (performance.now() - origin);
        if (wait > 0) {
          await new Promise((resolve) => setTimeout(resolve, wait));
        }
        feedChunks(line.payload?.chunks ?? line.payload?.terminal?.chunks);
      }
      window.__s0ReplayDone = true;
      setBanner(`replay done; frames=${frames.length}`);
    })();

    return () => {
      abort.stopped = true;
      cancelAnimationFrame(raf);
      term.dispose();
    };
  }, [mode]);

  return (
    <div className="shell">
      <div className="banner">
        S0 {mode} · {banner}
      </div>
      <div className="term" ref={hostRef} />
    </div>
  );
}

function ImeProcedure() {
  return (
    <div className="ime">
      <h1>S0 IME V9 four checks</h1>
      <p>
        Run these in this Chrome tab with a live xterm.js pane (open
        <code> ?mode=live </code>
        first, then return here with the live pane still available, or perform
        them on the live tab).
        Status stays
        <strong> PENDING_HUMAN </strong>
        until you fill REPORT.md.
      </p>
      <ol>
        <li>
          Candidate window follows the cursor while composing Hangul.
          <div className="slot">screenshot slot: ime-01-candidate-follows-cursor.png</div>
        </li>
        <li>
          Backspace during composition does not leak DEL to the shell.
          <div className="slot">screenshot slot: ime-02-backspace-no-del.png</div>
        </li>
        <li>
          Two or more adjacent Hangul syllables do not overwrite the next cell.
          <div className="slot">screenshot slot: ime-03-adjacent-hangul.png</div>
        </li>
        <li>
          ASCII letters echo immediately without waiting for composition.
          <div className="slot">screenshot slot: ime-04-ascii-immediate.png</div>
        </li>
      </ol>
    </div>
  );
}
