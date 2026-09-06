---
topic: "Hide native responsiveness: blank new tab, transient-size garbling, local scrollback, frame coalescing, latency instrumentation, legacy removal"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "Changes the terminal attach, resize, scroll and draw paths of a single-operator desktop app and deletes code with no reader; no data, credential, billing, network or destructive effect, and every live effect is confined to fixture workspaces the run creates and closes."
source_intake: "current conversation"
created_at: "2026-09-06"
updated_at: "2026-09-06"
---

# PRD: Hide native responsiveness

## 1. Summary

2026-09-06에 설치한 3759f4d 빌드에서 운영자가 네 가지를 보고했다.
스크롤이 여전히 버벅이고, 새 탭을 열면 커서만 있는 빈 pane이 계속 보이며, 전체 반응이 아직 느리고, 기존 pane에 들어갈 때 가끔 화면이 1-2열 폭으로 접혀 깨진다.
운영자의 기준은 하나다: hide는 네이티브 터미널처럼 반응해야 하고, 그렇지 않으면 herdr TUI만 쓰는 편이 낫다.

이번 라운드는 세 결함의 원인을 증거로 확정해 고치고, 스크롤과 입력 경로에서 hide 몫의 지연과 불필요한 그리기를 빼고, 그 결과를 숫자로 보일 수 있는 측정 시설을 셸에 넣는다.
같은 라운드에 읽는 곳이 없는 코드, 핀보다 낮은 herdr 버전을 위한 호환 경로, 은퇴한 기능의 잔재를 목록으로 만들어 지운다.
Implementor는 codex `gpt-6-astra`를 xhigh 추론으로 띄우고, 구현 전에 리서치 보고서를 먼저 쓴다.

Approval checklist:

- 범위: 빈 새 탭, 과도기 크기 깨짐과 control 상실 복구, herdr TUI와 동급인 스크롤 지연, 디스플레이 주사율 단위 프레임 합치기, 지연 측정 시설, 레거시 제거 (section 3).
- 구조 변경: 휠은 herdr가 판별하는 경로를 유지하되 hide 안의 고정 대기와 중간 홉이 사라지고, 수신 프레임이 디스플레이 프레임당 한 번만 그려지며, 셸에 signpost 기반 측정 시설이 생긴다 (section 5).
- 리서치로 정하는 결정: pane당 `herdr terminal session control` 서브프로세스를 소켓 스트림으로 바꿀지는 핀된 herdr 0.8.2의 Socket API가 지원할 때만 구현한다는 가정 (4.3 A4).
- 지연 예산 숫자(디스플레이 프레임 한 개, 16.7 ms)는 가정이며 측정 시설이 만든 기준선과 함께 보고한다 (4.3 A5).
- 검증 모드: build/static, automated behavior, app runtime, performance measurement, 픽스처 워크스페이스 한정 live herdr integration (section 9.1).
- 배포 모드: local. run 브랜치 `prd/hide-native-responsiveness`에 커밋하고 멈춘다. main 병합은 Observer가 한다 (4.3 A9).
- Implementor 실행 설정: codex `gpt-6-astra`, `model_reasoning_effort=xhigh` (4.3 D5).
- `review_profile: standard`와 그 근거 (frontmatter).

## 2. Problem, Goal, And Users

사용자는 Hide의 단일 운영자다.
운영자는 하루 종일 열 개가 넘는 에이전트 pane 사이를 오가며 스크롤하고 타이핑한다.
그 사람에게 hide의 가치는 herdr 위에 얹은 화면이 herdr TUI보다 느리지 않다는 데서 시작한다.

2026-09-06 아침, 뮤텍스 아래에서 탭마다 git을 포크하던 경로를 없앤 빌드(3759f4d)를 설치한 뒤에도 운영자는 같은 불만을 보고했다.
같은 시각 설치된 앱을 5초 샘플한 결과 메인 스레드는 표본의 57%를 이벤트 대기에 썼고 그리기는 8%였다(load 3.0, 인스턴스 1개, attach된 pane 8개, 에이전트 14개).
즉 남은 문제는 CPU 포화가 아니라 경로의 지연이다.
휠 한 단계는 지금 `terminal.scroll` 서버 왕복 하나이고, 수신 프레임은 오는 대로 그려지며, 키 입력에서 에코가 그려지기까지의 구간은 아무도 재지 않는다.

새 탭이 비어 있는 결함은 지난 run이 넣은 attach 해제(`released`)와 크기 대기(`terminal.attach_deferred`)의 상호작용이 의심되지만 확정되지 않았다.
깨짐 결함의 유일한 증거(스크린샷)는 "Another client owns terminal control" 배너와 함께 찍혔고, 그 시점에는 설치 직후 두 인스턴스가 같은 pane을 두고 resize를 다퉜다.
원인이 확정되지 않은 결함을 고치는 척하지 않기 위해, 두 결함 모두 재현과 진단 기록이 수정보다 먼저다.

