---
topic: "herdr-ide: Rust 네이티브 에이전트 IDE 전면 재구축"
status: "ready"
human_approval: "approved"
review_profile: "high-risk"
review_rationale: "원격 SSH 제어, 실제 에이전트 프로세스 종료, worktree 제거, loopback CDP, OpenRouter 자격증명, CEF helper 번들을 한 macOS 앱에서 다루므로 보안·파괴적 동작·프로토콜·배포 위험이 함께 존재한다."
source_intake: "current conversation"
created_at: "2026-08-26"
updated_at: "2026-08-26"
---

# PRD: herdr-ide: Rust 네이티브 에이전트 IDE 전면 재구축

## 1. Summary

`herdr-ide`를 Electron 호환 계층 없이 Rust 기반 macOS 네이티브 IDE로 전면 재구축한다.
앱은 Herdr가 소유하는 workspace, tab, pane, agent 상태 위에 빠른 실제 PTY terminal, 파일 workbench, CEF Browser pane, local·remote agent 탐색, 전역 `Option+Tab` overlay, 통합 Herdr Pet을 제공한다.
UI는 고정 3열이 아니라 왼쪽 navigator와 Ghostty처럼 tab·split을 자유롭게 쓰고 focused pane을 `Cmd+Shift+Enter`로 zoom할 수 있는 main canvas로 구성한다.
기존 Electron worktree는 시각 밀도, 상태 배지, local·remote 표기 같은 정보 구조만 읽기 전용으로 참고하며 코드, 상태 모델, 런타임은 재사용하지 않는다.

이 PRD는 승인된 `agents/prd/herdr-lightweight-ide/prd.md`의 Swift/libghostty/TUI 구조와 승인 대기 중인 `agents/prd/herdr-ide-native-shell/prd.md`의 Electron/fixed 3-column/Grab 구조를 모두 대체한다.
기존 구현과 worktree는 Rust 버전이 전체 native acceptance를 통과할 때까지 삭제하지 않는다.

첫 구현 단계는 화면 제작이 아니라 architecture preflight다.
AppKit + WGPU + CEF라는 큰 구조와 terminal, text, accessibility, shortcut, SSH, file, secret 관련 OSS를 유명 제품과 공식 프로젝트 근거로 비교하고, 정확한 revision·license·maintenance·packaging 위험을 ADR과 실행 가능한 `.app` spike로 확정한다.
현재 기준 가설은 AppKit/`objc2`, WGPU, `alacritty_terminal`, `cosmic-text`, CEF native child view이며, spike가 실패하면 본 구현을 넓히지 않고 구조 변경 승인을 다시 받는다.

### Approval checklist

- **대체와 저장소 경계** - 두 이전 PRD를 supersede하고 기존 Electron worktree는 읽기 전용 UI 참고로만 남기며, Rust 구현은 새 worktree에서 진행한다. (3장, 4.2장 HD1)
- **architecture preflight** - 본 구현 전에 제품·OSS 비교, license·maintenance 검토, terminal/IME/CEF/CDP/AX/bundle 성립성 spike와 ADR을 완료한다. (4.2장 HD2, 6장 R2-R3, 8장 T1)
- **장기 구조** - Herdr가 typed pane surface와 agent lineage의 단일 원본이 되고, AppKit이 native lifecycle, WGPU가 IDE/terminal pixels, CEF가 Browser pixels를 소유한다. (4.2장 HD3, 5장)
- **완결 범위** - tab/split/terminal, `Cmd+Shift+Enter` pane zoom, 세 sidebar view, file workbench, Browser QA, remote `mini`, shortcuts, `Option+Tab`, lineage, OpenRouter summaries, Pet까지 첫 전체 Done에 포함한다. (3장, 6장)
- **위험 경계** - destructive E2E는 exact owned fixture만 건드리고, remote 변경은 명시적 확인을 받으며, CDP는 scoped loopback endpoint만 열고, secret·transcript는 로그에 남기지 않는다. (4.2장 HD4-HD5, 11장)
- **hard performance gate** - release build가 warm usable 1초, terminal input-to-present p95 50ms, idle CPU 1%, Browser 닫힘 RSS 200MB, Browser 1개 포함 RSS 800MB 목표를 충족해야 하며 preflight가 불가능성을 보이면 수치 변경 전에 재승인을 받는다. (4.2장 HD6, 7장 AC24)
- **검증과 전달** - 설치된 `.app` 한 인스턴스를 실제 AX 입력과 screenshot으로 검증하고 Herdr snapshot, chromux, local·remote fixture로 상태를 대조하며 delivery mode는 local이다. (4.2장 HD7, 9장)

## 2. Problem, Goal, And Users

### 사용자

주 사용자는 local Mac과 원격 `mini`에서 Herdr workspace와 여러 Codex·Claude Code agent를 동시에 운영하는 개발자다.
이 사용자는 terminal, 파일, Browser QA, agent 상태와 계보를 한 앱에서 빠르게 다루고, 다른 창이나 임시 UI 상태를 통해 실제 Herdr 상태를 추측하고 싶지 않다.

### 문제

기존 Electron 시도에서는 단축키, 실제 terminal 입력, 파일트리, Browser 실행이 모두 일상 작업을 맡기기 어려울 정도로 불안정했다.
가짜 terminal 렌더링, 적은 Herdr API 사용, 고정된 화면 영역, UI별로 갈라진 상태 계산은 입력 신뢰성과 상태 일관성을 동시에 해쳤다.
Browser를 별도 창이나 plugin UI로 띄우면 agent가 요청한 Browser QA와 사용자가 보는 화면이 분리되고, remote와 Pet까지 더해질수록 현재 작업 위치와 주의가 필요한 agent를 찾는 비용이 커진다.

### 목표

Rust와 native macOS lifecycle을 기반으로 키 입력과 pane 전환에 즉각 반응하고, Herdr의 실제 상태를 모든 UI가 공유하는 daily-driver IDE를 만든다.
사용자는 workspace를 고른 뒤 tab, split, terminal, file, Browser를 자연스럽게 배치하고, 현재 pane에 집중할 때는 한 번의 shortcut으로 main canvas를 zoom하며, `Option+Tab`으로 local·remote agent 전체의 현재 상태를 확인해 정확한 pane으로 이동할 수 있어야 한다.
Browser QA는 source pane과 연결된 native Browser pane에서 chromux로 제어·검증하고, Pet은 별도 상태를 추측하지 않고 IDE와 같은 agent를 보여줘야 한다.

### 성공의 모습

사용자는 하루 작업을 설치된 `herdr-ide.app` 한 인스턴스에서 시작하고 끝낸다.
단축키 실행 뒤 UI와 Herdr snapshot이 같은 tab·pane ID와 layout을 보여주고, terminal은 한글 IME와 interactive TUI를 실제 PTY로 처리한다.
Browser, remote, file save, summary, Pet 중 하나가 실패해도 다른 기능으로 조용히 fallback하지 않고 실패 단계와 복구 경로가 화면에 남는다.

### 2.1 User Scenarios

- SC1. Workspace에서 실제 tab과 terminal pane으로 작업한다.
  Actors: 개발자, local Herdr server.
  Primary path: 개발자가 Workspaces view에서 workspace를 선택하고 `Cmd+T`, `Cmd+D`, `Cmd+Shift+D`로 실제 Herdr tab과 오른쪽·아래 terminal pane을 만든 뒤 keyboard, paste, 한글 IME, scroll, resize를 사용하며 focused terminal·editor·Browser pane에서 `Cmd+Shift+Enter`를 눌러 main canvas zoom을 켜고 끈다.
  Failure state: Herdr event sequence가 끊기거나 attach가 실패하거나 zoom target이 사라지면 빈 pane, stale zoom, 성공한 것처럼 보이는 layout을 만들지 않고 해당 surface를 stale 또는 failed로 표시하거나 zoom을 해제한 이유를 보여준다.
  Recovery: 사용자가 zoom을 다시 toggle하면 exact split ratios와 focus를 복원하고, connection을 복구하면 전체 snapshot으로 resync해 마지막으로 확인된 active tab과 pane을 복원한다.
  Reach: `herdr-ide-e2e-*` local workspace와 실제 PTY pane을 자동으로 준비하며 사용자 workspace는 사용하지 않는다.

- SC2. Agent가 요청한 Browser QA를 native pane에서 수행한다.
  Actors: 개발자, source terminal pane의 agent, chromux, local CEF Browser runtime.
  Primary path: agent가 source pane ID와 함께 Browser open을 요청하면 그 pane 오른쪽에 Browser surface가 열리고, chromux가 scoped CDP endpoint에 attach해 navigation, snapshot, click, fill, screenshot을 수행한다.
  Failure state: 비활성 workspace의 요청은 현재 focus를 빼앗지 않고 badge만 만들며, CEF 또는 CDP 연결 실패는 Browser surface와 요청 단계에 표시한다.
  Recovery: 같은 source pane이 다시 요청하면 기존 Browser surface를 재사용하고, chromux를 detach·reattach해도 페이지와 로그인 상태를 유지한다.
  Reach: disposable source pane, local test page, 전용 Browser profile과 view를 자동으로 준비한다.

- SC3. 부모 agent가 자식 agent를 만들고 두 pane이 통신한다.
  Actors: 개발자, parent agent, child agent, Herdr server.
  Primary path: parent pane 또는 IDE가 `herdr agent new`를 호출하면 새 pane과 agent가 한 번에 생성되고, 두 sidebar view에 같은 parent-child tree가 나타나며 두 agent가 고유 nonce를 요청·응답한다.
  Failure state: 같은 요청이 재시도되거나 UI 응답이 유실되어도 중복 pane·agent가 생기지 않고, parent가 먼저 종료되면 child는 사라지지 않은 채 ended parent 또는 orphan 상태를 명시한다.
  Recovery: 같은 idempotency key로 결과를 다시 조회하고, 재시작 후 Herdr snapshot에서 lineage와 pane 위치를 복원한다.
  Reach: 별도 test account 또는 설치된 agent CLI로 disposable parent·child 두 개를 만들고 nonce 외 사용자 문맥은 보내지 않는다.

