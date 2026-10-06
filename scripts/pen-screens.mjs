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
// Nested same-library refs carry their master's defaults and instance patches.
// The generator and checker share this effective color walk, addressed by each
// full instance path, so separate instances never share an override by accident.

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
  if (typeof id !== 'string' || !id) return null;
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
export function themedOverrides(masterId, document = libraryDocument()) {
  const master = findMaster({children: document.children}, masterId);
  if (!master) throw new Error(`pen-screens needs master ${masterId}, which ${CANVAS} no longer carries`);
  const top = {};
  const descendants = {};
  const colors = (node, address) => {
    const props = {};
    for (const prop of THEMED_PROPS) if (LOCAL_TOKEN.test(node[prop])) props[prop] = node[prop];
    if (!address.length) Object.assign(top, props);
    else if (Object.keys(props).length) descendants[address.join('/')] = props;
  };
  const patches = (address, scopes) => {
    const props = {};
    for (const {prefix, entries} of scopes) {
      if (!prefix.every((part, i) => address[i] === part)) continue;
      const key = address.slice(prefix.length).join('/');
      if (Object.hasOwn(entries, key)) Object.assign(props, entries[key]);
    }
    return props;
  };
  const walk = (node, prefix, scopes, activeMasters, isRoot = false) => {
    const address = isRoot ? prefix : [...prefix, node.id];
    const effective = {...node, ...patches(address, scopes)};
    if (effective.type === 'ref') {
      let resolved = effective;
      let nestedScopes = scopes;
      const active = new Set(activeMasters);
      while (resolved.type === 'ref') {
        const target = findMaster(document, resolved.ref);
        if (!target) throw new Error(`Component instance ${address.join('/')} has no target ${JSON.stringify(resolved.ref)}`);
        if (active.has(target.id)) throw new Error(`Component instance ${address.join('/')} cycles to active master ${target.id}`);
        active.add(target.id);
        // Inner master patches are defaults; enclosing instances apply last.
        nestedScopes = [{prefix: address, entries: resolved.descendants ?? {}}, ...nestedScopes];
        const {id: _id, type: _type, ref: _ref, children: _children, descendants: _descendants, ...overrides} = resolved;
        resolved = {...target, ...overrides};
      }
      colors(resolved, address);
      for (const child of resolved.children ?? []) walk(child, address, nestedScopes, active);
    } else {
      colors(effective, address);
      for (const child of effective.children ?? []) walk(child, prefix, scopes, activeMasters);
    }
  };
  walk(master, [], [], new Set([masterId]), true);
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
  const prefixed = descendants ? Object.fromEntries(Object.entries(descendants).map(([key, value]) => [key.split('/').map(part => `${ALIAS}:${part}`).join('/'), value])) : undefined;
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
// `activeIndex` is the one lit item, or an array of the lit ones for a
// multiple group (the graph's status chips), empty when none is lit.
function screenToggleGroup(id, items, activeIndex) {
  const lit = Array.isArray(activeIndex) ? activeIndex : [activeIndex];
  return frame(id, 'Toggle Group', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'},
    items.map((label, index) => themedXref(`${id}-${index}`, 'tog-m', label,
      lit.includes(index) ? {fill: '$--secondary'} : {},
      {'tog-t': {content: label, fill: lit.includes(index) ? '$--foreground' : '$--subtle-foreground'}})));
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
    {status: 'done', title: '원격 분리', branch: 'mailbox-decouple', device: 'mini', age: '1h'},
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
// draws it, so its footer holds the one-device window's device button, the usage
// chips and the Settings gear side by side. Its top is the device's Home row,
// marked while `overview` is the screen beside it, over the Projects | Agents
// strip (PRD home-device-rail D-09, D-10).
function screenSidebar(tokens, id, suffix, agents, {overview = false} = {}) {
  return frame(`${id}-${suffix}`, 'Sidebar', {width: num(tokens, '--size-sidebar-ideal'), layout: 'vertical', gap: '$--spacing-md', fill: '$--sidebar', padding: '$--spacing-md', cornerRadius: '$--radius-md'}, [
    frame(`${id}-overview-${suffix}`, 'Overview', {
      width: 'fill_container', height: num(tokens, '--size-project-row'), layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-sm',
      ...(overview ? {fill: '$--secondary'} : {}),
    }, [
      icon(`${id}-overviewi-${suffix}`, 'layout-dashboard', {size: num(tokens, '--size-checkout-icon'), fill: '$--subtle-foreground'}),
      text(`${id}-overviewt-${suffix}`, 'Overview', {size: '$--text-subhead', weight: '600'}),
      frame(`${id}-overviewgap-${suffix}`, 'Spacer', {width: 'fill_container', height: 1}, []),
      text(`${id}-overviewn-${suffix}`, '2 asking · ⌘⇧O', {fill: '$--muted-foreground'}),
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
      themedXref(`${id}-device-${suffix}`, 'Nyvom', 'Devices', {width: 20, height: 20}, {ZIZFR: {icon: 'laptop', fill: '$--muted-foreground'}}),
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
function viewTabs(id, items, activeIndex, waiting, answering = 0) {
  return frame(id, 'Tabs', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'}, items.map((label, index) => {
    const active = index === activeIndex;
    const content = {'tab-t': {content: label, fill: active ? '$--foreground' : '$--subtle-foreground'}};
    // 요청 carries the rows to answer (PRD overview-request-view B1), Agents the agents waiting.
    const count = label === 'Agents' ? waiting : label === '요청' ? answering : 0;
    if (count === 0) return themedXref(`${id}-${index}`, 'tab-m', label, active ? {fill: '$--secondary'} : {}, content);
    return frame(`${id}-${index}w`, label, {layout: 'horizontal', alignItems: 'center', height: 24, padding: [0, '$--spacing-sm', 0, 0], cornerRadius: '$--radius-xs', ...(active ? {fill: '$--secondary'} : {})}, [
      themedXref(`${id}-${index}`, 'tab-m', label, {padding: [0, '$--spacing-xs', 0, '$--spacing-sm']}, content),
      text(`${id}-${index}-n`, String(count), {size: '$--text-caption', fill: '$--warning'}),
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
// quiet action and 새 이슈 as the primary one, the facts line with the view's
// control at its right end (the Agents graph's filter, PRD agents-graph-view
// B24; the Issues filter and mode), then the tiles where the tab row was.
// `filter` is the graph's filter control, drawn by graphParts().filter.
function overviewHeader(tokens, id, suffix, {project, facts, view, width, mode, filter}) {
  const control = view === 'agents'
    ? [filter(`${id}-gfilter-${suffix}`)]
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

// The tiles (OverviewLenses.tsx LensTiles, B1-B5; 요청 first, PRD
// overview-request-view B2): one width each, the chosen
// one outlined; the name, the yellow badge of the operator's turn, the large
// number and its unit, and one bar whose parts carry the bar's tones.
const HERDR_TILES = [
  {id: 'requests', label: '요청', value: '5', badge: '2', bar: [['$--warning', 2], ['$--destructive', 1], ['$--success', 1], ['$--primary', 1]]},
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

// -- the Agents graph (GraphView.tsx over agentGraph.ts) ------------------------

// The Agents tab's graph (PRD agents-graph-view B2-B28, D-34), authored on
// local tokens like the task card: a checkout is a box (a head and a row per
// agent), a delegation into another checkout a rounded orthogonal line from
// the parent row's right port to the child row's left port. Every size is a
// `--graph-*` token and the column gap is `--home-dependency-gap`, so a box
// here is the box the web lays out; `Component / Agent graph box` is the same
// box as a library master. The lines are Pen paths drawn behind the boxes, and
// the working line's flowing dashes are drawn as a still dash pattern, since
// a Pen path carries no dash property.
const GRAPH_EDGE_RADIUS = 10; // web/src/agentGraph.ts EDGE_RADIUS, a code constant rather than a token
const GRAPH_WAIT_OPACITY = 0.45; // GraphView's 45% mix of the working colour for a line waiting on children
const GRAPH_FLOW_OPACITY = 0.4; // the lighter dashes of a working line, a mix of the working colour toward the foreground
const GRAPH_DASH = [6, 12]; // index.css .graph-edge-flow stroke-dasharray
const GRAPH_EDGE = {
  ask: {fill: '$--warning', word: '묻는 중'},
  flow: {fill: '$--agent-working', word: '일하는 중'},
  wait: {fill: '$--agent-working', word: '하위를 기다리는 중', opacity: GRAPH_WAIT_OPACITY},
  rest: {fill: '$--muted-foreground', word: '쉬는 중'},
};

function round2(value) {
  return Math.round(value * 100) / 100;
}

// The route's points with every corner cut: a straight run to the corner's
// near side, then a quadratic through the corner (agentGraph.ts forwardPath).
function graphRoute(points, radius) {
  const geometry = [`M ${round2(points[0][0])} ${round2(points[0][1])}`];
  const sampled = [points[0]];
  for (let i = 1; i < points.length - 1; i += 1) {
    const [px, py] = points[i - 1];
    const [x, y] = points[i];
    const [nx, ny] = points[i + 1];
    const before = Math.hypot(x - px, y - py);
    const after = Math.hypot(nx - x, ny - y);
    const r = Math.min(radius, before / 2, after / 2);
    const inX = x + ((px - x) / before) * r;
    const inY = y + ((py - y) / before) * r;
    const outX = x + ((nx - x) / after) * r;
    const outY = y + ((ny - y) / after) * r;
    geometry.push(`L ${round2(inX)} ${round2(inY)} Q ${round2(x)} ${round2(y)} ${round2(outX)} ${round2(outY)}`);
    sampled.push([inX, inY]);
    for (let step = 1; step <= 6; step += 1) {
      const t = step / 6;
      sampled.push([(1 - t) * (1 - t) * inX + 2 * (1 - t) * t * x + t * t * outX, (1 - t) * (1 - t) * inY + 2 * (1 - t) * t * y + t * t * outY]);
    }
  }
  const last = points[points.length - 1];
  geometry.push(`L ${round2(last[0])} ${round2(last[1])}`);
  sampled.push(last);
  return {d: geometry.join(' '), sampled};
}

// The route cut into dashes along its length: `on` drawn, `off` skipped.
function graphDashes(sampled, [on, off]) {
  const parts = [];
  let current = [sampled[0]];
  let drawing = true;
  let left = on;
  for (let i = 1; i < sampled.length; i += 1) {
    let [x0, y0] = sampled[i - 1];
    const [x1, y1] = sampled[i];
    let length = Math.hypot(x1 - x0, y1 - y0);
    while (length > left) {
      const t = left / length;
      x0 += (x1 - x0) * t;
      y0 += (y1 - y0) * t;
      if (drawing) {
        current.push([x0, y0]);
        parts.push(current);
      } else current = [[x0, y0]];
      drawing = !drawing;
      left = drawing ? on : off;
      length = Math.hypot(x1 - x0, y1 - y0);
    }
    left -= length;
    if (drawing) current.push([x1, y1]);
  }
  if (drawing && current.length > 1) parts.push(current);
  return parts.map(part => `M ${part.map(([x, y]) => `${round2(x)} ${round2(y)}`).join(' L ')}`).join(' ');
}

function graphParts(tokens) {
  const g = {
    boxWidth: num(tokens, '--graph-box-width'),
    headHeight: num(tokens, '--graph-head-height'),
    headLine: num(tokens, '--graph-head-line'),
    rowHeight: num(tokens, '--graph-row-height'),
    askingRowHeight: num(tokens, '--graph-row-asking-height'),
    rowLine: num(tokens, '--graph-row-line'),
    padBottom: num(tokens, '--graph-box-pad-bottom'),
    boxGap: num(tokens, '--graph-box-gap'),
    pad: num(tokens, '--graph-pad'),
    portOffset: num(tokens, '--graph-port-offset'),
    portRadius: num(tokens, '--graph-port-radius'),
    edgeWidth: num(tokens, '--graph-edge-width'),
    flowWidth: num(tokens, '--graph-edge-flow-width'),
    trayInsetX: num(tokens, '--graph-tray-inset-x'),
    trayInsetY: num(tokens, '--graph-tray-inset-y'),
    searchWidth: num(tokens, '--graph-search-width'),
    columnGap: num(tokens, '--home-dependency-gap'),
  };
  const sm = num(tokens, '--spacing-sm');
  const xs = num(tokens, '--spacing-xs');
  const indentStep = num(tokens, '--size-lineage-indent');
  const mark = num(tokens, '--size-agent-mark');
  const iconSmall = num(tokens, '--size-icon-sm');
  const dimmed = num(tokens, '--opacity-secondary');
  const caption = (id, content, fill = '$--muted-foreground', mono = false) => text(id, content, {size: '$--text-caption', fill, mono});
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);

  const rowHeightOf = row => (row.line ? g.askingRowHeight : g.rowHeight);
  const boxHeightOf = rows => g.headHeight + rows.reduce((sum, row) => sum + rowHeightOf(row), 0) + g.padBottom;
  /** A row's port, the middle of its first line, in the canvas the box stands in. */
  const portY = (boxY, rows, index) => boxY + g.headHeight + rows.slice(0, index).reduce((sum, row) => sum + rowHeightOf(row), 0) + g.portOffset;
  /** Where a child box stands so its row's port is level with the parent's. */
  const levelY = (parentPortY, rows, index) => parentPortY - g.headHeight - rows.slice(0, index).reduce((sum, row) => sum + rowHeightOf(row), 0) - g.portOffset;

  // A line (B8, B9, D-04): a path from the parent's port to the child's, one
  // trunk between the columns, coloured by the child's state; a working line
  // carries its dashes on top, a waiting one is the working colour at 45%.
  function edge(id, {sx, sy, tx, ty, trunk, kind}) {
    const style = GRAPH_EDGE[kind];
    const points = sy === ty ? [[sx, sy], [tx, ty]] : [[sx, sy], [trunk, sy], [trunk, ty], [tx, ty]];
    const pad = g.flowWidth;
    const left = Math.min(...points.map(([x]) => x)) - pad;
    const top = Math.min(...points.map(([, y]) => y)) - pad;
    const width = Math.max(...points.map(([x]) => x)) + pad - left;
    const height = Math.max(...points.map(([, y]) => y)) + pad - top;
    const route = graphRoute(points.map(([x, y]) => [x - left, y - top]), GRAPH_EDGE_RADIUS);
    const shape = {type: 'path', x: left, y: top, width, height, viewBox: [0, 0, width, height], strokeLinecap: 'round', strokeLinejoin: 'round'};
    const dot = (key, cx, cy) => ({type: 'ellipse', id: `${id}-${key}`, name: 'Port', x: round2(cx - g.portRadius), y: round2(cy - g.portRadius), width: 2 * g.portRadius, height: 2 * g.portRadius, fill: style.fill, ...(style.opacity ? {opacity: style.opacity} : {})});
    return [
      {...shape, id: `${id}-line`, name: `Line · ${style.word}`, geometry: route.d, stroke: style.fill, strokeWidth: g.edgeWidth, ...(style.opacity ? {opacity: style.opacity} : {})},
      ...(kind === 'flow' ? [{...shape, id: `${id}-flow`, name: 'Flowing dashes', geometry: graphDashes(route.sampled, GRAPH_DASH), stroke: '$--foreground', strokeWidth: g.flowWidth, opacity: GRAPH_FLOW_OPACITY}] : []),
      dot('out', sx, sy),
      dot('in', tx, ty),
    ];
  }

  // The issue chip (IssueChip): the source glyph and the id in muted mono.
  function issueChip(id, label) {
    return frame(id, 'Issue chip', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      icon(`${id}-g`, 'circle-dot', {size: iconSmall, fill: '$--muted-foreground'}), caption(`${id}-t`, label, '$--muted-foreground', true),
    ]);
  }

  // The PR chip (B13, PullRequestChip): the outline badge in the PR's state
  // colour with its number, then CI as ✓ / ✗ / ●, then `변경 요청` in warning.
  function prChip(id, {number, tone = 'open', checks, review}) {
    const fill = PR_TONE[tone];
    const ci = ciMark(tokens, `${id}-ci`, checks);
    return frame(id, 'PR chip', {
      layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', padding: [0, '$--spacing-xs'], height: g.headLine, cornerRadius: '$--radius-sm',
      stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
    }, [
      icon(`${id}-g`, tone === 'merged' ? 'git-merge' : 'git-pull-request', {size: iconSmall, fill}),
      caption(`${id}-n`, `#${number}`, fill, true),
      ...(ci ? [ci] : []),
      ...(review === 'changes_requested' ? [caption(`${id}-rv`, '변경 요청', '$--warning')] : []),
    ]);
  }

  // The head (B12, B15, B16): the glyph in the PR's colour and the mono
  // branch, the purpose, then the chips line; main's head the house and
  // `에이전트 N`. A merged box is dimmed and offers `정리`, always visible.
  function head(id, box) {
    const inner = g.boxWidth - 2 * sm;
    const glyph = box.primary ? 'house' : box.cleanup ? 'git-merge' : box.pr ? 'git-pull-request' : 'git-branch';
    const tone = box.cleanup ? '$--pr-merged' : box.pr ? PR_TONE[box.pr.tone ?? 'open'] : '$--muted-foreground';
    const glyphSize = num(tokens, '--size-checkout-icon');
    const chips = box.primary ? [caption(`${id}-agents`, `에이전트 ${box.agents}`, '$--muted-foreground', true)] : [
      ...(box.task ? [issueChip(`${id}-task`, box.task)] : []),
      ...(box.pr ? [prChip(`${id}-pr`, box.pr)] : []),
      ...(box.distance ? [caption(`${id}-dist`, box.distance, '$--muted-foreground', true)] : []),
      ...(box.files ? [caption(`${id}-files`, `${box.files} files`, '$--warning', true)] : []),
    ];
    return frame(id, 'Head', {
      x: 0, y: 0, layout: 'vertical', justifyContent: 'center', width: g.boxWidth, height: g.headHeight, padding: [0, '$--spacing-sm'], ...(box.cleanup ? {opacity: dimmed} : {}),
    }, [
      frame(`${id}-l1`, 'Branch', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: inner, height: g.headLine}, [
        icon(`${id}-g`, glyph, {size: glyphSize, fill: tone}),
        text(`${id}-b`, fitText(box.branch, inner - glyphSize - xs, 13, true), {size: '$--text-body', mono: true}),
      ]),
      frame(`${id}-l2`, 'Purpose', {layout: 'horizontal', alignItems: 'center', width: inner, height: g.headLine}, box.purpose ? [caption(`${id}-purpose`, fitText(box.purpose, inner, 11))] : []),
      frame(`${id}-l3`, 'Chips', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: inner, height: g.headLine}, [
        ...chips,
        spacer(`${id}-sp`),
        ...(box.cleanup ? [caption(`${id}-clean`, '정리', '$--subtle-foreground')] : []),
      ]),
    ]);
  }

  // A row (B6, B17, B22): the mark, provider, title and age on one line, a
  // step in and a corner arrow when it is a delegation inside this checkout,
  // the tucked badge before the age, and only while the agent asks the
  // question as a second line in warning.
  function row(id, value, top) {
    const [symbol, color] = AGENT_MARK[value.mark];
    const asking = Boolean(value.line);
    const indent = (value.depth ?? 0) * indentStep;
    const tucked = value.tucked ? value.tucked.map(({symbol: glyph, color: tone, count}, index) => frame(`${id}-tk${index}`, 'Tucked mark', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center'}, [
      screenStatusMark(tokens, `${id}-tk${index}-m`, glyph, tone), caption(`${id}-tk${index}-n`, String(count), '$--foreground', true),
    ])) : [];
    const room = g.boxWidth - 2 * sm - indent - mark - 14 - 4 * xs - 36 - (value.depth ? iconSmall + xs : 0) - (tucked.length ? 44 : 0);
    const first = frame(`${id}-l1`, 'Line', {
      layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: g.boxWidth, height: g.rowHeight, padding: [0, '$--spacing-sm', 0, sm + indent],
    }, [
      ...(value.depth ? [icon(`${id}-in`, 'corner-down-right', {size: iconSmall, fill: '$--muted-foreground'})] : []),
      screenStatusMark(tokens, `${id}-m`, symbol, color),
      frame(`${id}-p`, 'Provider artwork', {width: 14, height: 14, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${value.provider ?? 'claude'}.png`, mode: 'fit'}}, []),
      text(`${id}-t`, fitText(value.title, room, 13), {size: '$--text-body', weight: asking ? '600' : '400'}),
      spacer(`${id}-sp`),
      ...(tucked.length ? [frame(`${id}-tucked`, 'Tucked badge', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', padding: [0, '$--spacing-xs'], height: g.rowLine, cornerRadius: '$--radius-sm', fill: '$--secondary'}, tucked)] : []),
      caption(`${id}-age`, value.age, '$--muted-foreground', true),
    ]);
    return frame(id, value.title, {x: 0, y: top, layout: 'vertical', gap: 0, width: g.boxWidth, height: rowHeightOf(value)}, [
      first,
      ...(asking ? [frame(`${id}-l2`, 'Question', {layout: 'horizontal', alignItems: 'center', width: g.boxWidth, height: g.rowLine, padding: [0, '$--spacing-sm', 0, sm + indent + mark]}, [
        caption(`${id}-q`, fitText(value.line, g.boxWidth - 2 * sm - indent - mark, 11), '$--warning'),
      ])] : []),
    ]);
  }

  // A box (B2, B7, B21): the head, the rows under it with one pale tray behind
  // the rows of a shared tab, a selection outline, and dimmed when every row rests.
  function box(id, spec, place = {}) {
    const rows = spec.rows;
    const tops = [];
    let top = g.headHeight;
    for (const value of rows) {
      tops.push(top);
      top += rowHeightOf(value);
    }
    const trays = [];
    rows.forEach((value, index) => {
      if (!value.tray) return;
      const previous = trays[trays.length - 1];
      if (previous && previous.key === value.tray && previous.last === index - 1) {
        previous.last = index;
        previous.bottom = tops[index] + rowHeightOf(value);
      } else trays.push({key: value.tray, last: index, top: tops[index], bottom: tops[index] + rowHeightOf(value)});
    });
    return frame(id, spec.branch, {
      ...(place.x !== undefined ? {x: place.x, y: place.y} : {}), layout: 'none', width: g.boxWidth, height: boxHeightOf(rows), cornerRadius: '$--radius-md', fill: '$--card',
      stroke: spec.selected ? '$--primary' : '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', ...(spec.resting ? {opacity: dimmed} : {}),
    }, [
      ...trays.map((tray, index) => frame(`${id}-tray${index}`, 'Same tab', {
        x: g.trayInsetX, y: tray.top - g.trayInsetY, width: g.boxWidth - 2 * g.trayInsetX, height: tray.bottom - tray.top + 2 * g.trayInsetY,
        cornerRadius: '$--radius-sm', fill: '$--muted', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
      }, [])),
      head(`${id}-head`, spec),
      ...rows.map((value, index) => row(`${id}-r${index}`, value, tops[index])),
    ]);
  }

  // A folded line (FoldLine, B21, B23): its words and count, and the chevron.
  function fold(id, label, width, open = false) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width, padding: ['$--spacing-xs', '$--spacing-sm'], cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      caption(`${id}-t`, label, '$--subtle-foreground'),
      spacer(`${id}-sp`),
      icon(`${id}-g`, open ? 'chevron-down' : 'chevron-right', {size: 14, fill: '$--muted-foreground'}),
    ]);
  }

  // The line colours in words (EDGE_WORDS): a short rule in each colour and what it says.
  function legend(id) {
    return frame(id, 'Line colours', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'center'}, Object.entries(GRAPH_EDGE).map(([kind, style]) => frame(`${id}-${kind}`, style.word, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
      frame(`${id}-${kind}-r`, 'Rule', {width: 20, height: g.edgeWidth, cornerRadius: g.edgeWidth / 2, fill: style.fill, ...(style.opacity ? {opacity: style.opacity} : {})}, []),
      caption(`${id}-${kind}-t`, style.word),
    ])));
  }

  // The filter at the facts line's right end (B24-B26, D-29): the three status
  // chips as a multiple Toggle Group, `chips` the lit ones, and the search field
  // with its glyph. No device control: this scope has one device.
  function filter(id, {chips = [], query = ''} = {}) {
    const small = num(tokens, '--size-control-sm');
    return frame(id, 'Filter', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
      screenToggleGroup(`${id}-chips`, ['내 차례', '일하는 중', '쉬는 중'], chips),
      frame(`${id}-search`, '검색', {layout: 'none', width: g.searchWidth, height: small}, [
        {...themedXref(`${id}-in`, 'inp-m', 'Input', {width: g.searchWidth, height: small, padding: [0, '$--spacing-sm', 0, 24]}, {
          'inp-t': query ? {content: query, fill: '$--foreground'} : {content: '제목 · 브랜치 · #번호', fill: '$--muted-foreground'},
        }), x: 0, y: 0},
        {...icon(`${id}-g`, 'search', {size: iconSmall, fill: '$--muted-foreground'}), x: xs, y: (small - iconSmall) / 2},
      ]),
    ]);
  }

  // Nothing matches the filter (B28): one line where the graph stood.
  function filterEmpty(id, width) {
    return frame(id, 'Filter empty', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', justifyContent: 'center', width, padding: '$--spacing-xl'}, [
      caption(`${id}-t`, '필터에 맞는 에이전트가 없습니다'),
      screenButton(`${id}-clear`, '필터 해제', {variant: 'ghost', height: num(tokens, '--size-control-sm')}),
    ]);
  }

  return {g, box, edge, fold, legend, filter, filterEmpty, boxHeightOf, portY, levelY};
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

const MAIN_SPEC = 'web/src/App.tsx, sidebar.tsx, MainScreen.tsx, RequestView.tsx, TaskBoards.tsx, projectBoard.ts: Overview, the scope the sidebar’s global Overview row opens and marks. Its title carries Add project and 새 이슈 as the primary action (C); its facts line the project count, the open issues once every source has answered, and the open-PR and merged totals only when every project can give its part; its 요청 · Tasks · Agents · Projects tabs the count of rows to answer on 요청 and of agents waiting on the operator on Agents. 요청, which every way in opens, is the Project Overview’s request view over every project, each row with its project’s name; Agents’ view is the Project Overview’s graph over every project with the project’s name above each project’s band. Every project’s issues share one board, 백로그 · 진행 중 · 리뷰 · 완료, each card an issue with its project beside its id (a Local issue as L-N): 시작 on a backlog card under the pointer, the operator’s-turn cards in the warning border, the worktrees with no issue folded into one line at the foot of 진행 중, and 완료 folded to one line per project with its count. Every project has an issue source, so no project is set apart as unconnected. Its Dependencies mode draws an arrow that crosses projects, the blocker named with its repository on the lock line. The Projects view is the project list grouped by device.';

const OVERVIEW_SPEC = 'web/src/ProjectOverview.tsx, GraphView.tsx, agentGraph.ts, OverviewLenses.tsx, overviewLens.ts, TaskBoards.tsx, projectBoard.ts: a project’s Overview (PRD overview-lenses-tiles-agents, PRD agents-graph-view). The header carries the path back, New agent as the quiet action and 새 이슈 as the primary one (C), the facts line of worktrees, disk, main behind and N merged → 정리 with the view’s control at its right end, then the tiles 요청 · Agents · Issues · PRs · Sessions where the tab row was: the name, the yellow badge of the operator’s turn, the large number and its unit, one bar; the chosen tile outlined. Every entry opens 요청 (PRD overview-request-view, RequestView.tsx, requestList.ts): one row per agent grouped 답할 것, 고칠 것, 리뷰·머지, 멈춤, 결과 볼 것, 일하는 중, 기다리는 중 and 쉬는 중 (folded); a row is the status mark, the kind mark and the title, then on the right the descendants (자식 N · 일하는 중 M, 질문 K in warning), the PR chip with CI and +N, the issue chip, the checkout and the time; under it 나 › and the request on one line, its front cut and its end kept, then the result line (warning on a row to answer) with its open chips; an expanded row adds the request as written, the agent’s last words, its pull requests (예전 PR #N 머지됨 for one settled before), its descendants with 열기, and 패널 열기 ⌘↵. With no agent the view is one line and New agent; with nothing to do it is 할 일 없음 above the folded 쉬는 중. The Agents tile opens one graph (PRD agents-graph-view): a checkout is a box with its head (the glyph in its PR’s colour and the branch, the purpose, then the issue chip, the PR chip with its CI mark and 변경 요청, ↑N ↓N and the files in warning; main the house and 에이전트 N; a merged box dimmed with 정리) and a row per agent (mark, provider, title and age; a step in with a corner arrow for a delegation inside the checkout; a tucked badge such as ✓2 for folded children; only an asking row has a second line, its question in warning). The front checkout’s box carries the selection outline, main’s box is first, and boxes stand in columns by how deep their delegation runs. A delegation into another checkout is a rounded orthogonal line from the parent row’s right port to the child row’s left port, coloured by the child: blue with dashes while it works, orange when it asks, pale blue while it waits on children, grey otherwise. The worktrees with no agent, the ones to clean up and the resting ones fold into one line each. The facts line’s right end carries the filter: the status chips 내 차례 · 일하는 중 · 쉬는 중, a search field and, only with two devices in view, the device choice. Below the graph the box states (primary, a worktree with its chips, a merged box, an asking row, a row with a tucked badge, the selected box) and the filter that matches nothing, one line with 필터 해제. The Issues tile opens a board of issues only (PRD overview-lenses-issues): a card is the glyph, id and at most two labels, the title, the lock line, the checkout chip and the PR chip with its CI and review word, and at most two agents; its buttons fill the id line’s slot under the pointer (시작 S, Workspace O, the PR icon, a Local issue’s edit, ⋯); only the operator’s turn is outlined in warning. The worktrees and pull requests with no issue are one line each under 진행 중 and 리뷰, and 완료 is folded to one line per issue with the pull request that closed it. The facts line’s right end carries the filter and Board · List · Dependencies. A card opens the issue panel beside the board: the head (glyph, id, source, Open, ×), the title, the action line, the properties, 이 이슈로 한 일, the Markdown body and a GitHub issue’s latest comments; a Local issue edits in place, and a failed read is one line with 재시도. The PRs tile opens the project’s pull requests grouped 내 차례, 에이전트가 고치는 중, CI 실패 · 맡은 에이전트 없음 and 최근 머지 (folded) (PRD overview-lenses-prs): a row is ▸, the state glyph, the number, the title, the issue cell (a dotted circle when empty, the 이슈 잇기 icon under the pointer), 확인, the agents’ marks, the branch, CI, the review word and the time, whose fixed slot holds GitHub and ⋯, ▷ 맡기기 or 정리 under the pointer; an unfolded row shows the branch’s agents and GitHub, Workspace and 이슈 잇기. 이슈 잇기 on a GitHub issue asks once, 그만두기 first, before it writes Closes #N into the body.';

// -- Screen / Main ------------------------------------------------------------

// The Overview of every project (MainScreen.tsx, PRD task-agents-views D-10,
// titled Overview by PRD sidebar-shell D-02, reworked issue first): the
// sidebar's Overview row marked, the title with Add project and 새 이슈, the
// facts line, the 요청 · Tasks · Agents · Projects tabs, the request view every
// way in opens (PRD overview-request-view), every project's issues and
// worktrees on one board with the project beside each id, Done folded per
// project, and the Dependencies mode with an arrow that crosses projects.
function buildMain(tokens) {
  const {column, taskCard, stageColumn, doneColumn, foldLine, arrow, legend, chain} = issueBoardParts(tokens);
  const {requestGroup, requestRow} = requestParts(tokens);
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
      viewRow(`main-row-${key}`, viewTabs(`main-tabs-${key}`, ['요청', 'Tasks', 'Agents', 'Projects'], mode ? 1 : 0, 3, 2), mode, width),
    ]);
  }
  function build(suffix) {
    const sidebar = screenSidebar(tokens, 'main-sidebar', suffix, [
      {title: '카드 상태 시트 설계', status: 'Waiting', symbol: '○', badge: '●1', fold: 'folded', summaries: [
        {status: 'working', branch: 'web-view-overlay', pr: '#173'},
        {status: 'done', branch: 'web-side-panel', pr: '#170', more: 1},
      ]},
      {title: '조용한 순찰 기능 개발', status: 'Seen', symbol: '○', statusColor: '$--muted-foreground', fold: 'folded', summaries: [
        {status: 'done', branch: 'mailbox-decouple', device: 'mini'},
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
          card('r2', {task: gh(9), project: 'sasu', title: 'mailbox-decouple: move lineage tokens into the plugin', branch: '9-mailbox-decouple', pr: {number: 12, checks: 'pending', review: 'review_required'}}),
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
    // 요청 as every way into Home opens it (PRD overview-request-view B1, B3):
    // each row carries its project's name.
    const rows = requestRowsOf('herdr-ide');
    const r = (key, spec) => requestRow(`main-rq-${key}-${suffix}`, spec, width);
    const requests = frame(`main-requests-${suffix}`, 'Overview · 요청', {width, layout: 'vertical', gap: '$--spacing-md'}, [
      header(`r${suffix}`, null),
      requestGroup(`main-rqg1-${suffix}`, '답할 것', 2, [...r('answer', rows.answer), ...r('sasu', {mark: 'ask', title: 'judge 백엔드 전환', project: 'sasu', request: 'judge를 codex로도 돌리게', result: 'claude를 기본으로 둘까요?', place: '14-judge-backend', age: '4m', issue: gh(14)})]),
      requestGroup(`main-rqg2-${suffix}`, '결과 볼 것', 1, r('result', rows.result)),
      requestGroup(`main-rqg3-${suffix}`, '일하는 중', 1, r('working', rows.working)),
      requestGroup(`main-rqg4-${suffix}`, '쉬는 중', 7, [], {folded: true}),
    ]);
    return [sidebar, frame(`main-views-${suffix}`, 'Views', {layout: 'vertical', gap: '$--spacing-xl'}, [screenLineageDetails(tokens, suffix), requests, board, dependencies])];
  }
  return screenSheet('screen-main', 'Screen / Main', MAIN_SPEC, build, build);
}

// -- the request view (RequestView.tsx over requestList.ts) ----------------------

// A group head and its rows (PRD overview-request-view B3-B6, B13), authored
// on local tokens like the PR row, since no library master draws a request
// row. A row is the status mark, the agent's kind mark and its title (with its
// project on Home), then on the right the descendants, the PR chip with `+N`,
// the issue chip, the checkout and the time; under it `나 ›` and the request
// on one line, its front cut and its end kept, then the result line with its
// open chips. An expanded row adds the request as written, the agent's last
// words, its pull requests, its descendants and 패널 열기.
function requestParts(tokens) {
  const {taskId, prChip, caption, spacer} = issueBoardParts(tokens);
  const small = num(tokens, '--size-control-sm');
  const mark = num(tokens, '--size-agent-mark');
  const chipMax = num(tokens, '--size-pane-child-chip-max');

  function requestGroup(id, label, count, rows, {folded = false} = {}) {
    return frame(id, label, {layout: 'vertical', gap: 0, width: 'fill_container'}, [
      frame(`${id}-head`, 'Head', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', height: num(tokens, '--size-control')}, [
        text(`${id}-label`, `${label} ${count}${folded ? ' · 펼치기' : ''}`, {size: '$--text-caption', weight: '500', fill: '$--subtle-foreground'}),
        ...(label === '쉬는 중' ? [icon(`${id}-fold`, folded ? 'chevron-right' : 'chevron-down', {size: num(tokens, '--size-icon-sm'), fill: '$--subtle-foreground'})] : []),
      ]),
      ...rows,
    ]);
  }

  function openChip(id, label) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', padding: [0, '$--spacing-xs'], cornerRadius: '$--radius-xs', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      icon(`${id}-g`, 'link', {size: num(tokens, '--size-icon-sm'), fill: '$--subtle-foreground'}),
      caption(`${id}-t`, label, '$--subtle-foreground', true),
    ]);
  }

  // `width` is the row's; the request line is cut to it the way the web cuts it:
  // the end keeps up to 40% from a word boundary, the front is cut with an ellipsis.
  function requestRow(id, row, width) {
    const [symbol, color] = AGENT_MARK[row.mark];
    const lineWidth = width - 2 * num(tokens, '--spacing-sm') - 24;
    const words = row.request.split(' ');
    let tail = '';
    for (let index = words.length - 1; index > 0; index -= 1) {
      const candidate = words.slice(index).join(' ');
      if (textWidth(candidate, 11) > lineWidth * 0.4) break;
      tail = candidate;
    }
    const whole = textWidth(row.request, 11) <= lineWidth;
    const head = whole || !tail ? row.request : row.request.slice(0, row.request.length - tail.length).trimEnd();
    const right = [
      ...(row.children ? [caption(`${id}-kids`, row.children, '$--muted-foreground', true), ...(row.asking ? [caption(`${id}-ask`, `· 질문 ${row.asking}`, '$--warning', true)] : [])] : []),
      ...(row.pr ? [prChip(`${id}-pr`, row.pr), ...(row.more ? [caption(`${id}-more`, `+${row.more}`, '$--muted-foreground', true)] : [])] : []),
      ...(row.issue ? [taskId(`${id}-issue`, row.issue)] : []),
      caption(`${id}-place`, fitText(row.place, chipMax, 11, true), '$--muted-foreground', true),
      caption(`${id}-age`, row.age, '$--muted-foreground', true),
    ];
    const lines = frame(id, row.title, {layout: 'vertical', gap: '$--spacing-xxs', width, padding: ['$--spacing-xs', '$--spacing-sm'], cornerRadius: '$--radius-xs', ...(row.hover ? {fill: '$--accent'} : {})}, [
      frame(`${id}-top`, 'Title line', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: 'fill_container'}, [
        screenStatusMark(tokens, `${id}-mark`, symbol, color),
        frame(`${id}-p`, 'Provider artwork', {width: mark, height: mark, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${row.provider ?? 'claude'}.png`, mode: 'fit'}}, []),
        text(`${id}-t`, row.title, {size: '$--text-body'}),
        ...(row.project ? [caption(`${id}-proj`, `· ${row.project}`)] : []),
        spacer(`${id}-sp`),
        ...right,
      ]),
      frame(`${id}-req`, 'Request line', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: 'fill_container'}, [
        caption(`${id}-who`, `${row.sender ?? '나'} ›`, '$--subtle-foreground'),
        text(`${id}-rh`, whole ? head : fitText(head, lineWidth - textWidth(tail, 11) - 6, 11), {size: '$--text-caption'}),
        ...(whole || !tail ? [] : [text(`${id}-rt`, tail, {size: '$--text-caption'})]),
        ...(row.later ? [screenBadge(`${id}-later`, `이후 ${row.later}`)] : []),
      ]),
      ...(row.result || row.opens ? [frame(`${id}-res`, 'Result line', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: 'fill_container'}, [
        text(`${id}-rs`, fitText(row.result ?? '', lineWidth - 90 * (row.opens?.length ?? 0), 11), {size: '$--text-caption', fill: row.mark === 'ask' ? '$--warning' : row.fix ? '$--destructive' : '$--muted-foreground'}),
        spacer(`${id}-rsp`),
        ...(row.opens ?? []).map((label, index) => openChip(`${id}-o${index}`, label)),
      ])] : []),
    ]);
    if (!row.expanded) return [lines];
    const detail = row.expanded;
    return [lines, frame(`${id}-x`, 'Expanded', {layout: 'vertical', gap: '$--spacing-sm', width, padding: [0, '$--spacing-sm', '$--spacing-sm', num(tokens, '--spacing-sm') + 2 * mark]}, [
      caption(`${id}-xwho`, `${row.sender ?? '나'} ›`, '$--subtle-foreground'),
      ...detail.request.map((line, index) => text(`${id}-xr${index}`, line || ' ', {size: '$--text-caption'})),
      ...detail.reply.map((line, index) => caption(`${id}-xa${index}`, line)),
      ...(detail.pulls ?? []).map((pull, index) => frame(`${id}-xp${index}`, `PR #${pull.number}`, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center'}, [
        pull.live ? prChip(`${id}-xpc${index}`, pull) : caption(`${id}-xpo${index}`, `예전 PR #${pull.number} 머지됨`),
        text(`${id}-xpt${index}`, pull.title, {size: '$--text-caption'}),
      ])),
      ...(detail.children ?? []).map((child, index) => frame(`${id}-xc${index}`, child.title, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: 'fill_container'}, [
        screenStatusMark(tokens, `${id}-xcm${index}`, AGENT_MARK[child.mark][0], AGENT_MARK[child.mark][1]),
        text(`${id}-xct${index}`, child.title, {size: '$--text-caption'}),
        caption(`${id}-xcv${index}`, child.verb, '$--subtle-foreground'),
        caption(`${id}-xcl${index}`, child.line),
        spacer(`${id}-xcsp${index}`),
        caption(`${id}-xco${index}`, '열기', '$--foreground'),
      ])),
      frame(`${id}-xopen`, 'Open', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
        screenButton(`${id}-xob`, '패널 열기', {variant: 'secondary', height: small, icon: 'square-arrow-out-up-right'}),
        caption(`${id}-xok`, '⌘↵'),
      ]),
    ])];
  }

  return {requestGroup, requestRow};
}

