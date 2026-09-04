---
topic: "Hide paths and tab perf: terminal path links into the Explorer, wheel scroll coalescing, attach session release, split-workspace tab reorder"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "Adds a click path that hands files outside a checkout to the default app with an executable guard, and changes attach lifetime and the scroll wire; no data, credential, billing, or destructive action, and every live effect is confined to a throwaway workspace and windows the run closes again."
source_intake: "current conversation"
created_at: "2026-09-05"
updated_at: "2026-09-05"
---

# PRD: Hide paths and tab perf

## 1. Summary

터미널에 찍힌 파일이나 폴더 경로를 클릭하면 Explorer 파일 트리가 그 항목으로 따라가고, 체크아웃 바깥 경로는 hide 바깥에서 열린다.
같은 라운드에 2026-09-04 전면 리뷰가 남긴 상위 세 결함을 묶는다.
휠 스크롤이 행마다 same-size resize를 보내는 폭주, 떠난 탭의 attach 세션이 영원히 남는 누수와 view 없는 pane의 무한 버퍼, 그리고 두 herdr 워크스페이스가 한 경로를 공유하는 체크아웃에서 모든 탭 드래그가 거절되는 문제다.

경로 클릭 뒤의 화면 상태(포커스 체크아웃, 오른쪽 패널 가시성과 섹션, 트리의 펼침과 선택, 에디터 탭)는 core가 한 이벤트로 결정한다.
셸은 경로를 파일시스템에서 해석하고, 체크아웃 바깥 경로만 macOS에 넘긴다.

Approval checklist:

- 경로 클릭의 세 갈래: 체크아웃 안 파일은 에디터 탭과 트리 드러냄, 체크아웃 안 폴더는 트리 펼침과 선택, 바깥 파일은 기본 앱, 바깥 폴더는 Finder (R1-R4, section 3).
- 바깥 경로 중 실행 파일, 앱 번들, 설치 패키지는 실행하지 않고 Finder에서 드러낸다는 가정 (4.3 A1).
- 구조 변경: 경로 드러냄이 core 이벤트 하나가 되고, attach 세션에 최근 방문 상한과 관측 가능한 `released` 상태가 생기며, 스크롤 쓰기가 프레임 단위로 합쳐진다 (section 5).
- attach 상한: 보이는 탭과 최근 방문 4개 탭만 attach를 유지한다는 가정 (4.3 A2, R7).
- 대화 중 추가된 두 결함: Cmd+W로 pane을 닫을 때 "attach ended"가 한순간 보이는 것과, fork가 모달만 띄우고 결과를 보이지 않는 것 (R11, R12, 4.3 D7).
- 검증 모드: build/static, automated behavior, app runtime, performance measurement, throwaway 워크스페이스 한정 live herdr integration (section 9.1).
- 배포 모드: local. 실행 브랜치에 커밋 하나, push와 PR 없음 (4.3).
- `review_profile: standard`와 그 근거 (frontmatter).

## 2. Problem, Goal, And Users

사용자는 Hide의 단일 운영자다.
에이전트가 터미널에 파일 경로를 찍으면 운영자는 그 파일을 열어 보고, 폴더면 트리에서 그 자리로 가고 싶어 한다.
지금은 파일 클릭이 에디터 탭만 열고 트리는 그대로이며, 폴더 클릭은 "folder. Terminal links open files."라는 이유를 trace에만 남기고 화면에는 아무 일도 없다.
체크아웃 바깥 경로는 절대 경로면 포함 검사 없이 에디터로 열리고 폴더면 무시된다.

리뷰가 남긴 세 결함은 운영자가 매일 느끼는 것들이다.

- 휠 스크롤은 행이 바뀔 때마다 `terminal.scroll`과 same-size `terminal.resize`를 한 번의 쓰기로 보내고, 초당 65회 버스트가 그대로 herdr 서버의 재렌더 65회가 된다.
  크기를 모르는 pane은 24x80으로 resize를 보내 실제 PTY 크기를 덮어쓴다.
- attach 세션은 탭이나 체크아웃을 떠나도 풀리지 않아 자식 프로세스와 스트림이 방문한 탭 수만큼 쌓이고, 셸의 view 없는 pane 버퍼는 상한이 없다.
- 한 경로를 두 herdr 워크스페이스가 공유하는 체크아웃(이 저장소의 w2X와 w4G)에서는 순서를 바꾸는 드래그가 전부 `tab.reorder_split_workspace`로 거절된다.

PRD를 쓰는 동안 사용자가 두 가지를 더 보고했다.
에이전트가 없는 pane을 Cmd+W로 닫으면 pane이 사라지기 직전에 "terminal attach ended" 안내가 한순간 보인다.
herdr가 PTY를 닫으면 attach 자식이 먼저 끝나고 core가 그 종료를 `ended`로 투영해 안내 청크와 오버레이를 그린 뒤에야 `pane_closed` 이벤트가 도착해 pane을 치우기 때문이다.
그 종료는 이미 `terminal_closed`로 분류되어 있으므로 실패가 아니라 닫힘의 결과다.
pane 헤더의 fork는 "Forking w2X:p2F into a sibling pane. The agent has to start before it appears."라는 모달만 띄우고, 성공이든 실패든 snapshot의 상태 목록에만 기록되어 stderr에도 화면에도 결과가 남지 않는다.
진행 중인 동작을 모달로 알리는 것은 분할이 헤더 접미("splitting right…")로 알리는 기존 패턴과도 어긋난다.

목표는 경로 클릭이 트리와 에디터를 한 동작으로 맞추고, 스크롤과 attach가 사용량에 비례해 비용을 내며, 드래그가 어느 체크아웃에서든 자리를 잡고, pane을 닫거나 fork하는 동작이 결과를 있는 그대로 보이는 것이다.

### 2.1 User Scenarios

