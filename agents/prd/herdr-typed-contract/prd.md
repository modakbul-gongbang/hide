---
topic: "herdr-typed-contract"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "코어 내부의 Herdr wire 파싱을 생성 타입으로 옮기는 리팩토링으로, 사용자 동작과 외부 효과는 그대로이고 PR 머지만 외부로 나간다."
source_intake: "current conversation"
created_at: "2026-09-06"
updated_at: "2026-09-06"
---

# PRD: herdr-typed-contract

## 1. Summary

hide 코어는 `contracts/herdr-api.schema.json`(핀 Herdr가 답한 JSON Schema, protocol 21)을 갖고 있지만, 그 스키마를 코드로 쓰지 않는다.
`session_sync.rs`는 `WorkspaceWire`, `TabWire`, `PaneWire`, `WireAgent`, `ProjectionState`, 스무 개 남짓의 `*Event` 구조체를 손으로 다시 적고, 그 사이를 `serde_json::Value` 탐색 40곳이 메운다.
스키마가 바뀌면 손으로 쓴 구조체는 조용히 어긋나고, 빌드는 그것을 잡지 못한다.
이 PRD는 `build.rs`에서 `typify`로 스키마의 다섯 하위 스키마(request, success_response, event, subscription_event, error_response)마다 Rust 타입을 생성하고, `session_sync.rs`가 손으로 쓴 wire 구조체와 `Value` 탐색 대신 생성 타입을 하나의 경계 모듈을 통해 쓰게 한다.
`live.rs`와 `remote.rs`는 다음 사이클(A3)에서 같은 경계로 옮긴다.

Approval checklist:

- 범위 경계: `build.rs` 타입 생성 + `session_sync.rs`의 wire 파싱을 생성 타입으로 교체 + 경계 모듈 신설. `live.rs`, `remote.rs`는 non-goal (3장).
- 구조 변경: 생성 코드는 `OUT_DIR`에만 있고 커밋되지 않는다. 경계 모듈 하나가 생성 타입과 도메인 타입 사이의 변환을 독점한다 (5장).
- 의존성 추가: `typify`(build-dependency), `regress`(dependency). 둘 다 정확한 버전으로 핀 (4.3 A2).
- 검증 모드: build/static, automated behavior, desktop runtime(사이클 1의 격리 e2e 스크립트 재사용) required-for-done (9장).
- delivery mode: `pr`, squash 머지까지 위임 (4.3 D3).

## 2. Problem, Goal, And Users

사용자(hide 개발자)는 Herdr API와 코어 사이의 이원화를 없애고 싶어 한다("이원화 시러서", "typify 함 알아봐.. 그게 나을듯? schema gen").
목표는 스키마가 유일한 타입 출처가 되어, 핀을 옮겨 계약이 바뀌면 컴파일 에러가 어긋난 자리를 정확히 가리키게 하는 것이다.
사용자에게 보이는 동작은 바뀌지 않는다: 앱은 같은 Herdr에 같은 방식으로 붙고, 같은 스냅샷과 이벤트를 같은 프로젝션으로 만든다.

### 2.1 User Scenarios

- SC1. 무변화 확인: 사용자가 hide를 열면 번들 Herdr가 시작되고 연결되어 workspace와 pane이 그려지며, pane을 만들거나 닫으면 그 변화가 이벤트를 통해 화면에 반영된다.
  Actors: 사용자 (hide 운영자).
  Primary path: 격리 소켓에서 앱이 자체 Herdr에 연결해 pane 하나 이상을 답하고, 이벤트 구독이 이어진다.
  Failure state: 다른 프로토콜의 서버가 소켓을 점유하면 프로토콜 불일치 상태가 진단에 기록된다.
  Recovery: 그 서버를 멈추고 다시 열면 primary path로 돌아온다.
  Reach: 사이클 1이 저장소에 둔 격리 e2e 스크립트(`scripts/check-herdr-e2e.py`)가 세 경로를 만든다.

## 3. Scope And Non-Goals

