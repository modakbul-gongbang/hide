---
topic: "S0: 웹 셸 스파이크 4항목 + 현 Swift 앱 기준선"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "제품 코드를 바꾸지 않는 스파이크와 측정이지만, 결과가 34k줄 Swift 삭제 계획(우산 PRD D-06)의 진행/중단을 결정하므로 측정 절차의 정직성이 검토 대상이다."
source_intake: "agents/prd/web-shell-pivot/prd.md"
created_at: "2026-09-21"
updated_at: "2026-09-21"
---

# PRD: S0 웹 셸 스파이크 + 기준선

## Goal

hide의 단일 사용자가 Swift 셸을 웹 UI로 바꾸는 결정을 숫자로 내릴 수 있도록, 우산 PRD(`agents/prd/web-shell-pivot/prd.md`) D-06이 정한 스파이크 4항목을 측정하고 현 Swift 앱의 기준선을 같은 방법으로 기록한다.
산출물은 `agents/runs/web-shell-pivot-s0/REPORT.md` 하나와 그 근거 파일이며, 4항목이 모두 PASS일 때만 S1이 시작된다.
제품 코드(`herdr-core/`, `macos/`)는 바꾸지 않는다.

## Non-goals

- 제품 코드 변경: `herdr-core/`, `macos/`, `hide-*` 크레이트는 읽기만 한다. 스파이크 코드는 `spikes/web-shell/`에 두고 Cargo 워크스페이스 멤버로 넣지 않는다. 재검토: S1에서 `hided/`·`web/`로 새로 만든다.
- 토큰·Origin·인스턴스 락·유휴 종료 등 hided 제품 동작: 스파이크 hided는 격리 서버에서 토큰 없이 돈다. 재검토: S1.
- 느림 원인 조사: 기준선 숫자만 기록한다 (우산 PRD D-24).
- 디자인 토큰·shadcn·zustand: 측정에 필요 없는 UI는 만들지 않는다.
- CI 편입: 스파이크 코드와 측정 스크립트는 CI 게이트가 되지 않는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 4항목 게이트와 임계값은 우산 PRD D-06 그대로: ① 한글 IME V9 4체크 ② 키 입력→에코 p95 ≤ 현 Swift 앱 기준선 +5ms ③ Chrome 탭 + 스파이크 hided 합 RSS ≤ 400MB ④ 실사용 세션 캡처 델타 재생 120초 driven 구간에서 16.7ms 초과 프레임 ≤ 1%. 하나라도 FAIL이면 REPORT에 FAIL로 기록하고 끝낸다. 임계값을 이 PRD에서 바꾸지 않는다. | 우산 PRD D-06, D-35 |
| D-02 | 기준선은 현 설치된 Swift 앱(또는 같은 커밋의 dev 빌드)에서 `docs/PERFORMANCE_TESTING.md` 절차로 idle과 driven을 분리해 측정: 키 입력→에코 지연 분포(p50/p95/p99), CPU, RSS, 스냅샷 델타 크기·빈도. 측정 대상 세션의 프로젝트 수·pane 수·탭 수를 함께 기록한다. | 우산 PRD B2 |
| D-03 | 지연 측정은 양쪽에 같은 방법을 쓴다: 격리 Herdr 서버의 pane에서 고정 문자열을 타이핑하는 드라이버가 입력 시각을, 화면 측(SwiftTerm 뷰 / xterm.js 버퍼)이 에코 도착 시각을 기록해 차이를 낸다. 기존 `scripts/`의 지연 요약기가 있으면 재사용한다. 방법이 다르면 비교가 무효라 REPORT에 두 방법을 명시한다. | 우산 PRD D-06 "같은 측정법", engineering 7 |
| D-04 | 스냅샷 재생 픽스처: 실사용 세션의 델타 스트림(`herdr_core_snapshot` 출력)을 파일로 캡처하되 개인 내용을 담지 않도록 터미널 바이트는 길이만 남기고 치환한다. 캡처 파일은 `agents/runs/web-shell-pivot-s0/`에만 둔다. | 우산 PRD D-06 ④, AGENTS.md "Evidence Belongs Outside The Repository" |
| D-05 | 스파이크 hided: `spikes/web-shell/hided-spike/` 독립 Cargo 프로젝트. `herdr-core`를 path 의존으로 링크, axum WS 하나로 전체 스냅샷 + 델타 릴레이 + dispatch 수신. 격리 Herdr 서버(`HERDR_SOCKET_PATH`)에만 붙는다. | 우산 PRD D-01, D-14 |
| D-06 | 스파이크 web: `spikes/web-shell/web-spike/` Vite + React + TS + xterm.js 6 + addon-webgl, pane 하나. 라이브 모드(hided WS)와 재생 모드(캡처 파일)를 갖는다. 프레임 측정은 `requestAnimationFrame` 간격과 Chrome Performance 트레이스 둘 다 기록한다. | 우산 PRD D-02 |
| D-07 | IME V9 4체크는 사람이 실제 Chromium(Chrome 안정판)에서 수행한다. 항목: 후보창이 커서를 따라감, 조합 중 백스페이스가 DEL을 누출하지 않음, 인접 한글 두 글자 이상이 다음 셀을 덮지 않음, 영문이 즉시 에코됨. 작업자는 절차와 확인용 스크린샷 자리를 REPORT에 준비하고, 판정은 사용자가 채운다. 자동화된 CGEvent/AX 입력은 이 체크의 대체가 아니다. | 우산 PRD B3, 8월 pivot V9 |
| D-08 | REPORT.md 형식: 항목별 값·기준선·임계값·PASS/FAIL 표, 측정 환경(커밋, 앱 번들, Chrome 버전, 머신 부하), 방법, 원시 파일 경로, 미완 항목. 사람이 채워야 하는 IME 칸은 `PENDING_HUMAN`으로 남긴다. | 우산 PRD B1 |
| D-09 | 운영자의 실사용 Herdr 서버·pane·앱은 조작하지 않는다. 기준선 측정도 격리 서버에 붙인 Swift dev 빌드에서 하며, 실사용 세션에서는 스냅샷 캡처(읽기)만 한다. | AGENTS.md Performance Guide, 메모리 hide-e2e-isolation-needs-socket-path |
| D-10 | 전달: `agents/config.json` `delivery.mode: pr`. 커밋 대상은 `spikes/web-shell/` 코드와 측정 스크립트만; `agents/runs/`는 로컬. PR 본문에 REPORT의 표를 옮겨 적는다. | agents/config.json |
| D-11 | 원칙 intake: engineering 6/7(있는 것 재사용: 기존 지연 요약기, fake_herdr 픽스처), 12(측정은 관찰 가능한 값), design 12(한글 실제 폰트로 확인)를 적용. 스파이크라 engineering 8(버릴 기반 금지)은 우산 PRD가 "제품 코드 아님"으로 예외 처리한다. | `sasu principles list` 654485f |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | `agents/runs/web-shell-pivot-s0/REPORT.md`에 4항목 각각의 측정값, 기준선, 임계값, PASS/FAIL이 한 표로 있고, 표 아래에 측정 환경(커밋 SHA, 앱 번들 경로, Chrome 버전, 측정 시 머신 부하)이 적혀 있다. | D-01, D-08 |
| B2 | 기준선 절에 현 Swift 앱의 키 입력→에코 p50/p95/p99, idle CPU/RSS, driven CPU/RSS, 스냅샷 델타 크기·빈도가 있고 idle과 driven이 분리되어 있으며, 측정 세션의 프로젝트·pane·탭 수가 적혀 있다. | D-02, D-09 |
| B3 | 지연 측정은 Swift와 web 양쪽이 같은 드라이버와 같은 정의(입력 시각→에코 도착 시각)를 쓰고, REPORT가 그 방법을 한 단락으로 설명한다. 재실행 명령이 적혀 있어 다른 사람이 같은 숫자를 다시 낼 수 있다. | D-03 |
| B4 | `agents/runs/web-shell-pivot-s0/` 아래 캡처된 델타 스트림 파일이 있고, 그 안의 터미널 바이트는 길이 정보만 남긴 치환값이다. 리포지토리에는 캡처 파일이 없다. | D-04 |
| B5 | `spikes/web-shell/hided-spike/`에서 `cargo run`하면 격리 Herdr 서버에 붙어 WS로 전체 스냅샷 뒤 델타를 흘리고, `HERDR_SOCKET_PATH`가 없으면 시작을 거부하며 이유를 출력한다. 실사용 소켓에 자동으로 붙지 않는다. | D-05, D-09 |
| B6 | `spikes/web-shell/web-spike/`에서 `pnpm dev`하면 브라우저에 xterm.js pane 하나가 뜨고, 라이브 모드에서 격리 pane의 셸에 타이핑하면 에코가 보이며, 재생 모드에서 캡처 파일을 지정하면 120초 driven 구간을 재생한다. | D-06 |
| B7 | 재생 구간의 프레임 간격 분포(16.7ms 초과 비율)와 Chrome Performance 트레이스 파일 경로가 REPORT ④ 칸에 있다. | D-01, D-06 |
| B8 | Chrome 탭 RSS와 스파이크 hided RSS를 driven 구간 종료 시점에 각각 읽어 합산한 값이 REPORT ③ 칸에 있고, 읽은 방법(`ps`/Chrome task manager 등)이 적혀 있다. | D-01 |
| B9 | REPORT ① IME 칸은 4체크 절차와 스크린샷 자리가 준비된 `PENDING_HUMAN` 상태로 남아 있고, 작업자가 자동 입력으로 PASS를 채우지 않는다. | D-07 |
| B10 | ②③④ 중 하나라도 FAIL이면 REPORT 맨 위에 "S0 FAIL - S1 착수 금지"가 적히고, 어떤 항목이 얼마나 벗어났는지 한 줄씩 있다. 모두 PASS면 "S0 PASS (IME 판정 대기)"가 적힌다. | D-01, D-08 |
| B11 | `herdr-core/`, `macos/`, `hide-*` 크레이트에 diff가 없고, `scripts/verify-cargo.sh test`와 `scripts/verify-swift.sh test`가 변경 전과 같이 통과한다. | Non-goal |
| B12 | 실사용 Herdr 서버의 pane·탭·workspace가 S0 실행 전후로 같다(캡처는 읽기만). | D-09 |

