// The operator chose compact A, a contribution grid and an agent detail table
// in the HTML comparison. These sheets record that choice on the shared tokens.
import {frame, text, num} from './pen-system.mjs';
const caption = (id, value, props = {}) => text(id, value, {size: '$--text-caption', fill: '$--muted-foreground', ...props});
const row = (id, children, props = {}) => frame(id, 'Row', {layout: 'horizontal', width: 'fill_container', gap: '$--spacing-sm', alignItems: 'center', ...props}, children);
function cell(id, level, size, tokens, pending = false) {
  return frame(id, pending ? 'Not read yet' : `Level ${level}`, {layout: 'none', width: size, height: size, cornerRadius: '$--spacing-xxs', fill: '$--secondary', ...(pending ? {stroke: '$--border', strokeWidth: 1, fill: []} : {})}, level && !pending ? [frame(`${id}-fill`, 'Intensity', {width: size, height: size, cornerRadius: '$--spacing-xxs', fill: '$--primary', opacity: num(tokens, `--opacity-usage-level-${level}`)}, [])] : []);
}
const summary = (id, cost, tokens) => row(`${id}-summary`, [['추정 비용', cost], ['새 토큰', tokens]].map(([label, value], i) => frame(`${id}-metric-${i}`, label, {width: 'fill_container', layout: 'vertical', gap: '$--spacing-xs'}, [caption(`${id}-metric-label-${i}`, label, {size: '$--text-micro'}), text(`${id}-metric-value-${i}`, value, {size: '$--text-headline', mono: true, weight: '600'})])));
function table(id, state) {
  const values = [
    ['날짜 / 에이전트', '추정 비용', '새 토큰', '입력', '출력', '캐시 쓰기', '캐시 읽기', '전체 토큰'],
    ['⌄ 10월 11일 (일)', '≈ $8.00', '4M', '1.2M', '800K', '2M', '48M', '52M'],
    ['    Claude Code', '≈ $4.80', '2.4M', '720K', '480K', '1.2M', '28.8M', '31.2M'],
    ['    Codex', '≈ $1.60', '800K', '240K', '160K', '400K', '9.6M', '10.4M'],
    ['› 10월 10일 (토)', '≈ $6.00', '3M', '900K', '600K', '1.5M', '36M', '39M'],
    ['› 10월 9일 (금)', '≈ $4.00', '2M', '600K', '400K', '1M', '24M', '26M'],
    ['기간 합계', '≈ $132.00', '66M', '19.8M', '13.2M', '33M', '792M', '858M'],
  ];
  return frame(`${id}-table`, 'Date and agent usage', {width: 'fill_container', layout: 'vertical'}, values.map((values, r) => row(`${id}-table-row-${r}`, values.map((value, c) => caption(`${id}-table-${r}-${c}`, state === 'unpriced' && c === 1 && r ? value.replace('≈', '≥') : value, {width: c === 0 ? 148 : c === 1 ? 94 : 80, size: r === 0 ? '$--text-micro' : '$--text-caption', mono: r > 0 && c > 0, fill: r === 0 || [2, 3].includes(r) ? '$--muted-foreground' : '$--foreground'})), {gap: 0, padding: [8, 0], stroke: {fill: '$--border', thickness: {bottom: 1}}})));
}
export function usageHistory(tokens, id, surface, state = 'rest') {
  const year = surface === 'year', pending = state === 'backfill', size = num(tokens, '--size-usage-cell'), gap = num(tokens, '--size-usage-cell-gap');
  const cost = state === 'unpriced' ? '가격 미등록' : pending ? '아직 읽지 않음' : '≈ $8.00';
  if (year) return frame(id, 'Usage records', {width: num(tokens, '--size-usage-year'), layout: 'vertical', gap: '$--spacing-md', fill: '$--background'}, [
    row(`${id}-heading`, [text(`${id}-title`, '전체 기록', {size: '$--text-subhead', weight: '600'}), frame(`${id}-spacer`, 'Space', {width: 'fill_container', height: 1}, []), caption(`${id}-controls`, '모든 에이전트 ⌄   최근 30일 ⌄   정확한 숫자')]),
    summary(id, state === 'unpriced' ? '≥ $132.00' : '≈ $132.00', '66M'),
    ...(pending ? [caption(`${id}-reading`, '기록 읽는 중 · 1,280개 중 412개 파일 · 지금까지')] : []),
    table(id, state), caption(`${id}-foot`, 'API 단가 환산액 · 실제 청구액과 다를 수 있음                   이전  1 / 1  다음', {size: '$--text-micro'}),
  ]);
  const grass = row(`${id}-grid`, Array.from({length: 5}, (_, w) => frame(`${id}-week-${w}`, 'Week', {layout: 'vertical', gap}, Array.from({length: w === 4 ? 2 : 7}, (_, d) => cell(`${id}-day-${w}-${d}`, (w + d) % 5, size, tokens, pending && w < 2)))), {width: 'fit_content', gap});
  return frame(id, 'Compact usage', {width: 286, layout: 'vertical', gap: '$--spacing-md'}, [
    caption(`${id}-date`, state === 'day' ? '10월 11일 (일)' : '오늘'), summary(id, cost, pending ? '아직 읽지 않음' : '4M'),
    caption(`${id}-details`, '› 세부 내역', {size: '$--text-micro'}),
    frame(`${id}-grass-section`, 'Last 30 days', {width: 'fill_container', layout: 'vertical', gap: '$--spacing-sm', padding: [12, 0]}, [
      caption(`${id}-period`, '최근 30일                                          새 토큰', {size: '$--text-micro'}),
      row(`${id}-grass-row`, [grass, frame(`${id}-period-total`, 'Period total', {layout: 'vertical', gap: '$--spacing-xs'}, [text(`${id}-cost`, '≈ $132.00', {size: '$--text-subhead', mono: true, weight: '600'}), caption(`${id}-tokens`, '66M', {mono: true}), row(`${id}-legend`, [caption(`${id}-less`, '적음', {size: '$--text-micro'}), ...Array.from({length: 5}, (_, l) => cell(`${id}-legend-${l}`, l, size, tokens)), caption(`${id}-more`, '많음', {size: '$--text-micro'})], {gap: '$--spacing-xxs'})])], {gap: '$--spacing-lg'}),
    ]),
    ...(pending ? [caption(`${id}-reading`, '기록 읽는 중 · 1,280개 중 412개 파일', {size: '$--text-micro'})] : []),
  ]);
}
export function buildUsageHistory(tokens) {
  return frame('cmp-usage-history', 'Component / Usage history', {layout: 'vertical', padding: '$--spacing-xl', gap: '$--spacing-xl', fill: '$--card'}, ['Light', 'Dark'].map(mode => frame(`usage-history-${mode.toLowerCase()}`, mode, {theme: {Mode: mode}, layout: 'horizontal', padding: '$--spacing-lg', gap: '$--spacing-lg', fill: '$--background'}, ['rest', 'day', 'unpriced', 'backfill', 'sparse'].map(state => frame(`usage-history-${mode}-${state}`, state, {layout: 'vertical', gap: '$--spacing-sm'}, [caption(`usage-history-${mode}-${state}-title`, state), {...usageHistory(tokens, `usage-history-popover-${state}-${mode.toLowerCase()}`, 'popover', state), reusable: true}, ...(state === 'rest' ? [{...usageHistory(tokens, `usage-history-year-${mode.toLowerCase()}`, 'year'), reusable: true}] : [])])))));
}