- SC4. File tree와 workbench에서 local 또는 remote 파일을 안전하게 편집한다.
  Actors: 개발자, local filesystem 또는 remote SFTP service, Git.
  Primary path: 개발자가 파일을 탐색·검색해 열고 편집·저장하며 external change, diff, Git status를 같은 workspace 문맥에서 확인한다.
  Failure state: 편집 중 원본 revision이 바뀌거나 remote 연결이 끊기면 덮어쓰지 않고 충돌 또는 disconnected 상태를 표시한다.
  Recovery: 사용자는 자신의 buffer 유지, 새 원본 재읽기, diff 확인 중 하나를 선택하고 연결 복구 뒤 명시적으로 다시 저장한다.
  Reach: local과 `mini`에 disposable Git worktree, 변경 파일, 검색 fixture를 준비한다.

- SC5. Workspace, agent, worktree를 context menu에서 안전하게 관리한다.
  Actors: 개발자, Herdr server, Git.
  Primary path: 개발자는 workspace에서 Rename·Close, agent에서 Rename·Stop agent and close pane, worktree에서 Reveal·Remove checkout을 실행한다.
  Failure state: working agent 또는 dirty worktree처럼 손실 가능성이 있으면 대상, process, path, dirty 상태, 복구 가능성을 확인 전에 보여주고, 실패한 삭제를 UI에서 먼저 제거하지 않는다.
  Recovery: 확인 단계에서 취소하거나 실패 후 마지막 confirmed snapshot으로 돌아가며 안전해진 뒤 다시 실행한다.
  Reach: idle·working agent와 clean·dirty worktree를 모두 가진 exact-ID fixture를 준비한다.

- SC6. 단축키를 바꾸고 `Option+Tab`으로 전체 agent를 탐색한다.
  Actors: 개발자, macOS global shortcut service, local·remote Herdr servers.
  Primary path: 개발자는 Settings에서 physical-key 기준 shortcut을 바꾸고, `Option+Tab` overlay에서 workspace별 agent를 error, attention, working 우선순위로 본 뒤 keyboard로 정확한 pane에 이동한다.
  Failure state: macOS 예약 키나 다른 command와 충돌하거나 global registration이 실패하면 저장 또는 활성화를 조용히 성공 처리하지 않고 원인과 기존 binding 유지 여부를 보여준다.
  Recovery: 충돌을 해소해 즉시 다시 등록하거나 개별·전체 기본값으로 복원하고, overlay를 취소하면 이전 focus로 돌아간다.
  Reach: 충돌 binding fixture, idle·working·attention·error agent, disconnected remote를 준비한다.

- SC7. `mini`의 remote workspace를 local과 같은 흐름으로 사용한다.
  Actors: 개발자, local IDE, `mini`의 SSH·Herdr·SFTP service.
  Primary path: SSH config alias를 가져와 연결을 점검하고 remote badge가 붙은 workspace에서 terminal, agent, file navigation·search·edit·save·diff와 Browser 요청을 사용한다.
  Failure state: 인증, Herdr 설치, protocol version, SFTP, tunnel 중 어느 단계가 실패했는지 구분하고 remote server를 임의로 설치·업데이트·재시작·중지하지 않는다.
  Recovery: action-required 안내에 따라 사용자가 변경을 명시적으로 승인하거나 연결을 고친 뒤 reconnect하고 stale projection을 snapshot으로 교체한다.
  Reach: `mini`에 exact owned `herdr-ide-e2e-*` fixture와 target-scoped runtime 자원만 준비한다.

- SC8. IDE와 동일한 상태를 보여주는 Pet을 사용한다.
  Actors: 개발자, IDE, Herdr Pet window.
  Primary path: Settings에서 Pet을 켜면 기존 모닥불 asset을 쓰는 124x124 native window가 나타나고, IDE와 같은 최우선 agent·상태·요약을 보여주며 click하면 정확한 pane으로 이동한다.
  Failure state: 저장 위치가 화면 밖이거나 display 구성이 바뀌어도 window가 사라지지 않고, drag release를 click으로 오인하지 않으며, Pet을 꺼도 별도 polling process를 만들지 않는다.
  Recovery: visible screen으로 clamp하고 위치를 저장·복원하며 Settings에서 즉시 show/hide한다.
  Reach: 상태 우선순위 fixture, 실제 drag 좌표, off-screen 저장 좌표, 재실행 절차를 준비한다.

- SC9. Agent 종류와 작업 요약을 안전하게 확인한다.
  Actors: 개발자, Codex 또는 Claude Code agent, herdr-agent-context-labels, OpenRouter.
  Primary path: agent 행이 authoritative agent kind의 올바른 로고와 name, status, elapsed, host, summary를 보여주고, 사용자가 설명을 읽은 뒤 OpenRouter key를 Keychain에 저장해 요약을 켠다.
  Failure state: unknown agent, stale summary, key 부재, rate limit, provider error가 기본 agent 제어를 막거나 잘못된 logo·summary로 위장하지 않는다.
  Recovery: neutral identity 또는 summary error를 보여주고 key 교체·삭제·retry를 제공하며 agent control은 계속 동작한다.
  Reach: Codex·Claude·unknown metadata fixture와 stubbed summary provider를 기본으로 사용하고, 실제 OpenRouter probe는 별도 승인된 비민감 문맥에서만 실행한다.

## 3. Scope And Non-Goals

### 대체 관계와 저장소 경계

- 이 PRD가 승인되면 `agents/prd/herdr-lightweight-ide/prd.md`의 Swift/libghostty/TUI 결정과 `agents/prd/herdr-ide-native-shell/prd.md`의 Electron/TypeScript/fixed 3-column/Grab 결정은 superseded다.
- 구현은 `herdr-ide` 저장소의 새 Rust worktree에서 수행한다.
- 기존 Electron worktree는 읽기 전용 UI reference이며 diff를 만들지 않는다.
- 첫 전체 Done 전에는 Electron reference와 standalone `herdr-pet`을 제거하지 않는다.
- cross-repository 변경은 `herdr-ide`, `herdr`, `herdr-agent-context-labels`, `herdr-pet`에 한정하고 저장소별 coherent change로 유지한다.

### 포함 범위

- architecture, OSS, framework, license, maintenance, packaging research와 native risk spike.
- AppKit lifecycle과 native windows, WGPU IDE·terminal rendering, CEF native Browser child view.
- Herdr-owned workspace, tab, typed pane surface, layout, agent lifecycle, lineage snapshot과 ordered events.
- 실제 PTY terminal, tab 생성·선택·rename·close·drag reorder, split·focus·resize·close, focused terminal·editor·Browser pane의 main-canvas zoom.
- persistent left navigator의 Workspaces, Agents, Worktrees view와 flexible main split canvas.
- local·remote 파일 탐색, expansion persistence, filename/content search, open/edit/save, external-change detection, diff, Git status.
- source-pane-linked native Browser, local persistent profile, scoped loopback CDP, chromux attach·detach, remote request bridge.
- workspace·agent·worktree native context menu와 명시적 destructive confirmation.
- user-remappable command registry, physical-key matching, conflicts, default restore, global `Option+Tab` overlay.
- stable agent identity, atomic `herdr agent new`, parent-child lineage, pane-to-pane nonce communication.
- Codex·Claude Code logo, shared summary metadata, OpenRouter opt-in과 macOS Keychain.
- IDE process가 소유하는 124x124 native Pet window, 기존 asset·priority·drag·clamp·persistence behavior.
- local과 `mini` remote의 native E2E, accessibility, failure recovery, performance와 release bundle 검증.

### 명시적 비목표

- Electron, Node, DOM, webview IPC compatibility layer 또는 Electron source porting.
- fixed 3-column layout과 항상 예약된 오른쪽 Browser/file panel.
- GPUI, egui 또는 다른 full UI framework를 검증 없이 추가해 AppKit/WGPU ownership을 겹치는 것.
- CEF pixels를 WGPU off-screen rendering, IOSurface 또는 texture copy로 합성하는 것.
- Browser Grab, DOM design selection, element feedback, Browser action/context를 특정 agent pane에 전달하는 기능.
- file create, move, rename, delete와 drag-and-drop file management.
- full language-server IDE, debugger, extension marketplace, Git commit·push·PR UI, collaborative editing.
- 사용자 확인 없는 remote Herdr install, update, restart, stop 또는 임의의 SSH config 수정.
- mutable label, terminal transcript, token 존재 여부를 이용한 agent kind 또는 lineage 추측.
- 실제 사용자 workspace, pane, worktree, Browser profile을 E2E fixture로 사용하는 것.
- native parity가 검증되기 전 Electron reference 또는 standalone Herdr Pet 제거.
- Windows·Linux 지원과 첫 release의 Apple notarization·공개 배포 자동화.
- `Cmd+Shift+Enter` pane zoom을 macOS window fullscreen, 별도 window 또는 Herdr split topology 변경으로 구현하는 것.

### 제품 완결성

이 범위는 축소된 prototype이 아니라 사용자의 local·remote daily-driver 흐름을 완결하는 첫 release다.
architecture preflight는 기능 축소를 위한 spike가 아니라 실패 비용이 큰 기반을 먼저 증명하기 위한 gate다.
비목표를 다시 포함하려면 사용자 영향, 유지보수 비용, 재검토 조건을 새 PRD 또는 승인된 변경으로 명시해야 한다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

No external user-provided pre-work is required.
T1 architecture preflight remains mandatory implementation work and must pass before dependent tasks begin.
현재 implementation preflight, local Herdr, disposable fixture, unsigned local `.app` build에는 사용자가 새로 발급해야 하는 자격증명이나 구매가 없다.
실제 OpenRouter 호출은 optional verification이므로 key가 없으면 stub과 Keychain dummy secret로 필수 동작을 검증하고 live probe만 blocked로 기록한다.

### 4.2 Human Decisions Before PRD Approval

