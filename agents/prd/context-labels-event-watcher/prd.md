---
topic: "agent-context-labels watcher를 Herdr 이벤트 구독으로 전환하고 소켓 클라이언트를 공용 crate로 분리"
status: "ready"
human_approval: "approved"  # user 2026-09-16 verbatim: ㅇㅇ 다 승인하게 너가 orchestrator가 되서 codex luna max로 해서 작업을 시켜 implementor로 해서 작업 ㄱㄱ
review_profile: "standard"
review_rationale: "watcher의 입력을 폴링에서 소켓 구독으로 바꾸고 herdr-core의 소켓 클라이언트를 crate로 옮기는 런타임 구조 변경이며, 사용자 데이터·자격 증명·외부 부작용은 없지만 두 소비자의 Herdr 연결 경로가 함께 바뀐다."
source_intake: "current conversation"
created_at: "2026-09-16"
updated_at: "2026-09-16"
---

# PRD: agent-context-labels watcher를 Herdr 이벤트 구독으로 전환하고 소켓 클라이언트를 공용 crate로 분리

## Goal

Hide 개발자(호연)가 사이드바 라벨 watcher를 유휴 비용 0으로 만들고, 앞으로 Herdr 이벤트를 여러 종 구독할 때 코어와 플러그인이 같은 클라이언트를 쓰게 한다.
지금 watcher는 0.75초마다 `herdr agent list` 프로세스를 띄워(1회 210~260 ms 실측, 2026-09-16) 변화가 없는지 확인하고, 메타데이터 보고도 `herdr` 프로세스를 띄우며, `agent.view.set`은 손으로 짠 소켓 코드로 보낸다.
herdr-core에는 연결·요청·`events.subscribe`·재개 커서·리더 스레드를 갖춘 소켓 클라이언트(`herdr_api.rs`)가 있지만 `pub(crate)`라 밖에서 쓸 수 없다.
그래서 그 클라이언트를 workspace crate `hide-herdr-client`로 빼고, watcher가 `pane.*` 이벤트를 구독해 상태가 바뀐 순간에만 세션 파일을 읽고 라벨을 보고하게 한다.

## Non-goals

