---
topic: "웹 Overview의 Tasks 뷰(Board · Dependencies)와 Agents 뷰: 태스크 카드, 이슈와 PR의 구분, 기다리는 것 띠"
status: "draft"
human_approval: "design-approved"  # user 2026-09-26 verbatim on the final board: 음 우선 괜찮은 것 같아. 결정 기록해두고 이거 최종으로 남기고
review_profile: "standard"
review_rationale: "사용자가 매일 보는 웹 Overview의 두 탭을 다시 그리고, 코어가 GitHub 이슈의 선행 관계를 새로 읽지만, 자격 증명·결제·파괴적 동작은 없다. 스냅샷에 필드가 더해지므로 Swift 셸의 디코딩을 깨지 않는 것이 가장 큰 위험이다."
source_intake: "2026-09-26 디자인 리뷰 (보드 v1-v8, 최종 board-final)"
created_at: "2026-09-26"
updated_at: "2026-09-26"
---

# PRD: 웹 Overview의 Tasks 뷰(Board · Dependencies)와 Agents 뷰

## Goal

운영자는 여러 프로젝트에서 여러 에이전트를 동시에 돌리고, 일은 점점 태스크(GitHub 이슈, 나중에는 Linear나 로컬 파일) 단위로 흘러간다.
지금 웹 Overview의 Tasks 보드는 체크아웃이 중심이라 "어떤 일이 있고, 어디까지 갔고, 무엇이 무엇을 막고 있으며, 누가 나를 기다리나"를 한눈에 답하지 못한다.
이 PRD는 Tasks 뷰를 태스크가 중심인 Board와 Dependencies 두 모드로, Agents 뷰를 태스크 칩을 단 에이전트 카드로 바꾼다.
목표는 사람이 인지적으로 한눈에 일의 상태를 잡는 것이고, 화면에는 운영자가 행동할 수 있는 상태만 올린다(design 13).

추적 이슈는 #175다.
승인된 디자인은 `board-final.pen`의 여섯 시트다.
이미지와 원본은 공개 자산 저장소에 있다:

