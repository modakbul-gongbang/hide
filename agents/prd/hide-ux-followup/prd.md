---
topic: "hide 사용 중 보고된 UX 요구사항 10건"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "로컬 macOS 앱의 입력, 탐색, 상태 표시, pane attach와 성능 경로를 함께 바꾸지만 기존 사용자 세션, 외부 데이터, 인증, 공개 배포는 변경하지 않는다."
source_intake: "current conversation"
created_at: "2026-08-31"
updated_at: "2026-08-31"
---

# PRD: hide 사용 중 보고된 UX 요구사항 10건

## 1. Summary

사용자가 실제 hide를 쓰면서 보고한 UX 요구사항 10건을 기존 SwiftUI 셸과 herdr-core 구조 안에서 해결한다.
변경 범위는 terminal의 `Cmd+Delete`, agent 상태 점과 선택 표시, workspace 접기와 단축키, Pet 설정과 dashboard, workspace 전환 성능, `Option+Tab`과 Search, Finder 우선 New Workspace, 2타일 New Agent, pane attach 실패 복구다.
현재 Raycast 계열 다크 네이티브 도구 UI와 `HideTheme`을 유지하는 targeted evolution으로 구현하며, 별도 디자인 시스템이나 애니메이션 언어를 추가하지 않는다.
성능과 attach 문제는 코드 추측으로 고치지 않고 설치된 단일 앱과 실제 herdr server를 먼저 샘플링하고, disposable fixture에서 전환 지연과 attach 수명주기를 재현한 뒤 측정 결과에 맞는 가장 작은 구조 수정만 한다.
모든 구현과 검증은 로컬 전용이며 기존 `creator-studio`, `herdr-ide`, `modakbul`, `oh-my-principle`, `sasu` workspace와 그 pane 및 agent를 닫거나 옮기거나 takeover하지 않는다.

### Approval checklist

- 사용자 원문 우선: 10개 요구사항과 마지막 정정이 구현 의도의 기준이며, 2번의 이전 `2-B` 답변은 폐기한다.
- 상태 표시: 별도 spinner와 모든 회전·점멸 animation은 추가하지 않고, 배지 우하단 상태 점만 현재 위치에서 소폭 키우며 16과 19 양쪽을 실앱 screenshot으로 판정한다.
- 생성 흐름: New Workspace는 Finder 폴더 선택 후 Hide 확인 modal을 열고, New Agent는 Claude와 Codex logo tile 2개만 보여주며 bypass는 매번 기본 OFF다.
- 용어: 사용자 가시 용어는 `Workspace`를 유지하고 최근 `SPACES` 변경은 `WORKSPACES`와 workspace 빈 상태 문구로 되돌린다.
- 성능과 복구: pane 전환은 app/server sample과 baseline이 선행하며, attach failure는 pane 생존과 분리하고 자동 `--takeover` 또는 retry storm을 만들지 않는다.
- 안전: 기존 Herdr 세션은 관찰만 하고, 검증은 `herdr-ide-verify-*` fixture에서 수행하며, 공개 저장소 push, PR, CI, 공개 배포를 하지 않는다.
- 전달: `agents/config.json`의 local delivery로 검증 영수증과 semantic local commit까지 완료한다.

## 2. Problem, Goal, And Users

사용자는 hide에서 여러 workspace와 Claude 및 Codex agent를 오가며 terminal 작업, 상태 확인, agent 실행을 반복한다.
현재는 terminal 편집 단축키가 빠져 있고, 상태 점이 작으며, 선택된 agent가 사이드바에서 분명하지 않고, workspace를 접을 수 없고, 표시된 단축키와 실제 명령이 어긋난다.
Pet 설정은 찾기 어렵고 click 동작은 상태 dashboard가 아니라 attention pane 직행이며, workspace 전환은 체감상 느리고 때때로 terminal이 다시 붙는 듯 보인다.
`Option+Tab`, Search, New Workspace, New Agent 흐름도 사용자의 실제 작업 순서와 참고 화면의 정보 구조를 충분히 반영하지 못한다.
attach 경쟁이나 중복 시도는 살아 있는 pane을 닫힌 것으로 오인하게 만들 수 있고, 이를 자동 takeover로 숨기면 다른 client와 기존 세션을 끊을 위험이 있다.

목표는 hide의 단일 core snapshot 권한을 유지하면서 자주 쓰는 입력과 이동을 즉각적으로 만들고, 상태와 실패를 사용자가 계산하지 않아도 화면에서 바로 읽게 하는 것이다.
성공한 결과에서는 작은 상호작용이 기존 패턴과 자연스럽게 이어지고, workspace 전환과 attach 실패의 원인은 측정 및 구조화된 증거로 설명되며, 검증 fixture 밖의 사용자 세션에는 변화가 없다.

### 2.1 User Scenarios

- SC1. Terminal에서 현재 입력 줄을 빠르게 지운다.
  Actors: hide 사용자, local 또는 remote terminal.
  Primary path: 사용자가 IME 조합 중이 아닐 때 `Cmd+Delete`를 누르면 terminal에 `Ctrl+U`가 전달되어 cursor 앞의 현재 입력 줄이 지워진다.
  Failure state: 조합 중 입력이나 다른 modifier 조합을 가로채거나 macOS 편집 명령과 terminal 입력을 동시에 보내지 않는다.
  Recovery: 지원하지 않는 조합은 기존 terminal key path에 그대로 위임한다.
  Reach: disposable shell pane에서 cursor 앞뒤가 있는 명령과 한글 IME 조합 상태를 각각 준비한다.

- SC2. Sidebar에서 상태, focus, workspace 구조를 한눈에 읽고 단축키로 이동한다.
  Actors: hide 사용자.
  Primary path: 16 또는 19 크기 agent badge의 상태 점이 로고를 가리지 않는 범위에서 전보다 조금 더 잘 보이고, focused agent 행과 그 checkout이 함께 선택 표시되며, workspace를 접고 펼친 상태가 재실행 뒤에도 유지된다.
  Primary path: `Cmd+N`, `Cmd+Shift+N`, `Cmd+K`가 화면에 표시된 의미와 같은 New Agent, New Workspace, Search 흐름을 연다.
  Failure state: 별도 spinner, 회전, 점멸이 생기거나 mutable display label 때문에 동명이인 workspace의 잘못된 pane으로 이동하지 않는다.
  Recovery: focused pane이 사라지면 core snapshot 기준의 남은 focus 또는 선택 없음 상태로 수렴한다.
  Reach: 같은 표시명을 가진 checkout과 badge 16 및 19를 모두 포함한 fixture를 만든다.