- SC1. 체크아웃 안 파일 클릭: 운영자가 터미널에 찍힌 파일 경로를 클릭해 에디터와 Explorer가 함께 그 파일로 간다.
  Actors: 운영자.
  Primary path: 오른쪽 패널이 숨겨져 있거나 Changes 섹션을 보이는 상태에서 다른 체크아웃의 파일 경로(`:line:col` 접미 포함)를 클릭한다.
  그 체크아웃으로 전환되고, 오른쪽 패널이 Explorer 섹션으로 보이며, 트리는 조상이 펼쳐진 채 그 파일을 선택하고, 파일의 에디터 탭이 열린다.
  Failure state: 클릭 시점에 파일이 이미 지워졌으면 아무것도 열리지 않고 이유가 trace에 남는다(기존 미해석 링크와 같은 조용한 no-op).
  Recovery: 파일이 다시 생기면 같은 클릭이 정상 동작한다.
  Reach: 두 체크아웃이 등록된 상태에서 한 체크아웃의 pane이 다른 체크아웃의 파일 경로를 출력한다. 두 번째 체크아웃과 그 pane은 fixture가 만든다(T1).
- SC2. 체크아웃 안 폴더 클릭: 운영자가 폴더 경로를 클릭해 트리가 그 폴더를 펼친다.
  Actors: 운영자.
  Primary path: 아직 펼친 적 없는 깊은 폴더의 경로를 클릭한다. 오른쪽 패널이 Explorer 섹션으로 보이고, 트리는 조상과 그 폴더를 펼치고 폴더를 선택하며, 에디터 탭은 열리지 않는다.
  Failure state: 클릭 시점에 폴더가 지워졌으면 트리는 바뀌지 않고 이유가 trace에 남는다.
  Recovery: 폴더가 다시 생기면 같은 클릭이 정상 동작한다.
  Reach: fixture 체크아웃 안에 세 단계 깊이의 폴더가 있고 pane이 그 경로를 출력한다(T1).
- SC3. 체크아웃 바깥 경로 클릭: 운영자가 등록된 어느 체크아웃에도 속하지 않는 경로를 클릭해 macOS가 연다.
  Actors: 운영자.
  Primary path: 바깥의 텍스트 파일은 기본 앱에서 열리고, 바깥 폴더는 Finder 창으로 열리며, 바깥의 실행 파일이나 앱 번들은 실행되지 않고 Finder에서 선택된 채 드러난다. hide의 트리, 탭, 체크아웃은 바뀌지 않는다.
  Failure state: macOS가 열기를 거절하면(연결된 앱이 없는 경우 등) 상호작용 알림에 이유가 보이고 trace에 남는다.
  Recovery: 운영자가 다른 경로를 클릭하면 정상 동작하고, 알림은 다음 성공에서 사라진다.
  Reach: fixture가 체크아웃 밖 임시 폴더에 텍스트 파일, 하위 폴더, 실행 비트가 켜진 스크립트를 만들고 pane이 세 경로를 출력한다(T1).
- SC4. 긴 휠 스크롤: 운영자가 에이전트 출력이 쌓인 pane을 휠로 길게 스크롤한다.
  Actors: 운영자.
  Primary path: 5초 동안 이어지는 휠 입력에 스크롤 위치는 입력 그대로 따라오고, herdr 세션으로 나가는 쓰기는 pane당 프레임마다 최대 하나다.
  Failure state: 크기 보고가 아직 없는 pane에 휠이 들어오면 resize는 나가지 않고 진단만 남는다.
  Recovery: 크기 보고가 도착하면 다음 휠부터 정상 동작한다.
  Reach: fixture pane이 수천 줄을 출력한 상태에서 스크립트가 휠 이벤트를 보낸다(T1, T10).
- SC5. 탭을 떠났다가 돌아오기: 운영자가 여러 탭과 체크아웃을 오간 뒤 오래전에 본 탭으로 돌아온다.
  Actors: 운영자.
  Primary path: 여덟 개 탭을 차례로 방문하면 보이는 탭과 최근 방문 네 탭의 pane만 attach가 남고, 나머지는 세션이 풀려 `released`로 관측된다. 풀린 탭으로 돌아오면 첫 방문과 같이 attach되어 현재 화면이 그려진다.
  Failure state: 풀린 pane은 사이드바에서 오류나 죽은 pane으로 보이지 않고, 세션 tick은 풀린 pane을 다시 attach하지 않는다.
  Recovery: 돌아온 탭의 attach가 실패하면 기존 attach 실패와 같은 상태(`ended`/`unavailable`)와 진단으로 드러나고, 다음 방문이 다시 시도한다.
  Reach: throwaway 워크스페이스에 탭 여덟 개를 만드는 fixture(T1).
- SC6. 분할 워크스페이스 체크아웃의 탭 드래그: 운영자가 두 herdr 워크스페이스가 섞인 탭 스트립에서 탭을 끌어 옮긴다.
  Actors: 운영자.
  Primary path: 같은 워크스페이스의 탭 앞으로 끌면 그 워크스페이스 안에서 herdr `tab.move` 하나가 나가고 스트립이 그 자리에 확정된다. 다른 워크스페이스의 탭 사이로만 끌면 herdr 호출 없이 스트립 순서가 로컬로 확정된다. 어느 쪽도 다음 snapshot에서 되돌아가지 않는다.
  Failure state: herdr가 이동을 거절하면 스트립은 이전 순서로 돌아오고 진단이 거절을 기록한다.
  Recovery: 같은 드래그를 다시 하면 정상 동작한다.
  Reach: 한 경로에 두 herdr 워크스페이스를 만들어 한 체크아웃에 두 워크스페이스의 탭이 섞이게 하는 fixture(T1).
- SC7. 에이전트 없는 pane 닫기: 운영자가 셸만 도는 pane을 Cmd+W로 닫는다.
  Actors: 운영자.
  Primary path: pane이 사라질 때까지 그 pane의 화면은 마지막 내용 그대로이고, "attach ended"나 "transport is unavailable" 안내는 한 프레임도 보이지 않는다.
  Failure state: herdr가 닫기를 거절하면 pane은 남고 attach도 그대로이며 거절이 진단에 남는다.
  Recovery: 같은 Cmd+W를 다시 하면 정상 동작한다.
  Reach: fixture 워크스페이스에 에이전트 없는 pane을 하나 둔다(T1).
