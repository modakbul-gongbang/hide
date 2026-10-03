---
topic: "Overview를 프로젝트 소속이 아닌 하나의 모달/페이지로 열고 사이드바 행과 단축키로 들어가기"
status: "ready"
human_approval: "approved"  # user 2026-10-03 verbatim: mini 소컷 수정지시 어디서 되고잇어? 아직 안햇나? 349 350은 승인안됏으면 해버려
review_profile: "standard"
review_rationale: "웹 셸의 화면 전환, 사이드바, 툴바, 단축키 레지스트리와 ⌃Tab 순환 명세를 바꾸는 사용자 가시 UI 변경이며, core 상태, wire, Herdr 계약, 저장 데이터는 바꾸지 않는다."
source_intake: "agents/interview/overview-modal/qa-log.md"
created_at: "2026-10-03"
updated_at: "2026-10-03"
---

# PRD: Overview를 프로젝트 소속이 아닌 하나의 모달/페이지로 열고 사이드바 행과 단축키로 들어가기

## Goal

여러 프로젝트에서 에이전트를 돌리는 운영자가 지금 내 차례인 에이전트가 있는지 전체를 한눈에 보고 싶을 때, 지금은 Overview가 프로젝트마다 사이드바의 자식 행으로 숨어 있고, Overview에 들어가면 `⌃Tab`이 동작하지 않으며, 툴바에는 읽기 어려운 뱃지가 있다(이슈 #349).
확인할 목표 문장: "Overview는 프로젝트 소속이 아닌 하나의 화면이고, 사이드바 맨 위 Overview 행, 툴바 아이콘, ⌘⇧O로 열며, Workspace 위에서는 모달로, 아무것도 없으면 중앙 페이지로 보이고, 어느 쪽이든 ⌃Tab으로 가장 최근 에이전트 pane에 돌아간다."
사이드바 Projects와 Agents 탭도 각자의 직접 키(⌘⇧P, ⌘⇧A)를 갖는다. 프로젝트별 Overview 제거(#350)가 이 화면을 전제로 하므로 먼저 정한다.

## Non-goals

- Overview 안의 구성(타일, Agents 그래프, Issues, PRs와 Sessions 렌즈 배치, 시안의 C1/C2/C3)은 바꾸지 않는다(D-08). 새 틀 안에는 오늘의 내용이 그대로 들어간다. 운영자가 내부 구성을 다시 생각한 뒤 별도 이슈로 연다.
- 프로젝트별 Overview 화면, 프로젝트 아래 `Overview` 자식 행, 프로젝트 행 클릭은 바꾸지 않는다(D-09). 이 PRD만 들어가면 프로젝트별 Overview는 그대로 남고, 같은 정보를 단일 Overview의 현재 프로젝트 범위로도 볼 수 있다. #350이 그것을 지우며, #349가 먼저이거나 함께 들어간다.
- Home 행을 Home Workspace로 바꾸는 시안 문장("Home is only the Home workspace")은 넣지 않는다(D-17). 운영자가 #350 PRD나 Pen 보드 승인 때 원하면 다시 연다.
- Windows와 Linux의 키 매핑은 PR #347이 맡는다(D-10). 이 PRD는 레지스트리의 chord 모델만 쓰고 새 chord는 그 규칙을 따라 옮겨진다.
- ⌘E, ⌘B, ⌘⇧B의 의미는 바꾸지 않는다(D-03). main의 three-column-panel PRD가 정한 대로 ⌘⇧B는 File Views, ⌘E는 Tools이며, 그 구현이 먼저 들어오든 나중에 들어오든 이 PRD는 그 정의를 따른다.
- 모든 기기를 한 화면에 섞는 Overview는 만들지 않는다. All projects는 오늘 Home Overview처럼 앞에 있는 기기의 프로젝트다(D-12). 운영자가 기기 횡단 조감을 요청하면 다시 본다.
- core 상태, `contracts/hided-ws.schema.json`, Herdr API 계약과 pin, 저장 파일은 바꾸지 않는다(D-22).
- `.pen` 파일을 이 PRD 작업에서 손으로 고치지 않는다. 보드는 D-21 절차를 따른다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Overview는 프로젝트 소속이 아닌 하나의 모달/페이지다. 진입점은 사이드바 맨 위, Projects \| Agents 탭 위의 글자가 있는 `Overview` 행(내 차례 수와 단축키 포함)과 툴바의 글자 없는 작은 아이콘(내 차례가 있으면 점)이다. | 이슈 #349 "사이드바 맨 위에 글자가 있는 `Overview` 행을 둡니다", "툴바에는 작은 아이콘만 두고, 내 차례가 있으면 아이콘에 점을 찍습니다" |
| D-02 | 같은 Overview가 Workspace가 앞에 있으면 그 위의 모달로, 중앙에 Workspace가 없으면 중앙 페이지로 보인다. 범위는 All projects와 현재 프로젝트를 전환한다. | 이슈 #349 "아무것도 열려 있지 않으면 같은 Overview가 중앙 페이지로 보입니다", "범위는 All projects와 현재 프로젝트를 전환해서 볼 수 있습니다" |
| D-03 | 단축키는 토글이 아니라 각자 직접 키다: ⌘⇧O Overview 열기/닫기(⌘⇧H에서 이동), ⌘⇧P 사이드바 Projects 탭, ⌘⇧A 사이드바 Agents 탭. ⌘E, ⌘B, ⌘⇧B는 그대로이며 three-column-panel PRD를 따른다. | 이슈 #349 단축키 목록; 시안 "토글은 지금 어느 쪽인지 알아야 눌러서 헷갈립니다"; agents/prd/three-column-panel D-05 |
| D-04 | 사이드바 Projects / Agents 탭과 Overview 행 옆에 단축키를 보여 주고, Projects / Agents가 탭처럼 보이게 한다. | HANDOFF 설계 논의 결정; 운영자가 검토한 시안 보드(탭 옆 `⌘⇧P`, `⌘⇧A` keycap) |
| D-05 | Overview 모달이나 페이지가 열려 있어도 ⌃Tab이 가장 최근 에이전트 pane으로 간다. `docs/UI_BEHAVIOR.md`의 "Overview supplies no scope" 명세와 그 테스트를 바꾼다. | 이슈 #349 "Overview 모달이나 페이지가 열려 있어도 `⌃Tab`이 가장 최근 에이전트 pane으로 이동합니다"; #306 |
| D-06 | 툴바 Overview 아이콘의 점은 열린 View 개수 뱃지와 나란히 놓지 않는다. | 이슈 #349 "열린 View 개수 뱃지와 나란히 놓지 않습니다" |
| D-07 | Overview 아이콘은 툴바 오른쪽 아이콘 묶음의 맨 왼쪽, Open server(globe) 앞에 둔다. Open server가 점과 개수 뱃지 사이에 있어 둘이 붙지 않는다. three-column-panel의 File Views 개수 뱃지(B9)는 유지한다. 검토 시안은 globe, Overview, 패널 순서였고 개수 뱃지를 뺀 그림이었다. | 가정: 위임된 배치, 운영자가 Pen 보드에서 거부할 수 있음 |
| D-08 | Overview 안의 구성은 범위 밖이며 오늘의 내용을 새 틀에 그대로 담는다. | 이슈 #349 "Overview 안의 구성(타일, 그래프, PR 목록)은 이 이슈에 포함하지 않습니다" |
| D-09 | 프로젝트별 Overview 제거와 프로젝트 행 클릭은 #350의 별도 PRD다. #349는 혼자 서고, 순서는 #349가 먼저이거나 함께다. | 이슈 #350 Related "Overview를 모달/페이지로 여는 이슈가 먼저 또는 함께 들어가야 합니다" |
| D-10 | Windows/Linux 키 매핑은 PR #347(HEAD에 없음)이 맡고 이 PRD는 레지스트리 chord 모델만 쓴다. | 사실: `web/src/shortcuts.ts` REGISTRY, 브랜치 `ui/platform-shortcuts` 커밋 fa856ffb |
| D-11 | ⌘⇧O, ⌘⇧P, ⌘⇧A는 REGISTRY, `CHROME_RESERVED`, `MACOS_RESERVED`, Electron 메뉴 accelerator 어디에도 쓰이지 않는다. macOS 시스템 단축키와 다른 앱과의 충돌은 확인하지 않았다. | 사실: `web/src/shortcuts.ts:150-209,373`, `desktop/src/main/menu.ts:50-92`; 이슈 #349 Review |
| D-12 | All projects 범위는 오늘 기기 Home Overview(앞에 있는 기기의 프로젝트, Tasks · Agents · Projects)의 내용, 현재 프로젝트 범위는 오늘 그 프로젝트 Overview(Agents, Issues, PRs, Sessions 타일)의 내용이다. 앞에 있는 Workspace가 프로젝트에 속하지 않거나(Home) 없으면 프로젝트 선택지는 없다. | 가정: `web/src/MainScreen.tsx`, `web/src/ProjectOverview.tsx`를 그대로 담는 가장 단순한 대응 |
| D-13 | 범위 선택은 창마다의 페이지 상태로 세션 동안 유지되고 처음 열면 All projects다. 저장하지 않는다. | 가정: 이슈의 목적이 전체 조감 |
| D-14 | 내 차례 수는 앞에 있는 기기의 Needs You 에이전트 수이며 사이드바 `Needs You · N`과 같은 수다. 0이면 수도 점도 그리지 않는다. 문구는 승인된 보드를 따른다(검토 시안은 `N asking`). | 가정: `docs/status-model.md` Needs You 정의; 설계 원칙 13 |
| D-15 | 모달은 창 가운데에 뜨고 배경이 사이드바와 Workspace를 어둡게 덮는다. Escape(안쪽 층부터), 배경 클릭, ⌘⇧O, 툴바 아이콘으로 닫히고 키보드는 열기 전 주인에게 돌아간다. 뒤의 Workspace는 마운트된 채라 터미널이 리사이즈되지 않고, 네이티브 페이지는 기존 대화상자 오버레이 규칙대로 still이 된다. 모달 안에서 중앙을 옮기는 동작은 같은 한 번에 모달을 닫는다. | 가정: 기존 Radix 대화상자 primitive와 `docs/BROWSER_DISPLAYS.md` Overlays 재사용 |
| D-16 | 페이지 표현은 중앙에 Workspace가 없을 때(오늘 main 화면 자리) 쓰인다. Workspace가 아닌 화면(프로젝트별 Overview 포함)에서 ⌘⇧O나 Overview 행은 페이지를 보이고, 페이지에서 ⌘⇧O는 앞에 있는 Workspace가 있으면 그리로 돌아가고 없으면 아무것도 하지 않는다. | 가정: `web/src/App.tsx` CenterScreen, `navigation.ts` startupScreen 경로 유지 |
| D-17 | Home 행은 지금처럼 자기 기기의 Overview 페이지를 All projects 범위로 중앙에 연다(Workspace가 앞에 있어도 페이지). Overview가 앞에 있는 동안 선택 표시는 Overview 행이 갖고 Home 행은 갖지 않는다. 영향은 Home 행 클릭과 선택 표시뿐이다. | 가정: 오늘 경로 유지; 시안 문장 "Home is only the Home workspace"는 #350이나 보드 승인 때 다시 봄 |
| D-18 | ⌘⇧P/⌘⇧A는 사이드바가 숨어 있으면 보이게 하고 그 탭을 고르고 키보드를 그 목록으로 옮기며, 모달이 열려 있으면 먼저 닫는다. 같은 키를 다시 눌러도 같은 상태다. `toggle_sidebar_view`는 없애고 저장된 그 바인딩은 다른 바인딩을 잃지 않고 무시된다. 두 새 명령은 Settings에서 바꿀 수 있다. Overview 명령은 `project_home`의 ⌘⇧H 대신 ⌘⇧O를 받고, `project_home`은 기본 chord 없이 ⌘K, View 메뉴, 툴바 메뉴로 남는다(#350까지). | 가정: engineering 원칙 1(토글 명령 제거), 11(반복 수렴) |
| D-19 | Overview에서 ⌃Tab은 Agent pane 순환을 쓰고 첫 chord가 가장 최근 에이전트 pane을 가리킨다. 확정하면 모달을 닫거나 페이지를 떠나며 그 pane을 한 이벤트로 앞에 가져오고 키보드를 준다. Escape는 취소하고 Overview는 그대로다. 방문한 에이전트 pane이 없으면 아무 일도 없다. 모달은 Global Recent Panels의 방문이 아니다. | 가정: `web/src/areaCycle.ts:18,83,101-109`의 키보드 없는 시작 규칙 재사용 |
| D-20 | Overview 모달은 다른 대화상자 위에 쌓이지 않는다. ⌘K 팔레트가 열려 있을 때 ⌘⇧O는 팔레트를 검색어와 함께 닫고 같은 동작에서 Overview를 열며 키보드는 Overview 안으로 간다. 그 Overview를 닫으면 키보드는 팔레트를 열기 전 주인에게 돌아간다. Settings나 다른 대화상자가 열려 있으면 ⌘⇧O는 아무것도 하지 않는다. Overview 안에서 여는 대화상자는 모달 위에 열리고 Escape는 안쪽부터 닫는다. | 가정: `web/src/keyboard.ts:365-417`는 대화상자 중 chord를 막지 않음 |
| D-21 | 구현 전에 운영자가 승인한 Pen 보드가 필요하다. 운영자는 `home-final.html`의 보드(사이드바 Overview 행, 단축키가 붙은 탭, 점이 있는 툴바 아이콘과 hover 툴팁, 두 범위의 모달, 아무것도 없을 때의 페이지)를 보고 방향을 말로 승인했다. 구현자는 그 보드를 `design/hide-screens.pen`으로 옮기고 시안에 없는 상태(D-07 순서, 수 0, 라이트 테마, 좁은 창, 선택된 탭 모양)를 운영자에게 확인받는다. | HANDOFF "the operator approved the direction verbally"; `docs/DESIGN_WORKFLOW.md` flow 4 |
| D-22 | 어떤 화면이 보이는지와 Overview의 보기는 `web/src/ui.ts`의 페이지 상태이며 core 상태가 아니다. 내 차례 수는 스냅샷의 에이전트 group에서 이미 나온다. core, wire, Herdr 계약 변경은 없다. | 사실: `docs/ARCHITECTURE.md:684-686`, `web/src/ui.ts:74-81` |
| D-23 | 이 실행은 PRD만 담은 PR 하나를 연다. 구현은 운영자가 PRD와 Pen 보드를 승인한 뒤 `agents/config.json`의 PR 모드로 하고, merge는 어느 단계에서도 에이전트가 하지 않는다. | 운영자 "349 PRD작성 ㄱㄱ"; HANDOFF "Do NOT implement, do NOT merge"; agents/config.json delivery.mode=pr |
| D-24 | 구현 PR은 같은 변경에서 `docs/UI_BEHAVIOR.md`의 Web Workspace 툴바 문단, Scopes(⌘⇧H 문장과 Escape 문장), 사이드바 맨 위 두 줄과 선택 표시 문장, Project Home의 진입 목록, Recent navigation의 "Overview supplies no scope" 문장을, `docs/ARCHITECTURE.md`의 web shell 화면 문단과 단축키 표의 Project home 행을 고친다. | AGENTS.md "Update the owning guide ... in the same change" |
| D-25 | 원칙 intake: `sasu principles list`는 `~/projects/oh-my-principle`에 ROOT.md가 없어 실패했다. 그 저장소 커밋 8b0d709의 `engineering.md` 전문을 읽었다. 1(D-18 토글 명령 제거), 2(D-12 오늘 화면을 그대로 담음), 4와 10(B26의 버린 바인딩 진단), 11(B23, B21)을 반영했다. 12는 구현의 테스트 선택이라 행으로 옮기지 않았다. design 원칙 문서는 이 machine에 없어 HANDOFF가 적은 원칙 5, 11, 13을 썼다. | 가정: 원칙 저장소 부분 사용 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 사이드바 맨 위, 기기 이름 줄 아래이자 Projects \| Agents 탭 줄 위에 layout-dashboard glyph와 `Overview` 글자, 내 차례 수, `⌘⇧O` keycap이 있는 한 줄 행이 있고, Projects 탭과 Agents 탭 어느 쪽에서도 같은 자리에 보인다. | D-01, D-04 |
| B2 | Overview 행의 수는 앞에 있는 기기의 Needs You 에이전트 수로 사이드바 `Needs You · N`과 같고, 에이전트가 Needs You에 들어가거나 빠진 스냅샷과 같은 프레임에 바뀐다. 0이면, 첫 스냅샷 전이면, 연결되지 않은 기기면 수를 그리지 않는다. | D-01, D-14 |
| B3 | Overview 행을 클릭하거나 Enter, Space를 누르면 Workspace가 앞에 있을 때는 모달이, 아니면 페이지가 열린다. Overview가 앞에 있는 동안 이 행만 선택 fill과 현재 위치 표시를 갖고 Home 행은 갖지 않는다. 페이지가 이미 보일 때 다시 누르면 그대로다. | D-02, D-16, D-17 |
| B4 | Workspace 툴바에 글자 없는 작은 Overview 아이콘이 있고, 내 차례가 하나 이상이면 아이콘에 점이 찍히며 0이면 점이 없다. 아이콘의 접근성 이름은 `Overview`이고 내 차례 수가 접근성 설명으로 주어진다. 마우스를 올리거나 키보드 focus가 가면 툴팁에 `Overview`, 내 차례 수(0이면 없음), `⌘⇧O` keycap이 보인다. | D-01, D-14 |
| B5 | 툴바 오른쪽 아이콘은 Overview, Open server, 그다음 패널 아이콘(three-column-panel 구현 전에는 사이드 패널 토글, 구현 후에는 File Views, Tools) 순서다. Overview의 점과 열린 View 개수 뱃지 사이에는 언제나 Open server가 있어 둘이 붙어 보이지 않고, 툴바에 그 밖의 Overview 뱃지는 없다. | D-06, D-07 |
| B6 | Overview 아이콘을 누르면 그 Workspace 위에 Overview 모달이 열리고, 모달이 열린 동안 배경을 누르면 닫힌다. | D-02, D-15 |
| B7 | 모달은 창 가운데에 뜨고 반투명 배경이 사이드바와 Workspace를 덮는다. 머리 줄에 `Overview`, 범위 전환 `All projects \| <프로젝트 이름>`, `Esc` keycap이 있고, 내용은 모달 안에서 스크롤된다. 창을 좁혀도 모달은 창 안에 들어가고 가로 스크롤이 생기지 않으며, 긴 한글 프로젝트 이름은 범위 전환에서 잘리고 전체 이름은 툴팁으로 보인다. | D-02, D-15 |
| B8 | 모달이 열리고 닫히는 동안 뒤의 Agent 터미널은 크기가 바뀌지 않고 출력도 계속 흐르며, 닫으면 같은 탭, pane, View, 스크롤 위치가 그대로 보인다. | D-15 |
| B9 | 모달이 네이티브 페이지와 겹치면 그 페이지는 다시 로드되지 않고 still로 바뀌어 모달 아래에 보이며, 모달이 닫히면 같은 주소와 상태로 다시 살아난다. | D-15 |
| B10 | 모달은 대화상자 역할과 `Overview` 이름을 갖고, 열리면 키보드가 모달 안으로 들어가며 Tab이 뒤의 사이드바나 Workspace로 빠지지 않는다. | D-15 |
| B11 | All projects 범위는 오늘 Home Overview가 보이는 내용(앞에 있는 기기의 프로젝트, Tasks · Agents · Projects, Agents 탭의 내 차례 수, 제목 줄의 Add project와 새 이슈, 사실 줄)을 같은 동작으로 보여 준다. | D-08, D-12 |
| B12 | 현재 프로젝트 범위는 앞에 있는 Workspace의 프로젝트 이름으로 표시되고, 오늘 그 프로젝트 Overview의 사실 줄, 타일(Agents, Issues, PRs, Sessions)과 렌즈를 같은 동작으로 보여 주며, 이 범위로 바꿀 때 오늘 그 Overview를 열 때와 같은 배경 읽기를 한 번 보낸다. | D-08, D-12 |
| B13 | 범위 선택은 그 창에서 세션 동안 유지되어 닫았다 다시 열면 마지막 범위로 열리고, 처음 열면 All projects다. 다른 프로젝트의 Workspace로 옮긴 뒤 열면 프로젝트 선택지가 그 프로젝트 이름으로 바뀐다. | D-13 |
| B14 | 앞에 있는 Workspace가 Home이거나 Workspace가 없으면 범위 전환에 All projects만 있고, 마지막 선택이 프로젝트였어도 All projects로 열린다. | D-12, D-13 |
| B15 | 기기가 연결 중이거나 응답하지 않으면 모달과 페이지 모두 오늘 Overview처럼 그 이유를 보이고 수를 0으로 말하지 않으며, 다시 연결되면 그대로 채워진다. | D-12, D-14 |
| B16 | 모달은 Escape(검색어 지우기, 이슈 패널 닫기처럼 안쪽 층부터 하나씩), 배경 클릭, ⌘⇧O, 툴바 아이콘으로 닫히고, 닫히면 키보드는 열기 전 주인(pane, View, 사이드바)에게 돌아간다. | D-15, D-03 |
| B17 | 모달 안에서 에이전트 행, 체크아웃 머리, 프로젝트로 가는 링크처럼 오늘 중앙을 다른 화면으로 옮기는 동작을 하면 모달이 닫히고 중앙이 오늘과 같은 곳으로 한 번에 옮겨지며, 모달이 새 화면 위에 남은 프레임은 없다. | D-15 |
| B18 | ⌘K 팔레트가 열린 채 ⌘⇧O를 누르면 팔레트가 검색어와 함께 닫히고 같은 동작에서 Overview(Workspace 위면 모달, 아니면 페이지)가 열리며 키보드가 Overview 안에 있다. 그 모달을 닫으면 키보드는 팔레트를 열기 전 주인에게 돌아간다. Settings나 다른 대화상자가 열려 있으면 ⌘⇧O는 아무것도 하지 않는다. Overview 안에서 연 새 이슈, 시작, 디스크 정리 같은 대화상자는 모달 위에 열리고 Escape는 그 대화상자부터 닫는다. | D-20 |
| B19 | 중앙에 Workspace가 없을 때(첫 실행, 마지막 Workspace가 없어짐, 읽을 수 없는 배치 파일, Home 행) 중앙에는 배경 없이 같은 Overview가 페이지로 보이며 머리 줄, 범위 전환, 내용이 모달과 같고 사이드바는 그대로 쓸 수 있다. | D-02, D-16, D-17 |
| B20 | 프로젝트별 Overview 같은 Workspace가 아닌 화면에서 ⌘⇧O나 Overview 행은 페이지를 연다. 페이지에서 ⌘⇧O나 Escape는 앞에 있는 Workspace가 있으면 그 Workspace로 돌아가고 없으면 아무것도 하지 않는다. Workspace가 앞에 있어도 Home 행은 페이지를 All projects 범위로 연다. | D-16, D-17 |
| B21 | ⌘⇧O는 Workspace 위에서 모달을 열고, 열린 모달을 닫는다. 두 번 누르면 열렸다 닫혀 처음 상태로 돌아오고, 그 사이 범위 선택은 유지된다. | D-03, D-15 |
| B22 | ⌘⇧H는 기본으로 아무것도 하지 않는다. Project home 명령은 ⌘K, 데스크톱 View 메뉴(accelerator 없음), 툴바 메뉴의 `Open Project Overview`로 그대로 프로젝트별 Overview를 연다. | D-03, D-09, D-18 |
| B23 | ⌘⇧P를 누르면 사이드바가 ⌘B로 숨어 있었어도 보이고 Projects 탭이 선택되며 키보드가 Projects 목록의 선택된 행(없으면 첫 행)으로 간다. ⌘⇧A는 Agents 탭에 같은 일을 한다. 같은 키를 다시 눌러도 탭이 바뀌지 않고, 모달이 열려 있으면 먼저 모달이 닫힌다. 연결되지 않은 기기는 탭 줄이 없으므로 사이드바만 보인다. | D-03, D-18 |
| B24 | Projects \| Agents 줄은 탭 목록이고 선택된 탭이 탭 모양(승인된 보드의 선택 표시)으로 구분되며, 보조기술에는 탭과 선택 상태로 알려진다. focus된 탭에서 왼쪽, 오른쪽 화살표로 다른 탭으로 옮긴다. | D-04 |
| B25 | Projects 탭에는 `⌘⇧P`, Agents 탭에는 `⌘⇧A`, Overview 행에는 `⌘⇧O` keycap이 글자 옆에 보인다. Settings에서 키를 바꾸면 바뀐 키가, 키를 지우면 keycap 없이 보인다. 사이드바가 좁아지면 글자가 잘리기 전에 keycap이 먼저 빠진다. | D-04, D-18 |
| B26 | Settings의 Shortcuts와 ⌘/ 단축키 목록에 Overview(⌘⇧O), Projects 탭(⌘⇧P), Agents 탭(⌘⇧A)이 있고 `Toggle sidebar view`는 없다. Projects 탭과 Agents 탭 명령은 Settings에서 바꿀 수 있다. 업그레이드 전에 `Toggle sidebar view`에 키를 정해 둔 운영자는 그 키만 사라지고 다른 바꾼 키는 그대로이며, 버린 바인딩은 진단 로그에 남는다. | D-18, D-25 |
| B27 | 데스크톱 앱 View 메뉴에 `Overview`(⌘⇧O), `Projects`(⌘⇧P), `Agents`(⌘⇧A)가 있고 누르면 단축키와 같은 결과다. `Project home`은 accelerator 없이 남는다. | D-03, D-18 |
| B28 | ⌘E, ⌘B, ⌘⇧B는 이 변경 전과 같은 명령이다. 모달이 열린 동안 누르면 뒤의 Workspace와 사이드바에 오늘처럼 작용하고 모달은 열려 있다. 브라우저에서 연 hide는 레지스트리가 정한 브라우저 chord를 쓰며 ⌃Tab 자리는 ⌥`이다. | D-03, D-10 |
| B29 | Overview 모달이나 페이지에서 ⌃Tab(브라우저에서는 ⌥`)을 누르면 Agent pane 목록이 가장 최근 에이전트 pane을 가리키며 뜨고, ⌃⇧Tab은 반대쪽 끝에서 시작한다. modifier를 누르고 있는 동안 반복 chord는 목록 안에서만 움직이고 Overview, 범위, 중앙은 그대로다. | D-05, D-19 |
| B30 | modifier를 놓으면 모달이 닫히거나 페이지를 떠나면서 그 pane의 기기, 프로젝트, 탭, pane이 한 번에 앞에 오고 키보드가 그 pane에 있다. 다른 기기의 pane이면 사이드바와 중앙이 그 기기로 함께 옮긴다. | D-05, D-19 |
| B31 | 목록이 떠 있는 동안 Escape를 누르거나 창이 focus를 잃으면 목록만 사라지고 Overview는 같은 범위와 스크롤로, 키보드는 Overview 안 원래 자리에 남는다. | D-19 |
| B32 | 이 세션에서 방문한 에이전트 pane이 하나도 없으면 Overview에서 ⌃Tab은 아무 목록도 띄우지 않고 아무것도 옮기지 않는다. | D-19 |
| B33 | Overview에서 시작한 ⌃Tab 목록에는 에이전트 pane만 있고 View 탭은 없다. Global Recent Panels 목록에 모달을 연 일이 행으로 생기지 않으며, 페이지는 오늘 Home Overview처럼 한 행이다. | D-19 |
| B34 | 사이드바 Overview 행, 탭과 keycap, 툴바 아이콘(쉼, hover 툴팁, 점), 두 범위의 모달, 페이지가 운영자가 승인한 Pen 보드와 맞고, 다크와 라이트, 수 0, 긴 한글 이름에서 잘림 없이 읽힌다. | D-21, D-07 |
| B35 | 내 차례 수와 점은 스냅샷이 이미 싣는 에이전트 상태에서만 나오며, 사이드바 행과 툴바 아이콘을 그리는 일로 기기 읽기, 프로세스, tick이 늘지 않는다. 모달이나 페이지를 열 때의 읽기는 오늘 그 범위의 Overview를 열 때와 같다. | D-14, D-22 |

## Technical structure

웹 셸만 바뀐다. core → hided/WebSocket → React 경계, `workspace_view`, 스냅샷 wire, Herdr 계약과 저장 파일은 그대로다(D-22).
`web/src/ui.ts`의 페이지 상태에 Overview 모달의 열림과 창별 범위 선택이 더해진다. 모달은 Workspace 화면 위의 층이고 화면 종류(`main`, `overview`, `workspace`)를 바꾸지 않는다. 페이지는 지금의 `main` 화면 자리에 같은 Overview 틀을 그린다.
모달은 `web/src/components/ui`의 기존 Radix 대화상자 primitive를 쓴다. 그래서 네이티브 페이지 still과 Escape 층 순서가 기존 규칙으로 따라온다.
단축키 레지스트리에 Overview, Projects 탭, Agents 탭 명령을 더하고 `toggle_sidebar_view`를 지우며 `project_home`의 기본 chord를 뺀다. 데스크톱 메뉴는 레지스트리에서 그대로 만들어지고 `MENU_LAYOUT`이 세 항목을 싣는다.
⌃Tab의 Agent pane 순환은 Overview를 origin pane이 없는 시작점으로 받는다. View 영역 순환은 바뀌지 않는다.

## Risks

- three-column-panel 구현과의 병합 순서: 그 구현은 아직 main에 없다. 어느 쪽이 먼저 들어와도 Overview 아이콘은 Open server 앞에 오고(B5), 나중에 들어오는 쪽이 툴바 순서와 `docs/UI_BEHAVIOR.md` 툴바 문단을 맞춘다.
- #350과의 순서: 이 PRD만 들어가면 프로젝트별 Overview와 `Overview` 자식 행이 단일 Overview 행과 함께 남아 같은 정보로 가는 길이 둘이다. #350이 그것을 지우며, #350만 먼저 들어가면 전체 조감 화면이 없어지므로 그 순서는 막는다.
- 단축키 충돌: macOS 시스템 단축키와 다른 앱 단축키와의 충돌은 확인하지 않았다(D-11). 브라우저에서 연 hide는 Chrome이 ⌘⇧A(탭 검색)를 먼저 가질 수 있다. 구현 때 실제 데스크톱 앱과 Chrome에서 세 chord가 hide에 오는지 확인하고, 막히면 레지스트리의 브라우저 chord 이동 규칙으로 옮긴다.
- 운영자가 뒤집을 수 있는 위임 가정: 툴바 아이콘 순서(D-07, 시안은 globe 다음), Home 행 동작(D-17, 시안은 Home Workspace), 범위 대응과 기억(D-12, D-13), 수의 정의와 문구(D-14), 모달 닫기 규칙(D-15), 사이드바 탭 키의 키보드 이동(D-18), 팔레트와의 관계(D-20).
- Open question: Home 행이 Home Workspace를 열어야 하는지는 운영자가 #350 PRD나 Pen 보드 승인 때 정한다. 이 PRD의 행동 목록은 지금 동작(B20)으로 닫혀 있다.
- Open question: 시안의 "아무것도 안 열렸을 때" 보드는 사이드바 맨 위 Overview 행 없이 그려졌다. 이 PRD는 이슈대로 두 표현 모두에 행을 두며(B1), 보드를 옮길 때 그 보드를 고친다.
- 구현 착수 조건은 운영자의 PRD 승인과 Pen 보드 승인(D-21) 두 가지다. 자격 증명이나 외부 비용은 없다.
- 네이티브 검증은 운영자 앱을 건드리지 않고 격리된 Herdr 서버와 PID로 지정한 후보 창을 캡처해야 하며, 화면이 잠긴 machine에서는 미실행으로 기록한다.
