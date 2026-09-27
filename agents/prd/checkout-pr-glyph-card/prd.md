---
topic: "체크아웃 행의 PR 글리프: 클릭하면 PR 열기, 행 hover는 PR 카드"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "사이드바 행에 클릭 대상과 hover 카드를 더하고 Pen 라이브러리에 Component를 추가하는 사용자 대면 변경이며, 외부 링크는 사용자 클릭으로만 열리고 데이터·권한 효과는 없다."
source_intake: "agents/interview/checkout-pr-glyph-card/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: 체크아웃 PR 글리프 클릭과 hover 카드

## Goal

hide 사용자가 Projects 사이드바에서 체크아웃의 PR 글리프를 클릭해 그 PR을 바로 열고, 행에 마우스를 올리면 텍스트 네 줄 대신 PR 상태·리뷰·CI·브랜치·에이전트·커밋·경로를 한눈에 읽는 카드를 본다.
사용자의 말: "Checkout 옆에 붙는 git 표시에 클릭 가능하게? PR 생겨있거나 하면 바로 링크로 연동되게? 그리고 마우스오버할 때 나오는 팝업도 더 시각화가 잘되면 좋겠는데".
시각 참조는 `agents/runs/ux-fixes-2026-09-27/design/board-v3.pen` 섹션 6.

## Non-goals

