// The device Overview (PRD overview-device-tasks D-01..D-38): one device's
// Tasks, 백로그 and Projects under a title and the device switch, the panel
// beside the tabs, the device that does not answer, and the ⌘⇧M quick add.
// Controls are refs of the library's System masters supplied by pen-screens;
// the rows and cards are drawn here, as web/src/TasksTab.tsx, BacklogTab.tsx
// and ProjectsTab.tsx draw them from primitives. Pen has no dashed stroke, so
// the incoming card's dashed edge is drawn as a muted solid one.
import {num} from './pen-system.mjs';

export function overviewScreens(tokens, {frame, text, icon, screenButton, screenIconButton, screenInput, screenToggleGroup, screenPrMark, screenStatusMark}) {
  const FG = '$--foreground', SUBTLE = '$--subtle-foreground', MUTED = '$--muted-foreground';
  const HAIR = num(tokens, '--size-hairline');
  const XXS = num(tokens, '--spacing-xxs'), XS = num(tokens, '--spacing-xs'), SM = num(tokens, '--spacing-sm'), MD = num(tokens, '--spacing-md'), LG = num(tokens, '--spacing-lg');
  const ICON = num(tokens, '--size-icon'), ICON_SM = num(tokens, '--size-icon-sm'), ICON_LG = num(tokens, '--size-icon-lg');
  const DOT = num(tokens, '--size-tab-status-dot');
  const CARD_W = num(tokens, '--size-panel-min');
  const PANEL_W = num(tokens, '--size-lane-min');
  // StageBar's four steps (web/src/TasksTab.tsx).
  const STAGE_W = 32;
  const spacer = id => frame(id, 'Spacer', {width: 'fill_container', height: 1}, []);
  const rule = (id, width = 'fill_container') => frame(id, 'Rule', {width, height: HAIR, fill: '$--border'}, []);
  const caption = (id, content, fill = SUBTLE, extra = {}) => text(id, content, {size: '$--text-caption', fill, ...extra});
  const micro = (id, content, fill = MUTED, extra = {}) => text(id, content, {size: '$--text-micro', fill, ...extra});
  const row = (id, name, children, extra = {}) => frame(id, name, {layout: 'horizontal', gap: XS, alignItems: 'center', ...extra}, children);
  const col = (id, name, children, extra = {}) => frame(id, name, {layout: 'vertical', gap: XXS, ...extra}, children);

  // -- the head: title, device switch, Esc (Overview.tsx OverviewPage) ----------------------
  function deviceChoice(id, {label, own = false, selected = false, rail = false, offline = false, needs = false}) {
    const tile = frame(`${id}-tile`, 'Tile', {width: ICON_LG, height: ICON_LG, layout: 'horizontal', justifyContent: 'center', alignItems: 'center', fill: '$--secondary', cornerRadius: '$--radius-xs', ...(offline ? {opacity: 0.5} : {})}, [
      own ? icon(`${id}-g`, 'laptop', {size: ICON, fill: FG}) : micro(`${id}-m`, label.slice(0, 1).toUpperCase(), FG, {weight: '600'}),
    ]);
    return row(id, label, [
      needs ? frame(`${id}-tw`, 'Tile with dot', {layout: 'none', width: ICON_LG + 2, height: ICON_LG + 2}, [
        {...tile, x: 0, y: 2},
        {...frame(`${id}-dot`, 'Needs You dot', {width: DOT, height: DOT, fill: '$--warning', cornerRadius: DOT}, []), x: ICON_LG + 2 - DOT, y: 0},
      ]) : tile,
      caption(`${id}-t`, label, selected ? FG : SUBTLE, {weight: '500'}),
      ...(rail ? [frame(`${id}-rail`, 'rail', {padding: [0, XXS], fill: '$--muted', cornerRadius: '$--radius-xs'}, [micro(`${id}-rt`, 'rail')])] : []),
      ...(offline ? [micro(`${id}-off`, '연결 안 됨')] : []),
    ], {padding: [XXS, SM], cornerRadius: '$--radius-sm', ...(selected ? {fill: '$--secondary'} : {})});
  }
  function head(id, {selected = 'mac'} = {}) {
    return row(id, 'Head', [
      text(`${id}-title`, '개요', {size: '$--text-title', weight: '600'}),
      row(`${id}-devices`, 'Device switch', [
        deviceChoice(`${id}-mac`, {label: 'This Mac', own: true, rail: true, selected: selected === 'mac'}),
        deviceChoice(`${id}-mini`, {label: 'mini', needs: true, selected: selected === 'mini'}),
        deviceChoice(`${id}-studio`, {label: 'studio', offline: true, selected: selected === 'studio'}),
      ], {gap: XXS, padding: XXS, cornerRadius: '$--radius-md', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'}),
      spacer(`${id}-sp`),
      screenButton(`${id}-esc`, 'Esc', {variant: 'ghost'}),
    ], {width: 'fill_container', gap: MD, padding: [SM, LG], stroke: '$--border', strokeWidth: {bottom: HAIR}, strokeAlignment: 'inner'});
  }

  // -- the tabs with counts, and each tab's controls (MainScreen.tsx) ------------------------
  function tab(id, label, count, active, extra = []) {
    return row(id, label, [
      caption(`${id}-t`, label, active ? FG : SUBTLE, {weight: '500'}),
      ...(count === null ? [] : [caption(`${id}-n`, count, MUTED, {mono: true})]),
      ...extra,
    ], {height: num(tokens, '--size-control-sm'), padding: [0, SM], cornerRadius: '$--radius-sm', ...(active ? {fill: '$--background'} : {})});
  }
  // Tasks' count, its Needs You and the thin bar of where the tasks stand (TasksCount, B3).
  function tasksTab(id, active) {
    const bar = frame(`${id}-bar`, 'Tasks bar', {layout: 'horizontal', gap: 1, width: 28, height: HAIR, cornerRadius: HAIR, clip: true}, [
      ['needs', 2, '$--warning'], ['working', 3, '$--agent-working'], ['waiting', 1, '$--success'], ['resting', 1, MUTED],
    ].map(([key, value, fill]) => frame(`${id}-bar-${key}`, key, {width: value * 4 - 1, height: HAIR, fill}, [])));
    return tab(id, '태스크', '7', active, [caption(`${id}-ny`, '2', '$--warning', {mono: true}), bar]);
  }
  function tabs(id, active, controls) {
    return row(id, 'Tabs and controls', [
      row(`${id}-list`, 'Tabs', [
        tasksTab(`${id}-tasks`, active === 'tasks'),
        tab(`${id}-backlog`, '백로그', '5', active === 'backlog'),
        tab(`${id}-projects`, '프로젝트', '4', active === 'projects'),
        frame(`${id}-gap`, 'Gap', {width: MD, height: 1}, []),
        tab(`${id}-usage`, '사용량', null, false),
      ], {gap: XXS, padding: XXS, fill: '$--muted', cornerRadius: '$--radius-md'}),
      spacer(`${id}-sp`),
      ...controls,
    ], {width: 'fill_container', gap: MD, padding: [XS, LG], stroke: '$--border', strokeWidth: {bottom: HAIR}, strokeAlignment: 'inner'});
  }
  const projectChip = id => screenButton(id, '모든 프로젝트  ▾', {variant: 'outline'});
  const search = (id, placeholder) => screenInput(id, {placeholder, width: CARD_W});
  const taskControls = id => [screenToggleGroup(`${id}-chips`, ['전체', '내 차례 2', '일하는 중'], 0), projectChip(`${id}-project`), search(`${id}-search`, '일감, 브랜치, #PR, 이슈 검색')];
  const backlogControls = id => [screenButton(`${id}-labels`, '라벨  ▾', {variant: 'outline'}), projectChip(`${id}-project`), search(`${id}-search`, '이슈, 메모 검색')];

  // -- Tasks (TasksTab.tsx) -----------------------------------------------------------------
  const LINK_CHIP = (id, label, glyphs = [], fill = SUBTLE) => row(id, label, [...glyphs.map(([g, at]) => at === 'lead' ? icon(`${id}-g`, g, {size: ICON_SM, fill}) : null).filter(Boolean), caption(`${id}-t`, label, fill), ...glyphs.map(([g, at]) => at === 'tail' ? icon(`${id}-gt`, g, {size: ICON_SM, fill}) : null).filter(Boolean)], {gap: XXS, padding: [0, XXS], cornerRadius: '$--radius-xs'});
  function stageBar(id, stage, fill) {
    return frame(id, `Stage ${stage}`, {layout: 'horizontal', gap: 1, width: STAGE_W}, [1, 2, 3, 4].map(step => frame(`${id}-${step}`, `Step ${step}`, {width: (STAGE_W - 3) / 4, height: HAIR, cornerRadius: HAIR, fill: step <= stage ? fill : '$--muted'}, [])));
  }
  function card(id, {mark, markFill, title, age, question, children, prs = [], items = [], outProject, outDevice, stage, stageFill, line, lineFill = SUBTLE, needs = false, selected = false}) {
    const chips = [
      ...(children ? [row(`${id}-kids`, 'Children', [...children.map(([symbol, fill], index) => screenStatusMark(tokens, `${id}-kid${index}`, symbol, fill)), caption(`${id}-kidt`, `자식 ${children.length}`, MUTED)], {gap: XXS})] : []),
      ...prs.map((pr, index) => screenPrMark(`${id}-pr${index}`, pr)),
      ...items.map((item, index) => LINK_CHIP(`${id}-item${index}`, item, [[item.startsWith('L-') ? 'file-text' : 'circle-dot', 'lead']])),
      ...(outProject ? [LINK_CHIP(`${id}-outp`, outProject, [['arrow-right', 'lead']])] : []),
      ...(outDevice ? [LINK_CHIP(`${id}-outd`, outDevice, [['arrow-right', 'lead']])] : []),
    ];
    return col(id, title, [
      row(`${id}-top`, 'Title row', [
        screenStatusMark(tokens, `${id}-mark`, mark, markFill),
        caption(`${id}-title`, title, FG, {weight: '500'}),
        spacer(`${id}-sp`),
        micro(`${id}-age`, age, MUTED, {mono: true}),
      ], {width: 'fill_container'}),
      ...(question ? [caption(`${id}-q`, question, '$--warning', {width: CARD_W - 2 * SM})] : []),
      ...(chips.length ? [row(`${id}-chips`, 'Chips', chips, {gap: XS})] : []),
      row(`${id}-line`, 'Line', [...(stage ? [stageBar(`${id}-stage`, stage, stageFill)] : []), caption(`${id}-lt`, line, lineFill)], {gap: SM}),
    ], {width: CARD_W, padding: [XS, SM], fill: '$--card', cornerRadius: '$--radius-md', stroke: needs ? '$--warning' : selected ? '$--ring' : '$--border', strokeWidth: selected ? 2 : HAIR, strokeAlignment: 'inner'});
  }
  // Work a root on another device delegated here (B14): never counted, its child's mark and title.
  function incoming(id, {from, children}) {
    return col(id, 'Incoming', [
      caption(`${id}-from`, `← ${from}에서 맡긴 일`, MUTED),
      ...children.map(([symbol, fill, title], index) => row(`${id}-c${index}`, title, [screenStatusMark(tokens, `${id}-c${index}-m`, symbol, fill), caption(`${id}-c${index}-t`, title, FG)])),
    ], {width: CARD_W, padding: [XS, SM], cornerRadius: '$--radius-md', stroke: MUTED, strokeWidth: HAIR, strokeAlignment: 'inner'});
  }
  const foldChip = (id, label, fill) => row(id, label, [caption(`${id}-t`, label, fill), icon(`${id}-g`, 'chevron-right', {size: ICON_SM, fill})], {gap: XXS, padding: [XXS, SM], cornerRadius: '$--radius-sm', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'});
  function sectionHead(id, {label, counts, needs, backlog}) {
    return row(id, label, [
      text(`${id}-name`, label, {size: '$--text-subhead', weight: '600'}),
      caption(`${id}-counts`, counts, MUTED),
      ...(needs ? [caption(`${id}-needs`, `· 내 차례 ${needs}`, '$--warning')] : []),
      spacer(`${id}-sp`),
      ...(backlog ? [LINK_CHIP(`${id}-backlog`, `백로그 ${backlog}`, [['circle-dot', 'lead'], ['chevron-right', 'tail']])] : []),
      LINK_CHIP(`${id}-open`, '열기', [['external-link', 'tail']]),
    ], {width: 'fill_container', gap: SM, padding: [0, 0, XS, 0]});
  }
  const grid = (id, cards, width) => frame(id, 'Cards', {layout: 'horizontal', gap: SM, width, alignItems: 'start'}, cards);
  function tasksBody(p, width, {selected = false} = {}) {
    return col(`${p}-tasks`, 'Tasks', [
      col(`${p}-hide`, 'hide', [
        sectionHead(`${p}-hide-h`, {label: 'hide', counts: '일감 4 · 일하는 중 2', needs: 1, backlog: 3}),
        grid(`${p}-hide-g1`, [
          card(`${p}-c1`, {mark: '?', markFill: '$--warning', title: 'SIGTERM 처리와 자식 정리', age: '4m', question: '기존 stdin 종료 경로도 남길까요?', children: [['●', '$--agent-working'], ['○', MUTED]], prs: [{state: 'failed', number: 216}], items: ['#192'], stage: 2, stageFill: '$--warning', line: '답 · Claude Code가 기다림', lineFill: '$--warning', needs: true, selected}),
          card(`${p}-c2`, {mark: '●', markFill: '$--agent-working', title: '사이드바 타이포그래피 정리', age: '12m', prs: [{state: 'pending', number: 208}], items: ['#186'], outProject: 'sasu', stage: 3, stageFill: '$--agent-working', line: '#208 검토 대기'}),
          incoming(`${p}-c3`, {from: 'mini', children: [['●', '$--agent-working', '원격 빌드 캐시 측정']]}),
        ], width),
        grid(`${p}-hide-g2`, [
          card(`${p}-c4`, {mark: '●', markFill: '$--agent-working', title: 'Overview 백로그 빠른 추가', age: '1m', items: ['L-3'], outDevice: 'mini', stage: 1, stageFill: '$--agent-working', line: '일하는 중 · 에이전트 2'}),
          row(`${p}-folds`, 'Folds', [foldChip(`${p}-fold-clean`, '정리할 것 1', '$--pr-merged'), foldChip(`${p}-fold-rest`, '쉬는 일감 2', MUTED)], {gap: XS, alignItems: 'start'}),
        ], width),
      ], {gap: XS, width}),
      col(`${p}-sasu`, 'sasu', [
        sectionHead(`${p}-sasu-h`, {label: 'sasu', counts: '일감 1'}),
        grid(`${p}-sasu-g`, [card(`${p}-c5`, {mark: '✓', markFill: '$--success', title: 'judge backend 전환 검증', age: '25m', prs: [{state: 'merged', number: 14}], stage: 4, stageFill: '$--success', line: '끝남 · 아직 안 봄', lineFill: '$--success'})], width),
      ], {gap: XS, width}),
      col(`${p}-creator`, 'creator', [
        sectionHead(`${p}-creator-h`, {label: 'creator', counts: '일감 0', backlog: 1}),
        row(`${p}-creator-empty`, 'Empty', [caption(`${p}-creator-none`, '진행 중인 일감 없음 ·', MUTED), caption(`${p}-creator-new`, 'New agent', FG)]),
      ], {gap: XS, width}),
    ], {gap: LG, padding: [MD, LG], width: 'fill_container'});
  }

  // -- the panel beside the tabs (OverviewRecord.tsx, BacklogTab.tsx BacklogItemPanel) ------
  function panelHead(id, actions) {
    return row(id, 'Panel head', [screenIconButton(`${id}-x`, 'x'), spacer(`${id}-sp`), ...actions], {width: 'fill_container', padding: [XS, SM]});
  }
  function taskPanel(p) {
    return col(`${p}-panel`, 'Task record', [
      panelHead(`${p}-ph`, [screenButton(`${p}-ph-open`, 'Workspace 열기'), screenButton(`${p}-ph-lane`, 'lane에서 보기', {variant: 'outline'})]),
      col(`${p}-rec`, 'Record', [
        text(`${p}-rec-title`, 'SIGTERM 처리와 자식 정리', {size: '$--text-subhead', weight: '600'}),
        micro(`${p}-rec-place`, 'This Mac · hide › 192-hided-sigterm-handler · Claude Code', MUTED),
        row(`${p}-rec-stage`, 'Stage', [stageBar(`${p}-rec-sb`, 2, '$--warning'), caption(`${p}-rec-st`, '작업 → PR', SUBTLE)], {gap: SM}),
        rule(`${p}-rec-r1`),
        caption(`${p}-rec-now`, '지금 할 일', SUBTLE, {weight: '600'}),
        caption(`${p}-rec-ask`, '기존 stdin 종료 경로도 남길까요?', '$--warning', {width: PANEL_W - 2 * MD}),
        rule(`${p}-rec-r2`),
        caption(`${p}-rec-wt`, '워크트리', SUBTLE, {weight: '600'}),
        caption(`${p}-rec-wt1`, 'hide-192-hided-sigterm-handler', FG, {mono: true}),
      ], {gap: XS, padding: [SM, MD], width: 'fill_container'}),
    ], {width: PANEL_W, gap: 0, fill: '$--background', stroke: '$--border', strokeWidth: {left: HAIR}, strokeAlignment: 'inner', height: 'fill_container'});
  }
  function itemPanel(p) {
    return col(`${p}-panel`, 'Backlog item', [
      panelHead(`${p}-ph`, [screenButton(`${p}-ph-start`, '▷ 시작'), screenIconButton(`${p}-ph-gh`, 'external-link')]),
      col(`${p}-item`, 'Item', [
        row(`${p}-item-t`, 'Title', [caption(`${p}-item-tok`, '#214', MUTED, {mono: true}), text(`${p}-item-title`, '좁은 열에서 긴 한국어 요청 줄바꿈 확인', {size: '$--text-subhead', weight: '600'})], {gap: SM}),
        micro(`${p}-item-src`, '출처 · acme/hide · 만든 때 2시간 전', MUTED),
        rule(`${p}-item-r1`),
        caption(`${p}-item-origin`, '어디서 왔나', SUBTLE, {weight: '600'}),
        caption(`${p}-item-req`, '세션 기록 화면과 결과물 연결 구현', FG, {width: PANEL_W - 2 * MD}),
        micro(`${p}-item-turn`, 'Claude Code · hide › main · 놓친 일', MUTED),
        rule(`${p}-item-r2`),
        screenButton(`${p}-item-undo`, '되돌리기', {variant: 'ghost'}),
      ], {gap: XS, padding: [SM, MD], width: 'fill_container'}),
    ], {width: PANEL_W, gap: 0, fill: '$--background', stroke: '$--border', strokeWidth: {left: HAIR}, strokeAlignment: 'inner', height: 'fill_container'});
  }

  // -- 백로그 (BacklogTab.tsx) ----------------------------------------------------------------
  function sourceChip(id, label) {
    return row(id, label, [caption(`${id}-t`, label, SUBTLE), icon(`${id}-g`, 'chevron-down', {size: ICON_SM, fill: MUTED})], {gap: XXS, padding: [0, XS], cornerRadius: '$--radius-xs', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner'});
  }
  function backlogRow(id, {token, title, labels = [], flag = false, age, github = false, selected = false}) {
    return row(id, title, [
      caption(`${id}-tok`, token, MUTED, {mono: true}),
      caption(`${id}-t`, title, FG),
      ...(flag ? [icon(`${id}-flag`, 'flag', {size: ICON_SM, fill: '$--warning'})] : []),
      spacer(`${id}-sp`),
      ...labels.map(([name, fill], index) => row(`${id}-l${index}`, name, [frame(`${id}-l${index}-d`, 'Dot', {width: DOT + 1, height: DOT + 1, cornerRadius: DOT, fill}, []), micro(`${id}-l${index}-t`, name, SUBTLE)], {gap: XXS})),
      micro(`${id}-age`, age, MUTED, {mono: true}),
      ...(github ? [icon(`${id}-gh`, 'external-link', {size: ICON_SM, fill: MUTED})] : []),
    ], {width: 'fill_container', gap: SM, padding: [XS, SM], cornerRadius: '$--radius-sm', ...(selected ? {fill: '$--secondary'} : {})});
  }
  function backlogBody(p, width) {
    return col(`${p}-backlog`, 'Backlog', [
      col(`${p}-hide`, 'hide', [
        row(`${p}-hide-h`, 'hide', [
          text(`${p}-hide-n`, 'hide', {size: '$--text-subhead', weight: '600'}),
          sourceChip(`${p}-hide-src`, 'GitHub 이슈'),
          caption(`${p}-hide-open`, '열림 3', MUTED),
          caption(`${p}-hide-started`, '진행 중 1 → Tasks', MUTED),
        ], {gap: SM, padding: [0, 0, XS, 0]}),
        row(`${p}-hide-in`, 'Input', [screenInput(`${p}-hide-input`, {placeholder: 'hide에 이슈 적기', width: width - 240}), caption(`${p}-hide-dest`, 'Enter · GitHub에 바로 만듦 · 공개', '$--warning')], {gap: SM, padding: [0, 0, XS, 0]}),
        row(`${p}-pending`, 'Creating', [caption(`${p}-pending-t`, '빠른 추가 두 번째 줄 확인', FG), spacer(`${p}-pending-sp`), icon(`${p}-pending-g`, 'loader-circle', {size: ICON_SM, fill: MUTED}), caption(`${p}-pending-w`, '만드는 중', MUTED)], {width: 'fill_container', gap: SM, padding: [XS, SM], opacity: 0.6}),
        row(`${p}-failed`, 'Failed', [caption(`${p}-failed-t`, '원격 빌드 캐시 측정 이슈', FG), spacer(`${p}-failed-sp`), caption(`${p}-failed-why`, '네트워크 또는 사용량 제한', '$--destructive'), screenButton(`${p}-failed-retry`, '다시 시도', {variant: 'ghost'}), screenButton(`${p}-failed-rm`, '지우기', {variant: 'ghost'})], {width: 'fill_container', gap: SM, padding: [XS, SM]}),
        backlogRow(`${p}-r1`, {token: '#214', title: '좁은 열에서 긴 한국어 요청 줄바꿈 확인', flag: true, labels: [['enhancement', '$--agent-working']], age: '2시간', github: true, selected: true}),
        backlogRow(`${p}-r2`, {token: '#211', title: 'hided SIGTERM 뒤 남은 자식 프로세스 정리', labels: [['bug', '$--destructive']], age: '1일', github: true}),
        backlogRow(`${p}-r3`, {token: '#199', title: '원격 기본 체크아웃을 기기 연결로 읽기', age: '3일', github: true}),
      ], {gap: 0, width}),
      col(`${p}-creator`, 'creator', [
        row(`${p}-creator-h`, 'creator', [text(`${p}-creator-n`, 'creator', {size: '$--text-subhead', weight: '600'}), sourceChip(`${p}-creator-src`, '메모'), caption(`${p}-creator-open`, '열림 1', MUTED)], {gap: SM, padding: [0, 0, XS, 0]}),
        row(`${p}-creator-in`, 'Input', [screenInput(`${p}-creator-input`, {placeholder: 'creator에 메모 적기', width: width - 240}), caption(`${p}-creator-dest`, 'Enter · 이 기기에만 저장', MUTED)], {gap: SM, padding: [0, 0, XS, 0]}),
        backlogRow(`${p}-r4`, {token: 'L-7', title: 'AI 개발자 릴스 5탄 기획', age: '5일'}),
      ], {gap: 0, width}),
    ], {gap: LG, padding: [MD, LG], width: 'fill_container'});
  }

  // -- 프로젝트 (ProjectsTab.tsx) -------------------------------------------------------------
  const COLS = {project: 200, now: 260, backlog: 56, disk: 200, care: 140, activity: 80};
  function cell(id, key, children) {
    return row(id, key, children, {width: COLS[key], gap: XS});
  }
  function nowCell(id, {needs = 0, working = 0, waiting = 0, done = 0, resting = 0}) {
    const parts = [[needs, `내 차례 ${needs}`, '$--warning'], [working, `일하는 중 ${working}`, '$--agent-working'], [waiting, `기다림 ${waiting}`, '$--success'], [done, `끝남 ${done}`, SUBTLE]].filter(([value]) => value > 0);
    if (!parts.length) return cell(id, 'now', [caption(`${id}-t`, resting ? `쉬는 중 ${resting}` : '일감 없음', MUTED)]);
    return cell(id, 'now', parts.map(([, label, fill], index) => caption(`${id}-${index}`, index ? `· ${label}` : label, fill)));
  }
  function diskCell(id, {size, share, freed, measuring = false, folder = false}) {
    const bar = 120;
    return cell(id, 'disk', [
      ...(folder ? [] : [frame(`${id}-bar`, 'Bar', {layout: 'horizontal', width: bar, height: HAIR, fill: '$--muted', cornerRadius: HAIR, clip: true}, [
        frame(`${id}-used`, 'Used', {layout: 'horizontal', width: Math.max(2, Math.round(bar * share)), height: HAIR, fill: MUTED, justifyContent: 'end'}, [
          frame(`${id}-freed`, 'Reclaimable', {width: Math.round(bar * share * freed), height: HAIR, fill: '$--pr-merged'}, []),
        ]),
      ])]),
      caption(`${id}-t`, size, measuring ? MUTED : FG, {mono: true}),
      ...(measuring ? [icon(`${id}-spin`, 'loader-circle', {size: ICON_SM, fill: MUTED})] : []),
    ]);
  }
  function projectRow(id, {name, folder = false, now, backlog, disk, merged, activity, selected = false, measuring = false}) {
    return row(id, name, [
      cell(`${id}-p`, 'project', [caption(`${id}-pn`, name, FG, {weight: '500'}), ...(folder ? [micro(`${id}-pf`, '폴더')] : [])]),
      nowCell(`${id}-now`, now),
      cell(`${id}-b`, 'backlog', [caption(`${id}-bt`, backlog, backlog === '0' ? MUTED : SUBTLE, {mono: true})]),
      diskCell(`${id}-d`, {...disk, folder, measuring}),
      cell(`${id}-c`, 'care', merged ? [caption(`${id}-ct`, `${merged} merged → 정리`, '$--pr-merged')] : []),
      cell(`${id}-a`, 'activity', [micro(`${id}-at`, activity, MUTED, {mono: true})]),
      icon(`${id}-open`, 'external-link', {size: ICON_SM, fill: MUTED}),
    ], {width: 'fill_container', gap: MD, padding: [XS, SM], cornerRadius: '$--radius-sm', ...(measuring ? {opacity: 0.6} : {}), ...(selected ? {fill: '$--secondary'} : {})});
  }
  function projectsBody(p) {
    const header = row(`${p}-cols`, 'Columns', [
      ['project', '프로젝트'], ['now', '지금'], ['backlog', '백로그'], ['disk', '디스크'], ['care', '챙길 것'], ['activity', '마지막 활동'],
    ].map(([key, label]) => cell(`${p}-col-${key}`, key, [micro(`${p}-col-${key}-t`, label, MUTED, {weight: '600'})])), {width: 'fill_container', gap: MD, padding: [0, SM]});
    return col(`${p}-projects`, 'Projects', [
      row(`${p}-disk`, 'Disk line', [
        caption(`${p}-disk-free`, 'This Mac 디스크 여유 41 GB / 494 GB', FG, {mono: true}),
        caption(`${p}-disk-sum`, '· 프로젝트 합계 63 GB', SUBTLE, {mono: true}),
        caption(`${p}-disk-freed`, '· 정리하면 12 GB', '$--pr-merged', {mono: true}),
        spacer(`${p}-disk-sp`),
        screenButton(`${p}-sort`, '크기순', {variant: 'ghost'}),
      ], {width: 'fill_container', gap: XS, padding: [0, 0, SM, 0]}),
      header,
      rule(`${p}-rule`),
      projectRow(`${p}-hide`, {name: 'hide', now: {needs: 1, working: 2, waiting: 1}, backlog: '3', disk: {size: '38 GB', share: 1, freed: 0.3}, merged: 4, activity: '3분 전', selected: true}),
      projectRow(`${p}-sasu`, {name: 'sasu', now: {done: 1}, backlog: '0', disk: {size: '9.4 GB', share: 0.25, freed: 0}, activity: '25분 전'}),
      projectRow(`${p}-creator`, {name: 'creator', folder: true, now: {}, backlog: '1', disk: {size: '1.1 GB'}, activity: '2일 전'}),
      projectRow(`${p}-site`, {name: 'withhide-site', now: {resting: 2}, backlog: '0', disk: {size: '14 GB', share: 0.37, freed: 0}, activity: '1시간 전', measuring: true}),
      row(`${p}-quiet`, 'Quiet fold', [caption(`${p}-quiet-t`, '조용한 프로젝트 2 · 1.2 GB', MUTED), icon(`${p}-quiet-g`, 'chevron-right', {size: ICON_SM, fill: MUTED})], {gap: XXS, padding: [XS, SM]}),
    ], {gap: 0, padding: [MD, LG], width: 'fill_container'});
  }

  // -- the page: the head, the tabs and the body with its panel ------------------------------
  const PAGE_W = 1200, PAGE_H = 760;
  function page(id, name, {active, body, panel, controls, selected}) {
    return frame(id, name, {width: PAGE_W, height: PAGE_H, layout: 'vertical', fill: '$--background', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', clip: true}, [
      head(`${id}-head`, {selected}),
      tabs(`${id}-tabs`, active, controls),
      row(`${id}-body`, 'Body', [body, ...(panel ? [panel] : [])], {width: 'fill_container', height: 'fill_container', gap: 0, alignItems: 'start'}),
    ]);
  }
  const bodyWidth = panel => PAGE_W - 2 * LG - (panel ? PANEL_W : 0);
  function unanswered(id) {
    return frame(id, 'Device does not answer', {width: PAGE_W, height: 320, layout: 'vertical', fill: '$--background', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', clip: true}, [
      head(`${id}-head`, {selected: 'studio'}),
      tabs(`${id}-tabs`, 'tasks', []),
      col(`${id}-body`, 'Unanswered', [
        text(`${id}-t`, 'studio가 응답하지 않는다 · 연결이 끊김', {size: '$--text-body', fill: SUBTLE}),
        screenButton(`${id}-retry`, '다시 연결', {variant: 'secondary'}),
      ], {width: 'fill_container', height: 'fill_container', gap: MD, alignItems: 'center', justifyContent: 'center'}),
    ]);
  }

  // -- ⌘⇧M (QuickAdd.tsx): placed as the ⌘K palette is, on --popover ------------------------
  const QUICK_W = num(tokens, '--size-search-sheet-w');
  const SHADOW = {type: 'shadow', shadowType: 'outer', offset: {x: 0, y: 12}, blur: 32, spread: 0, color: '#00000073'};
  function quickAdd(id, state) {
    const surface = children => frame(id, `Quick add · ${state}`, {width: QUICK_W, layout: 'vertical', gap: XS, padding: MD, fill: '$--popover', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: HAIR, strokeAlignment: 'inner', effect: SHADOW}, children);
    if (state === 'made') {
      return surface([row(`${id}-made`, 'Made', [text(`${id}-made-t`, 'hide에 이슈 #77 만듦', {size: '$--text-body'}), screenButton(`${id}-view`, '보기', {variant: 'ghost'})], {gap: SM})]);
    }
    const typed = state === 'open' ? '' : '좁은 열에서 긴 한국어 요청 줄바꿈 확인';
    return surface([
      row(`${id}-proj`, 'Project', [
        caption(`${id}-proj-n`, 'hide', FG, {weight: '500'}),
        caption(`${id}-proj-r`, '· 앞에 있는 체크아웃', MUTED),
        spacer(`${id}-proj-sp`),
        ...(state === 'open' ? [micro(`${id}-proj-tab`, 'Tab: 다음 프로젝트')] : []),
      ], {width: 'fill_container'}),
      frame(`${id}-field`, 'Input', {width: 'fill_container', height: num(tokens, '--size-control-lg'), layout: 'horizontal', alignItems: 'center'}, [
        text(`${id}-field-t`, typed || 'hide에 이슈 적기', {size: '$--text-body', fill: typed ? FG : MUTED}),
      ]),
      state === 'creating'
        ? row(`${id}-dest`, 'Creating', [icon(`${id}-dest-g`, 'loader-circle', {size: ICON_SM, fill: MUTED}), micro(`${id}-dest-t`, '만드는 중')], {gap: XXS})
        : state === 'failed'
          ? row(`${id}-dest`, 'Failed', [micro(`${id}-dest-t`, '네트워크 또는 사용량 제한 · Enter로 다시 시도', '$--destructive')])
          : row(`${id}-dest`, 'Destination', [micro(`${id}-dest-t`, 'GitHub 이슈로 바로 만듦 · 공개', '$--warning')]),
    ]);
  }

  return {
    // The Overview over a Workspace, one board per tab and the states the tabs carry.
    boards(s) {
      const tasks = page(`ov-tasks-${s}`, 'Tasks · a card selected', {active: 'tasks', controls: taskControls(`ov-tasks-${s}-ctl`), body: tasksBody(`ov-tasks-${s}`, bodyWidth(true), {selected: true}), panel: taskPanel(`ov-tasks-${s}`)});
      const backlog = page(`ov-backlog-${s}`, '백로그 · an item selected', {active: 'backlog', controls: backlogControls(`ov-backlog-${s}-ctl`), body: backlogBody(`ov-backlog-${s}`, bodyWidth(true)), panel: itemPanel(`ov-backlog-${s}`)});
      const projects = page(`ov-projects-${s}`, '프로젝트', {active: 'projects', controls: [], body: projectsBody(`ov-projects-${s}`)});
      const quick = frame(`ov-quick-${s}`, '⌘⇧M', {layout: 'horizontal', gap: LG, alignItems: 'start'}, ['open', 'creating', 'failed', 'made'].map(state => quickAdd(`ov-quick-${s}-${state}`, state)));
      return [frame(`ov-column-${s}`, 'Boards', {layout: 'vertical', gap: '$--spacing-xl'}, [tasks, backlog, projects, unanswered(`ov-unanswered-${s}`), quick])];
    },
    // The Overview as the page a device's Home row opens: Tasks, no card selected.
    page(s) {
      return page(`mn-page-${s}`, 'Overview page · Tasks', {active: 'tasks', controls: taskControls(`mn-page-${s}-ctl`), body: tasksBody(`mn-page-${s}`, bodyWidth(false))});
    },
  };
}
