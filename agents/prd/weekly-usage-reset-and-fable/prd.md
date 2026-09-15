---
topic: "Weekly Usage 팝오버: 프로바이더별 reset 시각과 Claude Code Fable 주간 버킷"
status: "ready"
human_approval: "approved"  # user 2026-09-14 verbatim: ㅇㅇㅇ gen-prd 하고 /task-add 해버려
review_profile: "high-risk"
review_rationale: "Claude Code의 OAuth 토큰을 키체인에서 읽어 외부 API를 호출하는 자격 증명 경계 변경이며, 기존 문서 결정('credential 파일은 읽지 않는다')을 뒤집는다."
source_intake: "agents/interview/weekly-usage-reset-and-fable/qa-log.md"
target_repository: "modakbul-gongbang/hide"
target_branch: "main"
screen_evidence: "screenshot"
created_at: "2026-09-14"
updated_at: "2026-09-14"
---

# PRD: Weekly Usage 팝오버: 프로바이더별 reset 시각과 Claude Code Fable 주간 버킷

## Goal

hide를 쓰는 개발자가 툴바의 Weekly Usage 버튼을 누르면, Claude Code와 Codex의 주간 사용량 옆에 언제 리셋되는지가 바로 보이고, Claude Code 아래에 Fable 모델 전용 주간 버킷이 하위 행으로 보인다.
지금은 Claude 행이 이 머신에만 있는 개인 스크립트(`~/.claude/statusline.sh`)가 쓰는 캐시 파일에 의존해 다른 개발자의 hide에서는 항상 Unavailable이고, Fable 버킷은 그 파일에도 Claude Code statusline 페이로드에도 없다.
그래서 Rust core가 각 CLI의 로그인 토큰으로 프로바이더 usage API를 직접 조회하도록 바꾼다.

## Non-goals

