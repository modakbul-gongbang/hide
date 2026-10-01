---
topic: "에이전트 세션별 작업·진행 라벨 분리"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "저장된 라벨의 세션 소유와 비동기 분석 결과 및 표시 경계를 변경하지만 운영 데이터나 권한 정책은 변경하지 않는다."
source_intake: "current conversation"
created_at: "2026-10-01"
updated_at: "2026-10-01"
---

# PRD: 에이전트 세션별 작업·진행 라벨 분리

## Goal

hide 사용자가 새 에이전트의 첫 작업 전에 이전 세션의 작업·진행 문구를 보지 않게 하고, 같은 세션의 재연결과 워처 재시작에서는 유효한 기존 라벨을 복원한다.
이슈 #268의 새로운 pane과 재사용된 pane 양쪽에서 세션 경계를 지킨다.

## Non-goals

- 라벨이 없을 때 작업명이나 진행 문구를 만들어 채우지 않는다. 실제 provider·Herdr 실행 상태는 유지하며 라벨은 현재 세션 분석 후 나타난다.
- 에이전트 이름, lineage, core의 읽음·안 읽음 의미를 바꾸지 않는다. 이 동작들의 변경은 별도 요청에서 다룬다.
- 모든 session discovery 소비자의 정책을 바꾸지 않는다. 이번 변경은 라벨의 검증된 transcript 소유 경계에 한정하고 다른 사용량·대화 기능은 기존 계약을 유지한다.
- upstream Herdr binary나 공개 프로토콜을 수정하지 않는다. 현재 고정된 metadata 계약 안에서 세션에 맞는 게시와 소비를 처리한다.
- 새 오류 배너나 경고창을 추가하지 않는다. 사용자가 조치할 수 없는 분석·식별 문제는 기존 진단 흐름으로 보낸다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 새 세션은 자신의 첫 라벨 결과가 준비되기 전까지 task·progress·expected reply 등 이전 분석 문구를 표시하지 않는다. | 이슈 #268의 기대 동작과 모든 화면에 대한 완료 조건. |
| D-02 | 같은 검증된 세션의 워처 재시작·재연결은 기존 유효한 라벨을 복원하며, lifecycle sequence 변경만으로 새 세션이라고 판정하지 않는다. | 이슈 #268. |
| D-03 | 라벨은 검증된 provider와 native session reference에 귀속하며, 비동기 작업은 세션 교체 세대까지 구분한다. A→B→A에서도 이전 A 작업을 현재 A에 적용하지 않는다. | 가정: 읽기 전용 조사에서 pane만으로 저장·분석 결과를 귀속하는 경로가 확인되었고 engineering 13이 명시적 상태 모델을 요구한다. |
| D-04 | 세션 식별이 없는 새 pane과 owner 없는 기존 저장 상태는 라벨 소유를 증명하지 못하므로 표시·이웃 transcript 추정을 허용하지 않는다. 이미 확인된 세션의 일시적 식별 부재에는 저장 상태를 보존하되 확인 전 표시를 보류한다. | 가정: 새 세션 누출 금지와 같은 세션 복원의 양립. 기존 legacy 자료의 라벨은 현재 세션 재분석으로 회복한다. |
| D-05 | ID와 경로 reference가 같은 session을 가리키는 경우는 실제 session metadata와 해석된 transcript 근거로만 동일성을 확인한다. 파일 교체·truncation은 분석 cursor를 재설정하되 현재 session 소유가 입증되지 않은 라벨은 복원하지 않는다. | 가정: native reference 형식과 incremental reader의 기존 경계를 유지하기 위한 결정. |
| D-06 | 새 세션으로 확인되면 라벨·의미 분석·cursor·pending/retry·report cache와 reader 소유를 무효화하고 이전 분석의 성공·오류·retry 결과를 모두 폐기한다. 실행 중인 실제 worker는 기존 물리적 동시 실행 상한 안에서 종료까지 추적한다. | 읽기 전용 조사에서 outcome 처리 순서와 세션 없는 결과 경로가 확인됨. engineering 14·15. |
| D-07 | 세션 소유를 확인하는 게시·소비 경계를 통해 이전 task·progress가 새 세션의 어떤 화면에도 잠깐 나타나지 않게 한다. 저장 상태 삭제만으로 완료라고 판단하지 않는다. | 이슈 #268의 어느 화면에도 표시되지 않는 완료 조건, pinned Herdr metadata guard가 token patch 소유를 검증하지 않는다는 조사. |
| D-08 | Herdr metadata의 명시적 null 삭제, report당 16 keys, resource당 32 keys, 값 80문자 등 현재 고정된 상한 안에서 게시한다. 세션마다 새 source를 계속 추가하지 않는다. | pinned Socket API와 현재 plugin report 예산에 대한 조사. engineering 15. |
| D-09 | 같은 session의 유효 라벨은 sidebar·Agent area·자동 탭 제목 등 기존 소비자에서 같은 의미로 표시되고, 사용자 지정 이름과 core의 read/unread·descendant 의미는 유지한다. | 이슈 #268의 표시 경로와 프로젝트 status-model 계약. |
| D-10 | 각 이슈별 worktree에서 PR·CI·merge commit까지 수행하며 되돌릴 수 있는 추가 결정은 맡긴다. 운영 설치 앱 교체와 운영 서버 변경은 포함하지 않는다. | 사용자 원문: “issue하나당 worktree 파서 /please 로 다 PR올리고 머지하는것까지”, “나대신 추가로 나오는 의사결정 다해주고”. agents/config.json의 PR·CI 감시·worktree 설정. |
| D-11 | engineering/principles.md와 design/principles.md 전체를 읽고 engineering 4·5·7·9·12·13·14·15와 design 5·9·10·13을 적용한다. 새 리스트·폼·파괴 UX가 없어 해당 규칙은 별도 화면 행동으로 번역하지 않는다. | oh-my-principle commit 654485f96b7764c759662d2c3e9e386ebc221cf6 및 프로젝트 ARCHITECTURE·status-model·UI_BEHAVIOR 계약. |
| D-12 | 이슈와 대화가 완결된 원천이며 해당 intake qa-log가 없어 Spec Gate는 원천 문서 부재로 건너뛴다. 준비 검사와 구현 후 독립 전체 계약 리뷰를 수행하고 human_approval은 pending으로 유지한다. | gen-prd conversation-only 계약 및 사용자의 구현 위임. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 이전 세션에 task·progress가 있는 상태에서 새 pane의 새 세션을 시작해도 첫 분석 전 sidebar와 Agent 탭·제목에 이전 문구가 보이지 않는다. | D-01, D-07, D-09 |
| B2 | 같은 pane에서 에이전트를 새로 시작하거나 provider가 바뀌어도 새 세션에 이전 문구·expected reply·semantic 분석 상태가 섞이지 않는다. | D-01, D-03, D-06 |
| B3 | 새 세션의 첫 현재 분석 결과가 도착하면 그 세션의 문구만 표시되고 실제 실행 상태는 라벨 유무와 무관하게 유지된다. | D-01, D-09 |
| B4 | 라벨이 없는 동안 가짜 작업명·진행 문구가 채워지지 않으며 사용자 지정 에이전트 이름과 실제 provider 표시는 기존 제목 규칙을 따른다. | D-01, D-09, D-11 |
| B5 | 같은 검증된 세션에서 워처를 재시작하거나 재연결하면 유효한 task·progress가 복원되고 불필요한 새 세션 초기화를 하지 않는다. | D-02, D-03 |
| B6 | lifecycle의 working·idle·done 변동과 연결 순서 변화만으로 기존 라벨을 새 세션 것으로 바꾸거나 지우지 않는다. | D-02 |
| B7 | 이전 세션 분석이 세션 교체 뒤 완료되거나 오류·retry를 내도 현재 라벨·cursor·분석 예약·게시 결과에 적용되지 않는다. A→B→A 교체도 동일하다. | D-03, D-06 |
| B8 | session reference가 아직 없거나 unsupported인 새 pane은 이웃 session의 transcript나 pane ID만으로 라벨을 복원하지 않는다. reference가 확인되고 현재 분석을 읽을 수 있으면 정상 표시로 회복한다. | D-04, D-05 |
| B9 | 이미 확인된 세션의 reference가 일시적으로 사라져도 저장된 라벨 소유를 잃지 않는다. 확인 전에는 문구 표시를 보류하고 같은 reference가 돌아오면 유효 상태를 복원한다. 다른 reference면 새 세션으로 처리한다. | D-02, D-04 |
| B10 | owner 없는 기존 저장 상태나 삭제·교체된 transcript에서 추정한 이전 라벨은 새 세션에 보이지 않는다. 현재 세션을 다시 읽으면 라벨을 회복할 수 있고 저장 형식 문제는 진단으로 남는다. | D-04, D-05, D-11 |
| B11 | 같은 session이 ID 또는 path reference로 표현돼도 입증된 동일성 안에서는 라벨 복원이 유지된다. 서로 다른 transcript를 단순 cwd 일치로 같은 세션으로 보지 않는다. | D-03, D-05 |
| B12 | 현재 session transcript가 늦게 생기거나 아직 사용자 작업이 없거나 분석이 비활성인 동안 이전 라벨을 표시하지 않고 실제 lifecycle은 유지한다. | D-01, D-04 |
| B13 | 세션 전환의 저장·게시·소비 순서 중에도 모든 라벨 소비자는 현재 session 소유를 확인하므로 이전 문구를 잠깐 렌더링하지 않는다. | D-07, D-08, D-09 |
| B14 | 세션 교체로 새 분석이 예약돼도 기존 분석 worker를 무시하여 실제 동시 실행 상한을 넘지 않는다. 대기·retry 상태와 metadata source 수는 상한 안에서 유지한다. | D-06, D-08, D-11 |
| B15 | task·progress만 바뀌는 경우에도 core의 read/unread 상태와 descendant badge·delegated ownership 및 사용자 이름은 기존 계약대로 유지된다. | D-09 |
| B16 | 분석·reference·게시 실패는 원인과 session 상관관계를 가진 진단으로 남고, 개인 transcript 내용이나 비밀 값을 로그에 싣지 않으며 새 경고창을 만들지 않는다. | D-11 |
| B17 | plugin 안내와 제목·상태 소유 문서는 현재 세션 복원과 초기 라벨 없음의 실제 동작을 설명하며 폐기된 workspace-name fallback을 안내하지 않는다. | D-01, D-09 |

