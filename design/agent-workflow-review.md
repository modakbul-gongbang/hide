# UI 구현 인계

이 문서와 `design/hide.pen`의 **`Review / UI Handoff /` 묶음만** 최신 구현 목표로 읽는다.
시작 보드는 `CKb4q`이며, 전체 화면 01-06을 먼저 보고 담당 컴포넌트의 상태 시트를 확인한다.
이 문서는 디자인 인계이며 현재 native 앱이 이미 구현했다는 증거 또는 모든 데이터 확장의 구현 승인은 아니다.

## 범위와 정리 결과

사용자의 최종 정리 요청에 따라 Review 최상위 항목을 108개에서 20개로 정리했다.
R4 Pane/focus, 필요한 공통 identity·계층·Workspace·관계 모달·종류별 toolbar, Explorer Git 표시를 한 묶음으로 통합했다.
오래된 전체 화면과 중복 후보는 캔버스에서 제거했으며 이전 상태는 Git commit `d933a87`에 남아 있다.
이전 Final, Project-first, R2/R3/R4 이름을 찾아 조합할 필요가 없다.
R2/R3 헤더와 과한 여백의 Final을 새 구현 목표로 되살리지 않는다.

`System /`, `Component /`, `Screen /`의 기존 보드는 보존했다.
기존 Screen과 최신 Review가 참조하는 9개 master는 90 Shared dependencies에 이동해 ID/ref를 유지했다.
90은 참조 보존용이며 새 화면의 외형 목표가 아니다.
새 master는 채택 전까지 Proposed이고, 구현과 native 검증이 끝난 변경만 Component/Screen으로 승격한다.

이번 UI 범위는 다음과 같다.

- Projects/Agents 두 View, compact identity와 계층, 검색/Recent의 공통 identity.
- Pane 종류별 헤더, 부모·자식 이동, 확대/복원, focus와 inspection 분리.
- 관계 모달과 자식 전용 탭, 부모의 기존 배치 복귀.
- 우측 Overview의 작업 트리 후보와 상태.
- File Explorer의 Git 상태와 기존 파일 조작 유지.

Project 전체 화면 재설계, New Agent/Quick Workspace 개편, Git 탭 제거, Sessions history reader와 retention 정책은 별도 후속이다.
Sessions는 실제 live agent가 있는 surface의 identity 일치만 이번 범위에 포함한다.
과거 기록을 읽는 UI, 종료 이력 저장, 새 임시 Workspace를 이번 작업자가 함께 구현하지 않는다.

## 보드 지도

| 읽는 순서 / 보드 | ID |
| --- | --- |
| 00 Start here | `CKb4q` |
| 01 Parent workbench | `wzrRI` |
| 02 Child dedicated tab | `IhnIO` |
| 03 Explorer Git in workbench | `n2oTPf` |
| 04 Inspect without moving | `AZvEA` |
| 05 Agents status groups | `Snwg0` |
| 06 Relationship modal | `eXxKb` |
| 10 Pane header and focus | `mR198` |
| 11 Narrow and non-agent headers | `b6Nt3Q` |
| 12 Shared agent identity | `ZjPJ6` |
| 13 Hierarchy widths | `INj5G` |
| 14 Workspace disclosure | `g7trZv` |
| 15 Tree interaction states | `m0gZNn` |
| 16 Browser file diff toolbars | `p0Do2` |
| 17 Relationship node | `d8bYuE` |
| 18 Search Recent Sessions identity | `dexoX` |
| 19 Explorer Git states | `BMEZi` |
| 20 Overview scope decision | `c3Lf5` |
| 21 Overview availability | `m5T3n1` |
| 90 Shared dependencies | `Q7exE` |

01은 공통 Workbench master를 포함하고, 02-06은 같은 구조를 ref로 조합한다.
03은 새 Explorer Git 전체 화면이다.
05와 06의 배경 헤더도 최신 master로 교체해 이전 헤더가 구현 참고로 남지 않게 했다.
20에는 미결정인 Overview 기본 범위 하나만 비교안으로 남겼다.
나머지 컴포넌트/상태 시트는 기능 구현 시 필요한 명세다.

## 컴포넌트 → owner → 화면 → 행동 → 수용 조건

아래 owner는 현재 코드의 시작점이며, 신규 projection이나 view를 이미 존재하는 기능처럼 취급하지 않는다.
코드를 바꾸기 전에 `docs/README.md`, `DESIGN.md`, `docs/status-model.md`와 담당 영역의 아키텍처/성능 계약을 읽는다.

