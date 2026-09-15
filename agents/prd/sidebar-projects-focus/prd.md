---
topic: "사이드바 Projects Inactive 접기"
status: "ready"
human_approval: "approved"  # user 2026-09-14 verbatim: 승인
review_profile: "standard"
review_rationale: "사이드바 목록의 표시 방식과 저장 키 두 개를 바꾸는 사용자 가시 UI 변경이며, 데이터 파괴·인증·외부 효과는 없다."
source_intake: "agents/interview/sidebar-projects-focus/qa-log.md"
target_repository: "modakbul-gongbang/hide"
target_branch: "main"
screen_evidence: "screenshot"
created_at: "2026-09-14"
updated_at: "2026-09-14"
---

# PRD: 사이드바 Projects Inactive 접기

## Goal

Hide 사이드바 Projects 목록에서 머지·닫힘됐거나 7일간 활동 없는 checkout과 프로젝트를 각각 `Inactive N ▸` 한 줄로 접어, 지금 진행 중인 작업만 펼쳐진 상태로 보이게 한다.
살아있는 에이전트·dirty·미푸시·현재 선택이 있는 checkout은 어느 층에서도 접히지 않고, primary checkout은 프로젝트 안에서는 접히지 않지만 프로젝트 자체는 D-09의 판정으로 접힐 수 있다.
정렬은 기존 최근 활동순 그대로이고 에이전트 행은 손대지 않는다.
지금은 프로젝트 21개와 머지된 worktree들이 활성 작업과 같은 무게로 나열되어 어디에 집중할지 보이지 않는다.

## Non-goals

