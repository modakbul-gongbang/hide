// The layout rules a design baseline states, judged against what the browser
// measured (web/e2e/sidebar-geometry.mjs). Every expected value comes from the
// baseline's `rules`, never from the code under review, so a change that
// moves the code and its own test together still fails here.

const TOLERANCE = 0.5;

const near = (a, b) => Math.abs(a - b) <= TOLERANCE;

/**
 * Child indent: in every list of agent rows, the roots share one mark column,
 * and a row at depth d starts its status mark and its title d steps of
 * `childIndentPx` right of its list's roots.
 */
export function childIndent(columns, px) {
  const problems = [];
  const steps = [];
  for (const { list, rows } of columns) {
    const roots = rows.filter(row => row.depth === 0);
    if (!roots.length) continue;
    const base = roots[0];
    for (const row of rows.filter(row => row.depth > 0)) {
      const mark = (row.mark - base.mark) / row.depth;
      steps.push(Math.round(mark * 10) / 10);
      if (!near(row.mark - base.mark, row.depth * px)) problems.push(`${list} ${row.pane}: mark is ${round(row.mark - base.mark)}px right of the roots at depth ${row.depth}, expected ${row.depth * px}px`);
      if (!near(row.title - base.title, row.depth * px)) problems.push(`${list} ${row.pane}: title is ${round(row.title - base.title)}px right of the roots at depth ${row.depth}, expected ${row.depth * px}px`);
    }
  }
  return { expected: `${px}px per child level`, measured: steps.length ? `${[...new Set(steps)].join(', ')}px per level` : 'no child rows on screen', problems, applicable: steps.length > 0 };
}

/** Every root of one list starts its mark on one column. */
export function rootsAligned(columns) {
  const problems = [];
  for (const { list, rows } of columns) {
    const roots = rows.filter(row => row.depth === 0);
    for (const row of roots.slice(1)) {
      if (!near(row.mark, roots[0].mark)) problems.push(`${list} ${row.pane}: root mark at ${round(row.mark)}, first root at ${round(roots[0].mark)}`);
    }
  }
  return { expected: 'one mark column per list', measured: problems.length ? `${problems.length} root(s) off the column` : 'aligned', problems, applicable: true };
}

/**
 * Nothing moves when one row changes state: every row keeps its box while
 * the pointer rests on, or the keyboard focuses, any single row. `after` maps
 * the row that was hovered or focused to the boxes measured then.
 */
export function stable(rest, after, action) {
  const problems = [];
  for (const [target, boxes] of Object.entries(after)) {
    for (const [key, box] of Object.entries(rest)) {
      const now = boxes[key];
      if (!now) { problems.push(`${action} ${target}: ${key} left the list`); continue; }
      for (const side of ['y', 'height', 'x', 'width']) {
        if (!near(now[side], box[side])) problems.push(`${action} ${target}: ${key} ${side} ${box[side]} -> ${now[side]}`);
      }
    }
    for (const key of Object.keys(boxes)) if (!(key in rest)) problems.push(`${action} ${target}: ${key} appeared`);
  }
  return { expected: `no row moves or resizes under ${action}`, measured: `${Object.keys(after).length} rows ${action === 'hover' ? 'hovered' : 'focused'}, ${problems.length} change(s)`, problems, applicable: Object.keys(after).length > 0 };
}

/** One column for every row's time or badge end, one for every chevron. */
export function sharedColumns({ times, chevrons }) {
  const problems = [];
  if (times.length > 1) problems.push(`times and badges end on ${times.length} columns: ${times.join(', ')}`);
  if (chevrons.length > 1) problems.push(`chevrons stand on ${chevrons.length} columns: ${chevrons.join(', ')}`);
  return { expected: 'one time column, one chevron column', measured: `${times.length} time, ${chevrons.length} chevron column(s)`, problems, applicable: true };
}

/** Parts of a row never overlap or run past it, and no text spills out of its row. */
export function noOverlap(partProblems, rowsFit) {
  const problems = [...partProblems, ...rowsFit];
  return { expected: 'no overlapping parts', measured: `${problems.length} overlap(s)`, problems, applicable: true };
}

export function noSidewaysOverflow(overflow) {
  return { expected: 'no sideways scroll or part past its row', measured: `${overflow.length} problem(s)`, problems: overflow, applicable: true };
}

/**
 * Every rule the baseline states, for one measured condition. A rule the
 * baseline does not name is not judged; a rule with nothing on screen to judge
 * says so rather than passing.
 */
export function evaluate(rules, measured) {
  const results = [];
  const add = (rule, result) => results.push({ rule, ...result, pass: result.applicable ? result.problems.length === 0 : null });
  if (typeof rules.childIndentPx === 'number') add('childIndentPx', childIndent(measured.columns, rules.childIndentPx));
  if (rules.rootsAligned) add('rootsAligned', rootsAligned(measured.columns));
  if (rules.stableUnderHover) add('stableUnderHover', stable(measured.rest, measured.hover, 'hover'));
  if (rules.stableUnderFocus) add('stableUnderFocus', stable(measured.rest, measured.focus, 'focus'));
  if (rules.noOverlap) add('noOverlap', noOverlap(measured.partProblems, measured.rowsFit));
  if (rules.sharedColumns) add('sharedColumns', sharedColumns(measured.columnsAtEnd));
  if (rules.noSidewaysOverflow) add('noSidewaysOverflow', noSidewaysOverflow(measured.overflow));
  const unknown = Object.keys(rules).filter(rule => !KNOWN.has(rule));
  for (const rule of unknown) results.push({ rule, pass: null, applicable: false, expected: String(rules[rule]), measured: 'this command does not know how to measure this rule', problems: [] });
  return results;
}

const KNOWN = new Set(['childIndentPx', 'rootsAligned', 'stableUnderHover', 'stableUnderFocus', 'noOverlap', 'sharedColumns', 'noSidewaysOverflow']);

function round(value) {
  return Math.round(value * 10) / 10;
}
