---
topic: "CI와 테스트 신뢰성 및 실행 범위 정리"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "필수 verify의 선택과 집계 계약 및 실제 사용자 입력·탭 생성의 회귀 검사를 바꾸므로 독립 검토와 실제 OS 실행이 필요하다."
source_intake: "current conversation"
created_at: "2026-10-04"
updated_at: "2026-10-04"
---

# PRD: CI와 테스트 신뢰성 및 실행 범위 정리

## Goal

운영자가 PR 코드와 무관하게 반복되는 실패와 runner 대기 때문에 머지를 반복하지 않도록, 승인된 CI 비교·리팩토링 계획 전체를 구현하고 실제 검증을 거친 최종 수정본 PR을 제공한다.
불필요한 검사는 줄이고 필요한 검사의 누락·실패는 분명히 드러내며, 간헐적인 실제 제품 결함을 재실행 성공으로 감추지 않는다.

## Non-goals

- 운영자의 설치 앱·daemon·pane·server를 교체하거나 조작하지 않는다.
  실제 관찰은 사설 fixture 또는 정확히 식별된 후보 앱과 CI runner에서 수행한다.
- 진행 중인 focus-request-ordering 작업을 독립적으로 재작성하지 않는다.
  해당 결과의 포함 여부와 검증을 확인하고, 해결된 Windows pane-size race의 회귀 검사를 보존한다.
- 새로운 상시 scheduler·DB·dashboard나 테스트 framework를 만들지 않는다.
  기존 GitHub Actions, Rust, Playwright와 fixture를 확장하고 실제 용량 측정이 추가 인프라 필요성을 입증하면 별도 결정한다.
- 제한 시간·재시도·typing delay를 늘리거나 기대 결과를 약화해 실패를 감추지 않는다.
  제품의 기존 시간·입력·소유권 계약을 보존한다.
- PR을 머지하거나 release를 게시하지 않는다.
  최종 PR과 CI 결과를 제공하고 머지 판단은 운영자에게 남긴다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 계획뿐이라는 이전 범위를 최종 수정본 PR까지의 구현·검증으로 확장한다. | 사용자: "어어 그렇게 해서 쭉다 정리해서 최종 수정본 PR 올리는것까지 목표로 해줘. 잘 부탁해 제발!! CI가 너무 중요해~ 우리 구조에서" |
