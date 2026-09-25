// Draw every `Screen / <Area>` sheet the web shell renders today into
// design/hide-screens.pen (PRD web-design-system-reset B19, D-18, D-19). A Screen
// sheet is a layout proposal, not a part library: it imports design/hide-ui.lib.pen
// as a library (`imports: {hideui: './hide-ui.lib.pen'}`) and draws its content as
// refs of that library's `System /` and `Component /` masters, the same way a
// Component sheet is built from System masters - never a hand-redrawn control.
//
// A confirmed Pen toolchain limit shapes every helper below: a cross-library ref
// resolves its OWN internal `$--token` references using hide-ui.lib.pen's own
// default (Light) value, regardless of any `theme: {Mode: 'Dark'}` tag anywhere in
// the importing document - the Mode axis does not cross an `imports` boundary for a
// master's untouched properties. Two things DO resolve correctly per theme: a bare
// local `$--token` reference on a node this document itself authors (proven with a
// minimal repro), and an explicit property override placed AT the cross-library ref
// site using this document's own local `$--token` (also proven, and the descendant
// override KEY needs the alias prefix too, e.g. `hideui:btn-lb`, not just its ref
// target - undocumented in the Pen CLI's own docs). So this document carries its own
// full copy of the token variables (`readLocalVariables` below, read through the
// same pen-tokens.mjs pipeline gen-pen.mjs uses, so it can never drift from
// tokens.json), and `themedOverrides` restates a ref's colors by walking the ACTUAL
// master node in the live design/hide-ui.lib.pen (via pen-tokens.mjs's loadCanvas)
// and copying every `$--token` name it finds on the master's own top level and on
// every descendant, at any depth, into a local (non-aliased) override of the exact
// same name - never a hand-typed color recipe, so a master's default colors changing
// in the library is a change this generator picks up on its next run, not a second
// place to edit. Button and Badge additionally carry real design decisions per
// variant (which token names "destructive" or "secondary" mean), which is why those
// two import their variant tables from pen-system.mjs (`BUTTON_VARIANTS`,
// `BADGE_VARIANTS`) rather than recomputing them from a single default state.
//
// This only reaches a master's own literal `children` tree; a master that itself
// nests a same-library `ref` (the Icon Button inside Workspace row and Project row,
// for instance) is not walked past that ref, the same way `themedOverrides` doesn't
// walk into ITS OWN `descendants` map - matching what scripts/check-hide-screens.mjs
// enforces, so the generator and the checker agree on what "covered" means. Those
// nested-ref icons are undecorated by the master itself (no default icon or fill),
// so there is nothing there for either side to miss.

import {read as readTokenPlan, loadCanvas, CANVAS} from './pen-tokens.mjs';
import {BUTTON_VARIANTS, BADGE_VARIANTS} from './pen-system.mjs';

const LOCAL_TOKEN = /^\$--[A-Za-z0-9_-]+$/;
const THEMED_PROPS = ['fill', 'stroke'];

let libraryRoot;
let libraryDocumentCache;
let disabledOpacity;

/** Set once by screenSheets(); every themedOverrides() call reads design/hide-ui.lib.pen through this. */
function setLibraryRoot(root, tokens) {
  libraryRoot = root;
  libraryDocumentCache = undefined;
  // A variable-bound opacity renders invisible in this toolchain (the same
  // proven limit AGENTS.md and pen-system.mjs's Button Disabled state already
  // work around for width/height): materialize the number once, here, rather
  // than pass the `$--opacity-disabled` token string at a disabled state's ref site.
  disabledOpacity = num(tokens, '--opacity-disabled');
}

function libraryDocument() {
  if (!libraryRoot) throw new Error('pen-screens: themedOverrides() was called before screenSheets() set the library root');
  if (!libraryDocumentCache) libraryDocumentCache = loadCanvas(libraryRoot).document;
  return libraryDocumentCache;
}

function findMaster(node, id) {
  if (node && typeof node === 'object') {
    if (node.id === id) return node;
    for (const child of node.children ?? []) {
      const found = findMaster(child, id);
      if (found) return found;
    }
  }
  return null;
}

/**
 * Every `$--token` fill/stroke design/hide-ui.lib.pen's own `masterId` node
 * carries, restated as a local (non-aliased) override: `{top, descendants}`,
 * where `top` goes on the ref's own property overrides and `descendants` goes
 * on the ref's `descendants` map (xref() adds the alias prefix). A literal
 * color (not a `$--token` string) is left alone - it is already theme-independent.
 */
function themedOverrides(masterId) {
  const master = findMaster({children: libraryDocument().children}, masterId);
  if (!master) throw new Error(`pen-screens needs master ${masterId}, which ${CANVAS} no longer carries`);
  const top = {};
  for (const prop of THEMED_PROPS) if (LOCAL_TOKEN.test(master[prop])) top[prop] = master[prop];
  const descendants = {};
  (function walk(node) {
    if (!node || typeof node !== 'object') return;
    if (node.id !== masterId) {
      const props = {};
      for (const prop of THEMED_PROPS) if (LOCAL_TOKEN.test(node[prop])) props[prop] = node[prop];
      if (Object.keys(props).length) descendants[node.id] = props;
    }
    for (const child of node.children ?? []) walk(child);
  })(master);
  return {top, descendants};
}

/** xref(), with every themed color themedOverrides(masterId) finds restated first; `overrides`/`descendants` win on a shared key. */
function themedXref(id, masterId, name, overrides = {}, descendants = {}) {
  const auto = themedOverrides(masterId);
  const mergedDescendants = {...auto.descendants};
  for (const [key, props] of Object.entries(descendants)) mergedDescendants[key] = {...(mergedDescendants[key] ?? {}), ...props};
  return xref(id, masterId, name, {...auto.top, ...overrides}, mergedDescendants);
}

// design/hide-ui.lib.pen carries a few variables of its own that tokens.json never
// generates (a font family is a library choice, not a color/spacing/radius token);
// this document's text() helper needs them too, so they are read live from the
// library itself rather than duplicated as a literal, keeping the same
// never-drifts guarantee pen-tokens.mjs gives the generated set.
const LIBRARY_AUTHORED_VARIABLES = ['--font-ui', '--font-mono'];

export const ALIAS = 'hideui';
export const LIBRARY_PATH = './hide-ui.lib.pen';

function num(tokens, name) {
  const token = tokens[name];
  if (!token) throw new Error(`pen-screens needs token ${name}, which tokens.json does not carry`);
  return token.type === 'alias' ? num(tokens, token.value) : token.value;
}

function text(id, content, {fill = '$--foreground', size = '$--text-body', weight = '400', mono = false, width, align} = {}) {
  return {
    type: 'text', id, name: content.length > 28 ? content.slice(0, 28) : content, content, fill,
    fontFamily: mono ? '$--font-mono' : '$--font-ui', fontSize: size, fontWeight: weight,
    ...(width ? {textGrowth: 'fixed-width', width} : {}), ...(align ? {textAlign: align} : {}),
  };
}

