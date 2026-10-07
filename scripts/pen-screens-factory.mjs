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

const CRIT = '$--destructive';
// A card's own width picks its size (PRD factory-board-cards D-12, B4): small below
// 240, wide from 420, normal between; the board's lanes and the graph's nodes both
// follow it.
const SMALL_BELOW = 240;
const WIDE_FROM = 420;

// `resting` is not an engine state: it is a waiting Task that has run before and
// went back to waiting on a usage limit, which the board shows as 쉬는 중 (D-09).
const STATE_WORD = {
  drafting: '정리 중', waiting: '대기', resting: '쉬는 중', running: '실행 중', blocked: '막힘', verifying: '검증 중', merge_waiting: '머지 대기',
  stopped: '멈춤', outside: '밖에서 진행 중', done: '완료', landed: '머지됨',
};
const STATE_GLYPH = {
  drafting: 'circle-dashed', waiting: 'circle', resting: 'circle-pause', running: 'circle-dot', verifying: 'loader-circle', outside: 'circle-dot', blocked: 'circle-help',
  stopped: 'circle-pause', merge_waiting: 'git-merge', done: 'circle-check', landed: 'circle-check',
};
const TONE = {
  drafting: MUT, waiting: MUT, resting: MUT, running: WORK, verifying: WORK, outside: MUT, blocked: WARN, stopped: CRIT, merge_waiting: WARN, done: OK, landed: OK,
};