| 공유 master | Code owner / 확장 시작점 | 사용 화면·상태 | 클릭 / 키보드 | 수용 조건 |
| --- | --- | --- | --- | --- |
| Agent identity `HXWFK` | `AgentRow.swift`, `AgentRowPresentation`, `AgentNavigatorRow` | 01-06, 12/18; Working/Seen, demand, read/unread, unknown/disconnected | 본문은 agent 열기; 관계 target은 검사 | 동일 ID의 제목/provider/canonical 상태를 공유; surface별 높이는 compact/expanded |
| Tree row `n1zn1O`, segment `N6BTh4` | `LineageGuideView.swift`, `SidebarGrouping.swift`, `HideUI.swift` | 13/15; root/child/grandchild, first/middle/last, wrap/collapse | disclosure와 선택 분리; Left/Right 펼침, Up/Down 이동 | 긴 제목에서도 첫 줄 anchor와 조상 continuing rail 유지 |
| Workspace row `pPtY6` | `HideUI.swift`, `CheckoutCardPresentation.swift` | 01/14; 0/1/다수, expanded/collapsed, missing/disconnected | populated 본문은 펼침; 24pt 열기 target 별도 | 펼침만으로 Pane/tab/read 변경 없음; 접힌 요약과 펼친 자식 중복 없음 |
| Pane header `Z3BnL`, focused pane `ZvLjg` | `ShellView.swift`의 `HideTerminalPaneCard`, `PaneHeaderPresentation`, `PaneLineageHeader` | 01-06, 10/11; role/focus/width/zoom/계측 | 제목/배경은 focus; control은 자기 intent; Tab 후 Enter/Space | 240pt에서 복귀/zoom/overflow/close 보존; 상태 색과 focus 분리 |
| Child chip `j0Sji` | `PaneChildChip`, `PaneChildRow`, `PaneLineagePresentation` | header; 1/다수/partial | 이름은 전용 탭; +N/관계는 inspection | N은 보이지 않는 direct child 수; +0/total 중복 없음 |
| Workbench `OwIFR` | `HideMainView`, `HideTabStrip`, `ShellModel.swift` | 01-06; parent split / child tab / tools | 기존 typed event와 MRU 사용 | child를 operator Pane에 split하지 않음; 복귀 때 authoritative 부모 배치 선택 |
| Browser chrome `KUcQU`, address `eMlZD`, document toolbar `hxdu7` | `BrowserPaneView.swift`, `EditorViewerOverlay.swift` | 11/16; ready/loading/dirty/conflict/image/diff | reload/address/find/wrap/mode/reveal은 각 owner | agent controls 누출 없음; file/diff는 기존 중앙 editor tab 계약 유지 |
| Graph node `FKLct` | 신규 관계 view; `CorePaneChildren`, `CoreLineageStep` | 06/17; selected/unavailable/cross Workspace | 노드는 inspection, Open만 이동, Esc 복귀 | 모달 열기와 노드 선택은 read를 올리지 않음 |
| Overview `G4Sj9`, task row `AdQ5R` | `CheckoutOverview.swift`, `OverviewPresentation`; project agent forest 입력 확장 | 01/02/04, 20/21 | row/Return은 검사; 명시적 Open만 이동 | Git ancestry와 agent lineage 분리; 0/unknown/loading/disconnected 구분 |
| Explorer row `mSu8p`, panel `Wo6qx` | `WorkspaceOutlineView.swift`, `WorkspaceOutlinePresentation.swift`; `changes.rs`, `model.rs`, `runtime.rs` 데이터 확장 | 03/19; M/A/U/R/!, folder/clean/selected, 240-400pt | 기존 파일 열기·disclosure·rename·drag·menu 유지 | Git badge는 별도 action 아님; 파일명과 상태 열이 겹치지 않음 |
| Icon `Nyvom`, tabs `ywjtY`, 기존 panel header `ZYylH` | `HideIconButton`, choice/tooltip/keycap owner | 모든 화면 | registry shortcut과 동일 accessibility help | hover/focus로 footprint 변경 없음; terminal key 입력을 가로채지 않음 |

## 공통 identity와 왼쪽 계층

