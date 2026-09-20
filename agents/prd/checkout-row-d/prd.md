---
topic: "사이드바 체크아웃 행 D: PR 단계 글리프, 접힌 행의 목적 줄, 디테일은 Overview 카드로"
status: "ready"
human_approval: "approved"  # user 2026-09-20 verbatim: ㅇㅇ 승인, /implement 가자 (sol xhigh로 작업 시켜)
review_profile: "standard"
review_rationale: "사용자가 매일 보는 사이드바와 Overview의 행 구조를 바꾸고, 사용자 동작 시 저장소의 .git/config에 브랜치 설명 한 줄을 쓰며, 에이전트 hook 출력에 문장을 더하지만, 자격 증명·결제·프로덕션 데이터·파괴적 동작은 없다."
source_intake: "agents/interview/checkout-row-d/qa-log.md"
created_at: "2026-09-20"
updated_at: "2026-09-20"
---

# PRD: 사이드바 체크아웃 행 D: PR 단계 글리프, 접힌 행의 목적 줄, 디테일은 Overview 카드로

## Goal

Hide 운영자(호연)는 프로젝트 하나에 워크트리를 열 개 넘게 두고 사이드바에서 "어느 워크트리가 뭘 하는 중이고 PR이 어디까지 갔나"를 훑는다.
지금 행은 모두 같은 `⎇` 아이콘을 달고, PR 아이콘은 에이전트 유무와 접힘 상태에 따라 자리를 옮기며, 워크트리의 목적은 어디에도 없다.
이 PRD는 사이드바 행을 한 줄(단계 글리프 · 이름 · 마지막 커밋 나이 · 항상 예약된 chevron 슬롯)로 바꾸고, 접혔을 때만 둘째 줄(에이전트 요약 + 목적)을 붙이며, 파일 수·↑↓·리뷰 결정·CI는 Overview 그룹 헤더의 배지 카드로 옮긴다.
목적은 Herdr workspace 토큰 `purpose`를 통로로, git 브랜치 설명을 오래 남는 자리로 삼아 sasu·에이전트·사람이 한 줄로 덮어쓴다.
사이드바는 느낌, Overview는 비교 - 시각 계층을 둘로 나누는 것이 이 변경의 한 줄이다.

## Non-goals

