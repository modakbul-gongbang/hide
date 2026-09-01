---
topic: "Hide Workbench UX Round 3"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "다중 자동 저장이 사용자의 소스 파일을 변경하고 기존 SSH credential 경계로 원격 파일을 읽으므로 데이터 손실, path containment, 권한과 복구 동작을 엄격하게 검토해야 한다."
source_intake: "agents/interview/hide-workbench-ux-round3/qa-log.md"
created_at: "2026-09-01"
updated_at: "2026-09-01"
---

# PRD: Hide Workbench UX Round 3

## 1. Summary

Hide의 단축키, Workbench, Device 전환, Weekly Usage, 파일 탭과 Pane 닫기 및 복구를 하나의 일관된 macOS 작업 흐름으로 완성한다.
사용자는 모든 앱 명령을 실제 키 입력으로 설정하고, 현재 Device의 Files와 Recent Agents를 오가며, Codex와 Claude 사용량 및 Device 상태를 하단에서 바로 판단할 수 있어야 한다.
로컬 파일은 안전한 다중 편집 탭과 autosave를 제공하고, 원격 파일은 기존 SSH 및 SFTP 경계를 이용한 명시적 read-only 탭으로 제공한다.
Herdr는 terminal tab, Pane topology와 agent 상태의 권위자로 유지하고 Hide core는 file tab, 문서 상태, UI persistence와 복구 메타데이터만 소유한다.
마지막 Pane을 닫은 checkout은 빈 화면과 Agent 생성 CTA로 전환하며, Pane 복구는 종료된 프로세스를 되살리는 것이 아니라 검증 가능한 위치에 새 shell Pane을 만드는 동작이다.

Approval checklist:

- §3의 전체 단축키, Workbench, Usage, Device, 파일 탭, Remote Read-only, Pane 복구 범위와 Remote 편집 및 Command+Up 또는 Down 비목표를 승인한다.
- §5의 core-owned command, content, document, persistence state와 Herdr-owned topology를 분리하는 구조를 승인한다.
- §6 R4부터 R6까지의 2 MiB text, 20 MiB 및 40 MP image, checkout containment, 문서별 autosave, SFTP read-only와 stale 복구 계약을 승인한다.
- §6 R1과 R7의 context-sensitive Command+W 및 Command+Shift+Z, 마지막 Pane 빈 화면과 새 Pane 복구 의미를 승인한다.
- §9의 자동화, 실제 signed native app, remote integration, human-first 한글 IME와 app 및 herdr server 성능 검증을 Done 조건으로 승인한다.
- Repository delivery constraint는 agents/config.json의 `local` mode와 AGENTS.md의 semantic commit 규칙이며, 이 PRD 승인은 push, PR, CI 또는 merge 권한을 만들지 않는다는 점을 확인한다.

## 2. Problem, Goal, And Users

현재 Hide의 단축키는 네 개 Pane 명령 설정, SwiftUI 메뉴 shortcut, 앱 전역 monitor와 Recent Agents 전용 monitor로 분산돼 있다.
설정을 바꿔도 메뉴와 도움말이 stale할 수 있고, 실제 first responder보다 viewer visibility 같은 간접 상태가 Command+W 대상을 정한다.
현재 Workbench는 Files 단일 View이고, Weekly Usage 하단 요약은 두 provider 중 가장 높은 값 하나만 보여주며, Device Menu는 열리는 방향과 실제 remote phase를 정확히 제어하지 못한다.
현재 editor는 한 번에 한 경로와 한 draft만 소유한다.
특히 450ms debounce가 문서 identity 없이 contents만 캡처하고 실행 시점의 editor path에 저장하므로 파일 전환 시 다른 문서에 쓰일 수 있는 코드 경로상 데이터 손실 위험이 있다.
원격 Files는 목록만 있고 실제 file read 경로가 제품 runtime에 연결되지 않았다.
Herdr 0.8.2 protocol 21에는 Pane close와 split은 있지만 restore나 reopen은 없고, 마지막 Pane을 닫으면 terminal tab도 삭제된다.

목표는 한 명의 macOS 사용자가 자신의 로컬 권한과 기존 SSH 설정만으로 아래 작업을 자연스럽고 예측 가능하게 수행하는 것이다.

- 앱의 모든 사용자 명령 단축키를 한곳에서 직접 입력하고 충돌과 해제를 관리한다.
- 현재 Device의 Files와 Recent Agents를 한 번에 전환하고 Device별 마지막 작업 위치로 돌아간다.
- 두 provider의 Weekly Usage와 freshness를 팝오버를 열지 않고도 구분한다.
- 여러 로컬 파일을 안전하게 편집하고 원격 파일을 명시적 read-only 상태로 확인한다.
- 활성 파일 또는 Pane만 닫고, 데이터 손실 없이 실패를 복구하며, 닫힌 Pane 자리에 새 Pane을 만들 수 있다.

주 사용자는 Hide가 실행되는 macOS 계정의 단일 operator다.
이 사용자는 자신의 checkout, 파일 권한, SSH alias, ssh-agent와 known_hosts를 관리한다.
다중 사용자 권한, RBAC, credential 공유와 앱 내부 credential 입력은 존재하지 않는다.

### 2.1 User Scenarios

- SC1. 단축키를 직접 등록하고 실행한다.
  Actors: 단일 macOS operator.
  Primary path: 사용자가 Settings의 Shortcuts 행을 선택하고 실제 키 조합을 누르면 바인딩이 저장되며 메뉴, keycap, 도움말과 runtime command가 즉시 같은 값을 한 번만 사용한다.
  Failure state: 중복, macOS 예약키, modifier 없는 일반 문자, 지원하지 않는 키 또는 invalid legacy entry는 관련 행에 이유를 표시하고 다른 정상 바인딩을 초기화하지 않는다.
  Recovery: 사용자는 기존 항목에서 가져오기, Unset, 개별 Reset 또는 Reset All을 선택하고 재실행 후에도 결과를 유지한다.
  Reach: 기존 네 Pane string binding과 새 canonical binding을 함께 포함한 task-owned Settings fixture에서 시작한다.

- SC2. Workbench와 Device를 바꾸고 마지막 작업으로 돌아간다.
  Actors: 단일 macOS operator가 local Device와 하나 이상의 configured remote Device를 사용한다.
  Primary path: 사용자가 라벨이 있는 Files 또는 Recent Agents selector를 누르고, 위로 열리는 Device picker에서 Device를 선택하면 현재 Device의 MRU와 해당 Device 및 checkout의 마지막 content가 복원된다.
  Failure state: Device 또는 agent projection이 loading, stale, unavailable이거나 agent가 없으면 실제 상태와 빈 화면을 표시하고 다른 Device의 agent, terminal tab 또는 알 수 없는 `0 agents`를 섞지 않는다.
  Recovery: 사용자는 Files로 돌아가거나 다른 Device를 선택하며, 기억한 content가 없으면 Herdr focused 또는 active tab, 첫 server tab, empty checkout 순으로 수렴한다.
  Reach: 서로 다른 terminal tab과 agent 목록을 가진 task-owned local 및 remote Device fixture를 사용한다.

