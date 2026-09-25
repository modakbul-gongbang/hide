---
topic: "Web shell: agent pane scroll, Cmd+F find in pane, empty View region disappears"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "Three user-facing web shell fixes across the terminal input path, keyboard routing and View layout; no data migration, auth, or external side effect."
source_intake: "agents/interview/web-pane-scroll-find-areas/qa-log.md"
created_at: "2026-09-26"
updated_at: "2026-09-26"
---

# PRD: Web shell: agent pane scroll, Cmd+F find in pane, empty View region disappears

## Goal

웹 셸(hided + web)로 에이전트를 보는 운영자가 세 가지에서 막히고 있다.
에이전트 pane은 휠 스크롤이 되다가 안 되다가 하고, 활성 pane에서 Cmd+F를 눌러도 그 pane 안에서 검색이 되지 않으며, View 영역의 마지막 파일을 닫으면 빈 공간이 "No file or diff is open in this Workspace."로 남는다.
목표는 한 문장이다: 웹 셸에서 에이전트 pane은 일반 터미널 pane처럼 매번 휠로 스크롤되고, 포커스된 pane에서 Cmd+F가 그 pane을 검색하며, View 영역의 표시가 0개가 되면 그 영역이 사라진다.

## Non-goals

- Swift 셸의 스크롤·검색·레이아웃은 바꾸지 않는다. 운영자는 웹 셸에서만 차이를 본다. Swift 셸에서 같은 문제가 보고되면 다시 연다.
- Herdr 자체(pane 소유권, control 세션 규칙)는 수정하지 않는다. Herdr 계약 안에서 해결하고, 계약이 막으면 D-03의 표식으로 드러낸다. Herdr에 필요한 API가 생기면 다시 연다.
- 검색 옵션(대소문자, 정규식) UI를 새로 만들지 않는다. 기존 FindBar의 입력·이동·개수 표시만 쓴다. 옵션 요청이 오면 다시 연다.
- 새 단축키를 추가하지 않는다. 기존 `find_in_pane`(Cmd+F) 바인딩을 쓴다 (shortcuts.ts).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 변경 전 기준(고칠 대상): 휠 경로는 웹의 `terminal_scroll`을 코어가 Herdr로 보내는데, 이 클라이언트의 세션이 Control 모드이고 pane이 크기를 보고한 뒤일 때만 보낸다. Observe 세션(다른 클라이언트, 예: Swift 셸이 control 중)이나 크기 보고 전 pane에서는 휠이 조용히 버려진다. 이 드롭은 허용되는 동작이 아니라 제거할 결함이며, 구현은 실제 재현으로 원인을 확정한 뒤 D-02·D-03의 상태별 동작으로 바꾼다. | 사실: web/src/terminals.ts:241, herdr-core/src/runtime/events.rs:2486-2516, herdr-core/src/runtime/terminal.rs:319 |
| D-02 | 웹 셸에서 에이전트 pane 위의 휠은 일반 터미널 pane처럼 매번 기록을 스크롤한다. 간헐적 유실을 없애는 것이 목표이며 원인은 고치기 전에 재현한다. 상태별 동작: 이 클라이언트가 control 중인 pane은 즉시 스크롤된다. 크기를 아직 보고하지 않은 pane의 휠은 버리지 않고 크기가 보고되는 즉시 반영된다. 다른 클라이언트가 control 중인 pane은 D-03을 따른다. | 사용자: "에이전트 pane의 스크롤이 안먹음 (일반 터미널에서는 스크롤됨) 되다가 안되네" |
| D-03 | 수정 범위는 운영자의 실제 환경을 포함한다: 같은 Herdr 서버에 Swift 셸이 함께 붙어 있는 경우와 등록된 SSH 기기의 pane. Herdr가 관찰 중인 클라이언트의 스크롤을 허용하지 않으면, 휠을 조용히 버리지 않고 pane에 "다른 클라이언트가 스크롤을 잡고 있다"는 작은 표식을 보인다. | 가정: /please 위임, Herdr가 막을 때 재검토 |
| D-04 | 변경 전 기준(고칠 대상): Cmd+F는 `find_in_pane`에 묶여 있고, 에디터가 있고 View 영역이 그려져 있으면 포커스 위치와 무관하게 에디터 찾기로 간다. 아니면 FindBar가 열려 이 머신의 Herdr로 `pane_find`를 돌리고, SSH 기기 문맥에서는 알림으로 거절한다. | 사실: web/src/keyboard.ts:90-94, web/src/actions.ts:1107-1117, web/src/Overlays.tsx:145, herdr-core/src/live.rs:1811 |
| D-05 | 터미널 pane에 포커스가 있을 때 Cmd+F는 그 pane의 찾기를 열어 기록을 검색한다. Enter/Shift+Enter로 일치 항목을 오가며 pane이 그 위치로 스크롤되고, Escape는 닫고 pane으로 포커스를 돌려준다. View 영역에 포커스가 있으면 Cmd+F는 그 문서 찾기를 유지한다. 즉 새 동작에서는 에디터 존재 여부가 아니라 포커스 위치가 대상을 정한다: 터미널 pane 포커스는 pane 찾기, View·문서 포커스는 문서 찾기. | 사용자: "pane 활성화 된 곳에서 cmd + f 하면 그 안에서 검색되게" |
| D-06 | 등록된 SSH 기기의 pane에서도 그 기기의 Herdr를 통해 찾기가 동작하며, 지금의 거절 알림을 대체한다. | 가정: /please 위임, 기기 제어 경로가 pane 텍스트를 못 읽으면 재검토 |
| D-07 | 현재 모든 View 영역에 표시가 없으면 "No file or diff is open in this Workspace."와 Show Explorer, Open file이 그려진다. S6 D-03이 레이아웃 모드는 공간만 바꾼다고 정했기 때문이다. | 사실: web/src/ViewAreas.tsx:95-121; agents/prd/workspace-navigation-shell/prd.md D-03 |
| D-08 | Workspace View 영역의 마지막 표시가 닫히면 View 영역이 사라지고 에이전트 영역이 그 공간을 차지한다. 빈 상태 패널은 더 이상 그리지 않는다. S6 D-03의 빈 영역 유지는 이 경우에 한해 대체된다. | 사용자: "그 공간의 탭이 0개면 ... 그 공간이 사라져야지" |
| D-09 | Workspace가 고른 레이아웃 모드는 바꾸지 않는다. 파일·diff 등 표시를 다시 열면 같은 모드로 View 영역이 돌아온다. Views-only에서 열린 것이 없으면 에이전트 영역을 보인다. 파일을 여는 중에는 영역이 Opening 상태를 보인다. Explorer와 Changes 도구는 그대로다. | 가정: /please 위임; S6 D-03 모드 유지 |
| D-10 | 검증은 운영자 서버가 아닌 격리된 Herdr 서버와 후보 hided에서 한다. 긴 출력의 TUI pane이 에이전트를 대신하고, 두 번째 클라이언트가 control을 잡은 경우를 덮으며, Playwright와 실제 브라우저 확인으로 스크롤·찾기·View 영역을 보인다. | 가정: /please 위임; CLAUDE.md Performance Guide 격리 규칙 |
| D-11 | 전달은 새 worktree 브랜치에서 PR로 하고 CI 통과 후 사람이 merge를 승인한다. main 직접 push는 브랜치 보호로 불가하다. | 사용자: "worktree파서 새롭게 진행하고 문제없으면 PR올리는 것까지"; agents/config.json delivery.mode=pr |
| D-12 | 원칙 입력: engineering/principles.md와 design/principles.md(oh-my-principle 654485f)를 전부 읽었다. engineering #4·#10(조용한 드롭 금지, 운영자가 행동할 결과로 전달)은 B3, #13(같은 결정에 두 번째 휴리스틱 금지)은 B1의 원인 모델링으로, design #13(행동할 수 있는 상태만 화면에)은 B3의 표식 조건과 B7로 옮겼다. 나머지 규칙은 이 변경에 관찰 가능한 결과가 없다. | 원칙 intake |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 웹 셸에서 Claude·Codex 같은 에이전트 TUI pane 위로 휠을 올리면 매번 이전 출력이 보이고, 내리면 최신 쪽으로 돌아온다. 일반 셸 pane과 같은 반응이며 간헐적으로 무시되지 않는다. | D-01, D-02, D-12 |
| B2 | Swift 셸이 같은 Herdr 서버에 붙어 있어도, 그리고 등록된 SSH 기기의 pane에서도 B1이 동일하게 동작한다. 막 붙은 pane이나 탭을 바꿔 돌아온 pane에서도 첫 휠부터 스크롤된다: 크기 보고 전에 들어온 휠은 크기가 보고되는 즉시 반영되고 버려지지 않는다. | D-02, D-03 |
| B3 | 다른 클라이언트가 control 중인 pane에서 Herdr가 이 클라이언트의 스크롤을 받지 않는다면 휠은 조용히 사라지지 않는다: pane에 다른 클라이언트가 스크롤을 잡고 있다는 작은 표식이 보이고 원인은 진단 로그에 남는다. 스크롤이 가능해지면 표식은 사라진다. | D-03, D-12 |
| B4 | 터미널 pane에 포커스가 있을 때 Cmd+F를 누르면 옆에 에디터가 열려 있어도 그 pane의 찾기 막대가 열리고 입력칸에 포커스가 간다. 브라우저 자체 찾기는 뜨지 않는다. | D-04, D-05 |
| B5 | 찾기 막대에 입력하고 Enter를 누르면 일치 개수(n/total)가 보이고 pane이 그 줄로 스크롤된다. Shift+Enter는 이전 항목, 끝에서는 처음으로 돌아가며, 일치가 없으면 0/0이 보인다. Escape나 닫기는 막대를 닫고 포커스를 그 pane으로 돌려준다. | D-05 |
| B6 | 등록된 SSH 기기의 pane에서 Cmd+F도 B4·B5와 같이 동작하고, 지금의 "not available for <device>" 알림은 더 이상 뜨지 않는다. 기기 연결이 끊겨 검색할 수 없으면 찾기 막대에 그 이유가 한 줄로 보인다. | D-06 |
| B7 | View 영역에 포커스가 있을 때 Cmd+F는 지금처럼 그 문서 안에서 찾는다. | D-05 |
| B8 | Together 모드에서 View 영역의 마지막 파일·diff·기타 표시를 닫으면 View 영역이 사라지고 에이전트 영역이 Workspace 전체를 차지한다. "No file or diff is open in this Workspace." 패널은 보이지 않는다. | D-07, D-08 |
| B9 | 그 뒤 Explorer, Changes, ⌘P로 파일을 다시 열면 View 영역이 같은 Together 배치로 돌아온다. 여는 동안에는 영역에 Opening 상태가 보이고, 실패하면 기존 실패 표시가 보인다. Workspace의 레이아웃 모드 선택은 바뀌지 않는다. | D-08, D-09 |
| B10 | Views-only 모드에서 열린 표시가 없으면 빈 패널 대신 에이전트 영역이 보인다. 표시를 열면 Views-only 배치로 돌아간다. Explorer·Changes 도구의 표시 여부는 영역이 사라지고 돌아와도 그대로다. | D-08, D-09 |