- 카드에 스냅샷에 없는 값(리뷰어 이름, 체크 개수, 머지 가능 여부, base 브랜치, ahead/behind)을 그리지 않는다; 값이 생기면 그때 줄을 더한다 (D-03, design 원칙 10).
- PR이 없는 체크아웃의 글리프는 클릭되지 않는다 (D-02).
- Project 행·에이전트 행의 hover는 바꾸지 않는다.
- Checkout 우클릭 메뉴의 다른 항목(T6 범위)은 넣지 않는다; "Open pull request" 하나만 이 PR의 것이다 (D-07).
- engineering/principles.md 규칙 7이 기존 `pullRequestBadge`·`browser_open` 재사용을, design 규칙 4·7·9·10이 카드의 줄 규칙을 정한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 지금 PR 글리프는 색만 다르고 클릭되지 않으며 행 hover는 4줄 텍스트 tooltip이다. 스냅샷은 PR의 number·title·url·badge·review·is_draft·checks를 가지고, `browser_open` 이벤트로 hide 브라우저 탭을 연다. | 저장소 사실 (qa-log D-01) |
| D-02 | PR이 있는 체크아웃의 글리프는 버튼: 클릭하면 PR url을 앞 워크스페이스의 hide 브라우저 탭으로 연다(`browser_open`); ⌘클릭은 기본 브라우저. hover에 PR 색 ring. PR 없는 글리프는 클릭 불가. 글리프 클릭은 행 클릭(열기)을 일으키지 않는다. | 사용자: "git 표시에 클릭 가능하게? ... 바로 링크로 연동되게?" (qa-log D-02) |
| D-03 | 행 hover·포커스의 tooltip을 카드로: 헤더 = PR 배지(D-08 매핑) + #번호 + "Open PR ↗"(클릭은 D-02); 제목(두 줄); 구분선; Review·Checks·Branch·Agents·Commit·Path 줄(값·생략은 D-08·D-09). 값은 스냅샷 필드에서만. | 사용자: "팝업도 더 시각화가 잘되면" + 보드 승인 (qa-log D-03) |
| D-04 | 카드는 기존 tooltip과 같은 지연·위치 규칙으로 열리고 행 포커스에도 열린다; "Open PR"만 인터랙티브이며 마우스가 카드로 옮겨가도 유지된다; 스크린리더용 행 label(지금의 detail 문장)은 유지. | 가정 (qa-log D-04) |
| D-05 | Pen: "Component / PR hover card"를 System 마스터로 조립해 design/hide-ui.lib.pen에 추가(DESIGN_WORKFLOW의 Component 절차, HEAD JSON에 스크립트 이식, 구조 diff), Screen / Projects Sidebar 시트에 hover 상태 하나 추가. 코드는 web/src/components/pr-card.tsx. | 가정 (qa-log D-05) |
| D-06 | 검증: vitest(카드 줄 규칙), 웹 e2e 1개(fake PR: 글리프 클릭 → 브라우저 탭, hover → 카드), gen-screens·check-design-contract 통과, 라이브러리 구조 diff. | 가정 (qa-log D-06) |
| D-07 | T2와 T5가 main에 머지된 뒤 시작. please, Claude Implementor --effort high, PR 배포, Observer 자동 머지. Checkout 메뉴의 "Open pull request" 항목은 이 PR의 것. | 사용자 승인 의존 표 (qa-log D-07) |
| D-08 | 배지는 기존 `pullRequestBadge` 매핑 그대로: merged → Merged, closed → Closed, open → Draft/Open, review → Approved(success)/Changes requested(destructive)/Review required(muted); review이면서 is_draft이면 배지 옆에 "Draft"를 pr-draft 색으로 덧붙임. Review 줄은 review 필드(null이면 생략), Checks 줄은 checks 필드(passing/failed/pending; none·unknown은 생략). 새 매핑 함수 없이 `pullRequestBadge` 확장. | 가정 (qa-log D-08) |
| D-09 | PR 없는 체크아웃의 카드는 있는 값만: Branch(worktree.branch, detached면 "Detached HEAD" + 짧은 sha), Agents(합이 0보다 클 때), Commit(나이가 읽혔을 때), Path(항상). 플레인 폴더는 Path·Agents만. missing은 destructive 색 "Folder missing" 헤더 + Path. 값이 하나뿐이면 카드 대신 지금의 텍스트 tooltip 형태로 같은 내용. | 가정 (qa-log D-09) |
| D-11 | 가정: 앞 워크스페이스가 없거나(All projects) 체크아웃이 원격 디바이스의 것이면 글리프 클릭은 기본 브라우저로 연다; 그 외에는 항상 hide 브라우저 탭. | 가정 (qa-log D-11) |
| D-12 | 가정: PR이 있는 체크아웃의 카드도 D-09와 같은 출처·생략 규칙: Branch = worktree.branch(detached면 "Detached HEAD" + 짧은 head_sha), Agents = agent_summary(합 0이면 생략), Commit = last_commit_unix_seconds의 상대 나이(안 읽혔으면 생략), Path = checkout.path(항상). 값 없는 줄은 자리 없이 빠진다. | 가정 (qa-log D-12) |
| D-10 | 원칙 intake: engineering/principles.md와 design/principles.md(oh-my-principle 654485f)를 읽었다. design 4·7·9·10이 D-03·D-08·D-09에, engineering 7이 D-02·D-08에 반영됐다. | 가정 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | PR이 있는 체크아웃의 글리프에 마우스를 올리면 PR 색의 ring이 생기고 커서가 포인터가 된다; 클릭하면 앞 워크스페이스의 View 영역에 그 PR 페이지가 브라우저 탭으로 열린다(이미 같은 URL 탭이 있으면 그 탭이 앞에 온다). | D-02 |
| B2 | 글리프를 ⌘클릭하면 기본 브라우저에서 PR이 열리고 hide 안에는 아무 탭도 생기지 않는다. | D-02 |
| B3 | 글리프 클릭은 행을 열지 않는다(Workspace 전환·펼침 없음); PR 없는 체크아웃의 글리프는 ring도 포인터도 없고 클릭이 행 클릭으로 간다. | D-02 |
| B4 | 앞 워크스페이스가 없거나(All projects 화면) 원격 체크아웃이면 글리프 클릭은 기본 브라우저로 연다. | D-11 |
| B5 | 체크아웃 행에 hover하거나 포커스하면 지연 뒤 카드가 열린다: 배지(D-08 색·라벨), #번호, "Open PR ↗", 제목 두 줄, 구분선, Review/Checks/Branch/Agents/Commit/Path 줄. 각 줄은 D-08·D-12의 출처에 값이 있을 때만 있다. | D-03, D-08, D-12 |
| B6 | Review 줄은 Approved(success)/Changes requested(destructive)/Review required(muted); Checks 줄은 Passing(success)/Failed(destructive)/Pending(muted); Draft PR은 배지가 Draft이거나 review 배지 옆에 Draft가 덧붙는다. | D-08 |
| B7 | Agents 줄은 사이드바 배지와 같은 마크·색으로 상태별 개수를 보이고, Commit은 마지막 커밋 나이, Path는 mono muted 전체 경로다. | D-03, D-12 |
| B8 | PR 없는 체크아웃은 헤더 없는 카드(Branch/Agents/Commit/Path 중 있는 것); missing은 "Folder missing" 헤더와 Path; 값이 하나뿐이면 텍스트 tooltip. | D-09 |
| B9 | 카드 위로 마우스를 옮겨도 카드가 유지되고 "Open PR ↗" 클릭은 B1·B2와 같다; 행에서 벗어나거나 Escape로 닫힌다. 스크린리더는 지금과 같은 detail 문장을 읽는다. | D-04 |
| B10 | Checkout 우클릭 메뉴에 "Open pull request #n"이 있고(PR 없으면 없음) 동작은 B1과 같다. | D-07 |
| B11 | Pen 라이브러리에 "Component / PR hover card"가 있고 Projects Sidebar 시트에 hover 상태가 있으며, check-design-contract와 gen-screens가 통과한다; UI_BEHAVIOR.md 420-424행 근처가 새 동작을 서술한다. | D-05 |

## Technical structure

- web: `sidebar.tsx` 체크아웃 행의 글리프를 버튼으로, 새 `components/pr-card.tsx`, `projects.ts`의 카드 행 규칙과 `pullRequestBadge` 확장, `workspaceManage.ts` 메뉴 항목; 기존 `browser_open` 액션과 tooltip 컴포넌트 재사용.
- design: 라이브러리에 Component 1개, Screen 시트 상태 1개.
- 바뀌지 않음: core, Herdr 계약, 스냅샷 필드.

## Risks

- design/hide-ui.lib.pen 편집은 Pen CLI 저장이 무관한 노드를 망가뜨린 전력이 있어(PR #168) 스크립트 이식과 HEAD 구조 diff가 필수다.
- 외부 브라우저 열기는 사용자의 클릭에만 반응하고 URL은 스냅샷의 PR url만 쓴다.
- 사용자가 미리 해야 할 일: 없음.
