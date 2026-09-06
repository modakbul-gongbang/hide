---
topic: "herdr-runtime-release"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "공개 fork 저장소를 만들고 사용자가 내려받는 실행 바이너리를 릴리스로 배포하며, hide 저장소의 main에 머지될 변경을 PR로 밀어 올리는 외부 영향 작업이다."
source_intake: "current conversation"
created_at: "2026-09-06"
updated_at: "2026-09-06"
---

# PRD: herdr-runtime-release

## 1. Summary

hide의 코어는 `session.snapshot`에 `host`, `event_sequence`, `lineage`가 있는 Herdr API를 요구하지만, 공개된 Herdr 릴리스(stable v0.8.2, preview-2026-08-31) 어느 것도 그 API를 갖고 있지 않다.
그 API는 `~/projects/herdr` 작업 트리(master + 로컬 커밋 1개 + 미커밋 38파일)에만 있고, 사용자가 지금 돌리는 서버(`~/.local/bin/herdr`)가 바로 그 빌드다.
이 PRD는 그 작업 트리를 hide 조직의 fork(`modakbul-gongbang/herdr`)에 브랜치와 prerelease로 공개하고, hide가 그 릴리스를 핀으로 잡아 실제로 동작하는 앱을 만들며, 업스트림 제안용 브랜치와 Discussion 초안을 준비한다.
hide 쪽 작업은 PR #3(`herdr-runtime-owned`, 번들 Herdr만 실행하는 정책)의 연장선이며 그 브랜치 위에서 시작한다.

Approval checklist:

- 범위 경계: Herdr fork 공개 + hide 핀 전환 + 문서/고지 + e2e 회귀 스크립트 + 업스트림 제안 브랜치/초안. typify와 매핑 레이어는 별도 사이클 (3장).
- 업스트림 PR은 열지 않는다. herdrdev/herdr의 CONTRIBUTING이 비승인 기여자의 PR을 자동 종료하고 에이전트에게 거부를 요구한다 (3장 non-goal, 4.3 A1).
- 구조 변경: 공개 fork 저장소와 prerelease 채널 신설, manifest에 `repo` 필드 추가, 격리 e2e 검증 스크립트 신설 (5장).
- 검증 모드: build/static, automated behavior, desktop runtime(격리 소켓 e2e), external release state 모두 required-for-done (9장).
- delivery mode: `pr`. 사용자의 "모든 작업들 다 완수시켜"를 근거로 PR 생성/푸시/머지까지 위임된 것으로 기록 (4.3 A2).
- 사용자 Herdr 체크아웃과 실행 중 서버는 건드리지 않는다: 릴리스 커밋은 linked worktree에서 만들고, e2e는 항상 `HERDR_SOCKET_PATH` 격리 경로에서만 돈다 (11장).

## 2. Problem, Goal, And Users

