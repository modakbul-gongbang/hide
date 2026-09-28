---
topic: "Overview 렌즈 2: Issues 이슈만, 이슈 패널, 카드 규칙"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "이슈 본문·댓글을 읽고 Local 이슈의 제목·본문을 hide 안 파일에 고치는 UI 변경이며, GitHub에는 쓰지 않는다."
source_intake: "agents/interview/overview-lenses/qa-log.md"
created_at: "2026-09-28"
updated_at: "2026-09-28"
---

# PRD: Overview 렌즈 2: Issues 이슈만, 이슈 패널, 카드 규칙

## Goal

운영자는 Issues 탭에서 리뷰 열에 PR 카드가 서 있어 어느 이슈의 일인지 읽지 못한다("리뷰의 경우에 PR이 보이는데 어떤 이슈인지는 안보여서!").
이 PRD는 PRD `overview-lenses-tiles-agents`가 머지된 main 위에서 Issues 탭의 카드 머리를 언제나 이슈로 만들고, 카드 클릭이 오른쪽 이슈 패널을 열게 하며, 카드의 평소 · 호버 · 멈춤 · 클릭 규칙을 정한다.
운영자가 확인할 한 문장: "Issues 탭에는 이슈만 있고, 카드를 누르면 오른쪽 패널에 본문과 이 이슈로 한 일이 보이며, GitHub와 Local은 줄이 있고 없음만 다르다."
근거 보드: `agents/runs/overview-redesign/design/export-final/` ef-top, efs5, efs6, efs7, efs10, efs12, efs13.

## Non-goals

