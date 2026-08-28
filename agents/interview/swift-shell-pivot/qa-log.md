---
topic: "herdr-ide 셸을 SwiftUI + Rust 코어 하이브리드로 전환"
status: "active"
where: "brownfield"
selected_packs: "ux, compatibility, risk, operation, verification"
created_at: "2026-08-28"
updated_at: "2026-08-28"
question_count: 6
normalization_policy: "transcript-sync-with-checkpoint-backfill"
normalization_checkpoint_every: 10
---

# Interview Log: herdr-ide 셸을 SwiftUI + Rust 코어 하이브리드로 전환

## Current Understanding

- 목표: 투박한 UI와 한글 IME 결함을 해결하기 위해 macOS 셸을 SwiftUI로 바꾸고, SSH/herdr/도메인 로직은 Rust 코어로 남긴다.
- 현 코드는 objc2+wgpu+glyphon 직접 렌더 16040줄이며 플랫폼 의존이 app.rs(69곳)와 render.rs(2곳)에만 있어 코어 분리 비용이 낮다.
- 사용자는 윈도우 지원을 하지 않기로 했고, 그 결과 터미널 엔진을 Rust에 유지할 유일한 근거가 사라져 SwiftTerm 채택이 유력해졌다.
- 런타임 결정이 11일간 4번 바뀌었고(Swift+libghostty -> Rust/Tauri -> Electron/TS -> Rust native), 이번이 5번째 전환 후보다.
- 브라우저 패널은 현재 CEF CDP 게이트웨이 전제로 구현돼 있어(src/browser.rs) SwiftUI 전환 시 재결정이 필요하다.

## Intake Cursor

- next_decision_id: D-22
- next_question: (owned by the live conversation until checkpoint)
- last_materiality_sweep: checkpoint 1
- outstanding_raw_entries: none
- next_checkpoint_at: Q16

## Transcript Sources

| Runtime | Session ID | Start ref |
| --- | --- | --- |
| claude | 7084c601-ca0a-4883-9be8-6aedc1af55c9 | 7e6ca07a-dc90-411f-9c6c-c7029fa1e17c |

## Decision Register

