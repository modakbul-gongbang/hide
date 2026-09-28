---
topic: "Overview 렌즈 3: PRs 탭, 맡기기, 이슈 잇기"
status: "ready"
human_approval: "approved"  # user 2026-09-28 verbatim: PRD 3개 승인했으니 저거 3개를 순차적으로 opus 5.5 spawn해서 작업하게 해.
review_profile: "high-risk"
review_rationale: "이슈 잇기가 운영자의 GitHub PR 본문을 고치고 이슈를 만드는 바깥 쓰기이며, 맡기기가 PR 브랜치에서 에이전트를 시작한다."
source_intake: "agents/interview/overview-lenses/qa-log.md"
created_at: "2026-09-28"
updated_at: "2026-09-28"
---

# PRD: Overview 렌즈 3: PRs 탭, 맡기기, 이슈 잇기

## Goal

운영자는 올라온 PR마다 어느 이슈의 일이고 누가 고치고 있는지, 지금 공이 누구에게 있는지를 보고 싶다("현재 올라온 PR 베이스로 보기 -> 에이전트와 이슈 연결").
이 PRD는 PRD `overview-lenses-issues`가 머지된 main 위에서 PRs 타일과 PRs 탭을 만들고, CI가 막힌 PR을 에이전트에게 맡기며, 이슈 없는 PR을 이슈에 잇는다.
운영자가 확인할 한 문장: "PRs 탭은 내 차례부터 묶여 있고, 이슈 칸이 비면 이슈를 잇고, CI가 막히면 맡기기로 에이전트를 붙인다."
근거 보드: `agents/runs/overview-redesign/design/export-final/` ef-top, efs8, efs9, efs10, efs11, efs12.

## Non-goals