function icon(id, glyph, {size = 14, fill = '$--foreground', enabled = true} = {}) {
  return {type: 'icon', id, name: 'Glyph', enabled, width: size, height: size, icon: glyph, library: 'lucide', fill};
}

function frame(id, name, props, children) {
  return {type: 'frame', id, name, children, ...props};
}

// A cross-library ref: the target and every descendant-override key need the
// `hideui:` alias prefix (the key, not just the ref target - confirmed empirically,
// undocumented in DESIGN_WORKFLOW.md). `overrides` are plain top-level property
// overrides on the ref itself (fill, stroke, width...); `descendants` is a plain
// {rawId: props} map this function prefixes for you.
function xref(id, masterId, name, overrides = {}, descendants) {
  const prefixed = descendants ? Object.fromEntries(Object.entries(descendants).map(([key, value]) => [`${ALIAS}:${key}`, value])) : undefined;
  return {id, type: 'ref', ref: `${ALIAS}:${masterId}`, name, ...overrides, ...(prefixed ? {descendants: prefixed} : {})};
}

function themeFrame(id, name, mode, props, children) {
  return frame(id, name, {theme: {Mode: mode}, ...props}, children);
}

function screenSheet(id, name, spec, lightBuild, darkBuild) {
  return frame(id, name, {layout: 'vertical', gap: '$--spacing-xl', padding: '$--spacing-xl', fill: '#EDEDEE', width: 'fit_content'}, [
    text(`${id}-title`, name.replace('Screen / ', ''), {size: '$--text-headline', weight: '600'}),
    text(`${id}-spec`, spec, {size: '$--text-caption', fill: '$--muted-foreground', width: 960}),
    themeFrame(`${id}-light`, 'Light', 'Light', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start', padding: '$--spacing-lg', fill: '$--background', cornerRadius: '$--radius-md', width: 'fit_content'}, lightBuild('l')),
    themeFrame(`${id}-dark`, 'Dark', 'Dark', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start', padding: '$--spacing-lg', fill: '$--background', cornerRadius: '$--radius-md', width: 'fit_content'}, darkBuild('d')),
  ]);
}

// -- simple, frequently-reused controls: themedXref restates every default color --

function screenButton(id, label, {variant = 'default', height, width, icon: glyph} = {}) {
  const v = BUTTON_VARIANTS[variant];
  return themedXref(id, 'btn-m', label, {...v.overrides, ...(height ? {height} : {}), ...(width ? {width} : {})}, {
    'btn-ic': glyph ? {icon: glyph, fill: v.fg, enabled: true} : {enabled: false},
    'btn-lb': {content: label, fill: v.fg},
  });
}

function screenIconButton(id, glyph, {size} = {}) {
  return themedXref(id, 'Nyvom', 'Icon', size ? {width: size, height: size} : {}, {ZIZFR: {icon: glyph, fill: '$--muted-foreground'}});
}

function screenBadge(id, label, {variant = 'secondary'} = {}) {
  const v = BADGE_VARIANTS[variant];
  return themedXref(id, 'eHAjc', label, v.overrides, {xXuNa: {enabled: false}, n8L5dm: {content: label, fill: v.fg}});
}

function screenInput(id, {content, placeholder, mono, width} = {}) {
  return themedXref(id, 'inp-m', 'Input', width ? {width} : {}, {
    'inp-t': content !== undefined ? {content, fill: '$--foreground', ...(mono ? {fontFamily: '$--font-mono'} : {})} : {content: placeholder, fill: '$--muted-foreground'},
  });
}

function screenSelect(id, {content, placeholder, width} = {}) {
  return themedXref(id, 'sel-m', 'Select', width ? {width} : {}, {
    'sel-t': content !== undefined ? {content, fill: '$--foreground'} : {content: placeholder, fill: '$--muted-foreground'},
  });
}

// A horizontal tab strip: `items` is an array of labels, `activeIndex` the selected one.
function tabRefs(id, items, activeIndex) {
  return items.map((label, index) => themedXref(`${id}-${index}`, 'tab-m', label,
    index === activeIndex ? {fill: '$--secondary'} : {},
    {'tab-t': {content: label, fill: index === activeIndex ? '$--foreground' : '$--subtle-foreground'}}));
}

function screenTabs(id, items, activeIndex) {
  return frame(id, 'Tabs', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'}, tabRefs(id, items, activeIndex));
}

// The same segmented idiom as screenTabs, over System / Toggle Group's master.
function screenToggleGroup(id, items, activeIndex) {
  return frame(id, 'Toggle Group', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'},
    items.map((label, index) => themedXref(`${id}-${index}`, 'tog-m', label,
      index === activeIndex ? {fill: '$--secondary'} : {},
      {'tog-t': {content: label, fill: index === activeIndex ? '$--foreground' : '$--subtle-foreground'}})));
}

// A radio-styled swatch: System / Radio Group's master with its indicator dot
// disabled, since web's accent swatch is the whole circle in the accent color
// with a ring rather than a filled dot ([&_svg]:hidden on RadioGroupItem).
function screenSwatch(id, colorToken, selected) {
  return themedXref(id, 'rad-m', colorToken, {fill: colorToken, stroke: selected ? '$--foreground' : '$--border', strokeWidth: selected ? 2 : '$--size-hairline'}, {'rad-dot': {enabled: false}});
}

// System / Radio Group's own idiom (a dot inside the circle when selected),
// for a plain radio choice rather than an accent swatch.
function screenRadioItem(id, label, selected) {
  return frame(id, 'Radio', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
    themedXref(`${id}-dot`, 'rad-m', label, {}, {'rad-dot': {enabled: selected}}),
    text(`${id}-label`, label, {weight: '500'}),
  ]);
}

// The master's own Range width (96) and Thumb x (88) are one fixed demo state,
// not tied to any value; `fraction` (0-1) places both at the same point along
// the 160px-wide track the master draws, thumb centered on the range's end.
function screenSlider(id, fraction) {
  const trackWidth = 160, thumbRadius = 8;
  const fillWidth = Math.round(trackWidth * fraction);
  return themedXref(id, 'sld-m', 'Slider', {}, {'sld-range': {width: fillWidth}, 'sld-thumb': {x: fillWidth - thumbRadius}});
}

const MENU_ITEM_STATE = {default: {}, highlighted: {fill: '$--accent', text: '$--accent-foreground'}, disabled: {text: '$--muted-foreground'}, destructive: {text: '$--destructive'}};

function screenMenuItem(id, label, {state = 'default', glyph, reason, reasonWidth = 200, shortcut} = {}) {
  const s = MENU_ITEM_STATE[state];
  return themedXref(id, 'mnu-item-m', label, {...(s.fill ? {fill: s.fill} : {}), ...(state === 'disabled' ? {opacity: disabledOpacity} : {})}, {
    'mnu-item-icon': glyph ? {icon: glyph, fill: s.text ?? '$--muted-foreground', enabled: true} : {enabled: false},
    'mnu-item-label': {content: label, fill: s.text ?? '$--foreground'},
    'mnu-item-reason': reason ? {content: reason, enabled: true, textGrowth: 'fixed-width', width: reasonWidth} : {enabled: false},
    'mnu-item-shortcut': shortcut ? {content: shortcut, enabled: true} : {enabled: false},
  });
}

function screenMenuSeparator(id) {
  return themedXref(id, 'mnu-sep-m', 'Separator');
}

function screenMenuContent(id, width, children) {
  return frame(id, 'Content', {
    layout: 'vertical', gap: 0, padding: '$--spacing-xxs', width, cornerRadius: '$--radius-sm',
    fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
  }, children);
}

// A dialog/alert-dialog surface, matching System / Dialog's own composition
// (Card/Modal shaped header + footer actions as Button refs).
function screenDialogSurface(id, {width, title, description, body, actions}) {
  return frame(id, 'Surface', {
    width, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline',
    strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-md',
  }, [
    frame(`${id}-hdr`, 'Header', {layout: 'vertical', gap: '$--spacing-xs', padding: ['$--spacing-lg', '$--spacing-lg', 0, '$--spacing-lg']}, [
      text(`${id}-title`, title, {fill: '$--foreground', size: '$--text-title', weight: '600'}),
      ...(description ? [text(`${id}-desc`, description, {fill: '$--subtle-foreground', size: '$--text-caption', mono: true, width: width - 2 * 16})] : []),
    ]),
    ...(body ? [frame(`${id}-body`, 'Body', {layout: 'vertical', gap: '$--spacing-md', padding: [0, '$--spacing-lg']}, body)] : []),
    frame(`${id}-ftr`, 'Footer', {layout: 'horizontal', justifyContent: 'end', gap: '$--spacing-sm', padding: [0, '$--spacing-lg', '$--spacing-lg', '$--spacing-lg']}, actions),
  ]);
}

// A DialogBody field: a caption label above its control, the settings-rows.tsx
// idiom this dialog form also uses (a label the control sits directly under).
function screenField(id, label, control) {
  return frame(id, 'Field', {layout: 'vertical', gap: '$--spacing-xxs'}, [
    text(`${id}-label`, label, {size: '$--text-caption', weight: '500', fill: '$--subtle-foreground'}),
    control,
  ]);
}

// -- composite Component masters: themedXref restates every internal default color,
// so only content (and any state that changes what the master would not restate on
// its own, like the status dot's color) needs to be named at each call site. -------

// jb7mF (location) and w4jdH (age) carry an absolute x/y: 0 in the master, for a
// different (native shell) layout context; enabling them here overlaps the rest
// of the canvas instead of flowing in this frame, so they stay at the master's
// own disabled default and only the flowing fields (status, relationship count)
// carry location/age content in this proposal.
function screenAgentIdentity(id, {title, status, statusColor = '$--agent-working'}) {
  return themedXref(id, 'HXWFK', title, {width: 260}, {
    PCeNa: {fill: statusColor}, x0h40: {content: title}, X6LdE: {content: status},
  });
}

// The master's own title text (IxbXq) is `textGrowth: 'fixed-width', width:
// 'fill_container'`, which wraps across several lines once this ref's own
// width is overridden away from the master's narrower native context (Pen
// has no text-truncation property at all - `fixed-width` always wraps,
// `fixed-width-height` only clips without an ellipsis - so `auto` growth,
// the same no-textGrowth idiom Project row's own title (JkPyX) already uses,
// is the one way to keep a title and a path each on their own single line;
// a real ellipsis is not something this toolchain can draw).
function screenWorkspaceRow(id, {title, role, width = 260}) {
  return themedXref(id, 'pPtY6', title, {width}, {IxbXq: {content: title, textGrowth: 'auto'}, JzcqS: {content: role, textGrowth: 'auto'}});
}

function screenSectionHeader(id, {label, count, detail, width = 260}) {
  return themedXref(id, 'O79KF', label, {width}, {VOd6D: {content: label}, rVsKB: {content: count}, g7fzt: {content: detail}});
}

function screenLineRow(id, {label, meta, status, width = 260}) {
  return themedXref(id, 'jJJqB', label, {width}, {hJDwP: {content: label}, xT4J9: {content: meta}, e44ufH: {content: status}});
}

function screenProjectRow(id, {name, agents, width = 292}) {
  return themedXref(id, 'qdhY0', name, {width}, {JkPyX: {content: name}, nZkan: {content: agents}});
}

function screenSessionRow(id, {title, checkout, provider, time, width = 360}) {
  return themedXref(id, 'session-row', title, {width}, {'session-row-provider': {content: provider}, 'session-row-time': {content: time}, 'session-row-title': {content: title}, 'session-row-checkout': {content: checkout}});
}

// view-tab's own master carries a themed top-level fill (the card behind an
// inactive tab); the active tab is a different token the master does not
// default to, so it is the one property named explicitly, every time (never
// only when active), so an inactive tab is never left un-restated either.
function screenViewTab(id, {title, active}) {
  return themedXref(id, 'view-tab', title, {fill: active ? '$--background' : '$--card'}, {'view-tab-title': {content: title}});
}

// DevicePicker.tsx renders each row as a DropdownMenuItem, not the standalone
// device-row card System / Menu Item's own idiom already gives (glyph + a
// stacked label/reason body); the trailing checkmark reuses the menu item's
// shortcut slot (a plain text node) since the master has no icon slot there.
function screenDevicePickerRow(id, {name, detail, selected}) {
  return screenMenuItem(id, name, {glyph: 'server', reason: detail, shortcut: selected ? '✓' : undefined});
}

// The Agents/Projects sidebar App.tsx/sidebar.tsx always shows beside a
// screen's own content; every sheet that draws a whole screen (Main,
// Workspace) includes it so a reader sees the whole thing, not just its
// own feature in isolation.
function screenSidebar(id, suffix, agents) {
  return frame(`${id}-${suffix}`, 'Sidebar', {width: 220, layout: 'vertical', gap: '$--spacing-md', fill: '$--sidebar', padding: '$--spacing-md', cornerRadius: '$--radius-md'}, [
    frame(`${id}-tabs-${suffix}`, 'Tabs', {layout: 'horizontal', gap: '$--spacing-md'}, [
      text(`${id}-agentstab-${suffix}`, 'Agents', {weight: '600'}),
      text(`${id}-projectstab-${suffix}`, 'Projects', {fill: '$--muted-foreground'}),
    ]),
    text(`${id}-seen-${suffix}`, 'SEEN · 2', {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
    ...agents.map((agent, index) => screenAgentIdentity(`${id}-agent${index}-${suffix}`, agent)),
    frame(`${id}-newws-${suffix}`, 'New workspace', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', padding: ['$--spacing-sm', 0]}, [
      icon(`${id}-newwsi-${suffix}`, 'plus', {size: 12, fill: '$--muted-foreground'}),
      text(`${id}-newwst-${suffix}`, '새 워크스페이스 ⌥⇧N', {size: '$--text-caption', fill: '$--muted-foreground'}),
    ]),
    screenSelect(`${id}-device-${suffix}`, {content: 'This Mac', width: 190}),
  ]);
}

// -- Screen / Main ------------------------------------------------------------

function buildMain(tokens) {
  function build(suffix) {
    const sidebar = screenSidebar('main-sidebar', suffix, [
      {title: '두 번째 에이전트', status: 'Working'},
      {title: 'Agent one', status: 'Seen', statusColor: '$--muted-foreground'},
    ]);
    const list = frame(`main-list-${suffix}`, 'Projects', {width: 480, layout: 'vertical', gap: '$--spacing-md'}, [
      frame(`main-listhdr-${suffix}`, 'Header', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center'}, [
        text(`main-listtitle-${suffix}`, 'Projects', {size: '$--text-title', weight: '600'}),
        screenButton(`main-addproj-${suffix}`, 'Add project', {variant: 'secondary', height: num(tokens, '--size-control-sm')}),
      ]),
      screenSectionHeader(`main-sect-${suffix}`, {label: 'THIS MAC', count: '', detail: '', width: 480}),
      screenProjectRow(`main-proj1-${suffix}`, {name: 'sasu-web-design-system-reset', agents: '3 agents', width: 480}),
      screenProjectRow(`main-proj2-${suffix}`, {name: 'herdr-ide.worktrees/checkout-row-d', agents: 'No agents', width: 480}),
    ]);
    return [sidebar, list];
  }
  return screenSheet('screen-main', 'Screen / Main', 'web/src/App.tsx, sidebar.tsx, MainScreen.tsx: the agent/project sidebar beside the Projects list, grouped by device. Long checkout names truncate inside their row (B11).', s => build(s), s => build(s));
}

// -- Screen / Project Overview -------------------------------------------------

function buildProjectOverview(tokens) {
  function build(suffix) {
    // An explicit width is required for justifyContent: space_between to actually
    // distribute space between the two sides, rather than packing them together
    // with zero gap (proven empirically drawing Settings' Accent row).
    const crumb = frame(`ov-crumb-${suffix}`, 'Breadcrumb', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', width: 560}, [
      frame(`ov-crumb-left-${suffix}`, 'Path', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        text(`ov-crumb1-${suffix}`, 'Main', {fill: '$--muted-foreground'}),
        text(`ov-crumb2-${suffix}`, '/', {fill: '$--muted-foreground'}),
        text(`ov-crumb3-${suffix}`, 'sasu-web-design-system-reset', {weight: '600'}),
        screenBadge(`ov-device-${suffix}`, 'This Mac'),
      ]),
      screenButton(`ov-sessions-${suffix}`, 'Sessions', {variant: 'ghost', height: num(tokens, '--size-control-sm')}),
    ]);
    const body = frame(`ov-body-${suffix}`, 'Body', {width: 560, layout: 'vertical', gap: '$--spacing-lg'}, [
      screenSectionHeader(`ov-wssect-${suffix}`, {label: 'WORKSPACES', count: '· 2', detail: '', width: 560}),
      screenWorkspaceRow(`ov-ws1-${suffix}`, {title: 'prd/web-design-system-reset', role: '~/projects/sasu/worktrees/web-design-system-reset', width: 560}),
      screenWorkspaceRow(`ov-ws2-${suffix}`, {title: 'main', role: '~/projects/sasu', width: 560}),
      screenSectionHeader(`ov-agsect-${suffix}`, {label: 'AGENTS', count: '· 1', detail: '', width: 560}),
      screenAgentIdentity(`ov-agent-${suffix}`, {title: 'pen-system', status: 'Working'}),
    ]);
    return [crumb, body];
  }
  return screenSheet('screen-project-overview', 'Screen / Project Overview', 'web/src/MainScreen.tsx (ProjectRow expanded): a project’s Workspaces and Agents lists under a breadcrumb, with a Sessions shortcut.', s => [frame(`ovw-l-${s}`, 'Wrap', {layout: 'vertical', gap: '$--spacing-lg', width: 560}, build(s))], s => [frame(`ovw-d-${s}`, 'Wrap', {layout: 'vertical', gap: '$--spacing-lg', width: 560}, build(s))]);
}