- SC3. 로컬 파일을 편집하고 원격 파일을 안전하게 읽는다.
  Actors: 단일 macOS operator와 사용자가 이미 설정한 remote SSH host.
  Primary path: 로컬 Files 또는 Command+P로 연 파일은 현재 Device와 checkout의 file tab에서 편집 및 autosave되고, remote Workbench Files에서 연 text, Markdown 또는 image는 `Remote · Read-only` tab에서 표시된다.
  Failure state: size 또는 pixel limit, binary, corrupt image, symlink escape, permission loss, external conflict, first-load failure, transport failure와 stale remote content는 문서별 상태와 원인을 표시하고 잘못된 경로나 다른 tab에 쓰지 않는다.
  Recovery: local save failure와 conflict는 Retry, Discard, Cancel을 제공하고 close와 quit를 막으며, remote stale 또는 unavailable은 Retry와 Close를 제공하고 성공 시 Fresh content로 돌아간다.
  Reach: text, unknown syntax, Markdown, image, oversized, binary, symlink, permission, conflict와 remote transport 상태를 포함한 task-owned checkout fixture를 사용한다.

- SC4. Pane을 닫고 안전하게 새 Pane으로 복구한다.
  Actors: 단일 macOS operator와 Herdr 0.8.2 local 또는 remote session.
  Primary path: terminal Pane에서 Command+W로 닫은 뒤 Command+Shift+Z를 누르면 유효한 sibling anchor, 방향, 비율과 cwd를 사용해 같은 자리에 새 shell Pane 하나가 생긴다.
  Failure state: topology가 바뀌었거나 Device 또는 tab이 사라졌으면 Hide는 위치를 추측하지 않고 이유와 가능한 fallback을 표시하며 종료된 process, agent session 또는 scrollback을 복원했다고 표현하지 않는다.
  Recovery: 사용자가 `현재 Pane 옆에 복구` 또는 Cancel을 고르고, 성공 event 뒤에만 record가 stack에서 제거된다.
  Reach: exact layout, changed layout, missing Device, missing tab과 last-Pane close를 가진 task-owned Herdr fixture를 사용한다.

- SC5. 터미널에서 macOS 방식으로 입력 줄 경계를 이동한다.
  Actors: 한글과 영문 입력 소스를 사용하는 단일 macOS operator.
  Primary path: local 또는 remote terminal에서 Command+Left와 Command+Right는 현재 shell input의 시작과 끝으로 이동하고 Option+Left와 Option+Right는 단어 단위 이동을 유지한다.
  Failure state: IME composition, candidate navigation 또는 terminal application이 입력을 소유하는 동안 key routing이 조합 문자열을 깨뜨리거나 같은 bytes를 두 번 보내지 않는다.
  Recovery: 지원하지 않는 상태에서는 추측한 cursor mutation 없이 기존 terminal encoder 경계로 전달한다.
  Reach: 사용자가 자동 입력보다 먼저 한글을 입력하는 task-owned local 및 remote terminal fixture를 사용한다.

- SC6. Provider별 Weekly Usage freshness를 판단한다.
  Actors: 단일 macOS operator.
  Primary path: Codex와 Claude의 fresh nonzero weekly percentage가 하단에 동시에 보이고 기존 70퍼센트 및 90퍼센트 상태 임계와 reset detail을 사용한다.
  Failure state: 10초를 넘긴 refresh는 Updating, 최신 실패 또는 마지막 성공 후 90초 초과는 이전 값과 muted Stale, 유효 성공값이 없거나 만료 또는 invalid이면 dash와 Unavailable을 표시한다.
  Recovery: 다음 성공 snapshot은 해당 provider만 Fresh로 되돌리고 다른 provider의 값과 상태를 바꾸지 않는다.
  Reach: 두 provider 각각 fresh, delayed, unavailable, stale, recovery와 mixed state를 생성하는 deterministic usage fixture를 사용한다.

## 3. Scope And Non-Goals

In scope:

- Search, Open File, New Agent, New Workspace, Focus Agents, Focus Terminal, Focus Workbench, 좌우 panel toggle, Recent Agents, Pane split right 및 down, zoom, active-content close와 Pane restore를 포함한 전체 앱 command registry다.
- 실제 physical key capture, Unset, per-command Reset, Reset All, context-aware duplicate, reserved shortcut rejection, explicit `기존 항목에서 가져오기`와 dynamic menu 및 help label이다.
- 기본값은 New Agent Command+N, New Workspace Command+Shift+N, Search Command+K, Open File Command+P, Focus Agents 및 Terminal 및 Workbench Command+1 및 2 및 3, left sidebar Command+B, right Workbench Command+Shift+B, Recent Agents Control+Tab과 reverse Control+Shift+Tab이다.
- 기존 Pane 기본값인 split right Command+D, split down Command+Shift+D, zoom Command+Option+Return, close Command+W와 새 Pane restore Command+Shift+Z를 포함한다.
- 기존 Option+Tab과 right Workbench Command+Option+B alias는 제거한다.
- Workbench의 Files와 Recent Agents, Device별 persistent MRU, one-click labeled selector와 last selected view persistence다.
- 하단의 Codex 및 Claude percentage, freshness state, reset detail, actual remote phase와 위로 열리는 native Device popover다.
- 현재 Device 및 checkout에 한정된 unified content tab bar, Herdr terminal server order, file open order, duplicate file focus와 Device별 last content restore다.
- local text editor, syntax highlighting, explicit dark fallback, line numbers, current-line indication, native undo and redo, Command+F, horizontal and vertical scrolling, Markdown preview, image preview, conflict handling과 autosave다.
- local 및 remote size, image pixel, binary, permission과 checkout containment boundary다.
- existing Rust SSH 및 SFTP stack을 통한 Workbench Files remote read-only text, Markdown과 image loading, cancel, retry, stale와 unavailable recovery다.
- context-sensitive Command+W, last-Pane close empty checkout, Agent 생성 CTA와 app-session-only Pane restore stack이다.
- local 및 remote terminal Command+Left와 Command+Right line-boundary behavior와 Option word movement 유지다.
- DESIGN.md와 HideTheme에 맞는 native visual, keyboard와 accessibility quality다.
- user-facing menu, help와 support documentation의 새 command 및 file capability 반영이다.

Non-goals:

- Remote file editing, save, autosave, rename, delete, upload 또는 SFTP write capability는 포함하지 않는다.
  재검토 조건은 remote atomic publish, revision conflict, credential and capability contract와 destructive recovery를 별도 PRD에서 승인하는 것이다.
- Remote Command+P와 remote terminal file-link open은 포함하지 않는다.
  이번 진입점은 Workbench Files로 한정한다.
- Command+Up과 Command+Down terminal behavior는 포함하지 않는다.
  재검토 조건은 사용자가 scroll history, shell history 또는 document boundary 중 정확한 의미를 별도로 정하는 것이다.
- LSP, autocomplete, multi-cursor, minimap, code navigation, diagnostics protocol과 IDE-grade refactoring은 포함하지 않는다.
- File tab drag reorder, Herdr terminal tab reorder와 cross-Device tab aggregation은 포함하지 않는다.
- Closed process, agent conversation, PTY, scrollback과 exact session resurrection은 포함하지 않는다.
- Pane restore stack persistence across app restart는 포함하지 않는다.
- App-owned SSH credential entry, password prompt, keychain, host-key auto-accept, sudo, remote setup과 RBAC는 포함하지 않는다.
- New provider API, OAuth, external service, background polling, per-tick subprocess 또는 new package dependency는 포함하지 않는다.
- Crash recovery용 unsaved draft byte persistence는 포함하지 않는다.
  정상 quit는 unresolved documents가 해결될 때까지 보류한다.
- Browser UI, light mode와 DESIGN.md 밖의 별도 visual system은 포함하지 않는다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
구현과 검증은 기존 repository, configured local account, task-owned Herdr fixture와 이미 설정된 SSH alias 안에서 수행할 수 있으며 새 credential, purchase, production access 또는 user-owned setup이 필요하지 않다.

