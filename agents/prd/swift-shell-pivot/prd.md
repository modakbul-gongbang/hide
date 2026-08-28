---
topic: "herdr-ide macOS 셸을 SwiftUI + SwiftTerm으로 교체하고 Rust 코어를 남기는 하이브리드 전환"
status: "draft"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "SSH 자격증명 경로(SSH_AUTH_SOCK)와 원격 프로덕션 머신(mini)을 다루고, 실패 주입 검증이 사용자의 실행 중인 herdr 소켓과 chromux 로그인 프로필을 직접 대상으로 하며, 16040줄 중 약 3300줄을 삭제하는 되돌리기 어려운 구조 전환이다."
source_intake: "agents/interview/swift-shell-pivot/qa-log.md"
created_at: "2026-08-28"
updated_at: "2026-08-28"
---

# PRD: herdr-ide macOS 셸을 SwiftUI + SwiftTerm으로 교체하고 Rust 코어를 남기는 하이브리드 전환

## 1. Summary

herdr-ide의 UI 계층(직접 렌더하는 objc2 + wgpu + glyphon 스택, 약 3300줄)을 SwiftUI + SwiftTerm으로 교체하고, SSH·herdr·도메인 로직(약 12700줄)은 Rust 코어로 남겨 C ABI로 연결한다.

전환의 동기는 두 가지다. 첫째, 한글 IME가 제대로 동작하지 않는다. 현재 `firstRectForCharacterRange`가 좌표 하드코딩이라 후보창이 커서를 따라가지 않고, 조합 문자열이 두 글자를 넘으면 뒤 셀을 덮는다(D-03). 둘째, 텍스트만 그리는 수준이라 UI가 투박하다. 두 문제 모두 AppKit이 기본 제공하는 것을 직접 만들다 생긴 것이므로, 만들기를 그만두고 AppKit에 맡긴다.

브라우저는 앱 안에 넣지 않는다. grab이 비목표가 되면서(D-08) 인앱 브라우저의 존재 이유가 사라졌고, Swift에서 CEF 임베드는 실용 경로가 없다(D-10). herdr-ide는 chromux가 띄우는 진짜 Chrome을 열어주고 상태만 읽는다(D-11, D-13).

전환은 0단계 스파이크 4항목을 통과한 뒤에만 확정한다(D-18). 지난 11일간 런타임 결정이 4번 바뀌었고 그중 3번이 "결정한 뒤에 그 결정을 죽일 요인을 만나서" 죽었기 때문이다(D-05).

Approval checklist:

- 스코프 경계: 브라우저 임베드·grab·윈도우 지원·Sparkle 자동 업데이트·Keychain 키 입력이 모두 비목표다 (`3`)
- 구조 변경: `herdr-core`/`herdr-macos` 크레이트 분리와 C ABI 6함수 경계, 그리고 약 3300줄 삭제 (`5`, D-45/D-50)
- 스파이크 게이트: 4항목 중 하나라도 실패하면 멈추고 재검토하며, 삭제는 통과 후에만 한다 (`4.2`, `8` T1~T5)
- 검증 모드: 자동 검증은 peekaboo 기반이고 CI는 권한 없는 것만 돌린다 (`9.1`, `9.2`)
- 실패 주입 위험: 실제 herdr 소켓·mini·chromux `default` 프로필을 대상으로 하며 검증 중 실제 작업이 중단될 수 있다 (`4.2`, `10` RISK-1)
- 원격이 v1 릴리스 게이트다: mini가 응답하지 않으면 v1을 완료로 판정하지 않는다 (`9.2` V16, D-41)
- 환경변수 계약: Finder 실행 시 `SSH_AUTH_SOCK` 부재로 원격 인증이 실패할 수 있어 명시적 실패와 안내가 필요하다 (`6` R11, `10` RISK-2)
- delivery mode: `local` (`agents/config.json`의 `delivery.mode`)

## 2. Problem, Goal, And Users

**사용자.** 이 저장소의 단일 사용자이자 개발자. 여러 코딩 에이전트를 herdr 위에서 동시에 굴리며, 터미널·파일·에이전트 상태를 한 화면에서 다루려 한다. 한국어로 입력한다.

**문제.** 현재 셸은 wgpu + glyphon으로 텍스트를 직접 그린다. 그 결과 macOS가 공짜로 주는 것들을 전부 다시 만들어야 했고, 두 곳에서 실패했다.

- 한글 IME: 후보창 위치가 `NSRect(280, 92, 2, 22)`로 하드코딩되어 커서를 따라가지 않는다. `insertText`/`setMarkedText`의 `replacementRange`를 무시한다. `selectedRange`가 항상 `(0,0)`이다. 조합 중 편집 단축키 필터가 없다. 조합 문자열 전체를 커서 셀 하나에 넣어 두 글자 이상이면 뒤 셀을 덮는다. (D-03)
- UI 품질: 위젯이 없어 사각형과 텍스트만 있다. 네이티브 메뉴·스크롤바·접근성이 모두 자체 구현이거나 부재다.

**목표.** UI 계층을 AppKit/SwiftUI에 맡겨 위 두 문제를 구현이 아니라 채택으로 해결하고, 이미 동작하는 SSH·herdr·도메인 로직은 그대로 보존한다.

**측정 가능한 성공.** 한글 조합 입력이 터미널과 에디터 양쪽에서 정상 동작하고(`9.2` V9), UI 인수 6항목을 사람이 통과시키며(`9.2` V10), herdr-ide 프로세스 메모리가 400MB 이하이고(V8), 원격 workspace가 동작한다(V16).

### 2.1 User Scenarios

- SC1. 막힌 에이전트를 찾아 붙는다.
  Actors: 사용자 (herdr-ide 앞에 있음).
  Primary path: 사이드바 agents 목록에서 미확인 attention 표시를 보고 클릭하면 중앙 터미널이 그 pane의 PTY로 바뀌고, 답하면 표시가 사라진다. 항목은 2줄 구성이다(상태 심볼 + 라벨 + 에이전트 종류 / 요약 + 경과 시간).
  Failure state: herdr 서버가 실행 중이 아니면 소켓 파일 없음과 무응답을 구분해 표시하고 서버 실행 버튼을 준다. 자동 기동하지 않는다. 플러그인이 발행하는 요약 토큰이 없거나 비어 있으면 빈 줄로 두지 않고 플러그인 설정 확인 안내를 단다. herdr-ide는 그 원인이 키 부재인지 추론하지 않는다.
  Recovery: 서버 실행 버튼으로 재기동하면 사이드바가 스냅샷을 다시 읽어 항목이 복원된다. 재기동이 실패하면 그 사유를 표시한다.
  Reach: herdr 서버가 실행 중이고 workspace와 pane이 최소 하나 있어야 한다. attention 상태를 만들려면 에이전트가 질문 상태여야 한다.

- SC2. 에이전트가 만든 산출물을 확인하고 값 하나를 고친다.
  Actors: 사용자.
  Primary path: 우측 워크벤치 파일트리에서 파일을 클릭하면 이미지·마크다운·코드가 각각 렌더되고, 텍스트 파일은 그 자리에서 고쳐 저장할 수 있다.
  Failure state: 포커스된 pane이 트리 루트와 다른 경로에 있으면 트리 상단에 그 사실을 표시하고 자동으로 따라가지 않는다. 편집 중 에이전트가 같은 파일을 덮어쓰면 "내 편집 유지 / 다시 읽기"를 묻는다. 저장이 실패하면 사유를 표시하고 편집 내용을 버리지 않는다. 바이너리 파일은 미리보기 불가 사유를 표시한다.
  Recovery: 충돌은 사용자가 무엇을 잃을지 고른다. 저장 실패는 재시도하거나 다른 경로로 저장한다.
  Reach: 로컬 workspace와 읽을 수 있는 파일이 필요하다. 충돌 분기는 편집 중 외부에서 같은 파일을 수정해 만든다. 저장 실패 분기는 읽기 전용 경로가 필요하다.

