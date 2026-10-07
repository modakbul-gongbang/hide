// Links in a terminal pane's text, whatever program printed them.
//
// A program can mark a link itself (OSC 8), but most print URLs and paths as
// plain text, so the rows are read when xterm asks about the row under the
// pointer: an http(s) URL, or a path-shaped token that becomes a link only
// once the filesystem says it exists (`terminalLinkProvider.ts` asks). Herdr's
// frames carry no wrap flag, so a token cut at a row's end may continue on
// the next row: at the terminal's edge (the continuation `selection.ts` reads
// for a copy) or at a TUI's own margin, after the indentation it starts its
// wrapped line with; a row that ends short followed by one that starts at the
// left edge is two lines, as `ls` prints them. Every way to join the pieces
// is offered longest first, and the existence check decides which spelling
// is real; a URL cannot be checked, so it joins only at the terminal's edge.

import { rowContinues, type CellRow } from "./selection";

/** Rows above and below the hovered one a joined token may reach. */
export const JOIN_ROWS = 3;

/** Longer than any macOS path (`PATH_MAX` is 1024). */
const MAX_TEXT = 2048;

/** A piece of a token: its row in the rows handed in, and its columns (zero-based, end exclusive). */
export type Span = { row: number; start: number; end: number };

export type LinkTarget =
  | { kind: "url"; url: string }
  /** `path` as written: absolute, `~/`-relative, or relative to the pane's working folder. */
  | { kind: "path"; path: string; line: number | null; column: number | null };

export type LinkCandidate = { spans: Span[]; text: string; target: LinkTarget; original: boolean };

/**
 * Characters that end a token besides whitespace: the frames TUIs draw around
 * text (box drawing, blocks), the marks they put before a line (arrows,
 * bullets, `⎿`, `⏺`, check marks). `…` is not one, so a path a program
 * shortened is one token the parser refuses rather than a wrong path.
 */
const SEPARATOR = /[\s←-⇿─-▟⎿⏺•○●◦▪▶►✓✔✗✘]/u;

type Run = { row: number; start: number; end: number; text: string; first: boolean; last: boolean };

/** The row's tokens with their columns; a wide glyph's trailing half belongs to the glyph. */
function rowRuns(cells: CellRow, row: number): Run[] {
  const runs: Run[] = [];
  let text = "";
  let start = -1;
  let end = -1;
  const close = () => {
    if (start >= 0) runs.push({ row, start, end, text, first: false, last: false });
    text = "";
    start = -1;
  };
  for (let column = 0; column < cells.length; column += 1) {
    const chars = cells[column];
    if (chars === null) {
      if (start >= 0) end = column + 1;
      continue;
    }
    if (chars === undefined || chars === "" || SEPARATOR.test(chars)) {
      close();
      continue;
    }
    if (start < 0) start = column;
    text += chars;
    end = column + 1;
  }
  close();
  if (runs.length > 0) {
    runs[0]!.first = true;
    runs[runs.length - 1]!.last = true;
  }
  return runs;
}

/**
 * Every link that may cover row `at`, one group per token on that row. A
 * group's candidates are the ways to join that token with its neighbours on
 * the rows around it, longest first; the caller takes the first that holds.
 */
