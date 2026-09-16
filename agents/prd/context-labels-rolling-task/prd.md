---
topic: "agent-context-labels: 세션 전체를 조망하는 누적 task 라벨"
status: "ready"
human_approval: "approved"  # user 2026-09-16 verbatim: ㅇㅇ 다 승인하게 너가 orchestrator가 되서 codex luna max로 해서 작업을 시켜 implementor로 해서 작업 ㄱㄱ
review_profile: "standard"
review_rationale: "사이드바 라벨의 의미와 프롬프트·출력 스키마·발행 토큰이 바뀌는 사용자 가시 변경이며, 영속 데이터는 플러그인 상태 파일 한 필드뿐이고 자격 증명·외부 부작용은 없다."
source_intake: "current conversation"
created_at: "2026-09-16"
updated_at: "2026-09-16"
---

# PRD: agent-context-labels: 세션 전체를 조망하는 누적 task 라벨

## Goal

Herdr 사이드바에서 여러 에이전트를 돌리는 호연이 한 pane을 보고 "이 세션이 원래 뭘 하려던 건지"를 세션을 다시 훑지 않고 알 수 있게 한다.
지금 `$summary`는 최근 사용자 턴 몇 개만 요약해서 세션이 길어질수록 "이전 작업 계속 진행", "그럼 sasu 어케 써?" 같은 국소 제목이 되고, 그래서 사용자는 pane을 열어 세션을 다시 읽게 된다.
이 PRD는 라벨을 세션 단위의 누적 `task`로 바꾼다: 첫 요청만이 아니라 사용자가 보낸 요청들을 묶어 큰 그림을 잡고, 새 요청이 하위 작업이면 유지하고 목표가 바뀌면 교체한다.
UI에는 우선 `$task` 한 줄만 보인다.

## Non-goals

