---
topic: "herdr-protocol-compatibility"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "공개 Herdr 포크 릴리스와 Hide의 런타임 계약, 시작 복구 UI, 설치 앱을 함께 바꾸므로 잘못된 호환 판정이나 서버 전환이 실행 중인 터미널 작업에 영향을 줄 수 있다."
source_intake: "current conversation"
created_at: "2026-09-15"
updated_at: "2026-09-15"
---

# PRD: Herdr 프로토콜 호환성과 복구 안내

## Goal

Hide가 정확히 검증한 최신 Herdr 포크와 호환될 때는 기존 세션 또는 번들 서버에 정상 연결하고, 호환되지 않는 서버를 만났을 때는 어떤 변경 명령도 보내기 전에 멈춰서 사용자가 이해하고 안전하게 복구할 수 있는 안내를 보여준다.

## Non-goals

- upstream Herdr 0.9 서버에서 Hide 전용 host scope, ordered events, lineage 기능을 축소해 동작시키는 degraded mode는 만들지 않는다.
  사용자는 호환되는 수정 포크가 필요하며, upstream stable API가 해당 capability를 제공할 때 다시 검토한다.
- Hide 전용 소켓으로 기존 Herdr 세션과 분리하지 않는다.
  Hide가 사용자의 공용 Herdr 세션을 보여주는 현재 제품 구조를 유지한다.
- 실행 중인 Herdr 서버를 Hide가 자동 종료하거나 교체하지 않는다.
  현재 터미널이 종료될 수 있는 동작은 별도의 명시적 사용자 승인 없이는 수행하지 않는다.
- 앱 내부 자동 업데이트나 자동 서버 재시작 시스템을 새로 만들지 않는다.
  실행 중인 서버가 오래된 경우에는 안전한 수동 재시작 안내를 열고, Hide가 오래된 경우에는 Hide 릴리스 페이지를 연다.
- 전체 Herdr API를 capability 기반 degraded mode로 재설계하지 않는다.
  이번 변경은 정확한 계약 핀과 사전 호환성 차단을 완성하며, 안정 endpoint가 Hide의 필수 capability를 제공할 때 재검토한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 공용 Herdr 서버 구조를 유지하고 Hide와 서버의 호환 계약을 맞춘다. | 사용자 승인: "ㅇㅇ 그거 작업 바로하고"가 직전 제안의 공용 서버 유지와 호환성 갱신을 승인했다. |
