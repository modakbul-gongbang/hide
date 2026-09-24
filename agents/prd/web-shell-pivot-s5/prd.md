---
topic: "웹 셸 S5: Settings와 진단, 보류된 Workspace 관리"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "worktree 삭제와 원격 기기 제어, 에이전트 설정 파일 쓰기를 웹 경계로 옮기므로 기존 대상 검증과 실행 소유권을 보존해야 한다."
source_intake: "current conversation"
created_at: "2026-09-24"
updated_at: "2026-09-24"
---

# PRD: 웹 셸 S5

## Goal

사용자가 Swift 셸로 돌아가지 않고 웹에서 Settings와 진단을 확인하고, 단축키·프로젝트 pin·Workspace purpose·worktree·원격 기기를 관리할 수 있게 한다.
S4 이후 S5의 기존 범위를 완결하며, 후속 Workspace UX 개편의 시작점을 만든다.

## Non-goals

- S6 이후 Main/Project/Workspace 개편, Agents 재구성, Agent/View 영역·드래그 분할은 하지 않는다.
  기존 화면 구조를 유지하고 `workspace-ux-migration`의 후속 단계에서 다룬다.
- Swift 삭제, Electron, Pet·메뉴바·전역 단축키·Usage·실시간 대화 뷰어·원격 파일 뷰어는 제외한다.
  해당 기능은 기존 셸에 남고 삭제는 S10의 별도 승인을 기다린다.
- Memory 관리·Sessions 화면은 S8로 남긴다.
  Background AI 설정을 옮기는 것으로 Memory 동의나 자동 활성화를 대신하지 않는다.
- hided 외부 네트워크 공개, 자격증명 입력·복사, SSH 인증 우회, 프로젝트 등록의 허용 루트 확대는 하지 않는다.
  HOME 밖 등록은 기존 CLI 경로를 유지하고 보안 정책 변경은 별도 승인을 받는다.
- 새 디자인 시스템이나 병렬 설정 저장소를 만들지 않는다.
  engineering 7과 design 5에 따라 기존 Settings 구성·토큰·코어 소유권을 승계한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 범위는 General·Appearance·Agents·Devices·Shortcuts 및 진단, worktree 생성/삭제, 프로젝트 pin, Workspace purpose 편집이다. | 기존 `web-shell-pivot` B11, `web-shell-pivot-s2` D-01/D-08, 개정 `workspace-ux-migration` S5 표. |