- [1 · 범례, Task Board, Dependencies (프로젝트 범위)](https://github.com/yansfil/pr-assets/blob/main/hide/task-agents-views/design-final/1-task-project.png?raw=true)
- [2 · 카드 상태 시트 15개 조합과 카드 anatomy](https://github.com/yansfil/pr-assets/blob/main/hide/task-agents-views/design-final/2-card-states.png?raw=true)
- [3 · All projects 범위의 Board와 Dependencies](https://github.com/yansfil/pr-assets/blob/main/hide/task-agents-views/design-final/3-task-all-projects.png?raw=true)
- [4 · Agents 뷰 두 범위, 빈 화면·출처 읽기 실패·40개·긴 제목](https://github.com/yansfil/pr-assets/blob/main/hide/task-agents-views/design-final/4-agents-and-states.png?raw=true)
- [5 · Light 모드와 요약](https://github.com/yansfil/pr-assets/blob/main/hide/task-agents-views/design-final/5-light-and-summary.png?raw=true)
- [6 · 이슈 vs PR 9가지 경우, Agents 행, 규칙 요약](https://github.com/yansfil/pr-assets/blob/main/hide/task-agents-views/design-final/6-issue-vs-pr.png?raw=true)
- 원본: [board-final.pen](https://github.com/yansfil/pr-assets/blob/main/hide/task-agents-views/design-final/board-final.pen), 생성 스크립트 [build-final.mjs](https://github.com/yansfil/pr-assets/blob/main/hide/task-agents-views/design-final/build-final.mjs) (저장소 루트에서 `node <path>/build-final.mjs <out.pen>`, `agents/runs/<slug>/design/` 아래에 두면 상대 경로가 맞는다)

## Non-goals

- 에이전트 관계 Graph 뷰(hcoord의 생성·감시 관계): 사용자가 다음 논의로 미뤘다.
  이 PRD의 Dependencies는 태스크 사이의 선행 관계만 그린다.
- Linear와 로컬 파일 태스크 출처: 스냅샷 모양은 출처에 중립으로 두되(D-14), 이번에 구현하는 어댑터는 GitHub 이슈 하나다.
  로컬 파일 카드는 보드에 목표 모양으로만 그려져 있고, 파일 형식은 정해지지 않았다(Q7).
- task-factory 연결: 나중 계획이다.
- Swift 셸: 동결 상태라 이 화면을 옮기지 않는다.
  다만 코어 스냅샷이 바뀌므로 Swift 디코딩이 깨지지 않아야 한다(Risks).
- 태스크 편집(제목 변경, 선행 관계 추가, 상태 이동): 출처에서 한다.
  hide는 읽고 연다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 범위 × 뷰 모델은 #169 그대로다: 사이드바가 범위(All projects · 프로젝트 · 체크아웃)를, 헤더 탭이 뷰(`Tasks · Agents · Sessions`)를 고르고, 뷰는 범위를 바꿔도 유지되며, 그 뷰가 없는 범위는 첫 뷰로 떨어진다. | #169 머지; 사용자 "우선 그렇게 진행하자" |
| D-02 | Tasks 뷰 안에 `Board | Dependencies` 세그먼트 컨트롤을 탭 줄 오른쪽에 둔다. 새 탭이 아니라 Tasks의 모드다. | 사용자 "Task 안에서 의존성관계를 graph View로 볼수있게"; Q1은 열어 둔다 |
| D-03 | Board 열은 `백로그 · 준비 · 진행 중 · 리뷰 · 완료`, 헤더는 `이름 · 개수`. 단계는 지금처럼 Git이 정한다(머지 → 열린 PR → 변경이나 앞선 커밋 → 준비). 백로그는 연결된 체크아웃이 없는 열린 태스크다. 완료 열은 접힌 한 줄 목록(`제목 >`)이고 헤더의 `>`가 펼친다. 에이전트와 이슈 상태는 단계를 움직이지 않는다. | board-final 1, 3; 기존 `stageOf` 규칙 |
| D-04 | 태스크 카드 anatomy: 머리줄(출처 글리프 + `#id` 작고 muted, 그 뒤 제목; 제목이 여러 줄이면 ID는 첫 줄에 붙는다) → 막힘 줄(자물쇠 + `#171`, warning) → 배송 사실 칩(`N files` warning, `↑N`, PR 칩 + CI 칩, `↓N behind`) → 에이전트 행. 에이전트 행은 최대 2개이고 주의가 필요한 에이전트가 먼저, 나머지는 `+N`. | board-final 2 "4. Task 카드 상태 시트", 15개 조합 |
| D-05 | 줄이기 규칙: 카드에 링크 줄이 없다(`#id`가 출처를 열고, 출처 이름과 브랜치는 ID 툴팁). "에이전트 시작"은 에이전트가 없는 준비 카드에 hover/focus일 때만 보인다. 헤더의 주 버튼은 `New agent` 하나다. | 사용자 "불필요한 버튼이나 텍스트가 있다면 좀 줄이는 방향으로" |
| D-06 | 이슈는 할 일, PR은 결과물이다. 이슈는 카드 머리의 `#170`(GitHub는 circle-dot, 로컬 파일은 file-text 글리프)이고 출처 URL을 연다. 맨 `#N`은 언제나 이슈다. PR은 `PR #174` 칩(git-pull-request 글리프, 수명주기 색 open 초록 · draft 회색 · merged 보라 · closed 빨강, 읽었으면 CI 표시)이고 그 PR을 연다. | 사용자 "issue, pr 에 대한 명확한 구분"; board-final 6의 9가지 경우 |
| D-07 | 이슈와 PR의 경우들: 체크아웃 없는 이슈는 백로그 카드. 미추적 체크아웃은 브랜치를 제목으로, PR 칩만 달고, 조용한 "태스크 없음" 줄을 둔다. PR 하나가 이슈 둘을 닫으면 두 카드 모두에 같은 PR 칩이 붙는다. 머지된 카드는 흐리게, `↓N behind`는 완료 카드에서도 보인다. | board-final 6 경우 1-9b; board-final 2 경우 13-15 |
| D-08 | 클릭: 카드 머리(제목)는 그 태스크의 체크아웃을 연다. 에이전트 행은 그 에이전트의 pane을 연다. `#id`는 출처를, PR 칩은 PR을 연다. | board-final 6 "17. 규칙 한 장 요약" |
| D-09 | Dependencies는 Board와 같은 카드에 오른쪽 위 조용한 상태 단어 하나(`진행 중`, `준비`, `백로그`, `리뷰`, `완료`)를 더한다. 간선은 왼쪽에서 오른쪽으로, 선행 카드의 오른쪽 가운데에서 막힌 카드의 왼쪽 가운데로 간다. 간선마다 라벨은 없고 범례가 한 번 설명한다. 막힘 = 자물쇠 + 흐림, 완료 = 흐림, 사람을 기다림 = warning 테두리 + `?`, 오류 = destructive 테두리. 관계 없는 태스크는 아래 따로 모인다. 미추적 체크아웃은 Board에만 나온다. | 사용자 "task dependency에서도 task의 상태같은거가 카드에 동일하게 보이면"; board-final 1 "3." |
| D-10 | All projects 범위: Board는 모든 프로젝트의 태스크를 한 보드에 섞고, 출처가 없는데 에이전트가 있는 프로젝트는 보드 아래 한 칸(`<프로젝트> · 태스크 출처 연결 안 됨`, 에이전트 수, `GitHub 이슈 연결`)으로 모인다. Dependencies는 카드 제목 위에 프로젝트 이름을 달고, 프로젝트를 넘는 선행 관계(herdr-ide #170 → sasu 태스크)를 그린다. | board-final 3; 사용자 "모든 project 기준으로도 볼수있으면" |
| D-11 | 기다리는 것 띠: 무언가 기다릴 때만 헤더와 탭 사이에 나온다. 행은 `마크 · (프로젝트 ·) #id · 브랜치`(mono) + 질문 한 줄 + 나이 + `>`이고 행 전체가 그 에이전트의 pane을 연다. 라벨, "Observer 경유" 힌트, 열기 버튼은 없다. 답은 pane에서 한다. | v7 보드 승인(#169 뒤 PR (c)); 줄이기 라운드 |
| D-12 | Agents 뷰: 열은 지금처럼 `진행 중 · 내 확인 대기 · 끝`. 카드는 `마크 · 제공자 · 제목 · 나이`, 둘째 줄 체크아웃(All projects에서는 `프로젝트 · 브랜치`), 셋째 줄 태스크 칩(이슈 `#170`, 로컬 태스크는 file-text + 제목, 없으면 조용한 "태스크 없음")과 원격 기기 칩(서버 글리프 + 기기 이름). 위임받은 에이전트는 부모 아래 들여쓴다. PR은 태스크 칩 툴팁(제목 · 브랜치 · PR 번호)에만 나온다. | 사용자 "각 agents, tasks에서 어떻게 보일지만 합의"; board-final 4, 6 "16." |
| D-13 | 상태는 가장 작은 형태로 그린다(design 9): 빈 화면은 아이콘 + 한 문장 + `GitHub 이슈 연결`; 출처 읽기 실패는 마지막으로 확인한 상태를 그대로 두고 작은 warning 표시와 툴팁만, 배너 없음, 자세한 오류는 진단 로그; 40개 넘는 열은 열 안에서 스크롤하고 `N개 더 보기` 캡션; 긴 제목은 줄바꿈하고 ID는 첫 줄 위에 붙는다. | board-final 4 "10."; design 9, 13 |
| D-14 | 태스크 출처는 추상이다. 스냅샷의 태스크는 `출처 종류 · 출처 안의 ID(없을 수 있음) · URL · 제목 · 열림/닫힘 · 선행 태스크 참조들`을 갖고, 웹은 GitHub 모양을 직접 읽지 않는다. 이번 어댑터는 GitHub 이슈 하나다. | 사용자 "github issue일 수도 있고 linear일수도 있고 아니면 내 로컬일수도" |
| D-15 | 색·크기·간격은 기존 토큰만 쓴다(`--pr-open/draft/merged/closed`, `--agent-working`, `--warning`, `--destructive`, `--success`, `--size-agent-mark` 등). 새 토큰은 없다. 마크는 모든 뷰가 `--size-agent-mark` 한 크기를 공유한다. | build-final.mjs가 쓰는 토큰이 모두 `design/tokens.json`에 있음을 확인 |
| D-16 | 승인된 화면은 구현 PR에서 `scripts/pen-screens.mjs` + `node scripts/gen-screens.mjs`로 `design/hide-screens.pen`의 Screen 시트로 옮기고, 같은 PR에서 코드를 바꾼다(DESIGN_WORKFLOW 5-8). `board-final.pen`은 참조 원본이지 커밋 대상이 아니다. | docs/DESIGN_WORKFLOW.md |
| D-17 | Light와 Dark 모두 같은 규칙이다. | board-final 5 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 프로젝트를 고르고 Tasks를 누르면 다섯 열 보드가 보이고, 연결된 체크아웃이 없는 열린 이슈는 백로그에 있다. | D-03 |
| B2 | 이슈 #170에 연결된 워크트리에 변경 파일이 4개면 카드는 진행 중 열에 `4 files`를 달고 있다. | D-03, D-04 |
| B3 | 그 워크트리에서 PR #174를 열면 카드는 리뷰 열로 가고 초록 `PR #174` 칩과 CI 표시를 단다. 머리의 `#170`은 그대로 이슈다. | D-06 |
| B4 | PR이 머지되면 카드는 완료 열의 접힌 목록으로 가고, 펼치면 흐린 카드에 보라 PR 칩이 있다. | D-03, D-07 |
| B5 | `#170`을 누르면 이슈가, `PR #174`를 누르면 PR이, 제목을 누르면 체크아웃이, 에이전트 행을 누르면 그 pane이 열린다. | D-08 |
| B6 | 에이전트가 셋인 카드에는 둘만 보이고 `+1`이 붙으며, 질문 중인 에이전트가 먼저다. | D-04 |
| B7 | 에이전트 없는 준비 카드에 마우스를 올리거나 포커스하면 "에이전트 시작"이 나오고, 떼면 사라진다. | D-05 |
| B8 | GitHub에서 #171이 #170에 막혀 있으면 #171 카드에 자물쇠 `#170`이 보이고, Dependencies에서는 #170에서 #171로 간선이 그려진다. | D-04, D-09 |
| B9 | Dependencies로 바꾼 뒤 다른 프로젝트나 All projects로 범위를 바꿔도 Tasks 뷰에 남는다. 모드 유지 여부는 D-01의 뷰 유지와 같게 둔다(기본값, 사용자 확인 전). | D-01, D-02 |
| B10 | 에이전트가 질문하면 헤더 아래 띠에 한 줄이 생기고, 그 줄을 누르면 그 에이전트의 pane이 열린다. 기다리는 것이 없으면 띠는 없다. | D-11 |
| B11 | Agents 뷰의 카드에는 태스크 칩 `#170`만 있고, 칩에 마우스를 올리면 제목 · 브랜치 · PR 번호가 보인다. | D-12 |
| B12 | 원격 기기(mini)에서 도는 에이전트 카드에는 기기 칩이 붙는다. | D-12 |
| B13 | GitHub를 읽지 못하면 보드는 마지막 상태를 그대로 보이고 작은 표시에 마우스를 올려야 이유가 보인다. 배너는 없다. | D-13 |
| B14 | 출처도 에이전트도 없는 프로젝트의 Tasks는 빈 화면 한 문장과 `GitHub 이슈 연결`이다. | D-13 |
| B15 | All projects의 Tasks는 모든 프로젝트 태스크를 섞어 보이고, 출처 없는 프로젝트의 에이전트는 보드 아래 한 칸에 모인다. | D-10 |

## Technical structure (제안, 구현 시작 때 검증)

- herdr-core, 선행 관계 읽기: GitHub의 이슈 의존 관계(blocked by)를 읽는다.
  `gh issue list --json`에는 이 필드가 없으므로 GraphQL(`gh api graphql`)의 Issue `blockedBy` 연결을 쓰는 안이 유력하다.
  메서드와 필드 이름은 GitHub 공식 문서에서 먼저 확인하고(engineering 6), 추측하지 않는다.
  프로젝트를 넘는 참조(`owner/repo#n`)가 가능하므로 `IssueReference`를 그대로 쓴다.
- herdr-core, 읽는 자리: 기존 GitHub worker 경로(`github.rs`의 `read_issues`, `COMMAND_TIMEOUT`)에 붙인다.
  `Mutex<Runtime>` 아래에서 `gh`를 부르지 않는다(AGENTS.md Performance Guide).
  `ISSUE_LIMIT` 200 상한은 유지하고, 넘으면 지금처럼 overflow로 보고한다(engineering 15).
- herdr-core, 스냅샷: 태스크는 출처 중립 모양(D-14)으로 내보내고 필드는 더하기만 한다.
  기존 필드 이름과 열거형 값은 바꾸지 않는다: Swift 셸이 그 스냅샷을 디코딩하고, 열거형 값 하나가 어긋나 셸이 멈춘 적이 있다(PR #125).
  wire 변환은 `herdr-core/src/wire.rs`에만 둔다.
- web, 파생: `web/src/projectBoard.ts`가 다섯 열과 카드 모델(머리, 막힘, 배송 사실, 에이전트 최대 2 + N, 상태 단어)을 순수 함수로 만든다.
  기존 `stageOf`, `AGENT_COLUMNS`, `prioritized`를 확장하고 평행 구현을 만들지 않는다(engineering 7).
- web, Dependencies 배치: 선행 깊이로 열을 나누는 왼쪽→오른쪽 층 배치다.
  elkjs, dagre 같은 기존 라이브러리를 먼저 검토하고 채택 여부와 이유를 PR에 적는다(engineering 6).
  간선은 카드 오른쪽 가운데에서 왼쪽 가운데로 가는 SVG다.
- web, 기다리는 것 띠: 스냅샷의 에이전트 행(`needs_you`와 미확인 완료)에서 만들고, 행 클릭은 기존 pane 포커스 이벤트 하나를 보낸다(사용자 동작 하나 = 이벤트 하나).
- 디자인: 승인 화면을 `design/hide-screens.pen`의 Screen 시트로 옮기고 `node scripts/check-design-contract.mjs`를 통과시킨다(D-16).
- 문서: 웹 Overview를 소유한 `docs/UI_BEHAVIOR.md` 절(`docs/README.md`가 가리키는 곳)을 같은 PR에서 고친다.

## Delivery

한 이슈를 세 PR로 나눈다; 각 PR은 혼자 머지될 수 있어야 한다.

1. Tasks Board와 Agents 뷰: 출처 중립 태스크 스냅샷(GitHub 어댑터), 다섯 열, 카드 anatomy, 이슈와 PR, 줄이기 규칙, Agents 카드의 태스크 칩과 기기 칩, 두 범위, 상태들, Screen 시트.
2. Dependencies 모드: 선행 관계 읽기, 층 배치, 간선, 상태 단어, 프로젝트 간 간선.
3. 기다리는 것 띠.

## Open questions

- Q1. Dependencies는 Board와 같은 화면의 모드로 충분한가, 별도 탭이 나은가(지금은 모드).
- Q2. 출처가 여럿일 때(GitHub + 로컬) All projects에서 한 보드에 섞을지, 출처별로 나눌지.
- Q3. 40개 넘는 백로그는 열 안 스크롤로 충분한가, 따로 넓혀 보는 방식이 필요한가.
- Q4. 나중의 에이전트 Graph와 이 Dependencies를 같은 화면에서 토글할지, 완전히 나눌지.
- Q5. All projects 사이드바 행에 에이전트 수를 보일지(#169에서 남은 질문).
- Q6. All projects Board 카드에는 프로젝트 이름이 그려져 있지 않다(Dependencies에는 있다). 섞인 보드에서 어느 프로젝트인지 어떻게 보일지.
- Q7. 로컬 파일 태스크의 형식과 위치.
- Q8. 백로그 카드(체크아웃 없음)에서 에이전트를 시작하는 흐름(워크트리 생성 포함)은 그려지지 않았다.

## Risks

- 스냅샷 변경이 Swift 디코딩을 깨면 운영자의 앱이 멈춘다: 더하기만 하고, 구현 PR에서 Swift 디코딩 테스트(`scripts/verify-swift.sh`)를 돌린다.
- GitHub 의존 관계 API가 계정이나 저장소 설정에 따라 비어 있을 수 있다: 그때 막힘 줄과 간선은 없고, 읽기 실패는 D-13의 작은 표시와 진단 로그로 간다.
- GraphQL 호출이 늘어나면 rate limit에 가까워진다: 기존 갱신 주기와 상한 안에서 한 번에 읽고, 태스크마다 따로 부르지 않는다.
- 라이브 확인은 격리 Herdr 서버와 임시 저장소에서 하고, 운영자의 앱과 pane은 건드리지 않는다(docs/PERFORMANCE_TESTING.md).