- SC3. Finder에서 폴더를 고른 뒤 Workspace를 안전하게 등록한다.
  Actors: hide 사용자, macOS Finder panel, Git.
  Primary path: 사용자가 New Workspace를 실행하면 macOS 기본 폴더 선택기가 먼저 열리고, 폴더를 고른 뒤에만 Hide 확인 modal이 열려 기본 폴더명 기반 이름과 git 초기화 여부를 정한다.
  Failure state: Finder를 취소하면 Hide modal이나 side effect가 없고, git 초기화 실패는 성공으로 숨기지 않으며 폴더 등록 결과와 실패를 구분한다.
  Recovery: 실패 원인을 확인하고 git 없이 등록된 workspace를 쓰거나 다시 시도한다.
  Reach: disposable git 및 non-git 폴더와 의도적으로 git init을 실패시키는 fixture를 사용한다.

- SC4. Pet을 찾고 전체 agent 상태를 dashboard에서 확인한다.
  Actors: hide 사용자.
  Primary path: Settings의 명확한 Pet 표면에서 표시 여부를 바꾸고, Pet을 click하면 workspace별 agent dashboard가 열려 전체 수와 working, done, idle, error, disconnected 상태와 각 agent의 kind, status, summary, elapsed, unseen, connection을 보여준다.
  Primary path: dashboard 행을 click하면 정확한 pane으로 이동한다.
  Failure state: drag release를 click으로 오인하지 않고, ambient 수치가 없는 경우 0으로 꾸미지 않으며, Herdr가 주지 않는 model token, cost, quota, context 수치를 만들지 않는다.
  Recovery: disconnected 및 빈 목록도 dashboard 안에서 명시하고, 연결 복구 시 기존 core snapshot 주기로 갱신한다.
  Reach: 0개, 혼합 상태, unseen/error, disconnected, ambient 존재 및 부재 fixture를 사용한다.

- SC5. Workspace 사이를 빠르게 전환하고 attach 실패를 복구한다.
  Actors: hide 사용자, hide app, herdr server, 다른 terminal client.
  Primary path: warm A-B-A workspace 전환에서 선택 직후 대상 terminal의 첫 안정 frame이 나타나고, 현재 layout에 필요한 각 pane은 하나의 in-flight 또는 active attach만 가진다.
  Failure state: 다른 client가 disposable pane을 소유해 attach가 거절되어도 authoritative pane은 살아 있는 것으로 남고 transport만 unavailable로 표시되며, 자동 retry storm과 `--takeover`가 없다.
  Recovery: 소유권이 해제된 뒤 사용자가 명시적 Reconnect를 눌러 다시 붙는다.
  Reach: 1 pane, 15 pane, zoom-hidden output, delayed attach, external-owner conflict를 가진 disposable fixture를 사용한다.

- SC6. 최근에 보던 agent를 `Option+Tab` 또는 Search로 찾는다.
  Actors: hide 사용자.
  Primary path: hide가 활성인 동안 `Option+Tab`을 누르면 최근 선택한 pane 기준 MRU agent overlay가 열리고 Tab 반복으로 순환하며 Option release에서 선택을 확정한다.
  Primary path: Search는 `Workspace > agents` 구조로 결과를 묶고 agent 선택 시 정확한 pane에 focus한다.
  Failure state: agent가 없거나 하나뿐인 경우, overlay cancel, focus 대상 소멸, 동명 workspace에서도 잘못된 pane을 선택하지 않는다.
  Recovery: cancel 시 이전 focus를 유지하고 사라진 항목은 다음 authoritative snapshot에서 제거한다.
  Reach: 최소 두 workspace와 동명 label을 포함한 disposable agent fixture를 사용한다.

- SC7. Claude 또는 Codex를 올바른 옵션으로 시작한다.
  Actors: hide 사용자, 설치된 Claude 또는 Codex CLI.
  Primary path: New Agent modal은 Device, Claude/Codex logo tile 2개, Workspace 또는 checkout, Options, action 순서로 보이고, 선택 provider에 맞는 command를 실행한다.
  Failure state: Gemini나 Cursor tile이 나타나지 않고, permission bypass는 modal을 열 때마다 OFF이며 ON일 때 persistent warning이 보이고 단 한 번의 launch에만 해당 flag를 붙인다.
  Recovery: CLI 미설치 또는 spawn 실패는 해당 단계와 원인을 표시하고 modal 입력을 보존한다.
  Reach: provider별 pure argument fixture와 설치 및 미설치 UI fixture를 사용한다.

## 3. Scope And Non-Goals

### 포함 범위

- `Cmd+Delete`의 Command-only, non-composing terminal line clear를 local 및 remote key path에 추가한다.
- `AgentBadge`의 상태 점 지름 비율을 소폭 키우고 현재 우하단 위치와 working 및 unseen completion 색 의미를 유지한다.
- focused pane ID 기반 agent 행 선택 표시, checkout과의 동시 표시, 동명 label에서도 정확한 routing을 구현한다.
- workspace collapse 상태를 file tree expansion과 분리된 durable core UI state로 저장하고 복원한다.
- `Cmd+N`, `Cmd+Shift+N`, `Cmd+K`를 실제 SwiftUI command로 연결하고 표시 문구와 일치시킨다.
- Pet 설정 discoverability와 core snapshot 기반 dashboard 및 row-to-pane navigation을 구현한다.
- 실행 중 앱과 server sample, 전환 latency, snapshot/mutex, attach count를 측정한 뒤 성능 병목만 수정한다.
- app-local `Option+Tab` MRU overlay와 `Workspace > agents` Search grouping을 구현한다.
- Finder first New Workspace 확인 흐름과 `WORKSPACES` 용어를 복원한다.
- Claude와 Codex 두 logo tile만 있는 New Agent modal, 매회 bypass OFF, provider별 launch flag를 구현한다.
- pane lifecycle과 attach transport lifecycle을 분리하고 explicit Reconnect와 구조화 진단을 추가한다.
- 이 변경이 obsolete로 만드는 bypass persistence, raw path browser, 중복 attach 또는 잘못된 label routing 경로를 같은 변경에서 삭제한다.