| D-02 | upstream 0.9.0에 Hide 필수 포크 기능을 이식한 새 preview를 만들고, 다른 스키마가 같은 번호로 통과하지 않도록 프로토콜 23을 사용한다. | 저장소 및 릴리스 조사: upstream 0.9.0 protocol 22에는 host scope, ordered events, lineage, agent.new 계약이 없다. |
| D-03 | Hide는 새 preview 바이너리와 그 바이너리가 출력한 스키마를 `scripts/bump-herdr.sh`로 함께 핀한다. | 사용자 승인과 `docs/ARCHITECTURE.md`의 단일 핀 계약. |
| D-04 | Rust core가 제공하는 구조화된 Herdr 상태를 단일 readiness 근거로 삼고, `connected` 전에는 로컬 Herdr 변경 명령을 보내지 않는다. | 사용자 승인과 기존 `status.herdr.state` 구현. |
| D-05 | 프로토콜 불일치는 전용 네이티브 alert로 설명하고 raw JSON은 표시하지 않는다. | 사용자 결정: "친절한 오류 문구로 해야겠네". |
| D-06 | protocol 숫자를 비교해 오래된 구성요소를 안내한다. 실행 중인 Herdr가 오래되면 `Open Restart Guide`, Hide가 오래되면 `Open Hide Releases`를 제공하고, 판별할 수 없으면 진단 복사만 제공한다. 어느 상태에서도 Herdr 종료나 재설치를 자동화하지 않는다. | 전체 작업 위임 범위 안의 에이전트 소유 가정: 번들 런타임은 Hide가 소유하고, 사용자 질문 "그러면 herdr reinstall 하라고 시키는게 낫나? 어떤 게 나은것같아?"에 대해 파괴적 자동 교체 없이 오래된 구성요소만 정확히 지목하는 방향을 선택했다. |
| D-07 | 기존 generic CLI 실패도 JSON envelope의 message를 추출하고 파싱 불가한 경우에만 원문으로 되돌아간다. | 에이전트 가정: 기존 `HerdrErrorEnvelope` 재사용으로 같은 raw JSON 실패 유형을 막는 가역적 구현 선택. |
| D-08 | 아직 원격에 없는 packaged resource 접근 수정도 함께 포함해 머지된 최신 main 빌드가 실제 설치 경로에서 실행되게 한다. | 사용자의 이전 "해결해" 승인과 현재 `origin/main`의 재현된 `Bundle.module` 크래시. |
| D-09 | PR을 만들고 필수 CI와 리뷰에 문제가 없을 때 squash merge한 뒤 최신 main을 다시 빌드하고 설치한다. | 사용자 결정: "PR 올려서 문제없으면 머지하고 최신상태로 빌드까지 다 해줘". |
| D-10 | engineering 및 design 원칙 문서 전체를 커밋 `653c46267c79892316ab7e8ff91f3a9a7d1561fc`에서 적용한다. | `agents/config.json` 원칙 입력과 `sasu principles list --json`. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | `/Applications/hide.app`을 열면 패키지 리소스 경로 때문에 종료되지 않고 기본 창이 표시된다. | D-08 |
| B2 | 서버가 없으면 Hide가 검증된 번들 Herdr를 공용 소켓에 시작하고 연결된 상태와 기존 작업 화면을 표시한다. | D-01, D-02, D-03 |
| B3 | 같은 protocol 23과 필수 계약을 가진 Herdr 서버가 이미 실행 중이면 Hide는 그 서버와 기존 세션에 연결한다. | D-01, D-02, D-04 |
| B4 | 서버가 없거나 아직 연결을 확인하지 못한 시작 구간에는 터미널, 채팅, 에이전트 생성 요청이 실행되지 않고 초기화 상태를 유지한다. | D-04 |
| B5 | protocol 또는 필수 계약이 맞지 않으면 Hide는 workspace, terminal, chat, agent 생성 명령을 보내지 않고 마지막 정상 화면을 보존한다. | D-02, D-04 |
| B6 | 프로토콜 불일치 alert는 protocol 숫자를 비교해 `Restart Herdr when your work is safe`, `Hide needs an update`, 또는 판별 불가용 `Hide and Herdr aren’t compatible` 제목과 해당 설명을 표시하며 작업을 만들지 않았음을 사람이 읽을 수 있는 문장으로 설명한다. | D-05, D-06 |
| B7 | 실행 중인 Herdr가 오래되면 `Open Restart Guide`, Hide가 오래되면 `Open Hide Releases`, 어느 경우에도 `Copy Diagnostics`와 `OK`가 제공되고 각 동작이 이름 그대로 수행된다. 판별 불가 상태에는 잘못된 업데이트 링크를 제공하지 않는다. | D-06 |
| B8 | 복사되는 진단 정보에는 오류 code, Hide가 요구하는 protocol, 서버 protocol, Hide와 Herdr 버전처럼 실제로 확인된 값만 포함하며 개인 콘텐츠나 비밀은 포함하지 않는다. | D-05, D-06, D-10 |
| B9 | 프로토콜 불일치 alert와 상태 표시에는 CLI JSON envelope 또는 중괄호로 감싼 raw payload가 노출되지 않는다. | D-05, D-07 |
| B10 | 프로토콜 이외의 CLI 오류는 기존 흐름을 유지하되 envelope의 사람이 읽을 수 있는 message만 alert에 표시한다. | D-07 |
| B11 | 앱 실행 뒤 서버가 handoff 또는 재시작되어 호환성이 달라져도 다음 변경 요청 전에 최신 core readiness가 다시 적용되어 안전하게 차단된다. | D-04 |
| B12 | alert의 제목, 본문과 버튼은 macOS 접근성 트리에서 읽을 수 있고 실제 Inter/시스템 글꼴에서 잘리지 않는다. | D-05, D-06, D-10 |
| B13 | Hide가 호환되지 않는 서버를 발견해도 해당 서버나 그 pane 프로세스를 종료, 재시작 또는 변경하지 않는다. | D-06 |
| B14 | 설치 안내와 런타임 계약 문서는 새 포크 태그, 버전, 프로토콜과 복구 절차를 동일하게 설명한다. | D-02, D-03, D-09 |
| B15 | 새 Herdr preview와 Hide PR은 테스트 및 리뷰 결과가 현재 head와 일치할 때만 공개·머지되며, 머지 후 최신 main 설치본의 서명, 실행 파일 동일성, 단일 PID와 실제 창 렌더링이 확인된다. | D-09 |