- 5시간 창은 표시하지 않고 core 스냅샷에도 싣지 않는다 (D-19). 사용자는 팝오버에서 세션 한도를 볼 수 없다. 5h 표면을 요청하면 다시 연다.
- Codex의 모델별 한도(`additional_rate_limits`, 예: GPT-5.3-Codex-Spark)는 그리지 않는다 (D-18). Codex는 주간 행 하나뿐이다. 사용자가 Codex 모델 버킷을 요청하면 다시 연다.
- hide는 OAuth 토큰을 갱신하거나 키체인에 쓰지 않는다 (D-13). 만료된 토큰은 Unavailable로 보이고, 사용자가 `claude`를 한 번 실행하면 회복된다. CLI가 자체 갱신을 멈추는 경우에만 다시 연다.
- `~/.claude/statusline.sh`와 `~/.claude/.usage-cache.json` 계약은 삭제하고 확장하지 않는다 (D-11). 그 파일을 읽던 코드는 같은 변경에서 제거한다 (engineering/principles.md rule 1).
- Codex `app-server` RPC 프로브 폴백(orca 방식)은 두지 않는다. API 실패 시 폴백은 세션 JSONL이다 (D-15, D-24).
- 로그·스냅샷·FFI 페이로드에 토큰이나 이메일·계정 ID를 싣지 않는다 (D-06, engineering/principles.md rule 9).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Claude 주간 사용량은 hide가 직접 `GET https://api.anthropic.com/api/oauth/usage`(헤더 `anthropic-beta: oauth-2025-04-20`)를 호출해 얻는다. `seven_day.utilization`/`resets_at`가 메인 행, `limits[]` 중 `kind == weekly_scoped` 항목이 하위 행이다. 캐시 파일 경로(Q1의 A안)는 사용자가 "다른 개발자도 동일하게 동작"해야 한다며 거부했다. | Q6 "A로 ㄱㄱㄱ", Q6 "statusline.sh는 왜쓰는거야? 이거는 안써도 돼" |
| D-02 | 토큰은 시작 약 1초 뒤 첫 조회에서 로그인 키체인에서 읽는다: `CLAUDE_CONFIG_DIR`별 서비스 `Claude Code-credentials-<sha256(NFC(configDir))[:8]>` → 레거시 `Claude Code-credentials` → `~/.claude/.credentials.json` 순, 3초 제한. macOS 접근 프롬프트는 시스템에 맡긴다. 거부·없음·401은 Unavailable이고 hide는 토큰을 갱신하지 않는다 (orca 방식, refresh는 거부). | Q8 "a" |
| D-03 | `docs/AI_PROVIDERS.md`의 "credential 파일은 읽지 않는다" 결정을 개정한다: 토큰은 usage 조회에만 쓰고 로그·스냅샷·FFI에 싣지 않으며, 조회는 `Mutex<Runtime>` 밖에서 타이머로만 돈다. | Q6 (추천 수락) |
| D-04 | Codex 주간 사용량은 `~/.codex/auth.json`(`tokens.access_token`, `tokens.account_id`, `CODEX_HOME` 우선)으로 `GET https://chatgpt.com/backend-api/wham/usage`(헤더 `User-Agent: codex-cli`, `OpenAI-Beta: codex-1`, `originator: Codex Desktop`, `ChatGPT-Account-Id`)를 호출해 얻는다. 주간 창은 위치가 아니라 `limit_window_seconds == 604800`으로 고른다(실계정에서 `primary_window`가 주간, `secondary_window`는 null). POST는 405. | Q10 "B로 가자 ㅇㅇㅇ orca 처럼", 라이브 응답 2026-09-14 |
| D-05 | 각 행의 라벨 줄은 `Claude Code · in 2d 19h  76%`: 상대 reset을 라벨 뒤에 muted로, 퍼센트는 우측 정렬, 아래에 기존 3px 바. 1시간 미만은 `in 42m`. 툴팁은 절대 시각 `Resets Sep 17, 1:00 PM`. 팝오버 폭 250 유지. (Q2의 "바 아래 별도 줄" 배치는 Q12에서 이 안으로 대체) | Q2 "A", Q12 "B로 가자" |
| D-06 | Fable은 Claude Code 아래 들여쓴 하위 행 `└ Fable · in 2d 19h  61%`로, 같은 마크·작은 라벨·같은 퍼센트/바/reset 형식. `weekly_scoped` 항목이 없으면 하위 행을 그리지 않고, 있는데 깨졌거나 만료됐으면 Unavailable. 하위 행은 모델 범용: 항목마다 하나, 라벨은 `scope.model.display_name`. 긴 라벨은 한 줄로 자르고 툴팁에 전체 이름. | Q3 "A", Q4 "A", Q12 |
| D-07 | 조회 주기: 시작 1초 뒤, 이후 앱 창이 보일 때 5분마다, 팝오버를 열 때 마지막 조회가 60초보다 오래됐으면 즉시. 429는 `Retry-After`(없으면 5→10→15분 백오프)까지 대기. | Q9 "a ㅇㅇ" |
| D-08 | 값 우선순위(두 프로바이더 공통): (1) 최신 API 성공값 → (2) 직전 성공값을 마지막 성공 후 15분까지, 툴팁 `Last checked 12m ago · offline` → (3) Codex만: 최신 세션 JSONL `token_count` 이벤트, 툴팁 `From last Codex session · <세션 시각>` → (4) Unavailable. 토큰 없음·401은 (2)를 건너뛴다. reset 경과가 이 순서보다 우선한다: 유지 중인 값의 `resets_at`이 지나면 15분이 남았어도, JSONL 값이 있어도 즉시 Unavailable이다. | Q17 "ㅇㅇ" (F2 추천 수락), Q9, spec gate F1 "ㅇㅇㅇㅇ" |
| D-09 | 첫 조회가 끝나기 전(시작 1초 지연 중 팝오버를 연 경우 포함) 모든 프로바이더 행은 퍼센트 자리에 muted `…`, 빈 바, 툴팁 `Checking usage…`로 그리고, 첫 성공에 값으로, 첫 종결 실패에 Unavailable로 바뀐다. 기존 "Provider usage is not available yet." 줄은 없앤다. | Q17 "ㅇㅇ" (F1 추천 수락) |
| D-10 | 5h 창을 각 행 옆에 보이는 안(Q13 B)은 사용자가 "너무 많다"며 철회했다. 주간만 보인다. | Q14 "5h는 우선 빼" |
| D-11 | Codex `additional_rate_limits` 하위 행(Q11 A)은 사용자가 철회했다. | Q12 "GPT-5.3-codex-spark이건 필요없어! 우선 안씀" |
| D-12 | 조회는 Rust core에 둔다: `herdr-core/src/usage.rs`가 지금처럼 세션 싱크 스레드에서 mutex 밖에서 돌고, HTTPS는 `ureq`(rustls), 키체인은 `security-framework` 크레이트. 셸은 스냅샷만 그린다 (셸 I/O 어댑터 + 새 이벤트 안은 거부). | Q15 "A" |
| D-13 | 가정: `ProviderUsageSnapshot`에 하위 버킷 목록(라벨, 퍼센트, reset)과 마지막 성공 시각·마지막 오류가 실려 셸이 상태를 계산하지 않는다. `window_minutes`는 10080 유지. 필드명은 구현자가 정한다. | 가정: D-06, D-07, D-08에서 도출 (qa-log D-21) |
| D-14 | 퍼센트 색상 임계(70 warning, 90 danger), 3px 바, 다크 팝오버, `hide-weekly-usage-<provider>` 접근성 ID 등 기존 행 패턴을 유지하고 확장한다. | qa-log D-02 (repo fact), design/principles.md rule 5 |
| D-15 | 원칙 반영: engineering/principles.md와 design/principles.md(oh-my-principle 653c462)를 전부 읽었다. rule 4(실패를 명시)는 B10~B14의 Unavailable/stale 상태로, rule 9(비밀 제외)는 B16으로, design rule 4(파생 상태 표시)는 상대 reset으로, design rule 9(모든 상태 설계)는 B8~B14로 번역했다. engineering rule 7(있는 것 활용)은 D-12가 새 크레이트 2개를 추가하므로 근거를 명시: Rust 크레이트에 HTTP·키체인 의존성이 없고, Swift의 URLSession 사용은 core-owns-state 경계(ARCHITECTURE.md)를 깨므로 채택하지 않았다. | 근거: sasu principles list, Q15 |
| D-16 | 배포: `agents/config.json`대로 worktree에서 구현하고 PR로 전달하며 CI를 지켜본다 (`main`은 squash merge만). | agents/config.json delivery.mode=pr |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 툴바의 Weekly Usage 버튼을 누르면 폭 250의 팝오버에 `Claude Code`, `Codex` 두 행이 각각 `라벨 · in <상대 reset>` 과 우측 정렬 퍼센트, 그 아래 3px 바로 그려진다. | D-05, D-14 |
| B2 | 상대 reset은 하루 이상이면 `in 2d 19h`, 한 시간 이상이면 `in 5h 12m`, 한 시간 미만이면 `in 42m` 형식이고, 행에 마우스를 올리면 툴팁에 절대 시각 `Resets Sep 17, 1:00 PM`(사용자 로캘)이 보인다. | D-05 |
| B3 | Claude Code 값은 `/api/oauth/usage`의 `seven_day` 창에서 온다: 퍼센트는 `utilization`, reset은 `resets_at`(ISO-8601)을 변환한 값이다. | D-01 |
| B4 | 응답의 `limits[]`에 `kind == weekly_scoped` 항목이 있으면 Claude Code 아래에 들여쓴 하위 행 `└ Fable · in 2d 19h  61%`가 항목마다 하나씩, `scope.model.display_name`을 라벨로 그려진다. 항목이 없으면 하위 행이 전혀 없다. | D-06 |
| B5 | 하위 행의 라벨이 폭을 넘으면 한 줄로 잘리고 툴팁에 전체 이름과 절대 reset이 함께 보인다. | D-06 |
| B6 | Codex 값은 `wham/usage` 응답 중 `limit_window_seconds == 604800`인 창에서 온다(`primary_window`든 `secondary_window`든). 주간 창이 없으면 Codex 행은 Unavailable이다. | D-04 |
| B7 | Codex 행 아래에는 하위 행이 없다. `additional_rate_limits`는 무시된다. | D-11 |
| B8 | 앱 시작 직후 첫 조회가 끝나기 전에 팝오버를 열면 두 행 모두 퍼센트 자리에 muted `…`, 빈 바, 툴팁 `Checking usage…`가 보이고, 조회가 끝나는 순간 값 또는 Unavailable로 바뀐다. "Provider usage is not available yet." 문구는 더 이상 없다. | D-09 |
| B9 | 값은 시작 1초 뒤 처음 조회되고, 앱 창이 보이는 동안 5분마다 갱신되며, 팝오버를 열 때 마지막 조회가 60초보다 오래됐으면 즉시 다시 조회된다. 창이 가려져 있으면 주기 조회가 멈춘다. | D-07 |
| B10 | Claude Code 키체인 항목이 없거나, 사용자가 접근 프롬프트를 거부했거나, API가 401을 돌려주면 Claude 행은 퍼센트 자리에 `Unavailable`, 툴팁 `Sign in with claude to see usage`이고 Fable 하위 행은 없다. hide는 토큰을 갱신하거나 키체인에 쓰지 않는다. | D-02 |
| B11 | 사용자가 `claude`를 한 번 실행해 CLI가 토큰을 갱신하면, 다음 주기 조회(5분 이내) 또는 60초 뒤 팝오버 재오픈 시 hide 재시작 없이 값이 돌아온다. | D-02, D-07 |
| B12 | 네트워크 오류·타임아웃이면 마지막 성공값이 그대로 보이고 툴팁이 `Last checked 12m ago · offline`으로 바뀐다. 마지막 성공 후 15분이 지나도록 실패가 이어지면 Claude 행은 Unavailable로, Codex 행은 세션 JSONL 값으로 바뀐다. | D-08 |
| B13 | Codex 행이 세션 JSONL 값을 보일 때 툴팁은 `From last Codex session · <세션 시각>`이고, JSONL에도 주간 이벤트가 없으면 Unavailable에 기존 메시지가 보인다. `~/.codex/auth.json`이 없거나 401이면 15분 유지 없이 바로 JSONL로 간다. | D-08, D-04 |
| B14 | API가 429를 돌려주면 `Retry-After`(없으면 5, 10, 15분 순 백오프)까지 재조회하지 않고, 그동안 B12의 stale 규칙이 적용된다. | D-07 |
| B15 | reset 시각이 이미 지난 값은 지금처럼 `<label> weekly usage expired at its last reset` 메시지의 Unavailable로 보인다. 오프라인·429 대기 중에 유지하던 값(Claude, Codex, Fable 하위 행 모두)의 reset이 지나면 15분 유지와 JSONL 폴백을 건너뛰고 즉시 Unavailable, 툴팁 `<label> weekly usage expired at its last reset · offline`으로 바뀌며, 다음 성공 조회에 새 값으로 돌아온다. 퍼센트 색은 70 이상 warning, 90 이상 danger로 지금과 같다. | D-14, D-08 |
| B16 | 어떤 로그 줄, 스냅샷 JSON, FFI 페이로드에도 access token, refresh token, 이메일, 계정 ID가 나타나지 않는다. 조회 실패는 프로바이더·HTTP 상태·오류 종류만 담은 구조화 로그 이벤트 하나로 남는다. | D-03 |
| B17 | 조회가 도는 동안 터미널 입력·스크롤·탭 전환 지연이 없다: 네트워크와 키체인 I/O는 `Mutex<Runtime>`을 잡지 않는다. | D-03, D-12 |
| B18 | `~/.claude/.usage-cache.json`은 더 이상 읽지 않고, 그 파일이 있든 없든 동작이 같다. | D-01 |
| B19 | `docs/AI_PROVIDERS.md`는 개정된 자격 증명 결정(토큰은 usage 조회에만, 파일·키체인 읽기 허용, 갱신 금지)과 usage 표시 문장(161행)을 이 변경과 같은 PR에서 반영하고, `design/hide.pen`의 `Screen /` 보드에 팝오버의 값·로딩·Unavailable·offline 상태가 그려진다. | D-03, D-05, D-09 |
| B20 | 5시간 창은 팝오버 어디에도 없고 core 스냅샷에도 실리지 않는다. | D-10 |

