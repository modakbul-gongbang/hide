---
topic: "herdr-typed-live-remote"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "live.rs와 remote.rs의 손 파싱을 사이클 2가 만든 생성 타입 경계로 옮기는 리팩토링으로, 사용자 동작과 외부 효과는 그대로이고 PR 머지만 외부로 나간다."
source_intake: "current conversation"
created_at: "2026-09-06"
updated_at: "2026-09-06"
---

# PRD: herdr-typed-live-remote

## 1. Summary

사이클 2(PR #12, `27e6e2e`)는 `contracts/herdr-api.schema.json`에서 typify로 Rust 타입을 생성하고, `session_sync.rs`를 경계 모듈 `herdr-core/src/wire.rs` 하나를 통해 그 타입 위에 올렸다.
그 PRD가 non-goal로 미룬 두 파일이 남아 있다.
`live.rs`는 `workspace.create`, `tab.create`, `tab.move`, `pane.split`, `pane.layout`, `pane.read`, `session.snapshot` 응답과 `herdr agent new` CLI 출력을 `serde_json::Value` 포인터와 `.get("...")`로 더듬고, 요청 본문을 `json!`로 손으로 만든다.
`remote.rs`는 SSH 터널 너머 Herdr의 `session.snapshot` 응답을 `decode_remote_snapshot`에서 손으로 읽어 `RemoteSnapshotEnvelope`를 만든다.
이 PRD는 두 파일의 프로덕션 코드에서 `Value` 탐색과 손 요청 본문을 없애고, 응답 디코딩과 요청 파라미터 생성을 `wire.rs`로 옮긴다.
기존 진단(원격 stage/retryable/action_required 분류, 프로토콜 불일치 문구, 누락 필드 문구)은 그대로다.

Approval checklist:

- 범위 경계: `live.rs`와 `remote.rs`의 프로덕션 코드가 `wire.rs`를 통해서만 Herdr wire를 읽고 쓴다. 테스트 모듈 안의 fixture 단언은 범위 밖이다 (3장).
- 구조 변경: 새 모듈 없음. `wire.rs`가 응답 변형 선택과 요청 파라미터 생성을 추가로 갖는다 (5장).
- 의존성 추가: 없음.
- 검증 모드: build/static, automated behavior, desktop runtime(격리 e2e + 격리 서버 프로브) required-for-done (9장).
- delivery mode: `pr`, squash 머지까지 위임 (4.3 D3).

## 2. Problem, Goal, And Users

사용자(hide 개발자)는 Herdr API와 코어 사이의 이원화를 없애고 싶어 한다("이원화 시러서", "오케이 우선 너 추천방향대로 싹다 리팩토링").
사이클 2가 끝난 지금, 계약이 바뀌면 `session_sync.rs`는 컴파일 에러로 어긋난 자리를 가리키지만 `live.rs`와 `remote.rs`는 런타임에 "response is missing …"으로만 실패한다.
목표는 코어 전체에서 스키마가 유일한 wire 타입 출처가 되는 것이다.
사용자에게 보이는 동작은 바뀌지 않는다: pane 만들기/닫기/분할/포커스/줌, 원격 호스트의 스냅샷 동기화, 에이전트 fork가 같은 요청을 보내고 같은 답을 같은 방식으로 해석한다.

### 2.1 User Scenarios

- SC1. 로컬 pane 제어 무변화: 사용자가 hide에서 pane을 분할하거나 탭을 만들면 Herdr가 답한 새 pane/tab id로 화면이 이어진다.
  Actors: 사용자 (hide 운영자).
  Primary path: 격리 소켓의 핀 Herdr에 대해 `workspace.create`, `tab.create`, `tab.move`, `pane.split`, `pane.layout`, `pane.read`, `session.snapshot` 응답이 생성 타입으로 디코딩되어 같은 id와 같은 레이아웃을 돌려준다.
  Failure state: Herdr가 다른 변형(예: 오류 응답)을 답하면 기존과 같은 "<method> response is missing …" 문구로 실패한다.
  Recovery: 재시도는 기존 호출자의 몫이며 이 PRD는 바꾸지 않는다.
  Reach: 격리 서버 프로브(9장 V3)가 만든다.
- SC2. 원격 스냅샷 무변화: 사용자가 SSH 별칭으로 원격 Herdr에 붙으면 스냅샷이 host/sequence 검사를 거쳐 프로젝션에 반영되고, 프로토콜이 다른 원격은 Protocol 단계 진단으로 남는다.
  Actors: 사용자 (원격 호스트 운영자).
  Primary path: `decode_remote_snapshot`이 생성 `SessionSnapshot`으로 읽고 같은 `RemoteSnapshotEnvelope`를 만든다.
  Failure state: 프로토콜 불일치는 stage=Protocol, retryable=false, action_required=true로, 형식 오류는 기존 분류 그대로 남는다.
  Recovery: 기존 `Stale` 처리 그대로.
  Reach: 기존 remote 테스트가 만든다.
- SC3. 앱 연결 무변화: 사용자가 hide를 열면 번들 Herdr가 시작되어 연결되고, 프로토콜이 다른 서버가 소켓을 점유하면 진단이 남고, 그 서버를 멈추면 회복된다.
  Reach: 사이클 1의 격리 e2e 스크립트(`scripts/check-herdr-e2e.py`)가 세 경로를 만든다.

## 3. Scope And Non-Goals

포함:

- `live.rs` 프로덕션 코드(테스트 모듈 밖)의 모든 `serde_json::Value` 탐색을 `wire.rs` 호출로 바꾼다. 대상: `create_herdr_workspace`(`workspace.create` → root pane id), `execute_remote_control`(`tab.create` → tab id와 root pane id, `tab.move` → tab id 목록), `execute_pane_control`(`pane.split` → 만든 pane id), `fetch_pane_layout`(`pane.layout` → 레이아웃), `read_pane_text`(`pane.read` → text/truncated), `fetch_session_with_connector`(`session.snapshot`), `run_agent_fork`/`forked_pane_id`(`herdr agent new` 출력 → 만든 pane id).
- `live.rs`가 보내는 요청 파라미터를 생성 `*Params` 타입(`WorkspaceCreateParams`, `TabCreateParams`, `TabMoveParams`, `PaneSplitParams`, `PaneResizeParams`, `PaneLayoutParams`, `PaneReadParams`, `PaneZoomParams` 등 스키마가 가진 것)으로 만든다. 스키마에 파라미터 타입이 없는 메서드는 사이클 2와 같은 규칙으로 `wire.rs` 안에서만 손으로 만들고, 스키마가 따라잡으면 실패하는 테스트를 둔다.
- `forked_pane_id`의 네 포인터 탐색을 생성 응답 타입 디코딩으로 바꾼다. 핀 바이너리가 실제로 답하는 모양만 남긴다: 격리 서버에서 그 명령을 실제로 실행해 출력을 잡고, 그 출력이 생성 `SuccessResponse`/`ResponseResult`로 디코딩됨을 테스트로 고정한다. 실제 출력이 API 봉투가 아니라면 그 봉투는 `wire.rs` 안에서만 손으로 적고, 스키마가 그 모양을 선언하면 실패하는 테스트를 둔다.
- `remote.rs`의 `decode_remote_snapshot`, `required_string`, `string_ids`, `optional_string_ids`를 `wire.rs`의 변환으로 바꾼다. `RemoteSnapshotEnvelope`는 도메인 타입으로 남고, 정렬된 id 목록, 중복 id 거부, host/event_sequence 필수, 프로토콜 검사의 의미와 `RemoteDiagnostic`의 stage/retryable/action_required 값은 그대로다. 프로토콜은 전체 타입 디코딩 전에 검사해, 다른 프로토콜의 원격이 Malformed가 아니라 Protocol 단계로 보고되게 한다.
- `apply_wire_snapshot`과 `fetch_herdr_snapshot_value`의 경계에서 `Value`가 사라지거나 `wire.rs` 안으로 들어간다. 도메인 코드는 생성 타입을 직접 쓰지 않는다.
- 사이클 2의 구조 검사 스크립트를 `live.rs`와 `remote.rs`까지 넓히고, 두 파일의 모든 JSON fixture가 생성 타입으로 역직렬화됨을 단언하는 테스트를 추가한다.
- 격리 소켓에서 핀 Herdr를 띄워 위 메서드들의 실제 응답과 `herdr agent new` 출력이 생성 타입으로 디코딩됨을 검증하는 프로브를 검사 스크립트에 넣는다.

Non-goals:

- `session_sync.rs`, `runtime.rs`, `domain.rs`, `sidebar.rs`의 변경. 사이클 2가 끝냈다.
- 테스트 모듈 안의 fixture 단언(`snapshot["panes"].as_array()` 같은 검사)의 제거. 결과: 테스트는 JSON을 직접 읽어 단언해도 된다. 재검토: 없음.
- 이벤트 봉투 메타데이터(`sequence`, `host`, `protocol`)의 스키마 선언. 포크의 Herdr 쪽 작업이며 별도 사이클. 결과: `wire.rs`의 `EventMetadata`와 그 보호 테스트는 그대로다.
- `Request` 봉투의 태그 판별. 사이클 2와 같은 이유로 봉투는 손으로 만들고 `*Params`만 쓴다.
- 원격 SFTP/터미널 세션 프로브(`remote_sftp_fixture_probe`, `official_remote_terminal_session_fixture_probe`)와 실제 원격 호스트에 대한 프로브. 소유한 원격 fixture가 없다. 결과: 원격 경로는 fixture 테스트로만 검증된다. 재검토: 원격 fixture가 생기면.
- 스키마나 핀 변경, 성능 최적화, 재시도 정책 변경.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

- None required.

### 4.2 Human Decisions Before PRD Approval

- None required. 방향은 사용자가 대화에서 골랐고("오케이 우선 너 추천방향대로 싹다 리팩토링", "최종적으로 모든 작업들 다 완수시켜"), 나머지는 되돌릴 수 있는 가정으로 4.3에 기록했다.

### 4.3 Decision Traceability For Fidelity Review

사용자 결정 (대화 원문):

- D1. "typify 함 알아봐.. 그게 나을듯? schema gen" 및 "오케이 우선 너 추천방향대로 싹다 리팩토링": 코어 전체의 손 파싱을 생성 타입 경계로 옮긴다. 이 사이클은 그 마지막 두 파일이다. 표현: R1-R5, T1-T5.
- D2. "그런데 herdr-api.schema.json은 써야겠지? ... 이원화 시러서": 계약 파일이 유일한 타입 출처다. 표현: R1, R2.
- D3. "최종적으로 모든 작업들 다 완수시켜": PR 생성/머지까지 위임. 표현: delivery `pr`, 12장.
- D4. "codex astra medium으로 implementor 띄워서": Implementor는 Codex `gpt-6-astra`, effort medium. context-only.

에이전트 가정 (사후 거부 가능):

- A1. 두 파일의 프로덕션 `Value` 탐색은 모두 소켓 API 응답(`ResponseResult` 변형)이거나 `herdr agent new`의 stdout이다. 2026-09-06 `origin/main` 27e6e2e에서 함수별로 분류했다: 비테스트 사이트 12곳은 전부 `control_request`/`request_with_connector`의 결과이거나 CLI stdout이고, 나머지 `.as_str()` 매치는 enum 메서드다. 이 세션에서 관찰한 CLI 출력(`pane split` → `{"id":"cli:pane:split","result":{"pane":…,"type":"pane_info"}}`, `agent start` → `{"id":"cli:agent:start","result":{"agent":…,"argv":[…],"type":"agent_started"}}`)은 API 봉투와 같다. `agent new`의 출력은 프로브로 확인한다. 표현: R1, R3, AC3.
- A2. 생성 구조체 타입은 `PartialEq`를 derive하지 않는다(`Clone, Debug, Deserialize, Serialize`만). 그래서 `RemoteSnapshotEnvelope`와 `PaneLayoutSnapshot` 같은 도메인 타입이 비교와 저장을 맡고, 생성 타입은 `wire.rs`를 벗어나지 않는다. 표현: R2, AC2.
- A3. 원격 경로의 프로토콜 검사는 전체 디코딩 전에 `protocol` 필드만 먼저 읽어 수행한다. 사이클 2의 `validate_snapshot`이 같은 순서다. 표현: R4, AC4.
- A4. 격리 서버 프로브는 `scripts/fetch-herdr-runtime.sh`가 받는 핀 바이너리를 짧은 임시 경로의 `HERDR_SOCKET_PATH`에 띄우고, 끝나면 그 소켓의 서버만 멈춘다. 사용자의 기본 소켓은 건드리지 않는다. 프로브가 `herdr agent new`로 띄우는 에이전트 종류와 명령은 Implementor가 고른다. 표현: V3, AC7.
- A5. 요청 파라미터 타입이 스키마에 없는 메서드(현재 파악: `pane.focus`, `pane.close`, `tab.focus`, `tab.close`, `workspace.focus`, `pane.project`는 `*Target` 또는 별도 정의를 쓸 수 있다)는 Implementor가 생성 모듈에서 확인하고, 없는 것만 `wire.rs` 안에서 손으로 만든다. 표현: R2.
- A6. `live.rs`의 `SessionLayoutPayload` 역직렬화(`pane.layout`)는 생성 `PaneLayoutSnapshot` 타입을 거쳐 기존 `project_layout`으로 이어진다. 기존 레이아웃 프로젝션 테스트가 그대로 통과해야 한다. 표현: R3, AC5.

Principles intake: `~/projects/oh-my-principle` 커밋 35ab76c의 `engineering/principles.md`, `practices/env.md`, `practices/test.md`를 읽었다.
design 도메인은 화면을 바꾸지 않으므로 적용하지 않는다.

## 5. Major Technical Structure Changes

- 모듈 경계: `wire.rs`가 `live.rs`와 `remote.rs`가 쓰는 응답 변형 선택(`ResponseResult` → 도메인 결과)과 요청 파라미터 생성(`*Params` → `Value` 본문)을 추가로 독점한다. 새 모듈은 없다.
- 삭제: `live.rs`의 `forked_pane_id`와 포인터 목록, `remote.rs`의 `required_string`, `string_ids`, `optional_string_ids`와 `decode_remote_snapshot`의 손 파싱 본문.
- 의존성, 프로세스, 외부 호출, 런타임 아키텍처(단일 `Mutex<Runtime>`, 스냅샷 델타, 알림기)는 변경 없음.

## 6. Requirements

- R1. `live.rs`와 `remote.rs`의 프로덕션 코드(`#[cfg(test)]` 밖)에 `serde_json::Value` 탐색(`.get("`, `.pointer(`, `Value::as_*`, `value["key"]` 인덱싱)과 `json!` 요청 본문이 남아 있지 않다. 응답은 생성 타입으로 역직렬화된 뒤 `wire.rs`가 도메인 결과로 바꾼다.
- R2. 생성 타입(`herdr_contract::wire::*`)은 `wire.rs`와 `herdr_contract.rs` 밖에서 참조되지 않는다. `live.rs`와 `remote.rs`는 도메인 타입(`PaneLayoutSnapshot`, `PaneText`, `RemoteSnapshotEnvelope`, id 문자열)만 받는다.
- R3. `herdr agent new` 출력은 핀 바이너리가 실제로 답하는 모양으로 디코딩되고, 그 모양은 격리 서버에서 잡은 출력을 fixture로 삼는 테스트가 고정한다. 증명되지 않은 대체 경로는 남지 않는다.
- R4. 원격 스냅샷 디코딩의 진단이 이전과 같다: 프로토콜 불일치는 stage=Protocol, retryable=false, action_required=true와 기존 문구(`protocol mismatch expected=… received=…`), 누락 필드는 기존 문구, 중복 id는 기존 문구. 프로토콜 검사는 전체 디코딩보다 먼저다.
- R5. `live.rs`의 진단 문구("<method> response is missing …", "pane.layout response is malformed: …", "herdr agent new returned unreadable output: …", "herdr agent new reported no created pane")와 `SessionFetchError` 변형이 유지된다.
- R6. 두 파일의 기존 테스트와 fixture가 그대로 통과하고, 모든 fixture가 생성 타입으로 역직렬화됨을 단언하는 테스트가 있다. 보호하는 회귀: fixture가 계약과 어긋난 채 손 파서만 통과하는 것.
- R7. 격리 소켓의 핀 Herdr에 대해 SC1의 메서드 응답과 `herdr agent new` 출력이 생성 타입으로 디코딩되는 프로브가 통과하고, 격리 e2e 세 경로(SC3)가 통과한다.
- R8. `AGENTS.md`의 Herdr 계약 문단이 코어 전체에 손 파싱이 없음을 말하고, 커밋/PR 텍스트에 에이전트 귀속이 없다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | `live.rs`와 `remote.rs`의 프로덕션 코드에 `serde_json::Value` 탐색과 `json!` 요청 본문이 없다는 소스 검사가 통과한다 | machine | - |
| AC2 | 생성 타입 참조가 `wire.rs`와 `herdr_contract.rs`에만 있다는 소스 검사가 통과한다 | machine | - |
| AC3 | `forked_pane_id`가 삭제되고, 격리 서버에서 잡은 `herdr agent new` 출력 fixture가 생성 타입으로 디코딩되어 만든 pane id를 돌려주는 테스트가 통과한다 | machine | - |
| AC4 | `remote_wire_snapshot_is_typed_and_protocol_checked`, `remote_wire_snapshot_rejects_protocol_mismatch`, `remote_wire_snapshot_rejects_duplicate_ids_and_protocol_wraparound`, `remote_projection_marks_gap_stale_and_host_scope_is_preserved`, `malformed_wire_snapshot_marks_projection_stale`가 같은 stage/retryable/action_required와 문구로 통과한다 | machine | - |
| AC5 | `focus_uses_the_direct_socket_contract_before_reading_authoritative_layout`, `session_layout_projects_authoritative_nested_tree_and_zoom`, `missing_socket_file_is_distinguished_from_unreachable`를 포함한 기존 live 테스트가 같은 문구로 통과한다 | machine | - |
| AC6 | 두 파일의 모든 JSON fixture가 생성 타입으로 역직렬화된다는 테스트가 통과한다 | machine | - |
| AC7 | 격리 소켓의 핀 Herdr에 대한 프로브(SC1 메서드 응답 + `herdr agent new` 출력)와 격리 e2e 세 경로가 통과한다 | machine | - |
| AC8 | 코어 테스트 전체, ffi 계약 테스트, Swift 테스트, 핀 단일 출처 게이트, 스키마 게이트, 사이클 2 구조 검사가 통과한다 | machine | - |
| AC9 | `wire.rs`의 새 변환이 읽는 사람에게 명확하고, 두 파일의 진단 의미와 원격 분류를 잃지 않았다 | judged | `wire.rs` 전문과 `live.rs`, `remote.rs` 변경 diff |
| AC10 | 커밋, 브랜치, PR 본문에 에이전트/모델/도구 귀속이 없다 | machine | - |

## 8. PRD-Level Tasks

- T1. `wire.rs`에 `live.rs`가 쓰는 응답 변환(workspace/tab/pane/layout/read/snapshot)과 요청 파라미터 생성을 추가하고 `live.rs`의 소켓 경로를 옮긴다. Covers R1, R2, R5, AC1, AC2, AC5. Depends on: none.
- T2. 격리 서버에서 `herdr agent new` 출력을 잡아 fixture로 고정하고 `forked_pane_id`를 생성 타입 디코딩으로 바꾼다. Covers R3, AC3. Depends on: T1.
- T3. `remote.rs`의 스냅샷 디코딩을 `wire.rs`로 옮기고 손 헬퍼를 삭제한다. Covers R1, R2, R4, AC4. Depends on: T1.
- T4. 구조 검사를 두 파일로 넓히고, fixture 역직렬화 테스트와 격리 서버 프로브를 검사 스크립트에 넣어 전 스위트, 게이트, 격리 e2e를 통과시킨다. Covers R6, R7, AC6, AC7, AC8. Depends on: T2, T3.
- T5. `AGENTS.md`를 갱신하고 귀속 검사와 PR 근거를 정리한다. Covers R8, AC9, AC10. Depends on: T4.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | 코어/Swift 빌드, 게이트 스크립트, 구조 검사, 귀속 검사 | none |
| automated behavior | yes | 코어/ffi/Swift 테스트, fixture 역직렬화, CLI 출력 fixture | none |
| desktop runtime | yes | 격리 서버 프로브(SC1), 격리 e2e(SC3) | none |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1, R2, AC1, AC2, AC8 | 코어 release 빌드와 Swift 빌드가 통과하고, 두 파일의 손 파싱 부재와 경계 단일성이 소스 검사로 확인되며, 핀/스키마 게이트와 사이클 2 구조 검사가 통과한다 | yes | no |
| V2 | automated behavior | R3, R4, R5, R6, AC3, AC4, AC5, AC6, AC8, SC2 | 코어 테스트(기존 live/remote 진단 테스트, 새 fixture 테스트, CLI 출력 fixture 테스트 포함), ffi, Swift 테스트가 통과한다. 보호하는 회귀: 계약과 fixture의 어긋남, 진단 문구와 원격 분류의 변질 | yes | no |
| V3 | desktop runtime | R7, AC7, SC1, SC3 | 격리 소켓의 핀 Herdr에 대한 프로브와 격리 e2e 세 경로가 통과한다 | yes | no |
| V4 | build/static | R8, AC9, AC10 | `wire.rs`와 diff가 AC9를 만족하고 귀속 검색이 0건이다 | yes | no |

### 9.3 Human Verification

- None required. 사용자 가시 동작이 바뀌지 않으며 모든 증거가 기계 검사와 판정 diff로 남는다.

## 10. Risks And Open Decisions

- `herdr agent new` 출력이 API 봉투와 다를 수 있다. 프로브가 먼저 답하고, 다르면 `wire.rs` 안의 손 봉투와 보호 테스트로 격리한다(사이클 2의 `EventMetadata`와 같은 방식).
- 생성 `*Params` 타입의 필드 이름이나 필수 여부가 현재 `json!` 본문과 다를 수 있다(예: `focus`, `label`, `insert_index`). 생성 타입이 옳고, 차이는 프로브가 드러낸다.
- 원격 경로에서 전체 디코딩을 먼저 하면 다른 프로토콜의 원격이 Malformed로 보고될 수 있다. A3의 순서가 이를 막고 AC4가 고정한다.
- 격리 서버 프로브가 에이전트 프로세스를 실제로 띄운다. 프로브는 자기 소켓의 서버와 pane만 다루고 끝나면 그 서버만 멈춘다.
- 사이클 2가 기록한 coordinator 테스트 두 개의 시간 의존 불안정은 이 PRD 밖이다. 재현되면 재실행으로 구분하고 보고서에 남긴다.

## 11. Implementation Guardrails

- 범위를 넓히지 않는다: `session_sync.rs`, `runtime.rs`, `domain.rs`, `sidebar.rs`는 건드리지 않는다.
- 생성 코드를 커밋하지 않는다. 계약 파일과 핀을 바꾸지 않는다.
- 사용자의 실행 중 Herdr 서버와 기본 소켓을 건드리지 않는다. 프로브와 e2e는 항상 `HERDR_SOCKET_PATH` 격리 경로에서만 돌고, 자기가 띄운 서버만 멈춘다. 내가 만들지 않은 pane, tab, workspace는 닫거나 옮기지 않는다.
- 로컬 `main`(다른 세션의 미푸시 커밋)을 base로 쓰지 않는다. base는 `origin/main` 27e6e2e 이후다.
- 커밋, 브랜치, PR 본문에 에이전트/모델/도구 귀속을 넣지 않는다.
- run 산출물은 `agents/runs/herdr-typed-live-remote/` 아래에만 둔다.
- 원칙 번역 (engineering/principles.md, 35ab76c): 규칙 1: `forked_pane_id`, `required_string`, `string_ids`, `optional_string_ids`는 같은 변경에서 삭제한다. 규칙 2: `wire.rs`는 두 파일이 실제로 쓰는 변환만 갖는다. 규칙 4: 변환 실패는 기존 오류 타입과 문구로 표면화하고 기본값으로 덮지 않는다(`pane.read`의 `unwrap_or_default`는 생성 타입의 필수 여부를 따른다). 규칙 5: 도메인은 생성 타입을 모른다. 규칙 12: 새 테스트는 실제 서버 출력 fixture와 진단 문구를 단언한다. 규칙 13: 응답 변형은 문자열 매칭이 아니라 생성 enum 매칭으로 고른다. practices/env.md: 새 환경 변수는 프로브 게이트용(`HERDR_TEST_*` 기존 관례)만 허용하고 코드에 계약을 둔다. practices/test.md: 테스트는 회귀 위험을 이름 붙여 정당화한다(R6).

## 12. Implementation Result Report Contract

구현 에이전트는 다음을 보고한다:

- status와 사용자 가시 변화(없음이 기대값).
- `wire.rs`에 추가한 변환 목록, 삭제한 손 헬퍼 목록, `herdr agent new` 실제 출력 모양과 fixture 위치.
- 승인된 구조를 따랐는지, 도메인 타입 이름/필드 유지 여부, 스키마에 파라미터 타입이 없어 손으로 만든 메서드 목록.
- task 완료 상태, R/AC/V 커버리지, 모드별 증거(명령, 종료 코드).
- 추가한 테스트와 각 테스트가 막는 회귀.
- delivery: 브랜치, PR URL, CI 상태, 머지 커밋.
- 가정과 편차, 남은 사람 확인 항목, 다음 사이클로 넘길 것.
