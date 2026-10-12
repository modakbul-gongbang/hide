// `Component / Hide AI origin` (PRD hide-ai-origin-mark D-03, D-04, D-10): the
// two parts that say Hide AI started an agent, as web/src/AgentMark.tsx and
// the Hide AI group of web/src/sidebar.tsx draw them. The head of the group
// in a project's tree, and the provider icon with the Hide badge on its
// lower-right corner where a row stands alone. Each is one reusable master;
// every state in the Light and Dark frames is a `ref` of it with descendant
// overrides, as pen-agent-tree.mjs does. The Hide mark is the full square
// mark's 64-pixel derivative, never an icon of the work (the work is named
// in words by the row).
import {frame, num, text} from './pen-system.mjs';

export const HIDE_AI_ORIGIN_MASTERS = {
  head: 'hao-head',
  badgedIcon: 'hao-badged-icon',
};

const MARK = '../web/src/assets/hide-mark.png';
const image = url => ({type: 'image', enabled: true, url, mode: 'contain'});

export function buildHideAiOrigin(tokens) {
  const n = name => num(tokens, name);
  const LINE = n('--size-sidebar-line');
  const HEAD_MARK = n('--size-icon-sm');
  const ICON = n('--size-agent-badge-compact');
  const BADGE = n('--size-hide-badge');
  const BADGE_MARK = n('--size-hide-badge-mark');
  const HAIR = n('--size-hairline');
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

  // The provider's icon with the badge on its lower-right corner: an 11 plate in the panel's colour (the ring) holding the 9 mark, one hairline past the icon's edge.
  const badgedIcon = frame(HIDE_AI_ORIGIN_MASTERS.badgedIcon, 'Provider icon with Hide badge', {reusable: true, layout: 'none', width: ICON, height: ICON}, [
    frame('hao-icon', 'Provider artwork', {x: 0, y: 0, width: ICON, height: ICON, cornerRadius: '$--radius-xs', fill: image('../web/src/assets/agent-claude.png')}, []),
    frame('hao-badge', 'Hide badge', {
      x: ICON - BADGE + HAIR, y: ICON - BADGE + HAIR, width: BADGE, height: BADGE, cornerRadius: '$--radius-xs', fill: '$--sidebar',
      layout: 'horizontal', alignItems: 'center', justifyContent: 'center',
    }, [
      frame('hao-badge-mark', 'Hide mark', {width: BADGE_MARK, height: BADGE_MARK, cornerRadius: '$--radius-xs', fill: image(MARK)}, []),
    ]),
  ]);

  const states = suffix => {
    const id = name => `hao-${suffix}-${name}`;
    const provider = kind => ({'hao-icon': {fill: image(`../web/src/assets/agent-${kind}.png`)}});
    return [
      cell('Head, two checkouts', ref(id('head'), HIDE_AI_ORIGIN_MASTERS.head, 'Hide AI head')),
      cell('Head, one checkout', ref(id('head-one'), HIDE_AI_ORIGIN_MASTERS.head, 'Hide AI head', {}, {'hao-head-count': {content: '1'}})),
      cell('Badge on Claude', ref(id('badge-claude'), HIDE_AI_ORIGIN_MASTERS.badgedIcon, 'Provider icon with Hide badge')),
      cell('Badge on Codex', ref(id('badge-codex'), HIDE_AI_ORIGIN_MASTERS.badgedIcon, 'Provider icon with Hide badge', {}, provider('codex'))),
      // A Tools column is a card, not the sidebar: the badge's ring takes the panel it sits on.
      frame(id('on-card'), 'Badge on a card panel', {layout: 'vertical', gap: '$--spacing-xs', alignItems: 'start', width: 'fit_content'}, [
        text(id('on-card-cap'), 'Badge on a card panel (Sessions)', {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
        frame(id('on-card-panel'), 'Card panel', {layout: 'horizontal', padding: '$--spacing-sm', fill: '$--card', cornerRadius: '$--radius-sm', width: 'fit_content'}, [
          ref(id('badge-card'), HIDE_AI_ORIGIN_MASTERS.badgedIcon, 'Provider icon with Hide badge', {}, {'hao-badge': {fill: '$--card'}}),
        ]),
      ]),
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
    text(`${id}-spec`, 'web/src/AgentMark.tsx and the Hide AI group of web/src/sidebar.tsx (PRD hide-ai-origin-mark D-03, D-04): Hide AI started the agent, a fact the core reads from the coordination record of the spawn. In a project\'s tree the checkouts Hide AI made gather behind the operator\'s under a head, the full Hide mark at the icon size, the name Hide AI and the number of checkouts; the head exists only while the project has one, and the rows inside carry no badge. Where a row stands alone (the sidebar\'s Needs You and Done, Sessions), the provider\'s icon wears an 11 badge on its lower-right corner, a plate in the panel\'s colour holding the 9 mark; a Tools column is a card, so its badge takes the card\'s colour. The work itself (Memory 올리기 · m72) is words on the row, never an icon, because Hide AI will start other work.', {size: '$--text-caption', fill: '$--subtle-foreground', width: 820}),
    frame(`${id}-masters`, 'Masters', {layout: 'horizontal', gap: '$--spacing-xl', alignItems: 'start'}, [head, badgedIcon]),
    themeFrame(`${id}-light`, 'Light', states('l')),
    themeFrame(`${id}-dark`, 'Dark', states('d')),
  ]);
}
