---
topic: "herdr-ide: herdr 소켓 API 위의 네이티브 개발 셸"
status: "draft"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "검증이 실제 herdr 서버를 상대로 workspace.close와 worktree.remove를 실행해 진행 중인 에이전트 세션과 Git 체크아웃을 실제로 지우고, 앱이 브라우저 로그인 세션을 보관하면서 같은 workspace의 모든 에이전트가 서로의 탭을 제어할 수 있는 loopback CDP 엔드포인트를 연다."
source_intake: "agents/interview/herdr-ide-native-shell/qa-log.md"
created_at: "2026-08-26"
updated_at: "2026-08-26"
---

# PRD: herdr-ide: herdr 소켓 API 위의 네이티브 개발 셸

## 1. Summary

herdr 소켓 API 위에 Electron + TypeScript 네이티브 셸을 새로 만든다.
herdr TUI를 실행하지 않고 셸 전체(사이드바, 터미널, 파일 워크벤치, 브라우저 패널, 데스크톱 펫)를 네이티브로 구성한다.

해결하려는 불편은 herdrm 실사용에서 나왔다.
키맵이 안 먹었고, 파일 브라우저도 웹 브라우저도 없어 결국 다른 앱으로 나가야 했고, 사이드바를 원하는 대로 꾸밀 수 없었다.
herdrm은 herdr API 103개 중 18개만 쓰고 앱 고정 단축키 5개가 전부였다.

이 PRD는 v1 전체를 다루며 `agents/prd/herdr-lightweight-ide/prd.md`를 **폐기하고 대체한다**.
그 PRD의 Swift, libghostty, herdr TUI 통째 실행 결정이 전부 무효가 되었고, 8일간 소스 0줄로 막고 있던 차단 요인(Xcode 전체 설치)도 함께 사라졌다.

### Approval checklist

승인 전에 다음을 확인해 주세요.

- **이전 PRD 폐기** - `agents/prd/herdr-lightweight-ide/prd.md`는 여전히 `human_approval: approved`다. 이 PRD 승인이 곧 그 PRD의 폐기 승인이다. (3장, 4.2장 HD1)
- **스코프와 비목표 경계** - 브라우저 패널 + grab, 파일트리 F2(가벼운 인라인 편집까지), 데스크톱 펫 흡수, 원격 herdr 지원이 v1에 들어간다. 쿠키 임포트, diff 코멘트 배치, 파일 조작(생성·삭제·이동), 원격 편집, 원격 브라우저, Windows, 서명·공증은 명시적 비목표다. (3장)
- **기술 구조** - Electron + TypeScript, herdr-core(Rust) 재사용 없이 TS 재구현, pet-app 은퇴. 세 번째 스택 번복이다. (5장, 4.2장 HD2)
- **메모리 예산이 인수 조건** - workspace 7 / pane 11 기준으로 브라우저 닫힘 400MB, 열림 900MB를 넘으면 v1 미완료다. orca를 무게 때문에 지운 실증에서 나온 숫자다. (7장 AC17, 9.2장 V8)
- **수용한 위험 두 건** - 같은 workspace의 에이전트가 서로의 브라우저 탭을 제어할 수 있고, 워크벤치 편집 충돌 시 어느 쪽이든 한쪽 변경은 잃는다. (10장 RISK3, RISK4)
- **검증이 진짜를 건드린다** - 완료 판정의 주 모드가 실제 herdr 서버를 chromux로 모는 것이고, 파괴적 동작 검증은 `herdr-ide-verify-` 접두어 fixture에서만 돌린다. 이 가드가 v1 태스크다. (9장, 4.2장 HD5)
- **공개 저장소 발행** - v1 배포는 소스 공개 + 각자 빌드다. 공개 시점과 저장소 이름은 사용자 결정이다. (4.2장 HD6)
- **시각 밀도가 완료 게이트** - AC19를 자동으로 판정할 수 있는 주체가 없어 HV2(사람 판정)가 v1 완료를 막는 게이트가 된다. (4.2장 HD8, 9.3장 HV2)
- **delivery mode: local** - PR 자동화, CI, 워크트리 실행은 쓰지 않는다. (11장)

## 2. Problem, Goal, And Users

### 사용자

herdr로 코딩 에이전트를 동시에 여러 개 굴리는 개발자.
현재 실사용 규모는 workspace 7 / tab 9 / pane 11 / agent 9이고, 로컬 맥과 원격 mini 두 대의 herdr 서버를 쓴다.
`~/.config/herdr/config.toml`에 커스텀 키바인딩과 사이드바 레이아웃을 이미 작성해 두었고 플러그인 3개(`herdr-file-viewer`, `herdr-agent-context-labels`, `official.browser`)를 운용 중이다.
v1은 이 한 사람을 위해 만들지만 남들도 쓰는 것을 전제로 설계한다.

### 문제

에이전트가 멈추거나 산출물을 만들 때마다 herdr 밖으로 나가야 한다.
산출물을 보려면 다른 에디터를, 에이전트가 만든 화면을 보려면 다른 브라우저 창을 연다.
브라우저에서 본 것을 에이전트에게 넘기려면 다시 타이핑한다.
창을 전환하고 본 것을 다시 입력하는 이 두 동작이 집중을 끊는다.

기존 대안 두 개가 각각 실패했다.
herdrm은 herdr API를 거의 안 써서 앱 안에서 할 수 있는 일이 없었다.
orca는 경험이 나았지만 무거워서 지웠다.

### 목표

여러 에이전트를 굴리는 동안 터미널, 산출물, 브라우저를 한 화면에서 다루게 해서 창 전환과 재입력을 없앤다.
동시에 herdr가 이미 소유한 계약(키맵 개념, 사이드바 토큰, 에이전트 매니페스트, 플러그인)을 herdr-ide 안에서 잃지 않는다.

### 성공의 모습

하루 작업을 herdr-ide에서 시작해 끝낼 수 있고, herdrm이 실패한 세 가지가 뒤집힌다.
앱 안에서 키맵으로 조작되고, 파일트리와 브라우저가 붙어 있고, 사이드바가 사용자의 herdr 설정과 label 플러그인 요약을 그대로 보여준다.
그리고 orca를 지우게 만든 무게가 재현되지 않는다.

### 2.1 User Scenarios

- SC1. 막힌 에이전트를 찾아 붙는다.
  Actors: 개발자(herdr-ide 사용자).
  Primary path: 사이드바 agents 목록에서 미확인 attention 표시를 보고 클릭하면 중앙 터미널이 그 pane의 PTY로 바뀌고, 답하면 표시가 사라진다.
  Failure state: herdr 서버가 없으면 '소켓 파일 없음'과 '파일은 있으나 응답 없음'을 구분해 표시하고 자동 기동하지 않는다. 사용자가 이미 확인한 `?`는 attention이 아니라 idle로 내려간다.
  Recovery: 화면의 '서버 실행' 버튼으로 사용자가 명시적으로 띄우고, 기동 후 사이드바가 스냅샷을 다시 읽는다.
  Reach: 에이전트가 붙은 pane이 최소 2개 있고 그중 하나가 미확인 질문 상태인 herdr 세션이 필요하다. 그 상태를 만드는 fixture 준비가 태스크다.

- SC2. 에이전트가 만든 산출물을 확인한다.
  Actors: 개발자.
  Primary path: 우측 패널을 워크벤치로 두고 파일트리에서 파일을 클릭하면 이미지가 렌더되고 마크다운·코드·diff도 같은 자리에서 보이며, 값 하나는 그 자리에서 고쳐 저장한다.
  Failure state: 포커스된 pane이 트리 루트와 다른 경로에 있으면 트리 상단이 그 사실을 알린다(자동 추종하지 않는다). 편집 중 에이전트가 같은 파일을 덮어쓰면 '내 편집 유지 / 다시 읽기'를 묻는다.
  Recovery: 명시적 조작으로만 pane 경로로 트리를 옮긴다. 충돌은 사용자가 무엇을 잃을지 고른다.
  Reach: 이미지·마크다운·소스 파일이 있고 Git diff가 존재하며 pane 하나가 하위 경로로 `cd`한 workspace가 필요하다.

- SC3. 본 것을 에이전트에게 그대로 넘긴다.
  Actors: 개발자, 그리고 포커스된 pane에서 실행 중인 코딩 에이전트.
  Primary path: 우측 패널을 브라우저로 전환하면 탭에 어느 pane이 어느 프로필로 열었는지 배지가 보이고, grab 모드에서 요소를 고른 뒤 Intent(Change/Question)와 코멘트를 붙여 Add하면 HTML·CSS와 크롭 스크린샷 경로가 포커스된 에이전트 pane에 주입된다.
  Failure state: 포커스된 pane이 없거나 그 pane에 에이전트가 실행 중이 아니면 grab 모드 진입 자체를 막고 사유를 표시한다(조용한 no-op 금지). chromux가 없으면 패널은 뜨되 에이전트 연동만 degrade하고 사유를 표시한다. 다른 에이전트가 새 페이지를 열어도 보고 있던 탭을 자동 전환하지 않고 미확인 배지만 띄운다. 패널 브라우저는 herdr-ide 소유 파티션이라 처음에는 로그인이 안 되어 있다.
  Recovery: 로그인은 패널에서 한 번 하면 유지된다. 기존 로그인이 필요한 작업은 chromux의 별도 Chrome 창에서 계속한다. 대상이 없으면 안내대로 에이전트가 실행 중인 pane을 먼저 선택한다.
  Reach: 로컬 HTTP로 서빙되는 페이지 하나와, 에이전트가 실행 중인 pane 하나, 그리고 에이전트가 없는 맨 셸 pane 하나가 필요하다.