- HD1. 승인된 Swift PRD와 승인 대기 Electron PRD를 이 Rust PRD로 대체하고, 기존 Electron worktree는 native acceptance 전까지 읽기 전용 reference로 유지하는 것을 승인한다.
- HD2. architecture·product reference·OSS research와 runnable native spike를 첫 task이자 이후 작업의 hard dependency로 두고, spike 실패 시 큰 구조 변경 전에 재승인받는 방식을 승인한다.
- HD3. persistent left navigator + flexible Herdr tab/split canvas, AppKit/`objc2` + WGPU + CEF native child view라는 구조를 승인하고 fixed 3-column, GPUI/egui, WGPU Browser OSR을 거절한다.
- HD4. `herdr-ide`, `herdr`, `herdr-agent-context-labels`, `herdr-pet`의 cross-repository protocol·integration 변경과 parity 후 standalone Pet retirement를 승인한다.
- HD5. local과 `mini`에서 실제 agent, worktree remove, process stop을 검증하되 fixture manifest가 소유한 exact ID만 대상으로 삼고 기존 사용자 상태를 절대 변경하지 않는 안전 경계를 승인한다.
- HD6. warm usable 1초, input-to-present p95 50ms, idle CPU 1%, Browser 닫힘 RSS 200MB, Browser 1개 포함 RSS 800MB를 hard acceptance로 승인하며, preflight가 불가능성을 증명하면 구현자가 수치를 완화하지 않고 재결정을 요청하게 한다.
- HD7. delivery mode를 local로 두고 별도 Rust worktree에서 구현·검증하되 branch push, PR 생성, CI watching, 공개 배포는 이 PRD의 delivery에 포함하지 않는 것을 승인한다.
- HD8. OpenRouter summary를 opt-in으로 두고 최대 4,000자의 sanitized recent context가 외부로 전송될 수 있음을 사전에 고지하며 secret은 macOS Keychain에만 저장하는 경계를 승인한다.

### 4.3 Decision Traceability For Fidelity Review

**사용자 결정과 최신 disposition**

- D1. 기존 `herdr-ide` 저장소를 유지하고 기존 worktree는 참고만 한다. -> 3장 대체 관계, R1, AC1, T16.
- D2. Electron을 크게 따르지 않고 UI 구성과 정보 밀도만 참고한다. -> R1, R6, R23, 비목표 Electron compatibility.
- D3. native 반응성을 위해 Rust와 WGPU를 사용한다. -> R2-R3, AC2-AC3, T1, T4.
- D4. `Cmd+T`, `Cmd+D`, `Cmd+Shift+D`와 Ghostty-like tab management를 제공한다. -> SC1, R6-R8, AC6-AC7, T5.
- D5. 특정 source pane의 Browser QA는 IDE native Browser pane을 열고 chromux로 제어한다. -> SC2, R9-R10, AC8-AC9, T8.
- D6. Browser design action, Grab, DOM selection, action-to-pane 전달은 제외한다. -> 3장 비목표, R9, AC9.
- D7. Workspaces, Agents, Worktrees view와 workspace·agent context menu를 제공한다. -> R6, R13-R14, AC12-AC14, T6, T12.
- D8. file scope는 navigation, persisted expansion, search, open/edit/save, external change, diff, Git status이고 create/move/rename/delete는 제외한다. -> R15, AC15, T7, 비목표.
- D9. shortcut은 Settings에서 바꿀 수 있고 `Option+Tab`은 local·remote agent 현황과 focus 이동을 제공한다. -> SC6, R16-R17, AC16-AC17, T10-T11.
- D10. remote는 terminal, agent, file navigation·search·edit·save·diff를 지원하고 host·workspace를 표시하며 install/update는 확인을 받는다. -> SC7, R18, AC18, T9.
- D11. `herdr agent new`로 만든 agent는 stable parent-child lineage를 가지며 두 pane의 실제 통신을 검증한다. -> SC3, R11-R12, AC10-AC11, T3.
- D12. agent 행은 Codex·Claude Code logo와 작업 summary를 보여주고 OpenRouter key를 Settings에서 관리한다. -> SC9, R13, R19, AC12, AC19, T10.
- D13. 기존 Herdr Pet behavior와 모닥불 asset을 유지하고 IDE 설정, drag, state parity를 통합한다. -> SC8, R20, AC20, T11.
- D14. 첫 전체 Done은 Browser, remote, Pet을 포함하고 모든 주요 흐름을 E2E로 검증한다. -> R24-R26, AC23-AC25, T14-T16, V3-V8.
- D15. architecture와 stable OSS/framework 선택을 구현 첫 task에 포함한다. -> R2, AC2, T1.
- D16. 현재 합의한 stack은 AppKit/`objc2`, WGPU, `alacritty_terminal`, `cosmic-text`, CEF native child view이며 exact binding과 revision은 preflight에서 확정한다. -> R2-R3, AC2, T1.
- D17. 이전 fixed 3-column은 latest pane 요구와 충돌하므로 persistent left navigator + flexible split canvas로 대체한다. -> agent-owned recommendation, HD3 승인 대상, R6, AC3.
- D18. pane communication은 별도 chat UI가 아니라 real agent pane 사이의 unique nonce roundtrip을 뜻한다. -> R12, AC11, V7.
- D19. `Cmd+Shift+Enter`는 focused pane이 main canvas를 임시로 독점하는 zoom을 toggle하고 다시 누르면 원래 split layout으로 복귀한다. -> SC1, R27, AC26, T4-T5, T10, V2-V4.
- D20. 사용자는 T1 측정 결과를 확인한 뒤 “웅 800으로”라고 답해 Browser 1개 포함 RSS hard budget을 800MB로 변경했다. Browser 닫힘 200MB와 launch, latency, idle CPU 기준은 유지한다. -> HD6, R25, AC24, V8, RISK9.
- A1. 사용자의 “특정 pane이 전체 차지”는 macOS window fullscreen이 아니라 persistent navigator와 tab strip을 유지한 main-canvas zoom으로 해석한다. -> agent-owned assumption, 승인 checklist의 완결 범위, R27, AC26.
- A2. zoom은 tab별 transient state로 같은 app session의 tab·workspace 왕복에서는 유지하고 app relaunch에서는 해제하며, topology command는 zoom을 먼저 해제한 뒤 직전 target에 적용한다. -> agent-owned interaction assumption, R27, AC26, V2-V4.

**최신 14개 사용자 요청 trace matrix**

| 요청 | 사용자 의도 | Product contract | Verification |
| --- | --- | --- | --- |
| REQ-01 | 특정 pane의 Browser QA를 native Browser에서 수행 | SC2, R9-R10 | AC8-AC9, V5 |
| REQ-02 | `Cmd+D`, `Cmd+Shift+D` split과 Ghostty-like tab 관리 | SC1, R7-R8 | AC6-AC7, V3-V4 |
| REQ-03 | 두 agent pane의 실제 소통 | SC3, R12 | AC11, V6 |
| REQ-04 | workspace·agent 우클릭 rename과 종료 | SC5, R14 | AC13-AC14, V4 |
| REQ-05 | Workspaces, Agents, Worktrees 별도 view와 agent tree | SC1, SC3, R6, R13 | AC10, AC12, V2-V4 |
| REQ-06 | Settings에서 shortcut 변경·충돌·복원 | SC6, R16 | AC16, V2, V4 |
| REQ-07 | remote 설정과 remote workspace 표시·작업 | SC7, R18 | AC18, V7 |
| REQ-08 | Browser design·Grab·action-to-pane 기능 제외 | 3장 비목표, R9 | AC9, V5 |
| REQ-09 | 기존 Pet behavior·asset·drag·toggle·state parity | SC8, R20 | AC20, V2, V4, V8 |
| REQ-10 | workspace 선택 후 real tab·pane 생성과 분리 | SC1, R6-R8 | AC5-AC7, V3-V4 |
| REQ-11 | `herdr agent new`의 parent-child lineage tree | SC3, R11 | AC10-AC11, V2-V3, V6 |
| REQ-12 | Codex·Claude logo, 작업 summary, OpenRouter key Settings | SC9, R13, R19 | AC12, AC19, V2, V4, V9 |
| REQ-13 | `Cmd+Shift+Enter`로 focused pane zoom과 원래 split 복귀 | SC1, R27 | AC26, V2-V4 |
| REQ-14 | Browser 1개 포함 RSS hard budget을 800MB로 조정하고 나머지 성능 기준 유지 | HD6, R25 | AC24, V8 |

**검증된 현재 코드 사실과 architecture 영향**

- F1. 현재 Herdr pane은 `attached_terminal_id` 중심이고 plugin pane도 terminal-backed다. -> typed terminal/browser/editor surface protocol을 R5와 T2의 선행 변경으로 둔다.
- F2. 현재 Herdr에는 atomic `agent new`와 parent lineage field가 없다. -> R11과 T3에서 server API, CLI, snapshot, event를 함께 추가한다.
- F3. 현재 `herdr-agent-context-labels`는 summary metadata를 Herdr에 게시하고 OpenRouter key를 environment에서 읽는다. -> R19에서 single summary writer는 유지하되 key source를 Keychain으로 바꾸고 UI consumers는 metadata만 읽는다.
- F4. 현재 official Browser plugin은 pane-bound view와 scoped loopback CDP 계약을 제공한다. -> native Browser의 source/view identity와 chromux detach semantics에 재사용하고 pixels 구현은 계승하지 않는다.
- F5. 현재 Herdr Pet은 124x124 window, native drag loop, off-screen clamp, position persistence, local·remote state input을 갖는다. -> R20과 AC20의 parity baseline이다.
- F6. `cef-rs`는 macOS ARM64/x64와 CEF bundle helper를 제공하지만 AppKit/WGPU child-view integration은 제품 환경에서 증명되지 않았다. -> T1 spike와 RISK1의 hard gate다.

**제품·OSS reference disposition**

