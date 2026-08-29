# herdr-pet에서 옮겨온 기록

이 저장소는 `herdr-pet`(Rust + Tauri)을 대체한다.
펫 기능 이식은 완료되었다(`agents/prd/pet-integration/prd.md`).
셸은 SwiftUI, 코어는 herdr-core(Rust)다.

## 0. 결정 이력: "Rust 재사용 안 함"은 뒤집혔다

이전 인터뷰의 D-14는 herdr-pet의 Rust 구현을 재사용하지 않기로 했다.
pet-integration 인터뷰의 D-16이 그 결정을 공식적으로 뒤집었다.

- **코어 로직은 이식했다.** pose 우선순위, 수면 시퀀스, 상태 집계, `_new` 토큰 계약, 오프스크린 클램프 기하는 herdr-core로 옮겼다.
- **UI는 재작성했다.** Tauri 웹뷰 JS는 재사용하지 않고 SwiftUI로 다시 썼다.

근거: 이 로직들은 실제 사고에서 나온 회귀 테스트를 달고 있었고, 언어가 같으므로(Rust → Rust) 테스트째로 옮기는 쪽이 다시 쓰는 쪽보다 싸고 안전했다.
UI는 런타임 자체가 달라(웹뷰 → AppKit) 옮길 것이 없었다.

## 1. 그대로 유효한 지식 문서 (`docs/`)

| 파일 | 내용 |
| --- | --- |
| `status-model.md` | herdr `_new` 토큰 계약, pet pose 우선순위, 뱃지 행 |
| `pet-window-macos.md` | 투명·항상 위 창의 함정 목록 |
| `theme-contract.md` | 테마 계약. 이식하면서 schema v2로 갱신 |
| `ambient-signals.md` | 앰비언트 신호 설계와 프라이버시 경계 |
| `pet-assets.md`, `asset-prompts.md` | 펫 아트 규격과 생성 프롬프트 |
| `verification-fixtures.md` | 검증 픽스처 |
| `dev-runtime.md` | 인스턴스 하나만 띄우고 설치 번들로 검증하라는 규칙. Swift 셸 기준으로 갱신 |

## 2. 이식 결과: 원본은 어디로 갔나

`docs/ported-reference/`의 Rust/JS 원본 중 이식이 끝난 것은 이 변경에서 삭제했다.
이식본이 유일한 원본이고, 원본 사본을 남겨두면 둘이 갈라진다.

| 원본 | 이식된 곳 |
| --- | --- |
| `behavior.rs` | `herdr-core/src/pet.rs` - `sleep_phase_for_idle_ms`, `expanded_state`, `is_roam_allowed`, `pose`. 4초 전이와 우선순위 사다리 테스트를 그대로 가져왔다 |
| `aggregate.rs` | `herdr-core/src/pet.rs` - `PetSummary`, `summarize`, `top_status`, Disconnected 미집계 규칙 |
| `herdr.rs` | `_new` 토큰 판정은 이미 `herdr-core/src/sidebar.rs`가 소유하고 있어 재구현하지 않았다. `ambient` 엄격 파싱만 `sidebar.rs`로 옮겼다 |
| `window.rs` | 클램프 기하는 `macos/Sources/HerdrMacOS/PetWindow.swift`의 `PetPlacement`. 사고 좌표 `[542720, 163840]`는 `PetIntegrationTests`에 고정되어 있다 |
| `hotkey.js` | `macos/Sources/HerdrMacOS/PetHotkey.swift`. `event.key`가 아니라 물리 키를 쓰라는 함정은 `NSEvent.keyCode` 기반 판정으로 옮겼고 테스트로 고정했다 |

### 남겨둔 원본

| 원본 | 왜 남겼나 |
| --- | --- |
| `ssh.rs`, `remote-targets.example.toml` | 원격/SSH는 아직 이식되지 않았다(`src/remote.rs`가 유일한 미이식 영역). 그 작업의 참조로 계속 필요하다 |

