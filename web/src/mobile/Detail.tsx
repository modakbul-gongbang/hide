// One agent's detail on the phone (PRD mobile-companion B24-B28, D-18): the
// row's head, the pane's recent rows read-only in the terminal's colours with
// the newest at the bottom, a pull to the top for older rows, and the quick
// keys and one-line reply every agent has, whatever its group.

import { AnsiUp } from "ansi_up";
import { ArrowLeftIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Button } from "../components/ui/button";
import { AgentHead, Place } from "./parts";
import { loadMore, onReplyResult, sendKey, sendReply } from "./connection";
import { MAX_REPLY_CHARS, QUICK_KEYS, UNREACHABLE_TEXT, rowsProblem } from "./protocol";
import { agentOf, usePhone } from "./store";

const PULL_EDGE = 48;

function useAnsi() {
  return useMemo(() => {
    const ansi = new AnsiUp();
    ansi.use_classes = true;
    // Read-only rows: an OSC 8 link stays text, nothing on the page navigates away.
    ansi.url_allowlist = {};
    return ansi;
  }, []);
}

export function Detail({ onBack }: { onBack: () => void }) {
  const detail = usePhone((s) => s.detail);
  const groups = usePhone((s) => s.groups);
  const connected = usePhone((s) => s.connected);
  const unreachable = usePhone((s) => s.unreachable);
  const agent = agentOf(groups, detail?.key ?? null);
  const rows = detail?.rows ?? null;
  const problem = rows ? rowsProblem(rows.state) : null;
  // A pane that left the list is gone for the reply bar too (B28).
  const gone = rows?.state === "gone" || (groups !== null && !agent);
  return (
    <main className="flex h-full flex-col" data-phone-detail={detail ? `${detail.key.device_id}|${detail.key.pane_id}` : ""}>
      <header className="phone-safe-top shrink-0 border-b border-border bg-card px-lg pb-md">
        <div className="flex items-center justify-between pt-sm">
          <button type="button" onClick={onBack} className="-ml-sm flex min-h-(--size-touch-target) items-center gap-xs px-sm text-title text-foreground" data-phone-back="true">
            <ArrowLeftIcon aria-hidden="true" className="size-(--size-icon-lg)" />
            목록
          </button>
          {agent ? <span className="font-mono text-body text-muted-foreground">{agent.elapsed}</span> : null}
        </div>
        {agent ? (
          <div className="mt-xs flex flex-col gap-xs">
            <AgentHead agent={agent} large />
            <Place agent={agent} />
          </div>
        ) : null}
      </header>
      {unreachable && !connected ? (
        <div role="status" className="shrink-0 bg-secondary px-lg py-sm text-subhead text-subtle-foreground" data-phone-unreachable="true">
          {UNREACHABLE_TEXT}
        </div>
      ) : null}
      {problem ? (
        <p role="status" className="shrink-0 px-lg py-sm text-subhead text-warning" data-phone-rows-state={rows?.state}>
          {problem}
        </p>
      ) : gone && !rows ? (
        <p role="status" className="shrink-0 px-lg py-sm text-subhead text-warning" data-phone-rows-state="gone">
          {rowsProblem("gone")}
        </p>
      ) : null}
      <Scrollback text={rows?.text ?? ""} more={rows?.more ?? false} lines={rows?.lines ?? 0} loading={!rows} />
      <ReplyBar disabled={gone || !connected} />
    </main>
  );
}

function Scrollback({ text, more, lines, loading }: { text: string; more: boolean; lines: number; loading: boolean }) {
  const ansi = useAnsi();
  const html = useMemo(() => ansi.ansi_to_html(text), [ansi, text]);
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const heightBefore = useRef(0);
  const linesBefore = useRef(0);
  // Keep the newest row in view while pinned to the bottom; when older rows
  // arrive on top, keep the row the reader was on where it was (B24, B25).
  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    if (pinned.current) element.scrollTop = element.scrollHeight;
    else if (lines > linesBefore.current) element.scrollTop += element.scrollHeight - heightBefore.current;
    heightBefore.current = element.scrollHeight;
    linesBefore.current = lines;
  }, [html, lines]);
  return (
    <div
      ref={scroller}
      className="min-h-0 flex-1 overflow-auto overscroll-contain bg-background px-lg py-md"
      data-phone-scrollback={lines}
      onScroll={(event) => {
        const element = event.currentTarget;
        pinned.current = element.scrollHeight - element.scrollTop - element.clientHeight < PULL_EDGE;
        heightBefore.current = element.scrollHeight;
        if (element.scrollTop < PULL_EDGE && more) loadMore();
      }}
    >
      {more ? <p className="pb-sm text-center text-body text-muted-foreground">위로 당기면 더 불러와요</p> : null}
      {loading ? <p className="text-body text-muted-foreground">불러오는 중…</p> : null}
      <pre
        aria-label="터미널 최근 출력"
        className="phone-ansi m-none whitespace-pre font-mono text-body leading-normal text-foreground"
        // ansi_up escapes the text; it adds only its own colour spans.
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </div>
  );
}

function ReplyBar({ disabled }: { disabled: boolean }) {
  const [text, setText] = useState("");
  const pending = usePhone((s) => s.pendingInput);
  const error = usePhone((s) => s.inputError);
  useEffect(
    () =>
      onReplyResult((ok) => {
        if (ok) setText("");
      }),
    [],
  );
  const tooLong = [...text].length > MAX_REPLY_CHARS;
  const blocked = disabled || pending !== null;
  return (
    <footer className="phone-safe-bottom shrink-0 border-t border-border bg-card px-lg pt-md">
      <div className="grid grid-cols-5 gap-sm" role="group" aria-label="퀵키">
        {QUICK_KEYS.map((quick) => (
          <Button
            key={quick.key}
            variant="secondary"
            aria-label={quick.name}
            disabled={blocked}
            data-phone-key={quick.key}
            className="h-(--size-touch-target) rounded-lg font-mono text-title"
            onClick={() => sendKey(quick.key)}
          >
            {quick.label}
          </Button>
        ))}
      </div>
      <form
        className="mt-sm flex gap-sm pb-md"
        onSubmit={(event) => {
          event.preventDefault();
          sendReply(text);
        }}
      >
        <input
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder="답장..."
          aria-label="답장"
          disabled={disabled}
          enterKeyHint="send"
          autoComplete="off"
          autoCorrect="off"
          data-phone-reply="true"
          aria-invalid={tooLong || undefined}
          className="h-(--size-touch-target) min-w-0 flex-1 rounded-lg border border-border bg-input px-md text-title text-foreground outline-none placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
        />
        <Button type="submit" disabled={blocked || text.trim().length === 0 || tooLong} className="h-(--size-touch-target) rounded-lg px-lg text-title font-semibold" data-phone-send="true">
          보내기
        </Button>
      </form>
      {tooLong || error ? (
        <p role="alert" className="-mt-sm pb-md text-body text-destructive" data-phone-input-error="true">
          {tooLong ? `답장은 한 번에 ${MAX_REPLY_CHARS.toLocaleString("ko-KR")}자까지 보낼 수 있어요.` : error}
        </p>
      ) : null}
    </footer>
  );
}
