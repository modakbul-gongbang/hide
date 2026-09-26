---
topic: "sidebar-readability"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "기존 상태 계약을 유지하는 웹 사이드바 재설계이며 접힘, 접근성, 공유 행의 회귀 검증이 필요하다."
source_intake: "current conversation"
created_at: "2026-09-26"
updated_at: "2026-09-26"
---

# Agents / Projects sidebar readability

## Goal

사용자가 Projects에서 프로젝트, 체크아웃, 에이전트의 소속과 상태를 즉시 구분하고 Agents에서 필요한 작업을 안정된 위치에서 열 수 있게 한다.
승인된 Pen v2의 오른쪽 조작 영역과 명확한 정보 위계를 적용하고 hover로 행 높이와 시간이 바뀌는 문제를 없앤다.

## Non-goals

- Swift shell 개편, 상태 판정·읽음·소유권 프로토콜 변경, 새 에이전트 실행 API, 원격 제어 권한 확대.
- SSH Projects의 새 접힘 저장 기능, 에이전트 생성 시 자동 펼침 정책, 새 pane 열기 대기·실패 UI.
- 시안의 추가 branch/device 전용 줄 도입과 사이드바 밖 Overview의 밀도 재설계.
- PR 병합, 제품 배포 및 기존 운영 세션 변경.

## Decisions

