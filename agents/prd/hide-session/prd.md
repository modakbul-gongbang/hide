---
topic: "hide-session: 에이전트 세션 파일 읽기를 공용 crate로 분리"
status: "ready"
human_approval: "approved"  # user 2026-09-16 verbatim: ㅇㅇ 다 승인하게 너가 orchestrator가 되서 codex luna max로 해서 작업을 시켜 implementor로 해서 작업 ㄱㄱ
review_profile: "standard"
review_rationale: "로컬 세션 파일을 읽는 코드를 crate로 옮기고 사용자 턴 분류를 바로잡는 내부 구조 변경이며, 사이드바 라벨의 입력이 달라지지만 영속 데이터·자격 증명·외부 부작용은 없다."
source_intake: "current conversation"
created_at: "2026-09-16"
updated_at: "2026-09-16"
---

# PRD: hide-session: 에이전트 세션 파일 읽기를 공용 crate로 분리

## Goal

Hide 개발자(호연)가 Claude Code·Codex 세션 JSONL을 읽는 코드를 한 곳에서 다루게 한다.
지금 그 코드는 `plugins/agent-context-labels/src/lib.rs` 한 파일 안에 경로 탐색·꼬리 읽기·파싱·watcher가 한 덩어리로 있고, `herdr-core/src/usage.rs`가 Codex 세션 경로 탐색과 꼬리 읽기를 따로 한 벌 더 갖고 있다.
이 구조로는 다음 두 PRD(이벤트 구독 watcher, `$task`·`$progress` 라벨)가 세션 파일의 앞부분과 턴 종류를 필요로 하는 순간 파서를 다시 열어야 한다.
그래서 세션 파일의 위치 찾기, 증분 읽기, 대화 이벤트 파싱을 workspace crate `hide-session`으로 빼고, 플러그인과 herdr-core가 같은 crate를 쓰게 한다.
그 과정에서 지금 라벨이 "이전 작업을 의도적으로 중단", "task-notification" 같은 시스템 주입 문구를 사용자 요청으로 요약하는 결함을 부류째 고친다.

## Non-goals

