---
topic: "Cmd+Shift+Z로 닫은 pane/tab 되살리기"
status: "ready"
human_approval: "approved"  # user 2026-09-14 verbatim: 승인 ㅇㅇㅇㅇ
review_profile: "standard"
review_rationale: "사용자 눈에 보이는 단축키·메뉴·pane 재생성과 agent 세션 resume을 추가하지만, 영구 데이터 변경·인증·외부 비용·프로덕션 롤아웃 위험은 없다."
source_intake: "agents/interview/hide-reopen-closed-pane/qa-log.md"
target_repository: "modakbul-gongbang/hide"
target_branch: "main"
screen_evidence: "screenshot"
created_at: "2026-09-14"
updated_at: "2026-09-14"
---

# PRD: Cmd+Shift+Z로 닫은 pane/tab 되살리기

## Goal

Hide 사용자가 pane, tab, 파일 탭을 실수로 닫았을 때 Cmd+Shift+Z(또는 Window › Reopen Closed Tab)로 마지막에 닫은 것부터 순서대로 원래 자리에 되살리고, agent가 돌던 pane은 그 세션이 이어진 채로 돌아오게 한다. 지금은 닫기가 되돌릴 수 없고, Herdr에는 복원 API가 없어 닫힌 agent 대화를 다시 찾으려면 session id를 직접 알아내 수동으로 resume해야 한다.

## Non-goals

