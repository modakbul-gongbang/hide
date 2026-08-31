# hide 입력, 마우스, 링크, kitty 검증 - 2026-08-30

## 판정 범위

이 기록은 `prd/hide-rebrand`의 Swift 입력 경로를 현재 scratch 빌드와 실제 debug 앱 창에서 확인한 결과다.

- 검증 빌드: `swift test --package-path macos --scratch-path /tmp/hide-ui-sol-swift`와 같은 scratch path의 `swift build`.
- 정적 결과: 69개 테스트 전체 PASS, build PASS.
- 실제 앱: `/tmp/hide-ui-sol-swift/arm64-apple-macosx/debug/HerdrMacOS`, PID 33991, 창 10590.
- 인스턴스 조건: 검증 시작 시 debug 앱 한 인스턴스만 실행 중이었고 `/Applications/hide.app`은 실행하지 않았다.
- 전용 로컬 fixture: `/private/tmp/hide-input-local-20260830.qgIURk`, workspace `w43`, pane `w43:p1`과 `w43:p2`.
- 전용 원격 fixture: `/tmp/hide-input-remote-20260830.xE1xc7`, workspace `w4S`, pane `w4S:p1`.
- 실창 증거: [input-mouse-debug-fixture-20260830.png](input-mouse-debug-fixture-20260830.png), SHA-256 `0d3f273fa2d86a29f065ab5134f0f51875578033309d9213bcd387d27d4b2967`.

실창에서는 전용 checkout, Codex pane, 일반 shell pane, URL, 텍스트 파일 경로, 이미지 파일 경로가 공통 카드와 그리드에 렌더되는 것까지 확인했다.

Peekaboo 권한은 Screen Recording과 Accessibility 모두 허용 상태였지만, mutation 직전에 오래된 bridge host를 이유로 `CAPTURE_FAILED`를 반환했고 classic snapshot은 다음 프로세스 호출 전에 만료됐다.

CGEvent와 AX 대체 경로로 전용 checkout 선택까지 성공했으나 SwiftTerm first responder에 합성 키가 도달하지 않았고, PID 33991은 후속 입력 확인 전에 종료됐다.

설치본 단일 인스턴스는 패키징 담당자가 조율하므로 debug 앱이나 `/Applications/hide.app`을 다시 실행하지 않았다.

원칙 4에 따라 이 도구 실패 이후 직접 확인하지 못한 상호작용을 실앱 PASS로 기록하지 않고 `설치본 후속`으로 분리한다.

## 항목별 결과

