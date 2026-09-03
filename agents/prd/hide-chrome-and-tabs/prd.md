---
topic: "Hide chrome and tabs: header and titlebar removal, herdr-ordered tabs, one core-owned tab list"
status: "ready"
human_approval: "approved"  # user 2026-09-03 verbatim: PRD 이거 3개는 그냥 하나의 worktree 에서 다 작업해주면 되고 세개 PRD가 모두 완료되면 순차적으로 하나씩 작업해서 반영해줘 implementor는 opus5로 해서 작업해주고 context 50% 차면 implementor 새로 띄워서 계속 진행하게 해. 결과적으로 AC 들 다 수행하고 마지막에 다 완료됐으면 메인 머지까지해서최종 빌드하고 설치까지해줘. 나자러 가니까 이제 너가 알아서 저 PRD 완벽하게 수행해줘!
review_profile: "standard"
review_rationale: "User-visible shell layout and a new core-owned tab model; the only external effect is a herdr tab move confined to a throwaway workspace, with no data, credential, billing, or destructive action involved."
source_intake: "current conversation"
created_at: "2026-09-04"
updated_at: "2026-09-04"
---

# PRD: Hide chrome and tabs

## 1. Summary

작업 영역 위에 쌓인 두 줄을 걷어내고, 탭의 순서와 목록을 하나의 모델로 정리한다.

첫째, 터미널 컬럼의 54pt 헤더와 macOS 시스템 타이틀바를 없애 탭 스트립이 창의 맨 위에 붙는다.
둘째, 탭 순서가 가끔 뒤집히는 결함을 원인에서 고친다.
지금 navigator의 탭 목록은 herdr의 탭 순서가 아니라 layout 이벤트가 도착한 순서로 만들어지며, 탭 전환 핸들러가 전환된 탭을 목록 맨 앞으로 옮기는 임시 조작까지 하고 있다.
셋째, herdr 탭과 파일 탭을 Swift에서 이어 붙이는 대신 core가 checkout당 하나의 정렬된 탭 목록을 소유하고, 사용자가 스트립에서 드래그로 순서를 바꿀 수 있게 한다.

Approval checklist:

- 범위와 다섯 가지 비목표, 특히 브라우저 탭 렌더링과 파일 탭의 재시작 복원 제외 (section 3).
- 구조 변경: navigator 탭 순서의 권위를 herdr `tabs` 순서로 옮기고, core가 통합 탭 목록을 소유하며, 순서 저장소를 ui_state에 두지 않는 결정 (section 5, section 4.2).
- 타이틀바 제거 방식: 시스템 타이틀바를 숨기고 신호등 버튼을 사이드바 상단에 겹쳐 두며, 사이드바를 접으면 탭 스트립이 그 자리를 비워 둔다 (R2).
- 검증 모드: build/static, automated behavior, app runtime, 그리고 throwaway 워크스페이스에 한정된 live herdr integration 하나 (section 9.1).
- 배포 모드: local. 실행 브랜치에 커밋 하나, push와 PR 없음 (section 4.3).
- `review_profile: standard`와 그 근거 (frontmatter).

## 2. Problem, Goal, And Users

사용자는 Hide의 단일 운영자다.
Hide는 herdr 터미널 세션 위에 얹힌 SwiftUI macOS 셸이며, 하루 종일 여러 코딩 에이전트를 여기서 다룬다.

세 가지 문제가 한 번의 사용 세션에서 보고되었다.

- 화면 위쪽에 시스템 타이틀바 한 줄과 워크스페이스 이름을 반복하는 54pt 헤더 한 줄이 있어 작업 공간 높이를 잃는다.
  워크스페이스 이름과 브랜치는 사이드바가 이미 보여주고, herdr 버전은 사이드바 브랜드 헤더에도 이미 있다.
- ⌥Tab으로 탭을 오가다 보면 Tab 1과 Tab 2의 위치가 가끔 서로 바뀐다.
  원인은 코드에서 확인되었다.
  navigator는 `payload.layouts`를 순회하며 탭을 push하는데, 이 벡터는 `layout_updated`가 처음 도착한 탭을 끝에 붙이는 구조라 herdr의 탭 순서(`state.tabs`)와 무관하다.
  게다가 탭 전환 핸들러는 전환된 탭을 목록의 0번으로 옮겨 놓고, 다음 카탈로그 재구성이 그 조작을 되돌린다.
  herdr에서 탭을 옮긴 `tab_moved` 이벤트는 `state.tabs`만 바꾸고 `state.layouts`는 건드리지 않으므로 화면 순서는 영영 갱신되지 않는다.
- 앞으로 파일 탭, 브라우저 탭처럼 herdr가 모르는 탭 종류가 늘어난다.
  지금은 Swift의 `unifiedTabs`가 herdr 탭 배열 뒤에 파일 탭 배열을 이어 붙일 뿐이라 종류가 섞인 순서를 표현할 자리가 없다.

