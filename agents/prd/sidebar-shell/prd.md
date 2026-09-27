---
topic: "사이드바 셸: 전역 Overview 목록, Projects | Agents 탭 스트립과 🔍/+ 아이콘"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "왼쪽 pane의 상단 구성을 바꾸는 사용자 대면 UI 변경이며, 기존 검색·새 워크스페이스 동작을 다른 자리로 옮길 뿐 데이터·권한·외부 효과는 없다."
source_intake: "agents/interview/sidebar-shell/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: 사이드바 셸

## Goal

hide 사용자가 왼쪽 pane 맨 위에서 전역 목적지(지금은 Overview 하나)를 고르고, 그 아래 Projects | Agents 탭으로 뷰를 고르며, 탭 스트립 오른쪽의 🔍와 +로 검색과 새 워크스페이스를 연다.
사용자의 말: "All Projects가 있고 그 밑에 Projects | Agents 이렇게 해서 탭 형태로 view 보여주게 (Projects가 왼쪽에 default로)", "All Projects보다 Overview로 우선 있는게 나을것같누.. 나중에 다른 기능들 있으면 추가하려고", "둘 다 Overview로 가자".
시각 참조는 채팅에서 확정한 구성(안 A):

```
┌──────────────────────────┐
│ ⌂ Overview            12 │  전역 목적지 목록 (고정, 지금은 한 행)
├──────────────────────────┤
│ Projects  Agents    🔍 + │  탭 스트립; 🔍 = 검색, + = 새 워크스페이스(Projects 탭만)
├──────────────────────────┤
│ ▾ hide            ●2 ○1  │
│     Overview             │  프로젝트 안의 Overview 행(#198)
│     main                 │
├──────────────────────────┤
│ [mini ▾]      67%    ⚙   │  푸터 그대로
└──────────────────────────┘
```

## Non-goals

