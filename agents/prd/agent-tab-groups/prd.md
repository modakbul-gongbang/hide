---
topic: "Agent 탭 그룹: Files View와 같은 영역 분할·드래그·이동"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "core의 보이는 탭·attach 모델과 저장 파일(추가 키)과 닫기 효과 순서를 바꾸고 web의 Files View 코드를 공용으로 옮기는 사용자 대면 변경이며, 권한·개인정보·운영 데이터·비용 효과는 없다."
source_intake: "agents/interview/agent-tab-groups/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: Agent 탭 그룹 (Files View와 같은 영역)

## Goal

hide 사용자는 Workspace의 에이전트 칸에서 탭을 Files View처럼 그룹(Agent 영역)으로 나누고, 그룹마다 탭 바와 터미널을 두고, 탭을 끌어 분할·이동·재정렬하고, 구분선으로 크기를 바꾼다.
사용자의 말: "tab group이 시각적으로 안나뉘어져있다", "files view처럼 완전히 동일하게 동작해야함", 코드는 engineering 원칙대로 공유한다.
그룹은 Hide가 소유하므로 Herdr workspace·pane id·계보는 바뀌지 않고, 저장소 primary Herdr workspace의 마지막 탭도 닫힌다.

## Non-goals

- SSH 장치 Workspace의 다중 영역은 만들지 않는다: 장치 에이전트 칸은 영역 하나이고 분할할 수 없다 (D-22). 재검토: 로컬 영역이 머지된 뒤 장치 attach를 여러 탭으로 넓힐 때.
- 에이전트 탭과 View(파일·diff·페이지)는 한 트리로 합치지 않는다: 서로의 영역으로 끌어 넣을 수 없다 (D-08). 재검토: 사용자가 VS Code식 혼합 그룹을 요청할 때.
- 영역 배치 때문에 Herdr workspace를 만들거나 닫거나 pane을 workspace 사이로 옮기지 않는다 (D-07, D-03).
- Herdr TUI의 탭 순서와 Hide 영역 순서를 맞추지 않는다 (D-19). 재검토: TUI와 함께 쓰는 사용자가 순서 불일치를 문제로 보고할 때.
- 탭 이름·탭 구성·Rename은 agent-tab-names PRD의 것이다; 이 PR은 그 탭 렌더 단위를 영역마다 재사용한다 (D-20, D-26).
- 새 라이브러리 Component는 만들지 않는다 (D-24).
- 이전 빌드로 되돌렸을 때 Agent 배치를 보존하지 않는다 (D-28).
- engineering 원칙 1·5·7이 공용 트리와 어댑터 구조(D-15)를, 원칙 15가 attach 한도(D-17)를, 원칙 11·13이 교체 닫기(D-21)를 정한다.

## Decisions

