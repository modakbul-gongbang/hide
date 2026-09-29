import type { Subtree, SubtreeRow, SubtreeState } from "../close";
import { markTone } from "../agentRow";
import { DeviceChip } from "./device-chip";
import { StatusMark } from "./status-mark";
import { Hint } from "./ui/tooltip";

// The descendants a close or a removal would take with it (PRD
// close-agent-subtree D-17, D-32, D-33, D-42): a summary line of what needs
// the operator, then the tree, each row as the sidebar draws it. Colour is the
// status marks' alone. A row that is working, waiting for an answer, holding
// an unread result or unreadable is bright and says its state in a neutral
// word; a quiet one is dimmed, and its mark's tooltip and the row's
// accessible name carry its state.

// The close sheet speaks Korean; a removal dialog keeps its own English (D-35).
export type SubtreeWords = "ko" | "en";

const SUMMARY: { state: Exclude<SubtreeState, "quiet">; symbol: string; tone: string; word: Record<SubtreeWords, string> }[] = [
  { state: "working", symbol: "●", tone: "text-agent-working", word: { ko: "진행 중", en: "working" } },
  { state: "waiting", symbol: "?", tone: "text-warning", word: { ko: "답 대기", en: "waiting for you" } },
  { state: "unread", symbol: "✓", tone: "text-success", word: { ko: "확인 안 한 결과", en: "unread result" } },
  { state: "unknown", symbol: "~", tone: "text-subtle-foreground", word: { ko: "상태 모름", en: "status unknown" } },
];

const LIST_LABEL: Record<SubtreeWords, string> = { ko: "함께 닫히는 에이전트", en: "Agents outside" };

/** The summary in words, zero kinds left out, for a screen reader and a test. */
export function subtreeSummaryWords(counts: Subtree["counts"], words: SubtreeWords = "ko"): string {
  return SUMMARY.filter(({ state }) => counts[state] > 0)
    .map(({ state, word }) => `${word[words]} ${counts[state]}`)
    .join(" · ");
}

export function SubtreeList({ subtree, targetDevice, words = "ko" }: { subtree: Subtree; targetDevice: string | undefined; words?: SubtreeWords }) {
  const parts = SUMMARY.filter(({ state }) => subtree.counts[state] > 0);
  return (
    <div className="flex min-w-0 flex-col gap-xs" data-subtree-list="true">
      {parts.length > 0 ? (
        <p className="flex flex-wrap items-center gap-x-sm gap-y-xxs text-caption text-subtle-foreground" data-subtree-summary={subtreeSummaryWords(subtree.counts, words)}>
          {parts.map(({ state, symbol, tone, word }) => (
            <span key={state} className="inline-flex items-center gap-xxs" data-subtree-count={state}>
              <StatusMark symbol={symbol} className={tone} />
              <span>
                {word[words]} {subtree.counts[state]}
              </span>
            </span>
          ))}
        </p>
      ) : null}
      <ul className="flex max-h-(--size-relationship-list-max) min-w-0 flex-col gap-xxs overflow-y-auto rounded-md border border-border p-xs" aria-label={LIST_LABEL[words]}>
        {subtree.rows.map((row) => (
          <SubtreeItem key={row.agent.pane_id} row={row} targetDevice={targetDevice} />
        ))}
      </ul>
    </div>
  );
}

function SubtreeItem({ row, targetDevice }: { row: SubtreeRow; targetDevice: string | undefined }) {
  const { agent } = row;
  const quiet = row.state === "quiet";
  const bright = row.target || !quiet;
  const device = agent.device_id !== targetDevice && agent.device_label ? agent.device_label : null;
  return (
    <li
      tabIndex={0}
      aria-label={[agent.identity_label, device, agent.status_label].filter(Boolean).join(", ")}
      data-subtree-row={agent.pane_id}
      data-subtree-state={row.state}
      data-subtree-target={row.target ? "true" : undefined}
      className={`flex min-w-0 items-start gap-xs rounded-xs px-xs py-xxs outline-none focus-visible:ring-2 focus-visible:ring-ring ${bright ? "" : "opacity-(--opacity-read-status)"}`}
      style={{ paddingInlineStart: `calc(var(--spacing-xs) + var(--spacing-lg) * ${row.depth})` }}
    >
      <RowMark symbol={agent.symbol} tone={markTone(agent)} status={quiet ? agent.status_label : null} />
      <span className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-xs gap-y-xxs" aria-hidden="true">
        <span className={`min-w-0 break-words ${row.target ? "font-medium text-foreground" : "text-foreground"}`}>{agent.identity_label}</span>
        {quiet ? null : <span className="shrink-0 text-caption text-subtle-foreground" data-subtree-status="true">{agent.status_label}</span>}
        {device ? <DeviceChip label={device} className="max-w-full" /> : null}
      </span>
    </li>
  );
}

/**
 * A close list row's status mark. A quiet row shows no status word, so its
 * mark says the state on hover (D-42); the row's accessible name says it too.
 */
export function RowMark({ symbol, tone, status }: { symbol: string; tone: string; status: string | null }) {
  const mark = <StatusMark symbol={symbol} className={`mt-xxs ${tone}`} />;
  if (!status) return mark;
  return (
    <Hint label={status} reveals>
      <span className="inline-flex shrink-0" data-row-mark-hint={status}>
        {mark}
      </span>
    </Hint>
  );
}