목표는 두 줄의 크롬을 걷어 작업 공간을 넓히고, 탭 순서가 herdr와 사용자의 조작만을 따르며, 종류가 다른 탭이 한 목록에서 자유롭게 섞이는 구조를 세우는 것이다.

### 2.1 User Scenarios

- SC1. 크롬 제거: 운영자가 창을 열면 탭 스트립이 창의 맨 위에 붙어 있다.
  Actors: 운영자.
  Primary path: 창 상단에 시스템 타이틀바도 워크스페이스 헤더도 없고, 신호등 버튼 오른쪽에 사이드바 브랜드 헤더가, 사이드바 옆에 탭 스트립이 바로 이어진다. herdr 버전은 브랜드 헤더에서, 연결 상태는 상태 표시줄에서 읽힌다.
  Failure state: herdr가 없거나 연결이 끊기면 그 사실이 상태 표시줄에 경고색으로 남고, 헤더가 사라졌다고 해서 경고가 함께 사라지지 않는다.
  Recovery: 사이드바를 접으면 신호등 버튼이 탭 스트립의 왼쪽 여백 위에 놓이고 첫 탭과 겹치지 않는다. 사이드바를 다시 펴면 여백은 브랜드 헤더로 돌아간다.
  Reach: 워크스페이스 하나와 터미널 pane 하나가 있는 실행 중인 셸.

- SC2. 창 이동: 운영자가 타이틀바 없이 창을 끌어 옮긴다.
  Actors: 운영자.
  Primary path: 사이드바 브랜드 헤더의 빈 영역이나 탭 스트립의 빈 영역을 끌면 창이 따라 움직인다.
  Failure state: 탭 자체를 끄는 동작은 창 이동이 아니라 탭 재정렬로 해석된다.
  Recovery: 두 동작이 충돌하는 지점이 없다. 탭 위에서 시작한 드래그는 항상 재정렬, 빈 영역에서 시작한 드래그는 항상 창 이동이다.
  Reach: 탭이 둘 이상 열린 실행 중인 셸.

- SC3. 안정된 탭 순서: 운영자가 ⌥Tab과 ⌘1..9로 탭을 오가고, herdr 쪽에서 탭이 옮겨진다.
  Actors: 운영자.
  Primary path: 탭을 아무리 오가도 스트립의 순서가 바뀌지 않고, ⌘N이 가리키는 탭도 바뀌지 않는다. herdr가 탭을 옮기면 스트립이 그 순서를 따라간다.
  Failure state: 탭 전환 직후 아직 herdr의 확인 이벤트가 오지 않은 짧은 구간에도 스트립 순서는 유지되고, 활성 표시만 확인 이벤트 뒤에 옮겨 간다.
  Recovery: 새 탭은 목록 끝에 붙고, 닫힌 탭은 빠지며, 나머지 탭의 상대 순서는 그대로다.
  Reach: 한 checkout에 herdr 탭 셋이 열린 셸과 그 세션에 접근할 수 있는 herdr CLI.

- SC4. 섞인 탭의 드래그 재정렬: 운영자가 파일 탭을 두 herdr 탭 사이로 끌어 놓는다.
  Actors: 운영자.
  Primary path: 파일 탭이 그 자리에 들어가고, ⌘N 번호와 ⌥Tab 후보 순서가 새 순서를 따른다. herdr 탭을 다른 herdr 탭 너머로 끌면 herdr에도 같은 순서가 반영되어 CLI에서 읽은 탭 순서가 스트립과 일치한다.
  Failure state: herdr가 이동을 거부하면 스트립은 herdr가 알려준 순서로 되돌아가고, 거부 사실이 사용자에게 보인다.
  Recovery: 되돌아간 뒤 다시 끌면 다시 시도된다. 파일 탭끼리, 혹은 파일 탭을 herdr 탭 사이로 옮기는 동작은 herdr에 아무것도 보내지 않는다.
  Reach: herdr 탭 둘과 파일 탭 하나가 열린 checkout.

- SC5. 새 탭의 자리: 운영자가 파일을 열고, 새 herdr 탭을 만든다.
  Actors: 운영자.
  Primary path: 파일을 열면 파일 탭이 목록 끝에 붙고 활성화된다. ⌘T로 새 herdr 탭을 만들면 그 탭도 목록 끝에 붙는다. 어느 쪽도 다른 탭의 자리를 밀어내지 않는다.
  Failure state: 이미 열린 파일을 다시 열면 새 탭이 생기지 않고 기존 탭이 활성화된다.
  Recovery: 탭을 닫으면 남은 탭들이 순서를 유지하고, 닫힌 탭이 활성 탭이었다면 최근 사용 순서의 다음 탭이 활성화된다.
  Reach: 파일이 있는 checkout과 herdr 탭 하나.

## 3. Scope And Non-Goals

범위: 터미널 컬럼 헤더 제거, 시스템 타이틀바 제거, herdr 순서를 따르는 navigator 탭 순서, core가 소유하는 checkout당 통합 탭 목록, 스트립 드래그 재정렬, 그리고 이 변경이 쓸모없게 만드는 코드의 삭제.

비목표, 각각 의도된 제외:

- 브라우저 탭.
  탭 종류는 herdr 탭과 파일 탭 둘뿐이며, 브라우저 종류는 변형(variant)을 미리 두지 않는다.
  종류가 enum이라는 구조 자체가 확장 지점이고, 쓰이지 않는 변형을 두는 것은 사변적 추상화다.
  Consequence: 브라우저 탭을 열 방법이 없다.
  Revisit: 브라우저 표면을 만드는 PRD에서 변형을 추가한다.
- 한 탭 안에서 herdr pane과 네이티브 표면(파일 편집기)을 나란히 두는 분할.
  터미널과 브라우저를 반반으로 나누는 요구는 herdr의 `official.browser` 플러그인 pane이 herdr 탭 안에서 이미 해결하므로 hide가 자체 분할 트리를 갖지 않는다.
  Consequence: 파일 탭은 항상 탭 전체를 차지한다.
  Revisit: 파일과 터미널을 한 화면에 섞는 요구가 실제로 생길 때.
- 재시작 후 파일 탭 복원.
  파일 탭은 지금도 재시작을 넘기지 않으며, 이 PRD는 그 사실을 바꾸지 않는다.
  Consequence: 재시작 뒤 파일 탭은 닫혀 있고 herdr 탭만 herdr의 순서대로 남는다.
  Revisit: 세션 복원을 다루는 PRD에서.
- 탭 이름 변경 UI.
  hide에는 지금도 이름 변경 진입점이 없다.
  Consequence: 탭 이름은 herdr가 붙인 것이다.
  Revisit: 사용자가 이름 변경을 요청할 때.
- 원격(mini) 컨텍스트의 탭 목록 변경.
  원격 경로는 이미 `state.tabs` 순서를 따르고 파일 탭이 없다.
  Consequence: 원격 탭은 드래그 재정렬을 지원하지 않는다.
  Revisit: 원격 컨텍스트에 파일 탭이 생길 때.

탭 전환과 pane 포커스의 반응성(전환 시 layout이 비워지고 터미널 뷰가 재생성되는 문제)은 별도 PRD `hide-view-state`의 범위다.
이 PRD는 순서와 목록만 다룬다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
모든 작업은 저장소와 로컬 herdr 서버 안에서 에이전트가 할 수 있다.

### 4.2 Human Decisions Before PRD Approval

- 통합 탭 목록의 순서를 ui_state에 저장하지 않는 결정을 승인한다.
  대화에서 선택한 옵션 문구는 "ui_state에 저장"이었으나, herdr 탭의 순서는 herdr가 이미 영속하고 파일 탭은 재시작을 넘기지 않으므로 저장할 것이 없다.
  저장하면 존재하지 않는 파일 탭의 자리만 남는다.
  이 결정을 거부하면 파일 탭 복원이 함께 범위에 들어와야 한다.
- 브라우저 탭 종류를 예약하지 않는 결정을 승인한다.
  대화의 옵션 문구는 "kind enum만 예약"이었고, 이 PRD는 enum이라는 형태 자체를 예약으로 본다.
- 신호등 버튼의 자리를 승인한다.
  사이드바가 펴져 있으면 브랜드 헤더가, 접혀 있으면 탭 스트립이 왼쪽 여백을 비운다.

### 4.3 Decision Traceability For Fidelity Review

이 PRD는 인터뷰 qa-log 없이 대화만을 근거로 하므로 사용자의 결정을 여기에 그대로 남긴다.

