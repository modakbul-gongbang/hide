---
topic: "웹 셸 S5.5 기반 정리와 S6~S10 Workspace UX 전환 계획"
status: "draft"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "새 Workspace UI 상태의 영속화와 기존 설정 복귀, 문서 중복 표시의 미저장 보호, Swift 제거 전환 게이트를 함께 변경한다."
source_intake: "current conversation"
created_at: "2026-09-24"
updated_at: "2026-09-24"
---

# PRD: S5.5 기반 정리와 Workspace UX 단계 계획

## Goal

사용자가 여러 Project의 작업을 Main에서 조망하고, 각 Workspace에서 실행 중인 에이전트와 검토할 파일을 잃지 않고 오갈 수 있게 웹 셸의 작업 구조를 정리한다.
이 문서는 기존 웹 전환 우산 계약의 단계 순서·미룸 목록·디자인 범위를 개정하는 후속 계약이며, S0~S5의 상세 계약과 검증 이력을 다시 쓰지 않는다.
현재 단계표의 권위는 아래 D-02이고, 구 우산 PRD의 S6 삭제 및 로컬 `agents/runs/web-shell-pivot/task-plan.md`의 해당 순서는 역사적 입력으로 대체한다.
S6~S10 UX 합의의 원본은 `agents/interview/workspace-ux-migration/qa-log.md`이고, S5.5는 2026-09-24 후속 대화에서 추가된 방향이다.
S5.5 관련 추가 문안은 이전 인터뷰의 승인·검증을 승계하지 않는 개정 초안이다.
이번 요청은 대화 내용을 보존하고 새 담당자에게 S5.5 상세 PRD 작성을 인계하는 것까지이며 제품 구현은 시작하지 않는다.

## Non-goals