| ID | Kind | Area | Decision / fact | Priority | Source / owner | Status | PRD mapping / revisit |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D-01 | fact | 아키텍처 | 플랫폼 의존(objc2/AppKit)이 app.rs 69곳과 render.rs 2곳에만 있고 나머지 20개 파일(약 12700줄)은 std/serde/anyhow만 쓴다. wgpu/glyphon도 같은 두 파일에만 있다. 코어/셸 분리 비용이 낮다. | P0 | repo: src/*.rs grep (2026-08-28) | resolved | R#: 크레이트 분리 근거 |
| D-02 | fact | 아키텍처 | render.rs:22의 FrameModel(navigator_text, overlay_text, tab_text, connection_text, zoomed, focused, terminal, editor_text, ime_marked, input_generation)이 이미 ViewModel 스냅샷 구조다. FFI 경계용 스냅샷을 새로 설계할 필요가 없다. | P1 | repo: src/render.rs:22-33 | resolved | R#: 스냅샷 계약 |
| D-03 | fact | UX/design | 현 NSTextInputClient 구현에 4가지 결함이 있다. (1) firstRectForCharacterRange가 NSRect(280,92,2,22) 하드코딩이라 한글 후보창이 커서를 안 따라감 (2) insertText/setMarkedText의 replacementRange를 모두 무시 (3) selectedRange가 항상 (0,0)이고 attributedSubstringForProposedRange가 항상 None (4) 조합 중 편집 단축키 필터 없음. 또 render.rs:676이 조합 문자열 전체를 커서 셀 하나에 넣어 2글자 이상이면 뒤 셀을 덮는다. | P0 | repo: src/app.rs:794-880, src/render.rs:676-681 | resolved | R#: 전환 동기 |
| D-04 | fact | 참조제품 | 형제 제품 missuo/herdrm(SwiftUI, macOS 14+, 636 stars)은 SwiftTerm 1.19 + Sparkle 2.9.6 + 로컬 HerdrKit만 의존한다. project.yml:19 주석이 'SwiftTerm 1.19 ships the marked-text overlay used for IME preedit; do not reimplement it'라고 명시한다. SwiftTerm은 public func feed(byteArray:)(Terminal.swift:6559)와 send(source:data:) 델리게이트(MacTerminalView.swift:1436)를 제공해 Rust SSH 바이트와 2개 함수로 연결된다. | P0 | git clone missuo/herdrm + migueldeicaza/SwiftTerm (2026-08-28) | resolved | R#: 터미널 뷰 선택 근거 |
| D-05 | fact | 선행이력 | 런타임 결정이 11일간 4번 바뀌었다. herdr-lightweight-ide(2026-08-17, Swift+libghostty+WKWebView, approved 후 소스 0줄) -> D-05 Rust/Tauri(rejected) -> D-10 Electron+TS(2026-08-25 확정, herdr-core를 TS로 재구현하기로 결정) -> 브랜치 prd/herdr-ide-rust-native-800mb의 Rust native(현 코드 16040줄). PRD 디렉터리가 5개이고 전부 status ready다. 이번 SwiftUI 전환은 5번째 런타임 결정이 된다. | P0 | repo: agents/interview/*/qa-log.md, git log, agents/prd/ 5개 | resolved | R#: 전환 리스크 |
| D-06 | fact | 검증 | D-41이 메모리 예산을 v1 인수 조건으로 고정했다. workspace 7/pane 11 기준 브라우저 닫힌 상태 400MB 이하, 브라우저 하나 연 상태 900MB 이하. 초과 시 v1 미완료 판정. 근거는 사용자가 orca를 무거워서 제거한 실증이다. SwiftUI+Rust는 이 예산에 유리하므로 전환이 D-41과 충돌하지 않는다. | P0 | agents/interview/herdr-ide-native-shell/qa-log.md D-41, D-13 | resolved | R#: 인수 조건 유지 여부 |
| D-07 | fact | 브라우저 | src/browser.rs(721줄)는 CEF CDP 업스트림에 붙는 loopback CDP 게이트웨이로 구현돼 있다(에러 문구 'CEF CDP upstream unavailable', attach_chromux/detach_chromux, capability 스코프). 즉 현 설계는 앱에 CEF(Chromium)를 임베드하는 전제다. 선행 인터뷰 D-11은 브라우저 패널+grab을 v1 최대 기능으로, D-19는 chromux가 붙을 CDP 엔드포인트 노출을, D-53은 chromux를 전제조건으로 확정했다. 한편 D-12는 Rust에서 CEF 임베딩에 실용 경로가 없다고 기록했다. SwiftUI 전환 시 이 축이 전부 재결정 대상이다. | P0 | repo: src/browser.rs:43,92,109,144 + herdr-ide-native-shell D-11/D-12/D-19/D-53 | open | 미정 - Q1 |
| D-08 | decision | 스코프 | grab(브라우저 요소를 집어 HTML/CSS/크롭 스크린샷을 에이전트 pane에 주입)은 v1 비목표다. 선행 인터뷰 D-11이 v1 최대 기능으로 두었으나 사용자가 현재 필요 없다고 판정했다. 이 결정이 인앱 브라우저의 마지막 존재 이유를 제거한다. | P0 | user Q2 | resolved | 비목표(v1) / 선행 D-11의 grab 성분 무효 |
| D-09 | fact | 브라우저 | herdr official.browser 플러그인(ogulcancelik/herdr-browser v0.1.0, MIT, 사용자가 설치한 서드파티)은 --headless=new Chrome을 띄우고 매 프레임을 PNG로 캡처해 kitty graphics로 터미널에 그린다. 주소창(handleUrlInputKey)과 탭/뒤로/앞으로/새로고침/정지/줌 버튼은 이미 구현돼 있다. 즉 부족한 것은 UI가 아니라 전송 방식이며, 터미널 pane이 글자와 그림만 받으므로 fork로는 화질/반응 문제를 해결할 수 없다. | P1 | repo: ~/.config/herdr/plugins/github/official.browser-ff2a44eccae9/src/{chrome.ts:38,viewer.ts:292,302,839,1482-1489} | resolved | R#: 플러그인 fork 기각 근거 |
| D-10 | fact | 브라우저 | Swift에서 CEF 임베드에 실용 경로가 없다. 유일한 Swift 바인딩 lvsti/CEF.swift는 2021-11-01 이후 방치되고 릴리스가 0개다. CEF 자체는 SetAsChild(parent_nsview, rect)로 기존 NSView에 붙는 API가 있으나, CEF 포럼에 macOS 임베딩 시 빈 화면 렌더링, 부모 NSView 리사이즈 미추종, key window 하나 제약에 의한 포커스 문제가 반복 보고된다. Swift에서 쓰려면 C++/ObjC++/Swift 브리지를 자체 유지해야 한다. 선행 D-12가 Rust에 내린 판정과 동일하다. 조사 방식은 문서와 이슈 검토이며 실제 임베딩을 시도하지 않았다. | P0 | github.com/lvsti/CEF.swift API(마지막 push 2021-11-01, 릴리스 0, star 104), magpcss.org CEF 포럼 macOS 임베딩 스레드 다수 (2026-08-28 조사) | resolved | R#: 옵션 C 기각 근거 |
| D-11 | decision | 아키텍처 | A-1 확정. herdr-ide는 브라우저를 임베드하지 않는다. src/browser.rs 721줄(CEF CDP 게이트웨이, attach_chromux/detach_chromux, capability 토큰)을 삭제한다. 선행 인터뷰의 D-11(브라우저 패널+grab), D-19(CDP 엔드포인트 노출), D-53(chromux 전제조건)을 무효화한다. D-16의 화면 구조에서 우측 패널의 브라우저 전환이 빠지고 워크벤치만 남는다. 수용한 비용은 브라우저가 herdr-ide 창 밖의 별도 Chrome 창이라는 것이다. | P0 | user Q5 | resolved | R#: 브라우저 경계 / 선행 D-11/D-19/D-53 무효화 |
| D-12 | fact | 브라우저 | chromux는 headed(창 있는) 진짜 Chrome을 띄우고 loopback CDP를 외부에 노출한다. 실측: launchMode headed, headless false, port 9300, Chrome/151.0.7922.175, Protocol-Version 1.3, webSocketDebuggerUrl 제공, /json/list로 탭 목록 조회 가능. 포트는 chromux ps --json이 알려준다. 빈 Chrome 1개의 실측 자원은 프로세스 6개 / 렌더러 2개 / RSS 676MB다. 또한 chromux는 존재하지 않는 프로필을 스스로 만들지 않고 사용자 승인을 요구한다. | P0 | 로컬 실측 2026-08-28: chromux launch default + curl 127.0.0.1:9300/json/version, /json/list, chromux ps --json | resolved | R#: A-1 성립 근거 |
| D-13 | decision | 브라우저 | herdr-ide는 브라우저 행동을 chromux CLI로 위임하고(chromux launch / open / ps --json, std::process::Command), 상태 표시는 GET http://127.0.0.1:<port>/json/list 읽기 전용 HTTP로만 한다. herdr-ide가 CDP로 직접 조작하는 것은 금지한다. 근거는 chromux가 그 Chrome의 데몬/세션/직렬화(serialQueue)/pause 잠금을 소유하고 있어, herdr-ide가 같은 포트로 명령하면 주인이 둘이 되어 에이전트 조작 도중 충돌이 나기 때문이다. Rust CDP 크레이트(chromiumoxide 0.9.1, headless_chrome 1.0.22)는 읽기 전용 GET 하나에 과하므로 채택하지 않는다. herdr-ide도 chromux의 프로필 생성 규칙을 따라 없는 프로필을 임의 생성하지 않는다. 예상 코드량은 100줄 안쪽이다. | P1 | user Q6에서 수용한 권고 | resolved | R#: 브라우저 연동 방식 |
| D-14 | decision | 검증 | 메모리 인수 조건을 herdr-ide 프로세스만 400MB 이하로 재정의한다. 선행 D-41(브라우저 닫힘 400MB / 브라우저 열림 900MB)을 대체한다. 근거는 D-11로 브라우저가 앱 밖 별도 Chrome이 되어 우리가 통제할 수 없는 자원이 되었기 때문이며, 통제 못 하는 값으로 합격/불합격을 가르면 그 게이트는 곧 무시된다. 측정 시나리오는 D-41과 같이 workspace 7 / pane 11 실사용 규모를 유지하고 ps 기반 자동 판정도 유지한다. | P0 | user Q6 | resolved | V#: 메모리 인수 조건 / 선행 D-41 대체 |
| D-15 | decision | 아키텍처 | SwiftUI + Rust 코어 하이브리드 전환 자체의 최종 확정. 사용자가 'swift 괜찮은 것 같다'는 방향을 표명하고 계획 수립을 요청했으나 명시적 확정 응답은 아직 없다. 폐쇄 전 확인 필요. | P0 | 미확정 - 사용자 방향 표명만 있음 | open | 미정 |
| D-16 | decision | 터미널 | 터미널 뷰로 SwiftTerm(1.19 이상)을 채택한다. 엔진과 뷰를 모두 SwiftTerm에 맡긴다. Rust 코어는 SSH/PTY 바이트만 담당하고 feed(byteArray:)로 넣고 send(source:data:) 델리게이트로 받는다. 결과로 alacritty_terminal 의존과 src/terminal.rs 755줄, src/render.rs 921줄이 폐기된다. 윈도우 미지원 확정으로 터미널 엔진을 Rust에 유지할 유일한 근거가 사라진 것이 판단 근거다. | P0 | user Q8 | resolved | R#: 터미널 뷰 |
| D-17 | decision | 검증 | SwiftUI 앱의 에이전트 검증은 peekaboo(4.2.2 설치 확인)로 한다. peekaboo see --app으로 접근성 기반 UI 요소 맵을 받고 click/type/press로 조작한다. 선행 D-34의 'chromux가 herdr-ide CDP에 붙어 조작'을 대체한다. 결과 확인에 herdr 소켓 API를 쓰는 부분은 그대로 유지한다. SwiftUI가 접근성 트리를 기본 제공하므로 src/accessibility.rs 165줄은 폐기된다. 수용한 비용: Screen Recording과 Accessibility 권한이 필요해 D-29 공개 배포 대상자와 CI에서 마찰이 있다. 에이전트가 상태 스냅샷을 직접 읽는 별도 표면은 채택하지 않았다. | P0 | user Q7 | resolved | V#: 앱 검증 경로 / 선행 D-34 대체 |
| D-18 | decision | 리스크 | 5번째 런타임 전환을 성립시키는 조건. 이전 4번 중 herdr-lightweight-ide는 approved PRD가 소스 0줄로 끝났고, Electron 확정(선행 D-10) 역시 코드로 이어지지 않은 채 Rust 네이티브로 뒤집혔다. 이번 전환의 중단 조건, 되돌리기 기준, 기존 Rust 네이티브 16040줄의 처리 방침을 정해야 한다. | P0 | D-05의 전환 이력 | open | 미정 |
| D-19 | decision | 검증 | 한글 조합 입력을 사람 판정 완료 게이트로 둘지는 0단계 스파이크 결과를 보고 정한다. 판정 방법은 한글 IME를 켜고 peekaboo type 'gks'를 보내 '한'이 나오는지 보는 것이다. 나오면 peekaboo가 IME 경로를 거치는 것이라 자동 게이트로, 안 나오면 IME를 우회하는 것이라 사람 게이트로 간다. owner: 사용자. revisit trigger: 0단계 스파이크 완료 시점. 사람 게이트가 될 경우 확인 항목은 후보창이 커서를 따라오는가, 조합 중 백스페이스가 조합을 지우는가, 두 글자 이상 조합이 뒤 글자를 덮지 않는가, 터미널과 에디터 양쪽에서 되는가 네 가지다. | P0 | user Q8 | deferred | V#: IME 게이트 / revisit: 0단계 스파이크 |
| D-20 | decision | 아키텍처 | 기존 Rust 네이티브 셸 코드를 삭제해도 된다. 폐기 대상은 app.rs 2412줄 중 AppKit/wgpu 부분, render.rs 921줄, terminal.rs 755줄, accessibility.rs 165줄, browser.rs 721줄과 wgpu/glyphon/bytemuck/raw-window-handle/objc2/alacritty_terminal 의존이다. 유지 대상은 remote.rs 3550줄, herdr.rs 1154줄, files/commands/domain/navigator/layout/presentation/browser 축을 제외한 로직 계층이다. 삭제 시점과 되돌리기 지점은 D-18에서 정한다. | P0 | user Q8 | resolved | R#: 폐기 범위 |
| D-21 | fact | 운영 | Xcode.app은 설치되어 있지 않고 Command Line Tools만 있다(/Library/Developer/CommandLineTools). xcodebuild와 xcodegen은 쓸 수 없다. 그러나 SwiftUI.framework와 AppKit.framework가 CLT SDK에 포함되어 있고, swiftc -parse-as-library로 SwiftUI 앱을 실제로 컴파일해 arm64 Mach-O 실행파일을 얻는 데 성공했다(Swift 6.0.3). 따라서 선행 D-04가 기록한 PW1(Xcode 전체 설치) 차단요인 - 1차 전환 herdr-lightweight-ide를 소스 0줄로 죽인 원인 - 은 이번 전환에서 해소된다. 다만 herdrm이 쓰는 XcodeGen+xcodebuild 방식은 쓸 수 없고, .app 번들 조립과 코드사인/공증 경로는 아직 확인하지 않았다. | P0 | 로컬 실측 2026-08-28: xcode-select -p, xcodebuild -version, swiftc 빌드 성공 | resolved | R#: 1차 전환 차단요인 해소 |

