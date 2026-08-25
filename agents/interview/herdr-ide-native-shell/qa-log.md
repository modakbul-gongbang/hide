---
topic: "herdr-ide: herdr 소켓 API 위의 네이티브 개발 셸"
status: "complete"
where: "brownfield"
selected_packs: "ux, compatibility, risk, operation, verification, documented-domain"
created_at: "2026-08-25"
updated_at: "2026-08-26"
question_count: 44
normalization_policy: "transcript-sync-with-checkpoint-backfill"
normalization_checkpoint_every: 10
---

# Interview Log: herdr-ide: herdr 소켓 API 위의 네이티브 개발 셸

## Current Understanding

- **목표 한 문장**: herdr 소켓 API 위에 Electron 네이티브 셸을 올려, 여러 에이전트를 굴리는 동안 터미널·산출물·브라우저를 한 화면에서 다루게 함으로써 창 전환과 재입력으로 끊기던 집중을 없앤다.
- **런타임**: Electron + TypeScript, macOS만 지원, 소스 공개 후 각자 빌드. herdr TUI를 실행하지 않고 소켓 API로 셸 전체를 네이티브로 구성한다. orca(MIT)는 참조 대상이며 herdr-core(Rust)는 재사용하지 않고 TS로 재구현한다.
- **화면**: 3열 고정(좌 사이드바 + pet / 중앙 터미널 / 우 패널). 우측 패널은 워크벤치와 브라우저를 전환한다. 터미널 분할은 기본 기능이다.
- **브라우저**: workspace당 하나, pane별 프로필과 소속을 배지로 구분한 탭. loopback CDP를 열어 chromux가 붙고, 패널에서 고른 요소를 포커스된 에이전트 pane에 주입한다. chromux를 대체하지 않는다.
- **파일·편집**: workspace 루트 고정 트리에 명시적 pane 경로 점프, 뷰어와 가벼운 인라인 편집까지. 외부 변경 충돌은 사용자가 무엇을 잃을지 고른다.
- **pet**: herdr-ide가 데스크톱 펫까지 흡수하고 pet-app은 v1 완료 시 은퇴한다. 창이 앞에 있으면 사이드바가, 뒤에 있으면 펫이 알린다.
- **원격**: mini 등 원격 herdr 서버에 SSH ControlMaster로 붙어 터미널과 파일트리까지 지원하고, 인라인 편집과 브라우저 패널은 로컬 전용이다.
- **검증**: chromux가 herdr-ide의 CDP에 붙어 실제 앱을 몰고 herdr 소켓 API로 결과를 확인한다. 에이전트가 검증할 수 있게 만드는 것 자체가 설계 요구사항이다.
- **이 인터뷰는 이전 PRD(herdr-lightweight-ide)를 대체한다.** 그 PRD의 Swift·libghostty·TUI 통째 실행 결정이 모두 무효가 되었고, 차단 요인이던 Xcode 전체 설치도 함께 사라졌다.

## Intake Cursor

- next_decision_id: D-59
- next_question: (owned by the live conversation until checkpoint)
- last_materiality_sweep: checkpoint 7
- outstanding_raw_entries: none
- next_checkpoint_at: Q54

## Transcript Sources

| Runtime | Session ID | Start ref |
| --- | --- | --- |
| claude | 5b30f5a9-191a-4535-8ce5-7a5fdd36f671 | efeb4c11-979a-4cc0-972a-1ec0e1aaa506 |

## Decision Register

| ID | Kind | Area | Decision / fact | Priority | Source / owner | Status | PRD mapping / revisit |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D-01 | fact | herdr 연동 | herdr 0.8.0 소켓 API는 메서드 103개를 노출한다. layout.apply/export/set_split_ratio, pane.split/move/zoom/resize/focus_direction, plugin.list/enable/disable/link, pane.wait_for_output, agent.wait/explain, notification.show 포함. TUI 없이 셸 전체를 네이티브로 재구성 가능하다. | P0 | repo: herdr api schema --json (2026-08-25 실행) | resolved | R#: 네이티브 셸 근거 |
| D-02 | fact | herdr 연동 | pane cwd가 API에 노출된다: focused_pane_cwd, foreground_cwd, workspace_cwd. 파일트리 루트 계산에 별도 추론이 불필요하다. | P1 | repo: herdr api schema --json | resolved | 파일트리 루트 |
| D-03 | fact | 코드 재사용 | herdr-pet의 crates/herdr-core가 UnixStream NDJSON RPC, session.snapshot/events.subscribe, parse_snapshot, _new 토큰 계약, ssh -L 터널을 이미 구현하고 있다. apps/pet-app와 분리되어 있어 공용 크레이트 승격 비용이 낮다. | P0 | repo: ~/projects/herdr-pet/crates/herdr-core/src/{herdr,ssh}.rs | resolved | 워크스페이스 구조 |
| D-04 | fact | 선행 이력 | 이전 PRD agents/prd/herdr-lightweight-ide/prd.md가 approved 상태이나 소스 0줄이다. PW1(Xcode 전체 설치)에서 2026-08-17부터 차단됐다. | P1 | repo: ~/projects/herdr-ide/agents/implement/herdr-lightweight-ide/context-notes.md | resolved | 이전 PRD 대체 근거 |
| D-05 | decision | 아키텍처 | 스택을 Rust/Tauri + herdr-core 재사용으로 확정하고, herdr TUI 통째 실행을 기각한다. 이전 PRD의 D-13(Swift), D-15(libghostty), D-16(TUI 통째 실행)을 무효화한다. 대가로 config.toml의 [keys] prefix 바인딩과 plugin_action은 herdr-ide 안에서 네이티브 키맵으로 재설계해야 한다. | P0 | user invocation: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:efeb4c11-979a-4cc0-972a-1ec0e1aaa506 | rejected | D-10이 대체. Tauri는 herdr-core 611줄을 지키려고 브라우저 패널 상한을 포기하는 거래였고, 성능 논거도 뒤집혔다(D-13) |
| D-06 | decision | 아키텍처 | herdr 플러그인 3개를 네이티브 표면으로 흡수한다. herdr-file-viewer는 워크벤치로, herdr-agent-context-labels는 사이드바로 흡수한다. official.browser는 D-11/D-16/D-18에 따라 워크벤치 내부의 뷰어 종류가 아니라 우측 패널에서 워크벤치와 전환되는 독립 브라우저 패널이 된다(최초 기록에서 갱신됨). | P0 | user invocation: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:efeb4c11-979a-4cc0-972a-1ec0e1aaa506 | resolved | R#: 표면 구성 (D-11/D-16/D-18로 갱신) |
| D-07 | decision | UX/design | herdr-pet을 별도 앱이 아니라 herdr-ide의 한 표면으로 만든다. | P0 | user invocation: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:efeb4c11-979a-4cc0-972a-1ec0e1aaa506 | resolved | R#: pet 표면 |
| D-08 | fact | 참조 제품 | orca(stablyai/orca)는 MIT 라이선스 오픈소스 ADE다. TypeScript/Electron, 53.2k stars, macOS/Windows/Linux + 모바일. 기능: 워크트리별 격리 병렬 에이전트, Ghostty풍 WebGL 터미널 + 무한 split, Monaco 에디터 + 파일 탐색기, 워크트리별 임베디드 Chromium과 Design Mode(UI 요소 클릭 -> HTML/CSS/크롭 스크린샷을 에이전트로 전송), diff 라인에 마크다운 코멘트를 달아 배치로 에이전트에 반환, SSH 워크트리 + 포트포워딩, 20종 이상 CLI 에이전트, MCP/skill 통합, CLI 자동화. | P0 | https://www.onorca.dev/ + gh api repos/stablyai/orca (2026-08-25 조회) | resolved | 참조 기능 후보 |
| D-09 | decision | 제품 정체성 | orca(stablyai/orca)는 참조 대상이자 품질 기준선이며 대체 후보가 아니다. herdr-ide를 계속 만들고 orca의 기능 아이디어와 MIT 코드 패턴을 가져온다. 근거는 herdr 런타임 위에 이미 쌓인 자산(커스텀 키맵, 플러그인 3개, herdr-remote-handoff, pet 상태 모델, 맥미니 Hermes 게이트웨이)이고, orca의 워크트리 모델은 herdr의 workspace/tab/pane 모델과 충돌한다. 다만 사용자는 orca가 herdrm보다 나은 경험이었다고 평가하면서도 무게 때문에 제거했으므로, 경험은 기준선으로 삼되 무게는 반면교사로 삼는다(D-41). | P0 | user Q1 | resolved | R#: 제품 정체성 / 비목표: orca 이관 / D-41 근거 |
| D-10 | decision | 아키텍처 | 스택을 Electron + TypeScript로 확정한다. D-05(Rust/Tauri)를 대체하고, 이전 PRD의 D-13(Swift)/D-15(libghostty)/D-16(TUI 통째 실행)도 무효 상태를 유지한다. 결정 기준은 실제로 구속하는 제약인 브라우저 패널 + orca 코드 재사용이며, herdr-core 재사용은 611줄짜리 약한 제약으로 판정했다. | P0 | user Q6 | resolved | R#: 런타임 / 이전 PRD 대체 |
| D-11 | decision | UX/design | v1 브라우저 패널은 B2로 간다. Electron <webview> 패널 + 127.0.0.1 CDP 엔드포인트 노출(chromux --cdp-url attach 가능)에 더해, 패널에서 요소를 선택하면 HTML/CSS/크롭 스크린샷을 포커스된 에이전트 pane에 주입하는 grab 기능까지 v1에 포함한다. 전달 경로는 herdr의 agent.prompt / pane.send_text를 쓴다. Q2에서 잠정 선택했던 C안은 Tauri 전제였으므로 폐기한다. | P0 | user Q7 | resolved | R#: 브라우저 패널 / v1 최대 태스크 |
| D-12 | fact | 리스크 | Rust로 CDP 임베딩은 현재 실용 경로가 없다. cef-rs 크레이트 자체는 tauri-apps가 관리하며 v151.8.0+151.3.24, 2026-08-23 갱신, 90일 다운로드 100,355, macOS ARM64 지원, docs.rs에서 Settings.remote_debugging_port와 BrowserHost::{send,execute}_dev_tools_message 확인됨. 그러나 Tauri 결합은 tauri-apps/cef-rs#192가 2025-08-26 생성 후 2025-09-08 갱신을 마지막으로 OPEN 상태이며 코멘트 1개다. 검색되는 구현체는 별 0개 개인 포크뿐이다. 이전 PRD RISK1(libghostty 임베딩 비안정)과 같은 종류의 리스크다. | P0 | https://crates.io/api/v1/crates/cef, https://docs.rs/cef/latest/cef/, gh issue tauri-apps/cef-rs#192 (2026-08-25 조회) | resolved | D-10 근거 |
| D-13 | fact | 성능 | 성능 비교의 축은 Electron의 무게가 아니라 Chromium 엔진 개수다. Electron은 앱 UI·터미널·브라우저 패널이 Chromium 1개를 공유하고, Tauri+CEF는 WKWebView + Chromium으로 엔진이 2개가 되어 오히려 무겁다. Tauri가 가벼운 것은 Chromium을 앱에 넣지 않을 때뿐이다. 로컬 실측(2026-08-25): Notion 1,765MB, VS Code 1,087MB, Slack 808MB, 일반 Chrome 904MB/10프로세스. 갱신: 사용자가 orca를 무게 때문에 제거한 사실이 확인되어 메모리는 수용 비용이 아니라 확인된 이탈 요인이다. D-41이 이를 인수 조건으로 고정한다. | P0 | 로컬 ps 실측 + CEF 프로세스 모델 | resolved | D-10 근거 / RISK: 메모리 -> D-41로 게이트화 |
| D-14 | decision | 코드 재사용 | herdr-core(Rust)를 herdr-ide에서 재사용하지 않고 TypeScript로 재구현한다. 소켓 NDJSON RPC는 Node 기본 기능이고 ssh -L은 spawn 한 줄이다. 실제 이식 자산은 코드가 아니라 docs/status-model.md의 _new 토큰 계약과 pose 우선순위이며 이는 순수 로직이라 언어 무관하다. pet 전용 macOS 창 조작 1,131줄(window.rs 669 + macos_window.rs 462)은 D-07(pet을 herdr-ide 내부 표면으로 전환)에 의해 대부분 이미 폐기 예정이었다. | P0 | user Q6에서 수용한 권고 | resolved | R#: herdr 연동 계층 |
| D-15 | decision | 운영 | herdr-pet의 pet-app은 v1 완료 시점에 은퇴한다. D-32(W2)에 따라 herdr-ide가 데스크톱 펫까지 흡수하므로, 이전에 정한 '실사용 통과 후 삭제'보다 앞당겨진다. 은퇴 시점에는 둘 중 하나만 남긴다(원칙 1). 단 D-33의 스파이크가 실패하면 W1으로 후퇴하고 pet-app 은퇴도 함께 연기된다. | P1 | user Q6에서 수용한 권고 | resolved | 운영/은퇴 계획 (D-32로 갱신) |
| D-16 | decision | UX/design | 화면 구조는 L1(3열 고정 + 우측 패널 전환)으로 간다. 좌: 사이드바(spaces/agents/attention + pet), 중앙: 터미널, 우: 패널(워크벤치 <-> 브라우저 전환). L3(orca식 자유 분할)는 매번의 배치 결정이 인지 비용이라 기각. L2(터미널 전체 + 호출형)는 B2(요소를 집어 에이전트에 전달)가 브라우저와 터미널 동시 관찰을 요구하므로 기각. | P0 | user Q8 | resolved | R#: 레이아웃 / UX-01 |
| D-17 | decision | UX/design | 터미널 pane 분할은 v1 기본 기능이다. herdrm과 같은 계열의 앱 단축키(⌘D 세로, ⌘⇧D 가로, 닫기)를 기본 키맵으로 둔다. 구현은 herdr API의 pane.split, pane.close, pane.focus_direction, layout.set_split_ratio를 그대로 쓴다. 사용자의 기존 config.toml [keys](prefix+d 등)는 TUI 전용으로 남기고 herdr-ide는 별도 네이티브 키맵을 갖는다. | P0 | user Q8 | resolved | R#: 터미널 분할 |
| D-18 | decision | UX/design | 브라우저는 pane이 아니라 workspace 단위로 하나다. 그 workspace의 여러 pane/에이전트가 연 페이지는 우측 패널 안의 탭으로 구분되고, 탭 라벨에 어느 pane이 열었는지와 프로필을 표시한다. workspace를 전환하면 브라우저도 따라 바뀐다. 다른 에이전트가 새 페이지를 열어도 보고 있던 탭을 자동 전환하지 않고 미확인 배지만 띄운다(pet의 _new 토큰 계약 재사용). v1에서는 split된 터미널 자리에 브라우저를 넣지 않는다. | P0 | user Q9 | resolved | R#: 브라우저 패널 / UX 카드 |
| D-19 | decision | herdr 연동 | CDP 엔드포인트는 workspace 단위로 하나를 loopback에 연다. herdr-ide가 pane을 만들 때 herdr API의 env 파라미터(PaneSplitParams/TabCreateParams/WorkspaceCreateParams/agent 시작 파라미터에 모두 존재)로 CDP URL을 주입하고, 에이전트는 chromux open --cdp-url <주입값> --tab <match>로 붙는다. 에이전트 쪽 사용법 변경이 없다. | P0 | user Q9 통찰 + herdr api schema 확인 | resolved | R#: 브라우저-에이전트 연결 |
| D-20 | decision | 접근/리스크 | 같은 workspace 안의 에이전트가 서로의 브라우저 탭을 CDP로 제어하는 것을 허용한다. CDP 엔드포인트는 workspace 단위이므로 /json/list에 그 workspace의 탭이 모두 보인다. 근거는 (1) 쿠키·로그인은 프로필 파티션으로 이미 격리되어 세션 탈취가 불가하고, (2) herdr가 pane.send_input으로 이미 같은 신뢰 경계를 긋고 있어 브라우저만 막는 것은 일관성이 없으며, (3) 막으려면 pane별 타겟 필터링이 필요해 D-19에서 해결한 주입 시점 문제가 재발한다. 수용한 위험: 에이전트 오작동 시 다른 에이전트가 작업 중인 페이지(입력 중인 폼, 결제 화면 등)를 건드릴 수 있다. | P1 | user Q20 | resolved | 리스크: 에이전트 간 브라우저 제어 (수용) |
| D-21 | decision | UX/design | 브라우저 프로필은 herdr-ide가 소유한다(Electron session partition). chromux 프로필은 별도 Chrome 프로세스라 우측 패널 한 곳에 섞어 표시할 수 없기 때문이다. workspace마다 default 프로필이 있고, 특정 pane에서 다른 프로필로 페이지를 열면 그 탭만 별도 세션이 된다. 프로필 이름은 chromux와 같은 네임스페이스를 쓴다. 탭에 소속 pane과 프로필을 배지로 표시한다(디자인 원칙 7). | P0 | user Q11, Q12 | resolved | R#: 프로필 모델 |
| D-22 | decision | UX/design | chromux 프로필에서 herdr-ide 파티션으로의 쿠키 임포트는 v1에서 제외한다. herdr-ide 패널 브라우저에서는 처음 한 번 로그인한다. 근거는 orca가 이 문제에 78KB(browser-cookie-import.ts)와 Google source-bound 쿠키 예외 처리를 쓰고도 사이트에 따라 실패하며, 얻는 것이 '처음 한 번 로그인 면제'뿐이기 때문이다(원칙 2). 재검토 트리거: 실사용에서 패널 로그인 재설정이 반복적으로 흐름을 끊을 때. 그때는 orca의 MIT 구현을 참조한다. | P1 | user Q16 | resolved | 비목표(v1) / 재검토 트리거 |
| D-23 | decision | 제품 정체성 | herdr-ide 브라우저 패널은 chromux를 대체하지 않는다. chromux 명령(open/run/cdp/저장 스크립트)과 기존 프로필·로그인은 그대로 유지되며, 별도 Chrome 창 사용도 계속 유효한 경로다. 패널은 브라우저가 뜰 수 있는 자리가 하나 늘어나는 것이고, 결합은 제어 층(--cdp-url)에서 일어난다. | P0 | user Q14, Q15 | resolved | 비목표: chromux 대체 / R#: chromux 연동 |
| D-24 | fact | herdr 연동 | herdr에서 worktree와 workspace는 1:1이다. 'herdr worktree --help'가 'Manage Git worktree-backed workspaces'이고 WorktreeInfo에 open_workspace_id 필드가 있다. worktree를 만들면 그것이 곧 workspace가 되며, workspace 안에 worktree가 중첩되지 않는다. 따라서 worktree 병렬 작업은 workspace가 늘어나는 것으로 표현되고, 각 workspace가 자기 브라우저·파일트리 루트·default 프로필을 갖는다(D-18, D-21과 충돌 없음). | P0 | herdr worktree --help + herdr api schema WorktreeInfo/WorktreeCreateParams (2026-08-25 확인) | resolved | R#: 작업 경계 모델 |
| D-25 | decision | UX/design | 파일트리·워크벤치는 F2 수준으로 만든다. 탐색(펼치기/접기), 보기(이미지 렌더링, 마크다운, 코드 구문 강조, git diff), 그리고 텍스트 파일의 가벼운 인라인 편집과 저장까지 포함한다. F3(새 파일/이름 변경/삭제/드래그 이동/컨텍스트 메뉴)는 v1 비목표이며, 근거는 터미널이 항상 옆에 있어 mv/rm/touch가 이미 최소 경로이기 때문이다(디자인 원칙 3). F1(보기 전용)을 넘어선 이유는 값 하나 고치러 다른 에디터를 여는 것이 원래 해결하려던 문제이기 때문이다. | P0 | user Q21 | resolved | R#: 워크벤치 / 비목표: 파일 조작 |
| D-26 | decision | UX/design | 파일트리 루트는 T3다. 기본은 workspace 체크아웃 루트로 고정하고 pane 포커스를 자동 추종하지 않는다. 포커스된 pane의 foreground_cwd가 트리 루트와 다르면 그 사실을 트리 상단에 표시하고, 명시적 조작으로만 그 경로로 이동한다. T2(자동 추종)는 pane 전환마다 트리가 튀고 펼침 상태가 날아가 집중을 해친다. T1(완전 고정)은 cd로 다른 경로에서 작업하는 pane을 다루지 못한다. 디자인 원칙 4에 따라 '트리와 pane이 다른 곳에 있다'는 파생 상태를 화면이 알려준다. | P0 | user Q22 | resolved | R#: 파일트리 루트 / UX 카드 |
| D-27 | decision | 데이터/상태 | 워크벤치 편집 중 외부 변경이 감지되면 C2로 처리한다. 경고를 띄우고 '내 편집 유지 / 다시 읽기'를 사용자가 고른다. 유지를 고른 뒤 저장하면 에이전트 변경을 덮어쓴다. 어느 쪽이든 한쪽은 잃지만 무엇을 잃을지는 사용자가 정한다. 이전 PRD의 C1(경고 후 무조건 재로드)은 워크벤치를 잠깐 열었다 닫는 모델을 전제했으나 L1에서 워크벤치가 상시 열려 있으므로 전제가 무효다. C1은 디자인 원칙 6(파괴적 동작 전 결과 고지) 위반이기도 하다. C3(3-way 병합)은 값 하나 고치는 용도의 워크벤치에 과하다(원칙 2). 수용한 비용: 선택 다이얼로그가 흐름을 끊는다. | P1 | user Q23 | resolved | R#: 편집 충돌 정책 / 리스크: 편집 유실 |
| D-28 | decision | 아키텍처 | 커스텀은 '내부 단일 출처는 v1에 만들고, 외부 계약은 만들지 않는다'로 선을 긋는다. 키맵 테이블(command id -> 기본 키), 디자인 토큰, 표면 레지스트리(파일 타입 -> 뷰어)를 코드 안 한 곳에 모아 값이 흩어지지 않게 한다. 그러나 이를 사용자 설정 파일로 노출하거나 스키마·기본값·마이그레이션을 약속하지 않는다. 나중에 열 때는 같은 테이블을 파일에서 읽게만 바꾼다. 이 구분이 원칙 2(투기적 확장점 금지)와 원칙 8(버릴 foundation 금지)의 충돌을 해소한다. 예외: 에이전트 정의는 herdr가 server.agent_manifests / server.reload_agent_manifests로 이미 소유하므로 herdr-ide가 별도로 만들지 않고 읽어 쓴다(정의 이중화 금지). | P0 | user Q24 | resolved | R#: 설정 구조 / 비목표: 사용자 설정 계약 |
| D-29 | decision | 운영/배포 | herdr-ide는 남들도 쓰는 것을 전제로 만들되, v1 배포는 R1(소스 공개 + 각자 빌드)이다. 서명·공증(R2)과 Homebrew Cask(R3)는 v1 비목표다. 근거는 herdr-ide를 쓰려면 herdr을 먼저 설치·설정해야 하므로 그 문턱을 넘은 사용자에게 로컬 빌드가 장벽이 아니고, 서명·공증은 나중에 순수 추가 작업이라 되돌릴 것이 없기 때문이다(원칙 3). 이전 PRD의 '로컬 빌드 개인 사용'과 결과는 같지만 전제가 다르다. 이전 PRD는 개인용이었고 지금은 공개 배포를 전제한 첫 단계다. | P0 | user Q25 | resolved | R#: 배포 / 비목표: 서명·공증 |
| D-30 | decision | 운영/배포 | v1 지원 플랫폼은 macOS만이다. Electron으로 오면서 이전 PRD의 macOS 강제(libghostty)는 사라졌으나, 검증 가능한 환경이 이 맥과 mini(둘 다 macOS)뿐이므로 테스트하지 못하는 플랫폼을 지원한다고 약속하지 않는다. Linux는 '막지 않되 지원하지 않는다'로 둔다. 경로·셸 호출을 macOS 전제로 하드코딩하지 않아 나중에 켤 때 검증 작업만 남게 하되, README에 지원으로 적지 않는다. Windows는 명시적 비목표다. ⌘ 키맵(D-17), 경로, 셸이 모두 갈라져 v1 스코프가 배가 된다. 참고 사실: herdr 플러그인들의 platforms는 linux/macos/windows이고 orca는 세 플랫폼을 지원한다. | P0 | user Q26 | resolved | R#: 지원 플랫폼 / 비목표: Windows |
| D-31 | decision | UX/design | pet과 사이드바의 역할을 창 포커스 기준으로 나눈다(E1). herdr-ide 창을 보고 있을 때는 사이드바가 attention을 알리고 pet은 조용히 있거나 분위기만 낸다(roam/sleep). 창이 뒤로 가면 pet이 데스크톱에서 알린다. 근거는 '동시에 살아있는 주목 요구 표면은 최대 1개'라는 기준이며, pet의 docs/status-model.md가 이미 '대시보드는 사이드바와 불일치해서는 안 된다'고 규정한 것과 같은 결이다. E3(둘 다 표시)는 같은 사실을 두 곳에서 말해 신뢰가 갈라지므로 기각(디자인 원칙 7). 두 표면은 herdr-core의 상태 모델(_new 토큰 계약, pose 우선순위) 하나를 공유한다. | P0 | user Q27 | resolved | R#: pet 표면 / D-15 은퇴 조건 |
| D-32 | decision | UX/design | v1에서 herdr-ide가 데스크톱 펫까지 흡수한다(W2). 근거는 pet-app의 macOS 창 조작 1,131줄 대부분이 macOS 난이도가 아니라 Tauri/WKWebView 제약에서 나왔다는 점이다. docs/pet-window-macos.md가 드래그 아키텍처(8ms 네이티브 스레드, 별도 pet-hit 창)의 원인을 'WKWebView throttles timers in unfocused windows'로 직접 기록하고 있다. Electron/Chromium에는 해당 전제가 없고 hasShadow, acceptFirstMouse, setIgnoreMouseEvents(forward), setAlwaysOnTop('screen-saver'), setVisibleOnAllWorkspaces(visibleOnFullScreen), webPreferences.backgroundThrottling, -webkit-app-region:drag가 1급 옵션으로 존재한다. 이식 대상은 오프스크린 클램프 기하(window.rs의 순수 함수 + 실제 사고 좌표가 박힌 테스트)와 팝오버 배치 반전 로직뿐이며, 이는 재작성이 아니라 이식이다. D-14(Rust 미재사용)에 따라 pet-app의 Rust는 어차피 재사용되지 않으므로 W1은 재작성을 미루는 것에 불과하다. | P0 | user Q28, Q29 | resolved | R#: 데스크톱 펫 / D-15 갱신 |
| D-33 | decision | 리스크 | W2(데스크톱 펫 흡수)의 Electron 옵션들은 문서상 확인이며 실행 검증이 없다. 특히 클릭스루 + 첫 클릭 활성화 + 드래그 조합이 미검증이다. 따라서 v1 초반에 게이팅 스파이크를 둔다. 투명·무테·항상 위·전체 Spaces 창을 띄우고, 클릭스루 상태에서 드래그가 끊기지 않으며, 오프스크린 좌표에서 복구되는지를 실물로 확인한다. 이전 PRD의 T1(libghostty 임베딩 스파이크)이 있던 자리를 대체한다. 실패 시 W1(pet-app 유지)으로 후퇴하며, 그 시점에 pet-app이 아직 살아 있으므로 후퇴 비용이 없다. | P0 | user Q29에서 수용한 권고 | resolved | T#: 초반 스파이크 / RISK# |
| D-34 | decision | 검증 | v1 검증의 주 모드는 실제 앱 구동이다. herdr-ide를 --remote-debugging-port로 띄우고 chromux가 그 CDP에 붙어 UI를 조작한 뒤, 결과는 herdr 소켓 API로 확인한다(pane.list로 분할 생성, pane.read로 grab 텍스트 주입 등). UI 배선 유닛 테스트는 쓰지 않는다. 리팩터마다 깨지고 잡는 버그가 없어 원칙 12가 금지하는 형태다. 예외로 순수 함수 2건만 기존 테스트를 이식한다. (1) 오프스크린 클램프 기하 - 실패 모드가 웹 레이어 밖이라 chromux로 재현 불가하고 herdr-pet crates/herdr-core/src/window.rs에 실제 사고 좌표 [542720, 163840]가 박힌 테스트가 이미 있다. (2) _new 토큰 판정 - herdr.rs의 확인된 ? -> idle, 미확인 ? -> attention 테스트. 골든 시나리오(하루 실사용)는 사람 판정 항목으로 기록하되 완료 게이트로 쓰지 않는다. | P0 | user Q30, Q31 | resolved | V#: 검증 모드 |
| D-35 | decision | 아키텍처 | 에이전트가 검증할 수 있는 것은 최대한 에이전트가 검증하도록 만드는 것이 설계 요구사항이다. 검증 전략이 아니라 제품 요구로 취급한다. 구체적으로 (1) herdr-ide는 자기 CDP 엔드포인트를 열 수 있어야 한다, (2) UI가 표현하는 상태는 앱 내부에만 두지 말고 herdr 소켓 API나 관측 가능한 표면으로 확인 가능해야 한다, (3) 사람만 판정할 수 있는 항목은 최소로 남기고 명시한다. 원칙 10(모든 실패는 프로세스 밖에서 관측 가능해야 한다)과 같은 방향이며, 이 요구를 만족하지 못하는 기능 설계는 재검토 대상이다. | P0 | user Q31 | resolved | R#: 관측 가능성 / 설계 제약 |
| D-36 | decision | 운영 | 코드는 ~/projects/herdr-ide를 본진으로 새로 시작한다(S1). herdr-pet은 v1 완료 시 아카이브한다(D-15). 근거는 D-32로 pet-app이 은퇴하고 D-14로 Rust를 쓰지 않으므로 herdr-pet에 남는 것이 문서와 에셋뿐이며, 그 위에 Electron 앱을 얹으면 저장소가 '절반은 죽은 Rust' 상태로 남아 원칙 1에 어긋나기 때문이다. herdr-pet이 private인 점도 D-29(공개 배포)와 맞지 않는다. 필수 이식 대상(코드가 아니라 사고에서 나온 지식): docs/status-model.md, docs/pet-window-macos.md, docs/theme-contract.md, docs/ambient-signals.md, docs/pet-assets.md와 펫 아트 에셋, agents/rules/의 invariants(INV-herdr-unseen-token, INV-pet-state-off-main-thread 등), D-34가 지정한 순수 함수 테스트 2건, config.example.toml의 원격 타겟 형식(D-45), web/hotkey.js의 accelerator 캡처 함정(D-49). 이 이식을 v1 태스크로 명시한다. | P0 | user Q32 | resolved | T#: 문서·테스트 이식 / 운영 |
| D-37 | decision | compatibility | 원격 herdr 서버를 v1에서 지원한다(M2 + 원격 파일트리). SSH ControlMaster 연결 하나를 열어(~/.ssh/config의 별칭·키 인증 그대로 존중) -L 소켓 터널과 sftp를 함께 태운다. 원격에서 되는 것: 사이드바 표시, 터미널 attach·분할, 파일트리 탐색, 뷰어(이미지/마크다운/diff), T3 pane 경로 점프(focused_pane_cwd가 원격 스냅샷에 있음). 원격에서 안 되는 것: 인라인 편집·저장(보기 전용), 브라우저 패널과 grab. 원격 편집을 뺀 이유는 D-27(편집 충돌 C2)을 네트워크 너머에서 다시 풀어야 하고 저장 중 연결 끊김이라는 새 실패 모드가 생기기 때문이다. 원격 수정은 그 pane의 터미널에서 한다(디자인 원칙 3). 브라우저를 뺀 이유는 D-19의 CDP 주입이 원격에서는 역방향 터널을 요구하고 그 실패가 조용하기 때문이다(원칙 10). 안 되는 기능은 회색으로 비워두지 않고 이유를 표시한다(디자인 원칙 7). | P0 | user Q33, Q34 | resolved | R#: 원격 지원 / 비목표: 원격 편집·원격 브라우저 |
| D-38 | decision | UX/design | herdr 서버가 실행 중이 아닐 때 herdr-ide는 자동 기동하지 않는다(N2). 상태를 화면에 알리고 '서버 실행' 버튼을 제공해 사용자의 명시적 행위로만 띄운다. herdrm은 LocalServer.ensureRunning으로 자동 기동하지만(HerdrService.swift), herdr 서버는 모든 에이전트 세션을 들고 있는 상태 저장 프로세스이고 사용자는 mini에서 launchd로 게이트웨이 4개를 운영 중이라 '누가 이 서버를 띄웠나'가 흐려지면 디버깅이 어려워진다. 또한 자동 기동은 서버가 죽는 진짜 원인(크래시, 소켓 꼬임)을 덮어 원칙 4에 어긋난다. herdrm이 구분한 두 실패 상태는 가져온다: 소켓 파일이 아예 없음(서버 미기동)과 파일은 있으나 응답 없음(죽었거나 멈춤). 사용자가 할 일이 다르므로 화면이 구분해 알린다(디자인 원칙 4). | P1 | user Q35, Q38에서 명시 재확인 | resolved | R#: 서버 미실행 처리 / UX 실패 상태 |
| D-39 | fact | 제품 정체성 | 이 프로젝트의 동기는 herdrm 실사용 불만이다. 사용자가 꼽은 실패 이유 세 가지: (1) config.toml의 키맵이 안 먹었다, (2) 파일 브라우저도 웹 브라우저도 없어 결국 다른 앱으로 나가야 했다, (3) 사이드바를 더 꾸미고 싶었고 특히 agent-context-labels(label plugin)를 기본으로 붙이고 싶었다. 이 셋이 herdr-ide의 성공 조건으로 뒤집힌다: 앱 안에서 키맵으로 조작 가능(D-17, D-28), 파일트리와 브라우저 패널 내장(D-25, D-11), 사이드바가 herdr 토큰 설정을 읽고 label plugin 요약을 기본 표시(D-40). 참고: herdrm은 herdr API 103개 중 18개만 쓰고 앱 고정 단축키 5개가 전부였다. | P0 | user Q36, Q37 | resolved | R#: 성공 조건 / 동기 |
| D-40 | decision | UX/design | 사이드바는 herdr가 이미 가진 계약을 읽어 렌더한다. herdr config.toml의 [ui.sidebar.agents] rows(토큰 + fg/bold 스타일)를 읽어 네이티브로 그리고, 값은 session.snapshot의 agents[].tokens에서 가져온다. 실측 확인된 토큰: agent_claude, status_idle, elapsed, sort_rank, summary. 이는 D-28이 금지한 '새 설정 계약 만들기'가 아니라 에이전트 매니페스트와 같은 'herdr 소유 계약 읽기'이므로 정합한다. 따라서 사용자가 이미 작성한 사이드바 설정이 herdr-ide에서 그대로 동작한다. 또한 agent-context-labels의 요약은 tokens.summary로 이미 전달되므로(실측값 '노션 리소스 품질 기준 확정') herdr-ide는 새로 만들 것 없이 렌더링만 하면 된다. D-06의 흡수 비용이 이만큼 낮아진다. | P0 | user Q36 + herdr api snapshot 실측 | resolved | R#: 사이드바 / D-06 비용 갱신 |
| D-41 | decision | 검증 | 메모리 예산을 v1 인수 조건으로 둔다. 측정 시나리오는 합성 최소가 아니라 사용자 실사용 규모다: workspace 7 / pane 11 기준으로 브라우저 패널을 닫은 상태 400MB 이하, 브라우저 패널 하나를 연 상태 900MB 이하. 초과하면 v1 미완료로 판정한다. D-55(활성 workspace의 브라우저만 상주)가 이 예산이 workspace 수와 무관하게 성립하는 근거다. 근거는 사용자가 orca를 '무거워서 뺐다'는 실증이며, D-13의 메모리 비용이 가설이 아니라 확인된 이탈 요인이 되었으므로 관측 가능한 조건으로 고정한다. 측정은 ps 기반이라 자동 판정이 가능하다(D-35). 참고 기준선 실측: Slack 808MB, VS Code 1,087MB, Notion 1,765MB. | P0 | user Q37 | resolved | AC#: 메모리 예산(실사용 규모) / V#: 자동 측정 |
| D-42 | decision | UX/design | herdrm이 못 했던 기본 조작을 herdr-ide 안에서 전부 할 수 있어야 한다. 생성은 workspace, worktree, tab, pane 모두 v1에 포함한다(herdr API의 workspace.create / worktree.create / tab.create / agent.start / pane.split). 닫기도 workspace, tab, pane 모두 포함한다(workspace.close / tab.close / pane.close). worktree.remove는 체크아웃을 실제로 지우고 force 플래그를 가진 진짜 파괴적 동작이므로 v1에 포함하되 결과를 문장으로 고지하고 확인을 받는다(디자인 원칙 6). 근거는 D-39의 실패 이유 (2)이며, herdrm이 herdr API 103개 중 18개만 써서 앱 안에서 조작이 안 됐던 것이 이 프로젝트의 동기다. | P0 | user Q38 | resolved | R#: 앱 내 조작 범위 |
| D-43 | decision | UX/design | grab의 요소 선택 인터랙션 상세는 디자인 패스로 명시 연기한다. v1에 확정된 뼈대는 다음과 같다: 단축키로 grab 모드 진입, 호버 시 대상 요소 아웃라인 표시, 클릭으로 확정, ESC로 취소, 중첩 요소는 부모/자식 이동 키로 선택 범위 조정. 미결로 남기는 것은 아웃라인의 시각 형태, 확정 피드백, 다중 선택 허용 여부다. 재검토 트리거: 디자인 단계 착수 시점. 연기 사유는 이것이 taste 판단이며 뼈대가 정해진 이상 PRD 작성과 구현 착수를 막지 않기 때문이다. | P1 | user Q38 | deferred | 디자인 단계 / 재검토 트리거: 디자인 착수 |
| D-44 | decision | UX/design | 닫기의 파괴적 동작 고지 정책이다. pane.close: 그 pane의 에이전트가 working이거나 attention 상태면 확인 다이얼로그를 띄우고 닫기가 그 프로세스에 무엇을 하는지 문장으로 알린다(디자인 원칙 6). agent_status가 idle이면 확인 없이 닫는다. workspace.close와 tab.close는 D-50이 정한 집계 경고를 따른다. herdr의 pane.close가 PTY와 그 프로세스를 종료하는지 뷰만 떼는지는 구현 착수 시 herdr 동작으로 확정하고, 종료라면 확인 문구에 '세션과 진행 중인 작업이 사라진다'를 명시한다. | P1 | user Q38 | resolved | R#: pane 닫기 / D-50과 한 세트 |
| D-45 | decision | 운영 | 원격 herdr 타겟은 설정 파일로 입력할 수 있고, 동시에 ~/.ssh/config를 읽어 후보로 제시한다. 형식은 herdr-pet의 config.example.toml을 이식한다: [[targets]] 배열에 id / label / socket_path를 두고 [targets.ssh]에 host / remote_socket / options를 둔다. 이미 실사용으로 검증된 형식이므로 새로 설계하지 않는다. 이는 D-28이 금지한 '광범위한 사용자 설정 계약'이 아니라 원격 타겟 목록이라는 좁고 문서화된 예외이며, 손으로 편집해도 동작한다. 사용자가 UI에서 고른 호스트는 같은 파일에 기록한다. | P1 | user Q38 | resolved | R#: 원격 타겟 설정 / D-28 예외 |
| D-46 | decision | compatibility | herdr 버전 호환은 session.snapshot의 protocol 정수로 판정한다. 실측 기준값은 version 0.8.0 / protocol 19다. 문자열 semver보다 정수 비교가 정확하다. 요구 protocol에 미달하면 기능별 우회나 부분 비활성화 없이 명확한 메시지와 함께 실패한다(원칙 4). 근거는 D-29(공개 배포)로 남의 herdr 버전이 다를 수 있고, 조용한 기능 저하는 사용자가 원인을 못 찾는 실패 모드를 만들기 때문이다. | P1 | user Q38 | resolved | R#: 버전 게이팅 / 실패 처리 |
| D-47 | decision | 데이터/상태 | 브라우저 프로필의 세션 파티션 데이터(쿠키, 스토리지)는 workspace나 worktree를 지워도 자동 삭제하지 않고 보존한다. 삭제는 관리 화면에서 사용자가 명시적으로 한다. 근거는 worktree를 자주 만들고 지우는 사용 패턴에서 자동 삭제를 하면 D-22(쿠키 임포트 제외, 처음 한 번만 로그인)의 전제가 무너져 매번 다시 로그인해야 하기 때문이다. 수용한 비용: 쓰지 않는 프로필 데이터가 디스크에 남는다. 관리 화면이 프로필별 마지막 사용 시각과 용량을 보여 수동 정리를 돕는다. | P1 | user Q38 | resolved | R#: 프로필 수명 / 관리 화면 |
| D-48 | decision | 검증 | grab의 크롭 스크린샷은 임시 파일로 쓰고 그 경로를 텍스트로 에이전트 pane에 주입한다. base64 인라인은 수십 KB 텍스트가 프롬프트를 오염시키므로 쓰지 않는다. Claude Code와 Codex 모두 파일 경로를 읽을 수 있다. 검증은 두 단계다: (1) pane.read로 경로 문자열이 주입됐는지 확인, (2) 그 경로에 파일이 실제로 존재하고 이미지로 읽히는지 확인. 이로써 v1 최대 기능(D-11)의 스크린샷 성분에 관측 가능한 증명이 생긴다(D-35). | P0 | user Q38 | resolved | R#: grab 전송 방식 / V#: 스크린샷 증명 |
| D-49 | decision | UX/design | pet과 사이드바는 전역 단축키로 표시/숨김을 토글한다. pet-app이 이미 사용자 재바인딩 가능한 전역 단축키를 제공하며 현재 기본값은 Option+P다. herdr-ide도 같은 조작을 제공하고 사이드바에도 토글 단축키를 둔다. 이식할 함정: accelerator 캡처는 event.key가 아니라 event.code를 써야 한다. Alt를 누른 상태에서 macOS가 P 키를 π로 보고해 Alt+π가 아무것도 파싱되지 않는 사고가 있었다(web/hotkey.js에 기록됨). 이 단축키들은 D-28의 키맵 테이블에 등록되며 v1에서 파일로 노출하지는 않는다. | P1 | user Q38 | resolved | R#: pet/사이드바 토글 / D-36 이식 목록 |
| D-50 | decision | UX/design | workspace.close와 tab.close는 포함된 pane 중 working이거나 attention 상태인 것이 하나라도 있으면 집계 경고 하나를 띄우고 확인을 받는다. 경고는 종료될 에이전트를 목록으로 나열하고 각 항목에 session.snapshot의 tokens.summary(D-40)를 함께 보여 '무엇을 하던 에이전트인지'까지 드러낸다. 포함된 pane이 전부 idle이면 확인 없이 닫는다(D-44와 같은 규칙). pane마다 개별 확인은 클릭이 pane 수만큼 늘어 디자인 원칙 3에 어긋나고, 반복되면 내용을 읽지 않고 누르게 되어 확인 다이얼로그의 존재 이유가 사라지므로 기각한다. 조용히 닫기는 디자인 원칙 6을 정면으로 어기며 workspace 닫기가 pane 닫기보다 더 파괴적인데 확인이 없다는 비일관도 생기므로 기각한다. | P1 | user Q40 | resolved | R#: workspace/tab 닫기 / 파괴적 동작 고지 |
| D-51 | decision | 제품 정체성 | orca의 diff 라인 코멘트 배치 워크플로(diff 줄에 마크다운 코멘트를 달아 모았다가 한 번에 에이전트로 되돌리는 것)는 v1 비목표다. D-25의 F2가 뷰어와 가벼운 편집까지인 것과 일관되며, 코멘트 배치는 리뷰 워크플로 전체(코멘트 저장, 배치 관리, 되돌리기 형식)를 요구해 v1 스코프를 크게 늘린다. Q1에서 Design Mode와 함께 orca의 눈에 띄는 기능으로 꼽혔으나 Design Mode만 D-11로 채택됐다. v2 후보로 기록만 남긴다. | P1 | user Q41 | resolved | 비목표(v1) / v2 후보 |
| D-52 | decision | 운영 | herdr-remote-handoff는 그대로 유지하며 herdr-ide가 대체하지 않는다. D-37의 원격 지원은 '원격 herdr 서버에 붙어서 보고 조작한다'이고, herdr-remote-handoff는 '세션과 작업환경을 통째로 다른 머신으로 옮기고 되받는다'라 다른 일이다. pet-app(D-15)과 herdr-pet(D-36)처럼 은퇴시키지 않는다. 두 도구는 보완재로 공존한다. | P1 | user Q41 | resolved | 운영/생태계 경계 / 비목표: handoff 흡수 |
| D-53 | decision | 외부 의존 | chromux를 herdr-ide의 명시적 전제조건으로 D-29에 함께 적고, chromux 저장소에 라이선스를 추가한다(MIT 권장). 확인된 사실: modakbul-gongbang/chromux는 PUBLIC이고 41 stars이나 licenseInfo가 null이라 라이선스 파일이 없다. 공개 저장소라도 라이선스가 없으면 법적으로 전권 보유라 D-29의 공개 배포 대상자가 사용할 수 없다. chromux가 없는 환경에서는 브라우저 패널은 뜨되 에이전트 연동(D-19의 CDP attach와 D-11의 grab 전달)만 동작하지 않는 상태로 degrade하고 그 사유를 표시한다. 라이선스 추가는 herdr-ide 저장소 밖의 작업이므로 별도 선행 항목으로 기록한다. | P0 | user Q41 + gh repo view modakbul-gongbang/chromux (2026-08-26 확인) | resolved | PW#: chromux 라이선스 / R#: 전제조건·degrade |
| D-54 | decision | UX/design | herdr-ide는 자체 기본 사이드바 레이아웃을 내장하고, 사용자 herdr config.toml에 [ui.sidebar.agents]가 있으면 그것으로 덮어쓴다. D-40이 사용자 설정을 읽는 것만 정해 D-29의 공개 배포 대상자(그 섹션이 없는 사용자)에게 사이드바가 비거나 미정의가 되는 문제를 해소한다. 기본값은 현재 사용자 설정과 같은 모양이다: 첫 줄에 상태 토큰(status_*) + 에이전트 아이콘(agent_*) + workspace 라벨 + elapsed, 둘째 줄에 summary. | P1 | user Q41 | resolved | R#: 사이드바 기본값 / D-40 보완 |
| D-55 | decision | 성능 | 브라우저 WebContents는 활성 workspace의 것만 메모리에 유지한다. workspace를 전환하면 이전 workspace의 브라우저를 파기하고 열려 있던 탭의 URL과 프로필만 기억했다가 복귀 시 재로드한다. 그래서 workspace가 몇 개든 상주 브라우저는 항상 하나이고 메모리 예산이 규모와 무관해진다. 수용한 비용: workspace 복귀 시 페이지 재로드가 발생하고 페이지 내 미저장 상태는 유지되지 않는다. 근거는 사용자 실사용 규모가 workspace 7 / pane 11이라 브라우저를 모두 상주시키면 D-41 예산을 지킬 수 없기 때문이다. | P0 | user Q41 | resolved | R#: 브라우저 수명 / D-41 성립 조건 |
| D-56 | decision | 검증 | 파괴적 동작(workspace.close / tab.close / worktree.remove, D-42·D-44·D-50)의 자동 검증은 일회용 fixture에서만 돌린다. 검증용 workspace와 worktree는 herdr-ide-verify- 접두어로 만들고, 검증 스크립트는 대상 라벨이 그 접두어가 아니면 실행을 거부한다. 근거는 D-34의 검증 방식이 실제 herdr 서버를 상대로 실물 조작을 하기 때문에, 가드가 없으면 검증 실행이 사용자의 진짜 세션과 체크아웃을 지울 수 있기 때문이다. | P1 | user Q41 | resolved | V#: 파괴적 동작 검증 가드 |
| D-57 | decision | UX/design | grab은 받을 대상이 없으면 모드 진입 자체를 막고 사유를 표시한다. 대상 없음의 두 경우: (가) 포커스된 pane이 없음, (나) 포커스된 pane에 에이전트가 실행 중이 아님(예: 맨 셸 프롬프트). (나)에서 그대로 주입하면 HTML/CSS 수백 줄이 셸에 명령어처럼 타이핑된다. 조용한 no-op은 사용자가 요소를 고른 뒤 아무 일도 안 일어난 원인을 찾을 수 없게 하므로 금지한다(원칙 4). 판정은 session.snapshot의 agent_status로 하고, 안내 문구는 에이전트가 실행 중인 pane을 먼저 선택하라고 지시한다. | P1 | user Q42, Q43 | resolved | R#: grab 진입 조건 / UX-03 실패 상태 |
| D-58 | decision | 접근/리스크 | 브라우저 탭의 프로필 선택은 사람의 명시적 UI 행위로만 한다. 브라우저 패널에 프로필 선택기를 두고 사용자가 '새 탭 + 프로필'을 고른다. 에이전트는 D-19로 주입된 그 workspace의 default 프로필만 쓰고 스스로 프로필을 전환하지 않는다. 근거는 에이전트가 임의로 프로필을 갈아탈 수 있으면 '어느 로그인 세션으로 무엇을 했는지' 추적이 불가능해지고, 계정이 여러 개인 상황에서 실제 사고로 이어지기 때문이다. 멀티 계정 작업은 사용자가 해당 프로필 탭을 열어두면 에이전트가 그 탭에 붙어 수행하므로 기능 손실이 없고 프로필 전환 권한만 사람에게 남는다. | P1 | user Q42, Q43 | resolved | R#: 프로필 선택 트리거 / D-21 보완 |