### 명시적 비목표

- agent 이름 옆 별도 spinner, 상태 점 회전, 깜빡임, pulse 또는 다른 animation.
- 상태 점 위치 변경, working과 unseen completion의 기존 색 의미 변경.
- Hide 내부에 Finder 대체 폴더 탐색기 구현.
- `Space` 용어 통일, herdr API의 `workspace` 내부 명칭 변경.
- Gemini 또는 Cursor agent 지원이나 tile 추가.
- Herdr가 제공하지 않는 model token usage, cost, quota, context 정보의 추정 또는 표시.
- macOS 전체를 가로채는 global `Option+Tab` shortcut.
- 다른 client를 끊는 자동 `--takeover`와 무한 또는 주기적 자동 reconnect.
- 기존 사용자 workspace, pane, agent의 이동, 종료, rename, attach ownership 변경.
- 공개 저장소 push, PR, CI 실행, GitHub release, 공개 배포.
- 측정 전 특정 mutex, polling, attach 전략을 원인으로 확정하거나 수정하는 것.

### 제품 완결성

10개 UX 항목의 primary path뿐 아니라 cancel, disconnected, duplicate label, missing data, external owner, restart persistence를 함께 완료한다.
기능을 단계적으로 구현하되 마지막 Done은 10개 항목 전체와 공통 안전 및 성능 검증이 통과한 상태만 뜻한다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
필요한 선택은 현재 대화에서 모두 해결되었고, 구현 및 검증에 필요한 앱, herdr server, 소스, disposable fixture는 로컬에서 준비할 수 있다.

### 4.2 Human Decisions Before PRD Approval

None required.
사용자는 `$please`로 전체 로컬 pipeline을 위임했고, 질문이 필요했던 2번, 8번, 9번, 용어 결정을 모두 현재 대화에서 확정했다.

### 4.3 Decision Traceability For Fidelity Review

- D1. 사용자 원문이 의도의 기준이며 scratchpad의 작업 방식, 현재 코드 상태, 안전 규칙을 지킨다.
  Mapping: 전체 PRD, 특히 9장과 11장.
- D2. 최초 답변의 `2-B`, 즉 agent 이름 옆 별도 spinner는 사용자가 명시적으로 취소했다.
  Mapping: 비목표, R2, AC3.
- D3. 2번 최종 결정은 `AgentBadge` 우하단 상태 점의 `size * 0.32` 비율을 현재 위치에서 조금만 키우는 것이다.
  Mapping: R2, AC3, T3.
- D4. 배지 크기 16과 19에서 로고를 가리지 않는지는 실제 실행 앱 screenshot으로 구현자가 판단하며, 회전과 점멸은 금지한다.
  Mapping: AC3, V4.
- D5. working과 unseen completion은 animation이 아니라 기존 색으로 구분한다.
  Mapping: R2, AC3, 11장.
- D6. 8번은 Finder 폴더 선택기를 먼저 열고, 선택 뒤 Hide modal에서 이름과 git 초기화 여부를 정하는 A안이다.
  Mapping: R8, AC15, T5.
- D7. 사용자 가시 용어는 Workspace를 유지하며, commit `6592799`의 sidebar `SPACES` 및 관련 빈 상태 문구는 `WORKSPACES`와 workspace 문구로 되돌린다.
  Mapping: R8, AC15, T5.
- D8. herdr API 내부 명칭은 기존 `workspace`를 유지한다.
  Mapping: 비목표, 11장.
- D9. 9번은 Claude와 Codex 2개만 두는 A안이다.
  Mapping: R9, AC16, T6.
- D10. 참고 화면의 logo-led tile style과 정보 구조는 쓰되 4타일을 복제하지 않고 Gemini와 Cursor를 넣지 않는다.
  Mapping: R9, AC16.
- D11. 기존 결정에 따라 bypass는 modal을 열 때마다 기본 OFF이며 ON이면 경고를 계속 보이고 해당 launch 한 번에만 적용한다.
  Mapping: R9, AC16, AC17.
- D12. 6번 pane 전환은 실행 중인 앱과 herdr server에 `/usr/bin/sample`을 걸고 baseline을 얻기 전에는 수정하지 않는다.
  Mapping: R6, T1, AC11, V5.
- D13. 기존 `creator-studio`, `herdr-ide`, `modakbul`, `oh-my-principle`, `sasu`와 그 pane 및 agent는 닫거나 옮기거나 takeover하지 않는다.
  Mapping: SC5 Reach, 9장, 11장.
- D14. 공개 저장소에는 아무것도 push하지 않고 local delivery만 한다.
  Mapping: 비목표, 9장, 12장.
- D15. agent 소유 가정: `Option+Tab`은 hide가 활성인 동안만 동작하는 app-local monitor로 구현한다.
  Mapping: R7, AC14, RF3.
- D16. agent 소유 가정: Pet click은 기존 oldest-unseen direct jump를 dashboard open으로 교체하고 dashboard row click이 pane focus를 담당한다.
  Mapping: R5, AC9, RF2.
- D17. agent 소유 가정: 측정 후 성능 목표는 warm A-B-A 전환 p95 400ms 이하이면서 baseline 대비 50% 이상 개선이고 poll 주기만큼 빈 projection이 보이지 않는 것이다.
  Mapping: AC11, RF1.
- D18. HerdrM은 device 및 pane ID 기반 선택, 선택된 terminal의 단일 attach view, attach failure와 pane lifecycle 분리, explicit reconnect의 established behavioral reference로만 사용한다.
  Mapping: 5장, R10.
  Hide의 multi-pane grid와 안전 경계 때문에 HerdrM의 상시 `--takeover`는 채택하지 않는다.