## Raw Q&A

### Q1: 인터뷰 로그를 만들고 사전 조사 사실 7개를 등록했습니다: `agents/interview/swift-shell-pivot/qa-
- decision_ids: none
- route: mixed
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:471a91db-0de1-4288-bdb7-42db411c15df
- asked: 인터뷰 로그를 만들고 사전 조사 사실 7개를 등록했습니다: `agents/interview/swift-shell-pivot/qa-log.md`
  
  **지금까지 이해한 것**
  - 목표: 투박한 UI와 한글 IME 결함을 없애기 위해 macOS 셸을 SwiftUI로 바꾸고, SSH/herdr/도메인 로직은 Rust로 남긴다.
  - 분리 비용은 낮다. 플랫폼 의존이 `app.rs`(69곳)와 `render.rs`(2곳)뿐이고 나머지 약 12700줄은 이미 이식 가능.
  - 윈도우를 안 하기로 해서 터미널 엔진을 Rust에 둘 근거가 사라졌고, SwiftTerm 채택이 유력해졌다.
  - 다만 **런타임 결정이 11일간 4번 바뀌었다.** 이번이 5번째다. 이건 뒤에서 따로 여쭙겠습니다.
  
  조사하면서 **계획을 무너뜨릴 수 있는 것 하나**를 찾았습니다. 이것부터 여쭙겠습니다.
  
  ---
  
  ### Q1. 브라우저 패널을 어떻게 할까요?
  
  **왜 묻냐면:** `src/browser.rs`(721줄)가 **CEF(Chromium)를 앱에 넣는 전제**로 짜여 있습니다. 에러 문구가 `"CEF CDP upstream unavailable"`이고, `attach_chromux` / `detach_chromux` 함수로 chromux가 붙을 loopback CDP 게이트웨이를 노출합니다.
  
  선행 인터뷰에서 이걸 v1 최대 기능으로 못박아 두셨어요 (D-11 브라우저 패널 + grab, D-19 CDP 노출, D-53 chromux 전제).
  
  **문제는 SwiftUI로 가면 이게 안 따라옵니다.** SwiftUI에서 자연스러운 건 WKWebView인데, WKWebView는 Chromium이 아니라 Safari 엔진이라 **CDP 엔드포인트가 없습니다.** chromux가 붙을 곳이 사라집니다.
  
  선택지는 셋입니다.
  
  **A. 브라우저를 herdr-ide 밖으로 뺀다**
  herdr의 `official.browser` 플러그인 pane + chromux를 그대로 씁니다. 지금 세션 훅이 이미 그 경로를 표준으로 쓰고 있고요. herdr-ide는 터미널/파일/에이전트만 맡습니다.
  - 얻는 것: 이미 동작하는 것 재사용, 메모리 예산(D-41) 여유, 스코프 대폭 축소
  - 잃는 것: grab(요소 집어서 에이전트에 넘기기)이 herdr-ide 밖으로 나가거나 사라짐
  
  **B. WKWebView를 넣되 CDP는 포기한다** ← 제 추천
  브라우저 패널은 앱 안에 있고, grab도 됩니다. WKWebView가 `evaluateJavaScript`(요소 선택, HTML/CSS 추출)와 `takeSnapshot`(크롭 스크린샷)을 다 지원하거든요. **D-11의 grab은 CDP 없이도 구현됩니다.**
  - 얻는 것: grab 유지, 별도 프로세스라 메모리 가볍고 D-41에 유리, CEF 임베딩 리스크(D-12) 회피
  - 잃는 것: **chromux가 이 패널에 못 붙습니다.** 에이전트가 브라우저를 직접 몰려면 herdr browser pane을 따로 띄워야 합니다. `browser.rs` 721줄 폐기.
  
  **C. CEF를 Swift에서 임베드한다**
  CDP까지 다 지킵니다. 대신 D-12가 "Rust에 실용 경로 없음"이라고 기록한 그 리스크가 Swift로 옮겨갈 뿐이고(Swift에서 CEF는 C++ 브리지가 필요합니다), Chromium 하나가 앱에 들어와 D-41의 900MB 예산과 정면으로 부딪힙니다.
  
  ---
  
  **제가 B를 추천하는 이유:** grab은 "내가 화면에서 요소를 집어 에이전트한테 준다"는 사람의 행위고, 이건 WKWebView로 됩니다. chromux는 "에이전트가 브라우저를 자동으로 몬다"는 별개의 일이고, 이건 herdr browser pane으로 이미 되고 있습니다. **두 개를 한 엔진에 밀어넣으려던 게 CEF 요구사항의 정체**로 보입니다.
  
  다만 이건 제품 판단이라 확인이 필요합니다. **A / B / C 중 어느 쪽인가요?** 그리고 B라면, chromux가 herdr-ide 패널에 못 붙는 걸 받아들일 수 있는지도 같이 알려주세요.
