// Draw the System / Foundations sheet from the canvas's own variables.
//
// The sheet is generator-owned: gen-pen-layout.mjs rebuilds it on every run
// from the document's variables, which gen-pen-tokens.mjs has already made
// agree with HideTheme.swift. Every fill and font size on it is a `$--`
// reference, so the swatches resolve through the same variables the boards
// use; the only literals are the hex and pixel labels, which are read off the
// variable values so they cannot disagree with the swatch beside them.

const WIDTH = 1180;

const id = (...parts) => ['fnd', ...parts].join('-').replace(/[^a-z0-9-]/gi, '-').toLowerCase();

const text = (key, content, {size = '$--text-body', fill = '$--color-primary', weight = 'normal', mono = false, width} = {}) => ({
  type: 'text', id: id(key), name: content.length > 24 ? content.slice(0, 24) : content,
  fill, content, fontFamily: mono ? '$--font-mono' : '$--font-ui', fontSize: size, fontWeight: weight,
  ...(width ? {textGrowth: 'fixed-width', width} : {}),
});

const value = (key, content) => text(key, content, {size: '$--text-micro', fill: '$--color-muted', mono: true});

const column = (key, name, children, extra = {}) => ({
  type: 'frame', id: id(key), name, layout: 'vertical', gap: '$--spacing-sm', width: 'fill_container', ...extra, children,
});
const row = (key, name, children, extra = {}) => ({
  type: 'frame', id: id(key), name, layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center', width: 'fill_container', ...extra, children,
});

function section(key, title, note, body) {
  return column(key, title, [
    column(`${key}-head`, 'Heading', [
      text(`${key}-title`, title.toUpperCase(), {size: '$--text-caption', fill: '$--color-muted', weight: '600'}),
      ...(note ? [text(`${key}-note`, note, {size: '$--text-subhead', fill: '$--color-secondary', width: 'fill_container'})] : []),
    ], {gap: '$--spacing-xs'}),
    body,
  ], {gap: '$--spacing-lg'});
}

const variable = (variables, name) => {
  const entry = variables[name];
  if (!entry) throw new Error(`Foundations sheet needs ${name}, which the canvas does not carry`);
  return entry.value;
};

function swatchLadder(variables, key, steps) {
  return row(key, 'Ladder', steps.map(([token, label], index) => ({
    type: 'frame', id: id(key, token), name: label, layout: 'vertical', justifyContent: 'end', gap: '$--spacing-xxs',
    width: 'fill_container', height: 140, padding: '$--spacing-lg', fill: `$--${token}`,
    stroke: '$--color-divider', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    cornerRadius: index === 0 ? ['$--radius-lg', 0, 0, '$--radius-lg'] : index === steps.length - 1 ? [0, '$--radius-lg', '$--radius-lg', 0] : 0,
    children: [
      text(`${key}-${token}-label`, label, {size: '$--text-body'}),
      value(`${key}-${token}-value`, String(variable(variables, `--${token}`))),
    ],
  })), {gap: 0, alignItems: 'start'});
}

function textInks(variables, key, inks) {
  return row(key, 'Inks', inks.map(([token, label]) => column(`${key}-${token}`, label, [
    text(`${key}-${token}-word`, label, {size: '$--text-display', fill: `$--${token}`, weight: '500'}),
    value(`${key}-${token}-value`, String(variable(variables, `--${token}`))),
  ], {width: 'fit_content', gap: '$--spacing-xs'})), {gap: '$--spacing-xxxl', alignItems: 'start'});
}

function chips(variables, key, entries) {
  return row(key, 'Chips', entries.map(([token, label]) => ({
    type: 'frame', id: id(key, token), name: label, layout: 'vertical', gap: '$--spacing-md', width: 'fill_container',
    padding: '$--spacing-lg', fill: '$--color-panel', stroke: '$--color-divider', strokeWidth: '$--size-hairline',
    strokeAlignment: 'inner', cornerRadius: '$--radius-lg',
    children: [
      {type: 'rectangle', id: id(key, token, 'dot'), name: 'Colour', width: 24, height: 24, fill: `$--${token}`, cornerRadius: '$--radius-sm'},
      column(`${key}-${token}-text`, 'Text', [
        text(`${key}-${token}-label`, label, {size: '$--text-body'}),
        value(`${key}-${token}-value`, String(variable(variables, `--${token}`))),
      ], {gap: '$--spacing-xxs'}),
    ],
  })), {gap: '$--spacing-md', alignItems: 'start'});
}

