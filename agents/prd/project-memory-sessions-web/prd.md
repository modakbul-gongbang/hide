---
topic: "S8 Project Sessions 웹 표면 (Project Memory 웹 구현은 후속 TODO)"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "웹 셸에 읽기 전용 Project Sessions 표면과 명시적 Project 대상 지정을 추가하며, Memory 저장소·hook·사용자 데이터는 바꾸지 않는다."
source_intake: "agents/interview/workspace-ux-migration/qa-log.md"
created_at: "2026-09-24"
updated_at: "2026-09-24"
---

# PRD: S8 Project Sessions 웹 표면

## Goal

여러 Project에서 에이전트를 쓰는 운영자가 웹 셸의 Project 화면 안에서 연결된 worktree들의 지난 세션 기록을 찾아 읽게 한다.
지금 Sessions는 macOS 앱에만 있고 웹 셸은 core가 이미 보내는 Sessions 상태를 버린다.
S6이 Main과 Project Overview를 만들었으므로 기존 hide-session backend를 그대로 쓰는 Project 범위 Sessions 표면을 올린다.
Project Memory의 웹 구현은 사용자 결정(Q3)에 따라 이번 단계에서 하지 않고 아래 후속 TODO로 남긴다.

## Non-goals

- Project Memory 웹 구현 전체(목록·상세·편집·Forget/Undo·충돌 해결·활성화/비활성화·disclosure·분석 Retry·hook 갱신 안내·This turn/Show all, 세션 상세의 Memory 주입 기록 표시)는 후속 TODO이다(D-17). 웹에는 Memory 진입점, 비활성 버튼, "준비 중" 같은 placeholder를 두지 않는다. 그동안 Memory 관리는 기존 macOS 앱에서 한다.
- 후속 TODO가 승계할 계약: Project 범위 관리만 제공하고 Memory 옆에서 보기·Workspace Memory View·목적지 선택·임의 worktree 생성은 없다(Q1 답변); 기존 provenance/revision·disclosure·비활성화 시 데이터 보존·raw provider session 불삭제 의미(agents/prd/project-memory/prd.md, docs/agent-hooks.md); standalone hided는 hook을 쓰지 않는다. 착수 전 확인할 코드 사실: hided가 여는 Memory 저장소(`<state_dir>/project-memory.sqlite3`)와 hook이 읽는 `~/Library/Application Support/hide/project-memory.sqlite3`가 달라 웹 변경이 주입되지 않는다는 점, Memory 명령의 focused checkout 의존, disclosure 문구가 Swift에만 있다는 점. 재검토 조건은 사용자가 Memory 웹 구현을 다시 요청할 때이다.
- 이 단계는 Memory backend, 저장된 Memory 데이터, hook, 저장소 위치, macOS Memory 표면을 바꾸거나 지우지 않는다.
- 새 세션 분석·통계, 실행 중 agent pane의 대화 뷰어, 세션 자동 재실행은 하지 않는다. Sessions는 읽기 전용 archive이다.
- 원격 기기 Project의 Sessions는 이번 범위가 아니다. 그 기기의 provider 세션을 읽는 권한 계약이 생기면 다시 다룬다. 그때까지 원격 Project에는 기기 기준 unavailable 사유만 보인다.
- Swift 셸의 Sessions 패널은 바꾸지 않으며 별도로 승인되는 전환·삭제 결정 전까지 그대로 남는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | S8 범위는 기존 Project Sessions 기능을 Project 범위 웹 표면에서 완료하는 것이다. Project Memory 웹 구현은 D-17에 따라 후속 TODO이며 새 엔진은 만들지 않는다. | roadmap PRD S8 행; Q1 추천 1; 사용자 Q3 |
| D-02 | Sessions 이력 탐색은 Project 화면 안에서 끝나며 Workspace가 없는 Project에서도 동작한다. Workspace 목적지 선택과 옆에서 보기는 없다. | Q1 답변 "9번에 옆에서 보기는 빼버려"; Q1 추천 9 대체 |
| D-03 | Sessions 명령은 사용자가 연 Project의 identity를 명시적으로 대상으로 삼는다. 현재 agent focus나 선택된 checkout이 바뀌어도 대상 Project가 바뀌지 않고, 다른 Project로 간 뒤 도착한 늦은 결과는 현재 화면에 적용되지 않는다. macOS 앱의 기존 Sessions·Memory 동작은 그대로 유지된다. | roadmap PRD 기술 구조의 Project identity 확장; 가정: 기존 generation fence 재사용 |
| D-04 | Sessions는 연결된 worktree들의 기존 이력을 최신순, provider 필터, 검색으로 찾고 읽기 전용 archive 상세를 Project 안에서 연다. 결과 없음, 읽기 실패, 삭제·이동된 원본을 구별하고 Retry와 Copy source location을 유지한다. | roadmap PRD Sessions 행동; Q1 추천 1 |
| D-08 | 표면은 S6 Project Overview에서 들어가는 Project 범위 Sessions 화면이며 목록 옆 상세 구조를 쓴다. 이 배치는 기존 Swift Sessions 패턴과 proposal 보드의 Project 범위 구조를 따르는 작성자 가정이다. | design principle 1·5; 가정 |
| D-10 | 실패는 해당 목록·행·상세 위치에서 재시도로 복구하고, 조작할 수 없는 내부 실패는 진단 로그로 간다. | design principle 9·13 |
| D-11 | 화면은 기존 토큰·공용 master와 Swift Sessions 표면의 정보 구조를 따르고, 필요한 공용 상태 master를 같은 변경에서 library와 맞춘다. 밀도·한글 줄바꿈 세부는 작성자 판단과 검증이다. | Q1 추천 10; DESIGN.md library ownership |
| D-12 | 원칙 intake: mini의 원칙 저장소에 ROOT.md가 없어 S5.5 실행 기록의 654485f 사본에서 engineering·design 전문을 읽었다. design 1·4·9·10·12·13과 engineering 4·10은 목록 구조·파생 상태·상태 표식·실제 데이터·한글·진단으로 반영했다. design 6(Undo)은 읽기 전용 표면이라 해당 동작이 없다. | 가정: 원칙 사본 사용 |
| D-13 | 전달은 agents/config.json의 PR 모드로 commit, push, PR, 리뷰·CI까지이다. merge 여부는 이 PRD가 정하지 않고 PR 시점의 별도 사용자 전달 권한을 따른다. 격리 검증은 운영자의 실제 provider 세션·Memory 저장소·hook 파일·설치된 앱을 건드리지 않는다. | Q1 추천 11; agents/config.json delivery.mode=pr |
| D-17 | 사용자 결정: S8의 Project Memory 웹 구현은 명시적 후속 TODO로 미루고 Sessions/archive는 S8에 남긴다. 기존 Memory backend·데이터·hook·macOS 표면은 보존하며 가짜 Memory UI나 placeholder를 만들지 않는다. | 사용자 Q3 "어 근데 s8에 메모리는 그냥 아예 나중 구현으로 TODO로 적용해보면 어떨까 싶네?" |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Project Overview에서 그 Project의 Sessions로 들어가고, 다른 Project의 세션은 섞이지 않는다. Workspace가 하나도 없는 Project에서도 동작한다. | D-01, D-02, D-03 |
| B2 | Sessions는 연결된 모든 worktree의 지난 세션을 최신순으로 보이고 provider 필터와 검색으로 좁힌다. 각 행은 실제 첫 요청이나 제목, checkout, 시각, 가용성을 보이며 없는 값을 지어내지 않는다. | D-04 |
| B3 | 세션을 고르면 Project 안에서 읽기 전용 archive 상세가 목록 옆에 열린다. agent를 다시 실행하거나 세션을 Workspace로 보내는 동작은 없다. | D-02, D-04, D-08 |
| B4 | 세션이 없으면 "아직 없음" 빈 상태, 필터·검색 결과가 없으면 "일치 없음"과 필터 지우기, 목록 읽기 실패는 Retry를 보인다. | D-04, D-10 |
| B5 | 원본이 삭제·이동된 행은 unavailable과 사유, Retry, Copy source location을 보이고, 복사한 값은 실제 원본 위치이다. 상세를 열 수 없으면 상세 자리에서 같은 사유와 Retry를 보인다. | D-04, D-10 |
| B6 | Sessions 화면에 있는 동안 agent focus나 선택된 checkout이 바뀌어도 대상 Project와 보이는 목록이 바뀌지 않는다. 다른 Project로 옮긴 뒤 도착한 이전 요청의 결과는 새 화면에 적용되지 않는다. | D-03 |
| B7 | 원격 기기의 Project에서는 Sessions가 기기 기준 unavailable 사유와 함께 보이고 로컬 세션으로 대신 채우지 않는다. | D-03, D-10 |
| B8 | 웹 셸 어디에도 Project Memory 진입점, 비활성 Memory 버튼, placeholder가 없다. 이 변경 뒤에도 macOS 앱의 Memory와 Sessions 표면, 저장된 Memory 데이터, agent hook 주입은 이전과 같이 동작한다. | D-17, D-03 |
| B9 | 목록·필터·검색·상세·Retry·Copy source location이 키보드로 가능하고 focus가 보인다. 한글·영문 혼합 제목과 긴 경로가 지원 폭에서 읽히며 loading/empty/no-match/failed/unavailable 상태가 작은 표식으로 구별된다. | D-10, D-11, D-12 |

