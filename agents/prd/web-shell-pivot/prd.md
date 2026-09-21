---
topic: "hide 웹 셸 전환 우산 PRD: Swift 셸을 hided 데몬 + 브라우저 UI로 바꾸는 계획과 게이트"
status: "ready"
human_approval: "approved"  # user 2026-09-21 verbatim: 오 승인 이거 그러면 이제 PRD랑 task-plan.md를 /implement 하게 하고 herdr grok 4.6으로 개발하게 해줘(권한 물어보지 않고 근야 알아서 자율주행하도록 너가 설정해서 잘 틀어서 진행하렴
review_profile: "high-risk"
review_rationale: "34k줄 Swift 셸을 되돌리기 어렵게 삭제하는 계획이고, 브라우저가 붙는 로컬 데몬의 인증·Origin 경계를 새로 정의하며, 그 위에서 pane 닫기·파일 휴지통 이동 같은 파괴적 코어 이벤트가 실행된다."
source_intake: "agents/interview/web-shell-pivot/qa-log.md"
created_at: "2026-09-21"
updated_at: "2026-09-21"
---

# PRD: hide 웹 셸 전환 우산 PRD

## Goal

hide의 단일 사용자이자 개발자가 오픈소스 기여자를 받고 브라우저 수준에서 UI를 디버깅할 수 있도록, SwiftUI 셸(114파일 33,953줄)을 브라우저에서 돌아가는 웹 UI로 바꾸고 Rust 코어(`herdr-core`, 약 110k줄)는 `hided` 데몬으로 남겨 같은 UI가 후속 Electron 껍데기에도 그대로 붙게 한다.
이 문서는 계획·게이트·미룸 목록·공존 규칙을 담는 우산 계약이고, 실제 구현은 S0~S6 슬라이스 PRD가 각각 맡는다.
지금인 이유는 셸이 34k줄에서 더 커지기 전이 전환이 가장 싸고, 기록된 디버깅 고통(서명 번들, peekaboo, bundle-id 충돌)이 매 PR에 붙어 있기 때문이다.

## Non-goals