## Technical structure

구조 변경은 없다: 휠은 기존 `terminal_scroll` 이벤트, 찾기는 기존 `pane_find` 이벤트와 FindBar, View 영역은 기존 레이아웃 스냅샷을 쓴다.
스크롤 수정이 코어의 세션 모드 규칙이나 기기 제어 경로를 바꿀 수 있으며, Herdr 호출은 고정된 v0.9.1 계약(`contracts/herdr-api.schema.json`) 안에서만 한다.
기기 pane 찾기는 코어가 이미 가진 기기 제어 경로로 pane 텍스트를 읽는다. 새 WebSocket 메시지나 스냅샷 필드가 필요하면 `contracts/hided-ws.schema.json`과 wire 계약을 같은 변경에서 갱신한다.

## Risks

- 스크롤 원인이 Herdr의 control 소유권 규칙이면(관찰 클라이언트는 스크롤 불가) B1·B2를 Swift 셸 공존 상태에서 완전히 만족할 수 없을 수 있다. 그 경우 B3의 표식으로 드러내고 Herdr 변경 여부는 운영자가 정한다.
- 휠은 고빈도 경로다: docs/PERFORMANCE_TESTING.md에 따라 입력당 추가 작업과 알림 fan-out을 설명하고 입력을 버리지 않는다.
- 검증은 격리된 Herdr 서버(`HERDR_SOCKET_PATH`, 격리 HOME)와 후보 hided로만 한다. 운영자의 Herdr, Swift 앱, 실행 중인 hided(`w9P:p2`)와 mini의 pane은 건드리지 않는다. 기기 pane 검증은 격리된 경로가 없으면 단위·코어 테스트로 하고 확인하지 못한 부분을 PR에 적는다.
- 사용자에게서 구현 전에 필요한 것은 없다.
