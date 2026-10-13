// Approved heatmap A/C: one primary hue, start-anchored weeks and cost on hover.
import {frame, text, num} from './pen-system.mjs';
const ref = (id, target, name, props = {}) => ({id, type: 'ref', ref: target, name, ...props});
const caption = (id, value, props = {}) => text(id, value, {size: '$--text-caption', fill: '$--muted-foreground', ...props});
const row = (id, children, props = {}) => frame(id, 'Row', {layout: 'horizontal', width: 'fill_container', gap: '$--spacing-sm', alignItems: 'center', ...props}, children);
const AGENTS = ['Claude Code', 'Codex', 'OpenCode', 'Pi', 'omp', 'Grok'];
const MARKS = ['claude', 'codex', 'opencode', 'pi', 'omp', 'grok'];
function cell(id, level, size, tokens, pending = false) {
  return frame(id, pending ? 'Not read yet' : `Level ${level}`, {layout: 'none', width: size, height: size, cornerRadius: '$--radius-xs', fill: '$--secondary', ...(pending ? {stroke: '$--border', strokeWidth: 1, fill: []} : {})}, level && !pending ? [frame(`${id}-fill`, 'Intensity', {width: size, height: size, cornerRadius: '$--radius-xs', fill: '$--primary', opacity: num(tokens, `--opacity-usage-level-${level}`)}, [])] : []);
}
export function usageHistory(tokens, id, surface, state = 'rest') {
  const year = surface === 'year', size = num(tokens, year ? '--size-usage-year-cell' : '--size-usage-cell'), gap = num(tokens, '--size-usage-cell-gap');
  const pending = state === 'backfill', detail = state === 'day' || state === 'unpriced';
  const days = state === 'sparse' ? 9 : 37, weeks = Math.ceil(days / 7);
  const grid = row(`${id}-grid`, [
    frame(`${id}-weekdays`, 'Weekdays', {width: 22, layout: 'vertical', gap}, ['', '', 'Mon', '', 'Wed', '', 'Fri'].map((value, i) => frame(`${id}-label-${i}`, value || 'Space', {width: 22, height: i === 0 ? 18 : size}, value ? [text(`${id}-label-text-${i}`, value, {size: '$--text-micro', fill: '$--muted-foreground'})] : []))),
    ...Array.from({length: weeks}, (_, week) => frame(`${id}-week-${week}`, 'Week', {layout: 'vertical', gap}, [frame(`${id}-month-${week}`, 'Month', {width: size, height: 18}, [0, 4].includes(week) ? [text(`${id}-month-text-${week}`, week ? 'Oct' : 'Sep', {size: '$--text-micro', fill: '$--muted-foreground'})] : []), ...Array.from({length: Math.min(7, days - week * 7)}, (_, day) => cell(`${id}-day-${week}-${day}`, (week + day) % 5, size, tokens, pending && week < 2))])),
  ], {gap});
  const shares = frame(`${id}-shares`, 'Agent filters', {layout: year ? 'vertical' : 'horizontal', width: 'fill_container', gap: '$--spacing-xs'}, AGENTS.map((agent, i) => row(`${id}-agent-${i}`, [
    {id: `${id}-mark-${i}`, type: 'rectangle', width: 12, height: 12, fill: {type: 'image', url: i < 2 ? `../web/src/assets/agent-${MARKS[i]}.png` : `../web/src/assets/agents/${MARKS[i]}.svg`, mode: 'fit'}},
    ...(year ? [caption(`${id}-name-${i}`, agent, {width: 90})] : []),
    ...(year ? [frame(`${id}-bar-${i}`, 'Share', {width: 'fill_container', height: 4, fill: '$--secondary'}, [frame(`${id}-share-${i}`, 'Contribution', {width: [320, 106, 53, 27, 16, 11][i], height: 4, fill: '$--primary'}, [])])] : []),
    caption(`${id}-value-${i}`, ['46.8M', '15.6M', '7.8M', '3.9M', '2.3M', '1.6M'][i], {mono: true, size: '$--text-micro'}),
  ], year ? {} : {width: 'fit_content', gap: 2})));
  return frame(id, year ? 'Usage year' : 'Usage history', {width: year ? num(tokens, '--size-usage-year') : 286, clip: true, layout: 'vertical', gap: '$--spacing-sm'}, [
    row(`${id}-title`, [text(`${id}-heading`, 'New tokens', {size: '$--text-subhead', weight: '600'}), caption(`${id}-total`, pending ? '78M · so far' : '78M · 30 / 37 days active')]),
    grid,
    row(`${id}-legend`, [year ? caption(`${id}-streak`, pending ? 'So far' : '3-day streak') : ref(`${id}-history-link`, 'btn-m', 'Usage history', {fill: [], height: 24, descendants: {'btn-lb': {content: 'Usage history'}, 'btn-ic': {enabled: true, icon: 'arrow-up-right'}}}), frame(`${id}-space`, 'Space', {width: 'fill_container', height: 1}, []), text(`${id}-less`, 'Less', {size: '$--text-micro', fill: '$--muted-foreground'}), ...Array.from({length: 5}, (_, level) => cell(`${id}-legend-${level}`, level, size, tokens)), text(`${id}-more`, 'More', {size: '$--text-micro', fill: '$--muted-foreground'})], {gap: 3}),
    ...(pending ? [caption(`${id}-progress`, 'Reading history · 412 / 1,280 files')] : []),
    frame(`${id}-detail`, 'Reserved detail', {width: 'fill_container', layout: 'vertical', gap: '$--spacing-xs', ...(year ? {padding: '$--spacing-md', stroke: '$--border', strokeWidth: 1, cornerRadius: '$--radius-md'} : {height: 106})}, detail ? [
      caption(`${id}-date`, 'Sun, Oct 11 · 4M', {fill: '$--foreground'}), caption(`${id}-split`, 'In 1.2M · Out 800K · Cache write 2M'), caption(`${id}-read`, 'Cache read 48M · not counted'), caption(`${id}-cost`, state === 'unpriced' ? '≥ $8.00 API rate · model-with-no-public-price unpriced' : '≈ $8.00 API rate'), caption(`${id}-top`, 'Claude 2.4M · Codex 800K · OpenCode 400K · +3'),
    ] : [caption(`${id}-peak`, 'Peak · Sun, Oct 11 · 4M'), shares]),
  ]);
}
export function buildUsageHistory(tokens) {
  return frame('cmp-usage-history', 'Component / Usage history', {layout: 'vertical', padding: '$--spacing-xl', gap: '$--spacing-xl', fill: '$--card'}, ['Light', 'Dark'].map(mode => frame(`usage-history-${mode.toLowerCase()}`, mode, {theme: {Mode: mode}, layout: 'horizontal', padding: '$--spacing-lg', gap: '$--spacing-lg', fill: '$--background'}, ['rest', 'day', 'unpriced', 'backfill', 'sparse'].map(state => frame(`usage-history-${mode}-${state}`, state, {layout: 'vertical', gap: '$--spacing-sm'}, [caption(`usage-history-${mode}-${state}-title`, state), {...usageHistory(tokens, `usage-history-popover-${state}-${mode.toLowerCase()}`, 'popover', state), reusable: true}, ...(state === 'rest' ? [{...usageHistory(tokens, `usage-history-year-${mode.toLowerCase()}`, 'year'), reusable: true}] : [])])))));
}
