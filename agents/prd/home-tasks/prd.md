---
topic: "Project Home 조용한 판: Tasks/Agents 두 뷰 보드와 GitHub 이슈 칩"
status: "ready"
human_approval: "approved"  # user 2026-09-20 verbatim: 셋 다 그대로 가자 승인 + 그리고 이거 herdr astra high로 해서 implement 확실하게 시키고 작업 부탁해~ 나 자러간다!
review_profile: "standard"
review_rationale: "새 화면 하나와 gh 읽기 필드 추가, 그리고 사람이 직접 연결한 이슈를 purpose와 같은 경로로 저장하는 변경이며, GitHub 쓰기·자격 증명·프로덕션 데이터 변경은 없다."
source_intake: "current conversation"
created_at: "2026-09-21"
updated_at: "2026-09-21"
---

# PRD: Project Home 조용한 판: Tasks/Agents 두 뷰 보드와 GitHub 이슈 칩

## Goal

hide로 한 프로젝트에서 여러 워크트리와 여러 에이전트를 동시에 굴리는 운영자가, 체크아웃에 pane이 없을 때 보는 "No terminal open" 빈 상태와 ⇧⌘H 오버레이 자리에서 **프로젝트 한 장의 상황판**을 본다.
카드는 언제나 같은 부품이고, Tasks 뷰는 그 카드를 체크아웃 단위로 git 전달 단계(준비 → 작업 중 → 리뷰 → 머지됨)에 세우며, Agents 뷰는 같은 카드를 요청 단위로 생애(진행 중 → 내 확인 대기 → 끝)에 세운다.
각 카드는 GitHub 이슈와 연결되면 칩 하나로 그 이슈를 보여주고, 연결은 hide가 이미 읽는 자리(pane 토큰, 브랜치 설정, PR 본문, 브랜치 이름)에서 저절로 일어난다.
hide는 GitHub를 읽기만 하며, task-factory 같은 오케스트레이터가 무엇인지 모른 채 그 결과를 같은 화면에서 본다.
2026-09-19의 두 가설(그래프 PR #111, 레인 보드 PR #112)을 비교한 뒤 사용자가 칸반형 "조용한 판"을 골랐고, Pen 스크래치(`agents/runs/home-tasks/design/scratch.pen`, 시트 "Tasks view · 조용한 판", "Agents view", "이슈 연결 순간")가 승인된 그림이다.

## Non-goals

- GitHub에 쓰는 동작(이슈 생성, 상태 이동, 댓글, Project 편집). 운영자는 브라우저에서 한다. 재검토 조건: 사용자가 hide에서 직접 상태를 옮기고 싶다고 말할 때, 그때는 오케스트레이터와의 소유권 경계를 다시 정한다.
- 스케줄링, 이슈 claim, 워크트리 생성, 준비 열의 "Start" 버튼. 오케스트레이터의 일이다. 재검토 조건: 오케스트레이터 없이 hide만으로 이슈에서 워크트리를 만들고 싶을 때.
- Linear 등 외부 도구 직접 읽기. hide가 자격 증명을 처음 저장하는 결정이라 별도 PRD다. Linear의 GitHub 연동이 브랜치 이름 `HOY-42-…`로 이슈를 잇고 PR 상태를 흘리므로 GitHub만 읽어도 어긋나지 않는다.
- `agents/prd/<slug>/prd.md` frontmatter 읽기. 코어의 purpose 체인(워크스페이스 토큰 → 브랜치 설명 → 대표 에이전트 제목 → PR 제목)이 이미 카드 제목을 주고, 이슈는 아래 D-05의 네 신호로 충분하다. 재검토 조건: PRD를 복사하는 도구가 토큰이나 브랜치 설정을 못 쓰는 경우가 실제로 생길 때.
- 프로젝트 클릭 시 기본 화면, hide 앱 기본 뷰로의 승격. 이 보드 위에 얹는 다음 층이다. 재검토 조건: 이 보드가 쓰이고 나서 사용자가 진입점을 다시 그릴 때.
- 폴링과 hide 자체 타이머. GitHub 읽기는 기존 리더의 세대(generation) 변화에만 반응하고, "끝" 열의 카드는 시간이 아니라 pane 종료·워크트리 제거로 사라진다.
- PR #111의 그래프 뷰, PR #112의 인스펙터 열·Needs You 스트립·Sort·5칸 트랙. 조용한 판이 대체한다. 두 PR은 이 PRD의 PR이 열릴 때 닫는다.
- 네이티브 `.help()` 툴팁, 새 색·간격·반경 값. `DESIGN.md`와 `HideTheme` 규칙 그대로다. (design/principles.md 5, 7)

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Project Home은 "조용한 판"이다: 카드 3줄, 현재 단계 한 칸, 열과 같은 말은 쓰지 않는다. 원래 판(5칸 트랙, 상태 칩, 열 부제)은 기각. | 사용자: "조용한 판 괜찮은데", "좋아 조용한 판으로 가자" |
| D-02 | 보드는 태스크 칸반에 에이전트 상태를 결합한 것이다. 열은 git·코어 사실에서 나오고 사람이 카드를 옮기지 않는다. | 사용자: "원래 태스크 칸반인데 에이전트의 상태와 결합된 느낌으로 가는 게 가장 직관적이며넛 좋은 것 가틍넫" |
| D-03 | 같은 카드를 두 뷰로 다시 묶는다. Tasks 뷰(카드 = 체크아웃, 열 = 전달 단계)가 기본, Agents 뷰(카드 = 요청, 열 = 생애)가 토글. 뷰 선택은 세션 로컬이다. | 사용자: "이 Task Pane? 기준으로도 볼 수 있나...? 그니까 View 가 두개로 할 수도 있나 해서". 기본 뷰가 Tasks인 것과 Agents 뷰 포함은 가정: 사용자가 고른 그림이 Tasks 시트이고 Agents 시트는 같은 세션에 그려져 거부되지 않았다. 되돌릴 수 있는 선택이라 리뷰에서 뺄 수 있다. |
| D-04 | hide는 GitHub Issue와 Project 카드만 읽고, 어떤 오케스트레이터도 알지 못한다. task-factory는 GitHub에 쓰는 여러 writer 중 하나다. | 사용자: "task-factory를 hide가 알 필요는 없어. 그냥 task-factory 를 외부에서 사용할 수 잇는데 이걸 리드하는건 여기서도 볼 수 잇는거지" |
| D-05 | 워크트리·pane ↔ 이슈 연결은 먼저 찾는 신호 하나로: (1) pane 토큰 `issue=owner/repo#N`, (2) git 설정 `branch.<name>.issue`(purpose의 브랜치 설명 미러와 같은 자리), (3) PR 본문의 closing reference(`gh pr list --json closingIssuesReferences`), (4) 브랜치 이름 `N-…`·`ABC-N-…` 패턴. 아무것도 없으면 카드 메뉴 "이슈 연결…". | 사용자: "그거로 issue랑도 매핑이 가능하면 베스트긴 하겟노"; 신호 순서는 가정(확실한 것부터). `closingIssuesReferences`는 gh 2.76.2에서 확인. |
| D-06 | 열은 git 사실이 정한다. 이슈의 열림/닫힘과 Project Status는 카드의 칩이고, Project Status가 있고 열과 다를 때만 `≠ <Status>` 칩을 더한다. hide는 절대 옮기지 않는다. | 사용자가 승인한 "조용한 판"과 "이슈 연결 순간" 시트 4번 칸; 가정: 두 보드가 어긋날 때 로컬 git이 진실이다. |
| D-07 | 워크트리 없는 열린 이슈는 준비 열의 회색 한 줄 카드(백로그)다. 기존 gh 리더에 `gh issue list`를 얹어 읽는다. | 가정: 승인된 Tasks 시트의 준비 열에 그려져 있다. 새 읽기 하나라 리뷰에서 뺄 수 있다. |
| D-08 | 이슈가 연결된 카드가 Needs You일 때 첫 액션은 "이슈 열기"(브라우저), 둘째가 pane 포커스. 이슈 없는 카드는 pane 포커스가 첫 액션. | 가정: 오케스트레이터는 이슈 댓글만 답으로 세므로 pane에 직접 답하면 보드가 멈춘다. 되돌릴 수 있다. |
| D-09 | 질문·오류·경과된 위임(stall)은 열을 옮기지 않고 카드 후광으로 뜬다. 위임 자식은 카드 안에 왼쪽 선으로 들여쓴다. | 사용자가 승인한 두 시트; `docs/status-model.md`의 그룹 규칙과 소유권 축을 그대로 쓴다. |
| D-10 | 카드 제목은 코어의 checkout purpose(토큰 → 브랜치 설명 → 대표 에이전트 제목 → PR 제목)이고, 이슈가 연결되면 이슈 제목이 앞선다. Agents 뷰 카드의 제목은 에이전트의 `identityLabel`(task 토큰, 없으면 워크스페이스 라벨). | 가정: 이미 있는 값만 쓴다(engineering/principles.md 7). 사용자의 "내가 요청한 것이 잘 보였으면"에 가장 가까운 기존 값이 task 토큰이다. |
| D-11 | 사람이 직접 연결한 이슈는 purpose와 같은 경로로 저장한다: 워크스페이스 토큰 `issue` 먼저, git 설정 `branch.<name>.issue` 미러. 워크트리와 함께 사라진다. hide가 쓰는 유일한 것. | 가정: `runtime/projects.rs`의 purpose 저장 경로 재사용(engineering 7, 8). |
| D-12 | 진입점은 PR #112가 만든 그대로: pane 없는 체크아웃의 빈 상태 자리, ⇧⌘H와 탭 스트립 버튼의 오버레이, Escape로 닫힘. 그 PR의 셸 배관(`ShellModel.projectHomeVisible`, 메뉴 명령, 빈 상태 판정)은 재사용하고 보드 본문은 교체한다. | 가정: 두 에이전트의 판정 "보드가 3~8 체크아웃에서 이김"과 사용자의 칸반 선택. 사용자: "프로젝트 눌렀을 때와 hide앱의 기본 View로 ... 나중에" |
| D-13 | 전달: `prd/home-tasks` 워크트리에서 `main`으로 PR 하나, CI(verify) 통과 후 사람이 머지. 스크린샷은 `agents/runs/home-tasks/` 아래 두고 PR 본문에는 업로드 URL로 넣는다. | `agents/config.json` delivery.mode=pr, baseBranch=main; `CLAUDE.md` 증거 규칙 |
| D-14 | 원칙 반영: `oh-my-principle` 654485f의 design/principles.md와 engineering/principles.md를 전부 읽었다. design 13(행동할 수 있는 상태만 화면에)이 B25·B27·B30, design 9(각 상태의 가장 작은 형태)가 B26~B29, design 3(가장 잦은 동작이 가장 적은 클릭)이 B14·B21, engineering 15(자라는 자원의 상한)가 B31, engineering 10(실패의 전달)이 B29에 반영됐다. design 11은 이미 두 후보(원래 판/조용한 판)로 이행됐다. | 이 PRD |
| D-15 | Agents 뷰의 "끝" 열과 Tasks 뷰의 머지됨 열은 접힌 헤더가 기본이고, 카드는 시간이 아니라 pane 종료(Agents)·워크트리 제거(Tasks)로 사라진다. | 가정: 24h 타이머는 hide 자체 타이머를 만드는 일이고(`docs/status-model.md`: "no timer of Hide's own"), 접힌 헤더가 design 13에 맞다. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | pane이 없는 로컬 체크아웃을 고르면 "No terminal open" 자리에 Project Home 보드가 뜨고, "Start new terminal" 버튼은 보드 상단 바에 남아 같은 동작을 한다. | D-12 |
| B2 | ⇧⌘H, 메뉴 "Project Home", 탭 스트립의 그리드 버튼이 보드를 현재 프로젝트의 오버레이로 열고 닫는다. Escape는 위에 시트가 없을 때만 닫는다. 원격 컨텍스트나 프로젝트 미선택에서는 PR #112의 기존 거부 문구가 그대로 나온다. | D-12 |
| B3 | 상단 바의 Agents/Tasks 세그먼트가 뷰를 바꾼다. 기본은 Tasks, 선택은 세션 안에서만 유지되고 재시작 시 Tasks로 돌아온다. | D-03 |
| B4 | Tasks 뷰는 위쪽 "즉석" 줄(git 트랙이 없는 main과 폴더 체크아웃)과 준비·작업 중·리뷰·머지됨 네 열이다. 열 헤더는 이름과 개수만 있고 부제가 없다. | D-01, D-02 |
| B5 | 워크트리 카드의 열은 스냅샷 사실로만 정해진다: 머지됨(`worktree.merged == true` 또는 PR 배지 merged) → 리뷰(열린 PR 있음) → 작업 중(`changedFileCount > 0` 또는 `ahead > 0`) → 준비(그 외). 사람이 드래그로 옮길 수 없다. | D-02, D-06 |
| B6 | 카드는 최대 3덩어리다: 헤드(브랜치 이름 모노스페이스, 요청이 2개 이상일 때만 "N 요청"), 제목(D-10의 한 줄, 없으면 생략), 그리고 에이전트 줄들. "에이전트 없음", "1 요청" 같은 채움 문구는 없다. | D-01, D-10 |
| B7 | 카드 아래 현재 단계 칩 하나만: 작업 중이면 `N changed` 또는 `↑N`, 리뷰면 `PR #n`과 CI 상태(pending·failed·passing만, unknown이면 CI 칩 없음), 머지됨이면 `merged`, 준비면 칩 없음. 5칸 트랙은 없다. | D-01 |
| B8 | 에이전트 줄은 코어 `SidebarAgent` 그대로: 상태 마크(● 작업 중, ? 질문, × 오류, ✓ 끝·안 읽음, 회색 ● 읽음), 제공자 아이콘, `identityLabel`, 경과 시간. 둘째 줄(`detail`)은 질문·오류·끝(안 읽음)에만 나오고 작업 중에는 나오지 않는다. | D-01, D-09 |
| B9 | 카드 안 에이전트 중 하나라도 Needs You(질문·승인·오류·blocked·hard stall root)면 카드에 경고색 후광, 오류가 있으면 위험색 후광이 뜬다. 후광은 열을 바꾸지 않고, 열 안 정렬만 맨 위로 올린다. | D-09 |
| B10 | 위임 자식(`lineageDepth > 0`)은 부모 줄 아래 왼쪽 세로선으로 들여쓴 줄로 접혀 들어가고, 다른 체크아웃에서 도는 자식은 `↳ <브랜치>`를 덧붙인다. 자식은 Needs You·Done에 들어가지 않는 코어 규칙을 그대로 따른다. | D-09 |
| B11 | 즉석 줄의 main·폴더 카드는 헤드와 에이전트 줄만 있고 단계 칩과 트랙이 없다. 에이전트가 없는 main·폴더 체크아웃은 즉석 줄에 나타나지 않는다. | D-01, D-02 |
| B12 | 이슈가 연결된 카드는 칩 `⊙ #N`을 단계 칩 옆에 보인다. 이슈가 닫혀 있으면 칩이 PR-merged 색이고 `#N 닫힘`으로 읽힌다. | D-05, D-06 |
| B13 | 이슈 칩 hover 시 공용 명령 툴팁으로 저장소·번호·열림/닫힘, 이슈 제목, Project Status(있을 때), 연결 출처("pane 토큰", "브랜치 설정", "PR #n 본문", "브랜치 이름", "직접 연결")가 나온다. 접근성 도움말은 같은 문장이다. | D-05, D-13 |
| B14 | 이슈 칩 클릭은 이슈 URL을 기본 브라우저로 연다. 카드 컨텍스트 메뉴에도 "이슈 열기"가 있고, 이슈가 연결된 Needs You 카드에서는 그것이 첫 항목이며 pane 포커스가 둘째다. 이슈 없는 카드는 pane 포커스가 첫 항목이다. | D-08 |
| B15 | 연결 신호는 D-05 순서로 첫 번째 것을 쓴다. pane 토큰은 lineage 루트뿐 아니라 자식 pane에서도 읽혀 그 카드에 붙는다. 브랜치 이름 패턴으로 붙은 이슈는 툴팁 출처가 "브랜치 이름"이고 다른 표시 차이는 없다. | D-05 |
| B16 | Project Status가 있고 열이 그것과 다르면(예: 열린 PR인데 Status가 Working) `≠ Working` 경고색 칩이 이슈 칩 옆에 뜨고, hover 툴팁이 "Project: Working · git: PR #41 열림"처럼 양쪽을 나란히 말한다. Status가 없거나 일치하면 칩이 없다. | D-06 |
| B17 | 신호가 없는 워크트리 카드의 메뉴 "이슈 연결…"은 필드 하나짜리 팝오버를 연다. `owner/repo#N`, `#N`(현재 저장소), 이슈 URL을 받고, 형식이 맞지 않으면 필드 아래 한 줄로 거부하며 저장하지 않는다. 저장하면 팝오버가 닫히고 칩이 바로 뜬다. | D-05, D-11 |
| B18 | 직접 연결은 워크스페이스 토큰과 `branch.<name>.issue` 미러에 저장되어 hide를 재시작해도 남고, 워크트리를 지우면 함께 사라진다. 같은 카드 메뉴의 "이슈 연결 해제"가 둘 다 지운다. 저장 실패는 진단 로그로 가고 칩은 뜨지 않는다. | D-11 |
| B19 | 준비 열에는 워크트리가 없는 열린 이슈가 제목과 `⊙ #N` 칩만 있는 회색 카드로 뜬다. 같은 이슈가 워크트리를 얻으면 그 카드는 사라지고 워크트리 카드 하나만 남는다. | D-07 |
| B20 | 백로그 카드는 클릭·메뉴가 "이슈 열기" 하나이고, 열린 이슈가 200개를 넘으면 가장 최근 갱신 200개만 보이며 열 헤더 개수 옆에 `+`가 붙는다. | D-07 |
| B21 | 카드 클릭은 그 체크아웃(Tasks) 또는 그 에이전트 pane(Agents)을 고르고 보드를 닫는다. 에이전트 줄 클릭은 그 pane을 고른다. | D-12 |
| B22 | 머지됨 열은 접힌 헤더(`머지됨 N`)가 기본이고, 클릭하면 펼쳐져 흐린 카드들이 보인다. 워크트리가 제거되면 카드가 사라진다. | D-15 |
| B23 | Agents 뷰는 진행 중·내 확인 대기·끝 세 열이다. 카드 = lineage 루트 요청: 헤드와 제목 없이 에이전트 줄이 카드가 되고, 자식은 B10처럼 들여쓰며, 아래 칩은 체크아웃 브랜치 · 현재 단계 · 이슈 순이다. | D-03 |
| B24 | Agents 뷰의 열은 코어 그룹 그대로다: 진행 중 = Working + Needs You(후광), 내 확인 대기 = Done(끝났고 안 읽음), 끝 = Seen(읽음·유휴, 흐림). 끝 열은 접힌 헤더가 기본이다. | D-03, D-15 |
| B25 | GitHub를 읽는 중이거나 마지막 읽기가 실패한 상태는 보드에 배너가 아니라 이슈 칩·백로그 카드가 마지막 성공 값으로 남고 툴팁 끝에 "GitHub: N분 전"이 붙는 것으로만 보인다. 한 번도 성공하지 못했으면 이슈 칩과 백로그가 없을 뿐이다. 실패 사유는 진단 로그로 간다. | D-04, D-14 |
| B26 | Herdr 연결이 끊기면 에이전트 줄이 흐려지고 카드 열은 마지막 git 사실을 유지한다. 복구되면 다음 스냅샷으로 되돌아온다. | D-14 |
| B27 | git 저장소가 아닌 프로젝트는 Tasks 뷰에 즉석 줄만, Agents 뷰는 그대로 보인다. 체크아웃이 하나도 없으면 PR #112의 기존 빈 문구가 남는다. | D-12 |
| B28 | 워크트리는 있는데 pane도 요청도 없는 카드는 헤드와 제목만 있는 2줄 카드다. 경로가 없어진 워크트리는 기존 `missing` 배지가 헤드에 붙는다. | D-01 |
| B29 | gh가 없거나 로그인이 안 됐거나 15초를 넘기면 기존 `CoreGithubStatus.unavailableReason`이 채워지고 보드에는 아무 문구도 없다. Overview 패널의 기존 GitHub 상태 표시가 그 사실을 보이는 자리다. | D-04, D-14 |
| B30 | 이슈 목록 읽기는 PR 목록과 같은 리더, 같은 세대에서만 일어난다. 보드를 여는 것, 프로젝트를 고르는 것, 새로고침 명령이 세대를 올리고, 틱마다 읽지 않는다. | D-14 |
| B31 | 보드는 1 Hz 스냅샷마다 다시 계산하지 않고 스냅샷 리비전이 바뀔 때만 다시 만든다. 이슈 칩·백로그는 프로젝트당 최대 200개, 그 이상은 B20의 `+`로 보고된다. | D-14 |
| B32 | 한글·영문·긴 브랜치 이름이 섞인 카드에서 제목은 2줄까지 접히고 브랜치 이름은 꼬리가 잘리며, 툴팁이 전체를 보인다. 창 폭이 열 넷의 최소 폭보다 좁으면 보드가 가로 스크롤한다. | D-01 |
| B33 | 모든 색·간격·반경·글꼴은 `HideTheme` 토큰이고, 보드 전용 값(카드 폭, 후광 반경, 자식 들여쓰기)은 `HideTheme.Home`에 추가되어 `pen-token-map.json`에 매핑된다. `node scripts/check-design-contract.mjs`가 통과한다. | D-14 |

## Technical structure

- 셸: PR #112의 진입 배관(`ShellModel.projectHomeVisible`, `ShellMenuCommand.projectHome`, 빈 상태 판정, Escape 정책)을 `main`에 다시 얹고, 보드 본문을 두 뷰 프레젠테이션 + 공용 카드 뷰로 교체한다. 인스펙터·Needs You 스트립·Sort·5칸 트랙 코드는 가져오지 않는다.
- 코어 스냅샷: `CheckoutSnapshot`에 이슈 링크 한 항목(저장소, 번호, 제목, URL, 열림/닫힘, Project Status, 출처)이 붙고, 프로젝트 스냅샷에 워크트리 없는 열린 이슈 목록이 붙는다. 이슈 링크 해석은 `sidebar.rs`의 purpose 체인 옆에서 같은 입력(pane 토큰, 브랜치 설정, PR, 브랜치 이름)으로 한다.
- 경계: pane·워크스페이스 토큰은 `wire.rs`에서만 읽는다. git 설정 `branch.<name>.issue`는 `git_dir.rs`가 브랜치 설명과 같은 방식으로 읽고 쓴다. 직접 연결은 purpose 저장 경로(토큰 먼저, git 미러)를 그대로 타는 새 이벤트 하나다.
- GitHub: `github.rs`의 `gh pr list` JSON에 `closingIssuesReferences`를 더하고, 같은 워커·같은 세대 주기에 허용 목록 항목 `gh issue list --state open --limit 200 --json number,title,url,state,projectItems,updatedAt`을 추가한다. 15초 타임아웃, 프로젝트별 캐시, 폴링 없음은 그대로. 새 자격 증명·새 프로세스 소유자 없음.
- 런타임 뮤텍스 아래 서브프로세스·블로킹 I/O 없음(기존 리더 스레드 모델 유지). Herdr 계약 변경 없음(0.9.1 `pane.report_metadata`·`workspace.report_metadata`가 이미 토큰을 준다).

## Risks

- `projectItems`는 gh 토큰에 `read:project` 범위가 있어야 채워진다. 없으면 Project Status와 `≠` 칩이 그냥 없고, 사유는 기존 `unavailableReason` 경로로 로그된다. 사용자가 `gh auth refresh -s read:project`를 할지는 사용자 결정이며 구현 전에 필요하지 않다.
- 브랜치 이름 패턴(`12-…`, `HOY-42-…`)은 추정이다. 잘못 붙어도 칩 하나와 툴팁 출처뿐이고, "이슈 연결…"로 덮어쓸 수 있다.
- PR #112는 2026-09-19 main 기준이라 #122(체크아웃 행 재설계)와 충돌한다. 셸 배관만 가져오고 보드 본문은 새로 쓰므로 충돌 범위는 `ShellModel`·`HideMainView`·`HideTerminalSurface`·`HideTheme`에 갇힌다.
- 두 뷰(D-03)와 백로그(D-07)는 되돌릴 수 있는 가정이다. 사용자가 리뷰에서 하나를 빼면 Behaviors B19·B20·B23·B24만 빠지고 나머지는 그대로다.
- 네이티브 검증은 격리된 Herdr 서버와 지정 PID 창에서만 한다(`docs/PERFORMANCE_TESTING.md`). 운영자의 hide 앱·pane·서버는 건드리지 않는다. gh 읽기 검증은 이 저장소의 실제 PR·이슈를 읽기만 한다.
- 사용자가 구현 전에 할 일은 없다. 열린 결정(기본 뷰, Agents 뷰 포함, 백로그 포함)은 이 PRD 리뷰에서 답하면 된다.
