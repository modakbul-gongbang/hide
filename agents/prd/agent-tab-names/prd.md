---
topic: "에이전트 탭 이름: 에이전트 제목 → 프로세스명 → Tab N, 탭 구성 순서, Rename"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "core의 탭 라벨 규칙과 wire 경계에 pane 프로세스 필드를 더하는 사용자 대면 변경이며, Herdr tab.rename 호출을 새로 쓰지만 데이터·권한·외부 효과는 없다."
source_intake: "agents/interview/agent-tab-names/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: 에이전트 탭 이름과 구성

## Goal

hide 사용자가 워크스페이스의 탭 스트립에서 "Tab 6" 대신 그 탭에서 일하는 에이전트의 제목(사이드바 행과 같은 이름)이나 실행 중인 프로세스명을 보고, 탭을 [상태 마크][provider 로고] 제목으로 알아보며, 필요하면 이름을 직접 붙인다.
사용자의 말: "Tab1,Tab2 이런거 의미가 없어보이는데", "(상태 badge) 에이전트로고 텍스트 이렇게 보여주자".
시각 참조는 `agents/runs/ux-fixes-2026-09-27/design/board-v3.pen` 섹션 2(A열).

## Non-goals

- ⌘ 홀드 번호 키캡은 T3(electron-digit-shortcuts-hints)의 것이다; 이 PR은 키캡을 그리지 않는다.
- 탭 폭·스크롤·닫기 버튼 규칙(browser-like sizing)은 바꾸지 않는다 (D-03).
- 사이드바 에이전트 행은 바꾸지 않는다; 탭이 그 행의 제목·마크·로고 규칙을 재사용한다.
- Herdr가 foreground 프로세스를 주지 않는 pane은 "Tab N"으로 남는다; 프로세스 추정은 하지 않는다 (D-05). 재검토: Herdr가 그 필드를 안정적으로 채울 때.
- 라이브러리 Component는 바꾸지 않는다 (D-06).
- engineering/principles.md 규칙 5(도메인은 wire를 모름)가 D-05의 wire.rs 경계를, 규칙 7이 기존 `display_tab_label` 확장을 정한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 지금 탭 이름은 Herdr 라벨을 core가 "Tab N"으로만 바꾼다; Herdr 0.9.1은 `tab.rename`과 PaneInfo의 `terminal_title`을 준다. `foreground_processes`는 PaneInfo가 아니라 별도 `pane.process_info` 응답에 있다. | 저장소 사실 (qa-log D-01: herdr-core/src/model.rs, contracts/herdr-api.schema.json) |
| D-02 | 탭 라벨 우선순위: (1) 사용자가 붙인 이름, (2) 탭의 포커스 pane 에이전트 제목(사이드바 행과 같은 ai-title 토큰), (3) 그 pane의 foreground 프로세스명, (4) "Tab N". 조건이 바뀌면 라벨도 따라간다. 이름 붙은 탭은 덮어쓰지 않는다. | 사용자 추천 수락 "2. C로해서" (qa-log D-02) |
| D-03 | 에이전트 탭의 구성은 [상태 마크][provider 로고] 제목; 마크와 로고는 사이드바 행과 같은 기호·색·이미지. 셸만 있는 탭은 터미널 아이콘 + 프로세스명. 탭 폭 규칙은 그대로. | 사용자: "(상태 badge) 에이전트로고 텍스트 이렇게 보여주자" (qa-log D-03) |
| D-04 | 탭 우클릭 메뉴에 Rename…: 이름을 받아 Herdr `tab.rename`으로 저장; 빈 이름 저장은 이름을 지워 자동 라벨로 돌아간다. Copy name은 표시 중인 라벨을 복사한다. | 보드 섹션 3 승인 (qa-log D-04) |
| D-05 | foreground 프로세스명과 pane 제목은 wire.rs에서만 변환해 도메인 pane 필드로 넣고, 라벨은 core의 한 함수(`display_tab_label` 확장)가 계산한다. coordinator 소유의 bounded off-lock reader가 ATTACHED_TAB_LIMIT 안의 attached tab별 focused pane만 각 로컬·원격 host의 socket에 `pane.process_info`로 묻는다. focused pane 변경, 해당 pane의 agent 상태 변경(pane_updated), 낮은 빈도의 재확인 한 종류만 읽기를 요청하고 tick마다 조회하지 않는다. generation fencing으로 늦은 답을 버리고 값이 바뀔 때만 publish하며, 실패·무응답은 프로세스 없음과 진단으로 남긴다. terminal_title로 프로세스를 추정하지 않는다. 웹은 스냅샷의 라벨과 탭 구성 필드(상태·provider)를 그리기만 한다. 비어 있는 pane은 (4). | 가정 (qa-log D-05) |
| D-06 | UI_BEHAVIOR.md의 탭 이름·탭 메뉴 문장을 고치고 Screen / Workspace 시트의 탭 바를 [마크][로고] 제목으로 다시 그린다(gen-screens). 라이브러리는 그대로. | 가정 (qa-log D-06) |
| D-07 | 검증: core 단위 테스트(우선순위 4단계와 전환), wire 테스트, vitest(탭 구성), 웹 e2e 1개(에이전트 시작 시 탭 이름 변화와 Rename 유지). | 가정 (qa-log D-07) |
| D-08 | 실행: please, Codex Implementor(gpt-6-astra, effort high), PR 배포, Observer 자동 머지. T0 머지 후 main에서 시작. | 사용자: "core 작업(T4, T8)만 Codex" (qa-log D-08) |
| D-10 | Rename…은 에이전트 탭·셸 탭 가리지 않고 모든 탭에 있다(원격 탭은 원격 Herdr에 같은 메서드). 선택하면 탭 제목 자리에 인라인 입력이 현재 라벨을 채운 채 전체 선택으로 열리고, Enter가 저장, Escape·포커스 이탈이 취소다. 이름은 Herdr 탭 라벨이라 Herdr가 살아 있는 한 재접속 뒤에도 남는다. | 가정 (qa-log D-10) |
| D-11 | Herdr가 `tab.rename`을 거부하거나 답이 없으면 인라인 입력이 입력한 글자를 유지한 채 열려 있고 그 아래 한 줄 caption "이름을 저장하지 못했습니다 · 다시 시도"가 보인다; Enter가 재시도, Escape가 취소(라벨은 이전 그대로). 재시도용 요청 intent와 committed 표시 라벨은 분리하며, 실패해도 intent의 입력은 남기고 committed 라벨은 이전 그대로 둔다. 사유는 진단 로그에만. | 가정 (qa-log D-11) 및 HCOORD 승인 |
| D-12 | 자동 라벨은 사용자가 붙인 이름이 없는 탭에만 적용된다. Herdr 라벨이 "Tab N" 형식이거나 비어 있으면 이름 없음, 그 외는 사용자 이름(`next_tab_label`의 기존 규칙). | 가정 (qa-log D-12) |
| D-09 | 원칙 intake: engineering/principles.md와 design/principles.md(oh-my-principle 654485f)를 읽었다. engineering 5·7이 D-05에, design 4(파생 상태 표시)·7(시각 부호)이 D-03에 반영됐다. | 가정 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 사용자가 이름을 붙이지 않은 탭 중 에이전트가 있는 탭의 이름은 그 탭 포커스 pane의 에이전트 제목이고, 사이드바 Agents 탭의 같은 에이전트 행과 글자가 같다. | D-02, D-05, D-12 |
| B2 | 사용자가 이름을 붙이지 않은 탭 중 에이전트가 없는 탭의 이름은 포커스 pane의 foreground 프로세스명(예: zsh, cargo)이고, Herdr가 프로세스를 주지 않으면 "Tab N"이다. | D-02, D-05, D-12 |
| B3 | 에이전트가 시작·종료되거나 포커스 pane이 바뀌면 다음 스냅샷에서 탭 이름이 규칙대로 바뀐다; 사용자가 붙인 이름은 어떤 경우에도 바뀌지 않는다. | D-02 |
| B4 | 에이전트 탭은 왼쪽부터 상태 마크(사이드바와 같은 기호·색), provider 로고(같은 이미지), 제목 순으로 그려지고, 셸 탭은 터미널 아이콘과 이름이다. | D-03 |
| B5 | 모든 탭의 우클릭 메뉴는 New tab, Rename…, Copy name, Close tab…이다. Rename…은 탭 제목 자리에 현재 라벨이 전체 선택된 인라인 입력을 열고, Enter로 저장, Escape나 포커스 이탈로 취소한다; 저장 즉시 탭에 그 이름이 보이고 Herdr 세션이 살아 있는 한 재접속 뒤에도 남는다. | D-04, D-10 |
| B6 | Rename…에서 빈 이름을 저장하면 자동 라벨(B1·B2)로 돌아간다. | D-04 |
| B7 | Copy name은 지금 보이는 라벨을 클립보드에 넣는다. | D-04 |
| B8 | 탭의 폭·스크롤·닫기 동작은 지금과 같다; 긴 제목은 지금의 규칙대로 잘린다. | D-03 |
| B9 | Herdr가 `tab.rename`을 거부하거나 답이 없으면 인라인 입력이 입력한 글자를 유지한 채 남고 그 아래 "이름을 저장하지 못했습니다 · 다시 시도" 한 줄이 보인다; Enter가 다시 시도하고 Escape가 취소해 이전 라벨로 돌아간다. 사유는 진단 로그에만 남고 배너·알림은 없다. | D-11 |
| B10 | UI_BEHAVIOR.md와 Screen / Workspace 시트가 새 규칙을 서술한다. | D-06 |

