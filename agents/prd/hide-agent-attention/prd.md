---
topic: "Hide agent attention: one state model, pane-level read state, consistent Needs You and agent rows"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "Changes how the shell classifies and orders agents and adds a persisted per-pane read record; the only external effect is reporting synthetic lifecycle states on panes inside a throwaway workspace, with no data, credential, billing, or destructive action."
source_intake: "current conversation"
created_at: "2026-09-04"
updated_at: "2026-09-04"
---

# PRD: Hide agent attention

## 1. Summary

에이전트의 상태를 한 모델로 분류하고, "읽음"을 hide가 pane 단위로 소유하며, Needs You와 Agents 뷰의 행이 같은 모델과 같은 순서를 쓰게 한다.

지금은 여덟 개의 평면 상태 문자열(question, approval, blocked, error, working, unseen_completion, idle, unknown)이 "무엇을 요구하는가", "작업 중인가", "내가 봤는가"를 한 축에 섞어 놓았고, 그 어휘가 core와 Swift에 다섯 군데 서로 다른 정의로 복제되어 있다.
그리고 "봤다"의 판정은 herdr가 탭 단위로 내린다.
herdr 문서가 명시하듯 탭을 포커스하면 그 탭의 모든 pane이 seen이 되므로, 한 탭에 pane이 여섯 개면 Needs You 항목 셋이 한 번의 클릭에 함께 사라진다.

이 PRD 뒤에는 상태가 세 축(요구, 활동, 읽음)으로 나뉘고, 읽음은 "그 pane에 키보드 포커스가 간 시각이 마지막 상태 변화보다 뒤인가"로 hide가 판정하며, 그 판정은 재시작을 넘긴다.
Needs You에는 안 읽은 질문/승인/오류와 승인 프롬프트가 떠 있는 pane만 들어가고, 안 읽은 완료는 별도 Done 그룹으로 내려간다.
정렬은 core의 함수 하나가 정하고, 사이드바의 두 뷰, pet 배지, 에이전트 전환기가 모두 그 결과를 쓴다.

Approval checklist:

- 세 축 상태 모델과 Needs You, Done, Working, Seen 네 그룹의 정의 (R1, R3).
- 읽음 판정을 herdr의 탭 단위 seen에서 hide의 pane 단위 포커스 기록으로 옮기는 구조 변경과, 그에 따라 기존 규칙 `INV-herdr-unseen-token`의 문구를 갱신하는 결정 (section 5, section 4.2).
- 읽음 기록을 ui_state에 pane 단위로 영속하는 결정 (section 5).
- 라벨 플러그인의 `sort_rank` 토큰에 대한 의존을 끊고 hide가 순서를 계산하는 결정 (R4).
- 검증 모드: build/static, automated behavior, app runtime, 그리고 throwaway 워크스페이스에 한정된 live herdr integration 하나 (section 9.1).
- 배포 모드: local. 실행 브랜치에 커밋 하나, push와 PR 없음 (section 4.3).
- `review_profile: standard`와 그 근거 (frontmatter).

## 2. Problem, Goal, And Users

사용자는 Hide의 단일 운영자다.
여러 코딩 에이전트를 동시에 돌리며, 사이드바의 Needs You와 Agents 뷰로 "지금 누가 나를 기다리는가"를 판단한다.

보고된 문제는 둘이다.

- Needs You와 Agents 뷰의 항목이 일관되지 않다.
  "unseen completion"처럼 상태 이름이 그대로 노출되고, 질문(?)과 승인(!)처럼 사람이 답해야 하는 것, 작업 중인 것, 끝났는데 아직 안 본 것, 이미 본 것이 한 줄에 섞여 있어 인지적으로 어떤 상태인지 바로 읽히지 않는다.
  라벨 플러그인(`herdr-agent-context-labels`)은 `? ! × ● ○ ~` 기호와 안 읽음/읽음 토큰 쌍으로 이 구분을 이미 정의하고 있는데, hide는 그 어휘를 일부만 쓰고 그나마도 다섯 군데에 다르게 복제했다.
  core의 pet 모듈은 완료를 attention에 넣지 않고, Swift의 그룹핑은 넣는다.
- Needs You 항목 셋이 있을 때 하나를 눌렀더니 셋 다 사라졌다.
  herdr의 seen은 탭 단위다.
  문서 원문: "idle means ready for input after its tab has been seen in the focused Herdr UI; done is the same underlying idle state after unseen background work completes. Focusing that tab or targeting it with pane focus / agent focus marks it seen."
  한 탭에 여러 pane이 있으면 하나를 포커스하는 순간 나머지도 seen이 되고, 라벨 플러그인의 `_new` 토큰도 함께 떨어진다.
  hide는 지금 클릭에 대해 아무 읽음 기록도 하지 않고 herdr의 판정을 그대로 쓴다.

목표는 사용자가 사이드바를 한 번 훑어 "답해야 할 것, 확인할 것, 돌아가는 것, 끝난 것"을 순서대로 읽을 수 있고, 항목이 사라지는 시점이 정확히 "내가 그 pane을 봤을 때"인 것이다.

