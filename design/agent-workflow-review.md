# Agent workflow design review

Status: the user accepted the overall direction of complete screens 01-04 on 2026-09-14; this is not an approved implementation contract.
The product code and installed app are unchanged by this review.
The editable source is [hide.pen](hide.pen), with the latest complete-screen proposal under `Review / 2026-09-14 Project-first IA /`.
The existing `Screen /` boards continue to describe the existing design; this proposal does not silently supersede them or the current contracts in `DESIGN.md`.

## Decision history and next tasks: 2026-09-14

### Final 상세 시안: Project Overview 제외

후속 요청에서 사용자는 아래 변경 순서 1~6 중 Project Overview를 제외한 상세 디자인을 먼저 요청했다.
새 구현용 검토 기준은 `Review / 2026-09-14 Final /`이며 `00 Start here`부터 읽는다.
Final은 이번 상세 시안 묶음의 이름이며 제품 구현 완료나 아직 보지 않은 상태의 최종 사용자 검수를 뜻하지 않는다.
기존 Project-first 01-04와 이전 탐색 보드는 이력으로 보존했다.
Project Overview와 Task Kanban은 이번 변경 대상이 아니다.

| 화면 | 노드 | 변경 내용 |
| --- | --- | --- |
| F01 Projects and workspace | h2s8i | 기존 Projects 계층, 부모 요약, 간결한 자식 들여쓰기, 별도 Workspace 열기 |
| F02 Agents view | S9KrxV | 기존 네 그룹 유지, 공통 상태와 부모 요약, 자식의 위임 소속 |
| F03 Delegation graph | y4omg | 부모 Pane에서 관계 모달, 노드 선택과 실제 이동 분리 |
| F04 Child tab and Sessions | dagQX | 전용 자식 탭, 부모 배치 복귀, Workspace 범위의 Sessions |
| F05 Changes and diff | zZkVy | 오른쪽 변경 목록, 중앙 읽기 전용 diff, 기존 작업 탭 보존 |
| F06 Session reader | qiR4M | 원본 대화 발췌, 별도 읽기 전용 기록 탭, 도구 결과 기본 접힘 |
| F07 New Agent and Quick workspace | J3CUcI | 생성 위치 명시, 프로젝트 독립 Workspace, 명시적 Start |
| F08 Search | uytmT | 검색에 공통 Agent identity와 상태 적용 |
| F09 Recent Panels | aS0mm | MRU 순서·미리보기·확정 규칙 유지, 상태 표시 일치 |

| 컴포넌트 시트 | 노드 | 구현 시 변경/재사용할 부분 |
| --- | --- | --- |
| C01 Agent item | hjWwg | 기존 AgentRow를 확장해 상태·제목·제공자·위치·선택·관계 요약의 의미 통일 |
| C02 Navigation rows | Y8ipD | Workspace 행의 펼침과 명시적 열기 분리, 빈/없는 경로/단절 상태 |
| C03 Lineage summary | T3J4uA | Pane 자식 ID 나열 대체, 단일/미계측/부분/단절/정체 상태 |
| C04 Explorer Git rows | BMEZi | 기존 파일 행에 수정/추가/미추적/충돌/폴더 변경 표시 |
| C05 Session item and availability | uUT8O | 세션 identity+원본 발췌, 로딩/빈/무결과/미지원/실패/부분/오래된 기록 |
| C06 New Agent states | j9LXaV | provider/Workspace 선택, 실행 중/생성 실패/실행 실패 |
| C07 Narrow and interaction states | O6gzR | 320/344/400pt 긴 한글, hover/pressed/disabled/위임 강조 |
| C08 Branching graph and recovery | zVCTn | 분기 엣지, 타 Workspace 자식, 검색/맞춤/확대축소, 종료/부분/단절 |

새 재사용 master는 Final Agent item `FNgJ2`, Workspace row `YOjWl`, Lineage summary `P8ejA`다.
기존 Agent identity, Agent node, Explorer entry, Session item master를 함께 재사용한다.
기존 primitive 버튼·탭·검색·tooltip의 네이티브 owner를 확장하고 화면별 별도 스타일을 구현하지 않는다.
컴포넌트 상태 시트와 전체 화면은 구현 의도를 담은 정적 도안이며 모든 버튼/입력이 실제 동작하는 프로토타입은 아니다.

#### 이번 시안의 구체적 선택

Workspace의 기존 행 전체 펼침 동작은 보존하고 별도 열기 버튼을 제공한다.
접기/펼치기는 Pane 포커스·읽음·프로세스를 변경하지 않는다.
부모 아래의 같은 Workspace 자식은 하나의 들여쓰기와 단순한 연결선으로 표현한다.
Agents 그룹은 관계 트리로 교체하지 않으며 자식은 기존 Working/Seen 및 escalation 규칙을 따른다.
타 Workspace 관계는 소속을 표시하고 실제 Pane으로 이동하며 중복 집계하지 않는다.
Agent 선택은 기존 탭/Pane 이동만 하며 자동 Zoom이나 분할 배치 교체를 하지 않는다.
자식은 별도 탭으로 열고 부모의 기존 분할 배치는 남겨둔다.
관계 모달은 배경 조작을 막고 키보드 포커스를 내부에 한정하며, Esc/닫기는 이전 포커스로 복귀한다.
노드 선택과 검색/MRU 미리보기는 읽음 처리하지 않는다.
읽음 처리는 기존 실제 Pane 포커스 계약을 따른다.
오른쪽 도구는 Workspace 범위와 사용자가 선택한 섹션을 유지한다.
최종 화면의 Git details는 기존 Git 기능에 대한 명시적 진입점이며, 이력·워크트리 관리 기능을 삭제하라는 지시가 아니다.
기존 기능을 동일하게 열 수 없는 구현 단계에서는 기존 Git 탭도 유지한다.
삭제된 파일은 Explorer의 가짜 행이 아니라 Changes에서 표시한다.
Sessions 목록은 실제 원본 메시지 발췌이며 별도 AI 요약 생성 기능을 추가하지 않는다.
도구 결과는 목록에서 제외하고 읽기 화면에서 명시적으로 펼친다.
Quick workspace는 프로젝트 독립 실행 위치로 표시하고 Start 전에는 실행하지 않는다.
기존 Scratch 파일·세션을 자동 삭제하거나 /tmp로 이동하지 않는다.

