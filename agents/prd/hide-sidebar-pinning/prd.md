---
topic: "Projects 탭 project pin과 열린 agent를 닫는 project 제거"
status: "ready"
human_approval: "approved"  # user 2026-09-19 verbatim: 승인, /implement opus5로 가자
review_profile: "standard"
review_rationale: "사이드바 정렬·헤더·메뉴가 바뀌고 state.json에 필드 하나가 더해지는 사용자 가시 변경이며, project 제거가 열린 pane을 닫는 파괴적 동작을 새로 갖지만 확인 대화상자 뒤에 있고 디스크의 파일·저장소·세션은 건드리지 않는다. 자격 증명·외부 서비스·마이그레이션은 없다."
source_intake: "agents/interview/hide-sidebar-pinning/qa-log.md"
created_at: "2026-09-19"
updated_at: "2026-09-19"
---

# PRD: Projects 탭 project pin과 열린 agent를 닫는 project 제거

## Goal

여러 저장소를 돌리는 호연이 Projects 탭에서 자주 여는 project를 활동 순서와 무관하게 늘 맨 위에서 찾을 수 있게 한다.
지금 Projects 목록은 최근 활동 시각 내림차순뿐이라 자주 쓰는 project가 잠시 조용하면 아래로 내려가고, 7일이 지나면 `Inactive projects` fold 뒤로 사라진다.
등록된 project를 행 메뉴에서 pin하면 raised Needs You/Done 아래 `Pinned` 섹션에 활동순으로 한 번만 그려지고, 나머지는 기존 `Projects · Recent activity` 아래 오늘과 똑같이 보이며, pin은 등록(`WorkspaceRegistration`)에 저장되어 재시작 뒤에도 남는다.
같은 메뉴의 `Remove project…`는 지금처럼 pane이 열려 있다고 거부하지 않고, 열린 agent가 모두 닫힌다는 경고를 확인받은 뒤 pane을 닫고 등록만 지운다. 폴더·저장소·worktree·세션 파일은 그대로다.

## Non-goals