// -- Screen / Workspace ---------------------------------------------------------

// A leading-icon tab for the agent/terminal column's own tab strip
// (TabBar.tsx) - distinct from screenViewTab, which is the editor column's
// tab and carries no icon. No library master matches this exact shape
// (icon + label + its own underline), so it is hand-composed, the same way
// screenDialogSurface is where no Dialog master fits either.
function screenPanelTab(id, glyph, title, active) {
  return frame(id, title, {
    layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', padding: [0, '$--spacing-xs', '$--spacing-xxs', '$--spacing-xs'],
    ...(active ? {stroke: '$--foreground', strokeWidth: {bottom: 2}, strokeAlignment: 'inner'} : {}),
  }, [
    icon(`${id}-i`, glyph, {size: 12, fill: active ? '$--foreground' : '$--subtle-foreground'}),
    text(`${id}-t`, title, {size: '$--text-subhead', weight: active ? '600' : '400', fill: active ? '$--foreground' : '$--subtle-foreground'}),
  ]);
}

// The pane header bar above a terminal (Pane header and focus's own toolbar
// is the native shell's much larger agent-identity chrome; the web's own bar
// for a plain, session-less pane is this flat one: a pane id, its status,
// and the same overflow/close icon pair every pane header carries).
function screenPaneHeader(id, {label, status, width}) {
  return frame(id, 'Pane header', {width, height: 28, layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', padding: [0, '$--spacing-sm'], fill: '$--secondary'}, [
    text(`${id}-label`, label, {mono: true, size: '$--text-caption', fill: '$--subtle-foreground'}),
    frame(`${id}-trail`, 'Trailing', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
      text(`${id}-status`, status, {size: '$--text-caption', fill: '$--muted-foreground'}),
      screenIconButton(`${id}-more`, 'ellipsis', {size: 20}),
      screenIconButton(`${id}-close`, 'x', {size: 20}),
    ]),
  ]);
}