## Raw Q&A

### Q1: orca는 참조인가 대체 후보인가
- decision_ids: D-09
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:8fff31f9-e373-400c-b061-68c079ae3d0a
- asked: orca를 확인했습니다. 먼저 사실부터 (D-08에 기록):
  
  **stablyai/orca** - MIT 오픈소스, TypeScript/Electron, 53.2k stars, macOS/Windows/Linux + 모바일 앱. 카테고리 이름이 "ADE(Agent Development Environment)"입니다.
  
  당신이 방금 말한 세 가지가 이미 다 있습니다:
  
  | 당신이 원한 것 | orca의 구현 |
  | --- | --- |
  | 네이티브 파일트리 | Monaco 에디터 + 파일 탐색기, autosave |
  | 브라우저 옆에 바로 | **워크트리별 임베디드 Chromium** + **Design Mode** |
  | 커스텀 용이 | 20종+ CLI 에이전트 프리셋, MCP/skill 통합, CLI 자동화 |
  
  참조할 만한 게 확실히 있습니다. 특히 두 개가 눈에 띕니다.
  
  **Design Mode** - 브라우저에서 UI 요소를 클릭하면 그 요소의 HTML, CSS, **크롭된 스크린샷**을 에이전트에게 바로 보냅니다. "브라우저를 옆에 띄운다"의 진짜 값은 보는 게 아니라 **본 것을 에이전트에게 전달하는 것**이라는 통찰입니다.
  
  **diff 코멘트 배치** - diff 라인에 마크다운 코멘트를 달고, 모아서 한 번에 에이전트에게 되돌립니다. 리뷰가 대화가 아니라 워크플로가 됩니다.
  
  둘 다 당신 철학("인지적으로 집중")과 정확히 같은 방향입니다. 창을 전환하지 않고, 본 것을 다시 타이핑하지 않는 것.
  
  ---
  
  그런데 이걸 확인하고 나니 인터뷰에서 가장 먼저 풀어야 할 게 바뀌었습니다. orca가 MIT로 이 카테고리를 이미 하고 있어서, 이 답에 따라 PRD 전체가 달라집니다.
  
  **Q1. orca는 당신에게 "참조"입니까, "대체 후보"입니까?**
  
  세 갈래로 보입니다.
  
  1. **참조만** - herdr-ide를 계속 만들고 orca의 기능 아이디어(Design Mode, diff 코멘트 배치)만 가져온다. herdr, pet, agent-context-labels, remote-handoff에 이미 쌓은 게 있고 orca는 herdr을 안 쓰니까
  2. **orca로 갈아탄다** - orca를 쓰고, 부족한 부분은 MIT라서 포크하거나 skill/MCP로 붙인다. herdr-ide는 폐기
  3. **orca 위에 herdr 통합** - orca를 베이스로 herdr 백엔드를 붙인다
  
  제 추천은 **1번**입니다. 근거는 orca가 못 하는 게 아니라 **당신이 이미 herdr에 묶여 있어서**입니다. `~/.config/herdr/`에 커스텀 키맵과 플러그인 3개, `herdr-remote-handoff`로 mini 원격 세션 이관, pet의 `_new` 토큰 상태 모델, 맥미니 4개 Hermes 게이트웨이가 전부 herdr 런타임 위에 있습니다. orca로 가면 이게 전부 버려지고, orca의 워크트리 모델이 herdr의 workspace/tab/pane 모델과 겹치면서 충돌합니다.
  
  다만 이건 제 추론이고, 당신이 herdr에 얼마나 계속 투자할 생각인지는 제가 모릅니다. 어느 쪽인가요?
- recommended: 1번 참조만. 근거는 herdr 런타임 위에 쌓인 자산과 워크트리/workspace 모델 충돌
- answer: 참조지 ㅇㅇㅇ
- immediate_notes:
  - decision: orca는 참조만. herdr-ide를 계속 만든다.
  - reasoning: orca가 기능적으로 부족해서가 아니라, 사용자가 herdr 런타임에 이미 묶여 있어서다. 커스텀 키맵, 플러그인 3개, herdr-remote-handoff, pet 상태 모델, 맥미니 Hermes 게이트웨이가 전부 herdr 위에 있다.
  - constraints: orca의 워크트리 모델과 herdr의 workspace/tab/pane 모델이 겹치면 충돌한다.
  - explicit non-goals: orca로 이관하지 않는다. orca를 포크해 herdr 백엔드를 붙이지 않는다.
  - verified facts: orca는 MIT, TypeScript/Electron, 53.2k stars, 크로스플랫폼 + 모바일 (D-08).
  - unresolved follow-up: orca에서 구체적으로 무엇을 차용할지는 기능 단위로 따로 결정한다.
  - source: user Q1
- needs_normalization: false

### Q2: 브라우저 표면을 어디까지 만들까 (A/B/C)
- decision_ids: D-11, D-05
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:41d6d300-fc86-44a5-b2bc-81c12122576b
- asked: 좋습니다. **참조만**으로 기록합니다 (D-09).
  
  다음으로 넘어가기 전에, preflight에서 하나 걸린 게 있습니다. 브라우저 질문의 답을 바꿉니다.
  
  `chromux --help`에 이게 있습니다:
  
  ```
  chromux open <s> --cdp-url http://127.0.0.1:PORT [--tab active|<id>]
      Attach an externally owned CDP browser view in place.
      Loopback endpoints only; close detaches and never closes its tab.
  ```
  
  즉 herdr-ide가 브라우저 뷰를 띄우면서 loopback에 CDP 엔드포인트를 노출하면, **chromux가 그 뷰에 그대로 붙습니다.** orca의 Design Mode처럼 "요소 → 에이전트" 파이프를 새로 만들 필요 없이, 당신이 보는 브라우저와 에이전트가 조종하는 브라우저가 **같은 브라우저**가 됩니다.
  
  문제는 Tauri가 macOS에서 WKWebView를 쓴다는 겁니다. WKWebView는 CDP를 안 씁니다. 그래서 이 조합이 공짜가 아닙니다.
  
  **Q2. 브라우저 표면을 어디까지 만들까요?**
  
  | | 방식 | 얻는 것 | 비용 |
  | --- | --- | --- | --- |
  | **A** | Tauri WKWebView 임베드 | 옆에 뜬다. 구현 간단 | 에이전트의 chromux 브라우저와 **별개**. 본 걸 에이전트에 전달하려면 파이프를 따로 만들어야 함 |
  | **B** | 임베디드 Chromium(CEF) + CDP 노출 → chromux attach | 보는 브라우저 = 에이전트 브라우저. Design Mode를 거의 공짜로 얻음 | Tauri에 Chromium을 얹는 통합 난이도가 높고 번들이 무거워짐. v1 최대 리스크 |
  | **C** | 임베드 안 함. chromux 세션을 herdr-ide가 **관리·표시** (탭 목록, 스크린샷 미리보기, 클릭하면 해당 Chrome 창을 앞으로) | 기존 chromux 자산 100% 재사용, 가볍고 확실함 | "옆에 나란히"가 아님. 창 전환이 남음 |
  
  제 추천은 **C를 먼저, B를 나중에**입니다. 원칙 3(레이어로 성장)에 맞고, C가 이미 동작하는 chromux 위에 얹는 얇은 층이라 v1을 막지 않습니다. 그런데 당신이 "옆에서 바로 띄워서"라고 한 게 창 전환 제거가 핵심이면 C는 그 요구를 못 채웁니다.
  
  당신이 브라우저를 옆에 띄우고 싶은 **실제 순간**이 어떤 건가요? 에이전트가 만든 화면을 눈으로 확인하는 쪽인가요, 아니면 확인하고 나서 "여기 이거 고쳐"를 전달하는 쪽인가요? 이 답에 따라 A/B/C가 갈립니다.
- recommended: C 먼저(chromux 세션 관리·표시), B 나중(임베디드 Chromium + CDP)
- answer: 어 그렇게 할까... 음 우선 그렇게 해보자
- immediate_notes:
  - decision: 미확정. 답변 '어 그렇게 할까... 음 우선 그렇게 해보자'의 지시 대상이 모호해 Q3에서 확인을 시도했다.
  - reasoning: 추천(C 먼저)과 별개 질문(브라우저를 띄우고 싶은 실제 순간)을 같은 턴에 냈기 때문에 지시 대상이 갈렸다.
  - constraints: 당시 선택지는 전부 Tauri 전제였다.
  - verified facts: chromux는 `open <s> --cdp-url http://127.0.0.1:PORT`로 외부 CDP 뷰에 attach할 수 있다. Tauri의 macOS 백엔드 WKWebView는 CDP를 제공하지 않는다.
  - unresolved follow-up: D-10(Electron 확정)으로 전제가 바뀌어 A/B/C 선택지 자체가 무효화됐다. D-11에서 다시 묻는다.
  - source: user Q2
- needs_normalization: false

### Q3: C안 잠정 채택 확인
- decision_ids: D-11
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:0a72c83e-0977-4449-9d2f-8c505b3bf277
- asked: 확인 하나만 하고 넘어가겠습니다. "그렇게"가 제 추천(C 먼저, B 나중)을 가리키는 것 같은데, P0라 확실히 해두겠습니다.
  
  **C가 구체적으로 이런 모습입니다:**
  
  herdr-ide 우측 패널에 chromux 세션 목록이 뜹니다. 세션마다 탭과 마지막 스크린샷 썸네일이 보이고, 에이전트가 `chromux open`으로 페이지를 열면 그 썸네일이 갱신됩니다. 썸네일을 클릭하면 실제 Chrome 창이 앞으로 나옵니다. 즉 **상태는 herdr-ide 안에서 보고, 실제 조작은 Chrome 창에서** 합니다.
  
  - 얻는 것: "에이전트가 지금 어느 페이지를 보고 있나"를 창 전환 없이 앎
  - 남는 것: 실제로 페이지를 눌러보려면 Chrome 창으로 전환해야 함
  
  **Q3. 이게 "우선 그렇게 해보자"가 맞나요?**
  
  맞다면 B(임베디드 Chromium + CDP)는 폐기가 아니라 **deferred**로 기록하고, 재검토 트리거를 이렇게 걸어두겠습니다: *C를 실사용했는데도 Chrome 창 전환이 하루에 반복적으로 거슬리면 B를 다시 연다.*
  
  이렇게 두면 지금 답을 안 정해도 나중에 판정할 근거가 남습니다. 아까 제가 물었던 "브라우저를 옆에 띄우고 싶은 실제 순간"은 C를 며칠 써보면 저절로 답이 나올 질문이기도 하고요.
  
  맞으면 "응" 한마디면 되고, 아니면 어느 쪽이 틀렸는지만 알려주세요.
- recommended: C 채택 + B는 deferred, 재검토 트리거는 Chrome 창 전환이 반복적으로 거슬릴 때
- answer: https://github.com/stablyai/orca 여기서 browser 패널에 어떻게 띄운디야? 이거 리서치함만 해보고 ㅇㅇㅇ
- immediate_notes:
  - decision: 미응답. 사용자가 orca 브라우저 패널 구현 리서치로 화제를 전환했다.
  - reasoning: 확인 질문에 답하는 대신 판단 근거를 더 요구한 것으로 해석된다.
  - verified facts: 없음.
  - unresolved follow-up: D-11로 승계. Electron 확정 후 다시 묻는다.
  - source: user Q3 (미응답)
- needs_normalization: false

### Q4: orca 브라우저 패널 구현 리서치 요청
- decision_ids: D-08, D-12
- route: research
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:de7e3f6d-efef-40a6-b204-767e1dc9a86b
- asked: 리서치 결과입니다. 답이 꽤 선명하게 나왔습니다.
  
  ## orca가 브라우저 패널을 띄우는 방식
  
  `src/main/browser/`에 60개 넘는 파일이 있고, 구조는 `browser-backend.ts`(1.1KB)가 주석으로 직접 설명합니다:
  
  > browser pages can be backed two ways. A desktop renderer mounts an **Electron `<webview>`** (renderer backend). A headless `orca serve` has no renderer window, so it backs pages with **main-process offscreen WebContents**. Both register the page's WebContents into BrowserManager, so every downstream command resolves a WebContents uniformly.
  
  즉 데스크톱 패널은 **Electron `<webview>` 게스트 뷰**입니다. Electron이 곧 Chromium이라 브라우저를 "임베드"하는 게 아니라 이미 갖고 있는 걸 하나 더 띄우는 것뿐입니다.
  
  ### 핵심은 CDP 프록시입니다
  
  `cdp-ws-proxy.ts`(60KB `cdp-bridge.ts`와 한 세트)가 하는 일:
  
  ```ts
  this.httpServer.listen(0, '127.0.0.1', () => { ... })
  resolve(`ws://127.0.0.1:${this.port}`)
  ```
  그리고 `cdp-target-discovery.ts`가 `/json/version`, `/json/list`에 `webSocketDebuggerUrl`을 응답합니다.
  
  Electron의 `webContents.debugger`를 붙여서(`electron-debugger-lease.ts`) **패널을 진짜 Chrome처럼 보이게 하는 CDP 엔드포인트를 loopback에 엽니다.** 이게 `chromux open <s> --cdp-url http://127.0.0.1:PORT`가 정확히 기대하는 모양입니다. 우연이 아니라 같은 표준이에요.
  
  ### Design Mode의 정체는 "grab"
  
  - `grab-guest-overlay-script.ts` - 호버 오버레이
  - `grab-guest-element-context-script.ts` - 요소 컨텍스트 수집
  - `grab-guest-react-script.ts` - **React 컴포넌트 식별**
  - `browser-grab-payload.ts` / `browser-grab-screenshot.ts` - 에이전트로 보낼 페이로드와 크롭
  - `browser-guest-grab-shortcuts.ts` - 단축키
  
  게스트 뷰에 콘텐츠 스크립트를 주입해서 만듭니다. 별도로 `snapshot-engine.ts` + `snapshot-ax-tree-walk.ts`로 접근성 트리 스냅샷을 에이전트에게 주고요.
  
  ---
  
  ## 그런데 이게 D-05를 흔듭니다
  
  당신이 확정한 스택은 **Rust/Tauri**였고, 근거는 `herdr-core` 재사용이었습니다. 그런데 지금 확인된 사실은:
  
  | | 브라우저 패널 + CDP |
  | --- | --- |
  | **Electron** | Chromium이 이미 런타임. `<webview>` + `webContents.debugger` → chromux 붙는 CDP 엔드포인트가 사실상 공짜 |
  | **Tauri (macOS)** | WKWebView. **CDP 없음.** 같은 걸 하려면 CEF를 따로 얹어야 하고, 그게 아까 B안의 "v1 최대 리스크"였음 |
  
  당신이 원한 "브라우저 옆에 바로"의 가장 좋은 형태(에이전트와 같은 브라우저)가 Electron에서는 거의 공짜, Tauri에서는 최대 난제입니다. 스택 선택이 브라우저 표면의 상한을 정해버립니다.
  
  제가 지금 임의로 뒤집지 않겠습니다. 리서치만 하라고 하셨으니 사실만 올려둡니다. 다만 D-05를 **재검토 대상으로 열어두겠습니다** (인터뷰 규칙상 나중 증거가 노드를 무효화하면 다시 열어야 합니다).
  
  다음 턴에 이걸 정리해서 물어볼 텐데, 미리 알려드리면 선택지는 대략 이 셋입니다:
  
  1. **Tauri 유지 + 브라우저는 C안**(chromux 세션 관리·표시) - herdr-core 재사용을 지키고 브라우저 상한을 낮춤
  2. **Electron으로 전환** - 브라우저 상한을 열고, `herdr-core`는 Rust 사이드카 프로세스로 붙임 (재사용은 되지만 프로세스 경계가 하나 늘어남)
  3. **Tauri + CEF** - 둘 다 가지려다 v1이 늘어짐
  
  지금 답 안 하셔도 됩니다. 계속 리서치할 게 있으면 말씀하시고, 없으면 이어서 질문하겠습니다.