// The rows both request views draw: a question, a failed PR, a PR to review,
// a finished turn, a working agent, a parent waiting on its child, and the
// folded resting group.
const LONG_REQUEST = 'Overview 요청 보기에서 긴 요청을 한 줄로 보여 주세요 · request-row-….md 를 참고하고 · #336 리뷰도 함께 · 끝쪽 단어는 남겨 둘 것 · 이미지 1';
function requestRowsOf(project = null) {
  return {
    answer: {mark: 'ask', provider: 'codex', title: 'SIGTERM 처리와 자식 정리', project, request: '#192 hided가 SIGTERM에서 AI 자식부터 정리하게 해 줘', result: '기존 stdin 종료 경로도 남길까요?', place: '192-hided-sigterm-handler', age: '12m', issue: gh(192)},
    fix: {mark: 'seen', title: '탭 그룹 회귀 수정', project, request: 'CI 실패한 거 고쳐 줘', result: 'e2e 두 개를 고쳤습니다', fix: true, place: 'prd/agent-tab-groups', age: '8m', pr: {number: 217, tone: 'open', checks: 'failed'}, more: 1},
    review: {mark: 'seen', title: 'Herdr 서버 시작 구현', project, request: '#191 데스크톱 호스트가 Herdr 서버를 띄우게', result: 'PR 올림 · CI 통과', place: '191-desktop-starts-herdr', age: '20m', pr: {number: 222, tone: 'open', checks: 'passing'}, issue: gh(191)},
    stopped: {mark: 'seen', title: '설정 화면 스위치 추가', project, request: 'Settings에 에이전트 요약 스위치 넣어 줘', result: '테스트 환경이 없어 멈췄어요', place: 'prd/agent-summary-switch', age: '6m'},
    result: {mark: 'done', title: '설치 키트 항목 추가', project, request: 'Codex를 pane마다 실행하는 키트 항목 추가해 줘', result: '키트 항목을 추가했고 테스트가 통과했습니다', place: 'prd/codex-per-pane', age: '3m', opens: ['report', 'kit.rs']},
    working: {mark: 'work', title: '요청 보기 웹 화면 구현', project, request: LONG_REQUEST, result: '요청 보기 행을 그리는 중', place: 'prd/overview-request-view', age: '1m', later: 'ci-lead'},
    waiting: {mark: 'seen', title: 'SIGTERM 정리 오케스트레이션', project, request: '#192 SIGTERM 정리 맡겨서 끝까지 봐 줘', sender: '나', result: '리뷰어 결과를 기다리는 중', place: 'main', age: '25m', children: '자식 2 · 일하는 중 1', asking: 1},
  };
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

// The Overview (ProjectOverview.tsx, PRD agents-graph-view): the header with
// the tiles and the graph filter, 요청 as every entry opens it (PRD
// overview-request-view), its empty and nothing-to-do states, Agents (one graph, a
// box per checkout and a line per delegation), the box states and the filter
// that matches nothing, then the Issues tile's board and Dependencies mode,
// #218's Tasks board under its new name.
const SIGTERM_ASK = {mark: 'ask', provider: 'codex', title: 'SIGTERM 처리와 자식 정리', line: '기존 stdin 종료 경로도 남길까요?', age: '4m'};
const SIGTERM_REVIEW = {mark: 'work', title: '리뷰: 종료 경로 회귀', age: '2m'};
const TAB_GROUPS_IMPL = {mark: 'seen', provider: 'codex', title: 'Agent tab groups 구현', age: '8m'};
const CODEX_REST = {mark: 'seen', title: '체크아웃 기능 구현 및 정리', age: '2h'};

function buildProjectOverview(tokens) {
  const {column, taskCard, stageColumn, doneColumn, foldLine, arrow, legend, chain, preview, issuePanel} = issueBoardParts(tokens);
  const {prGroup, prRow} = prParts(tokens);
  const {requestGroup, requestRow} = requestParts(tokens);
  const graph = graphParts(tokens);
  const {g} = graph;
  const columns = 3;
  const width = columns * g.boxWidth + (columns - 1) * g.columnGap + 2 * g.pad;
  const boardWidth = 4 * column + 3 * num(tokens, '--spacing-md');
  function build(suffix) {
    const card = (id, value) => taskCard(`ov-${id}-${suffix}`, value);
    const header = (id, filter = {}) => overviewHeader(tokens, id, suffix, {project: 'herdr-ide', facts: HERDR_FACTS, view: 'agents', width, filter: key => graph.filter(key, filter)});

    // The graph (B1-B9, B21-B23): main's Observer delegates to a worktree that
    // asks (its line orange) and, through an Implementor that waits on a
    // reviewer, to a worktree that works (pale then blue, dashes flowing);
    // every box with a delegation stands level with the row that sent it, below
    // the boxes already in its column. The Implementor's folded children
    // are the badge on the row that sent it, the resting worktrees fold into
    // one line each.
    const mainBox = {primary: true, branch: 'main', purpose: 'Observer · 계획과 위임', agents: 3, rows: [
      {mark: 'seen', title: 'SIGTERM 정리 오케스트레이션', age: '20m'},
      {mark: 'seen', title: 'agent-tab-groups', age: '8m', tray: 'tab', tucked: [{symbol: '✓', color: '$--success', count: 2}]},
      {mark: 'work', title: 'Overview 진입 흐름', age: '1m', tray: 'tab'},
    ]};
    const askBox = {task: '#192', branch: '192-hided-sigterm-handler', purpose: '#192 SIGTERM 정리', pr: {number: 221, tone: 'draft', checks: 'failed', review: 'changes_requested'}, distance: '↑3', files: 4, selected: true, rows: [
      SIGTERM_ASK, {...SIGTERM_REVIEW, depth: 1},
    ]};
    const workBox = {branch: 'prd/agent-tab-groups', purpose: 'Agent tab groups', pr: {number: 217, tone: 'open', checks: 'passing'}, distance: '↑39 ↓17', rows: [TAB_GROUPS_IMPL]};
    const reviewBox = {branch: 'review/agent-tab-groups', purpose: '탭 그룹 회귀 리뷰', distance: '↑2', files: 2, rows: [
      {mark: 'work', title: '리뷰: 탭 그룹 회귀', age: '3m'},
    ]};
    const x = index => g.pad + index * (g.boxWidth + g.columnGap);
    const mainY = g.pad;
    const askY = Math.max(mainY, graph.levelY(graph.portY(mainY, mainBox.rows, 0), askBox.rows, 0));
    const workY = Math.max(graph.levelY(graph.portY(mainY, mainBox.rows, 1), workBox.rows, 0), askY + graph.boxHeightOf(askBox.rows) + g.boxGap);
    const reviewY = Math.max(graph.levelY(graph.portY(workY, workBox.rows, 0), reviewBox.rows, 0), g.pad);
    const height = Math.max(mainY + graph.boxHeightOf(mainBox.rows), askY + graph.boxHeightOf(askBox.rows), workY + graph.boxHeightOf(workBox.rows), reviewY + graph.boxHeightOf(reviewBox.rows)) + g.pad;
    const trunk = index => x(index) + g.boxWidth + g.columnGap / 2;
    const canvas = frame(`ov-gcanvas-${suffix}`, 'Graph', {layout: 'none', width, height}, [
      ...graph.edge(`ov-ge1-${suffix}`, {sx: x(0) + g.boxWidth, sy: graph.portY(mainY, mainBox.rows, 0), tx: x(1), ty: graph.portY(askY, askBox.rows, 0), trunk: trunk(0), kind: 'ask'}),
      ...graph.edge(`ov-ge2-${suffix}`, {sx: x(0) + g.boxWidth, sy: graph.portY(mainY, mainBox.rows, 1), tx: x(1), ty: graph.portY(workY, workBox.rows, 0), trunk: trunk(0), kind: 'wait'}),
      ...graph.edge(`ov-ge3-${suffix}`, {sx: x(1) + g.boxWidth, sy: graph.portY(workY, workBox.rows, 0), tx: x(2), ty: graph.portY(reviewY, reviewBox.rows, 0), trunk: trunk(1), kind: 'flow'}),
      graph.box(`ov-gb-main-${suffix}`, mainBox, {x: x(0), y: mainY}),
      graph.box(`ov-gb-ask-${suffix}`, askBox, {x: x(1), y: askY}),
      graph.box(`ov-gb-work-${suffix}`, workBox, {x: x(1), y: workY}),
      graph.box(`ov-gb-review-${suffix}`, reviewBox, {x: x(2), y: reviewY}),
    ]);
    const agents = frame(`ov-agents-${suffix}`, 'Project Overview · Agents', {layout: 'vertical', gap: '$--spacing-md', width}, [
      header('ov-ahead'),
      canvas,
      graph.legend(`ov-glegend-${suffix}`),
      frame(`ov-gfolds-${suffix}`, 'Folds', {layout: 'vertical', gap: '$--spacing-sm', width}, [
        graph.fold(`ov-gf0-${suffix}`, '에이전트 없는 워크트리 14', width),
        graph.fold(`ov-gf1-${suffix}`, '정리할 것 5', width),
        graph.fold(`ov-gf2-${suffix}`, '쉬는 체크아웃 2', width),
      ]),
    ]);

    // The box's states side by side (B12-B22): the primary box, a worktree with
    // its issue and PR chips (CI mark and 변경 요청), a merged box dimmed with
    // 정리, an asking row with its question, a row with a tucked badge, and the
    // box a way in selected.
    const stateCell = (key, label, node) => frame(`ov-gs-${key}-${suffix}`, label, {layout: 'vertical', gap: '$--spacing-xs', alignItems: 'start'}, [
      text(`ov-gs-${key}-cap-${suffix}`, label, {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}), node,
    ]);
    const box = (key, spec) => graph.box(`ov-gsb-${key}-${suffix}`, spec);
    const boxStates = frame(`ov-gstates-${suffix}`, 'Project Overview · Agents › Box states', {layout: 'vertical', gap: '$--spacing-lg', width}, [
      frame(`ov-gstates-r1-${suffix}`, 'Heads', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [
        stateCell('primary', '기본 박스 · main', box('primary', {primary: true, branch: 'main', purpose: 'Observer · 계획과 위임', agents: 2, rows: [
          {mark: 'seen', title: 'SIGTERM 정리 오케스트레이션', age: '20m'}, {mark: 'work', title: 'Overview 진입 흐름', age: '1m'},
        ]})),
        stateCell('pr', '워크트리 · 이슈 칩 + PR 칩', box('pr', {task: '#184', branch: '184-mailbox-plugin', purpose: 'Bundle mailbox as a Herdr plugin', pr: {number: 207, tone: 'open', checks: 'passing', review: 'changes_requested'}, distance: '↑5', files: 3, rows: [
          {mark: 'work', provider: 'codex', title: '리뷰 반영', age: '3m'},
        ]})),
        stateCell('merged', '머지됨 · 흐리게 + 정리', box('merged', {branch: 'fix/checkout-capability-follow-up', purpose: '체크아웃 권한 후속', cleanup: true, resting: true, pr: {number: 216, tone: 'merged'}, rows: [CODEX_REST]})),
      ]),
      frame(`ov-gstates-r2-${suffix}`, 'Rows', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [
        stateCell('ask', '묻는 행 · 질문 줄', box('ask', {task: '#192', branch: '192-hided-sigterm-handler', purpose: '#192 SIGTERM 정리', rows: [SIGTERM_ASK]})),
        stateCell('tucked', '접힌 하위의 배지 · ✓2', box('tucked', {branch: 'main', primary: true, purpose: 'Observer · 계획과 위임', agents: 1, rows: [
          {mark: 'work', title: 'agent-tab-groups', age: '8m', tucked: [{symbol: '✓', color: '$--success', count: 2}]},
        ]})),
        stateCell('selected', '선택한 박스', box('selected', {branch: 'prd/agent-tab-groups', purpose: 'Agent tab groups', distance: '↑39 ↓17', selected: true, rows: [TAB_GROUPS_IMPL]})),
      ]),
    ]);

    // Nothing matches (B28): `내 차례` lit and a search nothing satisfies, then
    // the one line that stands where the graph stood, with `필터 해제`.
    const filterFrame = frame(`ov-gfilter-${suffix}`, 'Project Overview · Agents › 필터 결과 없음', {layout: 'vertical', gap: '$--spacing-md', width}, [
      header('ov-fhead', {chips: [0], query: 'zzz'}),
      graph.filterEmpty(`ov-gempty-${suffix}`, width),
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
          card('r2', {task: gh(184), title: 'Bundle mailbox as a Herdr plugin with checkout lineage', branch: '184-mailbox-plugin', pr: {number: 207, checks: 'pending', review: 'changes_requested'}, agents: [
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
        card('s3', {task: gh(184), title: 'Bundle mailbox as a Herdr plugin with checkout lineage', branch: '184-mailbox-plugin', pr: {number: 207, checks: 'pending', review: 'changes_requested'}, hover: 'pr'}),
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
        byline: 'ana · 9월 26일 · 댓글 2',
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
      task: gh(192), title: ISSUE_192.title, stage: '진행 중', labels: [BUG], author: 'example · 9월 27일', updated: '9월 27일',
      work: {branch: ISSUE_192.branch, ahead: 3, files: 4, agents: [
        {mark: 'seen', title: 'SIGTERM 정리 오케스트레이션', age: '20m'},
        {mark: 'ask', provider: 'codex', title: 'SIGTERM 처리와 자식 정리 순서', line: '기존 stdin 종료 경로도 남길까요?', tone: 'request', age: '4m', depth: 1},
      ], pr: {number: 221, tone: 'draft', title: 'hided: stop AI children on SIGTERM before exit', review: 'changes_requested'}},
      body: [['h', '배경'], ['p', 'Found during the Swift removal (#188): hided installs no SIGTERM handler, so a background AI child is ended by the OS closing its stdin pipe.'], ['p', 'Add a graceful stop path: signal handler, owner-thread shutdown, child teardown with a bounded wait.']],
      comments: [['example · 9월 27일', '데스크톱 호스트 종료도 같은 경로로 가야 함']],
    }});
    const localPanel = withPanel('local', {name: 'Local', columns: [
      stageColumn(`ov-plb-${suffix}`, '백로그', 2, [
        card('pl-b1', {task: local(3), title: 'Overview 진입 흐름', selected: true}),
        card('pl-b2', {task: local(4), title: '세션 탭 빈 상태 문구'}),
      ], {newIssue: true}),
    ], spec: {
      task: local(3), title: 'Overview 진입 흐름', stage: '백로그', created: '9월 25일', updated: '9월 27일',
      body: [['p', '사이드바 프로젝트 이름으로 들어오면 Agents 그래프가 먼저 선다.'], ['p', '앞에 있던 체크아웃의 박스를 고른다.']],
    }});
    const editing = withPanel('edit', {name: 'Local 편집', columns: [
      stageColumn(`ov-peb-${suffix}`, '백로그', 2, [
        card('pe-b1', {task: local(3), title: 'Overview 진입 흐름', selected: true}),
        card('pe-b2', {task: local(4), title: '세션 탭 빈 상태 문구'}),
      ], {newIssue: true}),
    ], spec: {
      task: local(3), title: 'Overview 진입 흐름', stage: '백로그', created: '9월 25일', updated: '9월 27일',
      editing: {title: 'Overview 진입 흐름과 박스 선택', body: ['사이드바 프로젝트 이름으로 들어오면 Agents 그래프가 먼저 선다.', '앞에 있던 체크아웃의 박스를 고른다.']},
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
    // The request view as every entry opens it (PRD overview-request-view
    // B1-B8, B13, B52): the groups in their order with 쉬는 중 folded, a
    // working row's long request cut front and end, a finished row expanded,
    // then the empty view and the view with nothing to do.
    const rows = requestRowsOf();
    const r = (key, spec) => requestRow(`ov-rq-${key}-${suffix}`, spec, boardWidth);
    const requests = frame(`ov-requests-${suffix}`, 'Project Overview · 요청', {layout: 'vertical', gap: '$--spacing-md', width: boardWidth}, [
      overviewHeader(tokens, 'ov-rqhead', suffix, {project: 'herdr-ide', facts: HERDR_FACTS, view: 'requests', width: boardWidth}),
      requestGroup(`ov-rqg1-${suffix}`, '답할 것', 1, r('answer', rows.answer)),
      requestGroup(`ov-rqg2-${suffix}`, '고칠 것', 1, r('fix', rows.fix)),
      requestGroup(`ov-rqg3-${suffix}`, '리뷰·머지', 1, r('review', rows.review)),
      requestGroup(`ov-rqg35-${suffix}`, '멈춤', 1, r('stopped', rows.stopped)),
      requestGroup(`ov-rqg4-${suffix}`, '결과 볼 것', 1, r('result', {...rows.result, hover: true, expanded: {
        request: ['Codex를 pane마다 실행하는 키트 항목 추가해 줘', '', 'Settings › Devices 줄에서 끄고 켤 수 있게'],
        reply: ['키트 항목을 추가했고 테스트가 통과했습니다.', '보고서는 https://example.com/report 에 있습니다.'],
      }})),
      requestGroup(`ov-rqg5-${suffix}`, '일하는 중', 1, r('working', rows.working)),
      requestGroup(`ov-rqg6-${suffix}`, '기다리는 중', 1, r('waiting', {...rows.waiting, expanded: {
        request: ['#192 SIGTERM 정리 맡겨서 끝까지 봐 줘'],
        reply: ['리뷰어 결과를 기다리는 중'],
        pulls: [{number: 221, tone: 'draft', checks: 'pending', title: 'hided: stop AI children on SIGTERM before exit', live: true}, {number: 208, title: 'hided: graceful stop scaffolding', live: false}],
        children: [{mark: 'ask', title: 'SIGTERM 처리와 자식 정리', verb: '답할 것', line: '기존 stdin 종료 경로도 남길까요?'}, {mark: 'work', title: '리뷰: 종료 경로 회귀', verb: '일하는 중', line: '테스트를 돌리는 중'}],
      }})),
      requestGroup(`ov-rqg7-${suffix}`, '쉬는 중', 4, [], {folded: true}),
    ]);
    const requestStates = frame(`ov-rqstates-${suffix}`, 'Project Overview · 요청 › 빈 상태', {layout: 'vertical', gap: '$--spacing-md', width: boardWidth}, [
      frame(`ov-rqempty-${suffix}`, '에이전트 없음', {layout: 'vertical', gap: '$--spacing-sm', alignItems: 'center', width: boardWidth, padding: '$--spacing-xl'}, [
        text(`ov-rqempty-t-${suffix}`, '실행 중인 에이전트가 없습니다', {size: '$--text-caption', fill: '$--muted-foreground'}),
        screenButton(`ov-rqempty-b-${suffix}`, 'New agent', {variant: 'secondary', height: num(tokens, '--size-control'), icon: 'square-terminal'}),
      ]),
      frame(`ov-rqnone-${suffix}`, '할 일 없음', {layout: 'vertical', gap: '$--spacing-xs', width: boardWidth}, [
        text(`ov-rqnone-t-${suffix}`, '할 일 없음', {size: '$--text-caption', fill: '$--muted-foreground'}),
        requestGroup(`ov-rqnone-g-${suffix}`, '쉬는 중', 6, [], {folded: true}),
      ]),
    ]);
    return [
      frame(`ov-requestside-${suffix}`, '요청', {layout: 'vertical', gap: '$--spacing-xl'}, [requests, requestStates]),
      frame(`ov-agentside-${suffix}`, 'Agents', {layout: 'vertical', gap: '$--spacing-xl'}, [agents, boxStates, filterFrame]),
      frame(`ov-issueside-${suffix}`, 'Issues', {layout: 'vertical', gap: '$--spacing-xl'}, [issues, states, dependencies]),
      frame(`ov-panelside-${suffix}`, 'Issue panel', {layout: 'vertical', gap: '$--spacing-xl'}, [github, localPanel, editing, failed]),
      frame(`ov-prside-${suffix}`, 'PRs', {layout: 'vertical', gap: '$--spacing-xl'}, [prsView, prsConfirm]),
    ];
  }
  return screenSheet('screen-project-overview', 'Screen / Project Overview', OVERVIEW_SPEC, build, build);
}

// -- Screen / Workspace ---------------------------------------------------------

// The shared window template and explicit state differences are decoded from the
// reviewed Pen proposal. Every window keeps its real exported node ID. Master refs
// and local token overrides stay linked to the project library; no raster assets.
// Changes to this design should update this scoped factory and its Pen sheet together.
function buildWorkspace(tokens) {
  const windowTemplate = {
    "type": "frame",
    "id": "TUoDW",
    "name": "Workspace / Light / full1116",
    "clip": true,
    "width": 1456,
    "height": 900,
    "fill": "$--background",
    "layout": "none",
    "children": [
      {
        "type": "frame",
        "id": "vT0el",
        "x": 0,
        "y": 0,
        "name": "Native window chrome - comparison context",
        "width": 1456,
        "height": 28,
        "fill": "$--secondary",
        "stroke": "$--border",
        "strokeWidth": {
          "bottom": "$--size-hairline"
        },
        "strokeAlignment": "inner",
        "layout": "none",
        "children": [
          {
            "type": "ellipse",
            "id": "mDiyL",
            "x": 8,
            "y": 8,
            "name": "Inactive window control 1",
            "opacity": 0.5,
            "fill": "$--muted-foreground",
            "width": 12,
            "height": 12
          },
          {
            "type": "ellipse",
            "id": "sQ9MO",
            "x": 28,
            "y": 8,
            "name": "Inactive window control 2",
            "opacity": 0.5,
            "fill": "$--muted-foreground",
            "width": 12,
            "height": 12
          },
          {
            "type": "ellipse",
            "id": "rxHFj",
            "x": 48,
            "y": 8,
            "name": "Inactive window control 3",
            "opacity": 0.5,
            "fill": "$--muted-foreground",
            "width": 12,
            "height": 12
          },
          {
            "type": "text",
            "id": "JNfcl",
            "x": 714,
            "y": 5,
            "name": "Window title",
            "fill": "$--muted-foreground",
            "content": "hide",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-body",
            "fontWeight": "600"
          }
        ]
      },
      {
        "type": "frame",
        "id": "j82QG",
        "x": 0,
        "y": 28,
        "name": "Device rail",
        "width": 48,
        "height": 872,
        "fill": "$--sidebar",
        "stroke": "$--border",
        "strokeWidth": {
          "right": 1
        },
        "strokeAlignment": "inner",
        "layout": "vertical",
        "gap": 12,
        "padding": [
          8,
          4
        ],
        "children": [
          {
            "id": "NFivf",
            "type": "ref",
            "ref": "hideui:Nyvom",
            "name": "This Mac",
            "width": 40,
            "height": 40,
            "stroke": "$--foreground",
            "strokeWidth": 2,
            "cornerRadius": "$--radius-lg",
            "descendants": {
              "hideui:ZIZFR": {
                "fill": "$--subtle-foreground",
                "icon": "laptop"
              }
            }
          },
          {
            "id": "XHA5O",
            "type": "ref",
            "ref": "hideui:Nyvom",
            "name": "Add device",
            "width": 40,
            "height": 32,
            "stroke": "$--border",
            "strokeWidth": 1,
            "cornerRadius": "$--radius-lg",
            "descendants": {
              "hideui:ZIZFR": {
                "fill": "$--subtle-foreground",
                "icon": "plus"
              }
            }
          }
        ]
      },
      {
        "type": "frame",
        "id": "E6s0TD",
        "x": 48,
        "y": 28,
        "name": "Projects sidebar",
        "width": 292,
        "height": 872,
        "fill": "$--sidebar",
        "stroke": "$--border",
        "strokeWidth": {
          "right": 1
        },
        "strokeAlignment": "inner",
        "layout": "none",
        "children": [
          {
            "type": "frame",
            "id": "mFUBL",
            "x": 0,
            "y": 0,
            "name": "This Mac header",
            "width": 292,
            "height": 32,
            "stroke": "$--border",
            "strokeWidth": {
              "bottom": "$--size-hairline"
            },
            "strokeAlignment": "inner",
            "gap": 8,
            "padding": [
              0,
              12
            ],
            "alignItems": "center",
            "children": [
              {
                "type": "text",
                "id": "di8S5",
                "name": "Device title",
                "fill": "$--foreground",
                "textGrowth": "fixed-width",
                "width": "fill_container",
                "content": "This Mac",
                "fontFamily": "$--font-ui",
                "fontSize": "$--text-title",
                "fontWeight": "600"
              },
              {
                "id": "Rfg8S",
                "type": "ref",
                "ref": "hideui:Nyvom",
                "name": "Add project",
                "descendants": {
                  "hideui:ZIZFR": {
                    "fill": "$--subtle-foreground",
                    "icon": "plus"
                  }
                }
              },
              {
                "id": "Y02KdW",
                "type": "ref",
                "ref": "hideui:Nyvom",
                "name": "Search",
                "descendants": {
                  "hideui:ZIZFR": {
                    "fill": "$--subtle-foreground",
                    "icon": "search"
                  }
                }
              }
            ]
          },
          {
            "type": "frame",
            "id": "jAdn6",
            "x": 0,
            "y": 32,
            "name": "Sidebar tab strip",
            "width": 292,
            "height": 32,
            "stroke": "$--border",
            "strokeWidth": {
              "bottom": "$--size-hairline"
            },
            "strokeAlignment": "inner",
            "gap": 8,
            "padding": [
              0,
              12
            ],
            "alignItems": "center",
            "children": [
              {
                "type": "text",
                "id": "OYRm7",
                "name": "Selected tab",
                "fill": "$--foreground",
                "content": "Projects",
                "fontFamily": "$--font-ui",
                "fontSize": "$--text-body",
                "fontWeight": "normal"
              },
              {
                "type": "text",
                "id": "KGCa7",
                "name": "Other tab",
                "fill": "$--muted-foreground",
                "content": "Agents",
                "fontFamily": "$--font-ui",
                "fontSize": "$--text-body",
                "fontWeight": "normal"
              }
            ]
          },
          {
            "type": "frame",
            "id": "sv6IB",
            "x": 4,
            "y": 68,
            "name": "Home",
            "width": 284,
            "height": 36,
            "gap": 8,
            "padding": [
              0,
              8
            ],
            "alignItems": "center",
            "children": [
              {
                "type": "icon",
                "id": "h2d0u",
                "name": "Home icon",
                "width": 14,
                "height": 14,
                "icon": "house",
                "library": "lucide",
                "fill": "$--subtle-foreground"
              },
              {
                "type": "text",
                "id": "pv35D",
                "name": "Home title",
                "fill": "$--foreground",
                "textGrowth": "fixed-width",
                "width": "fill_container",
                "content": "Home",
                "fontFamily": "$--font-ui",
                "fontSize": "$--text-title",
                "fontWeight": "600"
              },
              {
                "type": "text",
                "id": "NDnyJ",
                "name": "Project count",
                "fill": "$--muted-foreground",
                "content": "0 projects",
                "fontFamily": "$--font-ui",
                "fontSize": "$--text-body",
                "fontWeight": "normal"
              }
            ]
          },
          {
            "type": "text",
            "id": "c4b0sQ",
            "x": 12,
            "y": 110,
            "name": "Recent activity",
            "fill": "$--muted-foreground",
            "content": "Projects · Recent activity · 1",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-micro",
            "fontWeight": "600"
          },
          {
            "type": "frame",
            "id": "HObxT",
            "x": 4,
            "y": 124,
            "name": "Selected checkout",
            "width": 284,
            "height": 36,
            "fill": "$--secondary",
            "cornerRadius": "$--radius-sm",
            "gap": 8,
            "padding": [
              0,
              8
            ],
            "alignItems": "center",
            "children": [
              {
                "type": "icon",
                "id": "qQH68",
                "name": "Checkout folder",
                "width": 14,
                "height": 14,
                "icon": "folder",
                "library": "lucide",
                "fill": "$--subtle-foreground"
              },
              {
                "type": "text",
                "id": "swzTQ",
                "name": "Checkout label",
                "fill": "$--foreground",
                "content": "fixture",
                "fontFamily": "$--font-ui",
                "fontSize": "$--text-body",
                "fontWeight": "600"
              }
            ]
          },
          {
            "type": "frame",
            "id": "F0TmV",
            "x": 0,
            "y": 840,
            "name": "Sidebar footer",
            "width": 292,
            "height": 32,
            "stroke": "$--border",
            "strokeWidth": {
              "top": 1
            },
            "strokeAlignment": "inner",
            "gap": 8,
            "padding": [
              0,
              12
            ],
            "justifyContent": "end",
            "alignItems": "center",
            "children": [
              {
                "id": "VnRz8",
                "type": "ref",
                "ref": "hideui:Nyvom",
                "name": "Background usage",
                "descendants": {
                  "hideui:ZIZFR": {
                    "fill": "$--subtle-foreground",
                    "icon": "activity"
                  }
                }
              },
              {
                "id": "ydsLS",
                "type": "ref",
                "ref": "hideui:Nyvom",
                "name": "Settings",
                "descendants": {
                  "hideui:ZIZFR": {
                    "fill": "$--subtle-foreground",
                    "icon": "settings"
                  }
                }
              }
            ]
          }
        ]
      },
      {
        "id": "W2zeQP",
        "type": "ref",
        "ref": "hideui:VMZTz",
        "name": "Shared Workspace toolbar",
        "fill": "$--sidebar",
        "stroke": "$--border",
        "x": 340,
        "y": 28,
        "width": 1116,
        "height": 32,
        "descendants": {
          "hideui:bkh81": {
            "fill": "$--subtle-foreground",
            "content": "Home  /  fixture  /  main"
          },
          "hideui:Kjfje/hideui:side-panel-tools-toggle/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button": {
            "fill": "$--secondary"
          },
          "hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge": {
            "fill": "$--primary",
            "enabled": false
          },
          "hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-count": {
            "content": "1"
          },
          "hideui:Kjfje/hideui:XHe9v/hideui:OaJhM": {
            "fill": "$--secondary"
          },
          "hideui:Kjfje/hideui:XHe9v/hideui:OaJhM/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          }
        }
      },
      {
        "id": "DtqSr",
        "type": "ref",
        "ref": "hideui:side-panel",
        "name": "Docked columns",
        "fill": "$--background",
        "x": 340,
        "y": 60,
        "width": 1116,
        "height": 840,
        "descendants": {
          "hideui:YpxMC": {
            "fill": "$--background",
            "enabled": true,
            "x": 0,
            "y": 0,
            "width": 480,
            "height": 840
          },
          "hideui:YpxMC/hideui:m3nqv": {
            "fill": "$--card",
            "stroke": "$--border"
          },
          "hideui:YpxMC/hideui:Yf5g6": {
            "fill": "$--secondary"
          },
          "hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-mark": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-title": {
            "fill": "$--foreground"
          },
          "hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-strike": {
            "fill": "$--muted-foreground"
          },
          "hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-dirty": {
            "fill": "$--warning"
          },
          "hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-close/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:ujfpg/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:GYdsi/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:jEy0G": {
            "fill": "$--background"
          },
          "hideui:YpxMC/hideui:blLvs": {
            "fill": "$--secondary",
            "stroke": "$--border"
          },
          "hideui:YpxMC/hideui:W2jHfs": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:v57l6H": {
            "fill": "$--foreground"
          },
          "hideui:YpxMC/hideui:CG4wC": {
            "fill": "$--muted-foreground"
          },
          "hideui:YpxMC/hideui:K8ikC/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:a63n6t/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:jPwvc": {
            "fill": "$--background"
          },
          "hideui:YpxMC/hideui:PNaQy": {
            "fill": "$--foreground"
          },
          "hideui:YpxMC/hideui:aTsYy": {
            "fill": "$--background",
            "stroke": "$--border"
          },
          "hideui:YpxMC/hideui:lFQa0": {
            "fill": "$--card",
            "stroke": "$--border"
          },
          "hideui:YpxMC/hideui:ljIwg": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:e7uYn": {
            "fill": "$--foreground"
          },
          "hideui:YpxMC/hideui:uJYZa": {
            "fill": "$--muted-foreground"
          },
          "hideui:YpxMC/hideui:x9QjtJ/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:E0Mk6/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:YpxMC/hideui:M6Mnyn": {
            "fill": "$--background"
          },
          "hideui:YpxMC/hideui:QSc1K": {
            "fill": "$--foreground"
          },
          "hideui:gPjhF": {
            "enabled": true,
            "x": 480,
            "y": 0,
            "width": 8,
            "height": 840
          },
          "hideui:gPjhF/hideui:side-panel-grip-line-top": {
            "fill": "$--muted-foreground"
          },
          "hideui:gPjhF/hideui:side-panel-grip-pill": {
            "fill": "$--card",
            "stroke": "$--border"
          },
          "hideui:gPjhF/hideui:side-panel-grip-glyph": {
            "fill": "$--muted-foreground"
          },
          "hideui:giyPa": {
            "fill": "$--card",
            "enabled": true,
            "x": 488,
            "y": 0,
            "width": 360,
            "height": 840
          },
          "hideui:giyPa/hideui:tPjjv": {
            "fill": "$--card",
            "stroke": "$--border"
          },
          "hideui:giyPa/hideui:b7bsc7": {
            "fill": "$--card",
            "width": 304
          },
          "hideui:giyPa/hideui:b7bsc7/hideui:view-tab-mark": {
            "fill": "$--file-blue"
          },
          "hideui:giyPa/hideui:b7bsc7/hideui:view-tab-title": {
            "fill": "$--foreground"
          },
          "hideui:giyPa/hideui:b7bsc7/hideui:view-tab-strike": {
            "fill": "$--muted-foreground"
          },
          "hideui:giyPa/hideui:b7bsc7/hideui:view-tab-dirty": {
            "fill": "$--warning"
          },
          "hideui:giyPa/hideui:b7bsc7/hideui:view-tab-close/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:giyPa/hideui:N6jVdg/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:giyPa/hideui:eeRGB/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:giyPa/hideui:fenJg": {
            "fill": "$--card",
            "stroke": "$--border"
          },
          "hideui:giyPa/hideui:fenJg/hideui:BUboy": {
            "fill": "$--subtle-foreground",
            "content": "… 제목과 경로 확인.md"
          },
          "hideui:giyPa/hideui:fenJg/hideui:ifLpE/hideui:btn-ic": {
            "fill": "$--primary-foreground"
          },
          "hideui:giyPa/hideui:fenJg/hideui:ifLpE/hideui:btn-lb": {
            "fill": "$--foreground"
          },
          "hideui:giyPa/hideui:fenJg/hideui:W8pA8k/hideui:btn-ic": {
            "fill": "$--primary-foreground"
          },
          "hideui:giyPa/hideui:fenJg/hideui:W8pA8k/hideui:btn-lb": {
            "fill": "$--foreground"
          },
          "hideui:giyPa/hideui:fenJg/hideui:iFSdQ/hideui:btn-ic": {
            "fill": "$--primary-foreground"
          },
          "hideui:giyPa/hideui:fenJg/hideui:iFSdQ/hideui:btn-lb": {
            "fill": "$--subtle-foreground"
          },
          "hideui:giyPa/hideui:fenJg/hideui:lEjtc/hideui:btn-ic": {
            "fill": "$--primary-foreground"
          },
          "hideui:giyPa/hideui:fenJg/hideui:lEjtc/hideui:btn-lb": {
            "fill": "$--subtle-foreground"
          },
          "hideui:giyPa/hideui:nFrdf": {
            "fill": "$--card"
          },
          "hideui:giyPa/hideui:aX9gF": {
            "fill": "$--secondary"
          },
          "hideui:giyPa/hideui:jOt0p": {
            "fill": "$--muted-foreground"
          },
          "hideui:giyPa/hideui:iExJ4": {
            "fill": "$--file-blue"
          },
          "hideui:giyPa/hideui:T1KFmN": {
            "fill": "$--muted-foreground"
          },
          "hideui:giyPa/hideui:sfXAM": {
            "fill": "$--foreground"
          },
          "hideui:giyPa/hideui:h10qC": {
            "fill": "$--muted-foreground"
          },
          "hideui:giyPa/hideui:fzHJn": {
            "fill": "$--foreground",
            "content": "한글과 English가 함께 있는 파일을 읽습니다."
          },
          "hideui:giyPa/hideui:GQ6yZ": {
            "fill": "$--muted-foreground"
          },
          "hideui:giyPa/hideui:oVfkk": {
            "fill": "$--foreground"
          },
          "hideui:w7GZ7c": {
            "enabled": true,
            "x": 848,
            "y": 0,
            "width": 8,
            "height": 840
          },
          "hideui:w7GZ7c/hideui:side-panel-grip-line-top": {
            "fill": "$--muted-foreground"
          },
          "hideui:w7GZ7c/hideui:side-panel-grip-pill": {
            "fill": "$--card",
            "stroke": "$--border"
          },
          "hideui:w7GZ7c/hideui:side-panel-grip-glyph": {
            "fill": "$--muted-foreground"
          },
          "hideui:eNvgI": {
            "fill": "$--card",
            "enabled": true,
            "x": 856,
            "y": 0,
            "width": 260,
            "height": 840
          },
          "hideui:eNvgI/hideui:H2M1v0": {
            "stroke": "$--border"
          },
          "hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-explorer": {
            "stroke": "$--primary"
          },
          "hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-explorer-glyph": {
            "fill": "$--foreground"
          },
          "hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-history-glyph": {
            "fill": "$--subtle-foreground"
          },
          "hideui:eNvgI/hideui:b22eS": {
            "stroke": "$--border"
          },
          "hideui:eNvgI/hideui:d927zh": {
            "fill": "$--subtle-foreground"
          },
          "hideui:eNvgI/hideui:DVJS6/hideui:ZIZFR": {
            "fill": "$--subtle-foreground"
          },
          "hideui:eNvgI/hideui:NZoEx": {
            "stroke": "$--border",
            "enabled": false,
            "height": 0,
            "width": 0
          },
          "hideui:eNvgI/hideui:iDnaZ": {
            "fill": "$--muted-foreground"
          },
          "hideui:eNvgI/hideui:MmldP": {
            "stroke": "$--border"
          },
          "hideui:eNvgI/hideui:Cwgap": {
            "fill": "$--file-blue"
          },
          "hideui:eNvgI/hideui:x1im3": {
            "fill": "$--foreground",
            "fontSize": "$--text-caption"
          },
          "hideui:eNvgI/hideui:fZi7M": {
            "stroke": "$--border"
          },
          "hideui:eNvgI/hideui:t0QOE": {
            "fill": "$--file-blue"
          },
          "hideui:eNvgI/hideui:aHlvp": {
            "fill": "$--foreground",
            "fontSize": "$--text-caption",
            "content": "한글과 English 작업 기록 - 긴 파일 제…"
          }
        }
      }
    ]
  };
  const states = [
    {"id":"TUoDW","mode":"Light","key":"full1116","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["TUoDW","vT0el","mDiyL","sQ9MO","rxHFj","JNfcl","j82QG","NFivf","XHA5O","E6s0TD","mFUBL","di8S5","Rfg8S","Y02KdW","jAdn6","OYRm7","KGCa7","sv6IB","h2d0u","pv35D","NDnyJ","c4b0sQ","HObxT","qQH68","swzTQ","F0TmV","VnRz8","ydsLS","W2zeQP","DtqSr"],"changes":[]},
    {"id":"PUDa5","mode":"Light","key":"mid1100","window":[1440,900],"body":1100,"visibleWidths":{"agents":480,"views":612,"tools":0},"openFileCount":1,"ids":["PUDa5","do6Xq","A0zOxv","ATZUh","WzX2i","i20ww","keYir","pMIs8","Wusym","XlqMm","OhD8s","R4J80","PXhqH","BTjWb","A2xtWy","f5eq5W","BVAgn","BPRAZ","hM2NT","PQu9f","C7H6gy","SkaIK","hrP2Y","z85Ehu","gnUYi","piBwT","d9xAM","UJsFv","r4zjY5","fjTt5"],"changes":[{"path":["children",0,"children",3,"x"],"value":706},{"path":["children",0,"width"],"value":1440},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",3,"width"],"value":1100},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":1108},{"path":["children",4,"descendants","hideui:giyPa","width"],"value":612},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":1100},{"path":["children",4,"width"],"value":1100},{"path":["name"],"value":"Workspace / Light / mid1100"},{"path":["width"],"value":1440}]},
    {"id":"e7GLci","mode":"Light","key":"mid848","window":[1188,900],"body":848,"visibleWidths":{"agents":480,"views":360,"tools":0},"openFileCount":1,"ids":["e7GLci","vz6lz","zVQGC","kahQR","LOm0g","K01Xq","F7TgXs","iLgOv","mLgAQ","FjPNn","p01YEE","ESnQC","rH7HS","fH7lb","XkxSU","Mm0p0","PIfKg","ON8hj","JQhkG","i1hVs4","FgxJy","QX59A","jIiPK","v4eXnS","M8Q2A","SjVw4","q31JuN","HZ2HD","u6Y1IX","HcIQ7"],"changes":[{"path":["children",0,"children",3,"x"],"value":580},{"path":["children",0,"width"],"value":1188},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",3,"width"],"value":848},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"width"],"value":848},{"path":["name"],"value":"Workspace / Light / mid848"},{"path":["width"],"value":1188}]},
    {"id":"wU6O3","mode":"Light","key":"narrow847agents","window":[1187,900],"body":847,"visibleWidths":{"agents":847,"views":0,"tools":0},"openFileCount":1,"ids":["wU6O3","O9JUq","XEKGT","PfYYC","AnWkc","U84r63","rgA0s","x3bCyj","Gdc8u","tARwm","F6Di99","cPxQV","EXdY1","IgOFJ","rQhwq","KT7b3","B0tNd","MeJlh","clMvA","SbbbC","dPBC5","Pt9Ah","XxJ0E","MMCLY","DdFbY","J1ISi7","j76tDF","sWiJq","X1RUk","MZ4ZX"],"changes":[{"path":["children",0,"children",3,"x"],"value":579.5},{"path":["children",0,"width"],"value":1187},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"width"],"value":847},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":847},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":855},{"path":["children",4,"descendants","hideui:gPjhF","enabled"],"value":false},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":847},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":847},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":847},{"path":["children",4,"width"],"value":847},{"path":["name"],"value":"Workspace / Light / narrow847agents"},{"path":["width"],"value":1187}]},
    {"id":"l6fpKj","mode":"Light","key":"narrow847views","window":[1187,900],"body":847,"visibleWidths":{"agents":0,"views":847,"tools":0},"openFileCount":1,"ids":["l6fpKj","HQ6SS","K3L13b","xmFd1","mPgRM","BXZpN","ioMIf","HpLPw","yc8X0","XzcpN","d6jROt","avk0e","AvcQN","LFvl8","S4hQO","hynVp","TroX8","rp8qE","D3yQeZ","osWqi","UZDzv","DkdUU","sRY6j","K5TuJo","E6TYSv","BrkeT","LNf9A","Jsx1c","e6Ozqa","sahiH"],"changes":[{"path":["children",0,"children",3,"x"],"value":579.5},{"path":["children",0,"width"],"value":1187},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",3,"width"],"value":847},{"path":["children",4,"descendants","hideui:YpxMC","enabled"],"value":false},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":847},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":855},{"path":["children",4,"descendants","hideui:gPjhF","enabled"],"value":false},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":0},{"path":["children",4,"descendants","hideui:giyPa","width"],"value":847},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":0},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":847},{"path":["children",4,"width"],"value":847},{"path":["name"],"value":"Workspace / Light / narrow847views"},{"path":["width"],"value":1187}]},
    {"id":"aoV0a","mode":"Light","key":"mid1100tools","window":[1440,900],"body":1100,"visibleWidths":{"agents":737,"views":0,"tools":355},"openFileCount":1,"ids":["aoV0a","zDLCi","ztEhg","Hjkup","F61zLZ","mw2X4","A4Cp3","l2WaGq","uQFGt","YlulQ","h7aSBI","ybhx9","MDGQY","r6B7Lt","j3M7MQ","w7QHo2","ELGF1","M9h2p","W41Xr","sH43B","ucQ37","auZaQ","I6zeHR","jekQ3","s2ho7","z2TKo4","VJofE","V136Si","nQqVo","KCq7h"],"changes":[{"path":["children",0,"children",3,"x"],"value":706},{"path":["children",0,"width"],"value":1440},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"width"],"value":1100},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":737},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":745},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":737},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":488},{"path":["children",4,"width"],"value":1100},{"path":["name"],"value":"Workspace / Light / mid1100tools"},{"path":["width"],"value":1440}]},
    {"id":"QJQsr","mode":"Light","key":"mid848tools","window":[1188,900],"body":848,"visibleWidths":{"agents":485,"views":0,"tools":355},"openFileCount":1,"ids":["QJQsr","fS5am","Gsg2l","Enf1X","x1Ko4c","n8cdF","eVee5","eN8MU","I9AAcW","Yf0Il","c6BbC","agrS2","QrHVK","T212xl","sraVI","m6c21Y","UmonT","ILiZy","tWmvY","DwHaH","g8YfS","oNL8A","G2J0Oz","nEW0f","R2ixny","X8nJT","DD39j","JUbCY","hguxV","xqsrN"],"changes":[{"path":["children",0,"children",3,"x"],"value":580},{"path":["children",0,"width"],"value":1188},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"width"],"value":848},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":485},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":493},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":485},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":488},{"path":["children",4,"width"],"value":848},{"path":["name"],"value":"Workspace / Light / mid848tools"},{"path":["width"],"value":1188}]},
    {"id":"OdCvV","mode":"Light","key":"narrow847tools","window":[1187,900],"body":847,"visibleWidths":{"agents":0,"views":0,"tools":847},"openFileCount":1,"ids":["OdCvV","y3IyH0","dAGe7","ktzql","ZdikY","r1iT7Y","x3p6Kk","wKi8I","ek5dK","iBhjp","DEgE1","ZwBpe","VxtaJ","kCFm1","HC9ww","MUe8s","rKvDw","PeEHM","I2NY5U","JZZgH","XtayH","SL2xL","pqohb","DvEAp","AlGIU","xyYRX","w4OGU","Y3LDHn","xa0Cy","AL6Wa"],"changes":[{"path":["children",0,"children",3,"x"],"value":579.5},{"path":["children",0,"width"],"value":1187},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"width"],"value":847},{"path":["children",4,"descendants","hideui:YpxMC","enabled"],"value":false},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":847},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":847},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":0},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","enabled"],"value":false},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":0},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":0},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":0},{"path":["children",4,"width"],"value":847},{"path":["name"],"value":"Workspace / Light / narrow847tools"},{"path":["width"],"value":1187}]},
    {"id":"PmKT5","mode":"Light","key":"toolszero","window":[1456,900],"body":1116,"visibleWidths":{"agents":753,"views":0,"tools":355},"openFileCount":0,"ids":["PmKT5","FuHUy","MBro6","zBlnI","cCflh","wzmiI","fCwVW","g86tY","YsPwb","bZFJP","NcoAY","UNT0u","P6f46","EywVA","F1z8N","qMDKg","ICvwN","JuJrg","bhrtH","QvgD1","BRuHK","T93o69","wO5wW","BD3O5","MQEGk","PPGRI","v0fzbi","ItbQR","XyPFU","XEdY8"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-count","content"],"value":"0"},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":753},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":761},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":753},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":761},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":761},{"path":["name"],"value":"Workspace / Light / toolszero"}]},
    {"id":"jEUuK","mode":"Light","key":"toolshiddenfile","window":[1456,900],"body":1116,"visibleWidths":{"agents":753,"views":0,"tools":355},"openFileCount":1,"ids":["jEUuK","NRbxh","Up8D1","WYjPW","O9IdOq","tcmXN","ejvfd","RwSJG","Re1gc","NxjdC","j92NQS","cLOxu","hlR0t","m2TQUy","t2JEa","Uuwy2","DgOev","rML9g","tSeEd","o3OZQ","wUCzi","lJ3a3","sCBVw","DGGgW","YRt8i","z3Ma7","zDgGn","dGUCQ","r5KQox","HVkDE"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":753},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":761},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":753},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":761},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":761},{"path":["name"],"value":"Workspace / Light / toolshiddenfile"}]},
    {"id":"DRb4n","mode":"Light","key":"dividerhover","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["DRb4n","aaTJ1","B8KRC","dEWrC","mSYXw","XyNRb","UZTqB","XsOz8","grCCD","x3VLW","HBzHV","v4WkhA","uEkie","d4erS","Koq7O","s8wznc","Qs4bQ","xlzXW","Po0Yy","VeG1E","Pi4GX","qpTnV","XbhFO","VwRXI","S0E5Qf","ZyOpL","lQKP1","q2pn0T","bV4cT","YDwUf"],"changes":[{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","height"],"value":840},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","y"],"value":408},{"path":["name"],"value":"Workspace / Light / dividerhover"}]},
    {"id":"sPxvx","mode":"Light","key":"dividerfocus","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["sPxvx","QQVEE","Ph5vx","Go93y","SENyY","nkm0C","c1OJBF","pNuxp","qvqrN","O6aOiC","a8NRA","l7NfT","oIOAb","cAuBY","TNPBu","OJGB8","e5vK3W","FioQ2","B9x6o","tffiV","OqoNP","HAtvW","N87rh","ATA8H","Qk1AU","Z5azt","BptWt","ZWGxq","CcKmE","Mnq1V"],"changes":[{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","height"],"value":840},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","stroke"],"value":"$--border"},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","y"],"value":408},{"path":["name"],"value":"Workspace / Light / dividerfocus"}]},
    {"id":"D3d5BU","mode":"Light","key":"dividerguide","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["D3d5BU","xlhgo","mj1iF","CPMwk","RXft7","KeWuF","REeGf","h9XzAW","FMfLu","N8Pw9","D4tzk","r0L6eA","mIhdS","xm1pt","a5nq2","i2FEP6","A09LQ","iBArK","hFjYu","UV6EW","i3hCtG","ZEa4C","zJRxJ","RkHyN","qVJCM","b6OlZD","ocTkR","x9IuOt","s3ksu","OlvXJ"],"changes":[{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","height"],"value":840},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","y"],"value":408},{"path":["children",5],"value":{"id":"HzVMM","type":"ref","ref":"hideui:side-panel-grip","name":"Temporary divider guide","x":852,"y":60,"width":8,"height":840,"descendants":{"hideui:side-panel-grip-line-top":{"fill":"$--muted-foreground","enabled":true,"height":840},"hideui:side-panel-grip-pill":{"fill":"$--card","stroke":"$--border","enabled":true,"y":408},"hideui:side-panel-grip-glyph":{"fill":"$--muted-foreground"}}}},{"path":["name"],"value":"Workspace / Light / dividerguide"}]},
    {"id":"d6YJdY","mode":"Light","key":"dividerrelease","window":[1456,900],"body":1116,"visibleWidths":{"agents":512,"views":596,"tools":0},"openFileCount":1,"ids":["d6YJdY","bsNeH","x4qidL","X8loM","NGctU","U4GvJ8","EQBgn","t2v4A","b2VsX","NwNBc","Lw339","f9Z5Wt","bQ3wt","llMUa","g882uE","m12WwT","ZZaL5","Ktlv3","ZnPWA","H7QlVN","C5PDs","E9mrC","QocHU","Ke1LQ","Evqfu","qtabo","TafRU","iVyiH","Ne2iu","ekAYc"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":512},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":1124},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":512},{"path":["children",4,"descendants","hideui:giyPa","width"],"value":596},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":520},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":1116},{"path":["name"],"value":"Workspace / Light / dividerrelease"}]},
    {"id":"ec1uu","mode":"Light","key":"dividercancel","window":[1940,900],"body":1600,"visibleWidths":{"agents":589,"views":640,"tools":355},"openFileCount":1,"ids":["ec1uu","eEMUc","e2TeTa","PzxQi","nYVOd","sqoI1","M8oSel","UyGio","biVeo","EHJyG","bi49h","l0y9Jm","s73UZ3","bnFuB","IE7lW","PiUC5","x59Vm","gjjwC","dP3sW","rVJft","FwBBm","wo0BA","J4Ak8","DV39t","W2UAfG","M6XZm","L4BiVy","TeO74","Ruf45","KrKXg"],"changes":[{"path":["children",0,"children",3,"x"],"value":956},{"path":["children",0,"width"],"value":1940},{"path":["children",2,"children",3,"content"],"value":"Projects · Recent activity · 2"},{"path":["children",2,"children",4,"children",1,"content"],"value":"beta"},{"path":["children",3,"descendants","hideui:bkh81","content"],"value":"Home  /  beta  /  main"},{"path":["children",3,"width"],"value":1600},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":589},{"path":["children",4,"descendants","hideui:YpxMC/hideui:aTsYy","enabled"],"value":false},{"path":["children",4,"descendants","hideui:YpxMC/hideui:aTsYy","height"],"value":780},{"path":["children",4,"descendants","hideui:YpxMC/hideui:aTsYy","width"],"value":0},{"path":["children",4,"descendants","hideui:YpxMC/hideui:v57l6H","content"],"value":"w2:p1"},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":1245},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":589},{"path":["children",4,"descendants","hideui:giyPa","width"],"value":640},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":597},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":1237},{"path":["children",4,"width"],"value":1600},{"path":["name"],"value":"Workspace / Light / dividercancel"},{"path":["width"],"value":1940}]},
    {"id":"OkoMU","mode":"Light","key":"longpathhover","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["OkoMU","x5tRd","PacrB","ts4FW","S2Fg3","uM71H","HeS3x","FVE1R","bNtOg","JM6Ki","P0NW8T","ja2cH","cDUiX","hZNaf","h3dhZ","ORUbg","CUJPq","xCUps","lk5F7","IBgU5","gdqIG","m8emt","arGs3","DZ4yX","j9vxae","EBDfy","n8DsA","b1RYB","L2sgn","s638L"],"changes":[{"path":["children",5],"value":{"id":"w6Ykvp","type":"ref","ref":"hideui:tip-open-l","name":"Full file identity tooltip","fill":"$--popover","stroke":"$--border","x":739,"y":96,"width":360,"height":"fit_content","descendants":{"hideui:tip-open-l-t":{"fill":"$--foreground","content":"File: /workspace/fixture/한국어와 English 작업 기록/한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md · Preview","name":"Actual native file identity","width":"fill_container","textGrowth":"fixed-width","fontWeight":"normal"}}}},{"path":["name"],"value":"Workspace / Light / longpathhover"}]},
    {"id":"V6UYh","mode":"Light","key":"views-tooltip","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["V6UYh","YblEd","h5Vazd","PcjCJ","FbRBI","t3GRBQ","bFDCw","M25NO","XAa87","rZs7d","MwKlA","VMusL","P0OvX","Wv6cF","uM74R","e0ZMHw","JHhu7","loQTy","UrJCT","nzM1F","ODcup","B17CM","T6LdxI","Y0Vees","sWzTz","XXxt8","y6GPr","JOOFN","nQbN6","raWzV"],"changes":[{"path":["children",5],"value":{"id":"Y81nx","type":"ref","ref":"hideui:tip-shortcut-l","name":"File Views tooltip","fill":"$--popover","stroke":"$--border","x":1332,"y":60,"descendants":{"hideui:tip-shortcut-l-t":{"fill":"$--foreground","content":"File Views","name":"File Views"},"hideui:tip-shortcut-l-k":{"fill":"$--muted-foreground","content":"⇧⌘B","name":"⇧⌘B"}}}},{"path":["name"],"value":"Workspace / Light / toolbarhints"}]},
    {"id":"X7Fcc","mode":"Dark","key":"full1116","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["X7Fcc","xSWYt","EWggy","iludN","oF7H1","Vt3Nu","YuyMn","JIRsZ","ETRdh","x3KXtp","bvCMq","hUI2z","T5HGp","L1X8Tr","ilbS0","MT33P","i1onXy","j9UIfy","yGxcG","XHUlP","quni8","wokvt","Mqmgr","R9g0lT","k519J","iEz6j","ztrwz","l79l0Q","Aj3R1","hae0G"],"changes":[{"path":["name"],"value":"Workspace / Dark / full1116"}]},
    {"id":"J3ckk6","mode":"Dark","key":"mid1100","window":[1440,900],"body":1100,"visibleWidths":{"agents":480,"views":612,"tools":0},"openFileCount":1,"ids":["J3ckk6","qIhMH","jXEVs","yj3AX","EGZNL","c64W1","wvz0E","KutwH","x9zTLj","H8ys6O","lV5VT","jcBGi","QSKEr","NTqgJ","K5QiC0","uW0PU","KfIgO","bkhkB","cMSOu","x8cftG","OOgfV","czRJl","c4PAeR","X0pQL","Wca3B","JnhcW","bQK7q","s3O4V","DneBV","AviT1"],"changes":[{"path":["children",0,"children",3,"x"],"value":706},{"path":["children",0,"width"],"value":1440},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",3,"width"],"value":1100},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":1108},{"path":["children",4,"descendants","hideui:giyPa","width"],"value":612},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":1100},{"path":["children",4,"width"],"value":1100},{"path":["name"],"value":"Workspace / Dark / mid1100"},{"path":["width"],"value":1440}]},
    {"id":"fbxi3","mode":"Dark","key":"mid848","window":[1188,900],"body":848,"visibleWidths":{"agents":480,"views":360,"tools":0},"openFileCount":1,"ids":["fbxi3","ijx5y","SiDhH","E1HIQ","sFXCu","Hw9ox","AxK1U","g1pyK","smCyt","Hz0uy","NAnCd","DpTz8","OmEtx","ODKyO","mmmwG","EPcRq","BH3qw","HfJpv","y5yau","Hok4y","z2QRE","biLw2","p7ssr","t8rpDR","iQfNQ","jXbgZ","UwYaw","xcVLf","vRFQe","kpB8k"],"changes":[{"path":["children",0,"children",3,"x"],"value":580},{"path":["children",0,"width"],"value":1188},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",3,"width"],"value":848},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"width"],"value":848},{"path":["name"],"value":"Workspace / Dark / mid848"},{"path":["width"],"value":1188}]},
    {"id":"Z6s9U8","mode":"Dark","key":"narrow847agents","window":[1187,900],"body":847,"visibleWidths":{"agents":847,"views":0,"tools":0},"openFileCount":1,"ids":["Z6s9U8","AcpxQ","naLSB","cEkes","mwUjJ","T431E","PPQnp","cz9u1","u7VVm1","J0sOlw","d1ubx","vuj7X","dPh0I","kZ3yA","pRPLM","eQmJQ","cmQ9R","oFmAY","HHXMg","KHFNb","d3fkrn","D6ICMs","BBcMF","zXkDs","qsGTb","sdA3w","W6ueJ1","PersO","LiSYT","jSY3P"],"changes":[{"path":["children",0,"children",3,"x"],"value":579.5},{"path":["children",0,"width"],"value":1187},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"width"],"value":847},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":847},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":855},{"path":["children",4,"descendants","hideui:gPjhF","enabled"],"value":false},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":847},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":847},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":847},{"path":["children",4,"width"],"value":847},{"path":["name"],"value":"Workspace / Dark / narrow847agents"},{"path":["width"],"value":1187}]},
    {"id":"Ql0b5","mode":"Dark","key":"narrow847views","window":[1187,900],"body":847,"visibleWidths":{"agents":0,"views":847,"tools":0},"openFileCount":1,"ids":["Ql0b5","ajcGp","Mv0Ph","PgkXh","SvO2F","hsWNW","q6h7k","pNFj8","elzvI","Ioc2s","UeEud","fLT07","oo3bP","rJU03","y6EQtY","y4mki8","s35w1","wylU7","MrBQb","l9STyt","w0e3t","NzG8f","MeaGe","NGz63","oN0Eh","DYJbA","t2wv28","f8LVqh","Q9VJH","xYHKW"],"changes":[{"path":["children",0,"children",3,"x"],"value":579.5},{"path":["children",0,"width"],"value":1187},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",3,"width"],"value":847},{"path":["children",4,"descendants","hideui:YpxMC","enabled"],"value":false},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":847},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":855},{"path":["children",4,"descendants","hideui:gPjhF","enabled"],"value":false},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":0},{"path":["children",4,"descendants","hideui:giyPa","width"],"value":847},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":0},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":847},{"path":["children",4,"width"],"value":847},{"path":["name"],"value":"Workspace / Dark / narrow847views"},{"path":["width"],"value":1187}]},
    {"id":"nBxva","mode":"Dark","key":"mid1100tools","window":[1440,900],"body":1100,"visibleWidths":{"agents":737,"views":0,"tools":355},"openFileCount":1,"ids":["nBxva","KkXP8","g7gnF","J1DwH","cpSAL","ggJGk","m82sA","Io5Ii","uv9m2","FyQlO","kYstM","N9hLo","TLkSm","uwhgP","M2XMV","f6Y5vY","MMkTu","Wh3OM","xrJbM","kIeLi","fDWkS","bJez6","h2GYVn","aGpXe","WaArP","zkQNe","MgIvy","xlEiO","XnKR2","MnlxC"],"changes":[{"path":["children",0,"children",3,"x"],"value":706},{"path":["children",0,"width"],"value":1440},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"width"],"value":1100},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":737},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":745},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":737},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":488},{"path":["children",4,"width"],"value":1100},{"path":["name"],"value":"Workspace / Dark / mid1100tools"},{"path":["width"],"value":1440}]},
    {"id":"V4UiI","mode":"Dark","key":"mid848tools","window":[1188,900],"body":848,"visibleWidths":{"agents":485,"views":0,"tools":355},"openFileCount":1,"ids":["V4UiI","SWFW9","HIIqu","s8Hn5","mKESL","YX6Q1","pZVGl","FFm0y","B6Ubsg","fMrF1","wBqrE","OSkgp","dJ3VU","e9Pt8p","Ypy1G","AqfPY","j4jPUv","x5XicG","UoG9A","Egjym","zUquX","buC3A","z3Edxd","LhC9h","Esw0M","s2wgzN","BBAao","So12G","lpszu","Z9IPYk"],"changes":[{"path":["children",0,"children",3,"x"],"value":580},{"path":["children",0,"width"],"value":1188},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"width"],"value":848},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":485},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":493},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":485},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":488},{"path":["children",4,"width"],"value":848},{"path":["name"],"value":"Workspace / Dark / mid848tools"},{"path":["width"],"value":1188}]},
    {"id":"Dz9IR","mode":"Dark","key":"narrow847tools","window":[1187,900],"body":847,"visibleWidths":{"agents":0,"views":0,"tools":847},"openFileCount":1,"ids":["Dz9IR","Wr9OS","G6Nfm","Rn7nw","vpCrE","PoPOH","cULY3","lVGGz","vXN7r","bEzhB","nKncM","U47yf","Ft1Zn","U00f1Q","XDHns","M9e9M","gUdfo","nZwFL","HzGgk","Rmhs0","VeNMx","XpcDx","J3mrcU","USX6Y","lZ1OA","vDkyN","IEJyA","L7P7jP","jExGZ","o2Oc5"],"changes":[{"path":["children",0,"children",3,"x"],"value":579.5},{"path":["children",0,"width"],"value":1187},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"width"],"value":847},{"path":["children",4,"descendants","hideui:YpxMC","enabled"],"value":false},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":847},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":847},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":0},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","enabled"],"value":false},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":0},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":0},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":0},{"path":["children",4,"width"],"value":847},{"path":["name"],"value":"Workspace / Dark / narrow847tools"},{"path":["width"],"value":1187}]},
    {"id":"a8qXDY","mode":"Dark","key":"toolszero","window":[1456,900],"body":1116,"visibleWidths":{"agents":753,"views":0,"tools":355},"openFileCount":0,"ids":["a8qXDY","YE9Qq","JbRLu","jzMVJ","Y4Svf","hySbN","H8tE9g","HVJe3","Fcvxo","y4y9c","XeZUW","GfHL1","l7bNIU","Wei01","k4TgO","fZi0H","H1l5m","CJxKQ","GTxNO","rSHUO","IB2md","QvQ6s","E9vq1G","j20Doj","q9B4L","H56hdj","AxFRY","xqle6","OcMTT","DAMag"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-count","content"],"value":"0"},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":753},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":761},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":753},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":761},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":761},{"path":["name"],"value":"Workspace / Dark / toolszero"}]},
    {"id":"Kk60A","mode":"Dark","key":"toolshiddenfile","window":[1456,900],"body":1116,"visibleWidths":{"agents":753,"views":0,"tools":355},"openFileCount":1,"ids":["Kk60A","S4peZ","EOo5W","KSVZq","WXATa","deT2R","Vxngs","w2DGuU","NqcSV","uOJ3P","yMBtz","MYU7v","OaU1d","GxP7h","nIW10","L29bhV","asj4x","NfCgC","B51rqs","w0LKDp","V9uM1J","c7KciP","rHona","O61bS","GtYyK","j3zopa","kpIcn","xADYu","w5rNR1","dOyvA"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge","enabled"],"value":true},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","fill"],"value":[]},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":753},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":761},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":753},{"path":["children",4,"descendants","hideui:giyPa","enabled"],"value":false},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":761},{"path":["children",4,"descendants","hideui:giyPa/hideui:b7bsc7","width"],"delete":true},{"path":["children",4,"descendants","hideui:giyPa/hideui:fenJg/hideui:BUboy","content"],"delete":true},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":761},{"path":["name"],"value":"Workspace / Dark / toolshiddenfile"}]},
    {"id":"uhq8e","mode":"Dark","key":"dividerhover","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["uhq8e","T3qDd","cUsH6","tvvSG","s6rYIa","k4JXVp","F22Q2","HMQJz","WSeq2","PLfTC","YhDgv","YtfoO","Wdmna","oiKqp","nx61m","VaR1J","g6gq6","cEfyc","PK3Iy","G24v5c","Cniay","ps7OC","ssMkC","ueols","favAG","C0B47C","FKTNu","BnlnV","S9YkR","Re1tY"],"changes":[{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","height"],"value":840},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","y"],"value":408},{"path":["name"],"value":"Workspace / Dark / dividerhover"}]},
    {"id":"eI9Uf","mode":"Dark","key":"dividerfocus","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["eI9Uf","B6tvm","WO7Sz","NwKXS","b0JjLN","vqjZn","L7pzP","CsSRF","W5UnK","etnxJ","hXqH3","DF8cd","g6kF0v","flNDz","qcsGY","a28xP","dSNlf","UrpK2","zo28A","bQiCk","BBBA6","q7s9F","f7qDt","LAoiB","x2zgyW","qCiMG","c0G5w5","gPqqa","XG5hI","pGIVX"],"changes":[{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","height"],"value":840},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","stroke"],"value":"$--border"},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","y"],"value":408},{"path":["name"],"value":"Workspace / Dark / dividerfocus"}]},
    {"id":"DPFSq","mode":"Dark","key":"dividerguide","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["DPFSq","w9gY4G","Sgjwb","pMOih","tXmni","sb6fW","HiTAX","jQY3r","SKV2s","eeiLZ","O9cEr","VkPK0","akwsS","u2Dksi","OytVS","wz8d5","r43GE","Vm57g","z99vD","A84cc","aLBvB","GkzOh","c6ESwj","vvxi9","jw2h8","QoxlP","SNIjU","BJOO8","a9WT2k","STctY"],"changes":[{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-line-top","height"],"value":840},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","enabled"],"value":true},{"path":["children",4,"descendants","hideui:gPjhF/hideui:side-panel-grip-pill","y"],"value":408},{"path":["children",5],"value":{"id":"dhyqM","type":"ref","ref":"hideui:side-panel-grip","name":"Temporary divider guide","x":852,"y":60,"width":8,"height":840,"descendants":{"hideui:side-panel-grip-line-top":{"fill":"$--muted-foreground","enabled":true,"height":840},"hideui:side-panel-grip-pill":{"fill":"$--card","stroke":"$--border","enabled":true,"y":408},"hideui:side-panel-grip-glyph":{"fill":"$--muted-foreground"}}}},{"path":["name"],"value":"Workspace / Dark / dividerguide"}]},
    {"id":"N8yit","mode":"Dark","key":"dividerrelease","window":[1456,900],"body":1116,"visibleWidths":{"agents":512,"views":596,"tools":0},"openFileCount":1,"ids":["N8yit","pIBZg","e9RBr","RjxAo","FdI2i","kgcsL","yWfSc","XqLLE","iiSei","svoic","DT0n5","PfuLO","N7GoP","noQDW","DVCMf","uyd6N","x10ras","frre7","ejwBU","UCrSF","lJalK","YBc8y","HGBfD","qfg7H","C6KkS","h0ADX","l6cFsx","i7phN","p8hdt","hmu6a"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","fill"],"value":[]},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":512},{"path":["children",4,"descendants","hideui:eNvgI","enabled"],"value":false},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":1124},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":512},{"path":["children",4,"descendants","hideui:giyPa","width"],"value":596},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":520},{"path":["children",4,"descendants","hideui:w7GZ7c","enabled"],"value":false},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":1116},{"path":["name"],"value":"Workspace / Dark / dividerrelease"}]},
    {"id":"YIxRN","mode":"Dark","key":"dividercancel","window":[1940,900],"body":1600,"visibleWidths":{"agents":589,"views":640,"tools":355},"openFileCount":1,"ids":["YIxRN","B2RbFU","NlEKy","CafJB","Wb8SU","Gb7bd","r9d0YB","zAzRe","M1V3je","U8dRPI","bIXIO","sgA9Q","YsBar","P6aMF","x0z8A","EtjPI","RxzPj","KW5JV","EsDzq","ypymv","F18tb","q1Rmif","H56Q5","RVFHf","MUsv4","Y820X","Qvuxu","Rj1e9","gaf4J","C687Ki"],"changes":[{"path":["children",0,"children",3,"x"],"value":956},{"path":["children",0,"width"],"value":1940},{"path":["children",2,"children",3,"content"],"value":"Projects · Recent activity · 2"},{"path":["children",2,"children",4,"children",1,"content"],"value":"beta"},{"path":["children",3,"descendants","hideui:bkh81","content"],"value":"Home  /  beta  /  main"},{"path":["children",3,"width"],"value":1600},{"path":["children",4,"descendants","hideui:YpxMC","width"],"value":589},{"path":["children",4,"descendants","hideui:YpxMC/hideui:aTsYy","enabled"],"value":false},{"path":["children",4,"descendants","hideui:YpxMC/hideui:aTsYy","height"],"value":780},{"path":["children",4,"descendants","hideui:YpxMC/hideui:aTsYy","width"],"value":0},{"path":["children",4,"descendants","hideui:YpxMC/hideui:v57l6H","content"],"value":"w2:p1"},{"path":["children",4,"descendants","hideui:eNvgI","width"],"value":355},{"path":["children",4,"descendants","hideui:eNvgI","x"],"value":1245},{"path":["children",4,"descendants","hideui:eNvgI/hideui:aHlvp","content"],"value":"한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md"},{"path":["children",4,"descendants","hideui:gPjhF","x"],"value":589},{"path":["children",4,"descendants","hideui:giyPa","width"],"value":640},{"path":["children",4,"descendants","hideui:giyPa","x"],"value":597},{"path":["children",4,"descendants","hideui:w7GZ7c","x"],"value":1237},{"path":["children",4,"width"],"value":1600},{"path":["name"],"value":"Workspace / Dark / dividercancel"},{"path":["width"],"value":1940}]},
    {"id":"fDgAU","mode":"Dark","key":"longpathhover","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["fDgAU","I6Fz0o","dJCwv","SEHfI","kIkme","gMxUN","ClZOI","kmjxb","F53lX","J4pFFA","rCoRu","Z6UEK3","WUPLJ","BRMdP","L4gdg3","fzdJu","IIpHb","TPaUb","hU3mm","GOtqS","QxUL7","j0Ux6","ahsaV","NmTzo","iZKD9","M4Ycl","D4XHA","VamER","ZGeLc","xfNIx"],"changes":[{"path":["children",5],"value":{"id":"cQEw5","type":"ref","ref":"hideui:tip-open-l","name":"Full file identity tooltip","fill":"$--popover","stroke":"$--border","x":739,"y":96,"width":360,"height":"fit_content","descendants":{"hideui:tip-open-l-t":{"fill":"$--foreground","content":"File: /workspace/fixture/한국어와 English 작업 기록/한글과 English 작업 기록 - 긴 파일 제목과 경로 확인.md · Preview","name":"Actual native file identity","width":"fill_container","textGrowth":"fixed-width","fontWeight":"normal"}}}},{"path":["name"],"value":"Workspace / Dark / longpathhover"}]},
    {"id":"u7vlOH","mode":"Dark","key":"views-tooltip","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["u7vlOH","hx9OA","Grlmv","kReFs","miTEo","l6ZmGe","pBzIy","r2B6Y","lbEQq","zaCuj","nwOVx","d8cFy","t8rDk","mHqtL","dIIaJ","aJgF6","wA0sN","JJMaJ","Hsar2","kmPKQ","j69Sx","W35ji","j4FGQD","WenQA","AuuB8","c7zAa","S8Onh","m9TMO","Iw2wz","gvQSX"],"changes":[{"path":["children",5],"value":{"id":"brw2Q","type":"ref","ref":"hideui:tip-shortcut-l","name":"File Views tooltip","fill":"$--popover","stroke":"$--border","x":1332,"y":60,"descendants":{"hideui:tip-shortcut-l-t":{"fill":"$--foreground","content":"File Views","name":"File Views"},"hideui:tip-shortcut-l-k":{"fill":"$--muted-foreground","content":"⇧⌘B","name":"⇧⌘B"}}}},{"path":["name"],"value":"Workspace / Dark / toolbarhints"}]},
    {"id":"Z6h2wQ","mode":"Light","key":"views-focus","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["Z6h2wQ","l4vs7","X2j4mm","s7XPrR","v1BYS","C29wUo","i6IS8","CM5Ye","ttmT4","UMbHJ","vYik3","uQMtF","oWSHR","NoNa4","p4lcP","ZgkOx","WZqV2","FyLg0","d1xaPV","dakYa","iqFiZ","M4xK2i","RiuxC","PpQgx","CZzD6","ceCJS","MnMYs","rKYpX","btUG7","l9WfA"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","stroke"],"value":"$--ring"},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","strokeAlignment"],"value":"outer"},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","strokeWidth"],"value":1},{"path":["children",5],"value":{"id":"UFK98","type":"ref","ref":"hideui:tip-shortcut-l","name":"File Views tooltip","fill":"$--popover","stroke":"$--border","x":1332,"y":60,"descendants":{"hideui:tip-shortcut-l-t":{"fill":"$--foreground","content":"File Views","name":"File Views"},"hideui:tip-shortcut-l-k":{"fill":"$--muted-foreground","content":"⇧⌘B","name":"⇧⌘B"}}}},{"path":["name"],"value":"Workspace / Light / views-focus"}]},
    {"id":"KLhGG","mode":"Light","key":"tools-tooltip","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["KLhGG","QanvV","GvEwy","d5pksU","k6ZfY","s8JXo","OdKXW","Z28FrA","CloNd","a43cA","EY99d","yplre","IccJd","nnZWD","Jaozv","fwlX0","dETup","l7Y1j6","v843V3","OPvNY","CcIpF","r8eX3","S2Nbp","x1YXTm","ODi4T","ZKi5g","g0vE5","L3EyO","a59fDV","JNF6r"],"changes":[{"path":["children",5],"value":{"id":"LgfQY","type":"ref","ref":"hideui:tip-shortcut-l","name":"Tools tooltip","fill":"$--popover","stroke":"$--border","x":1368,"y":60,"descendants":{"hideui:tip-shortcut-l-t":{"fill":"$--foreground","content":"Tools","name":"Tools"},"hideui:tip-shortcut-l-k":{"fill":"$--muted-foreground","content":"⌘E","name":"⌘E"}}}},{"path":["name"],"value":"Workspace / Light / tools-tooltip"}]},
    {"id":"obv9L","mode":"Light","key":"tools-focus","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["obv9L","HM2wg","tevZB","lpEQi","QKR6a","GtYXV","P7kRRJ","tIzgN","agWco","SEiHP","Jx5D2","sgmUg","ZhoLC","y692qZ","dOxN6","m9gd0t","Huhtg","NyAJh","XaIEw","uUJ5J","OwIgy","oaptw","BlDwy","QFHU0","zSx41","tFqIS","cIf3a","aR3eZ","mGTQl","M2FmQ"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","stroke"],"value":"$--ring"},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","strokeAlignment"],"value":"outer"},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","strokeWidth"],"value":1},{"path":["children",5],"value":{"id":"x42r4n","type":"ref","ref":"hideui:tip-shortcut-l","name":"Tools tooltip","fill":"$--popover","stroke":"$--border","x":1368,"y":60,"descendants":{"hideui:tip-shortcut-l-t":{"fill":"$--foreground","content":"Tools","name":"Tools"},"hideui:tip-shortcut-l-k":{"fill":"$--muted-foreground","content":"⌘E","name":"⌘E"}}}},{"path":["name"],"value":"Workspace / Light / tools-focus"}]},
    {"id":"EctRK","mode":"Light","key":"server-tooltip","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["EctRK","pQehc","r5whC","iMxTY","iQbIX","A8Eund","CW7tM","bxKGb","PsvFR","zTQ6Z","qFnyz","y1q0u","Yvd3r","xuNm0","sdU4E","b9Mvfy","bgn8s","MpU0X","QjuC8","bbHlJ","Zi2SL","iHL3C","BlDel","eXDmn","qHamE","KrHmj","o4bNy1","ukiuy","B7lGHA","jfa7g"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:side-panel-tools-toggle"],"value":{"fill":"$--secondary"}},{"path":["children",5],"value":{"id":"UrmEF","type":"ref","ref":"hideui:tip-open-l","name":"Open server tooltip","fill":"$--popover","stroke":"$--border","x":1320,"y":60,"descendants":{"hideui:tip-open-l-t":{"fill":"$--foreground","content":"Open server","name":"Open server"}}}},{"path":["name"],"value":"Workspace / Light / server-tooltip"}]},
    {"id":"tOJtb","mode":"Light","key":"server-focus","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["tOJtb","lDuuv","ENukt","KNGuB","ZbCXk","c5DXY4","VqIj2","IhG74","f2AsAj","qjq2s","sBo7V","rzHCi","sCaum","QUZ0G","RJKvi","G9Arxl","rV0fy","L60SgX","Uw3pS","UAeki","b6e86","S15pg6","E9UZEF","vPxeK","V1IaG","dziAs","XnSVC","EKvzN","J9e9F","T4QAvn"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:side-panel-tools-toggle"],"value":{"stroke":"$--ring","strokeWidth":1,"strokeAlignment":"outer","fill":"$--secondary"}},{"path":["children",5],"value":{"id":"OBLQQ","type":"ref","ref":"hideui:tip-open-l","name":"Open server tooltip","fill":"$--popover","stroke":"$--border","x":1320,"y":60,"descendants":{"hideui:tip-open-l-t":{"fill":"$--foreground","content":"Open server","name":"Open server"}}}},{"path":["name"],"value":"Workspace / Light / server-focus"}]},
    {"id":"I5MCOS","mode":"Dark","key":"views-focus","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["I5MCOS","ubM5F","GZyBz","qQtDv","OuYc6","hhWFP","E2fl25","BxSaD","WIQjb","x6QurC","pwA4Y","fabik","wv54a","fyimM","JwxaA","nGhco","OyAJU","XlQJu","l0OC16","kBQVJ","hTz67","m8MAgM","RF6kA","WvdND","uTcG8","Ct6D2","BE5Go","bMgDP","I1D9V","F06qKC"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","stroke"],"value":"$--ring"},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","strokeAlignment"],"value":"outer"},{"path":["children",3,"descendants","hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button","strokeWidth"],"value":1},{"path":["children",5],"value":{"id":"C1UNFi","type":"ref","ref":"hideui:tip-shortcut-l","name":"File Views tooltip","fill":"$--popover","stroke":"$--border","x":1332,"y":60,"descendants":{"hideui:tip-shortcut-l-t":{"fill":"$--foreground","content":"File Views","name":"File Views"},"hideui:tip-shortcut-l-k":{"fill":"$--muted-foreground","content":"⇧⌘B","name":"⇧⌘B"}}}},{"path":["name"],"value":"Workspace / Dark / views-focus"}]},
    {"id":"m3oMSt","mode":"Dark","key":"tools-tooltip","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["m3oMSt","brCGR","mBl8y","W15AR","S6TpF","SFLca","n2f9Kh","u4hEd","CnJeJ","AxY5q","s4vrix","u5jXC","Mlctg","ptqY1","Z8QK6r","pXT2b","qYMKy","u9lo8","WeZH5","uwD5D","Vw5SS","rJEI6","Xj8ol","R6YT5","WoQVx","b2KE5Y","m1ywqD","y6dQc0","vF17I","MOyZC"],"changes":[{"path":["children",5],"value":{"id":"Q3xTU","type":"ref","ref":"hideui:tip-shortcut-l","name":"Tools tooltip","fill":"$--popover","stroke":"$--border","x":1368,"y":60,"descendants":{"hideui:tip-shortcut-l-t":{"fill":"$--foreground","content":"Tools","name":"Tools"},"hideui:tip-shortcut-l-k":{"fill":"$--muted-foreground","content":"⌘E","name":"⌘E"}}}},{"path":["name"],"value":"Workspace / Dark / tools-tooltip"}]},
    {"id":"dZ0c4","mode":"Dark","key":"tools-focus","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["dZ0c4","QxaDH","A9AiQ","J02Au","F4Yjm","tLV19","bhcGw","e2PEn","GsGK2","BIr9n","biol0","Dj7os","ncYoR","xtYOH","A5Dha","n9mHf","U2sH7","BOnta","sDOyW","GBH20","Q8tQDx","d6nTn0","Z8IRj","baKCt","Ox6Mq","vAmI8","lIUdt","dmV3i","arkOL","GjPeI"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","stroke"],"value":"$--ring"},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","strokeAlignment"],"value":"outer"},{"path":["children",3,"descendants","hideui:Kjfje/hideui:XHe9v/hideui:OaJhM","strokeWidth"],"value":1},{"path":["children",5],"value":{"id":"H3iU3J","type":"ref","ref":"hideui:tip-shortcut-l","name":"Tools tooltip","fill":"$--popover","stroke":"$--border","x":1368,"y":60,"descendants":{"hideui:tip-shortcut-l-t":{"fill":"$--foreground","content":"Tools","name":"Tools"},"hideui:tip-shortcut-l-k":{"fill":"$--muted-foreground","content":"⌘E","name":"⌘E"}}}},{"path":["name"],"value":"Workspace / Dark / tools-focus"}]},
    {"id":"OKgI3","mode":"Dark","key":"server-tooltip","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["OKgI3","m4iF9t","m323z","qscy0","X3P5G","SNbTp","aLkGO","R6cY5","a8F2dL","dy3Q3","ps5RH","VF1Iw","ywCKD","Ka43Q","gp9f2","W387Tn","RLzat","O9XiSL","O45l4","helbU","CnhPb","Xg1ub","wOtn0","dCK9y","ZGH7e","i6A7M","SXjia","JzVmK","z9bOs","nLNPI"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:side-panel-tools-toggle"],"value":{"fill":"$--secondary"}},{"path":["children",5],"value":{"id":"DMGu4","type":"ref","ref":"hideui:tip-open-l","name":"Open server tooltip","fill":"$--popover","stroke":"$--border","x":1320,"y":60,"descendants":{"hideui:tip-open-l-t":{"fill":"$--foreground","content":"Open server","name":"Open server"}}}},{"path":["name"],"value":"Workspace / Dark / server-tooltip"}]},
    {"id":"KB1kt","mode":"Dark","key":"server-focus","window":[1456,900],"body":1116,"visibleWidths":{"agents":480,"views":360,"tools":260},"openFileCount":1,"ids":["KB1kt","j31j36","YmE70","X5Auz","JBK8O","m8OdbW","V8Xup9","h85z3","t1o2TK","fuekS","wAreH","o8YzL","QYsSH","v0vEjg","l8CTzO","Y693r8","lbfnX","nhxXs","I3tmy","yj5TW","Ouq8n","iWz53","mPfGb","cEssB","XNsWX","PNanN","WhP9r","qbotF","eWx75","fiGFm"],"changes":[{"path":["children",3,"descendants","hideui:Kjfje/hideui:side-panel-tools-toggle"],"value":{"stroke":"$--ring","strokeWidth":1,"strokeAlignment":"outer","fill":"$--secondary"}},{"path":["children",5],"value":{"id":"O31Ij","type":"ref","ref":"hideui:tip-open-l","name":"Open server tooltip","fill":"$--popover","stroke":"$--border","x":1320,"y":60,"descendants":{"hideui:tip-open-l-t":{"fill":"$--foreground","content":"Open server","name":"Open server"}}}},{"path":["name"],"value":"Workspace / Dark / server-focus"}]}
  ];
  const sheet = {
    "type": "frame",
    "id": "screen-workspace",
    "x": 0,
    "y": 32635,
    "name": "Screen / Workspace",
    "width": 2020,
    "fill": "#EDEDEE",
    "layout": "vertical",
    "gap": "$--spacing-xl",
    "padding": "$--spacing-xl",
    "children": [
      {
        "type": "text",
        "id": "wlf1z",
        "name": "Workspace title",
        "fill": "$--foreground",
        "content": "Workspace",
        "fontFamily": "$--font-ui",
        "fontSize": "$--text-headline",
        "fontWeight": "600"
      },
      {
        "type": "text",
        "id": "p47Nai",
        "name": "Workspace contract",
        "fill": "$--muted-foreground",
        "textGrowth": "fixed-width",
        "width": 1940,
        "content": "PRD #321 A: Agents | File Views | Tools. Shared toolbar; independent controls; File badge only while hidden. Minima 480/360/260, divider 8, thresholds1116/848. Narrow calls substitute a column. Every scene below is linked to the copied component library; no Tools overlay, Pin or Expand.",
        "fontFamily": "$--font-ui",
        "fontSize": "$--text-body",
        "fontWeight": "normal"
      },
      {
        "type": "frame",
        "id": "g0S6az",
        "name": "Light",
        "theme": {
          "Mode": "Light"
        },
        "width": 1972,
        "fill": "$--background",
        "layout": "vertical",
        "gap": 16,
        "padding": 16,
        "children": [
          {
            "type": "text",
            "id": "i8NtD",
            "name": "Theme",
            "fill": "$--foreground",
            "content": "Light",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-headline",
            "fontWeight": "600"
          },
          {
            "type": "text",
            "id": "u344ck",
            "name": "Full body 1116 - docked minima",
            "fill": "$--foreground",
            "content": "Full body 1116 - docked minima",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "TUoDW"
          },
          {
            "type": "text",
            "id": "bvuU6",
            "name": "1440 window, sidebar open - body 1100 mid fallback",
            "fill": "$--foreground",
            "content": "1440 window, sidebar open - body 1100 mid fallback",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "PUDa5"
          },
          {
            "type": "text",
            "id": "l32P9H",
            "name": "Body 848 - two columns at minima",
            "fill": "$--foreground",
            "content": "Body 848 - two columns at minima",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "e7GLci"
          },
          {
            "type": "text",
            "id": "D5BQE",
            "name": "Body 847 - Agents base, stored File View hidden",
            "fill": "$--foreground",
            "content": "Body 847 - Agents base, stored File View hidden",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "wU6O3"
          },
          {
            "type": "text",
            "id": "u0Kzb",
            "name": "Body 847 - explicitly called File Views replaces Agents",
            "fill": "$--foreground",
            "content": "Body 847 - explicitly called File Views replaces Agents",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "l6fpKj"
          },
          {
            "type": "text",
            "id": "s7jItz",
            "name": "Body 1100 - explicit Tools call replaces File Views",
            "fill": "$--foreground",
            "content": "Body 1100 - explicit Tools call replaces File Views",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "aoV0a"
          },
          {
            "type": "text",
            "id": "k6cNm",
            "name": "Body 848 - explicit Tools call replaces File Views",
            "fill": "$--foreground",
            "content": "Body 848 - explicit Tools call replaces File Views",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "QJQsr"
          },
          {
            "type": "text",
            "id": "juVtS",
            "name": "Body 847 - explicit Tools call replaces Agents",
            "fill": "$--foreground",
            "content": "Body 847 - explicit Tools call replaces Agents",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "OdCvV"
          },
          {
            "type": "text",
            "id": "NToss",
            "name": "Tools only - zero File Views, no File badge",
            "fill": "$--foreground",
            "content": "Tools only - zero File Views, no File badge",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "PmKT5"
          },
          {
            "type": "text",
            "id": "gyQKy",
            "name": "Tools with one hidden File View - count 1",
            "fill": "$--foreground",
            "content": "Tools with one hidden File View - count 1",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "jEUuK"
          },
          {
            "type": "text",
            "id": "L2lp9T",
            "name": "Divider hover - fixed column content",
            "fill": "$--foreground",
            "content": "Divider hover - fixed column content",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "DRb4n"
          },
          {
            "type": "text",
            "id": "ShHZU",
            "name": "Divider keyboard focus - fixed column content",
            "fill": "$--foreground",
            "content": "Divider keyboard focus - fixed column content",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "sPxvx"
          },
          {
            "type": "text",
            "id": "Q23epb",
            "name": "Frozen content drag",
            "fill": "$--foreground",
            "content": "Pointer drag - guide only, 480/360/260 columns frozen",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "D3d5BU"
          },
          {
            "type": "text",
            "id": "WiVSZ",
            "name": "Pointer release - committed 512/596 widths",
            "fill": "$--foreground",
            "content": "Pointer release - committed 512/596 widths",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "d6YJdY"
          },
          {
            "type": "text",
            "id": "gaBp0",
            "name": "Workspace switch cancels drag - restored 589/640/355",
            "fill": "$--foreground",
            "content": "Workspace switch cancels drag - restored 589/640/355",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "ec1uu"
          },
          {
            "type": "text",
            "id": "I59i4",
            "name": "Long Korean/English file title - full path tooltip",
            "fill": "$--foreground",
            "content": "Long Korean/English file title - full path tooltip",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "OkoMU"
          },
          {
            "type": "text",
            "id": "gNqDV",
            "name": "File Views tooltip",
            "fill": "$--foreground",
            "content": "File Views - tooltip",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "V6UYh"
          },
          {
            "type": "text",
            "id": "jpRRu",
            "name": "File Views focus",
            "fill": "$--foreground",
            "content": "File Views - focus",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "Z6h2wQ"
          },
          {
            "type": "text",
            "id": "prPMi",
            "name": "Tools tooltip",
            "fill": "$--foreground",
            "content": "Tools - tooltip",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "KLhGG"
          },
          {
            "type": "text",
            "id": "nf0zM",
            "name": "Tools focus",
            "fill": "$--foreground",
            "content": "Tools - focus",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "obv9L"
          },
          {
            "type": "text",
            "id": "dIeFC",
            "name": "Open server tooltip",
            "fill": "$--foreground",
            "content": "Open server - tooltip",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "EctRK"
          },
          {
            "type": "text",
            "id": "pgFdF",
            "name": "Open server focus",
            "fill": "$--foreground",
            "content": "Open server - focus",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "tOJtb"
          }
        ]
      },
      {
        "type": "frame",
        "id": "dkzAZ",
        "name": "Dark",
        "theme": {
          "Mode": "Dark"
        },
        "width": 1972,
        "fill": "$--background",
        "layout": "vertical",
        "gap": 16,
        "padding": 16,
        "children": [
          {
            "type": "text",
            "id": "RCFCu",
            "name": "Theme",
            "fill": "$--foreground",
            "content": "Dark",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-headline",
            "fontWeight": "600"
          },
          {
            "type": "text",
            "id": "JCBt8",
            "name": "Full body 1116 - docked minima",
            "fill": "$--foreground",
            "content": "Full body 1116 - docked minima",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "X7Fcc"
          },
          {
            "type": "text",
            "id": "Kwewr",
            "name": "1440 window, sidebar open - body 1100 mid fallback",
            "fill": "$--foreground",
            "content": "1440 window, sidebar open - body 1100 mid fallback",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "J3ckk6"
          },
          {
            "type": "text",
            "id": "JeQrI",
            "name": "Body 848 - two columns at minima",
            "fill": "$--foreground",
            "content": "Body 848 - two columns at minima",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "fbxi3"
          },
          {
            "type": "text",
            "id": "NPmEc",
            "name": "Body 847 - Agents base, stored File View hidden",
            "fill": "$--foreground",
            "content": "Body 847 - Agents base, stored File View hidden",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "Z6s9U8"
          },
          {
            "type": "text",
            "id": "ecD3E",
            "name": "Body 847 - explicitly called File Views replaces Agents",
            "fill": "$--foreground",
            "content": "Body 847 - explicitly called File Views replaces Agents",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "Ql0b5"
          },
          {
            "type": "text",
            "id": "IyOM9",
            "name": "Body 1100 - explicit Tools call replaces File Views",
            "fill": "$--foreground",
            "content": "Body 1100 - explicit Tools call replaces File Views",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "nBxva"
          },
          {
            "type": "text",
            "id": "d9jsX",
            "name": "Body 848 - explicit Tools call replaces File Views",
            "fill": "$--foreground",
            "content": "Body 848 - explicit Tools call replaces File Views",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "V4UiI"
          },
          {
            "type": "text",
            "id": "Y9l8n",
            "name": "Body 847 - explicit Tools call replaces Agents",
            "fill": "$--foreground",
            "content": "Body 847 - explicit Tools call replaces Agents",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "Dz9IR"
          },
          {
            "type": "text",
            "id": "Q7ovZH",
            "name": "Tools only - zero File Views, no File badge",
            "fill": "$--foreground",
            "content": "Tools only - zero File Views, no File badge",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "a8qXDY"
          },
          {
            "type": "text",
            "id": "U5KoMH",
            "name": "Tools with one hidden File View - count 1",
            "fill": "$--foreground",
            "content": "Tools with one hidden File View - count 1",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "Kk60A"
          },
          {
            "type": "text",
            "id": "gzJZl",
            "name": "Divider hover - fixed column content",
            "fill": "$--foreground",
            "content": "Divider hover - fixed column content",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "uhq8e"
          },
          {
            "type": "text",
            "id": "PwiVd",
            "name": "Divider keyboard focus - fixed column content",
            "fill": "$--foreground",
            "content": "Divider keyboard focus - fixed column content",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "eI9Uf"
          },
          {
            "type": "text",
            "id": "h44btJ",
            "name": "Frozen content drag",
            "fill": "$--foreground",
            "content": "Pointer drag - guide only, 480/360/260 columns frozen",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "DPFSq"
          },
          {
            "type": "text",
            "id": "QQpfQ",
            "name": "Pointer release - committed 512/596 widths",
            "fill": "$--foreground",
            "content": "Pointer release - committed 512/596 widths",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "N8yit"
          },
          {
            "type": "text",
            "id": "hv9wU",
            "name": "Workspace switch cancels drag - restored 589/640/355",
            "fill": "$--foreground",
            "content": "Workspace switch cancels drag - restored 589/640/355",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "YIxRN"
          },
          {
            "type": "text",
            "id": "rOn1g",
            "name": "Long Korean/English file title - full path tooltip",
            "fill": "$--foreground",
            "content": "Long Korean/English file title - full path tooltip",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "fDgAU"
          },
          {
            "type": "text",
            "id": "ZHGGw",
            "name": "File Views tooltip",
            "fill": "$--foreground",
            "content": "File Views - tooltip",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "u7vlOH"
          },
          {
            "type": "text",
            "id": "hUc27",
            "name": "File Views focus",
            "fill": "$--foreground",
            "content": "File Views - focus",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "I5MCOS"
          },
          {
            "type": "text",
            "id": "GcM9h",
            "name": "Tools tooltip",
            "fill": "$--foreground",
            "content": "Tools - tooltip",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "m3oMSt"
          },
          {
            "type": "text",
            "id": "ctqfa",
            "name": "Tools focus",
            "fill": "$--foreground",
            "content": "Tools - focus",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "dZ0c4"
          },
          {
            "type": "text",
            "id": "pin6N",
            "name": "Open server tooltip",
            "fill": "$--foreground",
            "content": "Open server - tooltip",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "OKgI3"
          },
          {
            "type": "text",
            "id": "ACTwd",
            "name": "Open server focus",
            "fill": "$--foreground",
            "content": "Open server - focus",
            "fontFamily": "$--font-ui",
            "fontSize": "$--text-subhead",
            "fontWeight": "600"
          },
          {
            "workspaceState": "KB1kt"
          }
        ]
      }
    ]
  };
  const materialized = new Map();
  function nodes(node, result = []) {
    result.push(node);
    for (const child of node.children ?? []) nodes(child, result);
    return result;
  }
  for (const state of states) {
    const window = structuredClone(windowTemplate);
    nodes(window).forEach((node, index) => { node.id = state.ids[index]; });
    for (const change of state.changes) {
      const parent = change.path.slice(0, -1).reduce((value, key) => value[key], window);
      const key = change.path.at(-1);
      if (change.delete) delete parent[key];
      else parent[key] = structuredClone(change.value);
    }
    for (const node of nodes(window)) {
      if (node.type !== 'ref') continue;
      if (!node.ref.startsWith(`${ALIAS}:`)) throw new Error(`Workspace reference ${node.ref} must use ${ALIAS}`);
      const masterId = node.ref.slice(ALIAS.length + 1);
      const master = findMaster({children: libraryDocument().children}, masterId);
      if (!master?.reusable) throw new Error(`Workspace needs reusable master ${masterId}`);
    }
    materialized.set(state.id, window);
  }
  function expand(node) {
    if (node.workspaceState) {
      const window = materialized.get(node.workspaceState);
      if (!window) throw new Error(`Workspace state ${node.workspaceState} is missing`);
      return window;
    }
    return {...node, ...(node.children ? {children: node.children.map(expand)} : {})};
  }
  const result = expand(sheet);
  for (const state of buildWorkspaceSupplemental()) {
    const parent = nodes(result).find(node => node.id === state.parent);
    if (!parent) throw new Error(`Workspace supplemental parent ${state.parent} is missing`);
    parent.children.push(...state.children);
  }
  // Frozen screen states keep their own overrides, while inherited master
  // colors need local tokens for the importing document's current theme.
  for (const node of nodes(result)) {
    if (node.type !== 'ref') continue;
    if (!node.ref.startsWith(`${ALIAS}:`)) throw new Error(`Workspace reference ${node.ref} must use ${ALIAS}`);
    const auto = themedOverrides(node.ref.slice(ALIAS.length + 1));
    for (const [prop, value] of Object.entries(auto.top)) if (!Object.hasOwn(node, prop)) node[prop] = value;
    for (const [key, props] of Object.entries(auto.descendants)) {
      const address = key.split('/').map(part => `${ALIAS}:${part}`).join('/');
      const patch = node.descendants?.[address] ?? {};
      const missing = Object.entries(props).filter(([prop]) => !Object.hasOwn(patch, prop));
      if (missing.length) (node.descendants ??= {})[address] = {...patch, ...Object.fromEntries(missing)};
    }
  }
  return result;
}