- Tide는 agent, command, code, diff, Browser를 한 workbench에 묶는 composition reference이며 code나 asset을 복사하지 않는다.
- herdrm은 Herdr-native agent list, remote grouping, live terminal navigation reference이며 terminal completeness나 Browser architecture의 근거로 사용하지 않는다.
- Ghostty는 tab, split, focus, resize, keyboard feel의 interaction reference다.
- Zed는 Rust GPU editor의 responsiveness, text surface, command architecture reference이며 GPUI 채택을 뜻하지 않는다.
- official.browser와 chromux는 source-pane identity, scoped CDP, detach ownership reference다.
- Herdr Pet은 native window, state priority, drag, asset parity reference다.
- 모든 외부 source·asset 재사용은 T1 license audit와 명시적 compatibility 확인 전까지 금지한다.

**principles intake**

- `~/projects/oh-my-principle/engineering/principles.md`와 `engineering/practices/env.md`, `engineering/practices/test.md`를 source commit `f03e930e8c5ad8c250a24d7f70be8c4889c2d6ca`에서 읽었다. -> R2, R4-R5, R21-R25, 11장 guardrails.
- `~/projects/oh-my-principle/design/principles.md`를 같은 source commit에서 읽었다. -> R6, R13-R14, R17-R18, R20, R22-R23, 11장 guardrails.
- project-local native screenshot, exactly-one-instance, remote ownership, dirty-worktree 규칙이 일반 원칙보다 구체적이므로 해당 검증·안전 경계를 우선한다. -> R14, R24, AC13-AC14, AC23, V4-V6.

**rejected 또는 deferred**

- Electron + TypeScript, Swift + libghostty/TUI, fixed 3-column, Browser Grab, remote read-only, shortcut fixed table, IDE-only lineage, fake terminal `<pre>` rendering은 rejected다.
- full LSP/debugger/extension marketplace, file mutation, cross-platform, notarization, PR delivery는 deferred non-goals이며 첫 전체 Done의 누락으로 처리하지 않는다.

## 5. Major Technical Structure Changes

### Architecture preflight와 freeze

본 구현 전에 제품 reference와 OSS 후보를 비교하고 exact version 또는 commit, license, maintainer activity, security/update path, macOS ARM64/x64, IME, Accessibility, packaging, performance, API stability를 기록한다.
비교 대상에는 최소한 Tide, herdrm, Ghostty, Zed, official.browser/chromux, Herdr Pet과 AppKit/`objc2`, WGPU, terminal/parser/text/PTY, CEF binding, AccessKit, global shortcut, file/search/editor, Keychain, SSH/SFTP, async/logging 후보가 포함된다.
GitHub stars만으로 안정성을 판단하지 않고 release history, unresolved blocker, transitive dependency, runnable spike 결과를 함께 본다.

기준 구조는 다음과 같다.

```text
local/remote Herdr server
  -> versioned snapshot + ordered events + idempotent commands
  -> host-scoped domain projection
  -> shared AgentPresentationStore
     -> left navigator / tab strip / split canvas
     -> window-local pane zoom presentation state
     -> Option+Tab overlay
     -> native Pet window

Herdr typed pane surface
  -> terminal: real PTY attach -> terminal model -> WGPU text rendering
  -> editor: file service -> WGPU editor/workbench surface
  -> browser: local CEF child NSView -> scoped CDP -> chromux
```

### Herdr protocol과 state ownership

Herdr server가 workspace, tab, split topology, pane surface identity, agent lifecycle, lineage의 유일한 원본이 된다.
각 pane leaf는 stable ID와 `terminal`, `editor`, `browser` 중 하나의 surface kind를 갖고, kind별 payload와 lifecycle event를 제공한다.
terminal-only command는 terminal surface에서만 성공하며 잘못된 kind에는 machine-readable error를 반환한다.
IDE는 snapshot과 sequence가 있는 event stream으로 projection을 만들고 sequence gap이 생기면 stale 상태를 표시한 뒤 full resync한다.
old protocol을 숨겨서 맞추는 compatibility layer는 만들지 않고 version mismatch를 명시적으로 실패시킨다.

### Agent lifecycle과 lineage

Herdr에 atomic `agent.new` server API와 `herdr agent new` CLI를 추가한다.
한 operation은 target layout의 pane 생성, agent start, stable `agent_instance_id`, `parent_agent_instance_id`, `spawned_from_pane_id`, host/workspace/tab/pane identity 기록을 함께 commit한다.
동일 idempotency key 재실행은 기존 결과를 반환하며 duplicate process나 pane을 만들지 않는다.
IDE와 terminal-invoked spawn은 같은 계약을 사용한다.

### Native process와 rendering ownership

AppKit이 `NSApplication`, main thread, main window, native menu/context menu, `Option+Tab` overlay window, Pet window, CEF child view lifecycle을 소유한다.
WGPU는 IDE chrome, navigator, tab strip, split canvas, terminal, editor, overlay pixels를 Metal 위에 그린다.
CEF는 Browser pixels와 Browser process/helper lifecycle을 소유하며 WGPU는 Browser texture를 복사하거나 합성하지 않는다.
Terminal runtime은 pane surface가 제공하는 `terminal_id` 또는 새 `herdr pane attach <pane-id>`·`herdr terminal attach <terminal-id>` 계약으로 실제 PTY stream을 attach하고 terminal state와 glyph shaping을 UI layout과 분리한다.
`herdr agent attach`는 agent-target convenience path일 뿐 terminal surface rendering의 필수 경로가 아니다.

### Pane zoom presentation state

Pane zoom은 macOS window fullscreen이나 Herdr split topology mutation이 아니라 active window와 tab에 속하는 transient presentation state다.
`Cmd+Shift+Enter`는 focused `terminal`, `editor`, `browser` pane의 stable ID를 `zoomed_pane_id`로 설정하고 main canvas의 sibling panes만 숨기며 persistent navigator와 tab strip은 유지한다.
같은 shortcut을 다시 누르면 zoom 직전의 split ratios, active pane, focus를 정확히 복원하고 tab·workspace를 바꿨다가 같은 app session에서 돌아오면 해당 tab의 zoom state를 복원하되 app relaunch는 normal split layout으로 시작한다.
숨겨진 sibling pane의 PTY process, editor buffer, Browser page·profile은 계속 살아 있고 zoom toggle은 Herdr snapshot의 pane tree, split ratios, process ownership을 변경하지 않는다.
split, move, resize, close처럼 topology를 바꾸는 command는 zoom을 먼저 명시적으로 해제한 뒤 직전 zoom target에 적용하며 target pane이 remote event로 사라지면 stale ID를 유지하지 않고 zoom 해제 이유와 새 focus를 표시한다.
focused zoomable pane이 없으면 command는 layout을 바꾸지 않고 unavailable reason을 표시한다.
zoom 상태는 tab strip과 Accessibility tree에서 읽을 수 있는 indicator로 표현하고 central command registry에서 shortcut remap과 conflict 처리를 공유한다.

### Local·remote service boundaries

local과 remote는 같은 domain interface를 쓰되 transport를 분리한다.
local Herdr는 Unix socket, remote Herdr는 SSH tunnel, remote terminal은 SSH PTY, remote files는 SFTP, Git은 target host에서 machine-readable system `git` 결과를 사용한다.
remote Browser 요청은 remote agent에서 local IDE control gateway로 session-scoped reverse bridge를 사용하고 실제 Browser는 local CEF surface다.
각 endpoint는 loopback 또는 user-only Unix socket에만 bind하며 connection, child PID, runtime directory, cleanup ownership을 operation manifest로 추적한다.

### Summary, settings, diagnostics, Pet

agent당 summary writer는 하나만 존재하고 `herdr-agent-context-labels`가 sanitized summary metadata를 Herdr에 게시한다.
IDE, sidebar, overlay, Pet은 OpenRouter를 각자 호출하지 않고 같은 metadata를 소비한다.
OpenRouter key는 macOS Keychain에 저장하며 Settings에는 presence와 health만 남긴다.
모든 shortcut은 central command registry를 사용하고 physical key, scope, precedence, conflict, global registration state를 함께 관리한다.
설정은 versioned schema와 atomic replace를 사용하고 secret을 포함하지 않는다.
structured diagnostics는 operation과 stable IDs를 포함하되 secret, transcript, raw prompt, user keystroke를 포함하지 않는다.

## 6. Requirements