- recommended: none
- answer: 어떤 게 나을 거 같아? 아 이거 좀 고민이네.
- immediate_notes:
  - decision: 리서치 수행. 제품 결정 없음.
  - verified facts: orca 데스크톱 패널은 Electron <webview> 게스트 뷰이고 headless 경로는 main 프로세스 offscreen WebContents다(src/main/browser/browser-backend.ts). cdp-ws-proxy.ts가 webContents.debugger를 붙여 127.0.0.1에 CDP HTTP/WS 엔드포인트를 열고 /json/version, /json/list에 webSocketDebuggerUrl을 응답한다. Design Mode의 실체는 grab-guest-*.ts 콘텐츠 스크립트 묶음이며 React 컴포넌트 식별까지 포함한다.
  - unresolved follow-up: 이 사실이 D-05(Tauri)를 무효화했고 D-10으로 이어졌다.
  - source: user Q4 요청 + 저장소 조사
- needs_normalization: false

### Q5: Electron 전환 권고에 대한 판단 요청
- decision_ids: D-10
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:85a40cd6-e055-4a7e-b08a-73b9e2775b7a
- asked: 숫자를 재보고 나니 고민이 줄어듭니다. 결론부터: **Electron/TypeScript를 추천합니다.**
  
  ## 지키려던 자산이 생각보다 작습니다
  
  `herdr-core`가 2,206줄인데, 그중 herdr 연동은 이만큼입니다:
  
  | 파일 | 줄 | 성격 |
  | --- | --- | --- |
  | `herdr.rs` | 505 | 소켓 RPC + snapshot 파싱 |
  | `ssh.rs` | 106 | `ssh -L` 터널 |
  | `aggregate.rs` / `behavior.rs` / `ambient.rs` | 673 | 상태 집계, pose 우선순위 |
  | `window.rs` | 669 | **macOS 창 조작 (pet 전용)** |
  
  Tauri를 지키는 논거가 보호하는 건 실질적으로 **RPC 611줄**입니다. 그런데 그 611줄이 하는 일은 Unix 소켓에 NDJSON 쓰고 읽기예요. Node는 그걸 기본으로 합니다. `ssh -L`도 `spawn('ssh', ...)` 한 줄이고요. **가장 재구현하기 싼 부분을 지키려고 가장 비싼 부분(브라우저 패널)을 포기하는 거래**입니다.
  
  진짜 자산은 코드가 아니라 `docs/status-model.md`의 `_new` 토큰 계약과 pose 우선순위입니다. 그건 순수 로직이라 어느 언어로든 옮겨집니다.
  
  ## 전환 비용의 대부분이 이미 사라졌습니다
  
  가장 아까운 건 `window.rs`(669) + `macos_window.rs`(462) = **1,131줄의 macOS 창 조작**입니다. Spaces 처리, 오프스크린 가드, `startDragging`, pid로 터미널 창 올리기. `docs/pet-window-macos.md`에 함정을 다 적어둘 만큼 고생한 부분이죠.
  
  그런데 **D-07에서 pet을 herdr-ide의 한 표면으로 만들기로 했습니다.** herdr-ide 창 안에 사는 pet은 Spaces를 알 필요도, 드래그 가능한 무테 패널일 필요도, 오프스크린 가드도 필요 없습니다. 그 1,131줄의 대부분이 **이미 폐기 예정**입니다. 아까워 보이던 게 실은 안 아깝습니다.
  
  ## Electron이 사는 것
  
  1. **브라우저 패널 상한이 열립니다.** `<webview>` + `webContents.debugger` → loopback CDP. B안이 최대 리스크에서 기본값이 됩니다
  2. **orca가 진짜 참조가 됩니다.** MIT + Electron + TS라서 `cdp-ws-proxy.ts`, `grab-guest-*` 패턴을 **읽고 베낄 수 있습니다.** Tauri면 orca는 "다른 런타임의 남의 코드"라 눈으로만 봐야 합니다. 당신이 "참조"라고 한 결정이 스택에 따라 값이 몇 배 달라집니다
  3. **CJK IME 리스크가 대체로 해소됩니다.** 제가 아까 유일하게 남은 게이팅 리스크로 꼽았던 건데, Chromium의 IME 처리는 WKWebView보다 낫고 orca가 실사용으로 증명하고 있습니다
  4. **터미널.** orca의 "Ghostty풍 WebGL 터미널"은 xterm.js WebGL 렌더러입니다. 검증된 경로가 이미 있습니다
  
  ## 솔직한 반대편
  
  - **무겁습니다.** Electron 번들과 메모리. 다만 당신은 chromux로 이미 Chrome을 상시 띄우고 있고, orca가 53k stars로 실용성을 증명합니다
  - **이게 세 번째 스택 번복입니다.** Swift → Tauri → Electron. 원칙 8(장기 관점, 버릴 걸 알면서 깔지 않는다)이 여기 걸립니다. 번복을 멈추는 방법은 **실제로 구속하는 제약**으로 고르는 겁니다. herdr-core 재사용은 611줄짜리 약한 제약이었고, 브라우저 패널 + orca 코드 재사용은 강한 제약입니다. 이 기준으로 고르면 다시 뒤집힐 이유가 없습니다
  - **pet 앱은 당분간 살려둡니다.** 원칙 3에 따라 Electron ide가 pet 표면을 갖추고 실사용을 통과한 뒤에 `pet-app`을 지웁니다. 지금 죽이지 않습니다
  
  ---
  
  **Q4. Electron/TypeScript로 갈까요?**
  
  간다면 D-05를 rejected로 바꾸고 새 결정으로 대체하겠습니다. 워크스페이스는 `herdr-ide` 저장소를 본진으로 하고, `herdr-pet`은 pet-app이 은퇴할 때까지 그대로 둡니다.
  
  망설여지는 지점이 "또 뒤집는 것" 쪽인지, "Electron이 무거운 것" 쪽인지 알려주시면 거기에 맞춰 더 파보겠습니다.
- recommended: Electron/TypeScript. herdr-core는 611줄짜리 약한 제약, 브라우저 패널 + orca 코드 재사용이 강한 제약
- answer: 음 성능이슈는 어떤 것 같아? RUST 유지하면서 어떻게 CDP로 잘할수있는방법은 없을까?
- immediate_notes:
  - decision: 보류. 사용자가 성능 이슈와 Rust 유지 가능성을 먼저 물었다.
  - reasoning: 세 번째 스택 번복이라 근거를 더 요구한 것으로 해석된다.
  - constraints: 사용자가 우려한 축은 '또 뒤집는 것'이 아니라 '성능'과 'Rust 포기'였다.
  - verified facts: herdr-core 2,206줄 중 herdr 연동은 611줄(herdr.rs 505 + ssh.rs 106). pet 전용 macOS 창 조작은 1,131줄.
  - unresolved follow-up: Q6 리서치로 해소.
  - source: user Q5
- needs_normalization: false

### Q6: 성능 이슈와 Rust 유지 시 CDP 경로 리서치
- decision_ids: D-10, D-12, D-13, D-14, D-15
- route: research
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:fbc0eb81-0c46-4513-b80b-94e816a91175
- asked: 성능부터 실측했습니다. 이 맥에서 지금 도는 Electron 앱들입니다.
  
  | 앱 | RSS 합계 |
  | --- | --- |
  | Notion | 1,765 MB |
  | VS Code | 1,087 MB |
  | Slack | 808 MB |
  | Chrome (chromux 아님, 일반 브라우징 10프로세스) | 904 MB |
  
  Electron 앱 3개가 이미 3.6GB를 먹고 있습니다. 무겁다는 건 사실입니다.
  
  ## 그런데 비교 대상이 잘못 잡혀 있었습니다
  
  핵심 질문은 "Electron이 무겁냐"가 아니라 **"Chromium을 몇 개 띄우냐"**입니다.
  
  | 구성 | 엔진 |
  | --- | --- |
  | Electron | Chromium **1개**. 앱 UI + 터미널 + 브라우저 패널이 전부 그 안 |
  | Tauri + CEF | WKWebView(시스템) + Chromium **1개**. 즉 **엔진 2개** |
  | Tauri + 임베드 없음 | WKWebView만. 진짜 가벼움 |
  
  CEF는 Chromium입니다. 브라우저 프로세스, GPU 프로세스, 렌더러, 헬퍼까지 Electron과 같은 프로세스 모델을 씁니다. **Tauri에 CEF를 얹으면 Electron보다 가벼워지는 게 아니라 오히려 엔진이 하나 더 생깁니다.**
  
  Tauri가 메모리에서 이기는 건 **Chromium을 앱 안에 안 넣을 때뿐**입니다.
  
  ## Rust로 CDP를 얻는 경로 3가지
  
  ### 1. cef-rs (tauri-apps 관리)
  
  크레이트 자체는 살아있습니다. `v151.8.0+151.3.24`, **2026-08-23 갱신**(이틀 전), 90일 다운로드 10만, macOS ARM64 지원. docs.rs에서 CDP 표면도 확인했습니다:
  
  ```
  cef::Settings.remote_debugging_port          ← loopback CDP 엔드포인트 = chromux --cdp-url 그대로
  BrowserHost::send_dev_tools_message
  BrowserHost::execute_dev_tools_method
  BrowserHost::add_dev_tools_message_observer  ← 소켓 없이 인프로세스 CDP
  ```
  
  기술적으로는 당신이 원하는 게 정확히 다 있습니다. **문제는 Tauri와의 결합입니다.**
  
  `tauri-apps/cef-rs#192 "Tracking: Integration with Tauri as an alternate web renderer"`가 **2025-08-26 생성, 2025-09-08 마지막 갱신, 코멘트 1개, 아직 OPEN**입니다. 1년째 멈춰 있어요. 검색되는 구현체는 `byeongsu-hong/tauri-runtime-cef`("extracted and ported from tauri feat/cef"), `zizdlp/tauri-cef` 같은 **별 0개짜리 개인 포크**뿐입니다.
  
  CEF는 자체 멀티프로세스 부트스트랩과 헬퍼 번들, 그리고 자기 메시지 루프가 필요합니다. 그걸 tao/winit 이벤트 루프와 공존시키는 게 저 이슈가 1년째 안 닫힌 이유고요. 이전 PRD의 RISK1(libghostty 임베딩 비안정)과 **정확히 같은 종류의 리스크**입니다. 스택만 바꿔서 같은 함정에 다시 들어가는 셈입니다.
  
  ### 2. CDP screencast 브리지 (CEF 없이)
  
  Chromium을 앱에 안 넣고 브라우저 패널을 만드는 방법입니다. 외부 Chrome에 `Page.startScreencast`를 걸어 JPEG 프레임을 받아 캔버스에 그리고, 입력은 `Input.dispatchMouseEvent`로 되돌립니다. chromux에 `chromux cdp <session> <M> '{}'` raw 패스스루가 있어서 배관은 이미 있습니다.
  
  **orca가 실제로 이걸 씁니다.** `browser-screencast-{stream,frame-pacer,viewport-fit,snapshot-capture}.ts` 일습이 있고, `Page.screencastFrameAck`로 백프레셔까지 겁니다:
  
  ```ts
  // CDP only sends the next frame after ACK; delaying ACK for
  // throttled frames applies back-pressure before Chromium/base64 work piles up.
  ```
  
  다만 **orca는 이걸 모바일 릴레이에 씁니다. 데스크톱 패널에는 안 씁니다.** 데스크톱은 `<webview>` 게스트고요. 그 선택 자체가 답입니다. base64 JPEG 스트림은 원격에는 충분해도, 옆에 놓고 개발하는 패널로는 텍스트가 뭉개지고 입력 지연이 남습니다.
  
  ### 3. chromiumoxide / headless_chrome
  
  둘 다 성숙합니다(90일 167만/81만 다운로드). 하지만 **CDP 클라이언트지 임베딩이 아닙니다.** 외부 Chrome을 조종하는 라이브러리고, 그 자리는 chromux가 이미 차지하고 있습니다. 새로 얻는 게 없습니다.
  
  ---
  
  ## 그래서 제 판단
  
  **Rust를 지키면서 CDP를 "잘" 하는 길은 지금 없습니다.** cef-rs는 크레이트로선 준비됐지만 Tauri 결합이 미완이고, 그걸 직접 뚫는 건 libghostty 때와 같은 함정입니다. screencast는 되긴 되지만 orca 자신이 데스크톱에는 안 쓰는 화질입니다.
  
  그리고 성능 논거가 뒤집혔습니다. Tauri+CEF는 Electron보다 **무겁습니다**(엔진 2개). Tauri가 가벼운 건 Chromium을 안 넣을 때뿐인데, 그건 브라우저 패널을 포기한다는 뜻입니다.
  
  결국 질문이 하나로 압축됩니다. **브라우저 패널이 herdr-ide에 얼마나 중요한가.**
  
  - **중요하다** → Electron. 무거운 건 사실이지만, 어차피 Chromium이 필요하고 Electron은 그걸 1개로 끝냅니다. 게다가 orca 코드를 직접 베낄 수 있습니다
  - **있으면 좋은 정도다** → Tauri 유지 + C안(chromux 세션 관리·표시). 가장 가볍고, 이미 있는 chromux를 그대로 씁니다. 대신 창 전환은 남습니다
  
  아까 Q2에서 제가 물었던 게 이거였고, 아직 답이 안 나왔습니다. 다시 여쭙겠습니다.
  
  **브라우저를 옆에 띄우고 싶은 실제 순간이 하루에 몇 번쯤 오나요?** 그리고 그때 하려는 게 "에이전트가 만든 화면을 눈으로 확인"인가요, 아니면 "확인하고 그 자리에서 요소를 집어 에이전트에게 넘기기"인가요?
  
  전자면 Tauri로 충분하고 이 고민이 끝납니다. 후자면 Electron이 답입니다.
- recommended: 브라우저 패널이 중요하면 Electron, 있으면 좋은 정도면 Tauri + C안
- answer: 그냥 일렉트론으로 가자.
- immediate_notes:
  - decision: Electron + TypeScript로 확정.
  - reasoning: Rust로 CDP 임베딩하는 실용 경로가 현재 없고(D-12), 성능 논거가 뒤집혔다. Tauri+CEF는 WKWebView + Chromium으로 엔진이 2개가 되어 Electron보다 무겁다(D-13).
  - constraints: Electron 앱은 실측상 0.8~1.8GB를 쓴다. 메모리는 감수하는 비용이다.
  - explicit non-goals: cef-rs로 Tauri에 Chromium을 얹지 않는다. CDP screencast 브리지를 데스크톱 패널의 기본 경로로 쓰지 않는다(orca 자신도 모바일 릴레이에만 쓴다).
  - verified facts: cef-rs는 크레이트로선 살아있으나 Tauri 결합은 tauri-apps/cef-rs#192가 1년째 OPEN. chromiumoxide/headless_chrome은 CDP 클라이언트지 임베딩이 아니며 그 자리는 chromux가 이미 차지한다.
  - unresolved follow-up: 브라우저 패널 범위(D-11)와 herdr-core 재구현 범위(D-14)를 사용자가 명시 확인해야 한다.
  - source: user Q6
- needs_normalization: false

### Q7: v1 브라우저 패널 범위 (B1 / B2)
- decision_ids: D-11
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:a6287a2a-844a-499c-bd7a-3c1259c01a2b
- asked: 체크포인트 1을 기록했습니다. 등록부 15행, drift 없음.
  
  **확정된 것** (D-10): Electron + TypeScript. D-05(Rust/Tauri)는 rejected로 바꿨습니다.
  
  권고로 받아들인 것으로 함께 기록했으니, 다르면 지금 말씀해 주세요.
  
  - **D-14** - `herdr-core`는 재사용하지 않고 TS로 재구현합니다. 이식하는 자산은 코드가 아니라 `docs/status-model.md`의 `_new` 토큰 계약과 pose 우선순위입니다
  - **D-15** - `pet-app`은 지금 안 죽입니다. Electron herdr-ide가 pet 표면을 갖추고 실사용을 통과한 뒤에 삭제합니다
  
  ---
  
  이제 P0 하나가 열려 있습니다 (D-11).
  
  Q2에서 잠정으로 "C안 먼저"를 골랐는데, 그건 **Tauri 전제였습니다.** Electron에서는 C안의 이유가 사라집니다. 임베디드 브라우저와 CDP가 거의 공짜니까 일부러 낮은 걸 고를 이유가 없어요. 그래서 선택지가 새로 갈립니다.
  
  **Q7. v1의 브라우저 패널을 어디까지 만들까요?**
  
  **B1 - 패널 + CDP 노출**
  herdr-ide 안에 브라우저 패널을 띄우고, orca의 `cdp-ws-proxy.ts`처럼 `127.0.0.1`에 CDP 엔드포인트를 엽니다. chromux가 `--cdp-url`로 붙습니다. **에이전트가 조종하는 브라우저를 내가 그대로 보게 됩니다.** 창 전환이 사라집니다.
  
  **B2 - B1 + grab (Design Mode)**
  여기에 더해 패널에서 요소를 클릭하면 그 요소의 HTML, CSS, 크롭 스크린샷을 **포커스된 에이전트 pane에 바로 넣습니다.** herdr API에 `agent.prompt`와 `pane.send_text`가 이미 있어서 전달 경로도 새로 만들 게 없습니다. orca의 `grab-guest-*.ts`가 MIT라 패턴을 그대로 참조할 수 있고요.
  
  **제 추천은 B2까지 v1입니다.** 이유는 B1만으로는 값이 약해서입니다. "브라우저를 패널에 띄운다"는 Chrome 창을 옆에 놓는 것과 크게 다르지 않습니다. 당신 철학("본 것을 다시 타이핑하지 않는다")이 실제로 실현되는 건 B2 지점이고, CDP가 이미 열려 있으면 B2는 콘텐츠 스크립트 + 페이로드 작업이라 별도 인프라가 안 듭니다.
  
  반대 논거도 있습니다. 원칙 3(레이어로 성장)으로 보면 B1을 먼저 세우고 B2를 얹는 게 맞고, B2에는 요소 선택 UX(오버레이, 호버, 선택 확정)라는 만만찮은 디자인 작업이 딸려옵니다.
  
  B2까지 v1으로 갈까요, 아니면 B1까지만 v1으로 두고 B2를 다음으로 미룰까요?
- recommended: B2까지 v1. B1만으로는 Chrome 창을 옆에 놓는 것과 차이가 적다
- answer: 비투로 가는 게 좋을 것 같아.
- immediate_notes:
  - decision: B2 채택. 패널 + loopback CDP + grab(요소를 집어 에이전트에 전달)까지 v1.
  - reasoning: 사용자 철학('본 것을 다시 타이핑하지 않는다')이 실현되는 지점이 B2이고, CDP가 이미 열려 있으면 B2는 콘텐츠 스크립트와 페이로드 작업이라 별도 인프라가 안 든다.
  - constraints: 요소 선택 UX(오버레이·호버·선택 확정) 디자인 작업이 딸려온다.
  - verified facts: herdr API에 agent.prompt와 pane.send_text가 있어 전달 경로를 새로 만들 필요가 없다. orca의 grab-guest-*.ts가 MIT로 참조 가능하다.
  - source: user Q7
- needs_normalization: false

### Q8: 화면 구조 (L1 / L2 / L3)와 터미널 분할
- decision_ids: D-16, D-17
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:5810d518-49ca-4394-a3c2-e2c715ba9e9d
- asked: B2로 확정했습니다 (D-11). 열린 P0가 없어졌습니다.
  
  ---
  
  이제 표면이 다 모였는데, 세어보니 다섯 개입니다.
  
  사이드바(spaces/agents + attention), 터미널, 파일트리+워크벤치, 브라우저 패널, pet.
  
  당신 철학이 "인지적으로 집중"인데 화면에 다섯 개가 상시로 떠 있으면 그 자체로 철학에 어긋납니다. 그래서 다음 질문이 무엇을 넣느냐가 아니라 **무엇이 상시 보이고 무엇이 불려 나오느냐**입니다. 이게 정해져야 파일트리도 커스텀도 답이 나옵니다.
  
  **Q8. 화면 구조를 어떤 모양으로 갈까요?**
  
  **L1 - 3열 고정 + 우측 패널 전환**
  ```
  ┌────────┬──────────────────┬──────────────┐
  │사이드바│    터미널        │  우측 패널   │
  │spaces  │  (에이전트 pane) │  ⌘E 워크벤치 │
  │agents  │                  │  ⌘B 브라우저 │
  │  pet   │                  │              │
  └────────┴──────────────────┴──────────────┘
  ```
  위치가 고정입니다. 우측 패널만 워크벤치/브라우저로 내용이 바뀝니다. 이전 PRD가 승인받은 3분할을 계승하고, 브라우저가 워크벤치와 같은 자리를 공유합니다.
  
  **L2 - 터미널 전체 + 호출형 패널**
  ```
  ┌────────┬─────────────────────────────────┐
  │사이드바│         터미널 (기본 전체)      │
  │  pet   │                                 │
  │        │   ⌘E/⌘B 누르면 패널이 밀고 들어옴│
  └────────┴─────────────────────────────────┘
  ```
  평소엔 터미널만 봅니다. 필요할 때만 패널이 나타나고, 같은 키로 사라집니다. 집중을 최대로 밀어붙인 형태입니다.
  
  **L3 - orca식 자유 분할**
  사용자가 원하는 대로 무한 split. herdr의 `layout.apply` / `set_split_ratio` API를 그대로 씁니다. 유연하지만 매번 배치를 결정해야 합니다.
  
  **제 추천은 L1입니다.**
  
  L3는 탈락입니다. 자유도가 곧 매번의 결정 비용이라 "집중"과 정반대고, 디자인 원칙 3(가장 잦은 동작이 최소 클릭)에 어긋납니다.
  
  L2와 L1 사이가 진짜 고민인데, L1을 고른 이유는 **브라우저 때문**입니다. B2(요소를 집어 에이전트에 던지기)는 브라우저와 터미널을 **동시에** 봐야 성립합니다. 브라우저가 터미널을 밀어내고 나타나는 L2에서는 "여기 고쳐"를 던진 결과를 그 자리에서 볼 수 없어요. 방금 B2를 v1으로 정했으니 L1이 그 결정과 맞물립니다.
  
  다만 L1은 폭이 좁습니다. 3열이면 터미널이 가운데에서 눌립니다. L2는 터미널이 넓고요.
  
  터미널 폭이 중요한가요, 아니면 브라우저와 터미널을 나란히 보는 게 중요한가요?
- recommended: L1(3열 고정 + 우측 패널 전환)
- answer: 음 근데 pane 여러개가 브라우저 에이전트를 각각 띄울수도 있다보니 저렇게 브라우저가 우측패널에 따로 있ㄲ는게 맞나? 아님녀 둘다 되는건가? 
  L1구조이고 터니널을 쪼개거나 하는거 다해야지.. herdrm에서 cmd + d , cmd +s hift+ d하면 되는 그런것들은 기볹거으로 다 되어야지 ㅇㅇㅇ
- immediate_notes:
  - decision: L1 채택. 터미널 pane 분할은 v1 기본 기능으로 확정.
  - reasoning: L3는 매번의 배치 결정이 인지 비용이라 '집중'과 반대다. L2는 B2가 브라우저와 터미널 동시 관찰을 요구하므로 맞지 않는다.
  - constraints: 사용자가 herdrm의 ⌘D / ⌘⇧D 계열을 기본으로 요구했다.
  - verified facts: herdr API의 pane.split, pane.close, pane.focus_direction, layout.set_split_ratio로 전부 구현 가능하다.
  - unresolved follow-up: 우측 패널이 3열에서 터미널 폭을 압박하는 문제는 실사용에서 판정한다.
  - source: user Q8
- needs_normalization: false

### Q9: 브라우저의 소속 단위 (pane / workspace)
- decision_ids: D-18, D-19
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:c428d502-75ab-4501-87c1-22fab3f9efbb
- asked: 좋은 지적입니다. 그게 L1의 진짜 약점이에요. 먼저 사실 하나 확인하고 답하겠습니다.
  
  지금 당신 herdr 스냅샷입니다: **workspace 7 / tab 9 / pane 11 / agent 9.** 계층은 workspace → tab → pane이고, pane마다 `cwd`, `foreground_cwd`, `agent_status`를 갖고 있습니다.
  
  ## 제 답: 브라우저는 pane이 아니라 workspace에 묶입니다
  
  pane마다 브라우저를 주면 11개가 됩니다. 화면이 터지고, "지금 보는 게 누구 브라우저지"를 매번 계산해야 합니다. 디자인 원칙 4(파생 상태를 사용자가 계산하게 하지 말 것) 위반입니다.
  
  대신 이렇게 두면 깔끔합니다.
  
  - **브라우저는 workspace당 하나.** 그 workspace의 pane 여러 개가 같은 브라우저를 공유합니다
  - **각 에이전트가 연 페이지는 탭이 됩니다.** 탭 라벨에 어느 pane/에이전트가 열었는지 표시합니다
  - **workspace를 바꾸면 우측 패널의 브라우저도 따라 바뀝니다.** 그래서 "누구 브라우저인가"를 볼 필요가 없습니다. 지금 보고 있는 workspace의 것이니까요
  
  orca도 정확히 같은 선택을 했습니다. `createTab({ worktreeId, profileId })` - 브라우저를 워크트리에 묶습니다. pane에 안 묶어요. 그리고 당신 데이터에서 workspace 7개가 이미 프로젝트 단위로 기능하고 있으니 자연스럽게 맞습니다.
  
  **"둘 다 되는가"에 대해서는 v1에서 아니오를 권합니다.** 브라우저의 정위치는 우측 패널이고, split된 터미널 자리에는 안 들어갑니다. 두 브라우저를 동시에 나란히 보는 건(에이전트 둘의 결과 비교) 실재하는 요구지만 v1의 필수는 아니고, 넣으면 레이아웃 상태가 배로 복잡해집니다. 나중에 "이 탭을 여기 고정" 같은 승격으로 열어두면 됩니다.
  
  ## 터미널 split은 기본으로 잡겠습니다
  
  이건 질문이 아니라 확정으로 받았습니다. herdrm의 ⌘D / ⌘⇧D 계열을 기본 키맵으로 두고, herdr API의 `pane.split`, `pane.close`, `pane.focus_direction`, `layout.set_split_ratio`로 구현합니다. 전부 이미 있는 메서드라 새로 만들 게 없습니다.
  
  기존 `config.toml`의 `prefix+d` / `ctrl+alt+d`는 그대로 두셔도 됩니다. 그건 mini에 ssh 붙을 때 쓰는 TUI용이고, herdr-ide는 ⌘ 계열로 갑니다.
  
  ---
  
  **Q9. "브라우저는 workspace당 하나 + 탭으로 구분"이 맞나요?**
  
  여기서 걸릴 만한 지점 하나를 미리 짚으면, **workspace 하나에서 에이전트 둘이 각자 다른 페이지를 보고 있을 때** 우측 패널은 하나뿐이라 탭 전환이 필요합니다. 이때 에이전트 B가 페이지를 열면 당신이 보던 에이전트 A의 탭에서 자동으로 전환될지, 아니면 배지만 뜨고 그대로 있을지가 갈립니다.
  
  제 추천은 **자동 전환 안 함, 배지만**입니다. 보던 걸 뺏기는 게 집중에는 최악이니까요. 다만 그러면 새 페이지가 열린 걸 놓칠 수 있어서 탭에 미확인 표시가 필요합니다. pet의 `_new` 토큰 계약을 여기 그대로 재사용할 수 있습니다.
  
  이 조합으로 갈까요?
- recommended: workspace당 하나 + 탭 구분, 자동 전환 없이 배지만
- answer: 브라우저의 경우엔 하나의 workspace에 탭으로 구분하게 해두고,, 그 탭에서 이제 Chromux의 경우 CDP로 연결하고 tab 베이스로 동작이되니까 이렇게 사용해볼 수 있겠는걸?
- immediate_notes:
  - decision: workspace 단위 브라우저 + 탭 구분 확정. 사용자가 chromux의 tab 기반 동작과 맞물린다는 점을 직접 지적했다.
  - reasoning: pane마다 브라우저를 주면 현재 11개가 되고 '누구 브라우저인가'를 매번 계산해야 한다(디자인 원칙 4 위반).
  - verified facts: 사용자 herdr 스냅샷은 workspace 7 / tab 9 / pane 11 / agent 9다. orca도 createTab({worktreeId, profileId})로 브라우저를 워크트리에 묶는다. herdr API의 env 파라미터로 pane 생성 시 CDP URL 주입이 가능하다.
  - explicit non-goals: v1에서 split된 터미널 자리에 브라우저를 넣지 않는다.
  - source: user Q9
- needs_normalization: false

### Q10: 에이전트 간 브라우저 탭 제어 허용 여부
- decision_ids: D-20
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:576f0e28-33b2-4e8f-86d2-d33283862405
- asked: 맞물립니다. 확인해보니 배관이 이미 다 있습니다.
  
  herdr API 스키마에서 찾은 건데, `PaneSplitParams`, `TabCreateParams`, `WorkspaceCreateParams`, 그리고 agent 시작 파라미터에 전부 이게 있습니다:
  
  ```json
  "env": { "additionalProperties": { "type": "string" }, "type": "object" }
  ```
  
  **herdr-ide가 pane을 만들 때 환경변수를 주입할 수 있습니다.** 그래서 이런 고리가 닫힙니다:
  
  1. herdr-ide가 workspace의 브라우저 패널을 띄우고, orca의 `cdp-ws-proxy` 패턴으로 `127.0.0.1:<port>`에 CDP 엔드포인트를 연다
  2. 그 workspace에서 pane을 만들 때 `env: { CHROMUX_CDP_URL: "http://127.0.0.1:<port>" }`를 주입한다
  3. 에이전트가 `chromux open <session> --cdp-url $CHROMUX_CDP_URL --tab <match>`로 붙는다
  4. **에이전트가 조종하는 탭이 곧 당신이 보고 있는 탭이 된다**
  
  에이전트 쪽에서 새로 배울 게 없습니다. chromux를 평소 쓰던 대로 쓰는데 붙는 대상만 herdr-ide 패널이 되는 거예요. 그리고 chromux가 tab 기준으로 동작한다는 게 여기서 정확히 값을 합니다.
  
  ## 다만 경계를 한 번 정해야 합니다
  
  CDP 엔드포인트를 **workspace 단위로 하나** 열면 `/json/list`에 그 workspace의 탭이 전부 뜹니다. 포트가 workspace마다 고정이라 pane 만들 때 주입하기 쉽고, 탭이 나중에 생겨도 문제없습니다. 반면 orca는 `CdpWsProxy`를 **WebContents(탭)마다 하나씩** 만듭니다. 격리는 확실하지만 탭이 생기기 전에는 포트를 알려줄 수 없어서 주입 시점이 꼬입니다.
  
  그래서 workspace 단위를 권하는데, 대가가 하나 있습니다. **같은 workspace의 에이전트 A가 에이전트 B의 탭을 조종할 수 있습니다.** `/json/list`에 다 보이니까요.
  
  **Q10. 이걸 허용할까요?**
  
  **허용** - 에이전트 A가 B가 열어둔 페이지를 읽거나 조작할 수 있습니다. 같은 프로젝트를 같이 하는 에이전트들이니 자연스럽고, 구현이 단순합니다. 사고가 나면 A가 B의 로그인 세션이나 작업 중인 폼을 건드릴 수 있습니다
  
  **격리** - 에이전트마다 자기 탭만 보이게 CDP 타겟을 필터링합니다. 안전하지만 프록시가 pane별 세션을 관리해야 하고, 주입 시점 문제를 따로 풀어야 합니다
  
  제 추천은 **v1은 허용**입니다. 같은 workspace = 같은 프로젝트 = 같은 신뢰 경계로 보는 게 맞고, herdr 자체가 이미 그 경계를 그렇게 긋고 있습니다(`pane.send_input`으로 아무 pane에나 입력을 넣을 수 있으니 브라우저만 격리해봐야 실익이 적습니다). 다만 이건 접근 경계 결정이라 제가 임의로 정하지 않겠습니다.
  
  어느 쪽인가요?