- 0.75초 폴링을 Herdr 이벤트 구독으로 바꾸는 것: 다음 PRD(event-driven watcher)가 맡는다. 이 PRD 뒤에도 watcher는 폴링이며, 세션 파일 읽기만 증분이 된다.
- `$task`(누적 큰 그림)·`$progress`(턴 진행) 두 줄 라벨과 프롬프트 변경: 세 번째 PRD. 이 PRD의 라벨은 지금과 같은 `$summary` 하나이고 프롬프트도 그대로다.
- Herdr 소켓 클라이언트를 crate로 빼는 것: 세션 파일과 무관하므로 event-driven watcher PRD에서 다룬다.
- Claude·Codex 외 다른 에이전트의 세션 형식, 원격(SSH) 머신의 세션 파일: 지금 소비자가 없다. 필요해지면 crate에 파서 한 종을 더한다.
- `herdr-core/src/usage.rs`의 Claude 사용량 경로: Codex 세션 폴백만 crate로 옮긴다. Claude 쪽은 세션 파일을 읽지 않는다.
- 사이드바 레이아웃, 색, 정렬 변경: 없음.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 새 workspace crate `hide-session/`을 만든다. 층은 셋: 세션 파일 위치 찾기(Claude는 cwd·session id, Codex는 id·cwd), 바이트 커서 기반 증분 읽기, 에이전트별 대화 이벤트 파서. 플러그인의 `LocalSessionReader`·`read_tail`·`parse_*_events`·`session_text`와 herdr-core `usage.rs`의 `newest_codex_session_files`·`read_tail`은 crate로 옮기고 원본은 지운다. | "crate로 리팩토링 가고"; 원칙 1(옮긴 코드는 같은 변경에서 삭제), 5(경로·읽기·파싱 분리), 7(herdr-core 중복 제거) |
| D-02 | 대화 이벤트는 `{role, kind, at_unix_ms, text}`로 넓어진다. `kind`는 `Human`(사람이 친 프롬프트), `Injected`(런타임이 user 역할로 끼워 넣은 문구), `Interrupted`(사용자 중단 표식), `Assistant` 넷이다. Claude는 `origin.kind == "human"`이고 `isMeta`가 아닌 항목만 `Human`; `promptSource == "system"`, `isMeta`, `origin` 없음, `<task-notification>`·`<system-reminder>`·"Another Claude session" 같은 알려진 주입 접두어는 `Injected`; `[Request interrupted` 접두어는 `Interrupted`. Codex는 `# AGENTS.md instructions`·`<environment_context>`·`<user_instructions>` 같은 알려진 주입 접두어를 `Injected`, 나머지 user 메시지를 `Human`으로 본다. 접두어 목록은 crate 안 한 곳에 있고 테스트가 고정한다. | 2026-09-16 실측: task-factory 세션의 user 항목 435개 중 사람 프롬프트는 20여 개, `<task-notification>` 48개, 스킬 본문(isMeta) 4개. 원칙 13(문구 하나가 아니라 부류를 고침) |
| D-03 | 슬래시 커맨드 항목(`<command-name>`/`<command-args>`)은 `Human`이며 text는 `<command-args>` 내용, args가 비면 `/<command-name>`이다. | 사용자 의도는 args에 있다(예: "/interview-me 응응 레스고 ...") |
| D-04 | 세션 파일은 pane을 처음 볼 때 한 번 전체를 읽고, 이후에는 기억한 바이트 오프셋부터 덧붙은 부분만 읽는다. 파일 길이가 오프셋보다 작아지거나 파일이 바뀌면(잘림·교체) 처음부터 다시 읽고 구조화 로그로 남긴다. 256 KB 꼬리 읽기는 플러그인에서 없앤다. | 실측: 8 MB 세션(사람 턴 123개)에서 256 KB 꼬리에 남는 사람 턴은 9개뿐이라 첫 요청이 사라짐; 사람 텍스트만 뽑으면 20 MB 파일도 수백 KB라 전체 1회 읽기는 싸다 |
| D-05 | 플러그인은 pane마다 분석에 필요한 만큼만 메모리에 둔다: 최근 사람 턴 `MAX_USER_REQUEST_TURNS`(8)개와 마지막 두 사람 턴 이후의 이벤트. 세션이 길어져도 pane당 메모리는 그 상한을 넘지 않는다. | 원칙 2(필요한 만큼만); 성능 가이드의 "pending-work bound" |
| D-06 | `turn_key`·분석 국면(`TurnStart`/`TurnEnd`)·`<all-user-requests>`·`<latest-exchange>`는 `Human` 이벤트만 사용자 턴으로 센다. `Interrupted`는 지금처럼 `‖` 표시와 "요청 안 보냄" 판단에만 쓰이고 요청 목록에는 들어가지 않는다. `Injected`는 어디에도 들어가지 않는다. | 주입 항목이 새 사용자 턴으로 읽히면 turn_key가 바뀌어 provider 호출이 한 번 더 나간다(task-notification 48개 = 최대 48회 낭비) |
| D-07 | 타임스탬프가 없거나 해석되지 않는 줄은 그 줄만 건너뛰고 `skipped_lines`에 이유 부류와 함께 세며, 파일 없음·읽기 실패·잘림은 타입이 있는 오류로 호출자에게 돌아간다. 빈 결과로 덮지 않는다. | 원칙 4, 10 |
| D-08 | 검증은 실제 형태의 JSONL 픽스처(2026-09-16 관측한 Claude `origin`·`isMeta`·`promptSource` 필드와 Codex `response_item`·주입 접두어)를 crate 테스트로 고정하고, 플러그인 `tests/cli.rs`와 herdr-core `usage` 테스트는 바뀌지 않은 채 통과해야 한다. `scripts/verify-cargo.sh test`가 게이트다. | 원칙 12(호출자가 보는 결과로 단정); `agents/config.json` verify 명령 |
| D-09 | Delivery: `agents/config.json`의 sasu PR 모드(base `main`, worktree, CI watch)로 PR을 열고 사람이 머지한다. | 저장소 규칙(main은 PR로만) |
| D-10 | 원칙 intake: `~/projects/oh-my-principle` engineering/principles.md(commit 653c462)를 전부 읽었다. design 도메인은 화면을 바꾸지 않으므로 해당 없음. 원칙 6(기존 라이브러리 채택)은 Claude·Codex 세션 형식이 문서화되지 않아 파서 라이브러리가 없으므로 번역하지 않는다. | `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 세션 파일이 256 KB를 넘는 긴 세션에서도 라벨 요청 맥락(`<all-user-requests>`)은 세션 전체의 최근 사람 턴 8개로 만들어지며, 첫 요청이 그 8개 안에 있으면 요약에 반영된다. | D-01, D-04, D-06 |
| B2 | `<task-notification>`, `<system-reminder>`, "Another Claude session", 스킬 본문 주입, Codex `# AGENTS.md instructions`·`<environment_context>` 같은 항목은 사용자 요청으로 요약되지 않는다. 사용자가 Esc로 중단한 뒤 라벨이 "이전 작업을 의도적으로 중단"처럼 바뀌지 않고, `‖` 표시와 직전 요약이 유지된다. | D-02, D-06 |
| B3 | 슬래시 커맨드로 시작한 세션의 라벨은 커맨드 인자에 적힌 요청을 요약한다(예: `/interview-me 응응 레스고 ...` -> 인터뷰 요청). | D-03 |
| B4 | 에이전트가 한 턴을 일하는 동안 task-notification이 여러 번 도착해도 provider 호출은 늘지 않는다. 턴당 최대 2회(시작·끝)는 그대로다. | D-06 |
| B5 | pane을 처음 본 뒤의 폴링은 세션 파일에 덧붙은 바이트만 읽는다. 파일이 잘리거나 교체되면 처음부터 다시 읽고 `session_rescanned` 로그 한 줄(pane id, 이유)을 남긴다. | D-04 |
| B6 | 세션 파일이 없거나 읽을 수 없으면 지금처럼 `raw_session_unavailable` 로그가 남고 직전 라벨이 유지된다. 해석되지 않는 줄은 `session_lines_skipped`에 개수와 이유 부류로 남는다. 로그에는 대화 본문이 들어가지 않는다. | D-07 |
| B7 | 긴 세션이라도 pane당 watcher 메모리는 최근 사람 턴 8개와 마지막 두 사람 턴 이후 이벤트로 제한된다. | D-05 |
| B8 | 플러그인 `analyze` 서브커맨드(stdin transcript)와 watcher는 같은 파서를 써서 같은 입력에 같은 맥락을 만든다. | D-01 |
| B9 | herdr-core의 Codex 사용량 폴백(세션 파일에서 주간 한도 읽기)은 값·시각이 전과 같고, 그 경로 탐색과 꼬리 읽기는 `hide-session`에서 온다. `usage.rs`의 중복 함수는 사라진다. | D-01 |
| B10 | `hide-session` 픽스처 테스트가 D-02·D-03의 분류와 D-04의 증분·잘림 재읽기를 고정하고, `scripts/verify-cargo.sh test`·`build`가 통과한다. 플러그인 `tests/cli.rs`와 herdr-core 기존 테스트는 수정 없이 통과한다. | D-08 |
| B11 | 새 crate는 workspace에 이미 고정된 의존성(`serde`, `serde_json`, `anyhow`)만 쓰고, `herdr-core`의 C ABI(`herdr_core.h`)와 macOS 셸 빌드는 바뀌지 않는다. | D-01 |