### 4.2 Human Decisions Before PRD Approval

None required beyond approval of this PRD.
Q1부터 Q23까지 scope, data safety, persistence, SSH capability, UI behavior, verification과 deferred boundaries가 모두 해결됐으며 `human_approval` metadata만 최종 문서 승인 여부를 나타낸다.

### 4.3 Decision Traceability For Fidelity Review

- D-30의 최종 목표는 §1과 R1부터 R10 전체에 반영한다.
- D-01, D-02, D-11, D-16, D-21, D-22, D-24, D-32, D-33과 D-44의 shortcut 결정과 현재 구조 사실은 R1, AC1부터 AC6, T1, V2와 V7에 반영한다.
- D-03, D-04, D-14, D-25와 D-39의 Workbench 및 Recent Agents 결정과 현재 UI 사실은 R2, AC7, T5, V4와 V7에 반영한다.
- D-05, D-38과 D-45의 provider별 Usage source, freshness와 fixture 결정은 R3, AC8과 AC9, T5, V4와 V7에 반영한다.
- D-07, D-29, D-34와 D-43의 Device picker, content ownership, tab identity와 terminal selection 결정은 R3와 R4, AC10부터 AC13, T2와 T5, V3, V4와 V7에 반영한다.
- D-08, D-09, D-12, D-15, D-20, D-23, D-26, D-35, D-36, D-42와 D-46의 file tab, editor, dependency, close safety, ordering, bounds, save state와 permission 결정은 R4와 R5, AC11부터 AC18 및 AC22, T2와 T3, V3와 V8에 반영한다.
- D-19, D-31, D-37, D-40과 D-48의 Remote Read-only, single-user access, provider boundary, deferred edit와 stale 결정은 §3, R6, AC19부터 AC21 및 AC32, T4, V6와 V9에 반영한다.
- D-10, D-13, D-17, D-18, D-27과 D-28의 Herdr contract, last-Pane behavior, LIFO stack, fallback과 empty checkout 결정은 R7, AC22부터 AC26, T6, V5와 V10에 반영한다.
- D-06과 D-40의 Command arrow 결정과 explicit deferral은 §3, R8, AC27, T7와 V11에 반영한다.
- D-41과 D-47의 isolated verification, performance, human-first IME와 native design 결정은 R9와 R10, AC28부터 AC31, §9와 §11에 반영한다.
- 기존 `NSTextView + Highlightr 2.3.0 + MarkdownUI 2.4.1`, Rust `russh 0.63.1 + russh-sftp 2.4.0`, Herdr 0.8.2 protocol 21을 사용하고 새 dependency를 추가하지 않는 선택은 D-13과 D-20의 context fact 및 Q21 승인에 근거하며 §5와 R5 및 R6에 반영한다.
- 제안 후 폐기된 마지막 Pane close 차단은 Q17에서 사용자에게 거절됐고 D-28의 empty checkout 및 special restore로 대체했다.
- Q6의 Command+Up 및 Down 포함 의향은 Q7에서 명시적으로 철회됐고 §3 non-goal로 유지한다.
- Remote edit와 save는 D-19와 D-40에 따라 별도 안전 계약 전까지 deferred이며 구현자가 임의로 capability를 열 수 없다.
- D-44의 legacy shortcut decoder는 기존 사용자 설정 보존을 위한 승인된 one-time persistence migration이다.
  이는 engineering/principles.md rule 1의 일반적인 compatibility-layer 금지를 이 좁은 migration에 한해 사용자 결정이 override하며, old runtime execution path까지 유지하는 허가는 아니다.
- Principles intake는 engineering/principles.md와 design/principles.md 전체를 source commit `35ab76ca23d45e714f1630054855a8c8c4568d03`에서 읽었다.
  두 domain의 모든 rule을 §11에 번역했고 deliberately omitted rule은 없다.
- Delivery mode `local`과 post-receipt semantic commit은 agents/config.json 및 AGENTS.md의 repository constraint다.
  이는 interview에서 승인된 product decision으로 취급하지 않으며, 사용자의 current-conversation implementation 요청은 product scope를 바꾸거나 Branch, push, PR, CI와 merge 권한을 추가하지 않는다.

## 5. Major Technical Structure Changes

1. Unified command authority.
   Core-owned UI state는 stable command IDs와 versioned physical-key preferences를 저장하고, macOS shell의 단일 router는 shortcut capture, native window and modal behavior, actual first responder와 selected center content를 기반으로 command를 한 번 dispatch한다.
   분산된 hardcoded menu shortcuts, Pane-only parser와 독립 Recent Agents monitor는 제거되고 menu, Settings, keycap과 help가 동일 registry를 읽는다.

2. Typed content navigation.
   Herdr terminal tabs는 계속 server snapshot의 read-only projection이며 Hide가 순서나 topology를 복제하지 않는다.
   Hide core는 Device 및 checkout에 귀속된 file tab descriptors, open order와 per-context last selected content만 소유하고 shell은 하나의 content bar로 두 종류를 합성해 렌더링한다.

3. Root-scoped document state machine.
   기존 단일 editor snapshot과 중복된 local 및 remote file paths를 하나의 core-owned document service boundary로 통합한다.
   각 document는 identity, revision, operation generation, draft와 loading, clean, dirty, saving, save_failed, conflict, read_only, unavailable 상태를 가지며 filesystem, git, SSH/SFTP, image decode와 large serialization은 runtime mutex 밖에서 수행된다.

4. Versioned persistence migrations.
   기존 outer UI state schema는 유지하고 nested shortcut preferences version 1, Device 및 checkout file tab metadata, active content와 Workbench view 및 MRU를 additive state로 저장한다.
   legacy four-command strings는 Rust persistence boundary에서 canonical physical keys로 한 번 변환하고 다음 successful atomic save는 canonical format만 기록한다.
   Draft bytes, file contents, remote contents와 Pane restore stack은 persistence에 넣지 않는다.

5. Read-only remote file capability.
   Existing Rust SSH config, ssh-agent, known_hosts와 SFTP transport를 core document service에 연결하되 UI-facing capability는 list, stat과 bounded read only다.
   Herdr Socket API는 topology and root discovery에만 쓰고 filesystem method를 추가하지 않는다.
   Remote save 또는 write event는 shell과 FFI contract에 노출하지 않는다.

6. Pane close and reconstruction state.
   Core는 close 전에 bounded metadata를 capture하고 official Herdr close completion 뒤 tab-scoped LIFO record를 추가한다.
   Restore는 valid sibling anchor에 targeted split을 요청하며 `layout.apply`로 살아 있는 Pane 전체를 재구성하지 않는다.
   Last-Pane close가 tab deletion을 유발한 경우에만 saved cwd로 new tab root Pane을 만드는 narrow exception을 사용한다.

7. Usage freshness projection.
   기존 provider data readers와 30-second refresh cadence는 유지하고 last-success timestamp, in-flight duration, last failure와 presentation state를 snapshot에 추가한다.
   No provider API, auth 또는 additional timer를 도입하지 않는다.

## 6. Requirements