## Technical structure

- core: coordinator가 소유하는 bounded off-lock `pane.process_info` reader를 추가한다. ATTACHED_TAB_LIMIT 안의 attached tab별 focused pane만 해당 host의 socket으로 읽고, 포커스 pane 변경·해당 pane의 agent 상태 변경·낮은 빈도의 재확인으로만 요청한다. generation fencing, 결과 변경 시에만 publish, 실패 시 프로세스 없음과 진단을 적용한다. `wire.rs`에 응답의 foreground 프로세스명 변환 추가 및 기존 terminal_title 변환 재사용, 도메인 pane 필드 추가, `display_tab_label` 확장(에이전트 제목·프로세스명 입력), 스냅샷의 탭 항목에 구성 필드(상태·provider) 추가, `rename_tab` 이벤트 → Herdr `tab.rename`; core의 재시도 intent와 committed 라벨을 분리하여 거부·무응답 시 B9의 이전 라벨과 입력을 유지한다.
- web: `TabBar.tsx`의 탭 렌더와 메뉴, Rename 입력; `pen-screens.mjs`의 Workspace 탭 바.
- 바뀌지 않음: 사이드바, 탭 폭 규칙, 다른 도메인.

## Risks

- `foreground_processes`의 형식(이름·pid 배열)은 `herdr api schema`로 확인하고 contract 파일에 맞춘다; 필드가 비면 B2의 "Tab N".
- T3(키캡 오버레이)가 이 PR의 탭 구성 위에 얹히므로 이 PR이 먼저 머지된다.
- 사용자가 미리 해야 할 일: 없음.