## Technical structure

- `herdr-core/src/usage.rs`: 파일 읽기 리더를 프로바이더별 HTTP 조회로 교체한다. Claude는 키체인(`security-framework`) → `.credentials.json` 순으로 토큰을 얻어 `/api/oauth/usage`를, Codex는 `~/.codex/auth.json`으로 `wham/usage`를 `ureq`(rustls)로 호출한다. 기존 세션 싱크 스레드에서 mutex 밖에서 돌며 조회 주기·백오프·stale 판정을 소유한다. 세션 JSONL 리더는 Codex 폴백으로 남는다.
- `herdr-core/src/model.rs`의 `ProviderUsageSnapshot`: 하위 버킷 목록과 마지막 성공 시각·마지막 오류를 추가한다(D-13). FFI C ABI 시그니처는 바뀌지 않고 스냅샷 JSON 형태만 넓어지며 `ffi_contract.rs`가 그 형태를 고정한다.
- `macos/Sources/HerdrMacOS/HideUI.swift`의 팝오버 행: 라벨 줄에 상대 reset, 하위 행, 로딩/offline 툴팁을 그린다. 새 색·간격은 `HideTheme`에서만 온다. `CoreBridge.swift`의 디코더가 새 필드를 읽는다.
- 새 크레이트 의존성: `ureq`(rustls 기능), `security-framework`. 다른 서비스·스키마·마이그레이션·인프라 변경은 없다.
- 외부 경계: 두 비공식 API(`api.anthropic.com/api/oauth/usage`, `chatgpt.com/backend-api/wham/usage`)를 읽기 전용으로 호출한다. 응답 형태 변화는 파서 테스트의 기록된 픽스처로 잡는다.

