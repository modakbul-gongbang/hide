---
topic: "herdr-ide 셸을 SwiftUI + Rust 코어 하이브리드로 전환"
status: "complete"
where: "brownfield"
selected_packs: "ux, compatibility, risk, operation, verification"
created_at: "2026-08-28"
updated_at: "2026-08-28"
question_count: 22
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

- next_decision_id: D-53
- next_question: (owned by the live conversation until checkpoint)
- last_materiality_sweep: checkpoint 8
- outstanding_raw_entries: none
- next_checkpoint_at: Q32

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
| D-07 | fact | 브라우저 | src/browser.rs(721줄)는 CEF CDP 업스트림에 붙는 loopback CDP 게이트웨이로 구현돼 있다(에러 문구 'CEF CDP upstream unavailable', attach_chromux/detach_chromux, capability 스코프). 즉 현 설계는 앱에 CEF(Chromium)를 임베드하는 전제다. 선행 인터뷰 D-11은 브라우저 패널+grab을 v1 최대 기능으로, D-19는 chromux가 붙을 CDP 엔드포인트 노출을, D-53은 chromux를 전제조건으로 확정했다. 한편 D-12는 Rust에서 CEF 임베딩에 실용 경로가 없다고 기록했다. SwiftUI 전환 시 이 축이 전부 재결정 대상이다. | P0 | repo: src/browser.rs:43,92,109,144 + herdr-ide-native-shell D-11/D-12/D-19/D-53 | resolved | D-11이 종결. browser.rs 삭제 대상 |
| D-08 | decision | 스코프 | grab(브라우저 요소를 집어 HTML/CSS/크롭 스크린샷을 에이전트 pane에 주입)은 v1 비목표다. 선행 인터뷰 D-11이 v1 최대 기능으로 두었으나 사용자가 현재 필요 없다고 판정했다. 이 결정이 인앱 브라우저의 마지막 존재 이유를 제거한다. | P0 | user Q2 | resolved | 비목표(v1) / 선행 D-11의 grab 성분 무효 |
| D-09 | fact | 브라우저 | herdr official.browser 플러그인(ogulcancelik/herdr-browser v0.1.0, MIT, 사용자가 설치한 서드파티)은 --headless=new Chrome을 띄우고 매 프레임을 PNG로 캡처해 kitty graphics로 터미널에 그린다. 주소창(handleUrlInputKey)과 탭/뒤로/앞으로/새로고침/정지/줌 버튼은 이미 구현돼 있다. 즉 부족한 것은 UI가 아니라 전송 방식이며, 터미널 pane이 글자와 그림만 받으므로 fork로는 화질/반응 문제를 해결할 수 없다. | P1 | repo: ~/.config/herdr/plugins/github/official.browser-ff2a44eccae9/src/{chrome.ts:38,viewer.ts:292,302,839,1482-1489} | resolved | R#: 플러그인 fork 기각 근거 |
| D-10 | fact | 브라우저 | Swift에서 CEF 임베드에 실용 경로가 없다. 유일한 Swift 바인딩 lvsti/CEF.swift는 2021-11-01 이후 방치되고 릴리스가 0개다. CEF 자체는 SetAsChild(parent_nsview, rect)로 기존 NSView에 붙는 API가 있으나, CEF 포럼에 macOS 임베딩 시 빈 화면 렌더링, 부모 NSView 리사이즈 미추종, key window 하나 제약에 의한 포커스 문제가 반복 보고된다. Swift에서 쓰려면 C++/ObjC++/Swift 브리지를 자체 유지해야 한다. 선행 D-12가 Rust에 내린 판정과 동일하다. 조사 방식은 문서와 이슈 검토이며 실제 임베딩을 시도하지 않았다. | P0 | github.com/lvsti/CEF.swift API(마지막 push 2021-11-01, 릴리스 0, star 104), magpcss.org CEF 포럼 macOS 임베딩 스레드 다수 (2026-08-28 조사) | resolved | R#: 옵션 C 기각 근거 |
| D-11 | decision | 아키텍처 | A-1 확정. herdr-ide는 브라우저를 임베드하지 않는다. src/browser.rs 721줄(CEF CDP 게이트웨이, attach_chromux/detach_chromux, capability 토큰)을 삭제한다. 선행 인터뷰의 D-11(브라우저 패널+grab), D-19(CDP 엔드포인트 노출), D-53(chromux 전제조건)을 무효화한다. D-16의 화면 구조에서 우측 패널의 브라우저 전환이 빠지고 워크벤치만 남는다. 수용한 비용은 브라우저가 herdr-ide 창 밖의 별도 Chrome 창이라는 것이다. | P0 | user Q5 | resolved | R#: 브라우저 경계 / 선행 D-11/D-19/D-53 무효화 |
| D-12 | fact | 브라우저 | chromux는 headed(창 있는) 진짜 Chrome을 띄우고 loopback CDP를 외부에 노출한다. 실측: launchMode headed, headless false, port 9300, Chrome/151.0.7922.175, Protocol-Version 1.3, webSocketDebuggerUrl 제공, /json/list로 탭 목록 조회 가능. 포트는 chromux ps --json이 알려준다. 빈 Chrome 1개의 실측 자원은 프로세스 6개 / 렌더러 2개 / RSS 676MB다. 또한 chromux는 존재하지 않는 프로필을 스스로 만들지 않고 사용자 승인을 요구한다. | P0 | 로컬 실측 2026-08-28: chromux launch default + curl 127.0.0.1:9300/json/version, /json/list, chromux ps --json | resolved | R#: A-1 성립 근거 |
| D-13 | decision | 브라우저 | herdr-ide의 chromux 연동 계약을 4개 조작으로 확정한다. (1) launch: chromux ps --json에 default 프로필이 running이 아니면 chromux launch default를 실행한다. (2) reuse: 이미 running이면 launch하지 않는다. (3) focus: 이미 running이면 그 Chrome 창을 앞으로 가져온다. (4) status: GET http://127.0.0.1:<port>/json/list로 현재 탭의 URL과 제목만 읽어 표시하고, 탭이 없으면 '열린 탭 없음'을 표시한다. URL을 herdr-ide가 받아서 여는 조작(browser_open(url))은 D-40에 따라 비목표이며 이 계약에서 제외한다. herdr-ide가 CDP로 페이지를 조작하는 것은 금지한다. 근거는 chromux가 데몬/세션/serialQueue/pause 잠금을 소유하고 있어 주인이 둘이 되면 에이전트 조작 도중 충돌이 나기 때문이다. Rust CDP 크레이트는 읽기 전용 GET 하나에 과하므로 쓰지 않는다. 없는 프로필을 임의 생성하지 않는다. 종료도 하지 않는다(D-31). 예상 코드량 100줄 안쪽. | P1 | user Q12 '1. 가' | resolved | R#: chromux 연동 방식 / D-40과 정합 |
| D-14 | decision | 검증 | 메모리 인수 조건을 herdr-ide 프로세스만 400MB 이하로 재정의한다. 선행 D-41(브라우저 닫힘 400MB / 브라우저 열림 900MB)을 대체한다. 근거는 D-11로 브라우저가 앱 밖 별도 Chrome이 되어 우리가 통제할 수 없는 자원이 되었기 때문이며, 통제 못 하는 값으로 합격/불합격을 가르면 그 게이트는 곧 무시된다. 측정 시나리오는 D-41과 같이 workspace 7 / pane 11 실사용 규모를 유지하고 ps 기반 자동 판정도 유지한다. | P0 | user Q6 | resolved | V#: 메모리 인수 조건 / 선행 D-41 대체 |
| D-15 | decision | 아키텍처 | SwiftUI + Rust 코어 하이브리드 전환을 확정한다. 합의 목표: herdr-ide의 macOS 셸을 SwiftUI + SwiftTerm으로 교체하고 SSH·herdr·도메인 로직은 Rust 코어로 남겨, 한글 IME와 UI 품질을 AppKit이 해결하게 한다. 브라우저는 앱 밖 chromux Chrome에 맡긴다. 단 D-18의 0단계 스파이크 4항목 통과가 선행 조건이며 실패 시 그 자리에서 멈추고 재검토한다. 기존 Rust 코드 삭제는 D-22에 따라 스파이크 통과 후 태그를 남기고 수행한다. | P0 | user Q12 '3. ㅇㅇㅇ' | resolved | R#: 전환 확정 / D-18 스파이크 조건부 |
| D-16 | decision | 터미널 | 터미널 뷰로 SwiftTerm을 채택한다(버전은 D-23). 엔진과 뷰를 모두 SwiftTerm에 맡기고, Rust 코어는 SSH/PTY 바이트만 담당해 feed(byteArray:)로 넣고 send(source:data:) 델리게이트로 받는다. 결과로 alacritty_terminal 의존과 src/terminal.rs 755줄, src/render.rs 921줄이 폐기된다. 기각한 대안 둘: (1) libghostty - 1차 인터뷰 D-13이 채택했다가 D-14가 최상위 리스크로 걸었던 것으로, 2026-08-28 재확인 시점에도 Ghostty 공식 문서가 'not yet stabilized for general-purpose embedding, may change significantly between releases'라고 명시하며 유일한 검증 소비자가 Ghostty 자체 macOS 앱이다. 안정화 근접한 것은 파서인 libghostty-vt뿐이라 우리가 필요한 뷰를 대체하지 못한다. (2) NSTextView 위 alacritty_terminal 직접 구현 - 셀 렌더링/스크롤백/선택/링크/리사이즈를 자체 구현해야 하며 이는 현재 render.rs 921줄이 하는 일 그대로라 전환의 목적에 반한다. SwiftTerm 채택의 잔여 리스크: 사실상 개인 주도 프로젝트(open issue 75), 릴리스 변동 잦음. 완화책은 D-23의 정확 버전 고정과 D-24의 herdrm 참조 경로다. | P0 | user Q8 | resolved | R#: 터미널 뷰 / libghostty 기각 근거 포함 |
| D-17 | decision | 검증 | SwiftUI 앱의 에이전트 검증은 peekaboo(4.2.2 설치 확인)로 한다. peekaboo see --app으로 접근성 기반 UI 요소 맵을 받고 click/type/press로 조작한다. 선행 D-34의 'chromux가 herdr-ide CDP에 붙어 조작'을 대체한다. 결과 확인에 herdr 소켓 API를 쓰는 부분은 그대로 유지한다. SwiftUI가 접근성 트리를 기본 제공하므로 src/accessibility.rs 165줄은 폐기된다. 수용한 비용: Screen Recording과 Accessibility 권한이 필요해 D-29 공개 배포 대상자와 CI에서 마찰이 있다. 에이전트가 상태 스냅샷을 직접 읽는 별도 표면은 채택하지 않았다. | P0 | user Q7 | resolved | V#: 앱 검증 경로 / 선행 D-34 대체 |
| D-18 | decision | 리스크 | 0단계 스파이크를 전환의 게이트로 둔다. 근거는 지난 4번의 런타임 전환 중 3번이 '결정 후에 죽일 요인을 만나서' 죽었다는 이력이다(1차는 승인 후 Xcode 전체 설치 장벽으로 소스 0줄, 3차는 질문 44개 인터뷰 후 메모리 실증과 충돌해 코드 전 폐기, 4차는 16040줄을 만든 뒤 UI 품질과 IME로 폐기). 스파이크에서 네 가지를 모두 시험한다: (1) Rust staticlib과 Swift 간 FFI 비용, 특히 SSH 이벤트를 Swift로 올리는 비동기 콜백 (2) Rust가 뽑은 SSH 바이트를 SwiftTerm feed(byteArray:)에 넣고 send 델리게이트로 되받는 왕복 (3) 원격 SSH + 에이전트 TUI 환경에서의 한글 IME 실동작 (4) xcodebuild 없이 .app 번들 조립, 코드사인, 공증, Sparkle. 하나라도 실패하면 그 자리에서 멈추고 재검토한다. 기간은 2~3일. | P0 | user Q9 | resolved | R#: 전환 게이트 / V#: 스파이크 통과 조건 |
| D-19 | decision | 검증 | 한글 조합 입력을 사람 판정 완료 게이트로 둘지는 0단계 스파이크 결과를 보고 정한다. 판정 방법은 한글 IME를 켜고 peekaboo type 'gks'를 보내 '한'이 나오는지 보는 것이다. 나오면 peekaboo가 IME 경로를 거치는 것이라 자동 게이트로, 안 나오면 IME를 우회하는 것이라 사람 게이트로 간다. owner: 사용자. revisit trigger: 0단계 스파이크 완료 시점. 사람 게이트가 될 경우 확인 항목은 후보창이 커서를 따라오는가, 조합 중 백스페이스가 조합을 지우는가, 두 글자 이상 조합이 뒤 글자를 덮지 않는가, 터미널과 에디터 양쪽에서 되는가 네 가지다. | P0 | user Q8 | deferred | V#: IME 게이트 / revisit: 0단계 스파이크 |
| D-20 | decision | 아키텍처 | 기존 Rust 네이티브 셸 코드를 삭제해도 된다. 폐기 대상은 app.rs 2412줄 중 AppKit/wgpu 부분, render.rs 921줄, terminal.rs 755줄, accessibility.rs 165줄, browser.rs 721줄과 wgpu/glyphon/bytemuck/raw-window-handle/objc2/alacritty_terminal 의존이다. 유지 대상은 remote.rs 3550줄, herdr.rs 1154줄, files/commands/domain/navigator/layout/presentation/browser 축을 제외한 로직 계층이다. 삭제 시점과 되돌리기 지점은 D-18에서 정한다. | P0 | user Q8 | resolved | R#: 폐기 범위 |
| D-21 | fact | 운영 | Xcode.app은 설치되어 있지 않고 Command Line Tools만 있다(/Library/Developer/CommandLineTools). xcodebuild와 xcodegen은 쓸 수 없다. 그러나 SwiftUI.framework와 AppKit.framework가 CLT SDK에 포함되어 있고, swiftc -parse-as-library로 SwiftUI 앱을 실제로 컴파일해 arm64 Mach-O 실행파일을 얻는 데 성공했다(Swift 6.0.3). 따라서 선행 D-04가 기록한 PW1(Xcode 전체 설치) 차단요인 - 1차 전환 herdr-lightweight-ide를 소스 0줄로 죽인 원인 - 은 이번 전환에서 해소된다. 다만 herdrm이 쓰는 XcodeGen+xcodebuild 방식은 쓸 수 없고, .app 번들 조립과 코드사인/공증 경로는 아직 확인하지 않았다. | P0 | 로컬 실측 2026-08-28: xcode-select -p, xcodebuild -version, swiftc 빌드 성공 | resolved | R#: 1차 전환 차단요인 해소 |
| D-22 | decision | 운영 | 기존 Rust 네이티브 코드(D-20의 폐기 대상)는 0단계 스파이크를 통과한 뒤에 삭제한다. 삭제 직전 커밋에 태그를 남긴다(예: rust-native-final). 근거는 스파이크가 실패했을 때 돌아갈 지점이 필요하기 때문이며, 지금 삭제하면 되돌릴 곳이 없어진다. 원칙 1(하위 호환 유지 안 함)은 살아 있는 코드 경로를 둘로 유지하지 말라는 것이지 되돌리기 지점을 남기지 말라는 것이 아니므로 충돌하지 않는다. | P0 | user Q9 | resolved | R#: 되돌리기 지점 / D-20 실행 조건 |
| D-23 | decision | 의존성 | SwiftTerm을 최신 릴리스 1.20.0(2026-08-18)으로 정확 고정한다. herdrm은 from: 1.19.0으로 하한만 두지만 herdr-ide는 Cargo.toml이 전부 = 고정인 기존 방침과 1차 인터뷰 D-24의 버전 고정 방침에 맞춰 exact 고정한다. 근거는 SwiftTerm 릴리스 속도가 빠르고(1.18 8/9, 1.19와 1.20이 같은 날 8/18) 1.19에서 1.20 사이 파괴적 변경 여부를 확인하지 않았기 때문이다. 업데이트는 수동 검증 후에만 올린다. | P1 | user Q10 | resolved | R#: SwiftTerm 버전 고정 |
| D-24 | decision | 리스크 | 한글 IME 문제가 발생하면 missuo/herdrm의 Sources/HerdrM/TerminalView.swift를 참조해 대응한다. 사용자가 herdrm에서 한글이 잘 동작한다고 실사용으로 확인했다. 참조 대상 패턴은 rememberIMECaretFrame(마지막 유효 캐럿 프레임을 캐시해 후보창이 0,0으로 떨어지는 것을 방지)과 isEditShortcutDuringComposition(조합 중 편집 단축키를 interpretKeyEvents에서 걸러냄)이다. 제약: herdrm은 GitHub API licenseInfo가 NONE이고 저장소에 LICENSE 파일이 없어 기본적으로 all rights reserved다. herdr-ide는 MIT이므로 herdrm 코드를 복사해 넣을 수 없고, 읽고 접근법을 이해한 뒤 독립 구현만 가능하다. 선행 D-53에서 chromux가 같은 상태(public이지만 라이선스 없음)였다가 MIT 추가로 해소된 전례가 있으므로, herdrm에 라이선스가 추가되면 이 제약을 재검토한다. | P1 | user Q10 | resolved | R#: 한글 IME 대응 경로 / revisit: IME 결함 발생 시 |
| D-25 | decision | compatibility | 윈도우 지원은 하지 않는다. macOS 전용이다. 초기 표현은 '우선 당장 안해도 돼'였으나 확인 질문에 '응 윈도우는 안해'로 답해 연기가 아닌 미지원으로 확정한다. 이 결정이 D-16(SwiftTerm 채택 - 터미널 엔진을 Rust에 유지할 근거 소멸)과 D-11(브라우저를 앱 밖 chromux에 위임)의 전제다. 두 결정은 이 전제 위에서만 성립하므로, 훗날 윈도우 요구가 생기면 셋을 함께 재검토해야 한다. | P0 | user Q12 '2. 응 윈도우는 안해' | resolved | 비목표(v1) / D-16·D-11의 전제 |
| D-26 | fact | 선행이력 | 이 인터뷰는 선행 인터뷰 herdr-ide-native-shell(질문 44개, D-01~D-59)을 대체하지 않고 런타임 축만 개정한다. 선행 인터뷰가 이미 확정한 것으로 이번에 재론하지 않는 항목: 제품 정체성(D-09), 화면 구조 3열(D-16), 파일 워크벤치 깊이 F2(D-25), workspace/worktree 모델(D-24), 사이드바 계약(D-40/D-54), 생성·닫기 조작과 파괴적 동작 고지(D-42/D-44/D-50), 원격 타겟 설정(D-45), herdr 버전 게이팅(D-46), pet 흡수(D-07/D-15), 배포 방침(D-29). 이번 인터뷰가 무효화한 것은 D-11, D-19, D-34, D-41, D-53이며 각각 D-08/D-11, D-11, D-17, D-14, D-11이 대체한다. | P0 | repo: agents/interview/herdr-ide-native-shell/qa-log.md (질문 44개, status complete) | resolved | R#: 이 인터뷰의 범위 경계 |
| D-27 | decision | UX/design | herdr-ide가 chromux Chrome 창의 위치·크기를 자동 배치해 한 화면처럼 보이게 하는 것은 v1 비목표다. 사용자가 브라우저가 앱 밖이라는 사실을 확인하고 수용했다. v2 후보로만 기록한다. 창이 두 개라는 것이 D-11의 수용한 비용이다. | P2 | user Q12 확인 응답에 대한 에이전트 정리, 사용자 이견 없음 | resolved | 비목표(v1) / v2 후보 |
| D-28 | decision | UX/design | 파일 트리와 뷰어의 구현 스택을 확정한다. 트리는 SwiftUI OutlineGroup/List로 직접 만들고 외부 의존성을 쓰지 않는다. 코드 뷰어와 가벼운 편집은 CodeEditSourceEditor(MIT, star 716, CodeEdit 프로젝트에서 분리한 macOS 네이티브 패키지, tree-sitter 하이라이팅)를 쓴다. 마크다운은 swift-markdown-ui(MIT, star 3921) 또는 swiftlang/swift-markdown(Apache-2.0)을 쓴다. 이미지는 SwiftUI Image 기본이다. git diff는 Rust 코어에서 계산해 색만 입혀 표시하며 별도 라이브러리를 쓰지 않는다. 기각한 후보: STTextView는 GPLv3 또는 상용 라이선스라 MIT인 herdr-ide에 쓸 수 없다(LICENSE.md 확인). Splash는 Swift 코드만 강조하고 2024-05-27 이후 정체다. Runestone은 iOS 중심으로 macOS 지원을 확인하지 못했다. Highlightr(MIT)는 NSTextView와 조합하는 더 가벼운 대안으로 남긴다. 잔여 리스크: CodeEditSourceEditor는 마지막 푸시 2026-04-20으로 4개월 정체이고 open issue 48개다. 이 리스크가 현실화하면 Highlightr 대안으로 후퇴한다. | P1 | user Q13 '추천 조합으로 가고' | resolved | R#: 워크벤치 구현 스택 / D-25(F2 깊이) 실현 수단 |
| D-29 | decision | 운영 | 빌드와 배포 계약을 확정한다. 지원 OS는 macOS 14.0+(LSMinimumSystemVersion 14.0), CPU 타겟은 arm64 전용이다. 유니버설 바이너리는 비목표이며 근거는 Rust를 두 아키텍처로 빌드해 lipo로 합치는 파이프라인 이중화를 피하기 위함이다. Sparkle 자동 업데이트는 v1 비목표이고 배포는 GitHub Releases로 한다. 근거는 자동 업데이트가 서명된 appcast, 배포 서버, EdDSA 키 관리를 함께 요구하며 나중에 얹을 수 있기 때문이다. 패키징은 swiftc로 실행파일을 만들고 Contents/MacOS와 Info.plist로 .app 번들을 셸 스크립트로 조립한 뒤 codesign --force --deep -s -로 ad-hoc 서명한다. Apple Developer Program 가입과 공증은 v1 비목표이며, 사용자는 첫 실행 시 우클릭 후 열기로 Gatekeeper를 통과한다. 공증이 필요해지면 notarytool과 stapler가 CLT에 이미 있으므로 계정만 추가하면 된다. | P1 | user Q14 (1 macOS 14.0+, 2 arm64, 3 Sparkle 비목표, 4 최대한 간단한 것으로 위임) | resolved | R#: 빌드·배포 계약 / 비목표: 유니버설, Sparkle, 공증 |
| D-30 | fact | 운영 | Xcode 없이 배포 가능한 .app을 만들 수 있음을 실측했다. (1) swiftc가 산출한 실행파일은 이미 ad-hoc 서명되어 있다(CodeDirectory flags=0x20002 adhoc,linker-signed). arm64 macOS는 모든 바이너리에 최소 ad-hoc 서명을 요구하므로 이는 필수 조건이며 자동으로 충족된다. (2) codesign(/usr/bin/codesign), notarytool(CLT 경로), stapler(/usr/bin/stapler), xcrun이 모두 Xcode 없이 사용 가능하다. (3) Contents/MacOS + Info.plist로 .app을 손조립하고 codesign --force --deep -s -로 재서명하면 Signature=adhoc, TeamIdentifier=not set 상태의 정상 번들이 된다. 따라서 D-18의 스파이크 4번 항목은 '서명 없는 .app이 실행되는가'로 좁혀지며, 패키징 가능성 자체는 이미 사실이다. | P0 | 로컬 실측 2026-08-28: codesign -dv, xcrun -f, .app 손조립 후 codesign --force --deep -s - | resolved | R#: D-29 성립 근거 / 스파이크 4번 항목 축소 근거 |
| D-31 | decision | 외부의존 | chromux 연동 계약을 확정한다. (1) 프로필은 default 하나만 쓴다. workspace별 프로필 분리는 비목표이며, 그 근거였던 브라우저 패널이 D-11로 사라졌고 프로필이 늘면 디스크와 로그인 관리가 따라온다. herdr-ide는 chromux의 규칙대로 없는 프로필을 임의 생성하지 않는다. (2) chromux가 없으면 브라우저 열기 버튼을 비활성화하고 사유와 설치 안내를 표시한다. 조용히 실패하지 않는다(원칙 4). 선행 D-53의 부분 degrade 방식을 대체한다. (3) 이미 실행 중이면 재사용한다. chromux ps --json의 status가 running이면 launch를 건너뛰고 open만 호출한다. 매번 새로 띄우면 로그인 세션이 날아가기 때문이다. (4) herdr-ide는 Chrome을 종료하지 않는다. herdr-ide를 닫아도 Chrome은 남긴다. 에이전트가 그 Chrome을 쓰고 있을 수 있고 우리가 만들지 않은 것을 닫지 않는다는 원칙 때문이다. 정리는 사용자의 chromux kill이나 chromux kill --idle에 맡긴다. (5) 버전 게이팅은 herdr에 적용한 D-46의 protocol 정수 판정 방식을 쓰지 않고, 필요한 명령이 없으면 그 사유를 표시하고 실패하는 방식으로 단순화한다. | P1 | user Q15 (1 가, 2~4 및 버전 방침 수용) | resolved | R#: chromux 프로바이더 계약 / 선행 D-21·D-22·D-53 대체 |
| D-32 | fact | 외부의존 | chromux는 사용자 본인이 만든 도구다(modakbul-gongbang/chromux, MIT). 따라서 통제 불가능한 서드파티 프로바이더가 아니며, herdr-ide가 필요로 하는 계약이 없거나 맞지 않으면 herdr-ide 쪽에서 우회 구현을 하지 말고 chromux를 고치는 것이 우선 경로다. 이는 D-31의 버전 게이팅을 단순화해도 되는 근거이기도 하다. herdrm(라이선스 없음, D-24)이나 CodeEditSourceEditor(4개월 정체, D-28) 같은 외부 의존과 성격이 다르므로 리스크 등급을 낮게 둔다. | P1 | user Q15 '참고로 chromux 내가 만든거라 내가 걍 고치면 되니까' | resolved | R#: 프로바이더 리스크 완화 / D-31 보완 |
| D-33 | decision | UX/design | UI 품질의 사람 판정 인수 기준 6항목을 확정한다. (1) 구조: 3열 유지(사이드바+pet / 터미널 / 워크벤치), 밀도는 docs/design-reference/orca-01 기준선 (2) 텍스트: 시스템 폰트에 크기 위계가 보이고 터미널만 등폭이며 한글이 깨지지 않음 (3) 상태 표현: 색과 구조로 드러나고 문장 설명은 최후 수단(design 원칙 7) (4) 다크모드: 시스템 설정을 따라감 (5) 리사이즈: 창 크기를 바꿔도 레이아웃이 유지되고 패널 경계를 드래그로 조절 가능 (6) 네이티브 관용구: 진짜 메뉴바, 진짜 우클릭 메뉴, 진짜 스크롤바를 쓰고 그려낸 흉내를 쓰지 않음. 판정 시점은 두 번이다: 3a(창+레이아웃 완성) 시점에 방향 확인, v1 종료 시점에 최종. 판정 방법은 스크린샷을 사용자가 보고 판정하며, 구조는 peekaboo see로 요소가 제대로 잡히는지 함께 본다. 시각 방침은 macOS 네이티브 관용구를 우선하되(사용자 선택 '나') orca-01을 밀도와 배치의 시각 기준선으로 함께 유지한다(사용자 지시). 선행 D-59를 이 형태로 갱신한다. | P1 | user Q16 | resolved | V#: UI 품질 인수 기준 / 사람 판정 항목 |
| D-34 | decision | UX/design | 사이드바 구성은 herdr-agent-context-labels(yansfil, 사용자 본인 제작) 플러그인의 레이아웃을 herdr-ide 네이티브로 적용한다. 확인된 구성: 항목당 2줄이며 1줄은 상태 심볼 + workspace 라벨 + 에이전트 종류, 2줄은 최대 30자 작업 요약 + 경과 시간(12s/4m/2h/3d 형식)이다. 상태는 question, approval, error, working, unseen completion, idle, unknown을 컴팩트 심볼로 표시하고 상태당 심볼 하나를 고정하며, working과 unseen completion은 깜빡임이 아니라 색으로 구분한다. 정렬은 플러그인이 발행하는 sort_rank와 activity 토큰을 따라 막고 있는 순서대로(에러, 질문/승인, 완료 미확인, 진행 중, 확인됨) 두고 동률은 activity 시계로 깬다. 이는 선행 D-40(config.toml [ui.sidebar.agents] rows를 읽어 렌더)과 D-54(기본 레이아웃 내장)를 구체화한 것이다. | P1 | user Q16 'herdr-label 플러그인 그거의 구성도 잘 적용해서 함께 넣어주고' | resolved | R#: 사이드바 구성 / 선행 D-40·D-54 구체화 |
| D-35 | decision | 데이터/상태 | OpenRouter API 키는 v1에서 기존 방식을 유지한다. 키는 ~/.zshrc의 OPENROUTER_API_KEY 환경변수에 남고, herdr-agent-context-labels 플러그인이 std::env::var로 읽는 현재 경로를 바꾸지 않는다. herdr-ide는 키 유무만 확인해 없을 때 안내를 표시한다. 조용히 빈 요약 줄을 두지 않는다(원칙 4). Keychain 입력 UI는 v1 비목표다. 근거는 실제 OpenRouter 호출 주체가 herdr-ide가 아니라 플러그인이고(선행 D-40의 구조: 플러그인이 토큰을 발행하고 herdr-ide는 session.snapshot에서 읽어 렌더), 플러그인이 herdr의 startup hook으로 실행되어 herdr-ide가 env를 주입할 지점이 없기 때문이다. 나중에 Keychain으로 옮기려면 플러그인이 Keychain 우선 env 폴백으로 읽도록 고쳐야 하며 사용자가 플러그인 소유자라 가능하다. revisit trigger: 사용자가 셸 프로필에서 키를 빼고 싶어질 때. | P1 | user Q17 '우선 다로 하자. 키가 없으면 안내만. Keychain은 나중에' | resolved | R#: OpenRouter 키 / 비목표(v1): Keychain 입력 UI |
| D-36 | decision | 아키텍처 | src/openrouter.rs 541줄과 security-framework 의존을 삭제한다. 사용자가 삭제를 선택했다(선택지 나). 현재도 lib.rs가 모듈 선언만 하고 어디서도 호출하지 않는 미사용 코드이며, D-35로 v1에서 Keychain을 쓰지 않기로 했다. 나중에 Keychain 작업을 할 때는 D-22의 rust-native-final 태그에서 꺼낸다. | P2 | user Q19 '1. 나' | resolved | D-20 폐기 범위에 포함 |
| D-37 | decision | 검증 | 검증 실행 환경을 둘로 나눈다. CI는 권한이 필요 없는 것만 돌린다: Rust 코어 cargo test, swiftc 빌드 통과, .app 번들 조립 성공. 로컬(사용자 맥)에서만 돌리는 것: peekaboo 기반 UX 시나리오 6개, 한글 IME 확인, 메모리 400MB 측정(D-14), 디자인 사람 판정(D-33). 근거는 peekaboo가 Screen Recording과 Accessibility 권한을 요구하는데 CI에는 그 권한도 로그인한 데스크톱 세션도 없기 때문이며, self-hosted runner에 권한을 상시 부여하는 비용이 얻는 것보다 크다(원칙 12). mini를 러너로 쓰는 안은 그 머신이 프로덕션 자동화를 돌리고 있어 기각한다. 수용한 비용: UI 회귀는 사용자가 로컬에서 돌려야 잡힌다. | P1 | user Q18 '가' | resolved | V#: 검증 실행 환경 / 선행 D-34 보완 |
| D-38 | fact | 데이터/상태 | herdr-ide가 디스크에 남기는 영속 상태는 ~/Library/Application Support/Herdr IDE/navigator.json(1434B, schema_version 1, 트리 펼침·선택 상태) 하나뿐이다. shortcuts.json은 코드에 경로만 있고 파일이 없으며, pet.json은 테스트의 temp_dir 경로다. 진짜 사용자 상태(workspace, tab, pane, 세션)는 herdr가 소유하고 herdr-ide는 소켓으로 읽기만 하므로 이번 재작성으로 잃는 사용자 데이터가 없다. 별도로 ~/Library/Application Support/herdr-ide/(소문자)에 Electron 시절 잔재 5.5MB가 남아 있었고(Cache, GPUCache, Local Storage, Session Storage, Partitions, Trust Tokens, blob_storage, browser-profiles.json, pet-position.json, 전부 2026-08-26 생성), 사용자 지시로 삭제했다. 삭제 전 scratchpad/electron-leftovers-backup.tgz(51K)로 백업했다. | P1 | 로컬 실측 2026-08-28 + user Q19 'Electron 잔재들 다 지워버려' | resolved | R#: 마이그레이션 범위 = 없음 |
| D-39 | decision | 데이터/상태 | navigator.json을 이행하지 않고 버린다. SwiftUI 셸은 기존 Rust 포맷을 읽지 않고 같은 목적의 상태(트리 펼침·선택)를 자체 포맷으로 새로 저장한다. 첫 실행은 모든 노드가 접힌 기본 상태에서 시작하며, 사용자는 다시 펼치면 된다. 잃는 것이 UI 편의값뿐이라 백업이나 롤백 경로를 두지 않는다. 상태 파일 읽기가 실패하거나 손상된 경우에도 앱을 막지 않고 메모리 기본값으로 진행하며 그 사실을 로그로 남긴다(원칙 4). workspace, tab, pane, 세션 같은 실제 사용자 상태는 herdr가 소유하므로 이 결정으로 잃는 사용자 데이터는 없다(D-38). | P2 | user Q21 '버리고 새로 시작 ㅇㅇㅇ' | resolved | R#: 첫 실행·영속 상태 계약 |
| D-40 | decision | UX/design | 브라우저는 herdr-ide가 열어주기만 한다. 버튼을 누르면 chromux default 프로필의 Chrome이 뜨거나(없으면 launch, 있으면 재사용) 앞으로 나오고, URL 입력은 Chrome 안에서 사용자가 한다. herdr-ide에 URL 입력창을 두지 않는다. 포커스된 pane의 cwd에서 dev 서버 포트를 추측해 여는 방식도 v1 비목표다(herdr official.browser 플러그인에 open-localhost 액션이 이미 있으므로 필요하면 그쪽을 쓴다). herdr-ide는 현재 열린 탭의 URL과 제목만 /json/list로 읽어 표시한다. 탭이 없으면 '열린 탭 없음'을 표시한다. | P1 | user Q19 '3. 가' | resolved | R#: 브라우저 열기 흐름 / 비목표: 앱 내 URL 입력 |
| D-41 | decision | 검증 | 원격(mini) workspace 지원을 v1 릴리스 게이트에 포함한다. 사용자가 mini에 herdr가 이미 실행 중이므로 실제 검증이 가능하다고 판단했다. 선행 인터뷰 UX-04의 '머신 가용성에 따라 차단 가능한 항목'을 격상한다. 검증 항목: 원격 workspace가 사이드바에 표시됨, 원격 pane attach와 분할 동작, 원격 파일 탐색과 뷰어 표시, 인라인 편집 비활성 사유 표시, SSH 연결 끊김 시 조용한 빈 목록이 아니라 사유 표시와 재연결 시도. mini가 응답하지 않으면 v1을 완료로 판정하지 않는다. | P1 | user Q19 '4. 나 v1에 넣어.. 이미 herdr remote 켜져있어서 그거 쓰면 되잖아' | resolved | V#: 원격 v1 릴리스 게이트 / UX-04 |
| D-42 | decision | 의존성 | 마크다운 렌더러는 swift-markdown-ui(gonzalezreal, MIT, star 3921) 하나로 확정한다. swiftlang/swift-markdown(Apache-2.0)은 파서라 렌더링을 자체 구현해야 하므로 기각한다. D-28의 병기 상태를 해소한다. | P1 | user Q19 '5. 가' | resolved | R#: Markdown 패키지 / D-28 확정 |
| D-43 | decision | UX/design | herdr-ide가 chromux Chrome 창의 위치·크기를 자동 배치해 한 화면처럼 보이게 하는 것은 v1 비목표이며, 재검토 트리거는 'v1 실사용에서 창 두 개가 실제로 불편하다고 사용자가 느낄 때'다. D-27의 v2 후보 기록에 이 트리거를 부여한다. owner: 사용자. | P2 | user Q19 '6. 가' | resolved | v2 후보 / revisit trigger 기록 |
| D-44 | fact | 운영 | Xcode 없이 SwiftPM으로 외부 Swift 의존성을 해석하고 빌드할 수 있음을 실측했다. Package.swift(swift-tools-version 5.10, platforms macOS v14)에 .package(url: migueldeicaza/SwiftTerm, exact: 1.20.0)을 선언하고 swift build를 실행해 SwiftTerm 전체 컴파일과 링크가 통과했다(Build complete! 81.60s, .build/arm64-apple-macosx/debug 산출). 따라서 빌드 구조는 XcodeGen과 xcodebuild가 아니라 SwiftPM Package.swift + Package.resolved(lockfile)이며, 의존성 버전은 exact로 고정한다. Rust는 staticlib으로 빌드해 linkerSettings의 unsafeFlags 또는 별도 링크 단계로 결합하고, 산출 실행파일을 D-29의 .app 조립 스크립트에 넣는다. Rust staticlib과의 실제 링크는 아직 검증하지 않았으며 D-18 스파이크 1번 항목에 포함된다. | P0 | 로컬 실측 2026-08-28: swift build로 SwiftTerm 1.20.0 해석 및 빌드 성공 | resolved | R#: Swift 빌드 구조 / 스파이크 리스크 해소 |
| D-45 | decision | 아키텍처 | Rust 코어와 Swift 셸 사이 FFI 계약을 이 초안으로 확정한다. 함수 6개: herdr_core_create(options_json) -> Core*, herdr_core_dispatch(Core*, event_json_bytes, len), herdr_core_snapshot(Core*) -> Bytes, herdr_core_on_change(Core*, cb, ctx), herdr_core_free_bytes(Bytes), herdr_core_destroy(Core*). 데이터는 양방향 모두 JSON UTF-8 바이트이고 스냅샷은 render.rs FrameModel의 계보를 잇는다(D-02). 소유권: Rust가 Core와 모든 버퍼를 소유하며 Swift는 받은 Bytes를 herdr_core_free_bytes로 반드시 반환한다. 스레드: create/dispatch/snapshot/destroy는 메인 스레드 전용이다. on_change 콜백은 임의 스레드에서 발생할 수 있고 데이터를 싣지 않으며 변경 사실만 알린다. Swift는 콜백 수신 후 메인 스레드로 홉해 snapshot을 당겨간다. 수명: ctx는 Core보다 오래 살아야 하고 destroy는 콜백을 해제한 뒤 해제한다. 오류: dispatch는 패닉을 FFI 경계 밖으로 넘기지 않고 오류를 다음 스냅샷의 오류 필드로 표면화하며(원칙 4) create는 실패 시 널을 반환한다. 스파이크(D-18 1번 항목)는 이 계약을 설계하는 것이 아니라 검증한다. 합격 기준: 원격 SSH 이벤트가 Rust 스레드에서 발생했을 때 Swift가 갱신된 스냅샷을 그리고, 100회 반복에 크래시와 누수가 없다. 불합격이면 계약을 재검토한다. | P1 | user Q21 '나. 이 초안으로 확정해' | resolved | R#: FFI 계약 / V1 합격 기준 |
| D-46 | decision | 아키텍처 | 레거시 코드와 상태의 폐기를 광범위하게 승인받았다. D-20의 폐기 목록(app.rs의 AppKit/wgpu 부분, render.rs, terminal.rs, accessibility.rs, browser.rs, openrouter.rs와 wgpu/glyphon/bytemuck/raw-window-handle/objc2/alacritty_terminal/security-framework 의존)에 더해, 새 셸이 기존 포맷·구조를 계승해야 한다는 제약을 두지 않는다. 원칙 1(하위 호환 유지 안 함)을 이 전환 전체에 적용한다. 다만 실행 순서는 D-22를 유지한다: 0단계 스파이크 4항목을 통과한 뒤에 삭제하고, 삭제 직전 커밋에 rust-native-final 태그를 남긴다. 이는 되돌리기 지점을 위한 것이지 하위 호환을 위한 것이 아니다. | P1 | user Q21 '레거시 다 지워도 돼 최대한 새로 하려면 새로 해도 되니까' | resolved | R#: 폐기 방침 / D-20·D-22 보강 |
| D-47 | decision | 검증 | 실패 경로 검증은 실제 서비스를 대상으로 해도 된다. 에이전트가 격리(전용 chromux 프로필, 가짜 herdr 소켓, SSH 터널만 끊기)를 권고했으나 사용자가 실제 서비스 사용을 명시적으로 승인했다. 대상에 포함되는 것: chromux default 프로필, 실행 중인 herdr 소켓, mini의 원격 herdr. 사용자에게 고지한 결과: 검증 실행 중 실제 작업이 중단될 수 있고, mini는 프로덕션 자동화가 도는 머신이며, default 프로필에는 사용자 로그인 세션이 있다. 사용자가 이를 알고 수용했다. 단 선행 D-56(파괴적 workspace/worktree 조작은 herdr-ide-verify- 접두어 fixture에서만 실행)은 별개 결정이며 그대로 유효하다. 이번 결정은 서비스 중단 형태의 장애 주입에 한하고, 사용자의 workspace나 worktree를 지우는 조작에는 적용되지 않는다. | P0 | user Q22 '실제 서비스 건드려도 돼' | resolved | V#: 실패 주입 격리 정책 / 사용자 수용 리스크 |
| D-48 | fact | 데이터/상태 | Electron 잔재 백업(scratchpad/electron-leftovers-backup.tgz, 51K)을 사용자 지시로 삭제했다. Electron 시절 데이터는 시스템에 남아 있지 않다. 보존 정책 없음. | P1 | user Q22 '백업 지워' + 실행 확인 | resolved | D-38 종결 |
| D-49 | decision | 보안/운영 | herdr-ide는 OPENROUTER_API_KEY의 존재를 감지하지 않는다. 근거는 Finder로 실행한 앱이 launchd 환경을 받아 ~/.zshrc를 읽지 않으므로, 터미널 실행과 Finder 실행에서 결과가 달라지는 비결정적 상태가 되기 때문이다. 대신 요약 줄이 비어 있다는 사실만 표시하고 플러그인 설정 확인 안내를 단다. herdr-ide는 키 값을 읽지도 저장하지도 로그에 남기지도 않는다. 사용자가 프로젝트 루트 .env(94B, 미추적, 과거 커밋 이력 없음)에도 키를 두었다고 알렸으나 herdr-ide는 이를 읽지 않는다. 확인된 위생 문제: .env가 .gitignore에 없어 git add . 시 커밋될 수 있다. remote가 아직 없어 유출은 발생하지 않았다. 수정 여부는 사용자 승인 대기. revisit trigger: 사용자가 키 상태 표시를 요구할 때. | P1 | user Q22 '우선 키 감지하지 않는거로만 해줘 나중에 고칠게' | resolved | R#: OpenRouter 신호 / D-35 보완 / revisit: 나중에 |
| D-50 | decision | 아키텍처 | D-45의 FFI 함수 계약에 데이터 스키마를 붙인다. 모든 페이로드는 최상위에 schema_version 정수를 갖는다. options_json: herdr 소켓 경로, 원격 타겟 목록, 상태 파일 경로. event: {schema_version, kind, payload} 형태이며 kind는 최소한 key, click, focus_pane, open_browser, create/close(workspace/tab/pane), file_open, file_save, retry_connect를 포함한다. snapshot: FrameModel 계보(navigator, overlay, tab, connection, zoomed, focused, editor)에 더해 status 객체를 신설한다. status는 herdr 연결 상태, 원격 연결 상태, chromux 가용성, 마지막 오류(kind, message, retryable, occurred_at)를 담는다. render.rs의 현 FrameModel에는 오류 필드가 없으므로 이번에 신설하는 것이다(원칙 4, 원칙 10). 알 수 없는 event kind는 무시하지 않고 status의 마지막 오류로 표면화한다. 스키마 버전이 맞지 않으면 조용히 진행하지 않고 명확히 실패한다. | P1 | user Q22의 FFI 확정(D-45)에 딸린 스키마 구체화 | resolved | R#: FFI 데이터 계약 / V1·V1b 합격 기준 |
| D-51 | decision | 검증 | chromux default 프로필 삭제는 검증에서 금지한다. 프로필 부재 분기(V13)는 존재하지 않는 이름(herdr-ide-verify-absent)을 지정해 재현하며 어떤 프로필도 지우지 않는다. 근거는 D-47이 승인한 것이 서비스 중단 형태의 장애 주입이고 서비스는 재기동으로 복구되는 반면, default 프로필 삭제는 사용자의 Chrome 로그인 세션을 복구 불가능하게 잃게 하기 때문이다. chromux가 없는 프로필을 스스로 생성하지 않는다는 것은 D-12에서 실측으로 확인했으므로 같은 분기가 파괴 없이 재현된다. | P0 | 게이트 cycle3 지적 반영, D-47의 사용자 승인 범위 내 | resolved | V13 / D-47 경계 명확화 |
| D-52 | decision | 보안/운영 | 환경변수 계약을 축소된 형태로 확정한다. herdr-core가 읽는 키를 한 곳에 열거 가능한 형태로 등록하되, 부팅 실패는 두지 않는다. 실측된 키의 성격: HOME은 사실상 항상 존재, HERDR_SOCKET_PATH는 선택(~/.config/herdr 기본 경로 존재), SSH_AUTH_SOCK은 선택이지만 원격 SSH 인증에 필요, TERM/LANG/LC_*는 PTY 전달용이다. 즉 없으면 앱이 뜰 수 없는 필수 키가 없으므로 부팅 실패는 과하다. 대신 SSH_AUTH_SOCK이 없으면 원격 기능만 사유와 함께 비활성화한다. 근거는 원격이 v1 릴리스 게이트(D-41)인데 Finder로 실행한 앱이 셸 환경을 상속하지 않아 이 키가 없을 수 있고, 조용히 실패하면 완료 판정이 막히기 때문이다(원칙 4, 원칙 10). 에이전트가 앞서 이를 부팅 실패 + 폴백 금지 정책으로 제안했으나 사용자가 축소안을 선택했다. | P1 | user (PRD 리뷰 중) '가' - 환경변수 계약 축소안 선택 | resolved | R#: 환경변수 레지스트리 / RISK: Finder 실행 시 원격 인증 |