- SC3. 브라우저로 결과를 확인하고 에이전트에게 QA를 맡긴다.
  Actors: 사용자, 에이전트(chromux를 통해 같은 Chrome을 조작).
  Primary path: 브라우저 열기를 누르면 chromux `default` 프로필의 진짜 Chrome 창이 herdr-ide 창 밖에 뜨거나 이미 떠 있으면 앞으로 나온다. herdr-ide는 현재 탭의 URL과 제목을 표시한다. URL 입력은 Chrome 안에서 사용자가 한다.
  Failure state: chromux가 없으면 버튼을 비활성화하고 사유와 설치 안내를 표시한다. 필요한 chromux 명령이 없으면 사유를 표시하고 실패한다. 데몬이나 포트가 응답하지 않으면 상태를 stale로 두고 마지막 확인 시각을 함께 보인다. 지정한 프로필이 없으면 임의 생성하지 않고 `chromux profile new` 안내를 표시한다. 열린 탭이 없으면 "열린 탭 없음"을 표시한다.
  Recovery: 다시 열기로 재시도한다. herdr-ide는 Chrome을 종료하지 않으므로 정리는 사용자가 `chromux kill`로 한다.
  Reach: chromux가 PATH에 있고 `default` 프로필이 존재해야 한다. 부재 분기는 PATH에서 chromux를 가려 만들고, 프로필 부재 분기는 존재하지 않는 이름을 지정해 만든다.

- SC4. 원격(mini) workspace에서 작업한다.
  Actors: 사용자, mini의 herdr 서버.
  Primary path: 사이드바에 원격 workspace가 함께 보이고, 선택하면 터미널이 원격 pane에 attach되며 분할도 동작한다. 파일트리로 원격 파일을 탐색하고 뷰어로 본다.
  Failure state: 인라인 편집은 원격에서 동작하지 않으며 회색으로 비워두지 않고 이유를 표시한다. SSH 연결이 끊기면 조용한 빈 목록 대신 끊김 사실을 표시한다. `SSH_AUTH_SOCK`이 환경에 없으면 인증 실패를 조용히 넘기지 않고 사유와 실행 방법 안내를 표시한다.
  Recovery: 재연결을 시도하고 성공하면 목록이 복원되며 실패하면 사유를 표시한다. 원격 파일 수정은 그 pane의 터미널에서 한다.
  Reach: mini에 herdr 서버가 실행 중이어야 하고 `~/.ssh/config`의 별칭으로 접속 가능해야 한다. 끊김 분기는 SSH 터널을 끊어 만든다.

- SC5. workspace·tab·pane을 앱 안에서 만들고 닫는다.
  Actors: 사용자.
  Primary path: 사이드바에서 workspace/worktree를 만들고 라벨을 준다. pane을 열어 에이전트를 띄우고 끝나면 닫는다. CLI로 나가지 않는다.
  Failure state: working이나 attention 상태인 pane을 닫으려 하면 확인 다이얼로그가 뜨고 그 프로세스에 무슨 일이 생기는지 문장으로 알린다. idle이면 바로 닫는다. workspace나 tab을 닫을 때는 개별 확인 대신 집계 경고 하나를 띄우고 종료될 에이전트를 요약과 함께 나열한다. worktree 제거는 체크아웃을 실제로 지우므로 결과를 고지하고 확인을 받는다. Git work tree가 아닌 곳에서는 herdr가 돌려준 사유를 그대로 보인다. herdr protocol이 요구 버전에 미달하면 부분 비활성화 없이 명확히 실패한다.
  Recovery: 파괴적 동작은 확인 단계에서 취소할 수 있다.
  Reach: 검증은 `herdr-ide-verify-` 접두어 fixture workspace/worktree에서만 수행한다. working 상태 pane이 필요하므로 fixture에 장시간 실행 프로세스를 띄운다.

- SC6. 창이 뒤로 갔을 때 알림을 받는다.
  Actors: 사용자 (다른 앱을 보고 있음).
  Primary path: 데스크톱 펫이 상태를 드러내고(에러·질문·승인 우선) 클릭하면 herdr-ide가 앞으로 나오면서 해당 에이전트로 이동한다.
  Failure state: herdr-ide 창이 앞에 있을 때는 펫이 조용히 있고 사이드바가 알린다. 같은 사실을 두 곳에서 동시에 말하지 않는다. 저장된 펫 위치가 연결되지 않은 모니터를 가리키면 주 디스플레이로 복구한다.
  Recovery: 오프스크린 좌표는 클램프로 복구한다.
  Reach: 펫 창은 투명·무테·항상 위·클릭스루 조합이 필요하다. 선행 Electron 스파이크 결과는 무효이므로 AppKit `NSWindow`로 다시 확인해야 한다.

## 3. Scope And Non-Goals

### In Scope

- `herdr-core`(플랫폼 무지 Rust) / `herdr-macos`(현행 셸, 최종 삭제) 크레이트 분리와 C ABI 경계.
- SwiftUI 셸: 3열 레이아웃, 사이드바, 워크벤치, 메뉴바, 컨텍스트 메뉴, 펫 표면.
- SwiftTerm 1.20.0 기반 터미널 뷰와 Rust SSH/PTY 바이트 왕복.
- 파일트리(SwiftUI `OutlineGroup`)와 뷰어/가벼운 편집(CodeEditSourceEditor, swift-markdown-ui, SwiftUI `Image`).
- chromux 위임 브라우저 열기·재사용·포커스·읽기 전용 탭 상태.
- 원격(mini) workspace: 표시, attach, 분할, 파일 탐색, 편집 비활성 사유, 끊김·재연결.
- `swiftc`/SwiftPM 빌드와 `.app` 번들 조립, ad-hoc 서명.
- 환경변수 레지스트리와 부팅 시 명시적 검증.

### Non-Goals

