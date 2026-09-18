---
topic: "사이드바 에이전트 행과 pane 헤더: 안정된 세션 이름, 상태가 고르는 둘째 줄, 플러그인의 task/progress/expected_reply"
status: "ready"
human_approval: "approved"  # user 2026-09-18 verbatim: ㅇㅇ 승인 /implement ㄱㄱ opus5로 new pane ㄱㄱㄱ
review_profile: "standard"
review_rationale: "사이드바 행·pane 헤더·Herdr 탭 이름이 바뀌는 사용자 가시 변경이며, 플러그인이 Herdr에 agent.rename·tab.rename을 쓰기 시작한다. 자격 증명·외부 서비스·영속 데이터 변경은 없고, 새로 쓰는 값은 소유권 규칙으로 보호된다."
source_intake: "agents/interview/agent-row-identity/qa-log.md"
created_at: "2026-09-18"
updated_at: "2026-09-18"
---

# PRD: 사이드바 에이전트 행과 pane 헤더: 안정된 세션 이름, 상태가 고르는 둘째 줄, 플러그인의 task/progress/expected_reply

## Goal

여러 에이전트를 돌리는 호연이 사이드바를 한 번 훑고 "누가 내 답을 기다리나, 뭘 답하면 되나, 나머지는 뭘 하고 있나"를 pane을 열지 않고 알 수 있게 한다.
지금 행의 제목은 `chat title → summary 토큰 → Herdr 에이전트 이름 → 워크스페이스 라벨` 순서라, 라벨이 없으면 한 워크스페이스의 모든 행이 워크스페이스 이름(`task-factory`)으로 보이고, 라벨이 있어도 턴마다 바뀌는 `task`가 이름 자리를 차지해 "아까 그 결제 건"을 다시 찾을 수 없다.
둘째 줄은 `Working`, `Idle`처럼 마크가 이미 말한 것을 한 번 더 쓰고, 플러그인이 이미 계산하는 `progress`와 `expected_reply`는 어디에도 보이지 않는다.

이 PRD는 세 가지를 바꾼다.

1. 행의 제목은 세션당 한 번 정해지는 이름이 된다. Claude Code가 세션 파일에 쓰는 `ai-title`, Codex는 첫 사람 턴. 플러그인이 이 이름을 Herdr 에이전트 이름과 탭 라벨로 써 넣으므로 Herdr TUI에서도 같은 이름이 보인다.
2. 둘째 줄은 상태가 내용을 고른다. 내 답을 기다리는 행(`?` `!` `×`)과 아직 안 본 완료 행(`✓`)은 상태 단어 + `expected_reply`(없으면 `progress`), 일하는 행(`●`)은 `progress`만, 본 뒤 쉬는 행(`○`)은 한 줄.
3. pane 헤더는 같은 규칙을 한 줄로 편다: `이름 · [단어] 문장`.

기준 화면은 인터뷰 중 실제 토큰(Inter 11/10/9pt, `#1D1F21`, 380pt)으로 렌더해 사용자가 승인한 `row-mock4`이며, `design/hide.pen`의 `Screen / Sidebar / Agent row states`와 `Screen / Pane / Header states` 보드로 옮긴다.

## Non-goals