D-15~D-27은 사용자 결정이 아니라 `/please` 위임 아래 에이전트가 고른 되돌릴 수 있는 가정이다; 사용자 결정은 D-06~D-10, D-28, D-29이고 나머지는 저장소·계약 사실이다.

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 지금 에이전트 칸은 체크아웃당 탭 바 1개·캔버스 1개이고 core의 보이는 탭도 체크아웃당 하나다(`visible_tab_ids`). | 저장소 사실 (qa-log D-01) |
| D-02 | Files View는 core 분할 트리(`view_layout.rs`, `workspace_views.rs`, `runtime/view_areas.rs`)와 web 순수 규칙(`viewLayout.ts`, `viewDrag.ts`)·`ViewAreas.tsx`로 그리고, 에이전트 탭 드래그는 `TabBar.tsx`의 별도 간이 구현이다. | 저장소 사실 (qa-log D-02) |
| D-03 | Herdr 0.9.1 `tab.move`는 workspace 안에서만 되고, workspace를 넘는 `pane.move`는 새 공개 pane id를 붙인다. | Herdr 계약·공식 문서 (qa-log D-03) |
| D-04 | `tab.close`에는 확인 플래그가 없고, linked worktree가 열린 primary workspace의 마지막 탭 닫기는 거부되며, 유일한 우회 `close_group`은 linked worktree까지 닫는다; `w9J:tM` 닫기가 이것으로 실패했다. | 로그·계약 (qa-log D-04) |
| D-05 | Hide 체크아웃(폴더)과 Herdr workspace(탭 컨테이너)는 1:1이 아니다; main 탭 줄에 `w8P`와 `w9J` 탭이 섞여 있었다. | 저장소 사실 (qa-log D-05) |
| D-06 | 에이전트 칸은 Files View와 완전히 같은 탭 그룹 동작(시각 분리, 그룹별 탭 바, 가장자리 분할, 이동·재정렬, 크기 조절, 빈 그룹 정리)을 갖고, 공유할 코드는 한 벌로 관리한다. | 사용자: "files view처럼 완전히 동일하게 동작해야함. 만약 코드를 share할 수 있으면 enginerring 원칙처럼 잘 관리하는게 중요할듯" (qa-log D-06) |
| D-07 | 그룹은 Hide가 소유한다; 이동은 Herdr 호출 없이 끝나고 pane id가 유지된다. 기각: 영역 = Herdr workspace. | 사용자 선택 "Hide 소유 영역 (Recommended)" (qa-log D-07) |
| D-08 | Agent 영역은 에이전트 칸의 자체 트리이고 사이드 패널 Files View와는 코드만 공유한다. 기각: 한 트리로 합치기. | 사용자 선택 "Agent 영역 따로 (Recommended)" (qa-log D-08) |
| D-09 | primary workspace 마지막 탭을 닫으면 같은 workspace에 셸 탭을 먼저 만들고 닫는다. 기각: 거부만 표시, 범위 제외. | 사용자 선택 "셸 탭으로 교체 후 닫기 (Recommended)" (qa-log D-09) |
| D-10 | PRD부터 구현까지 이어서 하고, 드래그로 탭 공간이 Files View처럼 나뉘는 것을 실제로 검증하며, 코드를 재활용하고 필요한 인터페이스를 설계한다. | 사용자 호출 (qa-log D-10) |
| D-11 | 배포는 `agents/config.json`의 PR 모드이고 main은 PR 머지만 받는다. | 설정 사실 (qa-log D-11) |
| D-12 | Herdr는 `session.json`에 workspace별 공개 탭 번호를 저장하므로 탭 id는 재시작 뒤에도 유지된다고 보고, 배치는 탭 id로 탭을 가리킨다. | 관찰 기반 추론 (qa-log D-12) |
| D-13 | 체크아웃 탭 순서는 메모리에만 있고, attach는 최근 탭 5개, View 한도는 영역 6·깊이 3이다. | 저장소 사실 (qa-log D-13) |
| D-14 | 닫기 거부는 Herdr가 판단하고 재시도 가능한 알림과 진단으로 이어지며, core는 새 socket 호출 없이 Herdr workspace의 worktree 정보로 primary 여부를 판단할 수 있다. | 저장소 사실 (qa-log D-14) |
| D-15 | core는 트리 부분을 항목 타입에 대해 일반화한 한 모듈로 빼고 View·Agent 레이아웃이 얇은 래퍼가 된다; web은 트리·기하·드롭·드래그 세션·영역 트리 컴포넌트를 공용으로 옮기고 View·Agent가 어댑터만 준다; `TabBar.tsx` 드래그와 로컬 단일 탭 줄 재정렬 경로는 같은 변경에서 지운다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-15) |
| D-16 | Agent 배치는 Workspace마다 `workspace-views.json` schema 2의 새 키 `agent_layout`으로 저장하고, 항목은 Herdr 탭 id, 한도는 View와 같으며, 한 탭은 한 영역에만 있고, 처음 보는 Workspace는 지금 탭 줄 순서의 영역 하나로 시작한다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-16) |
| D-17 | 영역마다 활성 탭 하나가 동시에 보이고 모두 attach되며 released되지 않는다; attach 총량은 max(5, 보이는 탭 수) ≤ 6; 활성 영역의 탭만 Herdr focus·읽음·sleep 방문 대상이고 보이는 탭은 모두 on screen으로 잠들지 않는다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-17) |
| D-18 | 드래그·삽입선·가장자리 분할·금지 커서·취소·구분선·최소 크기·탭 바 스크롤·좁은 창은 View 영역 규칙을 그대로 따른다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-18) |
| D-19 | 영역 안 탭 순서는 Hide가 소유하고 드래그는 `tab.move`를 보내지 않는다; 새 탭은 활성 영역(또는 요청한 영역) 끝, 사라진 탭은 빠지고 빈 영역은 접히며, 위임 전용 탭은 탭 바에 없다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-19) |
| D-20 | Agent 탭 메뉴는 New tab, Split ×4, Move ×4, Copy tab name, Close tab…이고 팔레트는 같은 명령과 영역 포커스·크기 명령을 준다; ⌘T/⌥T는 활성 영역에 탭을 만든다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-20) |
| D-21 | 교체 닫기는 core가 Herdr workspace 정보로 미리 판단해 한 operator 이벤트 안에서 셸 탭 생성(intent marker로 재시도 수렴) 후 닫기를 하고, 셸 탭은 닫힌 탭 자리에 들어가며, 생성 실패는 아무것도 닫지 않고, 닫기 거부는 셸 탭을 남긴다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-21) |
| D-22 | SSH 장치 Workspace는 공용 컴포넌트로 영역 하나를 그리고 재정렬은 장치 Herdr 순서로 하며 분할은 이유와 함께 쓸 수 없다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-22) |
| D-23 | 재시작하면 트리·비율·영역별 탭과 활성 탭·활성 영역이 돌아오고, 없는 탭은 빠지며, 배치되지 않은 탭은 활성 영역에 들어가고, 되살린 탭은 닫힌 영역(없으면 활성 영역)에 들어간다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-23) |
| D-24 | 삽입선·분할 overlay·구분선·스위처는 View와 같은 모양을 공용 코드로 쓰고, Screen / Workspace에 두 영역 상태를 추가하며, UI_BEHAVIOR.md와 ARCHITECTURE.md를 같은 변경에서 고친다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-24) |
| D-25 | 검증은 Rust 단위·runtime 테스트, vitest, 격리 hided+Herdr의 web e2e 실제 포인터 드래그와 스크린샷, desktop e2e(CI; 로컬은 먼저 물음), web-shell-measure 측정이고, 운영 앱·Herdr·pane은 건드리지 않는다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-25) |
| D-26 | PR 배포와 사용자 승인 머지; Implementor는 Codex(gpt-6-astra, high); 겹치는 동시 PR 위로 rebase하고 탭 렌더 단위를 재사용한다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-26) |
| D-27 | 원칙 intake: engineering·design principles(oh-my-principle 654485f) 전문을 읽고 반영했으며, design 11은 기존 패턴이 배치를 정하므로 적용하지 않았다. | 에이전트 소유 가정(/please 위임으로 기록, 사용자 결정이 아니며 거부하면 바뀜; qa-log D-27) |
| D-28 | 이전 빌드가 `agent_layout`을 지워도 되고, 돌아오면 모든 탭을 담은 영역 하나로 시작한다. 기각: 별도 파일 보존. | 사용자 답: "잃어도 됨 (Recommended)" (qa-log D-28) |
| D-29 | 단일 영역은 main 대비 측정 오차 안(회귀면 실패), 다중 영역은 보이는 탭당 추가 비용을 idle·driven으로 보고해 리뷰가 판단한다. | 사용자 답: "단일 영역 무회귀 + 다중은 보고 (Recommended)" (qa-log D-29) |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 에이전트 칸은 하나 이상의 Agent 영역으로 나뉘어 보이고, 영역마다 자기 탭 바(그 영역의 탭들, 끝에 New tab)와 그 영역 활성 탭의 pane 캔버스가 있으며, 영역 사이에는 구분선이 있다. | D-06, D-08 |
| B2 | 처음 여는 Workspace와 업그레이드 직후 Workspace는 지금 탭 줄 순서 그대로 모든 탭을 담은 영역 하나로 보인다. | D-16, D-28 |
| B3 | 한 영역이 활성이다: 활성 영역의 활성 탭에만 accent 표시가 있고, 다른 영역은 활성 탭을 강조 없이 보여 준다; 탭이나 pane을 누르면 그 영역이 활성이 되고 키보드가 그 pane으로 간다. | D-17, D-18 |
| B4 | 모든 영역의 활성 탭 터미널이 동시에 살아 있고 출력이 계속 갱신되며, 보이는 탭에는 released 캡션이 뜨지 않고, 보이는 탭의 에이전트는 잠들지 않는다; 읽음 처리는 활성 영역의 탭에만 일어난다. | D-17 |
| B5 | 각 영역의 pane은 지금처럼 헤더·자식 칩·관계 컨트롤·pane 분할을 갖고, 찾기 바는 포커스된 pane이 있는 영역에 뜬다. | D-17, D-18 |
| B6 | 탭을 활성화 거리 이상 끌면 떠 있는 탭 복사본이 포인터를 따라오고 원래 탭은 자리를 지키며, 드래그 중에는 어떤 터미널도 크기가 바뀌지 않는다; 활성화 거리 안에서 놓으면 클릭이다. | D-18 |
| B7 | 탭 바 위에서는 삽입선이 놓일 자리를 보이고, 자기 바에 놓으면 순서가 바뀌고 다른 영역 바에 놓으면 분할 없이 그 영역으로 옮겨지며, 옮긴 탭이 그 영역의 활성 탭이 되고 그 영역이 활성이 된다; pane id와 Herdr 탭 순서는 바뀌지 않는다. | D-07, D-18, D-19 |
| B8 | 영역 내용의 왼쪽·오른쪽·위·아래 가장자리 근처에서는 생길 절반이 `Split left/right/up/down` 라벨과 함께 강조되고, 놓으면 새 영역이 그 절반을 차지하며 탭이 그리로 옮겨져, 두 영역이 각자 탭 바와 살아 있는 터미널을 갖는다; 터미널 크기는 놓을 때 한 번 바뀐다. | D-06, D-18 |
| B9 | 영역의 유일한 탭을 그 영역 가장자리로 끌 때, 영역이 최소 크기라 반으로 못 나눌 때, 영역 6개나 깊이 3에 닿았을 때, 에이전트 칸 밖(사이드 패널 포함)일 때는 overlay 없이 금지 커서가 보이고, Escape·창 밖에서 놓기·그 사이 사라지거나 불가능해진 대상은 순서와 배치를 그대로 둔다; 유효한 드롭만 배치를 한 번 바꾼다. | D-08, D-16, D-18 |
| B10 | 영역의 마지막 탭이 이동·닫기·Herdr에서 사라짐으로 빠지면 그 영역이 접히고 이웃이 자리를 차지한다; 마지막 남은 영역에 탭이 없으면 "No agent tab is open" 빈 상태와 New tab이 보인다. | D-18, D-19 |
| B11 | 영역 사이 구분선은 hover와 키보드 focus에 accent 색이 되고, 끌면 가이드 선이 따라오며 놓을 때 크기가 한 번 바뀌고, focus된 구분선은 화살표 키 한 번에 한 단계 움직인다; 어느 쪽도 최소 크기보다 작아지지 않고 각 쪽은 15~85%를 유지한다. | D-18 |
| B12 | 영역의 탭이 폭을 넘으면 그 영역 탭 바가 스크롤해 활성 탭이 보이게 한다. | D-18 |
| B13 | 창이 좁아 모든 영역에 최소 크기를 줄 수 없으면 활성 영역만 보이고 영역 스위처로 다른 영역을 고르며, 창을 넓히면 저장된 배치가 그대로 돌아온다. | D-18 |
| B14 | 영역의 New tab은 그 영역 끝에, ⌘T/⌥T는 활성 영역 끝에 새 탭을 만들어 그 영역의 활성 탭으로 보이고, Herdr나 다른 클라이언트가 만든 탭은 화면을 바꾸지 않고 활성 영역 끝에 들어간다. | D-19, D-20 |
| B15 | Herdr TUI에서 탭 순서를 바꿔도 Hide 영역의 순서는 그대로다. | D-19 |
| B16 | 위임 전용 탭은 어느 영역 탭 바에도 없고, 사이드바에서 고르면 활성 영역 캔버스에 그 탭이 보인다. | D-19 |
| B17 | 사이드바·팔레트·탭 순환으로 탭이나 에이전트를 고르면 그 탭이 있는 영역이 활성이 되어 그 탭을 보여 준다. | D-17 |
| B18 | Agent 탭 우클릭(또는 메뉴 키) 메뉴는 New tab, Split right/left/up/down, Move right/left/up/down(그 방향에 영역이 있을 때만), Copy tab name, Close tab… 순이고, 만들 수 없는 Split은 이유와 함께 비활성으로 남으며, 메뉴를 열어도 포커스와 상태는 바뀌지 않는다. | D-20 |
| B19 | 키보드가 에이전트 칸에 있을 때 팔레트는 활성 영역에 대한 Split·Move·다음/이전 영역 포커스·영역 키우기/줄이기를 주고, 지금 못 하는 명령은 흐리게 이유와 함께 보인다. | D-20 |
| B20 | 앱을 다시 시작하면 마지막 Workspace의 영역 트리·비율·영역별 탭 순서·활성 탭·활성 영역이 그대로 돌아오고, 더 없는 탭은 빠지며(빈 영역은 접힘), 어느 영역에도 없는 현재 탭은 활성 영역 끝에 들어간다. | D-12, D-16, D-23 |
| B21 | Reopen closed tab으로 되살린 탭은 닫힌 영역이 남아 있으면 그 영역에, 없으면 활성 영역에 들어간다. | D-23 |
| B22 | linked worktree가 열린 저장소의 primary Herdr workspace의 마지막 탭(또는 그 탭의 마지막 pane)을 닫으면, 기존 확인·상태 가드를 거친 뒤 탭이 닫히고 같은 영역·위치에 체크아웃 루트의 셸 탭이 나타나며, 다른 worktree의 workspace와 에이전트는 그대로다. | D-04, D-09, D-14, D-21 |
| B23 | 교체 닫기에서 셸 탭 생성이 실패하면 아무것도 닫히지 않고 재시도 가능한 알림이 뜨며, 생성 뒤 닫기가 거부되면 셸 탭은 남고 알림의 재시도는 닫기만 다시 해 셸 탭을 두 번 만들지 않는다; Herdr의 원래 코드와 메시지는 진단 로그에만 남는다. | D-14, D-21 |
| B24 | 그 밖의 탭·pane 닫기는 지금 가드(작업 중 확인, 상태 모름이면 새로고침)를 그대로 따르고, 닫힌 탭은 자기 영역에서 빠진다. | D-19, D-21 |
| B25 | SSH 장치 Workspace의 에이전트 칸은 같은 영역 컴포넌트로 영역 하나를 그리고, 드래그 재정렬은 지금처럼 장치 Herdr 순서를 바꾸며, 가장자리로 끌면 금지 커서가 보이고 Split 항목은 이유와 함께 비활성이다. | D-22 |
| B26 | 이전 빌드를 쓰다가 이 빌드로 돌아오면 Agent 영역은 모든 탭을 담은 영역 하나로 시작하고 탭은 하나도 사라지지 않으며 Files View 배치는 그대로다. | D-28 |
| B27 | Files View(View 영역)의 기존 동작(미리보기, Open to the side, 같은 문서 규칙, 페이지 정지 화면, 탭 메뉴, 드래그, 좁은 창)은 공용 코드로 옮긴 뒤에도 그대로다. | D-15 |
| B28 | 영역 조작은 Herdr workspace를 만들거나 닫거나 pane을 workspace 사이로 옮기지 않아서 pane id·계보·토큰과 Herdr TUI의 workspace 목록이 그대로다. | D-03, D-07 |
| B29 | 영역이 하나인 Workspace의 응답성과 자원 사용은 같은 기계에서 main 대비 측정 오차 안이고, 영역 2개 이상의 보이는 탭당 추가 비용(idle CPU, 메모리, 키 에코 지연)은 idle·driven 측정으로 PR에 보고된다. | D-17, D-29 |
| B30 | UI_BEHAVIOR.md와 ARCHITECTURE.md가 Agent 영역·탭 순서 소유·attach 규칙·교체 닫기를 서술하고, Screen / Workspace 시트가 에이전트 칸이 두 영역으로 나뉜 상태를 그린다. | D-24 |

