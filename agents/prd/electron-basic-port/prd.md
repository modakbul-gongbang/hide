---
topic: "Electron basic port: a desktop host for the web shell"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "Adds a new desktop host package and a native process that attaches to or starts the existing local daemon; no data migration, credentials, signing or network exposure beyond loopback."
source_intake: "agents/interview/electron-basic-port/qa-log.md"
created_at: "2026-09-26"
updated_at: "2026-09-26"
---

# PRD: Electron basic port: a desktop host for the web shell

## Goal

hide 운영자는 web shell을 브라우저 탭에서 쓰고 있어서, ⌘T나 ⌘W 같은 단축키가 브라우저에 먹히고 hide가 다른 탭 사이에 섞인다.
이 변경은 `desktop/`에 Electron 앱을 추가해, 실행 중인 hided에 붙거나(없으면 띄워서) 지금 web shell을 전용 창에 띄운다.
운영자는 hide를 독립된 macOS 앱으로 열고, 브라우저 때문에 옮겨 두었던 단축키를 원래 ⌘ 조합으로 다시 쓰며, 창 크기와 위치가 유지되는 기본 데스크톱 경험을 얻는다.

## Non-goals

- 코드 서명, 공증, 자동 업데이트, 설치 DMG 배포는 하지 않는다. 이 Mac용 서명 안 된 로컬 .app과 dev 실행 명령만 만든다. 재검토: 다른 사람에게 배포할 때(D-05).
- 트레이/메뉴바 항목, 전역 단축키, Dock 배지, pet 창, Browser pane, 파일 드래그 드롭, 사용량 표시, 원격 파일 뷰어는 하지 않는다. 우산 PRD의 미룸 목록이며 후속 Electron PRD에서 다룬다(D-05).
- 앱을 종료해도 hided를 멈추지 않는다. 데몬의 수명은 데몬의 것이다(D-03).
- Swift shell과 S10 삭제에는 손대지 않는다(D-09).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 기본 Electron 호스트를 지금 만들고, 범위는 에이전트가 정한다. | 위임 "electron으로 한번 포팅하는거도 우선 기본적인 구현냉ㅛ용 너가 뽑아서 implement해" |
| D-02 | hided는 loopback HTTP와 토큰 WS를 제공하고, 상태 파일 `hided.json`(pid, port, token, socket, 0600)이 state dir에 있다. `hide open`은 살아 있는 데몬을 재사용하거나 띄우고 `http://127.0.0.1:<port>/#token=<token>`을 연다. 우산 PRD가 `desktop/`을 Electron 자리로 예약했고, `web/src/shortcuts.ts`의 electron 열은 Swift 단축키로 채울 자리로 비어 있다. | 사실: `hided/src/cli.rs:52-195`, `hided/src/state_file.rs`, `agents/prd/web-shell-pivot/prd.md`의 결정 1, 11, `web/src/shortcuts.ts:56-83`, `ShellMenuCommand.swift:85-96` |
| D-03 | `desktop/`은 pnpm workspace 멤버이고 Electron(현재 stable)과 TypeScript를 쓴다. main 프로세스는 `hide open`과 같은 발견 경로를 쓴다: 상태 파일의 살아 있는 데몬에 붙고, 없으면 `hide` CLI로 데몬을 띄운다. 두 번째 데몬은 절대 만들지 않고, 앱을 종료해도 데몬은 멈추지 않는다. 토큰은 지금처럼 URL hash로만 renderer에 전달한다. | 가정: 위임 하 engineering 7, 8, 14 |
| D-04 | 포함: 크기와 위치가 유지되는 주 창 하나, single-instance(두 번째 실행은 기존 창 포커스), 표준 Edit/Window 역할이 있는 macOS 앱 메뉴, Swift 단축키로 채운 electron 열(⌘T, ⌘W, ⌘⇧T, ⌘⇧N, ⌘K, ⌘P, ⌘⇧H, ⌘B, ⌘E, ⌘⇧B, ⌘F, ⌘⇧K)을 같은 web 동작으로 연결, Electron 안에서는 단축키 시트가 electron 단축키를 보여 줌. 외부 링크는 기본 브라우저로 열고 탐색은 데몬 origin에 묶는다. | 가정: 위임 |
| D-05 | 제외: 서명, 공증, 자동 업데이트, 설치 배포, 트레이/메뉴바, 전역 단축키, Dock 배지, pet, Browser pane, 드래그 드롭, 사용량, 원격 파일 뷰어. 빌드는 서명 안 된 로컬 .app과 dev 실행 명령이다. | 가정: 위임, 우산 D-04 미룸 목록 |
| D-06 | 실패: hided에 닿지도 띄우지도 못하면 창에 원인 분류와 Retry가 있는 화면 하나가 보이고 세부는 로그로 간다. Retry는 발견을 다시 한다. 열린 동안 데몬이 죽으면 web shell의 기존 끊김 상태가 보이고 web shell이 다시 붙을 때 호스트도 발견을 다시 한다. 낡은 상태 파일은 `hide open`과 같은 생존 확인으로 처리한다. | 가정: 위임 하 engineering 4, 10 |
| D-07 | renderer 보안: contextIsolation 켬, nodeIntegration 끔, sandbox 켬, 메뉴 명령 채널과 호스트 식별만 노출하는 최소 preload, 탐색과 새 창은 데몬 origin으로 제한. | 가정: Electron 보안 체크리스트 |
| D-08 | 증거: 단축키 라우팅과 레지스트리 electron 열 단위 테스트, isolated hided(fake_herdr fixture)에 붙는 Playwright `_electron` smoke(사이드바와 터미널 pane이 보임), 정확히 한 인스턴스의 네이티브 스크린샷. verify 워크플로에 desktop typecheck/lint/unit 추가(CI에서 Electron e2e를 못 돌리면 로컬로 두고 PR에 적음). 문서: AGENTS.md Repository Layout, `docs/ARCHITECTURE.md` Electron 열, `docs/BUILD.md` desktop 명령. | 가정: 위임 |
| D-09 | PR 모드. Swift와 S10과 독립이고, web-design-system-reset이 머지된 뒤(또는 그 위에 쌓아) main에서 시작한다. | 가정: 위임 |
| D-10 | 원칙 입력: `~/projects/oh-my-principle` 654485f의 engineering, design 문서를 읽었다. engineering 1, 4, 6, 7, 8, 10, 14와 design 9, 13을 행동이나 비목표로 옮겼다. | 원칙 intake |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | dev 명령이나 로컬 .app으로 앱을 실행하면 hided를 찾는 동안 창에 연결 중 표시가 보이고, 살아 있는 hided가 있으면 거기에 붙어, 없으면 `hide` CLI로 띄운 뒤 web shell이 창 안에 나타난다. 발견은 정해진 시간 안에 끝나고, 넘기면 실패 화면으로 간다. | D-02, D-03, D-06 |
| B2 | `hide` 실행 파일은 환경 변수 override, dev 실행 시 이 worktree에서 빌드한 바이너리, PATH 순으로 찾는다. 찾지 못하면 "hide CLI를 찾을 수 없음" 실패 화면과 Retry가 보이고 시도한 경로는 로그에 남는다. | D-03, D-06 |
| B3 | hided에 닿지도 띄우지도 못하면 원인 분류(실행 파일 없음, 시작 실패, 응답 없음)와 Retry가 있는 화면 하나가 보이고, Retry는 발견을 처음부터 다시 한다. | D-06 |
| B4 | 앱이 열려 있는 동안 hided가 죽으면 web shell의 기존 끊김 상태가 보이고, hided가 다시 뜨면 앱이 다시 붙는다. | D-06 |
| B5 | 앱을 종료해도 hided와 그 안의 터미널, 에이전트는 계속 돌고, 다음 실행이나 브라우저의 `hide open`이 같은 데몬에 붙는다. 두 번째 데몬은 생기지 않는다. | D-03 |
| B6 | 앱을 두 번 실행하면 새 창이 뜨지 않고 기존 창이 앞으로 온다. | D-04 |
| B7 | 마지막 창을 닫아도 앱은 macOS 관례대로 살아 있고, Dock 아이콘을 누르면 창이 다시 열리며, ⌘Q로 종료한다. | D-04, D-05 |
| B8 | 창 크기와 위치는 다음 실행에 복원된다. 저장된 값이 없거나 화면 밖이면 기본 크기로 화면 중앙에 열린다. | D-04 |
| B9 | 앱 안에서 ⌘T, ⌘W, ⌘⇧T, ⌘⇧N, ⌘K, ⌘P, ⌘⇧H, ⌘B, ⌘E, ⌘⇧B, ⌘F, ⌘⇧K가 브라우저에서 옮겨 둔 동작과 같은 web 동작을 실행하고, 단축키 시트는 Electron 안에서 이 단축키를 보여 준다. 복사, 붙여넣기, 실행 취소 같은 표준 편집 단축키가 터미널과 에디터에서 동작한다. | D-02, D-04 |
| B10 | web shell 안의 외부 링크는 기본 브라우저로 열리고, 창은 데몬 origin 밖으로 이동하지 않는다. | D-04, D-07 |
| B11 | renderer는 Node API에 접근할 수 없고, preload는 메뉴 명령 채널과 호스트 식별만 노출한다. | D-07 |
| B12 | 브라우저에서 쓰는 web shell과 단축키는 전과 같다. electron 열이 채워져도 브라우저 단축키는 바뀌지 않는다. | D-04 |
| B13 | AGENTS.md, `docs/ARCHITECTURE.md`, `docs/BUILD.md`가 desktop 패키지, 실행과 빌드 명령, Electron 단축키 열을 설명한다. | D-08 |
| B14 | 전달 시점에 단축키 단위 테스트, `_electron` smoke, 한 인스턴스 네이티브 스크린샷이 `agents/runs/`에 있고, verify 워크플로가 desktop typecheck/lint/unit을 돌린다. | D-08 |

