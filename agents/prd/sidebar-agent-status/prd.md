---
topic: "Sidebar agent status: waiting-on-child parent, descendant badge popover, progress visibility"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "It changes the core status projection every snapshot consumer reads and the web sidebar's rows, with an additive wire field kept compatible with the frozen Swift shell; no data migration or external effect."
source_intake: "agents/interview/sidebar-agent-status/qa-log.md"
created_at: "2026-09-26"
updated_at: "2026-09-26"
---

# PRD: Sidebar agent status: waiting-on-child parent, descendant badge popover, progress visibility

## Goal

hide 운영자는 web 사이드바에서 여러 에이전트를 훑어보며 어디에 손을 대야 할지 판단한다.
지금은 자식에게 일을 맡기고 기다리는 부모가 Done 그룹에 회색으로 들어가 일이 끝난 것처럼 보이고, 모든 행이 계속 바뀌는 progress 문장을 늘 보여 줘서 읽을 것이 에이전트 수만큼 늘어난다.
이 변경으로 자식을 기다리는 부모는 파란 링으로 Working 그룹에 남고, 부모의 badge를 누르면 자식 목록을 바로 보고 이동할 수 있으며, progress 문장은 운영자를 기다리거나 새로 바뀌었거나 선택한 행에만 나타난다.

## Non-goals