- SC4. 원격(mini) workspace에서 작업한다.
  Actors: 개발자, mini의 herdr 서버.
  Primary path: 사이드바에 원격 workspace가 함께 보이고, 선택하면 터미널이 원격 pane에 attach되며 분할도 동작하고, 파일트리로 원격 파일을 탐색해 뷰어로 본다.
  Failure state: 인라인 편집과 브라우저 패널은 원격에서 동작하지 않으며 회색으로 비워두지 않고 이유를 표시한다. SSH 연결이 끊기면 그 사실을 알리고 조용히 빈 목록을 보여주지 않는다.
  Recovery: 원격 파일 수정은 그 pane의 터미널에서 한다. 연결 끊김은 재연결을 시도하고 결과를 알린다.
  Reach: mini가 켜져 있고 SSH ControlMaster로 붙을 수 있어야 하며 그쪽에 herdr 세션이 살아 있어야 한다. 머신 가용성에 따라 차단 가능한 시나리오다.

- SC5. 창이 뒤로 갔을 때 알림을 받는다.
  Actors: 개발자.
  Primary path: herdr-ide가 아닌 앱을 보고 있을 때 데스크톱의 펫이 상태를 드러내고(에러·질문·승인 우선), 클릭하면 herdr-ide가 앞으로 나오며 해당 에이전트로 이동한다.
  Failure state: herdr-ide 창이 앞에 있을 때는 펫이 조용히 있고 사이드바가 알린다. 같은 사실을 두 곳에서 동시에 말하지 않는다. 저장된 펫 위치가 연결되지 않은 모니터를 가리키면 주 디스플레이로 복구한다.
  Recovery: 오프스크린 좌표는 클램프로 복구하고, 전역 단축키로 펫과 사이드바를 숨기고 다시 띄운다.
  Reach: attention 상태 에이전트가 있는 herdr 세션과, herdr-ide 창의 포커스를 뺏을 다른 앱이 필요하다. 오프스크린 복구는 실제 사고 좌표 `[542720, 163840]`를 입력으로 쓴다.

- SC6. workspace·worktree·pane을 앱 안에서 만들고 닫는다.
  Actors: 개발자.
  Primary path: 사이드바에서 새 workspace 또는 worktree를 만들고 라벨을 주고, 그 안에서 새 pane을 열어 에이전트를 띄우고, 끝나면 닫는다. CLI로 나가지 않는다.
  Failure state: working이나 attention 상태 pane을 닫으려 하면 확인 다이얼로그가 그 프로세스에 무슨 일이 생기는지 문장으로 알린다(idle이면 바로 닫는다). workspace나 tab을 닫을 때는 개별 확인 대신 집계 경고 하나가 종료될 에이전트를 `tokens.summary`와 함께 나열한다. worktree 제거는 체크아웃을 실제로 지우므로 결과를 고지한다. Git work tree가 아닌 곳에서 worktree 생성을 시도하면 herdr의 `not_git_worktree` 사유를 그대로 보여준다. herdr protocol이 요구 버전에 미달하면 부분 비활성화 없이 명확한 메시지로 실패한다.
  Recovery: 파괴적 동작은 확인 단계에서 취소할 수 있다. 프로필 데이터는 workspace를 지워도 남으므로 로그인은 유지된다.
  Reach: Git work tree 안과 밖 두 경로가 필요하고, working 상태 pane 2개 이상을 가진 workspace가 필요하다. 파괴적 조작은 `herdr-ide-verify-` 접두어 fixture에서만 만든다.

## 3. Scope And Non-Goals

### 이전 PRD 폐기

`agents/prd/herdr-lightweight-ide/prd.md`는 이 PRD로 대체되어 폐기된다.
그 PRD가 확정했던 Swift 네이티브 앱, libghostty 터미널 서피스, herdr TUI 통째 실행, WKWebView UI, C1 충돌 정책(경고 후 무조건 재로드), 개인용 로컬 빌드 전제가 전부 무효다.
그 PRD는 `human_approval: approved` 상태이지만 소스 0줄이며 2026-08-17부터 PW1(Xcode 전체 설치)에서 차단되어 있었다.
구현 착수 시 그 PRD의 `status`를 `superseded`로 표기하는 것이 v1 태스크다.

### v1 범위

- herdr 소켓 API 클라이언트를 TypeScript로 새로 구현한다(NDJSON RPC, snapshot, 이벤트 구독, protocol 버전 게이팅).
- 3열 고정 레이아웃: 좌 사이드바 + 펫, 중앙 터미널, 우 패널(워크벤치 ↔ 브라우저 전환).
- 사이드바가 사용자 herdr `config.toml`의 `[ui.sidebar.agents]`를 읽어 렌더하고, 없으면 내장 기본 레이아웃을 쓴다.
- 터미널 attach와 분할(⌘D 세로, ⌘⇧D 가로), 닫기, 방향 포커스, split ratio.
- 앱 안에서의 생성과 닫기 전 범위: workspace, worktree, tab, pane 생성. workspace, tab, pane 닫기. worktree 제거.
- 파일트리(workspace 루트 고정 + 명시적 pane 경로 점프)와 워크벤치 뷰어(이미지, 마크다운, 코드 구문 강조, git diff) + 텍스트 파일 인라인 편집·저장.
- 브라우저 패널: workspace당 하나, 여러 pane이 연 페이지를 탭으로 구분하고 소속 pane과 프로필을 배지로 표시.
- workspace 단위 loopback CDP 엔드포인트와 pane 생성 시 `env` 주입, chromux attach.
- grab: 브라우저 패널에서 고른 요소의 HTML·CSS·크롭 스크린샷 경로와 Intent·코멘트를 포커스된 에이전트 pane에 주입.
- 브라우저 프로필(Electron session partition)을 herdr-ide가 소유하고, 프로필 관리 화면을 제공.
- 데스크톱 펫과 사이드바의 포커스 기준 역할 분리, 전역 단축키 토글.
- 원격 herdr 서버 지원: SSH ControlMaster 위의 터미널 attach·분할, 파일트리 탐색, 뷰어.
- herdr-pet에서 옮겨온 지식 문서·에셋·순수 함수 테스트 2건의 랜딩.
- 소스 공개 + 로컬 빌드 배포, macOS 지원.

### 비목표

- **orca로의 이관과 orca 포크.** 참조와 품질 기준선으로만 쓴다.
- **herdr TUI 실행.** 셸 전체를 네이티브로 구성한다.
- **herdr-core(Rust) 재사용.** TypeScript로 재구현한다.
- **chromux 대체.** chromux 명령, 저장 스크립트, 기존 프로필·로그인, 별도 Chrome 창 사용이 전부 유효한 경로로 남는다.
- **herdr-remote-handoff 흡수.** 세션을 통째로 옮기고 되받는 별개의 일이며 보완재로 공존한다.
- **chromux 프로필에서 herdr-ide 파티션으로의 쿠키 임포트.** 패널에서 처음 한 번 로그인한다. 재검토 트리거: 패널 로그인 재설정이 반복적으로 흐름을 끊을 때.
- **diff 라인 코멘트 배치 워크플로.** v2 후보로만 기록한다.
- **파일 조작(생성·이름 변경·삭제·드래그 이동·컨텍스트 메뉴).** 터미널이 항상 옆에 있어 `mv`/`rm`/`touch`가 이미 최소 경로다.
- **원격 인라인 편집·저장, 원격 브라우저 패널과 grab.** 원격 수정은 그 pane의 터미널에서 한다.
- **split된 터미널 자리에 브라우저 넣기.**
- **사용자 설정 파일 계약.** 키맵·디자인 토큰·표면 레지스트리를 코드 안 단일 출처로 모으되 스키마·기본값·마이그레이션을 외부에 약속하지 않는다. 원격 타겟 목록은 문서화된 좁은 예외다.
- **에이전트 정의의 이중화.** herdr가 `server.agent_manifests`로 이미 소유하므로 읽어 쓴다.
- **Apple 서명·공증, Homebrew Cask.**
- **Windows 지원.** Linux는 막지 않되 지원하지 않는다.
- **UI 배선 유닛 테스트.**

### 제품 완결성

v1은 축소한 MVP가 아니라 하루 작업을 담을 수 있는 셸이다.
위 비목표는 전부 사용자 결정이며 각각 사유와 재검토 조건을 갖는다.
herdrm이 실패한 세 가지(키맵, 파일·웹 브라우저 부재, 사이드바 커스텀)는 축소 대상이 아니라 성공 조건이다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
v1이 의존하는 외부 요소가 전부 이미 설치·검증되어 있다.
herdr 0.8.0(protocol 19), chromux, Node v24.12.0, pnpm 10.14.0이 로컬에 있고, mini는 `~/.ssh/config`의 별칭과 키 인증으로 비밀번호 없이 붙는다.
Apple 서명·공증이 비목표라 계정 소유자만 할 수 있는 작업이 없다.

