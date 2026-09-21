---
topic: "위임된 자식 에이전트의 노출 경계: 부모 배지, 기본 접힘, 역할 열, stall clock 삭제"
status: "ready"
human_approval: "approved"  # user 2026-09-21 verbatim: ㅇㅇ 승인 /implement 시작해 opus5로
review_profile: "standard"
review_rationale: "사이드바 행의 그룹·read 축·트리 접힘이라는 사용자 가시 계약과 persisted UI 상태 키 하나가 바뀌지만, 데이터 파괴·인증·외부 효과는 없다."
source_intake: "agents/interview/delegated-agent-boundary/qa-log.md"
created_at: "2026-09-21"
updated_at: "2026-09-21"
---

# PRD: 위임된 자식 에이전트의 노출 경계

## Goal

Hide 운영자가 에이전트에게 일을 위임하면 자식 pane이 생기는데, 지금은 자식이 정상적으로 일하는 동안에도 5분 뒤 부모 행에 "N minutes on no visible progress" 문장이 붙고 15분에 부모가 Needs You로 올라온다. hook이 turn 단위로만 보고하므로 긴 turn은 예외 없이 이 경고가 되어, 경고가 "고장"과 "정상"을 구분하지 못한다. 이 변경은 "현황은 매니저(부모)에게 묻는다"를 원칙으로 삼아 stall clock을 없애고, 부모 행이 후손 집계 배지로 보고하며, 후손의 요구 변화나 완료가 조상 행을 다시 unread로 켜고, 후손은 기본 접힘이며, 자식 행은 역할 열의 글리프 하나로 "누군가의 것"임을 말하게 한다. 운영자가 확인할 문장: **자식 에이전트는 트리에 남되 운영자를 부르지 않고, 부모 행의 배지와 unread가 그 현황을 보고한다.**

## Non-goals