- D19. principles intake는 `~/projects/oh-my-principle` commit `35ab76ca23d45e714f1630054855a8c8c4568d03`의 engineering 및 design principles와 engineering `practices/test.md` 전문을 기준으로 했다.
  Mapping: 11장 전부.
- D20. design direction은 기존 Raycast 계열 dark native tool UI의 targeted evolution이며 variance 4, motion 1, density 7이다.
  Mapping: R2, R3, R5, R7, R8, R9, AC3, AC5, AC9, AC14, AC15, AC16.

거절 상태를 유지하는 대안:

- 별도 spinner와 모든 animation은 최종 사용자 정정으로 거절됐다.
- Hide 내부 폴더 탐색기는 macOS Finder 우선 흐름에 밀려 거절됐다.
- `SPACES`와 `Space` 용어 통일은 Workspace 유지 결정으로 거절됐다.
- Gemini, Cursor와 4타일 구성은 Claude 및 Codex 2타일 결정으로 거절됐다.
- 자동 `--takeover`는 사용자 세션을 끊을 수 있어 안전 규칙과 충돌하므로 거절됐다.

## 5. Major Technical Structure Changes

- Terminal key policy는 기존 `ImeTerminalView`와 remote pointer routing의 modified-key 분기 안에 Command-only `Delete`를 `Ctrl+U` byte로 번역하는 한 규칙을 추가하고 두 경로가 같은 pure policy를 사용하게 한다.
- Sidebar identity는 display label이 아니라 authoritative pane ID를 focus와 navigation key로 사용한다.
  Workspace collapse는 file tree의 `expanded_paths`와 구분되는 core-owned durable state로 추가하고 기존 snapshot 및 dispatch 채널 안에서 전달한다.
- Pet은 새 polling process나 subprocess를 만들지 않고 기존 core snapshot에서 dashboard projection을 만든다.
  표시 설정은 기존 `pet_visible` 권한을 재사용하고, click과 drag를 구분하며 dashboard row만 focus event를 dispatch한다.
- Performance 변경은 T1 측정 gate 이후에만 확정한다.
  앱과 server sample, click-to-first-stable-frame, snapshot size 및 mutex hold, active/in-flight attach 수를 함께 보며, lock 밖 precompute와 revisioned delta wire라는 기존 architecture를 보존한다.
- MRU는 mutable label이나 activity 추정이 아니라 authoritative focused-pane 변화에서 기록한다.
  Overlay는 Swift presentation state로 유지하되 commit되는 focus는 core event를 통해서만 바꾼다.
- New Workspace는 `NSOpenPanel` 결과를 presentation state로 넘긴 뒤 confirmation modal을 연다.
  Core event 계약의 path, label, `initialize_git`는 유지하되 git init과 catalog subprocess가 runtime mutex 아래에서 실행되지 않게 하고 partial success와 failure를 명시한다.
- New Agent는 기존 bundled `AgentMark` 자산과 provider argument builder를 재사용한다.
  obsolete bypass persistence를 제거하고 modal-local one-launch option으로 제한한다.
- Pane 상태는 authoritative existence 및 closed와 attach transport의 `starting`, `attached`, `unavailable`, `ended`를 분리한다.
  pane 및 generation마다 in-flight 또는 active attempt를 하나로 제한하고, exit를 owner conflict 등 구조화 category로 기록하며 explicit Reconnect만 새 attempt를 만든다.

## 6. Requirements

- R1. IME 조합 중이 아닐 때 local 및 remote terminal의 Command-only `Delete`는 `Ctrl+U`로 번역되어 cursor 앞의 현재 입력을 지우고, 다른 modifier 및 composing event는 기존 경로를 유지한다.
- R2. `AgentBadge` 상태 점은 우하단의 현재 위치에서 `size * 0.32`보다 소폭 큰 단일 비율을 사용하고, size 16과 19에서 logo를 가리지 않는다.
  별도 spinner와 animation은 없고 working 및 unseen completion은 기존 색 의미로만 구분한다.
- R3. Sidebar의 focused agent 행은 checkout selection과 함께 시각적으로 선택됨을 보이고 accessibility state도 제공한다.
  Focus와 navigation은 pane ID로 판정해 동명 workspace 및 checkout에서도 정확하다.
- R4. 각 workspace는 접고 펼칠 수 있고 collapse 상태는 core 권한의 별도 durable state로 저장되어 relaunch 뒤 복원된다.
  `Cmd+N`, `Cmd+Shift+N`, `Cmd+K`는 각각 New Agent, New Workspace, Search를 열며 표시된 shortcut과 실제 command가 일치한다.
- R5. Settings는 사용자가 찾을 수 있는 명확한 Pet 표면을 제공하고 기존 `pet_visible` 상태와 동기화한다.
  Pet click은 agent dashboard를 열고 dashboard는 core가 실제 제공하는 total, working, done, idle, error, disconnected, workspace, kind, status, summary, elapsed, unseen, connection과 존재할 때만 ambient를 보여주며 row click은 pane ID로 focus한다.
- R6. Workspace 전환 성능 수정 전에 실행 중인 정확히 하나의 앱과 herdr server를 `/usr/bin/sample`로 측정하고 disposable A-B-A fixture의 baseline을 기록한다.
  측정 후 수정은 runtime mutex를 blocking I/O, subprocess, 큰 serialization 동안 잡지 않고, subprocess를 tick 및 event path에서 fork하지 않으며, snapshot wire의 delta contract를 유지한다.
- R7. hide 활성 상태의 `Option+Tab`은 authoritative pane-selection MRU의 agent를 overlay에서 순환하고 Option release에 focus를 commit한다.
  Search는 agent를 `Workspace > agents`로 group하고 pane ID로 이동하며 empty, one-item, cancel, removed-target를 안전하게 처리한다.
- R8. New Workspace는 macOS Finder folder picker를 먼저 열고, 선택 뒤 Hide confirmation modal에서 editable name과 git initialization을 결정한다.
  Finder cancel은 아무 modal이나 side effect를 만들지 않고, git init 실패와 registration 결과를 구분해서 표시하며 sidebar는 `WORKSPACES`와 workspace 빈 상태 문구를 사용한다.