#### 구현 게이트와 검증 범위

Sessions는 지원 provider의 원본 기록·Workspace 연결·가용성·보존/개인정보 계약을 확인한 뒤 구현한다.
원본이 없을 때 샘플 대화로 채우지 않고 C05 미지원 상태를 사용한다.
Quick workspace의 물리 경로와 기존 Scratch 이관/보존 정책은 구현 전 명시적으로 확정한다.
이 두 데이터 결정은 나머지 탐색/관계/Explorer 작업의 선행 조건이 아니다.
각 화면의 새 동작은 구현 시 현행 DESIGN.md, status-model.md, 관련 테스트와 함께 갱신한다.
Pen 검증은 레이아웃·문자·토큰·band를 확인하며 실제 앱 동작을 증명하지 않는다.
네이티브 검증에서는 실제 320/344/400pt, 키보드, 읽음, 타 Workspace 이동, 깊은 관계와 종료된 노드, 단절 복구를 확인한다.
Pen에서 opacity 변수 0.45가 0.0045로 해석되는 차이를 확인해 C07은 기존 secondary/muted 색으로 낮은 강조를 표현했다.
제품 구현은 이 도안의 대체색을 새로운 상태 규칙으로 복사하지 않고 HideTheme의 원래 opacity 값을 사용한다.
Project Overview 작업은 이 Final 묶음 이후 별도로 진행한다.

### 합의된 범위

사용자는 01-04 화면의 큰 흐름을 괜찮다고 평가하고, 이력 보존과 IDE 변경 태스크 정리를 요청했다.
Projects / Agents 두 가지 왼쪽 View는 유지한다.
핵심 개선 대상은 Project Overview 캔버스이며, Workspace별 작업 진행과 PR 상태를 인지부하가 낮게 보여준다.
Workspace는 별도 현황판을 추가하지 않고 기존 탭과 Pane 배치를 여는 방향을 유지한다.
관계 모달과 자식 전용 탭 진입은 03-04의 흐름을 따른다.
이 수용은 왼쪽 자식 행의 상세 구조, 모든 샘플 문구, Sessions 데이터 계약, Git 탭 제거, 임시 Workspace 수명까지 승인한 것은 아니다.
이번 요청에서는 문서만 갱신하며 Pen 보드, 제품 코드, 설치 앱은 변경하지 않는다.

### 탐색 이력

| 단계 | 검토한 방향 | 현재 판단 |
| --- | --- | --- |
| 최초 문제 제기 | 깨진 관계선, 커밋 중심 Overview, 자식 ID 칩, Git/Explorer, Sessions, 검색/MRU 불일치, Chat/Scratch 용어 | 변경 태스크의 출발점으로 보존 |
| 초기 Review | 개별 컴포넌트와 전체 작업 현황, 여러 캔버스 형태 | 구조 탐색이며 그대로 구현하지 않음 |
| UX walkthrough | 왼쪽·가운데·오른쪽과 진입점을 함께 설명 | 전역 현황과 Workspace 현황이 혼재해 개념이 복잡했음 |
| Project-first 01-04 | 프로젝트 현황 → 기존 작업 배치 → 관계 모달 → 자식 탭 | 사용자가 큰 방향을 수용한 최신 기준 |
| 이번 후속 논의 | Projects / Agents 유지, 왼쪽 item과 자식 표현 재검토 | 아래 추천안을 검토한 뒤 상세 디자인 결정 |

과거 보드는 탐색 기록으로만 남기며, 구현자는 최신 01-04와 이 절부터 읽는다.
현재 제품 동작의 근거는 여전히 DESIGN.md와 docs/status-model.md이며, 이 문서가 이를 소급 변경하지 않는다.

### 왼쪽 패널 추천안: 아직 미확정

원칙 2인 작업 흐름 중심 구성과 원칙 7인 구조의 시각적 표현을 적용해, 위치 탐색과 위임 관계 탐색을 분리한다.
Projects는 Project > Workspace > 그곳의 에이전트를 찾는 위치 탐색으로 유지한다.
Agents는 기존 Needs You / Done / Working / Seen 그룹과 상태 우선순위를 유지한다.
왼쪽 전체를 노드 그래프로 만들거나 별도 View를 추가하지 않는다.

| Item | 기본 표시 | 선택 또는 별도 동작 |
| --- | --- | --- |
| Project | 이름, 펼침 상태 | 이름으로 Project Overview 진입, 펼침은 독립 동작으로 구분하는 안 검토 |
| Workspace | 브랜치/폴더명, primary 등 체크아웃 속성, 기존 상태 요약, PR | 작업 공간 열기와 펼침을 명확히 분리하는 안 검토 |
| 부모 Agent | 상태 기호, 제공자, 작업 제목, 필요한 보조 정보 | 행 선택은 Pane 포커스, 자식 요약은 별도 관계 보기 진입점 |
| 부모의 자식 요약 | 예: 자식 3 · 작업 중 2, 확인 가능한 실제 데이터만 | 접기/펼치기와 관계 모달 열기를 구별 |
| 펼쳐진 자식 | 동일 Agent item, 한 단계 들여쓰기, 부모가 처리하는 작업임을 약한 강조로 표현 | 해당 자식의 실제 Workspace/탭/Pane으로 이동 |
| 타 Workspace의 자식 | 소속 Workspace에서는 로컬 행, 부모 쪽에서는 위치가 붙은 관계 링크 | 복제 실행이나 소속 이동 없이 같은 Agent로 이동 |

