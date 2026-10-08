// Approved v6 Sessions and v9 pane-header compositions. Controls are refs of
// the existing System masters supplied by pen-screens, including themed colors.
export function sessionScreens(tokens, {frame, text, icon, themedXref, screenButton, screenBadge, screenIconButton}) {
  const caption = (id, value, fill = '$--muted-foreground', extra = {}) => text(id, value, {size:'$--text-caption', fill, ...extra});
  const row = (id, spec, width) => frame(id, spec.title, {
    width, layout:'vertical', gap:4, padding:[8,12],
    ...(spec.front ? {stroke:'$--primary', strokeWidth:{left:2}, strokeAlignment:'inner'} : {}),
  }, [
    frame(`${id}-head`, 'Identity and live facts', {width:'fill_container',layout:'horizontal',gap:6,alignItems:'center'}, [
      caption(`${id}-mark`, spec.mark ?? '●', spec.tone ?? '$--agent-working'),
      frame(`${id}-provider`, 'Provider', {width:14,height:14,fill:{type:'image',enabled:true,url:'../web/src/assets/agent-codex.png',mode:'contain'}}, []),
      caption(`${id}-title`, spec.title, '$--foreground', {width:'fill_container',weight:'500'}),
      ...(spec.issue ? [screenBadge(`${id}-issue`, `#${spec.issue}`, {variant:'outline'})] : []),
      ...(spec.pr ? [screenBadge(`${id}-pr`, `#${spec.pr} ${spec.checks ?? '✓'}`, {variant:'outline'})] : []),
      ...(spec.children ? [themedXref(`${id}-children`, 'PaMPI', 'Direct child badge')] : []),
      caption(`${id}-age`, spec.age ?? '3m'),
    ]),
    frame(`${id}-line`, 'Task, last line and location', {width:'fill_container',layout:'horizontal',gap:6,alignItems:'center'}, [
      ...(spec.tag ? [caption(`${id}-tag`, spec.tag, spec.tone ?? '$--subtle-foreground')] : []),
      caption(`${id}-summary`, spec.line ?? '', '$--subtle-foreground', {width:'fill_container'}),
      caption(`${id}-checkout`, spec.checkout ?? 'main'),
    ]),
  ]);
  function panel(suffix, width=480, id=`sfu-panel-${suffix}`) {
    const group=(key,title,rows,folded=false)=>frame(`${id}-${key}`,title,{width:'fill_container',layout:'vertical',gap:0},[
      frame(`${id}-${key}-heading`,'Group heading',{width:'fill_container',height:32,layout:'horizontal',gap:6,alignItems:'center',padding:[0,12]},[
        ...(folded?[icon(`${id}-${key}-fold`,'chevron-right',{size:12,fill:'$--muted-foreground'})]:[]),
        caption(`${id}-${key}-label`,title,key==='turn'?'$--warning':'$--subtle-foreground',{weight:'500'}),
        caption(`${id}-${key}-group-count`,String(rows.length)),
      ]),
      ...(folded?[]:rows.map((spec,index)=>row(`${id}-${key}-${index}`,spec,width))),
    ]);
    return frame(id,'Sessions panel',{width,height:760,layout:'vertical',gap:8,fill:'$--card',stroke:'$--border',strokeWidth:1,strokeAlignment:'center',clip:true},[
      frame(`${id}-tabs`,'Tools tabs',{width:'fill_container',height:32,layout:'horizontal',gap:12,padding:[0,12],alignItems:'center'},[
        caption(`${id}-tab-sessions`,'세션','$--foreground',{weight:'600'}),caption(`${id}-tab-explorer`,'Explorer'),caption(`${id}-tab-history`,'History'),
      ]),
      frame(`${id}-scope`,'Project and checkout scope',{width:'fill_container',layout:'vertical',gap:8,padding:[8,12]},[
        caption(`${id}-project`,'herdr-ide','$--foreground',{weight:'600'}),
        frame(`${id}-filters`,'Checkout filters',{layout:'horizontal',gap:8},[screenBadge(`${id}-all`,'모든 체크아웃'),screenBadge(`${id}-front`,'main만',{variant:'outline'})]),
        frame(`${id}-counts`,'Scope row counts',{width:'fill_container',layout:'horizontal',gap:12},[
          caption(`${id}-turn-count`,'내 차례 3','$--warning'),caption(`${id}-review-count`,'리뷰·머지 2'),caption(`${id}-progress-count`,'진행 중 2'),caption(`${id}-resolved-count`,'오늘 해결 1'),
        ]),
      ]),
      group('turn','내 차례',[
        {title:'입력과 세션 복귀 흐름 검토',mark:'!',tone:'$--warning',tag:'승인',line:'검증 명령 실행 권한이 필요합니다',front:true,issue:192},
        {title:'한국어 입력 경계 검토',mark:'?',tone:'$--warning',tag:'답하기',line:'기존 stdin 종료 경로도 남길까요?',checkout:'192-input'},
        {title:'자식 검토에서 답이 필요함',mark:'!',tone:'$--warning',tag:'승인',line:'↰ 입력 경계 검토',checkout:'review/input'},
      ]),
      group('review','리뷰·머지',[
        {title:'세션 상태 투영 정리',mark:'○',tone:'$--muted-foreground',tag:'머지',line:'CI 통과 · 승인됨',pr:221,checkout:'session-state'},
        {title:'오래된 PR의 검토',mark:'○',tone:'$--muted-foreground',tag:'리뷰',line:'세션 닫힘',pr:217,checks:'…',checkout:'review/history'},
      ]),
      group('progress','진행 중',[
        {title:'세션 패널 구현',line:'프로젝트 범위를 연결하는 중',pr:222,checks:'…',issue:201,front:true,children:true},
        {title:'회귀 테스트',line:'검증 결과를 기다리는 중',checkout:'test/session'},
      ]),
      group('rest','쉬는 중',[{}],true),group('resolved','오늘 해결',[{}],true),
    ]);
  }
  function headerBoard(suffix) {
    const id=`sfu-ph-board-${suffix}`;
    const cases=[
      ['휴면 중 · 12m','$--muted-foreground',null],['다시 시작하지 못함 · 세션을 찾을 수 없음','$--destructive',null],
      ['종료 코드 1','$--destructive',null],['다른 기기 연결 안 됨 · mini','$--muted-foreground',null],
      ['승인 · bash scripts/verify-cargo.sh test','$--warning',null],['답하기 · 기존 stdin 종료 경로도 남길까요?','$--warning',null],
      ['고치기 · verify 실패','$--destructive','PR 열기'],['머지 · checks 통과, 승인됨','$--pr-open','PR 열기'],
      ['↳ 한국어 입력 경계 검토 · 승인 +1','$--warning','열기'],['결과 보기 · 보고서가 준비됐습니다','$--success',null],
    ];
    const pane=(key,content,tone,action)=>frame(`${id}-${key}`,'Pane state',{width:560,height:144,layout:'vertical',gap:0,clip:true,fill:'$--background',stroke:'$--border',strokeWidth:1,strokeAlignment:'inner'},[
      themedXref(`${id}-${key}-identity`,'Z3BnL','Quiet identity',{width:'fill_container'}),
      frame(`${id}-${key}-terminal`,'Unchanged terminal viewport',{width:'fill_container',height:116,layout:'none'},[
        caption(`${id}-${key}-terminal-text`,'› 승인과 답은 터미널에서 계속합니다.','$--muted-foreground',{width:540}),
        ...(content?[themedXref(`${id}-${key}-band`,'BaBaS','Overlaid state band',{x:0,y:0,width:560,stroke:tone},{yTDmi:{fill:tone},GgeQQ:{content,fill:tone},E0B9l:{enabled:!!action},'E0B9l/btn-lb':{content:action??'열기'}})]:[
          frame(`${id}-${key}-working`,'Working line',{x:0,y:0,width:560,height:2,fill:'$--agent-working'},[]),
        ]),
      ]),
    ]);
    return frame(id,'Pane header v9 states',{width:1240,layout:'vertical',gap:20,padding:48,fill:'$--background'},[
      text(`${id}-title`,'pane 머리 C · 승인된 v9',{size:'$--text-headline',weight:'600'}),
      ...Array.from({length:5},(_,index)=>frame(`${id}-row-${index}`,'State comparison',{layout:'horizontal',gap:24,width:'fill_container'},cases.slice(index*2,index*2+2).map(([content,tone,action],offset)=>pane(`${index}-${offset}`,content,tone,action)))),
      frame(`${id}-quiet`,'Working and quiet states',{layout:'horizontal',gap:24},[pane('working',null,null,null),frame(`${id}-rest`,'Waiting / idle / shell',{width:560,layout:'vertical'},[themedXref(`${id}-rest-header`,'Z3BnL','Quiet identity',{width:560}),caption(`${id}-rest-note`,'CI 대기 · 기다림 · 쉼: 띠 없음')])]),
      caption(`${id}-rule`,'배지는 공통 직계 자식 목록을 엽니다. 띠는 PTY 크기를 바꾸지 않습니다.'),
    ]);
  }
  return {panel,headerBoard};
}