목표는 세 가지다.
운영자가 보고한 세 결함이 사라지고, 스크롤과 입력이 디스플레이 프레임 단위로 반응하며, 그 사실을 셸 자신의 측정으로 증명한다.

### 2.1 User Scenarios

- SC1. 새 탭 열기: 운영자가 hide에서 새 탭을 만들면 셸 프롬프트가 곧바로 보인다.
  Actors: 운영자.
  Primary path: Cmd+T로 새 탭을 만든다. pane이 나타나고 2초 안에 셸 프롬프트가 보이며 타이핑이 된다.
  Failure state: attach가 시작되지 못하면 pane 위에 그 이유(크기 대기, 해제, 서버 거절)가 보이고, 커서만 있는 빈 화면으로 남지 않는다.
  Recovery: 운영자가 아무것도 하지 않아도 크기가 보고되는 순간 attach가 시작되고, 실패는 자동 재시도 뒤 pane 위에 남는다.
  Reach: 설치된 앱 또는 dev 번들을 라이브 세션에 붙이고, run이 만든 픽스처 워크스페이스에서 탭을 다섯 번 만든다.
- SC2. 기존 pane으로 전환하고 스크롤하기: 운영자가 여러 pane이 있는 탭으로 전환하거나 줌을 바꾸면 각 pane이 자기 열 폭에 맞게 그려지고, 휠은 즉시 움직인다.
  Actors: 운영자.
  Primary path: 탭 전환 뒤 모든 pane이 정착한 크기로 한 번에 그려진다. 휠 한 단계의 지연이 같은 pane을 herdr TUI에서 스크롤할 때와 한 디스플레이 프레임 안에서 같고, Claude Code 같은 마우스 추적 TUI에서는 앱 자체 스크롤과 "jump to bottom" 클릭이 herdr TUI에서처럼 동작한다.
  Failure state: 전환 중의 과도기 크기(1-2열)가 Herdr로 가면 TUI가 그 폭으로 다시 그려 화면이 접힌다. 이번 라운드 뒤에는 그 크기가 Herdr로 가지 않는다.
  Recovery: 잘못된 폭으로 그려진 pane은 정착한 크기의 resize 하나로 즉시 복구된다.
  Reach: 픽스처 워크스페이스에 pane 4개짜리 탭 두 개를 만들고, 한쪽 pane에서 에이전트 TUI 크기의 출력을 계속 찍는 스크립트를 돌린 채 탭 전환과 줌을 반복한다.
- SC3. 출력 중인 pane에서 타이핑하기: 에이전트가 초당 10프레임 이상 그리는 pane에 운영자가 타이핑한다.
  Actors: 운영자.
  Primary path: 키 입력이 바로 다음 디스플레이 프레임에 에코되고, 수신 프레임은 프레임당 한 번만 그려진다.
  Failure state: 수신 프레임마다 그리면 타이핑 에코가 그리기 뒤로 밀린다. 보이지 않는 pane은 그리지 않는다.
  Recovery: 해당 없음. 지연은 측정 시설이 기록하고, 예산을 넘는 구간은 signpost에 남는다.
  Reach: 픽스처 pane에서 초당 10프레임 이상 전체 화면을 다시 그리는 스크립트를 돌리고, 측정 시설로 키 입력과 그리기 구간을 잰다.
- SC4. control을 잃었다가 되찾기: 다른 클라이언트가 같은 pane의 control을 잡으면 hide는 읽기 전용으로 내려가고, 그 클라이언트가 떠나면 스스로 control을 되찾는다.
  Actors: 운영자, 다른 herdr 클라이언트(두 번째 hide 인스턴스 또는 CLI).
  Primary path: 다른 클라이언트가 떠난 뒤 5초 안에 hide가 control을 되찾고 배너가 사라지며 타이핑이 된다.
  Failure state: 다른 클라이언트가 붙어 있는 동안에는 읽기 전용 배너가 남고 재시도는 제한된 간격으로만 일어난다.
  Recovery: 재시도가 계속 거절되면 배너가 그 사실과 마지막 시도 시각을 보이고, Reconnect는 여전히 동작한다.
  Reach: 픽스처 pane에 `herdr terminal session control`을 CLI로 하나 더 붙였다가 끝낸다.

## 3. Scope And Non-Goals

포함:

- hide가 만든 새 pane이 빈 화면으로 남는 결함의 원인 확정과 수정 (R1).
- Herdr로 가는 resize를 view가 정착한 크기로 한정하고, 깨짐 결함을 인스턴스 하나로 재현해 결과를 기록 (R2).
- control 상실 뒤 자동 재획득 (R3).
- 휠 스크롤의 hide 몫 지연을 없애 herdr TUI와 동급으로 만들고, 마우스 추적 TUI의 자체 스크롤과 클릭이 동작하게 함 (R4).
- 키 입력 경로에서 hide 몫의 지연을 디스플레이 프레임 하나로 제한 (R5).
- 수신 프레임을 디스플레이 주사율에 합치고, 보이지 않는 pane은 그리지 않음 (R6).
- 셸의 signpost 기반 측정 시설과 요약 스크립트, 그리고 변경 전 기준선 (R7).
- pane당 attach 서브프로세스의 소켓 대체 여부를 리서치로 결정하고 기록 (R8).
- 읽는 곳이 없는 코드, 핀 아래 herdr 버전용 호환 경로, 은퇴한 기능의 잔재를 목록으로 지움 (R9, R10).

비목표:

- 디자인 시스템 정리와 토큰 강제 검사. 이유: 별도 라운드로 제안된 상태이고 반응성과 무관하다. 다시 볼 조건: 운영자가 그 라운드를 요청할 때.
- herdr 핀 변경이나 Socket API 계약 확장. 이유: 핀은 `herdr-runtime-owned` 브랜치의 다른 세션이 다루고 있다. 다시 볼 조건: 그 브랜치가 main에 들어온 뒤 리서치가 새 API를 필요로 할 때.
- 브라우저 pane 기능의 동작 변경. 이유: 이번 라운드의 base(main caaa665)에 방금 병합된 기능이며 운영자의 보고와 무관하다. 다시 볼 조건: 측정이 브라우저 pane을 지연 원인으로 지목할 때.
- 원격(SSH) 런타임의 동작 변경. 이유: 보고된 결함은 모두 로컬 세션에서 났다. 죽은 코드 제거는 포함된다.
- 새 기능. 이유: 운영자의 방향은 "불필요한 기능은 빠져야 하고 단순하게"다.
- 로컬 스크롤백. 이유: herdr 0.8.2는 pane 안의 앱이 마우스를 추적하는지를 클라이언트에 알려주지 않아(pane.get, pane.read, 프레임 모두 동일, 리서치 보고서), 휠을 앱 보고와 로컬 버퍼 중 어디로 보낼지 hide가 정할 수 없다. 운영자 결정 D7. 다시 볼 조건: herdr가 pane의 마우스 추적 상태를 노출할 때.
- 지난 라운드의 남은 결함 중 반응성과 무관한 항목(Cmd+W 레이블, `Tab 2` 저장 레이블, 원격 read record). 이유: 메모리 목록에 있고 이번 목표와 무관하다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
자격 증명, 계정, 구매, 권한이 필요한 항목이 없다.

### 4.2 Human Decisions Before PRD Approval

None required.
모든 결정이 운영자의 대화 발언에 근거하거나 4.3의 되돌릴 수 있는 가정으로 기록되어 있다.

### 4.3 Decision Traceability For Fidelity Review

운영자 결정 (2026-09-06 대화):

- D1. "스크롤이 여튼 좀 버벅이이 있는 느낌은 여전히 있고" - 스크롤 지연은 남아 있다. R4, R6, AC5, AC7, SC2.
- D2. "새 탭열면 이렇게 계속 보이고 ... 탭 자체를 만드는 건 빠른데 뭔가 2번처럼 지금 안열리고" - 새 탭이 빈 pane으로 남는다. R1, AC1, SC1.
- D3. "여전히 느린 느낌이 더있어 ... 여튼 네이티브하게 반응해야돼. 이거 메모리나 성능이나 아직도 아쉬워서" - 목표는 네이티브 반응과 메모리. R5, R6, R7, R8, AC6, AC11.
- D4. "가끔씩 깨질 때가 있는데 해결된건지.. 아니면 잔존할지 궁금하네. 이미 있는 pane 들어갔을 때 가끔 그래" - 깨짐의 잔존 여부를 알고 싶다. R2, R3, AC2, AC3, AC4, SC2, SC4.
- D5. "불필요한 옛날 코드나 레거시들이 있는것들이 있다면 싹 정리해서 잘 정리해봐" - 레거시 제거. R9, R10, AC9.
- D6. "implementor codex astra 로 띄워서 더 고민하게 해서 리서치 쭉 시키고 작업 진행시켜" - Implementor는 codex `gpt-6-astra`(`~/.codex/config.toml`의 기본 모델)를 xhigh 추론으로 띄우고, 리서치를 먼저 한다. T2, 11장.
- D8. herdr 0.8.2가 포인터 입력 계약과 마우스 추적 상태를 노출하지 않는다는 리서치 결과에 대한 운영자 결정 "에이전트 종류로 판별 (Recommended)" - herdr가 에이전트를 claude로 감지한 pane에서만 클릭을 마우스 보고로 보내고, 다른 pane은 로컬 선택을 유지한다. 거부된 대안: Option+클릭으로만 전달, 클릭 보류. R4, AC5, T5, A3.
- D7. 운영자 힌트 "claude codex 각각 세션 리줌하고 스크롤하는게 나을듯? 코덱스는 괜찮은것같은데 클로드코드가 졸라 느려.. jump to bottom이라고 원래 클릭하면 밑으로 다시 내려가는 기능이있는데 그거도 클릭이 안되고 확인해봐"와, herdr가 마우스 모드를 노출하지 않는다는 리서치 결과에 대한 결정 "herdr TUI와 동급 지연" - 휠은 herdr가 앱 마우스 보고와 스크롤백을 판별하는 경로를 유지하고, hide 몫의 지연을 없애 같은 pane의 herdr TUI와 동급으로 만든다. Claude Code의 "jump to bottom" 클릭이 동작해야 한다. R4, AC5, SC2, T5, 비목표.

