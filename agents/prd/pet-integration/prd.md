---
topic: "herdr-pet을 herdr-ide로 완전 통합"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "로컬 데스크톱 앱의 사용자 가시 UI와 코어 상태 로직을 크게 바꾸지만, 프로덕션 데이터·인증·과금·외부 파괴적 side effect 경계는 건드리지 않는다."
source_intake: "agents/interview/pet-integration/qa-log.md"
created_at: "2026-08-30"
updated_at: "2026-08-30"
---

# PRD: herdr-pet을 herdr-ide로 완전 통합

## 1. Summary

herdr-pet(Rust+Tauri 데스크톱 펫)의 기능 전체를 herdr-ide 앱 안의 네이티브 부속 플로팅 펫 창으로 재구현한다.
현재 herdr-ide의 PetWindow는 발자국 버튼뿐인 자리표시자이고, 실제 펫 기능(애니메이션 pose, 상태 뱃지, `_new` attention, 클릭 점프, 테마, 토글 표면)은 전부 herdr-pet 저장소에만 있다.
코어 로직(pose 우선순위, 상태 집계, herdr 소켓 `_new` 판정)은 `docs/ported-reference/`의 Rust 원본을 테스트와 함께 herdr-core로 이식하고, UI는 Tauri 웹뷰 JS를 SwiftUI로 재작성한다.
완료되면 herdr-pet 저장소는 은퇴 가능한 상태가 된다(실제 은퇴는 별도 후속).

Approval checklist:

