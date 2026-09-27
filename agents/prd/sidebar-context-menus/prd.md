---
topic: "사이드바 우클릭 메뉴: Project·Checkout·Agent 행"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "사이드바 세 행 종류의 우클릭 메뉴를 보드대로 채우는 사용자 대면 변경이며, 파괴적 항목(Remove, Delete worktree, Close tab)은 기존 확인 흐름을 재사용하고 데이터·권한·외부 효과는 없다."
source_intake: "agents/interview/sidebar-context-menus/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: 사이드바 우클릭 메뉴

## Goal

hide 사용자가 Projects 사이드바의 Project 행, Checkout 행, Agent 행을 우클릭하면 보드 섹션 3에 확정된 항목이 다 있는 메뉴를 보고, 거기서 열기·새 탭·경로 복사·Finder·고정·제거·대표 체크아웃 지정·에이전트 정지를 한 번에 한다.
사용자의 말: "우클릭 메뉴는 좀 sparse 한 느낌인데", "T6 우클릭 메뉴를 이번 배치에 다시 넣어 주세요", "hide에 보이는 행이면 모두 같은 프로젝트로 취급".
시각 참조는 `agents/runs/ux-fixes-2026-09-27/design/board-v3.pen` 섹션 3(항목·순서·구분선은 `design/build.mjs` 157-207행).

## Non-goals

