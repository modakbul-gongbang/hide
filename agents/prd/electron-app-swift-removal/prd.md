---
topic: "Swift 셸 삭제와 Electron 배포 앱 전환 (우산 PRD S6 + 패키징)"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "34k줄 Swift 셸과 C ABI를 한 PR에서 되돌리기 어렵게 삭제하고, 릴리스 산출물과 설치 경로(배포 앱, 서명, Herdr pin 위치)라는 공개 계약을 바꾼다."
source_intake: "agents/interview/electron-app-swift-removal/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: Swift 셸 삭제와 Electron 배포 앱 전환

## Goal

hide의 단일 사용자이자 개발자가 SwiftUI 셸 없이 Electron 앱 하나로 hide를 설치하고 쓴다.
우산 PRD(`agents/prd/web-shell-pivot/prd.md`) S6이 약속한 삭제를 지금 한 PR로 실행하고, `desktop/`을 유일한 배포 앱으로 만들어 릴리스 zip을 Finder에서 열면 PATH 없이 데몬이 뜨고 셸이 붙게 한다.
사용자의 말: "swift 코드 + 테스트 + 문서 다 없애버릴거고 electron으로 완전히 마이그레이션 하는것까지."

## Non-goals

- 우산 PRD D-04 미룸 기능(pet 윈도우, 메뉴바/전역 단축키/Dock 배지, 파일 드래그 드롭, 원격 파일 뷰어)은 만들지 않는다. 사용자는 Electron 앱에서 그 기능 없이 일한다. 재검토: 사용자가 그중 하나를 요청할 때 그 항목의 PRD로 (D-04, D-15).
- 공증(notarization), 자동 업데이트, installer(dmg/pkg), electron-builder 도입은 하지 않는다. 첫 실행은 지금처럼 Gatekeeper 절차를 거친다. 재검토: 외부 사용자 배포가 목표가 될 때 (D-09).
- Swift 앱 전용 저장물(UserDefaults, `~/Library/Application Support/hide`의 theme.json·pet-theme·instances·host-helper)을 읽거나 변환하거나 지우지 않는다. 디스크에 남는다. 재검토: 없음 (D-17).
- 코어 도메인·session_sync·wire 경계, hided의 WS/HTTP 계약, 웹 셸의 화면은 바꾸지 않는다 (D-13).
- 이 PR이 머지되기 전에는 다른 UX 태스크를 시작하지 않는다 (D-05).
- engineering/principles.md 규칙 1은 이 PRD 전체의 형식이고(마지막 소비자가 사라지는 코드는 같은 변경에서 지운다), 규칙 8은 패키징에서 임시 경로를 두지 않게 한다. design/principles.md는 새 화면이 없어 번역할 규칙이 없다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 삭제 인벤토리는 `macos/` 전체(279 .swift, 33,575줄, Vendor/SwiftTerm·Highlightr, CHerdrCore, Tests, VerificationFixtures, scripts, Resources, AGENTS.md·CLAUDE.md), `scripts/build-app.sh`, `scripts/verify-swift.sh`, `macos/scripts/*`, `pr.yml`의 swift job, `release.yml`의 Swift 단계, macos/ 경로를 읽는 check 스크립트 27개, Swift를 서술하는 문서 12개다. | qa-log D-01 (저장소 사실) |
| D-02 | 위 인벤토리에 더해 C ABI(`herdr-core/src/ffi.rs`, `include/herdr_core.h`, `tests/ffi_contract.rs`, `tests/ffi_smoke.c`, staticlib crate-type), `spikes/swift-shell-pivot/` 전체, `hided/src/coexist.rs`(우산 B9 Swift 공존)와 그 호출·테스트를 한 PR에서 지운다. 부분 삭제·점진 삭제는 기각. | 사용자: "swift 코드 + 테스트 + 문서 다 없애버릴거고", "C ABI와 spikes: 둘 다 삭제" (qa-log D-02, D-13) |
| D-03 | Electron(`desktop/`)이 유일한 배포 앱이다: 패키지된 hide.app이 release `hided`(web/dist 내장)·`hide` CLI·pinned Herdr 바이너리를 Resources에 내장하고, Finder에서 열면 PATH 없이 데몬을 띄워 셸을 보인다. "삭제만, Electron은 PATH의 hide에 attach"는 기각. | 사용자: "삭제 + Electron이 배포 앱" (qa-log D-03) |
| D-04 | 우산 D-04 미룸 기능은 범위 밖이며 삭제 PR이 "Electron backlog: Swift-only features" 이슈 하나를 열어 항목별로 적고 그 번호를 docs/README.md와 PR 본문에 적는다. "파일 드롭+Dock 배지 포함", "pet까지 포함"은 기각. | 사용자: "전부 제외, 백로그 이슈로만" (qa-log D-04, D-15) |
| D-05 | 실행은 /please 파이프라인, Claude Implementor, PR 배포(`agents/config.json` mode pr). Observer는 verify PASS + CI 초록 + 리뷰 Fix now 없음이면 사용자 확인 없이 머지한다. | 사용자: "please + Claude Implementor", "전부 자동 머지", "너가 문제 없으면 머지하고 바로 해" (qa-log D-05) |
| D-06 | Herdr pin 매니페스트는 `contracts/herdr-bundle.json`으로 옮긴다(스키마 옆). 단일 출처 검사, `fetch-herdr-runtime.sh`, `bump-herdr.sh`, `herdr-update.yml`, `pr.yml`, AGENTS.md, docs가 그 경로만 읽는다. 내용은 그대로. | 가정: 되돌릴 수 있음 (qa-log D-06) |
| D-07 | 패키지된 앱의 CLI 탐색 순서: `HIDE_CLI_PATH` → (unpackaged) worktree `target/{debug,release}/hide` → (packaged) Resources의 `hide` → PATH → 기억된 CLI → 로그인 셸 PATH → 설치 디렉터리. Swift 앱 fallback 항목은 제거. 데몬 시작 시 `HERDR_BIN_PATH`가 비어 있으면 Resources의 herdr를 넘기고, 설정돼 있으면 그대로 둔다. `hide`는 자기 실제 파일 옆의 `hided`를 실행하므로 두 바이너리는 같은 디렉터리에 둔다. | 가정: 되돌릴 수 있음; `hided/src/cli.rs` daemon_binary, `docs/ARCHITECTURE.md` desktop host (qa-log D-07) |
| D-08 | 패키징은 이미 있는 `@electron/packager` 위에 (1) `cargo build --release`의 `hided`·`hide`와 `fetch-herdr-runtime.sh`의 herdr를 Resources로 복사, (2) `codesign -s -`(ad-hoc), (3) zip + SHA-256 체크섬을 더한다. 앱 버전은 릴리스 태그(`HIDE_VERSION`)에서 오고, unpackaged 개발 빌드는 태그 없이 동작한다. 산출물 이름은 지금과 같은 `hide-v<version>-macos-arm64.zip`. | 가정: engineering 규칙 7·2; `desktop/scripts/package.mjs`, `scripts/build-app.sh` (qa-log D-08, D-09) |
| D-09 | 디자인 파이프라인의 Swift 흔적 제거: Pen 시트·라이브러리가 참조하는 `macos/` 아래 이미지는 `web/src/assets`로 옮겨 재참조하고, `scripts/swift-source-tokens.mjs`와 HideTheme.swift 관련 검사·문서 문장을 지운다. `design/tokens.json` → `web/src/tokens.css` 생성은 그대로. | 가정 (qa-log D-10) |
| D-10 | 검증 게이트에서 Swift lane을 뺀다: `agents/config.json` verify는 test/lint(verify-cargo.sh) + web(`bash scripts/verify-web.sh`: web과 desktop의 typecheck·lint·unit·build)이고, CONTRIBUTING.md 게이트 표와 `pr.yml`의 `verify` job 의존을 맞춘다. 기존 web/desktop lane과 Electron e2e는 그대로 CI 게이트. | 가정 (qa-log D-11); config 변경은 커밋 d96d498a로 선반영 |
| D-11 | 문서는 Swift 셸이 없는 저장소를 서술한다(AGENTS.md, ARCHITECTURE.md, BUILD.md, INSTALL.md, dev-runtime.md, PERFORMANCE_TESTING.md, UI_BEHAVIOR.md의 "Native owner" 줄, DESIGN_WORKFLOW.md, theme-contract.md, verification-fixtures.md, docs/README.md, README.md, SECURITY.md, CONTRIBUTING.md, contracts/README.md). 삭제한 문서의 살아 있는 내용은 남는 문서로 옮기고 나머지는 git 이력에만 둔다. | 가정 (qa-log D-12) |
| D-12 | attached 앱이 데몬을 잃었을 때의 동작은 지금의 상태기계 그대로다(2초 health, 두 번 놓치면 lost, 3초마다 `hide status --json`, 다른 데몬이면 그 URL). 이번 변경은 내장 CLI가 그 대상이 되는 것만 더한다. | 저장소 사실: `desktop/src/main/host.ts`, ARCHITECTURE.md 652-653 (qa-log D-16) |
| D-13 | 기존 데이터 정책: 코어 소유 상태(`~/.local/state/hide` 전부, hook 설정, 세션 인덱스)는 Electron+hided가 그대로 계속 쓴다. 자격 증명은 hide가 저장한 적 없다. 롤백은 이전 GitHub 릴리스의 Swift hide.app 재설치이며 코어 상태 호환은 그 릴리스 시점까지만. | 가정: 되돌릴 수 있음 (qa-log D-17) |
| D-14 | 실제 관찰은 패키지된 hide.app을 격리 HOME과 격리 Herdr 소켓에서 `open -a`로 실행해 내장 바이너리만으로 셸이 붙는 것을 스크린샷으로 남긴다. 사용자의 라이브 hide/Herdr에는 붙지 않는다. 사용자 부재 중 창이 잠깐 뜨는 것은 허용됨. | 가정 (qa-log D-14) |
| D-15 | 원칙 intake: engineering/principles.md와 design/principles.md(oh-my-principle 654485f)를 읽었다. 규칙 1·8이 Non-goals와 D-08에 반영됐고, 규칙 4는 B12(내장 바이너리 부재 시 실패 표시)로 번역됐다. design 규칙은 새 화면이 없어 번역하지 않았다. | 가정 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 머지 후 저장소에 `macos/`, `spikes/swift-shell-pivot/`, `scripts/build-app.sh`, `scripts/verify-swift.sh`, `herdr-core/src/ffi.rs`, `herdr-core/include/`, `herdr-core/tests/ffi_*`, `hided/src/coexist.rs`가 없고, `rg "macos/|HideTheme|herdr_core\.h|verify-swift|build_dev_app|SwiftUI|swift test"`가 소스·스크립트·워크플로·docs에서 아무 것도 찾지 못한다(git 이력과 `agents/` 제외). "Swift"라는 낱말이 남는 곳은 두 군데뿐이다: INSTALL.md의 이전 Swift 앱 잔여 파일 한 줄(B13)과 백로그 이슈 본문(B15). | D-01, D-02 |
| B2 | `cargo build --release`가 `hided`와 `hide`를 만들고 `herdr-core`는 staticlib 없이 rlib만 만든다; `bash scripts/verify-cargo.sh test`와 `lint`가 통과한다. | D-02 |
| B3 | `hide`를 실행해도 Swift 앱의 소켓 공존 경고·질문이 더 이상 없다. | D-02 |
| B4 | `pnpm --dir desktop package`가 `desktop/out/hide.app`을 만들고 그 `Contents/Resources`에 `hided`, `hide`, `herdr`가 실행 가능 상태로 있으며, 앱이 `codesign -s -`로 서명돼 있다. `HIDE_VERSION`이 있으면 그 값이 앱 버전이고, 없으면 unpackaged 개발 실행은 그대로 된다. | D-03, D-08 |
| B5 | 릴리스 태그를 밀면 `release.yml`이 Rust 테스트·web/desktop 검증을 거쳐 `hide-v<version>-macos-arm64.zip`과 `.sha256`를 만들어 draft 릴리스에 올린다. Swift 단계는 없다. | D-03, D-08, D-10 |
| B6 | 패키지된 hide.app을 PATH에 hide·herdr가 없는 격리 HOME에서 `open -a`로 열면 Resources의 `hide`로 `hide connect`를 실행하고 Resources의 herdr를 `HERDR_BIN_PATH`로 넘겨 hided를 띄운 뒤 창에 웹 셸이 붙는다. | D-03, D-07, D-14 |
| B7 | `HIDE_CLI_PATH`가 설정돼 있으면 내장 CLI보다 그것을 먼저 쓰고, `HERDR_BIN_PATH`가 설정돼 있으면 내장 herdr 대신 그것을 그대로 넘긴다(격리 e2e·개발 서버). 시도한 경로는 지금처럼 호스트 로그에 한 번씩 남는다. | D-07 |
| B8 | 내장 `hide`가 실행 불가하면 호스트 상태 페이지가 `cli_missing`과 시도한 경로를 보이고 Retry를 준다; 데몬이 10초 안에 응답하지 않으면 `no_response`. attached 뒤 데몬을 잃으면 D-12의 상태기계 그대로 lost → 재발견한다. | D-07, D-12 |
| B9 | Herdr pin은 `contracts/herdr-bundle.json` 한 곳에 있고 `scripts/check-herdr-pin-single-source.sh`가 다른 재기술을 거부하며, `fetch-herdr-runtime.sh`·`bump-herdr.sh`·`herdr-update.yml`·`pr.yml`이 그 파일을 읽어 지금과 같은 바이너리를 받는다. | D-06 |
| B10 | `pr.yml`에 swift job이 없고 `verify` job은 rust·scripts·web·desktop lane만 요구한다; `bash scripts/verify-web.sh`가 web과 desktop의 typecheck·lint·unit·build를 한 번에 돌리고 CONTRIBUTING.md 게이트 표가 그것을 안내한다. macos/를 읽던 check 스크립트는 삭제되거나 web/desktop 대상으로 바뀌어 CI에서 통과한다. | D-10 |
| B11 | `node scripts/check-design-contract.mjs`와 `gen-screens`가 macos/ 자산 없이 통과하고, Pen 시트의 provider 마크는 `web/src/assets`의 이미지로 그려진다. | D-09 |
| B12 | 패키징 스크립트는 내장할 바이너리(hided, hide, herdr) 중 하나라도 없거나 실행 불가하면 그 자리에서 이름을 말하며 실패하고 앱을 만들지 않는다; 기본값으로 대체하지 않는다. | D-08, D-15 |
| B13 | INSTALL.md는 릴리스 zip 설치, 첫 실행 Gatekeeper 절차, 소스 빌드(`pnpm --dir desktop package`), 그리고 이전 Swift 앱의 남는 파일 한 줄을 설명한다. BUILD.md·dev-runtime.md는 cargo·pnpm·Electron만 다룬다. | D-11, D-13 |
| B14 | AGENTS.md Repository Layout에 `macos/`와 "S6까지 공존" 문장이 없고 `desktop/`이 배포 앱으로 서술된다; ARCHITECTURE.md의 Swift 셸·C ABI·공존 절이 없고 desktop host 절이 내장 바이너리와 서명을 서술한다; UI_BEHAVIOR.md에 "Native owner" 줄이 없다; docs/README.md 소유 표가 갱신된다. | D-11 |
| B15 | GitHub 이슈 "Electron backlog: Swift-only features"가 D-04 목록을 항목별로 담고, docs/README.md와 PR 본문이 그 번호를 가리킨다. | D-04 |
| B16 | 기존 사용자의 `~/.local/state/hide` 아래 상태와 hook 설정은 머지 전후로 같은 파일을 쓰며, Swift 앱 전용 파일은 그대로 남는다. | D-13 |