| D-02 | 전체 승인 계획의 실패별 조사·수정, fixture, 선택 실행, 집계, flaky 관리, concurrency, 중복 정리와 성공 증명을 보존한다. | 사용자 요청의 "CI와 테스트 전체 리팩토링" 및 "위 테스트마다", "먼저 할 것 3개", "기한·재시도를 늘려 가리는 방식은 쓰지 않는다"와 D-01의 전체 구현 승인. |
| D-03 | 탭 누락 사례를 공통 원인 후보로 조사하되 원인이 같다고 미리 단정하지 않는다. | 사용자 추가 데이터: checkout-owner :80/:99/:163과 Windows s2 :138은 marked workspace에 탭이 붙지 않아 하나 모자라는 한 원인 클래스일 가능성. |
| D-04 | focus 담당과 기존 Windows coverage·입력 readiness 작업의 소유권을 보존하고 이미 해결된 결과를 중복 수정하지 않는다. | 사용자: "focus-request-ordering pane이 지금 이 테스트를 따로 고치는 중이니 겹치지 않게" 및 현재 공개 PR·main의 확인 결과. |
| D-05 | required verify는 항상 게시하고 명시적으로 계획한 검사 전체의 성공만 승인한다. main 및 예약 전체 검사를 유지한다. | 승인 계획의 선택 manifest와 집계 계약; 가정: 경로 매핑이 불명확하면 전체 검사로 확장한다. |
| D-06 | 실패 이력과 정확한 시나리오별 격리는 GitHub Actions 결과·artifact 및 단일 registry로 관리한다. | 가정: raw 14일, 정규화 이력 30일, 격리 최초 만료 7일을 적용하고 실제 제품 결함은 미확정 상태로 제외하지 않는다. |
| D-07 | 동일 PR의 최신 실행을 유지하고 서로 다른 PR의 최신 실행은 취소하지 않는다. runner 상한과 준비 재사용은 측정 후 결정한다. | 승인 계획; 가정: 여섯 Linux shard의 max-parallel 4와 6을 비교하고 coverage·최초 통과율이 악화되면 해당 최적화만 되돌린다. |
| D-08 | engineering 원칙 전체를 읽고 관찰 가능한 계약, 원인 종류 해결, 자원 소유권·상한과 오류 전달을 적용한다. | engineering/principles.md at 654485f96b7764c759662d2c3e9e386ebc221cf6; 화면 디자인은 변경하지 않아 design domain의 별도 설계는 적용하지 않는다. |
| D-09 | 결과는 격리 worktree에서 coherent commit, 독립 Fidelity·Code 검토, 최종 커밋의 검증, PR의 실제 CI로 전달한다. | 사용자 D-01의 최종 PR 요청과 agents/config.json의 PR delivery 기본값; human_approval pending은 별도 PRD 문서 승인과 대화의 구현 권한을 구분한다. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 모든 PR에서 verify가 게시되며 문서만 바뀐 PR에는 필요한 문서·privacy·harness·정책 검사만 선택된다. | D-02, D-05 |
| B2 | Rust leaf 변경은 실제 역의존 계약을 포함하고 core·daemon·wire·ownership·focus 변경은 실제 Herdr 및 사용자 E2E와 관련 OS smoke를 포함한다. | D-02, D-05 |
| B3 | web·desktop 변경은 각 정적·unit·build·E2E 검사를 선택하고 terminal·store·shortcut·공유 shell은 전체 관련 연결 검사를 포함한다. | D-02, D-05 |
| B4 | platform·kit·hooks·공통 process/fs/path·Herdr pin 변경은 관련 모든 OS 계약과 install/package/integration 검사에서 누락되지 않는다. | D-02, D-05 |
| B5 | 테스트만 바뀌어도 해당 테스트와 fixture lane은 실행되며 workflow·classifier·공통 fixture·root lock/toolchain의 영향이 불명확하면 full 검사가 실행된다. | D-05 |
| B6 | rename·삭제·새 경로·실행 payload를 문서 변경으로 잘못 축소하지 않고 diff 실패나 빈 선택은 full 또는 명시적 실패로 표시된다. | D-05, D-08 |
| B7 | 실행 결과에는 계획한 lane, 실행·제외 이유, source identity와 정책 version이 표시되고 필요한 lane의 unexpected skip·실패·취소·미보고·unknown은 verify를 실패시킨다. | D-05, D-08 |
| B8 | 실제 merge 결과의 main 전체 검사와 세 OS 예약 coverage를 유지하며 진행 중인 coverage PR의 결과를 중복 없이 반영한다. | D-04, D-05 |
| B9 | 최초 실패와 이후 성공은 별도로 남고 test·assertion·실제 suite·OS·runner·run/job/attempt·head/base/tested SHA를 연결해서 확인할 수 있다. | D-02, D-06 |
| B10 | 같은 attempt의 중복 출력은 한 실패로 집계하며 assertion·fixture·provisioning·취소를 구분하고 API 오류·수집 누락은 실패 0건으로 표시하지 않는다. | D-06, D-08 |
| B11 | suite별 최초 통과율과 제한된 기간의 실패 이력이 job summary에 나타나고 raw·정규화 artifact는 유한 기간 보존된다. | D-06, D-08 |
| B12 | 격리는 정확한 test·OS·signature·근거 run·issue·담당자·등록/만료·대체 coverage·복귀 조건을 갖추고 누락·만료·미등록 flaky 표시는 필수 정책 검사에서 실패한다. | D-06 |
| B13 | 격리 검사는 관련 PR과 예약 실행에서 계속 결과를 남기되 일반 required 검사 완료를 지연시키지 않으며 해당 동작·test·fixture 수정 PR에서는 성공 증거가 필수다. | D-06 |
| B14 | 격리 범위 밖의 새 failure signature와 실제 사용자 의도·입력·IPC·atomic replacement 위반은 단순 advisory 성공으로 숨겨지지 않는다. | D-02, D-06 |
| B15 | 같은 PR에 새 commit이 오면 오래된 PR 실행을 취소하고 package·design·quarantine에도 일관된 독립 취소 scope가 적용된다. | D-07 |
| B16 | 서로 다른 PR의 최신 실행은 유지되고 main의 실행 중 full 검사는 취소하지 않으며 pending 병합·release/manual 범위를 구분해 기록한다. | D-07 |
| B17 | runner 동시 실행 상한·중복 command·build 재사용의 변경은 같은 SHA·profile·feature·OS/arch/toolchain 경계와 필요한 실제 계약 coverage를 보존한다. | D-07, D-08 |
| B18 | checkout-owner adoption·plain-folder와 Windows S2에서 각각의 새 탭 의도가 정확히 한 tab을 만들고 올바른 marked owner에 붙으며 UI·실제 Herdr·선택 ID가 일치한다. | D-02, D-03 |
| B19 | 빠른 두 탭 생성에서도 두 의도를 유지하고 plain-folder의 기존 unmarked 탭 1개와 marked owner의 새 탭 2개를 보존하며 Windows S2의 기대 4개를 유지한다. | D-02, D-03 |
| B20 | desktop edge-drag 이후 두 live group 각각에 모든 합성 입력 byte가 정확히 한 번 도착하고 다른 pane으로 전달되지 않으며 기존 shortcut·transport 검사가 통과한다. | D-02 |
| B21 | web external focused creation의 실제 tab ID가 bar에 나타나고 기존 두 canvas와 area layout을 보존하며 외부 focus가 원하는 canvas를 선택한다. | D-02 |
| B22 | IPC의 peer connect/shutdown/drop burst는 listener의 계약 안에서 연결을 잃지 않고 이후 정상 client가 응답하며 계약 밖 overload는 구분된 결과로 관찰된다. | D-02, D-08 |
| B23 | 동시 링크 교체와 실제 reader가 겹칠 때 old/new 내용 중 하나만 보이고 경로가 사라지지 않으며 임시 링크·thread가 남지 않는다. | D-02, D-08 |
| B24 | old hcoord shim의 유효 Node 선택을 보존하고 invalid/too-old 후보 및 probe 실패·취소·child 종료가 구분되며 fallback 성공이 fixture probe 실패를 감추지 않는다. | D-02, D-08 |
| B25 | native area-cycle의 launch 준비 실패와 실제 native focus/modifier 실패를 구분하고 exact candidate PID/window의 실제 입력·commit/cancel 결과를 유지한다. | D-02 |
| B26 | HTTP health·subscription·initial snapshot·terminal input 준비를 필요한 경계별로 확인하고 준비 전 입력 계약 검사는 별도로 유지한다. | D-02, D-08 |
| B27 | polling은 관찰만 수행하고 action·재전송은 명시적인 한 의도로 실행되며 고정 sleep을 늘려 상태 완료를 추정하지 않는다. | D-02, D-08 |
| B28 | 정상·실패·timeout·취소·worker exit에서 소유한 child/socket/home을 종료·회수하고 종료 확인 전 home을 삭제하지 않으며 수집 overflow·cleanup 실패를 보고한다. | D-02, D-08 |
| B29 | focus 담당 결과는 요청 동시성·마지막 선택·실제 focus·입력 대상·unknown 이후 새 의도 계약의 실제 증거로 확인하고 기존 Windows pane-size regression을 필수 coverage에 남긴다. | D-04 |
| B30 | 각 문제 OS의 수정은 retry 0·기존 deadline의 30회 독립 반복과 원래 suite에서 확인하고 가능하면 고정 이벤트 순서의 수정 전 실패·후 성공과 잘못된 결과를 검출하는 대조를 제공한다. | D-02, D-08 |
| B31 | CI 효과는 docs/web/core·platform scope와 PR 1개/동시 부하의 완료·준비 후 queue·job 실행·job-minutes·취소 낭비·최초 통과율을 구분하고 목표와 실제 측정·미확인을 구분한다. | D-02, D-07 |
| B32 | 관련 소유 guide·CONTRIBUTING의 실행 계약이 실제 workflow와 일치하고 obsolete wait·중복 gate를 같은 변경에서 제거하며 실행 evidence는 source commit에 포함되지 않는다. | D-02, D-08 |