## Technical structure

- core 공용 트리: `view_layout.rs`의 트리 부분(영역·분할·id 발급·한도·move/split/resize/collapse/neighbour, repair의 공통 불변식)을 항목 타입에 대해 일반화한 모듈 하나로 옮기고, View 레이아웃(Display 항목: 미리보기, 같은 문서 규칙, 페이지)과 새 Agent 레이아웃(Herdr 탭 id 항목)이 그 위의 래퍼가 된다. 한도 상수는 트리가 받는 값이다.
- core Agent 레이아웃: Workspace(장치+경로)마다 `workspace-views.json` schema 2에 `agent_layout` 키로 저장(기존 워커 저장·옮겨두기 규칙 재사용), `agent_layout` 이벤트(focus, focus_area, move, split+request_id, resize)는 `view_layout`과 같은 Workspace stale 검사·request_id 링·unknown id 거부 코드를 공유, Herdr 탭 목록과의 reconcile(새 탭 배치, 사라진 탭 제거, 빈 영역 접기, 위임 탭 제외), 스냅샷 `workspace_view.agent_layout`(트리, 활성 영역, 한도; 탭 내용은 기존 `checkout.tabs`).
- core 보이는 탭: 체크아웃당 보이는 탭 하나를 영역별 보이는 탭 집합과 활성 탭으로 바꾸고, attach는 보이는 탭 전부 + 최근 탭으로 총량 max(5, 보이는 탭 수); Herdr `tab.focus`·읽음·sleep 방문은 활성 영역의 탭에만, last-look은 보이는 탭 전부에. `checkout.active_tab_id`는 활성 영역의 탭으로 남는다. 로컬 체크아웃의 단일 탭 줄 재정렬(`reorder_tab`, `PendingTabMove`, `checkout_tab_order`의 로컬 사용)은 소비자가 없어지면 지우고, 장치 탭 줄 경로는 유지한다.
- core 교체 닫기: close 승인 시 Herdr workspace의 worktree 정보로 판단해 기존 close operation 상태기계 안에서 `tab.create`(체크아웃 루트, marker env) -> `tab.close` 순서로 runtime mutex 밖 워커가 실행하고, 재시도는 marker로 이미 만든 셸 탭을 재사용한다. 새 Herdr 메서드는 없다.
- web 공용 계층: 트리·기하·드롭 판정(항목 id 기준, 같은 항목 규칙은 어댑터), 드래그 세션, 영역 트리 컴포넌트(구분선·가이드·스위처·드래그 미리보기·탭 바 스크롤)를 공용 모듈로 옮기고, `ViewAreas.tsx`와 새 Agent 영역은 탭·본문·메뉴·드롭 dispatch 어댑터만 준다. 그려진 프레임 레지스트리는 칸(View/Agent)별로 둔다. `PaneCanvas`는 탭을 인자로 받는다. `TabBar.tsx`의 `useTabDrag`는 지운다.
- 계약과 문서: `web/src/snapshot.ts` 타입과 관련 wire 계약, UI_BEHAVIOR.md, ARCHITECTURE.md, `scripts/pen-screens.mjs`로 생성한 `design/hide-screens.pen`의 Screen / Workspace.
- 바뀌지 않음: Herdr 계약과 pin, 사이드 패널과 View 영역의 동작, 장치 attach, 사이드바.