### 2.1 User Scenarios

- SC1. 한 탭의 완료 셋: 운영자가 한 탭에 나란히 둔 pane 셋에서 에이전트가 차례로 끝난다.
  Actors: 운영자.
  Primary path: Done 그룹에 셋이 쌓이고, 운영자가 첫 항목을 누르면 그 pane에 포커스가 가며 그 항목만 Done에서 빠진다. 나머지 둘은 같은 탭에 보이지만 각각 포커스가 갈 때까지 남는다.
  Failure state: herdr가 탭 단위로 seen을 보고해도 hide의 그룹은 흔들리지 않는다. 항목이 사라지는 유일한 경로는 그 pane의 포커스다.
  Recovery: 포커스가 갔던 pane의 에이전트가 다시 끝나면 항목은 다시 Done에 올라온다.
  Reach: throwaway 워크스페이스의 한 탭에 pane 셋을 두고 herdr CLI로 각 pane의 생명주기 상태를 보고한다.

- SC2. 보고 있는 pane의 질문: 운영자가 포커스한 pane의 에이전트가 질문을 던진다.
  Actors: 운영자.
  Primary path: 그 pane은 이미 포커스 상태이므로 항목은 안 읽음으로 올라오지 않고 Agents 뷰에서 읽은 질문(수그러든 `?`)으로 보인다. 운영자가 다른 pane으로 옮긴 뒤 그 에이전트가 새 질문을 던지면 그때는 Needs You에 밝은 `?`로 올라온다.
  Failure state: herdr가 승인 프롬프트를 인식해 blocked로 보고하는 pane은 읽음 여부와 무관하게 프롬프트가 떠 있는 동안 Needs You에 남는다.
  Recovery: 승인이 처리되어 herdr의 blocked가 풀리면 Needs You에서 빠진다.
  Reach: throwaway 워크스페이스의 pane에 herdr CLI로 질문 토큰과 blocked 상태를 보고한다.

- SC3. 재시작: 운영자가 Hide를 껐다 켠다.
  Actors: 운영자.
  Primary path: 끄기 전에 읽은 항목은 켠 뒤에도 읽음이고, 끄기 전에 안 읽은 항목은 켠 뒤에도 Needs You나 Done에 그대로 있다. 켜져 있지 않은 동안 상태가 바뀐 pane은 안 읽음으로 올라온다.
  Failure state: 읽음 기록 파일이 손상되면 기록은 비워지고 그 사실이 진단으로 남으며, 앱은 모든 항목을 안 읽음으로 보여 준다. 조용히 읽음으로 처리하지 않는다.
  Recovery: 사라진 pane의 기록은 다음 저장 때 정리되어 파일이 자라지 않는다.
  Reach: 안 읽은 항목 하나와 읽은 항목 하나를 만든 뒤 셸을 재시작한다.

- SC4. 두 뷰의 일관성: 운영자가 Projects 뷰와 Agents 뷰를 번갈아 본다.
  Actors: 운영자.
  Primary path: Projects 뷰 상단의 Needs You와 Done, Agents 뷰의 전체 목록, pet 대시보드의 행이 같은 에이전트에 대해 같은 기호, 같은 색, 같은 짧은 상태 낱말을 보여주고, 같은 순서 규칙을 따른다. Agents 뷰에서는 네 그룹의 경계가 눈에 보인다.
  Failure state: 어떤 뷰도 상태 이름을 밑줄 문자열 그대로 노출하지 않는다.
  Recovery: 한 항목의 상태가 바뀌면 모든 뷰에서 같은 틱에 같은 그룹으로 옮겨 간다.
  Reach: 네 그룹에 하나씩 이상 들어가도록 상태를 보고한 throwaway 워크스페이스.

- SC5. pet 배지: 운영자가 메뉴바 pet의 배지 수를 본다.
  Actors: 운영자.
  Primary path: pet의 "act now" 수는 Needs You의 항목 수와 같고, done 수는 Done 그룹의 수와 같다.
  Failure state: 연결이 끊기면 pet은 기존대로 disconnected를 보이고 수를 세지 않는다.
  Recovery: 재연결 뒤 수가 사이드바와 다시 일치한다.
  Reach: SC4와 같은 워크스페이스.

## 3. Scope And Non-Goals

범위: 세 축 상태 모델, pane 단위 읽음 기록과 영속, 네 그룹 정렬, Needs You와 Done 섹션, Agents 뷰의 그룹 경계, 하나의 에이전트 행 컴포넌트, pet 배지의 같은 그룹 사용, 다섯 군데 복제된 어휘의 삭제, 그리고 규칙 `INV-herdr-unseen-token` 문구의 갱신.

비목표, 각각 의도된 제외:

- 라벨 플러그인 자체의 변경.
  플러그인의 토큰(`status_*`, `summary`, `elapsed`, `activity`)은 입력으로 그대로 쓰고, `sort_rank`만 더 이상 읽지 않는다.
  Consequence: herdr 자체 사이드바의 순서와 hide의 순서가 다를 수 있다. 운영자는 hide만 쓴다.
  Revisit: herdr TUI를 병행하게 될 때.
- hide의 읽음 판정을 herdr에 되돌려 보내기.
  herdr에 pane 단위 seen을 기록할 API가 없다.
  Consequence: herdr의 `done`/`idle`은 hide의 읽음과 독립적으로 움직인다.
  Revisit: herdr가 pane 단위 acknowledgement를 제공할 때.
- `pane.agent_status_changed` 이벤트 구독으로의 전환.
  이 이벤트는 pane마다 따로 구독해야 하고 토큰을 싣지 않으므로 지금의 1초 `agent.list` 폴링을 대체하지 못한다.
  Consequence: 상태 변화가 화면에 오기까지 최대 1초의 지연은 그대로다.
  Revisit: 반응성 PRD `hide-view-state` 또는 herdr가 전역 구독을 제공할 때.
- 읽음을 "화면에 보임"으로 판정하기.
  사용자가 포커스 기준을 선택했다.
  Consequence: 탭을 열어 놓기만 하고 포커스하지 않은 pane은 안 읽음으로 남는다.
  Revisit: 사용자가 기준 변경을 요청할 때.
- 에이전트 행의 디자인 시스템 편입(툴팁, 키캡, 컴포넌트 모듈 분리).
  별도 디자인 시스템 PRD의 범위다. 이 PRD는 행 하나로 통일하되 기존 토큰과 스타일 안에서 한다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
필요한 상태는 herdr CLI의 `pane report-agent`와 `pane report-metadata`로 throwaway pane에 만들 수 있다.

### 4.2 Human Decisions Before PRD Approval

- 규칙 `INV-herdr-unseen-token`의 문구 갱신을 승인한다.
  현재 문구는 "acknowledged ones must fall through to idle and disappear from the dashboard list, matching what the herdr sidebar shows"이며, 읽음의 권위가 hide로 옮겨오면 herdr 사이드바와의 일치는 더 이상 규칙이 아니다.
  갱신 후 규칙은 "안 읽음 여부는 hide의 pane 단위 기록만이 정하고, herdr의 seen 토큰 형태는 요구 축(질문/승인/오류)의 입력으로만 쓴다"가 된다.
  규칙의 트리거 경로도 존재하지 않는 `crates/` 경로에서 현재 경로로 바로잡는다.
  규칙은 `rules add`로만 다루므로 이 갱신은 T7의 작업이며 사람이 승인해야 한다.
- 승인 프롬프트가 떠 있는 pane(herdr `blocked`)은 읽음과 무관하게 Needs You에 남는 규칙을 승인한다.
  대화의 선택 문구는 "Needs You = 안 읽은 질문/승인/오류 + blocked"였고 이 PRD는 그것을 그대로 옮겼다.

### 4.3 Decision Traceability For Fidelity Review

이 PRD는 인터뷰 qa-log 없이 대화만을 근거로 하므로 사용자의 결정을 여기에 그대로 남긴다.

