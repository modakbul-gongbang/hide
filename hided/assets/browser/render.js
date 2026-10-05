/*
 * Ported from chromux (https://github.com/modakbul-gongbang/chromux, commit
 * 93f770f, chromux.mjs parseSnapshotText, renderSnapshotDiff,
 * renderSnapshotGrep and captureVerifyDiff) for `hide browser snapshot --diff`,
 * `--grep` and the `changed` field of an action.
 *
 * MIT License
 *
 * Copyright (c) 2026 Hoyeon Lee
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 * One function expression: (op, args) => result. It reads only its arguments,
 * so hide runs it in an isolated world where page scripts cannot observe the
 * text of other frames it is given.
 *   'diff'    {previous, current}        -> diff text (B9)
 *   'grep'    {text, pattern}            -> grep text (B10)
 *   'changes' {previous, current, ref}   -> {count, selfEchoOnly}
 *   'changed' {previous, current}        -> the bounded `changed` text (B23)
 */
((op, args) => {
  function parseSnapshotText(text) {
    const lines = String(text).split('\n');
    const title = lines[0]?.startsWith('# ') ? lines[0] : '# ';
    const url = lines[1]?.startsWith('# ') ? lines[1] : '# ';
    const body = lines.slice(2).filter(line => line.trim());
    return { title, url, body };
  }

  // Multiset line diff between the previous and current snapshot. Stable
  // @refs make unchanged elements produce identical lines, so the diff stays
  // small even on large pages. Falls back to the full snapshot when there is
  // no baseline or the page's address changed since the baseline.
  function renderSnapshotDiff(previousText, currentText) {
    if (typeof currentText !== 'string') return currentText;
    const current = parseSnapshotText(currentText);
    if (typeof previousText !== 'string') {
      return `${current.title}\n${current.url}\n# diff: no previous snapshot of this document; full snapshot shown\n\n${current.body.join('\n')}\n`;
    }
    const previous = parseSnapshotText(previousText);
    if (previous.url !== current.url) {
      return `${current.title}\n${current.url}\n# diff: url changed since previous snapshot; full snapshot shown\n\n${current.body.join('\n')}\n`;
    }
    const remaining = new Map();
    for (const line of previous.body) remaining.set(line, (remaining.get(line) || 0) + 1);
    const added = [];
    for (const line of current.body) {
      const count = remaining.get(line) || 0;
      if (count > 0) remaining.set(line, count - 1);
      else added.push(line);
    }
    const removed = [];
    for (const [line, count] of remaining) {
      for (let i = 0; i < count; i++) removed.push(line);
    }
    const unchanged = current.body.length - added.length;
    if (!added.length && !removed.length) {
      return `${current.title}\n${current.url}\n# diff: no changes since previous snapshot (${unchanged} unchanged lines omitted)\n`;
    }
    const summary = `# diff vs previous snapshot: +${added.length} added, -${removed.length} removed, ${unchanged} unchanged omitted`;
    const diffLines = [
      ...added.map(line => `+ ${line}`),
      ...removed.map(line => `- ${line}`),
    ];
    return `${current.title}\n${current.url}\n${summary}\n\n${diffLines.join('\n')}\n`;
  }

  // Filter a snapshot down to lines matching a pattern, keeping each match's
  // ancestor lines so the tree context (which form, which section) survives.
  // The pattern is tried as a case-insensitive regex first; if that matches
  // nothing (or fails to compile) it is retried as a literal substring, so
  // text like "Price (USD)" or "$50" still greps verbatim.
  function renderSnapshotGrep(text, pattern) {
    if (typeof text !== 'string') return text;
    const { title, url, body } = parseSnapshotText(text);
    const MAX_MATCHES = 100;
    const indentOf = (line) => (line.match(/^ */) || [''])[0].length;
    const collect = (re) => {
      const keep = new Set();
      const matchedIdx = new Set();
      let matches = 0;
      for (let i = 0; i < body.length; i++) {
        if (!re.test(body[i])) continue;
        matches++;
        matchedIdx.add(i);
        if (matches > MAX_MATCHES) continue;
        keep.add(i);
        let depth = indentOf(body[i]);
        for (let j = i - 1; j >= 0 && depth > 0; j--) {
          const d = indentOf(body[j]);
          if (d < depth) { keep.add(j); depth = d; }
        }
      }
      return { keep, matches, matchedIdx };
    };
    const literalRe = new RegExp(String(pattern).replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i');
    let re = literalRe;
    try { re = new RegExp(pattern, 'i'); } catch {}
    let mode = 'regex';
    let { keep, matches, matchedIdx } = collect(re);
    let literalNote = '';
    if (re.source !== literalRe.source) {
      if (!matches) {
        ({ keep, matches } = collect(literalRe));
        if (matches) mode = 'literal';
      } else {
        // Silent-wrong guard: a pattern that is a valid regex can match the
        // WRONG lines (e.g. "price (USD)" as a group) while the literal text
        // matches different ones. Whenever the literal reading matches any
        // line this regex result does NOT include, say so.
        const literal = collect(literalRe);
        const literalOnly = [...literal.matchedIdx].filter(i => !matchedIdx.has(i)).length;
        if (literalOnly > 0) {
          literalNote = `; NOTE: read as literal text this pattern also matches ${literalOnly} line${literalOnly === 1 ? '' : 's'} NOT shown here — escape regex metacharacters if you meant the literal string`;
        }
      }
    }
    if (!matches) {
      return `${title}\n${url}\n# grep ${JSON.stringify(pattern)}: 0 of ${body.length} lines matched (regex and literal); broaden the pattern or take a plain snapshot\n`;
    }
    const capNote = matches > MAX_MATCHES ? `; first ${MAX_MATCHES} shown` : '';
    const modeNote = mode === 'literal' ? '; matched literally' : '';
    const lines = [...keep].sort((a, b) => a - b).map((i) => body[i]);
    return `${title}\n${url}\n# grep ${JSON.stringify(pattern)}: ${matches} of ${body.length} lines matched (ancestor lines kept for context${modeNote}${capNote}${literalNote})\n\n${lines.join('\n')}\n`;
  }

  const changedLines = (previous, current) => renderSnapshotDiff(previous, current).split('\n')
    .filter(line => line.startsWith('+ ') || line.startsWith('- '));

  if (op === 'diff') return renderSnapshotDiff(args.previous ?? undefined, args.current);
  if (op === 'grep') return renderSnapshotGrep(args.text, args.pattern);
  if (op === 'changes') {
    // Debounced UIs (searches, validations) first echo the acted element's
    // own value and land their real update a beat later; the caller samples
    // again when that is all that changed.
    const lines = changedLines(args.previous, args.current);
    const refToken = typeof args.ref === 'string' && /^@[a-z0-9]*:?\d+$/.test(args.ref) ? args.ref + ' ' : null;
    return { count: lines.length, selfEchoOnly: Boolean(refToken) && lines.length > 0 && lines.every(line => line.includes(refToken)) };
  }
  if (op === 'changed') {
    // `changed` answers "did my action do the small thing I expected". A huge
    // diff means navigation or a churning page: summarize instead of making
    // every action on such pages expensive to read.
    const diff = renderSnapshotDiff(args.previous, args.current);
    const lines = diff.split('\n');
    const changed = lines.filter(line => line.startsWith('+ ') || line.startsWith('- '));
    if (changed.length > 40) {
      return lines.slice(0, 3).join('\n')
        + '\n' + changed.slice(0, 12).join('\n')
        + `\n# changed: large update (${changed.length} changed lines — navigation or dynamic page); showing first 12, use snapshot for the rest`;
    }
    if (diff.length > 4000) return diff.slice(0, 4000) + '\n# changed: output truncated; take a full snapshot if needed';
    return diff;
  }
  throw new Error('unknown render op ' + op);
})