- Tab 메뉴는 바꾸지 않는다; #203이 끝냈다 (D-01).
- "Mark as seen"과 "Stop agent…"는 넣지 않는다: Herdr 0.9.1에 pane을 seen으로 만드는 메서드도 agent.stop도 없고, 사용자가 Stop agent를 빼기로 했다. 재검토: Herdr가 그 메서드를 줄 때 (D-05).
- Checkout 메뉴의 "Open pull request #n"은 T7(checkout-pr-glyph-card)의 항목이다; 이 PR은 그 항목을 옮기거나 바꾸지 않는다 (D-03).
- 라이브러리 Component는 바꾸지 않는다; Screen 시트만 갱신한다 (D-10).
- 사이드바 헤더(탭 스트립, 검색, 전역 Overview)는 T9의 것이다 (D-12).
- design/principles.md 규칙 3·5·6과 engineering/principles.md 규칙 7이 D-02~D-08·D-14를 정한다 (D-13).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 지금 Project 행은 Pin/Unpin·New worktree…·Remove project…(registered만), Checkout 행은 Set purpose…·Delete worktree…, Agent 행은 메뉴가 없다. 메뉴는 `EntryContextMenu`가 열고 항목은 `workspaceManage.ts`의 빌더가 만든다(`MenuItem`: id, label, unavailable 사유, separated). Herdr 0.9.1에는 agent.stop과 seen 표시 메서드가 없고, T8이 `set_primary_checkout` 이벤트를 만들었다. | 저장소 사실 (qa-log D-01) |
| D-02 | Project 행: Open Overview, New worktree…, New tab in main ⌘T, ─, Reveal in Finder, Copy path, ─, Pin/Unpin, Remove project…. | 사용자 보드 승인 (qa-log D-02) |
| D-03 | Checkout 행: Open, New tab here ⌘T, Open pull request #n(T7), ─, Set purpose…, Set as default checkout, Copy branch name, Copy path, Reveal in Finder, ─, Delete worktree…(destructive). Set as default checkout은 `set_primary_checkout`을 보낸다. | 사용자 보드 승인 (qa-log D-03) |
| D-04 | Agent 행: Show(번호가 있으면 ⌥n 표기), ─, Copy title, Copy session id, ─, Close tab…. Show는 행 클릭과 같은 이동. Mark as seen과 Stop agent…는 D-05로 제외. | 사용자 보드 승인 + "항목 빼기" (qa-log D-04) |
| D-05 | Mark as seen과 Stop agent… 제외: Herdr 0.9.1에 seen 표시 메서드도 agent.stop도 없다; 사용자가 Stop agent를 "항목 빼기"로 정했다. Herdr가 메서드를 주면 재검토. | 사용자 답변 (qa-log D-05) |
| D-06 | Close tab… = 그 pane이 든 탭을 기존 탭 닫기 경로(확인 포함)로 닫는다. Copy title = 행 제목, Copy session id = 스냅샷 agent의 session_id. | 가정 (qa-log D-06) |
| D-07 | 보드 노트대로 "Finder/Terminal은 Electron에서만. 회색 항목은 상황에 따라 비활성(PR 없음, 원격 등)": Reveal in Finder는 Electron 전용으로 기존 브리지로 열고 브라우저 호스트에는 항목이 없다. 원격 디바이스 행에서 Reveal·Set as default checkout·Delete worktree…는 사유와 함께 회색. Copy path/branch는 navigator.clipboard. | 사용자 보드 노트 승인 (qa-log D-07) |
| D-08 | Open = 펼침 없는 `focus_checkout`. New tab in main = 프로젝트의 대표 체크아웃에 `create_tab` 뒤 포커스; New tab here = 그 체크아웃에 같은 동작; ⌘T 표기는 레지스트리 `new_tab` chord. Open Overview = 프로젝트 Overview 화면. | 가정 (qa-log D-08) |
| D-09 | Set as default checkout은 이미 대표·플레인 폴더·원격에서 사유와 함께 unavailable. Open pull request는 T7 규칙. | 가정 (qa-log D-09) |
| D-10 | Pen: 라이브러리 불변; `pen-screens.mjs`의 Projects Sidebar 시트에 메뉴 열린 프레임을 Project·Checkout·Agent 하나씩 추가(gen-screens). | 가정 (qa-log D-10) |
| D-11 | 검증: vitest(세 빌더의 항목·순서·구분선·unavailable, 호스트별 Reveal, 미등록 행 Pin/Remove), 웹 e2e 1개(New tab in main → 탭; Set as default checkout → 집 아이콘 이동; Copy session id → 클립보드; 미등록 행 Pin → 등록+고정), gen-screens·check-design-contract. | 가정 (qa-log D-11) |
| D-12 | T7 머지 후 시작. please, Claude Implementor --effort high, PR 배포, Observer 자동 머지. T9와 병렬 가능, sidebar.tsx 헤더는 건드리지 않음. | 사용자 승인 (qa-log D-12) |
| D-13 | 원칙 intake: design 3·5·6이 D-02~D-06에, engineering 7이 D-06·D-08·D-14에 반영. | 가정 (qa-log D-13) |
| D-14 | 미등록 프로젝트 행(Herdr 워크스페이스라서 보이는 행)에도 Pin/Unpin과 Remove가 똑같이 있다. 미등록 행의 Pin = 그 행의 기기와 프로젝트 루트로 등록하면서 바로 고정(고정 = 등록 + pinned). 미등록 행의 Remove = 기존 제거 흐름(지우는 대로 알림)으로 그 프로젝트의 pane을 닫는다; 등록이 없으니 Herdr가 워크스페이스를 내리면 행이 사라진다. UI_BEHAVIOR.md와 `projectMenu` 주석도 같은 변경에서 고친다. | 사용자 결정 (qa-log D-14) |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Project 행을 우클릭하면 Open Overview, New worktree…, New tab in main ⌘T, 구분선, Reveal in Finder, Copy path, 구분선, Pin(또는 Unpin), Remove project… 순서의 메뉴가 열린다. 브라우저 호스트에는 Reveal in Finder가 없다. | D-02, D-07 |
| B2 | Open Overview는 그 프로젝트의 Overview를 연다; New tab in main은 대표 체크아웃의 Workspace에 새 탭을 만들어 앞에 두고; Copy path는 프로젝트 루트 경로를 클립보드에 넣고; Reveal in Finder는 그 폴더를 Finder에서 보인다. | D-07, D-08 |
| B3 | 미등록 프로젝트 행에도 Pin/Unpin과 Remove project…가 있다. Pin은 그 행을 등록하면서 고정하고(행이 고정 행으로 바뀜), Remove project…는 기존 제거 흐름대로 알림을 띄우고 그 프로젝트의 pane을 닫으며, Herdr가 워크스페이스를 내리면 행이 사라진다. | D-14 |
| B4 | Checkout 행을 우클릭하면 Open, New tab here ⌘T, Open pull request #n(PR 있을 때), 구분선, Set purpose…, Set as default checkout, Copy branch name, Copy path, Reveal in Finder, 구분선, Delete worktree… 순서의 메뉴가 열린다. | D-03 |
| B5 | Open은 행 클릭의 열기와 같이 그 Workspace를 앞에 두되 에이전트를 펼치지 않고; New tab here는 그 체크아웃에 새 탭을 만들어 앞에 둔다; Copy branch name과 Copy path는 클립보드에 넣는다. | D-08 |
| B6 | Set as default checkout을 고르면 사이드바에서 집 아이콘과 맨 위 자리가 그 체크아웃으로 옮겨가고 이전 대표는 보통 행이 된다; 이미 대표인 체크아웃·플레인 폴더·원격 체크아웃에서는 항목이 회색이고 hover에 사유가 보인다. | D-03, D-09 |
| B7 | Agent 행을 우클릭하면 Show(⌥n이 있으면 표기), 구분선, Copy title, Copy session id, 구분선, Close tab… 순서의 메뉴가 열린다. Show는 그 pane으로 이동한다. Mark as seen과 Stop agent…는 없다. | D-04, D-05 |
| B8 | Copy title은 행에 보이는 제목을, Copy session id는 그 에이전트의 세션 id를 클립보드에 넣는다. Close tab…은 지금의 탭 닫기 확인 흐름으로 그 탭을 닫는다. | D-06 |
| B9 | 원격 디바이스의 행에서는 Reveal in Finder·Set as default checkout·Delete worktree…가 회색이고 hover에 사유가 보인다; 나머지 항목은 로컬과 같다. | D-07 |
| B10 | 파괴적 항목(Remove project…, Delete worktree…, Close tab…)은 지금의 확인 시트를 거치고, 메뉴의 나머지 항목은 즉시 실행된다; 새로 생기는 배너나 알림은 없고, Remove project…의 지우는 대로 알림은 지금 있는 흐름 그대로다. | D-06, D-14 |
| B11 | docs/UI_BEHAVIOR.md의 사이드바 메뉴 절과 `projectMenu` 주석이 새 항목과 미등록 행 규칙을 서술하고, Screen / Projects Sidebar 시트에 메뉴 열린 프레임 세 개가 있으며 gen-screens와 check-design-contract가 통과한다. | D-10, D-14 |