- recommended: none
- answer: 1. grab은 우선 필요없어서 패스 
  
  근데 결국 agent가 브라우저를 통신해서 조작하게 하려면 chromux 쓰게하는게 제일 나을것같은데 대안이 저거밖에 없다는거지? 그래야 브라우저 QA 확실하게 하지!
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q2: 네, 사실상 그것밖에 없습니다. 이유를 짧게 설명드릴게요.
- decision_ids: none
- route: mixed
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:380c243e-1355-4f27-947c-75788a796701
- asked: 네, 사실상 그것밖에 없습니다. 이유를 짧게 설명드릴게요.
  
  **에이전트가 브라우저를 조작하려면 "원격 조종 프로토콜"이 필요한데, 현실에 쓸 만한 건 두 개뿐입니다.**
  
  - **CDP** (Chrome DevTools Protocol) - Chromium 계열만. chromux, Playwright, Puppeteer가 전부 이걸 씁니다.
  - **WebDriver** - Selenium 계열. Safari는 `safaridriver`로 지원하는데, **이건 Safari.app을 조종하는 거지 앱에 박아넣은 WKWebView를 조종하는 게 아닙니다.**
  
  WKWebView는 둘 다 없습니다. 밖에서 붙을 구멍 자체가 없어요. `evaluateJavaScript`로 우리 앱 코드가 스크립트를 실행시킬 수는 있지만, 그건 **우리가 미리 짜둔 것만** 되는 거지 에이전트가 자유롭게 모는 것과는 다릅니다. QA용으로는 못 씁니다.
  
  **그래서 결론: 에이전트가 모는 브라우저는 진짜 Chromium이어야 하고, 그건 우리 SwiftUI 앱 밖에 있어야 합니다.** chromux가 최선인 게 아니라, 사실상 유일한 실용 경로입니다.
  
  ---
  
  ### 그런데 이게 답을 바꿉니다
  
  제가 아까 B(WKWebView 넣기)를 추천했던 **유일한 이유가 grab이었습니다.** grab을 뺀다고 하셨으니 B의 근거가 통째로 사라졌어요.
  
  grab 없는 WKWebView 패널은 이렇게 됩니다:
  - 에이전트는 못 건드림 (CDP 없음)
  - 사람이 페이지를 눈으로 볼 수만 있음
  - 그런데 브라우저 QA는 어차피 herdr browser pane + chromux로 함
  
  **즉 "보기만 하는 브라우저"가 앱 안에 하나 더 생기는 겁니다.** 값은 별로 없고 유지비만 붙습니다.
  
  그리고 좋은 소식은, **에이전트 브라우저 QA 경로는 이미 완성되어 있다는 거예요.** 지금 이 세션의 훅에 표준 절차로 박혀 있습니다:
  
  ```
  herdr plugin pane open --plugin official.browser   ← 진짜 Chromium pane
    → cli.ts connect --view <id>  → cdp_http_url 획득
    → chromux open herdr-view --cdp-url <url>
    → chromux snapshot / click / screenshot
  ```
  
  herdr-ide가 이걸 다시 만들 이유가 없습니다. **A로 가면 `browser.rs` 721줄이 그냥 지워지고, 브라우저 QA 능력은 하나도 안 잃습니다.**
  
  ---
  
  ### Q2. 그러면 "사람이 보는 용도"의 브라우저 패널은 필요하신가요?
  
  이게 마지막 갈림길입니다. 두 가지가 다른 얘기예요.
  
  **A-1. 브라우저 패널을 완전히 없앤다** ← 제 추천
  herdr-ide는 터미널 / 파일 / 에이전트만. 웹은 herdr browser pane이 맡습니다.
  - `browser.rs` 721줄 삭제, 선행 D-11/D-19/D-53 전부 무효화
  - 화면 구조도 단순해집니다 (D-16의 "우측 패널이 워크벤치↔브라우저 전환" → 워크벤치만 남음)
  - 잃는 것: 터미널 옆에서 페이지를 **바로** 보는 편의. 대신 herdr browser pane을 옆에 띄우면 되긴 합니다.
  
  **A-2. WKWebView를 보기 전용 뷰어로만 남긴다**
  로컬 dev 서버 결과를 옆에서 훑어보는 용도. 에이전트 연동은 처음부터 비목표로 명시.
  - 얻는 것: 터미널 옆 미리보기
  - 비용: SwiftUI 패널 하나 + 탭/주소창/프로필 관리가 따라붙음. 그리고 **"이거 왜 chromux 안 붙어?"라는 혼란이 두고두고 생깁니다.** 겉보기가 브라우저인데 능력이 다르니까요.
  
  **A-1을 추천하는 이유:** 원칙 2(현재 요구를 충족하는 가장 단순한 구현)와 원칙 1(하위 호환 유지 안 함)에 맞습니다. grab이 빠진 순간 인앱 브라우저의 존재 이유가 사라졌는데, "있으면 편할 것 같아서" 남기면 그게 정확히 지난 4번의 런타임 전환에서 스코프를 부풀린 방식이었습니다.
  
  **A-1인가요, A-2인가요?**