- recommended: v1은 허용. 같은 workspace = 같은 신뢰 경계
- answer: ㅇㅇㅇ 우선 그렇게 하자
- immediate_notes:
  - decision: 허용. 사용자가 프로필 격리 설계를 제시하며 이의를 제기하지 않았고, 프로필 파티션이 세션 탈취 위험을 낮춘다.
  - reasoning: herdr가 pane.send_input으로 이미 같은 경계를 긋고 있어 브라우저만 격리해봐야 실익이 적다.
  - constraints: 저장소는 파티션으로 격리되지만 제어는 공유된다.
  - source: user Q10, Q11
- needs_normalization: false

### Q11: 프로필 분리 정책 (workspace 공용 / pane별 자동)
- decision_ids: D-21
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:d511fbc1-674d-4940-8712-5c653271328f
- asked: 방향은 맞는데, 중요한 구분이 하나 있습니다. 확인해보니 chromux의 "프로필"과 당신이 원하는 "프로필"이 다른 층입니다.
  
  ```
  chromux ps          List running profiles
  chromux kill <name> Stop profile (Chrome + daemon)
  ```
  
  **chromux의 프로필은 별도 Chrome 프로세스입니다.** 프로필마다 Chrome이 따로 뜹니다. 그래서 pane별로 chromux 프로필을 다르게 하면, 우측 패널 하나에 그 탭들을 **섞어서 보여줄 수 없습니다.** 서로 다른 프로세스니까요.
  
  ## 그래서 herdr-ide가 프로필을 소유해야 합니다
  
  Electron은 한 프로세스 트리 안에서 세션 파티션을 여러 개 가질 수 있습니다(`session.fromPartition('persist:pane-3')`). orca가 정확히 이걸 씁니다:
  
  - `createTab({ url, worktreeId, profileId })`
  - `browser-session-partition-policies.ts`, `browser-session-registry.ts`, `browser-cookie-source-partition.ts`
  
  그래서 역할이 이렇게 갈립니다.
  
  | | 소유 |
  | --- | --- |
  | 프로필(쿠키·세션·스토리지 격리) | **herdr-ide**. Electron 파티션으로 pane별 분리 |
  | 탭 조작 | **chromux**. `--cdp-url`로 붙기만 함 |
  
  즉 우측 패널 하나에 탭이 여럿 있고, **탭마다 소속 pane과 프로필이 다릅니다.** 탭 라벨에 어느 pane이 열었는지 표시하고요. 당신이 말한 그림이 그대로 됩니다.
  
  **그리고 이게 Q10을 자연스럽게 풉니다.** 저장소는 파티션으로 격리되고, 제어(CDP)만 공유됩니다. 에이전트 A가 B의 탭을 조작할 수는 있어도 B의 로그인 쿠키를 자기 탭으로 가져갈 수는 없습니다. 아까 제가 걱정했던 "A가 B의 로그인 세션을 건드린다"가 대부분 사라집니다. 그래서 Q10은 **허용**으로 기록하겠습니다. 다르게 보시면 말씀해 주세요.
  
  ---
  
  **Q11. 그럼 프로필을 언제 나눌까요?** 이게 남은 실질 결정입니다.
  
  **A. workspace 공용이 기본, 필요할 때만 분리**
  같은 workspace의 pane들이 같은 프로필을 씁니다. 한 번 로그인하면 그 workspace의 모든 에이전트가 씁니다. 멀티 계정 테스트처럼 분리가 필요할 때만 새 프로필을 명시적으로 만듭니다.
  
  **B. pane마다 자동 분리**
  pane이 생기면 프로필도 새로 생깁니다. 격리는 완벽하지만 **에이전트마다 로그인을 다시 해야 합니다.**
  
  **제 추천은 A입니다.** 실사용에서 압도적으로 자주 필요한 건 로그인 재사용입니다. 같은 workspace = 같은 프로젝트라 보통 같은 계정을 씁니다. B로 하면 에이전트를 하나 더 띄울 때마다 로그인 벽을 만나서, 집중을 위해 만든 도구가 매번 흐름을 끊습니다.
  
  다만 A에는 대가가 있습니다. 에이전트 하나가 로그아웃하거나 세션을 망가뜨리면 **그 workspace의 다른 에이전트도 같이 당합니다.**
  
  A로 갈까요?
- recommended: A(workspace 공용 기본, 필요할 때만 분리)
- answer: 음 프로필은 herdr-ide 소유인데 chromux base로 동작하게 할 수 있어? 생태계를 결합하고 싶어서! 
  음 그니까 default profile이 있고.. 이거를 특정 pane에서 어떤 다른 프로필로 뭔가를 띄울 수 있잖아? 그러면 해당 브라우저 탭에서 다른 세션? 같은게 열리는거고 그건 다른 프로필이겟지? 그게 여튼 잘 표시되면 좋겟긴 함
- immediate_notes:
  - decision: A 채택. 사용자가 'default profile이 있고 특정 pane에서 다른 프로필로 띄울 수 있다'로 직접 기술했다. 더해 프로필 소유 주체를 herdr-ide로 확정했다.
  - reasoning: 실사용에서 압도적으로 자주 필요한 것은 로그인 재사용이다. pane마다 자동 분리하면 에이전트를 띄울 때마다 로그인 벽을 만난다.
  - constraints: chromux 프로필은 별도 Chrome 프로세스라 우측 패널 한 곳에 섞어 표시할 수 없다. 따라서 Electron session partition을 쓴다.
  - verified facts: chromux ps는 running profiles를 나열하고 chromux kill <name>은 Chrome + daemon을 종료한다(프로필 = 프로세스). orca는 browser-session-partition-policies.ts 등으로 파티션을 모델링한다.
  - unresolved follow-up: 탭 배지의 시각 형태는 디자인 단계에서 정한다.
  - source: user Q11
- needs_normalization: false

### Q12: chromux 생태계 결합 방식과 쿠키 임포트
- decision_ids: D-21, D-22, D-23
- route: mixed
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:62aa0522-bf00-40e4-84b3-145a03278159
- asked: 결합할 수 있습니다. 다만 층을 나눠서 봐야 합니다.
  
  ## 저장소는 직접 공유가 안 되고, 제어는 완전히 됩니다
  
  chromux 프로필은 디스크상의 실제 Chrome user-data-dir이고, herdr-ide는 Electron의 Chromium입니다. 빌드가 달라서 같은 디렉터리를 함께 쓰면 프로필 스키마가 어긋나고 Chrome이 디렉터리를 잠급니다. **직접 공유는 안 됩니다.**
  
  반면 제어는 이미 결합됩니다. `chromux open <s> --cdp-url ...`로 붙는 순간 `run`, `cdp`, 저장한 스크립트까지 herdr-ide 탭에 그대로 먹습니다. 당신이 쌓아둔 chromux 스킬과 스크립트가 전부 살아있습니다.
  
  ## 그런데 저장소도 다리를 놓을 수 있습니다
  
  orca가 이미 해뒀습니다. `chromium-cookie-path.ts`:
  
  ```ts
  // Chromium 96+ moved the cookie DB under Network, while older profiles
  // still use the profile root.
  export function resolveChromiumCookiesPath(profileDir: string): string | null {
    const networkPath = join(profileDir, 'Network', 'Cookies')
    ...
  }
  ```
  
  실제 Chrome 프로필의 쿠키 DB를 읽어서 Electron 파티션으로 임포트합니다. `browser-cookie-import.ts`가 78KB고, `merge` / `replace-imported-domains` 모드에 Google의 source-bound 쿠키(`SIDCC`, `__Secure-1PSIDCC` 등) 예외 처리까지 들어있습니다. 직접 하면 며칠 걸릴 지저분한 문제인데 MIT로 풀려 있습니다.
  
  **그래서 당신이 말한 그림이 이렇게 됩니다:**
  
  - herdr-ide가 프로필을 소유하되, **프로필 이름은 chromux와 같은 네임스페이스**를 씁니다
  - workspace에 default 프로필이 있고, 특정 pane에서 다른 프로필로 열면 그 탭만 다른 세션이 됩니다
  - 그 프로필 이름의 chromux 프로필이 이미 있으면 **쿠키를 임포트해 로그인 상태를 승계**합니다
  - 탭에 프로필 배지를 붙여 어느 pane이 어느 프로필로 열었는지 보입니다 (디자인 원칙 7: 상태는 시각으로 인코딩)
  
  ---
  
  **Q12. 쿠키 임포트 다리를 v1에 넣을까요?**
  
  넣으면 생태계 결합이 실감납니다. chromux `modakbul` 프로필로 로그인해둔 걸 herdr-ide에서 그대로 씁니다.
  
  **대가는 보안입니다.** herdr-ide가 당신의 살아있는 로그인 세션을 들고 있게 되고, 같은 workspace의 에이전트들이 CDP로 그 탭을 조작할 수 있습니다(Q10에서 허용으로 정한 부분). 에이전트가 인증된 세션으로 뭘 할 수 있는지의 경계가 넓어집니다.
  
  **제 추천은 "지원하되 자동은 아님"입니다.** 임포트는 프로필 단위의 **명시적 행위**로만 일어납니다. 새 프로필을 만들 때 "chromux `<name>`에서 쿠키 가져오기"를 고르는 식이고, 백그라운드에서 알아서 동기화하지 않습니다. 그러면 어떤 세션이 herdr-ide 안에 들어와 있는지를 당신이 항상 압니다.
  
  이걸로 갈까요, 아니면 v1은 임포트 없이 독립 프로필로만 가고 나중에 붙일까요?
- recommended: 임포트는 지원하되 자동이 아닌 명시적 행위로만
- answer: 음 너무 복잡해지네... chromux이 chrome을 켜는거 자체가 electron이라 불가능하ㅏ는거지? ㅠㅠ 
  그런데 또 Rust로 하자니 또 복잡해지는거고?
- immediate_notes:
  - decision: 결합은 제어 층에서 하고 저장소 층 임포트는 v1에서 뺀다(Q13에서 확정).
  - reasoning: 사용자가 복잡도를 이유로 되물었고, 복잡도의 단일 출처가 쿠키 브리지였다.
  - verified facts: 두 브라우저가 프로필 디렉터리를 공유할 수 없는 것은 스택 문제가 아니다. CEF로 가도 동일하다. orca는 chromium-cookie-path.ts로 Chrome의 <profileDir>/Network/Cookies를 읽어 Electron으로 임포트하며 browser-cookie-import.ts가 78KB다.
  - source: user Q12
- needs_normalization: false

### Q13: 쿠키 임포트를 v1에서 뺄지 확인
- decision_ids: D-22
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:94f00a68-cb5c-48a3-85a7-7e3ccbe772e8
- asked: 아니요, 그건 오해입니다. 정정할게요.
  
  **chromux가 Chrome을 켜는 건 계속 됩니다.** herdr-ide가 Electron인 것과 무관해요. 별개 프로세스니까요. 안 되는 건 딱 하나, **두 브라우저가 프로필 디렉터리를 같이 쓰는 것**입니다.
  
  그리고 이건 Rust로 가도 안 풀립니다. CEF도 별개 Chromium이라 시스템 Chrome 프로필을 그대로 못 씁니다. 똑같은 벽이에요. 게다가 Tauri+CEF는 결합 이슈가 1년째 열려 있고요(D-12). **스택 문제가 아니라 "브라우저가 둘이면 생기는 문제"입니다.**
  
  ## 복잡도의 출처는 쿠키 브리지 하나입니다
  
  빼면 됩니다. 나머지는 다 단순합니다.
  
  | | 빼면 |
  | --- | --- |
  | 프로필 | herdr-ide 파티션. 독립. **처음 한 번 로그인하면 끝** |
  | chromux 결합 | 제어 층에서 그대로 결합. `--cdp-url`로 붙고 `run`, `cdp`, 저장 스크립트 전부 먹음 |
  | 탭 표시 | 어느 pane, 어느 프로필인지 배지 |
  | 잃는 것 | chromux에 이미 해둔 로그인을 승계 못 함. 처음 한 번 다시 로그인 |
  
  "처음 한 번 다시 로그인"이 전부입니다. 그 대가로 78KB짜리 쿠키 임포트와 보안 경계 고민이 통째로 사라집니다.
  
  원칙 2(현재 요구를 충족하는 가장 단순한 구현)와 원칙 3(레이어로 성장)에 맞습니다. 나중에 진짜 아쉬우면 orca의 MIT 구현이 그대로 있으니 그때 붙이면 되고, 지금 안 정해도 잃는 게 없습니다.
  
  **Q13. 쿠키 임포트를 v1에서 뺄까요?**
  
  빼는 쪽을 추천합니다. "응" 하시면 D-12를 그렇게 기록하고 다음으로 넘어가겠습니다.
- recommended: 뺀다
- answer: 쿠키 임포트 이것 좀 애매한데 좀 더 쉽게 잘 안되는 거야? 아 헷갈리지
- immediate_notes:
  - decision: 미응답. 사용자가 chromux를 쓰는지 마는지를 되물어 개념 확인으로 전환됐다.
  - unresolved follow-up: Q16에서 확정됨.
  - source: user Q13 (미응답)
- needs_normalization: false

### Q14: Electron이면 chromux로 Chrome 켜는 게 불가능한지 확인
- decision_ids: D-23
- route: fact
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:0cbc6c11-e5b9-4b96-a25e-afb6bfb02ae9
- asked: 쉽게 말하면 이겁니다.
  
  **로그인 상태는 브라우저마다 따로 삽니다.** 크롬에서 프로필을 새로 만들면 GitHub도 노션도 다 다시 로그인해야 하죠? herdr-ide의 브라우저 패널은 그 "새 프로필"과 같은 처지입니다. 처음엔 아무 데도 로그인이 안 되어 있어요.
  
  **쿠키 임포트 = 기존 브라우저의 로그인 상태를 새 브라우저로 복사해오기.**
  
  되긴 되는데 잘 안 됩니다. 사이트들이 그런 복사를 막으려고 방어 장치를 두거든요. 특히 구글 계열이 심해서, orca가 이 문제 하나에 78KB를 썼습니다. 그러고도 사이트에 따라 실패합니다.
  
  그래서 선택은 이렇게 단순합니다.
  
  | | 무슨 일이 벌어지나 |
  | --- | --- |
  | **빼기** | herdr-ide에서 처음 GitHub 열면 로그인 화면. 한 번 로그인하면 그다음부터는 계속 로그인 상태 |
  | **넣기** | 기존 로그인을 긁어오려고 시도. 되는 사이트도 있고 안 되는 사이트도 있음. 안 되면 결국 로그인 |
  
  **빼도 "매번 로그인"이 아닙니다. 딱 처음 한 번입니다.**
  
  넣어도 어차피 실패하는 사이트에선 로그인해야 하고요. 그래서 빼는 게 이득입니다.
  
  **Q13. 뺄까요?** ("응"이면 됩니다)
- recommended: none
- answer: ㅊ근데 크롬박스 쓰면 된다는 거 아니야? 네가 별도라며?
- immediate_notes:
  - decision: 오해 정정. chromux의 Chrome 실행은 herdr-ide가 Electron인 것과 무관하게 계속 동작한다. 불가능한 것은 두 브라우저가 프로필 디렉터리를 공유하는 것뿐이며 이는 Rust/CEF로 가도 같다.
  - verified facts: 별개 프로세스이므로 간섭하지 않는다. D-12가 Tauri+CEF 경로의 미성숙을 이미 확인했다.
  - source: user Q14
- needs_normalization: false

### Q15: chromux를 쓰는 것인지 마는 것인지 확인
- decision_ids: D-23
- route: fact
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:9e9ef1e2-7d9f-47a6-8bf4-eb14b6908696
- asked: 맞습니다. 그리고 그게 정확히 핵심이에요.
  
  **chromux는 그대로 돌아갑니다.** 지금 로그인해둔 프로필들 안 없어지고, 하던 작업 다 그대로입니다.
  
  브라우저가 뜰 수 있는 자리가 두 군데가 되는 겁니다.
  
  | | 로그인 | 창 전환 |
  | --- | --- | --- |
  | **chromux의 Chrome 창** (지금 쓰는 것) | 이미 되어 있음 | 필요함 |
  | **herdr-ide 우측 패널** (새로 생기는 것) | 처음 한 번 필요 | 없음 |
  
  패널이 chromux를 대체하는 게 아니라 **자리가 하나 늘어나는 것**입니다. 기존 로그인이 필요한 작업은 그냥 chromux로 계속 하시면 되고, 패널은 "터미널 옆에서 바로 보고 요소를 집어 던지고 싶을 때" 쓰는 자리예요.
  
  그래서 쿠키 임포트를 빼도 **잃는 게 없습니다.** 지금 있는 건 하나도 안 망가지고, 새로 생기는 자리에서만 처음 한 번 로그인하는 거죠.
  
  **Q13. 그럼 뺄까요?**
- recommended: none
- answer: 음 이해가 잘안가노... 그래서 chromux 쓴다는거야만다는거야
- immediate_notes:
  - decision: 확인. chromux는 계속 쓴다. 명령과 저장 스크립트가 그대로 유지되고, 바뀌는 것은 조종 대상 화면이 뜨는 위치가 하나 늘어난다는 점뿐이다.
  - source: user Q15
- needs_normalization: false

### Q16: 쿠키 임포트 v1 제외 최종 확인
- decision_ids: D-22
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:d13ca3e6-b638-4f94-a81a-98367dc89a4f
- asked: **씁니다.** 계속 씁니다.
  
  `chromux open`, `chromux run`, 저장해둔 스크립트 전부 그대로 씁니다. 안 버립니다.
  
  바뀌는 건 하나뿐입니다. **chromux가 조종하는 화면이 어디에 뜨느냐.**
  
  - 지금: 별도 Chrome 창에 뜸
  - 앞으로: 별도 Chrome 창에도 뜰 수 있고, herdr-ide 패널에도 뜰 수 있음
  
  명령어는 똑같고, 붙일 곳이 하나 늘어나는 겁니다.
  
  ---
  
  쿠키 임포트는 그중 **herdr-ide 패널에 뜰 때만** 걸리는 문제입니다. 그 패널은 새 브라우저라서 로그인이 안 되어 있거든요.
  
  - **빼기** = 패널에서 처음 한 번 로그인한다
  - **넣기** = 기존 로그인을 복사해보는데, 잘 안 되고 코드가 복잡해진다
  
  **Q13. 빼는 걸 추천합니다. 응?**
- recommended: 뺀다
- answer: 알겟어..
- immediate_notes:
  - decision: v1에서 뺀다. 사용자 승인('알겟어').
  - reasoning: 얻는 것이 '처음 한 번 로그인 면제'뿐이고, 넣어도 방어가 걸린 사이트에서는 결국 로그인해야 한다.
  - constraints: 재검토 트리거는 패널 로그인 재설정이 반복적으로 흐름을 끊을 때다.
  - source: user Q16
- needs_normalization: false

### Q17: 체크포인트 2 후 계속/중단 선택
- decision_ids: none
- route: mixed
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:7fdc7b8b-2da9-4758-870f-c95be95ffe8f
- asked: 체크포인트 2 기록했습니다. 등록부 23행, 열린 P0/P1 없음, drift 없음.
  
  ## 지금까지 정해진 것
  
  - **런타임** Electron + TypeScript. orca는 참조만
  - **레이아웃** 3열 고정. 좌 사이드바(+pet) / 중앙 터미널 / 우 패널
  - **터미널** ⌘D, ⌘⇧D 분할 기본
  - **우측 패널** 워크벤치 ↔ 브라우저 전환
  - **브라우저** workspace당 하나, 탭으로 구분. 탭에 어느 pane·어느 프로필인지 배지. 요소를 집어 에이전트에 던지는 것(B2)까지 v1
  - **chromux** 그대로 씀. 제어는 `--cdp-url`로 결합. 쿠키 임포트는 v1 제외
  - **플러그인 3개** 네이티브로 흡수. pet은 사이드바 표면으로
  
  ## 남은 것
  
  당신이 처음 말한 세 가지 중 **파일트리**와 **커스텀**이 아직입니다. 그 외에 워크벤치 편집 범위, 워크트리 활용, 검증 방식, 저장소 위치가 남았습니다. 대략 6~8문항쯤 더 봅니다.
  
  ---
  
  브라우저 얘기가 길었고 헷갈리는 구간이었습니다. 여기서 한 번 끊는 것도 괜찮습니다.
  
  **계속할까요, 아니면 여기서 저장하고 나중에 이어갈까요?**
  
  이어가면 `sasu interview sync`로 정확히 이 지점부터 다시 시작합니다. 계속하시면 다음은 **파일트리**입니다. 당신이 "최대한 네이티브하게"라고 한 게 무슨 뜻인지부터 좁히겠습니다.
- recommended: 여기서 끊고 재개
- answer: 한번 끊고 그 다음에 이제 이어서 다시 해보자
- immediate_notes:
  - decision: 인터뷰를 여기서 일시 중단하기로 했다. 제품 결정은 없다.
  - reasoning: 브라우저 구간이 길고 혼란스러웠던 직후라 사용자가 휴지를 택했다.
  - source: user Q17 (PRD 영향 없음 - 진행 관리 턴이라 decision_ids none)
- needs_normalization: false

### Q18: paused 저장 후 기록 상태 확인 요청
- decision_ids: D-20, D-22
- route: fact
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:bb2fe917-295f-42ef-b2a3-7ed21c63bc98
- asked: 인터뷰를 `paused`로 저장했습니다.
  
  **`~/projects/herdr-ide/agents/interview/herdr-ide-native-shell/qa-log.md`**
  
  Q16까지 전부 정규화됐고, 등록부 23행에 열린 P0/P1이 없습니다. 미완인 채로 끊긴 결정이 하나도 없다는 뜻이라 이어가기 좋은 지점입니다.
  
  ## 이어갈 때
  
  새 세션에서 **`/interview-me`** 를 부르고 herdr-ide 이어간다고 말씀하시면 됩니다. `sasu interview sync`가 새 세션을 바인딩하면서 Q17부터 붙습니다. 지금까지의 결정 23건은 그대로 살아있습니다.
  
  다음 순서는 이렇게 잡아뒀습니다.
  
  1. **파일트리** - "최대한 네이티브하게"의 정의부터
  2. **커스텀 용이성** - 무엇을 커스텀하는가 (키맵? 레이아웃? 표면 추가?)
  3. 워크벤치 편집 범위 (뷰어까지인가, 편집까지인가)
  4. 워크트리 활용 (herdr에 `worktree.*` API가 있고 orca의 핵심 기능이기도 합니다)
  5. pet 표면 상세
  6. 검증 방식과 저장소 위치
  
  ## 하나 남겨둘 것
  
  `~/projects/herdr-ide`의 이전 PRD `herdr-lightweight-ide`가 아직 `approved` 상태로 남아 있습니다. 이번 인터뷰가 그 PRD의 D-13(Swift), D-15(libghostty), D-16(TUI 통째 실행)을 전부 뒤집었으니 실질적으로는 폐기된 문서입니다. 새 PRD를 쓸 때 그 상태를 명시적으로 정리하는 게 좋겠습니다. 지금 건드리진 않았습니다.
  
  수고하셨습니다. 브라우저 구간이 특히 얽혀 있었는데, 결과적으로 v1 최대 태스크(B2)와 그 대가를 다 드러내고 정리했습니다.
- recommended: none
- answer: 기록이 어떻게 됐어? 계속헤메는 거 그런거 다 된거지 ?
- immediate_notes:
  - decision: 기록 감사 수행. 두 건을 수정했다. D-20을 무응답 기반 resolved에서 open으로 되돌렸고(침묵은 동의가 아님), D-22의 출처를 Q13(미응답)에서 Q16(실제 승인)으로 바로잡았다.
  - verified facts: Q13~Q16의 혼란 구간이 원문 그대로 남아 있고, Q13은 미응답, Q14/Q15는 route: fact로 분류되어 결정으로 오인되지 않는다.
  - source: user Q18
- needs_normalization: false

### Q19: 인터뷰 재개 지시
- decision_ids: none
- route: mixed
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:f495206d-5caa-44fe-a4eb-200970b8a355
- asked: 네, 다 들어갔습니다. 헤맨 구간도 **그대로** 남아 있습니다.
  
  ## 헤맨 턴이 어떻게 기록됐나
  
  Q13~Q16이 그 구간인데, 답변이 원문 그대로 박혀 있습니다.
  
  ```
  ### Q13: 쿠키 임포트를 v1에서 뺄지 확인
  - answer: 쿠키 임포트 이것 좀 애매한데 좀 더 쉽게 잘 안되는 거야? 아 헷갈리지
  - immediate_notes: decision: 미응답. 사용자가 chromux를 쓰는지 마는지를 되물어 개념 확인으로 전환됐다.
  
  ### Q15: chromux를 쓰는 것인지 마는 것인지 확인
  - answer: 음 이해가 잘안가노... 그래서 chromux 쓴다는거야만다는거야
  - route: fact
  - immediate_notes: decision: 확인. chromux는 계속 쓴다. 바뀌는 것은 조종 대상 화면이 뜨는 위치가 하나 늘어난다는 점뿐이다.
  
  ### Q16: 쿠키 임포트 v1 제외 최종 확인
  - answer: 알겟어..
  - immediate_notes: decision: v1에서 뺀다. 사용자 승인.
  ```
  
  헤맨 걸 지우거나 미화하지 않았습니다. **Q13은 "미응답"으로, Q14·Q15는 결정이 아니라 "사실 확인"(`route: fact`)으로 분류**해서 결정으로 오인되지 않게 해뒀습니다. 실제 승인은 Q16에서 났고요.
  
  그리고 각 답변에 `source_ref`로 세션 메시지 ID가 붙어 있어서, 나중에 "이 결정이 어디서 나왔지" 하면 원문 턴을 바로 찾을 수 있습니다.
  
  ## 확인하다 잘못 처리한 걸 하나 찾아서 고쳤습니다
  
  **D-20**(같은 workspace의 에이전트끼리 서로 브라우저 탭을 조작해도 되는가)을 제가 `resolved`로 기록해뒀었습니다. 근거를 "이의 없음"으로 써놨더군요.
  
  그건 잘못입니다. Q10에서 제가 "허용을 추천한다"고 했을 때 당신은 답하지 않고 프로필 얘기로 넘어갔습니다. **침묵이나 화제 전환은 동의가 아닙니다.** 접근 권한 결정을 제가 임의로 닫은 셈이라 `open`으로 되돌렸습니다.
  
  재개할 때 첫 질문으로 이걸 여쭙겠습니다. 지금 등록부 상태:
  
  ```
  register: 23 rows | open P0/P1: D-20 (P1 접근/리스크, open)
  ```
  
  D-22의 출처도 Q13(미응답)에서 Q16(실제 승인)으로 바로잡았습니다.
  
  ---
  
  정리하면 **결정 23건 중 22건이 확정, 1건(D-20)이 열린 채 대기**입니다. 열린 게 하나 있는 상태로 끊는 게 오히려 정확한 기록이고, 재개 지점도 명확합니다.
- recommended: none
- answer: 오케이 그럼 이어서 진행 ㄱㄱ
- immediate_notes:
  - decision: 재개. status를 paused에서 active로 되돌리고 sync로 재바인딩했다. 제품 결정 없음.
  - source: user Q19 (진행 관리 턴)
- needs_normalization: false

### Q20: 에이전트 간 브라우저 탭 제어 허용 여부 (재확인)
- decision_ids: D-20
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:c9d161fe-fefb-4e5a-8c7f-1d994a4a646c
- asked: 재개합니다. 열린 D-20부터 닫겠습니다.
  
  **Q20. 같은 workspace 안의 에이전트가 서로의 브라우저 탭을 조작해도 될까요?**
  
  상황을 구체적으로 그리면 이렇습니다. workspace 하나에 에이전트 A와 B가 돌고 있고, A는 로컬 앱 화면을 띄워놨고 B는 문서를 보고 있습니다. 우측 패널에 탭 두 개가 있는 상태죠.
  
  **허용하면** - B가 A의 탭으로 가서 페이지를 읽거나 버튼을 누를 수 있습니다. `/json/list`에 그 workspace의 탭이 다 보이니까요. "다른 에이전트가 띄워둔 화면 확인해봐" 같은 게 됩니다.
  
  **막으면** - B는 자기가 연 탭만 보입니다. A의 탭은 존재조차 모릅니다.
  
  **제 추천은 허용입니다.** 세 가지 이유입니다.
  
  1. 쿠키·로그인은 어차피 프로필 파티션으로 격리됩니다(D-21). B가 A의 탭을 조작해도 A의 세션을 자기 쪽으로 가져갈 수는 없습니다. 남는 건 "조작 가능" 하나뿐입니다
  2. herdr가 이미 그 경계를 그렇게 긋고 있습니다. `pane.send_input`으로 아무 pane에나 입력을 넣을 수 있어서, 브라우저만 막아봐야 실익이 적습니다
  3. 막으려면 프록시가 pane별로 CDP 타겟을 필터링해야 하는데, 그러면 탭이 생기기 전에 포트를 알려줄 수 없는 문제가 다시 생깁니다(D-19에서 workspace 단위로 푼 그 문제)
  
  **반대 논거도 있습니다.** 에이전트가 오작동하면 당신이 보고 있던 다른 에이전트의 화면을 건드립니다. 폼에 뭘 쓰고 있었거나 결제 화면이었으면 곤란하죠. 다만 그건 `pane.send_input`으로도 똑같이 가능한 일이라 브라우저에서만 막는 게 일관성이 없긴 합니다.
  
  허용으로 갈까요? 아니면 막는 쪽이 마음이 편하신가요?
- recommended: 허용. 프로필 파티션으로 세션은 이미 격리되고 herdr가 pane.send_input으로 같은 경계를 긋는다
- answer: 허용
- immediate_notes:
  - decision: 허용. 명시적 사용자 승인으로 D-20을 닫았다.
  - reasoning: (1) 쿠키·로그인은 프로필 파티션으로 격리되어 세션 탈취가 불가하고, (2) herdr가 pane.send_input으로 이미 같은 신뢰 경계를 긋고 있으며, (3) 막으려면 pane별 타겟 필터링이 필요해 D-19가 해결한 주입 시점 문제가 재발한다.
  - constraints: 에이전트 오작동 시 다른 에이전트가 작업 중인 페이지를 건드릴 수 있다. 이 위험을 수용했다.
  - source: user Q20
- needs_normalization: false

### Q21: 파일트리·워크벤치 수준 (F1/F2/F3)
- decision_ids: D-25, D-24
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:887c06ed-7fc4-437c-9195-ea5ae560f464
- asked: D-20 닫았습니다. 열린 P0/P1이 없습니다.
  
  ---
  
  이제 **파일트리**입니다. 당신이 처음 말한 "최대한 네이티브하게"가 무슨 뜻인지부터 좁히겠습니다.
  
  맥락을 하나 짚으면, 이전 PRD에서 이 기능을 만들려던 원래 이유가 이거였습니다.
  
  > `herdr-file-viewer` 플러그인이 파일트리를 제공하지만 **read-only이고 이미지 렌더링이 없다.** VS Code를 경로 지정해 새로 여는 우회는 매번 창을 전환해야 해서 흐름이 끊긴다.
  
  **Q21. 파일트리를 어느 수준으로 만들까요?**
  
  **F1 - 탐색 + 보기**
  트리를 펼치고 접고, 파일을 클릭하면 우측에 내용이 뜹니다. 이미지, 마크다운 렌더링, 코드 구문 강조, git diff. **보는 것까지만** 합니다. 파일을 만들거나 지우거나 이름을 바꾸는 건 터미널에서 합니다.
  
  **F2 - F1 + 가벼운 편집**
  텍스트 파일을 그 자리에서 고치고 저장합니다. 에이전트가 같은 파일을 밖에서 바꾸면 경고하고 다시 읽습니다.
  
  **F3 - F2 + 파일 조작**
  새 파일, 이름 바꾸기, 삭제, 드래그로 이동, 컨텍스트 메뉴. Finder나 VS Code 탐색기에 가까운 조작감.
  
  **제 추천은 F2입니다.**
  
  F3를 뺀 이유는 **당신이 터미널을 항상 옆에 두고 있어서**입니다. `mv`, `rm`, `touch`가 트리에서 우클릭보다 빠릅니다. 디자인 원칙 3(가장 잦은 동작이 최소 클릭)을 적용하면 파일 조작은 이미 최소 경로가 존재하는 셈이라, 트리에 넣는 건 중복입니다. 게다가 드래그 이동은 실수로 디렉터리를 옮기는 사고가 잦습니다.
  
  F1이 아닌 이유는 **오타 하나 고치러 창을 옮기는 게 아까워서**입니다. 에이전트가 만든 파일에서 값 하나 바꾸는 일이 실제로 잦고, 그때마다 vim을 열거나 VS Code로 가면 원래 문제로 돌아갑니다.
  
  한 가지 미리 말씀드리면, F2에는 **동시 편집 충돌**이 딸려옵니다. 당신이 고치는 중에 에이전트가 같은 파일을 덮어쓸 수 있습니다. 이전 PRD는 "경고하고 다시 읽기만, 병합은 안 함"으로 정했고 그러면 **당신의 미저장 변경이 날아갑니다.** 그 정책을 그대로 가져갈지는 F2로 정해지면 따로 여쭙겠습니다.
  
  F2로 갈까요?