export function linkCandidates(rows: CellRow[], at: number): LinkCandidate[][] {
  const runsByRow = new Map<number, Run[]>();
  const runsOf = (row: number) => {
    let runs = runsByRow.get(row);
    if (!runs) {
      const cells = rows[row];
      runs = cells ? rowRuns(cells, row) : [];
      runsByRow.set(row, runs);
    }
    return runs;
  };
  const groups: LinkCandidate[][] = [];
  for (const run of runsOf(at)) {
    // The chain of pieces this token may be part of: the previous row's last
    // token while this is its row's first, and the next row's first while
    // this is its row's last.
    const before: Run[] = [];
    for (let piece = run, row = at - 1; piece.first && row >= Math.max(0, at - JOIN_ROWS); row -= 1) {
      const previous = runsOf(row).at(-1);
      if (!previous) break;
      before.unshift(previous);
      piece = previous;
    }
    const after: Run[] = [];
    for (let piece = run, row = at + 1; piece.last && row <= Math.min(rows.length - 1, at + JOIN_ROWS); row += 1) {
      const next = runsOf(row)[0];
      if (!next) break;
      after.push(next);
      piece = next;
    }
    const chain = [...before, run, ...after];
    const own = before.length;
    const group: LinkCandidate[] = [];
    for (let from = 0; from <= own; from += 1) {
      for (let to = own; to < chain.length; to += 1) {
        const pieces = chain.slice(from, to + 1);
        group.push(...joined(rows, pieces));
      }
    }
    group.sort((a, b) => spellingLength(b) - spellingLength(a) || b.text.length - a.text.length);
    if (group.length > 0) groups.push(group);
  }
  return groups;
}

/** Location text is clickable too, but precedence compares the actual path spelling. */
function spellingLength(candidate: LinkCandidate): number {
  return candidate.target.kind === "path" ? candidate.target.path.length : candidate.text.length;
}

function joined(rows: CellRow[], pieces: Run[]): LinkCandidate[] {
  const raw = pieces.map((piece) => piece.text).join("");
  if (raw.length > MAX_TEXT) return [];
  const breaks = pieces.slice(0, -1).map((piece, index) => (rowContinues(rows[piece.row] ?? []) ? "edge" : pieces[index + 1]!.start > 0 ? "margin" : null));
  if (breaks.includes(null)) return [];
  return interpretations(raw).flatMap((parsed) => {
    // URLs retain their existing parser and edge-only wrap rule.
    if (parsed.target.kind === "url" && breaks.includes("margin")) return [];
    const spans = trimSpans(rows, pieces, parsed.lead, parsed.trail);
    return spans ? [{ spans, text: parsed.text, target: parsed.target, original: parsed.original }] : [];
  });
}

/**
 * Map UTF-16 slice boundaries back to the buffer cells that supplied them.
 * One cell may hold a surrogate pair or a combining sequence, and null cells
 * extend its preceding wide glyph. Never cut either kind of glyph in half.
 */
function trimSpans(rows: CellRow[], pieces: Run[], lead: number, trail: number): Span[] | null {
  const end = pieces.reduce((length, piece) => length + piece.text.length, 0) - trail;
  const spans: Span[] = [];
  let offset = 0;
  for (const piece of pieces) {
    const cells = rows[piece.row] ?? [];
    for (let column = piece.start; column < piece.end; column += 1) {
      const chars = cells[column];
      if (!chars) continue;
      let next = column + 1;
      while (next < piece.end && cells[next] === null) next += 1;
      const after = offset + chars.length;
      if ((lead > offset && lead < after) || (end > offset && end < after)) return null;
      if (offset >= lead && after <= end) {
        const last = spans.at(-1);
        if (last?.row === piece.row && last.end === column) last.end = next;
        else spans.push({ row: piece.row, start: column, end: next });
      }
      offset = after;
    }
  }
  return spans.length > 0 ? spans : null;
}

const OPENERS = "([{<\"'`";
const CLOSERS = ")]}>\"'`.,;:!?";
const PAIRS: Record<string, string> = { ")": "(", "]": "[", "}": "{", ">": "<" };

/** Removes wrapping punctuation; a closing bracket stays when the token opened it. */
function strip(token: string): { text: string; lead: number; trail: number } {
  let text = token;
  let lead = 0;
  let trail = 0;
  // `Update(web/src/a.ts)`: a tool header names its argument in parentheses.
  const call = /^[A-Za-z][\w-]*\((.+)\)$/u.exec(text);
  if (call?.[1]) {
    lead += text.length - call[1].length - 1;
    trail += 1;
    text = call[1];
  }
  while (text.length > 0 && OPENERS.includes(text[0]!)) {
    text = text.slice(1);
    lead += 1;
  }
  while (text.length > 0 && CLOSERS.includes(text[text.length - 1]!)) {
    const closer = text[text.length - 1]!;
    const opener = PAIRS[closer];
    if (opener && count(text, opener) >= count(text, closer)) break;
    text = text.slice(0, -1);
    trail += 1;
  }
  return { text, lead, trail };
}