- 사용자 요청 원문 (2026-09-03): "가운데 상단바를 그냥 없애도 될듯? 바로 탭을 보여주게 하고". R1, AC1, SC1.
- 사용자 요청 원문: "저기 맨위 줄빼고 바로 붙이고 싶어.. 최대한 작업공간 높이를 많이 가져가고 싶어서". R2, AC2, AC3, SC1, SC2.
- 사용자 보고 원문: "cmd 하고 탭 왔다갔다 하면 Tab 1,2 가 Tab 2,1 이렇게 가끔바뀔때가 있어.. 이거순서가 뭔가 herdr 베이스로 해서 그런가?". 원인은 herdr가 아니라 core의 순서 유도 방식으로 확인되었다. R3, AC4, AC5, SC3.
- 사용자 요청 원문: "앞으로 File Tab도 그러고 여러 tab이 있을 수 있거든? 브라우저 탭도 있을 수 있고... 유연하게 구조를 잘 가져가야할 것 같아서 이것들이 잘 되도록 + 견고하고 단순하게 재구성을 하면 좋겠어". R4, R5, R6, SC4, SC5.
- 사용자 선택 (2026-09-04, 질문 "3번 탭 모델을 이번에 어디까지 갈까요?"): "core 소유 탭 목록 + 파일/herdr 종류". 옵션 설명은 "순서 버그 수정 + checkout당 정렬된 탭 목록을 core가 소유하고 ui_state에 저장. 드래그 재정렬 포함. 브라우저 탭은 kind enum만 예약하고 렌더링은 다음 단계". 수용: core 소유 목록(R4), 드래그 재정렬(R5), 렌더링 제외(비목표). 에이전트 재해석 두 가지는 4.2의 인간 결정으로 올렸다: ui_state 저장 생략, 브라우저 변형 미예약.
- 사용자 선택 (2026-09-04): "3개로 분할". 이 PRD는 항목 1, 3, 8을 담고 4번(반응성)은 `hide-view-state`, 6과 7(에이전트 상태)은 `hide-agent-attention`으로 분리되었다.
- 사용자 선택 (2026-09-04): "hide만 쓴다". herdr TUI와의 순서 동기화를 실시간으로 지킬 필요는 없으나, herdr 탭 순서는 여전히 herdr에 반영한다(R5). herdr가 순서의 영속 저장소이기 때문이다.
- 에이전트가 제안하고 사용자가 수용한 경계 (2026-09-03 논의): 터미널과 브라우저의 반반 분할은 herdr 플러그인 pane으로 해결하고 hide는 자체 분할 트리를 갖지 않는다. 비목표로 기록.
- 에이전트 가정 (사용자 결정 아님): 파일을 열면 파일 탭은 목록 끝에 붙는다. 활성 탭 옆에 끼우는 브라우저 관행 대신 예측 가능한 규칙을 택했다. R4, SC5.
- 에이전트 가정 (사용자 결정 아님): 창 이동은 브랜드 헤더와 탭 스트립의 빈 영역에서만 가능하다. R2, SC2.
- 배포 모드: `agents/config.json`의 `delivery.mode: local`, `worktree.enabled: true`를 그대로 따른다. push, PR, CI 없음.
- 원칙 인테이크: `~/projects/oh-my-principle` 커밋 `35ab76ca23d45e714f1630054855a8c8c4568d03`에서 `engineering/principles.md`(트리거: 코드 작성과 변경, 아키텍처 제안)와 `design/principles.md`(트리거: 사용자가 작업하는 화면의 구축과 변경)를 전문으로 읽었다. 적용 규칙은 section 11에 번역했다. engineering 규칙 9, 10, 11은 이 PRD가 새 로그 표면이나 반복 실행되는 외부 부작용을 만들지 않으므로 guardrail로 번역하지 않았고, 단 herdr 이동 거부 경로는 규칙 4와 10을 함께 적용한다. design 규칙 1, 2, 6은 목록 화면이나 파괴적 동작이 없어 번역하지 않았다.
- 프로젝트 규칙 인테이크: `AGENTS.md`의 "Herdr API Contract"(R5의 `tab.move`), "Performance Guide"(R4의 순서 계산은 lock 안에서 subprocess를 부르지 않음), "Design Reference"(R7), "Evidence Belongs Outside The Repository"(스크린샷 위치)를 section 11에 번역했다.

## 5. Major Technical Structure Changes

- navigator의 탭 순서 권위가 `state.layouts` 도착 순서에서 herdr의 `state.tabs` 순서로 바뀐다.
  layout은 tab id로 조회하는 부속 데이터가 되고, `tab_moved`가 화면 순서에 반영된다.
  탭 전환 핸들러가 전환된 탭을 목록 0번으로 옮기는 조작과, 그 조작에 기대어 첫 탭을 활성 탭으로 간주하던 Swift의 기본값은 삭제된다.
  활성 탭은 herdr가 알려주는 워크스페이스의 활성 탭 id에서 유도한다.
- core가 로컬 checkout마다 하나의 정렬된 통합 탭 목록을 소유한다.
  항목은 herdr 탭 또는 파일 탭이며, 목록이 snapshot으로 셸에 내려간다.
  Swift의 두 배열 이어 붙이기(`unifiedTabs`)는 삭제되고 셸은 core의 목록을 그대로 그린다.
  이 목록은 메모리에만 있으며 ui_state 스키마는 바뀌지 않는다.
- 새 core 이벤트 하나: 탭 재정렬.
  herdr 탭이 다른 herdr 탭을 넘어가면 core가 herdr 소켓의 `tab.move`를 호출하고, `tab_moved` 이벤트로 확정한다.
  이 메서드는 `contracts/herdr-api.schema.json`(2026-08-28 동기화본)에 있으며 hide가 처음으로 호출하는 herdr 메서드다.
- 창 구성: 시스템 타이틀바를 숨기고 콘텐츠가 창 전체를 채운다.
  창 제목 문자열은 유지된다(Mission Control, 접근성, pet 검증 영수증이 읽는다).
  신호등 버튼이 차지하는 왼쪽 여백은 디자인 토큰으로 `HideTheme.Layout`에 추가된다.
- 스키마, 저장소, 인증, 결제, 배포 변경 없음. 새 서드파티 의존성 없음.

## 6. Requirements