- recommended: F2(탐색 + 보기 + 가벼운 편집)
- answer: F2 괜찮은 것 같음.. 근데 이거 뭐라고 해야하지.. 음 workspace -> 메인, worktree 개념이 있는것 같고, 그 작업경계가 애매한게 있구만? 예를 들어 하나의 워크스페이스에서 작업하는데 터미널 페인을 여러 개 띄었다고 하자. 근데 그 페인 중에 다른 애는 뭔가 다른 경로로 이동해서 작업을 시켰어. 그리고 어떤 애는 워크트립 해가지고 작업을 했어. 하나의 이 워크스페이스에 워크트립을 여러 개 세션별로 할 수도 있잖아. 그러면 어떻게 되는 걸까?
- immediate_notes:
  - decision: F2 채택. 더불어 사용자가 workspace/worktree/pane 경계의 모호함을 제기했다.
  - reasoning: F3는 터미널이 옆에 있어 mv/rm/touch가 이미 최소 경로라 중복이다(디자인 원칙 3). F1은 값 하나 고치러 다른 에디터를 여는 원래 문제로 되돌아간다.
  - verified facts: herdr에서 worktree와 workspace는 1:1이다(worktree --help가 'worktree-backed workspaces', WorktreeInfo에 open_workspace_id). workspace 안에 worktree가 중첩되지 않으므로 사용자가 우려한 '하나의 workspace에 worktree 여러 개'는 herdr 모델에 존재하지 않는다.
  - unresolved follow-up: cd로 다른 경로에 가 있는 pane의 트리 처리 -> Q22에서 해소.
  - source: user Q21
- needs_normalization: false

### Q22: 파일트리 루트 정책 (T1/T2/T3)
- decision_ids: D-26
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:7e3622d3-3bd6-47bd-af28-b894b5a64703
- asked: 확인해보니 **herdr가 이미 답을 정해놨습니다.** 당신이 애매하다고 느낀 이유가 있어요.
  
  ```
  herdr worktree --help
    Manage Git worktree-backed workspaces
    list    List worktree workspaces
  ```
  
  그리고 API 스키마의 `WorktreeInfo`에 이게 있습니다:
  
  ```json
  "open_workspace_id": { "type": ["string", "null"] }
  ```
  
  **herdr에서 worktree와 workspace는 1:1입니다.** worktree를 만들면 그게 곧 workspace가 됩니다. workspace 안에 worktree가 여러 개 들어가는 구조가 아니에요.
  
  그래서 당신이 말한 "하나의 워크스페이스에 워크트리를 여러 개 세션별로"는 herdr 모델에서는 **워크스페이스가 여러 개 생기는 것**입니다. worktree 3개를 파면 workspace가 3개입니다.
  
  ## 그러면 경계가 깨끗해집니다
  
  | 단위 | 뜻 | 딸려오는 것 |
  | --- | --- | --- |
  | **workspace** | 체크아웃 하나 (메인 또는 worktree 하나) | 브라우저 1개, 파일트리 루트 1개, 프로필 default 1개 |
  | **pane** | 그 안의 터미널 하나 | 브라우저 탭, 프로필 선택 |
  
  worktree로 병렬 작업을 하면 workspace가 늘어나고, `prefix+1..9`나 사이드바로 전환합니다. 각 workspace가 자기 브라우저와 자기 트리를 갖습니다. **섞이지 않습니다.**
  
  ## 남는 애매함은 하나뿐입니다
  
  당신이 든 예 중에 **"pane 하나가 `cd`로 다른 경로로 이동해서 작업"**하는 경우입니다. 이건 worktree가 아니라서 workspace가 안 늘어납니다. 그 pane만 다른 데 가 있는 거죠.
  
  herdr이 두 값을 다 주긴 합니다 (D-02): `workspace_cwd`와 `focused_pane_cwd` / `foreground_cwd`.
  
  **Q22. 파일트리 루트를 어느 쪽으로 잡을까요?**
  
  **T1 - workspace 고정**
  트리는 항상 그 workspace의 체크아웃 루트를 보여줍니다. pane이 어디로 가든 안 움직입니다. 예측 가능하지만, 딴 데 가 있는 pane의 파일은 트리로 못 봅니다.
  
  **T2 - 포커스된 pane 추종**
  지금 보고 있는 pane의 `foreground_cwd`를 따라갑니다. 항상 "그 에이전트가 있는 곳"이 보이지만, **pane을 옮길 때마다 트리가 튑니다.** 펼쳐둔 상태도 날아가고요.
  
  **T3 - T1 기본 + 명시적 점프**
  평소엔 고정. pane이 다른 경로에 있으면 트리 상단에 "이 pane 경로로 이동" 같은 걸 띄우고, 누르면 그때 옮깁니다.
  
  **제 추천은 T3입니다.** T2는 트리가 예고 없이 튀는 게 집중에 최악입니다. 파일 찾다가 pane 전환 한 번에 위치를 잃어요. T1은 안전하지만 당신이 든 그 상황(딴 경로에서 작업하는 pane)을 못 다룹니다. T3는 기본이 예측 가능하면서 필요할 때 한 번의 클릭으로 열립니다. 디자인 원칙 4(파생 상태를 보여주되 계산시키지 말 것)에 맞게, **pane이 트리와 다른 곳에 있다는 사실 자체를 화면이 알려주는** 형태이기도 합니다.
  
  T3로 갈까요?
- recommended: T3(workspace 고정 + 명시적 점프)
- answer: T3
- immediate_notes:
  - decision: T3 채택.
  - reasoning: T2는 pane 전환마다 트리가 튀고 펼침 상태가 날아가 집중을 해친다. T1은 cd로 다른 경로에서 작업하는 pane을 다루지 못한다.
  - verified facts: herdr가 workspace_cwd와 focused_pane_cwd / foreground_cwd를 모두 노출한다(D-02).
  - source: user Q22
- needs_normalization: false

### Q23: 워크벤치 편집 중 외부 변경 충돌 정책 (C1/C2/C3)
- decision_ids: D-27
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:39d0c41f-358c-4c40-b0db-f01a3b599c6d
- asked: 기록했습니다. F2 + T3.
  
  ---
  
  F2를 고르셨으니 아까 예고한 걸 여쭐 차례입니다. **동시 편집 충돌**입니다.
  
  상황은 이렇습니다. 당신이 워크벤치에서 `config.ts`를 열어 몇 줄 고치는 중인데, 저장하기 전에 에이전트가 같은 파일을 덮어씁니다. 에이전트가 돌고 있는 환경이라 드물지 않은 일입니다.
  
  **Q23. 이때 어떻게 할까요?**
  
  **C1 - 경고하고 다시 읽기**
  "파일이 외부에서 바뀌었습니다. 다시 읽습니다"를 띄우고 에이전트 버전으로 갈아탑니다. **당신이 고치던 내용은 사라집니다.** 이전 PRD가 정했던 정책입니다.
  
  **C2 - 경고하고 선택하게 하기**
  "외부에서 바뀜. 내 편집 유지 / 다시 읽기" 중 고르게 합니다. 유지를 고르면 당신 버전이 남고, 저장하면 에이전트 변경을 덮어씁니다. 어느 쪽이든 하나는 날아가지만 **누가 날아갈지 당신이 정합니다.**
  
  **C3 - 3-way diff로 병합**
  양쪽을 보여주고 합칩니다. 아무것도 안 잃지만 만들 게 많습니다.
  
  **제 추천은 C2입니다.**
  
  C1이 이전 PRD의 선택이었는데, 그건 **⌘E로 잠깐 열었다 닫는 모델**이라 편집 중일 확률이 낮다는 전제가 있었습니다. 지금은 L1에서 워크벤치가 상시 열려 있어서 전제가 바뀌었습니다. 편집 중에 덮어쓰일 확률이 훨씬 높아졌어요.
  
  그리고 C1은 디자인 원칙 6(파괴적 동작 전에 결과를 알린다)에 어긋납니다. 알림은 뜨지만 이미 잃은 뒤니까요.
  
  C3는 원칙 2(현재 요구를 충족하는 가장 단순한 구현)로 보면 과합니다. 워크벤치는 값 하나 고치는 용도지 본격 편집기가 아니고, 그런 편집에서 3-way 병합이 필요한 경우는 드뭅니다.
  
  C2에도 대가는 있습니다. **선택 다이얼로그가 뜨는 것 자체가 흐름을 끊습니다.** 에이전트가 파일을 자주 건드리는 workspace에서는 성가실 수 있어요. 다만 그건 "안 물어보고 날려버리는 것"보다는 낫다고 봅니다.
  
  C2로 갈까요?
- recommended: C2(경고 후 사용자가 선택)
- answer: C2 고고우
- immediate_notes:
  - decision: C2 채택.
  - reasoning: 이전 PRD의 C1은 워크벤치를 잠깐 열었다 닫는 모델을 전제했으나 L1에서 상시 열려 있으므로 전제가 무효다. C1은 디자인 원칙 6(파괴적 동작 전 결과 고지) 위반이기도 하다. C3는 값 하나 고치는 용도에 과하다(원칙 2).
  - constraints: 선택 다이얼로그가 흐름을 끊는 비용을 수용했다. 어느 쪽을 골라도 한쪽 변경은 잃는다.
  - source: user Q23
- needs_normalization: false

### Q24: herdr-ide를 나만 쓰는가 남도 쓰는가
- decision_ids: D-28, D-29
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:d7a2b469-660f-45a3-9078-f195498b45be
- asked: 이제 당신이 처음 말한 세 번째, **커스텀**입니다.
  
  그런데 이걸 답하려면 먼저 정해야 할 게 있습니다. 커스텀의 형태가 여기서 갈리거든요.
  
  **Q24. herdr-ide를 당신만 쓸 건가요, 남도 쓰게 할 건가요?**
  
  이전 PRD는 **"로컬 빌드 개인 사용, 서명·공증·자동 배포 없음"**으로 정했었습니다. 그 전제가 지금도 유효한지가 궁금합니다.
  
  이게 왜 커스텀을 가르냐면:
  
  **나만 쓴다면** - 가장 유연한 커스텀은 **코드를 고치는 것**입니다. 설정 파일 스키마를 만들고 파싱하고 검증하는 건 전부 "내가 내 코드를 직접 못 고칠 때" 필요한 우회예요. 당신은 고칠 수 있습니다. 그러면 설정으로 뺄 건 **자주 바꾸는 것만** 남습니다. 키맵, 패널 비율, 색 정도.
  
  **남도 쓴다면** - 얘기가 완전히 달라집니다. 설정 계약, 기본값, 마이그레이션, 플러그인 매니페스트, 배포 서명, 업데이트 채널이 전부 스코프에 들어옵니다. 이전 PRD가 "외부 뷰어 플러그인 배포 계약"을 비목표로 뺀 이유도 이거였고요.
  
  > 매니페스트, 샌드박싱, 설치 흐름은 만들지 않는다. 새 뷰어는 herdr-ide 코드에 직접 추가한다. **사용자가 직접 개발하는 앱이므로 v1에서 외부 계약의 이득이 비용보다 작다.**
  
  **제 추천은 "v1은 나만"입니다.** 원칙 3(레이어로 성장)에 맞고, 배포 계약은 한 번 만들면 되돌리기 어려운 종류의 약속이라 실사용으로 형태가 굳은 뒤에 만드는 게 낫습니다.
  
  다만 이건 제 추측이 섞였습니다. 당신은 `herdr-remote-handoff`, `herdr-agent-context-labels`를 이미 공개 저장소로 내고 계시고, herdr 생태계에 기여하는 흐름이 있어 보입니다. herdr-ide도 그 연장선이면 처음부터 다르게 지어야 합니다.
  
  어느 쪽인가요?
- recommended: v1은 나만 (이전 PRD의 로컬 빌드 개인 사용 전제 유지)
- answer: 남들도 쓰게 할거야! 근데 커스텀은 우선은 다 하게는안할거지만 애초에 커스텀을 고려해서 너무 하드코딩하지 않는게 중요할듯?
- immediate_notes:
  - decision: 남들도 쓰는 것을 전제로 만든다. 추천을 뒤집은 사용자 결정이다. 다만 v1에서 커스텀을 전면 개방하지는 않고, '애초에 커스텀을 고려해 하드코딩하지 않는 것'을 요구했다.
  - reasoning: 사용자는 herdr-remote-handoff, herdr-agent-context-labels를 이미 공개 저장소로 운영 중이며 herdr-ide도 그 연장선이다.
  - constraints: 원칙 2(투기적 확장점 금지)와 원칙 8(버릴 foundation 금지)이 충돌하는 지점이라, '내부 단일 출처는 만들되 외부 계약은 만들지 않는다'로 선을 그었다.
  - explicit non-goals: 사용자 설정 파일 스키마, 기본값·마이그레이션 약속, 플러그인 매니페스트. 에이전트 정의는 herdr가 소유하므로 herdr-ide가 이중으로 만들지 않는다.
  - source: user Q24
- needs_normalization: false

### Q25: v1 배포 방식 (R1/R2/R3)
- decision_ids: D-29
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:106fc0b7-0593-43e8-94e5-436da9d78a99
- asked: 좋은 구분입니다. 그런데 여기 원칙끼리 부딪히는 지점이 있어서 선을 정확히 그어야 합니다.
  
  원칙 2는 **투기적 확장점을 금지**합니다. "지금은 하나뿐인데 확장 포인트를 미리 열어두는 것"이 정확히 그 예시고요. 반면 원칙 8은 **버릴 걸 알면서 깔지 말라**고 합니다. 남들이 쓸 걸 알면서 하드코딩하는 건 8이 금지하는 임시방편이죠.
  
  해소는 이렇게 됩니다. **"하드코딩하지 않는다"와 "확장점을 연다"는 다른 얘기입니다.**
  
  | | v1에서 | 뜻 |
  | --- | --- | --- |
  | **내부 단일 출처** | **한다** | 값을 코드 곳곳에 흩뿌리지 않고 한 테이블에 모은다. 그냥 좋은 구조지 투기적 추상화가 아니다 |
  | **외부 계약** | **안 한다** | 그 테이블을 사용자 설정 파일로 노출하고, 스키마·기본값·마이그레이션을 약속한다 |
  
  나중에 열 때는 **그 테이블을 파일에서 읽게만 바꾸면 됩니다.** 구조를 다시 짜지 않아요. 이게 당신이 말한 "애초에 커스텀을 고려해서 하드코딩하지 않는 것"의 정확한 형태라고 봅니다.
  
  단일 출처로 모을 것은 이 넷으로 보입니다.
  
  1. **키맵 테이블** - `command id → 기본 키`. ⌘D 같은 게 코드에 박히지 않고 테이블에만 있음
  2. **디자인 토큰** - 색, 간격, 타이포
  3. **표면 레지스트리** - 어떤 뷰어가 어떤 파일 타입을 담당하는가. 새 뷰어 추가가 등록 한 줄
  4. ~~에이전트 정의~~ - **이건 만들면 안 됩니다**
  
  4번을 뺀 이유가 있습니다. herdr API에 `server.agent_manifests`와 `server.reload_agent_manifests`가 이미 있습니다. **에이전트 정의는 herdr가 소유합니다.** herdr-ide가 따로 만들면 두 곳에 같은 정의가 생겨서 반드시 어긋납니다. orca가 20종+ 프리셋을 자기가 들고 있는 것과 다른 선택이지만, 우리는 herdr 위에 있으니 herdr 것을 읽어 쓰는 게 맞습니다.
  
  ---
  
  이제 "남도 쓴다"의 실질 비용을 확인해야 합니다.
  
  **Q25. v1에서 남들이 herdr-ide를 어떻게 받나요?**
  
  **R1 - 소스만 공개, 각자 빌드**
  GitHub에 올리고 `pnpm build` 하라고 합니다. 서명·공증 불필요, 비용 0. 대신 herdr 쓰는 개발자만 씁니다.
  
  **R2 - 서명·공증한 빌드 배포**
  `.dmg`를 받아서 바로 실행합니다. Apple Developer 계정(연 $99)과 공증 파이프라인이 필요하고, CI에 인증서를 넣어야 합니다.
  
  **R3 - Homebrew Cask**
  orca가 `Casks/` 디렉터리를 갖고 있더군요. `brew install --cask herdr-ide`. R2 위에 얹는 형태입니다.
  
  **제 추천은 v1은 R1입니다.** herdr 자체가 개발자 도구고, herdr-ide를 쓰려면 어차피 herdr을 깔고 설정해야 합니다. 그 문턱을 넘은 사람에게 `pnpm build`는 장벽이 아닙니다. 서명·공증은 사용자가 늘어난 뒤에 붙여도 늦지 않고, 그때는 R2가 순수 추가 작업이라 되돌릴 게 없습니다.
  
  R1로 갈까요?
- recommended: R1(소스 공개 + 각자 빌드)
- answer: 음 R1으로 하게하면 좋을듯~
- immediate_notes:
  - decision: R1 채택.
  - reasoning: herdr-ide를 쓰려면 herdr을 먼저 설치·설정해야 하므로 그 문턱을 넘은 사용자에게 로컬 빌드는 장벽이 아니다. 서명·공증은 나중에 순수 추가 작업이라 되돌릴 것이 없다(원칙 3).
  - explicit non-goals: Apple 서명·공증(R2), Homebrew Cask(R3).
  - verified facts: orca는 Casks/ 디렉터리를 갖고 배포한다.
  - source: user Q25
- needs_normalization: false

### Q26: v1 지원 플랫폼 (P1/P2/P3)
- decision_ids: D-30
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:6605a86f-5f40-4f0c-9ba2-5047f8ff90ea
- asked: "남도 쓴다"가 정해지니 하나가 따라옵니다. **플랫폼**입니다.
  
  이전 PRD는 macOS 전용이 강제였습니다. libghostty의 검증된 임베딩 경로가 macOS Swift뿐이라서요. **Electron으로 오면서 그 강제가 사라졌습니다.** 이제 리눅스도 윈도우도 빌드는 됩니다.
  
  관련 사실 두 개를 확인했습니다.
  
  - herdr 플러그인들의 `platforms`가 `["linux", "macos", "windows"]`입니다. `herdr-file-viewer`는 윈도우 빌드 스크립트도 갖고 있습니다. **herdr 자체는 세 플랫폼을 봅니다**
  - orca도 macOS/Windows/Linux를 다 지원합니다
  
  **Q26. v1 지원 플랫폼을 어디까지 약속할까요?**
  
  **P1 - macOS만 지원**
  빌드가 리눅스에서 될 수도 있지만 **약속하지 않습니다.** README에 macOS만 적습니다.
  
  **P2 - macOS + Linux 지원**
  둘 다 약속합니다. 리눅스 검증 환경이 필요합니다.
  
  **P3 - 세 플랫폼 지원**
  윈도우까지. 경로 처리, 셸, 단축키(⌘ → Ctrl)가 전부 갈라집니다.
  
  **제 추천은 P1입니다.**
  
  이유는 검증입니다. 당신이 지금 검증할 수 있는 건 이 맥과 mini(맥) 둘뿐입니다. 원칙 10(모든 실패는 프로세스 밖에서 관측 가능해야 한다)과 검증 원칙에 비춰보면, **테스트 못 하는 플랫폼을 지원한다고 적는 건 거짓 약속**입니다. 사용자가 리눅스에서 깨진 걸 만나도 당신은 재현조차 못 합니다.
  
  P1로 두더라도 **일부러 macOS에 종속시키지는 않습니다.** D-28의 "하드코딩하지 않는다"와 같은 결로, 경로나 셸 호출을 macOS 전제로 박지 않으면 나중에 리눅스를 켜는 게 검증 작업만 남습니다. 다만 그건 "지원"이 아니라 "막지 않음"이고, README에 쓰지 않습니다.
  
  윈도우(P3)는 더 강하게 빼는 걸 권합니다. ⌘ 키맵(D-17), 경로, 셸이 전부 갈라져서 v1 스코프가 실질적으로 배가 됩니다.
  
  P1로 갈까요?
- recommended: P1(macOS만 지원)
- answer: ㅇㅇㅇ P1
- immediate_notes:
  - decision: P1 채택.
  - reasoning: 검증 가능한 환경이 이 맥과 mini(둘 다 macOS)뿐이다. 테스트하지 못하는 플랫폼을 지원한다고 적는 것은 거짓 약속이다.
  - constraints: Linux는 '막지 않되 지원하지 않는다'. 경로·셸을 macOS 전제로 하드코딩하지 않되 README에 지원으로 적지 않는다.
  - explicit non-goals: Windows. ⌘ 키맵(D-17), 경로, 셸이 갈라져 스코프가 배가 된다.
  - verified facts: herdr 플러그인 platforms는 linux/macos/windows이고 orca는 세 플랫폼을 지원한다. Electron 전환으로 이전 PRD의 macOS 강제(libghostty)는 사라졌다.
  - source: user Q26
- needs_normalization: false

### Q27: pet과 사이드바의 역할 분리 (E1/E2/E3)
- decision_ids: D-31
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:5b5e3582-a1da-4f52-b02e-72f5b45a8c26
- asked: 체크포인트 3 기록했습니다. 등록부 30행, 열린 P0/P1 없음.
  
  ---
  
  이제 **pet**입니다. D-07에서 "herdr-ide의 한 표면"으로 정했는데 구체 형태가 비어 있습니다.
  
  여기 문제가 하나 있습니다. 대화 초반에 제가 이렇게 제안했었죠.
  
  > 화면에 동시에 살아있는 **"주목 요구" 표면은 최대 1개.** pet 뱃지와 사이드바 뱃지가 같은 `_new` 토큰을 동시에 표시하면 위반
  
  지금 L1(D-16)에서 사이드바에 **agents 목록과 attention 표시**가 있고, 그 **같은 사이드바에 pet도 있습니다.** 둘이 같은 걸 두 번 말합니다. pet의 `docs/status-model.md`에 당신이 직접 써둔 문장과도 부딪힙니다.
  
  > the dashboard must not disagree with it
  
  **Q27. pet의 역할을 어떻게 나눌까요?**
  
  **E1 - 역할 분리 (창 포커스 기준)**
  herdr-ide 창을 **보고 있을 때는** 사이드바가 알린다. pet은 조용히 있거나 분위기만 낸다(roam, sleep). 창이 **뒤로 가면** pet이 데스크톱 위로 나와서 알린다. 지금 pet-app이 하던 일을 그대로 이어받는 거죠.
  
  **E2 - pet이 사이드바 안에서 유일한 알림 표면**
  agents 목록은 상태를 텍스트/색으로만 보여주고, "지금 뭔가 필요하다"는 신호는 pet만 냅니다.
  
  **E3 - 둘 다 표시**
  중복을 허용합니다. 놓칠 확률이 낮아집니다.
  
  **제 추천은 E1입니다.**
  
  pet이 원래 잘하는 게 **당신이 herdr-ide를 안 보고 있을 때** 알리는 겁니다. 데스크톱 펫이라는 형태 자체가 그 용도예요. 창 안에 갇힌 pet은 사이드바 뱃지보다 정보 밀도가 낮으면서 자리만 차지합니다.
  
  E1로 가면 **D-15(pet-app 은퇴)의 조건도 명확해집니다.** herdr-ide가 "창이 뒤로 갔을 때 데스크톱에 나오는 pet"까지 구현해야 pet-app을 지울 수 있습니다. 그전까지는 pet-app이 그 역할을 계속합니다.
  
  E3는 디자인 원칙 7과 제가 제안한 기준 둘 다에 어긋나서 뺐습니다. 같은 사실을 두 곳에서 말하면 신뢰가 갈라져서, 결국 둘 다 안 믿게 됩니다.
  
  다만 E1에는 기술적 부담이 있습니다. Electron 창이 뒤로 갔을 때 **데스크톱에 항상 떠 있는 펫 창**을 띄워야 하는데, 이건 `pet-window-macos.md`에 당신이 적어둔 함정들(그림자 끄기, Spaces, `startDragging`)을 Electron에서 다시 만나는 일입니다. Tauri에서 이미 푼 문제를 다시 푸는 거죠.
  
  E1로 갈까요? 아니면 v1에서는 pet을 사이드바 안에만 두고(분위기 담당), 데스크톱 펫은 pet-app에 계속 맡길까요?
- recommended: E1(창 포커스 기준 역할 분리)
- answer: 음 그런거 같다. 레스고
- immediate_notes:
  - decision: E1 채택.
  - reasoning: 사이드바와 pet이 같은 _new 토큰을 동시에 표시하면 '주목 요구 표면은 최대 1개' 기준과 pet의 status-model.md('대시보드는 사이드바와 불일치해서는 안 된다')에 둘 다 어긋난다. pet이 원래 잘하는 것은 herdr-ide를 안 보고 있을 때 알리는 일이다.
  - explicit non-goals: E3(둘 다 표시). 같은 사실을 두 곳에서 말하면 신뢰가 갈라진다(디자인 원칙 7).
  - unresolved follow-up: v1 구현 범위(데스크톱 펫을 herdr-ide가 만드는가) -> Q28.
  - source: user Q27
- needs_normalization: false

### Q28: v1에서 데스크톱 펫을 herdr-ide가 만들 것인가 (W1/W2)
- decision_ids: D-32
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:a18dfc85-398c-4960-88ac-8cc4d1fbeaf6
- asked: E1로 기록했습니다 (D-31). 그런데 E1은 **모델**이고, v1에서 어디까지 만드느냐는 아직 안 정해졌습니다. 이게 실제 작업량을 크게 가릅니다.
  
  **Q28. v1에서 데스크톱 펫을 herdr-ide가 직접 만들까요?**
  
  **W1 - v1은 사이드바 pet만. 데스크톱 펫은 pet-app이 계속 담당**
  herdr-ide는 창 안의 pet만 만듭니다. 창이 뒤로 갔을 때의 알림은 지금처럼 `pet-app`이 합니다. 두 앱이 같은 herdr 서버를 보고 있으니 상태는 어긋나지 않습니다.
  
  **W2 - v1에서 herdr-ide가 데스크톱 펫까지 흡수**
  Electron으로 항상 위에 뜨는 무테 창을 만들고, `pet-window-macos.md`의 함정들(그림자 끄기, Spaces 처리, 오프스크린 가드, `startDragging`)을 Electron에서 다시 풉니다. 그러면 `pet-app`을 바로 지울 수 있습니다.
  
  **제 추천은 W1입니다.**
  
  원칙 3(레이어로 성장, 동작하는 제품을 미완성 복잡도와 바꾸지 않는다)에 정확히 걸리는 사례입니다. `pet-app`은 지금 동작하고, 그 안의 1,131줄은 macOS 창 조작의 함정을 이미 다 통과한 코드입니다. 그걸 v1에 다시 짜면 herdr-ide의 본체(터미널, 브라우저, 워크벤치)가 늦어집니다.
  
  그리고 이게 **D-15의 은퇴 조건을 구체화합니다.** "herdr-ide가 데스크톱 펫을 갖추면 pet-app을 지운다"가 되는 거죠. 조건이 명확하니 흐지부지 둘 다 남는 사태를 막습니다(원칙 1).
  
  W1의 대가는 **v1 동안 앱이 두 개 떠 있는 것**입니다. 메뉴바에 herdr-ide와 pet-app이 같이 있고, 종료·업데이트를 따로 해야 합니다. 그리고 E1의 "창 포커스에 따른 역할 분리"가 완전히는 안 됩니다. herdr-ide가 앞에 있어도 pet-app은 그걸 모르니 계속 자기 일을 합니다.
  
  여기서 하나 더 갈리는데, W1을 고르면 **그 반쪽 상태를 어떻게 처리할지**를 정해야 합니다. pet-app이 herdr-ide의 포커스를 알아야 조용해지거든요. 방법은 있습니다. herdr API에 `notification.show`와 이벤트 구독이 있으니 herdr-ide가 포커스 상태를 알리고 pet-app이 그걸 듣는 식으로요. 다만 그건 곧 지울 앱에 배선을 추가하는 일이라 원칙 8(버릴 걸 알면서 깔지 말 것)과 부딪힙니다.
  
  그래서 W1을 고르면 저는 **배선을 안 하는 쪽**을 권합니다. v1 동안은 pet-app이 herdr-ide 앞뒤를 모른 채 그냥 평소대로 동작하고, 약간의 중복을 감수합니다. 임시 배선을 깔지 않는 게 낫습니다.
  
  W1로 갈까요?
- recommended: W1(v1은 사이드바 pet만, 데스크톱 펫은 pet-app 유지)
- answer: 오 W2를 하는게 나을지도? 애초에 RUST로 안가는거면...
- immediate_notes:
  - decision: 사용자가 W2를 제시하며 추천을 뒤집었다. 근거는 'Rust로 안 갈 거면 pet-app의 코드는 어차피 버려진다'였다.
  - reasoning: D-14로 Rust를 재사용하지 않으므로 pet-app을 살려두는 것은 자산 보존이 아니라 재작성을 미루는 것이다. W1은 곧 지울 앱에 포커스 연동 배선을 깔아야 해 원칙 8과도 부딪힌다.
  - unresolved follow-up: Electron에서의 실제 비용 확인 -> Q29에서 조사.
  - source: user Q28
- needs_normalization: false

### Q29: W2의 실제 비용 조사와 스파이크 배치
- decision_ids: D-32, D-33, D-15
- route: mixed
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:fe69d01b-e726-46f2-bde0-e6085b1a5fab
- asked: 당신 판단이 맞습니다. **제 추천을 W2로 바꾸겠습니다.**
  
  `pet-window-macos.md`를 읽어보니, 그 1,131줄의 대부분이 **macOS가 어려워서가 아니라 Tauri/WKWebView가 부족해서** 생긴 코드입니다.
  
  | 겪은 함정 | 원인 | Electron에서는 |
  | --- | --- | --- |
  | `tauri.conf.json` **두 개**에 `shadow: false` | Tauri 설정 이중화 | `hasShadow: false` 한 곳 |
  | 드래그가 끊김 → 루프를 Rust로 이관, 8ms 네이티브 스레드 | **"WKWebView throttles timers in unfocused windows"** | Chromium은 이 제약이 다르고, `backgroundThrottling: false`가 표준 옵션. `-webkit-app-region: drag`면 드래그 자체가 네이티브 |
  | sync Tauri 커맨드가 메인 스레드에서 드래그 틱 사이에 끼어듦 | Tauri sync 커맨드 모델 | Electron IPC는 기본 비동기 |
  | 첫 클릭 활성화 함정 → 별도 `pet-hit` 창 | AppKit first-click | `acceptFirstMouse: true` + `setIgnoreMouseEvents(true, {forward:true})` |
  | 항상 위 / Spaces | Tauri에 API 부족 | `setAlwaysOnTop(true, 'screen-saver')`, `setVisibleOnAllWorkspaces(true, {visibleOnFullScreen:true})` |
  
  **드래그 아키텍처 전체가 WKWebView의 타이머 스로틀링 하나 때문에 생긴 것**이라는 게 문서에 직접 적혀 있습니다.
  
  > WKWebView throttles timers in unfocused windows, and the hit window is never focused, so the per-frame invoke path stuttered exactly while dragging.
  
  Chromium 위에서는 그 전제가 없습니다. 그러니 `macos_window.rs` 462줄과 네이티브 드래그 스레드가 통째로 필요 없어질 가능성이 큽니다.
  
  **살아남는 건 두 가지입니다.**
  
  1. **오프스크린 클램프 기하** - 연결 안 된 모니터 좌표(`[542720, 163840]` 사건)를 막는 로직. 이건 `window.rs`에 순수 함수 + 테스트로 있어서 **TS로 그대로 옮기면 됩니다.** 실제 사고 좌표가 테스트에 박혀 있으니 그 테스트도 같이 옮깁니다
  2. **팝오버 배치 뒤집기** - 모니터 작업 영역 기준 좌우/상하 반전
  
  즉 **경험은 남고 코드만 옮기는 것**입니다. 재작성이 아니라 이식이에요.
  
  ---
  
  그래서 W2로 가면 이렇게 정리됩니다.
  
  - `pet-app`을 v1에서 은퇴시킵니다. D-15의 "실사용 통과 후 삭제"가 "v1 완료 시 삭제"로 앞당겨집니다
  - 앱이 하나로 유지되고, E1(창 포커스에 따른 역할 분리)이 **실제로 동작합니다.** W1에서는 pet-app이 herdr-ide 앞뒤를 몰라서 반쪽이었죠
  - 곧 지울 앱에 배선을 까는 원칙 8 위반도 사라집니다
  
  **다만 검증되지 않은 전제가 있습니다.** 위 Electron 옵션들은 문서상 존재하는 것이고 제가 실제로 돌려본 게 아닙니다. 특히 클릭스루 + 첫 클릭 + 드래그 조합은 실물로 확인해야 합니다. 그래서 이걸 **v1 초반의 게이팅 스파이크**로 두는 걸 권합니다. 이전 PRD의 T1(libghostty 스파이크)이 있던 자리에 이게 들어가는 셈입니다. 스파이크가 실패하면 그때 W1으로 후퇴하면 되고, 그때는 pet-app이 아직 살아 있으니 후퇴 비용도 없습니다.
  
  W2 + 초반 스파이크로 갈까요?
