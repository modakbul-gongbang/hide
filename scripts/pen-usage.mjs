// Compact usage popover; account windows retain the existing standalone master.
import {frame, text, num, findNode} from './pen-system.mjs';
export function updateUsageMasters(document) {
  const chip = findNode(document, 'usage-chip');
  if (!chip) throw new Error('Usage chip master is missing');
  if (!chip.children.some(node => node.id === 'usage-chip-window')) chip.children.splice(1, 0,
    {...text('usage-chip-window', '5h', {size: '$--text-micro', mono: true, weight: '600', fill: '$--muted-foreground'}), enabled: false});
  Object.assign(findNode(document, 'usage-row'), {width: 262});
  Object.assign(findNode(document, 'usage-row-mark'), {enabled: false});
  Object.assign(findNode(document, 'usage-row-label'), {content: '7 days', name: 'Window', fontSize: '$--text-caption'});
}
const ref = (id, target, name, descendants = {}, props = {}) => ({id, type: 'ref', ref: target, name, ...props, descendants});
export function buildUsage(tokens) {
  const chip = (id, window, value, fill = '$--success') => ref(id, 'usage-chip', `${window} ${value}`, {
    'usage-chip-window': {enabled: !!window, content: window},
    'usage-chip-percent': {enabled: !!value, content: value, fill},
    'usage-chip-mark': {opacity: value ? 1 : num(tokens, '--opacity-dimmed')},
  });
  const window = (id, label, value, fill = '$--success') => frame(id, label, {width: 'fill_container', layout: 'horizontal', alignItems: 'center', gap: '$--spacing-xs'}, [
    text(`${id}-label`, label, {size: '$--text-caption'}),
    text(`${id}-reset`, label === '5 hours' ? '· in 3h 12m' : '· in 5d 4h', {size: '$--text-micro', fill: '$--muted-foreground'}),
    frame(`${id}-space`, 'Spacer', {width: 'fill_container', height: 1}, []),
    frame(`${id}-bar`, 'Limit', {width: num(tokens, '--size-control-lg'), height: num(tokens, '--spacing-xs'), fill: '$--border', cornerRadius: '$--radius-full'}, [
      frame(`${id}-fill`, 'Used', {width: num(tokens, '--size-control-lg') * parseInt(value) / 100, height: num(tokens, '--spacing-xs'), fill, cornerRadius: '$--radius-full'}, []),
    ]),
    text(`${id}-value`, value, {size: '$--text-micro', mono: true, weight: '600', fill}),
  ]);
  const states = s => [
    frame(`usage-states-${s}`, 'Chip states', {layout: 'horizontal', gap: '$--spacing-sm'}, [
      chip(`usage-default-${s}`, '', '81%', '$--warning'), chip(`usage-five-${s}`, '5h', '11%'), chip(`usage-week-${s}`, '7d', '81%', '$--warning'),
      chip(`usage-empty-${s}`, '5h', ''), chip(`usage-loading-${s}`, '', ''),
    ]),
    frame(`usage-popover-${s}`, 'Usage popover', {reusable: true, width: num(tokens, '--size-usage-popover'), layout: 'vertical', gap: '$--spacing-sm', padding: '$--spacing-lg', fill: '$--popover', stroke: '$--border', strokeWidth: 1, cornerRadius: '$--radius-md'}, [
      frame(`usage-header-${s}`, 'Header', {width: 'fill_container', alignItems: 'center'}, [
        text(`usage-title-${s}`, 'Usage', {size: '$--text-subhead', weight: '600'}),
        frame(`usage-spacer-${s}`, 'Spacer', {width: 'fill_container', height: 1}, []),
        ref(`usage-choose-${s}`, 'btn-m', 'Choose chips', {'btn-lb': {enabled: false}, 'btn-ic': {icon: 'settings', enabled: true, fill: '$--subtle-foreground'}}, {height: 24, fill: []}),
      ]),
      ref(`usage-popover-history-${s}`, `usage-history-popover-rest-${s}`, 'Usage history'),
      ...['Claude Code', 'Codex'].map((label, i) => frame(`usage-card-${i}-${s}`, label, {reusable: true, width: 'fill_container', layout: 'vertical', gap: '$--spacing-xs'}, [
        frame(`usage-agent-header-${i}-${s}`, label, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
          {id: `usage-agent-mark-${i}-${s}`, type: 'rectangle', width: 16, height: 16, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${i === 0 ? 'claude' : 'codex'}.png`, mode: 'contain'}},
          text(`usage-agent-${i}-${s}`, label, {size: '$--text-caption', weight: '600'}),
        ]),
        ...(i === 0 ? [window(`usage-session-${s}`, '5 hours', '11%')] : []),
        window(`usage-weekly-${i}-${s}`, '7 days', '81%', '$--warning'),
        ...(i === 0 ? [window(`usage-scoped-${s}`, '└ Fable', '42%')] : []),
      ])),
      ref(`usage-history-link-${s}`, 'btn-m', 'Full history', {'btn-lb': {content: '전체 기록'}, 'btn-ic': {icon: 'arrow-up-right', enabled: true}}, {width: 'fill_container', height: 24, fill: []}),
    ]),
  ];
  return frame('cmp-usage-windows', 'Component / Usage windows', {layout: 'vertical', gap: '$--spacing-xl', padding: '$--spacing-xl', fill: '$--card'}, ['Light', 'Dark'].map(mode => frame(`usage-${mode.toLowerCase()}`, mode, {theme: {Mode: mode}, layout: 'vertical', gap: '$--spacing-lg', padding: '$--spacing-lg', fill: '$--background'}, states(mode.toLowerCase()))));
}