function count(text: string, char: string): number {
  let found = 0;
  for (const each of text) if (each === char) found += 1;
  return found;
}

/** `path:12`, `path:12:5`, `path:12-20`, `path#L12`, `path#L12-L20`, `path(12,5)`. */
const LOCATION = /^(.+?)(?::(\d+)(?::(\d+))?(?:-\d+)?|#L(\d+)(?:-L?\d+)?|\((\d+)(?:,\s*(\d+))?\))$/u;

/** A bare file name: a name with a letter in it and an extension that starts with one. */
const BARE_NAME = /^[\p{L}\p{N}_@+-][\p{L}\p{N}_.@+-]*\.[A-Za-z][A-Za-z0-9]{0,9}$/u;
const DOTFILE = /^\.[A-Za-z][\w.-]*$/u;

type Interpretation = { text: string; lead: number; trail: number; target: LinkTarget; original: boolean };

// A finite grammar boundary, never a loop that truncates arbitrary Hangul.
// A base particle may take one of the four listed secondary particles.
const GRAMMAR = /[)\]}>"'`](?:에서|에게|으로|부터|까지|에|께|로|와|과|을|를|은|는|이|가|의|도|만)(?:도|만|는|은)?$/u;

/** The text up to and including the closing mark a Korean particle follows, or null when none does. */
function withoutParticle(text: string): string | null {
  const match = GRAMMAR.exec(text);
  return match ? text.slice(0, match.index + 1) : null;
}

/** Six finite stages preserve literal punctuation and location before interpreting either. */
function interpretations(token: string): Interpretation[] {
  const symbols = strip(token);
  const ordinary = parseTarget(symbols.text);
  // URI schemes follow the existing parser; never probe a URL as a local path.
  if (ordinary?.kind === "url" || uriTarget(symbols.text) !== undefined) {
    const parsed = parseToken(token);
    return parsed ? [{ ...parsed, original: true }] : [];
  }
  // Grammar-only removal preserves both leading and closing punctuation.
  // Apply it separately to raw and symbol-only spelling, never one character
  // at a time, so each literal stage can beat a shorter interpretation.
  // Existing outer sentence punctuation is context, but raw leading symbols
  // remain part of this grammar-only literal stage.
  const rawGrammarText = withoutParticle(token.slice(0, token.length - symbols.trail));
  const grammarText = withoutParticle(symbols.text);
  const grammar = grammarText === null ? { text: symbols.text, lead: 0, trail: 0 } : strip(grammarText);
  const interpreted = parseTarget(grammar.text);
  const result: Interpretation[] = [];
  const add = (text: string, lead: number, trail: number, target: LinkTarget | null, original: boolean) => {
    if (!target || result.some((each) => each.text === text && JSON.stringify(each.target) === JSON.stringify(target))) return;
    result.push({ text, lead, trail, target, original });
  };
  const literal = (text: string): LinkTarget | null => {
    // A recognized path's punctuation/grammar/location may be its real name.
    // Validation still rejects arbitrary schemes, controls and shortened paths.
    if (!interpreted || interpreted.kind !== "path" || text.length > 1024 || text.includes("…") || /\p{Cc}/u.test(text)) return null;
    if (/^[A-Za-z][\w+-]*:/u.test(text) && !LOCATION.test(text)) return null;
    return { kind: "path", path: text, line: null, column: null };
  };
  add(token, 0, 0, literal(token), true);
  add(symbols.text, symbols.lead, symbols.trail, literal(symbols.text), false);
  if (rawGrammarText !== null) add(rawGrammarText, 0, token.length - rawGrammarText.length, literal(rawGrammarText), false);
  if (grammarText !== null) add(grammarText, symbols.lead, token.length - symbols.lead - grammarText.length, literal(grammarText), false);
  const lead = symbols.lead + grammar.lead;
  const trail = token.length - lead - grammar.text.length;
  add(grammar.text, lead, trail, literal(grammar.text), false);
  add(grammar.text, lead, trail, interpreted, false);
  return result;
}