## Risks

- 동시 진행 중인 agent-tab-names(TabBar 탭 렌더·core 라벨), close-chord-policy(⌘W), view-new-tab-page(View New tab)가 같은 파일을 고친다; main에 먼저 머지된 것 위로 rebase하고, 탭 렌더 단위를 영역마다 재사용해 충돌을 줄인다.
- Files View 코드를 공용으로 옮기는 리팩터가 View 동작을 깨뜨릴 수 있다; 기존 s7 e2e와 viewLayout/viewDrag 단위 테스트가 그대로 통과해야 한다 (B27).
- 동시에 보이는 탭이 늘면 Herdr 서버가 attach된 pane을 모두 렌더하는 비용이 커진다(알려진 upstream 비용); D-29 기준으로 보고한다.
- 탭 id 안정성(D-12)이 틀리면 재시작 뒤 배치가 영역 하나로 돌아간다; 탭은 잃지 않는다.
- Herdr가 primary workspace 마지막 탭의 마지막 pane 닫기를 어떻게 처리하는지는 격리 Herdr에서 확인한다; 교체는 판단 조건에 따라 두 경로 모두에 적용된다. worktree 정보가 없는 workspace는 교체하지 않고 Herdr 거부가 지금처럼 알림으로 보인다.
- 실제 앱 검증은 격리된 Herdr 서버·상태 디렉터리·후보 앱에서만 하고 운영 앱·운영 Herdr·운영 pane은 건드리지 않는다; 로컬 desktop e2e는 operator 화면에 창을 띄우므로 실행 전에 묻는다.
- 사용자가 미리 해야 할 일: 없음.
