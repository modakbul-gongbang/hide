---
topic: "오른쪽 패널 Overview를 worktree 그룹 한 리스트로 재구성 (오른쪽 패널 재구성 PR 1)"
status: "ready"
human_approval: "approved"  # user 2026-09-19 verbatim: ㅇㅇ 확정, 진행해 opus 5로 띄워거
review_profile: "standard"
review_rationale: "오른쪽 패널의 사용자 가시 화면이 통째로 바뀌고 코어 스냅샷에 필드 두 개(upstream behind, worktree 생성 시각)와 이벤트 두 개가 더해지지만, 자격 증명·외부 쓰기·데이터 마이그레이션·파괴적 동작은 새로 없고 기존 worktree 삭제·pane 닫기 확인 흐름을 그대로 쓴다."
source_intake: "agents/interview/right-panel-overview/qa-log.md"
created_at: "2026-09-19"
updated_at: "2026-09-19"
---

# PRD: 오른쪽 패널 Overview를 worktree 그룹 한 리스트로 재구성

## Goal

여러 worktree에서 에이전트를 돌리는 호연이 오른쪽 패널 Overview 한 화면에서 "내 worktree들이 어떤 Git 상태이고 그 안에 누가 일하고 있나"를 읽고, 행 한 번 클릭으로 그 에이전트 pane이나 PR로 바로 가게 한다.
지금 Overview는 Tasks/Git 두 모드, 요약 3행, Needs You 바, Git ancestry 그래프, 하단 인스펙터로 나뉘어 있는데 인스펙터·Changes·그래프는 쓰지 않고 에이전트로 가려면 두 번 눌러야 한다.
이 PR은 Overview를 worktree 그룹 하나의 리스트로 바꾸고, 상단에 파생 사실 스트립을 두고, 쓰지 않는 것을 지운다. 오른쪽 패널의 세 번째 섹션 Changes를 History로 바꾸는 본문 교체는 PR 2다.

## Non-goals