- `progress`를 단계(착수/진행 중/완료)로 구조화하는 것: 실측 결과 자유 문장 한 줄("#1/#8 머지됨, #9 정지 상태로 재개 필요")이 그대로 유용하다.
- 상태 마크의 크기·모양·색 변경: `docs/status-model.md`의 공유 상태 계약은 그대로다.
- instrumentation `?` 마크의 위치 변경: PR #93 이전 helper 때문에 모든 행에 붙어 있었을 뿐이고, 재설치 후 새 pane에서 사라지는 것을 확인했다. 세 자리(헤더, 행, Overview) 규칙은 유지한다.
- 사용자가 직접 지은 이름·라벨을 바꾸는 것: `herdr agent new <NAME>`, `agent rename`, 사용자가 붙인 탭 라벨은 절대 덮어쓰지 않는다.
- 워크스페이스 라벨 정정(`w7J`의 `task-factory`): 이 PRD와 무관한 운용 작업이다.
- 라벨 생성 빈도·provider 선택·프롬프트의 task 판정 규칙 변경: `context-labels-rolling-task` PRD의 D-01~D-03을 그대로 둔다. 이 PRD가 프롬프트에 손대는 것은 `expected_reply`의 길이·어미뿐이다.
- 두 에이전트 이상이 든 탭의 이름: 그대로 둔다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 행의 정체성 ladder는 `chat title → Herdr 에이전트 이름 → task → 워크스페이스 라벨`이 된다. `summary` 토큰 읽기와 `MISSING_SUMMARY`("Check agent-context-labels settings")는 삭제한다. `task`는 이름이 없을 때의 대체일 뿐 더는 이름이 아니다. | 인터뷰 D-01, D-02, D-14; 원칙 1(옛 경로 삭제) |
| D-02 | 세션 이름은 플러그인이 만든다. Claude: 세션 JSONL의 마지막 `ai-title` 레코드(`aiTitle`). Codex: 첫 `Human` 이벤트를 공백 정리 후 30자로 자른 것(`normalize_task`와 같은 규칙). 둘 다 없으면 이름을 쓰지 않는다. AI 호출은 없다. | 인터뷰 D-03; 세션당 한 번 정해지는 값이라 행이 안정된다 |
| D-03 | 플러그인은 이름을 `agent.rename`으로 Herdr 에이전트 이름에 쓴다. 조건: 에이전트에 이름이 없거나, 이름이 플러그인이 마지막으로 쓴 값과 같을 때만(`PersistedDisplayState.plugin_name`). `ai-title`이 바뀌면 같은 조건에서 다시 쓴다. 토큰 슬롯을 쓰지 않고, Herdr TUI·`herdr agent list`·hide가 한 이름을 본다. | 인터뷰 D-11; 메모 herdr-pane-metadata-tokens(이름은 운영자 소유 텍스트) |
| D-04 | 플러그인은 탭도 같은 소유권 규칙으로 `tab.rename`한다. 조건: 탭 라벨이 Herdr 생성형(`^\d+$` 또는 `^Tab \d+$`)이거나 플러그인이 마지막으로 쓴 값(`plugin_tab_label`)과 같고, 그 탭의 에이전트 pane이 정확히 하나일 때. 탭 라벨 상한은 셸의 `tabTitleMaxWidth`(200pt)가 처리한다. | 인터뷰 D-12 |
| D-05 | 플러그인은 `progress`와 `expected_reply`를 토큰으로 발행한다. 기존 보고가 16개 상한이라 같은 source로 두 번째 `pane.report_metadata`를 보낸다(pane당 32개 상한 안: 16 + 2 + hooks 3). `expected_reply`는 프롬프트에서 40자 이내 명령형("~하세요", "~을 선택")으로 요구하고 파서가 40자에서 자른다. 두 토큰은 상태 파일에 이미 있는 값이라 재시작 시 provider 호출 없이 다시 발행된다. | 인터뷰 D-04, D-05, D-13; Herdr 값 상한 80자 |
| D-06 | 둘째 줄은 코어가 상태로 고른다(`sidebar.rs`). 강조 행(`needs_you` 그룹과 안 본 `done`): 상태 단어 + `expected_reply`, 비어 있으면 `progress`. `working`: `progress`만. 본 `idle`과 `unknown`: 없음. 문장이 하나도 없으면 강조·working 행은 지금처럼 상태 단어만 남긴다(빈 상태). 결과는 `AgentChipSnapshot.detail`과 새 필드 `status_word_visible`로 셸에 내려간다. | 인터뷰 D-06, D-07, D-15; 코어가 UI 상태를 소유(`docs/ARCHITECTURE.md`) |
| D-07 | 둘째 줄 타이포: 문장은 `Typography.caption`(10pt) regular, 상태 단어는 caption medium + 상태색. 문장 색은 행의 emphasis를 따른다: 강조 행 `primary`, working 행 `secondary`. 새 토큰 없음. `muted` 3단계는 렌더에서 회색 둘이 구분되지 않아 버렸다. 한 줄, 꼬리 truncation, 전문은 툴팁·접근성 라벨. | 인터뷰 D-08; DESIGN.md "상태색은 마크 색" |
| D-08 | pane 헤더는 28pt 한 줄을 지키고 제목을 `이름 · [단어] 문장`으로 편다. 이름 caption semibold `primary`, ` · ` `muted`, 단어 caption medium 상태색, 문장 caption regular에 D-07의 emphasis 색. 문장은 자르지 않은 원문(`expected_reply` 40자, `progress`)이고 truncation은 middle에서 tail로 바꾼다. `ViewThatFits`로 폭이 모자라면 문장부터, 그다음 단어를 떨군다. 셸 작업 문자열(` · forking…`, ` · reopening…`)은 지금처럼 문장 자리보다 우선한다. | 인터뷰 D-10; DESIGN.md "Pane header lineage and ownership" |
| D-09 | `PaneHeaderPresentation.title`의 ladder는 `Herdr pane 라벨 → 코어 정체성(D-01) → 터미널 제목 → 워크스페이스 라벨 → pane id`로 유지하되, 코어 정체성이 `identity_label`이므로 헤더와 행이 같은 이름을 보인다. | 헤더와 사이드바가 한 pane을 다르게 부르지 않는다(PRD B5, B10, B34 계승) |
| D-10 | 접근성: 행의 accessibility 라벨은 `이름, 에이전트 종류, 상태 단어, 문장` 순서로 상태 단어를 항상 포함한다(화면에서 빠져도). 헤더도 같다. | 마크만으로 상태를 전하지 않는다(design 7의 역방향) |
| D-11 | 플러그인의 Herdr 호출은 `hide-herdr-client`의 `request`로 소켓 메서드 `agent.rename`, `tab.rename`을 쓴다(둘 다 pinned 계약에 있음). CLI 호출 없음. 실패는 `agent_rename_failed`/`tab_rename_failed` 로그 한 줄로 남기고 다음 스캔에서 재시도하지 않는다(이름이 안 붙은 것이 화면에 보이는 실패다). | Herdr API 계약 규칙; 원칙 10 |
| D-12 | 문서: DESIGN.md의 "Native Git and lineage tokens" 행 규칙과 "Pane header lineage and ownership"에 둘째 줄 표와 헤더 한 줄 규칙을 적고, `docs/status-model.md`에 상태별 둘째 줄 표를, 플러그인 README에 이름·탭 소유권 규칙과 두 토큰을 적는다. `design/hide.pen`의 `Screen / Sidebar / Projects` 보드는 새 행으로 다시 그리고 `Screen / Pane / Conversation - *` 보드의 헤더도 맞춘다. | 저장소 규칙: 문서는 행동과 같은 변경에서 |
| D-13 | 검증: `scripts/verify-cargo.sh test`(코어 ladder·상태별 둘째 줄·헤더 문자열, 플러그인 이름 도출·소유권·두 번째 보고), `scripts/verify-swift.sh test`, `node scripts/check-design-contract.mjs`. 라이브: 설치된 앱에서 Claude pane과 Codex pane 각각 새로 띄워 이름·탭·둘째 줄을 스크린샷으로 확인하고, 한글 30자 이름과 40자 `expected_reply`가 실제 사이드바 폭에서 잘리는 모양을 본다. 증거는 `agents/runs/agent-row-identity/`. | 인터뷰 D-16; design 12 |
| D-14 | 원칙 intake: engineering/principles.md 전부(1 옛 `summary` 경로 삭제, 4·10 이름/탭 실패를 로그와 빈 이름으로 드러냄, 5 이름은 플러그인·표시 규칙은 코어·그리기는 셸, 7 `hide-session`·`hide-herdr-client` 재사용, 15 토큰 32개 상한 안). design/principles.md 전부(3 가장 잦은 행동 "누가 나를 기다리나"에 줄을 씀, 4 둘째 줄이 파생 상태, 7 단어는 마크가 못 하는 곳에만, 8 새 컨테이너 없음, 9 빈 상태·seen·delegated 행 정의, 11 세 후보에서 사용자가 골랐음, 12 실제 한글로 1:1 렌더). | `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Claude pane을 새로 열어 첫 요청을 보내면 몇 초 뒤 행 제목이 Claude가 지은 세션 이름(예: "Hook 버그 확인")이 되고, 그 뒤 턴이 이어져도 제목은 바뀌지 않는다. Codex pane은 첫 요청의 앞 30자가 제목이 된다. | D-02, D-03 |
| B2 | 답을 기다리는 행(`?`)은 둘째 줄에 노란 `Question`과 "A/B 선택 후 DB 마이그레이션 승인"처럼 내가 할 일이 흰 글자로 보인다. 승인(`!`)·오류(`×`) 행은 각 색의 단어와 `progress`가 보인다. | D-06, D-07 |
| B3 | 아직 안 본 완료 행(`✓`)은 초록 `Done`과 "#1/#8 머지됨, #9 재개 필요"처럼 무엇이 끝났는지가 흰 글자로 보인다. | D-06, D-07 |
| B4 | 일하는 행(`●`)은 단어 없이 회색 문장("hook 보고 경로를 소켓 호출로 교체 중")만 보이고, 본 뒤 쉬는 행(`○`)은 제목 한 줄이다. | D-06, D-07 |
| B5 | 문장은 한 줄에서 `…`로 잘리고, 행에 마우스를 올리면 전문이 툴팁에 보인다. VoiceOver는 화면에 단어가 없는 행에서도 "이름, claude, Working, 문장"을 읽는다. | D-07, D-10 |
| B6 | 플러그인 watcher가 없거나 아직 라벨이 없는 pane은 지금처럼 제목(이름 또는 워크스페이스 라벨)과 상태 단어만 보인다. 둘째 줄이 통째로 비는 행은 없다. | D-06 |
| B7 | pane 헤더는 `● ❋ Hook 버그 확인 · hook 보고 경로를 소켓 호출로 교체 중`, 답을 기다리면 `? ❋ 결제 멱등키 PR · Question A/B 중 하나를 선택하고 DB 마이그레이션 실행 승인 여부를 지시하세요`처럼 한 줄로 보인다. 긴 문장은 꼬리가 `…`로 잘린다. | D-08 |
| B8 | pane을 세 개 이상 나눠 헤더가 좁아지면 문장이 먼저 사라지고 이름과 단어가 남는다. 더 좁아지면 이름만 남는다. | D-08 |
| B9 | fork·reopen 중인 pane의 헤더는 지금처럼 ` · forking…`을 보이고 문장은 그동안 숨는다. | D-08 |
| B10 | 사용자가 `herdr agent new impl-x`로 이름을 준 에이전트는 세션 이름이 생겨도 `impl-x`로 남는다. 플러그인이 붙인 이름은 Claude의 `ai-title`이 바뀌면 따라 바뀐다. | D-03 |
| B11 | `Tab 3`처럼 Herdr가 지은 라벨의 탭에 에이전트 pane이 하나면 탭 이름이 세션 이름이 된다. 사용자가 이름을 바꾼 탭과 에이전트가 둘 이상인 탭은 그대로다. | D-04 |
| B12 | `herdr agent list`에서 각 pane의 토큰에 `task`, `progress`, `expected_reply`가 보이고, `expected_reply`는 40자를 넘지 않는다. watcher를 재시작해도 세 값이 provider 호출 없이 다시 붙는다. | D-05 |
| B13 | 옛 플러그인(`summary` 발행)과 새 코어를 섞어 쓰면 행 제목은 Herdr 이름 또는 워크스페이스 라벨이 되고 둘째 줄은 상태 단어만 남는다. 앱을 열면 아무 경고 없이 이 빈 상태로 보인다(플러그인 업그레이드가 운용 후속). | D-01, D-06 |
| B14 | 코어·플러그인 테스트와 세 게이트(`verify-cargo.sh test`, `verify-swift.sh test`, `check-design-contract.mjs`)가 통과한다. | D-13 |
| B15 | 설치된 앱에서 Claude pane 하나, Codex pane 하나를 새로 띄운 스크린샷이 B1~B4·B7·B11을 보이고, 한글 30자 이름과 40자 문장의 잘림이 실제 폭에서 확인된다. | D-13 |
| B16 | `design/hide.pen`에 새 행의 상태별 프레임(question, approval, error, done unseen, working, seen idle, delegated, no-label)과 헤더 프레임(working, question, narrow, forking)이 `Screen /` 보드로 있고 `check-pen.mjs`가 통과한다. | D-12 |

## Technical structure

- `herdr-core/src/sidebar.rs`: `token_string(tokens, "summary")`와 `MISSING_SUMMARY` 삭제; `task`, `progress`, `expected_reply` 읽기; `agent_identity_label` ladder 변경; 상태 그룹 → (단어 표시 여부, 문장) 테이블 함수 하나. `SidebarAgentSnapshot`에 `task`, `progress`, `expected_reply`, `detail`, `status_word_visible` 추가, `summary` 삭제. `model.rs`의 `AgentChipSnapshot.detail`이 이 문장을 싣는다.
- `herdr-core/src/runtime.rs:1271-1290` 근처 `summary` 필터와 `CoreBridge.swift`의 fixture `summary`를 같은 이름으로 옮긴다.
- `macos/Sources/HerdrMacOS/CoreBridgeWorkspaceSnapshot.swift`: `summary` 디코딩 삭제, 새 필드 디코딩.
- `macos/Sources/HerdrMacOS/AgentRow.swift`: 둘째 줄을 `status_word_visible`·`detail`로 그린다(단어 caption medium 상태색, 문장 caption regular emphasis 색, `firstTextBaseline`). 현재 `Text(presentation.statusLabel)` 줄이 이 줄로 흡수된다. `qualifier`, `failed`, instrumentation help는 문장 뒤 그대로.
- `macos/Sources/HerdrMacOS/ShellView.swift`: `PaneHeaderPresentation`에 `sentence`(단어·문장) 추가, 제목 `Text`를 세 조각으로 나누고 `ViewThatFits` 2단계, `truncationMode(.tail)`.
- `plugins/agent-context-labels/src/lib.rs`: `session_name(agent, events)`(Claude `ai-title` 마지막 레코드, Codex 첫 Human 30자); `PersistedDisplayState`에 `plugin_name`, `plugin_tab_label`; `rename_if_owned`(`agent.rename`, `tab.rename`)와 두 번째 `pane.report_metadata`; `context_label.rs` 프롬프트에 `expected_reply` 40자 명령형 규칙, 파서 절단.
- `hide-session/src/lib.rs`: Claude 라인 파서가 `ai-title`을 `ParsedSession.title: Option<String>`로 넘긴다(이벤트가 아니라 세션 속성). Codex는 `None`.
- `hide-herdr-client`: 변경 없음(`request`로 두 메서드 호출).
- 문서·디자인: DESIGN.md, docs/status-model.md, plugins/agent-context-labels/README.md·AGENTS.md, design/hide.pen. 새 의존성 없음. FFI 변경 없음(스냅샷 JSON 필드만 늘고 준다).

## Risks

- `ai-title`은 Claude Code의 비공개 세션 파일 형식이다. 레코드가 사라지면 Claude pane은 Codex처럼 첫 사람 턴으로 떨어지며, 그때 로그 `session_title_missing` 한 줄로 드러난다.
- `agent.rename`은 Herdr가 `agent_status_changed`/`agent_list` 갱신을 내므로 플러그인의 이벤트 루프가 자기 rename에 반응해 재분석하면 안 된다. 이름 변경 이벤트는 라벨 분석 트리거에서 제외한다.
- `expected_reply` 40자 제한은 모델이 지키지 않을 수 있어 파서가 자른다. 잘린 문장이 의미를 잃는 경우는 헤더(원문)와 툴팁으로 보완된다.
- 실측에서 한 세션이 60초 provider 시한을 두 번 넘겼다(입력 2,301자). 이 PRD의 범위 밖이지만 둘째 줄이 오래 비는 원인이 될 수 있으므로 별도 조사 항목으로 남긴다.
- 운용 후속(사용자): 머지 후 플러그인 업그레이드(`herdr plugin install modakbul-gongbang/hide/plugins/agent-context-labels`), `~/.config/herdr/config.toml`의 `$summary`→`$task`와 `$progress`·`$expected_reply` 추가, `herdr server reload-config`, 앱 재설치. 그 전까지는 B13의 빈 상태다.
