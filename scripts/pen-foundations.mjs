// Draw the System / Foundations sheet from the canvas's own variables.
//
// The sheet is generator-owned: gen-pen.mjs rebuilds it on every run from the
// document's variables, which the same run's token pass has already made agree
// with design/tokens.json. Every fill and font size on it is a `$--` reference, so
// the swatches resolve through the same variables the System parts use; the only
// literals are the hex and pixel labels, which are read off the variable values so
// they cannot disagree with the swatch beside them. The color ladder is the one
// section that varies by theme, drawn once inside a `Light` frame and once inside a
// `Dark` frame (`theme: {Mode: ...}`); the scale, spacing, radius and control/icon
// sizes below it do not change by theme, so they are drawn once.

const WIDTH = 1180;

const id = (...parts) => ['fnd', ...parts].join('-').replace(/[^a-z0-9-]/gi, '-').toLowerCase();

const text = (key, content, {size = '$--text-body', fill = '$--foreground', weight = 'normal', mono = false, width} = {}) => ({
  type: 'text', id: id(key), name: content.length > 24 ? content.slice(0, 24) : content,
  fill, content, fontFamily: mono ? '$--font-mono' : '$--font-ui', fontSize: size, fontWeight: weight,
  ...(width ? {textGrowth: 'fixed-width', width} : {}),
});

const value = (key, content) => text(key, content, {size: '$--text-micro', fill: '$--muted-foreground', mono: true});

const column = (key, name, children, extra = {}) => ({
  type: 'frame', id: id(key), name, layout: 'vertical', gap: '$--spacing-sm', width: 'fill_container', ...extra, children,
});
const row = (key, name, children, extra = {}) => ({
  type: 'frame', id: id(key), name, layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center', width: 'fill_container', ...extra, children,
});

function section(key, title, note, body) {
  return column(key, title, [
    column(`${key}-head`, 'Heading', [
      text(`${key}-title`, title.toUpperCase(), {size: '$--text-caption', fill: '$--muted-foreground', weight: '600'}),
      ...(note ? [text(`${key}-note`, note, {size: '$--text-subhead', fill: '$--subtle-foreground', width: 'fill_container'})] : []),
    ], {gap: '$--spacing-xs'}),
    body,
  ], {gap: '$--spacing-lg'});
}

const variable = (variables, name) => {
  const entry = variables[name];
  if (!entry) throw new Error(`Foundations sheet needs ${name}, which the canvas does not carry`);
  return entry.value;
};

// A color variable's own value is the Mode-themed array gen-pen.mjs writes
// ([{value}, {value, theme:{Mode:"Dark"}}]), not a plain hex string; a caption
// that prints "the hex under this swatch" has to pick the one entry that matches
// the palette block it is drawn inside, at generation time, the same theme the
// swatch's own `$--` fill will resolve to when Pen renders that block.
const hexFor = (variables, name, mode) => {
  const entry = variables[`--${name}`];
  if (!entry) throw new Error(`Foundations sheet needs --${name}, which the canvas does not carry`);
  const list = Array.isArray(entry.value) ? entry.value : [{value: entry.value}];
  const found = list.find(v => (mode === 'Dark' ? v.theme?.Mode === 'Dark' : !v.theme));
  if (!found) throw new Error(`--${name} has no ${mode} value`);
  return found.value;
};

function swatchLadder(variables, key, steps) {
  return row(key, 'Ladder', steps.map(([token, label], index) => ({
    type: 'frame', id: id(key, token), name: label, layout: 'vertical', justifyContent: 'end', gap: '$--spacing-xxs',
    width: 'fill_container', height: 120, padding: '$--spacing-lg', fill: `$--${token}`,
    stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    cornerRadius: index === 0 ? ['$--radius-lg', 0, 0, '$--radius-lg'] : index === steps.length - 1 ? [0, '$--radius-lg', '$--radius-lg', 0] : 0,
    children: [text(`${key}-${token}-label`, label, {size: '$--text-caption'})],
  })), {gap: 0, alignItems: 'start'});
}

function chips(variables, key, entries, mode) {
  return row(key, 'Chips', entries.map(([token, label]) => ({
    type: 'frame', id: id(key, token), name: label, layout: 'vertical', gap: '$--spacing-md', width: 'fill_container',
    padding: '$--spacing-lg', fill: '$--card', stroke: '$--border', strokeWidth: '$--size-hairline',
    strokeAlignment: 'inner', cornerRadius: '$--radius-lg',
    children: [
      {type: 'rectangle', id: id(key, token, 'dot'), name: 'Colour', width: 24, height: 24, fill: `$--${token}`, cornerRadius: '$--radius-sm'},
      column(`${key}-${token}-text`, 'Text', [
        text(`${key}-${token}-label`, label, {size: '$--text-body'}),
        value(`${key}-${token}-value`, hexFor(variables, token, mode)),
      ], {gap: '$--spacing-xxs'}),
    ],
  })), {gap: '$--spacing-md', alignItems: 'start'});
}