- 실제 Browser 내장, pet, 메뉴바·전역 단축키·Dock, 사용량, 실행 중 터미널의 별도 대화 뷰어는 후속 Electron 범위이다. Browser를 사용할 수 있는 것처럼 가짜 탭이나 비활성 기능을 납품하지 않는다.
- 원격 파일 뷰어는 더 이상 Electron 미룸 항목이 아니다. S5.5에서 기존 로컬 파일 흐름의 원격 동등성을 먼저 다루되 저장·삭제·연결 실패의 구체 계약은 별도 상세 PRD에서 확정한다.
- 새 통계 수집·분석 시스템, Memory 엔진, 세션 분석 모델은 만들지 않는다. Main/Overview와 Memory/Sessions는 실제 기존 데이터·기능을 재구성한다.
- Swift 동시 UX 재설계, 새로운 Agents 분류 체계와 My Work 필터는 제외한다. Agents는 All 하나로 보이고 차후 사용자 아이디어를 위한 변경은 별도 요청으로 다룬다.
- Project Memory의 옆에서 보기, Workspace Memory 탭, 목적지 Workspace 선택은 제공하지 않는다. 파일의 명시적 옆에 열기는 이 제외와 관계없이 포함한다.
- Project Memory 웹 구현은 D-20에 따라 S6~S9 범위 밖의 후속 TODO이다. 그동안 Memory 관리는 기존 macOS 앱에서 하며 Memory backend·저장 데이터·hook·macOS 표면은 보존한다. 웹에 가짜 Memory UI나 placeholder를 두지 않는다. TODO는 D-10과 B14의 계약을 승계하고, 착수 전 hided와 hook이 서로 다른 Memory 저장소를 여는 문제, Memory 명령의 focused checkout 의존, Swift에만 있는 disclosure 문구를 먼저 해결한다. 사용자가 Memory 웹 구현을 다시 요청하면 재검토한다.
- Herdr tab/pane 복제 저장소, 자동 세션 부활, Workspace 사이 View drag, 일반 drag에 의한 문서 복제, 새로운 Git staging/discard/commit 흐름은 도입하지 않는다.
- 자동 merge·release, 설치 앱 교체, 실사용 pane 조작 및 Swift 삭제 승인은 이 문서 개정에 포함하지 않는다. 별도 실행 단계에서도 새 비용·보안 경계·파괴적 작업의 권한을 추정하지 않는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | S0~S5의 번호·범위·검증 이력을 유지하고 새 UX를 웹 셸에 추가한다. 기존 우산 계약의 기술 스택, 루프백 인증, WS, 성능 기준, 공존·attach 경계는 유지한다. 기존 S3가 이미 포함한 terminal 파일/이미지 첨부와 영상 뷰어를 다시 Electron으로 미루지 않는다. | user Q1 추천 1·2 승인; 기존 S3 D-02/D-09 |
| D-02 | 작성자 단계 분해안: S5.5 기기 독립 Workspace 기반과 로컬/원격 동등성 → S6 Workspace 탐색과 기본 작업 셸 → S7 Views 분할·문서 표시·복원 → S8 Project Memory/Sessions(D-20 이후 Sessions만) → S9 통합 수용·전환 검토 → S10 Swift 제거. 구 S6의 삭제 요구는 S10으로 이동하며 새 번호는 사용자가 직접 지정한 것이 아니다. 각 단계는 별도 슬라이스 PRD/런으로 수행한다. | Q1 "우선 S문서 개정하고 어떻게 재구성할지"; 가정: engineering 3의 사용 가능한 end-to-end 단위 |
| D-03 | Main은 전체 Project, Project Overview는 한 Project, Workspace는 checkout의 작업 공간이다. Project Memory/Sessions는 Project에, Explorer/Changes는 Workspace에, Agent/View 상태는 Workspace에 속한다. Main/Overview는 현재 데이터가 없는 지표를 새로 만들지 않는다. | user Q1 추천 1; proposal D01/D05/D06 |
| D-04 | Agent 탭 아래 여러 Herdr pane을 유지하며 Views는 별도 탭 영역이다. 상단 Agents/Together/Views 3아이콘 A안을 채택하고 Explorer/Changes 토글은 독립한다. B 가장자리 손잡이는 기각한다. 모드 선택은 공간만 바꾸며 비교 split을 만들지 않는다. | user Q1 추천 3; proposal D02/D04/D13 |
| D-05 | 마지막 사용 View 영역에 파일을 열고 영역별 preview 1개는 단일클릭으로 교체한다. 더블클릭/편집은 고정한다. 이미 열린 파일은 기존 탭으로 이동한다. 명시적 파일 옆에 열기만 복수 표시를 만들며 하나의 편집 버퍼와 각 표시의 독립 스크롤을 사용한다. | user Q1 추천 4 승인; 9번 예외는 Memory에만 적용 |
| D-06 | 탭 drag는 재정렬/이동, 콘텐츠 가장자리 drop은 좌우·상하 분할이다. 삽입선 또는 분할 영역 preview 후 유효 drop에서만 반영한다. 반복 split과 divider resize는 최소 크기를 지킬 때 허용한다. 빈 분할은 정리하되 마지막 Views는 빈 상태로 남긴다. | user Q1 추천 5; proposal D14 |
| D-07 | 좁은 창의 도구는 임시 overlay이고 두 작업영역이 못 들어가면 현재 작업 중인 영역을 우선한다. 넓히면 사용자의 선호 모드/배치를 복원한다. 정확한 임계치·hit-zone·비율·focus/단축키 세부는 기존 관례와 검증에 근거한 작성자 가정으로 정한다. | user Q1 추천 5·6·10 위임 |
| D-08 | 마지막 Workspace로 시작하고 첫 실행/대상 소멸 시 Main으로 간다. Workspace별 Views·분할·활성 View·모드·도구 상태를 재시작 후 복원하되 pane/agent 존재와 터미널 배치는 현재 Herdr에서 읽는다. 기존 dirty 버퍼·저장·충돌 보호는 유지한다. | user Q1 추천 7; proposal D09/D10 |
| D-09 | Projects/Agents 탐색기를 유지하고 Agents는 전체 표시만 제공한다. 직접 자식 칩을 모두 부모 pane header 아래 한 줄에 두며 overflow는 가로 스크롤이다. 칩은 기존 자식으로 바로 이동하고 관계 상세는 별도 메뉴에 둔다. 반대 영역 대상을 명시적으로 열면 Together와 해당 focus로 전환한다. | user Q1 추천 8; proposal D07/D08/D12 |
| D-10 | Memory의 목록·상세·편집·설정과 Sessions의 기존 Project 이력 탐색은 Project 안에서 끝낸다. Memory 옆에서 보기와 Workspace 목적지 선택은 제거한다. 현재 backend의 provenance/revision·설정·삭제 경계는 승계한다. | user Q1 "9번에 옆에서 보기는 빼버려"; 추천 1 승인 |
| D-11 | Agent 탭은 실제 agent/provider 아이콘, 파일은 문서 종류, Diff는 변경 비교, 향후 Browser는 Browser 아이콘으로 구분하고 제목·전체 identity tooltip·접근성 이름을 함께 유지한다. 미확인 provider를 특정 브랜드로 꾸미지 않고 일반 terminal/agent 표식을 쓴다. | user Q1 "각 탭의 경우 에이전트면 에이전트 아이콘 + 파일,브라우저나 그런거면 좀 다르게"; design 7·10 |
| D-12 | 우클릭은 클릭한 대상에 작동하며 메뉴 열기는 focus/read state를 바꾸지 않는다. 숨기기·문서 닫기·pane/tab 종료·파일 삭제는 구별하고 기존 보호를 유지한다. Move to Group 대신 방향별 나누기/옮기기를 사용한다. 구체적인 지원 메뉴 구성은 기존 기능과 권한에 맞춘 작성자 가정이다. | proposal context-menu/drag 검토; user Q1 추천 10 세부 위임 |
| D-13 | 매 단계에서 선택한 Pen 구조, 제품, 필요한 공용 master/state sheet와 토큰을 함께 정렬한다. 기존 팔레트·밀도 방향을 승계하고 새 브랜드/정보구조는 만들지 않는다. library는 공용 컴포넌트만, 화면은 proposal/scratch에 둔다. 숫자 권위는 tokens.json 하나이며 이전 우산의 토큰만 공유한다는 가정을 대체한다. | user Q1 추천 10; DESIGN.md library ownership |
| D-14 | 새 UI 저장 상태는 버전 구분하고 기존 설정 원본을 보존하여 Swift 복귀를 막지 않는다. 자동 설치/출시 없이 격리 환경에서 확인하고 실제 사용 전환은 사용자가 결정한다. S5.5를 포함한 S1~S9 검증 PASS와 별도 삭제 PRD 승인 전에는 S10에 착수하지 않는다. | user Q1 추천 1·7·11; 기존 우산 D-05 |
| D-15 | 문서 → 슬라이스 PRD → 구현 → 검증 → 커밋/PR의 승인된 범위를 보존하되 이번 턴은 문서와 로컬 커밋까지만 한다. 이후 승인 범위의 되돌릴 수 있는 세부는 문안마다 재승인을 요구하지 않되 가정으로 기록한다. 불가능한 검증·새 hard authority는 미완료로 보고한다. | user Q1 추천 11 승인 및 "우선 S문서 개정"; agents/config.json delivery.mode=pr |
| D-16 | engineering/design 원칙 654485f를 읽었다. engineering 3·7·8은 단계별 완결 흐름과 기존 state owner 재사용, 4·10·14·15는 복구 가능한 실패와 자원 경계, design 3·7·9·10·12·13은 직접 이동·아이콘·실제 데이터·상태·한글 검증으로 적용한다. 파일 삭제는 design 6의 일반 Undo 선호보다 프로젝트의 기존 확인 후 휴지통 이동 계약을 우선한다. | principles intake; AGENTS.md와 DESIGN.md의 현행 파괴적 동작 경계 |
| D-17 | S6 전에 로컬/원격을 동일한 인터페이스로 제공할 기반과 관련 리팩토링을 정리한다. 기기를 바꾸면 대상 호스트만 바뀌며 파일·프로젝트·작업 상태를 다른 기기의 동일 경로와 섞지 않는다. 기존 S5 완료를 전체 원격 기능 동등성의 완료로 해석하지 않는다. | user: "난 local, remote 다 동일한 인터페이스에서 동작하는게 가장 중요한것같은데"; "그거 먼저 진행하자 S6전에"; "리팩토링 할 거있으면 싹다 미리 하자" |
| D-18 | 작성자 구조 제안은 기기별 대상/catalog, 상태 identity, 파일/Git 경계, 공통 명령/비동기 수명, capability/설정 범위, 전환 경로 정리의 여섯 축이다. 기존 core/Herdr/hide-project 책임을 재사용하고 아래 A~D는 구현 순서의 제안이지 승인된 PR 개수나 새 런타임 도입 결정이 아니다. | 가정: engineering 3·5·7·8; 위 사용자 방향을 상세 PRD로 구체화할 담당자의 검토 대상 |
| D-19 | 현재 인계는 S5.5 PRD 작성만 허용한다. 기존 기능의 원격 확대에 필요한 삭제·hook/설정 쓰기·호스트 신뢰·전송 보조 프로세스의 권한과 안전성은 기존 계약을 먼저 대조하고 미해결이면 차단 사항으로 남긴다. 기기 간 Memory 자동 동기화나 새 원격 daemon 설치를 묵시적으로 승인하지 않는다. | user: "이것들 남기고 handoff시켜서"; "5.5 PRD 정리하게 해주라"; 가정: 이번 요청의 작업 경계 |
| D-20 | 사용자 결정(2026-09-24 후속): S8의 Project Memory 웹 구현은 명시적 후속 TODO로 미루고 Sessions/archive는 S8에 남긴다. 기능·데이터 제거가 아니라 구현 연기이며 기존 Memory backend·데이터·hook·macOS 표면을 보존한다. D-10의 Memory 부분과 B14는 그 TODO의 계약으로 남는다. 가정(작성자): macOS 앱이 유일한 Memory 관리 표면이므로 S10은 Memory 웹 구현 또는 그에 대한 별도 사용자 결정 전에는 시작하지 않는다. | 사용자: "어 근데 s8에 메모리는 그냥 아예 나중 구현으로 TODO로 적용해보면 어떨까 싶네? 그 observer에서하고잇는거" |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Main에서 등록된 Project 전체를 보고 Project Overview와 기존 Workspace로 들어간다. 한 Project의 Memory/Sessions를 열어도 다른 Project의 기록이 섞이지 않는다. 기존 데이터가 없으면 빈 상태, 읽기에 실패하면 해당 표면의 재시도 상태를 보이며 가짜 통계를 채우지 않는다. | D-01, D-03, D-10 |
| B2 | Workspace에서 여러 Agent 탭과 각 탭의 실제 pane을 사용하면서 별도 Views 파일을 함께 본다. Agent 탭을 바꾸어도 그 Workspace의 열린 Views는 그대로이다. 숨긴 영역을 다시 열면 살아 있는 대상과 배치가 돌아오고 프로세스를 새로 만들지 않는다. | D-04, D-08 |
| B3 | 상단 세 아이콘과 접근 가능한 이름으로 Agents/Together/Views를 선택한다. Views만 눌러도 기존 한 영역이 넓어질 뿐 두 파일 비교 배치가 생기지 않는다. Explorer/Changes는 세 모드 모두 독립적으로 열고 닫는다. | D-04, D-06 |
| B4 | 파일 단일클릭은 마지막 View 영역의 preview를 교체하고, 더블클릭/첫 편집은 고정한다. 이미 열린 파일은 기존 표시로 이동하며 이미 여러 표시가 있으면 마지막 활성 표시를 선택한다. dirty/저장 중/저장 실패 문서를 preview 교체로 잃지 않는다. | D-05, D-08 |
| B5 | 파일의 명시적 옆에 열기는 기존 버퍼를 공유하는 두 표시를 만든다. 한쪽 편집이 다른 쪽에도 보이고 스크롤은 독립이다. 표시 하나를 닫아도 남은 표시의 내용은 유지되며 마지막 닫기는 기존 dirty/save/conflict 보호를 거친다. | D-05, D-12 |
| B6 | 탭바 drag에는 삽입선, 콘텐츠 좌우/상하 가장자리에는 목적 영역 overlay와 짧은 방향 안내가 하나만 보인다. drag 중에는 실제 문서/terminal geometry나 저장 배치를 바꾸지 않고 유효 drop에서 한 번 이동 또는 분할한다. | D-06 |
| B7 | Escape, 외부 drop, 공간 부족, drop 직전 사라진 목적지는 원래 배치를 보존한다. 기존 다른 영역으로 drop하면 새 split 없이 이동한다. 반복 split은 최소 크기로 제한하고 divider를 조절할 수 있으며 마지막 탭이 나간 빈 split은 정리한다. 마지막 Views 전체는 모드를 바꾸지 않는 빈 상태로 남는다. | D-06, D-07 |
| B8 | 좁은 창에서는 Explorer/Changes가 dismiss 가능한 overlay로 열리고 focus가 원래 도구 호출 위치로 돌아온다. Together가 들어가지 않으면 현재 작업 영역을 우선 표시하고 다른 영역을 명시적으로 선택할 수 있다. 창을 넓히면 선호 배치가 돌아오며 반응형 임시 상태는 영구 설정을 덮어쓰지 않는다. | D-07 |
| B9 | 재시작하면 마지막 유효 Workspace의 View 탭·분할·활성 대상·모드·도구 상태가 복원된다. 최초 실행이나 Workspace 소멸은 Main으로 돌아온다. 종료된 agent를 실행하거나 오래된 terminal split/zoom을 Herdr에 다시 쓰지 않는다. | D-08, D-14 |
| B10 | 복원 중 없어진 파일은 해당 View에서 unavailable과 닫기/재시도를 제공하고 나머지 작업을 보존한다. 기존 IndexedDB draft와 core dirty 문서의 대조·충돌 보호를 유지하며 저장 실패를 성공으로 숨기지 않는다. 손상/지원하지 않는 새 UI 상태는 진단하고 원본을 보존한 채 Main에서 안전하게 다시 배치를 선택하게 한다. | D-08, D-14, D-16 |
| B11 | Agents는 전체 현재 에이전트를 보여주며 My Work/All 전환은 없다. 탐색기 자체 변경은 focus/read state를 바꾸지 않는다. delegated Working/Seen과 descendant-to-ancestor unread 의미는 유지하고 상태 변화만으로 사용자의 작업 화면을 전환하지 않는다. | D-09 |
| B12 | 부모 아래 직접 자식 칩을 한 줄에서 모두 찾을 수 있고 넘치면 가로로 이동한다. 클릭은 해당 기존 tab/pane으로 직접 이동하며 관계 상세는 별도 메뉴이다. 중복 pending 요청은 막고 사라진 대상/거절/실패는 그 요청의 재시도·dismiss로 처리하며 부모 pane을 새로 split하지 않는다. | D-09, D-12 |
| B13 | Views-only에서 에이전트를 열거나 Agents-only에서 파일을 열면 Together로 바뀌고 요청한 대상에 focus한다. 공간이 모자라면 B8의 임시 단일 영역 규칙을 적용한다. 단순 항목 hover/메뉴 열기는 이 이동을 일으키지 않는다. | D-07, D-09, D-12 |
| B14 | (후속 TODO, D-20: S6~S9 범위 밖) Project Memory에서 disclosure·enable/disable, 검색/개수, 목록·상세·source/revision·제공 세션 수, Edit/Save/Cancel, Forget/Undo, 충돌 해결, 분석/복구 및 hook 갱신, This turn/Show all을 기존 의미로 사용한다. Workspace로 보내기, 옆에서 보기, 목적지 선택은 없고 Workspace가 없는 Project에서도 관리한다. 비활성화는 데이터를 보존하고 확인 후 파생 데이터 삭제도 raw provider session을 지우지 않는다. 실패/충돌은 해당 항목이나 설정에서 복구한다. | D-03, D-10 |
| B15 | Project Sessions에서 연결된 worktree들의 기존 이력을 최신순, provider 필터와 검색으로 찾고 읽기 전용 archive 상세를 Project 안에서 연다. 실제 요청/제목·checkout·시각·가용성을 표시한다. 결과 없음과 읽기 실패, 삭제/이동된 원본은 구별하고 Retry/Copy source location을 유지하며 자동 agent 재실행으로 대체하지 않는다. 실행 중 terminal 대화 뷰어 추가로 확대하지 않는다. | D-01, D-10 |
| B16 | Agent/provider, 파일 종류, Diff는 탭의 아이콘과 제목으로 구별된다. 좁아져 제목이 줄어도 전체 이름과 종류는 tooltip/접근성 이름으로 확인한다. 여러 pane의 탭은 기존 focused-pane identity 규칙을 사용하고 Herdr 탭 이름을 UI가 덮어쓰지 않는다. Browser 아이콘은 실제 후속 Browser 기능에 붙이며 지금 가짜 Browser 탭을 만들지 않는다. | D-04, D-11 |
| B17 | 우클릭/overflow/키보드 메뉴는 클릭한 대상의 가능한 동작만 제공한다. 닫기와 숨기기는 같은 말로 표시하지 않고, 방향 메뉴는 적합한 목적지만 보인다. 문서 닫기는 파일 삭제가 아니며 pane/tab/worktree/Memory의 파괴적 동작은 기존 확인과 read-only 경계를 유지한다. | D-12 |
| B18 | 매 단계의 화면은 선택된 Pen 구조 및 공용 컴포넌트와 맞으며 hover/focus/selected/pending/empty/loading/failed/stale/read-only 상태를 실제 의미에 맞는 작은 표식으로 구별한다. 조작할 수 없는 내부 오류는 로그에 남기고 화면 경고로 만들지 않는다. 한글·혼합 제목·긴 경로를 실제 지원 폭에서 읽을 수 있다. | D-11, D-13, D-16 |
| B19 | 모드/도구/Workspace 전환은 terminal 입력을 삼키거나 불필요한 재시작/재attach를 만들지 않는다. 기존 bounded attach·통지·입력 비용 계약과 IME/에코/프레임 성능 기준을 유지한다. drag preview는 한 개로 한정하고 View 분할 깊이·개수, 열린 문서와 저장 상태의 자원 상한 및 초과 시 복구 행동을 각 슬라이스가 명시한다. | D-01, D-06, D-08, D-16 |
| B20 | 새 UI 저장 상태를 만들어도 기존 설정을 덮어쓰지 않으며 사용자가 이전 앱을 선택하면 기존 설정과 실제 Herdr 대상을 사용할 수 있다. S9에서는 전체 작업 흐름·복원·기존 설정 복귀를 격리 환경에서 확인하고 실사용 전환 결정을 남긴다. S10은 S5.5를 포함한 S1~S9 PASS, Project Memory 웹 구현 완료 또는 그에 대한 별도 사용자 결정, 별도 삭제 승인 없이는 시작하지 않는다. | D-02, D-14, D-20 |
| B21 | S5.5 이후 기기를 바꾸어 같은 Explorer/파일/Changes/Workspace 작업 진입점을 사용한다. 기능의 실제 가용 여부와 제한은 선택한 호스트 기준으로 표시하며 원격에서 거절된 명령을 로컬에서 대신 실행하지 않는다. 지원할 세부 동작과 미지원 사유는 S5.5 상세 PRD에서 현재 계약과 대조한다. | D-17, D-18, D-19 |
| B22 | 다른 기기에 같은 경로의 파일이 있어도 편집 draft·문서·작업 대상을 섞지 않는다. 기기 전환이나 연결 끊김 뒤 도착한 응답이 현재 대상을 덮어쓰지 않고, 저장 결과가 불명확하면 성공으로 표시하거나 중복 실행하지 않으며 복구할 내용을 보존한다. | D-17, D-18 |

