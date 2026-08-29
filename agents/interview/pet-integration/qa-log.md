---
topic: "herdr-pet을 herdr-ide로 완전 통합"
status: "active"
where: "brownfield"
selected_packs: "ux, compatibility, data, operation, verification"
created_at: "2026-08-29"
updated_at: "2026-08-30"
question_count: 6
normalization_policy: "transcript-sync-with-checkpoint-backfill"
normalization_checkpoint_every: 10
---

# Interview Log: herdr-pet을 herdr-ide로 완전 통합

## Current Understanding

- 합의된 목표 한 줄: herdr-pet의 기능 전체(question payload 제외)를 herdr-ide 앱의 네이티브 부속 플로팅 펫 창으로 재구현해 herdr-pet 저장소를 은퇴 가능한 상태로 만든다.
- 통합 형태는 네이티브 완전 흡수(D-05): 자리표시자 PetWindow를 실제 펫(애니메이션 pose, 뱃지 행, _new attention, 드래그+클램프, 클릭 점프, 테마, 단축키/URL scheme 토글)으로 채운다. 범위는 전체 패리티, 작업 순서만 코어 5개 → 부가 2개 층위(D-06).
- 구현 성격: 코어 로직(pose 우선순위, 상태 집계, herdr 소켓 파싱)은 docs/ported-reference/의 Rust 원본을 herdr-core로 이식(테스트 포함), UI는 Tauri 웹뷰 JS를 SwiftUI로 재작성. 데이터 소스는 herdr-core가 이미 쓰는 herdr 로컬 API 소켓 polling 경로를 공유(D-13).
- question payload(#8)는 명시적 non-goal(D-07, 재방문 조건 기록). herdr-pet 저장소 정리는 설치 번들 스크린샷 검증 통과 후 별도 후속(D-12).

## Intake Cursor

- next_decision_id: D-20
- next_question: (owned by the live conversation until checkpoint)
- last_materiality_sweep: checkpoint 2
- outstanding_raw_entries: none
- next_checkpoint_at: Q16

## Transcript Sources

| Runtime | Session ID | Start ref |
| --- | --- | --- |
| claude | 189f4c2b-415e-4f87-95da-04d6794f6040 | eb9a809a-d1eb-4c17-a487-d61759ca3351 |

## Decision Register

| ID | Kind | Area | Decision / fact | Priority | Source / owner | Status | PRD mapping / revisit |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D-01 | fact | architecture | herdr-ide는 herdr-pet(Rust+Tauri)을 대체하는 저장소다. 지식 문서(status-model, pet-window-macos, theme-contract, ambient-signals, pet-assets 등)는 이미 이식됐고, 당시 결정(과거 인터뷰 D-14)은 'Rust 구현 재사용 안 함'이었다. 이 제약은 Electron 전제였고 본 인터뷰의 D-16이 공식 대체한다(코어 Rust 로직 이식 재개) | P0 | repo docs/PORTING.md:1-8 | resolved | 배경 fact. D-16이 일부 대체 |
| D-02 | fact | architecture | herdr-pet 저장소 구성: apps/pet-app(Tauri 데스크톱 펫), crates/herdr-core, plugins/herdr-agent-pet(herdr plugin, pane metadata로 question payload 노출), themes/(펫 아트+theme.json), web/. 최근 커밋: 글로벌 단축키, ambient subagent/background-task 뱃지 | P0 | repo ../herdr-pet, plugins/herdr-agent-pet/herdr-plugin.toml | resolved |  |
| D-03 | fact | UX/design | herdr-ide macos 셸에는 이미 PetWindow.swift(363줄: 화면 클램프, 타원 히트영역, 발자국 버튼 오버레이 - 자리표시자 수준)와 herdr-core model의 Pet 항목이 있다. herdr-pet의 애니메이션 펫/상태 뱃지/테마는 아직 없다 | P1 | repo macos/Sources/HerdrMacOS/PetWindow.swift, herdr-core/src/model.rs:119 | resolved |  |
| D-04 | fact | operation | 현재 설치된 herdr(~/.local/bin/herdr) plugin 목록에 herdr-agent-pet은 없다(agent-context-labels, file-viewer, official.browser만 설치됨). 펫 에셋 원본은 herdr-ide/assets/pet-theme/default/에 이미 복사되어 있다(PORTING.md 4절) | P1 | herdr plugin list --json, docs/PORTING.md | resolved |  |
| D-05 | decision | architecture | 통합 형태 = 네이티브 완전 흡수: PetWindow를 herdr-pet의 실제 기능으로 채워 herdr-ide 앱의 부속 플로팅 창으로 만들고 herdr-pet 저장소는 은퇴시킨다. 모노레포 동거(B)와 위젯 축소(C)는 기각 | P0 | user | resolved | R#: 통합 전체 방향 |
| D-06 | decision | scope | 범위 = 기존 herdr-pet 기능 전체 패리티(애니메이션 펫, 뱃지 행, _new attention, 드래그+클램프, 클릭 포커스, 테마, 글로벌 단축키+URL scheme). 작업 순서는 1~5를 먼저 end-to-end 관통 후 6~7을 얹는 층위 - 범위 축소가 아니라 순서 | P0 | user | resolved | R#/T#: 기능 패리티 + 작업 층위 |
| D-07 | decision | scope | question payload(#8)는 이번 범위에서 제외. herdr-agent-pet은 herdr 쪽에 설치되는 플러그인이라 herdr-ide 안으로 흡수 불가하고, 현재 설치도 안 돼 있어 죽은 기능. 재방문 조건: 펫에서 에이전트 질문 보기를 다시 원하면 플러그인 소스 이사 + 설치로 되살린다 | P1 | user | resolved | non-goal + revisit: 질문 표시 기능 재요청 시 |
| D-08 | decision | UX/design | 펫 클릭 = herdr-ide 메인 윈도우 포커스 + attention pane으로 점프. 복수 attention이면 가장 오래된 unseen부터. (기존 pid 기반 터미널 raise를 herdr-ide 문법으로 재해석) | P1 | user | resolved | R#/AC#: 클릭 동작 |
| D-09 | decision | UX/design | 펫 표시 정책: herdr-ide 실행 시 자동 표시, 글로벌 단축키와 URL scheme으로 토글, 마지막 표시 상태 기억(꺼둔 채 종료 시 꺼진 채 시작) | P1 | user | resolved | R#/AC#: 표시/토글 |
| D-10 | decision | UX/design | URL scheme 계약: 새 scheme은 herdr-ide:// 로 하고 명령은 기존과 동일한 hide/show/toggle, 관용 파싱(뒤 슬래시·셸 정규화 표기 허용, herdr-pet main.rs:906-913 및 테스트가 규범)을 유지한다. 구 herdr-pet:// 는 등록하지 않는다(은퇴 앱 scheme, 의도적 미지원) | P1 | user | resolved | R#/AC#: URL scheme 계약 + 미지원 non-goal |
| D-11 | decision | UX/design | 테마: theme.json 로더 구조는 그대로 구현하되 이번엔 저장소에 이미 있는 default 테마 1개만 제공(assets/pet-theme/default/, 59개 에셋). 검증: theme.json 로드·에셋 해석 단위 테스트 + 설치 번들에서 default 테마 아트가 실제 렌더된 스크린샷 | P2 | user | resolved | R#/V#: 테마 로더 테스트 + 번들 스크린샷 |
| D-12 | decision | operation | 은퇴 절차: 이번 구현 범위는 herdr-ide 쪽 완성까지. 설치 번들 스크린샷 검증 통과 후 로컬 herdr-pet 앱 제거와 저장소 아카이브는 별도 후속으로 사용자 확인 후 진행. 이번 범위에 herdr-pet 저장소 변경 없음 | P1 | user | resolved | non-goal(이번 범위) + 후속 운영 절차 |
| D-13 | fact | architecture | herdr-core는 이미 herdr 로컬 API 소켓으로 session snapshot polling과 pane.layout 조회를 한다(live.rs). 펫의 데이터 소스는 이 기존 경로를 공유한다. 단 _new 토큰 attention 판정과 상태 집계는 아직 없어 docs/ported-reference/herdr.rs, aggregate.rs, behavior.rs에서 Rust→Rust 이식이 필요하다 | P1 | repo herdr-core/src/live.rs:1-175, docs/ported-reference/ | resolved | T#: 코어 이식 작업 |
| D-14 | fact | UX/design | 글로벌 단축키의 herdr-pet 원 동작(패리티 계약): 사용자 캡처로 accelerator 문자열(예 'Alt+Cmd+P')을 설정 파일(v3)에 저장하고, 빈 값이면 아무 단축키도 등록하지 않는 것이 기본이다. 등록 충돌 시 명시적 오류 메시지. Swift 재구현은 이 동작을 따르고 event.code 기반 판정 함정(hotkey.js)을 준수한다 | P1 | repo ../herdr-pet/apps/pet-app/src-tauri/src/main.rs:74,1956, docs/ported-reference/hotkey.js | resolved | R#/T#: 단축키 패리티 |
| D-15 | assumption | UX/design | 펫 아트는 animated webp 프레임이다(herdr-pet 커밋 'restore the animated webp frames'). SwiftUI/AppKit에서의 재생 방식(CGImageSource 프레임 분해 등)은 구현 초기 스파이크로 검증하고, 실패 시 프레임 시퀀스 변환으로 대체한다 | P2 | agent default + ../herdr-pet git log, assets/pet-theme/default/ | resolved | Risk/T#: 애니메이션 재생 스파이크. revisit: 스파이크 실패 시 |
| D-16 | decision | architecture | 과거 인터뷰 D-14('herdr-pet Rust 구현 재사용 안 함')를 공식 뒤집는다: 그 결정은 Electron 전제였고, Swift 셸 + Rust 코어 피벗 이후에는 docs/ported-reference/의 코어 Rust 로직(pose 우선순위, 상태 집계, _new 판정, 테스트 포함)을 herdr-core로 이식 재개한다. UI는 여전히 SwiftUI로 재작성한다. D-01에 기록된 과거 제약을 대체한다 | P0 | user | resolved | ADR + T#: 코어 이식. D-01·D-13과 정합 |
| D-17 | fact | data lifecycle | attention 수명주기: _new(unseen)→plain(acknowledged) 전환은 herdr 서버 소관이다 - 사용자가 pane을 보면 herdr가 토큰을 plain으로 바꾼다. 펫은 스냅샷을 소비만 하고 자체 seen 상태를 저장하지 않는다(stateless 재계산). unseen이 하나도 없으면 attention이 아니므로 클릭은 메인 윈도우 포커스만 한다. pane 소멸·스냅샷 후퇴도 매 폴링 재계산으로 자연 해소된다 | P1 | repo docs/status-model.md:6-19, agents/rules INV-herdr-unseen-token, docs/ported-reference/herdr.rs 테스트 | resolved | R#/AC#: attention 계약 + 회귀 테스트 |
| D-18 | fact | states and recovery | 상태·복구 모델(패리티 계약): per-agent AgentStatus::Disconnected와 TopStatus::Disconnected가 herdr-pet aggregate.rs에 존재하고, disconnected 에이전트는 유지되되 waiting으로 집계되지 않는다. 소켓 부재는 live.rs의 명시 분기(서버 미실행)로 구분 표시한다. 일시 폴링 실패 시 마지막 유효 스냅샷 표시 + disconnected 승격은 이식되는 집계 로직이 결정한다. 로딩 초기 상태는 연결 전 disconnected 표기와 동일 취급 | P1 | repo ../herdr-pet/crates/herdr-core/src/aggregate.rs:12-61,222-252, herdr-core/src/live.rs:254-256 | resolved | R#/AC#: 상태 전이 + 테스트 |
| D-19 | fact | UX/design | 펫 위치 영속(패리티 계약): herdr-pet은 위치를 설정 파일에 저장하고, 복원 시 연결된 모니터 위에 있을 때만 신뢰한다(RESTORE_MARGIN, window.rs:302-316). 사고 좌표 [542720,163840] 픽스처가 회귀 테스트에 고정되어 있다. 표시 상태 기억(D-09)과 같은 영속 계층에서 위치도 복원한다 | P1 | repo ../herdr-pet/crates/herdr-core/src/window.rs:302-316,469, docs/ported-reference/window.rs | resolved | R#/AC#: 위치 복원 + 클램프 테스트 |

## Raw Q&A

### Q1: 통합 형태 선택 - A 네이티브 완전 흡수
- decision_ids: D-05
- route: mixed
- source_ref: claude:189f4c2b-415e-4f87-95da-04d6794f6040:f62b8cc4-a502-4961-81e9-11f70acc30c4
- asked: Preflight 결과를 먼저 공유드립니다. 질문하신 "통합할 수 있나?"에 대한 사실 관계가 이미 상당 부분 정해져 있었습니다.
  
  **확인된 사실:**
  - `docs/PORTING.md`가 명시적으로 "이 저장소(herdr-ide)는 herdr-pet(Rust+Tauri)을 대체한다"고 선언하고 있습니다. 지식 문서(status-model, pet-window-macos, theme-contract, ambient-signals 등)와 펫 아트 에셋 원본은 이미 이 저장소로 옮겨져 있고, Rust 코드는 재사용하지 않기로 결정되어 있습니다(이전 인터뷰 D-14).
  - macos 셸에는 이미 `PetWindow.swift`(363줄)가 있습니다. 다만 화면 클램프 + 타원 히트영역 + "발자국 버튼" 수준의 자리표시자이고, herdr-pet의 실제 기능(캠프파이어 애니메이션, pose 상태 모델, 뱃지 행, 테마, 글로벌 단축키, ambient 뱃지)은 아직 없습니다.
  - herdr-pet 쪽에는 Tauri 펫 앱 외에 `plugins/herdr-agent-pet`이라는 herdr 플러그인(pane metadata로 question payload를 펫에 노출)이 있는데, 현재 설치된 herdr 플러그인 목록에는 없습니다.
  
  그러니 답은 "가능하다"가 아니라 **"이미 그렇게 하기로 되어 있고, 뼈대까지 있다"**입니다. 남은 것은 통합의 형태와 범위 결정입니다.
  
  ---
  
  **Q1.** 통합의 최종 형태를 어떤 모습으로 원하시나요? 이 결정이 전체 스코프와 herdr-pet 저장소의 운명을 가릅니다.
  
  - **(A) 네이티브 완전 흡수 (추천)** - 지금의 PetWindow를 herdr-pet의 실제 기능(애니메이션 펫, pose/뱃지 상태 모델, 테마)으로 채워 herdr-ide 앱의 부속 플로팅 창으로 만들고, herdr-pet 저장소는 은퇴시킵니다. PORTING.md의 기존 결정(코드 재사용 없이 재구현)과 일치합니다.
  - **(B) 모노레포 이사만** - herdr-pet Tauri 앱을 이 저장소 안으로 옮기되 별도 앱으로 유지합니다. 통합이라기보다 동거라서, "대체한다"는 기존 결정과 충돌합니다.
  - **(C) 별도 펫 창 없이 기능만 흡수** - 플로팅 펫을 버리고 메뉴바 아이콘이나 IDE 내부 위젯으로 상태 표시만 가져옵니다.
  
  A를 추천합니다. 어느 쪽으로 갈까요?
- recommended: A 네이티브 완전 흡수 (PORTING.md의 기존 "대체" 결정과 일치)
- answer: A 네이티브 완전 흡수하게 할 수 있나? ㅇㅇ
  지금은 잘 안동작하는ㄱ ㅓㅅ 같아서 ㅎ
- immediate_notes: 사용자가 추천안 A를 수락 → D-05. "지금은 잘 안동작"은 자리표시자 PetWindow 상태로 설명됨(Q2 서두에서 안내). 구체 증상은 Q2·Q3에서 재질문했으나 무응답 - 추가 함정 증거 없음으로 종결.
- needs_normalization: false

### Q2: 기능 범위 - 기존 기능 전체 패리티
- decision_ids: D-06
- route: mixed
- source_ref: claude:189f4c2b-415e-4f87-95da-04d6794f6040:9ae40277-43c4-454d-9a19-60540f4d50da
- asked: 네, 가능합니다. A(네이티브 완전 흡수)로 확정하겠습니다.
  
  참고로 "지금 잘 안 동작"하는 건 당연한 상태입니다. 현재 herdr-ide의 PetWindow는 발자국 버튼만 있는 자리표시자라 실제 펫 기능이 아예 없고, herdr-pet 쪽 실제 기능은 아직 아무것도 이식되지 않았습니다.
  
  ---
  
  **Q2.** herdr-pet이 지금 가진 기능 인벤토리는 이렇습니다:
  
  1. **애니메이션 펫** - 캠프파이어 불꽃이 일하고/기다리고/돌아다니고/잠자는 pose (상태 모델 기반 우선순위)
  2. **뱃지 행** - 파랑/초록/노랑/빨강 에이전트 상태 뱃지 + ambient subagent/background-task 뱃지
  3. **`_new` 토큰 attention** - 안 본 이벤트만 attention으로 승격하는 계약
  4. **드래그 이동 + 화면 클램프** - 투명 always-on-top 창 (클램프는 이미 Swift로 구현됨)
  5. **클릭 시 해당 터미널로 포커스 이동**
  6. **테마 시스템** - theme.json + 아트 59개 (에셋은 이미 이 저장소에 있음)
  7. **글로벌 단축키 + URL scheme으로 펫 숨기기/보이기** (herdr-pet 최신 커밋)
  8. **question payload 노출** - herdr-agent-pet 플러그인이 pane metadata로 에이전트 질문을 펫에 전달
  
  이 중 **어디까지가 이번 통합의 필수 범위인가요?** 추천은 1~5를 필수(펫의 정체성 + 실사용 가치), 6은 default 테마 하나만 로드, 7~8은 이번 범위에서 명시적 후순위입니다.
  
  그리고 혹시 herdr-pet 앱을 실제로 띄워두고 쓰시다가 "잘 안 동작"을 겪으신 거라면, 구체적으로 뭐가 안 되던가요? (재작성 때 피해야 할 함정 목록에 넣겠습니다)
- recommended: 1~5 필수, 6은 default 테마만, 7~8은 명시적 후순위
- answer: 음 기존꺼 다해야긴 해야할듯? 어렵나? 그대로 코드 복붙 개념이 아냐?
- immediate_notes: |
  - decision: 축소 추천안을 기각하고 기존 기능 전체 패리티로 범위 확정 → D-06 (#8은 Q3에서 별도 제외)
  - reasoning: "기존꺼 다해야긴 해야할듯"
  - constraints: 없음
  - explicit non-goals: 이 시점엔 없음 (Q3에서 #8 제외로 조정)
  - verified facts: 난이도/복붙 질문에는 Q3 서두에서 사실로 답변 - 코어 Rust 로직(behavior/aggregate/herdr.rs)은 Rust→Rust 이식 가능, 웹뷰 UI(app.js 등)는 SwiftUI 재작성 필요
  - unresolved follow-up: "복붙 개념이냐" → Q3 서두에서 해소
  - source: user
- needs_normalization: false

### Q3: question payload(#8) 제외 - (b)
- decision_ids: D-07
- route: mixed
- source_ref: claude:189f4c2b-415e-4f87-95da-04d6794f6040:c1104ac6-b54a-4c1f-8467-0f5b2205e01f
- asked: 어렵지 않냐는 질문에 정확히 답하면: **절반은 복붙 개념이 맞고, 절반은 재작성입니다.**
  
  - **복붙에 가까운 절반 (코어 로직)**: pose 우선순위(`behavior.rs`), 상태 집계(`aggregate.rs`), herdr 소켓 파싱(`herdr.rs`)은 Rust이고, herdr-ide의 herdr-core도 Rust입니다. 참조 원본이 `docs/ported-reference/`에 테스트까지 포함해 이미 준비돼 있고, 소켓/`_new` 토큰 처리는 herdr-core에 이미 일부 들어가 있습니다(`live.rs`, `model.rs`). 과거 D-14가 "Rust 재사용 안 함"이라 한 건 Electron으로 가던 시절 결정이고, Swift 셸 + Rust 코어로 피벗한 지금은 오히려 되살릴 수 있습니다.
  - **재작성인 절반 (UI)**: herdr-pet의 화면은 Tauri 웹뷰 안 HTML/JS(`app.js`, `ambient-badges.js`, animated webp)입니다. herdr-ide는 SwiftUI라 이 부분은 복붙이 안 되고 Swift로 다시 그려야 합니다. 대신 함정 목록(`pet-window-macos.md`)과 아트 에셋 59개, 검증 픽스처가 전부 이 저장소에 이미 있어서 맨땅은 아닙니다.
  
  범위는 "기존 기능 전부(1~8)"로 잡되, 작업 순서만 1~5(펫+뱃지+attention+이동+포커스)를 먼저 끝까지 관통시키고 6~8을 얹는 층위로 가겠습니다. 범위 축소가 아니라 순서입니다.
  
  ---
  
  **Q3.** 그런데 8번(question payload)에는 구조적 제약이 하나 있습니다. `herdr-agent-pet`은 펫 앱이 아니라 **herdr(멀티플렉서) 쪽에 설치되는 플러그인**이라, 펫을 herdr-ide 안으로 흡수해도 이 플러그인만은 herdr-ide 안으로 넣을 수 없고 herdr에 별도로 설치되어 있어야 합니다. 그리고 지금 확인해보니 **현재 이 플러그인은 설치되어 있지 않습니다** - 즉 question payload 기능은 지금도 죽어 있는 상태입니다.
  
  어떻게 할까요?
  
  - **(a) 기능 유지 + 플러그인 소스를 herdr-ide 저장소로 이사** (추천) - herdr-pet 은퇴 후에도 플러그인이 살 집이 생기고, 설치해서 기능을 되살립니다.
  - **(b) 이번엔 제외** - 어차피 안 쓰고 있었다면 죽은 기능을 옮길 이유가 없습니다.
  
  실제로 펫에서 에이전트 질문이 뜨는 걸 쓰신 적 있나요? 없다면 (b)가 맞을 수 있습니다. (그리고 아까 못 들은 것: herdr-pet 앱을 쓰시다 "잘 안 동작"한 증상이 구체적으로 뭐였나요? 재작성 함정 목록에 넣겠습니다.)
- recommended: (a) 기능 유지 + 플러그인 소스를 herdr-ide로 이사
- answer: 우선 b로 하자 ㅇㅇ
- immediate_notes: 사용자가 추천 (a)를 기각하고 (b) 제외 선택 → D-07. "우선"이라는 표현에 맞춰 재방문 조건(질문 표시 기능 재요청 시 플러그인 소스 이사 + 설치)을 D-07에 기록. "잘 안 동작" 증상 재질문은 2차 무응답.
- needs_normalization: false

### Q4: 클릭 동작 - 메인 윈도우 포커스 + attention pane 점프 (a)
- decision_ids: D-08
- route: user-decision
- source_ref: claude:189f4c2b-415e-4f87-95da-04d6794f6040:c02f2c7a-5fd7-46fc-942c-233e469747d4
- asked: b로 확정합니다. #8(question payload)은 이번 범위에서 제외하고, "펫에서 에이전트 질문 보기를 다시 원하게 되면 플러그인 소스 이사 + 설치로 되살린다"를 재방문 조건으로 명시적 non-goal에 기록해두겠습니다.
  
  ---
  
  **Q4.** 클릭 동작(#5)의 의미가 통합 후 달라집니다. 기존 herdr-pet은 펫을 클릭하면 **herdr가 돌고 있는 터미널 앱을 pid로 raise**했습니다. 이제 펫이 herdr-ide의 부속 창이 되면 클릭의 목적지를 정해야 합니다:
  
  - **(a) herdr-ide 메인 윈도우 포커스 + attention 있는 pane으로 점프 (추천)** - "펫이 노랗게 변했네 → 클릭 → 바로 그 질문 걸린 pane 앞"이라는 원래 가치를 herdr-ide 문법으로 완성한 형태입니다.
  - **(b) herdr-ide 메인 윈도우 포커스만** - 지금 자리표시자가 하는 동작 그대로. 단순하지만 어느 pane인지는 직접 찾아야 합니다.
  
  attention이 여러 pane에 걸려 있으면 (a)는 가장 오래된 unseen부터 점프하는 걸 기본으로 하겠습니다. 어느 쪽으로 할까요?
- recommended: (a) herdr-ide 메인 윈도우 포커스 + 가장 오래된 unseen attention pane 점프
- answer: a로 가야지 ㅇㅇㅇㅇ
- immediate_notes: 사용자가 추천 (a)를 수락 → D-08. 복수 attention 시 가장 오래된 unseen 우선이라는 세부안까지 질문에 포함해 함께 수락됨.
- needs_normalization: false

### Q5: 표시 정책·URL scheme·테마·은퇴 절차 일괄 확정
- decision_ids: D-09, D-10, D-11, D-12, D-14
- route: user-decision
- source_ref: claude:189f4c2b-415e-4f87-95da-04d6794f6040:c8ebde19-db12-48a5-bb42-7673bbc8e3e8
- asked: a로 확정입니다. 클릭하면 herdr-ide 메인 윈도우 포커스 + 가장 오래된 unseen attention pane으로 점프.
  
  ---
  
  **Q5.** 남은 결정은 작은 것 4개라 묶어서 여쭙니다. 각각 추천안이 있으니 "다 ㅇㅋ"거나 번호로 바꿔주시면 됩니다.
  
  1. **펫 표시 정책** - herdr-ide 실행 시 펫 자동 표시, 글로벌 단축키와 URL scheme으로 토글, 마지막 표시 상태 기억(꺼둔 채 종료했으면 꺼진 채 시작). *추천: 이대로*
  2. **URL scheme 이름** - 기존 `herdr-pet://` 대신 herdr-ide 앱의 scheme으로 개명. 은퇴할 앱 이름을 물려받을 이유가 없어서요. *추천: 개명*
  3. **테마** - theme.json 로더 구조는 그대로 구현하되, 이번엔 이 저장소에 이미 있는 default 테마 1개만 제공. (herdr-pet도 실사용 테마는 default 하나였습니다) *추천: 이대로*
  4. **herdr-pet 은퇴 절차** - 이번 구현 범위는 herdr-ide 쪽 완성까지. 통합이 설치 번들 스크린샷 검증을 통과한 뒤에, 로컬 herdr-pet 앱 제거와 저장소 아카이브를 별도 후속으로 진행(그때 다시 확인). *추천: 이대로*
- recommended: 4개 항목 모두 추천안대로 (자동 표시+상태 기억 / scheme 개명 / default 테마 1개 / 은퇴는 검증 후 별도 후속)
- answer: ㅇㅇㅇ 그렇게 가자
- immediate_notes: 4개 추천안 일괄 수락 → D-09, D-10, D-11, D-12. D-14(단축키 사용자 캡처 방식 + event.code 함정 준수)는 이 배치의 표시 정책에서 파생된 P2 가정으로 함께 기록.
- needs_normalization: false

### Q6: 마감 확인 - Rust 이식 재개 확정 + URL scheme herdr-ide://
- decision_ids: D-16, D-10, D-11
- route: user-decision
- source_ref: claude:189f4c2b-415e-4f87-95da-04d6794f6040:e05be267-5c3a-4951-8838-0318b462b0cb
- asked: 게이트 발견사항 대부분은 herdr-pet 코드에서 사실로 닫을 수 있음을 확인했습니다(단축키는 원래 사용자 설정 방식에 미설정이 기본, deep link 명령은 hide/show/toggle, 위치 저장·disconnected 상태 모델도 기존 코드에 존재). 사용자 확인이 정말 필요한 것만 2개 남았습니다.
  
  ---
  
  **Q6 (마지막 확인 2개).**
  
  1. **과거 결정 뒤집기 확정** - 이전 인터뷰에서 "herdr-pet의 Rust 구현은 재사용하지 않는다"(당시 D-14)고 정했는데, 그건 Electron으로 가던 시절 결정입니다. 지금은 Swift 셸 + Rust 코어라서, 코어 로직(pose 우선순위·상태 집계·`_new` 판정, 테스트 포함)을 `docs/ported-reference/`에서 herdr-core로 **이식 재개**하는 게 맞습니다. 이 뒤집기를 공식 확정할까요? *추천: 확정 (UI는 여전히 SwiftUI 재작성)*
  
  2. **URL scheme 이름** - 기존은 `herdr-pet://hide|show|toggle`이었습니다. 새 이름을 **`herdr-ide://`**로 하고 명령(hide/show/toggle)과 관용 파싱은 그대로, 구 `herdr-pet://`는 등록하지 않음(은퇴 앱 scheme이라 미지원)으로 할까요? *추천: 이대로*
- recommended: 1) 과거 'Rust 재사용 안 함' 결정 공식 뒤집기(코어 이식 재개, UI는 SwiftUI 재작성) 2) 새 scheme herdr-ide:// + 명령 hide/show/toggle 유지 + 구 herdr-pet:// 미등록
- answer: ㅇㅇㅇㅇㅇ 괜찮은ㄱ ㅓㅅ 같아 고고
- immediate_notes: gap-audit 1차 BLOCK의 requiresHuman 발견 중 사용자 판단이 필요한 2건을 배치로 확인, 둘 다 추천안 수락 → D-16(D-01의 과거 제약 대체), D-10 구체화. 나머지 발견은 herdr-pet 코드·문서 증거로 fact(D-14 전환, D-17, D-18, D-19)와 검증 보강(D-11)으로 닫음.
- needs_normalization: false

## UX Scenario Cards

### UX-01: 펫으로 에이전트 상태 인지 후 attention pane으로 점프
- trigger: herdr-ide 사용 중 에이전트 pane에 unseen 이벤트(`_new` 토큰)가 생기거나 에이전트 상태가 바뀐다
- happy path: 펫이 pose(일함/기다림/배회/잠)와 뱃지 행(파랑/초록/노랑/빨강 + ambient subagent/background-task)으로 상태를 반영한다 → 사용자가 펫을 클릭한다 → herdr-ide 메인 윈도우가 포커스되고 가장 오래된 unseen attention pane이 선택된다
- state / failure: herdr 서버 미실행(소켓 부재)이면 펫이 연결 끊김 상태를 명시적으로 표시한다(live.rs의 socket-missing 분기를 그대로 노출, 조용한 기본값 금지). 개별 에이전트의 Disconnected는 유지되되 waiting으로 집계되지 않는다(D-18). unseen이 하나도 없으면 attention이 아니므로 클릭은 메인 윈도우 포커스만 수행한다(D-17). `_new`→plain 전환은 herdr 서버 소관이고 펫은 스냅샷을 stateless로 재계산만 한다
- recovery: 소켓이 다시 생기면 기존 polling 경로가 재개되어 펫 상태가 자동 복구된다. 일시 폴링 실패는 마지막 유효 스냅샷 표시 + 집계 로직의 disconnected 승격 규칙을 따른다
- proof: 설치 번들 실행 후 screencapture 스크린샷으로 pose/뱃지 렌더와 default 테마 아트 확인, 클릭 후 포커스된 pane 확인(스크린샷), `_new` 판정·집계·상태 전이는 ported-reference의 Rust 테스트를 herdr-core로 이식해 자동 검증, theme.json 로더는 단위 테스트(D-11)
- linked decisions: D-06, D-08, D-13, D-17, D-18, D-11

### UX-02: 펫 이동, 표시 토글, 재시작 복원
- trigger: 펫 드래그, 글로벌 단축키(사용자 캡처 설정, 미설정 시 등록 없음 - D-14) 또는 `herdr-ide://hide|show|toggle` 호출(D-10), herdr-ide 재시작
- happy path: 드래그로 원하는 위치에 두고, 단축키/URL scheme으로 숨기고 다시 부르며, 재시작하면 마지막 위치(연결된 모니터 위일 때만 신뢰 - D-19)와 표시 상태가 그대로 복원된다
- state / failure: 저장된 위치가 오프스크린 좌표(실제 사고 좌표 [542720, 163840]가 회귀 테스트에 고정됨)라도 클램프가 가시 화면 안으로 되돌린다. 꺼둔 채 종료했으면 꺼진 채 시작한다
- recovery: 클램프가 항상 가시 영역을 강제하므로 펫을 잃어버리는 상태가 존재하지 않는다
- proof: 클램프·위치 신뢰 단위 테스트(사고 좌표 픽스처), 재시작 후 위치/표시 상태 스크린샷, 단축키 캡처·재설정과 `herdr-ide://` 각 명령의 동작 스크린샷(관용 파싱 테스트 포함)
- linked decisions: D-09, D-10, D-14, D-19, D-03

## Evidence From Code, Docs, Or Research

- `docs/PORTING.md` - herdr-ide가 herdr-pet을 대체한다는 선언, 이식된 지식 문서/에셋/참조 코드 목록, 과거 결정 D-14·D-32·D-34의 요약
- `docs/ported-reference/{behavior.rs, aggregate.rs, herdr.rs, window.rs, hotkey.js}` - 이식 대상 원본(테스트 포함). hotkey.js의 event.code 함정, window.rs의 사고 좌표 픽스처
- `herdr-core/src/live.rs:1-175` - herdr 로컬 API 소켓 snapshot polling과 pane.layout 조회가 이미 구현됨. socket-missing 오류 분기 존재. `_new` attention 판정은 아직 없음
- `macos/Sources/HerdrMacOS/PetWindow.swift` (363줄) - 화면 클램프(PetPlacement), 타원 히트영역(PetHitRegion), 발자국 버튼 오버레이 자리표시자
- `assets/pet-theme/default/` - default 테마 원본 59개(theme.json + 아트), PORTING.md 4절
- `../herdr-pet` - apps/pet-app(Tauri, UI는 HTML/JS: app.js, ambient-badges.js, hotkey.js), plugins/herdr-agent-pet(herdr-plugin.toml: pane metadata로 question payload 노출), 최근 커밋에 글로벌 단축키·URL scheme·ambient 뱃지
- `herdr plugin list --json` (2026-08-29 실행) - 설치된 플러그인은 agent-context-labels, file-viewer, official.browser뿐. herdr-agent-pet 미설치
- `../herdr-pet/apps/pet-app/src-tauri/src/main.rs:74,904-913,1956` - accelerator 설정 계약(빈 값 = 미등록), `herdr-pet://hide|show|toggle` deep link와 관용 파싱 테스트
- `../herdr-pet/crates/herdr-core/src/aggregate.rs:12-61,222-252` - AgentStatus/TopStatus::Disconnected, disconnected는 waiting으로 집계되지 않음
- `../herdr-pet/crates/herdr-core/src/window.rs:302-316,469` - 저장 위치는 연결된 모니터 위일 때만 신뢰(RESTORE_MARGIN)
- `docs/status-model.md:6-51` - `_new`=unseen 계약, plain=acknowledged, pose 우선순위와 뱃지 행 의미

## Documented Domain Checks

- docs inspected: docs/PORTING.md, docs/status-model.md, docs/pet-window-macos.md, docs/dev-runtime.md (경로와 PORTING.md 요약 기준)
- canonical terms: pose(일함/기다림/배회/잠), attention, `_new` 토큰, 뱃지 행, ambient 뱃지, 클램프
- glossary or code conflicts: dev-runtime.md와 일부 규칙에 Tauri 전제(`cargo tauri dev`, `pgrep herdr-pet-app`)가 남아 있어 Swift 셸 기준으로 재작성 필요(PORTING.md 3절이 이미 지적)
- concrete scenarios tested: herdr 소켓 부재 시 표시, 오프스크린 좌표 복원, 복수 attention 클릭 순서
- docs mutation: 구현 시 dev-runtime.md·pet 관련 문서의 Tauri 전제를 갱신해야 함
- ADR candidate: "과거 D-14(Rust 재사용 안 함)는 Electron 전제였고, Swift 셸 + Rust 코어 피벗 이후 ported-reference의 Rust 코어 로직 이식을 재개한다"

## Checkpoint And Sweep History

### Checkpoint 1
- after_question: Q5
- normalized_entries: Q1, Q2, Q3, Q4, Q5
- register_changes: D-05~D-15 신규: 통합 형태(A 완전 흡수), 전체 패리티 범위+층위 순서, #8 non-goal, 클릭 점프, 표시 정책, URL scheme 개명, default 테마, 은퇴 후속 분리, 소켓 데이터 소스 fact, 단축키 가정, webp 재생 가정
- reopened_decisions: none
- highest_remaining_gap: animated webp의 SwiftUI 재생 방식은 구현 스파이크로 확인 필요(D-15로 기록, 사용자 결정 불요)

### Checkpoint 2
- after_question: Q6
- normalized_entries: Q6
- register_changes: 게이트 1차 BLOCK 대응: D-16(과거 Rust 재사용 금지 공식 대체), D-10 구체화(herdr-ide:// 계약+구 scheme 미지원), D-14 fact 전환(단축키 패리티), D-17(attention 수명주기), D-18(상태·복구 모델), D-19(위치 영속), D-11 검증 보강, D-01 정합
- reopened_decisions: none
- highest_remaining_gap: 없음 - 남은 P2는 D-15 webp 재생 스파이크뿐

## Audit History