- R1. Hide는 전체 앱 command를 하나의 configurable registry와 router에서 관리해야 한다.
  Registry는 stable ID, user-visible title, default physical chord, active context와 menu placement를 가진다.
  Settings는 실제 key capture, Unset, per-command Reset, Reset All, reserved-key reason, overlapping-context duplicate와 explicit transfer를 제공한다.
  Existing valid user bindings를 보존하고 new default collision은 new command만 Unset하며 invalid or unknown entry는 affected command만 비활성화한다.
  Command routing priority는 recording, separate Settings window, sheet or modal or picker, actual native text responder, selected file content, selected terminal content, empty context 순이다.
  Editable file에서 Command+Shift+Z는 native Redo이고 read-only preview에서는 Pane restore로 leak하지 않으며 terminal에서만 restore command를 실행한다.
  Menu, toolbar keycap과 help text는 effective registry chord를 표시하고 command가 실제 AppKit focus target을 이동시켜야 한다.
  Key repeat와 menu 및 monitor 이중 수신은 non-repeatable command를 두 번 실행하지 않아야 한다.

- R2. Right Workbench는 Files와 Recent Agents를 라벨이 있는 one-click segmented selector로 제공해야 한다.
  Selected view는 재실행 후 복원한다.
  Recent Agents는 현재 Device의 agent만 persisted Device-specific MRU로 정렬하고 existing local agent refresh 또는 remote snapshot refresh에서 사라진 entry를 제거한다.
  Agent click은 existing ownership-aware selection boundary를 사용하고 no-agent, loading, stale, unavailable을 별개 상태로 표시한다.
  MRU를 위한 polling, subprocess 또는 cross-Device fallback을 추가하지 않는다.

- R3. Bottom status area는 Codex와 Claude Weekly Usage를 항상 동시에 `logo + percentage or dash`로 보여주고 Device selector를 control 위로 여는 native popover로 제공해야 한다.
  Weekly window는 정확히 10,080분이고 data source는 Claude의 local `.usage-cache.json` weekly entry와 Codex local session `rate_limits` event의 exact 10,080-minute value다.
  Existing 30-second reader cadence를 유지하고 provider network API, auth 또는 alternate estimate를 사용하지 않는다.
  Fresh usage는 existing success, warning at 70 percent, danger at 90 percent policy를 사용한다.
  Refresh가 10초를 넘으면 previous value and Updating, latest failure 또는 last success age가 90초를 넘으면 previous value and muted Stale with timestamp and reason, no valid success or expired or invalid data는 dash and Unavailable을 표시한다.
  Next successful snapshot은 해당 provider만 Fresh로 되돌린다.
  Detailed popover는 reset time, freshness, last checked와 failure reason을 유지한다.
  Device rows는 actual remote phase, selected state와 known agent count만 표시하고 registration을 ready로 또는 unknown count를 zero로 표현하지 않는다.

- R4. Unified content bar는 현재 Device와 checkout에 속한 Herdr terminal tabs와 Hide file tabs만 표시해야 한다.
  Terminal tabs는 server order를 보존하고 file tabs는 그 뒤에 open order로 표시하며 drag reorder를 지원하지 않는다.
  File identity는 Device ID, checkout ID와 canonical target inside canonical checkout root이고 persistence는 checkout-relative logical path를 사용한다.
  Same target or inside-root symlink alias를 다시 열면 existing tab에 focus한다.
  File tab selection은 owning Device and checkout을 선택하고 Device switch는 remembered last content, Herdr focused or active tab, first server terminal tab, empty checkout 순으로 복원한다.
  Missing Device, checkout or path는 다른 context로 fallback하거나 tab을 삭제하지 않고 owner label, Retry와 Close를 가진 unavailable tab으로 유지한다.
  Persisted state는 descriptors, order와 selections만 포함하고 file bytes, drafts와 revisions는 포함하지 않는다.

- R5. Local file tabs는 production-quality lightweight editor와 loss-safe document lifecycle을 제공해야 한다.
  Text and Markdown은 2 MiB까지 읽고 unknown syntax에도 readable dark foreground, insertion point와 selection을 제공한다.
  Editor는 line numbers, current-line indication, native undo and redo, Command+F, horizontal and vertical scrolling, syntax highlight, Markdown edit and preview와 supported image preview를 제공한다.
  Encoded image는 20 MiB, decoded dimensions는 40 MP까지 허용하고 background ImageIO downsample을 사용한다.
  Unsupported binary, NUL content, corrupt image, oversized content와 checkout escape는 bytes를 전체 load하거나 decode하지 않고 reason and limit을 표시한다.
  Every tree, Search, terminal link and restore entry point는 동일 canonical root resolver를 사용한다.
  Autosave는 document identity and expected revision을 capture하고 one in-flight operation per document로 serialize하며 edits during save는 latest draft 하나로 coalesce한다.
  Stale 또는 duplicate completion은 다른 document나 newer state에 적용되지 않고 같은 request가 두 번 와도 한 번만 publish한다.
  Retry는 expected revision guard를 유지해 external content를 overwrite하지 않는다.
  Discard는 consequence를 먼저 알린 뒤 current disk version으로 reload and close하거나 missing file draft를 폐기하고 close하며 Cancel은 tab and draft를 유지한다.
  Dirty, saving, save_failed와 conflict는 Command+W와 normal quit를 resolve될 때까지 막는다.
  Readable but unwritable file은 read-only, unreadable file은 unavailable with Retry and Close, permission loss during save는 original bytes and draft를 보존한 save_failed로 처리한다.
  App은 chmod, permission bypass 또는 checkout 밖 alternate save를 시도하지 않는다.

- R6. Remote Workbench Files는 existing core-owned SSH and SFTP stack을 통해 bounded read-only content를 제공해야 한다.
  UI capability는 root-scoped list, stat, cancelable bounded read와 explicit Refresh뿐이고 save, write, rename, delete, upload와 autosave event는 존재하지 않는다.
  Text and Markdown 2 MiB, image 20 MiB and 40 MP와 binary 및 corrupt-content policy는 local과 동일하다.
  Workbench Files node는 stable checkout-relative identity로 open 가능해야 하고 server-side canonical containment를 확인한다.
  Loading operation은 operation ID, attempt와 Cancel을 가지며 duplicate same-file open은 하나로 합치고 Device switch or tab close는 owned operation을 invalidate and cancel한다.
  Auth, host key, DNS, timeout, disconnect, SFTP subsystem, file permission, not-found, root escape, too-large, invalid UTF-8, unsupported type, corrupt image와 Cancel을 structured stage로 구분한다.
  Previous successful content가 없는 failure는 Unavailable with Retry and Close다.
  Time alone does not make remote content stale.
  Device phase stale or unavailable 또는 explicit Refresh failure after success는 last content, loaded-at timestamp와 reason을 Stale로 유지하고 Retry and Close를 제공하며 success는 new bytes and Fresh로 돌아간다.
  No automatic retry, polling, shell cat fallback, password prompt, host-key acceptance, credential storage 또는 Herdr filesystem API를 추가한다.
  Unsupported ProxyJump, ProxyCommand, encrypted-key and multi-IdentityFile cases는 terminal을 local로 바꾸지 않고 Files capability만 action-required unavailable로 표시한다.