- R1. 터미널 컬럼의 54pt 헤더를 제거하고 탭 스트립이 컬럼의 첫 줄이 된다.
  헤더에 있던 컨트롤 중 사이드바 복원 버튼은 탭 스트립의 왼쪽 끝으로, 오른쪽 패널 복원 버튼은 오른쪽 끝으로 옮겨 각각 해당 패널이 숨겨져 있을 때만 보인다.
  헤더의 herdr 버전 라벨은 사이드바 브랜드 헤더가 이미 같은 값을 보여주므로 삭제하고, 상태 표시줄에 두 번째 버전 문자열을 넣지 않는다.
  "herdr unavailable" 경고와 원격 대상 상태는 상태 표시줄이 이미 연결 점과 상태 메시지로 보여주므로, 헤더 제거 후에도 그 정보가 같은 색 의미로 계속 읽히는지 확인하고 상태 표시줄에 빠진 경우만 보탠다.
  워크스페이스 이름, 브랜치, 아이콘은 사이드바가 이미 보여주므로 컬럼에서 제거된다.
- R2. 시스템 타이틀바를 숨기고 콘텐츠가 창 전체를 채운다.
  창 제목 문자열은 유지된다.
  사이드바가 펴져 있으면 브랜드 헤더가, 접혀 있으면 탭 스트립이 신호등 버튼만큼의 왼쪽 여백을 비워 어떤 컨트롤도 신호등과 겹치지 않는다.
  브랜드 헤더와 탭 스트립의 빈 영역을 끌면 창이 이동하고, 탭 위에서 시작한 드래그는 창을 움직이지 않는다.
- R3. navigator의 herdr 탭 순서는 herdr 워크스페이스의 탭 순서와 항상 같다.
  `tab_moved`는 화면 순서에 반영되고, 기존 탭의 `layout_updated`는 순서를 바꾸지 않으며, 새 탭은 herdr가 둔 자리에 나타난다.
  탭 전환은 순서를 바꾸지 않는다.
  활성 탭은 herdr가 알려주는 워크스페이스의 활성 탭 id에서 유도하고, 그 id가 목록에 없으면 첫 탭으로 대체하지 않고 그 상태를 진단으로 드러낸다.
- R4. core는 로컬 checkout마다 herdr 탭과 파일 탭이 섞인 하나의 정렬된 탭 목록을 소유하고 snapshot에 싣는다.
  herdr 탭끼리의 상대 순서는 R3의 순서와 같고, 파일 탭은 자기 자리를 지킨다.
  새 herdr 탭과 새 파일 탭은 목록 끝에 붙고, 이미 열린 파일을 다시 열면 기존 탭이 활성화된다.
  ⌘1..9 번호, ⌥Tab 후보, 닫기 단축키 대상은 모두 이 목록을 따른다.
  셸에서 두 배열을 이어 붙이던 경로는 삭제된다.
- R5. 스트립에서 탭을 드래그해 임의의 자리로 옮길 수 있다.
  herdr 탭이 다른 herdr 탭을 넘어가면 core가 herdr에 같은 순서를 요청하고 `tab_moved`로 확정한다.
  herdr가 거부하거나 응답하지 않으면 목록은 herdr가 알려준 순서로 돌아가고 그 사실이 사용자에게 보이며 구조화된 진단으로 남는다.
  파일 탭만 관련된 이동은 herdr에 아무것도 보내지 않는다.
- R6. 탭 라벨은 탭의 정체성에서만 나온다.
  herdr의 숫자 라벨은 "Tab N"으로, 그 밖의 라벨은 그대로 보이며, 스트립 위치에서 라벨을 만드는 경로는 삭제된다.
- R7. 스트립 높이, 신호등 여백, 컨트롤 간격은 `HideTheme`의 토큰에서 오고 뷰에 리터럴로 쓰이지 않는다.
  기존 토큰이 덮지 못하는 값은 토큰으로 추가한 뒤 사용한다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | 터미널 컬럼에서 탭 스트립 위에 아무 행도 없고, 사이드바와 오른쪽 패널 복원 버튼은 각 패널이 숨겨졌을 때 스트립 양끝에 나타나며, herdr 버전은 브랜드 헤더에서, 연결 경고와 원격 상태는 상태 표시줄에서 읽히고 같은 정보가 두 곳에 중복되지 않는다 | judged | 실행 중인 앱 캡처: 두 패널이 모두 보일 때, 왼쪽만 접었을 때, 오른쪽만 접었을 때, herdr 서버를 멈춘 상태 각각의 상단과 상태 표시줄 |
