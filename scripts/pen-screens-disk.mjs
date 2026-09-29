// The children of `Screen / Disk Cleanup` (PRD disk-layers B1-B28): the Overview's
// disk entry, the cleanup sheet as a checkout x layer table with its checkboxes,
// the states the table carries, the confirm step, the running step and the result.
// Drawn on this document's local tokens plus library refs (Checkbox, Button), the
// way every other Screen sheet is, and called from pen-screens.mjs, which owns the
// ref helpers (`themedXref`, `screenButton`) and the sheet frame. Every name,
// path and number is invented mock content, never a value the shell could read.

import {frame, icon, num, text} from './pen-system.mjs';

export function diskCleanupRows(tokens, {themedXref, screenButton}, s) {
  const HAIR = num(tokens, '--size-hairline');
  const DIM = num(tokens, '--opacity-dimmed');
  const DISABLED = num(tokens, '--opacity-disabled');
  const BODY = num(tokens, '--text-body');
  const CAPTION = num(tokens, '--text-caption');
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);
  const rule = (id, width = 'fill_container') => frame(id, 'Rule', {width, height: HAIR, fill: '$--border'}, []);
  const gb = value => value === 0 ? '–' : value < 1 ? `${Math.round(value * 1000)} MB` : `${value.toFixed(1)} GB`;

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

  // -- the model ---------------------------------------------------------------------

  const BUILD = {name: '빌드 캐시', glyph: 'hammer', tone: '$--file-orange', total: 38.2};
  const DEPS = {name: '의존성', glyph: 'package', tone: '$--file-blue', total: 7.9};
  const TREE = {name: '워크트리', glyph: 'folder-git-2', tone: '$--muted-foreground'};
  const OTHER = {name: '기타', glyph: 'circle-dashed', tone: '$--file-yellow', total: 3.9};
  const SOURCE_REST = 6.5;
  const VOLUME = {size: 460, free: 1.6};

  // state: work (in use), rest (idle, not finished), done (PR merged or closed); tree: 'ok', a refusal in words, or 'none' (main)
  const CHECKOUTS = [
    {id: 'main', name: 'main', kind: 'main', state: 'rest', build: 4.9, deps: 0.66, other: 2.6, tree: 'none'},
    {id: 'rwb', name: 'fix/remote-workspace-bridge', pr: [227, 'merged'], state: 'done', build: 7.3, deps: 0.65, other: 0.005, tree: 'ok'},
    {id: 'oif', name: 'feat/overview-issue-first', pr: [218, 'closed'], state: 'done', build: 5.9, deps: 0.33, other: 0, tree: '머지되지 않음'},
    {id: 'mc', name: 'mobile-conversation', pr: [252, 'open'], state: 'work', build: 4.1, deps: 0.33, other: 0, tree: '에이전트 작업 중'},
    {id: 'cas', name: 'prd/close-agent-subtree', state: 'work', build: 3.7, deps: 0, other: 0, tree: '에이전트 작업 중'},
    {id: 'pfs', name: 'fix/pane-find-scroll', state: 'rest', build: 3.6, deps: 0.33, other: 0.1, tree: '바뀐 파일 3'},
    {id: 'ol', name: 'feat/overview-lenses', pr: [238, 'merged'], state: 'done', build: 3.4, deps: 0.33, other: 0, tree: 'ok'},
    {id: 'crl', name: 'ci/runner-layout', pr: [248, 'merged'], state: 'done', build: 2.4, deps: 0.33, other: 0, tree: 'ok'},
    {id: 'atg', name: 'prd/agent-tab-groups', pr: [217, 'merged'], state: 'done', build: 0, deps: 0.33, other: 0.82, tree: 'ok'},
  ].map(checkout => ({...checkout, total: checkout.build + checkout.deps + checkout.other + 0.6}));
  const FILTERS = [['전체', 36], ['끝난 것', 19], ['쉬는 것', 11], ['작업 중', 6]];
  const PR_TONE = {open: '$--pr-open', merged: '$--pr-merged', closed: '$--pr-closed'};
  const PR_GLYPH = {open: 'git-pull-request', merged: 'git-merge', closed: 'git-pull-request'};

  const cacheState = (checkout, key) => !checkout[key] ? 'none' : checkout.state === 'work' ? 'work' : 'ok';

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

  const prChip = (id, [number, state]) => frame(id, `PR #${number}`, {layout: 'horizontal', gap: 2, alignItems: 'center'}, [
    icon(`${id}-g`, PR_GLYPH[state], {size: 11, fill: PR_TONE[state]}),
    text(`${id}-n`, `#${number}`, {size: '$--text-caption', fill: PR_TONE[state], mono: true}),
  ]);

  function checkoutMark(id, checkout) {
    const box = {width: 12, height: 12, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'};
    if (checkout.kind === 'main') return frame(id, 'Mark', box, [icon(`${id}-i`, 'house', {size: 11, fill: '$--subtle-foreground'})]);
    if (checkout.state === 'work') return frame(id, 'Mark', box, [{type: 'ellipse', id: `${id}-dot`, name: 'Dot', width: 7, height: 7, fill: '$--agent-working'}]);
    if (checkout.state === 'rest') return frame(id, 'Mark', box, [{type: 'ellipse', id: `${id}-ring`, name: 'Ring', width: 7, height: 7, stroke: '$--muted-foreground', strokeWidth: HAIR, strokeAlignment: 'inner'}]);
    return frame(id, 'Mark', box, [icon(`${id}-i`, 'git-branch', {size: 11, fill: '$--muted-foreground'})]);
  }

  function checkoutName(id, checkout, width) {
    const prWidth = checkout.pr ? 44 : 0;
    return frame(id, checkout.name, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width}, [
      checkoutMark(`${id}-m`, checkout),
      text(`${id}-t`, fitText(checkout.name, width - 20 - prWidth, BODY), {size: '$--text-body', fill: checkout.state === 'done' ? '$--subtle-foreground' : '$--foreground'}),
      ...(checkout.pr ? [prChip(`${id}-pr`, checkout.pr)] : []),
    ]);
  }

  function layerBar(id, parts, width, height = 8) {
    const total = parts.reduce((sum, [, value]) => sum + value, 0);
    const shown = parts.filter(([, value]) => value > 0);
    return frame(id, 'Bar', {layout: 'horizontal', gap: 1, width, height, cornerRadius: height / 2, clip: true},
      shown.map(([fill, value], index) => frame(`${id}-${index}`, 'Part', {width: Math.max(2, Math.round((width - shown.length) * value / total)), height, fill}, [])));
  }

  const PARTS = [[BUILD.name, BUILD.tone, BUILD.total], [DEPS.name, DEPS.tone, DEPS.total], [OTHER.name, OTHER.tone, OTHER.total], ['소스 · .git', '$--muted-foreground', SOURCE_REST]];

  function volumeLine(id, width) {
    const project = PARTS.reduce((sum, [, , value]) => sum + value, 0);
    return frame(id, 'Volume', {layout: 'vertical', gap: '$--spacing-xs', width}, [
      frame(`${id}-top`, 'Numbers', {layout: 'horizontal', alignItems: 'center', gap: '$--spacing-sm', width}, [
        text(`${id}-p`, `이 프로젝트 ${project.toFixed(1)} GB`, {size: '$--text-caption', fill: '$--subtle-foreground', mono: true}),
        spacer(`${id}-sp`),
        icon(`${id}-w`, 'triangle-alert', {size: 12, fill: '$--warning'}),
        text(`${id}-f`, `디스크 여유 ${VOLUME.free} GB / ${VOLUME.size} GB`, {size: '$--text-caption', fill: '$--warning', mono: true}),
      ]),
      layerBar(`${id}-bar`, PARTS.map(([, tone, value]) => [tone, value]), width),
      frame(`${id}-lg`, 'Legend', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center'}, PARTS.map(([name, tone], index) =>
        frame(`${id}-lg${index}`, name, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
          frame(`${id}-lg${index}-sw`, 'Swatch', {width: 8, height: 8, cornerRadius: 2, fill: tone}, []),
          text(`${id}-lg${index}-t`, name, {size: '$--text-caption', fill: '$--subtle-foreground'}),
        ]))),
    ]);
  }

  function dialog(id, {title, subtitle, width, body, footer}) {
    return frame(id, 'Dialog', {layout: 'vertical', width, fill: '$--popover', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', clip: true}, [
      frame(`${id}-h`, 'Header', {layout: 'horizontal', alignItems: 'start', gap: '$--spacing-md', width, padding: ['$--spacing-lg', '$--spacing-lg', '$--spacing-sm', '$--spacing-lg']}, [
        frame(`${id}-ht`, 'Title', {layout: 'vertical', gap: '$--spacing-xxs'}, [
          text(`${id}-title`, title, {size: '$--text-title', weight: '600'}),
          ...(subtitle ? [text(`${id}-sub`, subtitle, {size: '$--text-caption', fill: '$--muted-foreground'})] : []),
        ]),
        spacer(`${id}-hs`),
        icon(`${id}-x`, 'x', {size: 16, fill: '$--muted-foreground'}),
      ]),
      frame(`${id}-b`, 'Body', {layout: 'vertical', gap: '$--spacing-md', width, padding: ['$--spacing-sm', '$--spacing-lg', '$--spacing-md', '$--spacing-lg']}, body),
      ...(footer ? [rule(`${id}-fr`, width), frame(`${id}-f`, 'Footer', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width, padding: ['$--spacing-sm', '$--spacing-lg']}, footer)] : []),
    ]);
  }

  const tooltip = (id, content) => frame(id, 'Tooltip', {padding: ['$--spacing-xxs', '$--spacing-sm'], fill: '$--popover', cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
    text(`${id}-t`, content, {size: '$--text-caption'}),
  ]);

  const SUBTITLE = 'herdr-ide · 36 체크아웃 · 크기는 할당된 블록이고 실제로 비워지는 양은 정리 후 디스크에서 잰다';

  // -- entry: the Overview's facts line ------------------------------------------------

  function fact(id, glyph, label, {fill = '$--subtle-foreground', underline = false} = {}) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', ...(underline ? {padding: [0, 0, 1, 0], stroke: fill, strokeWidth: {bottom: HAIR}} : {})}, [
      icon(`${id}-g`, glyph, {size: 12, fill}),
      text(`${id}-t`, label, {size: '$--text-caption', fill, mono: true}),
    ]);
  }

  function breakdown(id) {
    const rows = [['할당 56.5 GB · 눌러서 정리', '', null], ...PARTS.map(([name, tone, value]) => [name === OTHER.name ? '기타 · 지우지 않음' : name, `${value} GB`, tone])];
    return frame(id, 'Tooltip', {layout: 'vertical', gap: 2, padding: ['$--spacing-xs', '$--spacing-sm'], fill: '$--popover', cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'},
      rows.map(([label, value, tone], index) => frame(`${id}-${index}`, label, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: 196}, [
        ...(tone ? [frame(`${id}-${index}-sw`, 'Swatch', {width: 8, height: 8, cornerRadius: 2, fill: tone}, [])] : []),
        text(`${id}-${index}-l`, label, {size: '$--text-caption', fill: tone ? '$--subtle-foreground' : '$--foreground'}),
        spacer(`${id}-${index}-sp`),
        text(`${id}-${index}-v`, value, {size: '$--text-caption', fill: '$--subtle-foreground', mono: true}),
      ])));
  }

  function entry(low) {
    const k = low ? 'l' : 'n';
    const facts = [
      fact(`en-${k}-wt-${s}`, 'folder-git-2', '36 worktrees'),
      fact(`en-${k}-disk-${s}`, 'hard-drive', '56.5 GB', {underline: !low}),
      ...(low ? [fact(`en-l-free-${s}`, 'triangle-alert', '여유 1.6 GB · 23 GB 비울 수 있음', {fill: '$--warning', underline: true})] : []),
      fact(`en-${k}-mg-${s}`, 'git-merge', '4 merged → 정리', {fill: '$--pr-merged'}),
    ];
    return frame(`en-${k}-${s}`, low ? 'Entry: low disk' : 'Entry: disk under the pointer', {layout: 'vertical', gap: '$--spacing-sm', width: 640, padding: '$--spacing-md', fill: '$--background', cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      frame(`en-${k}-title-${s}`, 'Title row', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        text(`en-${k}-c1-${s}`, 'Overview', {size: '$--text-caption', fill: '$--subtle-foreground'}),
        text(`en-${k}-c2-${s}`, '/', {size: '$--text-caption', fill: '$--muted-foreground'}),
        text(`en-${k}-c3-${s}`, 'herdr-ide', {size: '$--text-headline', weight: '600'}),
      ]),
      frame(`en-${k}-facts-${s}`, 'Facts', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center'}, facts),
      ...(low ? [] : [frame(`en-${k}-tipwrap-${s}`, 'Tooltip anchor', {padding: [0, 0, 0, 96]}, [breakdown(`en-${k}-tip-${s}`)])]),
    ]);
  }

  // -- the table and its checkboxes ----------------------------------------------------------

  const W = 880;
  const INNER = W - 48;
  const SEL = 28;
  const COL = 116;
  const OTHER_COL = 84;
  const TOTAL_COL = 76;
  const NAME = INNER - SEL - COL * 3 - OTHER_COL - TOTAL_COL;

  function segmented(id, active, counts = {}) {
    return frame(id, 'Filter', {layout: 'horizontal', gap: 2, padding: 2, fill: '$--muted', cornerRadius: '$--radius-sm'}, FILTERS.map(([label, total], index) =>
      frame(`${id}-${index}`, label, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', height: 22, padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-xs', ...(index === active ? {fill: '$--background'} : {})}, [
        text(`${id}-${index}-t`, label, {size: '$--text-caption', weight: index === active ? '500' : '400', fill: index === active ? '$--foreground' : '$--subtle-foreground'}),
        text(`${id}-${index}-n`, String(counts[index] ?? total), {size: '$--text-caption', mono: true, fill: '$--muted-foreground'}),
      ])));
  }

  function headCell(id, layer, {state, total, reach, checkable = true}) {
    return frame(id, layer.name, {layout: 'vertical', gap: 2, width: layer === OTHER ? OTHER_COL : COL, padding: [0, 0, 0, '$--spacing-xs']}, [
      frame(`${id}-n`, 'Name', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        ...(checkable ? [checkbox(`${id}-c`, state)] : []),
        icon(`${id}-g`, layer.glyph, {size: 12, fill: layer.tone}),
        text(`${id}-t`, layer.name, {size: '$--text-caption', weight: '600'}),
      ]),
      text(`${id}-v`, total, {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
      text(`${id}-r`, reach, {size: '$--text-caption', fill: '$--muted-foreground'}),
    ]);
  }

  const cellBase = {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: COL, padding: [0, 0, 0, '$--spacing-xs']};
  const skeletonBar = (id, width = 40) => frame(id, 'Skeleton', {width, height: 8, cornerRadius: 4, fill: '$--muted'}, []);

  // A cache cell: its checkbox and size, dimmed with the reason's glyph when it cannot be chosen.
  function cacheCell(id, checkout, key, pick, measured = true) {
    if (!measured) return frame(id, 'Measuring', cellBase, [checkbox(`${id}-c`, 'off', {disabled: true}), skeletonBar(`${id}-sk`)]);
    const state = cacheState(checkout, key);
    if (state === 'none') return frame(id, 'Empty', cellBase, [text(`${id}-t`, '–', {size: '$--text-caption', fill: '$--muted-foreground', mono: true})]);
    if (state === 'work') return frame(id, 'In use', {...cellBase, opacity: DIM}, [
      icon(`${id}-g`, 'loader-circle', {size: 12, fill: '$--muted-foreground'}),
      text(`${id}-t`, gb(checkout[key]), {size: '$--text-caption', mono: true, fill: '$--muted-foreground'}),
    ]);
    return frame(id, 'Cell', cellBase, [
      checkbox(`${id}-c`, pick),
      text(`${id}-t`, gb(checkout[key]), {size: '$--text-caption', mono: true, fill: pick === 'included' ? '$--muted-foreground' : key === 'deps' ? '$--subtle-foreground' : '$--foreground'}),
      ...(key === 'deps' ? [icon(`${id}-sh`, 'link-2', {size: 11, fill: '$--muted-foreground'})] : []),
    ]);
  }

  function treeCell(id, checkout, pick, measured = true) {
    if (checkout.tree === 'none') return frame(id, 'None', cellBase, []);
    if (!measured) return frame(id, 'Measuring', cellBase, [checkbox(`${id}-c`, 'off', {disabled: true}), skeletonBar(`${id}-sk`)]);
    if (checkout.tree !== 'ok') return frame(id, 'Refused', {...cellBase, opacity: DIM}, [icon(`${id}-g`, 'lock', {size: 12, fill: '$--muted-foreground'})]);
    return frame(id, 'Cell', cellBase, [
      checkbox(`${id}-c`, pick),
      text(`${id}-t`, gb(checkout.total), {size: '$--text-caption', mono: true, fill: pick === 'on' ? '$--destructive' : '$--foreground'}),
    ]);
  }

  const otherCell = (id, checkout, measured = true) => frame(id, 'Other', {layout: 'horizontal', alignItems: 'center', width: OTHER_COL, padding: [0, 0, 0, '$--spacing-xs']}, [
    measured ? text(`${id}-t`, gb(checkout.other), {size: '$--text-caption', mono: true, fill: '$--muted-foreground'}) : skeletonBar(`${id}-sk`, 28),
  ]);

  const totalCell = (id, checkout, measured = true) => frame(id, 'Total', {layout: 'horizontal', justifyContent: 'end', alignItems: 'center', width: TOTAL_COL}, [
    measured ? text(`${id}-t`, gb(checkout.total), {size: '$--text-caption', mono: true, fill: '$--subtle-foreground'}) : skeletonBar(`${id}-sk`, 36),
  ]);

  // The row checkbox reaches the caches only; a worktree pick counts them as included.
  function rowPick(checkout, pick) {
    if (checkout.state === 'work') return 'off';
    const cells = ['build', 'deps'].filter(key => cacheState(checkout, key) === 'ok').map(key => pick[key]);
    if (cells.every(value => value === 'on' || value === 'included')) return 'on';
    return cells.some(value => value === 'on' || value === 'included') ? 'mixed' : 'off';
  }

  // A row of the table. `variant` picks the state a row can be in besides the plain one:
  // measuring (skeleton cells, checkbox disabled) or unmeasured (dimmed, sizes blank, B6).
  function tableRow(tag, checkout, pick = {}, {variant = 'plain'} = {}) {
    const id = `ma-${checkout.id}-${tag}`;
    const work = checkout.state === 'work';
    const included = pick.tree === 'on';
    const p = {build: included ? 'included' : pick.build ?? 'off', deps: included ? 'included' : pick.deps ?? 'off', tree: pick.tree ?? 'off'};
    if (variant === 'unmeasured') {
      return frame(id, checkout.name, {layout: 'horizontal', alignItems: 'center', width: INNER, height: 32, opacity: DIM}, [
        frame(`${id}-sel`, 'Row select', {width: SEL, layout: 'horizontal', alignItems: 'center', padding: [0, 0, 0, 4]}, [checkbox(`${id}-rc`, 'off', {disabled: true})]),
        checkoutName(`${id}-n`, checkout, NAME - 8),
        ...['b', 'd', 't'].map(k => frame(`${id}-${k}`, 'Empty', cellBase, [])),
        frame(`${id}-o`, 'Empty', {width: OTHER_COL}, []),
        frame(`${id}-tot`, 'Empty', {width: TOTAL_COL}, []),
      ]);
    }
    const measured = variant !== 'measuring';
    return frame(id, checkout.name, {layout: 'horizontal', alignItems: 'center', width: INNER, height: 32, ...(pick.hover ? {fill: '$--accent', cornerRadius: '$--radius-sm'} : {})}, [
      frame(`${id}-sel`, 'Row select', {width: SEL, layout: 'horizontal', alignItems: 'center', padding: [0, 0, 0, 4]}, [checkbox(`${id}-rc`, measured ? rowPick(checkout, p) : 'off', {disabled: work || !measured})]),
      checkoutName(`${id}-n`, checkout, NAME - 8),
      cacheCell(`${id}-b`, checkout, 'build', p.build, measured),
      cacheCell(`${id}-d`, checkout, 'deps', p.deps, measured),
      treeCell(`${id}-t`, checkout, p.tree, measured),
      otherCell(`${id}-o`, checkout, measured),
      totalCell(`${id}-tot`, checkout, measured),
    ]);
  }

  function tableHead(tag, states, reach) {
    return frame(`ma-head-${tag}`, 'Head', {layout: 'horizontal', alignItems: 'end', width: INNER, padding: [0, 0, '$--spacing-xs', 0]}, [
      frame(`ma-hsel-${tag}`, 'Select all', {width: SEL, layout: 'horizontal', padding: [0, 0, 2, 4]}, [checkbox(`ma-hsel-c-${tag}`, states.all)]),
      frame(`ma-hn-${tag}`, '체크아웃', {width: NAME, layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', padding: [0, 0, 2, 0]}, [
        text(`ma-hn-t-${tag}`, '체크아웃', {size: '$--text-caption', weight: '600', fill: '$--subtle-foreground'}),
        text(`ma-hn-s-${tag}`, '· 크기순', {size: '$--text-caption', fill: '$--muted-foreground'}),
        icon(`ma-hn-g-${tag}`, 'arrow-down', {size: 11, fill: '$--muted-foreground'}),
      ]),
      headCell(`ma-hb-${tag}`, BUILD, {state: states.build, total: `${BUILD.total} GB`, reach: reach.build}),
      headCell(`ma-hd-${tag}`, DEPS, {state: states.deps, total: `${DEPS.total} GB`, reach: reach.deps}),
      headCell(`ma-ht-${tag}`, TREE, {state: states.tree, total: '머지 4 · 12.8 GB', reach: reach.tree}),
      headCell(`ma-ho-${tag}`, OTHER, {checkable: false, total: `${OTHER.total} GB`, reach: '지우지 않음'}),
      frame(`ma-htot-${tag}`, '합계', {layout: 'vertical', gap: 2, width: TOTAL_COL, alignItems: 'end'}, [
        text(`ma-htot-t-${tag}`, '합계', {size: '$--text-caption', weight: '600'}),
        text(`ma-htot-v-${tag}`, '56.5 GB', {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
      ]),
    ]);
  }

  const filterBar = (tag, active) => frame(`ma-fb-${tag}`, 'Filter bar', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: INNER}, [
    segmented(`ma-seg-${tag}`, active),
    spacer(`ma-fb-sp-${tag}`),
    text(`ma-fb-n-${tag}`, '체크박스는 보이는 행에만 닿는다', {size: '$--text-caption', fill: '$--muted-foreground'}),
  ]);

  function selectionFooter(tag, {summary, warning, note, go, goDisabled = false, goVariant = 'default'}) {
    return [
      frame(`ma-sum-${tag}`, 'Selection', {layout: 'vertical', gap: 2}, [
        text(`ma-sum-t-${tag}`, summary, {size: '$--text-body', weight: '500', mono: true, ...(goDisabled ? {fill: '$--muted-foreground'} : {})}),
        ...(warning ? [frame(`ma-sum-w-${tag}`, 'Worktree warning', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
          icon(`ma-sum-wg-${tag}`, 'triangle-alert', {size: 11, fill: '$--destructive'}),
          text(`ma-sum-wt-${tag}`, warning, {size: '$--text-caption', fill: '$--destructive'}),
        ])] : []),
        ...(note ? [text(`ma-sum-n-${tag}`, note, {size: '$--text-caption', fill: '$--muted-foreground'})] : []),
      ]),
      spacer(`ma-fsp-${tag}`),
      screenButton(`ma-cancel-${tag}`, '취소', {variant: 'ghost'}),
      goDisabled
        ? {...screenButton(`ma-go-${tag}`, go, {variant: goVariant, icon: 'trash-2'}), opacity: DISABLED}
        : screenButton(`ma-go-${tag}`, go, {variant: goVariant, icon: 'trash-2'}),
    ];
  }

  const fold = (tag, label, state) => frame(`ma-fold-${tag}`, 'Fold', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: INNER, height: 28}, [
    frame(`ma-fold-sel-${tag}`, 'Row select', {width: SEL - 4, layout: 'horizontal', padding: [0, 0, 0, 4]}, [checkbox(`ma-fold-c-${tag}`, state)]),
    icon(`ma-fold-g-${tag}`, 'chevron-right', {size: 12, fill: '$--muted-foreground'}),
    text(`ma-fold-t-${tag}`, label, {size: '$--text-caption', fill: '$--muted-foreground'}),
  ]);

  // Everything shown, a hand-made mix: a whole row, single cells, one worktree.
  function sheetMixed() {
    const tag = `${s}1`;
    const picks = {rwb: {build: 'on', deps: 'on'}, oif: {build: 'on'}, crl: {build: 'on'}, ol: {tree: 'on'}, pfs: {hover: true}};
    const rows = CHECKOUTS.map(checkout => tableRow(tag, checkout, picks[checkout.id]));
    const tipAt = CHECKOUTS.findIndex(checkout => checkout.id === 'pfs') + 1;
    const tip = frame(`ma-tipwrap-${tag}`, 'Tooltip anchor', {padding: [0, 0, 0, SEL + NAME + COL * 2 - 100]}, [tooltip(`ma-tip-${tag}`, '바뀐 파일 3 · 워크트리째는 못 지움, 캐시는 가능')]);
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', subtitle: SUBTITLE, width: W,
      body: [
        volumeLine(`ma-vol-${tag}`, INNER),
        filterBar(tag, 0),
        frame(`ma-table-${tag}`, 'Table', {layout: 'vertical', width: INNER}, [
          tableHead(tag, {all: 'mixed', build: 'mixed', deps: 'mixed', tree: 'mixed'}, {build: '고를 수 있는 5곳', deps: '고를 수 있는 6곳', tree: '고를 수 있는 4곳'}),
          rule(`ma-hr-${tag}`, INNER),
          ...rows.slice(0, tipAt), tip, ...rows.slice(tipAt),
          fold(tag, '작은 체크아웃 27 · 9.8 GB', 'off'),
        ]),
      ],
      footer: selectionFooter(tag, {summary: '빌드 캐시 3 · 의존성 1 · 워크트리 1 · 19.9 GB', warning: 'feat/overview-lenses는 폴더째 지워진다', note: '의존성은 다음 install이 다시 받는다', go: '정리'}),
    });
  }

  // The 끝난 것 filter, then the top-left checkbox: every finished checkout's caches.
  function sheetDone() {
    const tag = `${s}2`;
    const rows = CHECKOUTS.filter(checkout => checkout.state === 'done').map(checkout => tableRow(tag, checkout, {build: 'on', deps: 'on'}));
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', subtitle: SUBTITLE, width: W,
      body: [
        filterBar(tag, 1),
        frame(`ma-table-${tag}`, 'Table', {layout: 'vertical', width: INNER}, [
          tableHead(tag, {all: 'on', build: 'on', deps: 'on', tree: 'off'}, {build: '4곳 선택됨', deps: '5곳 선택됨', tree: '고를 수 있는 4곳'}),
          rule(`ma-hr-${tag}`, INNER),
          ...rows,
          fold(tag, '작은 체크아웃 14 · 캐시 2.1 GB 모두 선택', 'on'),
        ]),
      ],
      footer: selectionFooter(tag, {summary: '끝난 19곳의 빌드 캐시 · 의존성 · 23.1 GB', note: '의존성은 다음 install이 다시 받는다', go: '정리'}),
    });
  }

  // The states a row and the sheet carry besides a plain one: usage unreadable (B25),
  // measuring (B5), not measured (B6), in use with its reason (B14), another cleanup running (B20).
  function sheetStates() {
    const tag = `${s}3`;
    const notice = frame(`ma-notice-${tag}`, 'Usage unreadable', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: INNER, padding: ['$--spacing-xs', '$--spacing-sm'], cornerRadius: '$--radius-sm', fill: '$--muted'}, [
      icon(`ma-notice-g-${tag}`, 'circle-help', {size: 13, fill: '$--muted-foreground'}),
      text(`ma-notice-t-${tag}`, '지금 쓰는 중인지 확인할 수 없다', {size: '$--text-caption', fill: '$--subtle-foreground'}),
      spacer(`ma-notice-sp-${tag}`),
      screenButton(`ma-notice-b-${tag}`, '다시', {variant: 'secondary', height: 22}),
    ]);
    const byId = Object.fromEntries(CHECKOUTS.map(checkout => [checkout.id, checkout]));
    const tip = frame(`ma-tipwrap-${tag}`, 'Tooltip anchor', {padding: [0, 0, 0, SEL + NAME - 40]}, [tooltip(`ma-tip-${tag}`, '터미널에서 vite 실행 중')]);
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', subtitle: SUBTITLE, width: W,
      body: [
        notice,
        filterBar(tag, 0),
        frame(`ma-table-${tag}`, 'Table', {layout: 'vertical', width: INNER}, [
          tableHead(tag, {all: 'off', build: 'off', deps: 'off', tree: 'off'}, {build: '고를 수 있는 곳 없음', deps: '고를 수 있는 곳 없음', tree: '고를 수 있는 곳 없음'}),
          rule(`ma-hr-${tag}`, INNER),
          tableRow(tag, byId.main, {}, {variant: 'measuring'}),
          tableRow(tag, byId.rwb, {}, {variant: 'measuring'}),
          tableRow(tag, byId.mc),
          tip,
          tableRow(tag, byId.cas),
          tableRow(tag, byId.ol, {}, {variant: 'unmeasured'}),
        ]),
      ],
      footer: selectionFooter(tag, {summary: '비울 캐시가 없다', go: '다른 정리가 진행 중', goDisabled: true}),
    });
  }

  // B13: a filter no checkout matches.
  function sheetEmpty() {
    const tag = `${s}4`;
    return dialog(`ma-${tag}`, {
      title: '디스크 정리', subtitle: SUBTITLE, width: 640,
      body: [
        frame(`ma-fb-${tag}`, 'Filter bar', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: 592}, [segmented(`ma-seg-${tag}`, 3, {3: 0})]),
        frame(`ma-empty-${tag}`, 'Empty', {layout: 'vertical', gap: '$--spacing-sm', alignItems: 'center', justifyContent: 'center', width: 592, height: 120}, [
          text(`ma-empty-t-${tag}`, '이 필터에 맞는 체크아웃이 없다', {size: '$--text-body', fill: '$--muted-foreground'}),
          screenButton(`ma-empty-b-${tag}`, '전체 보기', {variant: 'secondary'}),
        ]),
      ],
      footer: selectionFooter(tag, {summary: '비울 캐시가 없다', go: '정리', goDisabled: true}),
    });
  }

  // -- confirm, running, result -----------------------------------------------------------------

  // B18: a worktree is in the selection, so the one confirmation names it. Neither button holds the keyboard.
  function confirmStep() {
    const width = 560;
    const inner = width - 48;
    return dialog(`cf-${s}`, {
      title: '워크트리 1개를 폴더째 지운다', subtitle: 'feat/overview-lenses 폴더가 통째로 사라지고 되돌릴 수 없다. 브랜치는 남는다.', width,
      body: [
        frame(`cf-list-${s}`, 'Worktrees', {layout: 'vertical', width: inner}, [
          rule(`cf-r0-${s}`, inner),
          frame(`cf-row-${s}`, 'feat/overview-lenses', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: inner, height: 32}, [
            icon(`cf-row-g-${s}`, 'folder-git-2', {size: 14, fill: '$--destructive'}),
            text(`cf-row-t-${s}`, 'feat/overview-lenses', {size: '$--text-body'}),
            spacer(`cf-row-sp-${s}`),
            text(`cf-row-v-${s}`, '3.6 GB', {size: '$--text-caption', mono: true, fill: '$--destructive'}),
          ]),
          rule(`cf-r1-${s}`, inner),
        ]),
        frame(`cf-sum-${s}`, 'Caches', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: inner}, [
          icon(`cf-sum-g-${s}`, 'hammer', {size: 12, fill: '$--muted-foreground'}),
          text(`cf-sum-t-${s}`, '캐시 칸 4곳 · 16.3 GB는 바로 비운다', {size: '$--text-caption', fill: '$--subtle-foreground', mono: true}),
        ]),
      ],
      footer: [spacer(`cf-fsp-${s}`), screenButton(`cf-back-${s}`, '돌아가기', {variant: 'secondary'}), screenButton(`cf-go-${s}`, '워크트리 1개와 캐시 정리', {variant: 'destructive', icon: 'trash-2'})],
    });
  }

  const outcomeLine = (width, id, glyph, tone, label, what, value, note) => frame(`${id}-${s}`, label, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width, height: 28}, [
    icon(`${id}-g-${s}`, glyph, {size: 13, fill: tone}),
    text(`${id}-t-${s}`, label, {size: '$--text-body'}),
    text(`${id}-w-${s}`, what, {size: '$--text-caption', fill: '$--subtle-foreground'}),
    ...(note ? [text(`${id}-n-${s}`, note, {size: '$--text-caption', fill: '$--muted-foreground'})] : []),
    spacer(`${id}-sp-${s}`),
    text(`${id}-v-${s}`, value, {size: '$--text-caption', mono: true, fill: '$--subtle-foreground'}),
  ]);

  // B20: the folders are already gone from their checkouts; the volume's free space is read once the deleting ends.
  function runningStep() {
    const width = 640;
    const inner = width - 48;
    return dialog(`rn-${s}`, {
      title: '비우는 중 · 3/7', subtitle: '시트를 닫아도 정리는 계속된다. 끝나면 다시 열었을 때 결과가 보인다.', width,
      body: [
        layerBar(`rn-bar-${s}`, [['$--primary', 3], ['$--muted', 4]], inner, 6),
        frame(`rn-list-${s}`, 'Progress', {layout: 'vertical', width: inner}, [
          rule(`rn-r0-${s}`, inner),
          outcomeLine(inner, 'rn-a', 'circle-check', '$--success', 'fix/remote-workspace-bridge', '빌드 캐시 · 의존성', '8.0 GB'),
          outcomeLine(inner, 'rn-b', 'circle-check', '$--success', 'feat/overview-issue-first', '빌드 캐시', '5.9 GB'),
          outcomeLine(inner, 'rn-c', 'loader-circle', '$--muted-foreground', 'feat/overview-lenses', '워크트리 삭제', '3.6 GB'),
          outcomeLine(inner, 'rn-d', 'circle-dashed', '$--muted-foreground', 'ci/runner-layout', '빌드 캐시', '2.4 GB'),
          rule(`rn-r1-${s}`, inner),
        ]),
      ],
      footer: [spacer(`rn-fsp-${s}`), {...screenButton(`rn-go-${s}`, '다른 정리가 진행 중', {variant: 'default', icon: 'trash-2'}), opacity: DISABLED}],
    });
  }

  // B22: the free space before and after leads; allocated total sits beside it as the lesser number.
  function resultStep() {
    const width = 640;
    const inner = width - 48;
    return dialog(`rs-${s}`, {
      title: '디스크 정리', subtitle: '선택한 칸을 정리했다', width,
      body: [
        frame(`rs-top-${s}`, 'Outcome', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'end', width: inner}, [
          frame(`rs-big-${s}`, 'Freed', {layout: 'vertical', gap: 2}, [
            text(`rs-big-l-${s}`, '디스크에서 잰 여유', {size: '$--text-caption', fill: '$--muted-foreground'}),
            frame(`rs-big-v-${s}`, 'Value', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'end'}, [
              text(`rs-big-a-${s}`, '1.6', {size: '$--text-title', mono: true, fill: '$--muted-foreground'}),
              icon(`rs-big-ar-${s}`, 'arrow-right', {size: 14, fill: '$--muted-foreground'}),
              text(`rs-big-b-${s}`, '18.3 GB', {size: '$--text-headline', weight: '600', mono: true}),
            ]),
          ]),
          spacer(`rs-sp-${s}`),
          text(`rs-note-${s}`, '할당 합계 17.5 GB · 실제로 늘어난 여유 16.7 GB', {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
        ]),
        frame(`rs-list-${s}`, 'Outcomes', {layout: 'vertical', width: inner}, [
          rule(`rs-r0-${s}`, inner),
          outcomeLine(inner, 'rs-a', 'circle-check', '$--success', 'fix/remote-workspace-bridge', '빌드 캐시 · 의존성', '8.0 GB'),
          outcomeLine(inner, 'rs-b', 'circle-check', '$--success', 'feat/overview-issue-first', '빌드 캐시', '5.9 GB'),
          outcomeLine(inner, 'rs-c', 'circle-check', '$--success', 'feat/overview-lenses', '워크트리 삭제', '3.6 GB'),
          outcomeLine(inner, 'rs-d', 'ban', '$--warning', 'ci/runner-layout', '빌드 캐시', '2.4 GB', '확인 사이에 빌드가 시작돼 건너뜀'),
          outcomeLine(inner, 'rs-e', 'circle-x', '$--destructive', 'prd/agent-tab-groups', '의존성', '0.3 GB', '지우지 못함 · 권한 없음'),
          rule(`rs-r1-${s}`, inner),
        ]),
      ],
      footer: [spacer(`rs-fsp-${s}`), screenButton(`rs-again-${s}`, '다시 검토', {variant: 'ghost', icon: 'refresh-cw'}), screenButton(`rs-close-${s}`, '닫기', {variant: 'secondary'})],
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
      labelled(`dc-l-en-${s}`, '입구 · 디스크 숫자 위에 올리면', '요약 줄의 디스크 숫자가 레이어별 내역을 툴팁으로 보여주고, 누르면 정리 시트가 열린다.', entry(false), 640),
      labelled(`dc-l-el-${s}`, '입구 · 디스크가 모자랄 때만', '볼륨 여유가 10 GB 미만일 때만 경고 칸이 생긴다. 비울 수 있는 양은 끝난 체크아웃의 캐시. 누르면 끝난 것 필터로 열린다.', entry(true), 640),
    ]),
    row(`dc-r1-${s}`, [
      labelled(`dc-l-m1-${s}`, '시트 · 전체에서 손으로 고르기', '행 하나 통째, 칸 몇 개, 워크트리 하나를 고른 상태. 열 머리와 왼쪽 위는 일부만 골라서 가운데 줄. 쓰는 중인 행은 흐리고 이유는 멈추면 뜬다.', sheetMixed(), W),
      labelled(`dc-l-m3-${s}`, '시트 · 측정 중, 재지 못함, 확인 불가', '측정 중인 행은 skeleton과 비활성 체크박스, 재지 못한 행은 흐리고 비어 있다. 쓰는 중인지 읽지 못하면 표 위에 한 줄과 다시, 체크박스는 모두 비활성.', sheetStates(), W),
    ]),
    row(`dc-r2-${s}`, [
      labelled(`dc-l-m2-${s}`, '시트 · 끝난 것 필터 + 왼쪽 위 체크', '가장 흔한 정리: 끝난 체크아웃의 빌드 캐시와 의존성을 클릭 두 번에. 워크트리는 켜지지 않는다.', sheetDone(), W),
      frame(`dc-c2-${s}`, 'Steps', {layout: 'vertical', gap: '$--spacing-xl'}, [
        labelled(`dc-l-cf-${s}`, '확인 · 워크트리가 든 정리만', '캐시와 의존성만 고르면 확인 없이 바로 실행된다. 어느 버튼도 기본 포커스가 아니고 돌아가기는 선택을 그대로 둔다.', confirmStep(), 560),
        labelled(`dc-l-em-${s}`, '필터 결과 없음', '맞는 체크아웃이 없으면 한 줄과 전체 보기. 정리는 비활성.', sheetEmpty(), 640),
      ]),
    ]),
    row(`dc-r3-${s}`, [
      labelled(`dc-l-rn-${s}`, '실행 중', '폴더는 체크아웃에서 곧바로 사라진다. 시트를 닫아도 계속되고 다시 열면 진행이나 결과가 보인다.', runningStep(), 640),
      labelled(`dc-l-rs-${s}`, '결과', '크기는 정리 전후 볼륨 여유로 말한다. 확인 사이에 상태가 바뀐 칸은 건너뛰고 이유를 적는다.', resultStep(), 640),
    ]),
  ])];
}