- 사용자 요청 원문 (2026-09-03): "Needs You나 Agent Item보면 다 일관성있게 보여져야 할 것 같은데... 인지적으로 보기 편하게 내게 질문해서 확인해야하는건 ?, ! 이게 있고 그담에 작업중인거랑 완료됐는데 내가 아직 확인 안한거, 이미 확인한거 이런 식의 분류가 좀 필요해". R1, R3, R5, SC4.
- 사용자 요청 원문: "그래야 사람에게 일관성있는 순서로 보여줄 수있고 + 인지적으로 어떤상태인지 바로 이해하기 쉬울 것 같아 (Needs You 에는 뭐가들어가야할까 이런상태에서~)". R3, R4.
- 사용자 참조: "~/projects/herdr-label-... 이거랑 herdr의 config를 보면 좀더 이해하기 쉬울듯?". 라벨 플러그인 README의 기호 표와 안 읽음/읽음 토큰 쌍, herdr config의 `$status_*` 토큰 색을 읽었고 R2, R5의 기호 어휘가 그것을 따른다.
- 사용자 보고 원문: "3개정도 Needs You 가 있었는데 그거 하나 눌렀는데 다 사라지더라고? 합리적 의심은.. 그 탭에 보여지기만하면 사라지는건가 싶기는 했었는데". herdr 문서에서 탭 단위 seen으로 확인. R2, SC1.
- 사용자 선택 (2026-09-04, 질문 "Needs You 구성과 '읽음' 기준"): "제안대로". 옵션 설명은 "Needs You = 안 읽은 질문/승인/오류 + blocked. 안 읽은 완료는 별도 'Done' 그룹. 읽음 = 그 pane에 키보드 포커스가 간 시각이 마지막 상태 변경보다 뒤". R2, R3 그대로. 거부된 대안 둘: 완료를 Needs You에 유지, 읽음을 "화면에 보임"으로 판정. 후자는 비목표로 기록.
- 사용자 선택 (2026-09-04): "hide만 쓴다". herdr 사이드바와의 순서 일치는 더 이상 요구가 아니다. 비목표와 4.2의 규칙 갱신.
- 사용자 선택 (2026-09-04): "3개로 분할". 이 PRD는 항목 6과 7을 담는다.
- 에이전트 제안, 사용자가 수용한 것 (2026-09-03 논의): 정렬 함수 하나를 core에 두고 Needs You와 Agents 뷰가 같은 함수를 쓴다; 행 컴포넌트를 하나로 통일한다; 기호와 색을 플러그인의 `? ! × ● ○`와 맞춘다. R4, R5.
- 에이전트 가정 (사용자 결정 아님): herdr의 `blocked` 생명주기는 요구 축의 "승인"으로 사상한다. 플러그인 README가 `!`의 출처를 "Herdr's blocked lifecycle, or the hook seeing a permission request"로 정의하기 때문이다. R1.
- 에이전트 가정 (사용자 결정 아님): 상태 변화의 정의는 herdr의 `state_change_seq`가 오르거나, 유도된 (요구, 활동) 쌍이 바뀌는 것이다. 플러그인 토큰만 바뀌고 herdr 시퀀스가 오르지 않는 경우를 놓치지 않기 위해서다. R2.
- 에이전트 가정 (사용자 결정 아님): 읽음 기록은 ui_state에 pane id로 영속한다. hide는 dev 빌드로 자주 재시작되므로 메모리에만 두면 재시작마다 모든 항목이 안 읽음으로 돌아온다. R2, SC3.
- 배포 모드: `agents/config.json`의 `delivery.mode: local`, `worktree.enabled: true`를 그대로 따른다.
- 원칙 인테이크: `~/projects/oh-my-principle` 커밋 `35ab76ca23d45e714f1630054855a8c8c4568d03`에서 `engineering/principles.md`와 `design/principles.md`를 전문으로 읽었다. 적용 규칙은 section 11에 번역했다. engineering 규칙 6은 플러그인 README의 기호 체계를 설계 참조로 채택한 것으로 충족되어 별도 guardrail로 두지 않았고, design 규칙 1, 2, 6은 목록의 형태 변경이나 파괴적 동작이 없어 번역하지 않았다.
- 프로젝트 규칙 인테이크: `AGENTS.md` "Performance Guide"(읽음 판정과 정렬은 lock 안에서 subprocess를 부르지 않고 매 틱 재계산이 커지지 않게), "Harness Namespace"(규칙은 `rules add`로만), "Evidence Belongs Outside The Repository", "Design Reference"를 section 11에 번역했다. `docs/status-model.md`의 pet 포즈 우선순위는 그룹 정의와 충돌하지 않으며 R6이 유지한다.

## 5. Major Technical Structure Changes

- 에이전트 projection의 상태가 평면 문자열 하나에서 세 축으로 바뀐다.
  요구: 질문, 승인, 오류, 없음.
  활동: 작업 중, 멈춤, 알 수 없음.
  읽음: 안 읽음, 읽음.
  core는 세 축과 함께 유도값(그룹, 기호, 강조 여부, 닫기 확인 필요 여부)을 snapshot에 싣고, 셸은 유도값을 그리기만 한다.
  기존 `state` 문자열과 그것을 해석하던 다섯 군데의 어휘 사본(core의 pet 버킷과 닫기 확인, Swift의 그룹핑 집합, pane 헤더의 리터럴 배열, pet 대시보드의 버킷)은 삭제된다.
- 읽음 기록이 core의 새 책임이 된다.
  pane마다 마지막으로 확인한 (herdr 시퀀스, 요구, 활동)을 기억하고, 그 pane이 hide의 포커스 pane인 동안 매 갱신마다 현재 값으로 올린다.
  기록은 ui_state에 pane id 키의 맵으로 영속되며 스키마 버전은 올리지 않고 기본값으로 로드된다.
  사라진 pane의 항목은 저장 때 정리되며, 같은 방식으로 기존 `pane_text_scales`도 정리된다.
- 순서 계산이 라벨 플러그인의 `sort_rank` 토큰에서 core의 함수로 옮겨온다.
  네 그룹 순서 뒤에 최근 활동 내림차순, 그 뒤에 snapshot 순서다.
  `sort_rank` 읽기와 그 검증은 삭제된다.
- 규칙 `INV-herdr-unseen-token`의 본문과 트리거 경로가 `rules` CLI를 통해 갱신된다.
- 스키마 버전, 저장소 위치, 인증, 결제, 배포 변경 없음. 새 서드파티 의존성 없음.

## 6. Requirements

