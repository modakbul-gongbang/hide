---
topic: "Hide-first 비동기 상태와 pane/tab 닫기 일관성"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "실행 중인 pane의 종료와 재시도 경계를 바꾸므로 잘못된 대상 종료, 중복 실행, 복원 기록 손실을 막아야 한다."
source_intake: "current conversation"
created_at: "2026-09-16"
updated_at: "2026-09-16"
---

# PRD: Hide-first 비동기 상태와 pane/tab 닫기 일관성

## Goal

Hide 사용자가 tab이나 pane을 닫거나 이동해 작업을 이어갈 때 입력은 즉시 화면에 반영되고, 서버 결과가 늦거나 일부 이벤트가 오지 않아도 닫힌 항목이 계속 남거나 다른 작업까지 멈추지 않게 한다.
일관된 원칙은 **사용자 의도는 core에서 즉시 반영하고, pane의 실제 존재와 배치는 Herdr의 확인된 상태로 확정한다**이며, 즉시 반응과 성공 확정을 구분한다.

## Non-goals

- Swift shell의 독립 상태 저장소, 범용 작업 엔진, 새 의존성, 별도 daemon은 만들지 않는다.
  기존 core의 intent와 세션 동기화를 확장하고 대체된 플래그·실패 분기를 같은 변경에서 제거한다; 별도 서비스가 필요한 독립 요구가 생길 때만 재검토한다 (engineering 1·2·5·6·7·8).
- Herdr의 topology 소유권 이전, 확인 전 split 비율 추정, 낙관적 PTY 크기 변경은 하지 않는다.
  geometry 동작은 확인 전 대기 표시가 남으며, 서버의 원자적 예측·확정 계약이 생기면 재검토한다.
- 결과 불명인 파괴적 요청의 자동 재전송, exactly-once 보장, 이번 변경을 위한 Herdr API 확장·pin 교체는 범위 밖이다.
  현재 계약으로 확인할 수 없으면 불명 상태를 명시하며, 자동 복구가 반드시 파괴적 재실행까지 해야 한다는 별도 요구가 승인되면 서버의 operation/대상 세대 계약부터 설계한다.
- 원격·Scratch·Browser의 되살리기 지원 확대, 재시작을 넘는 undo/작업 이력 저장, 종료된 프로세스 메모리·스크롤백 복원은 하지 않는다.
  현재 복원 범위를 유지하며 이 기능들이 따로 요청되면 재검토한다.
- split·zoom·resize·move의 제스처나 기능 자체를 다시 설계하지 않는다.
  이들 작업에는 같은 대기·확정·재확인 규칙과 충돌 보호를 적용하되 화면 구성 변경은 별도 요구가 있을 때 다룬다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 닫기 오류를 개별 예외 처리로 덮지 않고 Hide-first 비동기 상태 계약으로 해결한다; 이번 산출물은 검토 가능한 PRD이며 구현은 승인 후 진행한다. | 사용자: "근본적인 문제해결은 어떻게 해야할까", "디자인 방향을 어떻게 일관성있게 안되나.. 약간 Hide first로 하고 나중에 상태를 반영하게 한다같은?", "이거 리뷰하고 최종 계획안 PRD로 세워봐". 아래 구체 정책은 별도 승인 전의 제안이다. |