- sasu가 dispatch에서 `purpose`를 쓰는 변경: `~/projects/sasu` 저장소의 일이라 이 PRD 밖이다. 그것이 들어오기 전까지 sasu가 만든 워크트리는 대표 에이전트 제목으로 떨어진다(D-26). sasu 변경이 들어오면 B12의 대체 경로가 그대로 그 값을 받는다.
- Herdr의 workspace 수명 변경: 마지막 pane이 닫히면 workspace와 토큰이 사라지는 것은 업스트림 동작이고 hide는 배포 바이너리를 고치지 않는다. 그래서 git 브랜치 설명이 오래 남는 자리다(D-11). 업스트림이 pane 없는 workspace를 지원하면 미러는 남겨도 무해하다.
- hide 전용 목적 저장소: hide는 이미 base 브랜치를 자체 영속 상태에 두지만, 목적은 다른 도구와 워크트리가 읽을 수 있는 git 자리에 둔다(D-11, 거부안). hide 밖에서 읽을 필요가 사라지지 않는 한 다시 보지 않는다.
- detached HEAD와 원격 장치의 체크아웃에서 오래 남는 목적: 브랜치가 없거나 그 저장소 파일이 이 Mac에 없으므로 토큰만 쓴다. pane이 닫히면 대체값으로 떨어진다(B13, B14).
- 사이드바에 PR 번호·파일 수·↑↓·리뷰 결정·CI를 남기는 것: 사용자가 "왼쪽은 전체 느낌"으로 정했다(D-03). CI 실패가 사이드바에서 안 보이는 것이 아쉬워지면 왼쪽 글리프 색을 리뷰 결정으로 바꾸는 손잡이가 남아 있다.
- 사이드바 PR popover 유지: 같은 popover가 Overview에 있어 삭제한다(D-05). 뒤에 남는 코드는 같은 변경에서 지운다(engineering/principles.md 규칙 1).
- `design/hide-ui.lib.pen`에 화면 보드 추가: 라이브러리는 System/Component 시트만 받는다. 승인된 화면은 `agents/runs/<slug>/design/scratch.pen`에 다시 그리고 컴포넌트 둘만 D-15로 승격한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 행 레이아웃 D: 한 줄 36pt(kind 슬롯 · 이름 · missing/temporary 배지 · 마지막 커밋 나이 · 항상 예약된 chevron 슬롯). 둘째 줄은 접혔을 때만, 보일 글(목적 → 대표 에이전트 세션 제목 → PR 제목)이 있거나 에이전트가 있으면 생긴다. 에이전트가 없으면 요약 칩 없이 글만, 펼친 행은 한 줄이고 목적을 보이지 않는다. 대체 순서까지 에이전트 제목 뒤 PR 제목을 두는 안을 골랐다. | 사용자 "D로 가자 넘 좋은데"; Q1 "추천대로 가자"; Q14 항목 4; Q15 F3 수락; qa-log D-08, D-11, D-13, D-24, D-31 |
| D-02 | kind 슬롯 글리프: PR이 있으면 단계 옥티콘(draft/open/merged/closed, 수명주기 색, GitHub stale이면 muted), 없으면 브랜치, 주 체크아웃은 집, detached는 커밋, 폴더 프로젝트는 폴더. `primary`·`detached` 배지는 없앤다. | 사용자 "오 B 좋은데?"(옥티콘을 왼쪽으로); qa-log D-09 |
| D-03 | 사이드바에서 PR 번호·파일 수·↑↓·리뷰 단어·CI를 뺀다. 행 툴팁은 PR 줄(`#n · 상태 · 제목`)을 앞에, 그 뒤 지금 내용(에이전트 수, detached SHA, 경로, GitHub 불가·stale 이유). 옥티콘 클릭과 컨텍스트 메뉴 "Open PR #n"이 GitHub를 연다. 행 클릭은 지금처럼 접기/선택이다. | 사용자 "PR number도 없어도 괜찮을듯 … 마우스 오버하거나 클릭"; Q14 항목 3; qa-log D-10, D-23 |
| D-04 | 나이는 마지막 커밋 시각을 기존 `RelativeActivityToken`으로 그리고, 모르면 비운다. 워크트리 생성 시각은 툴팁에. 생성 시각·PR 갱신 시각 안은 거부. | Q9 "마지막 커밋으로 가자"; qa-log D-16 |
| D-05 | 사이드바 PR popover를 삭제한다. 새로고침·불가 이유·stale은 Overview의 GitHub popover가 맡는다. 클릭이 popover를 여는 안은 거부. | Q8 "삭제로 가자"; qa-log D-15 |
| D-06 | Overview 그룹 헤더는 카드 B: 1줄 = 접기 chevron · kind 글리프 · 브랜치 이름 · 이름 옆 muted 목적(대체값은 PR 제목, 없으면 없음); 2줄 = HideBadge 띠: PR 배지(옥티콘 + 상태 단어), CI 배지(`✓ Checks`/`✗ Checks`/`… Checks`, checks 없으면 없음), 파일 배지(`Clean` muted 또는 `N files` warning), `↓n behind <base>`(0 초과일 때, warning), `↑n ahead`(PR이 없고 0 초과일 때만), 용량. 문장형·컬럼형·최소형 후보와 두 줄+배지형 A는 거부. | Q4 "purpose가 있기는 해야 … PR 상태나 CI 상태 … 보기 편하게"; Q5 "오 B 좋은데?"; Q11 "2"; qa-log D-14, D-18 |
| D-07 | PR 배지 단어 우선순위: merged/closed > draft > 리뷰 결정(`Review required` open색, `Changes requested` warning, `Approved` success) > `Open`. 상태표는 승인된 보드에 그대로 있다. | Q15 F1 수락 "너 추천 좋은데"; qa-log D-29 |
| D-08 | Overview PR·CI 배지 클릭은 기존 GitHub popover를 연다. `N files` 배지는 지금처럼 History를 연다. GitHub 직접 열기 안은 거부. | Q12 "popover로 가자"; Q14 항목 2; qa-log D-19, D-22 |
| D-09 | 목적의 통로는 Herdr workspace 토큰 `purpose` 하나다. Herdr 실측: 값은 80자에서 잘리고, 소스가 달라도 마지막에 쓴 값이 이기며, 이긴 소스가 지우면 키가 사라진다. hide는 소스별 우선순위를 두지 않고 Herdr가 준 값을 그린다. | Q7 "덮어쓰는대로 가야지"; 실측 workspace w7J 2026-09-20; qa-log D-04, D-17 |
| D-10 | 목적은 한눈에 읽히는 한 줄이 목표다: 입력 필드는 글자 수를 보이고 40자를 넘으면 warning, 80자는 하드 스톱. sasu와 hook 안내도 "40자 이내"를 말한다. 행에서 넘치면 잘리고 툴팁에 전체. | Q7 "기본적으로 30-40자 정도에 요약 … 제한은 있지만 한번에 딱 보기 편하게"; qa-log D-17 |
| D-11 | 오래 남는 자리는 git의 `branch.<name>.description`이다. 토큰 값이 바뀔 때마다(누가 썼든) hide가 설명을 그 값으로 맞추므로 설명은 항상 마지막 토큰 값이고, workspace가 없으면 설명을 읽는다. "설명이 비어 있을 때만 미러" 안은 거부. 목적의 열쇠는 경로가 아니라 브랜치 이름이다: missing 워크트리도 브랜치가 남아 있으면 목적을 보이고, 브랜치를 지우면 설명이 같이 사라지며, 브랜치를 남긴 채 워크트리를 지웠다가 같은 브랜치로 다시 만들면 그 목적이 다시 보이고, 같은 경로에 다른 브랜치를 만들면 아무것도 넘어오지 않는다. hide 영속 상태 안과 "pane 열린 동안만" 안은 거부. | Q17 "1번으로 가자"; spec 게이트 F1·F3 묶음에 "ㅇㅇ"(추천 수락); qa-log D-33, D-34 |
| D-12 | 사람이 쓰는 자리 둘: 워크트리 생성 시트의 선택 필드 `Purpose`, 그리고 사이드바 행·Overview 헤더 컨텍스트 메뉴 "Set purpose…"가 여는 한 칸 시트. 빈 값 저장은 토큰과 설명을 지운다. hide는 소스 `hide`로 쓴다. 메뉴만 두는 안은 거부. | Q13 "ㅇㅇㅇㅇㅇ"(추천 수락); qa-log D-20 |
| D-13 | 쓰기 순서와 실패: 토큰을 먼저, 설명을 그 다음 쓴다. 토큰이 실패하면 아무것도 바뀌지 않고 시트에 오류. 토큰은 됐는데 설명이 실패하면 행은 이미 새 글(토큰이 이김)이고 시트에는 "저장됐지만 git에 기록하지 못함" 오류가 남아 Save가 둘 다 다시 쓴다(같은 값이라 안전). Cancel하면 토큰만 새 값이고 설명은 옛것이며 진단 로그에 남는다. 생성 시트는 워크트리 생성이 먼저 끝나므로 목적 쓰기 실패가 생성을 실패시키지 않는다: 시트는 닫히고 진단 로그만 남고 행은 대체값. | Q15 F2·F6 수락; spec 게이트 F2 묶음에 "ㅇㅇ"(추천 수락); qa-log D-30 |
| D-14 | Herdr 지원: 번들 0.9.1이 최소이고 로컬은 항상 지원. 서버 버전이 그보다 낮은 원격 장치에서는 "Set purpose…"가 비활성이고 툴팁에 이유, 행은 대체 경로. | Q15 F4 수락; qa-log D-32 |
| D-15 | hook 안내: hide-agent-hooks가 Claude·Codex SessionStart 양쪽 stdout에 영어 한 문장 additionalContext를 낸다 - 워크트리를 만들면 40자 이내 한 줄로 `herdr workspace report-metadata <workspace> --source <you> --token purpose="…"`를 실행하라. 지금은 어느 이벤트에도 stdout이 없으므로 HOOK_VERSION을 올린다. | Q14 항목 1; qa-log D-06, D-12, D-21 |
| D-16 | 디자인 시스템 제안 둘: HideBadge에 앞쪽 이미지 옵션(PR 배지), 그리고 `Component / Checkout row`·`Component / Overview group header` 시트. 승인된 보드(`agents/interview/checkout-row-d/design/checkout-row-final.pen`)를 구현 시작 때 `agents/runs/<slug>/design/scratch.pen`으로 옮겨 그린다. | Q14 항목 5; qa-log D-07, D-25 |
| D-17 | DESIGN.md의 사이드바 행 규정(detached 배지, disclosure 화살표), 그룹 헤더 줄과 "상태 단어는 툴팁에만" 규정, 사이드바 popover 문장을 같은 PR에서 고친다. `docs/status-model.md`는 그대로. | qa-log D-28 |
| D-18 | 검증: Swift 프레젠테이션 단위 테스트(상태별 글리프·둘째 줄 규칙과 대체 순서·나이·툴팁·헤더 배지와 ↑↓ 규칙·시트의 40/80 규칙), herdr-core 테스트(workspace 토큰과 브랜치 설명이 체크아웃 스냅샷의 목적으로 나오는 것, 미러와 지우기), hide-agent-hooks 테스트(두 런타임 SessionStart 출력의 문장), 격리 Herdr 서버 위의 dev 앱 스크린샷 QA를 승인 보드와 비교. 산출물은 `agents/runs/<slug>/`. | qa-log D-27; docs/PERFORMANCE_TESTING.md 격리 규칙 |
| D-19 | 원칙 intake: `~/projects/oh-my-principle` `654485f`의 `design/principles.md`와 `engineering/principles.md`를 전부 읽었다. 디자인 3·7·9·13은 D-03·D-06·D-07·D-13으로, 11은 후보 보드로, 엔지니어링 1·4·6·7·9·13은 D-05·D-09·D-11·B18·B22·기술 구조로 번역했다. 디자인 6(되돌리기)은 목적 지우기가 다시 입력하면 되돌아가는 일이라 확인창을 두지 않는 것으로 만족한다. 엔지니어링 14·15는 상주 프로세스가 없어 해당 없음. | 이 PRD 작성 시 읽음 |
| D-20 | 배포: `agents/config.json`대로 worktree 브랜치에서 `main`으로 PR을 열고 CI를 지켜보며, merge는 사용자의 명시적 승인 뒤에만 한다. | agents/config.json delivery.mode=pr |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 에이전트 없는 워크트리 행은 한 줄이다: 왼쪽 글리프, 이름, 오른쪽에 `4h` 같은 마지막 커밋 나이, 빈 chevron 자리. 에이전트가 생기거나 행을 접고 펼쳐도 나이와 글리프의 x 위치가 움직이지 않는다. | D-01, D-04 |
| B2 | 접힌 행에 에이전트가 있으면 둘째 줄이 생겨 상태 마크 · 제공자 배지 · `+N` 칩 뒤에 목적이 온다. 목적이 없으면 대표 에이전트의 세션 제목이 muted로, 그것도 없고 PR이 있으면 PR 제목이 온다. 에이전트가 없고 글만 있으면 칩 없이 글만이다. 셋 다 없으면 한 줄이다. | D-01 |
| B3 | 행을 펼치면 둘째 줄이 사라지고 한 줄이 되며 그 아래 에이전트 행이 온다. 다시 접으면 둘째 줄이 돌아온다. | D-01 |
| B4 | 글리프: PR open은 초록 pull-request 옥티콘, draft는 회색 draft 옥티콘, merged는 보라 merge, closed는 빨강 closed, PR 없는 워크트리는 브랜치, 주 체크아웃은 집, detached는 커밋, 폴더 프로젝트는 폴더. 사이드바 어디에도 `primary`·`detached` 배지가 없다. `missing`(danger)과 `temporary`(warning) 배지는 남는다. | D-02 |
| B5 | merged·closed 행은 지금처럼 흐려진다. GitHub가 stale이면 옥티콘만 muted다. gh가 없거나 로그아웃이면 브랜치 글리프이고 툴팁에 이유 문장이 있다. 폴더가 없으면 글리프가 danger 색이고 나이가 없다. git status를 읽는 중이면 나이도 둘째 줄도 없다. | D-02, D-07 |
| B6 | 행에 마우스를 올리면 툴팁 첫 줄이 `#118 · Open · <PR 제목>`(리뷰 결정이 있으면 그 단어, stale이면 `· Last known 2h`)이고, 그 아래 지금의 에이전트 수·detached SHA·경로가 온다. 사이드바 어디에도 PR 번호, 파일 수, ↑↓, `review`/`changes` 단어, CI 표시가 없다. | D-03 |
| B7 | PR이 있는 행의 옥티콘을 클릭하면 GitHub에서 그 PR이 열린다. 컨텍스트 메뉴에 "Open PR #118"이 있고 같은 일을 한다. PR이 없으면 둘 다 없다. 행의 나머지 클릭은 지금처럼 접기(에이전트 있음) 또는 선택이다. | D-03 |
| B8 | 사이드바 PR 아이콘을 눌러 열리던 popover는 없다. 새로고침과 GitHub 상태 상세는 Overview의 GitHub 셀 popover에만 있다. | D-05 |
| B9 | Overview 그룹 헤더 1줄은 접기 chevron · 글리프 · 굵은 브랜치 이름 · 그 옆 muted 목적(없으면 PR 제목, 그것도 없으면 이름만)이다. 목적이 길면 잘리고 헤더 툴팁에 전체가 있다. | D-06 |
| B10 | 헤더 2줄은 배지 띠다: `⬡ #112 Changes requested`(warning) `✗ Checks`(danger) `Clean` `↓64 behind main`(warning) `2.2 GB`. checks가 없는 PR은 CI 배지가 없고, 통과는 `✓ Checks` success, 실행 중은 `… Checks` warning. 미커밋 변경이 있으면 `3 files` warning. PR이 없고 base보다 앞선 커밋이 있으면 `↑1 ahead`가 있고, PR이 있으면 ↑는 없다. behind가 0이면 ↓ 배지가 없다. | D-06, D-07 |
| B11 | 헤더 배지의 실패 상태: git status 못 읽음 `? files` warning, 읽는 중 `… files` muted, 폴더 없음 `missing` danger, gh 불가 `? PR` warning(이유는 popover), stale이면 PR 배지 muted. 폴더 프로젝트는 지금처럼 용량 배지만. | D-06 |
| B12 | 헤더의 PR 배지나 CI 배지를 클릭하면 Overview의 GitHub popover가 열리고(제목, 상태, checks, 새로고침, 불가·stale 이유, GitHub 열기), `N files` 배지를 클릭하면 그 체크아웃이 History에 열린다. `Clean`은 클릭이 없다. 어떤 배지도 GitHub를 바로 열지 않는다. | D-08 |
| B13 | 누군가 `herdr workspace report-metadata <ws> --source x --token purpose="…"`를 실행하면 다음 스냅샷에서 그 워크트리의 사이드바 둘째 줄과 Overview 헤더에 그 글이 보인다. 다른 소스가 다시 쓰면 그 값으로 바뀐다. 100자를 쓰면 80자로 잘려 보인다. | D-09 |
| B14 | 토큰 값이 바뀔 때마다 hide가 `branch.<name>.description`을 그 값으로 맞춘다. 그 워크트리의 pane을 모두 닫아 Herdr workspace가 사라져도 행과 헤더의 목적은 그대로이고, `git config branch.<name>.description`으로 같은 글을 읽을 수 있다. detached 체크아웃과 원격 장치의 체크아웃은 토큰만 쓰므로 workspace가 사라지면 대체값으로 떨어진다. | D-11 |
| B26 | 폴더가 사라진(missing) 워크트리도 브랜치가 남아 있으면 목적을 계속 보인다. hide에서 워크트리를 지우면서 브랜치도 지우면 목적이 사라지고, 브랜치를 남기면 같은 브랜치로 다시 만든 워크트리가 그 목적을 다시 보인다. 같은 경로에 다른 브랜치로 만든 워크트리에는 목적이 넘어오지 않는다. | D-11 |
| B15 | 워크트리 생성 시트에 `Purpose · optional, one line` 필드가 Agent 아래 있다. 오른쪽에 `20 / 40` 글자 수가 있고 40자를 넘으면 숫자가 warning 색이며 80자에서 더 입력되지 않는다. 비워 두고 Create해도 지금처럼 만들어진다. | D-10, D-12 |
| B16 | 생성 시트에 목적을 적고 Create하면 워크트리가 생기고 그 행이 접힌 채 둘째 줄에 그 목적을 보인다. 브랜치 설명과 (workspace가 생긴 경우) 토큰 둘 다 그 글이다. | D-11, D-12 |
| B17 | 사이드바 행과 Overview 헤더의 컨텍스트 메뉴에 "Set purpose…"가 있다. 열리는 시트는 브랜치 이름, 현재 목적이 든 한 칸 필드와 같은 글자 수 표시, Cancel/Save다. Save하면 시트가 닫히고 행·헤더가 새 글로 바뀐다. 비우고 Save하면 목적이 사라지고 대체값이 온다. | D-12 |
| B18 | Set purpose에서 Herdr 토큰 쓰기가 거부되거나 응답이 없으면 아무것도 바뀌지 않고 필드 아래에 한 줄 오류(`Herdr did not answer. Your text is kept; Save tries again.`)가 뜨며 입력은 그대로다. 토큰은 됐는데 git 설명 쓰기가 실패하면 행은 이미 새 글이고 시트에 `Saved to Herdr, but git did not record it. Save tries again.`이 남는다. Save는 둘 다 다시 쓰고 Cancel은 시트만 닫는다. 진단 로그에 어느 쪽이 왜 실패했는지 남는다. | D-13 |
| B19 | 생성 시트에서 워크트리는 만들어졌는데 목적 쓰기가 실패하면 시트는 정상처럼 닫히고 행은 대체값을 보이며, 진단 로그에 실패 이유가 남는다. 사용자는 "Set purpose…"로 다시 적을 수 있다. 알림·배너는 없다. | D-13 |
| B20 | Settings에서 붙인 원격 장치의 Herdr가 0.9.1보다 낮으면 그 장치 행의 "Set purpose…"가 비활성이고 툴팁이 이유를 말한다. 생성 시트의 Purpose 필드는 그 장치에서 보이지 않는다. 행은 에이전트 제목·PR 제목 대체 경로를 쓴다. | D-14 |
| B21 | hide 안에서 Claude나 Codex 세션이 시작되면 SessionStart hook 출력에 워크트리 목적 안내 한 문장(40자 이내, 정확한 herdr 명령 포함)이 additionalContext로 들어 있다. 다른 hook 이벤트의 출력은 지금처럼 없다. | D-15 |
| B22 | 사이드바 행의 접근성 라벨은 이름, 글리프가 뜻하는 단계, 나이, 둘째 줄 글을 순서대로 읽고, Overview 헤더 라벨은 이름·목적·각 배지를 단어로 읽는다(`pull request 112 changes requested`, `checks failing`). | D-06, D-03 |
| B23 | `bash scripts/verify-swift.sh test`와 `bash scripts/verify-cargo.sh test`에 D-18의 테스트가 포함돼 통과한다. `node scripts/check-design-contract.mjs`가 통과한다. | D-18 |
| B24 | DESIGN.md에 새 행 구조(글리프 집합, 둘째 줄 규칙, 툴팁·클릭), 그룹 헤더 카드(배지 띠, 상태 단어 표면, ↑↓ 규칙), 목적의 두 자리와 대체 순서가 적혀 있고, "detached 배지", "상태 단어는 툴팁에만", 사이드바 popover 문장은 없다. | D-17 |
| B25 | PR 본문에 승인 보드와 실제 앱 스크린샷의 비교, 격리 서버로 찍은 접힘·펼침·시트·헤더 스크린샷, 토큰 실측 명령 기록이 있다. | D-18, D-20 |