같은 Workspace 안에서는 부모 밑으로 자식을 접어 보여주는 기존 방식부터 정돈하는 것을 추천한다.
연결선은 일정한 들여쓰기와 하나의 짧은 세로선으로 제한하고, 펼침 기호·상태 아이콘·관계선의 자리를 겹치지 않게 한다.
깊은 손자 계층은 계속 들여쓰지 않고 관계 모달에서 탐색하는 안을 검토한다.
기본 접힘 정책은 아직 미정이며 기존 사용자 펼침 상태를 보존하는 것을 우선한다.
완료한 자식을 숨겨도 접근 경로는 남기고, 자식의 상태를 독립적인 사용자 Done 알림으로 승격하지 않는다.
Agents View에서는 부모 요약을 붙이되 기존 상태 그룹을 위임 트리로 대체하지 않는 것을 추천한다.
그룹을 가로지르는 자식은 별도 소유 행과 관계 링크를 구분하고, 보조 링크를 카운트·바로가기에서 중복 계산하지 않는다.
연결 불명·미계측·서버 단절은 자식 0으로 바꾸지 않으며, 알려진 자식과 불완전한 계측 정보를 함께 표현한다.
현재 계약상 populated Workspace의 행 전체 클릭은 펼침이며, 최신 시안의 Workspace 진입 의도와 다르다.
따라서 이름 클릭/chevron 분리 또는 명시적 열기 제어 중 어느 방식을 쓸지 결정한 뒤 해당 계약과 접근성을 함께 바꿔야 한다.
01-04 수용만으로 이 클릭 동작 변경까지 승인됐다고 보지 않는다.

### IDE 변경 백로그

아래는 실행하거나 외부 시스템에 등록한 Task가 아니라, 다음 작업을 나누기 위한 문서상 백로그다.
담당 영역은 현행 문서가 가리키는 시작점이며 이번 턴에 구현 코드를 재검사하거나 네이티브 검증하지 않았다.

| ID | 순서 | 변경 단위 | 완료 시 확인할 결과 | 시작점 / 선행 결정 |
| --- | --- | --- | --- | --- |
| IA-01 | 먼저 | 왼쪽 행 동작과 자식 표현 확정 | Project/Workspace/Agent 선택, 펼침, 관계 보기가 서로 혼동되지 않는 상태별 시안 | 본 절 추천안, AgentRow / SidebarPresentation / status-model; 사용자 선택 필요 |
| IA-02 | 핵심 | Project Overview 중앙 진입 | Projects / Agents는 유지, 프로젝트에서 01 진입, Workspace로 이동 후 배치와 포커스 복원 | ShellModel / runtime / CheckoutOverview; IA-01 |
| IA-03 | 핵심 | Workspace 작업 요약 | 핵심 작업, 응답 필요, primary 대비 상태, 관련 PR 표시; 독립 작업 여러 개를 한 제목으로 합치지 않음 | project_context / sidebar / CheckoutOverview; IA-02 |
| IA-04 | 핵심 | PR 상세 축소·정돈 | 전체 PR 이력 대신 관련 PR 상태·CI·상세 링크, 조회 실패/오래된 값/PR 없음 구분 | github / CheckoutCardPresentation / Overview; IA-03 |
| IA-05 | 기반 | 공통 Agent item | Projects, Agents, 검색, Recent Panels, Sessions에서 제목·상태·제공자 의미 일치; 내부 ID는 기본 보조문구에서 제거 | AgentRow / HideUI / AgentMRU; 현행 상태 계약 유지 |
| IA-06 | 핵심 | 왼쪽 부모·자식 렌더링 정돈 | 선과 아이콘 충돌 없음, 펼침 상태 유지, 타 Workspace 소속과 집계 정확, 키보드 탐색 일치 | sidebar / SidebarPresentation; IA-01, IA-05 |
| IA-07 | 핵심 | Pane 부모 요약과 관계 모달 | 잘린 fork ID 나열 대신 자식 진행 요약, 03 노드 선택은 읽기만, 명시적 열기로 04 진입 | Pane header / lineage projection; IA-05, IA-06 |
| IA-08 | 핵심 | 자식 탭 이동·복귀 | 자식은 전용 탭, 부모 분할에 자동 삽입하지 않음, 부모 복귀 시 저장된 배치 유지 | 기존 navigation / pane focus; IA-02, IA-07 |
| IA-09 | 후속 | 오른쪽 도구 범위 일관화 | Workspace 범위와 Explorer/Changes/기록 선택 보존, Pane 포커스 변경에 도구가 튀지 않음 | shell panel state; IA-02 |
| IA-10 | 독립 | Explorer Git decoration | 수정/추가/삭제/충돌과 폴더 집계, 선택 색상·긴 경로·단절에서도 판독 가능 | WorkspaceOutlineView / WorkspaceOutlinePresentation / files |
| IA-11 | 조사 후 | Sessions 목록과 기록 읽기 | 해당 Workspace의 세션과 대화 발췌, 기본 목록에서 tool 결과 숨김, 기록 열기가 실행을 재시작하지 않음 | provider별 기록 가능 범위·보존·프라이버시 결정 후 구현 |
| IA-12 | 보류 조건부 | Git 탭 기능 재배치 | 기능 목록을 먼저 작성하고 Changes/Overview/상세로 모두 이관한 경우에만 탭 제거 | 현행 Git 기능 inventory; IA-04, IA-09 |
| IA-13 | 후속 | Chat/Scratch 용어 정리 | New Agent와 프로젝트 독립 Workspace 시작 경로, 기존 데이터와 세션 보존 | Quick workspace 이름·경로·수명·정리 정책 결정 필요; /tmp 자동 이동 금지 |
| IA-14 | 각 단위 | 상태·네이티브 검증 | 빈 목록/하나/대량/긴 한글/중첩/타 Workspace/계측 불완전/단절/PR 실패/키보드·읽음 회귀 검증 | 현행 상태·성능·디자인 검사; 실제 앱 단일 인스턴스의 격리 환경 |

우선 묶음은 IA-01 결정 후 IA-02~04로 Project Overview를 개선하는 것이다.
공통 행과 부모·자식은 IA-05~08로 이어가며, Sessions와 Git 탭 제거는 이 핵심 흐름을 막지 않는 별도 작업으로 둔다.
IA-14는 마지막 일괄 검사가 아니라 각 변경 단위의 완료 조건이다.
Task Kanban, 실행 스케줄링, Task와 Workspace의 매핑 구현은 이번 백로그 범위 밖이다.

## Latest complete-screen proposal: 2026-09-14

Start with these four boards, all of which include the left navigation, central canvas, and contextual right pane.
This supersedes the earlier navigation explorations below, not the implemented product contract.

