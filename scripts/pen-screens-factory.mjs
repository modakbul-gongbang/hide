// The children of `Screen / Factory` (PRD software-factory-ui D-03, D-18, B1-B24):
// 내 차례, 보드, 그래프, the Task page, the create sheet and the empty states, each
// inside the window with the sidebar's Factory row and the 비서 row beneath it.
// Drawn on this document's local tokens plus library refs (Button, Select, Kbd,
// Radio, Checkbox), the way every other Screen sheet is, and called from
// pen-screens.mjs, which owns the ref helpers and the sheet frame. Every name,
// title and number is invented mock content derived from one example data set
// below, so the flow counts, the tab number and the sidebar badge always agree.

import {frame, icon, num, text} from './pen-system.mjs';
import {fitText, textWidth} from './pen-screens-disk.mjs';

const FG = '$--foreground';
const SUB = '$--subtle-foreground';
const MUT = '$--muted-foreground';
const WARN = '$--warning';
const WORK = '$--agent-working';
const OK = '$--success';

// -- the example data -----------------------------------------------------------------
// The words are the shipped Korean catalog's (web/src/i18n/resources/factory.ts).

const STATE_WORD = {
  drafting: '정리 중', waiting: '대기', running: '실행 중', blocked: '막힘', verifying: '검증 중', merge_waiting: '머지 대기',
  stopped: '멈춤', outside: '밖에서 진행 중', done: '완료', landed: '머지됨',
};
const STATE_GLYPH = {
  drafting: 'circle', waiting: 'circle', running: 'circle-dot', verifying: 'circle-dot', outside: 'circle-dot', blocked: 'circle-help',
  stopped: 'circle-pause', merge_waiting: 'git-merge', done: 'circle-check', landed: 'circle-check',
};
const TONE = {
  drafting: MUT, waiting: MUT, running: WORK, verifying: WORK, outside: WORK, blocked: WARN, stopped: WARN, merge_waiting: WARN, done: OK, landed: OK,
};

// Task cards in the engine's board order: the person's cards first, then by priority and age.
const TASKS = {
  t7: {id: 'T-7', project: 'herdr-ide', title: '알림 설정 화면 정리', state: 'drafting', column: 'drafting'},
  t431: {id: '#431', project: 'herdr-ide', title: '문서 깨진 링크 정리', state: 'waiting', column: 'waiting'},
  t421: {id: '#421', project: 'herdr-ide', title: 'Task 상세 화면', state: 'waiting', column: 'waiting', lock: '#420'},
  t422: {id: '#422', project: 'herdr-ide', title: '보드에서 Task 상세 패널 열기', state: 'waiting', column: 'waiting', lock: '#421'},
  t420: {id: '#420', project: 'herdr-ide', title: 'Task 상세 API 응답 형식', state: 'blocked', column: 'running', turn: true, since: '3일'},
  t405: {id: '#405', project: 'herdr-ide', title: 'hide-ai 호출 상한 조정', state: 'merge_waiting', column: 'running', turn: true},
  t417: {id: '#417', project: 'herdr-ide', title: '디스크 정리 표 다시 그리기', state: 'stopped', column: 'running', turn: true},
  t398: {id: '#398', project: 'herdr-ide', title: 'Sessions 검색 속도 개선', state: 'verifying', column: 'running'},
  t430: {id: '#430', project: 'herdr-ide', title: 'flaky: pane-focus 테스트', state: 'outside', column: 'running', external: true},
  t412: {id: '#412', project: 'herdr-ide', title: 'Issues 보드에 정렬 추가', state: 'running', column: 'running'},
  t415: {id: '#415', project: 'herdr-ide', title: '보드 정렬 상태 기억', state: 'running', column: 'running'},
  t409: {id: '#409', project: 'herdr-ide', title: 'Task 목록 정렬 키 문서화', state: 'landed', column: 'done', age: '5시간', today: true},
  t410: {id: '#410', project: 'herdr-ide', title: '정렬 API: tasks.rs에 updated_at', state: 'done', column: 'done', age: '2시간', today: true, unread: true},
  s91: {id: '#91', project: 'sasu', title: 'implement 단계 로그 정리', state: 'waiting', column: 'waiting'},
  s88: {id: '#88', project: 'sasu', title: 'gate 결과 요약 보기', state: 'running', column: 'running'},
  s86: {id: '#86', project: 'sasu', title: 'verify 리포트 한 줄 요약', state: 'done', column: 'done', age: '50분', today: true},
};
const COLUMNS = [['drafting', '정리 중'], ['waiting', '대기'], ['running', '실행 중'], ['done', '완료']];
// The board's order inside a column, per Factory (engine order, copied not recomputed).
const BOARD = {
  drafting: [['herdr-ide', ['t7']]],
  waiting: [['herdr-ide', ['t431', 't421', 't422']], ['sasu', ['s91']]],
  running: [['herdr-ide', ['t420', 't405', 't417', 't398', 't430', 't412', 't415']], ['sasu', ['s88']]],
  done: [['herdr-ide', ['t409', 't410']], ['sasu', ['s86']]],
};
const FOLDED_DONE = 2;
const flowCount = column => BOARD[column].flatMap(([, keys]) => keys).length;
const doneToday = Object.values(TASKS).filter(task => task.today).length;