function buildWorkspace() {
  const SIDEBAR_W = 220, AGENT_W = 300, VIEW_W = 380, EXPLORER_W = 240;
  const TOTAL_W = SIDEBAR_W + AGENT_W + VIEW_W + EXPLORER_W + 3 * 12;
  function chrome(suffix) {
    const sidebar = screenSidebar('ws-sidebar', suffix, [
      {title: 'Agent two', status: 'Working'},
      {title: 'Agent one', status: 'Seen', statusColor: '$--muted-foreground'},
    ]);
    const crumb = frame(`ws-crumb-${suffix}`, 'Breadcrumb', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
      text(`ws-c1-${suffix}`, 'Main', {fill: '$--muted-foreground'}),
      text(`ws-c2-${suffix}`, '/', {fill: '$--muted-foreground'}),
      text(`ws-c3-${suffix}`, 'demo', {fill: '$--muted-foreground'}),
      text(`ws-c4-${suffix}`, '/', {fill: '$--muted-foreground'}),
      text(`ws-c5-${suffix}`, 'demo', {weight: '600'}),
    ]);
    const toolIcons = frame(`ws-tools-${suffix}`, 'Tools', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      screenIconButton(`ws-tool1-${suffix}`, 'square-terminal'), screenIconButton(`ws-tool2-${suffix}`, 'columns-2'), screenIconButton(`ws-tool3-${suffix}`, 'file-text'),
      screenIconButton(`ws-tool4-${suffix}`, 'ellipsis'), screenIconButton(`ws-tool5-${suffix}`, 'folder'), screenIconButton(`ws-tool6-${suffix}`, 'git-branch'),
    ]);
    const topBar = frame(`ws-topbar-${suffix}`, 'Top bar', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', width: TOTAL_W}, [crumb, toolIcons]);

    // Column 1: the agent/terminal area - its own tab strip, a pane header,
    // the terminal body filling the rest of the column.
    const agentTabs = frame(`ws-agtabs-${suffix}`, 'Tab bar', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      screenPanelTab(`ws-agtab1-${suffix}`, 'square-terminal', 'Tab 1', true),
      screenIconButton(`ws-agtabclose-${suffix}`, 'x', {size: 20}),
      screenIconButton(`ws-agtabadd-${suffix}`, 'plus', {size: 20}),
    ]);
    const paneHeader = screenPaneHeader(`ws-panehdr-${suffix}`, {label: 'w2:p1', status: 'Unknown', width: AGENT_W});
    // The xterm viewport takes --background in either theme (commit 7052afa); a
    // local token, so it themes correctly without any ref-site restating.
    const terminal = frame(`ws-terminal-${suffix}`, 'Terminal', {width: AGENT_W, height: 260, fill: '$--background', padding: '$--spacing-sm', layout: 'vertical', gap: '$--spacing-xxs'}, [
      text(`ws-term1-${suffix}`, 'fixture % echo capture-demo 한글 확인', {fill: '$--foreground', mono: true, size: '$--text-caption'}),
    ]);
    const agentColumn = frame(`ws-agcol-${suffix}`, 'Agent area', {width: AGENT_W, layout: 'vertical', gap: 0}, [agentTabs, paneHeader, terminal]);

    // Column 2: the view/editor area - its own view tab strip, a document
    // header (path + Wrap/Find/preview), the editor body with line numbers.
    const viewTabs = frame(`ws-vtabs-${suffix}`, 'Tab bar', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      screenViewTab(`ws-vtab1-${suffix}`, {title: '한글 노트.md', active: true}),
    ]);
    const docHeader = frame(`ws-dochdr-${suffix}`, 'Document header', {width: VIEW_W, height: 28, layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', padding: [0, '$--spacing-sm'], fill: '$--card'}, [
      text(`ws-docpath-${suffix}`, '~/projects/sasu/demo', {mono: true, size: '$--text-caption', fill: '$--muted-foreground'}),
      frame(`ws-doclinks-${suffix}`, 'Links', {layout: 'horizontal', gap: '$--spacing-md'}, [
        text(`ws-docwrap-${suffix}`, 'Wrap', {size: '$--text-caption', fill: '$--subtle-foreground'}),
        text(`ws-docfind-${suffix}`, 'Find', {size: '$--text-caption', fill: '$--subtle-foreground'}),
        text(`ws-docpreview-${suffix}`, 'preview', {size: '$--text-caption', fill: '$--subtle-foreground'}),
      ]),
    ]);
    const editorBody = frame(`ws-editor-${suffix}`, 'Editor body', {width: VIEW_W, height: 232, layout: 'vertical', gap: '$--spacing-xxs', padding: '$--spacing-sm', fill: '$--background'}, [
      frame(`ws-line1-${suffix}`, 'Line', {layout: 'horizontal', gap: '$--spacing-sm'}, [
        text(`ws-line1n-${suffix}`, '1', {mono: true, size: '$--text-caption', fill: '$--muted-foreground'}),
        text(`ws-line1t-${suffix}`, 'export const answer = 41;', {mono: true, size: '$--text-caption'}),
      ]),
      frame(`ws-line2-${suffix}`, 'Line', {layout: 'horizontal', gap: '$--spacing-sm'}, [
        text(`ws-line2n-${suffix}`, '2', {mono: true, size: '$--text-caption', fill: '$--muted-foreground'}),
      ]),
    ]);
    const viewColumn = frame(`ws-vcol-${suffix}`, 'View area', {width: VIEW_W, layout: 'vertical', gap: 0}, [viewTabs, docHeader, editorBody]);

    // Column 3: Explorer - header, root row + inline git-status warning, files.
    const explorerHeader = frame(`ws-exphdr-row-${suffix}`, 'Header', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', width: EXPLORER_W}, [
      text(`ws-exphdr-${suffix}`, 'EXPLORER', {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
      screenIconButton(`ws-expclose-${suffix}`, 'x', {size: 20}),
    ]);
    const explorerRoot = frame(`ws-exproot-${suffix}`, 'Root', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', width: EXPLORER_W}, [
      text(`ws-exproott-${suffix}`, 'demo', {size: '$--text-caption', weight: '600', fill: '$--muted-foreground'}),
      icon(`ws-exprefresh-${suffix}`, 'refresh-cw', {size: 12, fill: '$--muted-foreground'}),
    ]);
    const explorerNotice = text(`ws-expnotice-${suffix}`, 'Git status unavailable: ~/projects/sasu/demo is not inside a Git repository', {fill: '$--warning', size: '$--text-caption', width: EXPLORER_W});
    const explorer = frame(`ws-explorer-${suffix}`, 'Explorer', {width: EXPLORER_W, layout: 'vertical', gap: '$--spacing-xs', fill: '$--card', padding: '$--spacing-sm', cornerRadius: '$--radius-md'}, [
      explorerHeader, explorerRoot, explorerNotice,
      screenLineRow(`ws-file1-${suffix}`, {label: 'README.md', meta: '', status: '', width: EXPLORER_W - 20}),
      screenLineRow(`ws-file2-${suffix}`, {label: '한글 노트.md', meta: '', status: 'M', width: EXPLORER_W - 20}),
      screenLineRow(`ws-file3-${suffix}`, {label: 'scripts/pen-screens.mjs', meta: '', status: 'A', width: EXPLORER_W - 20}),
    ]);

    const content = frame(`ws-content-${suffix}`, 'Content', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [sidebar, agentColumn, viewColumn, explorer]);
    // screenSheet's theme frame lays its top-level return array out
    // horizontally (every other screen's peers sit side by side); this
    // screen instead needs the top bar stacked above the content row.
    return [frame(`ws-wrap-${suffix}`, 'Wrap', {layout: 'vertical', gap: '$--spacing-md', width: TOTAL_W}, [topBar, content])];
  }
  return screenSheet('screen-workspace', 'Screen / Workspace', 'web/src/WorkspaceScreen.tsx, TabBar.tsx, ViewAreas.tsx, ExplorerTree.tsx: the whole workspace screen - top bar, agent/terminal column, view/editor column and Explorer, with a Korean file name to verify B11 wrapping.', chrome, chrome);
}