- R1. core는 모든 에이전트를 세 축으로 분류한다.
  요구 축은 라벨 플러그인의 오류, 질문, 승인 토큰(안 읽음 형태와 읽음 형태 모두)과 herdr의 `blocked` 생명주기에서 오고, blocked는 승인이다.
  활동 축은 herdr의 `working`과 플러그인의 작업 토큰이면 작업 중, `idle`과 `done`이면 멈춤, 그 밖이면 알 수 없음이다.
  herdr의 `done`과 `idle`의 구분, 그리고 토큰의 `_new` 접미사는 읽음 축에 쓰이지 않는다.
- R2. 읽음 축은 hide가 pane 단위로 판정한다.
  pane은 hide의 포커스 pane이 되는 순간, 그리고 포커스 pane인 동안의 매 갱신에 현재 상태를 읽은 것으로 기록한다.
  상태 변화는 herdr의 `state_change_seq`가 오르거나 유도된 (요구, 활동) 쌍이 바뀌는 것이며, 기록보다 뒤의 변화가 있으면 안 읽음이다.
  기록은 ui_state에 영속되어 재시작을 넘기고, 손상되면 비워지며 그 사실이 진단으로 남고, 사라진 pane의 항목은 정리된다.
  읽음 기록의 변화는 pane id와 시퀀스를 담은 구조화된 진단으로 남는다.
- R3. 그룹은 넷이며 이 순서다.
  Needs You: 안 읽은 요구(질문, 승인, 오류)가 있는 에이전트와 herdr가 지금 blocked로 보고하는 에이전트.
  Done: 요구 없이 멈췄고 안 읽은 에이전트.
  Working: 작업 중인 에이전트.
  Seen: 나머지 전부(읽은 요구, 읽은 완료, 알 수 없음).
  Projects 뷰에는 Needs You와 Done이 비어 있지 않을 때만 상단에 섹션으로 나타나고, 두 섹션에 든 에이전트는 아래 프로젝트 트리에서 중복되지 않는다.
  Agents 뷰는 네 그룹의 경계를 섹션 라벨로 보이며 빈 그룹은 생략한다.
- R4. 정렬은 core의 함수 하나가 정한다.
  그룹 순서, 그 안에서 최근 활동 내림차순(플러그인 `activity` 토큰, 없으면 herdr 시퀀스), 그 뒤 snapshot 순서다.
  Needs You, Done, Agents 뷰, pet 대시보드, 에이전트 전환기의 후보 순서가 모두 이 결과를 쓴다.
  `sort_rank` 토큰은 읽지 않는다.
- R5. 에이전트 행은 컴포넌트 하나다.
  기호는 질문 `?`, 승인 `!`, 오류 `×`, 작업 중 `●`, 안 읽은 완료 `●`, 읽음 `○`, 알 수 없음 `~`이며 색은 기존 토큰(경고, 위험, 강조, 성공, 보조)에서 온다.
  안 읽은 요구는 밝게, 읽은 요구는 수그러든 색으로 같은 기호를 유지한다.
  상태 낱말은 짧은 사람 말(Question, Approval, Error, Working, Done, Idle, Unknown)이며 어떤 뷰도 밑줄 문자열을 그대로 노출하지 않는다.
  같은 행이 Needs You, Done, Agents 뷰, 프로젝트 트리 안의 checkout 그룹, pet 대시보드에 쓰인다.
- R6. pet 배지의 "act now" 수는 Needs You의 수, done 수는 Done의 수이며, pet 포즈 우선순위와 disconnected 처리는 바뀌지 않는다.
- R7. pane 닫기 확인이 필요한지는 core가 유도값으로 내리며, 작업 중이거나 Needs You 또는 Done에 든 pane이 대상이다.
  셸의 리터럴 상태 배열은 삭제된다.
