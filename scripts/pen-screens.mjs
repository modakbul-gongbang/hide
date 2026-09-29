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
import {BUTTON_VARIANTS, BADGE_VARIANTS, frame, icon, num, text} from './pen-system.mjs';
import {diskCleanupRows} from './pen-screens-disk.mjs';

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
// (Card/Modal shaped header + footer actions as Button refs). The description
// is a path in mono unless `prose`; `titleGlyph` leads the title.
function screenDialogSurface(id, {width, title, titleGlyph, description, prose = false, body, actions}) {
  const heading = text(`${id}-title`, title, {fill: '$--foreground', size: '$--text-title', weight: '600'});
  return frame(id, 'Surface', {
    width, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline',
    strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-md',
  }, [
    frame(`${id}-hdr`, 'Header', {layout: 'vertical', gap: '$--spacing-xs', padding: ['$--spacing-lg', '$--spacing-lg', 0, '$--spacing-lg']}, [
      titleGlyph ? frame(`${id}-titlerow`, 'Title', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [icon(`${id}-titleg`, titleGlyph, {size: 14, fill: '$--muted-foreground'}), heading]) : heading,
      ...(description ? [text(`${id}-desc`, description, prose ? {fill: '$--subtle-foreground', size: '$--text-body', width: width - 2 * 16} : {fill: '$--subtle-foreground', size: '$--text-caption', mono: true, width: width - 2 * 16})] : []),
    ]),
    ...(body ? [frame(`${id}-body`, 'Body', {layout: 'vertical', gap: '$--spacing-md', padding: [0, '$--spacing-lg']}, body)] : []),
    // DialogFooter ends its actions at the surface's right edge.
    frame(`${id}-ftr`, 'Footer', {layout: 'horizontal', justifyContent: 'end', alignItems: 'center', gap: '$--spacing-sm', width: 'fill_container', padding: [0, '$--spacing-lg', '$--spacing-lg', '$--spacing-lg']}, actions),
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

// -- the status mark --------------------------------------------------------------
// status-mark.tsx: `●` and `○` are a dot and a ring of one diameter, and every other
// mark is its glyph in the same box, so no mark renders larger than another. The
// master that carries a mark (Sidebar agent row) holds the three as siblings in
// one box, and a row's mark enables the one it draws.
function markOverrides(nodes, symbol, fill) {
  if (symbol === '●') return {[nodes.dot]: {enabled: true, fill}, [nodes.ring]: {enabled: false}, [nodes.glyph]: {enabled: false}};
  if (symbol === '○') return {[nodes.dot]: {enabled: false}, [nodes.ring]: {enabled: true, stroke: fill}, [nodes.glyph]: {enabled: false}};
  return {[nodes.dot]: {enabled: false}, [nodes.ring]: {enabled: false}, [nodes.glyph]: {enabled: true, content: symbol, fill}};
}

// The same mark for a row this document draws itself rather than from a master.
function screenStatusMark(tokens, id, symbol, fill) {
  const box = num(tokens, '--size-agent-mark');
  const size = num(tokens, '--size-status-mark');
  const shape = symbol === '●'
    ? {type: 'ellipse', id: `${id}-dot`, name: 'Dot', width: size, height: size, fill}
    : symbol === '○'
      ? {type: 'ellipse', id: `${id}-ring`, name: 'Ring', width: size, height: size, stroke: fill, strokeWidth: num(tokens, '--size-hairline'), strokeAlignment: 'inner'}
      : text(`${id}-glyph`, symbol, {fill, mono: true, size: '$--text-caption'});
  return frame(id, 'Status mark', {width: box, height: box, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [shape]);
}

// -- composite Component masters: themedXref restates every internal default color,
// so only content (and any state that changes what the master would not restate on
// its own, like the status dot's color) needs to be named at each call site. -------

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

// sidebar-agent-row.tsx's SidebarAgentRow, from the library's Sidebar agent row:
// line one is the marks, the title, the chips, the badge, the elapsed time and a
// parent's fold; a request or news has its own line and Agents adds the context
// line. `inset` is where the marks start. Every row keeps the fold slot, so its
// time ends on the column every sidebar row's time ends on; `fold` is `folded`
// on a parent whose chevron shows at rest, and `unfolded` or null leaves the
// slot empty at rest. A list with nothing to fold (an Overview card) passes
// `none`, and the row has no fold slot at all.
function screenSidebarAgentRow(id, {title, symbol = '●', color = '$--agent-working', provider = 'claude', age, line, lineFill = '$--warning', place, device, branch, badge, fold = null, bright = false, selected = false, inset = 4, width = 268}) {
  return themedXref(id, 'sidebar-agent-row', title, {width, padding: ['$--spacing-xs', '$--spacing-xs', '$--spacing-xs', inset], ...(selected ? {fill: '$--secondary'} : {})}, {
    ...markOverrides({dot: 'sar-dot', ring: 'sar-ring', glyph: 'sar-glyph'}, symbol, color),
    'sar-provider': {fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${provider}.png`, mode: 'fit'}},
    // 12/400 in every state: attention and selection brighten the title, never thicken it.
    'sar-title': {content: title, ...(bright || selected ? {fill: '$--foreground'} : {})},
    'sar-elapsed': {content: age},
    'sar-line': line ? {enabled: true, content: line, fill: lineFill} : {enabled: false},
    'sar-place': place ? {enabled: true, content: place} : {enabled: false},
    'sar-device': device ? {enabled: true} : {enabled: false},
    'sar-device-label': {content: device ?? ''},
    'sar-branch': branch ? {enabled: true} : {enabled: false},
    'sar-branch-label': {content: branch ?? ''},
    'sar-badge': badge ? {enabled: true} : {enabled: false},
    'sar-badge-label': {content: badge ?? ''},
    'sar-fold': fold === 'none' ? {enabled: false} : {opacity: fold === 'folded' ? 1 : 0},
    'sar-chevron': {icon: fold === 'unfolded' ? 'chevron-down' : 'chevron-right'},
  });
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

// The approved R1 remote-device treatment: the existing Badge shape, a server
// glyph, and the real device display name. It is shared by folded lineage
// summaries, the child list, and pane-header child chips.
function screenDeviceChip(id, name) {
  return frame(id, 'Device chip', {layout: 'horizontal', gap: '$--spacing-xxs', padding: [0, '$--spacing-xs'], height: 16, alignItems: 'center', cornerRadius: '$--radius-xs', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
    icon(`${id}-g`, 'server', {size: 10, fill: '$--muted-foreground'}),
    text(`${id}-t`, name, {size: '$--text-micro', fill: '$--subtle-foreground', weight: '500'}),
  ]);
}

function screenLineageSummary(tokens, id, {status, branch, pr, device, more}) {
  const marks = {
    working: ['●', '$--agent-working'], done: ['✓', '$--success'], question: ['?', '$--warning'], seen: ['○', '$--muted-foreground'],
  };
  const [symbol, fill] = marks[status];
  return frame(id, 'Other checkout lineage', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', padding: [0, '$--spacing-xs', 0, 40]}, [
    screenStatusMark(tokens, `${id}-m`, symbol, fill),
    text(`${id}-b`, branch, {size: '$--text-caption', fill: '$--subtle-foreground'}),
    ...(pr ? [text(`${id}-pr`, pr, {size: '$--text-caption', fill: '$--muted-foreground', mono: true})] : []),
    ...(device ? [screenDeviceChip(`${id}-d`, device)] : []),
    ...(more ? [text(`${id}-more`, `+${more}`, {size: '$--text-caption', fill: '$--muted-foreground', mono: true})] : []),
  ]);
}

function screenLineageDetails(tokens, suffix) {
  const children = [
    {status: 'working', title: '레이아웃 재구조화', branch: 'web-view-overlay', age: '4m'},
    {status: 'done', title: '패널 디자인 검토', branch: 'web-side-panel', age: '13m'},
    {status: 'done', title: '원격 분리', branch: 'hcoord-decouple', device: 'mini', age: '1h'},
  ];
  const popover = frame(`main-lineage-pop-${suffix}`, 'Children list', {width: num(tokens, '--size-agent-children-popover'), layout: 'vertical', fill: '$--popover', cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', padding: ['$--spacing-xxs', 0]}, [
    ...children.map((child, index) => frame(`main-lineage-pop${index}-${suffix}`, 'Child', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', padding: ['$--spacing-xs', '$--spacing-sm'], ...(index === 0 ? {fill: '$--accent'} : {})}, [
      screenStatusMark(tokens, `main-lineage-pop${index}-m-${suffix}`, child.status === 'working' ? '●' : '✓', child.status === 'working' ? '$--agent-working' : '$--success'),
      frame(`main-lineage-pop${index}-txt-${suffix}`, 'Text', {layout: 'vertical', gap: 0}, [
        text(`main-lineage-pop${index}-t-${suffix}`, child.title, {size: '$--text-caption'}),
        frame(`main-lineage-pop${index}-sub-${suffix}`, 'Sub', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
          text(`main-lineage-pop${index}-s-${suffix}`, child.status === 'working' ? 'Working' : 'Done', {size: '$--text-caption', fill: '$--muted-foreground'}),
          text(`main-lineage-pop${index}-b-${suffix}`, child.branch, {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
          ...(child.device ? [screenDeviceChip(`main-lineage-pop${index}-d-${suffix}`, child.device)] : []),
        ]),
      ]),
      frame(`main-lineage-pop${index}-gap-${suffix}`, 'Spacer', {width: 'fill_container', height: 1}, []),
      text(`main-lineage-pop${index}-a-${suffix}`, child.age, {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
    ])),
    frame(`main-lineage-pop-rule-${suffix}`, 'Rule', {width: 'fill_container', height: 1, fill: '$--border'}, []),
    text(`main-lineage-pop-open-${suffix}`, '하위 에이전트 펼치기', {size: '$--text-caption', fill: '$--subtle-foreground'}),
  ]);
  const chip = (child, index) => frame(`main-lineage-chip${index}-${suffix}`, 'Child chip', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', padding: [0, '$--spacing-xs'], height: 20, cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
    screenStatusMark(tokens, `main-lineage-chip${index}-m-${suffix}`, child.status === 'working' ? '●' : '✓', child.status === 'working' ? '$--agent-working' : '$--success'),
    text(`main-lineage-chip${index}-b-${suffix}`, child.branch, {size: '$--text-caption', fill: '$--subtle-foreground', mono: true}),
    ...(child.device ? [screenDeviceChip(`main-lineage-chip${index}-d-${suffix}`, child.device)] : []),
  ]);
  const pane = frame(`main-lineage-pane-${suffix}`, 'Pane header lineage', {width: 640, layout: 'vertical', cornerRadius: '$--radius-sm', clip: true, stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
    frame(`main-lineage-pane-h-${suffix}`, 'Pane header', {width: 640, height: num(tokens, '--size-pane-header'), layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-sm'], fill: '$--secondary'}, [
      screenStatusMark(tokens, `main-lineage-pane-m-${suffix}`, '○', '$--agent-working'),
      text(`main-lineage-pane-t-${suffix}`, '카드 상태 시트 설계', {size: '$--text-caption'}),
    ]),
    frame(`main-lineage-pane-c-${suffix}`, 'Children', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', padding: ['$--spacing-xs', '$--spacing-sm'], fill: '$--secondary'}, children.map(chip)),
    frame(`main-lineage-pane-b-${suffix}`, 'Body', {height: 64, fill: '$--card'}, []),
  ]);
  return frame(`main-lineage-details-${suffix}`, 'Lineage detail states', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [popover, pane]);
}

// The weekly usage chip at the sidebar's foot (web/src/components/weekly-usage.tsx):
// the master draws Claude Code's mark, so only another provider's mark is named.
function screenUsageChip(id, {provider, value}) {
  const mark = provider === 'claude' ? {} : {'usage-chip-mark': {fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${provider}.png`, mode: 'fit'}}};
  return themedXref(id, 'usage-chip', `Usage ${provider}`, {}, {...mark, 'usage-chip-percent': {content: value}});
}

// The Agents/Projects sidebar App.tsx/sidebar.tsx always shows beside a
// screen's own content; every sheet that draws a whole screen (Main,
// Workspace) includes it so a reader sees the whole thing, not just its
// own feature in isolation. It is --size-sidebar-ideal wide, as sidebar.tsx
// draws it, so its footer holds the device picker, the usage chips and the
// Settings gear side by side. Its top is the global Overview row, marked while
// `overview` is the screen beside it, over the Projects | Agents strip on
// Agents, where Search alone ends the strip (PRD sidebar-shell D-02..D-04).
function screenSidebar(tokens, id, suffix, agents, {overview = false} = {}) {
  return frame(`${id}-${suffix}`, 'Sidebar', {width: num(tokens, '--size-sidebar-ideal'), layout: 'vertical', gap: '$--spacing-md', fill: '$--sidebar', padding: '$--spacing-md', cornerRadius: '$--radius-md'}, [
    frame(`${id}-overview-${suffix}`, 'Overview', {
      width: 'fill_container', height: num(tokens, '--size-project-row'), layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-sm',
      ...(overview ? {fill: '$--secondary'} : {}),
    }, [
      icon(`${id}-overviewi-${suffix}`, 'house', {size: num(tokens, '--size-checkout-icon'), fill: '$--subtle-foreground'}),
      text(`${id}-overviewt-${suffix}`, 'Overview', {size: '$--text-subhead', weight: '600'}),
      frame(`${id}-overviewgap-${suffix}`, 'Spacer', {width: 'fill_container', height: 1}, []),
      text(`${id}-overviewn-${suffix}`, '12 projects', {fill: '$--muted-foreground'}),
    ]),
    frame(`${id}-tabs-${suffix}`, 'Tabs', {width: 'fill_container', layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
      text(`${id}-projectstab-${suffix}`, 'Projects', {size: '$--text-caption', fill: '$--muted-foreground'}),
      text(`${id}-agentstab-${suffix}`, 'Agents', {size: '$--text-caption'}),
      frame(`${id}-tabsgap-${suffix}`, 'Spacer', {width: 'fill_container', height: 1}, []),
      screenIconButton(`${id}-search-${suffix}`, 'search'),
    ]),
    text(`${id}-seen-${suffix}`, 'Seen · 2', {size: '$--text-micro', fill: '$--muted-foreground', weight: '500'}),
    // Agents' rows name their project and checkout on a fixed context line.
    ...agents.flatMap((agent, index) => [
      screenSidebarAgentRow(`${id}-agent${index}-${suffix}`, {
        title: agent.title, symbol: agent.symbol ?? '●', color: agent.statusColor ?? '$--agent-working', age: agent.age ?? '3m', place: agent.place ?? 'herdr-ide › main',
        device: agent.device, badge: agent.badge, fold: agent.fold,
        width: num(tokens, '--size-sidebar-ideal') - 2 * num(tokens, '--spacing-md'),
      }),
      ...(agent.summaries ?? []).map((summary, summaryIndex) => screenLineageSummary(tokens, `${id}-agent${index}-summary${summaryIndex}-${suffix}`, summary)),
    ]),
    frame(`${id}-footer-${suffix}`, 'Footer', {width: 'fill_container', gap: '$--spacing-xs', alignItems: 'center'}, [
      screenSelect(`${id}-device-${suffix}`, {content: 'This Mac', width: 84}),
      frame(`${id}-footergap-${suffix}`, 'Gap', {width: 'fill_container', height: 1}, []),
      screenUsageChip(`${id}-usage0-${suffix}`, {provider: 'claude', value: '62%'}),
      screenUsageChip(`${id}-usage1-${suffix}`, {provider: 'codex', value: '59%'}),
      screenIconButton(`${id}-settings-${suffix}`, 'settings', {size: 20}),
    ]),
  ]);
}

// A scope's facts line (MainScreen.tsx FACTS_LINE): mono caption facts, each a
// glyph and its number, drawn only when the system has the number. The open
// issues carry their source after them, muted (ProjectOverview.tsx IssuesFact).
function factsLine(id, facts) {
  return frame(id, 'Facts', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center'}, facts.map((fact, index) =>
    frame(`${id}-${index}`, fact.label, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      icon(`${id}-${index}-g`, fact.glyph, {size: 12, fill: fact.fill ?? '$--subtle-foreground'}),
      text(`${id}-${index}-t`, fact.label, {size: '$--text-caption', fill: fact.fill ?? '$--subtle-foreground', mono: true}),
      ...(fact.after ? [text(`${id}-${index}-a`, fact.after, {size: '$--text-caption', fill: '$--muted-foreground', mono: true})] : []),
    ])));
}

// Pen draws no ellipsis and no line clamp, so text the web truncates is
// written already cut. The widths are an estimate per script for Inter and
// JetBrains Mono at `size`; the exported sheet is the check.
function textWidth(content, size, mono = false) {
  let width = 0;
  for (const character of content) {
    const code = character.codePointAt(0);
    if ((code >= 0xac00 && code <= 0xd7a3) || (code >= 0x3130 && code <= 0x318f)) width += size * 0.93;
    else if (mono) width += size * 0.6;
    else if (character === ' ') width += size * 0.28;
    else if (/[A-Z#@%MW]/.test(character)) width += size * 0.68;
    else if (/[il.,:;'|!]/.test(character)) width += size * 0.28;
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

// `line-clamp-2`: what fits in two lines of `max`, less the words a wrap strands.
function fitLines(content, max, size, lines = 2) {
  return fitText(content, max * lines - size * 3, size);
}

// 새 이슈, the Overview's primary action, and its C keycap (ProjectOverview.tsx,
// MainScreen.tsx). The web draws the Kbd inside the button; a Button ref
// cannot hold another ref, so the keycap stands right beside it.
function newIssueButton(tokens, id) {
  return frame(id, '새 이슈', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
    screenButton(`${id}-b`, '새 이슈', {height: num(tokens, '--size-control'), icon: 'plus'}),
    themedXref(`${id}-k`, 'kbd-m', 'C', {}, {'kbd-t': {content: 'C'}}),
  ]);
}

// The view tabs under a scope's header: the Agents trigger carries, in
// warning, how many agents wait on the operator - the only place outside the
// cards that says so (MainScreen.tsx; a project has tiles instead).
function viewTabs(id, items, activeIndex, waiting) {
  return frame(id, 'Tabs', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'}, items.map((label, index) => {
    const active = index === activeIndex;
    const content = {'tab-t': {content: label, fill: active ? '$--foreground' : '$--subtle-foreground'}};
    if (label !== 'Agents' || waiting === 0) return themedXref(`${id}-${index}`, 'tab-m', label, active ? {fill: '$--secondary'} : {}, content);
    return frame(`${id}-${index}w`, label, {layout: 'horizontal', alignItems: 'center', height: 24, padding: [0, '$--spacing-sm', 0, 0], cornerRadius: '$--radius-xs', ...(active ? {fill: '$--secondary'} : {})}, [
      themedXref(`${id}-${index}`, 'tab-m', label, {padding: [0, '$--spacing-xs', 0, '$--spacing-sm']}, content),
      text(`${id}-${index}-n`, String(waiting), {size: '$--text-caption', fill: '$--warning'}),
    ]);
  }));
}

// The tab row: the view tabs, and on the Tasks view its Board | List |
// Dependencies mode on the right (TaskBoards.tsx TasksModeToggle).
const TASKS_MODES = ['board', 'list', 'dependencies'];
function viewRow(id, tabs, mode, width) {
  return frame(id, 'View row', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', width}, [
    tabs,
    ...(mode ? [screenToggleGroup(`${id}-mode`, ['Board', 'List', 'Dependencies'], TASKS_MODES.indexOf(mode))] : []),
  ]);
}

// A project's Overview header (ProjectOverview.tsx, PRD
// overview-lenses-tiles-agents B1, B8, B9): the path back, New agent as the
// quiet action and 새 이슈 as the primary one, the facts line with the chosen
// view's mode control at its right end, then the tiles where the tab row was.
const AGENTS_MODES = ['checkouts', 'lineage'];
function overviewHeader(tokens, id, suffix, {project, facts, view, width, mode}) {
  const control = view === 'agents'
    ? [screenToggleGroup(`${id}-mode-${suffix}`, ['체크아웃', '계보'], AGENTS_MODES.indexOf(mode ?? 'checkouts'))]
    : view === 'issues' ? [frame(`${id}-ictl-${suffix}`, 'Filter and mode', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
        screenIconButton(`${id}-filter-${suffix}`, 'list-filter', {size: num(tokens, '--size-control-sm')}),
        screenToggleGroup(`${id}-mode-${suffix}`, ['Board', 'List', 'Dependencies'], TASKS_MODES.indexOf(mode ?? 'board')),
      ])] : [];
  return frame(`${id}-${suffix}`, 'Header', {layout: 'vertical', gap: '$--spacing-sm', width}, [
    frame(`${id}-title-${suffix}`, 'Title row', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'center', width}, [
      frame(`${id}-crumb-${suffix}`, 'Path', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        text(`${id}-crumb1-${suffix}`, 'Overview', {size: '$--text-caption', fill: '$--subtle-foreground'}),
        text(`${id}-crumb2-${suffix}`, '/', {size: '$--text-caption', fill: '$--muted-foreground'}),
        text(`${id}-crumb3-${suffix}`, project, {size: '$--text-headline', weight: '600'}),
      ]),
      frame(`${id}-gap-${suffix}`, 'Spacer', {width: 'fill_container', height: 1}, []),
      screenButton(`${id}-new-${suffix}`, 'New agent', {variant: 'ghost', height: num(tokens, '--size-control'), icon: 'square-terminal'}),
      newIssueButton(tokens, `${id}-issue-${suffix}`),
    ]),
    frame(`${id}-factsrow-${suffix}`, 'Facts row', {layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', width}, [
      factsLine(`${id}-facts-${suffix}`, facts),
      ...control,
    ]),
    lensTiles(tokens, `${id}-tiles-${suffix}`, view, width),
    frame(`${id}-rule-${suffix}`, 'Rule', {width, height: 1, fill: '$--border'}, []),
  ]);
}

// The tiles (OverviewLenses.tsx LensTiles, B1-B5): one width each, the chosen
// one outlined; the name, the yellow badge of the operator's turn, the large
// number and its unit, and one bar whose parts carry the bar's tones.
const HERDR_TILES = [
  {id: 'agents', label: 'Agents', value: '11', badge: '2', bar: [['$--warning', 2], ['$--success', 2], ['$--agent-working', 2], ['$--muted-foreground', 5]]},
  {id: 'issues', label: 'Issues', value: '22', unit: '열림', bar: [['$--muted-foreground', 18], ['$--warning', 2], ['$--success', 2]]},
  {id: 'prs', label: 'PRs', value: '9', unit: '열림', badge: '4', bar: [['$--warning', 4], ['$--agent-working', 1], ['$--destructive', 4]]},
  {id: 'sessions', label: 'Sessions', value: '14', unit: '오늘', bar: [['$--file-orange', 9], ['$--agent-working', 5]]},
];
function lensTiles(tokens, id, view, width) {
  const gap = num(tokens, '--spacing-md');
  const tileWidth = Math.floor((width - gap * (HERDR_TILES.length - 1)) / HERDR_TILES.length);
  const inner = tileWidth - 2 * num(tokens, '--spacing-sm');
  const barGap = num(tokens, '--spacing-xxs');
  return frame(id, 'Tiles', {layout: 'horizontal', gap: '$--spacing-md', width}, HERDR_TILES.map((tile) => {
    const total = tile.bar.reduce((sum, [, count]) => sum + count, 0);
    const room = inner - barGap * (tile.bar.length - 1);
    return frame(`${id}-${tile.id}`, tile.label, {
      layout: 'vertical', gap: '$--spacing-xs', padding: '$--spacing-sm', width: tileWidth, fill: '$--card', cornerRadius: '$--radius-md',
      stroke: tile.id === view ? '$--primary' : '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    }, [
      frame(`${id}-${tile.id}-name`, 'Name', {layout: 'horizontal', alignItems: 'center', width: inner}, [
        text(`${id}-${tile.id}-label`, tile.label, {size: '$--text-body', weight: '500'}),
        frame(`${id}-${tile.id}-sp`, 'Spacer', {width: 'fill_container', height: 1}, []),
        ...(tile.badge ? [text(`${id}-${tile.id}-badge`, tile.badge, {size: '$--text-caption', fill: '$--warning', mono: true})] : []),
      ]),
      frame(`${id}-${tile.id}-value`, 'Value', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'end', height: num(tokens, '--size-control')}, [
        text(`${id}-${tile.id}-n`, tile.value, {size: '$--text-headline', weight: '600', mono: true}),
        ...(tile.unit ? [text(`${id}-${tile.id}-u`, tile.unit, {size: '$--text-caption', fill: '$--muted-foreground'})] : []),
      ]),
      frame(`${id}-${tile.id}-bar`, 'Bar', {layout: 'horizontal', gap: '$--spacing-xxs', width: inner, height: num(tokens, '--lens-bar-height')},
        tile.bar.map(([fill, count], index) => frame(`${id}-${tile.id}-bar${index}`, 'Part', {width: Math.max(2, Math.round(room * count / total)), height: num(tokens, '--lens-bar-height'), fill, cornerRadius: num(tokens, '--lens-bar-height') / 2}, []))),
    ]);
  }));
}

