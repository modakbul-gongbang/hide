// The children of `Screen / Factory` (PRD software-factory-ui D-03, D-18, B1-B24; the 라인,
// 결정 필요 and Task page frames from PRD factory-human-loop D-09, D-31..D-35, D-43, B20..B33):
// 라인, 보드, 그래프, the Task page, the create sheet and the empty states, each
// inside the window with the sidebar's Factory row and the 비서 row beneath it.
// Drawn on this document's local tokens plus library refs (Button, Select, Kbd,
// Radio, Checkbox), the way every other Screen sheet is, and called from
// pen-screens.mjs, which owns the ref helpers and the sheet frame. Every name,
// title and number is invented mock content derived from one example data set
// below, so the 결정 필요 count, the tab number and the sidebar badge always agree.

import {frame, icon, num, text} from './pen-system.mjs';
import {fitText} from './pen-screens-disk.mjs';

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
  stopped: '멈춤', paused: '일시정지', outside: '밖에서 진행 중', done: '완료', landed: '머지됨',
};
const STATE_GLYPH = {
  drafting: 'circle-dashed', waiting: 'circle', resting: 'circle-pause', running: 'circle-dot', verifying: 'loader-circle', outside: 'circle-dot', blocked: 'circle-help',
  stopped: 'circle-pause', paused: 'circle-pause', merge_waiting: 'git-merge', done: 'circle-check', landed: 'circle-check',
};
const TONE = {
  drafting: MUT, waiting: MUT, resting: MUT, running: WORK, verifying: WORK, outside: MUT, blocked: WARN, stopped: CRIT, paused: MUT, merge_waiting: WARN, done: OK, landed: OK,
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

// 결정 필요 (PRD factory-human-loop B20, D-32, D-33): only what the person moves. Every item is one
// shape: a sentence, what it stopped, then 2-3 choices with their results, 다른 답, the default and
// its deadline, or one button for a to-do; the evidence folds at its foot. `key` is the Task the item
// stopped, absent for one that stops the whole Factory.
const NEED_QUESTION = {kind: 'question', glyph: 'message-square', key: 't420', sentence: 'Task 상세를 새 REST 엔드포인트로 낼까요, 기존 WS snapshot에 합칠까요?', stopped: '#420 작업 · #421, #422가 기다림', cue: '3일째',
  choices: [
    {label: 'WS snapshot에 합치기', result: 'Task마다 snapshot이 약 2 KB 커지고, 새 route는 없습니다', rec: true},
    {label: 'REST 엔드포인트', result: 'hided에 route가 하나 생기고, 웹은 Task 페이지를 열 때 따로 읽습니다'},
  ],
  deadline: '기본값 없음 · 답할 때까지 기다립니다',
  evidence: {summary: 'Factory AI가 확신하지 못함 · 두 길의 비용이 비슷함', lines: [
    ['왜 나에게', 'Factory AI가 확신하지 못해 내가 정합니다'],
    ['Factory AI의 이유', 'WS에 합치면 Task마다 snapshot이 약 2 KB 커지고, REST는 hided에 route가 하나 생깁니다. 어느 쪽도 카드의 완료 기준을 바꾸지 않습니다'],
    ['관련 카드', '완료 기준 2 "Task 페이지가 1초 안에 열린다"'],
    ['증거', '작업자가 잰 snapshot 크기 · Task 40개에서 81 KB, 합치면 163 KB'],
  ]}};
const NEED_MERGE = {kind: 'question', glyph: 'git-merge', key: 't405', sentence: 'PR #561은 위험 경로 hided/를 바꿉니다. 머지할까요?', stopped: '#405 머지', cue: '1시간',
  choices: [
    {label: '머지', result: 'main에 머지하고 #405를 닫습니다', rec: true},
    {label: '변경 요청', result: '쓴 요청을 작업자가 받아 고치고 다시 검증합니다'},
  ],
  deadline: '기본값 없음 · 위험 경로는 내가 머지합니다',
  evidence: {summary: '검증 통과 · 바뀐 파일 4개 · 그중 hided/ 2개'}};
const NEED_STOP = {kind: 'question', glyph: 'circle-alert', key: 't417', sentence: 'web-e2e가 세 번 연속 실패해 #417이 멈췄습니다. 어떻게 할까요?', stopped: '#417 검증', cue: '40분',
  choices: [
    {label: '다시 시작', result: '실패 횟수를 0으로 두고 작업자가 마지막 실패부터 이어서 고칩니다', rec: true},
    {label: '취소', result: 'Task를 닫고 PR #560은 열어 둡니다'},
  ],
  deadline: '기본값 없음 · 고를 때까지 멈춰 있습니다',
  evidence: {summary: '세 번 모두 디스크 정리 표의 정렬 확인에서 실패 · 마지막 실패 40분 전'}};
const NEED_AI_OFF = {kind: 'question', glyph: 'circle-off', sentence: 'Hide AI가 꺼져 있어 #438의 카드를 쓰지 못했습니다. 어떻게 시작할까요?', stopped: '#438 시작', task: '#438', cue: '20분',
  choices: [
    {label: 'Hide AI 켜기', result: '설정의 Hide AI를 엽니다. 켜면 접수 리뷰가 카드를 쓰고 바로 시작합니다', rec: true},
    {label: '이슈 그대로 시작', result: '이슈 본문을 목표로, 체크박스를 완료 기준으로 삼아 지금 시작합니다'},
  ],
  deadline: '기본값 없음 · 고를 때까지 시작하지 않습니다'};
const NEED_DISK = {kind: 'todo', glyph: 'hard-drive', key: 't431', sentence: '디스크 여유가 5GB보다 작아 새 작업자를 띄울 수 없습니다. 공간을 비워 주세요', stopped: '#431 시작', cue: '3시간',
  button: '다시 확인', buttonGlyph: 'refresh-cw', result: '여유가 5GB를 넘으면 #431 작업자를 띄웁니다',
  evidence: {summary: '자동 복구 4번 · 여유 3.1GB 그대로', open: true, lines: [
    ['자동 복구', '10:40 끝난 worktree 정리 · 일부만   11:10 작업자 재우기 · 그대로   12:10 끝난 worktree 정리 · 그대로   13:10 작업자 재우기 · 그대로'],
    ['지금', '여유 3.1GB · 기준 5GB'],
  ]}};
const NEED_COMMAND = {kind: 'todo', glyph: 'square-terminal', key: 't412', sentence: '이전 시작이 남긴 터미널이 #412 작업자 이름을 쥐고 있어 작업자를 띄울 수 없습니다. 그 터미널을 닫아 주세요', stopped: '#412 작업 다시 시작', cue: '15분',
  command: 'herdr pane close w4:p2', button: '닫았어요', buttonGlyph: 'check', result: '누르면 #412 작업자를 다시 띄웁니다',
  evidence: {summary: '같은 worktree의 터미널 하나가 작업자 이름을 쥐고 있음 · 보드에는 이 Task의 작업자 창이 없음'}};
const NEED_LOGIN = {kind: 'todo', glyph: 'key-round', sentence: 'GitHub에 다시 로그인하세요', stopped: '#398 CI 결과 읽기 · #405 머지', cue: '9분',
  command: 'gh auth login', button: '다시 확인', buttonGlyph: 'refresh-cw', result: '로그인이 통과하면 멈춘 일을 이어서 합니다',
  evidence: {summary: 'gh가 로그인을 다시 요구함 · 막힌 동작 2개는 기다리는 중'}};
// What 결정 필요 holds in the example data: the herdr-ide items the 라인 draws, and one sasu question.
const NEEDS = [
  {project: 'herdr-ide', ...NEED_QUESTION}, {project: 'herdr-ide', ...NEED_MERGE}, {project: 'herdr-ide', ...NEED_STOP},
  {project: 'sasu', kind: 'question', key: 's88'},
];
const needCount = project => NEEDS.filter(need => !project || need.project === project).length;
// The 결정 필요 frame draws every kind at once, so its count is its own.
const NEEDS_ALL = [NEED_QUESTION, NEED_MERGE, NEED_STOP, NEED_AI_OFF, NEED_DISK, NEED_COMMAND, NEED_LOGIN];

// The graph: layers by what each Task waits on; 420 -> 422 is implied by 420 -> 421 -> 422 and is not drawn.
const GRAPH_LAYERS = [['t420', 't410'], ['t421', 't412', 't415'], ['t422']];
const GRAPH_EDGES = [['t420', 't421'], ['t421', 't422'], ['t410', 't412'], ['t410', 't415'], ['t412', 't422']];
const GRAPH_UNRELATED = ['t7', 't431', 't405', 't417', 't398', 't426', 't430', 't409'];
const GRAPH_SASU = ['s91', 's88', 's86'];

// -- Observer (PRD factory-observer) ---------------------------------------------------------
// Two Factories: herdr-ide runs in 보조 and sasu in 자율 (D-03, D-18). These Tasks appear only
// on the Observer frames, so the board, graph and 내 차례 counts above stay as they were.
const OBS_TASKS = {
  t433: {id: '#433', project: 'herdr-ide', title: '설정 검색 결과 강조', state: 'stopped', lane: 'stuck', wait: 'me', stage: 1, agent: 'claude', age: '18분',
    summary: '설정 검색에서 맞은 글자를 굵게 보인다', problem: {glyph: 'circle-alert', text: '작업자 사라짐 · 다시 띄웠지만 또 사라짐', tone: CRIT}, action: ['다시 시작', '기록 보기']},
  t434: {id: '#434', project: 'herdr-ide', title: '빈 Factory 안내 문구', state: 'paused', lane: 'stuck', wait: 'me', stage: 1, agent: 'codex', age: '25분',
    summary: 'Task가 없을 때 넣는 방법을 한 줄로 안내한다', problem: {glyph: 'circle-pause', text: '일시정지 · Hide에서 작업자 창을 닫음', tone: MUT}, action: ['다시 시작', '기록 보기']},
  t435: {id: '#435', project: 'herdr-ide', title: 'Sessions 칩 정렬', state: 'stopped', lane: 'stuck', wait: 'me', stage: 1, agent: 'codex', age: '9분',
    summary: 'Sessions 칩을 최근 활동순으로 놓는다', problem: {glyph: 'circle-alert', text: '보고 없음 · 깨웠지만 답 없음', tone: CRIT}, action: ['다시 시작', '기록 보기'],
    diagnosis: '테스트 실행을 기다리다 멈춘 것으로 보입니다'},
  t436: {id: '#436', project: 'herdr-ide', title: '보드 빈 열 문구', state: 'blocked', lane: 'stuck', wait: 'me', stage: 1, agent: 'claude', age: '6분',
    summary: '빈 열에 보일 한 줄 문구를 정한다',
    ask: {question: '빈 열에 무엇을 보일까요?', choices: ['아무것도 보이지 않기', '"없음" 한 단어', '열마다 다른 안내']}, mark: ['?', '질문', WARN], ai: '세 안을 화면에 그려 두고 답을 기다림'},
  t437: {id: '#437', project: 'herdr-ide', title: '정렬 상태 기억', state: 'blocked', lane: 'stuck', wait: 'me', stage: 1, agent: 'codex', age: '30분',
    summary: '고른 정렬을 다시 열어도 그대로 둔다',
    ask: {question: '카드가 틀림: 완료 조건이 #412와 겹칩니다', choices: ['AI 제안: #412에 합치기', '그대로 진행']}, mark: ['?', '질문', WARN], ai: '두 Task의 완료 조건을 비교하고 답을 기다림'},
};
const taskOf = key => TASKS[key] ?? OBS_TASKS[key];
// The Factories: herdr-ide runs with 함께 and sasu, paused, with 맡김.
const OBS_FACTORIES = [
  {project: 'herdr-ide', mode: 1, worker: '작업자 후보 3', paused: false},
  {project: 'sasu', mode: 2, worker: '작업자 후보 1', paused: true},
];
// The worker candidates: an agent, model and effort with a line the operator writes; the
// Factory AI picks one per Task at intake from the card and these lines, the first is the default,
// and a candidate at its usage limit hands new starts to the next one.
const WORKERS = [
  {label: '기본', agent: 'Codex', model: 'gpt-6.1-sol', effort: 'high', when: '대부분의 Task'},
  {label: '후보', agent: 'Claude Code', model: 'opus', effort: 'max', when: 'herdr-core, 동시성, 큰 리팩터'},
  {label: '후보', agent: 'Codex', model: 'gpt-6.1-luna', effort: 'low', when: '문구, 문서, 작은 UI'},
];
// The full list (five, B32): an agent whose adapter declares no model or effort argument shows
// only "CLI 기본값".
const WORKERS_FULL = [
  ...WORKERS,
  {label: '후보', agent: 'Claude Code', model: 'sonnet', effort: 'medium', when: '테스트만 고치는 Task'},
  {label: '후보', agent: 'OpenCode', model: null, effort: null, when: '실험, 버려도 되는 시도'},
];
// What each choice hands to the AI (D-14, D-21, D-32): 직접 is the PRD's 수동, 함께 보조, 맡김 자율.
// The settings show the picked choice's two lists instead of the whole table.
const MODES = [
  {name: '직접', line: '모든 결정을 내가', me: ['기술 결정', '제품 결정', '카드 고침', '권한', '위험 경로 머지'], ai: ['답이 이미 있는 질문'], risk: '이 경로를 바꾼 PR은 내가 머지합니다'},
  {name: '함께', line: '기술은 AI, 제품은 내가', me: ['제품 결정', '카드 고침 (AI 제안)', '권한', '위험 경로 머지'], ai: ['기술 결정', '답이 이미 있는 질문'], risk: '이 경로를 바꾼 PR은 내가 머지합니다'},
  {name: '맡김', line: '권한만 내가', me: ['권한'], ai: ['기술 결정', '제품 결정', '카드 고침', '위험 경로 머지', '답이 이미 있는 질문'], risk: '이 경로를 바꾼 PR은 AI가 승인합니다. 다른 게이트가 있으면 내가 머지합니다'},
];

export function factoryRows(tokens, {themedXref, screenButton, screenSelect, screenIconButton, screenDialogSurface, screenRadioItem, screenMenuItem, screenMenuSeparator, screenMenuContent}, s) {
  const HAIR = num(tokens, '--size-hairline');
  const DISABLED = num(tokens, '--opacity-disabled');
  const DIMMED = num(tokens, '--opacity-dimmed');
  const W = 1440;
  const H = 900;
  const BOARD_H = 1000;
  const LOOP_LINE_H = 1910;
  const LOOP_DECISIONS_H = 2010;
  const LOOP_TASK_H = 1890;
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
  // A Factory under the sidebar's Factory row: picking it is the same choice as the header's project picker.
  function factoryChild(id, f) {
    return frame(id, f.project, {width: 'fill_container', height: 26, layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', padding: [0, '$--spacing-sm', 0, 26], cornerRadius: '$--radius-sm', ...(f.selected ? {fill: '$--secondary'} : {})}, [
      frame(`${id}-m`, f.paused ? 'Paused' : 'No mark', {width: 12, height: 12, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, f.paused ? [icon(`${id}-pg`, 'pause', {size: 10, fill: MUT})] : []),
      icon(`${id}-i`, 'folder-git-2', {size: 12, fill: MUT}),
      text(`${id}-t`, f.project, {size: '$--text-body', fill: f.selected ? FG : SUB}),
      spacer(`${id}-s`),
      ...(f.count ? [cap(`${id}-n`, String(f.count), WARN, {mono: true})] : []),
    ]);
  }
  // The sidebar with the Factory row under Overview (count, ⇧⌘F) and, once a Factory exists, the 비서 row beneath it.
  // With `factories`, each Factory is a row under it, and a picked one takes the selection from the Factory row.
  function sidebar(id, {count, secretary, selected = true, factories}) {
    return frame(id, 'Sidebar', {width: SIDE, height: 'fill_container', fill: '$--sidebar', stroke: '$--border', strokeWidth: {right: HAIR}, strokeAlignment: 'inner', layout: 'vertical', gap: '$--spacing-xs', padding: '$--spacing-md', clip: true}, [
      row(`${id}-h`, [text(`${id}-ht`, 'This Mac', {size: '$--text-subhead', weight: '600'}), spacer(`${id}-hs`), screenIconButton(`${id}-hp`, 'plus'), screenIconButton(`${id}-hq`, 'search')], {width: 'fill_container', height: 28, padding: [0, 0, 0, '$--spacing-sm']}),
      placeRow(`${id}-ov`, 'layout-dashboard', 'Overview', [cap(`${id}-ovn`, '2', WARN, {mono: true}), cap(`${id}-ovk`, '⇧⌘O', MUT, {mono: true})], false),
      placeRow(`${id}-fa`, 'factory', 'Factory', [...(count ? [cap(`${id}-fan`, String(count), WARN, {mono: true})] : []), cap(`${id}-fak`, '⇧⌘F', MUT, {mono: true})], selected && !factories?.some(f => f.selected)),
      ...(factories ?? []).map((f, i) => factoryChild(`${id}-fp${i}`, f)),
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
  function windowFrame(id, name, main, {height = H, count = needCount(), secretary = true, factories} = {}) {
    return frame(id, name, {width: W, height, fill: '$--background', layout: 'vertical', clip: true, cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      chrome(`${id}-chrome`),
      frame(`${id}-body`, 'Body', {width: W, height: height - CHROME, layout: 'horizontal'}, [rail(`${id}-rail`), sidebar(`${id}-side`, {count, secretary, factories}), main]),
    ]);
  }
  const caption = (id, label) => text(id, label, {size: '$--text-subhead', weight: '600', fill: FG});
  const captioned = (id, label, node) => col(`${id}-wrap`, [caption(`${id}-cap`, label), node], {gap: '$--spacing-sm'});

  // -- the header: title, project filter, create and ask, then the tabs with the Factory's state marks
  // at the row's end (PRD factory-human-loop B21, B24, B31, D-46): the last GitHub read, dimmed once
  // it is old, main 깨짐 and the daily AI limit. The flow bar is gone; the 라인 is the count.
  function factoryTabs(id, active, count) {
    const tabs = ['라인', '보드', '그래프', '설정'];
    return frame(id, 'Tabs', {layout: 'horizontal', gap: '$--spacing-xxs', padding: '$--spacing-xxs', fill: '$--card', cornerRadius: '$--radius-sm'}, tabs.map((label, i) =>
      frame(`${id}-${i}`, label, {layout: 'horizontal', alignItems: 'center', gap: '$--spacing-xs', height: 24, padding: [0, '$--spacing-md'], cornerRadius: '$--radius-xs', ...(i === active ? {fill: '$--secondary'} : {})}, [
        text(`${id}-${i}-l`, label, {size: '$--text-body', weight: '500', fill: i === active ? FG : SUB}),
        ...(i === 0 && count ? [cap(`${id}-${i}-n`, String(count), WARN, {mono: true})] : []),
      ])));
  }
  // A state mark at the tab row's end: main 깨짐 ringed in its colour, the limit plain, a stale read dimmed.
  const headMark = (id, glyph, label, fill, {ringed = false, dim = false} = {}) => row(id, [icon(`${id}-g`, glyph, {size: 12, fill}), cap(`${id}-t`, label, fill, {weight: ringed ? '600' : '400'})], {
    gap: '$--spacing-xs', height: 22, padding: [0, '$--spacing-sm'], cornerRadius: 11, ...(ringed ? {stroke: fill, strokeWidth: HAIR, strokeAlignment: 'inner'} : {}), ...(dim ? {opacity: DIMMED} : {})});
  // `pause` is null with every project picked, 'off' for a running Factory and 'on' for a paused one:
  // pausing starts no Task, puts every worker to sleep and calls no AI until 다시 시작.
  // `marks` replaces the plain read time; `read: null` draws no state at all (before the first read).
  function header(id, {active, count = needCount(), read = '3분 전', marks, scope = '모든 프로젝트', pause = null}) {
    const controlH = num(tokens, '--size-control-sm');
    return frame(id, 'Header', {layout: 'vertical', gap: '$--spacing-md', width: MAIN, padding: ['$--spacing-lg', GUTTER, '$--spacing-sm', GUTTER]}, [
      row(`${id}-tr`, [
        text(`${id}-t`, 'Factory', {size: '$--text-headline', weight: '600'}), screenSelect(`${id}-scope`, {content: scope, width: 148}),
        ...(pause === 'on' ? [row(`${id}-pz`, [icon(`${id}-pz-g`, 'pause', {size: 12, fill: MUT}), cap(`${id}-pz-t`, '일시정지됨', SUB)], {gap: '$--spacing-xs', height: 24, padding: [0, '$--spacing-md'], cornerRadius: 12, fill: '$--muted'})] : []),
        spacer(`${id}-s`),
        ...(pause === 'off' ? [screenButton(`${id}-pause`, '일시정지', {variant: 'ghost', height: controlH, icon: 'pause'})] : []),
        ...(pause === 'on' ? [screenButton(`${id}-resume`, '다시 시작', {variant: 'outline', height: controlH, icon: 'play'})] : []),
        screenButton(`${id}-new`, 'Factory 만들기', {variant: 'ghost', height: controlH, icon: 'plus'}),
        screenButton(`${id}-ask`, '비서에게 묻기', {variant: 'ghost', height: controlH, icon: 'message-square'}),
      ], {width: 'fill_container'}),
      row(`${id}-tb`, [
        factoryTabs(`${id}-tabs`, active, count), spacer(`${id}-tbs`),
        ...(marks ?? (read ? [cap(`${id}-read`, `GitHub 읽음 ${read}`)] : [])),
      ], {width: 'fill_container'}),
    ]);
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
    const task = taskOf(key);
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
      if (task.diagnosis) {
        children.push(dashedRule(`${id}-dr`, inner));
        children.push(row(`${id}-ai`, [icon(`${id}-aig`, 'sparkles', {size: 12, fill: MUT}), cap(`${id}-ait`, fitText(`Observer: ${task.diagnosis}`, inner - 18, 11), SUB)], {gap: '$--spacing-xs'}));
      } else if (task.ai && task.agent) {
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
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [header(`${id}-hd`, {active: 1, count: needCount('herdr-ide'), scope: 'herdr-ide'}), boardBody(`${id}-board`)]);
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
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [header(`${id}-hd`, {active: 2}), graphBody(`${id}-graph`)]);
  }

  // A bulleted line (the worker-pick page's card fields).
  function bullet(id, glyph, content, width, fill = FG) {
    const mark = glyph ? icon(`${id}-g`, glyph, {size: 12, fill: MUT}) : frame(`${id}-gw`, 'Bullet', {width: 12, height: 16, layout: 'horizontal', justifyContent: 'center', alignItems: 'center'}, [{type: 'ellipse', id: `${id}-g`, name: 'Ring', width: 8, height: 8, stroke: MUT, strokeWidth: HAIR, strokeAlignment: 'inner'}]);
    return row(`${id}`, [mark, text(`${id}-t`, content, {size: '$--text-body', fill, width: width - 20})], {gap: '$--spacing-sm', alignItems: 'start', width});
  }
  const sectionLabel = (id, label) => text(id, label, {size: '$--text-subhead', weight: '600'});

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
  // -- Observer (PRD factory-observer) -------------------------------------------------------------
  const sideFactories = picked => OBS_FACTORIES.map(f => ({...f, count: needCount(f.project), selected: f.project === picked}));
  // Settings rows (settings-rows.tsx Group and Row) in the settings sheet's width.
  const SHEET_W = num(tokens, '--size-settings-sheet-w');
  const settingsGroup = (id, title, rows, note) => col(id, [
    row(`${id}-hd`, [text(`${id}-t`, title, {size: '$--text-body', weight: '600', fill: SUB}), ...(note ? [cap(`${id}-n`, note, MUT)] : [])], {gap: '$--spacing-sm'}),
    frame(`${id}-box`, title, {layout: 'vertical', width: SHEET_W, cornerRadius: '$--radius-md', fill: '$--card', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', clip: true},
      rows.flatMap((node, i) => (i ? [rule(`${id}-r${i}`), node] : [node]))),
  ], {gap: '$--spacing-sm'});
  const settingsRow = (id, label, control, detail) => col(id, [
    row(`${id}-h`, [text(`${id}-l`, label, {size: '$--text-subhead'}), spacer(`${id}-s`), ...control], {width: 'fill_container'}),
    ...(detail ? [detail] : []),
  ], {gap: '$--spacing-sm', width: 'fill_container', padding: ['$--spacing-sm', '$--spacing-md']});
  const numberField = (id, value) => frame(id, 'Number field', {layout: 'horizontal', alignItems: 'center', justifyContent: 'end', width: 72, height: controlSm, padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-sm', fill: '$--background', stroke: '$--input', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
    text(`${id}-t`, value, {size: '$--text-body', mono: true}),
  ]);
  const switchOn = (id, on) => frame(id, 'Switch', {width: 32, height: 18, cornerRadius: 9, fill: on ? '$--primary' : '$--secondary', padding: 2, layout: 'horizontal', justifyContent: on ? 'end' : 'start', alignItems: 'center'}, [
    frame(`${id}-k`, 'Knob', {width: 14, height: 14, cornerRadius: 7, fill: on ? '$--primary-foreground' : MUT}, []),
  ]);
  // An empty field shows its placeholder in the muted colour.
  const textValue = (id, value, width, {mono = true, placeholder} = {}) => frame(id, 'Text field', {layout: 'horizontal', alignItems: 'center', width, height: controlSm, padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-sm', fill: '$--background', stroke: '$--input', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
    value || !placeholder ? text(`${id}-t`, value, {size: '$--text-body', mono}) : text(`${id}-t`, placeholder, {size: '$--text-body', fill: MUT}),
  ]);
  // A folded line (settings-rows.tsx Disclosure): chevron, title, a short summary of what is inside.
  const disclosure = (id, title, summary, open = false) => row(id, [
    icon(`${id}-g`, open ? 'chevron-down' : 'chevron-right', {size: 14, fill: MUT}), text(`${id}-t`, title, {size: '$--text-subhead'}), spacer(`${id}-s`), ...(summary ? [cap(`${id}-x`, summary, MUT)] : []),
  ], {width: 'fill_container', padding: ['$--spacing-sm', '$--spacing-md']});
  // An agent, its model and its effort, on one line; an agent whose adapter declares neither
  // launch argument shows "CLI 기본값" in their place (B32).
  const agentPick = (id, agent, model, effort) => model
    ? [screenSelect(`${id}-a`, {content: agent, width: 128}), screenSelect(`${id}-m`, {content: model, width: 128}), cap(`${id}-el`, 'effort', MUT), screenSelect(`${id}-e`, {content: effort, width: 84})]
    : [screenSelect(`${id}-a`, {content: agent, width: 128}), cap(`${id}-cli`, 'CLI 기본값', SUB, {width: 128 + 84 + 2 * GAP_SM + 40})];
  // The three choices as radio cards; the picked one is filled and ringed.
  const PAD_MD = num(tokens, '--spacing-md');
  const GAP_SM = num(tokens, '--spacing-sm');
  const CHOICE_W = Math.floor((SHEET_W - 2 * PAD_MD - 2 * GAP_SM) / 3);
  const choiceCard = (id, mode, picked, dim) => frame(id, mode.name, {layout: 'vertical', gap: '$--spacing-xxs', width: CHOICE_W, padding: ['$--spacing-sm', '$--spacing-md'], cornerRadius: '$--radius-md', fill: picked ? '$--secondary' : '$--background', stroke: picked ? FG : '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', ...(dim ? {opacity: DISABLED} : {})}, [
    screenRadioItem(`${id}-r`, mode.name, picked), frame(`${id}-lw`, 'Line', {layout: 'horizontal', padding: [0, 0, 0, 22]}, [cap(`${id}-l`, mode.line, SUB)]),
  ]);
  const chip = (id, label) => frame(id, label, {layout: 'horizontal', alignItems: 'center', height: 20, padding: [0, '$--spacing-sm'], cornerRadius: '$--radius-sm', fill: '$--secondary'}, [cap(`${id}-t`, label, FG)]);
  const whoLine = (id, label, glyph, items) => row(id, [
    row(`${id}-l`, [icon(`${id}-g`, glyph, {size: 12, fill: MUT}), cap(`${id}-t`, label, SUB)], {gap: '$--spacing-xs', width: 104}), ...items.map((item, i) => chip(`${id}-${i}`, item)),
  ], {gap: '$--spacing-xs'});
  const meter = (id, used, limit, width = 160) => frame(id, 'Meter', {width, height: 6, cornerRadius: 3, fill: '$--secondary', layout: 'horizontal'}, [
    frame(`${id}-v`, 'Used', {width: Math.round((width * used) / limit), height: 6, cornerRadius: 3, fill: '$--primary'}, []),
  ]);
  // One Factory's settings, for the project the header picks. Only what the operator decides:
  // who decides what (the choice and its two lists), which agents, how merges land, and the
  // macOS line; every other engine default sits under 고급 설정 and `hide factory config`.
  // 고급 설정 unfolded (B36): the seven rows B36 names, then every other value with the control
  // it has today under small subheads; `hide factory config` sets any of them too.
  const unit = (id, value, label) => row(id, [numberField(`${id}-n`, value), cap(`${id}-u`, label, SUB)], {gap: '$--spacing-xs'});
  const FIELD_W = num(tokens, '--size-settings-control-w');
  // A small heading inside 고급 설정 and the rows it groups, hairlines between the rows only.
  const subgroup = (id, title, rows) => col(id, [
    row(`${id}-head`, [cap(`${id}-head-t`, title, SUB, {weight: '600'})], {width: 'fill_container', padding: ['$--spacing-md', '$--spacing-md', '$--spacing-xxs', '$--spacing-md']}),
    ...rows.flatMap((node, i) => (i ? [rule(`${id}-r${i}`), node] : [node])),
  ], {gap: 0, width: 'fill_container'});
  const RECOVERY = ['끝난 Task의 worktree 지우기', '멈춘 작업자 다시 시작', '입력을 기다리는 작업자 재우고 깨우기', '사용량이 막히면 런타임 바꾸기', 'GitHub 읽기 다시 시도와 다시 연결'];
  function advancedRows(id) {
    return [
      disclosure(`${id}`, '고급 설정', '', true),
      settingsRow(`${id}-ask`, '질문 기한', [unit(`${id}-ask-v`, '24', '시간')], cap(`${id}-ask-d`, '기한이 지나면 기본 행동으로 진행합니다', MUT)),
      settingsRow(`${id}-stall`, '멈춤 판단 시간', [unit(`${id}-stall-q`, '30', '분 동안 조용하면'), unit(`${id}-stall-r`, '2', '분 동안 보고가 없으면')]),
      settingsRow(`${id}-lim`, 'AI 판단 상한', [unit(`${id}-lim-v`, '100', '번 / 하루')], cap(`${id}-lim-d`, '닿으면 그날 남은 결정은 나에게 옵니다', MUT)),
      settingsRow(`${id}-watch`, '점검', [unit(`${id}-watch-i`, '30', '분마다'), unit(`${id}-watch-n`, '5', '번 / 하루')]),
      settingsRow(`${id}-keep`, '보관 기간', [unit(`${id}-keep-c`, '7', '일 취소'), unit(`${id}-keep-d`, '3', '일 완료 접기'), unit(`${id}-keep-a`, '90', '일 목록')]),
      settingsRow(`${id}-rec`, '복구 범위', [cap(`${id}-rec-n`, '사람 없이 하는 일', MUT)],
        col(`${id}-rec-list`, RECOVERY.map((label, i) => row(`${id}-rec-${i}`, [checkbox(`${id}-rec-${i}-c`, false), cap(`${id}-rec-${i}-t`, label, FG)], {gap: '$--spacing-sm'})), {gap: '$--spacing-xs'})),
      settingsRow(`${id}-args`, '작업자 인자', [cap(`${id}-args-n`, '에이전트마다 시작할 때 붙임', MUT)],
        col(`${id}-args-list`, [['Claude Code', '--permission-mode acceptEdits'], ['Codex', ''], ['OpenCode', '']].map(([agent, args], i) => row(`${id}-args-${i}`, [
          cap(`${id}-args-${i}-a`, agent, SUB, {width: 96}), textValue(`${id}-args-${i}-v`, args, SHEET_W - 2 * PAD_MD - 96 - GAP_SM),
        ], {gap: '$--spacing-sm'})), {gap: '$--spacing-xs'})),
      subgroup(`${id}-sv`, '검증', [
        settingsRow(`${id}-sv-ci`, '필수 체크', [textValue(`${id}-sv-ci-v`, 'web-e2e, rust-test', FIELD_W)]),
        settingsRow(`${id}-sv-fail`, '멈추기 전 실패 횟수', [numberField(`${id}-sv-fail-v`, '3')]),
        settingsRow(`${id}-sv-time`, '검증 시간 제한(분)', [numberField(`${id}-sv-time-v`, '30')]),
      ]),
      subgroup(`${id}-sm`, '머지', [
        settingsRow(`${id}-sm-way`, '머지 방법', [screenSelect(`${id}-sm-way-v`, {content: 'squash', width: 104})]),
        settingsRow(`${id}-sm-quick`, '머지 전 빠른 점검', [textValue(`${id}-sm-quick-v`, 'scripts/verify-web.sh', FIELD_W)]),
      ]),
      subgroup(`${id}-sr`, '실행', [
        settingsRow(`${id}-sr-h`, 'harness preset', [textValue(`${id}-sr-h-v`, '', FIELD_W, {mono: false, placeholder: '이름: 작업 방식'})], cap(`${id}-sr-h-d`, '이름과 작업 방식. worker 프롬프트에 들어갑니다', MUT)),
        settingsRow(`${id}-sr-new`, 'worker가 더할 수 있는 새 Task', [numberField(`${id}-sr-new-v`, '3')]),
        settingsRow(`${id}-sr-disk`, '남길 디스크 공간(GB)', [numberField(`${id}-sr-disk-v`, '5')]),
        settingsRow(`${id}-sr-prd`, 'PRD를 issue에 넣기', [switchOn(`${id}-sr-prd-sw`, false)]),
      ]),
      subgroup(`${id}-sc`, '점검', [
        settingsRow(`${id}-sc-read`, 'GitHub 읽기 간격(분)', [numberField(`${id}-sc-read-v`, '5')]),
        settingsRow(`${id}-sc-0`, 'done 직후', [cap(`${id}-sc-0-t`, '변경이 요구한 범위 밖으로 번지지 않았는지 본다', SUB)]),
        settingsRow(`${id}-sc-add`, '점검 더하기', [
          screenSelect(`${id}-sc-add-at`, {content: 'done 직후', width: 104}),
          textValue(`${id}-sc-add-v`, '', FIELD_W, {mono: false, placeholder: '점검할 내용'}),
          {...screenButton(`${id}-sc-add-b`, '더하기', {variant: 'secondary', height: controlSm}), opacity: DISABLED},
        ]),
      ]),
      subgroup(`${id}-sa`, '자율 처리', [
        settingsRow(`${id}-sa-0`, 'branch 이름을 Task 번호에 맞춘다', [switchOn(`${id}-sa-0-sw`, true)]),
        settingsRow(`${id}-sa-diff`, '자율 변경 최대 크기(줄)', [numberField(`${id}-sa-diff-v`, '200')]),
      ]),
      row(`${id}-cli`, [cap(`${id}-cli-t`, '모든 값은 hide factory config로도 바꿀 수 있습니다', MUT)], {width: 'fill_container', padding: ['$--spacing-xs', '$--spacing-md']}),
    ];
  }
  function settingsBody(id, {aiOff, mode = 1, workers = WORKERS, used = 37, advanced = false}) {
    const pick = MODES[mode];
    const full = workers.length >= 5;
    const capped = used >= 100;
    const handOff = settingsGroup(`${id}-ai`, 'AI에게 맡기기', [
      col(`${id}-mode`, [
        row(`${id}-cards`, MODES.map((m, i) => choiceCard(`${id}-c${i}`, m, i === mode, aiOff)), {gap: '$--spacing-sm'}),
        aiOff
          ? row(`${id}-off`, [icon(`${id}-off-g`, 'circle-off', {size: 12, fill: MUT}), cap(`${id}-off-t`, 'Hide AI가 꺼져 있어 모든 결정이 나에게 옵니다. 앱 설정에서 켜면 고른 칸이 적용됩니다.', SUB)], {gap: '$--spacing-xs'})
          : col(`${id}-who`, [whoLine(`${id}-me`, '나에게 오는 것', 'user', pick.me), whoLine(`${id}-bot`, 'AI가 하는 것', 'sparkles', pick.ai)], {gap: '$--spacing-xs'}),
      ], {gap: '$--spacing-md', width: 'fill_container', padding: '$--spacing-md'}),
      aiOff
        ? settingsRow(`${id}-ai-a`, '에이전트', [cap(`${id}-ai-a-v`, '없음', SUB)])
        : settingsRow(`${id}-ai-a`, '에이전트', agentPick(`${id}-ai-a`, 'Claude Code', 'sonnet', 'low')),
      ...(aiOff ? [] : [settingsRow(`${id}-today`, '오늘 AI 판단', [meter(`${id}-today-m`, used, 100), cap(`${id}-today-n`, `${used} / 100`, capped ? WARN : SUB, {mono: true})],
        capped ? cap(`${id}-today-d`, '상한에 닿아 오늘 남은 결정은 나에게 옵니다 · 내일 0부터 다시 셉니다', MUT) : null)]),
    ], '작업자의 질문과 머지를 누가 정할지');
    const candidate = (cid, w, i) => settingsRow(cid, w.label, [
      ...agentPick(cid, w.agent, w.model, w.effort),
      i ? screenIconButton(`${cid}-x`, 'x', {size: 20}) : frame(`${cid}-xp`, 'Pad', {width: 20, height: 1}, []),
    ], textValue(`${cid}-w`, w.when, SHEET_W - 2 * PAD_MD, {mono: false}));
    const worker = settingsGroup(`${id}-wk`, '작업자', [
      ...workers.map((w, i) => candidate(`${id}-wk${i}`, w, i)),
      row(`${id}-wk-add`, [
        {...screenButton(`${id}-wk-add-b`, '후보 추가', {variant: 'ghost', height: controlSm, icon: 'plus'}), ...(full ? {opacity: DISABLED} : {})},
        ...(full ? [cap(`${id}-wk-full`, '후보는 다섯 개까지', MUT)] : []),
        spacer(`${id}-wk-add-s`), cap(`${id}-wk-d`, '동시에 도는 작업자는 이 Mac 전체에서 5명 · 모든 프로젝트 설정', MUT),
      ], {width: 'fill_container', padding: ['$--spacing-xs', '$--spacing-md']}),
    ], aiOff ? '기본 후보로 시작하고, 사용량이 막히면 다음 후보' : 'Factory AI가 Task마다 카드와 설명을 보고 고름 · 사용량이 막히면 다음 후보');
    const merge = settingsGroup(`${id}-merge`, '머지', [
      settingsRow(`${id}-mm`, '검증을 통과하면 바로 머지', [switchOn(`${id}-mm-sw`, true)], cap(`${id}-mm-d`, '끄면 모든 PR을 내가 머지합니다', MUT)),
      settingsRow(`${id}-vf`, '검증', [cap(`${id}-vf-t`, 'CI 필수 체크 · web-e2e, rust-test', SUB)]),
      settingsRow(`${id}-rp`, '위험 경로', [textValue(`${id}-rp-v`, 'hided/, herdr-core/', 200)], cap(`${id}-rp-d`, aiOff ? MODES[0].risk : pick.risk, MUT)),
    ]);
    const more = settingsGroup(`${id}-more`, '그 밖', [
      settingsRow(`${id}-mac`, '내 차례 macOS 알림', [switchOn(`${id}-mac-sw`, true)], cap(`${id}-mac-d`, '답할 것, 머지, 멈춤이 생기면 알립니다. 누르면 그 항목이 열립니다', MUT)),
      ...(advanced ? advancedRows(`${id}-adv`) : [disclosure(`${id}-adv`, '고급 설정', '질문 기한 · 멈춤 판단 시간 · AI 판단 상한 · 점검 · 보관 기간 · 복구 범위 · 작업자 인자')]),
      settingsRow(`${id}-close`, 'Factory 닫기', [screenButton(`${id}-close-b`, '닫기', {variant: 'outline', height: controlSm})], cap(`${id}-close-d`, '새 작업을 받지 않습니다. 기록은 남습니다', MUT)),
    ]);
    return frame(id, '설정', {layout: 'vertical', gap: '$--spacing-lg', width: MAIN, height: 'fill_container', padding: ['$--spacing-sm', GUTTER, '$--spacing-lg', GUTTER], clip: true}, [handOff, worker, merge, more]);
  }
  // Settings with every project picked: one row per Factory (state, choice, worker, 내 차례,
  // pause), which opens that Factory's settings, and the one machine-wide number.
  function allSettingsBody(id) {
    const factoryRow = (rid, f) => row(rid, [
      text(`${rid}-p`, f.project, {size: '$--text-subhead', weight: '600', width: 112}),
      row(`${rid}-st`, f.paused ? [icon(`${rid}-st-g`, 'pause', {size: 12, fill: MUT}), cap(`${rid}-st-t`, '일시정지', SUB)] : [dot(`${rid}-st-d`, WORK, 7), cap(`${rid}-st-t`, '돌고 있음', SUB)], {gap: '$--spacing-xs', width: 88}),
      row(`${rid}-m`, [icon(`${rid}-m-g`, 'sparkles', {size: 12, fill: MUT}), cap(`${rid}-m-t`, MODES[f.mode].name, SUB)], {gap: '$--spacing-xs', width: 64}),
      row(`${rid}-w`, [icon(`${rid}-w-g`, 'square-terminal', {size: 12, fill: MUT}), cap(`${rid}-w-t`, f.worker, SUB)], {gap: '$--spacing-xs'}),
      spacer(`${rid}-sp`),
      ...(f.count ? [cap(`${rid}-n`, `내 차례 ${f.count}`, WARN)] : []),
      screenIconButton(`${rid}-pz`, f.paused ? 'play' : 'pause'),
      icon(`${rid}-go`, 'chevron-right', {size: 14, fill: MUT}),
    ], {width: 'fill_container', gap: '$--spacing-md', padding: ['$--spacing-sm', '$--spacing-md']});
    return frame(id, '설정 · 모든 프로젝트', {layout: 'vertical', gap: '$--spacing-lg', width: MAIN, height: 'fill_container', padding: ['$--spacing-sm', GUTTER, '$--spacing-lg', GUTTER], clip: true}, [
      settingsGroup(`${id}-fs`, 'Factory', sideFactories(null).map((f, i) => factoryRow(`${id}-f${i}`, f)), '누르면 그 프로젝트의 설정'),
      settingsGroup(`${id}-mach`, '이 Mac 전체', [
        settingsRow(`${id}-mw`, '동시에 도는 작업자', [numberField(`${id}-mw-n`, '5')], cap(`${id}-mw-d`, '모든 Factory의 작업자를 합친 수입니다', MUT)),
      ]),
    ]);
  }
  function settingsMain(id, {project = 'herdr-ide', ...opts} = {}) {
    const all = project === null;
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [
      header(`${id}-hd`, all ? {active: 3} : {active: 3, count: needCount(project), scope: project, pause: 'off'}),
      all ? allSettingsBody(`${id}-set`) : settingsBody(`${id}-set`, {aiOff: false, ...opts}),
    ]);
  }
  // The cards the Observer changes, at the three sizes.
  const OBS_ROWS = [['t436', '답 필요 · 선택지 셋'], ['t437', '카드가 틀림 · AI 제안'], ['t435', '보고 없음 · 진단'], ['t433', '작업자 사라짐 · 재시작 뒤'], ['t434', '일시정지']];
  function obsCardsBody(id) {
    const LABEL_W = 132;
    return frame(id, 'Observer 카드', {layout: 'vertical', gap: '$--spacing-lg', padding: '$--spacing-xl', fill: '$--background', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      row(`${id}-h`, [frame(`${id}-h-pad`, 'Pad', {width: LABEL_W, height: 1}, []), ...SIZE_COLUMNS.map(([label, width], ci) => cap(`${id}-h${ci}`, label, SUB, {weight: '500', width}))], {gap: '$--spacing-xl'}),
      ...OBS_ROWS.map(([key, label], ri) => row(`${id}-r${ri}`, [
        cap(`${id}-r${ri}-l`, label, SUB, {width: LABEL_W}),
        ...SIZE_COLUMNS.map(([, width], ci) => taskCard(`${id}-r${ri}-${ci}`, key, width)),
      ], {gap: '$--spacing-xl', alignItems: 'start'})),
    ]);
  }
  // A Task that has not started yet, on the Task page (PRD factory-human-loop B27), with the worker
  // menu open (B33): the Factory AI's pick is checked and carries its reason; picking another
  // candidate makes it the person's. The menu sits under the track, where the page puts it.
  function obsPickPage(id) {
    const task = TASKS.t431;
    const inner = MAIN - 2 * GUTTER;
    const TEXT_W = 760;
    const pickW = 520;
    const label = w => (w.model ? `${w.agent} · ${w.model} · ${w.effort}` : `${w.agent} · CLI 기본값`);
    const AI_PICK = 2;
    const HOVER = 1;
    // Every row keeps the check's slot, as a radio menu does, so the labels line up; the slot is
    // painted in the row's own fill except on the picked row.
    const pickItem = (mid, w, i) => themedXref(mid, 'mnu-item-m', label(w), i === HOVER ? {fill: '$--accent'} : {}, {
      'mnu-item-icon': {icon: 'check', enabled: true, fill: i === AI_PICK ? FG : i === HOVER ? '$--accent' : '$--popover'},
      'mnu-item-label': {content: label(w), fill: i === HOVER ? '$--accent-foreground' : FG},
      'mnu-item-reason': {content: i === AI_PICK ? `${w.when} · Factory AI가 고름` : w.when, enabled: true, textGrowth: 'fixed-width', width: pickW - 48},
      'mnu-item-shortcut': {enabled: false},
    });
    const menu = screenMenuContent(`${id}-menu`, pickW, [
      ...WORKERS.map((w, i) => pickItem(`${id}-mi${i}`, w, i)),
      screenMenuSeparator(`${id}-msep`),
      screenMenuItem(`${id}-mfoot`, '후보는 설정의 작업자에서 바꿉니다', {state: 'disabled'}),
    ]);
    return frame(`${id}-page`, 'Task page', {width: MAIN, height: 'fill_container', layout: 'vertical', gap: '$--spacing-xl', padding: [14, GUTTER, '$--spacing-xl', GUTTER], clip: true}, [
      row(`${id}-nav`, [screenButton(`${id}-back`, '라인', {variant: 'ghost', height: controlSm, icon: 'arrow-left'}), spacer(`${id}-ns`)], {width: 'fill_container'}),
      col(`${id}-title`, [
        row(`${id}-tr`, [
          text(`${id}-t`, task.title, {size: '$--text-headline', weight: '600'}),
          row(`${id}-chip`, [icon(`${id}-chip-g`, STATE_GLYPH[task.state], {size: 12, fill: TONE[task.state]}), cap(`${id}-chip-t`, STATE_WORD[task.state], TONE[task.state])], {gap: '$--spacing-xs', height: 24, padding: [0, '$--spacing-md'], cornerRadius: 12, fill: '$--muted'}),
          spacer(`${id}-ts`),
          screenButton(`${id}-cancel`, '취소', {variant: 'ghost', height: controlSm}),
        ], {gap: '$--spacing-md', width: 'fill_container'}),
        body(`${id}-state`, '작업자 자리를 기다림', {fill: SUB}),
        row(`${id}-meta`, [taskRef(`${id}-ref`, task.id), cap(`${id}-meta-t`, 'herdr-ide · 431-doc-links')], {gap: '$--spacing-xs', width: 'fill_container'}),
      ], {gap: 6, width: 'fill_container'}),
      trackWide(`${id}-track`, 1, MUT, {0: [['ai', '가정 1']]}, 720),
      col(`${id}-pick`, [
        row(`${id}-wk`, [icon(`${id}-wk-g`, 'square-terminal', {size: 14, fill: MUT}), body(`${id}-wk-t`, '작업자'), spacer(`${id}-wk-s`), screenSelect(`${id}-wk-sel`, {content: label(WORKERS[AI_PICK]), width: 240})], {width: pickW}),
        menu,
        row(`${id}-why`, [icon(`${id}-why-g`, 'sparkles', {size: 12, fill: MUT}), cap(`${id}-why-t`, `Factory AI가 고른 후보: ${WORKERS[AI_PICK].when} · 문서 링크만 고치는 작은 변경`, SUB)], {gap: '$--spacing-xs'}),
        cap(`${id}-pick-h`, '다른 후보를 고르면 그 후보로 시작하고 Factory AI는 고르지 않습니다', MUT),
      ], {gap: '$--spacing-sm'}),
      rule(`${id}-rule`),
      col(`${id}-sum`, [section(`${id}-sum-h`, '요약'), body(`${id}-sum-t`, task.summary, {width: TEXT_W})]),
      col(`${id}-cr`, [section(`${id}-cr-h`, '완료 기준'), ...['깨진 상대 링크 23개가 맞는 문서를 가리킨다', 'check-doc-links가 docs 전체에서 통과한다'].map((line, i) => bullet(`${id}-d${i}`, null, line, inner))]),
    ]);
  }
  function obsPickMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [obsPickPage(`${id}-tp`)]);
  }
  // -- 라인, 결정 필요 and the Task page (PRD factory-human-loop) ------------------------------
  // The operator picked candidate A of the scratch drafts: 결정 필요 on top, then one table row per
  // Task, then the folded 후속 후보. These parts are this screen's own (web/src/factory/), so they
  // are drawn here on local tokens and library refs, like the board's cards.
  const LG = num(tokens, '--spacing-lg');
  const SM = num(tokens, '--spacing-sm');
  const MD = num(tokens, '--spacing-md');
  const INNER = MAIN - 2 * GUTTER;
  const STEP_WORDS = ['접수', '작업', '검증', '머지'];
  const radioDot = (id, on) => themedXref(id, 'rad-m', on ? 'Radio on' : 'Radio off', {}, {'rad-dot': {enabled: on}});
  const field = (id, width, {value, placeholder}) => themedXref(id, 'inp-m', 'Input', {width, height: controlSm}, {'inp-t': value ? {content: value, fill: FG} : {content: placeholder, fill: MUT}});
  const rightCap = (id, content, fill, width) => text(id, content, {size: '$--text-caption', fill, width, align: 'right'});

  // The four steps 접수 · 작업 · 검증 · 머지 (B25). `at` is the current step (0-3, 4 merged); `tone`
  // paints it: working blue while the factory moves it, warning on the person's turn, muted while
  // it waits for a slot or a predecessor.
  const cellFill = (i, at, tone) => (i < at ? OK : i === at ? tone : '$--border');
  function trackSmall(id, at, tone) {
    return col(id, [
      frame(`${id}-c`, 'Cells', {layout: 'horizontal', gap: 2}, STEP_WORDS.map((word, i) => frame(`${id}-c${i}`, word, {width: 28, height: 4, cornerRadius: 2, fill: cellFill(i, at, tone)}, []))),
      cap(`${id}-w`, at >= 4 ? '머지됨' : STEP_WORDS[at], at >= 4 ? OK : tone, {weight: '500'}),
    ], {gap: '$--spacing-xs', width: 118, name: 'Track'});
  }
  // The Task page's track: the step names joined by rules, and under a step who decided there.
  function trackWide(id, at, tone, marks, width) {
    const cells = [];
    STEP_WORDS.forEach((word, i) => {
      const done = i < at;
      const now = i === at;
      cells.push(col(`${id}-s${i}`, [
        row(`${id}-s${i}-w`, [
          icon(`${id}-s${i}-g`, done ? 'circle-check' : now ? 'circle-dot' : 'circle', {size: 12, fill: done ? OK : now ? tone : MUT}),
          body(`${id}-s${i}-t`, word, {fill: now ? tone : done ? FG : MUT, weight: now ? '600' : '400'}),
        ], {gap: '$--spacing-xs'}),
        ...(marks[i] ?? []).map(([who, label], j) => row(`${id}-s${i}-m${j}`, [
          who === 'note' ? frame(`${id}-s${i}-m${j}-g`, 'No glyph', {width: 11, height: 11}, []) : icon(`${id}-s${i}-m${j}-g`, who === 'ai' ? 'sparkles' : 'user', {size: 11, fill: who === 'me' ? WARN : MUT}),
          cap(`${id}-s${i}-m${j}-t`, label, who === 'me' ? WARN : MUT),
        ], {gap: '$--spacing-xxs', padding: [0, 0, 0, 16]})),
      ], {gap: '$--spacing-xxs', name: word}));
      if (i < 3) cells.push(frame(`${id}-l${i}`, 'Link', {width: 'fill_container', height: 18, layout: 'vertical', justifyContent: 'center'}, [frame(`${id}-l${i}-r`, 'Rule', {width: 'fill_container', height: 1.5, cornerRadius: 1, fill: i < at ? OK : '$--border'}, [])]));
    });
    return frame(id, 'Track', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'start', width}, cells);
  }
  const taskRef = (id, issue, pr) => row(id, [cap(`${id}-i`, issue, MUT, {mono: true}), ...(pr ? [icon(`${id}-g`, 'git-pull-request', {size: 11, fill: MUT}), cap(`${id}-p`, pr, MUT, {mono: true})] : [])], {gap: '$--spacing-xs', name: 'Refs'});
  // How many answers the Factory AI gave for this Task (B25); nothing at zero.
  const aiCount = (id, count) => (count ? row(id, [icon(`${id}-g`, 'sparkles', {size: 12, fill: MUT}), cap(`${id}-n`, String(count), SUB, {mono: true})], {gap: '$--spacing-xxs', name: `AI ${count}`}) : frame(id, 'No AI', {width: 1, height: 1}, []));

  // A 결정 필요 item (B22, B23, D-33): question or to-do, one shape.
  const CHOICE_MAX = 640;
  function choiceLine(id, c, width) {
    return frame(id, c.label, {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'start', width, padding: [7, 10], cornerRadius: '$--radius-sm',
      stroke: c.rec ? FG : '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', ...(c.rec ? {fill: '$--accent'} : {})}, [
      frame(`${id}-rw`, 'Radio', {width: 16, height: 18, layout: 'horizontal', alignItems: 'center'}, [radioDot(`${id}-r`, !!c.rec)]),
      col(`${id}-t`, [body(`${id}-l`, c.label, {weight: c.rec ? '600' : '400'}), cap(`${id}-x`, c.result, SUB, {width: width - 2 * 10 - 16 - 2 * SM - (c.rec ? 40 : 0)})], {gap: '$--spacing-xxs'}),
      spacer(`${id}-s`),
      ...(c.rec ? [cap(`${id}-rec`, '추천', MUT, {weight: '500'})] : []),
    ]);
  }
  function evidenceLines(id, ev, width, open) {
    if (!ev) return [];
    const head = row(`${id}-h`, [
      icon(`${id}-h-g`, open ? 'chevron-down' : 'chevron-right', {size: 12, fill: MUT}), cap(`${id}-h-t`, '근거', SUB, {weight: '500'}),
      ...(open ? [] : [cap(`${id}-h-x`, fitText(ev.summary, width - 60, 11), MUT)]),
    ], {gap: '$--spacing-xs', width: 'fill_container', name: '근거'});
    if (!open) return [head];
    return [head, col(`${id}-l`, ev.lines.map(([label, content], i) => row(`${id}-l${i}`, [cap(`${id}-l${i}-k`, label, MUT, {width: 92}), cap(`${id}-l${i}-v`, content, SUB, {width: width - 92 - SM - 16})], {gap: '$--spacing-sm', alignItems: 'start'})), {gap: 6, padding: [0, 0, 0, 16], name: 'Evidence'})];
  }
  function needItem(id, o, {width = INNER, open = false} = {}) {
    const PAD = 14;
    const INDENT = 26;
    const inner = width - 2 * PAD - INDENT;
    const lineW = Math.min(inner, CHOICE_MAX);
    const parts = [];
    if (o.kind === 'question') {
      parts.push(col(`${id}-ch`, o.choices.map((c, i) => choiceLine(`${id}-ch${i}`, c, lineW)), {gap: '$--spacing-xs', name: 'Choices'}));
      parts.push(field(`${id}-other`, lineW, {placeholder: '다른 답 쓰기'}));
      parts.push(row(`${id}-send`, [screenButton(`${id}-sb`, '이 답으로 보내기', {height: num(tokens, '--size-control'), icon: 'corner-down-left'}), cap(`${id}-dl`, o.deadline, MUT)], {gap: '$--spacing-md'}));
    } else {
      if (o.command) {
        parts.push(frame(`${id}-cmd`, 'Command', {layout: 'horizontal', gap: '$--spacing-sm', alignItems: 'center', width: lineW, height: 30, padding: [0, '$--spacing-xs', 0, 10], cornerRadius: '$--radius-sm', fill: '$--muted', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [
          body(`${id}-cmd-t`, o.command, {mono: true}), spacer(`${id}-cmd-s`), screenButton(`${id}-cmd-b`, '복사', {variant: 'ghost', height: controlSm, icon: 'copy'}),
        ]));
      }
      parts.push(row(`${id}-act`, [screenButton(`${id}-ab`, o.button, {height: num(tokens, '--size-control'), icon: o.buttonGlyph}), cap(`${id}-ar`, o.result, SUB)], {gap: '$--spacing-md'}));
    }
    parts.push(...evidenceLines(`${id}-ev`, o.evidence, inner, open && o.evidence?.lines));
    const ref = o.key ? taskOf(o.key).id : o.task;
    return frame(id, o.sentence, {layout: 'vertical', gap: 10, width, padding: [12, PAD], cornerRadius: '$--radius-md', fill: '$--background', stroke: WARN, strokeWidth: HAIR, strokeAlignment: 'inner'}, [
      row(`${id}-h`, [
        frame(`${id}-gw`, 'Glyph', {width: 18, height: 20, layout: 'horizontal', alignItems: 'center'}, [icon(`${id}-g`, o.glyph, {size: 14, fill: WARN})]),
        col(`${id}-st`, [
          text(`${id}-q`, o.sentence, {size: '$--text-subhead', weight: '600', width: width - 2 * PAD - 18 - SM - 200}),
          row(`${id}-stp`, [cap(`${id}-stp-k`, '멈춘 것', MUT, {weight: '500'}), cap(`${id}-stp-v`, o.stopped, SUB)], {gap: '$--spacing-sm'}),
        ], {gap: '$--spacing-xs'}),
        spacer(`${id}-hs`),
        ...(ref ? [cap(`${id}-ref`, ref, MUT, {mono: true})] : []),
        rightCap(`${id}-cue`, o.cue, MUT, 64),
      ], {width: 'fill_container', alignItems: 'start'}),
      col(`${id}-b`, parts, {gap: 10, padding: [0, 0, 0, INDENT], width: 'fill_container', name: 'Body'}),
    ]);
  }
  const needHead = (id, count) => row(id, [cap(`${id}-t`, '결정 필요', WARN, {weight: '600'}), cap(`${id}-n`, String(count), WARN, {mono: true, weight: '600'})], {gap: '$--spacing-xs', height: 24});

  // The 라인's table (B25, B26): one row per Task, the person's turn alone in the warning band.
  const LINE = [
    ['t420', 1, '답을 기다림 · Task 상세를 어떤 길로 낼지', 1],
    ['t405', 3, '검증 통과 · 위험 경로라 내 머지를 기다림', 0],
    ['t417', 2, 'web-e2e가 세 번 실패해 멈춤', 2],
    ['t412', 1, 'web-e2e 정렬 fixture를 고치고 다시 검증하는 중', 1],
    ['t415', 1, '저장은 끝, 하위 둘이 복원과 테스트를 쓰는 중', 0],
    ['t398', 2, '색인을 붙이고 PR #559의 CI를 기다림', 2],
    ['t426', 1, '사용량 한도로 쉬는 중 · 14:00에 다시 시작', 0],
    ['t430', 3, 'PR #566이 이 이슈를 닫는 중', 0],
    ['t7', 0, '접수 리뷰가 카드를 쓰는 중', 0],
    ['t431', 1, '작업자 자리를 기다림', 1],
    ['t421', 1, '#420을 기다림', 0],
    ['t422', 1, '#421을 기다림', 0],
    ['t410', 4, 'PR #558 머지', 0],
    ['t409', 4, 'PR #557 머지', 1],
  ];
  const FOLLOW_UPS = 2;
  const COL = {task: 380, track: 118, age: 84, ai: 44};
  const NOW_W = INNER - 2 * MD - 3 - COL.task - COL.track - COL.age - COL.ai - 4 * LG;
  function tableHead(id) {
    return row(id, [
      cap(`${id}-t`, 'Task', MUT, {weight: '500', width: COL.task}), cap(`${id}-k`, '단계', MUT, {weight: '500', width: COL.track}), cap(`${id}-n`, '지금', MUT, {weight: '500'}), spacer(`${id}-s`),
      rightCap(`${id}-a`, '경과', MUT, COL.age), rightCap(`${id}-ai`, 'AI', MUT, COL.ai),
    ], {width: 'fill_container', height: 28, padding: [0, MD, 0, MD + STRIPE], gap: '$--spacing-lg', name: 'Table head'});
  }
  function lineRow(id, [key, at, sentence, ai]) {
    const task = taskOf(key);
    const turn = task.wait === 'me';
    const waiting = task.lane === 'before' || task.state === 'resting' || task.state === 'outside';
    const tone = turn ? WARN : waiting ? MUT : WORK;
    const done = at >= 4;
    return frame(id, task.title, {layout: 'horizontal', width: 'fill_container', ...(done ? {opacity: DIMMED} : {})}, [
      frame(`${id}-band`, turn ? '사람 차례' : 'No band', {width: STRIPE, height: 'fill_container', ...(turn ? {fill: WARN} : {})}, []),
      row(`${id}-c`, [
        col(`${id}-t`, [
          text(`${id}-tt`, fitText(task.title, COL.task, 13), {size: '$--text-subhead', weight: '600'}),
          taskRef(`${id}-r`, task.id, task.pr ? `#${task.pr}` : null),
        ], {gap: '$--spacing-xxs', width: COL.task}),
        trackSmall(`${id}-k`, at, tone),
        text(`${id}-n`, fitText(sentence, NOW_W, 12), {size: '$--text-body', fill: turn ? WARN : SUB, width: NOW_W}),
        rightCap(`${id}-a`, task.age, turn ? WARN : MUT, COL.age),
        frame(`${id}-ai`, 'AI', {width: COL.ai, layout: 'horizontal', justifyContent: 'end'}, [aiCount(`${id}-ai-n`, ai)]),
      ], {width: 'fill_container', padding: [10, MD], gap: '$--spacing-lg'}),
    ]);
  }
  const lineTable = (id, rows) => col(id, [tableHead(`${id}-h`), rule(`${id}-r0`), ...rows.flatMap((r, i) => [r, rule(`${id}-r${i + 1}`)])], {gap: 0, width: 'fill_container', name: 'Table'});
  const followFold = (id, count) => row(id, [
    icon(`${id}-c`, 'chevron-right', {size: 12, fill: MUT}), icon(`${id}-g`, 'lightbulb', {size: 12, fill: MUT}), cap(`${id}-t`, '후속 후보', SUB, {weight: '500'}), cap(`${id}-n`, String(count), SUB, {mono: true}),
    cap(`${id}-x`, '작업 중 찾은 무관한 일 · 이슈로 만들기, Factory에 넣기, 버리기', MUT),
  ], {gap: '$--spacing-xs', height: 32, padding: [0, MD], name: '후속 후보'});
  const loopBody = (id, children) => col(id, children, {gap: '$--spacing-sm', width: MAIN, padding: ['$--spacing-sm', GUTTER, '$--spacing-lg', GUTTER], name: '라인'});

  function loopLineMain(id) {
    const items = NEEDS.filter(need => need.project === 'herdr-ide');
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical', clip: true}, [
      header(`${id}-hd`, {active: 0, count: items.length, scope: 'herdr-ide', pause: 'off'}),
      loopBody(`${id}-content`, [
        needHead(`${id}-nh`, items.length),
        ...items.map((need, i) => needItem(`${id}-n${i}`, need)),
        frame(`${id}-gap`, 'Gap', {width: 1, height: 6}, []),
        lineTable(`${id}-tb`, LINE.map((entry, i) => lineRow(`${id}-l${i}`, entry))),
        followFold(`${id}-fu`, FOLLOW_UPS),
      ]),
    ]);
  }
  // Every 결정 필요 kind at once (B5, B16, B20-B24, B33), under the header's three state marks;
  // the Task row a GitHub refusal holds reads 권한 기다림.
  function loopDecisionsMain(id) {
    const marks = [
      headMark(`${id}-mk0`, 'circle-x', 'main 깨짐', CRIT, {ringed: true}),
      headMark(`${id}-mk1`, 'sparkles', 'AI 판단 오늘 100/100 · 남은 결정은 내가', WARN),
      headMark(`${id}-mk2`, 'clock-3', 'GitHub 마지막으로 읽음 13:40', MUT, {dim: true}),
    ];
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical', clip: true}, [
      header(`${id}-hd`, {active: 0, count: NEEDS_ALL.length, scope: 'herdr-ide', pause: 'off', marks}),
      loopBody(`${id}-content`, [
        needHead(`${id}-nh`, NEEDS_ALL.length),
        ...NEEDS_ALL.map((need, i) => needItem(`${id}-n${i}`, need, {open: i === 0 || need === NEED_DISK})),
        frame(`${id}-gap`, 'Gap', {width: 1, height: 6}, []),
        lineTable(`${id}-tb`, [lineRow(`${id}-l0`, ['t398', 2, '권한 기다림 · GitHub에서 CI 결과를 읽지 못함', 2])]),
      ]),
    ]);
  }
  function loopEmptyMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [
      header(`${id}-hd`, {active: 0, count: 0, scope: 'herdr-ide', pause: 'off'}),
      row(`${id}-e`, [
        icon(`${id}-e-g`, 'tag', {size: 14, fill: MUT}), body(`${id}-e-t`, '라벨을 붙인 이슈가 여기에 줄로 나타납니다', {fill: SUB}),
        frame(`${id}-e-l`, 'Label', {layout: 'horizontal', alignItems: 'center', height: 20, padding: [0, '$--spacing-sm'], cornerRadius: 10, stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}, [cap(`${id}-e-lt`, 'factory', SUB, {mono: true})]),
      ], {gap: '$--spacing-sm', padding: ['$--spacing-md', GUTTER + MD]}),
    ]);
  }
  function loopReadingMain(id) {
    const bar = (bid, width, height = 10) => frame(bid, 'Skeleton', {width, height, cornerRadius: 3, fill: '$--secondary'}, []);
    const skeleton = (sid, i) => frame(sid, `Skeleton row ${i}`, {layout: 'horizontal', width: 'fill_container', gap: '$--spacing-lg', padding: [12, MD, 12, MD + STRIPE], alignItems: 'center'}, [
      col(`${sid}-t`, [bar(`${sid}-t0`, [300, 260, 320][i]), bar(`${sid}-t1`, 90, 8)], {gap: 6, width: COL.task}),
      col(`${sid}-k`, [frame(`${sid}-kc`, 'Cells', {layout: 'horizontal', gap: 2}, [0, 1, 2, 3].map(c => frame(`${sid}-kc${c}`, 'Cell', {width: 28, height: 4, cornerRadius: 2, fill: '$--secondary'}, []))), bar(`${sid}-kw`, 28, 8)], {gap: 6, width: COL.track}),
      bar(`${sid}-n`, [420, 360, 300][i]), spacer(`${sid}-s`), bar(`${sid}-a`, 48, 8),
    ]);
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [
      header(`${id}-hd`, {active: 0, count: 0, scope: 'herdr-ide', pause: 'off', read: null}),
      loopBody(`${id}-content`, [lineTable(`${id}-tb`, [0, 1, 2].map(i => skeleton(`${id}-sk${i}`, i)))]),
    ]);
  }
  function loopReadFailMain(id) {
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical'}, [
      col(`${id}-p`, [
        row(`${id}-nav`, [screenButton(`${id}-back`, '라인', {variant: 'ghost', height: controlSm, icon: 'arrow-left'}), spacer(`${id}-ns`)], {width: 'fill_container'}),
        row(`${id}-f`, [icon(`${id}-f-g`, 'circle-alert', {size: 14, fill: MUT}), body(`${id}-f-t`, '이 Task를 읽지 못했습니다', {fill: SUB}), screenButton(`${id}-f-b`, '다시 읽기', {variant: 'outline', height: controlSm, icon: 'refresh-cw'})], {gap: '$--spacing-sm', padding: ['$--spacing-xl', 0]}),
      ], {gap: '$--spacing-lg', width: MAIN, padding: [14, GUTTER, '$--spacing-xl', GUTTER]}),
    ]);
  }

  // The Task page (B27-B30, D-35) of #398 while it verifies: title and the state sentence, the track
  // with who decided where, summary, the criteria checklist, decisions split 나 / AI, the activity
  // timeline, the original issue (opened, rendered) and this Task's 후속 후보.
  const T398_PAGE = {
    state: 'PR #559의 CI 검증을 기다립니다',
    meta: '· 398-session-search · Claude Code · sonnet · high',
    summary: '세션 검색을 SQLite 전문 색인으로 바꿔, 세션 1만 개에서도 검색어를 친 뒤 1초 안에 결과를 보인다. 색인은 세션을 읽을 때 함께 갱신하고, ⌘K 검색도 같은 색인을 쓴다.',
    criteria: [
      ['충족', '세션 1만 개에서 검색 결과가 1초 안에 보인다'],
      ['충족', '새 세션이 30초 안에 색인에 들어간다'],
      ['판단 불가', '한국어 검색어가 조사 앞에서도 맞는다', '테스트 기록에 영어 검색어만 있음'],
      ['충족', '색인이 깨지면 처음부터 다시 만든다'],
      ['충족', '검색 결과의 순서가 지금과 같다'],
      ['미충족', '색인 파일이 200 MB를 넘지 않는다', '세션 1만 개 기록에서 240 MB'],
    ],
    mine: [{question: '카드는 Sessions 검색만 말하는데, 바뀐 코드는 ⌘K 검색도 같은 색인으로 답합니다. ⌘K도 이 Task에 넣을까요?', answer: '⌘K도 넣고 카드 목표에 한 줄 더한다 (AI 제안)', kind: '카드 고침 · 완료 후 검사', at: '11:20'}],
    ai: [
      {question: '색인을 무엇으로 만들까?', answer: 'SQLite FTS5 전문 색인을 쓴다', why: '세션 저장에 이미 SQLite를 쓰고 새 의존성이 필요 없음', kind: '가정 · 접수 리뷰', at: '09:01'},
      {question: '작업자 보고에 "큰 기록으로 시간을 재지 않음"이 있다. 이대로 검증으로 넘길까?', answer: '되돌림: 세션 1만 개 기록으로 검색 시간을 재고 다시 보고한다', why: '완료 기준 1이 세션 1만 개에서 1초를 요구함', kind: '되돌림 · 완료 후 검사', at: '11:20',
        open: '작은 기록으로 잰 결과로 충분하다'},
    ],
    issue: [
      ['h', 'What happened'],
      ['p', '세션이 많아지면 Sessions 검색이 검색어를 칠 때마다 몇 초씩 멈춥니다. 세션 8천 개인 기기에서 한 글자마다 2~3초가 걸렸습니다.'],
      ['h', 'What you expected'],
      ['p', '세션이 많아도 검색어를 친 뒤 1초 안에 결과가 보입니다. 결과의 순서는 지금과 같습니다.'],
      ['h', 'Where the authority for it lives'],
      ['p', 'hide-session의 세션 읽기와 web의 Sessions 검색입니다. 세션 파일 자체는 바뀌지 않습니다.'],
    ],
    followUp: '색인이 커지면 오래된 세션을 덜어 낼 방법이 없습니다. 세션 1만 개 기록에서 색인이 240 MB였습니다.',
  };
  const CRIT_MARK = {충족: ['circle-check', OK], 미충족: ['circle-x', CRIT], '판단 불가': ['circle-help', MUT]};
  const section = (id, label, note) => row(id, [text(`${id}-t`, label, {size: '$--text-subhead', weight: '600'}), ...(note ? [cap(`${id}-n`, note, MUT)] : [])], {gap: '$--spacing-sm'});
  function criteriaList(id, width) {
    return col(id, T398_PAGE.criteria.map(([state, content, why], i) => {
      const [glyph, fill] = CRIT_MARK[state];
      return row(`${id}-${i}`, [
        icon(`${id}-${i}-g`, glyph, {size: 14, fill}),
        col(`${id}-${i}-t`, [body(`${id}-${i}-c`, content, {width: width - 14 - 72 - 2 * SM}), ...(why ? [cap(`${id}-${i}-w`, why, MUT)] : [])], {gap: '$--spacing-xxs'}),
        text(`${id}-${i}-s`, state, {size: '$--text-caption', fill, weight: '500', width: 72, align: 'right'}),
      ], {width, alignItems: 'start'});
    }), {gap: 10, name: '완료 기준'});
  }
  function decisionLine(id, o, width, ai) {
    const textW = width - 18 - 280;
    return col(id, [
      row(`${id}-r`, [
        frame(`${id}-gw`, 'Glyph', {width: 13, height: 17, layout: 'horizontal', alignItems: 'center'}, [icon(`${id}-g`, ai ? 'sparkles' : 'user', {size: 13, fill: ai ? MUT : WARN})]),
        col(`${id}-t`, [
          cap(`${id}-q`, o.question, MUT, {width: textW}),
          text(`${id}-a`, o.answer, {size: '$--text-body', weight: '500', width: textW}),
          ...(o.why ? [cap(`${id}-y`, `이유: ${o.why}`, SUB, {width: textW})] : []),
        ], {gap: 3}),
        spacer(`${id}-s`), cap(`${id}-k`, o.kind, MUT),
        ...(ai ? [screenButton(`${id}-b`, '다른 답', {variant: 'ghost', height: controlSm})] : []),
        rightCap(`${id}-at`, o.at, MUT, 40),
      ], {width, alignItems: 'start'}),
      ...(o.open ? [col(`${id}-o`, [
        row(`${id}-of`, [field(`${id}-oi`, 520, {value: o.open}), screenButton(`${id}-ob`, '이 답으로 바꾸기', {variant: 'outline', height: controlSm})], {gap: '$--spacing-sm'}),
        cap(`${id}-on`, '바꾸면 내가 정한 것으로 옮기고, 진행 중인 작업자에게 편지로 알립니다', MUT),
      ], {gap: '$--spacing-xs', padding: [0, 0, 0, 26], name: '다른 답'})] : []),
    ], {gap: '$--spacing-sm', name: o.answer.slice(0, 28)});
  }
  function report(id, rows, width) {
    return col(id, [
      ...rows.map(([label, content], i) => row(`${id}-${i}`, [cap(`${id}-${i}-k`, label, MUT, {weight: '500', width: 84}), body(`${id}-${i}-v`, content, {width: width - 84 - SM, fill: label === '확인 못 한 것' ? WARN : FG})], {gap: '$--spacing-sm', alignItems: 'start'})),
      row(`${id}-raw`, [screenButton(`${id}-raw-b`, '원문 펼치기', {variant: 'ghost', height: controlSm, icon: 'chevron-down'})]),
    ], {gap: 6, width, name: '보고'});
  }
  function event(id, at, glyph, fill, title, extra = []) {
    return row(id, [
      cap(`${id}-at`, at, MUT, {mono: true, width: 40}),
      frame(`${id}-gw`, 'Glyph', {width: 13, height: 17, layout: 'horizontal', alignItems: 'center'}, [icon(`${id}-g`, glyph, {size: 13, fill})]),
      col(`${id}-c`, [body(`${id}-t`, title), ...extra], {gap: 6}),
    ], {gap: '$--spacing-sm', alignItems: 'start', width: 'fill_container', name: title.slice(0, 28)});
  }
  function timeline(id, width) {
    const w = width - 40 - 13 - 2 * SM;
    return col(id, [
      event(`${id}-0`, '09:01', 'tag', MUT, 'factory 라벨 · 접수 리뷰가 카드를 채움', [cap(`${id}-0-x`, '완료 기준 6개, 가정 1개 · 이슈 본문은 그대로')]),
      event(`${id}-1`, '09:14', 'wrench', MUT, '자동 복구 · 끝난 worktree 정리', [cap(`${id}-1-x`, '디스크 여유 3.8GB로 기준보다 작음 → 나아짐 · 6.2GB 확보')]),
      event(`${id}-2`, '09:15', 'play', WORK, '작업자 시작 · Claude Code · sonnet · high'),
      event(`${id}-3`, '11:08', 'file-text', FG, '보고', [report(`${id}-3-r`, [
        ['결과', '세션 검색이 SQLite 전문 색인으로 답합니다'],
        ['바뀐 것', '세션을 읽을 때 색인을 갱신하고, 검색은 색인에만 묻습니다. ⌘K 검색도 같은 색인을 씁니다'],
        ['확인한 것', 'cargo 테스트, web e2e 검색 spec'],
        ['확인 못 한 것', '세션 1만 개 기록에서의 검색 시간'],
      ], w)]),
      event(`${id}-4`, '11:08', 'git-pull-request', OK, 'PR #559 열림'),
      event(`${id}-5`, '11:20', 'undo-2', MUT, '완료 후 검사가 작업자에게 되돌림 · 세션 1만 개 기록으로 시간 재기'),
      event(`${id}-6`, '11:31', 'circle-check', OK, 'CI verify 통과'),
      event(`${id}-7`, '12:41', 'file-text', FG, '보고', [report(`${id}-7-r`, [
        ['결과', '세션 1만 개에서 검색 결과가 0.4초 안에 보입니다'],
        ['바뀐 것', '코드는 앞 보고와 같습니다. 1만 개 기록을 만드는 측정 스크립트를 더했습니다'],
        ['확인한 것', '세션 1만 개, 검색어 20개: 중간값 0.18초, 가장 느린 것 0.41초 · 색인 240 MB'],
        ['확인 못 한 것', '한국어 검색어의 조사 처리'],
      ], w)]),
      event(`${id}-8`, '12:41', 'lightbulb', MUT, '후속 후보 1개 · 색인이 커지면 오래된 세션을 덜어 낼 방법이 없음'),
      event(`${id}-9`, '12:42', 'loader-circle', WORK, '검증 · 시도 1 · 실패 0/3'),
    ], {gap: '$--spacing-md', width, name: '활동'});
  }
  function loopTaskMain(id) {
    const task = TASKS.t398;
    const TEXT_W = 760;
    const issueW = TEXT_W;
    return frame(`${id}-main`, 'Main', {width: MAIN, height: 'fill_container', layout: 'vertical', clip: true}, [
      col(`${id}-p`, [
        row(`${id}-nav`, [screenButton(`${id}-back`, '라인', {variant: 'ghost', height: controlSm, icon: 'arrow-left'}), spacer(`${id}-ns`)], {width: 'fill_container'}),
        col(`${id}-title`, [
          row(`${id}-tr`, [
            text(`${id}-t`, task.title, {size: '$--text-headline', weight: '600'}),
            row(`${id}-chip`, [icon(`${id}-chip-g`, STATE_GLYPH.verifying, {size: 12, fill: WORK}), cap(`${id}-chip-t`, STATE_WORD.verifying, WORK)], {gap: '$--spacing-xs', height: 24, padding: [0, '$--spacing-md'], cornerRadius: 12, fill: '$--muted'}),
            spacer(`${id}-ts`), screenButton(`${id}-cancel`, '취소', {variant: 'ghost', height: controlSm}),
          ], {gap: '$--spacing-md', width: 'fill_container'}),
          body(`${id}-state`, T398_PAGE.state, {fill: SUB}),
          row(`${id}-meta`, [taskRef(`${id}-ref`, task.id, `#${task.pr}`), cap(`${id}-meta-t`, T398_PAGE.meta), spacer(`${id}-meta-s`), screenButton(`${id}-worker`, '작업자 보기', {variant: 'outline', height: controlSm})], {gap: '$--spacing-xs', width: 'fill_container'}),
        ], {gap: 6, width: 'fill_container'}),
        trackWide(`${id}-track`, 2, WORK, {
          0: [['ai', '가정 1']],
          1: [['ai', '되돌림 1'], ['me', '카드 고침 1']],
          2: [['note', '시도 1 · 실패 0/3']],
          3: [['note', '검증을 통과하면 자동 머지']],
        }, 720),
        rule(`${id}-rule`),
        col(`${id}-sum`, [section(`${id}-sum-h`, '요약'), body(`${id}-sum-t`, T398_PAGE.summary, {width: TEXT_W})]),
        col(`${id}-cr`, [section(`${id}-cr-h`, '완료 기준', '충족 4 · 미충족 1 · 판단 불가 1'), criteriaList(`${id}-cr-l`, TEXT_W)]),
        col(`${id}-dc`, [
          section(`${id}-dc-h`, '결정'),
          cap(`${id}-dc-me`, `내가 정한 것 ${T398_PAGE.mine.length}`, SUB, {weight: '600'}),
          ...T398_PAGE.mine.map((o, i) => decisionLine(`${id}-dm${i}`, o, INNER, false)),
          frame(`${id}-dc-gap`, 'Gap', {width: 1, height: 4}, []),
          cap(`${id}-dc-ai`, `AI가 정한 것 ${T398_PAGE.ai.length}`, SUB, {weight: '600'}),
          ...T398_PAGE.ai.map((o, i) => decisionLine(`${id}-da${i}`, o, INNER, true)),
        ], {gap: 10}),
        col(`${id}-tl`, [section(`${id}-tl-h`, '활동'), timeline(`${id}-tl-l`, INNER)], {gap: '$--spacing-md'}),
        col(`${id}-is`, [
          row(`${id}-is-h`, [icon(`${id}-is-g`, 'chevron-down', {size: 14, fill: MUT}), text(`${id}-is-t`, '원본 이슈', {size: '$--text-subhead', weight: '600'}), cap(`${id}-is-n`, task.id, MUT, {mono: true})], {gap: '$--spacing-xs'}),
          col(`${id}-is-b`, T398_PAGE.issue.map(([kind, content], i) => (kind === 'h'
            ? text(`${id}-is-${i}`, content, {size: '$--text-title', weight: '600', width: issueW})
            : text(`${id}-is-${i}`, content, {size: '$--text-body', fill: SUB, width: issueW}))),
          {gap: '$--spacing-sm', padding: ['$--spacing-md', '$--spacing-lg'], cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', name: 'Issue'}),
        ]),
        col(`${id}-fu`, [
          section(`${id}-fu-h`, '이 Task의 후속 후보', '1'),
          row(`${id}-fu-r`, [
            frame(`${id}-fu-gw`, 'Glyph', {width: 13, height: 17, layout: 'horizontal', alignItems: 'center'}, [icon(`${id}-fu-g`, 'lightbulb', {size: 13, fill: MUT})]),
            col(`${id}-fu-t`, [body(`${id}-fu-c`, T398_PAGE.followUp, {width: INNER - 13 - 380}), cap(`${id}-fu-w`, `${task.id} · 12:41`)], {gap: '$--spacing-xxs'}),
            spacer(`${id}-fu-s`),
            screenButton(`${id}-fu-b0`, '이슈로 만들기', {variant: 'outline', height: controlSm}), screenButton(`${id}-fu-b1`, 'Factory에 넣기', {variant: 'outline', height: controlSm}), screenButton(`${id}-fu-b2`, '버리기', {variant: 'ghost', height: controlSm}),
          ], {width: INNER, alignItems: 'start'}),
        ]),
      ], {gap: '$--spacing-xl', width: MAIN, padding: [14, GUTTER, '$--spacing-xl', GUTTER]}),
    ]);
  }

  const id = name => `fx-${name}-${s}`;
  // The sidebar when herdr-ide's 결정 필요 holds `count` items instead of the example data's.
  const sideNeeds = count => sideFactories('herdr-ide').map(f => (f.project === 'herdr-ide' ? {...f, count} : f));
  const board = windowFrame(id('board'), '보드', boardMain(id('board')), {height: BOARD_H, count: needCount('herdr-ide')});
  const sizes = sizesBody(id('sizes'));
  const graph = windowFrame(id('graph'), '그래프', graphMain(id('graph')));
  const none = windowFrame(id('none'), 'Factory 없음', noFactoryMain(id('none')), {height: 360, count: 0, secretary: false});
  const loopLine = windowFrame(id('loop-line'), '라인', loopLineMain(id('loop-line')), {height: LOOP_LINE_H, count: needCount(), factories: sideFactories('herdr-ide')});
  const loopDecisions = windowFrame(id('loop-decisions'), '결정 필요 · 모든 종류', loopDecisionsMain(id('loop-decisions')), {height: LOOP_DECISIONS_H, count: NEEDS_ALL.length + needCount('sasu'), factories: sideNeeds(NEEDS_ALL.length)});
  const loopTask = windowFrame(id('loop-task'), 'Task 페이지', loopTaskMain(id('loop-task')), {height: LOOP_TASK_H, count: needCount(), factories: sideFactories('herdr-ide')});
  const loopEmpty = windowFrame(id('loop-empty'), '라인 · Task 없음', loopEmptyMain(id('loop-empty')), {height: 300, count: needCount('sasu'), factories: sideNeeds(0)});
  const loopReading = windowFrame(id('loop-reading'), '라인 · 처음 읽는 중', loopReadingMain(id('loop-reading')), {height: 370, count: needCount('sasu'), factories: sideNeeds(0)});
  const loopReadFail = windowFrame(id('loop-readfail'), 'Task 페이지 · 읽기 실패', loopReadFailMain(id('loop-readfail')), {height: 260, count: needCount(), factories: sideFactories('herdr-ide')});
  const obsSettings = windowFrame(id('obs-set'), '설정 · herdr-ide', settingsMain(id('obs-set')), {height: 1160, count: needCount(), factories: sideFactories('herdr-ide')});
  const obsCards = obsCardsBody(id('obs-cards'));
  const obsOff = windowFrame(id('obs-off'), '설정 · Hide AI 꺼짐', settingsMain(id('obs-off'), {aiOff: true}), {height: 1100, count: needCount(), factories: sideFactories('herdr-ide')});
  const obsAll = windowFrame(id('obs-all'), '설정 · 모든 프로젝트', settingsMain(id('obs-all'), {project: null}), {height: 640, count: needCount(), factories: sideFactories(null)});
  const obsDirect = windowFrame(id('obs-direct'), '설정 · 직접 · 고급 설정', settingsMain(id('obs-direct'), {mode: 0, workers: WORKERS.slice(0, 1), advanced: true}), {height: 2290, count: needCount(), factories: sideFactories('herdr-ide')});
  const obsAuto = windowFrame(id('obs-auto'), '설정 · 맡김 · 후보 다섯', settingsMain(id('obs-auto'), {mode: 2, workers: WORKERS_FULL, used: 100}), {height: 1340, count: needCount(), factories: sideFactories('herdr-ide')});
  const obsPick = windowFrame(id('obs-pick'), 'Task 페이지 · 작업자 고르기', obsPickMain(id('obs-pick')), {count: needCount(), factories: sideFactories('herdr-ide')});

  return [
    col(id('frames'), [
      row(id('r1'), [
        captioned(id('loop-line'), '라인: 맨 위 결정 필요(사람이 움직일 것만), 그 아래 Task마다 한 줄(제목, 이슈와 PR, 접수 · 작업 · 검증 · 머지의 지금 칸, 지금 하는 일 한 문장, 경과, AI가 정한 것의 수), 사람 차례인 줄만 주황 띠, 맨 아래 접힌 후속 후보', loopLine),
        captioned(id('loop-decisions'), '결정 필요의 모든 종류: 질문(근거 펼침), 위험 경로 머지, 검증 상한으로 멈춤, Hide AI 꺼짐, 복구를 다 한 뒤의 할 일(근거 펼침), 사람이 할 명령, GitHub 재로그인. 머리줄에 main 깨짐, 하루 상한, 흐린 마지막 읽은 시각', loopDecisions),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
      row(id('r2'), [
        captioned(id('loop-task'), 'Task 페이지: 제목과 지금 상태, 누가 어디서 정했는지 표시한 네 칸 트랙, 요약, 완료 기준, 결정(내가 정한 것 / AI가 정한 것, 다른 답 하나 펼침), 활동, 원본 이슈(펼침), 이 Task의 후속 후보', loopTask),
        col(id('r2s'), [
          captioned(id('loop-empty'), '라인 · Task가 없을 때: 라벨을 붙이면 줄이 생긴다는 한 줄과 라벨 이름', loopEmpty),
          captioned(id('loop-reading'), '라인 · 처음 읽는 중: 줄 자리에 흐린 틀', loopReading),
          captioned(id('loop-readfail'), 'Task 페이지를 읽지 못했을 때: 한 줄과 다시 읽기', loopReadFail),
          captioned(id('none'), 'Factory가 없을 때: Factory 만들기만 보인다', none),
        ], {gap: '$--spacing-xl'}),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
      row(id('r3'), [
        captioned(id('board'), '보드: 움직임으로 네 열, 멈춤은 나를 기다림과 다른 걸 기다림으로 나누고, 시작 전과 완료는 좁아 카드가 작게 그려진다', board),
        captioned(id('graph'), '그래프: 층으로 놓고 중복 화살표(#420 → #422)는 그리지 않는다', graph),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
      row(id('r4'), [
        captioned(id('sizes'), '같은 카드, 세 크기: 좁을수록 덜 중요한 것부터 빠진다. 색은 상태 아이콘, 문제 줄, 왼쪽 띠에만', sizes),
      ], {alignItems: 'start'}),
      row(id('r5'), [
        scrim(id('create'), 'Factory 만들기: 감지 중 · 필수 체크 · verify 명령 후보 · 검증이 없을 때(auto 불가, manual)', [
          createDialog(id('cd0'), 'detecting'), createDialog(id('cd1'), 'ci'), createDialog(id('cd2'), 'commands'), createDialog(id('cd3'), 'none'),
        ]),
      ], {alignItems: 'start'}),
      row(id('r6'), [
        captioned(id('obs-set'), 'Observer · 설정: 헤더와 사이드바가 고른 프로젝트 하나. 세 칸에서 고르면 나에게 오는 것과 AI가 하는 것이 보이고, 작업자는 후보 중 Factory AI가 Task마다 고른다', obsSettings),
        captioned(id('obs-off'), 'Observer · Hide AI가 꺼졌을 때: 세 칸은 흐려지고 모든 결정이 나에게 온다', obsOff),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
      row(id('r7'), [
        captioned(id('obs-cards'), 'Observer · 카드: 선택지가 있는 결정 요청, AI 제안, 보고 없음과 진단, 작업자 사라짐, 일시정지', obsCards),
      ], {alignItems: 'start'}),
      row(id('r8'), [
        captioned(id('obs-all'), 'Observer · 모든 프로젝트의 설정: Factory마다 한 줄, 누르면 그 설정으로. 이 Mac 전체 숫자는 여기에만', obsAll),
        captioned(id('obs-pick'), 'Observer · 시작 전 Task에서 작업자 고르기: Factory AI가 고른 후보에 체크와 이유, 다른 후보를 고르면 그 후보가 쓰인다', obsPick),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
      row(id('r9'), [
        captioned(id('obs-direct'), 'Observer · 직접: 나에게 오는 것이 가장 많다. 후보 하나(기본, 뺄 수 없음), 고급 설정을 펼친 모습', obsDirect),
        captioned(id('obs-auto'), 'Observer · 맡김: 권한만 나에게 온다. 오늘 상한에 닿음, 후보 다섯(더 추가 못함), 선언 없는 에이전트는 CLI 기본값', obsAuto),
      ], {alignItems: 'start', gap: '$--spacing-xl'}),
    ], {gap: '$--spacing-xl'}),
  ];
}