## Raw Q&A

### Q1: 인터뷰 로그를 만들고 사전 조사 사실 7개를 등록했습니다: `agents/interview/swift-shell-pivot/qa-
- decision_ids: D-08
- route: user-decision
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
- decision_ids: D-09
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
- decision_ids: D-09
- route: research
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
- decision_ids: D-09, D-10
- route: research
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
- decision_ids: D-11
- route: user-decision
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
- decision_ids: D-14
- route: user-decision
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

### Q7: `peekaboo 4.2.2` 설치되어 있습니다 (어제 설치하셨네요). 이제 질문드립니다.
- decision_ids: D-17
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:fa6e3648-1929-4cbe-ad8c-7cf34b813000
- asked: `peekaboo 4.2.2` 설치되어 있습니다 (어제 설치하셨네요). 이제 질문드립니다.
  
  ---
  
  ## 구멍: 앱을 에이전트가 검증할 방법이 사라졌습니다
  
  선행 인터뷰에서 이렇게 못박아 두셨습니다.
  
  **D-34 (v1 주 검증 모드):**
  > herdr-ide를 `--remote-debugging-port`로 띄우고 **chromux가 그 CDP에 붙어 UI를 조작**한 뒤, 결과는 herdr 소켓 API로 확인한다. UI 배선 유닛 테스트는 쓰지 않는다.
  
  **D-35 (제품 요구):**
  > 에이전트가 검증할 수 있는 것은 최대한 에이전트가 검증하도록 만드는 것이 **설계 요구사항이다. 검증 전략이 아니라 제품 요구로 취급한다.** (1) herdr-ide는 자기 CDP 엔드포인트를 열 수 있어야 한다 ...
  
  **방금 A-1으로 CDP를 없앴습니다.** SwiftUI 네이티브 앱에는 `--remote-debugging-port`가 없습니다. **D-34의 주 검증 모드가 통째로 무효가 됐습니다.**
  
  D-35의 요구는 남아 있습니다. 그러니 대체 경로를 정해야 합니다.
  
  ---
  
  ### Q7. 앱을 어떻게 검증할까요?
  
  **(1) peekaboo로 대체한다**
  `peekaboo see --app herdr-ide --json` → UI 요소 맵과 ID를 받고, `click` / `type` / `press`로 조작합니다. macOS 접근성 트리 기반이에요.
  - 잘 맞는 이유: **SwiftUI가 접근성을 거의 공짜로 줍니다.** (그래서 `accessibility.rs` 165줄이 삭제되는 거고요.) 그리고 선생님 글로벌 규칙에 이미 macOS 자동화 표준으로 박혀 있습니다.
  - 비용: Screen Recording + Accessibility 권한이 필요합니다. 선생님 맥에선 이미 되지만, **D-29(공개 배포)로 남이 쓸 때와 CI에서는 마찰**입니다.
  
  **(2) herdr-ide가 자체 검증 소켓을 연다**
  우리가 명령/조회용 소켓을 직접 만듭니다. 확실하지만 우리 코드가 늘고, "테스트용 뒷문"이 제품에 생깁니다.
  
  **(3) 둘을 나눈다** ← 제 추천
  - **조작은 peekaboo.** 사람이 실제로 클릭하는 경로 그대로라 가장 정직한 검증입니다.
  - **상태 확인은 스냅샷을 밖으로 내보낸다.** 그런데 이게 **거의 공짜입니다.** SwiftUI에 넘기려고 어차피 만드는 그 스냅샷(`FrameModel` → JSON)을 파일이나 소켓으로 한 벌 더 내보내면 끝이에요.
  
  ```
  Swift가 그리려고 받는 것  ─┐
                            ├─  같은 JSON 하나
  검증이 읽는 것            ─┘
  ```
  
  **하나를 만들어서 두 군데 쓰는 겁니다.** 원칙 10(모든 실패는 프로세스 밖에서 관측 가능해야 한다)을 거의 비용 없이 만족합니다. 그리고 D-35가 원했던 "UI가 표현하는 상태를 앱 내부에만 두지 마라"가 바로 이겁니다.
  
  - 결과 확인은 기존대로 herdr 소켓 API도 같이 씁니다 (pane 생겼나 등). 이건 A-1과 무관하게 그대로 살아 있습니다.
  
  ---
  
  **한 가지 더 짚어야 할 게 있습니다.**
  
  이번 전환의 **가장 큰 동기가 한글 IME**인데, **조합 입력을 자동으로 검증할 수 있는지는 제가 확인 못 했습니다.** `peekaboo type`이 IME를 거쳐 들어가는지, 아니면 완성된 문자를 바로 주입하는지에 따라 달라집니다. 후자면 "ㅎ+ㅏ+ㄴ → 한" 조합 과정을 자동으로 못 재현합니다.
  
  즉 **한글 조합은 사람 판정 항목으로 남을 가능성이 있습니다.** D-34가 "골든 시나리오는 사람 판정으로 기록하되 완료 게이트로 쓰지 않는다"고 했는데, IME는 이번 전환의 존재 이유라 **여기서만은 사람 판정을 완료 게이트로 올려야 할 수도** 있습니다.
  
  ---
  
  **(1) / (2) / (3) 중 어느 쪽인가요?** 그리고 **한글 IME를 사람 판정 완료 게이트로 둘지**도 같이 정해주시면 좋겠습니다.