| D-02 | core는 로컬 선택 의도·진행 중 작업·확인된 서버 상태로 하나의 화면 snapshot을 만든다; shell은 이를 렌더링하고 이벤트만 보낸다. | 가정: 채택 권고. `docs/ARCHITECTURE.md`의 단일 core 소유권을 유지하며 optimistic presentation과 서버 truth를 분리한다. |
| D-03 | tab 선택·키보드 focus 등 core 소유 값은 즉시 변경하고 최신 의도가 이긴다; pane 존재·배치·zoom·크기 등 Herdr 소유 값은 확인 전 확정하지 않는다. | 가정: 채택 권고. 현행 소유권 계약을 보존하고 close에도 동일한 구분을 적용한다. |
| D-04 | 닫기 승인이 끝나면 즉시 closing을 표시하고 입력·재닫기·MRU 재진입을 막는다; 확인된 같은 checkout의 대체 항목으로 작업을 옮기되 geometry는 조작하지 않는다. | 가정: 채택 권고. 단순 숨김 후 성공 취급과 전체 화면 rollback은 기각한다; `DESIGN.md`의 navigation·inline feedback을 재사용한다. |
| D-05 | 작업은 준비·전송·결과 확인·완료/거절/결과 불명으로 관리한다; 성공 응답만으로 충돌 보호를 해제하지 않고 이벤트 또는 fresh snapshot으로 현재 topology를 확인한다. | 사실: `runtime.rs`의 close effect 응답 시 guard 해제와 close 완료 검사 없는 reopen이 경쟁을 허용한다. 가정: 수명주기 통합을 채택한다. |
| D-06 | 복원 대상은 immutable 복원 정보를 확보한 뒤에만 close를 전송한다; `layout_not_found` 등도 typed 결과와 fresh snapshot으로 구분하며 문자열 비교로 성공 처리하지 않는다. | 사실: `live.rs`의 export 실패는 close 전송 전 실패이고, 관찰된 오류도 이 경로다. 가정: 이미 없어진 대상의 정리와 존재하는 대상의 capture 실패를 분리한다. |
| D-07 | 미완성 topology는 의존 범위의 마지막 confirmed 상태로 격리하고, 독립 tab/workspace의 완성된 변경은 계속 게시한다; 후속 focus 이벤트를 전체 publication의 필수 조건으로 두지 않는다. | 사실: `session_sync.rs`의 `ready_to_publish`는 pending layout/closure/focus 하나로 같은 host 전체 게시를 막는다. pinned 서버는 background workspace의 active tab을 닫아도 전역 focus가 그대로면 replacement focus 이벤트를 내지 않을 수 있다. |
| D-08 | 복원 준비와 개별 외부 요청은 각각 전체 수명 5초의 절대 deadline을 가진다; 불완전 topology 또는 전송 후 미확정 결과는 최초 발생부터 5초 안에 fresh snapshot 확인을 시작하고, 확인 실패 시 불명 상태와 읽기 전용 재확인을 제공한다. | 가정: 유한 복구 기준으로 채택. 기존 socket read/write timeout만으로 전체 요청 상한을 보장할 수 없으므로 별도 deadline이 필요하다; 무기한 spinner와 상시 polling은 기각한다. |
| D-09 | 같은 tab에 영향을 주는 mutation은 겹쳐 실행하지 않는다; move는 양쪽 tab, workspace 폐쇄가 걸린 작업은 그 workspace까지 충돌 범위를 넓힌다. 독립 범위는 병렬 진행한다. | 가정: 채택 권고. pane·부모 tab의 중복 종료와 바뀐 대상 집합에 대한 뒤늦은 실행을 막으며, 거절한 입력은 숨은 queue로 남기지 않는다. |
| D-10 | 작업 identity는 host·연결 세대·사용자 intent와 당시 대상 범위를 묶는다; 늦은 응답은 원래 작업에만 반영한다. 전달 여부가 불명인 close/create/move/toggle은 자동 재전송하지 않는다. | 사실: close request ID는 correlation이며 멱등성·원자적 대상 incarnation 검사 계약이 아니다. 가정: 현재 서버 계약 안에서 안전하게 수렴하며 exactly-once를 주장하지 않는다. |
| D-11 | 미확정 복원 예약과 확정된 최근 닫기 기록을 분리한다; 예약은 기존 20개 기록을 퇴출하지 않고, 완료 후 사용자 요청 순서로 한 건만 확정한다. | 사실: 현재 예약을 20개 stack에 먼저 넣으면 이후 거절돼도 밀려난 과거 기록은 돌아오지 않는다. 가정: 기존 session-only LIFO 계약은 보존하고 확정 시점만 바로잡는다. |
| D-12 | 닫힘을 확인하기 전에는 그 예약을 reopen할 수 없으며, 최신 예약이 미확정이면 더 오래된 기록을 몰래 대신 열지 않는다. 이미 확보한 복원 정보는 결과 불명만으로 버리지 않는다. | 가정: 채택 권고. `docs/ARCHITECTURE.md`·`DESIGN.md`의 복원 정책에 close/reopen 경쟁 방지 경계를 보강한다; Cmd+Shift+T와 기존 복원 대상 범위는 유지한다. |
| D-13 | 로컬·원격·Scratch·Browser 닫기는 같은 사용자 피드백과 결과 분류를 따르되 기존 실행 주체·권한·복원 적격성은 바꾸지 않는다; 파일 닫기는 저장 성공 전 제거하지 않는다. | 가정: 채택 권고. 경로별 executor는 유지하고 상태 계약만 일치시킨다; 현재 파일 draft 보호 및 plugin/remote 경계를 보존한다. |
| D-14 | 진행/오류는 기존 pane 헤더 또는 tab strip의 request-scoped 표시를 쓰고, 대상 소멸 시 같은 checkout의 기존 안내 위치로 옮긴다; 새 전역 오류 모달·배너는 추가하지 않는다. | 가정: 채택 권고. `DESIGN.md`의 operation feedback 및 기존 파괴적 닫기 확인을 재사용한다. |
| D-15 | 복구는 기존 host 세션 동기화 경계가 담당하고, host당 snapshot 조회 하나와 coalesced 후속 확인 하나로 제한한다; 오래된 응답이 최신 이벤트나 로컬 선택을 덮지 않는다. | 가정: 채택 권고. `docs/ARCHITECTURE.md`, `docs/PERFORMANCE_TESTING.md`; 요청별 새 polling loop나 shell 재조회 경로는 기각한다. |
| D-16 | 실제 pinned 서버의 active/background workspace·외부 닫기·응답/이벤트 역전·disconnect 시나리오와 native 앱 동작으로 계약을 검증한다; fixture 성공만으로 실제 연동 성공을 주장하지 않는다. | 가정: 검증 권고. `docs/PERFORMANCE_TESTING.md`, engineering 12·13; 앞선 "close 후 focus 이벤트가 없다"는 일반화는 outer loop의 focus sync 때문에 철회한다. |
| D-17 | engineering/design 원칙을 전체 적용하며 별도 시스템보다 기존 상태 경계를 강화한다; 새 목록이나 미정 레이아웃이 없어 design 1·11은 독립 요구로 만들지 않는다. | 원칙 intake: `engineering/principles.md`, `design/principles.md`, source commit `653c46267c79892316ab7e8ff91f3a9a7d1561fc`; engineering 1·2·5·6·7·8은 Non-goals/구조, 3은 일관된 end-to-end 구현 순서, 4·9·10·11·12·13 및 design 2~10·12는 아래 동작으로 반영한다. 원칙 간 override는 없다. |
| D-18 | 이번 전달은 격리 worktree의 PRD 커밋까지다; 구현 승인 후 worktree에서 구현하고 main 대상 PR·CI 확인으로 전달하는 저장소 기본값을 권고한다. | 사용자 요청은 PRD 작성까지이며 push·PR·merge·설치 권한은 부여하지 않았다. `agents/config.json`의 delivery.mode=pr, worktree.enabled=true, ci.watch=true는 후속 전달의 제안 근거이지 승인 근거가 아니다. |
| D-19 | 닫기 대상에 작업 중인 에이전트가 있을 때 작업 중단을 알리는 모달로 한 번 확인한다; 에이전트 프로세스가 켜져 있다는 사실이나 완료 결과의 미열람만으로 묻지 않는다. tab은 그 안의 대상들을 합쳐 한 번만 확인한다. | 사용자: "오 좋고 거기에 더해 에이전트가 이미 작업중이면 한번 모달로 물어보게 하기? 그냥 에이전트만 켜져있고 종료됐으면 괜찮고 ㅇㅇ". tab 집계는 기존 단일 모달 흐름을 재사용하는 가정이다. |
| D-20 | 확인 필요 여부는 core의 작업 activity·미해결 demand에서 도출하고 sidebar 그룹·읽음 여부와 분리한다; 미완료 질문·승인 대기도 보호하며, 알 수 없는 상태를 완료로 간주하지 않는다. 확인은 해당 intent와 검토한 대상 범위에만 유효하다. | 가정: design 3·5·6·9, engineering 4·7·13에 따른 안전 정책. `docs/status-model.md`와 `sidebar.rs`의 현재 Working/NeedsYou/Done 기반 확인은 미열람 완료까지 포함하므로 D-19에 맞게 변경한다. 기존 콘텐츠 손실 확인은 없애지 않고 필요한 경우 같은 모달에 합친다. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | tab 선택·pane focus는 네트워크 왕복을 기다리지 않고 첫 로컬 화면 갱신에 반영된다; 이전 요청의 늦은 응답·거절이 더 최근 선택을 되돌리지 않는다. | D-02, D-03 |
| B2 | 진행 중인 로컬 선택 의도가 없으면 Herdr의 실제 전역 focus 이동을 따른다; 다른 workspace의 active tab 정보 갱신만으로 현재 checkout이 바뀌거나 보지 않은 형제 pane이 읽음 처리되지 않는다. | D-03, D-07 |
| B3 | 작업 중인 에이전트 보호 또는 별도 콘텐츠 손실 확인이 필요하면 승인 전에는 숨기거나 종료하지 않는다; 취소하면 원래 화면·프로세스·복원 기록이 그대로이며 닫기 타이머·복원 예약도 시작하지 않는다. | D-04, D-13, D-19, D-20 |
| B4 | 닫기 승인 직후 대상에 closing 표시가 생기며 재닫기·입력·MRU 재진입이 막힌다; 같은 checkout에 확인된 대체 tab이 있으면 즉시 그 tab으로 이동하고, 현재 split 안에서는 기존 geometry의 생존 pane에만 focus를 옮긴다. | D-04, D-09 |
| B5 | 대체 항목이 없으면 마지막 확인된 자리에 닫기 진행 상태가 남는다; 실제 종료 확인 후 기존 빈 상태를 보이고 다른 프로젝트나 임의의 새 pane으로 이동하지 않는다. | D-03, D-04 |
| B6 | pane 닫기 중 부모 tab 닫기, tab 닫기 중 자식 pane 닫기, 같은 대상 연타는 추가 외부 mutation을 만들지 않는다; 처리 중임을 보이고 이전 요청이 끝난 뒤 자동 실행하지 않는다. | D-05, D-09 |
| B7 | 독립 tab의 닫기·선택·입력은 다른 tab의 느린 capture나 누락된 layout/focus 이벤트 때문에 멈추지 않는다; 불완전 범위에는 마지막 confirmed 상태와 동기화 표시가 남고 확인된 다른 변경은 계속 보인다. | D-07, D-09, D-15 |
| B8 | 복원 정보 준비에 실패하고 fresh snapshot에 대상이 존재하면 close를 보내지 않는다; 해당 closing 표시만 해제하고 "복원 정보를 준비하지 못해 닫지 않았음"과 재시도 가능한 이유를 표시하며 기존 복원 기록은 보존한다. | D-06, D-11 |
| B9 | export/not-found 응답 뒤 fresh snapshot에서 대상 부재가 확인되면 stale 항목을 정리하고 "이미 닫힌 항목의 화면을 동기화함"으로 처리한다; 이 요청이 실제 종료를 실행했다고 주장하거나 확보하지 못한 복원 기록을 만들지 않는다. | D-06, D-10 |
| B10 | close 성공 응답이 먼저 와도 closing/충돌 보호는 topology 확인까지 유지된다; topology가 먼저 도착해도 하나의 종료만 반영하고 뒤늦은 응답이 항목을 되살리거나 새 대상을 제거하지 않는다. | D-05, D-10 |
| B11 | active tab, background workspace의 active tab, 비활성 tab, 마지막 pane/tab/workspace의 닫힘 모두 후속 global focus 이벤트 없이도 fresh snapshot으로 수렴한다; replacement 선택은 확인된 topology에서만 정한다. | D-03, D-07, D-08 |
| B12 | 미확정 결과·불완전 topology는 발생 후 5초 안에 자동 상태 확인을 시작한다; 조회도 전체 5초 안에 끝나며, 확인 실패 시 최대 10초 안에 무한 대기 대신 "결과 확인 필요"와 "상태 확인"을 표시한다. 연속 이벤트가 기한을 계속 미루지 않는다. | D-08, D-15 |
| B13 | timeout·연결 끊김·해석 불가 응답은 확정 거절과 구별한다; 실제 종료 여부를 모르면 자동 재닫기·reopen 생성 없이 복원 예약을 보존하고 상태 확인은 읽기만 수행한다. | D-05, D-10, D-12 |
| B14 | fresh snapshot에서 대상이 여전히 존재하더라도 전달 불명인 이전 close가 실행되지 않았다고 단정하지 않는다; 해당 범위의 재실행 보호는 유지하고, 현재 상태와 아직 확인되지 않은 이전 결과를 구별해 보여준다. | D-05, D-10 |
| B15 | 서버의 확정 거절이면 해당 작업만 실패로 끝내고 closing과 그 예약을 해제한다; 다른 탭에서 진행 중인 작업이나 그 사이 사용자가 선택한 화면을 되돌리지 않으며 이유를 해당 작업의 위치에 남긴다. | D-05, D-11, D-14 |
| B16 | 연결 복구 시 새 authoritative 상태로 확인하며 과거 mutation을 재전송하지 않는다; 이전 연결·이전 조회의 늦은 결과가 새 topology나 최신 선택을 덮지 않고, 확인 불가능한 상태는 불명으로 남는다. | D-10, D-15 |
| B17 | 최근 닫기 기록이 20개여도 실패하거나 미확정인 새 닫기가 기존 기록을 지우지 않는다; 완료된 닫기만 요청 순서 기준 한 항목으로 반영하며 21번째 확정 기록이 생길 때 가장 오래된 확정 기록을 제거한다. | D-11 |
| B18 | 가장 최근 닫기가 미확정일 때 Cmd+Shift+T 또는 메뉴를 사용해도 새 pane을 만들거나 더 오래된 항목을 대신 열지 않고 "닫힘 확인 중" 또는 "결과 확인 필요"를 표시한다; 닫힘 확인 후에는 기존 LIFO 복원이 정상 동작한다. | D-11, D-12 |
| B19 | 외부 클라이언트 닫기나 자체 종료만으로 새 undo 항목이 생기지 않는다; Hide가 복원 정보를 확보하고 close를 전송한 뒤 부재를 확인한 경우에는 그 예약 하나만 복원 가능해지며 실행 주체까지 추정하지 않는다. | D-06, D-10, D-11 |
| B20 | 원격·Scratch·Browser 닫기도 즉시 대기 표시·중복 방지·거절/불명 구분을 제공한다; 연결 또는 권한이 없으면 요청하지 않고 이유를 표시하며 기존 복원 제외 정책을 유지한다. | D-09, D-13, D-14 |
| B21 | 파일 탭은 저장 성공 후에만 사라진다; 저장 오류·권한 거절이면 draft와 탭을 보존하고 오류를 보여주며, pane 닫기 개선 때문에 저장 확인이 생략되지 않는다. | D-13 |
| B22 | split·zoom·resize·move 요청은 즉시 진행 상태를 보이지만 확인 전 가짜 pane·비율·PTY 크기를 적용하지 않는다; 빠른 resize는 최신 목표값으로 합치고 확인 불명인 생성·이동·toggle은 재전송하지 않는다. | D-03, D-09, D-10, D-15 |
| B23 | 진행·실패·부분 완료는 해당 요청에 귀속되어 다른 성공 응답으로 지워지지 않는다; 대상이 사라지면 같은 checkout의 tab strip 안내로 옮기고 새 오류 모달을 띄우지 않는다. | D-14 |
| B24 | 키보드와 마우스 닫기가 같은 결과를 만들며 대기·불명·비활성 이유를 접근성 이름/help로도 알 수 있다; 좁은 창과 한국어·영어 안내에서도 닫기·상태 확인 조작이 가려지지 않는다. | D-04, D-14, D-17 |
| B25 | 서버 지연 중에도 독립 pane 입력·선택과 렌더링이 진행되고, idle에서는 추가 상시 snapshot polling이 생기지 않는다; 반복 상태 확인은 host당 진행 조회 하나와 후속 확인 하나를 넘는 대기열을 만들지 않는다. | D-07, D-08, D-15 |
| B26 | 지원 진단에서 어느 host·작업·대상·단계가 언제 멈췄고 어떤 확인으로 끝났는지 추적할 수 있다; 터미널 내용·사용자 프롬프트·인증 정보는 새 로그에 포함하지 않는다. | D-10, D-16, D-17 |
| B27 | 앱을 다시 시작하면 이전 미확정 요청을 실행하지 않고 새 서버 상태를 읽는다; 최근 닫기 이력은 기존대로 세션 한정이며, 복원이 프로세스 메모리나 스크롤백까지 되돌린다고 안내하지 않는다. | D-10, D-11, D-12 |
| B28 | 닫기 전 복원 정보 준비는 승인 후 5초 안에 성공하거나 실패로 끝난다; 시간 초과 후 도착한 export 결과가 뒤늦게 close를 보내지 않으며, 이미 전송한 요청의 시간 초과는 취소 성공이 아니라 결과 불명으로 처리한다. | D-05, D-06, D-08, D-10 |
| B29 | 작업 중인 에이전트의 pane을 닫으면 대상과 작업 중단 결과를 알리는 모달이 한 번 뜬다; tab에 작업 중인 에이전트가 여러 개면 실제 대상 개수를 합쳐 한 번만 묻고 pane마다 연속으로 묻지 않는다. | D-19, D-20 |
| B30 | 에이전트가 입력 대기 중인 idle이거나 작업 완료 상태면, 프로세스가 살아 있거나 결과를 아직 읽지 않았어도 작업 보호 모달 없이 닫기 흐름으로 간다; 일반 터미널도 같다. 별도 콘텐츠 손실·저장 보호가 필요한 경우의 기존 안전 절차는 유지한다. | D-13, D-19, D-20 |
| B31 | 모달은 "작업을 중단하고 닫기"와 "계속 열어두기"를 제공하고 파괴적 버튼을 기본 선택하지 않는다; Escape는 취소하며 단축키 연타로 모달이 중복되거나 종료 승인이 재사용되지 않는다. 승인 후에만 기존 Hide-first closing 흐름을 한 번 시작한다. | D-04, D-09, D-19, D-20 |
| B32 | 미해결 질문·권한 승인을 기다리는 작업도 읽음 여부나 delegated 표시와 무관하게 보호하며, 실제 이유를 "답변/승인 대기 중"으로 표시한다; 완료·idle에 남은 단순 오류/미열람 표시만으로 작업 중이라 하지 않는다. 활동 상태를 알 수 없으면 작업 중이라고 단정하는 모달 대신 상태 확인 필요를 알리고 확인 전 종료를 보내지 않는다. | D-08, D-13, D-20 |
| B33 | 확인 중 대상이 사라지거나 tab에 새 pane이 추가되면 기존 승인을 다른 대상·늘어난 범위에 적용하지 않고 그 요청을 취소해 변경 사실을 알린다; 대상을 다시 선택해야 새 요청이 된다. 확인 없이 닫는 경로에서도 외부 전송 전 새 작업 시작이 관찰되면 전송을 멈추고 작업 보호 확인으로 전환한다. | D-05, D-10, D-19, D-20 |