| 항목 | 판정 | 근거 | 설치본 후속 |
| --- | --- | --- | --- |
| Shift+Enter, 로컬 | PASS | 사용자가 같은 변경 계열의 로컬 pane에서 줄바꿈 동작을 실측했고, `ImeTerminalView.swift:100-122`가 kitty 비활성 시 `ESC CR`을 전송한다. kitty 활성 시 SwiftTerm이 `CSI 13;2u`를 인코딩한다. | 최신 설치본에서 한 번 재확인한다. |
| Shift+Enter, 원격 | 코드 PASS, 실앱 후속 | `TerminalPointerRouting.swift:136-150`의 `HideRemoteTerminalView`가 로컬과 같은 `ModifiedTerminalInputPolicy`를 SSH child delegate에 적용한다. 원격 수정 커밋은 `674f5ab`이다. 수정 후 원격 Codex pane의 직접 입력은 PID 종료 때문에 완료하지 못했다. | Mac mini의 전용 Codex pane에서 두 줄이 제출 없이 유지되는지 확인한다. |
| 수식키 없는 드래그 선택 | 코드 PASS, 실앱 후속 | `TerminalPointerRouting.swift:23-42, 67-98`이 일반 drag를 무조건 local selection으로 정하고, `ImeTerminalView`와 `HideRemoteTerminalView`가 같은 router를 사용한다. TUI로 drag를 보내는 역방향 탈출구는 Option+drag다. | mouse reporting을 켠 Codex 또는 Claude pane에서 drag 뒤 선택 강조와 복사 결과를 확인한다. |
| 선택 후 Command+C 복사 | 코드 PASS, 실앱 후속 | SwiftTerm `MacTerminalView.copy`가 현재 selection을 general pasteboard에 기록하고 Hide는 Command+C를 pane 명령으로 예약하지 않는다. | drag, double-click, triple-click 각각 뒤 clipboard 문자열을 대조한다. |
| 더블클릭 단어 선택 | 코드 PASS, 실앱 후속 | SwiftTerm `MacTerminalView.mouseDown`의 `clickCount == 2`가 `selectWordOrExpression`을 호출하고 Hide router는 다중 클릭을 local selection으로 보낸다. | fixture의 `alpha`를 더블클릭하고 Command+C 결과가 `alpha`인지 확인한다. |
| 트리플클릭 줄 선택 | 코드 PASS, 실앱 후속 | 같은 SwiftTerm 분기에서 3회 이상 클릭은 `selection.select(row:)`를 호출한다. | fixture의 `drag-copy-target alpha beta gamma` 행 전체가 복사되는지 확인한다. |
| URL 클릭 후 브라우저 열기 | 코드 PASS, 실앱 후속 | 로컬과 원격 terminal 모두 `linkReporting = .implicit`, `linkHighlightMode = .hover`를 설정한다. `ShellModel.swift:336-344`는 `http`와 `https`를 `NSWorkspace.shared.open`으로 연다. router는 drag가 아닌 link activation을 TUI click보다 우선한다. | hover 밑줄과 `https://example.com/hide-input-proof`의 기본 브라우저 전환을 확인한다. |
| 로컬 파일 경로 클릭 후 Workbench 열기 | 코드 PASS, 실앱 후속 | `TerminalLinkResolver`가 절대, 상대, `path:line`, `path:line:column`을 한 곳에서 해석한다. `ShellModel.swift:345-369`는 checkout 내부의 읽을 수 있는 파일만 `core.openFile`로 열고 실패 사유를 표시한다. | fixture의 `sample-terminal-link.txt:2` 클릭 뒤 Workbench 선택 파일을 확인한다. |
| 이미지 경로 클릭 후 미리보기 | 코드 PASS, 실앱 후속 | 파일 경로는 같은 resolver와 Workbench 경로를 사용하고, `WorkbenchPanel.swift:238-240, 300-310, 348-350`이 지원 이미지 확장자를 `NSImage` 미리보기로 렌더한다. | fixture의 `sample-terminal-image.png` 클릭 뒤 이미지 미리보기를 확인한다. |
| 여러 줄 붙여넣기의 bracketed paste | 코드 PASS, 실앱 후속 | SwiftTerm `MacTerminalView.paste`는 `isPaste: true`로 전달하고, terminal이 bracketed paste mode일 때 `ESC[200~`, 본문, `ESC[201~` 순서로 보낸다. | Codex prompt에 두 줄을 붙여넣고 줄별 제출이 발생하지 않는지 확인한다. |
| Option+Backspace 단어 삭제 | 코드 PASS, 실앱 후속 | SwiftTerm의 `optionAsMetaKey` 기본값은 `true`이고 kitty 비활성 legacy encoder는 Option+Backspace를 `ESC DEL`로 보낸다. Hide의 IME 정책은 Option이 있는 Backspace를 plain composing Backspace로 분류하지 않는다. | Codex prompt에서 앞 단어 하나만 삭제되는지 확인한다. |
| Option+B와 Option+F 단어 이동 | 코드 PASS, 실앱 후속 | SwiftTerm은 `optionAsMetaKey == true`일 때 Option 문자 앞에 `ESC`를 붙여 보내므로 각각 `ESC b`와 `ESC f`가 된다. Hide는 이 조합을 가로채지 않는다. | Codex prompt에서 커서가 단어 단위로 좌우 이동하는지 확인한다. |
| Shift+Tab | 코드 PASS, 실앱 후속 | SwiftTerm은 `insertBacktab`을 legacy `CSI Z`, 즉 `ESC[Z`로 인코딩하며 Hide는 이 selector를 가로채지 않는다. | Codex 또는 Claude에서 Shift+Tab에 연결된 모드 전환을 확인한다. |

## 원격 파일 경로 정책

원격 pane의 파일 경로는 로컬 파일 시스템 경로로 오인하지 않는다.