## Technical structure

- `modakbul-gongbang/herdr`의 upstream 0.9.0 기반 별도 worktree에 기존 Hide 필수 계약을 포팅하고 protocol 23 preview 바이너리와 스키마를 하나의 릴리스 자산으로 만든다.
- Hide의 manifest, 계약 스키마, 생성 wire 타입은 그 정확한 릴리스에서 파생하며 임의의 프로토콜 상수나 별도 버전 출처를 추가하지 않는다.
- Rust core의 `status.herdr`가 호환성과 readiness의 단일 출처로 남고, Swift shell은 그 typed state를 모든 로컬 Herdr 변경 진입점의 공통 정책으로 사용한다.
- 기존 CLI 오류 envelope 파서를 공통 typed 오류로 확장하고 전용 protocol mismatch notice가 title, message, actions, diagnostics를 소유한다.
- 기존 macOS alert와 `ExternalBrowser`, clipboard 경로를 재사용하고 새 디자인 토큰이나 alert 컴포넌트를 만들지 않는다.
- `DESIGN.md`와 `design/hide.pen`의 Startup protocol mismatch 화면이 구현 전 목표와 머지 시점의 as-built 상태를 함께 소유한다.

## Risks

- Herdr 필수 포크 변경은 upstream 0.9.0과 충돌할 수 있다.
  별도 clean worktree에서 포팅하고 Herdr 전체 테스트와 스키마 비교가 통과하기 전에는 태그나 릴리스를 공개하지 않는다.
- protocol 23 preview가 배포돼도 현재 실행 중인 upstream protocol 22 서버는 호환되지 않는다.
  Hide는 이를 친절하게 차단하고 서버를 자동 변경하지 않으며, 실제 설치 검증은 운영 서버와 분리된 소켓에서 수행한다.
- 공개 prerelease는 외부 효과다.
  사용자의 이번 one-shot 승인 범위로 생성하되 자산 digest, 버전, 스키마와 소스 commit이 일치하지 않으면 Hide 핀과 PR을 진행하지 않는다.
- readiness가 지나치게 엄격하면 정상 초기화 중 사용자 동작이 거절될 수 있다.
  `initializing`, `connected`, 명시적 failure를 구분하고 실제 연결 전 명령이 실행되지 않는 caller-observable 테스트를 둔다.
- alert copy와 버튼이 기존 generic notice를 깨뜨릴 수 있다.
  protocol 전용 상태와 generic fallback을 분리하고 macOS 네이티브 스크린샷 및 접근성 트리로 검증한다.
- 사용자 기본 Herdr 서버와 다른 작업 세션은 이번 구현·검증에서 변경하지 않는다.
  전역 서버 전환이 필요하면 별도 명시적 승인 뒤에만 수행한다.