## Technical structure

- herdr-core, 워크스페이스 토큰: `ProjectedWorkspace`가 Herdr `WorkspaceInfo.tokens`를 받고(지금은 wire에서 버려진다), 체크아웃 스냅샷에 `purpose { text, origin }`(origin은 token · branch_description · agent_title · pr_title 열거형)을 더한다. 둘째 줄 글과 그 출처는 코어가 정하고 셸은 그리기만 한다(status-model의 둘째 줄 소유 규칙과 같음). 문자열 매칭이 아니라 열거형으로 출처를 모델링한다.
- herdr-core, 브랜치 설명 읽기: `git_dir::Repository`에 `common_dir/config`의 `[branch "<name>"] description` 한 키를 읽는 함수를 더한다. 코어에 config 파서가 없으니 그 키에 한정한 작은 파서를 두고(첫 줄만, git의 따옴표 규칙), git 프로세스는 띄우지 않는다. 읽기 실패는 진단 로그와 "설명 없음"이다.
- herdr-core, 쓰기: 새 이벤트 `set_checkout_purpose { checkout_id, text }`가 기존 task-operation 경로(`worktree_create`와 같은 worker 스레드, `TaskOperationSnapshot`의 working/ready/failed)로 `SystemGit.run(["config", "branch.<b>.description", text])`(빈 값이면 `--unset`)와 Herdr `workspace.report_metadata`(소스 `hide`, workspace가 있을 때만)를 순서대로 실행한다. `create_worktree` 이벤트는 선택 `purpose`를 받아 `worktree.create` 성공 뒤 같은 쓰기를 하되 그 실패는 작업 실패가 아니라 진단이다. 미러(토큰 → 설명)는 같은 worker에서 락 밖에서 한다. Mutex<Runtime> 아래 서브프로세스는 없다.
- hide-agent-hooks: `run_hook`이 SessionStart에서 런타임별 stdout JSON을 내고(Claude `hookSpecificOutput.additionalContext`, Codex는 그 런타임의 동등 필드), `HOOK_VERSION`을 2로 올려 설치된 hook 파일이 갱신되게 한다.
- macOS 셸: `CheckoutNavigatorRow` 재작성, `WorkspacePullRequestControl` 삭제, `CheckoutOverview.groupHeader`를 배지 띠로 재작성, `OverviewPresentation.headerChips`를 배지 모델로 확장, `HideBadge`에 앞쪽 이미지, `WorktreeCreationSheet`의 Purpose 필드, 새 Set purpose 시트, 컨텍스트 메뉴 항목 둘. 원격 장치 판단은 코어가 이미 아는 서버 버전으로 한다.
- 새 서비스·스키마·네트워크 경계는 없다. 저장소에 쓰는 것은 사용자 동작(또는 새 토큰의 미러)에 따른 `.git/config`의 브랜치 설명 한 키뿐이다.

