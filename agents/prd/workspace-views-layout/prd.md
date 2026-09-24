---
topic: "S7 Views 분할·문서 표시·복원"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "하나의 편집 버퍼를 여러 표시가 공유하고 재시작 복원 상태 파일을 이행하므로 미저장 편집 손실과 손상 상태 복구가 핵심 위험이다."
source_intake: "agents/interview/workspace-ux-migration/qa-log.md"
created_at: "2026-09-24"
updated_at: "2026-09-24"
---

# PRD: S7 Views 분할·문서 표시·복원

## Goal

에이전트가 만든 변경을 검토하는 운영자가 Workspace의 Views 안에서 파일과 Diff를 좌우·상하로 나란히 놓고, 탭을 끌어 옮기거나 나누며, 같은 파일을 두 곳에서 보면서도 편집 내용을 하나로 유지하게 한다.
창이 좁아지거나 앱을 다시 켜도 선호한 배치와 미저장 내용이 돌아와야 한다.
S6이 Workspace마다 View 영역 하나를 만들었고 S5.5가 기기·checkout·문서마다 버퍼 하나를 정했으므로, 그 위에 여러 View 영역과 표시 identity를 올린다.

## Non-goals

- Workspace 사이 View 이동, 일반 drag에 의한 문서 복제, Agent 탭을 View로 바꾸기, View drag로 Herdr terminal split을 바꾸기는 하지 않는다. 필요하면 별도 요청으로 다룬다.
- 기존 Project Sessions 이전은 S8에서 Project 범위로 수행하고, Project Memory 웹 구현은 사용자 결정(D-17)에 따라 후속 TODO이다. Memory 옆에서 보기, Workspace Memory View, 목적지 선택은 어느 단계에서도 만들지 않는다(Q1 답변). S7의 View 영역과 복원 상태에는 Memory 표시가 들어가지 않으며 Memory backend·데이터·hook을 바꾸지 않는다. 인터뷰의 UX-04(Project 범위 Memory 관리) 시나리오는 D-17에 따라 역사로만 남고, 이 단계에는 그 웹 진입점·UI·동작·증명 요구가 없다.
- 실제 Browser 내장과 Browser View는 후속 Electron 범위이며 가짜 Browser 탭을 만들지 않는다.
- 새 단축키 기본값은 할당하지 않는다. 분할·이동은 메뉴, 팔레트, drag로 제공하고 기존 탭 닫기·이동 단축키는 활성 View에 작동한다.
- Herdr tab/pane 배치 저장, 종료된 agent 재실행, 새 Git staging/discard/commit 흐름은 도입하지 않는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | S7 범위는 View 영역별 preview/고정과 공유 버퍼 표시, 반복 split·drag preview·resize·빈 영역 정리, 좁은 창 대응, 전체 재시작·손상 상태 복원이다. S6의 Workspace 모드·도구·View 영역 하나를 확장한다. 승인된 Agents/Together/Views 세 아이콘 선택(B안 기각, Views 선택은 비교 split을 만들지 않음), 독립 Explorer/Changes 토글, 직접 자식 칩 한 줄·바로 이동·별도 관계 메뉴·Together 전환과 focus, 종류별 탭 아이콘(색상만으로 구분하지 않음, Browser 아이콘은 실제 Browser 기능 때)은 S6 PRD(workspace-navigation-shell)가 소유하며 S7은 그 동작을 바꾸지 않고 여러 View 영역에서도 그대로 유지한다. | roadmap PRD S6·S7 행(workspace-ux-migration); Q1 추천 3·4·5·6·7·8; Q1 답변 탭 아이콘 |
| D-02 | 파일은 마지막으로 사용한 View 영역에 연다. 영역마다 preview 한 칸을 단일클릭으로 교체하고 더블클릭이나 첫 편집이 고정한다. 이미 열린 파일은 기존 표시로 이동하며 여러 표시가 있으면 마지막 활성 표시를 고른다. 편집은 preview를 즉시 고정하므로 dirty·저장 중·저장 실패 문서는 preview 칸에 있지 않고 교체되지 않는다. | Q1 추천 4 |
| D-03 | 명시적 "옆에 열기"만 같은 파일의 두 번째 표시를 만든다. 표시들은 S5.5의 기기·checkout·문서 버퍼 하나를 공유하고 스크롤·선택 위치는 표시마다 독립이다. 표시 하나를 닫아도 버퍼는 남고, 마지막 표시를 닫을 때만 기존 dirty/save/conflict 보호를 거친다. | Q1 추천 4; S5.5 PRD 버퍼 identity 결정 |
| D-04 | 탭바 drag는 삽입선으로 재정렬 또는 기존 영역 이동이고, 콘텐츠 가장자리 drag는 목적 영역 overlay와 짧은 방향 안내로 새 split이다. 한 번에 하나의 목적지만 강조하고 유효 drop에서만 한 번 반영한다. drag 중에는 실제 문서·terminal 크기나 저장 배치를 바꾸지 않는다. | Q1 추천 5; proposal drag-review 23-28 |
| D-05 | Escape, 유효하지 않은 곳이나 창 밖 drop, 공간 부족, drop 직전 사라지거나 자격을 잃은 목적지는 원래 배치를 보존한다. | Q1 추천 5; proposal drag-review 28 |
| D-06 | 좌우·상하 split을 반복하고 경계선으로 크기를 조절한다. 영역이 최소 크기를 지킬 수 없으면 split을 막고 이유를 보인다. 탭이 모두 빠진 split은 정리하고 마지막 Views 전체가 비면 모드를 바꾸지 않는 빈 상태로 남는다. 최소 크기·영역 수 상한·비율은 작성자 가정으로 정하고 계약 문서에 기록한다. | Q1 추천 5 |
| D-07 | 방향 메뉴는 "오른쪽으로 나누기", "아래로 나누기", "왼쪽으로 옮기기" 같은 공간 표현을 쓰고 실제로 가능한 목적지만 보인다. Move to Group 표현은 쓰지 않는다. View 탭 메뉴는 preview의 Keep Open, 방향 나누기·옮기기, 경로 복사·reveal, Close View를 제공하며 파일 삭제는 없다. | Q1 추천 10; proposal context-menu 검토 |
| D-08 | 좁은 창에서는 Explorer/Changes가 닫을 수 있는 임시 overlay로 열리고 닫으면 focus가 호출 위치로 돌아간다. Together가 들어가지 않으면 현재 작업 영역을 우선 보이고 다른 영역을 명시적으로 고를 수 있다. 창을 넓히면 선호 배치가 돌아오며 반응형 임시 상태는 저장된 배치를 덮어쓰지 않는다. 폭 임계값은 작성자 가정이다. | Q1 추천 6 |
| D-09 | 재시작하면 마지막 유효 Workspace의 View 영역 트리·비율·탭·preview/고정·활성 표시·모드·도구 상태를 복원한다. 첫 실행이나 Workspace 소멸은 Main이다. pane/agent 존재와 terminal 배치는 현재 Herdr를 따르며 종료된 agent를 실행하지 않는다. | Q1 추천 7 |
| D-10 | 복원 중 없어진 파일은 그 표시에 unavailable과 닫기·재시도를 보이고 나머지는 보존한다. 손상되거나 지원하지 않는 상태는 원본을 보존하고 진단을 남긴 뒤 Main에서 안전하게 다시 고르게 한다. S6 버전 상태는 S7 버전으로 이행하고 이행 실패는 같은 복구 경로를 쓴다. 기존 설정 파일은 바꾸지 않는다. | Q1 추천 7; S6 PRD Workspace UI 상태 결정 |
| D-11 | Hide core가 Workspace별 View 영역 트리, 표시 identity, 활성 표시를 소유하고 한 번의 사용자 동작은 한 번의 core 이벤트로 반영한다. 편집 버퍼는 S5.5 owner를 재사용하며 표시마다 복제하지 않는다. Herdr가 terminal split·zoom을 계속 소유한다. | proposal 상태 소유 표; ARCHITECTURE.md 단일 이벤트 규칙 |
| D-12 | View 영역 수, 분할 깊이, Workspace당 열린 표시 수에 상한을 두고, 상한에 닿으면 새 split·열기를 막고 이유를 보인다. drag preview는 하나로 제한한다. 구체적 값은 작성자 가정이며 성능 측정으로 확인한다. | roadmap PRD 자원 상한 요구; PERFORMANCE_TESTING.md |
| D-13 | 모든 split·이동·닫기·크기 조절은 키보드로 도달하는 메뉴와 팔레트 명령을 가지며 영역 사이 focus 이동이 보인다. 한글 IME 입력과 에코는 표시가 여러 개여도 기존 기준을 지킨다. | Q1 추천 5·10 위임 |
| D-14 | 화면은 drag-review 23-28과 기존 토큰·공용 master를 따르고, 바뀐 탭·삽입선·분할 overlay·빈 상태 master를 같은 변경에서 library와 맞춘다. 밀도와 크기 세부는 작성자 판단과 검증이다. | Q1 추천 10; DESIGN.md library ownership |
| D-15 | 원칙 intake: mini의 원칙 저장소에 ROOT.md가 없어 S5.5 실행 기록의 654485f 사본에서 engineering·design 전문을 읽었다. design 6·9·12·13과 engineering 4·10·11은 미저장 보호·상태 표식·한글 확인·진단·중복 실행 안전으로 반영했다. 번역하지 않은 규칙은 없다. | 가정: 원칙 사본 사용 |
| D-16 | 전달은 agents/config.json의 PR 모드로 commit, push, PR, 리뷰·CI까지이다. merge 여부는 이 PRD가 정하지 않고 PR 시점의 별도 사용자 전달 권한을 따른다. | Q1 추천 11; agents/config.json delivery.mode=pr |
| D-17 | 사용자 결정: S8의 Project Memory 웹 구현은 명시적 후속 TODO로 미루고 Sessions/archive는 S8에 남긴다. 기존 Memory backend·데이터·hook·macOS 표면은 보존하며 이 단계도 가짜 Memory UI나 placeholder를 만들지 않는다. | 사용자 Q3 "어 근데 s8에 메모리는 그냥 아예 나중 구현으로 TODO로 적용해보면 어떨까 싶네?" |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Explorer나 Changes에서 파일을 단일클릭하면 마지막으로 사용한 View 영역의 preview 탭(기울임 표시)이 그 파일로 바뀐다. 같은 영역의 다른 고정 탭은 그대로다. | D-02 |
| B2 | preview 탭을 더블클릭하거나 편집하거나 Keep Open을 고르면 고정 탭이 되어 다음 단일클릭에 교체되지 않는다. 첫 편집이 곧 고정이므로 dirty·저장 중·저장 실패 문서는 항상 고정 탭이며, 영역에는 preview가 최대 하나만 있고 다음 단일클릭은 그 preview 칸(없으면 새 preview)만 사용한다. | D-02 |
| B3 | 이미 열린 파일을 다시 열면 새 탭 대신 기존 표시로 이동하고, 여러 표시가 있으면 마지막으로 활성이었던 표시를 고른다. | D-02 |
| B4 | 파일의 "옆에 열기"는 인접 영역(없으면 오른쪽에 새 영역)에 두 번째 표시를 만든다. 한쪽의 편집은 다른 쪽에 즉시 보이고 스크롤과 커서 위치는 각자 유지된다. | D-03, D-11 |
| B5 | 같은 파일의 표시 하나를 닫으면 남은 표시의 내용과 dirty 상태가 유지된다. 마지막 표시를 닫을 때 dirty·저장 실패·충돌이면 기존 확인과 보호를 거치고, 저장 결과가 불명확하면 성공으로 표시하지 않는다. | D-03 |
| B6 | 탭을 같은 탭바에서 끌면 삽입선이 보이고 놓으면 순서만 바뀐다. 다른 영역의 탭바에 놓으면 새 split 없이 그 영역으로 옮겨진다. 복사본은 생기지 않는다. | D-04 |
| B7 | 탭을 문서 영역의 좌·우·상·하 가장자리로 끌면 반투명 목적 영역, 경계, 짧은 방향 안내 하나가 보인다. 놓으면 그 방향에 새 View 영역이 생기고 탭이 그리로 옮겨진다. drag 동안 실제 문서와 terminal 크기는 변하지 않는다. | D-04, D-12 |
| B8 | Escape, 창 밖이나 무효 위치에 놓기, 공간 부족, 놓는 순간 사라지거나 자격을 잃은 목적지는 금지 커서 또는 preview 제거와 함께 원래 탭 순서와 배치를 그대로 둔다. | D-05 |
| B9 | split을 반복할 수 있고 경계선을 끌거나 키보드로 크기를 조절한다. 최소 크기나 영역 수 상한 때문에 split이 불가하면 목적 overlay가 나타나지 않고 메뉴 명령은 비활성화되며 이유를 보인다. | D-06, D-12 |
| B10 | 마지막 탭이 빠진 split 영역은 사라지고 이웃 영역이 공간을 받는다. 마지막 Views 전체가 비면 모드를 바꾸지 않고 빈 상태와 Explorer 열기 동작을 보인다. | D-06 |
| B11 | View 탭 우클릭·overflow 메뉴는 preview일 때 Keep Open, 가능한 방향의 나누기·옮기기, 경로 복사·reveal, Close View만 보인다. 해당 방향에 목적 영역이 없거나 공간이 모자라면 그 명령은 보이지 않거나 비활성 이유가 있다. 파일 삭제는 이 메뉴에 없다. | D-07 |
| B12 | 좁은 창에서 Explorer/Changes를 열면 작업 영역 위의 임시 overlay로 열리고 Escape나 바깥 클릭으로 닫히며 focus가 여는 버튼으로 돌아간다. | D-08 |
| B13 | Together가 들어가지 않는 폭이면 현재 작업 중인 영역(마지막 focus)만 보이고 다른 영역으로 가는 명시적 전환이 보인다. 창을 다시 넓히면 저장된 모드, 영역 비율, 도구 배치가 돌아오고 좁은 상태가 그 설정을 덮어쓰지 않는다. | D-08 |
| B14 | 앱을 다시 켜면 마지막 유효 Workspace의 View 영역 트리, 비율, 탭 순서, preview/고정, 활성 표시, 모드, 도구 상태가 복원되고 dirty 문서의 초안이 대조되어 돌아온다. 첫 실행이나 Workspace 소멸이면 Main으로 시작한다. | D-09, D-03 |
| B15 | 복원 시 Herdr의 현재 tab/pane을 그대로 쓰며 종료된 agent를 실행하거나 옛 terminal split·zoom을 Herdr에 다시 쓰지 않는다. 다른 client가 그 사이 바꾼 terminal 배치가 이긴다. | D-09, D-11 |
| B16 | 복원한 표시의 파일이 사라졌거나 읽을 수 없으면 그 표시에만 unavailable과 닫기·재시도가 보이고 다른 표시와 영역은 유지된다. 원격 기기가 끊겼으면 기기 기준 사유가 보이고 재접속 후 재시도한다. | D-10 |
| B17 | 상태 파일이 손상되었거나 모르는 버전이면 원본을 보존하고 진단을 남긴 채 Main에서 시작하며, 운영자는 Workspace를 골라 새 배치로 이어간다. 기존 설정 파일과 Swift 앱 설정은 바뀌지 않는다. S6 형식 상태는 한 번 이행되고 이행 실패는 같은 경로로 복구된다. | D-10 |
| B18 | 분할, 이동, 닫기, 크기 조절은 한 번의 동작으로 반영되어 중간 배치가 보이거나 저장되지 않는다. 같은 동작이 두 번 도착해도 배치가 두 번 바뀌지 않는다. | D-11 |
| B19 | View 영역 수, 분할 깊이, 열린 표시 수 상한에 닿으면 새 split·열기가 막히고 이유가 보이며 기존 표시는 영향받지 않는다. 여러 표시와 drag 중에도 terminal 입력 유실·불필요한 재attach가 없고 입력·프레임 기준을 지킨다. | D-12 |
| B20 | 모든 분할·이동·닫기·크기 조절을 메뉴와 팔레트로 키보드만으로 할 수 있고, 영역 사이 focus 이동과 현재 활성 영역이 보인다. 두 표시에서 한글을 입력해도 조합과 에코가 깨지지 않는다. | D-13 |
| B21 | 탭, 삽입선, 분할 overlay, 비활성 목적지, 빈 상태, unavailable 표시가 drag-review 보드와 공용 master에 맞고, 한글·긴 경로 제목이 지원 폭에서 잘림과 전체 이름 tooltip으로 읽힌다. | D-14, D-15 |
| B22 | 여러 View 영역이 있어도 S6의 세 모드 선택과 독립 Explorer/Changes 토글이 같은 동작이다. 부모 pane header 아래 모든 직접 자식 칩은 한 줄에 남고 넘치면 가로 스크롤하며, 칩은 이미 있는 자식 tab/pane으로 바로 이동하고 관계 상세는 별도 메뉴에 남는다. Agent 탭은 실제 agent/provider 아이콘, 파일·Diff 탭은 종류별 아이콘과 제목을 가지며 색상만으로 종류를 구분하지 않고 Browser 아이콘은 실제 Browser 기능에만 붙는다. Views 모드 선택만으로 split이 생기지 않고, Agents-only에서 파일을 열면 Together로 바뀌어 마지막 사용 View 영역에 focus한다. | D-01, D-02 |
| B23 | 원격 기기의 Workspace에서도 같은 분할·표시·복원 규칙이 적용되며, 한 기기의 문서가 다른 기기의 같은 경로 문서와 버퍼나 표시를 공유하지 않는다. | D-03, D-11 |