- R9. New Agent modal은 Device, Claude 및 Codex logo tile 2개, Workspace 또는 checkout, Options, actions 순서로 구성한다.
  Bypass는 매번 OFF로 시작하고 ON 동안 warning을 계속 보여주며 선택 launch에만 Claude `--dangerously-skip-permissions` 또는 Codex `--dangerously-bypass-approvals-and-sandbox`를 붙인다.
- R10. Authoritative pane lifecycle과 attach transport lifecycle을 분리한다.
  pane 및 generation마다 attach attempt는 최대 하나이며 owner conflict나 transport EOF가 pane 자체를 closed로 만들지 않고 자동 retry 또는 takeover하지 않으며, 사용자가 명시적으로 Reconnect할 수 있다.
- R11. Attach와 전환 진단은 pane ID, generation, attempt, elapsed, exit category, retry decision을 구조화해 외부에서 관찰 가능하게 한다.
- R12. 모든 검증은 이 작업이 만든 disposable workspace, pane, agent, filesystem fixture만 변경하고 기존 사용자 세션은 관찰 이외의 대상으로 쓰지 않는다.
- R13. 구현은 기존 `HideTheme`, core snapshot 권한, six-function C ABI, Settings 및 keyboard command 패턴을 확장하며 새 외부 dependency나 별도 state authority를 만들지 않는다.
- R14. delivery는 local semantic commit으로 끝내고 public remote push, PR, CI, release를 실행하지 않는다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | Command-only `Delete`가 non-composing local 및 remote key path에서 정확히 `Ctrl+U` 하나로 번역되고 다른 modifier 및 composing input은 기존 처리로 남는다. | machine | automated behavior: shared key policy 테스트 |
| AC2 | 실제 disposable terminal에서 cursor 앞에 문자가 있는 명령을 입력한 뒤 `Cmd+Delete`를 누르면 cursor 앞의 현재 입력이 지워지고 cursor 뒤 문자는 shell의 `Ctrl+U` 의미대로 유지된다. | judged | native screenshot + terminal capture: 입력 전후와 IME 비간섭 |
| AC3 | AgentBadge 상태 점은 이전보다 조금 크고 현재 우하단 위치를 유지하며, 실제 앱의 size 16 및 19 badge 양쪽에서 Claude 및 Codex logo 핵심 형상을 가리지 않는다. 별도 spinner, 회전, pulse, blink가 없고 working과 unseen completion은 기존 색으로 구분된다. | judged | native screenshot: 16 및 19의 provider별 working/unseen 조합 |
| AC4 | Focused agent와 그 checkout이 동시에 selected style 및 accessibility state를 보이고, 동명 workspace 또는 checkout fixture에서도 pane ID가 가리키는 정확한 agent로 이동한다. | judged | automated identity test + native screenshot: 동명 fixture의 두 선택 표시 |
| AC5 | Workspace collapse를 바꾼 뒤 앱을 relaunch하면 상태가 복원되고 file tree expansion과 서로 영향을 주지 않는다. | judged | persistence test + native screenshot: relaunch 전후 |
| AC6 | `Cmd+N`, `Cmd+Shift+N`, `Cmd+K`가 각각 New Agent, New Workspace Finder picker, Search를 열고 화면 shortcut label과 일치한다. | judged | native interaction capture: 세 shortcut의 실제 결과 |
| AC7 | Settings의 명확한 Pet 표면에서 표시 여부를 바꾸면 기존 sidebar 표면과 Pet window 상태가 즉시 동기화되고 재시작 후에도 일치한다. | judged | native screenshot: Settings 및 sidebar 동기화와 relaunch |
| AC8 | Pet click이 dashboard를 열고 0개, 혼합 상태, disconnected, unseen/error를 정확히 보여주며, dashboard row click이 pane ID가 가리키는 agent로 이동한다. | judged | native screenshot + snapshot log: 상태 fixture와 focus 결과 |
| AC9 | Dashboard는 Herdr가 제공하는 필드만 표시하고 ambient가 없을 때 0으로 만들지 않으며 model token, cost, quota, context 값을 추정하거나 표시하지 않는다. Drag release는 dashboard open으로 처리되지 않는다. | judged | projection test + native capture: absent ambient와 drag/click 분리 |
| AC10 | 성능 수정 전에 정확히 하나의 설치 또는 dev app 인스턴스와 그 정체, herdr server PID, ambient load, `/usr/bin/sample` 두 결과, A-B-A baseline, active/in-flight attach 수가 증거로 남는다. | machine | logs/files: process inventory, app/server samples, baseline report |
| AC11 | 측정 뒤 warm A-B-A 전환 p95가 400ms 이하이고 baseline보다 50% 이상 빠르며, 선택 후 1초 poll을 기다리는 빈 terminal projection이 보이지 않는다. 1 pane과 15 pane fixture에서 runtime mutex가 blocking I/O, subprocess, 대형 serialization을 기다리는 sample stack이 없어야 한다. | judged | timestamped performance log + samples + native recording |
| AC12 | Snapshot의 revisioned rest, top-level scalar, terminal chunk cursor 의미가 유지되고 chunk-only update가 retained terminal state 전체를 다시 보내지 않으며 tick 또는 event당 git subprocess가 생기지 않는다. | machine | Rust tests + snapshot size/serialization counters + process trace |
| AC13 | Pane focus 변화에서 만든 MRU가 deterministic하게 갱신되고 removed pane을 제거하며 Option+Tab cycle 및 cancel pure state가 empty, one, many 케이스에서 맞는다. | machine | automated MRU/cycle tests |
| AC14 | hide 활성 상태에서 Option+Tab 반복과 Option release가 MRU agent를 정확히 선택하고 cancel은 이전 focus를 유지하며, Search가 `Workspace > agents` 구조와 정확한 pane focus를 보여준다. | judged | native screenshot/recording: empty, one, many, cancel, grouped Search |
| AC15 | New Workspace는 Finder가 먼저 열리고 cancel 시 Hide modal과 side effect가 없으며, 폴더 선택 후 확인 modal에서 이름과 git init을 정한다. Git init 성공, 선택 해제, 실패가 각각 정확히 표시되고 sidebar header 및 빈 상태는 `WORKSPACES`와 workspace 용어다. | judged | native screenshot + filesystem check: cancel, success, no-init, failure |
| AC16 | New Agent modal은 Device, Claude/Codex logo tile 2개, Workspace/checkout, Options, action 순서이며 Gemini와 Cursor가 없고, bypass OFF 및 ON warning 상태를 모두 올바르게 보여준다. | judged | native screenshot: modal 전체와 두 bypass 상태 |
| AC17 | Modal을 닫고 다시 열면 bypass가 OFF이고, Claude 및 Codex 선택은 정확한 provider flag를 launch 한 번에만 붙이며 이후 launch에는 남지 않는다. Obsolete persisted bypass 설정과 경로가 없다. | machine | pure argument tests + relaunch/modal state test + dead-path search |
| AC18 | Delayed attach와 반복 poll에서도 pane 및 generation마다 in-flight 또는 active attach가 하나를 넘지 않고, transport EOF와 owner conflict가 authoritative pane existence를 closed로 바꾸지 않는다. | machine | deterministic attach lifecycle tests + structured logs |
| AC19 | 다른 client가 소유한 disposable pane에서 hide는 `--takeover`하지 않고 unavailable 상태와 원인을 표시하며 자동 retry하지 않는다. 소유권 해제 후 explicit Reconnect 한 번으로 붙고 기존 client 및 사용자 session에는 영향이 없다. | judged | native screenshot + server/client log: conflict, no retry, reconnect |
| AC20 | Rust 및 Swift 전체 테스트와 dev app build가 통과하고, exactly-one-instance 조건에서 설치 또는 명시된 dev bundle의 실제 native screenshot 세트가 10개 요구사항을 커버한다. | judged | build/test logs + screenshot manifest + process inventory |
| AC21 | 최종 diff와 local commit에는 이 작업의 named source, tests, docs 및 receipt만 있고 기존 사용자 WIP, public push, PR, CI, release, 다른 session mutation이 없다. | machine | git diff/status/log + remote refs unchanged + Herdr before/after inventory |