공개 저장소 발행은 사람의 승인이 필요하지만 착수를 막지 않으므로 4.2의 결정 항목으로 둔다.

### 4.2 Human Decisions Before PRD Approval

- HD1. `agents/prd/herdr-lightweight-ide/prd.md`의 폐기를 승인한다. 그 PRD는 아직 `approved` 상태이며 이 승인 없이는 두 개의 승인된 PRD가 서로 모순된 스택을 지시한다.
- HD2. Electron + TypeScript 확정과 herdr-core(Rust) 미재사용을 승인한다. 이 프로젝트의 세 번째 스택 번복이다.
- HD3. 메모리 예산 수치(브라우저 닫힘 400MB, 열림 900MB, workspace 7 / pane 11 기준)를 v1 인수 조건으로 승인한다. 초과하면 v1 미완료다.
- HD4. 같은 workspace의 에이전트가 서로의 브라우저 탭을 CDP로 제어할 수 있다는 위험을 수용한다. 에이전트 오작동 시 다른 에이전트가 작업 중인 페이지(입력 중인 폼, 결제 화면)를 건드릴 수 있다.
- HD5. 검증이 실제 herdr 서버를 상대로 파괴적 조작(workspace 닫기, worktree 제거)을 실행한다는 점과, 그 안전 경계가 `herdr-ide-verify-` 접두어 fixture 가드라는 점을 승인한다.
- HD6. 공개 저장소 발행 시점과 저장소 이름·가시성을 결정한다. v1 배포 형태가 소스 공개 + 각자 빌드이므로 발행이 v1의 일부다.
- HD7. D-53(chromux 라이선스 해소와 전제조건), D-59(orca 실행 화면을 디자인 기준선으로 채택), D-43(grab 인터랙션 확정)이 인터뷰의 독립 심판(gap-audit 사이클 3)을 거치지 않았음을 확인한다. 이 PRD의 spec 게이트가 같은 내용을 qa-log와 대조해 다시 검사한다.
- HD8. 최종 카피, 시각 밀도, 골든 시나리오(하루 실사용) 판정이 사람 몫으로 남는 것을 승인한다. 그중 **시각 밀도(HV2)는 완료를 막는 게이트**다. AC19가 완료 필수이고 'orca 기준선과 같은 밀도인가'를 판정할 수 있는 주체가 사람뿐이므로, 자동 검증이 아니라 사람 판정이 v1 완료를 막는다. 골든 시나리오(HV1)는 게이트가 아니다.

### 4.3 Decision Traceability For Fidelity Review

**사실 (herdr·코드·연구에서 확인)**