## Technical structure

core의 확인된 서버 projection, 로컬 선택 의도, 진행 중 operation을 분리하고 그 합성 결과만 shell snapshot으로 노출한다.
operation은 요청 identity·충돌 범위·단계·deadline·복원 예약을 소유하며, topology 완료와 transport 응답을 별개 증거로 처리한다.
작업 보호 확인도 core가 같은 intent와 대상 범위에 묶어 관리하고 기존 native 확인 모달을 재사용하며, shell의 단순 확인 boolean만으로 달라진 대상의 종료를 승인하지 않는다.
동일 범위의 작업 수는 하나로 제한하고, 복원 순서 예약 때문에 독립 범위의 외부 실행을 직렬화하지 않는다.
미완성 event 후보와 게시 가능한 confirmed projection을 구분하여 의존 범위만 보류하며, cross-tab/workspace 변경은 그 전체 범위가 일관될 때 함께 게시한다.
기존 host 세션 동기화 경계에서 fresh snapshot과 event cursor/연결 세대를 함께 조정하고, 조회 도중 도착한 이벤트를 순서 검증해 병합하거나 다시 bootstrap하여 오래된 상태의 역적용을 막는다.
조회·export·close I/O와 큰 직렬화는 runtime lock 밖에서 실행하고, notifier는 실제 상태 변경만 burst 단위로 알린다.
확정 undo stack과 미확정 예약은 core 메모리에만 두고, 기존 로컬/원격/plugin 실행 경로와 파일 저장 경계는 보존한다.
새 서비스·영구 저장소·외부 API는 추가하지 않으며, 구현 시 현재 architecture/design 가이드와 기존 `design/hide.pen`의 해당 상태를 같은 변경에서 갱신한다.