status 12pt, provider artwork 16pt, title 12pt, 상태·위치 11pt 슬롯을 사용한다.
아이콘/상태/disclosure/title의 열 위치는 행마다 바뀌지 않는다.
provider는 기존 번들 artwork를 사용하고 문자열이나 임의 로고로 대체하지 않는다.
Projects에서 Project > Workspace는 소속이고 agent parent > child는 위임 관계다.
같은 Workspace 위치를 매 행 반복하지 않고 타 Workspace 자식만 위치를 보조 줄에 표시한다.
같은 agent가 물리 Workspace와 관계 링크 양쪽에서 접근되더라도 ID와 count를 중복 집계하지 않는다.

Sidebar는 240/320/344/400pt에서 최대 두 줄 title을 허용한다.
두 줄 초과는 마지막 줄 tail ellipsis이고 full title/path는 tooltip과 동일 accessibility help에 남긴다.
긴 비분리 영문은 문자 단위로 줄바꿈할 수 있지만 provider와 상태 슬롯은 이동하지 않는다.
compact Search/Recent/Overview는 한 줄 title을 쓰며 동일 높이를 모든 surface에 강제하지 않는다.
Search는 검색 정렬, Recent는 MRU를 유지하고 일반 terminal/browser/file/diff의 고유 identity도 보존한다.

tree disclosure는 16pt 슬롯이고 agent depth는 기존 18pt token이다.
부모 trunk는 disclosure 아래부터 행 끝까지 내려온다.
중간 자식은 전체 행 높이만큼 trunk를 통과시키고 첫 줄 상태 중심에서 분기한다.
마지막 자식은 첫 줄 elbow에서 세로선을 끝내고 독립 root로 연결하지 않는다.
다중 행 title의 가운데 높이를 elbow로 쓰지 않는다.
손자 row를 지나는 조상 열은 continuing 정보로 유지한다.
접힌 row 아래에는 자식 rail이 없으며 깊이 2보다 긴 경로는 관계 view로 이어간다.
Pen의 고정 y 좌표를 runtime 코드에 복사하지 않고 실제 row layout anchor로 계산한다.

부모의 canonical status 다음 보조 슬롯에 `자식 3 · 작업 2`를 둔다.
이는 알려진 direct child panes의 total/working이고 후손 전체나 in-process subagent 수가 아니다.
0명은 관계 요약 없음, partial은 알려진 범위로 한정, disconnected는 live 진행 수를 숨긴다.
populated Workspace는 펼침 상태에서 대표 chip을 숨기고, 접혔을 때만 요약한다.
0명은 agent chip 없음, 1명은 +0 없음이다.

Agents의 Needs You / Done / Working / Seen 그룹은 관계 트리로 대체하지 않는다.
delegated 완료를 독립 사용자 Done으로 올리지 않고 Working/Seen ownership 계약을 보존한다.
escalation과 demand, unread/read를 서로 다른 축으로 유지한다.
read는 실제 Hide pane keyboard focus에 의해 갱신하며 inspection/disclosure/검색/모달 열기는 read를 올리지 않는다.

## Pane 헤더와 interaction 계약

첫 줄 28pt는 현재 identity와 actions이고, 알려진 자식이 있으면 24pt 둘째 줄을 추가한다.
tab strip은 배치/문서 탐색, Pane header는 현재 Pane identity와 동작을 맡는다.
같은 제목·상태·Workspace·breadcrumb를 두 곳에 반복하지 않는다.
stable tab label로 바꾸는 부분은 현재 title precedence와 저장된 탭의 fallback/migration을 함께 검토한다.

| 상태 | 표시 | 행동 / 수용 조건 |
| --- | --- | --- |
| root, 자식 0 | 한 줄 | 빈 관계 영역과 0명 요약 없음 |
| child, 자식 0 | 첫 줄에 부모 복귀 아이콘 | 한 줄이어도 child 관계를 식별하고 부모로 돌아갈 수 있음 |
| parent, 자식 1 | 둘째 줄에 방향 아이콘과 child 이름 | +0 없음 |
| parent, 자식 다수 | 첫 child 이름과 나머지 +N | total 중복 없음; +N은 관계 inspection |
| child이면서 parent | 위로 복귀와 아래 자식 관계를 동시에 표시 | 현재 identity가 가장 강하고 부모 제목은 보조 톤 |
| 240/320/344/400pt | title 한 줄 말줄임 | 부모 이름 먼저 숨김; provider/status와 24pt 복귀/zoom/overflow/close 유지 |
| zoom | maximize가 minimize로 교체 | 우측 같은 위치에서 Pane 확대/배치 복원; 글자 배율과 다름 |
| 미계측 detected agent | 안내 아이콘 | 자식 없음과 계측 불가를 구분; 알려진 자식 없으면 한 줄 |
| partial | 안내 + 확인한 child만 | 전체 count인 것처럼 보이지 않게 help로 범위를 설명 |
| disconnected / parent 불가 | retained identity와 이유 | 종료 근거가 없으면 종료라고 쓰지 않음; capability에 따라 열기/복귀 가능 여부 결정 |
| 일반 terminal/browser/file/diff | 종류별 title과 actions | agent status/관계/unknown 행 없음; 문서 toolbar는 기존 editor 의미 유지 |