- SC8. pane fork: 운영자가 에이전트가 도는 pane의 헤더에서 fork를 누른다.
  Actors: 운영자.
  Primary path: 모달 없이 그 pane의 헤더가 "forking…" 접미로 진행을 보이고, 에이전트가 시작되면 오른쪽에 fork 표시가 붙은 형제 pane이 나타나며 접미가 사라진다.
  Failure state: fork가 실패하면 접미가 사라지고 실패 이유가 상호작용 알림과 stderr 진단에 남는다.
  Recovery: 같은 fork를 다시 누르면 새 시도가 된다. 진행 중에 다시 누른 것은 지금처럼 이름으로 거절된다.
  Reach: fixture 워크스페이스에 fork 가능한 claude 에이전트 pane을 하나 둔다(T1).

## 3. Scope And Non-Goals

포함:

- 로컬 컨텍스트의 터미널 링크 라우팅: 체크아웃 안 파일, 체크아웃 안 폴더, 바깥 파일, 바깥 폴더, 실행 파일과 앱 번들의 구분 (R1-R5).
- 경로 드러냄에 필요한 core 이벤트와 셸의 외부 핸드오프 (R2-R5).
- 휠 스크롤 쓰기의 프레임 단위 합치기와 크기 미상 pane의 폴백 삭제 (R6).
- attach 세션의 최근 방문 상한, `released` 상태, 재방문 attach, 셸 pane 버퍼 상한 (R7, R8).
- 분할 워크스페이스 체크아웃의 탭 재정렬 소유권 판정과 거절 경로 삭제 (R9).
- AGENTS.md의 런타임 아키텍처와 성능 가이드 갱신 (R10).
- hide가 요청한 pane 닫기의 attach 종료를 실패로 투영하지 않는 것 (R11).
- fork의 진행과 결과를 헤더 접미, 알림, stderr 진단으로 보이고, 현재 fork가 pane을 만들지 못하는 원인을 재현해 고치는 것 (R12).
- 위 전부에 대한 회귀 테스트, 런타임 증거, 성능 측정.

비목표:

- 원격(SSH) 컨텍스트의 링크는 지금처럼 웹 주소만 연다. 원격 파일은 읽기 전용 snapshot 계약이 가져올 수 없다. 원격 편집 계약이 생기면 다시 본다.
- 해석되지 않는 링크는 지금처럼 화면에 아무것도 띄우지 않고 trace만 남긴다. 감지가 임의 출력 위의 추측이라 오탐이 일상이고, 모달은 운영자에게 잘못 클릭한 대가를 치르게 한다는 기존 결정을 유지한다.
- `:line:col`로 에디터 커서를 옮기는 것. 에디터 탭은 지금처럼 파일 단위로 열린다. 에디터가 위치 이동을 지원하면 다시 본다.
- herdr가 보이지 않는 pane을 서버에서 렌더하는 비용. herdr 쪽 변경이며 이 저장소 밖이다.
- 우선순위 4 이하 항목: Cmd+W 레이블과 중복 단축키, herdr에 저장된 `Tab 2` 레이블 네 개, layout이 오지 않은 탭의 스트립 누락, 원격 읽음 기록. 메모리에 기록된 대로 다음 라운드로 넘긴다.
- 사용자가 열어 둔 pane, 탭, 워크스페이스에 대한 어떤 변경. fixture 워크스페이스 w5D-w5H는 9월 7일 재검증을 위해 그대로 둔다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

- 없음. 자격 증명, 계정, 구매, 권한 부여가 필요한 항목이 없고 fixture는 에이전트가 만든다.

### 4.2 Human Decisions Before PRD Approval

None required.
모든 제품 결정이 대화에 근거하거나 4.3의 되돌릴 수 있는 가정으로 기록되어 있다.

### 4.3 Decision Traceability For Fidelity Review

사용자 결정(2026-09-05 대화):

- D1. 기능 제안 원문: "특정 파일이나 폴더에서 그거 누르면 Explorer 의 파일트리에서 그게 실행되게 하면 안되나? 그래서 파일이면 그냥 그게 탭으로 열리는거고 그게 아니면 그냥 파일트리에 해당 폴더가 열리는거고? 근데 이제 그 스코프 밖이면 그냥 여기 hide 바깥에서 뭔가 열어버리는그런느낌?" → R1-R4, SC1-SC3.
- D2. "hide 바깥에서 연다"의 뜻을 두 안으로 물었다: 모두 Finder에서 드러냄, 또는 파일은 기본 앱과 폴더는 Finder. 사용자 "1번은 후자로" → R4의 파일은 기본 앱, 폴더는 Finder.
- D3. 폴더 클릭이 오른쪽 패널을 자동으로 보이게 하고 필요하면 체크아웃을 전환해도 되는지 물었다. 사용자 "2번은 ㅇㅇ" → R2, R3의 패널 표시와 체크아웃 전환.
- D4. "1,2,3 으로 같이 묶어서 해주면 좋을듯" → 우선순위 1 휠 스크롤 폭주(R6), 2 attach 누수와 버퍼 상한(R7, R8), 3 분할 워크스페이스 드래그 거절(R9)을 이 PRD에 묶는다. 우선순위 4-6은 비목표.
- D5. "이거 바로 진행해볼래? /please" → 대화가 인터뷰이고 사람의 PRD 승인 왕복은 이 호출 기록으로 대체된다. `human_approval`은 `pending`으로 남는다.
- D7. PRD 작성 중 사용자 보고: "pane (아직 agent 안 킨 상태) 에서 cmd +w 로 pane종료하면 잠깐 terminal attach ended 가 나왔다가 사라져. 이거 왜 보이는거야? 그리고 이거 fork 하면 모달이 뜨고 아무것도 안일어나는데 (cmd+d효과)옆에 pane 띄워서 forke된거가 약간 다르게 해서 표시되면 좋을 것 같은데.. 이것들에 대해 어떻게 생각해" → 원인은 section 2에 답했고, 둘 다 이 PRD에 넣었다(R11, R12, SC7, SC8). 사용자가 묻기만 했으므로 포함은 가정 A12다.
- D6. 이번 대화 초반의 지시(코드 리뷰 뒤 고쳐서 빌드하고 설치)와 이전 PRD의 지시(Implementor는 opus)는 이 라운드에도 이어지는 운영 관행으로 읽는다 → A8, A9.