- hide는 PR을 만들거나 머지하거나 리뷰하지 않는다. 리뷰와 머지는 GitHub에서 하며 줄의 GitHub 아이콘이 그리로 간다 (efs12 "GitHub · 리뷰하고 머지").
- PR 본문에 쓰는 것은 `Closes #N` 한 줄뿐이다 (D-13). 제목 · 라벨 · 리뷰어를 고치지 않는다. 되돌릴 조건: 운영자가 다른 GitHub 쓰기를 결정할 때.
- 이슈 잇기의 hide 안 기록만(A안)은 기각이다 (D-13 "B로 가자!!!"). Local 이슈는 GitHub가 모르므로 hide 링크만 남는다 (D-34).
- 닫힌(머지 안 된) PR은 보이지 않는다 (D-32). 되돌릴 조건: 운영자가 닫힌 PR의 정리를 원할 때.
- 맡기기는 에이전트를 시작할 뿐 CI를 다시 돌리거나 리뷰 코멘트에 답하지 않는다.
- 사이드바의 PR 카드(`Component / PR hover card`)는 `PRs 탭에서 보기` 줄만 얻고 나머지는 그대로다.
- 디자인 원칙 6 (`design/principles.md`): 바깥 쓰기는 확인 하나이고 확인 버튼은 결과로 이름 짓는다. 관찰 가능한 형태는 B10, B12에 있다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-11 | PRs 탭(신설)은 누구 차례인지로 묶는다: 내 차례 → 에이전트가 고치는 중 → CI 실패 · 맡은 에이전트 없음 → 최근 머지(접힘). 줄 = 상태 글리프 · 번호 · 제목 · 이슈 칸(비면 점선 원) · 에이전트 마크 · 브랜치 · CI · 리뷰 결정 · 시간. 펼친 줄 = 에이전트와 조상 + 아이콘 버튼. | qa-log D-11 (efs8, efs9) |
| D-12 | 맡기기(CI 실패 또는 변경 요청, 맡은 에이전트 없음) = 같은 시작 대화상자를 PR 브랜치에서 열고 실패한 검사 · 리뷰 코멘트를 첫 지시로 넣는다. | qa-log D-12 (efs8, efs10) |
| D-13 | 이슈 잇기 = 이슈를 고르거나 PR 제목 · 본문으로 새로 만든 뒤 GitHub PR 본문에 `Closes #N`을 써서 머지 시 GitHub가 닫게 한다. 바깥 쓰기이므로 확인 한 번. A(hide 안 기록만)는 기각. | qa-log D-13, 운영자: "B로 가자!!!" |
| D-31 | 새 이슈 만들기는 확인 하나가 이슈 만들기 → PR 본문 쓰기 둘을 묶는다. 본문 쓰기가 실패하면 만들어진 이슈는 남기고 대화상자가 그 자리에 실패와 재시도(본문만)를 보인다. 본문에 이미 `Closes #N`이 있으면 다시 쓰지 않는다. 기각: 확인 두 번, 만들기를 새 이슈 대화상자로 미루기. | qa-log D-31, 운영자 답: "확인 하나, 이슈는 남김 (Recommended)" |
| D-32 | 묶음 우선순위: 머지됨 = 최근 머지. 열린 PR 중 그 브랜치 체크아웃에 working 에이전트가 있으면 에이전트가 고치는 중, 아니면 checks failed 또는 changes_requested면 CI 실패 · 맡은 에이전트 없음, 나머지(리뷰 필요, approved, 끝난 에이전트 확인, 초안)는 내 차례. 닫힌 PR은 안 보임. 타일 배지 = 내 차례 수. | qa-log D-32, 운영자 답: "머지 > 에이전트 작업 중 > CI·변경요청 > 내 차례 (Recommended)" |
| D-33 | 머지된 PR 줄의 `정리` = 기존 Delete worktree 대화상자, 브랜치 삭제 기본 꺼짐. | qa-log D-33 |
| D-53 | 있는 GitHub 이슈의 본문 쓰기 실패: hide 링크는 남기고 본문만 재시도, 이미 있으면 쓰지 않음. 기각: 링크 되돌림, 본문 먼저. | qa-log D-53, 운영자 답: "hide 링크는 남기고 본문만 재시도 (Recommended)" |
| D-54 | 정리 대화상자 안의 pane: 기존대로 `pane N개 닫고 삭제`로 닫고 삭제, 실행 중 에이전트는 경고 줄, 취소는 무변경, 닫힌 대화는 Sessions에서 다시 읽음. | qa-log D-54, 운영자 답: "기존 대화상자 그대로: pane 닫고 삭제, 실행 중은 경고 (Recommended)" |
| D-34 | 이슈 잇기 목록 = 프로젝트 출처의 열린 이슈. GitHub 이슈 = hide 링크 + `Closes #N`(확인 한 번). Local 이슈 = hide 링크만. 다른 저장소 이슈는 없다. | qa-log D-34, 운영자 답: "GitHub 이슈는 Closes, Local 이슈는 hide 안 링크만 (Recommended)" |
| D-09 | 공통 규칙: 평소 세 가지만, 호버 = 폭 72 고정 시간 자리의 제자리 버튼, 칩 0.5초 멈춤 = 미리보기, 클릭 = 목적지 하나, ⌘클릭 = GitHub. | qa-log D-09 (efs9) |
| D-10 | 버튼은 아이콘이나 한 단어, 설명은 popover(efs10: 이슈 잇기, 맡기기, GitHub, 이슈 없음, PRs 탭에서 보기). | qa-log D-10 |
| D-14 | 전달: PR 셋의 세 번째. PRD 2 머지 후 main에서 시작. 이 PR이 PRs 타일을 Agents · Issues 다음 자리에 넣고, 앞 PR들의 PR 칩과 `이슈 없는 PR N` 줄의 목적지를 PRs 탭으로 바꾼다. | qa-log D-14, D-15 |
| D-21 | 사실: `PullRequestSnapshot`은 checks(unknown · none · pending · failed · passing), review(review_required · changes_requested · approved), closing_issues, merged_at, is_draft를 이미 싣는다. `gh pr list`가 statusCheckRollup · reviewDecision · closingIssuesReferences를 읽는다. | qa-log D-21 |
| D-24 | 사실: gh 쓰기는 `run_gh` 허용 목록이 막고 #218의 유일한 쓰기는 `gh issue create`다. `gh pr edit`는 거부되고 테스트가 단언한다. | qa-log D-24 (herdr-core/src/github.rs:1037-1062) |
| D-25 | 사실: 시작 대화상자는 새 워크트리만 만든다. 기존 체크아웃에 지시와 함께 시작하는 경로는 Git 프로젝트에 없고, 코어 이벤트 `agent_start_in_checkout`은 prompt를 받는다. | qa-log D-25 |
| D-46 | 가정: 맡기기의 첫 지시는 `gh pr view --json statusCheckRollup,reviews,comments`로 worker가 읽은 실패한 검사 이름 · 링크와 변경 요청 리뷰의 코멘트를 그대로 적은 텍스트이며, 대화상자에서 고칠 수 있다. PR 브랜치에 로컬 체크아웃이 없으면(예: dependabot) 대화상자가 그 브랜치를 추적하는 워크트리를 만든 뒤 시작한다. | 가정: efs10 "첫 지시 = 실패한 검사 · 리뷰 코멘트", D-25의 빈 경로 |
| D-47 | 가정: `Closes #N`은 본문 끝에 빈 줄 뒤 한 줄로 붙이고, 본문은 쓰기 직전 `gh pr view --json body`로 읽어 붙인다. 같은 이슈가 이미 본문에 있으면 쓰기 없이 성공으로 끝난다. | 가정: D-31의 멱등 조건을 구현 경계로 옮김(engineering #11) |
| D-48 | 가정: 끝난 에이전트 확인 = 그 PR 브랜치 체크아웃의 에이전트가 done 묶음에 있는 것이며 줄에 노란 `확인` 배지가 선다. 그 패널을 보면 배지가 사라진다. | 가정: efs9 "끝난 에이전트 확인" 케이스를 status-model의 read 축으로 옮김 |
| D-50 | PRD 1이 wire에 실은 `SidebarAgentSnapshot.message`가 에이전트 말 전문 미리보기의 데이터다. | qa-log D-50 (PRD 1) |
| D-52 | 가정: `최근 머지` 포함 규칙 = 로컬 워크트리 기록이 남은 머지 PR 전부 + 최근 14일 안 머지 PR, 머지 시각 내림차순, 기본 접힘, 헤더 클릭으로 펼침. | 가정: efs8 "최근 머지 12(접힘)"와 사실 줄의 merged 워크트리 수를 하나의 규칙으로 |
| D-49 | 원칙 intake: `engineering/principles.md`와 `design/principles.md`(oh-my-principle 654485f9)를 읽음. design #3 · #4 · #6 · #9 · #13이 B7 · B12 · B17 · B22로, engineering #4 · #11 · #14가 D-47 · B18 · Technical structure로 옮겨짐. 옮기지 않은 규칙: 없음. | `sasu principles list` 2026-09-28 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 타일 줄이 Agents · Issues · PRs · Sessions 넷이 된다. PRs 타일의 수는 열린 PR 수와 '열림', 배지는 내 차례 PR 수, 막대는 내 차례 · 에이전트가 고치는 중 · CI 실패 비율이다. 배지에 멈추면 `내 차례 3 / 리뷰 2 · 끝난 에이전트 확인 1` popover다. | D-11, D-32 |
| B2 | PRs 탭은 `내 차례 N`, `에이전트가 고치는 중 N`, `CI 실패 · 맡은 에이전트 없음 N`, `최근 머지 N`(접힘) 네 묶음이며 빈 묶음은 헤더가 없다. 머지된 PR이 최근 머지, working 에이전트가 있는 열린 PR이 고치는 중, 아니면서 CI 실패 또는 변경 요청이면 CI 실패, 나머지 열린 PR이 내 차례다. 닫힌 PR은 없다. | D-32 |
| B3 | 줄(평소)은 왼쪽부터 `▸` · PR 상태 글리프(열림 초록, 초안 회색, 머지 보라) · 번호 · 제목 · 이슈 칸 · 에이전트 마크 · 브랜치(mono) · CI 마크 · 리뷰 결정 한 단어 · 시간이다. 이슈 칸은 제목 바로 오른쪽이고 비면 점선 원이다. 변경 요청과 `확인` 배지만 노란색이다. | D-11, D-48 |
| B4 | 에이전트 마크는 그 PR 브랜치 체크아웃의 에이전트 상태 마크들(최대 셋, `+N`)이다. 하나면 클릭이 그 패널, 여럿이면 줄을 펼친다. | D-11 |
| B5 | 줄 클릭과 ↵는 줄을 펼친다. 펼친 줄은 그 체크아웃의 에이전트를 조상부터 계보로 보이고(내 차례는 노란 줄), 아래에 GitHub · Workspace · 이슈 잇기(이슈가 없을 때) 아이콘 버튼이 선다. ← 접기, → 펼치기, ↑↓ 줄 이동, ⌘↵ · ⌘클릭 = GitHub. | D-11, D-09 |
| B6 | 줄에 멈추면 폭이 고정된 시간 자리에 버튼이 서고 줄은 움직이지 않는다. 기본은 GitHub 아이콘과 `⋯`(맡기기, 이슈 잇기, 브랜치 이름 복사)이고, CI 실패 · 변경 요청에 에이전트가 없으면 `▷ 맡기기`, 머지된 줄이면 `정리`다. 시간은 버튼 뒤로 숨는다. | D-09, D-10 |
| B7 | 이슈 칸에 멈추면 이슈 미리보기 카드, PR 번호에 멈추면 기존 PR 카드, 브랜치에 멈추면 체크아웃 카드, 에이전트 마크나 펼친 줄의 에이전트 말에 멈추면 그 에이전트의 마지막 말 전문(PRD 1이 wire에 실은 `message`) popover가 0.5초 뒤 뜬다. 빈 이슈 칸은 호버하면 이슈 잇기 아이콘으로 바뀌고 popover는 "이 PR을 이슈에 잇는다. 이을 이슈가 없으면 PR 제목 · 본문으로 새로 만든다"이다. | D-09, D-10, D-50 |
| B8 | 이슈 칸 클릭은 이슈 패널, 브랜치 클릭은 Workspace, CI 마크 클릭은 GitHub 검사 페이지, 리뷰 결정 클릭은 GitHub 리뷰다. | D-09 |
| B9 | 이슈 잇기 아이콘은 popover 안에 프로젝트 출처의 열린 이슈 목록(검색 가능)과 `새 이슈 만들기`를 연다. 다른 저장소 이슈와 닫힌 이슈는 목록에 없다. | D-34 |
| B10 | GitHub 이슈를 고르면 확인 대화상자가 `PR #N 본문에 "Closes #M"을 씁니다. 머지되면 GitHub가 이슈를 닫습니다.`와 [본문에 쓰기] [그만두기]를 보인다. 그만두기가 기본이다. 확인하면 hide 링크(브랜치 이슈 링크와 워크스페이스 토큰)를 남기고 본문을 쓴다. 성공하면 줄의 이슈 칸이 그 이슈로 채워진다. 본문 쓰기가 실패하면 hide 링크는 남아 이슈 칸이 채워진 채 대화상자가 그 자리에 한 줄 이유와 [본문 다시 쓰기] [닫기]를 보이고, 다시 쓰기는 본문 쓰기만 다시 한다. | D-13, D-34, D-53 |
| B11 | Local 이슈를 고르면 확인 없이 hide 링크만 남고 이슈 칸이 채워진다. GitHub에는 아무것도 쓰지 않는다. | D-34 |
| B12 | `새 이슈 만들기`는 PR 제목 · 본문이 채워진 제목 · 본문 필드와 하나의 확인 `이슈를 만들고 PR #N 본문에 "Closes #(새 번호)"를 씁니다` [만들고 쓰기] [그만두기]를 보인다. 순서는 이슈 만들기 다음 본문 쓰기다. | D-31 |
| B13 | 본문 쓰기가 실패하면 대화상자가 그 자리에 `이슈 #M은 만들었고 PR 본문 쓰기는 실패했습니다` 한 줄 이유와 [본문 다시 쓰기] [닫기]를 보인다. 만든 이슈는 남고 hide 링크는 남는다. 다시 쓰기는 본문 쓰기만 다시 한다. | D-31 |
| B14 | 본문에 같은 `Closes #M`이 이미 있으면 쓰지 않고 성공으로 끝난다. 다시 시도해도 두 번 붙지 않는다. | D-31, D-47 |
| B15 | `▷ 맡기기`(popover: "PR 브랜치에서 시작 대화상자를 연다. 첫 지시 = 실패한 검사 · 리뷰 코멘트")는 시작 대화상자를 연다. 워크트리 이름은 PR 브랜치로 고정되어 보이고, 첫 지시 필드에는 실패한 검사 이름 · 링크와 변경 요청 코멘트가 미리 적혀 고칠 수 있다. 시작하면 그 체크아웃에 에이전트가 붙고 화면이 그 패널로 간다. | D-12, D-46 |
| B16 | PR 브랜치의 로컬 체크아웃이 없으면 대화상자가 `이 브랜치의 워크트리를 만들고 시작합니다`라고 말하고, 시작이 워크트리를 만든 뒤 에이전트를 붙인다. 브랜치를 새로 만들지는 않는다. | D-46 |
| B17 | 맡기기로 붙은 에이전트가 일하는 동안 그 PR은 `에이전트가 고치는 중`으로 옮겨가고 줄에 마크가 선다. 에이전트가 끝나면 `확인` 배지와 함께 `내 차례`로 오고, 그 에이전트 패널을 보면 배지가 사라지고 줄은 리뷰 결정에 따라 남는다. | D-32, D-48 |
| B18 | 맡기기의 첫 지시 읽기(검사 · 리뷰 코멘트)가 실패하면 대화상자의 첫 지시 필드가 비고 그 자리에 이유와 `다시 읽기`가 선다. 시작은 막지 않는다. | D-46 |
| B19 | `최근 머지`는 기본 접힘이고 헤더 클릭이 펼치고 접는다. 안에는 머지된 PR 중 로컬에 워크트리 기록이 남은 것 전부와 그 밖의 최근 14일 안에 머지된 PR이 머지 시각 내림차순으로 있고, 줄은 흐리다. | D-32, D-52 |
| B20 | 머지된 줄의 호버 `정리`: 워크트리 폴더가 있으면 기존 Delete worktree 대화상자(브랜치 삭제 기본 꺼짐, 강제 없음)를 연다. 안에 pane이 있으면 버튼이 `pane N개 닫고 삭제`이고 실행 중 에이전트는 경고 줄로만 보이며, 취소는 아무것도 바꾸지 않는다. 폴더가 없고, 폴더가 없고 기록만 남았으면 `기록만 지웁니다` 확인 뒤 기록을 지운다. 워크트리 기록이 아예 없으면 정리 버튼이 없다. | D-33, D-54 |
| B21 | 앞 PR에서 GitHub로 가던 레인 머리 · 노드 · 이슈 카드의 PR 칩 클릭이 PRs 탭의 그 줄(펼침)로 바뀌고 ⌘클릭이 GitHub다. Issues 탭의 `이슈 없는 PR N` 줄 클릭도 PRs 탭이다. 사이드바 PR 카드 머리에 `PRs 탭에서 보기` 줄(popover: "이 PR의 이슈와 에이전트 계보")이 더해진다. | D-14, D-10 |
| B22 | GitHub를 아직 읽지 못했으면 탭은 skeleton 줄 셋이고 타일 수 자리가 빈다. 읽기가 실패하면 마지막 PR을 유지하고 PRs 타일 이름 옆 ⚠ 하나가 서며 popover에 실패와 나이가 있다. 배너는 없고 이유는 로그에 있다. | D-11 |
| B23 | ⌘⇧H · 프로젝트 행 진입은 여전히 Agents › 체크아웃이고 PRs 탭은 타일 클릭, 배지, 사이드바 PR 카드, ^Tab 복원으로만 열린다. | D-14 |
| B24 | 호버 · 포커스 · 멈춤은 로컬 상태이며 스냅샷 발행, 코어 이벤트, Git · 디스크 작업을 일으키지 않는다. 바깥 쓰기는 확인 뒤 이벤트 하나로 나간다. | D-09, D-13 |
| B25 | `docs/UI_BEHAVIOR.md` Project Home 절에 PRs 탭 · 맡기기 · 이슈 잇기가 더해지고, hide가 GitHub에 쓰는 명령 목록(`issue create`, `pr edit`)은 `docs/ARCHITECTURE.md`에 적힌다. `design/hide-screens.pen`의 `Screen / Project Overview`가 PRs 탭 · 펼친 줄 · 확인 대화상자(Light · Dark) 프레임을 갖는다. | D-14 |

## Technical structure

- 코어 이벤트 둘이 는다: `pr_link_issue {pr_number, issue_key | new_issue {title, body}}`와 `pr_delegate {pr_number, provider, prompt}`. 둘 다 worker에서 `gh`를 돌리고 진행 · 실패를 새 wire 절 `pr_work`(단계 · 실패 이유 · 만든 이슈 번호)로 돌려준다. 재시도는 같은 이벤트를 다시 보내고 본문 멱등 검사가 중복을 막는다.
- `run_gh` 허용 목록에 `pr edit --body`, `pr view --json body,statusCheckRollup,reviews,comments`가 늘고 거부 테스트가 함께 갱신된다. 다른 쓰기는 여전히 거부다.
- 맡기기는 체크아웃이 있으면 `agent_start_in_checkout {prompt}`, 없으면 기존 브랜치를 추적하는 `create_worktree`에 prompt를 실어 보낸다.
- wire: `PullRequestSnapshot`은 그대로이고 웹 `PullRequest` 타입이 closing_issues · merged_at · updated_at을 선언한다. 묶음 규칙은 `web/src/projectBoard.ts`의 순수 함수이며 단위 테스트가 D-32의 우선순위를 단언한다.
- `Mutex<Runtime>` 아래 subprocess 없음(gh는 worker), 사용자 행동은 이벤트 하나, 호버는 발행 없음이라는 `CLAUDE.md` 규칙을 지킨다.
- 시각 권위는 `design/hide-ui.lib.pen`이며 PR 줄 · 확인 대화상자를 `Component /` 시트로 더한 뒤 `design/hide-screens.pen`에 그린다.

## Risks

- 바깥 쓰기: `gh pr edit`와 `gh issue create`는 운영자의 GitHub 계정으로 나간다. 실증은 fixture 저장소(운영자 소유의 테스트 저장소 또는 `gh` shim)에서만 하고, 실 저장소에는 쓰지 않는다. 운영자가 미리 준비할 것: 실증용 GitHub 저장소 하나를 지정하거나 shim 실증을 승인한다.
- 부분 실패(이슈는 만들고 본문은 실패)는 D-31대로 이슈를 남긴다. 고아 이슈는 대화상자에 번호로 남아 운영자가 GitHub에서 처리할 수 있다.
- 묶음 우선순위(D-32)에서 "working 에이전트"는 status-model의 activity 축이며, 자식 대기 중인 부모도 working이다.
- 맡기기의 첫 지시는 리뷰 코멘트 원문을 담으므로 프롬프트가 길어질 수 있다. 대화상자에서 고칠 수 있게 두고 자르지 않는다.
- D-46 · D-47 · D-48은 되돌릴 수 있는 가정이며 운영자가 검토에서 뒤집을 수 있다.