## Risks

- 진단 근거는 기존 close 성공 뒤 stale UI 요청이 이어진 로그와 현재 core/핀된 서버 코드다; 특정 누락 이벤트가 원래 사용자 사례의 직접 원인이었다는 재현은 아직 없다.
  서버 handler 외 outer focus sync까지 보면 "close 뒤 focus 이벤트 없음"은 틀리므로, background workspace에서 replacement 이벤트가 없는 경로와 일반 foreground 경로를 구분해 실제 pinned binary로 검증한다.
- public ID 재사용 사고를 이번 리뷰에서 재현하지 않았고 정상 close/create 카운터는 증가한다.
  다만 원자적 incarnation·멱등 close 계약이 없으므로 snapshot의 부재는 현재 상태만 증명하고 실행 주체나 exactly-once를 증명하지 않는다; 결과 불명이 계속되면 같은 범위 mutation은 잠길 수 있으며 자동 destructive 복구는 이 PRD가 약속하지 않는다.
- 5초/10초는 기존 성능 측정 결과가 아니라 이 PRD의 제안 상한이다.
  connect·부분 응답·느린 응답·동기화 coordinator 정체를 포함한 deadline과 stale completion 차단을 구현해야 하며, timer가 publish 성공에 의존하면 요구를 만족하지 못한다.