// -- Screen / Project Sessions --------------------------------------------------

function buildSessions() {
  function build(suffix) {
    const crumb = frame(`ss-crumb-${suffix}`, 'Breadcrumb', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
      text(`ss-c1-${suffix}`, 'Main', {fill: '$--muted-foreground'}), text(`ss-c2-${suffix}`, '/', {fill: '$--muted-foreground'}),
      text(`ss-c3-${suffix}`, 'fixture', {fill: '$--muted-foreground'}), text(`ss-c4-${suffix}`, '/', {fill: '$--muted-foreground'}),
      text(`ss-c5-${suffix}`, 'Sessions', {weight: '600'}),
    ]);
    const list = frame(`ss-list-${suffix}`, 'List', {width: 320, layout: 'vertical', gap: '$--spacing-sm'}, [
      screenTabs(`ss-tabs-${suffix}`, ['All', 'Codex', 'Claude Code'], 0),
      screenInput(`ss-search-${suffix}`, {placeholder: 'Search sessions', width: 300}),
      text(`ss-count-${suffix}`, '1 session', {size: '$--text-caption', fill: '$--muted-foreground'}),
      screenSessionRow(`ss-row1-${suffix}`, {title: '배포 스크립트 정리하고 release note 초안까지 작성해줘', checkout: 'fixture', provider: 'Claude Code', time: 'Sep 21, 10:00 AM', width: 300}),
    ]);
    const detail = frame(`ss-detail-${suffix}`, 'Detail', {width: 480, height: 320, alignItems: 'center', justifyContent: 'center', fill: '$--card', cornerRadius: '$--radius-md'}, [
      text(`ss-empty-${suffix}`, 'Choose a session to read it here.', {fill: '$--muted-foreground'}),
    ]);
    return [frame(`ss-wrap-${suffix}`, 'Wrap', {layout: 'vertical', gap: '$--spacing-md'}, [crumb, frame(`ss-row-${suffix}`, 'Row', {layout: 'horizontal', gap: '$--spacing-lg'}, [list, detail])])];
  }
  return screenSheet('screen-sessions', 'Screen / Project Sessions', 'web/src/SessionsScreen.tsx: the provider-filtered session list with search, and the read-only detail pane, using a real Korean session title.', build, build);
}