포함:

- `herdr-core/build.rs`가 계약 파일의 다섯 하위 스키마를 분리하고 `#/schemas/<name>/$defs/X` 참조를 `#/$defs/X`로 고쳐 `typify`로 각각 생성하며, 하위 스키마마다 모듈 하나로 `OUT_DIR`에 쓴다. 기존 `HERDR_PROTOCOL_REVISION`, `HERDR_API_SCHEMA_VERSION` 상수는 그대로 남는다.
- 생성 모듈이 `herdr_contract.rs` 아래 `wire` 모듈로 노출되고, 생성 코드가 요구하는 `regress`가 의존성에 추가된다.
- 경계 모듈(anti-corruption layer) 하나가 생성 wire 타입을 코어의 도메인 타입(`ProjectionState`와 그 구성요소, 이벤트 적용 입력, `agent.list` 결과, `events.subscribe` 파라미터)으로 바꾸는 변환을 독점한다. 도메인 코드는 생성 타입을 직접 쓰지 않는다.
- `session_sync.rs`의 손으로 쓴 wire 구조체와 `Value` 탐색을 경계 모듈 호출로 대체한다. 프로토콜 검증, 스냅샷 필수 필드 검증, 이벤트 시퀀스 커서, 구독 오류 처리는 같은 진단 메시지와 같은 실패 상태를 유지한다.
- 기존 fixture(JSON)를 그대로 쓰는 테스트가 계속 통과하고, 모든 fixture가 생성 타입으로 역직렬화됨을 단언하는 테스트가 추가된다.
- `AGENTS.md`의 Herdr 계약 문단이 "계약은 코드로 생성된다, 손으로 wire 구조체를 적지 말라"를 말한다.

Non-goals:

- `live.rs`(34곳)와 `remote.rs`(20곳)의 `Value` 탐색 교체. 다음 사이클. 결과: 그동안 두 파일은 손 파싱을 유지한다. 재검토: 이 PR 머지 직후.
- `Request` 타입의 태그 판별. typify는 `method` const를 판별자로 쓰지 않아 `Variant0..`으로 나오므로, 요청 봉투는 계속 손으로 만들고 `*Params` 구조체만 쓴다.
- 생성 코드를 저장소에 커밋하는 것. 빌드마다 스키마에서 만든다.
- 스키마나 핀 변경. 계약은 사이클 1의 것 그대로다.
- 성능 최적화. 프로젝션 재계산 경로와 mutex 규율(AGENTS.md Performance Guide)은 건드리지 않는다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

- None required.

### 4.2 Human Decisions Before PRD Approval

- None required. 방향은 사용자가 대화에서 골랐고("typify 함 알아봐.. 그게 나을듯?", "오케이 우선 너 추천방향대로 싹다 리팩토링"), 나머지는 되돌릴 수 있는 가정으로 4.3에 기록했다.

### 4.3 Decision Traceability For Fidelity Review

사용자 결정 (대화 원문):

- D1. "typify 함 알아봐.. 그게 나을듯? schema gen" 및 "오케이 우선 너 추천방향대로 싹다 리팩토링": 스키마에서 타입을 생성하고 `Value` 탐색을 매핑 레이어로 대체한다. 표현: R1-R5, T1-T5.
- D2. "그런데 herdr-api.schema.json은 써야겠지? ... 이원화 시러서": 계약 파일은 유지하되 그것이 유일한 타입 출처가 된다. 표현: R1, 5장, non-goal(생성 코드 미커밋).
- D3. "최종적으로 모든 작업들 다 완수시켜": PR 생성/머지까지 위임. 표현: delivery `pr`, 12장.
- D4. "codex astra medium으로 implementor 띄워서": Implementor는 Codex `gpt-6-astra`, effort medium. context-only.

에이전트 가정 (사후 거부 가능):

