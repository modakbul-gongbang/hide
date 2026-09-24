---
topic: "S6 Workspace 탐색과 기본 작업 셸"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "웹 셸의 화면 구조와 core가 소유한 Workspace 표시 상태·영속 파일을 새로 만들지만 사용자 데이터 삭제·권한·외부 부작용은 바꾸지 않는다."
source_intake: "agents/interview/workspace-ux-migration/qa-log.md"
created_at: "2026-09-24"
updated_at: "2026-09-24"
---

# PRD: S6 Workspace 탐색과 기본 작업 셸

## Goal

여러 Project에서 에이전트를 돌리는 운영자가 웹 셸에서 Main으로 전체 Project를 보고, 한 Project의 Overview를 거쳐 Workspace에 들어가 에이전트 탭과 파일·Diff View를 함께 보며 오갈 수 있게 한다.
지금 웹 셸은 파일을 열면 terminal 캔버스를 대체하고, Agents 목록은 그룹·위임 표시가 없으며, 자식 작업으로 가는 길이 없다.
S5.5가 기기 독립 기반을 정리했으므로, 그 위에 승인된 Workspace UX의 탐색·모드·도구·탭 identity를 올리는 것이 S7(View 분할·복원)과 S8(Project Sessions)의 전제이다.

## Non-goals