// -- Screen / Settings ----------------------------------------------------------

// Group/Row (web/src/components/settings-rows.tsx): a titled card of
// hairline-divided rows with a note line below it, matching HideSettingsGroup.
function settingsGroup(id, title, note, rows, width) {
  const box = frame(`${id}-box`, 'Box', {
    layout: 'vertical', gap: 0, width, cornerRadius: '$--radius-md', fill: '$--card',
    stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
  }, rows.flatMap((row, index) => index === 0 ? [row] : [
    {type: 'line', id: `${id}-sep-${index}`, name: 'Divider', width: 'fill_container', height: 0, stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'center'},
    row,
  ]));
  return frame(id, title, {layout: 'vertical', gap: '$--spacing-sm', width}, [
    text(`${id}-title`, title, {size: '$--text-body', weight: '600', fill: '$--subtle-foreground'}),
    box,
    text(`${id}-note`, note, {size: '$--text-body', fill: '$--muted-foreground', width}),
  ]);
}

function settingsRow(id, label, control) {
  return frame(id, 'Row', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', width: 'fill_container', padding: ['$--spacing-sm', '$--spacing-md']}, [
    text(`${id}-label`, label, {size: '$--text-subhead'}),
    frame(`${id}-control`, 'Control', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, Array.isArray(control) ? control : [control]),
  ]);
}

function buildSettings(tokens) {
  const W = num(tokens, '--size-settings-sheet-w');
  const TABS = ['General', 'Appearance', 'Agents', 'Devices', 'Shortcuts'];
  const SUBTITLE = 'Theme, accent and interface density.';
  const OWNER = 'Appearance, shortcuts, Background AI, hooks and the device list are kept by hided on fixture.';
  const ACCENTS = [
    {name: 'Lime', token: '$--accent-choice-lime'},
    {name: 'Sky', token: '$--accent-choice-sky'},
    {name: 'Violet', token: '$--accent-choice-violet'},
    {name: 'Amber', token: '$--accent-choice-amber'},
  ];
  const bodyW = W - 2 * 32;
  function build(suffix) {
    const header = frame(`set-hdr-${suffix}`, 'Header', {layout: 'horizontal', alignItems: 'start', gap: '$--spacing-md', width: W, padding: '$--spacing-lg'}, [
      frame(`set-hdrtext-${suffix}`, 'Text', {layout: 'vertical', gap: '$--spacing-xxs', width: W - 64 - 24}, [
        text(`set-title-${suffix}`, 'Settings', {size: '$--text-headline', weight: '600'}),
        text(`set-sub-${suffix}`, SUBTITLE, {size: '$--text-body', fill: '$--subtle-foreground'}),
        text(`set-owner-${suffix}`, OWNER, {size: '$--text-caption', fill: '$--muted-foreground', width: W - 64 - 24}),
      ]),
      screenIconButton(`set-close-${suffix}`, 'x'),
    ]);
    const nav = frame(`set-nav-${suffix}`, 'Tabs', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: W, padding: ['$--spacing-sm', '$--spacing-lg'], fill: '$--sidebar'},
      tabRefs(`set-tabs-${suffix}`, TABS, 1));
    const accentRow = settingsRow(`set-accent-${suffix}`, 'Accent', [
      ...ACCENTS.map((a, i) => screenSwatch(`set-swatch-${i}-${suffix}`, a.token, i === 0)),
      text(`set-accenthex-${suffix}`, '#B9FF66', {mono: true, fill: '$--subtle-foreground'}),
    ]);
    // Each row shows its own theme selected, matching the web page it depicts
    // rather than a single fixed demo state: System=0, Light=1, Dark=2.
    const themeIndex = suffix === 'd' ? 2 : 1;
    const theme = settingsGroup(`set-theme-${suffix}`, 'Theme',
      'System follows macOS as it changes. Accent tints primary buttons, focus rings and the editor caret; agent status colors keep their meaning whatever the accent.',
      [settingsRow(`set-appearance-${suffix}`, 'Appearance', screenToggleGroup(`set-appearance-v-${suffix}`, ['System', 'Light', 'Dark'], themeIndex)), accentRow], bodyW);
    const density = settingsGroup(`set-density-${suffix}`, 'Density',
      'Terminal and editor text keep their own size (⌘= and ⌘- in a pane or document).',
      [settingsRow(`set-font-${suffix}`, 'Interface font', [screenSlider(`set-slider-${suffix}`, 1 / 3), text(`set-fontval-${suffix}`, '13 pt', {mono: true})])], bodyW);
    const body = frame(`set-body-${suffix}`, 'Body', {layout: 'vertical', gap: '$--spacing-lg', width: W, padding: '$--spacing-lg'}, [theme, density]);
    return [frame(`set-dialog-${suffix}`, 'Settings', {
      layout: 'vertical', gap: 0, width: W, cornerRadius: '$--radius-lg', fill: '$--popover',
      stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    }, [header, nav, body])];
  }
  return screenSheet('screen-settings', 'Screen / Settings', 'web/src/SettingsSheet.tsx (a Dialog), settings.ts SETTINGS_TABS, settings-rows.tsx Group/Row: the five-tab strip and, on Appearance, the Theme group (ToggleGroup + accent swatches) and Density group (Slider), matching web/src/SettingsSheet.tsx’s AppearanceTab exactly rather than a Select-based approximation.', build, build);
}