- R1. 새 Rust PRD와 구현은 기존 Swift·Electron architecture를 대체한다. Rust 구현은 새 worktree에서 진행하고 Electron worktree는 native acceptance까지 읽기 전용 UI reference로 유지하며 Electron/Node runtime 또는 compatibility layer를 새 dependency graph에 포함하지 않는다.
- R2. 첫 task는 architecture·product·OSS research gate다. 유명 제품의 interaction과 현재 공식 project의 maintenance, release, license, security, macOS support, IME, Accessibility, packaging, performance, API stability를 비교하고 exact dependency revision과 선택·기각 이유를 versioned ADR로 남긴다.
- R3. architecture freeze 전 runnable release `.app` spike가 AppKit main window, WGPU surface, 실제 PTY terminal, terminal/editor/browser 한글 IME, CEF native child view, chromux CDP attach, Cmd shortcut routing, AX tree, Retina resize, focus/z-order, helper bundle, cold·warm relaunch를 증명해야 한다. 기준 가설이 실패하면 본 구현을 진행하지 않고 구조 변경 승인을 요청한다.
- R4. Herdr server는 local·remote workspace, tab, split topology, pane surface, agent lifecycle, lineage의 단일 원본이다. IDE는 host-scoped ID, initial snapshot, ordered event sequence로 projection을 만들고 gap, reconnect, version mismatch를 `connected`, `reconnecting`, `stale`, `failed`, `action required`로 구분한다.
- R5. Herdr pane protocol은 `terminal`, `editor`, `browser` typed surface와 stable identity를 표현하고 create, focus, move, split, tab, close, restore event를 snapshot과 함께 제공한다. typed terminal surface는 plain terminal과 agent-backed terminal 모두에 stable `pane_id`, optional `agent_instance_id`, required terminal attach endpoint를 반환하며 terminal command는 terminal surface에만 적용하고 Browser/editor가 IDE-local phantom layout이 되지 않게 한다.
- R6. 화면은 persistent left navigator와 flexible main tab/split canvas를 사용한다. navigator는 Workspaces(`host -> workspace -> tab -> pane`), Agents(`host -> workspace -> agent lineage`), Worktrees(`repository -> worktree -> workspace`) view를 전환하며 selection과 expansion을 저장한다.
- R7. workspace 선택은 실제 active tab, split layout, last-focused pane을 복원한다. `Cmd+T`는 실제 Herdr tab과 initial terminal pane, `Cmd+D`는 오른쪽 terminal split, `Cmd+Shift+D`는 아래 terminal split을 만들며 tab select·rename·close·drag reorder와 pane focus·resize·close가 Herdr snapshot과 일치한다.
- R8. 모든 visible terminal surface는 실제 PTY를 가지며 `herdr pane attach <pane-id>` 또는 `herdr terminal attach <terminal-id>`로 연결된다. `herdr agent attach`는 agent-backed terminal의 convenience path로만 사용하고 plain terminal과 agent-backed terminal을 모두 지원한다. ANSI, alternate screen, cursor, mouse mode, selection, clipboard, bracketed paste, scrollback, resize, 한글 IME, physical/meta key를 처리하고 attach conflict 또는 child exit를 pane 안에 표시한다.
- R9. source terminal pane의 agent 또는 사용자가 Browser open을 요청하면 source 옆 오른쪽에 native CEF Browser surface를 만들거나 같은 source의 기존 surface를 재사용한다. inactive workspace 요청은 focus를 훔치지 않고 badge를 만들며 Browser Grab, DOM selection, design action, context-to-pane 전달을 제공하지 않는다.
- R10. Browser view마다 stable view ID, source pane ID, persistent profile, session-scoped capability가 붙은 loopback CDP endpoint를 제공한다. chromux는 navigation, snapshot, click, fill, screenshot을 수행할 수 있고 detach는 CDP client만 해제하며 page, login, Browser surface를 닫지 않는다.
- R11. `herdr agent new`는 idempotency key와 target layout을 받아 pane create, agent start, lineage record를 원자적으로 수행하고 stable agent, host, workspace, tab, pane, parent, spawned-from identity를 snapshot과 events에 반환한다. IDE와 parent terminal에서 호출한 경우 모두 같은 tree를 만든다.
- R12. 두 실제 agent pane은 Herdr의 명시적 prompt/read 계약으로 unique nonce 요청과 응답을 주고받을 수 있어야 한다. lineage나 성공 여부를 terminal transcript parsing으로 추측하지 않는다.
- R13. Workspace view, Agents view, tab/pane badge, `Option+Tab`, Pet은 하나의 presentation store와 stable agent identity를 사용한다. `AgentInfo.agent` 또는 normalized authoritative kind로 Codex·Claude Code logo를 하나만 고르고 unknown은 neutral icon으로 표시하며 name, state, summary, elapsed, host를 함께 보여준다.
- R14. native context menu는 workspace에 Rename·Close, agent에 Rename·Stop agent and close pane, worktree에 Reveal·Remove checkout을 제공한다. tab·pane·workspace close가 working·attention process를 끝내면 affected agent와 summary를 한 confirmation에 모아 보여주고, agent stop과 dirty worktree remove도 exact target, process, path, dirty state, recovery consequence를 먼저 표시하며 confirmed server result 전에는 UI에서 제거하지 않는다.
- R15. local filesystem과 remote SFTP는 같은 FileService behavior를 제공한다. navigation, persisted expansion, filename/content search, open/edit/save, external-change detection, diff, Git status를 지원하고 save 전 revision을 비교해 외부 변경을 자동 덮어쓰지 않으며 file create/move/rename/delete는 제공하지 않는다.
- R16. 모든 IDE·global shortcut은 하나의 command registry에 physical key, scope, precedence, default, current binding, conflict, registration state로 등록한다. Settings는 검색, capture, 즉시 적용, restart persistence, macOS reserved·duplicate conflict, 개별·전체 reset을 제공하고 실패한 변경은 기존 유효 binding을 유지한다.
- R17. `Option+Tab`은 기본 global binding이며 local·remote agent를 workspace별로 묶고 error, attention, working을 우선하며 idle group을 기본 접는다. arrow·Tab navigation, Enter focus, Esc·재호출 close, previous-focus restore를 지원하고 terminal의 다른 Option/Meta input을 가로채지 않는다.
- R18. Remote Settings는 SSH config alias import, host add, 단계별 connection test, Herdr install/version/protocol, SFTP, tunnel capability를 보여준다. Remote workspace는 일관된 host badge와 state를 표시하고 terminal, agent, file search/edit/save/diff, source-linked Browser request를 지원하며 install/update/restart/stop은 대상과 결과를 보여준 뒤 명시적 확인을 받아야 한다.
- R19. summary는 opt-in이며 활성화 전에 최대 4,000자의 sanitized recent agent context가 OpenRouter로 전송될 수 있음을 알린다. key는 macOS Keychain에만 저장하고 config, environment, Herdr snapshot, logs, crash report에 넣지 않으며 single summary writer가 rate/debounce/error policy와 metadata publication을 소유한다.
- R20. IDE process는 기존 모닥불 asset과 status mapping을 쓰는 하나의 124x124 borderless native Pet window를 소유한다. Pet은 shared presentation store의 최우선 agent와 `_new` attention semantics를 사용하고 Settings toggle, native drag, drag-click suppression, screen clamp, display-change recovery, position persistence, click-to-exact-pane를 제공한다.
- R21. 앱과 child process가 읽는 environment variable은 code registry에 requirement, validator, fallback, missing behavior, scope가 선언되어야 한다. settings는 versioned schema와 atomic write를 사용하고 command, agent create, settings write, reconnect, cleanup은 재실행되어도 한 결과로 수렴해야 한다.
- R22. terminal, Herdr stream, Browser, CDP, SSH, SFTP, file, Keychain, OpenRouter, global shortcut, bundle helper failure는 대상, 단계, retry 가능 여부를 UI와 structured diagnostics에 노출한다. background failure를 log에만 남기거나 silent fallback하지 않고 optimistic state를 last confirmed snapshot으로 되돌린다.
- R23. WGPU custom UI는 macOS Accessibility tree에 sidebar tree, tabs, panes, buttons, menus, settings, status, terminal, editor, Browser의 meaningful role, label, value, selected, expanded, disabled state를 제공한다. keyboard-only navigation, visible focus, tooltip 또는 accessible label, sufficient contrast, reduced motion을 제공하고 Electron의 dark low-chrome density와 상태 표현만 참고한다.
- R24. E2E harness는 실행 전 existing app process, Herdr session, Browser view, `mini` state를 inventory하고 installed release `.app` 한 인스턴스만 허용한다. `herdr-ide-e2e-*` fixture를 만든 즉시 exact owned ID manifest에 기록하고 cleanup은 manifest ID만 사용하며 같은 suite를 두 번 실행해도 user state나 orphan resource를 늘리지 않는다.
- R25. target Mac release build에서 warm first usable state가 1초 이하, terminal input-to-present p95가 50ms 이하, 안정 idle CPU 평균이 1% 이하, Browser closed 7 workspace·11 pane process-tree RSS가 200MB 이하, CEF Browser 1개와 helper를 포함한 RSS가 800MB 이하여야 한다. 측정은 build identity, machine, condition, sample count, process tree를 기록하고 dev build 결과를 acceptance로 쓰지 않는다.
- R26. release `.app`은 Rust executable, CEF framework·helper·localization·resources, app icon, Pet assets를 포함하고 rpath, executable permissions, helper identity, crash-free relaunch를 검증한다. 이 PRD의 Done 시점까지 Electron reference와 standalone Pet을 유지하고, native acceptance와 parity evidence를 사용자가 확인한 뒤 별도 승인된 exact-target cleanup에서만 retirement한다.
- R27. `Cmd+Shift+Enter`는 focused terminal·editor·Browser pane이 navigator와 tab strip을 제외한 main canvas를 독점하는 zoom을 toggle하며 sibling lifecycle과 Herdr topology를 바꾸지 않고 해제 시 exact split ratios와 focus를 복원한다. command는 central registry에서 remap 가능해야 하고 missing·removed target, tab switch, topology mutation을 deterministic state transition으로 처리하며 현재 zoom state를 시각적 indicator와 Accessibility state로 노출하고 zoomable focus가 없으면 layout을 바꾸지 않은 채 unavailable reason을 보여준다.

## 7. Acceptance Criteria