- **인앱 브라우저 임베드**(WKWebView, CEF 모두). grab이 비목표가 되어 존재 이유가 사라졌고 Swift+CEF는 실용 경로가 없다. 결과: 브라우저는 별도 창이고 창 전환이 한 번 생긴다. 재검토 조건: 창 두 개가 실제로 불편하다고 사용자가 느낄 때. (D-08, D-10, D-11)
- **grab**(요소를 집어 에이전트에 주입). 사용자가 현재 불필요로 판정. (D-08)
- **윈도우 지원**. macOS 전용. 결과: SwiftTerm 채택과 브라우저 외부화가 이 전제 위에서만 성립하므로, 윈도우 요구가 생기면 셋을 함께 재검토한다. (D-25)
- **Sparkle 자동 업데이트**. 서명된 appcast·배포 서버·EdDSA 키 관리를 함께 요구한다. 배포는 GitHub Releases. 재검토 조건: 사용자 외 배포 대상이 생길 때. (D-29)
- **유니버설 바이너리**. arm64 전용. Rust를 두 아키텍처로 빌드해 `lipo`로 합치는 파이프라인 이중화를 피한다. (D-29)
- **코드사인 인증서와 공증**. ad-hoc 서명만. 첫 실행은 우클릭 후 열기로 통과한다. `notarytool`과 `stapler`가 CLT에 있으므로 계정만 추가하면 나중에 가능하다. (D-29, D-30)
- **Keychain 기반 OpenRouter 키 입력 UI**. 실제 호출 주체가 herdr-ide가 아니라 플러그인이고, 플러그인은 herdr의 startup hook이 띄우므로 herdr-ide가 값을 전달할 지점이 없다. `src/openrouter.rs` 541줄은 삭제한다. 재검토 조건: 사용자가 셸 프로필에서 키를 빼고 싶어질 때. (D-35, D-36, D-49)
- **herdr-ide의 OpenRouter 키 존재 감지**. Finder 실행과 터미널 실행에서 결과가 달라지는 비결정적 상태가 된다. 요약 줄이 비었다는 사실만 표시한다. (D-49)
- **herdr-ide가 CDP로 브라우저를 조작하는 것**. chromux가 데몬·세션·직렬화·잠금을 소유하므로 주인이 둘이 되면 에이전트 조작 중 충돌한다. (D-13)
- **herdr-ide 내 URL 입력창과 dev 서버 포트 추측**. 열어주기만 한다. (D-40)
- **`navigator.json` 이행**. 트리 펼침·선택 상태뿐이라 첫 실행은 접힌 상태로 시작한다. (D-39)
- **Chrome 창 자동 배치**(v2 후보). 재검토 조건: 창 두 개가 실제로 불편할 때. (D-27, D-43)
- **파일 조작**(새 파일·이름 변경·삭제·드래그 이동). 선행 인터뷰 herdr-ide-native-shell의 워크벤치 깊이 결정(F3 비목표)을 유지한다. 터미널이 항상 옆에 있어 `mv`/`rm`/`touch`가 이미 최소 경로다.
- **CI에서의 UI 자동 검증**. Screen Recording과 Accessibility 권한, 로그인 데스크톱 세션이 필요하다. self-hosted runner에 권한을 상시 부여하는 비용이 크다. (D-37)

### 선행 인터뷰와의 관계

이 PRD는 선행 인터뷰 `herdr-ide-native-shell`(질문 44개)을 대체하지 않고 런타임 축만 개정한다(D-26). 무효화한 선행 결정은 herdr-ide-native-shell의 브라우저 패널+grab, CDP 엔드포인트 노출, chromux가 앱 CDP에 붙는 검증 방식, 400/900MB 메모리 예산, chromux 전제조건 다섯이며, 각각 이 PRD의 비목표와 D-17·D-14·D-13이 대체한다. 나머지 선행 결정(제품 정체성, 3열 구조, 워크벤치 깊이 F2, workspace/worktree 모델, 사이드바 계약, 생성·닫기 고지 정책, 원격 타겟 설정, herdr 버전 게이팅, pet 흡수, 공개 배포)은 그대로 유효하다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

- **mini의 herdr 서버가 실행 중이고 `ssh mini`로 접속 가능한지 사용자가 확인한다.** V16이 v1 릴리스 게이트이므로 mini가 응답하지 않으면 완료 판정이 불가능하다. 에이전트가 대신 할 수 없는 이유: 원격 머신의 가용성과 그 머신에서 무엇이 돌고 있는지는 사용자만 판단할 수 있고, 프로덕션 자동화가 도는 머신이다.
- **peekaboo의 Screen Recording과 Accessibility 권한이 부여되어 있는지 사용자가 확인한다.** `peekaboo permissions status --json`이 둘 다 허용을 보고해야 한다. 에이전트가 대신 할 수 없는 이유: macOS 권한 부여는 시스템 설정에서 사람이 직접 승인해야 한다.
- **실패 주입 검증을 시작하기 전에 사용자가 진행 중인 실제 작업이 없음을 확인한다.** 검증이 실행 중인 herdr 소켓과 mini 연결을 중단시킨다. 에이전트가 대신 할 수 없는 이유: 어떤 작업이 진행 중이고 중단해도 되는지는 사용자만 안다.

### 4.2 Human Decisions Before PRD Approval

- **스코프와 비목표 승인.** 특히 인앱 브라우저·grab·윈도우·Sparkle·Keychain 키 입력이 모두 빠지는 것.
- **구조 변경 승인.** 크레이트 분리, C ABI 6함수 경계와 데이터 스키마, 약 3300줄 삭제.
- **실패 주입이 실제 서비스를 대상으로 하는 것 승인.** 검증 중 실행 중인 herdr 세션과 mini 연결이 중단된다. chromux `default` 프로필은 삭제하지 않지만 Chrome 프로세스는 영향을 받을 수 있다. (D-47, D-51)
- **원격을 v1 릴리스 게이트로 두는 것 승인.** mini가 응답하지 않으면 v1 미완료로 판정된다. (D-41)
- **삭제 시점 승인.** 스파이크 통과 전에는 아무것도 삭제하지 않고, 삭제 직전 커밋에 `rust-native-final` 태그를 남긴다. (D-22, D-46)
- **한글 IME 게이트 소유자가 스파이크 결과로 정해지는 것 승인.** `peekaboo type "gks"`가 "한"을 만들면 자동, 아니면 사람 판정이 완료 게이트가 된다. (D-19)

### 4.3 Decision Traceability For Fidelity Review

**전환 확정과 게이트**

- D-15 (사용자 결정): SwiftUI + Rust 코어 전환 확정, 스파이크 조건부 → `3` 스코프, `8` T1~T5.
- D-18 (사용자 결정): 스파이크 4항목을 전환 게이트로 → `8` T1~T4, `9.2` V1/V23/V2/V9/V11.
- D-22, D-46 (사용자 결정): 삭제는 스파이크 통과 후, 직전 커밋에 태그 → `8` T5, `11` 가드레일.
- D-05, D-26 (사실): 11일간 4번의 런타임 전환 이력과 선행 인터뷰 범위 경계 → `3` 선행 인터뷰와의 관계, `10` RISK-4.
- D-20 (사용자 결정): 기존 Rust 셸 코드 삭제 승인 → `8` T5.

**터미널과 IME**

- D-16 (사용자 결정): SwiftTerm 채택, 엔진과 뷰 모두 위임. libghostty는 공식 문서가 임베딩 미안정을 명시해 기각, NSTextView 직접 구현은 전환 목적에 반해 기각 → `5`, `6` R3.
- D-23 (사용자 결정): SwiftTerm 1.20.0 정확 고정 → `6` R3, `11` 가드레일.
- D-03 (사실): 현 NSTextInputClient 4가지 결함과 조합 문자열 셀 덮어쓰기 → `2` 문제, `9.2` V9.
- D-24 (사용자 결정): IME 문제 시 herdrm `TerminalView.swift` 참조. 라이선스가 없어 복사 불가, 읽고 독립 구현만 → `11` 가드레일, `10` RISK-5.
- D-19 (사용자 결정, 연기): IME 게이트 소유자는 스파이크 결과로 확정 → `9.2` V9, `10` OPEN-1.

**브라우저**

- D-08 (사용자 결정): grab v1 비목표 → `3` 비목표.
- D-11 (사용자 결정): 브라우저 임베드 안 함, `browser.rs` 721줄 삭제 → `3` 비목표, `8` T5.
- D-13 (사용자 결정): chromux 연동을 launch/reuse/focus/status 4개로. 직접 CDP 조작 금지 → `6` R5, SC3.
- D-31 (사용자 결정): `default` 프로필만, 부재 시 안내, 재사용, 종료 안 함, 버전 게이팅 없이 명령 부재 시 실패 → `6` R5, SC3.
- D-40 (사용자 결정): 열기만, URL 입력창 없음 → `3` 비목표, `6` R5.
- D-09, D-10, D-12 (사실): herdr 브라우저 플러그인이 headless 스크린캐스트라는 것, Swift+CEF 경로 부재, chromux headed CDP 실측 → `3` 비목표 근거, `10` 맥락.
- D-32 (사실): chromux는 사용자 본인 제작이므로 계약 불일치 시 herdr-ide에서 우회하지 말고 chromux를 고친다 → `11` 가드레일.
- D-27, D-43 (사용자 결정): Chrome 창 자동 배치는 v2, 재검토 트리거 기록 → `3` 비목표.