## Risks

- 두 API 모두 비공식이라 응답 형태가 바뀔 수 있다. 바운드: 파싱 실패는 Unavailable + 구조화 로그로 드러나고(B12, B16), 픽스처 테스트가 알려진 형태를 고정한다.
- 키체인 접근 프롬프트: 다른 서명 앱이 `Claude Code-credentials`를 읽으면 macOS가 첫 실행에 프롬프트를 띄우고, dev 빌드는 재서명될 때마다 다시 뜰 수 있다. 바운드: 3초 제한과 거부 시 Unavailable(B10); 이 사실을 `docs/AI_PROVIDERS.md`에 적는다. 프롬프트 문구·시점은 macOS 소유라 사용자 취향 판단 대상이 아니다.
- 토큰 노출: 토큰은 core 메모리에만 있고 요청 헤더로만 나간다(B16). 리뷰 포커스: 로그 이벤트와 스냅샷 직렬화 경로.
- 잘못된 시각 표시: `resets_at`은 Claude가 ISO-8601, Codex가 epoch 초라 변환 단위 오류가 나기 쉽다. 바운드: 두 픽스처 모두에 절대 시각 기대값을 둔다.
- 라이브 증명 경계: 검증은 이 머신의 로그인 계정으로 두 API를 읽기만 하며, 토큰 갱신·쓰기·로그아웃을 일으키지 않는다. 팝오버 스크린샷은 `agents/runs/<slug>/`에만 둔다.
- 사용자가 미리 할 일: 없음. 이 머신에는 두 로그인이 모두 있다.
- 열린 결정: 없음. `screen_evidence`는 팝오버가 바뀌므로 `screenshot`이다.