## Technical structure

- 새 패키지 `desktop/`(pnpm workspace 멤버): Electron main, 최소 preload, 발견과 실패 화면. web shell은 hided가 서빙하는 그대로 쓴다.
- 데몬 경계: 호스트는 `hide` CLI와 hided 상태 파일로만 데몬과 만난다. 데몬을 내장하거나 두 번째로 띄우지 않는다.
- web shell 변경은 electron 단축키 열 채우기와, Electron 호스트일 때 메뉴 명령을 받는 입구뿐이다.
- 창 상태는 Electron app userData의 작은 JSON 파일에 저장하고, 읽지 못하면 기본값을 쓴다.

## Risks

- web-design-system-reset이 먼저 들어가야 최신 UI가 보인다. 들어가지 않았으면 그 위에 쌓거나, main의 web shell로 진행한다(D-09).
- Electron 바이너리 다운로드 때문에 CI에서 e2e가 무거울 수 있다. CI에서 못 돌리면 로컬 smoke로 남기고 PR에 적는다.
- 서명하지 않은 앱이라 Gatekeeper가 처음 실행을 막을 수 있다. 로컬 빌드에서는 우클릭 열기로 넘기며, 배포는 비목표다.
- 네이티브 확인은 isolated hided와 dev 인스턴스 하나로 하고, 운영자의 앱과 서버는 건드리지 않는다.
- 사용자가 미리 할 일은 없다.