| Board | Entry | Center | Right pane |
| --- | --- | --- | --- |
| 01 Project dashboard (`xSu9F`) | Select a Project | Workspace-level work, attention, and PR summaries | Selected Workspace details and explicit open action |
| 02 Workspace multi-pane (`qz4SC`) | Open a Workspace | Its saved tabs and four-Pane layout | Workspace tools, showing Explorer with Git decorations |
| 03 Agent relationships (`xvRDO`) | Select relationship control in parent Pane | Graph overlay above the unchanged working layout | Existing Workspace tools remain visible |
| 04 Delegated agent tab (`fN6Go`) | Explicitly open a child from graph or navigation | Child's dedicated tab | Workspace tools, showing Sessions as a manually selected example |

The left side retains Projects / Agents, without adding a third global dashboard mode.
Project selection opens the Project dashboard; Workspace selection opens its existing working layout, not another dashboard.
Selecting a Pane focuses it without automatic zoom or destruction of sibling Panes.
A delegated child lives in a separate tab, following the current architecture contract; opening that tab preserves the parent's four-Pane layout for return.
Workspace membership and delegation are independent: the graph illustrates an ancestor in master and its descendants in review.
Graph selection only inspects; opening the selected child is a separate explicit action.
Closing the graph restores the previous focus and layout without navigation or implicit read acknowledgements.
The right pane follows the selected Workspace, not the type of the focused Pane, and retains its selected tool across tab switches.
Sessions in board 04 is a user-selected tool example, not an automatic consequence of opening a child.
Project dashboard rows summarize meaningful work instead of listing every Pane; multiple independent work roots must remain distinguishable.
Future Tasks are not assumed to map one-to-one to Workspaces.
Git history and maintenance remain secondary inspection actions; removing the Git tab still requires preserving its capabilities.
Quick workspace remains a naming proposal, with persistence and retention unresolved.
These boards use sample data and existing tokens; terminal and browser contents are illustrative, not native verification or a functional prototype.
All prior 2026-09-13 boards remain available as earlier explorations, but their global All work or Workspace dashboard concepts are not part of this latest proposal.

## Earlier direction and explorations

The operator talks to one parent agent and can understand the work it delegates without visiting every child pane.
Three different relationships must stay distinct: Project to Workspace membership, parent to delegated agent ownership, and a Workspace's Git comparison against the primary branch.
The interface should answer what is running, what needs the operator, where it is running, and how to return to it.
Commit history, raw pane identifiers, allocation details, and instrumentation explanations belong in inspection surfaces.

All example messages, names, counts, PR numbers, and timestamps in the proposal are labelled design samples, not a live status report or content to ship as constants.
Existing `HideTheme` variables are reused; no application token or product code is added.
The latest user direction prioritizes a natural node-and-edge parent/child visualization and consistent visual UI across navigation surfaces.
Start with `UX 00 Start here`, then follow UX 01 through UX 06 and the optional UX 07 example.
That earlier navigation proposal retains Projects / Agents and explores center/right transitions in complete application windows; use the 2026-09-14 section above for the latest scope and entry points.
The earlier `01 All work full screen`, `02 Project full screen` and `Workbench visual` boards are information-layout explorations, not the current navigation contract for this proposal.
Their canvas annotations point to the newer journey; retain their useful information hierarchy without copying their conflicting sidebar or right-panel behavior.
English navigation terms remain aligned with the product while Korean task names and message content exercise mixed-script layouts.
Korean labels in the proposal are copy candidates, not a decision to localize the entire application.

## Inspection and confidence

| Evidence | Finding | Limit |
| --- | --- | --- |
| User reference 1 | Workspace lineage connectors compete with disclosure, status marks, and provider icons; stopped children occupy substantial space | Pixel observation, not a proven geometry root cause |
| User references 2 and 3 | Overview emphasizes commits, tags, folded commits and a long PR list instead of active work | Observed supplied screenshots, not freshly navigated |
| User reference 4 | Pane child chips expose truncated fork identifiers; `Children unknown` appears beside known child entries | Known children and incomplete instrumentation can coexist; do not call their coexistence a data contradiction |
| User reference 5 and current `WorkspaceOutlineView.configure` | Desired Explorer Git decoration is absent from the current native row renderer; ordinary entry text uses primary color and the cell has icon/name slots | Source-verified; no dirty Explorer interaction was completed |
| User references 6 and 7 and current `HideUI.swift` | Search uses a generic kind icon and internal identifier subtitle; Recent Panels uses provider artwork but omits canonical status | Source and supplied-image agreement |
| Fresh native capture | One installed Hide app was identified and its window captured; Git has a large blank upper area and lower rows split branch/path text into narrow multiline columns | Installed screenshot, not proof that this checkout built the installed binary |
| Current `model.rs`, `sidebar.rs`, `HideUI.swift` search | Session identity exists; no persistent conversation-history reader or Sessions surface was found in the inspected source | This is a bounded search, not proof that no external provider can supply history |
| Current Pen inventory | Existing sheets cover panel/section headers, line/card rows, primitives and older as-built review rows; new workflows lacked dedicated component contracts | Seven proposed masters including the shared walkthrough navigation |

Native observation used the installed bundle, an exact window capture, and verified Screen Recording/Accessibility permissions.
Attempts to navigate the installed window were refused by the automation tool because its inventory could not resolve the target for interaction; no click or keystroke was dispatched to Hide.
There was no app rebuild, installation, restart, pane creation, pane selection, process termination, or file operation in the product.
The native screenshot and tool evidence remain local under the review run directory, outside source control.
No performance, hover, keyboard, screen-reader, or new-feature native acceptance claim is made.

## Canvas map

Every name below has the prefix `Review / 2026-09-13 Agent workflow /`.
Node IDs identify editable boards in the one shared document.