const HERDR_FACTS = [
  {glyph: 'folder-git-2', label: '20 worktrees'},
  {glyph: 'hard-drive', label: '37 GB'},
  {glyph: 'arrow-down', label: 'main ↓10 behind origin', fill: '$--warning'},
  {glyph: 'git-merge', label: '12 merged → 정리', fill: '$--pr-merged'},
];

// -- the issue-first boards (TaskBoards.tsx) -------------------------------------

const PR_TONE = {open: '$--pr-open', draft: '$--pr-draft', merged: '$--pr-merged', closed: '$--pr-closed'};
const REVIEW_WORD = {review_required: ['리뷰 필요', '$--muted-foreground'], changes_requested: ['변경 요청', '$--warning'], approved: ['승인됨', '$--success']};
const AGENT_MARK = {ask: ['?', '$--warning'], work: ['●', '$--agent-working'], done: ['✓', '$--success'], seen: ['○', '$--muted-foreground'], error: ['×', '$--destructive']};
// A row the operator has to look at: its title in foreground, medium weight on the web.
const ATTENTION = new Set(['ask', 'done', 'error']);

// The Tasks board and Dependencies mode both scopes draw
// (TaskBoards.tsx over projectBoard.ts), authored here on local tokens since
// no library master draws a task card; a card's agent row is the library's
// Sidebar agent row, as agent-row.tsx is on the web, and every chip, button,
// keycap and toggle is a library ref.
// A pull request's CI mark once read: passing, failed or still running
// (web/src/TaskBoards.tsx ChecksMark); nothing before GitHub answers.
function ciMark(tokens, id, checks) {
  if (checks === 'passing') return icon(id, 'check', {size: 12, fill: '$--success'});
  if (checks === 'failed') return icon(id, 'x', {size: 12, fill: '$--destructive'});
  return checks === 'pending' ? screenStatusMark(tokens, id, '●', '$--muted-foreground') : null;
}

function issueBoardParts(tokens) {
  const column = num(tokens, '--home-column-width');
  const inner = column - 2 * num(tokens, '--spacing-sm');
  const small = num(tokens, '--size-control-sm');
  const branchMax = num(tokens, '--home-collapsed-width') - num(tokens, '--size-icon-sm') - num(tokens, '--spacing-xxs');
  const dimmed = num(tokens, '--opacity-secondary');
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);
  const kbd = (id, label) => themedXref(id, 'kbd-m', label, {}, {'kbd-t': {content: label}});
  const caption = (id, content, fill = '$--muted-foreground', mono = false) => text(id, content, {size: '$--text-caption', fill, mono});

  // The id the source shows, after its glyph: circle-dot for GitHub, a page for Local (TaskId).
  function taskId(id, {source = 'github', label}) {
    return frame(id, 'Id', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      icon(`${id}-g`, source === 'local' ? 'file-text' : 'circle-dot', {size: 12, fill: '$--muted-foreground'}),
      caption(`${id}-t`, label, '$--muted-foreground', true),
    ]);
  }

  // Where the work is: a branch in mono, a house for the primary checkout (Place).
  function place(id, branch, {primary = false, max} = {}) {
    return frame(id, 'Branch', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      icon(`${id}-g`, primary ? 'house' : 'git-branch', {size: 12, fill: '$--muted-foreground'}),
      caption(`${id}-t`, max ? fitText(branch, max, 11, true) : branch, '$--muted-foreground', true),
    ]);
  }

  // The result (PrChipView): #n on an outline Badge in its lifecycle colour,
  // the CI mark once read, and on a card the review GitHub asks for.
  function prChip(id, {number, tone = 'open', checks, review}) {
    const fill = PR_TONE[tone];
    const ci = ciMark(tokens, `${id}-ci`, checks);
    return frame(id, `PR #${number}`, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      themedXref(`${id}-b`, 'eHAjc', `#${number}`, BADGE_VARIANTS.outline.overrides, {xXuNa: {enabled: true, icon: 'git-pull-request', fill}, n8L5dm: {content: `#${number}`, fill}}),
      ...(ci ? [ci] : []),
      ...(review ? [caption(`${id}-rv`, REVIEW_WORD[review][0], REVIEW_WORD[review][1])] : []),
    ]);
  }

  // A card's agent row: the Sidebar agent row, spanning the card, with no fold
  // column (agent-row.tsx with onToggleTree null). A request keeps its warning
  // line and news its bright one; a quiet line waits for the pointer.
  function agentRow(id, agent, width) {
    const [symbol, color] = AGENT_MARK[agent.mark];
    const lineWidth = width - 44;
    return screenSidebarAgentRow(id, {
      title: fitText(agent.title, width - 92, 12), symbol, color, provider: agent.provider ?? 'claude', age: agent.age,
      line: agent.line ? fitText(agent.line, lineWidth, 11) : null, lineFill: agent.tone === 'request' ? '$--warning' : '$--foreground',
      bright: ATTENTION.has(agent.mark), inset: 0, width, fold: 'none',
    });
  }

  // A GitHub label: a dot in its colour and its name (IssueLabelView).
  function label(id, [name, fill]) {
    return frame(id, name, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      frame(`${id}-dot`, 'Dot', {width: num(tokens, '--issue-label-dot'), height: num(tokens, '--issue-label-dot'), cornerRadius: num(tokens, '--issue-label-dot') / 2, fill}, []),
      caption(`${id}-t`, name),
    ]);
  }

  // Where the work is (CheckoutChipView): the branch, ↑N and the changed files in warning.
  function checkoutChip(id, card, max) {
    return frame(id, 'Checkout', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
      place(`${id}-br`, card.branch, {primary: card.primary, max}),
      ...(card.ahead ? [caption(`${id}-ahead`, `↑${card.ahead}`, '$--muted-foreground', true)] : []),
      ...(card.files ? [caption(`${id}-files`, `${card.files} files`, '$--warning', true)] : []),
    ]);
  }

  // The id line's reserved slot, filled under the pointer or focus (CardActions,
  // B6): 시작 and S on a backlog issue, Workspace and O in progress, the PR icon
  // in review, a Local issue's edit icon, and ⋯.
  function cardActions(id, card) {
    const first = card.hover === 'start'
      ? [screenButton(`${id}-start`, '시작', {variant: 'secondary', height: small, icon: 'play'}), kbd(`${id}-sk`, 'S')]
      : card.hover === 'workspace' ? [screenIconButton(`${id}-ws`, 'square-terminal', {size: small}), kbd(`${id}-ok`, 'O')]
        : card.hover === 'pr' ? [screenIconButton(`${id}-pr`, 'git-pull-request', {size: small})] : [];
    return frame(id, 'Actions, on hover', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      ...first,
      ...(card.edit ? [screenIconButton(`${id}-edit`, 'pencil', {size: small})] : []),
      screenIconButton(`${id}-menu`, 'ellipsis', {size: small}),
    ]);
  }

  // IssueCardView (PRD overview-lenses-issues B1-B6): the source glyph, the id
  // and at most two labels, on the Overview the project; the title in at most
  // two lines; the lock line; the checkout chip and the PR chip with its CI
  // and the review word; at most two agents and +N. Only the operator's turn
  // is outlined in warning, the panel's card in primary; a done or (in
  // Dependencies) blocked card is dimmed.
  function taskCard(id, card) {
    const chips = card.branch || card.pr;
    return frame(id, card.title, {
      layout: 'vertical', gap: '$--spacing-xs', padding: '$--spacing-sm', width: column, fill: card.hover ? '$--accent' : '$--card', cornerRadius: '$--radius-md',
      stroke: card.needsYou ? '$--warning' : card.selected ? '$--primary' : '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
      ...(card.dim ? {opacity: dimmed} : {}),
    }, [
      frame(`${id}-idl`, 'Id line', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: inner, height: small}, [
        taskId(`${id}-id`, card.task),
        ...(card.labels ?? []).slice(0, 2).map((value, index) => label(`${id}-lb${index}`, value)),
        ...(card.project ? [caption(`${id}-proj`, card.project)] : []),
        spacer(`${id}-idsp`),
        ...(card.failure ? [icon(`${id}-warn`, 'triangle-alert', {size: 12, fill: '$--warning'})] : []),
        ...(card.word && !card.hover ? [caption(`${id}-word`, card.word)] : []),
        ...(card.hover ? [cardActions(`${id}-act`, card)] : []),
      ]),
      text(`${id}-title`, fitLines(card.title, inner, 14), {size: '$--text-title', weight: '600', width: inner}),
      ...(card.locked ? [frame(`${id}-lock`, 'Blocked by', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
        icon(`${id}-lock-g`, 'lock', {size: 12, fill: '$--warning'}),
        caption(`${id}-lock-t`, `먼저 끝나야 함: ${card.locked}`, '$--warning'),
      ])] : []),
      ...(chips ? [frame(`${id}-chips`, 'Chips', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: inner}, [
        ...(card.branch ? [checkoutChip(`${id}-co`, card, branchMax)] : []),
        ...(card.pr ? [prChip(`${id}-pr`, card.pr)] : []),
      ])] : []),
      ...(card.agents?.length ? [frame(`${id}-rows`, 'Agents', {layout: 'vertical', gap: 0, width: inner}, card.agents.slice(0, 2).map((agent, index) => agentRow(`${id}-a${index}`, agent, inner)))] : []),
      ...(card.more ? [caption(`${id}-more`, `+${card.more}`)] : []),
    ]);
  }

  // The preview a half-second rest on the id opens (IssuePreviewBody, B8):
  // id, labels, state, title, the body's first three lines, author, date and comments.
  function preview(id, card, {body, byline}) {
    const width = num(tokens, '--size-pr-popover');
    const room = width - 2 * num(tokens, '--spacing-md');
    return frame(id, 'Issue preview', {layout: 'vertical', gap: '$--spacing-xs', padding: '$--spacing-md', width, fill: '$--popover', cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      frame(`${id}-head`, 'Head', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: room}, [
        taskId(`${id}-id`, card.task),
        ...(card.labels ?? []).map((value, index) => label(`${id}-lb${index}`, value)),
        spacer(`${id}-sp`),
        caption(`${id}-state`, 'Open', '$--success'),
      ]),
      text(`${id}-title`, fitLines(card.title, room, 12), {size: '$--text-body', weight: '600', width: room}),
      ...body.map((line, index) => caption(`${id}-b${index}`, fitText(line, room, 11), '$--subtle-foreground')),
      caption(`${id}-by`, byline),
    ]);
  }

  // A column's head (ColumnHead): the stage and its count; Backlog's carries + for a new issue.
  function columnHead(id, label, count, action) {
    return frame(`${id}-head`, 'Head', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: column, height: num(tokens, '--size-control'), padding: [0, '$--spacing-xs']}, [
      text(`${id}-label`, label, {size: '$--text-subhead', weight: '600'}),
      text(`${id}-count`, String(count), {size: '$--text-subhead', fill: '$--muted-foreground'}),
      ...(action ? [spacer(`${id}-hsp`), action] : []),
    ]);
  }

  // One line at a column's foot (FoldLine): `+N`, or the work with no issue folded.
  function foldLine(id, label) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: column, padding: ['$--spacing-xxs', '$--spacing-sm'], cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      caption(`${id}-t`, label), spacer(`${id}-sp`), icon(`${id}-g`, 'chevron-right', {size: 14, fill: '$--muted-foreground'}),
    ]);
  }

  function stageColumn(id, label, count, cards, {newIssue = false, foot = []} = {}) {
    return frame(id, label, {layout: 'vertical', gap: '$--spacing-sm', width: column}, [
      columnHead(id, label, count, newIssue ? screenIconButton(`${id}-new`, 'plus', {size: small}) : null),
      ...cards,
      ...foot,
    ]);
  }

  // Done starts folded (DoneColumn, B3): its head is the toggle, then one line
  // per issue, its glyph, id, title and the pull request that closed it; on
  // the Overview one line per project with its count.
  function doneColumn(id, count, lines, more) {
    return frame(id, '완료', {layout: 'vertical', gap: '$--spacing-sm', width: column}, [
      frame(`${id}-head`, 'Head', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: column, height: num(tokens, '--size-control'), padding: [0, '$--spacing-xs']}, [
        text(`${id}-label`, '완료', {size: '$--text-subhead', weight: '600'}),
        text(`${id}-count`, String(count), {size: '$--text-subhead', fill: '$--muted-foreground'}),
        icon(`${id}-g`, 'chevron-right', {size: 14, fill: '$--muted-foreground'}),
      ]),
      frame(`${id}-names`, 'Folded', {layout: 'vertical', gap: 0, width: column}, [
        ...lines.map((line, index) => frame(`${id}-n${index}`, line.title ?? line.name, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', padding: ['$--spacing-xxs', '$--spacing-xs'], width: column}, line.task ? [
          icon(`${id}-n${index}-g`, line.task.source === 'local' ? 'file-text' : 'circle-check', {size: 12, fill: '$--pr-merged'}),
          caption(`${id}-n${index}-id`, line.task.label, '$--subtle-foreground', true),
          caption(`${id}-n${index}-t`, fitText(line.title, column - 120, 11), '$--subtle-foreground'),
          spacer(`${id}-n${index}-sp`),
          ...(line.pr ? [icon(`${id}-n${index}-pg`, 'git-merge', {size: 12, fill: '$--muted-foreground'}), caption(`${id}-n${index}-p`, String(line.pr), '$--muted-foreground', true)] : []),
        ] : [
          icon(`${id}-n${index}-g`, 'git-merge', {size: 12, fill: '$--pr-merged'}),
          caption(`${id}-n${index}-t`, line.name, '$--subtle-foreground', true),
          ...(line.count ? [caption(`${id}-n${index}-c`, `· ${line.count}`, '$--muted-foreground', true)] : []),
        ])),
        ...(more ? [frame(`${id}-more`, 'More', {padding: ['$--spacing-xxs', '$--spacing-xs']}, [caption(`${id}-more-t`, `+${more}`)])] : []),
      ]),
    ]);
  }

  // One Dependencies arrow between two cards of a row: the blocker on the
  // left, the task it blocks on the right (D-09).
  function arrow(id) {
    return frame(id, 'Blocks', {layout: 'horizontal', alignItems: 'center', width: num(tokens, '--home-dependency-gap')}, [
      {type: 'line', id: `${id}-l`, name: 'Line', width: 'fill_container', height: 0, stroke: '$--warning', strokeWidth: '$--size-hairline', strokeAlignment: 'center'},
      icon(`${id}-g`, 'chevron-right', {size: 12, fill: '$--warning'}),
    ]);
  }
  function legend(id) {
    return frame(id, 'Legend', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
      icon(`${id}-g`, 'arrow-right', {size: 14, fill: '$--warning'}),
      caption(`${id}-t`, '선행 · 왼쪽 태스크가 끝나야 화살표가 향하는 태스크를 시작할 수 있음'),
    ]);
  }
  function chain(id, parts) {
    return frame(id, 'Chain', {layout: 'horizontal', alignItems: 'center'}, parts);
  }

  // The issue panel beside the board (IssuePanel.tsx, B11-B19): the head,
  // the title, the action line, the properties a source has, what was done
  // for the issue, the body in Markdown and a GitHub issue's latest three
  // comments. A Local issue can be drawn with its editor open in place.
  function issuePanel(id, panel) {
    const width = num(tokens, '--issue-panel-min-width') + 80;
    const room = width - 2 * num(tokens, '--spacing-lg');
    const local = panel.task.source === 'local';
    const section = (sid, heading, children) => frame(sid, heading, {layout: 'vertical', gap: '$--spacing-xs', width: room}, [
      caption(`${sid}-h`, heading), ...children,
    ]);
    const property = (pid, name, value, fill = '$--foreground') => frame(pid, name, {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'center', width: room}, [
      text(`${pid}-k`, name, {size: '$--text-body', fill: '$--muted-foreground', width: 40}),
      typeof value === 'string' ? text(`${pid}-v`, value, {size: '$--text-body', fill}) : value,
    ]);
    const first = panel.stage === '백로그'
      ? [screenButton(`${id}-start`, '시작', {height: small, icon: 'play'}), kbd(`${id}-sk`, 'S')]
      : [screenButton(`${id}-ws`, 'Workspace', {height: small, icon: 'square-terminal'}), kbd(`${id}-ok`, 'O')];
    const editing = panel.editing;
    return frame(id, 'Issue panel', {layout: 'vertical', gap: '$--spacing-md', padding: '$--spacing-lg', width, fill: '$--card', cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      frame(`${id}-head`, 'Head', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: room}, [
        taskId(`${id}-id`, panel.task),
        caption(`${id}-src`, local ? 'Local' : 'GitHub'),
        spacer(`${id}-hsp`),
        caption(`${id}-state`, 'Open', '$--success'),
        screenIconButton(`${id}-close`, 'x', {size: small}),
      ]),
      ...(editing ? [
        frame(`${id}-editor`, 'Editor', {layout: 'vertical', gap: '$--spacing-sm', width: room}, [
          screenInput(`${id}-ed-title`, {content: editing.title, width: room}),
          frame(`${id}-ed-body`, 'Body', {layout: 'vertical', gap: '$--spacing-xxs', padding: '$--spacing-sm', width: room, height: 140, cornerRadius: '$--radius-sm', stroke: '$--input', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'},
            editing.body.map((line, index) => text(`${id}-ed-b${index}`, line, {size: '$--text-body'}))),
          ...(editing.failure ? [caption(`${id}-ed-fail`, editing.failure, '$--destructive')] : []),
          frame(`${id}-ed-act`, 'Save', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
            screenButton(`${id}-ed-save`, '저장', {height: small}), kbd(`${id}-ed-sk`, '⌘↵'),
            screenButton(`${id}-ed-cancel`, '취소', {variant: 'ghost', height: small}), kbd(`${id}-ed-ck`, 'Esc'),
          ]),
        ]),
      ] : [
        text(`${id}-title`, fitLines(panel.title, room, 18, 3), {size: '$--text-headline', weight: '600', width: room}),
        frame(`${id}-actions`, 'Actions', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: room}, [
          ...first,
          screenIconButton(`${id}-side`, local ? 'pencil' : 'external-link', {size: small}),
          spacer(`${id}-asp`),
          screenIconButton(`${id}-menu`, 'ellipsis', {size: small}),
        ]),
      ]),
      frame(`${id}-props`, 'Properties', {layout: 'vertical', gap: '$--spacing-xs', width: room}, [
        property(`${id}-p-stage`, '단계', panel.stage),
        ...(panel.labels ? [property(`${id}-p-labels`, '라벨', frame(`${id}-p-lv`, 'Labels', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, panel.labels.map((value, index) => label(`${id}-p-l${index}`, value))))] : []),
        ...(panel.author ? [property(`${id}-p-author`, '작성', panel.author)] : []),
        ...(panel.assignees ? [property(`${id}-p-assign`, '담당', panel.assignees)] : []),
        ...(panel.created ? [property(`${id}-p-created`, '만듦', panel.created)] : []),
        property(`${id}-p-updated`, '갱신', panel.updated),
        ...(panel.blocked ? [property(`${id}-p-blocked`, '막힘', panel.blocked, '$--warning')] : []),
      ]),
      ...(panel.work ? [section(`${id}-work`, '이 이슈로 한 일', [
        frame(`${id}-w-co`, 'Checkout', {layout: 'horizontal', alignItems: 'center', width: room}, [
          checkoutChip(`${id}-w-chip`, panel.work),
          spacer(`${id}-w-sp`),
          screenIconButton(`${id}-w-ws`, 'square-terminal', {size: small}),
        ]),
        ...panel.work.agents.map((agent, index) => screenSidebarAgentRow(`${id}-w-a${index}`, {
          title: fitText(agent.title, room - 100, 12), symbol: AGENT_MARK[agent.mark][0], color: AGENT_MARK[agent.mark][1], provider: agent.provider ?? 'claude', age: agent.age,
          line: agent.line ?? null, lineFill: agent.tone === 'request' ? '$--warning' : '$--foreground', bright: ATTENTION.has(agent.mark), inset: agent.depth ? 16 : 0, width: room, fold: 'none',
        })),
        ...(panel.work.pr ? [frame(`${id}-w-pr`, 'Pull request', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: room}, [
          prChip(`${id}-w-prc`, {...panel.work.pr, review: undefined}),
          caption(`${id}-w-prt`, fitText(panel.work.pr.title, room - 160, 11), '$--foreground'),
          spacer(`${id}-w-prsp`),
          ...(panel.work.pr.review ? [caption(`${id}-w-prr`, REVIEW_WORD[panel.work.pr.review][0], REVIEW_WORD[panel.work.pr.review][1])] : []),
        ])] : []),
      ])] : []),
      ...(editing ? [] : [section(`${id}-body`, local ? '본문 · 누르면 고침' : '본문', panel.reading
        ? [0, 1, 2].map((index) => frame(`${id}-sk${index}`, 'Skeleton', {width: Math.round(room * (1 - index * 0.2)), height: num(tokens, '--size-icon'), cornerRadius: '$--radius-xs', fill: '$--muted'}, []))
        : panel.failure
          ? [frame(`${id}-fail`, 'Failed read', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: room}, [
              caption(`${id}-fail-t`, panel.failure), screenButton(`${id}-retry`, '재시도', {variant: 'secondary', height: small}),
            ])]
          : panel.body.map(([kind, line], index) => text(`${id}-b${index}`, line, kind === 'h' ? {size: '$--text-title', weight: '600', fill: '$--primary'} : {size: '$--text-body', width: room})))]),
      ...(local || editing ? [] : [section(`${id}-comments`, `댓글 ${panel.comments.length}`, [
        ...panel.comments.map(([who, body], index) => frame(`${id}-c${index}`, who, {layout: 'vertical', gap: '$--spacing-xxs', width: room}, [
          caption(`${id}-c${index}-by`, who), text(`${id}-c${index}-t`, body, {size: '$--text-body', width: room}),
        ])),
        caption(`${id}-cw`, panel.comments.length === 0 ? '댓글 없음 · 쓰기는 GitHub에서' : '쓰기는 GitHub에서'),
      ])]),
    ]);
  }

  return {column, taskCard, stageColumn, doneColumn, foldLine, arrow, legend, chain, preview, issuePanel, label, taskId, checkoutChip, prChip, agentRow, kbd, caption, spacer};
}

// -- the Agents lens (OverviewLenses.tsx over overviewLens.ts) -----------------