**파일과 워크벤치**

- D-28 (사용자 결정): 트리는 SwiftUI `OutlineGroup` 자체 구현, 코드 뷰/편집은 CodeEditSourceEditor, 이미지는 SwiftUI `Image`, git diff는 Rust 계산 후 표시 → `6` R4.
- D-42 (사용자 결정): 마크다운은 swift-markdown-ui 단일 확정 → `6` R4.
- STTextView 기각 (사실, D-28): GPLv3 또는 상용 라이선스라 MIT 프로젝트에 사용 불가 → `11` 가드레일.

**빌드와 배포**

- D-29 (사용자 결정): macOS 14.0+, arm64 전용, Sparkle 비목표, ad-hoc 서명, GitHub Releases → `3` 비목표, `6` R9.
- D-30, D-21, D-44 (사실): Xcode 없이 SwiftUI 컴파일·`.app` 조립·ad-hoc 서명·SwiftPM 의존성 해석이 모두 실측으로 확인됨 → `5`, `9.2` V11.

**데이터와 상태**

- D-38 (사실): 영속 상태는 `navigator.json` 하나뿐이고 실제 사용자 상태는 herdr가 소유. Electron 잔재 5.5MB 삭제 완료 → `3` 스코프, `10` 맥락.
- D-39 (사용자 결정): `navigator.json` 이행하지 않고 새로 시작, 손상 시 앱을 막지 않고 메모리 기본값 → `6` R8.
- D-35, D-36, D-49 (사용자 결정): OpenRouter 키 현행 유지, `openrouter.rs` 삭제, 키 감지 안 함 → `3` 비목표, `6` R10.

**검증**

- D-17 (사용자 결정): peekaboo가 chromux-CDP 검증을 대체 → `9.1`, `9.2`.
- D-37 (사용자 결정): CI는 권한 불필요한 것만, UI·IME·메모리·디자인은 로컬 → `9.1`, `9.2`.
- D-41 (사용자 결정): 원격을 v1 릴리스 게이트로 승격 → `9.2` V16, V19.
- D-47 (사용자 결정, 수용 리스크): 실패 주입이 실제 서비스 대상 → `4.2`, `10` RISK-1.
- D-51 (에이전트 결정, D-47 범위 내): chromux `default` 프로필 삭제 금지, 부재 분기는 없는 이름으로 재현 → `9.2` V13, `11` 가드레일.
- D-14 (사용자 결정): 메모리 인수 조건을 herdr-ide 프로세스만 400MB 이하로 재정의 → `7` AC8, `9.2` V8.
- D-33 (사용자 결정): UI 품질 사람 판정 6항목, 판정 시점 2회, 네이티브 관용구 우선 + orca-01 밀도 기준선 병행 → `7` AC9, `9.3`.
- D-34 (사용자 결정): 사이드바는 herdr-agent-context-labels 레이아웃을 네이티브로 적용 → `6` R2.

**FFI**

- D-45, D-50 (사용자 결정): C ABI 6함수와 데이터 스키마, 소유권·스레드·수명·오류 규칙, 스파이크 합격 기준 → `5`, `6` R6, `9.2` V1/V23.
- D-02 (사실): `FrameModel`이 이미 ViewModel 구조 → `5`.
- D-01 (사실): 플랫폼 의존이 두 파일에만 있어 분리 비용이 낮음 → `5`.

**에이전트 가정 (사용자 결정 아님)**

- 가정: `agents/config.json`의 `delivery.mode: "local"`을 그대로 따르며 PR 자동화를 요구하지 않는다. 사용자가 PR 발행을 원하면 `/ship`으로 별도 처리한다.
- 가정: `9.2`의 V12·V17은 사용자가 명시적으로 요구하지 않았으나 D-31과 SC2의 실패 상태를 증명하기 위해 필요하다고 판단해 추가했다.
- D-52 (사용자 결정): 환경변수 레지스트리를 축소된 형태로 확정. 부팅 실패는 두지 않고 `SSH_AUTH_SOCK` 부재 시 원격 기능만 사유와 함께 비활성화한다 → `6` R11, `7` AC11, `9.2` V14. 에이전트가 먼저 부팅 실패 + 폴백 금지 정책을 제안했으나 사용자가 축소안을 골랐고, 그 격상은 승인 없이 이루어진 것이었다.

**원칙 인테이크**

- 읽은 문서: `engineering/principles.md`(규칙 13개 전체), `design/principles.md`(규칙 7개 전체), `engineering/practices/env.md`, `engineering/practices/test.md`. 출처 커밋 `f03e930e8c5ad8c250a24d7f70be8c4889c2d6ca`.
- 번역하지 않은 규칙: engineering 규칙 11(모든 연산은 두 번 실행된다고 가정)은 이 PRD의 변경이 멱등성이 문제되는 외부 부수효과를 만들지 않으므로 가드레일로 옮기지 않았다. 단 `9.2` V11의 `.app` 조립 스크립트는 재실행 수렴을 요구하므로 `11`에 남겼다.

## 5. Major Technical Structure Changes

**크레이트 분리.** 단일 크레이트를 `herdr-core`(플랫폼 무지)와 `herdr-macos`(현행 셸, 최종 삭제)로 나눈다. `herdr-core`의 `Cargo.toml`에 objc2·wgpu·glyphon·raw-window-handle을 넣지 않아 컴파일러가 경계를 강제한다. 실측 근거: 플랫폼 의존이 `app.rs` 69곳과 `render.rs` 2곳에만 있고 나머지 20개 파일 약 12700줄은 `std`/`serde`/`anyhow`만 쓴다(D-01).

**새 FFI 경계.** `herdr-core`를 `staticlib`으로 빌드하고 C ABI 6함수를 노출한다: `herdr_core_create` / `dispatch` / `snapshot` / `on_change` / `free_bytes` / `destroy`. 데이터는 양방향 JSON UTF-8 바이트이며 최상위에 `schema_version`을 갖는다. 소유권은 Rust가 `Core`와 모든 버퍼를 갖고 Swift가 `free_bytes`로 반환한다. `create`/`dispatch`/`snapshot`/`destroy`는 메인 스레드 전용이고, `on_change` 콜백은 임의 스레드에서 데이터 없이 변경 사실만 알리며 Swift가 메인으로 홉해 `snapshot`을 당겨간다. `dispatch`는 패닉을 경계 밖으로 넘기지 않고 오류를 다음 스냅샷의 `status.last_error`로 표면화한다. (D-45, D-50)

**스냅샷 스키마 확장.** 기존 `FrameModel`에 `status` 객체를 신설한다. herdr 연결 상태, 원격 연결 상태, chromux 가용성, `last_error(kind, message, retryable, occurred_at)`를 담는다. 현 `FrameModel`에는 오류 필드가 없어 실패가 프로세스 밖에서 관측되지 않는다(원칙 10). (D-50)

**UI 계층 교체.** SwiftUI + AppKit 셸이 `render.rs`(921줄), `terminal.rs`(755줄), `accessibility.rs`(165줄), `browser.rs`(721줄), `app.rs`의 AppKit/wgpu 부분(약 1000줄)을 대체한다. 제거되는 의존: wgpu, glyphon, bytemuck, raw-window-handle, objc2 3종, alacritty_terminal, security-framework.