- recommended: none
- answer: 1
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q8: `peekaboo type`은 `--app`으로 프로세스에 키 이벤트를 넣는 방식입니다. **IME를 거치는지는 명시되어 있지 않
- decision_ids: D-16, D-19, D-20
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:d08c241a-cc84-412a-98cd-c4984001c431
- asked: `peekaboo type`은 `--app`으로 프로세스에 키 이벤트를 넣는 방식입니다. **IME를 거치는지는 명시되어 있지 않아 확인 못 했습니다.**
  
  다만 방법은 있습니다. 한글 IME를 켠 상태에서 `peekaboo type "gks"`를 보내면, 두벌식으로 ㅎ+ㅏ+ㄴ이라 **정상 경로면 "한"이 나옵니다.** 나오면 자동 검증 가능, 안 나오면 IME를 우회하는 겁니다. **0단계 스파이크에서 10분이면 판정됩니다.**
  
  ---
  
  ### Q8. 한글 조합 입력을 "사람이 직접 확인해야 완료"로 둘까요?
  
  이번 전환의 존재 이유가 한글 IME인데, 자동 검증이 될지 지금은 모릅니다.
  
  **(가) 사람 판정을 완료 게이트로 둔다** ← 추천
  자동 검증이 되든 안 되든, **한글 조합은 사람이 직접 쳐보고 통과시켜야 v1 완료.** 확인 항목은 4개면 충분합니다: 후보창이 커서를 따라오는가 / 조합 중 백스페이스가 조합을 지우는가 / 두 글자 이상 조합이 뒤 글자를 안 덮는가 / 터미널과 에디터 양쪽에서 되는가.
  
  - 근거: 선행 D-34가 "사람 판정은 완료 게이트로 쓰지 않는다"고 했지만, 그건 **골든 시나리오(하루 실사용)** 얘기였습니다. IME는 이 전환의 목적 그 자체라 성격이 다릅니다. 자동 검증이 안 되는 걸 이유로 게이트에서 빼면, **고치려던 문제를 확인 안 하고 끝내는** 게 됩니다.
  - 비용: 선생님이 직접 4개 항목을 쳐보셔야 합니다. 5분.
  
  **(나) 스파이크 결과를 보고 정한다**
  peekaboo가 IME를 거치면 자동 게이트, 아니면 사람 게이트.
  
  **(다) 자동 검증만 쓴다**
  안 되면 게이트에서 뺀다.
  
  ---
  
  **(가)/(나)/(다)?**
  
  그리고 이거 정하면 **브라우저 축은 완전히 닫히고**, 남은 P0가 셋입니다. 다음 순서로 여쭙겠습니다:
  
  1. **터미널 뷰** - SwiftTerm 채택할지 (제 추천은 채택)
  2. **기존 Rust 16040줄을 어떻게 할지** - 폐기 범위와 되돌리기 기준
  3. **이번이 5번째 런타임 전환인데, 무엇이 이번엔 다른지** - 중단 조건