- 원격 기기의 pane/tab 복원. 원격에서 닫은 것은 스택에 쌓이지 않는다. 원격 제어 경로에 `agent.new`가 없어 새 소켓 경로가 필요하기 때문이며, 그 경로가 생기면 revisit한다 (D-13).
- Scratch chat pane과 Browser plugin pane 복원. Scratch는 되살릴 세션이 없고 Browser는 별도 생명주기를 가진다. Browser pane 단독 닫기는 스택에 쌓이지 않고, 섞인 tab에서는 그 자리를 비운 채 notice로 알린다. 브라우저 세션 복원 요구가 생기면 revisit한다 (D-06, D-21).
- 다른 클라이언트(herdr TUI/CLI, 다른 agent)가 닫거나 프로세스가 스스로 종료돼 사라진 pane의 복원. undo는 사용자 자신의 행동만 되돌린다. agent 자체 종료 복원 요구가 생기면 revisit한다 (D-12).
- 터미널 스크롤백과 파일 탭 스크롤 위치 복원. 재생성 방식이라 돌아오지 않는다 (D-14).
- 앱 재시작 후 스택 유지. 스택은 메모리 전용이며 재시작 시 비운다 (D-07, D-19).
- Explorer 휴지통 되돌리기(Cmd+Z). 삭제된 파일의 탭은 되살리지 않고 Finder 휴지통을 안내한다. 별도 PRD로 다룬다 (D-25).
- 복원 확인 다이얼로그. 확인은 닫기에만 있고 되살리기는 즉시 실행한다 (D-14).
- 원칙 intake에서 번역하지 않은 규칙: engineering 9(구조화 로그)는 기존 `HideLaunchTrace` 관례를 따르므로 별도 행을 두지 않고, design 1·2·11·12는 이 변경에 새 목록·폼·레이아웃 후보가 없어 해당하지 않는다 (D-33).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Herdr 0.8.2 pin에는 pane/tab 복원 method가 없다. 되살리기는 Hide가 닫기 전 사실을 기억했다가 `tab.create` / `pane.split` / `agent.new` / `layout.apply`로 새로 만드는 방식이다. | 사실: `contracts/herdr-api.schema.json` method 목록, `herdr agent new --help` |
| D-02 | resume 명령은 Claude `--resume <session_id>`(fork의 `--fork-session` 없이), Codex `resume <session_id>`. session id는 hooks가 `pane.report_agent_session`으로 보고한 값이다. 그 외 agent kind는 D-11 저하 경로. | 사실: `fork.rs:39-47`, `model.rs:288`, `codex resume --help` (D-16, D-17 확정) |
| D-03 | Herdr는 tab의 마지막 pane을 닫으면 tab을, workspace의 마지막 pane을 닫으면 workspace를 함께 제거한다. 그래서 pane 항목도 tab·workspace 재생성이 필요할 수 있다. | 사실: `session_sync.rs:2293-2340` |
| D-04 | Cmd+Shift+Z는 shell 단축키 레지스트리에 비어 있지만 텍스트 에디터에서는 시스템 Redo다. 현재 닫기는 Cmd+W(통합), Cmd+Shift+W(pane)다. | 사실: `ShellMenuCommand.swift:74-92`, `PaneShortcutSettings.swift:33` |
| D-06 | 복원 대상은 Herdr pane(agent 있음/없음), Herdr tab, 편집기 파일 탭이며 하나의 "최근 닫은 것" 스택으로 통합한다. Herdr pane/tab만 두는 (a)안은 기각. Scratch pane은 제외. | Q1 "b로 가면 나을 것 같기는 한데?", Scratch 제외는 Q2 서두 제안에 이의 없음 |
| D-07 | Cmd+Shift+Z는 앱 전역 LIFO 스택을 pop한다. 반복하면 그 전에 닫은 것이 순서대로 돌아온다. workspace를 가리지 않고, 상한 20, 앱 재시작 시 비운다. "마지막 하나만"과 "현재 workspace만" 안은 기각. | Q2 "어어 맞아! a로", 상한·재시작은 Q10 "ㅇㅇ" |
| D-08 | 닫기 한 번 = 스택 항목 하나. pane 여러 개인 tab을 닫았다가 되살리면 tab 전체가 split 배치·비율과 각 pane의 agent 세션 resume까지 함께 돌아온다. pane 하나씩 되살리는 안과 빈 shell로만 되살리는 안은 기각. | Q3 "A" |
| D-09 | 텍스트 편집 중(파일 편집 탭, 검색·작성기 입력란)에는 Cmd+Shift+Z가 시스템 Redo로 남고, 그 외 포커스(터미널 pane, 탭 바, 사이드바)에서만 되살리기가 발동한다. Window 메뉴에 "Reopen Closed Tab"(Cmd+Shift+Z)을 두고 스택이 비면 비활성. 항상 되살리기가 이기는 안과 Cmd+Shift+T로 바꾸는 안은 기각. | Q4 "a", 메뉴는 Q10 "ㅇㅇ" |
| D-10 | 원래 자리 우선 복원. pane은 원래 tab에서 원래 이웃 pane 옆에 같은 방향으로 split, 이웃이 없으면 그 tab의 포커스 pane 옆에. tab은 원래 workspace의 원래 순서 위치에. workspace가 사라졌으면 같은 checkout으로 다시 만들고 그 안에 복원한다. 복원된 것으로 포커스를 옮긴다. 현재 위치에 붙이는 안과 workspace 소실 시 포기하는 안은 기각. | Q5 "a" |
| D-11 | 단계적 저하. session id가 없거나 resume 명령을 모르는 agent는 같은 cwd에 같은 종류의 새 agent 세션으로 열고 pane 헤더 notice로 이전 대화가 이어지지 않음을 알린다. cwd가 삭제됐으면 checkout 루트에서 열고 notice. Herdr 거부/timeout이면 아무것도 만들지 않고 항목을 스택에 남긴 채 notice. 빈 shell만 여는 안과 복원 포기 안은 기각. | Q6 "a" |
| D-12 | 스택에는 Hide에서 사용자가 닫은 것만 쌓인다: Cmd+W, Cmd+Shift+W, 헤더 ×, 닫기 확인 승인. 외부 닫힘·자체 종료는 제외. | Q7 "ㅇㅇㅇㅇ a" |
| D-13 | 원격은 제외한다. Q8에서 포함(b)을 골랐으나 원격 제어 경로에 `agent.new`가 없음을 보고 Q9에서 제외(c)로 바꿨다. | Q9 "c로 하자 ㅇㅇ" |
| D-14 | 되살리기는 확인 없이 즉시 실행. agent 없는 터미널 pane은 같은 cwd에 새 shell만 열고 notice 없음. 파일 탭은 같은 경로를 다시 열고 스크롤 위치는 복원하지 않음. 복원 후 notice는 저하 경우에만 표시. | Q10 "ㅇㅇ" |
| D-15 | 검증: 스택 동작·재생성 인자·저하 경로는 Rust 단위 테스트, 단축키·메뉴 라우팅은 Swift 테스트, 실제 resume은 격리 Herdr 서버(HERDR_SOCKET_PATH 분리)에서 Claude pane을 닫고 되살려 대화가 이어지는지 네이티브 e2e로 확인한다. | Q10 "ㅇㅇ" |
| D-18 | tab 항목은 닫기 직전 `layout.export(tab_id)` 트리를 담고, 되살릴 때 pane 노드의 command를 resume 명령으로 바꿔 `layout.apply(root, workspace_id, tab_label, focus)`로 만든다. 복원 fidelity는 split 토폴로지·방향·비율·각 pane cwd다. | 사실: schema `LayoutExportParams`/`LayoutApplyParams`/`LayoutNode` |
| D-19 | 스택은 core가 `Mutex<Runtime>` 안에서 소유하고, 확인된 close 이벤트를 처리하는 시점에 Herdr로 close를 보내기 전에 항목을 원자적으로 만든다. 스냅샷에는 스택 크기와 최상단 항목 라벨만 노출한다. | 사실: AGENTS.md Runtime Architecture |
| D-20 | 파일 탭은 자동 저장이고 닫기 전에 보류 저장을 flush하므로 잃는 dirty buffer가 없다. 되살리기는 디스크 내용을 다시 연다. | 사실: `CoreBridge.swift:3214-3218` |
| D-21 | Browser plugin pane은 비목표. 단독 닫기는 스택에 쌓지 않고, 섞인 tab을 되살릴 때는 터미널 pane만 복원하며 notice로 알린다. | Q11 1번 "ㅇㅇ" |
| D-22 | tab 부분 복원 원자성: tab 생성 자체가 실패하면 항목을 유지해 재시도 가능. tab이 생기면 항목은 소비되고, 실패한 pane은 각각 D-11 저하 경로로 채워 pane 개수를 맞춘다. pane 단위 재시도는 없다. | Q11 3번 "tab ㅇㅇㅇ" |
| D-23 | Window 메뉴 클릭은 포커스와 무관하게 항상 되살리기를 실행한다(Redo 예외는 단축키에만). | Q11 4번 "ㅇㅇ" |
| D-24 | session id가 있는데 resume이 거부·실패하면 session id 없음과 같은 저하로 처리하고 notice에 원래 session id를 표기하며 항목은 소비한다. | Q11 5번 "ㅇㅇㅇ" |
| D-25 | 파일 탭 예외: 파일이 삭제됨 → 열지 않고 notice(Finder 휴지통 안내), 항목 버림. 이미 열려 있음 → 그 탭에 포커스, 항목 소비. 디스크에서 바뀜 → 현재 내용으로 연다. 휴지통 undo 포함 안은 기각. | Q12 "우선 a ㅇㅇㅇ" |
| D-26 | 전달은 `agents/config.json`대로 worktree에서 구현하고 `main` 대상 PR로 배달하며 CI를 감시한다(watch, 수정 시도 2회). 로컬 커밋만 남기는 안은 채택하지 않는다. | agents/config.json delivery.mode=pr, worktree.enabled=true |
| D-27 | 가정: `layout.apply`의 command로 시작한 pane이 Herdr agent 목록·lineage에 등록되지 않으면, tab 항목은 `tab.create` 후 `agent.new --pane`으로 pane마다 채운다. 구현 초기에 확인. | 가정: qa-log checkpoint 2 highest_remaining_gap |
| D-28 | Herdr 거부/timeout 후 재시도는 같은 항목을 같은 idempotency key로 보내 중복 pane을 만들지 않는다. | engineering/principles.md 11 (commit 653c462), D-34로 사용자 채택 |
| D-29 | 되살리기 실패는 모두 사용자에게 보이는 notice로 끝나고, 조용히 항목만 버리는 경로는 없다. | engineering/principles.md 4, 10 (commit 653c462), D-34로 사용자 채택 |
| D-30 | 닫기 직후와 복원 결과 알림은 기존 pane 헤더 notice 패턴을 쓰고 새 배너나 카드를 만들지 않는다. | design/principles.md 5, 7, 8 (commit 653c462), D-34로 사용자 채택 |
| D-31 | 스택이 빈 상태, 되살리는 중, 부분 성공, 실패 상태를 각각 메뉴 비활성·헤더 대기 표시·notice로 드러낸다. | design/principles.md 9 (commit 653c462), D-34로 사용자 채택 |
| D-32 | 되살리기는 메뉴 한 번 또는 단축키 한 번으로 끝나며 추가 단계가 없다. | design/principles.md 3, 6 (commit 653c462), D-34로 사용자 채택 |
| D-33 | 원칙 intake: `engineering/principles.md`와 `design/principles.md`(commit 653c462)를 전문 읽었다. engineering 9는 기존 로그 관례를 따르고, design 1·2·11·12는 해당 화면 요소가 없어 번역하지 않았다. | sasu principles list |
| D-34 | 원칙에서 온 가정 D-28~D-32를 사용자 승인 결정으로 채택한다. | spec 게이트 번들 1번 "ㅇㅇ 우선 그렇게 해" |
| D-35 | tab이 만들어진 뒤에는 pane 생성 실패든 resume 실패든 그 pane 자리를 빈 shell 또는 새 세션으로 채우고 notice하며 항목은 소비한다. "항목 유지 + 재시도"는 tab 생성 자체가 실패한 경우에만. | spec 게이트 번들 2번 "ㅇㅇ 우선 그렇게 해" |
| D-36 | agent 없는 터미널 pane도 원래 cwd가 삭제됐으면 checkout 루트에서 열고 notice하며 항목은 소비한다. | spec 게이트 번들 3번 "ㅇㅇ 우선 그렇게 해" |
| D-37 | 파일 탭 경로는 있는데 열기 실패(권한 등)면 탭을 열지 않고 notice하며 항목은 스택에 남겨 재시도 가능하게 한다. | spec 게이트 번들 4번 "ㅇㅇ 우선 그렇게 해" |
| D-38 | 되살리기 관련 알림은 모달 없이 인라인이다. pane이 생긴 경우는 그 pane 헤더에, pane이 안 생긴 경우는 원래 자리의 이웃 pane 헤더 또는 tab 바에 표시한다. `interactionNotice` alert 모달은 쓰지 않는다. | "음 notice가 뭐야? 모달 뜨는거야?" → "a로"; `HideUI.swift:221-231, 2038` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Hide에서 Cmd+W, Cmd+Shift+W, 헤더 ×, 또는 닫기 확인 승인으로 Herdr pane·Herdr tab·파일 탭을 닫으면 그 항목이 "최근 닫은 것" 스택 맨 위에 쌓인다. 다른 클라이언트가 닫거나 프로세스가 스스로 끝난 pane, Scratch pane, Browser pane 단독 닫기, 원격 pane/tab은 쌓이지 않는다. | D-06, D-12, D-13, D-19, D-21 |
| B2 | 스택은 앱 전역이며 상한 20개다. 21번째 항목이 쌓이면 가장 오래된 항목이 버려진다. 앱을 재시작하면 스택은 비어 있다. | D-07 |
| B3 | Window 메뉴에 "Reopen Closed Tab"(⇧⌘Z)이 있고, 스택이 비어 있으면 비활성이며 그 상태에서 단축키를 눌러도 아무 일도 일어나지 않는다. | D-09, D-31 |
| B4 | 파일 편집 탭이나 텍스트 입력란(검색, 작성기)에 커서가 있을 때 Cmd+Shift+Z는 텍스트 Redo를 실행하고 스택은 건드리지 않는다. 터미널 pane, 탭 바, 사이드바에 포커스가 있을 때는 되살리기를 실행한다. | D-04, D-09 |
| B5 | Window 메뉴 항목을 클릭하면 포커스가 어디에 있든 되살리기를 실행한다. | D-23, D-32 |
| B6 | 되살리기는 확인 없이 즉시 실행되고, 스택 맨 위 항목을 꺼내 되살린다. 반복해서 누르면 그 전에 닫은 항목이 순서대로 되살아난다. | D-07, D-14, D-32 |
| B7 | agent가 있던 pane을 되살리면 원래 tab에서 원래 이웃 pane 옆에 같은 방향으로 split되어 나타나고, 그 pane으로 포커스가 이동하며, agent가 이전 세션을 이어받아(Claude `--resume`, Codex `resume`) 이전 대화가 보이는 상태로 시작한다. | D-02, D-10 |
| B8 | 원래 이웃 pane이 더 이상 없으면 그 tab의 포커스 pane 옆에 split된다. 원래 tab이 사라졌으면(마지막 pane 닫기로 제거됨) 원래 workspace의 원래 순서 위치에 tab이 다시 만들어지고 그 안에 pane이 생긴다. | D-03, D-10 |
| B9 | 원래 workspace가 사라졌으면(마지막 pane 닫기로 drop됨) 같은 checkout으로 workspace가 먼저 다시 만들어지고 그 안에 tab과 pane이 복원된다. | D-03, D-10 |
| B10 | agent가 없던 터미널 pane을 되살리면 같은 cwd에 새 shell이 열리고 notice는 없다. 스크롤백은 돌아오지 않는다. 원래 cwd가 삭제됐으면 B15와 같이 checkout 루트에서 열리고 notice가 붙는다. | D-14, D-36 |
| B11 | pane 여러 개인 tab을 되살리면 한 번에 tab 전체가 돌아온다: 닫기 직전의 split 토폴로지·방향·비율이 유지되고, 각 pane은 원래 cwd에서 열리며, agent가 있던 pane은 각각 세션을 이어받는다. tab은 원래 workspace의 원래 순서 위치에 놓이고 포커스가 그 tab으로 이동한다. | D-08, D-10, D-18, D-27 |
| B12 | 되살리는 동안 대상 pane 헤더 또는 tab에 기존 split/fork와 같은 대기 표시가 보이고, 완료되거나 실패하면 사라진다. | D-31 |
| B13 | session id가 없거나 resume 명령을 모르는 agent(Claude·Codex 외)의 pane은 같은 cwd에 같은 종류의 새 agent 세션으로 열리고, 그 pane 헤더에 인라인 notice가 "이전 대화는 이어지지 않음"을 알린다. | D-02, D-11, D-29, D-30, D-38 |
| B14 | session id가 있는데 resume 명령이 거부되거나 실패하면 같은 cwd에 새 세션이 열리고 notice에 원래 session id가 표기된다. 항목은 소비된다. | D-24, D-29 |
| B15 | 원래 cwd가 삭제됐으면 checkout 루트에서 열리고 그 pane 헤더 notice가 원래 경로가 없음을 알린다. 항목은 소비된다. | D-11, D-29, D-36 |
| B16 | Herdr가 생성을 거부하거나 timeout이면 아무것도 만들어지지 않고 항목은 스택 맨 위에 남으며, 원래 자리의 이웃 pane 헤더(또는 tab 바)에 인라인 notice가 이유와 재시도 가능함을 알린다. 모달은 뜨지 않는다. 다시 Cmd+Shift+Z를 누르면 같은 항목을 재시도하고, 재시도가 중복 pane을 만들지 않는다. | D-11, D-28, D-29, D-38 |
| B17 | tab 복원에서 tab 생성 자체가 실패하면 B16과 같이 항목이 남는다. tab이 생긴 뒤에는 pane 생성 실패든 resume 실패든 그 pane 자리가 빈 shell 또는 새 세션으로 채워지고 각 pane 헤더 notice가 이유를 알려 pane 개수가 맞으며, 항목은 소비되고, 포커스는 tab의 첫 pane으로 가며, pane 단위 재시도는 제공되지 않는다. | D-22, D-29, D-35 |
| B18 | Browser pane이 섞여 있던 tab을 되살리면 터미널 pane만 복원되고, notice가 브라우저 pane은 복원되지 않았음을 알린다. | D-21 |
| B19 | 파일 탭을 되살리면 같은 경로가 디스크의 현재 내용으로 다시 열리고 활성 탭이 된다. 닫기 당시 스크롤 위치는 복원되지 않는다. 닫기 전 편집 내용은 자동 저장으로 이미 디스크에 있다. | D-14, D-20, D-25 |
| B20 | 되살릴 파일이 이미 열려 있으면 새 탭을 만들지 않고 그 탭에 포커스하며 항목은 소비된다. | D-25 |
| B21 | 되살릴 파일이 삭제됐으면 탭을 열지 않고 tab 바 인라인 notice가 파일이 삭제됐으며 Finder 휴지통에서 복원할 수 있음을 알린다. 항목은 버려진다. | D-25, D-29, D-38 |
| B22 | 되살릴 파일의 경로는 있는데 열기에 실패하면(권한 등) 탭을 열지 않고 tab 바 인라인 notice가 이유를 알리며, 항목은 스택 맨 위에 남아 다시 Cmd+Shift+Z로 재시도할 수 있다. | D-29, D-37, D-38 |
| B23 | 되살리기 관련 알림은 어떤 경우에도 모달 alert로 뜨지 않는다. | D-30, D-38 |
| B24 | 원격 기기 컨텍스트에서 pane/tab을 닫아도 스택에 쌓이지 않고, 원격 컨텍스트에서 Cmd+Shift+Z를 누르면 로컬 스택의 맨 위 항목이 로컬 workspace에 복원된다. | D-13 |
| B25 | 스택 조작과 복원 실행은 렌더 잠금 아래에서 서브프로세스나 블로킹 I/O를 하지 않으며, 되살리는 동안 다른 pane의 입력과 렌더링이 멈추지 않는다. | D-19 |
| B26 | 구현은 worktree에서 이뤄지고 `main` 대상 PR로 배달되며, PR 본문에 격리 Herdr 서버에서의 resume e2e 관찰 결과가 기록된다. | D-15, D-26 |