- View 영역 분할, 탭 drag 분할·이동, 파일 옆에 열기, 영역별 preview 규칙 확장, 좁은 창 overlay, 재시작 시 View 배치 복원은 S7이다. S6은 Workspace마다 View 영역 하나만 제공하므로 두 파일 비교는 S7 전까지 탭 전환으로 한다.
- 기존 Project Sessions 기능 이전은 D-01에 따라 S8에서 한다. Project Memory 웹 구현은 사용자 결정(D-17)에 따라 S8에서도 하지 않는 후속 TODO이며, 그 경계(Project 범위 관리, Memory 옆에서 보기·Workspace Memory View·목적지 선택·임의 worktree 생성 없음)는 그 TODO로 그대로 넘긴다. S6의 Main/Overview는 Memory/Sessions 진입점, 비활성 버튼, placeholder를 보이지 않으므로 그 전까지 웹에서는 둘 다 쓸 수 없고 Memory는 기존 macOS 앱에서 관리한다. S6은 Memory backend·데이터·hook을 바꾸지 않는다.
- Swift 셸의 My Work/All, Project Home, 오른쪽 Overview 섹션은 바꾸지 않는다(roadmap 비목표의 Swift 동시 UX 재설계 제외). S10 삭제 전까지 Swift 전환 경로로 남는다.
- 실제 Browser 내장, 새 통계·지표 수집, 새 Agents 분류 체계, 새 단축키 기본값 할당은 하지 않는다. 필요해지면 별도 요청으로 다룬다.
- Herdr tab/pane 배치 저장, 종료된 agent 재실행, Workspace 사이 View 이동은 도입하지 않는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | S6 범위는 Main/Project Overview/Workspace 탐색, All Agents와 직접 자식 이동, Agents/Together/Views 모드와 독립 Tools, 종류별 탭 identity이다. 기존 editor/diff를 기본 View 영역 하나에서 사용하고 새 상태는 처음부터 버전과 Workspace 범위를 갖춘다. Q1 추천 1이 승인한 기존 Sessions 기능 이전은 단계 배치로 S8에서 수행하고, Project Memory 웹 구현은 D-17에 따라 후속 TODO로 미루며, S6은 Sessions 표면이 들어갈 탐색 구조만 만든다. | roadmap PRD S6·S8 행(workspace-ux-migration); Q1 추천 1·9 답변; 사용자 Q3; 사용자 "6, 7, 8 이렇게 이어서 쭉쭉 작업시키게" |
| D-02 | Main은 등록된 전체 Project, Project Overview는 한 Project의 Workspace와 에이전트, Workspace는 checkout의 작업 공간이다. 둘 다 현재 core가 가진 실제 데이터만 쓰고 없는 지표를 만들지 않는다. | Q1 추천 1; proposal D01; design principle 10 |
| D-03 | Workspace 상단의 Agents/Together/Views 세 아이콘(A안)으로 영역 표시를 고른다. 같은 선택지를 이름이 있는 레이아웃 메뉴로도 제공한다. 모드는 공간만 바꾸고 탭·문서·pane을 닫거나 비교 배치를 만들지 않는다. 가장자리 접기 B안은 기각한다. | Q1 추천 3; proposal D04/D13 |
| D-04 | Agent 탭과 View 탭은 별개의 탭 그룹이다. Agent 탭은 Herdr 탭과 그 pane을, View 탭은 파일·Diff 문서를 담는다. View는 Workspace에 속해 Agent 탭을 바꿔도 유지된다. 현재 terminal 캔버스를 문서가 대체하는 동작은 이 분리로 대체한다. | Q1 추천 1·3; proposal D02/D03/D09 |
| D-05 | Explorer와 Changes는 Workspace 도구이며 각각 독립적으로 열고 닫는다. 세 모드 모두에서 동작하고 선택된 Agent 탭에 속하지 않는다. 사용자에게 보이는 이름은 DESIGN.md의 현행 이름을 따른다. | Q1 추천 3; proposal D05 |
| D-06 | Projects/Agents 두 탐색기를 유지한다. Agents는 전체 현재 에이전트를 Needs You/Done/Working/Seen 그룹으로 보이고 My Work/All 전환은 두지 않는다. 위임 행은 Working/Seen만 가지며 자손의 요청·완료는 조상을 unread로만 바꾼다. | Q1 추천 8; proposal D07; docs/status-model.md |
| D-07 | 부모 pane header 아래 한 줄에 직접 자식 칩을 모두 보이고 넘치면 가로 스크롤한다. 칩 클릭은 기존 자식 tab/pane으로 바로 이동하고, 관계 상세와 명시적 Open은 별도 메뉴에 둔다. 첫 자식 + `+N` 표시와 inspect 후 Open 기본 경로는 대체한다. | Q1 추천 8; proposal D12/D18 |
| D-08 | Views-only에서 에이전트를 명시적으로 열거나 Agents-only에서 파일을 명시적으로 열면 Together로 바꾸고 요청 대상에 focus한다. 상태 변화, hover, 메뉴 열기는 화면을 전환하지 않는다. | Q1 추천 8; proposal D18 |
| D-09 | Agent 탭은 실제 agent/provider 아이콘, 파일은 문서 종류 아이콘, Diff는 비교 아이콘을 제목과 함께 쓰고 tooltip과 접근성 이름에 전체 identity를 둔다. 모르는 provider는 중립 terminal/agent 표식이다. Browser 아이콘은 실제 Browser 기능이 생길 때 붙인다. | Q1 답변 "에이전트면 에이전트 아이콘 + 파일,브라우저나 그런거면 좀 다르게"; proposal D15 |
| D-10 | Workspace별 모드, 도구 표시, 활성 View 탭은 Hide core가 소유하는 새 UI 상태이다. 기존 설정 파일과 분리된 버전 있는 파일에 저장하여 이전 앱의 설정을 덮어쓰지 않는다. 손상되거나 모르는 버전의 파일은 원본을 보존하고 기본값으로 시작하며 진단을 남긴다. | Q1 추천 7; roadmap PRD 재시작 복원·설정 분리 결정; ARCHITECTURE.md core 소유 원칙 |
| D-11 | 앱은 마지막 유효 Workspace로 시작하고, 첫 실행이거나 그 Workspace가 없어졌으면 Main으로 시작한다. 전체 View 배치·분할 복원은 S7이다. | Q1 추천 7; roadmap S6/S7 행 |
| D-12 | S5.5의 기기 범위 catalog 위에서 로컬과 원격 기기의 Project/Workspace를 같은 화면과 명령으로 다룬다. 선택한 기기의 실제 가용성과 거절 사유를 표시하고 원격에서 거절된 명령을 로컬로 대신 실행하지 않는다. | 사용자 "난 local, remote 다 동일한 인터페이스에서 동작하는게 가장 중요한것같은데"(roadmap PRD S5.5 방향) |
| D-13 | 우클릭·overflow·키보드 메뉴는 클릭한 대상에 작동하고 메뉴 열기만으로 focus·read state를 바꾸지 않는다. View 닫기, 도구 숨기기, pane/tab 종료는 서로 다른 말과 기존 확인 경계를 쓴다. 메뉴의 세부 구성은 작성자 가정이다. | Q1 추천 10; proposal context-menu 검토 |
| D-14 | 화면은 선택된 Pen 구조와 기존 토큰·공용 master를 따르고, 바뀐 child row 등 공용 컴포넌트 상태는 같은 변경에서 library와 제품을 함께 맞춘다. 밀도·한글 줄바꿈·크기·최소 폭·hover/focus 세부는 작성자 판단과 검증에 맡긴다. | Q1 추천 10; DESIGN.md library ownership |
| D-15 | 원칙 intake: sasu principles list는 mini의 원칙 저장소에 ROOT.md가 없어 실패했고, S5.5 실행 기록의 654485f 사본에서 engineering과 design 문서 전체를 읽었다. design 4·7·9·10·12·13은 Behaviors로, engineering 3·7·8·10은 기존 owner 재사용과 진단 경로로 반영했다. 번역하지 않은 규칙은 없다. | 가정: 원칙 사본 사용, S5.5 run context/principles |
| D-16 | 전달은 agents/config.json의 PR 모드로 commit, branch push, PR 생성과 리뷰·CI까지이다. merge 여부는 이 PRD가 정하지 않고 PR 시점의 별도 사용자 전달 권한을 따른다. branch protection 우회, 미실행 검사의 PASS 표기, 설치 앱 교체는 하지 않는다. | Q1 추천 11; agents/config.json delivery.mode=pr |
| D-17 | 사용자 결정: S8의 Project Memory 웹 구현은 명시적 후속 TODO로 미루고 Sessions/archive는 S8에 남긴다. 기존 Memory backend·데이터·hook·macOS 표면은 보존하며 이 단계도 가짜 Memory UI나 placeholder를 만들지 않는다. | 사용자 Q3 "어 근데 s8에 메모리는 그냥 아예 나중 구현으로 TODO로 적용해보면 어떨까 싶네?" |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Main에서 등록된 모든 Project를 기기 identity와 함께 보고, 각 Project의 Workspace 수와 에이전트 상태 그룹별 수를 실제 snapshot 값으로 본다. Project를 고르면 그 Project의 Overview로 간다. | D-01, D-02, D-12 |
| B2 | Project Overview에서 그 Project의 Workspace 목록(checkout 이름, branch, pin/purpose 등 현재 있는 사실)과 그 Project에서 돌고 있는 에이전트를 보고, Workspace나 에이전트를 골라 해당 Workspace와 대상 pane으로 들어간다. 다른 Project의 항목은 섞이지 않는다. | D-02, D-06 |
| B3 | Project가 하나도 없으면 Main은 빈 상태와 기존 Project 추가 동작을 보인다. Workspace가 없는 Project의 Overview는 빈 상태와 기존 Workspace 생성 동작을 보인다. catalog를 읽는 중이거나 기기가 연결되지 않았으면 해당 Project 행에 로딩·unavailable 표식과 재시도를 보이고 가짜 수치를 채우지 않는다. | D-02, D-12 |
| B4 | Workspace 화면과 Main/Overview 사이를 오가는 경로가 항상 보이며(Main, 해당 Project), 이동은 에이전트나 pane을 새로 만들거나 종료하지 않는다. | D-01, D-02 |
| B5 | Workspace 상단의 세 아이콘으로 Agents/Together/Views를 고르고 현재 선택이 시각적으로 구별된다. 각 아이콘은 tooltip과 같은 접근성 이름을 가지며, 같은 선택지가 이름이 있는 레이아웃 메뉴와 팔레트 명령으로도 제공된다. | D-03, D-09 |
| B6 | Together에서는 Agent 영역(Agent 탭 줄과 그 pane들)과 View 영역(파일·Diff 탭 줄과 문서)이 나란히 보인다. 두 영역 사이 경계를 끌어 크기를 조절할 수 있고, 각 영역은 읽을 수 있는 최소 폭 아래로 줄어들지 않는다. | D-03, D-04, D-14 |
| B7 | Agents-only나 Views-only로 바꾸면 선택된 영역이 넓어질 뿐 탭·문서·pane은 닫히지 않고 두 파일 비교 배치도 생기지 않는다. 다시 Together로 돌아오면 숨겨졌던 영역이 이전 탭과 활성 대상으로 돌아오며 terminal 프로세스를 새로 만들지 않는다. | D-03, D-04 |
| B8 | Agent 탭을 바꾸어도 그 Workspace의 View 탭과 활성 문서는 그대로다. 다른 Workspace에 갔다 돌아오면 그 Workspace의 모드, 도구 표시, View 탭과 활성 문서가 돌아온다. | D-04, D-10 |
| B9 | View 영역에 열린 문서가 없으면 빈 상태와 Explorer를 여는 동작을 보이며 모드를 멋대로 바꾸지 않는다. Agent 영역에 Herdr 탭이 없으면 기존 새 탭 동작을 가진 빈 상태를 보인다. | D-03, D-04 |
| B10 | Explorer와 Changes를 각각 독립적으로 열고 닫는다. 둘 다 열면 도구 영역에 함께 보이고, 세 모드 어디서든 같은 상태를 유지한다. 도구 표시는 Workspace마다 기억되며 한 Workspace의 도구를 닫아도 다른 Workspace에는 영향이 없다. | D-05, D-10 |
| B11 | Explorer에서 파일을 열거나 Changes에서 변경 파일을 열면 그 Workspace의 View 영역에 문서가 열린다. Agents-only였다면 Together로 바뀌고 열린 문서에 focus한다. 기존 preview/고정, dirty·저장 중·저장 실패·충돌 보호는 그대로 적용된다. | D-04, D-08 |
| B12 | Views-only에서 사이드바나 Overview의 에이전트를 명시적으로 열면 Together로 바뀌고 그 pane에 focus한다. 에이전트 상태가 바뀌거나 항목에 hover하거나 메뉴를 여는 것만으로는 모드와 focus가 바뀌지 않는다. | D-08, D-13 |
| B13 | Agents 탐색기는 현재 모든 에이전트를 Needs You/Done/Working/Seen 그룹으로 보이고 빈 그룹은 생략한다. My Work/All 전환은 없다. 위임 행은 절제된 표시와 Working/Seen만 가지며, 자손 badge는 살아 있는 자손 수를 보인다. 탐색기 전환만으로 focus나 read state가 바뀌지 않는다. | D-06 |
| B14 | 자식이 있는 pane의 header 아래 한 줄에 모든 직접 자식 칩이 상태 표식, provider 표식, 제한된 길이의 제목으로 보인다. 넘치면 가로 스크롤하며 header 줄이 늘어나지 않는다. 자식이 없으면 빈 줄이나 0 표시가 없고, 칩의 전체 이름은 tooltip과 접근성 이름으로 확인한다. | D-07, D-09, D-14 |
| B15 | 칩을 클릭하면 다른 탭이나 Workspace에 있는 기존 자식 pane으로 바로 이동한다. 이동 중에는 같은 대상의 중복 요청이 막히고 진행 표식이 보인다. 대상이 사라졌거나 Herdr가 거절·시간 초과하면 그 요청에 실패 사유와 재시도·닫기가 보이며 부모 pane을 split하거나 새 pane을 만들지 않는다. | D-07, D-13 |
| B16 | 자식 pane의 header에는 부모로 돌아가는 동작이 있고 B15와 같은 진행·실패·재시도 규칙을 따른다. 관계 상세 메뉴는 부모·형제·자식을 보여주고 명시적 Open으로만 이동한다. | D-07, D-13 |
| B17 | Agent 탭은 provider 아이콘(모르는 provider는 중립 표식), 파일 탭은 문서 종류 아이콘, Diff 탭은 비교 아이콘과 제목으로 구별된다. 제목이 줄어도 tooltip과 접근성 이름으로 전체 이름과 종류를 확인하며, UI가 Herdr 탭 이름을 덮어쓰지 않는다. | D-09 |
| B18 | Agent 탭, View 탭, pane header, Workspace 도구 막대의 우클릭·overflow 메뉴는 클릭한 대상의 가능한 동작만 보인다. View 닫기는 파일 삭제나 pane 종료와 다른 말이며 dirty 문서 닫기는 기존 보호를 거친다. Escape는 선택을 바꾸지 않고 메뉴를 닫는다. | D-13 |
| B19 | 앱을 다시 열면 마지막으로 사용한 유효 Workspace와 그 모드·도구·View 탭이 돌아온다. 첫 실행이거나 그 Workspace가 사라졌으면 Main으로 시작한다. 복원은 종료된 agent를 실행하거나 옛 terminal split/zoom을 Herdr에 다시 쓰지 않는다. | D-10, D-11 |
| B20 | 새 Workspace UI 상태 파일이 손상되었거나 모르는 버전이면 원본 파일을 보존한 채 기본값으로 시작하고 진단 로그를 남긴다. 기존 설정 파일과 Swift 앱의 설정은 변경되지 않는다. 사라진 파일을 가리키는 View 탭은 unavailable과 닫기를 보이고 나머지 탭은 유지된다. | D-10, D-15 |
| B21 | 원격 기기의 Project/Workspace도 같은 Main, Overview, 모드, 도구, 탭 identity를 쓴다. 기기가 끊겼거나 기능이 거절되면 해당 대상에 기기 기준 사유가 보이고 로컬 경로로 대신 실행하지 않는다. | D-12 |
| B22 | 모드·도구·Workspace 전환과 영역 크기 조절은 terminal 입력을 잃거나 불필요한 재시작·재attach를 만들지 않고 기존 attach 한도와 통지 비용 계약을 지킨다. 조작할 수 없는 내부 실패는 화면 경고가 아니라 진단 로그로 간다. | D-01, D-15 |
| B23 | 모든 새 표면은 키보드로 도달하고 조작할 수 있으며 focus가 보인다. 한글·영문 혼합 이름과 긴 경로가 지원 폭에서 잘림 표시와 전체 이름 tooltip으로 읽힌다. hover/focus/selected/pending/empty/loading/failed/unavailable 상태가 작은 표식으로 구별된다. | D-14, D-15 |