| Board suffix | Node | Purpose |
| --- | --- | --- |
| Audit | `Mn89s` | Review direction and the seven requested areas |
| Proposal / Agent item component | `eo5fV` | Shared identity/state row and canonical status variants |
| Proposal / Workspace hierarchy | `QjBOk` | Expanded/collapsed parent and delegated-work grouping at 320pt |
| Proposal / Overview candidates | `BcACs` | Action list A versus primary comparison map B at 344pt |
| Proposal / Delegation structural alternatives | `tjKNd` | Earlier tree/map comparison; use the later graph visual for the latest visual direction |
| Proposal / Explorer changes | `Wyafr` | File letters, folder descendant decoration, selection, narrow/wide layouts |
| Proposal / Sessions | `Jamt8` | Workspace history list and user/assistant-only reader |
| Proposal / Search and recent | `WStaR` | One Agent item across search and recent navigation |
| Proposal / New agent and Quick workspace | `t6SuFD` | New Agent entry and project-independent workspace |
| Proposal / GitHub and Git details | `dlMBT` | Focused PR inspection, retained Git diagnostics, lookup states |
| Proposal / Team summary states | `x8Are` | None, one, many, unavailable, disconnected and escalated delegation |
| Proposal / Empty and unavailable | `TXiJD` | Empty, no results, partial, denied, remote and pending examples |
| Proposal / Explorer entry component | `f10VU` | Modified, untracked, added, renamed, conflict and clean variants |
| Proposal / Workspace work item component | `j62vn` | Normal, no PR, stale GitHub and no agent variants |
| Proposal / Session item component | `O2Egh` | Live, ended, no messages and partial-record variants |
| Proposal / Workbench visual | `QLNQG` | Full 1440pt workbench with shared agent identity, delegated group, parent summary and action-focused Overview |
| Proposal / Agent node component | `R0tOWH` | Reusable graph node with shared identity, ownership, direct-child count and selected/question/seen states |
| Proposal / Delegation graph visual | `nE4ek` | Parent/child/grandchild node-and-edge graph, selected-node inspector, Graph/List and viewport controls |
| Proposal / 01 All work full screen | `WZvs5` | Complete 1600pt application window: global attention, project team lanes, compact Git context and selected-parent message/history inspector |
| Proposal / 02 Project full screen | `psy2M` | Complete 1600pt project view: primary comparison table, delegation graph, selected-workspace files/history and agent/PR inspector |
| Proposal / UX 00 Start here | `Ra5bK` | Start here: three-region responsibilities and user-action sequence |
| Proposal / UX 01 현재는 이렇게 작업한다 | `Z5Zjp` | Simplified current-structure reconstruction, not a pixel-exact live screenshot |
| Proposal / UX 02 변경 후에도 작업 화면은 그대로다 | `tU8kV` | Same parent and reference-document split; Workspace tools remain on the right |
| Proposal / UX 03 전체 현황을 잠깐 펼쳐 본다 | `IX8oz` | Global overview in the center; explicit return to retained master work |
| Proposal / UX 04 자식 노드는 살펴보기만 한다 | `tPlqM` | Inspect a review child without leaving the retained master execution context |
| Proposal / UX 05 에이전트를 열 때 작업 장소가 바뀐다 | `MQCiA` | Explicit Open agent changes the execution Workspace to review and its tools |
| Proposal / UX 06 부모로 돌아오면 원래 배치로 이어간다 | `gMGOa` | Return to the parent's preserved split and Explorer selection |
| Proposal / UX 07 오른쪽 도구 탭만 바꾸는 경우 | `vWgav` | Sessions tab changes only the right list; the center terminal remains visible |
| Proposal / UX Navigation continuity component | `L4UyI` | One shared Projects/Agents navigation master across the complete-window journey |

There are 29 new review boards; existing current-screen boards and the previous review are retained.

## Latest user journey: stable left, explicit center, contextual right

The user could not map the earlier full-screen mockups back to the existing application because the sidebar and right-panel roles had changed without a transition story.
The journey resolves that problem by retaining the existing Projects / Agents switch and showing both sidebars in every 1440pt application window.
The addition is an All work button above the switch and an explicit project-level overview button, not a third list mode or a replacement global navigation taxonomy.
Project disclosure remains disclosure; opening its overview has a separately labelled action.
Switching Projects / Agents changes the left navigation representation, not the central work or inspection mode.

| User action | Left navigation | Center | Right | Execution context |
| --- | --- | --- | --- | --- |
| Open All work | Preserve mode, expansion and last execution highlight | Global overview | Selected subject detail, or an explicit no-selection state | Retain master and its panel layout |
| Open creator overview | Preserve navigation | Creator overview and relationships | Selected subject detail | Still retain master |
| Select the review child node | Do not move execution highlight to the child | Keep project graph; highlight the inspected child | Review child detail, clearly labelled as inspection | No Workspace switch or read acknowledgement |
| Open selected agent | Highlight the actual opened child | Review's existing execution layout | Review's Workspace tools | This is the deliberate switch to review |
| Return to work from overview | Restore last execution highlight | Restore the retained layout | Restore that Workspace's last tool tab and visibility | No new agent or terminal is started |
| Return to parent from child work | Highlight the parent | Restore master's parent terminal and reference document | Restore master's last tool tab | Child execution continues |
| Select Explorer, Changes or Sessions tab | No change | No change | Change only the Workspace tool list | No navigation to another Workspace |
| Open a file, diff or session record | Retain the same Workspace | Open the requested content using the document/panel interaction contract | Keep the relevant tool list | Do not close or restart existing terminals |

The right panel has two clearly named roles, not two simultaneous tab families: Selected subject details during overview, and Workspace tools during execution work.
Overview and Git no longer remain as duplicate right-panel tabs in the recommended journey; their information is accessible through the central project overview and its scoped Git details.
While inspecting review from master, the right header says review, while the center explicitly identifies the retained master work.
Only Open agent changes the execution context; the inspected subject and active execution Workspace are deliberately distinct until that action.
The overview's return control identifies the retained work rather than acting as an ambiguous browser Back action.
The parent-return control follows known direct parent identity, not a title/cwd guess and not the previously visited unrelated agent.

Retaining a work surface means preserving its panel identities, split geometry, selected document, focus target and applicable viewport state, subject to the existing attachment/runtime contracts.
The design does not require keeping hidden terminals attached indefinitely or making a second runtime owner; implementation must respect the current ownership and attachment limits.
No overview transition closes a pane, restarts an agent, discards an unsaved document, or rearranges the saved workspace layout.
If the retained pane has ended or the Workspace was removed, show that specific condition and offer an existing destination; never silently create a replacement execution.
If the user changes data elsewhere while the overview is visible, restore current valid state rather than promising a frozen snapshot of outdated file content.