// Every Task in one example data set. `lane` is the board lane (D-06, D-07) and,
// in 멈춤, `wait` is whose move it is; `stage` is the current cell of the four-cell
// bar (0 대기, 1 작업, 2 검증, 3 머지, 4 all done, B14); `mark` is the worker pane's
// status mark and word from the shipped agent catalog; `ai` is that pane's label line.
const TASKS = {
  t7: {id: 'T-7', project: 'herdr-ide', title: '알림 설정 화면 정리', state: 'drafting', lane: 'before', stage: 0, age: '5분', summary: '알림 켜고 끄기를 설정 한 화면에 모은다'},
  t431: {id: '#431', project: 'herdr-ide', title: '문서 깨진 링크 정리', state: 'waiting', lane: 'before', stage: 0, age: '1시간', summary: 'docs 안의 깨진 상대 링크 23개를 고친다'},
  t421: {id: '#421', project: 'herdr-ide', title: 'Task 상세 화면', state: 'waiting', lane: 'before', stage: 0, age: '3일', summary: 'Task 상세 응답을 받아 오른쪽 패널에 그린다',
    problem: {glyph: 'lock', text: '#420 기다림', tone: MUT}},
  t422: {id: '#422', project: 'herdr-ide', title: '보드에서 Task 상세 패널 열기', state: 'waiting', lane: 'before', stage: 0, age: '3일', summary: '보드 카드를 누르면 상세 패널이 열린다',
    problem: {glyph: 'lock', text: '#421 기다림', tone: MUT}},
  t412: {id: '#412', project: 'herdr-ide', title: 'Issues 보드에 정렬 추가', state: 'running', lane: 'moving', stage: 1, pr: '563', agent: 'claude', age: '12분',
    summary: '최근 수정순 · 번호순 · 만든 순으로 보드 카드를 정렬한다', problem: {glyph: 'triangle-alert', text: '검증 실패 1/3', tone: WARN},
    mark: ['●', '작업 중', WORK], ai: 'web-e2e 정렬 fixture를 고치고 다시 검증하는 중'},
  t415: {id: '#415', project: 'herdr-ide', title: '보드 정렬 상태 기억', state: 'running', lane: 'moving', stage: 1, agent: 'codex', kids: [2, 1], age: '8분',
    summary: '고른 정렬을 ui_state에 남겨 다시 열어도 그대로 둔다', mark: ['○', '하위 대기', WORK], ai: '저장은 끝, 하위 둘이 복원과 테스트를 쓰는 중'},
  t398: {id: '#398', project: 'herdr-ide', title: 'Sessions 검색 속도 개선', state: 'verifying', lane: 'moving', stage: 2, pr: '559', agent: 'claude', age: '4분',
    summary: '세션 검색을 색인으로 바꿔 1초 안에 답한다', mark: ['○', '대기', MUT], ai: '색인을 붙이고 CI 결과를 기다리는 중'},
  t420: {id: '#420', project: 'herdr-ide', title: 'Task 상세 API 응답 형식', state: 'blocked', lane: 'stuck', wait: 'me', stage: 1, agent: 'codex', age: '3일',
    summary: 'Task 상세를 웹에 보낼 응답 모양을 정한다',
    ask: {question: 'Task 상세를 새 REST 엔드포인트로 낼까요, 기존 WS snapshot에 합칠까요?', choices: ['WS snapshot에 합치기', 'REST 엔드포인트']},
    mark: ['?', '질문', WARN], ai: '두 방식의 snapshot 크기를 재고 답을 기다림'},
  t405: {id: '#405', project: 'herdr-ide', title: 'hide-ai 호출 상한 조정', state: 'merge_waiting', lane: 'stuck', wait: 'me', stage: 3, pr: '561', agent: 'claude', age: '1시간',
    summary: 'hide-ai가 분당 부르는 횟수 상한을 설정으로 뺀다', problem: {glyph: 'lock', text: 'manual 머지', tone: WARN}, action: ['머지', 'PR 보기'],
    mark: ['○', '대기', MUT], ai: '상한 설정과 테스트를 올리고 머지를 기다림'},
  t417: {id: '#417', project: 'herdr-ide', title: '디스크 정리 표 다시 그리기', state: 'stopped', lane: 'stuck', wait: 'me', stage: 2, pr: '560', agent: 'claude', age: '40분',
    summary: '디스크 정리 표를 크기순으로 다시 그린다', problem: {glyph: 'circle-alert', text: '검증 3회 실패', tone: CRIT}, action: ['다시 시작', '기록 보기'],
    mark: ['○', '대기', MUT], ai: 'web-e2e 세 번째 실패 뒤 멈춤'},
  t426: {id: '#426', project: 'herdr-ide', title: '단축키 도움말 시트', state: 'resting', lane: 'stuck', wait: 'other', stage: 1, age: '50분',
    summary: '⌘/로 여는 단축키 목록 시트를 만든다', problem: {glyph: 'circle-pause', text: '쉬는 중 · 14:00 재개', tone: MUT}},
  t430: {id: '#430', project: 'herdr-ide', title: 'flaky: pane-focus 테스트', state: 'outside', lane: 'stuck', wait: 'other', stage: 3, age: '2시간',
    summary: 'pane-focus e2e가 가끔 실패하는 원인을 찾는다', problem: {glyph: 'git-pull-request', text: '#566이 이 이슈를 닫는 중', tone: MUT}},
  t409: {id: '#409', project: 'herdr-ide', title: 'Task 목록 정렬 키 문서화', state: 'done', lane: 'done', stage: 4, pr: '557', merged: true, age: '5시간', today: true,
    summary: '정렬 키 세 개를 docs/factory.md에 적는다'},
  t410: {id: '#410', project: 'herdr-ide', title: '정렬 API: tasks.rs에 updated_at', state: 'done', lane: 'done', stage: 4, pr: '558', merged: true, age: '2시간', today: true,
    summary: 'tasks.rs가 updated_at으로 정렬할 수 있게 한다'},
  s91: {id: '#91', project: 'sasu', title: 'implement 단계 로그 정리', state: 'waiting', lane: 'before', stage: 0, age: '2시간', summary: 'implement 로그를 단계별로 묶는다'},
  s88: {id: '#88', project: 'sasu', title: 'gate 결과 요약 보기', state: 'running', lane: 'moving', stage: 1, agent: 'codex', age: '20분', summary: 'gate 결과를 한 줄씩 요약해 보인다',
    mark: ['●', '작업 중', WORK], ai: 'gate마다 한 줄 요약을 만드는 중'},
  s86: {id: '#86', project: 'sasu', title: 'verify 리포트 한 줄 요약', state: 'done', lane: 'done', stage: 4, pr: '84', merged: true, age: '50분', today: true, summary: 'verify 리포트 맨 위에 한 줄 요약을 둔다'},
};
// The board's lanes by movement (D-06) with their width weights (D-08: 시작 전 and
// 완료 narrow), shown for the herdr-ide filter in the engine's order inside a lane.
const LANES = [['before', '시작 전', 0.8], ['moving', '진행 중', 1.2], ['stuck', '멈춤', 1.4], ['done', '완료', 0.7]];
const BOARD = {
  before: ['t7', 't431', 't421', 't422'],
  moving: ['t412', 't415', 't398'],
  stuck: [['me', '나를 기다림', ['t420', 't405', 't417']], ['other', '다른 걸 기다림', ['t426', 't430']]],
  done: ['t410', 't409'],
};
const FOLDED_DONE = 2;
const laneCount = lane => (lane === 'stuck' ? BOARD.stuck.reduce((sum, [, , keys]) => sum + keys.length, 0) : BOARD[lane].length);
const allCount = lane => Object.values(TASKS).filter(task => task.lane === lane && (lane !== 'done' || task.today)).length;

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
const GRAPH_UNRELATED = ['t7', 't431', 't405', 't417', 't398', 't426', 't430', 't409'];
const GRAPH_SASU = ['s91', 's88', 's86'];

