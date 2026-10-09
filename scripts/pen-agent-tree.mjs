// `Component / Agent tree parts` (PRD agent-hierarchy-screens D-37 to D-40,
// D-44): the parts every agent tree draws the same way, as
// web/src/components/agent-tree.tsx renders them. Each part is one reusable
// master; every state in the Light and Dark frames is a `ref` of it with
// descendant overrides, as pen-system.mjs's System parts are. The screens in
// design/hide-screens.pen ref these masters by id, so the ids are stable.
import {frame, icon, num, text} from './pen-system.mjs';

export const AGENT_TREE_MASTERS = {
  verb: 'ath-verb',
  prIcon: 'ath-pr-icon',
  mark: 'ath-mark',
  treeButton: 'ath-tree-button',
  row: 'ath-row',
};

const PR_STATES = [
  ['failed', 'git-pull-request', '$--destructive', 'CI 실패'],
  ['pending', 'git-pull-request', '$--pr-pending', 'CI 도는 중 · 리뷰 대기'],
  ['mergeable', 'git-pull-request', '$--success', '머지 가능'],
  ['merged', 'git-merge', '$--pr-merged', '머지됨'],
];

export function buildAgentTreeParts(tokens) {
  const n = name => num(tokens, name);
  const HAIR = n('--size-hairline');
  const MARK = n('--size-agent-mark');
  const DOT = n('--size-status-mark');
  const ICON = n('--size-icon-sm');
  const PR = n('--size-pr-icon');
  const LINE = n('--size-sidebar-line');
  const DETAIL = n('--size-sidebar-line-detail');
  const INDENT = n('--size-lineage-indent');
  const LANE = n('--size-lineage-chevron');
  const RAIL_X = n('--size-lineage-rail-x');
  const ELBOW_Y = n('--size-lineage-elbow-y');
  const DIMMED = n('--opacity-dimmed');
  const caption = (id, content, fill, opts = {}) => text(id, content, {size: '$--text-caption', fill, ...opts});
  const ref = (id, master, name, overrides = {}, descendants) => ({id, type: 'ref', ref: master, name, ...overrides, ...(descendants ? {descendants} : {})});
  const cell = (label, instance) => frame(`${instance.id}-cell`, label, {layout: 'vertical', gap: '$--spacing-xs', alignItems: 'start', width: 'fit_content'}, [
    text(`${instance.id}-cap`, label, {size: '$--text-micro', fill: '$--muted-foreground', weight: '600'}),
    instance,
  ]);

  // Verb: 승인 · 답변 · 확인 · 초안, coloured text with no box (D-37).
  const verb = frame(AGENT_TREE_MASTERS.verb, 'Verb', {reusable: true, layout: 'horizontal', width: 'fit_content'}, [
    caption('ath-verb-t', '승인', '$--warning', {weight: '600'}),
  ]);

  // PR icon: the sidebar's PR mark, no number, in the worst own state's colour; dimmed while GitHub cannot be read (D-39, B24).
  const prIcon = frame(AGENT_TREE_MASTERS.prIcon, 'PR icon', {reusable: true, layout: 'horizontal', width: PR, height: PR, justifyContent: 'center', alignItems: 'center'}, [
    icon('ath-pr-g', 'git-pull-request', {size: PR, fill: '$--destructive'}),
  ]);

  // Descendant mark: `! N` raised, else `● N` working, on a folded parent only (B17).
  const mark = frame(AGENT_TREE_MASTERS.mark, 'Descendant mark', {reusable: true, layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', width: 'fit_content'}, [
    caption('ath-mark-bang', '!', '$--warning', {weight: '600', mono: true}),
    {type: 'ellipse', id: 'ath-mark-dot', name: 'Dot', enabled: false, width: DOT, height: DOT, fill: '$--agent-working'},
    caption('ath-mark-n', '2', '$--warning', {weight: '600', mono: true}),
  ]);

  // Tree button: the pane header's tree icon and direct child count, no word (B21).
  const treeButton = frame(AGENT_TREE_MASTERS.treeButton, 'Tree button', {
    reusable: true, layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', height: DETAIL, padding: [0, '$--spacing-xs'],
    cornerRadius: '$--radius-xs', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner',
  }, [
    icon('ath-tb-tree', 'list-tree', {size: ICON, fill: '$--muted-foreground'}),
    caption('ath-tb-n', '8', '$--subtle-foreground', {mono: true}),
    icon('ath-tb-chev', 'chevron-down', {size: ICON, fill: '$--muted-foreground'}),
  ]);

  // Tree row: one line; the rails and the chevron lane left of the marks, the
  // PR icon and the folded parent's mark before the age; a device line only
  // for a child on another machine (D-38, B14).
  const rail = (id, props) => frame(id, 'Rail', {fill: '$--lineage-rail', ...props}, []);
  const row = frame(AGENT_TREE_MASTERS.row, 'Tree row', {reusable: true, layout: 'horizontal', alignItems: 'start', width: 260, cornerRadius: '$--radius-sm'}, [
    frame('ath-row-rail', 'Ancestor rail', {layout: 'none', width: INDENT, height: LINE + 8, enabled: false}, [rail('ath-row-rail-v', {x: RAIL_X, y: 0, width: HAIR, height: LINE + 8})]),
    frame('ath-row-elbow', 'Elbow', {layout: 'none', width: INDENT, height: LINE + 8, enabled: false}, [
      rail('ath-row-elbow-v', {x: RAIL_X, y: 0, width: HAIR, height: ELBOW_Y}),
      rail('ath-row-elbow-h', {x: RAIL_X, y: ELBOW_Y, width: INDENT - RAIL_X, height: HAIR}),
    ]),
    frame('ath-row-lane', 'Chevron lane', {layout: 'horizontal', width: LANE, height: LINE + 8, justifyContent: 'center', padding: ['$--spacing-xs', 0, 0, 0]}, [
      icon('ath-row-chev', 'chevron-right', {size: ICON, fill: '$--muted-foreground'}),
    ]),
    frame('ath-row-body', 'Body', {layout: 'vertical', gap: 0, width: 'fill_container', padding: ['$--spacing-xs', '$--spacing-xs', '$--spacing-xs', '$--spacing-xs']}, [
      frame('ath-row-l1', 'Line one', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width: 'fill_container', height: LINE}, [
        frame('ath-row-mark', 'Status mark', {width: MARK, height: MARK, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [
          {type: 'ellipse', id: 'ath-row-dot', name: 'Dot', enabled: false, width: DOT, height: DOT, fill: '$--agent-working'},
          {type: 'ellipse', id: 'ath-row-ring', name: 'Ring', width: DOT, height: DOT, stroke: '$--agent-working', strokeWidth: HAIR, strokeAlignment: 'inner'},
          {...caption('ath-row-glyph', '!', '$--warning', {mono: true, weight: '600'}), enabled: false},
        ]),
        frame('ath-row-provider', 'Provider artwork', {width: 14, height: 14, fill: {type: 'image', enabled: true, url: '../web/src/assets/agent-claude.png', mode: 'contain'}}, []),
        {...text('ath-row-title', '에이전트 제목', {fill: '$--subtle-foreground'}), textGrowth: 'fixed-width', width: 'fill_container'},
        ref('ath-row-pr', AGENT_TREE_MASTERS.prIcon, 'PR icon'),
        ref('ath-row-desc', AGENT_TREE_MASTERS.mark, 'Descendant mark', {enabled: false}),
        caption('ath-row-age', '3m', '$--muted-foreground', {mono: true}),
      ]),
      frame('ath-row-device', 'Device line', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', height: DETAIL, padding: [0, '$--spacing-xs'], cornerRadius: '$--radius-xs', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', enabled: false}, [
        icon('ath-row-device-g', 'server', {size: ICON, fill: '$--muted-foreground'}),
        caption('ath-row-device-t', 'Mac mini', '$--muted-foreground'),
      ]),
    ]),
  ]);

  const states = suffix => {
    const id = name => `ath-${suffix}-${name}`;
    const verbs = ['승인', '답변', '확인', '초안'].map((word, index) => cell(`Verb ${index + 1}`, ref(id(`verb-${index}`), AGENT_TREE_MASTERS.verb, 'Verb', {}, {'ath-verb-t': {content: word}})));
    const icons = PR_STATES.map(([state, glyph, fill, label]) => cell(label, ref(id(`pr-${state}`), AGENT_TREE_MASTERS.prIcon, 'PR icon', {}, {'ath-pr-g': {icon: glyph, fill}})));
    const stale = cell('흐림 (GitHub 못 읽음)', ref(id('pr-stale'), AGENT_TREE_MASTERS.prIcon, 'PR icon', {opacity: DIMMED}, {'ath-pr-g': {fill: '$--destructive'}}));
    const marks = [
      cell('! N raised', ref(id('mark-raised'), AGENT_TREE_MASTERS.mark, 'Descendant mark')),
      cell('● N working', ref(id('mark-working'), AGENT_TREE_MASTERS.mark, 'Descendant mark', {}, {'ath-mark-bang': {enabled: false}, 'ath-mark-dot': {enabled: true}, 'ath-mark-n': {content: '3', fill: '$--agent-working'}})),
    ];
    const buttons = [
      cell('Tree button rest', ref(id('tb-rest'), AGENT_TREE_MASTERS.treeButton, 'Tree button')),
      cell('Tree button open', ref(id('tb-open'), AGENT_TREE_MASTERS.treeButton, 'Tree button', {fill: '$--secondary'}, {'ath-tb-n': {content: '2'}})),
    ];
    const rows = [
      cell('Root, opened', ref(id('row-root'), AGENT_TREE_MASTERS.row, 'Tree row', {}, {'ath-row-chev': {icon: 'chevron-down'}, 'ath-row-ring': {enabled: false}, 'ath-row-dot': {enabled: true}, 'ath-row-title': {content: '루트 에이전트', fill: '$--foreground'}, 'ath-row-pr/ath-pr-g': {fill: '$--success'}})),
      cell('Child, folded with a raised descendant', ref(id('row-child'), AGENT_TREE_MASTERS.row, 'Tree row', {}, {'ath-row-elbow': {enabled: true}, 'ath-row-title': {content: '자식'}, 'ath-row-desc': {enabled: true}, 'ath-row-age': {content: '19h'}})),
      cell('Grandchild, no children', ref(id('row-grand'), AGENT_TREE_MASTERS.row, 'Tree row', {}, {'ath-row-rail': {enabled: true}, 'ath-row-elbow': {enabled: true}, 'ath-row-chev': {enabled: false}, 'ath-row-ring': {enabled: false}, 'ath-row-glyph': {enabled: true, content: '✓', fill: '$--success'}, 'ath-row-title': {content: '손자'}, 'ath-row-pr': {enabled: false}, 'ath-row-age': {content: '40m'}})),
      cell('Child on another device', ref(id('row-remote'), AGENT_TREE_MASTERS.row, 'Tree row', {}, {'ath-row-elbow': {enabled: true}, 'ath-row-chev': {enabled: false}, 'ath-row-ring': {stroke: '$--muted-foreground'}, 'ath-row-title': {content: '다른 기기의 자식'}, 'ath-row-pr': {enabled: false}, 'ath-row-device': {enabled: true}, 'ath-row-age': {content: '5h'}})),
    ];
    return [...verbs, ...icons, stale, ...marks, ...buttons, ...rows];
  };
  const themeFrame = (frameId, mode, cells) => frame(frameId, mode, {
    theme: {Mode: mode}, layout: 'horizontal', gap: '$--spacing-lg', alignItems: 'end', padding: '$--spacing-lg',
    fill: '$--background', width: 'fit_content', cornerRadius: '$--radius-md',
  }, cells);

  const id = 'cmp-agent-tree-parts';
  return frame(id, 'Component / Agent tree parts', {
    layout: 'vertical', gap: '$--spacing-xl', padding: '$--spacing-xl', fill: '$--card', cornerRadius: '$--radius-lg', width: 'fit_content',
  }, [
    text(`${id}-title`, 'Agent tree parts', {size: '$--text-headline', weight: '600'}),
    text(`${id}-spec`, 'web/src/components/agent-tree.tsx (PRD agent-hierarchy-screens D-37 to D-40): the verb a Needs You line and an ask band start with, coloured text with no box; the sidebar PR icon in the worst state of the agent\'s own PRs (failed red, pending amber, mergeable green, merged purple) and dimmed while GitHub cannot be read; the one descendant mark a folded parent wears, ! N raised else ● N working; the pane header\'s tree button, a tree icon and the direct child count; and the one-line tree row with its elbow rails, chevron lane, PR icon, mark and age, plus a device line only for a child on another machine. The Sessions and pane header PR chip and its list are the existing PR chip.', {size: '$--text-caption', fill: '$--subtle-foreground', width: 820}),
    frame(`${id}-masters`, 'Masters', {layout: 'horizontal', gap: '$--spacing-xl', alignItems: 'start'}, [verb, prIcon, mark, treeButton, row]),
    themeFrame(`${id}-light`, 'Light', states('l')),
    themeFrame(`${id}-dark`, 'Dark', states('d')),
  ]);
}