// -- Screen / Palette ------------------------------------------------------------

function buildPalette() {
  function build(suffix) {
    function surface(id, placeholder, rows) {
      return frame(id, 'Surface', {width: 480, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', layout: 'vertical'}, [
        frame(`${id}-in`, 'Field', {height: 32, layout: 'horizontal', alignItems: 'center', gap: '$--spacing-sm', padding: [0, '$--spacing-md']}, [
          icon(`${id}-ini`, 'search', {size: 14, fill: '$--muted-foreground'}), text(`${id}-int`, placeholder, {fill: '$--muted-foreground'}),
        ]),
        frame(`${id}-list`, 'List', {layout: 'vertical', gap: 0, padding: '$--spacing-xxs'}, rows),
      ]);
    }
    const command = surface(`pal-cmd-${suffix}`, 'Search agents, workspaces and commands', [
      screenMenuItem(`pal-c0-${suffix}`, 'sasu-web-design-system-reset', {state: 'highlighted', glyph: 'folder-git-2'}),
      screenMenuItem(`pal-c1-${suffix}`, 'New Worktree', {glyph: 'git-branch-plus'}),
      screenMenuItem(`pal-c2-${suffix}`, 'Open Settings', {glyph: 'settings'}),
    ]);
    const file = surface(`pal-file-${suffix}`, 'Open file', [
      screenMenuItem(`pal-f0-${suffix}`, '한글 노트.md', {state: 'highlighted', glyph: 'file-text'}),
      screenMenuItem(`pal-f1-${suffix}`, 'scripts/pen-screens.mjs', {glyph: 'file-code'}),
    ]);
    return [frame(`pal-wrap-${suffix}`, 'Wrap', {layout: 'horizontal', gap: '$--spacing-lg'}, [command, file])];
  }
  return screenSheet('screen-palette', 'Screen / Palette', 'web/src/Palette.tsx over Command/CommandDialog: the command palette and the file palette, same List Item row idiom as System / Command.', build, build);
}

// -- Screen / Dialogs and Sheets -------------------------------------------------

function buildDialogs(tokens) {
  const H = num(tokens, '--size-control');
  function build(suffix) {
    // WorkspaceDialogs.tsx's worktree form: Branch (Input), Base (Select),
    // "Start in the new pane" (a plain radio choice, Terminal only/Claude/Codex),
    // Purpose (optional, Input) - not a bare title, the fields are the dialog.
    const newWorktree = screenDialogSurface(`dlg-neww-${suffix}`, {
      width: 380, title: 'New worktree in sasu-web-design-system-reset', description: '~/projects/sasu/worktrees/web-design-system-reset',
      body: [
        screenField(`dlg-neww-branch-${suffix}`, 'Branch', screenInput(`dlg-neww-branchv-${suffix}`, {placeholder: 'feature/name', mono: true, width: 348})),
        screenField(`dlg-neww-base-${suffix}`, 'Base', screenSelect(`dlg-neww-basev-${suffix}`, {content: 'main', width: 348})),
        screenField(`dlg-neww-agent-${suffix}`, 'Start in the new pane', frame(`dlg-neww-agentrow-${suffix}`, 'Row', {layout: 'horizontal', gap: '$--spacing-md'}, [
          screenRadioItem(`dlg-neww-agent0-${suffix}`, 'Terminal only', true),
          screenRadioItem(`dlg-neww-agent1-${suffix}`, 'Claude', false),
          screenRadioItem(`dlg-neww-agent2-${suffix}`, 'Codex', false),
        ])),
        screenField(`dlg-neww-purpose-${suffix}`, 'Purpose (optional)', screenInput(`dlg-neww-purposev-${suffix}`, {placeholder: '', width: 348})),
      ],
      actions: [screenButton(`dlg-neww-cancel-${suffix}`, 'Cancel', {variant: 'ghost', height: H}), screenButton(`dlg-neww-ok-${suffix}`, 'Create worktree', {height: H})],
    });
    const deleteWorktree = screenDialogSurface(`dlg-delw-${suffix}`, {
      width: 380, title: 'Delete worktree prd/checkout-row-d?', description: '~/projects/sasu/worktrees/checkout-row-d',
      actions: [screenButton(`dlg-delw-cancel-${suffix}`, 'Keep worktree', {variant: 'secondary', height: H}), screenButton(`dlg-delw-ok-${suffix}`, 'Delete worktree', {variant: 'destructive', height: H})],
    });
    const removeProject = screenDialogSurface(`dlg-remp-${suffix}`, {
      width: 380, title: 'Remove project sasu-web-design-system-reset?', description: '~/projects/sasu/worktrees/web-design-system-reset',
      actions: [screenButton(`dlg-remp-cancel-${suffix}`, 'Keep project', {variant: 'secondary', height: H}), screenButton(`dlg-remp-ok-${suffix}`, 'Close 3 panes and remove', {variant: 'destructive', height: H})],
    });
    return [
      frame(`dlg-row1-${suffix}`, 'Row 1', {layout: 'horizontal', gap: '$--spacing-lg'}, [newWorktree, deleteWorktree, removeProject]),
    ];
  }
  function build2(suffix) {
    return [frame(`dlg-row2-${suffix}`, 'Row 2', {layout: 'horizontal', gap: '$--spacing-lg'}, [
      screenDialogSurface(`dlg-purp2-${suffix}`, {width: 380, title: 'Purpose', description: 'What this workspace is for', actions: [screenButton(`dlg-purp2-cancel-${suffix}`, 'Cancel', {variant: 'ghost', height: H}), screenButton(`dlg-purp2-ok-${suffix}`, 'Save', {height: H})]}),
      screenDialogSurface(`dlg-draft2-${suffix}`, {width: 380, title: 'Unsaved drafts', actions: [screenButton(`dlg-draft2-discard-${suffix}`, 'Discard drafts', {variant: 'destructive', height: H}), screenButton(`dlg-draft2-ok-${suffix}`, 'Keep and continue', {height: H})]}),
      screenDialogSurface(`dlg-neww3-${suffix}`, {width: 380, title: 'New workspace', description: '~/…', actions: [screenButton(`dlg-neww3-cancel-${suffix}`, 'Cancel', {variant: 'ghost', height: H}), screenButton(`dlg-neww3-ok-${suffix}`, 'Add workspace', {height: H})]}),
      screenDialogSurface(`dlg-short2-${suffix}`, {width: 260, title: 'Keyboard shortcuts', actions: [screenButton(`dlg-short2-close-${suffix}`, 'Close', {variant: 'ghost', height: H})]}),
    ])];
  }
  function full(suffix) { return [...build(suffix), ...build2(suffix)]; }
  return screenSheet('screen-dialogs', 'Screen / Dialogs and Sheets', 'web/src/WorkspaceDialogs.tsx, NewWorkspace.tsx, DraftRecovery.tsx, ShortcutSheet.tsx: every Dialog/AlertDialog surface the shell opens, shaped from System / Dialog and System / Alert Dialog with Button refs for every action.', full, full);
}