- recommended: none
- answer: herdr browser pane 별론데... 그거 플러그인이지? 그거 chromux로 붙기는한거같은데 맞아?
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q3: 확인 끝났습니다. **선생님 감이 맞았고, 제가 놓친 세 번째 길이 있습니다.**
- decision_ids: none
- route: mixed
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:b2fd9d75-4607-47ab-865f-85b16e91c4e6
- asked: 확인 끝났습니다. **선생님 감이 맞았고, 제가 놓친 세 번째 길이 있습니다.**
  
  (확인하다가 `chromux open --help`가 Chrome을 자동 실행시켜서, 제가 띄운 프로필은 바로 닫았습니다.)
  
  ---
  
  ### 왜 herdr browser pane이 별로인지 - 원인 확인됨
  
  플러그인 소스를 직접 봤습니다. `official.browser` v0.1.0 (MIT, ogulcancelik/herdr-browser)
  
  ```
  src/chrome.ts:38 →  "--headless=new"
  ```
  
  **헤드리스 Chrome을 띄우고, 그 화면을 kitty graphics로 터미널에 스트리밍합니다.** (`herdrGraphics.ts`, `screencast*.ts`, `kitty.ts`) 진짜 창이 없어요. 터미널 안에 그려진 브라우저 영상입니다. 별로인 게 당연합니다.
  
  QA 자체는 됩니다 (`browser.ts:1090`에서 `cdp_http_url`을 내주고, Playwright MCP / browser-use / chrome-devtools-mcp 스니펫까지 만들어 줍니다). 하지만 **보는 경험이 나쁩니다.**
  
  ### 그런데 chromux는 혼자서도 됩니다
  
  ```
  $ chromux open --help
  Auto-launching profile [default]...
  {
    "headless": false,
    "launchMode": "headed",          ← 진짜 창
    "appBundle": "~/.chromux/apps/cx(default).app"
  }
  ```
  
  **chromux는 자기 소유의 진짜 Chrome 창을 띄웁니다.** 헤드리스가 아니고, 전용 앱 번들까지 따로 있습니다. herdr 플러그인이 전혀 필요 없어요.
  
  `chromux launch --headless`가 **옵션**인 걸 보면, 기본이 헤드리스가 아닌 게 설계 의도입니다.
  
  ---
  
  ### 그래서 선택지가 바뀝니다
  
  브라우저 QA 경로가 원래 이거였습니다:
  
  | | 에이전트 조작 | 사람이 보는 경험 | herdr-ide에 코드 필요? |
  |---|---|---|---|
  | herdr browser pane | 됨 | **터미널에 그려진 영상 (별로)** | 없음 |
  | **chromux 자체 Chrome** | **됨** | **진짜 Chrome 창** | **없음** |
  | CEF 임베드 (기존 `browser.rs`) | 됨 | 앱 안에 있음 | 721줄 + CEF 리스크 |
  
  **herdr browser pane을 안 써도 됩니다.** 세션 훅이 그 경로를 표준으로 적어둔 거지, 유일한 길이 아니었습니다. 선생님 글로벌 설정에도 "Use `chromux` for browser work"라고만 되어 있지 herdr pane을 쓰라고는 안 되어 있고요.
  
  그리고 chromux 쪽이 **선생님이 원하시던 것에 더 가깝습니다.** 진짜 Chrome 창이 옆에 떠 있고, 에이전트가 그걸 몰고, 선생님은 눈으로 그대로 봅니다. 사람과 에이전트가 같은 화면을 봅니다.
  
  ---
  
  ### Q2 (수정). 이걸로 정리하면 될까요?
  
  **A-1: herdr-ide에서 브라우저를 완전히 뺀다. 브라우저 QA는 chromux의 헤드리스 아닌 Chrome으로 한다.**
  
  - `src/browser.rs` 721줄 삭제
  - 선행 결정 D-11(브라우저 패널 + grab), D-19(CDP 노출), D-53(chromux 전제조건) 전부 무효화
  - D-16의 화면 구조에서 "우측 패널이 워크벤치↔브라우저 전환" → **워크벤치만** 남음
  - CEF 임베딩 리스크(D-12)와 메모리 예산 압박(D-41의 900MB 항목) 동시 소멸
  - herdr-ide는 **터미널 / 파일 / 에이전트**에 집중
  
  **얻는 것:** SwiftUI 전환의 가장 큰 걸림돌이 사라집니다. 브라우저가 빠지면 SwiftUI로 못 하는 게 없어집니다.
  
  **잃는 것:** 브라우저가 herdr-ide 창 안이 아니라 별도 Chrome 창입니다. 창 전환이 한 번 생깁니다. (선행 인터뷰에서 "창 전환으로 끊기던 집중을 없앤다"가 목표 문장이었어서, **이게 목표와 부딪히는지**가 판단 포인트입니다.)
  
  ---
  
  이대로 A-1으로 갈까요? 아니면 "창 전환 없이 한 화면에서 보는 것"이 여전히 포기하기 어려우신가요?
  
  (후자라면 C(CEF 임베드)가 유일한 길인데, 그건 SwiftUI 전환을 크게 무겁게 만듭니다. 그때는 전환 자체를 재검토해야 합니다.)