/** What a whitespace-free token links to, with how much of it the link leaves out at either end. */
export function parseToken(token: string): { text: string; lead: number; trail: number; target: LinkTarget } | null {
  const symbols = strip(token);
  if (symbols.text.length === 0) return null;
  const target = parseTarget(symbols.text);
  if (target?.kind !== "url") return target ? { ...symbols, target } : null;
  // A URL cannot be checked the way a path is, so a particle after its
  // closing mark is always prose: `(https://x/pull/682)이` links the address.
  const prose = withoutParticle(symbols.text);
  if (prose === null) return { ...symbols, target };
  const bare = strip(prose);
  const url = parseTarget(bare.text);
  const lead = symbols.lead + bare.lead;
  return url ? { text: bare.text, lead, trail: token.length - lead - bare.text.length, target: url } : null;
}

function parseTarget(text: string): LinkTarget | null {
  if (/^https?:\/\/\S+$/iu.test(text)) {
    try {
      return { kind: "url", url: new URL(text).href };
    } catch {
      return null;
    }
  }
  const uri = uriTarget(text);
  if (uri !== undefined) return uri;
  const location = LOCATION.exec(text);
  const path = location ? location[1]! : text;
  const line = location ? Number(location[2] ?? location[4] ?? location[5]) : null;
  const column = location && (location[3] ?? location[6]) ? Number(location[3] ?? location[6]) : null;
  if (!pathShaped(path)) return null;
  return { kind: "path", path, line: line && line > 0 ? line : null, column: column && column > 0 ? column : null };
}

function pathShaped(path: string): boolean {
  if (path.length > 1024 || path.includes("…") || path.includes(":") || /\p{Cc}/u.test(path)) return false;
  if (path === "/" || path.startsWith("//")) return false;
  if (path.startsWith("/") || path.startsWith("~/") || path.startsWith("./") || path.startsWith("../")) return true;
  if (path.includes("/")) return /[\p{L}]/u.test(path);
  return DOTFILE.test(path) || (BARE_NAME.test(path) && /\p{L}/u.test(path.slice(0, path.lastIndexOf("."))));
}

/**
 * A link a program marked with OSC 8, or `undefined` when the address is not
 * one a pane link follows. `file://` and the `vscode://file/` links Codex
 * writes for its file citations are paths; http(s) is a URL.
 */
export function osc8Target(uri: string): LinkTarget | null | undefined {
  if (/^https?:\/\//iu.test(uri)) {
    try {
      return { kind: "url", url: new URL(uri).href };
    } catch {
      return null;
    }
  }
  return uriTarget(uri);
}

/** A `file:` or `vscode://file/` address as a path; `undefined` for any other text. */
function uriTarget(text: string): LinkTarget | null | undefined {
  let rest: string;
  if (/^file:\/\//iu.test(text)) {
    try {
      const url = new URL(text);
      if (url.host !== "" && url.host !== "localhost") return null;
      rest = decodeURIComponent(url.pathname) + (url.hash.startsWith("#L") ? url.hash : "");
    } catch {
      return null;
    }
  } else if (/^vscode:\/\/file\//iu.test(text)) {
    try {
      rest = decodeURIComponent(text.slice("vscode://file".length));
    } catch {
      return null;
    }
  } else {
    return undefined;
  }
  const location = LOCATION.exec(rest);
  const path = location ? location[1]! : rest;
  if (!path.startsWith("/") || path.includes("\0")) return null;
  const line = location ? Number(location[2] ?? location[4] ?? location[5]) : null;
  const column = location && (location[3] ?? location[6]) ? Number(location[3] ?? location[6]) : null;
  return { kind: "path", path, line: line && line > 0 ? line : null, column: column && column > 0 ? column : null };
}