## 8. PRD-Level Tasks

- T1. 측정과 안전 fixture를 먼저 만든다.
  Exactly-one app identity, herdr server, ambient load를 확인하고 disposable A-B-A workspace, 1 및 15 pane, delayed attach, external-owner conflict fixture를 준비해 app/server sample, click-to-first-stable-frame, snapshot 및 mutex timing, attach count baseline을 기록한다.
  Covers R6, R10, R11, R12, AC10.
  Depends on: none.
- T2. 측정으로 확인된 전환 병목과 attach lifecycle class를 수정한다.
  Runtime lock, delta snapshot, projection clear, required rendered layout attach 정책 중 sample과 trace가 지목한 경로만 바꾸고 authoritative pane state, transport state, one-attempt invariant, explicit Reconnect, structured diagnostics를 구현한다.
  Covers R6, R10, R11, AC11, AC12, AC18, AC19.
  Depends on: T1.
- T3. Terminal 및 Sidebar 기본 상호작용을 구현한다.
  `Cmd+Delete`, 상태 점 소폭 확대, pane-ID focus highlight, label routing class fix, durable workspace collapse, 세 command shortcut을 구현한다.
  Covers R1, R2, R3, R4, AC1-AC6.
  Depends on: none.
- T4. Agent 탐색을 구현한다.
  Focused-pane MRU, app-local Option+Tab monitor lifecycle, release commit 및 cancel, `Workspace > agents` Search grouping과 pane-ID navigation을 구현한다.
  Covers R7, AC13, AC14.
  Depends on: T3.
- T5. New Workspace 흐름과 용어를 구현한다.
  Finder first, confirmation modal, name 및 git init, partial failure 표현, subprocess lock 경계, `WORKSPACES` 문구와 obsolete raw path browser 제거를 구현한다.
  Covers R8, AC15.
  Depends on: none.
- T6. New Agent modal을 구현한다.
  Existing AgentMark logo를 재사용해 Claude/Codex 2타일과 작업 순서를 구성하고 bypass를 modal-local one-launch option으로 바꾸며 provider argument test와 obsolete persistence 삭제를 완료한다.
  Covers R9, AC16, AC17.
  Depends on: none.
- T7. Pet 설정과 dashboard를 구현한다.
  Existing `pet_visible` 권한을 재사용해 discoverable Settings surface, click과 drag 분리, snapshot projection, state summary, row focus와 empty/disconnected UI를 구현한다.
  Covers R5, AC7-AC9.
  Depends on: none.
- T8. 전체 통합 검증과 local delivery를 완료한다.
  T1 fixture에서 automated, native, performance, attach conflict 검증을 수행하고 screenshot manifest, implementation result, receipt를 만든 뒤 semantic local commit을 생성한다.
  Covers R1-R14, AC1-AC21.
  Depends on: T2, T3, T4, T5, T6, T7.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust core, Swift shell, dead path 및 bundle 건강 | none |
| automated behavior | yes | key policy, identity, persistence, MRU, provider args, snapshot delta, attach lifecycle | none |
| native desktop/runtime | yes | 10개 사용자 흐름과 visual state의 실제 macOS 동작 | user가 위임한 구현자 screenshot 판정, 최종 taste review는 optional |
| performance profiling | yes | 전환 baseline 및 개선, mutex와 server sample, attach count | none |
| live Herdr fixture | yes | pane focus, external owner conflict, reconnect, relaunch persistence | none |