## Technical structure

기존 Rust core → hided/WS → React 경계와 토큰 생성 경로를 유지한다.
Herdr가 실제 tab/pane 존재·terminal split/zoom·PTY·agent lifecycle을 소유하고, Hide core가 Workspace별 View 표시 트리·선택·모드·도구 상태를 소유한다.
동일 문서의 편집 identity와 표시 identity를 구분하여 buffer를 복제하지 않으며 버전이 있는 UI 상태는 기존 설정과 분리한다.
Project Memory/Sessions backend를 재사용하고 Browser plugin 자원 소유권이나 저장 엔진을 새로 만들지 않는다.
현행 Sessions 명령의 focused checkout 의존은 S8에서 명시적 Project identity로 확장하여 대상이 현재 agent focus에 따라 바뀌지 않게 하고, Memory 명령의 같은 확장은 D-20의 후속 TODO에서 한다.
기존 archive 편집/읽기 모델은 재사용하되 새 Project 상세의 표시 컨텍스트를 Workspace View와 분리하며 background 분석·provenance·hook retrieval은 유지한다.

| 단계 | 변경 범위와 종료 결과 | 선행 조건 |
| --- | --- | --- |
| S0 | 기존 spike/IME·echo·메모리·frame 게이트 및 기록 유지 | 기존 계약 |
| S1 | 기존 hided/WS/CLI와 기본 terminal·Agents 유지 | S0 PASS |
| S2 | 기존 탭·pane 분할·줌·Project/checkout·단축키 유지 | S1 PASS |
| S3 | 기존 Explorer/에디터/뷰어·영상·terminal 첨부 유지 | S2 PASS |
| S4 | 기존 Changes/diff 검토 | S3 PASS |
| S5 | 기존 Settings/진단과 S2가 이관한 단축키 설정, worktree 생성/제거·pin/purpose 편집, 원격 기기 등록/연결 | S2 PASS; S3/S4와 병렬 가능 |
| S5.5 | 기기별 Project/checkout 대상 정규화, Explorer/파일·Changes·작업 명령 동등성, 설정/권한 범위와 실패 복구, 관련 중복 제거. 상세 범위·미해결 권한을 별도 PRD로 정리한 뒤 승인된 범위만 구현한다. | S3~S5의 현행 계약과 코드 대조 + S5.5 상세 PRD |
| S6 | Main/Project/Workspace 탐색, All Agents·직접 자식 이동, 모드/독립 Tools, 종류별 탭 identity. 기존 editor/diff를 기본 View 영역에서 사용하고 새 상태는 처음부터 버전·Workspace 범위를 갖춘다. | S3~S5 및 S5.5 PASS |
| S7 | View 영역별 preview/고정·공유 buffer, 반복 split·drag preview·resize·빈 영역 정리, 좁은 창 대응과 전체 재시작/충돌 복원 | S6 PASS |
| S8 | Project Sessions의 기존 기능을 Project 범위 웹 표면에서 완료. Project Memory 웹 구현은 D-20에 따라 후속 TODO; Memory 옆에서 보기 없음 | S6 PASS; S7과 독립 검증 가능 |
| S9 | S8 다음에 반드시 이어서 수행(사용자 "그럼 S9도 근데 돌리는거맞지?"). S6~S8 전체 여정(Project Memory 웹 표면 제외, macOS Memory 표면·데이터·hook 불변 확인), Pen/제품/library 일치, 키보드·한글·성능·복원·rollback 통합 검증 및 사용자 전환 검토 | S7·S8 PASS |
| S10 | 구 S6 Swift 제거: macos/vendor/build·sign/Swift CI·생성기 정리, 공유 자산·Herdr pin의 후속 소유자 확정, 미룸 항목의 Electron 인계 | S5.5를 포함한 S1~S9 PASS + Project Memory 웹 구현 완료 또는 그에 대한 별도 사용자 결정(D-20) + 별도 삭제 PRD 사용자 승인 |