## Technical structure

- 새 디렉터리 `spikes/web-shell/`: `hided-spike/`(독립 Cargo 프로젝트, `herdr-core` path 의존, axum WS), `web-spike/`(Vite React xterm.js), `measure/`(지연 드라이버·요약 스크립트, 기존 `scripts/` 요약기 재사용 우선).
- Cargo 워크스페이스, C ABI, Swift 셸, CI 워크플로는 변경 없음.
- 프로세스 경계: 격리 Herdr 서버 ↔ 스파이크 hided ↔ 브라우저(루프백 WS, 토큰 없음). 실사용 서버는 스냅샷 캡처 읽기 전용.

## Risks

- 측정 방법 불일치가 결론을 왜곡한다. B3이 같은 드라이버를 요구하고 REPORT가 방법을 적는 것이 유일한 방어.
- 머신 부하(다른 에이전트, 스왑)가 숫자를 흔든다. 측정 시 `sasu`/herdr 외 부하를 기록하고, 흔들리면 3회 반복해 중앙값을 쓴다.
- IME 판정은 사람 몫이다. 작업자는 `PENDING_HUMAN`으로 두고 끝내며, Observer가 사용자에게 절차를 전달한다.
- 실사용 서버 오염: D-09/B12가 경계. 격리 서버 기동은 `HERDR_SOCKET_PATH`를 명시한 별도 서버로.
- 사용자가 할 일: REPORT ① IME 4체크를 Chrome에서 직접 수행하고 결과를 채운다. 그 외 없음.