- R7. Command+W와 Command+Shift+Z는 active content and Herdr topology에 맞는 close and restore behavior를 제공해야 한다.
  File content에서 Command+W는 R5 save guard를 거친 active file tab만 닫는다.
  Terminal content에서 Command+W는 focused Pane만 닫고 마지막 Pane도 허용한다.
  Last-Pane close로 Herdr tab이 삭제되면 Hide는 fake zero-Pane tab을 만들지 않고 checkout empty state, `Agent를 추가하세요` copy와 existing New Agent CTA를 표시한다.
  Remote에서 Agent start capability가 없으면 CTA를 enabled처럼 속이지 않고 unavailable reason을 표시한다.
  Close-confirmed Pane record는 Device, checkout, tab, cwd, label, parent split direction and ratio와 sibling anchor를 tab-scoped maximum 20-item LIFO in memory에 저장한다.
  Valid anchor and topology가 남아 있으면 targeted split로 new shell Pane 하나를 만들고, changed topology는 `현재 Pane 옆에 복구`와 Cancel을 제시한다.
  Cancel and failure는 record를 유지하고 successful created event 뒤에만 pop하며 in-flight repeat는 duplicate Pane을 만들지 않는다.
  Last-Pane close cause가 확인된 record만 saved cwd로 new Herdr tab and root Pane을 만들 수 있고 다른 missing tab은 explicit failure다.
  Original process, agent command, conversation와 scrollback은 자동 재실행하지 않고 prior Agent에는 explicit restart action만 제공한다.

- R8. Local and remote terminal input은 Command+Left와 Command+Right를 current input line beginning and end로, Option+Left와 Option+Right를 existing word movement로 처리해야 한다.
  File editor는 native macOS arrow behavior를 유지한다.
  IME composition과 terminal application input을 깨뜨리거나 duplicate bytes를 보내지 않는다.
  Command+Up과 Command+Down은 변경하지 않는다.

- R9. 모든 visible surface는 DESIGN.md와 HideTheme을 일관되게 사용해야 한다.
  Four-step dark surface ladder, 1px hairline borders, no drop shadows, Inter with ss03, 6px to 16px radius scale와 spacing tokens를 사용한다.
  Saturated provider or status accents는 작은 marks and state feedback에만 쓰고 chrome fill로 확장하지 않는다.
  Settings rows, segmented selector, usage badges, Device popover, content tabs, editor state banners, empty checkout와 restore choice는 existing compact control and keycap patterns를 확장한다.
  State는 icon, badge, label과 hierarchy를 함께 사용하고 color alone에 의존하지 않는다.
  Every icon-only action은 accessibility label and tooltip을 가지고 keyboard navigation, Enter selection, Escape dismissal, visible focus와 reduced-motion friendly transitions를 지원한다.

- R10. Core authority, observability and performance boundary를 지켜야 한다.
  Herdr Socket API는 session snapshot, ordered topology, focus and layout read에 사용하고 CLI 또는 existing wrapper는 terminal streaming and higher-level Pane action에 사용한다.
  Implementation 전에 installed `herdr --version`, generated schema와 pinned contract를 다시 확인하고 protocol mismatch를 action-required failure로 표시한다.
  Runtime mutex는 filesystem, git, SSH/SFTP, subprocess, image decode 또는 large serialization 동안 hold하지 않는다.
  Usage, MRU, remote file과 Pane restore를 위한 new polling, per-tick process 또는 duplicate refresh source를 만들지 않는다.
  Every async result는 document, Device, tab and operation identity를 확인하고 late completion을 다른 context에 적용하지 않는다.
  Save, remote read, UI-state persistence, Pane close and restore failure는 structured stage, operation ID, target, retryability와 user-visible error를 남긴다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | Settings가 R1에 열거된 모든 command와 승인된 default chord를 한 registry에서 표시하고 Option+Tab 및 Command+Option+B alias가 effective binding에 남지 않는다. | machine | - |