| D-02 | 현행 Settings의 섹션별 읽기 행과 명시적 편집 방식을 웹 공유 컨트롤로 옮긴다. Pet 탭은 노출하지 않고 진단은 관련 Settings/연결 상태에서 찾는다. 별도의 전체 UX 재설계는 기각한다. | 기존 `DESIGN.md`, `HideSettings.swift`; 가정: 웹 Settings 진입은 기존 chrome의 아이콘과 키보드 접근으로 제공. |
| D-03 | 브라우저는 설정·대상 상태의 권위가 아니다. 기존 코어 이벤트·스냅샷·영속 상태를 재사용하고 Swift에만 남은 실행은 hided/공유 런타임의 단일 소유 실행 경계로 옮긴다. | `docs/ARCHITECTURE.md`; 조사: worktree 삭제 완료와 생성 후 agent 시작은 현재 Swift 소비자가 맡는다. |
| D-04 | General은 실제 daemon/runtime 연결·버전·상태 위치와 진단을 보여준다. 브라우저에 없는 bundle 정보나 확인하지 못한 설치 상태를 만들어내지 않는다. Appearance는 기존 accent와 11~17의 interface font 설정을 승계한다. | `HideSettings.swift` General/Appearance; design 10. |
| D-05 | Agents는 CLI 설치·로그인 가용성, Background AI provider/model과 선택/실제 활성 상태, hook 진단을 보여준다. 기존 provider 경계와 저장 위치·fallback 정책을 유지한다. | `docs/AI_PROVIDERS.md`, `docs/agent-hooks.md`. |
| D-06 | hook 설치·갱신은 대상 로컬 설정 파일과 보존 범위를 알린 뒤 명시적으로 실행한다. 기존 안정적인 helper 경로 검증을 지키며 번들 밖 개발 helper는 계속 거부한다. 웹 대응을 이유로 임시 빌드 경로를 저장하거나 원격 host 설정을 쓰지 않는다. | 기존 `agent-hooks.md` 설치 소유권; 가정: 사용할 안정적 helper가 없으면 이유와 기존 지원 경로를 표시. |
| D-07 | 기존 8개 pane 명령의 사용자 단축키를 웹에서도 편집·저장·초기화한다. 실제 실행과 도움말은 같은 유효 registry를 읽고 Chrome 예약 chord·명령 간 충돌·잘못된 입력을 거부한다. Swift와 browser host별 유효 바인딩을 분리해 서로의 설정을 망가뜨리지 않는다. | S2 단축키 표와 S5 보류, `PaneShortcutSettings.swift`, `web/src/shortcuts.ts`; 가정: host별 override를 기존 core UI 설정에 둔다. |
| D-08 | 로컬 worktree 생성은 branch/base/시작 agent/선택 purpose를 받고 생성된 정확한 pane에만 agent를 시작한다. 삭제는 기존 core gate·pane 종료 확인·실행 직전 재검사와 non-force Git 경로를 승계한다. | 기존 `WorktreeCreationSheet`, `worktrees.rs`, `GitWorktreeRemover`; 삭제·branch 제거의 기존 확인 범위 유지. |
| D-09 | pin은 등록된 로컬 Project의 영속 설정, purpose는 해당 Workspace의 한 줄 텍스트다. purpose는 40자 초과 안내·80 Unicode scalar 상한·빈 값으로 지우기를 승계한다. | S2 보류; `DESIGN.md` Projects/worktree form. |
| D-10 | Devices에서 이름·SSH alias 등록, 연결 상태·Test·재시도·선택·등록 제거를 제공한다. 인증은 daemon host의 기존 SSH 환경이 맡고 remote/local 대상 경계는 서버에서 검증한다. | S2 보류, 기존 native Devices; 브라우저가 다른 기기여도 daemon host가 설정과 연결의 주체다. |
| D-11 | 작업중·실패·부분 성공·stale을 대상별로 보여주며 실패 후 입력을 보존한다. 중복 실행은 같은 의도로 수렴하고 알 수 없는 결과를 성공 또는 무조건 재실행으로 처리하지 않는다. | engineering 4/10/11, design 9/13. |
| D-12 | 전달은 전용 worktree의 PR과 필수 CI 통과다. 이번 실행은 사용자 지시에 따라 맥미니의 새 pane에서 Claude Opus 5.5의 native `/goal`로 수행하고 `$implement`/Sasu 구현·반복 리뷰 루프를 사용하지 않는다. | 사용자: "맥미니에서 herdr로 pane opus5.5 열어서 ... /goal로 시키고 implement 안시키고 ... 검증 얼마나 오래하는지 보고 비교하려고 S4랑". |
| D-13 | 검증은 변경 영향에 맞춘 테스트·실제 웹 QA와 필수 CI로 수행한다. 마지막 수정은 영향받은 검증만 다시 하며 작은 문서 변경 때문에 전체 리뷰를 반복하지 않는다. 실제 결함은 고치고 미실행·환경 실패는 구분한다. | 사용자 검증 반복 축소 요청; 저장소 `CONTRIBUTING.md`의 필수 gate 유지. |
| D-14 | engineering/design 전체와 process practice를 `654485f96b7764c759662d2c3e9e386ebc221cf6`에서 읽었다. 7/11/14/15는 재사용·중복 방지·소유 종료·상한으로, design 5/6/9/12/13은 기존 구조·삭제 확인·국소 상태·가독성으로 반영한다. 새 구조가 필요할 때만 design 11의 사용자 선택을 요청한다. | `sasu principles list`; 범용 구현 제약은 저장소 규칙에 유지하며 제품 요구를 중복 생성하지 않는다. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 작업 화면에서 Settings를 열어 General/Appearance/Agents/Devices/Shortcuts를 전환하고 닫으면 원래 작업과 키보드 포커스로 돌아온다. 시트를 열고 닫는 것으로 pane·tab·draft가 사라지지 않는다. | D-01, D-02 |
| B2 | General에서 실제 daemon/runtime 상태·가용한 버전과 상태 위치를 읽는다. 미확인 값은 unavailable로 구분하고 복사 진단에는 토큰·자격증명·터미널 내용·사용자 prompt가 들어가지 않는다. | D-04, D-11 |
| B3 | 연결 대기·단절·프로토콜 불일치는 원인에 맞는 기존 복구 안내와 진단 접근을 제공한다. 진단 보기/복사는 서버 재시작·pane 생성·사용자 앱 종료를 일으키지 않는다. | D-04, D-11 |
| B4 | accent와 interface font를 바꾸면 웹에 반영되고 새로고침 뒤 설정이 복원된다. agent lifecycle 색의 의미와 별도 terminal/editor text scale은 유지된다. 저장 실패는 해당 설정에 표시한다. | D-03, D-04 |
| B5 | Agents는 CLI 없음·로그인 필요·가용·조회 실패를 구분한다. provider/model은 기존 backend가 공급한 값과 현재 저장된 선택을 표시하며 누락된 모델을 다른 값으로 조용히 바꾸지 않는다. | D-05 |
| B6 | Background AI provider/model 변경은 기존 영속 설정에 반영되고 다음 소비 요청부터 사용된다. selected/active/degraded가 다르면 구분되며 쓰기 실패가 보이고, API key 입력이나 Memory 활성화는 일어나지 않는다. | D-03, D-05 |
| B7 | hook 진단에서 미설치·구버전·세션 선행·config 읽기 실패·보고 실패를 구분한다. 설치/갱신은 로컬 파일과 다른 설정 보존을 확인한 뒤 한 번 실행되고 실제 읽어온 결과를 보인다. 안정적 helper가 없거나 설정을 읽지 못하면 쓰지 않고 이유를 보인다. | D-06, D-11 |
| B8 | Agents 관찰을 끝내거나 클라이언트가 끊기면 그 클라이언트의 관찰 수요가 해제된다. 화면 밖 capability probe가 계속 돌거나 여러 탭이 같은 조회/설치를 중복 기동하지 않는다. | D-03, D-05, D-14 |
| B9 | Shortcuts는 명령·기본값·실제 유효 chord를 보여주고 8개 pane 명령을 편집·적용·기본값 복원할 수 있다. 저장 후 도움말과 실제 키 동작이 일치하고 새로고침 뒤 유지된다. | D-07 |
| B10 | 잘못된 chord·충돌·Chrome 예약 조합은 적용 전에 같은 행에서 거부되어 기존 바인딩이 유지된다. IME 조합과 일반 입력은 shortcut 녹화나 명령 실행으로 유출되지 않고 browser 설정이 Swift 바인딩을 덮어쓰지 않는다. | D-07 |
| B11 | 등록된 로컬 Project 메뉴에서 Pin/Unpin하면 기존 Pinned 목록과 정렬에 즉시 반영되고 재시작 뒤 유지된다. 미등록·원격 대상에는 지원하지 않는 pin 액션을 제공하지 않는다. | D-09 |
| B12 | Workspace 메뉴의 purpose 편집은 현재 값을 보여주며 한글을 포함한 한 줄 80자 상한과 40자 안내를 유지한다. 저장/지우기 후 실제 결과를 표시하고 실패 시 입력과 재시도 수단을 보존하며 다른 Workspace에 기록하지 않는다. | D-09, D-11 |
| B13 | 로컬 Git 프로젝트에서 branch·base·Terminal only/지원 agent·선택 purpose로 worktree를 만든다. 유효하지 않은 branch나 중복 경로는 원본을 바꾸지 않고 거부하며 성공하면 생성된 checkout/pane으로 이동한다. | D-08 |
| B14 | 선택 agent 시작이 실패해도 이미 생성된 worktree와 pane을 성공처럼 숨기거나 다시 생성하지 않는다. 생성 결과와 agent 시작 실패를 구분하고 정확한 기존 pane에서 복구할 수 있다. | D-08, D-11 |
| B15 | 삭제 확인은 정확한 worktree·pane/작업 중단 영향·복구 불가능한 폴더 제거와 선택 branch 제거를 설명한다. 취소는 아무것도 바꾸지 않으며 branch 삭제는 기본 해제되고 기존 merged/named gate가 허용할 때만 선택된다. | D-08 |
| B16 | 삭제는 remote/main/base/dirty/nested/상태 불명 등 기존 거부 조건을 유지하고 unmerged/unpushed/live-agent 경고를 생략하지 않는다. 확인 뒤 pane 종료와 현재 경로·등록·HEAD·branch·dirt를 재검사해 바뀐 대상을 삭제하지 않으며 force 옵션을 쓰지 않는다. | D-08, D-11 |
| B17 | 삭제의 timeout/실패는 worktree가 남았는지와 이미 닫힌 pane을 구분하며 실패를 성공으로 확정하지 않는다. 재요청은 현재 상태에서 계속하고 한 요청의 실행은 한 owner만 수행한다. 웹 요청만으로 완료 이벤트를 위조해 미실행 작업을 성공시킬 수 없다. | D-03, D-08, D-11 |
| B18 | Devices는 실제 등록 목록과 로컬/원격·연결 상태를 보여준다. 이름과 SSH alias를 등록하면 해당 host의 연결을 시도하고 Test/재시도는 실제 새 시도의 결과를 보여준다. 빈 목록·pending·실패·stale 상태를 구분한다. | D-10, D-11 |
| B19 | 기기 선택 후 pane 제어는 선택된 host의 정확한 대상에만 전달되고 연결 실패가 로컬 대상으로 fallback하지 않는다. 원격 파일/로컬 전용 관리 액션은 지원 범위 밖임을 보이고 실행하지 않는다. | D-03, D-10 |
| B20 | 원격 기기 제거는 연결/등록만 정리하고 그 기기의 파일·Herdr 서버·agent 작업을 종료하지 않는다. local 제거는 거부하며 SSH 암호·키 입력이나 자동 host 신뢰 우회는 제공하지 않는다. | D-10 |
| B21 | 새로고침·재접속 뒤 저장된 설정/등록은 core snapshot에서 복원된다. 진행중 mutation을 단순 재접속으로 재실행하지 않고 화면의 stale 결과로 다른 대상의 완료를 표시하지 않는다. | D-03, D-11 |
| B22 | 좁은 창과 긴 한국어/영문 이름에서도 섹션·값·오류·확인 버튼을 읽고 키보드로 사용할 수 있다. Esc/취소·focus 복귀·명명된 컨트롤과 text/symbol 상태를 제공하고 실제 토큰과 공유 컨트롤을 쓴다. | D-02, D-14 |
| B23 | Settings와 새 관리 기능은 토큰·Origin·schema·등록 checkout 경계 및 HOME 등록 제한을 우회하지 않는다. 사용자 선택 경로를 임의 명령 문자열로 실행하지 않고 server가 실제 대상과 권한을 다시 확인한다. | D-03, D-06, D-08, D-10 |
| B24 | Settings 표시/hover/타이핑만으로 Git·SSH·provider 프로세스가 반복 생성되지 않는다. 변경만 통지하고 실제 작업은 mutex 밖에서 기존 bounded worker로 실행하며 닫기/종료 시 소유 리소스를 해제한다. | D-03, D-11, D-14 |