- checkout(worktree) pin: 특정 worktree만 자주 보게 되어 project pin으로 부족해지면 다시 연다. 그때까지 한 저장소의 worktree는 project 안에서 활동순이다.
- pane(agent) pin: pane은 Herdr 세션 소유라 앱 재시작·pane 종료로 사라지므로 영속 pin이 "이미 없는 것"을 가리킨다. 다시 열지 않는다.
- ⌘K 검색 변경: 결과 행은 선택 버튼 하나뿐이고 PROJECTS 그룹에는 접힌 project만 나오므로, pin/unpin·제거 액션을 검색에 넣지 않는다. 검색 결과 순서·표시도 그대로다.
- pin 글리프, pin한 순서 저장, 드래그 재배치, 단축키, hover 버튼: 사용자가 헤더 섹션 하나로 충분하다고 골랐다. 순서를 손으로 정하고 싶어지면 다시 연다.
- device마다 `Pinned` 섹션 반복: 사이드바에 device 헤더가 없어 헤더 없는 밴드가 목록 중간에 반복되는 모양이 된다. `Pinned` 섹션은 하나이고 안에서 device가 1차 정렬키다.
- 원격 navigation 컨텍스트(사이드바 하단 device 선택으로 원격 Hide의 목록을 볼 때)의 `Pinned` 섹션: 그 wire는 inactive fold도 싣지 않는다. 원격 wire가 pin을 실을 때 함께 연다. 로컬 목록에 든 원격 device의 등록은 pin된다.
- project 제거의 실행 취소: 제거는 등록만 지우므로 같은 폴더를 다시 등록하면 복구된다(pin은 다시 해야 한다). Undo를 만들지 않는다.
- Herdr가 `pane.close` 뒤에도 agent 프로세스를 남기는 경우의 처리: Herdr 소유 동작이다. Risks에 기록한다.
- design/principles.md 규칙 8(컨테이너): 새 컨테이너·테두리 없이 기존 헤더 행과 간격만 쓴다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | pin은 등록된 project(`WorkspaceRegistration`)에만 붙는다. checkout pin은 deferred, pane pin은 non-goal. | Q1에서 A(project+checkout) → Q6 "Project 만 우선 pin 되게 해버리자 workspace ㅏㄹ고!" (인터뷰 D-04) |
| D-02 | pin된 project는 활동순 목록에서 빠져 한 번만 그려진다(이동, 중복 아님). raised Needs You/Done 아래 `Pinned N` 헤더 섹션 하나가 pin이 1개 이상일 때만 나타나고, 기존 `Projects · Recent activity N` 헤더는 오늘처럼 항상 있으며 카운트는 pin을 뺀 수다. pin 0개면 오늘 화면과 같다. 헤더는 기존 `HideSectionLabel` 문법이고 pin 글리프는 없다. | Q2 "이도잉 나을 것 같은데", Q10 "(i)로 가자", Q14 정정 수락(`HideSidebar.swift:52`가 헤더를 이미 그림) (인터뷰 D-05, D-14) |
| D-03 | `Pinned` 안 순서는 트리와 같은 device → 최근 활동 → id 순이다. device가 1차 키라 device 경계를 넘는 정렬은 없다. device마다 섹션을 반복하는 안은 device 헤더가 없어 기각. | Q4 "활동순으로", Q5 "device별로" → Q14 (a) 수락 (인터뷰 D-07, D-08) |
| D-04 | pin된 project는 `Inactive projects N` fold에 들어가지 않는다. 그 안의 오래된 checkout은 지금처럼 `Inactive N` fold로 접힌다. | Q11 "다 OK" (인터뷰 D-05) |
| D-05 | 진입점은 project 행의 기존 `⋯` 메뉴와 새로 추가하는 우클릭 컨텍스트 메뉴(항목 동일)의 `Pin` / `Unpin`. checkout 행이 이미 우클릭 메뉴를 갖고 있어 project 행도 같은 패턴이 된다. | Q3 "추천", Q14 정정 수락 (인터뷰 D-06); design 5 |
| D-06 | Pin/Remove 항목은 등록된 project 행에만 있다. 주황색 미등록/임시 폴더 workspace 행에는 없다(저장할 registration이 없다). 로컬 목록에 든 원격 device의 등록도 pin된다. | Q11 "다 OK" (인터뷰 D-10) |
| D-07 | 저장은 `WorkspaceRegistration.pinned: bool`, `serde(default)`. `pinned`가 없는 기존 state.json은 모두 unpinned로 읽힌다. 등록을 제거하면 pin도 사라진다. pin 시각·경로 목록은 저장하지 않는다. | Q6 (인터뷰 D-09); engineering 2 |
| D-08 | 같은 값으로 다시 pin/unpin하면 변화 없음으로 끝나고 저장하지 않는다. 등록되지 않은 workspace id로 오면 `workspace.pin_unregistered` 오류로 거부한다(조용히 무시하지 않음). | engineering 4, 11 |
| D-09 | `Remove project…`는 열린 pane/agent가 있어도 가능하다. 지금의 `workspace.registration_in_use` 거부(`events.rs:1403`)와 그 안내 문구는 이 흐름으로 대체되어 삭제된다. | Q14 "그거지우면 관련 agent 올라간거 다 꺼진다고 경고만(근데 파일삭제하거나 그런 개념들은아니고)", Q15 "다 OK" (인터뷰 D-15, D-16); engineering 1 |
| D-10 | 제거 확인 대화상자: 제목 `Remove <label> from Hide?`, 본문 `Closes N panes (M running agents). The folder, repository, and worktrees stay on disk.`, 버튼 destructive `Close N panes and remove` / `Cancel`. pane이 없으면 지금 문구(`Hide will remove only its registration. …`)와 `Remove registration` 버튼 그대로. N·M은 코어 스냅샷의 수다. | Q15 "다 OK" (인터뷰 D-16); design 6, 10 |
| D-11 | 확인 후 코어는 worktree 삭제와 같은 방식으로 그 project의 모든 checkout pane에 Herdr `pane.close`를 보내고 Herdr가 닫혔다고 확인한 뒤 등록을 지운다. 시간 안에 확인이 오지 않으면 등록은 남고 `workspace.remove_failed` 오류로 알린다. 반쯤 지운 상태는 없다. 다시 시도하면 남은 pane부터 이어서 수렴한다. | Q15 "다 OK" (인터뷰 D-16); `worktree_control.rs:377`; engineering 7, 11, 14 |
| D-12 | 제거 뒤 Claude/Codex 세션 파일과 폴더·저장소·worktree는 디스크에 남는다. 같은 폴더를 다시 등록하면 unpinned 상태로 돌아온다. | Q15 "다 OK" (인터뷰 D-16) |
| D-13 | 문서: DESIGN.md "Projects and checkout context"에 `Pinned` 섹션 규칙과 제거 흐름을, docs/README.md 소유자 표에 반영한다. `design/hide.pen`의 `Screen / Sidebar / Projects` 보드를 `Pinned` 섹션으로 다시 그리고 pin 0개·pin 있음·제거 대화상자 프레임을 둔다. 보드는 구현 PR에서 코드와 함께 그린다(PRD 단계에서 그리지 않은 것은 `agent-row-identity` PRD와 같은 관행). | 저장소 규칙: 문서·캔버스는 행동과 같은 변경에서 (인터뷰 D-12) |
| D-14 | 검증: `bash scripts/verify-cargo.sh test`·`lint`, `bash scripts/verify-swift.sh test`, `node scripts/check-design-contract.mjs`. 라이브: 격리된 Herdr 서버(`HERDR_SOCKET_PATH`, 별도 HOME)에서 dev 앱을 띄워 pin 1개/0개, 모든 checkout이 비활성인 pin project, agent 2개가 든 project 제거를 스크린샷으로 남긴다. 증거는 `agents/runs/hide-sidebar-pinning/`. | Q11 "다 OK" (인터뷰 D-12); docs/PERFORMANCE_TESTING.md 격리 규칙 |
| D-15 | 전달: `agents/config.json`대로 PR 모드, `main` 기준 worktree에서 구현하고 CI(`verify`, `design-contract`)를 지켜본다. | agents/config.json `delivery.mode: pr` |
| D-16 | 원칙 intake(oh-my-principle fa5186d): engineering/principles.md 전부 읽음 - 1(`registration_in_use` 경로와 문구를 같은 변경에서 삭제), 2(bool 하나, pin 시각 없음), 4·10(미등록 pin·제거 타임아웃은 오류 배너), 7(`close_worktree_panes` 패턴·`HideSectionLabel`·기존 alert 재사용), 11(같은 값 pin은 no-op, 제거 재시도 수렴), 12(테스트는 스냅샷 순서와 저장 결과를 단언), 14(pane.close는 확인까지 기다림). design/principles.md 전부 읽음 - 3(행 메뉴 한 번으로 pin), 5(기존 헤더·메뉴·alert 문법), 6(제거는 대상과 수를 확인, 버튼이 결과를 말함), 7(구조는 헤더가 말하고 글리프 없음), 8(non-goal로 기록), 9(pin 0개·모두 pin·비활성 pin·원격·pane 없는 제거·타임아웃 상태를 B에 둠), 10(N·M은 스냅샷 값), 11(사용자가 (i)/(ii), (a)/(b)를 ASCII로 보고 골랐음), 12(긴 폴더 이름의 truncation을 실제 폭에서 확인). 15(자원 상한)는 새 자원이 없어 해당 없음. | `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 등록된 project 행을 우클릭하거나 `⋯`를 누르면 지금 항목(Refresh GitHub status, New worktree…, Remove registration) 위치에 `Pin` 또는 `Unpin`이 있고, 두 메뉴의 항목이 같다. | D-05 |
| B2 | `Pin`을 누르면 그 행이 raised 그룹 아래 새로 나타난 `Pinned 1` 헤더 밑으로 즉시 옮겨지고 아래 활동순 목록에서는 사라진다. `Projects · Recent activity` 카운트가 하나 줄고, 행의 펼침 상태·선택·`2 agents · 3m` 디테일은 그대로다. | D-02 |
| B3 | pin이 여럿이면 `Pinned` 안에서 device가 같은 것끼리 최근 활동순으로 서고, 새 agent 활동이 생기면 그 순서가 바뀐다. 로컬 등록이 원격 device 등록보다 앞선다. | D-03 |
| B4 | `Unpin`하면 행이 활동순 자리로 돌아가고, 마지막 pin을 풀면 `Pinned` 헤더가 사라져 화면이 오늘과 똑같다. | D-02 |
| B5 | 앱을 껐다 켜도 pin이 남는다. `pinned`가 없는 이전 state.json으로 켜면 모두 unpinned이고 경고가 없다. | D-07 |
| B6 | 모든 checkout이 7일 비활성·머지됨인 pin project는 `Inactive projects N`에 접히지 않고 `Pinned`에 남는다. 그 안의 오래된 worktree는 지금처럼 `Inactive N` 뒤로 접힌다. | D-04 |
| B7 | 주황색 미등록/임시 폴더 행의 메뉴에는 `Pin`도 `Remove project…`도 없다. | D-06 |
| B8 | ⌘K 검색의 결과·순서·동작은 바뀌지 않는다. | D-05 |
| B9 | 사이드바 하단에서 원격 device의 navigation으로 바꾸면 그 목록에는 `Pinned` 섹션이 없다(inactive fold와 같다). | - |
| B10 | 이미 pin된 project에 다시 `Pin`을 보내는 등 같은 값이면 화면과 state.json이 그대로다. 등록이 사라진 id로 오면 사이드바 상단 오류 배너에 `workspace.pin_unregistered` 메시지가 보인다. | D-08 |
| B11 | pane이 열린 project에서 `Remove project…`를 누르면 `Remove hide from Hide?` / `Closes 3 panes (2 running agents). The folder, repository, and worktrees stay on disk.` / `[Close 3 panes and remove]` `[Cancel]`가 뜬다. 수는 그 project의 실제 pane·실행 중 agent 수다. | D-10 |
| B12 | 확인하면 그 project의 모든 checkout pane이 닫히고, 닫힘이 확인된 뒤 행이 사이드바(Pinned에 있었으면 거기)에서 사라진다. Herdr TUI·`herdr pane list`에서도 그 pane들이 없다. 폴더·worktree·`~/.claude/projects`·Codex 세션 파일은 그대로다. | D-09, D-11, D-12 |
| B13 | pane이 없는 project는 지금과 같은 `Hide will remove only its registration. …` 문구와 `Remove registration` 버튼으로 바로 지워진다. | D-10 |
| B14 | Herdr가 시간 안에 닫힘을 확인하지 않으면 행과 등록이 남고 오류 배너가 이유를 말한다. 다시 `Remove project…`를 누르면 남은 pane부터 이어서 진행된다. | D-11 |
| B15 | 제거한 폴더를 `⇧⌘N`으로 다시 등록하면 unpinned 상태로 활동순 목록에 돌아온다. | D-12 |
| B16 | VoiceOver는 `Pinned` 헤더와 카운트, 메뉴의 `Pin`/`Unpin`/`Remove project…`를 읽고, 제거 대화상자의 본문과 버튼 라벨을 그대로 읽는다. | D-05, D-10 |
| B17 | 긴 한글·영문 폴더 이름의 pin된 행은 지금 project 행과 같은 규칙으로 이름이 먼저 폭을 차지하고 트레일링 디테일이 잘린다. | D-02 |
| B18 | 네 게이트(`verify-cargo.sh test`/`lint`, `verify-swift.sh test`, `check-design-contract.mjs`)가 통과한다. 코어 테스트는 pin 정렬·fold 면제·state.json 왕복·미등록 거부·제거의 close→확인→제거 순서와 타임아웃 시 등록 유지를, Swift 테스트는 `Pinned`/`Projects` 행 구성과 pin 0개일 때 헤더 부재, 대화상자 문구를 단언한다. | D-14 |
| B19 | 격리된 Herdr 서버의 dev 앱 스크린샷이 B2·B4·B6·B11·B12를 보이고 `agents/runs/hide-sidebar-pinning/`에 남는다. | D-14 |
| B20 | `design/hide.pen`의 `Screen / Sidebar / Projects` 보드가 `Pinned` 섹션을 포함해 다시 그려지고, pin 0개·pin 있음·제거 대화상자 프레임이 있으며 `check-pen.mjs`가 통과한다. DESIGN.md와 docs/README.md가 같은 PR에서 갱신된다. | D-13 |

## Technical structure

- `herdr-core/src/model.rs`: `WorkspaceRegistration.pinned: bool`(`serde(default)`)와 `WorkspaceSnapshot.pinned: bool`, 그리고 제거 대화상자용 `WorkspaceSnapshot.removal { pane_count, running_agent_count }`(`WorktreeDeletionGateSnapshot`과 같은 역할). 스냅샷 JSON은 `Serialize` 파생으로 그대로 실린다. FFI·Herdr 스키마·`contracts/` 변경 없음.
- `herdr-core/src/project_context.rs`: `sort_projects`의 키가 device → pinned → 활동 → id가 되고, `refresh_inactive_groups`가 pinned project를 device fold에서 제외한다. 셸은 정렬·fold 규칙을 반복하지 않는다.
- `herdr-core/src/runtime/events.rs`: 새 이벤트 `workspace_pin_set { workspace_id, pinned }`(미등록 거부, 같은 값 no-op, `persist_ui_state`는 기존 off-lock 저장 스레드). `remove_workspace`는 pane이 있으면 거부하는 대신 live worker에 그 project의 pane 목록을 넘겨 `pane.close` 후 확인을 기다리는 작업을 시작하고(`worktree_control.rs`의 `close_worktree_panes` 패턴 재사용, 락 밖), 완료 시 등록을 지우고 타임아웃 시 `workspace.remove_failed`를 세운다. `registration_in_use` 분기와 문구 삭제.
- `macos/Sources/HerdrMacOS`: `CoreBridgeSnapshot.swift`·`CoreBridgeWorkspaceSnapshot.swift`가 `pinned`·`removal`을 디코드(없으면 false/0). `SidebarPresentation.swift`의 프로젝션이 pinned 행과 나머지 행을 나눠 주고, `HideSidebar.swift`가 `Pinned` `HideSectionLabel`을 조건부로 그린 뒤 기존 `Projects · Recent activity`를 잇는다. `WorkspaceNavigatorRow`에 `.contextMenu`(`⋯`와 같은 항목)와 `Pin`/`Unpin`/`Remove project…`; `ShellRootView.swift`의 alert 문구·버튼을 `removal` 값으로 고른다. `CoreBridge`에 `setWorkspacePinned`·기존 `removeWorkspace` 사용.
- 새 의존성·마이그레이션·외부 호출 없음. state.json은 필드 추가만이라 이전 버전과 양방향 호환.

## Risks

- Herdr가 `pane.close` 뒤에도 agent 프로세스를 남길 수 있다(`docs/ARCHITECTURE.md:114`, 2026-09-06 fork에서 닫힌 pane의 프로세스가 42분 살아 있었음). 대화상자는 "pane을 닫는다"고만 말하고, 라이브 검증에서 upstream 0.9.1로 agent가 든 pane을 닫은 뒤 `ps`로 프로세스 잔존을 확인해 run 기록에 남긴다. 남으면 Herdr 이슈로 별도 보고한다.
- 확인과 닫힘 사이에 그 project에 새 pane이 생기면 Herdr 확인 목록에 없던 pane이 남은 채 등록이 지워져, 지금도 있는 주황색 미등록 workspace로 보인다. 기존 상태라 별도 처리 없음.
- `Pinned` 안 device 1차 정렬은 원격 device 등록이 많아지면 로컬 pin이 위, 원격 pin이 아래로 뭉친다. 사용자가 (a)를 알고 골랐다.
- 라이브 검증 경계: 운용 중인 Hide·Herdr는 건드리지 않고 격리 서버(`HERDR_SOCKET_PATH`, 별도 HOME, dev 빌드 번들)만 쓴다. 제거 검증은 그 격리 서버 안의 임시 폴더 project로만 한다.
- 구현 전 사용자가 해야 할 일: 없음.