**빌드 시스템 교체.** Cargo 단독에서 SwiftPM(`Package.swift` + `Package.resolved`) + Cargo 조합으로 바뀐다. XcodeGen과 `xcodebuild`는 쓰지 않는다(설치되어 있지 않으며 필요하지도 않음을 실측). 산출은 `swiftc` 실행파일 + Rust `staticlib` 링크 → `.app` 손조립 → ad-hoc 서명. (D-29, D-30, D-44)

**환경변수 레지스트리 신설.** `herdr-core`가 읽는 모든 키를 한 곳에 열거 가능한 형태로 등록하고 부팅 시 일괄 검증한다. 현재 `herdr.rs:81`과 `remote.rs:386`이 `env::var(key)` 간접 호출로 흩어져 있다.

**외부 서비스 경계 변화.** CEF CDP 게이트웨이(자체 loopback 서버, capability 토큰)가 사라지고, 그 자리를 chromux CLI 호출과 `127.0.0.1:<port>/json/list` 읽기 전용 GET이 대신한다. herdr-ide가 여는 리스닝 포트가 없어진다.

## 6. Requirements

- R1. SwiftUI 셸이 3열 레이아웃(사이드바+펫 / 터미널 / 워크벤치)을 제공하고, 창 크기 변경 시 레이아웃이 유지되며 패널 경계를 드래그로 조절할 수 있다.
- R2. 사이드바가 herdr `session.snapshot`의 토큰을 읽어 항목당 2줄로 렌더한다. 1줄은 상태 심볼 + workspace 라벨 + 에이전트 종류, 2줄은 최대 30자 작업 요약 + 경과 시간(`12s`/`4m`/`2h`/`3d` 형식)이다. 상태 7종(question, approval, error, working, unseen completion, idle, unknown)을 상태당 하나의 고정 심볼로 표시하고, working과 unseen completion은 깜빡임이 아니라 색으로 구분한다. 정렬은 막고 있는 순서를 따른다: 완료 미확인(에러 → 질문/승인 → 일반 완료) → 진행 중 → 이미 확인됨. 각 그룹 안의 동률은 `activity` 시계로 깨고 가장 최근 활동이 앞에 온다.
- R3. 터미널 뷰가 SwiftTerm 1.20.0(정확 고정)이며, Rust 코어의 SSH/PTY 바이트를 `feed(byteArray:)`로 받고 사용자 입력을 `send(source:data:)` 델리게이트로 코어에 돌려준다. 한글 조합 입력이 터미널과 에디터 양쪽에서 동작한다.
- R4. 워크벤치가 파일트리(SwiftUI `OutlineGroup`)와 뷰어를 제공한다. 이미지는 SwiftUI `Image`, 마크다운은 swift-markdown-ui, 코드는 CodeEditSourceEditor로 렌더하며 텍스트 파일은 인라인 편집과 저장을 지원한다. git diff는 Rust 코어가 계산한 결과를 표시한다.
- R5. 브라우저 열기가 chromux CLI에 위임된다. `chromux ps --json`으로 상태를 확인해 running이 아니면 `chromux launch`하고, running이면 재사용하며 창을 앞으로 가져온다. `/json/list`를 읽기 전용으로 조회해 현재 탭의 URL과 제목을 표시한다. herdr-ide는 CDP로 페이지를 조작하지 않고, 없는 프로필을 생성하지 않으며, Chrome을 종료하지 않는다.
- R6. Rust 코어가 C ABI 6함수(`herdr_core_create` / `dispatch` / `snapshot` / `on_change` / `free_bytes` / `destroy`)를 노출하고 `5`가 정의한 소유권·스레드·수명·오류 규칙을 지킨다.
- R6a. `options_json`은 `schema_version`, herdr 소켓 경로, 원격 타겟 목록, 앱 상태 파일 경로를 담는다.
- R6b. 이벤트는 `{schema_version, kind, payload}` 형태이며 `kind`는 최소한 `key`, `click`, `focus_pane`, `open_browser`, `create_workspace`, `create_tab`, `create_pane`, `close_workspace`, `close_tab`, `close_pane`, `file_open`, `file_save`, `retry_connect`를 포함한다.
- R6c. 스냅샷은 `schema_version`과 기존 `FrameModel` 필드(`navigator`, `overlay`, `tab`, `connection`, `zoomed`, `focused`, `editor`)에 더해 `status` 객체를 갖는다. `status`는 herdr 연결 상태, 원격 연결 상태, chromux 가용성, 그리고 `last_error(kind, message, retryable, occurred_at)`를 담는다.
- R6d. 알 수 없는 이벤트 `kind`는 무시되지 않고 `status.last_error`로 표면화되며, `schema_version` 불일치는 조용히 진행하지 않고 명확히 실패한다.
- R7. 원격 workspace가 사이드바에 표시되고 attach·분할·파일 탐색이 동작한다. 인라인 편집은 원격에서 비활성화되며 회색 공백이 아니라 사유가 표시된다. SSH 연결이 끊기면 조용한 빈 목록 대신 끊김 사실이 표시되고 재연결을 시도하며 결과를 알린다.
- R8. 앱의 UI 상태(트리 펼침·선택)가 자체 포맷으로 저장·복원된다. 기존 `navigator.json`은 읽지 않는다. 상태 파일이 없거나 손상된 경우 앱을 막지 않고 기본값으로 진행하며 그 사실을 구조화된 로그로 남긴다.
- R9. 빌드가 Xcode 없이 완결된다. SwiftPM이 의존성을 정확 버전으로 해석하고, Rust `staticlib`과 링크되며, macOS 14.0+ / arm64 `.app` 번들로 조립되고 ad-hoc 서명된다. 조립 스크립트를 두 번 실행해도 같은 결과로 수렴한다.
- R10. herdr-ide는 `OPENROUTER_API_KEY`를 읽지도 저장하지도 로그에 남기지도 않으며, 키의 존재 여부를 추론하지도 않는다. 판단 근거는 herdr 스냅샷의 요약 토큰이 있는지뿐이다. 요약 토큰이 없거나 비어 있으면 빈 줄로 두지 않고 플러그인 설정 확인 안내를 표시한다.
- R11. `herdr-core`가 읽는 모든 환경변수가 한 곳에 열거 가능한 형태로 등록되고, 각 키가 선택/필요 구분·형태·부재 시 동작을 명시한다. 부팅을 실패시키지 않는다. `SSH_AUTH_SOCK`이 없으면 원격 기능만 사유와 함께 비활성화하고 나머지 앱은 정상 동작한다. 애플리케이션 코드는 레지스트리를 통해서만 환경변수를 읽는다. 오류나 상태 메시지에 값을 넣지 않는다. (D-52)
- R12. 모든 빈 상태·로딩·실패가 사유와 다음 행동을 함께 표시한다. `2.1`의 각 시나리오 Failure state가 회색 공백이나 조용한 무동작으로 나타나지 않는다.

## 7. Acceptance Criteria