// Retained valid review targets are separate from the column condition matrix.
// Their actual Pen IDs and Korean selection ownership are preserved.
function buildWorkspaceSupplemental() {
  return [
    {"mode":"Light","parent":"g0S6az","children":[{"type":"frame","id":"ws-focus-l","name":"One keyboard area, other selections readable","width":960,"height":440,"fill":"$--background","children":[{"type":"frame","id":"ws-focus-l-1","name":"Keyboard owner","width":"fill_container","height":"fill_container","layout":"vertical","children":[{"type":"frame","id":"ws-focus-l-1-bar","name":"Tab bar","width":"fill_container","height":32,"fill":"$--background","children":[{"id":"ws-focus-l-1-tab1","type":"ref","ref":"hideui:view-tab","name":"Selected Korean tab","fill":"$--background","stroke":"$--foreground","strokeWidth":{"bottom":"$--size-tab-indicator"},"strokeAlignment":"inner","descendants":{"hideui:view-tab-mark":{"fill":"$--file-document"},"hideui:view-tab-title":{"fill":"$--foreground","content":"한글 노트.md"},"hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:view-tab-dirty":{"fill":"$--warning"},"hideui:view-tab-close":{"enabled":true}}},{"id":"ws-focus-l-1-tab2","type":"ref","ref":"hideui:view-tab","name":"검증 결과.md","fill":"$--card","descendants":{"hideui:view-tab-mark":{"fill":"$--file-document"},"hideui:view-tab-title":{"fill":"$--subtle-foreground","content":"검증 결과.md"},"hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:view-tab-dirty":{"fill":"$--warning"}}}]},{"type":"frame","id":"ws-focus-l-1-body","name":"Readable Korean content","width":"fill_container","height":"fill_container","fill":"$--background","layout":"vertical","padding":"$--spacing-sm","children":[{"type":"text","id":"ws-focus-l-1-text","name":"# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다","fill":"$--foreground","textGrowth":"fixed-width","width":"fill_container","content":"# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다.\n다른 영역의 원래 선택과 내용은 읽을 수 있습니다.\n\nfixture % echo 한글 확인\n한글 확인","fontFamily":"$--font-mono","fontSize":"$--text-caption","fontWeight":"normal"}]}]},{"type":"frame","id":"ws-focus-divider-l","name":"Area divider","width":"fit_content(0)","height":"fill_container","fill":"$--border"},{"type":"frame","id":"ws-focus-l-2","name":"Other area, retained selection","width":"fill_container","height":"fill_container","layout":"vertical","children":[{"type":"frame","id":"ws-focus-l-2-bar","name":"Tab bar","width":"fill_container","height":32,"fill":"$--card","children":[{"id":"ws-focus-l-2-tab1","type":"ref","ref":"hideui:view-tab","name":"Selected Korean tab","fill":"$--secondary","descendants":{"hideui:view-tab-mark":{"fill":"$--file-document"},"hideui:view-tab-title":{"fill":"$--foreground","content":"한글 노트.md"},"hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:view-tab-dirty":{"fill":"$--warning"},"hideui:view-tab-close":{"enabled":true}}},{"id":"ws-focus-l-2-tab2","type":"ref","ref":"hideui:view-tab","name":"검증 결과.md","fill":"$--card","descendants":{"hideui:view-tab-mark":{"fill":"$--file-document"},"hideui:view-tab-title":{"fill":"$--subtle-foreground","content":"검증 결과.md"},"hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:view-tab-dirty":{"fill":"$--warning"}}}]},{"type":"frame","id":"ws-focus-l-2-body","name":"Readable Korean content","width":"fill_container","height":"fill_container","fill":"$--background","layout":"vertical","padding":"$--spacing-sm","children":[{"type":"text","id":"ws-focus-l-2-text","name":"# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다","fill":"$--foreground","textGrowth":"fixed-width","width":"fill_container","content":"# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다.\n다른 영역의 원래 선택과 내용은 읽을 수 있습니다.\n\nfixture % echo 한글 확인\n한글 확인","fontFamily":"$--font-mono","fontSize":"$--text-caption","fontWeight":"normal"}]}]}]},{"type":"frame","id":"ewWtp","name":"Multiple running servers; both File Views and Tools off","clip":true,"width":1440,"height":900,"fill":"$--background","layout":"none","children":[{"type":"frame","id":"NlkIg","x":0,"y":0,"name":"Native window chrome - comparison context","width":1440,"height":28,"fill":"$--secondary","stroke":"$--border","strokeWidth":{"bottom":"$--size-hairline"},"strokeAlignment":"inner","layout":"none","children":[{"type":"ellipse","id":"j9wRLv","x":8,"y":8,"name":"Inactive window control 1","opacity":0.5,"fill":"$--muted-foreground","width":12,"height":12},{"type":"ellipse","id":"PbX47","x":28,"y":8,"name":"Inactive window control 2","opacity":0.5,"fill":"$--muted-foreground","width":12,"height":12},{"type":"ellipse","id":"slAYW","x":48,"y":8,"name":"Inactive window control 3","opacity":0.5,"fill":"$--muted-foreground","width":12,"height":12},{"type":"text","id":"PXL5q","x":706,"y":5,"name":"Window title","fill":"$--muted-foreground","content":"hide","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"600"}]},{"type":"frame","id":"lE5kU","x":0,"y":28,"name":"Device rail","width":48,"height":872,"fill":"$--sidebar","stroke":"$--border","strokeWidth":{"right":1},"strokeAlignment":"inner","layout":"vertical","gap":12,"padding":[8,4],"children":[{"id":"tQ1fK","type":"ref","ref":"hideui:Nyvom","name":"This Mac","width":40,"height":40,"stroke":"$--foreground","strokeWidth":2,"cornerRadius":"$--radius-lg","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"laptop"}}},{"id":"O9kV0","type":"ref","ref":"hideui:Nyvom","name":"Add device","width":40,"height":32,"stroke":"$--border","strokeWidth":1,"cornerRadius":"$--radius-lg","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"plus"}}}]},{"type":"frame","id":"WXmI0","x":48,"y":28,"name":"Projects sidebar","width":292,"height":872,"fill":"$--sidebar","stroke":"$--border","strokeWidth":{"right":1},"strokeAlignment":"inner","layout":"none","children":[{"type":"frame","id":"e1gTd","x":0,"y":0,"name":"This Mac header","width":292,"height":32,"stroke":"$--border","strokeWidth":{"bottom":"$--size-hairline"},"strokeAlignment":"inner","gap":8,"padding":[0,12],"alignItems":"center","children":[{"type":"text","id":"YhV3w","name":"Device title","fill":"$--foreground","textGrowth":"fixed-width","width":"fill_container","content":"This Mac","fontFamily":"$--font-ui","fontSize":"$--text-title","fontWeight":"600"},{"id":"hjnnU","type":"ref","ref":"hideui:Nyvom","name":"Add project","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"plus"}}},{"id":"lr1rE","type":"ref","ref":"hideui:Nyvom","name":"Search","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"search"}}}]},{"type":"frame","id":"x47WeE","x":0,"y":32,"name":"Sidebar tab strip","width":292,"height":32,"stroke":"$--border","strokeWidth":{"bottom":"$--size-hairline"},"strokeAlignment":"inner","gap":8,"padding":[0,12],"alignItems":"center","children":[{"type":"text","id":"zVAh1","name":"Selected tab","fill":"$--foreground","content":"Projects","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"normal"},{"type":"text","id":"aBZ23","name":"Other tab","fill":"$--muted-foreground","content":"Agents","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"normal"}]},{"type":"frame","id":"e1Y8X","x":4,"y":68,"name":"Home","width":284,"height":36,"gap":8,"padding":[0,8],"alignItems":"center","children":[{"type":"icon","id":"z9Zjo","name":"Home icon","width":14,"height":14,"icon":"house","library":"lucide","fill":"$--subtle-foreground"},{"type":"text","id":"x0S7j","name":"Home title","fill":"$--foreground","textGrowth":"fixed-width","width":"fill_container","content":"Home","fontFamily":"$--font-ui","fontSize":"$--text-title","fontWeight":"600"},{"type":"text","id":"vGB7F","name":"Project count","fill":"$--muted-foreground","content":"0 projects","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"normal"}]},{"type":"text","id":"iWAse","x":12,"y":110,"name":"Recent activity","fill":"$--muted-foreground","content":"Projects · Recent activity · 1","fontFamily":"$--font-ui","fontSize":"$--text-micro","fontWeight":"600"},{"type":"frame","id":"vhwXo","x":4,"y":124,"name":"Selected checkout","width":284,"height":36,"fill":"$--secondary","cornerRadius":"$--radius-sm","gap":8,"padding":[0,8],"alignItems":"center","children":[{"type":"icon","id":"klozz","name":"Checkout folder","width":14,"height":14,"icon":"folder","library":"lucide","fill":"$--subtle-foreground"},{"type":"text","id":"vmjI1","name":"Checkout label","fill":"$--foreground","content":"fixture","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"600"}]},{"type":"frame","id":"D2gM4I","x":0,"y":840,"name":"Sidebar footer","width":292,"height":32,"stroke":"$--border","strokeWidth":{"top":1},"strokeAlignment":"inner","gap":8,"padding":[0,12],"justifyContent":"end","alignItems":"center","children":[{"id":"pDsO7","type":"ref","ref":"hideui:Nyvom","name":"Background usage","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"activity"}}},{"id":"M9bxgS","type":"ref","ref":"hideui:Nyvom","name":"Settings","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"settings"}}}]}]},{"id":"KabPT","type":"ref","ref":"hideui:VMZTz","name":"Shared Workspace toolbar","fill":"$--sidebar","stroke":"$--border","x":340,"y":28,"width":1100,"height":32,"descendants":{"hideui:bkh81":{"fill":"$--subtle-foreground","content":"Home  /  fixture  /  main"},"hideui:Kjfje/hideui:side-panel-tools-toggle/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button":{"fill":[]},"hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge":{"fill":"$--primary","enabled":false},"hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-count":{"content":"1"},"hideui:Kjfje/hideui:XHe9v/hideui:OaJhM":{"fill":[]},"hideui:Kjfje/hideui:XHe9v/hideui:OaJhM/hideui:ZIZFR":{"fill":"$--subtle-foreground"}}},{"id":"H4VH2","type":"ref","ref":"hideui:side-panel","name":"Docked columns","fill":"$--background","x":340,"y":60,"width":1100,"height":840,"descendants":{"hideui:YpxMC":{"fill":"$--background","enabled":true,"x":0,"y":0,"width":1100,"height":840},"hideui:YpxMC/hideui:m3nqv":{"fill":"$--card","stroke":"$--border"},"hideui:YpxMC/hideui:Yf5g6":{"fill":"$--secondary"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-mark":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-title":{"fill":"$--foreground"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-dirty":{"fill":"$--warning"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-close/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:ujfpg/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:GYdsi/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:jEy0G":{"fill":"$--background","width":1100},"hideui:YpxMC/hideui:blLvs":{"fill":"$--secondary","stroke":"$--border"},"hideui:YpxMC/hideui:W2jHfs":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:v57l6H":{"fill":"$--foreground"},"hideui:YpxMC/hideui:CG4wC":{"fill":"$--muted-foreground"},"hideui:YpxMC/hideui:K8ikC/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:a63n6t/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:jPwvc":{"fill":"$--background"},"hideui:YpxMC/hideui:PNaQy":{"fill":"$--foreground"},"hideui:YpxMC/hideui:aTsYy":{"fill":"$--background","stroke":"$--border","enabled":false,"width":0,"height":0},"hideui:YpxMC/hideui:lFQa0":{"fill":"$--card","stroke":"$--border"},"hideui:YpxMC/hideui:ljIwg":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:e7uYn":{"fill":"$--foreground"},"hideui:YpxMC/hideui:uJYZa":{"fill":"$--muted-foreground"},"hideui:YpxMC/hideui:x9QjtJ/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:E0Mk6/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:M6Mnyn":{"fill":"$--background"},"hideui:YpxMC/hideui:QSc1K":{"fill":"$--foreground"},"hideui:gPjhF":{"enabled":false,"x":480,"y":0,"width":0,"height":0},"hideui:gPjhF/hideui:side-panel-grip-line-top":{"fill":"$--muted-foreground"},"hideui:gPjhF/hideui:side-panel-grip-pill":{"fill":"$--card","stroke":"$--border"},"hideui:gPjhF/hideui:side-panel-grip-glyph":{"fill":"$--muted-foreground"},"hideui:giyPa":{"fill":"$--card","enabled":false,"x":488,"y":0,"width":612,"height":840},"hideui:giyPa/hideui:tPjjv":{"fill":"$--card","stroke":"$--border"},"hideui:giyPa/hideui:b7bsc7":{"fill":"$--card","width":304},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-mark":{"fill":"$--file-blue"},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-title":{"fill":"$--foreground"},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-dirty":{"fill":"$--warning"},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-close/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:N6jVdg/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:eeRGB/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:fenJg":{"fill":"$--card","stroke":"$--border"},"hideui:giyPa/hideui:fenJg/hideui:BUboy":{"fill":"$--subtle-foreground","content":"… 제목과 경로 확인.md"},"hideui:giyPa/hideui:fenJg/hideui:ifLpE/hideui:btn-ic":{"fill":"$--primary-foreground"},"hideui:giyPa/hideui:fenJg/hideui:ifLpE/hideui:btn-lb":{"fill":"$--foreground"},"hideui:giyPa/hideui:fenJg/hideui:W8pA8k/hideui:btn-ic":{"fill":"$--primary-foreground"},"hideui:giyPa/hideui:fenJg/hideui:W8pA8k/hideui:btn-lb":{"fill":"$--foreground"},"hideui:giyPa/hideui:fenJg/hideui:iFSdQ/hideui:btn-ic":{"fill":"$--primary-foreground"},"hideui:giyPa/hideui:fenJg/hideui:iFSdQ/hideui:btn-lb":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:fenJg/hideui:lEjtc/hideui:btn-ic":{"fill":"$--primary-foreground"},"hideui:giyPa/hideui:fenJg/hideui:lEjtc/hideui:btn-lb":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:nFrdf":{"fill":"$--card"},"hideui:giyPa/hideui:aX9gF":{"fill":"$--secondary"},"hideui:giyPa/hideui:jOt0p":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:iExJ4":{"fill":"$--file-blue"},"hideui:giyPa/hideui:T1KFmN":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:sfXAM":{"fill":"$--foreground"},"hideui:giyPa/hideui:h10qC":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:fzHJn":{"fill":"$--foreground","content":"한글과 English가 함께 있는 파일을 읽습니다."},"hideui:giyPa/hideui:GQ6yZ":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:oVfkk":{"fill":"$--foreground"},"hideui:w7GZ7c":{"enabled":false,"x":1100,"y":0,"width":0,"height":0},"hideui:w7GZ7c/hideui:side-panel-grip-line-top":{"fill":"$--muted-foreground"},"hideui:w7GZ7c/hideui:side-panel-grip-pill":{"fill":"$--card","stroke":"$--border"},"hideui:w7GZ7c/hideui:side-panel-grip-glyph":{"fill":"$--muted-foreground"},"hideui:eNvgI":{"fill":"$--card","enabled":false,"x":1108,"y":0,"width":260,"height":840},"hideui:eNvgI/hideui:H2M1v0":{"stroke":"$--border"},"hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-explorer":{"stroke":"$--primary"},"hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-explorer-glyph":{"fill":"$--foreground"},"hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-history-glyph":{"fill":"$--subtle-foreground"},"hideui:eNvgI/hideui:b22eS":{"stroke":"$--border"},"hideui:eNvgI/hideui:d927zh":{"fill":"$--subtle-foreground"},"hideui:eNvgI/hideui:DVJS6/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:eNvgI/hideui:NZoEx":{"stroke":"$--border","enabled":false,"height":0,"width":0},"hideui:eNvgI/hideui:iDnaZ":{"fill":"$--muted-foreground"},"hideui:eNvgI/hideui:MmldP":{"stroke":"$--border"},"hideui:eNvgI/hideui:Cwgap":{"fill":"$--file-blue"},"hideui:eNvgI/hideui:x1im3":{"fill":"$--foreground","fontSize":"$--text-caption"},"hideui:eNvgI/hideui:fZi7M":{"stroke":"$--border"},"hideui:eNvgI/hideui:t0QOE":{"fill":"$--file-blue"},"hideui:eNvgI/hideui:aHlvp":{"fill":"$--foreground","fontSize":"$--text-caption","content":"한글과 English 작업 기록 - 긴 파일 제…"}}},{"type":"frame","id":"E3BX0","x":1056,"y":60,"name":"Running servers picker","width":320,"fill":"$--popover","cornerRadius":"$--radius-md","stroke":"$--border","strokeWidth":1,"strokeAlignment":"inner","layout":"vertical","gap":"$--spacing-xs","padding":"$--spacing-sm","children":[{"type":"text","id":"ILw1e","name":"Running servers","fill":"$--popover-foreground","content":"Running servers","fontFamily":"$--font-ui","fontSize":"$--text-caption","fontWeight":"500"},{"id":"u8ij3q","type":"ref","ref":"hideui:btn-m","name":"127.0.0.1:3000","width":"fill_container","height":28,"justifyContent":"start","gap":"$--spacing-xs","padding":[0,"$--spacing-md"],"fill":"$--accent","stroke":"$--ring","strokeWidth":1,"strokeAlignment":"outer","descendants":{"hideui:btn-ic":{"enabled":true,"icon":"globe","fill":"$--popover-foreground"},"hideui:btn-lb":{"content":"127.0.0.1:3000","fontFamily":"$--font-mono","fontSize":"$--text-caption","fill":"$--popover-foreground"}}},{"id":"BOKUB","type":"ref","ref":"hideui:btn-m","name":"[::1]:5173","width":"fill_container","height":28,"justifyContent":"start","gap":"$--spacing-xs","padding":[0,"$--spacing-md"],"fill":[],"stroke":[],"strokeWidth":0,"strokeAlignment":"outer","descendants":{"hideui:btn-ic":{"enabled":true,"icon":"globe","fill":"$--popover-foreground"},"hideui:btn-lb":{"content":"[::1]:5173","fontFamily":"$--font-mono","fontSize":"$--text-caption","fill":"$--popover-foreground"}}}]}]}]},
    {"mode":"Dark","parent":"dkzAZ","children":[{"type":"frame","id":"ws-focus-d","name":"One keyboard area, other selections readable","width":960,"height":440,"fill":"$--background","children":[{"type":"frame","id":"ws-focus-d-1","name":"Keyboard owner","width":"fill_container","height":"fill_container","layout":"vertical","children":[{"type":"frame","id":"ws-focus-d-1-bar","name":"Tab bar","width":"fill_container","height":32,"fill":"$--background","children":[{"id":"ws-focus-d-1-tab1","type":"ref","ref":"hideui:view-tab","name":"Selected Korean tab","fill":"$--background","stroke":"$--foreground","strokeWidth":{"bottom":"$--size-tab-indicator"},"strokeAlignment":"inner","descendants":{"hideui:view-tab-mark":{"fill":"$--file-document"},"hideui:view-tab-title":{"fill":"$--foreground","content":"한글 노트.md"},"hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:view-tab-dirty":{"fill":"$--warning"},"hideui:view-tab-close":{"enabled":true}}},{"id":"ws-focus-d-1-tab2","type":"ref","ref":"hideui:view-tab","name":"검증 결과.md","fill":"$--card","descendants":{"hideui:view-tab-mark":{"fill":"$--file-document"},"hideui:view-tab-title":{"fill":"$--subtle-foreground","content":"검증 결과.md"},"hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:view-tab-dirty":{"fill":"$--warning"}}}]},{"type":"frame","id":"ws-focus-d-1-body","name":"Readable Korean content","width":"fill_container","height":"fill_container","fill":"$--background","layout":"vertical","padding":"$--spacing-sm","children":[{"type":"text","id":"ws-focus-d-1-text","name":"# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다","fill":"$--foreground","textGrowth":"fixed-width","width":"fill_container","content":"# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다.\n다른 영역의 원래 선택과 내용은 읽을 수 있습니다.\n\nfixture % echo 한글 확인\n한글 확인","fontFamily":"$--font-mono","fontSize":"$--text-caption","fontWeight":"normal"}]}]},{"type":"frame","id":"ws-focus-divider-d","name":"Area divider","width":"fit_content(0)","height":"fill_container","fill":"$--border"},{"type":"frame","id":"ws-focus-d-2","name":"Other area, retained selection","width":"fill_container","height":"fill_container","layout":"vertical","children":[{"type":"frame","id":"ws-focus-d-2-bar","name":"Tab bar","width":"fill_container","height":32,"fill":"$--card","children":[{"id":"ws-focus-d-2-tab1","type":"ref","ref":"hideui:view-tab","name":"Selected Korean tab","fill":"$--secondary","descendants":{"hideui:view-tab-mark":{"fill":"$--file-document"},"hideui:view-tab-title":{"fill":"$--foreground","content":"한글 노트.md"},"hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:view-tab-dirty":{"fill":"$--warning"},"hideui:view-tab-close":{"enabled":true}}},{"id":"ws-focus-d-2-tab2","type":"ref","ref":"hideui:view-tab","name":"검증 결과.md","fill":"$--card","descendants":{"hideui:view-tab-mark":{"fill":"$--file-document"},"hideui:view-tab-title":{"fill":"$--subtle-foreground","content":"검증 결과.md"},"hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:view-tab-dirty":{"fill":"$--warning"}}}]},{"type":"frame","id":"ws-focus-d-2-body","name":"Readable Korean content","width":"fill_container","height":"fill_container","fill":"$--background","layout":"vertical","padding":"$--spacing-sm","children":[{"type":"text","id":"ws-focus-d-2-text","name":"# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다","fill":"$--foreground","textGrowth":"fixed-width","width":"fill_container","content":"# 한글 노트\n\n현재 입력을 받는 영역만 강조합니다.\n다른 영역의 원래 선택과 내용은 읽을 수 있습니다.\n\nfixture % echo 한글 확인\n한글 확인","fontFamily":"$--font-mono","fontSize":"$--text-caption","fontWeight":"normal"}]}]}]},{"type":"frame","id":"es3RE","name":"Multiple running servers; both File Views and Tools off","clip":true,"width":1440,"height":900,"fill":"$--background","layout":"none","children":[{"type":"frame","id":"aTb9G","x":0,"y":0,"name":"Native window chrome - comparison context","width":1440,"height":28,"fill":"$--secondary","stroke":"$--border","strokeWidth":{"bottom":"$--size-hairline"},"strokeAlignment":"inner","layout":"none","children":[{"type":"ellipse","id":"OXSdD","x":8,"y":8,"name":"Inactive window control 1","opacity":0.5,"fill":"$--muted-foreground","width":12,"height":12},{"type":"ellipse","id":"PJTbo","x":28,"y":8,"name":"Inactive window control 2","opacity":0.5,"fill":"$--muted-foreground","width":12,"height":12},{"type":"ellipse","id":"FO6Yr","x":48,"y":8,"name":"Inactive window control 3","opacity":0.5,"fill":"$--muted-foreground","width":12,"height":12},{"type":"text","id":"XgiMy","x":706,"y":5,"name":"Window title","fill":"$--muted-foreground","content":"hide","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"600"}]},{"type":"frame","id":"uwCzX","x":0,"y":28,"name":"Device rail","width":48,"height":872,"fill":"$--sidebar","stroke":"$--border","strokeWidth":{"right":1},"strokeAlignment":"inner","layout":"vertical","gap":12,"padding":[8,4],"children":[{"id":"m3Yw2","type":"ref","ref":"hideui:Nyvom","name":"This Mac","width":40,"height":40,"stroke":"$--foreground","strokeWidth":2,"cornerRadius":"$--radius-lg","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"laptop"}}},{"id":"FDTjR","type":"ref","ref":"hideui:Nyvom","name":"Add device","width":40,"height":32,"stroke":"$--border","strokeWidth":1,"cornerRadius":"$--radius-lg","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"plus"}}}]},{"type":"frame","id":"VJuAf","x":48,"y":28,"name":"Projects sidebar","width":292,"height":872,"fill":"$--sidebar","stroke":"$--border","strokeWidth":{"right":1},"strokeAlignment":"inner","layout":"none","children":[{"type":"frame","id":"ad4af","x":0,"y":0,"name":"This Mac header","width":292,"height":32,"stroke":"$--border","strokeWidth":{"bottom":"$--size-hairline"},"strokeAlignment":"inner","gap":8,"padding":[0,12],"alignItems":"center","children":[{"type":"text","id":"OA1kQ","name":"Device title","fill":"$--foreground","textGrowth":"fixed-width","width":"fill_container","content":"This Mac","fontFamily":"$--font-ui","fontSize":"$--text-title","fontWeight":"600"},{"id":"k09v3","type":"ref","ref":"hideui:Nyvom","name":"Add project","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"plus"}}},{"id":"CY50L","type":"ref","ref":"hideui:Nyvom","name":"Search","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"search"}}}]},{"type":"frame","id":"USpqd","x":0,"y":32,"name":"Sidebar tab strip","width":292,"height":32,"stroke":"$--border","strokeWidth":{"bottom":"$--size-hairline"},"strokeAlignment":"inner","gap":8,"padding":[0,12],"alignItems":"center","children":[{"type":"text","id":"W0Y3y","name":"Selected tab","fill":"$--foreground","content":"Projects","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"normal"},{"type":"text","id":"ukkoQ","name":"Other tab","fill":"$--muted-foreground","content":"Agents","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"normal"}]},{"type":"frame","id":"T6Nm2","x":4,"y":68,"name":"Home","width":284,"height":36,"gap":8,"padding":[0,8],"alignItems":"center","children":[{"type":"icon","id":"V84dHa","name":"Home icon","width":14,"height":14,"icon":"house","library":"lucide","fill":"$--subtle-foreground"},{"type":"text","id":"F2k1Um","name":"Home title","fill":"$--foreground","textGrowth":"fixed-width","width":"fill_container","content":"Home","fontFamily":"$--font-ui","fontSize":"$--text-title","fontWeight":"600"},{"type":"text","id":"OrXCs","name":"Project count","fill":"$--muted-foreground","content":"0 projects","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"normal"}]},{"type":"text","id":"vWmVO","x":12,"y":110,"name":"Recent activity","fill":"$--muted-foreground","content":"Projects · Recent activity · 1","fontFamily":"$--font-ui","fontSize":"$--text-micro","fontWeight":"600"},{"type":"frame","id":"RBYXs","x":4,"y":124,"name":"Selected checkout","width":284,"height":36,"fill":"$--secondary","cornerRadius":"$--radius-sm","gap":8,"padding":[0,8],"alignItems":"center","children":[{"type":"icon","id":"tPXqp","name":"Checkout folder","width":14,"height":14,"icon":"folder","library":"lucide","fill":"$--subtle-foreground"},{"type":"text","id":"aYWyU","name":"Checkout label","fill":"$--foreground","content":"fixture","fontFamily":"$--font-ui","fontSize":"$--text-body","fontWeight":"600"}]},{"type":"frame","id":"QuMHI","x":0,"y":840,"name":"Sidebar footer","width":292,"height":32,"stroke":"$--border","strokeWidth":{"top":1},"strokeAlignment":"inner","gap":8,"padding":[0,12],"justifyContent":"end","alignItems":"center","children":[{"id":"xaaDR","type":"ref","ref":"hideui:Nyvom","name":"Background usage","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"activity"}}},{"id":"M3iGzm","type":"ref","ref":"hideui:Nyvom","name":"Settings","descendants":{"hideui:ZIZFR":{"fill":"$--subtle-foreground","icon":"settings"}}}]}]},{"id":"Uw8N7","type":"ref","ref":"hideui:VMZTz","name":"Shared Workspace toolbar","fill":"$--sidebar","stroke":"$--border","x":340,"y":28,"width":1100,"height":32,"descendants":{"hideui:bkh81":{"fill":"$--subtle-foreground","content":"Home  /  fixture  /  main"},"hideui:Kjfje/hideui:side-panel-tools-toggle/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button":{"fill":[]},"hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-button/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-badge":{"fill":"$--primary","enabled":false},"hideui:Kjfje/hideui:xMinO/hideui:side-panel-toggle-count":{"content":"1"},"hideui:Kjfje/hideui:XHe9v/hideui:OaJhM":{"fill":[]},"hideui:Kjfje/hideui:XHe9v/hideui:OaJhM/hideui:ZIZFR":{"fill":"$--subtle-foreground"}}},{"id":"ejdE4","type":"ref","ref":"hideui:side-panel","name":"Docked columns","fill":"$--background","x":340,"y":60,"width":1100,"height":840,"descendants":{"hideui:YpxMC":{"fill":"$--background","enabled":true,"x":0,"y":0,"width":1100,"height":840},"hideui:YpxMC/hideui:m3nqv":{"fill":"$--card","stroke":"$--border"},"hideui:YpxMC/hideui:Yf5g6":{"fill":"$--secondary"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-mark":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-title":{"fill":"$--foreground"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-dirty":{"fill":"$--warning"},"hideui:YpxMC/hideui:Yf5g6/hideui:view-tab-close/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:ujfpg/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:GYdsi/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:jEy0G":{"fill":"$--background","width":1100},"hideui:YpxMC/hideui:blLvs":{"fill":"$--secondary","stroke":"$--border"},"hideui:YpxMC/hideui:W2jHfs":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:v57l6H":{"fill":"$--foreground"},"hideui:YpxMC/hideui:CG4wC":{"fill":"$--muted-foreground"},"hideui:YpxMC/hideui:K8ikC/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:a63n6t/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:jPwvc":{"fill":"$--background"},"hideui:YpxMC/hideui:PNaQy":{"fill":"$--foreground"},"hideui:YpxMC/hideui:aTsYy":{"fill":"$--background","stroke":"$--border","enabled":false,"width":0,"height":0},"hideui:YpxMC/hideui:lFQa0":{"fill":"$--card","stroke":"$--border"},"hideui:YpxMC/hideui:ljIwg":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:e7uYn":{"fill":"$--foreground"},"hideui:YpxMC/hideui:uJYZa":{"fill":"$--muted-foreground"},"hideui:YpxMC/hideui:x9QjtJ/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:E0Mk6/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:YpxMC/hideui:M6Mnyn":{"fill":"$--background"},"hideui:YpxMC/hideui:QSc1K":{"fill":"$--foreground"},"hideui:gPjhF":{"enabled":false,"x":480,"y":0,"width":0,"height":0},"hideui:gPjhF/hideui:side-panel-grip-line-top":{"fill":"$--muted-foreground"},"hideui:gPjhF/hideui:side-panel-grip-pill":{"fill":"$--card","stroke":"$--border"},"hideui:gPjhF/hideui:side-panel-grip-glyph":{"fill":"$--muted-foreground"},"hideui:giyPa":{"fill":"$--card","enabled":false,"x":488,"y":0,"width":612,"height":840},"hideui:giyPa/hideui:tPjjv":{"fill":"$--card","stroke":"$--border"},"hideui:giyPa/hideui:b7bsc7":{"fill":"$--card","width":304},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-mark":{"fill":"$--file-blue"},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-title":{"fill":"$--foreground"},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-strike":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-dirty":{"fill":"$--warning"},"hideui:giyPa/hideui:b7bsc7/hideui:view-tab-close/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:N6jVdg/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:eeRGB/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:fenJg":{"fill":"$--card","stroke":"$--border"},"hideui:giyPa/hideui:fenJg/hideui:BUboy":{"fill":"$--subtle-foreground","content":"… 제목과 경로 확인.md"},"hideui:giyPa/hideui:fenJg/hideui:ifLpE/hideui:btn-ic":{"fill":"$--primary-foreground"},"hideui:giyPa/hideui:fenJg/hideui:ifLpE/hideui:btn-lb":{"fill":"$--foreground"},"hideui:giyPa/hideui:fenJg/hideui:W8pA8k/hideui:btn-ic":{"fill":"$--primary-foreground"},"hideui:giyPa/hideui:fenJg/hideui:W8pA8k/hideui:btn-lb":{"fill":"$--foreground"},"hideui:giyPa/hideui:fenJg/hideui:iFSdQ/hideui:btn-ic":{"fill":"$--primary-foreground"},"hideui:giyPa/hideui:fenJg/hideui:iFSdQ/hideui:btn-lb":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:fenJg/hideui:lEjtc/hideui:btn-ic":{"fill":"$--primary-foreground"},"hideui:giyPa/hideui:fenJg/hideui:lEjtc/hideui:btn-lb":{"fill":"$--subtle-foreground"},"hideui:giyPa/hideui:nFrdf":{"fill":"$--card"},"hideui:giyPa/hideui:aX9gF":{"fill":"$--secondary"},"hideui:giyPa/hideui:jOt0p":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:iExJ4":{"fill":"$--file-blue"},"hideui:giyPa/hideui:T1KFmN":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:sfXAM":{"fill":"$--foreground"},"hideui:giyPa/hideui:h10qC":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:fzHJn":{"fill":"$--foreground","content":"한글과 English가 함께 있는 파일을 읽습니다."},"hideui:giyPa/hideui:GQ6yZ":{"fill":"$--muted-foreground"},"hideui:giyPa/hideui:oVfkk":{"fill":"$--foreground"},"hideui:w7GZ7c":{"enabled":false,"x":1100,"y":0,"width":0,"height":0},"hideui:w7GZ7c/hideui:side-panel-grip-line-top":{"fill":"$--muted-foreground"},"hideui:w7GZ7c/hideui:side-panel-grip-pill":{"fill":"$--card","stroke":"$--border"},"hideui:w7GZ7c/hideui:side-panel-grip-glyph":{"fill":"$--muted-foreground"},"hideui:eNvgI":{"fill":"$--card","enabled":false,"x":1108,"y":0,"width":260,"height":840},"hideui:eNvgI/hideui:H2M1v0":{"stroke":"$--border"},"hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-explorer":{"stroke":"$--primary"},"hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-explorer-glyph":{"fill":"$--foreground"},"hideui:eNvgI/hideui:HWGiW/hideui:side-panel-tool-history-glyph":{"fill":"$--subtle-foreground"},"hideui:eNvgI/hideui:b22eS":{"stroke":"$--border"},"hideui:eNvgI/hideui:d927zh":{"fill":"$--subtle-foreground"},"hideui:eNvgI/hideui:DVJS6/hideui:ZIZFR":{"fill":"$--subtle-foreground"},"hideui:eNvgI/hideui:NZoEx":{"stroke":"$--border","enabled":false,"height":0,"width":0},"hideui:eNvgI/hideui:iDnaZ":{"fill":"$--muted-foreground"},"hideui:eNvgI/hideui:MmldP":{"stroke":"$--border"},"hideui:eNvgI/hideui:Cwgap":{"fill":"$--file-blue"},"hideui:eNvgI/hideui:x1im3":{"fill":"$--foreground","fontSize":"$--text-caption"},"hideui:eNvgI/hideui:fZi7M":{"stroke":"$--border"},"hideui:eNvgI/hideui:t0QOE":{"fill":"$--file-blue"},"hideui:eNvgI/hideui:aHlvp":{"fill":"$--foreground","fontSize":"$--text-caption","content":"한글과 English 작업 기록 - 긴 파일 제…"}}},{"type":"frame","id":"Y0b444","x":1056,"y":60,"name":"Running servers picker","width":320,"fill":"$--popover","cornerRadius":"$--radius-md","stroke":"$--border","strokeWidth":1,"strokeAlignment":"inner","layout":"vertical","gap":"$--spacing-xs","padding":"$--spacing-sm","children":[{"type":"text","id":"eydR1","name":"Running servers","fill":"$--popover-foreground","content":"Running servers","fontFamily":"$--font-ui","fontSize":"$--text-caption","fontWeight":"500"},{"id":"UX8VT","type":"ref","ref":"hideui:btn-m","name":"127.0.0.1:3000","width":"fill_container","height":28,"justifyContent":"start","gap":"$--spacing-xs","padding":[0,"$--spacing-md"],"fill":"$--accent","stroke":"$--ring","strokeWidth":1,"strokeAlignment":"outer","descendants":{"hideui:btn-ic":{"enabled":true,"icon":"globe","fill":"$--popover-foreground"},"hideui:btn-lb":{"content":"127.0.0.1:3000","fontFamily":"$--font-mono","fontSize":"$--text-caption","fill":"$--popover-foreground"}}},{"id":"zn5Ax","type":"ref","ref":"hideui:btn-m","name":"[::1]:5173","width":"fill_container","height":28,"justifyContent":"start","gap":"$--spacing-xs","padding":[0,"$--spacing-md"],"fill":[],"stroke":[],"strokeWidth":0,"strokeAlignment":"outer","descendants":{"hideui:btn-ic":{"enabled":true,"icon":"globe","fill":"$--popover-foreground"},"hideui:btn-lb":{"content":"[::1]:5173","fontFamily":"$--font-mono","fontSize":"$--text-caption","fill":"$--popover-foreground"}}}]}]}]}
  ];
}