| AC2 | Physical capture, Unset, individual Reset, Reset All, reserved rejection, overlapping-context duplicate와 explicit transfer가 다른 정상 binding을 손상하지 않고 수렴한다. | machine | - |
| AC3 | Legacy four-command string bindings가 canonical shortcut_preferences version 1로 losslessly migration되고 invalid 또는 unknown entry는 affected command만 비활성화하며 다음 atomic save가 canonical format만 기록한다. | machine | - |
| AC4 | One key event가 recording, native window or modal, actual text responder, file, terminal and empty context precedence에 따라 최대 한 command만 실행하고 non-repeatable command는 repeat 또는 menu-monitor duplication으로 두 번 실행되지 않는다. | machine | - |
| AC5 | Focus Agents, Terminal and Workbench가 실제 first responder를 옮기고 menu, keycap과 help label이 effective shortcut을 즉시 표시한다. | judged | scripted native shortcut flow: rebind focus and panel commands, invoke each from main and Settings windows, capture focus target and visible labels |
| AC6 | Control+Tab and Control+Shift+Tab이 current Device Recent Agents를 forward and reverse로 전환하고 hold modifier release에 한 번 commit하며 Option+Tab은 app command로 소비되지 않는다. | judged | scripted native switcher flow: cycle both directions across three agents, release Control, verify selected agent and terminal input preservation |
| AC7 | Workbench의 labeled Files and Recent Agents selector, persisted selected view와 Device-specific MRU가 current snapshot만 사용하고 no-agent, loading, stale and unavailable을 다른 Device data 없이 표시한다. | judged | scripted two-Device Workbench flow with restart, stale and empty snapshots, row selection and screenshots |
| AC8 | Bottom bar가 Codex와 Claude를 동시에 logo plus percentage or dash로 표시하고 fresh values에 70 and 90 percent thresholds를 적용하며 color 외 label and accessibility value를 제공한다. | judged | native usage-state gallery showing both providers at below-70, 70, 90, unavailable and mixed values |
| AC9 | Provider state가 exact 10,080-minute Claude local-cache and Codex local-session rate-limit sources와 existing 30-second cadence만 사용하고 10-second Updating, 90-second Stale, no-history Unavailable와 next-success Fresh recovery를 provider별로 독립 적용한다. | machine | - |
| AC10 | Device picker가 control 위로 열리고 selected state, actual remote phase와 known count만 표시하며 unavailable selection failure가 current context를 보존한다. | judged | native Device popover flow with ready, loading, stale and unavailable remote fixtures, keyboard selection and screenshot |
| AC11 | Unified content bar가 current Device and checkout의 terminal server order와 file open order를 보존하고 same canonical target을 duplicate 없이 focus하며 drag reorder를 제공하지 않는다. | machine | - |
| AC12 | Device or checkout switch가 remembered content, Herdr focused or active tab, first server tab, empty checkout 순으로 수렴하고 missing owner or path를 explicit unavailable tab으로 보존한다. | machine | - |
| AC13 | Restart가 file descriptor, order, selected Workbench view, Device-specific MRU와 per-context last selection을 복원하되 draft bytes, file content, revision과 Pane restore record를 persisted state에 포함하지 않는다. | machine | - |
| AC14 | Local unknown-syntax text, supported source, Markdown and image tab이 readable dark editor or preview, line numbers, current line, native undo and redo, Command+F와 both-axis scroll을 제공한다. | judged | native local-file gallery and interaction trail covering unknown syntax, code, Markdown edit-preview, image, find, undo-redo and both scroll axes |
| AC15 | Text 2 MiB, encoded image 20 MiB, decoded image 40 MP, binary, corrupt image와 canonical checkout containment boundaries가 all entry points에서 일관되며 oversized or unsafe bytes를 전체 read or decode하지 않는다. | machine | - |
| AC16 | Document-specific serialized autosave가 A draft를 B path에 쓰지 않고 concurrent edit, repeated request와 late completion을 latest revision으로 수렴시키며 successful publish가 original metadata and durability contract를 보존한다. | machine | - |
| AC17 | Dirty, saving, save_failed and conflict가 Command+W와 normal quit를 막고 Retry가 external revision을 overwrite하지 않으며 Discard consequence and Cancel semantics가 draft and disk bytes를 정확히 보존하거나 폐기한다. | machine | - |
| AC18 | Readable but unwritable local file은 read-only, unreadable file은 unavailable with Retry and Close, save-time permission loss는 original bytes and draft를 보존한 save_failed가 되며 chmod or out-of-root write가 없다. | machine | - |
| AC19 | Remote Workbench Files가 bounded text, Markdown and background-downsampled image를 `Remote · Read-only` tab으로 열고 UI and event surface에 save or write capability가 존재하지 않는다. | judged | task-owned remote SFTP flow opening text, Markdown and image and proving read-only controls and content rendering |
| AC20 | Remote loading, Cancel, auth, host-key, transport, SFTP, permission, not-found, containment, size, encoding and decode failures가 structured state로 구분되며 same-path Retry가 new operation을 보내고 late completion이 다른 tab or Device에 결합되지 않는다. | machine | - |
| AC21 | Successful remote content는 time alone으로 stale되지 않고 Device phase failure or explicit Refresh failure에서 last bytes, loaded-at and reason을 Stale로 유지하며 Retry success가 new bytes and Fresh로 복구하고 no-history failure는 Unavailable이다. | machine | - |
| AC22 | Command+W가 active local or remote file tab만 닫거나 terminal focused Pane만 닫고 Settings, sheet, modal, picker and empty context 뒤의 content를 변경하지 않는다. | machine | - |
| AC23 | Last terminal Pane close가 Herdr tab deletion 뒤 checkout empty state, `Agent를 추가하세요`와 capability-correct CTA를 표시하고 app and checkout을 유지한다. | judged | isolated Herdr last-Pane close flow with topology snapshot and native empty-state screenshot and CTA interaction |
| AC24 | Valid sibling anchor restore는 saved direction, ratio and cwd에 new shell Pane 하나를 만들고 changed layout은 silent guess 없이 current-Pane fallback or Cancel을 제시한다. | judged | isolated exact and changed-layout restore flows with before-after topology and recovery choice screenshots |
| AC25 | Pane restore stack이 per-tab LIFO, local and remote, maximum 20, app-session-only이며 Cancel and failure는 record를 유지하고 successful created event만 pop하며 repeated in-flight command가 duplicate Pane을 만들지 않는다. | machine | - |
| AC26 | Restore가 original process, agent command, conversation or scrollback을 자동 재실행하지 않고 confirmed last-Pane close 외 missing tab에서 new tab을 추측 생성하지 않는다. | machine | - |
| AC27 | Local and remote terminal에서 Command+Left and Right가 line boundary, Option+Left and Right가 word movement로 동작하고 native editor와 human-first Korean IME composition을 깨뜨리거나 duplicate bytes를 보내지 않는다. | judged | human-first native terminal and editor trail: user types Korean before any synthetic input, judges composition and caret, then verifier records local and remote arrow outcomes |
| AC28 | New visible surfaces가 DESIGN.md and HideTheme tokens, four-step surfaces, hairlines, no shadows, Inter ss03, radius and spacing scale와 restrained accents를 일관되게 사용한다. | judged | signed native app screenshot set at representative normal, loading, empty, error and recovery states with design-system comparison |
| AC29 | Every interactive and icon-only control has correct accessibility label, tooltip or value, keyboard navigation, visible focus and Escape behavior, and state meaning is not color-only. | judged | native keyboard and accessibility inspection across Settings, Workbench, Usage, Device, tabs, editor banners and restore choices |
| AC30 | Runtime mutex is not held across filesystem, git, SSH/SFTP, subprocess, image decode or large serialization and no new polling, per-tick process or duplicate refresh source is introduced. A deliberately stalled remote read cannot prevent a unique terminal sentinel from reaching and echoing in its Pane or prevent Cancel from reaching `cancelled` before the stalled transport gate is released. | machine | - |
| AC31 | Installed and running Herdr protocol mismatch, persistence failure, remote stage failure, save failure and Pane restore failure are externally observable with structured target, operation and recovery detail instead of silent fallback. | machine | - |
| AC32 | The app uses only the current macOS account's local permissions, configured SSH alias, ssh-agent and known_hosts and does not store credentials, prompt for passwords, auto-accept host keys, modify remote setup or fall back to another Device. | machine | - |
| AC33 | User-visible menu and help documentation describes effective shortcut customization, Remote Read-only support, file limits, Stale and Unavailable meaning, Pane reconstruction semantics and deferred Remote editing and Command+Up or Down boundaries. | machine | - |

## 8. PRD-Level Tasks

- T1. Build the unified command registry, physical-key preferences, migration, conflict policy, context router, dynamic menu and focus behavior, and remove obsolete shortcut execution paths.
  Covers R1, AC1-AC6.
  Depends on: none.

- T2. Build the core-owned content navigation and persistence model that composes Herdr terminal projections with Device and checkout-owned file tab descriptors and deterministic selection fallback.
  Covers R4, AC11-AC13.
  Depends on: none.

- T3. Consolidate the root-scoped local document service, multi-document state machine, editor baseline, bounded preview, canonical resolver, serialized autosave and close or quit recovery.
  Covers R5 and relevant R10 boundaries, AC14-AC18, AC22, AC30-AC31.
  Depends on: T2.

- T4. Connect the existing Rust SSH and SFTP stack as a bounded core-owned Remote Read-only document capability with cancellation, structured failure, freshness and capability isolation.
  Covers R6 and relevant R10 boundaries, AC19-AC21, AC30-AC32.
  Depends on: T2, T3.

- T5. Deliver the Files and Recent Agents Workbench, provider-by-provider Weekly Usage, upward Device popover and actual Device state projection using the shared content and persistence model.
  Covers R2, R3 and part of R9, AC7-AC10, AC28-AC29.
  Depends on: T1, T2.

- T6. Deliver context-safe active-content close, last-Pane empty checkout and bounded local and remote Pane reconstruction through the official Herdr boundary.
  Covers R7 and relevant R10 boundaries, AC22-AC26, AC30-AC31.
  Depends on: T1, T2.

- T7. Deliver local and remote terminal line-boundary key behavior without changing Command+Up or Down and without breaking native editor or IME input.
  Covers R8, AC27.
  Depends on: T1.

- T8. Integrate the final native visual and accessibility system across all new normal, loading, empty, stale, unavailable, conflict, destructive and recovery states.
  Covers R9, AC28-AC29.
  Depends on: T3, T4, T5, T6, T7.

- T9. Add high-return deterministic fixtures and regression coverage for shortcut migration and routing, tab selection, autosave races, permissions, remote operations, Usage freshness, Pane recovery and performance invariants.
  Covers R1-R10 and AC1-AC32.
  Depends on: T1, T2, T3, T4, T5, T6, T7, T8.

- T10. Update user-facing menu and help documentation, run the complete required verification contract, resolve all findings and produce a receipt-ready local implementation.
  Covers R1-R10, AC28-AC33.
  Depends on: T9.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust and Swift build health, pinned Herdr contract, dependency and forbidden-path checks, docs consistency | none |
