// Draw every `System / <Part>` sheet the gallery names (web/src/gallery/manifest.ts,
// checked by check-pen-gallery.mjs) from primitives in hide's own idiom, re-dressed
// with hide tokens and matched 1:1 to what web/src/components/ui/*.tsx renders.
//
// Each sheet is generator-owned, the way Foundations is: gen-pen.mjs rebuilds every
// part's content on every run from the document's own (already-renamed) variables,
// so a fill is always a `$--` reference that resolves per `Mode` theme. Width and
// height are the one exception (a variable-bound width/height renders at zero in the
// installed Pen toolchain), so every frame's own size is a plain number read from
// tokens.json at generation time, never a `$--size-*` reference.
//
// A part draws ONE reusable master; every state in both the Light and Dark frame is
// a `ref` of that same master id with descendant overrides, never a redrawn copy or
// a second master per theme - the master's own `$--` fills already resolve per
// theme from wherever its instance sits, which is the entire point of Pen's `Mode`
// theme axis (pencil:shadcn works the same way: one master, instanced under both a
// Light-tagged and a Dark-tagged frame). Only the ref ids differ between the two
// frames, since two instances of one master still need two distinct node ids.
//
// The four System masters this replaces (Icon Button, Panel Tab, Badge, Keycap) are
// still referenced by many `Component /` sheets; `foldLegacyMasters` finds each by
// its existing node id and folds it into the part sheet that plays its old role,
// unchanged beyond the variable rename gen-pen already applied, so no Component ref
// dangles. Badge's old master doubles as the new Badge part's master directly
// (same shape: optional leading icon + label); Icon Button's old master doubles as
// Button's Icon/Icon Small states directly (same shape: one centered icon, no
// label). Panel Tab and Keycap are a different idiom from Tabs and Kbd, so their
// masters live on as an inert `Legacy master` sibling of Light/Dark, ignored by
// check-pen-gallery.mjs (it only reads children named exactly `Light` or `Dark`).

const UI = '$--font-ui';
const MONO = '$--font-mono';

function num(tokens, name) {
  const token = tokens[name];
  if (!token) throw new Error(`pen-system needs token ${name}, which tokens.json does not carry`);
  return token.type === 'alias' ? num(tokens, token.value) : token.value;
}

function text(id, content, {fill = '$--foreground', size = '$--text-body', weight = '400', mono = false, width} = {}) {
  return {
    type: 'text', id, name: content.length > 28 ? content.slice(0, 28) : content, content, fill,
    fontFamily: mono ? MONO : UI, fontSize: size, fontWeight: weight,
    ...(width ? {textGrowth: 'fixed-width', width} : {}),
  };
}

function icon(id, glyph, {size = 14, fill = '$--foreground', enabled = true} = {}) {
  return {type: 'icon', id, name: 'Glyph', enabled, width: size, height: size, icon: glyph, library: 'lucide', fill};
}

function frame(id, name, props, children) {
  return {type: 'frame', id, name, children, ...props};
}

function ref(id, masterId, name, overrides = {}, descendants) {
  return {id, type: 'ref', ref: masterId, name, ...overrides, ...(descendants ? {descendants} : {})};
}