## Technical structure

- `herdr-core`: `Runtime`에 메모리 전용 "최근 닫은 것" 스택을 추가한다. 항목은 pane·tab·파일 탭 세 종류이며, 확인된 close 이벤트 처리 시점에 Herdr로 close를 보내기 전에 만들어진다. tab 항목은 `layout.export` 결과 트리를 담는다. 스냅샷에 스택 크기와 최상단 항목 라벨이 추가되고, 새 `reopen_closed` 이벤트가 pop과 재생성을 시작한다.
- 재생성은 기존 fork 워커 경로(`live.rs`의 `herdr agent new` 서브프로세스, `fork.rs`의 resume 인자)를 확장해 fork 대신 resume을 만들고, tab은 `layout.apply` 또는 `tab.create` + `agent.new --pane`, workspace 소실 시 기존 `create_herdr_workspace`를 재사용한다. 모든 Herdr 호출은 잠금 밖의 워커에서 실행되고 결과만 잠금 아래에서 반영된다.
- `macos`: `ShellMenuCommand`에 `reopenClosedTab`(⇧⌘Z, Window 메뉴)을 추가하고, 단축키는 포커스가 텍스트 편집 중이면 Redo에 양보하며, 메뉴 클릭은 항상 이벤트를 보낸다. notice는 기존 pane 헤더 인라인 notice를 쓰고, pane이 없는 실패는 이웃 pane 헤더 또는 tab 바에 같은 인라인 형식으로 붙인다. `interactionNotice` alert는 쓰지 않는다.
- Herdr 계약, 스키마, 영구 저장, 인증, 외부 서비스 변경은 없다. `docs/ARCHITECTURE.md`(core 소유 상태), `DESIGN.md`(단축키·메뉴 표), `hide.pen`의 해당 `Screen /` 보드를 같은 PR에서 갱신한다.