에이전트 가정(되돌릴 수 있음, 사용자가 사후 거부 가능):

- A1. 바깥 경로 중 실행 비트가 켜진 파일, 앱 번들(`.app`), 설치 패키지(`.pkg`, `.dmg`)는 기본 앱으로 열지 않고 Finder에서 선택된 채 드러낸다. 링크 감지는 임의 에이전트 출력 위에서 돌고 기본 앱 열기는 실행 파일을 실행하므로, 클릭 한 번이 프로그램 실행이 되지 않게 한다. "후자"는 파일을 보는 이야기였다 → R4, AC1, AC4.
- A2. attach 상한은 화면에 보이는 탭 하나에 최근에 보였던 탭 네 개를 더한 다섯 탭이며, 체크아웃을 가로질러 전역으로 센다. 시간 기반 만료는 core에 타이머가 없고 검증이 비결정적이라 택하지 않았다 → R7, AC8, AC9.
- A3. "스코프"는 This Mac 내비게이터에 등록된 모든 체크아웃이다. 경로는 양쪽 다 심볼릭 링크를 푼 뒤 가장 긴 접두 일치로 소유 체크아웃을 정하고, 어느 체크아웃에도 속하지 않으면 바깥이다 → R1, AC1.
- A4. 폴더 클릭은 조상만이 아니라 그 폴더 자체를 펼치고 선택한다. 파일 클릭도 트리에서 그 파일을 선택한다 → R2, R3.
- A5. 드러냄은 오른쪽 패널이 Changes 섹션에 있어도 Explorer 섹션으로 바꾼다. 운영자가 클릭한 것은 트리에서 보겠다는 뜻이다 → R2, R3, AC5.
- A6. 스크롤 합치기 창은 한 프레임(16 ms)이고 pane마다 따로 센다. same-size resize는 herdr 0.8.2에서 `terminal.scroll`만으로 프레임이 오지 않는다는 것이 라이브 확인으로 유지될 때만 남기고, 프레임이 오면 삭제한다 → R6, T4.
- A7. 풀린 세션은 pane의 transport 상태 `released`와 진단 하나로 관측된다. 새 필드가 아니라 기존 상태 열거에 값 하나를 더한다 → R7.
- A8. 배포는 config대로 local이다. 영수증이 완료되면 Observer가 실행 브랜치를 main에 병합하고 dev 번들을 빌드해 `/Applications/hide.app`에 설치하되 날짜가 붙은 롤백 복사본을 남긴다. 이는 AC가 아니라 사용자가 답장으로 취소할 수 있는 Observer 단계다.
- A9. Implementor는 opus, high effort로 띄운다. 새 지시가 없으므로 이전 라운드의 지시를 잇는다.
- A10. 셸의 view 없는 pane 버퍼 상한은 core의 보존 청크 상한과 같은 512 청크다 → R8, AC10.
- A12. 사용자가 의견을 물은 두 결함을 이 PRD에 포함한다. 닫기 깜빡임은 T5가 만지는 세션 생명주기 투영에 붙고, fork는 모달을 헤더 접미로 바꾸는 작은 셸 변경과 결과 진단이며, "다르게 표시"는 이미 있는 헤더 fork 표시를 그대로 쓴다. fork가 pane을 못 만드는 원인이 herdr 쪽이면 고치지 않고 재현 기록과 함께 보고한다 → R11, R12.
- A11. 바깥 파일을 기본 앱으로 여는 검증은 fixture의 텍스트 파일로 하고, 실행이 열어 둔 창은 실행이 닫는다 → guardrail.

거부되거나 미룬 것:

- 바깥 경로도 hide의 에디터로 여는 안. 사용자가 "hide 바깥"을 명시했다.
- 바깥 경로를 모두 Finder에서 드러내는 안(D2의 전자). 사용자가 후자를 골랐다.
- 시간 기반 attach 만료. A2 참조.

## 5. Major Technical Structure Changes

- 경로 드러냄이 core 이벤트 하나가 된다. 셸은 경로를 파일시스템에서 해석하고 소유 체크아웃을 붙여 보내며, core가 그 이벤트 안에서 체크아웃 전환, 오른쪽 패널 가시성과 섹션, 트리의 펼침과 선택, 에디터 탭 열기를 원자적으로 결정한다. 지금은 셸이 포커스된 체크아웃 id로 파일 열기를 보내고 core가 다른 체크아웃을 거절하며, dispatch가 fire-and-forget이라 셸이 전환과 열기를 순서대로 조합할 수 없다.
- attach 세션에 생명주기 상한이 생긴다. core가 최근 방문 탭 집합을 유지하고 그 밖의 pane 세션을 풀며, 풀린 상태를 snapshot과 진단으로 드러낸다. 유지되는 캔버스는 지금처럼 attach 집합에 묶이므로 풀린 pane의 캔버스도 함께 사라진다.
- 스크롤 쓰기 경로에 pane별 프레임 합치기가 들어간다. 새 경계는 없고 기존 터미널 세션 쓰기 경로 안에서 순 델타를 모아 프레임마다 한 번 쓴다.
- 탭 재정렬의 소유권 판정이 체크아웃 단위에서 드래그 단위로 바뀐다. 거절 경로와 그 진단은 삭제된다.
- 새 외부 경계: 셸이 macOS에 파일 열기와 Finder 드러냄을 위임한다. 지금 웹 주소를 브라우저에 넘기는 것과 같은 종류의 경계다.

DB, 인증, 결제, 원격 서비스 변경은 없다.

## 6. Requirements