사용자(hide 개발자이자 유일한 운영자)는 hide를 공개 저장소로 정리하는 중이다.
런타임 정책 리팩토링(PR #3)으로 hide는 번들된 Herdr만 실행하게 됐지만, 핀으로 잡을 수 있는 공개 Herdr가 없어 그 앱은 첫 실행에서 `snapshot is missing lineage`로 멈춘다.
목표는 "hide가 스스로 시작한 Herdr에 연결되어 pane을 그리는 앱"을 오늘 배포 가능한 상태로 만들고, 그 런타임의 출처를 저장소와 문서가 정직하게 설명하게 하는 것이다.

부차 목표는 로컬에만 있는 Herdr 변경을 fork에 공개해 유실 위험을 없애고, 업스트림에 제안할 준비물을 만드는 것이다.

### 2.1 User Scenarios

- SC1. 첫 실행: 사용자가 hide를 열면 hide가 번들 Herdr를 기본 소켓에 시작하고 연결하여 workspace와 pane을 그린다.
  Actors: 사용자 (hide 운영자).
  Primary path: 소켓에 서버가 없다. hide가 번들 바이너리를 검증하고 시작하며, 상태 표시가 연결됨을 보이고 pane이 렌더된다.
  Failure state: 소켓에 다른 프로토콜의 Herdr가 이미 떠 있다. hide는 두 프로토콜 번호와 `herdr server stop` 복구 방법을 상태 표시에 보이고, 그 서버를 건드리지 않는다.
  Recovery: 사용자가 그 서버를 멈추고 hide를 다시 열면 primary path로 돌아간다.
  Reach: 검증자는 dev 번들을 격리된 HOME, 상태 파일 경로, 짧은 절대 소켓 경로로 띄운다. failure state는 같은 소켓에 stable v0.8.2 서버를 먼저 띄워 만든다. 이 준비를 하는 스크립트가 T6이다.

## 3. Scope And Non-Goals

포함:

- Herdr 작업 트리를 fork 브랜치 `hide-runtime`에 커밋하고, preview 스킴의 버전 문자열로 macOS aarch64 바이너리를 빌드해 fork의 prerelease 자산 `herdr-macos-aarch64`로 공개한다.
- hide manifest가 릴리스 저장소(`repo`)를 갖고, bump/fetch 스크립트와 갱신 워크플로가 그 값으로 URL을 만들며, 문서 치환이 저장소 세그먼트까지 바꾼다.
- hide 핀을 그 릴리스로 옮기고, 계약(`contracts/herdr-api.schema.json`)을 그 바이너리가 답한 스키마로 재생성하며, 코어의 계약 필드 테스트를 포함한 모든 게이트와 스위트를 녹색으로 만든다.
- README, INSTALL, contracts/README, AGENTS, Apache 고지가 "hide는 자체 fork에서 빌드한 Herdr preview를 배포하며 그 이유는 업스트림에 아직 없는 API"임을 말한다.
- dev 번들을 격리 소켓에서 띄워 SC1의 primary/failure/recovery를 기계적으로 확인하는 스크립트를 저장소에 둔다.
- 업스트림 제안: 같은 변경을 현재 upstream master 위로 rebase한 브랜치 `upstream-proposal`을 fork에 push하고, 사용자가 직접 올릴 GitHub Discussion 초안을 run 디렉터리에 쓴다.

Non-goals:

- herdrdev/herdr에 PR, issue, Discussion을 여는 것. CONTRIBUTING.md는 `.github/APPROVED_CONTRIBUTORS`에 없는 계정(yansfil은 없음)의 구현 PR을 자동 종료하고, 에이전트에게 feature request나 구현 계획 제출을 거부하라고 명시한다. 결과: 사용자는 초안을 읽고 직접 Discussion을 올린다. 재검토 조건: 사용자가 승인 기여자 목록에 오르거나 메인테이너가 요청할 때.
- `hide-runtime` 릴리스 브랜치를 upstream master(92 커밋 앞섬, protocol 22) 위로 rebase하는 것. 오늘 사용자가 실제로 돌리는 소스를 그대로 배포하는 것이 안전하다. rebase는 `upstream-proposal`에서만 시도한다.
- typify 기반 타입 생성(build.rs)과 `serde_json::Value` 탐색을 대체하는 매핑 레이어. 핀이 정해진 뒤 별도 PRD 사이클로 진행한다.
- hide PR #1, #2의 rebase/머지와 branch protection 활성화. 이 run의 PR이 main에 들어간 뒤 메인 세션이 수행한다.
- Linux/x86_64 자산, Windows 자산. hide는 macOS aarch64만 배포한다.
- Herdr 저장소의 `just ci` 전체 실행. `just`와 `cargo nextest`가 이 머신에 없으므로 `cargo test --locked`를 그 대용으로 쓴다 (4.3 A9).
- 서명/공증. hide 앱 빌드가 번들 전체를 ad-hoc 서명하는 기존 동작을 유지한다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

- None required. fork 생성, 릴리스 업로드, PR 작업은 모두 인증된 `gh`(계정 yansfil, 조직 modakbul-gongbang의 admin)로 에이전트가 수행할 수 있다.

### 4.2 Human Decisions Before PRD Approval

- None required. 공개 fork와 릴리스 배포는 사용자가 대화에서 명시적으로 고른 선택지 1이고("1 풀고"), 나머지는 되돌릴 수 있는 에이전트 가정으로 4.3에 기록했다.

### 4.3 Decision Traceability For Fidelity Review

사용자 결정 (대화 원문):

- D1. "1 풀고": 로컬 Herdr 빌드를 내려받을 수 있는 자산으로 공개하고 그것을 핀으로 잡는다. 표현: R1, R2, R4, AC1-AC4, T1-T4.
- D2. "2병행하고": 로컬 Herdr 변경을 업스트림에 올린다. 표현: R8, AC9, AC10, T7. 업스트림 PR 자체는 A1에 따라 non-goal.
- D3. "최종적으로 모든 작업들 다 완수시켜": 이 사이클을 끝까지(검증, PR, 머지) 위임. 표현: delivery mode `pr` (A2), 12장 delivery 보고.
- D4. "codex astra medium으로 implementor 띄워서": Implementor는 Codex `gpt-6-astra`, reasoning effort medium. 표현: 디스패치 설정, context-only.
- D5. 이전 대화에서 사용자가 승인한 방향("오케이 우선 너 추천방향대로 싹다 리팩토링"): 런타임 정책은 "hide는 번들 Herdr만 실행, 정확한 핀, 프로토콜 불일치 메시지, 계약은 핀에서 파생". 이미 PR #3 브랜치에 구현되어 있으며 이 run은 그 위에서 시작한다. context-only.

에이전트 가정 (사용자가 사후에 거부할 수 있음):

- A1. 업스트림 PR/issue/Discussion을 열지 않고, rebase 브랜치 push와 Discussion 초안으로 대체한다. 근거: herdrdev/herdr CONTRIBUTING.md의 정책과 에이전트 지시. 표현: non-goal, R8, T7.
- A2. delivery mode `pr`와 머지 승인은 D3의 원문을 근거로 한다. `agents/config.json`의 `delivery.mode`를 `pr`로 바꿨다. 표현: 12장.
- A3. typify와 매핑 레이어는 이 PRD 이후 별도 사이클. 핀이 확정되기 전에 스키마에 의존하는 설계를 하지 않기 위해서다. 표현: non-goal.
- A4. fork는 hide가 사는 조직 `modakbul-gongbang` 아래 `herdr`라는 이름으로 만든다(현재 없음). 표현: R1, AC1.
- A5. 버전/태그 스킴은 업스트림 preview와 같은 형태를 따른다: `HERDR_BUILD_CHANNEL=preview`, `HERDR_BUILD_ID=<YYYY-MM-DD>-<12자 커밋>`, `HERDR_BUILD_COMMIT=<커밋>`, 태그 `preview-<YYYY-MM-DD>-<12자 커밋>`, prerelease, 자산 이름 `herdr-macos-aarch64`. 기존 `scripts/bump-herdr.sh`의 preview 태그 규칙(태그 마지막 세그먼트가 버전 문자열에 포함)을 그대로 만족한다. 표현: R1, AC2.
- A6. 릴리스 브랜치 `hide-runtime`은 사용자 작업 트리 내용 그대로(로컬 커밋 1e107419 + 미커밋 38파일)이며 rebase하지 않는다. 사용자 체크아웃은 건드리지 않고 linked worktree에서 커밋한다. 표현: R1, AC3, 11장.
- D6 (amend, 2026-09-06). 작업 트리가 `not_agent_backed` 오류 코드를 새로 넣으면서 옛 테스트 `agent_explain_rejects_hook_only_full_lifecycle_authority`의 기대값을 고치지 않아 `cargo test`가 실패한다. 사용자 답변 "테스트 기대값 커밋 허용 (Recommended)": 릴리스 브랜치 위에 그 테스트의 기대값을 `not_agent_backed`로 바꾸는 커밋 하나를 허용한다. 사용자 체크아웃은 그대로 두고, 비테스트 소스는 작업 트리와 동일해야 한다. 표현: R1, AC1, V5.
- D7 (amend, 2026-09-06). 기대값 커밋 뒤에도 `live_handoff_keeps_unmanaged_agent_name_bound_to_saved_session`이 작업 트리가 새로 넣은 `NotAgentBacked` 분기 때문에 실패한다. 사용자 답변 "Implementor가 동작까지 고침": 작업 트리가 깨뜨린 테스트를 통과시키기 위한 Herdr 소스 동작 수정 커밋을 허용한다. 같은 종류의 실패가 더 나오면 다시 묻지 않고 같은 방식으로 고친다. 각 수정은 별도 커밋으로 고친 테스트 이름을 밝히고, 테스트 제외나 기대값만 바꾸는 우회는 금지한다. 사용자 체크아웃은 그대로 둔다. 표현: R1, AC1, V5.
- A7. 갱신 워크플로(`herdr-update.yml`)는 계속 `herdrdev/herdr`의 stable 릴리스를 감시하고 `--repo herdrdev/herdr`로 bump를 제안한다. 업스트림이 필요한 API를 갖게 되는 순간 그 PR의 계약 필드 테스트가 녹색이 되어 신호가 된다. 표현: R3, AC6.
- A8. SC1의 e2e는 judged 스크린샷이 아니라 기계 검사로 증명한다(격리 소켓의 `api snapshot`과 앱 진단 로그). 스크린샷은 9.3의 사람 확인용. 표현: R6, AC7, AC8, V4.
- A9. Herdr 쪽 테스트는 `cargo test --locked`로 대신한다(`just`, `cargo nextest` 부재). 표현: R1, V5.
- A10. hide PR은 `herdr-runtime-owned`(a90f61b) 위에서 시작하므로 PR #3의 상위 집합이 되고, 머지 뒤 PR #3은 닫는다. 표현: 12장.

Principles intake: `~/projects/oh-my-principle` 커밋 35ab76c의 `engineering/principles.md`와 `practices/env.md`를 전부 읽었다.
design 도메인은 사용자가 작업을 수행하는 화면을 새로 만들거나 바꾸지 않으므로 적용하지 않는다.
번역한 규칙은 11장에 출처와 함께 적었고, 번역하지 않은 규칙은 없다.

## 5. Major Technical Structure Changes

- 외부 경계: 공개 fork 저장소 `modakbul-gongbang/herdr`와 그 prerelease 자산이 hide 런타임의 배포 출처가 된다. 이전에는 `herdrdev/herdr` 릴리스였다.
- 핀 manifest(`herdr-bundle.json`)가 `repo` 필드를 얻고, 모든 URL 파생(fetch, bump, 워크플로, 문서 치환)이 그 값을 쓴다. 단일 출처 게이트가 이 필드를 함께 감시한다.
- 검증 경계: dev 번들을 격리 소켓에서 띄워 SC1을 확인하는 스크립트가 저장소의 검증 도구로 추가된다.
- Herdr 저장소에는 새 브랜치 두 개(`hide-runtime`, `upstream-proposal`)와 태그가 생긴다. 코어 아키텍처 변경은 없다.

## 6. Requirements

- R1. Herdr 작업 트리의 변경 전체가 fork 브랜치 `hide-runtime`의 첫 커밋으로 존재하고, 그 위에 작업 트리가 깨뜨린 테스트를 통과시키는 데 필요한 최소 수정 커밋들(D6, D7: 테스트 기대값 또는 동작 수정, 테스트 제외 금지)을 더할 수 있으며, 그 tip에서 빌드된 바이너리가 fork의 prerelease 자산으로 공개된다. 바이너리가 보고하는 버전과 `api schema --json`은 사용자가 돌리는 서버와 같은 스키마를 답한다.
- R2. hide 핀 manifest는 `repo`를 갖고, `scripts/fetch-herdr-runtime.sh`, `scripts/bump-herdr.sh`, `.github/workflows/herdr-update.yml`이 그 값으로 URL을 만든다. bump는 `--repo <owner/name>`로 다른 저장소의 태그를 받을 수 있고, 문서 치환은 릴리스 URL 전체(저장소 + 태그)를 바꾼다.
- R3. 갱신 워크플로는 업스트림 stable 릴리스를 감시하며 `--repo herdrdev/herdr`로 bump를 제안한다.
- R4. hide 핀은 fork 릴리스를 가리키고, 계약은 그 바이너리가 답한 스키마이며, `the_pinned_herdr_promises_every_snapshot_field_the_replica_reads`를 포함한 코어 테스트, ffi 계약 테스트, Swift 테스트, 핀 단일 출처 게이트, 스키마 게이트가 모두 통과한다.
- R5. README, `docs/INSTALL.md`, `contracts/README.md`, `AGENTS.md`, Apache 고지가 런타임 출처(fork 태그와 커밋), 이유(업스트림에 없는 API), 업스트림 stable로 돌아가는 경로(갱신 워크플로)를 설명한다. 고지는 수정된 Herdr를 배포한다는 사실과 원 저작권/라이선스를 유지한다.
- R6. 저장소에 격리 e2e 스크립트가 있어, dev 번들을 격리된 HOME/상태 파일/소켓으로 띄우고 (a) hide가 번들 Herdr를 시작해 연결하고 pane이 존재함, (b) 다른 프로토콜의 서버가 소켓을 점유하면 프로토콜 불일치 상태가 기록되고 그 서버를 멈춘 뒤 다시 열면 연결됨을 확인한다. 스크립트는 자기가 만든 프로세스만 끝내고 사용자 기본 소켓은 절대 쓰지 않는다.
- R7. dev 번들은 새 핀의 바이너리를 담고, `build_dev_app.sh`와 `build-app.sh`는 변경 없이 새 릴리스 URL에서 내려받아 검증한다.
- R8. fork에 `upstream-proposal` 브랜치가 있다. 현재 upstream master 위로 rebase가 기계적으로 끝나고 `cargo test --locked`가 통과하면 그 상태를, 아니면 rebase 전 상태를 push하고 그 사실을 기록한다. `agents/runs/herdr-runtime-release/upstream-discussion.md`에 사람이 읽을 Discussion 초안(문제, 왜 중요한가, 브랜치 링크)을 쓴다.
- R9. 이 run의 모든 커밋, 브랜치, PR 텍스트, 릴리스 노트에 에이전트/모델/도구 귀속이 없다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | fork `modakbul-gongbang/herdr`가 존재하고 브랜치 `hide-runtime`이 있으며, 그 브랜치는 작업 트리 전체를 담은 커밋 위에 작업 트리가 깨뜨린 테스트를 고치는 커밋들만 얹은 것이고, 그 커밋들은 각각 고친 테스트 이름을 메시지에 밝힌다 | machine | - |
| AC2 | fork의 prerelease 자산 `herdr-macos-aarch64`를 내려받으면 sha256이 manifest의 `sha256`과 같고, `--version`이 manifest의 `version`과 같으며, 태그가 manifest의 `tag`와 같다 | machine | - |
| AC3 | 핀 바이너리의 `api schema --json`이 `contracts/herdr-api.schema.json`과 같고, 사용자가 돌리는 서버(`~/.local/bin/herdr`)가 답하는 스키마와도 같다 | machine | - |
| AC4 | 코어 테스트 전체(계약 필드 테스트 포함), ffi 계약 테스트, Swift 테스트, 핀 단일 출처 게이트, 스키마 게이트가 통과한다 | machine | - |
| AC5 | `scripts/bump-herdr.sh`가 현재 핀 태그에 대해 `unchanged`를 답하고, `--repo herdrdev/herdr v0.8.2 --dry-run`에 대해 herdrdev URL의 `planned`를 답한다 | machine | - |
| AC6 | 갱신 워크플로가 upstream stable 태그를 `--repo herdrdev/herdr`와 함께 bump에 넘긴다 | machine | - |
| AC7 | 격리 소켓에서 띄운 dev 번들이 번들 Herdr를 시작하고 연결하여, 그 소켓의 `api snapshot`이 핀 버전과 pane 하나 이상을 답한다 | machine | - |
| AC8 | 같은 소켓에 stable v0.8.2 서버가 떠 있으면 앱 진단이 `protocol_mismatch` 상태를 기록하고, 그 서버를 멈추고 다시 띄우면 AC7 상태로 돌아온다 | machine | - |
| AC9 | fork에 `upstream-proposal` 브랜치가 있고, rebase 여부와 테스트 결과가 run 기록에 남아 있다 | machine | - |
| AC10 | `agents/runs/herdr-runtime-release/upstream-discussion.md`가 문제, 영향, 제안 브랜치 링크를 사람이 읽을 분량으로 담고 구현 세부를 나열하지 않는다 | machine | - |
| AC11 | README, INSTALL, contracts/README, AGENTS, Apache 고지가 런타임 출처가 fork의 preview 빌드임과 그 이유, 업스트림 stable로 돌아가는 경로를 일관되게 설명하고, herdrdev 릴리스에서 내려받는다는 낡은 문장이 남아 있지 않다 | judged | 다섯 문서의 변경 diff와 최종 본문 |
| AC12 | 이 run이 만든 커밋, 브랜치, 릴리스 노트, PR 본문 어디에도 에이전트/모델/도구 귀속 문구가 없다 | machine | - |

## 8. PRD-Level Tasks

- T1. Herdr 작업 트리를 linked worktree에서 `hide-runtime` 브랜치로 커밋하고(사용자 체크아웃은 그대로), preview 스킴 버전으로 macOS aarch64 바이너리를 빌드해 `cargo test --locked`를 통과시킨다. Covers R1, AC1. Depends on: none.
- T2. fork `modakbul-gongbang/herdr`를 만들고 `hide-runtime`과 태그를 push하며, prerelease와 자산을 올리고, 자산의 digest/버전/스키마를 확인한다. Covers R1, AC2, AC3. Depends on: T1.
- T3. hide manifest에 `repo`를 더하고 fetch/bump/워크플로/단일 출처 게이트가 그 값을 쓰게 하며, bump가 `--repo`를 받고 문서 치환이 릴리스 URL 전체를 바꾸게 한다. Covers R2, R3, AC5, AC6. Depends on: none.
- T4. `scripts/bump-herdr.sh`로 핀을 fork 릴리스로 옮기고 계약을 재생성하여 모든 테스트와 게이트를 녹색으로 만든다. Covers R4, AC3, AC4. Depends on: T2, T3.
- T5. 다섯 문서와 Apache 고지를 런타임 출처와 이유에 맞게 고친다. Covers R5, AC11. Depends on: T4.
- T6. 격리 e2e 스크립트를 저장소에 만들고 dev 번들을 새 핀으로 빌드해 SC1의 세 경로를 통과시킨다. Covers R6, R7, AC7, AC8, SC1. Depends on: T4.
- T7. `upstream-proposal` 브랜치를 준비해 fork에 push하고 Discussion 초안을 run 디렉터리에 쓴다. Covers R8, AC9, AC10. Depends on: T2.
- T8. 커밋/브랜치/릴리스/PR 텍스트에서 귀속 문구가 없음을 확인하고 PR 본문 근거를 정리한다. Covers R9, AC12. Depends on: T5, T6, T7.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | hide 빌드, 게이트 스크립트, 귀속 검사 | none |
| automated behavior | yes | 코어/ffi/Swift 테스트, bump 스크립트 동작, Herdr 테스트 | none |
| desktop runtime | yes | SC1 격리 e2e | 스크린샷 최종 확인 |
| external release state | yes | fork, 브랜치, 릴리스 자산, 스키마 일치 | none |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R4, R7, AC4 | hide 코어 release 빌드, Swift 빌드, 핀 단일 출처 게이트, 스키마 게이트가 통과하고 dev 번들이 새 핀 digest의 바이너리를 담는다 | yes | no |
| V2 | automated behavior | R2, R3, R4, AC4, AC5, AC6 | 코어 테스트(계약 필드 테스트 포함), ffi, Swift 테스트가 통과하고 bump 스크립트가 같은 태그에 수렴하며 `--repo`로 다른 저장소 URL을 만든다. 보호하는 회귀: 핀과 계약이 다시 어긋나는 것, 저장소 세그먼트가 문서에 낡게 남는 것 | yes | no |
| V3 | external release state | R1, AC1, AC2, AC3, AC9 | fork/브랜치/태그/prerelease가 존재하고 자산의 digest, 버전, 스키마가 manifest 및 사용자 서버와 일치하며 `upstream-proposal`이 push되어 있다 | yes | no |
| V4 | desktop runtime | R6, AC7, AC8, SC1 | 격리 소켓 e2e 스크립트가 primary(자체 시작 후 연결, pane 존재), failure(불일치 상태 기록), recovery(서버 정지 후 재실행 시 연결)를 모두 통과한다 | yes | no |
| V5 | automated behavior | R1 | Herdr `hide-runtime` 커밋에서 `cargo test --locked`가 통과한다. 보호하는 회귀: 배포하는 소스가 자기 테스트를 깨는 것 | yes | no |
| V6 | build/static | R5, R9, AC10, AC11, AC12 | 문서/고지 diff가 AC11을 만족하고, Discussion 초안이 존재하며, 귀속 문구 검색이 0건이다 | yes | no |

### 9.3 Human Verification

- 격리 e2e가 남긴 연결 상태 스크린샷(run 디렉터리)에서 상태 표시 문구와 pane 렌더가 자연스러운지 사용자가 본다. 기계 검사는 소켓 상태와 진단 로그만 본다.
- `upstream-discussion.md`를 사용자가 읽고 직접 herdrdev/herdr Discussions에 올릴지 결정한다. 에이전트는 올리지 않는다.

## 10. Risks And Open Decisions

- 수정된 Herdr를 hide 조직 이름으로 배포한다. Apache-2.0은 허용하지만 고지에 수정 사실과 출처 커밋을 남겨야 한다 (R5).
- fork 릴리스는 hide가 관리하는 채널이 되어, 업스트림 보안 수정이 자동으로 오지 않는다. 갱신 워크플로가 stable을 계속 제안하지만 API가 맞을 때까지 그 PR은 빨간불이다 (A7).
- Herdr 빌드는 zig 패치 툴체인이 필요할 수 있다(`vendor/libghostty-vt`). 사용자가 2026-08-28에 같은 트리를 로컬에서 빌드했으므로 가능하다고 본다. 실패하면 T1이 막히고 OBSERVER_BLOCK으로 올린다.
- `upstream-proposal` rebase는 92커밋 차이로 충돌할 수 있다. 충돌이 기계적으로 풀리지 않으면 rebase 전 상태를 push하고 기록한다 (R8).
- 격리 e2e에서 소켓 경로를 잘못 주면 사용자의 실제 서버에 붙어 pane 제어를 가져간다. 2026-09-06에 실제로 일어났다. 11장의 격리 규칙이 이를 막는다.
- 코어 계약 상수(`HERDR_PROTOCOL_REVISION`)가 계약 파일에서 파생되므로 재생성 뒤 fixture 테스트가 흔들릴 수 있다. 흔들리면 fixture를 새 스키마에 맞춘다.

## 11. Implementation Guardrails

안전 규칙:

- 사용자의 Herdr 체크아웃(`~/projects/herdr`)의 브랜치, 인덱스, 작업 트리를 바꾸지 않는다. 커밋은 `git worktree add`로 만든 linked worktree에서 `git diff HEAD`를 적용해 만든다.
- 사용자의 실행 중 Herdr 서버(기본 소켓 `~/.config/herdr/herdr.sock`)에 `server stop`, pane/tab/workspace 생성·종료, 프로세스 kill을 하지 않는다. run이 만든 프로세스만 끝낸다.
- dev 번들은 항상 `env -i`, 격리 HOME, `--state-path`, 짧고 존재하지 않는 절대 `HERDR_SOCKET_PATH`로만 띄운다. `NSHomeDirectory()`는 `$HOME`을 무시하므로 HOME만 바꾸면 사용자 소켓에 붙는다.
- 로컬 `main`(다른 세션의 미푸시 커밋 40개)을 base로 쓰거나 push하지 않는다. base는 `origin/main`(= `public/main`)이고 run 브랜치는 `herdr-runtime-owned` 위에서 시작한다.
- herdrdev/herdr에 PR, issue, Discussion, comment를 만들지 않는다.
- 커밋, 브랜치, 태그, 릴리스 노트, PR 본문에 에이전트/모델/도구 귀속을 넣지 않는다.
- run 산출물(스크린샷, 로그, 다운로드 바이너리)은 `agents/runs/herdr-runtime-release/` 아래에만 둔다. `docs/verification/`, `docs/screenshots/`에는 아무것도 쓰지 않는다.
- 사용자 세션이 다른 세션의 worktree(`herdr-ide.worktrees/*`)를 쓰고 있다. 그 디렉터리를 건드리지 않는다.

범위 규칙:

- 범위를 넓히지 않는다. typify, 매핑 레이어, PR #1/#2 머지, branch protection은 하지 않는다.
- 새 서비스, 새 저장소(fork 하나 제외), 새 외부 호출을 추가하지 않는다.
- 릴리스 브랜치를 rebase하지 않는다.

원칙 번역 (engineering/principles.md, 커밋 35ab76c):

- 규칙 1: `repo` 필드를 더하면서 `herdrdev/herdr`를 하드코딩한 URL 문자열과 그것을 전제로 한 분기는 같은 변경에서 지운다. 호환 경로를 남기지 않는다.
- 규칙 2: `repo`는 manifest의 문자열 하나와 bump의 `--repo` 하나로 끝낸다. 여러 저장소를 다루는 추상화를 만들지 않는다.
- 규칙 4: fetch/bump는 저장소나 태그가 비어 있으면 기본값 없이 실패한다. e2e 스크립트는 연결 확인이 안 되면 타임아웃과 이유를 내고 실패한다.
- 규칙 7: 이미 있는 `check-herdr-pin-single-source.sh`, `bump-herdr.sh`, `fetch-herdr-runtime.sh`를 확장한다. 병렬 스크립트를 만들지 않는다.
- 규칙 9/10: e2e 스크립트는 단계별 결과를 이름 붙은 줄(예: `e2e.primary.connected`)로 출력해 실패 지점이 로그에서 보이게 한다.
- 규칙 11: 릴리스 생성, fork 생성, 브랜치 push, 캐시 채우기는 두 번 실행해도 같은 상태로 수렴한다(존재하면 확인만).
- 규칙 12: 새 테스트는 호출자가 관찰하는 결과(스크립트 출력, `gh api` 응답, 소켓 응답)를 단언한다.
- 규칙 13: 스키마 불일치를 개별 필드 grep으로 막지 않고 이미 있는 계약 필드 테스트(코어가 읽는 필드 목록 상수)로 막는다.
- practices/env.md: 새 환경 변수를 추가하지 않는다. `HERDR_SOCKET_PATH`, `HIDE_HERDR_CACHE`는 이미 코드가 계약을 갖고 있다.

## 12. Implementation Result Report Contract

구현 에이전트는 다음을 보고한다:

- status: `Done`, `Partially Done`, `Blocked`.
- 사용자에게 보이는 변화: hide가 자체 Herdr로 실제 동작하는지, 상태 표시 문구.
- Herdr 쪽 결과: fork URL, 브랜치 두 개의 커밋 해시, 태그, 릴리스 URL, 자산 digest, rebase 결과.
- hide 쪽 결과: manifest 값(repo/tag/version/sha256), 계약 protocol, 바뀐 스크립트/워크플로/문서 목록.
- 선택한 파일/모듈 구조와 책임 경계, 승인된 구조를 따랐는지.
- task 완료 상태, R/AC/V 커버리지, 모드별 검증 증거(명령, 종료 코드, 산출물 경로).
- 추가/수정한 자동 테스트와 각 테스트가 막는 회귀.
- delivery 결과: 브랜치, PR URL, CI 상태, 머지 커밋, PR #3 종료 여부.
- 가정과 편차(특히 A1의 업스트림 미제출, 손 디스패치).
- 남은 사람 확인 항목: 스크린샷, Discussion 초안 게시 여부.
- 미완 항목과 후속 후보(typify 사이클, 매핑 레이어 사이클, PR #1/#2 머지, branch protection).