function textInks(variables, key, inks, mode) {
  // --text-display reads well across a full 1180 sheet with short names (the old
  // single-ladder Foundations); inside a PALETTE_WIDTH column carrying hide's
  // longer shadcn ink names (subtle-foreground, muted-foreground) it overflows,
  // so this row uses --text-title instead.
  return row(key, 'Inks', inks.map(([token, label]) => column(`${key}-${token}`, label, [
    text(`${key}-${token}-word`, label, {size: '$--text-title', fill: `$--${token}`, weight: '600'}),
    value(`${key}-${token}-value`, hexFor(variables, token, mode)),
  ], {width: 'fit_content', gap: '$--spacing-xs'})), {gap: '$--spacing-lg', alignItems: 'start'});
}

// The palette varies by theme, so it is the one section drawn twice: once inside a
// `Light`-tagged frame and once inside a `Dark`-tagged frame. Every swatch fill is
// a `$--` reference, resolving to that theme's own value; a caption's literal hex
// text is picked for the matching theme at generation time (see `hexFor`). A
// literal pixel width, not `fit_content`, is required here: everything nested
// inside uses `fill_container` to stretch to its row, and `fill_container` has no
// size to resolve against under a `fit_content` ancestor.
const PALETTE_WIDTH = 540;

function palette(variables, mode) {
  return column(`palette-${mode.toLowerCase()}`, mode, [
    column(`palette-${mode}-base`, 'Base', [
      swatchLadder(variables, `pal-${mode}-surface`, [['background', 'background'], ['sidebar', 'sidebar'], ['card', 'card'], ['popover', 'popover'], ['secondary', 'secondary']]),
      textInks(variables, `pal-${mode}-ink`, [['foreground', 'foreground'], ['subtle-foreground', 'subtle-foreground'], ['muted-foreground', 'muted-foreground'], ['primary', 'primary']], mode),
    ], {gap: '$--spacing-md'}),
    chips(variables, `pal-${mode}-accent`, [['accent-choice-lime', 'lime'], ['accent-choice-sky', 'sky'], ['accent-choice-violet', 'violet'], ['accent-choice-amber', 'amber']], mode),
    chips(variables, `pal-${mode}-semantic`, [['agent-working', 'agent working'], ['success', 'success'], ['warning', 'warning'], ['destructive', 'destructive']], mode),
    chips(variables, `pal-${mode}-pr`, [['pr-open', 'PR open'], ['pr-merged', 'PR merged'], ['pr-closed', 'PR closed'], ['pr-draft', 'PR draft']], mode),
    chips(variables, `pal-${mode}-diff`, [['diff-added', 'diff added'], ['diff-removed', 'diff removed'], ['border', 'border'], ['ring', 'ring']], mode),
  ], {gap: '$--spacing-lg', width: PALETTE_WIDTH, clip: true, padding: '$--spacing-xl', fill: '$--background', cornerRadius: '$--radius-lg', theme: {Mode: mode}});
}

function typeScale(variables, key, steps) {
  return column(key, 'Scale', steps.map(([token, sample, mono]) => ({
    type: 'frame', id: id(key, token), name: token, layout: 'horizontal', alignItems: 'center', gap: '$--spacing-xl',
    width: 'fill_container', padding: ['$--spacing-md', 0], stroke: '$--border', strokeWidth: {bottom: '$--size-hairline'}, strokeAlignment: 'inner',
    children: [
      {...value(`${key}-${token}-name`, token.replace('text-', '')), textGrowth: 'fixed-width', width: 130},
      {...value(`${key}-${token}-px`, `${variable(variables, `--${token}`)}px`), textGrowth: 'fixed-width', width: 60},
      text(`${key}-${token}-sample`, sample, {size: `$--${token}`, mono, weight: variable(variables, `--${token}`) >= 17 ? '600' : 'normal'}),
    ],
  })), {gap: 0});
}