| automated behavior | yes | command, persistence, content, document, usage, remote and Pane state-machine regressions | none |
| native runtime | yes | real signed macOS interaction, first responder, layout, visual, accessibility and recovery flows | final visual judgment |
| remote integration | yes | task-owned configured SSH and SFTP read-only flows and failure recovery | none, existing non-production fixture only |
| performance | yes | runtime mutex, main-thread responsiveness, idle refresh and process behavior | none |
| human-first native | yes | Korean IME composition and key behavior where the user must enter the first input | user must be the first input source |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R10, AC1-AC33 | Rust and Swift tests and builds, signed app assembly, codesign, pinned Herdr 0.8.2 protocol 21 contract and user-facing docs remain coherent with no new dependency or forbidden evidence path | yes | no |
| V2 | automated behavior | R1, AC1-AC6 | Legacy migration, physical chord canonicalization, Unset and Reset, context overlap, reserved conflict, atomic transfer, release-commit switcher, first-responder routing and duplicate-dispatch regressions are deterministically protected | yes | no |
| V3 | automated behavior | R4-R5, AC11-AC18, AC22 | Typed tab selection, persistence, canonical path, 2 MiB and image bounds, A-to-B debounce race, repeated save, conflict, discard, permission and close or quit state transitions preserve observable bytes and owner context | yes | no |
| V4 | automated behavior | R2-R3, AC7-AC13 | Device-specific MRU and selected content, terminal order, exact 10,080-minute Claude cache and Codex rate-limit source selection, Usage fresh or delayed or stale or unavailable or recovery mapping and popover state converge without new polling, provider call or cross-Device fallback | yes | no |
| V5 | automated behavior | R7-R8, AC22-AC27 | Close routing, last-Pane cause, stack bound, exact and changed topology, duplicate restore, missing tab, no agent relaunch and terminal key encoding are protected as caller-observable state | yes | no |
| V6 | automated behavior | R6, AC19-AC21, AC30-AC32 | Bounded read, read-only capability, containment, structured failure, cancellation, stale retention, retry recovery and late-completion isolation are deterministic and no write path is reachable | yes | no |
| V7 | native runtime | SC1, SC2, SC6, R1-R3, R9, AC5-AC10, AC28-AC29 | One signed app instance proves shortcut capture and execution, Workbench and MRU, upward Device picker, Device restoration, two-provider Usage states, keyboard navigation, accessibility and representative visual hierarchy | yes | no |
| V8 | native runtime | SC3, R4-R5, R9, AC11-AC18, AC22, AC28-AC29 | Local multi-file tabs prove dedupe, restart, readable unknown syntax, editor and previews, save and conflict recovery, permission states, Command+W and quit blocking with disk bytes matching the visible outcome | yes | no |
| V9 | remote integration | SC3, R6, R10, AC19-AC21, AC30-AC32 | A task-owned remote fixture proves SFTP text, Markdown and image read-only loading, Cancel, first-load failure, stale retained content, Retry recovery, unsupported capability and zero local misrouting without mutating remote files | yes | no |
| V10 | native runtime | SC4, R7, R9, AC22-AC26, AC28-AC29 | Task-owned Herdr fixtures prove exact restore, changed-layout choice, Cancel retention, double-key suppression, last-Pane empty checkout, capability-correct CTA and no process or agent resurrection | yes | no |
| V11 | human-first native | SC5, R8, AC27, AC29 | The user types Korean first and judges composition, candidates, caret and deletion before any automation, then local and remote Command and Option arrow behavior is recorded without duplicate bytes or editor regression | yes | yes |
| V12 | performance | R2-R7, R10, AC9, AC16, AC20-AC21, AC25, AC30-AC31 | With the owned remote transport held after loading begins, a unique terminal sentinel reaches and echoes in its Pane and Cancel reaches `cancelled` before the transport gate is released. Samples of the single signed app and herdr server during the same largest fixture show no I/O under the runtime mutex and no new idle polling or per-tick process | yes | no |
| V13 | native runtime | SC1-SC6, R9, AC28-AC29 | Final normal, loading, empty, stale, unavailable, conflict, destructive and recovery screenshots form one coherent DESIGN.md and HideTheme composition without ad hoc chrome, clipped controls or color-only meaning | yes | no |
| V14 | automated behavior | R1, R4, R10, AC3, AC13, AC31 | Deterministic unsupported-protocol and atomic UI-state write fault injection produces action-required state with structured target, operation, stage, retryability and recovery detail, preserves the last valid projection or persisted bytes and never silently resets unrelated state | yes | no |
| V15 | build/static | R1-R8, AC33 | A deterministic documentation-content check finds effective shortcut customization, Remote Read-only support, text and image limits, Fresh or Updating or Stale or Unavailable meaning, Pane reconstruction semantics and deferred Remote editing and Command+Up or Down boundaries in the user-visible menu or help surface | yes | no |

### 9.3 Human Verification

- H1. The user must be the first person to type into the local and remote terminal and local file editor during the Korean IME run.
  No automatic typing, key press, input-source switch or synthetic input may occur before that judgment.
- H2. A human reviewer must judge the final signed native screenshots for visual hierarchy, compactness, readability, state distinction and consistency with DESIGN.md after automated accessibility and token checks pass.
- H3. The reviewer must confirm that destructive Discard copy states the exact consequence before the action and that Pane restore copy never implies process, agent conversation or scrollback resurrection.

## 10. Risks And Open Decisions

No blocking product decision remains.

Deferred decisions:

- Remote edit and save remain deferred until a separately approved atomic remote write, revision conflict, credential capability and recovery contract exists.
- Command+Up and Command+Down remain deferred until the user selects an exact terminal meaning.
- Crash recovery of unsaved draft bytes remains deferred.
  Normal quit protection is required now.
- Remote Command+P, remote terminal file links and file-tab drag reorder remain deferred.

Risks:

- RF1. Current debounce can write one document's draft to another path.
  Mitigation: T3 must land document identity, serialized save, revision guard and late-completion isolation before multiple editable tabs are considered usable.
- RF2. Existing active file code and a typed FileService overlap.
  Mitigation: consolidate one root-scoped service and remove obsolete active execution paths instead of extending both.
- RF3. Filesystem, git, SFTP and image work can recreate the prior runtime-mutex latency incident.
  Mitigation: precompute outside the lock, keep only short state commits inside it, structurally test the boundary and require V12 samples.
- RF4. Existing SFTP implementation does not cover every OpenSSH alias feature.
  Mitigation: capability-gate each configured Device, keep terminal projection intact, expose action-required Files failure and forbid silent shell fallback.
- RF5. Large or malicious images can consume memory during decode.
  Mitigation: stat and bounded read before transfer, 20 MiB and 40 MP limits, background ImageIO downsample and cancellation.
- RF6. Shortcut migration can reset unrelated UI state or strand commands.
  Mitigation: retain outer schema, version the nested object, migrate only known legacy bindings, preserve unaffected entries and atomically persist before removing old runtime paths.
- RF7. Pane topology may change after close.
  Mitigation: validate anchor and tab identity, never apply a full layout, offer explicit fallback, retain records on failure and pop only after authoritative creation event.
- RF8. Two provider values, Device state and controls can overcrowd the bottom bar.
  Mitigation: use compact marks and short values, preserve detailed popovers, test smallest supported layout and make overflow explicit rather than clipping.
- RF9. Fresh, Updating, Stale and Unavailable can be confused.
  Mitigation: use distinct icon, label, timestamp and reason with color only as secondary encoding and provider-isolated fixtures.
- RF10. The approved surface is broad and cross-cuts persistence, I/O, routing and native UI.
  Mitigation: implement tasks in dependency order as complete vertical layers, close each with focused checks and do not declare Done before the unified receipt.

## 11. Implementation Guardrails

- engineering/principles.md rule 1: remove obsolete hardcoded menu bindings, independent shortcut monitors, Pane-only runtime shortcut ownership and duplicate active file paths in the same change.
  The user-approved one-time legacy shortcut decoder is the only narrow exception to no compatibility layers and must not preserve an old parallel execution path.
