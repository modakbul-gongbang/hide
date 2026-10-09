// The Sessions panel and pane header (PRD agent-hierarchy-screens D-14 to
// D-24, D-37 to D-43; the approved Pen round 2, D-44). Controls are refs of the
// existing System masters supplied by pen-screens, and the verb, descendant
// mark and tree button are refs of Component / Agent tree parts.
export function sessionScreens(tokens, {frame, text, icon, themedXref, screenButton, screenBadge, screenIconButton}) {
  const caption = (id, value, fill = '$--muted-foreground', extra = {}) => text(id, value, {size:'$--text-caption', fill, ...extra});
  const verb = (id, word) => themedXref(id, 'ath-verb', word, {}, {'ath-verb-t': {content: word}});
  const descendantMark = (id, kind, count) => themedXref(id, 'ath-mark', 'Descendant mark', {}, {
    'ath-mark-bang': {enabled: kind === 'raised'}, 'ath-mark-dot': {enabled: kind === 'working'},
    'ath-mark-n': {content: String(count), fill: kind === 'raised' ? '$--warning' : '$--agent-working'},
  });
  const provider = (id, kind = 'claude') => frame(id, 'Provider', {width:14,height:14,fill:{type:'image',enabled:true,url:`../web/src/assets/agent-${kind}.png`,mode:'contain'}}, []);
  // The PR chip: one PR is its number and state glyph; several are `PR n` with the worst glyph and its count (B22).
  const GLYPH = {failed:['×','$--destructive'], pending:['◷','$--muted-foreground'], mergeable:['✓','$--success'], merged:['⇥','$--pr-merged']};
  const prChip = (id, {number, state, count, worst}) => frame(id, 'PR chip', {layout:'horizontal',gap:2,alignItems:'center',height:16,padding:[0,4],cornerRadius:'$--radius-xs',stroke:'$--border',strokeWidth:1,strokeAlignment:'inner'}, [
    caption(`${id}-n`, count ? `PR ${count}` : `#${number}`, '$--subtle-foreground', {mono:true}),
    caption(`${id}-g`, GLYPH[state][0], GLYPH[state][1], {mono:true,weight:'600'}),
    ...(count ? [caption(`${id}-c`, String(worst), GLYPH[state][1], {mono:true})] : []),
  ]);
  const chevron = (id, open) => frame(id, 'Chevron lane', {width:16,height:20,layout:'horizontal',justifyContent:'center',alignItems:'center'}, open === null ? [] : [icon(`${id}-g`, open ? 'chevron-down' : 'chevron-right', {size:12,fill:'$--muted-foreground'})]);

  // A Sessions row (B30, B31): mark, provider, title, own PR chip, the folded
  // descendant mark and the age; the second line is the ask for Needs You and
  // otherwise the label line, absent when there is none.
  const row = (id, spec, width) => frame(id, spec.title, {width, layout:'horizontal', alignItems:'start', padding:[4,8,4,spec.depth ? 0 : 4]}, [
    ...(spec.depth ? [frame(`${id}-elbow`, 'Elbow', {layout:'none',width:18,height:28}, [
      frame(`${id}-ev`, 'Rail', {x:8,y:0,width:1,height:spec.last ? 13 : 28,fill:'$--lineage-rail'}, []),
      frame(`${id}-eh`, 'Rail', {x:8,y:13,width:10,height:1,fill:'$--lineage-rail'}, []),
    ])] : []),
    chevron(`${id}-chev`, spec.open ?? null),
    frame(`${id}-body`, 'Lines', {width:'fill_container', layout:'vertical', gap:2, padding:[0,0,0,4]}, [
      frame(`${id}-head`, 'Line one', {width:'fill_container',height:20,layout:'horizontal',gap:6,alignItems:'center'}, [
        caption(`${id}-mark`, spec.mark ?? '●', spec.tone ?? '$--agent-working', {mono:true,weight:'600'}),
        provider(`${id}-provider`, spec.provider),
        caption(`${id}-title`, spec.title, '$--foreground', {width:'fill_container',weight: spec.depth ? '400' : '500'}),
        ...(spec.pr ? [prChip(`${id}-pr`, spec.pr)] : []),
        ...(spec.desc ? [descendantMark(`${id}-desc`, spec.desc[0], spec.desc[1])] : []),
        caption(`${id}-age`, spec.age ?? '3m', '$--muted-foreground', {mono:true}),
      ]),
      ...(spec.ask ? [frame(`${id}-ask`, 'Ask', {width:'fill_container',layout:'horizontal',gap:6,alignItems:'center'}, [verb(`${id}-verb`, spec.ask[0]), caption(`${id}-what`, spec.ask[1], '$--warning')])]
        : spec.line ? [caption(`${id}-line`, spec.line, '$--subtle-foreground')] : []),
    ]),
  ]);
  function panel(suffix, width=420, id=`sfu-panel-${suffix}`) {
    const group=(key,title,rows,{folded=false,more=0,count}={})=>frame(`${id}-${key}`,title,{width:'fill_container',layout:'vertical',gap:0},[
      frame(`${id}-${key}-heading`,'Group heading',{width:'fill_container',height:32,layout:'horizontal',gap:6,alignItems:'center',padding:[0,12]},[
        ...(folded?[icon(`${id}-${key}-fold`,'chevron-right',{size:12,fill:'$--muted-foreground'})]:[]),
        caption(`${id}-${key}-label`,title,key==='needs'?'$--warning':'$--subtle-foreground',{weight:'500'}),
        caption(`${id}-${key}-group-count`,String(count ?? rows.length)),
      ]),
      ...(folded?[]:rows.map((spec,index)=>row(`${id}-${key}-${index}`,spec,width))),
      ...(more?[frame(`${id}-${key}-more`,'Folded rest',{width:'fill_container',height:28,layout:'horizontal',gap:6,alignItems:'center',padding:[0,12,0,24]},[
        icon(`${id}-${key}-more-g`,'chevron-right',{size:12,fill:'$--muted-foreground'}),caption(`${id}-${key}-more-l`,`그 외 ${more}`,'$--subtle-foreground'),
      ])]:[]),
    ]);
    return frame(id,'Sessions panel',{width,height:760,layout:'vertical',gap:8,fill:'$--card',stroke:'$--border',strokeWidth:1,strokeAlignment:'inner',clip:true},[
      frame(`${id}-tabs`,'Tools tabs',{width:'fill_container',height:32,layout:'horizontal',gap:12,padding:[0,12],alignItems:'center'},[
        caption(`${id}-tab-sessions`,'세션','$--foreground',{weight:'600'}),caption(`${id}-tab-explorer`,'Explorer'),caption(`${id}-tab-history`,'History'),
      ]),
      frame(`${id}-scope`,'Project and checkout scope',{width:'fill_container',layout:'horizontal',gap:8,padding:[8,12],alignItems:'center'},[
        caption(`${id}-project`,'herdr-ide','$--foreground',{weight:'600',width:'fill_container'}),
        screenBadge(`${id}-all`,'모든 체크아웃'),screenBadge(`${id}-front`,'main만',{variant:'outline'}),
      ]),
      group('needs','Needs You',[
        {title:'hide 에이전트 지원 PR 묶음 머지 조율',mark:'!',tone:'$--warning',open:false,age:'12m',ask:['승인','e2e 테스트 돌리던 중']},
      ]),
      group('working','Working',[
        {title:'공통 후보 전체 검증과 PR 제출',provider:'codex',pr:{number:874,state:'failed'},age:'4h',line:'sealed verify 다시 돌리는 중'},
        {title:'Mac mini 연동 5단계 순차 구현',mark:'○',open:true,age:'2m',line:'4단계 리뷰 지적 수정 중'},
        {title:'Implementor',depth:1,pr:{number:1188,state:'pending'},desc:['working',1],age:'2m'},
        {title:'사전 리뷰',depth:1,last:true,mark:'✓',tone:'$--success',age:'40m'},
        {title:'에이전트 패널 표시 규칙과 UI 구조 정리안',age:'38s',line:'Sessions 분류 리뷰 3건을 v3에 반영 중'},
      ],{count:3}),
      group('done','Done',[
        {title:'Factory 워커 시작 지연 수정',mark:'✓',tone:'$--success',pr:{number:872,state:'pending'},age:'6m',line:'진단 로그 추가하고 PR 올림'},
      ]),
      group('idle','Idle',[
        {title:'P8 읽기 전용 준비 계획',provider:'codex',mark:'◐',tone:'$--warning',pr:{number:809,state:'failed'},age:'19h',line:'Grok 훅 연결 마무리 남음'},
        {title:'에이전트 막힘 상태 PRD',mark:'○',tone:'$--muted-foreground',pr:{number:861,state:'failed'},age:'3h'},
        {title:'세션 읽기 큰 레코드 건너뛰기',mark:'○',tone:'$--muted-foreground',pr:{number:869,state:'mergeable'},age:'2h'},
      ],{more:2,count:5}),
      group('resolved','Resolved',[],{folded:true,count:7}),
    ]);
  }

  // A pane: its header (the ancestor path, the marks and title, the own PR
  // chip and the tree button, then menu, zoom and close) over its terminal,
  // with the one band a pane carries overlaid at the top (D-14, D-43; B20, B21).
  function pane(id, {path = [], mark = '○', tone = '$--agent-working', provider: kind = 'codex', title, pr, children, band, terminal}) {
    const header = frame(`${id}-header`, 'Header', {width:'fill_container',height:28,layout:'horizontal',gap:6,alignItems:'center',padding:[0,8],fill:'$--card'}, [
      ...path.flatMap((name, index) => [caption(`${id}-path-${index}`, name, '$--muted-foreground'), caption(`${id}-sep-${index}`, '›', '$--muted-foreground')]),
      caption(`${id}-mark`, mark, tone, {mono:true,weight:'600'}),
      provider(`${id}-provider`, kind),
      caption(`${id}-title`, title, '$--foreground'),
      ...(pr ? [prChip(`${id}-pr`, pr)] : []),
      ...(children ? [themedXref(`${id}-tree`, 'ath-tree-button', 'Tree button', {}, {'ath-tb-n': {content: String(children)}})] : []),
      frame(`${id}-sp`, 'Spacer', {width:'fill_container',height:1}, []),
      icon(`${id}-zoom`, 'maximize-2', {size:12,fill:'$--subtle-foreground'}),
      icon(`${id}-menu`, 'ellipsis', {size:14,fill:'$--subtle-foreground'}),
      icon(`${id}-close`, 'x', {size:14,fill:'$--subtle-foreground'}),
    ]);
    const bandRow = band ? [frame(`${id}-band`, 'Ask band', {width:'fill_container',height:28,layout:'horizontal',gap:6,alignItems:'center',padding:[0,8],fill:'$--secondary',stroke:'$--warning',strokeWidth:{left:2},strokeAlignment:'inner'}, [
      verb(`${id}-band-verb`, band.verb),
      caption(`${id}-band-what`, band.what, '$--foreground'),
      ...(band.who ? [provider(`${id}-band-who-p`, band.who[0]), caption(`${id}-band-who`, band.who[1], '$--muted-foreground')] : []),
      ...(band.unreceived ? [caption(`${id}-band-unreceived`, band.unreceived, '$--muted-foreground')] : [caption(`${id}-band-age`, band.age ?? '12m', '$--muted-foreground', {mono:true})]),
      ...(band.more ? [caption(`${id}-band-more`, `외 ${band.more}건`, '$--muted-foreground')] : []),
      frame(`${id}-band-sp`, 'Spacer', {width:'fill_container',height:1}, []),
      screenButton(`${id}-band-open`, '열기', {variant:'outline', height:20}),
    ])] : [];
    return frame(id, 'Pane', {width:'fill_container',height:120,layout:'vertical',gap:0,clip:true,fill:'$--background',stroke:'$--border',strokeWidth:1,strokeAlignment:'inner'}, [
      header, ...bandRow,
      frame(`${id}-terminal`, 'Terminal', {width:'fill_container',height:'fill_container',padding:[8,12]}, [caption(`${id}-terminal-text`, terminal ?? '› 테스트 작성 결과를 기다리는 중', '$--muted-foreground', {mono:true})]),
    ]);
  }
  function headerBoard(suffix) {
    const id=`sfu-ph-board-${suffix}`;
    const root='hide 에이전트 지원 PR 묶…';
    const bands=[
      ['approval',{verb:'승인',what:'권한 요청에서 멈춤',who:['codex','테스트 작성'],age:'12m'}],
      ['answer',{verb:'답변',what:'PR을 하나로 합칠까요, 나눌까요?',who:['claude','P7 Pi·omp 재우기'],unreceived:'hide 에이전트 지원 PR… 60분째 못 받음'}],
      ['confirm',{verb:'확인',what:'리뷰 지적 3건 반영하고 멈춤',who:['claude','사전 리뷰'],age:'1h'}],
      ['draft',{verb:'초안',what:'내 입력 초안 때문에 질문이 못 감',who:['codex','P8 읽기 전용 준비'],age:'8m'}],
    ];
    // One column at the approved board's width, each pane under the line that says what it shows.
    const labeled = (key, label, children) => frame(`${id}-sec-${key}`, label, {width:'fill_container',layout:'vertical',gap:8}, [caption(`${id}-sec-${key}-label`, label, '$--subtle-foreground', {weight:'600'}), ...children]);
    return frame(id,'Pane header states',{width:761,layout:'vertical',gap:20,padding:[0,0,24,0],fill:'$--background'},[
      labeled('root','Root pane, a descendant raised: band with 외 1건 (B4, B6)',[pane(`${id}-root`,{mark:'!',tone:'$--warning',provider:'claude',title:'hide 에이전트 지원 PR 묶음 머지 조율',pr:{number:812,state:'mergeable'},children:8,band:{verb:'승인',what:'e2e 테스트 돌리던 중',who:['codex','테스트 작성'],age:'12m',more:1},terminal:'› 에이전트 지원 PR 다섯 개의 머지 순서를 맞추는 중'})]),
      labeled('child','Child pane: path back to the root, its own PR and its children after the title (B20, B21)',[pane(`${id}-child`,{path:[root],title:'P8 읽기 전용 준비',pr:{number:811,state:'failed'},children:2})]),
      labeled('grand','Grandchild pane: two ancestors, no PR and no children (B20)',[pane(`${id}-grand`,{path:[root,'P8 읽기 전용 준비'],mark:'!',tone:'$--warning',title:'테스트 작성',terminal:'Allow bash: pnpm --dir web e2e ? (y/n)'})]),
      labeled('two','Two own PRs: PR 2 ×1 (B22)',[pane(`${id}-two`,{path:[root],mark:'✓',tone:'$--success',provider:'claude',title:'P6 OpenCode 세션 리더',pr:{count:2,state:'failed',worst:1}})]),
      labeled('bands','Band per cause (B3)',bands.map(([key,band])=>pane(`${id}-band-${key}`,{path:[root],title:'P8 읽기 전용 준비',band}))),
      caption(`${id}-rule`,'승인은 label 줄이 없으면 "권한 요청에서 멈춤". 답변의 대기는 부모가 못 받은 시간. 초안은 초안이 걸린 부모를 연다. 띠는 PTY 크기를 바꾸지 않는다.'),
    ]);
  }
  return {panel,headerBoard};
}