## Technical structure

- 삭제: `macos/`, `spikes/swift-shell-pivot/`, `herdr-core` FFI 모듈·헤더·staticlib crate-type·FFI 테스트, `hided/src/coexist.rs`, Swift 빌드·서명·검증 스크립트, Swift CI job, Swift 전용 문서와 검사 스크립트.
- 이동: Herdr pin 매니페스트 → `contracts/herdr-bundle.json`; Pen이 쓰는 provider 이미지 → `web/src/assets`.
- 추가: `desktop/scripts/package.mjs`가 release 바이너리 3개를 Resources에 넣고 ad-hoc 서명·zip·체크섬을 만든다; `desktop/src/main/cli.ts`에 packaged Resources 후보와 `HERDR_BIN_PATH` 전달; `scripts/verify-web.sh`; `release.yml`을 desktop 패키징으로 교체.
- 바뀌지 않음: 코어 도메인·session_sync·wire, hided의 WS/HTTP 계약, 웹 셸 화면, 데몬 상태기계.

## Risks

- 삭제 누락: 검사 스크립트 27개와 문서 12개가 macos/ 경로를 읽는다. B1의 `rg`와 CI 전체 lane 통과가 상한이다.
- 내장 바이너리 아키텍처: Herdr pin은 `herdr-macos-aarch64`뿐이라 산출물은 arm64 전용이다(지금과 같음).
- 서명: ad-hoc 서명 앱은 첫 실행에 Gatekeeper 확인이 필요하다(지금과 같음); B13이 안내한다.
- 사용자 부재 중 관찰: B6 스크린샷은 격리 HOME·격리 소켓·`open -a`로 찍고, 사용자의 라이브 hide/Herdr 창·pane·서버를 조작하지 않는다.
- 사용자가 미리 해야 할 일: 없음.