- AC1. 새 PRD가 두 이전 PRD를 명시적으로 supersede하고 Rust implementation worktree만 변경되며 Electron reference worktree의 before·after diff가 동일하고 Rust runtime graph에 Electron·Node가 없다.
- AC2. T1 결과가 product·library comparison, license·maintenance evidence, exact pinned versions, accepted·rejected rationale, threat·failure boundaries를 담고, dependent task 시작 전에 runnable spike evidence가 모두 통과한다.
- AC3. release spike와 final app이 한 창에서 persistent left navigator와 flexible tab/split canvas를 그리고, Retina scale·resize·sleep/wake 후 WGPU와 CEF 사이에 blank area, overlap, z-order, coordinate, focus 오류가 없다.
- AC4. local 또는 remote event sequence gap을 주입하면 모든 dependent surface가 stale를 표시하고 임의 event 적용을 멈춘 뒤 full snapshot으로 같은 stable IDs와 state를 복원한다.
- AC5. terminal, editor, Browser를 섞은 layout이 Herdr snapshot의 real typed leaf로 보이고 앱을 재시작해 같은 tab·split·source 관계가 복원되며 close 뒤 snapshot에서도 exact leaf가 사라진다.
- AC6. `Cmd+T`, `Cmd+D`, `Cmd+Shift+D`, tab select·rename·close·drag reorder, pane focus·resize·close 직후 UI와 Herdr snapshot의 exact workspace, tab, pane, layout IDs가 일치하며 실패 시 phantom surface가 남지 않는다.
- AC7. plain terminal과 agent-backed terminal pane 모두에서 required attach endpoint를 통해 ANSI TUI, cursor, selection, clipboard, paste, scrollback, resize, mouse mode, 한글 IME, Option/Meta 입력이 동작하고 input-to-present가 R25 기준을 충족하며 child exit·takeover conflict가 pane 안에 표시된다. plain terminal에 agent-only attach를 요청하면 phantom agent를 만들지 않고 typed error를 반환한다.
- AC8. active source pane의 Browser request는 오른쪽 Browser surface를 만들고 같은 source의 재요청은 같은 stable view를 재사용하며 inactive workspace 요청은 current focus를 유지한 채 badge만 추가한다.
- AC9. chromux가 Browser view에 attach해 navigation, snapshot, click, fill, screenshot을 수행하고 detach·reattach 뒤 같은 page와 login state가 남으며 Grab·DOM design·action-to-pane UI와 command가 존재하지 않는다.
- AC10. 동일 idempotency key의 `agent new`를 두 번 실행해도 pane과 process가 하나만 생기고, parent·child rename과 app restart 뒤에도 Workspace·Agents view의 lineage가 동일하며 ended parent 관계도 보존된다.
- AC11. disposable parent와 child agent가 unique nonce를 왕복하고 두 terminal pane에서 요청·응답이 확인되며 Herdr state와 lineage event가 같은 stable IDs를 가리킨다.
- AC12. Workspaces, Agents, Worktrees view가 각각 정의된 hierarchy, selected row, expansion, remote badge를 표시하고 agent row의 logo가 authoritative kind와 일치하며 unknown kind·summary failure는 neutral state로 보인다.
- AC13. workspace·agent context menu의 rename은 Herdr snapshot과 일치하고, working agent stop 또는 tab·pane·workspace close는 종료될 process와 pane을 한 confirmation에 보여주며 실패한 close는 row를 유지한다.
- AC14. worktree Remove checkout은 exact path, dirty status, affected workspace, recovery consequence를 보여주고 dirty fixture를 confirmation 없이 제거하지 않으며 cleanup은 manifest에 기록된 exact owned worktree만 삭제한다.
- AC15. local과 `mini`에서 file navigation, persisted expansion, filename/content search, edit/save, external change, diff, Git status가 동작하고 stale revision save가 원본을 자동 덮어쓰지 않으며 create/move/rename/delete action은 노출되지 않는다.
- AC16. shortcut 변경이 즉시 적용되고 restart 뒤 유지되며 duplicate·reserved·global registration failure가 화면에 표시되고 invalid change는 이전 binding을 보존하며 `Option+letter`와 `Option+Backspace` terminal input이 overlay binding 때문에 깨지지 않는다.
- AC17. `Option+Tab` overlay가 local·remote agent를 workspace별 우선순위로 보여주고 idle을 접으며 keyboard로 exact workspace/tab/pane에 이동하고 취소 시 이전 focus를 복원한다.
- AC18. `mini` 연결이 SSH, auth, Herdr, protocol, SFTP, tunnel 단계별로 보이고 remote badge가 Herdr connection state와 일치하며 disconnect·stale tunnel·protocol mismatch가 visible recovery state가 되고 사용자 확인 없이 remote process를 변경하지 않는다.
- AC19. Codex·Claude·unknown identity, summary success·stale·error가 올바르게 표현되고 key add·replace·delete가 Keychain과 일치하며 known test key가 config, environment dump, Herdr snapshot, logs, crash report 어디에도 나타나지 않는다.
- AC20. Pet on/off, state priority, summary, agent selection이 IDE와 일치하고 실제 drag가 부드럽게 움직이며 release click을 억제하고 off-screen·display-change 좌표를 clamp하며 restart 뒤 위치와 visibility를 복원하고 click이 exact pane으로 이동한다.
- AC21. PTY, Herdr, CEF, CDP, SSH, SFTP, file conflict, Keychain, OpenRouter, global shortcut 각각의 injected failure가 target과 stage, retry 또는 action-required를 화면과 redacted diagnostics에 남기고 성공하지 않은 state를 유지하지 않는다.
- AC22. keyboard-only로 navigator, tabs, splits, context menu, settings, overlay, Pet focus action에 도달하고 AX inspection이 meaningful roles·labels·values·states를 반환하며 screenshots에서 visible focus, contrast, remote·attention·error structure가 읽힌다.
- AC23. 모든 native E2E가 시작 전 exactly one installed app instance와 owned fixture manifest를 확인하고 실제 AX/CGEvent interaction, Herdr snapshot before·after, native `screencapture`를 evidence로 남기며 suite 재실행 뒤 기존 user state와 orphan count가 변하지 않는다.
- AC24. fixed release fixture에서 warm usable, terminal latency, idle CPU, Browser closed RSS, one-Browser RSS가 각각 R25 한계를 넘지 않고 process-tree 전체와 CEF helper가 측정에 포함된다.
- AC25. packaged `.app`을 clean launch context에서 실행해 terminal, CEF, Keychain, global shortcut, Pet asset이 동작하고 helper/rpath/resources가 유효하며 이 PRD의 Done evidence가 기록되는 동안 Electron reference와 standalone Pet이 그대로 남는다.
- AC26. terminal·editor·Browser가 함께 있는 split에서 각 pane을 focus하고 `Cmd+Shift+Enter`를 누르면 해당 pane만 main canvas를 채우고 navigator·tab strip·zoom indicator가 남으며, 다시 누르면 동일한 logical split ratios와 focus가 복원되고 sibling PTY·buffer·Browser page 및 Herdr snapshot topology가 변하지 않는다. zoom 중 topology command, tab·workspace switch, target removal, shortcut remap·conflict를 수행하거나 zoomable focus 없이 command를 호출해도 blank canvas, hidden new pane, stale zoom ID, lost process가 생기지 않고 AX inspection이 zoomed state 또는 unavailable state를 반환한다.

## 8. PRD-Level Tasks

- T1. architecture·product·OSS preflight를 수행한다. Tide, herdrm, Ghostty, Zed, official.browser/chromux, Herdr Pet의 relevant pattern과 native stack 후보의 license·maintenance·security·macOS·IME·AX·bundle·performance를 비교하고 ADR을 작성하며 release `.app` spike로 WGPU, PTY, CEF, CDP, 한글 IME, shortcuts, cross-surface pane zoom bounds·focus, AX, packaging, launch metrics를 증명한다. Covers R2-R3, R23, R25-R27, AC2-AC3, AC7, AC22, AC24-AC26. Depends on: none.
- T2. Herdr protocol을 versioned typed surface, ordered event sequence, snapshot resync, host-scoped stable IDs, idempotent operation contract로 확장한다. typed terminal payload는 stable `pane_id`, optional `agent_instance_id`, required terminal attach endpoint를 제공하고 plain terminal의 agent-only attach는 typed error로 실패시킨다. Covers R4-R5, R8, R21-R22, AC4-AC7, AC21. Depends on: T1.
- T3. atomic `agent.new` API·CLI와 durable lineage lifecycle을 구현하고 IDE·terminal spawn을 같은 contract에 연결한다. Covers R11-R12, AC10-AC11. Depends on: T2.
- T4. AppKit application lifecycle, WGPU shell, persistent navigator, flexible tab/split canvas, window-local pane zoom state, native menus, accessibility bridge, shared domain projection과 presentation store를 구현한다. Covers R3-R6, R13, R22-R23, R27, AC3-AC5, AC12, AC21-AC22, AC26. Depends on: T1, T2.
- T5. 실제 PTY terminal runtime과 Herdr tab·split·focus·resize·rename·reorder·close command 및 topology-safe pane zoom toggle을 구현한다. Covers R7-R8, R27, AC6-AC7, AC26. Depends on: T4.
- T6. Workspaces, Agents, Worktrees view와 authoritative agent identity, lineage, host/state/summary presentation을 구현한다. Covers R6, R11, R13, AC10, AC12. Depends on: T3, T4.
- T7. local filesystem·remote SFTP FileService, search, editor, external-change guard, diff, Git status를 typed editor surface에 연결한다. Covers R5, R15, AC5, AC15. Depends on: T4.
- T8. CEF Browser runtime, source-linked typed Browser surface, persistent profile, loopback CDP gateway와 chromux lifecycle을 구현한다. Covers R5, R9-R10, R22, AC5, AC8-AC9, AC21. Depends on: T1, T2, T4.
- T9. SSH alias setup, staged capability test, remote Herdr projection, PTY, SFTP, Git, reverse Browser bridge, reconnect와 owned tunnel cleanup을 구현한다. Covers R4-R5, R18, R21-R22, AC4-AC5, AC18, AC21. Depends on: T2, T5, T7, T8.
- T10. central command registry, shortcut Settings, global registration, pane zoom remap·conflict handling, Keychain-backed OpenRouter Settings와 single summary-writer integration을 구현한다. Covers R16, R19, R21-R22, R27, AC16, AC19, AC21, AC26. Depends on: T4, T6.
- T11. `Option+Tab` overlay와 IDE-owned native Pet을 shared presentation store에 연결하고 기존 Pet behavior·assets를 parity한다. Covers R17, R20, R23, AC17, AC20, AC22. Depends on: T6, T10.
- T12. workspace·agent·worktree native context menu, consequence disclosure, server-confirmed mutation과 exact-ID destructive guard를 구현한다. Covers R14, R21-R22, R24, AC13-AC14, AC21, AC23. Depends on: T4, T6.
- T13. cross-process structured diagnostics, redaction, operation manifest, reconnect state와 user-visible recovery surfaces를 완성한다. Covers R4, R18-R19, R21-R22, AC4, AC18-AC19, AC21. Depends on: T8, T9, T10, T12.
- T14. exactly-one-instance installed-app E2E harness와 local·remote fixture ownership, AX/CGEvent control, Herdr snapshot comparison, native screenshot capture를 구현한다. Covers R24, AC23. Depends on: T3, T5-T13.
- T15. automated regression, protocol integration, two-real-agent nonce, Browser/chromux, `mini`, pane zoom, performance process-tree, release bundle acceptance를 실행하고 모든 required evidence를 고정한다. Covers R1-R27, AC1-AC26. Depends on: T14.
- T16. 두 이전 PRD의 superseded 상태를 명확히 하고 Electron reference와 standalone Pet의 before·after unchanged evidence와 향후 exact-target retirement 조건을 결과 보고에 남긴다. 실제 retirement는 이 PRD receipt와 사용자 확인 뒤 별도 승인된 cleanup으로 미룬다. Covers R1, R26, AC1, AC25. Depends on: T15.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust workspace와 cross-repo schema, dependency/license, bundle structure, secret contract | none |
| automated behavior | yes | projection, event gap, idempotency, layout, pane zoom state, file conflict, shortcut, status, Pet, cleanup regressions | none |
| protocol/integration | yes | 실제 local Herdr snapshot·command·typed surface·lineage | none |
| native runtime | yes | 설치된 macOS 앱의 terminal, UI, AX, shortcuts, pane zoom, context menu, overlay, Pet | 최종 시각·interaction taste는 9.3 |
| browser/CDP runtime | yes | CEF Browser surface와 chromux lifecycle | none |
| agent runtime | yes | 두 disposable real agent의 nonce roundtrip | provider 사용과 fixture safety는 HD5 |
| remote runtime | yes | `mini`의 Herdr·PTY·SFTP·Browser bridge와 recovery | machine unavailable이면 Done을 차단 |
| performance/bundle | yes | hard performance budgets와 packaged app | 수치 변경은 HD6 재승인 필요 |
| live external API | no/blockable | 실제 OpenRouter connectivity | 사용자가 key와 비민감 probe를 제공할 때만 |