- recommended: W2 + v1 초반 게이팅 스파이크 (조사 후 추천을 W1에서 뒤집음)
- answer: ㅇㅇㅇ 레스고
- immediate_notes:
  - decision: W2 채택, 초반 스파이크 포함. 조사 결과 에이전트 추천이 W1에서 W2로 바뀌었다.
  - verified facts: docs/pet-window-macos.md가 드래그 아키텍처(8ms 네이티브 틱, 별도 pet-hit 창)의 원인을 'WKWebView throttles timers in unfocused windows'로 직접 기록한다. Electron/Chromium에는 그 전제가 없고 hasShadow, acceptFirstMouse, setIgnoreMouseEvents(forward), setAlwaysOnTop('screen-saver'), setVisibleOnAllWorkspaces(visibleOnFullScreen), backgroundThrottling, -webkit-app-region:drag가 1급 옵션이다. 살아남는 이식 대상은 오프스크린 클램프 기하(실제 사고 좌표 [542720, 163840]가 테스트에 고정)와 팝오버 배치 반전뿐이다.
  - constraints: 위 Electron 옵션들은 문서 확인이며 실행 검증이 없다. 특히 클릭스루 + 첫 클릭 + 드래그 조합이 미검증이라 게이팅 스파이크로 둔다.
  - unresolved follow-up: 스파이크 실패 시 W1으로 후퇴하며 그 시점에 pet-app이 살아 있어 후퇴 비용이 없다.
  - source: user Q29
- needs_normalization: false

### Q30: v1 완료 판정 방식 (V1/V2/V3)
- decision_ids: D-34
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:906fe7d0-e56d-47a5-997e-babac52115bf
- asked: 기록했습니다. D-15도 "v1 완료 시 은퇴"로 갱신했습니다.
  
  ---
  
  이제 **검증**입니다. 이게 PRD의 완료 판정을 정하는 부분이라 남은 것 중 제일 중요합니다.
  
  먼저 사실 하나. 이전 PRD는 완료 판정을 이렇게 잡았습니다.
  
  > v1의 완료 판정은 **골든 시나리오 실사용 통과**다.
  > 하루 작업을 터미널 앱 대신 herdr-ide로 시작해도 아쉬움이 없고, 이미지 확인을 위해 다른 앱을 여는 일이 사라진다.
  
  그리고 "3초 내"라는 기준을 자동 계측 없이 수동 판정하기로 했었습니다.
  
  **Q30. v1 완료를 무엇으로 판정할까요?**
  
  **V1 - 골든 시나리오 실사용** (이전 PRD 방식)
  당신이 하루를 herdr-ide로 일해보고 판정합니다. 진짜 목표에 가장 가깝지만, 언제 끝났는지가 당신 기분에 달려 있습니다.
  
  **V2 - 관측 가능한 체크리스트**
  "터미널에 attach된다", "⌘D로 분할된다", "브라우저 탭에서 요소를 집으면 에이전트 pane에 텍스트가 들어간다"처럼 눈으로 확인 가능한 항목을 나열하고 전부 통과하면 완료입니다.
  
  **V3 - V2 + 자동화 테스트**
  순수 로직(오프스크린 클램프, 상태 모델, `_new` 토큰 판정)은 자동 테스트로, 나머지는 체크리스트로 봅니다.
  
  **제 추천은 V3입니다.**
  
  V1만으로는 원칙 10(모든 실패는 프로세스 밖에서 관측 가능해야 한다)을 못 지킵니다. "아쉬움이 없다"는 밖에서 관측이 안 되고, 나중에 회귀했는지도 알 수 없어요.
  
  V2만으로도 부족한 이유는, 이미 **테스트를 옮겨올 자산이 있어서**입니다. D-32에서 확인했듯 `window.rs`의 클램프 함수에는 실제 사고 좌표(`[542720, 163840]`)가 박힌 테스트가 있고, pet의 `_new` 토큰 판정에도 테스트가 있습니다(`herdr.rs`). 이건 **당신이 실제로 겪은 버그를 막는 테스트**라 원칙 12(테스트는 값이 유지비를 넘을 때만)의 기준을 확실히 통과합니다. 안 옮기면 같은 사고를 다시 겪습니다.
  
  반대로 **UI 배선을 테스트하지는 않는 걸** 권합니다. "사이드바가 렌더된다" 같은 테스트는 리팩터마다 깨지고 잡는 버그가 없습니다. 원칙 12가 명시적으로 금지하는 형태고요.
  
  그리고 골든 시나리오(V1)는 **버리지 말고 사람 검증 항목으로 남기는** 게 맞습니다. 자동으로 못 재는 게 진짜로 있으니까요. 다만 그건 완료 게이트가 아니라 "이 정도면 쓸 만한가"를 묻는 별도 항목입니다.
  
  정리하면 세 층입니다.
  
  | 층 | 대상 | 판정 |
  | --- | --- | --- |
  | 자동 테스트 | 클램프 기하, 상태 모델, `_new` 토큰, 키맵 테이블 해석 | 필수 통과 |
  | 런타임 확인 | attach, 분할, 브라우저 grab, 파일트리 T3, 편집 충돌 C2, 펫 스파이크 | 필수 통과 |
  | 사람 판정 | 하루 실사용 후 골든 시나리오 | 기록하되 게이트 아님 |
  
  V3로 갈까요?
- recommended: V3(자동 테스트 + 런타임 확인 + 사람 판정)
- answer: V2로 가도 될듯? chromux나 computer-use든 뭐든 검증 가능한거아냐?
- immediate_notes:
  - decision: 사용자가 V2로 충분하다고 보고, chromux나 computer-use로 검증 가능하지 않냐고 되물었다.
  - reasoning: 실제 앱을 몰아서 확인하는 편이 유닛 테스트보다 낫다는 지적이며 원칙 12('호출자가 관측하는 것을 검증하라')에 더 맞는다.
  - unresolved follow-up: 순수 함수 영역의 예외 처리 -> Q31.
  - source: user Q30
- needs_normalization: false

### Q31: chromux로 herdr-ide 자체를 검증하는 방식 확정
- decision_ids: D-34, D-35
- route: mixed
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:79678957-8451-431a-8222-b21d58acaf8c
- asked: 맞습니다. 그리고 Electron이라서 더 잘 됩니다.
  
  **herdr-ide 자체가 Chromium이라 CDP를 엽니다.** `--remote-debugging-port`로 띄우면 **chromux가 herdr-ide에 붙어서 UI를 직접 조작**할 수 있습니다. 브라우저 패널에 CDP를 여는 것(D-19)과 같은 방식이 앱 자체에도 적용되는 거죠.
  
  그러면 검증이 이렇게 됩니다.
  
  ```
  chromux open test --cdp-url http://127.0.0.1:<herdr-ide port>
  chromux run test -  # 사이드바 클릭, ⌘D 분할, grab 버튼 누르기
  ```
  
  그리고 결과는 **herdr 소켓 API로 확인**합니다. `pane.list`로 분할이 실제로 생겼는지, `pane.read`로 grab한 텍스트가 에이전트 pane에 들어갔는지 봅니다. UI를 흉내 내는 게 아니라 **진짜 앱을 몰고 진짜 상태를 읽는** 거라, 원칙 12가 말하는 "호출자가 관측하는 것을 검증하라"에 더 잘 맞습니다. 제가 제안한 유닛 테스트보다 낫습니다.
  
  ## 다만 한 조각은 남습니다
  
  **오프스크린 클램프 기하**입니다. `[542720, 163840]` 사건이요.
  
  이건 chromux로 못 잡습니다. 실패 모드가 "창이 연결 안 된 모니터 좌표에 놓여서 `show()`는 성공하는데 아무것도 안 보임"이라, 웹 레이어 밖이고 재현하려면 모니터를 뽑아야 합니다.
  
  그런데 이건 **이미 순수 함수로 짜여 있고 테스트도 있습니다.**
  
  > The geometry for both lives in `crates/herdr-core/src/window.rs` as pure functions with tests. **The real incident coordinates are pinned in those tests.**
  
  `저장된 좌표 + 모니터 목록 → 클램프된 좌표`. 앱도 화면도 필요 없습니다. TS로 옮기는 데 몇 십 줄이고 유지비가 사실상 0이며, **실제로 일어난 사고**를 막습니다. 원칙 12의 가격 기준을 확실히 통과합니다. 안 옮기면 같은 사고를 다시 겪고요.
  
  `_new` 토큰 판정도 비슷합니다. `herdr.rs`에 테스트가 있고(확인된 `?` → idle, 미확인 `?` → attention), 이건 chromux로도 잡히긴 하지만 순수 함수라 옮기는 게 더 쌉니다.
  
  ## 그래서 이렇게 제안합니다
  
  | 층 | 방법 |
  | --- | --- |
  | **주 검증** | chromux로 herdr-ide를 몰고, herdr 소켓 API로 상태 확인 |
  | **작은 예외** | 순수 함수 2건(클램프 기하, `_new` 토큰)은 기존 테스트를 이식 |
  | **사람 판정** | 하루 실사용 골든 시나리오. 기록하되 게이트 아님 |
  
  UI 배선 유닛 테스트는 안 씁니다. 리팩터마다 깨지고 잡는 버그가 없습니다.
  
  **Q31. 이렇게 갈까요?** 사실상 당신이 말한 V2인데, 이미 있는 테스트 2개만 버리지 말고 가져오자는 얘기입니다.
- recommended: chromux 주도 검증 + 순수 함수 2건만 테스트 이식
- answer: ㅇㅇㅇ 그렇게 하자 최대한 에이전트가 다 검증할건 할수있도록 하는게 핵심인듯
- immediate_notes:
  - decision: 채택. 더불어 사용자가 '에이전트가 최대한 다 검증할 수 있게 하는 것이 핵심'이라는 요구를 추가했고, 이를 검증 전략이 아니라 설계 요구사항(D-35)으로 승격했다.
  - verified facts: herdr-ide가 Electron이므로 --remote-debugging-port로 자기 CDP를 열 수 있고 chromux가 붙어 UI를 조작할 수 있다. 결과는 herdr 소켓 API(pane.list, pane.read)로 확인한다.
  - constraints: 오프스크린 클램프는 실패 모드가 웹 레이어 밖이라 chromux로 재현 불가하므로 순수 함수 테스트를 이식한다. _new 토큰 판정도 순수 함수라 이식이 더 싸다.
  - explicit non-goals: UI 배선 유닛 테스트. 리팩터마다 깨지고 잡는 버그가 없다(원칙 12).
  - source: user Q31
- needs_normalization: false

### Q32: 코드를 어느 저장소에 둘 것인가 (S1/S2)
- decision_ids: D-36, D-06
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:5b032441-2116-4800-af97-4f84a708950a
- asked: "에이전트가 최대한 검증"을 **설계 요구사항**으로 따로 기록했습니다 (D-35). 검증 전략이 아니라 제품 요구로요. 이 조건을 못 만족하는 기능 설계는 재검토 대상이 됩니다.
  
  ---
  
  거의 다 왔습니다. 실무적인 것 하나가 남았습니다.
  
  **Q32. 코드를 어느 저장소에 둘까요?**
  
  지금 상태입니다.
  
  - `~/projects/herdr-ide` - 소스 0줄, `agents/`만 있음. 커밋 하나도 없음(`master`에 커밋 없음)
  - `~/projects/herdr-pet` - 동작하는 Tauri 앱. `crates/herdr-core`, `apps/pet-app`, `plugins/herdr-agent-pet`. 공개 저장소 `yansfil/herdr-pet` (private)
  
  **S1 - `herdr-ide`를 본진으로**
  새 저장소에서 Electron 앱을 시작합니다. `herdr-pet`은 v1 완료 시 아카이브합니다(D-15). 이식할 것(클램프 기하 테스트, `_new` 토큰 판정, status-model 문서)은 복사해 옵니다.
  
  **S2 - `herdr-pet`을 개명해서 본진으로**
  기존 저장소에 `apps/ide-app`을 추가하고 이름을 바꿉니다. 히스토리와 `docs/`(status-model, pet-window-macos, ambient-signals 등)가 그대로 남습니다.
  
  **제 추천은 S1입니다.**
  
  D-32로 pet-app이 v1에 은퇴하고 D-14로 Rust를 안 쓰기로 했으니, `herdr-pet`에 남는 건 **문서와 에셋뿐**입니다. Rust 크레이트 2,206줄, Tauri 앱 2,576줄, `Cargo.lock` 116KB가 전부 죽은 코드가 됩니다. 그 위에 Electron 앱을 얹으면 저장소가 한동안 "절반은 안 쓰는 Rust"인 상태로 남습니다. 원칙 1(하위호환 유지 안 함, 옛 경로는 지운다)에 어긋나고요.
  
  `herdr-pet`이 private이라는 것도 걸립니다. D-29에서 공개 배포를 정했으니 어차피 공개 저장소가 필요합니다.
  
  **꼭 옮겨야 할 것은 코드가 아니라 문서입니다.**
  
  - `docs/status-model.md` - `_new` 토큰 계약, pose 우선순위. D-31의 근거
  - `docs/pet-window-macos.md` - D-33 스파이크가 확인해야 할 함정 목록
  - `docs/theme-contract.md`, `docs/ambient-signals.md`
  - `docs/pet-assets.md` + 펫 아트 에셋
  - `agents/rules/` 의 invariants (`INV-herdr-unseen-token`, `INV-pet-state-off-main-thread` 등)
  
  이것들은 **실제 사고에서 나온 지식**이라 잃으면 같은 사고를 다시 겪습니다. S1로 가면 이걸 명시적으로 옮기는 작업이 v1 태스크로 들어가야 합니다.
  
  S1로 갈까요? `herdr-pet` 히스토리를 남기는 게 중요하면 S2도 합리적입니다.
- recommended: S1(herdr-ide를 본진으로 새로 시작)
- answer: S1으로 가야지 ㅇㅇㅇ
- immediate_notes:
  - decision: S1 채택.
  - reasoning: D-32로 pet-app이 은퇴하고 D-14로 Rust를 안 쓰므로 herdr-pet에 남는 것이 문서와 에셋뿐이다. 그 위에 Electron을 얹으면 '절반은 죽은 Rust' 상태가 되어 원칙 1에 어긋난다. herdr-pet이 private인 것도 D-29(공개 배포)와 맞지 않는다.
  - constraints: 이식해야 할 것은 코드가 아니라 사고에서 나온 지식이다(status-model, pet-window-macos, theme-contract, ambient-signals, pet-assets와 아트, agents/rules의 invariants, 순수 함수 테스트 2건). 이 이식을 v1 태스크로 명시한다.
  - source: user Q32
- needs_normalization: false

### Q33: v1 원격 herdr 서버 지원 범위 (M1/M2/M3)
- decision_ids: D-37
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:35c2cfea-dee4-470e-9faf-cdaab0767877
- asked: 마지막으로 큰 게 하나 남았습니다. 제가 여태 안 물어봤는데 스코프를 크게 가릅니다.
  
  **원격입니다.** 당신은 mini(맥미니)를 Tailscale로 쓰고 있고, `herdr-remote-handoff`로 세션을 넘기는 도구까지 만들어 두셨죠. 이전 PRD는 원격을 **스코프 안**에 뒀었습니다.
  
  > 리모트: herdr 리모트 서버 attach와 리모트 pane의 파일 보기 전용 탐색.
  
  그런데 이번 결정들 때문에 **원격에서 깨지는 게 생겼습니다.**
  
  D-19에서 이렇게 정했습니다. herdr-ide가 `127.0.0.1:<port>`에 CDP를 열고, pane 만들 때 그 주소를 env로 주입하고, 에이전트가 chromux로 붙는다.
  
  **원격 workspace에서는 이게 성립하지 않습니다.** mini에서 도는 에이전트에게 당신 맥의 `127.0.0.1`은 자기 자신입니다. 붙으려면 역방향 터널(`ssh -R`)을 깔아서 mini의 loopback을 당신 맥의 herdr-ide 포트로 연결해야 합니다. 파일트리도 마찬가지로 원격 파일을 읽어야 하는데 herdr 소켓 API로는 안 되고 별도 경로가 필요합니다.
  
  **Q33. v1에서 원격을 어디까지 볼까요?**
  
  **M1 - 로컬 전용**
  herdr-ide는 로컬 herdr 서버만 봅니다. mini는 지금처럼 TUI + `herdr-remote-handoff`로 씁니다.
  
  **M2 - 원격 터미널까지만**
  `ssh -L`로 원격 herdr 소켓에 붙어서 사이드바에 원격 workspace가 보이고 터미널 attach가 됩니다. **파일트리와 브라우저 패널은 로컬 workspace에서만** 동작하고, 원격 workspace를 선택하면 우측 패널에 "원격에서는 지원하지 않음"이 표시됩니다.
  
  **M3 - 원격 완전 지원**
  역방향 터널, 원격 파일 접근까지 다 만듭니다.
  
  **제 추천은 M2입니다.**
  
  터미널 attach는 싸게 됩니다. `herdr-pet/crates/herdr-core/src/ssh.rs`가 **106줄**로 이미 증명했어요. `ssh -L <local>:<remote-sock>` 하나면 나머지 소켓 API가 전부 그대로 동작합니다. 그것만으로도 "mini에서 뭐가 돌고 있나"를 사이드바에서 보고 바로 붙는 값이 큽니다.
  
  M3는 v1에 안 맞습니다. 역방향 터널은 배관이 늘 뿐 아니라 **실패 모드가 조용합니다.** 터널이 끊기면 에이전트의 chromux가 붙기만 하고 아무 일도 안 일어나요. 원칙 10(모든 실패는 프로세스 밖에서 관측 가능)을 지키려면 그 상태를 감지해 알리는 것까지 만들어야 하고, 그러면 v1이 늘어집니다.
  
  M1이 아닌 이유는 mini가 실제로 쓰이는 기계라서입니다. 다만 **당신이 mini에서 대화형 개발을 얼마나 하는지**는 제가 모릅니다. Hermes 게이트웨이 같은 무인 자동화가 주 용도고 대화형은 가끔이라면, M1으로 자르고 원격은 기존 TUI에 맡기는 것도 충분히 합리적입니다.
  
  M2로 갈까요, M1으로 자를까요?
- recommended: M2(원격 터미널까지, 파일트리·브라우저는 로컬만)
- answer: M2! 그런데 파일트리 연동까지는 해주면 안될까..? ㅠ-ㅠ
- immediate_notes:
  - decision: M2 채택하되 사용자가 원격 파일트리를 추가 요청했다.
  - reasoning: D-19의 CDP 주입이 원격에서는 역방향 터널을 요구하고 그 실패가 조용해 원칙 10을 지키려면 감지·고지까지 만들어야 한다. 반면 터미널 attach는 ssh -L 하나로 되며 herdr-pet의 ssh.rs 106줄이 이미 증명했다.
  - unresolved follow-up: 원격 파일트리의 편집 허용 여부 -> Q34.
  - source: user Q33
- needs_normalization: false

### Q34: 원격 파일트리 추가와 편집 경계
- decision_ids: D-37
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:a4444231-90a0-47ab-ae1c-631d587bf3ac
- asked: 됩니다. 생각보다 싸게 붙일 수 있어요.
  
  ## 배관이 이미 있습니다
  
  원격 터미널을 위해 어차피 SSH 연결을 엽니다(D-19의 `ssh -L`). **같은 연결을 파일 접근에 재사용**하면 됩니다.
  
  구체적으로는 ControlMaster를 씁니다.
  
  ```
  ssh -M -S <control-socket> mini      # 연결 1회, ~/.ssh/config 그대로 존중
  ssh -S <control-socket> -L ...       # herdr 소켓 터널
  sftp -o ControlPath=<control-socket> # 파일 나열·읽기, 재인증 없음
  ```
  
  `~/.ssh/config`의 `mini` 별칭과 Tailscale 호스트, 키 인증이 전부 그대로 먹습니다. herdr-pet의 `ssh.rs`가 `ssh` CLI를 쓴 것과 같은 이유죠. 라이브러리를 쓰면 ssh config를 직접 파싱해야 하는데 그건 함정이 많습니다.
  
  SFTP는 디렉터리 나열과 파일 읽기가 1급 기능이라 파일트리에 그대로 맞습니다. T3(pane 경로 점프)도 원격에서 동작합니다. `focused_pane_cwd`가 원격 herdr 스냅샷에 이미 들어있으니까요.
  
  ## 다만 원격은 보기 전용을 권합니다
  
  이전 PRD가 이걸 명시적으로 뺐는데, 이유가 지금도 유효합니다.
  
  > **리모트 파일 편집과 리모트 검색** - 리모트는 보기 전용이다. 저장 충돌, 권한, 원격 검색 실행의 엣지 케이스 비용이 크다.
  
  원격 쓰기를 넣으면 D-27(편집 충돌 C2)을 네트워크 너머에서 다시 풀어야 합니다. 저장 중 연결이 끊기면 파일이 반쯤 써진 상태로 남을 수 있고, 그건 로컬 편집에는 없던 실패 모드입니다. 원칙 4(실패를 명시적으로 드러낸다)를 지키려면 그 경로를 따로 설계해야 하고요.
  
  **원격에서 고쳐야 하면 그 pane의 터미널에서 하면 됩니다.** 터미널이 바로 옆에 붙어 있으니 경로가 이미 최단입니다(디자인 원칙 3).
  
  ## 정리하면 이렇게 됩니다
  
  | | 로컬 workspace | 원격 workspace |
  | --- | --- | --- |
  | 터미널 attach·분할 | 됨 | 됨 |
  | 파일트리 탐색 | 됨 | **됨** (SFTP) |
  | 뷰어 (이미지·md·diff) | 됨 | **됨** |
  | 인라인 편집·저장 (F2) | 됨 | **안 됨** (보기 전용) |
  | 브라우저 패널·grab | 됨 | 안 됨 |
  
  원격에서 안 되는 두 칸은 그 자리에 이유를 표시합니다. 회색으로 비워두고 사용자가 짐작하게 두지 않습니다(디자인 원칙 7).
  
  **Q34. 이렇게 갈까요?** 원격 파일트리는 넣되 편집은 로컬만, 브라우저는 로컬만입니다.
- recommended: 원격 파일트리는 넣되 보기 전용, 브라우저는 로컬만
- answer: 오케이 우선 그렇게 가자!
- immediate_notes:
  - decision: 채택. 원격은 탐색·뷰어까지 되고 인라인 편집과 브라우저 패널은 로컬 전용이다.
  - reasoning: 원격 쓰기는 D-27(편집 충돌 C2)을 네트워크 너머에서 다시 풀어야 하고 저장 중 연결 끊김이라는 새 실패 모드가 생긴다. 원격 수정은 그 pane의 터미널이 이미 최단 경로다(디자인 원칙 3).
  - verified facts: SSH ControlMaster 연결 하나로 -L 터널과 sftp를 함께 태울 수 있고 ~/.ssh/config의 별칭·키 인증이 그대로 존중된다. T3 점프에 필요한 focused_pane_cwd는 원격 스냅샷에도 있다.
  - constraints: 원격에서 안 되는 기능은 회색으로 비워두지 않고 이유를 표시한다(디자인 원칙 7).
  - source: user Q34
- needs_normalization: false

### Q35: herdr 서버 미실행 시 동작 (N1/N2)
- decision_ids: D-38
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:f9df4392-3862-42ed-baf2-5b52e30a5f60
- asked: 기록했습니다. 이제 마지막 하나입니다. **herdr 서버가 없을 때 어떻게 할지**입니다.
  
  herdrm이 이 문제를 다루는 방식을 확인해봤습니다. `HerdrService.swift`에 이렇게 되어 있더군요.
  
  ```swift
  try await localServer.ensureRunning(socketPath: socketPath)
  ```
  
  소켓이 없거나 응답이 없으면 **자동으로 herdr 서버를 띄웁니다.** `LocalServer.swift` 11.7KB가 그 일만 합니다. 그리고 "소켓 파일 자체가 없는 경우"와 "파일은 있는데 죽은 경우"를 구분해서 다룹니다.
  
  **Q35. herdr-ide를 켰는데 herdr 서버가 안 돌고 있으면?**
  
  **N1 - 자동으로 띄운다** (herdrm 방식)
  앱을 켜면 알아서 서버가 뜹니다. 사용자는 herdr가 있다는 걸 몰라도 됩니다.
  
  **N2 - 안내하고 사용자가 띄운다**
  "herdr 서버가 실행 중이 아닙니다. `herdr`을 실행해 주세요"를 띄웁니다. 버튼 하나로 띄우는 것도 포함입니다.
  
  **제 추천은 N2, 단 버튼 포함입니다.**
  
  N1이 편해 보이지만 문제가 있습니다. herdr 서버는 **당신의 모든 에이전트 세션을 들고 있는 상태 저장 프로세스**입니다. herdr-ide가 그걸 조용히 띄우면, 어느 herdr가 언제 왜 떴는지가 흐려집니다. 특히 당신은 mini에서 launchd로 게이트웨이 4개를 돌리고 있어서, "누가 이 서버를 띄웠나"가 모호해지면 디버깅이 어려워집니다.
  
  원칙 4(실패를 기본값으로 덮지 말고 그 지점에서 드러낸다)에 걸리는 지점이기도 합니다. "서버가 없다"는 사실을 자동 기동으로 덮으면, 서버가 죽는 진짜 문제(크래시, 소켓 꼬임)를 계속 못 보게 됩니다.
  
  버튼을 두면 편의는 거의 그대로입니다. 클릭 한 번 늘어나는 대신 **무슨 일이 벌어지는지가 보입니다.**
  
  그리고 herdrm이 구분한 두 상태는 그대로 가져오는 게 좋겠습니다. **소켓 파일이 없음**(서버가 아예 안 뜸)과 **파일은 있는데 응답 없음**(죽었거나 멈춤)은 사용자가 할 일이 다릅니다. 후자는 `herdr server stop` 후 재시작이 필요할 수 있으니까요. 디자인 원칙 4(파생 상태를 보여주고 계산시키지 말 것)에 맞게 화면이 구분해서 알려줍니다.
  
  N2로 갈까요?
- recommended: N2(안내 + 실행 버튼, 자동 기동 없음)
- answer: N2로 갈까 ?
- immediate_notes:
  - decision: N2 채택.
  - reasoning: herdr 서버는 모든 에이전트 세션을 들고 있는 상태 저장 프로세스이고 사용자가 mini에서 launchd로 게이트웨이 4개를 운영 중이라 '누가 띄웠나'가 흐려지면 디버깅이 어려워진다. 자동 기동은 서버가 죽는 진짜 원인을 덮어 원칙 4에 어긋난다.
  - verified facts: herdrm은 HerdrService.swift의 LocalServer.ensureRunning으로 자동 기동하며 '소켓 파일 없음'과 '파일은 있으나 응답 없음'을 구분한다. 그 구분은 사용자가 할 일이 다르므로 가져온다.
  - source: user Q35
- needs_normalization: false

### Q36: herdrm이 별로였던 이유와 orca 체감
- decision_ids: D-39, D-09, D-13, D-40, D-41
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:38ad9da2-4945-42fc-9b71-0291f9247168
- asked: 게이트가 백그라운드로 넘어갔습니다. 심판이 도는 동안 현재 상태를 정리해 드립니다.
  
  ## 목표 한 문장
  
  > herdr 소켓 API 위에 Electron 네이티브 셸을 올려, 여러 에이전트를 굴리는 동안 터미널·산출물·브라우저를 한 화면에서 다루게 함으로써 **창 전환과 재입력으로 끊기던 집중을 없앤다.**
  
  다른 에이전트가 이 문장만 보고도 무엇을 만들지 알아야 한다는 게 기준인데, 읽어보시고 어긋나면 말씀해 주세요.
  
  ## 인터뷰 결과
  
  **질문 35개, 결정 38건, 체크포인트 4회, 열린 P0/P1 없음.**
  
  특히 값이 컸던 세 순간이 있었습니다.
  
  1. **orca 조사가 스택을 뒤집었습니다.** 브라우저 패널이 Electron `<webview>` + CDP 프록시라는 걸 확인하면서, "herdr-core 611줄을 지키려고 브라우저 상한을 포기하는 거래"였던 Tauri 선택이 무너졌습니다
  2. **성능 논거가 반대로 뒤집혔습니다.** Tauri+CEF는 엔진이 2개라 Electron보다 무겁습니다. Tauri가 가벼운 건 Chromium을 안 넣을 때뿐인데, 그건 브라우저 패널을 포기한다는 뜻이었죠
  3. **당신이 W2를 제시해서 제 추천이 뒤집혔습니다.** pet 창 문서를 다시 읽으니 드래그 아키텍처 전체가 WKWebView 타이머 스로틀링 하나 때문이었고, Electron에는 그 전제가 없었습니다
  
  헤맸던 브라우저·프로필 구간도 원문 그대로 남아 있고, Q13은 "미응답", Q14·Q15는 `route: fact`로 분류해서 결정으로 오인되지 않게 해뒀습니다.
  
  ## 남은 것 하나
  
  `agents/prd/herdr-lightweight-ide/prd.md`가 아직 `human_approval: approved`로 남아 있습니다. 이번 인터뷰가 그 PRD의 Swift·libghostty·TUI 통째 실행을 전부 뒤집었으니 실질적으로 폐기된 문서입니다. 새 PRD가 그 상태를 명시적으로 정리해야 하고, 저는 아직 건드리지 않았습니다.
  
  게이트 결과가 나오면 알려드리겠습니다. PASS면 `status: complete`로 바꾸고 `/gen-prd`로 넘기면 되고, 블록이 걸리면 그 항목만 여쭙겠습니다.
- recommended: herdrm 실패 이유를 검사 가능한 성공 조건으로 전환
- answer: 내가 우선 이걸 하는이유는 herdrm 자체를 썼는데 좀 너무 별로라서임! 
  차라리 Orca가 훨씬 경험이 나았던듯?
- immediate_notes:
  - decision: 프로젝트 동기가 herdrm 실사용 불만임을 확정했다. 실패 이유 세 가지는 키맵이 안 먹음, 파일 브라우저와 웹 브라우저 부재, 사이드바를 더 꾸미고 싶고 label plugin을 기본으로 붙이고 싶음이다.
  - reasoning: 이 셋을 성공 조건으로 뒤집어 PRD 판정에 넣는다.
  - constraints: 사용자는 orca가 herdrm보다 나은 경험이었다고 평가하면서도 '무거워서 뺐다'. Electron을 막 선택한 직후라 이 실증이 D-13의 성격을 가설에서 확인된 이탈 요인으로 바꾼다.
  - verified facts: config.toml의 [ui.sidebar.agents] rows가 참조하는 토큰이 session.snapshot의 agents[].tokens로 실제 전달되며 tokens.summary가 agent-context-labels의 산출물이다. 따라서 사이드바 커스텀과 label plugin 흡수는 herdr 계약을 읽는 것으로 해결되고 새 스키마가 필요 없다.
  - source: user Q36
- needs_normalization: false