function bars(variables, key, steps, {axis}) {
  return row(key, 'Steps', steps.map(([token, label]) => column(`${key}-${token}`, label, [
    axis === 'height'
      ? {type: 'rectangle', id: id(key, token, 'bar'), name: 'Bar', width: variable(variables, `--${token}`), height: 40, fill: '$--primary'}
      : {type: 'rectangle', id: id(key, token, 'box'), name: 'Box', width: 72, height: 72, fill: '$--card', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', cornerRadius: `$--${token}`},
    value(`${key}-${token}-value`, `${label} ${variable(variables, `--${token}`)}`),
  ], {width: 'fit_content', alignItems: 'start', gap: '$--spacing-sm'})), {gap: '$--spacing-xl', alignItems: 'end'});
}

function sizes(variables, key, entries) {
  return column(key, 'Sizes', entries.map(([token, label]) => ({
    type: 'frame', id: id(key, token), name: label, layout: 'horizontal', alignItems: 'center', gap: '$--spacing-xl',
    width: 'fill_container', padding: ['$--spacing-sm', 0], stroke: '$--border', strokeWidth: {bottom: '$--size-hairline'}, strokeAlignment: 'inner',
    children: [
      {...text(`${key}-${token}-label`, label, {size: '$--text-body'}), textGrowth: 'fixed-width', width: 200},
      {...value(`${key}-${token}-value`, `${variable(variables, `--${token}`)}`), textGrowth: 'fixed-width', width: 60},
      value(`${key}-${token}-token`, `--${token}`),
    ],
  })), {gap: 0});
}

export function foundations(variables) {
  const v = variables;
  return {
    type: 'frame', id: id('sheet'), name: 'System / Foundations', clip: true, width: WIDTH,
    fill: '$--background', layout: 'vertical', gap: '$--spacing-xxxl', padding: '$--spacing-xxl',
    children: [
      column('title', 'Title block', [
        text('title-name', 'Hide', {size: '$--text-display', weight: '600'}),
        text('title-sub', 'Medium density, light and dark. Four-step surface ladder, 1px hairlines, no drop shadows. Every value on this sheet is read from the canvas variables that gen-pen.mjs writes from design/tokens.json, and the sheet itself is redrawn by the same run.', {size: '$--text-subhead', fill: '$--subtle-foreground', width: 'fill_container'}),
      ], {gap: '$--spacing-sm'}),

      section('palette', 'Palette', 'The Mode theme axis: the same variables, each with a Light value and a Dark value.', row('palette-frames', 'Themes', [palette(v, 'Light'), palette(v, 'Dark')], {gap: '$--spacing-xl', alignItems: 'start'})),

      section('type', 'Type scale', 'Inter with ss03 in the app. pen renders plain Inter, so glyph shapes here are approximate; sizes are exact. Terminal and editor sizes render in JetBrains Mono, standing in for SF Mono.', typeScale(v, 'type-scale', [
        ['text-micro', '12 unread agent updates'], ['text-caption', 'Badge and metadata text'], ['text-body', 'Sidebar rows and control labels use this size'],
        ['text-subhead', 'Pane header and section header'], ['text-title', 'Dialog title'], ['text-terminal-base', 'cargo test --workspace', true],
        ['text-editor-document', 'Markdown document body', true], ['text-headline', 'Sheet headline'], ['text-display', 'No pane selected'],
      ])),

      section('spacing', 'Spacing', null, bars(v, 'spacing-steps', [
        ['spacing-xxs', 'xxs'], ['spacing-xs', 'xs'], ['spacing-sm', 'sm'], ['spacing-md', 'md'], ['spacing-lg', 'lg'], ['spacing-xl', 'xl'], ['spacing-xxl', 'xxl'], ['spacing-xxxl', 'xxxl'],
      ], {axis: 'height'})),

      section('radius', 'Radius', 'Runs from the 4px selection to the 16px container. Borders are 1px; there are no drop shadows anywhere in the system.', bars(v, 'radius-steps', [
        ['radius-xs', 'xs'], ['radius-sm', 'sm'], ['radius-md', 'md'], ['radius-lg', 'lg'], ['radius-xl', 'xl'],
      ], {axis: 'radius'})),

      section('control', 'Control and icon sizes', 'The medium-density scale every System part is built on (D-07): a body control is --size-control tall, its icon --size-icon.', sizes(v, 'control-set', [
        ['size-control-sm', 'control, small'],
        ['size-control', 'control, default'],
        ['size-control-lg', 'control, large'],
        ['size-icon-sm', 'icon, small'],
        ['size-icon', 'icon, default'],
        ['size-checkbox', 'checkbox / radio / switch track'],
        ['size-badge-height', 'badge'],
        ['size-keycap-height', 'kbd'],
      ])),

      section('size', 'Sizes the layout is built on', 'The heights and widths a board reaches for before writing a number.', sizes(v, 'size-set', [
        ['size-hairline', 'hairline'],
        ['size-pane-header', 'pane header'],
        ['size-tab-strip', 'tab strip'],
        ['size-checkout-row', 'checkout row'],
        ['size-sidebar-ideal', 'sidebar, ideal'],
        ['size-panel-ideal', 'panel, ideal'],
        ['size-settings-control-w', 'settings control width'],
        ['size-worktree-dialog', 'dialog width'],
      ])),

      section('opacity', 'Opacity', 'Applied to a whole view, never baked into a colour; a wash colour carries its own alpha instead.', sizes(v, 'opacity-set', [
        ['opacity-secondary', 'secondary'],
        ['opacity-read-status', 'read status'],
        ['opacity-dimmed', 'dimmed'],
        ['opacity-disabled', 'disabled'],
      ])),
    ],
  };
}

// The variables the sheet reads, for a fixture that has to carry them. Found by
// building the sheet against a recording stub rather than kept as a list that
// would drift from the builder above.
export function requiredVariables() {
  const seen = new Set();
  const stub = new Proxy({}, {get: (_, name) => { seen.add(name); return {type: 'number', value: 0}; }});
  foundations(stub);
  return [...seen];
}