- Electron 껍데기: 이번 범위 밖, 별도 후속 태스크. 사용자 결과는 메뉴바·전역 단축키·Dock·투명 pet 윈도우가 없는 브라우저 탭. 재검토: S6 이후 Electron 인터뷰 (Q2).
- 미룸 표면(D-04 목록: pet, Browser pane, 대화 뷰어, Home/Overview 통계, 사용량 표시, 메뉴바/전역 단축키, 파일 드래그 드롭, 원격 파일 뷰어): S6 이후 Electron까지 부재. 사용자가 그 기간 동안 해당 기능 없이 일한다. 재검토: Electron PRD 인터뷰 (Q4, Q19).
- 원격 접속(다른 기기에서 hided에 붙기): 루프백 전용. 재검토: SSH 터널 또는 Electron 원격 태스크 (Q7).
- 느림 원인 조사: 별도 후속 태스크. S0는 기준선만 측정한다. 웹 전환이 느림을 고친다고 약속하지 않는다 (Q18).
- 코어 소유권 변경: pane 존재·분할 지오메트리·PTY는 Herdr, 탭·포커스·패널·텍스트 스케일·펼침 집합은 코어라는 현행 계약을 웹은 그대로 승계하고 셸 측 권위를 만들지 않는다 (`AGENTS.md` Runtime Architecture).
- 코어 수준 attach 소유권 조정(한 pane 한 클라이언트): 공존 전용 코어 변경이라 기각 (Q17).
- 디자인 리셋: Tailwind 기본 팔레트로 새로 시작하지 않는다. 토큰과 DESIGN.md 원칙은 승계 (Q12).
- S2~S5 표면별 상세 수용 기준: 각 슬라이스 PRD가 정의한다. 이 문서는 슬라이스마다 끝나는 조건 한 줄만 둔다 (Q16).
- 프로세스 스폰 규칙(`practices/process.md` 1: 단일 spawn 헬퍼), 구조화 로그(engineering 9)는 구현 제약으로 슬라이스 PRD가 받는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 1차 산출물은 `hided`(herdr-core를 감싼 Rust 데몬, WS로 dispatch·snapshot·change 통지 노출) + 순수 브라우저 웹 UI. Electron은 후속 태스크이되 경계는 Electron이 같은 UI를 그대로 붙일 수 있게 설계한다(Orca `orcad`/`--serve` 모델). Electron 1차, Tauri는 기각. | Q2 "Electron은 뒤로 미루자. 근데 그 구조를 고려해서 a를 하자" |
| D-02 | 스택: React 19 + TypeScript + Vite + zustand, 터미널 xterm.js + addon-webgl, Tailwind + shadcn/ui(Radix, `web/src/components/ui/`로 복사해 소유). ghostty-web, Svelte/Solid, Radix 직접, Tailwind 단독은 기각. | Q3, Q13 |
| D-03 | 에디터 CodeMirror 6(Markdown Live 편집 유지, `@codemirror/merge` diff, 언어팩 lazy import), PDF pdf.js, 이미지 `<img>`. Explorer 트리는 @tanstack/react-virtual 위에 직접 구현(코어 펼침 집합을 평탄화 행으로 1:1 매핑, 바뀐 행만 리렌더). Monaco, react-arborist는 성능(기동 비용, 리렌더 통제) 이유로 기각. | Q10 "퍼포먼스 관점에서 react-virtual이 더 나을것같아서.. editor도 퍼포먼스가 중요" |
| D-04 | Swift 셸은 일상 사용이 웹으로 옮겨진 뒤 S6에서 일괄 삭제. 삭제 게이트 범위: 터미널+탭+분할, 에이전트 사이드바, 프로젝트/체크아웃 전환, Explorer+에디터+뷰어, Changes/diff, Settings. 미룸 목록: pet, Browser pane, 대화 뷰어, Home/Overview 통계, 사용량, 메뉴바/전역 단축키/Dock, 파일 드래그 드롭, 원격 파일 뷰어. 즉시 삭제와 완전 패리티 대기는 기각. | Q4 "미루는것과 현재 하는것을 명확히 해서 나중에 누락안되게 기록해줘" |
| D-05 | S6 게이트: S1~S5 슬라이스 PRD가 각자 검증 PASS로 끝난 뒤 삭제 PRD를 사용자가 직접 승인하면 삭제. 최소 관찰 기간 없음, 판정자는 사용자. S6 이후 Electron까지 D-04 미룸 기능의 공백을 수용한다. | Q19 "swift 바로 걷어낼 수 있으면 걷어내는게 좋다 ... 나혼자써서 backward 신경 덜 써도 되고" |
| D-06 | Stage-0 스파이크 4항목, 하나라도 실패하면 멈추고 재검토: ① xterm.js 한글 IME V9 4체크 ② 키 입력→에코 p95가 현 Swift 앱 기준선 +5ms 이내 ③ Chrome 탭+hided 합 메모리 400MB 이하 ④ 실사용 세션에서 캡처한 스냅샷 델타 재생 중 120초 driven 구간에서 16.7ms 초과 프레임 1% 이하. 스파이크 축소·생략은 기각. | Q6, Q20 |
| D-07 | 슬라이스: S0 스파이크+기준선 → S1 hided+WS 계약+`hide` CLI+web 골격+터미널 1 pane+에이전트 사이드바 → S2 탭·분할·줌·프로젝트/체크아웃·단축키 → S3 Explorer+에디터+뷰어 → S4 Changes/diff → S5 Settings+진단 표면 → S6 Swift 삭제. 우산 PRD 하나 + 슬라이스별 PRD/런. 단일 런은 기각. | Q16 |
| D-08 | hided 접근 경계: 127.0.0.1만 바인드. 기동마다 32바이트 랜덤 토큰을 만들어 WS 핸드셰이크에 요구, 데몬 종료 시 상태 파일과 함께 삭제, 만료 없음. Origin 허용은 hided 서빙 origin 하나 + `hide dev`가 명시한 Vite origin 하나, 그 외 거부+진단 로그. 상태 파일 `~/.local/state/hide/hided.json` 0600. HTTP는 정적 자산 + `GET /health`만, 상태 변경은 전부 WS. Unix 소켓 전용, 토큰 없는 루프백은 기각. | Q7, Q20 |
| D-09 | 생명주기: `hide` CLI가 소유. `hide`는 인스턴스 락으로 실행 중 hided를 찾고 없으면 기동해 상태 파일(포트·토큰·PID)을 쓴 뒤 기본 브라우저로 URL을 연다. 마지막 클라이언트 해제 후 10분 유휴 뒤 자체 종료, `hide serve --keep-alive`로 상주. Herdr 서버는 자동 기동하지 않는다. launchd 상주, 수동 실행 전용은 기각. | Q15 |
| D-10 | 공존 규칙: 개발·스파이크는 격리 Herdr 서버(`HERDR_SOCKET_PATH`)에서, 실사용 세션은 한 번에 한 셸만 attach. `hide`는 기동 시 Swift 앱이 같은 소켓에 붙어 있으면 경고하고 진행 여부를 묻는다(자동 종료 없음). | Q17 |
| D-11 | 저장소: 같은 모노레포에 `hided/`(Cargo 워크스페이스 멤버) + `web/`(pnpm 워크스페이스), 후속 Electron은 `desktop/`. 루트 `pnpm-workspace.yaml` 추가, 기존 `scripts/*.mjs` 유지, `macos/`는 S6까지 공존. 별도 UI 저장소, `herdr-core/bin` 내장은 기각. | Q11 |
| D-12 | 디자인 토큰 권위를 `design/tokens.json` 하나로 옮기고 거기서 `HideTheme.swift`(공존 기간), web CSS 변수(shadcn 변수 포함), Pen 변수를 생성. `check-design-contract.mjs`에 web lane 추가. AGENTS.md/DESIGN.md의 "HideTheme.swift가 숫자 권위" 규칙은 같은 변경에서 갱신. DESIGN.md 셸 규칙(툴팁 단일 modifier, 컨테이너 정당화, 조용한 화면)은 web용으로 다시 쓰되 원칙은 유지하고, Pen Component 시트는 shadcn 변형과 1:1 대응을 목표로 하지 않고 토큰만 공유(가정). 권위 두 번 이동, 디자인 리셋은 기각. | Q12 |
| D-13 | 파일 조작·저장 의미론은 코어가 소유하고 웹은 승계: 삭제는 `path_trash`(휴지통, 영구 삭제 경로 없음), 이름 충돌은 거부, 확인 모달 문구·버튼·삭제 후 선택 이동은 explorer-delete-with-confirmation PRD D-02~D-06, dirty 상태는 코어 스냅샷. 웹은 새 파괴적 경로를 만들지 않는다. | 사실: `agents/prd/explorer-delete-with-confirmation/prd.md:34-39`, `CoreBridgeEditorSnapshot.swift:13` |
| D-14 | 가정: WS 계약은 핸드셰이크(토큰, 스키마 버전) → 전체 스냅샷 1회 → revision/terminal_sequence가 붙은 델타(현 `herdr_core_snapshot` 인자 모델). 클라이언트는 마지막 revision으로 재연결, 갭이면 서버가 전체 스냅샷으로 리싱크. 스키마 버전 불일치는 연결 거부. 백프레셔는 코어의 once-per-burst 통지를 그대로. 스키마는 `contracts/`에 두고 Rust 테스트와 TS 타입 생성이 같은 파일을 읽는다. | 가정: `herdr-core/src/ffi.rs:316-340`, `docs/ARCHITECTURE.md` 통지 규칙 |
| D-15 | 가정: 브라우저 연결 상태는 connecting → live → reconnecting(지수 백오프, 상한 30s) → gone. 재연결 시 탭·포커스는 코어 상태에서 복원, 미저장 에디터 버퍼는 브라우저 IndexedDB에 보관 후 코어 dirty 문서와 대조해 복원. 상태는 한 줄 배지, 상세는 진단 로그. | 가정: design 13, engineering 10 |
| D-16 | 가정: 릴리스 hided는 `web/dist`를 바이너리에 임베드해 단일 실행파일로 서빙, 개발은 Vite dev server가 hided WS로 프록시(`hide dev`). `hide` CLI는 hided와 같은 크레이트. 탭·분할·줌 지오메트리는 코어 projection 트리를 CSS grid로 그리고 resize 의도만 이벤트로 보내며, xterm.js fit은 pane 크기 확정 후 한 번, 그리드 크기를 attach 요청에 싣는다. | 가정: `docs/ARCHITECTURE.md:54-56`, `.github/workflows/release.yml` |
| D-17 | 가정: 검증 lane은 `web/` vitest + Playwright(Chromium, `fake_herdr` 픽스처의 hided에 붙는 e2e)를 `verify` 워크플로에 추가, hided는 `scripts/verify-cargo.sh`에 포함, Swift lane은 S6까지 유지. 문서는 ARCHITECTURE.md에 hided 절, AGENTS.md Repository Layout에 `hided/`·`web/` 추가, Swift 전용 문서는 S6에서 미룸 목록과 함께 Electron 입력으로 보존, `spikes/swift-shell-pivot/`은 동결. | 가정: `.github/workflows/verify.yml`, `herdr-core/src/fake_herdr.rs`, `docs/README.md` |
| D-18 | 전달: `agents/config.json` `delivery.mode: pr`. 이 우산 PRD는 코드 변경이 없어 PRD 커밋만 PR로 올린다. 각 슬라이스 PRD가 자기 PR을 만든다. | `agents/config.json` |
| D-19 | 원칙 intake: `oh-my-principle` 654485f의 engineering, design 두 문서를 전부 읽었다. engineering 3(가장 작은 end-to-end 먼저)은 D-07 슬라이스 순서로, 8(버릴 기반 금지)은 D-01 경계 설계로, 14/15(소유·상한)는 D-09와 B-rows로, design 13(조용한 화면)은 B13/B16으로 번역. design 11(구조적 후보 제시)은 슬라이스 PRD 단계로 위임. | `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | S0가 끝나면 `agents/runs/web-shell-pivot-s0/`에 4항목 각각의 측정값·기준선·PASS/FAIL이 기록되고, 하나라도 FAIL이면 S1 PRD는 착수되지 않고 사용자에게 재검토 결정이 돌아온다. | D-06, D-07 |
| B2 | S0 기준선은 현 Swift 앱에서 키 입력→에코 지연, idle/driven CPU, 메모리, 스냅샷 크기·빈도를 `PERFORMANCE_TESTING.md` 절차로 idle과 driven을 분리해 기록한다. | D-06 |
| B3 | 한글 IME V9 4체크(후보창 커서 추적, 조합 중 백스페이스가 DEL을 누출하지 않음, 인접 한글이 다음 셀을 덮지 않음, 영문 즉시 에코)는 실제 Chromium에서 사람이 수행하고 결과가 S0 기록에 남는다. | D-06 |
| B4 | 터미널에서 `hide`를 치면 실행 중 hided가 있으면 그것에, 없으면 새로 기동한 hided에 붙어 기본 브라우저가 토큰이 담긴 URL로 열린다. | D-09, D-08 |
| B5 | 마지막 브라우저 탭이 닫힌 뒤 10분이 지나면 hided가 스스로 종료하고 상태 파일과 토큰이 사라진다. `hide serve --keep-alive`로 띄운 hided는 종료하지 않는다. | D-09, D-08 |
| B6 | hided가 Herdr 소켓을 못 찾으면 "소켓 없음"과 "무응답"을 구분한 상태 행을 보이고 서버를 자동 기동하지 않는다. | D-09 |
| B7 | 허용되지 않은 Origin이나 잘못된 토큰의 WS 연결은 거부되고 진단 로그에 남는다. 브라우저 화면은 연결 거부 상태와 `hide` 재실행 안내만 보인다. 로컬의 다른 프로세스가 토큰 없이 코어 이벤트를 보낼 경로는 없다. | D-08 |
| B8 | hided의 HTTP 응답은 정적 자산과 `GET /health`뿐이며, 어떤 HTTP 요청도 코어 상태를 바꾸지 않는다. | D-08 |
| B9 | `hide` 기동 시 Swift 앱이 같은 Herdr 소켓에 붙어 있으면 경고와 함께 진행 여부를 묻고, 사용자가 거부하면 hided는 attach하지 않는다. 자동으로 Swift 앱을 종료하지 않는다. | D-10 |
| B10 | S1이 끝나면 왼쪽 에이전트 사이드바에 상태/attention 행이 보이고, 행을 클릭하면 중앙 xterm.js pane이 그 pane의 PTY로 바뀌고, 한글/영문으로 답하면 attention이 사라진다. 이것이 실제 Herdr 세션에서 동작하면 S1 종료. | D-07, D-02 |
| B11 | S2가 끝나면 하루 작업(탭·분할·줌, 프로젝트/체크아웃 전환, 키보드 단축키)을 브라우저 탭 하나로 시작할 수 있다. S3은 Explorer에서 파일을 열어 값 하나를 고쳐 저장할 수 있으면, S4는 커밋 전 diff 검토가 되면, S5는 Settings(원격 기기·Agents·단축키)와 진단 표면까지 웹에서 끝나면 종료. 각 슬라이스의 상세 흐름·빈 상태·실패는 그 슬라이스 PRD가 정한다. | D-07 |
| B12 | Explorer 트리는 코어 스냅샷의 펼침 집합·선택을 그대로 반영하고(터미널 경로 클릭 `reveal_path` 포함), Git 데코레이션은 코어의 changed-file 집합에서만 온다. 행·스크롤·hover·paint에서 Git 프로세스를 시작하지 않는다. | D-03, D-13 |
| B13 | Explorer 삭제는 확인 모달 뒤 휴지통 이동이며 실패 시 파일이 그대로 남고 이유가 행 아래 한 줄로 보인다. 저장 실패는 탭 배지로, 상세는 진단 로그. 웹에 새 파괴적 경로는 없다. | D-13 |
| B14 | WS가 끊기면 상단 한 줄 배지가 reconnecting을 보이고, 재연결되면 열린 탭·포커스가 코어 상태대로 복원되며 미저장 에디터 내용은 사라지지 않는다. hided가 종료된 것이 확인되면 gone 상태와 `hide` 재실행 안내를 보인다. | D-15, D-14 |
| B15 | 스키마 버전이 다른 클라이언트와 서버는 연결 시점에 거부되고, 한쪽만 아는 enum 값이 화면을 멈추는 일은 없다(현 `contracts/snapshot-wire-enums.json` 규칙 승계). | D-14 |
| B16 | 웹 슬라이스는 스냅샷 한 번에 하는 일을 현 Swift 셸보다 늘리지 않는다: 델타에서 바뀐 행만 리렌더하고, 통지 fan-out과 대기 작업 상한을 각 슬라이스 PRD가 명시한다. | D-03, D-14 |
| B17 | 색·간격·radius·타이포는 `design/tokens.json`에서 생성된 CSS 변수만 쓰고, `check-design-contract.mjs` web lane이 Tailwind 임의값과 인라인 색을 거부한다. 공존 기간 Swift 앱과 웹이 같은 토큰 값을 쓴다. | D-12, D-02 |
| B18 | S6 삭제 PR은 `macos/`, SwiftTerm·Highlightr vendor, build/sign 스크립트, Swift 토큰 생성기, Swift CI lane, 관련 AGENTS.md/docs 항목을 한 번에 제거하고, 미룸 목록(D-04)을 Electron 입력 문서로 남긴다. 이 PR은 S1~S5 검증 PASS 기록과 사용자의 승인 인용 없이는 열리지 않는다. | D-05, D-04, D-17 |
| B19 | 기여자는 `pnpm dev`(web)와 `cargo run -p hided`(또는 `hide dev`)로 브라우저에서 UI를 띄우고 Chrome DevTools로 디버깅할 수 있으며, `verify` 워크플로는 web lane(typecheck·lint·vitest·Playwright)을 포함한다. | D-11, D-17 |

## Technical structure

- 새 크레이트 `hided/`: `herdr-core`를 링크해 하나의 `Mutex<Runtime>`을 소유하고, axum(또는 동급) 위에 WS 엔드포인트(토큰 핸드셰이크, 전체 스냅샷 → 델타 스트림, dispatch 수신)와 정적 자산·`/health`를 서빙. `hide` CLI 바이너리 동거(인스턴스 락, 상태 파일, 브라우저 열기, 유휴 종료).
- 새 `web/` pnpm 워크스페이스: Vite React 앱. 코어 스냅샷을 zustand 스토어에 두고 표면별 셀렉터로 구독. 터미널 바이트는 WS 프레임으로 xterm.js에 feed.
- `contracts/`에 WS 메시지 스키마 추가, Rust 테스트와 TS 타입 생성이 공유. `design/tokens.json` 신설과 세 생성기(Swift·CSS·Pen).
- C ABI(`herdr_core.h`)와 Swift 셸은 S6까지 그대로. 코어 도메인·session_sync·wire 경계는 변경 없음.
- 프로세스 경계: 브라우저 ↔ hided(루프백 WS) ↔ Herdr 소켓. hided는 Herdr를 기동하지 않는다.

## Risks

- 두 번째 셸 재작성: 8월 pivot이 11일간 결정을 4번 바꿨다. 이 문서의 D-01~D-03은 S0 게이트 통과 전엔 삭제로 이어지지 않는다(D-06).
- 기능 공백: S6 이후 Electron까지 D-04 목록이 없다. 사용자가 명시적으로 수용했고(D-05), 미룸 목록이 Electron PRD 입력으로 보존된다.
- 성능 기대치: 웹 전환은 느림을 고치지 않는다(D-05 사실, 느림 원인 조사는 별도). B16이 병목을 그대로 옮기는 것을 막는 유일한 계약.
- 보안: 루프백+토큰+Origin은 로컬 사용자 권한의 다른 프로세스가 상태 파일을 읽는 것을 막지 못한다(0600은 같은 사용자에겐 열림). 이는 현 Herdr 소켓과 같은 신뢰 경계로 수용.
- 공존 기간 이중 attach(D-10): 규칙과 경고만 있고 강제가 없다. 위반 시 Herdr 렌더 비용 두 배가 느림으로 나타난다.
- 라이브 검증 경계: 스파이크와 e2e는 격리 Herdr 서버와 `fake_herdr` 픽스처에서만; 운영자의 실사용 세션·pane·앱은 조작하지 않는다.
- 사용자가 할 일: 없음. S6 직전 삭제 PRD 승인만 사용자 몫이다.