| ID | 결정 | 근거 |
| --- | --- | --- |
| D-1 | 웹 Agents와 Projects의 기존 목록을 개선한다. | 사용자는 현재 웹의 과밀한 텍스트와 약한 계층을 지적하고 기존 Swift와 Orca 이미지를 참고로 제시했다. |
| D-2 | 계층은 정렬, 들여쓰기, 글자 무게와 여백으로 구분하고 펼친 체크아웃에만 작은 그룹 배경을 둔다. | 승인된 Pen v2; 왼쪽 아이콘과 접기 칸의 중첩을 줄여 제목 공간을 확보한다. |
| D-3 | 프로젝트와 부모 에이전트의 접기 버튼을 오른쪽으로 옮긴다. 펼친 버튼은 hover/focus-within/메뉴 열림에서, 접힌 버튼은 항상 보인다. | Orca 참고와 v2 권장안 B를 채택하는 구현 기준이다. 버튼 칸은 노출 전에도 예약한다. |
| D-4 | 내용 종류별 높이를 고정하고 hover·focus·선택으로 줄을 추가하거나 늘리지 않는다. | 사용자가 보고한 높이 변화의 직접 해결이다. quiet 상세는 tooltip에서, news/request 상세는 기본 한 줄에서 읽는다. |
| D-5 | 시간은 오른쪽에 계속 보이며 메뉴와 별도 공간을 쓴다. | Projects 안 에이전트 시간도 유지한다. agent elapsed는 표시 상태 변경 후 경과, checkout age는 마지막 Git 커밋 나이, project recency는 authoritative 활동 시각이다. |
| D-6 | Projects 안 부모 에이전트에도 기존 계보 접힘과 자식 배지를 연결한다. | v2의 실행·자식 생성·접힘 사례를 실제로 지원한다. 현재 Projects의 항상 펼친 계보에서 바뀌는 명시적 동작이며 Agents와 동일한 core 확장 상태를 쓴다. |
| D-7 | 탐색, 읽음, 상태, 소유권과 실제 집계는 기존 core 계약을 따른다. | 프로젝트 본문은 Overview, 체크아웃 본문은 Workspace, 에이전트 본문은 해당 pane을 연다. 접기와 메뉴는 탐색이 아니다. |
| D-8 | 로딩·빈 상태·실패·미확인·연결 끊김을 각 표면의 현 계약대로 표현한다. | 시안은 상태 커버리지 참고이며 존재하지 않는 데이터·숫자·오류 처리 기능의 추가 권한이 아니다. |
| D-9 | 키보드, hover 없는 입력, 좁은 폭, 한글·영문 장문, Light/Dark를 같은 구성요소의 상태로 검증한다. | 숨긴 조작도 접근 가능해야 하고 마우스를 올려야만 핵심 상태를 읽을 수 있어서는 안 된다. |
| D-10 | 기존 토큰·System 부품을 재사용하고 승인된 행과 상태를 Component 및 screen 시트와 코드에 함께 반영한다. | 디자인 workflow가 시각·수치·코드의 소유권을 나눈다. scratch는 커밋하지 않고 반복되는 상태는 master/ref로 표현한다. |
| D-11 | 이슈 #174와 `feat/sidebar-readability`를 Mac mini 전용 worktree에서 이어받아 구현·검증·리뷰·origin push·main 대상 PR 생성 및 CI 확인까지 진행한다. 기존 인계만 수행한다는 제한은 이 후속 지시로 종료하며 병합·배포는 제외한다. | 사용자 후속 지시: “mac mini에 herdr로 작업하게 해줄래? opus 5.5로 /goal 해서 이거 작업하라고 해서 PR까지 다 올리게~ 하면 좋을듯~?” 기존 디자인과 제품 범위는 유지한다. |
| D-12 | engineering/design 원칙을 기존 구조 재사용, 실제 상태 표시, 좁은 오류 노출과 부수효과 없는 접힘으로 적용한다. | `oh-my-principle`의 `engineering/principles.md`, `design/principles.md`, source commit `654485f96b7764c759662d2c3e9e386ebc221cf6`을 읽었다. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Projects에서 프로젝트 제목, 체크아웃 종류/이름, 에이전트 상태/제목이 서로 다른 무게와 단계별 정렬로 구분된다. 중첩 카드 테두리는 누적되지 않고 선택 배경은 실제 열린 scope에 대응한다. | D-1, D-2 |
| B2 | 프로젝트와 부모 에이전트의 펼침 버튼은 오른쪽에 있다. 접힌 버튼은 상시 보이고 펼친 버튼은 hover, focus-within 또는 메뉴 열림에서 보인다. 버튼 노출 전후 제목·시간의 x좌표와 행 높이가 같다. | D-3, D-9 |
| B3 | hover 없는 입력에서는 조작 버튼을 계속 표시한다. 잎 에이전트와 에이전트 없는 체크아웃에는 무의미한 접기 버튼이 없고 잎 행에 계보용 칸을 추가하지 않는다. | D-3, D-9 |
| B4 | 동일 내용의 행은 rest→hover→키보드 focus→selected 및 메뉴 열림에서 높이와 다음 행 y좌표가 같다. quiet 상세는 기본 행을 늘리지 않고 news/request는 처음부터 한 줄을 확보한다. | D-4 |
| B5 | Agents의 프로젝트/체크아웃 문맥은 기본 상태에 고정 줄로 보인다. Projects 안 에이전트는 상위 행이 제공하는 소속을 중복 표기하지 않는다. 제목·상세는 각각 말줄임하고 원문은 tooltip으로 읽을 수 있다. | D-2, D-4 |
| B6 | 질문·승인·오류의 요청 문장은 hover 전에도 보인다. 요청을 읽은 뒤에도 해결 전까지 의미 색과 표시를 유지한다. | D-4, D-7 |
| B7 | 에이전트 elapsed는 Agents와 Projects 내부 행 모두에서 보이고 hover·메뉴 노출로 사라지지 않는다. 체크아웃의 커밋 나이도 메뉴와 함께 남으며 누락된 시각을 임의의 0s로 채우지 않는다. | D-5, D-8 |
| B8 | 프로젝트 본문 클릭은 Overview, 체크아웃 본문은 Workspace, 에이전트 본문은 해당 pane을 연다. All projects·project·checkout·agent의 선택은 현재 중앙 화면과 일치한다. | D-7 |
| B9 | 접기 버튼·자식 배지·overflow·PR 링크 클릭은 해당 동작만 수행한다. 단순 접힘·hover·메뉴 열기는 중앙 화면, focused pane/tab, 읽음, 상태 그룹과 실행 중 프로세스를 바꾸지 않는다. | D-3, D-7, D-12 |
| B10 | 지원되는 로컬 project/checkout/agent 접힘은 기존 core 저장 상태를 사용하며 새로고침 후 유지된다. SSH Projects는 현행 펼침 정책을 유지하고 로컬 접힘 설정을 원격 트리에 적용하지 않는다. | D-6, D-7 |
| B11 | 에이전트가 없는 체크아웃, 최초 준비, 작업 시작, 자식 생성, 자식 선택, 자식 질문, 부모·체크아웃·프로젝트 접힘을 순서대로 표시할 수 있다. 새 항목만으로 자동 펼침 정책을 바꾸지 않는다. | D-6, D-8 |
| B12 | Projects의 부모 에이전트도 자식 목록을 접고 펼친다. Agents와 Projects가 동일 pane의 확장 상태를 공유하고 접힌 배지는 모든 live descendant의 실제 집계를 표시하며 펼치면 사라진다. | D-6, D-7 |
| B13 | 자식 배지는 오류→승인→질문→작업→완료 우선순위와 ready의 ↳N 표현을 유지한다. 직접 자식 popover는 상태 단어·이름·시간·다른 체크아웃을 표시하고 마지막 항목으로 전체 자식을 펼칠 수 있다. | D-6, D-7 |
| B14 | 자식 popover의 방향키·Enter·Escape가 기존 탐색과 focus 복귀를 유지한다. 접기 버튼에는 대상과 확장 상태가 접근 가능한 이름으로 전달되고 숨겨진 상태에서도 Tab으로 도달해 표시된다. | D-6, D-9 |
| B15 | 새 idle 에이전트는 Done이 아닌 회색 링으로 표시하고 작업명 전에는 Workspace 이름을 쓴다. 내부 제어 이름을 제목으로 대신 표시하지 않는다. | D-7, D-8, D-12 |
| B16 | delegated child는 Working/Seen만 사용한다. 자식의 demand/완료는 조상을 unread로 만들지만 Needs You로 승격시키지 않고 조용한 부모가 작업 중 자식을 기다리면 Working의 파란 링을 유지한다. | D-7 |
| B17 | Needs You/Done/Working/Seen은 빈 그룹을 숨기고 접힌 descendants까지 포함한 기존 집계를 유지한다. 프로젝트를 접어도 Agents의 필요한 작업 접근성과 상태 집계는 변하지 않는다. | D-6, D-7 |
| B18 | 알려지지 않은 활동은 Idle로 오인되지 않는 미확인 표시를 유지하고 관측되지 않는 하위 항목의 숫자를 만들어내지 않는다. 서버 단절은 stale Working/Done으로 표시하지 않는다. | D-8, D-12 |
| B19 | disconnected 원격 device의 오래된 에이전트는 Agents 목록에 다시 노출하지 않는다. retained Workspace summary의 Disconnected 표시는 기존 별도 계약을 따른다. | D-7, D-8 |
| B20 | 프로젝트 없음에는 Add project, 초기 연결 중에는 loading, 검색 결과 없음에는 현행 빈 결과를 표시한다. 로딩을 성공한 빈 목록이나 0 agents로 표현하지 않고 복구 후 실제 데이터로 바뀐다. | D-8, D-12 |
| B21 | 사라진 체크아웃은 missing 의미 표시를 유지하고 커밋 나이를 숨긴다. 기존 복구·제거 affordance만 사용하며 사용자가 대응할 수 없는 내부 실패에 새 배너를 만들지 않는다. | D-8, D-12 |
| B22 | 일반 폴더는 project/checkout 두 행으로 중복되지 않는다. primary는 첫 체크아웃이고 inactive로 이동하지 않으며 pinned·recent·inactive의 기존 정렬과 모든 항목의 검색 가능성을 유지한다. | D-2, D-7 |
| B23 | PR 상태, 일반 branch, primary home, detached commit의 종류 우선순위를 유지한다. stale PR만 흐리게 하고 GitHub 상태 불가 시 branch로 표시하며 hover·선택이 PR 의미 색을 지우지 않는다. | D-7, D-8 |
| B24 | 다른 체크아웃의 child와 원격 device 표시는 기존 첫 줄 메타데이터와 올바른 소유권을 유지한다. 표시 개선이 원격 명령의 대상 host나 연결·권한 조건을 바꾸지 않는다. | D-7, D-8 |
| B25 | 240px 시안 폭과 앱에서 지원하는 최소 폭에서 긴 한글/영문 제목·branch·요청을 표시해도 시간·조작 영역이 겹치지 않고 가로 overflow가 생기지 않는다. 한글은 실제 앱 폰트에서도 잘리지 않는다. | D-4, D-5, D-9 |
| B26 | Light/Dark 각각에서 선택, hover, focus ring, 의미 색과 muted 텍스트를 구분할 수 있다. 기존 글자 크기 설정을 적용해도 해당 크기 안에서 행의 hover 높이가 안정적이다. | D-9, D-10 |
| B27 | 사이드바 행 개선이 공유 구성요소를 쓰는 Overview의 행 밀도나 클릭 범위를 우연히 바꾸지 않는다. 접기·hover마다 새 네트워크 요청, subprocess 또는 독립적인 시간 갱신 루프를 만들지 않는다. | D-7, D-10, D-12 |
| B28 | 재사용 행의 상태는 기존 System 부품과 토큰으로 구성된 Component 시트에 모이고, 관련 screen·실제 앱이 두 테마에서 같은 계층과 상태를 표현한다. 승인 scratch 자체는 공유 제품 라이브러리에 들어가지 않는다. | D-10 |