- 사이드바에서 에이전트를 중지하지 않는다. 팝오버에는 이동과 펼치기만 있다. 재검토: 사용자가 사이드바 중지를 요청할 때(D-04).
- Swift shell의 사이드바는 바꾸지 않는다. 동결된 앱은 새 필드를 무시하고 해당 행을 일반 Working 행으로 그린다(D-07, D-10).
- 진행률 숫자나 단계 표시는 만들지 않는다. 에이전트가 보고하지 않는 값은 그리지 않는다(D-05).
- S10(Swift 삭제)과 Electron 포팅은 별도 PRD다(D-11).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 부모(루트) 자신은 idle 또는 done인데 살아 있는 자손 중 하나라도 조용하지 않으면(작업 중, 질문, 승인, 에러) 파생 상태 "자식 대기"로 그린다. 표시는 작업 색의 속이 빈 링이고 Working 그룹에 둔다. 자손의 요청은 지금처럼 badge에 표시되고 부모를 안 읽음으로 만들지만 Needs You로 옮기지는 않는다. 부모와 모든 자손이 조용해져야 Done이 된다. 표시 우선순위는 자기 요청 > 자기 작업 중 > 자식 대기 > idle/done이다. 파생은 core 상태 모델에서 한다. | Q1 "A", 대화 "B안으로 가자" |
| D-02 | 자손 badge(상태별 표시와 개수, 모든 살아 있는 자손 합산, 접혀 있을 때 표시)는 지금대로 둔다. | 대화 "badge 괜찮은듯한데" |
| D-03 | badge를 누르면 직접 자식 목록 팝오버가 열린다: 상태 표시, 이름, 상태 단어, 다를 때만 브랜치, 경과 시간. 강조된 자식은 Enter나 이동 버튼으로 연다. 아래 항목으로 목록에서 자식들을 펼친다. 손자는 badge에 합산되고 자기 부모 아래에서만 보인다. | 대화 "badge 팝오버도 좋아" |
| D-04 | 팝오버에 중지 동작은 없다. 되돌릴 수 없는 동작이라 별도의 확인 흐름이 필요하다. | 가정: 위임 하 design principle 6 |
| D-05 | 행 글자 규칙: 1줄은 항상 안정된 task 이름(identity label)과 상태 표시, badge, 경과 시간이다. progress/detail 줄은 (a) 운영자를 기다릴 때 질문이나 승인 문장을 경고색으로, (b) 마지막으로 본 뒤 바뀌었을 때(안 읽음) 밝게 보이고 본 뒤 사라지며, (c) 선택하거나 hover할 때 전체 문장을 두 줄까지(넘치면 툴팁) 보인다. 그 외에는 한 줄이다. 브랜치 칩은 에이전트의 체크아웃이 부모나 그룹 맥락과 다를 때만 보인다. 진행률 숫자를 지어내지 않는다. | 대화 "좋은 것 같은데?"(Pen 보드 D, E) |
| D-06 | 현재 web 사이드바는 core 그룹별 목록, 들여쓴 위임 행, per-state 개수를 title로 가진 `↳N` badge, `agent_kind / detail` 둘째 줄이다. core가 group, unread, emphasized, descendant_counts를 소유한다. | 사실: `web/src/sidebar.tsx:90-205`, `docs/status-model.md` |
| D-07 | web shell만 바꾸고 web-design-system-reset의 System 부품(Popover, Badge, Tooltip), 토큰, 테마 위에 만든다. Swift shell은 바꾸지 않는다. core 상태 모델 변경은 모든 snapshot 소비자에 적용된다. | 위임 "지금 리디자인한거랑 그거 바탕으로 implement" |
| D-08 | `docs/status-model.md`에 자식 대기 파생, 그룹 배치, 표시 우선순위, 회귀 담당 테스트를 같은 변경에서 적는다. | 가정: AGENTS.md 담당 가이드 갱신 규칙 |
| D-09 | 증거: 파생과 그룹 배치 core 단위 테스트, 팝오버 열기와 키보드와 이동과 progress 노출 규칙 web 테스트, Light/Dark 사이드바 캡처를 Pen 보드 A~E 옆에 두어 `agents/runs/`에 남긴다. | 가정: 위임 |
| D-10 | snapshot에 새 enum 값을 추가하지 않는다. 자식 대기는 group=working과 새 가산 boolean 필드(예: `waiting_on_descendants`)로 전달되고, 옛 decoder는 이를 무시한다. | 가정: PR #125 wire enum 불일치 사고, D-07 |
| D-11 | S10과 Electron 포팅은 같은 위임 흐름의 별도 PRD다. | 위임 메시지(각 항목 분리) |
| D-12 | 원칙 입력: `~/projects/oh-my-principle` 654485f의 engineering, design 문서를 읽었다. design 4, 7, 9, 10, 13은 B1~B8로, engineering 4, 13은 B2, B9로 옮겼다. design 1, 2, 3, 5, 6, 8, 11, 12는 B6(한글 줄바꿈)과 D-04 외에 새 행동이 없어 옮기지 않았다. | 원칙 intake |
| D-13 | 전달은 `agents/config.json`의 PR 모드이고, 기반 PR이 main에 들어간 뒤 그 위에서 시작한다. 기반이 머지되지 않았으면 그 브랜치 위에 쌓는다. | 위임 "stackedpr을 하든 머든 어케든" |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 자기 일은 끝났고 자식이 작업 중인 부모 행은 속이 빈 파란 링으로 Working 그룹에 보이고, badge가 자식 상태(예: 파란 점 1)를 보여 준다. | D-01, D-02 |
| B2 | 그 부모의 자식이 질문하면 부모는 링을 유지한 채 Working에 남고 badge에 `?1`이 뜨며 부모 행이 안 읽음으로 밝아진다. Needs You로는 옮겨지지 않는다. | D-01 |
| B3 | 부모와 모든 자손이 조용해지면 부모는 Done 그룹으로 옮겨지고 링은 사라진다. 자식 pane이 닫히면 다음 projection에서 badge와 상태가 그에 맞게 바뀐다. | D-01, D-02 |
| B4 | 부모 자신이 질문 중이면 `?`, 부모 자신이 작업 중이면 채운 점이 링보다 우선한다. | D-01 |
| B5 | badge를 클릭하거나 키보드로 활성화하면 직접 자식 목록 팝오버가 열리고 각 행에 상태 표시, 이름, 상태 단어, 다를 때 브랜치, 경과 시간이 보인다. 방향키로 이동하고 Enter나 이동 버튼으로 그 자식 pane이 열리며, Esc로 닫으면 포커스가 부모 행으로 돌아온다. | D-03 |
| B6 | 팝오버 아래 항목을 누르면 목록에서 부모의 자식들이 펼쳐진다. 팝오버가 열려 있는 동안 자식이 사라지면 그 행이 빠지고, 남은 자식이 없으면 팝오버가 닫힌다. 팝오버에는 중지 동작이 없다. | D-03, D-04 |
| B7 | 조용한(읽은) 행은 한 줄이다. 운영자를 기다리는 행은 질문이나 승인 문장을 경고색 둘째 줄로 보여 주고, 그 줄은 요청이 해결될 때까지 남는다. 새로 바뀐 행은 progress를 밝은 둘째 줄로 보여 주고, 운영자가 그 행을 보고 나면 그 줄만 사라진다. | D-05 |
| B8 | 선택하거나 hover한 행은 progress 전체 문장을 두 줄까지 보여 주고, 더 긴 문장은 툴팁으로 전부 보인다. 한글과 긴 식별자도 행 밖으로 넘치지 않는다. | D-05, D-12 |
| B9 | 브랜치 칩은 에이전트의 체크아웃이 부모나 그룹 맥락과 다를 때만 보이고, 진행률 숫자나 단계는 어디에도 그려지지 않는다. | D-05 |
| B10 | 새 표시와 팝오버는 Light와 Dark 두 테마에서 System 부품과 토큰으로 그려진다. | D-07 |
| B11 | 동결된 Swift 앱은 새 snapshot을 받아도 멈추거나 decode 오류를 내지 않고, 자식 대기 부모를 일반 Working 행으로 보여 준다. | D-10 |
| B12 | `docs/status-model.md`가 자식 대기 파생, 그룹 배치, 우선순위, 회귀 담당 테스트를 설명한다. | D-08 |
| B13 | 전달 시점에 core 테스트, web 테스트, Light/Dark 캡처와 Pen 보드 비교가 `agents/runs/`에 있고 커밋되지 않는다. | D-09 |

## Technical structure

- core 상태 모델(`herdr-core` sidebar/lineage projection)이 자식 대기를 파생하고, 그룹을 Working으로 두며, snapshot의 에이전트 행에 가산 boolean 필드 하나를 더한다. enum 값은 늘지 않는다. web 생성 타입은 schema에서 다시 만든다.
- web 사이드바 행과 badge 팝오버는 기반 PRD의 System 부품 위에 다시 그린다. 새 데이터 소스나 프로세스는 없다.

## Risks

- 기반 PR(web-design-system-reset)이 먼저 들어가야 한다. 들어가지 않았으면 그 브랜치 위에 쌓고, 충돌은 기반 머지 뒤에 정리한다.
- pet 배지와 그룹 개수도 core 그룹을 읽으므로, 자식 대기 부모가 Done에서 Working으로 옮겨 가면 그 개수가 바뀐다. 의도된 변화이고 core 테스트가 보장한다.
- Pen 보드(`agents/runs/sidebar-child-status/design/scratch.pen`)는 로컬 scratch다. 구현자는 그 보드를 참고하되 최종 모양은 기반 PRD의 System 부품을 따른다.
- 사용자가 미리 할 일은 없다. 모양에 대한 최종 확인은 아침 검토 항목이다.