// The checkout lanes and the lineage board (PRD overview-lenses-tiles-agents
// D-05, D-06, B13-B25), authored on local tokens like the task card, since no
// library master draws a lane or a node. A lane is a fixed height here so a
// delegation line runs through the cells it crosses: the parent's cell draws
// its node and the line down from it, a lane in between a line through the
// column its line keeps free, the child's cell the line's end and its node.
function lensParts(tokens) {
  const nodeWidth = num(tokens, '--lens-node-width');
  const headWidth = num(tokens, '--lens-lane-head');
  const gap = num(tokens, '--spacing-xl');
  const laneHeight = 84;
  const top = num(tokens, '--spacing-sm');
  const dimmed = num(tokens, '--opacity-secondary');
  const nodeInner = nodeWidth - 2 * num(tokens, '--spacing-sm');
  const caption = (id, content, fill = '$--muted-foreground', mono = false) => text(id, content, {size: '$--text-caption', fill, mono});
  const spacer = (id, height) => frame(id, 'Spacer', {width: 1, height}, []);
  const vertical = (id, height = 'fill_container') => ({type: 'line', id, name: 'Line', width: 0, height, stroke: '$--muted-foreground', strokeWidth: '$--size-hairline', strokeAlignment: 'center'});

  // A node (AgentNode, B21): the mark, provider, title and age, then the
  // core's line - the question in warning with a warning outline on the
  // operator's turn, a result after ✓, the children's summary, the progress
  // - and nothing, dimmed, for a resting agent; a lineage node adds its chips.
  function node(id, agent, {chips} = {}) {
    const [symbol, color] = AGENT_MARK[agent.mark];
    const turn = agent.mark === 'ask' || agent.mark === 'done';
    const rest = agent.mark === 'seen' && !agent.line;
    return frame(id, agent.title, {
      layout: 'vertical', gap: '$--spacing-xxs', padding: ['$--spacing-xs', '$--spacing-sm'], width: nodeWidth, fill: '$--card', cornerRadius: '$--radius-md',
      stroke: turn ? '$--warning' : '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', ...(rest ? {opacity: dimmed} : {}),
    }, [
      frame(`${id}-l1`, 'Line 1', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: nodeInner}, [
        screenStatusMark(tokens, `${id}-m`, symbol, color),
        frame(`${id}-p`, 'Provider artwork', {width: 16, height: 16, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${agent.provider ?? 'claude'}.png`, mode: 'fit'}}, []),
        text(`${id}-t`, fitText(agent.title, nodeInner - 80, 13), {size: '$--text-body', weight: turn ? '600' : '400'}),
        frame(`${id}-sp`, 'Spacer', {width: 'fill_container', height: 1}, []),
        caption(`${id}-age`, agent.age, '$--muted-foreground', true),
      ]),
      ...(agent.line ? [caption(`${id}-line`, fitText(agent.line, nodeInner, 11), agent.mark === 'ask' ? '$--warning' : agent.mark === 'done' ? '$--foreground' : '$--muted-foreground')] : []),
      ...(chips ? [frame(`${id}-chips`, 'Chips', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, chips)] : []),
    ]);
  }

  // A lane cell: a node, the line through a lane between parent and child,
  // or nothing; `down` draws the line on from a parent, `from` ends one at a child.
  function cell(id, {agent, down = false, from = false, through = false}) {
    const parts = through ? [vertical(`${id}-v`)]
      : agent ? [
          ...(from ? [vertical(`${id}-in`, top - 6), icon(`${id}-head`, 'chevron-down', {size: 8, fill: '$--muted-foreground'})] : [spacer(`${id}-top`, top)]),
          node(`${id}-n`, agent),
          ...(down ? [vertical(`${id}-out`)] : []),
        ]
        : [];
    return frame(id, 'Cell', {layout: 'vertical', alignItems: 'center', width: nodeWidth, height: laneHeight}, parts);
  }

  // A right arrow between two nodes of one lane or one lineage (B14, B23).
  function across(id, drawn) {
    return frame(id, drawn ? 'Delegates' : 'Gap', {layout: 'horizontal', alignItems: 'center', width: gap, height: drawn === 'lineage' ? 'fit_content' : laneHeight, padding: drawn === 'lineage' ? 0 : [top + 12, 0, 0, 0]}, drawn ? [
      {type: 'line', id: `${id}-l`, name: 'Line', width: 'fill_container', height: 0, stroke: '$--muted-foreground', strokeWidth: '$--size-hairline', strokeAlignment: 'center'},
      icon(`${id}-g`, 'chevron-right', {size: 10, fill: '$--muted-foreground'}),
    ] : []);
  }

  // A lane head (LaneHead, B15, B18): the glyph in its PR's colour and the
  // branch, the purpose, then the issue chip, PR chip, ↑N ↓N and the files;
  // main's head the house, main, the purpose and its agent count; a merged
  // lane dimmed with the merge glyph and 정리.
  function head(id, lane) {
    const inner = headWidth - 2 * num(tokens, '--spacing-sm');
    const glyph = lane.primary ? 'house' : lane.cleanup ? 'git-merge' : lane.pr ? 'git-pull-request' : 'git-branch';
    const tone = lane.cleanup ? '$--pr-merged' : lane.pr ? PR_TONE[lane.pr.tone ?? 'open'] : '$--muted-foreground';
    const facts = lane.primary ? [caption(`${id}-agents`, `에이전트 ${lane.agents}`, '$--muted-foreground', true)] : [
      ...(lane.task ? [frame(`${id}-task`, 'Issue chip', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
        icon(`${id}-task-g`, 'circle-dot', {size: 12, fill: '$--muted-foreground'}), caption(`${id}-task-t`, lane.task, '$--muted-foreground', true),
      ])] : []),
      ...(lane.pr ? [themedXref(`${id}-pr`, 'eHAjc', `#${lane.pr.number}`, BADGE_VARIANTS.outline.overrides, {xXuNa: {enabled: true, icon: 'git-pull-request', fill: PR_TONE[lane.pr.tone ?? 'open']}, n8L5dm: {content: `#${lane.pr.number}`, fill: PR_TONE[lane.pr.tone ?? 'open']}})] : []),
      ...(lane.distance ? [caption(`${id}-dist`, lane.distance, '$--muted-foreground', true)] : []),
      ...(lane.files ? [caption(`${id}-files`, `${lane.files} files`, '$--warning', true)] : []),
    ];
    return frame(id, 'Lane head', {
      layout: 'vertical', gap: '$--spacing-xxs', padding: '$--spacing-sm', width: headWidth, height: laneHeight, ...(lane.cleanup ? {opacity: dimmed} : {}),
    }, [
      ...(lane.project ? [text(`${id}-proj`, lane.project, {size: '$--text-micro', fill: '$--muted-foreground'})] : []),
      frame(`${id}-l1`, 'Branch', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: inner}, [
        icon(`${id}-g`, glyph, {size: 14, fill: tone}),
        text(`${id}-b`, fitText(lane.branch, inner - 60, 13, true), {size: '$--text-body', mono: true}),
        frame(`${id}-sp`, 'Spacer', {width: 'fill_container', height: 1}, []),
        ...(lane.cleanup ? [caption(`${id}-clean`, '정리', '$--subtle-foreground')] : []),
      ]),
      ...(lane.purpose ? [caption(`${id}-purpose`, fitText(lane.purpose, inner, 11))] : []),
      frame(`${id}-l3`, 'Facts', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, facts),
    ]);
  }

  // One lane (LaneRow): the head, a rule, then the cells and the gaps between them.
  function lane(id, value, columns, {selected = false} = {}) {
    const cells = [];
    for (let index = 0; index < columns; index += 1) {
      if (index > 0) cells.push(across(`${id}-g${index}`, value.arrows?.includes(index) ? 'lane' : null));
      cells.push(cell(`${id}-c${index}`, value.cells[index] ?? {}));
    }
    return frame(id, value.branch, {
      layout: 'horizontal', alignItems: 'start', height: laneHeight, cornerRadius: '$--radius-sm',
      ...(selected ? {stroke: '$--primary', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'} : {}),
    }, [
      head(`${id}-head`, value),
      frame(`${id}-rule`, 'Rule', {width: 1, height: laneHeight, fill: '$--border'}, []),
      frame(`${id}-cells`, 'Agents', {layout: 'horizontal', alignItems: 'start', padding: [0, '$--spacing-sm']}, cells),
    ]);
  }

  // A folded line (FoldLine, B20, B25): its words and count, and the chevron.
  function fold(id, label, width, open = false) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width, padding: ['$--spacing-xs', '$--spacing-sm'], cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      caption(`${id}-t`, label, '$--subtle-foreground'),
      frame(`${id}-sp`, 'Spacer', {width: 'fill_container', height: 1}, []),
      icon(`${id}-g`, open ? 'chevron-down' : 'chevron-right', {size: 14, fill: '$--muted-foreground'}),
    ]);
  }

  // The checkout mode (CheckoutLanes): the column heads, the lanes separated
  // by rules, then the two folded lines.
  function lanes(id, rows, {columns, width, selected, folds}) {
    const rule = key => frame(`${id}-r${key}`, 'Rule', {width, height: 1, fill: '$--border'}, []);
    return frame(id, 'Checkout lanes', {layout: 'vertical', gap: 0, width}, [
      frame(`${id}-cols`, 'Column heads', {layout: 'horizontal', width}, [
        frame(`${id}-ch1`, '체크아웃', {width: headWidth + 1, padding: ['$--spacing-xs', '$--spacing-sm']}, [caption(`${id}-ch1-t`, '체크아웃')]),
        frame(`${id}-ch2`, '에이전트', {padding: ['$--spacing-xs', '$--spacing-sm']}, [caption(`${id}-ch2-t`, '에이전트')]),
      ]),
      ...rows.flatMap((row, index) => [lane(`${id}-l${index}`, row, columns, {selected: index === selected}), rule(index)]),
      frame(`${id}-folds`, 'Folds', {layout: 'vertical', gap: '$--spacing-sm', width, padding: ['$--spacing-sm', 0, 0, 0]}, folds.map((label, index) => fold(`${id}-f${index}`, label, width))),
    ]);
  }

  // The lineage mode (LineageLens): the column names, then a row per lineage,
  // each node beside its parent with an arrow, its chips on a third line.
  function lineage(id, rows, {width, names, folds}) {
    return frame(id, 'Lineage', {layout: 'vertical', gap: '$--spacing-md', width}, [
      frame(`${id}-cols`, 'Column heads', {layout: 'horizontal', gap}, names.map((name, index) => frame(`${id}-ch${index}`, name, {width: nodeWidth, padding: ['$--spacing-xs', 0]}, [caption(`${id}-ch${index}-t`, name)]))),
      ...rows.map((row, index) => frame(`${id}-row${index}`, `Lineage ${index + 1}`, {layout: 'horizontal', alignItems: 'center'}, row.flatMap((agent, depth) => [
        ...(depth > 0 ? [across(`${id}-row${index}-a${depth}`, 'lineage')] : []),
        node(`${id}-row${index}-n${depth}`, agent, {chips: agent.chips}),
      ]))),
      frame(`${id}-folds`, 'Folds', {layout: 'vertical', gap: '$--spacing-sm', width}, folds.map((label, index) => fold(`${id}-f${index}`, label, width))),
    ]);
  }

  // A lineage node's chips (B24): the checkout, the issue and the PR.
  function chips(id, {branch, primary = false, task, pr}) {
    return [
      frame(`${id}-co`, 'Checkout chip', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
        icon(`${id}-co-g`, primary ? 'house' : 'git-branch', {size: 12, fill: '$--muted-foreground'}), caption(`${id}-co-t`, fitText(branch, 110, 11, true), '$--muted-foreground', true),
      ]),
      ...(task ? [frame(`${id}-task`, 'Issue chip', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
        icon(`${id}-task-g`, 'circle-dot', {size: 12, fill: '$--muted-foreground'}), caption(`${id}-task-t`, task, '$--muted-foreground', true),
      ])] : []),
      ...(pr ? [themedXref(`${id}-pr`, 'eHAjc', `#${pr}`, BADGE_VARIANTS.outline.overrides, {xXuNa: {enabled: true, icon: 'git-pull-request', fill: '$--pr-open'}, n8L5dm: {content: `#${pr}`, fill: '$--pr-open'}})] : []),
    ];
  }

  return {lanes, lineage, chips, nodeWidth, headWidth, gap};
}

const gh = number => ({source: 'github', label: `#${number}`});
const local = number => ({source: 'local', label: `L-${number}`});

// The cards and agents of herdr-ide, the project both boards draw.
const BUG = ['bug', '$--destructive'];
const ENHANCEMENT = ['enhancement', '$--agent-working'];
const ISSUE_192 = {task: gh(192), labels: [BUG], title: 'hided has no SIGTERM handler: AI children die with the stdin pipe', branch: '192-hided-sigterm-handler', ahead: 3, files: 4, needsYou: true, agents: [
  {mark: 'ask', provider: 'codex', title: 'SIGTERM 처리와 자식 정리 순서', line: '기존 stdin 종료 경로도 남길까요?', tone: 'request', age: '4m'},
  {mark: 'work', title: '종료 경로 테스트 작성', age: '2m'},
]};
const ISSUE_186 = {task: gh(186), title: 'Set the Projects sidebar’s type ladder, row heights and width', branch: '186-sidebar-typography', pr: {number: 208, checks: 'passing', review: 'review_required'}, needsYou: true, agents: [
  {mark: 'done', title: 'observer-sidebar-typography', line: 'Implementor가 PR #208 준비 완료', tone: 'news', age: '1m'},
  {mark: 'seen', title: '사이드바 타이포그래피 구현', age: '26m'},
]};
// A Local issue an agent works on in main, on the project's board.
const LOCAL_3 = {task: local(3), title: 'Overview 진입 흐름', branch: 'main', primary: true, agents: [
  {mark: 'work', title: 'Overview 진입 흐름 디자인', line: 'Pen 보드 작성 중', age: '1m'},
]};
// A Local issue an agent works on in a project's primary checkout, on the Overview of every project.
const LOCAL_5 = {task: local(5), project: 'creator', title: '소프트웨어 팩토리 경험담 글쓰기', branch: 'main', primary: true, needsYou: true, agents: [
  {mark: 'done', title: '경험담 초안 윤문', line: 'AI 티 윤문 완료, 빠진 문장 3곳 확인 대기', tone: 'news', age: '3m'},
]};

const MAIN_SPEC = 'web/src/App.tsx, sidebar.tsx, MainScreen.tsx, TaskBoards.tsx, projectBoard.ts: Overview, the scope the sidebar’s global Overview row opens and marks. Its title carries Add project and 새 이슈 as the primary action (C); its facts line the project count, the open issues once every source has answered, and the open-PR and merged totals only when every project can give its part; its Tasks · Agents · Projects tabs the count of agents waiting on the operator on Agents, whose view is the Project Overview’s checkout lanes or lineage over every project with the project’s name above each lane head. Every project’s issues share one board, 백로그 · 진행 중 · 리뷰 · 완료, each card an issue with its project beside its id (a Local issue as L-N): 시작 on a backlog card under the pointer, the operator’s-turn cards in the warning border, the worktrees with no issue folded into one line at the foot of 진행 중, and 완료 folded to one line per project with its count. Every project has an issue source, so no project is set apart as unconnected. Its Dependencies mode draws an arrow that crosses projects, the blocker named with its repository on the lock line. The Projects view is the project list grouped by device.';

const OVERVIEW_SPEC = 'web/src/ProjectOverview.tsx, OverviewLenses.tsx, overviewLens.ts, TaskBoards.tsx, projectBoard.ts: a project’s Overview (PRD overview-lenses-tiles-agents). The header carries the path back, New agent as the quiet action and 새 이슈 as the primary one (C), the facts line of worktrees, disk, main behind and N merged → 정리 with the chosen view’s mode control at its right end, then the tiles Agents · Issues · PRs · Sessions where the tab row was: the name, the yellow badge of the operator’s turn, the large number and its unit, one bar; the chosen tile outlined. Every entry opens Agents › 체크아웃 with the front checkout’s lane outlined. A lane is a checkout: its head (the glyph in its PR’s colour and the branch, the purpose, the issue chip, PR chip, ↑N ↓N and the files in warning; main the house and 에이전트 N; a merged worktree dimmed with 정리) and its agents to the right. main is pinned on top, then the operator’s turn, working, resting. A delegation runs down across lanes in its Observer’s column or right within a lane, and no line crosses a node. A node reads mark, provider, title and age, then the core’s line; only the operator’s turn is yellow, a resting node is dimmed with no line. The worktrees with no agent and the ones to clean up fold into one line each. Beside it the lineage mode: Observer · Implementor · 하위 에이전트 columns, a row per lineage with the asking one first, each node’s chips (checkout, issue, PR), resting lineages folded. The Issues tile opens a board of issues only (PRD overview-lenses-issues): a card is the glyph, id and at most two labels, the title, the lock line, the checkout chip and the PR chip with its CI and review word, and at most two agents; its buttons fill the id line’s slot under the pointer (시작 S, Workspace O, the PR icon, a Local issue’s edit, ⋯); only the operator’s turn is outlined in warning. The worktrees and pull requests with no issue are one line each under 진행 중 and 리뷰, and 완료 is folded to one line per issue with the pull request that closed it. The facts line’s right end carries the filter and Board · List · Dependencies. A card opens the issue panel beside the board: the head (glyph, id, source, Open, ×), the title, the action line, the properties, 이 이슈로 한 일, the Markdown body and a GitHub issue’s latest comments; a Local issue edits in place, and a failed read is one line with 재시도. The PRs tile opens the project’s pull requests grouped 내 차례, 에이전트가 고치는 중, CI 실패 · 맡은 에이전트 없음 and 최근 머지 (folded) (PRD overview-lenses-prs): a row is ▸, the state glyph, the number, the title, the issue cell (a dotted circle when empty, the 이슈 잇기 icon under the pointer), 확인, the agents’ marks, the branch, CI, the review word and the time, whose fixed slot holds GitHub and ⋯, ▷ 맡기기 or 정리 under the pointer; an unfolded row shows the branch’s agents and GitHub, Workspace and 이슈 잇기. 이슈 잇기 on a GitHub issue asks once, 그만두기 first, before it writes Closes #N into the body.';

// -- Screen / Main ------------------------------------------------------------

// The Overview of every project (MainScreen.tsx, PRD task-agents-views D-10,
// titled Overview by PRD sidebar-shell D-02, reworked issue first): the
// sidebar's Overview row marked, the title with Add project and 새 이슈, the
// facts line, the Tasks · Agents · Projects tabs, every project's issues and
// worktrees on one board with the project beside each id, Done folded per
// project, and the Dependencies mode with an arrow that crosses projects.
function buildMain(tokens) {
  const {column, taskCard, stageColumn, doneColumn, foldLine, arrow, legend, chain} = issueBoardParts(tokens);
  const width = 4 * column + 3 * num(tokens, '--spacing-md');
  // `key` tells the Board's header from the Dependencies one, in each theme.
  function header(key, mode) {
    return frame(`main-hdr-${key}`, 'Header', {layout: 'vertical', gap: '$--spacing-sm', width}, [
      frame(`main-titlerow-${key}`, 'Title row', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'center', width}, [
        text(`main-title-${key}`, 'Overview', {size: '$--text-headline', weight: '600'}),
        frame(`main-titlegap-${key}`, 'Spacer', {width: 'fill_container', height: 1}, []),
        screenButton(`main-addproj-${key}`, 'Add project  ⇧⌘N', {variant: 'ghost', height: num(tokens, '--size-control'), icon: 'plus'}),
        newIssueButton(tokens, `main-issue-${key}`),
      ]),
      factsLine(`main-facts-${key}`, [
        {glyph: 'folder', label: '5 projects'},
        {glyph: 'circle-dot', label: '8 open issues'},
        {glyph: 'git-pull-request', label: '2 open PRs'},
        {glyph: 'git-merge', label: '19 merged', fill: '$--pr-merged'},
      ]),
      frame(`main-rule-${key}`, 'Rule', {width, height: 1, fill: '$--border'}, []),
      viewRow(`main-row-${key}`, viewTabs(`main-tabs-${key}`, ['Tasks', 'Agents', 'Projects'], 0, 3), mode, width),
    ]);
  }
  function build(suffix) {
    const sidebar = screenSidebar(tokens, 'main-sidebar', suffix, [
      {title: '카드 상태 시트 설계', status: 'Waiting', symbol: '○', badge: '●1', fold: 'folded', summaries: [
        {status: 'working', branch: 'web-view-overlay', pr: '#173'},
        {status: 'done', branch: 'web-side-panel', pr: '#170', more: 1},
      ]},
      {title: '조용한 순찰 기능 개발', status: 'Seen', symbol: '○', statusColor: '$--muted-foreground', fold: 'folded', summaries: [
        {status: 'done', branch: 'hcoord-decouple', device: 'mini'},
      ]},
    ], {overview: true});
    const card = (id, value) => taskCard(`main-${id}-${suffix}`, value);
    const board = frame(`main-list-${suffix}`, 'Overview · Tasks › Board', {width, layout: 'vertical', gap: '$--spacing-md'}, [
      header(suffix, 'board'),
      frame(`main-cols-${suffix}`, 'Columns', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [
        stageColumn(`main-backlog-${suffix}`, '백로그', 4, [
          card('b1', {task: gh(201), project: 'herdr-ide', title: 'Add Workspace design reference and visual review coverage', hover: 'start'}),
          card('b2', {task: gh(14), project: 'sasu', title: 'judge backend switch for claude and codex'}),
          card('b3', {task: local(7), project: 'creator', title: 'AI 개발자 릴스 5탄 기획'}),
          card('b4', {task: gh(199), project: 'herdr-ide', title: 'Read the remote primary checkout over device connections'}),
        ], {newIssue: true}),
        stageColumn(`main-working-${suffix}`, '진행 중', 2, [
          card('w1', LOCAL_5),
          card('w2', {...ISSUE_192, project: 'herdr-ide'}),
        ], {foot: [foldLine(`main-loose-${suffix}`, '이슈 없는 워크트리 5')]}),
        stageColumn(`main-review-${suffix}`, '리뷰', 2, [
          card('r1', {...ISSUE_186, project: 'herdr-ide'}),
          card('r2', {task: gh(9), project: 'sasu', title: 'hcoord-decouple: move lineage tokens into the plugin', branch: '9-hcoord-decouple', pr: {number: 12, checks: 'pending', review: 'review_required'}}),
        ]),
        doneColumn(`main-done-${suffix}`, 19, [{name: 'herdr-ide', count: 12}, {name: 'sasu', count: 6}, {name: 'creator', count: 1}]),
      ]),
    ]);
    // A blocker in another project is an arrow too; its lock line names it with its repository.
    const dependencies = frame(`main-deps-${suffix}`, 'Overview · Tasks › Dependencies', {width, layout: 'vertical', gap: '$--spacing-lg'}, [
      header(`d${suffix}`, 'dependencies'),
      legend(`main-dlegend-${suffix}`),
      chain(`main-dchain-${suffix}`, [
        card('d1', {...ISSUE_192, project: 'herdr-ide', word: '진행 중'}),
        arrow(`main-da1-${suffix}`),
        card('d2', {task: gh(14), project: 'sasu', title: 'judge backend switch for claude and codex', word: '백로그', locked: 'modakbul-gongbang/hide#192', dim: true}),
      ]),
      text(`main-dunrel-${suffix}`, '관계 없는 태스크', {size: '$--text-subhead', weight: '600', fill: '$--subtle-foreground'}),
      frame(`main-dunrelrow-${suffix}`, 'Unrelated', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [
        card('d3', {...LOCAL_5, word: '진행 중'}),
        card('d4', {...ISSUE_186, project: 'herdr-ide', word: '리뷰'}),
      ]),
    ]);
    return [sidebar, frame(`main-views-${suffix}`, 'Views', {layout: 'vertical', gap: '$--spacing-xl'}, [screenLineageDetails(tokens, suffix), board, dependencies])];
  }
  return screenSheet('screen-main', 'Screen / Main', MAIN_SPEC, build, build);
}

// -- the PRs view (PullRequestsView.tsx over projectBoard.ts) --------------------

// A group and its rows (PRD overview-lenses-prs B2-B6), authored on local
// tokens like the task card, since no library master draws a pull request row.
// A row is ▸, the state glyph, the number, the title, the issue cell, `확인`,
// then the marks, the branch, CI, the review word and the fixed time slot,
// which holds the buttons under the pointer so nothing moves.
function prParts(tokens) {
  const {taskId, agentRow, caption, spacer} = issueBoardParts(tokens);
  const small = num(tokens, '--size-control-sm');
  const slot = num(tokens, '--size-pr-slot');
  const review = num(tokens, '--size-pr-review');
  const numberWidth = num(tokens, '--size-pr-number');
  const branchMax = num(tokens, '--size-pr-branch-max');
  const indent = num(tokens, '--size-pr-indent');
  const glyph = {open: 'git-pull-request', draft: 'git-pull-request-draft', merged: 'git-merge'};

  function prGroup(id, label, count, tone, rows, {folded = false} = {}) {
    return frame(id, label, {layout: 'vertical', gap: 0, width: 'fill_container'}, [
      frame(`${id}-head`, 'Head', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', height: num(tokens, '--size-control')}, [
        ...(folded ? [icon(`${id}-fold`, 'chevron-right', {size: num(tokens, '--size-icon'), fill: '$--muted-foreground'})] : []),
        text(`${id}-label`, label, {size: '$--text-subhead', weight: '600', fill: tone ?? '$--foreground'}),
        text(`${id}-count`, String(count), {size: '$--text-subhead', fill: '$--muted-foreground'}),
      ]),
      ...rows,
    ]);
  }

  function prRow(id, pr) {
    const tone = pr.tone ?? 'open';
    const issue = pr.issue ? taskId(`${id}-issue`, pr.issue)
      : pr.hover && pr.linkable ? icon(`${id}-issue`, 'link-2', {size: 12, fill: '$--foreground'})
        : icon(`${id}-issue`, 'circle-dashed', {size: 12, fill: '$--muted-foreground'});
    const marks = (pr.agents ?? []).slice(0, 3).map((agent, index) => screenStatusMark(tokens, `${id}-m${index}`, AGENT_MARK[agent.mark][0], AGENT_MARK[agent.mark][1]));
    const ci = ciMark(tokens, `${id}-ci`, pr.checks);
    const act = pr.hover === 'delegate' ? [screenButton(`${id}-take`, '맡기기', {variant: 'secondary', height: small, icon: 'play'})]
      : pr.hover === 'default' ? [screenIconButton(`${id}-gh`, 'external-link', {size: small}), screenIconButton(`${id}-more`, 'ellipsis', {size: small})]
        : [caption(`${id}-age`, pr.age, '$--muted-foreground', true)];
    const row = frame(id, `PR #${pr.number}`, {
      layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: 'fill_container', height: num(tokens, '--size-control-lg'),
      padding: [0, '$--spacing-sm', 0, '$--spacing-xs'], cornerRadius: '$--radius-sm',
      ...(pr.hover ? {fill: '$--accent'} : pr.open ? {fill: '$--secondary'} : {}),
    }, [
      icon(`${id}-fold`, pr.open ? 'chevron-down' : 'chevron-right', {size: 12, fill: '$--muted-foreground'}),
      icon(`${id}-g`, glyph[tone], {size: num(tokens, '--size-pr-icon'), fill: PR_TONE[tone]}),
      frame(`${id}-n`, 'Number', {layout: 'horizontal', width: numberWidth}, [caption(`${id}-nt`, `#${pr.number}`, '$--muted-foreground', true)]),
      text(`${id}-t`, pr.title, {size: '$--text-subhead'}),
      issue,
      // The yellow `확인` (D-48): the outline Badge in the warning tone, as the web draws it.
      ...(pr.look ? [frame(`${id}-look`, '확인', {layout: 'horizontal', padding: [0, '$--spacing-xs'], cornerRadius: '$--radius-sm', stroke: '$--warning', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [caption(`${id}-look-t`, '확인', '$--warning')])] : []),
      spacer(`${id}-sp`),
      ...(marks.length ? [frame(`${id}-marks`, 'Agents', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, marks)] : []),
      caption(`${id}-br`, fitText(pr.branch, branchMax, 11, true), '$--muted-foreground', true),
      frame(`${id}-cib`, 'Checks', {layout: 'horizontal', width: num(tokens, '--size-icon-sm')}, ci ? [ci] : []),
      frame(`${id}-rvb`, 'Review', {layout: 'horizontal', width: review, justifyContent: 'end'}, pr.review ? [caption(`${id}-rv`, REVIEW_WORD[pr.review][0], REVIEW_WORD[pr.review][1])] : []),
      frame(`${id}-slot`, 'Time or buttons', {layout: 'horizontal', gap: '$--spacing-xxs', width: slot, justifyContent: 'end', alignItems: 'center'}, act),
    ]);
    if (!pr.open) return [row];
    const width = 460;
    return [row, frame(`${id}-x`, 'Unfolded', {layout: 'vertical', gap: '$--spacing-xxs', padding: [0, 0, '$--spacing-sm', indent]}, [
      ...(pr.agents ?? []).map((agent, index) => agentRow(`${id}-xa${index}`, agent, width)),
      frame(`${id}-xb`, 'Buttons', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        screenIconButton(`${id}-xgh`, 'external-link', {size: small}),
        screenIconButton(`${id}-xws`, 'square-terminal', {size: small}),
        ...(pr.issue ? [] : [screenIconButton(`${id}-xln`, 'link-2', {size: small})]),
      ]),
    ])];
  }

  return {prGroup, prRow};
}