### Q37: 메모리 예산을 인수 조건에 넣을지
- decision_ids: D-41
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:968486ec-f901-4009-9094-4dd9430d8230
- asked: 중요한 얘기입니다. 이건 기록에 꼭 들어가야 합니다.
  
  지금까지 저는 orca를 "기능 아이디어를 빌려올 참조"로 기록해뒀는데(D-09), 방금 말씀은 다릅니다. **orca가 품질 기준선**이라는 뜻이고, herdrm은 반례라는 뜻이죠. 이건 PRD의 성공 판정에 직접 들어갑니다.
  
  그리고 돌이켜보면 이 대화의 첫 질문이 그거였습니다. "herdrm에서 shortcut 설정 같은 거 다 되나?" 안 됐고, 그게 herdrm이 별로였던 이유 중 하나였겠죠.
  
  다만 "별로였다"를 그대로 두면 PRD에서 쓸 수가 없습니다. 당신 원칙 문서가 요구하는 대로 **검사 가능한 조건**으로 바꿔야 합니다.
  
  **Q36. herdrm에서 구체적으로 뭐가 별로였나요?** 짚이는 걸 골라주시면 됩니다. 여러 개여도 됩니다.
  
  제가 코드를 읽으면서 짐작한 후보들입니다.
  
  1. **키맵·플러그인이 안 먹음** - `config.toml`의 `prefix+f`도 file-viewer도 죽어 있었죠. herdrm은 앱 고정 단축키 5개가 전부였습니다
  2. **앱 안에서 분할·워크스페이스 조작이 안 됨** - herdrm은 herdr API 103개 중 18개만 씁니다. `layout.*`, `pane.split`, `worktree.*`를 아예 안 써요. 터미널 하나에 붙는 게 전부였습니다
  3. **결국 다른 앱으로 나가야 함** - 파일도 브라우저도 없어서 산출물 확인은 밖에서
  4. **사이드바가 상태를 충분히 안 보여줌** - 무엇이 나를 기다리는지 한눈에 안 들어옴
  5. **터미널 자체의 품질** - 폰트, 렌더링, 스크롤백
  
  그리고 **orca에서 특히 좋았던 게 뭐였는지**도 알려주시면 좋겠습니다. 기능 목록으로는 Design Mode와 diff 코멘트가 눈에 띄었는데, 실제로 써보신 분의 체감은 다를 수 있으니까요. 워크트리 격리인지, 브라우저인지, 에디터인지, 아니면 그냥 전반적인 만듦새인지.
  
  이걸 받으면 **"herdr-ide가 실패한 상태"를 판정 가능한 조건으로** 적어둘 수 있습니다. 예를 들어 1번이 핵심이었다면 "herdrm이 못 했던 조작 N개가 herdr-ide에서 키 하나로 된다"가 성공 조건이 되고, 3번이었다면 "하루 작업에서 다른 앱을 연 횟수 0"이 조건이 됩니다.
  
  지금 게이트가 백그라운드로 돌고 있는데, 이 답이 들어가면 로그가 바뀌어서 게이트를 다시 돌려야 합니다. 그건 정상 경로니까 신경 쓰지 않으셔도 됩니다.
- recommended: 브라우저 패널 닫힘 400MB, 열림 900MB를 인수 조건으로
- answer: 키맵이 우선 안먹었고,, 뭔가 파일 브라우저도 없고 브라우저도 안붙어있어서? 그리고 사이드바도 뭔가 더 내가 잘 꾸밀 수 있을 것 같아서(우선 label plugin 그것도 기본으로 붙이고 싶고?) 
  orca는 음.. 잘 모르겟노..뭔가 무거워서 뺀듯?
- immediate_notes:
  - decision: 제안한 수치 그대로 인수 조건에 넣는다. 초과 시 v1 미완료다.
  - reasoning: orca 이탈이 무게 때문이었으므로 감이 아니라 관측 가능한 조건으로 고정한다. ps 기반이라 자동 판정이 가능하다(D-35).
  - constraints: herdr-ide가 herdr 서버에 PTY를 맡기는 뷰어라 orca보다 가벼울 근거는 있으나 검증 대상이지 면제 사유가 아니다.
  - verified facts: 로컬 실측 Slack 808MB, VS Code 1,087MB, Notion 1,765MB.
  - source: user Q37
- needs_normalization: false

### Q38: gap-audit 게이트 8건 일괄 판정
- decision_ids: D-42, D-43, D-44, D-38, D-45, D-46, D-47, D-48, D-49, D-36
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:f74cf1f2-e364-4b5d-b638-f987ecf8b1cb
- asked: 세 가지가 다 기록에 들어갑니다. 그리고 사이드바 얘기 덕분에 좋은 걸 찾았습니다.
  
  ## 사이드바 커스텀은 이미 계약이 있습니다
  
  당신 `config.toml`에 이게 있죠.
  
  ```toml
  [ui.sidebar.agents]
  rows = [[{ token = "$status_question_new", fg = "#f9e2af", bold = true }, ...],
          [{ token = "$summary", fg = "#74c7ec", bold = true }]]
  ```
  
  그리고 방금 스냅샷을 까보니 **그 토큰들이 API로 그대로 옵니다.**
  
  ```json
  "tokens": { "agent_claude": "❋", "status_idle": "○", "elapsed": "1d",
              "sort_rank": "10", "summary": "노션 리소스 품질 기준 확정" }
  ```
  
  두 가지가 따라옵니다.
  
  **첫째, label plugin은 이미 붙어 있습니다.** 저 `summary` 값이 `agent-context-labels`가 만든 겁니다. herdr-ide는 새로 만들 게 없고 **오는 걸 렌더링만** 하면 됩니다. D-06의 "사이드바로 흡수"가 생각보다 훨씬 쌉니다.
  
  **둘째, 사이드바 커스텀을 herdr 계약으로 해결할 수 있습니다.** herdr-ide가 `[ui.sidebar.agents]`의 `rows`를 그대로 읽어 네이티브로 렌더하면, 당신이 이미 짜둔 설정이 herdr-ide에서도 먹습니다. D-28에서 "새 설정 계약은 안 만든다"고 했는데 이건 **새로 만드는 게 아니라 herdr 것을 읽는 것**이라, 에이전트 매니페스트와 같은 예외 논리로 정합합니다.
  
  즉 "사이드바를 더 잘 꾸미고 싶다"가 v1에서 됩니다. 새 스키마 없이요.
  
  ## 그런데 orca 얘기가 걸립니다
  
  "무거워서 뺐다"고 하셨죠. 이건 그냥 넘길 수 없습니다. **당신은 Electron ADE를 실제로 써보고 무게 때문에 지운 사람**이고, 우리는 방금 Electron을 골랐습니다.
  
  D-13에 메모리를 수용 비용으로 적어두긴 했지만, 그건 가설이었습니다. 이제 실증이 생겼어요. **herdr-ide도 무거우면 당신이 똑같이 지울 겁니다.**
  
  변호할 여지는 있습니다. orca는 TS만 113MB에 모바일 릴레이, 20종+ 에이전트 관리, 워크트리별 임베디드 Chromium, 쿠키 임포트까지 다 들고 있습니다. herdr-ide는 herdr 서버가 PTY를 소유하니 **뷰어에 가깝고** 범위가 훨씬 좁습니다. 하지만 "좁으니까 가볍겠지"는 검증이 아닙니다.
  
  그래서 이걸 감으로 두지 말고 **인수 조건으로 박는 걸** 제안합니다. D-35(에이전트가 검증 가능하게)에 따라 `ps`로 잴 수 있으니 자동 판정도 됩니다.
  
  **Q37. 메모리 예산을 인수 조건에 넣을까요?**
  
  실측 기준선은 이렇습니다. Slack 808MB, VS Code 1,087MB, Notion 1,765MB.
  
  제안하는 숫자입니다.
  
  | 상태 | 예산 |
  | --- | --- |
  | 브라우저 패널 닫힘, 터미널 3개 | **400MB 이하** |
  | 브라우저 패널 열림 | **900MB 이하** |
  
  넘으면 v1 미완료로 봅니다. 숫자는 제가 제안한 거라 조정하셔도 됩니다. 오히려 **"이 수치를 넘으면 나는 이 앱을 지운다"**는 선이 어디인지 당신이 아시니, 그 감각을 숫자로 주시면 그게 더 정확합니다.
  
  넣을까요? 넣는다면 저 숫자로 갈까요, 다른 선이 있으신가요?
- recommended: 8건 각각에 개별 추천 제시
- answer: ㅇㅇㅇㅇ 그렇게 넣자
- immediate_notes:
  - decision: 8건 전부 확정. (1) 생성은 workspace/worktree/tab/pane 전부 v1이고 닫기도 포함하며 'herdrm이 못 한 기본 기능은 다 앱에서'가 기준이다. (2) grab 인터랙션 상세는 뼈대만 정하고 디자인 패스로 연기. (3) working/attention pane 닫기는 확인 다이얼로그. (4) D-38(N2)을 명시 재확인. (5) 원격 타겟은 설정 파일 입력과 ~/.ssh/config 후보 제시를 둘 다 한다. (6) protocol 정수로 버전 게이팅하고 미달 시 명확히 실패. (7) 프로필 데이터는 자동 삭제하지 않고 수동 정리. (8) 스크린샷은 임시 파일 경로로 주입.
  - reasoning: 사용자가 5번에서 '설정 파일에서 입력 가능하게 할 수 있나'를 물었고 herdr-pet의 config.example.toml에 이미 검증된 [[targets]] 형식이 있어 새 설계 없이 이식으로 해결했다. 9번 답변에서 pet-app이 이미 재바인딩 가능한 전역 단축키(기본 Option+P)를 갖고 있음이 드러나 D-49로 확정했다.
  - verified facts: worktree.remove에 force 플래그, Git work tree 밖에서는 not_git_worktree 에러. snapshot이 version 0.8.0 / protocol 19 노출. web/hotkey.js에 'Alt를 누르면 macOS가 P를 π로 보고하므로 event.code를 써야 한다'는 함정이 기록됨.
  - explicit non-goals: base64 인라인 스크린샷, 버전 미달 시 기능별 부분 비활성화, 프로필 데이터 자동 삭제.
  - source: user Q38
- needs_normalization: false

### Q39: 원격 설정 형식 확인 후 일괄 기록
- decision_ids: D-45, D-36
- route: fact
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:9f64acbe-5093-4b4e-a225-68782958d0d7
- asked: 게이트 결과입니다. **BLOCKED, 9건.** 남은 클로저 라운드는 1회입니다.
  
  전부 실제 빈틈입니다. 특히 **D-38을 "N2로 갈까?"라는 되묻는 어투로 확정한 것**을 심판이 잡았는데, 다른 결정들은 "ㅇㅇㅇ", "오케이" 같은 명확한 승인이었던 것과 대비된다고 지적했습니다. 맞는 지적입니다.
  
  8건에 각각 제 추천을 붙였습니다. 훑어보시고 **"다 좋아"면 그렇게 확정하고, 바꿀 것만 짚어주세요.** 아래 문구 그대로 기록됩니다.
  
  **1. 앱 안에서 workspace·worktree·pane을 만들 수 있나?**
  지금까지는 전환·분할만 정했고 **생성**을 안 정했습니다. herdr API에 `workspace.create`, `worktree.create`, `tab.create`, `agent.start`가 다 있습니다.
  → **추천: 생성은 v1 포함(⌘N 계열), 삭제·제거는 CLI.** 만드는 건 매일 하는 동작이라 최소 클릭이어야 하고(디자인 원칙 3), 지우는 건 드물고 파괴적입니다. `worktree.remove`에 `force` 플래그가 있는 것도 그래서고요.
  
  **2. grab의 요소 선택 인터랙션**
  진입·종료, 호버, 확정, 취소, 중첩 요소 처리가 안 정해졌습니다.
  → **추천: 디자인 패스로 명시 연기.** 다만 뼈대는 지금 못 박습니다. 단축키로 진입, 호버 시 아웃라인, 클릭으로 확정, ESC로 취소, 중첩은 부모/자식 이동 키. 재검토 트리거는 "디자인 단계 착수 시점"입니다.
  
  **3. 에이전트가 돌고 있는 pane을 닫으면?**
  `pane.close`가 에이전트 프로세스를 죽이는지 뷰만 떼는지 안 정했습니다.
  → **추천: 에이전트가 working이거나 attention 상태면 확인 다이얼로그.** 무슨 일이 벌어지는지 문장으로 알리고 닫습니다(디자인 원칙 6). idle이면 그냥 닫습니다.
  
  **4. D-38 재확인**
  herdr 서버가 안 돌 때 자동 기동 없이 **안내 + 실행 버튼**, 그리고 "소켓 파일 없음"과 "응답 없음"을 구분해 표시. 이게 맞나요? 이번엔 명확히 답해주세요.
  
  **5. 원격 호스트 목록은 어디서 오나**
  D-28에서 사용자 설정 파일을 안 만들기로 했는데 호스트 목록은 어딘가 있어야 합니다.
  → **추천: `~/.ssh/config`를 읽어 후보로 제시하고, 사용자가 고른 호스트만 앱 상태에 저장.** 이건 "설정 계약"이 아니라 "앱 상태"라 D-28과 충돌하지 않습니다. 저장 위치는 Electron userData.
  
  **6. herdr 최소 버전**
  공개 배포하면 남의 herdr 버전이 다릅니다.
  → **추천: 스냅샷의 `protocol` 정수로 판정.** 실측하니 `version: 0.8.0, protocol: 19`입니다. 문자열 semver보다 정수 비교가 정확합니다. 미달이면 **기능별 우회 없이 명확한 메시지로 실패**합니다(원칙 4).
  
  **7. 브라우저 프로필 데이터 수명**
  workspace를 지우면 그 프로필의 쿠키·스토리지도 지울지 안 정했습니다.
  → **추천: 자동 삭제하지 않고 보존, 관리 화면에서 수동 삭제.** worktree는 자주 만들고 지우는데 그때마다 로그인이 날아가면 D-22(쿠키 임포트 제외)의 "처음 한 번만 로그인"이 무너집니다.
  
  **8. 스크린샷을 텍스트 채널로 어떻게 보내나**
  grab이 크롭 이미지를 보내야 하는데 `pane.send_text`는 텍스트입니다. 이게 v1 최대 기능의 검증 구멍이었습니다.
  → **추천: 임시 파일로 쓰고 경로를 텍스트로 전달.** base64를 터미널에 쏟으면 수십 KB가 프롬프트를 오염시킵니다. Claude Code도 Codex도 파일 경로를 읽습니다. 검증은 `pane.read`로 경로 주입 확인 + 그 경로에 파일이 실제로 있는지 확인입니다.
  
  **9. (P2)** pet 포커스 전환 검증 라인이 UX-05에 빠졌습니다. 이건 사람 판단이 필요 없어서 제가 추가하겠습니다. CDP로 창 포커스를 바꾸고 펫/사이드바 상태가 뒤집히는지 보는 항목입니다.
  
  ---
  
  전부 괜찮으면 "다 좋아"라고 해주시고, 특히 **3번(pane 닫기)과 7번(프로필 데이터)**은 데이터를 잃는 쪽이라 한 번 더 봐주시면 좋겠습니다.
- recommended: none
- answer: 1. 전부 생성해야지! herdrm 보면 기본 기능은 다 거기서 붙어야됨 
  2. 응 우선 그렇게
  3. ㅇㅇㅇ
  4. ㅇㅇㅇ
  5. remote 설정은 설정 파일에서 입력 가능하게 할 수 있나? 근데 추천도 좋고
  6. 음 그러네.. ㄱㄱ
  7. ㅇㅇㅇ 그렇게 가자
  8. ㅇㅇㅇ 경로 주입하는식으로
  9. 기존에 app-pet 동작 보면 이해가 잘 갈듯.. 우선 단축키로 숨기고 보이고 할수있고(현재는 option+ p로 ㅎ둠) 사이드바도 ㄱㄱ
- immediate_notes:
  - decision: 기록 수행. herdr-pet의 원격 타겟 형식과 hotkey 함정을 D-36 이식 목록에 추가했다.
  - source: user Q38 후속 (진행 턴)
- needs_normalization: false

### Q40: workspace/tab 닫기의 확인 정책 (A/B/C)
- decision_ids: D-50, D-44
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:216a5ed4-34df-485e-951c-4c9023c07b27
- asked: 게이트 2차: **9건 → 1건.** 그리고 그 1건은 제가 클로저 라운드에서 만든 구멍입니다.
  
  D-42로 `workspace.close`와 `tab.close`를 v1에 넣었는데, D-44의 확인 다이얼로그 정책은 **`pane.close`에만** 걸어뒀습니다. workspace 안에 작업 중인 pane이 여러 개면 어떻게 되는지가 비어 있습니다.
  
  심판 라운드는 소진됐습니다(2/2). 이건 사람이 정해야 하는 항목이라 제가 채울 수 없어서, 마지막 질문 하나만 드리겠습니다.
  
  **Q40. 작업 중인 에이전트가 여럿 들어있는 workspace나 tab을 닫으면?**
  
  **A. 집계 경고 하나**
  > 이 workspace를 닫으면 작업 중인 에이전트 3개가 종료됩니다.
  > · claude — 노션 리소스 품질 기준 확정 (working)
  > · codex — 테스트 수정 중 (attention)
  > · claude — 빌드 대기 (working)
  >
  > [취소] [닫기]
  
  **B. pane마다 개별 확인**
  포함된 pane 수만큼 다이얼로그가 뜹니다.
  
  **C. 조용히 닫기**
  workspace/tab 닫기는 확인 없이 진행합니다.
  
  **추천은 A입니다.**
  
  B는 pane이 5개면 클릭이 5번입니다. 디자인 원칙 3(가장 잦은 동작이 최소 클릭)에 어긋나고, 반복되면 사람이 내용을 안 읽고 누르게 됩니다. 그러면 확인 다이얼로그의 존재 이유가 사라집니다.
  
  C는 원칙 6(파괴적 동작 전에 결과를 말한다)을 정면으로 어깁니다. workspace 닫기는 pane 닫기보다 **더** 파괴적인데 확인이 없으면 일관성도 없고요.
  
  A는 **무엇을 잃는지 목록으로 보여줍니다.** 사이드바가 이미 `tokens.summary`로 각 에이전트의 요약을 갖고 있으니(D-40), 그걸 그대로 써서 "무엇을 하던 에이전트인지"까지 보여줄 수 있습니다. 디자인 원칙 7(상태를 시각으로 인코딩)에도 맞고, 새로 만들 게 없습니다.
  
  `idle`만 들어있으면 확인 없이 닫습니다. D-44와 같은 규칙입니다.
  
  A로 갈까요? 이거 하나만 정하면 인터뷰가 닫힙니다.
- recommended: A(집계 경고 하나에 종료될 에이전트 목록)
- answer: ㅇㅇㅇ 그렇게 가자 A로
- immediate_notes:
  - decision: A 채택. workspace.close와 tab.close는 포함된 pane 중 working/attention이 하나라도 있으면 집계 경고 하나를 띄우고 종료될 에이전트를 tokens.summary와 함께 나열한다. 전부 idle이면 확인 없이 닫는다.
  - reasoning: B(pane마다 개별 확인)는 클릭이 pane 수만큼 늘어 디자인 원칙 3에 어긋나고, 반복되면 내용을 읽지 않고 누르게 되어 확인의 존재 이유가 사라진다. C(조용히 닫기)는 디자인 원칙 6을 어기고 workspace 닫기가 pane 닫기보다 더 파괴적인데 확인이 없다는 비일관을 만든다.
  - verified facts: 사이드바가 이미 tokens.summary를 갖고 있으므로(D-40) '무엇을 하던 에이전트인지'를 새로 만들 것 없이 보여줄 수 있다.
  - source: user Q40 (게이트 2차 클로저 findings의 유일한 잔여 항목)
- needs_normalization: false

### Q41: 게이트 사이클 2의 8건 일괄 판정
- decision_ids: D-51, D-52, D-53, D-54, D-55, D-41, D-56, D-57, D-58
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:69b1ffd2-009b-449c-82b9-b92cee32a022
- asked: 사이클 2를 돌리고 있습니다. 결과 나오면 알려드리겠습니다.
  
  그동안 게이트 이력을 짚어보면 흐름이 이렇습니다.
  
  | 라운드 | 결과 |
  | --- | --- |
  | 사이클 1 · 전체 | BLOCKED, 9건 |
  | 사이클 1 · 클로저 | BLOCKED, 1건 (해소 8건) |
  | 사이클 2 · 전체 | 진행 중 |
  
  1건이 남았던 건 제가 클로저 라운드에서 D-42로 `workspace.close`와 `tab.close`를 새로 넣으면서 D-44의 확인 정책을 확장하지 않았기 때문입니다. **고치는 과정에서 새 구멍을 만든 것**이라, 심판을 한 번 더 도는 게 실제로 값을 했습니다.
  
  지금 문서 상태입니다.
  
  **질문 40개 · 결정 50건 · 체크포인트 6회 · UX 카드 6장 · 열린 P0/P1 없음**
  
  결정 50건 중 되돌린 게 3건 있습니다. D-05(Tauri → rejected), D-20(무응답을 동의로 처리했던 것 → 재질의), D-38(애매한 동의 → 명시 재확인). 뒤집힌 흔적을 지우지 않고 남겨뒀으니, 나중에 "왜 Electron이지?"를 물을 때 근거와 번복 이력이 같이 나옵니다.
  
  PASS가 뜨면 `status: complete`로 바꾸고 감사 이력을 기록한 뒤 `/gen-prd`로 넘길 준비를 하겠습니다.
- recommended: 8건 각각에 개별 추천 제시
- answer: 돌아가고 이성?
- immediate_notes:
  - decision: 6건 즉시 확정, 2건(grab 대상 없음, 프로필 선택 트리거)은 사용자가 뜻을 되물어 Q42~Q43에서 해소.
  - 확정된 것: diff 코멘트 배치는 v1 비목표(D-51). herdr-remote-handoff는 유지·보완재(D-52). chromux를 전제조건에 명시하고 라이선스를 추가하며 부재 시 degrade(D-53). 사이드바 기본 레이아웃을 내장하고 사용자 config가 있으면 덮어씀(D-54). 활성 workspace의 브라우저만 상주하고 전환 시 파기 후 재로드(D-55). 메모리 예산 측정 시나리오를 실사용 규모(workspace 7 / pane 11)로 재정의(D-41 갱신). 파괴적 동작 검증은 herdr-ide-verify- 접두어 fixture에서만(D-56).
  - verified facts: modakbul-gongbang/chromux는 PUBLIC이고 41 stars이나 licenseInfo가 null이라 라이선스 파일이 없다. 공개 저장소라도 라이선스가 없으면 D-29의 배포 대상자가 법적으로 사용할 수 없다.
  - reasoning: 메모리 지적이 가장 날카로웠다. 400MB를 workspace 1개·터미널 3개라는 합성 최소 조건에서 잰 예산은 실사용 규모(workspace 7 / pane 11)에서 의미가 없었다. D-55로 브라우저 상주를 1개로 묶어 예산이 규모와 무관하게 성립하도록 만들었다.
  - source: user Q41
- needs_normalization: false

### Q42: grab 대상 부재와 프로필 선택 트리거 (설명 요청)
- decision_ids: D-57, D-58
- route: mixed
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:d61807a1-b162-404a-9e2b-5e73203fc0df
- asked: 사이클 2 결과: **BLOCKED, 8건.** 새 사이클이라 전면 재검토였고 전부 새로운 지적입니다. 클로저 라운드가 1회 남았습니다.
  
  두 건은 제가 사실로 확인했습니다.
  
  **chromux는 공개되어 있습니다.** `modakbul-gongbang/chromux`, PUBLIC, 41 stars. 그런데 **라이선스 파일이 없습니다** (`licenseInfo: null`). 공개 저장소라도 라이선스가 없으면 법적으로는 전권 보유라 남이 쓸 수 없습니다. D-29에서 공개 배포를 정했는데 v1 핵심 기능이 chromux에 의존하니 이건 실제 문제입니다.
  
  나머지 7건에 추천을 붙였습니다. **"다 좋아"면 그대로 확정하고, 바꿀 것만 짚어주세요.**
  
  **1. orca의 diff 코멘트 배치**
  Q1에서 제가 "눈에 띄는 두 기능" 중 하나로 꼽아놓고 한 번도 안 닫았습니다.
  → **추천: v1 비목표로 명시.** F2는 뷰어+가벼운 편집인데 코멘트 배치는 리뷰 워크플로 전체를 요구합니다. v2 후보로 기록만 남깁니다.
  
  **2. `herdr-remote-handoff`는 어떻게 되나**
  pet-app과 herdr-pet은 은퇴를 정했는데 이건 안 정했습니다.
  → **추천: 유지, 보완재.** D-37은 "원격에 붙어서 본다"이고 handoff는 "세션과 작업환경을 통째로 옮긴다"라 다른 일입니다. 대체되지 않는다고 명시합니다.
  
  **3. grab을 눌렀는데 받을 pane이 없으면**
  포커스된 pane이 없거나 그냥 셸이면 어디로 보낼지 없습니다.
  → **추천: grab 모드 진입 자체를 막고 인라인 메시지.** 조용한 no-op은 최악입니다. 사용자가 요소를 골랐는데 아무 일도 안 일어나면 원인을 못 찾습니다(원칙 4).
  
  **4. 다른 프로필로 여는 걸 누가 트리거하나**
  D-21에 "특정 pane에서 다른 프로필로 열 수 있다"만 있고 방법이 없습니다.
  → **추천: 사람이 UI에서.** 패널의 프로필 피커로 새 탭을 엽니다. 에이전트는 D-19로 주입된 자기 기본 프로필을 쓰고 임의로 바꾸지 않습니다. 에이전트가 프로필을 갈아탈 수 있으면 "누가 어느 세션으로 뭘 했나"를 추적할 수 없습니다.
  
  **5. chromux 라이선스와 전제조건**
  → **추천: chromux에 라이선스 추가(MIT 권장) + D-29의 전제조건에 herdr와 함께 명시.** chromux가 없으면 브라우저 패널은 뜨되 에이전트 연동만 안 되는 상태로 degrade합니다. 라이선스 추가는 herdr-ide 밖의 작업이라 별도 항목으로 기록합니다.
  
  **6. 사이드바 기본 레이아웃**
  D-40이 당신 `config.toml`을 읽는데, 남들은 그 섹션이 없습니다.
  → **추천: herdr-ide가 자체 기본 레이아웃을 내장하고 `[ui.sidebar.agents]`가 있으면 덮어쓴다.** 기본값은 지금 당신 설정과 같은 모양입니다. 첫 줄에 상태 토큰 + 에이전트 아이콘 + workspace 라벨 + elapsed, 둘째 줄에 summary.
  
  **7. 메모리 예산을 어느 규모에서 재나** ← 제일 날카로운 지적입니다
  400MB를 "workspace 1개, 터미널 3개"에서 쟀는데, **당신 실제 규모는 workspace 7 / pane 11**입니다. 합성 최소 조건에서만 통과하는 예산은 의미가 없습니다.
  → **추천 둘입니다.** (a) **활성 workspace의 브라우저만 메모리에 둡니다.** 전환하면 이전 것을 파기하고 URL만 기억했다가 복귀 시 재로드합니다. 그러면 workspace가 몇 개든 브라우저는 항상 1개라 예산이 규모와 무관해집니다. 대가는 전환 시 페이지 재로드입니다. (b) **측정 시나리오를 실사용 규모로 다시 씁니다.** workspace 7 / pane 11 / 브라우저 1개 열림 기준으로 900MB, 브라우저 닫힘 기준 400MB.
  
  **8. (P2)** 파괴적 동작 검증이 당신 진짜 workspace를 지울 수 있다는 지적입니다. 일회용 테스트 fixture 규약을 정하라는 건데, 사람 판단이 필요 없어서 제가 넣겠습니다. `herdr-ide-verify-*` 접두어를 쓴 workspace/worktree에서만 파괴적 검증을 돌리고, 그 접두어가 아니면 검증 스크립트가 거부하는 식입니다.
  
  ---
  
  **7번의 (a)**만 한 번 더 봐주시면 좋겠습니다. 브라우저를 매번 재로드하는 게 거슬릴 수 있는데, 그걸 감수하면 메모리가 workspace 수와 무관해집니다. 반대로 다 살려두면 브라우저 7개가 되고 예산은 못 지킵니다.
- recommended: none
- answer: 1. ㅇㅇ 
  2. 오 이거는 우선 냅두자 ㅇㅇㅇ
  3. 받을 pane이 없다는게 뭐야?
  4. 브라우정 ㅣ야기하는거야?
  5. ㅇㅇㅇ 그렇게 하자
  6. ㅇㅇㅇ 그렇게 가자
  7. ㅇㅇㅇ
  8. ㅇㅇㅇㅇ
- immediate_notes:
  - decision: 사용자가 'grab이 정확히 뭐냐'와 '받을 pane이 없다는 게 뭐냐'를 되물어 용어 설명으로 전환했다. 제품 결정은 Q43에서 났다.
  - reasoning: 에이전트가 grab을 도메인 용어로 쓰면서 구체 상황을 제시하지 않아 생긴 혼선이다.
  - source: user Q42
- needs_normalization: false

### Q43: grab 진입 조건과 프로필 선택 주체 확정
- decision_ids: D-57, D-58
- route: user-decision
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:679ce031-cb32-4ff3-9593-75e187dce96b
- asked: 여섯 건 기록했습니다. 남은 둘을 설명드리겠습니다.
  
  ## 3번 — "받을 pane이 없다"가 무슨 뜻이냐면
  
  grab은 **브라우저 패널에서 요소를 클릭하면 그 정보가 에이전트에게 간다**는 기능이죠. 그런데 "에이전트에게"의 목적지가 **지금 포커스된 pane**입니다. 그게 없거나 엉뚱할 수 있습니다.
  
  두 경우입니다.
  
  **(가) 포커스된 pane이 아예 없음** — 앱을 막 켜서 아직 아무 터미널도 안 고른 상태
  
  **(나) 포커스된 pane이 그냥 셸** — `zsh` 프롬프트만 떠 있고 에이전트가 안 돌고 있는 pane. 여기에 HTML과 CSS를 밀어 넣으면 **셸에 그대로 타이핑됩니다.** 수백 줄짜리 HTML이 명령어로 들어가는 거죠
  
  **추천: 이 상태면 grab 모드에 아예 못 들어가게 막고 이유를 표시.** "요소를 보낼 에이전트가 없습니다. 에이전트가 실행 중인 pane을 먼저 선택하세요" 정도로요.
  
  조용히 무시하는 게 최악입니다. 요소를 힘들게 골랐는데 아무 일도 안 일어나면 원인을 못 찾습니다. `agent_status`로 에이전트 실행 여부를 알 수 있으니 판정도 됩니다.
  
  ## 4번 — 네, 브라우저 얘기 맞습니다
  
  D-21에서 이렇게 정했습니다.
  
  > workspace마다 default 프로필이 있고, **특정 pane에서 다른 프로필로 페이지를 열면** 그 탭만 별도 세션이 된다
  
  그런데 "다른 프로필로 연다"는 **행위를 누가 시작하는지**를 안 정했습니다. 두 가지가 가능합니다.
  
  **(가) 사람이 UI에서** — 브라우저 패널에 프로필 선택기를 두고, "새 탭 + 프로필 `test-account`" 식으로 엽니다
  
  **(나) 에이전트가 명령으로** — 에이전트가 chromux에 프로필 옵션을 줘서 자기가 알아서 다른 프로필로 엽니다
  
  **추천은 (가)입니다.**
  
  (나)를 열면 에이전트가 마음대로 프로필을 갈아탈 수 있습니다. 그러면 **"어느 세션으로 무슨 짓을 했나"를 추적할 수 없습니다.** 로그인된 계정이 여러 개일 때 이건 실제 사고로 이어집니다. 에이전트는 D-19로 주입된 자기 workspace 기본 프로필만 쓰고, 프로필을 바꾸는 건 사람의 명시적 행위로 둡니다.
  
  멀티 계정 테스트처럼 진짜 필요한 경우에는 당신이 UI에서 그 프로필 탭을 열어두면, 에이전트는 그 탭에 붙어서 작업합니다. 필요한 일은 다 되면서 프로필 전환 권한만 사람에게 남습니다.
  
  ---
  
  **Q42. 3번과 4번, 이 추천으로 갈까요?**