- 부분 게시가 잘못되면 서로 다른 시점의 pane·layout·focus가 섞일 수 있다.
  close cascade, 양쪽 tab move, 이벤트 중복·누락·역전, 오래된 snapshot, reconnect, 동시 닫기, 준비 실패, 응답 유실, close 직후 reopen, 가득 찬 undo stack을 caller-observable 회귀로 검증한다.
- 작업 보호 확인은 agent 프로세스 생존이나 Done/NeedsYou 그룹만 보고 판정하면 과잉 확인 또는 미완료 작업 손실을 만든다.
  Working·idle·미열람 완료·질문/승인 대기·단순 오류·unknown, 여러 pane 집계, 모달 취소·연타·대상 변경과 로컬/원격 공통 진입점을 검증한다; 서버의 원자적 상태 precondition이 없으므로 관찰하지 못한 상태 변화를 완전히 차단한다고 주장하지 않는다.
- 검증은 별도 socket·상태·fixture를 가진 격리 Herdr와 정확히 식별한 native candidate에서만 수행한다.
  운영 앱·서버·사용자 pane을 닫거나 재시작하지 않으며, native screenshot/실제 입력 흐름과 idle/driven 측정을 구분한다; fixture 테스트·코드 검토만으로 native 정상 동작을 주장하지 않는다.
- 현재 `docs/README.md`가 가리키는 계약과 `agents/rules/INDEX.md`의 focus/read 규칙을 기준으로 회귀를 검사하고, 오래된 reopen PRD의 과거 API·단축키를 다시 도입하지 않는다.
  Rust/Swift 관련 suite, 실제 pin/schema 계약, design/architecture 경계 검사 및 성능 가이드를 함께 통과해야 구현 완료로 판단한다.
- 현재 대화를 source로 사용했으며 이 주제의 완성된 interview qa-log가 없어 독립 Spec Gate는 source 부재로 생략한다.
  구체 정책은 제안 상태이며 `human_approval: pending`이다; 추가 계정·자격 증명은 필요 없고, 구현 전에 필요한 사용자 결정은 이 PRD의 범위·안전 정책 승인이다.