- recommended: none
- answer: 나로 가자 
  
  1. 터미널 뷰는 SwiftTerm
  2. 기존 Rust는 삭제해도 돼
  3. ?
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q9: 3번이 무슨 질문이었는지
- decision_ids: D-18, D-22
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:ed24dafc-eb6c-411b-a109-fe131a781416
- asked: ## 3번이 무슨 질문이었는지
  
  **전환 이력을 보니 패턴이 하나 있습니다. 매번 그 결정을 죽일 요인을 시험해보지 않은 채 결정했습니다.**
  
  | | 무엇으로 정했나 | 무엇에 죽었나 | 결과 |
  |---|---|---|---|
  | 1차 Swift+libghostty (8/17) | PRD 승인까지 감 | **Xcode 전체 설치** 장벽을 승인 후에 만남 | **소스 0줄** |
  | 2차 Rust/Tauri (8/25) | 인터뷰 중 | CEF 경로 없음 | 같은 인터뷰에서 기각 |
  | 3차 Electron (8/25) | **질문 44개 인터뷰** | 메모리. orca를 무거워서 버린 자기 실증과 충돌 | **코드 전에 폐기** |
  | 4차 Rust 네이티브 (8/26) | 실제로 만듦 (16040줄) | UI 품질 + 한글 IME | **오늘 폐기 결정** |
  
  **매번 "만들어보거나 부딪혀보고" 나서야 죽일 요인을 발견했습니다.** 3번 질문은 이거였습니다: *이번엔 그걸 먼저 만나게 할 방법이 있는가.*
  
  ---
  
  ### 그래서 방금 하나를 미리 죽여봤습니다
  
  **1차를 죽인 게 Xcode 전체 설치였는데, Swift로 돌아가면 그게 다시 옵니다.** 확인했습니다.
  
  ```
  $ xcode-select -p       → /Library/Developer/CommandLineTools   (Xcode.app 없음)
  $ xcodebuild -version   → error: requires Xcode
  $ xcodegen              → not found
  ```
  
  **여전히 없습니다.** herdrm이 쓰는 XcodeGen + xcodebuild 방식은 못 씁니다.
  
  그런데 SDK를 열어보니 SwiftUI가 들어 있길래, 실제로 빌드해봤습니다.
  
  ```
  $ ls $(xcrun --show-sdk-path)/System/Library/Frameworks/ | grep SwiftUI
  SwiftUI.framework
  SwiftUICore.framework
  
  $ swiftc -parse-as-library -o probe main.swift    # SwiftUI 앱 15줄
  BUILD_OK
  probe: Mach-O 64-bit executable arm64
  ```
  
  **Xcode 없이 SwiftUI 앱이 컴파일됩니다.** 1차를 죽였던 장벽은 이번엔 없습니다.
  
  ---
  
  ### 남은 킬러 후보 4개
  
  Xcode는 치웠고, 아직 안 죽여본 게 넷 있습니다.
  
  1. **FFI 비용** - Rust staticlib ↔ Swift, 특히 SSH 이벤트를 Swift로 올리는 비동기 콜백
  2. **SwiftTerm 결합** - Rust가 뽑은 SSH 바이트를 `feed()`에 넣고 델리게이트로 되받는 왕복
  3. **한글 IME** - 우리 케이스(원격 SSH + 에이전트 TUI)에서 실제로 되는가
  4. **배포** - `xcodebuild` 없이 `.app` 번들 조립, 코드사인, 공증, Sparkle **← 미확인. 1차를 죽인 것과 같은 종류입니다.**
  
  ---
  
  ### Q9. 이렇게 할까요?
  
  **(가) 0단계 스파이크에서 4개를 전부 시험한다.** ← 추천
  2~3일. 하나라도 실패하면 그 자리에서 멈추고 재검토합니다. 코드를 지우기 전에요.
  
  **그리고 기존 Rust는 스파이크 통과 후에 지웁니다.** 지금 지우면 실패했을 때 돌아갈 곳이 없습니다. 방법은 간단합니다 - 지우기 직전 커밋에 태그 하나 (`rust-native-final`). 나중에 필요하면 거기서 꺼냅니다.
  
  **(나) 1·2·3만 시험하고 배포(4)는 나중에.** 스파이크가 하루로 줄지만, 1차를 죽인 것과 같은 종류의 위험을 뒤로 미룹니다.
  
  **(다) 스파이크 없이 바로 간다.** 이미 충분히 봤다고 보고 3단계로 직행.
  
  ---
  
  제가 **(가)** 를 미는 이유는 하나입니다. **지난 4번 중 3번이 "결정 후에 죽일 요인을 만나서" 죽었습니다.** 2~3일 스파이크는 그 패턴을 깨는 가장 싼 방법이고, 특히 4번(배포)은 이미 한 번 이 프로젝트를 죽인 적이 있습니다.
  
  **(가)/(나)/(다)?**
- recommended: none
- answer: ㅇㅇㅇ 가로 가자
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q10: 체크포인트 2 기록. **결정이 다 모였습니다.** 마지막으로 한 문장 확인만 받겠습니다.
- decision_ids: none
- route: mixed
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:4a044aca-2ad8-49a3-9f82-2b327752a75b
- asked: 체크포인트 2 기록. **결정이 다 모였습니다.** 마지막으로 한 문장 확인만 받겠습니다.
  
  ---
  
  ## 합의된 목표 (한 문장)
  
  > **herdr-ide의 macOS 셸을 SwiftUI + SwiftTerm으로 교체하고 SSH·herdr·도메인 로직은 Rust 코어로 남겨, 한글 IME와 UI 품질을 AppKit이 해결하게 한다. 브라우저는 앱 밖 chromux Chrome에 맡기고, 전환은 2~3일 스파이크 4항목을 통과한 뒤에만 확정한다.**
  
  ## 확정된 것
  
  | | 결정 |
  |---|---|
  | **런타임** | SwiftUI(Xcode 없이 `swiftc`로 빌드 가능 확인) + Rust 코어 |
  | **터미널** | SwiftTerm 1.19+. 엔진·뷰 모두 위임. `feed()` / `send()` 2개 함수로 Rust SSH와 연결 |
  | **브라우저** | herdr-ide는 안 만듦. chromux CLI 위임 + `/json/list` 읽기. `browser.rs` 721줄 삭제 |
  | **grab** | v1 비목표 |
  | **검증** | peekaboo (`see` / `click` / `type`). chromux-CDP 방식 폐기 |
  | **한글 IME 게이트** | 스파이크에서 `peekaboo type "gks"` → "한" 나오는지 보고 결정 |
  | **메모리** | herdr-ide 프로세스만 400MB 이하 (Chrome 제외) |
  | **폐기** | `render.rs` · `terminal.rs` · `accessibility.rs` · `browser.rs` · `app.rs` 플랫폼부 + wgpu/glyphon/objc2/alacritty 의존 |
  | **유지** | `remote.rs`(3550) · `herdr.rs`(1154) · files/commands/domain/navigator/layout/presentation |
  | **되돌리기** | 스파이크 통과 후 삭제. 직전 커밋에 `rust-native-final` 태그 |
  
  ## 무효화된 선행 결정
  
  `D-11`(브라우저 패널+grab) · `D-19`(CDP 노출) · `D-34`(chromux가 앱 CDP에 붙어 검증) · `D-41`(400/900MB) · `D-53`(chromux 전제조건) → 전부 이번 인터뷰가 대체
  
  ---
  
  ### 마지막 확인
  
  위 한 문장을 다른 에이전트에게 주면 의도한 걸 만들 수 있어야 합니다. **이대로 확정할까요?**
  
  확정하시면 gap-audit 게이트를 돌리고, 통과하면 `/gen-prd`로 넘어갑니다.
- recommended: none
- answer: 나근데 궁금한게 Swift app에서 터미널은 저게 최선인거야? herdrm 프로젝트 봣을 때 쟨 뭐로햆어?
- immediate_notes: 폐쇄 확인 문장을 제시했으나 사용자가 확정 대신 SwiftTerm 질문으로 응답했다. D-15 확정 미수령.
- needs_normalization: false

### Q11: **herdrm은 SwiftTerm입니다.** `project.yml`에 이렇게 박혀 있습니다:
- decision_ids: D-23, D-24
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:6a239340-7c00-4220-89a3-e09875f7c35e
- asked: **herdrm은 SwiftTerm입니다.** `project.yml`에 이렇게 박혀 있습니다:
  
  ```yaml
  SwiftTerm:
    url: https://github.com/migueldeicaza/SwiftTerm
    # 1.19 ships the marked-text overlay used for IME preedit; do not reimplement it.
    from: "1.19.0"
  ```
  
  "최선인가"는 따로 봤습니다. **실질적인 후보가 셋뿐인데, 이 프로젝트가 이미 한 번 다른 걸 골랐다가 접었습니다.**
  
  ---
  
  ## 후보 3개
  
  **1. SwiftTerm** - MIT, ⭐1671, **어제 커밋**(8/27)
  ```
  v1.18.0  2026-08-09
  v1.19.0  2026-08-18
  v1.20.0  2026-08-18
  ```
  이번 달에만 릴리스 3개. 활발합니다. IME marked-text 오버레이 내장. herdrm이 실사용 중.
  
  **2. libghostty** - ⭐60364짜리 Ghostty의 임베딩 라이브러리
  
  **이게 1차 전환 때 고른 것이었습니다.** 1차 인터뷰 D-13이 "Swift 앱 + libghostty 터미널 서피스"로 확정했고, D-14가 "libghostty 비안정 대응: 버전 고정 + 임베딩 스파이크 최우선"을 최상위 리스크로 걸었습니다.
  
  **지금도 그대로입니다.** 방금 Ghostty 공식 문서를 다시 확인했습니다:
  
  > The libghostty API is currently used primarily by the macOS app and **is not yet stabilized for general-purpose embedding**. The API **may change significantly between releases.**
  
  - 서드파티 임베딩용 **안정 C API가 아직 없습니다**
  - 유일한 검증된 소비자가 **Ghostty 자기 macOS 앱**
  - 안정화 근접한 건 `libghostty-vt`(파서)뿐 - 우리가 필요한 건 파서가 아니라 뷰입니다
  
  **3. NSTextView + alacritty_terminal 직접** - 앞에서 검토했다가 접은 안
  IME는 AppKit이 공짜로 주지만, 셀 렌더링·스크롤백·선택·링크·리사이즈를 우리가 다 만듭니다. **지금 `render.rs` 921줄로 하고 있는 그 일**이고, 벗어나려던 게 그거였습니다.
  
  ---
  
  ## 결론: 네, 현재로선 SwiftTerm이 최선입니다
  
  이유는 SwiftTerm이 완벽해서가 아니라, **기술적으로 더 나을 수 있는 libghostty가 공식적으로 "임베딩용으로 안정화 안 됨"이라고 못박고 있고, 이 프로젝트가 이미 그 리스크에 한 번 걸렸기 때문**입니다.
  
  ### 다만 솔직한 주의사항 3개
  
  1. **사실상 개인 프로젝트입니다.** Miguel de Icaza 주도, open issue 75개. herdrm과 여러 앱이 쓰고 있어 방치 위험은 낮지만 팀 프로젝트는 아닙니다.
  2. **릴리스 속도가 빠릅니다.** 1.19와 1.20이 **같은 날**(8/18) 나왔습니다. 활발하다는 뜻이기도 하고 변동이 잦다는 뜻이기도 합니다. **1.19→1.20에 깨진 변경이 있는지는 확인 못 했습니다.**
  3. **버전을 고정해야 합니다.** 1차 인터뷰 D-24가 이미 "herdr 버전 고정" 방침을 세웠는데, SwiftTerm에도 같은 규칙을 적용하는 게 맞습니다. herdrm은 `from: "1.19.0"`(하한만)인데, 우리는 **정확한 버전 고정**을 권합니다. 지금 Cargo.toml이 전부 `=` 고정인 것과 같은 방침이고요.
  
  **그리고 한글 IME가 우리 케이스(원격 SSH + 에이전트 TUI)에서 진짜 되는지는 스파이크 3번 항목**입니다. SwiftTerm이 해준다는 건 herdrm의 로컬 환경 얘기고, 우리 조건에서 확인 전까지는 가정입니다.
  
  ---
  
  이대로 두면 될까요? 아니면 SwiftTerm 버전 고정 방침을 결정으로 추가할까요? (추가하는 쪽을 권합니다 - `1.20.0` 정확 고정)