- 최근 활동순 정렬 기능 추가: 이미 구현되어 있다 (`project_context.rs sort_projects`, `DESIGN.md` Projects and checkout context). 이 PRD는 접기만 얹는다 (D-01).
- 머지된 checkout을 사이드바에서 완전히 숨기거나 삭제를 유도하기: 존재는 접힌 묶음의 숫자로 남는다. 재검토는 없다; 정리 흐름은 Overview의 Clean up merged worktrees가 계속 맡는다 (D-04).
- checkout 아래 에이전트 행(Idle 포함) 접기: 상태 축과 Option+1~9 순서에 묶여 있어 별도 PRD. 재검토는 이 PRD 머지 뒤 에이전트 행 길이가 여전히 문제로 보고될 때 (D-10).
- 7일 기준의 설정 항목: 코드 상수다. 재검토는 사용자가 다른 값을 요구할 때 (D-06).
- `design/hide.pen` 병렬 워크트리 충돌의 구조적 해결(직렬화 규약, JSON merge driver): issue #58. 재검토는 hide.pen rebase 충돌을 손으로 푸는 일이 처음 실제로 발생하는 PR (D-12).
- 접힌 항목을 Cmd+K로 선택했을 때 묶음을 자동으로 펼치기: 현재 선택 예외가 그 행을 밖으로 꺼내므로 별도 상태를 두지 않는다 (D-16).
- 활동 시각을 core가 모르는 checkout을 "오래됨"으로 추정하기: 추정한 시각으로 접지 않는다 (`DESIGN.md` "no UI interaction or local clock invents recency", engineering 4).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 프로젝트와 checkout의 최근 활동순 정렬은 이미 있으므로 새 정렬은 만들지 않고, 정렬된 목록 위에 접기만 더한다. | qa-log D-01: `herdr-core/src/project_context.rs:17-95`, `DESIGN.md:1113-1115` |
| D-02 | "머지됨"의 출처는 core에 있는 두 사실이다: `WorktreeSnapshot.merged == true`(git 기준) 또는 `pull_request.badge`가 Merged/Closed(gh 기준). 둘 중 하나면 머지로 본다. gh 조회가 실패한 checkout은 git 사실만으로 판정한다. | qa-log D-02, D-06: `model.rs:1408`, `model.rs:1281` |
| D-03 | 새 표시 상태는 기존 `expanded` 접힘 상태와 같은 경로를 따른다: core가 소유하고 이벤트로 바뀌며 영구 저장된다. | qa-log D-03: `model.rs:362`, `persistence.rs:25 expanded_paths` |
| D-04 | 머지된 checkout은 사이드바에서 사라지지 않고 프로젝트 아래 한 줄 묶음으로 접힌다; 펼치면 보인다. 완전 숨김과 삭제 유도는 기각. | Q1, 사용자: "B가 나을 것 같긴 해" |
| D-05 | 접힘 예외: Working 또는 Needs You 에이전트가 살아 있거나, dirty 변경이나 미푸시 커밋이 남은 checkout은 조건을 만족해도 접히지 않는다. 묶음 숫자가 Overview의 머지 목록 숫자와 달라질 수 있음을 감수한다. | Q2, 사용자: "그렇게 접힌거는 그렇게 보여야될듯" |
| D-06 | 묶음은 프로젝트당 하나, 라벨 `Inactive N ▸`. 포함 조건은 머지(D-02) 또는 stale: 마지막 에이전트 활동 시각과 마지막 커밋 시각이 둘 다 7일보다 오래됨(둘 중 하나라도 7일 안이면 stale이 아니다). 펼친 행은 각자 이유를 그대로 드러낸다(머지는 기존 PR 아이콘, 오래된 것은 경과 시간). 7일은 코드 상수이며 설정 항목을 만들지 않는다. 머지·stale 두 묶음으로 나누는 안은 기각. | Q3-Q5, 사용자: "7일로 하자" |
| D-07 | 프로젝트 안의 checkout 목록에서 primary(main) checkout은 절대 접히지 않는다. 현재 선택된(`focused_checkout_id`) checkout은 접히지 않으며 선택이 옮겨진 뒤에야 묶음에 들어간다. checkout 묶음의 펼침 여부는 프로젝트별로 기억하고 기본은 접힘. | Q6, Q11, 사용자: "우선 그렇게 가자" / "추천대로" |
| D-08 | 저장 상태는 묶음 펼침 여부뿐이다. 어느 checkout/프로젝트가 Inactive인지는 매 스냅샷마다 core가 계산하며 어디에도 저장하지 않는다. | Q6, 사용자: "상태는 최소화하고 버그 안나게" |
| D-09 | 프로젝트 층 판정은 checkout 층과 별개다: 프로젝트의 모든 checkout(primary 포함)이 각각 머지 또는 7일 무활동이고, 어느 checkout도 예외(D-05, 현재 선택)에 걸리지 않을 때만 그 프로젝트가 같은 device 그룹의 맨 아래 `Inactive projects N ▸`로 접힌다. primary 비접힘 규칙은 프로젝트 안에서만 적용되므로 primary가 7일 놀면 프로젝트째로 접힐 수 있다. | Q7, Q11, 사용자: "프로젝트에도 적용" / "추천대로" |
| D-10 | checkout 아래 에이전트 행은 비범위. | Q8, 사용자: "a로 우선" |
| D-11 | 설계는 `design/hide.pen`의 `Screen / Sidebar / Projects` 보드를 접힘·펼침·프로젝트 묶음 상태별 프레임으로 다시 그려 같은 PR에 싣는다. 접힌 묶음 행이 새 컴포넌트가 되면 `Proposed / Inactive fold row`로 그리고 이 표에 기록한다. 새 색·간격 값은 `HideTheme`에 먼저 추가한다. | Q9, 사용자: "design 변경사항이면 pen에 그려서 보여줘야지"; `AGENTS.md` Design Canvas |
| D-12 | hide.pen 병렬 워크트리 충돌은 위험으로만 다룬다: 이 PR은 자기 보드와 추가 노드만 건드리고, rebase 충돌 시 양쪽 노드를 살린 뒤 `gen-pen.mjs` → `check-pen.mjs`로 검증한다. 구조적 해결은 issue #58. | Q10, Q11, 사용자: "우선 issue로만 만들어두고" / "추천대로" |
| D-13 | 다시 그릴 보드 `Screen / Sidebar / Projects`는 이미 있다. hide.pen은 747 KB 단일 JSON이며 merge driver가 없어 병렬 수정 시 텍스트 충돌이 난다. | qa-log D-13: `design/hide.pen`, `.gitattributes` |
| D-14 | Cmd+K 검색은 접힌 checkout/프로젝트도 찾는다. 검색으로 접힌 항목을 선택해도 묶음은 펼쳐지지 않고, 선택된 checkout이 현재 선택 예외로 묶음 밖에 나타난다. | Q11, 사용자: "추천대로" |
| D-15 | 증명: core 단위 테스트가 판정(머지/7일 경계/예외/primary/선택/활동 시각 부재)과 묶음 위치·순서를 고정하고, 설치된 dev 빌드 스크린샷이 checkout 묶음의 접힘·펼침과 프로젝트 묶음을 보인다. 스크린샷은 `agents/runs/<slug>/` 아래에만 둔다. | 가정: qa-log D-15 (agent default, P2); `AGENTS.md` Evidence Belongs Outside The Repository |
| D-16 | `Inactive projects` 묶음의 펼침 여부는 device 그룹당 하나, 기본 접힘, 프로젝트별 `expanded`와 checkout 묶음 키와 독립된 저장 키. 저장 데이터에 키가 없으면 접힘으로 읽는다. | Q11, 사용자: "추천대로" |
| D-17 | 원칙 반영: engineering(`653c462`) 1·2·4·7·12, design(`653c462`) 4·5·7·9·10을 읽었다. design 4·7·9·10은 B1·B4·B11·B14로, engineering 4는 B9·B10과 non-goal로 옮겼다. design 11(구조적 후보 선택)은 인터뷰 Q1의 A/B/C 선택이 이미 수행했으므로 다시 묻지 않는다. | `sasu principles list` |
| D-19 | `Projects · Recent activity` 헤더의 카운트는 접힌 프로젝트를 포함한 전체 프로젝트 수를 유지한다; 접힌 수는 `Inactive projects N` 행이 따로 말한다. | spec gate 답변, 사용자: "ㅇㅇㅇㅇ"(추천 수락) |
| D-20 | Cmd+K로 접힌 프로젝트를 고르면 그 primary checkout이 선택되고, 현재 선택 예외로 그 프로젝트 행이 펼쳐진 목록의 활동순 자리에 기억된 펼침 상태로 나타난다; `Inactive projects` 묶음은 접힌 채 그대로이며 선택이 떠나면 프로젝트는 다시 묶음으로 돌아간다. | spec gate 답변, 사용자: "ㅇㅇㅇㅇ"(추천 수락) |
| D-18 | Delivery: `agents/config.json`의 PR 모드(base `main`, worktree, CI watch)로 Implementor가 PR을 열고 사람이 머지한다. | `agents/config.json` delivery.mode = pr |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 프로젝트를 펼치면 활성 checkout 행들 아래 마지막 줄에 `Inactive N ▸` 한 줄이 보인다; N은 그 프로젝트에서 접힌 checkout 수이고, 접힌 checkout이 없으면 이 줄은 그려지지 않는다. | D-04, D-06 |
| B2 | PR이 머지/닫힘되었거나 git이 base에 머지됐다고 보는 checkout, 또는 에이전트 활동과 커밋이 모두 7일보다 오래된 checkout은 다음 스냅샷에서 펼쳐진 목록에서 빠지고 묶음 N에 더해진다. | D-02, D-06 |
| B3 | Working/Needs You 에이전트가 살아 있거나 dirty 변경 또는 미푸시 커밋이 남은 checkout은 머지·7일 조건을 만족해도 펼쳐진 목록에 그대로 남는다; 그래서 묶음 N은 Overview의 머지 목록 수와 다를 수 있다. | D-05 |
| B4 | `Inactive N ▸`를 클릭하면 펼쳐져 그 checkout 행들이 기존 행 모양 그대로 보인다: 머지된 행은 기존 PR 아이콘, 오래된 행은 기존 경과 시간 표기(`12d`)로 각자 이유를 드러내며 별도 설명 문장은 없다. 다시 클릭하면 접힌다. | D-06 |
| B5 | checkout 묶음의 펼침 여부는 프로젝트별로 앱을 재시작해도 유지되고, 처음 보는 프로젝트는 접힌 상태로 시작한다. | D-03, D-07, D-08 |
| B6 | primary(main) checkout은 어떤 조건에서도 프로젝트 안에서 접히지 않는다. | D-07 |
| B7 | 현재 선택된 checkout은 조건을 만족해도 접히지 않고, 선택이 다른 checkout으로 옮겨진 다음 스냅샷부터 묶음에 들어간다. | D-07 |
| B8 | Cmd+K 검색 결과에는 접힌 checkout과 프로젝트도 나온다; 접힌 checkout을 선택하면 묶음은 그대로 접힌 채 그 행이 펼쳐진 목록에 나타나고, 접힌 프로젝트를 선택하면 그 primary checkout이 선택되어 프로젝트 행이 활동순 자리에 기억된 펼침 상태로 나타난다; 어느 쪽이든 선택이 옮겨지면 다시 묶음으로 돌아간다. | D-14, D-20 |
| B9 | 활동 시각을 core가 모르는 checkout(에이전트 활동도 커밋 시각도 없음)은 7일 조건으로 접히지 않는다; 머지 조건으로만 접힐 수 있다. | D-06 |
| B10 | gh 조회가 실패하거나 stale인 프로젝트에서는 git 머지 사실과 7일 조건만으로 판정하며, PR 상태를 추정하지 않는다. | D-02 |
| B11 | 모든 checkout(primary 포함)이 머지 또는 7일 무활동이고 예외 checkout이 없는 프로젝트는 Projects 목록에서 빠지고, 같은 device 그룹의 맨 아래에 `Inactive projects N ▸` 한 줄로 모인다; 접힌 프로젝트가 없으면 이 줄은 그려지지 않는다. `Projects · Recent activity` 헤더의 카운트는 접힌 프로젝트를 포함한 전체 프로젝트 수를 유지한다. | D-09, D-19 |
| B12 | `Inactive projects N ▸`를 클릭하면 펼쳐져 프로젝트 행들이 기존 모양과 각자 기억된 펼침 상태 그대로 보인다; 이 묶음의 펼침 여부는 device 그룹당 하나로 재시작 후에도 유지되고, 저장 데이터에 없으면 접힌 상태다. | D-09, D-16 |
| B13 | 접힌 프로젝트 안에서 에이전트 활동, 커밋, 또는 checkout 선택이 생기면 다음 스냅샷에서 그 프로젝트가 펼쳐진 목록의 활동순 위치로 돌아온다; 접힌 checkout도 같은 방식으로 돌아온다. | D-06, D-09 |
| B14 | 묶음 행은 다른 사이드바 행과 같은 hover·pressed·focus 피드백과 접근성 라벨(예: "Inactive, 4 checkouts, collapsed")을 가지며, 키보드로 펼치고 접을 수 있다. | D-11 |
| B15 | 이 PR의 `design/hide.pen`에는 `Screen / Sidebar / Projects` 보드가 checkout 묶음 접힘, checkout 묶음 펼침, 프로젝트 묶음 접힘, 프로젝트 묶음 펼침 네 상태 프레임으로 다시 그려져 있고, `node scripts/check-design-contract.mjs`가 통과한다. | D-11, D-12, D-13 |
| B16 | Rust 테스트 스위트에 머지/7일 경계(6일 23시간 vs 7일 1시간)/예외/primary/현재 선택/활동 시각 부재/프로젝트 predicate/묶음 위치를 고정하는 테스트가 있고, 설치된 dev 빌드에서 찍은 checkout 묶음 접힘·펼침과 프로젝트 묶음 스크린샷이 `agents/runs/<slug>/`에 있다. | D-15 |
| B17 | 접기 이후에도 펼쳐진 checkout과 프로젝트의 상대 순서, 에이전트 행, Needs You/Done 상단 그룹, SCRATCH 섹션은 이전과 동일하게 보인다. | D-01, D-10 |

