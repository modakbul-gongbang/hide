---
topic: "Electron 단축키: ⌘1-9 탭 전환, ⌥1-9 에이전트 이동, modifier 홀드 힌트"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "단축키 레지스트리와 전역 키 처리기에 chord와 홀드 힌트를 더하는 사용자 대면 변경이며, 데이터·권한·외부 효과는 없다."
source_intake: "agents/interview/electron-digit-shortcuts-hints/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: ⌘1-9 탭, ⌥1-9 에이전트, 홀드 힌트

## Goal

Electron 앱의 hide 사용자가 Swift 앱에서 쓰던 대로 ⌘n으로 n번째 탭, ⌥n으로 사이드바의 n번째 에이전트로 가고, ⌘나 ⌥을 잠깐 누르고 있으면 번호 키캡이 탭과 에이전트 행 오른쪽에 떠서 어느 번호인지 본다.
사용자의 말: "cmd + 1,2,3 -> 탭 전환이 없네, option 1,2,3 이것도 없고.. 그리고 cmd, option 같은거 눌렀을 때 해당하는 단축키가 ui 상에 보이는 것도 빠져있노!", "우측에 그냥 floating 되서 보이도록? 굳이 기존 자리를 차지하지 않게!"
시각 참조는 `agents/runs/ux-fixes-2026-09-27/design/board-v3.pen` 섹션 1(C열)과 섹션 2(B열).

## Non-goals