- A1. 생성은 `build.rs`에서 `typify` 크레이트로 하고 `cargo typify` 출력물을 커밋하지 않는다. 2026-09-06 실험(스크래치)에서 다섯 하위 스키마가 각각 113+79+25+11+2 타입으로 생성되고 라이브 스냅샷, `agent.list`, 이벤트 413줄이 오류 없이 역직렬화됐다. 표현: R1, 5장.
- A2. 의존성은 `typify = "=0.7.0"`(build), `regress = "=0.12.0"`(runtime)으로 정확히 핀한다. 저장소의 모든 의존성이 `=` 핀이다. 표현: R1.
- A3. 이 사이클은 `session_sync.rs`까지만 옮긴다. 생성 타입에 소비자가 없는 상태로 끝내지 않기 위해 typify와 첫 소비자를 한 사이클로 묶고, `live.rs`/`remote.rs`는 다음 사이클로 미룬다. 표현: non-goal.
- A4. 경계 모듈은 `herdr-core/src/wire.rs` 같은 단일 파일이며, 생성 타입 → 도메인 타입 변환(`TryFrom` 또는 명명 함수)과 요청 파라미터 생성만 담는다. 도메인 타입의 이름과 필드는 유지해 `runtime.rs`, `domain.rs`, `sidebar.rs`의 호출자가 바뀌지 않게 한다. 표현: R2, R3, 5장.
- A5. 생성 타입이 패턴 검증 newtype(예: pane id)을 만들면 변환은 경계 모듈에서 하고, 검증 실패는 기존 `SessionFetchError::Malformed`로 표면화한다. 표현: R3, AC4.
- A6. 컴파일 시간 증가(실험에서 디버그 약 6초)는 수용한다. 표현: 10장.

Principles intake: `~/projects/oh-my-principle` 커밋 35ab76c의 `engineering/principles.md`, `practices/env.md`, `practices/test.md`를 읽었다.
design 도메인은 화면을 바꾸지 않으므로 적용하지 않는다.

## 5. Major Technical Structure Changes

- 빌드 경계: `build.rs`가 계약 스키마에서 wire 타입을 생성한다. 생성물은 `OUT_DIR`에만 존재한다.
- 새 모듈 경계: 경계 모듈이 생성 wire 타입과 도메인 타입 사이의 유일한 변환 지점이 된다. `session_sync.rs`의 손으로 쓴 wire 구조체는 삭제된다.
- 의존성: `typify`(build), `regress`(runtime) 추가. 새 서비스, 새 프로세스, 새 외부 호출 없음.
- 런타임 아키텍처(단일 `Mutex<Runtime>`, 스냅샷 델타, 알림기)는 변경 없음.

## 6. Requirements