부모 복귀는 corner-up-left, 자식 관계는 corner-down-right 아이콘이다.
부모 이름은 480pt 미만에서 숨기고 넓어도 현재 title 공간이 부족하면 숨긴다.
우측 순서는 zoom/restore, overflow, close이고 각각 24pt target이다.
fork/split/ports/ancestor/sibling은 실제 capability에 따라 기존 intent로 overflow에서 접근한다.
hover-only 항목은 없는 기능을 새로 암시하지 않으며, 키보드에서는 모든 action에 접근할 수 있어야 한다.
tooltip과 accessibility help는 전체 제목, Workspace와 action을 동일하게 제공한다.

부모 action은 기존 배치로 복귀하고 child 이름은 별도 전용 탭을 연다.
pending은 같은 target에 표시해 중복 실행을 막고, 실패 시 기존 Pane과 배치를 유지하며 이유와 재시도를 노출한다.
관계 아이콘/모달/노드 선택은 검사이고 명시적 Open만 이동한다.
Esc는 invocation control로 focus를 되돌린다.
터미널 responder의 화살표 키와 입력을 header나 tree의 전역 handler가 가로채지 않는다.

| 시각 신호 | 의미 |
| --- | --- |
| Pane header selected wash | 현재 보여주는 선택 Pane |
| Pane 바깥 primary hairline | 실제 터미널 keyboard focus |
| Overview row selected wash | inspector에서 살펴보는 task ID |
| Overview row Pane icon | 중앙에서 보여주는 task ID |
| Overview row outline | 그 row의 keyboard focus |
| 제목 semibold / regular | 기존 unread / read 표현, 부모/자식 역할이 아님 |

`HideTerminalPaneCard.isFocused` 하나를 실제 native responder와 같다고 가정하지 않는다.
shown Pane과 keyboard responder를 분리한 입력을 owner에서 확정하고 native로 검증한다.
Overview로 focus가 이동하면 terminal outline은 꺼지지만 현재 Pane의 shown wash는 유지한다.
상태 색은 focus로 바뀌지 않는다.

## Explorer Git 표시

기존 `WorkspaceOutlineView`는 22pt row와 file-type icon/name을 사용하며 Git decoration slot은 아직 없다.
이번 표본은 기존 22pt 밀도를 유지하고 오른쪽 12pt Git 문자 슬롯을 추가한다.
선택된 파일명도 기본 primary를 유지하고 Git 상태는 문자와 semantic color로 별도 판독한다.
clean에서도 빈 상태 슬롯을 예약해 파일명의 끝 위치가 흔들리지 않는다.
disclosure는 기존 파일 tree 동작과 inset을 유지하며 agent tree의 18pt depth 규칙을 가져오지 않는다.
Pen의 file-code icon은 slot 표본이고 구현에서는 기존 SetiFileIconCatalog의 실제 파일 아이콘을 유지한다.

| 표시 | 뜻 / 데이터 | 동작 / 수용 조건 |
| --- | --- | --- |
| M | Modified | 기존 파일 열기 |
| A | Added | 같은 파일의 index/worktree 조합은 canonical 상태로 정규화 |
| U | Untracked | raw Git porcelain U를 그대로 표시한 것이 아님 |
| R | Renamed 목표 상태 | 실제 rename 정보가 전달될 때만; 현재 경로는 Explorer, 이전→현재 비교는 Changes |
| ! | Conflict 목표 상태 | 실제 conflict enum/근거가 필요; Modified 색으로 숨기지 않음 |
| 폴더 ● | descendant 변경 존재 | 파일 M이 아니며 폴더 펼침 유지; count 중복 없음 |
| 공백 | 확인된 clean 또는 non-Git/unsupported에서 장식 없음 | 실패를 clean으로 판정하지 않음 |
| selected/hover/focus | 기존 row interaction | Git 문자와 상태 색이 사라지지 않음 |
| 삭제 | Explorer에 가짜 파일 row 없음 | Changes에서 D |
| editor dirty | 아직 저장하지 않은 버퍼 | Git M과 분리 |