- R8. 규칙 `INV-herdr-unseen-token`은 `rules` CLI를 통해 본문이 hide의 pane 단위 읽음 권위를 말하도록, 트리거 경로가 현재 core 경로를 가리키도록 갱신된다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | 안 읽음 형태와 읽음 형태의 질문/승인/오류 토큰, herdr의 blocked/working/done/idle/unknown 각각이 정의된 (요구, 활동) 쌍으로 분류되고, blocked는 승인이며, herdr의 done과 idle은 같은 활동값을 낸다 | machine | - |
| AC2 | 한 탭의 pane 셋이 차례로 완료 상태가 되었을 때 셋 모두 안 읽음이고, 그중 하나에 포커스가 가면 그 하나만 읽음이 되며, herdr가 그 탭의 나머지를 seen으로 보고해도 나머지 둘은 안 읽음으로 남는다 | machine | - |
| AC3 | 포커스된 pane의 에이전트가 새 요구를 내면 그 요구는 읽음으로 기록되고, 포커스가 다른 pane으로 옮겨진 뒤의 새 요구는 안 읽음이 된다 | machine | - |
| AC4 | 읽음 기록은 재시작 뒤에도 같고, 손상된 기록 파일은 빈 기록과 진단으로 로드되며, 사라진 pane의 항목은 저장 뒤 파일에 없다 | machine | - |
| AC5 | 네 그룹의 소속이 정의대로이고(안 읽은 요구와 blocked는 Needs You, 요구 없는 안 읽은 멈춤은 Done, 작업 중은 Working, 나머지는 Seen), 그룹 안에서는 최근 활동 내림차순, 그 뒤 snapshot 순서이며, `sort_rank` 토큰 값은 순서에 영향을 주지 않는다 | machine | - |
| AC6 | Projects 뷰 상단의 Needs You와 Done 섹션, Agents 뷰의 네 그룹, pet 대시보드가 같은 에이전트에 같은 기호와 색과 상태 낱말을 보여주고, 어떤 뷰에도 밑줄이 든 상태 문자열이 보이지 않으며, Needs You나 Done에 든 에이전트는 프로젝트 트리에 중복되지 않는다 | judged | 네 그룹에 하나씩 이상 든 throwaway 워크스페이스에서 Projects 뷰, Agents 뷰, pet 대시보드를 캡처해 같은 에이전트 행을 나란히 비교 |
| AC7 | Done 그룹의 항목 셋 중 하나를 누르면 그 pane에 포커스가 가고 그 항목만 Done에서 빠지며, 나머지 둘은 같은 탭에 보이는 채로 남는다 | judged | throwaway 워크스페이스에서 한 탭의 pane 셋에 완료를 보고한 뒤 첫 항목을 누르고, 누르기 전과 후의 사이드바와 pane 포커스를 캡처 |
| AC8 | herdr가 blocked로 보고하는 pane은 포커스가 다녀간 뒤에도 blocked가 풀릴 때까지 Needs You에 남는다 | judged | throwaway pane에 blocked를 보고하고, 포커스를 주었다 뺀 뒤, blocked를 idle로 바꾸기 전후의 Needs You 캡처 |
| AC9 | pet 배지의 act now 수와 done 수가 각각 Needs You와 Done의 항목 수와 같고, 연결이 끊기면 수를 세지 않는다 | machine | - |
| AC10 | pane 닫기 확인은 작업 중이거나 Needs You 또는 Done에 든 pane에만 요구되고, 셸에 상태 문자열 리터럴 배열이 남아 있지 않다 | machine | - |
| AC11 | `agents/rules/invariants/INV-herdr-unseen-token.md`의 본문이 hide의 pane 단위 읽음 권위를 말하고 트리거 경로가 현재 core 경로를 가리키며, 규칙 원장이 `rules` CLI로 갱신되어 있다 | machine | - |

## 8. PRD-Level Tasks

- T1. core의 에이전트 projection을 세 축과 유도값으로 바꾸고, 평면 상태 문자열과 다섯 군데 어휘 사본을 삭제한다. Covers R1, R7, AC1, AC10. Depends on: none.
- T2. pane 단위 읽음 기록을 core에 두고 포커스 pane의 갱신마다 올리며, ui_state에 기본값 로드로 영속하고, 사라진 pane 항목을 정리하며, 변화를 구조화된 진단으로 남긴다. Covers R2, AC2, AC3, AC4, SC1, SC2, SC3. Depends on: T1.
- T3. 네 그룹과 정렬 함수를 core에 두고 `sort_rank` 읽기를 삭제하며, Needs You, Done, Agents 뷰의 그룹 경계, 에이전트 전환기 후보가 그 결과를 쓰게 한다. Covers R3, R4, AC5, SC4. Depends on: T1.
- T4. 에이전트 행을 컴포넌트 하나로 통일해 기호, 색, 상태 낱말을 core의 유도값에서 그리고, 다섯 표면이 그 행을 쓰게 한다. Covers R5, AC6, AC7, AC8, SC1, SC2, SC4. Depends on: T3.
- T5. pet 배지와 대시보드가 core의 그룹을 쓰게 하고 pet 모듈의 자체 버킷을 삭제한다. Covers R6, AC9, SC5. Depends on: T3.
- T6. 검증 픽스처를 준비한다: throwaway herdr 워크스페이스에 한 탭 pane 셋과 별도 pane, 네 그룹을 채우는 상태 보고 스크립트, blocked 전환 스크립트, 재시작 전후 비교 절차. Covers SC1, SC2, SC3, SC4, SC5. Depends on: none.
- T7. `rules` CLI로 `INV-herdr-unseen-token`의 본문과 트리거 경로를 갱신한다. Covers R8, AC11. Depends on: T2.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust core와 Swift 셸의 빌드, 셸의 상태 문자열 리터럴 부재 검사 | none |
| automated behavior | yes | 분류, 읽음 판정, 영속, 정렬, pet 수의 회귀 | none |
| app runtime | yes | 세 표면의 일관성과 클릭 시 항목 소멸 | 최종 시각 판단 |
| live herdr integration | yes | herdr CLI로 보고한 상태가 hide의 그룹과 읽음에 반영되는 흐름 | 이 PRD에서 승인된 격리 경계 |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R8, AC10, AC11 | Rust core와 Swift 셸이 깨끗이 빌드되고, 셸 소스에 에이전트 상태 문자열 리터럴 배열이 남아 있지 않으며, 규칙 파일의 본문과 트리거 경로와 원장 행이 갱신되어 있는 것이 정적으로 확인된다 | yes | no |
| V2 | automated behavior | R1, R2, R3, R4, R6, R7, AC1, AC2, AC3, AC4, AC5, AC9 | 회귀 위험을 직접 겨냥한 테스트가 있다: herdr의 탭 단위 seen이 다시 hide의 읽음을 정하게 되는 것, 포커스 중 발생한 요구가 안 읽음으로 올라오는 것, 재시작이 읽음을 잃는 것, 손상 파일이 조용히 읽음으로 처리되는 것, `sort_rank`가 다시 순서를 정하는 것, pet 수가 사이드바와 어긋나는 것. 각 테스트는 snapshot에 실린 그룹, 읽음, 순서를 단언한다 | yes | no |
| V3 | app runtime | R3, R5, R6, AC6, AC7, AC8, SC1, SC2, SC4, SC5 | 조립된 dev 번들에서 세 표면이 같은 행을 보이고, Done 항목 클릭이 그 항목만 지우며, blocked pane이 포커스 뒤에도 남고, pet 배지 수가 사이드바와 같다 | yes | no |
| V4 | live herdr integration | R1, R2, AC2, AC3, SC1, SC2, SC3 | throwaway 워크스페이스의 pane에 herdr CLI로 보고한 생명주기와 토큰이 hide의 분류와 읽음에 반영되고, 셸 재시작 뒤에도 읽음이 유지되며, 이 실행이 만든 것만 건드린다 | yes | no |