에이전트 가정 (되돌릴 수 있음, 운영자가 거부 가능):

- A1. 새 탭 결함의 유력한 원인은 attach 해제 상태와 크기 대기의 상호작용이며, 수정 전에 dev 번들의 stderr 진단으로 확정한다. 확정되지 않으면 T2 보고서가 그 사실을 남기고 다른 원인을 찾는다. R1, T2, T3.
- A2. 깨짐 스크린샷의 원인은 두 인스턴스가 resize를 다툰 것으로 추정한다. 인스턴스 하나로 재현되지 않으면 "재현 불가, 추정 원인 두 인스턴스"로 기록하고, 과도기 크기 차단(R2)과 자동 재획득(R3)은 그와 별개로 구현한다. AC3, T4.
- A3. (D7, D8로 대체됨) 스크롤 기제는 herdr의 `terminal.scroll` 경로다. hide 몫의 지연 중 없앨 것은 고정 16 ms 대기(첫 휠은 즉시 보내고 응답 대기 중의 휠만 합친다), 수신 프레임의 지연 표시, 그리기 비용이다. 클릭은 herdr의 pane 에이전트 감지가 claude인 pane에서만 SGR 마우스 보고로 재생된다. Claude Code가 추적을 꺼 둔 드문 경우 입력창에 이스케이프 문자가 보일 수 있으나 Enter는 가지 않는다. 판별 근거는 herdr의 `agent.list` 또는 pane 상태의 agent 필드다. R4, T5.
- A4. pane당 `herdr terminal session control` 서브프로세스를 소켓 스트림으로 바꾸는 일은 핀된 0.8.2의 공식 Socket API가 터미널 스트림을 지원한다는 문서 증거가 있을 때만 구현한다. 없으면 근거와 함께 연기한다. R8, T7.
- A5. 지연 예산은 디스플레이 프레임 하나(16.7 ms, 60 Hz 기준; 실제 주사율을 측정 시설이 기록)다. 휠 한 단계에서 그리기까지, 키 입력에서 전송까지, 수신에서 그리기까지 각각 이 예산 안이어야 한다. 예산 밖의 숫자는 실패가 아니라 보고 대상이며 운영자가 예산을 바꿀 수 있다. R5, R6, AC5, AC6, AC7.
- A6. 측정은 dev 번들을 라이브 세션(운영자의 실제 워크스페이스, attach된 pane 8개 이상, 에이전트 10개 이상)에 붙여 인스턴스 하나로 한다. 기준선은 코드 변경 전 같은 조건에서 잡는다. R7, T1, T9.
- A7. run이 만든 픽스처 워크스페이스, 탭, pane만 닫는다. 운영자의 워크스페이스와 지난 라운드의 픽스처(w5D-w5H)는 건드리지 않는다. 11장.
- A8. control 재획득은 5초 간격 재시도로 시작해 30초까지 두 배씩 늘리고, 성공하면 배너가 사라진다. R3, AC4.
- A9. 배포 모드는 local이다. run 브랜치에 커밋하고 멈춘다. main은 다른 세션의 worktree에 체크아웃되어 있어 병합은 Observer가 나중에 한다. 12장.
- A10. 레거시 판정 기준은 네 가지다: 읽는 곳이 없는 스냅샷 필드나 함수, 핀보다 낮은 herdr 버전을 위한 호환 경로, 은퇴한 기능의 잔재, 같은 일을 하는 두 번째 구현. 각 항목은 검색 결과나 호출 그래프를 근거로 남긴다. 첫 항목은 메모리에 기록된 `pane_read_records`와 `state_change_seq`(revisioned rest에 실리지만 셸이 읽지 않음)다. R9, AC9.

