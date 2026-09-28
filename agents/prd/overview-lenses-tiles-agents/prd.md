---
topic: "Overview 렌즈 1: 타일 머리와 Agents 체크아웃 · 계보"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "웹 셸의 Overview 화면 구조를 바꾸고 기존 워크트리 삭제 대화상자를 재사용할 뿐, 새 바깥 쓰기·인증·데이터 이동은 없다."
source_intake: "agents/interview/overview-lenses/qa-log.md"
created_at: "2026-09-28"
updated_at: "2026-09-28"
---

# PRD: Overview 렌즈 1: 타일 머리와 Agents 체크아웃 · 계보

## Goal

운영자는 hide에서 main의 Observer가 워크트리의 Implementor에게 일을 위임하는 방식으로 일한다.
지금(#218) Overview는 Tasks 보드가 첫 화면이고 에이전트는 인박스 묶음으로만 보여서, 어느 체크아웃에서 누가 무엇을 기다리는지 한눈에 읽히지 않는다.
이 PRD는 PR #218이 머지된 main 위에서 Overview의 탭 줄을 렌즈 타일로 바꾸고, Agents 탭을 체크아웃 레인(기본)과 계보 두 모드로 만들며, Overview의 첫 화면을 Agents › 체크아웃으로 둔다.
운영자가 확인할 한 문장: "Overview에 들어오면 체크아웃 레인이 보이고, main이 맨 위, 내 차례 레인이 그다음이며, 노란 노드를 누르면 그 에이전트 패널에서 답한다."
근거 보드: `agents/runs/overview-redesign/design/export-final/` ef-top, efs1, efs2, efs3, efs4, efs10, efs11, efs12, efs13.

## Non-goals

- Issues 탭의 이슈만 보이기·이슈 패널·카드 규칙은 PRD `overview-lenses-issues`가, PRs 탭·맡기기·이슈 잇기는 PRD `overview-lenses-prs`가 맡는다 (D-14, D-15). 이 PR에서 이슈 칩은 Issues 탭의 그 카드로, PR 칩은 GitHub로 간다.
- PRs 타일은 이 PR에 없고 PRD 3이 Agents · Issues 다음 자리에 넣는다 (D-36). 결과: 이 PR의 타일은 Agents · Issues · Sessions 셋이다. 되돌릴 조건: 운영자가 죽은 타일이라도 자리를 먼저 원할 때.
- Inbox(내 차례 · 실행 중 · 쉬는 중 · 정리할 것 묶음)와 대기 밴드는 돌아오지 않는다 (D-03). 내 차례는 노란 노드, 레인 순서, Agents 타일 배지가 전달한다.
- 마지막으로 고른 타일·모드·레인의 프로젝트별 복원은 하지 않는다 (D-17). ^Tab의 최근 화면 복원만 그 역할을 한다.
- 일괄 정리 시트(`cleanup_review`)를 Overview에 연결하지 않는다 (D-33). 정리는 워크트리 하나씩 기존 대화상자로 한다.
- ⌘K 팔레트에 이슈·PR·에이전트 노드 항목을 더하지 않는다 (D-28). 되돌릴 조건: 운영자가 팔레트에서 렌즈로 가고 싶다고 할 때.
- 에이전트의 마지막 말 전문은 훅이 코어에 보고한 마지막 말(질문, 승인 요청, 진행 보고)의 전체 텍스트이며, 세션 transcript를 다시 읽지는 않는다 (D-50). 되돌릴 조건: 운영자가 transcript의 마지막 답변 전체를 원할 때.
- 디자인 원칙 7·8 (`design/principles.md`): 타일과 레인은 설명 문장과 중첩 카드를 두지 않는다. 관찰 가능한 형태는 B2, B20에 있다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 네 렌즈(이슈, 체크아웃, 에이전트, PR)는 같은 네 객체를 다른 축으로 본다. | qa-log D-01, 운영자: "1. 현재 올라온 PR 베이스로 보기 … 4. Checkout 베이스로 보기?" |
| D-02 | 탭 줄 자리에 렌즈 타일(후보 B). 타일 = 이름 · 노란 내 차례 배지 · 큰 수 · 막대 하나, 문장 없음. 범례와 배지 내역은 호버 popover. 사실 줄은 저장소 사실만 남긴다. 모드 컨트롤은 사실 줄 오른쪽 끝. | qa-log D-02, 운영자: "탭 위 후보는 B로 우선 ㄱㄱ" (efs1) |
| D-03 | Agents 탭 = 체크아웃(기본) · 계보 두 모드. Inbox 제거. 탭 이름은 Agents, 타일 수와 배지는 에이전트를 센다. | qa-log D-03, 운영자: "체크아웃이 사실 메인인 것 같은데.. … Inbox는 우선 빼고 … 2가지만 하자" |
| D-04 | Overview의 첫 화면은 Agents › 체크아웃. 진입: 프로젝트 행, Overview 행, ⌘⇧H. '마지막 본 탭'과 'Issues'는 기각. | qa-log D-04 (ef-top, efs11) |
| D-05 | 체크아웃 레인: 행 = 체크아웃, 레인 안 = 그 체크아웃의 에이전트, 위임은 레인을 건너 아래로 선. main 맨 위, 내 차례, 일하는 중, 쉬는 중 순. 빈 레인과 정리할 것은 접힌 줄. 레인 머리와 호버 카드, 정리 버튼. | qa-log D-05 (efs2, efs4) |
| D-06 | 계보 = 두 번째 모드. Observer → Implementor → 하위, 왼쪽에서 오른쪽. 노드 셋째 줄 = 체크아웃 · 이슈 · PR 칩. 묻는 계보가 맨 위. | qa-log D-06 (efs3) |
| D-09 | 공통 규칙: 평소 = 세 가지만(무엇, 어디까지, 누가), 내 차례만 색. 호버 = 비워 둔 자리의 제자리 버튼. 칩에 0.5초 멈춤 = 미리보기 카드. 클릭 = 영역마다 목적지 하나, ⌘클릭 = GitHub. | qa-log D-09, 운영자: "너무 번잡하지 않으면서 hover하면 추가로 정보를 보일수도 있고 클릭으로 액션할 수 있는건 또 잘 구성해야할 것 같아서" (efs4) |
| D-10 | 버튼은 아이콘이나 한 단어, 설명은 호버 popover. 문안은 efs10(정리, Workspace, 접힌 줄, 타일 배지, 레인 머리 사실). | qa-log D-10, 운영자: "버튼으로 하고 그거 호버했을 때 popover같은거로 설명을 넣는 식으로" |
| D-14 | 전달: #218 머지 뒤 PR 셋의 첫 번째. 이 PR = 타일 머리 + Agents 체크아웃 · 계보 + Inbox 제거 + 첫 화면. | qa-log D-14, 운영자: "#218 머지 후 새 PR들" |
| D-15 | PRD 셋이 qa-log `overview-lenses` 하나를 공유하고 순서대로 implement한다. `agents/config.json`: delivery pr, base main. 각 PR은 앞 PR 머지 후 main에서 시작한다. | qa-log D-15, 운영자 답: "PRD 3개 (Recommended)" |
| D-16 | Sessions 타일 = 오늘 활동한 세션 수(`updated_at`이 오늘), 막대 = Claude/Codex. Overview를 열 때 히스토리를 한 번 읽고, 읽기 전에는 수 자리를 비운다. 기각: 오늘 시작, 전체 수, 이름만. | qa-log D-16, 운영자 답: "오늘 활동한 세션 (Recommended)" |
| D-17 | ⌘⇧H와 프로젝트 행은 항상 앞 체크아웃의 레인을 고른다. 마지막 선택 복원은 기각. | qa-log D-17, 운영자 답: "항상 앞 체크아웃의 레인 (Recommended)" |
| D-33 | 정리 = 기존 Delete worktree 대화상자를 그 워크트리로 연다. 브랜치 삭제는 기본 꺼짐. 폴더 없는 워크트리는 기록만 지운다. | qa-log D-33, 운영자 답: "기존 Delete worktree 대화상자, 브랜치는 기본 유지 (Recommended)" |
| D-54 | 정리 대화상자 안의 pane: 기존대로 `pane N개 닫고 삭제`로 닫고 삭제, 실행 중 에이전트는 경고 줄, 취소는 무변경. | qa-log D-54, 운영자 답: "기존 대화상자 그대로: pane 닫고 삭제, 실행 중은 경고 (Recommended)" |
| D-35 | 가정: 모든 프로젝트 Overview(All projects)는 탭 줄 `Tasks · Agents · Projects`를 유지하되, 그 Agents 뷰도 체크아웃 · 계보가 되고 레인 머리 위에 프로젝트 이름을 단다. 인박스 코드는 두 범위에서 함께 지운다(engineering #1). 되돌릴 조건: 운영자가 All projects에도 타일을 원할 때. | 가정: 보드는 프로젝트 Overview만 그렸고 인박스 코드는 두 범위가 공유한다 (qa-log D-20) |
| D-36 | 가정: 이 PR의 타일은 Agents · Issues · Sessions 셋이고 PRs 타일은 PRD 3이 넣는다. 아무 데도 가지 않는 타일을 두지 않기 위해서다(design #10). | 가정: PRs 탭은 PRD 3에서 생긴다 (qa-log D-14) |
| D-37 | 가정: 타일 막대 구간. Agents = 내 차례(노랑) · 일하는 중(초록) · 자식 대기(파랑) · 쉬는 중(회색). Issues = 백로그 · 진행 중 · 리뷰. Sessions = Claude · Codex. 배지 내역: Agents = 질문 · 승인 · 오류 · 끝남 수. | 가정: efs1의 범례 popover(백로그 18 · 진행 중 2 · 리뷰 2)와 배지 popover(리뷰 2 · 끝난 에이전트 확인 1)를 코어 상태 축(status-model)으로 옮김 |
| D-38 | 가정: 레인 안 노드 순서 = 내 차례, 일하는 중, 자식 대기, 쉬는 중, 같은 묶음 안에서는 최근 활동 순. 레인 폭이 모자라면 레인이 가로로 스크롤하고 타일은 두 줄로 접는다. | 가정: 보드에 좁은 폭 설계 없음, `UI_BEHAVIOR.md` Narrow windows 패턴을 따름 |
| D-50 | 미리보기 내용은 D-09대로 에이전트의 마지막 말 원문 전체이지, 코어가 골라 줄인 둘째 줄이 아니다. 가정은 데이터 경로뿐이다: 코어는 훅이 보고한 마지막 말 원문을 이미 갖고 있으므로 `SidebarAgentSnapshot`에 `message`(원문 전체) 한 필드를 더해 wire에 싣고, popover는 그 원문 전체를 보인다. 둘째 줄은 지금처럼 코어가 만든다. | 가정: D-09의 "에이전트 말 전문"을 코어가 이미 가진 값을 직렬화하는 가장 작은 경로로 채움 |
| D-51 | 가정: Issues 타일은 이 PR에서 #218의 Tasks 보드(Board · List · Dependencies, 사실 줄 오른쪽의 필터와 모드)를 이름만 Issues로 바꿔 연다. 내용은 PRD 2가 바꾼다. | 가정: PRD 2 전에도 타일이 살아 있어야 함(D-36) |
| D-39 | 원칙 intake: `engineering/principles.md`와 `design/principles.md`(oh-my-principle 654485f9)를 읽음. design #4·#7·#9·#10·#13이 B2·B6·B20·B29로, engineering #1·#3·#14가 B9·D-36·Technical structure로 옮겨짐. 옮기지 않은 규칙: 없음. | `sasu principles list` 2026-09-28 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 프로젝트 Overview의 제목 줄과 사실 줄 아래, 탭 줄이 있던 자리에 타일 Agents · Issues · Sessions가 같은 폭으로 놓인다. 선택된 타일만 강조 테두리이고, 타일 클릭이 그 탭을 연다. | D-02, D-36 |
| B2 | 타일 안에는 이름, 내 차례 수의 노란 배지(0이면 없음), 큰 수, 작은 단위어(열림 · 오늘), 막대 하나뿐이고 문장이 없다. | D-02, D-37 |
| B3 | Agents 타일의 수는 프로젝트의 에이전트 수, 배지는 내 차례(needs_you 또는 done 묶음) 수, 막대는 내 차례 · 일하는 중 · 자식 대기 · 쉬는 중 비율이다. Issues 타일의 수는 열린 이슈 수와 '열림', 막대는 백로그 · 진행 중 · 리뷰 비율이다. | D-03, D-37 |
| B4 | 막대에 멈추면 구간 이름과 수의 범례 popover가, 배지에 멈추면 내역 popover(예: 질문 1 · 끝남 1)가 뜬다. 툴팁과 popover 밖에는 설명이 없다. | D-02, D-10 |
| B5 | Sessions 타일의 수는 오늘(이 기기의 날짜) 활동한 세션 수와 '오늘', 막대는 Claude · Codex 비율이다. Overview를 열 때 프로젝트 세션 히스토리를 한 번 읽어 채운다. | D-16 |
| B6 | 아직 읽지 못한 값은 수 자리가 비고 막대가 없다. 0은 0으로 그린다. 출처 읽기가 실패하면 그 타일 이름 옆에 ⚠ 하나가 서고, 멈추면 popover가 실패와 마지막 값의 나이를 말한다. 배너·알림은 없고 이유는 진단 로그에 있다. | D-02 |
| B7 | Issues 타일 클릭은 #218의 Tasks 보드를 이름만 Issues로 바꿔 연다. 열, 카드, 시작 · 새 이슈 · 이슈 연결은 #218 그대로다. | D-51 |
| B8 | 사실 줄은 워크트리 수, 디스크, main behind(0 초과만), `N merged → 정리`(0 초과만)만 남는다. 열린 이슈 수와 PR 수는 사실 줄에서 사라진다. 사실 줄 오른쪽 끝에 현재 탭의 모드 컨트롤이 선다: Agents면 `체크아웃 · 계보` 토글, Issues면 필터와 `Board · List · Dependencies`. | D-02 |
| B9 | 제목 줄의 New agent와 새 이슈(C)는 그대로다. | D-02 |
| B10 | Agents 탭은 체크아웃(기본)과 계보 두 모드뿐이다. 인박스 묶음(내 차례 · 실행 중 · 쉬는 중 · 정리할 것)과 그 코드, 대기 밴드의 잔재는 두 범위에서 사라진다. | D-03, D-35 |
| B11 | 프로젝트 행, Overview 행, 팔레트, ⌘⇧H 어느 것으로 들어와도 Overview는 Agents › 체크아웃으로 열린다. ^Tab(Recent Panels)만 떠났던 화면을 타일 · 모드 · 선택까지 그대로 복원한다. | D-04, D-17 |
| B12 | ⌘⇧H와 프로젝트 행은 앞 체크아웃의 레인을 선택(강조 테두리, 보이도록 스크롤)한다. 앞 체크아웃이 없으면 main 레인이 선택된다. | D-17 |
| B13 | 체크아웃 모드는 행 하나가 체크아웃 하나다. main 레인이 맨 위에 고정되고, 다음에 내 차례 에이전트가 있는 레인, 일하는 중 레인, 쉬는 레인 순이며 같은 묶음 안은 최근 활동 순이다. 레인 안 노드는 내 차례, 일하는 중, 자식 대기, 쉬는 중 순이다. | D-05, D-38 |
| B14 | 위임은 부모 노드에서 자식 노드까지 선으로 그린다. 다른 레인의 자식이면 레인을 건너 아래로, 같은 레인이면 오른쪽 화살표다. Observer가 main에서 위임한 Implementor는 그 워크트리 레인에 선다. | D-05 |
| B15 | 레인 머리(평소)는 PR 상태 색의 글리프와 브랜치(mono), 목적 한 줄(없으면 PR 제목, 없으면 브랜치만), 셋째 줄에 이슈 칩 · PR 칩 · ↑N ↓N · 변경 파일 N(dirty일 때 경고색)이다. main 레인 머리는 집 글리프 · main, 목적, `에이전트 N`이다. | D-05 |
| B16 | 레인 머리에 멈추면 배경이 밝아지고 체크아웃 카드가 뜬다: 경로, 기준 브랜치와 ↑↓, +N −N · 파일 N, 마지막 커밋 나이, PR 상태 · CI. 카드 안 `↵ Workspace`가 클릭과 같다. | D-05, D-09, D-10 |
| B17 | 레인 머리 클릭은 그 Workspace로 간다(main 포함). 이슈 칩 클릭은 Issues 탭의 그 카드로, PR 칩 클릭은 GitHub로 간다. ⌘클릭은 어디서든 GitHub다. | D-09 |
| B18 | 머지된 워크트리는 보라 머지 글리프에 흐리게, 폴더 없는 워크트리는 × 글리프와 `폴더 없음`으로 그리고, 둘 다 머리 오른쪽에 `정리` 한 단어 버튼이 선다. 멈추면 popover가 무엇을 지우는지 말한다. | D-05, D-10, D-33 |
| B19 | `정리` 클릭은 기존 Delete worktree 대화상자를 그 워크트리로 연다. 브랜치 삭제 체크는 기본 꺼짐이고, 폴더 없는 워크트리는 기록만 지운다는 확인이다. 안에 pane이 있으면 버튼이 `pane N개 닫고 삭제`이고 실행 중 에이전트는 경고 줄로만 보인다. 취소는 아무것도 바꾸지 않고, 닫힌 에이전트의 대화는 Sessions 탭에서 다시 읽을 수 있다. | D-33, D-54 |
| B20 | 에이전트가 없는 워크트리는 레인 대신 `에이전트 없는 워크트리 N` 한 줄로, 머지되었거나 폴더 없는 워크트리와 거기서 쉬는 에이전트는 `정리할 것 N` 한 줄로 접힌다. 줄 클릭이 그 자리에서 펼치고, 0이면 줄이 없다. 사실 줄의 `N merged → 정리`는 Agents › 체크아웃으로 가서 `정리할 것`을 펼친다. | D-05 |
| B21 | 노드(평소)는 상태 마크 · provider 마크 · 제목 · 나이 한 줄과 코어가 만드는 둘째 줄(말 한 줄) 한 줄이다. 내 차례는 노란 테두리와 노란 질문 줄, 끝남(아직 안 봄)은 ✓와 결과 줄, 위임 중은 파란 ○과 자식 요약(`일하는 중 1 · 물음 1`), 일하는 중은 파란 ●과 진행 줄, 쉬는 중은 흐리게 말 없이다. 색은 내 차례에만 있다. | D-09 |
| B22 | 노드에 멈추면 배경만 밝아진다. 말에 0.5초 멈추면 그 에이전트의 마지막 말 원문 전체(wire의 `message`; 줄인 둘째 줄이 아니다) popover가 뜨고 그 안 `↵ 패널에서 답하기`가 클릭과 같다. 노드 클릭은 그 에이전트 패널을 여는 이벤트 하나다. | D-09, D-50 |
| B23 | 계보 모드는 열이 Observer(보통 main) · Implementor · 하위 에이전트이고, 행 하나가 계보 하나다. 묻는 노드가 있는 계보가 맨 위, 다음 일하는 중, 다음 쉬는 중이다. 부모에서 자식으로 화살표가 가고, 부모 없는 에이전트는 첫 열에 선다. | D-06 |
| B24 | 계보 노드의 셋째 줄은 체크아웃 칩 · 이슈 칩 · PR 칩이다. 체크아웃 칩 클릭은 Workspace, 이슈 칩은 Issues 탭의 그 카드, PR 칩은 GitHub다. 칩에 0.5초 멈추면 체크아웃 카드, 이슈 미리보기(id · 상태 · 제목 · 갱신), 기존 PR 카드가 뜬다. | D-06, D-09 |
| B25 | 계보 모드에서 쉬는 에이전트는 `쉬는 에이전트 N`, 정리할 것은 `정리할 것 N` 한 줄로 접힌다. | D-06 |
| B26 | 노드와 레인 머리는 포커스를 받는다. 방향키가 노드 사이를 옮기고 ↵는 클릭과 같으며, Esc는 지금처럼 Overview를 나간다. | D-09 |
| B27 | 호버, 포커스, 0.5초 멈춤은 로컬 상태다. 스냅샷 발행, 코어 이벤트, Git·디스크 작업을 일으키지 않는다. | D-09 |
| B28 | 아이콘 버튼과 칩마다 툴팁과 같은 접근성 이름이 있고, efs10의 popover 문안이 그 이름이다. | D-10 |
| B29 | 첫 스냅샷 전에는 셸의 연결 중 상태가 보인다. GitHub를 아직 읽지 못했으면 PR 칩과 PR 색 글리프가 없고, 레인은 Git 사실만으로 선다. 레인 안 값을 읽지 못하면 물음표 마크지 거짓 0이 아니다. | D-02 |
| B30 | All projects의 Agents 뷰도 같은 체크아웃 · 계보이며 레인 머리 위에 프로젝트 이름이 붙는다. All projects의 탭 줄은 그대로다. | D-35 |
| B31 | Light 테마에서 같은 화면이 efs13대로 그려진다. | D-02 |
| B32 | 20개 워크트리와 14개 접힘의 실데이터 규모에서 타일 값과 레인 순서는 스냅샷의 순수 계산이고, 창 폭이 레인 최소 폭보다 좁으면 레인이 가로로 스크롤하고 타일은 두 줄이 된다. | D-38 |
| B33 | `docs/UI_BEHAVIOR.md`의 Project Home 절이 이 행동(타일, 첫 화면, Agents 두 모드, 인박스 삭제)으로 갱신되고, `docs/DESIGN_WORKFLOW.md`의 Web screen list와 `design/hide-screens.pen`의 `Screen / Project Overview`가 Agents › 체크아웃과 계보 프레임(Light · Dark)을 갖는다. 인박스 프레임은 지운다. | D-14 |

## Technical structure

- 코어 wire에 `SidebarAgentSnapshot.message` 한 필드(훅이 보고한 에이전트의 마지막 말 원문 전체)가 는다. `wire.rs`가 아니라 `sidebar.rs`가 이미 가진 값을 직렬화할 뿐이며, 델타 비교 비용은 문자열 하나다. 그 밖의 타일 수·배지·막대, 레인 묶음과 순서, 접힘 수는 기존 스냅샷 필드(`SidebarAgentSnapshot`의 group · lineage, `CheckoutSnapshot`의 pull_request · ahead · behind · dirty · exists, `WorktreeSnapshot`의 merged · missing)로 셸이 계산하는 순수 함수이며 `web/src/projectBoard.ts`의 인박스 규칙을 대체한다.
- Overview 열기는 기존 `github_request`와 `card_measure_disk`에 더해 `sessions_refresh {workspace_id}`를 보낸다. 오늘 수는 `project_sessions.rows`의 `updated_at_unix_ms`로 셸이 센다. 새 코어 이벤트는 없다.
- 정리는 기존 `remove_worktree` 이벤트와 Delete worktree 대화상자(`deletion_gate`)를 그대로 쓴다.
- 탭 · 모드 · 선택 레인은 #218처럼 셸의 페이지 상태이고 영속하지 않는다. ^Tab 복원은 #218의 screen surface에 타일 · 모드 · 선택을 더한다.
- `Mutex<Runtime>` 아래 subprocess·blocking I/O 없음(gh는 worker), 사용자 행동은 이벤트 하나, 호버는 발행 없음이라는 `CLAUDE.md` 규칙을 지킨다.
- 시각 권위는 `design/hide-ui.lib.pen`이고 새 타일 · 레인 머리 · 노드는 `Component /` 시트로 더한 뒤 `design/hide-screens.pen`에 그린다.

## Risks

- #218이 머지되기 전에는 시작할 수 없다. 머지 결과가 리뷰 중 바뀌면(파일 이름, 이벤트) 구현 시작 시 main을 다시 읽는다.
- 인박스 삭제로 "내 차례" 목록이 사라지는 것은 결정(D-03)이다. 노드 색 · 레인 순서 · 배지가 그 역할을 다 하는지는 운영자가 첫 화면에서 판단한다.
- Overview를 열 때마다 세션 히스토리를 한 번 읽는 비용은 Sessions 탭이 지금 내는 비용과 같고, 읽는 동안 수 자리만 빈다. `docs/PERFORMANCE_TESTING.md`의 Overview cost contract 안에서 측정해 보고한다.
- 위임선은 SVG 오버레이라 레인 스크롤과 창 크기 변화에서 어긋날 수 있다. e2e 스크린샷으로 부모·자식 위치를 확인한다.
- D-35 · D-36 · D-37 · D-38은 되돌릴 수 있는 가정이며 운영자가 PRD 검토에서 뒤집을 수 있다.
- 실증은 격리된 Herdr 서버와 fixture 스냅샷으로 하고 운영자의 앱 · pane · 서버를 건드리지 않는다. 운영자가 미리 준비할 것은 없다.