- 푸터(디바이스 피커, 주간 사용량, 설정)와 Herdr 상태 줄은 바꾸지 않는다 (D-05).
- 행 우클릭 메뉴는 T6의 것이다; 이 PR은 `workspaceManage.ts`를 건드리지 않는다 (D-10).
- 두 번째 전역 목적지(Task Automation 등)는 이 PR에 없다; 목록 구조만 그 자리를 준다. 재검토: 그런 기능의 PRD가 승인될 때 그 PRD가 행을 추가한다 (D-02).
- Agents 탭의 오른쪽 빈 자리(미래 필터)는 비워 둔다 (D-04).
- 라이브러리 Component는 바꾸지 않는다; Screen 시트만 다시 그린다 (D-08).
- design/principles.md 규칙 2·5·8과 engineering/principles.md 규칙 1이 D-02·D-03·D-08을 정한다 (D-11).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 지금 pane은 위에서부터 Projects/Agents 텍스트 토글(오른쪽에 toggle_sidebar_view chord), SearchField 행(클릭 = 검색 오버레이, chord ⌘K), Herdr 상태 줄, 목록(Projects 목록 첫 행 = "All projects" + 프로젝트 수), "+ 새 워크스페이스" 풀폭 버튼, 푸터다. All projects 화면은 `MainScreen.tsx`. | 저장소 사실 (qa-log D-01) |
| D-02 | 맨 위 = 전역 목적지 목록, 지금은 "⌂ Overview" 한 행(= All projects 화면, 프로젝트 수 유지); 미래 기능은 여기에 행으로 추가. 그 화면이 앞이면 선택 fill. All projects 화면 제목도 "Overview". 프로젝트 안의 Overview 행과 이름이 같아도 둘 다 Overview. | 사용자 (qa-log D-02) |
| D-03 | 그 아래 Projects \| Agents 탭 스트립(Projects 기본), 오른쪽에 🔍(검색 오버레이, Search 행 대체)와 +(새 워크스페이스, Projects 탭만). Search 필드 행과 하단 "+ 새 워크스페이스" 버튼 삭제. | 사용자 + Observer 안 A (qa-log D-03) |
| D-04 | Agents 탭에서는 🔍만; + 자리는 비움. 탭 선택은 ui store sidebarMode 그대로; toggle_sidebar_view chord 표기는 스트립에서 빼고 hover Hint로. | 가정 (qa-log D-04) |
| D-05 | Herdr 상태 줄은 탭 스트립 아래·목록 위 그대로; 빈 목록의 "Add project" 버튼 그대로; 푸터 불변. | 가정 (qa-log D-05) |
| D-06 | Overview 행 = 프로젝트 행과 같은 높이·글꼴·포커스 링의 내비게이션 행: 집 글리프, "Overview", 오른쪽 프로젝트 수. 목록 스크롤 밖에 고정. | 가정 (qa-log D-06) |
| D-07 | Overview 행은 Enter/Space로 열림; 탭은 aria-pressed 버튼; 🔍·+는 Hint에 제목과 chord("Search ⌘K", "New workspace" + 레지스트리 chord). 원격 디바이스 선택 중에도 같은 구성. | 가정 (qa-log D-07) |
| D-08 | Pen: 라이브러리 불변(텍스트 탭, icon-sm 버튼 마스터 재사용); `pen-screens.mjs`의 Screen / Projects Sidebar 시트를 새 구성으로 다시 그리고 gen-screens. 참조는 위 ASCII 구성. | 가정 (qa-log D-08) |
| D-09 | 검증: vitest(헤더 구성, 선택 상태, + 노출 규칙, Search 필드 없음, 제목), 웹 e2e 1개, gen-screens·check-design-contract, "All projects" 문자열을 찾는 기존 테스트 갱신, UI_BEHAVIOR.md 갱신. | 가정 (qa-log D-09) |
| D-10 | T3(#206)·T7 머지 후 시작. please, Claude Implementor --effort high, PR 배포, Observer 자동 머지. T6와 병렬, `workspaceManage.ts` 불변. | 사용자 승인 (qa-log D-10) |
| D-11 | 원칙 intake: design 2·5·8과 engineering 1이 D-02·D-03·D-08에 반영. | 가정 (qa-log D-11) |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 왼쪽 pane 맨 위에 집 글리프, "Overview", 오른쪽에 프로젝트 수가 있는 행이 목록 스크롤과 무관하게 항상 보이고, 클릭·Enter·Space로 Overview 화면(지금의 All projects)이 열린다. | D-02, D-06, D-07 |
| B2 | Overview 화면이 앞에 있으면 그 행이 선택 fill을 갖고, 프로젝트나 Workspace가 앞이면 갖지 않는다; 화면 제목은 "Overview"다. | D-02 |
| B3 | 그 아래 스트립에 Projects, Agents 텍스트 탭이 있고 처음엔 Projects가 선택돼 있으며, 탭을 누르면 아래 목록이 바뀐다; 선택은 지금처럼 세션 동안 유지된다. | D-03, D-04 |
| B4 | 스트립 오른쪽 끝에 🔍 아이콘이 있고 누르면 지금의 검색 오버레이가 열린다; hover하면 "Search ⌘K"가 보인다. 예전 Search 필드 행은 없다. | D-03, D-07 |
| B5 | Projects 탭일 때 🔍 왼쪽에 + 아이콘이 있고 누르면 지금의 새 워크스페이스 흐름이 열린다; hover에 "New workspace"와 chord가 보인다. Agents 탭에서는 + 자리가 비어 있다. 예전 하단 "+ 새 워크스페이스" 버튼은 없다. | D-03, D-04 |
| B6 | Herdr 상태 줄(로컬)은 스트립 아래·목록 위에 그대로 있고, 프로젝트가 없을 때의 "Add project" 버튼도 목록 안에 그대로 있다. 푸터는 바뀌지 않는다. | D-05 |
| B7 | Projects 목록 안에는 더 이상 "All projects" 행이 없다; 프로젝트 행부터 시작한다. | D-02 |
| B8 | 원격 디바이스를 선택해도 같은 구성이고, +는 지금 규칙대로 동작한다. | D-07 |
| B9 | ⌘K(검색)와 새 워크스페이스 chord는 그대로 동작하고, 단축키 시트의 항목도 그대로다. | D-03 |
| B10 | Screen / Projects Sidebar 시트가 새 구성을 보이고 gen-screens·check-design-contract가 통과하며, UI_BEHAVIOR.md의 사이드바 계층·scope 문장이 "Overview"로 서술된다. | D-08, D-09 |

## Technical structure

- web: `sidebar.tsx` 상단 구성(전역 목록 + 탭 스트립 + 아이콘; SearchField·NewWorkspace 버튼 삭제), `MainScreen.tsx` 제목, `projects.ts`/`navigation.ts`의 All projects 선택 규칙 이름, 관련 vitest·e2e 문자열, `pen-screens.mjs`.
- 바뀌지 않음: core, Herdr 계약, 레지스트리 chord, `workspaceManage.ts`, 푸터.

## Risks

- "All projects"를 찾는 기존 e2e가 여럿이라 문자열 갱신이 빠지면 CI가 깨진다; 첫 단계에서 `rg "All projects" web/` 전수 갱신.
- T6가 같은 `sidebar.tsx`의 행 부분을 고치므로 머지 순서에 따라 한 번 재병합이 필요할 수 있다.
- 사용자가 미리 해야 할 일: 없음.