거부되거나 미룬 선택:

- 성능 표본 없이 기능을 더 얹는 것. 운영자: "불필요한 기능은 빠져야하고 단순하게 만들어져야". 비목표.
- 지난 run의 AC7(새 빌드 휠 표본)을 다시 여는 것. 이번 라운드는 자기 측정 시설로 새 기준선을 잡는다.

## 5. Major Technical Structure Changes

- 스크롤 경로: 휠 이벤트가 core의 `terminal_scroll` 이벤트와 Herdr `terminal.scroll` 요청을 거치는 구조는 유지된다. 그 안의 고정 16 ms 합치기 대기가 응답 대기 중에만 합치는 구조로 바뀌고, 응답 프레임은 디스플레이 링크에서 곧바로 표시된다.
- 그리기 경로: 수신 터미널 청크가 도착 즉시 그려지던 구조에서, 디스플레이 링크 단위로 합쳐 pane당 프레임당 한 번 그리는 구조로 바뀐다. 보이지 않는 pane은 파싱만 하고 그리지 않는다.
- 크기 경로: view의 모든 크기 보고가 Herdr로 가던 구조에서, 정착한 크기만 가는 구조로 바뀐다.
- 측정 시설: 셸에 os_signpost 구간(키 입력에서 전송, 수신에서 그리기, 휠에서 그리기, 탭 전환에서 첫 그리기)과 그것을 요약하는 스크립트가 생긴다.
- attach 전송(조건부): 리서치가 지지할 때만 pane당 CLI 서브프로세스가 core의 소켓 스트림으로 바뀐다.

## 6. Requirements