S5.5 구조 제안은 다음과 같으며 공개 API 이름이나 새 저장소를 지금 확정하지 않는다.

| 축 | 유지할 경계와 정리 방향 |
| --- | --- |
| 대상/catalog | device → Project → checkout → Herdr tab/pane의 기존 identity를 연결한다. remote raw workspace 목록을 로컬 Project 목록과 같은 모델로 투영하되 Herdr topology를 복제 소유하지 않는다. |
| 문서/상태 | device와 checkout으로 문서·draft·비동기 요청을 한정하고 문서 buffer identity와 View 표시 identity를 분리한다. 기존 저장 상태는 버전 있는 이행으로 보존한다. |
| 파일/Git | 동일 사용 흐름을 제공하되 파일 I/O와 Git 비교 책임을 구분한다. 기존 로컬 경로·symlink·대용량·저장 보호를 유지하고 원격 전송의 안전성을 검증 없이 가정하지 않는다. |
| 명령/수명 | 버튼·단축키·메뉴가 같은 대상 지정 명령으로 수렴한다. 기존 core 비동기 작업 모델을 재사용하고 기기 전환·취소·deadline·늦은 응답·결과 불명의 외부 효과를 한곳에서 다룬다. |
| 가용성/설정 | 실제 capability와 unavailable 이유를 제공하고 core에서도 실행 경계를 검사한다. UI 선호, 호스트 실행 설정, Project 설정의 소유자를 구별한다. |
| 전환 정리 | 이관한 기능의 중복 local/remote 분기와 오래된 소비 경로를 같은 변경에서 제거한다. S10 전까지 필요한 Swift·wire·설정 호환 경로는 유지한다. |

