// The children of `Screen / Disk Cleanup` (PRD disk-layers B1-B28): the Overview's
// disk entry, the cleanup sheet as a checkout x layer table with its checkboxes,
// the states the table carries, the confirm step, the running step and the result.
// Drawn on this document's local tokens plus library refs (Checkbox, Button), the
// way every other Screen sheet is, and called from pen-screens.mjs, which owns the
// ref helpers (`themedXref`, `screenButton`) and the sheet frame. Every name,
// path and number is invented mock content, never a value the shell could read.

import {frame, icon, num, text} from './pen-system.mjs';

export function diskCleanupRows(tokens, {themedXref, screenButton, screenDialogSurface}, s) {
  const HAIR = num(tokens, '--size-hairline');
  const DIM = num(tokens, '--opacity-dimmed');
  const SECONDARY = num(tokens, '--opacity-secondary');
  const DISABLED = num(tokens, '--opacity-disabled');
  const BODY = num(tokens, '--text-body');
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);
  const rule = (id, width = 'fill_container') => frame(id, 'Rule', {width, height: HAIR, fill: '$--border'}, []);

  // The shell's own size wording (projectBoard.ts formatBytes): one decimal under 10, none above.
  const GB = 1024 ** 3;
  function fmt(gb) {
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let value = gb * GB;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
    if (unit === 0) return `${Math.trunc(value)} B`;
    return `${value < 10 ? value.toFixed(1) : value.toFixed(0)} ${units[unit]}`;
  }

  // Pen draws no ellipsis, so a name the web truncates is written already cut.
  function textWidth(content, size, mono = false) {
    let width = 0;
    for (const character of content) {
      const code = character.codePointAt(0);
      if ((code >= 0xac00 && code <= 0xd7a3) || (code >= 0x3130 && code <= 0x318f)) width += size * 0.93;
      else if (mono) width += size * 0.6;
      else if (character === ' ') width += size * 0.28;
      else if (/[A-Z#@%MW]/.test(character)) width += size * 0.68;
      else if (/[il.,:;'|!/]/.test(character)) width += size * 0.28;
      else width += size * 0.55;
    }
    return width;
  }
  function fitText(content, max, size, mono = false) {
    if (textWidth(content, size, mono) <= max) return content;
    let cut = content;
    while (cut.length && textWidth(`${cut}…`, size, mono) > max) cut = cut.slice(0, -1);
    return `${cut.trimEnd()}…`;
  }

  // -- the model (web/src/diskCleanup.ts) ----------------------------------------------

  const LAYERS = {
    build_cache: {label: '빌드 캐시', glyph: 'hammer', dot: '$--accent-choice-amber'},
    dependencies: {label: '의존성', glyph: 'package', dot: '$--file-blue'},
    worktree: {label: '워크트리', glyph: 'folder-git-2'},
    other: {label: '기타', dot: '$--file-yellow'},
    source: {label: '워크트리 소스', dot: '$--file-neutral'},
    shared_git: {label: '공유 Git', dot: '$--file-neutral'},
  };
  const PR_TONE = {open: '$--pr-open', merged: '$--pr-merged', closed: '$--pr-closed'};
  const OTHER_TEXT = 'hide가 모르는 폴더라 지우지 않는다';

  // inUse: the words inUseText gives; tree: 'ok', or the reason the worktree cell is a lock; sizes in GB
  const CHECKOUTS = [
    {id: 'main', name: 'main', main: true, build: 4.9, deps: 0.64, other: 2.6, source: 0.6},
    {id: 'rwb', name: 'fix/remote-workspace-bridge', pr: [227, 'merged'], build: 7.3, deps: 0.63, other: 0.005, source: 0.6, tree: 'ok'},
    {id: 'oif', name: 'feat/overview-issue-first', pr: [218, 'closed'], build: 5.9, deps: 0.32, other: 0, source: 0.6, tree: 'main에 머지되지 않음'},
    {id: 'mc', name: 'mobile-conversation', pr: [252, 'open'], build: 4.1, deps: 0.32, other: 0, source: 0.6, inUse: '에이전트 작업 중', tree: '에이전트 작업 중'},
    {id: 'vt', name: 'feat/vite-dev-server', build: 3.9, deps: 0.32, other: 0, source: 0.6, inUse: '터미널에서 vite 실행 중', tree: '터미널에서 vite 실행 중'},
    {id: 'pfs', name: 'fix/pane-find-scroll', build: 3.6, deps: 0.32, other: 0.1, source: 0.6, tree: '바뀐 파일 3'},
    {id: 'ol', name: 'feat/overview-lenses', pr: [238, 'merged'], build: 3.4, deps: 0.32, other: 0, source: 0.6, tree: 'ok'},
    {id: 'crl', name: 'ci/runner-layout', pr: [248, 'merged'], build: 2.4, deps: 0.32, other: 0, source: 0.6, tree: 'ok'},
    {id: 'atg', name: 'prd/agent-tab-groups', pr: [217, 'merged'], build: 0, deps: 0.32, other: 0.82, source: 0.6, tree: 'ok'},
  ].map(checkout => ({...checkout, total: checkout.build + checkout.deps + checkout.other + checkout.source}));
  const byId = Object.fromEntries(CHECKOUTS.map(checkout => [checkout.id, checkout]));
  const FILTERS = [['전체', 36], ['끝난 것', 19], ['쉬는 것', 11], ['작업 중', 6]];

  const cacheBlocked = checkout => Boolean(checkout.inUse);

  // -- parts -------------------------------------------------------------------------

  // The library Checkbox: on, off, mixed (its Indeterminate), included (its Checked Disabled).
  function checkbox(id, state = 'off', {disabled = false} = {}) {
    const filled = state !== 'off';
    const overrides = {
      ...(filled ? {fill: '$--primary', stroke: '$--primary', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'} : {}),
      ...(disabled || state === 'included' ? {opacity: DISABLED} : {}),
    };
    return themedXref(id, 'chk-m', `Checkbox ${state}`, overrides, {
      'chk-i': {enabled: state === 'on' || state === 'included'},
      'chk-bar': {enabled: state === 'mixed'},
    });
  }

  const dot = (id, fill) => frame(id, 'Swatch', {width: 8, height: 8, cornerRadius: 4, fill}, []);
  const tooltip = (id, content) => frame(id, 'Tooltip', {padding: ['$--spacing-xxs', '$--spacing-sm'], fill: '$--popover', cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
    text(`${id}-t`, content, {size: '$--text-caption'}),
  ]);

  // The usage bar: the layers, and one grey part for the source and the shared Git.
  const USAGE = [['build_cache', 38.2], ['dependencies', 7.9], ['other', 3.9], ['source', 6.4]];
  const USAGE_TOTAL = USAGE.reduce((sum, [, gb]) => sum + gb, 0);
  const USAGE_LABEL = {build_cache: '빌드 캐시', dependencies: '의존성', other: '기타', source: '소스 · .git'};

  function usageBar(id, width, {partial = false, free = 1.6} = {}) {
    const shown = USAGE.filter(([, gb]) => gb > 0);
    const height = 6;
    return frame(id, 'Usage', {layout: 'vertical', gap: '$--spacing-xs', width}, [
      frame(`${id}-top`, 'Numbers', {layout: 'horizontal', alignItems: 'baseline', gap: '$--spacing-md', width}, [
        text(`${id}-p`, `이 프로젝트 ${partial ? '≥ ' : ''}${fmt(USAGE_TOTAL)}`, {size: '$--text-caption', fill: '$--subtle-foreground', mono: true}),
        spacer(`${id}-sp`),
        text(`${id}-f`, `디스크 여유 ${fmt(free)}`, {size: '$--text-caption', fill: '$--subtle-foreground', mono: true}),
      ]),
      frame(`${id}-bar`, 'Bar', {layout: 'horizontal', gap: 1, width, height, cornerRadius: 2, clip: true, fill: '$--muted'},
        shown.map(([key, gb], index) => frame(`${id}-bar${index}`, LAYERS[key].label, {width: Math.max(2, Math.round((width - shown.length) * gb / USAGE_TOTAL)), height, fill: LAYERS[key].dot ?? '$--file-neutral'}, []))),
    ]);
  }

  function dialog(id, {title, width, body, footer}) {
    return frame(id, 'Dialog', {layout: 'vertical', width, fill: '$--popover', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', clip: true}, [
      frame(`${id}-h`, 'Header', {layout: 'horizontal', alignItems: 'start', gap: '$--spacing-md', width, padding: ['$--spacing-lg', '$--spacing-lg', '$--spacing-sm', '$--spacing-lg']}, [
        frame(`${id}-ht`, 'Title', {layout: 'vertical', gap: '$--spacing-xxs'}, [
          text(`${id}-title`, title, {size: '$--text-title', weight: '600'}),
        ]),
        spacer(`${id}-hs`),
        icon(`${id}-x`, 'x', {size: 16, fill: '$--muted-foreground'}),
      ]),
      frame(`${id}-b`, 'Body', {layout: 'vertical', gap: '$--spacing-md', width, padding: ['$--spacing-md', '$--spacing-lg']}, body),
      ...(footer ? [rule(`${id}-fr`, width), frame(`${id}-f`, 'Footer', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'start', width, padding: ['$--spacing-md', '$--spacing-lg']}, footer)] : []),
    ]);
  }

  // -- entry: the Overview's facts line (DiskEntrance.tsx) ---------------------------------

  function fact(id, glyph, label, {fill = '$--subtle-foreground', underline = false} = {}) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', ...(underline ? {padding: [0, 0, 1, 0], stroke: fill, strokeWidth: {bottom: HAIR}} : {})}, [
      icon(`${id}-g`, glyph, {size: 12, fill}),
      text(`${id}-t`, label, {size: '$--text-caption', fill, mono: true}),
    ]);
  }

  // Lines in the app's order: caches, the source of the worktrees, other, the shared Git.
  function breakdown(id, size, partial) {
    const lines = [['build_cache', '빌드 캐시', 38.2], ['dependencies', '의존성', 7.9], ['source', '워크트리 소스', 5.6], ['other', '기타 · 지우지 않음', 3.9], ['shared_git', '공유 Git', 0.8]];
    const head = frame(`${id}-h`, 'Head', {layout: 'horizontal', width: 220}, [text(`${id}-ht`, `할당 ${size} · 눌러서 정리`, {size: '$--text-caption', fill: '$--foreground'})]);
    const warn = partial ? [frame(`${id}-w`, 'Partial', {layout: 'horizontal', width: 220}, [text(`${id}-wt`, '일부 체크아웃은 크기를 재지 못해 잰 것만 합했다', {size: '$--text-caption', fill: '$--warning', width: 220})])] : [];
    return frame(id, 'Tooltip', {layout: 'vertical', gap: 2, padding: ['$--spacing-xs', '$--spacing-sm'], fill: '$--popover', cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      head, ...warn,
      ...lines.map(([key, label, gb], index) => frame(`${id}-${index}`, label, {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center', width: 220}, [
        frame(`${id}-${index}-l`, label, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
          dot(`${id}-${index}-sw`, LAYERS[key].dot),
          text(`${id}-${index}-t`, label, {size: '$--text-caption', fill: '$--foreground'}),
        ]),
        spacer(`${id}-${index}-sp`),
        text(`${id}-${index}-v`, fmt(gb), {size: '$--text-caption', fill: '$--foreground', mono: true}),
      ])),
    ]);
  }

  // kind: 'plain' (tooltip on the number), 'partial' (a checkout could not be measured: `≥`), 'low' (the warning cell)
  function entry(kind) {
    const partial = kind === 'partial';
    const size = `${partial ? '≥ ' : ''}${fmt(USAGE_TOTAL)}`;
    const facts = [
      fact(`en-${kind}-wt-${s}`, 'folder-git-2', '36 worktrees'),
      fact(`en-${kind}-disk-${s}`, 'hard-drive', size, {underline: kind !== 'low'}),
      ...(kind === 'low' ? [fact(`en-low-free-${s}`, 'triangle-alert', `여유 ${fmt(1.6)} · ${fmt(23)} 비울 수 있음`, {fill: '$--warning', underline: true})] : []),
      fact(`en-${kind}-mg-${s}`, 'git-merge', '4 merged → 정리', {fill: '$--pr-merged'}),
    ];
    return frame(`en-${kind}-${s}`, `Entry: ${kind}`, {layout: 'vertical', gap: '$--spacing-sm', width: 560, padding: '$--spacing-md', fill: '$--background', cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      frame(`en-${kind}-title-${s}`, 'Title row', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        text(`en-${kind}-c1-${s}`, 'Overview', {size: '$--text-caption', fill: '$--subtle-foreground'}),
        text(`en-${kind}-c2-${s}`, '/', {size: '$--text-caption', fill: '$--muted-foreground'}),
        text(`en-${kind}-c3-${s}`, 'herdr-ide', {size: '$--text-headline', weight: '600'}),
      ]),
      frame(`en-${kind}-facts-${s}`, 'Facts', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center'}, facts),
      ...(kind === 'low' ? [] : [frame(`en-${kind}-tipwrap-${s}`, 'Tooltip anchor', {padding: [0, 0, 0, 96]}, [breakdown(`en-${kind}-tip-${s}`, size, partial)])]),
    ]);
  }

  // -- the table (DiskCleanupSheet.tsx GRID) --------------------------------------------------

  const W = 1160;
  const INNER = W - 48;
  const GAP = 8;
  const SEL = 24;
  const FR = (INNER - SEL - GAP * 6) / 8;
  const NAME = Math.round(FR * 2.6);
  const COL = Math.round(FR * 1.2);
  const NARROW = Math.round(FR * 0.9);

  function segmented(id, active, counts = {}) {
    return frame(id, 'Filter', {layout: 'horizontal', gap: 2, padding: 2, fill: '$--muted', cornerRadius: '$--radius-sm'}, FILTERS.map(([label, total], index) =>
      frame(`${id}-${index}`, label, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', height: 22, padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-xs', ...(index === active ? {fill: '$--background'} : {})}, [
        text(`${id}-${index}-t`, label, {size: '$--text-caption', weight: index === active ? '500' : '400', fill: index === active ? '$--foreground' : '$--subtle-foreground'}),
        text(`${id}-${index}-n`, String(counts[index] ?? total), {size: '$--text-caption', mono: true, fill: '$--muted-foreground'}),
      ])));
  }

  const skeleton = (id, width = 40) => frame(id, 'Skeleton', {width, height: 12, cornerRadius: '$--radius-xs', fill: '$--muted'}, []);
  const rowFrame = {layout: 'horizontal', alignItems: 'center', gap: GAP, width: INNER, padding: ['$--spacing-sm', 0], stroke: '$--border', strokeWidth: {bottom: HAIR}};

  function headCell(id, label, {state, bytes, checkable = true, align = 'start', width = COL}) {
    return frame(id, label, {layout: 'vertical', gap: 2, width, ...(align === 'end' ? {alignItems: 'end'} : {})}, [
      frame(`${id}-n`, 'Name', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        ...(checkable ? [checkbox(`${id}-c`, state === 'none' ? 'off' : state, {disabled: state === 'none'})] : []),
        text(`${id}-t`, label, {size: '$--text-caption', weight: '500'}),
      ]),
      text(`${id}-v`, bytes, {size: '$--text-caption', fill: '$--muted-foreground', mono: true, ...(align === 'end' ? {} : {})}),
    ]);
  }

  function tableHead(tag, states, sums) {
    return frame(`ma-head-${tag}`, 'Head', {...rowFrame, alignItems: 'end', padding: [0, 0, '$--spacing-sm', 0]}, [
      frame(`ma-hsel-${tag}`, 'Select all', {width: SEL, layout: 'horizontal'}, [checkbox(`ma-hsel-c-${tag}`, states.all === 'none' ? 'off' : states.all, {disabled: states.all === 'none'})]),
      frame(`ma-hn-${tag}`, '체크아웃', {width: NAME, layout: 'horizontal'}, [text(`ma-hn-t-${tag}`, '체크아웃', {size: '$--text-caption', fill: '$--subtle-foreground'})]),
      headCell(`ma-hb-${tag}`, '빌드 캐시', {state: states.build, bytes: sums.build}),
      headCell(`ma-hd-${tag}`, '의존성', {state: states.deps, bytes: sums.deps}),
      headCell(`ma-ht-${tag}`, '워크트리', {state: states.tree, bytes: sums.tree}),
      frame(`ma-ho-${tag}`, '기타', {layout: 'vertical', gap: 2, width: NARROW, alignItems: 'end'}, [
        text(`ma-ho-t-${tag}`, '기타', {size: '$--text-caption', fill: '$--subtle-foreground'}),
        text(`ma-ho-v-${tag}`, sums.other, {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
      ]),
      frame(`ma-htot-${tag}`, '합계', {layout: 'vertical', gap: 2, width: NARROW, alignItems: 'end'}, [
        text(`ma-htot-t-${tag}`, '합계', {size: '$--text-caption', fill: '$--subtle-foreground'}),
        text(`ma-htot-v-${tag}`, sums.total, {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
      ]),
    ]);
  }

  function checkoutName(id, checkout, width) {
    const pr = checkout.pr ? [text(`${id}-pr`, `#${checkout.pr[0]}`, {size: '$--text-caption', fill: PR_TONE[checkout.pr[1]], mono: true})] : [];
    const busy = checkout.inUse ? [text(`${id}-use`, checkout.inUse, {size: '$--text-caption', fill: '$--warning'})] : [];
    const reserve = (checkout.pr ? 40 : 0) + (checkout.inUse ? textWidth(checkout.inUse, 12) + 8 : 0);
    return frame(id, checkout.name, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width}, [
      icon(`${id}-g`, checkout.main ? 'house' : 'git-branch', {size: 14, fill: '$--muted-foreground'}),
      text(`${id}-t`, fitText(checkout.name, width - 22 - reserve, BODY), {size: '$--text-body', fill: '$--foreground'}),
      ...pr, ...busy,
    ]);
  }

  const cellFrame = (id, name, extra = {}, children = []) => frame(id, name, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: COL, ...extra}, children);

  // A cache cell: a checkbox and its size; the cell of a checkout in use keeps both, dimmed and unticked.
  function cacheCell(id, checkout, key, pick, variant) {
    if (variant === 'pending') return cellFrame(id, 'Measuring', {}, [checkbox(`${id}-c`, 'off', {disabled: true}), skeleton(`${id}-sk`)]);
    if (variant === 'unavailable') return cellFrame(id, 'Unavailable');
    if (!checkout[key]) return cellFrame(id, 'Empty', {}, [text(`${id}-t`, '-', {size: '$--text-body', fill: '$--muted-foreground'})]);
    const included = pick === 'included';
    const blocked = cacheBlocked(checkout);
    return cellFrame(id, blocked ? 'In use' : 'Cell', {...(included || blocked ? {opacity: SECONDARY} : {})}, [
      checkbox(`${id}-c`, included ? 'included' : blocked ? 'off' : pick, {disabled: blocked}),
      text(`${id}-t`, fmt(checkout[key]), {size: '$--text-body', fill: '$--foreground', mono: true}),
    ]);
  }

  function treeCell(id, checkout, pick, variant) {
    if (checkout.main) return cellFrame(id, 'None');
    if (variant === 'pending') return cellFrame(id, 'Measuring', {}, [checkbox(`${id}-c`, 'off', {disabled: true}), skeleton(`${id}-sk`)]);
    if (variant === 'unavailable') return cellFrame(id, 'Unavailable');
    if (checkout.tree !== 'ok') return cellFrame(id, 'Blocked', {opacity: SECONDARY}, [icon(`${id}-g`, 'lock', {size: 14, fill: '$--muted-foreground'})]);
    return cellFrame(id, 'Cell', {}, [
      checkbox(`${id}-c`, pick),
      text(`${id}-t`, fmt(checkout.total), {size: '$--text-body', mono: true, fill: pick === 'on' ? '$--destructive' : '$--foreground'}),
    ]);
  }

  const otherCell = (id, checkout, variant) => frame(id, 'Other', {layout: 'horizontal', justifyContent: 'end', alignItems: 'center', width: NARROW}, variant === 'plain' && checkout.other > 0
    ? [text(`${id}-t`, fmt(checkout.other), {size: '$--text-body', mono: true, fill: '$--subtle-foreground'})] : []);

  const totalCell = (id, checkout, variant) => frame(id, 'Total', {layout: 'horizontal', justifyContent: 'end', alignItems: 'center', width: NARROW}, [
    ...(variant === 'plain' ? [text(`${id}-t`, fmt(checkout.total), {size: '$--text-body', mono: true, fill: '$--subtle-foreground'})] : variant === 'pending' ? [skeleton(`${id}-sk`, 36)] : []),
  ]);

  // The row checkbox reaches the caches only; a worktree pick counts them as included.
  function rowPick(checkout, pick) {
    if (cacheBlocked(checkout)) return 'none';
    const cells = ['build', 'deps'].filter(key => checkout[key]).map(key => pick[key]);
    if (cells.length === 0) return 'none';
    if (cells.every(value => value === 'on' || value === 'included')) return pick.tree === 'on' ? 'included' : 'on';
    return cells.some(value => value === 'on' || value === 'included') ? 'mixed' : 'off';
  }

  // A row of the table. variant: plain, pending (measuring: skeletons, boxes disabled) or unavailable (dimmed and blank, B6).
  function tableRow(tag, checkout, pick = {}, {variant = 'plain'} = {}) {
    const id = `ma-${checkout.id}-${tag}`;
    const included = pick.tree === 'on';
    const p = {build: included ? 'included' : pick.build ?? 'off', deps: included ? 'included' : pick.deps ?? 'off', tree: pick.tree ?? 'off'};
    const box = variant === 'plain' ? rowPick(checkout, p) : 'none';
    const rowBox = box === 'none' ? checkbox(`${id}-rc`, 'off', {disabled: true}) : checkbox(`${id}-rc`, box);
    return frame(id, checkout.name, {...rowFrame, ...(variant === 'unavailable' ? {opacity: DIM} : {}), ...(pick.hover ? {fill: '$--muted'} : {})}, [
      frame(`${id}-sel`, 'Row select', {width: SEL, layout: 'horizontal', alignItems: 'center'}, [rowBox]),
      checkoutName(`${id}-n`, checkout, NAME),
      cacheCell(`${id}-b`, checkout, 'build', p.build, variant),
      cacheCell(`${id}-d`, checkout, 'deps', p.deps, variant),
      treeCell(`${id}-t`, checkout, p.tree, variant),
      otherCell(`${id}-o`, checkout, variant),
      totalCell(`${id}-tot`, checkout, variant),
    ]);
  }

  const filterBar = (tag, active, counts) => frame(`ma-fb-${tag}`, 'Filter bar', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center', width: INNER}, [
    segmented(`ma-seg-${tag}`, active, counts),
  ]);

  // The bottom line (B16): `N칸 · X`, with `워크트리 M` between when a worktree is chosen; cancel and the clean button.
  function selectionFooter(tag, {summary, go = '정리', goDisabled = false}) {
    return [
      text(`ma-sum-t-${tag}`, summary || ' ', {size: '$--text-subhead', weight: '600'}),
      spacer(`ma-fsp-${tag}`),
      screenButton(`ma-cancel-${tag}`, '취소', {variant: 'ghost'}),
      goDisabled
        ? {...screenButton(`ma-go-${tag}`, go, {variant: 'default', icon: 'trash-2'}), opacity: DISABLED}
        : screenButton(`ma-go-${tag}`, go, {variant: 'default', icon: 'trash-2'}),
    ];
  }

  const fold = (tag, label, state) => frame(`ma-fold-${tag}`, 'Fold', {...rowFrame, gap: '$--spacing-sm'}, [
    checkbox(`ma-fold-c-${tag}`, state),
    icon(`ma-fold-g-${tag}`, 'chevron-right', {size: 14, fill: '$--muted-foreground'}),
    text(`ma-fold-t-${tag}`, label, {size: '$--text-body', fill: '$--subtle-foreground'}),
  ]);

  const usageLine = tag => usageBar(`ma-usage-${tag}`, INNER);

  // Everything shown, a hand-made mix: a whole row, single cells, one worktree.
  function sheetMixed() {
    const tag = `${s}1`;
    const picks = {rwb: {build: 'on', deps: 'on'}, oif: {build: 'on'}, crl: {build: 'on'}, ol: {tree: 'on'}, pfs: {hover: true}};
    const rows = CHECKOUTS.map(checkout => tableRow(tag, checkout, picks[checkout.id]));
    const tipAt = CHECKOUTS.findIndex(checkout => checkout.id === 'pfs') + 1;
    const tip = frame(`ma-tipwrap-${tag}`, 'Tooltip anchor', {padding: [0, 0, 0, SEL + NAME + COL * 2 + GAP * 3 - 40]}, [tooltip(`ma-tip-${tag}`, '바뀐 파일 3')]);
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', width: W,
      body: [
        usageLine(tag),
        filterBar(tag, 0),
        frame(`ma-table-${tag}`, 'Table', {layout: 'vertical', width: INNER}, [
          tableHead(tag, {all: 'mixed', build: 'mixed', deps: 'mixed', tree: 'mixed'}, {build: fmt(38.2), deps: fmt(7.9), tree: fmt(46), other: fmt(3.9), total: fmt(56.4)}),
          ...rows.slice(0, tipAt), tip, ...rows.slice(tipAt),
          fold(tag, `작은 체크아웃 27 · ${fmt(9.8)}`, 'off'),
        ]),
      ],
      footer: selectionFooter(tag, {summary: `4칸 · 워크트리 1 · ${fmt(19.9)}`}),
    });
  }

  // The 끝난 것 filter, then the top-left checkbox: every finished checkout's caches.
  function sheetDone() {
    const tag = `${s}2`;
    const rows = CHECKOUTS.filter(checkout => !checkout.main && !checkout.inUse && checkout.pr && checkout.pr[1] === 'merged').map(checkout => tableRow(tag, checkout, {build: 'on', deps: 'on'}));
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', width: W,
      body: [
        usageLine(tag),
        filterBar(tag, 1),
        frame(`ma-table-${tag}`, 'Table', {layout: 'vertical', width: INNER}, [
          tableHead(tag, {all: 'on', build: 'on', deps: 'on', tree: 'off'}, {build: fmt(19.4), deps: fmt(2.1), tree: fmt(9.3), other: fmt(0.9), total: fmt(23.1)}),
          ...rows,
          fold(tag, `작은 체크아웃 14 · ${fmt(2.1)}`, 'on'),
        ]),
      ],
      footer: selectionFooter(tag, {summary: `38칸 · ${fmt(23.1)}`}),
    });
  }

  // Measuring (B5), not measured (B6), and in use with its reason (B14): the rows a sheet carries before and beside a plain one.
  function sheetStates() {
    const tag = `${s}3`;
    const tip = frame(`ma-tipwrap-${tag}`, 'Tooltip anchor', {padding: [0, 0, 0, SEL + NAME - 60]}, [tooltip(`ma-tip-${tag}`, '터미널에서 vite 실행 중')]);
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', width: W,
      body: [
        filterBar(tag, 0),
        frame(`ma-table-${tag}`, 'Table', {layout: 'vertical', width: INNER}, [
          tableHead(tag, {all: 'none', build: 'none', deps: 'none', tree: 'none'}, {build: fmt(0), deps: fmt(0), tree: fmt(0), other: fmt(0), total: fmt(0)}),
          tableRow(tag, byId.main, {}, {variant: 'pending'}),
          tableRow(tag, byId.rwb, {}, {variant: 'pending'}),
          tableRow(tag, byId.mc),
          tableRow(tag, byId.vt),
          tip,
          tableRow(tag, byId.ol, {}, {variant: 'unavailable'}),
        ]),
      ],
      footer: selectionFooter(tag, {summary: '검토하는 중…', goDisabled: true}),
    });
  }

  // B25: usage cannot be read, so nothing is ticked and the notice stands over the table.
  function sheetUnreadable() {
    const tag = `${s}5`;
    const notice = frame(`ma-notice-${tag}`, 'Usage unreadable', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: INNER}, [
      text(`ma-notice-t-${tag}`, '지금 쓰는 중인지 확인할 수 없다', {size: '$--text-body', fill: '$--warning'}),
      screenButton(`ma-notice-b-${tag}`, '다시', {variant: 'ghost', height: 28, icon: 'refresh-cw'}),
    ]);
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', width: W,
      body: [
        usageLine(tag),
        notice,
        filterBar(tag, 0),
        frame(`ma-table-${tag}`, 'Table', {layout: 'vertical', width: INNER}, [
          tableHead(tag, {all: 'none', build: 'none', deps: 'none', tree: 'none'}, {build: fmt(38.2), deps: fmt(7.9), tree: fmt(46), other: fmt(3.9), total: fmt(56.4)}),
          ...['main', 'rwb', 'crl'].map(id => tableRow(tag, byId[id], {}, {})),
        ]),
      ],
      footer: selectionFooter(tag, {summary: '쓰는 중인지 확인해야 고를 수 있다', goDisabled: true}),
    });
  }

  // B20: another project's cleanup is running; the clean button says so and the summary is empty.
  function sheetBusy() {
    const tag = `${s}6`;
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', width: W,
      body: [
        usageLine(tag),
        filterBar(tag, 0),
        frame(`ma-table-${tag}`, 'Table', {layout: 'vertical', width: INNER}, [
          tableHead(tag, {all: 'none', build: 'none', deps: 'none', tree: 'none'}, {build: fmt(38.2), deps: fmt(7.9), tree: fmt(46), other: fmt(3.9), total: fmt(56.4)}),
          ...['main', 'rwb'].map(id => tableRow(tag, byId[id], {}, {})),
        ]),
      ],
      footer: selectionFooter(tag, {summary: '', go: '다른 정리가 진행 중', goDisabled: true}),
    });
  }

  // B13: a filter no checkout matches.
  function sheetEmpty() {
    const tag = `${s}4`;
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', width: 640,
      body: [
        frame(`ma-fb-${tag}`, 'Filter bar', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: 592}, [segmented(`ma-seg-${tag}`, 3, {3: 0})]),
        frame(`ma-empty-${tag}`, 'Empty', {layout: 'vertical', gap: '$--spacing-sm', alignItems: 'center', justifyContent: 'center', width: 592, height: 96}, [
          text(`ma-empty-t-${tag}`, '이 필터에 맞는 체크아웃이 없다', {size: '$--text-body', fill: '$--muted-foreground'}),
          screenButton(`ma-empty-b-${tag}`, '전체 보기', {variant: 'secondary', height: 28}),
        ]),
      ],
      footer: selectionFooter(tag, {summary: '비울 캐시가 없다', goDisabled: true}),
    });
  }

  // -- confirm, running, result -----------------------------------------------------------------

  // B18: an AlertDialog over the sheet, only when a worktree is picked; neither button holds the keyboard.
  function confirmStep() {
    const width = 470;
    const inner = width - 32;
    return screenDialogSurface(`cf-${s}`, {
      width, title: 'feat/overview-lenses 폴더째 삭제', description: '브랜치는 남음', prose: true,
      body: [
        frame(`cf-row-${s}`, 'feat/overview-lenses', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'baseline', width: inner}, [
          text(`cf-row-t-${s}`, 'feat/overview-lenses', {size: '$--text-body', mono: true}),
          spacer(`cf-row-sp-${s}`),
          text(`cf-row-v-${s}`, fmt(3.6), {size: '$--text-body', mono: true, fill: '$--muted-foreground'}),
        ]),
      ],
      actions: [screenButton(`cf-back-${s}`, '돌아가기', {variant: 'secondary'}), screenButton(`cf-go-${s}`, '워크트리 1개와 캐시 정리', {variant: 'destructive'})],
    });
  }

  // B20: the deleting has begun, the folders are already gone from their checkouts.
  function runningStep() {
    const width = 560;
    return dialog(`rn-${s}`, {
      title: '디스크 정리', width,
      body: [
        frame(`rn-body-${s}`, 'Running', {layout: 'vertical', gap: '$--spacing-sm', width: width - 48, padding: ['$--spacing-lg', 0]}, [
          frame(`rn-head-${s}`, 'Title', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
            icon(`rn-spin-${s}`, 'loader-circle', {size: 14, fill: '$--muted-foreground'}),
            text(`rn-t-${s}`, '비우는 중 · 3/7', {size: '$--text-title', weight: '600'}),
          ]),
          text(`rn-n-${s}`, '닫아도 정리는 계속된다. 다시 열면 진행이나 결과가 보인다.', {size: '$--text-body', fill: '$--muted-foreground'}),
        ]),
      ],
    });
  }

  // B22: the free space before and after leads; the allocated total is the lesser number beside it.
  function resultStep() {
    const width = 560;
    const inner = width - 48;
    const lines = [
      ['fix/remote-workspace-bridge', '빌드 캐시 · 의존성', 'removed', null, fmt(7.3 + 0.63)],
      ['feat/overview-issue-first', '빌드 캐시', 'removed', null, fmt(5.9)],
      ['feat/overview-lenses', '워크트리 삭제', 'removed', null, fmt(3.6)],
      ['ci/runner-layout', '빌드 캐시', 'skipped', '확인 사이에 쓰는 중이 됨', null],
      ['prd/agent-tab-groups', '의존성', 'failed', '폴더를 지우지 못함', null],
    ];
    const OUTCOME = {removed: ['지움', '$--success'], skipped: ['건너뜀', '$--warning'], failed: ['실패', '$--destructive']};
    const line = ([label, what, outcome, reason, size], index) => frame(`rs-l${index}-${s}`, label, {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'baseline', width: inner, padding: ['$--spacing-sm', 0], ...(index ? {stroke: '$--border', strokeWidth: {top: HAIR}} : {})}, [
      text(`rs-l${index}-a-${s}`, fitText(label, 150, BODY), {size: '$--text-body', width: 150}),
      text(`rs-l${index}-b-${s}`, what, {size: '$--text-body', fill: '$--muted-foreground', width: 120}),
      text(`rs-l${index}-c-${s}`, `${OUTCOME[outcome][0]}${reason ? ` · ${reason}` : ''}`, {size: '$--text-caption', fill: OUTCOME[outcome][1], width: 170}),
      spacer(`rs-l${index}-sp-${s}`),
      text(`rs-l${index}-d-${s}`, size ?? ' ', {size: '$--text-caption', mono: true, fill: '$--subtle-foreground'}),
    ]);
    return dialog(`rs-${s}`, {
      title: '디스크 정리', width,
      body: [
        text(`rs-big-${s}`, `여유 ${(1.6).toFixed(1)} → ${(18.3).toFixed(1)} GB`, {size: '$--text-headline', weight: '600', mono: true}),
        frame(`rs-list-${s}`, 'Outcomes', {layout: 'vertical', width: inner, stroke: '$--border', strokeWidth: {top: HAIR, bottom: HAIR}}, lines.map(line)),
        frame(`rs-act-${s}`, 'Actions', {layout: 'horizontal', gap: '$--spacing-sm', justifyContent: 'end', width: inner}, [
          screenButton(`rs-again-${s}`, '다시 검토', {variant: 'ghost', icon: 'refresh-cw'}),
          screenButton(`rs-close-${s}`, '닫기', {variant: 'secondary'}),
        ]),
      ],
    });
  }

  // -- assembly ----------------------------------------------------------------------------------------

  const label = (id, title, note, width) => frame(id, 'Label', {layout: 'vertical', gap: '$--spacing-xxs', width}, [
    text(`${id}-t`, title, {size: '$--text-subhead', weight: '600'}),
    ...(note ? [text(`${id}-n`, note, {size: '$--text-caption', fill: '$--muted-foreground', width})] : []),
  ]);
  const labelled = (id, title, note, node, width) => frame(id, title, {layout: 'vertical', gap: '$--spacing-sm'}, [label(`${id}-l`, title, note, width), node]);
  const row = (id, children) => frame(id, 'Row', {layout: 'horizontal', gap: '$--spacing-xl', alignItems: 'start'}, children);

  return [frame(`dc-${s}`, 'Disk Cleanup', {layout: 'vertical', gap: '$--spacing-xl'}, [
    row(`dc-r0-${s}`, [
      labelled(`dc-l-en-${s}`, '입구 · 디스크 숫자 위에 올리면', '요약 줄의 디스크 숫자가 레이어별 내역을 툴팁으로 보여주고, 누르면 정리 시트가 열린다.', entry('plain'), 560),
      labelled(`dc-l-ep-${s}`, '입구 · 재지 못한 체크아웃이 있을 때', '재지 못한 체크아웃이 있으면 잰 것만 합해 ≥ 로 적고 툴팁이 그렇게 말한다. 시스템이 만들 수 없는 총합은 적지 않는다.', entry('partial'), 560),
      labelled(`dc-l-el-${s}`, '입구 · 디스크가 모자랄 때만', '볼륨 여유가 10 GB 미만일 때만 경고 칸이 생긴다. 비울 수 있는 양은 끝난 체크아웃의 캐시. 누르면 끝난 것 필터로 열린다.', entry('low'), 560),
    ]),
    labelled(`dc-l-m1-${s}`, '시트 · 전체에서 손으로 고르기', '행 하나 통째, 칸 몇 개, 워크트리 하나를 고른 상태. 열 머리와 왼쪽 위는 일부만 골라서 가운데 줄. 쓰는 중인 행은 이유가 이름 옆에 선다. 워크트리 칸을 켜면 같은 행의 캐시 칸은 포함됨.', sheetMixed(), W),
    labelled(`dc-l-m2-${s}`, '시트 · 끝난 것 필터 + 왼쪽 위 체크', '가장 흔한 정리: 끝난 체크아웃의 빌드 캐시와 의존성을 클릭 두 번에. 워크트리는 켜지지 않는다.', sheetDone(), W),
    labelled(`dc-l-m3-${s}`, '시트 · 측정 중, 재지 못함, 쓰는 중', '측정 중인 칸은 skeleton과 비활성 체크박스, 재지 못한 행은 흐리고 비어 있다. 쓰는 중인 행은 캐시 칸이 비활성이고 이유는 멈추면 뜬다.', sheetStates(), W),
    row(`dc-r4-${s}`, [
      labelled(`dc-l-m5-${s}`, '시트 · 쓰는 중인지 읽지 못함', '표 위에 한 줄과 다시. 체크박스는 모두 비활성이다.', sheetUnreadable(), W),
    ]),
    row(`dc-r5-${s}`, [
      labelled(`dc-l-m6-${s}`, '시트 · 다른 정리가 진행 중', '정리 버튼이 이유를 말하고 요약 줄은 비어 있다.', sheetBusy(), W),
    ]),
    row(`dc-r6-${s}`, [
      labelled(`dc-l-cf-${s}`, '확인 · 워크트리가 든 정리만', '캐시와 의존성만 고르면 확인 없이 바로 실행된다. 어느 버튼도 기본 포커스가 아니고 돌아가기는 선택을 그대로 둔다.', confirmStep(), 470),
      labelled(`dc-l-em-${s}`, '필터 결과 없음', '맞는 체크아웃이 없으면 한 줄과 전체 보기. 정리는 비활성.', sheetEmpty(), 640),
    ]),
    row(`dc-r7-${s}`, [
      labelled(`dc-l-rn-${s}`, '실행 중', '폴더는 체크아웃에서 곧바로 사라진다. 시트를 닫아도 계속되고 다시 열면 진행이나 결과가 보인다.', runningStep(), 560),
      labelled(`dc-l-rs-${s}`, '결과', '결과는 정리 전후 볼륨 여유 한 줄이다. 확인 사이에 상태가 바뀐 칸은 건너뛰고 이유를 적는다.', resultStep(), 560),
    ]),
  ])];
}