| AC2 | 창에 시스템 타이틀바가 보이지 않고 콘텐츠가 창 상단까지 채우며, 창 제목 문자열은 여전히 "hide"다 | machine | - |
| AC3 | 사이드바가 펴진 상태와 접힌 상태 모두에서 신호등 버튼이 어떤 컨트롤이나 탭과도 겹치지 않고, 브랜드 헤더와 스트립의 빈 영역을 끌면 창이 이동하며 탭 위에서 끌면 창이 움직이지 않는다 | judged | 실행 중인 앱에서 두 상태의 상단 캡처와, 빈 영역 드래그 전후 및 탭 드래그 전후의 창 위치 비교 |
| AC4 | herdr 워크스페이스에 탭이 셋일 때 navigator의 탭 순서는 herdr가 보고하는 순서와 같고, 탭 이동 이벤트 뒤에는 이동된 순서와 같으며, 기존 탭의 layout 갱신과 탭 전환은 순서를 바꾸지 않는다 | machine | - |
| AC5 | 활성 탭은 herdr가 알려준 워크스페이스 활성 탭이고, 그 id가 목록에 없을 때 첫 탭이 대신 활성으로 보고되지 않으며 진단이 남는다 | machine | - |
| AC6 | herdr 탭 둘과 파일 탭 하나가 열린 checkout의 snapshot 탭 목록은 열린 순서 그대로이고, 파일 탭을 두 herdr 탭 사이로 옮긴 뒤에는 그 순서가 목록과 ⌘N 번호와 ⌥Tab 후보에 동일하게 반영된다 | machine | - |
| AC7 | herdr 탭을 다른 herdr 탭 너머로 끈 뒤 herdr CLI가 보고하는 그 워크스페이스의 탭 순서가 스트립 순서와 같고, herdr가 이동을 거부하면 스트립이 herdr 순서로 돌아가며 거부가 사용자에게 보인다 | judged | throwaway 워크스페이스에서의 스크립트 실행: 탭 셋을 만들고, 스트립에서 하나를 끌어 옮긴 뒤 CLI 탭 목록과 스트립 캡처를 나란히 두고, 거부 경로는 존재하지 않는 탭 id로 이동을 요청해 되돌림과 알림을 캡처 |
| AC8 | 파일을 열면 목록 끝에 활성 파일 탭이 생기고, 같은 파일을 다시 열면 탭 수가 늘지 않으며, 새 herdr 탭도 목록 끝에 생긴다 | machine | - |
| AC9 | herdr 라벨 "2"는 "Tab 2"로, 라벨 "notes"는 "notes"로 보이고, 스트립 위치에서 라벨을 만드는 코드 경로가 남아 있지 않다 | machine | - |
| AC10 | 스트립 높이, 신호등 여백, 컨트롤 간격 값이 뷰 파일에 리터럴로 쓰이지 않고 `HideTheme`에서 온다 | machine | - |
| AC11 | 원격(mini) 컨텍스트의 탭 목록과 순서는 이 변경 전과 같다 | machine | - |

## 8. PRD-Level Tasks

- T1. navigator의 탭 목록을 herdr `tabs` 순서로 만들고 layout은 tab id로 조회하도록 바꾸며, 탭 전환의 0번 이동 조작과 셸의 첫 탭 기본값을 삭제하고 활성 탭을 herdr의 활성 탭 id에서 유도한다. Covers R3, AC4, AC5, SC3. Depends on: none.
- T2. core에 checkout당 통합 탭 목록을 두고 snapshot에 실으며, 셸의 배열 이어 붙이기와 위치 기반 라벨 경로를 삭제하고 ⌘N, ⌥Tab, 닫기 단축키가 이 목록을 따르게 한다. Covers R4, R6, AC6, AC8, AC9, AC11, SC5. Depends on: T1.
- T3. `tab.move`를 계약 스키마와 실제 바이너리에서 확인한 뒤 탭 재정렬 이벤트를 추가하고, herdr 탭 이동은 herdr에 요청해 확정하며 거부와 무응답은 되돌리고 드러낸다. Covers R5, AC7, SC4. Depends on: T2.
- T4. 스트립에 드래그 재정렬 제스처를 붙이고 탭 위 드래그와 빈 영역 드래그를 구분한다. Covers R5, AC3, SC2, SC4. Depends on: T3.
- T5. 터미널 컬럼 헤더를 제거하고 복원 버튼을 스트립 양끝으로 옮기며, 버전 라벨은 삭제하고 연결 경고와 원격 상태가 상태 표시줄에서 읽히는지 확인한다. Covers R1, R7, AC1, AC10, SC1. Depends on: none.
- T6. 시스템 타이틀바를 숨기고 창 제목을 유지하며, 신호등 여백 토큰을 추가해 브랜드 헤더와 스트립이 상태에 따라 여백을 비우게 하고, 빈 영역 드래그로 창이 이동하게 한다. Covers R2, R7, AC2, AC3, AC10, SC1, SC2. Depends on: T5.
- T7. 검증 픽스처를 준비한다: throwaway herdr 워크스페이스에 탭 셋, 파일 탭이 열릴 파일, 존재하지 않는 탭 id로의 이동 거부 경로. Covers SC3, SC4, SC5. Depends on: none.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust core와 Swift 셸의 빌드, 토큰 리터럴 검사 | none |
| automated behavior | yes | 탭 순서 유도, 통합 목록, 라벨, 활성 탭 유도의 회귀 | none |
| app runtime | yes | 크롬 제거, 신호등 여백, 창 이동, 드래그 재정렬 | 최종 시각 판단 |
| live herdr integration | yes | herdr 탭 이동의 왕복과 거부 경로 | 이 PRD에서 승인된 격리 경계 |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R7, AC2, AC10 | Rust core와 Swift 셸이 깨끗이 빌드되고, 창 제목이 유지되며, 스트립과 여백 값이 토큰에서만 오는 것이 정적으로 확인된다 | yes | no |
| V2 | automated behavior | R3, R4, R6, AC4, AC5, AC6, AC8, AC9, AC11 | 회귀 위험을 직접 겨냥한 테스트가 있다: layout 도착 순서가 다시 순서를 정하게 되는 것, 탭 전환이 순서를 바꾸는 것, 활성 탭이 첫 탭으로 기본값 처리되는 것, 파일 탭이 자리를 잃는 것, 라벨이 위치에서 나오는 것, 원격 경로가 바뀌는 것. 각 테스트는 호출자가 보는 snapshot 순서를 단언한다 | yes | no |
| V3 | app runtime | R1, R2, R7, AC1, AC3, SC1, SC2 | 조립된 dev 번들에서 상단 두 줄이 없고, 복원 버튼과 상태 표시줄 정보가 제자리에 있으며, 신호등이 두 사이드바 상태 모두에서 겹치지 않고, 빈 영역 드래그만 창을 움직인다 | yes | no |
| V4 | app runtime | R4, R5, AC6, SC4, SC5 | 실행 중인 앱에서 파일 탭을 herdr 탭 사이로 끌어 놓은 순서가 스트립, ⌘N, ⌥Tab에 동일하게 반영되고, 새 탭이 끝에 붙는다 | yes | no |
| V5 | live herdr integration | R5, AC7, SC3, SC4 | throwaway 워크스페이스에서 herdr 탭을 끌어 옮긴 뒤 CLI가 보고하는 순서가 스트립과 같고, CLI로 옮긴 순서가 스트립에 따라오며, 거부 경로에서 스트립이 herdr 순서로 돌아가고 알림이 보인다 | yes | no |