## Technical structure

- 새 crate `hide-session/`을 `Cargo.toml` workspace members에 추가. 공개 경계는 셋: 세션 파일 위치(에이전트 종류 + cwd/id -> 경로), 파일 커서(`offset`, 파일 identity 보관, 덧붙은 부분 읽기, 잘림 감지), 파서(에이전트 종류별 줄 -> `ConversationEvent{role, kind, at_unix_ms, text}` + skipped 집계). 원시 줄 접근도 노출해 herdr-core usage 폴백처럼 대화가 아닌 레코드를 읽는 소비자도 같은 위치·커서를 쓴다.
- `plugins/agent-context-labels`: `SessionReader` 구현이 crate를 감싸고 pane별 커서와 D-05의 상한 버퍼를 든다. `analysis_context`·`turn_key`·interrupted 판정은 `kind`를 본다. 프롬프트·출력 스키마·메타데이터 토큰은 불변.
- `herdr-core`: `usage.rs` Codex 폴백이 crate의 위치·꼬리 읽기를 호출. `Cargo.toml`에 path 의존성 추가. FFI·스냅샷 스키마 불변.
- 영속 상태 추가 없음(커서는 프로세스 메모리). 새 외부 의존성 없음.

## Risks

- Claude Code·Codex 세션 형식은 문서화되지 않았고 버전마다 바뀐다. `origin`이 없는 옛 Claude 파일은 접두어 목록으로만 분류되므로 새 주입 문구가 나오면 다시 `Human`으로 새어 들어온다. 경계: 픽스처에 관측 버전을 적고, 접두어 목록이 한 곳에 있어 추가가 한 줄이다.
- 두 소비자(플러그인, herdr-core)가 한 PR에서 바뀐다. herdr-core 스위트는 부하 시 flake 이력이 있어(2026-09-10 기록) CI 실패가 이 변경 탓인지 A/B로 가려야 할 수 있다.
- 첫 전체 읽기는 20 MB 파일에서 수백 ms이며 폴링 스레드에서 돈다. pane당 1회라 허용하고, 다음 PRD가 폴링을 없앤다.
- 사용자가 미리 해 줄 일: 없음.