export function factoryRows(tokens, {themedXref, screenButton, screenSelect, screenIconButton, screenDialogSurface, screenRadioItem}, s) {
  const HAIR = num(tokens, '--size-hairline');
  const DISABLED = num(tokens, '--opacity-disabled');
  const DIMMED = num(tokens, '--opacity-dimmed');
  const W = 1440;
  const H = 900;
  const BOARD_H = 1000;
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
    const cells = [['시작 전', counts.before], ['진행 중', counts.moving], ['멈춤', counts.stuck], ['완료 오늘', counts.done]];
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
  function header(id, {active, counts, count = INBOX_COUNT, read, scope = '모든 프로젝트'}) {
    const inner = MAIN - 2 * GUTTER;
    return frame(id, 'Header', {layout: 'vertical', gap: '$--spacing-md', width: MAIN, padding: ['$--spacing-lg', GUTTER, '$--spacing-sm', GUTTER]}, [
      row(`${id}-tr`, [
        text(`${id}-t`, 'Factory', {size: '$--text-headline', weight: '600'}), screenSelect(`${id}-scope`, {content: scope, width: 148}), spacer(`${id}-s`),
        screenButton(`${id}-new`, 'Factory 만들기', {variant: 'ghost', height: num(tokens, '--size-control-sm'), icon: 'plus'}),
        screenButton(`${id}-ask`, '비서에게 묻기', {variant: 'ghost', height: num(tokens, '--size-control-sm'), icon: 'message-square'}),
      ], {width: 'fill_container'}),
      flowBar(`${id}-flow`, inner, {counts, read}),
      factoryTabs(`${id}-tabs`, active, count),
    ]);
  }
  const FLOW = {before: allCount('before'), moving: allCount('moving'), stuck: allCount('stuck'), done: allCount('done')};
  const BOARD_FLOW = {before: laneCount('before'), moving: laneCount('moving'), stuck: laneCount('stuck'), done: laneCount('done')};

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
  // One card at three sizes (D-10, D-12, B4-B8): color only on the state icon, the
  // problem line and the left band of a card waiting on the person; bold only on the
  // title and the question; everything else small and gray.
  const STRIPE = 3;
  const PAD_X = num(tokens, '--spacing-md');
  const BAR_CELL = 12;
  const BAR_GAP = 2;
  const BAR_W = 4 * BAR_CELL + 3 * BAR_GAP;
  const sizeOf = width => (width < SMALL_BELOW ? 'small' : width >= WIDE_FROM ? 'wide' : 'normal');
  const logo = (id, agent) => frame(id, `${agent} logo`, {width: 14, height: 14, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${agent}.png`, mode: 'fit'}}, []);
  // The sidebar's descendant badge (DescendantBadge, Badge secondary in mono): working and done children.
  const kidsBadge = (id, [working, done]) => frame(id, '하위 에이전트', {layout: 'horizontal', gap: '$--spacing-xs', alignItems: 'center', height: 16, padding: [0, '$--spacing-xs'], cornerRadius: '$--radius-sm', fill: '$--secondary'}, [
    cap(`${id}-w`, `●${working}`, WORK, {mono: true}), cap(`${id}-d`, `✓${done}`, OK, {mono: true}),
  ]);
  // The four-cell stage bar 대기 · 작업 · 검증 · 머지 (B14): cells before the current one in
  // the done color, the current one in the working color, hatched when the card is in 멈춤.
  function stageBar(id, task) {
    const cell = (cid, name, fill) => frame(cid, name, {width: BAR_CELL, height: 3, cornerRadius: 1.5, fill}, []);
    const hatched = cid => frame(cid, '지금 칸, 멈춤', {width: BAR_CELL, height: 3, layout: 'horizontal', gap: 2}, [0, 1, 2, 3].map(i => frame(`${cid}-${i}`, 'Hatch', {width: 1.5, height: 3, fill: WORK}, [])));
    return frame(id, '단계 막대', {width: BAR_W, height: 3, layout: 'horizontal', gap: BAR_GAP}, [0, 1, 2, 3].map(i => {
      const cid = `${id}-${i}`;
      if (i < task.stage) return cell(cid, '끝난 칸', OK);
      if (i > task.stage) return cell(cid, '남은 칸', '$--border');
      return task.lane === 'stuck' ? hatched(cid) : cell(cid, '지금 칸', WORK);
    }));
  }
  function dashedRule(id, width) {
    const dashes = [];
    for (let x = 0; x < width; x += 6) dashes.push(`M ${x} 0.5 L ${Math.min(x + 3, width)} 0.5`);
    return {type: 'path', id, name: 'Dashed rule', width, height: 1, viewBox: [0, 0, width, 1], geometry: dashes.join(' '), stroke: '$--border', strokeWidth: HAIR};
  }
  const controlSm = num(tokens, '--size-control-sm');
  function taskCard(id, key, width, {height} = {}) {
    const task = TASKS[key];
    const size = sizeOf(width);
    const small = size === 'small';
    const wide = size === 'wide';
    const turn = task.wait === 'me';
    const inner = width - 2 * PAD_X - (turn ? STRIPE : 0);
    const glyphW = 14 + 6;
    const top = row(`${id}-a`, [
      cap(`${id}-id`, task.id, MUT, {mono: true}),
      ...(task.pr ? [row(`${id}-pr`, [icon(`${id}-prg`, task.merged ? 'git-merge' : 'git-pull-request', {size: 12, fill: MUT}), cap(`${id}-prn`, task.pr, MUT, {mono: true})], {gap: '$--spacing-xxs'})] : []),
      spacer(`${id}-as`),
      ...(task.agent ? [logo(`${id}-lg`, task.agent)] : []),
      ...(task.kids && !small ? [kidsBadge(`${id}-kb`, task.kids)] : []),
      ...(!small ? [cap(`${id}-ag`, task.age)] : []),
    ], {gap: '$--spacing-xs', width: 'fill_container', height: 16});
    const titleW = small ? inner - glyphW : inner - glyphW - BAR_W - 8;
    const title = row(`${id}-tt`, [
      frame(`${id}-sgw`, STATE_WORD[task.state], {width: 14, height: 18, layout: 'horizontal', alignItems: 'center'}, [icon(`${id}-sg`, STATE_GLYPH[task.state], {size: 14, fill: TONE[task.state]})]),
      small
        ? text(`${id}-t`, fitText(task.title, 2 * titleW - 16, 13), {size: '$--text-subhead', weight: '600', width: titleW})
        : text(`${id}-t`, fitText(task.title, titleW, 13), {size: '$--text-subhead', weight: '600'}),
      ...(!small ? [spacer(`${id}-ts`), stageBar(`${id}-bar`, task)] : []),
    ], {gap: 6, width: 'fill_container', alignItems: small ? 'start' : 'center'});
    const children = [col(`${id}-head`, [top, title], {gap: '$--spacing-xxs', width: 'fill_container'})];
    if (!small) {
      children.push(wide
        ? text(`${id}-sm`, fitText(task.summary, 2 * inner - 16, 12), {size: '$--text-body', fill: SUB, width: inner})
        : text(`${id}-sm`, fitText(task.summary, inner, 12), {size: '$--text-body', fill: SUB}));
      if (task.problem) {
        children.push(row(`${id}-pb`, [icon(`${id}-pbg`, task.problem.glyph, {size: 12, fill: task.problem.tone}), cap(`${id}-pbt`, task.problem.text, task.problem.tone, {weight: '500'})], {gap: '$--spacing-xs'}));
      }
      if (task.ask) {
        const [suggested, ...others] = task.ask.choices;
        children.push(frame(`${id}-ask`, '질문', {layout: 'vertical', gap: '$--spacing-sm', width: 'fill_container', padding: ['$--spacing-sm', 10], cornerRadius: '$--radius-sm', fill: '$--secondary'}, [
          text(`${id}-q`, fitText(task.ask.question, 2 * (inner - 20) - 16, 12), {size: '$--text-body', weight: '600', width: inner - 20}),
          row(`${id}-qa`, [
            screenButton(`${id}-qa0`, suggested, {height: controlSm}),
            ...(wide ? others.map((choice, i) => screenButton(`${id}-qa${i + 1}`, choice, {variant: 'outline', height: controlSm})) : []),
            screenButton(`${id}-qo`, '다른 답', {variant: 'ghost', height: controlSm}),
          ], {gap: '$--spacing-xs'}),
        ]));
      }
      if (task.action) {
        const [primary, secondary] = task.action;
        children.push(row(`${id}-act`, [
          screenButton(`${id}-act0`, primary, {height: controlSm}),
          ...(wide ? [screenButton(`${id}-act1`, secondary, {variant: 'outline', height: controlSm})] : []),
        ], {gap: '$--spacing-xs'}));
      }
    }
    if (wide) {
      children.push(row(`${id}-st`, [
        cap(`${id}-sw`, STATE_WORD[task.state], SUB, {weight: '500'}),
        ...(task.mark ? [row(`${id}-mk`, [text(`${id}-mg`, task.mark[0], {size: '$--text-caption', fill: task.mark[2], mono: true}), cap(`${id}-mw`, task.mark[1])], {gap: '$--spacing-xxs'})] : []),
      ], {gap: '$--spacing-md'}));
      if (task.ai && task.agent) {
        children.push(dashedRule(`${id}-dr`, inner));
        children.push(row(`${id}-ai`, [icon(`${id}-aig`, 'sparkles', {size: 12, fill: MUT}), cap(`${id}-ait`, fitText(task.ai, inner - 18, 11), SUB)], {gap: '$--spacing-xs'}));
      }
    }
    const done = task.state === 'done' || task.state === 'landed';
    return frame(id, task.title, {layout: 'horizontal', width, ...(height ? {height} : {}), cornerRadius: '$--radius-md', fill: '$--card', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', clip: true, ...(done ? {opacity: DIMMED} : {})}, [
      ...(turn ? [frame(`${id}-band`, '나를 기다림', {width: STRIPE, height: 'fill_container', fill: task.state === 'stopped' ? CRIT : WARN}, [])] : []),
      col(`${id}-body`, children, {gap: '$--spacing-sm', width: 'fill_container', padding: ['$--spacing-sm', PAD_X]}),
    ]);
  }

  // -- 보드 ---------------------------------------------------------------------------------------
  const LANE_GAP = num(tokens, '--spacing-md');
  function laneWidths() {
    const avail = MAIN - 2 * GUTTER - (LANES.length - 1) * LANE_GAP;
    const total = LANES.reduce((sum, [, , weight]) => sum + weight, 0);
    return LANES.map(([, , weight]) => Math.floor((avail * weight) / total));
  }
  const groupHead = (id, label) => row(id, [cap(`${id}-t`, label, MUT, {weight: '500'}), rule(`${id}-r`)], {width: 'fill_container', gap: '$--spacing-sm'});
  function boardBody(id) {
    const widths = laneWidths();
    return frame(id, '보드', {layout: 'vertical', gap: '$--spacing-sm', width: MAIN, height: 'fill_container', padding: [0, GUTTER, '$--spacing-lg', GUTTER], clip: true}, [
      row(`${id}-bar`, [spacer(`${id}-bs`), screenButton(`${id}-cancelled`, '취소됨', {variant: 'ghost', height: controlSm})], {width: 'fill_container'}),
      row(`${id}-cols`, LANES.map(([lane, label], li) => {
        const width = widths[li];
        const cards = keys => keys.map(key => taskCard(`${id}-c${li}-${key}`, key, width));
        const body = lane === 'stuck'
          ? BOARD.stuck.flatMap(([wait, name, keys]) => [groupHead(`${id}-c${li}-${wait}`, `${name} ${keys.length}`), ...cards(keys)])
          : [
            ...cards(BOARD[lane]),
            ...(lane === 'done' ? [row(`${id}-c${li}-fold`, [icon(`${id}-c${li}-fi`, 'chevron-right', {size: 12, fill: MUT}), cap(`${id}-c${li}-ft`, `3일 지난 완료 ${FOLDED_DONE}개`)], {gap: '$--spacing-xxs'})] : []),
          ];
        return col(`${id}-c${li}`, [cap(`${id}-c${li}-h`, `${label} ${laneCount(lane)}`, SUB, {weight: '500'}), ...body], {width, gap: '$--spacing-sm'});
      }), {gap: LANE_GAP, alignItems: 'start', width: 'fill_container'}),
    ]);
  }
  function boardMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [header(`${id}-hd`, {active: 1, counts: BOARD_FLOW, scope: 'herdr-ide'}), boardBody(`${id}-board`)]);
  }

  // -- 같은 카드, 세 크기 ------------------------------------------------------------------------------
  // Every kind of card at the three sizes side by side, the reference the web card is built from.
  const SIZE_COLUMNS = [['작게 · 240px 미만', 210], ['보통', 300], ['넓게 · 420px 이상', 470]];
  const SIZE_ROWS = [['t420', '답 필요'], ['t405', '머지 대기'], ['t417', '멈춤'], ['t412', '실행 중 · 검증 실패'], ['t415', '하위 에이전트가 있는 작업자'], ['t398', '검증 중'], ['t426', '쉬는 중 · 한도'], ['t430', '밖에서 진행'], ['t421', '시작 전 · 선행 기다림'], ['t410', '완료']];
  function sizesBody(id) {
    const LABEL_W = 132;
    return frame(id, '같은 카드, 세 크기', {layout: 'vertical', gap: '$--spacing-lg', padding: '$--spacing-xl', fill: '$--background', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      row(`${id}-h`, [frame(`${id}-h-pad`, 'Pad', {width: LABEL_W, height: 1}, []), ...SIZE_COLUMNS.map(([label, width], ci) => cap(`${id}-h${ci}`, label, SUB, {weight: '500', width}))], {gap: '$--spacing-xl'}),
      ...SIZE_ROWS.map(([key, label], ri) => row(`${id}-r${ri}`, [
        cap(`${id}-r${ri}-l`, label, SUB, {width: LABEL_W}),
        ...SIZE_COLUMNS.map(([, width], ci) => taskCard(`${id}-r${ri}-${ci}`, key, width)),
      ], {gap: '$--spacing-xl', alignItems: 'start'})),
    ]);
  }

  // -- 그래프 ------------------------------------------------------------------------------------
  // A graph node is a small card (B4), held at one height so the arrows meet its middle.
  const NODE_W = 216;
  const NODE_H = 62;
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
    const nodes = Object.entries(position).map(([key, [x, y]]) => ({...taskCard(`${id}-n-${key}`, key, NODE_W, {height: NODE_H}), x, y}));
    const layered = y0 + 3 * (NODE_H + rowGap) - rowGap;
    const perRow = 4;
    const gap = Math.floor((MAIN - 2 * x0 - perRow * NODE_W) / (perRow - 1));
    const unrelated = (idp, keys, y) => keys.map((key, i) => ({...taskCard(`${idp}-${key}`, key, NODE_W, {height: NODE_H}), x: x0 + (i % perRow) * (NODE_W + gap), y: y + Math.floor(i / perRow) * (NODE_H + rowGap)}));
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
    const empty = {before: 0, moving: 0, stuck: 0, done: 0};
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [
      header(`${id}-hd`, {active: 0, counts: empty, count: 0, read: null}),
      frame(`${id}-intake`, 'Intake', {width: MAIN, padding: ['$--spacing-md', GUTTER], layout: 'horizontal'}, [
        text(`${id}-it`, '아직 Task가 없습니다. 대화 중인 에이전트에게 넣어 달라고 하거나 GitHub issue에 factory 라벨을 붙이세요.', {size: '$--text-body', fill: MUT, width: MAIN - 2 * GUTTER}),
      ]),
    ]);
  }

  const id = name => `fx-${name}-${s}`;
  const turn = windowFrame(id('turn'), '내 차례', turnMain(id('turn')));
  const board = windowFrame(id('board'), '보드', boardMain(id('board')), {height: BOARD_H});
  const sizes = sizesBody(id('sizes'));
  const graph = windowFrame(id('graph'), '그래프', graphMain(id('graph')));
  const task = windowFrame(id('task'), 'Task 페이지', taskMain(id('task')));
  const none = windowFrame(id('none'), 'Factory 없음', noFactoryMain(id('none')), {height: 360, count: 0, secretary: false});
  const empty = windowFrame(id('empty'), 'Task 없음', noTaskMain(id('empty')), {height: 360, count: 0});

  return [
    col(id('frames'), [
      row(id('r1'), [
        captioned(id('turn'), '내 차례: 맨 위 항목이 펼쳐져 제안이 골라져 있고, ⏎ 한 번으로 보낸다', turn),
        captioned(id('board'), '보드: 움직임으로 네 열, 멈춤은 나를 기다림과 다른 걸 기다림으로 나누고, 시작 전과 완료는 좁아 카드가 작게 그려진다', board),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
      row(id('r1s'), [
        captioned(id('sizes'), '같은 카드, 세 크기: 좁을수록 덜 중요한 것부터 빠진다. 색은 상태 아이콘, 문제 줄, 왼쪽 띠에만', sizes),
      ], {alignItems: 'start'}),
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