Native 검증은 코드 검사나 process 생존으로 대체하지 않는다.
검증 전에 `/Applications/hide.app` 또는 명시된 dev bundle 중 어느 것을 실행하는지 기록하고 정확히 한 인스턴스만 실행한다.
설치 bundle을 교체해야 한다면 기존 bundle을 삭제한 뒤 새 bundle을 복사하고 덮어쓰지 않는다.
`peekaboo permissions status --json`으로 Screen Recording과 Accessibility를 확인하고, target app 및 window를 재탐색한 뒤 실제 screenshot을 남긴다.
성능 검증은 `/usr/bin/sample <app-pid>`와 `/usr/bin/sample <herdr-server-pid>`를 모두 포함한다.

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | automated behavior | SC1, R1, AC1, AC2 | local 및 remote `Cmd+Delete` 정책과 actual terminal line-clear 결과가 일치한다. | yes | no |
| V2 | automated behavior | SC2, R3, R4, AC4, AC5, AC6 | pane-ID identity, duplicate label, collapse persistence, real shortcut command를 검증한다. | yes | no |
| V3 | native desktop/runtime | SC2, R2, R3, R4, AC3-AC7 | size 16 및 19 badge, simultaneous selection, relaunch collapse, shortcut, Pet setting을 실제 앱에서 판정한다. | yes | no |
| V4 | native desktop/runtime | SC4, R5, AC7, AC8, AC9 | Pet dashboard의 모든 fixture 상태, 제공되지 않은 수치 부재, row focus, drag/click 분리를 검증한다. | yes | no |
| V5 | performance profiling | SC5, R6, R11, AC10, AC11, AC12 | 수정 전 baseline과 app/server sample이 존재하고 목표 latency 및 delta/lock 계약을 만족한다. | yes | no |
| V6 | live Herdr fixture | SC5, R10, R11, R12, AC18, AC19 | one-attempt invariant, external owner unavailable, no takeover/retry storm, explicit reconnect를 disposable pane에서 검증한다. | yes | no |
| V7 | automated behavior | SC6, R7, AC13 | MRU, cycle, cancel, removed target의 pure behavior를 검증한다. | yes | no |
| V8 | native desktop/runtime | SC6, R7, AC14 | app-local Option+Tab과 grouped Search의 empty, one, many, cancel을 검증한다. | yes | no |
| V9 | native desktop/runtime | SC3, R8, AC15 | Finder first, cancel, git success/no-init/failure, Workspace 용어를 검증한다. | yes | no |
| V10 | automated behavior | SC7, R9, AC17 | provider argument와 bypass one-launch non-persistence를 검증한다. | yes | no |
| V11 | native desktop/runtime | SC7, R9, AC16 | 2 logo tile, modal workflow, bypass warning을 실제 앱에서 검증한다. | yes | no |
| V12 | build/static | R13, R14, AC20, AC21 | 전체 suite와 build, exact diff, receipt, local commit, remote 및 existing Herdr state 불변을 검증한다. | yes | no |

### 9.3 Human Verification

필수 human gate는 없다.
사용자는 `$please`로 실제 screenshot을 근거로 한 size 16 및 19 상태 점 크기와 기존 UI에 맞는 visual taste 판정을 구현자에게 위임했다.
구현자는 판정 근거 screenshot을 결과에 포함하고, 사용자는 전달 뒤 원하면 최종 감성 판단만 재검토할 수 있다.

## 10. Risks And Open Decisions

- RF1. p95 400ms 및 baseline 대비 50% 개선 목표는 측정 전 agent 소유 가정이다.
  Baseline이 이미 400ms 이하거나 물리적으로 50% 개선 여지가 없으면 구현자가 임의로 수치를 바꾸지 않고 측정값과 사용자가 느낀 지연의 재현 여부를 보고한다.
- RF2. Pet click의 기존 oldest-unseen direct jump를 dashboard open으로 바꾸는 것은 사용자 원문을 따른 agent 소유 해석이다.
  상태 dashboard와 row-to-pane navigation을 완료하되 기존 동작과의 차이를 결과 보고에 명시한다.
- RF3. `Option+Tab`은 app-local로 해석했다.
  macOS 전역 interception은 시스템 및 다른 앱의 shortcut과 충돌하므로 비목표이며, 사용자가 전역 동작을 원하면 별도 권한 및 conflict PRD가 필요하다.
- RF4. Herdr snapshot에는 model token, cost, quota, context 정보가 없다.
  이를 0 또는 추정값으로 표시하지 않으며, 향후 표시하려면 Herdr protocol 변경을 별도 결정한다.
- RF5. Finder cancel, sandbox permission, git init 실패는 서로 다른 결과다.
  어느 경로도 success 또는 empty state로 묵살하지 않는다.
- RF6. 다른 client가 pane을 소유하면 attach가 실패할 수 있다.
  자동 takeover 대신 unavailable과 explicit Reconnect를 사용하므로 사용자가 소유권을 해제하기 전에는 terminal을 렌더하지 못하는 것이 정상이다.
- Open user decision 없음.

## 11. Implementation Guardrails

### Engineering principles

`~/projects/oh-my-principle` commit `35ab76ca23d45e714f1630054855a8c8c4568d03`의 engineering principles를 다음처럼 적용한다.