- R1. 로컬 컨텍스트에서 터미널 링크가 파일시스템 경로로 해석되면(`:line:col` 접미는 벗긴 뒤), 심볼릭 링크를 푼 실제 경로를 This Mac의 모든 체크아웃 경로와 비교해 가장 긴 접두 일치 체크아웃을 소유자로 정한다. 어느 체크아웃에도 속하지 않으면 바깥 경로다.
- R2. 체크아웃 안 파일 클릭은 한 core 이벤트로 소유 체크아웃을 포커스하고, 오른쪽 패널을 Explorer 섹션으로 보이게 하며, 트리에서 조상을 펼치고 그 파일을 선택하고, 파일의 에디터 탭을 연다.
- R3. 체크아웃 안 폴더 클릭은 같은 이벤트 종류로 소유 체크아웃을 포커스하고, 오른쪽 패널을 Explorer 섹션으로 보이게 하며, 조상과 그 폴더를 펼치고 폴더를 선택하되 에디터 탭은 열지 않는다.
- R4. 바깥 파일은 기본 앱으로 열리고, 바깥 폴더는 Finder 창으로 열린다. 실행 비트가 켜진 파일, 앱 번들, 설치 패키지는 Finder에서 선택된 채 드러나고 실행되지 않는다. macOS가 거절하면 상호작용 알림에 이유가 보인다. hide의 트리, 탭, 체크아웃은 바뀌지 않는다.
- R5. 경로 클릭의 모든 결과(탭 열림, 트리 드러남, 기본 앱 열림, Finder 드러냄, 거절, 미해석)는 trace 또는 진단으로 프로세스 바깥에서 읽을 수 있고 어느 갈래였는지와 이유를 담는다.
- R6. 휠 스크롤은 pane마다 한 프레임(16 ms) 안의 행 델타를 합쳐 프레임당 최대 한 번 herdr 세션에 쓴다. 순 델타가 0이면 쓰지 않는다. 합친 결과의 최종 스크롤 위치는 합치지 않은 순서와 같다. 크기 보고가 없는 pane에는 resize를 보내지 않고 진단을 남긴다. same-size resize 재도색은 herdr 0.8.2 라이브 확인으로 필요할 때만 남기고, 그 확인 결과를 코드 주석과 AGENTS.md에 기록한다.
- R7. 화면에 보이는 탭과 최근에 보였던 네 탭의 pane만 attach를 유지한다. 그 밖의 attach 세션은 풀리고, 풀린 pane은 transport 상태 `released`와 진단으로 관측되며 사이드바에서 오류로 보이지 않는다. 세션 tick은 풀린 pane을 다시 attach하지 않고, 탭을 다시 보일 때 첫 방문과 같이 attach된다. 풀린 pane의 유지 캔버스는 세션과 함께 사라진다.
- R8. 셸의 view 없는 pane 바이트 버퍼는 pane당 512 청크를 넘지 않고 넘치면 가장 오래된 청크를 버리며, pane의 세션이 풀리거나 pane이 사라지면 비워진다. 버린 사실은 trace에 남는다.
- R9. 탭 드래그는 옮기는 탭의 herdr 워크스페이스 부분수열이 바뀔 때만 그 워크스페이스 안 삽입 인덱스로 `tab.move` 하나를 보내고, 다른 워크스페이스의 탭 사이로만 옮기면 herdr 호출 없이 스트립 순서를 로컬로 확정한다. herdr 응답의 워크스페이스 순서는 다른 워크스페이스의 탭이 섞인 체크아웃 순서와 대조해 확정된다. 체크아웃 단위 거절 경로와 `tab.reorder_split_workspace` 진단은 삭제된다.
- R11. hide가 닫기를 요청한 pane, 또는 herdr가 이미 없다고 알린 pane의 attach 종료(`terminal_closed`)는 `ended`로 투영되지 않고 안내 청크도 붙지 않는다. pane은 마지막 화면 그대로 사라진다. 다른 이유의 종료는 지금처럼 `ended`와 이유로 보인다.
- R12. fork는 모달 대신 그 pane의 헤더 접미로 진행을 보이고, 성공은 진단으로, 실패는 상호작용 알림과 stderr 진단으로 이유를 남긴다. fork로 생긴 pane은 헤더에 fork 표시가 붙는다. fixture의 claude pane에서 fork를 재현해 지금 pane이 나타나지 않는 원인을 확인하고, 원인이 이 저장소 안이면 고친다.
- R10. AGENTS.md의 런타임 아키텍처 단락은 경로 드러냄 이벤트, attach 상한과 `released`, 드래그 단위 소유권을 기술하고, 성능 가이드는 스크롤 쓰기가 프레임 단위인 이유와 그 사건을 기록한다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | 링크 해석기는 체크아웃 안 파일을 소유 체크아웃과 함께 파일 갈래로, 체크아웃 안 폴더를 폴더 갈래로, 바깥 텍스트 파일을 기본 앱 갈래로, 바깥 폴더를 Finder 갈래로, 바깥의 실행 파일과 앱 번들을 Finder 드러냄 갈래로 분류하며, 심볼릭 링크 차이(`/tmp`와 `/private/tmp`)와 중첩 체크아웃에서 가장 긴 접두 일치를 고른다 | machine | - |
| AC2 | 오른쪽 패널이 숨겨져 있거나 Changes 섹션인 상태에서 다른 체크아웃의 파일 경로를 클릭하면 그 체크아웃으로 전환되고 패널이 Explorer 섹션으로 보이며 트리가 조상을 펼친 채 그 파일을 선택하고 에디터 탭이 열린다 | judged | scripted run in a suffixed dev instance: hide the panel, click the printed path in the fixture pane, capture the window before and after, and record the trace lines for the click |
| AC3 | 펼친 적 없는 깊은 폴더 경로를 클릭하면 패널이 Explorer 섹션으로 보이고 트리가 조상과 그 폴더를 펼치고 폴더를 선택하며 에디터 탭은 열리지 않는다 | judged | scripted run: click the printed folder path, capture the window after, and record the open editor tab set before and after |
| AC4 | 바깥 텍스트 파일 클릭은 기본 앱을, 바깥 폴더 클릭은 Finder 창을 앞에 띄우고, 실행 비트가 켜진 바깥 스크립트 클릭은 실행하지 않고 Finder에서 선택된 채 드러내며, hide의 체크아웃, 트리, 탭은 바뀌지 않는다 | judged | scripted run: click each of the three fixture paths, capture the frontmost window after each, show the script left no execution marker, and record the trace lines |
| AC5 | 다른 체크아웃의 파일을 향한 드러냄 이벤트 뒤 snapshot은 그 체크아웃을 포커스로, 오른쪽 패널을 보임과 Explorer 섹션으로, 선택 경로를 그 파일로, 펼침 경로에 모든 조상을 담고 파일의 에디터 탭을 열어 두며, 폴더를 향한 이벤트는 같은 상태에 폴더 자체를 펼치고 탭을 열지 않으며, 등록되지 않은 체크아웃을 향한 이벤트는 상태를 바꾸지 않고 오류를 남긴다 | machine | - |
| AC6 | 한 프레임 안에 들어온 휠 행 100개는 순 델타를 담은 쓰기 하나가 되고, 서로 상쇄되는 델타는 쓰기 0개가 되며, 크기 보고가 없는 pane의 휠은 쓰기 0개와 진단 하나가 되고, 어떤 순서의 휠도 합치지 않은 순서와 같은 최종 위치에 닿는다 | machine | - |
| AC7 | 같은 5초 휠 버스트를 기준 빌드와 새 빌드에 걸었을 때 새 빌드의 app 표본과 herdr server 표본이 기준보다 낮거나 같고, 두 측정 모두 load와 인스턴스 상태가 함께 기록된다 | judged | two sampled windows per build on the fixture pane under a scripted wheel burst, each quoting load, pid, the other instance's presence, and the symbolication rate |
| AC8 | 탭 일곱 개를 차례로 보이게 하면 attach 세션은 보이는 탭과 최근 네 탭의 pane에만 남고, 풀린 pane은 `released` 상태와 진단 하나를 가지며, herdr 상태가 그대로인 세션 tick은 어떤 pane도 다시 attach하지 않고, 풀린 탭을 다시 보이면 그 pane만 attach되며, 유지 캔버스 집합에서 풀린 pane이 빠진다 | machine | - |
| AC9 | throwaway 워크스페이스의 탭 여덟 개를 방문한 뒤 dev 인스턴스가 가진 terminal session 자식 프로세스는 다섯 탭 분량을 넘지 않고, 사이드바는 풀린 pane을 오류로 보이지 않으며, 풀린 탭으로 돌아오면 현재 화면이 그려진다 | judged | scripted run: create eight tabs, visit them in order, list the dev instance's child processes filtered by parent pid, capture the sidebar and the revisited tab, and record the release diagnostics |
| AC10 | view가 등록되지 않은 pane에 청크 600개를 넣으면 512개만 남고 가장 오래된 것이 버려지며, 세션이 풀리거나 pane이 사라지면 버퍼가 비고, 버린 사실이 trace에 남는다 | machine | - |
| AC11 | 두 herdr 워크스페이스가 섞인 체크아웃에서 다른 워크스페이스의 탭 사이로만 옮기는 드래그는 herdr 호출 없이 스트립에 확정되고, 같은 워크스페이스 안에서 순서를 바꾸는 드래그는 그 워크스페이스 안 인덱스의 `tab.move` 하나를 보내며 응답으로 스트립이 확정되고, 거절 응답은 스트립을 이전 순서로 되돌리며, `tab.reorder_split_workspace` 경로는 소스에 남지 않는다 | machine | - |
| AC12 | 이 저장소 경로를 공유하는 두 herdr 워크스페이스의 체크아웃에서 두 종류의 드래그 모두 자리를 잡고 다음 snapshot 뒤에도 유지된다 | judged | scripted run: capture the strip before and after each drag and once more after a later snapshot, with the tab.move diagnostics for the run |
| AC13 | AGENTS.md는 경로 드러냄 이벤트, attach 상한과 `released`, 드래그 단위 소유권, 프레임 단위 스크롤 쓰기와 그 사건을 기술한다 | judged | the AGENTS.md diff of the run read against R10 |
| AC14 | core 테스트, 셸 빌드와 테스트, herdr 계약 검사, 변경한 crate의 clippy가 모두 통과한다 | machine | - |
| AC15 | hide가 닫기를 요청한 pane의 attach가 `terminal_closed`로 끝나면 snapshot의 그 pane은 `ended`가 되지 않고 안내 청크가 붙지 않으며, 다른 이유(`spawn_failed` 등)의 종료는 지금처럼 `ended`와 이유를 가진다 | machine | - |
| AC16 | 에이전트 없는 pane을 Cmd+W로 닫을 때 pane이 사라지기까지의 어떤 프레임에도 attach 종료 안내가 없고, fork를 누르면 모달 없이 헤더 접미가 진행을 보이고 fork 표시가 붙은 형제 pane이 나타나거나 실패 이유가 알림과 stderr에 남는다 | judged | scripted run: record the pane region at frame rate through the close, and capture the header and the sibling pane after a fork, with the fork diagnostics for the run |