// -- Screen / Project Sessions --------------------------------------------------

// The Overview on its Sessions tile: the same header as the Issues board, then
// the Project's session list and the read-only detail beside it.
function buildSessions(tokens) {
  function build(suffix) {
    const header = overviewHeader(tokens, 'ss-head', suffix, {project: 'fixture', facts: [
      {glyph: 'folder-git-2', label: '2 worktrees'},
      {glyph: 'hard-drive', label: '812 MB'},
    ], view: 'sessions', width: 1136});
    const list = frame(`ss-list-${suffix}`, 'List', {width: 320, layout: 'vertical', gap: '$--spacing-sm'}, [
      screenTabs(`ss-tabs-${suffix}`, ['All', 'Codex', 'Claude Code'], 0),
      screenInput(`ss-search-${suffix}`, {content:'화검', width:300}),
      text(`ss-copy-status-${suffix}`, 'Search titles and conversation contents', {size:'$--text-caption', fill:'$--muted-foreground'}),
      frame(`ss-copy-controls-${suffix}`, 'Copied history', {layout:'horizontal', gap:'$--spacing-xs', alignItems:'center'}, [
        text(`ss-copy-label-${suffix}`, 'Copied history', {size:'$--text-caption', fill:'$--muted-foreground'}),
        screenSelect(`ss-retention-${suffix}`, {content:'90 days', width:100}),
        screenButton(`ss-rebuild-${suffix}`, 'Rebuild index', {variant:'ghost'}),
      ]),
      text(`ss-copy-privacy-${suffix}`, 'Local copy only. Off clears this Project’s copy; originals stay intact.', {size:'$--text-micro',fill:'$--muted-foreground',width:300,textGrowth:'fixed-width'}),
      text(`ss-count-${suffix}`, '1 session', {size: '$--text-caption', fill: '$--muted-foreground'}),
      screenSessionRow(`ss-row1-${suffix}`, {title:'배포 스크립트 정리하고 release note 초안까지 작성해줘',checkout:'fixture',provider:'Claude Code',time:'Sep 21, 10:00 AM',width:300}),
      text(`ss-snippet-context-${suffix}`, 'Assistant · Sep 21, 10:00 AM', {size:'$--text-micro',fill:'$--muted-foreground'}),
      text(`ss-snippet-${suffix}`, '대화검색으로 로그인 연결을 확인했습니다.', {size:'$--text-caption',width:300,textGrowth:'fixed-width'}),
    ]);
    const detail = frame(`ss-detail-${suffix}`, 'Matching conversation', {width:800,height:780,layout:'vertical',gap:'$--spacing-lg',padding:'$--spacing-lg',fill:'$--card',cornerRadius:'$--radius-md'}, [
      text(`ss-detail-title-${suffix}`, '로그인 연결 확인', {size:'$--text-body',weight:'600'}),
      text(`ss-human-role-${suffix}`, 'Human', {size:'$--text-caption',fill:'$--muted-foreground'}),
      text(`ss-human-text-${suffix}`, '지난 작업에서 로그인 연결을 확인해 줘.', {width:'fill_container',textGrowth:'fixed-width'}),
      frame(`ss-selected-${suffix}`, 'Matching Assistant message', {width:'fill_container',layout:'vertical',gap:'$--spacing-sm',padding:'$--spacing-md',fill:'$--accent',cornerRadius:'$--radius-sm',stroke:'$--primary',strokeWidth:'$--size-hairline'}, [
        text(`ss-assistant-role-${suffix}`, 'Assistant · Sep 21, 10:00 AM', {size:'$--text-caption',fill:'$--muted-foreground'}),
        text(`ss-assistant-text-${suffix}`, '대화검색으로 로그인 연결을 확인했습니다.', {width:'fill_container',textGrowth:'fixed-width'}),
      ]),
    ]);
    const main = frame(`ss-main-${suffix}`, 'Project Sessions', {width:1136,height:928,layout:'vertical',gap:'$--spacing-md'}, [header,frame(`ss-row-${suffix}`, 'Row', {layout:'horizontal',gap:'$--spacing-lg'}, [list,detail])]);
    return [frame(`ss-wrap-${suffix}`, 'Sessions with sidebar', {width:1440,layout:'horizontal',gap:'$--spacing-md',alignItems:'start'}, [screenSidebar(tokens,'ss-sidebar',suffix,[]),main])];
  }
  return screenSheet('screen-sessions', 'Screen / Project Sessions', 'web/src/ProjectOverview.tsx on its Sessions tab, ProjectSessions.tsx: the Overview’s header and tabs over the provider-filtered session list with search, and the read-only detail pane, with grouped Human/Assistant snippets, exact-message jumps and local copied-history retention/rebuild controls. Pending, failed and changed-source states preserve metadata results.', build, build);
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

// The ⌘K palette (PRD cmdk-navigation) and the ⌘P file palette on the same
// shell: the sidebar's Search icon that opens ⌘K, the query row with its Esc
// keycap, two-line rows with the agent's own mark and ↵ on the selected row,
// and for ⌘K a detail beside the list. Rows are authored here on local tokens, as the
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
        text(id('side-a0t'), 'mailbox 원격 에이전트 구현', {weight: '500'}),
      ]),
    ]);

    // ⌘K's own wide layout (PRD cmdk-navigation D-01): the list at the shared
    // palette width and the highlighted row's detail beside it.
    const WIDE = Math.round(W * 1.5);
    const DETAIL = WIDE - W;
    const pill = (key, label, fill = '$--secondary', color = '$--subtle-foreground') => frame(id(key), 'Pill', {padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-sm', fill}, [
      text(`${id(key)}-t`, label, {size: '$--text-caption', fill: color}),
    ]);
    const status = (key, label, color) => text(id(key), label, {size: '$--text-caption', fill: color});
    function wide(key, {query, placeholder, list, detail, footer}) {
      const hasBody = list !== null;
      return frame(id(key), 'Palette · wide', {width: WIDE, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', layout: 'vertical', clip: true}, [
        frame(`${id(key)}-in`, 'Query', {height: num(tokens, '--size-control-lg'), width: WIDE, layout: 'horizontal', alignItems: 'center', gap: '$--spacing-sm', padding: [0, '$--spacing-md'], ...(hasBody ? {stroke: '$--border', strokeWidth: {bottom: num(tokens, '--size-hairline')}} : {})}, [
          icon(`${id(key)}-ini`, 'search', {size: num(tokens, '--size-icon'), fill: '$--muted-foreground'}),
          frame(`${id(key)}-inf`, 'Field', {width: 'fill_container'}, [
            query ? text(`${id(key)}-inq`, query) : text(`${id(key)}-inp`, placeholder, {fill: '$--muted-foreground'}),
          ]),
          kbd(`${key}-esc`, 'Esc'),
        ]),
        ...(hasBody ? [
          frame(`${id(key)}-body`, 'Body', {layout: 'horizontal', width: WIDE}, [
            frame(`${id(key)}-list`, 'List', {layout: 'vertical', gap: 0, padding: '$--spacing-xxs', width: W, stroke: '$--border', strokeWidth: {right: num(tokens, '--size-hairline')}}, list),
            frame(`${id(key)}-detail`, 'Detail', {layout: 'vertical', gap: '$--spacing-sm', padding: '$--spacing-md', width: DETAIL}, detail),
          ]),
          frame(`${id(key)}-foot`, 'Footer', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center', width: WIDE, padding: ['$--spacing-xxs', '$--spacing-md'], stroke: '$--border', strokeWidth: {top: num(tokens, '--size-hairline')}}, footer ?? [
            text(`${id(key)}-f1`, '↑ ↓ 이동', {size: '$--text-caption', fill: '$--muted-foreground'}),
            text(`${id(key)}-f2`, '↵ 열기', {size: '$--text-caption', fill: '$--muted-foreground'}),
            text(`${id(key)}-f3`, 'Esc 닫기', {size: '$--text-caption', fill: '$--muted-foreground'}),
          ]),
        ] : []),
      ]);
    }
    const detailLines = (key, kind, title, pills, facts, related) => [
      text(`${id(key)}-k`, kind, {size: '$--text-caption', weight: '600', fill: '$--muted-foreground'}),
      text(`${id(key)}-t`, title, {weight: '600'}),
      frame(`${id(key)}-p`, 'Pills', {layout: 'horizontal', gap: '$--spacing-xs'}, pills),
      ...facts.map(([label, value], i) => frame(`${id(key)}-fact${i}`, 'Fact', {layout: 'horizontal', gap: '$--spacing-md'}, [
        text(`${id(key)}-fl${i}`, label, {size: '$--text-caption', fill: '$--muted-foreground'}),
        text(`${id(key)}-fv${i}`, value, {size: '$--text-caption'}),
      ])),
      ...(related ? [text(`${id(key)}-rh`, '관계', {size: '$--text-caption', weight: '600', fill: '$--muted-foreground'}), ...related.map(([label, depth, bold], i) => frame(`${id(key)}-rel${i}`, 'Relation', {padding: [0, 0, 0, depth * 12]}, [
        text(`${id(key)}-rl${i}`, label, {size: '$--text-caption', weight: bold ? '600' : '400', fill: bold ? '$--foreground' : '$--subtle-foreground'}),
      ]))] : []),
    ];

    // Empty query in front of an agent: the Overview's grouping, issues on
    // top, the checkout's group with its pull request and lineage, a parent
    // elsewhere as one `↑ 부모` line.
    const relations = wide('related', {placeholder: '이름이나 #번호를 입력하세요', list: [
      heading('g-related', 'Related'),
      row('q0', {lead: glyph('q0i', 'circle-dot'), title: '#273 mailbox 쓰기 명령이 sandbox 거부를 internal로 숨김', detail: 'Issue · herdr-ide', selected: true}),
      row('q1', {lead: glyph('q1i', 'git-branch'), title: 'fix/mailbox-sandbox-letters', detail: 'herdr-ide'}),
      row('q2', {lead: glyph('q2i', 'git-pull-request'), title: '#275 Surface mailbox sandbox refusals', detail: 'PR · Open · fix/mailbox-sandbox-letters'}),
      row('q3', {lead: mark('q3m', 'claude'), title: 'mailbox 쓰기 명령 sandbox 오류 해결', detail: 'herdr-ide › fix/mailbox-sandbox-letters · Done'}),
      row('q4', {lead: mark('q4m', 'codex'), title: '↑ 부모 codex workspace-write 원인 조사', detail: 'herdr-ide › main · Working'}),
    ], detail: detailLines('rel-d', 'Issue', 'mailbox 쓰기 명령이 sandbox 거부를 internal로 숨김', [pill('rel-p1', 'Open', '$--secondary', '$--success'), pill('rel-p2', '#273')], [['프로젝트', 'herdr-ide'], ['맡은 곳', 'fix/mailbox-sandbox-letters'], ['닫는 PR', '#275'], ['읽음', '4분 전 읽음']], [['#273 mailbox 쓰기 명령이 sandbox 거부를…', 0, true], ['fix/mailbox-sandbox-letters', 0, false], ['#275 Surface mailbox sandbox refusals', 1, false]])});

    const typed = wide('typed', {query: 'sand', list: [
      heading('g-issues', 'Issues'),
      row('t0', {lead: glyph('t0i', 'circle-dot'), title: '#273 mailbox 쓰기 명령이 sandbox 거부를 internal로 숨김', detail: 'Issue · herdr-ide'}),
      heading('g-agents', 'Agents'),
      row('t1', {lead: mark('t1m', 'claude'), title: 'mailbox 쓰기 명령 sandbox 오류 해결', detail: 'herdr-ide › fix/mailbox-sandbox-letters · Done', selected: true}),
      heading('g-checkouts', 'Checkouts'),
      row('t2', {lead: glyph('t2i', 'git-branch'), title: 'herdr-ide / fix/mailbox-sandbox-letters', detail: '~/projects/herdr-ide.worktrees/sandbox', mono: true}),
      frame(id('t-gh'), 'GitHub row', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: ROW, padding: ['$--spacing-xs', '$--spacing-sm']}, [
        glyph('t-ghi', 'search'),
        text(id('t-ght'), 'GitHub에서 "sand" 검색', {weight: '500'}),
      ]),
    ], detail: detailLines('typed-d', 'Agent · Claude', 'mailbox 쓰기 명령 sandbox 오류 해결', [pill('typed-p1', 'Done', '$--secondary', '$--success'), pill('typed-p2', 'This Mac')], [['checkout', 'herdr-ide › fix/mailbox-sandbox-letters']], null)});

    const number = wide('number', {query: '#275', list: [
      heading('g-prs', 'Pull requests'),
      row('n0', {lead: glyph('n0i', 'git-pull-request'), title: '#275 Surface mailbox sandbox refusals', detail: 'PR · Open · fix/mailbox-sandbox-letters', selected: true}),
    ], detail: detailLines('number-d', 'Pull request', 'Surface mailbox sandbox refusals', [pill('number-p1', 'Open', '$--secondary', '$--success'), pill('number-p2', 'CI 진행 중', '$--secondary', '$--warning'), pill('number-p3', '#275')], [['Review', '리뷰 필요'], ['브랜치', 'fix/mailbox-sandbox-letters'], ['닫는 이슈', '#273']], null)});

    const collapsed = wide('collapsed', {placeholder: '이름이나 #번호를 입력하세요', list: null});

    const ghRow = (key, label, lead) => frame(id(key), 'GitHub row', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: ROW, padding: ['$--spacing-xs', '$--spacing-sm'], fill: '$--accent', cornerRadius: '$--radius-xs'}, [
      lead,
      text(id(`${key}-t`), label, {weight: '500', fill: '$--foreground'}),
    ]);
    const ghDetail = (key) => [
      text(`${id(key)}-k`, 'GitHub', {size: '$--text-caption', weight: '600', fill: '$--muted-foreground'}),
      text(`${id(key)}-t`, '"quota wall"', {weight: '600'}),
      text(`${id(key)}-d`, '이 Mac의 GitHub 프로젝트 저장소에서 PR과 이슈를 한 번 검색합니다. 입력하는 동안에는 GitHub를 부르지 않습니다.', {size: '$--text-caption', fill: '$--muted-foreground', width: DETAIL - 2 * num(tokens, '--spacing-md')}),
    ];
    const ghWorking = wide('gh-working', {query: 'quota wall', list: [
      empty('gh-working-e', '일치하는 항목 없음'),
      ghRow('gh-working-r', 'GitHub에서 "quota wall" 검색', icon(`${id('gh-working-ri')}`, 'loader-circle', {size: num(tokens, '--size-icon-sm'), fill: '$--muted-foreground'})),
    ], detail: ghDetail('gh-working-d')});
    const ghResults = wide('gh-results', {query: 'quota wall', list: [
      heading('g-github', 'GitHub'),
      row('gr0', {lead: glyph('gr0i', 'git-pull-request'), title: '#118 Close stale sandbox watches', detail: 'PR · acme/herdr-ide · Merged', selected: true}),
      row('gr1', {lead: glyph('gr1i', 'circle-dot'), title: '#96 mailbox sandbox 거부 로그가 비어 있음', detail: 'Issue · acme/herdr-ide · Closed'}),
    ], detail: detailLines('gh-results-d', 'Pull request · GitHub', 'Close stale sandbox watches', [pill('gh-results-p1', 'Merged'), pill('gh-results-p2', '#118'), pill('gh-results-p3', 'acme/herdr-ide')], [], null)});
    const ghFailed = wide('gh-failed', {query: 'quota wall', list: [
      empty('gh-failed-e', '일치하는 항목 없음'),
      ghRow('gh-failed-r', 'GitHub 검색 실패 · 다시 시도', icon(`${id('gh-failed-ri')}`, 'triangle-alert', {size: num(tokens, '--size-icon'), fill: '$--warning'})),
    ], detail: ghDetail('gh-failed-d')});

    const noMatch = surface('nomatch', {query: 'zzz', list: [empty('nomatch-e', '일치하는 항목 없음')]});
    const files = surface('files', {placeholder: 'Search files by name', list: [
      row('f0', {lead: glyph('f0i', 'file-text'), title: 'docs/한글 노트.md', selected: true}),
      row('f1', {lead: glyph('f1i', 'file-code'), title: 'scripts/pen-screens.mjs'}),
    ]});
    const filesCell = frame(id('files-wrap'), 'Files', {layout: 'vertical', gap: '$--spacing-xs'}, [
      files,
      text(id('files-hint'), '⌘↵ 옆에 열기', {size: '$--text-caption', fill: '$--muted-foreground'}),
    ]);

    return [
      labelled('side-cell', 'Sidebar · the Search icon opens ⌘K', sidebar),
      frame(id('wide-states'), 'Wide states', {layout: 'vertical', gap: '$--spacing-lg'}, [
        labelled('related-cell', '⌘K · empty, in front of an agent: relations', relations),
        labelled('typed-cell', '⌘K · typed', typed),
        labelled('number-cell', '⌘K · #number', number),
        labelled('collapsed-cell', '⌘K · nothing in front: the input alone', collapsed),
        labelled('ghw-cell', '⌘K · GitHub searching', ghWorking),
        labelled('ghr-cell', '⌘K · GitHub results', ghResults),
        labelled('ghf-cell', '⌘K · GitHub failed', ghFailed),
      ]),
      frame(id('states'), 'States', {layout: 'vertical', gap: '$--spacing-lg'}, [
        labelled('nomatch-cell', '⌘K · narrow window or no detail: no match', noMatch),
        labelled('files-cell', '⌘P · same shell, ⌘↵ opens beside', filesCell),
      ]),
    ];
  }
  return screenSheet('screen-palette', 'Screen / Palette', 'web/src/SearchPalette.tsx and web/src/Palette.tsx over Command/CommandDialog (PRD cmdk-navigation): the sidebar’s Search icon with its ⌘K hint, ⌘K’s wide list-and-detail layout (empty in front of an agent drawn as the Overview groups it, typed results by group, #number, the input alone, GitHub searching, results and failed) and the ⌘P file palette whose ⌘↵ opens beside.', build, build);
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