- recommended: none
- answer: 그래서 chromux 붙이면서 pane에 chrome 창읠 띄어버릴 수 있다는 이야기인거야? 좀 쉽게 알려줘봐~ browser pane은 프럴그인이자나! 내가 설치했던 custom
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q4: 아, 제가 헷갈리게 말했네요. **아닙니다.** 쉽게 정리할게요.
- decision_ids: none
- route: mixed
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:7a20eda0-5f30-4e80-9a02-f3587188f621
- asked: 아, 제가 헷갈리게 말했네요. **아닙니다.** 쉽게 정리할게요.
  
  ## 세 개가 서로 다른 물건입니다
  
  **1. herdr의 pane** = 터미널 안의 칸
  글자만 들어갑니다. **진짜 창은 절대 못 넣습니다.** 터미널 칸 안에 Chrome 창을 박아넣는 건 물리적으로 안 되는 일이에요.
  
  그래서 그 브라우저 플러그인이 **헤드리스 + 화면 스트리밍**이라는 방식을 쓴 겁니다. 창 없는 Chrome을 띄우고, 그 화면을 계속 캡처해서 터미널에 그림으로 뿌리는 거죠. **pane에 브라우저를 넣으려면 그 방법밖에 없어서** 그렇게 만든 거고, 그래서 화질과 반응이 별로인 겁니다. 만든 사람 잘못이 아니라 pane의 한계예요.
  
  **2. chromux의 Chrome** = 진짜 macOS 창
  바탕화면에 뜹니다. **pane 안에 못 들어갑니다.** 터미널 옆에 따로 떠 있는 거예요.
  
  **3. herdr-ide** = 우리가 만드는 네이티브 앱 ← **여기가 중요**
  얘는 터미널이 아닙니다. **자기 창을 가진 진짜 앱**이에요. 그래서 pane 제약을 안 받습니다. **herdr-ide 창 안에는 진짜 브라우저를 넣을 수 있습니다.**
  
  ```
  [터미널 안의 pane]  → 그림 스트리밍만 가능    (별로)
  [chromux Chrome]    → 진짜 창, 따로 떠 있음   (품질 좋음, 창 분리)
  [herdr-ide 창 안]   → 진짜 브라우저 넣기 가능  (품질 좋음 + 한 화면)  ← 옵션 C
  ```
  
  **즉 선생님이 원하시는 "한 화면에서 제대로 보이는 브라우저"는 herdr-ide 안에서만 가능합니다.** pane으로는 원래 안 되는 거였어요.
  
  ## 그리고 그 플러그인은 우리 것과 무관합니다
  
  맞습니다, 선생님이 설치하신 서드파티 플러그인이에요 (`ogulcancelik/herdr-browser`, MIT). herdr-ide가 그걸 의존하지 않고, 계속 쓰시든 지우시든 herdr-ide와 상관없습니다. 제가 아까 "herdr browser pane 쓰면 된다"고 한 건 **그 별로인 경험을 계속 쓰시라는 말이 되어버렸네요.** 그게 선생님이 걸린 지점이었고, 맞는 지적이었습니다.
  
  ## 그래서 진짜 선택지는 두 개입니다
  
  | | 브라우저 위치 | 품질 | herdr-ide 코드 | 에이전트 조작 |
  |---|---|---|---|---|
  | **A-1** | 별도 Chrome 창 (chromux) | 진짜 Chrome | **0줄** | 됨 |
  | **C** | herdr-ide 창 안 | 진짜 Chromium | **CEF 임베드 필요** | 됨 |
  
  (pane 스트리밍은 품질 때문에 선생님이 이미 탈락시키셨습니다.)
  
  **A-1**: 창이 두 개. 터미널/파일은 herdr-ide, 브라우저는 옆에 Chrome. 대신 herdr-ide는 브라우저를 아예 안 만듭니다.
  
  **C**: 창이 하나. 대신 **SwiftUI 앱에 CEF(Chromium)를 임베드**해야 합니다. 이게 얼마나 아픈지가 관건인데, 솔직히 **제가 아직 확인 못 했습니다.** Swift에서 CEF를 붙인 실사례가 있는지, 있다면 얼마나 굴러가는지요. 선행 인터뷰 D-12는 "Rust에는 실용 경로 없음"만 기록했고 Swift는 안 봤습니다.
  
  ---
  
  ### Q2. 어떻게 할까요?
  
  **(가) A-1로 확정** - 창 두 개를 받아들이고, herdr-ide는 브라우저를 안 만든다. `browser.rs` 721줄 삭제.
  
  **(나) C를 조사부터 해달라** - Swift + CEF가 현실적인지 제가 먼저 확인하고, 그 결과를 보고 정한다. 30분 정도 걸립니다.
  
  **(다) C로 확정** - 한 화면이 양보 불가. CEF 비용은 감수.
  
  솔직한 제 의견은 **(나)** 입니다. 지금 (가)를 고르면 "한 화면"이라는 원래 목표를 포기하는 건데, 그 대가가 얼마인지 모르는 채로 포기하는 거니까요. 반대로 (다)를 고르면 모르는 비용을 떠안는 거고요. **30분 조사하면 이 질문이 사실 질문으로 바뀝니다.**
  
  어느 쪽으로 갈까요?