const INBOX = [
  {group: '답할 것', items: [
    {kind: 'blocking', glyph: 'message-square', key: 't420', title: 'Task 상세를 새 REST 엔드포인트로 낼까요, 기존 WS snapshot에 합칠까요?', why: '답 없이는 진행 불가', text: 'WS에 합치면 Task마다 snapshot이 약 2 KB 커지고, REST는 hided에 route가 하나 생깁니다.', cue: '3일째 기다림', cueFill: WARN,
      choices: ['WS snapshot에 합치기', 'REST 엔드포인트'], result: 'worker를 깨워 이어갑니다 · 끝나면 #421이 풀립니다', footer: ['ban', '기본 행동 없음 · 답할 때까지 기다립니다']},
    {kind: 'default', glyph: 'message-square', key: 't412', title: '보드 정렬 기본값은 무엇으로?', text: '처음 열 때 어떤 정렬로 보일지 정해야 합니다.', cue: '21시간 남음'},
    {kind: 'default', glyph: 'message-square', key: 's88', title: '요약은 gate마다 한 줄? 실패만?', text: 'gate가 열두 개라 모두 적으면 긴 줄이 됩니다.', cue: '5시간 남음'},
  ]},
  {group: '머지 대기', items: [{kind: 'merge', glyph: 'git-merge', key: 't405', title: 'PR #561 머지', text: '검증을 통과했고 manual 머지 대기입니다.', cue: '1시간'}]},
  {group: '멈춤', items: [{kind: 'stopped', glyph: 'circle-pause', key: 't417', title: '검증 3회 실패로 멈춤', text: 'web-e2e가 세 번 연속 실패했습니다.', cue: '40분'}]},
  {group: '알림', items: [
    {kind: 'notice', glyph: 'bell', key: 't398', title: 'main 깨짐 → revert 됨', text: 'main이 깨져 마지막 머지를 되돌렸습니다.', cue: '25분'},
    {kind: 'notice', glyph: 'bell', key: 't415', title: '#412와 같은 정렬을 다르게 푸는 중', text: '두 Task가 같은 정렬 상태를 서로 다르게 바꾸고 있습니다.', cue: '10분'},
  ]},
];
const INBOX_COUNT = INBOX.reduce((sum, group) => sum + group.items.length, 0);

// The graph: layers by what each Task waits on; 420 -> 422 is implied by 420 -> 421 -> 422 and is not drawn.
const GRAPH_LAYERS = [['t420', 't410'], ['t421', 't412', 't415'], ['t422']];
const GRAPH_EDGES = [['t420', 't421'], ['t421', 't422'], ['t410', 't412'], ['t410', 't415'], ['t412', 't422']];
const GRAPH_UNRELATED = ['t7', 't431', 't405', 't417', 't398', 't430', 't409'];
const GRAPH_SASU = ['s91', 's88', 's86'];