## Risks

- `.git/config` 쓰기는 git 자체의 lock을 거친다. 다른 git 프로세스가 잡고 있으면 실패하고 B18의 오류로 돌아온다. 미러 쓰기 실패는 진단만 남기고 다음 스냅샷에서 다시 시도하지 않는다(무한 재시도 방지). 그 사이 행은 토큰 값을 보인다.
- `git branch --edit-description`으로 여러 줄을 적어 둔 브랜치는 첫 줄만 보인다. hide가 저장하면 한 줄로 덮어쓴다. 이 손실은 B17의 시트에 현재 값을 보여 주는 것으로 사용자가 알 수 있다.
- 에이전트가 토큰을 새로 쓰면 미러가 사용자의 설명을 덮어쓴다. "마지막에 쓴 값이 이긴다"(D-09, D-11)의 의도된 결과다. 토큰만 쓰이고 설명 쓰기가 실패한 채 Cancel한 경우는 pane을 다 닫으면 옛 글로 돌아간다(D-13); 진단 로그가 그 시점을 남긴다.
- 브랜치 이름에 따옴표·백슬래시가 있으면 config 섹션 이름이 escape된다. 파서는 git의 규칙을 따르고, 못 읽으면 "설명 없음"과 진단이다.
- 사이드바 행이 두 줄이 되는 순간 사이드바가 길어진다. 접힌 행에 글이 없으면 한 줄이므로 목적을 안 쓰는 프로젝트는 지금 높이 그대로다.
- 원격 장치의 체크아웃은 이 Mac에 저장소 파일이 없어 설명을 못 읽는다. 토큰만으로 동작하고 pane이 닫히면 대체값이다(B14, B20).
- 라이브 증명은 격리 Herdr 서버와 임시 저장소에서만 한다. 운영자의 실제 저장소 config에는 쓰지 않는다.
- 사용자가 미리 해 줄 일은 없다.