- AC1. 한글 IME 후보창이 터미널과 에디터 양쪽에서 커서 위치를 따라간다.
- AC2. 조합 중 백스페이스가 PTY로 전달되지 않고 조합을 지운다.
- AC3. 두 글자 이상 조합 문자열이 인접한 다음 글자를 덮지 않는다.
- AC4. 사이드바 항목을 선택하면 herdr가 보고하는 focused pane이 그 pane으로 바뀐다.
- AC5. chromux가 PATH에 없을 때 브라우저 열기가 비활성 상태이고 그 사유가 화면에 보인다.
- AC6. chromux Chrome이 이미 실행 중일 때 두 번째 열기가 새 인스턴스를 만들지 않는다.
- AC7. 원격 SSH 연결이 끊기면 빈 목록이 아니라 끊김 사실과 재연결 결과가 화면에 보인다.
- AC8. workspace 7 / pane 11 구성에서 herdr-ide 프로세스 RSS가 400MB 이하다.
- AC9. `9.3`의 UI 품질 6항목이 사람 리뷰를 통과한다.
- AC10. 알 수 없는 이벤트 `kind`를 코어에 넣으면 무시되지 않고 다음 스냅샷의 `status.last_error`에 나타난다.
- AC11. `SSH_AUTH_SOCK`이 없는 상태에서 앱은 정상적으로 뜨고, 원격 기능만 비활성 상태로 그 사유가 화면에 보이며, 표시된 메시지에 환경변수 값이 포함되지 않는다.
- AC12. Xcode가 설치되지 않은 상태에서 빌드 산출물이 macOS 14.0+ arm64 `.app`으로 실행된다.
- AC13. 조립 스크립트를 연속 두 번 실행한 결과가 첫 실행 결과와 같다.
- AC14. working 상태 pane을 닫으려 하면 확인 다이얼로그가 뜨고 그 프로세스에 무슨 일이 생기는지 문장으로 표시된다.
- AC15. 워크벤치 저장이 실패하면 편집 내용이 유지되고 실패 사유가 표시된다.

## 8. PRD-Level Tasks

- T1. 0단계 스파이크: Rust `staticlib`과 Swift 간 FFI 왕복을 `5`의 계약대로 구현하고, 원격 SSH 이벤트가 Rust 스레드에서 발생했을 때 Swift가 갱신된 스냅샷을 그리는 것을 100회 반복해 크래시와 누수가 없음을 확인한다. `spikes/` 아래에서 수행하고 본 코드를 변경하지 않는다. Covers R6.
- T2. 0단계 스파이크: Rust가 뽑은 SSH 바이트를 SwiftTerm `feed(byteArray:)`에 넣고 `send` 델리게이트로 되받는 왕복을 확인한다. Covers R3. Depends on: T1.
- T3. 0단계 스파이크: 원격 SSH + 에이전트 TUI 환경에서 한글 조합 입력을 확인하고, `peekaboo type "gks"`가 "한"을 만드는지 판정해 IME 게이트 소유자를 확정한다. Covers R3, AC1, AC2, AC3. Depends on: T2.
- T4. 0단계 스파이크: `xcodebuild` 없이 SwiftPM + Rust `staticlib` 링크 → `.app` 번들 조립 → ad-hoc 서명 → 실행까지 확인한다. Covers R9, AC12. Depends on: T1.
- T5. 스파이크 4항목 통과 확인 후, 삭제 직전 커밋에 `rust-native-final` 태그를 남기고 `herdr-core`/`herdr-macos` 크레이트를 분리한 뒤 폐기 대상(`render.rs`, `terminal.rs`, `accessibility.rs`, `browser.rs`, `openrouter.rs`, `app.rs`의 AppKit/wgpu 부분과 해당 의존)을 삭제한다. Depends on: T1, T2, T3, T4.
- T6. `herdr-core`에 환경변수 레지스트리를 만들고 부팅 시 일괄 검증을 붙이며, 애플리케이션 코드의 직접 접근을 레지스트리 경유로 바꾼다. Covers R11, AC11. Depends on: T5.
- T7. C ABI 6함수와 스냅샷 `status` 객체를 `herdr-core`에 구현한다. Covers R6, AC10. Depends on: T5.
- T8. SwiftUI 셸의 창·3열 레이아웃·리사이즈·메뉴바를 만든다. Covers R1. Depends on: T7.
- T9. SwiftTerm 터미널 뷰를 붙이고 Rust 바이트 왕복을 연결한다. Covers R3, AC1, AC2, AC3. Depends on: T8.
- T10. 사이드바를 herdr 토큰 기반 2줄 레이아웃으로 만든다. Covers R2, AC4. Depends on: T8.
- T11. 워크벤치 파일트리와 뷰어·인라인 편집을 만든다. Covers R4, AC15. Depends on: T8.
- T12. chromux 위임 브라우저 열기·재사용·포커스·상태 표시를 만든다. Covers R5, AC5, AC6. Depends on: T7.
- T13. 원격 workspace 표시·attach·파일 탐색·편집 비활성 사유·끊김과 재연결을 연결한다. Covers R7, AC7. Depends on: T9, T10, T11.
- T14. 앱 UI 상태 저장·복원과 손상 시 기본값 진행을 만든다. Covers R8. Depends on: T8.
- T15. 생성·닫기와 파괴적 동작 고지를 만든다. Covers AC14. Depends on: T10.
- T16. 모든 빈·로딩·실패 상태에 사유와 다음 행동을 붙인다. Covers R10, R12. Depends on: T9, T10, T11, T12, T13.
- T17. `.app` 조립·ad-hoc 서명 스크립트를 재실행 수렴하도록 만들고 빌드 파이프라인에 넣는다. Covers R9, AC12, AC13. Depends on: T5.
- T18. 펫 표면을 AppKit NSWindow로 만들고 창 포커스에 따른 역할 분리와 오프스크린 좌표 복구를 붙인다. Covers SC6. Depends on: T8.
- T19. 검증에 필요한 `herdr-ide-verify-` 접두어 fixture(workspace, worktree, 장시간 실행 pane)를 만드는 도구를 작성한다. Covers SC5의 Reach. Depends on: T7.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | 크레이트 경계, 빌드, `.app` 조립 | none |
| automated behavior | yes | 코어 순수 로직, FFI 스키마, 환경변수 계약 | none |
| browser/runtime | yes | peekaboo 기반 앱 UI 시나리오 | 최종 UX 판정 |
| external/remote | yes | mini 원격 workspace | mini 가용성 확인 |
| human review | yes | UI 품질 6항목, 한글 IME(스파이크 결과에 따름) | 시각·조합 입력 판정 |