// -- Screen / Project Overview -------------------------------------------------

// The Overview (ProjectOverview.tsx, PRD overview-lenses-tiles-agents): the
// header with the tiles, Agents › 체크아웃 as every entry opens it (efs2), the
// lineage mode (efs6), and the Issues tile's board and Dependencies mode,
// #218's Tasks board under its new name.
const SIGTERM_OBSERVER = {mark: 'seen', title: 'SIGTERM 정리 오케스트레이션', line: '일하는 중 1 · 물음 1', age: '20m'};
const SIGTERM_ASK = {mark: 'ask', provider: 'codex', title: 'SIGTERM 처리와 자식 정리', line: '기존 stdin 종료 경로도 남길까요?', age: '4m'};
const SIGTERM_REVIEW = {mark: 'work', title: '리뷰: 종료 경로 회귀', line: '회귀 테스트 3개 중 2개 통과', age: '2m'};
const OVERVIEW_WORK = {mark: 'work', title: 'Overview 진입 흐름', line: 'Pen 보드 작성 중', age: '1m'};
const TAB_GROUPS_OBSERVER = {mark: 'done', title: 'agent-tab-groups', line: 'Implementor 끝남 · CI 통과, 머지 대기', age: '8m'};
const TAB_GROUPS_IMPL = {mark: 'seen', provider: 'codex', title: 'Agent tab groups 구현', age: '8m'};
const CODEX_REST = {mark: 'seen', title: '코덱스 구현 및 PR 머지', age: '2h'};

function buildProjectOverview(tokens) {
  const {column, taskCard, stageColumn, doneColumn, foldLine, arrow, legend, chain, preview, issuePanel} = issueBoardParts(tokens);
  const {prGroup, prRow} = prParts(tokens);
  const {lanes, lineage, chips, nodeWidth, headWidth, gap} = lensParts(tokens);
  const columns = 4;
  const width = headWidth + 1 + 2 * num(tokens, '--spacing-sm') + columns * nodeWidth + (columns - 1) * gap;
  const boardWidth = 4 * column + 3 * num(tokens, '--spacing-md');
  function build(suffix) {
    const card = (id, value) => taskCard(`ov-${id}-${suffix}`, value);
    const checkouts = frame(`ov-agents-${suffix}`, 'Project Overview · Agents › 체크아웃', {layout: 'vertical', gap: '$--spacing-md', width}, [
      overviewHeader(tokens, 'ov-ahead', suffix, {project: 'herdr-ide', facts: HERDR_FACTS, view: 'agents', mode: 'checkouts', width}),
      lanes(`ov-lanes-${suffix}`, [
        {primary: true, branch: 'main', purpose: 'Observer · 계획과 위임', agents: 4, cells: [
          {agent: SIGTERM_OBSERVER, down: true}, {agent: OVERVIEW_WORK}, {agent: TAB_GROUPS_OBSERVER, down: true}, {agent: CODEX_REST, down: true},
        ]},
        {branch: '192-hided-sigterm-handler', purpose: '#192 SIGTERM 정리', task: '#192', pr: {number: 221, tone: 'draft'}, distance: '↑3', files: 4, arrows: [1], cells: [
          {agent: SIGTERM_ASK, from: true}, {agent: SIGTERM_REVIEW}, {through: true}, {through: true},
        ]},
        {branch: 'prd/agent-tab-groups', purpose: 'Agent tab groups', pr: {number: 217, tone: 'open'}, distance: '↑39 ↓17', cells: [
          {}, {}, {agent: TAB_GROUPS_IMPL, from: true}, {through: true},
        ]},
        {branch: 'fix/checkout-capability-follow-up', purpose: '체크아웃 권한 후속', cleanup: true, pr: {number: 216, tone: 'merged'}, cells: [
          {}, {}, {}, {agent: {...CODEX_REST, title: '체크아웃 기능 구현 및 정리'}, from: true},
        ]},
      ], {columns, width, selected: 1, folds: ['에이전트 없는 워크트리 14', '정리할 것 5']}),
    ]);
    const lineages = frame(`ov-lineage-${suffix}`, 'Project Overview · Agents › 계보', {layout: 'vertical', gap: '$--spacing-md', width}, [
      overviewHeader(tokens, 'ov-lhead', suffix, {project: 'herdr-ide', facts: HERDR_FACTS, view: 'agents', mode: 'lineage', width}),
      lineage(`ov-lin-${suffix}`, [
        [
          {...SIGTERM_OBSERVER, chips: chips(`ov-lc0-${suffix}`, {branch: 'main', primary: true})},
          {...SIGTERM_ASK, chips: chips(`ov-lc1-${suffix}`, {branch: '192-hided-sigterm-handler', task: '#192', pr: 221})},
          {...SIGTERM_REVIEW, chips: chips(`ov-lc2-${suffix}`, {branch: '192-hided-sigterm-handler', task: '#192'})},
        ],
        [
          {...TAB_GROUPS_OBSERVER, chips: chips(`ov-lc3-${suffix}`, {branch: 'main', primary: true})},
          {...TAB_GROUPS_IMPL, chips: chips(`ov-lc4-${suffix}`, {branch: 'prd/agent-tab-groups', pr: 217})},
        ],
        [{...OVERVIEW_WORK, chips: chips(`ov-lc5-${suffix}`, {branch: 'main', primary: true})}],
      ], {width, names: ['Observer · 보통 main', 'Implementor · 워크트리', '하위 에이전트'], folds: ['쉬는 에이전트 3', '정리할 것 5']}),
    ]);
    const issues = frame(`ov-tasks-${suffix}`, 'Project Overview · Issues › Board', {layout: 'vertical', gap: '$--spacing-md', width: boardWidth}, [
      overviewHeader(tokens, 'ov-head', suffix, {project: 'herdr-ide', facts: HERDR_FACTS, view: 'issues', mode: 'board', width: boardWidth}),
      frame(`ov-cols-${suffix}`, 'Columns', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [
        stageColumn(`ov-backlog-${suffix}`, '백로그', 5, [
          card('b1', {task: gh(201), labels: [ENHANCEMENT], title: 'Add Workspace design reference and visual review coverage', hover: 'start'}),
          card('b2', {task: gh(199), title: 'Read the remote primary checkout over device connections'}),
          card('b3', {task: gh(194), labels: [BUG], title: 'Post-Swift-removal loose ends: hook reinstall after the app swap'}),
          card('b4', {task: gh(191), title: 'Desktop host starts the bundled Herdr server when none answers', locked: '#192'}),
          card('b5', {task: gh(193), title: 'Public release signing: hardened runtime and notarization', locked: '#191'}),
        ], {newIssue: true}),
        stageColumn(`ov-working-${suffix}`, '진행 중', 2, [
          card('w1', ISSUE_192),
          card('w2', LOCAL_3),
        ], {foot: [foldLine(`ov-loose-${suffix}`, '이슈 없는 워크트리 3')]}),
        stageColumn(`ov-review-${suffix}`, '리뷰', 2, [
          card('r1', ISSUE_186),
          card('r2', {task: gh(184), title: 'Bundle hcoord as a Herdr plugin with checkout lineage', branch: '184-hcoord-plugin', pr: {number: 207, checks: 'pending', review: 'changes_requested'}, agents: [
            {mark: 'work', provider: 'codex', title: '리뷰 반영', age: '3m'},
          ]}),
        ], {foot: [foldLine(`ov-loosepr-${suffix}`, '이슈 없는 PR 1')]}),
        doneColumn(`ov-done-${suffix}`, 12, [
          {task: gh(180), title: 'Sidebar readability and the status badge', pr: 180},
          {task: gh(173), title: 'The Workspace side panel', pr: 173},
          {task: gh(169), title: 'Web scope navigation', pr: 169},
          {task: local(2), title: '체크아웃 권한 후속 정리'},
        ], 8),
      ]),
    ]);
    // The card's states (B5-B8, B17): at rest, under the pointer in each
    // stage, a Local issue's edit, the operator's turn, blocked, a failed
    // source read and the card whose panel is open; and the id's preview.
    const states = frame(`ov-states-${suffix}`, 'Project Overview · Issues › Card states', {layout: 'vertical', gap: '$--spacing-md', width: boardWidth}, [
      frame(`ov-states-r1-${suffix}`, 'Under the pointer', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [
        card('s1', {task: gh(201), labels: [ENHANCEMENT], title: 'Add Workspace design reference and visual review coverage', hover: 'start'}),
        card('s2', {...LOCAL_3, hover: 'workspace'}),
        card('s3', {task: gh(184), title: 'Bundle hcoord as a Herdr plugin with checkout lineage', branch: '184-hcoord-plugin', pr: {number: 207, checks: 'pending', review: 'changes_requested'}, hover: 'pr'}),
        card('s4', {task: local(3), title: 'Overview 진입 흐름', hover: 'start', edit: true}),
      ]),
      frame(`ov-states-r2-${suffix}`, 'States', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [
        card('s5', ISSUE_192),
        card('s6', {task: gh(191), title: 'Desktop host starts the bundled Herdr server when none answers', locked: '#192'}),
        card('s7', {task: gh(199), title: 'Read the remote primary checkout over device connections', failure: true}),
        card('s8', {...LOCAL_3, selected: true}),
      ]),
      preview(`ov-preview-${suffix}`, {task: gh(201), labels: [ENHANCEMENT], title: 'Add Workspace design reference and visual review coverage'}, {
        body: ['Workspace 화면에도 Pen 기준 화면과 리뷰 규칙을 둔다.', 'design-review baseline을 Workspace에 만든다.', 'Light와 Dark 모두 캡처한다.'],
        byline: 'hoyeon · 9월 26일 · 댓글 2',
      }),
    ]);
    // The issue panel beside the board, which keeps the width left to it
    // (B10-B16, B18, B19): a GitHub issue in progress, a Local one in the
    // backlog, a Local one being edited, and a GitHub one whose read failed.
    const withPanel = (key, panel) => frame(`ov-panel-${key}-${suffix}`, `Project Overview · Issues › Panel · ${panel.name}`, {layout: 'vertical', gap: '$--spacing-md', width: boardWidth}, [
      overviewHeader(tokens, `ov-phead-${key}`, suffix, {project: 'herdr-ide', facts: HERDR_FACTS, view: 'issues', mode: 'board', width: boardWidth}),
      frame(`ov-psplit-${key}-${suffix}`, 'Board and panel', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [
        frame(`ov-pcols-${key}-${suffix}`, 'Columns', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, panel.columns),
        issuePanel(`ov-panel-${key}-p-${suffix}`, panel.spec),
      ]),
    ]);
    const github = withPanel('gh', {name: 'GitHub', columns: [
      stageColumn(`ov-pgb-${suffix}`, '백로그', 5, [
        card('pg-b1', {task: gh(201), labels: [ENHANCEMENT], title: 'Add Workspace design reference and visual review coverage'}),
        card('pg-b2', {task: gh(199), title: 'Read the remote primary checkout over device connections'}),
      ], {newIssue: true}),
      stageColumn(`ov-pgw-${suffix}`, '진행 중', 2, [card('pg-w1', {...ISSUE_192, selected: true}), card('pg-w2', LOCAL_3)], {foot: [foldLine(`ov-pgloose-${suffix}`, '이슈 없는 워크트리 3')]}),
    ], spec: {
      task: gh(192), title: ISSUE_192.title, stage: '진행 중', labels: [BUG], author: 'yansfil · 9월 27일', updated: '9월 27일',
      work: {branch: ISSUE_192.branch, ahead: 3, files: 4, agents: [
        {mark: 'seen', title: 'SIGTERM 정리 오케스트레이션', age: '20m'},
        {mark: 'ask', provider: 'codex', title: 'SIGTERM 처리와 자식 정리 순서', line: '기존 stdin 종료 경로도 남길까요?', tone: 'request', age: '4m', depth: 1},
      ], pr: {number: 221, tone: 'draft', title: 'hided: stop AI children on SIGTERM before exit', review: 'changes_requested'}},
      body: [['h', '배경'], ['p', 'Found during the Swift removal (#188): hided installs no SIGTERM handler, so a background AI child is ended by the OS closing its stdin pipe.'], ['p', 'Add a graceful stop path: signal handler, owner-thread shutdown, child teardown with a bounded wait.']],
      comments: [['yansfil · 9월 27일', '데스크톱 호스트 종료도 같은 경로로 가야 함']],
    }});
    const localPanel = withPanel('local', {name: 'Local', columns: [
      stageColumn(`ov-plb-${suffix}`, '백로그', 2, [
        card('pl-b1', {task: local(3), title: 'Overview 진입 흐름', selected: true}),
        card('pl-b2', {task: local(4), title: '세션 탭 빈 상태 문구'}),
      ], {newIssue: true}),
    ], spec: {
      task: local(3), title: 'Overview 진입 흐름', stage: '백로그', created: '9월 25일', updated: '9월 27일',
      body: [['p', '사이드바 프로젝트 이름으로 들어오면 Agents › 체크아웃이 먼저 선다.'], ['p', '앞에 있던 체크아웃의 레인을 고른다.']],
    }});
    const editing = withPanel('edit', {name: 'Local 편집', columns: [
      stageColumn(`ov-peb-${suffix}`, '백로그', 2, [
        card('pe-b1', {task: local(3), title: 'Overview 진입 흐름', selected: true}),
        card('pe-b2', {task: local(4), title: '세션 탭 빈 상태 문구'}),
      ], {newIssue: true}),
    ], spec: {
      task: local(3), title: 'Overview 진입 흐름', stage: '백로그', created: '9월 25일', updated: '9월 27일',
      editing: {title: 'Overview 진입 흐름과 레인 선택', body: ['사이드바 프로젝트 이름으로 들어오면 Agents › 체크아웃이 먼저 선다.', '앞에 있던 체크아웃의 레인을 고른다.']},
    }});
    const failed = withPanel('fail', {name: '읽기 실패', columns: [
      stageColumn(`ov-pfb-${suffix}`, '백로그', 5, [
        card('pf-b1', {task: gh(199), title: 'Read the remote primary checkout over device connections', selected: true}),
        card('pf-b2', {task: gh(194), labels: [BUG], title: 'Post-Swift-removal loose ends: hook reinstall after the app swap'}),
      ], {newIssue: true}),
    ], spec: {
      task: gh(199), title: 'Read the remote primary checkout over device connections', stage: '백로그', updated: '9월 26일',
      failure: '이슈를 읽지 못함 · 이유는 로그에', body: [], comments: [],
    }});
    const dependencies = frame(`ov-deps-${suffix}`, 'Project Overview · Issues › Dependencies', {layout: 'vertical', gap: '$--spacing-lg', width: boardWidth}, [
      overviewHeader(tokens, 'ov-dhead', suffix, {project: 'herdr-ide', facts: HERDR_FACTS, view: 'issues', mode: 'dependencies', width: boardWidth}),
      legend(`ov-dlegend-${suffix}`),
      chain(`ov-dchain-${suffix}`, [
        card('d1', {...ISSUE_192, word: '진행 중'}),
        arrow(`ov-da1-${suffix}`),
        card('d2', {task: gh(191), title: 'Desktop host starts the bundled Herdr server when none answers', word: '백로그', locked: '#192', dim: true}),
        arrow(`ov-da2-${suffix}`),
        card('d3', {task: gh(193), title: 'Public release signing: hardened runtime and notarization', word: '백로그', locked: '#191', dim: true}),
      ]),
      text(`ov-dunrel-${suffix}`, '관계 없는 태스크', {size: '$--text-subhead', weight: '600', fill: '$--subtle-foreground'}),
      frame(`ov-dunrelrow-${suffix}`, 'Unrelated', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [
        card('d4', {...ISSUE_186, word: '리뷰'}),
        card('d5', {task: gh(201), title: 'Add Workspace design reference and visual review coverage', word: '백로그'}),
        card('d6', {task: gh(199), title: 'Read the remote primary checkout over device connections', word: '백로그'}),
      ]),
    ]);
    // The PRs tile's view (PRD overview-lenses-prs B2-B6, B19): the groups by
    // whose move it is, a row unfolded to its lineage and icon buttons, a row
    // under the pointer with 맡기기 in the time slot, 최근 머지 folded; then
    // 이슈 잇기's confirmation over it (B10).
    const prsView = frame(`ov-prs-${suffix}`, 'Project Overview · PRs', {layout: 'vertical', gap: '$--spacing-md', width: boardWidth}, [
      overviewHeader(tokens, 'ov-prhead', suffix, {project: 'herdr-ide', facts: HERDR_FACTS, view: 'prs', width: boardWidth}),
      prGroup(`ov-prg1-${suffix}`, '내 차례', 4, '$--warning', [
        ...prRow(`ov-pr222-${suffix}`, {number: 222, title: 'Start the bundled Herdr server when none answers on the socket', issue: gh(191), branch: '191-desktop-starts-herdr', checks: 'passing', review: 'review_required', age: '12m', agents: [{mark: 'done', title: 'Herdr 서버 시작 구현', line: 'PR 올림 · CI 통과', age: '12m'}], look: true, open: true}),
        ...prRow(`ov-pr218-${suffix}`, {number: 218, title: 'Overview lenses: tiles and checkout lanes', branch: 'feat/overview-lenses', checks: 'passing', review: 'review_required', age: '1h', linkable: true}),
        ...prRow(`ov-pr189-${suffix}`, {number: 189, title: 'Bump actions/upload-artifact from 4 to 7', branch: 'dependabot/github_actions/upload-artifact-7', checks: 'passing', review: 'approved', age: '1d'}),
      ]),
      prGroup(`ov-prg2-${suffix}`, '에이전트가 고치는 중', 1, null, [
        ...prRow(`ov-pr221-${suffix}`, {number: 221, tone: 'draft', title: 'hided: stop AI children on SIGTERM before exit', issue: gh(192), branch: '192-hided-sigterm-handler', checks: 'failed', review: 'changes_requested', age: '4m', agents: [{mark: 'work'}, {mark: 'ask'}, {mark: 'seen'}]}),
      ]),
      prGroup(`ov-prg3-${suffix}`, 'CI 실패 · 맡은 에이전트 없음', 2, null, [
        ...prRow(`ov-pr190-${suffix}`, {number: 190, title: 'Bump tokio-tungstenite from 0.26.2 to 0.29.0', branch: 'dependabot/cargo/tokio-tungstenite-0.29.0', checks: 'failed', age: '1d', hover: 'delegate'}),
        ...prRow(`ov-pr138-${suffix}`, {number: 138, title: 'Bump sha2 from 0.10.9 to 0.11.0', branch: 'dependabot/cargo/sha2-0.11.0', checks: 'failed', age: '2d'}),
      ]),
      prGroup(`ov-prg4-${suffix}`, '최근 머지', 12, null, [], {folded: true}),
    ]);
    const prsConfirm = frame(`ov-prconfirm-${suffix}`, 'Project Overview · PRs › 이슈 잇기 확인', {layout: 'vertical', gap: '$--spacing-md', width: boardWidth}, [
      prGroup(`ov-prcg-${suffix}`, '내 차례', 4, '$--warning', [
        ...prRow(`ov-prc218-${suffix}`, {number: 218, title: 'Overview lenses: tiles and checkout lanes', branch: 'feat/overview-lenses', checks: 'passing', review: 'review_required', age: '1h', linkable: true, hover: 'default'}),
      ]),
      screenDialogSurface(`ov-prcd-${suffix}`, {
        width: num(tokens, '--size-add-device-sheet-w'), title: 'PR #218을 #212에 잇기', prose: true,
        description: 'PR #218 본문에 "Closes #212"을 씁니다. 머지되면 GitHub가 이슈를 닫습니다.',
        actions: [screenButton(`ov-prcd-no-${suffix}`, '그만두기', {variant: 'secondary'}), screenButton(`ov-prcd-yes-${suffix}`, '본문에 쓰기')],
      }),
    ]);
    return [
      frame(`ov-agentside-${suffix}`, 'Agents', {layout: 'vertical', gap: '$--spacing-xl'}, [checkouts, lineages]),
      frame(`ov-issueside-${suffix}`, 'Issues', {layout: 'vertical', gap: '$--spacing-xl'}, [issues, states, dependencies]),
      frame(`ov-panelside-${suffix}`, 'Issue panel', {layout: 'vertical', gap: '$--spacing-xl'}, [github, localPanel, editing, failed]),
      frame(`ov-prside-${suffix}`, 'PRs', {layout: 'vertical', gap: '$--spacing-xl'}, [prsView, prsConfirm]),
    ];
  }
  return screenSheet('screen-project-overview', 'Screen / Project Overview', OVERVIEW_SPEC, build, build);
}

// -- Screen / Workspace ---------------------------------------------------------