## 8. PRD-Level Tasks

- T1. fixture 스크립트: throwaway 워크스페이스에 한 경로를 공유하는 두 herdr 워크스페이스, 두 번째 체크아웃 디렉터리(세 단계 폴더와 텍스트 파일 포함), 체크아웃 밖 임시 폴더(텍스트 파일, 하위 폴더, 실행 비트가 켜진 스크립트), 탭 여덟 개, 경로들을 출력하고 수천 줄을 쌓는 pane, 에이전트 없는 pane 하나, fork 가능한 claude 에이전트 pane 하나를 만들고, 만든 것만 지우는 정리 경로를 갖춘다. 기존 fixture 스크립트의 패턴을 따른다. Covers SC1-SC8 reach. Depends on: none.
- T2. 링크 해석기: 심볼릭 링크를 푼 경로와 체크아웃 목록으로 소유 체크아웃을 정하고, 파일, 폴더, 기본 앱, Finder 창, Finder 드러냄의 다섯 갈래를 낸다. 갈래마다 테스트를 둔다. Covers R1, R4, AC1. Depends on: none.
- T3. 경로 드러냄: core 이벤트가 체크아웃 전환, 패널 가시성과 섹션, 트리 상태, 에디터 탭을 원자적으로 정하고, 셸은 링크 클릭을 그 이벤트 또는 macOS 핸드오프로 보내며 결과마다 trace를 남기고 거절은 상호작용 알림에 보인다. core 테스트가 AC5를 증명한다. Covers R2, R3, R4, R5, AC2-AC5. Depends on: T2.
- T4. 휠 스크롤 합치기: 라이브 herdr 0.8.2에서 `terminal.scroll`만으로 프레임이 오는지 먼저 확인하고 결과를 기록한 뒤, pane별 프레임 합치기와 크기 미상 pane의 진단을 넣고 24x80 폴백을 삭제한다. 쓰기 횟수와 최종 위치를 테스트한다. Covers R6, AC6. Depends on: none.
- T5. attach 상한: 최근 방문 탭 집합, 세션 해제, `released` 상태와 진단, tick 안정성, 재방문 attach, 캔버스 집합 연동을 core에 넣고 테스트한다. Covers R7, AC8. Depends on: none.
- T6. 셸 pane 버퍼 상한과 비우기, 셸 테스트. Covers R8, AC10. Depends on: none.
- T7. 드래그 단위 재정렬: 워크스페이스 부분수열 비교, `tab.move` 하나 또는 로컬 확정, 응답 대조, 거절 복구, 거절 경로 삭제, 테스트. Covers R9, AC11. Depends on: none.
- T8. 런타임 검증: 접미가 붙은 dev 인스턴스와 T1 fixture로 SC1-SC3과 SC6을 스크립트로 몰아 증거를 남긴다. Covers AC2, AC3, AC4, AC12. Depends on: T1, T3, T7.
- T9. attach 상한 런타임 검증: 탭 여덟 개 방문 뒤 자식 프로세스 수, 사이드바, 재방문 화면을 증거로 남긴다. Covers AC9. Depends on: T1, T5, T6.
- T10. 성능 측정: 스크립트 휠 버스트로 기준 빌드와 새 빌드를 각각 표본하고 load, pid, 다른 인스턴스, 심볼 해석률을 함께 기록한다. Covers AC7. Depends on: T1, T4.
- T11. 문서: AGENTS.md 갱신. Covers R10, AC13. Depends on: T3, T4, T5, T7.
- T13. 닫기 종료 투영: hide가 요청한 닫기와 herdr가 치운 pane의 `terminal_closed` 종료를 `ended`로 투영하지 않고 안내 청크를 붙이지 않으며, 다른 종료는 그대로 둔다. 테스트가 두 갈래를 증명한다. Covers R11, AC15. Depends on: T5.
- T14. fork 표면: fixture의 claude pane에서 fork를 재현해 pane이 나타나지 않는 원인을 기록하고 이 저장소 안이면 고친 뒤, 모달을 헤더 접미로 바꾸고 결과를 알림과 stderr 진단으로 남긴다. Covers R12, AC16. Depends on: T1.
- T12. 저장소 건강: 전체 테스트, 셸 빌드와 테스트, 계약 검사, clippy. Covers AC14. Depends on: T8, T9, T10, T11, T13, T14.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | core와 셸 빌드, clippy, herdr 계약 검사 | none |
| automated behavior | yes | 해석기 갈래, 드러냄 이벤트, 스크롤 합치기, attach 상한, 버퍼 상한, 재정렬 소유권의 회귀 | none |
| app runtime | yes | SC1-SC3, SC5, SC6을 접미가 붙은 dev 인스턴스에서 스크립트로 몰고 창을 찍은 증거 | none; 사용자는 설치 뒤 사후 판단 |
| performance measurement | yes | 휠 버스트 아래 app과 herdr server 표본, attach 자식 프로세스 수 | none |
| live herdr integration | yes, throwaway 워크스페이스 한정 | 라이브 herdr 0.8.2의 스크롤 재도색 동작, `tab.move`, 세션 해제 | none |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent |
| --- | --- | --- | --- |
| V1 | build/static | AC14 | core와 셸이 경고 없이 빌드되고 계약 검사가 herdr 0.8.2 스키마와 일치한다 |
| V2 | automated behavior | AC1, AC10 | 셸 테스트가 해석기의 다섯 갈래와 접두 일치, 버퍼 상한과 비우기를 증명한다 |
| V3 | automated behavior | AC5, AC6, AC8, AC11, AC15 | core 테스트가 드러냄 이벤트의 원자적 상태, 프레임당 쓰기 수와 최종 위치, attach 상한과 tick 안정성, 드래그 단위 소유권, 닫기 종료의 투영 제외를 증명한다 |
| V4 | app runtime | AC2, AC3, AC4, SC1, SC2, SC3 | 접미가 붙은 dev 인스턴스에서 세 갈래의 클릭이 각 카드의 primary path, failure state, recovery를 보이고 창 캡처와 trace가 이를 담는다 |
| V5 | app runtime | AC9, AC12, AC16, SC5, SC6, SC7, SC8 | 탭 여덟 개 방문 뒤 자식 프로세스가 상한 안에 있고 재방문이 그려지며, 분할 체크아웃의 두 드래그가 확정되고 유지되고, 닫기에 종료 안내가 없으며, fork가 접미와 형제 pane 또는 이유 있는 실패로 끝난다 |
| V6 | performance measurement | AC7, SC4 | 기준 빌드와 새 빌드의 휠 버스트 표본이 load와 함께 기록되고 새 빌드가 낮거나 같다 |
| V7 | live herdr integration | AC6 (T4의 사전 확인), AC12 | 라이브 서버의 스크롤 재도색 확인 결과가 기록되고 `tab.move`가 throwaway 워크스페이스에서 확정된다 |
| V8 | build/static | AC13 | AGENTS.md diff가 R10의 네 항목을 담는다 |