- recommended: none
- answer: 조사 조금만 해주라! 
  긔리고 그동안 현재 browser plugin을 fork해서 위에 주소창 입력이나 그런 기본적인 브라우저 창 형태로 뭔가 만드는게 가능하려나? 그런것도 확인해봐
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q5: 조사 끝났습니다. 둘 다 답이 나왔는데, **두 개 다 "그 길은 아니다"** 쪽입니다.
- decision_ids: none
- route: mixed
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:e04a9481-b889-441f-8d4b-886cb765a27e
- asked: 조사 끝났습니다. 둘 다 답이 나왔는데, **두 개 다 "그 길은 아니다"** 쪽입니다.
  
  ---
  
  ## 1. 플러그인 fork해서 주소창 붙이기 → 이미 다 있습니다
  
  `viewer.ts`(1640줄)를 봤는데, 주소창도 버튼도 이미 만들어져 있습니다.
  
  ```
  viewer.ts:1482  [+]     새 탭
  viewer.ts:1484  [<]     뒤로
  viewer.ts:1485  [>]     앞으로
  viewer.ts:1486  [R]     새로고침
  viewer.ts:1487  [Stop]  정지
  viewer.ts:1488  [-][+]  줌
  viewer.ts:839   handleUrlInputKey()   ← URL 입력창
  탭 목록도 클릭으로 전환됨
  ```
  
  **부족한 게 UI가 아니었습니다.** 진짜 문제는 화면을 나르는 방식이에요:
  
  ```
  viewer.ts:292  const png = await screenshotData();     ← 매 프레임 PNG로 캡처
  viewer.ts:302  encodeKittyPng(png, ...)                ← 터미널에 그림으로 출력
  ```
  
  **매 프레임을 사진으로 찍어서 터미널에 붙이고 있습니다.** 흐릿하고 느린 게 당연해요.
  
  그리고 fork해도 이건 못 고칩니다. **터미널 pane은 글자랑 그림밖에 못 받거든요.** 진짜 브라우저를 그 칸에 넣는 방법이 없어서 사진으로 우회한 거라, 우회를 개선할 수는 있어도 없앨 수는 없습니다.
  
  → **fork는 노력 대비 얻는 게 없습니다.**
  
  ---
  
  ## 2. Swift + CEF → 길은 있는데 우리가 닦아야 합니다
  
  **Swift 바인딩은 죽었습니다.**
  
  `lvsti/CEF.swift` (유일한 Swift 바인딩)
  - 마지막 커밋: **2021-11-01** (5년 전)
  - 릴리스: **0개**
  - 별 104개
  
  **CEF 자체는 남의 NSView에 붙을 수 있습니다.** `SetAsChild(parent_nsview, rect)` API가 있습니다. 기술적으로 불가능한 건 아니에요.
  
  **그런데 macOS 임베딩은 알려진 문제가 많습니다.** CEF 공식 포럼에 계속 올라오는 것들:
  - 기존 NSView에 붙였는데 **화면이 하얗게 나옴** (조작은 되는데 안 그려짐)
  - 부모 NSView 리사이즈를 **CEF 뷰가 안 따라감**
  - **포커스 문제** - macOS는 key window가 하나뿐이라 앱 창과 Chromium이 동시에 포커스를 못 가짐
  
  **그리고 Swift에서 쓰려면 브리지를 직접 만들어 유지해야 합니다.** CEF는 C++ API라서 `C++ → ObjC++ → Swift` 다리를 놓아야 하고, 그 다리는 우리 것이 됩니다. CEF는 Chromium 버전 따라 자주 올라가고요.
  
  → **선행 인터뷰 D-12가 Rust에 대해 내린 판정("실용 경로 없음")이 Swift에서도 똑같습니다.** 언어를 바꿔도 문제가 안 풀립니다.
  
  **추가로:** CEF를 넣으면 Chromium 하나가 앱에 상주합니다. D-41의 메모리 예산 900MB와 정면으로 부딪히고, 그건 선생님이 **orca를 무거워서 버린 바로 그 이유**입니다.
  
  ---
  
  ## 정리하면
  
  | 길 | 판정 |
  |---|---|
  | pane 플러그인 fork | 주소창은 이미 있음. 사진 스트리밍이라 fork로 안 고쳐짐 |
  | Swift + CEF 임베드 | 바인딩 5년 방치, macOS 임베딩 버그 다수, 브리지 자체 유지, 메모리 예산 충돌 |
  | **chromux 진짜 Chrome 창** | **오늘 바로 됨. 코드 0줄** |
  
  세 번째 말고는 답이 없어 보입니다. 다만 이건 제가 30분 본 결과라, **CEF를 실제로 붙여본 게 아니라 문서와 이슈를 읽은 판단**입니다.
  
  ---
  
  ## Q3. 그러면 이렇게 갈까요?
  
  **A-1 확정: herdr-ide는 브라우저를 안 만든다.** `browser.rs` 721줄 삭제, D-11/D-19/D-53 무효화, 우측 패널은 워크벤치만.
  
  남는 문제는 **"창이 두 개"** 하나뿐인데, 여기에 값싼 보완책이 하나 있습니다:
  
  **herdr-ide가 chromux Chrome 창을 자기 옆에 붙여준다.** macOS 접근성 API로 다른 앱 창의 위치/크기를 조절할 수 있으니, herdr-ide가 "브라우저 열기"를 누르면 chromux Chrome을 띄우고 herdr-ide 오른쪽에 딱 맞춰 배치하는 겁니다. 화면상으로는 한 화면처럼 보이고, 브라우저는 진짜 Chrome입니다.
  
  *(이건 아이디어고 **아직 확인 안 했습니다.** 선생님 pet 앱이 이미 macOS 창 조작을 하고 있었으니 경험은 있으실 텐데, 실제로 되는지는 별도 확인이 필요합니다.)*
  
  **선택해 주세요:**
  
  **(가)** A-1 확정. 창 배치 보완책은 나중에 별도로 검토.
  **(나)** A-1 확정 + 창 배치 보완책도 v1에 포함. (먼저 되는지 확인 필요)
  **(다)** 아직 CEF를 못 놓겠다. 더 깊게 파본다.
  
  제 추천은 **(가)** 입니다. 창 배치는 나중에 얹을 수 있는 기능이고, 지금 v1에 넣으면 SwiftUI 전환이라는 본 작업에 또 하나가 붙습니다. 지난 4번의 런타임 전환이 정확히 그런 식으로 무거워졌고요.