- R1. hide가 만든 새 pane은 view가 크기를 보고한 뒤 attach가 시작되고 셸 프롬프트가 보인다. 크기 대기, 해제, 서버 거절 같은 대기 상태는 진단으로 남고 pane 위에 보이며, 어떤 상태도 영구 대기가 되지 않는다.
- R2. hide가 Herdr로 보내는 resize는 view가 정착한 크기뿐이다. 탭 전환, 줌, 분할 중의 과도기 크기는 전송되지 않고, 정착은 짧은 창(디스플레이 프레임 몇 개) 안에 마지막 크기로 결정된다.
- R3. 다른 클라이언트가 control을 잡아 hide가 읽기 전용이 되면, hide는 제한된 간격으로 control을 다시 요청하고 소유자가 떠나면 스스로 되찾는다. 재시도와 결과는 진단으로 남는다.
- R4. 휠 한 단계의 지연은 같은 pane을 herdr TUI에서 스크롤할 때와 한 디스플레이 프레임 안에서 같다. 휠은 herdr가 앱 마우스 보고와 스크롤백을 판별하는 경로로 가되, hide 안의 고정 대기는 없고 첫 휠은 즉시 전송되며 응답 대기 중의 휠만 합쳐진다. herdr가 에이전트를 claude로 감지한 pane에서는 일반 클릭이 앱에 SGR 마우스 보고로 전달되어 앱 자체 스크롤과 "jump to bottom" 클릭이 herdr TUI에서처럼 동작한다. 다른 pane의 클릭은 지금처럼 로컬 선택이다. 지연 비교는 마우스 추적 TUI와 일반 셸 두 종류의 pane에서 한다.
- R5. 키 입력에서 바이트 전송까지, 그리고 에코 수신에서 그리기까지 hide 몫의 지연은 각각 디스플레이 프레임 하나 안이다.
- R6. 수신 프레임은 pane당 디스플레이 프레임당 한 번만 그려지고, 보이지 않는 pane은 그리지 않는다.
- R7. 셸은 R4-R6의 구간을 signpost로 기록하고, 저장소의 스크립트 하나가 그 기록을 구간별 p50/p95/최대와 표본 수로 요약한다. 변경 전 기준선과 변경 후 숫자를 같은 조건에서 잰다. AGENTS.md 성능 안내가 그 측정 방법을 담는다.
- R8. pane당 attach 서브프로세스를 소켓 스트림으로 바꿀지는 핀된 herdr의 공식 CLI 참조와 Socket API 참조를 전부 읽은 뒤 결정하고, 결정과 근거를 보고서에 남긴다. 구현하면 CLI attach 경로는 지운다.
- R9. 레거시 목록: A10의 기준에 맞는 항목을 core와 셸 전체에서 찾아 근거와 함께 목록으로 만들고, 같은 변경에서 지운다. 호환 계층이나 죽은 경로를 남기지 않는다.
- R10. 기존 테스트는 지워진 코드의 테스트를 제외하고 모두 통과하고, 지워진 테스트는 목록에 항목별로 적힌다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | hide에서 만든 새 탭 다섯 개가 모두 2초 안에 셸 프롬프트를 보이고, 빈 pane으로 남는 것이 없다 | judged | scripted run: 픽스처 워크스페이스에서 새 탭 5회 생성, 각 생성 뒤 2초 시점 스크린샷과 stderr 진단 발췌(attach 시작과 종료 기록) |
| AC2 | 탭 전환, 줌, 분할 중 view가 거친 과도기 크기는 Herdr로 가지 않고, 정착한 크기 하나만 간다 | machine | - |
| AC3 | pane 4개짜리 탭 두 개를 스트리밍 출력 중에 20회 오가고 줌을 10회 바꾸는 동안, 어떤 pane도 자기 열 폭이 아닌 폭으로 그려지지 않는다 | judged | scripted run: 전환 뒤 스크린샷 표본 6장 이상과 core가 전송한 resize 목록, 그리고 원인 확정 또는 재현 불가 판정이 담긴 보고서 |
| AC4 | 다른 클라이언트가 control을 놓은 뒤 5초 안에 hide가 control을 되찾고, 배너가 사라지며, 타이핑이 pane에 도달한다 | judged | scripted run: 픽스처 pane에 CLI control 세션을 붙였다 끝내는 절차, 배너 전후 스크린샷, 재획득 진단 발췌 |
| AC5 | 마우스 추적 TUI(Claude Code)와 일반 셸 두 pane 각각에서, 휠 한 단계부터 내용이 움직인 그리기까지의 지연 p95가 같은 pane을 herdr TUI에서 스크롤한 지연 p95보다 한 디스플레이 프레임(16.7 ms) 넘게 크지 않고, hide 안의 고정 대기가 0 ms이며, herdr가 claude로 감지한 pane에서 Claude Code의 "jump to bottom" 클릭이 동작하고 셸 pane의 클릭은 로컬 선택으로 남는다 | judged | measurement: 두 pane 종류의 기준선, 변경 후, herdr TUI의 지연 요약(각 표본 100 이상, p50/p95/최대)과 측정 방법, 그리고 클릭 전후 스크린샷 |
| AC6 | 초당 10프레임 이상 출력 중인 pane에서 키 입력부터 바이트 전송까지, 에코 수신부터 그리기까지가 각각 p95 16.7 ms 이내다 | judged | measurement: 기준선과 변경 후의 signpost 요약(각 구간 표본 100 이상)과 측정 조건(load, pane 수, 에이전트 수) |
| AC7 | 초당 10프레임 이상 출력 중인 pane은 디스플레이 프레임당 최대 한 번 그려지고, 보이지 않는 pane의 그리기 횟수는 0이다 | machine | - |
| AC8 | 저장소의 스크립트 하나가 signpost 기록에서 R7의 네 구간을 구간별 p50/p95/최대와 표본 수로 요약해 출력한다 | machine | - |
| AC9 | 레거시 목록의 모든 항목이 A10의 기준 하나와 근거(검색 결과 또는 호출 그래프)를 갖고, 지워진 뒤 호환 계층이나 죽은 경로가 남지 않는다 | judged | 목록 문서와 diff, 각 항목의 근거 검색 명령과 결과 |
| AC10 | core 테스트, 셸 테스트, 계약 검사가 모두 통과한다 | machine | - |
| AC11 | dev 번들을 라이브 세션에 10분 붙인 뒤 RSS가 기준선 대비 늘지 않고, pane당 프로세스 수 결정이 보고서에 근거와 함께 적혀 있다 | judged | measurement: 기준선과 변경 후의 RSS 표본(1분 간격), 프로세스 목록, 보고서 발췌 |
| AC12 | 리서치 보고서가 새 탭 원인, 깨짐 원인, 스크롤 기제, attach 전송의 네 결정을 herdr 공식 참조와 코드 위치를 인용해 담고 있다 | judged | 보고서 본문과 인용 목록 |

## 8. PRD-Level Tasks