## Technical structure

기존 React → 인증된 hided WebSocket → core의 상태/typed event 경계를 유지하고 Settings 및 Devices의 필요한 snapshot 계약을 확장한다.
Swift만 소비하던 worktree 실행과 생성 후 agent 시작은 daemon/공유 runtime의 대상 검증된 비동기 작업으로 옮기되 Swift 공존 경로와 단일 실행 소유권을 유지한다.
기존 UI state·device registry·AI 설정을 재사용하고 browser shortcut override만 host별로 분리한다.
hook 쓰기는 `hide-agent-hooks`의 기존 안정적 helper·명시 동의·보존 경계를 유지하며 새 원격 설정 쓰기 경로를 만들지 않는다.

## Risks

- 삭제와 host routing은 화면만으로 검증할 수 없다.
  격리 fixture의 실제 생성/삭제·실패·재접속과 server의 잘못된 대상 거부를 검사하고 운영자의 pane·저장소·설정 파일은 시험 대상에서 제외한다.
- macOS bundled helper가 없는 hided 개발 환경에서는 hook 설치가 거부될 수 있다.
  이 제한을 숨기거나 임시 helper를 설치하지 않으며 다른 설치 소유권이 필요하면 별도 승인 사안이다.
- UI는 기존 Settings 구조를 이식하며 신규 구조/토큰은 확정한 것처럼 만들지 않는다.
  새로운 인간 판단이 필요한 선택은 작업을 넓히지 않고 명시한다.
- 본 문서는 기존 단계 계약과 현재 대화에서 작성했으며 별도 S5 interview qa-log가 없어 spec gate는 no-source 규칙으로 생략한다.
  문서 승인 상태는 pending이고 위 사용자 지시가 이 범위의 실행 권한이다.
- 구현·검증·재작업·CI·환경 준비 시간을 별도로 기록한다.
  S4와 기기·범위·초기 캐시·실행 방식이 달라 속도 차이를 모델 하나의 효과로 단정하지 않는다.
- 사용자에게 현재 필요한 작업은 없다.
  인증 갱신이나 새 보안 정책 같은 실제 외부 권한이 필요하면 구체적인 차단 원인을 알린다.