파일 이름 클릭/Enter는 기존 중앙 editor tab 열기, disclosure는 펼침이다.
Git badge는 별도 버튼이 아니며 클릭을 가로채지 않는다.
rename, drag move, context menu, Copy Path, Reveal, native selection을 보존한다.
파일명과 전체 경로, 상태의 풀네임을 tooltip/accessibility help로 제공한다.
보드 19의 240/320/344/400pt에서 상태 슬롯을 남기고 파일명만 말줄임한다.

Git loading/failure는 파일 목록을 지우지 않고 panel notice로 표시하며 retry는 기존 refresh 경로에 연결한다.
파일 목록 자체의 loading/empty/read failure는 기존 Explorer 상태를 사용한다.
non-Git와 unsupported remote는 Git 상태를 합성하지 않는다.
stale 값을 유지할 경우 이전 결과임을 알리고 현재 clean이라고 주장하지 않는다.
폴더 집계는 같은 Workspace의 고유 변경 경로에서 파생하며 lazy로 펼친 자식만 보고 전체가 clean이라고 판단하지 않는다.

### 구현 전 반드시 해결할 데이터 항목

현재 `ChangedFileStatus`는 Modified/Added/Deleted/Untracked 네 가지다.
`from_porcelain`은 Rename을 Modified로 축약하며 독립 Conflict 상태를 전달하지 않는다.
따라서 R/!은 구현 목표 표본이며 기존 snapshot만으로 그릴 수 있다고 가정하면 안 된다.
현재 `ChangedFileSnapshot`에는 이전 rename 경로도 없으므로 이전→현재 표시에 필요한 입력을 확정해야 한다.
문자열 파일명이나 UI text를 파싱해서 rename/conflict를 추측하지 않는다.

현재 `Runtime::changes_request`는 Changes panel 또는 active Diff가 있어야 request를 만든다.
Explorer만 켠 상태에서는 기존 Changes projection에 바로 기대어 최신 Git badge를 그릴 수 없다.
Explorer 표시를 위한 요청/가용성 입력과 캐시·갱신 정책을 기존 reader 경계에서 확장하는 작업을 포함해야 한다.
`ChangesReader`와 정규화 경계를 재사용하고, row 생성/hover/scroll마다 git subprocess를 실행하지 않는다.
core lock 안의 blocking I/O 금지와 background refresh 경계는 그대로 지킨다.
구현 시 `docs/PERFORMANCE_TESTING.md`에 따라 입력당 작업량과 notification 범위를 검증한다.
이는 엔지니어링 원칙 7의 기존 owner 확장, 원칙 4의 실패와 빈 상태 분리를 따른다.

## Overview에 남은 한 가지 구조 선택

20의 A(추천)는 프로젝트 전체 현재 task forest + 선택한 task의 짧은 상세다.
왼쪽은 Workspace 소속에서 작업을 찾고, 오른쪽은 Workspace를 넘는 위임 관계와 작업 상태를 확인한다.
B는 현재 작업의 확인 가능한 최상위 조상과 그 자손만 보여줘 다른 독립 작업을 숨긴다.
기본 범위의 최종 선택은 아직 사용자 확정 전이며 다른 UI 작업의 선행 조건이 아니다.
Overview 작업을 구현하기 전에 이 선택을 확정한다.

상단은 Project 이름/상세, 작업/Git 보기, 현재 Pane 찾기, 검색이다.
task root에 Workspace를, 타 Workspace child에 위치를 보조 슬롯으로 표시한다.
아래 inspector는 선택한 identity/status, Workspace, 명시적 Open/Return과 Workspace 상세 진입만 둔다.
Workspace 변경 파일 수를 agent 개인 산출물 수라고 표시하지 않는다.
GitHub/disk/cleanup의 Project 상세 이동은 기능 삭제가 아닌 배치 제안이며 기존 진입점을 보존해야 한다.
Git 탭을 제거하지 않고 Git ancestry와 agent delegation 선을 섞지 않는다.