## Technical structure

하나의 변경 범위 분류가 실행 manifest를 만들고 workflow 조건과 최종 verify가 그 계획을 함께 사용한다.
기존 GitHub Actions 결과·artifact를 정규화해 제한된 실패 이력을 만들며, 단일 격리 registry의 정책 검증과 별도 관찰 실행을 연결한다.
Rust·Playwright의 실제 외부 경계와 기존 fixture를 재사용하며, 재현이 제품의 의도·owner·projection·input·OS 계약 결함을 입증한 경우 그 소유 경계에서 수정한다.
새 상시 서비스나 데이터 저장소를 추가하지 않고, 설치 앱·운영자 상태·branch protection은 변경하지 않는다.

## Risks

- 경로 선택이 검사를 누락하거나 unexpected skip을 허용할 수 있으므로 실제 경로 표본·누락 대조와 full 결과 비교를 제공하고 공통 경계는 보수적으로 확장한다.
- 성공 표본·반복 0건은 희귀 flake의 부재를 입증하지 않으므로 deterministic 재현과 최초 실패 이력을 함께 유지한다.
- org runner 용량과 다른 저장소 부하는 아직 미확정이므로 속도 개선 수치를 달성했다고 미리 주장하지 않고 matched 측정과 롤백 단위를 남긴다.
- native 전경 입력과 OS별 검증은 사설 후보 및 CI 환경에서 수행하고 실행되지 않은 검사를 통과로 표시하지 않는다.
- 열린 PR·동시 pane의 변경은 소유자가 다르므로 해당 결과를 확인·통합하되 다른 worktree를 수정하거나 되돌리지 않는다.
- reversible policy 수치와 구현 선택은 agent-owned 가정이며 운영자가 이견을 제시하면 영향을 기록해 조정한다.
  현재 범위의 구현·PR·CI 실행은 D-01의 기존 권한으로 진행하며 새 credential·유료 인프라·release·merge 권한은 요구하지 않는다.