- T1. 셸 signpost 측정 시설과 요약 스크립트를 만들고, 코드 변경 전 설치된 앱과 dev 번들에서 기준선을 잡는다. Covers R7, AC8. Depends on: none.
- T2. 리서치 보고서: herdr 0.8.2의 CLI 참조와 Socket API 참조를 전부 읽고, 새 탭 결함과 깨짐 결함을 dev 번들의 stderr 진단으로 재현하며, 스크롤 기제와 attach 전송을 결정한다. Covers R1, R2, R4, R8, AC12. Depends on: none.
- T3. 새 pane의 attach 시작 결함을 고친다. Covers R1, AC1. Depends on: T2.
- T4. resize를 정착한 크기로 한정하고 control 자동 재획득을 넣는다. Covers R2, R3, AC2, AC3, AC4. Depends on: T2.
- T5. 휠 경로에서 hide의 고정 대기와 지연 표시를 없애고, 클릭 재생이 앱 마우스 보고로 도달하게 하며, herdr TUI와 같은 pane에서 지연을 비교 측정한다. Covers R4, AC5. Depends on: T2.
- T6. 수신 프레임을 디스플레이 링크에 합치고 보이지 않는 pane의 그리기를 없애며, 키 입력 경로의 hide 몫을 프레임 하나로 만든다. Covers R5, R6, AC6, AC7. Depends on: T1.
- T7. attach 전송 결정을 구현하거나 근거와 함께 연기한다. Covers R8, AC11. Depends on: T2, T5.
- T8. 레거시 목록을 만들고 지운다. Covers R9, R10, AC9, AC10. Depends on: T3, T4, T5, T6, T7.
- T9. 변경 후 측정을 기준선과 같은 조건에서 잡고, AGENTS.md 성능 안내에 측정 방법과 이번 숫자를 적는다. Covers R7, AC5, AC6, AC11. Depends on: T8.
- T10. dev 번들을 조립하고 SC1-SC4의 절차를 픽스처 워크스페이스에서 돌려 증거를 등록한다. Covers AC1, AC3, AC4. Depends on: T9.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | core, 셸, 계약 검사 | none |
| automated behavior | yes | 크기 정착, 프레임 합치기, 휠 즉시 전송과 합치기, 재획득 재시도의 단위 동작 | none |
| app runtime | yes | SC1-SC4를 dev 번들로 픽스처 워크스페이스에서 | none |
| performance measurement | yes | R4-R6의 구간 지연과 RSS, 기준선 대비 | 예산 숫자의 최종 수용 |
| live herdr integration | yes | 픽스처 워크스페이스 한정의 실제 attach, control 다툼, 이력 수신 | none |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent |
| --- | --- | --- | --- |
| V1 | automated behavior | AC2, AC7 | 셸과 core 테스트가 과도기 크기 시퀀스의 단일 resize 정착, 스트리밍 청크의 프레임당 한 번 그리기, 첫 휠의 즉시 전송과 응답 대기 중 합치기, 규칙대로 늘어나는 재획득 재시도 간격을 호출자가 관찰하는 결과(전송된 resize 목록, 그리기 횟수, 전송된 scroll 요청 목록과 시각, 재시도 시각)로 단언한다 |
| V2 | app runtime | AC1, AC3, AC4, SC1, SC2, SC4 | dev 번들이 픽스처 워크스페이스에서 새 탭 5회, 전환 20회와 줌 10회, control 다툼을 돌리고, 스크린샷과 진단 발췌가 각 카드의 primary path, failure state, recovery를 보이며, 깨짐은 원인 확정 또는 재현 불가로 판정된다 |
| V3 | performance measurement | AC5, AC6, AC11, SC3 | T1의 기준선과 T9의 변경 후 숫자가 같은 조건(라이브 세션, 인스턴스 하나, load 기록)에서 비교되고, 요약 스크립트 출력이 예산 안이거나 예산 밖이면 그 숫자와 조건이 보고서에 그대로 적힌다 |
| V4 | live herdr integration | AC9, AC12 | 리서치 보고서의 네 결정과 레거시 목록의 각 항목이 인용된 문서 절과 코드 위치를 갖고, 목록의 근거 검색을 다시 돌리면 같은 결과가 나오며, 이력 수신과 control 다툼은 픽스처 워크스페이스의 실제 herdr에서 확인된다 |
| V5 | build/static | AC8 | 저장소의 스크립트 하나가 signpost 기록을 읽어 R7의 네 구간 각각의 p50/p95/최대와 표본 수를 낸다 |
| V6 | build/static | AC10 | `agents/config.json`의 test, build, typecheck, lint 명령이 모두 종료 코드 0으로 끝난다 |

### 9.3 Human Verification

- 운영자가 설치된 빌드에서 하루 사용한 뒤 "네이티브하게 반응한다"고 판단한다. 이 판단은 done 조건이 아니라 다음 라운드의 입력이다.
- 예산 숫자(A5)의 최종 수용.

## 10. Risks And Open Decisions