## Technical structure

기존 Rust core → hided/WS → React 경계를 유지한다.
core가 Workspace(기기 포함 checkout)마다 View 영역 트리(방향, 비율, 영역별 탭 순서·preview·활성 표시)를 소유하고, 각 표시는 S5.5의 문서 버퍼 identity를 참조한다.
분할·이동·닫기·크기 조절은 각각 하나의 typed 이벤트이며 적용 여부를 결정한 뒤 한 번 publish한다.
S6이 만든 버전 있는 Workspace UI 상태 파일을 S7 버전으로 이행하고, 기존 `core-state.json`/Swift `state.json`은 바꾸지 않는다.
웹은 drag preview와 좁은 창 판단을 표시 전용 임시 상태로 두고 유효 drop이나 명시 명령에서만 core에 보낸다.
Herdr의 terminal split·zoom 권한은 바뀌지 않는다.
DESIGN.md, docs/ARCHITECTURE.md와 공용 library의 탭·분할 master를 제품과 함께 갱신한다.

## Risks

- 공유 버퍼와 여러 표시는 저장 경합과 미저장 손실 위험이 있다. S5.5 버퍼 owner 하나와 마지막 표시 닫기 보호를 수용 기준으로 둔다.
- S7 PRD는 S5.5·S6 merge 전에 작성되었다. 착수 시 merge된 소스와 대조하고 동작 계약을 유지한 채 실제 owner를 재사용한다.
- drag와 경계 조절은 terminal resize 폭주를 부를 수 있다. drag 중 크기 불변과 조절 중 PTY resize 빈도를 PERFORMANCE_TESTING.md 절차로 확인한다.
- 상태 파일 이행 실패는 배치 손실로 보일 수 있다. 원본 보존과 진단, Main 복구로 제한한다.
- 두 물리 기기 검증은 MacBook이 오프라인이면 미실행으로 기록한다. 시각 품질은 사후 취향 검토이며 착수 전 필요한 사용자 작업은 없다.