`automated behavior`는 코어 계층에 대해 required-for-done이다. UI 계층은 peekaboo가 접근성 트리를 통해 실제 앱을 몰기 때문에 `browser/runtime`이 그 자리를 대신하며, UI 배선 단위 테스트는 리팩터마다 깨지고 잡는 버그가 없어 쓰지 않는다(원칙 12, `practices/test.md` 규칙 6).

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | automated behavior | R6, R6d, AC10 | 알 수 없는 이벤트 kind와 schema_version 불일치가 무시되지 않고 각각 status.last_error와 명확한 실패로 드러난다 | yes | no |
| V23 | browser/runtime | R6 | 링크된 실제 앱에서 Rust 스레드 이벤트 100회가 메인 스레드 홉을 거쳐 화면에 반영되고 크래시와 버퍼 누수가 없다 | yes | no |
| V2 | browser/runtime | R3, SC1 | SSH/PTY 바이트가 SwiftTerm에 표시되고 사용자 입력이 코어를 거쳐 원격에 도달한다 | yes | no |
| V3 | browser/runtime | R2, AC4, SC1 | 사이드바 항목 선택이 herdr가 보고하는 focused pane을 바꾸고, herdr 서버 부재가 소켓 없음과 무응답으로 구분 표시된다 | yes | no |
| V26 | browser/runtime | R2 | 7개 상태 토큰을 담은 fixture에서 각 상태가 고유 심볼로, working과 unseen completion이 서로 다른 색으로 나타나고, 2줄 형식(30자 요약 + 경과 시간)과 막고 있는 순서 정렬 및 activity 동률 처리가 화면 순서로 확인된다 | yes | no |
| V4 | browser/runtime | R4, AC15, SC2 | 파일 선택이 종류별로 렌더되고, 편집 저장이 디스크에 반영되며, 저장 실패 시 편집 내용이 유지되고 사유가 보이고, 외부 변경 충돌이 선택지를 제시한다 | yes | no |
| V27 | browser/runtime | R4 | 알려진 내용의 git diff가 있는 파일에서 Rust 코어가 계산한 추가·삭제 라인이 화면에 그대로 표시된다 | yes | no |
| V5 | browser/runtime | R5, AC6, SC3 | 브라우저 열기가 chromux Chrome을 띄우거나 재사용하고 현재 탭 URL이 표시된다 | yes | no |
| V6 | browser/runtime | R5, AC5, SC3 | chromux 부재 시 버튼이 비활성이고 사유가 보이며 조용한 무동작이 없다 | yes | no |
| V7 | browser/runtime | AC14, SC5 | working pane 닫기가 확인 다이얼로그와 결과 문장을 띄우고, idle pane은 확인 없이 닫힌다 | yes | no |
| V8 | browser/runtime | AC8 | 실사용 규모(workspace 7 / pane 11)에서 herdr-ide 프로세스 RSS가 400MB 이하다 | yes | no |
| V9 | human review | R3, AC1, AC2, AC3 | 한글 조합에서 후보창이 커서를 따라오고, 조합 중 백스페이스가 조합을 지우고, 두 글자 이상 조합이 뒤 글자를 덮지 않으며, 터미널과 에디터 양쪽에서 동작한다 | yes | no |
| V10 | human review | AC9 | UI 품질 6항목이 3a 완료 시점과 v1 완료 시점 모두에서 통과한다 | yes | no |
| V11 | build/static | R9, AC12, AC13 | Xcode 없이 빌드·링크·`.app` 조립·ad-hoc 서명이 통과하고 실행되며, 조립 재실행이 같은 결과로 수렴한다 | yes | no |
| V12 | browser/runtime | R5, SC3 | chromux 데몬이나 포트가 응답하지 않을 때 상태가 stale로 표시되고 마지막 확인 시각이 보인다 | yes | no |
| V13 | browser/runtime | R5, SC3 | 존재하지 않는 프로필 이름을 지정하면 임의 생성 없이 안내가 표시된다 | yes | no |
| V14 | browser/runtime | R11, AC11 | SSH_AUTH_SOCK이 없는 환경에서 앱이 정상적으로 뜨고 원격 기능만 사유와 함께 비활성이며, 표시된 메시지에 환경변수 값이 없다 | yes | no |
| V15 | browser/runtime | R12, SC1 | herdr protocol 미달이 부분 비활성화 없이 명확한 실패로 드러난다 | yes | no |
| V16 | external/remote | R7, SC4 | mini 원격 workspace가 표시되고 attach·분할·파일 탐색이 동작하며 인라인 편집 비활성 사유가 보인다 | yes | no |
| V17 | browser/runtime | R8 | 상태 파일이 없거나 손상돼도 앱이 기본값으로 뜨고 그 사실이 로그에 남는다 | yes | no |
| V28 | browser/runtime | R8 | 트리 펼침·선택을 바꾸고 앱을 다시 띄우면 그 상태가 복원되며, sentinel 값을 넣은 레거시 navigator.json이 있어도 그 값이 화면에 나타나지 않는다 | yes | no |
| V18 | browser/runtime | R12, SC1 | herdr 서버를 내린 뒤 서버 실행 버튼을 누르면 사이드바가 스냅샷을 다시 읽어 복원되거나 재기동 실패 사유가 표시된다 | yes | no |
| V19 | external/remote | R7, AC7, SC4 | mini 연결을 끊으면 끊김이 표시되고 재연결 시도 결과가 성공 또는 명시적 실패로 드러난다 | yes | no |
| V20 | browser/runtime | R10, SC1 | 요약 토큰이 비었을 때 빈 줄이 아니라 플러그인 설정 확인 안내가 보이고, 화면과 로그 어디에도 키 값이 없다 | yes | no |
| V25 | automated behavior | R10 | 소스 전체에 OPENROUTER_API_KEY 접근이 없고, sentinel 값을 넣고 앱을 돌려도 상태 파일과 로그 어디에도 그 값이 기록되지 않는다 | yes | no |
| V21 | build/static | R11, R6a, R6b, R6c | `herdr-core`가 objc2·wgpu·glyphon 없이 빌드되고, 애플리케이션 코드에 레지스트리를 우회한 환경변수 직접 접근이 없으며, options·event·snapshot 스키마가 선언된 형태와 일치한다 | yes | no |
| V22 | external/remote | SC4, R7 | Finder에서 실행한 앱에서도 원격 인증 경로가 동작하거나, `SSH_AUTH_SOCK` 부재가 조용한 실패가 아니라 사유와 함께 드러난다 | yes | no |
| V24 | browser/runtime | SC6 | 펫이 herdr-ide 창이 뒤에 있을 때만 상태를 드러내고 앞에 있을 때는 사이드바가 알리며, 오프스크린 좌표가 주 디스플레이로 복구된다 | yes | no |

실패 주입 정책: 사용자가 실제 서비스 대상 검증을 승인했다(D-47). chromux `default` 프로필, 실행 중인 herdr 소켓, mini의 원격 herdr를 직접 대상으로 삼을 수 있다. 고지된 결과는 검증 실행 중 실제 작업이 중단될 수 있다는 것이다. 단 파괴적 삭제는 이 승인에 포함되지 않는다. chromux `default` 프로필 삭제는 사용자 Chrome 로그인을 복구 불가능하게 잃게 하므로 금지하며 V13은 존재하지 않는 이름으로 재현한다(D-51). workspace/worktree 생성·삭제는 `herdr-ide-verify-` 접두어 fixture에서만 수행한다. 검증 시작을 사용자에게 알리고, 끝나면 중단시킨 서비스를 원상 복구한다.

CI에서 돌리는 것은 V1, V11, V14, V21뿐이다. 나머지는 Screen Recording과 Accessibility 권한, 로그인 데스크톱 세션, mini 가용성이 필요하다(D-37).

### 9.3 Human Verification

- 한글 조합 입력 4항목(V9): 후보창이 커서를 따라오는가, 조합 중 백스페이스가 조합을 지우는가, 두 글자 이상 조합이 뒤 글자를 덮지 않는가, 터미널과 에디터 양쪽에서 되는가. T3의 스파이크 결과 `peekaboo type "gks"`가 "한"을 만들면 자동 검증으로 전환할 수 있으나, 전환 전까지 이 항목이 완료 게이트다.
- UI 품질 6항목(V10): 구조(3열, orca-01 밀도 기준선), 텍스트(시스템 폰트 위계, 터미널만 등폭, 한글 안 깨짐), 상태 표현(색과 구조 우선, 문장은 최후), 다크모드(시스템 추종), 리사이즈(레이아웃 유지 + 드래그 조절), 네이티브 관용구(진짜 메뉴바·우클릭 메뉴·스크롤바). 판정 시점은 T8 완료 시점과 v1 완료 시점 두 번이다.
- 실패 주입 검증 시작 승인: 실행 중인 herdr 세션과 mini 연결이 중단되므로 시작 전 사용자 확인이 필요하다.
- 스파이크 4항목 통과 판정과 삭제 착수 승인(T5): 되돌리기 지점을 지나는 결정이다.