- 클릭 전달이 에이전트 감지에 기대므로, Claude Code가 마우스 추적을 끈 상태이거나 감지가 늦은 pane에서는 클릭이 잘못 가거나 늦게 간다. 완화: 보고 바이트에 Enter를 포함하지 않고, 감지 전에는 로컬 선택으로 두며, 진단에 판별 근거를 남긴다. 다시 볼 조건: herdr가 마우스 추적 상태를 노출할 때.
- 휠 경로가 herdr 왕복을 유지하므로 지연의 하한은 herdr 서버와 앱(Claude Code의 전체 화면 재그리기)이 정한다. 완화: 목표를 herdr TUI 동급으로 두고, 같은 pane에서 TUI의 지연을 같은 방법으로 재어 비교한다. 그 하한 자체가 느리면 보고서가 herdr 쪽 원인으로 기록한다.
- 프레임 합치기는 한 프레임 안의 청크를 모아 파싱은 즉시, 그리기만 미룬다. 파싱을 미루면 커서 위치와 IME 오버레이가 어긋난다.
- 새 탭 결함이 진단으로 재현되지 않으면 T3는 추정 수정이 된다. 완화: 재현되지 않으면 OBSERVER_BLOCK으로 올리고 Observer가 운영자의 실제 세션에서 stderr를 잡을 방법을 정한다.
- 레거시 제거가 `herdr-runtime-owned` 브랜치와 충돌할 수 있다. 완화: 그 브랜치가 다루는 파일(herdr 핀, `scripts/bump-herdr.sh`, `herdr-core/src/version.rs`, 계약 스키마)은 목록에서 제외하고 보고서에 그 이유를 적는다.
- 측정 시설 자체가 지연을 더할 수 있다. 완화: signpost는 릴리스 빌드에서도 비용이 낮은 os_signpost를 쓰고, 요약 스크립트는 프로세스 밖에서 돈다.
- codex Implementor는 이 저장소에서 처음이다. 완화: 핸드오프가 대화형 질문 도구 금지, OBSERVER_BLOCK 형식, herdr 문서 정독을 명시한다.

## 11. Implementation Guardrails

- Implementor는 codex `gpt-6-astra`, `model_reasoning_effort=xhigh`로 실행되고, 구현 전에 T2 보고서를 `agents/runs/hide-native-responsiveness/evidence/research.md`에 쓴다.
- 코드를 바꾸기 전에 herdr 0.8.2의 CLI 참조와 Socket API 참조를 전부 읽고, 메서드 이름과 응답 필드는 `herdr api schema --json`과 `contracts/herdr-api.schema.json`으로 확인한다.
- 뮤텍스 아래에서 서브프로세스, 블로킹 I/O, 큰 직렬화를 하지 않는다. 스냅샷 필드를 더하면 채널(rest, top-level, cursor)을 정한다. AGENTS.md 성능 안내의 규칙이 그대로 적용된다.
- run이 만든 픽스처 워크스페이스, 탭, pane만 닫는다. 운영자의 워크스페이스와 w5D-w5H는 건드리지 않는다. 운영자의 앱 인스턴스를 죽이지 않는다. 측정 중에는 인스턴스가 하나뿐임을 먼저 확인한다.
- 증거는 `agents/runs/hide-native-responsiveness/` 아래에만 쓴다. `docs/verification/`, `docs/screenshots/`에는 아무것도 넣지 않는다.
- 지우는 코드는 같은 커밋에서 그 테스트와 문서 참조까지 지운다. 호환 계층, 기능 플래그, "나중에 지울" 경로를 남기지 않는다 (engineering principle 1).
- `herdr-runtime-owned` 브랜치가 다루는 파일은 건드리지 않는다.
- 대화형 질문 도구를 쓰지 않는다. 막히면 OBSERVER_BLOCK을 마지막 텍스트로 내고 턴을 끝낸다.
- 커밋 메시지와 문서에 에이전트나 모델 이름을 넣지 않는다.
- run 브랜치에 커밋하고 멈춘다. main 병합, 설치, 앱 재실행은 하지 않는다.

## 12. Implementation Result Report Contract

- 상태와 run 브랜치의 커밋 목록.
- 세 결함 각각의 원인, 근거, 수정 또는 재현 불가 판정.
- 리서치 보고서의 네 결정과 인용.
- 기준선과 변경 후의 측정 표: 구간별 p50/p95/최대, 표본 수, load, pane 수, 에이전트 수, 디스플레이 주사율, RSS.
- 레거시 목록: 항목, 기준, 근거, 지운 줄 수, 지운 테스트.
- 가정 A1-A10 중 실제로 적용한 것과 바뀐 것.
- 검증 판정과 소요 시간.
- 미해결 항목과 그 이유.