- 범위: herdr-pet 기능 전체 패리티, 단 question payload(#8)는 명시적 제외 - 3장
- 과거 "Rust 재사용 안 함" 결정(이전 인터뷰 D-14)의 공식 뒤집기: 코어 Rust 로직 이식 재개 - 5장, 4.3장
- 구조 변경: herdr-core에 펫 상태 모듈 신설 + C ABI 스냅샷/이벤트 확장 + macos 셸에 펫 렌더러·메뉴바·Settings 섹션·URL scheme - 5장
- 클릭 동작: 메인 윈도우 포커스 + 가장 오래된 unseen pane 점프(최초 관찰 시각 기준, pane 순서 fallback) - R5, AC5
- 표시 토글 표면 4종(Settings 토글, 메뉴바, 글로벌 단축키, `herdr-ide://`)이 단일 상태 공유 - R7, AC7
- 검증 모드: 이식된 Rust/Swift 자동 테스트 + 설치 번들 실행 스크린샷 필수 - 9장
- delivery mode: local (agents/config.json 기본값, PR 자동화 없음)

## 2. Problem, Goal, And Users

사용자(이 저장소의 단일 운영자)는 herdr 멀티플렉서로 여러 코딩 에이전트를 돌리면서, 화면 구석의 펫 하나로 "지금 누가 일하고, 누가 나를 기다리는가"를 주변시로 인지하고 싶어 한다.
그 역할을 하던 herdr-pet은 별도 Tauri 앱으로 남아 있고, herdr-ide는 그것을 대체하기로 이미 선언했지만(docs/PORTING.md) 실제 기능은 아직 이식되지 않아 "잘 안 동작하는" 상태다.
목표: 펫의 전체 가치를 herdr-ide 하나로 흡수해서, 별도 앱 없이 IDE 자체가 주변시 상태 표시와 attention 점프를 제공하게 한다.

### 2.1 User Scenarios

- SC1. 상태 인지와 attention 점프: 에이전트가 질문을 남기면 펫이 바뀌고, 클릭 한 번에 그 pane 앞에 도착한다.
  Actors: 운영자(herdr-ide 사용자).
  Primary path: herdr 서버가 돌고 있는 동안 펫이 default 테마 아트로 pose(일함/기다림/배회/잠, 우선순위 error > notification > ... > sleeping)와 뱃지 행(파랑/초록/노랑/빨강 + ambient subagent/background-task)을 표시한다. unseen(`_new`) 질문이 생기면 attention으로 승격된다. 운영자가 펫을 클릭하면 herdr-ide 메인 윈도우가 포커스되고 가장 오래된 unseen attention pane이 선택된다.
  Failure state: herdr 서버 미실행(소켓 부재)이면 펫이 연결 끊김 상태를 명시적으로 표시한다(조용한 기본값 금지). 개별 에이전트 Disconnected는 유지되되 waiting으로 집계되지 않는다. unseen이 하나도 없으면 클릭은 메인 윈도우 포커스만 한다. 깨진 스냅샷은 전체 깨짐이면 마지막 유효 상태 유지, 개별 항목 깨짐이면 그 항목만 제외하고 제외를 로그에 남긴다.
  Recovery: 소켓이 다시 생기거나 유효 스냅샷이 오면 다음 폴링에서 자동 복구된다.
  Reach: herdr 서버를 로컬에서 실행하고 에이전트 pane 2개 이상을 서로 다른 시각에 unseen 질문 상태로 만든다. 상태 조작이 실제 에이전트로 번거로우면 기존 검증 픽스처 경로(herdr-core fixture)를 확장하는 것이 T 태스크다.

- SC2. 펫 이동, 표시 토글, 재시작 복원: 펫을 원하는 곳에 두고, 4개 표면 어느 것으로든 껐다 켜며, 재시작해도 그대로다.
  Actors: 운영자.
  Primary path: 드래그로 위치를 옮기고, Settings의 Pet 토글, 메뉴바 아이콘 메뉴(Show/Hide Pet, Pet Settings), 글로벌 단축키(사용자가 캡처로 설정), `herdr-ide://hide|show|toggle` 중 어느 표면으로든 숨기고 다시 부른다. 네 표면은 하나의 표시 상태를 공유해 항상 일치한다. 재시작하면 마지막 위치와 표시 상태가 복원된다.
  Failure state: 저장된 위치가 오프스크린 좌표(실사고 좌표 [542720, 163840] 포함)면 클램프가 가시 영역으로 되돌린다. 단축키 미설정이면 아무것도 등록하지 않고, 등록 충돌이면 명시적 오류를 보여준다. 꺼둔 채 종료했으면 꺼진 채 시작한다.
  Recovery: 클램프가 항상 가시 영역을 강제하므로 펫을 잃어버리는 상태가 없다. 숨긴 펫은 남은 세 표면 어느 것으로든 되부를 수 있다.
  Reach: 설치 번들(dev 빌드 스크립트 산출물)을 실행하고, 위치·표시 상태는 영속 설정을 직접 조작하거나 앱 재시작으로 만든다.

## 3. Scope And Non-Goals

포함:

- 애니메이션 펫 렌더(default 테마, pose 우선순위) - R1
- 뱃지 행(상태 버킷 + ambient 뱃지) - R2
- `_new` 토큰 attention 계약과 stateless 재계산 - R3
- 상태 집계·연결 수명주기·깨진 스냅샷 처리 - R4
- 클릭 시 메인 윈도우 포커스 + 가장 오래된 unseen pane 점프 - R5
- 드래그 + 화면 클램프 + 위치·표시 상태 영속 - R6
- 표시 토글 표면 4종(Settings, 메뉴바, 글로벌 단축키, URL scheme)의 단일 상태 - R7
- theme.json 테마 로더(이번엔 default 테마 1개 제공) - R8
- 이식 완료에 따른 문서·참조물 정리 - T8

Non-goals (의도적 제외, 각각 사용자 결정):

- question payload 표시(#8): herdr-agent-pet 플러그인은 herdr 쪽 설치물이라 흡수 불가하고 현재 설치도 안 된 죽은 기능. 제외 (D-07). 재방문 조건: 펫에서 에이전트 질문 보기를 다시 원하면 플러그인 소스 이사 + 설치로 되살린다.
- 구 `herdr-pet://` scheme 등록: 은퇴 앱의 scheme이라 의도적 미지원 (D-10).
- 메뉴바 메뉴의 Quit 항목: herdr-pet에서는 펫 앱 종료였지만 여기서는 IDE 전체 종료가 되므로 제외 (D-23, 패리티의 의도적 예외).
- herdr-pet 저장소·로컬 앱 정리: 설치 번들 검증 통과 후 별도 후속에서 사용자 확인을 받아 진행. 이번 범위에서 herdr-pet 저장소 변경 없음 (D-12).
- default 외 추가 테마 제작: 로더 구조만 갖추고 테마는 1개 (D-11). 재방문 조건: 사용자가 새 테마를 요청할 때.

Product Completeness: 위 제외 항목 외의 주 여정(상태 인지 → attention 점프, 이동/토글/복원)과 실패·복구 경계(소켓 부재, 깨진 스냅샷, 오프스크린 좌표, 단축키 충돌)는 전부 포함이다. MVP 축소가 아니라 작업 순서만 층위화한다(T 순서).

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required - 필요한 에셋(펫 아트 59개)과 참조 코드는 저장소에 이미 있고, herdr 서버·빌드·검증 전부 에이전트가 로컬에서 실행할 수 있다. 계정, 크리덴셜, 구매, 사용자 소유 자산이 없다.

### 4.2 Human Decisions Before PRD Approval

None required beyond the Approval checklist - 재료가 되는 결정(범위, 구조, 클릭 동작, 토글 표면, 검증 모드, Rust 재사용 뒤집기)은 인터뷰에서 이미 개별 승인되었고, 이 PRD는 그것을 계약으로 고정할 뿐이다.

### 4.3 Decision Traceability For Fidelity Review

인터뷰 Decision Register (agents/interview/pet-integration/qa-log.md) 전체의 처분:

- D-01 (fact): herdr-ide가 herdr-pet을 대체하며 과거 "Rust 재사용 안 함" 제약이 있었다 - 배경 사실, D-16이 일부 대체. 컨텍스트로만 사용.
- D-02 (fact): herdr-pet 저장소 구성 - 컨텍스트. #8 관련 부분은 non-goal 근거.
- D-03 (fact): PetWindow 자리표시자와 core의 Pet 항목 존재 - R1·T4의 출발점.
- D-04 (fact): herdr-agent-pet 미설치, 에셋은 이 저장소에 존재 - non-goal(#8) 근거와 R8 전제.
- D-05 (user, P0): 네이티브 완전 흡수. 모노레포 동거·위젯 축소는 기각 - 전체 방향. R1-R8.
- D-06 (user, P0): 기능 전체 패리티 + 층위 순서(코어 5개 먼저) - R1-R8, T 순서.
- D-07 (user): question payload 제외 + 재방문 조건 - non-goal.
- D-08 (user): 클릭 = 메인 윈도우 포커스 + 가장 오래된 unseen pane 점프 - R5, AC5.
- D-09 (user): 자동 표시, 토글, 마지막 표시 상태 기억 - R6, R7, AC6, AC7.
- D-10 (user): `herdr-ide://hide|show|toggle` + 관용 파싱, `herdr-pet://` 미등록 - R7, AC8, non-goal.
- D-11 (user): theme.json 로더 + default 테마 1개, 로더 테스트와 번들 스크린샷 검증 - R8, V2, V4.
- D-12 (user): 은퇴는 검증 후 별도 후속, 이번 범위에서 herdr-pet 저장소 변경 없음 - non-goal, guardrail.
- D-13 (fact): herdr-core의 기존 소켓 polling 경로 공유, `_new` 판정·집계는 이식 필요 - 5장, T2.
- D-14 (fact): 단축키 패리티 계약(사용자 캡처, 빈 값 = 미등록, 충돌 시 명시 오류, event.code 함정) - R7, AC7, guardrail.
- D-15 (assumption, agent 소유): 펫 아트는 animated webp이고 Swift 재생 방식은 스파이크로 검증, 실패 시 프레임 시퀀스 변환 대체 - T1, 리스크 RF1. 가정으로 유지하며 사용자 결정으로 승격하지 않는다.
- D-16 (user, P0): 과거 "Rust 재사용 안 함" 공식 뒤집기, 코어 이식 재개(UI는 SwiftUI 재작성) - 5장, T2, ADR성 기록.
- D-17 (fact): `_new`→plain 전환은 herdr 서버 소관, 펫은 stateless 재계산, unseen 없으면 포커스만 - R3, R5, AC3, AC5.
- D-18 (fact): Disconnected 상태 모델(웨이팅 미집계), 소켓 부재 명시 분기, 마지막 유효 스냅샷 - R4, AC4.
- D-19 (fact): 위치 영속(연결 모니터 위일 때만 신뢰, RESTORE_MARGIN, 사고 좌표 픽스처) - R6, AC6.
- D-20 (user): 깨진 스냅샷 = herdr-pet 패리티 + 개별 제외 로그 보강 - R4, AC4.
- D-21 (user): oldest-unseen 판정 키 = 최초 관찰 시각(메모리) + pane 순서 fallback, 비영속 - R5, AC5.
- D-22 (user): Settings Pet 토글, D-09와 단일 상태 - R7, AC7.
- D-23 (user): 메뉴바 아이콘 이식(Show/Hide + Pet Settings, Quit 제외) - R7, AC7, non-goal(Quit).
- D-24 (fact): 트레이 원형과 기존 Settings 씬 - T7 참조 컨텍스트.

기각·거절된 옵션 (거절 상태 유지):

- Q1의 (B) 모노레포 동거, (C) 위젯 축소 - 기각.
- Q2의 축소 추천(1~5만 필수) - 사용자가 기각하고 전체 패리티 선택.
- Q3의 (a) 플러그인 소스 이사 - 사용자가 기각하고 제외 선택.
- Q4의 (b) 포커스만 - 기각.
- Q9의 (b) 상태바 아이콘 생략 - 기각.

Principles intake: `~/projects/oh-my-principle` (commit 35ab76c)의 engineering/principles.md와 design/principles.md를 전문으로 읽었다. 적용 규칙은 11장 guardrail로 번역했고, 관측 가능한 것은 AC에 반영했다(AC4의 명시 표시·제외 로그 = engineering 4·9·10, AC7의 1클릭 토글·상태 시각 인코딩 = design 3·7). engineering 11(멱등)은 토글·폴링·클램프가 본질적으로 수렴형이라 별도 AC 없이 guardrail로만 번역했다. 나머지 미번역 규칙은 이 변경의 표면과 무관하다(예: env practice - 새 환경 변수 없음).

## 5. Major Technical Structure Changes

- herdr-core(Rust)에 펫 상태 모듈 신설: 기존 herdr 로컬 API 소켓 polling 경로(live.rs)를 공유하고, `docs/ported-reference/`의 herdr.rs(`_new` 판정·스냅샷 파싱), aggregate.rs(상태 집계·Disconnected), behavior.rs(pose 우선순위)를 테스트와 함께 이식한다. 과거 "Rust 재사용 안 함" 결정을 공식 뒤집는 구조 변경이다(D-16).
- C ABI 확장: 기존 6함수 계약(`herdr_core.h`)은 유지하고, 스냅샷 JSON에 펫 상태(pose, 뱃지, attention 목록, 표시 상태)를 추가하며 dispatch 이벤트에 펫 이벤트(클릭, 토글, 위치 변경)를 추가한다. 새 ABI 함수는 만들지 않는다.
- macos 셸(SwiftUI): PetWindow를 실제 펫 렌더러(테마 아트 애니메이션 + 뱃지 행)로 교체하고, 메뉴바 상태 아이템, Settings의 Pet 섹션, `herdr-ide://` URL scheme 등록(Info.plist 수준), 글로벌 단축키 등록을 추가한다.
- 영속: 펫 위치·표시 상태·단축키 accelerator는 herdr-core의 기존 영속 계층(persistence)에 얹는다. 새 저장소·DB·외부 서비스 없음.
- 신규 외부 서비스, 네트워크 경계, 스키마 마이그레이션 없음. herdr 서버와의 계약(로컬 소켓, `_new` 토큰)은 소비만 하고 변경하지 않는다.

## 6. Requirements

- R1. 펫 플로팅 창은 always-on-top 투명 창으로 default 테마의 애니메이션 아트를 표시하고, pose는 status-model.md의 우선순위(error > notification > sweeping > attention > carrying/juggling > working > thinking > idle/roam > sleeping, idle 8초 후 roam·60초 후 수면 시퀀스)를 따른다.
- R2. 펫에 뱃지 행을 표시한다: 에이전트 상태 버킷(파랑/초록/노랑/빨강)과 ambient subagent/background-task 뱃지. 파랑=working, 초록=finished-unconfirmed, 노랑=unseen 질문/승인, 빨강=unseen 오류(status-model.md 의미 그대로).
- R3. `_new`(unseen) 토큰만 attention/error로 승격한다. plain(acknowledged) 토큰은 승격하지 않고, legacy boolean 형태도 unseen으로 수용한다. 펫은 seen 상태를 자체 저장하지 않고 매 폴링 스냅샷에서 stateless로 재계산한다.
- R4. 연결 수명주기를 명시적으로 다룬다: 소켓 부재는 연결 끊김으로 구분 표시하고, 개별 에이전트 Disconnected는 유지하되 waiting으로 집계하지 않으며, 폴링 실패·전체 깨진 스냅샷은 마지막 유효 상태를 유지하고, 개별 항목 깨짐은 그 항목만 제외하며 제외를 로그로 남기고, 다음 유효 폴링에서 자동 복구한다.
- R5. 펫 클릭은 herdr-ide 메인 윈도우를 포커스하고 attention pane으로 점프한다. 복수 unseen이면 각 pane의 unseen을 처음 관찰한 시각(메모리 기록)이 가장 이른 pane을, 기록이 없거나 동시각이면 스냅샷 pane 순서로 결정론적으로 선택한다. unseen이 없으면 포커스만 한다.
- R6. 펫은 드래그로 이동하고, 위치는 항상 가시 영역으로 클램프되며, 마지막 위치(연결된 모니터 위에 있을 때만 신뢰)와 표시 상태가 재시작 후 복원된다. 기본은 실행 시 자동 표시다.
- R7. 표시 토글 표면 4종이 하나의 표시 상태를 공유한다: Settings의 Pet 섹션 토글, 메뉴바 펫 아이콘(정적 템플릿 아이콘, 메뉴: Show/Hide Pet + Pet Settings 열기), 글로벌 단축키(사용자 캡처로 설정, 빈 값이면 미등록, 충돌 시 명시적 오류, 물리 키 기반 판정), `herdr-ide://hide|show|toggle`(뒤 슬래시 등 관용 파싱). 어느 표면의 변경도 즉시 반영되고 서로 일치한다.
- R8. 펫 아트는 theme.json 로더로 로드한다. 이번 릴리스는 `assets/pet-theme/default/`의 default 테마 1개를 번들한다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | herdr 서버가 실행 중이면 펫이 default 테마 아트로 현재 에이전트 상태에 맞는 pose를 애니메이션으로 표시한다. 에이전트가 일하면 working 계열, 모두 idle이면 8초 후 roam이 가능하고 60초 후 수면 시퀀스로 진입한다 | judged | screenshot: 설치 번들에서 pose 애니메이션 캡처 |
| AC2 | 뱃지 행이 상태 버킷별 에이전트 수와 ambient subagent/background-task 존재를 반영하고, 상태 변화가 다음 폴링 주기 안에 뱃지에 나타난다 | judged | screenshot: 뱃지 행 상태 반영 캡처 |
| AC3 | unseen(`_new`) 질문·승인·오류 토큰만 attention/error pose와 노랑/빨강 뱃지로 승격되고, 같은 내용의 acknowledged(plain) 토큰은 승격되지 않는다. legacy boolean 형태는 unseen으로 취급된다 | machine | log: 이식된 코어 테스트 실행 결과 |
| AC4 | herdr 서버를 중단하면 펫이 연결 끊김 상태를 표시하고, 서버 재시작 후 다음 유효 폴링에서 자동 복구된다. 전체 깨진 스냅샷에서는 마지막 유효 상태가 유지되고, 개별 항목만 깨진 스냅샷에서는 그 항목만 제외된 집계가 표시되며 제외 사실이 구조화 로그로 남는다 | judged | screenshot + log: 끊김/복구 캡처와 제외 로그 |
| AC5 | 서로 다른 시각에 unseen이 된 pane 2개가 있을 때 펫 클릭은 먼저 관찰된 pane을 선택하고, 관찰 기록이 없는 재시작 직후에는 스냅샷 pane 순서의 첫 unseen pane을 선택하며, unseen이 없으면 메인 윈도우 포커스만 일어난다 | judged | screenshot + log: 클릭 3케이스 캡처와 판정 테스트 결과 |
| AC6 | 드래그로 옮긴 위치가 재시작 후 복원되고, 저장된 위치가 오프스크린 좌표(사고 좌표 [542720, 163840] 포함)거나 연결이 끊긴 모니터 위면 가시 영역으로 클램프되어 나타난다. 꺼둔 채 종료하면 꺼진 채 시작한다 | judged | screenshot + log: 재시작 복원 캡처와 클램프 테스트 결과 |
| AC7 | Settings 토글, 메뉴바 메뉴, 글로벌 단축키, URL scheme 중 어느 것으로 토글해도 펫이 즉시 나타나거나 사라지고, 네 표면이 보여주는 상태가 서로 일치한다. 단축키는 캡처 UI로 설정·재설정되고, 빈 값이면 아무것도 등록되지 않으며, 등록 충돌 시 명시적 오류 메시지가 보인다 | judged | screenshot: 4개 토글 표면 일치 캡처 |
| AC8 | `herdr-ide://hide`, `herdr-ide://show`, `herdr-ide://toggle`이 동작하고 뒤 슬래시가 붙은 표기도 같은 명령으로 파싱된다. `herdr-pet://` 호출은 herdr-ide가 받지 않는다(scheme 미등록) | judged | screenshot + log: scheme 명령 동작 캡처와 파싱 테스트 결과 |

## 8. PRD-Level Tasks

- T1. animated webp 재생 스파이크: default 테마 아트를 Swift에서 애니메이션 재생하는 방식을 확정하고, 실패 시 프레임 시퀀스 변환 대체 경로를 확정한다. Covers R1. Depends on: none.
- T2. 코어 이식: ported-reference의 `_new` 판정·스냅샷 파싱, 상태 집계(Disconnected 포함), pose 우선순위를 기존 테스트와 함께 herdr-core로 이식하고, 깨진 스냅샷 처리(전체=실패 폴링, 개별=제외+구조화 로그)와 oldest-unseen 판정 키를 구현한다. Covers R2, R3, R4, R5. Depends on: none.
- T3. C ABI 노출: 스냅샷 JSON에 펫 상태를, dispatch에 펫 이벤트(클릭, 토글, 위치)를 추가하고 기존 6함수 계약을 유지한다. Covers R1, R5, R6, R7. Depends on: T2.
- T4. SwiftUI 펫 렌더러: 테마 로더(theme.json + default 테마 번들)와 pose 애니메이션·뱃지 행 렌더로 자리표시자 오버레이를 교체한다. Covers R1, R2, R8. Depends on: T1, T3.
- T5. 클릭 점프: 펫 클릭 → 메인 윈도우 포커스 + 선택 pane 점프를 연결한다. Covers R5. Depends on: T4.
- T6. 이동·영속: 드래그, 클램프(기존 PetPlacement 재사용), 위치·표시 상태 영속과 복원(연결 모니터 신뢰 규칙, 사고 좌표 픽스처 테스트 이식)을 완성한다. Covers R6. Depends on: T4.
- T7. 토글 표면 4종: Settings Pet 섹션, 메뉴바 아이콘과 메뉴, 글로벌 단축키(캡처 설정, 물리 키 판정), `herdr-ide://` scheme 등록과 관용 파싱을 단일 표시 상태 위에 구현한다. Covers R7. Depends on: T6.
- T8. 정리와 문서: 이식이 끝난 `docs/ported-reference/` 원본은 같은 변경에서 삭제하고(engineering 원칙 1), PORTING.md에 이식 완료를 기록하며, dev-runtime.md 등 남은 Tauri 전제를 Swift 셸 기준으로 갱신한다. 순수 release hygiene. Depends on: T7.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust·Swift 빌드와 정적 검사 건강 | none |
| automated behavior | yes | 이식 로직·판정 키·클램프·파싱의 회귀 | none |
| browser/runtime | yes | 설치 번들에서의 실제 펫 동작(스크린샷 증거) | 최종 비주얼 판단 |

browser/runtime은 이 데스크톱 앱에서는 dev 빌드 스크립트로 만든 설치 번들을 실제 실행하고 스크린샷으로 증명하는 것을 뜻한다(코드 검사·프로세스 생존 확인으로 대체 불가).
live external API 모드는 없다: 유일한 외부 경계인 herdr 서버는 로컬 소켓이고 검증 중 에이전트가 직접 띄운다.

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R8 | Rust(herdr-core)와 Swift(macos) 빌드·정적 검사가 회귀하지 않는다 | yes | no |
| V2 | automated behavior | R1, R2, R3, R4, R5, AC1, AC2, AC3, AC4, AC5 | 이식된 코어 테스트가 다음의 회귀를 잡는다: pose 우선순위 사다리(error > notification > ... > sleeping) 전체와 시간 전이(idle 8초 후 roam 허용, 60초 후 수면 시퀀스), 상태 버킷별 뱃지 수와 ambient subagent/background-task 집계, `_new` 승격(acknowledged 비승격·legacy boolean 수용), Disconnected 미집계, 깨진 스냅샷 처리(전체=유지, 개별=제외+로그), oldest-unseen 판정 키와 pane 순서 fallback | yes | no |
| V3 | automated behavior | R6, R7, R8, AC6, AC7, AC8 | 셸 측 테스트가 다음의 회귀를 잡는다: 클램프·위치 신뢰(사고 좌표 픽스처 포함), `herdr-ide://` 관용 파싱과 미지원 scheme(`herdr-pet://` 형태) 비수용, 단축키 accelerator의 물리 키 기반 판정과 빈 값 = 미등록 규칙, 테마 로더(theme.json 로드·에셋 해석) | yes | no |
| V4 | browser/runtime | SC1, R1, R2, AC1, AC2, AC4, AC5 | 설치 번들에서 스크린샷으로 증명된다: 서로 다른 에이전트 상태 각각에 대한 pose 표시와 우선순위 반영, idle 8초 후 roam·60초 후 수면 전이의 실제 발생, 뱃지 행의 버킷별 수와 ambient 뱃지, 상태 변화가 다음 폴링 주기 안에 반영됨, 클릭 3케이스(oldest 선택, 재시작 fallback, unseen 없음), 연결 끊김 표시와 서버 재시작 후 자동 복구 | yes | no |
| V5 | browser/runtime | SC2, R6, R7, AC6, AC7, AC8 | 설치 번들에서 스크린샷으로 증명된다: 드래그 이동과 재시작 후 위치·표시 상태 복원, 4개 토글 표면(Settings, 메뉴바, 단축키, URL scheme) 각각의 즉시 반영과 상호 상태 일치, 단축키 캡처·재설정 UI 동작, 단축키 미설정 시 미등록, 등록 충돌 시 명시적 오류 표시, `herdr-ide://` 각 명령의 동작과 `herdr-pet://` 호출이 펫에 아무 영향도 주지 않음(scheme 미등록의 부정 확인), 메뉴바 메뉴(Show/Hide, Pet Settings) 동작 | yes | no |

### 9.3 Human Verification

- 펫 애니메이션·뱃지의 최종 비주얼 품질과 감성 판단(아트 렌더가 "herdr-pet답게" 보이는가)은 스크린샷 증거를 보고 사용자가 확정한다.
- herdr-pet 로컬 앱 제거·저장소 아카이브 착수 여부는 이 구현의 검증 통과 후 사용자가 별도로 결정한다(D-12, 이번 범위 밖).

## 10. Risks And Open Decisions

- RF1. animated webp를 Swift에서 재생하지 못할 수 있다. 대응: T1 스파이크를 최우선으로 돌리고, 실패 시 프레임 시퀀스 변환으로 대체한다. 이 대체 경로는 사용자 승인이 아니라 agent 소유 가정(D-15)이며, 스파이크가 실패하면 결과 보고에 그 사실과 선택한 경로를 명시한다.
- RF2. 글로벌 단축키가 다른 앱과 충돌하거나 macOS 권한에 걸릴 수 있다. 대응: 패리티 계약대로 충돌을 명시적 오류로 노출하고 기본은 미등록이므로 실패해도 다른 3개 토글 표면이 살아 있다.
- RF3. oldest-unseen 관찰 기록은 메모리라 앱 재시작 후 선택 순서가 바뀔 수 있다. 이것은 승인된 동작(pane 순서 fallback, D-21)이며 결함이 아니다.
- RF4. 검증 중 실제 에이전트 상태 연출이 번거로울 수 있다. 대응: SC1 Reach에 따라 기존 fixture 경로 확장을 T2 범위에서 흡수한다.
- Open decision 없음. 남은 사용자 판단은 9.3의 비주얼 확정과 은퇴 후속뿐이다.

## 11. Implementation Guardrails

- 승인된 범위를 확장하지 않는다. 특히 question payload(#8) 기능·herdr-agent-pet 플러그인을 이번 범위에 들이지 않는다 (D-07).
- `../herdr-pet` 저장소를 변경하지 않는다 (D-12). `spikes/` 디렉토리는 동결 기록이므로 수정하지 않는다 (저장소 규칙).
- C ABI는 기존 6함수 계약 안에서 스냅샷·이벤트 확장으로만 넓힌다. 새 ABI 함수, 새 외부 서비스, 새 저장소, 스키마 마이그레이션을 도입하지 않는다.
- `herdr-pet://` scheme을 등록하지 않는다 (D-10).
- herdr 서버와의 소켓 계약은 소비만 하고 변경하지 않는다.
- 이식이 대체하는 것은 같은 변경에서 삭제한다: 자리표시자 오버레이의 죽은 경로, 이식 완료된 ported-reference 원본 (engineering/principles.md rule 1).
- 무효 상태를 조용한 기본값으로 덮지 않는다: 소켓 부재·깨진 스냅샷은 명시 상태와 구조화 로그로 표면화한다 (engineering rules 4, 9, 10; D-20).
- 토글·폴링·위치 저장은 반복 실행이 수렴하게 만든다: 같은 토글 이벤트 두 번, 같은 스냅샷 두 번, 같은 위치 저장 두 번이 상태를 깨뜨리지 않는다 (engineering rule 11).
- 기존 것을 먼저 쓴다: PetPlacement 클램프, live.rs polling, persistence 계층, 기존 Settings 씬 패턴을 확장하고 병렬 구현을 만들지 않는다 (engineering rule 7; design rule 5).
- 상태는 시각으로 인코딩한다: pose·뱃지·색이 1차 표현이고 설명 문장은 최후 수단이다. 가장 잦은 동작(토글, attention 점프)은 1클릭을 유지한다 (design rules 3, 7).
- 테스트는 호출자 관점 결과를 단언한다: 이식 테스트의 기존 단언을 유지하고 내부 배선 단언을 추가하지 않는다 (engineering rule 12).

## 12. Implementation Result Report Contract

구현 에이전트는 다음을 보고한다:

- status: Done / Partially Done / Blocked.
- 사용자 가시 변경: 펫 창, 뱃지, 클릭 동작, 4개 토글 표면, 메뉴바, Settings 섹션.
- 주요 변경 모듈: herdr-core 펫 모듈, C ABI 스냅샷/이벤트 형태, macos 셸 구성. 실제 선택한 파일·모듈 구조와 책임 경계.
- 승인된 기술 구조(5장)를 따랐는지, 벗어난 지점과 사유.
- T1-T8 완료 상태와 R/AC/V 커버리지.
- 모드별 검증 증거: 자동 테스트 결과(각 테스트가 막는 회귀 명시), 설치 번들 스크린샷 세트(V4·V5의 각 케이스).
- T1 스파이크 결론(webp 직접 재생 vs 변환 대체).
- deviations와 남은 인간 검토(9.3), 미완 항목과 후속 후보(은퇴 절차 포함).
- delivery: local 모드이므로 커밋 기준 보고(브랜치·PR 없음).
