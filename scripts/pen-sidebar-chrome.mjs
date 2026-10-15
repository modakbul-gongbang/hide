// `Component / Sidebar chrome` (PRD sidebar-titlebar-refresh D-05, D-08, D-11,
// D-12): the parts around the sidebar's list as web/src/components/sidebar-header.tsx,
// web/src/sidebar.tsx and web/src/components/help-menu.tsx render them. The Search
// row and the section header are reusable masters; every state in the Light and
// Dark frames is a `ref` of one with descendant overrides, as pen-system.mjs's
// System parts are. The Help menu is drawn from System / Dropdown Menu's item and
// separator masters. The screens in design/hide-screens.pen ref these masters by
// id, so the ids are stable.
import {frame, icon, num, text} from './pen-system.mjs';

export const SIDEBAR_CHROME_MASTERS = {
  search: 'sbc-search',
  section: 'sbc-section',
};

export function buildSidebarChrome(tokens) {
  const n = name => num(tokens, name);
  const WIDTH = n('--size-sidebar-ideal');
  const ROW = WIDTH - 2 * n('--spacing-xs');
  const ICON = n('--size-icon-sm');
  const ref = (id, master, name, overrides = {}, descendants) => ({id, type: 'ref', ref: master, name, ...overrides, ...(descendants ? {descendants} : {})});
  const cell = (label, instance) => frame(`${instance.id}-cell`, label, {layout: 'vertical', gap: '$--spacing-xs', alignItems: 'start', width: 'fit_content'}, [
    text(`${instance.id}-cap`, label, {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
    instance,
  ]);

  // The Search row under the window's top row: a field-shaped button that reads
  // Search and the bound ⌘K chord, and opens the palette (D-08).
  const search = frame(SIDEBAR_CHROME_MASTERS.search, 'Search row', {
    reusable: true, width: WIDTH, layout: 'horizontal', padding: ['$--spacing-sm', '$--spacing-sm', '$--spacing-xs', '$--spacing-sm'], fill: '$--sidebar',
  }, [
    frame('sbc-search-field', 'Field', {
      width: 'fill_container', height: n('--size-sidebar-search'), layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-sm'],
      cornerRadius: '$--radius-sm', fill: '$--background', stroke: '$--border', strokeWidth: 1, strokeAlignment: 'inner',
    }, [
      icon('sbc-search-icon', 'search', {size: ICON, fill: '$--muted-foreground'}),
      {...text('sbc-search-label', 'Search', {fill: '$--muted-foreground'}), textGrowth: 'fixed-width', width: 'fill_container'},
      ref('sbc-search-chord', 'kbd-m', 'Chord', {}, {'kbd-t': {content: '⌘K'}}),
    ]),
  ]);

  // A section header of the Projects list, 10/500 on its baseline in a 26 row. It
  // sticks to the list's top on the sidebar's fill and wears a hairline while rows
  // pass under it; Pinned leads with the pin, and Projects ends in Add project (D-05).
  const section = frame(SIDEBAR_CHROME_MASTERS.section, 'Section header', {
    reusable: true, width: ROW, height: n('--size-sidebar-section-header'), layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'end',
    padding: [0, '$--spacing-xs', '$--spacing-xs', '$--spacing-sm'], fill: '$--sidebar',
  }, [
    icon('sbc-section-pin', 'pin', {size: ICON, fill: '$--muted-foreground', enabled: false}),
    {...text('sbc-section-label', 'Projects · Recent activity · 10', {size: '$--text-micro', weight: '500', fill: '$--muted-foreground'}), textGrowth: 'fixed-width', width: 'fill_container'},
    ref('sbc-section-add', 'Nyvom', 'Add project', {width: n('--size-sidebar-line'), height: n('--size-sidebar-line'), enabled: false}, {ZIZFR: {icon: 'plus', width: ICON, height: ICON}}),
  ]);

  // The Help menu the footer's ? opens: three items, a separator and the version line, which takes no press (D-12).
  const helpMenu = suffix => frame(`sbc-help-${suffix}`, 'Help menu', {
    layout: 'vertical', gap: 0, padding: '$--spacing-xxs', width: 220, cornerRadius: '$--radius-sm',
    fill: '$--popover', stroke: '$--border', strokeWidth: 1, strokeAlignment: 'inner',
  }, [
    ...[['shortcuts', 'Keyboard shortcuts', '⌘/'], ['diagnostics', 'Copy diagnostics'], ['issue', 'Report an issue']].map(([key, label, shortcut]) => ref(`sbc-help-${suffix}-${key}`, 'mnu-item-m', label, {}, {
      'mnu-item-icon': {enabled: false},
      'mnu-item-label': {content: label},
      'mnu-item-reason': {enabled: false},
      'mnu-item-shortcut': shortcut ? {content: shortcut, enabled: true} : {enabled: false},
    })),
    ref(`sbc-help-${suffix}-sep`, 'mnu-sep-m', 'Separator'),
    frame(`sbc-help-${suffix}-version`, 'Version', {width: 'fill_container', padding: ['$--spacing-xs', '$--spacing-sm']}, [
      text(`sbc-help-${suffix}-version-t`, 'hide 0.4.0', {size: '$--text-caption', fill: '$--muted-foreground', mono: true}),
    ]),
  ]);

  // The View menu the footer's eye opens (PRD agent-logo-view-mode D-01, D-06): the Agent logo label, two choices with a check on the current one,
  // a separator and the key line, which takes no press (⌥ on macOS, Alt on Windows and Linux).
  const viewMenu = suffix => frame(`sbc-view-${suffix}`, 'View menu', {
    layout: 'vertical', gap: 0, padding: '$--spacing-xxs', width: 220, cornerRadius: '$--radius-sm',
    fill: '$--popover', stroke: '$--border', strokeWidth: 1, strokeAlignment: 'inner',
  }, [
    frame(`sbc-view-${suffix}-label`, 'Label', {width: 'fill_container', padding: ['$--spacing-xs', '$--spacing-sm']}, [
      text(`sbc-view-${suffix}-label-t`, 'Agent logo', {size: '$--text-caption', fill: '$--muted-foreground'}),
    ]),
    ...[['shown', 'Show beside each row', false], ['hidden', 'Hide', true]].map(([key, label, checked]) => ref(`sbc-view-${suffix}-${key}`, 'mnu-item-m', label, {}, {
      'mnu-item-icon': checked ? {enabled: true, icon: 'check'} : {enabled: false},
      'mnu-item-label': {content: label},
      'mnu-item-reason': {enabled: false},
      'mnu-item-shortcut': {enabled: false},
    })),
    ref(`sbc-view-${suffix}-sep`, 'mnu-sep-m', 'Separator'),
    frame(`sbc-view-${suffix}-key`, 'Key line', {width: 'fill_container', padding: ['$--spacing-xs', '$--spacing-sm']}, [
      text(`sbc-view-${suffix}-key-t`, 'Hold ⌥ to peek', {size: '$--text-caption', fill: '$--muted-foreground'}),
    ]),
  ]);

  const states = suffix => {
    const id = name => `sbc-${suffix}-${name}`;
    return [
      cell('Search row', ref(id('search'), SIDEBAR_CHROME_MASTERS.search, 'Search row')),
      cell('Pinned · N', ref(id('pinned'), SIDEBAR_CHROME_MASTERS.section, 'Pinned', {}, {'sbc-section-pin': {enabled: true}, 'sbc-section-label': {content: 'Pinned · 2'}})),
      cell('Projects with Add project', ref(id('projects'), SIDEBAR_CHROME_MASTERS.section, 'Projects', {}, {'sbc-section-add': {enabled: true}})),
      cell('Stuck to the top: the hairline', ref(id('stuck'), SIDEBAR_CHROME_MASTERS.section, 'Stuck', {stroke: '$--border', strokeWidth: {bottom: 1}, strokeAlignment: 'inner'}, {'sbc-section-add': {enabled: true}})),
      cell('View menu', viewMenu(suffix)),
      cell('Help menu', helpMenu(suffix)),
    ];
  };
  const themeFrame = (frameId, mode, cells) => frame(frameId, mode, {
    theme: {Mode: mode}, layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'end', padding: '$--spacing-lg',
    fill: '$--background', width: 'fit_content', cornerRadius: '$--radius-md',
  }, cells);

  const id = 'cmp-sidebar-chrome';
  return frame(id, 'Component / Sidebar chrome', {
    layout: 'vertical', gap: '$--spacing-xl', padding: '$--spacing-xl', fill: '$--card', cornerRadius: '$--radius-lg', width: 'fit_content',
  }, [
    text(`${id}-title`, 'Sidebar chrome', {size: '$--text-headline', weight: '600'}),
    text(`${id}-spec`, 'web/src/components/sidebar-header.tsx, sidebar.tsx, view-menu.tsx and help-menu.tsx (PRD sidebar-titlebar-refresh D-05, D-08, D-11, D-12; PRD agent-logo-view-mode D-01, D-06): the Search row under the window\'s top row, a field-shaped button that reads Search and the bound ⌘K chord and opens the palette; the Projects list\'s section header, 10/500 on its baseline in a 26 row on the sidebar\'s fill, which sticks to the list\'s top and wears a hairline while rows pass under it, Pinned led by the pin (its rows carry none) and Projects ending in Add project; the View menu the footer\'s eye opens: the Agent logo label, Show beside each row and Hide with a check on the current one and, under a separator, the key line that names the key held to peek, which takes no press; and the Help menu the footer\'s ? opens: Keyboard shortcuts with its chord, Copy diagnostics, Report an issue and, under a separator, the hide version line, which takes no press.', {size: '$--text-caption', fill: '$--subtle-foreground', width: 820}),
    frame(`${id}-masters`, 'Masters', {layout: 'horizontal', gap: '$--spacing-xl', alignItems: 'start'}, [search, section]),
    themeFrame(`${id}-light`, 'Light', states('l')),
    themeFrame(`${id}-dark`, 'Dark', states('d')),
  ]);
}