## Technical structure

기존 Rust core → hided/WS → React 경계를 유지한다.
Herdr가 tab/pane 존재, terminal split/zoom, PTY, agent lifecycle을 계속 소유하고, core가 Workspace별 모드·도구 표시·활성 View 탭과 Agent/View 경계 비율을 소유한다.
이 상태는 checkout identity(기기 포함)로 키를 잡고, 기존 `core-state.json`/Swift `state.json`과 분리된 버전 있는 파일에 원자적으로 저장한다.
checkout strip은 Agent 탭(Herdr)과 View 탭(파일·Diff)을 구분해 내보내고, 웹은 기존 editor/diff/viewer 구현을 View 영역에서 재사용한다.
자식 칩 이동과 부모 복귀는 기존 `focus_pane` request id와 `pane_focus_request` 결과를 쓰고 새 navigation 권한을 만들지 않는다.
Main/Overview는 기존 catalog·sidebar·checkout snapshot을 재구성하며 새 수집기를 추가하지 않는다.
Swift 셸이 읽는 전역 패널 상태와 wire는 전환 경로로 유지하고, 웹이 더 쓰지 않는 중복 웹 코드(문서가 캔버스를 대체하는 분기 등)는 같은 변경에서 제거한다.
DESIGN.md, docs/ARCHITECTURE.md, design/agent-workflow-review.md와 공용 library의 child row·layout 선택 master를 제품과 함께 갱신한다.

## Risks

- S6 PRD는 S5.5 merge 전에 작성되었다. 착수 시 merge된 S5.5 소스(기기 범위 catalog, 문서 identity)와 대조하고, 이 PRD와 충돌하는 구조가 있으면 동작 계약을 유지한 채 S5.5 owner를 재사용한다.
- Agent/View 분리는 core strip과 editor snapshot 모양을 바꾼다. Swift가 쓰는 wire를 깨지 않도록 Swift test와 contract 검사를 필수 gate로 둔다.
- 영역 크기 변화는 terminal 크기 변화를 일으킨다. 크기 조절 중 PTY resize 폭주와 입력 유실을 PERFORMANCE_TESTING.md 절차로 확인한다.
- 원격 기기 흐름의 실기기 검증은 MacBook이 오프라인이면 격리된 mini endpoint로만 할 수 있으며, 두 물리 기기 검증은 미실행으로 기록한다.
- 시각 품질과 밀도는 사람이 나중에 판단할 수 있는 취향 검토이며 착수 전 필요한 권한이 아니다.
- 사용자에게 착수 전에 필요한 작업은 없다.