### 9.2 Required Agent Verification

`Required For Done = yes`는 `Can Be Blocked`보다 우선한다.
필수 row가 blocked, unrun, failing 중 하나이면 최종 implementation status는 `Done`일 수 없고 exact blocker evidence와 함께 `Partially Done` 또는 `Blocked`여야 한다.
`Can Be Blocked = yes`는 승인된 external dependency 때문에 해당 row가 실패할 수 있다는 뜻일 뿐 `Done`에서 생략할 수 있다는 뜻이 아니다.

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked | Allowed Side Effect | Sensitive Data Policy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R3, R19, R21, R23, R26, AC1-AC3, AC19, AC22, AC25 | 네 저장소의 build·lint·schema·bundle·license checks가 통과하고 Electron reference diff가 없으며 dependency graph, config, bundle inspection이 승인된 Rust/CEF 구조와 secret boundary를 위반하지 않는다 | yes | no | build artifact와 disposable bundle 생성 | path와 account name을 redact하고 secret·transcript를 수집하지 않는다 |
| V2 | automated behavior | R4-R6, R11-R17, R19-R24, R27, AC4-AC6, AC10, AC12-AC17, AC19-AC23, AC26 | event gap과 resync, typed layout persistence, pane zoom toggle·target removal·tab switch·topology mutation, duplicate `agent.new`, lineage rename/restart, agent identity, external file conflict, shortcut conflict, status priority, Pet clamp, operation cleanup의 실제 회귀 위험이 deterministic tests로 차단된다 | yes | no | temporary state와 fake identifiers 생성 | fixture는 합성 내용만 쓰고 test secret은 출력하지 않는다 |
| V3 | protocol/integration | SC1, R4-R8, R11, R21-R22, R27, AC4-AC7, AC10, AC21, AC26 | real local Herdr에서 tab·terminal/editor/Browser leaf, split·focus·close, snapshot IDs, stale/resync, idempotent agent creation이 UI projection과 일치하고 pane zoom 전후 server topology가 변하지 않는다 | yes | no | `herdr-ide-e2e-*` local workspace와 pane 생성·종료 | manifest-owned exact IDs만 사용하고 user sessions를 읽거나 닫지 않는다 |
| V4 | native runtime | SC1, SC4-SC6, SC8-SC9, R6-R8, R13-R17, R19-R24, R26-R27, AC3, AC6-AC7, AC12-AC17, AC19-AC23, AC25-AC26 | installed release `.app` 한 인스턴스에서 실제 terminal/IME, navigator, file flow, native menus, shortcut remap, terminal·editor·Browser pane zoom과 exact restore, `Option+Tab`, Pet, failure states, keyboard/AX 접근성이 작동하고 Herdr snapshot과 native screenshots가 같은 state를 증명한다 | yes | no | owned local fixtures, app settings fixture, dummy Keychain item 생성·삭제 | screenshots와 diagnostics에 secret, transcript, username, absolute home path를 남기지 않는다 |
| V5 | browser/CDP runtime | SC2, R5, R9-R10, R18, R22-R24, AC5, AC8-AC9, AC18, AC21-AC23 | source-linked CEF Browser가 focus를 지키며 open·reuse되고 chromux navigation/snapshot/click/fill/screenshot과 detach·reattach 뒤에도 same profile/page가 유지되며 Grab surface가 없다 | yes | no | local test page, owned Browser view/profile, scoped CDP endpoint 생성 | test login만 사용하고 cookies, tokens, form secrets를 artifact에 포함하지 않는다 |
| V6 | agent runtime | SC3, R11-R13, R24, AC10-AC12, AC23 | disposable parent·child real agents가 unique nonce를 왕복하고 두 view의 lineage와 exact Herdr IDs가 일치하며 retry가 duplicate agent를 만들지 않는다 | yes | yes | nonce-only prompt와 disposable agent process 생성·종료 | 사용자 transcript나 project content를 prompt에 넣지 않는다 |
| V7 | remote runtime | SC7, R4-R5, R7-R10, R13, R15, R18, R21-R24, AC4-AC9, AC12, AC15, AC18, AC21-AC23 | `mini`의 exact owned fixture에서 remote badge, Herdr state, PTY, SFTP file flow, Git diff, Browser reverse bridge, disconnect·reconnect·stale tunnel·protocol mismatch가 동작하고 기존 remote resource는 변하지 않는다 | yes | yes | `herdr-ide-e2e-*` remote workspace, files, scoped tunnels 생성·정리 | SSH key와 remote env를 출력하지 않고 manifest-owned target만 변경한다 |
| V8 | performance/bundle | R2-R3, R8-R10, R20, R24-R26, AC2-AC3, AC7-AC9, AC20, AC23-AC25 | exact release build와 process tree에서 launch, latency, CPU, RSS가 hard budget을 통과하고 packaged CEF helper, resources, rpath, Pet asset, relaunch가 clean launch context에서 동작한다 | yes | no | release bundle 설치·실행과 owned performance fixture 생성 | metrics에는 process IDs와 build identity만 남기고 사용자 content를 기록하지 않는다 |
| V9 | live external API | SC9, R19, R22, AC19, AC21 | 사용자가 승인한 key와 비민감 synthetic context로 OpenRouter summary가 게시되고 key replace·delete와 provider error recovery가 실제 endpoint에서도 contract와 일치한다 | no | yes | 소량의 승인된 API request와 비용 발생 | synthetic 4,000자 이하 context만 전송하고 key·request body·response body를 artifact에 남기지 않는다 |

### 9.3 Human Verification

- HV1. persistent navigator, flexible tab/split canvas, pane zoom indicator와 복귀 감각이 Electron reference의 dark low-chrome density, Ghostty의 조작 감각, Tide·herdrm의 정보 grouping을 적절히 흡수했는지 최종 시각·interaction taste를 판단한다.
- HV2. workspace close, agent stop, worktree remove, remote install/update confirmation 문구가 실제 결과와 손실 가능성을 오해 없이 설명하는지 판단한다.
- HV3. OpenRouter opt-in disclosure가 어떤 context가 외부로 전송되는지 충분히 명확하고 과도하게 숨기거나 겁주지 않는지 판단한다.
- HV4. 설치 앱을 실제 하루 작업 흐름에 사용했을 때 terminal, file, Browser, agent switching, remote, Pet 사이에 반복적인 우회가 남지 않았는지 최종 daily-driver 판단을 한다.

## 10. Risks And Open Decisions

- RISK1. AppKit + WGPU + CEF는 focus, IME, Accessibility, z-order, Retina, helper bundle이 만나는 가장 큰 architecture risk다. T1 release spike의 모든 항목이 통과하기 전에는 본 UI 구현을 확장하지 않으며 실패하면 대안과 비용을 사용자에게 다시 승인받는다.
- RISK2. Herdr의 terminal-only pane 모델을 typed surface로 바꾸면 protocol 전반에 영향이 크다. compatibility shim으로 복잡성을 숨기지 않고 새 protocol revision, explicit version mismatch, cross-repo contract tests로 한 번에 전환한다.
- RISK3. CEF는 Chromium security update와 큰 helper footprint를 동반한다. exact revision, update cadence, bundle provenance, scoped CDP를 ADR에 기록하고 오래된 revision을 조용히 유지하지 않는다.
- RISK4. `Option+Tab` global registration과 terminal Meta input이 충돌할 수 있다. default는 유지하되 scope·precedence·failure visibility·remap을 제품 계약으로 두고 terminal Option sequences를 E2E로 고정한다.
- RISK5. remote tunnel, SFTP, Browser reverse bridge는 stale process와 broad bind 위험이 있다. loopback/user-only binding, scoped capability, owned PID/runtime manifest, explicit remote mutation approval로 제한한다.
- RISK6. worktree remove와 agent stop E2E는 사용자 작업을 파괴할 수 있다. prefix만 믿지 않고 creation 직후 기록한 exact IDs, dirty check, before inventory, exact cleanup으로 제한한다.
- RISK7. real agent nonce E2E는 provider availability와 비용 때문에 흔들릴 수 있다. deterministic protocol fixture를 항상 실행하고 live row도 Done 필수로 유지하되 blocked 상태는 완전 완료를 막는다고 명시한다.
- RISK8. WGPU custom editor·terminal은 native control보다 AX와 IME 구현 비용이 크다. T1에서 최소 vertical slice를 증명하고 AccessKit 또는 더 적합한 maintained bridge를 선택하되 architecture ownership 변경은 재승인받는다.
- RISK9. R25의 Browser closed 200MB와 Browser 포함 800MB budget이 release 구성에서 불가능할 수 있다. Browser 포함 기준은 T1의 CEF 단독 측정 후 사용자 결정으로 600MB에서 800MB로 변경했으며, 이후 baseline도 수치를 자동 완화하는 근거가 될 수 없다. 추가 변경은 HD6을 다시 열어 사용자가 결정한다.
- RISK10. shared summary metadata가 stale하거나 여러 writer에게 덮일 수 있다. agent당 writer ownership, revision, generated-at, provider error state를 명시하고 UI가 stale을 success로 표시하지 않는다.
- RISK11. WGPU terminal·editor와 native CEF child view는 zoom 시 bounds, focus, z-order update 경로가 달라 sibling이 겹치거나 blank surface가 남을 수 있다. T1 cross-surface spike와 V2-V4에서 동일 state transition, Herdr topology 불변, before·zoomed·restored evidence를 함께 고정한다.