When no subject is selected, the detail panel shows a concise selection prompt or stays collapsed rather than retaining misleading details from another scope.
If the right panel was closed, do not force it open merely on entering overview; an explicit selection can reveal inspection according to the adopted interaction policy.
Remember Workspace tool tabs separately from overview inspection so returning to master restores Explorer even after inspecting another project's PR.
Session tab selection itself never opens a document or starts a process; the explicit record-opening action is a separate step.
Session-reader support and unavailable/loading/partial states remain prerequisite contracts, not a capability established by UX 07's sample messages.

UX 01 is a labelled reconstruction of the current information structure from the supplied references and prior inspection, not a fresh native acceptance capture.
UX 02 through UX 07 are the proposed future behavior, rendered as static designs and checked for clipping and mixed-script legibility.
Principles 2 and 5 are applied by walking through the user's existing work without replacing the familiar navigation model, and principle 7 by labelling all three regions and showing the inspection/open boundary visually.
This is design clarification, not approval to implement the navigation changes.

## Earlier whole-screen information layout

The following describes the earlier information-layout exploration only; the newer journey above takes precedence for navigation, right-panel modes, and execution-context changes.

The original workbench demonstrates an active terminal, but is not the overview entry point for this proposal.
The two newer screens use the central application area for an integrated read view instead of squeezing the overview into a narrow side panel.
They are complementary navigation levels, not two competing layouts for the same scope.

`01 All work full screen` answers, in order: which parent needs my response, what each project is doing, which children the parent delegated to, and what the selected parent's last visible message says.
The global sidebar's project numbers count unique agents, not workspaces; accessible labels must state that unit.
The shared fixture has three projects, five workspaces and seven agents: two operator-demand parents, four Working agents and one Seen agent.
The same parent appears in the attention list, its relationship lane and the inspector, but contributes only once to identity-based totals.
Creator has one Question parent, two Working children and one Idle child; hide has one Working parent; presentation has one Approval parent and one Working child.
The global Git summary is explicitly project-scoped while the selected-parent file count is Workspace-scoped; creator's five changed files comprise two in master and three in review.

`02 Project full screen` answers which branch is primary, what each Workspace is doing relative to it, how the project's agents are related, and what files/messages belong to the selected work.
Selecting the review Workspace or its agent highlights the same subject and changes the lower files/history panels and right inspector to review; it does not filter the project-wide graph into a misleading partial tree.
The parent remains visible in master, even though the selected child is in review.
Comparison rows are Git comparisons, and graph edges are agent delegation; the interface never uses one connector to imply both.
For a Workspace with several agents, show a truthful agent count and explicit selection rather than pretending the representative task is the only agent.

All work is a proposed global navigation destination; project selection opens the project-level central view, and explicit agent-opening actions return to the active execution surface.
New Agent retains the relevant creation scope: global navigation offers the location chooser, while the project action starts with that project's Workspace choice.
The full-screen labels and navigation are proposals, not evidence these destinations already exist.
The complete-window designs reuse the Agent item, Agent node and Explorer entry masters and the existing token set.

Known empty, loading, partial, failed, disconnected and history-unavailable treatments inherit the state sheets in this review.
Unknown counts must remain unknown, unavailable message readers must show an unsupported/unavailable state, and no missing Git result may be rendered as clean or passing.
At more projects or children than fit, scroll the main content, keep attention reachable, and use explicit collapsed groups with truthful counts rather than shrinking all task titles to fit the viewport.
At smaller window widths, collapse the optional right inspector and open it on selection before reducing graph-node text sizes; the exact native width threshold remains an implementation decision.
The new screenshots verify the populated 1600pt layouts only; smaller-window behavior and live navigation are not verified by these static boards.

## Visual system and interaction details

Use the existing dark surface hierarchy: background for the working area, sidebar for navigation, panel for inspection, and elevated for selection and controls.
Use existing typography at 17pt for surface titles, 13pt for task identity, 12pt for controls, and 10pt for compact context; Korean task names wrap instead of being replaced by internal IDs.
Reserve blue, amber, green and red for semantic status; graph edges remain neutral so a connector cannot imply success or failure.
Selected nodes use both a boundary and a raised surface, independently of their activity color.
Graph cards earn their boundary because each is a selectable agent with its own identity and connection ports; ordinary navigation rows do not receive redundant boxes.
Use real icon, input, keycap, button and disclosure nodes rather than whitespace-aligned text approximations for the refined Search, Recent, New Agent and Explorer surfaces.
The full workbench is a layout demonstration, not a proposal to replace terminal rendering with message bubbles or to invent live terminal content.

The graph reads top to bottom with rounded orthogonal edges and fixed top/bottom center ports.
Each edge means one known direct delegation, never Git ancestry or project membership.
The selected child's inspector preserves the root context and exposes an explicit Open agent action; single selection only inspects.
Graph is the proposed primary visual mode following the user's latest direction; List is the keyboard-friendly and dense-team alternative, not a separate status model.
Keep deterministic sibling ordering, stable node placement on status-only updates, and no force-directed animation that moves the target while it is being inspected.
At large counts, preserve the selected root-to-node path, collapse sibling groups with truthful counts, and offer Fit to view, pan/zoom and List without hiding escalated attention.
Unknown relationships belong in a separately labelled unlinked group rather than fabricated edges; duplicate identities, missing ancestors and cycles require explicit data-quality handling before layout.
These large-team and malformed-data behaviors are handoff requirements, not a claim that the five-node static board implements a graph engine.
No animated motion, drag interaction, keyboard navigation or native accessibility behavior is verified by these drawings.

Design principles 5, 7, 8 and 12 are applied through shared component instances, visual ownership edges, restrained containers, and rendered mixed-script layout checks.
Principle 11 still requires a decision on unresolved structural alternatives before implementation; the user requested candidate drawings, not product implementation in this turn.

## Requested changes and implementation boundaries

