---
topic: "Web Project Overview: per-project Tasks/Agents board in the web shell"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "A new read-mostly web screen built from snapshot data the core already produces, porting existing Swift board rules; no core state, data migration, auth or external write."
source_intake: "agents/interview/web-project-overview/qa-log.md"
created_at: "2026-09-26"
updated_at: "2026-09-26"
---

# PRD: Web Project Overview: per-project Tasks/Agents board in the web shell

## Goal

hide 운영자는 한 프로젝트 안에서 여러 worktree와 에이전트가 동시에 움직일 때, 어떤 작업이 어느 단계에 있고 어디가 자기를 기다리는지 한 화면에서 보고 싶다.
Swift 앱에는 Project Home 보드가 있지만 web shell에는 없어서, web으로 옮긴 뒤로는 사이드바를 하나씩 훑어야 한다.
이 변경으로 web shell에 프로젝트별 Overview가 생겨, 체크아웃 카드가 git 진행 단계(준비, 작업 중, 리뷰, 머지됨) 열에 놓이고, 운영자를 기다리는 카드가 강조되며, 카드 안 에이전트 행을 눌러 바로 그 pane으로 갈 수 있다.

## Non-goals

- GitHub에 쓰지 않는다. 이슈와 Project 상태는 읽기만 하고 불일치는 칩으로만 보인다(D-03).
- Swift Project Home은 바꾸지 않는다(D-07).
- 새 core projection을 만들지 않는다. web snapshot 타입에 필요한 필드가 빠져 있으면 기존 core schema에서 타입을 다시 생성할 뿐이다(D-03).
- 사이드바 행 규칙 자체는 sidebar-agent-status PRD가 정한다. 이 화면은 그 규칙을 따른다(D-05).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | web shell에 Pen 보드 F를 따르는 프로젝트별 Overview를 만든다: 헤더(프로젝트 이름, Tasks/Agents 탭, 상태 요약, New agent), 즉석 줄(main과 폴더 작업), Tasks 열 준비/작업 중/리뷰/머지됨(머지됨 접힘)의 체크아웃 카드. 카드에는 브랜치, 목적, 연결 이슈 칩, 에이전트 행, 전달 상태 한 줄이 있고, 운영자를 기다리는 에이전트가 있는 카드는 경고 halo를 두고 git 열에 머문다. | 대화 "project 별로 볼 수 있는 overview screen", 보드 F에 "좋은 것 같은데?" |
| D-02 | Swift Project Home(PR #124)이 같은 보드를 구현한다: 단계 규칙(worktree나 PR이 머지됨이면 머지됨, 열린 PR이면 리뷰, 변경이나 ahead가 있으면 작업 중, 그 외 준비), 즉석(worktree가 아닌 체크아웃 중 에이전트가 있는 것), 연결 안 된 열린 이슈는 준비 열 backlog 카드, Agents 보기는 lineage 루트마다 카드, needs-you 카드 먼저. core snapshot에 이슈 링크, home_issues, PR, worktree merged, ahead, 변경 파일 수, GitHub 상태가 이미 있다. | 사실: `ProjectHomePresentation.swift:36-172`, `herdr-core/src/model.rs:665,767` |
| D-03 | web 보드는 Swift ProjectHomeBoard 규칙(단계, 즉석, backlog 이슈, Agents 보기, needs-you 우선 정렬, 이슈/Project 상태 불일치 칩)을 snapshot 위의 순수 web 표현 함수로 옮기고 결과가 같다. 필요한 필드가 web 타입에 없으면 기존 core schema에서 다시 생성한다. | 가정: 위임 하 design 5, engineering 7 |
| D-04 | 진입: ⌘⇧H(예약된 `project_home` 단축키, 지금은 notReady)와 사이드바 프로젝트 이름 클릭이 그 프로젝트 Overview를 메인 영역에 연다. Esc나 pane 선택은 pane grid로 돌아간다. 에이전트 행 클릭은 그 pane을, 카드 헤더 클릭은 그 체크아웃을 연다. | 가정: 위임, `web/src/keyboard.ts:82` |
| D-05 | 카드 안 에이전트 행은 sidebar-agent-status 행 규칙을 따른다: 기본 한 줄, needs-you(경고색)와 안 읽음 변경과 선택일 때만 둘째 줄, 다른 체크아웃의 자식만 브랜치 칩, 자식 대기 부모는 속이 빈 링. | 대화 "좋은 것 같은데?"(보드 D~F) |
| D-06 | 상태 우선순위: (1) shell의 첫 snapshot 전에는 Overview를 그리지 않고 기존 연결 중 상태가 보인다. (2) 에이전트가 하나도 없는 프로젝트는 git 여부와 상관없이 New agent가 있는 빈 상태다. (3) 에이전트가 있는 git 아닌 프로젝트는 즉석 줄만 있고 열이 없다. (4) 그 외에는 전체 보드다. hided가 끊겼거나 원격 기기에 닿지 않으면 보드는 마지막 snapshot을 유지하고 기존 연결/원격 표시만이 신호이며(새 배너 없음), 다음 snapshot에 갱신되고 별도 재시도 버튼은 없다. GitHub가 오래됐거나 못 읽으면 이슈 칩 툴팁에만 마지막 성공 시각이 나온다. 보드는 로컬 snapshot만 읽으므로 권한 제한 상태는 없다. | 가정: 위임 하 design 9, 13 |
| D-07 | web shell만 바꾸고 web-design-system-reset의 System 부품, 토큰, Light/Dark 위에 만든다. 기반이 머지된 뒤(또는 그 브랜치 위에 쌓아) 시작하고 sidebar-agent-status와 병렬로 갈 수 있다. 둘이 같은 에이전트 행 부품을 건드리면 나중에 머지되는 쪽이 앞쪽 위에서 정리한다. | 위임 "지금 리디자인한거랑 그거 바탕으로 implement", "stackedpr을 하든 머든 어케든" |
| D-08 | 증거: Swift ProjectHomeBoard 사례와 같은 fixture로 보드 함수 단위 테스트, 단축키와 프로젝트 클릭 진입, 카드와 에이전트 클릭, Tasks/Agents 전환, 머지됨 접힘, 빈 상태와 git 아닌 상태 web e2e, Pen 보드 F 옆 Light/Dark 캡처를 `agents/runs/`에 둔다. | 가정: 위임 |
| D-09 | 원칙 입력: `~/projects/oh-my-principle` 654485f의 engineering, design 문서를 읽었다. design 1, 2, 4, 5, 7, 8, 9, 10, 13과 engineering 5, 7을 행동으로 옮겼고 나머지는 이 화면에 새 행동이 없다. | 원칙 intake |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | ⌘⇧H나 사이드바의 프로젝트 이름을 누르면 그 프로젝트의 Overview가 메인 영역에 Tasks 보기로 열리고, Esc나 pane 선택으로 pane grid로 돌아간다. | D-04 |
| B2 | 헤더에 프로젝트 이름, Tasks/Agents 탭, worktree 수와 열린 PR 수와 main이 origin보다 뒤처진 커밋 수(0이 아닐 때만), New agent 버튼이 있다. | D-01, D-09 |
| B3 | New agent는 web shell의 기존 새 에이전트 흐름을 이 프로젝트를 대상으로 연다. 취소하면 Overview로 돌아오고, 실패하면 그 흐름이 지금 보여 주는 결과를 그대로 따른다. | D-01, D-06 |
| B4 | 즉석 줄에는 worktree가 아닌 체크아웃(main, 폴더) 중 에이전트가 있는 것이 카드로 나온다. | D-01, D-02 |
| B5 | worktree 체크아웃 카드는 준비, 작업 중, 리뷰, 머지됨 열 중 Swift와 같은 단계 규칙이 정한 열에 놓이고, 머지됨 열은 접힌 채 시작한다. 연결 안 된 열린 이슈는 준비 열에 backlog 카드로 보인다. | D-02, D-03 |
| B6 | 카드에는 브랜치, 목적 한 줄, 연결 이슈 칩, 에이전트 행, 전달 상태 한 줄(변경 수, ahead 커밋, PR 번호와 CI 결과, 머지됨)이 보이고, 이슈 Project 상태가 git 단계와 다르면 불일치 칩과 툴팁이 보인다. | D-01, D-03 |
| B7 | 운영자를 기다리는 에이전트가 있는 카드는 경고 halo로 강조되고 자기 git 열 맨 위로 올라가며, 열을 옮기지 않는다. | D-01, D-02 |
| B8 | 카드 안 에이전트 행은 사이드바 행 규칙을 따라 기본 한 줄이고, 기다림과 안 읽음 변경과 선택일 때만 둘째 줄이 보이며, 다른 체크아웃의 자식에만 브랜치 칩이 붙는다. 행을 누르면 그 pane이 열리고 카드 헤더를 누르면 그 체크아웃이 열린다. | D-04, D-05 |
| B9 | Agents 탭은 lineage 루트마다 카드를 만들어 Swift와 같은 lifecycle 열로 보여 주고, 같은 행 규칙과 halo를 쓴다. | D-02, D-03 |
| B10 | 에이전트가 하나도 없는 프로젝트는 git 여부와 상관없이 New agent가 있는 빈 상태를 보이고, 에이전트가 있는 git 아닌 프로젝트는 즉석 줄만 보인다. shell이 첫 snapshot을 받기 전에는 Overview 대신 기존 연결 중 상태가 보인다. | D-06 |
| B11 | GitHub 정보가 오래됐거나 읽히지 않으면 이슈 칩 툴팁에만 마지막 성공 시각이 나오고 배너나 오류 문장은 없다. hided가 끊겼거나 원격 기기에 닿지 않는 동안 보드는 마지막 snapshot을 유지하고 기존 연결/원격 표시만 바뀌며, 다음 snapshot이 오면 저절로 갱신된다. | D-06 |
| B12 | 열이 화면보다 넓으면 보드가 가로로 스크롤되고, 카드 제목은 두 줄까지 줄바꿈되며 브랜치는 끝이 잘리고 툴팁으로 전체가 보인다. 한글 목적과 긴 브랜치도 카드 밖으로 넘치지 않는다. | D-01, D-09 |
| B13 | 화면은 Light와 Dark 두 테마에서 System 부품과 토큰으로 그려진다. | D-07 |
| B14 | 전달 시점에 보드 함수 테스트, e2e, Pen 보드 F 옆 Light/Dark 캡처가 `agents/runs/`에 있고 커밋되지 않는다. | D-08 |

## Technical structure

- web에 Overview 화면과, snapshot에서 보드를 만드는 순수 표현 함수가 추가된다. Swift ProjectHomeBoard 규칙을 옮긴 것이다.
- core 변경은 없다. web snapshot 타입에 이슈, home_issues, GitHub 상태 필드가 빠져 있으면 기존 schema에서 다시 생성한다.
- 진입은 기존 예약 단축키와 사이드바 프로젝트 클릭이다.

## Risks

- 기반 PR과 sidebar-agent-status가 먼저이거나 병렬이다. 에이전트 행 부품이 겹치면 나중에 머지되는 쪽이 정리한다(D-07).
- Swift 보드를 옮기는 것이라 두 곳에 같은 규칙이 생긴다. S10에서 Swift가 지워지면 web이 유일한 구현이 된다.
- 원격 프로젝트는 snapshot에 있는 만큼만 그려지므로 로컬보다 정보가 적을 수 있다.
- 사용자가 미리 할 일은 없다. 모양 최종 확인은 아침 검토 항목이다.