## Technical structure

기존 라벨 저장·분석·게시 흐름에 검증된 세션 소유와 교체 세대를 도입하고, UI 소비 경계에서 소유가 확인된 라벨만 표시한다.
native reference 해석은 기존 session 경계를 재사용하며 core의 wire 변환 위치와 runtime 소유를 유지한다.
추가 서비스·권한·외부 저장소나 upstream binary 변경 없이 현재 metadata 계약의 상한 안에서 동작한다.

## Risks

- owner 없는 오래된 저장 자료는 소유를 입증할 수 없다. 한 번의 현재 세션 재분석 전까지 라벨이 비어 있을 수 있으며 이는 가짜 복원보다 정확한 상태다.
- 저장 상태와 이미 게시된 tokens가 서로 다를 수 있다. 실제 관리되는 watcher와 격리 앱에서 새 pane·재사용 pane·재연결·지연 분석을 관찰한다.
- 기존 report는 key 예산을 사용 중이므로 소유 증명을 추가할 때 전체 예산과 소비자가 함께 맞아야 한다. schema와 고정 binary를 대조하며 공개 API를 바꾸지 않는다.
- transcript 내용·session 개인 경로가 증거와 로그에 노출되지 않게 합성 session fixture와 격리 HOME을 사용한다. 운영 앱·서버는 유지한다.
- 구현은 다른 label 관련 worktree의 미커밋 작업을 가져오거나 지우지 않는다. 해당 문제에 필요한 변경만 자신의 branch에 커밋한다.
- 사용자 선행 작업은 없으며 사후 QA에서는 기존 업무 세션의 라벨 복원과 새 에이전트 시작 화면을 확인한다.