// A state's top-level node name must be exactly the manifest's state string
// (check-pen-gallery.mjs); the instance's own id makes the cell's ids unique.
function cell(stateName, instance) {
  const key = instance.id;
  return frame(`${key}-cell`, stateName, {layout: 'vertical', gap: '$--spacing-xs', alignItems: 'start', width: 'fit_content'}, [
    text(`${key}-cap`, stateName, {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
    instance,
  ]);
}

// check-pen-gallery.mjs reads this frame's DIRECT children as the state list, so a
// cell can never sit inside a grouping "Row" frame; Pen's own layout has no wrap,
// so a Light or Dark frame with many states is a single wide horizontal strip
// rather than a wrapped grid.
function themeFrame(id, mode, cells) {
  return frame(id, mode, {
    theme: {Mode: mode}, layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'end', padding: '$--spacing-lg',
    fill: '$--background', width: 'fit_content', cornerRadius: '$--radius-md',
  }, cells);
}

function masterCard(id, label, node) {
  return frame(id, label, {layout: 'vertical', gap: '$--spacing-xs', alignItems: 'start', width: 'fit_content'}, [
    text(`${id}-cap`, label, {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
    node,
  ]);
}

function buildSheet(id, name, spec, masters, lightCells, darkCells) {
  return frame(id, name, {
    layout: 'vertical', gap: '$--spacing-xl', padding: '$--spacing-xl', fill: '$--card',
    cornerRadius: '$--radius-lg', width: 'fit_content',
  }, [
    text(`${id}-title`, name.replace('System / ', ''), {size: '$--text-headline', weight: '600'}),
    text(`${id}-spec`, spec, {size: '$--text-caption', fill: '$--subtle-foreground', width: 820}),
    ...(masters.length ? [frame(`${id}-masters`, 'Masters', {layout: 'horizontal', gap: '$--spacing-xl', alignItems: 'start'}, masters)] : []),
    themeFrame(`${id}-light`, 'Light', lightCells),
    themeFrame(`${id}-dark`, 'Dark', darkCells),
  ]);
}

// -- menu idiom shared by Select, Dropdown Menu, Context Menu, Command -------------

const MENU_ITEM_STATE = {default: {}, highlighted: {fill: '$--accent', text: '$--accent-foreground'}, disabled: {text: '$--muted-foreground'}, destructive: {text: '$--destructive'}};

function menuItem(id, label, state = 'default', glyph) {
  const s = MENU_ITEM_STATE[state];
  return frame(id, 'Item', {
    layout: 'horizontal', alignItems: 'center', gap: '$--spacing-sm', cornerRadius: '$--radius-xs',
    padding: ['$--spacing-xs', '$--spacing-sm'], fill: s.fill ?? '#00000000',
    ...(state === 'disabled' ? {opacity: 0.45} : {}),
  }, [
    ...(glyph ? [icon(`${id}-i`, glyph, {size: 14, fill: s.text ?? '$--muted-foreground'})] : []),
    text(`${id}-t`, label, {fill: s.text ?? '$--foreground', size: '$--text-body'}),
  ]);
}

function menuSeparator(id) {
  return frame(id, 'Separator', {height: 1, width: 'fill_container', fill: '$--border'}, []);
}

function menuContent(id, width, children) {
  return frame(id, 'Content', {
    layout: 'vertical', gap: 0, padding: '$--spacing-xxs', width, cornerRadius: '$--radius-md',
    fill: '$--popover', stroke: '$--border', strokeWidth: 1, strokeAlignment: 'inner',
  }, children);
}

// -- Button (also carries the folded Icon Button master) ---------------------------

function buildButton(tokens, legacy) {
  const H = num(tokens, '--size-control'), HS = num(tokens, '--size-control-sm'), HL = num(tokens, '--size-control-lg');
  const DISABLED = num(tokens, '--opacity-disabled');
  const master = frame('btn-m', 'Button', {
    reusable: true, height: H, cornerRadius: '$--radius-sm', fill: '$--primary',
    layout: 'horizontal', alignItems: 'center', justifyContent: 'center', gap: '$--spacing-xs', padding: [0, '$--spacing-md'],
  }, [icon('btn-ic', 'sparkles', {fill: '$--primary-foreground', enabled: false}), text('btn-lb', 'Button', {fill: '$--primary-foreground', weight: '500'})]);

  function states(suffix) {
    const variant = (key, name, overrides, fg) => ref(`btn-${key}-${suffix}`, master.id, name, overrides, {'btn-ic': {fill: fg}, 'btn-lb': {fill: fg}});
    return [
      cell('Default', variant('default', 'Default', {}, '$--primary-foreground')),
      cell('Default Hover', variant('defhover', 'Default Hover', {opacity: 0.9}, '$--primary-foreground')),
      cell('Default Focus', variant('deffocus', 'Default Focus', {stroke: '$--ring', strokeWidth: 1, strokeAlignment: 'outer'}, '$--primary-foreground')),
      cell('Default Disabled', variant('defdis', 'Default Disabled', {opacity: DISABLED}, '$--primary-foreground')),
      cell('Pending', variant('pending', 'Pending', {opacity: 0.72}, '$--primary-foreground')),
      cell('Secondary', variant('secondary', 'Secondary', {fill: '$--secondary', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, '$--subtle-foreground')),
      cell('Secondary Hover', variant('sechover', 'Secondary Hover', {fill: '$--accent', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, '$--accent-foreground')),
      cell('Outline', variant('outline', 'Outline', {fill: '$--background', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, '$--foreground')),
      cell('Ghost', variant('ghost', 'Ghost', {fill: '#00000000'}, '$--subtle-foreground')),
      cell('Ghost Hover', variant('ghosthover', 'Ghost Hover', {fill: '$--accent'}, '$--accent-foreground')),
      cell('Destructive', variant('destructive', 'Destructive', {fill: '$--destructive'}, '$--destructive-foreground')),
      cell('Destructive Hover', variant('desthover', 'Destructive Hover', {fill: '$--destructive', opacity: 0.9}, '$--destructive-foreground')),
      cell('Link', variant('link', 'Link', {fill: '#00000000'}, '$--primary')),
      cell('Small', variant('small', 'Small', {height: HS, padding: [0, '$--spacing-sm']}, '$--primary-foreground')),
      cell('Large', variant('large', 'Large', {height: HL, padding: [0, '$--spacing-lg']}, '$--primary-foreground')),
      cell('Icon', ref(`btn-icon-${suffix}`, legacy.iconButton.master.id, 'Icon', {width: H, height: H})),
      cell('Icon Small', ref(`btn-iconsm-${suffix}`, legacy.iconButton.master.id, 'Icon Small', {})),
    ];
  }
  return buildSheet('sys-button', 'System / Button', 'shadcn Button variants and sizes: rounded-sm, text-body medium, gap-xs, disabled at --opacity-disabled. Icon and Icon Small reuse the folded Icon Button master below.',
    [masterCard('btn-master-card', 'Master', master), masterCard('btn-legacy-card', 'Legacy master (Icon Button)', legacy.iconButton.master)], states('l'), states('d'));
}

// -- Input ---------------------------------------------------------------------

function buildInput(tokens) {
  const H = num(tokens, '--size-control');
  const master = frame('inp-m', 'Input', {
    reusable: true, height: H, width: 220, cornerRadius: '$--radius-sm', fill: '$--background',
    stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    layout: 'horizontal', alignItems: 'center', padding: [0, '$--spacing-sm'],
  }, [text('inp-t', 'Value', {fill: '$--foreground'})]);
  function states(suffix) {
    return [
      cell('Default', ref(`inp-default-${suffix}`, master.id, 'Default')),
      cell('Placeholder', ref(`inp-placeholder-${suffix}`, master.id, 'Placeholder', {}, {'inp-t': {content: 'Search branches', fill: '$--muted-foreground'}})),
      cell('Filled', ref(`inp-filled-${suffix}`, master.id, 'Filled', {}, {'inp-t': {content: 'feature/theme-reset'}})),
      cell('Focus', ref(`inp-focus-${suffix}`, master.id, 'Focus', {stroke: '$--ring', strokeWidth: 1})),
      cell('Invalid', ref(`inp-invalid-${suffix}`, master.id, 'Invalid', {stroke: '$--destructive', strokeWidth: 1})),
      cell('Disabled', ref(`inp-disabled-${suffix}`, master.id, 'Disabled', {opacity: num(tokens, '--opacity-disabled')})),
      cell('Mono', ref(`inp-mono-${suffix}`, master.id, 'Mono', {}, {'inp-t': {content: 'origin/main', fontFamily: MONO}})),
    ];
  }
  return buildSheet('sys-input', 'System / Input', 'shadcn Input: height --size-control, rounded-sm, hairline border-input. `mono` swaps the interface face for machine text.', [masterCard('inp-master-card', 'Master', master)], states('l'), states('d'));
}

// -- Select ----------------------------------------------------------------------

function buildSelect(tokens) {
  const H = num(tokens, '--size-control'), W = num(tokens, '--size-settings-control-w');
  const master = frame('sel-m', 'Select Trigger', {
    reusable: true, height: H, width: W, cornerRadius: '$--radius-sm', fill: '$--background',
    stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    layout: 'horizontal', alignItems: 'center', justifyContent: 'space_between', padding: [0, '$--spacing-sm'],
  }, [text('sel-t', 'Value', {fill: '$--foreground'}), icon('sel-i', 'chevron-down', {size: 14, fill: '$--muted-foreground'})]);
  function states(suffix) {
    const openContent = menuContent(`sel-content-${suffix}`, W, [
      menuItem(`sel-item1-${suffix}`, 'Dark'), menuItem(`sel-item2-${suffix}`, 'Light', 'highlighted'), menuItem(`sel-item3-${suffix}`, 'System'),
    ]);
    return [
      cell('Default', ref(`sel-default-${suffix}`, master.id, 'Default')),
      cell('Placeholder', ref(`sel-placeholder-${suffix}`, master.id, 'Placeholder', {}, {'sel-t': {content: 'Choose a theme', fill: '$--muted-foreground'}})),
      cell('Focus', ref(`sel-focus-${suffix}`, master.id, 'Focus', {stroke: '$--ring', strokeWidth: 1})),
      cell('Disabled', ref(`sel-disabled-${suffix}`, master.id, 'Disabled', {opacity: num(tokens, '--opacity-disabled')})),
      cell('Open', frame(`sel-openwrap-${suffix}`, 'Open', {layout: 'vertical', gap: '$--spacing-xxs'}, [ref(`sel-opentrig-${suffix}`, master.id, 'Trigger'), openContent])),
    ];
  }
  return buildSheet('sys-select', 'System / Select', 'shadcn Select over Radix: trigger matches Input height, content shares the menu idiom with Dropdown Menu and Command.', [masterCard('sel-master-card', 'Master', master)], states('l'), states('d'));
}

// -- Checkbox ----------------------------------------------------------------------

function buildCheckbox(tokens) {
  const S = num(tokens, '--size-checkbox');
  const master = frame('chk-m', 'Checkbox', {
    reusable: true, width: S, height: S, cornerRadius: '$--radius-xs', fill: '$--background',
    stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    layout: 'horizontal', alignItems: 'center', justifyContent: 'center',
  }, [icon('chk-i', 'check', {size: 12, fill: '$--primary-foreground', enabled: false})]);
  function states(suffix) {
    const checked = {fill: '$--primary', stroke: '$--primary', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'};
    return [
      cell('Unchecked', ref(`chk-off-${suffix}`, master.id, 'Unchecked')),
      cell('Checked', ref(`chk-on-${suffix}`, master.id, 'Checked', checked, {'chk-i': {enabled: true}})),
      cell('Focus', ref(`chk-focus-${suffix}`, master.id, 'Focus', {stroke: '$--ring', strokeWidth: 1})),
      cell('Disabled', ref(`chk-disabled-${suffix}`, master.id, 'Disabled', {opacity: num(tokens, '--opacity-disabled')})),
      cell('Checked Disabled', ref(`chk-ondis-${suffix}`, master.id, 'Checked Disabled', {...checked, opacity: num(tokens, '--opacity-disabled')}, {'chk-i': {enabled: true}})),
    ];
  }
  return buildSheet('sys-checkbox', 'System / Checkbox', 'shadcn Checkbox over Radix: --size-checkbox square, rounded-xs, checked fills --primary.', [masterCard('chk-master-card', 'Master', master)], states('l'), states('d'));
}

// -- Switch --------------------------------------------------------------------

function buildSwitch(tokens) {
  const W = num(tokens, '--size-control'), H = num(tokens, '--size-checkbox'), T = num(tokens, '--size-icon');
  const master = frame('sw-m', 'Switch', {
    reusable: true, width: W, height: H, cornerRadius: '$--radius-xl', fill: '$--input',
    layout: 'horizontal', alignItems: 'center', padding: ['$--spacing-xxs'],
  }, [{type: 'rectangle', id: 'sw-thumb', name: 'Thumb', width: T, height: T, fill: '$--background', cornerRadius: '$--radius-xl'}]);
  function states(suffix) {
    return [
      cell('Off', ref(`sw-off-${suffix}`, master.id, 'Off')),
      cell('On', ref(`sw-on-${suffix}`, master.id, 'On', {fill: '$--primary', justifyContent: 'end'})),
      cell('Focus', ref(`sw-focus-${suffix}`, master.id, 'Focus', {stroke: '$--ring', strokeWidth: 1, strokeAlignment: 'outer'})),
      cell('Disabled', ref(`sw-disabled-${suffix}`, master.id, 'Disabled', {opacity: num(tokens, '--opacity-disabled')})),
    ];
  }
  return buildSheet('sys-switch', 'System / Switch', 'shadcn Switch over Radix: --size-control wide, --size-checkbox tall track, --size-icon thumb.', [masterCard('sw-master-card', 'Master', master)], states('l'), states('d'));
}

// -- Radio Group -----------------------------------------------------------------

function buildRadioGroup(tokens) {
  const S = num(tokens, '--size-checkbox');
  const master = frame('rad-m', 'Radio Item', {
    reusable: true, width: S, height: S, cornerRadius: S / 2, fill: '$--background',
    stroke: '$--input', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    layout: 'horizontal', alignItems: 'center', justifyContent: 'center',
  }, [{type: 'ellipse', id: 'rad-dot', name: 'Indicator', width: 6, height: 6, fill: '$--primary', enabled: false}]);
  function states(suffix) {
    const on = ref(`rad-on-${suffix}`, master.id, 'On', {stroke: '$--primary', strokeWidth: '$--size-hairline'}, {'rad-dot': {enabled: true}});
    const off = ref(`rad-off-${suffix}`, master.id, 'Off');
    const group = frame(`rad-group-${suffix}`, 'Default', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'center'}, [on, off]);
    return [
      cell('Default', group),
      cell('Focus', ref(`rad-focus-${suffix}`, master.id, 'Focus', {stroke: '$--ring', strokeWidth: 1})),
      cell('Disabled', ref(`rad-disabled-${suffix}`, master.id, 'Disabled', {opacity: num(tokens, '--opacity-disabled')})),
    ];
  }
  return buildSheet('sys-radio', 'System / Radio Group', 'shadcn Radio Group over Radix: --size-checkbox circle, --spacing-sm gap between items, filled dot indicator.', [masterCard('rad-master-card', 'Master', master)], states('l'), states('d'));
}

// -- Toggle Group ------------------------------------------------------------------

function buildToggleGroup(tokens) {
  const H = num(tokens, '--size-control-sm');
  const master = frame('tog-m', 'Toggle Group Item', {
    reusable: true, height: H, cornerRadius: '$--radius-xs', layout: 'horizontal', alignItems: 'center',
    justifyContent: 'center', padding: [0, '$--spacing-sm'],
  }, [text('tog-t', 'Split', {fill: '$--subtle-foreground'})]);
  function states(suffix) {
    const off1 = ref(`tog-off1-${suffix}`, master.id, 'Tabbed', {}, {'tog-t': {content: 'Tabbed'}});
    const on = ref(`tog-on-${suffix}`, master.id, 'Split', {fill: '$--secondary'}, {'tog-t': {content: 'Split', fill: '$--foreground'}});
    const off2 = ref(`tog-off2-${suffix}`, master.id, 'Stacked', {}, {'tog-t': {content: 'Stacked'}});
    const group = frame(`tog-group-${suffix}`, 'Default', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'}, [off1, on, off2]);
    return [
      cell('Default', group),
      cell('Hover', ref(`tog-hover-${suffix}`, master.id, 'Hover', {}, {'tog-t': {content: 'Tabbed', fill: '$--foreground'}})),
      cell('Focus', ref(`tog-focus-${suffix}`, master.id, 'Focus', {stroke: '$--ring', strokeWidth: 1}, {'tog-t': {content: 'Tabbed'}})),
      cell('Disabled', ref(`tog-disabled-${suffix}`, master.id, 'Disabled', {opacity: num(tokens, '--opacity-disabled')}, {'tog-t': {content: 'Tabbed'}})),
    ];
  }
  return buildSheet('sys-toggle', 'System / Toggle Group', 'shadcn Toggle Group over Radix (Layout, Provider, Theme choices): a --spacing-xxs padded track, the active item on --secondary.', [masterCard('tog-master-card', 'Master', master)], states('l'), states('d'));
}

// -- Slider ------------------------------------------------------------------------

function buildSlider(tokens) {
  const THUMB = num(tokens, '--size-checkbox'), TRACK = num(tokens, '--spacing-xs');
  const master = frame('sld-m', 'Slider', {reusable: true, width: 160, height: THUMB, layout: 'horizontal', alignItems: 'center'}, [
    frame('sld-track', 'Track', {width: 'fill_container', height: TRACK, cornerRadius: '$--radius-xl', fill: '$--secondary'}, [
      {type: 'rectangle', id: 'sld-range', name: 'Range', width: 96, height: TRACK, fill: '$--primary', cornerRadius: '$--radius-xl'},
    ]),
    {type: 'ellipse', id: 'sld-thumb', name: 'Thumb', x: 88, width: THUMB, height: THUMB, fill: '$--background', stroke: '$--primary', strokeWidth: '$--size-hairline'},
  ]);
  function states(suffix) {
    return [
      cell('Default', ref(`sld-default-${suffix}`, master.id, 'Default')),
      cell('Focus', ref(`sld-focus-${suffix}`, master.id, 'Focus', {}, {'sld-thumb': {stroke: '$--ring', strokeWidth: 1}})),
      cell('Disabled', ref(`sld-disabled-${suffix}`, master.id, 'Disabled', {opacity: num(tokens, '--opacity-disabled')})),
    ];
  }
  return buildSheet('sys-slider', 'System / Slider', 'shadcn Slider over Radix: --spacing-xs track on --secondary, --primary range, --size-checkbox thumb.', [masterCard('sld-master-card', 'Master', master)], states('l'), states('d'));
}

// -- Tabs (also carries the folded Panel Tab legacy master) -----------------------

function buildTabs(tokens, legacy) {
  const H = num(tokens, '--size-control-sm');
  const master = frame('tab-m', 'Tabs Trigger', {
    reusable: true, height: H, cornerRadius: '$--radius-xs', layout: 'horizontal', alignItems: 'center',
    justifyContent: 'center', padding: [0, '$--spacing-sm'],
  }, [text('tab-t', 'History', {fill: '$--subtle-foreground'})]);
  function states(suffix) {
    const active = ref(`tab-active-${suffix}`, master.id, 'Overview', {fill: '$--secondary'}, {'tab-t': {content: 'Overview', fill: '$--foreground'}});
    const inactive = ref(`tab-inactive-${suffix}`, master.id, 'History', {}, {'tab-t': {content: 'History'}});
    const list = frame(`tab-list-${suffix}`, 'Default', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'}, [active, inactive]);
    return [
      cell('Default', list),
      cell('Hover', ref(`tab-hover-${suffix}`, master.id, 'Hover', {}, {'tab-t': {content: 'History', fill: '$--foreground'}})),
      cell('Focus', ref(`tab-focus-${suffix}`, master.id, 'Focus', {stroke: '$--ring', strokeWidth: 1}, {'tab-t': {content: 'History'}})),
      cell('Disabled', ref(`tab-disabled-${suffix}`, master.id, 'Disabled', {opacity: num(tokens, '--opacity-disabled')}, {'tab-t': {content: 'History'}})),
    ];
  }
  return buildSheet('sys-tabs', 'System / Tabs',
    'shadcn Tabs over Radix: a --spacing-xxs padded track, the active trigger on --secondary. Panel Tab (the pane-header underline tab) is a different idiom and lives on as a Legacy master below.',
    [masterCard('tab-master-card', 'Master', master), masterCard('tab-legacy-card', 'Legacy master (Panel Tab)', legacy.panelTab.master)], states('l'), states('d'));
}

// -- Badge (reuses the folded Badge master directly) -------------------------------

function buildBadge(tokens, legacy) {
  const master = legacy.badge.master, iconId = legacy.badge.iconId, labelId = legacy.badge.labelId;
  function states(suffix) {
    const variant = (key, name, overrides, textFill) => ref(`bdg-${key}-${suffix}`, master.id, name, overrides, {[iconId]: {enabled: false}, [labelId]: {content: name, fill: textFill}});
    return [
      cell('Default', variant('default', 'Default', {fill: '$--primary', strokeWidth: 0}, '$--primary-foreground')),
      cell('Secondary', variant('secondary', 'Secondary', {fill: '$--secondary', strokeWidth: 0}, '$--subtle-foreground')),
      cell('Destructive', variant('destructive', 'Destructive', {fill: '$--destructive', strokeWidth: 0}, '$--destructive-foreground')),
      cell('Outline', variant('outline', 'Outline', {fill: '#00000000', stroke: '$--border'}, '$--subtle-foreground')),
    ];
  }
  return buildSheet('sys-badge', 'System / Badge', 'shadcn Badge: --size-badge-height tall, rounded-xs, text-micro. The master is the folded former System / Badge master, unchanged beyond its variable rename.', [masterCard('bdg-master-card', 'Master', master)], states('l'), states('d'));
}

// -- Kbd (also carries the folded Keycap legacy master) ---------------------------

function buildKbd(tokens, legacy) {
  const H = num(tokens, '--size-keycap-height');
  const master = frame('kbd-m', 'Kbd', {
    reusable: true, height: H, cornerRadius: '$--radius-xs', fill: '$--secondary',
    layout: 'horizontal', alignItems: 'center', justifyContent: 'center', padding: [0, '$--spacing-xs'],
  }, [text('kbd-t', '⌘K', {fill: '$--subtle-foreground', size: '$--text-caption', weight: '500'})]);
  function states(suffix) {
    const mod = ref(`kbd-mod-${suffix}`, master.id, 'Modifier', {}, {'kbd-t': {content: '⇧'}});
    const key = ref(`kbd-key-${suffix}`, master.id, 'Key', {}, {'kbd-t': {content: 'K'}});
    return [
      cell('Default', ref(`kbd-default-${suffix}`, master.id, 'Default')),
      cell('Group', frame(`kbd-groupwrap-${suffix}`, 'Group', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [mod, key])),
    ];
  }
  return buildSheet('sys-kbd', 'System / Kbd',
    'shadcn Kbd: --size-keycap-height tall, rounded-xs, --secondary fill, text-caption. Keycap (the divider-bordered mono keycap) is a different idiom and lives on as a Legacy master below.',
    [masterCard('kbd-master-card', 'Master', master), masterCard('kbd-legacy-card', 'Legacy master (Keycap)', legacy.keycap.master)], states('l'), states('d'));
}

// -- Separator -----------------------------------------------------------------

function buildSeparator(tokens) {
  const H = num(tokens, '--size-hairline');
  function states(suffix) {
    return [
      cell('Horizontal', frame(`sep-h-${suffix}`, 'Horizontal', {width: 160, height: H, fill: '$--border'}, [])),
      cell('Vertical', frame(`sep-v-${suffix}`, 'Vertical', {width: H, height: 48, fill: '$--border'}, [])),
    ];
  }
  return buildSheet('sys-separator', 'System / Separator', 'shadcn Separator over Radix: --size-hairline on --border, either axis.', [], states('l'), states('d'));
}

// -- Dropdown Menu (also the model Context Menu shares) ----------------------------

function buildDropdownMenu(tokens) {
  const H = num(tokens, '--size-control'), W = num(tokens, '--size-settings-control-w');
  const master = frame('ddm-m', 'Trigger', {
    reusable: true, height: H, cornerRadius: '$--radius-sm', fill: '$--secondary', stroke: '$--border',
    strokeWidth: '$--size-hairline', strokeAlignment: 'inner', layout: 'horizontal', alignItems: 'center',
    justifyContent: 'center', padding: [0, '$--spacing-md'],
  }, [text('ddm-t', 'Actions', {fill: '$--subtle-foreground'})]);
  function states(suffix) {
    return [
      cell('Closed', ref(`ddm-closed-${suffix}`, master.id, 'Closed')),
      cell('Open', frame(`ddm-openwrap-${suffix}`, 'Open', {layout: 'vertical', gap: '$--spacing-xxs'}, [
        ref(`ddm-opentrig-${suffix}`, master.id, 'Trigger'),
        menuContent(`ddm-openc-${suffix}`, W, [
          menuItem(`ddm-oi0-${suffix}`, 'Rename', 'default', 'pencil'),
          menuItem(`ddm-oi1-${suffix}`, 'Duplicate', 'default', 'copy'),
          menuSeparator(`ddm-osep-${suffix}`),
          menuItem(`ddm-oi2-${suffix}`, 'Delete', 'destructive', 'trash-2'),
        ]),
      ])),
      cell('Highlighted', menuContent(`ddm-hlc-${suffix}`, W, [
        menuItem(`ddm-hi0-${suffix}`, 'Rename', 'highlighted', 'pencil'),
        menuItem(`ddm-hi1-${suffix}`, 'Duplicate', 'default', 'copy'),
      ])),
      cell('Disabled Item', menuContent(`ddm-dic-${suffix}`, W, [
        menuItem(`ddm-dii0-${suffix}`, 'Rename', 'default', 'pencil'),
        menuItem(`ddm-dii1-${suffix}`, 'Duplicate', 'disabled', 'copy'),
      ])),
      cell('Destructive Item', menuContent(`ddm-destc-${suffix}`, W, [
        menuItem(`ddm-desti0-${suffix}`, 'Rename', 'default', 'pencil'),
        menuItem(`ddm-desti1-${suffix}`, 'Delete', 'destructive', 'trash-2'),
      ])),
    ];
  }
  return buildSheet('sys-dropdown-menu', 'System / Dropdown Menu', 'shadcn Dropdown Menu over Radix: rounded-md popover, --radius-xs items, --accent highlight. Context Menu draws the identical menu content over a point anchor.', [masterCard('ddm-master-card', 'Master', master)], states('l'), states('d'));
}

function buildContextMenu(tokens) {
  const W = num(tokens, '--size-settings-control-w');
  function states(suffix) {
    const surface = frame(`ctx-surf-${suffix}`, 'Target', {
      width: 220, height: 64, cornerRadius: '$--radius-md', fill: '$--card', stroke: '$--border',
      strokeWidth: '$--size-hairline', strokeAlignment: 'inner', layout: 'horizontal', alignItems: 'center', justifyContent: 'center',
    }, [text(`ctx-surf-t-${suffix}`, 'Right-click target', {fill: '$--muted-foreground', size: '$--text-caption'})]);
    return [
      cell('Open', frame(`ctx-openwrap-${suffix}`, 'Open', {layout: 'vertical', gap: '$--spacing-xxs'}, [
        surface,
        menuContent(`ctx-openc-${suffix}`, W, [menuItem(`ctx-oi0-${suffix}`, 'Copy path', 'default', 'copy'), menuItem(`ctx-oi1-${suffix}`, 'Reveal in Finder', 'default', 'folder-open')]),
      ])),
      cell('Highlighted', menuContent(`ctx-hlc-${suffix}`, W, [menuItem(`ctx-hi0-${suffix}`, 'Copy path', 'highlighted', 'copy'), menuItem(`ctx-hi1-${suffix}`, 'Reveal in Finder', 'default', 'folder-open')])),
      cell('Disabled Item', menuContent(`ctx-dic-${suffix}`, W, [menuItem(`ctx-dii0-${suffix}`, 'Copy path', 'default', 'copy'), menuItem(`ctx-dii1-${suffix}`, 'Reveal in Finder', 'disabled', 'folder-open')])),
    ];
  }
  return buildSheet('sys-context-menu', 'System / Context Menu', 'shadcn’s DropdownMenu over a controlled point anchor: same menu idiom as Dropdown Menu, opened at the pointer or under the target for the menu key and ⇧F10.', [], states('l'), states('d'));
}

// -- Dialog / Alert Dialog / Sheet / Popover / Tooltip / Command / Sonner ---------

function buildDialog(tokens) {
  const W = num(tokens, '--size-worktree-dialog'), PAD = num(tokens, '--spacing-lg');
  function surface(id, withClose) {
    return frame(id, withClose ? 'With Close Button' : 'Open', {
      width: W, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline',
      strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-md',
    }, [
      frame(`${id}-hdr`, 'Header', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'start', padding: ['$--spacing-lg', '$--spacing-lg', 0, '$--spacing-lg']}, [
        text(`${id}-title`, 'Rename worktree', {fill: '$--foreground', size: '$--text-title', weight: '600'}),
        ...(withClose ? [icon(`${id}-x`, 'x', {size: 14, fill: '$--muted-foreground'})] : []),
      ]),
      text(`${id}-desc`, 'Choose a new name for this worktree. Panes stay attached.', {fill: '$--subtle-foreground', width: W - 2 * PAD}),
      frame(`${id}-ftr`, 'Footer', {layout: 'horizontal', justifyContent: 'end', gap: '$--spacing-sm', padding: [0, '$--spacing-lg', '$--spacing-lg', '$--spacing-lg']}, [
        text(`${id}-cancel`, 'Cancel', {fill: '$--subtle-foreground'}),
        frame(`${id}-ok`, 'Action', {fill: '$--primary', cornerRadius: '$--radius-sm', height: num(tokens, '--size-control'), padding: [0, '$--spacing-md'], layout: 'horizontal', alignItems: 'center', justifyContent: 'center'}, [text(`${id}-okt`, 'Rename', {fill: '$--primary-foreground', weight: '500'})]),
      ]),
    ]);
  }
  function states(suffix) { return [cell('Open', surface(`dlg-open-${suffix}`, false)), cell('With Close Button', surface(`dlg-close-${suffix}`, true))]; }
  return buildSheet('sys-dialog', 'System / Dialog', 'shadcn Dialog over Radix: --size-worktree-dialog wide, rounded-lg, an overlay at --opacity-secondary behind it.', [], states('l'), states('d'));
}

function buildAlertDialog(tokens) {
  const W = num(tokens, '--size-add-device-sheet-w'), PAD = num(tokens, '--spacing-lg');
  function surface(id, pending) {
    return frame(id, pending ? 'Pending' : 'Open', {
      width: W, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline',
      strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-md', padding: '$--spacing-lg',
    }, [
      text(`${id}-title`, 'Delete this worktree?', {fill: '$--foreground', size: '$--text-title', weight: '600'}),
      text(`${id}-desc`, 'This removes the checkout and its build cache. The branch itself is not deleted.', {fill: '$--subtle-foreground', width: W - 2 * PAD}),
      frame(`${id}-ftr`, 'Footer', {layout: 'horizontal', justifyContent: 'end', gap: '$--spacing-sm'}, [
        frame(`${id}-cancel`, 'Cancel', {fill: '$--secondary', cornerRadius: '$--radius-sm', height: num(tokens, '--size-control'), padding: [0, '$--spacing-md'], layout: 'horizontal', alignItems: 'center', justifyContent: 'center'}, [text(`${id}-cancelt`, 'Cancel', {fill: '$--subtle-foreground', weight: '500'})]),
        frame(`${id}-del`, 'Action', {fill: '$--destructive', cornerRadius: '$--radius-sm', height: num(tokens, '--size-control'), padding: [0, '$--spacing-md'], layout: 'horizontal', alignItems: 'center', justifyContent: 'center', opacity: pending ? 0.72 : 1}, [text(`${id}-delt`, pending ? 'Deleting…' : 'Delete worktree', {fill: '$--destructive-foreground', weight: '500'})]),
      ]),
    ]);
  }
  function states(suffix) { return [cell('Open', surface(`adlg-open-${suffix}`, false)), cell('Pending', surface(`adlg-pending-${suffix}`, true))]; }
  return buildSheet('sys-alert-dialog', 'System / Alert Dialog', 'shadcn Alert Dialog: a confirmation for an irreversible effect. Cancel keeps things as they are; the destructive action names its result.', [], states('l'), states('d'));
}

function buildSheetPart(tokens) {
  const W = num(tokens, '--size-panel-ideal');
  function panel(id, side) {
    return frame(id, side, {
      width: side === 'Right' || side === 'Left' ? 260 : 320, height: side === 'Right' || side === 'Left' ? 180 : 120,
      fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-xs', padding: '$--spacing-md',
    }, [text(`${id}-title`, 'Device details', {fill: '$--foreground', size: '$--text-title', weight: '600'}), text(`${id}-desc`, `Slides in from the ${side.toLowerCase()} edge, --size-panel-ideal wide when vertical.`, {fill: '$--subtle-foreground', width: 220})]);
  }
  function states(suffix) { return [cell('Right', panel(`sheet-right-${suffix}`, 'Right')), cell('Left', panel(`sheet-left-${suffix}`, 'Left'))]; }
  return buildSheet('sys-sheet', 'System / Sheet', `shadcn Sheet over Radix Dialog: a panel on one window edge, --size-panel-ideal (${W}px) wide on the left or right, an overlay at --opacity-dimmed behind it.`, [], states('l'), states('d'));
}

function buildPopover(tokens) {
  const W = num(tokens, '--size-pr-popover');
  function states(suffix) {
    const content = frame(`pop-open-${suffix}`, 'Open', {
      width: W, cornerRadius: '$--radius-md', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline',
      strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-xs', padding: '$--spacing-sm',
    }, [text(`pop-t-${suffix}`, 'Open a pull request', {fill: '$--foreground', weight: '500'}), text(`pop-d-${suffix}`, '#118 into main · CI green', {fill: '$--subtle-foreground', size: '$--text-caption'})]);
    return [cell('Open', content)];
  }
  return buildSheet('sys-popover', 'System / Popover', `shadcn Popover over Radix: --size-pr-popover (${W}px) wide, rounded-md, the same surface idiom as Dropdown Menu content.`, [], states('l'), states('d'));
}

function buildTooltip(tokens) {
  function balloon(id, withShortcut) {
    return frame(id, withShortcut ? 'With Shortcut' : 'Open', {
      cornerRadius: '$--radius-sm', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline',
      strokeAlignment: 'inner', layout: 'horizontal', alignItems: 'center', gap: '$--spacing-sm', padding: ['$--spacing-xs', '$--spacing-sm'],
    }, [text(`${id}-t`, 'Split terminal', {fill: '$--foreground', size: '$--text-caption'}), ...(withShortcut ? [text(`${id}-k`, '⌘D', {fill: '$--muted-foreground', size: '$--text-caption', mono: true})] : [])]);
  }
  function states(suffix) { return [cell('Open', balloon(`tip-open-${suffix}`, false)), cell('With Shortcut', balloon(`tip-shortcut-${suffix}`, true))]; }
  return buildSheet('sys-tooltip', 'System / Tooltip', 'shadcn Tooltip over Radix: --size-tooltip-max-width cap, rounded-sm, text-caption. Its label is the trigger’s accessible name.', [], states('l'), states('d'));
}

function buildCommand(tokens) {
  const W = num(tokens, '--size-search-sheet-w'), IH = num(tokens, '--size-control-lg');
  function inputRow(id) {
    return frame(id, 'Field', {height: IH, layout: 'horizontal', alignItems: 'center', gap: '$--spacing-sm', padding: [0, '$--spacing-md'], stroke: '$--border', strokeWidth: {bottom: '$--size-hairline'}, strokeAlignment: 'inner'}, [
      icon(`${id}-i`, 'search', {size: 14, fill: '$--muted-foreground'}), text(`${id}-t`, 'Search commands and files…', {fill: '$--muted-foreground'}),
    ]);
  }
  function surface(id, children) {
    return frame(id, 'Surface', {width: W, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', layout: 'vertical'}, children);
  }
  function states(suffix) {
    const withList = surface(`cmd-default-${suffix}`, [inputRow(`cmd-in-${suffix}`), frame(`cmd-list-${suffix}`, 'List', {layout: 'vertical', gap: 0, padding: '$--spacing-xxs'}, [
      menuItem(`cmd-i0-${suffix}`, 'Open Settings', 'default', 'settings'), menuItem(`cmd-i1-${suffix}`, 'New Worktree', 'highlighted', 'git-branch-plus'), menuItem(`cmd-i2-${suffix}`, 'Focus Terminal', 'default', 'square-terminal'),
    ])]);
    const filtered = surface(`cmd-filtered-${suffix}`, [inputRow(`cmd-inf-${suffix}`), frame(`cmd-listf-${suffix}`, 'List', {layout: 'vertical', gap: 0, padding: '$--spacing-xxs'}, [menuItem(`cmd-fi0-${suffix}`, 'New Worktree', 'highlighted', 'git-branch-plus')])]);
    const empty = surface(`cmd-empty-${suffix}`, [inputRow(`cmd-ine-${suffix}`), frame(`cmd-emptyrow-${suffix}`, 'Empty', {padding: '$--spacing-md', layout: 'horizontal', justifyContent: 'center'}, [text(`cmd-emptyt-${suffix}`, 'No matches', {fill: '$--muted-foreground', size: '$--text-caption'})])]);
    return [cell('Default', withList), cell('Filtered', filtered), cell('Empty', empty)];
  }
  return buildSheet('sys-command', 'System / Command', `shadcn Command over cmdk: --size-search-sheet-w (${W}px) wide, a --size-control-lg field, the shared menu-item idiom for its list.`, [], states('l'), states('d'));
}

function buildSonner(tokens) {
  function toast(id, name, description, action) {
    return frame(id, name, {
      width: 300, cornerRadius: '$--radius-md', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline',
      strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-xxs', padding: '$--spacing-md',
    }, [
      text(`${id}-t`, 'Copied to clipboard', {fill: '$--foreground'}),
      ...(description ? [text(`${id}-d`, description, {fill: '$--subtle-foreground', size: '$--text-caption', width: 260})] : []),
      ...(action ? [frame(`${id}-a`, 'Action', {layout: 'horizontal', justifyContent: 'end'}, [text(`${id}-at`, 'Undo', {fill: '$--primary', size: '$--text-caption', weight: '500'})])] : []),
    ]);
  }
  function states(suffix) {
    return [
      cell('Default', toast(`son-default-${suffix}`, 'Default')),
      cell('With Description', toast(`son-desc-${suffix}`, 'With Description', 'origin/main → pasteboard')),
      cell('With Action', toast(`son-action-${suffix}`, 'With Action', null, true)),
    ];
  }
  return buildSheet('sys-sonner', 'System / Sonner', 'shadcn Toaster (sonner): a transient confirmation, never the only place a state the operator must act on lives (design 13).', [], states('l'), states('d'));
}

// -- assembly ------------------------------------------------------------------

/** Find a node anywhere in the (already variable-renamed) document tree by id. */
export function findNode(document, id) {
  let found;
  function walk(node) {
    if (found || !node) return;
    if (node.id === id) { found = node; return; }
    for (const child of node.children ?? []) walk(child);
  }
  for (const child of document.children ?? []) walk(child);
  if (!found) throw new Error(`pen-system: no node ${id} in the document (a legacy System master was expected to still exist)`);
  return found;
}

/** The four masters System / Icon Button, Panel Tab, Badge and Keycap folded into their new homes. */
export function foldLegacyMasters(document) {
  return {
    iconButton: {master: findNode(document, 'Nyvom')},
    panelTab: {master: findNode(document, 'ywjtY')},
    badge: {master: findNode(document, 'eHAjc'), iconId: 'xXuNa', labelId: 'n8L5dm'},
    keycap: {master: findNode(document, 'Ec2W1')},
  };
}

/** The old top-level sheets folded away entirely; System / Badge is replaced in place by name. */
export const RETIRED_SHEETS = ['System / Icon Button', 'System / Panel Tab', 'System / Keycap'];

export function systemSheets(tokens, legacy) {
  return [
    {name: 'System / Button', build: () => buildButton(tokens, legacy)},
    {name: 'System / Input', build: () => buildInput(tokens)},
    {name: 'System / Select', build: () => buildSelect(tokens)},
    {name: 'System / Checkbox', build: () => buildCheckbox(tokens)},
    {name: 'System / Switch', build: () => buildSwitch(tokens)},
    {name: 'System / Radio Group', build: () => buildRadioGroup(tokens)},
    {name: 'System / Toggle Group', build: () => buildToggleGroup(tokens)},
    {name: 'System / Slider', build: () => buildSlider(tokens)},
    {name: 'System / Tabs', build: () => buildTabs(tokens, legacy)},
    {name: 'System / Badge', build: () => buildBadge(tokens, legacy)},
    {name: 'System / Kbd', build: () => buildKbd(tokens, legacy)},
    {name: 'System / Separator', build: () => buildSeparator(tokens)},
    {name: 'System / Dropdown Menu', build: () => buildDropdownMenu(tokens)},
    {name: 'System / Context Menu', build: () => buildContextMenu(tokens)},
    {name: 'System / Dialog', build: () => buildDialog(tokens)},
    {name: 'System / Alert Dialog', build: () => buildAlertDialog(tokens)},
    {name: 'System / Sheet', build: () => buildSheetPart(tokens)},
    {name: 'System / Popover', build: () => buildPopover(tokens)},
    {name: 'System / Tooltip', build: () => buildTooltip(tokens)},
    {name: 'System / Command', build: () => buildCommand(tokens)},
    {name: 'System / Sonner', build: () => buildSonner(tokens)},
  ];
}