Live 모드의 부작용 경계:

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked | Allowed Side Effect | Sensitive Data Policy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| V5 | live herdr integration | R5, AC7, SC3, SC4 | 위와 같음 | yes | no | 이 실행이 만든 throwaway herdr 워크스페이스 하나와 그 안의 탭과 pane을 만들고 옮기고 닫는다. 이 실행이 만들지 않은 pane, 탭, 워크스페이스는 닫거나 옮기거나 이름을 바꾸거나 프롬프트를 보내지 않는다 | 캡처에서 checkout 밖의 경로와 토큰을 가린다 |

### 9.3 Human Verification

- 상단 크롬이 사라진 뒤의 시각 균형: 스트립의 높이, 신호등 옆 여백, 상태 표시줄로 옮겨간 라벨의 위치를 `DESIGN.md` 기준으로 판단한다.
- 새 탭을 목록 끝에 두는 규칙이 운영자의 기대와 맞는지 확인한다. 활성 탭 옆에 끼우는 관행을 원하면 규칙 하나를 바꾼다.

## 10. Risks And Open Decisions

- `tab.move`는 hide가 처음 호출하는 herdr 메서드다.
  계약 스키마에는 있으나 실제 0.8.2 바이너리의 파라미터 형태(insert index의 기준, 응답 형태)는 `herdr api schema --json`과 `scripts/check-herdr-contract.sh`로 확인한 뒤 쓴다.
  형태가 다르면 T3에서 보고하고 진행한다.
- 시스템 타이틀바를 숨기면 창 이동 영역을 셸이 직접 정해야 한다.
  드래그 재정렬과 창 이동이 같은 스트립 위에 있으므로 SC2의 구분 규칙이 지켜지는지 V3에서 본다.
- 탭 전환 직후 herdr 확인 이벤트까지 활성 표시가 늦는 구간이 남는다.
  지금은 index-0 이동과 첫 탭 fallback 때문에 활성 표시가 즉시 옮겨가므로, 이 PRD만 적용되면 활성 표시가 `tab_focused` 이벤트(약 170ms)를 기다리는 체감 회귀가 생긴다.
  이는 `hide-view-state` PRD가 없애므로 두 PRD는 바로 이어서 적용한다.
  이 PRD에서는 그 구간에 순서가 흔들리지 않는 것만 보장한다.
- live 검증은 운영자의 herdr 서버에서 돈다.
  V5의 격리 경계가 완화책이고 section 11이 금지로 적는다.
- 스크린샷은 `agents/runs/hide-chrome-and-tabs/` 아래에만 두며 커밋하지 않는다.

## 11. Implementation Guardrails

운영자의 지침과 이 저장소의 규칙에서:

- 이 실행이 만들지 않은 herdr pane, 탭, 워크스페이스를 닫거나 옮기거나 이름을 바꾸거나 프롬프트를 보내지 않는다. 운영자의 실행 중인 Hide 인스턴스와 상호작용하지 않는다.
- section 6을 넘어 범위를 넓히지 않고, section 5를 넘어 구조를 바꾸지 않으며, 서드파티 의존성을 추가하지 않는다.
- 숨겨진 사용자 흐름을 추가하지 않는다. 새 컨트롤은 section 6이 이름 붙인 것뿐이다.
- Herdr 통합은 `AGENTS.md` "Herdr API Contract"를 따른다: 공식 CLI와 Socket API 문서를 읽고, `tab.move`를 `contracts/herdr-api.schema.json`과 `scripts/check-herdr-contract.sh`로 확인하며, 기존 호출부에서 파라미터를 추측하지 않는다.
- 성능은 `AGENTS.md` "Performance Guide"를 따른다: 순서 계산과 목록 구성은 lock 안에서 subprocess나 블로킹 I/O를 부르지 않고, 탭 목록은 snapshot의 revisioned `rest` 섹션에 실린다.
- 증거는 `AGENTS.md` "Evidence Belongs Outside The Repository"를 따른다: 모든 캡처와 로그는 `agents/runs/hide-chrome-and-tabs/`에 두고 커밋하지 않는다.
- 디자인은 `AGENTS.md` "Design Reference"와 `DESIGN.md`를 따른다: 새 높이, 여백, 색은 `HideTheme`에 추가해 쓰고 뷰에 리터럴을 쓰지 않는다.
- engineering/principles.md 규칙 1: 이 변경이 쓸모없게 만드는 것을 같은 변경에서 지운다. 헤더 뷰, 셸의 배열 이어 붙이기, 위치 기반 라벨 폴백, 탭 전환의 0번 이동 조작, 첫 탭 기본값이 모두 함께 사라지며 호환 경로를 남기지 않는다.
- engineering/principles.md 규칙 2: 요구를 충족하는 가장 단순한 구현을 고른다. 브라우저 변형, 순서 저장소, 일반화된 탭 종류 레지스트리를 만들지 않는다.
- engineering/principles.md 규칙 3: 층으로 키운다. 순서 수정(T1)이 먼저 끝나고 통합 목록(T2)이 그 위에, 재정렬(T3, T4)이 그 위에 얹힌다.
- engineering/principles.md 규칙 4: 실패를 드러낸다. 활성 탭 id가 목록에 없으면 첫 탭으로 대체하지 않고, herdr의 이동 거부는 조용히 무시되지 않는다.
- engineering/principles.md 규칙 10: herdr 이동 거부와 무응답은 사용자에게 보이는 알림과 구조화된 진단으로 프로세스 밖에서 관찰된다.
- engineering/principles.md 규칙 5, 7: 순서 유도는 core의 projection에, 렌더링은 셸에 머물고, 셸의 기존 MRU와 단축키 번호 매기기 코드를 새 목록에 맞춰 확장한다.
- engineering/principles.md 규칙 8: herdr 탭 순서의 권위를 herdr에 두는 것은 장기 결정이다. 임시로 hide가 별도 순서를 갖지 않는다.
- engineering/principles.md 규칙 12, 13: 테스트는 호출자가 보는 snapshot 순서를 단언하고, 순서 결함은 도착 순서 의존이라는 부류를 없애는 방식으로 고친다.
- design/principles.md 규칙 3: 가장 잦은 동작이 가장 적은 클릭으로 된다. 탭 전환과 재정렬은 한 동작이다.
- design/principles.md 규칙 4: 유도 상태를 보여준다. 연결 경고는 상태 표시줄에서, 버전은 브랜드 헤더에서 헤더가 사라진 뒤에도 읽힌다.
- design/principles.md 규칙 5: 기존 패턴을 따른다. 복원 버튼은 기존 툴바 버튼 스타일을, 스트립은 기존 탭 항목 구성을 유지한다.
- design/principles.md 규칙 7: 상태는 시각적으로 부호화한다. 이동 거부 알림은 기존 알림 표면을 쓰고 설명 문단을 추가하지 않는다.
- Git과 PR 귀속: 브랜치 이름, 커밋 메시지, 트레일러, 생성 텍스트 어디에도 에이전트, 모델, 벤더, 도구 이름을 쓰지 않는다.

## 12. Implementation Result Report Contract

보고 항목:

- status: `Done`, `Partially Done`, `Blocked`.
- 사용자에게 보이는 변화를 세 문제(크롬 두 줄, 순서 뒤집힘, 섞인 탭) 각각에 대해.
- 바뀐 모듈과 새 모듈의 책임 경계, 실제로 고른 파일 구조.
- section 5의 구조를 따랐는지, 벗어난 곳과 이유.
- T1부터 T7까지의 완료 상태와 R/AC/V 커버리지.
- 모드별 검증 증거와 각 산출물이 있는 실행 디렉터리.
- T3에 대해: 실제 바이너리에서 확인한 `tab.move`의 파라미터와 응답 형태, 계약 스키마와 다른 점.
- V5에 대해: 이 실행이 만든 herdr 워크스페이스, 탭, pane과 그 전부가 닫혔다는 확인, 그 밖의 어떤 것도 건드리지 않았다는 확인.
- 추가되거나 바뀐 자동 테스트와 각각이 막는 회귀.
- 삭제된 코드 경로의 목록.
- 이탈, 남은 인간 검토, 미완 항목과 후속 후보.