// -- Screen / Menus and Overlays --------------------------------------------------

function buildMenus() {
  function build(suffix) {
    // workspaceManage.ts projectMenu(): Pin/Unpin, New worktree…, Remove project…
    // (no Rename/Duplicate - those belong to no menu this shell has today, and
    // Remove project carries no destructive style in the real menu either).
    const rowMenu = screenMenuContent(`mn-row-${suffix}`, 200, [
      screenMenuItem(`mn-row0-${suffix}`, 'Pin'),
      screenMenuItem(`mn-row1-${suffix}`, 'New worktree…'),
      screenMenuItem(`mn-row2-${suffix}`, 'Remove project…'),
    ]);
    // ExplorerTree.tsx's row-context branch (a file row, not the empty-area
    // branch that offers New File/New Folder instead): Open to the side (with
    // its width-reason, since this Explorer is too narrow to split), Rename,
    // then Move to Trash separated and destructive.
    const explorerCtx = screenMenuContent(`mn-ctx-${suffix}`, 280, [
      screenMenuItem(`mn-ctx0-${suffix}`, 'Open to the side', {state: 'disabled', reason: 'This view area is too narrow to open a second view beside it.', reasonWidth: 240}),
      screenMenuItem(`mn-ctx1-${suffix}`, 'Rename'),
      screenMenuSeparator(`mn-ctxsep-${suffix}`),
      screenMenuItem(`mn-ctx2-${suffix}`, 'Move to Trash', {state: 'destructive'}),
    ]);
    const devicePicker = frame(`mn-dev-${suffix}`, 'Device picker', {width: 260, layout: 'vertical', padding: '$--spacing-xxs', gap: 0, cornerRadius: '$--radius-sm', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      text(`mn-devhdr-${suffix}`, 'Devices', {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
      screenDevicePickerRow(`mn-dev0-${suffix}`, {name: 'This Mac', detail: 'Local · 2 agents', selected: true}),
      screenDevicePickerRow(`mn-dev1-${suffix}`, {name: 'Mac mini', detail: 'Remote · Connected · 7 agents'}),
    ]);
    // ExplorerTree.tsx renders this as inline warning-colored text directly
    // under the Explorer root row, not a filled banner with an icon.
    const notice = frame(`mn-notice-${suffix}`, 'Explorer notice', {width: 360, layout: 'vertical', gap: '$--spacing-xxs'}, [
      frame(`mn-noticeroot-${suffix}`, 'Root', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', width: 360}, [
        text(`mn-noticeroott-${suffix}`, 'demo', {size: '$--text-caption', weight: '600', fill: '$--muted-foreground'}),
        icon(`mn-noticerefresh-${suffix}`, 'refresh-cw', {size: 12, fill: '$--muted-foreground'}),
      ]),
      text(`mn-noticet-${suffix}`, 'Git status unavailable: ~/projects/sasu/demo is not inside a Git repository', {fill: '$--warning', size: '$--text-caption', width: 360}),
    ]);
    return [frame(`mn-wrap-${suffix}`, 'Wrap', {layout: 'vertical', gap: '$--spacing-lg'}, [
      frame(`mn-row-a-${suffix}`, 'Row', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [rowMenu, explorerCtx, devicePicker]),
      notice,
    ])];
  }
  return screenSheet('screen-menus', 'Screen / Menus and Overlays', 'entry-menu.tsx RowMenu/EntryContextMenu, DevicePicker.tsx, and the Explorer git-status notice: overlays shown anchored in their real screen context rather than the abstract System gallery.', build, build);
}

// -- assembly ---------------------------------------------------------------------

export function readLocalVariables(root) {
  const {expected} = readTokenPlan(root);
  const {document} = loadCanvas(root);
  const libraryAuthored = {};
  for (const name of LIBRARY_AUTHORED_VARIABLES) {
    if (!(name in document.variables)) throw new Error(`pen-screens needs library-authored variable ${name}, which ${CANVAS} no longer carries`);
    libraryAuthored[name] = document.variables[name];
  }
  return {...Object.fromEntries(expected), ...libraryAuthored};
}

export function screenSheets(tokens, root) {
  setLibraryRoot(root, tokens);
  return [
    {name: 'Screen / Main', build: () => buildMain(tokens)},
    {name: 'Screen / Project Overview', build: () => buildProjectOverview(tokens)},
    {name: 'Screen / Workspace', build: () => buildWorkspace()},
    {name: 'Screen / Project Sessions', build: () => buildSessions()},
    {name: 'Screen / Settings', build: () => buildSettings(tokens)},
    {name: 'Screen / Palette', build: () => buildPalette()},
    {name: 'Screen / Dialogs and Sheets', build: () => buildDialogs(tokens)},
    {name: 'Screen / Menus and Overlays', build: () => buildMenus()},
  ];
}