### 9.3 Human Verification

필수 항목 없음.
사용자는 설치된 빌드에서 경로 클릭, 스크롤, 탭 이동의 느낌을 사후에 판단하고, 4.3의 가정을 거부할 수 있다.

## 10. Risks And Open Decisions

- 실행 파일 가드(A1)가 사용자가 원하는 것보다 좁을 수 있다. 거부하면 Finder 드러냄을 기본 앱 열기로 바꾸는 한 줄 변경이다.
- herdr 0.8.2가 `terminal.scroll`만으로 프레임을 내지 않으면 same-size resize가 프레임당 하나로 남는다. 그래도 초당 65회가 최대 60회 이하로 줄고 상쇄 델타는 0회가 된다.
- attach 상한 다섯 탭(A2)이 운영자의 습관보다 작으면 자주 돌아오는 탭이 매번 첫 방문 attach가 된다. 상수 하나로 조정한다.
- 드래그 단위 대조(R9)는 herdr 응답의 워크스페이스 순서와 체크아웃의 섞인 순서를 맞추는 코드가 핵심이다. 기존 `split_checkout_payload`와 재정렬 helper로 테스트할 수 있다.
- 설치된 앱이 실행 중이므로 dev 인스턴스는 접미가 붙은 번들로 띄워야 한다. 두 인스턴스가 동시에 살아 있음을 측정 기록에 남긴다.
- 기본 앱 열기와 Finder는 사용자의 Mac에 창을 띄운다. 실행이 연 창은 실행이 닫는다.
- fork가 pane을 못 만드는 원인이 herdr 0.8.2의 `agent new` 쪽이면 이 저장소에서 고칠 수 없다. 그 경우 재현 기록과 함께 보고하고 R12의 나머지(모달 제거, 결과 진단)는 그대로 완료한다.
- fork 재현은 fixture 워크스페이스의 claude 에이전트를 실제로 시작하므로 그 pane과 fork된 pane은 fixture 정리 경로가 닫는다.

