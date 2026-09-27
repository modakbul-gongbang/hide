---
topic: "사이드바: 체크아웃 행 클릭으로 펼치기, 프로젝트 아래 Overview 행"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "사이드바의 행 클릭 의미를 바꾸는 사용자 대면 UI 변경이며 PR #167의 계약을 번복하지만, 데이터·권한·외부 효과는 없다."
source_intake: "agents/interview/sidebar-row-click-overview-row/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: 사이드바 체크아웃 행 클릭 펼침과 Overview 행

## Goal

hide 사용자가 Projects 사이드바에서 체크아웃 행을 클릭하면 그 체크아웃이 열리면서 에이전트 행이 함께 펼쳐지고, 펼친 프로젝트의 맨 위에서 Overview 행으로 프로젝트 Overview에 간다.
사용자의 말: "체크아웃 row 클릭하면 나오게 해줘", "Overview가 하나는 있게 보여지면 좋을 것 같아".
시각 참조는 `agents/runs/ux-fixes-2026-09-27/design/board-v3.pen` 섹션 1(A·B열)이다.

## Non-goals

- 프로젝트 이름 행의 동작(Overview 열기, chevron으로 체크아웃 접기)은 바꾸지 않는다 (D-03).
- 플레인 폴더 프로젝트의 한 행에는 Overview 행을 넣지 않는다; Overview는 팔레트·툴바에서 간다. 재검토: 폴더 프로젝트에 Overview가 필요하다는 요청이 올 때 (D-03).
- 에이전트 행 자체의 내용·접기(부모 에이전트의 자식 접기)는 바꾸지 않는다.
- design/hide-ui.lib.pen의 Component는 바꾸지 않는다; Screen 시트만 갱신한다 (D-06).
- Electron 전용 검증은 없다; 호스트와 무관한 동작이다 (D-07).
- design/principles.md 규칙 3(가장 잦은 동작이 가장 적은 클릭)이 D-02의 근거이고, 규칙 5(기존 패턴)가 Overview 행을 체크아웃 행 마스터의 인스턴스로 그리게 한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 지금 체크아웃 행 전체는 열기이고 chevron만 펼치기다; 펼침 상태는 core의 `expanded_checkout_ids`가 소유한다; 프로젝트 이름 행은 Overview를 연다. | 저장소 사실 (qa-log D-01: web/src/sidebar.tsx, UI_BEHAVIOR 277·401·410) |
| D-02 | 체크아웃 행 클릭 = 열기 + 에이전트 펼치기. 이미 열려 있고 펼쳐진 체크아웃의 행을 다시 클릭하면 접는다(열림 유지). chevron은 그대로 접기/펼치기만. 에이전트가 없는 체크아웃은 열기만. PR #167의 "행은 열기만" 결정을 번복한다. | 사용자: "저 체크아웃 row 클릭하면 나오게 해줘!" (qa-log D-02) |
| D-03 | 펼친 프로젝트의 체크아웃 목록 첫 행에 Overview 행(layout-dashboard 글리프, 체크아웃 행의 열)을 둔다. 클릭하면 Project Overview. Overview가 앞이면 선택 fill은 이 행이 받고 프로젝트 이름 행은 받지 않는다. 플레인 폴더에는 없다. | 사용자: "Overview가 하나는 있게 보여지면 좋을 것 같아!!!" (qa-log D-03) |
| D-04 | Overview 행은 체크아웃 행과 같은 높이·포커스 링·키보드 동작(Enter/Space로 열기)을 가지며 배지·시간·chevron 슬롯은 비어 있다. 행 클릭 펼침은 마우스와 Enter/Space가 같고 다른 행을 움직이지 않는다. | 가정 (qa-log D-04) |
| D-05 | 펼침 상태는 그대로 core가 소유한다. 행 클릭은 열기 이벤트 하나에 펼침 의도를 함께 실어 보낸다(한 동작 = 한 이벤트); 웹이 두 이벤트를 연달아 보내지 않는다. core 변경은 그 이벤트의 페이로드 확장뿐. | 가정; AGENTS.md "A user action is one event, not a sequence" (qa-log D-05) |
| D-06 | Pen: Screen / Projects Sidebar 시트에 Overview 행을 넣고(gen-screens), 라이브러리는 바꾸지 않는다. UI_BEHAVIOR.md 401·407·410행과 시트 설명을 새 동작으로 고친다. | 가정 (qa-log D-06) |
| D-07 | 검증: vitest(행 클릭 규칙, 선택 규칙), 웹 e2e 1개(클릭 → 열림+펼침, 재클릭 → 접힘, Overview 행 → Overview, 선택 fill), gen-screens diff. | 가정 (qa-log D-07) |
| D-08 | 실행: please, Claude Implementor --effort high, PR 배포, Observer 자동 머지. T0 머지 후 main에서 시작. | 사용자 (qa-log D-08) |
| D-10 | 여러 체크아웃이 동시에 펼쳐진 채 남을 수 있다(지금의 `expanded_checkout_ids` 집합 그대로); 다른 체크아웃 행을 클릭해도 이전에 펼친 것은 접히지 않고, 접기는 그 행의 재클릭이나 chevron만 한다. | 가정 (qa-log D-10) |
| D-09 | 원칙 intake: engineering/principles.md와 design/principles.md(oh-my-principle 654485f)를 읽었다. design 규칙 3·5가 D-02·D-03에, engineering 규칙 1이 "행은 열기만" 코드 경로 삭제에 반영됐다. | 가정 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 접힌 체크아웃 행(에이전트 있음)을 클릭하면 그 체크아웃의 Workspace가 앞에 오고 같은 프레임에 에이전트 행이 펼쳐지며, 행은 선택 fill과 펼침 그룹 fill을 갖는다. | D-02, D-05 |
| B2 | 이미 앞에 있고 펼쳐진 체크아웃 행을 다시 클릭하면 에이전트 행이 접히고 Workspace는 그대로 앞에 남는다; 한 번 더 클릭하면 다시 펼쳐진다. 다른 체크아웃을 클릭해도 이전에 펼친 체크아웃은 펼쳐진 채 남는다. | D-02, D-10 |
| B3 | chevron 클릭은 지금처럼 펼침만 토글하고 Workspace를 바꾸지 않는다; 에이전트가 없는 체크아웃은 행 클릭이 열기만 하고 chevron 슬롯은 비어 있다. | D-02 |
| B4 | 키보드로 체크아웃 행에 포커스하고 Enter 또는 Space를 누르면 B1·B2와 같다. | D-04 |
| B5 | 펼친 프로젝트(Git)의 체크아웃 목록 첫 행은 "Overview"이고, 클릭, Enter 또는 Space로 그 프로젝트의 Overview가 열린다. 플레인 폴더 프로젝트에는 이 행이 없다. | D-03 |
| B6 | Overview가 앞에 있을 때 Overview 행이 선택 fill을 갖고 프로젝트 이름 행은 갖지 않는다; 체크아웃 Workspace가 앞이면 지금처럼 그 체크아웃 행만 선택 fill을 갖는다. | D-03 |
| B7 | Overview 행은 체크아웃 행과 같은 높이·글꼴·포커스 링이며 배지·시간·chevron이 없다; 프로젝트를 접으면 함께 사라진다. | D-03, D-04 |
| B8 | 행 클릭 펼침과 Overview 행 추가로 다른 행의 높이나 위치가 바뀌지 않는다(펼친 에이전트 행이 아래 행을 미는 것 외). | D-04 |
| B9 | 웹 셸이 보내는 이벤트는 행 클릭당 하나이고, Herdr가 열기를 거부하면 펼침도 일어나지 않는다(반쯤 움직인 화면 없음). | D-05 |
| B10 | UI_BEHAVIOR.md와 Screen / Projects Sidebar 시트가 새 동작을 서술하고, gen-screens 산출물에 Overview 행이 있다. | D-06 |

## Technical structure

- core: 체크아웃 열기 이벤트의 페이로드에 펼침 의도 추가(`expanded_checkout_ids` 소유는 그대로); 다른 도메인 변경 없음.
- web: `sidebar.tsx`의 체크아웃 행 클릭 핸들러와 선택 규칙(`projects.ts`), Overview 행 렌더; `pen-screens.mjs`의 Projects Sidebar 빌더.
- 바뀌지 않음: 라이브러리 Component, 프로젝트 행, 에이전트 행, Herdr 계약.

## Risks

- 행 클릭이 열기와 펼침을 함께 하므로 "열기만" 하고 싶은 사용자는 chevron이 없는 대안이 없다; D-02의 결정이고 되돌릴 수 있다.
- 다른 wave-1 PR(T1: WorkspaceScreen/actions, T4: TabBar)과 파일이 겹치지 않는다; T3·T7이 이 PR 뒤에 온다.
- 사용자가 미리 해야 할 일: 없음.