### 의도적으로 가져오지 않은 것

이식 대상 파일 안에 있었지만 이번 범위가 아닌 것들이다.
되살리려면 아래 근거부터 다시 검토한다.

- **question payload 표시** (`herdr.rs`의 `QuestionPayload`, `attach_session_questions`): `herdr-agent-pet` 플러그인이 herdr 쪽 설치물이라 흡수 불가하고 현재 설치도 안 된 상태다 (D-07).
- **attention escalation 레벨** (`aggregate.rs`의 `EscalationEngine`, `AttentionLevel`, `attention-0..3` 아트): 2/10/30분 경과에 따라 attention 아트를 격상하던 기능. `status-model.md`의 뱃지·pose 계약에 없고 PRD가 요구하지 않는다.
- **사운드 큐** (`aggregate.rs`의 `UrgentTransition`): 위와 같다.
- **이벤트 구독·중복 제거·디바운스** (`herdr.rs`의 `subscribe_stream`, `EventDeduper`, `EventDebouncer`): herdr-core는 1초 폴링으로 세션을 읽는다. 폴링 경로가 이 셋을 대체한다.
- **호스팅 터미널 raise** (`window.rs`의 `select_session_process`, AppleScript 탭 선택): herdr-pet은 외부 터미널을 앞으로 끌어와야 했다. herdr-ide는 자기 자신이 터미널이므로 클릭은 메인 윈도우를 포커스하고 pane을 선택한다.

## 3. 규칙 (`agents/rules/`)

`INDEX.md`의 herdr-pet 유래 규칙들은 `trigger.paths`와 `check`가 herdr-pet의 `crates/herdr-core/*` 경로를 가리킨다.
이식이 끝났으므로 이 저장소 경로로 다시 랜딩할 수 있다.

| 규칙 | 상태 |
| --- | --- |
| `INV-herdr-unseen-token` | **내용 유효, 이식됨.** `_new` 토큰만 attention으로 승격한다는 계약은 `herdr-core/src/sidebar.rs`가 단독으로 소유한다. 펫은 그 판정 결과를 버킷으로 묶을 뿐 토큰을 다시 읽지 않는다 |
| `INV-pet-state-off-main-thread` | **폐기 대상.** "sync Tauri 커맨드가 메인 스레드에서 돈다"는 Tauri 고유 제약이다. herdr-core의 세션 폴링은 전용 스레드에서 돌고 스냅샷은 콜백으로 전달된다 |
| `FACT-drag-native-loop`, `FACT-pet-window-macos` | 창 제약은 유효하나 Tauri 전제가 섞여 있다. `pet-window-macos.md` 참조 |
| `FACT-dev-runtime-instances` | **유효.** `dev-runtime.md`가 Swift 셸 기준으로 갱신되었다 |
| `FACT-status-model` | **유효, 이식됨** |
| `REG-window-position-offscreen` | **유효, 이식됨.** 사고 좌표는 Swift 테스트에 고정 |

## 4. 에셋 (`assets/pet-theme/default/`)

`themes/default`의 원본을 옮긴 것이다.
`theme.json`은 이식하면서 schema v2로 갱신했다: 코어가 낼 수 있는 13개 pose 전부를 매핑하고, 애니메이션 webp와 스프라이트 시트를 구분해 선언한다.
자세한 계약은 `theme-contract.md`에 있다.

blue slime 계열 정적 아트(`idle.png`, `attention-0..3.png` 등)는 default 테마가 참조하지 않는다.
`pet-assets.md`가 설명하는 캐릭터 교체용 소재이며, 이번 릴리스는 테마 1개만 번들한다.

## 5. 옮기지 않은 것

`apps/pet-app`, `web/`, `plugins/herdr-agent-pet`.
`herdr-pet` 저장소 자체의 은퇴는 이 구현의 검증이 통과한 뒤 사용자가 별도로 결정한다.