PRD 승인 후 blocking open decision은 없다.
Exact crate versions, CEF Rust binding 또는 thin audited bridge, PTY/Accessibility/global-hotkey/SSH supporting libraries는 T1이 승인된 architecture 안에서 결정한다.
T1 결과가 AppKit + WGPU + CEF ownership 또는 hard performance budget을 바꿔야 한다면 implementation을 중단하고 새 human decision을 요청한다.

## 11. Implementation Guardrails

### Engineering principles

- `engineering/principles.md` rule 1, Do not preserve backward compatibility: Electron/Node compatibility, old Swift/TUI architecture, old Herdr protocol fallback을 만들지 않고 새 revision mismatch를 명시한다.
- `engineering/principles.md` rule 2, Choose the simplest implementation that fully meets the current requirements: AppKit, WGPU, CEF의 owner를 하나씩 두고 같은 surface를 두 framework가 그리거나 두 store가 계산하지 않는다.
- `engineering/principles.md` rule 3, Grow the system in layers: T1 preflight, Herdr protocol, native shell, terminal, file, Browser, remote, overlay/Pet, E2E 순서의 dependency를 건너뛰지 않는다.
- `engineering/principles.md` rule 4, Surface failures explicitly: failure를 빈 tree, blank pane, stale success, disabled control without reason으로 숨기지 않는다.
- `engineering/principles.md` rule 5, Keep components modular and concerns clearly separated: Herdr adapter, terminal runtime, Browser runtime, file service, presentation store, native windows, settings·secret boundary가 서로 transport나 UI를 직접 소유하지 않는다.
- `engineering/principles.md` rule 6, Prefer established, well-maintained libraries: T1 matrix와 spike 없이 custom PTY parser, text shaper, SSH, Keychain, accessibility bridge를 작성하지 않는다.
- `engineering/principles.md` rule 7, Lean on dependencies already in the project: Herdr protocol/plugin semantics, official.browser/chromux ownership, context-label summary policy, Herdr Pet assets·state·drag knowledge를 먼저 재사용하되 잘못된 Electron/Tauri runtime assumption은 이식하지 않는다.
- `engineering/principles.md` rule 8, Make architectural decisions for the long term: Browser/editor를 terminal-backed phantom pane으로 덮지 않고 typed surface를 Herdr source of truth로 만든다.
- `engineering/principles.md` rule 9, Log for later questions: operation, host, workspace, tab, pane, agent, Browser view, reconnect stage, performance context를 structured field로 남긴다.
- `engineering/principles.md` rule 10, Every failure must be observable outside the process: UI state, Herdr snapshot, exit status, redacted diagnostics, AX tree, screenshot 중 적합한 외부 surface에서 실패를 확인할 수 있어야 한다.
- `engineering/principles.md` rule 11, Assume every operation runs twice: agent create, Browser open/reuse, pane zoom toggle, save, settings write, reconnect, cleanup은 idempotency key, exact revision·identity 또는 deterministic state transition으로 수렴한다.
- `engineering/principles.md` rule 12, Price a test before writing it: projection·parser·idempotency·conflict·priority는 빠른 automated test로, native focus·IME·CEF·AX·Pet은 실제 runtime proof로 검증하고 brittle pixel snapshot은 쓰지 않는다.
- `engineering/principles.md` rule 13, Fix the class of failure: shortcut, state mismatch, phantom pane, stale tunnel, duplicate writer를 개별 예외가 아니라 central registry, single source, typed protocol, ownership manifest로 고친다.

### Environment and test practices

- `engineering/practices/env.md`: environment variable의 이름, requirement, validation, fallback, scope는 code registry에 있고 값만 process environment에 둔다. raw environment dump와 scattered `std::env` access를 금지한다.
- `engineering/practices/test.md`: 실제 회귀 위험을 막지 않는 UI wiring unit test와 brittle screenshot diff를 추가하지 않는다. native behavior는 installed app interaction과 state evidence를 함께 요구한다.

### Design principles

- `design/principles.md` rule 1, A list is a read view: Workspaces, Agents, Worktrees를 하나의 억지 tree에 섞지 않고 각 data 관계에 맞는 view로 보여준다.
- `design/principles.md` rule 2, The screen follows the operator's workflow: persistent navigator와 flexible canvas는 workspace 선택, agent 확인, terminal/file/Browser 작업 순서를 따른다.
- `design/principles.md` rule 3, The most frequent action takes the fewest clicks: workspace focus, tab/split creation, focused pane zoom, blocked agent focus, Browser open/reuse, Pet jump는 shortcut 또는 한 번의 primary action으로 끝낸다.
- `design/principles.md` rule 4, Show derived state: remote host, stale connection, attention priority, summary age, dirty worktree, shortcut conflict, source Browser pane, current pane zoom을 사용자가 계산하지 않게 표시한다.
- `design/principles.md` rule 5, Follow existing patterns: Electron reference의 density, Ghostty tab/split, herdrm grouping, current Herdr status token을 참고하되 현재 요구와 충돌하는 fixed layout과 Grab은 따르지 않는다.
- `design/principles.md` rule 6, State the consequence before destructive action: agent stop, workspace close, worktree remove, remote install/update는 대상과 결과를 확인 전에 설명한다.
- `design/principles.md` rule 7, Encode state and structure visually: hierarchy, focus, source relationship, pane zoom, remote, working, attention, error, stale을 indentation, icon, badge, color, placement로 표현하고 icon에는 accessible label을 둔다.

### Project-local safety and scope

- 기존 dirty·untracked 파일과 내가 만들지 않은 Herdr pane, tab, workspace, Browser view, remote service를 수정·이동·종료하지 않는다.
- native visual claim 전 exactly one app instance와 installed/dev identity를 확인하고 실제 `screencapture`를 남긴다.
- fixture cleanup은 이름 glob, current workspace, prefix match만으로 실행하지 않고 manifest에 기록한 exact owned IDs만 사용한다.
- `mini`에서 install, update, restart, stop이 필요하면 current state와 영향 범위를 먼저 보여주고 사용자 확인을 받는다.
- `_new` status만 unseen attention으로 승격하고 acknowledged status를 다시 attention으로 올리지 않는다.
- Pet drag와 disk/state work를 같은 main-thread hot loop에 넣지 않고 native gesture authority, drag-click guard, off-screen clamp를 유지한다.
- secret, raw transcript, user keystroke, full environment, Browser cookie·token을 logs, screenshots, test artifacts, implementation report에 포함하지 않는다.
- T1 이후 approved major architecture, external service, destructive boundary, performance budget, delivery mode를 바꾸거나 새 user flow를 추가하려면 먼저 사용자 승인을 받는다.
- delivery mode는 local이며 implementation은 별도 worktree를 쓰지만 branch push, PR, CI watching, publishing은 수행하지 않는다.

## 12. Implementation Result Report Contract

구현 에이전트는 다음을 보고해야 한다.

- status: `Done`, `Partially Done`, `Blocked` 중 하나와 그 판정 근거.
- user-visible changes: workspace, tab, split, pane zoom, terminal, file, Browser, agent views, remote, shortcuts, overlay, summary, Pet에서 사용자가 실제로 보게 되는 변화.
- architecture preflight: 비교한 products와 libraries, exact chosen versions·commits, license·maintenance 판정, accepted·rejected alternatives, runnable spike 결과, 남은 update risk.
- actual structure: 각 저장소의 실제 modules와 Herdr protocol/data shape, AppKit/WGPU/CEF ownership, PTY, file, SSH/SFTP, Keychain, diagnostics responsibility boundary.
- approved structure fidelity: 5장의 구조와 T1 ADR을 따랐는지, 벗어났다면 사전 승인 evidence와 이유.
- task status: T1-T16 각각의 완료·부분·차단 상태와 dependencies.
- coverage: SC1-SC9, R1-R27, AC1-AC26, V1-V9 각각의 결과와 누락 이유.
- verification evidence by mode: build/static, automated behavior, protocol/integration, native runtime, browser/CDP, agent runtime, remote runtime, performance/bundle, optional live external API.
- native evidence: exactly-one installed app identity, AX tree, actual input flow, pane zoom before·zoomed·restored state, Herdr snapshot topology invariance, native screenshots, CEF/chromux evidence, Pet drag·restore evidence.
- performance evidence: release build identity, machine, cold·warm definitions, sample counts, input-to-present distribution, idle interval, full process-tree RSS와 CEF helper 포함 여부.
- safety evidence: local·remote before inventory, fixture manifests, exact cleanup results, rerun orphan count, Electron reference diff, existing user resource non-mutation.
- security evidence: endpoint bind scope, tunnel ownership, Keychain state, secret scan, diagnostics redaction, OpenRouter disclosure와 live probe boundary.
- automated tests added or updated and each test가 막는 realistic regression risk.
- human verification HV1-HV4의 현재 상태와 남은 reviewer judgment.
- deviations, unresolved risks, not-done items, follow-up candidates와 각각이 첫 전체 Done을 차단하는지 여부.
- retirement evidence: Electron reference와 standalone Pet이 이 PRD의 Done 시점까지 유지되었는지, 향후 별도 cleanup이 사용할 exact targets와 parity 전제.
- delivery result: local worktree와 build artifact 위치만 보고하고 branch, PR, CI, publish를 수행하지 않았음을 명시한다.
