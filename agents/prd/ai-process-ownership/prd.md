---
topic: "hide-ai가 codex app-server를 소유하고 budget 안에 두며, 라벨 watcher는 Herdr와 함께 죽는다 (issue #82 Part A)"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "사용자의 codex 로그인 자격 증명(auth.json)을 가리키는 symlink를 hide-ai가 만든 임시 디렉터리에 두고, 상주 프로세스의 종료·재시작·상한 정책을 바꾸는 변경이라 자격 증명 노출과 프로세스 생명주기 회귀가 모두 걸린다."
source_intake: "agents/interview/ai-process-ownership/qa-log.md"
created_at: "2026-09-17"
updated_at: "2026-09-17"
---

# PRD: hide-ai가 codex app-server를 소유하고 budget 안에 두며, 라벨 watcher는 Herdr와 함께 죽는다 (issue #82 Part A)

## Goal

Hide 운영자(호연)의 Mac이 2026-09-17에 멈췄다: 라벨 플러그인 `hide-agent-context-labels watch` 하나가 `codex app-server` 자손 1,699개, RSS 11.6 GB를 이틀 동안 붙들고 있었고(issue #82), 아무 신호도 없었다.
원인은 넷이다. `-c mcp_servers={}`가 MCP 서버를 막지 못해 thread마다 `~/.codex/config.toml`의 MCP 서버 전부가 자식으로 뜨고(실측: thread당 8개), codex 0.154에는 thread를 닫는 메서드가 없으며, `Session::drop`의 SIGKILL이 pnpm node wrapper만 죽여 진짜 codex와 그 자식을 고아로 만들고, Herdr가 종료돼도 watcher는 영원히 재접속하며 app-server를 살려 둔다.
이 PRD는 `hide-ai`가 자신이 띄운 프로세스를 소유하게 만든다: 사설 `CODEX_HOME`으로 MCP 자식을 0개로, 종료를 graceful로, 유휴 10분이면 app-server를 내리고, 요청·프로세스 상한을 넘으면 실패로 보고하며, 요청마다 자손 수와 RSS를 기록한다.
그리고 watcher는 부모 Herdr가 사라지면 스스로 종료한다.
새지 않게, 새면 보이게 - 이것이 이 변경의 한 줄이다.

## Non-goals

- issue #82 Part B(`hide-ai-broker` 상주 프로세스, unix socket JSON-RPC `ai/*`, Herdr의 `[[startup]]` process-group 소유, `ai/status` Herdr 패널, 클라이언트별 budget): 플러그인은 계속 `AiRouter`를 직접 링크하고 app-server를 각자 띄운다. Herdr 쪽 변경은 herdr fork(별도 저장소)이고 `docs/AI_PROVIDERS.md`의 "daemon/socket 거부" 결정을 뒤집는 일이라 두 번째 AI 플러그인이 생길 때 별도 PRD로 다시 본다. 이 PR이 열릴 때 issue #82에 분리 코멘트를 남긴다.
- `plugins/agent-context-labels/src/lib.rs`의 `label/`·`daemon/` 모듈 분리: Part B와 함께 다시 본다.
- 매 turn 후 `thread/close`: codex 0.154 프로토콜에 없다(D-02). 스키마에 생기면 이 PRD의 유휴 종료·재시작이 그 자리를 넘겨준다.
- codex가 MCP를 끄는 설정 키를 제공하는 경우의 재검토: `-c mcp_servers={}`와 thread `config` 오버라이드 모두 실측으로 무효였다(D-01). codex가 문서화된 키를 내면 사설 `CODEX_HOME`을 대체할 수 있다.
- claude 백엔드의 생명주기 변경: 요청당 `claude -p` 자식 하나라는 현재 모델을 유지한다. budget의 요청 상한은 두 백엔드에 같이 적용되고 프로세스 상한은 codex에만 있다.
- budget 값을 설정 파일로 노출: 상한은 코드의 상수이며 실측(D-05)으로만 바꾼다(practices/process.md 규칙 4).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 범위는 issue #82 Part A + watcher host-death exit. Part B와 모듈 분리는 후속 issue로 미룬다. | Q2 "Part A + watcher host-death exit로 가고"; qa-log D-07 |
| D-02 | `-c mcp_servers={}`는 지운다. hide-ai는 세션마다 소유자 전용(0700) 임시 디렉터리를 만들어 `CODEX_HOME`으로 넘기며, 그 안에는 사용자의 `auth.json`(환경의 `CODEX_HOME` 아래, 없으면 `~/.codex/auth.json`)을 가리키는 symlink만 둔다. config.toml은 없다. 세션이 끝나면 디렉터리를 지운다. 디렉터리나 symlink를 만들지 못하면 `ProviderUnavailable`이고 `~/.codex`로 되돌아가지 않는다. | Q6 수락(Q3 항목 1), Q7 "둘 다 예"; 실측 D-01: 그 구성에서 자손 1개; qa-log D-09, D-18, D-19 |
| D-03 | app-server 종료는 graceful 순서다: stdin close → 3초 유예 → SIGTERM → SIGKILL, 그리고 `wait`. `Session::drop`, 세션 교체, 상한 초과 재시작이 모두 이 한 경로를 지난다. 자식은 hide-ai가 drop 없이 죽어도 stdin EOF로 종료된다(실측 D-03). | 실측: SIGKILL은 wrapper만 죽여 8개 고아, SIGTERM/stdin close는 3초 안에 전부 종료; qa-log D-03, D-10; practices/process.md 규칙 1·3 |
| D-04 | 유휴 10분(마지막 요청 완료 기준)이면 app-server를 내리고, 다음 요청이 새로 띄운다. thread 수 기반 재시작(N=100)은 채택하지 않는다: 사고는 이틀간 요청 0건인 상태에서 났고 카운트 상한은 그때 아무것도 하지 않는다. | Q5 제안을 Q6에서 수락; qa-log D-10 |
| D-05 | `RouterConfig`에 budget을 더한다: 동시 요청 1, 분당 요청 30, codex app-server 자손 상한 4, RSS 상한 1 GiB. 값은 실측(최대 20건/분, 정상 자손 1~2개)에서 나왔고 코드 상수다. 요청 상한 초과는 `AiError::OverBudget`(거부 계열: 제출 전이라 재시도 안전, 다른 provider로 fallback하지 않음). 프로세스 상한 초과는 app-server를 D-03 경로로 끝내고 다시 띄우며 `ai.app_server.over_budget`에 측정값과 상한을 남긴다; 그 재시작 자체도 연속 3회에서 멈추고 넘으면 `ProviderUnavailable(app_server_restart_cap)`이며, 상한 아래로 완료된 요청이 카운터를 되돌린다. | Q6 수락(Q3 항목 3), Q8 "둘 다 예"; qa-log D-05, D-11, D-21; practices/process.md 규칙 4 |
| D-06 | 매 codex 요청 후 `ai.request.finished`(기존 필드: request id, feature, provider, outcome, duration, tokens)에 app-server pid, 자손 수, RSS bytes를 필드로 더한다. 측정은 macOS libproc(`proc_listchildpids`, `proc_pidinfo`)로 커널에서 읽고 서브프로세스를 띄우지 않는다. Linux에서는 측정이 `Unavailable`이고 프로세스 상한 검사를 하지 않으며 로그 줄에 unavailable로 적는다(0으로 적지 않음); `/proc` 리더는 Linux를 운영할 때 더한다. 프롬프트·토큰·경로는 여전히 기록하지 않는다. | Q6 수락(Q3 항목 6), Q8 "Linux는 b로"; qa-log D-14, D-22; practices/process.md 규칙 5; 원칙 9 |
| D-07 | 라벨 플러그인은 `OverBudget`을 환경 실패로 다룬다: 마지막 라벨을 유지하고 10분 뒤 다시 묻는다(`PROVIDER_RECOVERY_INTERVAL`). 루프 재시도는 없다. | Q6 수락(Q3 항목 4); issue #82 "degrades (keeps its last label)"; qa-log D-12 |
| D-08 | watcher는 시작 시 부모 pid를 기억하고 이벤트 루프의 매 tick(대기 상한을 5초로 내린다)마다 `getppid()`와 비교한다. 부모가 바뀌면(launchd로 재부모화) `watcher_host_gone`을 로그에 쓰고 lock을 풀고 exit 0 한다. app-server는 stdin EOF로 따라 죽는다. Herdr가 재시작하는 동안의 재접속 백오프는 그대로 두되, 부모가 사라진 경우는 재접속이 아니라 종료다. | Q6 수락(Q3 항목 5); qa-log D-04, D-13; practices/process.md 규칙 3 |
| D-09 | 검증: (a) `thread/start`마다 자식을 띄우고 stdin EOF에 자식과 함께 끝나는 가짜 app-server로 500건을 실제 코드 경로로 돌려 자손 수가 상한 아래에서 안정임을 CI 스위트에서 단언한다; (b) 소유자 프로세스를 `kill -9` 한 뒤 5초 안에 자손 0을 단언한다; (c) 실제 codex app-server가 사설 `CODEX_HOME`에서 MCP 자식 0개임을 단언하는 테스트는 `#[ignore]`로 두고 codex가 PATH에 있을 때 문서화된 명령으로 돌린다; (d) `verify-provider`가 app-server 자손 수를 출력한다; (e) 실제 turn 관찰은 codex 주간 한도 리셋(2026-09-19 17:10 KST) 뒤에 하며 그때까지 PR에 unrun으로 적는다. | Q6 수락(Q3 항목 7); issue #82 Acceptance; qa-log D-15; practices/process.md 규칙 6 |
| D-10 | `docs/AI_PROVIDERS.md`를 같은 변경에서 고쳐 쓴다: "usage cap도 budget surface도 없다는 것은 결정"이라는 문단을 budget·프로세스 소유 계약으로 바꾸고, codex 절에 사설 `CODEX_HOME`, 종료 순서, 유휴 종료, 상한을 적는다. `docs/README.md`의 해당 행도 맞춘다. | Q6 수락(Q3 항목 8); qa-log D-06, D-16 |
| D-11 | 이미 샌 프로세스의 수동 정리는 선행 조건이 아니다: 2026-09-17 확인 시 watcher도 app-server도 없었고, 남은 MCP 프로세스 149개는 대화형 codex 세션의 자식이라 범위 밖이다. | Q7 "둘 다 예"; qa-log D-20 |
| D-12 | 원칙 intake: `~/projects/oh-my-principle` `fa5186d`의 `engineering/principles.md`와 `practices/process.md`를 전부 읽었다. 규칙 1·3·4·5·6은 D-03·D-05·D-06·D-08·D-09로 번역했다. 규칙 2(`start` 핸들의 drop이 `close`를 보냄)는 codex 0.154에 `thread/close`가 없어 thread 단위로는 적용할 수 없고, 세션(app-server) 단위의 drop 종료로 대신한다; 스키마에 `thread/close`가 생기면 실패하는 테스트를 남겨 이행을 요구한다. 규칙 1의 "spawn 헬퍼 하나"는 hide-ai 크레이트 안에서 지키며(codex·claude 백엔드 모두 그 헬퍼를 지남), 저장소 전체 CI grep은 herdr-core의 기존 spawn까지 바꾸는 일이라 이 PRD 밖이다. | gen-prd Principles Intake |
| D-13 | 배포는 `agents/config.json`대로 worktree 브랜치에서 `main`으로 PR을 열고 CI를 지켜보며, merge는 사용자의 명시적 승인 뒤에만 한다. sasu 게이트는 codex 한도 소진으로 `SASU_JUDGE_BACKEND=claude`로 돌린다. | qa-log D-08, D-17 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | hide-ai가 띄운 `codex app-server`의 프로세스 트리를 보면 MCP 서버 자식이 하나도 없다. 요청이 아무리 반복돼도 자손 수는 상한(4) 아래에서 안정이다. | D-02, D-05 |
| B2 | 사설 `CODEX_HOME` 디렉터리는 hide-ai 프로세스 소유자만 읽을 수 있는 0700이고 `auth.json` symlink 하나만 들어 있으며, 세션이 끝나면 사라진다. codex의 토큰 갱신 쓰기는 symlink를 통해 원래 파일에 닿는다. | D-02 |
| B3 | 사설 `CODEX_HOME`을 만들 수 없으면 요청은 `ProviderUnavailable`로 실패하고 로그에 그 이유(`codex_home_unavailable:<kind>`)가 남는다; `~/.codex`로 조용히 되돌아가지 않는다. | D-02 |
| B4 | 세션이 drop되거나 교체될 때(settings 변경으로 router를 다시 만들 때 포함) 진짜 codex 프로세스와 그 자손이 3초 안에 함께 끝나고 고아가 남지 않는다. | D-03 |
| B5 | hide-ai를 링크한 프로세스가 `kill -9`로 죽어도 app-server와 그 자손은 5초 안에 모두 끝난다. | D-03, D-09 |
| B6 | 마지막 요청 뒤 10분 동안 요청이 없으면 app-server가 종료되고 `ai.app_server.idle_exit`이 기록된다; 다음 요청은 새 app-server를 띄워 정상 답을 받는다. | D-04 |
| B7 | 같은 router에 두 번째 요청이 동시에 들어오거나 1분 안에 31번째 요청이 들어오면 그 요청은 제출되지 않고 `AiError::OverBudget`(어느 상한인지 이름 포함)으로 즉시 돌아오며 `ai.budget.exceeded`가 기록된다. 이미 진행 중인 요청과 다른 provider에는 영향이 없다. | D-05 |
| B8 | 요청 뒤 측정한 app-server 자손 수가 4를 넘거나 RSS가 1 GiB를 넘으면 `ai.app_server.over_budget`에 측정값과 상한이 기록되고 app-server가 D-03 순서로 종료된 뒤 다음 요청에서 새로 뜬다. 연속 3회째 재시작이 필요하면 `ProviderUnavailable(app_server_restart_cap)`로 실패한다. | D-05 |
| B9 | 매 codex 요청의 `ai.request.finished` 줄에 `app_server_pid`, `descendants`, `rss_bytes` 필드가 있다(Linux에서는 `measurement=unavailable`). 프롬프트·입력·답·토큰·경로는 없다. | D-06 |
| B10 | 라벨 플러그인이 `OverBudget`을 받으면 pane의 기존 라벨이 그대로 남고 10분 뒤 다시 요청한다; 그 사이 같은 turn에 대한 반복 요청은 없다. | D-07 |
| B11 | Herdr가 종료되면(정상 종료·크래시 모두) watcher는 5초 안에 `watcher_host_gone`을 기록하고 exit 0 하며, `ps`에 `hide-agent-context-labels watch`도 `codex app-server`도 남지 않는다. | D-08 |
| B12 | Herdr가 다시 시작되면 `[[startup]]`이 새 watcher를 띄우고 lock 충돌 없이 시작된다. Herdr 소켓만 잠깐 끊긴 경우(부모 생존)는 지금처럼 백오프 재접속한다. | D-08 |
| B13 | `hide-agent-context-labels verify-provider --provider codex` 출력에 app-server 자손 수가 함께 나온다. | D-09 |
| B14 | `bash scripts/verify-cargo.sh test`에 가짜 app-server 500건 안정성 테스트와 `kill -9` 생존자 0 테스트가 포함돼 통과한다; 실제 codex 대상 MCP-0 테스트는 `#[ignore]`이며 문서화된 명령으로 돌리면 통과한다. | D-09 |
| B15 | `docs/AI_PROVIDERS.md`에 budget 값, 프로세스 소유 계약, 사설 `CODEX_HOME`, 종료·유휴·재시작 정책이 적혀 있고 "budget surface 없음은 결정"이라는 문장은 없다. | D-10 |
| B16 | PR 본문에 실제 turn 관찰이 codex 리셋(2026-09-19) 전이라 unrun임이 명시돼 있고, issue #82에 Part B 분리 코멘트가 달려 있다. | D-09, D-01 |

## Technical structure

- `hide-ai/src/codex.rs`: `Session`이 사설 `CODEX_HOME` 디렉터리(생성·0700·symlink·삭제)를 소유하고, spawn은 크레이트 안의 단일 spawn 헬퍼를 지나며, 종료는 `shutdown()` 한 경로(stdin close → 유예 → SIGTERM → SIGKILL → wait)로 모인다. 유휴 타이머와 마지막 완료 시각은 세션 옆에 둔다.
- `hide-ai/src/router.rs`: `RouterConfig`에 budget 상수 필드(`max_in_flight`, `max_per_minute`, `max_descendants`, `max_rss_bytes`)와 sliding-window 카운터를 더하고, `AiError::OverBudget { cap, measured }`를 거부 계열로 분류한다. `AiBackend`에 "요청 후 측정값"과 "재시작" 훅이 생기며 claude 백엔드는 측정값 없음을 명시적으로 답한다.
- 프로세스 측정: macOS libproc FFI(`proc_listchildpids`, `proc_pidinfo` `PROC_PIDTASKINFO`)를 hide-ai 안에 작은 모듈로 두고 `libc` 크레이트만 추가한다. 다른 플랫폼은 `Unavailable`. 서브프로세스 없음.
- `plugins/agent-context-labels/src/lib.rs`: 이벤트 루프 tick 상한 5초, 부모 pid 감시, `watcher_host_gone` 종료 경로; `AnalysisFailure::retry_after`에 `OverBudget` → `PROVIDER_RECOVERY_INTERVAL`.
- `hide-ai/tests/fixtures/fake-app-server.py`: `thread/start`마다 자식 하나를 띄우고 stdin EOF에 자식을 끝내는 모드를 더한다; 500건·`kill -9` 계약 테스트는 `hide-ai/tests/`에, 스키마에 `thread/close`가 생기면 실패하는 guard 테스트도 같은 곳에.
- 새 서비스·스키마·저장소·네트워크 경계는 없다. 자격 증명은 symlink로 참조만 하고 읽지 않는다.

## Risks

- 자격 증명 symlink: `auth.json`을 가리키는 링크가 임시 디렉터리에 생긴다. 0700 + symlink만 + 세션 종료 시 삭제로 묶었고, hide-ai는 그 파일을 읽지 않는다. `docs/AI_PROVIDERS.md`의 "credential file을 읽지 않는다"는 문장은 이 참조를 명시하도록 고친다.
- 사설 `CODEX_HOME`에 config.toml이 없으므로 사용자의 `model_provider`·프록시 같은 설정이 빠진다. 지금 운영자 환경에서는 `auth.json`만으로 thread가 열렸다(실측). 실제 turn은 codex 리셋 뒤에야 볼 수 있으니 그때까지 이 PR은 "live turn unrun"이며 merge 전 확인 항목이다.
- codex가 향후 `CODEX_HOME` 안에 세션·로그를 쓰기 시작하면 임시 디렉터리 삭제가 그것을 같이 지운다. ephemeral thread라 지금은 아무것도 남기지 않는다.
- libproc FFI는 macOS 전용이다. 플러그인 manifest는 linux도 선언하지만 운영 환경은 macOS뿐이므로 Linux에서는 측정이 `Unavailable`이고 상한 검사가 없다(D-06); 조용히 0을 내지 않는다.
- 유휴 종료로 첫 요청의 지연이 app-server 시작 시간(1~2초)만큼 늘어난다. 라벨은 백그라운드라 허용한다.
- 사용자가 미리 해 줄 일은 없다.