`ShellModel.swift:345-350`은 원격 파일 링크를 현재 read-only snapshot 계약에서 열 수 없다고 명시적으로 표시하고 Workbench로 포커스한다.

따라서 원격 파일과 이미지 클릭의 현재 판정은 조용한 실패가 아니라 명시적 제약 표시 PASS다.

원격 Workbench에서 실제 파일과 이미지를 여는 기능은 원격 파일 읽기 계약을 추가하는 D-3 후속 범위다.

## kitty keyboard protocol 조사

영향을 받은 attach session의 kitty keyboard protocol은 비활성 상태였다.

근거는 사용자가 수정 전 Shift+Enter가 `CSI 13;2u`가 아니라 평범한 CR로 제출됐다고 실측한 점과, SwiftTerm이 `keyboardEnhancementFlags`의 초기값을 빈 집합으로 두는 점이다.

SwiftTerm 자체에는 kitty 지원이 구현돼 있다.

- `Terminal.swift:411-425`의 normal/alternate keyboard mode는 flags와 stack을 각각 보유한다.
- `Terminal.swift:1012-1079`는 application이 보내는 `CSI ? u`, `CSI = Ps u`, `CSI > Ps u`, `CSI < u`를 처리한다.
- `MacTerminalView.swift:1571-1635`는 flags가 활성일 때 modifier와 functional key를 kitty encoder로 보낸다.
- `KittyKeyboardEncoder.swift`는 Shift+Enter를 codepoint 13과 Shift modifier로 `CSI 13;2u`에 해당하도록 인코딩한다.

확정된 경계는 Hide가 kitty를 지원하지 않는 것이 아니라, kitty mode가 terminal application의 협상 출력으로만 켜지는 세션 상태라는 점이다.

현재 구조에서 가장 유력한 원인은 Hide가 이미 실행 중인 agent pane에 뒤늦게 attach할 때 earlier `CSI > Ps u` 또는 `CSI = Ps u` 협상 바이트가 새 SwiftTerm view에 다시 전달되지 않고, 새 view의 flags가 빈 상태로 시작하는 것이다.

이 late-attach 설명은 사용자 실측과 코드 경계가 일치하는 구조적 추론이며, debug PID 종료 때문에 이번 라운드에서 wire byte trace까지 확보하지는 못했다.

Hide가 flags를 임의로 강제하면 상대 CLI가 동의하지 않은 입력 형식을 보내게 되므로 안전한 해결이 아니다.

현재 수정은 kitty가 실제로 활성일 때 SwiftTerm 경로를 그대로 사용하고, 비활성 attach에서는 로컬과 원격 모두 `ESC CR` fallback을 쓰는 방식이다.

장기적으로 kitty 상태를 late attach에도 보존하려면 `herdr attach`가 terminal mode state를 재협상하거나, core snapshot/attach 계약이 keyboard enhancement flags를 전달해야 한다.

이 계약 변경은 `herdr-core/**` 소유자와 별도 D-3 설계가 필요하며 이번 Swift 범위에서는 변경하지 않았다.

## 중복 포커스 경로 확인

요청된 `ShellView.swift:133`의 직접 `core.focusPane` 우회는 이미 제거돼 있다.

현재 agent row는 `model.selectAgent(agent)`를 호출하고, pane card는 `ShellView.swift:240`에서 `model.focusPane(pane.id)`를 호출한다.

`ShellModel.swift:312-334`가 선택 device를 기준으로 원격 `remote.focus` 또는 로컬 `core.focusPane`을 고르는 단일 라우팅 경계다.

추가 Swift 수정은 필요하지 않았다.

## 최종 상태

- Swift 테스트: 69/69 PASS.
- Swift build: PASS.
- 앱 인스턴스: debug PID 33991은 종료됐다. 최종 정리 시 패키징 담당자가 실행한 설치본 PID 93445가 확인됐으며 이 검증에서는 실행, 조작, 종료하지 않았다.
- 제품 코드 FAIL: 이번 교차 확인에서 새로 확정된 항목 없음.
- 남은 게이트: 위 표의 `실앱 후속` 항목을 finisher의 최신 설치본 한 인스턴스에서 직접 재확인한다.