작성자 권장 구현 순서는 A 대상 모델과 원격 Explorer 읽기 한 흐름 → B 파일 열기·편집·뷰어와 draft 보존 → C Git/Workspace/pane 명령 동등성 → D 설정 범위·재연결 교차 흐름·남은 중복 정리이다.
각 흐름에서 안전성·실패 복구를 함께 완료하며 D까지 미루지 않는다.
A~D는 리뷰 가능한 end-to-end 순서 제안이며 실제 PR 분할은 상세 PRD와 의존관계 확인 후 정한다.
S5.5는 S6 화면 재설계를 당겨 구현하는 단계가 아니라 그 화면들이 공통으로 사용할 경계를 정리하는 단계이다.

S6~S8은 각각 구현과 함께 Pen·공용 컴포넌트·동작 상태·회귀 검증을 완료한다.
S9는 디자인/테스트를 처음 시작하는 단계가 아니라 교차 흐름 수용 단계이다.
이 표는 의존관계이지 현재 완료 현황이 아니며, 착수 전 실제 슬라이스 receipt와 작업 상태를 확인한다.

## Risks

- 원격 저장의 read-compare-write는 원자적 충돌 방지 보장이 아니다. 전송 프로토콜과 원격 경로 검증 능력을 확인한 뒤 실제 보장 수준과 필요한 추가 권한을 상세 PRD에 명시한다.
- 원격 삭제·worktree 제거·hook/설정 쓰기 등의 안전한 의미가 현행 계약으로 결정되지 않으면 사용자 결정이 필요한 차단 사항이다. 문서 작성 승인을 운영 기기 변경 승인으로 확대하지 않는다.
- 과거 문서와 번호 혼동: 이 개정이 구 S6와 과거 task-plan의 순서/미룸 목록을 대체하며 S0~S5 계약과 과거 evidence는 유지한다. 완료를 번호 존재만으로 추정하지 않는다.
- 반복 split과 문서 복수 표시는 저장 손실·focus·성능 위험이 있다. core owner 하나, shared buffer, 유효 drop의 원자적 변경과 기존 draft 보호를 수용 기준으로 둔다.
- 기존 Memory/Sessions의 기능 이전과 새 엔진 개발을 혼동하지 않는다. source·revision·삭제·설정 의미는 현행 owning guide를 기준으로 명시적으로 대조한다.
- 기존 S3의 Session/Memory 탭 제외는 S3 자체에서는 유지하지만 이 계획의 S8로 앞당긴다. Electron까지 미루는 대화 뷰어는 실행 중 agent pane의 terminal/ledger 전환 표면이며 Project archive와 구별한다.
- 실제 Browser를 미룬 동안 mock의 Browser는 검토용 미래 표면일 뿐이다. UI 종류 분리만으로 내장이 완료되었다고 주장하지 않는다.
- 격리 검증은 운영자 앱·pane·서버를 종료/이동하지 않는다. 새 권한이 필요한 조치는 중단하고 미실행 검증과 사람만 판단할 전환을 분리 보고한다.
- 작성된 문안 자체의 human_approval은 pending으로 유지한다. 대화에서 허용한 후속 구현 범위는 D-15로 남기며 이번 문서 작업 종료를 야간 구현 완료로 표현하지 않는다.