function buildMenus(tokens) {
  const FG = '$--foreground', SUBTLE = '$--subtle-foreground', MUTED = '$--muted-foreground';
  const XS = num(tokens, '--spacing-xs'), SM = num(tokens, '--spacing-sm'), MD = num(tokens, '--spacing-md');
  const at = (node, x, y) => ({...node, x: Math.round(x), y: Math.round(y)});
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);

  // The ⌘N start panel (StartPanel), placed as the ⌘K palette is: the same width,
  // --size-settings-sheet-window-inset from the window top, --popover with a
  // hairline and a shadow. On --popover a --border hairline vanishes in Dark, so
  // its rule, the three dropdowns and the menus stand on --secondary.
  const ASK_W = num(tokens, '--size-search-sheet-w');
  const ASK_TOP = 16;
  const FIELD_H = 88, BAR_H = 40, DD_Y = 7, DD_H = 26;
  const SHADOW = {type: 'shadow', shadowType: 'outer', offset: {x: 0, y: 12}, blur: 32, spread: 0, color: '#00000073'};
  const DD = {target: {x: MD, w: 164}, kind: {x: MD + 164 + SM, w: 112}, model: {x: MD + 164 + SM + 112 + SM, w: 132}};
  const PROMPT = '블로그 초안을 두 가지 톤으로 다시 써서 비교해줘';
  const providerArt = (id, provider, size = 14) => frame(id, `${provider} mark`, {width: size, height: size, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${provider}.png`, mode: 'fit'}}, []);
  function dropdown(id, {lead, label, width, open = false}) {
    return frame(id, label, {width, height: DD_H, layout: 'horizontal', gap: 6, alignItems: 'center', padding: [0, SM], cornerRadius: '$--radius-sm', fill: '$--secondary', ...(open ? {stroke: MUTED, strokeWidth: 1, strokeAlignment: 'inner'} : {})}, [
      lead, text(`${id}-t`, label, {size: '$--text-caption'}), spacer(`${id}-sp`), icon(`${id}-c`, open ? 'chevron-up' : 'chevron-down', {size: 12, fill: MUTED}),
    ]);
  }
  function startPanel(p, {open = null} = {}) {
    return frame(`${p}-ask`, '⌘N', {width: ASK_W, layout: 'vertical', fill: '$--popover', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: 1, strokeAlignment: 'inner', clip: true, effect: SHADOW}, [
      frame(`${p}-field`, 'Prompt', {width: ASK_W, height: FIELD_H, padding: [MD, num(tokens, '--spacing-lg')], layout: 'vertical'}, [
        text(`${p}-q`, PROMPT, {size: '$--text-subhead'}),
      ]),
      frame(`${p}-bar`, 'Bar', {width: ASK_W, height: BAR_H, layout: 'none', fill: '$--popover'}, [
        at(frame(`${p}-barrule`, 'Rule', {width: ASK_W, height: 1, fill: '$--secondary'}, []), 0, 0),
        at(dropdown(`${p}-dt`, {lead: icon(`${p}-dtg`, 'folder-git-2', {size: 12, fill: SUBTLE}), label: 'herdr-ide · main', width: DD.target.w, open: open === 'target'}), DD.target.x, DD_Y),
        at(dropdown(`${p}-dk`, {lead: providerArt(`${p}-dkg`, 'claude'), label: 'Claude', width: DD.kind.w, open: open === 'kind'}), DD.kind.x, DD_Y),
        at(dropdown(`${p}-dm`, {lead: icon(`${p}-dmg`, 'cpu', {size: 12, fill: SUBTLE}), label: 'opus', width: DD.model.w, open: open === 'model'}), DD.model.x, DD_Y),
        at(themedXref(`${p}-kbd`, 'kbd-m', '⏎', {}, {'kbd-t': {content: '⏎'}}), ASK_W - MD - 56 - SM - 22, 10),
        at(screenButton(`${p}-go`, '시작', {height: DD_H, width: 56}), ASK_W - MD - 56, DD_Y),
      ]),
    ]);
  }
  // A menu row: the lead, the label and the check on the row's right edge; a
  // device that is not connected keeps its rows, dimmed, with 연결 안 됨 under.
  function menuRow(id, {lead, label, checked = false, highlighted = false, detail, width}) {
    return frame(id, label, {width, height: detail ? 40 : 28, layout: 'horizontal', gap: SM, alignItems: 'center', padding: [0, SM], cornerRadius: '$--radius-xs', ...(highlighted ? {fill: '$--accent'} : {})}, [
      lead,
      detail
        ? frame(`${id}-b`, 'Body', {layout: 'vertical', gap: 0}, [text(`${id}-t`, label, {size: '$--text-body', fill: MUTED}), text(`${id}-d`, detail, {size: '$--text-caption', fill: MUTED})])
        : text(`${id}-t`, label, {size: '$--text-body'}),
      spacer(`${id}-sp`),
      ...(checked ? [icon(`${id}-ck`, 'check', {size: 14, fill: FG})] : []),
    ]);
  }
  const menuSep = (id, width) => frame(id, 'Separator', {width, height: 9, layout: 'vertical', justifyContent: 'center'}, [frame(`${id}-l`, 'Line', {width, height: 1, fill: '$--secondary'}, [])]);
  const menuBox = (id, width, rows) => frame(id, 'Menu', {width, layout: 'vertical', padding: XS, fill: '$--popover', cornerRadius: '$--radius-sm', stroke: '$--secondary', strokeWidth: 1, strokeAlignment: 'inner', effect: SHADOW}, rows);
  const lead = (id, glyph) => frame(id, 'Lead', {width: 16, height: 16, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [icon(`${id}-i`, glyph, {size: 14, fill: MUTED})]);
  const MENU_W = 236, ROW_W = MENU_W - 2 * XS;
  // The target follows what is in front (here herdr-ide main); Home stays on top
  // so going back to it is one pick, then this device's checkouts, then every
  // other device's Home and checkouts.
  const targetMenu = p => menuBox(`${p}-mt`, MENU_W, [
    menuRow(`${p}-mt0`, {lead: lead(`${p}-mt0g`, 'house'), label: 'Home', width: ROW_W}),
    menuSep(`${p}-mts0`, ROW_W),
    menuRow(`${p}-mt1`, {lead: lead(`${p}-mt1g`, 'folder-git-2'), label: 'herdr-ide · main', checked: true, highlighted: true, width: ROW_W}),
    menuRow(`${p}-mt2`, {lead: lead(`${p}-mt2g`, 'folder-git-2'), label: 'oh-my-principle · main', width: ROW_W}),
    menuRow(`${p}-mt3`, {lead: lead(`${p}-mt3g`, 'folder-git-2'), label: 'sasu · main', width: ROW_W}),
    menuRow(`${p}-mt4`, {lead: lead(`${p}-mt4g`, 'folder-git-2'), label: 'demo · main', width: ROW_W}),
    menuSep(`${p}-mts`, ROW_W),
    menuRow(`${p}-mt5`, {lead: lead(`${p}-mt5g`, 'server'), label: 'mini · Home', width: ROW_W}),
    menuRow(`${p}-mt6`, {lead: lead(`${p}-mt6g`, 'server'), label: 'build-box · Home', detail: '연결 안 됨', width: ROW_W}),
  ]);
  const kindMenu = p => menuBox(`${p}-mk`, 180, [
    menuRow(`${p}-mk0`, {lead: providerArt(`${p}-mk0g`, 'claude', 16), label: 'Claude', checked: true, highlighted: true, width: 172}),
    menuRow(`${p}-mk1`, {lead: providerArt(`${p}-mk1g`, 'codex', 16), label: 'Codex', width: 172}),
  ]);
  const CLAUDE_MODELS = ['haiku', 'sonnet', 'opus', 'fable'];
  const CODEX_MODELS = ['gpt-6-astra', 'gpt-6-sol', 'gpt-6-luna', 'gpt-5.6-sol', 'gpt-5.6-terra', 'gpt-5.6-luna', 'gpt-5.5'];
  const modelMenu = (p, models, pick, w = 180) => menuBox(`${p}-mm`, w, models.map((m, i) => menuRow(`${p}-mm${i}`, {lead: lead(`${p}-mm${i}g`, 'cpu'), label: m, checked: m === pick, highlighted: m === pick, width: w - 2 * XS})));
  function startCut(p, {open, menu}) {
    const menuX = DD[open].x;
    const menuH = {target: 8 + 28 * 5 + 9 * 2 + 28 + 40, kind: 8 + 2 * 28, model: 8 + 4 * 28}[open];
    return frame(`${p}-cut`, 'Cut', {layout: 'none', width: ASK_W + 48, height: 24 + FIELD_H + BAR_H + 4 + menuH + 24, fill: '$--background', cornerRadius: '$--radius-md', clip: true}, [
      at(startPanel(p, {open}), 24, ASK_TOP),
      at(menu, 24 + menuX, ASK_TOP + FIELD_H + DD_Y + DD_H + 4),
    ]);
  }
  const captioned = (id, label, node) => frame(id, label, {layout: 'vertical', gap: '$--spacing-sm', alignItems: 'start'}, [text(`${id}-l`, label, {size: '$--text-caption', weight: '600', fill: MUTED}), node]);
  // The panel at rest with the three dropdowns closed, then each dropdown open.
  function startPanels(suffix) {
    const p = key => `mn-ask-${key}-${suffix}`;
    return frame(`mn-ask-${suffix}`, '⌘N start panel', {layout: 'vertical', gap: '$--spacing-lg'}, [
      captioned(p('rest'), '⌘N · 창 위에 뜬 판, 글을 적은 상태, 드롭다운 셋 닫힘', frame(p('restbox'), 'Rest', {layout: 'vertical', padding: 24, fill: '$--background', cornerRadius: '$--radius-md'}, [startPanel(p('r'))])),
      frame(p('row1'), 'Kind and model menus', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [
        captioned(p('k'), '종류 메뉴', startCut(p('kc'), {open: 'kind', menu: kindMenu(p('kc'))})),
        captioned(p('m'), '모델 메뉴 · Claude', startCut(p('mc'), {open: 'model', menu: modelMenu(p('mc'), CLAUDE_MODELS, 'opus')})),
      ]),
      frame(p('row2'), 'Target and Codex menus', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [
        captioned(p('t'), '대상 메뉴', startCut(p('tc'), {open: 'target', menu: targetMenu(p('tc'))})),
        captioned(p('x'), 'Codex를 고르면 모델 목록이 바뀐다', frame(p('xw'), 'Codex models', {layout: 'vertical', padding: 16, fill: '$--background', cornerRadius: '$--radius-md'}, [modelMenu(p('xm'), CODEX_MODELS, 'gpt-6-astra', 200)])),
      ]),
    ]);
  }

  function build(suffix) {
    // workspaceManage.ts projectMenu() in a browser tab, which has no Finder
    // (PRD sidebar-context-menus D-02, D-07); Remove project carries no
    // destructive style in the real menu either. The desktop app's menu, with
    // Reveal in Finder, is on Screen / Projects Sidebar.
    const rowMenu = screenMenuContent(`mn-row-${suffix}`, 220, [
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
    // The one-device window's footer button: the device it is, and the way to add another.
    const deviceMenu = screenMenuContent(`mn-dev-${suffix}`, 200, [
      screenMenuItem(`mn-dev0-${suffix}`, 'This Mac · 이 기기', {glyph: 'laptop', shortcut: '✓'}),
      screenMenuSeparator(`mn-devsep-${suffix}`),
      screenMenuItem(`mn-dev1-${suffix}`, '기기 추가…', {glyph: 'plus', state: 'highlighted'}),
    ]);
    // ExplorerTree.tsx puts the Git status in the Explorer's root row, so its
    // answer never moves a row (issue 570): a spinner while the first answer is on
    // its way, the outline Badge in the warning color, whose tooltip says why,
    // for a failure the operator can repair, and nothing for a current answer
    // or a folder that is not a repository.
    const gitRow = (key, mark) => frame(`mn-git${key}-${suffix}`, 'Explorer root row', {layout: 'horizontal', alignItems: 'center', gap: '$--spacing-xs', width: 360}, [
      text(`mn-git${key}t-${suffix}`, 'demo', {size: '$--text-caption', weight: '600', fill: '$--muted-foreground', width: 'fill_container'}),
      ...(mark ? [mark] : []),
      icon(`mn-git${key}r-${suffix}`, 'refresh-cw', {size: 12, fill: '$--muted-foreground'}),
    ]);
    const notice = frame(`mn-notice-${suffix}`, 'Explorer Git status', {width: 360, layout: 'vertical', gap: '$--spacing-sm'}, [
      gitRow('l', icon(`mn-gitls-${suffix}`, 'loader-circle', {size: 12, fill: '$--muted-foreground'})),
      gitRow('u', themedXref(`mn-gitub-${suffix}`, 'eHAjc', '!', {...BADGE_VARIANTS.outline.overrides, stroke: '$--warning'}, {xXuNa: {enabled: false}, n8L5dm: {content: '!', fill: '$--warning'}})),
      gitRow('n', null),
    ]);
    return [frame(`mn-wrap-${suffix}`, 'Wrap', {layout: 'vertical', gap: '$--spacing-lg'}, [
      frame(`mn-row-a-${suffix}`, 'Row', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [rowMenu, explorerCtx, deviceMenu]),
      notice,
      startPanels(suffix),
    ])];
  }
  return screenSheet('screen-menus', 'Screen / Menus and Overlays', 'entry-menu.tsx EntryContextMenu, the sidebar footer’s device button menu (This Mac, 기기 추가…), the Explorer root row’s Git status marks and the ⌘N start panel (PRD home-device-rail D-18..D-20): overlays shown anchored in their real screen context rather than the abstract System gallery. The panel floats over the window like ⌘K with one line to write and three dropdowns, target, agent kind and model, and 시작 with ⏎. The target defaults to what is in front and its menu lists Home first, then the front device’s checkouts, then every other device’s Home, an unreachable one dimmed with 연결 안 됨; the kind menu is Claude and Codex, and the model menu follows the kind (Claude: haiku, sonnet, opus, fable; Codex: the list the CLI reports). The kind and each kind’s model are remembered; the target is not.', build, build);
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

  // The device's Home row (sidebar-header.tsx), in the place the Overview row held:
  // the project row's columns, the house glyph, the project count where a badge
  // stands, and under the pointer + in the fold slot (a new tab in Home).
  function homeRow(id, {count, selected = false, hover = false}) {
    return themedXref(id, 'qdhY0', 'Home', {width: row, height: num(tokens, '--size-project-row'), ...(selected ? {fill: '$--secondary'} : hover ? {fill: '$--accent'} : {})}, {
      wgtfo: {opacity: hover ? 1 : 0},
      iBYjj: {icon: 'plus', fill: '$--foreground'},
      mIzlX: {icon: 'house'},
      JkPyX: {content: 'Home'},
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

  // -- the device rail and the header line (PRD home-device-rail D-09..D-14, quick device-rail-slack) --
  const railWidth = num(tokens, '--size-rail');
  const WIN_H = 720;
  const TILE = 32, RING = 2, RAIL_TOP = 12;
  const MARK = num(tokens, '--size-rail-mark'), BADGE = num(tokens, '--size-rail-badge'), BADGE_TEXT = num(tokens, '--size-rail-badge-text'), CUT = 2;
  const FG = '$--foreground', SUBTLE = '$--subtle-foreground', MUTED = '$--muted-foreground';
  const at = (node, x, y) => ({...node, x: Math.round(x), y: Math.round(y)});
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);
  const DEVICES = {
    mac: {glyph: 'laptop', label: 'This Mac'},
    mini: {monogram: 'M', label: 'mini'},
    build: {monogram: 'Bb', label: 'build-box'},
    add: {glyph: 'plus', label: '기기 추가'},
  };
  // The state fills the Agents tab's counts wear as text.
  const BADGE_FILL = {needs_you: '$--warning', done: '$--success', working: '$--agent-working'};

  // A mark notched into a tile's corner: the mark on a ring of the rail's own fill.
  function notched(id, name, w, h, inner, children = []) {
    return frame(id, name, {width: w + 2 * CUT, height: h + 2 * CUT, cornerRadius: (h + 2 * CUT) / 2, fill: '$--sidebar', layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [
      frame(`${id}-m`, name, {width: w, height: h, cornerRadius: h / 2, layout: 'horizontal', justifyContent: 'center', alignItems: 'center', ...inner}, children),
    ]);
  }

  // A tile is a 32 square with This Mac's laptop or a device's monogram and no
  // name under it; the selected one is ringed 2 off its edge. One mark notched into
  // the top-right shows the most urgent state: the Needs You count (9+ from ten),
  // else a dot for unseen Done. Working has no mark, and an unreachable device dims
  // its glyph and wears a x at the bottom-right. The add tile's edge is dashed in the app; Pen has no
  // dash, so it is drawn solid.
  function railTile(id, key, {selected = false, off = false, needs = 0, done = false} = {}) {
    const d = DEVICES[key];
    const box = TILE + 4 * RING, x0 = (railWidth - TILE) / 2, y0 = 2 * RING;
    const glyphFill = key === 'add' ? MUTED : selected ? FG : SUBTLE;
    const drawn = d.monogram ? text(`${id}-g`, d.monogram, {size: '$--text-body', weight: '600', fill: glyphFill}) : icon(`${id}-g`, d.glyph, {size: key === 'add' ? 14 : 20, fill: glyphFill});
    const glyph = off ? {...drawn, opacity: num(tokens, '--opacity-dimmed')} : drawn;
    const pill = needs >= 10 ? BADGE + 2 : BADGE;
    return frame(id, d.label, {layout: 'none', width: railWidth, height: box}, [
      ...(selected ? [at(frame(`${id}-ring`, 'Selected', {width: box, height: box, cornerRadius: 12, stroke: FG, strokeWidth: RING, strokeAlignment: 'inner'}, []), x0 - 2 * RING, y0 - 2 * RING)] : []),
      at(frame(`${id}-t`, 'Tile', {
        width: TILE, height: TILE, cornerRadius: '$--radius-md', layout: 'horizontal', justifyContent: 'center', alignItems: 'center',
        ...(key === 'add' ? {stroke: '$--border', strokeWidth: 1, strokeAlignment: 'inner'} : {fill: '$--secondary'}),
      }, [glyph]), x0, y0),
      ...(done && needs === 0 && !off ? [at(notched(`${id}-done`, 'Done', MARK, MARK, {fill: BADGE_FILL.done}), x0 + TILE + 2 - MARK - CUT, y0 - 2 - CUT)] : []),
      ...(needs > 0 && !off ? [at(notched(`${id}-needs`, 'Needs You', pill, BADGE, {fill: BADGE_FILL.needs_you}, [
        text(`${id}-nt`, needs >= 10 ? '9+' : String(needs), {size: BADGE_TEXT, weight: '600', fill: '$--status-foreground'}),
      ]), x0 + TILE + 4 - pill - CUT, y0 - 4 - CUT)] : []),
      ...(off ? [at(notched(`${id}-x`, 'Not connected', BADGE, BADGE, {fill: '$--card'}, [
        text(`${id}-xt`, '×', {size: BADGE_TEXT, fill: MUTED}),
      ]), x0 + TILE + 4 - BADGE - CUT, y0 + TILE + 4 - BADGE - CUT)] : []),
    ]);
  }

  // The sidebar's full-height left column: This Mac and the registered devices in
  // order, and + directly under the last one, 44 apart as in the app (each frame
  // carries the ring's 4 above and below the tile, so the gap is 4, not 12); the sidebar beside it follows the
  // selected tile. `tiles` overrides the three-device set.
  function deviceRail(p, selected, {miniOff = false, tiles} = {}) {
    const set = tiles ?? [
      ['mac', {selected: selected === 'mac', needs: 2, done: true}],
      ['mini', {selected: selected === 'mini', needs: miniOff ? 0 : 12, off: miniOff}],
      ['build', {selected: selected === 'build', off: true}],
    ];
    return frame(`${p}-rail`, 'Device rail', {width: railWidth, height: WIN_H, layout: 'vertical', gap: '$--spacing-xs', padding: [RAIL_TOP - 2 * RING, 0, 4, 0], fill: '$--sidebar', stroke: '$--border', strokeWidth: {right: 1}, strokeAlignment: 'inner'}, [
      ...set.map(([key, options], i) => railTile(`${p}-rt${i}`, key, options)),
      railTile(`${p}-radd`, 'add'),
    ]);
  }

  // The header line beside the traffic lights names the device in front, with Add
  // project and Search at its end; with the rail hidden the name carries a chevron
  // and opens the device menu.
  function headerLine(p, {name, tag, icons = [], menu = false}) {
    return frame(`${p}-band`, 'Header line', {width, height: num(tokens, '--size-tab-strip'), layout: 'horizontal', gap: sm, alignItems: 'center', padding: [0, xs, 0, '$--spacing-md']}, name ? [
      text(`${p}-bn`, name, {size: '$--text-subhead', weight: '600'}),
      ...(tag ? [text(`${p}-bt`, tag, {size: '$--text-caption', fill: MUTED})] : []),
      ...(menu ? [icon(`${p}-bc`, 'chevron-down', {size: 12, fill: MUTED})] : []),
      spacer(`${p}-bsp`),
      ...icons.map(g => screenIconButton(`${p}-b${g}`, g)),
    ] : []);
  }

  const ruleLine = id => frame(id, 'Rule', {width, height: 1, fill: '$--border'}, []);

  // Projects | Agents, under the header line of every device that can be read.
  function tabStrip(p, {agents = false} = {}) {
    return frame(`${p}-strip`, 'Tabs', {width, height: num(tokens, '--size-tab-strip'), padding: [0, xs], gap: xs, alignItems: 'center'}, [
      ...['Projects', 'Agents'].map((label, i) => frame(`${p}-tab${i}`, label, {width: 'fill_container', height: num(tokens, '--size-control-sm'), layout: 'horizontal', gap: xs, alignItems: 'center', justifyContent: 'center', cornerRadius: '$--radius-sm', ...((agents ? i === 1 : i === 0) ? {fill: '$--secondary'} : {})}, [
        text(`${p}-t${i}`, label, {size: '$--text-caption'}),
        text(`${p}-k${i}`, i ? '⌘⇧A' : '⌘⇧P', {size: '$--text-micro', fill: MUTED, mono: true}),
      ])),
    ]);
  }

  function sharedOverviewRow(p, count = 0) {
    return frame(`${p}-overview`, 'Overview', {width, height: num(tokens, '--size-tab-strip'), layout: 'horizontal', gap: sm, padding: [0, '$--spacing-md'], alignItems: 'center'}, [
      icon(`${p}-overview-g`, 'layout-dashboard', {size: 16, fill: SUBTLE}),
      text(`${p}-overview-t`, 'Overview', {size: '$--text-body'}), spacer(`${p}-overview-space`),
      ...(count ? [text(`${p}-overview-n`, `${count} asking`, {size: '$--text-caption', fill: '$--warning'})] : []),
      text(`${p}-overview-k`, '⌘⇧O', {size: '$--text-micro', fill: MUTED, mono: true}),
    ]);
  }

  // `Needs You N · Done N · Working N` above the Agents list; a zero count is left out.
  function stateCounts(p, counts) {
    const WORDS = {needs_you: 'Needs You', done: 'Done', working: 'Working'};
    return frame(`${p}-cnt`, 'State counts', {width, layout: 'horizontal', gap: xs, padding: [sm, '$--spacing-md', 0, '$--spacing-md']}, counts.flatMap(([state, count], i) => [
      ...(i > 0 ? [text(`${p}-cd${i}`, '·', {size: '$--text-caption', fill: MUTED})] : []),
      text(`${p}-c${i}`, `${WORDS[state]} ${count}`, {size: '$--text-caption', fill: BADGE_FILL[state]}),
    ]));
  }

  // Usage chips and the Settings gear.
  function footer(p) {
    return frame(`${p}-ftr`, 'Footer', {width, layout: 'vertical'}, [
      ruleLine(`${p}-fr`),
      frame(`${p}-fb`, 'Footer row', {width, layout: 'horizontal', padding: [sm, '$--spacing-md'], gap: xs, alignItems: 'center'}, [
        screenUsageChip(`${p}-u0`, {provider: 'claude', value: '62%'}),
        screenUsageChip(`${p}-u1`, {provider: 'codex', value: '59%'}),
        spacer(`${p}-fsp`),
        screenIconButton(`${p}-gear`, 'settings', {size: 20}),
      ]),
    ]);
  }

  // Agent rows the device sidebars share: title, mark, and the context line the
  // Needs You and Agents lists carry; a remote row wears the device chip.
  const AGENTS = {
    deploy: {title: '배포 전 확인', status: 'asking', age: '30s', line: '변경 내용을 확인해 주세요', place: 'herdr-ide › main', bright: true},
    blog: {title: '블로그 초안 정리', status: 'asking', age: '2m', line: '톤을 이대로 갈까요?', place: 'Home', bright: true},
    research: {title: '두 프로젝트 비교 조사', status: 'seen', provider: 'codex', age: '14m', place: 'Home'},
    ci: {title: 'CI 러너 샤드 정리', status: 'done', age: '6m', place: 'herdr-ide › ci-shards', bright: true},
    readable: {title: '사이드바 가독성 개선', status: 'working', age: '1m', place: 'herdr-ide › main'},
    principle: {title: '원칙 문서 정리', status: 'working', provider: 'codex', age: '3m', place: 'oh-my-principle › main'},
    sasu: {title: '판정 로그 확인', status: 'seen', age: '1h', place: 'sasu › main'},
    batch: {title: '배치 감시', status: 'asking', provider: 'codex', age: '5m', line: 'PR #252 머지할까요?', place: 'Home', bright: true},
    build: {title: '릴리스 빌드 확인', status: 'working', provider: 'codex', age: '1m', place: 'hide › main'},
  };
  // A raised group over Home: its heading with the whole count, its most recent
  // rows up to the cap, and a More fold for the rest.
  const raised = (p, title, list, total = list.length) => [
    section(`${p}-sn`, `${title} · ${total}`),
    ...list.map(([key, extra], i) => agentRow(`${p}-n${i}`, {...AGENTS[key], inset: sm, ...extra})),
    ...(total > list.length ? [fold(`${p}-more`, `More ${total - list.length}`, 'project')] : []),
  ];

  // The Home row with the agents that belong to no project under it.
  function homeBlock(p, {count, agents, selected = false}) {
    return frame(`${p}-home`, 'Home', {width, layout: 'vertical', padding: [0, xs, xs, xs]}, [
      homeRow(`${p}-hr`, {count, selected}),
      ...agents.map((key, i) => agentRow(`${p}-ha${i}`, {...AGENTS[key], place: undefined, line: undefined, inset: 30})),
    ]);
  }

  // One device's sidebar: the header line over Projects | Agents, then the Projects tab's Needs You and Done groups, its Home
  // and projects, or the Agents tab's counts and sections; a fixed `height` is the window's, and the list takes what is left.
  function deviceSidebar(key, s, {name, tag, icons = ['plus', 'search'], menu = false, needs, done, home, agentsTab = false, counts, rows = [], height}) {
    const p = `psb-${key}`;
    const id = `${p}-${s}`;
    return frame(`${p}-${s}`, 'Sidebar', {width, ...(height ? {height} : {}), layout: 'vertical', fill: '$--sidebar', clip: true}, [
      headerLine(id, {name, tag, icons, menu}),
      ruleLine(`${id}-r0`), sharedOverviewRow(id, counts?.find(([state]) => state === "needs_you")?.[1] ?? needs?.total ?? 0), tabStrip(id, {agents: agentsTab}), ruleLine(`${id}-r1`),
      ...(counts ? [stateCounts(id, counts)] : []),
      ...(needs ? [frame(`${id}-needs`, 'Needs You', {width, layout: 'vertical', padding: [0, xs, xs, xs]}, raised(needs.p, 'Needs You', needs.rows, needs.total))] : []),
      ...(done ? [frame(`${id}-done`, 'Done', {width, layout: 'vertical', padding: [0, xs, xs, xs]}, raised(done.p, 'Done', done.rows, done.total))] : []),
      ...(home ? [homeBlock(id, home)] : []),
      frame(`${id}-list`, agentsTab ? 'Agents list' : 'Projects list', {width, ...(height ? {height: 'fill_container'} : {}), layout: 'vertical', padding: [0, xs], clip: Boolean(height)}, rows),
      footer(id),
    ]);
  }

  function build(s) {
    const id = key => `psb-${key}-${s}`;
    const mark = {question: 1, working: 2, done: 1};
    // The rest state the review target names: This Mac in front, its Needs You
    // and Done groups over Projects, the Overview child of herdr-ide selected.
    const rest = deviceSidebar('sidebar', s, {
      name: 'This Mac', needs: {p: `psb-nu-${s}`, rows: [['deploy'], ['blog']], total: 2}, done: {p: `psb-dn-${s}`, rows: [['ci']], total: 1}, home: {count: '6 projects', agents: ['blog', 'research']},
      rows: [
        section(`psb-sec-pin-${s}`, 'Pinned · 1'),
        folderRow(`psb-p-notes-${s}`, {name: 'team-notes', marks: {idle: 1}, purpose: '회의록 요약 정리'}),
        section(`psb-sec-recent-${s}`, 'Projects · Recent activity · 5'),
        projectRow(`psb-p-herdr-${s}`, {name: 'herdr-ide', marks: {question: 3, working: 5, done: 1, idle: 1}, expanded: true}),
        group(`psb-g-main-${s}`, [
          checkoutRow(`psb-c2-${s}`, {name: 'main', kind: 'primary', age: 'now', marks: {question: 2, working: 4, idle: 1}, purpose: '사이드바 가독성 개선', expanded: true}),
          // An unfolded parent: its chevron waits in the slot, its children follow.
          agentRow(`psb-a1-${s}`, {title: '사이드바 가독성 개선', status: 'working', age: '1m', fold: 'unfolded'}),
          agentRow(`psb-a1c1-${s}`, {title: '컴포넌트 구…', status: 'working', provider: 'codex', age: '42s', depth: 1, branch: 'feat/ui'}),
          agentRow(`psb-a1c2-${s}`, {title: '한글 가독성 확인', status: 'seen', age: '38s', depth: 1}),
          // A folded parent waiting on its children: ring in Working, the badge, the chevron shown.
          foldedAgent(`psb-a2-${s}`, {title: '후속 UX 계획 인터뷰', status: 'working', age: '2m', badge: '?1', fold: 'folded'}, [
            {status: 'working', branch: 'agent-sleep', pr: '#183'},
            {status: 'done', branch: 'mailbox-decouple', device: 'mini'},
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
      ],
    });

    // The Home row at rest and under the pointer, where + starts a tab in Home.
    const homeCut = (key, hover) => frame(id(key), hover ? 'Home row under the pointer' : 'Home row', {width, layout: 'vertical', padding: xs, fill: '$--sidebar', cornerRadius: '$--radius-sm'}, [
      homeRow(`${id(key)}-row`, {count: '6 projects', hover}),
      agentRow(`${id(key)}-a0`, {...AGENTS.blog, place: undefined, line: undefined, inset: 30}),
    ]);

    // Agents tab: this device's own agents by state with the three counts above, and no device chip on any row.
    const agentsTab = deviceSidebar('agents', s, {
      name: 'This Mac', icons: ['search'], height: WIN_H, agentsTab: true, counts: [['needs_you', 2], ['done', 1], ['working', 3]],
      rows: [
        section(`psb-in-s0-${s}`, 'Needs You · 2'),
        agentRow(`psb-in-a0-${s}`, {...AGENTS.deploy, inset: sm}), agentRow(`psb-in-a1-${s}`, {...AGENTS.blog, inset: sm}),
        section(`psb-in-s1-${s}`, 'Done · 1'), agentRow(`psb-in-a3-${s}`, {...AGENTS.ci, inset: sm}),
        section(`psb-in-s2-${s}`, 'Working · 3'),
        agentRow(`psb-in-a4-${s}`, {...AGENTS.readable, inset: sm, selected: true}), agentRow(`psb-in-a6-${s}`, {...AGENTS.principle, inset: sm}),
        section(`psb-in-s3-${s}`, 'Seen · 2'), agentRow(`psb-in-a7-${s}`, {...AGENTS.research, inset: sm}), agentRow(`psb-in-a8-${s}`, {...AGENTS.sasu, inset: sm}),
      ],
    });

    // A remote device in front: its own Home and Projects, tagged Remote; its
    // twelve Needs You draw the five most recent and fold the rest.
    const remoteNeeds = [
      ['batch', {place: 'Home'}],
      ['batch', {title: '릴리스 노트 검토', age: '7m', line: '이 문구로 확정할까요?', place: 'hide › main'}],
      ['batch', {title: '프런트모스트 창 고정', provider: 'claude', age: '12m', line: '접근성 권한을 요청할까요?', place: 'hide › quick/246-frontmost'}],
      ['batch', {title: '판정 로그 재실행', age: '20m', line: '실패한 3건을 다시 돌릴까요?', place: 'sasu › main'}],
      ['batch', {title: '디스크 정리', provider: 'claude', age: '31m', line: '캐시 12 GB를 지울까요?', place: 'Home'}],
    ];
    const remote = deviceSidebar('remote', s, {
      name: 'mini', tag: 'Remote', height: WIN_H, needs: {p: `psb-rm-${s}`, rows: remoteNeeds, total: 12}, home: {count: '2 projects', agents: ['batch']},
      rows: [
        section(`psb-rm-sp-${s}`, 'Projects · Recent activity · 2'),
        projectRow(`psb-rm-p0-${s}`, {name: 'hide', marks: {working: 1}, expanded: true}),
        checkoutRow(`psb-rm-p0m-${s}`, {name: 'main', kind: 'primary', age: '1m', purpose: '릴리스 빌드 확인', marks: {working: 1}, selected: true}),
        checkoutRow(`psb-rm-p0w-${s}`, {name: 'quick/246-frontmost', kind: 'open', age: '3h', purpose: '#246 frontmost 창 고정'}),
        projectRow(`psb-rm-p1-${s}`, {name: 'sasu'}),
      ],
    });

    // A device that is not connected: its name, the state, one action; the last tree is not drawn.
    const offBody = frame(`psb-off-body-${s}`, 'Not connected', {width, height: 'fill_container', layout: 'vertical', gap: sm, alignItems: 'center', padding: [120, '$--spacing-md', 0, '$--spacing-md']}, [
      icon(`psb-off-g-${s}`, 'server', {size: 20, fill: MUTED}),
      text(`psb-off-n-${s}`, 'mini', {size: '$--text-subhead', weight: '600'}),
      text(`psb-off-s-${s}`, '연결 안 됨', {size: '$--text-caption', fill: MUTED}),
      screenButton(`psb-off-b-${s}`, '다시 연결', {variant: 'secondary', height: num(tokens, '--size-control-sm'), icon: 'refresh-cw'}),
    ]);
    const off = frame(`psb-off-${s}`, 'Sidebar', {width, height: WIN_H, layout: 'vertical', fill: '$--sidebar', clip: true}, [
      headerLine(`psb-off-${s}`, {name: 'mini', tag: 'Remote', icons: ['search']}), ruleLine(`psb-off-r0-${s}`), offBody, footer(`psb-off-${s}`),
    ]);

    // One device: the rail still shows with This Mac alone and its unseen Done dot, and the Home row heads Projects.
    const one = deviceSidebar('one', s, {
      height: WIN_H, home: {count: '4 projects', agents: ['blog', 'research']},
      rows: [
        section(`psb-one-sp-${s}`, 'Projects · Recent activity · 4'),
        projectRow(`psb-one-p0-${s}`, {name: 'herdr-ide', marks: mark, expanded: true}),
        checkoutRow(`psb-one-p0m-${s}`, {name: 'main', kind: 'primary', age: 'now', purpose: '사이드바 가독성 개선', marks: {question: 1, working: 1}, selected: true}),
        checkoutRow(`psb-one-p0w-${s}`, {name: 'feat/home-device-rail', kind: 'open', age: '12m', purpose: 'Home · 기기 레일', marks: {working: 1}}),
        projectRow(`psb-one-p1-${s}`, {name: 'oh-my-principle', marks: {working: 1}}),
        projectRow(`psb-one-p2-${s}`, {name: 'sasu', marks: {idle: 1}}),
      ],
    });

    // The rail hidden: the name is the device menu, and the menu is open under it.
    const hiddenMenu = frame(id('hidden-menu'), 'Device menu', {layout: 'vertical', gap: 0}, [
      screenMenuContent(id('hidden-menu-content'), 220, [
        screenMenuItem(`psb-hm-0-${s}`, 'This Mac'), screenMenuItem(`psb-hm-1-${s}`, 'mini'), screenMenuItem(`psb-hm-2-${s}`, 'build-box · 연결 안 됨'),
        screenMenuSeparator(`psb-hm-s-${s}`),
        screenMenuItem(`psb-hm-3-${s}`, '기기 추가…'), screenMenuItem(`psb-hm-4-${s}`, '레일 표시'),
      ]),
    ]);
    const hidden = deviceSidebar('hidden', s, {
      name: 'This Mac', menu: true, height: WIN_H, home: {count: '4 projects', agents: ['blog', 'research']},
      rows: [
        section(`psb-hid-sp-${s}`, 'Projects · Recent activity · 2'),
        projectRow(`psb-hid-p0-${s}`, {name: 'herdr-ide', marks: mark}),
        projectRow(`psb-hid-p1-${s}`, {name: 'sasu', marks: {idle: 1}}),
      ],
    });

    const labeled = (key, label, children) => frame(id(`cap-${key}`), label, {layout: 'vertical', gap: '$--spacing-sm', alignItems: 'start'}, [
      text(`${id(`cap-${key}`)}-l`, label, {size: '$--text-caption', weight: '600', fill: MUTED}), ...children,
    ]);
    const paired = (key, selected, sidebar, options) => frame(id(`pair-${key}`), 'Rail and sidebar', {layout: 'horizontal', gap: 0, alignItems: 'start'}, [deviceRail(id(key), selected, options), sidebar]);
    return [frame(id('wrap'), 'Wrap', {layout: 'vertical', gap: '$--spacing-xl'}, [
      frame(id('row-rest'), 'This Mac in front', {layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'start'}, [
        labeled('rest', 'This Mac in front · rail, Projects | Agents, Needs You, Done, Home, Projects', [frame(id('pair-rest'), 'Rail and sidebar', {layout: 'horizontal', gap: 0, alignItems: 'start'}, [deviceRail(id('rest'), 'mac'), rest])]),
        labeled('home', 'Home row · rest and under the pointer', [homeCut('home0', false), homeCut('home1', true)]),
        hoverState(s), menuStates(s),
      ]),
      frame(id('row-states'), 'Rail states', {layout: 'horizontal', gap: '$--spacing-xl', alignItems: 'start'}, [
        labeled('agents', 'Agents tab · this device only, the three counts above, no chip', [paired('agents', 'mac', agentsTab)]),
        labeled('remote', 'mini in front · 9+ pill, Needs You past its cap', [paired('remote', 'mini', remote)]),
        labeled('off', 'mini not connected · dimmed glyph and x, no mark', [paired('off', 'mini', off, {miniOff: true})]),
        labeled('one', 'One device · the rail shows with This Mac alone', [frame(id('pair-one'), 'Rail and sidebar', {layout: 'horizontal', gap: 0, alignItems: 'start'}, [deviceRail(id('one'), 'mac', {tiles: [['mac', {selected: true, done: true}]]}), one])]),
        labeled('hidden', 'Rail hidden · the name is the device menu', [frame(id('pair-hidden'), 'Sidebar and menu', {layout: 'horizontal', gap: '$--spacing-md', alignItems: 'start'}, [hidden, hiddenMenu])]),
      ]),
    ])];
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
  return screenSheet('screen-projects-sidebar', 'Screen / Projects Sidebar', 'sidebar.tsx, sidebar-header.tsx, projects.ts (quick device-rail-badges, replacing PRD home-device-rail D-09..D-14): the sidebar follows a device rail that is always shown (quick device-rail-slack). The rail is the sidebar’s full-height left column: This Mac and each registered device as a 32 tile with no name under it (the laptop glyph, or the monogram of the device’s name; the hint is the name with its counts in full), the selected tile ringed 2 off its edge, one mark notched into a tile’s top-right for its most urgent state (the Needs You count, ten or more reading 9+, else a dot for unseen Done), no mark for Working, an unreachable device with its glyph dimmed and a x at the bottom-right, and + directly under the last tile to add a device, dashed in the app and drawn solid here. Every device’s sidebar has a header line with the device in front (This Mac, mini Remote) and Add project and Search at its end, then the Projects | Agents strip. Projects holds the device’s Needs You and Done groups first, each drawing its five (Needs You) or three (Done) most recent agents and folding the rest behind a More N row, then its Home row (house glyph, the project count, + under the pointer for a new tab in Home) with the agents that belong to no project under it, then Projects. Agents holds the device’s own agents as Needs You, Done, Working and Seen with Needs You N · Done N · Working N above and no device chip on any row. The rest frame is This Mac in front with the main checkout of herdr-ide selected; a remote device in front draws its own Home and Projects; a device that is not connected draws its name, 연결 안 됨 and 다시 연결, and no tree. With one device the rail still shows with This Mac alone. With the rail hidden the name on the header line carries a chevron and opens the device menu (the devices, 기기 추가…, 레일 표시). In the list, pinned and activity-ordered projects, checkout rows with their kind glyph, age and agent line, an opened checkout’s agent rows, and both inactive folds. Every line ends in its time or status badge and then a fold slot, so names never move and the times, badges and chevrons stand in one column each. A row’s menu opens on a right-click, with nothing drawn for it; beside the sidebar each row kind is drawn with its menu open (Project: New worktree…, New tab in main, Reveal in Finder, Copy path, Pin, Remove project…; Checkout: Open, New tab here, Open pull request, Set purpose…, Set as default checkout, Copy branch name, Copy path, Reveal in Finder, Delete worktree…; Agent: Show, Copy title, Copy session id, Close tab…). A status badge counts agents under the mark each agent’s own row draws, worst first (× ! ? ● ✓ ○). A checkout row opens the checkout and unfolds its agents; clicking its already selected, unfolded Workspace folds them without leaving it, and the chevron changes disclosure alone. A checkout name is 12/400 with its prefix up to the first slash muted. Line two is the purpose with the last-commit age on the time column, drawn only for a purpose or a raised-from parent. A parent agent folds its children with the same chevron and speaks for them with its badge; a folded parent draws one line per other checkout, with the server-glyph device chip. The kind glyph is the pull request’s lifecycle when GitHub knows one, else folder, primary, detached or branch; a missing folder is danger with no age. Beside the sidebar: a pull-request row under the pointer with its card (Component / PR hover card) opened to its right, the glyph a button that opens the pull request.', build, build);
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
  // The list's + opens the start sheet (PRD home-device-rail B42).
  function startButton(id) {
    return frame(id, 'New agent', {width: 32, height: 32, cornerRadius: PILL, fill: '$--secondary', alignItems: 'center', justifyContent: 'center'}, [icon(`${id}-i`, 'plus', {size: 18, fill: FG})]);
  }
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
      {title: 'mailbox 플러그인 구현', status: 'question', kind: 'codex', project: 'herdr-ide', branch: 'prd/mailbox-plugin', machine: 'mini', age: '9m', request: '워크스페이스 이름을 어떤 걸로 할까요?'},
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
      header(`${id}-h`, {title: 'hide', sub: `${MACHINE} · 폰 1대 더 연결됨`, connected: !unreachable, trailing: startButton(`${id}-new`)}),
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
        frame(`${id}-logo`, 'Logo', {width: 64, height: 64, cornerRadius: '$--radius-xl', clip: true, fill: {type: 'image', enabled: true, url: '../web/public/m/icon-192.png', mode: 'fit'}}, []),
        text(`${id}-t`, `${MACHINE}과 연결`, {size: 22, weight: '600', fill: FG}),
        text(`${id}-d`, '이 폰에서 hide의 에이전트를 보고, 기다리는 에이전트에 답할 수 있어요.', {size: '$--text-title', fill: MUTED, width: CONTENT_W - 32, align: 'center'}),
        frame(`${id}-sp`, 'Gap', {height: LG, width: 1}, []),
        button(`${id}-ok`, '연결', {primary: true, width: CONTENT_W, height: 48, size: 16}),
        text(`${id}-n`, '코드는 5분 안에 만료돼요. 만료되면 맥에서 QR을 다시 여세요.', {size: '$--text-body', fill: MUTED, width: CONTENT_W - 32, align: 'center'}),
      ]),
    ];
  }

  // -- phone: start sheet --
  // The bottom sheet over the dimmed list: what to do, then target, kind and model
  // as one field each (kind and model start on the desktop's remembered values,
  // the target on This Mac · Home), and 시작.
  function startField(id, label, value, leadNode) {
    return frame(id, label, {layout: 'horizontal', gap: SP.sm, alignItems: 'center', width: CONTENT_W, height: 44, padding: [0, SP.md], fill: '$--secondary', cornerRadius: 12}, [
      text(`${id}-l`, label, {size: '$--text-body', fill: MUTED, width: 40}),
      ...(leadNode ? [leadNode] : []),
      text(`${id}-v`, value, {size: 15, fill: FG}),
      spacer(`${id}-s`),
      icon(`${id}-c`, 'chevron-down', {size: 16, fill: MUTED}),
    ]);
  }
  function startBody(id) {
    const bodyH = PHONE_H - STATUS_H - HOME_H;
    const sheetH = 380;
    const sheet = frame(`${id}-sheet`, 'Start sheet', {layout: 'vertical', gap: SP.md, width: PHONE_W, height: sheetH, padding: [SP.sm, SP.lg, SP.lg, SP.lg], fill: '$--popover', cornerRadius: [24, 24, 0, 0], stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      frame(`${id}-grab`, 'Grab', {width: CONTENT_W, height: 12, alignItems: 'center', justifyContent: 'center'}, [frame(`${id}-grabb`, 'Bar', {width: 36, height: 4, cornerRadius: PILL, fill: MUTED, opacity: 0.6}, [])]),
      frame(`${id}-box`, 'Text box', {layout: 'vertical', width: CONTENT_W, height: 96, padding: SP.md, fill: '$--secondary', cornerRadius: 12}, [
        text(`${id}-q`, '블로그 초안을 두 가지 톤으로 다시 써서 비교해줘', {size: 15, fill: FG, width: CONTENT_W - 2 * MD}),
      ]),
      startField(`${id}-target`, '대상', 'This Mac · Home', icon(`${id}-tg`, 'house', {size: 14, fill: MUTED})),
      startField(`${id}-kind`, '종류', 'Claude', provider(`${id}-kg`, 'claude', 16)),
      startField(`${id}-model`, '모델', 'opus', icon(`${id}-mg`, 'cpu', {size: 14, fill: MUTED})),
      button(`${id}-go`, '시작', {primary: true, width: CONTENT_W, height: 48, size: 16}),
    ]);
    return [frame(`${id}-stack`, 'List under the sheet', {layout: 'none', width: PHONE_W, height: bodyH, clip: true}, [
      {...frame(`${id}-dim`, 'Dimmed list', {layout: 'vertical', width: PHONE_W, height: bodyH, opacity: 0.4}, listBody(`${id}-l`)), x: 0, y: 0},
      {...sheet, x: 0, y: bodyH - sheetH},
    ])];
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
        labeled(p('cap-start'), '⑦ 시키기', '목록의 +가 여는 시트. 글, 대상(기본 This Mac · Home), 종류, 모델. 종류와 모델은 데스크톱의 마지막 값으로 미리 골라져 있고 시작하면 그 에이전트 상세로 간다.', phone(p('start'), 'Phone · Start', startBody(p('start-b')))),
      ]),
    ])];
  }
  return screenSheet('screen-mobile', 'Screen / Mobile', 'web/src/MobileTab.tsx, mobile.ts, and the phone app under web/src/mobile/ (entry web/mobile.html), PRD mobile-companion D-08 and D-12: Settings > Mobile after Devices, blocked with only the failing check lit and one action beside it and no QR, then ready with every check passing, the QR, the ts.net address, the code countdown with 새 코드, the connected phones (2 / 4) with 해지 and the automatic-removal notice, and the three push modes. The phone app: the pairing confirm the QR opens, the list in four groups (내 확인 대기, 끝, 진행 중, 확인함) on the desktop row rules at phone sizes, the detail with its read-only scrollback, five quick keys and one-line reply, the unreachable state (one line naming both causes over the dimmed last list), the empty state, the push banner that opens the detail, and the start sheet the list’s + opens (PRD home-device-rail D-24): a text box, target (default This Mac · Home), kind, model and 시작 over the dimmed list. The machine name and tailnet are placeholders and the QR is never a real code.', build, build);
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

function buildOverview(tokens) {
  const build = suffix => ['all', 'project', 'zero', 'narrow', 'page'].map((state, index) => {
    const id = `shared-overview-${suffix}-${state}`;
    const narrow = state === 'narrow';
    const width = narrow ? 600 : 960;
    const contentWidth = width - 2 * num(tokens, '--spacing-lg');
    const project = state === 'project';
    const themeIndex = suffix === 'l' ? 2 : 3;
    const original = project
      ? buildProjectOverview(tokens).children[themeIndex].children[0].children[0]
      : buildMain(tokens).children[themeIndex].children[1].children.find(node => node.name === 'Overview · 요청');
    const copy = node => ({...node, id: `${id}-${node.id}`, ...(typeof node.width === 'number' ? {width: Math.min(node.width, contentWidth)} : {}), ...(node.children ? {children: node.children.map(copy)} : {})});
    const content = copy(original);
    content.width = contentWidth;
    return frame(id, `Overview / ${state}`, {width, height: 640, layout: 'vertical', gap: '$--spacing-sm', fill: '$--popover', cornerRadius: '$--radius-lg', stroke: '$--border', padding: '$--spacing-lg', clip: true}, [
      frame(`${id}-sidebar`, 'Shared sidebar entry', {width: 'fill_container', layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
        icon(`${id}-entry-icon`, 'layout-dashboard', {size: num(tokens, '--size-icon'), fill: '$--subtle-foreground'}),
        text(`${id}-entry-label`, 'Overview', {size: '$--text-body'}),
        ...(state !== 'zero' ? [text(`${id}-asking`, '2 asking', {size: '$--text-caption', fill: '$--warning'})] : []),
        text(`${id}-entry-key`, '⌘⇧O', {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
        screenToggleGroup(`${id}-sidebar-tabs`, ['Projects · ⌘⇧P', 'Agents · ⌘⇧A'], index % 2),
      ]),
      frame(`${id}-toolbar`, 'Workspace toolbar icons', {width: 'fill_container', layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
        frame(`${id}-overview-control`, 'Overview', {layout: 'horizontal', gap: '$--spacing-xxs'}, [screenIconButton(`${id}-overview`, 'layout-dashboard'), ...(state !== 'zero' ? [frame(`${id}-dot`, 'Needs You dot', {width: num(tokens, '--size-tab-status-dot'), height: num(tokens, '--size-tab-status-dot'), fill: '$--warning', cornerRadius: '$--radius-lg'}, [])] : [])]),
        screenIconButton(`${id}-server`, 'globe'), screenIconButton(`${id}-files`, 'panel-left'),
        text(`${id}-filecount`, '3', {size: '$--text-caption', fill: '$--primary'}), screenIconButton(`${id}-tools`, 'panel-right'),
      ]),
      frame(`${id}-header`, 'Overview scope', {width: 'fill_container', layout: 'horizontal', gap: '$--spacing-md', alignItems: 'center'}, [
        text(`${id}-title`, 'Overview', {size: '$--text-title', weight: '600'}),
        screenToggleGroup(`${id}-scope`, ['All projects', 'herdr-ide'], project ? 1 : 0),
        frame(`${id}-space`, 'Spacer', {width: 'fill_container', height: 1}, []),
        screenButton(`${id}-close`, 'Esc', {variant: 'ghost'}),
      ]),
      content,
    ]);
  });
  return screenSheet('screen-overview', 'Screen / Overview', 'Shared Overview modal over the mounted Workspace, or a central page. Approved 2026-10-04: Overview before Open server; File Views badge retained; zero hides count and dot; Light, narrow windows and selected scope use existing tokens and patterns. Existing content is reused below the shared scope header. The entry and toolbar state rows document their placement separately.', build, build);
}

// -- Screen / Onboarding ---------------------------------------------------------

// The first-run agent choice (web/src/AgentOnboarding.tsx): a Dialog over a
// grid of square tiles, one per adapter. A set-up agent's tile is on or off, an
// agent that is not set up is dimmed with no switch. Logos are the bundled
// marks of web/src/assets/agents (manifest.json names each source); an agent
// with no mark that may be bundled draws a monogram, never a drawn logo.
function buildOnboarding(tokens) {
  const W = num(tokens, '--size-onboarding-dialog-w');
  const LOGO = num(tokens, '--size-agent-logo');
  const GAP = num(tokens, '--spacing-sm');
  const COLUMNS = 4;
  const INNER = W - 2 * num(tokens, '--spacing-lg');
  const TILE = Math.floor((INNER - (COLUMNS - 1) * GAP) / COLUMNS);
  const dimmed = num(tokens, '--opacity-dimmed');
  const MARKS = {
    'claude-code': 'agent-claude.png', codex: 'agent-codex.png', opencode: 'agents/opencode.svg', cursor: 'agents/cursor.svg',
    'qwen-code': 'agents/qwen-code.svg', goose: 'agents/goose.svg',
    cline: 'agents/cline.svg', 'kilo-code': 'agents/kilo-code.svg', 'mistral-vibe': 'agents/mistral-vibe.svg',
  };
  // [id, label, state]: on, off, or none (not set up on this machine).
  const AGENTS = [
    ['claude-code', 'Claude Code', 'on'], ['codex', 'Codex', 'on'], ['opencode', 'OpenCode', 'on'], ['gemini-cli', 'Gemini CLI', 'off'],
    ['cursor', 'Cursor', 'none'], ['copilot-cli', 'Copilot CLI', 'none'], ['amp', 'Amp', 'none'], ['factory-droid', 'Factory Droid', 'none'],
    ['kiro', 'Kiro', 'none'], ['qwen-code', 'Qwen Code', 'none'], ['goose', 'Goose', 'none'], ['cline', 'Cline', 'none'],
    ['kilo-code', 'Kilo Code', 'none'], ['crush', 'Crush', 'none'], ['junie', 'Junie', 'none'], ['augment', 'Augment', 'none'],
    ['pi', 'Pi', 'none'], ['grok', 'Grok', 'none'], ['kimi-code', 'Kimi Code', 'none'], ['mistral-vibe', 'Mistral Vibe', 'none'],
  ];
  const monogramOf = label => {
    const words = label.split(/[\s-]+/).filter(Boolean);
    return (words.length > 1 ? words.slice(0, 2).map(word => word[0]) : [...words[0]].slice(0, 2)).join('').toUpperCase();
  };
  function build(suffix) {
    const id = name => `onb-${name}-${suffix}`;
    const tile = ([agent, label, state]) => {
      const on = state === 'on';
      const plate = frame(id(`${agent}-plate`), 'Logo plate', {width: LOGO, height: LOGO, cornerRadius: '$--radius-md', fill: '$--logo-plate', layout: 'horizontal', alignItems: 'center', justifyContent: 'center', clip: true},
        MARKS[agent]
          ? [frame(id(`${agent}-logo`), 'Logo', {width: LOGO - 8, height: LOGO - 8, fill: {type: 'image', enabled: true, url: `../web/src/assets/${MARKS[agent]}`, mode: 'fit'}}, [])]
          : [text(id(`${agent}-mono`), monogramOf(label), {mono: true, weight: '600', fill: '$--muted-foreground'})]);
      const children = [
        plate,
        text(id(`${agent}-name`), label, {weight: '600', fill: '$--foreground'}),
        text(id(`${agent}-state`), state === 'none' ? 'Not installed' : on ? 'On' : 'Off', {size: '$--text-caption', fill: '$--muted-foreground'}),
      ];
      if (state !== 'none') {
        children.push(frame(id(`${agent}-check`), 'Check', {
          width: 16, height: 16, cornerRadius: 8, layout: 'horizontal', alignItems: 'center', justifyContent: 'center',
          ...(on ? {fill: '$--primary'} : {}), stroke: on ? '$--primary' : '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner', layoutPosition: 'absolute', x: TILE - 16 - 8, y: 8,
        }, on ? [icon(id(`${agent}-checki`), 'check', {size: 12, fill: '$--primary-foreground'})] : []));
      }
      return frame(id(`${agent}-tile`), `Tile · ${label} · ${state}`, {
        width: TILE, height: TILE, layout: 'vertical', alignItems: 'center', justifyContent: 'center', gap: '$--spacing-xs', padding: '$--spacing-sm',
        cornerRadius: '$--radius-md', stroke: on ? '$--primary' : '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner',
        ...(on ? {fill: '$--card'} : {}), ...(state === 'none' ? {opacity: dimmed} : {}),
      }, children);
    };
    const rows = [];
    for (let at = 0; at < AGENTS.length; at += COLUMNS) {
      rows.push(frame(id(`row-${at / COLUMNS}`), 'Row', {layout: 'horizontal', gap: GAP}, AGENTS.slice(at, at + COLUMNS).map(tile)));
    }
    return [screenDialogSurface(id('surface'), {
      width: W, title: 'Choose the agents Hide works with', prose: true,
      description: 'Each agent you leave on gets a skill for driving Hide’s browser and, where the agent supports one, a session hook. Hide edits only its own entries, and you can change this any time in Settings, Agents.',
      body: [
        frame(id('grid'), 'Grid', {layout: 'vertical', gap: GAP}, rows),
        text(id('devices'), 'Connected devices get the same choice, each by what is installed there.', {size: '$--text-caption', fill: '$--muted-foreground', width: INNER}),
      ],
      actions: [screenButton(id('apply'), 'Apply')],
    })];
  }
  return screenSheet('screen-onboarding', 'Screen / Onboarding', 'web/src/AgentOnboarding.tsx over Dialog (agent adapters PRD, first-run choice): shown while the core says the choice is pending, and only Apply ends it. One square tile per adapter in four columns, each with its logo on a --logo-plate square (the bundled marks of web/src/assets/agents, each with its source in manifest.json) or, where no mark may be bundled, a monogram; never a drawn or approximated logo. An agent set up on the machine is on by default and shows its state as a word and a check mark as well as the primary border; an agent that is not set up is dimmed, reads Not installed and has no switch. Apply is the only button: Escape, a click outside and a close button do nothing, so a stray key cannot finish a choice that leaves Claude Code and Codex off; Apply installs the agents left on here and on every device that waits for the choice.', build, build);
}

export function screenSheets(tokens, root) {
  setLibraryRoot(root, tokens);
  return [
    {name: 'Screen / Overview', build: () => buildOverview(tokens)},
    {name: 'Screen / Main', build: () => buildMain(tokens)},
    {name: 'Screen / Project Overview', build: () => buildProjectOverview(tokens)},
    {name: 'Screen / Workspace', build: () => buildWorkspace(tokens)},
    {name: 'Screen / Project Sessions', build: () => buildSessions(tokens)},
    {name: 'Screen / Settings', build: () => buildSettings(tokens)},
    {name: 'Screen / Palette', build: () => buildPalette(tokens)},
    {name: 'Screen / Dialogs and Sheets', build: () => buildDialogs(tokens)},
    {name: 'Screen / Menus and Overlays', build: () => buildMenus(tokens)},
    {name: 'Screen / Projects Sidebar', build: () => buildProjectsSidebar(tokens)},
    {name: 'Screen / Mobile', build: () => buildMobile(tokens)},
    {name: 'Screen / Disk Cleanup', build: () => buildDiskCleanup(tokens)},
    {name: 'Screen / Onboarding', build: () => buildOnboarding(tokens)},
  ];
}