export function factoryRows(tokens, {themedXref, screenButton, screenSelect, screenIconButton, screenDialogSurface, screenRadioItem}, s) {
  const HAIR = num(tokens, '--size-hairline');
  const DISABLED = num(tokens, '--opacity-disabled');
  const W = 1440;
  const H = 900;
  const CHROME = 28;
  const RAIL = num(tokens, '--size-rail');
  const SIDE = num(tokens, '--size-sidebar-ideal');
  const MAIN = W - RAIL - SIDE;
  const GUTTER = num(tokens, '--spacing-xl');
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);
  const rule = (id, width = 'fill_container') => frame(id, 'Rule', {width, height: HAIR, fill: '$--border'}, []);
  const row = (id, children, props = {}) => frame(id, props.name ?? 'Row', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', ...props}, children);
  const col = (id, children, props = {}) => frame(id, props.name ?? 'Column', {layout: 'vertical', gap: '$--spacing-sm', ...props}, children);
  const cap = (id, content, fill = MUT, {mono = false, weight = '400', width} = {}) => text(id, content, {size: '$--text-caption', fill, mono, weight, ...(width ? {width} : {})});
  const body = (id, content, opts = {}) => text(id, content, {size: '$--text-body', ...opts});
  const dot = (id, fill, size) => ({type: 'ellipse', id, name: 'Dot', width: size, height: size, fill});
  const kbd = (id, label) => themedXref(id, 'kbd-m', label, {}, {'kbd-t': {content: label}});
  const glyphBox = (id, glyph, fill, size = num(tokens, '--size-icon')) => frame(id, 'Glyph', {width: size, height: size, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [icon(`${id}-g`, glyph, {size, fill})]);

  // The library Checkbox: on or off.
  const checkbox = (id, on) => themedXref(id, 'chk-m', `Checkbox ${on ? 'on' : 'off'}`, on ? {fill: '$--primary', stroke: '$--primary', strokeWidth: HAIR, strokeAlignment: 'inner'} : {}, {'chk-i': {enabled: on}, 'chk-bar': {enabled: false}});

  // -- window ----------------------------------------------------------------------------
  function chrome(id) {
    return frame(id, 'Window chrome', {width: W, height: CHROME, fill: '$--secondary', stroke: '$--border', strokeWidth: {bottom: HAIR}, strokeAlignment: 'inner', layout: 'horizontal', gap: '$--spacing-sm', padding: [0, '$--spacing-sm'], alignItems: 'center'}, [
      dot(`${id}-a`, MUT, 12), dot(`${id}-b`, MUT, 12), dot(`${id}-c`, MUT, 12), spacer(`${id}-s`), text(`${id}-t`, 'hide', {size: '$--text-body', weight: '600', fill: MUT}), spacer(`${id}-s2`), frame(`${id}-pad`, 'Pad', {width: 52, height: 1}, []),
    ]);
  }
  function rail(id) {
    return frame(id, 'Device rail', {width: RAIL, height: 'fill_container', fill: '$--sidebar', stroke: '$--border', strokeWidth: {right: HAIR}, strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-md', padding: [10, 0], alignItems: 'center'}, [
      frame(`${id}-ring`, 'This Mac', {width: 40, height: 40, cornerRadius: 12, stroke: FG, strokeWidth: 2, strokeAlignment: 'inner', layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [
        frame(`${id}-tile`, 'Tile', {width: 32, height: 32, cornerRadius: '$--radius-md', fill: '$--secondary', layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [icon(`${id}-g`, 'laptop', {size: 18, fill: FG})]),
      ]),
      frame(`${id}-add`, 'Add device', {width: 32, height: 32, cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [icon(`${id}-ag`, 'plus', {size: 14, fill: MUT})]),
    ]);
  }
  function placeRow(id, glyph, label, right, selected) {
    return frame(id, label, {width: 'fill_container', height: 36, layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-sm', ...(selected ? {fill: '$--secondary'} : {})}, [
      icon(`${id}-i`, glyph, {size: 14, fill: selected ? FG : SUB}), text(`${id}-t`, label, {size: '$--text-subhead', weight: '600'}), spacer(`${id}-s`), ...right,
    ]);
  }
  function checkoutRow(id, glyph, label, status, meta) {
    const mark = status === 'work' ? dot(`${id}-m`, WORK, 7) : status === 'seen' ? {type: 'ellipse', id: `${id}-m`, name: 'Ring', width: 7, height: 7, stroke: MUT, strokeWidth: HAIR, strokeAlignment: 'inner'} : null;
    return frame(id, label, {width: 'fill_container', height: 26, layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-sm', 0, 26]}, [
      frame(`${id}-mk`, 'Status mark', {width: 12, height: 12, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, mark ? [mark] : []),
      icon(`${id}-i`, glyph, {size: 12, fill: MUT}), text(`${id}-t`, label, {size: '$--text-body', mono: true, fill: SUB}), spacer(`${id}-s`), cap(`${id}-x`, meta),
    ]);
  }
  // The sidebar with the Factory row under Overview (count, ⇧⌘F) and, once a Factory exists, the 비서 row beneath it.
  function sidebar(id, {count, secretary, selected = true}) {
    return frame(id, 'Sidebar', {width: SIDE, height: 'fill_container', fill: '$--sidebar', stroke: '$--border', strokeWidth: {right: HAIR}, strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-xs', padding: '$--spacing-md', clip: true}, [
      row(`${id}-h`, [text(`${id}-ht`, 'This Mac', {size: '$--text-subhead', weight: '600'}), spacer(`${id}-hs`), screenIconButton(`${id}-hp`, 'plus'), screenIconButton(`${id}-hq`, 'search')], {width: 'fill_container', height: 28, padding: [0, 0, 0, '$--spacing-sm']}),
      placeRow(`${id}-ov`, 'layout-dashboard', 'Overview', [cap(`${id}-ovn`, '2', WARN, {mono: true}), cap(`${id}-ovk`, '⇧⌘O', MUT, {mono: true})], false),
      placeRow(`${id}-fa`, 'factory', 'Factory', [...(count ? [cap(`${id}-fan`, String(count), WARN, {mono: true})] : []), cap(`${id}-fak`, '⇧⌘F', MUT, {mono: true})], selected),
      ...(secretary ? [frame(`${id}-sec`, '비서', {width: 'fill_container', height: 26, layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-sm', 0, 26]}, [
        frame(`${id}-secm`, 'No mark', {width: 12, height: 12}, []),
        icon(`${id}-secp`, 'sparkle', {size: 12, fill: MUT}),
        text(`${id}-sect`, '비서', {size: '$--text-body', fill: SUB}),
      ])] : []),
      frame(`${id}-g1`, 'Gap', {width: 1, height: 6}, []),
      frame(`${id}-tabs`, 'Tabs', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'}, ['Projects', 'Agents'].map((label, i) =>
        frame(`${id}-tab${i}`, label, {layout: 'horizontal', alignItems: 'center', height: 22, padding: [0, '$--spacing-md'], cornerRadius: '$--radius-xs', ...(i === 0 ? {fill: '$--secondary'} : {})}, [text(`${id}-tab${i}-t`, label, {size: '$--text-body', weight: '500', fill: i === 0 ? FG : SUB})]))),
      frame(`${id}-g2`, 'Gap', {width: 1, height: 4}, []),
      placeRow(`${id}-home`, 'house', 'Home', [cap(`${id}-homen`, '3', MUT, {mono: true})], false),
      placeRow(`${id}-p1`, 'folder-git-2', 'herdr-ide', [cap(`${id}-p1n`, '9', MUT, {mono: true}), icon(`${id}-p1c`, 'chevron-down', {size: 12, fill: MUT})], false),
      checkoutRow(`${id}-c0`, 'house', 'main', null, ''),
      checkoutRow(`${id}-c1`, 'git-branch', '412-board-sort', 'work', '2m'),
      checkoutRow(`${id}-c2`, 'git-branch', '415-sort-memory', 'work', '1m'),
      checkoutRow(`${id}-c3`, 'git-branch', '398-session-search', 'work', '4m'),
      checkoutRow(`${id}-c4`, 'git-branch', '430-flaky-pane-focus', 'work', '6m'),
      checkoutRow(`${id}-c5`, 'git-branch', '420-task-detail-api', 'seen', '자는 중'),
      placeRow(`${id}-p2`, 'folder-git-2', 'sasu', [cap(`${id}-p2n`, '2', MUT, {mono: true}), icon(`${id}-p2c`, 'chevron-right', {size: 12, fill: MUT})], false),
      frame(`${id}-fill`, 'Fill', {width: 1, height: 'fill_container'}, []),
      row(`${id}-f`, [icon(`${id}-fl`, 'laptop', {size: 14, fill: MUT}), spacer(`${id}-fs`), screenIconButton(`${id}-fg`, 'settings', {size: 20})], {width: 'fill_container', height: 28, padding: [0, '$--spacing-xs']}),
    ]);
  }
  function windowFrame(id, name, main, {height = H, count = INBOX_COUNT, secretary = true} = {}) {
    return frame(id, name, {width: W, height, fill: '$--background', layout: 'vertical', clip: true, cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      chrome(`${id}-chrome`),
      frame(`${id}-body`, 'Body', {width: W, height: height - CHROME, layout: 'horizontal'}, [rail(`${id}-rail`), sidebar(`${id}-side`, {count, secretary}), main]),
    ]);
  }
  const caption = (id, label) => text(id, label, {size: '$--text-subhead', weight: '600', fill: FG});
  const captioned = (id, label, node) => col(`${id}-wrap`, [caption(`${id}-cap`, label), node], {gap: '$--spacing-sm'});

  // -- the header: title, project filter, create and ask, the flow bar and the tabs ----------
  function flowBar(id, width, {counts, read = '3분 전'}) {
    const cells = [['정리 중', counts.drafting], ['대기', counts.waiting], ['실행 중', counts.running], ['완료 오늘', counts.done]];
    return frame(id, 'Flow bar', {layout: 'horizontal', gap: '$--spacing-xxs', alignItems: 'center', width, padding: '$--spacing-xxs', fill: '$--muted', cornerRadius: '$--radius-md'}, [
      ...cells.map(([label, count], i) => row(`${id}-${i}`, [text(`${id}-${i}-l`, label, {size: '$--text-body', fill: SUB}), text(`${id}-${i}-n`, String(count), {size: '$--text-body', weight: '600'})], {width: 'fill_container', height: 28, padding: [0, '$--spacing-md']})),
      ...(read ? [row(`${id}-read`, [cap(`${id}-rt`, `GitHub 읽음 ${read}`)], {height: 28, padding: [0, '$--spacing-md']})] : []),
    ]);
  }
  function factoryTabs(id, active, count) {
    const tabs = ['내 차례', '보드', '그래프', '설정'];
    return frame(id, 'Tabs', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'}, tabs.map((label, i) =>
      frame(`${id}-${i}`, label, {layout: 'horizontal', alignItems: 'center', gap: '$--spacing-xs', height: 24, padding: [0, '$--spacing-md'], cornerRadius: '$--radius-xs', ...(i === active ? {fill: '$--secondary'} : {})}, [
        text(`${id}-${i}-l`, label, {size: '$--text-body', weight: '500', fill: i === active ? FG : SUB}),
        ...(i === 0 && count ? [cap(`${id}-${i}-n`, String(count), WARN, {mono: true})] : []),
      ])));
  }
  function header(id, {active, counts, count = INBOX_COUNT, read}) {
    const inner = MAIN - 2 * GUTTER;
    return frame(id, 'Header', {layout: 'vertical', gap: '$--spacing-md', width: MAIN, padding: ['$--spacing-lg', GUTTER, '$--spacing-sm', GUTTER]}, [
      row(`${id}-tr`, [
        text(`${id}-t`, 'Factory', {size: '$--text-headline', weight: '600'}), screenSelect(`${id}-scope`, {content: '모든 프로젝트', width: 148}), spacer(`${id}-s`),
        screenButton(`${id}-new`, 'Factory 만들기', {variant: 'ghost', height: num(tokens, '--size-control-sm'), icon: 'plus'}),
        screenButton(`${id}-ask`, '비서에게 묻기', {variant: 'ghost', height: num(tokens, '--size-control-sm'), icon: 'message-square'}),
      ], {width: 'fill_container'}),
      flowBar(`${id}-flow`, inner, {counts, read}),
      factoryTabs(`${id}-tabs`, active, count),
    ]);
  }
  const FLOW = {drafting: flowCount('drafting'), waiting: flowCount('waiting'), running: flowCount('running'), done: doneToday};

  // -- 내 차례 ----------------------------------------------------------------------------------
  const cue = (id, item) => text(id, item.cue, {size: '$--text-caption', fill: item.cueFill ?? MUT, width: 84, align: 'right'});
  const place = (id, key) => [
    text(`${id}-id`, TASKS[key].id, {size: '$--text-caption', mono: true, fill: MUT, width: 48, align: 'right'}),
    text(`${id}-p`, TASKS[key].project, {size: '$--text-caption', fill: MUT, width: 64}),
  ];
  function choiceRow(id, n, label, width, picked, suggested) {
    return frame(id, label, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width, height: 28, padding: [0, '$--spacing-md'], cornerRadius: '$--radius-sm',
      stroke: picked ? OK : '$--border', strokeWidth: picked ? 2 : HAIR, strokeAlignment: 'inner', fill: picked ? '$--accent' : '$--background'}, [
      kbd(`${id}-k`, String(n)), text(`${id}-t`, label, {size: '$--text-body'}), spacer(`${id}-s`),
      ...(suggested ? [cap(`${id}-tag`, '제안', MUT), ...(picked ? [icon(`${id}-c`, 'check', {size: 12, fill: OK})] : [])] : []),
    ]);
  }
  function closedItem(id, item, width) {
    const meta = textWidth(TASKS[item.key].id, 11, true) + 64 + 84 + 3 * 8;
    const free = width - 2 * 12 - 14 - meta - 16 - 8;
    const titleWidth = Math.min(textWidth(item.title, 12), Math.floor(free * 0.6));
    return frame(id, item.title, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width, height: 32, padding: [0, '$--spacing-md'], cornerRadius: '$--radius-sm'}, [
      glyphBox(`${id}-gl`, item.glyph, SUB), body(`${id}-t`, fitText(item.title, titleWidth + 4, 12)),
      cap(`${id}-x`, fitText(item.text, free - titleWidth, 11)), spacer(`${id}-s`), ...place(id, item.key), cue(`${id}-c`, item),
    ]);
  }
  function openItem(id, item, width) {
    const inner = width - 2 * 12;
    const indent = 14 + 8;
    const choices = [...item.choices.map((label, i) => [label, i === 0]), ['직접 답하기', false]];
    const picked = choices.findIndex(([, suggested]) => suggested);
    return frame(id, 'Open item', {layout: 'vertical', gap: '$--spacing-sm', width, padding: [10, 12], cornerRadius: '$--radius-md', stroke: WARN, strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      row(`${id}-h`, [
        frame(`${id}-gw`, 'Glyph', {width: 14, height: 18, layout: 'horizontal', justifyContent: 'center', alignItems: 'start'}, [icon(`${id}-gi`, item.glyph, {size: 14, fill: WARN})]),
        col(`${id}-ht`, [
          text(`${id}-q`, item.title, {size: '$--text-body', weight: '600', width: inner - indent - 48 - 64 - 84 - 7 * 8}),
          text(`${id}-w`, `${item.why} · ${item.text}`, {size: '$--text-body', fill: SUB, width: inner - indent - 48 - 64 - 84 - 7 * 8}),
        ], {gap: '$--spacing-xxs'}),
        spacer(`${id}-hs`), ...place(`${id}-m`, item.key), cue(`${id}-c`, item),
      ], {width: 'fill_container', alignItems: 'start'}),
      col(`${id}-ch`, choices.map(([label, suggested], i) => choiceRow(`${id}-o${i}`, i + 1, label, inner - indent, i === picked, suggested && i === 0)), {gap: '$--spacing-xxs', padding: [0, 0, 0, indent]}),
      row(`${id}-send`, [
        screenButton(`${id}-sb`, `${choices[picked][0]}(으)로 보내기`, {height: num(tokens, '--size-control'), icon: 'corner-down-left'}),
        cap(`${id}-sr`, item.result, SUB),
      ], {padding: [0, 0, 0, indent]}),
      row(`${id}-f`, [
        icon(`${id}-fi`, item.footer[0], {size: 12, fill: MUT}), cap(`${id}-ft`, item.footer[1]), spacer(`${id}-fs`),
        screenButton(`${id}-more`, '자세히', {variant: 'ghost', height: num(tokens, '--size-control-sm'), icon: 'arrow-right'}),
      ], {width: 'fill_container', padding: [0, 0, 0, indent]}),
    ]);
  }
  function turnList(id) {
    const inner = MAIN - 2 * GUTTER;
    const children = [];
    INBOX.forEach((group, gi) => {
      children.push(row(`${id}-g${gi}`, [cap(`${id}-g${gi}-t`, `${group.group} ${group.items.length}`, SUB, {weight: '500'})], {height: 30, padding: [0, '$--spacing-md']}));
      group.items.forEach((item, ii) => children.push(gi === 0 && ii === 0 ? openItem(`${id}-g${gi}-${ii}`, item, inner) : closedItem(`${id}-g${gi}-${ii}`, item, inner)));
    });
    children.push(
      frame(`${id}-push`, 'Push', {width: 1, height: 'fill_container'}, []),
      rule(`${id}-fr`),
      row(`${id}-out`, [cap(`${id}-ot`, 'Factory 밖 에이전트 요청 2'), screenButton(`${id}-ob`, '요청', {variant: 'ghost', height: num(tokens, '--size-control-sm'), icon: 'arrow-right'})], {height: 36, padding: [0, '$--spacing-md']}),
    );
    return frame(id, '내 차례', {layout: 'vertical', gap: '$--spacing-xxs', width: MAIN, height: 'fill_container', padding: [0, GUTTER, 0, GUTTER], clip: true}, children);
  }
  function turnMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [header(`${id}-hd`, {active: 0, counts: FLOW}), turnList(`${id}-list`)]);
  }

  // -- Task cards (board and graph) ------------------------------------------------------------
  function stateMark(id, task) {
    return row(`${id}-st`, [icon(`${id}-sg`, STATE_GLYPH[task.state], {size: 12, fill: task.turn ? WARN : TONE[task.state]}), cap(`${id}-sw`, STATE_WORD[task.state], task.turn ? WARN : TONE[task.state])], {gap: '$--spacing-xxs'});
  }
  function taskCard(id, key, width, {showProject = false, dim = false} = {}) {
    const task = TASKS[key];
    return frame(id, task.title, {layout: 'vertical', gap: '$--spacing-xxs', width, padding: ['$--spacing-sm', '$--spacing-md'], cornerRadius: '$--radius-md', fill: '$--card',
      stroke: task.turn ? WARN : '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', ...(dim ? {opacity: DISABLED} : {})}, [
      row(`${id}-a`, [
        cap(`${id}-id`, task.id, MUT, {mono: true}), ...(showProject ? [cap(`${id}-p`, task.project)] : []), spacer(`${id}-as`),
        ...(task.unread ? [dot(`${id}-u`, WORK, num(tokens, '--size-tab-status-dot'))] : []),
      ], {gap: '$--spacing-xs', width: 'fill_container'}),
      text(`${id}-t`, fitText(task.title, width - 2 * 12, 12), {size: '$--text-body'}),
      row(`${id}-b`, [
        stateMark(id, task),
        ...(task.lock ? [row(`${id}-lk`, [icon(`${id}-lg`, 'lock', {size: 12, fill: MUT}), cap(`${id}-lt`, task.lock)], {gap: '$--spacing-xxs'})] : []),
        ...(task.external ? [cap(`${id}-ex`, '외부 대기')] : []),
        ...(task.age ? [cap(`${id}-ag`, task.age)] : []),
      ], {gap: '$--spacing-xs'}),
    ]);
  }

  // -- 보드 ---------------------------------------------------------------------------------------
  function boardBody(id) {
    const colW = Math.floor((MAIN - 2 * GUTTER - 3 * 12) / 4);
    const many = true;
    return frame(id, '보드', {layout: 'vertical', gap: '$--spacing-sm', width: MAIN, height: 'fill_container', padding: [0, GUTTER, '$--spacing-lg', GUTTER], clip: true}, [
      row(`${id}-bar`, [spacer(`${id}-bs`), screenButton(`${id}-cancelled`, '취소됨', {variant: 'ghost', height: num(tokens, '--size-control-sm')})], {width: 'fill_container'}),
      row(`${id}-cols`, COLUMNS.map(([column, label], ci) => {
        const groups = BOARD[column];
        const total = groups.reduce((sum, [, keys]) => sum + keys.length, 0);
        return col(`${id}-c${ci}`, [
          cap(`${id}-c${ci}-h`, `${label} ${total}`, SUB, {weight: '500'}),
          ...groups.flatMap(([project, keys], gi) => [
            ...(many ? [cap(`${id}-c${ci}-g${gi}`, project)] : []),
            ...keys.map(key => taskCard(`${id}-c${ci}-${key}`, key, colW)),
            ...(column === 'done' && gi === 0 ? [row(`${id}-c${ci}-fold`, [icon(`${id}-c${ci}-fi`, 'chevron-right', {size: 12, fill: MUT}), cap(`${id}-c${ci}-ft`, `3일 지난 완료 ${FOLDED_DONE}개`)], {gap: '$--spacing-xxs'})] : []),
          ]),
        ], {width: colW, gap: '$--spacing-sm'});
      }), {gap: '$--spacing-md', alignItems: 'start', width: 'fill_container'}),
    ]);
  }
  function boardMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [header(`${id}-hd`, {active: 1, counts: FLOW}), boardBody(`${id}-board`)]);
  }

  // -- 그래프 ------------------------------------------------------------------------------------
  const NODE_W = 248;
  const NODE_H = 66;
  function arrow(id, sx, sy, tx, ty) {
    const pad = 4;
    const mid = sx + (tx - sx) / 2;
    const points = sy === ty ? [[sx, sy], [tx - 6, ty]] : [[sx, sy], [mid, sy], [mid, ty], [tx - 6, ty]];
    const left = Math.min(...points.map(p => p[0])) - pad;
    const top = Math.min(...points.map(p => p[1])) - pad;
    const width = Math.max(...points.map(p => p[0])) + pad - left;
    const height = Math.max(...points.map(p => p[1])) + pad - top;
    const d = points.map(([x, y], i) => `${i ? 'L' : 'M'} ${x - left} ${y - top}`).join(' ');
    return [
      {type: 'path', id: `${id}-l`, name: 'Edge', x: left, y: top, width, height, viewBox: [0, 0, width, height], geometry: d, stroke: MUT, strokeWidth: 1.5, strokeLinecap: 'round', strokeLinejoin: 'round'},
      {type: 'path', id: `${id}-h`, name: 'Head', x: tx - 7, y: ty - 4, width: 7, height: 8, viewBox: [0, 0, 7, 8], geometry: 'M 0 0 L 7 4 L 0 8 Z', fill: MUT},
    ];
  }
  function graphBody(id) {
    const x0 = GUTTER;
    const colGap = Math.floor((MAIN - 2 * x0 - 3 * NODE_W) / 2);
    const rowGap = 14;
    const y0 = 52;
    const position = {};
    GRAPH_LAYERS.forEach((layer, li) => layer.forEach((key, ri) => { position[key] = [x0 + li * (NODE_W + colGap), y0 + ri * (NODE_H + rowGap)]; }));
    const edges = GRAPH_EDGES.flatMap(([from, to], i) => arrow(`${id}-e${i}`, position[from][0] + NODE_W, position[from][1] + NODE_H / 2, position[to][0], position[to][1] + NODE_H / 2));
    const nodes = Object.entries(position).map(([key, [x, y]]) => ({...taskCard(`${id}-n-${key}`, key, NODE_W, {showProject: true, dim: TASKS[key].state === 'done' || TASKS[key].state === 'landed'}), x, y}));
    const layered = y0 + 3 * (NODE_H + rowGap) - rowGap;
    const perRow = 4;
    const gap = Math.floor((MAIN - 2 * x0 - perRow * NODE_W) / (perRow - 1));
    const unrelated = (idp, keys, y) => keys.map((key, i) => ({...taskCard(`${idp}-${key}`, key, NODE_W, {showProject: true, dim: TASKS[key].state === 'done' || TASKS[key].state === 'landed'}), x: x0 + (i % perRow) * (NODE_W + gap), y: y + Math.floor(i / perRow) * (NODE_H + rowGap)}));
    const unrelatedY = layered + 44;
    const sasuY = unrelatedY + 2 * (NODE_H + rowGap) + 18;
    return frame(id, '그래프', {layout: 'none', width: MAIN, height: 'fill_container', clip: true}, [
      {...cap(`${id}-legend`, '노드를 누르면 그 Task 페이지가 열립니다 · 왼쪽 Task가 먼저 끝나야 합니다'), x: x0, y: 4},
      {...text(`${id}-herdr`, 'herdr-ide', {size: '$--text-subhead', weight: '600'}), x: x0, y: 26},
      ...edges, ...nodes,
      {...rule(`${id}-rule`, MAIN - 2 * x0), x: x0, y: layered + 16},
      {...cap(`${id}-ut`, '관계 없는 Task', SUB, {weight: '500'}), x: x0, y: layered + 24},
      ...unrelated(`${id}-u`, GRAPH_UNRELATED, unrelatedY),
      {...text(`${id}-sasu`, 'sasu', {size: '$--text-subhead', weight: '600'}), x: x0, y: sasuY - 22},
      ...unrelated(`${id}-s`, GRAPH_SASU, sasuY),
    ]);
  }
  function graphMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [header(`${id}-hd`, {active: 2, counts: FLOW}), graphBody(`${id}-graph`)]);
  }

  // -- Task page ----------------------------------------------------------------------------------
  const TASK_PAGE = {
    goal: 'Issues 보드에서 정렬을 고를 수 있다.',
    done: ['정렬 메뉴에서 최근 수정순 · 번호순 · 만든 순을 고른다', '고른 정렬로 다섯 열의 카드 순서가 바뀐다', 'web e2e가 세 정렬을 모두 확인한다'],
    out: ['List view 정렬', '정렬 상태 저장 (#415)'],
    log: [['②', '정렬 값은 URL이 아니라 ui_state에 둔다', 'worker · 2시간 전'], ['①', '보드 e2e fixture 정렬 고정', 'worker · 1시간 전']],
  };
  function bullet(id, glyph, content, width, fill = FG) {
    const mark = glyph ? icon(`${id}-g`, glyph, {size: 12, fill: MUT}) : frame(`${id}-gw`, 'Bullet', {width: 12, height: 16, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [{type: 'ellipse', id: `${id}-g`, name: 'Ring', width: 8, height: 8, stroke: MUT, strokeWidth: HAIR, strokeAlignment: 'inner'}]);
    return row(`${id}`, [mark, text(`${id}-t`, content, {size: '$--text-body', fill, width: width - 20})], {gap: '$--spacing-sm', alignItems: 'start', width});
  }
  const sectionLabel = (id, label) => text(id, label, {size: '$--text-subhead', weight: '600'});
  function chainCell(id, key, label, width) {
    const task = TASKS[key];
    return col(`${id}`, [
      cap(`${id}-l`, label),
      frame(`${id}-b`, task.title, {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', width, height: 28, padding: [0, '$--spacing-md'], cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
        icon(`${id}-g`, STATE_GLYPH[task.state], {size: 12, fill: TONE[task.state]}), cap(`${id}-st`, STATE_WORD[task.state], TONE[task.state]),
        cap(`${id}-id`, task.id, MUT, {mono: true}), body(`${id}-t`, fitText(task.title, width - 150, 12)),
      ]),
    ], {gap: '$--spacing-xs'});
  }
  function taskPage(id) {
    const inner = MAIN - 2 * GUTTER;
    const colW = Math.floor((inner - 48) / 2);
    const chainW = Math.floor((inner - 2 * 40 - 120) / 2);
    const left = col(`${id}-left`, [
      col(`${id}-goal`, [sectionLabel(`${id}-gl`, '목표'), text(`${id}-gt`, TASK_PAGE.goal, {size: '$--text-body', width: colW})]),
      col(`${id}-crit`, [sectionLabel(`${id}-cl`, '완료 조건'), ...TASK_PAGE.done.map((line, i) => bullet(`${id}-d${i}`, null, line, colW))]),
      col(`${id}-out`, [sectionLabel(`${id}-ol`, '범위 밖'), ...TASK_PAGE.out.map((line, i) => bullet(`${id}-o${i}`, 'ban', line, colW, SUB))]),
    ], {gap: '$--spacing-xl', width: colW});
    const right = col(`${id}-right`, [
      col(`${id}-prog`, [
        sectionLabel(`${id}-pl`, '진행'),
        row(`${id}-pr`, [icon(`${id}-pr-g`, 'git-pull-request', {size: 14, fill: MUT}), text(`${id}-pr-t`, 'PR #563', {size: '$--text-body', fill: OK})]),
        row(`${id}-ve`, [icon(`${id}-ve-g`, 'list-checks', {size: 14, fill: WARN}), text(`${id}-ve-t`, '검증 1/3', {size: '$--text-body', fill: WARN})]),
        row(`${id}-wk`, [
          icon(`${id}-wk-g`, 'square-terminal', {size: 14, fill: MUT}), body(`${id}-wk-t`, 't-412-worker'), cap(`${id}-wk-c`, 't-412', MUT, {mono: true}), spacer(`${id}-wk-s`),
          screenButton(`${id}-wk-b`, 'worker 보기', {variant: 'outline', height: num(tokens, '--size-control-sm')}),
        ], {width: colW}),
        row(`${id}-at`, [cap(`${id}-at1`, '시도 1'), cap(`${id}-at2`, 'Task'), cap(`${id}-at3`, '실패', WARN), cap(`${id}-at4`, 'web-e2e', FG, {mono: true}), cap(`${id}-at5`, '30분'),
          cap(`${id}-at6`, '로그 보기', OK), cap(`${id}-at7`, 'CI', OK)], {gap: '$--spacing-sm'}),
      ], {gap: '$--spacing-sm'}),
      col(`${id}-log`, [
        sectionLabel(`${id}-ll`, '결정 기록'),
        ...TASK_PAGE.log.map(([, line, who], i) => row(`${id}-l${i}`, [body(`${id}-l${i}-t`, line), spacer(`${id}-l${i}-s`), cap(`${id}-l${i}-w`, who)], {width: colW})),
      ], {gap: '$--spacing-sm'}),
    ], {gap: '$--spacing-xl', width: colW});
    return frame(`${id}-page`, 'Task page', {width: MAIN, height: 'fill_container', layout: 'vertical', gap: '$--spacing-lg', padding: [14, GUTTER, '$--spacing-lg', GUTTER], clip: true}, [
      row(`${id}-nav`, [screenButton(`${id}-back`, '내 차례', {variant: 'ghost', height: num(tokens, '--size-control-sm'), icon: 'arrow-left'}), spacer(`${id}-ns`)], {width: 'fill_container'}),
      col(`${id}-title`, [
        row(`${id}-tr`, [
          text(`${id}-t`, TASKS.t412.title, {size: '$--text-headline', weight: '600'}),
          row(`${id}-chip`, [icon(`${id}-chip-g`, 'circle-dot', {size: 12, fill: WORK}), cap(`${id}-chip-t`, '실행 중', WORK)], {gap: '$--spacing-xs', height: 24, padding: [0, '$--spacing-md'], cornerRadius: 12, fill: '$--muted'}),
          spacer(`${id}-ts`),
          screenButton(`${id}-pause`, '일시정지', {variant: 'secondary', height: num(tokens, '--size-control-sm')}),
          screenButton(`${id}-cancel`, '취소', {variant: 'ghost', height: num(tokens, '--size-control-sm')}),
        ], {gap: '$--spacing-md', width: 'fill_container'}),
        cap(`${id}-meta`, '#412 · herdr-ide · 412-board-sort'),
      ], {gap: '$--spacing-xs', width: 'fill_container'}),
      row(`${id}-ask`, [
        icon(`${id}-ask-g`, 'message-square', {size: 14, fill: WARN}), body(`${id}-ask-t`, '답할 것: 보드를 처음 열 때 정렬 기본값은?'), cap(`${id}-ask-c`, '21시간 남음', MUT), cap(`${id}-ask-d`, '기본 행동: 최근 수정순으로 진행', MUT),
        screenButton(`${id}-ask-b`, '내 차례에서 답하기', {variant: 'ghost', height: num(tokens, '--size-control-sm'), icon: 'arrow-right'}),
      ], {gap: '$--spacing-md'}),
      row(`${id}-chain`, [
        chainCell(`${id}-ch0`, 't410', '선행', chainW),
        icon(`${id}-ca0`, 'arrow-right', {size: 14, fill: MUT}),
        col(`${id}-ch1`, [cap(`${id}-ch1-l`, '이 Task'), row(`${id}-ch1-b`, [icon(`${id}-ch1-g`, 'circle-dot', {size: 12, fill: WORK}), text(`${id}-ch1-t`, '#412', {size: '$--text-body', weight: '600', mono: true})], {gap: '$--spacing-xs', height: 28, padding: [0, '$--spacing-md'], cornerRadius: '$--radius-md', fill: '$--muted'})], {gap: '$--spacing-xs'}),
        icon(`${id}-ca1`, 'arrow-right', {size: 14, fill: MUT}),
        chainCell(`${id}-ch2`, 't422', '기다리는 것', chainW),
      ], {gap: '$--spacing-md', alignItems: 'end', width: 'fill_container'}),
      rule(`${id}-rule`),
      row(`${id}-cols`, [left, right], {gap: 48, alignItems: 'start', width: inner}),
    ]);
  }
  function taskMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [taskPage(`${id}-tp`)]);
  }

  // -- create sheet ---------------------------------------------------------------------------
  const DIALOG_W = 440;
  const step = (id, title, children) => col(id, [text(`${id}-t`, title, {size: '$--text-subhead', weight: '600'}), ...children], {gap: '$--spacing-sm'});
  function radioLine(id, label, selected, {detail, disabled = false} = {}) {
    return col(id, [
      screenRadioItem(`${id}-r`, label, selected),
      ...(detail ? [cap(`${id}-d`, detail, MUT, {mono: true, width: DIALOG_W - 2 * 24 - 24})] : []),
    ], {gap: '$--spacing-xxs', ...(disabled ? {opacity: DISABLED} : {})});
  }
  function githubLines(id) {
    const lines = [['계정', 'octo-example'], ['저장소', 'example/herdr-ide'], ['읽기', 'factory 라벨이 붙은 issue와 PR 상태'], ['쓰기', 'factory 라벨 만들기']];
    return col(id, lines.map(([k, v], i) => row(`${id}-${i}`, [cap(`${id}-${i}-k`, k, MUT, {width: 40}), body(`${id}-${i}-v`, v, {mono: i < 2, fill: FG})], {gap: '$--spacing-md', alignItems: 'start'})), {gap: '$--spacing-xs'});
  }
  function commandLines(id) {
    return col(id, ['scripts/verify-web.sh', 'scripts/verify-cargo.sh'].map((command, i) => row(`${id}-${i}`, [checkbox(`${id}-${i}-c`, true), cap(`${id}-${i}-t`, command, FG, {mono: true})], {gap: '$--spacing-sm'})), {gap: '$--spacing-xs', padding: [0, 0, 0, 24]});
  }
  // state: detecting (the engine is probing), ci (the detected required checks), commands (verify commands, each a checkbox), none (nothing detected, auto unavailable).
  function createDialog(id, state) {
    const body = [];
    body.push(step(`${id}-sp`, '프로젝트', [screenSelect(`${id}-sel`, {content: 'herdr-ide', width: DIALOG_W - 2 * 16})]));
    if (state === 'detecting') {
      body.push(step(`${id}-sv`, '검증', [row(`${id}-dt`, [icon(`${id}-dg`, 'loader-circle', {size: 12, fill: MUT}), cap(`${id}-dtt`, '필수 체크와 verify 명령을 찾는 중')], {gap: '$--spacing-xs'})]));
    } else {
      const none = state === 'none';
      const commands = state === 'commands';
      body.push(step(`${id}-sv`, '검증', none
        ? [radioLine(`${id}-v0`, 'CI 필수 체크', false, {detail: '감지한 필수 체크 없음', disabled: true}), radioLine(`${id}-v1`, 'verify 명령', false, {disabled: true}), radioLine(`${id}-v2`, '검증 없음', true)]
        : [
          radioLine(`${id}-v0`, 'CI 필수 체크', !commands, {detail: 'web-e2e, rust-test'}),
          col(`${id}-v1`, [screenRadioItem(`${id}-v1r`, 'verify 명령', commands), ...(commands ? [commandLines(`${id}-v1c`)] : [])], {gap: '$--spacing-xs'}),
          radioLine(`${id}-v2`, '검증 없음', false),
        ]));
      body.push(step(`${id}-sm`, '머지 모드', none
        ? [col(`${id}-m0`, [screenRadioItem(`${id}-m0r`, 'auto', false), cap(`${id}-m0d`, '검증이 없으면 auto를 쓸 수 없어 manual로 만들어집니다.', MUT, {width: DIALOG_W - 2 * 24 - 24})], {gap: '$--spacing-xxs', opacity: DISABLED}), screenRadioItem(`${id}-m1`, 'manual', true)]
        : [screenRadioItem(`${id}-m0`, 'auto', true), screenRadioItem(`${id}-m1`, 'manual', false)]));
      body.push(step(`${id}-sg`, 'GitHub에서 하는 일', [githubLines(`${id}-gh`)]));
    }
    const create = screenButton(`${id}-create`, state === 'detecting' ? 'Factory 만들기' : 'herdr-ide Factory 만들기');
    return screenDialogSurface(id, {
      width: DIALOG_W, title: 'Factory 만들기', body,
      actions: [screenButton(`${id}-cancel`, '취소', {variant: 'ghost'}), state === 'detecting' ? {...create, opacity: DISABLED} : create],
    });
  }
  function scrim(id, label, children) {
    return col(`${id}-wrap`, [caption(`${id}-cap`, label), frame(id, 'Scrim', {layout: 'horizontal', gap: '$--spacing-xl', alignItems: 'start', padding: '$--spacing-xl', fill: '$--muted', cornerRadius: '$--radius-lg'}, children)], {gap: '$--spacing-sm'});
  }

  // -- empty states ---------------------------------------------------------------------------
  function noFactoryMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical', alignItems: 'center', justifyContent: 'center', gap: '$--spacing-md'}, [
      screenButton(`${id}-create`, 'Factory 만들기', {icon: 'plus'}),
    ]);
  }
  function noTaskMain(id) {
    const empty = {drafting: 0, waiting: 0, running: 0, done: 0};
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [
      header(`${id}-hd`, {active: 0, counts: empty, count: 0, read: null}),
      frame(`${id}-intake`, 'Intake', {width: MAIN, padding: ['$--spacing-md', GUTTER], layout: 'horizontal'}, [
        text(`${id}-it`, '아직 Task가 없습니다. 대화 중인 에이전트에게 넣어 달라고 하거나 GitHub issue에 factory 라벨을 붙이세요.', {size: '$--text-body', fill: MUT, width: MAIN - 2 * GUTTER}),
      ]),
    ]);
  }

  const id = name => `fx-${name}-${s}`;
  const turn = windowFrame(id('turn'), '내 차례', turnMain(id('turn')));
  const board = windowFrame(id('board'), '보드', boardMain(id('board')));
  const graph = windowFrame(id('graph'), '그래프', graphMain(id('graph')));
  const task = windowFrame(id('task'), 'Task 페이지', taskMain(id('task')));
  const none = windowFrame(id('none'), 'Factory 없음', noFactoryMain(id('none')), {height: 360, count: 0, secretary: false});
  const empty = windowFrame(id('empty'), 'Task 없음', noTaskMain(id('empty')), {height: 360, count: 0});

  return [
    col(id('frames'), [
      row(id('r1'), [
        captioned(id('turn'), '내 차례: 맨 위 항목이 펼쳐져 제안이 골라져 있고, ⏎ 한 번으로 보낸다', turn),
        captioned(id('board'), '보드: 사람 차례 카드는 경고 테두리로 열 맨 위에, 3일 지난 완료는 접는다', board),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
      row(id('r2'), [
        captioned(id('graph'), '그래프: 층으로 놓고 중복 화살표(#420 → #422)는 그리지 않는다', graph),
        captioned(id('task'), 'Task 페이지: 사슬, 왼쪽 카드 필드, 오른쪽 진행과 결정 기록', task),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
      row(id('r3'), [
        scrim(id('create'), 'Factory 만들기: 감지 중 · 필수 체크 · verify 명령 후보 · 검증이 없을 때(auto 불가, manual)', [
          createDialog(id('cd0'), 'detecting'), createDialog(id('cd1'), 'ci'), createDialog(id('cd2'), 'commands'), createDialog(id('cd3'), 'none'),
        ]),
      ], {alignItems: 'start'}),
      row(id('r4'), [
        captioned(id('none'), 'Factory가 없을 때: Factory 만들기만 보인다', none),
        captioned(id('empty'), 'Task가 없을 때: 넣는 방법 한 줄만 보인다', empty),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
    ], {gap: '$--spacing-xl'}),
  ];
}
