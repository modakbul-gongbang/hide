---
topic: "herdr-ide 셸을 SwiftUI + Rust 코어 하이브리드로 전환"
status: "active"
where: "brownfield"
selected_packs: "ux, compatibility, risk, operation, verification"
created_at: "2026-08-28"
updated_at: "2026-08-28"
question_count: 0
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

- next_decision_id: D-08
- next_question: (owned by the live conversation until checkpoint)
- last_materiality_sweep: preflight
- outstanding_raw_entries: none
- next_checkpoint_at: Q10

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

## Raw Q&A

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

## Audit History