- recommended: none
- answer: ㅇㅇ 최신 버전 기준으로 고정시키자. 그리고 나중에 한굴 이슈생기면 저 herdrm 코드베이스보고 고칠거 있으면 고치는 식으로 한번 해보자. 여기는 한글이 잘 되서!
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q12: 게이트가 BLOCK하면서 **제 기록 오류를 정확히 잡았습니다.** 정정했고, 확인이 필요한 게 셋 남았습니다.
- decision_ids: D-13, D-15, D-25, D-27
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:04b958dd-0c90-4996-8934-6ac1d85861b2
- asked: 게이트가 BLOCK하면서 **제 기록 오류를 정확히 잡았습니다.** 정정했고, 확인이 필요한 게 셋 남았습니다.
  
  ## 제가 잘못 기록한 것
  
  **D-13 (브라우저 제어 방식)** - 제가 "chromux CLI로만 위임하고 직접 CDP 조작은 금지"를 **결정으로 기록했는데, 선생님이 승인하신 적이 없습니다.**
  
  실제 흐름은 이랬습니다:
  - 선생님: *"chromux Chrome을 띄우게 하고 **cdp로 조절이 가능하게**?"* ← CDP 쪽을 요청하심
  - 제가: CLI 위임 + 읽기 전용을 권고
  - 선생님: *"ㅇㅇㅇ 그렇게 하자 **1로 ㄱㄱ**"* ← 이건 **메모리 선택지 (1)** 에 대한 답
  
  제가 "그렇게 하자"를 제 권고에 대한 동의로 읽었는데, 그 문장 바로 뒤에 "1로 ㄱㄱ"가 붙어 있으니 메모리 질문에 대한 답으로 보는 게 맞습니다. **open으로 되돌렸습니다.**
  
  ---
  
  ## 확인 3개
  
  ### ① 브라우저를 herdr-ide가 CDP로 직접 조작할까요?
  
  **(가) chromux CLI 위임 + 읽기 전용만** (제가 권고했던 것)
  `chromux launch/open/ps`로 행동, `/json/list`로 상태만 읽음. herdr-ide는 CDP로 조작 안 함.
  - 근거: chromux가 그 Chrome의 데몬·세션·직렬화·잠금을 소유합니다. 우리가 같은 포트로 명령하면 주인이 둘이 됩니다.
  
  **(나) herdr-ide도 CDP로 직접 조작**
  선생님이 처음 요청하신 방향. 더 많은 걸 할 수 있지만, 에이전트가 클릭하는 중에 herdr-ide가 페이지를 바꾸는 충돌 가능성이 실재합니다.
  
  여전히 **(가)** 를 권합니다. 다만 이번엔 선생님 결정으로 기록하겠습니다.
  
  ### ② 윈도우 - 이렇게 기록했는데 맞나요?
  
  선생님 표현이 *"우선 윈도우는 **당장** 안해도 돼"* 였습니다. 영구 기각이 아니라 **연기**로 기록했습니다 (`deferred`, revisit = 윈도우 요구 재발 시).
  
  중요한 이유가 있습니다. **SwiftTerm 채택(D-16)과 브라우저 외부화(D-11)가 둘 다 "윈도우 안 함"을 전제로 성립**합니다. 윈도우가 되살아나면 두 결정을 같이 다시 봐야 합니다. 이대로 괜찮으신가요?
  
  ### ③ 전환 전체 확정
  
  아까 제시한 한 문장입니다:
  
  > herdr-ide의 macOS 셸을 SwiftUI + SwiftTerm으로 교체하고 SSH·herdr·도메인 로직은 Rust 코어로 남겨, 한글 IME와 UI 품질을 AppKit이 해결하게 한다. 브라우저는 앱 밖 chromux Chrome에 맡기고, **전환은 2~3일 스파이크 4항목을 통과한 뒤에만 확정한다.**
  
  개별 결정(SwiftTerm, Rust 삭제, 스파이크 게이트, 버전 고정)은 다 받았는데 **전체 확정만 안 받았습니다.** 이대로 확정할까요?
  
  ---
  
  ## 나머지 게이트 지적 (P1 10개)
  
  게이트가 v1 워크플로, UX 상태 매트릭스, 데이터 마이그레이션 등도 지적했는데, **상당수는 선행 인터뷰(질문 44개)가 이미 정한 것**입니다. 게이트는 그 파일을 못 봅니다. 그래서 "이 인터뷰는 런타임 축만 개정하고 선행 인터뷰를 대체하지 않는다"를 **D-26으로 명시했습니다.**
  
  진짜로 안 정해진 P1은 넷입니다: **FFI 계약 / 기존 저장 상태 마이그레이션 / chromux 실패 시 동작 / xcodebuild 없는 빌드·배포**. 이건 전부 **0단계 스파이크가 답을 만들 것들**이라, 스파이크 결과를 받고 PRD에서 확정하는 게 맞다고 봅니다.
  
  **①②③ 답 주시면 마지막 게이트 한 번 돌리고 마무리하겠습니다.** (게이트 재심 기회가 1번 남았습니다.)
- recommended: none
- answer: 1. 가 
  2. 응 윈도우엇ㄴ 안해
  3. ㅇㅇㅇ 근데 앱 박인데 실제로 앱 안에 브라우저가들어간거아냐? 앱 안에 잇는게아니라고 그럼 결국?
- immediate_notes: decision: 브라우저 제어는 chromux CLI 위임 + 읽기 전용(가), 윈도우 미지원 확정, SwiftUI 전환 확정. reasoning: 소유권 충돌 회피와 macOS 전용 고정. constraints: 창이 두 개가 되는 것을 수용. explicit non-goals: 윈도우 지원, herdr-ide가 CDP 직접 조작, Chrome 창 자동 배치. verified facts: chromux headed CDP 실측(D-12), Xcode 없이 SwiftUI 빌드 성공(D-21). unresolved follow-up: 스파이크 4항목(D-18)과 IME 게이트(D-19). source: user Q12.
- needs_normalization: false

### Q13: 마지막 게이트 라운드가 백그라운드에서 돌고 있습니다. 그동안 정리해 드립니다.
- decision_ids: D-28
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:3e68a94a-1b05-4318-962d-bac6282bc739
- asked: 마지막 게이트 라운드가 백그라운드에서 돌고 있습니다. 그동안 정리해 드립니다.
  
  ## 확정된 것 (결정 27개)
  
  | | |
  |---|---|
  | **런타임** | SwiftUI + Rust 코어. Xcode 없이 `swiftc`로 빌드 가능 확인 |
  | **플랫폼** | macOS 전용. 윈도우 미지원 확정 |
  | **터미널** | SwiftTerm **1.20.0 정확 고정**. 엔진·뷰 모두 위임 |
  | **브라우저** | 앱 밖 chromux Chrome. herdr-ide는 CLI 위임 + 상태 읽기만 (~100줄) |
  | **grab** | v1 비목표 |
  | **검증** | peekaboo. chromux-CDP 방식 폐기 |
  | **메모리** | herdr-ide 프로세스만 400MB 이하 |
  | **한글 IME** | 문제 생기면 herdrm `TerminalView.swift` 참조 (읽기만, 복사 불가) |
  | **게이트** | 0단계 스파이크 4항목 통과 전에는 아무것도 삭제하지 않음 |
  | **되돌리기** | 삭제 직전 커밋에 `rust-native-final` 태그 |
  
  ## 무효화된 선행 결정 5개
  
  `D-11` 브라우저 패널+grab · `D-19` CDP 노출 · `D-34` chromux가 앱 CDP에 붙어 검증 · `D-41` 400/900MB · `D-53` chromux 전제조건
  
  나머지 선행 인터뷰(질문 44개)는 **그대로 유효**합니다. 이번 인터뷰는 런타임 축만 개정했습니다 (D-26).
  
  ## 이번 인터뷰에서 실제로 뒤집힌 것
  
  처음에 제가 드린 답 중 셋이 조사하면서 틀린 걸로 드러났습니다.
  
  1. **"WKWebView + grab"을 추천** → grab이 빠지자 존재 이유가 사라짐
  2. **"herdr browser pane 쓰시면 됩니다"** → 헤드리스 스크린캐스트라 선생님이 이미 탈락시킨 것이었음
  3. **"Swift로 가면 Xcode가 필요"** → CLT만으로 SwiftUI 빌드 성공. 1차 전환을 죽였던 장벽이 사라짐
  
  그리고 게이트가 **제가 동의 없이 결정으로 기록한 2건**을 잡아냈습니다. 되돌려서 다시 여쭙고 확정했습니다.
  
  ## 남은 미확정 (스파이크가 답할 것)
  
  FFI 계약 · 기존 저장 상태 마이그레이션 · chromux 실패 시 동작 · `xcodebuild` 없는 빌드/배포
  
  이 넷은 0단계 스파이크 산출물로 PRD에서 확정합니다.
  
  ---
  
  게이트 결과 나오면 바로 알려드리고, PASS면 `/gen-prd`로 넘어갈 준비가 끝납니다.
- recommended: none
- answer: ㅇㅇㅇ 그렇게 하고 근데 File Tree + File View는 어떤거 붙여스는게 나을까?
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q14: 조사 결과와 게이트 결과 둘 다 나왔습니다.
- decision_ids: D-29, D-30
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:39b483b6-266f-4e92-ba36-27aec67453bc
- asked: 조사 결과와 게이트 결과 둘 다 나왔습니다.
  
  ## 1. File Tree + File View
  
  **트리는 붙일 게 없습니다.** SwiftUI에 `OutlineGroup` / `DisclosureGroup`이 기본으로 있고, macOS에는 `NSOutlineView`가 있습니다. 파일 트리는 SwiftUI에서 import하는 물건이 아니라 List 하나 쓰는 일입니다. **의존성 0개.**
  
  **뷰어/편집기가 진짜 질문입니다.** D-25가 "탐색 + 이미지/마크다운/코드 강조/git diff 보기 + **가벼운 인라인 편집과 저장**"까지로 정해뒀으니, 읽기 전용이 아니라 편집기가 필요합니다.
  
  | 후보 | 별 | 최근 푸시 | 라이선스 | 판정 |
  |---|---|---|---|---|
  | **CodeEditSourceEditor** | 716 | 2026-04-20 | **MIT** | **유력** |
  | STTextView | 1588 | 2026-08-18 | **GPLv3 / 상용** | ❌ **탈락** |
  | Highlightr | 1868 | 2026-02-13 | MIT | 가벼운 대안 |
  | Runestone | 3201 | 2026-03-25 | MIT | iOS 중심 (macOS 지원 확인 못 함) |
  | Splash | 1869 | **2024-05-27** | MIT | ❌ Swift 코드만 강조 + 2년 정체 |
  
  **STTextView는 함정입니다.** 가장 활발(10일 전 푸시, open issue 3개)해서 매력적인데, 라이선스를 열어보니:
  
  > If you are creating an open source application under a license compatible with the **GNU GPL license v3**, you may use STTextView under the terms of the GPLv3. ... commercial licenses are available.
  
  **herdr-ide는 MIT입니다.** 이걸 쓰면 herdr-ide 전체가 GPLv3가 되거나 상용 라이선스를 사야 합니다. herdrm 라이선스 없음 건에 이어 **오늘 두 번째 라이선스 함정**이네요.
  
  ### 추천 조합
  
  ```
  트리        SwiftUI OutlineGroup              의존성 0
  코드 뷰/편집  CodeEditSourceEditor (MIT)        tree-sitter 하이라이팅 + 편집
  마크다운     swift-markdown-ui (MIT, ⭐3921)    또는 swiftlang/swift-markdown (Apache-2.0, 오늘 푸시)
  이미지       SwiftUI Image                     의존성 0
  git diff    Rust에서 계산 → 색만 입혀 표시       의존성 0
  ```
  
  `CodeEditSourceEditor`는 CodeEdit 에디터 프로젝트에서 떼어낸 macOS 네이티브 패키지입니다. tree-sitter 기반이라 **앞서 논의한 tree-sitter를 Rust가 아니라 Swift 쪽에서 얻게 됩니다.**
  
  **주의:** 마지막 푸시가 2026-04-20으로 **4개월 정체**, open issue 48개. SwiftTerm(어제 푸시)만큼 활발하지 않습니다.
  
  **더 가벼운 대안:** `NSTextView` + `Highlightr`. 편집은 NSTextView 기본 기능으로 하고 하이라이팅만 얹습니다. 의존성 하나로 끝나지만 tree-sitter급 정확도는 아닙니다.
  
  ---
  
  ## 2. 게이트 결과: BLOCKED (재심 소진)
  
  **P0 3건은 전부 해소됐습니다.** 브라우저 제어 동의, 윈도우, 전환 확정 모두 통과했습니다.
  
  **P1 7건이 남았고, 재심 기회(2/2)를 다 썼습니다.** 이 사이클은 종료됐고, 제가 판정을 한 번 더 부를 수 없습니다.
  
  남은 7건: v1 워크플로 명시 · 상태/복구 매트릭스 · UI 품질 인수 기준 · chromux 프로바이더 계약 · 빌드/배포 · 검증 매트릭스 · chromux 실패 동작
  
  게이트 말이 맞습니다. 제가 D-26으로 "선행 인터뷰 참조"라고만 적었는데, 게이트는 *"참조하지 말고 실제로 옮겨 적거나 정확히 인용하라"*고 요구합니다.
  
  ### 세 가지 길
  
  **(A) 스파이크부터 하고 나중에 마무리** ← 추천
  qa-log를 `paused`로 두고 0단계 스파이크를 먼저 돌립니다. **남은 P1 중 넷(FFI 계약, 빌드/배포, 검증 매트릭스, chromux 실패 동작)은 스파이크가 사실로 만들어 줍니다.** 지금 말로 정하면 또 "결정하고 나서 죽일 요인 만나기" 패턴이 됩니다.
  
  **(B) 지금 P1 7건을 다 답한다**
  질문 10~15개 더 필요합니다. 게이트를 다시 열려면 선생님의 명시적 변경 요청이 근거로 필요합니다 (`gate reopen`).
  
  **(C) 선생님이 직접 override**
  `sasu gate override`는 **사용자 전용 명령**이라 제가 못 돌립니다. 남은 P1을 감수하고 PRD로 넘어가는 선택입니다.
  
  ---
  
  지금 물어보신 File Tree 결정을 추가하는 것 자체가 변경 요청이라 게이트 재개 근거가 됩니다.
  
  **File Tree/Viewer는 위 추천 조합으로 갈까요?** 그리고 **(A)/(B)/(C) 중 어느 쪽**인가요? 저는 **File Tree 결정을 기록하고 → (A)로 스파이크 먼저**를 권합니다.
- recommended: none
- answer: FIle Tree/Viewer는 추천ㄴ조합으로 가고 B로 하자
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q15: 선행 인터뷰에 **UX 시나리오 카드 6장**이 상태·실패·복구·증명까지 다 있습니다. 게이트가 요구한 건 "참조하지 말고 옮겨
- decision_ids: D-31, D-32
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:1cacfddd-02d4-47ce-b3a0-dd1ee1094cac
- asked: 선행 인터뷰에 **UX 시나리오 카드 6장**이 상태·실패·복구·증명까지 다 있습니다. 게이트가 요구한 건 "참조하지 말고 옮겨 적어라"니, 그건 제가 하겠습니다 (증명 방식만 chromux→peekaboo로 바꾸고, 브라우저 카드는 새로 씁니다).
  
  **선생님께 물어야 하는 건 4개 영역**입니다. 가장 위험한 것부터 갑니다.
  
  ---
  
  ### Q14. 빌드/배포 (4개 묶음)
  
  이게 **1차 전환을 죽인 영역**이고, `xcodebuild`가 없다는 새 제약이 걸려 있습니다.
  
  **① 지원 macOS 버전** — herdrm은 14.0+입니다. 우리도 **macOS 14.0+**? (SwiftUI 최신 API를 쓰려면 높을수록 편하고, 선생님 맥은 15.x입니다)
  
  **② CPU 타겟** — herdrm은 유니버설(arm64+x86_64)입니다. 우리는 **arm64 전용**을 권합니다. 선생님 맥이 arm64고, 유니버설은 Rust도 두 번 빌드해서 `lipo`로 합쳐야 해서 빌드 파이프라인이 두 배가 됩니다. D-29(공개 배포)가 있지만 2026년에 Intel 맥 사용자는 소수입니다.
  
  **③ Sparkle 자동 업데이트를 v1에 넣나?** — herdrm은 씁니다(2.9.6). **v1 비목표**를 권합니다. 자동 업데이트는 서명된 appcast + 배포 서버 + EdDSA 키 관리가 따라옵니다. v1은 GitHub Releases에서 받아 쓰는 걸로 충분하고, 이건 나중에 얹을 수 있습니다.
  
  **④ 코드사인/공증** — 이게 진짜 미지수입니다. `xcodebuild` 없이 `codesign`/`notarytool`만으로 되는지 **확인 못 했습니다.** (둘 다 CLT에 포함되어 있을 가능성은 높습니다.)
  
  권고: **v1은 서명 없이 배포**하고, 사용자가 처음 실행할 때 우클릭→열기로 Gatekeeper를 통과시키게 안내합니다. 오픈소스 프로젝트에서 흔한 방식이고, Apple Developer Program(연 $99)이 없어도 됩니다.
  → 대신 **0단계 스파이크 4번 항목을 "서명 없는 .app 번들을 만들어 실행되는가"로 좁힙니다.**
  
  **①~④ 이대로 갈까요?** 다르게 하고 싶은 항목만 말씀해 주세요.
  
  (다음은 chromux 계약 → UI 품질 인수 기준 → 검증 권한 순서로 여쭙겠습니다.)