function typeScale(variables, key, steps) {
  return column(key, 'Scale', steps.map(([token, sample, mono]) => ({
    type: 'frame', id: id(key, token), name: token, layout: 'horizontal', alignItems: 'center', gap: '$--spacing-xl',
    width: 'fill_container', padding: ['$--spacing-md', 0], stroke: '$--color-divider', strokeWidth: {bottom: '$--size-hairline'}, strokeAlignment: 'inner',
    children: [
      {...value(`${key}-${token}-name`, token.replace('text-', '')), textGrowth: 'fixed-width', width: 120},
      {...value(`${key}-${token}-px`, `${variable(variables, `--${token}`)}px`), textGrowth: 'fixed-width', width: 60},
      text(`${key}-${token}-sample`, sample, {size: `$--${token}`, mono, weight: variable(variables, `--${token}`) >= 17 ? '600' : 'normal'}),
    ],
  })), {gap: 0});
}

function bars(variables, key, steps, {axis}) {
  return row(key, 'Steps', steps.map(([token, label]) => column(`${key}-${token}`, label, [
    axis === 'height'
      ? {type: 'rectangle', id: id(key, token, 'bar'), name: 'Bar', width: variable(variables, `--${token}`), height: 40, fill: '$--color-accent'}
      : {type: 'rectangle', id: id(key, token, 'box'), name: 'Box', width: 72, height: 72, fill: '$--color-panel', stroke: '$--color-divider', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', cornerRadius: `$--${token}`},
    value(`${key}-${token}-value`, `${label} ${variable(variables, `--${token}`)}`),
  ], {width: 'fit_content', alignItems: 'start', gap: '$--spacing-sm'})), {gap: '$--spacing-xl', alignItems: 'end'});
}

function sizes(variables, mapped, key, entries) {
  return column(key, 'Sizes', entries.map(([token, label]) => ({
    type: 'frame', id: id(key, token), name: label, layout: 'horizontal', alignItems: 'center', gap: '$--spacing-xl',
    width: 'fill_container', padding: ['$--spacing-sm', 0], stroke: '$--color-divider', strokeWidth: {bottom: '$--size-hairline'}, strokeAlignment: 'inner',
    children: [
      {...text(`${key}-${token}-label`, label, {size: '$--text-body'}), textGrowth: 'fixed-width', width: 200},
      {...value(`${key}-${token}-value`, `${variable(variables, `--${token}`)}`), textGrowth: 'fixed-width', width: 60},
      value(`${key}-${token}-swift`, `HideTheme.${mapped[`--${token}`]}`),
    ],
  })), {gap: 0});
}

export function foundations(variables, mapped) {
  const v = variables;
  return {
    type: 'frame', id: id('sheet'), name: 'System / Foundations', clip: true, width: WIDTH,
    fill: '$--color-background', layout: 'vertical', gap: '$--spacing-xxxl', padding: '$--spacing-xxl',
    children: [
      column('title', 'Title block', [
        text('title-name', 'Hide', {size: '$--text-display', weight: '600'}),
        text('title-sub', 'Single dark mode. Four-step surface ladder, 1px hairlines, no drop shadows. Every value on this sheet is read from the canvas variables that gen-pen-tokens.mjs writes from HideTheme.swift; the sheet itself is rebuilt by gen-pen-layout.mjs.', {size: '$--text-subhead', fill: '$--color-secondary', width: 'fill_container'}),
      ], {gap: '$--spacing-sm'}),

      section('surface', 'Surface ladder', null, swatchLadder(v, 'surface-ladder', [
        ['color-background', 'background'], ['color-sidebar', 'sidebar'], ['color-panel', 'panel'], ['color-elevated', 'elevated'], ['color-balloon', 'balloon'],
      ])),

      section('ink', 'Text', null, textInks(v, 'ink-set', [
        ['color-primary', 'primary'], ['color-secondary', 'secondary'], ['color-muted', 'muted'], ['color-accent', 'accent'],
      ])),

      section('semantic', 'Semantic', 'The only saturated colour in the chrome. Everything else stays on the neutral ladder.', chips(v, 'semantic-set', [
        ['color-agent-working', 'agent working'], ['color-success', 'success'], ['color-warning', 'warning'], ['color-danger', 'danger'],
      ])),

      section('pr', 'Pull request and graph', 'Status colours a row shows for a pull request, and the four lanes the Overview graph cycles through.', column('pr-body', 'Rows', [
        chips(v, 'pr-set', [['color-pr-open', 'open'], ['color-pr-merged', 'merged'], ['color-pr-closed', 'closed'], ['color-pr-draft', 'draft']]),
        chips(v, 'lane-set', [['color-graph-lane-1', 'lane 1'], ['color-graph-lane-2', 'lane 2'], ['color-graph-lane-3', 'lane 3'], ['color-graph-lane-4', 'lane 4']]),
        chips(v, 'diff-set', [['color-diff-added', 'diff added'], ['color-diff-removed', 'diff removed'], ['color-divider', 'divider'], ['color-hover-wash', 'hover wash']]),
      ], {gap: '$--spacing-md'})),

      section('type', 'Type scale', 'Inter with ss03 in the app. pen renders plain Inter, so glyph shapes here are approximate; sizes are exact. Terminal and editor sizes render in JetBrains Mono, standing in for SF Mono.', typeScale(v, 'type-scale', [
        ['text-micro', '12 unread agent updates'], ['text-caption', 'Keycap and metadata text'], ['text-body', 'Sidebar rows and tab titles use this size'],
        ['text-subhead', 'Pane header and section header'], ['text-title', 'Dialog title'], ['text-terminal-base', 'cargo test --workspace', true],
        ['text-editor-document', 'Markdown document body', true], ['text-headline', 'Sheet headline'], ['text-display', 'No pane selected'],
      ])),

      section('spacing', 'Spacing', null, bars(v, 'spacing-steps', [
        ['spacing-xxs', 'xxs'], ['spacing-xs', 'xs'], ['spacing-sm', 'sm'], ['spacing-md', 'md'], ['spacing-lg', 'lg'], ['spacing-xl', 'xl'], ['spacing-xxl', 'xxl'], ['spacing-xxxl', 'xxxl'],
      ], {axis: 'height'})),

      section('radius', 'Radius', 'Runs from the 4px selection to the 16px container. Borders are 1px; there are no drop shadows anywhere in the system.', bars(v, 'radius-steps', [
        ['radius-xs', 'xs'], ['radius-sm', 'sm'], ['radius-md', 'md'], ['radius-lg', 'lg'], ['radius-xl', 'xl'],
      ], {axis: 'radius'})),

      section('size', 'Sizes the layout is built on', 'The heights and widths a board reaches for before writing a number, and the HideTheme constant each one is.', sizes(v, mapped, 'size-set', [
        ['size-hairline', 'hairline'],
        ['size-pane-header', 'pane header'],
        ['size-tab-strip', 'tab strip'],
        ['size-control-compact', 'control, compact'],
        ['size-control-regular', 'control, regular'],
        ['size-icon-button-standard', 'icon button, standard'],
        ['size-keycap-height', 'keycap'],
        ['size-badge-height', 'badge'],
        ['size-checkout-row', 'checkout row'],
        ['size-sidebar-ideal', 'sidebar, ideal'],
        ['size-panel-ideal', 'panel, ideal'],
      ])),

      section('opacity', 'Opacity', 'Applied to a whole view, never baked into a colour; a wash colour carries its own alpha instead.', sizes(v, mapped, 'opacity-set', [
        ['opacity-secondary', 'secondary'],
        ['opacity-read-status', 'read status'],
        ['opacity-dimmed', 'dimmed'],
        ['opacity-disabled', 'disabled'],
      ])),
    ],
  };
}