- History 섹션 본문(커밋 로그, 커밋별 파일·diff): PR 2 `right-panel-history`. 이 PR은 탭 라벨만 `History`로 바꾸고 본문은 지금의 Changes 그대로 둔다. PR 2가 곧바로 뒤따르므로 라벨과 본문의 짧은 불일치를 받아들인다 (인터뷰 D-11, D-12).
- Overview 안 상태별 정렬(needs-you > done > working > seen): 왼쪽 sidebar가 이미 한다. Overview 그룹 순서는 고정이다 (인터뷰 D-14). 사용자가 Overview에서도 급한 것부터 보고 싶다고 하면 다시 연다.
- Git 쓰기 동작(브랜치 rename, push, fetch, merge): Hide가 Git을 쓰는 건 worktree 생성·삭제뿐이라는 경계를 지킨다. `behind origin` 수는 마지막 fetch 기준이며 Hide는 fetch하지 않는다 (인터뷰 D-19).
- Git ancestry 그래프의 대체 뷰: 그래프는 삭제하고 대체하지 않는다 (인터뷰 D-03). 커밋 관계가 필요해지면 PR 2 History에서 다시 연다.
- Needs You 필터·바: 행의 `!` 마크와 sidebar 그룹이 답한다 (인터뷰 D-05).
- 인스펙터(선택만 하고 pane은 안 옮기는 상태): 삭제 (인터뷰 D-06). `overview_select` 이벤트와 `inspected_checkout_path`도 같이 지운다.
- 원격 navigation 컨텍스트의 Overview: 지금처럼 "Overview is local only"만 보인다.
- design/principles.md 규칙 8(컨테이너): 새 카드·테두리 없이 섹션 헤더와 간격만 쓴다. 규칙 11: 후보 A/B를 pen 보드로 그려 사용자가 B를 골랐다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Overview는 worktree 그룹 하나의 리스트다(후보 B). 그룹 헤더가 worktree, 그 아래 행이 그 worktree의 에이전트. 두 렌즈 토글(후보 A)과 Tasks/Git 모드는 기각. | "B로 가자" (인터뷰 D-02) |
| D-02 | 행 클릭 = 그 pane으로 이동(`selectAgent`). 트레일링 `↗`는 같은 동작의 힌트. 하단 `Inspected task`/`Selected workspace` 인스펙터, 점검 선택 상태, `overview_select`·`inspected_checkout_path`는 삭제. | "에이전트 리스트에 클릭하면 바로 에이전트로 넘어가는게 좋을지도", "Inspected task … 없애도 될듯" (인터뷰 D-06) |
| D-03 | Git ancestry 그래프 삭제: `OverviewGitTree.swift`, `herdr-core/src/git_history.rs`와 `ProjectWorktreesSnapshot.history`, `HideTheme.Overview`의 그래프 토큰과 lane 색 4개, `scripts/pen-token-map.json`의 해당 항목. `cleanupHeight`만 남긴다. | "그래프는 삭제하고" (인터뷰 D-03) |
| D-04 | 그룹 헤더 오른쪽은 mono micro 한 줄: `<변경 파일 수> · <↑a ↓b> · <용량> · <PR 칩>`. `Clean`은 변경 0. `↑↓`는 base 대비, 0이면 안 그리고 behind는 warning 색. 용량은 그 worktree의 `disk.total_bytes`. PR 칩은 번들 octicon(`pullRequestIcon`: open/merged/closed/draft) + 번호, 색은 `pullRequestColor`, 상태 단어는 툴팁에만, 클릭은 `openPullRequest`. 없는 항목은 안 그린다. | "PR 잇으면 그거 링크도 연결 잘하고", "merged open 이런건 icon으로", "worktree 별로 용량을 … 저 checkout에서 보여주게" (인터뷰 D-04, D-07, D-09) |
| D-05 | 그룹 순서는 고정: primary checkout 먼저, 나머지 worktree는 생성 시각 순(오래된 것부터, 새 worktree는 맨 아래), 그 뒤 `Inactive N` 접기. 에이전트 상태·검색으로 순서가 바뀌지 않는다. 생성 시각은 `.git/worktrees/<name>` 항목의 생성 시각. 그룹 안 에이전트는 부모 아래 자식(`↳`), 같은 깊이에서는 pane 생성 순. | "그룹 정렬은 primary 있고 그냥 생성된 순으로", "둘 다 확정" (인터뷰 D-13) |
| D-06 | Inactive 접기는 sidebar와 같은 규칙(`project_context.rs` 7일·merged·closed·live 예외)과 같은 코어 상태(`inactive_checkouts.expanded`, `inactive_checkouts_toggle`)를 공유한다. Overview에서 펼치면 sidebar도 펼쳐진다. | "Inactive 접기는 sidebar와 공유" (인터뷰 D-14); engineering 7 |
| D-07 | 상단 = 프로젝트 이름 + `Project · N workspaces · M inactive` + refresh 아이콘 버튼 + stat strip 두 행. 1행 항상: `N GB on disk`(클릭 = 기존 디스크 팝오버), `⑂ N open PRs`(클릭 = 기존 GitHub 팝오버). 2행 조건부: `<base> ↓N behind origin`(클릭 없음, 툴팁), `N merged to clean up`(클릭 = 기존 cleanup 시트). 1행 두 셀은 값이 0이어도 그린다(`⑂ 0 open PRs`); 2행 셀은 0이면 안 그리고, 2행 셀이 둘 다 없으면 2행 자체가 없다. "해당 없음"(D-08)은 0과 다르다: base에 upstream이 없으면 behind 셀은 0이 아니라 없음이다. `···` 메뉴, 요약 3행, Needs You 바는 삭제. | "overview 상단에 뭐가 좀 더 있어도 좋겠는데", "아냐 그것도 넣어버려!!", "Needs you 이쪽 헤더는 우선 지워" (인터뷰 D-05, D-08) |
| D-08 | 값 자리 글리프 언어 하나: 정상 = 숫자, 측정·로딩 중 = `…`(muted), 못 읽음 = `?`(warning), 해당 없음 = 셀·칩을 안 그림. 이유와 다음 행동(Sign in, Refresh, as of, since last fetch)은 툴팁과 팝오버에만. stat 셀과 헤더 칩(files, 용량, ↑↓, PR) 모두 같은 규칙. | "텍스트로 다 표시하기보단 뭔가 더 단순하고", "둘 다 확정" (인터뷰 D-18) |
| D-09 | `behind origin`: base 브랜치에 upstream이 없으면 셀 없음; upstream은 있는데 ref를 못 읽으면 `<base> ↓?`; 수는 마지막 fetch된 upstream ref 기준(`FETCH_HEAD` 시각을 툴팁에). GitHub: 로딩 `⑂ …`, 로그인 필요·실패 `⑂ ?`(팝오버 첫 줄이 `githubLabel`의 이유), stale은 수 유지 + 팝오버 `as of`; 헤더 PR 칩은 마지막으로 알던 PR 유지·클릭 가능. | 인터뷰 D-19 |
| D-10 | worktree 헤더 우클릭 메뉴 = sidebar checkout 메뉴(`WorktreeMenuPolicy`: New worktree…, Set as base branch, Copy Path, Open in ▸ Finder/Default editor, Delete worktree…) + `New agent here ▸`, `Open pull request #N`(PR 있을 때), `Open in History`. 헤더 클릭 = `selectCheckout`. 헤더의 `N files` 칩 클릭 = 그 checkout을 열고 History 섹션으로. 에이전트 없는 그룹은 `No agent · Start agent…` 한 행(= `New agent here ▸`). 에이전트 행 우클릭 메뉴 = `Open pane`, `Reveal in sidebar`, `Copy pane id`, `Close pane…`(pane 헤더의 `closePaneFromHeader`와 같은 확인 흐름). | "UI 표시 좀 잘해봐", 최종 보드 승인 "ㅇㅇ 확정" (인터뷰 D-10) |
| D-11 | 인터뷰 D-10의 "New Agent 시트"는 쉘에 없다. 승인된 보드의 `New agent here…`가 뜻하는 것은 "provider를 고르고 그 worktree를 cwd로 시작"이고, 쉘이 provider를 고르는 기존 자리는 New worktree 시트의 `Start with` 피커(`Terminal only` / `Claude` / `Codex`)다. 새 시트를 만드는 대신 같은 세 항목을 하위 메뉴 `New agent here ▸`(`Terminal only` / `Claude` / `Codex`)로 두고, 고르면 그 checkout의 Herdr workspace에 cwd가 그 경로인 새 탭을 만들고 provider를 시작한다(`startAgentInCreatedPane` 경로 재사용, 코어 이벤트 하나). `Start agent…` 행도 같은 하위 메뉴. | Spec gate F1에 사용자 답 "1a" (하위 메뉴 선택, "1a 2a 진행해"); 조사: `WorktreeCreationSheet.swift:43-50`, `ShellModel.swift:1788-1798` |
| D-12 | "checkout 열고 History로"는 이벤트 하나(`overview_open_section { checkout_path, section }`)로 checkout 포커스와 섹션 전환을 함께 한다. 지금의 `overview_changes`(포커스된 checkout일 때만 동작)를 이 이벤트가 대체한다. | AGENTS.md "A user action is one event"; 인터뷰 D-10 |
| D-13 | 검색: 에이전트 이름·task·브랜치 매치, 매치된 행의 그룹 헤더 유지, 순서 유지. 매치 없음은 `No matching agents or workspaces` + `Clear search`. `HideSearchField`와 `overview-search` 식별자 유지. | 인터뷰 D-14; design 9 |
| D-14 | 상태: loading(첫 스냅샷 전), remote(로컬 전용), 워크스페이스 없음, 폴더 프로젝트(Git 아님: 용량만, files·↑↓·PR 없음), Herdr 미연결(마지막 에이전트 + 경고 라벨, 행 클릭은 동작), 에이전트 0(그룹마다 `Start agent…`), 검색 결과 없음. `Screen / Overview / States` 보드가 그린 대로. | 최종 보드 승인 (인터뷰 D-13, D-17); design 9, 13 |
| D-15 | 코어: `WorktreeSnapshot`에 `behind_upstream: Option<u32>`(upstream 없음·못 읽음 구분은 기존 `upstream_state`)와 `created_at_unix_ms: Option<u64>`를 더한다. 둘 다 기존 worktree reader의 같은 pass(변경 감지 시 백그라운드 스레드, 뮤텍스 밖)에서 읽고, git 프로세스는 하나도 늘지 않는다: 기존 `upstream()`의 `rev-list --count @{u}..HEAD`를 `rev-list --left-right --count @{u}...HEAD` 하나로 바꿔 unpushed와 behind를 같은 출력에서 얻고(`ahead_behind`가 base에 이미 쓰는 형태), 생성 시각은 `.git/worktrees/<name>` 메타데이터의 stat이다. 인터뷰 D-16이 허용한 두 경로 중 "기존 worktree reader cadence"를 택한다: `git_dir.rs`는 ref만 읽고 커밋 조상은 세지 못한다. per-tick·per-tab git 포크 없음, worktree당 프로세스 수 불변. `git_history.rs` 읽기와 `history` 필드는 삭제. | Spec gate F2에 사용자 답 "2a" (기존 rev-list 출력 형태 변경, 프로세스 증가 0, "1a 2a 진행해"); AGENTS.md Performance Guide; 조사 `worktrees.rs:502-607, 758-796`; 인터뷰 D-16 |
| D-16 | 디자인·문서: `design/hide.pen`의 `Screen / Panel / Overview`, `Screen / Overview / States`, `Screen / Overview / Actions`, `Screen / Panel / History`, `Screen / History / States and actions`가 목표 보드(생성 스크립트 `agents/runs/right-panel-overview/design/final_boards.py`로 worktree에 적용, 옛 `Project task forest`·`Inspect without moving`·`Availability states`·`Panel / Changes` 보드 삭제, Explorer 보드 탭 라벨만 변경). DESIGN.md 1182-1265의 Overview·Git Tree·인스펙터·토큰 문단을 이 PRD대로 다시 쓰고, `docs/README.md` 소유자 표를 맞춘다. | "최종 pen 디자인 그려봐~", "ㅇㅇ 확정" (인터뷰 D-13, D-17) |
| D-17 | 검증: `bash scripts/verify-cargo.sh test`·`lint`, `bash scripts/verify-swift.sh test`, `node scripts/check-design-contract.mjs`. 라이브: 격리 Herdr 서버(`HERDR_SOCKET_PATH`, 별도 HOME, dev 빌드)에서 worktree 2개 이상·위임 자식 1개·PR 있는 브랜치로 B3·B6·B9·B12·B15를 스크린샷. 증거는 `agents/runs/right-panel-overview/`. | 인터뷰 UX-01..03; docs/PERFORMANCE_TESTING.md 격리 규칙 |
| D-18 | 전달: `agents/config.json`대로 `main` 기준 worktree에서 PR, CI(`verify`, `design-contract`) 감시. 구현은 Claude Opus 5 Implementor 하나를 이 세션이 디스패치. | "please로 해서 작업 opus5로 시켜버리자" (인터뷰 D-12) |
| D-19 | 원칙 intake(oh-my-principle fa5186d): engineering 전부 읽음 - 1(그래프·인스펙터·요약 행·`overview_select`·`overview_changes`·history 읽기를 같은 변경에서 삭제), 2(필드 둘, 이벤트 둘), 4·10(못 읽은 값은 `?`와 이유, 조용한 0 없음), 7(`WorktreeMenuPolicy`·팝오버·cleanup 시트·`pullRequestIcon`·inactive 상태 재사용), 12(테스트는 헤더 문자열·순서·숨김 규칙을 밖에서 정한 답으로 단언), 14·15(reader 스레드는 기존 소유·상한 그대로). design 전부 읽음 - 1(리스트는 읽기 뷰), 3(행 클릭 한 번), 4(스트립은 파생 상태), 5(sidebar 메뉴·팝오버 문법), 7(PR 상태는 아이콘, 글리프 3개), 8(non-goal), 9·13(상태 보드, 조용한 셀 숨김), 10(스냅샷 값만), 11(A/B 보드로 선택), 12(긴 브랜치 이름 truncation을 실제 폭에서 확인). | `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 오른쪽 패널 탭이 `Overview · Explorer · History` 순이고, History 탭의 본문은 지금의 Changes와 같다. 저장된 `changes` 선택은 그대로 그 탭으로 돌아온다. | 비목표(History), D-16 |
| B2 | Overview 상단에 프로젝트 이름, `Project · 4 workspaces · 2 inactive`, refresh 아이콘, 그 아래 `25 GB on disk`·`⑂ 2 open PRs`가 보이고(열린 PR이 없으면 `⑂ 0 open PRs`), Tasks/Git 토글·GitHub·Allocated on disk·Clean up 행·`? Needs You` 바는 없다. | D-07 |
| B3 | base 브랜치가 upstream보다 뒤면 2행에 `main ↓3 behind origin`이 warning 색으로 보이고, 정리할 merged worktree가 있으면 `2 merged to clean up`이 보인다. 둘 다 0이면 2행이 없다. `merged to clean up` 클릭은 지금의 cleanup 시트를 연다. | D-07, D-09 |
| B4 | `N GB on disk` 클릭은 worktree별·Shared Git data 행이 있는 기존 디스크 팝오버, `⑂ N open PRs` 클릭은 기존 GitHub 팝오버를 연다. 측정 중이면 `… GB`, 못 읽으면 `? GB`(warning)와 툴팁 이유. | D-07, D-08 |
| B5 | GitHub 로딩 중이면 `⑂ …`, 로그인 필요·실패면 `⑂ ?`(warning)이고 팝오버 첫 줄이 이유(`Sign in required` 등)다. stale이면 수는 남고 팝오버에 `as of <time>`. | D-08, D-09 |
| B6 | 리스트는 primary worktree가 맨 위, 나머지가 생성 시각 순, 맨 아래 `› Inactive N`. 에이전트가 일하거나 끝나도, 검색해도 그룹 순서가 바뀌지 않는다. | D-05 |
| B7 | 그룹 헤더는 `▾ ⎇ prd/hide-orchestrator` 왼쪽, 오른쪽에 `12 files · ↑3 · 2.4 GB · ⑂#107`. 변경 0이면 `Clean`, `↑↓`는 0인 쪽을 생략하고 behind는 warning 색, PR 없으면 칩 없음, 용량 측정 중 `…`, 못 읽음 `? GB`. 긴 브랜치 이름은 왼쪽이 잘리고 오른쪽 문자열은 온전하다. | D-04, D-08 |
| B8 | PR 칩은 open/merged/closed/draft를 번들 octicon과 `pullRequestColor` 색으로 구분하고, 툴팁이 `#107 · open`처럼 상태 단어를 말하며, 클릭하면 브라우저에서 PR이 열린다. | D-04 |
| B9 | 그룹 안 에이전트 행은 마크(`!`·`●`·`✓`·`○`)·배지·이름·task·트레일링 `↗`이고, 위임된 자식은 부모 아래 `↳`로 들여쓴다. 행을 클릭하면 터미널 포커스가 그 pane으로 옮겨진다. | D-02 |
| B10 | 다른 worktree로 위임된 자식은 자기 worktree 그룹 밑에 `↳ from <부모 이름> · <부모 브랜치>` 캡션과 함께 보인다. 부모를 모르는 에이전트는 자기 그룹의 루트 행으로 남는다. | D-05 |
| B11 | 그룹 헤더 클릭은 그 workspace를 연다. 헤더의 `12 files` 칩은 호버 시 wash와 `Open in History` 툴팁, 클릭하면 그 checkout이 포커스되고 오른쪽 패널이 History 섹션으로 바뀐다(다른 checkout이어도 한 번에). | D-10, D-12 |
| B12 | 헤더 우클릭 메뉴: `New agent here ▸`(Terminal only / Claude / Codex), `New worktree…`, `Set as base branch`, 구분선, `Open pull request #107`(PR 있을 때), `Open in History`, 구분선, `Copy Path`, `Open in ▸`, 구분선, `Delete worktree…`(sidebar와 같은 gate·문구). | D-10, D-11 |
| B13 | `New agent here ▸ Claude`를 고르면 그 checkout의 workspace에 cwd가 그 경로인 새 탭이 생기고 Claude가 시작되며, 새 에이전트 행이 그 그룹에 나타난다. 에이전트가 없는 그룹의 `No agent · Start agent…` 행도 같은 하위 메뉴다. | D-11 |
| B14 | 에이전트 행 우클릭 메뉴: `Open pane`, `Reveal in sidebar`, 구분선, `Copy pane id`, 구분선, destructive `Close pane…`. `Close pane…`은 pane 헤더 닫기와 같은 확인(작업 중 상태 요약)을 거친다. | D-10 |
| B15 | 검색 필드에 입력하면 에이전트 이름·task·브랜치가 맞는 행과 그 그룹 헤더만 남고 순서는 그대로다. 맞는 게 없으면 `No matching agents or workspaces`와 `Clear search`. | D-13 |
| B16 | Git이 아닌 폴더 프로젝트는 상단에 `412 MB on disk`만, 그룹 헤더에 브랜치 아이콘·files·↑↓·PR 없이 용량만 보인다. | D-14, D-08 |
| B17 | Herdr 연결이 끊기면 `⚡ Live task status unavailable; showing the last known agents` 라벨이 검색 위에 뜨고 마지막 에이전트 행이 남으며 행 클릭은 동작한다. 첫 스냅샷 전은 `Loading Overview`, 원격은 `Overview is local only`, 워크스페이스 없음은 `No workspace`. | D-14 |
| B18 | 프로젝트에 에이전트가 하나도 없으면 각 그룹 헤더 아래 `No agent · Start agent…` 한 행만 있다. | D-14, D-11 |
| B19 | `Inactive N`을 Overview에서 펼치면 sidebar의 같은 접기도 펼쳐지고, 접힌 checkout은 헤더 한 줄(`›`)로 보인다. | D-06 |
| B20 | Overview에서 pane·workspace를 옮기는 것은 행 클릭, 헤더 클릭, files 칩, 메뉴의 `Open pane`뿐이다. 스크롤·검색·접기·refresh는 터미널 포커스와 read 상태를 바꾸지 않는다. | D-02 |
| B21 | VoiceOver는 그룹 헤더를 `prd/hide-orchestrator, 12 changed files, 3 ahead, 2.4 GB, pull request 107 open`처럼, 에이전트 행을 `Open sasu, Working, prd/hide-orchestrator`처럼 읽고, stat 셀과 칩의 툴팁 문구가 accessibility help와 같다. | D-04, D-08 |
| B22 | 네 게이트가 통과한다. Rust 테스트는 `behind_upstream`(upstream 없음·gone·못 읽음·N)과 `created_at_unix_ms`, history 필드·`overview_select` 부재를, Swift 테스트는 헤더 문자열(0·해당 없음 숨김 규칙 포함), 고정 순서, 검색, stat 셀의 `…`/`?`/1행 0 표시/2행 0 숨김/해당 없음 숨김, 메뉴 항목 목록을 밖에서 정한 답으로 단언한다. `OverviewGraph`·`ProjectTaskForestPresentation`·`overview-view-mode` 테스트는 삭제된다. | D-15, D-17 |
| B23 | 격리 Herdr 서버 위 dev 앱 스크린샷이 B3·B6·B7·B9·B12·B15·B17을 보이고 `agents/runs/right-panel-overview/`에 남는다. | D-17 |
| B24 | `design/hide.pen`이 D-17의 다섯 보드를 담고 옛 Overview·Changes 보드가 없으며 `check-pen.mjs`가 통과한다. DESIGN.md의 Overview 계약 문단이 이 리스트·스트립·글리프 규칙으로 바뀌고 Git Tree·인스펙터·lane 팔레트 문단은 사라진다. | D-16 |