- PRs 탭, 맡기기, 이슈 잇기는 PRD `overview-lenses-prs`가 맡는다 (D-14). 이 PR에서 카드의 PR 칩 클릭은 GitHub이고, `이슈 없는 PR N` 줄은 그 자리에서 PR 목록을 펼친다.
- GitHub 이슈의 제목 · 본문 · 라벨 · 댓글을 hide에서 고치거나 쓰지 않는다 (D-08). 결과: 편집 아이콘은 Local 이슈에만 있고 GitHub는 GitHub 링크다. 되돌릴 조건: 운영자가 hide 안 GitHub 편집을 결정할 때.
- 세 번째 출처(예: Linear)는 없다 (D-08의 표는 줄 규칙의 예일 뿐이다).
- 백로그 카드에 Linear식 우선순위 · 담당자 배정을 두지 않는다. 담당자는 패널의 읽기 줄일 뿐이다.
- 이슈 패널은 Overview 안의 층이지 Workspace의 뷰가 아니다. Workspace 안에서 이슈를 여는 것은 이 PRD 밖이다.
- List와 Dependencies 모드의 배치는 #218 그대로다. 이 PRD는 카드 규칙과 패널만 더한다.
- 디자인 원칙 6 · 9 (`design/principles.md`): 패널의 Local 편집은 되돌릴 수 있고(Esc 취소), 읽기 상태는 skeleton과 ⚠으로만 그린다. 관찰 가능한 형태는 B16, B22에 있다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-07 | Issues 탭은 이슈만 보이고 카드 머리는 언제나 이슈다. 이슈 없는 일은 열마다 접힌 한 줄. 리뷰 열 = closing reference 또는 브랜치 이슈 링크로 이어진 열린 PR이 있는 이슈. 완료 열은 이슈 이름과 닫은 PR 번호. | qa-log D-07, 운영자: "Issues 베이스가 맞나? 리뷰의 경우에 PR이 보이는데 어떤 이슈인지는 안보여서!" (efs5) |
| D-08 | 카드 클릭 = 보드 위 오른쪽 이슈 패널(보드 유지, ↑↓ 카드 이동, Esc 닫기). 뼈대는 머리 → 속성 → 이 이슈로 한 일 → 본문과 댓글이며 출처 차이는 줄의 있고 없음뿐. Local은 제목 · 본문을 그 자리에서 고치고 GitHub는 GitHub로 보낸다. 백로그 첫 동작 = 시작(S), 진행 중 = Workspace(O). 패널은 열 때 본문 · 라벨 · 작성 · 담당 · 댓글을 락 밖에서 한 번 읽는다. | qa-log D-08, 운영자: "Issues 탭에서는 클릭하면 Issue정보가 어떻게 보이나? 플랫폼별로 다른가!" (efs6) |
| D-09 | 공통 규칙: 평소 = 세 가지만, 내 차례만 색. 호버 = 카드 id 줄 끝의 제자리 버튼. 칩에 0.5초 멈춤 = 미리보기 카드. 클릭 = 영역마다 목적지 하나, ⌘클릭 = GitHub. | qa-log D-09 (efs7) |
| D-10 | 버튼은 아이콘이나 한 단어, 설명은 호버 popover(efs10: 시작, Workspace, GitHub, 편집, 접힌 줄). | qa-log D-10 |
| D-14 | 전달: PR 셋의 두 번째. 이 PR = Issues 이슈만 + 이슈 패널 + 카드 규칙. PRD 1 머지 후 main에서 시작. | qa-log D-14, D-15 |
| D-22 | 사실: `TaskSnapshot`은 key · source · id · url · title · open · updated_at · blocked_by만 싣는다. 본문은 `issue_detail_request`가 그때 읽는다(`issue_work.detail`). 라벨 · 작성 · 담당 · 댓글은 wire에 없다. | qa-log D-22 (herdr-core/src/tasks.rs:27-44) |
| D-40 | 패널의 GitHub 읽기는 기존 `issue_detail_request`를 넓혀 `gh issue view --json body,labels,author,assignees,comments`를 worker에서 한 번 읽고 `issue_work.detail`에 싣는다. 댓글은 수와 최근 셋. Local은 `local-issues.json`을 다 읽는다. | qa-log D-08의 표 "지금 읽는 것 · 더 읽을 것" (efs6), D-30 |
| D-41 | Local 이슈의 제목 · 본문 편집은 새 코어 이벤트 `local_issue_update {key, title, body}`가 `local-issues.json`에 쓴다. GitHub에는 쓰기 이벤트가 없다. | qa-log D-08 표 "제목 · 본문: 패널에서 바로 고침(Local) / 고치기는 GitHub에서" |
| D-42 | 가정: 카드 id에 0.5초 멈춘 미리보기 카드(id · 라벨 · 제목 · 본문 첫 세 줄 · 작성 · 날짜 · 댓글 수)는 패널과 같은 읽기를 쓰고 이슈마다 한 번 캐시한다. 읽기 전에는 본문 자리가 비어 있다. | 가정: efs7의 미리보기 카드 내용이 스냅샷에 없어 D-40의 읽기를 재사용 |
| D-43 | 가정: 이슈 패널의 폭은 보드 폭의 40%, 최소 360px이고 보드는 남은 폭에서 가로로 스크롤한다. 본문은 기존 Markdown 뷰어로 그린다. | 가정: efs6 · efs13의 비율, `web/src/viewers/`의 Markdown 뷰어 재사용(engineering #7) |
| D-44 | 가정: `이슈 없는 PR N` 줄은 이 PR에서 그 자리에 PR 번호 · 제목 줄로 펼쳐지고 줄 클릭은 GitHub다. PRD 3이 PRs 탭으로 목적지를 바꾼다. | 가정: PRs 탭이 아직 없음(D-14 순서) |
| D-50 | PRD 1이 wire에 실은 `SidebarAgentSnapshot.message`(둘째 줄의 원문 전체)가 에이전트 행 미리보기의 데이터다. | qa-log D-50 (PRD 1) |
| D-45 | 원칙 intake: `engineering/principles.md`와 `design/principles.md`(oh-my-principle 654485f9)를 읽음. design #1 · #3 · #6 · #9 · #13이 B1 · B10 · B17 · B22로, engineering #7 · #11이 D-43 · B20으로 옮겨짐. 옮기지 않은 규칙: 없음. | `sasu principles list` 2026-09-28 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Issues 탭의 네 열 백로그 · 진행 중 · 리뷰 · 완료에는 이슈 카드만 있다. 카드 머리는 언제나 출처 글리프 · id · 라벨(GitHub, 최대 둘)이고 그 아래 제목이다. 워크트리와 PR은 카드가 되지 않는다. | D-07 |
| B2 | 백로그 카드는 id · 라벨 · 제목뿐이다. 진행 중 카드는 그 아래 체크아웃 칩(브랜치 · ↑N · 파일 N)과 PR 칩, 그리고 에이전트 행 최대 둘(내 차례 먼저)과 `+N`이다. 리뷰 카드는 PR 칩 · CI 마크 · 리뷰 결정 한 단어를 더한다. | D-07, D-09 |
| B3 | 리뷰 열의 카드는 PR 본문의 closing reference나 브랜치의 이슈 링크로 이어진 열린 PR이 있는 이슈다. 완료 열은 접혀 `이슈 id · 제목 · 닫은 PR 번호` 한 줄씩이고 헤더의 `>`가 펼친다. 닫은 PR 번호에 멈추면 `PR #N 머지 · 날짜` popover다. | D-07 |
| B4 | 이슈 없는 워크트리는 진행 중 열 아래 `이슈 없는 워크트리 N` 한 줄, 이슈 없는 PR은 리뷰 열 아래 `이슈 없는 PR N` 한 줄이다. 멈추면 popover가 어디로 가는지와 목록(`#217 · #218`)을 말한다. 워크트리 줄 클릭은 Agents › 체크아웃으로 가고, `이슈 없는 PR N` 줄 클릭은 그 자리에서 PR 번호 · 제목 줄들로 펼쳐지며 펼쳐진 PR 줄 하나의 클릭이 GitHub다. 0이면 줄이 없다. | D-07, D-44 |
| B5 | 내 차례(에이전트가 묻거나 끝난) 카드만 노란 테두리이고 물음 행이 노란 색이며 열 맨 위로 온다. 막힌 카드는 노란 자물쇠와 막는 이슈 id를 보이고 시작은 막지 않는다(경고만). 완료 카드는 흐리다. 그 밖의 카드에는 색이 없다. | D-09 |
| B6 | 카드에 멈추면 id 줄 끝의 비워 둔 자리에 버튼이 나타나고 카드 높이는 그대로다. 백로그 = `▷ 시작` · `S` · `⋯`, 진행 중 = Workspace 아이콘 · `O` · `⋯`, 리뷰 = PR 아이콘 · `⋯`, Local = 편집 아이콘이 더해진다. `⋯`는 시작 · Workspace · GitHub · 편집(Local)을 담는다. | D-09, D-10 |
| B7 | 버튼에 멈추면 efs10의 popover가 뜬다: 시작 = "이 이슈로 워크트리와 에이전트를 만든다. 이름은 AI가 제안", Workspace = "Workspace 열기 O", 편집 = "Local 이슈만. 제목 · 본문을 그 자리에서 고친다". | D-10 |
| B8 | 카드 id에 0.5초 멈추면 이슈 미리보기 카드(id · 라벨 · 상태 · 제목 · 본문 첫 세 줄 · 작성자 · 날짜 · 댓글 수)가 뜬다. 에이전트 행의 말에 0.5초 멈추면 그 에이전트의 마지막 말 전문(PRD 1이 wire에 실은 `message`) popover, PR 칩에 멈추면 기존 PR 카드, 체크아웃 칩에 멈추면 체크아웃 카드다. | D-09, D-42, D-50 |
| B9 | 카드의 빈 곳과 제목 클릭은 이슈 패널을 연다. 에이전트 행 클릭은 그 패널, 체크아웃 칩은 Workspace, PR 칩은 GitHub, `#N` ⌘클릭은 GitHub다. 영역마다 목적지는 하나다. | D-09 |
| B10 | 카드를 누르면 보드 오른쪽에 이슈 패널이 열리고 보드는 남은 폭에 그대로 있다. ↑↓가 같은 열의 다음 · 이전 카드로, ←→가 이웃 열의 같은 높이 카드로 패널을 옮기고 열의 끝에서는 머문다. Esc는 패널을 먼저 닫고 그다음 Overview를 나간다. 다른 타일로 가면 패널은 닫힌다. | D-08, D-43 |
| B11 | 패널 머리는 출처 글리프 · id · 출처 이름 · 상태(Open · Closed) · ×, 그 아래 제목, 그 아래 동작 줄이다. 동작 줄의 첫 동작은 백로그면 `▷ 시작`과 `S`, 진행 중이면 Workspace와 `O`이고, 옆에 GitHub 아이콘(GitHub) 또는 편집 아이콘(Local), 오른쪽 끝 `⋯`다. | D-08 |
| B12 | 속성 줄은 단계, 라벨(GitHub), 작성(GitHub: 이름 · 날짜), 담당(GitHub, 있을 때), 갱신, 막힘(있을 때)이다. Local은 라벨 · 작성 · 담당 줄이 아예 없고 만든 날짜 줄이 있다. 없는 값은 줄이 없지 빈 값이 아니다. | D-08 |
| B13 | `이 이슈로 한 일`은 체크아웃 칩 줄(브랜치 · ↑N · 파일 N, 오른쪽 Workspace 아이콘), 그 아래 에이전트 행 전부(위임은 한 단 들여씀, 물음은 노란 줄), 그 아래 PR 칩 줄(번호 · 제목 · 리뷰 결정 또는 CI)이다. 셋 다 비면 절 자체가 없다. | D-08 |
| B14 | 본문은 Markdown으로 그려지고, 그 아래 댓글은 GitHub면 `댓글 N`과 최근 셋(작성자 · 날짜 · 본문), `쓰기는 GitHub에서`이고 Local이면 댓글 절이 없다. | D-08, D-40 |
| B15 | 패널이 열릴 때 본문 · 라벨 · 작성 · 담당 · 댓글을 한 번 읽는다. 읽는 동안 본문과 속성 자리는 skeleton이고 머리와 `이 이슈로 한 일`은 스냅샷 값으로 바로 선다. 같은 이슈를 다시 열면 캐시를 보이고 다시 읽는다. | D-08, D-40 |
| B16 | 읽기가 실패하면 본문 자리에 한 줄 이유와 `재시도`가 서고 패널의 나머지는 그대로다. 재시도는 그 이슈의 읽기만 다시 보낸다. 배너 · 알림은 없다. | D-08 |
| B17 | 출처 목록 읽기가 실패하면 보드는 마지막 이슈를 유지하고 카드마다 작은 ⚠ 하나가 서며, 멈추면 `GitHub 읽기 실패 · N분 전 값 · 이유는 로그에` popover다. Issues 타일에도 같은 ⚠이 선다. | D-07, D-09 |
| B18 | Local 이슈의 패널에서 제목이나 본문을 클릭하거나 편집 아이콘을 누르면 그 자리에서 편집이 된다. ⌘↵가 저장, Esc가 취소이며 저장 전에는 카드가 바뀌지 않는다. 저장 실패는 편집 자리에 이유와 그대로 남은 텍스트다. | D-08, D-41 |
| B19 | GitHub 이슈의 패널에는 편집이 없고, GitHub 아이콘과 ⌘클릭이 GitHub를 연다. | D-08 |
| B20 | 키보드: 카드는 포커스를 받고 ↑↓가 같은 열 안을, ←→가 이웃 열로 옮기며 끝에서는 머문다(패널이 열려 있으면 B10대로 패널이 따라온다). ↵ = 패널, S = 시작(가능한 카드), O = Workspace(진행 중 카드), Space = 미리보기 카드, C = 새 이슈, Esc = 패널 닫기 · 나가기. | D-09 |
| B21 | 필터와 `Board · List · Dependencies`는 사실 줄 오른쪽 끝에 있다. List와 Dependencies의 이슈 줄 · 카드 클릭도 같은 패널을 연다. | D-08 |
| B22 | 호버 · 포커스 · 멈춤은 로컬 상태이며 스냅샷 발행, 코어 이벤트, Git · 디스크 작업을 일으키지 않는다. 미리보기 읽기는 이슈마다 한 번이고 동시에 하나만 나간다. | D-09, D-42 |
| B23 | 시작(S)은 #218의 시작 대화상자를 그대로 연다. 새 Workspace로 화면이 옮겨지고, PR이 열리면 카드는 리뷰 열로 간다. | D-08 |
| B24 | Light 테마에서 패널과 카드는 efs13대로 그려진다. | D-08 |
| B25 | `docs/UI_BEHAVIOR.md` Project Home 절의 Tasks 보드 문단이 이 행동(이슈만, 접힌 줄, 카드 규칙, 이슈 패널)으로 갱신되고 `design/hide-screens.pen`의 `Screen / Project Overview`가 Issues 보드 · 카드 상태 · 이슈 패널(GitHub, Local, Light · Dark) 프레임을 갖는다. | D-14 |

## Technical structure

- 코어: `issue_detail_request`의 읽기를 본문 · 라벨 · 작성 · 담당 · 댓글로 넓히고 `issue_work.detail`에 싣는다(worker, 15초 타임아웃, 락 밖). 새 이벤트 `local_issue_update {key, title, body}`가 `local-issues.json`에 쓴다. `run_gh` 허용 목록에는 `issue view`의 json 필드만 늘고 새 쓰기 명령은 없다.
- wire: `TaskSnapshot`은 그대로다. 라벨 · 작성 · 댓글은 스냅샷이 아니라 요청 응답(`issue_work`)에만 있다. 웹 `PullRequest` 타입에 `closing_issues`를 선언한다(wire에 이미 있음).
- 셸: 리뷰 열 조건, 접힌 줄, 카드 규칙은 `web/src/projectBoard.ts`의 순수 함수이며 단위 테스트가 조건을 단언한다. 패널과 미리보기는 같은 캐시를 읽는다.
- `Mutex<Runtime>` 아래 subprocess 없음, 사용자 행동은 이벤트 하나, 호버는 발행 없음이라는 `CLAUDE.md` 규칙을 지킨다.
- 시각 권위는 `design/hide-ui.lib.pen`이며 이슈 카드 · 이슈 패널을 `Component /` 시트로 더한 뒤 `design/hide-screens.pen`에 그린다.

## Risks

- `gh issue view`에 댓글까지 붙으면 응답이 커진다. 댓글은 최근 셋만 싣고 본문은 요청 응답에만 두어 스냅샷 델타를 키우지 않는다.
- 리뷰 열 조건이 두 길(closing reference, 브랜치 링크)이라 한 PR이 두 이슈 카드에 칩으로 서는 것은 의도다(#218 규칙 유지).
- Local 편집은 hide의 파일 하나를 고치므로 되돌릴 수 있다. 저장 실패는 그 자리에 남는다.
- D-42 · D-43 · D-44는 되돌릴 수 있는 가정이며 운영자가 검토에서 뒤집을 수 있다.
- 실증은 fixture 스냅샷과 `gh` shim으로 하고, 실 GitHub에는 읽기만 나간다. 운영자가 미리 준비할 것은 없다.