- 브라우저 호스트에는 ⌘1-9·⌥1-9 chord를 두지 않는다(Chrome 예약). 결과: 브라우저 탭에서 쓰는 사용자는 번호 이동이 없다. 재검토: 사용자가 브라우저 호스트용 대체 chord(예: ⌥ 조합)를 요청할 때 (D-02).
- 힌트는 chord가 있는 호스트에서만 뜬다: 브라우저 호스트에서 ⌘·⌥ 홀드는 아무것도 보이지 않는다(보일 번호가 없으므로). 재검토: 위와 같음 (D-03).
- 10번째 이후 탭·에이전트에는 번호가 없다 (D-02).
- 접힌 자식 에이전트 행은 번호를 받지 않는다 (D-02).
- 사이드바 Projects 탭의 에이전트 행에는 번호를 두지 않는다; Agents 탭만 (D-02).
- Pen 라이브러리·Screen 시트는 바꾸지 않는다; 보드 v3가 참조다 (D-05).
- engineering/principles.md 규칙 13(상태를 모델링, 두 번째 문자열 매칭 금지)이 D-04의 힌트 상태 모듈을 정한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Swift 앱은 ⌘n 탭, ⌥n 에이전트, modifier 홀드 힌트(`HideHintState`: 정확한 조합을 딜레이 후 노출, 놓거나 앱 비활성·시트 열림에 해제)가 있었다. 웹 레지스트리에는 셋 다 없고 힌트는 hover 툴팁뿐이다. | 저장소 사실 (qa-log D-01: web/src/shortcuts.ts, keyboard.ts; Swift HerdrApp.swift 644-655, HideHintState.swift는 T0에서 삭제되므로 git 이력 684c4989 기준) |
| D-02 | 레지스트리에 `select_tab_1..9`(Electron ⌘1-9)와 `select_agent_1..9`(Electron ⌥1-9)를 추가; 브라우저 chord는 null. 번호는 웹이 부여: 탭은 앞 워크스페이스 탭 스트립 왼쪽부터, 에이전트는 사이드바 Agents 탭의 보이는 행(접힌 자식 제외) 위에서부터, 1-9. 빈 번호는 무시. 단축키 시트와 Settings 바인딩에 나타나며 재바인딩 규칙은 기존과 같다. | 사용자: "cmd + 1,2,3 -> 탭 전환이 없네, option 1,2,3 이것도 없고", "Electron 기준으로" (qa-log D-02) |
| D-03 | ⌘ 또는 ⌥을 정확히 그 조합으로 잠깐 누르고 있으면 번호 키캡이 나타난다: 탭은 탭 우상단, 에이전트 행은 행 우측 위. 키캡은 popover 배경·border·작은 그림자·caption 크기 mono 숫자이고 절대 위치로 떠서 기존 레이아웃(시간, chevron, 탭 폭)을 밀지 않는다. modifier를 놓거나, 앱이 비활성화되거나, 시트·메뉴가 열리면 사라진다. 딜레이·해제 규칙은 Swift `HideHintState`와 같다. | 사용자: "우측에 그냥 floating 되서 보이도록? 굳이 기존 자리를 차지하지 않게!" (qa-log D-03) |
| D-04 | 가정: 힌트 상태는 순수 함수 모듈(modifier 집합, deadline, revealed; 시간 주입)로 두고 keydown/keyup/blur/visibilitychange를 한 곳에서 듣는다; 어떤 조합이 어떤 힌트를 드러내는지는 레지스트리 chord에서 계산한다. | 가정; engineering 규칙 13 (qa-log D-04) |
| D-05 | 가정: UI_BEHAVIOR.md 583행 힌트 문장과 단축키 절, ARCHITECTURE.md 레지스트리 절을 갱신한다. 키캡은 web/src/components/ui의 컴포넌트로 만들고 Pen 시트는 바꾸지 않는다. | 가정 (qa-log D-05) |
| D-06 | 가정: 검증: vitest(chord·번호 부여·힌트 상태기계), 웹 e2e 1개, Electron e2e 1개, CI에서 실행, 로컬은 한 번. 각 e2e의 호스트와 내용은 D-09가 정한다. | 가정 (qa-log D-06) |
| D-09 | 가정: Electron e2e 2개(⌘2 탭 전환; ⌘ 홀드 시 키캡 노출과 놓으면 해제). 웹(브라우저 호스트) e2e는 "⌘·⌥ 홀드에 키캡이 뜨지 않음"만 확인한다(chord가 null이라 보일 번호가 없음). 키캡 렌더 규칙은 vitest. | 가정 (qa-log D-09) |
| D-07 | T1(⌘W 정책), T2(사이드바 행), T4(탭 구성)가 main에 머지된 뒤 시작. please, Claude Implementor --effort high, PR 배포, Observer 자동 머지. | 사용자 승인 의존 표 (qa-log D-07) |
| D-08 | 가정: 원칙 intake: engineering/principles.md와 design/principles.md(oh-my-principle 654485f)를 읽었다. engineering 13이 D-04에, design 7(시각 부호)·8(컨테이너는 정보를 실을 때만: 키캡은 번호라는 정보)이 D-03에 반영됐다. | 가정 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Electron 앱에서 ⌘n(1-9)을 누르면 앞 워크스페이스 탭 스트립의 n번째 탭이 앞에 온다; 그 번호에 탭이 없으면 아무 일도 없다. | D-02 |
| B2 | Electron 앱에서 ⌥n(1-9)을 누르면 사이드바 Agents 탭의 n번째 보이는 에이전트 행이 열린다(그 행 클릭과 같음: 해당 pane으로 이동); 접힌 자식 행은 세지 않는다. | D-02 |
| B3 | 브라우저 호스트에서는 두 chord가 등록되지 않아 단축키 시트에 "이 호스트에 없음"으로 보이고 아무 동작도 가로채지 않으며, ⌘·⌥ 홀드에 키캡도 뜨지 않는다. | D-02, D-03 |
| B4 | 단축키 시트와 Settings의 바인딩 목록에 Select tab 1-9와 Select agent 1-9가 그룹으로 보이며, 기존 규칙대로 재바인딩과 충돌 검사가 된다. | D-02 |
| B5 | Electron 앱에서 ⌘만 정확히(다른 modifier 없이) 누르고 딜레이가 지나면 각 탭의 우상단에 그 번호 키캡이 뜨고, ⌥만 누르면 사이드바 Agents 탭의 각 보이는 에이전트 행 우측 위에 번호 키캡이 뜬다. 키캡은 시간·chevron·탭 폭을 밀지 않는다. | D-03 |
| B6 | modifier를 놓거나, 다른 modifier가 더해지거나, 창이 비활성화되거나, 시트·메뉴·팔레트가 열리면 키캡이 즉시 사라진다; 홀드 중 키를 눌러 이동해도 그 순간 사라진다. | D-03 |
| B7 | 키캡은 popover 배경·border·작은 그림자·mono 숫자로 그려지고 Light/Dark 토큰을 쓴다; 번호가 없는 탭·행에는 키캡이 없다. | D-03 |
| B8 | 힌트 딜레이 전에 modifier를 놓으면 아무것도 나타나지 않는다(⌘C 같은 조합에 힌트가 깜빡이지 않음). | D-03, D-04 |
| B9 | 기존 hover 툴팁의 단축키 표기는 그대로이고, 힌트와 툴팁이 동시에 뜨면 서로 겹치지 않는다. | D-03 |
| B10 | UI_BEHAVIOR.md와 ARCHITECTURE.md의 단축키·힌트 절이 새 동작을 서술한다. | D-05 |

## Technical structure

- web: `shortcuts.ts` 레지스트리 항목 18개, `keyboard.ts`의 실행 분기(번호 → 탭/에이전트 선택), 새 힌트 상태 모듈과 리스너, 키캡 컴포넌트를 `TabBar.tsx`와 사이드바 에이전트 행에 오버레이.
- desktop: 메뉴 항목이 필요하면 기존 명령 전달 경로(`onCommand`)로; 새 IPC 없음.
- 바뀌지 않음: core, Herdr 계약, 사이드바 데이터.

## Risks

- ⌘1-9는 Electron 메뉴 가속키와 겹칠 수 있다; 앱 메뉴에 같은 chord를 두지 않거나 메뉴가 셸로 전달하게 한다.
- 번호는 화면 순서라 탭 재정렬·에이전트 정렬 변화에 따라 바뀐다; 홀드 힌트가 그 순간의 번호를 보이므로 사용자가 추측하지 않는다.
- 사용자가 미리 해야 할 일: 없음.