- engineering/principles.md rule 2: choose the smallest structure that satisfies this complete PRD and do not add a new editor, shortcut, SSH, polling or persistence framework.
- engineering/principles.md rule 3: land command authority, content identity, document safety, remote read, UI composition and verification in dependency order as complete end-to-end layers.
  This orders the work and does not permit an intentionally incomplete user journey.
- engineering/principles.md rule 4: never convert invalid shortcut, missing tab, permission, size, conflict, persistence, protocol, remote or restore failure into a default, empty value or silent skip.
- engineering/principles.md rule 5: keep registry definition, event routing, persistence migration, content navigation, document I/O, remote transport, Pane reconstruction and presentation concerns modular with one owner each.
- engineering/principles.md rule 6: the repository and pinned upstream contracts were inspected first.
  No replacement library may be added without a demonstrated requirement the existing AppKit, Highlightr, MarkdownUI, ImageIO, russh and SFTP stack cannot meet and fresh user approval.
- engineering/principles.md rule 7: reuse existing HideTheme, Agent marks, UI-state atomic persistence, snapshot refresh, FileService types, SSH configuration and Herdr CLI or Socket boundaries before creating parallel implementations.
- engineering/principles.md rule 8: the unified content and document model must be the lasting authority, not an overlay or adapter already planned for removal.
- engineering/principles.md rule 9: log structured operation ID, Device, checkout, tab or document target, stage, attempt, duration, retryability and failure category needed to diagnose save, remote read, persistence and restore incidents without logging file contents or credentials.
- engineering/principles.md rule 10: every failure must appear in snapshot state, native UI or an explicit diagnostic channel and must not leave the user guessing whether data was saved, loaded, closed or restored.
- engineering/principles.md rule 11: assume every shortcut, save, open, cancel, Device switch, close, restore and async completion occurs twice and make the outcome idempotent.
- engineering/principles.md rule 12 and engineering/practices/test.md: test caller-observable bytes, state and topology where regression value is high and avoid brittle layout snapshots, framework-default tests and tests that duplicate native behavior without protecting a product risk.
- engineering/principles.md rule 13: fix the class of cross-document save, context routing, stale completion, containment and silent-fallback failure across every entry point rather than patching the screenshot case.
- design/principles.md rule 1: Files, Recent Agents and Device rows are read views whose visible columns and empty states come from available data, never fabricated placeholders.
- design/principles.md rule 2: Settings, Workbench, Device, file and recovery surfaces follow the operator's configure, navigate, inspect, edit, close and recover flow rather than exposing persistence or transport schemas.
- design/principles.md rule 3: frequent Files or Recent Agents switching, Device selection, tab activation, clean close and Retry each require the fewest safe interactions.
- design/principles.md rule 4: show effective shortcut, actual Device phase, provider freshness, current file ownership and save or restore state directly so the operator never computes them from raw metadata.
- design/principles.md rule 5: extend DESIGN.md, HideTheme, existing compact keycap, popover, segmented control, tab and status patterns and add a token before using a one-off value.
- design/principles.md rule 6: state exact disk or topology consequence before Discard, last-Pane close fallback or current-Pane restore and never hide a customer-visible destructive result behind an icon.
- design/principles.md rule 7: encode Device, provider, read-only, stale, conflict, unavailable, active content and restore structure with hierarchy, icon, badge, label and focus state, using sentences only for consequence and recovery detail.
- Project DESIGN.md overrides generic UI preferences.
  Use its single dark mode, four-step surface ladder, 1px hairlines, no shadows, Inter ss03, radius 6 through 16, spacing and restrained accent rules through HideTheme.
- Herdr core remains the only authority for Pane layout, focus, zoom, persisted runtime state and synchronization.
  Swift shell must dispatch typed events and render snapshots rather than invent authoritative topology or file state.
- Read the current official Herdr CLI and Socket API references and run the pinned contract check before relying on any method.
  Use targeted pane.split and tab.create only as documented and do not invent pane.restore.
- Never hold the runtime mutex across subprocess, filesystem, SSH/SFTP, network-like I/O, image decode or large serialization.
  Never add per-tick git, SSH or other subprocess work.
- Snapshot additions must use the channel appropriate to change rate.
  Rare metadata belongs in revisioned state, active editor content in its existing dedicated channel and bounded remote image payloads outside wholesale rest snapshots.
- All local and remote file entry points must resolve a structured Device, checkout and root-relative identity through one containment boundary.
  Never accept arbitrary absolute paths or follow a symlink outside the checkout.
- Remote UI must not expose or dispatch any write operation.
  Do not ask for passwords, accept host keys, store secrets, change SSH config, run sudo or install remote software.
- Do not mutate existing user Herdr sessions, panes, workspaces or Devices during verification.
  Create and label task-owned fixtures, keep them non-focused and clean up only resources created by the run.
- Before any native visual check, confirm exactly one app instance and whether it is the freshly built signed bundle.
  Verify visible UI with a real screenshot rather than source inspection or process existence.
- Human-first IME means absolutely no automatic typing, key press, input-source switch or synthetic input before the user's first local and remote terminal and editor input.
- Evidence, screenshots, logs, traces and sample output belong under `agents/runs/<slug>/` and must never be force-added under docs or spike evidence paths.
- Before editing implementation paths, rerun `sasu rules relevant` and obey any newly applicable project-local invariant.
- Preserve unrelated worktree changes and use the harness dirty-attribution contract rather than staging, reverting or absorbing them.
- Delivery is local only after a fresh complete receipt.
  Do not push, open a PR, watch CI, merge, deploy or modify release artifacts without new user authorization.

## 12. Implementation Result Report Contract

The implementing agent must report:

- Status as `Done`, `Partially Done`, or `Blocked` and the exact reason when it is not Done.
- User-visible shortcut, Workbench, Usage, Device, file tab, editor, Remote Read-only, close, empty checkout, restore and terminal-key behavior delivered.
- Actual core, shell, persistence, document service, SFTP, Herdr operation and presentation boundaries selected during implementation and whether §5 was followed.
- Obsolete hardcoded shortcut and duplicate file paths removed, plus any approved one-time migration retained and why.
- T1 through T10 completion and AC1 through AC33 status with no parked criterion hidden as complete.
- V1 through V15 evidence by mode, including exact test and build results, signed bundle identity, one-instance native screenshots, task-owned local and remote fixture results, protocol and persistence fault injection, documentation-content checks, human-first IME trail and app plus herdr server samples.
- Acceptance judge, fidelity judge and high-risk reviewer invocation, lane-local verdicts, timing, risk ledger disposition and evidence that mechanical failure made zero judge calls when applicable.
- Evidence that finalize performed no tests, judges, capture tools or external commands and that the final source and registered artifact hashes remain fresh.
- Completion fingerprint, `receipt.json`, `implementation-result.md` and authoritative complete `state.json` paths.
- Data-safety result for cross-document autosave, external conflict, permission loss, Discard, normal quit, remote read-only and no out-of-root or remote write side effects.
- Herdr compatibility result for installed version, protocol 21, pinned schema, last-Pane deletion, targeted restore and no unsupported method use.
- Design and accessibility findings for every representative normal, loading, empty, stale, unavailable, conflict, destructive and recovery state, with remaining human taste judgment called out plainly.
- Automated tests added or updated and the concrete regression risk each protects, excluding low-value framework or implementation-detail tests.
- Deviations from this PRD, their approval evidence and any remaining risk or deferred follow-up.
- Delivery result as one semantic local commit created only after the complete receipt, including commit hash and file list.
  Report explicitly that no push, PR, CI, merge or deployment occurred.