현재 Overview의 대표 agent 한 개와 Git Tree가 project task forest의 데이터 계약을 충족한다고 가정하지 않는다.
stable agent ID, lineage parent ID, Workspace, detected/instrumentation와 availability 입력이 필요하다.
retired parent ID 누락을 종료 또는 독립 root의 근거로 만들지 않는다.
행/Return/Locate/filter는 검사만 하고, 명시적 Open만 기존 typed Pane 선택 intent로 이동한다.
검색 결과는 확인 가능한 ancestor 경로를 남겨 부모가 없는 것처럼 왜곡하지 않는다.
조회 전은 loading, 확인한 0개는 empty, 연결 단절은 retained 관계 + Disconnected다.
검색 결과 없음은 query clear를 제공하고 기존 inspector를 유지한다.
remote local-only 제약, 일부 조회 실패, no agent 상태를 현재 owner의 availability로 처리한다.
과거 완료 작업 아카이브는 live projection으로 약속하지 않는다.

## 다음 작업자의 실행 순서와 수용 기준

1. 공통 identity/Workspace/tree와 header를 master 기준으로 구현하고 실제 240pt부터 확인한다.
2. parent/child/zoom/focus 이동을 한 흐름으로 연결하고 read와 기존 배치 보존을 검증한다.
3. Explorer Git의 data gate를 해결하고 03/19 기준으로 기존 file operations와 함께 검증한다.
4. 검색/Recent/관계 view에서 identity와 inspection 동작을 통일한다.
5. Overview 기본 범위 선택과 입력 확정 후 20/21을 구현한다.

| 검증 대상 | 반드시 확인할 결과 |
| --- | --- |
| 계층 | root/child/grandchild/여러 형제/마지막/접힘, 긴 한글·영문에서 rail 연결과 고정 열 유지 |
| focus/read | 부모→자식→부모 복귀, 모달 inspection, Overview 검사에서 read가 의도 없이 갱신되지 않음 |
| header | 240/320/344/400pt, 0/1/다수/partial/미계측/단절, zoom 위치와 header 높이 유지 |
| Pane 종류 | plain terminal/browser/file/diff에 agent controls 없음; 문서 편집/주소 입력 focus 유지 |
| Explorer | M/A/U/R/!/clean/folder, selected/hover/focus, 삭제/rename/dirty 구분, Git 실패에서도 파일 탐색 가능 |
| Explorer 데이터 | Explorer만 보일 때 refresh, 다른 Workspace 전환 시 잘못된 badge 재사용 없음, row별 git 실행 없음 |
| native | 한 개의 식별된 앱과 격리 서버, 운영자 Pane 유지, 실제 screenshot과 keyboard/responder 확인 |
| 계약 | 해당 UI 테스트와 `gen-pen.mjs`, `check-design-contract.mjs`; 바뀐 owner guide 함께 갱신 |

디자인 원칙 5에 따라 기존 밀도·tokens·control owners를 보존하고 원칙 7에 따라 상태를 고정 슬롯과 아이콘으로 표현했다.
원칙 11에 따라 미결정 Overview 범위를 비교안으로 남겼고 원칙 12에 따라 한글/영문과 실제 좁은 폭을 렌더로 확인했다.
정적 검사는 native UI 동작 또는 미적 승인을 대신하지 않는다.
Pen의 Inter/JetBrains Mono 대체와 opacity 백분율 차이는 여전히 native 렌더와 별도로 검증해야 한다.
표본 제목과 수는 상태 fixture이며 실제 프로젝트 측정 결과가 아니다.

이번 정리는 Pen과 이 문서만 수정하며 앱 실행·설치·제품 코드는 변경하지 않았다.
실행 산출물은 `agents/runs/ui-handoff-cleanup/`에만 보관한다.
기존 master 참조를 추적해 이동/삭제 후 dangling ref가 없음을 확인했다.
최종 저장 후 `gen-pen.mjs`, `check-design-contract.mjs`, `git diff --check`가 통과했다.
파일을 다시 열어 Review 20개, dangling ref 0건, 가시 요소 clipping 0건, 미정의 변수 0건을 확인했다.
계약 검사는 133개 생성 token, 11개 회귀 fixture와 component/control ownership을 확인했다.
Explorer와 공통 Sidebar의 240/320/344/400pt, 변경한 전체 화면 및 시작 안내를 렌더로 검사했다.
실제 native 앱의 focus/read/resize/파일 조작은 이번 디자인 정리에서 실행 검증하지 않았다.
