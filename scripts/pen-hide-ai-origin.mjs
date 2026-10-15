// `Component / Hide AI origin` (PRD hide-ai-origin-mark D-03, D-04, D-10; PRD
// agent-mark-quiet D-04): the two parts that say Hide AI started an agent, as
// web/src/AgentMark.tsx and the Hide AI group of web/src/sidebar.tsx draw
// them. The head of the group in a project's tree, and the Hide owl beside the
// status mark, in the place of a provider logo, where a row stands alone. Each is one reusable master;
// every state in the Light and Dark frames is a `ref` of it with descendant
// overrides, as pen-agent-tree.mjs does. The Hide mark is the full square
// mark's 64-pixel derivative, never an icon of the work (the work is named
// in words by the row).
import {frame, num, text} from './pen-system.mjs';

export const HIDE_AI_ORIGIN_MASTERS = {
  head: 'hao-head',
  owl: 'hao-owl',
};

const MARK = '../web/src/assets/hide-mark.png';
const image = url => ({type: 'image', enabled: true, url, mode: 'contain'});

export function buildHideAiOrigin(tokens) {
  const n = name => num(tokens, name);
  const LINE = n('--size-sidebar-line');
  const HEAD_MARK = n('--size-icon-sm');
  const STATUS = n('--size-agent-mark');
  const DOT = n('--size-status-mark');
  const ICON = n('--size-agent-badge-compact');
  const ref = (id, master, name, overrides = {}, descendants) => ({id, type: 'ref', ref: master, name, ...overrides, ...(descendants ? {descendants} : {})});
  const cell = (label, instance) => frame(`${instance.id}-cell`, label, {layout: 'vertical', gap: '$--spacing-xs', alignItems: 'start', width: 'fit_content'}, [
    text(`${instance.id}-cap`, label, {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
    instance,
  ]);

  // The group's head: its mark, its name and how many checkouts it holds. It exists only while the project has one (D-08).
  const head = frame(HIDE_AI_ORIGIN_MASTERS.head, 'Hide AI head', {
    reusable: true, layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', height: LINE, width: 'fit_content', padding: ['$--spacing-xs', '$--spacing-xs', 0, 0],
  }, [
    frame('hao-head-mark', 'Hide mark', {width: HEAD_MARK, height: HEAD_MARK, cornerRadius: '$--radius-xs', fill: image(MARK)}, []),
    text('hao-head-name', 'Hide AI', {size: '$--text-micro', weight: '500', fill: '$--muted-foreground'}),
    text('hao-head-count', '2', {size: '$--text-micro', weight: '500', fill: '$--muted-foreground'}),
  ]);

  // The owl a row that stands alone draws in the provider logo's place: the full square mark at the icon size.
  const owl = frame(HIDE_AI_ORIGIN_MASTERS.owl, 'Hide owl mark', {reusable: true, width: ICON, height: ICON, cornerRadius: '$--radius-xs', fill: image(MARK)}, []);

  const states = suffix => {
    const id = name => `hao-${suffix}-${name}`;
    const status = (key, fill) => frame(`${id(key)}-status`, 'Status mark', {width: STATUS, height: STATUS, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [
      {type: 'ellipse', id: `${id(key)}-dot`, name: 'Dot', width: DOT, height: DOT, fill},
    ]);
    // A row's lead: the status mark, then the owl (the sidebar's Needs You and Done, Sessions); a row the operator started has the status mark alone.
    const lead = (key, label, withOwl, panel) => frame(`${id(key)}-cell`, label, {layout: 'vertical', gap: '$--spacing-xs', alignItems: 'start', width: 'fit_content'}, [
      text(`${id(key)}-cap`, label, {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
      frame(`${id(key)}-panel`, panel ? 'Card panel' : 'Lead', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', ...(panel ? {padding: '$--spacing-sm', fill: '$--card', cornerRadius: '$--radius-sm'} : {}), width: 'fit_content'}, [
        status(key, '$--agent-working'),
        ...(withOwl ? [ref(id(`${key}-owl`), HIDE_AI_ORIGIN_MASTERS.owl, 'Hide owl mark')] : []),
      ]),
    ]);
    return [
      cell('Head, two checkouts', ref(id('head'), HIDE_AI_ORIGIN_MASTERS.head, 'Hide AI head')),
      cell('Head, one checkout', ref(id('head-one'), HIDE_AI_ORIGIN_MASTERS.head, 'Hide AI head', {}, {'hao-head-count': {content: '1'}})),
      lead('alone', 'Row started by Hide AI', true, false),
      lead('operator', 'Row the operator started', false, false),
      // Sessions is a card, not the sidebar.
      lead('on-card', 'Row started by Hide AI (Sessions)', true, true),
    ];
  };
  const themeFrame = (frameId, mode, cells) => frame(frameId, mode, {
    theme: {Mode: mode}, layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'end', padding: '$--spacing-lg',
    fill: '$--sidebar', width: 'fit_content', cornerRadius: '$--radius-md',
  }, cells);

  const id = 'cmp-hide-ai-origin';
  return frame(id, 'Component / Hide AI origin', {
    layout: 'vertical', gap: '$--spacing-xl', padding: '$--spacing-xl', fill: '$--card', cornerRadius: '$--radius-lg', width: 'fit_content',
  }, [
    text(`${id}-title`, 'Hide AI origin', {size: '$--text-headline', weight: '600'}),
    text(`${id}-spec`, 'web/src/AgentMark.tsx and the Hide AI group of web/src/sidebar.tsx (PRD hide-ai-origin-mark D-03, D-04, replaced by agent-mark-quiet D-04): Hide AI started the agent, a fact the core reads from the coordination record of the spawn. In a project\'s tree the checkouts Hide AI made gather behind the operator\'s under a head, the full Hide mark at the icon size, the name Hide AI and the number of checkouts; the head exists only while the project has one, and the rows inside carry no owl. Where a row stands alone (the sidebar\'s Needs You and Done, Sessions), the Hide owl stands beside the status mark in the place of the provider\'s logo, which no list row draws (PRD agent-mark-quiet D-01, D-04); a row the operator started has the status mark alone. The work itself (Memory 올리기 · m72) is words on the row, never an icon, because Hide AI will start other work.', {size: '$--text-caption', fill: '$--subtle-foreground', width: 820}),
    frame(`${id}-masters`, 'Masters', {layout: 'horizontal', gap: '$--spacing-xl', alignItems: 'start'}, [head, owl]),
    themeFrame(`${id}-light`, 'Light', states('l')),
    themeFrame(`${id}-dark`, 'Dark', states('d')),
  ]);
}