## 10. Risks And Open Decisions

- RISK-1 (높음). 실패 주입 검증이 사용자의 실행 중인 herdr 세션과 mini 연결을 중단시킨다. 사용자가 알고 승인했다(D-47). 완화: 검증 시작을 사전 고지하고 종료 후 원상 복구하며, 파괴적 삭제는 `herdr-ide-verify-` fixture와 존재하지 않는 프로필 이름으로 대체한다(D-51).
- RISK-2 (높음). Finder로 실행한 앱은 셸 환경을 상속하지 않으므로 `SSH_AUTH_SOCK`이 없을 수 있고, 원격이 v1 릴리스 게이트이므로 이것이 완료 판정을 막을 수 있다. 완화: R11의 레지스트리가 이 키의 부재를 조용한 실패 대신 원격 기능 비활성화와 사유 표시로 드러낸다(D-52). V22가 이를 증명한다. 근본 해결(launchd 환경 주입 또는 앱 자체 설정)은 스파이크 결과를 보고 판단한다.
- RISK-3 (중간). CodeEditSourceEditor는 마지막 푸시가 2026-04-20으로 4개월 정체이고 open issue가 48개다. 완화: 현실화하면 `NSTextView` + Highlightr(MIT) 조합으로 후퇴한다(D-28).
- RISK-4 (중간). 이번이 5번째 런타임 전환이며 이전 4번 중 3번이 결정 후에 그 결정을 죽일 요인을 만나 무산됐다. 완화: T1~T4 스파이크가 게이트이며 통과 전에는 삭제하지 않고, 삭제 직전 커밋에 `rust-native-final` 태그를 남긴다(D-18, D-22).
- RISK-5 (낮음). herdrm은 GitHub 라이선스가 NONE(all rights reserved)이므로 코드를 복사할 수 없다. 완화: 읽고 접근법을 이해한 뒤 독립 구현만 한다. herdrm에 라이선스가 추가되면 재검토한다(D-24).
- RISK-6 (낮음). SwiftTerm은 사실상 개인 주도 프로젝트이고(open issue 75) 릴리스 변동이 잦다(1.19와 1.20이 같은 날 발행). 완화: 1.20.0 정확 고정, 업데이트는 수동 검증 후(D-23).
- OPEN-1 (연기, blocking 아님). 한글 IME 완료 게이트의 소유자가 사람인지 에이전트인지 미정이다. T3에서 `peekaboo type "gks"` 결과로 확정한다. 그때까지 V9는 사람 판정으로 둔다. owner: 사용자. (D-19)

## 11. Implementation Guardrails

`implement`는 사용자에게 묻지 않고 다음을 하지 않는다.

- 승인된 스코프를 넘기지 않는다. 특히 인앱 브라우저, grab, 윈도우 지원, Sparkle, Keychain 키 입력 UI, 파일 조작(생성·이름 변경·삭제)을 추가하지 않는다.
- 스파이크 T1~T4가 전부 통과하기 전에 어떤 파일도 삭제하지 않고 `rust-native-final` 태그 없이 T5를 시작하지 않는다.
- `herdr-core`에 objc2·wgpu·glyphon·AppKit 타입을 넣지 않는다. 컴파일러가 강제하도록 `Cargo.toml`에 그 의존을 추가하지 않는다. (원칙 5)
- 승인되지 않은 외부 서비스·리스닝 포트·스키마를 도입하지 않는다. herdr-ide는 CDP 조작을 하지 않고 리스닝 포트를 열지 않는다.
- chromux `default` 프로필을 삭제하지 않는다. 없는 프로필을 생성하지 않는다. Chrome을 종료하지 않는다. (D-31, D-51)
- 사용자의 실제 workspace나 worktree를 지우지 않는다. 파괴적 조작 검증은 `herdr-ide-verify-` 접두어 fixture에서만 한다.
- herdrm의 코드를 복사하지 않는다. 라이선스가 없으므로 읽고 독립 구현만 한다. (D-24)
- STTextView를 도입하지 않는다. GPLv3 또는 상용이라 MIT 프로젝트와 충돌한다. (D-28)
- 실패를 기본값·빈 값·조용한 무동작으로 덮지 않는다. 필수 환경변수에 폴백을 붙이지 않는다. (원칙 4, `practices/env.md` 규칙 3)
- 실패를 로그로만 남기고 진행하지 않는다. 모든 실패는 스냅샷 `status`를 통해 프로세스 밖에서 관측 가능해야 한다. (원칙 10)
- 자체 구현으로 대체하지 않는다. chromux 계약이 맞지 않으면 herdr-ide에서 우회하지 말고 chromux를 고친다. (D-32, 원칙 6·7)
- 나중에 교체할 작정의 임시 기반을 놓지 않는다. FFI 경계는 확정된 계약대로 만든다. (원칙 8)
- `.app` 조립 스크립트를 두 번 실행하면 첫 실행과 같은 결과로 수렴해야 한다. (원칙 11)
- UI 배선 단위 테스트를 추가하지 않는다. 리팩터마다 깨지고 잡는 버그가 없다. 자체 모듈을 모킹하지 않는다. (원칙 12, `practices/test.md` 규칙 2)
- 상태를 문장으로 설명하지 않는다. 색·배지·아이콘·배치가 먼저이고 라벨은 그것으로 부족할 때만 더한다. 설명 문단이 필요한 화면은 레이아웃에서 실패한 것이다. (design 원칙 7)
- 파괴적이거나 사용자에게 보이는 동작 전에 결과를 문장으로 알린다. (design 원칙 6)
- 사용자가 계산하게 만들지 않는다. 파생 상태(연결됨/끊김, 유효/만료, working/idle)는 화면이 계산해 보여준다. (design 원칙 4)
- 기존 패턴을 따른다. 선행 인터뷰가 정한 3열 구조와 사이드바 계약을 새로 발명하지 않는다. (design 원칙 5)

## 12. Implementation Result Report Contract

구현 에이전트는 다음을 보고한다.

- 상태: `Done` / `Partially Done` / `Blocked`.
- 사용자에게 보이는 변화.
- 변경된 주요 모듈·경계·데이터 형태: 크레이트 분리 결과, C ABI 실제 시그니처, 스냅샷 스키마 최종형.
- 구현 중 선택한 실제 파일/모듈 구조와 각 책임 경계.
- 승인된 기술 구조를 따랐는지, 벗어났다면 무엇을 왜.
- 스파이크 T1~T4 각각의 판정 결과와 T3의 IME 게이트 소유자 결정(OPEN-1 해소).
- `rust-native-final` 태그의 커밋 해시와 삭제된 파일·의존 목록.
- T1~T18 완료 상태.
- R1~R12(R6a~R6d 포함), AC1~AC15, V1~V28 커버리지.
- 모드별 검증 증거: CI 로그, peekaboo 스냅샷과 스크린샷, `ps` 메모리 측정, mini 원격 증거.
- 추가·수정한 자동 테스트와 각각이 막는 회귀 위험. 테스트를 쓰지 않은 영역은 왜 다른 증명 모드가 더 강한지.
- 환경변수 레지스트리의 최종 키 목록과 각 키의 필수/선택·부재 시 동작.
- 실패 주입 검증으로 중단시킨 서비스와 원상 복구 결과.
- 편차와 그 사유.
- 남은 사람 리뷰 항목(V9, V10).
- 하지 못한 것과 후속 후보.
- delivery: `agents/config.json`의 `local` 모드이므로 PR 발행은 이 PRD의 범위 밖이며, 필요하면 `/ship`으로 별도 수행한다.