- R1. `cargo build -p herdr-core`가 계약 파일만으로 다섯 하위 스키마의 Rust 타입을 생성하고, 계약 파일이 바뀌면 재생성한다. 생성 실패(참조 미해결, 스키마 파싱 실패)는 빌드 실패로 표면화된다.
- R2. `session_sync.rs`에 손으로 쓴 wire 구조체(`WorkspaceWire`, `WorkspaceWorktreeWire`, `TabWire`, `PaneWire`, `WireAgent`, `AgentListResult`, `SequencedEventEnvelope`, `*Event` 구조체들)와 `serde_json::Value` 탐색이 남아 있지 않다. 스냅샷, 이벤트, `agent.list` 응답, 구독 라인은 생성 타입으로 역직렬화된 뒤 경계 모듈이 도메인 타입으로 바꾼다.
- R3. 프로토콜 불일치, 필수 필드 누락, 잘못된 host 식별자, 구독 오류의 진단 메시지와 상태(`protocol_mismatch` 등)가 이전과 같다. `SNAPSHOT_FIELDS_THE_REPLICA_READS`와 계약 필드 테스트는 유지된다.
- R4. `events.subscribe` 파라미터(`after_sequence`, `subscriptions`)는 생성 `*Params` 타입으로 만든다.
- R5. 기존 session_sync 테스트와 fixture가 그대로 통과하고, 모든 session_sync fixture가 생성 타입으로 역직렬화됨을 단언하는 테스트가 있다. 보호하는 회귀: fixture가 계약과 어긋난 채 손 파서만 통과하는 것.
- R6. 앱의 사용자 가시 동작(SC1)이 변하지 않는다: 격리 e2e의 세 경로가 통과한다.
- R7. `AGENTS.md`가 타입 생성과 경계 모듈 규칙을 말하고, 커밋/PR 텍스트에 에이전트 귀속이 없다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | 코어 빌드가 계약 파일에서 다섯 하위 스키마의 타입을 생성하고, 계약의 `SessionSnapshot`, `EventEnvelope`, `AgentListResult`에 해당하는 생성 타입이 코어 안에서 참조된다 | machine | - |
| AC2 | `session_sync.rs`에 손으로 쓴 wire 구조체 이름과 `serde_json::Value` 탐색 호출(`.get("`, `as_array()`, `as_str()` 등 wire 필드 접근)이 없다 | machine | - |
| AC3 | 도메인 타입과 생성 타입 사이의 변환이 경계 모듈 한 곳에만 있고, `runtime.rs`, `domain.rs`, `sidebar.rs`는 생성 타입을 참조하지 않는다 | machine | - |
| AC4 | 프로토콜 불일치, 필수 필드 누락, 빈 host 식별자, 구독 오류에 대한 기존 테스트가 같은 메시지와 상태로 통과한다 | machine | - |
| AC5 | 모든 session_sync fixture가 생성 타입으로 역직렬화된다는 테스트가 통과한다 | machine | - |
| AC6 | 코어 테스트 전체, ffi 계약 테스트, Swift 테스트, 핀 단일 출처 게이트, 스키마 게이트가 통과한다 | machine | - |
| AC7 | 격리 e2e의 primary/failure/recovery 세 경로가 통과한다 | machine | - |
| AC8 | 경계 모듈이 생성 타입을 도메인으로 바꾸는 방식이 읽는 사람에게 명확하고, 손 파싱의 진단 의미를 잃지 않았다 | judged | 경계 모듈 전문과 session_sync 변경 diff |
| AC9 | 커밋, 브랜치, PR 본문에 에이전트/모델/도구 귀속이 없다 | machine | - |

## 8. PRD-Level Tasks

- T1. `build.rs`에 typify 생성을 넣고 `regress`를 추가하여 다섯 모듈이 컴파일되게 한다. Covers R1, AC1. Depends on: none.
- T2. 경계 모듈을 만들어 스냅샷, `agent.list`, 이벤트 봉투, 구독 파라미터의 변환을 정의한다. Covers R2, R3, R4, AC3. Depends on: T1.
- T3. `session_sync.rs`를 경계 모듈로 옮기고 손으로 쓴 wire 구조체와 `Value` 탐색을 삭제한다. Covers R2, R3, AC2, AC4. Depends on: T2.
- T4. fixture 역직렬화 테스트를 추가하고 전 스위트와 게이트, 격리 e2e를 통과시킨다. Covers R5, R6, AC5, AC6, AC7. Depends on: T3.
- T5. `AGENTS.md`를 갱신하고 귀속 검사와 PR 근거를 정리한다. Covers R7, AC8, AC9. Depends on: T4.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | 코어/Swift 빌드, 게이트 스크립트, 구조 검사, 귀속 검사 | none |
| automated behavior | yes | 코어/ffi/Swift 테스트, fixture 역직렬화 | none |
| desktop runtime | yes | SC1 격리 e2e | none |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1, R2, AC1, AC2, AC3, AC6 | 코어 release 빌드와 Swift 빌드가 통과하고, 손 파싱 부재와 경계 단일성이 소스 검사로 확인되며, 핀/스키마 게이트가 통과한다 | yes | no |
| V2 | automated behavior | R3, R5, AC4, AC5, AC6 | 코어 테스트(기존 진단 테스트와 새 fixture 테스트 포함), ffi, Swift 테스트가 통과한다. 보호하는 회귀: 계약과 fixture의 어긋남, 진단 메시지 변질 | yes | no |
| V3 | desktop runtime | R6, AC7, SC1 | 격리 e2e 스크립트가 세 경로를 통과한다 | yes | no |
| V4 | build/static | R7, AC8, AC9 | 경계 모듈과 diff가 AC8을 만족하고 귀속 검색이 0건이다 | yes | no |