// A leading-icon tab for the agent/terminal column's own tab strip
// (TabBar.tsx) - distinct from screenViewTab, which is the editor column's
// tab and carries no icon. No library master matches this exact shape
// (icon + label + its own underline), so it is hand-composed, the same way
// screenDialogSurface is where no Dialog master fits either.
function screenPanelTab(id, glyph, title, active, provider = null) {
  return frame(id, title, {
    layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', padding: [0, '$--spacing-xs', '$--spacing-xxs', '$--spacing-xs'],
    ...(active ? {stroke: '$--foreground', strokeWidth: {bottom: 2}, strokeAlignment: 'inner'} : {}),
  }, [
    ...(provider ? [text(`${id}-status`, '●', {fill: '$--agent-working', size: '$--text-caption'}), frame(`${id}-provider`, 'Provider logo', {width: 12, height: 12, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${provider}.png`, mode: 'fit'}}, [])] : [icon(`${id}-i`, glyph, {size: 12, fill: active ? '$--foreground' : '$--subtle-foreground'})]),
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

// The side panel's own icon button: a pressed one sits on --secondary with a
// --foreground glyph, the rest stay --subtle-foreground (WorkspaceScreen.tsx).
function sidePanelButton(id, glyph, {pressed = false, size} = {}) {
  return themedXref(id, 'Nyvom', glyph, {...(pressed ? {fill: '$--secondary'} : {}), ...(size ? {width: size, height: size} : {})}, {ZIZFR: {icon: glyph, fill: pressed ? '$--foreground' : '$--subtle-foreground'}});
}

// The panel toggle as the toolbar carries it while the panel is closed: the
// open-view count rides on it as a --primary badge (Component / Side panel toggle).
function sidePanelToggle(id, count) {
  return frame(id, 'Side panel toggle', {layout: 'none', width: 24, height: 24}, [
    {...sidePanelButton(`${id}-button`, 'panel-right'), x: 0, y: 0},
    frame(`${id}-badge`, 'Open views badge', {x: 12, y: -2, width: 14, height: 14, fill: '$--primary', cornerRadius: '$--radius-lg', layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [
      text(`${id}-count`, String(count), {size: '$--text-micro', weight: '600', fill: '$--primary-foreground'}),
    ]),
  ]);
}

function buildWorkspace(tokens) {
  const SIDEBAR_W = num(tokens, '--size-sidebar-ideal'), MAIN_W = 900, MAIN_H = 460;
  const ROW = num(tokens, '--size-tab-strip'), GAP = num(tokens, '--spacing-sm'), TOOLS_W = num(tokens, '--size-panel-ideal');
  const HAIR = '$--size-hairline', PANEL_W = GAP + 300 + TOOLS_W;
  const rule = {stroke: '$--border', strokeWidth: {bottom: HAIR}, strokeAlignment: 'inner'};
  const TERMINAL = [
    ['fixture % echo capture-demo 한글 확인', '$--foreground'],
    ['capture-demo 한글 확인', '$--subtle-foreground'],
    ['fixture % git status --short', '$--foreground'],
    [' M docs/한글 노트.md', '$--warning'],
    ['?? scripts/pen-screens.mjs', '$--subtle-foreground'],
    ['fixture % ', '$--foreground'],
  ];

  // The toolbar spans only the agent column: the path back, and the panel
  // toggle only while the panel is closed. No Explorer or History toggles.
  function toolbar(key, count) {
    return frame(`ws-topbar-${key}`, 'Toolbar', {width: 'fill_container', height: ROW, layout: 'horizontal', justifyContent: 'space_between', alignItems: 'center', padding: [0, '$--spacing-sm'], fill: '$--sidebar', ...rule}, [
      frame(`ws-crumb-${key}`, 'Breadcrumb', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        text(`ws-c1-${key}`, 'Main', {fill: '$--muted-foreground'}),
        text(`ws-c2-${key}`, '/', {fill: '$--muted-foreground'}),
        text(`ws-c3-${key}`, 'demo', {fill: '$--muted-foreground'}),
        text(`ws-c4-${key}`, '/', {fill: '$--muted-foreground'}),
        text(`ws-c5-${key}`, 'demo', {weight: '600'}),
      ]),
      ...(count ? [sidePanelToggle(`ws-paneltoggle-${key}`, count)] : []),
    ]);
  }

  // The agent column: the toolbar, the agents' tab strip, the pane, the terminal.
  // The panel never resizes it while it floats over it.
  function agentArea(key, active = true) {
    return frame(`ws-agentarea-${key}`, 'Agent area', {width: 'fill_container', height: 'fill_container', layout: 'vertical', gap: 0, fill: '$--background', clip: true}, [
      frame(`ws-agtabs-${key}`, 'Tab bar', {width: 'fill_container', height: ROW, layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
        screenPanelTab(`ws-agtab1-${key}`, 'square-terminal', active ? '탭 이름과 구성 개선' : '검증 결과 확인', active, 'claude'),
        screenIconButton(`ws-agtabclose-${key}`, 'x', {size: 20}),
        screenIconButton(`ws-agtabadd-${key}`, 'plus', {size: 20}),
      ]),
      screenPaneHeader(`ws-panehdr-${key}`, {label: 'w2:p1', status: 'Working', width: 'fill_container'}),
      // The xterm viewport takes --background in either theme (commit 7052afa).
      frame(`ws-terminal-${key}`, 'Terminal', {width: 'fill_container', height: 'fill_container', fill: '$--background', padding: '$--spacing-sm', layout: 'vertical', gap: '$--spacing-xxs'},
        TERMINAL.map(([line, fill], index) => text(`ws-term${index}-${key}`, line, {fill, mono: true, size: '$--text-caption'}))),
    ]);
  }

  function agentColumn(key, count, groups = false) {
    const areas = groups
      ? frame(`ws-agentareas-${key}`, 'Two Agent areas', {width: 'fill_container', height: 'fill_container', layout: 'horizontal', gap: 0}, [
        agentArea(`${key}-left`),
        frame(`ws-agentdivider-${key}`, 'Agent area divider', {width: '$--size-resize-handle', height: 'fill_container', fill: '$--border'}),
        agentArea(`${key}-right`, false),
      ])
      : agentArea(key);
    return frame(`ws-agents-${key}`, 'Agent column', {width: MAIN_W, height: MAIN_H, layout: 'vertical', gap: 0, fill: '$--background', clip: true}, [toolbar(key, count), areas]);
  }

  // Component / Side panel (issue 170, "Side panel hierarchy, revised"), from
  // flat refs and local tokens: full height beside the agent column's toolbar,
  // the --spacing-sm gap on its left, the --card card with a --radius-lg
  // top-left corner and a --border hairline, no shadow.
  function sidePanel(key) {
    const tab = (id, title, glyph, fill, active) => themedXref(id, 'view-tab', title, {
      fill: '$--card', ...(active ? {stroke: '$--primary', strokeWidth: {bottom: '$--size-tab-indicator'}, strokeAlignment: 'inner'} : {}),
    }, {'view-tab-mark': {icon: glyph, fill}, 'view-tab-title': {content: title, fill: active ? '$--foreground' : '$--subtle-foreground'}, 'view-tab-close': {enabled: active}});
    // Row 1, at the toolbar row's height: the area's tabs and its New tab, then
    // the tool-column toggle, Expand, Pin and the panel toggle.
    const row1 = frame(`ws-sp-row1-${key}`, 'Row 1', {width: 'fill_container', height: ROW, layout: 'horizontal', alignItems: 'center', ...rule}, [
      frame(`ws-sp-tabs-${key}`, 'Area tabs', {width: 'fill_container', height: ROW, layout: 'horizontal', alignItems: 'center', clip: true}, [
        tab(`ws-sp-tab1-${key}`, '한글 노트.md', 'file-text', '$--file-blue', true),
        tab(`ws-sp-tab2-${key}`, 'pen-screens.mjs', 'file-code', '$--file-orange', false),
        sidePanelButton(`ws-sp-new-${key}`, 'plus', {size: 20}),
        frame(`ws-sp-tabsgap-${key}`, 'Spacer', {width: 'fill_container', height: 1}, []),
        frame(`ws-sp-viewactions-${key}`, 'View actions', {width: num(tokens, '--size-tab-overflow-control'), height: ROW, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [
          icon(`ws-sp-viewactions-i-${key}`, 'ellipsis', {size: num(tokens, '--size-icon'), fill: '$--subtle-foreground'}),
        ]),
      ]),
      frame(`ws-sp-actions-${key}`, 'Side panel actions', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', padding: [0, '$--spacing-sm', 0, '$--spacing-xs']}, [
        sidePanelButton(`ws-sp-tools-${key}`, 'panel-right-dashed', {pressed: true}), sidePanelButton(`ws-sp-expand-${key}`, 'maximize-2'),
        sidePanelButton(`ws-sp-pin-${key}`, 'pin'), sidePanelButton(`ws-sp-hide-${key}`, 'panel-right', {pressed: true}),
      ]),
    ]);
    // The web document header (Editor.tsx): the path, then Live (Markdown), Wrap and Find as ghost buttons.
    const docHeader = frame(`ws-dochdr-${key}`, 'Document header', {width: 'fill_container', height: ROW, layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-md'], ...rule}, [
      text(`ws-docpath-${key}`, 'docs/한글 노트.md', {size: '$--text-caption', fill: '$--subtle-foreground', width: 'fill_container'}),
      frame(`ws-doclinks-${key}`, 'Links', {layout: 'horizontal', gap: '$--spacing-md'}, [
        text(`ws-doclive-${key}`, 'Live', {weight: '500', fill: '$--foreground'}),
        text(`ws-docwrap-${key}`, 'Wrap', {weight: '500', fill: '$--muted-foreground'}),
        text(`ws-docfind-${key}`, 'Find', {weight: '500', fill: '$--muted-foreground'}),
      ]),
    ]);
    const line = (index, code) => frame(`ws-line${index}-${key}`, 'Line', {layout: 'horizontal', gap: '$--spacing-sm'}, [
      text(`ws-line${index}n-${key}`, String(index), {mono: true, size: '$--text-caption', fill: '$--muted-foreground'}),
      text(`ws-line${index}t-${key}`, code, {mono: true, size: '$--text-caption'}),
    ]);
    const views = frame(`ws-sp-views-${key}`, 'View areas', {width: 'fill_container', height: 'fill_container', layout: 'vertical', gap: 0}, [
      docHeader,
      frame(`ws-editor-${key}`, 'Editor body', {width: 'fill_container', height: 'fill_container', layout: 'vertical', gap: '$--spacing-xxs', padding: '$--spacing-sm', clip: true},
        ['# 한글 노트', '', '작업 공간의 사이드 패널은 에이전트 위에 뜹니다.', 'Pin 하면 에이전트 옆에 고정됩니다.'].map((code, index) => line(index + 1, code))),
    ]);
    // Row 2 over the tool column: the Explorer and History icon tabs, the active one marked.
    const toolTab = (id, glyph, name, active) => frame(id, name, {width: ROW, height: ROW, layout: 'horizontal', justifyContent: 'center', alignItems: 'center', ...(active ? {stroke: '$--primary', strokeWidth: {bottom: '$--size-tab-indicator'}, strokeAlignment: 'inner'} : {})}, [
      icon(`${id}-i`, glyph, {size: num(tokens, '--size-icon'), fill: active ? '$--foreground' : '$--subtle-foreground'}),
    ]);
    const row = (id, name, glyph, fill, {folder = false, status = '', indent = 0} = {}) => frame(`${id}-indent`, 'Tree indent', {width: 'fill_container', layout: 'horizontal', padding: [0, 0, 0, indent]}, [
      themedXref(id, 'mSu8p', name, {width: 'fill_container'}, {
        vEOYq: folder ? {icon: 'chevron-down', fill: '$--subtle-foreground'} : {fill: []},
        LWCQZ: {icon: glyph, fill}, kj232: {content: name}, ZzvYJ: {content: status, fill: '$--warning'},
      }),
    ]);
    const tools = frame(`ws-sp-tools-col-${key}`, 'Tool column', {width: TOOLS_W, height: 'fill_container', layout: 'vertical', gap: 0, stroke: '$--border', strokeWidth: {left: HAIR}, strokeAlignment: 'inner'}, [
      frame(`ws-sp-tooltabs-${key}`, 'Row 2: tool tabs', {width: 'fill_container', height: ROW, layout: 'horizontal', alignItems: 'center', padding: [0, 0, 0, '$--spacing-xxs'], ...rule}, [
        toolTab(`ws-sp-explorer-${key}`, 'folder', 'Explorer', true), toolTab(`ws-sp-history-${key}`, 'git-branch', 'History', false),
      ]),
      // The Explorer's root row (ExplorerTree.tsx), as tall as a tab strip.
      frame(`ws-exproot-${key}`, 'Root', {width: 'fill_container', height: ROW, layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', padding: [0, '$--spacing-sm', 0, '$--spacing-md'], ...rule}, [
        text(`ws-exproott-${key}`, 'demo', {size: '$--text-caption', fill: '$--subtle-foreground', width: 'fill_container'}),
        screenIconButton(`ws-exprefresh-${key}`, 'refresh-cw', {size: 20}),
      ]),
      row(`ws-file1-${key}`, 'docs', 'folder-open', '$--subtle-foreground', {folder: true, status: '●'}),
      row(`ws-file2-${key}`, '한글 노트.md', 'file-text', '$--file-blue', {status: 'M', indent: '$--spacing-md'}),
      row(`ws-file3-${key}`, 'scripts', 'folder-open', '$--subtle-foreground', {folder: true}),
      row(`ws-file4-${key}`, 'pen-screens.mjs', 'file-code', '$--file-orange', {status: 'A', indent: '$--spacing-md'}),
      row(`ws-file5-${key}`, 'README.md', 'file-text', '$--file-blue'),
    ]);
    return frame(`ws-sidepanel-${key}`, 'Side panel', {x: MAIN_W - PANEL_W, y: 0, width: PANEL_W, height: MAIN_H, layout: 'horizontal', gap: 0, fill: '$--background'}, [
      frame(`ws-sp-grip-${key}`, 'Resize grip (the gap)', {width: GAP, height: 'fill_container'}, []),
      frame(`ws-sp-card-${key}`, 'Card', {width: 'fill_container', height: 'fill_container', layout: 'vertical', gap: 0, clip: true, fill: '$--card', cornerRadius: ['$--radius-lg', 0, 0, 0], stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
        row1, frame(`ws-sp-body-${key}`, 'Body', {width: 'fill_container', height: 'fill_container', layout: 'horizontal', gap: 0}, [views, tools]),
      ]),
    ]);
  }

  // One composition: the sidebar, then the Workspace. Open, the panel runs its
  // full height over the agent column; closed, the toolbar carries the panel
  // toggle with the open-view count.
  function workspace(key, open, groups = false) {
    const sidebar = screenSidebar(tokens, 'ws-sidebar', key, [
      {title: 'Agent two', status: 'Working'},
      {title: 'Agent one', status: 'Seen', symbol: '○', statusColor: '$--muted-foreground'},
    ]);
    const main = open
      ? frame(`ws-main-${key}`, 'Workspace', {width: MAIN_W, height: MAIN_H, layout: 'none', clip: true}, [{...agentColumn(key, 0), x: 0, y: 0}, sidePanel(key)])
      : frame(`ws-main-${key}`, 'Workspace', {width: MAIN_W, height: MAIN_H, layout: 'vertical', gap: 0}, [agentColumn(key, 2, groups)]);
    return frame(`ws-wrap-${key}`, groups ? 'Two Agent areas, independent tab bars and live terminals' : open ? 'Side panel open' : 'Side panel closed, two views open', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [sidebar, main]);
  }
  function newTab(key, changed) {
    const choice = (name, glyph, shortcut = '') => frame(`ws-newtab-${name}-${key}`, name, {width: 'fill_container', height: num(tokens, '--size-control-lg'), layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', padding: [0, '$--spacing-lg'], fill: '$--secondary', cornerRadius: '$--radius-sm'}, [
      icon(`ws-newtab-${name}-i-${key}`, glyph, {size: num(tokens, '--size-icon'), fill: '$--subtle-foreground'}),
      text(`ws-newtab-${name}-t-${key}`, name, {width: 'fill_container', size: '$--text-body', weight: '500'}),
      ...(shortcut ? [themedXref(`ws-newtab-kbd-${key}`, 'kbd-m', shortcut, {}, {'kbd-t': {content: shortcut}})] : []),
    ]);
    return frame(`ws-newtab-${key}`, changed ? 'New tab, checkout has changes' : 'New tab, clean checkout', {width: 480, height: 360, layout: 'vertical', gap: 0, fill: '$--card', clip: true}, [
      frame(`ws-newtab-tabs-${key}`, 'View tabs', {width: 'fill_container', height: ROW, layout: 'horizontal', alignItems: 'center', ...rule}, [
        themedXref(`ws-newtab-tab-${key}`, 'view-tab', 'New tab', {fill: '$--card', stroke: '$--primary', strokeWidth: {bottom: '$--size-tab-indicator'}, strokeAlignment: 'inner'}, {'view-tab-mark': {icon: 'globe', fill: '$--subtle-foreground'}, 'view-tab-title': {content: 'New tab', fill: '$--foreground'}, 'view-tab-close': {enabled: true}}),
        sidePanelButton(`ws-newtab-plus-${key}`, 'plus'),
      ]),
      frame(`ws-newtab-address-row-${key}`, 'Address row', {width: 'fill_container', height: ROW, layout: 'horizontal', gap: '$--spacing-xs', padding: [0, '$--spacing-sm'], alignItems: 'center', ...rule}, [
        ...['arrow-left', 'arrow-right', 'rotate-cw'].map((glyph, n) => icon(`ws-newtab-nav${n}-${key}`, glyph, {size: num(tokens, '--size-icon'), fill: '$--muted-foreground', opacity: num(tokens, '--opacity-disabled')})),
        frame(`ws-newtab-address-${key}`, 'Empty focused address', {width: 'fill_container', height: num(tokens, '--size-control-sm'), layout: 'horizontal', alignItems: 'center', padding: [0, '$--spacing-sm'], fill: '$--background', cornerRadius: '$--radius-sm', stroke: '$--ring', strokeWidth: 1}, [text(`ws-newtab-placeholder-${key}`, 'Enter a URL', {mono: true, size: '$--text-caption', fill: '$--muted-foreground'})]),
      ]),
      frame(`ws-newtab-body-${key}`, 'Open', {width: 'fill_container', layout: 'vertical', gap: '$--spacing-sm', padding: '$--spacing-xl'}, [
        text(`ws-newtab-heading-${key}`, 'Open', {size: '$--text-body', weight: '500', fill: '$--muted-foreground'}),
        choice('File', 'file-search', '⌘P'), ...(changed ? [choice('Diff', 'git-compare-arrows')] : []),
      ]),
    ]);
  }
  const build = suffix => [workspace(`${suffix}o`, true), workspace(`${suffix}c`, false), workspace(`${suffix}g`, false, true), newTab(`${suffix}n`, true), newTab(`${suffix}e`, false)];
  return screenSheet('screen-workspace', 'Screen / Workspace', 'web/src/WorkspaceScreen.tsx, AreaTree.tsx, AgentAreas.tsx, TabBar.tsx, ViewAreas.tsx, Tools.tsx: the Workspace with its side panel (Component / Side panel, issue 170) open at full height over the agent column, then closed with two views still open, and with two Agent areas using the shared divider and independent tab bars. The toolbar spans only the agent column and holds no tool toggles; row 1 of the panel holds the area tabs and their New tab, then the tool-column toggle, Expand, Pin and the panel toggle, which the toolbar carries with the open-view count while the panel is closed; row 2 holds the document header and the Explorer and History tool tabs. A Korean file name verifies B11 wrapping.', build, build);
}

// -- Screen / Project Sessions --------------------------------------------------

// The Overview on its Sessions tile: the same header as the Issues board, then
// the Project's session list and the read-only detail beside it.
function buildSessions(tokens) {
  function build(suffix) {
    const header = overviewHeader(tokens, 'ss-head', suffix, {project: 'fixture', facts: [
      {glyph: 'folder-git-2', label: '2 worktrees'},
      {glyph: 'hard-drive', label: '812 MB'},
    ], view: 'sessions', width: 820});
    const list = frame(`ss-list-${suffix}`, 'List', {width: 320, layout: 'vertical', gap: '$--spacing-sm'}, [
      screenTabs(`ss-tabs-${suffix}`, ['All', 'Codex', 'Claude Code'], 0),
      screenInput(`ss-search-${suffix}`, {placeholder: 'Search sessions', width: 300}),
      text(`ss-count-${suffix}`, '1 session', {size: '$--text-caption', fill: '$--muted-foreground'}),
      screenSessionRow(`ss-row1-${suffix}`, {title: '배포 스크립트 정리하고 release note 초안까지 작성해줘', checkout: 'fixture', provider: 'Claude Code', time: 'Sep 21, 10:00 AM', width: 300}),
    ]);
    const detail = frame(`ss-detail-${suffix}`, 'Detail', {width: 480, height: 320, alignItems: 'center', justifyContent: 'center', fill: '$--card', cornerRadius: '$--radius-md'}, [
      text(`ss-empty-${suffix}`, 'Choose a session to read it here.', {fill: '$--muted-foreground'}),
    ]);
    return [frame(`ss-wrap-${suffix}`, 'Wrap', {layout: 'vertical', gap: '$--spacing-md'}, [header, frame(`ss-row-${suffix}`, 'Row', {layout: 'horizontal', gap: '$--spacing-lg'}, [list, detail])])];
  }
  return screenSheet('screen-sessions', 'Screen / Project Sessions', 'web/src/ProjectOverview.tsx on its Sessions tab, ProjectSessions.tsx: the Overview’s header and tabs over the provider-filtered session list with search, and the read-only detail pane, using a real Korean session title.', build, build);
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
  const TABS = ['General', 'Appearance', 'Agents', 'Issues', 'Devices', 'Performance', 'Shortcuts'];
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
  return screenSheet('screen-settings', 'Screen / Settings', 'web/src/SettingsSheet.tsx (a Dialog), settings.ts SETTINGS_TABS, settings-rows.tsx Group/Row: the seven-tab strip (General, Appearance, Agents, Issues, Devices, Performance, Shortcuts) and, on Appearance, the Theme group (ToggleGroup + accent swatches) and Density group (Slider), matching web/src/SettingsSheet.tsx’s AppearanceTab exactly rather than a Select-based approximation.', build, build);
}

// -- Screen / Palette ------------------------------------------------------------

// The ⌘K palette in the search view's form (issue #154): the sidebar's
// Search field that opens it, the query row with its Esc keycap, results under
// `<project> > AGENTS` / `WORKSPACE > COMMANDS` / `WORKSPACES > PROJECTS` /
// `WORKSPACES > CHECKOUTS` headers, two-line rows with the agent's own mark,
// and ↵ on the selected row. Rows are authored here on local tokens, as the
// Overview board authors its agent rows, since no library master draws a
// palette row; the keycaps are System / Kbd refs. The marks are the provider
// artwork the web shell bundles (web/src/assets), not a stand-in.
function buildPalette(tokens) {
  const W = num(tokens, '--size-search-sheet-w');
  const SIDEBAR = num(tokens, '--size-sidebar-ideal');
  const MARK = num(tokens, '--size-agent-badge-compact');
  const ROW = W - 2 * num(tokens, '--spacing-xxs');
  const MARKS = {claude: '../web/src/assets/agent-claude.png', codex: '../web/src/assets/agent-codex.png'};
  function build(suffix) {
    const id = (name) => `pal-${name}-${suffix}`;
    const kbd = (key, label) => themedXref(id(key), 'kbd-m', label, {}, {'kbd-t': {content: label}});
    const mark = (key, kind) => frame(id(key), 'Agent mark', {width: MARK, height: MARK, cornerRadius: '$--radius-xs', fill: {type: 'image', url: MARKS[kind], mode: 'fill'}}, []);
    const glyph = (key, name) => frame(id(key), 'Icon', {width: MARK, height: MARK, layout: 'horizontal', alignItems: 'center', justifyContent: 'center'}, [
      icon(`${id(key)}-i`, name, {size: num(tokens, '--size-icon'), fill: '$--muted-foreground'}),
    ]);
    function row(key, {lead, title, detail, mono = false, selected = false, unavailable = false}) {
      return frame(id(key), selected ? 'Row · selected' : 'Row', {
        layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: ROW, padding: ['$--spacing-xs', '$--spacing-sm'],
        cornerRadius: '$--radius-xs', ...(selected ? {fill: '$--accent'} : {}), ...(unavailable ? {opacity: disabledOpacity} : {}),
      }, [
        lead,
        frame(`${id(key)}-body`, 'Body', {layout: 'vertical', gap: 0, width: 'fill_container'}, [
          text(`${id(key)}-title`, title, {weight: '500', fill: unavailable ? '$--muted-foreground' : '$--foreground'}),
          ...(detail ? [text(`${id(key)}-detail`, detail, {size: '$--text-caption', fill: '$--muted-foreground', mono})] : []),
        ]),
        ...(selected ? [text(`${id(key)}-enter`, '↵', {size: '$--text-caption', fill: '$--muted-foreground'})] : []),
      ]);
    }
    const heading = (key, label) => frame(id(key), 'Group heading', {padding: ['$--spacing-xs', '$--spacing-sm'], width: ROW}, [
      text(`${id(key)}-t`, label, {size: '$--text-caption', weight: '500', fill: '$--muted-foreground'}),
    ]);
    function surface(key, {query, placeholder, list}) {
      return frame(id(key), 'Palette', {width: W, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', layout: 'vertical', clip: true}, [
        frame(`${id(key)}-in`, 'Query', {height: num(tokens, '--size-control-lg'), width: W, layout: 'horizontal', alignItems: 'center', gap: '$--spacing-sm', padding: [0, '$--spacing-md'], stroke: '$--border', strokeWidth: {bottom: num(tokens, '--size-hairline')}}, [
          icon(`${id(key)}-ini`, 'search', {size: num(tokens, '--size-icon'), fill: '$--muted-foreground'}),
          frame(`${id(key)}-inf`, 'Field', {width: 'fill_container'}, [
            query ? text(`${id(key)}-inq`, query) : text(`${id(key)}-inp`, placeholder, {fill: '$--muted-foreground'}),
          ]),
          kbd(`${key}-esc`, 'Esc'),
        ]),
        frame(`${id(key)}-list`, 'List', {layout: 'vertical', gap: 0, padding: '$--spacing-xxs'}, list),
      ]);
    }
    const empty = (key, words) => frame(id(key), 'Empty', {padding: ['$--spacing-sm', '$--spacing-md']}, [
      text(`${id(key)}-t`, words, {size: '$--text-caption', fill: '$--muted-foreground'}),
    ]);
    const labelled = (key, label, node) => frame(id(key), label, {layout: 'vertical', gap: '$--spacing-xs'}, [
      text(`${id(key)}-cap`, label, {size: '$--text-micro', weight: '600', fill: '$--muted-foreground'}),
      node,
    ]);

    // The sidebar's tab strip on Agents: Search, the icon at its end, opens
    // ⌘K, and its hint names the chord (PRD sidebar-shell D-03, D-07).
    const sidebar = frame(id('side'), 'Sidebar', {width: SIDEBAR, layout: 'vertical', gap: '$--spacing-xs', fill: '$--sidebar', padding: [0, 0, '$--spacing-md', 0], cornerRadius: '$--radius-md'}, [
      frame(id('side-modes'), 'Modes', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: SIDEBAR, height: num(tokens, '--size-tab-strip'), padding: [0, '$--spacing-xs', 0, '$--spacing-md']}, [
        text(id('side-projects'), 'Projects', {size: '$--text-caption', fill: '$--muted-foreground'}),
        text(id('side-agents'), 'Agents', {size: '$--text-caption'}),
        frame(id('side-gap'), 'Spacer', {width: 'fill_container', height: 1}, []),
        screenIconButton(id('side-search'), 'search'),
      ]),
      frame(id('side-hintwrap'), 'Search hint', {width: SIDEBAR, layout: 'horizontal', justifyContent: 'end', padding: [0, '$--spacing-xs']}, [
        frame(id('side-hint'), 'Hint', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: ['$--spacing-xs', '$--spacing-sm'], cornerRadius: '$--radius-sm', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
          text(id('side-hintl'), 'Search', {size: '$--text-caption', fill: '$--popover-foreground'}),
          text(id('side-hintk'), '⌘K', {size: '$--text-caption', fill: '$--muted-foreground'}),
        ]),
      ]),
      frame(id('side-sectwrap'), 'Section', {padding: ['$--spacing-sm', '$--spacing-md', 0, '$--spacing-md']}, [
        text(id('side-sect'), 'NEEDS YOU · 1', {size: '$--text-micro', fill: '$--muted-foreground'}),
      ]),
      frame(id('side-a0'), 'Agent row', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', padding: ['$--spacing-xs', '$--spacing-md']}, [
        screenStatusMark(tokens, id('side-a0s'), '?', '$--warning'),
        mark('side-a0m', 'claude'),
        text(id('side-a0t'), 'hcoord 원격 에이전트 구현', {weight: '500'}),
      ]),
    ]);

    const results = surface('default', {placeholder: 'Search agents and workspaces', list: [
      heading('g-herdr', 'herdr-ide > AGENTS'),
      row('r0', {lead: mark('r0m', 'claude'), title: 'Electron포팅지침이행', detail: '웹 E2E 테스트 60개 통과, verify 진행 중', selected: true}),
      row('r1', {lead: mark('r1m', 'claude'), title: 'Electron 기본 포팅 구현', detail: 'PR #149 main 병합 진행: 단위테스트 통과, e2e 및 코드 리뷰 실행 중'}),
      row('r2', {lead: mark('r2m', 'codex'), title: 'macOS 단축키 Electron 포팅', detail: 'Idle'}),
      heading('g-sasu', 'sasu > AGENTS'),
      row('r3', {lead: mark('r3m', 'claude'), title: 'hcoord 원격 에이전트 구현', detail: '방안 A와 데몬 재시작을 승인하세요'}),
      heading('g-projects', 'WORKSPACES > PROJECTS'),
      row('r4', {lead: glyph('r4i', 'folder'), title: 'herdr-ide', detail: '~/projects/herdr-ide', mono: true}),
      heading('g-checkouts', 'WORKSPACES > CHECKOUTS'),
      row('r5', {lead: glyph('r5i', 'git-branch'), title: 'herdr-ide / main', detail: '~/projects/herdr-ide', mono: true}),
      row('r6', {lead: glyph('r6i', 'git-branch'), title: 'herdr-ide / quick/154-search-palette', detail: '~/projects/herdr-ide.worktrees/web-search-palette', mono: true}),
    ]});

    const commands = surface('commands', {query: 'split', list: [
      heading('g-commands', 'WORKSPACE > COMMANDS'),
      row('c0', {lead: glyph('c0i', 'chevron-right'), title: 'Split right', selected: true}),
      row('c1', {lead: glyph('c1i', 'chevron-right'), title: 'Split down', detail: 'This Workspace already shows 6 view areas, the most it can.', unavailable: true}),
    ]});
    const noMatch = surface('nomatch', {query: 'zzz', list: [empty('nomatch-e', 'No matching agents or workspaces')]});
    const nothing = surface('nothing', {placeholder: 'Search agents and workspaces', list: [empty('nothing-e', 'No agents or workspaces yet')]});
    const files = surface('files', {placeholder: 'Search files by name', list: [
      row('f0', {lead: glyph('f0i', 'file-text'), title: 'docs/한글 노트.md', selected: true}),
      row('f1', {lead: glyph('f1i', 'file-code'), title: 'scripts/pen-screens.mjs'}),
    ]});

    return [
      labelled('side-cell', 'Sidebar · the Search icon opens ⌘K', sidebar),
      labelled('default-cell', '⌘K · default', results),
      frame(id('states'), 'States', {layout: 'vertical', gap: '$--spacing-lg'}, [
        labelled('commands-cell', '⌘K · a Workspace on screen, typed', commands),
        labelled('nomatch-cell', '⌘K · no match', noMatch),
        labelled('nothing-cell', '⌘K · nothing to search', nothing),
        labelled('files-cell', '⌘P · same shell', files),
      ]),
    ];
  }
  return screenSheet('screen-palette', 'Screen / Palette', 'web/src/Palette.tsx over Command/CommandDialog (issue #154): the sidebar’s Search icon with its ⌘K hint, the query row with its Esc keycap, results grouped under <project> > AGENTS, WORKSPACE > COMMANDS, WORKSPACES > PROJECTS and WORKSPACES > CHECKOUTS in the order of each group’s best match, two-line rows with the agent’s own mark and its state sentence, ↵ on the selected row, the no-match and nothing-to-search states, and ⌘P on the same shell.', build, build);
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
  // IssueDialogs.tsx (the issue-first Overview): New issue and Start from an
  // issue, each --size-worktree-dialog wide with a body-size label above each
  // control and its note at the label's right (Field), the submit button's
  // ⌘↵ keycap beside it.
  const W = num(tokens, '--size-worktree-dialog');
  const inner = W - 2 * num(tokens, '--spacing-lg');
  const LINE = 18;
  function issueField(id, label, control, aside = []) {
    return frame(id, 'Field', {layout: 'vertical', gap: '$--spacing-xxs', width: inner}, [
      frame(`${id}-lr`, 'Label', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', width: inner}, [
        text(`${id}-l`, label, {fill: '$--subtle-foreground'}),
        ...(aside.length ? [frame(`${id}-sp`, 'Spacer', {width: 'fill_container', height: 1}, []), ...aside] : []),
      ]),
      control,
    ]);
  }
  // The text field's look over `rows` rows; IssueDialogs.tsx draws it on a
  // plain textarea, so it is authored here on local tokens.
  function textArea(id, content, rows) {
    const padX = num(tokens, '--spacing-sm'), padY = num(tokens, '--spacing-xs');
    return frame(id, 'Text area', {layout: 'vertical', width: inner, height: rows * LINE + 2 * padY, padding: [padY, padX], cornerRadius: '$--radius-sm', fill: '$--background', stroke: '$--input', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', clip: true}, [
      text(`${id}-t`, content, {width: inner - 2 * padX}),
    ]);
  }
  function checkbox(id, label, checked) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
      themedXref(`${id}-box`, 'chk-m', label, checked ? {fill: '$--primary', stroke: '$--primary'} : {}, {'chk-i': {enabled: checked}}),
      text(`${id}-t`, label),
    ]);
  }
  function submit(id, label) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
      screenButton(`${id}-b`, label, {height: H}),
      themedXref(`${id}-k`, 'kbd-m', '⌘↵', {}, {'kbd-t': {content: '⌘↵'}}),
    ]);
  }
  function newIssue(id, {where, title, body, start}) {
    return screenDialogSurface(id, {
      width: W, title: '새 이슈',
      body: [
        issueField(`${id}-f-where`, '어디에', screenSelect(`${id}-f-wherev`, {content: where, width: inner})),
        issueField(`${id}-f-title`, '제목', screenInput(`${id}-f-titlev`, {content: title, width: inner})),
        issueField(`${id}-f-body`, '내용', textArea(`${id}-f-bodyv`, body, 4)),
        checkbox(`${id}-f-start`, '만들고 바로 시작', start),
      ],
      actions: [screenButton(`${id}-cancel`, '취소', {variant: 'secondary', height: H}), submit(`${id}-ok`, start ? '만들고 시작…' : '이슈 만들기')],
    });
  }
  function startIssue(id) {
    const agents = frame(`${id}-agentrow`, 'Agents', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center', height: H}, [
      screenRadioItem(`${id}-agent0`, '터미널만', false),
      screenRadioItem(`${id}-agent1`, 'Claude', true),
      screenRadioItem(`${id}-agent2`, 'Codex', false),
    ]);
    return screenDialogSurface(id, {
      width: W, title: '#192 작업 시작', titleGlyph: 'circle-dot', prose: true,
      description: 'hided has no SIGTERM handler: AI children die with the stdin pipe',
      body: [
        issueField(`${id}-name`, '워크트리 · 브랜치', screenInput(`${id}-namev`, {content: '192-hided-sigterm-handler', mono: true, width: inner}), [
          icon(`${id}-name-ai`, 'sparkles', {size: 12, fill: '$--primary'}),
          text(`${id}-name-note`, 'AI 지음 · 고칠 수 있음', {size: '$--text-caption', fill: '$--muted-foreground'}),
        ]),
        frame(`${id}-grid`, 'Base and agent', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'end', width: inner}, [
          frame(`${id}-base`, 'Field', {layout: 'vertical', gap: '$--spacing-xxs', width: 'fill_container'}, [
            text(`${id}-base-l`, '기준', {fill: '$--subtle-foreground'}),
            screenSelect(`${id}-basev`, {content: 'main', width: 180}),
          ]),
          frame(`${id}-agent`, 'Field', {layout: 'vertical', gap: '$--spacing-xxs'}, [
            text(`${id}-agent-l`, '에이전트', {fill: '$--subtle-foreground'}),
            agents,
          ]),
        ]),
        issueField(`${id}-prompt`, '첫 지시', textArea(`${id}-promptv`, 'Issue #192를 해결해줘: hided has no SIGTERM handler: AI children die with the stdin pipe\n\n완료되면 이 이슈를 닫는 PR을 열어줘 (PR 본문에 Closes #192).', 5), [
          text(`${id}-prompt-note`, '이슈 본문에서 채움 · 고칠 수 있음', {size: '$--text-caption', fill: '$--muted-foreground'}),
        ]),
      ],
      actions: [screenButton(`${id}-cancel`, '취소', {variant: 'secondary', height: H}), submit(`${id}-ok`, '시작')],
    });
  }
  function build3(suffix) {
    return [frame(`dlg-row3-${suffix}`, 'Row 3 · issues', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [
      newIssue(`dlg-issue-${suffix}`, {where: 'herdr-ide · GitHub modakbul-gongbang/hide', title: 'Overview: Tasks board as the default tab', body: '이슈 우선 흐름: 백로그 → 진행 중 → 리뷰 → 완료.\n카드는 이슈, 에이전트, PR 순서로 읽는다.', start: false}),
      newIssue(`dlg-issuego-${suffix}`, {where: 'creator · Local', title: '릴스 6탄 대본 초안', body: '인터뷰 녹취에서 핵심 세 문장을 뽑는다.', start: true}),
      startIssue(`dlg-start-${suffix}`),
    ])];
  }
  function full(suffix) {
    return [frame(`dlg-rows-${suffix}`, 'Rows', {layout: 'vertical', gap: '$--spacing-xl'}, [...build(suffix), ...build2(suffix), ...build3(suffix)])];
  }
  return screenSheet('screen-dialogs', 'Screen / Dialogs and Sheets', 'web/src/WorkspaceDialogs.tsx, NewWorkspace.tsx, DraftRecovery.tsx, ShortcutSheet.tsx, IssueDialogs.tsx: every Dialog/AlertDialog surface the shell opens, shaped from System / Dialog and System / Alert Dialog with Button refs for every action. The third row is the issue-first Overview’s: New issue (어디에, 제목, 내용 and 만들고 바로 시작, unchecked and then checked, when the button reads 만들고 시작…) and Start from an issue (#192 작업 시작 over the issue’s title, the worktree name the AI wrote and the operator can edit, 기준, the agent, and 첫 지시 filled from the issue).', full, full);
}

// -- Screen / Menus and Overlays --------------------------------------------------

function buildMenus() {
  function build(suffix) {
    // workspaceManage.ts projectMenu() in a browser tab, which has no Finder
    // (PRD sidebar-context-menus D-02, D-07); Remove project carries no
    // destructive style in the real menu either. The desktop app's menu, with
    // Reveal in Finder, is on Screen / Projects Sidebar.
    const rowMenu = screenMenuContent(`mn-row-${suffix}`, 220, [
      screenMenuItem(`mn-row0-${suffix}`, 'Open Overview'),
      screenMenuItem(`mn-row1-${suffix}`, 'New worktree…'),
      screenMenuItem(`mn-row2-${suffix}`, 'New tab in main', {shortcut: '⌥T'}),
      screenMenuSeparator(`mn-rowsep1-${suffix}`),
      screenMenuItem(`mn-row3-${suffix}`, 'Copy path'),
      screenMenuSeparator(`mn-rowsep2-${suffix}`),
      screenMenuItem(`mn-row4-${suffix}`, 'Pin'),
      screenMenuItem(`mn-row5-${suffix}`, 'Remove project…'),
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
  return screenSheet('screen-menus', 'Screen / Menus and Overlays', 'entry-menu.tsx EntryContextMenu, DevicePicker.tsx, and the Explorer git-status notice: overlays shown anchored in their real screen context rather than the abstract System gallery.', build, build);
}

// -- Screen / Projects Sidebar -------------------------------------------------

// The sidebar's Projects tab (sidebar.tsx, projects.ts), drawn from the library's
// Project row, Checkout row, Sidebar agent row and Inactive Fold Row. The left
// reads: a project's glyph at the row's inset, a checkout's glyph one lineage step
// in, and an opened checkout's agent marks under the checkout name. Every line
// ends the same way: its time or status badge, then its fold slot, so names never
// move and every time, badge and chevron shares one column; a folded chevron is
// drawn, an unfolded one waits for the pointer. A row's menu is a right-click,
// with nothing drawn for it. An opened checkout and its agents share a group fill.
function buildProjectsSidebar(tokens) {
  const width = num(tokens, '--size-sidebar-ideal');
  const xs = num(tokens, '--spacing-xs');
  const sm = num(tokens, '--spacing-sm');
  const row = width - 2 * xs;
  const indent = num(tokens, '--size-lineage-indent');
  const glyphColumn = sm + indent;
  const nameColumn = glyphColumn + num(tokens, '--size-checkout-icon') + sm;
  const markSlot = num(tokens, '--size-agent-mark');
  const STATUS = {working: '$--agent-working', asking: '$--warning', done: '$--success', seen: '$--muted-foreground'};
  const SYMBOL = {working: '●', asking: '?', done: '✓', seen: '○'};
  const KIND = {
    overview: {icon: 'layout-dashboard', fill: '$--subtle-foreground'},
    primary: {icon: 'house', fill: '$--subtle-foreground'},
    branch: {icon: 'git-branch', fill: '$--subtle-foreground'},
    folder: {icon: 'folder', fill: '$--subtle-foreground'},
    open: {icon: 'git-pull-request', fill: '$--pr-open'},
    draft: {icon: 'git-pull-request-draft', fill: '$--pr-draft'},
    missing: {icon: 'git-branch', fill: '$--destructive'},
  };

  // The status badge (status-badge.tsx): one part per mark the agents' rows
  // draw, worst first, zero parts left out; no agents, no badge.
  function status(prefix, marks) {
    const parts = ['error', 'approval', 'question', 'working', 'done', 'idle'];
    const counted = parts.filter(part => (marks?.[part] ?? 0) > 0);
    if (!counted.length) return {[`${prefix}-status`]: {enabled: false}};
    return Object.fromEntries([
      [`${prefix}-status`, {enabled: true}],
      ...parts.map(part => [`${prefix}-${part}`, {enabled: counted.includes(part)}]),
      ...counted.map(part => [`${prefix}-${part}-count`, {content: String(marks[part])}]),
    ]);
  }

  // A project's badge is its checkouts' added up, and it stays while they are open.
  function projectRow(id, {name, marks, expanded, selected = false}) {
    return themedXref(id, 'qdhY0', name, {width: row, height: num(tokens, '--size-project-row'), ...(selected ? {fill: '$--secondary'} : {})}, {
      iBYjj: {icon: expanded ? 'chevron-down' : 'chevron-right'},
      // An unfolded project's chevron waits for the pointer in its slot.
      wgtfo: {opacity: expanded ? 0 : 1},
      mIzlX: {icon: 'folder-git-2'},
      JkPyX: {content: name},
      ...status('project', marks),
    });
  }

  // sidebar-header.tsx's Overview row, the one global destination above the
  // strip: the project row's columns with the fold slot empty, the house glyph,
  // and the project count where a badge stands.
  function overviewDestination(id, {count, selected = false}) {
    return themedXref(id, 'qdhY0', 'Overview', {width: row, height: num(tokens, '--size-project-row'), ...(selected ? {fill: '$--secondary'} : {})}, {
      wgtfo: {opacity: 0},
      mIzlX: {icon: 'house'},
      JkPyX: {content: 'Overview'},
      nZkan: {enabled: true, content: count},
      'project-status': {enabled: false},
    });
  }

  // Line one is the name, 12/400 with its path prefix muted (500 while its
  // Workspace is in front), the badge while the agent rows are folded (always,
  // on a folder, whose badge is its project's) and the fold slot; line two is
  // the purpose (after the parent it was raised from, if any) and the
  // last-commit age, drawn only when one of them exists. A row without either
  // is one line with its age there, and a missing folder has none.
  // Pen draws no ellipsis, so a long name or purpose is written already cut the
  // way the row truncates it.
  function checkoutRow(id, {name, kind = 'branch', age, purpose, raisedFrom, marks, expanded = false, selected = false, hovered = false}) {
    const k = KIND[kind];
    const project = kind === 'folder';
    const agents = Boolean(marks);
    const secondLine = Boolean(purpose || raisedFrom);
    const slash = name.indexOf('/');
    const [prefix, rest] = project || slash <= 0 ? ['', name] : [name.slice(0, slash + 1), name.slice(slash + 1)];
    // Line one starts with the master's mark slot and a gap before the glyph.
    const leading = (project ? sm : glyphColumn) - markSlot - sm;
    const inner = row - xs - leading;
    return themedXref(id, 'DLP27', name, {
      width: row, height: num(tokens, secondLine ? '--size-checkout-row-detailed' : project ? '--size-project-row' : '--size-checkout-row'),
      padding: [0, xs, 0, leading],
      ...(selected ? {fill: '$--secondary'} : hovered ? {fill: '$--accent'} : {}),
    }, {
      QetL0: {width: inner},
      VtSRn: {icon: k.icon, fill: k.fill},
      'checkout-name-prefix': prefix ? {content: prefix, enabled: true} : {enabled: false},
      q91d7: project ? {content: name, fontSize: '$--text-subhead', fontWeight: '600'} : {content: rest, fontWeight: selected ? '500' : 'normal'},
      TS5x9: {enabled: kind === 'missing'},
      ...status('checkout', project || !expanded ? marks : null),
      TuosT: !secondLine && age ? {content: age, enabled: true} : {enabled: false},
      // The fold keeps its slot on a row with no agents, so every age lines up.
      CgZj3: {icon: expanded ? 'chevron-down' : 'chevron-right'},
      IXwZI: {opacity: agents && !expanded ? 1 : 0},
      o6XLUj: {enabled: secondLine, width: inner},
      yfur4: {width: markSlot + sm},
      D37Vr: raisedFrom || purpose ? {content: raisedFrom ? `↰ ${raisedFrom}에서${purpose ? ` · ${purpose}` : ''}` : purpose, enabled: true} : {enabled: false},
      'line2-age': secondLine && age ? {content: age, enabled: true} : {enabled: false},
    });
  }

  // sidebar.tsx's FolderRowView: the project's name and badge on line one, in
  // the checkout row's columns, and the purpose on line two; a folder has no commit age.
  function folderRow(id, options) {
    return checkoutRow(id, {...options, kind: 'folder'});
  }

  // An opened checkout's agent row: its marks under the checkout name, one
  // lineage step further per level, the context already said by the rows above.
  function agentRow(id, {status, depth = 0, ...options}) {
    return screenSidebarAgentRow(id, {symbol: SYMBOL[status], color: STATUS[status], inset: nameColumn + depth * indent, width: row, ...options});
  }

  function foldedAgent(id, options, summaries) {
    return frame(id, 'Folded agent with other checkouts', {layout: 'vertical', width: row}, [
      agentRow(`${id}-row`, options),
      ...summaries.map((summary, index) => screenLineageSummary(tokens, `${id}-summary${index}`, summary)),
    ]);
  }

  // An opened checkout and its agent rows, on one small group fill.
  function group(id, rows) {
    return frame(id, 'Opened checkout', {width: row, layout: 'vertical', padding: ['$--spacing-xs', 0], fill: '$--muted', cornerRadius: '$--radius-sm'}, rows);
  }

  function fold(id, label, level) {
    return themedXref(id, 'inactive-fold-row', label, {width: row, padding: [0, xs, 0, level === 'project' ? sm : glyphColumn]}, {'inactive-fold-label': {content: label}});
  }

  // The library's Section Header, 10/500 in sentence case (D-11), at the
  // list's own inset, with neither chevron nor detail: these sections do not fold.
  function section(id, label) {
    return themedXref(id, 'O79KF', label, {width: row, padding: ['$--spacing-sm', '$--spacing-sm', '$--spacing-xxs', '$--spacing-sm']}, {
      eK6gz: {enabled: false},
      VOd6D: {content: label, fill: '$--muted-foreground'},
      rVsKB: {enabled: false},
      g7fzt: {enabled: false},
    });
  }

  function build(s) {
    // The global destinations, fixed above the strip (D-02, D-06); the count is
    // what the rows below can produce: one pinned, herdr-ide and sasu, and
    // three folded inactive projects.
    const destinations = frame(`psb-dest-${s}`, 'Destinations', {width, layout: 'vertical', padding: xs}, [
      overviewDestination(`psb-all-${s}`, {count: '6 projects'}),
    ]);
    // Projects | Agents, then New workspace (Projects only) and Search at the end (D-03, D-04).
    const strip = frame(`psb-hdr-${s}`, 'Tabs', {width, height: num(tokens, '--size-tab-strip'), padding: [0, xs, 0, '$--spacing-md'], gap: '$--spacing-sm', alignItems: 'center'}, [
      text(`psb-projects-${s}`, 'Projects', {size: '$--text-caption'}),
      text(`psb-agents-${s}`, 'Agents', {size: '$--text-caption', fill: '$--muted-foreground'}),
      frame(`psb-hgap-${s}`, 'Spacer', {width: 'fill_container', height: 1}, []),
      screenIconButton(`psb-new-${s}`, 'plus'),
      screenIconButton(`psb-search-${s}`, 'search'),
    ]);
    const rule = (key) => frame(`psb-${key}-${s}`, 'Rule', {width, height: 1, fill: '$--border'}, []);
    const header = frame(`psb-top-${s}`, 'Top', {width, layout: 'vertical'}, [destinations, rule('rule0'), strip, rule('rule1')]);
    const list = frame(`psb-list-${s}`, 'Projects list', {width, layout: 'vertical', padding: [0, xs]}, [
      section(`psb-sec-pin-${s}`, 'Pinned · 1'),
      folderRow(`psb-p-notes-${s}`, {name: 'team-notes', marks: {idle: 1}, purpose: '회의록 요약 정리'}),
      section(`psb-sec-recent-${s}`, 'Projects · Recent activity · 5'),
      projectRow(`psb-p-herdr-${s}`, {name: 'herdr-ide', marks: {question: 3, working: 5, done: 1, idle: 1}, expanded: true}),
      checkoutRow(`psb-overview-${s}`, {name: 'Overview', kind: 'overview', selected: true}),
      group(`psb-g-main-${s}`, [
        checkoutRow(`psb-c2-${s}`, {name: 'main', kind: 'primary', age: 'now', marks: {question: 2, working: 4, idle: 1}, purpose: '사이드바 가독성 개선', expanded: true}),
        // An unfolded parent: its chevron waits in the slot, its children follow.
        agentRow(`psb-a1-${s}`, {title: '사이드바 가독성 개선', status: 'working', age: '1m', fold: 'unfolded'}),
        agentRow(`psb-a1c1-${s}`, {title: '컴포넌트 구…', status: 'working', provider: 'codex', age: '42s', depth: 1, branch: 'feat/ui'}),
        agentRow(`psb-a1c2-${s}`, {title: '한글 가독성 확인', status: 'seen', age: '38s', depth: 1}),
        // A folded parent waiting on its children: ring in Working, the badge, the chevron shown.
        foldedAgent(`psb-a2-${s}`, {title: '후속 UX 계획 인터뷰', status: 'working', age: '2m', badge: '?1', fold: 'folded'}, [
          {status: 'working', branch: 'agent-sleep', pr: '#183'},
          {status: 'done', branch: 'hcoord-decouple', device: 'mini'},
        ]),
        agentRow(`psb-a3-${s}`, {title: '배포 전 확인', status: 'asking', age: '30s', line: '프로덕션 배포 전에 변경 내용을 확인해…', bright: true}),
      ]),
      checkoutRow(`psb-c3-${s}`, {name: 'quick/155-browser-display', age: '40m', marks: {question: 1}, raisedFrom: 'main', purpose: '#155 browser display (WebCon…'}),
      checkoutRow(`psb-c4-${s}`, {name: 'quick/154-search-palette', kind: 'draft', age: '1h', marks: {done: 1}, raisedFrom: 'main', purpose: '#154 ⌘K search palette UI'}),
      checkoutRow(`psb-c1-${s}`, {name: 'electron-shortcut-bindings', kind: 'open', age: '2h', marks: {working: 1}, purpose: 'Electron desktop host for the we…'}),
      checkoutRow(`psb-c5-${s}`, {name: 'design/workspace-ux-prop…', age: '5h', purpose: 'Workspace UX 제안과 상태 소유…'}),
      checkoutRow(`psb-c6-${s}`, {name: 'fix/registered-projects-only', age: '1d'}),
      checkoutRow(`psb-c7-${s}`, {name: 'legacy-shell', kind: 'missing'}),
      fold(`psb-f-herdr-${s}`, 'Inactive 14', 'checkout'),
      projectRow(`psb-p-sasu-${s}`, {name: 'sasu', marks: {working: 1, done: 1, idle: 1}, expanded: false}),
      fold(`psb-f-proj-${s}`, 'Inactive projects 3', 'project'),
    ]);
    const footer = frame(`psb-ftr-${s}`, 'Footer', {width, layout: 'vertical'}, [
      frame(`psb-rule2-${s}`, 'Rule', {width, height: 1, fill: '$--border'}, []),
      frame(`psb-dev-${s}`, 'Device picker', {width, padding: ['$--spacing-sm', '$--spacing-md'], gap: '$--spacing-xs', alignItems: 'center'}, [
        icon(`psb-devi-${s}`, 'laptop', {size: 12, fill: '$--subtle-foreground'}),
        text(`psb-devt-${s}`, 'This Mac', {size: '$--text-caption', fill: '$--subtle-foreground'}),
        icon(`psb-devc-${s}`, 'chevrons-up-down', {size: 12, fill: '$--muted-foreground'}),
      ]),
    ]);
    return [frame(`psb-sidebar-${s}`, 'Sidebar', {width, layout: 'vertical', fill: '$--sidebar', clip: true}, [header, list, footer]), hoverState(s), menuStates(s)];
  }

  // A row's right-click menu, one per row kind (PRD sidebar-context-menus
  // D-02..D-04, D-10): a cut of the list around the row on the hover wash,
  // and the menu (Component / Menu parts) opened under the pointer. The items,
  // their order and separators are workspaceManage.ts's projectMenu,
  // checkoutMenu and agentMenu as the desktop app draws them.
  function menuStates(s) {
    const I = (id, label, options) => screenMenuItem(`${id}-${s}`, label, options);
    const S = id => screenMenuSeparator(`${id}-${s}`);
    const cut = (id, rows) => frame(`psb-m-${id}-list-${s}`, 'Projects list, cut', {width, layout: 'vertical', padding: [xs, xs], fill: '$--sidebar', cornerRadius: '$--radius-sm', clip: true}, rows);
    const opened = (id, title, rows, menuWidth, items) => frame(`psb-m-${id}-${s}`, title, {layout: 'vertical', gap: 0}, [
      cut(id, rows),
      frame(`psb-m-${id}-at-${s}`, 'Menu under the pointer', {layout: 'horizontal', padding: [0, 0, 0, 3 * num(tokens, '--spacing-lg')]}, [screenMenuContent(`psb-m-${id}-menu-${s}`, menuWidth, items)]),
    ]);
    return frame(`psb-menus-${s}`, 'Row menus', {layout: 'vertical', gap: '$--spacing-lg'}, [
      opened('proj', 'Project row menu', [
        projectRow(`psb-m-proj-row-${s}`, {name: 'herdr-ide', marks: {question: 3, working: 5, done: 1, idle: 1}, expanded: true}),
      ], 220, [
        I('psb-m-proj-0', 'Open Overview'),
        I('psb-m-proj-1', 'New worktree…'),
        I('psb-m-proj-2', 'New tab in main', {shortcut: '⌘T'}),
        S('psb-m-proj-s1'),
        I('psb-m-proj-3', 'Reveal in Finder'),
        I('psb-m-proj-4', 'Copy path'),
        S('psb-m-proj-s2'),
        I('psb-m-proj-5', 'Pin'),
        I('psb-m-proj-6', 'Remove project…'),
      ]),
      opened('co', 'Checkout row menu', [
        checkoutRow(`psb-m-co-row-${s}`, {name: 'electron-shortcut-bindings', kind: 'open', age: '2h', marks: {working: 1}, purpose: 'Electron desktop host for the we…', hovered: true}),
      ], 240, [
        I('psb-m-co-0', 'Open'),
        I('psb-m-co-1', 'New tab here', {shortcut: '⌘T'}),
        I('psb-m-co-2', 'Open pull request #149'),
        S('psb-m-co-s1'),
        I('psb-m-co-3', 'Set purpose…'),
        I('psb-m-co-4', 'Set as default checkout'),
        I('psb-m-co-5', 'Copy branch name'),
        I('psb-m-co-6', 'Copy path'),
        I('psb-m-co-7', 'Reveal in Finder'),
        S('psb-m-co-s2'),
        I('psb-m-co-8', 'Delete worktree…', {state: 'destructive'}),
      ]),
      opened('ag', 'Agent row menu', [
        agentRow(`psb-m-ag-row-${s}`, {title: '배포 전 확인', status: 'asking', age: '30s', line: '프로덕션 배포 전에 변경 내용을 확인해…', bright: true}),
      ], 220, [
        I('psb-m-ag-0', 'Show', {shortcut: '⌥3'}),
        S('psb-m-ag-s1'),
        I('psb-m-ag-1', 'Copy title'),
        I('psb-m-ag-2', 'Copy session id'),
        S('psb-m-ag-s2'),
        I('psb-m-ag-3', 'Close tab…'),
      ]),
    ]);
  }

  // The pull-request row under the pointer with its card beside it (PRD
  // checkout-pr-glyph-card D-03, D-05): a cut of the list around the row, the
  // row on the hover wash, and Component / PR hover card to the right at the
  // row's top, the way the tooltip places it. Every text on the card is the
  // row's own facts; the card's width is the master's.
  function hoverState(s) {
    const rowH = num(tokens, '--size-checkout-row-detailed');
    const projectH = num(tokens, '--size-project-row');
    const plainH = num(tokens, '--size-checkout-row');
    const gap = num(tokens, '--spacing-sm');
    const cardW = num(tokens, '--size-pr-popover');
    const rows = [
      projectRow(`psb-h-p-${s}`, {name: 'herdr-ide', marks: {question: 3, working: 5, done: 1, idle: 1}, expanded: true}),
      checkoutRow(`psb-h-overview-${s}`, {name: 'Overview', kind: 'overview'}),
      checkoutRow(`psb-h-c3-${s}`, {name: 'quick/155-browser-display', age: '40m', marks: {question: 1}, purpose: '#155 browser display (WebCon…'}),
      checkoutRow(`psb-h-c4-${s}`, {name: 'quick/154-search-palette', kind: 'draft', age: '1h', marks: {done: 1}, purpose: '#154 ⌘K search palette UI'}),
      checkoutRow(`psb-h-c1-${s}`, {name: 'electron-shortcut-bindings', kind: 'open', age: '2h', marks: {working: 1}, purpose: 'Electron desktop host for the we…', hovered: true}),
      checkoutRow(`psb-h-c5-${s}`, {name: 'design/workspace-ux-prop…', age: '5h', purpose: 'Workspace UX 제안과 상태 소유…'}),
    ];
    // The hovered row's top: the project row, the Overview row and two detailed rows above it.
    const rowTop = projectH + plainH + 2 * rowH;
    const listH = 2 * xs + projectH + plainH + 4 * rowH;
    const list = frame(`psb-h-list-${s}`, 'Projects list, cut', {width, layout: 'vertical', padding: [xs, xs], fill: '$--sidebar', cornerRadius: '$--radius-sm', clip: true}, rows);
    const card = themedXref(`psb-h-card-${s}`, 'pr-card', 'PR hover card', {x: width + gap, y: xs + rowTop}, {
      'pr-card-badge-label': {content: 'Open', fill: '$--pr-open'},
      'pr-card-number': {content: '#149'},
      'pr-card-title': {content: 'Electron desktop host for the web shell'},
      'pr-card-row-review': {enabled: false},
      'pr-card-checks': {content: 'Passing', fill: '$--success'},
      'pr-card-branch': {content: 'electron-shortcut-bindings'},
      'pr-card-agents-question': {enabled: false},
      'pr-card-agents-idle': {enabled: false},
      'pr-card-agents-working-count': {content: '1'},
      'pr-card-commit': {content: '2h ago'},
      'pr-card-path': {content: '~/projects/herdr-ide.worktrees/electron-shortcut-bindings'},
    });
    return frame(`psb-hover-${s}`, 'Checkout row under the pointer, with its card', {width: width + gap + cardW, height: listH}, [{...list, x: 0, y: 0}, card]);
  }
  return screenSheet('screen-projects-sidebar', 'Screen / Projects Sidebar', 'sidebar.tsx, sidebar-header.tsx, projects.ts: the Projects tab, the scope picker. Above it, fixed, the global destinations: one Overview row with the house glyph and the project count, which opens the every-project Overview and carries the selected fill only while that screen is in front; under it the Projects | Agents tab strip, Projects first, ending in New workspace (Projects only) and Search, the icon that opens ⌘K. The list starts at the first project; the row of the scope the center shows carries the selected fill, here the Overview child under herdr-ide, and a checkout only while its Workspace is in front. A project’s primary checkout comes first, then the rest by activity. Every line ends in its time or status badge and then a fold slot, so names never move and the times, badges and chevrons stand in one column each; a folded chevron is drawn and an unfolded one waits for the pointer. A row’s menu opens on a right-click, with nothing drawn for it; beside the sidebar each row kind is drawn with its menu open (Project: Open Overview, New worktree…, New tab in main, Reveal in Finder, Copy path, Pin, Remove project…; Checkout: Open, New tab here, Open pull request, Set purpose…, Set as default checkout, Copy branch name, Copy path, Reveal in Finder, Delete worktree…; Agent: Show, Copy title, Copy session id, Close tab…). A status badge counts agents under the mark each agent’s own row draws, worst first (× ! ? ● ✓ ○). A project row opens the Overview and wears its checkouts’ badges added up, open or folded, and no time. The first row under an expanded Git project is Overview, an instance of the checkout row with a layout-dashboard glyph and empty trailing slots. A checkout row opens the checkout and unfolds its agents; clicking its already selected, unfolded Workspace folds them without leaving it. The chevron changes disclosure alone. While its agent rows are folded its badge ends line one, and its chevron, there only while agents run, opens them on one group fill in place of the badge. A checkout name is 12/400 with its prefix up to the first slash muted. Line two is the purpose with the last-commit age on the time column, drawn only for a purpose or a raised-from parent; a checkout without either is one line with its age there. A parent agent folds its children with the same chevron and speaks for them with its badge. A folded parent keeps its own-checkout descendants in the badge and draws one C-style line per other checkout, with the R1 server-glyph device chip. A checkout raised by an external root prefixes line two with the parent checkout. The kind glyph is the pull request’s lifecycle when GitHub knows one, else folder, primary, detached or branch; a missing folder is danger with no age. Beside the sidebar: a pull-request row under the pointer with its card (Component / PR hover card) opened to its right, the glyph a button that opens the pull request.', build, build);
}

// -- Screen / Mobile ---------------------------------------------------------------

// The mobile companion (PRD mobile-companion D-08, D-12): Settings > Mobile on the
// desktop, blocked on a failing check and ready with its QR, and the phone app's
// pairing, list, detail, unreachable and empty states and its push banner. Drawn
// as the operator-approved board drew it: plain nodes on this file's local tokens,
// because no library master draws a phone screen, so every size is a number this
// builder owns. The machine name and tailnet are placeholders, and the QR is a
// deterministic pattern, never a real code.
function buildMobile(tokens) {
  const XS = num(tokens, '--spacing-xs'), SM = num(tokens, '--spacing-sm'), MD = num(tokens, '--spacing-md');
  const LG = num(tokens, '--spacing-lg'), XL = num(tokens, '--spacing-xl');
  const SP = {xs: '$--spacing-xs', sm: '$--spacing-sm', md: '$--spacing-md', lg: '$--spacing-lg', xl: '$--spacing-xl'};
  const FG = '$--foreground', MUTED = '$--muted-foreground', SUBTLE = '$--subtle-foreground';
  const WARN = '$--warning', OK = '$--success', WORK = '$--agent-working', BAD = '$--destructive';
  const PILL = 999;
  const PHONE_W = 390, PHONE_H = 800, STATUS_H = 44, HOME_H = 24;
  const CONTENT_W = PHONE_W - 2 * LG;
  const SET_W = num(tokens, '--size-settings-sheet-w');
  const MACHINE = 'my-mac';
  const URL = `https://${MACHINE}.tailnet-name.ts.net`;

  // Pen has no ellipsis: cut a string to a measured width.
  function measure(s, size, mono = false) {
    let w = 0;
    for (const ch of s) {
      const c = ch.codePointAt(0);
      if ((c >= 0xac00 && c <= 0xd7a3) || (c >= 0x3130 && c <= 0x318f)) w += size * 0.93;
      else if (mono) w += size * 0.6;
      else if (ch === ' ') w += size * 0.28;
      else if (/[A-Z#@%MW]/.test(ch)) w += size * 0.68;
      else if (/[il.,:;'|!]/.test(ch)) w += size * 0.28;
      else w += size * 0.55;
    }
    return w;
  }
  function fit(s, max, size, mono = false) {
    if (!s || measure(s, size, mono) <= max) return s;
    let cut = s;
    while (cut.length && measure(`${cut}…`, size, mono) > max) cut = cut.slice(0, -1);
    return `${cut.trimEnd()}…`;
  }

  // -- atoms --
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);
  const rule = (id, width) => ({type: 'line', id, name: 'Rule', width, height: 0, stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'center'});
  const STATUS = {working: ['●', WORK], done: ['✓', OK], seen: ['○', SUBTLE], question: ['?', WARN], approval: ['!', WARN], error: ['×', BAD]};
  function mark(id, status, size = 14) {
    const [glyph, fill] = STATUS[status];
    const box = size + 2;
    const shell = children => frame(id, 'Mark', {width: box, height: box, alignItems: 'center', justifyContent: 'center'}, children);
    if (glyph === '●') return shell([frame(`${id}-d`, 'Dot', {width: 8, height: 8, cornerRadius: PILL, fill}, [])]);
    if (glyph === '○') return shell([frame(`${id}-r`, 'Ring', {width: 8, height: 8, cornerRadius: PILL, stroke: fill, strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [])]);
    return shell([text(`${id}-g`, glyph, {size, fill, weight: '600', mono: true})]);
  }
  function provider(id, kind, size = 14) {
    return frame(id, 'Provider', {width: size, height: size, cornerRadius: 3, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${kind}.png`, mode: 'fit'}}, []);
  }
  function chip(id, label, {glyph, fill = '$--secondary', color = MUTED} = {}) {
    return frame(id, 'Chip', {layout: 'horizontal', gap: SP.xs, alignItems: 'center', padding: [1, 6], fill, cornerRadius: PILL, height: 18}, [
      ...(glyph ? [icon(`${id}-i`, glyph, {size: 11, fill: color})] : []),
      text(`${id}-t`, label, {size: '$--text-caption', fill: color}),
    ]);
  }
  function button(id, label, {primary = false, width, height = 40, size = 15} = {}) {
    return frame(id, label, {layout: 'horizontal', alignItems: 'center', justifyContent: 'center', height, ...(width ? {width} : {padding: [0, SP.lg]}), fill: primary ? '$--primary' : '$--secondary', cornerRadius: '$--radius-lg'}, [
      text(`${id}-t`, label, {size, weight: '600', fill: primary ? '$--primary-foreground' : FG}),
    ]);
  }

  // -- phone chrome --
  function phone(id, name, body) {
    return frame(id, name, {layout: 'vertical', width: PHONE_W, height: PHONE_H, fill: '$--background', cornerRadius: 40, clip: true, stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      frame(`${id}-sb`, 'Status bar', {layout: 'horizontal', width: PHONE_W, height: STATUS_H, padding: [0, SP.xl], alignItems: 'center'}, [
        text(`${id}-sb-t`, '9:41', {size: 15, weight: '600', fill: FG}),
        spacer(`${id}-sb-s`),
        text(`${id}-sb-r`, '●●● ▲ ▮', {size: '$--text-caption', fill: FG}),
      ]),
      frame(`${id}-body`, 'Body', {layout: 'vertical', width: PHONE_W, height: PHONE_H - STATUS_H - HOME_H, clip: true}, body),
      frame(`${id}-hb`, 'Home', {width: PHONE_W, height: HOME_H, alignItems: 'center', justifyContent: 'center'}, [
        frame(`${id}-hb-b`, 'Bar', {width: 134, height: 5, cornerRadius: PILL, fill: FG, opacity: 0.6}, []),
      ]),
    ]);
  }
  function header(id, {title, sub, back = null, connected = true, trailing = null}) {
    const titleRow = frame(`${id}-r`, 'Title row', {layout: 'horizontal', gap: SP.sm, alignItems: 'center', width: CONTENT_W, height: 32}, [
      ...(back ? [frame(`${id}-bk`, 'Back', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', height: 32}, [icon(`${id}-bi`, 'arrow-left', {size: 18, fill: FG}), text(`${id}-bt`, back, {size: 15, fill: FG})])] : []),
      ...(back ? [] : [text(`${id}-t`, title, {size: 22, weight: '600', fill: FG})]),
      spacer(`${id}-s`),
      ...(trailing ? [trailing] : []),
      ...(back ? [] : [frame(`${id}-dot`, 'Connection', {width: 8, height: 8, cornerRadius: PILL, fill: connected ? OK : MUTED}, [])]),
    ]);
    const subRow = sub ? [frame(`${id}-sub`, 'Sub', {layout: 'horizontal', gap: SP.xs, alignItems: 'center', width: CONTENT_W, height: 18}, [
      icon(`${id}-si`, 'laptop', {size: 12, fill: MUTED}),
      text(`${id}-st`, sub, {size: '$--text-body', fill: MUTED}),
    ])] : [];
    return frame(id, 'Header', {layout: 'vertical', gap: '$--spacing-xxs', width: PHONE_W, padding: [SP.sm, SP.lg, SP.md, SP.lg]}, [titleRow, ...subRow]);
  }

  // -- phone: list --
  const INSET = 16 + XS + 14 + XS;
  function sectionLabel(id, label, count) {
    return frame(id, label, {layout: 'horizontal', gap: SP.sm, alignItems: 'center', width: PHONE_W, height: 32, padding: [SP.md, SP.lg, 2, SP.lg]}, [
      text(`${id}-t`, label, {size: '$--text-body', weight: '500', fill: MUTED}),
      text(`${id}-c`, String(count), {size: '$--text-body', fill: MUTED, mono: true}),
    ]);
  }
  function agentRow(id, {title, status, kind = 'claude', project, branch, machine = null, age, request = null, news = null, dim = false}) {
    const attention = status === 'question' || status === 'approval' || status === 'error';
    const titleW = CONTENT_W - 16 - XS - 14 - SM - 36 - SM - 16;
    const line2 = request ?? news ?? null;
    return frame(id, title, {layout: 'vertical', gap: 3, width: PHONE_W, height: line2 ? 82 : 64, padding: [SP.sm, SP.lg, SP.sm, SP.lg]}, [
      frame(`${id}-l1`, 'Line 1', {layout: 'horizontal', gap: SP.xs, alignItems: 'center', width: CONTENT_W, height: 22}, [
        mark(`${id}-m`, status),
        provider(`${id}-p`, kind),
        frame(`${id}-tw`, 'Title', {width: titleW, height: 22, alignItems: 'center', clip: true, padding: [0, 0, 0, SP.xs]}, [
          text(`${id}-t`, fit(title, titleW, 15), {size: 15, weight: attention ? '600' : dim ? '400' : '500', fill: dim ? MUTED : attention ? FG : SUBTLE}),
        ]),
        spacer(`${id}-s`),
        text(`${id}-a`, age, {size: '$--text-body', fill: MUTED, mono: true}),
        icon(`${id}-c`, 'chevron-right', {size: 16, fill: MUTED}),
      ]),
      frame(`${id}-l2`, 'Line 2', {layout: 'horizontal', gap: SP.sm, alignItems: 'center', width: CONTENT_W, height: 18, padding: [0, 0, 0, INSET]}, [
        text(`${id}-pb`, fit(`${project} · ${branch}`, CONTENT_W - 120, 12), {size: '$--text-body', fill: MUTED}),
        ...(machine ? [chip(`${id}-mc`, machine, {glyph: 'server'})] : []),
      ]),
      ...(line2 ? [frame(`${id}-l3`, 'Line 3', {layout: 'horizontal', width: CONTENT_W, height: 18, padding: [0, 0, 0, INSET], alignItems: 'center'}, [
        text(`${id}-rq`, fit(line2, CONTENT_W - 40, 13), {size: '$--text-subhead', fill: request ? WARN : FG}),
      ])] : []),
      spacer(`${id}-sp`),
      rule(`${id}-rule`, CONTENT_W),
    ]);
  }
  const AGENTS = {
    needs: [
      {title: '솔루션 6 구현', status: 'approval', project: 'herdr-ide', branch: 'prd/mobile-companion', age: '2m', request: 'Bash(cargo test -p hided) 실행을 허용할까요?'},
      {title: 'hcoord 플러그인 구현', status: 'question', kind: 'codex', project: 'herdr-ide', branch: 'prd/hcoord-plugin', machine: 'mini', age: '9m', request: '워크스페이스 이름을 어떤 걸로 할까요?'},
    ],
    done: [
      {title: '사이드바 행 클릭으로 펼치기', status: 'done', project: 'herdr-ide', branch: 'prd/sidebar-row-click-unfold', age: '14m', news: 'PR #186 열림 · CI 통과'},
    ],
    working: [
      {title: '리뷰 반영', status: 'working', project: 'herdr-ide', branch: 'fix/hook-probe', age: '5m'},
      {title: '세션 초기화 작업 대기', status: 'working', kind: 'codex', project: 'contong', branch: 'main', machine: 'mini', age: '21m'},
      {title: 'Agent tab group 시각화 검토', status: 'working', project: 'herdr-ide', branch: 'prd/agent-tab-groups', age: '38m'},
    ],
    seen: [
      {title: '코덱스 데이터 구조 조사', status: 'seen', project: 'herdr-ide', branch: 'main', age: '1h', dim: true},
      {title: 'tab-names 구현 상태 점검', status: 'seen', project: 'herdr-ide', branch: 'main', age: '2h', dim: true},
      {title: '체크아웃 capability 구현', status: 'seen', project: 'herdr-ide', branch: 'prd/checkout-capability', age: '3h', dim: true},
    ],
  };
  function listBody(id, {unreachable = false, empty = false} = {}) {
    const groups = empty ? [] : [
      sectionLabel(`${id}-g1`, '내 확인 대기', AGENTS.needs.length), ...AGENTS.needs.map((a, i) => agentRow(`${id}-n${i}`, a)),
      sectionLabel(`${id}-g2`, '끝', AGENTS.done.length), ...AGENTS.done.map((a, i) => agentRow(`${id}-d${i}`, a)),
      sectionLabel(`${id}-g3`, '진행 중', AGENTS.working.length), ...AGENTS.working.map((a, i) => agentRow(`${id}-w${i}`, a)),
      sectionLabel(`${id}-g4`, '확인함', AGENTS.seen.length), ...AGENTS.seen.map((a, i) => agentRow(`${id}-s${i}`, a)),
    ];
    const banner = unreachable ? [frame(`${id}-un`, 'Unreachable', {layout: 'horizontal', gap: SP.sm, alignItems: 'center', width: PHONE_W, padding: [SP.sm, SP.lg, SP.sm, SP.lg], fill: '$--muted'}, [
      icon(`${id}-ui`, 'loader-circle', {size: 14, fill: MUTED}),
      text(`${id}-ut`, '연결 안 됨 · 맥의 hide가 꺼져 있거나 폰의 Tailscale이 꺼져 있어요. 다시 시도 중', {size: '$--text-body', fill: MUTED, width: CONTENT_W - 22}),
    ])] : [];
    const emptyLine = empty ? [frame(`${id}-em`, 'Empty', {width: PHONE_W, padding: [SP.xl, SP.lg]}, [text(`${id}-et`, '실행 중인 에이전트가 없어요', {size: '$--text-title', fill: MUTED})])] : [];
    return [
      header(`${id}-h`, {title: 'hide', sub: `${MACHINE} · 폰 1대 더 연결됨`, connected: !unreachable}),
      ...banner,
      frame(`${id}-list`, 'List', {layout: 'vertical', width: PHONE_W, opacity: unreachable ? 0.45 : 1}, [...groups, ...emptyLine]),
    ];
  }

  // -- phone: detail --
  const SCROLLBACK = [
    ['● 페어링 코드는 hided 상태 파일에 0600으로 두고, 코드 교환은 /ws의', FG],
    ['  첫 프레임으로 받겠습니다. 먼저 테스트를 돌립니다.', FG],
    ['', MUTED],
    ['● Bash(cargo test -p hided)', SUBTLE],
    ['  ⎿  Running…', MUTED],
    ['', MUTED],
    ['$ cargo test -p hided', SUBTLE],
    ['   Compiling hided v0.1.0 (/Users/example/projects/herdr-ide/hided)', MUTED],
    ['    Finished test [unoptimized] target(s) in 41.2s', MUTED],
    ['test server::tests::token_compare_needs_the_whole_token ... ok', MUTED],
    ['test pane_auth::tests::sweep_drops_unregistered ... ok', MUTED],
    ['test pane_auth::tests::phone_credential_refuses_file_open ... ok', MUTED],
    ['test mobile::tests::pairing_code_is_single_use ... ok', MUTED],
    ['test mobile::tests::pairing_code_expires_after_five_minutes ... ok', MUTED],
    ['test result: ok. 128 passed; 0 failed', OK],
    ['', MUTED],
    ['● hided의 페어링 코드 검사를 구현했습니다. 다음으로 7일 미접속', FG],
    ['  해지 스윕을 붙이겠습니다.', FG],
    ['', MUTED],
    ['╭─────────────────────────────────────────────╮', MUTED],
    ['│ Bash command                                │', FG],
    ['│   cargo test -p hided -- --include-ignored  │', FG],
    ['│                                             │', MUTED],
    ['│ Do you want to proceed?                     │', FG],
    ['│ ❯ 1. Yes                                    │', WARN],
    ['│   2. Yes, and don\'t ask again this session  │', FG],
    ['│   3. No, and tell Claude what to do         │', FG],
    ['╰─────────────────────────────────────────────╯', MUTED],
  ];
  function scrollback(id, height) {
    return frame(id, 'Scrollback', {layout: 'vertical', gap: 0, width: PHONE_W, height, padding: [SP.sm, SP.lg, SP.sm, SP.lg], fill: '$--card', clip: true, alignItems: 'start', justifyContent: 'end'}, SCROLLBACK.map(([s, fill], i) =>
      frame(`${id}-l${i}`, 'Line', {width: CONTENT_W, height: 18, alignItems: 'center', clip: true}, [text(`${id}-t${i}`, s === '' ? ' ' : fit(s, CONTENT_W, 11, true), {size: '$--text-caption', fill, mono: true})])));
  }
  function quickKeys(id) {
    const keys = ['⏎', 'Esc', '↑', '↓', '^C'];
    const w = (CONTENT_W - 4 * SM) / keys.length;
    return frame(id, 'Quick keys', {layout: 'horizontal', gap: SP.sm, width: PHONE_W, padding: [SP.sm, SP.lg, 0, SP.lg]}, keys.map((k, i) =>
      frame(`${id}-k${i}`, k, {width: w, height: 34, fill: '$--secondary', cornerRadius: '$--radius-md', alignItems: 'center', justifyContent: 'center'}, [text(`${id}-kt${i}`, k, {size: '$--text-title', weight: '600', fill: FG, mono: true})])));
  }
  function replyBar(id) {
    return frame(id, 'Reply bar', {layout: 'horizontal', gap: SP.sm, alignItems: 'center', width: PHONE_W, padding: [SP.sm, SP.lg, SP.md, SP.lg]}, [
      frame(`${id}-in`, 'Input', {layout: 'horizontal', alignItems: 'center', width: CONTENT_W - SM - 76, height: 40, padding: [0, SP.md], fill: '$--secondary', cornerRadius: 12}, [
        text(`${id}-ph`, '답장…', {size: 15, fill: MUTED}),
      ]),
      button(`${id}-send`, '보내기', {primary: true, width: 76}),
    ]);
  }
  function detailBody(id, a) {
    const headH = 32 + 2 + 18 + SM + MD;
    const bodyH = PHONE_H - STATUS_H - HOME_H;
    const sbH = bodyH - headH - 22 - (SM + 34) - (SM + 40 + MD);
    return [
      header(`${id}-h`, {back: '목록', title: a.title, trailing: text(`${id}-age`, a.age, {size: '$--text-body', fill: MUTED, mono: true})}),
      frame(`${id}-ar`, 'Agent', {layout: 'vertical', gap: '$--spacing-xxs', width: PHONE_W, padding: [0, SP.lg, SP.sm, SP.lg]}, [
        frame(`${id}-ar1`, 'Line 1', {layout: 'horizontal', gap: SP.xs, alignItems: 'center', width: CONTENT_W, height: 22}, [
          mark(`${id}-m`, a.status), provider(`${id}-p`, a.kind ?? 'claude'),
          text(`${id}-t`, a.title, {size: 17, weight: '600', fill: FG}),
        ]),
        frame(`${id}-ar2`, 'Line 2', {layout: 'horizontal', gap: SP.sm, alignItems: 'center', width: CONTENT_W, height: 18, padding: [0, 0, 0, INSET]}, [
          text(`${id}-pb`, `${a.project} · ${a.branch}`, {size: '$--text-body', fill: MUTED}),
          ...(a.machine ? [chip(`${id}-mc`, a.machine, {glyph: 'server'})] : []),
        ]),
      ]),
      scrollback(`${id}-sb`, sbH),
      quickKeys(`${id}-qk`),
      replyBar(`${id}-rb`),
    ];
  }

  // -- phone: pairing --
  function pairBody(id) {
    return [
      frame(`${id}-c`, 'Pairing', {layout: 'vertical', gap: SP.lg, alignItems: 'center', width: PHONE_W, height: PHONE_H - STATUS_H - HOME_H, padding: [120, SP.lg, 0, SP.lg]}, [
        frame(`${id}-logo`, 'Logo', {width: 64, height: 64, cornerRadius: '$--radius-xl', fill: '$--primary', alignItems: 'center', justifyContent: 'center'}, [text(`${id}-lt`, 'h', {size: 34, weight: '700', fill: '$--primary-foreground'})]),
        text(`${id}-t`, `${MACHINE}과 연결`, {size: 22, weight: '600', fill: FG}),
        text(`${id}-d`, '이 폰에서 hide의 에이전트를 보고, 기다리는 에이전트에 답할 수 있어요.', {size: '$--text-title', fill: MUTED, width: CONTENT_W - 32, align: 'center'}),
        frame(`${id}-sp`, 'Gap', {height: LG, width: 1}, []),
        button(`${id}-ok`, '연결', {primary: true, width: CONTENT_W, height: 48, size: 16}),
        text(`${id}-n`, '코드는 5분 안에 만료돼요. 만료되면 맥에서 QR을 다시 여세요.', {size: '$--text-body', fill: MUTED, width: CONTENT_W - 32, align: 'center'}),
      ]),
    ];
  }

  // -- push banner --
  function pushBanner(id) {
    const W = 360;
    return frame(id, 'Push banner', {layout: 'horizontal', gap: SP.md, alignItems: 'center', width: W, padding: [SP.md, SP.lg], fill: '$--popover', cornerRadius: 22, stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      frame(`${id}-ic`, 'App icon', {width: 38, height: 38, cornerRadius: 9, fill: '$--primary', alignItems: 'center', justifyContent: 'center'}, [text(`${id}-it`, 'h', {size: 22, weight: '700', fill: '$--primary-foreground'})]),
      frame(`${id}-tx`, 'Text', {layout: 'vertical', gap: 1, width: W - 2 * LG - 38 - MD}, [
        frame(`${id}-r1`, 'Row', {layout: 'horizontal', width: 'fill_container', alignItems: 'center'}, [text(`${id}-app`, 'hide', {size: '$--text-subhead', weight: '600', fill: FG}), spacer(`${id}-s`), text(`${id}-when`, '지금', {size: '$--text-body', fill: MUTED})]),
        text(`${id}-ti`, '솔루션 6 구현', {size: '$--text-title', weight: '600', fill: FG}),
        text(`${id}-bo`, '내 확인 대기 · herdr-ide', {size: '$--text-subhead', fill: SUBTLE}),
      ]),
    ]);
  }

  // -- Settings > Mobile --
  const ROW_W = SET_W - 2 * XL;
  // Deterministic pseudo-QR: three finder patterns and a seeded module field, drawn
  // as rows of cells. Black on white in both themes, as a camera needs it.
  function qr(id, size = 168) {
    const n = 25, cell = Math.floor((size - 16) / n), pad = (size - cell * n) / 2;
    const finder = (r, c) => (r >= 0 && r < 7 && c >= 0 && c < 7) && (r === 0 || r === 6 || c === 0 || c === 6 || (r >= 2 && r <= 4 && c >= 2 && c <= 4));
    let seed = 7;
    const rnd = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed / 0x7fffffff; };
    const rows = [];
    for (let r = 0; r < n; r++) {
      const cells = [];
      for (let c = 0; c < n; c++) {
        let on;
        if (r < 7 && c < 7) on = finder(r, c);
        else if (r < 7 && c >= n - 7) on = finder(r, c - (n - 7));
        else if (r >= n - 7 && c < 7) on = finder(r - (n - 7), c);
        else if ((r < 8 && c < 8) || (r < 8 && c >= n - 8) || (r >= n - 8 && c < 8)) on = false;
        else on = rnd() < 0.45;
        cells.push(frame(`${id}-c${r}-${c}`, 'm', {width: cell, height: cell, fill: on ? '#101112' : '#FFFFFF'}, []));
      }
      rows.push(frame(`${id}-r${r}`, 'row', {layout: 'horizontal', gap: 0, width: cell * n, height: cell}, cells));
    }
    return frame(id, 'QR', {layout: 'vertical', gap: 0, width: size, height: size, padding: pad, fill: '#FFFFFF', cornerRadius: '$--radius-md'}, rows);
  }
  // settings.ts SETTINGS_TABS with Mobile after Devices (PRD mobile-companion B1).
  const TABS = ['General', 'Appearance', 'Agents', 'Devices', 'Mobile', 'Performance', 'Shortcuts'];
  function tabStrip(id) {
    return frame(`${id}-w`, 'Tabs', {layout: 'vertical', gap: 0, width: SET_W}, [
      frame(id, 'Tab strip', {layout: 'horizontal', gap: SP.xs, width: SET_W, padding: [SP.sm, SP.xl], fill: '$--sidebar'}, TABS.map((t, i) =>
        frame(`${id}-t${i}`, t, {padding: [4, 10], cornerRadius: '$--radius-sm', ...(t === 'Mobile' ? {fill: '$--secondary'} : {})}, [text(`${id}-tt${i}`, t, {size: '$--text-body', weight: t === 'Mobile' ? '600' : '400', fill: t === 'Mobile' ? FG : MUTED})]))),
      rule(`${id}-rule`, SET_W),
    ]);
  }
  function groupTitle(id, label) {
    return frame(id, label, {width: ROW_W, padding: [SP.lg, 0, SP.xs, 0]}, [text(`${id}-t`, label, {size: '$--text-caption', weight: '500', fill: MUTED})]);
  }
  function row(id, {label, detail = null, trailing = null, leading = null, tone = FG}) {
    return frame(`${id}-w`, label, {layout: 'vertical', gap: 0, width: ROW_W}, [
      frame(id, label, {layout: 'horizontal', gap: SP.md, alignItems: 'center', width: ROW_W, padding: [SP.sm, 0]}, [
        ...(leading ? [leading] : []),
        frame(`${id}-tx`, 'Text', {layout: 'vertical', gap: '$--spacing-xxs', width: 'fill_container'}, [
          text(`${id}-l`, label, {size: '$--text-body', fill: tone}),
          ...(detail ? [text(`${id}-d`, detail, {size: '$--text-caption', fill: MUTED, width: ROW_W - 160})] : []),
        ]),
        ...(trailing ? [trailing] : []),
      ]),
      rule(`${id}-rule`, ROW_W),
    ]);
  }
  function toggle(id, on) {
    return frame(id, 'Switch', {width: 32, height: 18, cornerRadius: PILL, fill: on ? '$--primary' : '$--secondary', padding: 2, layout: 'horizontal', justifyContent: on ? 'end' : 'start', alignItems: 'center'}, [
      frame(`${id}-k`, 'Knob', {width: 14, height: 14, cornerRadius: PILL, fill: on ? '$--primary-foreground' : MUTED}, []),
    ]);
  }
  function radio(id, on) {
    return frame(id, 'Radio', {width: 14, height: 14, cornerRadius: PILL, stroke: on ? '$--primary' : '$--border', strokeWidth: on ? 4 : 1, strokeAlignment: 'inner', fill: '$--background'}, []);
  }
  function linkButton(id, label, danger = false) {
    return frame(id, label, {padding: [4, 10], cornerRadius: '$--radius-sm', fill: '$--secondary'}, [text(`${id}-t`, label, {size: '$--text-body', fill: danger ? BAD : FG})]);
  }
  function checkItem(id, label, state, action = null) {
    const glyph = state === 'ok' ? 'check' : state === 'fail' ? 'triangle-alert' : 'circle-alert';
    const fill = state === 'ok' ? OK : state === 'fail' ? WARN : MUTED;
    const trailing = action ? frame(`${id}-a`, 'Action', {layout: 'horizontal', gap: SP.xs, alignItems: 'center'}, [text(`${id}-at`, action, {size: '$--text-body', fill: '$--primary'}), icon(`${id}-ai`, 'external-link', {size: 12, fill: '$--primary'})]) : null;
    const detail = state === 'fail' ? 'Tailscale 관리 콘솔 › DNS에서 MagicDNS와 HTTPS Certificates를 켜세요. 켜면 여기서 바로 이어집니다.' : null;
    return row(id, {label, tone: state === 'todo' ? MUTED : FG, leading: icon(`${id}-i`, glyph, {size: 14, fill}), trailing, detail});
  }
  function settings(id, {passing}) {
    const content = [
      groupTitle(`${id}-g0`, '모바일'),
      row(`${id}-on`, {label: '폰에서 hide 열기', detail: '맥의 Tailscale로 이 hide를 tailnet 안에서만 엽니다. hide가 tailscale serve를 켜고, 끄면 자기가 만든 항목만 지웁니다.', trailing: toggle(`${id}-sw`, true)}),
      checkItem(`${id}-c1`, '맥에 Tailscale 설치됨', 'ok'),
      checkItem(`${id}-c2`, `Tailscale에 로그인됨 · ${MACHINE}`, 'ok'),
      checkItem(`${id}-c3`, passing ? 'tailnet에 MagicDNS와 HTTPS 켜짐' : 'tailnet에 HTTPS가 꺼져 있어요', passing ? 'ok' : 'fail', passing ? null : '관리 콘솔 열기'),
      checkItem(`${id}-c4`, '폰에도 Tailscale 앱을 설치하고 같은 계정으로 로그인', passing ? 'ok' : 'todo'),
    ];
    const qrBlock = passing ? [
      frame(`${id}-qrw`, 'QR row', {layout: 'horizontal', gap: SP.xl, alignItems: 'center', width: ROW_W, padding: [SP.lg, 0]}, [
        qr(`${id}-qr`),
        frame(`${id}-qt`, 'QR text', {layout: 'vertical', gap: SP.sm, width: ROW_W - 168 - XL}, [
          text(`${id}-q1`, '폰 카메라로 찍으세요', {size: '$--text-title', weight: '600', fill: FG}),
          text(`${id}-q2`, '열리는 페이지에서 연결을 누르고, 공유 › 홈 화면에 추가로 앱처럼 두세요.', {size: '$--text-body', fill: MUTED, width: ROW_W - 168 - XL}),
          text(`${id}-q3`, URL, {size: '$--text-caption', fill: SUBTLE, mono: true}),
          frame(`${id}-qx`, 'Expiry', {layout: 'horizontal', gap: SP.sm, alignItems: 'center'}, [text(`${id}-q4`, '코드는 4:38 후 만료', {size: '$--text-caption', fill: MUTED, mono: true}), linkButton(`${id}-qn`, '새 코드')]),
        ]),
      ]),
    ] : [
      frame(`${id}-noqr`, 'No QR', {layout: 'horizontal', gap: SP.sm, alignItems: 'center', width: ROW_W, padding: [SP.lg, 0]}, [
        icon(`${id}-nqi`, 'circle-alert', {size: 14, fill: MUTED}),
        text(`${id}-nqt`, '위 항목이 모두 통과하면 QR이 여기 나타납니다.', {size: '$--text-body', fill: MUTED}),
      ]),
    ];
    const phones = passing ? [
      groupTitle(`${id}-g1`, '연결된 폰 · 2 / 4'),
      row(`${id}-p1`, {label: 'iPhone 15 Pro', detail: '방금 · 알림 받는 중', trailing: linkButton(`${id}-p1r`, '해지', true)}),
      row(`${id}-p2`, {label: 'iPad', detail: '3일 전 · 4일 뒤 자동 해지', trailing: linkButton(`${id}-p2r`, '해지', true)}),
      groupTitle(`${id}-g2`, '푸시 알림'),
      row(`${id}-r0`, {label: '끔', leading: radio(`${id}-r0b`, false)}),
      row(`${id}-r1`, {label: '앱이 닫혀 있을 때만', detail: '데스크톱 hide가 연결돼 있지 않은 동안만 폰으로 보냅니다. 내 확인 대기와 끝 두 전이에서만.', leading: radio(`${id}-r1b`, true)}),
      row(`${id}-r2`, {label: '항상', leading: radio(`${id}-r2b`, false)}),
    ] : [];
    return frame(id, passing ? 'Settings · Mobile · ready' : 'Settings · Mobile · blocked', {layout: 'vertical', width: SET_W, fill: '$--background', cornerRadius: 12, clip: true, stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      frame(`${id}-ttl`, 'Sheet title', {layout: 'horizontal', alignItems: 'center', width: SET_W, height: 44, padding: [0, SP.xl], fill: '$--sidebar'}, [text(`${id}-tt`, 'Settings', {size: '$--text-title', weight: '600', fill: FG}), spacer(`${id}-ts`), icon(`${id}-tx`, 'x', {size: 14, fill: MUTED})]),
      tabStrip(`${id}-tabs`),
      frame(`${id}-body`, 'Body', {layout: 'vertical', width: SET_W, padding: [0, SP.xl, SP.xl, SP.xl]}, [...content, ...qrBlock, ...phones]),
    ]);
  }

  // -- sheet --
  function labeled(id, label, note, node) {
    return frame(id, label, {layout: 'vertical', gap: SP.sm, alignItems: 'start'}, [
      text(`${id}-l`, label, {size: '$--text-title', weight: '600', fill: FG}),
      text(`${id}-n`, note, {size: '$--text-caption', fill: MUTED, width: node.width ?? PHONE_W}),
      node,
    ]);
  }
  function build(s) {
    const p = key => `mob-${key}-${s}`;
    const needs0 = AGENTS.needs[0];
    const rowFrame = (key, name, children) => frame(p(key), name, {layout: 'horizontal', gap: SP.xl, alignItems: 'start'}, children);
    return [frame(p('wrap'), 'Wrap', {layout: 'vertical', gap: SP.xl}, [
      rowFrame('row-settings', 'Settings', [
        labeled(p('cap-blocked'), '설정 › Mobile · 3단계에서 막힘', '막힌 단계만 강조하고 정확한 행동 하나를 붙입니다. QR은 안 나옵니다.', settings(p('set-blocked'), {passing: false})),
        labeled(p('cap-ready'), '설정 › Mobile · 모두 통과', 'QR + ts.net 주소 + 만료 카운트다운, 연결된 폰 목록(해지, 자동 해지 예고), 푸시 3단.', settings(p('set-ready'), {passing: true})),
      ]),
      rowFrame('row-phone', 'Phone', [
        labeled(p('cap-pair'), '① 페어링 확인', 'QR을 찍으면 Safari에 열리는 첫 화면. 연결을 누르면 코드가 교환되고 목록으로.', phone(p('pair'), 'Phone · Pairing', pairBody(p('pair-b')))),
        labeled(p('cap-list'), '② 목록', '내 확인 대기 / 끝 / 진행 중 / 확인함. 요청은 warning, 소식은 bright, 확인한 건 dim. SSH 머신은 칩.', phone(p('list'), 'Phone · List', listBody(p('list-b')))),
        labeled(p('cap-detail'), '③ 상세', '읽기 전용 스크롤백(위로 당기면 더), 퀵키 5개, 한 줄 답장. 권한 프롬프트는 ↑↓⏎로 답합니다.', phone(p('detail'), 'Phone · Detail', detailBody(p('detail-b'), needs0))),
      ]),
      rowFrame('row-states', 'Phone states', [
        labeled(p('cap-unreach'), '④ 연결 안 됨', '한 줄로 두 원인을 말하고 자동 재시도. 목록은 마지막 상태로 흐리게.', phone(p('unreach'), 'Phone · Unreachable', listBody(p('unreach-b'), {unreachable: true}))),
        labeled(p('cap-empty'), '⑤ 비어 있음', '에이전트가 없을 때의 가장 작은 형태.', phone(p('empty'), 'Phone · Empty', listBody(p('empty-b'), {empty: true}))),
        labeled(p('cap-push'), '⑥ 푸시 배너', '루트 에이전트당 하나, 작업명 + 상태 + 프로젝트. 탭하면 ③으로.', pushBanner(p('push'))),
      ]),
    ])];
  }
  return screenSheet('screen-mobile', 'Screen / Mobile', 'web/src/MobileTab.tsx, mobile.ts, and the phone app under web/src/mobile/ (entry web/mobile.html), PRD mobile-companion D-08 and D-12: Settings > Mobile after Devices, blocked with only the failing check lit and one action beside it and no QR, then ready with every check passing, the QR, the ts.net address, the code countdown with 새 코드, the connected phones (2 / 4) with 해지 and the automatic-removal notice, and the three push modes. The phone app: the pairing confirm the QR opens, the list in four groups (내 확인 대기, 끝, 진행 중, 확인함) on the desktop row rules at phone sizes, the detail with its read-only scrollback, five quick keys and one-line reply, the unreachable state (one line naming both causes over the dimmed last list), the empty state, and the push banner that opens the detail. The machine name and tailnet are placeholders and the QR is never a real code.', build, build);
}

// -- assembly ---------------------------------------------------------------------

// -- Screen / Disk Cleanup -------------------------------------------------------

const DISK_CLEANUP_SPEC = 'web/src/DiskCleanupSheet.tsx, DiskEntrance.tsx, diskCleanup.ts (PRD disk-layers B1-B28, worded from the shipped app): the Overview facts line with the disk number’s breakdown tooltip (빌드 캐시, 의존성, 워크트리 소스, 기타, 공유 Git), the `≥` number and its tooltip line while a checkout could not be measured, and the low-disk cell that opens the sheet on 끝난 것, then the sheet as a checkout x layer table (빌드 캐시, 의존성, 워크트리, 기타 read-only, 합계) with the checkbox for every unit picked at once (cell, row, column, top-left, the fold of small checkouts, all limited to the rows the filter shows; a worktree pick turns its row’s caches into the disabled Checked state, a partly picked group draws the Checkbox’s Indeterminate). The states the table carries follow: measuring, not measured, in use with its reason beside the name, usage unreadable, another cleanup running, no row for the filter. The table has no subtitle, no second header line and no legend, and its footer says only `N칸 · X`. The confirm step exists only when a worktree is picked and names the folder; the result is one line of the volume’s free space before and after over its per-cell rows. Sizes and names are invented mock content.';

function buildDiskCleanup(tokens) {
  const build = suffix => diskCleanupRows(tokens, {themedXref, screenButton, screenDialogSurface}, suffix);
  return screenSheet('screen-disk-cleanup', 'Screen / Disk Cleanup', DISK_CLEANUP_SPEC, build, build);
}

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
    {name: 'Screen / Workspace', build: () => buildWorkspace(tokens)},
    {name: 'Screen / Project Sessions', build: () => buildSessions(tokens)},
    {name: 'Screen / Settings', build: () => buildSettings(tokens)},
    {name: 'Screen / Palette', build: () => buildPalette(tokens)},
    {name: 'Screen / Dialogs and Sheets', build: () => buildDialogs(tokens)},
    {name: 'Screen / Menus and Overlays', build: () => buildMenus()},
    {name: 'Screen / Projects Sidebar', build: () => buildProjectsSidebar(tokens)},
    {name: 'Screen / Mobile', build: () => buildMobile(tokens)},
    {name: 'Screen / Disk Cleanup', build: () => buildDiskCleanup(tokens)},
  ];
}