### 1. Workspace lineage

Use a parent Agent item followed by one indented delegated-work group.
The group has a quiet left boundary; it does not connect to status marks or provider logos.
Independent parents share one leading column, so a reader can distinguish siblings from children.
Collapsed groups keep a canonical state breakdown; expansion changes neither execution nor read state.
At greater depth, open the full relationship view instead of squeezing each generation into less space.
Cross-workspace children retain their physical Workspace label, and a child appearing in two navigation contexts must not be counted twice.

Do not infer parentage from indentation, title similarity, branch names, or matching working directories.
Do not rename child completion to Done if the existing ownership/read projection says Idle or Seen.
The first proposal uses Working/Seen group counts, while each visible child retains its precise demand/status label.
The default collapsed treatment for stopped descendants is a proposal to decide before implementation, not authority to hide escalated attention.

### 2. Overview and GitHub

A is recommended: primary branch anchor, Needs You work, Working work, then collapsed merged work.
B uses a primary-to-workspace comparison structure for users who prioritize the relationship overview.
The B connector means comparison against the selected primary, not a Git commit-parent edge or evidence of where a branch was created.
Use the actual primary branch; never hard-code `main` or equate remote default and primary without the current policy.

Each item carries branch, representative agent, PR/CI, changed-file state, divergence, and an explicit next action.
Selection opens inspection; only a labelled Open/Return action changes the active pane and its read state.
Keep merged work collapsed by default and keep cleanup eligibility separate from PR merge status.
GitHub defaults to PRs matched to this project's registered workspaces, with stale/authentication/lookup limits available in details.
Do not use the latest-200 per-branch query as a repository-wide total or present absent check data as passing.
Do not equate passing checks with mergeability, review approval, or authorization to merge.

### 3. Parent-centric delegation

Keep the existing 28pt title row and 24pt conditional delegation row as the starting geometry.
Replace the child-name-chip strip with one summary control that opens a relationship detail.
The current contract explicitly chose name chips; the new proposal must be adopted before that contract is replaced.
The earlier A detail explores a tree beside an inspector and B explores a relationship map.
The later graph visual follows the user's explicit node-and-edge direction and is now the recommended visual starting point, with List retained for keyboard traversal and dense teams.
Viewport gestures, large-team collapse, focus order and native graph accessibility still require implementation decisions and real interaction checks.

Expose the parent, selected child's title and status, actual Workspace, and an explicit Open agent action.
Closing the detail returns focus to its invocation control; inspecting a child does not mark it read.
Unknown instrumentation is separate from unknown activity and from a confirmed count of zero.
Known children can be shown while explaining that more children may not be observable.
Use the existing stall escalation decision; do not introduce a new timeout or promote all child questions to operator attention.
Retired parents must not remain clickable live agents; retain only relationship information the history contract can actually supply.

### 4. Explorer Git decoration

Reserve a trailing Git-decoration slot while leaving normal file activation intact.
Use M, A, U, R and a conflict mark with semantic color; expose their full meaning in help/accessibility text.
Folders indicate descendant changes rather than pretending to be Modified files.
Deleted files belong in Changes, not as fabricated existing Explorer entries.
Unsaved editor state remains distinct from Git modification.
Non-Git and unsupported remote workspaces have no invented Git state; a failed Git read is an explicit failure rather than Clean.

The proposal is not approval to run Git per visible row or on hover.
The implementation must use the established change data and performance ownership contracts.
The 24pt sample row needs comparison with the current 22pt native tree geometry before adoption; no new spacing token was added in this design-only change.

### 5. Git absorption and Sessions

Keep Explorer for file navigation and Changes for reviewing modified files.
Move Git's base selection, divergence, upstream/pushed/fetch status, history, disk details and cleanup access into Overview inspection before removing its tab.
Maintain each existing action's scope and error recovery; do not discard features just because the tab is unpopular.
Saved Git-section selection needs an explicit migration to the corresponding Overview detail.

Sessions is a new read view scoped to the current Workspace, with provider, title, recorded time and a short conversation preview.
Show only user and assistant messages; do not expose tool calls, tool results, hidden reasoning, or raw terminal escape sequences.
Live status comes from the linked live agent; Ended describes a historical session, not the Done attention group.
Reading a session must not start a process or claim the session is resumable.
Offer Open current agent only when its live identity can be resolved.
The proposed wide reader can be a central document surface while the narrow Sessions list stays in the side panel; its final presentation must be selected before implementation.

Session identity alone does not provide historical content, durable lineage, completeness, permission, retention, or resume semantics.
Those data contracts are prerequisite work; do not mock the reader into production or scrape transient viewport text to pretend to offer complete history.
Read-only history should remain useful when the original pane has closed, but that requires a supported provider reader and an agreed retention policy.

### 6. One Agent item

The proposed master has status mark, task title, provider and Workspace context, with an optional trailing timestamp or shortcut.
The same underlying agent must keep the same title, provider identity and canonical status in Sidebar, Search, Recent Panels and delegation detail.
Use the actual supported provider artwork when implementing the existing badge slot; the proposal uses provider text, not invented brand artwork.
Search may group or rank matches, and Recent Panels must preserve MRU order; shared visuals do not imply shared sorting.
Keep File, Diff, Browser and plain Terminal results in Recent Panels with their own type identity and no fabricated agent status.
Move pane IDs to copyable inspection details instead of the primary search subtitle.
At narrow widths, retain status and meaningful task text; full titles/paths remain available through the shared tooltip and accessibility help.

### 7. New Agent and project-independent work

Remove Chat as a domain term from the proposed navigation, entry action, tooltip and shortcut description.
Use New Agent for starting an execution and Workspace for its working location.
Replace the separate Scratch domain with a fixed project-independent Workspace, provisionally called Quick workspace.
Existing Scratch contents, running sessions and files are preserved; the request is not authorization to erase them.
The current Workspace stays the default when starting an agent there, while Quick workspace offers a direct project-independent path.

The user suggested a temporary location such as `/tmp`; this design does not allocate or migrate any directory.
A directory in OS-managed temporary storage and a workspace expected to retain history have different lifetimes.
Decide that lifetime and recovery behavior before selecting the path.
Do not promise permanent file retention in a temporary directory or silently clean up active work.