- `$task`·`$progress` 두 줄 라벨과 프롬프트 변경: 세 번째 PRD. 이 PRD의 라벨 내용·토큰·레이아웃은 불변이다.
- 한 프로세스 안에서 여러 소비자에게 이벤트를 나눠 주는 dispatcher: 지금은 프로세스마다 소비자가 하나(코어 하나, watcher 하나)라 채널 하나면 된다. 한 프로세스에 두 번째 소비자가 생길 때 더한다(원칙 2).
- 세션 파일 변경을 파일시스템 감시로 받는 것: 세션 읽기는 pane 이벤트가 촉발한다. 턴 중간의 파일 변화는 라벨에 필요 없다.
- Herdr 통합 훅(`herdr integration install`)의 등록 방식 변경: 훅은 지금처럼 스크립트가 `hook` 서브커맨드를 부른다.
- 플러그인 CLI 액션(refresh, enable/disable)의 셸 스크립트 진입점 제거: Herdr 매니페스트가 명령 배열을 요구하므로 유지한다.
- 원격(SSH) Herdr 세션 구독: 코어의 원격 경로는 지금 구조를 유지하고 crate만 쓴다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 새 workspace crate `hide-herdr-client/`를 만들고 herdr-core `herdr_api.rs`의 연결(`ApiConnector`·`UnixSocketConnector`), 요청/응답 봉투, `events.subscribe` 확인·리더 스레드·`after_sequence` 재개, 프로토콜 리비전 상수와 typify 생성 wire 타입(`build.rs`)을 옮긴다. herdr-core `wire.rs`는 코어 도메인 변환 경계로 남아 crate의 생성 타입을 import한다. 플러그인의 `CliHerdr`(프로세스 spawn)와 손으로 짠 소켓 코드(`apply_priority_agent_view`)는 지운다. | "앞으로 herdr 이벤트 여러 개 구독할 건데"; 원칙 1, 5, 7, 8; 저장소 규칙 "long-lived subscription은 raw socket" |
| D-02 | watcher는 스레드 하나의 이벤트 루프가 된다: 채널 하나에 (a) 구독 리더 스레드의 이벤트 줄, (b) 훅·refresh 깨우기, (c) 경과 시간 타이머가 들어오고 `recv_timeout`으로 기다린다. 코어 `session_sync.rs`의 `CoordinatorMessage` 패턴을 그대로 따른다. | 원칙 7(코어에 이미 있는 패턴), 성능 가이드 "publish only actual state transitions" |
| D-03 | 구독 종류는 `pane.created`, `pane.updated`, `pane.closed`, `pane.exited`, `pane.focused`, `pane.agent_detected`, `pane.agent_status_changed`. 연결(재연결 포함) 직후 `agent.list` 한 번으로 pane 목록과 `state_change_seq`를 채우고 확인 응답의 `sequence`를 커서로 삼는다. 이후 상태 변화 시각은 이벤트 도착 시각이다. | 계약 `contracts/herdr-api.schema.json`의 구독 목록; `state_change_seq`는 `AgentInfo`에만 있고 이벤트에는 없다 |
| D-04 | 유휴 시 주기 작업은 경과 표시 갱신뿐이며, 다음 표시가 바뀌는 시각(60초 미만은 1초 뒤, 그 뒤는 다음 분·시·일 경계)까지 잔다. 주기적으로 `herdr` 프로세스를 띄우거나 소켓 요청을 보내지 않는다. | "0.75가 너무 잦지 않아?"; 실측 spawn 비용 ~230 ms × 80회/분 |
| D-05 | `hook`·`request-refresh` 서브커맨드는 지금처럼 상태 파일을 쓴 뒤 watcher가 상태 디렉터리에 여는 unix 소켓에 한 줄을 보내 깨운다. watcher가 없으면 파일만 남고 다음 시작 때 읽힌다(지금 동작). 파일 감시 crate는 넣지 않는다. | 원칙 2, 7; 새 의존성 없이 즉시 반영 |
| D-06 | 재연결은 코어와 같은 지수 백오프(100 ms → 최대 5 s). 재개 시 `after_sequence`로 놓친 이벤트를 받고, 커서가 `oldest_available_sequence`보다 오래되면 `agent.list`로 다시 부트스트랩한다. 프로토콜 리비전 불일치는 명시적 실패로 watcher가 멈추고 로그에 기대값·실제값을 남긴다. | 코어 `session_sync.rs` 정책 재사용; 원칙 4, 10, 11 |
| D-07 | `agent.list`, `pane.report_metadata`, `agent.view.set`은 모두 crate를 통해 소켓으로 보낸다. 메타데이터 보고는 지금처럼 표시가 실제로 바뀔 때만 나간다. | 원칙 5; 성능 가이드 |
| D-08 | 검증: 코어 테스트의 in-memory `ApiStream` 방식으로 가짜 커넥터가 이벤트 줄을 흘려 주고, watcher가 보낸 요청 목록을 단정한다. `scripts/verify-runtime.sh`는 실제 Herdr에 대한 살아있는 확인으로 유지한다. 유휴 비용은 60초 동안 `herdr` 자식 프로세스 수를 세어 0임을 확인한다. | 원칙 12; `agents/config.json` verify 명령 |
| D-09 | 이 PRD는 `hide-session` PRD가 먼저 머지된 위에 쌓는다. 세션 읽기는 그 crate의 커서 API를 이벤트 도착 시점에 호출한다. | 순서 1 → 2 → 3 합의 |
| D-10 | Delivery: `agents/config.json`의 sasu PR 모드(base `main`, worktree, CI watch). `docs/ARCHITECTURE.md`·플러그인 `README.md`·`AGENTS.md`의 폴링 설명을 같은 PR에서 고친다. | 저장소 규칙(owning guide를 같은 변경에서 갱신) |
| D-11 | 원칙 intake: engineering/principles.md(commit 653c462) 전부 읽음. design 도메인은 화면 변경이 없어 해당 없음. 원칙 6은 Herdr 소켓 프로토콜이 이 저장소 고유라 외부 라이브러리가 없어 번역하지 않는다. | `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | pane 활동이 없는 60초 동안 watcher는 `herdr` 자식 프로세스를 하나도 띄우지 않고 소켓 요청도 보내지 않는다(현재 약 80회/분). | D-02, D-04 |
| B2 | 에이전트 상태가 바뀌면(working ↔ idle/done/blocked) 사이드바 심볼과 경과 시간이 폴링 주기를 기다리지 않고 이벤트 도착 즉시 갱신된다. | D-03 |
| B3 | pane을 포커스하면 unseen 색이 즉시 seen으로 바뀐다. | D-03 |
| B4 | 훅이 질문·승인·오류를 보고하면 watcher가 깨어나 즉시 `?`·`!`·`×`를 반영한다. watcher가 꺼져 있으면 다음 시작 때 반영된다. | D-05 |
| B5 | "Refresh active pane summary" 액션은 watcher를 깨워 포커스된 pane을 다시 분석한다. | D-05 |
| B6 | Herdr 서버가 재시작되면 watcher가 백오프로 다시 붙고 놓친 이벤트를 커서부터 재생하거나, 버퍼가 없으면 `agent.list`로 다시 맞춘다. 사이드바는 조작 없이 따라잡고, 로그에 `herdr_subscription_lost`·`herdr_subscription_resumed`(커서, 재부트스트랩 여부)가 남는다. | D-06 |
| B7 | 시작 시 Herdr에 닿지 않으면 watcher는 종료하지 않고 재시도하며, 실패 스트릭의 첫 줄과 회복 줄만 로그에 남는다(지금 `watcher_scan_failed`/`recovered`와 같은 규칙). | D-06 |
| B8 | 프로토콜 리비전이 다르면 watcher는 반쯤 동작하지 않고 멈추며, 로그와 표준 오류에 기대·실제 리비전을 적는다. | D-06 |
| B9 | 경과 시간은 `15s`→`16s`, `4m`→`5m`처럼 계속 흐르고, 표시가 바뀌는 순간에만 보고가 나간다. | D-04, D-07 |
| B10 | watcher 코드에 `herdr` CLI 호출이 남지 않는다: `agent list`, `pane report-metadata`, 손으로 짠 `agent.view.set` 소켓 코드가 모두 crate 호출로 바뀐다. 셸 스크립트 진입점은 그대로다. | D-01, D-07 |
| B11 | herdr-core는 crate를 통해 같은 연결·구독을 하며 동작이 바뀌지 않는다. 기존 코어 테스트, `scripts/check-herdr-contract.sh`, `check-herdr-pin-single-source.sh`가 수정 없이 통과하고 프로토콜 리비전은 여전히 계약 파일 한 곳에서 온다. | D-01 |
| B12 | 가짜 커넥터 테스트가 B2·B3·B6·B8을 고정하고 `scripts/verify-cargo.sh test`·`build`가 통과한다. | D-08 |
| B13 | `scripts/verify-runtime.sh`가 실제 Herdr에서 구독 확인과 라벨 보고를 확인하고, 60초 유휴 측정에서 `herdr` 자식 프로세스 0을 보고한다. | D-08 |
| B14 | `docs/ARCHITECTURE.md`와 플러그인 `README.md`·`AGENTS.md`가 이벤트 구독 구조와 깨우기 소켓을 설명하고 0.75초 폴링 언급이 사라진다. | D-10 |

## Technical structure

- 새 crate `hide-herdr-client/`: `build.rs`(typify 생성)와 `contracts/herdr-api.schema.json` 참조가 herdr-core에서 이 crate로 이동. 공개 경계: 커넥터 trait + unix 소켓 구현, 단발 요청(상관 id, 타임아웃), 구독(확인 응답, 리더 스레드 → `mpsc`, 커서, 종료 핸들), 프로토콜 리비전 상수, 생성 wire 타입 모듈.
- `herdr-core`: `herdr_api.rs` 삭제, `wire.rs`·`session_sync.rs`가 crate를 import. FFI(`herdr_core.h`)·스냅샷 스키마 불변.
- `plugins/agent-context-labels`: `HerdrTransport` 구현이 crate로 교체되고 `Watcher`가 이벤트 루프로 바뀐다. 상태 디렉터리(`~/.local/state/hide.agent-context-labels/`)에 깨우기 unix 소켓 하나 추가(영속 아님, 시작 시 재생성). 프롬프트·토큰·정렬 불변.
- 새 외부 의존성 없음. 영속 상태 형식 불변.

## Risks

- 이벤트 페이로드에 `state_change_seq`·`revision`이 없어 "변화 시각"의 근거가 `agent.list` 값에서 이벤트 도착 시각으로 바뀐다. 재연결 재생 시 과거 이벤트가 지금 시각으로 찍히지 않도록 재생 구간은 시각을 갱신하지 않는다.
- 두 소비자(코어, 플러그인)의 연결 경로가 한 PR에서 바뀐다. 코어 스위트는 부하 flake 이력이 있어 실패가 이 변경 탓인지 A/B로 가려야 할 수 있다.
- macOS unix 소켓 경로는 104바이트 한도가 있다. 상태 디렉터리 경로가 이를 넘는 HOME에서는 깨우기 소켓 생성이 명시적으로 실패하고 로그에 남으며, 훅 반영은 다음 이벤트까지 늦어진다.
- pinned Herdr는 fork 빌드(preview-2026-09-15)라 구독 의미가 upstream과 다를 수 있다. 계약은 그 바이너리의 `api schema`이며 `check-herdr-contract.sh`가 지킨다.
- 사용자가 미리 해 줄 일: 없음.
