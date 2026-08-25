# herdr-pet에서 옮겨온 기록

이 저장소는 `herdr-pet`(Rust + Tauri)을 대체한다.
결정 근거는 `agents/interview/herdr-ide-native-shell/qa-log.md`의 D-14, D-32, D-36이다.

옮긴 것은 **코드가 아니라 사고에서 나온 지식**이다.
Rust 구현은 재사용하지 않는다(D-14). 아래 파일들은 TypeScript로 다시 쓸 때의 근거이자 회귀 방지 자료다.

## 1. 그대로 유효한 지식 문서 (`docs/`)

| 파일 | 내용 | 관련 결정 |
| --- | --- | --- |
| `status-model.md` | herdr `_new` 토큰 계약, pet pose 우선순위, 뱃지 행 | D-31, D-40 |
| `pet-window-macos.md` | 투명·항상 위 창의 함정 목록. **D-33 스파이크가 점검해야 할 체크리스트** | D-32, D-33 |
| `theme-contract.md` | 테마 계약 | D-32 |
| `ambient-signals.md` | 앰비언트 신호 설계 | D-31 |
| `pet-assets.md`, `asset-prompts.md` | 펫 아트 규격과 생성 프롬프트 | D-32 |
| `verification-fixtures.md` | 검증 픽스처 | D-34, D-56 |
| `dev-runtime.md` | 인스턴스 하나만 띄우고 설치 번들로 검증하라는 규칙. **Tauri 명령(`cargo tauri dev`, `pgrep herdr-pet-app`)은 Electron 기준으로 다시 써야 한다** | D-34 |

`pet-window-macos.md`가 가장 중요하다.
그 문서가 드래그 아키텍처의 원인을 "WKWebView throttles timers in unfocused windows"로 기록해 두었고, 그 문장이 D-32(Electron이면 이 함정 대부분이 사라진다)의 근거가 됐다.
단, **Electron에서 실제로 사라지는지는 아직 검증되지 않았다.** D-33 스파이크가 그것을 확인한다.

## 2. 이식 대상 원본 (`docs/ported-reference/`)

빌드 대상이 아니다. TS로 다시 쓸 때 읽는 참조다.

| 파일 | 왜 남겼나 |
| --- | --- |
| `window.rs` | 오프스크린 클램프 기하. **실제 사고 좌표 `[542720, 163840]`가 테스트에 고정되어 있다.** D-34가 지정한 이식 대상 1 |
| `herdr.rs` | 소켓 NDJSON RPC와 `parse_snapshot`, `_new` 토큰 판정 테스트. D-34가 지정한 이식 대상 2 |
| `behavior.rs`, `aggregate.rs` | pose 우선순위와 상태 집계 |
| `ssh.rs` | `ssh -L` 터널. D-37의 ControlMaster 설계가 이것을 확장한다 |
| `remote-targets.example.toml` | 원격 타겟 설정 형식(`[[targets]]` + `[targets.ssh]`). D-45가 이 형식을 그대로 쓴다 |
| `hotkey.js` | accelerator 캡처. **함정: `event.key`가 아니라 `event.code`를 써야 한다.** Alt를 누르면 macOS가 `P`를 `π`로 보고해 `Alt+π`가 파싱되지 않는다. D-49 |

## 3. 손대야 하는 것 (`agents/rules/`)

**아래 규칙들은 herdr-pet에서 그대로 복사한 것이고, 이 저장소에서는 아직 유효하지 않다.**
`trigger.paths`와 `check`가 존재하지 않는 Rust 경로를 가리킨다.
`INDEX.md`는 `rules add`가 관리하는 파일이라 손으로 고치지 않았다.

| 규칙 | 상태 |
| --- | --- |
| `INV-herdr-unseen-token` | **내용은 유효.** `_new` 토큰만 attention으로 승격한다는 계약은 그대로다. `trigger.paths`(`crates/herdr-core/*.rs`)와 `check.run`(`cargo test -p herdr-core`)을 TS 경로와 명령으로 다시 랜딩해야 한다 |
| `INV-pet-state-off-main-thread` | **재검토 필요.** "sync Tauri 커맨드가 메인 스레드에서 돈다"는 Tauri 고유 제약이고 Electron IPC는 기본 비동기다(D-32). 이 저장소에서는 폐기하거나 다른 형태로 다시 세워야 한다 |
| `FACT-*` 항목들 | 랜딩 경로는 `docs/`로 살아 있으나 Tauri 전제가 섞여 있다 |

TS 코드가 생긴 뒤 `sasu rules add`로 다시 랜딩한다.
그 전까지 이 디렉터리는 **참조 사본**이지 강제되는 규칙이 아니다.

## 4. 에셋 (`assets/pet-theme/default/`)

`themes/default`의 원본 59개(`theme.json` + 아트).
`dist/` 빌드 산출물이 아니라 소스를 옮겼다.

## 5. 옮기지 않은 것

`crates/herdr-core`의 Rust 구현 전체, `apps/pet-app`, `Cargo.*`, `web/`의 나머지, `plugins/herdr-agent-pet`.
D-14와 D-32에 따라 재사용하지 않는다.
`herdr-pet` 저장소는 v1 완료 시 아카이브한다(D-15, D-36).