## Risks

- `layout.apply`의 command로 시작한 pane이 Herdr agent 목록·lineage에 안 잡힐 수 있다. D-27의 대안(`agent.new --pane`으로 pane마다 채움)을 구현 초기에 확인해 고른다. 소유: 구현자.
- 원래 이웃·tab·workspace가 사라진 뒤 위치를 계산하는 경우의 수가 많다. B7~B9의 fallback 순서를 Rust 단위 테스트로 고정한다.
- Claude/Codex의 resume 명령 인터페이스가 CLI 버전에 따라 바뀔 수 있다. 실패는 B14의 저하 경로로 흡수되고 session id가 notice에 남는다.
- 닫기 확인 다이얼로그를 승인한 뒤 Herdr가 close를 거부하면 스택에 유령 항목이 남을 수 있다. close 실패 시 항목을 제거한다.
- 네이티브 e2e는 격리 Herdr 서버(`HERDR_SOCKET_PATH` 분리)와 하나의 식별된 앱 인스턴스에서만 실행하고, 사용자의 라이브 Herdr pane을 건드리지 않는다. Claude resume e2e는 로그인된 사용자 CLI가 필요하므로 isolated HOME에서는 claude shim을 쓴다.
- 사용자가 구현 전에 준비할 것은 없다.