- recommended: none
- answer: 1. mac os 는 14.0+ 로 
  2. arm 64
  3. ㅇㅇㅇ 그렇게 ㄱㄱ
  4. 너가 최대한 간단하게 할 수 있는것으로 해서 해봐
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q16: Q15. chromux 계약 (4개 묶음)
- decision_ids: D-33, D-34
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:a66e5415-8c97-43d6-a2c2-91095d5a2992
- asked: ### Q15. chromux 계약 (4개 묶음)
  
  게이트가 "행복한 경로만 있고 실패 동작이 없다"고 지적한 부분입니다.
  
  **① 어느 프로필을 쓰나?**
  chromux는 없는 프로필을 스스로 안 만듭니다(실측 확인). 선택지:
  - **(가) `default` 프로필 하나만 쓴다** ← 추천. 가장 단순하고, 지금 선생님이 쓰는 방식입니다.
  - (나) workspace마다 프로필을 나눈다 → 선행 D-21/D-22가 브라우저 패널 시절 정했던 건데, 패널이 사라졌으니 근거도 약해졌습니다. 프로필이 늘면 디스크와 로그인 관리가 따라옵니다.
  
  **② chromux가 없으면?**
  선행 D-53은 "브라우저 패널은 뜨되 에이전트 연동만 degrade"였는데 패널이 없어졌습니다. 이제 단순합니다 - **"브라우저 열기" 버튼이 비활성화되고 이유를 표시**합니다 (원칙 4: 조용히 실패하지 않기). 설치 안내 문구를 같이 보여줍니다.
  
  **③ 이미 떠 있으면 재사용하나?**
  **재사용을 권합니다.** `chromux ps --json`으로 `status: running`이면 `launch`를 건너뛰고 `open`만 합니다. 매번 새로 띄우면 로그인 세션이 날아갑니다.
  
  **④ herdr-ide가 끄나?**
  **안 끕니다.** herdr-ide를 닫아도 Chrome은 그대로 둡니다. 근거는 에이전트가 그 Chrome을 쓰고 있을 수 있고, 우리가 만들지 않은 것을 닫지 않는다는 원칙입니다. 정리는 사용자가 `chromux kill`로 하거나 `chromux kill --idle`에 맡깁니다.
  
  **추가로 하나 더:** chromux **최소 버전**을 정해야 하는데, 지금 `chromux --version`을 확인 안 했습니다. 선행 D-46이 herdr에 대해 "protocol 정수로 판정하고 미달하면 조용히 degrade하지 말고 명확히 실패"를 정했는데, chromux에도 같은 방침을 적용할지요. 다만 chromux는 CLI라 버전 확인이 문자열 파싱이 됩니다. **간단하게는 "필요한 명령이 없으면 그 사유를 표시하고 실패"** 정도가 적당해 보입니다.
  
  **①~④ + 버전 방침, 이대로 갈까요?** 바꾸고 싶은 것만 말씀해 주세요.
- recommended: none
- answer: ㅇㅇㅇ 우선 가로 하자.. 
  2. ㅇㅇㅇ
  3. ㅇㅇㅇ 고고
  4. ㅇㅇ 우선 그렇게 해...
  ㅇㅇㅇ (참고로 chromux 내가만든거라 내가 걍 고치면되니까 고고)
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q17: `docs/design-reference/`에 orca 스크린샷 3장이 있네요 (orca-02는 grab용이라 이제 무관).
- decision_ids: D-35, D-36
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:fd833701-91c2-41d2-8577-047e1cae721e
- asked: `docs/design-reference/`에 orca 스크린샷 3장이 있네요 (orca-02는 grab용이라 이제 무관).
  
  ### Q16. "투박한 UI"의 합격 기준 (마지막에서 두 번째)
  
  이번 전환의 목적이 "UI가 투박하다"인데, **무엇이 되면 합격인지가 없으면 판정할 수 없습니다.** 게이트가 정확히 이걸 지적했습니다.
  
  **사람이 눈으로 판정할 항목 6개를 제안합니다:**
  
  | | 합격 기준 |
  |---|---|
  | **구조** | 3열 유지 (사이드바+pet / 터미널 / 워크벤치). 밀도는 `orca-01` 기준선 |
  | **텍스트** | 시스템 폰트 + 크기 위계가 보임. 터미널만 등폭. 한글이 안 깨짐 |
  | **상태 표현** | 색과 구조로 드러남. 문장 설명은 최후 수단 (design 원칙 7) |
  | **다크모드** | 시스템 설정을 따라감 |
  | **리사이즈** | 창 크기를 바꿔도 레이아웃이 유지되고, 패널 경계를 드래그로 조절 |
  | **네이티브 관용구** | 진짜 메뉴바 · 진짜 우클릭 메뉴 · 진짜 스크롤바 (그려낸 흉내가 아니라) |
  
  **판정 시점은 두 번**을 제안합니다:
  - **3a 끝(창+레이아웃 완성)** - 여기서 방향이 틀렸으면 일찍 잡습니다
  - **v1 끝** - 최종
  
  **판정 방법:** 스크린샷을 보고 선생님이 판정. 구조는 `peekaboo see`로 요소가 제대로 잡히는지 같이 봅니다.
  
  **그리고 `orca-01`을 시각 기준선으로 계속 쓸까요?** 선행 D-59가 그렇게 정했는데, orca는 Electron이라 밀도와 표현이 웹 스타일입니다. macOS 네이티브로 가면 오히려 **시스템 관용구를 따르는 게 맞을 수도** 있습니다.
  
  - **(가) orca-01을 밀도 기준선으로 유지** ← 선행 결정 유지
  - **(나) macOS 네이티브 관용구 우선, orca는 참고만** ← 제 추천
  
  **6개 항목 + 판정 시점 이대로 괜찮은지, 그리고 (가)/(나) 중 어느 쪽인지** 알려주세요.
- recommended: none
- answer: ㅇㅇㅇ orca-01을 시각 기준선으로 + herdr-label 플러그인(내가 만든거) 그거의 구성도 잘 적용해서 함께 넣어주고 + openrouter key 입력받게 세팅도 하고 (이거 지금 ~/.zshrc에 이씅ㅁ) + 나로 우선 고고 
  여튼 디자인 좀 깔쌈하게
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q18: 드리프트 경고가 떴습니다 (매 턴 결정을 쓰지 말고 체크포인트에 모으라는 것). 앞으로 모아서 쓰겠습니다.
- decision_ids: D-37
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:ded93e63-44e9-423b-9823-a2c6e6576a7d
- asked: 드리프트 경고가 떴습니다 (매 턴 결정을 쓰지 말고 체크포인트에 모으라는 것). 앞으로 모아서 쓰겠습니다.
  
  ### Q17. OpenRouter 키 — 이게 마지막 질문입니다
  
  **찾은 사실:**
  ```
  플러그인 src/lib.rs:50  OpenRouterApiKey => "OPENROUTER_API_KEY"
  플러그인 src/lib.rs:55  std::env::var(self.name())        ← 환경변수로만 읽음
  플러그인 실행 주체        herdr의 startup hook (herdr-ide가 아님)
  현재 키 위치              ~/.zshrc의 export
  herdr-ide 쪽              src/openrouter.rs 541줄에 Keychain 저장소가 이미 구현됨 (아직 아무 데서도 안 씀)
  ```
  
  그리고 선행 D-40이 정한 구조가 이렇습니다: **플러그인이 토큰을 만들어 herdr에 발행하고, herdr-ide는 `session.snapshot`에서 그 토큰을 읽어서 그린다.** 즉 OpenRouter를 실제로 호출하는 건 **플러그인**이지 herdr-ide가 아닙니다.
  
  그래서 "herdr-ide가 키를 입력받는다"가 세 갈래로 갈립니다.
  
  **(가) herdr-ide가 Keychain에 저장하고, 플러그인이 Keychain에서 읽도록 고친다** ← 추천
  - 선생님이 플러그인 소유자라 가능합니다 (chromux와 같은 패턴)
  - `~/.zshrc`에서 API 키가 사라집니다. 셸 프로필에 비밀을 두지 않는 게 맞습니다
  - 플러그인은 "Keychain 우선, 없으면 env" 순으로 읽으면 기존 사용자도 안 깨집니다
  - 비용: 플러그인 수정이 herdr-ide 작업에 딸려옵니다
  
  **(나) herdr-ide가 Keychain에 저장하고, 플러그인 실행 시 env로 주입한다**
  - 문제: 플러그인을 띄우는 건 herdr지 herdr-ide가 아닙니다. herdr-ide가 끼어들 지점이 없습니다. **사실상 불가능해 보입니다.**
  
  **(다) 키는 그대로 `~/.zshrc`에 두고, herdr-ide는 키 유무만 보여준다**
  - 가장 싸지만 "입력받게 세팅"이라는 요청을 만족 못 합니다
  
  ---
  
  **추가로 정할 것 2개** (가로 가실 경우):
  
  - **키가 없으면?** 사이드바에 요약 줄이 안 뜹니다. **설정 화면으로 가는 안내를 사이드바에 표시**하는 걸 권합니다 (조용히 빈 줄로 두지 않기, 원칙 4).
  - **기존 zshrc 키 마이그레이션?** 첫 실행 시 `OPENROUTER_API_KEY`가 환경에 있으면 **"Keychain으로 옮길까요?"를 묻고**, 옮긴 뒤 zshrc에서 지우라고 안내하는 걸 권합니다. 자동으로 zshrc를 고치지는 않습니다.
  
  **(가)/(다) 중 어느 쪽인가요?** 그리고 추가 2개도 이대로 괜찮은지 알려주세요.
- recommended: none
- answer: 우선 다로 하자. 나중에 고치지 뭐 
  
  키가 없으면 안내만!
  Keychain은 나중에~
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q19: UX 카드 6장 이식 완료. **마지막 질문 하나만 남았습니다.**
- decision_ids: D-36, D-38, D-39, D-40, D-41, D-42, D-43
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:d7e29b70-f166-4608-9815-e04e41702e1b
- asked: UX 카드 6장 이식 완료. **마지막 질문 하나만 남았습니다.**
  
  ### Q18. 검증을 어디서 돌리나?
  
  peekaboo는 **Screen Recording + Accessibility 권한**이 필요합니다. CI(GitHub Actions 등)에는 그 권한도, 로그인한 데스크톱 세션도 없습니다.
  
  **(가) UI 검증은 선생님 맥에서만. CI는 빌드와 순수 테스트만.** ← 추천
  ```
  CI            cargo test (Rust 코어) + swiftc 빌드 통과 + .app 조립 성공
  로컬(선생님)    peekaboo UI 시나리오 6개 + 한글 IME + 메모리 400MB + 디자인 판정
  ```
  - 근거: 원칙 12(테스트는 값이 비용을 넘을 때만). macOS UI 자동화를 CI에 올리려면 self-hosted runner에 권한을 상시 부여해야 하는데, 그 비용이 얻는 것보다 큽니다.
  - 비용: UI 회귀는 선생님이 돌려야 잡힙니다.
  
  **(나) self-hosted runner(예: mini)에 권한 주고 CI에서도 UI 검증**
  - mini가 이미 SSH로 붙어 있고 macOS입니다. 다만 그 머신은 프로덕션 자동화가 도는 곳이라 권한 상시 부여가 부담입니다.
  
  **(다) CI 자체를 v1 비목표로 둔다**
  - 지금 이 저장소에 CI가 있는지 확인 안 했습니다. 없으면 이게 제일 정직합니다.
  
  **(가)/(나)/(다)?** 답 주시면 검증 매트릭스를 쓰고 게이트 돌리겠습니다.