- recommended: none
- answer: 가로 가자. 근데 리서치좀 해볼래? 그래서 chromux Chrome을 띄우게 하고 cdp로 조절이 가능하게?
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q6: 조사 끝났습니다. **실제로 붙여서 확인했습니다.** (테스트로 띄운 Chrome은 닫았습니다.)
- decision_ids: none
- route: mixed
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:cca7c3c2-4825-47c0-bdd2-c1faaa5d0dd8
- asked: 조사 끝났습니다. **실제로 붙여서 확인했습니다.** (테스트로 띄운 Chrome은 닫았습니다.)
  
  ## 결론: 됩니다. 그것도 아주 싸게.
  
  chromux Chrome을 띄우고 CDP 포트에 **외부 프로세스가 직접 붙는 걸 실측**했습니다.
  
  ```
  $ chromux launch default
    { "port": 9300, "headless": false, "launchMode": "headed" }
  
  $ curl http://127.0.0.1:9300/json/version
    { "Browser": "Chrome/151.0.7922.175",
      "Protocol-Version": "1.3",
      "webSocketDebuggerUrl": "ws://127.0.0.1:9300/devtools/browser/0893b33f-..." }
  
  $ curl http://127.0.0.1:9300/json/list
    browser_ui | Who's using Chrome? | chrome://profile-picker/
  ```
  
  **진짜 Chrome 151이 창을 띄운 채로 CDP를 열어두고 있고, 아무 프로세스나 붙을 수 있습니다.** herdr-ide도 붙을 수 있다는 뜻입니다.
  
  포트는 `chromux ps --json`이 알려줍니다:
  ```json
  { "profile": "default", "port": 9300, "pid": 72390,
    "launchMode": "headed", "status": "running",
    "resources": { "chromeProcesses": 6, "renderers": 2, "rssMb": 676 } }
  ```
  
  ---
  
  ## 그런데 herdr-ide가 CDP를 직접 쓰면 안 됩니다
  
  기술적으로는 되는데, **하면 안 되는 이유가 있습니다.**
  
  chromux는 그 Chrome의 **주인**입니다. 데몬을 띄우고, 세션을 관리하고, 탭 작업을 직렬화하고(`serialQueue.ts`), `pause`/`resume`으로 잠급니다. 여기에 herdr-ide가 같은 포트로 따로 명령을 쏘면 **주인이 둘**이 됩니다. 에이전트가 클릭하는 도중에 herdr-ide가 페이지를 바꾸는 식의 사고가 납니다.
  
  그래서 이렇게 나누는 걸 권합니다.
  
  **행동은 chromux CLI로** (herdr-ide가 명령어를 실행)
  ```
  chromux launch default              브라우저 켜기
  chromux open <세션> <url>            페이지 열기
  chromux ps --json                   상태 확인
  ```
  Rust에서 `std::process::Command` 몇 줄입니다. **의존성 0개.**
  
  **상태 표시는 HTTP GET 하나로**
  ```
  GET http://127.0.0.1:<port>/json/list   → 지금 열린 탭 목록
  ```
  읽기만 하니까 충돌이 없습니다. 이것도 **crate 필요 없습니다.** (`chromiumoxide` 0.9.1, `headless_chrome` 1.0.22 같은 Rust CDP 크레이트가 있긴 한데, 우리한테는 과합니다. 읽기 전용 GET 하나에 async CDP 스택을 끌어올 이유가 없어요.)
  
  **에이전트 조작은 그대로 chromux**
  에이전트는 지금처럼 `chromux snapshot / click / screenshot`을 씁니다. herdr-ide는 여기 안 끼어듭니다.
  
  ---
  
  ## 그래서 herdr-ide가 실제로 갖는 브라우저 코드
  
  ```rust
  // 대략 이 정도. 100줄 안쪽.
  fn browser_launch()          -> chromux launch <profile>
  fn browser_open(url)         -> chromux open <session> <url>
  fn browser_status()          -> chromux ps --json  +  GET /json/list
  ```
  
  **`browser.rs` 721줄이 100줄 안쪽으로 줄어듭니다.** CEF도, CDP 게이트웨이도, capability 토큰도 전부 사라집니다.
  
  그리고 안전장치가 하나 확인됐습니다. chromux는 **프로필을 자기 마음대로 안 만듭니다.**
  ```
  $ chromux launch herdr-test
  Profile "herdr-test" does not exist, and chromux does not create profiles on its own.
  Use the "default" profile, or ask the user before creating a new one.
  ```
  herdr-ide도 이 규칙을 따라야 합니다. 있는 프로필만 쓰고, 새로 만들려면 사용자에게 묻습니다.
  
  ---
  
  ## 그런데 이게 결정 하나를 건드립니다
  
  빈 Chrome 하나가 **676MB / 프로세스 6개**를 씁니다.
  
  선행 인터뷰 D-41이 메모리를 v1 인수 조건으로 못박아뒀는데, 그게 **브라우저가 앱 안에 있다는 전제**였습니다:
  > workspace 7 / pane 11 기준, 브라우저 패널 닫은 상태 **400MB 이하**, 브라우저 하나 연 상태 **900MB 이하**. 초과하면 v1 미완료.
  
  이제 브라우저가 밖에 있으니 이 조건을 다시 써야 합니다.
  
  ### Q4. 메모리 예산을 어떻게 고칠까요?
  
  **(1) herdr-ide 프로세스만 잰다. 400MB 이하.** ← 추천
  Chrome은 사용자가 직접 켜고 끄는 별도 앱이니 우리 예산 밖. 조건이 단순해지고, SwiftUI+Rust면 400MB는 여유롭습니다. (지금 Rust 네이티브도 그 근처일 가능성이 큽니다.)
  
  **(2) Chrome까지 합산해서 900MB 유지.**
  "내 맥에서 이 작업이 얼마나 무거운가"를 보는 관점. 근데 Chrome 하나가 이미 676MB라 herdr-ide에 224MB밖에 안 남습니다. 그리고 사용자가 Chrome에 탭을 많이 열면 우리 잘못이 아닌 걸로 v1이 실패 판정됩니다.
  
  **(3) 메모리 인수 조건 자체를 뺀다.**
  orca를 무거워서 버리신 실증이 근거였는데, 그 무게의 정체가 "Electron이 Chromium을 앱에 넣은 것"이었습니다. 그걸 안 하기로 했으니 원인이 사라졌다는 논리. 다만 **측정 가능한 인수 조건 하나를 잃습니다.**
  
  제가 **(1)** 을 추천하는 이유는, 우리가 통제할 수 있는 것만 재는 게 검증으로서 정직하기 때문입니다. Chrome 메모리는 우리가 못 고치는데 그걸로 합격/불합격을 가르면 그 게이트는 곧 무시됩니다.
  
  **(1)(2)(3) 중 어느 쪽인가요?**
- recommended: none
- answer: ㅇㅇㅇ 그렇게 하자 1로 ㄱㄱ
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

## UX Scenario Cards

## Evidence From Code, Docs, Or Research

## Documented Domain Checks

- docs inspected:
- canonical terms:
- glossary or code conflicts:
- concrete scenarios tested:
- docs mutation:
- ADR candidate:

## Checkpoint And Sweep History

### Checkpoint 1
- after_question: Q6
- normalized_entries: Q1, Q2, Q3, Q4, Q5, Q6
- register_changes: 브라우저 축 종결: grab 비목표(D-08), 플러그인 fork 기각(D-09), Swift+CEF 기각(D-10), A-1 확정(D-11), chromux headed CDP 실측(D-12), 연동 방식 확정(D-13), 메모리 예산 재정의(D-14). 선행 인터뷰 D-11/D-19/D-41/D-53 무효화.
- reopened_decisions: 선행 D-34, D-35 (CDP 전제 붕괴 -> D-17로 이관)
- highest_remaining_gap: SwiftUI 네이티브 앱을 에이전트가 자동 검증할 경로가 없다. D-35가 에이전트 검증 가능성을 제품 요구로 규정했으므로 P0.

## Audit History