## Additional findings

| Priority | Improvement | Why / acceptance focus |
| --- | --- | --- |
| P0 | Separate operator attention from delegated demand | A parent should not send the operator through every child question; escalations remain reachable |
| P0 | Preserve inspection versus focus | Opening a PR, row inspector or relationship modal must not acknowledge another agent |
| P0 | Treat incomplete instrumentation honestly | Known child entries plus incomplete observability need one coherent explanation, not zero or a false contradiction |
| P1 | Replace Git's wide field strip with vertical inspection | Fresh screenshot shows severe branch/path wrapping and large unused space |
| P1 | Give long identifiers progressive disclosure | Task titles get the main width; raw IDs and absolute paths stay in details |
| P1 | Separate project scope from workspace scope | Overview is project-scoped; Explorer, Changes and Sessions need a visible Workspace qualifier |
| P1 | One creation entry vocabulary | The existing plus and New chat row duplicate an action; use one consistent New Agent action and a location chooser |
| P1 | Persisted navigation migration | Removing Git and Scratch must retain meaningful selection and existing content |
| P1 | Keep stale PR state visibly qualified | A retained green check must not masquerade as a fresh successful lookup |
| P1 | Accessible graph/list alternatives | Keyboard traversal, full task labels, focus return, and non-color status meaning need actual native checks |
| P2 | Hide housekeeping behind its action | Allocation and cleanup are useful secondary details, not the first explanation of project activity |
| P2 | Preserve honest time semantics | Relative times need a defined event source; activity, last message and last fetch are not interchangeable |
| P2 | Dense and empty states both matter | Many descendants, long Korean titles, missing titles, no results, non-Git folders and remote failures are real states |

## Proposed masters and remaining coverage

| Proposed master | ID | Consumers / states |
| --- | --- | --- |
| Agent item | `Z5BtlX` | Sidebar, Search, Recent, delegation and live Session identity; Working, Question, Approval, Error, Done, Idle, Unknown, Disconnected |
| Team summary | `W2m8VC` | Pane delegation row; zero, one, many, unknown instrumentation, disconnected, escalation |
| Explorer entry | `mSu8p` | Explorer and state sheet; modified, added, untracked, renamed, conflict, clean |
| Workspace work item | `wa3TP` | Overview and state sheet; normal, no PR, stale query, no live agent |
| Session item | `ey7uz` | Sessions and state sheet; live, ended, no visible messages, partial record |
| Agent node | `uCQGy` | Graph nodes and state sheet; shared Agent item, selected boundary, ownership and direct-child count |
| Walkthrough navigation | `LLvYW` | Complete-window journey; persistent Projects/Agents switch, project overview action, parent/child execution selection |

Masters are placed inside proposal sheets and referenced by the illustrated consumers and state examples.
Existing button, search, tab, tooltip, icon and badge owners remain the implementation starting point.
Explorer file examples now use the shared entry master, and graph nodes nest the common Agent item.
The complete migration must replace remaining equivalent manually drawn sample rows, tabs and summary controls with adopted masters; not every illustrative control on these concept boards is a component instance.
These are reviewable interaction/layout proposals, not a pixel-perfect native catalog or an implemented prototype.
Hover, pressed, focus and disabled treatments are specified using existing tokens; their live behavior is not tested by static Pen images.

## Decisions to make before implementation

| Decision | Recommendation | Alternative / unresolved part |
| --- | --- | --- |
| Overview structure | A: action-grouped work list | B: primary comparison map |
| Delegation detail | Node-and-edge Graph with adjacent inspector, following the latest user direction | List fallback and large-team keyboard/viewport behavior still need agreement |
| Project-independent workspace name | Quick workspace | General workspace; final product language remains undecided |
| Temporary workspace lifetime | Preserve existing contents and define retention explicitly | OS-temporary files versus persistent application-owned files |
| Sessions availability | Supported local provider history reader first, explicit unsupported states elsewhere | Provider coverage, remote access, retention and completeness are unresolved |
| Session reader placement | Central read-only document with Sessions list retained | Separate sheet; confirm against actual app width |
| Default descendant collapse | Keep active work visible and collapse stopped work | Existing disclosure preference and escalated attention must be preserved |

The node-and-edge visual direction is user-requested; detailed interactions and the remaining recommendations are design judgment, not blanket implementation approval.
Design principle 11 requires choosing a structural candidate before product implementation; it does not prevent drawing the candidates requested here.

## Handoff order

1. Review the Overview/delegation candidates and agree on vocabulary and temporary-workspace lifetime.
2. Adopt the shared Agent item and Workspace lineage, preserving status/read/ownership behavior across all consumers.
3. Adopt the parent summary and delegation inspector, including incomplete/retired/cross-workspace relationships.
4. Implement the selected Overview and focused PR/Git details; migrate every Git function before removing the tab.
5. Add Explorer decorations from supported change data, then verify native selection, long paths and conflict/remote states.
6. Establish the Sessions data contract, then implement the list and reader; do not make Git consolidation depend on unsupported history ingestion.
7. Replace New Chat/Scratch terminology and navigation with a content-preserving Workspace migration.

For each implementation change, update the owning `DESIGN.md` and active references together, promote accepted proposal masters into `Component /`, update the corresponding `Screen /` boards, and remove the adopted Review boards.
Do not promote all proposals automatically or treat this note as a human-approved PRD.
Read current architecture, status and performance contracts before changing their owning code; this note does not introduce new Herdr methods or a new runtime owner.

## Review checks

The native baseline was captured from one identified installed app.
The proposal boards were rendered with Pen and inspected for layout, mixed Korean/English text, and overflow; direct board references are in the canvas map.
Narrow panel examples include 320pt and 344pt, with a wider Explorer comparison; native 320/344/400pt acceptance remains implementation work.
Run `node scripts/gen-pen.mjs`, then `node scripts/check-design-contract.mjs` before committing the canvas.
The passing static checks prove token/band consistency, not design approval or real interaction behavior.
Keep native captures, rendered exports and verification transcripts local under `agents/runs/`, never in this committed design note or the canvas as embedded workstation screenshots.