## Technical structure

- web: `workspaceManage.ts`의 `projectMenu`/`checkoutMenu`/`folderMenu` 확장과 새 `agentMenu`, `sidebar.tsx`와 에이전트 행에 `EntryContextMenu` 연결, `actions.ts`에 항목 실행(기존 `focus_checkout`·`create_tab`·`set_primary_checkout`·pane/tab 닫기·pin·remove 이벤트 재사용, 미등록 행 Pin의 등록+고정), 데스크톱 브리지의 기존 reveal 경로 사용(새 IPC는 preload 확인 뒤에만), `pen-screens.mjs` 프레임 3개.
- core: 미등록 행 Pin이 등록과 고정을 한 이벤트로 하지 못하면 등록 이벤트의 페이로드에 pinned를 더한다(한 동작 = 한 이벤트); 그 외 core·Herdr 계약 변경 없음.
- 바뀌지 않음: Tab 메뉴, 라이브러리 Component, 사이드바 헤더.

## Risks

- T7과 같은 `checkoutMenu`를 고치므로 T7 머지 뒤에 시작하고, T9와는 파일 겹침을 헤더 밖으로 한정한다.
- 미등록 행의 Pin이 등록을 만드는 것은 새 등록 경로다; 등록 실패는 진단과 함께 행을 그대로 두고, 성공만 고정 행으로 바꾼다.
- 사용자가 미리 해야 할 일: 없음.