## Technical structure

기존 Rust core → hided/WS → React 경계와 hide-session backend를 유지한다.
Sessions 명령과 snapshot을 focused checkout 대신 명시적 Project identity로 대상 지정할 수 있게 확장하고 기존 generation fence로 늦은 결과를 막는다. macOS 앱이 쓰는 기존 명령 의미는 유지한다.
웹은 S6 Project 화면에 Sessions 표면을 추가하며 View 영역이나 Workspace 상태를 쓰지 않는다.
Memory 저장소 위치, hide-memory, hide-agent-hooks는 바꾸지 않는다.
Swift 표면과 wire는 별도 전환 결정 전까지 유지하고 docs/ARCHITECTURE.md의 Project sessions and Memory 절과 DESIGN.md를 함께 갱신하며, Memory 웹 구현이 후속 TODO라는 사실을 그 절에 적는다.

## Risks

- Sessions와 Memory가 core의 같은 worker와 focused checkout 경로를 공유한다. Project identity 확장이 macOS Memory 동작을 바꾸지 않도록 기존 Memory 테스트와 Swift test를 필수 gate로 둔다.
- 후속 TODO가 잊히면 macOS 앱 삭제(S10) 시 Memory 관리 표면이 사라진다. S10은 Memory 웹 구현 또는 그에 대한 별도 사용자 결정이 있기 전까지 시작하지 않는다.
- S8 PRD는 S6 merge 전에 작성되었다. 착수 시 merge된 S6 Project 화면과 대조하고 동작 계약을 유지한다.
- 격리 검증은 임시 provider 세션 fixture와 명시적 상태 디렉터리로만 하고 운영자의 실제 세션·Memory 저장소·hook 파일은 건드리지 않는다.
- 착수 전 사용자에게 필요한 작업은 없다.