- 자식 행을 트리에서 없애고 부모 안에 합치는 것. 자식은 실제 pane이고 운영자가 막힌 프롬프트를 보러 들어갈 일이 있다 (Q1). 재검토: 자식 pane을 직접 볼 일이 없어졌다는 관찰이 쌓일 때.
- 시간 임계값을 줄이거나 조건만 좁힌 stall clock 유지 (Q3 (c) 거부). 5/15분이라는 숫자가 맞는지 알 방법이 없어 이번 오탐이 났다. 재검토: 조상 unread 규칙으로도 방치된 자식을 놓치는 사례가 보고될 때.
- 자식의 요구를 자식 행 자체가 Needs You로 올리는 것 (Q3 대안 A 거부). 자식 질문은 부모 일이며, sasu Observer가 답한다. 재검토: sasu 밖에서 띄운 자식이 방치되는 사례가 반복될 때.
- 부모의 progress 문장을 Hide가 자식 상태로 덧쓰는 것. 문장은 부모 에이전트가 쓴다 (D-05). 재검토 없음.
- 다른 worktree의 자식을 worktree 밑에서 지우고 부모 밑에만 두는 것 (Q7 (b) 거부). worktree chip이 세는 에이전트는 펼쳐볼 수 있어야 한다. 재검토: 기본 접힘 이후에도 중복이 문제로 남을 때.
- Claude 내부 subagent 카운터(`hide_sub_working`)를 herdr 자식과 합치는 것. 별개 건.
- 저장된 접은-pane 집합의 이전 (D-16). 업그레이드 후 첫 실행은 모두 접힌 채 시작한다.
- `design/principles.md` 규칙 11(구조적으로 다른 후보 제시)은 pen 보드 A~D로 이미 수행했고, 규칙 12(실제 한글·혼합 스크립트 폭 검증)는 native 스크린샷 검증에서 확인한다; 별도 행 없음.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 자식 에이전트는 팀원(독립 pane)이되 현황은 매니저(부모)에게 묻는다. 자식 행은 트리에 남아 들어갈 수 있고 운영자를 부르지 않는다. "위임된 행은 Working/Seen만" 규칙은 유지한다. (a) 내부 구현으로 숨김, (c) 조건부 노출은 거부. | Q1 (b) "매니저한테 물어보지 일일이 말단에게 물어보지않자나" |
| D-02 | 부모 행에 후손 집계 배지를 둔다: 상태별 마크와 수, 접힌 checkout 요약 pill과 같은 문법. Hide가 파생하며 부모의 progress 문장과 독립. 문장형 알림(c)은 거부. | Q2 (b) |
| D-03 | stall clock을 전부 삭제한다: `StallClock`, 5/15분 임계값, escalation, `stall_notice`/`stall_level`, ownership 축의 `escalated` 값, AgentRow 알림 UI, 관련 테스트. 대신 후손의 요구(question/approval/error) 변화 또는 완료(요구 없이 stopped)가 모든 조상 행의 read 축을 unread로 되돌린다. 자식이 일을 시작하는 변화는 켜지 않는다. 부모 자신의 demand/activity는 불변이므로 자식 때문에 Needs You로 가지 않고 최대 Done이다. | Q3 (a), Q4 (a), Q5 "a로 가자", Q12 기본값 2 승인 |
| D-04 | 배지 위치는 1줄 오른쪽, 경과 시간 앞(트리 행·raised 행 공통). 부모의 후손이 펼쳐져 있으면 배지를 숨긴다(자식 행이 자기 상태를 든다). raised 행은 후손을 펼치지 않으므로 항상 배지. (b) 3줄 오른쪽, (c) 2줄 앞은 거부. | Q6/Q7 (a) "괜찮은데", Q8 |
| D-05 | 배지는 모든 후손을 합산한다(직계만이 아님). | Q9 "a로 가게 하자" |
| D-06 | 부모의 후손은 기본 접힘. 저장은 "접은 집합"에서 "펼친 집합"으로 뒤집어 UI 상태에 pane id로 두고, 지금의 접은 집합과 같은 수명을 가진다: Herdr 서버가 살아 있는 동안 Hide를 다시 실행하면 유지되고, Herdr 서버가 재시작되어 pane id가 새로 나면 그 id는 더 이상 어떤 행과도 맞지 않아 모두 접힌 채 시작하며, 사라진 pane의 항목은 read 기록과 같은 pass에서 정리된다. 새 자식이 생겨도 부모는 접힌 채 배지만 갱신. | Q9 "기본을 자식이 숨겨지게", Q12 기본값 3 승인 |
| D-07 | 다른 worktree에서 도는 자식은 부모 밑(펼쳤을 때)과 자기 worktree 밑 둘 다에 나온다(기존 규칙). 자기 worktree 밑의 자식 행은 맨 앞 역할 열에 ↳ 글리프(호버: 부모 이름, 클릭: 부모로 이동), 제목 muted. 부모 밑의 자식은 트리선과 qualifier 슬롯의 worktree 이름. 행 밖에 떠 있던 `lineageWorktreeBadge` 칩은 qualifier 슬롯으로 옮긴다. 텍스트 qualifier(`↳ from …`) 안은 거부. | Q10 "a로 할까", Q11 "이걸로 우선 해볼까" |
| D-08 | 가정(사용자 승인): 배지 마크는 기존 상태 마크 재사용(`?`, 승인·에러 마크, ● working, ✓ done), 0인 상태는 그리지 않음, 순서는 에러 > 승인 > 질문 > working > done. 새 글리프 없음. | Q12 기본값 1 "ㅇㅇ 괜찮아" |
| D-09 | 삭제 범위는 stall 표시가 남은 모든 표면(pane 헤더, Project Home 카드, Overview, pet). `docs/status-model.md`의 stall clock 절과 ownership 축을 같은 PR에서 갱신. | Q12 기본값 4 "ㅇㅇ 괜찮아" |
| D-10 | 디자인 승인: `agents/runs/delegated-agent-boundary/design/scratch.pen`의 'Scratch / delegated agent boundary' 열 B(기본 접힘+배지), C(펼침), D(자식 질문 전후)가 구현 목표, 열 A는 현재 대조. 배지는 elevated 배경 알약에 마크+mono 수, 역할 열은 chevron-right / chevron-down / corner-down-right, 펼친 자식의 qualifier는 git-branch 아이콘+worktree 이름. 보드는 구현 시작 시 `agents/runs/<slug>/design/`으로 내보낸다. | "봤어 괜찮네 이대로 PRD 써줘" |
| D-11 | 저장된 접은-pane 집합은 이전 없이 버린다: 로드 시 무시하고 새 키로 대체하므로 업그레이드 후 첫 실행은 모든 부모가 접힌 채 시작한다. pane id는 Herdr 서버 재시작마다 바뀌어 옛 집합의 보존 가치가 낮다. stall 상태는 메모리 전용이라 지울 저장물이 없다. | Q16 "둘 다 그대로 가자" (gap-audit F1) |
| D-12 | 배지는 현재 live한 후손만 센다(닫힌 pane은 즉시 제외). pane 닫힘 자체는 조상을 켜지 않는다. 부모가 사라진 고아는 기존 고아 규칙대로 root가 되어 어느 배지에도 들지 않는다. 재시작 후 read 기록은 PR #115의 `pane_read_records` 유지 규칙을 따르고, 후손 변화로 만든 unread도 같은 기록이라 함께 유지된다. | Q16 "둘 다 그대로 가자" (gap-audit F2) |
| D-13 | 검증: 코어 자동 테스트(후손 요구/완료 변화 → 조상 unread, 시작은 아님; 부모 그룹은 자신의 축만; 배지가 모든 live 후손 합산; stall 필드 부재), Swift presentation 테스트(배지 표시/숨김, 역할 열 글리프, qualifier 슬롯), 격리 Herdr 서버의 dev 앱 native 스크린샷으로 B/C/D 확인. 기존 stall 테스트는 삭제. | 가정: 저장소 관행(`docs/status-model.md` regression owners, CONTRIBUTING lanes) |
| D-14 | 전달: `agents/config.json`대로 worktree에서 구현하고 PR로 전달, CI 감시. | `agents/config.json` delivery.mode=pr |
| D-15 | 원칙 intake(oh-my-principle 654485f): engineering 전부 읽음 - 1(stall 코드·테스트·문서를 같은 변경에서 삭제, B14), 2(새 저장소 없이 read 지문 확장, 기술 구조), 4·10(알 수 없는 후손 상태는 세지 않고 진단 로그, B7), 7(기존 마크·pill·되돌아가기 버튼 재사용, B4·B10), 12(테스트는 밖에서 정한 그룹·read 답을 단언, D-13), 13(임계값 대신 상태 모델링, D-03). design 전부 읽음 - 3(↳ 클릭 한 번, B11), 4(배지는 파생 상태, B4), 5(pill 문법·되돌아가기 재사용), 7(문장 대신 마크·글리프, B3·B10), 8(새 컨테이너 없음), 9·13(운영자가 행동할 상태만: 정상 작업은 무표시, B1), 10(배지 수는 스냅샷에서만), 11·12는 비목표에 기록. | `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 위임된 자식이 얼마나 오래 같은 상태로 일하든, 부모 행에는 문장형 알림이 붙지 않고 부모의 그룹은 자식 때문에 바뀌지 않는다. 스냅샷에 `stall_notice`·`stall_level` 필드와 `escalated` ownership 값이 없다. | D-03, D-09 |
| B2 | 위임된 자식 행은 여전히 Working 또는 Seen에만 자리하고 Needs You·Done에는 들어가지 않으며, 제목은 muted로 그려지고 자기 마크(●, ?, ✓ 등)를 단다. | D-01 |
| B3 | 후손이 하나 이상인 행의 1줄 오른쪽, 경과 시간 앞에 배지가 있다: elevated 배경 알약 안에 상태 마크와 mono 수가 상태별로 하나씩, 0인 상태는 없고, 순서는 에러 > 승인 > 질문 > working > done. 후손이 없는 행에는 배지가 없다. | D-02, D-04, D-08 |
| B4 | 배지는 직계뿐 아니라 모든 live 후손을 센다. 손자의 질문은 root의 배지에 `?1`로 나타난다. 닫힌 pane은 즉시 빠진다. | D-05, D-12 |
| B5 | 후손의 요구가 none에서 question/approval/error 중 하나로 들어가거나 그 셋 사이에서 바뀌면 모든 조상 행이 unread가 된다: idle+read였던 조상은 Seen의 어두운 행에서 Done의 밝은 행으로 올라오고, Working 중인 조상은 이미 밝으므로 배지만 바뀐다. 위임된 중간 조상은 Seen에서 밝아지기만 한다. 후손의 요구가 해소되어 none으로 돌아가는 변화(답을 받아 다시 working이 되는 것)는 조상을 켜지 않고 배지만 바뀐다. | D-03 |
| B6 | 후손이 working에서 요구 없이 stopped가 되면(완료) 모든 조상 행이 unread가 된다. 조상을 켜는 전이는 B5의 요구 진입·전환과 이 완료뿐이다: 후손이 stopped → working으로 일을 시작하는 변화, 요구가 none으로 해소되는 변화, 후손 pane이 닫히는 것은 조상을 켜지 않는다. | D-03, D-12 |
| B7 | 자식 때문에 조상이 Needs You에 들어가는 일은 없다. 조상의 그룹은 조상 자신의 demand·activity·read로만 정해진다. 활동을 알 수 없는(unknown) 후손은 배지에 세지 않고 진단 로그에 남긴다. | D-03 |
| B8 | 운영자가 조상 행을 포커스하면 read 축만 read가 되고 그룹은 조상 자신의 축으로 정해진다: idle이고 자기 요구가 없으면 Done에서 Seen으로 내려가고, Working이면 Working에 남으며, 자기 요구가 있으면 기존 규칙대로(읽은 질문·승인·에러는 Seen, blocked는 답할 때까지 Needs You). 배지는 그대로 남는다. 그 뒤 후손이 B5·B6의 전이를 다시 만들면 다시 unread가 된다. | D-03 |
| B9 | 부모 행의 후손은 기본 접힘이다: 역할 열에 `>`가 있고 자식 행은 보이지 않으며 배지가 보인다. 운영자가 펼치면(`⌄`) 배지가 사라지고 자식 행이 트리선 밑에 자기 마크·detail과 함께 나타난다. 펼침 상태는 같은 Herdr 서버에 대해 Hide를 다시 실행해도 유지되고, Herdr 서버가 재시작되어 pane id가 바뀌면 모두 접힌 채 시작한다. 새 자식이 생겨도 접힌 부모는 접힌 채 배지만 갱신된다. 업그레이드 후 첫 실행에서는 모든 부모가 접혀 있다. | D-06, D-11 |
| B10 | 자기 worktree 헤더 밑에 나오는 자식 행은 역할 열에 ↳ 글리프를 단다. 호버하면 부모 이름 툴팁(동일한 접근성 도움말), 클릭하면 부모 pane으로 이동한다. 부모가 사라진 고아는 기존 고아 힌트 규칙을 따른다. | D-07 |
| B11 | 부모를 펼쳤을 때 다른 worktree에서 도는 자식 행은 qualifier 슬롯에 git-branch 아이콘과 worktree 이름을 단다. 행 밖에 별도로 떠 있던 worktree 칩은 없다. 같은 worktree의 자식은 qualifier가 비어 있다. | D-07 |
| B12 | raised 행(Needs You·Done 섹션)은 후손을 펼치지 않으므로 항상 배지를 보이고, 역할 열은 트리와 같은 글리프(`>` 또는 빈칸)를 쓴다. | D-04 |
| B13 | Overview·Project Home·pane 헤더·pet 등 다른 표면은 stall 문장·시계 아이콘·escalated 상태를 더 이상 그리지 않으며, 그 외 lineage 표시는 지금과 같다. | D-09 |
| B14 | `docs/status-model.md`는 stall clock 절 대신 "후손 변화가 조상을 unread로" 규칙을 담고, ownership 축은 operator/delegated 두 값이며, regression owner 테스트 이름이 실제 테스트와 일치한다. `AGENTS.md`의 "docs/status-model.md owns the ownership axis and the stall clock" 문구와 `DESIGN.md`의 에이전트 행 절도 맞춘다. | D-09 |
| B15 | 격리된 Herdr 서버의 dev 앱에서 보드 B(접힘+배지), C(펼침), D(자식 질문 전후) 세 상태가 실제로 그려지고, 긴 한글 제목과 배지·경과 시간이 한 줄에서 잘림 규칙대로 공존한다. | D-10, D-13 |

## Technical structure

- 코어(`herdr-core`): `StallClock`·임계값·`stall_escalations`·`stall_publish_due`·`stall_tick_needs_publish`와 스냅샷의 `stall_notice`/`stall_level` 필드, ownership `escalated` 값을 삭제한다. 대신 `sidebar.rs`의 read 지문(`PaneReadRecord`)에 후손의 (demand, completed) 집계를 한 요소로 더해, 후손 변화가 그 행 자신의 상태 변화와 같은 경로로 unread를 만든다. 새 저장소·타이머·이벤트 없음. 배지 집계(`descendant_counts`)는 lineage를 세우는 같은 pass에서 파생되어 스냅샷 행에 실린다.
- 저장(`persistence.rs`/`ui_state`): `collapsed_agent_pane_ids`를 `expanded_agent_pane_ids`로 대체한다. 옛 키는 로드 시 무시하고 다음 저장에서 사라진다. 토글 이벤트 payload도 같은 이름으로 바뀐다. 항목의 정체성은 지금과 같은 pane id이며, 사라진 pane의 항목은 read 기록 정리와 같은 pass에서 제거된다.
- 셸(`macos`): `AgentRow`의 stallNotice 슬롯을 삭제하고 1줄에 후손 배지 뷰를 추가한다(`WorkspaceAgentSummary`의 pill 스타일 재사용). `HideSidebar`의 역할 열이 `>`/`⌄`/`↳`/빈칸을 그리고, ↳는 기존 되돌아가기 버튼의 동작·툴팁을 쓴다. `lineageWorktreeBadge` 별도 뷰는 qualifier 슬롯으로 흡수. 필요한 새 토큰(배지 안 마크 크기 등)은 `HideTheme`에 먼저 추가한다.
- 문서: `docs/status-model.md`, `AGENTS.md` Runtime Architecture 항목, `DESIGN.md` 에이전트 행 절.
- 바뀌지 않는 것: Herdr API 사용, lineage 출처(`parent_pane` 토큰), 위임 행의 그룹 규칙, `pane_read_records`의 저장 형식과 재시작 유지 규칙(#115).

## Risks

- read 지문에 후손 집계를 넣으면 후손이 많은 root의 지문이 자주 바뀔 수 있다. 바운드: 지문은 요구 변화와 완료만 반영하고 활동 시작·pane 닫힘은 제외한다(B6). 스냅샷 publish는 실제 상태 전이에만 일어난다(Performance Guide).
- 기본 접힘으로 바꾸면 운영자가 자식이 어디 갔는지 잠시 헤맬 수 있다. 바운드: 배지가 접힌 행에 항상 보이고, 다른 worktree의 자식은 worktree 밑에서 ↳로 여전히 보인다.
- sasu 밖에서 띄운 자식이 질문한 채 방치되면 부모는 Done까지만 올라온다. 이는 결정(D-03)이며, 방치 사례가 쌓이면 비목표의 재검토 조건이 열린다.
- native 스크린샷 검증은 격리 Herdr 서버(`HERDR_SOCKET_PATH`)와 dev 앱의 한 창을 대상으로 하고, 운영자의 앱·pane·서버는 건드리지 않는다.
- 사용자가 구현 전에 해야 할 일: 없음.
