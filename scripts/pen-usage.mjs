// Approved usage-r2 parts on the existing chip and row identities.
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
  const window = (id, label, value, fill = '$--success') => ref(id, 'usage-row', label, {
    'usage-row-label': {content: label}, 'usage-row-value': {content: value, fill},
    'usage-row-reset': {content: label === '5 hours' ? '· in 3h 12m' : '· in 5d 4h'},
    'usage-row-fill': {fill, width: value === '11%' ? 29 : 212},
  });
  const states = s => [
    frame(`usage-states-${s}`, 'Chip states', {layout: 'horizontal', gap: '$--spacing-sm'}, [
      chip(`usage-default-${s}`, '', '81%', '$--warning'), chip(`usage-five-${s}`, '5h', '11%'), chip(`usage-week-${s}`, '7d', '81%', '$--warning'),
      chip(`usage-empty-${s}`, '5h', ''), chip(`usage-loading-${s}`, '', ''),
    ]),
    frame(`usage-popover-${s}`, 'Usage popover', {reusable: true, width: num(tokens, '--size-usage-popover'), layout: 'vertical', gap: '$--spacing-md', padding: '$--spacing-lg', fill: '$--popover', stroke: '$--border', strokeWidth: 1, cornerRadius: '$--radius-md'}, [
      frame(`usage-header-${s}`, 'Header', {width: 'fill_container', alignItems: 'center'}, [
        text(`usage-title-${s}`, 'Usage', {size: '$--text-subhead', weight: '600'}),
        frame(`usage-spacer-${s}`, 'Spacer', {width: 'fill_container', height: 1}, []),
        ref(`usage-choose-${s}`, 'btn-m', 'Choose chips', {'btn-lb': {content: 'Choose chips', fill: '$--subtle-foreground'}, 'btn-ic': {icon: 'settings', enabled: true, fill: '$--subtle-foreground'}}, {height: 24, fill: []}),
      ]),
      ...['Claude Code', 'Codex'].map((label, i) => frame(`usage-card-${i}-${s}`, label, {reusable: true, width: 'fill_container', layout: 'vertical', gap: '$--spacing-md', padding: '$--spacing-md', stroke: '$--border', strokeWidth: 1, cornerRadius: '$--radius-md'}, [
        frame(`usage-agent-header-${i}-${s}`, label, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center'}, [
          {id: `usage-agent-mark-${i}-${s}`, type: 'rectangle', width: 16, height: 16, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${i === 0 ? 'claude' : 'codex'}.png`, mode: 'contain'}},
          text(`usage-agent-${i}-${s}`, label, {size: '$--text-subhead', weight: '600'}),
        ]),
        ...(i === 0 ? [window(`usage-session-${s}`, '5 hours', '11%')] : []),
        window(`usage-weekly-${i}-${s}`, '7 days', '81%', '$--warning'),
        ...(i === 0 ? [ref(`usage-scoped-${s}`, 'usage-row', 'Fable', {'usage-row-branch': {enabled: true}, 'usage-row-label': {content: 'Fable'}, 'usage-row-value': {content: '42%'}, 'usage-row-fill': {width: 110}})] : []),
      ])),
      ref(`usage-popover-history-${s}`, `usage-history-popover-rest-${s}`, "Usage history"),
    ]),
  ];
  return frame('cmp-usage-windows', 'Component / Usage windows', {layout: 'vertical', gap: '$--spacing-xl', padding: '$--spacing-xl', fill: '$--card'}, ['Light', 'Dark'].map(mode => frame(`usage-${mode.toLowerCase()}`, mode, {theme: {Mode: mode}, layout: 'vertical', gap: '$--spacing-lg', padding: '$--spacing-lg', fill: '$--background'}, states(mode.toLowerCase()))));
}