- recommended: none
- answer: 가
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q20: **SwiftPM이 Xcode 없이 SwiftTerm 1.20.0을 받아서 빌드했습니다.** `Build complete! (81
- decision_ids: D-38
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:bf425f48-6c11-482f-aea1-942b04c75dca
- asked: **SwiftPM이 Xcode 없이 SwiftTerm 1.20.0을 받아서 빌드했습니다.** `Build complete! (81.60s)`, arm64 바이너리 생성 확인. Swift 의존성 리스크가 사실로 해소됐습니다.
  
  ---
  
  # 마지막 확인 (게이트 재심 1회 남음)
  
  ## 먼저, 제 월권 정정 ⚠️
  
  **`src/openrouter.rs` 541줄 삭제를 제가 결정으로 기록했습니다.** 선생님은 "Keychain은 나중에"라고만 하셨는데 제가 삭제까지 확정했습니다. 게이트가 P0로 잡았습니다.
  
  **① 어떻게 할까요?**
  - **(가) 그냥 둔다** ← 추천. 미사용 코드지만 나중에 Keychain 작업할 때 그대로 씁니다. 541줄이 빌드에 드는 비용은 무시할 수준입니다.
  - (나) 삭제. 필요하면 `rust-native-final` 태그에서 꺼냅니다.
  
  ---
  
  ## 사실로 확인된 것 (질문 아님)
  
  **저장 상태가 거의 없습니다.**
  ```
  ~/Library/Application Support/Herdr IDE/navigator.json   1434B, 트리 펼침/선택 상태뿐
  ~/Library/Application Support/herdr-ide/                 ← Electron 시절 잔재 (8/26)
  ```
  진짜 사용자 상태(workspace, pane, 세션)는 **herdr가 갖고 있고** herdr-ide는 소켓으로 읽기만 합니다. 마이그레이션할 게 사실상 없습니다.
  
  **② 이렇게 처리할까요?** navigator.json은 **버리고 새로 시작**(트리 펼침 상태라 잃어도 무해), Electron 잔재 디렉터리는 **삭제**. → 이대로 괜찮으면 "ㅇㅇ"만 주세요.
  
  ---
  
  ## 나머지 4개
  
  **③ 브라우저 열기 - URL은 어디서 오나?**
  - **(가) 열기만.** 버튼 누르면 Chrome이 뜨고 끝. URL은 사용자가 Chrome에서 침 ← 추천 (가장 단순)
  - (나) herdr-ide에 URL 입력창을 둔다
  - (다) 포커스된 pane의 cwd에서 dev 서버 포트를 추측해서 연다 (herdr 플러그인에 `open-localhost` 액션이 이미 있습니다)
  
  **④ 원격(mini)이 v1 릴리스 게이트인가?**
  `remote.rs` 3550줄이 이미 있으니 기능은 v1입니다. 질문은 **"mini가 안 켜져 있으면 v1을 완료로 못 치는가"** 입니다.
  - **(가) v1 게이트에서 뺀다.** 기능은 만들되, 검증은 mini 가용할 때 ← 추천
  - (나) v1 게이트에 넣는다
  
  **⑤ Markdown 패키지 하나 확정** — 게이트가 둘을 병기했다고 지적했습니다.
  - **(가) `swift-markdown-ui`** (MIT, ⭐3921) ← 추천. SwiftUI 렌더러라 바로 붙습니다
  - (나) `swiftlang/swift-markdown` (Apache-2.0) — 파서라 렌더는 우리가 짜야 함
  
  **⑥ Chrome 창 자동배치(v2 후보) 재검토 조건**
  - **(가) "창 두 개가 실제로 불편하다고 느낄 때"** ← 추천. v1 실사용 후 판단
  - (나) 아예 영구 비목표로 닫는다
  
  ---
  
  **①~⑥ 답 주시면**, FFI 최소 계약과 UX 상태 매트릭스(첫 실행/빈/로딩/복구) 기본값까지 제가 써서 마지막 게이트를 돌리겠습니다. 그 둘도 초안을 보시고 고치실 수 있게 같이 보여드리겠습니다.
- recommended: none
- answer: 1. 나 
  Electron 잔재들 다 지워버려! 안쓴다
  
  3. 가
  4. 나 v1에 넣어.. 이미 remote 기능잇으면 테스트까지 해보면 좋지 이미 herdr remote 켜져있어서 그거 쓰면 되잖아;
  5. 가
  6. 가
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q21: 검증 매트릭스에 **V18(서버 재기동 복구)** 와 **V19(원격 재연결)** 를 추가했습니다 — 이건 이미 승인하신 결정에서
- decision_ids: D-39, D-45, D-46
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:5aae5963-eaf7-4921-8498-c088ae554aeb
- asked: 검증 매트릭스에 **V18(서버 재기동 복구)** 와 **V19(원격 재연결)** 를 추가했습니다 — 이건 이미 승인하신 결정에서 나오는 거라 물을 필요가 없었습니다.
  
  ---
  
  # 마지막 두 개만 답해주세요
  
  ### ① `navigator.json` 어떻게 할까요?
  
  `~/Library/Application Support/Herdr IDE/navigator.json` (1434바이트). 내용은 **트리에서 뭘 펼쳐놨고 뭘 선택했는지**뿐입니다. workspace·pane·세션 같은 진짜 상태는 herdr가 갖고 있습니다.
  
  - **(가) 버리고 새로 시작** ← 추천. SwiftUI 셸이 자기 포맷으로 새로 저장합니다. 잃는 건 "트리 펼침 상태" 하나고, 첫 실행 때 다 접혀 있다가 다시 펼치면 끝입니다.
  - (나) 읽어서 이행한다. Rust 포맷을 Swift가 파싱하는 코드를 씁니다.
  
  ### ② FFI 계약 — 지금 확정할까요, 스파이크 결과 보고 확정할까요?
  
  제가 초안을 썼습니다 (함수 6개, JSON 바이트, Rust가 소유권, 콜백은 "변경됐다"만 알리고 Swift가 메인 스레드에서 당겨감, 오류는 다음 스냅샷에 실림).
  
  - **(가) 스파이크 산출물로 미룬다** ← 추천. 지금 `deferred`로 돌려놨습니다. **스파이크에서 실제로 100번 왕복시켜보고** 확정합니다. 이게 이 프로젝트가 4번 넘어진 패턴("만들어보기 전에 확정")을 피하는 방식입니다.
  - (나) 지금 이 초안으로 확정. 스파이크는 검증만.
  
  ---
  
  **①②만 답 주시면** 게이트를 다시 열고(선생님 답이 재개 근거가 됩니다) 마지막으로 돌리겠습니다.
  
  그리고 앞으로는 **제가 제안한 건 선생님이 명시적으로 답하기 전까지 `open`으로 두겠습니다.** 세 번 같은 실수를 했으니 방식을 바꾸는 게 맞습니다.
- recommended: none
- answer: 버리고 새로 시작 ㅇㅇㅇ 
  레거시 다 ㅣ주어도돼 최대한 새로 하려면 새로해도되니까~~
  
  나. 이 초안으로 확정해
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q22: 게이트가 **세 개의 진짜 구멍**을 찾았습니다. 특히 두 개는 제가 놓치면 사고가 날 것들입니다.
- decision_ids: D-13, D-47, D-48, D-49, D-50
- route: user-decision
- source_ref: claude:7084c601-ca0a-4883-9be8-6aedc1af55c9:a96519e7-6c31-4bb6-bcc8-ab4dd209d465
- asked: 게이트가 **세 개의 진짜 구멍**을 찾았습니다. 특히 두 개는 제가 놓치면 사고가 날 것들입니다.
  
  ## ⚠️ 위험한 것부터
  
  ### P0. 실패 테스트가 실제 서비스를 건드리게 써놨습니다
  
  제가 V12~V19에 이렇게 썼습니다:
  - "mini 연결을 강제로 끊음(원격 herdr 정지)" ← **mini는 프로덕션 자동화가 도는 머신입니다**
  - "default 프로필을 지운 상태" ← **선생님 Chrome 로그인이 들어있는 그 프로필입니다**
  - "herdr 소켓을 내림" ← **지금 이 세션이 돌고 있는 herdr입니다**
  
  **이대로 검증을 돌리면 실제 작업을 죽입니다.** 게이트 지적이 맞습니다.
  
  **Q22-①. 실패 주입을 어떻게 격리할까요?**
  - **(가) 검증 전용 리소스만 쓴다** ← 추천
    - chromux: `herdr-ide-verify` 전용 프로필 (선생님 승인 후 `chromux profile new`로 1회 생성). `default`는 절대 안 건드림
    - herdr: 실제 소켓 대신 **가짜 소켓 경로**로 herdr-ide를 띄워 "서버 없음" 분기를 만듦
    - mini: 원격 herdr를 죽이지 않고 **SSH 터널만 끊어서** 연결 끊김을 재현
    - chromux 부재: `PATH`만 가림 (이미 V6)
  - (나) 실패 경로 검증을 v1 게이트에서 뺀다
  
  ### P0. Electron 백업을 제가 남겼습니다
  
  선생님은 "다 지워버려"라고 하셨는데 제가 `scratchpad/electron-leftovers-backup.tgz` (51K)를 남겼습니다.
  
  **Q22-②.** 지울까요? (**(가) 지운다** ← 추천 / (나) 스파이크 끝날 때까지 둔다)
  
  ---
  
  ### P1. OpenRouter 키 감지가 불가능합니다 — 이것도 좋은 지적입니다
  
  **Finder로 실행한 앱은 `~/.zshrc`를 읽지 않습니다.** launchd 환경을 받거든요. 즉 herdr-ide가 `OPENROUTER_API_KEY`를 **볼 수가 없습니다.** 터미널에서 띄우면 보이고 Finder로 띄우면 안 보이는, 실행 방법에 따라 달라지는 상태가 됩니다.
  
  **Q22-③. 어떻게 할까요?**
  - **(가) 키를 감지하지 않는다** ← 추천. herdr-ide는 **요약 줄이 비었다는 사실만** 보여주고 "플러그인 설정 확인" 안내를 답니다. 키 유무를 herdr-ide가 알 필요가 없어집니다. 결정론적이고, 키를 앱이 만지지 않으니 노출 위험도 없습니다.
  - (나) herdr 스냅샷의 플러그인 토큰으로 간접 판정
  - (다) 앱이 `~/.zshrc`를 직접 파싱 → **비추천.** 셸 파일 파싱은 깨지기 쉽고 API 키를 앱이 읽게 됩니다
  
  ---
  
  ## 제가 고칠 것 (물을 필요 없음)
  
  - **chromux 계약 모순** — D-13에 `browser_open(url)`이 있는데 D-40이 "URL 입력 없음"이라 충돌합니다. `launch` / `reuse` / `focus` / `읽기 전용 탭 상태` 4개로 정리하겠습니다
  - **FFI 스키마** — 함수 이름만 있고 options·event·snapshot 스키마와 오류 필드가 없습니다. `FrameModel`에 오류 필드도 실제로 없고요. 버전 붙인 스키마로 씁니다
  - **V1 분리** — CI(계약 검사)와 로컬(콜백·메모리 100회)로 쪼갭니다
  
  **①②③만 답 주시면 마무리하겠습니다.** (재심 1회 남음)
- recommended: none
- answer: 실제 서비스 건드려도 돼 
  백업 지워
  openrouter key는 .env에 넣어놨으니 참고해
  우선 키 감지하지 않는거로만 해줘 나중에 고칠게
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

## UX Scenario Cards

선행 인터뷰 herdr-ide-native-shell의 UX-01~UX-06을 이 런타임 결정에 맞춰 이식한다.
증명 방식은 chromux가 herdr-ide CDP에 붙는 방식(선행 D-34)에서 peekaboo(D-17)로 전부 교체했다.
UX-03은 브라우저 패널과 grab이 D-08/D-11로 사라져 새로 작성했다.

### UX-01: 막힌 에이전트를 찾아 붙는다
- trigger: 여러 workspace에 에이전트가 돌고 있고 그중 하나가 질문, 승인, 에러로 멈춰 있다.
- happy path: 사이드바 agents 목록에서 미확인(`_new`) attention 표시를 보고 클릭한다. 중앙 SwiftTerm 뷰가 그 pane의 PTY로 바뀐다. 답하면 표시가 사라진다. 항목은 2줄 구성이다(D-34).
- state / failure: (1) herdr 서버 미실행 - 소켓 파일 없음과 응답 없음을 구분해 표시하고 서버 실행 버튼을 준다. 자동 기동하지 않는다(선행 D-38). (2) 확인된 `?`는 attention이 아니라 idle로 내려간다(INV-herdr-unseen-token). (3) 플러그인이 발행한 요약 토큰이 없거나 비어 있으면 빈 줄로 두지 않고 플러그인 설정 확인 안내를 표시한다. herdr-ide는 그 원인이 키 부재인지 추론하지 않는다(D-35, D-49).
- recovery: 서버가 죽으면 버튼으로 재기동하고, 재기동 후 사이드바가 스냅샷을 다시 읽는다.
- proof: `peekaboo see --app herdr-ide --json`으로 사이드바 항목을 찾아 `peekaboo click`하고, herdr `pane.get`의 focused가 그 pane으로 바뀌는지 확인한다. `_new` 판정은 이식한 순수 함수 테스트로 본다.
- linked decisions: D-16, D-17, D-33, D-34, D-35

### UX-02: 에이전트가 만든 산출물을 확인한다
- trigger: 에이전트가 이미지나 파일을 생성했고 눈으로 확인해야 한다.
- happy path: 우측 워크벤치의 파일트리(SwiftUI OutlineGroup)에서 파일을 클릭한다. 이미지는 SwiftUI Image로, 마크다운은 swift-markdown-ui로, 코드는 CodeEditSourceEditor로 렌더된다. 값 하나를 고쳐야 하면 그 자리에서 편집하고 저장한다(선행 D-25의 F2 깊이).
- state / failure: (1) 포커스된 pane이 트리 루트와 다른 경로에 있으면 트리 상단에 그 사실을 표시하고 자동으로 따라가지 않는다. (2) 편집 중 에이전트가 같은 파일을 덮어쓰면 내 편집 유지 / 다시 읽기를 묻는다(선행 D-27). (3) 우측 패널은 워크벤치만이며 브라우저 전환은 없다(D-11).
- recovery: 명시적 조작으로만 pane 경로로 트리를 옮긴다. 충돌 시 사용자가 어느 쪽을 잃을지 고른다.
- proof: peekaboo로 트리 노드를 클릭하고 뷰어에 내용이 뜨는지 스크린샷으로 확인한다. 충돌 분기는 파일을 외부에서 수정한 뒤 다이얼로그가 뜨는지로 본다.
- linked decisions: D-11, D-17, D-28, D-33

### UX-03: 브라우저로 결과를 확인하고 에이전트에게 QA를 맡긴다
- trigger: 에이전트가 만든 로컬 화면을 확인해야 하거나, 에이전트에게 브라우저 QA를 시켜야 한다.
- happy path: herdr-ide에서 브라우저 열기를 누르면 chromux default 프로필의 진짜 Chrome 창이 별도로 뜬다. 이미 running이면 재사용한다. herdr-ide는 현재 열린 탭의 URL과 제목을 상태로 표시한다. 에이전트는 같은 Chrome에 chromux로 붙어 snapshot/click/screenshot을 수행한다.
- state / failure: (1) chromux가 없으면 버튼을 비활성화하고 사유와 설치 안내를 표시한다. 조용히 실패하지 않는다(D-31). (2) 필요한 chromux 명령이 없으면 그 사유를 표시하고 실패한다. 버전 정수 게이팅은 하지 않는다(D-31). (3) chromux 데몬이나 포트가 응답하지 않으면 상태 표시를 마지막 확인 시각과 함께 stale로 두고 갱신 실패를 알린다. (4) default 프로필이 없으면 herdr-ide가 임의 생성하지 않고 사용자에게 chromux profile new를 안내한다(D-12로 확인된 chromux 규칙). (5) 브라우저는 herdr-ide 창 밖의 별도 창이며 앱 안에 임베드되지 않는다(D-11, D-27).
- recovery: herdr-ide를 닫아도 Chrome을 종료하지 않는다. 에이전트가 쓰고 있을 수 있고 우리가 만들지 않은 것을 닫지 않는다. 정리는 사용자의 chromux kill이나 chromux kill --idle에 맡긴다(D-31).
- proof: peekaboo로 브라우저 열기를 클릭한 뒤 `chromux ps --json`의 status가 running인지, `GET 127.0.0.1:<port>/json/list`에 기대한 URL이 있는지 확인한다. chromux 부재 분기는 PATH에서 chromux를 가린 상태로 버튼이 비활성화되고 사유가 표시되는지로 본다.
- linked decisions: D-08, D-11, D-12, D-13, D-27, D-31, D-32

### UX-04: 원격(mini) workspace에서 작업한다
- trigger: mini에서 돌고 있는 에이전트를 확인하거나 붙어야 한다.
- happy path: 사이드바에 원격 workspace가 함께 보인다. 선택하면 SwiftTerm 뷰가 원격 pane에 attach되고 분할도 동작한다. 파일트리로 원격 파일을 탐색하고 뷰어로 본다. SSH와 sftp는 Rust 코어(remote.rs 3550줄)가 그대로 담당한다.
- state / failure: (1) 인라인 편집은 원격에서 동작하지 않으며 회색으로 비워두지 않고 이유를 표시한다(선행 D-37). (2) SSH 연결이 끊기면 그 사실을 알린다. 조용히 빈 목록을 보여주지 않는다. (3) 브라우저는 로컬 chromux 창이므로 원격 workspace에서도 동작하지만, 원격 호스트의 페이지를 보려면 사용자가 포트포워딩을 해야 한다.
- recovery: 원격 파일 수정은 그 pane의 터미널에서 한다. 연결 끊김은 재연결을 시도하고 결과를 알린다.
- proof: mini에 붙어 원격 workspace가 사이드바에 뜨고 attach와 파일 나열이 되는지 확인한다. 머신 가용성에 따라 차단 가능한 항목으로 둔다.
- linked decisions: D-11, D-17, D-31

### UX-05: 창이 뒤로 갔을 때 알림을 받는다
- trigger: herdr-ide가 아닌 다른 앱을 보고 있는데 에이전트가 멈췄다.
- happy path: 데스크톱의 펫이 상태를 드러낸다(에러, 질문, 승인 우선). 클릭하면 herdr-ide가 앞으로 나오면서 해당 에이전트로 이동한다.
- state / failure: (1) herdr-ide 창이 앞에 있을 때는 펫이 조용히 있고 사이드바가 알린다. 같은 사실을 두 곳에서 말하지 않는다. (2) 저장된 펫 위치가 연결되지 않은 모니터를 가리키면 주 디스플레이로 복구한다(REG-window-position-offscreen).
- recovery: 오프스크린 좌표는 클램프로 복구하며 실제 사고 좌표가 테스트에 고정되어 있다.
- proof: 클램프 기하는 순수 함수 테스트로 본다. 투명, 무테, 항상 위, 클릭스루, 드래그 조합은 SwiftUI/AppKit에서 다시 확인해야 한다. 선행 D-33의 Electron 스파이크는 무효이며 AppKit NSWindow 기준으로 재검증한다. 역할 분리는 herdr-ide 창을 앞뒤로 보낸 뒤 peekaboo로 펫 표시와 사이드바 attention 표시가 반대로 뒤집히는지 확인한다.
- linked decisions: D-15, D-17, D-33

### UX-06: workspace, worktree, pane을 앱 안에서 만들고 닫는다
- trigger: 새 작업을 시작하거나 끝난 작업을 정리한다.
- happy path: 사이드바에서 새 workspace/worktree를 만들고 라벨을 준다. workspace 안에서 새 pane을 열어 에이전트를 띄운다. 끝나면 닫는다. CLI로 나가지 않는다.
- state / failure: (1) working이나 attention 상태인 pane을 닫으려 하면 확인 다이얼로그를 띄우고 그 프로세스에 무슨 일이 생기는지 문장으로 알린다. idle이면 바로 닫는다(선행 D-44). (2) workspace나 tab을 닫을 때는 집계 경고 하나를 띄우고 종료될 에이전트를 tokens.summary와 함께 나열한다(선행 D-50). (3) worktree 제거는 체크아웃을 실제로 지우므로 결과를 고지하고 확인을 받는다. (4) Git work tree가 아닌 곳에서는 herdr가 not_git_worktree를 돌려주므로 그 사유를 그대로 보여준다. (5) herdr protocol이 요구 버전에 미달하면 부분 비활성화하지 않고 명확한 메시지로 실패한다(선행 D-46).
- recovery: 파괴적 동작은 확인 단계에서 취소할 수 있다.
- proof: peekaboo로 생성 조작을 실행한 뒤 herdr workspace.list / worktree.list / pane.list에 반영됐는지 확인한다. 파괴적 동작 검증은 herdr-ide-verify- 접두어 fixture에서만 돌린다(선행 D-56).
- linked decisions: D-17, D-33

## State And Recovery Matrix

D-33(디자인), D-40(브라우저), D-41(원격), 원칙 4(실패를 명시적으로 드러낸다)에 따른 상태 계약.
모든 빈 상태는 회색 공백이 아니라 사유와 다음 행동을 함께 보여준다.

| 표면 | 첫 실행 / 빈 | 로딩 | 실패 | 복구 |
| --- | --- | --- | --- | --- |
| 사이드바 | herdr 연결 전에는 연결 중 표시. workspace가 없으면 만들기 안내 | 스냅샷 첫 수신까지 자리 유지, 스켈레톤 | herdr 서버 미실행은 소켓 없음과 무응답을 구분해 표시하고 서버 실행 버튼 제공(자동 기동 안 함) | 버튼으로 재기동 후 스냅샷 재수신 |
| 요약 줄 | 요약 토큰이 없거나 비면 플러그인 설정 확인 안내 표시. 키 존재 여부는 추론하지 않음(D-49) | 요약 미도착은 빈 줄 대신 대기 표시 | 플러그인 미실행이면 그 사실 표시 | 사용자가 플러그인 설정을 고친 뒤 herdr 재시작 |
| 터미널 | pane 미선택이면 선택 안내 | attach 중 표시 | PTY 종료 시 종료 사실과 종료 코드 표시, 창을 조용히 비우지 않음 | 같은 pane 재attach 또는 새 pane |
| 워크벤치 | 파일 미선택이면 트리에서 고르라는 안내 | 대용량 파일 로딩 표시 | 읽기 실패는 경로와 사유 표시. 바이너리는 미리보기 불가 사유 표시 | 다시 열기 |
| 워크벤치 저장 | - | 저장 중 표시 | 저장 실패는 사유 표시하고 편집 내용을 버리지 않음 | 재시도 또는 다른 경로로 저장 |
| 워크벤치 충돌 | - | - | 편집 중 외부 변경 시 내 편집 유지 / 다시 읽기를 묻는다(선행 D-27) | 사용자가 무엇을 잃을지 선택 |
| 원격(D-41) | 원격 타겟 미설정이면 설정 안내 | SSH 연결 중 표시 | 연결 끊김은 조용한 빈 목록이 아니라 끊김 사실 표시. 인라인 편집은 비활성 + 사유 | 재연결 시도 후 결과 알림 |
| 브라우저(D-40) | chromux 없으면 버튼 비활성 + 설치 안내. 탭 없으면 열린 탭 없음 | launch 진행 표시 | 데몬·포트 무응답은 상태를 stale로 두고 마지막 확인 시각 표시. default 프로필 없으면 chromux profile new 안내 | 다시 열기. herdr-ide는 Chrome을 종료하지 않음(D-31) |
| 파괴적 동작 | - | - | working/attention pane 닫기는 확인 다이얼로그. workspace/tab은 집계 경고(선행 D-44, D-50) | 확인 단계에서 취소 가능 |

## Verification Matrix

D-17(peekaboo), D-37(실행 환경), D-19(IME 게이트 미정)에 따른 시나리오별 검증 계약.

| # | 대상 | 트리거 | 통과 판정 | 증거 | 환경 | 소유 |
| --- | --- | --- | --- | --- | --- | --- |
| V1a | FFI 계약(스키마) | 코어에 이벤트 JSON을 넣고 스냅샷을 받음 | schema_version 일치, 알 수 없는 kind가 status.last_error로 표면화, 버전 불일치 시 명확히 실패(D-50) | 스냅샷 JSON 덤프 | CI(Rust 단독) | 에이전트 |
| V1b | FFI 수명·스레드 | arm64로 링크한 실제 앱에서 Rust 스레드 이벤트를 100회 발생 | 콜백이 임의 스레드에서 오고 Swift가 메인에서 snapshot을 당겨 그림. 100회 반복에 크래시 없음, free_bytes 누수 없음(leaks 또는 heap 계측) | 실행 로그 + 누수 계측 출력 | 로컬(링크된 .app) | 에이전트 |
| V2 | SSH 바이트 왕복 | 원격 pane attach 후 키 입력 | SwiftTerm feed로 들어온 출력이 화면에 보이고 send 델리게이트가 Rust로 되돌아감 | 스크린샷 + herdr pane.read | 로컬 | 에이전트 |
| V3 | UX-01 막힌 에이전트 | peekaboo click으로 사이드바 항목 선택 | herdr pane.get의 focused가 그 pane | peekaboo see JSON + pane.get | 로컬 | 에이전트 |
| V4 | UX-02 파일 워크벤치 | peekaboo click으로 트리 노드 선택 | 뷰어에 내용 렌더, 편집 저장이 디스크에 반영 | 스크린샷 + 파일 해시 | 로컬 | 에이전트 |
| V5 | UX-03 브라우저 위임 | peekaboo click으로 브라우저 열기 | chromux ps --json status=running, /json/list에 기대 URL | 두 JSON | 로컬 | 에이전트 |
| V6 | UX-03 chromux 부재 | PATH에서 chromux를 가림 | 버튼 비활성 + 사유 표시, 조용한 실패 없음 | 스크린샷 | 로컬 | 에이전트 |
| V7 | UX-06 파괴적 동작 | working pane 닫기 시도 | 확인 다이얼로그가 뜨고 종료될 에이전트를 나열 | 스크린샷 | 로컬 (herdr-ide-verify- fixture 전용) | 에이전트 |
| V8 | 메모리 예산 | workspace 7 / pane 11 구성 | herdr-ide 프로세스 RSS 400MB 이하 | ps 출력 | 로컬 | 에이전트 |
| V9 | 한글 IME | 한글 IME 켜고 조합 입력 | 후보창이 커서를 따라옴 / 조합 중 백스페이스가 조합을 지움 / 2글자 이상 조합이 뒤 글자를 안 덮음 / 터미널과 에디터 양쪽 동작 | 스크린샷 | 로컬 | **D-19 미정: peekaboo type "gks"가 "한"을 만들면 에이전트, 아니면 사람** |
| V10 | 디자인 인수 | 3a 완료 시점, v1 완료 시점 | D-33의 6항목 | 스크린샷 | 로컬 | **사람(사용자)** |
| V11 | 빌드/패키징 | 릴리스 빌드 | swiftc 통과 + .app 조립 + codesign adhoc 성공 + 실행됨 | 빌드 로그 + codesign -dv | CI(조립까지) + 로컬(실행) | 에이전트 |
| V12 | chromux 데몬/포트 실패 | 포트를 막거나 데몬을 죽임 | 상태가 stale로 표시되고 마지막 확인 시각이 보임 | 스크린샷 | 로컬 | 에이전트 |
| V13 | chromux 프로필 부재 | 존재하지 않는 프로필 이름(herdr-ide-verify-absent)을 지정. **default를 지우지 않는다** | 임의 생성하지 않고 chromux profile new 안내 표시 | 스크린샷 + chromux 응답 | 로컬 | 에이전트 |
| V14 | herdr 서버 미실행 | herdr 소켓을 내림 | 소켓 없음과 무응답을 구분 표시하고 서버 실행 버튼 노출 | 스크린샷 | 로컬 | 에이전트 |
| V15 | herdr protocol 미달 | 요구 protocol 미만 서버 | 부분 비활성화 없이 명확한 실패 메시지(선행 D-46) | 스크린샷 | 로컬 | 에이전트 |
| V16 | 원격 v1 게이트(D-41) | mini의 herdr에 연결 | 사이드바 표시, attach, 분할, 파일 탐색, 편집 비활성 사유, 끊김 시 사유 표시 | 스크린샷 + herdr pane.list | 로컬(mini 필수) | 에이전트 |
| V17 | 워크벤치 저장 실패 | 읽기 전용 경로에 저장 시도 | 사유 표시하고 편집 내용 보존 | 스크린샷 | 로컬 | 에이전트 |
| V18 | UX-01 서버 재기동 복구 | herdr 소켓을 내린 뒤 peekaboo로 서버 실행 버튼 클릭 | 서버가 뜨고 사이드바가 스냅샷을 다시 읽어 항목이 복원됨. 실패 시 재기동 실패 사유가 표시됨 | 스크린샷 + herdr session.snapshot | 로컬 | 에이전트 |
| V19 | 원격 재연결(D-41) | mini 연결을 강제로 끊음(네트워크 차단 또는 원격 herdr 정지) | 끊김 사실이 표시되고 재연결을 시도하며, 성공 시 목록이 복원되고 실패 시 사유가 표시됨. 조용한 빈 목록 금지 | 스크린샷 + herdr pane.list | 로컬(mini 필수) | 에이전트 |

실패 주입 정책(D-47): 사용자가 실제 서비스 대상 검증을 승인했다. chromux default 프로필, 실행 중인 herdr 소켓, mini의 원격 herdr를 직접 대상으로 삼을 수 있다.
고지된 결과: 검증 실행 중 실제 작업이 중단될 수 있고, mini는 프로덕션 자동화가 도는 머신이며 default 프로필에는 사용자 로그인 세션이 있다.
단 파괴적 삭제 조작은 이 승인에 포함되지 않는다. chromux default 프로필 삭제는 사용자 Chrome 로그인을 복구 불가능하게 잃게 하므로 금지하며, 프로필 부재 분기는 존재하지 않는 이름을 지정해 재현한다(V13). workspace/worktree 생성·삭제는 선행 D-56대로 herdr-ide-verify- 접두어 fixture에서만 실행한다.
검증 실행 전에 사용자에게 시작을 알리고, 끝나면 중단시킨 서비스를 원상 복구한다.

CI에서 돌리지 않는 것과 이유: V2~V10은 Screen Recording/Accessibility 권한과 로그인 데스크톱 세션이 필요하다(D-37).

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

### Checkpoint 2
- after_question: Q9
- normalized_entries: Q7, Q8, Q9
- register_changes: 검증 경로 확정(D-17 peekaboo), IME 게이트 연기(D-19, revisit=스파이크), SwiftTerm 채택(D-16), 기존 Rust 폐기 승인(D-20), Xcode 차단요인 해소 실측(D-21), 스파이크 게이트 4항목 확정(D-18), 되돌리기 태그 확정(D-22).
- reopened_decisions: 없음
- highest_remaining_gap: D-15(SwiftUI 전환 자체의 명시적 확정)만 open. 폐쇄 문장 확인으로 종결 예정.

### Checkpoint 3
- after_question: Q11
- normalized_entries: Q10, Q11
- register_changes: SwiftTerm 1.20.0 정확 고정(D-23), 한글 IME 대응 경로로 herdrm 참조 확정 + 라이선스 제약 기록(D-24), D-16에 libghostty/NSTextView 기각 근거 보강, D-15 전환 확정(스파이크 조건부).
- reopened_decisions: 없음
- highest_remaining_gap: herdrm 라이선스 부재로 코드 복사 불가. 참조 후 독립 구현만 가능하며 D-24에 기록됨.

### Checkpoint 4
- after_question: Q12
- normalized_entries: Q12
- register_changes: 게이트 BLOCK 대응: 동의 없이 resolved였던 D-13/D-15를 open으로 되돌린 뒤 사용자 확인을 받아 재확정. 윈도우 미지원 확정(D-25, deferred->resolved). Chrome 창 자동배치 v1 비목표(D-27). 선행 인터뷰와의 범위 경계 명시(D-26). Raw Q&A 12건 전부 decision_ids 연결.
- reopened_decisions: D-13, D-15 (동의 미확인으로 open 복귀 후 Q12에서 재확정)
- highest_remaining_gap: FFI 계약, 저장 상태 마이그레이션, chromux 실패 동작, xcodebuild 없는 빌드/배포 4건은 0단계 스파이크(D-18) 산출물로 PRD에서 확정한다.

### Checkpoint 5
- after_question: Q18
- normalized_entries: Q13, Q14, Q15, Q16, Q17, Q18
- register_changes: File Tree/Viewer 스택(D-28), 빌드·배포 계약 + Xcode 없는 패키징 실측(D-29/D-30), chromux 프로바이더 계약 + 사용자 소유 사실(D-31/D-32), UI 품질 인수 6항목(D-33), 사이드바 구성(D-34), OpenRouter 키 현행 유지(D-35), openrouter.rs 폐기(D-36), 검증 실행 환경 분리(D-37). UX 시나리오 카드 6장 이식(증명을 peekaboo로 교체, UX-03 신규). 검증 매트릭스 V1~V11 신설.
- reopened_decisions: 없음
- highest_remaining_gap: D-19(IME 게이트 소유자)만 스파이크 대기. 나머지 P1은 이번 사이클에서 해소.

### Checkpoint 6
- after_question: Q20
- normalized_entries: Q19, Q20
- register_changes: openrouter.rs 삭제 확정(D-36, 사용자 선택), 영속 상태 실측 + Electron 잔재 5.5MB 삭제(D-38/D-39), 브라우저 열기만(D-40), 원격 v1 게이트 승격(D-41), Markdown 단일 확정(D-42), Chrome 배치 revisit 트리거(D-43), SwiftPM 빌드 실측(D-44), FFI 최소 계약 + 스파이크 합격 기준(D-45). State/Recovery 매트릭스 신설, 검증 V12~V17 실패경로 추가.
- reopened_decisions: 없음
- highest_remaining_gap: D-19(IME 게이트 소유자)만 스파이크 대기.

### Checkpoint 7
- after_question: Q21
- normalized_entries: Q21
- register_changes: navigator.json 폐기 확정(D-39, 사용자 명시 승인), FFI 계약 초안 확정(D-45, 사용자 명시 승인 - 스파이크는 설계가 아니라 검증), 레거시 폐기 광범위 승인(D-46, 단 D-22의 스파이크 통과 후 삭제 순서 유지). 검증 V18(서버 재기동 복구) V19(원격 재연결) 추가.
- reopened_decisions: 없음
- highest_remaining_gap: D-19(IME 게이트 소유자)만 스파이크 대기.

### Checkpoint 8
- after_question: Q22
- normalized_entries: Q22
- register_changes: 실패 주입은 실제 서비스 대상 승인(D-47, 사용자 수용 리스크 명시), Electron 백업 삭제(D-48), OpenRouter 키 감지 안 함 + .env 위생 문제 기록(D-49), chromux 계약을 launch/reuse/focus/status 4개로 정리해 D-40과 모순 해소(D-13), FFI 데이터 스키마 신설(D-50, status.last_error 포함). V1을 V1a(CI 스키마)와 V1b(로컬 수명·스레드 100회)로 분리.
- reopened_decisions: 없음
- highest_remaining_gap: D-19(IME 게이트 소유자) 스파이크 대기. .gitignore에 .env 추가는 사용자 승인 대기.

## Audit History

### Audit 1
- type: gap-audit-gate
- result: fail (cycle 1, closure exhausted)
- missing decision_ids: D-13, D-15 (에이전트가 동의 없이 resolved로 기록 -> open 복귀 후 사용자 재확정), D-25 (윈도우 정책 미기록)
- unsupported assumptions: 브라우저 CLI 전용 제어를 사용자 승인 없이 확정
- UX or behavior gap: v1 워크플로와 상태 매트릭스가 선행 인터뷰 참조로만 존재
- highest-risk blocker: 전환 전체 확정(D-15) 미수령
- final-blocking-question: 브라우저 직접 CDP 조작 여부 / 윈도우 정책 / 전환 확정
- PRD impact: 세 P0 모두 Q12에서 사용자 답변으로 해소

### Audit 2
- type: gap-audit-gate
- result: fail (cycle 2, closure exhausted)
- missing decision_ids: D-36 (openrouter.rs 삭제를 동의 없이 확정), D-39 (navigator.json 폐기를 에이전트 기본값으로 확정), D-45 (FFI 계약을 에이전트 제안으로 확정)
- unsupported assumptions: 위 3건 모두 사용자 승인 없이 resolved
- UX or behavior gap: 첫 실행/빈/로딩/복구 상태 계약 부재, chromux 실패 동작 미정
- highest-risk blocker: 에이전트 제안을 사용자 결정으로 기록하는 패턴 (3회 반복)
- final-blocking-question: openrouter.rs 삭제 / navigator.json 처리 / FFI 계약 확정 시점
- PRD impact: Q19, Q21에서 전부 사용자 답변으로 해소. State/Recovery 매트릭스와 검증 V12~V19 신설

### Audit 3
- type: gap-audit-gate
- result: fail (cycle 3, closure exhausted) -> PASS (overridden by user)
- missing decision_ids: 없음
- unsupported assumptions: 없음
- UX or behavior gap: 없음
- highest-risk blocker: V13이 사용자의 실제 chromux default 프로필 삭제를 요구했고, D-47의 사용자 승인 범위(서비스 중단형 장애 주입)를 벗어났다. 프로필 삭제는 Chrome 로그인 세션을 복구 불가능하게 잃게 한다.
- final-blocking-question: 없음. 판정 후 V13을 비파괴 방식(존재하지 않는 이름 herdr-ide-verify-absent 지정)으로 수정하고 D-51로 default 프로필 삭제를 금지했다. 그 수정으로 게이트가 stale이 되었고 재심이 소진되어 사용자가 override했다.
- PRD impact: 지적된 결함은 override 전에 이미 해소되었으나 재판정을 받지 않았다. override reason: "남은 P0 1건(V13 프로필 삭제)은 이미 비파괴 방식으로 수정됨. 스파이크부터 시작하기로 하고 PRD로 진행"