- D-01 herdr 0.8.0 소켓 API 메서드 103개 -> R1, 5장 근거.
- D-02 pane cwd 노출(`focused_pane_cwd`/`foreground_cwd`/`workspace_cwd`) -> R7, SC2.
- D-03 herdr-core가 RPC·SSH 터널을 이미 구현 -> D-14로 재사용 기각. 컨텍스트 사실.
- D-04 이전 PRD가 approved이나 소스 0줄, PW1에서 차단 -> 3장 폐기 근거, T2.
- D-08 orca는 MIT, TS/Electron, 53.2k stars -> 컨텍스트 사실. D-09의 입력.
- D-12 Rust CDP 임베딩에 실용 경로 없음(cef-rs#192가 1년째 OPEN) -> D-10 근거, 10장 컨텍스트.
- D-13 성능 축은 무게가 아니라 Chromium 엔진 개수. Tauri+CEF는 엔진 2개 -> D-10 근거. orca 이탈 실증이 붙어 RISK1으로 게이트화.
- D-24 herdr에서 worktree와 workspace는 1:1 -> R6, SC6. 사용자가 상정했던 '한 workspace에 worktree 여러 개'는 herdr 모델에 없다.
- D-39 프로젝트 동기는 herdrm 실사용 불만 세 가지 -> 2장 문제·성공의 모습, AC 판정 기준.

**채택된 사용자 결정**

- D-09 orca는 참조·품질 기준선이며 대체 후보가 아니다 -> 3장 비목표(orca 이관), R19.
- D-06 herdr 플러그인 3개를 네이티브 표면으로 흡수 -> R3, R8, R9.
- D-07 pet을 별도 앱이 아니라 herdr-ide의 한 표면으로 -> R13.
- D-10 스택은 Electron + TypeScript -> 5장, HD2.
- D-11 브라우저 패널은 B2(패널 + loopback CDP + grab까지 v1) -> R9, R11, R12, T8~T10.
- D-14 herdr-core를 재사용하지 않고 TS 재구현 -> R1, 5장, HD2.
- D-15 pet-app은 v1 완료 시 은퇴 -> T15. D-33 스파이크 실패 시 연기된다.
- D-16 화면 구조 L1(3열 고정 + 우측 패널 전환) -> R4, SC1~SC3.
- D-17 터미널 분할은 v1 기본, ⌘D/⌘⇧D 계열 앱 단축키 -> R5, AC5.
- D-18 브라우저는 workspace 단위 하나, 탭으로 구분, 자동 전환 없이 미확인 배지 -> R9, SC3.
- D-19 CDP 엔드포인트는 workspace 단위 loopback, pane 생성 시 `env`로 URL 주입 -> R11, AC11.
- D-20 같은 workspace 에이전트 간 브라우저 탭 제어 허용(위험 수용) -> RISK3, HD4.
- D-21 브라우저 프로필은 herdr-ide 소유(Electron session partition), workspace마다 default, 탭에 배지 -> R10, AC10.
- D-22 쿠키 임포트는 v1 제외 -> 3장 비목표 + 재검토 트리거.
- D-23 herdr-ide 브라우저 패널은 chromux를 대체하지 않는다 -> 3장 비목표, R11.
- D-25 파일트리·워크벤치는 F2(뷰어 + 가벼운 인라인 편집) -> R8. F3는 비목표.
- D-26 트리 루트는 T3(workspace 루트 고정 + 명시적 점프 + 불일치 표시) -> R7, AC7, SC2.
- D-27 편집 충돌은 C2(내 편집 유지 / 다시 읽기 선택) -> R8, AC8, RISK4.
- D-28 커스텀은 내부 단일 출처까지, 외부 계약은 만들지 않는다 -> R16, 3장 비목표, 11장 가드레일.
- D-29 v1 배포는 R1(소스 공개 + 각자 빌드) -> R18, HD6. 서명·공증은 비목표.
- D-30 v1 지원 플랫폼은 macOS만 -> R18, 3장 비목표(Windows).
- D-31 pet과 사이드바의 역할을 창 포커스로 분리(E1) -> R13, AC13, SC5.
- D-32 v1에서 herdr-ide가 데스크톱 펫까지 흡수(W2) -> R13, T3, T15.
- D-33 W2의 Electron 창 옵션 조합은 미검증이므로 초반 게이팅 스파이크 -> T3, RISK2.
- D-34 검증 주 모드는 chromux가 herdr-ide CDP로 실제 앱을 모는 것. 순수 함수 2건만 이식 -> 9.1장, V2, V3~V7.
- D-35 에이전트가 검증할 수 있게 만드는 것 자체가 설계 요구사항 -> R15, 11장 가드레일.
- D-36 코드는 `~/projects/herdr-ide`를 본진으로 새로 시작하고 herdr-pet은 아카이브, 지식 이식은 v1 태스크 -> T1, T15.
- D-37 원격 herdr 지원(M2 + 원격 파일트리, 편집·브라우저는 로컬 전용) -> R14, SC4, V9.
- D-38 herdr 서버 미실행 시 자동 기동하지 않고 두 실패 상태를 구분(N2) -> R2, AC2, SC1.
- D-40 사이드바는 herdr `config.toml`의 `[ui.sidebar.agents]`와 `agents[].tokens`를 읽어 렌더 -> R3, AC3.
- D-41 메모리 예산을 실사용 규모(workspace 7 / pane 11) 인수 조건으로 -> AC17, V8, HD3.
- D-42 앱 안에서 생성·닫기 전 범위 지원 -> R6, SC6.
- D-43 grab 인터랙션 확정(토글 진입 + 배너, 아웃라인, 팝오버 순서, Intent = Change/Question, Add/Cancel/ESC) -> R12, AC12. 호버 형태·중첩 이동·다중 선택만 디자인 단계로 잔류.
- D-44 pane 닫기 확인 정책(working·attention이면 확인, idle이면 즉시) -> R6, AC6.
- D-45 원격 타겟은 설정 파일 입력 + `~/.ssh/config` 후보 제시, 형식은 herdr-pet의 `[[targets]]` 이식 -> R14, D-28의 문서화된 예외.
- D-46 herdr 버전은 `protocol` 정수로 게이팅하고 미달 시 부분 비활성화 없이 실패 -> R1, AC1.
- D-47 프로필 세션 데이터는 workspace·worktree 삭제 시 보존하고 관리 화면에서 수동 정리 -> R10, AC10.
- D-48 grab 스크린샷은 임시 파일 경로로 주입하고 파일 존재까지 검증 -> R12, AC12, V5.
- D-49 pet·사이드바 전역 단축키 토글, `event.code` 함정 이식 -> R13, AC13.
- D-50 workspace·tab 닫기는 집계 경고 하나 + `tokens.summary` 나열 -> R6, AC6.
- D-51 diff 코멘트 배치는 v1 비목표 -> 3장 비목표, v2 후보.
- D-52 herdr-remote-handoff는 유지하며 대체하지 않는다 -> 3장 비목표.
- D-53 chromux를 명시적 전제조건으로 두고 부재 시 degrade. 라이선스 선행 항목은 MIT 추가로 해소 -> R11, R18, AC11.
- D-54 사이드바 기본 레이아웃을 내장하고 사용자 config가 있으면 덮어쓴다 -> R3, AC3.
- D-55 브라우저 WebContents는 활성 workspace의 것만 상주, 전환 시 파기 후 URL·프로필로 재로드 -> R9, AC9, AC17 성립 조건.
- D-56 파괴적 동작 검증은 `herdr-ide-verify-` 접두어 fixture에서만 -> T11, V6, HD5.
- D-57 grab은 받을 대상이 없으면 진입 자체를 막고 사유 표시 -> R12, AC12, SC3.
- D-58 브라우저 탭 프로필 선택은 사람의 명시적 UI 행위로만 -> R10, AC10.
- D-59 orca 실행 화면을 시각·인터랙션 기준선으로 채택 -> R19, HD8, `docs/design-reference/`.

**기각·대체된 결정**

- D-05 Rust/Tauri + herdr-core 재사용 -> **기각**. D-10이 대체했다. 브라우저 패널 상한을 잃는 거래였고 성능 논거도 뒤집혔다.
- 이전 PRD의 D-13(Swift), D-15(libghostty), D-16(TUI 통째 실행), C1(무조건 재로드) -> **무효 유지**. 3장에 명시.
- Q2의 C안(chromux 세션 관리·표시만) -> **폐기**. Tauri 전제였다.
- E3(펫과 사이드바 동시 표시) -> 기각. 같은 사실을 두 곳에서 말하면 신뢰가 갈린다.
- B(pane마다 개별 닫기 확인), C(조용히 닫기) -> 기각. D-50의 근거에 기록.
- T1(트리 완전 고정), T2(자동 추종) -> 기각. D-26의 근거에 기록.
- F1(보기 전용), F3(파일 조작) -> 기각. D-25의 근거에 기록.
- C1(경고 후 무조건 재로드), C3(3-way 병합) -> 기각. D-27의 근거에 기록.
- L2(터미널 전체 + 호출형), L3(자유 분할) -> 기각. D-16의 근거에 기록.
- base64 인라인 스크린샷 -> 기각. 프롬프트 오염.
- 버전 미달 시 기능별 부분 비활성화 -> 기각. 조용한 기능 저하 금지.
- 프로필 데이터 자동 삭제 -> 기각. 재로그인 강제.

**에이전트 소유 가정 (사용자 결정 아님)**

- ASM1. Electron의 브라우저 임베딩 API로 무엇을 쓸지는 구현이 정한다. 인터뷰는 orca가 `<webview>` 게스트를 쓴다는 사실을 기록했으나, Electron은 `<webview>`를 권장 API로 두지 않고 `WebContentsView`를 두고 있다. 이 PRD가 요구하는 것은 "패널 안의 브라우저 뷰와 그 뷰에 붙는 loopback CDP 엔드포인트"라는 능력이지 특정 태그가 아니다. 구현이 판단해 고르고 그 선택을 결과 보고에 남긴다.
- ASM2. `pane.close`가 PTY와 그 프로세스를 종료하는지 뷰만 떼는지는 herdr 실동작으로 확정한다. 종료라면 확인 문구에 '세션과 진행 중인 작업이 사라진다'를 명시한다(D-44가 이 확인을 지시).
- ASM3. 원격 파일 접근은 SSH ControlMaster 위의 sftp를 쓴다고 인터뷰가 기록했으나, 실제 전송 수단은 구현이 원격 herdr API로 대체할 수 있으면 그렇게 한다. 사용자 결정은 "원격에서 탐색·보기까지 되고 편집은 안 된다"이지 전송 프로토콜이 아니다.
- ASM4. 상태바에 메모리를 상시 노출한다는 것은 D-59의 orca 대응에서 나온 해석이다. 회귀를 늦게 발견하지 않게 한다는 목적은 유지하되 시각 형태는 디자인 단계 몫이다.
- ASM5. `INV-herdr-unseen-token` 불변식은 herdr-pet **대시보드**가 확인된 에이전트를 목록에서 지운다고 기록하지만, herdr-ide 사이드바는 herdr 자체 사이드바를 재현하는 표면이라 같은 규칙이 아니다(D-40). 이 저장소로 규칙을 다시 랜딩할 때 적용 범위를 사이드바가 아닌 attention 승격 판정으로 좁힌다. 승격 규칙 자체(`_new`만 attention)는 그대로 유효하다.

**원칙 적용**

`agents/config.json`이 `~/projects/oh-my-principle`을 principle 저장소로 선언하고 `sasu principles list`가 engineering과 design 두 도메인을 보고한다.
두 도메인 모두 이 PRD가 다루는 일(코드·아키텍처·의존성·에러 경로, 그리고 사용자가 작업을 수행하는 화면)에 트리거가 걸리므로, 두 문서를 전문으로 읽고 규칙 전부를 11장 가드레일로 옮겼다.
번역하지 않은 규칙: engineering 9(로그 설계)와 11(모든 연산은 두 번 돈다)은 v1의 관측 가능 표면이 herdr 소켓 API와 CDP라 별도 가드레일 없이 R15와 AC15로 이미 강제된다.
engineering 6·7(기존 라이브러리 우선)은 11장에 일반 조항으로만 두고 개별 AC로 만들지 않는다.

## 5. Major Technical Structure Changes

- **새 런타임과 저장소.** `~/projects/herdr-ide`에 Electron + TypeScript 앱을 새로 만든다. 기존 저장소에 얹지 않는다.
- **herdr 연동 계층을 TypeScript로 신설.** Unix 소켓 NDJSON RPC 클라이언트, `session.snapshot` 파싱, 이벤트 구독, `protocol` 정수 기반 버전 게이팅. herdr-core(Rust)는 이식 참조일 뿐 빌드 대상이 아니다.
- **터미널 표면.** herdr TUI를 실행하지 않고 pane PTY에 직접 attach한다. 분할과 레이아웃은 herdr API(`pane.split`, `pane.close`, `pane.focus_direction`, `layout.set_split_ratio`)에 위임하고 herdr-ide는 렌더와 입력만 맡는다.
- **브라우저 서브시스템 신설.** workspace당 브라우저 뷰 하나, Electron session partition 기반 프로필, loopback에 여는 CDP HTTP/WS 엔드포인트(`/json/version`, `/json/list`에 `webSocketDebuggerUrl` 응답), 게스트 페이지에 주입하는 grab 콘텐츠 스크립트.
- **새 외부 신뢰 경계.** loopback CDP 엔드포인트가 열린다. 그 workspace의 모든 에이전트가 그 엔드포인트의 모든 탭을 제어할 수 있다(D-20 수용).
- **로컬 영속 상태 신설.** 브라우저 프로필 파티션(쿠키·스토리지), 펫 창 위치, 원격 타겟 설정 파일, 열려 있던 탭의 URL·프로필. 프로필 데이터는 workspace 삭제와 수명이 분리된다.
- **원격 경계.** SSH ControlMaster 연결 하나 위에 herdr 소켓 터널과 파일 접근을 함께 태운다. 편집과 브라우저는 이 경계를 넘지 않는다.
- **데스크톱 펫 창.** 투명·무테·항상 위·전체 Spaces 창을 herdr-ide 프로세스가 직접 소유한다. 별도 앱 프로세스가 사라진다.
- **herdr 소유 계약을 읽기만 한다.** 사이드바 레이아웃(`[ui.sidebar.agents]`)과 에이전트 정의(`server.agent_manifests`)를 herdr-ide가 이중으로 정의하지 않는다.

## 6. Requirements

- R1. herdr 소켓 API 클라이언트를 TypeScript로 구현한다. `session.snapshot`과 이벤트 구독으로 workspace/tab/pane/agent 계층과 `agents[].tokens`를 읽고, `protocol` 정수가 요구 버전에 미달하면 기능별 우회나 부분 비활성화 없이 명확한 사유와 함께 실패한다.
- R2. herdr 서버가 없을 때 자동 기동하지 않는다. '소켓 파일 없음'과 '파일은 있으나 응답 없음'을 서로 다른 상태로 표시하고, 사용자의 명시적 조작(서버 실행 버튼)으로만 띄운다.
- R3. 사이드바는 herdr `config.toml`의 `[ui.sidebar.agents]` rows(토큰 + fg/bold)를 읽어 렌더하고 값은 `agents[].tokens`에서 가져온다. 그 섹션이 없으면 내장 기본 레이아웃(첫 줄 상태 토큰 + 에이전트 아이콘 + workspace 라벨 + elapsed, 둘째 줄 summary)을 쓴다. `_new` 접미 토큰만 attention으로 승격하고, 사용자가 이미 확인한 상태는 idle로 떨어져 attention 강조가 풀린다. 그 에이전트는 목록에서 사라지지 않고 idle 항목으로 계속 보인다. herdr 자체 사이드바가 보여주는 것과 같다.
- R4. 창은 3열 고정이다. 좌측은 사이드바와 펫, 중앙은 터미널, 우측은 패널이며 우측 패널은 워크벤치와 브라우저를 전환한다. 매번 배치를 정하게 하지 않는다.
- R5. 터미널 pane을 attach하고 앱 단축키로 세로·가로 분할, 닫기, 방향 포커스 이동, 분할 비율 조정을 한다. 조작은 herdr API에 위임한다.
- R6. workspace, worktree, tab, pane을 앱 안에서 만들고 workspace, tab, pane을 닫고 worktree를 제거한다. 파괴적 동작은 실행 전에 결과를 문장으로 알린다. pane 닫기는 그 pane의 에이전트가 working이나 attention이면 확인을 받고 idle이면 즉시 닫는다. workspace·tab 닫기는 개별 확인 대신 집계 경고 하나를 띄우고 종료될 에이전트를 `tokens.summary`와 함께 나열하며, 포함된 pane이 전부 idle이면 확인 없이 닫는다. worktree 제거는 체크아웃이 지워진다는 결과를 고지하고 확인을 받는다. herdr가 돌려주는 실패 사유(`not_git_worktree` 등)는 그대로 보여준다.
- R7. 파일트리 루트는 workspace 체크아웃 루트에 고정하고 pane 포커스를 자동 추종하지 않는다. 포커스된 pane의 `foreground_cwd`가 트리 루트와 다르면 그 사실을 트리 상단에 표시하고, 그 경로로의 이동은 명시적 조작으로만 일어난다.
- R8. 워크벤치는 이미지 렌더링, 마크다운, 코드 구문 강조, git diff를 보여주고 텍스트 파일의 인라인 편집과 저장을 지원한다. 편집 중 외부 변경이 감지되면 '내 편집 유지 / 다시 읽기'를 사용자가 고르게 하고, 어느 쪽이든 무엇을 잃는지 알린다.
- R9. 브라우저 패널은 workspace당 하나다. 그 workspace의 여러 pane이 연 페이지는 탭으로 구분되고 탭 라벨에 소속 pane과 프로필이 배지로 표시된다. 다른 에이전트가 새 페이지를 열어도 보고 있던 탭을 자동 전환하지 않고 미확인 배지만 띄운다. 브라우저 WebContents는 활성 workspace의 것만 상주하고, workspace를 전환하면 파기했다가 복귀 시 URL과 프로필로 재로드한다.
- R10. 브라우저 프로필은 herdr-ide가 소유한다. workspace마다 default 프로필이 있고 다른 프로필로 탭을 여는 것은 사람의 명시적 UI 행위로만 일어난다. 에이전트는 주입된 default 프로필만 쓰고 스스로 전환하지 않는다. 프로필 세션 데이터는 workspace나 worktree를 지워도 보존되며, 관리 화면이 프로필별 마지막 사용 시각과 용량을 보여 수동 정리를 돕는다.
- R11. workspace 단위로 loopback CDP 엔드포인트를 열고, pane을 만들 때 herdr API의 `env` 파라미터로 그 URL을 주입해 에이전트가 `chromux open --cdp-url` 로 그대로 붙게 한다. chromux가 없는 환경에서는 브라우저 패널은 뜨되 에이전트 연동만 동작하지 않는 상태로 degrade하고 그 사유를 표시한다.
- R12. grab은 브라우저 패널에서 요소를 골라 포커스된 에이전트 pane에 넘긴다. 명시적 토글로 진입하고 켜진 동안 배너로 상태를 남기며, 선택된 요소에 아웃라인을 그리고, 팝오버가 대상 식별(요소 요약 + 셀렉터) -> 자유 입력 코멘트 -> Intent(Change/Question) 선택 -> Cancel/Add 순서로 진행한다. Add하면 HTML·CSS와 크롭 스크린샷의 임시 파일 경로, 그리고 Intent와 코멘트가 그 pane에 주입된다. 포커스된 pane이 없거나 그 pane에 에이전트가 실행 중이 아니면 진입 자체를 막고 사유를 표시한다.
- R13. 데스크톱 펫은 herdr-ide가 직접 띄운다. herdr-ide 창이 앞에 있으면 사이드바가 attention을 알리고 펫은 조용히 있으며, 창이 뒤로 가면 펫이 알린다. 같은 사실을 두 표면이 동시에 말하지 않는다. 저장된 펫 위치가 화면 밖이면 복구하고, 펫과 사이드바는 전역 단축키로 표시·숨김을 토글한다.
- R14. 원격 herdr 서버에 SSH ControlMaster 연결 하나로 붙어 사이드바 표시, 터미널 attach와 분할, 파일트리 탐색, 뷰어, pane 경로 점프를 지원한다. 원격 타겟은 설정 파일로 입력할 수 있고 동시에 `~/.ssh/config`를 읽어 후보로 제시하며, 사용자가 UI에서 고른 호스트는 같은 파일에 기록된다. 인라인 편집과 브라우저 패널은 원격에서 제공하지 않으며, 회색으로 비워두지 않고 이유를 표시한다. 연결이 끊기면 그 사실을 알리고 빈 목록으로 위장하지 않는다.
- R15. herdr-ide가 표현하는 상태는 앱 밖에서 확인 가능해야 한다. 앱이 자기 CDP 엔드포인트를 열 수 있어야 하고, UI가 보여주는 상태는 herdr 소켓 API나 파일 시스템 같은 관측 가능한 표면으로 대조할 수 있어야 한다. 사람만 판정할 수 있는 항목은 최소로 남기고 명시한다.
- R16. 키맵 테이블(command id -> 기본 키), 디자인 토큰, 표면 레지스트리(파일 타입 -> 뷰어)는 코드 안 한 곳에 모은다. 값이 컴포넌트에 흩어지지 않는다. 이를 사용자 설정 파일로 노출하거나 스키마·기본값·마이그레이션을 약속하지 않는다.
- R17. 실사용 규모에서 무겁지 않다. 상태바가 현재 메모리 사용량을 상시 노출해 회귀를 늦게 발견하지 않게 한다.
- R18. macOS에서 소스를 받아 로컬 빌드로 실행할 수 있다. herdr와 chromux가 전제조건임을 문서가 명시하고, 경로와 셸 호출을 macOS 전제로 하드코딩하지 않되 다른 플랫폼을 지원한다고 적지 않는다.
- R19. 시각·인터랙션 밀도는 `docs/design-reference/`의 orca 실행 화면을 기준선으로 삼는다. 기능을 베끼지 않고 밀도, 배치, 상태 표현 방식을 따르며 스코프는 이 PRD가 우선한다.

## 7. Acceptance Criteria

- AC1. herdr 0.8.0(protocol 19) 서버에서 사이드바·터미널·파일트리가 실제 workspace/tab/pane/agent 데이터로 채워진다. 요구 protocol에 미달하는 서버에 붙으면 앱이 어떤 기능도 부분 동작시키지 않고 버전 불일치 사유를 화면에 표시한 채 멈춘다.
- AC2. herdr 소켓 파일이 없는 상태와 소켓은 있으나 응답이 없는 상태에서 화면이 서로 다른 문구를 보여주고, 어느 쪽에서도 앱이 서버를 자동으로 띄우지 않는다. '서버 실행'을 누른 뒤에만 서버 프로세스가 생긴다.
- AC3. 사용자 `config.toml`에 `[ui.sidebar.agents]`가 있으면 사이드바 각 항목이 그 rows 정의대로 두 줄로 그려지고 `tokens.summary`(agent-context-labels의 산출물)가 표시된다. 그 섹션을 지우면 내장 기본 레이아웃으로 같은 정보가 그려진다. 사용자가 이미 확인한 질문 상태 에이전트는 attention으로 강조되지 않고 idle 항목으로 목록에 남아, herdr 자체 사이드바가 같은 순간에 보여주는 것과 일치한다.
- AC4. 창을 열면 좌 사이드바·중앙 터미널·우 패널이 배치되어 있고, 우측 패널 전환 조작으로 워크벤치와 브라우저가 서로 바뀐다. 사용자가 배치를 직접 구성하지 않아도 세 표면이 동시에 보인다.
- AC5. 앱 단축키로 터미널이 세로·가로로 나뉘고, 방향 이동으로 포커스가 옮겨가고, 분할 경계를 끌면 비율이 바뀌고, 닫기로 pane이 사라진다. 그 결과가 herdr 쪽 pane 구조에도 동일하게 반영되어 있다.
- AC6. 앱 안에서 workspace, worktree, tab, pane을 만들 수 있고 herdr 쪽에도 생긴다. working 상태 에이전트가 있는 pane을 닫으려 하면 그 프로세스에 무슨 일이 생기는지 문장으로 알리는 확인이 뜨고, idle pane은 확인 없이 닫힌다. working pane 2개를 가진 workspace를 닫으려 하면 확인이 두 번이 아니라 한 번 뜨고 그 안에 두 에이전트가 각자의 요약과 함께 나열된다. worktree 제거는 체크아웃이 지워진다는 것을 알린 뒤에만 진행된다. Git work tree가 아닌 경로에서 worktree를 만들려 하면 herdr가 준 사유가 그대로 보인다.
- AC7. 파일트리가 workspace 체크아웃 루트를 보여주고, 포커스된 pane을 하위 경로로 `cd`한 pane으로 바꿔도 트리와 펼침 상태가 그대로 유지된다. 대신 트리 상단이 그 pane이 다른 경로에 있다는 것을 알리고, 명시적 조작을 했을 때만 트리가 그 경로로 이동한다.
- AC8. 워크벤치에서 PNG는 이미지로, 마크다운은 렌더된 문서로, 소스 파일은 구문 강조된 코드로, 변경된 파일은 diff로 보인다. 텍스트 파일을 고쳐 저장하면 디스크에 반영된다. 편집 중 같은 파일이 밖에서 바뀌면 선택 다이얼로그가 뜨고, '내 편집 유지'를 고른 뒤 저장하면 내 내용이, '다시 읽기'를 고르면 바깥 내용이 남는다.
- AC9. 한 workspace의 서로 다른 pane이 연 페이지가 같은 브라우저 패널의 탭으로 보이고 각 탭에 소속 pane과 프로필 배지가 붙는다. 보고 있지 않은 탭에 새 페이지가 열리면 화면이 그 탭으로 튀지 않고 미확인 배지만 생긴다. 다른 workspace로 갔다가 돌아오면 열려 있던 탭이 같은 URL과 프로필로 다시 뜬다.
- AC10. 브라우저 패널에서 사용자가 프로필을 골라 새 탭을 열면 그 탭만 별도 로그인 세션이 된다. 에이전트가 CDP로 조작해도 프로필은 workspace default에서 바뀌지 않는다. workspace를 지워도 그 프로필로 다시 열면 로그인이 남아 있고, 관리 화면에 프로필별 마지막 사용 시각과 용량이 보이며 거기서 지울 수 있다.
- AC11. herdr-ide가 만든 pane 안에서 환경변수로 주입된 CDP URL을 그대로 써서 `chromux open --cdp-url`이 패널의 탭에 붙고, 붙은 뒤 그 탭을 조종할 수 있다. chromux가 설치되지 않은 환경에서는 브라우저 패널 자체는 뜨고 에이전트 연동 기능만 사유와 함께 비활성 상태로 보인다.
- AC12. 에이전트가 실행 중인 pane을 포커스한 상태에서 grab을 켜면 배너가 뜨고, 요소에 아웃라인이 그려지고, 팝오버가 대상 요약과 셀렉터를 보여주며 코멘트 입력과 Change/Question 선택을 받는다. Add를 누르면 그 pane에 HTML·CSS와 스크린샷 파일 경로, Intent, 코멘트가 들어가고 그 경로의 파일이 실제로 존재하며 이미지로 열린다. ESC나 Cancel은 아무것도 주입하지 않는다. 에이전트가 없는 맨 셸 pane을 포커스한 상태에서는 grab이 켜지지 않고 그 이유가 표시되며, 셸에 아무것도 타이핑되지 않는다.
- AC13. herdr-ide 창이 앞에 있으면 attention이 사이드바에만 나타나고 펫은 조용하다. 다른 앱으로 포커스를 옮기면 펫이 그 상태를 드러낸다. 두 표면이 동시에 같은 attention을 주장하지 않는다. 전역 단축키로 펫과 사이드바가 각각 사라졌다 다시 나타난다. 화면 밖 좌표가 저장되어 있어도 다음 실행에서 펫이 보이는 위치에 나타난다.
- AC14. mini의 herdr 서버가 사이드바에 함께 보이고 그쪽 pane에 attach해 입력할 수 있고 분할도 된다. 원격 파일트리로 탐색하고 뷰어로 볼 수 있다. 원격 workspace에서 인라인 편집과 브라우저 패널은 제공되지 않고 그 이유가 화면에 적혀 있다. SSH 연결을 끊으면 앱이 빈 목록이 아니라 연결 끊김 상태를 보여준다.
- AC15. herdr-ide를 원격 디버깅 포트와 함께 띄우면 외부에서 그 CDP에 붙어 UI를 조작할 수 있고, 조작 결과를 herdr 소켓 API 조회로 확인할 수 있다.
- AC16. 앱의 모든 기본 단축키가 하나의 키맵 테이블에서 나오고, 색·간격·타이포가 하나의 토큰 정의에서 나오며, 파일 타입과 뷰어의 연결이 하나의 레지스트리에서 나온다. 같은 값이 두 곳에 하드코딩되어 있지 않다. 사용자 설정 파일 스키마는 원격 타겟 목록 외에 없다.
- AC17. workspace 7 / pane 11 규모에서 브라우저 패널을 닫은 상태의 herdr-ide 전체 프로세스 메모리 합이 400MB 이하이고, 브라우저 패널 하나를 연 상태에서 900MB 이하다. 상태바에 그 값이 상시 보인다.
- AC18. 저장소를 받아 문서에 적힌 절차대로 macOS에서 빌드하면 앱이 실행된다. 문서가 herdr와 chromux를 전제조건으로 명시하고 지원 플랫폼을 macOS로만 적는다.
- AC19. 완성된 화면이 `docs/design-reference/`의 orca 기준선과 같은 밀도로 정보를 담는다. 상태를 문장이 아니라 색·배지·배치로 드러내고, 설명 문단으로 레이아웃을 대신하지 않는다.

## 8. PRD-Level Tasks

- T1. Electron + TypeScript 프로젝트 뼈대를 세우고 herdr-pet에서 옮겨온 문서·에셋·참조 원본이 새 구조 안에서 제자리를 갖게 한다. `agents/rules/`의 규칙들을 이 저장소 기준으로 다시 랜딩하거나 폐기한다(`INV-pet-state-off-main-thread`는 Tauri 고유 제약이라 재검토 대상이다). Covers R18, AC18.
- T2. `agents/prd/herdr-lightweight-ide/prd.md`를 폐기 상태로 표기하고 이 PRD가 대체한다는 것을 그 문서에 남긴다. Covers 3장. Depends on: none.
- T3. 펫 창 게이팅 스파이크. 투명·무테·항상 위·전체 Spaces 창을 띄우고 클릭스루 상태에서 드래그가 끊기지 않으며 오프스크린 좌표에서 복구되는지 실물로 확인한다. `docs/pet-window-macos.md`의 함정 목록이 체크리스트다. 실패하면 W1(pet-app 유지)으로 후퇴하고 D-15의 은퇴를 연기한다. Covers R13. Depends on: T1.
- T4. herdr 소켓 클라이언트와 상태 모델을 TypeScript로 구현한다. NDJSON RPC, 스냅샷 파싱, 이벤트 구독, protocol 게이팅, `_new` 토큰 판정. `docs/ported-reference/herdr.rs`의 순수 함수 테스트를 함께 이식한다. Covers R1, AC1. Depends on: T1.
- T5. 서버 미실행 두 상태 처리와 명시적 기동 경로. Covers R2, AC2. Depends on: T4.
- T6. 3열 레이아웃과 사이드바. `[ui.sidebar.agents]` 읽기, 내장 기본 레이아웃, 토큰 렌더, attention 표시. Covers R3, R4, AC3, AC4. Depends on: T4.
- T7. 터미널 attach와 분할·닫기·방향 포커스·비율, 그리고 키맵 테이블 단일 출처. Covers R5, R16, AC5, AC16. Depends on: T6.
- T8. 앱 안의 생성·닫기 조작 전 범위와 파괴적 동작 고지(개별 확인, 집계 경고, worktree 제거 고지, herdr 실패 사유 전달). Covers R6, AC6. Depends on: T6.
- T9. 파일트리(T3 루트 정책)와 워크벤치 뷰어·인라인 편집·충돌 선택. Covers R7, R8, AC7, AC8. Depends on: T6.
- T10. 브라우저 패널과 프로필 모델: workspace당 하나, 탭·배지·미확인 배지, 활성 workspace만 상주, 프로필 선택기와 관리 화면, 프로필 데이터 수명. Covers R9, R10, AC9, AC10. Depends on: T6.
- T11. loopback CDP 엔드포인트와 `env` 주입, chromux attach 경로, chromux 부재 시 degrade. Covers R11, AC11. Depends on: T10.
- T12. grab: 진입 조건 판정, 모드 배너와 아웃라인, 팝오버(대상·코멘트·Intent·Add/Cancel), 크롭 스크린샷 임시 파일 생성과 경로 주입. Covers R12, AC12. Depends on: T11.
- T13. 데스크톱 펫과 사이드바의 포커스 기준 역할 분리, 전역 단축키 토글(`event.code` 함정 반영), 오프스크린 클램프 기하와 그 순수 함수 테스트 이식. Covers R13, AC13. Depends on: T3, T6.
- T14. 원격 herdr 지원: SSH ControlMaster, 원격 타겟 설정 파일과 `~/.ssh/config` 후보 제시, 원격 사이드바·터미널·파일트리·뷰어, 비지원 기능의 사유 표시, 연결 끊김 고지. Covers R14, AC14. Depends on: T9.
- T15. 검증 하네스: herdr-ide를 원격 디버깅 포트로 띄우고 chromux가 붙어 조작하는 경로, herdr 소켓 API로 결과를 확인하는 경로, 그리고 `herdr-ide-verify-` 접두어가 아닌 대상에서는 실행을 거부하는 파괴적 동작 fixture 가드. Covers R15, AC15. Depends on: T4.
- T16. 메모리 계측과 상태바 상시 노출, 실사용 규모 예산 판정 스크립트. Covers R17, AC17. Depends on: T10.
- T17. 디자인 토큰과 표면 레지스트리 단일 출처를 orca 기준선에 맞춰 정리하고 상태 표현을 색·배지·배치로 통일한다. Covers R19, R16, AC19. Depends on: T9, T10.
- T18. 빌드·실행 문서와 전제조건(herdr, chromux) 명시, 지원 플랫폼 표기. Covers R18, AC18. Depends on: T17.
- T19. pet-app 은퇴와 herdr-pet 아카이브. T3 스파이크가 통과하고 T13이 완료된 뒤에만 실행한다. Covers R13. Depends on: T13.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | 저장소 건강성, 타입·린트·빌드 | none |
| automated behavior | yes | 이식한 순수 함수(오프스크린 클램프, `_new` 토큰 판정)와 herdr 프로토콜 파싱의 회귀 | none |
| browser/runtime | yes | 실제 herdr 서버 위에서 chromux가 herdr-ide CDP로 앱을 몰고 herdr 소켓 API로 결과를 확인하는 주 경로 | 최종 카피 판정. 시각 밀도는 HV2가 완료 게이트로 판정한다 |
| performance measurement | yes | 실사용 규모 메모리 예산 | 예산 수치 승인(HD3) |
| remote runtime | no/blockable | 원격 herdr 서버 지원 | mini 가용성 |

`automated behavior`가 순수 함수와 프로토콜 파싱에 한정되는 것은 사용자 결정이다(D-34).
UI 배선 유닛 테스트는 리팩터마다 깨지고 잡는 버그가 없어 유지비가 보호가치를 넘는다.
그 자리를 `browser/runtime`이 실제 앱 구동으로 대신하며, 이 모드가 v1 완료 판정의 주 모드다.

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R19, AC16, AC18 | 타입 검사·린트·프로덕션 빌드가 실패 없이 통과하고 빌드 산출물이 macOS에서 실행된다. 더해 구조 검사가 키맵 테이블·디자인 토큰·표면 레지스트리가 각각 단일 모듈에서만 나오는 것을 확인해, 같은 값이 두 곳에 하드코딩되면 빌드가 실패한다 | yes | no |
| V2 | automated behavior | R1, R13, AC1, AC13 | 오프스크린 좌표 복구와 `_new` 토큰 판정이 이식한 테스트로 고정되어, 토큰 매핑이나 창 복구 로직을 바꿔도 확인된 상태가 attention으로 새거나 저장된 화면 밖 좌표가 그대로 복원되는 회귀가 잡힌다. protocol 게이팅 판정도 같은 층에서 고정한다 | yes | no |
| V3 | browser/runtime | R1-R5, R19, AC1-AC5, AC19, SC1 | 사이드바 항목을 눌러 그 pane으로 포커스가 옮겨가고 분할·비율·닫기가 herdr 쪽 구조에 반영되며, 서버 소켓 부재와 무응답이 서로 다른 화면으로 구분되고 어느 쪽에서도 서버가 자동으로 뜨지 않는다. 같은 화면에서 3열이 동시에 보이고, 상태가 설명 문단이 아니라 색·배지·배치로 드러난다. orca 기준선과의 밀도 비교 자체는 자동으로 판정하지 않고 HV2가 맡는다 | yes | no |
| V4 | browser/runtime | R7, R8, AC7, AC8, SC2 | 트리가 workspace 루트를 유지한 채 pane 경로 불일치를 알리고, 이미지·마크다운·코드·diff가 각각 제 형태로 렌더되며, 외부 변경 충돌에서 사용자가 고른 쪽이 실제로 남는다 | yes | no |
| V5 | browser/runtime | R11, R12, AC11, AC12, SC3 | grab이 대상 pane에 HTML·CSS와 Intent·코멘트를 주입하고 주입된 스크린샷 경로에 파일이 실제로 존재해 이미지로 열리며, 에이전트가 없는 pane에서는 진입이 막히고 셸에 아무것도 타이핑되지 않는다 | yes | no |
| V6 | browser/runtime | R6, AC6, SC6 | 생성 조작이 herdr 목록에 반영되고, working pane 닫기에 확인이 뜨고 idle pane에는 뜨지 않으며, working pane 2개를 가진 workspace 닫기에 개별 확인이 아니라 요약이 붙은 집계 경고 하나가 뜨고, Git work tree 밖의 worktree 생성 실패 사유가 그대로 표시된다. 파괴적 조작은 `herdr-ide-verify-` 접두어 대상에서만 실행되며 가드가 다른 접두어를 거부한다 | yes | no |
| V7 | browser/runtime | R13, AC13, SC5 | herdr-ide 창의 포커스를 뺏으면 attention 표시가 사이드바에서 펫으로 넘어가고 되돌리면 반대로 뒤집히며, 전역 단축키가 펫과 사이드바의 표시 상태를 토글하고, 클릭스루 상태의 드래그가 끊기지 않는다 | yes | no |
| V8 | performance measurement | R17, AC17 | workspace 7 / pane 11 규모에서 브라우저 닫힘 400MB, 열림 900MB 예산을 실제 프로세스 측정으로 만족하고 상태바가 같은 값을 보여준다 | yes | no |
| V9 | remote runtime | R14, AC14, SC4 | mini의 원격 workspace가 사이드바에 뜨고 attach·분할·파일 탐색·뷰어가 동작하며, 원격에서 비지원인 편집과 브라우저가 사유와 함께 표시되고 연결 끊김이 빈 목록으로 위장되지 않는다 | no | yes |
| V10 | browser/runtime | R9, R10, R15, AC9, AC10, AC15 | 탭 배지가 소속 pane과 프로필을 드러내고 미확인 배지가 자동 전환 없이 붙으며, workspace 왕복 후 탭이 같은 URL·프로필로 재로드되고, 에이전트 조작으로는 프로필이 바뀌지 않으며, workspace 삭제 후에도 프로필 로그인이 남는다. 이 확인이 전부 외부 CDP 조작과 herdr API 조회만으로 이루어져 관측 가능성 요구를 함께 증명한다 | yes | no |

부작용과 민감 데이터 경계는 두 행에만 붙는다.

- **V6.** `herdr-ide-verify-` 접두어를 가진 검증 전용 workspace와 worktree의 생성·종료·제거만 허용한다. 사용자의 실제 세션과 체크아웃에는 손대지 않으며, 가드가 접두어를 확인하지 못하면 실행을 거부한다. 검증 로그와 산출물에 브라우저 프로필의 쿠키·토큰·로그인 정보를 남기지 않는다.
- **V9.** mini에서는 읽기와 attach만 한다. 원격에서 파괴적 조작을 실행하지 않는다. SSH 키와 원격 경로를 로그에 남기지 않는다. mini 가용성에 따라 차단 가능하며, 차단되면 그 사유를 기록한다.

### 9.3 Human Verification

- HV1. 골든 시나리오. 하루 작업을 herdr-ide로 시작해 끝내고, 창을 전환하거나 본 것을 다시 타이핑하는 일이 사라졌는지 판정한다. 완료 게이트가 아니라 사람 판정 항목이다.
- HV2. **완료 필수 게이트.** 시각 밀도와 배치가 `docs/design-reference/`의 orca 기준선에 부합하는지 판정한다. 부합하지 않으면 다른 모든 검증이 통과해도 v1 미완료다. 자동 검증이 아니라 사람 판정이 게이트인 이유는 이 비교의 판정 주체가 사람뿐이기 때문이다(HD8).
- HV3. 확인 다이얼로그와 실패 상태의 최종 문구. 무엇을 잃는지 사용자에게 정확히 전달되는지는 사람이 읽어야 안다.
- HV4. grab의 잔여 인터랙션 결정(호버 아웃라인의 시각 형태, 중첩 요소 간 이동, 다중 선택 허용 여부).
- HV5. 공개 저장소 발행 시점과 저장소 이름·가시성(HD6).
- HV6. T3 스파이크가 실패했을 때 W1으로 후퇴할지 다른 경로를 볼지의 판단.

## 10. Risks And Open Decisions

- RISK1. **메모리.** 사용자가 orca를 무게 때문에 지웠다. 이것은 가설이 아니라 확인된 이탈 요인이다. AC17이 이를 인수 조건으로 고정하고 D-55(활성 workspace의 브라우저만 상주)가 예산이 workspace 수와 무관하게 성립하는 근거다. 예산 초과는 v1 미완료다.
- RISK2. **펫 창 게이팅 스파이크.** Electron의 투명·클릭스루·항상 위·드래그 옵션 조합은 문서 확인이며 실행 검증이 없다. T3가 실패하면 W1(pet-app 유지)으로 후퇴하고 D-15의 은퇴가 연기된다. 그 시점에 pet-app이 아직 살아 있어 후퇴 비용은 없다.
- RISK3. **에이전트 간 브라우저 제어(수용).** CDP 엔드포인트가 workspace 단위라 그 workspace의 에이전트가 서로의 탭을 제어할 수 있다. 쿠키·로그인은 프로필 파티션으로 격리되어 세션 탈취는 불가하지만, 에이전트 오작동 시 다른 에이전트가 작업 중인 페이지를 건드릴 수 있다. HD4로 승인받는다.
- RISK4. **편집 유실(수용).** C2 정책에서는 어느 쪽을 고르든 한쪽 변경이 사라진다. 무엇을 잃을지 사용자가 정한다는 것이 완화책이며 자동 병합은 하지 않는다.
- RISK5. **독립 심판을 거치지 않은 결정 3건.** D-53, D-59, D-43은 인터뷰 gap-audit 사이클 2 봉인 이후에 추가·갱신되었고 사이클 3이 사용자 판단으로 중단되었다. 이 PRD의 spec 게이트가 같은 내용을 qa-log와 대조하는 것이 유일한 심판이다. HD7로 확인받는다.
- RISK6. **브라우저 임베딩 API 선택(ASM1).** orca가 쓰는 `<webview>`는 Electron이 권장 API로 두는 경로가 아니다. 이 PRD는 능력을 요구하고 태그를 요구하지 않으며, 구현이 고른 API와 그 이유를 결과 보고에 남긴다. 잘못 고르면 버릴 foundation이 되므로 원칙 8의 적용 지점이다.
- RISK7. **herdr 버전 드리프트.** 공개 배포하면 남의 herdr 버전이 다르다. `protocol` 정수 게이팅으로 조용한 기능 저하 대신 명확한 실패를 택했고(D-46), 그 대가로 herdr가 protocol을 올릴 때마다 herdr-ide가 따라가야 한다.
- OPEN1. `pane.close`의 실동작(프로세스 종료인지 뷰 분리인지)은 구현 착수 시 herdr로 확정한다(ASM2). 확인 문구가 이 답에 달려 있다.
- OPEN2. 우측 패널이 3열에서 터미널 폭을 압박하는지는 실사용에서 판정한다. 인터뷰가 미결로 남긴 항목이며 v1 완료를 막지 않는다.
- OPEN3. 원격 파일 접근의 실제 전송 수단(sftp인지 원격 herdr API인지)은 구현이 고른다(ASM3). 사용자 결정은 경계이지 프로토콜이 아니다.

## 11. Implementation Guardrails

**스코프**

- 3장의 비목표를 승인 없이 v1로 끌어오지 않는다. 특히 쿠키 임포트, diff 코멘트 배치, 파일 조작, 원격 편집, 원격 브라우저는 각각 사용자가 근거를 갖고 뺀 것이다.
- 5장에 없는 구조 변경(새 프로세스 경계, 새 외부 서비스, 새 영속 저장소)을 임의로 만들지 않는다.
- herdr가 이미 소유한 계약을 herdr-ide가 다시 정의하지 않는다. 에이전트 정의는 `server.agent_manifests`를 읽고, 사이드바 레이아웃은 `[ui.sidebar.agents]`를 읽는다.

**engineering 원칙**

- 원칙 1(하위 호환 유지 금지): 이전 PRD의 Swift·libghostty 경로를 흔적으로 남기지 않는다. pet-app을 herdr-ide와 나란히 유지하지 않고 T19에서 은퇴시킨다. herdr-pet의 Rust를 부분 재사용하지 않는다.
- 원칙 2(현재 요구를 충족하는 가장 단순한 구현): 사용자 설정 파일 스키마, 플러그인 매니페스트, 뷰어 확장점을 만들지 않는다. R16의 단일 출처는 코드 안에 머문다. 원격 타겟 목록만 문서화된 예외다.
- 원칙 3(레이어로 성장): T4~T7이 동작하는 셸이 된 뒤에 브라우저와 grab을 얹는다. 세 표면을 동시에 미완성으로 들고 있지 않는다.
- 원칙 4(실패를 명시적으로 드러냄): 조용한 no-op을 만들지 않는다. grab 대상 부재, chromux 부재, protocol 미달, SSH 끊김, 서버 무응답은 전부 사유가 표시되는 상태다. 빈 목록이나 회색 비활성으로 실패를 위장하지 않는다.
- 원칙 5(모듈 경계): herdr 소켓 계층은 UI를 모른다. UI 컴포넌트가 소켓 RPC를 직접 만들지 않는다.
- 원칙 6·7(기존 라이브러리 우선): 터미널 렌더링, 마크다운, 구문 강조, diff는 검증된 라이브러리를 쓴다. 직접 구현하기 전에 이미 들어온 의존성의 문서와 타입을 확인한다.
- 원칙 8(장기 결정): 나중에 버릴 걸 알면서 까는 기반을 만들지 않는다. RISK6의 브라우저 임베딩 API 선택이 이 원칙의 적용 지점이다.
- 원칙 10(프로세스 밖에서 관측 가능): R15는 검증 편의가 아니라 제품 요구다. 이 요구를 만족하지 못하는 기능 설계는 재검토 대상이다.
- 원칙 12(테스트에 값을 매김): UI 배선 유닛 테스트를 쓰지 않는다. 자동 테스트는 이식된 순수 함수와 프로토콜 파싱에 한정하고 나머지는 실제 앱 구동으로 증명한다.
- 원칙 13(실패의 부류를 고침): 에이전트 상태 판정을 문자열 매칭으로 늘리지 않는다. 판정 근거는 herdr가 주는 `agent_status`와 `_new` 토큰이다.

**design 원칙**

- 원칙 3(가장 잦은 행동이 가장 적은 클릭): 막힌 에이전트로 가는 경로, 산출물을 보는 경로, 본 것을 넘기는 경로는 각각 한 번의 조작이어야 한다. 파괴적 동작 확인을 pane 수만큼 반복시키지 않는다.
- 원칙 4(파생 상태를 보여줌): 트리 루트와 pane 경로의 불일치, 미확인 탭, 서버 상태, 원격 연결 상태, 메모리 사용량을 사용자가 계산하지 않게 화면이 알린다.
- 원칙 5(기존 패턴을 따름): `docs/design-reference/`가 기준선이며 화면마다 새 구조를 발명하지 않는다.
- 원칙 6(파괴적 동작 전 결과 고지): pane·workspace·tab 닫기와 worktree 제거는 무엇이 사라지는지 문장으로 말한 뒤에만 진행한다.
- 원칙 7(상태를 시각적으로 인코딩): 프로필과 소속 pane, 미확인, attention, 원격, 비지원 사유를 색·배지·배치로 드러낸다. 설명 문단으로 레이아웃을 대신하지 않되, 의미를 나르는 아이콘에는 라벨이나 툴팁을 붙인다.

**안전**

- 검증이 만드는 workspace와 worktree는 `herdr-ide-verify-` 접두어를 갖는다. 가드가 접두어를 확인하지 못하면 실행을 거부한다. 사용자의 실제 세션과 체크아웃을 검증이 지우지 않는다.
- 브라우저 프로필의 쿠키·토큰·로그인 정보를 로그, 검증 산출물, 결과 보고에 남기지 않는다.
- mini에서는 읽기와 attach만 한다. 원격에서 파괴적 조작을 실행하지 않는다.
- 이 저장소의 `agents/prd/herdr-lightweight-ide/` 이력을 지우지 않는다. 폐기 표기만 한다.

**delivery**

- delivery mode는 local이다. 브랜치 생성, 푸시, PR 개설, CI 실행, 워크트리 격리 실행을 하지 않는다. 공개 저장소 발행은 HD6 승인 뒤에 별도로 한다.

## 12. Implementation Result Report Contract

구현 에이전트는 다음을 보고한다.

- 상태: `Done` / `Partially Done` / `Blocked`.
- 사용자가 보게 되는 변화: 어떤 표면이 생겼고 herdrm이 실패한 세 가지(키맵, 파일·웹 브라우저, 사이드바 커스텀)가 각각 어떻게 뒤집혔는지.
- 주요 모듈·프로세스 경계·데이터 형태: herdr 소켓 계층, 브라우저 서브시스템, 펫 창, 원격 계층의 실제 파일·모듈 구조와 각 책임 경계.
- **브라우저 임베딩 API로 무엇을 골랐고 왜인지(ASM1/RISK6).**
- **`pane.close`의 실동작 확정 결과와 그에 따른 확인 문구(ASM2/OPEN1).**
- **원격 파일 접근의 실제 전송 수단(ASM3/OPEN3).**
- 5장의 승인된 구조를 따랐는지, 벗어났다면 어디서 왜인지.
- T1~T19의 완료 상태. T3 스파이크 결과와 W1 후퇴 여부.
- R1-R19 / AC1-AC19 / V1-V10 / SC1-SC6 커버리지.
- 모드별 검증 증거: 빌드·타입 결과, 이식한 순수 함수 테스트 결과, chromux 구동 기록과 herdr API 대조 결과, 메모리 측정 실측값(workspace·pane 수를 함께), 원격 검증 여부와 차단 사유.
- 추가·갱신한 자동 테스트와 각각이 막는 회귀 위험.
- `agents/rules/`의 규칙 랜딩 결과: 어느 규칙을 TS 경로로 다시 세웠고 어느 것을 폐기했는지(`INV-pet-state-off-main-thread` 처리 포함).
- 이전 PRD 폐기 표기 결과.
- 편차와 그 사유.
- 남은 사람 판정 항목(HV1~HV6)과 각각의 현재 상태. HV2는 완료 게이트이므로 판정 결과를 명시한다.
- 하지 않은 것과 후속 후보(diff 코멘트 배치, 쿠키 임포트 재검토 트리거, 서명·공증, 다른 플랫폼).

delivery mode가 local이므로 브랜치·PR·CI 결과는 보고 대상이 아니다.