- recommended: grab은 받을 에이전트가 없으면 진입 차단 + 안내, 프로필은 사람이 선택
- answer: 저게 무슨 상황이고 좀 더 쉽게 이야기해줘.. grab이 뭔데 정확히
- immediate_notes:
  - decision: 둘 다 채택.
  - reasoning: 포커스된 pane에 에이전트가 없으면 HTML/CSS 수백 줄이 셸에 명령어처럼 타이핑된다. 조용한 no-op은 사용자가 원인을 찾을 수 없게 한다(원칙 4). 프로필을 에이전트가 갈아탈 수 있으면 '어느 로그인 세션으로 무엇을 했는지' 추적이 불가능해지고 계정이 여러 개일 때 실제 사고로 이어진다.
  - constraints: 멀티 계정 작업은 사용자가 해당 프로필 탭을 열어두면 에이전트가 그 탭에 붙어 수행하므로 기능 손실이 없다.
  - verified facts: 판정 근거인 agent_status는 session.snapshot에 이미 있다.
  - source: user Q43
- needs_normalization: false

### Q44: 사이클 2 판정 기록 및 UX 카드 보강
- decision_ids: D-57, D-58
- route: fact
- source_ref: claude:5b30f5a9-191a-4535-8ce5-7a5fdd36f671:24cf39fb-cb62-489b-9c5f-7f0ff3bb9ed7
- asked: 쉽게 설명드리겠습니다. 구체적인 상황으로요.
  
  ## grab이 뭐냐면
  
  모닥불 홈페이지 신청 페이지를 고치고 있다고 해봅시다. 에이전트한테 시켜놨고, 브라우저 패널에 그 페이지가 떠 있습니다. 보니까 신청 버튼이 좀 이상합니다.
  
  **지금(grab 없을 때)은** 에이전트한테 말로 설명해야 합니다.
  
  > "신청 페이지에 결제 금액 밑에 있는 버튼 있잖아, 파란색 그거. 그거 패딩이 좀 이상한데 봐줘"
  
  에이전트는 그 말만 듣고 코드에서 어느 버튼인지 찾아야 합니다. 헤매고, 엉뚱한 걸 고치기도 하고요.
  
  **grab이 있으면** 그냥 그 버튼을 **마우스로 클릭합니다.** 그러면 자동으로 이 셋이 에이전트한테 갑니다.
  
  - 그 버튼의 HTML (`<button class="apply-btn" ...>`)
  - 적용된 CSS (`padding: 8px 12px; background: #3b82f6` 등)
  - 그 버튼만 잘라낸 스크린샷 이미지
  
  말로 설명할 필요가 없어집니다. 손가락으로 가리키는 거죠. 당신이 처음에 말한 **"본 것을 다시 타이핑하지 않는다"**가 이겁니다.
  
  ## 그래서 3번 질문이 뭐냐면
  
  **그 정보를 어느 에이전트한테 보내죠?**
  
  답은 "지금 터미널에 떠 있는 에이전트"입니다. 보통은 문제없어요. 그 에이전트한테 시켜놓고 브라우저로 확인하던 거니까요.
  
  문제는 터미널에 **에이전트가 안 돌고 있을 때**입니다. 그냥 `zsh` 프롬프트만 있는 pane이면, HTML 수백 줄이 **명령어처럼 타이핑됩니다.** 엔터도 안 쳤는데 프롬프트가 쓰레기로 가득 차죠.
  
  → 그래서 **그럴 땐 grab을 못 누르게 막고 "에이전트가 있는 터미널을 먼저 고르세요"라고 알려주자**는 겁니다.
  
  ## 4번은
  
  브라우저 패널에서 **새 탭을 열 때 어느 로그인 계정으로 열지**를 누가 정하느냐입니다.
  
  예를 들어 모닥불 관리자 페이지를 열 때, 당신 계정으로 열지 테스트 계정으로 열지요.
  
  → 추천은 **사람이 고른다**입니다. 에이전트가 자기 마음대로 계정을 갈아탈 수 있으면, 나중에 "이 작업을 어느 계정으로 했지?"를 추적할 수 없습니다. 계정이 여러 개일 때 사고 나기 좋고요.
  
  에이전트는 당신이 열어둔 탭에서 작업합니다. 계정을 바꾸는 것만 사람 몫입니다.
  
  ---
  
  **Q42. 이 둘, 이렇게 갈까요?**
  
  - 3번: 받을 에이전트가 없으면 grab 진입을 막고 안내
  - 4번: 브라우저 탭의 계정(프로필)은 사람이 고른다
- recommended: none
- answer: ㅇㅇㅇ 그렇게 가자
- immediate_notes:
  - decision: 기록 수행. UX-03의 실패 상태에 grab 대상 부재, 프로필 전환 주체, workspace 전환 시 브라우저 파기, chromux 부재 degrade를 추가했다.
  - source: user Q43 후속 (진행 턴)
- needs_normalization: false

## UX Scenario Cards

### UX-01: 막힌 에이전트를 찾아 붙는다
- trigger: 여러 workspace에 에이전트가 돌고 있고, 그중 하나가 질문·승인·에러로 멈춰 있다.
- happy path: 사이드바 agents 목록에서 미확인(`_new`) attention 표시를 보고 클릭한다. 중앙 터미널이 그 pane의 PTY로 바뀐다. 답하면 표시가 사라진다.
- state / failure: (1) herdr 서버 미실행 - 소켓 파일 없음과 응답 없음을 구분해 표시하고 '서버 실행' 버튼을 준다. 자동 기동하지 않는다. (2) 확인된 `?`는 attention이 아니라 idle로 내려간다.
- recovery: 서버가 죽은 경우 버튼으로 재기동한다. 재기동 후 사이드바가 스냅샷을 다시 읽는다.
- proof: chromux로 herdr-ide CDP에 붙어 사이드바 항목을 클릭하고, herdr `pane.get`의 focused 상태가 그 pane으로 바뀌는지 확인한다. `_new` 판정은 이식한 순수 함수 테스트로 본다.
- linked decisions: D-06, D-16, D-31, D-34, D-35, D-38, D-40, D-42, D-46

### UX-02: 에이전트가 만든 산출물을 확인한다
- trigger: 에이전트가 이미지나 파일을 생성했고 눈으로 확인해야 한다.
- happy path: 우측 패널을 워크벤치로 두고 파일트리에서 해당 파일을 클릭한다. 이미지가 렌더되고, 마크다운·코드·diff도 같은 자리에서 보인다. 값 하나를 고쳐야 하면 그 자리에서 편집하고 저장한다.
- state / failure: (1) 포커스된 pane이 트리 루트와 다른 경로에 있으면 트리 상단에 그 사실을 표시한다. 자동으로 따라가지 않는다. (2) 편집 중 에이전트가 같은 파일을 덮어쓰면 '내 편집 유지 / 다시 읽기'를 묻는다.
- recovery: 명시적 조작으로만 pane 경로로 트리를 옮긴다. 충돌 시 사용자가 어느 쪽을 잃을지 고른다.
- proof: chromux로 트리 노드를 클릭하고 뷰어에 내용이 뜨는지 확인한다. 충돌 분기는 파일을 외부에서 수정한 뒤 다이얼로그가 뜨는지로 본다.
- linked decisions: D-25, D-26, D-27, D-02

### UX-03: 본 것을 에이전트에게 그대로 넘긴다
- trigger: 에이전트가 만든 화면을 브라우저 패널에서 보다가 특정 요소를 고치라고 지시하고 싶다.
- happy path: 우측 패널을 브라우저로 전환한다. 탭에 어느 pane이 어느 프로필로 열었는지 배지가 보인다. 요소를 선택하면 HTML·CSS·크롭 스크린샷이 포커스된 에이전트 pane에 주입된다. 터미널이 옆에 그대로 있어 결과를 즉시 본다.
- state / failure: (0) 포커스된 pane이 없거나 그 pane에 에이전트가 실행 중이 아니면 grab 모드 진입을 막고 사유를 표시한다. 맨 셸에 주입하면 HTML/CSS가 명령어처럼 타이핑된다. (0-b) 프로필 전환은 사람의 UI 행위로만 일어나고 에이전트는 workspace default 프로필만 쓴다. (0-c) workspace를 전환하면 이전 브라우저는 파기되고 복귀 시 URL과 프로필로 재로드되므로 페이지 내 미저장 상태는 유지되지 않는다. (0-d) chromux가 없으면 패널은 뜨되 에이전트 연동만 동작하지 않으며 그 사유를 표시한다. (1) 다른 에이전트가 새 페이지를 열어도 보고 있던 탭을 자동 전환하지 않고 미확인 배지만 띄운다. (2) 패널 브라우저는 herdr-ide 소유 파티션이라 처음에는 로그인이 안 되어 있다. (3) 원격 workspace에서는 브라우저 패널을 제공하지 않으며 그 이유를 표시한다.
- recovery: 로그인은 패널에서 한 번 하면 이후 유지된다. 기존 로그인이 필요한 작업은 chromux의 별도 Chrome 창에서 계속한다.
- proof: chromux로 패널의 요소 선택을 실행한 뒤 herdr `pane.read`로 대상 pane에 HTML/CSS와 스크린샷 경로가 주입됐는지 확인하고, 주입된 경로에 파일이 실제로 존재하며 이미지로 읽히는지 확인한다(D-48).
- linked decisions: D-11, D-18, D-19, D-20, D-21, D-22, D-23, D-37, D-43, D-48, D-53, D-55, D-57, D-58

### UX-04: 원격(mini) workspace에서 작업한다
- trigger: mini에서 돌고 있는 에이전트를 확인하거나 붙어야 한다.
- happy path: 사이드바에 원격 workspace가 함께 보인다. 선택하면 터미널이 원격 pane에 attach되고 분할도 동작한다. 파일트리로 원격 파일을 탐색하고 뷰어로 본다.
- state / failure: (1) 인라인 편집과 브라우저 패널은 원격에서 동작하지 않으며, 회색으로 비워두지 않고 이유를 표시한다. (2) SSH 연결이 끊기면 그 사실을 알린다. 조용히 빈 목록을 보여주지 않는다.
- recovery: 원격 파일 수정은 그 pane의 터미널에서 한다. 연결 끊김은 재연결을 시도하고 결과를 알린다.
- proof: mini에 붙어 원격 workspace가 사이드바에 뜨고 attach와 파일 나열이 되는지 확인한다. 머신 가용성에 따라 차단 가능한 항목으로 둔다.
- linked decisions: D-37, D-24, D-26

### UX-05: 창이 뒤로 갔을 때 알림을 받는다
- trigger: herdr-ide가 아닌 다른 앱을 보고 있는데 에이전트가 멈췄다.
- happy path: 데스크톱의 펫이 상태를 드러낸다(에러·질문·승인 우선). 클릭하면 herdr-ide가 앞으로 나오면서 해당 에이전트로 이동한다.
- state / failure: (1) herdr-ide 창이 앞에 있을 때는 펫이 조용히 있고 사이드바가 알린다. 같은 사실을 두 곳에서 동시에 말하지 않는다. (2) 저장된 펫 위치가 연결되지 않은 모니터를 가리키면 주 디스플레이로 복구한다.
- recovery: 오프스크린 좌표는 클램프로 복구한다. 실제 사고 좌표가 테스트에 고정되어 있다.
- proof: 클램프 기하는 이식한 순수 함수 테스트로 본다. 투명·무테·항상 위·클릭스루·드래그 조합은 v1 초반 게이팅 스파이크에서 실물로 확인한다. 역할 분리(E1)는 CDP나 IPC로 herdr-ide 창의 focus/blur를 발생시킨 뒤 펫 표시 상태와 사이드바 attention 표시가 서로 반대로 뒤집히는지 확인한다. 전역 단축키 토글은 accelerator를 발생시켜 펫과 사이드바의 표시 상태가 바뀌는지로 본다.
- linked decisions: D-07, D-31, D-32, D-33, D-34


### UX-06: workspace·worktree·pane을 앱 안에서 만들고 닫는다
- trigger: 새 작업을 시작하거나(브랜치별 worktree, 새 에이전트) 끝난 작업을 정리한다.
- happy path: 사이드바에서 새 workspace/worktree를 만들고 라벨을 준다. workspace 안에서 새 pane을 열어 에이전트를 띄운다. 끝나면 닫는다. CLI로 나가지 않는다.
- state / failure: (1) 에이전트가 working이거나 attention 상태인 pane을 닫으려 하면 확인 다이얼로그를 띄우고 그 프로세스에 무슨 일이 생기는지 문장으로 알린다. idle이면 바로 닫는다. workspace나 tab을 닫을 때는 개별 확인 대신 집계 경고 하나를 띄우고 종료될 에이전트를 tokens.summary와 함께 나열한다. 포함된 pane이 전부 idle이면 확인 없이 닫는다. (2) worktree 제거는 체크아웃을 실제로 지우므로 결과를 고지하고 확인을 받는다. (3) Git work tree가 아닌 곳에서 worktree 생성을 시도하면 herdr가 `not_git_worktree`를 돌려주므로 그 사유를 그대로 보여준다. (4) herdr protocol이 요구 버전에 미달하면 기능을 부분 비활성화하지 않고 명확한 메시지로 실패한다.
- recovery: 파괴적 동작은 확인 단계에서 취소할 수 있다. 프로필 데이터는 workspace를 지워도 남으므로 로그인은 유지된다.
- proof: chromux로 생성 조작을 실행한 뒤 herdr `workspace.list` / `worktree.list` / `pane.list`에 반영됐는지 확인한다. 확인 다이얼로그는 working 상태 pane을 닫으려 할 때 뜨는지, 그리고 working pane 2개 이상을 가진 workspace를 닫을 때 집계 경고가 그 에이전트들을 나열하는지로 본다.
- linked decisions: D-42, D-44, D-50, D-24, D-46, D-47, D-40

## Evidence From Code, Docs, Or Research

- herdr 소켓 API: `herdr api schema --json` 결과 메서드 103개. `layout.apply`/`set_split_ratio`, `pane.split`/`close`/`focus_direction`/`read`/`send_text`, `agent.prompt`/`wait`/`explain`, `plugin.list`/`enable`, `notification.show`. pane cwd는 `focused_pane_cwd`/`foreground_cwd`/`workspace_cwd`로 노출. `env` 파라미터가 `PaneSplitParams`/`TabCreateParams`/`WorkspaceCreateParams`/agent 시작 파라미터에 존재.
- herdr worktree 모델: `herdr worktree --help`가 "Manage Git worktree-backed workspaces", `WorktreeInfo`에 `open_workspace_id`. worktree와 workspace는 1:1이다.
- 사용자 herdr 현황(2026-08-25 스냅샷): workspace 7 / tab 9 / pane 11 / agent 9.
- herdrm(missuo/herdrm, Swift): `Packages/HerdrKit`(SocketRPC 7.7KB, HerdrService 27KB, SSHTunnel 22KB) + `Sources/HerdrM`. herdr API 103개 중 18개만 사용. 터미널은 `herdr agent attach <paneID> --takeover` 서브프로세스 + SwiftTerm. 서버 미실행 시 `LocalServer.ensureRunning`으로 자동 기동.
- orca(stablyai/orca, MIT, TypeScript/Electron, 53.2k stars): 데스크톱 브라우저 패널은 Electron `<webview>` 게스트, headless는 main 프로세스 offscreen WebContents(`src/main/browser/browser-backend.ts`). `cdp-ws-proxy.ts`가 `listen(0, '127.0.0.1')`로 CDP HTTP/WS를 열고 `cdp-target-discovery.ts`가 `/json/version`·`/json/list`에 `webSocketDebuggerUrl`을 응답. Design Mode의 실체는 `grab-guest-*.ts` 콘텐츠 스크립트(React 컴포넌트 식별 포함). 쿠키 임포트는 `chromium-cookie-path.ts`가 `<profileDir>/Network/Cookies`를 읽고 `browser-cookie-import.ts`가 78KB. 스크린캐스트(`browser-screencast-*.ts`)는 `Page.screencastFrameAck` 백프레셔를 쓰며 모바일 릴레이용이고 데스크톱 패널에는 쓰지 않는다.
- chromux: `open <s> --cdp-url http://127.0.0.1:PORT [--tab active|<id>|<match>]`로 외부 CDP 뷰에 attach(loopback 전용, close는 detach만). 프로필은 별도 Chrome 프로세스 단위(`ps`가 running profiles, `kill <name>`이 Chrome + daemon 종료).
- Rust CDP 경로: `cef` 크레이트는 tauri-apps 관리, v151.8.0+151.3.24, 2026-08-23 갱신, 90일 다운로드 100,355, macOS ARM64 지원. docs.rs에서 `Settings.remote_debugging_port`와 `BrowserHost::{send,execute,add}_dev_tools_*` 확인. 그러나 Tauri 결합은 `tauri-apps/cef-rs#192`가 2025-08-26 생성 후 2025-09-08 갱신을 마지막으로 OPEN, 코멘트 1개. 구현체는 별 0개 개인 포크뿐. `chromiumoxide`(90일 166만)/`headless_chrome`(81만)은 CDP 클라이언트지 임베딩이 아니다.
- 성능 로컬 실측(2026-08-25, `ps -Ao rss,comm`): Notion 1,765MB, VS Code 1,087MB, Slack 808MB, 일반 Chrome 904MB/10프로세스.
- herdr-pet 규모: `crates/herdr-core` 2,206줄(herdr 연동은 `herdr.rs` 505 + `ssh.rs` 106 = 611줄, pet 전용 `window.rs` 669), `apps/pet-app/src-tauri` 2,576줄(`main.rs` 2,111 + `macos_window.rs` 462). 프런트엔드는 프레임워크 없는 순수 JS.
- pet 창 함정의 원인(`docs/pet-window-macos.md`): 드래그 아키텍처(8ms 네이티브 틱, 별도 `pet-hit` 창)가 존재하는 이유를 "WKWebView throttles timers in unfocused windows"로 명시. 오프스크린 사고 좌표 `[542720, 163840]`가 `window.rs` 순수 함수 테스트에 고정되어 있음.
- 사이드바 계약: 사용자 `config.toml`의 `[ui.sidebar.agents] rows`가 토큰 + fg/bold 스타일을 정의하고, `session.snapshot`의 `agents[].tokens`가 그 값을 실제로 전달한다. 실측 예: `{"agent_claude":"❋","status_idle":"○","elapsed":"1d","sort_rank":"10","summary":"노션 리소스 품질 기준 확정"}`. `summary`는 agent-context-labels 플러그인의 산출물이다.
- herdr 버전 노출: `session.snapshot`이 `version: "0.8.0"`, `protocol: 19`를 준다.
- `worktree.remove`는 `force` 플래그를 갖는 파괴적 동작이고, Git work tree 밖에서는 `not_git_worktree` 에러를 돌려준다.
- herdr-pet의 원격 타겟 설정 형식(`config.example.toml`, `crates/herdr-core/src/config.rs`): `[[targets]]`에 id/label/socket_path, `[targets.ssh]`에 host/remote_socket/options. 실사용으로 검증된 형식이다.
- herdr-pet의 전역 단축키(`web/hotkey.js`): 사용자 재바인딩 가능하며 기본값 Option+P. 기록된 함정 - accelerator 캡처는 `event.key`가 아니라 `event.code`를 써야 한다. Alt를 누르면 macOS가 P를 π로 보고해 `Alt+π`가 파싱되지 않는다.
- 이전 PRD: `agents/prd/herdr-lightweight-ide/prd.md`가 `human_approval: approved`이나 소스 0줄. `agents/implement/herdr-lightweight-ide/context-notes.md`가 PW1(Xcode 전체 설치) 차단을 2026-08-17부터 기록.

## Documented Domain Checks

- docs inspected: `~/projects/herdr-pet/docs/{status-model,pet-window-macos,dev-runtime,theme-contract,ambient-signals}.md`, `~/projects/herdr-ide/agents/prd/herdr-lightweight-ide/prd.md`, `~/projects/herdr-ide/agents/implement/herdr-lightweight-ide/context-notes.md`, `~/projects/oh-my-principle/{engineering,design}/principles.md`, herdr `api schema`, chromux `--help`, stablyai/orca `src/main/browser/*`.
- canonical terms: workspace(체크아웃 하나, worktree와 1:1) / tab / pane(터미널 하나) / agent. 프로필(브라우저 세션 파티션)은 chromux의 프로필(별도 Chrome 프로세스)과 다른 층이므로 혼용하지 않는다. `_new` 접미 토큰은 '미확인'을 뜻하며 attention 승격의 유일한 근거다.
- glossary or code conflicts: (1) 사용자가 '하나의 workspace에 worktree 여러 개'를 상정했으나 herdr 모델에서는 worktree = workspace다(D-24). (2) '프로필'이 chromux와 herdr-ide에서 다른 것을 가리켜 Q11~Q16에서 혼선이 있었고 D-21에서 소유 주체를 herdr-ide로 확정해 해소했다. (3) 이전 PRD의 D-06 표현('official.browser를 워크벤치 뷰어 종류로')이 D-11/D-16/D-18과 어긋나 D-06을 갱신했다.
- concrete scenarios tested: `herdr api snapshot`으로 실제 계층과 필드 확인, `herdr worktree list`로 worktree=workspace 확인, `ps`로 Electron/Chrome 메모리 실측, `crates.io`/`docs.rs`/`gh issue`로 cef-rs 성숙도 확인, orca 소스에서 브라우저 백엔드·CDP 프록시·grab·쿠키 임포트 경로 확인.
- docs mutation: 없음. 이 인터뷰는 기존 문서를 수정하지 않았다. D-36이 herdr-pet의 문서·에셋·invariants를 herdr-ide로 이식하는 것을 v1 태스크로 지정한다.
- ADR candidate: 있음. '스택을 Electron으로 정한 이유'(D-10, D-12, D-13)는 세 번째 스택 번복이고 근거가 실측·이슈 상태에 걸려 있어 ADR로 남길 가치가 있다. '커스텀은 내부 단일 출처까지, 외부 계약은 만들지 않는다'(D-28)도 원칙 2와 8의 충돌을 해소한 판단이라 ADR 후보다.

## Checkpoint And Sweep History

### Checkpoint 1
- after_question: Q6
- normalized_entries: Q1, Q2, Q3, Q4, Q5, Q6
- register_changes: D-05(Rust/Tauri) rejected. D-09(orca 참조만), D-10(Electron+TS 확정), D-12(cef-rs/Tauri 결합 미성숙 사실), D-13(엔진 개수 기준 성능 사실), D-14(herdr-core TS 재구현), D-15(pet-app 단계적 은퇴) 추가. D-11(브라우저 패널 범위)은 P0 open.
- reopened_decisions: D-05 -> rejected, D-10으로 대체. Q2/Q3에서 잠정 선택했던 브라우저 C안은 Tauri 전제였으므로 무효, D-11로 재개방.
- highest_remaining_gap: Electron 확정이 브라우저 패널을 B안(임베디드 Chromium + loopback CDP)으로 만든다는 해석을 사용자가 아직 명시 확인하지 않았다. D-11이 확정되기 전에는 v1 스코프와 최대 태스크가 정해지지 않는다.

### Checkpoint 2
- after_question: Q16
- normalized_entries: Q7, Q8, Q9, Q10, Q11, Q12, Q13, Q14, Q15, Q16
- register_changes: D-11(브라우저 B2) resolved. D-16(L1 레이아웃), D-17(터미널 분할 기본), D-18(workspace 단위 브라우저 + 탭), D-19(workspace CDP 엔드포인트 + env 주입), D-20(에이전트 간 탭 제어 허용), D-21(herdr-ide 소유 프로필 모델), D-22(쿠키 임포트 v1 제외), D-23(chromux 비대체) 추가. 등록부 23행, 열린 P0/P1 없음.
- reopened_decisions: 없음. D-11이 Q7에서 확정되며 닫혔다.
- highest_remaining_gap: 사용자의 원래 요청 3건 중 '네이티브 파일트리'와 '커스텀 용이성'이 아직 미탐색이다. 워크벤치 편집 범위, 워크트리 활용, 검증 방식, 저장소 위치도 미결이다.

### Checkpoint 3
- after_question: Q26
- normalized_entries: Q17, Q18, Q19, Q20, Q21, Q22, Q23, Q24, Q25, Q26
- register_changes: D-20 resolved(에이전트 간 탭 제어 허용, 명시 승인). D-24(worktree=workspace 1:1 사실), D-25(파일트리 F2), D-26(트리 루트 T3), D-27(편집 충돌 C2), D-28(내부 단일 출처만, 외부 계약 없음), D-29(배포 R1), D-30(플랫폼 macOS만) 추가. 등록부 30행.
- reopened_decisions: 없음.
- highest_remaining_gap: pet 표면의 구체 형태(사이드바 어디에, 무엇을 표시, herdr-ide 포커스 여부에 따른 역할 분리)가 미결이다. 그 외 검증 방식, 저장소 위치, UX Scenario Cards 작성이 남았다.

### Checkpoint 4
- after_question: Q35
- normalized_entries: Q27, Q28, Q29, Q30, Q31, Q32, Q33, Q34, Q35
- register_changes: D-31(pet 역할 분리 E1), D-32(v1에 데스크톱 펫 흡수 W2), D-33(펫 창 게이팅 스파이크), D-34(chromux 주도 검증 + 순수함수 2건 이식), D-35(에이전트 검증 가능성을 설계 요구로 승격), D-36(저장소 S1 + 문서·테스트 이식 태스크), D-37(원격 M2 + 원격 파일트리 보기 전용), D-38(서버 미실행 시 N2) 추가. D-06을 D-11/D-16/D-18과 정합하도록 갱신, D-15를 v1 완료 시 은퇴로 갱신. UX 카드 5장, Evidence, Documented Domain Checks 작성.
- reopened_decisions: 없음. Q29에서 에이전트 추천이 W1에서 W2로 바뀌었으나 이는 조사에 따른 추천 변경이고 확정된 노드의 번복은 아니다.
- highest_remaining_gap: 재료는 모였다. 남은 것은 이전 PRD(herdr-lightweight-ide)의 폐기 처리를 새 PRD가 명시하는 것뿐이며 이는 gen-prd 단계에서 다룬다.

### Checkpoint 5
- after_question: Q39
- normalized_entries: Q36, Q37, Q38, Q39
- register_changes: 게이트 findings 9건 해소. D-42(앱 내 생성·닫기 전부), D-43(grab 인터랙션 연기 + 뼈대), D-44(pane 닫기 확인), D-45(원격 타겟 설정 형식 이식), D-46(protocol 버전 게이팅), D-47(프로필 데이터 보존), D-48(스크린샷 경로 주입), D-49(전역 단축키 토글) 추가. D-38 명시 재확인, D-36 이식 목록 확장, D-09/D-13 갱신. UX-06 신설, UX-03/UX-05 proof 보강. 등록부 49행.
- reopened_decisions: D-38이 애매한 동의('N2로 갈까 ?')로 확정됐다는 심판 지적을 받아 재질의 후 명시 승인으로 재확정.
- highest_remaining_gap: 재료는 모두 모였다. 남은 것은 이전 PRD(herdr-lightweight-ide) 폐기 처리를 새 PRD가 명시하는 것이며 gen-prd 단계에서 다룬다.

### Checkpoint 6
- after_question: Q40
- normalized_entries: Q40
- register_changes: D-50(workspace/tab 닫기 집계 경고) 추가, D-44를 D-50과 한 세트로 갱신. UX-06의 실패 상태와 proof 보강. 등록부 50행.
- reopened_decisions: 없음.
- highest_remaining_gap: 게이트 2차 클로저의 유일한 잔여 findings가 해소됐다. 남은 것은 이전 PRD 폐기 처리 명시이며 gen-prd 단계 항목이다.

### Checkpoint 7
- after_question: Q44
- normalized_entries: Q41, Q42, Q43, Q44
- register_changes: 게이트 사이클 2 findings 8건 전부 해소. D-51(diff 코멘트 비목표), D-52(handoff 유지), D-53(chromux 전제조건·라이선스·degrade), D-54(사이드바 기본 레이아웃), D-55(활성 workspace 브라우저만 상주), D-56(파괴적 검증 fixture), D-57(grab 진입 조건), D-58(프로필 선택은 사람) 추가. D-41 측정 시나리오를 실사용 규모로 갱신. UX-03 실패 상태 보강. 등록부 58행.
- reopened_decisions: 없음. 사이클 2는 사용자의 명시적 변경 요청(Q40 답변)으로 재개방된 것이다.
- highest_remaining_gap: 게이트 사이클 2 findings가 모두 해소됐다. 남은 것은 이전 PRD(herdr-lightweight-ide) 폐기 처리 명시이며 gen-prd 단계 항목이다.

## Audit History

### Audit 1
- type: gap-audit-gate
- result: fail
- missing decision_ids: 사이클 1 전체 라운드에서 9건(P1 8 + P2 1). 앱 내 생성 범위, grab 선택 인터랙션, pane 닫기 시 에이전트 처리, D-38 동의 근거, 원격 호스트 목록 출처, herdr 최소 버전, 프로필 데이터 수명, 스크린샷 전송 방식, pet 포커스 전환 검증.
- unsupported assumptions: D-38이 'N2로 갈까 ?'라는 되묻는 어투를 명시 동의로 처리했다. 다른 결정들의 'ㅇㅇㅇ'·'오케이'와 대비된다는 지적이었고 타당하여 재질의했다.
- UX or behavior gap: grab이 v1 최대 기능인데 크롭 스크린샷을 텍스트 채널(pane.send_text)로 어떻게 보내는지 미결이었고, 검증도 텍스트 주입만 확인해 스크린샷이 누락돼도 통과하는 구멍이었다.
- highest-risk blocker: 위 스크린샷 전송 방식.
- final-blocking-question: 없음. 8건을 사용자에게 일괄 질의해 전부 확정했다.
- PRD impact: D-42~D-49 신설, D-36·D-38 갱신, UX-06 신설, UX-03/UX-05 proof 보강.

### Audit 2
- type: gap-audit-gate
- result: fail
- missing decision_ids: 사이클 1 클로저에서 1건. D-42가 workspace.close/tab.close를 v1에 추가했으나 D-44의 확인 다이얼로그 정책이 pane.close에만 걸려 있었다.
- unsupported assumptions: 없음.
- UX or behavior gap: 작업 중인 pane 여러 개를 포함한 workspace를 닫을 때 개별 확인인지 집계 확인인지 조용한 종료인지 미결.
- highest-risk blocker: 위와 동일. 클로저 라운드 소진으로 사이클이 종료됐다.
- final-blocking-question: 작업 중인 에이전트가 여럿 들어있는 workspace나 tab을 닫으면 어떻게 할지. 사용자가 집계 경고(A)를 선택했다.
- PRD impact: D-50 신설, D-44 갱신. 이 발견은 클로저 라운드에서 에이전트가 D-42를 추가하며 새로 만든 구멍이었다.

### Audit 3
- type: gap-audit-gate
- result: fail
- missing decision_ids: 사용자의 명시 변경 요청으로 재개방한 사이클 2 전체 라운드에서 8건(P1 7 + P2 1). diff 코멘트 배치 스코프, herdr-remote-handoff의 운명, grab 대상 부재 처리, 프로필 선택 트리거, chromux 공개·라이선스, 사이드바 기본 레이아웃, 메모리 예산의 측정 규모, 파괴적 검증 fixture.
- unsupported assumptions: D-41의 메모리 예산이 workspace 1개·터미널 3개라는 합성 최소 조건에서만 정의돼 있었다. 사용자 실사용 규모는 workspace 7 / pane 11이므로 그 예산은 실사용에서 의미가 없었다.
- UX or behavior gap: D-21이 'pane별 다른 프로필'을 정하고도 그 전환을 누가 트리거하는지 미결이었고, grab에 받을 대상이 없는 경우가 미정의였다.
- highest-risk blocker: 메모리 예산의 측정 규모. 통과하고도 실사용에서 무거우면 사용자가 orca를 지웠던 이탈이 그대로 재현된다.
- final-blocking-question: 없음. 8건 중 6건은 일괄 확정, 2건은 용어 설명 후 확정했다.
- PRD impact: D-51~D-58 신설, D-41 측정 시나리오를 실사용 규모로 갱신, UX-03 실패 상태 보강. chromux 라이선스 부재(licenseInfo null)를 herdr-ide 밖의 선행 항목으로 기록.

### Audit 4
- type: gap-audit-gate
- result: pass
- missing decision_ids: 없음.
- unsupported assumptions: 없음.
- UX or behavior gap: 없음.
- highest-risk blocker: 없음.
- final-blocking-question: 없음.
- PRD impact: 사이클 2 클로저 PASS, sealed. qa-log를 complete로 전환한다. 이후 프런트매터와 이 섹션 외의 수정은 gate status를 STALE로 만든다.
