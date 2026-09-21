---
topic: "S2: 탭·분할·줌, 프로젝트/체크아웃 전환, 단축키"
status: "ready"
human_approval: "approved"  # user 2026-09-21 verbatim: ㅇㅇㅇ 레스고 opus xhigh로 해서 작업 dispatch시키자!
review_profile: "high-risk"
review_rationale: "브라우저 페이지가 처음으로 로컬 파일시스템 목록(remote_file_list)과 워크스페이스 등록(create_workspace)을 요청하게 되어 접근 경계가 새로 생기고, 다중 pane 렌더가 고빈도 경로(스냅샷 델타·resize)에 작업을 더한다."
source_intake: "agents/interview/web-shell-pivot-s2/qa-log.md"
created_at: "2026-09-21"
updated_at: "2026-09-21"
---

# PRD: S2 탭·분할·줌, 프로젝트/체크아웃 전환, 단축키

## Goal

S1(PR #130)로 브라우저 탭 하나에서 포커스 pane 하나와 에이전트 사이드바를 쓸 수 있게 되었다.
S2는 그 위에 하루 작업의 나머지 골격을 얹는다: 코어 projection 트리 그대로의 pane 분할·줌, 체크아웃별 탭 바, 프로젝트/체크아웃 전환, 새 워크스페이스 등록, 그리고 현 Swift 단축키 카탈로그에 대응하는 호스트별 단축키 레지스트리.
끝나는 조건은 우산 PRD B11 그대로다: 사용자가 실제 Herdr 세션에서 Swift 앱 없이 `hide`만으로 하루 작업(탭·분할·줌, 프로젝트/체크아웃 전환, 단축키)을 시작한다.

## Non-goals

- Explorer/에디터/뷰어(S3), Changes(S4), Settings·진단 표면·단축키 사용자 정의(S5), Swift 삭제(S6): 각자 PRD. ⌘K 검색과 ⌘P 파일 열기는 레지스트리에 자리만 있고 S3에서 동작한다.
- 사이드바의 편집 동작: 워크트리 생성·제거, 핀 토글, purpose 편집, 원격 기기 등록·연결은 S5. 사용자는 그동안 Swift 앱(또는 `herdr` CLI)으로 한다. 재검토: S5 PRD (D-02).
- Electron 호스트 매핑(⌘ 계열 복원)과 네이티브 폴더 선택창: 레지스트리와 등록 흐름에 슬롯만 두고 Electron PRD에서 채운다. 사용자는 브라우저 기간 동안 ⌥ 계열과 경로 입력창을 쓴다. 재검토: S6 이후 Electron 인터뷰 (D-01, D-04).
- 홈 밖 경로의 웹 등록: `hide open <path>` CLI로만. 재검토: 사용자가 홈 밖 프로젝트를 웹에서 등록하려 할 때 (D-09).
- 프로젝트 홈/Overview 화면, 대화 뷰어(⌘⌥C), pet, 사용량: 우산 PRD 결정 4 미룸 표면. 프로젝트 클릭은 홈 없이 탭으로 간다 (D-07).
- 새 코어 이벤트·스냅샷 형태·소유권 규칙 변경: S2에 필요한 이벤트는 모두 이미 있다(`herdr-core/src/runtime/events.rs`: create_tab·focus_tab·reorder_tab·close_tab·check_close_status·reopen_closed·create_pane·close_pane·resize_pane·toggle_zoom·pane_text_scale·focus_checkout·create_workspace·remote_file_list·inactive_*_toggle; 지오메트리는 `Snapshot.pane_layouts`, `model.rs:1029-1080`). Herdr가 결정하는 지오메트리를 웹이 미리 그리지 않는다 (engineering 5, 우산 PRD 결정 16).
- Swift 셸 변경: `macos/`는 토큰 생성기 출력 외에 바뀌지 않는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 단축키 레지스트리는 명령 → 호스트별 키 매핑 표 하나. 브라우저 호스트: Chrome이 페이지에 넘기지 않는 6개(⌘T·⌘W·⌘⇧T·⌘⇧N·⌃Tab·⌃⇧Tab)만 ⌥ 계열로 옮기고(⌥T 새 탭, ⌥W 탭 닫기, ⌥⇧T 닫은 탭 복원, ⌥⇧N 새 워크스페이스, ⌥\` / ⌥⇧\` 최근 탭), 나머지 11개(⌘K·⌘P·⌘B·⌘E·⌘F·⌘⇧B·⌘⇧H·⌘⇧K·⌘⌫·⌥Tab·⌥⇧Tab)는 Swift와 동일하게 `preventDefault`로 가로챈다. Electron 호스트 열은 Swift와 같은 ⌘ 계열이며 지금은 비워 둔다(TODO). ⌥ 영구 통일(b)과 단축키 없이 팔레트(c)는 기각. | Q2 "ㅇㅇㅇ 우선 TODO로 남겨놔 여튼 Electron일때는 그대로 적용해야지~"; 예약 키 목록은 코드 사실(`ShellMenuCommand.swift:81-97`; Q4 정정: ⌘H는 hide 단축키가 아님) |
| D-02 | 프로젝트/체크아웃 사이드바 S2 범위: 전환 + 읽기 전용 표시(핀 상태, purpose 한 줄, 브랜치·PR 배지) + inactive 체크아웃·프로젝트 접기. 편집 동작(워크트리 생성·제거, 핀 토글, purpose 편집, 원격 기기)은 S5. 전환만(b)과 전부(c)는 기각. | Q1 "Q2 a로 ㄱㄱㄱ" |
| D-03 | 분할선 드래그 resize: 드래그 중엔 가이드선만 그리고, 놓을 때 `resize_pane` 이벤트 1개(비율 0.001~0.5 밖이면 보내지 않음). 드래그 중 쓰로틀 실시간 반영(b)은 고빈도 경로·프레임 게이트 때문에 기각. | Q1 "Q3는 a ㄱㄱ" |
| D-04 | 새 워크스페이스 등록: 경로 입력창 + 코어 디렉터리 목록(`remote_file_list`) 자동완성(홈부터) + 최근 등록 경로 제안 → `create_workspace`. Electron에선 네이티브 폴더 선택창으로 교체(TODO). S2 제외·CLI만(b)과 둘 다(c)는 기각. | Q3 "a로 원 가자" |
| D-05 | 가정: 보이는 탭의 pane만 xterm 인스턴스를 갖고 다른 탭의 pane은 해제한다. attach 규칙(최근 5탭, `ATTACHED_TAB_LIMIT`)과 released 표시는 코어 그대로. 재검토: 탭 전환 첫 프레임이 게이트를 넘을 때. | 가정: 우산 PRD 결정 16, ARCHITECTURE.md attach 규칙 |
| D-06 | 가정: 탭 바는 상단, 체크아웃별 visible tab. 탭 닫기는 `check_close_status` → 필요 시 확인 → `close_tab`의 현 Swift 흐름. 줌은 `toggle_zoom`. 같은 에이전트 가정으로(D-01 사용자 결정과 별개, 승인 아님) pane 명령 카탈로그(`PaneShortcutSettings.swift:32-41`: ⌘D·⌘⇧D 분할, ⌘⌥↩ 줌, ⌘⇧W pane 닫기, ⌘=·⌘-·⌘0 글자 크기)도 레지스트리에 넣고, ⌘⇧W는 Chrome 창 닫기 예약 키라 ⌥⇧W로 옮긴다; ⌘⌥C 대화 뷰어는 미룸 표면이라 제외. 재검토: 사용자 거부(⌥⇧W 또는 카탈로그 포함). | 가정: design 5; qa-log Addendum D-13 |
| D-07 | 가정: 프로젝트 클릭은 홈 화면 없이 그 프로젝트의 마지막 체크아웃 탭으로 전환한다(`focus_checkout`). 재검토: Electron PRD(Home/Overview 미룸 표면). | 가정: 우산 PRD 결정 4(미룸 표면) |
| D-08 | 가정: S0 방식 재측정을 다중 pane(분할 4개, 탭 5개 attach)에서 수행. 게이트는 우산 PRD 결정 6 그대로: echo p95 ≤ Swift 6.228 + 5 ms, 120 s driven 16.7 ms 초과 프레임 ≤ 1%. S1 측정 하네스(`scripts/web-shell-measure/`) 재사용. | 가정: 우산 PRD 결정 6; S1 B8/B12 |
| D-09 | 파일시스템 경계: 디렉터리 목록과 등록 모두 `$HOME` 하위 디렉터리로 제한. 심볼릭 링크는 실경로가 홈 밖이면 제외, 숨김 디렉터리는 기본 숨김, 파일은 목록에서 제외. 홈 밖은 `hide open <path>` CLI로만. 제한 없음(b) 기각, 허용 루트 설정(c)은 S5 재검토 후보. 경계는 hided(서버)가 강제하고 웹은 그 결과만 그린다. | Q4 "a로 우선 갑시다.. 담에 바꿀수도잇지만 여튼" |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 사이드바에 프로젝트와 그 아래 체크아웃 행이 현 Swift 순서로 보이고, 행마다 라벨·브랜치·purpose 한 줄·핀 상태·PR 배지가 읽기 전용으로 보인다. inactive 체크아웃/프로젝트는 접힌 그룹으로 보이고 클릭하면 펼쳐진다(`inactive_*_toggle`). 편집 컨트롤은 없다. | D-02 |
| B2 | 체크아웃 행을 클릭하면 `focus_checkout` 이벤트 하나가 나가고 탭 바가 그 체크아웃의 탭들로, 중앙이 그 체크아웃의 visible tab으로 바뀐다. 프로젝트 행 클릭은 홈 화면 없이 그 프로젝트의 마지막 체크아웃 탭으로 같은 전환을 한다. | D-02, D-07 |
| B3 | 탭 바가 상단에 체크아웃별 탭을 보이고, 클릭·⌥\`·⌥⇧\`로 전환한다. 탭을 드래그해 놓으면 `reorder_tab` 1개가 나가고 코어 순서대로 다시 그려진다. ⌥T는 새 탭(`create_tab`), ⌥⇧T는 마지막으로 닫힌 탭을 탭 바 끝에 복원해 활성화한다(`reopen_closed`; 복원할 탭이 없으면 아무 일도 없고 진단 로그에만 남는다). 탭 전환은 코어의 `pane_layouts` 조회이며 기다리는 상태를 그리지 않는다. | D-01, D-06 |
| B4 | 중앙은 활성 탭의 projection 트리를 CSS grid로 그대로 그린다: Split은 direction·ratio대로, 각 Pane은 xterm 인스턴스, 포커스 pane은 현 Swift와 같은 표시. 줌(⌘⌥↩, `toggle_zoom`)이면 그 pane만 전체로 보이고 다시 누르면 돌아온다. ⌘D/⌘⇧D는 `create_pane`으로 분할하고 Herdr가 지오메트리를 내려준 뒤에만 새 pane이 그려진다. | D-01, D-06 |
| B5 | attach 한도 밖 탭의 pane은 released 표시로 그려지고 클릭하면 다시 붙는다. 보이지 않는 탭의 pane은 xterm 인스턴스를 갖지 않으며, 탭을 돌아오면 코어 상태에서 다시 그려진다. | D-05 |
| B6 | 분할선을 끌면 가이드선만 움직이고, 놓는 순간 `resize_pane` 1개가 나가며 Herdr가 새 지오메트리를 내려주면 grid와 각 xterm의 fit·`terminal_resize`가 따라간다. 비율이 범위 밖이면 이벤트 없이 가이드선이 원위치로 돌아간다. Herdr 거부·타임아웃은 화면 변화 없이 진단 로그에 남는다. | D-03 |
| B7 | ⌥W는 탭 닫기, ⌥⇧W는 pane 닫기. 살아 있는 프로세스가 있으면(`requires_close_status_check`/`requires_close_confirmation`) 현 Swift와 같은 확인 흐름을 거치고, 확인 전까지 탭·pane은 그대로 있다. pane이 닫히면 Herdr가 내려준 새 지오메트리로 grid가 다시 그려진다. Herdr가 거부하면 탭·pane이 남고 이유는 진단 로그다. | D-01, D-06 |
| B8 | ⌥⇧N 또는 사이드바 하단 "새 워크스페이스"를 누르면 경로 입력창이 열리고, 홈 디렉터리부터 하위 디렉터리가 자동완성되며 최근 등록 경로가 위에 제안된다. 확정하면 `create_workspace`가 나가고 사이드바에 새 프로젝트가 나타나 포커스된다. | D-04 |
| B9 | 등록 입력창에서 존재하지 않는 경로, 파일 경로, 이미 등록된 경로, 홈 밖 경로는 입력창 아래 한 줄로 이유가 보이고 이벤트는 나가지 않는다. 코어가 등록을 거부하면 같은 자리에 이유가 보인다. 입력을 고쳐 다시 확정할 수 있다. | D-04, D-09 |
| B10 | hided는 `remote_file_list`·`create_workspace`의 경로가 `$HOME` 하위 디렉터리가 아니면(실경로 기준; 심볼릭 링크로 나가는 경로 포함) 코어에 넘기지 않고 이유 코드와 함께 거부하며 진단 로그에 남긴다. 목록 응답에는 파일과 숨김 디렉터리가 없다. 다른 이벤트 kind의 경로는 이 경계와 무관하다. | D-09 |
| B11 | ⌘/ 로 단축키 시트가 열려 브라우저 호스트 매핑 전체가 명령별로 보이고, Chrome 예약 때문에 옮겨진 6개(D-01)와 ⌥⇧W(D-06 가정)에는 그 표시가 붙는다. 시트 내용은 레지스트리에서 생성된다. | D-01 |
| B12 | 레지스트리의 모든 브라우저 매핑이 xterm pane에 포커스가 있을 때도 브라우저 기본 동작(북마크, 찾기, 확대)을 대신해 hide 명령을 실행한다. 한글 조합 중에는 문자 키 단축키가 적용되지 않는다. ⌘K·⌘P는 S3까지 "준비 중" 표시만 한다. | D-01 |
| B13 | ⌘=·⌘-·⌘0은 포커스 pane만의 글자 크기를 `pane_text_scale`로 바꾸고, 새 크기의 fit 결과가 `terminal_resize`로 전달된다. 코어의 크기 한계에 닿으면 더 바뀌지 않는다. | D-06 |
| B14 | WS가 끊겼다 붙으면 탭 바·분할·줌·포커스가 코어 상태대로 복원된다(S1 B11 승계). 진행 중이던 드래그나 등록 입력은 취소되고 입력창의 텍스트는 남는다. | D-06 |
| B15 | 분할 4개·탭 5개 attach 상태에서 S0 방법으로 echo p95가 Swift 6.228 + 5 ms 안이고, 120 s driven 구간에서 16.7 ms 초과 프레임이 1% 이하다. 스냅샷 델타에서 바뀐 탭·pane만 리렌더된다. | D-08 |
| B16 | `web/src` 어디에도 색·간격·radius 리터럴이 없고 `node scripts/check-design-contract.mjs`가 통과한다. `pr.yml` web job(typecheck·lint·vitest·Playwright)이 통과하며 Playwright는 격리 Herdr 위에서 체크아웃 2개·탭 3개·분할 2개·등록 1회·거부 1회를 수행한다. | - |
| B17 | `docs/ARCHITECTURE.md` hided 절에 파일시스템 경계와 단축키 레지스트리(호스트별 표, Electron 열 TODO)가 있고, `docs/BUILD.md`/`CONTRIBUTING.md`의 web lane 설명이 S2 측정 명령을 포함한다. | - |
| B18 | 끝나는 조건: 사용자가 Swift 앱을 닫고 실제 Herdr 세션에서 `hide`만으로 하루 작업(체크아웃 전환, 새 탭, 분할, 줌, 탭 닫기, 워크스페이스 등록, 단축키 시트)을 수행한다. 이 관찰은 사용자가 하고, 구현자는 격리 서버 Playwright와 측정까지 한다. | D-08 |

## Technical structure

- 전달: `delivery.mode: pr`(agents/config.json). 하나의 PR, 커밋은 단위별(레지스트리 / 탭 바 / 분할·줌 렌더 / 사이드바 전환 / 등록 흐름 / hided 경로 경계 / 측정·docs). 검증 lane은 S1의 web job·verify-cargo·verify-swift 그대로.
- `web/`: 단축키 레지스트리(명령 → 호스트별 키, 브라우저 열만 채움), 탭 바, projection 트리 CSS grid 렌더러(xterm 인스턴스는 활성 탭만), 사이드바 프로젝트/체크아웃 섹션, 워크스페이스 등록 입력창, 단축키 시트. 상태는 S1 zustand 스토어의 셀렉터로만.
- `hided/`: dispatch 경로에 파일시스템 경계 필터 한 곳(`remote_file_list`, `create_workspace`의 경로를 `$HOME` 실경로 기준으로 검사, 거부 이유 코드). 새 HTTP 엔드포인트 없음.
- `herdr-core`: 변경 없음(이벤트·스냅샷 기존 것). `contracts/hided-ws.schema.json`에 거부 이유 코드 추가.
- 측정: `scripts/web-shell-measure/`에 다중 pane 시나리오 추가. 증거는 `agents/runs/web-shell-pivot-s2/`.
- 프로세스 경계는 S1 그대로(브라우저 ↔ hided 루프백 WS ↔ Herdr 소켓).

## Risks

- 다중 xterm 인스턴스가 델타마다 리렌더되면 프레임 게이트를 넘는다. 활성 탭만 인스턴스를 갖고, 탭·pane 셀렉터가 바뀐 것만 넘기는지 B15 측정으로 확인한다.
- Chrome 예약 키 목록은 브라우저 버전에 따라 달라질 수 있다. 레지스트리가 매핑을 한 곳에 두므로 바뀌면 표 한 줄을 고친다; 실패 형태는 "단축키가 브라우저 동작을 한다"이고 데이터 손실은 없다.
- `$HOME` 경계는 hided가 강제하지만 코어의 `remote_file_list`는 원격 기기용으로도 쓰인다. 경계 필터는 로컬 hided 클라이언트 경로에만 걸리고 코어 동작은 바꾸지 않는다; Security 리뷰가 심볼릭 링크·`..`·URL 인코딩 우회를 본다.
- 확인 흐름(`check_close_status`)의 Swift 프레젠테이션 로직이 웹으로 옮겨지며 코어로 내려야 할 것이 보이면 Follow-up으로 기록한다(S1 Risks 승계).
- 원칙 intake(`sasu principles list` 654485f, 세 문서 전체): engineering 5·8이 D-03과 지오메트리 비목표, 4·10이 B6/B9/B10, 15가 D-05, 12가 D-08; design 3·5가 D-01, 9·13이 B5/B7. `practices/env.md`: 경계 루트는 코드가 `$HOME`에서 읽고 설정값으로 두지 않는다(S5의 허용 루트 옵션 전까지).
- 라이브 검증 경계: 구현자는 격리 Herdr(`HERDR_SOCKET_PATH`)만. 실사용 관찰(B18)은 사용자가 한다. 사용자가 할 일은 B18 외에 없다.