## Technical structure

- 판정은 core(`herdr-core`)의 프로젝트 정렬 단계 옆에 붙는 순수 함수다: `sort_projects`가 이미 계산하는 checkout별 활동 키(에이전트 활동 unix_ms, 마지막 커밋 시각)와 `CheckoutSnapshot`의 `worktree.merged`, `pull_request.badge`, `dirty`, `unpushed`, `agent_summary.{working, needs_you}`, `focused_checkout_id`를 읽어 checkout·프로젝트의 Inactive 여부를 스냅샷 필드로 내보낸다. 셸은 그 결과를 그리기만 하고 자체 판정을 하지 않는다 (`docs/ARCHITECTURE.md`).
- 7일 비교의 "지금"은 core가 스냅샷을 만드는 시점의 시계다; 경과 시간 표기와 같은 근원이라 두 값이 어긋나지 않는다.
- 스냅샷에 프로젝트별 `inactive_checkouts` 묶음(펼침 여부, 행 목록)과 device 그룹별 `inactive_projects` 묶음이 추가된다. 두 묶음의 펼침 토글은 새 이벤트 두 종류로 들어오고, `persistence.rs`의 저장 상태에 `expanded_paths`와 같은 꼴의 키 두 개(프로젝트 경로 목록, device id 목록)가 추가된다. 저장 파일에 키가 없으면 빈 목록으로 읽는다.
- 그 밖의 스키마, 외부 서비스, Herdr 계약 변경은 없다.

## Risks

- Overview의 머지 목록 수와 사이드바 묶음 N이 달라 보일 수 있다(예외 규칙 때문). 사용자가 감수했다 (D-05); 묶음을 펼치면 행이 이유를 보여 준다.
- 7일 판정은 스냅샷 시점에만 다시 계산되므로, 앱이 열린 채 아무 변화가 없으면 경계를 넘긴 checkout이 다음 스냅샷까지 펼쳐진 채 남는다. 정렬의 경과 시간도 같은 성질이고 (`DESIGN.md:1117`), 실제 사용에서 스냅샷은 초 단위로 온다.
- `design/hide.pen`은 단일 JSON이라 다른 워크트리가 같은 파일을 바꾸면 rebase 충돌이 난다. 완화는 D-12; 구조적 해결은 issue #58.
- 묶음 라벨 문구와 접힌 행의 시각적 무게는 사람의 취향 판단이 남는다; PR 리뷰에서 pen 보드와 스크린샷으로 확인한다.
- 사용자가 미리 해 줄 일은 없다.