- `$progress`(턴 단위 "지금 뭘 하고 어디까지")를 사이드바에 보이는 것: 사용자가 "UI는 우선 task만"으로 정했다. 모델은 `progress`를 같은 호출에서 내고 상태·로그에 남기지만 토큰으로 발행하지 않는다. 한 보고의 토큰 상한(16)이 지금 꽉 차 있어 발행하려면 보고를 둘로 나눠야 하며, UI가 필요로 할 때 그 결정과 함께 한다(D-05).
- 턴 중간의 라벨 갱신: 호출은 지금처럼 턴 시작·끝 두 번뿐이다. 긴 턴 동안 라벨은 시작 때 값으로 머문다. 필요해지면 "N분마다 1회" 옵션으로 재검토한다.
- 사이드바 행 수·색·정렬 변경: `$summary` 자리에 `$task`가 들어가는 것 외에 없다.
- 라벨의 다국어화: 지금처럼 한국어 8~30자다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 라벨은 세션 단위 `task` 하나로, 누적(rolling) 방식으로 유지한다. 호출마다 모델은 이전 `task`와 직전 호출 이후 새 사람 턴(델타)만 받고, 새 요청이 이전 task의 하위 작업이면 유지, 다른 목표면 교체(또는 "A → B"로 이어붙임)한다. 이전 task가 없으면(처음 보는 pane, 상태 소실) 세션의 첫 사람 턴 3개와 마지막 8개를 생략 표시와 함께 준다. | "전체 조망은 맨 처음만 보기보단 사용자의 요청을 묶어서 얻으면 좋을 것 같은데"; 델타만 주면 입력이 `task(30자)+새 턴 1~2개`로 고정되어 세션 길이와 무관 |
| D-02 | 모델 출력은 `{"task", "task_changed", "progress", "expected_reply", "attention"}` 다섯 필드(이 순서)이며 `SCHEMA_VERSION`을 `context_label.v2`로 올린다. `task_changed`가 false면 모델이 돌려준 `task` 문자열을 버리고 이전 문자열을 그대로 쓴다. `attention`·`expected_reply` 규칙은 v1과 같다. | 모델이 매번 살짝 다르게 써서 사이드바가 흔들리는 것을 막고, 로그에서 "언제 큰 그림이 바뀌었나"를 셀 수 있게 |
| D-03 | 턴 시작 호출은 델타(방금 온 사람 턴)로 `task`를 판정하고 `progress`는 착수 문구가 된다. 턴 끝 호출은 델타가 비어 있으므로 `task`는 유지되고 `progress`·`attention`을 판정한다. 턴당 최대 2회는 그대로다. | 요청은 사용자가 말할 때만 바뀐다; 기존 `analysis_phase` 구조 재사용 |
| D-04 | 발행 토큰은 `summary`를 `task`로 바꾼다. 같은 슬롯을 쓰므로 16개 상한 안이다. watcher는 시작 시 pane마다 한 번 옛 `summary` 토큰을 지우는 보고를 보낸다. README의 사이드바 설정과 이 machine의 `~/.config/herdr/config.toml`은 `$task`로 바꾼다. | 원칙 1(옛 토큰을 남기지 않음); 토큰 이름이 의미를 말해야 한다 |
| D-05 | `progress`는 `PersistedDisplayState`에 저장하고 구조화 로그(`analysis_recorded`: pane, 국면, task_changed, 각 필드 길이)에 남기되 토큰으로 발행하지 않는다. | "UI는 우선 task만 보여주게"; 16-토큰 상한 |
| D-06 | `task`는 상태 파일(`PersistedDisplayState.task`)과 Herdr 토큰 양쪽에 남아 watcher 재시작 때 provider 호출 없이 복원된다. 상태 파일이 없으면 D-01의 초기 입력으로 다시 도출한다. | 재시작마다 다시 물으면 요청 예산 낭비; 메모 `herdr-pane-metadata-tokens` |
| D-07 | "Refresh active pane summary" 액션은 이전 task를 버리고 D-01의 초기 입력으로 다시 도출한다. 이름은 "Refresh active pane task"로 바꾼다. | 누적이 드리프트했을 때 사용자가 되돌릴 길이 하나는 있어야 한다 |
| D-08 | 사용자 중단(`Interrupted`)·provider 불가·잘못된 응답에서는 task를 건드리지 않고 기존 실패 로그 규칙을 따른다. | 원칙 4, 10; 기존 동작 유지 |
| D-09 | 검증: 프롬프트·파서 테스트(task_changed true/false, 잘못된 형태, 길이 규칙), 입력 구성 테스트(초기 입력의 3+8 절단과 생략 표시, 델타 입력), 재시작 복원 테스트. `scripts/verify-cargo.sh test`가 게이트이고 `tests/cli.rs`는 토큰 이름 외 수정 없이 통과한다. | 원칙 12 |
| D-10 | `hide-session` PRD(사람 턴 분류·전체 읽기) 위에 쌓는다. 이벤트 watcher PRD와는 독립이라 순서는 그 뒤여도 앞이어도 된다. | 델타와 초기 입력 모두 `kind == Human` 이벤트에 의존 |
| D-11 | Delivery: `agents/config.json`의 sasu PR 모드. README·AGENTS.md·`docs/status-model.md`(라벨 언급이 있으면)를 같은 PR에서 고친다. | 저장소 규칙 |
| D-12 | 원칙 intake: engineering/principles.md(commit 653c462) 전부 읽음. design/principles.md도 읽음(사이드바 행이 사용자가 보는 화면): 4번(파생 상태를 보여준다)이 task 라벨 자체를 뒷받침하고, 9번(데이터가 만들 수 있는 모든 상태)은 B7·B8로, 12번(실제 문자·폰트에서 가독성 확인)은 B13으로 번역했다. 나머지 design 규칙은 행 레이아웃을 바꾸지 않아 해당 없음. | `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 사이드바 둘째 줄 `$task`에 세션이 하려는 일의 한국어 제목(8~30자)이 보인다. 예: 첫 요청 "Task Factory 오케스트레이터 구축" 뒤에 "sasu 어케 써?", "다 정리됐어?"가 이어져도 라벨은 "Task Factory 오케스트레이터 구축"으로 남는다. | D-01 |
| B2 | 목표가 다른 요청("이제 Herdr 종료 안정화 PRD 써")이 오면 그 턴이 시작될 때 라벨이 바뀐다. | D-01, D-03 |
| B3 | 요청이 이어지는 동안 라벨 문자열은 글자 하나 바뀌지 않는다(모델이 다시 써도 이전 문자열 유지). | D-02 |
| B4 | watcher를 재시작해도 각 pane의 task가 provider 호출 없이 그대로 보인다. 상태 파일이 지워졌으면 다음 턴 경계에서 세션의 처음 3개와 마지막 8개 사람 턴으로 다시 만들어진다. | D-06 |
| B5 | "Refresh active pane task" 액션을 실행하면 포커스된 pane의 task가 세션 전체 기준으로 다시 도출되어 드리프트한 라벨이 바로잡힌다. | D-07 |
| B6 | 로그 `analysis_recorded`에 국면(start/end)·`task_changed`·필드 길이가 남고 대화 본문은 남지 않는다. `progress`는 상태 파일에서 볼 수 있지만 사이드바에는 나타나지 않는다. | D-05 |
| B7 | Esc로 턴을 중단하면 `‖`가 보이고 task는 그대로다. provider가 답하지 못하거나 답이 형식에 맞지 않아도 직전 task가 유지되고 기존 실패 로그가 남는다. | D-08 |
| B8 | 아직 사람 턴이 없는 pane(방금 연 에이전트)은 task 없이 상태 심볼·에이전트·경과 시간만 보인다. | D-01 |
| B9 | 한 턴에 provider 호출은 최대 2회(시작·끝)이며, 턴 끝 호출에서 task는 바뀌지 않는다. | D-03 |
| B10 | 옛 `$summary` 토큰은 watcher 시작 후 사라지고, README의 사이드바 설정 예시는 `$task`를 쓴다. `$summary`를 남겨 둔 설정은 둘째 줄이 비어 보인다. | D-04 |
| B11 | 프롬프트·파서·입력 구성·재시작 복원 테스트가 있고 `scripts/verify-cargo.sh test`·`build`가 통과한다. | D-09 |
| B12 | 플러그인 README·AGENTS.md가 누적 task 규칙, v2 스키마, 발행 토큰, refresh 의미를 설명한다. | D-11 |
| B13 | 30자 한국어 task가 실제 Herdr 사이드바 폭에서 온전히 보이거나 `…`로 깔끔히 잘리는 것을 실제 사이드바 화면으로 확인한다(라틴 예시로 대체하지 않음). | D-12 |

## Technical structure

- `plugins/agent-context-labels/src/context_label.rs`: 시스템 프롬프트와 출력 스키마를 v2로 교체, `Analysis`에 `task`, `task_changed`, `progress` 반영.
- `plugins/agent-context-labels/src/lib.rs`: `analysis_context`가 (이전 task, 델타 사람 턴) 또는 초기 입력(첫 3 + 마지막 8)을 만든다. `PersistedDisplayState`에 `task`·`progress`·`task_input_cursor`(마지막으로 모델에 준 사람 턴 위치) 추가, `summary` 필드는 `task`로 이름 변경(옛 상태 파일의 `summary`는 시작 시 한 번 읽어 `task`로 옮기고 버린다). `metadata_arguments`가 `task` 토큰을 발행.
- `herdr-plugin.toml`·스크립트: 액션 id `refresh-active-pane-task`.
- `hide-session` crate의 `kind == Human` 이벤트와 커서에 의존. herdr-core·셸·FFI 변경 없음. 새 의존성 없음.

## Risks

- 누적 요약은 "A → B → C"로 갈수록 초반이 압축된다. 30자 제목의 본질이라 허용하고, refresh 액션이 되돌릴 길이다.
- 모델이 `task_changed`를 과하게 true로 내면 라벨이 자주 바뀐다. 프롬프트가 "하위 작업이면 유지"를 명시하고 픽스처 테스트가 판정 예시를 고정하지만, 실제 빈도는 로그의 `task_changed` 비율로 배포 후 확인한다.
- 사용자가 미리 해 줄 일: 머지 후 `~/.config/herdr/config.toml`의 `$summary`를 `$task`로 바꾸고 `herdr server reload-config`. 그 전까지 둘째 줄이 비어 보인다.