## Technical structure

core snapshot은 상태·읽음·계보·접힘·소유권의 권위로 남고 웹 셸은 표시 밀도와 조작 위치만 결정한다.
Projects 계보의 새 표시도 기존 typed event와 확장 상태를 사용하며 별도 로컬 상태 모델을 만들지 않는다.
시각 기준은 Pen library/screen, 수치는 기존 token 원본, 렌더링은 공통 UI 부품과 hide 합성 구성요소에 둔다.
변경한 hover·Projects 계보 규칙은 UI behavior와 status-model의 웹 계약에 함께 반영하고 Swift 계약과 구분한다.
검증은 실제 계층 fixture의 높이/좌표, 클릭 부수효과, 키보드, 상태 전이와 두 테마의 실제 앱 관찰을 포함한다.

## Risks

- 기존 Projects는 자식을 항상 펼쳐 그린다. B12는 의도적 변경이며 다른 체크아웃 소유 child를 중복 집계하거나 숨기지 않는 회귀 검증이 필요하다.
- Pen은 한글 대체 폰트와 수동 말줄임을 쓴다. 제안 높이 project 40, checkout 36/54, agent 32/52 및 context 50/68은 구현 시 기존 토큰·실제 폰트에 맞춰 확인하며 숫자만 강제해 글자를 자르지 않는다.
- 별도 branch/device 줄과 새 pane 열기 피드백은 시안에 있어도 이번 범위에 포함되지 않는다. 기존 메뉴·권한·복구 규칙을 보존한다.
- 입력은 현재 대화와 승인된 Pen v2이며 별도 interview qa-log는 없다. 독립 Spec gate는 미실행이고 이 PRD 자체의 human approval은 pending이다. D-11의 후속 작업 지시는 별도 원문 근거로 보존한다.
- 디자인 파일은 local-only 실행 산출물이다. 이슈에 명시한 Mac mini의 비공개 bundle과 checksum으로 전달하고 원격 작업자는 PRD를 최신 동작 기준으로 삼는다.
