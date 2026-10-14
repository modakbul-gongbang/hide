// The Sessions record (PRD session-record-redesign): the PRs this session and
// its descendants made, the assigned work as a checklist with each task's
// missed work under it, the operator's own words behind the `내 말` toggle,
// and the summary-off and state boards. Controls reference the existing
// library through pen-screens' themed helpers; the PR mark is a ref of
// Component / Agent tree parts, as the Sessions rows draw it.
import {AGENT_TREE_MASTERS, PR_STATES} from './pen-agent-tree.mjs';

export function sessionWorkScreens({frame, text, icon, screenButton, screenBadge, themedXref}) {
  const WIDTH = 372;
  const STEP = 19;
  const label = (id, content, fill = '$--foreground', extra = {}) => text(id, content, {size: '$--text-caption', fill, ...extra});
  const micro = (id, content, fill = '$--muted-foreground', extra = {}) => text(id, content, {size: '$--text-micro', fill, ...extra});
  const provider = (id, kind = 'claude', size = 14) => frame(id, 'Provider', {width: size, height: size, fill: {type: 'image', enabled: true, url: `../web/src/assets/agent-${kind}.png`, mode: 'contain'}}, []);
  const prMark = (id, number, state) => {
    const [, glyph, fill] = PR_STATES.find(([key]) => key === state);
    return themedXref(id, AGENT_TREE_MASTERS.prIcon, `PR #${number}`, {}, {'ath-pr-g': {icon: glyph, fill}, 'ath-pr-n': {content: `#${number}`}, 'ath-pr-more': {enabled: false}});
  };
  const heading = (id, title, count, extra = []) => frame(id, title, {width: 'fill_container', height: 24, layout: 'horizontal', gap: 6, alignItems: 'center', padding: [8, 0, 0, 0]}, [
    label(`${id}-t`, title, '$--subtle-foreground', {weight: '500'}),
    ...(count === undefined ? [] : [micro(`${id}-n`, count)]),
    ...(extra.length ? [frame(`${id}-sp`, 'Spacer', {width: 'fill_container', height: 1}, []), ...extra] : []),
  ]);
  const top = (id, {title, state, kind = 'claude'}) => [
    screenButton(`${id}-back`, 'Sessions로 돌아가기', {variant: 'ghost', icon: 'arrow-left'}),
    frame(`${id}-head`, 'Title', {width: 'fill_container', layout: 'horizontal', gap: 6, alignItems: 'center', padding: [4, 0, 0, 0]}, [
      provider(`${id}-mark`, kind, 16),
      text(`${id}-title`, title, {size: '$--text-subhead', weight: '600', width: 'fill_container'}),
    ]),
    micro(`${id}-state`, state),
  ];

  // Every PR the session and its descendants made, worst first; a descendant's
  // PR names that child at the row's end.
  const prRow = (id, {number, state, title, by}) => frame(id, `PR #${number}`, {width: 'fill_container', height: 22, layout: 'horizontal', gap: 6, alignItems: 'center'}, [
    prMark(`${id}-mark`, number, state),
    label(`${id}-title`, title, '$--foreground', {width: 'fill_container'}),
    ...(by ? [frame(`${id}-by`, 'Made by', {layout: 'horizontal', gap: 3, alignItems: 'center'}, [provider(`${id}-by-mark`, by[0], 12), micro(`${id}-by-name`, by[1])])] : []),
  ]);

  // The operator's own words: a muted person glyph and a bubble, two lines
  // until pressed, then the whole text and 접기.
  const said = (id, content, {open = false, quiet = false} = {}) => frame(id, 'Operator words', {width: 'fill_container', layout: 'horizontal', gap: 5, alignItems: 'start', padding: [2, 0, 0, STEP]}, [
    icon(`${id}-who`, 'user', {size: 11, fill: '$--muted-foreground'}),
    frame(`${id}-bubble`, 'Bubble', {width: 'fill_container', layout: 'vertical', gap: 2, padding: [4, 8], cornerRadius: '$--radius-sm', fill: '$--secondary'}, [
      label(`${id}-text`, content, quiet ? '$--subtle-foreground' : '$--foreground', {width: 'fill_container'}),
      ...(open ? [micro(`${id}-less`, '접기')] : []),
    ]),
  ]);
  // Missed work under the task whose turn found it: open in warning with its
  // two actions, closed as one muted line.
  const missed = (id, content, {open = false} = {}) => frame(id, 'Missed work', {width: 'fill_container', layout: 'vertical', gap: 4, padding: [2, 0, 0, STEP]}, [
    frame(`${id}-line`, 'Line', {width: 'fill_container', layout: 'horizontal', gap: 5, alignItems: 'center'}, [
      icon(`${id}-flag`, 'flag', {size: 11, fill: open ? '$--warning' : '$--muted-foreground'}),
      label(`${id}-text`, content, open ? '$--warning' : '$--muted-foreground', {width: 'fill_container'}),
    ]),
    ...(open ? [frame(`${id}-actions`, 'Missed actions', {layout: 'horizontal', gap: 4, padding: [0, 0, 0, 16]}, [
      screenButton(`${id}-issue`, '이슈로', {variant: 'outline', height: 22}),
      screenButton(`${id}-dismiss`, '무시', {variant: 'ghost', height: 22}),
    ])] : []),
  ]);
  const GLYPH = {done: ['✓', '$--success'], working: ['◐', '$--agent-working'], review: ['◐', '$--success'], unfinished: ['◐', '$--warning']};
  const task = (id, {state, name, pr, words = [], misses = []}) => frame(id, name, {width: 'fill_container', layout: 'vertical', gap: 2, padding: [4, 0]}, [
    frame(`${id}-head`, 'Task', {width: 'fill_container', layout: 'horizontal', gap: 6, alignItems: 'start'}, [
      label(`${id}-glyph`, GLYPH[state][0], GLYPH[state][1], {weight: '600'}),
      label(`${id}-name`, name, state === 'done' ? '$--subtle-foreground' : '$--foreground', {width: 'fill_container', weight: state === 'done' ? '400' : '500'}),
      ...(pr ? [prMark(`${id}-pr`, pr[0], pr[1])] : []),
    ]),
    ...words.map((word, index) => said(`${id}-w${index}`, word.text ?? word, word)),
    ...misses.map((miss, index) => missed(`${id}-m${index}`, miss.text, miss)),
  ]);
  const toggle = (id, on) => screenBadge(`${id}-words`, '내 말', {variant: on ? 'secondary' : 'outline'});
  const children = id => [
    heading(`${id}-children`, '자식', '1'),
    frame(`${id}-child`, 'Delegated child', {width: 'fill_container', height: 24, layout: 'horizontal', gap: 6, alignItems: 'center', stroke: '$--lineage-rail', strokeWidth: {left: 1}, strokeAlignment: 'inner', padding: [0, 0, 0, 8]}, [
      icon(`${id}-child-fold`, 'chevron-right', {size: 12, fill: '$--muted-foreground'}),
      provider(`${id}-child-mark`, 'claude', 12),
      label(`${id}-child-title`, '상주 데몬 버그 3건 수정 PR', '$--foreground', {width: 'fill_container'}),
      micro(`${id}-child-state`, '쉬는 중'),
    ]),
  ];
  const panel = (id, name, body) => frame(id, name, {width: WIDTH, layout: 'vertical', gap: 2, padding: [8, 12, 16, 12], fill: '$--card', stroke: '$--border', strokeWidth: 1, strokeAlignment: 'inner', cornerRadius: '$--radius-sm'}, body);

  // The one session the boards draw, read from a real record: three tasks,
  // two closed missed items found during the second, and a PR the child made.
  const TITLE = '성능 최적화 리뷰와 서비스 개선안';
  const WORDS = {
    review: '옆 codex pane의 퍼포먼스 최적화 작업을 리뷰하고 그다음에 실제로 우리 서비스에서 느려지거나 할 때 (herdr 병목은 우선 어쩔 수 없음) 어떻게 개선할 수 있을지 고민해보자',
    measure: '설치가 됐으니 이제 너가 직접 하나 hide 실측을 진행해보고 너가 제안한것들 혹시 코드 구조나 앱…',
    bugs: '버그 3건 한 PR로 묶어서 진행해줘 opus 5.5로 하나 띄워서 진행시키자',
    merge: 'PR 열리면 리뷰하고 머지까지 진행해줘',
  };
  const tasks = (id, on) => [
    task(`${id}-t1`, {state: 'done', name: 'codex 성능 최적화 작업 리뷰 및 서비스 개선안 도출', words: on ? [{text: WORDS.review, open: true}] : []}),
    task(`${id}-t2`, {state: 'done', name: '설치 빌드 실측 및 개선 제안 안전성 평가', words: on ? [WORDS.measure] : [], misses: [
      {text: 'Codex developer role 매핑 수정 · 처리됨'},
      {text: 'links ledger 세션 불일치 수정 · 처리됨'},
    ]}),
    task(`${id}-t3`, {state: 'working', name: '버그 3건 한 PR로 묶어 수정 (Opus 에이전트)', pr: [1093, 'merged'], words: on ? [WORDS.bugs, WORDS.merge] : []}),
  ];
  function record(suffix, on) {
    const id = `swr-${on ? 'words' : 'record'}-${suffix}`;
    return panel(id, on ? '내 말 켬' : 'Session record', [
      ...top(id, {title: TITLE, state: '쉬는 중 · 자식 1'}),
      heading(`${id}-prs`, 'PR', '2'),
      prRow(`${id}-pr1`, {number: 1100, state: 'pending', title: 'Measure the install build after the fixes'}),
      prRow(`${id}-pr2`, {number: 1093, state: 'merged', title: 'Stop three resident hided loops', by: ['claude', '상주 데몬 버그…']}),
      heading(`${id}-work`, '시킨 일', '2/3', [toggle(id, on)]),
      ...tasks(id, on),
      ...(on ? [
        micro(`${id}-other`, '그 밖의 말', '$--muted-foreground', {weight: '500'}),
        said(`${id}-o1`, '그러면 지금 처리하는 작업들 말고 그다음에 너가 해야할것들 정리좀 해줘봐 ~', {quiet: true}),
        said(`${id}-o2`, '잘 되고 있어?', {quiet: true}),
      ] : []),
      ...children(id),
    ]);
  }
  // Summaries off: no task judgment, so the PRs and the operator's raw
  // requests, oldest first with the last five open.
  function off(suffix) {
    const id = `swr-off-${suffix}`;
    const request = (key, at, content) => frame(`${id}-${key}`, at, {width: 'fill_container', layout: 'horizontal', gap: 8, alignItems: 'start', padding: [3, 0]}, [
      micro(`${id}-${key}-at`, at, '$--muted-foreground', {mono: true}),
      label(`${id}-${key}-text`, content, '$--foreground', {width: 'fill_container'}),
    ]);
    return panel(id, 'Summaries off', [
      ...top(id, {title: TITLE, state: '쉬는 중 · 자식 1 · 요청 7'}),
      heading(`${id}-prs`, 'PR', '1'),
      prRow(`${id}-pr1`, {number: 1093, state: 'merged', title: 'Stop three resident hided loops', by: ['claude', '상주 데몬 버그…']}),
      heading(`${id}-requests`, '요청', '7'),
      micro(`${id}-older`, '› 이전 요청 2'),
      request('r3', '15:39', WORDS.bugs),
      request('r4', '15:53', '그러면 지금 처리하는 작업들 말고 그다음에 너가 해야할것들 정리좀 해줘봐 ~'),
      request('r5', '16:00', WORDS.merge),
      request('r6', '16:43', '잘 되고 있어?'),
      request('r7', '17:13', '설치하고 재측정해줘'),
      ...children(id),
    ]);
  }
  function states(suffix) {
    const id = `swr-states-${suffix}`;
    const group = (key, title, body) => frame(`${id}-${key}`, title, {width: 'fill_container', layout: 'vertical', gap: 4}, [micro(`${id}-${key}-cap`, title, '$--subtle-foreground', {weight: '600'}), ...body]);
    return panel(id, 'States', [
      group('open', '열린 놓친 일: 그 일 밑에 노란 줄과 두 버튼', [
        task(`${id}-open-t`, {state: 'unfinished', name: '설치 후 재측정', misses: [{text: '재측정 전 설치본 버전 확인 안 됨', open: true}]}),
      ]),
      group('now', '판정 전 마지막 요청: 내 말을 꺼도 맨 끝에 보임', [
        task(`${id}-now-t`, {state: 'done', name: '설치 빌드 실측 및 개선 제안 안전성 평가'}),
        frame(`${id}-now-row`, 'Not yet judged', {width: 'fill_container', layout: 'horizontal', gap: 6, alignItems: 'start', padding: [4, 0]}, [
          icon(`${id}-now-g`, 'loader-circle', {size: 12, fill: '$--agent-working'}),
          label(`${id}-now-w`, '지금', '$--agent-working', {weight: '500'}),
          label(`${id}-now-t2`, '설치하고 재측정해줘', '$--foreground', {width: 'fill_container'}),
        ]),
      ]),
      group('chapters', '목표가 둘 이상일 때만 챕터 줄', [
        micro(`${id}-ch1`, '세션 작업 기록 구현 · 09:00', '$--subtle-foreground', {weight: '500'}),
        task(`${id}-ch1-t`, {state: 'done', name: 'Sessions 기록 화면'}),
        micro(`${id}-ch2`, '세션 검색과 놓친 일 정리 · 11:20', '$--subtle-foreground', {weight: '500'}),
        task(`${id}-ch2-t`, {state: 'review', name: '⌘K 세션 검색', pr: [1122, 'failed']}),
      ]),
      group('loading', '읽는 중', [frame(`${id}-skeleton`, 'Skeleton', {width: 'fill_container', height: 32, fill: '$--secondary', cornerRadius: '$--radius-sm'}, [])]),
      group('failure', '기록을 읽지 못함', [screenButton(`${id}-retry`, '다시 시도', {variant: 'ghost'})]),
      group('search', '세션 검색 결과', [label(`${id}-search-title`, '세션 작업 기록과 검색 구현'), micro(`${id}-matched`, '세션 검색을 연결하고 좁은 칸에서도 읽기 좋게 해줘', '$--muted-foreground', {width: 'fill_container'})]),
      group('draft', '이슈로: 새 이슈', [
        micro(`${id}-repo`, 'example/hide'),
        micro(`${id}-note`, '내용을 확인하고 수정한 뒤 만들어 주세요.', '$--subtle-foreground'),
        label(`${id}-draft-title`, '재측정 전 설치본 버전 확인'),
        label(`${id}-body`, '확인할 일\n재측정 전에 설치된 앱의 버전과 빌드를 기록합니다.\n\n완료 조건\n측정 기록에 버전이 함께 남습니다.', '$--foreground', {width: 'fill_container'}),
        frame(`${id}-footer`, 'Explicit creation', {width: 'fill_container', layout: 'horizontal', gap: 8, justifyContent: 'end'}, [screenButton(`${id}-cancel`, '취소', {variant: 'secondary'}), screenButton(`${id}-create`, '이슈 만들기')]),
      ]),
    ]);
  }
  return suffix => [record(suffix, false), record(suffix, true), off(suffix), states(suffix)];
}