- Rule 1, obsolete 삭제: raw path browser, bypass persistence, label-based routing, superseded attach close path처럼 변경이 대체한 경로를 같은 change에서 제거하고 compatibility shim을 남기지 않는다.
- Rule 2, 가장 단순한 완전 구현: 상태 점은 기존 overlay 비율만 조정하고 spinner framework를 만들지 않으며, 기존 SwiftUI 및 core pattern 안에서 끝낸다.
- Rule 3, 층위 성장: measurement fixture, class fix, UI integration, full native verification 순서로 진행하되 최종 Done 범위는 축소하지 않는다.
- Rule 4, 실패 명시: Finder cancel, git init failure, CLI spawn failure, attach conflict, disconnected, malformed snapshot을 success나 empty 값으로 덮지 않는다.
- Rule 5, 모듈성: key policy, MRU, Pet projection, workspace creation, provider args, attach transport를 서로 분리하고 shell presentation이 core authority를 복제하지 않는다.
- Rule 6, established solution 탐색: official HerdrM의 pane-ID selection, selected attach, failure overlay와 reconnect를 behavioral reference로 쓰되 Hide의 multi-pane 및 no-takeover 경계에 맞춘다.
- Rule 7, 기존 것 활용: `HideTheme`, `AgentBadge`, `AgentMark`, `pet_visible`, Settings, `NSEvent` monitor lifecycle, snapshot/dispatch, `CatalogCache`를 확장하고 새 dependency를 추가하지 않는다.
- Rule 8, 장기 구조: pane existence와 attach transport를 분리하고 durable collapse 및 pane-ID identity를 단일 진실로 만들어 임시 flag를 남기지 않는다.
- Rule 9, 답할 수 있는 로그: pane ID, generation, attempt, elapsed, exit category, retry decision과 performance phase를 구조화해 기록한다.
- Rule 10, 외부 관찰 가능: attach unavailable, reconnect, git init failure, server disconnected를 UI 또는 안정된 log artifact에서 관찰할 수 있게 한다.
- Rule 11, 두 번 실행 가정: repeated poll, duplicate attach completion, double modal open, repeated Reconnect, duplicate snapshot이 중복 process나 잘못된 상태를 만들지 않는다.
- Rule 12, 테스트 가격: 사용자 관찰 결과와 pure boundary를 검증하고 SwiftUI 내부 view tree나 private wiring을 고정하는 저수익 테스트는 만들지 않는다.
- Rule 13, 실패 class 수정: duplicate label은 pane-ID identity 전체로, attach EOF는 lifecycle 분리와 one-attempt invariant로 해결하며 개별 symptom을 조건문으로 덮지 않는다.

### Environment and test practices

새 environment variable 계약은 만들지 않으므로 `engineering/practices/env.md` 변경은 없다.
`engineering/practices/test.md`에 따라 key translation, MRU, provider args, persistence, attach state machine처럼 stable input/output boundary만 자동 테스트하고 visual taste와 native interaction은 실제 앱 증거로 검증한다.

### Design principles

같은 principle commit의 design principles를 다음처럼 적용한다.

- Rule 1, data decides list shape: Search와 Pet dashboard는 실제 workspace, pane, status, ambient 존재 여부가 행과 group을 결정하게 한다.
- Rule 2, operator workflow: Finder 선택 후 확인, Device 후 Agent 및 Workspace 선택처럼 사용자의 작업 순서로 modal을 구성한다.
- Rule 3, frequent action 최소 click: shortcut과 Option+Tab, dashboard row focus를 한 흐름으로 유지한다.
- Rule 4, derived state 표시: focused agent, selected checkout, working/done/error/disconnected count와 attach unavailable을 사용자가 계산하지 않게 보여준다.
- Rule 5, 기존 패턴: `HideTheme`, checkout selection style, AgentMark, Settings pattern을 확장하고 새 visual language를 만들지 않는다.
- Rule 6, 결과 선고지: bypass ON warning, git init, explicit Reconnect와 owner conflict의 결과를 action 전에 표시한다.
- Rule 7, 상태와 구조 시각화: color dot, selection surface, `Workspace > agents` grouping, logo tile, transport state를 우선하고 설명 문장은 실패와 위험에만 쓴다.

### Project-local safety and architecture

- `herdr-core`가 모든 authoritative state를 소유하고 shell은 typed event를 보내고 snapshot을 렌더한다.
- Runtime mutex를 subprocess, blocking I/O, large serialization 동안 잡지 않고 per-tick 및 per-event git subprocess를 금지하며 snapshot delta channel 의미를 보존한다.
- 성능 수정 전 exactly-one app identity를 확인하고 app 및 herdr server 양쪽을 sample한다.
- `creator-studio`, `herdr-ide`, `modakbul`, `oh-my-principle`, `sasu`와 그 pane 및 agent는 read-only inventory 외에 건드리지 않는다.
- 검증용 이름은 `herdr-ide-verify-*` 또는 `/tmp/herdr-ide-verify-*`로 제한하고 생성 manifest를 남기며 그 manifest의 exact target만 정리한다.
- attach command에 `--takeover`를 추가하지 않는다.
- 설치 bundle을 갱신할 때는 이전 bundle을 먼저 삭제하고 새 bundle을 복사하며 overwrite하지 않는다.
- `spikes/swift-shell-pivot/`은 동결 기록이므로 수정하지 않는다.
- public push, PR, CI, release는 실행하지 않는다.
- Git commit과 PR metadata에 agent 또는 도구 attribution을 넣지 않는다.

## 12. Implementation Result Report Contract

구현자는 다음을 `implementation-result.md`와 receipt에 보고한다.

- 최종 status: Done, Partially Done, Blocked 중 하나와 미완 항목.
- 10개 사용자 요구사항별 실제 변경, 관련 R, AC, V, screenshot 또는 log evidence.
- 최초 2-B를 폐기하고 상태 점 소폭 확대만 구현했다는 명시와 최종 비율, size 16 및 19 판단 근거.
- 성능 수정 전 exactly-one app 및 server inventory, ambient load, app/server sample 파일, A-B-A baseline, 수정 후 p50/p95, snapshot 및 mutex 측정, active/in-flight attach 수.
- 측정이 지목한 실제 병목과 선택한 구조 변경, 기각한 가설, AGENTS.md Performance Guide 준수 여부.
- external-owner attach conflict, no takeover, no retry storm, explicit Reconnect의 증거.
- Finder cancel, git init success/no-init/failure와 Claude/Codex provider flag 및 bypass non-persistence의 증거.
- Pet dashboard가 표시한 실제 제공 필드와 의도적으로 표시하지 않은 token, cost, quota, context 및 absent ambient 처리.
- 전체 automated test, Rust build, Swift build, installed 또는 dev native app 검증 명령과 결과.
- native screenshot manifest와 각 screenshot의 app identity, window, fixture, AC binding.
- 시작 및 종료 시 기존 Herdr workspace, pane, agent inventory 비교와 불변 확인.
- 생성한 disposable fixture manifest, 정리 결과, 남은 artifact.
- 최종 diff 및 semantic local commit hash와 포함 파일 목록.
- public remote push, PR, CI, release를 수행하지 않았다는 확인.
- 승인된 구조 또는 범위에서 벗어난 deviation, agent 소유 가정의 실제 결론, 남은 optional human taste review.
