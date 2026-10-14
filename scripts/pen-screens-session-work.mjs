// The operator-approved Sessions record and its summary-off and failure states.
// Controls reference the existing library through pen-screens' themed helpers.
export function sessionWorkScreens({frame, text, icon, screenButton}) {
  const label = (id, content, fill='$--foreground', extra={}) => text(id,content,{size:'$--text-caption',fill,...extra});
  const line = (id, content, fill='$--foreground') => frame(id,content,{width:'fill_container',layout:'horizontal',gap:'$--spacing-xs',padding:[4,0]},[label(`${id}-text`,content,fill,{width:'fill_container'})]);
  const group = (id,title,children) => frame(id,title,{width:'fill_container',layout:'vertical',gap:'$--spacing-xs',padding:[8,0]},[label(`${id}-heading`,title,'$--subtle-foreground',{weight:'500'}),...children]);
  function record(suffix,{off=false,width=420}={}) {
    const id=`swr-${off?'off':'record'}-${suffix}`;
    return frame(id,off?'Summaries off':'Session record',{width,height:900,layout:'vertical',gap:8,padding:12,fill:'$--background',clip:true},[
      screenButton(`${id}-back`,'세션',{variant:'ghost'}),
      label(`${id}-title`,off?'기록 화면 구현':'세션 작업 기록과 검색 구현','$--foreground',{size:'$--text-subhead',weight:'600'}),
      label(`${id}-state`,'쉬는 중 · 1개 자식','$--muted-foreground'),
      ...(!off?[frame(`${id}-verdict`,'Verdict',{width:'fill_container',layout:'vertical',gap:8,padding:8,cornerRadius:'$--radius-sm',fill:'$--card'},[
        label(`${id}-counts`,'1/4 끝남 · 하는 중 0 · 남음 3','$--success',{weight:'600'}),
        frame(`${id}-bar`,'Work verdict',{width:'fill_container',height:4,layout:'horizontal',gap:2},[frame(`${id}-done`,'Done',{width:90,height:4,fill:'$--success',cornerRadius:'$--radius-xs'},[]),frame(`${id}-left`,'Left',{width:'fill_container',height:4,fill:'$--muted-foreground',cornerRadius:'$--radius-xs'},[])]),
      ])]:[]),
      group(`${id}-outputs`,'결과물',[
        line(`${id}-pr1`,'✓ PR #121 Sessions 기록 화면','$--pr-merged'),
        line(`${id}-pr2`,'PR #122 자식 결과물 합치기 · CI 실패','$--destructive'),
        label(`${id}-source`,'하위 작업 검증에서 만듦','$--muted-foreground'),
      ]),
      ...(!off?[group(`${id}-missed`,'놓친 일 · 2',[
        line(`${id}-leftover`,'좁은 Tools 칸에서 긴 한국어 요청의 줄바꿈 확인'),
        frame(`${id}-action`,'Missed actions',{layout:'horizontal',gap:4},[screenButton(`${id}-issue`,'이슈로',{variant:'ghost'}),screenButton(`${id}-dismiss`,'넘기기',{variant:'ghost'})]),
        line(`${id}-problem`,'이슈 생성 실패 후 재시도 검증'),
        line(`${id}-closed`,'› 닫힘 2','$--muted-foreground'),
      ])]:[]),
      group(`${id}-work`,'시킨 일',off?[
        line(`${id}-raw2`,'세션 검색을 연결하고 좁은 칸에서도 읽기 좋게 해줘'),
        line(`${id}-raw1`,'기록에 결과물과 놓친 일을 보여줘'),
      ]:[
        label(`${id}-chapter1`,'세션 작업 기록 구현 · 09:00','$--subtle-foreground',{weight:'500'}),
        line(`${id}-task1`,'✓ Sessions 기록 화면 · PR #121','$--subtle-foreground'),
        line(`${id}-task2`,'◐ 자식 결과물 합치기 · CI 실패','$--destructive'),
        label(`${id}-chapter2`,'세션 검색과 놓친 일 정리 · 09:01','$--subtle-foreground',{weight:'500'}),
        line(`${id}-task3`,'◐ ⌘K 세션 검색 · 남음','$--warning'),
        line(`${id}-task4`,'⛔ 이슈 미리보기와 재시도 · 막힘','$--warning'),
        line(`${id}-raw`,'검색 결과에서 어떤 기록을 여는 거야?','$--muted-foreground'),
      ]),
      group(`${id}-children`,'자식 · 1',[frame(`${id}-child`,'Delegated child',{width:'fill_container',layout:'horizontal',gap:8},[icon(`${id}-fold`,'chevron-right',{size:12,fill:'$--muted-foreground'}),label(`${id}-child-title`,'하위 작업 검증','$--foreground',{width:'fill_container'}),label(`${id}-child-state`,'쉬는 중','$--muted-foreground')])]),
    ]);
  }
  function states(suffix) {
    const id=`swr-states-${suffix}`;
    return frame(id,'Loading, failure, ended and issue preview',{width:420,layout:'vertical',gap:16,padding:12,fill:'$--background'},[
      group(`${id}-loading`,'읽는 중',[frame(`${id}-skeleton`,'Skeleton',{width:'fill_container',height:32,fill:'$--muted',cornerRadius:'$--radius-sm'},[])]),
      group(`${id}-failure`,'기록을 읽지 못함',[screenButton(`${id}-retry`,'다시 읽기',{variant:'ghost'})]),
      group(`${id}-ended`,'› 놓친 일 2',[line(`${id}-ended-row`,'세션 검색과 놓친 일 정리 · 2')]),
      group(`${id}-search`,'세션',[line(`${id}-search-title`,'세션 작업 기록과 검색 구현'),label(`${id}-matched`,'세션 검색을 연결하고 좁은 칸에서도 읽기 좋게 해줘','$--muted-foreground',{width:'fill_container'})]),
      group(`${id}-draft`,'새 이슈',[
        label(`${id}-repo`,'example/hide','$--muted-foreground'),
        label(`${id}-note`,'내용을 확인하고 수정한 뒤 만들어 주세요.','$--subtle-foreground'),
        line(`${id}-draft-title`,'좁은 Tools 칸에서 한국어 요청 줄바꿈 확인'),
        label(`${id}-body`,'확인할 일\n긴 한국어 요청이 좁은 Tools 칸에서도 잘리지 않는지 확인합니다.\n\n완료 조건\nLight와 Dark에서 가로 넘침 없이 읽을 수 있습니다.','$--foreground',{width:'fill_container'}),
        frame(`${id}-footer`,'Explicit creation',{width:'fill_container',layout:'horizontal',gap:8,justifyContent:'end'},[screenButton(`${id}-cancel`,'취소',{variant:'secondary'}),screenButton(`${id}-create`,'이슈 만들기')]),
      ]),
    ]);
  }
  return suffix => [record(suffix),record(suffix,{off:true,width:360}),states(suffix)];
}