### 9.3 Human Verification

- None required. 사용자 가시 동작이 바뀌지 않으며 모든 증거가 기계 검사와 판정 diff로 남는다.

## 10. Risks And Open Decisions

- typify가 만든 타입 이름이 스키마 제목에 따라 예상과 다를 수 있다. 경계 모듈이 이름 차이를 흡수하고, 생성 코드는 `OUT_DIR`에서 직접 읽어 확인한다.
- 패턴 검증 newtype이 fixture의 느슨한 값(예: 테스트용 id)을 거부할 수 있다. fixture를 계약에 맞게 고치는 것이 옳고, 검증을 끄지 않는다.
- 컴파일 시간이 늘어난다(A6). 생성 모듈은 다섯 개로 분리되어 있어 변경 없는 빌드는 캐시된다.
- `SessionReplica::apply`의 이벤트 분기가 문자열 매칭에서 enum 매칭으로 바뀌며 누락된 variant는 컴파일 에러로 드러난다. 처리하지 않던 이벤트는 명시적 no-op 분기로 남기고 진단은 유지한다.

## 11. Implementation Guardrails

- 범위를 넓히지 않는다: `live.rs`, `remote.rs`는 건드리지 않는다. 다음 사이클이다.
- 생성 코드를 커밋하지 않는다. 계약 파일과 핀을 바꾸지 않는다.
- 사용자의 실행 중 Herdr 서버와 기본 소켓을 건드리지 않는다. e2e는 항상 `HERDR_SOCKET_PATH` 격리 경로에서만 돈다.
- 로컬 `main`(다른 세션의 미푸시 커밋)을 base로 쓰지 않는다. base는 `origin/main`(2c7caf0 이후)이다.
- 커밋, 브랜치, PR 본문에 에이전트/모델/도구 귀속을 넣지 않는다.
- run 산출물은 `agents/runs/herdr-typed-contract/` 아래에만 둔다.
- 원칙 번역 (engineering/principles.md, 35ab76c): 규칙 1: 손으로 쓴 wire 구조체와 그 헬퍼는 같은 변경에서 삭제한다. 규칙 2: 경계 모듈은 필요한 변환만 갖고 제네릭 추상화를 만들지 않는다. 규칙 4: 변환 실패는 기존 오류 타입으로 표면화하고 기본값으로 덮지 않는다. 규칙 5: 도메인은 생성 타입을 모른다. 규칙 6/7: 직접 파서를 쓰지 말고 typify와 serde를 쓴다. 규칙 12: 새 테스트는 fixture 역직렬화 결과와 진단 메시지를 단언한다. 규칙 13: 이벤트 분기는 문자열 매칭이 아니라 생성 enum 매칭으로 한다. practices/env.md: 새 환경 변수 없음. practices/test.md: 테스트는 회귀 위험을 이름 붙여 정당화한다(R5).

## 12. Implementation Result Report Contract

구현 에이전트는 다음을 보고한다:

- status와 사용자 가시 변화(없음이 기대값).
- 생성 모듈 목록과 타입 수, 경계 모듈의 위치와 변환 목록, 삭제된 손 구조체 목록.
- 승인된 구조를 따랐는지, 도메인 타입 이름/필드 유지 여부.
- task 완료 상태, R/AC/V 커버리지, 모드별 증거(명령, 종료 코드).
- 추가한 테스트와 각 테스트가 막는 회귀.
- delivery: 브랜치, PR URL, CI 상태, 머지 커밋.
- 가정과 편차, 남은 사람 확인 항목, 다음 사이클(`live.rs`, `remote.rs`)로 넘길 것.