## Technical structure

- `herdr-core/src/worktrees.rs`: `describe`가 `behind_upstream`(기존 `upstream()`의 rev-list를 `--left-right --count @{u}...HEAD`로 바꿔 unpushed와 함께 읽음, upstream 없음·gone은 `None`)과 `created_at_unix_ms`(`<common_dir>/worktrees/<name>` 생성 시각, primary는 `None`)를 채운다. 같은 백그라운드 pass, 뮤텍스 밖. `git_history.rs`, `ProjectWorktreesSnapshot.history`, `worktrees.rs:95` 읽기 삭제.
- `herdr-core/src/model.rs`: `WorktreeSnapshot`에 두 필드 추가(`serde(default)`), `CheckoutCardSnapshot.inspected_checkout_path` 삭제. `RightPanelSection`은 PR 2에서 바뀌므로 이 PR은 enum을 두고 Swift `title`만 `History`.
- `herdr-core/src/runtime/events.rs`·`runtime/projects.rs`·`runtime.rs`: `OverviewSelect`·`overview_selection`·`OverviewChanges` 삭제; `overview_open_section { checkout_path, section }`(checkout 포커스 + 섹션 전환 + `persist_ui_state`, 모르는 경로는 `overview.unknown_checkout` 오류)와 `agent_start_in_checkout { checkout_path, provider }`(그 checkout의 workspace에 `cwd` 탭 생성 후 기존 agent 시작 경로) 추가. `inactive_checkouts_toggle`은 그대로 공유.
- `macos/Sources/HerdrMacOS`: `CheckoutOverview.swift` 재작성(스트립·그룹·행·메뉴), `OverviewGitTree.swift` 삭제, `OverviewPresentation.swift`에 헤더 문자열·순서·글리프·검색 규칙을 순수 함수로(테스트 대상), `WorktreeTasks.swift`의 `WorktreeMenuPolicy`에 Overview 헤더 항목 추가, `HideTheme.swift`의 `Overview` 그래프 토큰·lane 색 삭제와 `pen-token-map.json` 동기화, `CoreBridgeSnapshot.swift`·`GitWorktreesPresentation.swift`에 새 필드 디코드, `RightPanelSection.title`의 `Changes` → `History`.
- 새 의존성·마이그레이션·외부 호출 없음. 스냅샷 JSON은 필드 추가·삭제뿐이며 저장된 `ui_state`는 그대로 읽힌다.

## Risks

- behind 수는 기존 upstream rev-list의 출력 형태만 바꿔 얻으므로 worktree당 프로세스 수는 그대로다. 그래도 구현 보고에 worktree 6개 프로젝트의 reader pass 시간을 idle/driven으로 기록해 변화가 없음을 보인다.
- `.git/worktrees/<name>` 생성 시각은 파일시스템 birthtime이다. 복사·복원된 저장소는 순서가 흐트러질 수 있다. 그때는 이름순으로 떨어지고, 사용자가 순서를 손으로 정하고 싶다면 별도 PRD.
- "New agent here"는 쉘에 시트가 없어 New worktree 시트의 `Start with` 항목을 그대로 하위 메뉴로 확정했다(D-11, design 3·5).
- History 탭 라벨과 Changes 본문의 불일치는 PR 2 머지까지만이다.
- 라이브 검증 경계: 운용 중인 Hide·Herdr는 건드리지 않고 격리 서버(`HERDR_SOCKET_PATH`, 별도 HOME, dev 번들)만 쓴다. GitHub 팝오버는 `gh` 로그인 상태에 따라 `⑂ ?`가 정상 결과일 수 있으니 그 경우 그대로 기록한다.
- 구현 전 사용자가 해야 할 일: 없음.