## 11. Implementation Guardrails

원칙 적용(engineering, 2026-09-04 커밋 35ab76ca 기준):

- 규칙 1: 체크아웃 단위 거절 경로, `tab.reorder_split_workspace`, 24x80 폴백, 폴더를 "unusable"로 만드는 해석기 갈래는 이 변경에서 삭제한다. 호환 계층을 남기지 않는다.
- 규칙 2, 8: 합치기는 기존 쓰기 경로 안의 순 델타 누적으로, attach 상한은 최근 방문 집합 하나로 푼다. 타이머, 새 스레드, 새 캐시 계층을 만들지 않는다.
- 규칙 4, 10: 크기 미상 pane의 휠, 등록되지 않은 체크아웃을 향한 드러냄, macOS의 열기 거절, 버퍼 폐기는 각각 진단, 오류, 알림, trace로 드러난다. 조용한 기본값은 없다.
- 규칙 5, 7: 경로 판정과 화면 상태는 core가, 파일시스템 해석과 macOS 핸드오프는 셸이 맡는다. 웹 주소를 브라우저에 넘기는 기존 경계와 같은 모양을 쓴다.
- 규칙 9: 경로 클릭의 갈래와 이유, 세션 해제와 재attach, `tab.move`의 인덱스와 응답을 기존 trace와 진단 체계에 남긴다.
- 규칙 11: 같은 경로를 두 번 클릭하면 두 번째는 이미 열린 탭과 선택을 유지하고, 풀린 pane의 해제가 두 번 와도 오류가 아니다.
- 규칙 12, 13: 테스트는 쓰기 횟수, 최종 위치, attach 집합, 스트립 순서처럼 호출자가 관측하는 답을 밖에서 정해 놓고 단언한다. 이번에 고치는 것은 체크아웃 하나의 드래그가 아니라 소유권 판정의 단위다.

원칙 적용(design):

- 규칙 3, 5: 클릭 한 번이 트리와 에디터를 함께 맞춘다. 트리의 펼침과 선택, 오른쪽 패널의 섹션 전환은 기존 Explorer 패턴을 그대로 쓴다.
- 규칙 4, 7: 풀린 pane은 사이드바에서 오류가 아니라 평상시와 같은 모양이고, 사용자가 attach 상태를 계산할 필요가 없다.
- DESIGN.md와 `HideTheme` 토큰 밖의 값을 뷰에 쓰지 않는다. 새 시각 요소는 필요 없다.

AGENTS.md 규칙:

- 런타임 뮤텍스를 서브프로세스, 블로킹 I/O, 큰 직렬화에 걸쳐 잡지 않는다. 심볼릭 링크 해석과 파일시스템 검사는 셸에서 한다.
- tick이나 이벤트 경로에서 서브프로세스를 만들지 않는다. 세션 해제는 기존 세션 drop 경로를 쓴다.
- 새 snapshot 값은 채널을 정한다. `released`는 기존 pane transport 상태 값이며 revisioned rest에 새 필드를 얹지 않는다.
- 성능 주장은 `/usr/bin/sample`로, 인스턴스가 몇 개인지 확인하고 load와 함께 기록한다.
- 증거는 `agents/runs/hide-paths-and-tab-perf/` 아래에만 쓴다. `docs/verification/`, `docs/screenshots/`에는 아무것도 넣지 않는다.
- check 바인딩은 worktree 안에 둔다.
- herdr 계약: 새 메서드나 필드를 추정하지 않고 `herdr api schema --json`과 계약 검사로 확인한다.

운영 경계:

- 실행 중인 설치 앱(운영자가 쓰는 인스턴스)을 종료하지 않는다. dev 인스턴스는 worktree 빌드의 접미 번들로 띄우고 pid로 구분한다.
- 실행이 만들지 않은 pane, 탭, 워크스페이스를 닫거나 옮기거나 포커스하지 않는다. w5D-w5H는 건드리지 않는다.
- 기본 앱과 Finder 검증은 fixture의 텍스트 파일과 폴더로 하고, 실행이 연 창은 실행이 닫는다.
- 커밋 메시지와 코드에 에이전트 이름을 넣지 않고, em dash를 쓰지 않는다.

## 12. Implementation Result Report Contract

최종 보고는 다음을 담는다.

- 사용자 결정으로 대체된 가정 목록(4.3의 A1-A11 중 실제 적용된 것)을 맨 위에.
- 갈래별 경로 클릭 동작과 그 증거 위치.
- herdr 0.8.2 스크롤 재도색 확인 결과와 resize의 존폐.
- 휠 버스트 표본 비교표(빌드, load, pid, 인스턴스 수, 심볼 해석률).
- attach 상한 검증의 자식 프로세스 수와 재방문 캡처.
- 분할 체크아웃 드래그의 확정 캡처와 `tab.move` 진단.
- 닫기 깜빡임의 제거 캡처와 fork 재현 결과(원인, 고쳤는지, herdr 쪽이면 보고).
- 삭제된 코드 경로 목록.
- 모든 AC의 판정과 V 행의 결과, 미해결 항목.
- 배포 결과: 실행 브랜치의 로컬 커밋 해시. main 병합과 설치는 Observer가 A8에 따라 뒤에 보고한다.