Live 모드의 부작용 경계:

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked | Allowed Side Effect | Sensitive Data Policy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| V4 | live herdr integration | R1, R2, AC2, AC3, SC1, SC2, SC3 | 위와 같음 | yes | no | 이 실행이 만든 throwaway herdr 워크스페이스 하나와 그 안의 탭과 pane을 만들고, 그 pane에만 생명주기 상태와 메타데이터 토큰을 보고하며, 끝나면 닫는다. 실제 에이전트 세션을 시작하지 않는다. 이 실행이 만들지 않은 pane, 탭, 워크스페이스는 닫거나 옮기거나 상태를 보고하거나 프롬프트를 보내지 않는다 | 캡처에서 checkout 밖의 경로와 토큰을 가린다. 합성 상태 외의 에이전트 대화 내용을 담지 않는다 |

### 9.3 Human Verification

- 네 그룹의 이름(Needs You, Done, Working, Seen)과 상태 낱말이 운영자에게 자연스러운지, 그리고 밝은 기호와 수그러든 기호의 대비가 충분한지 `DESIGN.md` 기준으로 판단한다.
- 규칙 `INV-herdr-unseen-token` 갱신 문구의 승인 (4.2).

## 10. Risks And Open Decisions

- herdr의 `state_change_seq`가 플러그인 토큰 변화에 오르지 않을 수 있다.
  R2가 (요구, 활동) 쌍의 변화도 상태 변화로 세는 이유이며, T2에서 실제 바이너리로 확인해 결과를 보고한다.
- pane id는 herdr 서버 재시작을 넘겨 안정적이라는 보장이 없다.
  읽음 기록이 pane id 키이므로 서버 재시작 뒤 항목이 안 읽음으로 돌아올 수 있다.
  hide 재시작(잦음)과 herdr 서버 재시작(드묾)을 구분해 후자는 받아들인다.
- `INV-herdr-unseen-token`은 `rules` CLI로만 갱신해야 하며 CLI가 갱신을 지원하지 않으면 T7에서 보고하고 사람의 지시를 기다린다.
- 읽음의 기준이 되는 "hide의 포커스 pane"은 지금 herdr의 `pane_focused` 이벤트에서 온다.
  `hide-view-state` PRD가 이를 hide 소유로 바꾸면 R2의 판정 시점이 즉시로 당겨질 뿐 규칙은 같다.
- live 검증은 운영자의 herdr 서버에서 돈다. V4의 격리 경계가 완화책이고 section 11이 금지로 적는다.
- 스크린샷은 `agents/runs/hide-agent-attention/` 아래에만 두며 커밋하지 않는다.

## 11. Implementation Guardrails

운영자의 지침과 이 저장소의 규칙에서:

- 이 실행이 만들지 않은 herdr pane, 탭, 워크스페이스에 상태를 보고하거나 닫거나 옮기거나 프롬프트를 보내지 않는다. 실제 에이전트 세션을 시작하지 않는다. 운영자의 실행 중인 Hide 인스턴스와 상호작용하지 않는다.
- section 6을 넘어 범위를 넓히지 않고, section 5를 넘어 구조를 바꾸지 않으며, 서드파티 의존성을 추가하지 않는다.
- 숨겨진 사용자 흐름을 추가하지 않는다.
- 규칙 원장과 규칙 파일은 손으로 편집하지 않고 `rules` CLI로만 다룬다 (`AGENTS.md` "Harness Namespace").
- 성능은 `AGENTS.md` "Performance Guide"를 따른다: 분류, 읽음 판정, 정렬은 lock 안에서 subprocess나 블로킹 I/O를 부르지 않으며, 읽음 기록 저장은 기존 ui_state 저장 경로를 쓰고 매 틱 디스크 쓰기를 추가하지 않는다.
- 증거는 `AGENTS.md` "Evidence Belongs Outside The Repository"를 따른다: 모든 캡처와 로그는 `agents/runs/hide-agent-attention/`에 두고 커밋하지 않는다.
- 디자인은 `AGENTS.md` "Design Reference"와 `DESIGN.md`를 따른다: 기호 색은 기존 토큰에서 오고 새 값은 `HideTheme`에 추가해 쓴다.
- engineering/principles.md 규칙 1: 평면 상태 문자열, 다섯 군데 어휘 사본, `sort_rank` 읽기, pet 모듈의 자체 버킷을 같은 변경에서 지우고 호환 경로를 남기지 않는다.
- engineering/principles.md 규칙 2: 세 축과 네 그룹 이상의 일반 상태 기계를 만들지 않는다.
- engineering/principles.md 규칙 3: 분류(T1)가 먼저, 읽음(T2)과 정렬(T3)이 그 위에, 행 통일(T4)과 pet(T5)이 그 위에 얹힌다.
- engineering/principles.md 규칙 4, 10: 손상된 읽음 기록은 조용히 읽음으로 처리되지 않고 비워지며 진단으로 드러난다. 분류할 수 없는 상태는 "알 수 없음"으로 보이지 idle로 숨지 않는다.
- engineering/principles.md 규칙 5: 분류와 읽음과 정렬은 core의 projection에, 렌더링은 셸의 행 컴포넌트 하나에 머문다.
- engineering/principles.md 규칙 7: 읽음 기록은 ui_state의 기존 pane 키 맵 패턴(`pane_text_scales`)을 따르고 새 저장 파일을 만들지 않는다.
- engineering/principles.md 규칙 8: 읽음의 권위를 hide에 두는 것은 장기 결정이다. herdr의 seen을 폴백으로 남기지 않는다.
- engineering/principles.md 규칙 9: 읽음 기록의 변화는 이벤트 이름, pane id, 시퀀스를 담은 구조화된 진단으로 남기고 내용은 담지 않는다.
- engineering/principles.md 규칙 11: 같은 포커스와 같은 상태로 두 번 갱신되어도 읽음 기록은 같은 값으로 수렴한다.
- engineering/principles.md 규칙 12, 13: 테스트는 snapshot에 실린 그룹과 읽음을 단언하고, 어휘 사본이라는 부류를 없애는 방식으로 고친다.
- design/principles.md 규칙 3: Done 항목을 확인하는 가장 잦은 동작은 클릭 한 번이며 그 클릭이 읽음까지 처리한다.
- design/principles.md 규칙 4: 유도 상태(그룹, 강조, 닫기 확인)는 core가 계산해 보여주고 사용자가 조합하지 않는다.
- design/principles.md 규칙 5: 섹션 라벨, 행, 배지는 사이드바의 기존 패턴을 쓴다.
- design/principles.md 규칙 7: 상태는 기호와 색으로 먼저 부호화하고 짧은 낱말 하나만 곁들이며, 설명 문장을 두지 않는다.
- Git과 PR 귀속: 브랜치 이름, 커밋 메시지, 트레일러, 생성 텍스트 어디에도 에이전트, 모델, 벤더, 도구 이름을 쓰지 않는다.

## 12. Implementation Result Report Contract

보고 항목:

- status: `Done`, `Partially Done`, `Blocked`.
- 사용자에게 보이는 변화를 두 문제(일관성 없는 항목, 한꺼번에 사라지는 Needs You) 각각에 대해.
- 바뀐 모듈과 새 모듈의 책임 경계, 실제로 고른 파일 구조.
- section 5의 구조를 따랐는지, 벗어난 곳과 이유.
- T1부터 T7까지의 완료 상태와 R/AC/V 커버리지.
- 모드별 검증 증거와 각 산출물이 있는 실행 디렉터리.
- T2에 대해: 플러그인 토큰 변화에 herdr `state_change_seq`가 오르는지 실제 바이너리에서 관찰한 결과.
- T7에 대해: `rules` CLI가 실행한 정확한 갱신과 원장의 결과 행.
- V4에 대해: 이 실행이 만든 herdr 워크스페이스, 탭, pane과 그 전부가 닫혔다는 확인, 그 밖의 어떤 것도 건드리지 않았다는 확인.
- 추가되거나 바뀐 자동 테스트와 각각이 막는 회귀.
- 삭제된 어휘 사본과 코드 경로의 목록.
- 이탈, 남은 인간 검토, 미완 항목과 후속 후보.
