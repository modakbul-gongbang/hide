---
topic: "hide 리브랜딩·패키징·사이드바 재설계"
status: "ready"
human_approval: "approved"
review_profile: "high-risk"
review_rationale: "비공개 코드를 public GitHub 레포로 공개하는 비가역 외부 행위와 --dangerously-skip-permissions 실행 경로, SSH 원격 제어·타사 바이너리 번들이 포함된다."
source_intake: "agents/interview/hide-rebrand/qa-log.md"
created_at: "2026-08-29"
updated_at: "2026-08-30"
---

# PRD: hide 리브랜딩·패키징·사이드바 재설계

## 1. Summary

herdr-ide macOS 앱을 `hide`(bundle id `me.grab.hide`)로 리브랜딩하고 정식 번들로 패키징해, Spotlight에서 검색·실행되고 herdr 미설치 외부 사용자도 `modakbul-gongbang/hide` public GitHub Releases의 zip 하나로 쓸 수 있게 한다.
좌측 패널을 [New Workspace / New Agent / Search 상단 액션 + WORKSPACES(repo→체크아웃 트리) + AGENTS(에이전트 pane 감시 목록) + 하단 디바이스·설정]으로 재설계하고, 체크아웃 선택 시 탭·pane 그리드와 우측 파일트리가 함께 전환되는 풀 컨텍스트 전환 모델로 바꾼다.
원격 디바이스(임의 SSH 호스트, 기본 사례 mini)도 로컬과 동일한 UX로 동작하도록 `src/remote.rs`를 포팅하고, 다크 전용 디자인 토큰과 Raycast DESIGN.md 기준으로 전 화면을 리팩토링한다.

Approval checklist:

- 스코프 경계와 non-goal 목록 (3장): 공증 없음·내부 리네임 없음·gemini/cursor 없음·Intel/Windows 없음·라이트 테마 없음.
- 주요 구조 변경 (5장): herdr-core 멀티 워크스페이스·디바이스 확장, remote.rs 포팅과 retired `src/` 크레이트 삭제, herdr 바이너리 번들, GitHub Actions 릴리스 파이프라인.
- 공개 배포 (R10, 9.3): 코드가 `modakbul-gongbang/hide` **public** 레포로 올라간다. 공개 push 직전 시크릿 스캔 후 사용자 최종 승인이 게이트다.
- 검증 모드 (9.1): 자동 테스트 + 실행 앱 스크린샷 + **mini 실기기 라이브 원격 검증**(전용 테스트 워크스페이스만 생성·정리, 기존 세션 불간섭) + 릴리스 파이프라인 라이브 검증.
- 보안 결정 (R6, R7): Bypass permissions 토글 기본 OFF + 상시 경고, SSH 무자격증명 모델(저장·입력 UI 없음).
- 인간 검증 (9.3): 앱 아이콘 후보 선택, 다크 디자인 최종 taste, 공개 push 승인, draft 릴리스 publish.
- delivery mode: local (agents/config.json 기본값 - PR 없이 로컬 브랜치에서 완결하고, GitHub 레포 생성·push는 배포 작업(T11)의 산출물로 취급).

## 2. Problem, Goal, And Users

herdr-ide는 현재 개발자 본인만 쓸 수 있는 dev 빌드다.
ad-hoc 서명 debug 바이너리가 `macos/build/assembled/`에 조립될 뿐이라 Spotlight에 뜨지 않고, 외부인에게 주면 Gatekeeper에 막히며, 앱의 뼈대인 herdr 데몬이 없는 사람에게는 빈 껍데기다.
UI도 단일 워크스페이스에 묶인 3패널 구조라, 여러 프로젝트·워크트리·에이전트를 오가는 실사용 흐름(herdr-label 사이드바가 이미 증명한 흐름)을 담지 못한다.

목표: hide를 "받아서 바로 쓰는" 완성 앱으로 만든다.
사용자는 두 부류다.
첫째, 본인(및 팀): 여러 repo·worktree·디바이스에서 도는 claude/codex 에이전트를 한 창에서 감시·조작한다.
둘째, herdr를 모르는 외부 사용자: zip을 받아 설치 안내대로 열면 번들 herdr로 곧바로 동작한다.

### 2.1 User Scenarios

- SC1. 체크아웃 전환: 사용자가 사이드바 WORKSPACES에서 다른 체크아웃을 선택해 작업 컨텍스트를 통째로 바꾼다.
  Actors: hide 사용자.
  Primary path: WORKSPACES에서 repo 그룹 아래 main 또는 ⑂worktree 행을 클릭하면 메인 영역이 그 체크아웃의 탭 스트립+pane 그리드로 전환되고 우측 파일트리 루트가 그 체크아웃 경로로 바뀐다. 상단 [+]로 새 탭을 추가할 수 있다.
  Failure state: 열린 탭이 없는 체크아웃은 새 탭/터미널 시작을 유도하는 빈 상태를 보여준다. worktree는 마지막 pane이 닫히면 목록에서 사라지고, 그 worktree를 보던 중이면 부모 repo의 기본 체크아웃으로 복귀한다.
  Recovery: herdr 연결이 끊기면 사이드바에 연결 상태 문구(소켓 미설정/서버 불가/연결됨-빈 상태 구분)가 표시된다.
  Reach: 로컬에 git repo 워크스페이스 2개(하나는 worktree 포함)를 등록하고 각 체크아웃에 pane을 연 상태에서 시작한다. 준비는 태스크가 처리한다.
- SC2. Agents 감시 목록: 어느 체크아웃에서 돌든 에이전트 pane이 사이드바에 항상 보인다.
  Actors: hide 사용자.
  Primary path: claude·codex 에이전트 pane이 herdr-label과 동일한 순서·상태로 AGENTS 섹션에 뜬다(에이전트 favicon + 이름 + 상태 심볼/경과 + 폴더 라벨). 행을 클릭하면 해당 워크스페이스→체크아웃→탭으로 점프하고 그 pane에 포커스가 간다.
  Failure state: 일반 터미널 pane은 목록에 나타나지 않는다. 에이전트 종료 시 done 상태로 표기되고 sortRank 규칙대로 정렬된다.
  Recovery: 점프 대상 pane이 이미 닫혔으면 해당 행이 제거된다.
  Reach: 한 탭에 claude+codex+일반 터미널 2개, 다른 탭에 터미널 1개를 배치한 상태를 만든다.
- SC3. New Agent 시작: 모달에서 디바이스·에이전트·위치를 골라 에이전트를 실행한다.
  Actors: hide 사용자.
  Primary path: 상단 New Agent 클릭 → 모달에서 Device(Local/등록 원격) → Agent 타일(claude/codex) → Space(그 디바이스의 워크스페이스+체크아웃 선택기) → Bypass permissions 토글(기본 OFF) → Start. 대상 체크아웃의 활성 탭에 pane이 생기고(탭이 없으면 새 탭 자동 생성) AGENTS 섹션에 나타난다. 토글을 켜면 `--dangerously-skip-permissions`의 위험 문구가 토글 아래 상시 노출된다.
  Failure state: 선택 디바이스가 연결 불가면 Start가 비활성화되고 원인 문구가 보인다. CLI가 미설치면 해당 타일이 '설치 필요' 상태와 설치 링크를 보여준다. CLI 실행 실패는 pane 출력에 그대로 노출된다.
  Recovery: Cancel로 무변경 종료.
  Reach: claude·codex CLI가 설치된 로컬과, 탭이 없는 빈 체크아웃 하나를 준비한다.
- SC4. New Workspace 추가와 Remove: 폴더를 워크스페이스로 등록하고 해제한다.
  Actors: hide 사용자.
  Primary path: New Workspace → Finder 폴더 선택 → git repo면 즉시 WORKSPACES에 추가된다. git이 없으면 'Initialize git repository' 체크박스(기본 ON)가 보이는 확인 단계를 거친다. 등록 목록은 재시작 후에도 유지된다.
  Failure state: 체크박스를 끄고 추가하면 worktree 계층 없는 평평한 행이 된다. 이미 등록된 폴더는 기존 행 선택으로 처리된다. git init 실패 시 폴더는 non-git 워크스페이스로 추가되고 실패 원인이 알림으로 표시된다. 자동(무언) git init은 절대 없다.
  Recovery: Remove는 등록 해제만 하며 디스크 파일을 삭제하지 않는다. 확인 다이얼로그에 "파일은 삭제되지 않습니다"가 명시되고, 그 폴더에서 pane이 돌고 있으면 자동 발견으로 임시 항목으로 계속 표시된다.
  Reach: git 없는 임시 폴더와 git repo 폴더를 하나씩 준비한다.
- SC5. Search 팔레트: 에이전트·스페이스를 통합 검색해 바로 이동한다.
  Actors: hide 사용자.
  Primary path: 상단 Search → 오버레이 팔레트에 에이전트+스페이스(워크스페이스/체크아웃) 통합 목록. 타이핑으로 필터하고 행에는 종류·워크스페이스·디바이스 뱃지가 보인다. ↑↓ 이동 후 ⏎: 에이전트 행이면 해당 pane으로 점프, 스페이스 행이면 그 체크아웃으로 전환.
  Failure state: 매칭 없음 상태가 표시된다. 일반 터미널은 검색 대상이 아니다.
  Recovery: esc로 무변경 닫기.
  Reach: 로컬·원격에 걸쳐 에이전트와 워크스페이스가 여러 개 있는 상태에서 실행한다.
- SC6. 원격 디바이스 전환: 하단 디바이스에서 원격 호스트를 선택해 로컬과 동일하게 작업한다.
  Actors: hide 사용자, 원격 호스트(mini)의 herdr 데몬.
  Primary path: 하단 디바이스 목록(연결 상태 점 포함)에서 mini 선택 → WORKSPACES가 mini의 워크스페이스 목록으로 전환되고, 체크아웃 선택 시 원격 pane·파일트리가 로컬과 동일 UX로 동작한다.
  Failure state: SSH 불가·인증 실패·미신뢰 host key는 원인별 문구로 명시적으로 실패한다(자격증명 입력·저장 UI는 없다). 원격에 herdr가 없으면 공식 설치 원라이너를, 버전 미달이면 업그레이드 명령을 안내만 한다(자동 설치·변경 없음).
  Recovery: 연결이 끊기면 원격 컨텍스트에 머물며 '연결 끊김' 상태와 재연결 시도가 표시되고, 로컬 전환은 사용자가 직접 한다(자동 복귀 없음). 재연결 시 기존 pane에 재attach한다(출력 이력은 herdr 보존 범위만큼). 세션이 이미 소멸했으면 행을 제거하고 "원격에서 세션이 종료되었습니다"를 안내한 뒤 목록을 재동기화하며, 복구를 시도하지 않는다.
  Reach: mini에 SSH 키 인증이 이미 구성되어 있고 herdr가 설치되어 있다. 검증용 워크스페이스·pane은 mini에 전용으로 생성했다가 정리한다.
- SC7. 외부 사용자 설치: herdr를 모르는 사람이 zip 하나로 hide를 쓴다.
  Actors: 외부 사용자.
  Primary path: GitHub Releases에서 zip 다운로드 → 압축 해제 → hide.app을 /Applications로 이동 → 동봉 안내대로 우클릭 열기 → 실행. Spotlight에서 "hide"로 검색·실행된다. herdr가 없으니 번들 herdr로 데몬이 자동 기동된다.
  Failure state: 더블클릭 시 Gatekeeper "확인 불가" 경고가 뜬다. 동봉 README의 우클릭 열기 또는 xattr 한 줄로 해소된다. 살아있는 herdr 소켓이 있으면 그 데몬이, 설치된 herdr가 있으면 설치본이 번들보다 우선한다. 설치본 버전이 최소 미달이면 번들로 기동하고 업그레이드를 안내한다.
  Recovery: 실패해도 앱·데이터 손상이 없다.
  Reach: 격리 속성(quarantine)이 있는 zip과, PATH·표준 설치 경로에서 herdr가 보이지 않는 격리 환경을 준비한다.
- SC8. 설정: 단축키·herdr 상태·테마·에이전트 기본값·디바이스를 한 창에서 관리한다.
  Actors: hide 사용자.
  Primary path: 하단 설정 아이콘 → 설정 창에 5개 섹션(pane 단축키, herdr 연결 상태/버전, 테마: 액센트 컬러+폰트 크기, 에이전트 기본 옵션: bypass 기본값, 디바이스: SSH 호스트 관리). 테마 변경은 즉시 적용·자동 저장되고 Reset to defaults로 복원된다. 디바이스 추가는 이름+SSH 호스트(~/.ssh/config alias) 입력→연결 테스트→저장이며 사이드바 하단에 반영된다.
  Failure state: 연결 테스트 실패는 원인(SSH 불가/원격 herdr 없음)을 구분해 표시하되 저장은 허용한다. bypass 기본값을 ON으로 바꾸는 순간에도 동일 경고가 노출된다.
  Recovery: 디바이스 삭제는 로컬 등록 해제(detach)만이며, 확인 다이얼로그에 "실행 중인 에이전트 N개는 원격에서 계속 실행되며 재추가 시 재연결됩니다"가 명시된다. 재추가로 되돌릴 수 있다.
  Reach: mini가 디바이스로 등록되어 있고 그 위에 에이전트 pane이 하나 도는 상태에서 삭제·재추가를 검증한다.

## 3. Scope And Non-Goals

포함:

- 브랜딩·번들: 표시명 `hide`, bundle id `me.grab.hide`, ima2 후보에서 선택한 앱 아이콘(.icns), release 빌드 정식 번들, /Applications 설치로 Spotlight 노출.
- herdr 확보 체인: ①살아있는 소켓 ②설치본(`~/.local/bin` → `/opt/homebrew/bin` → `/usr/local/bin`) ③번들 herdr. 번들은 hide 릴리스마다 당시 최신으로 갱신되는 빌드 시점 고정(현재 v0.8.2, SHA-256 기록)이고 최소 지원 버전은 그 릴리스의 번들 버전이다. Apache-2.0 고지 동봉.
- 사이드바 재설계: WORKSPACES(repo 그룹→체크아웃 행), AGENTS(에이전트 pane 감시 목록), 하단 디바이스+설정, 상단 New Workspace/New Agent/Search.
- 풀 컨텍스트 전환: 체크아웃 선택 시 탭 스트립·pane 그리드·파일트리가 함께 전환. 탭 추가 UI.
- 워크스페이스 수명: 영속 등록 목록 + 미등록 폴더 pane 자동 발견(임시 표시). worktree 행은 순수 pane 기반 자동.
- 원격: 임의 SSH 호스트(macOS·Linux, arm64/x86_64) 풀 컨텍스트 전환, `src/remote.rs` 포팅, SSH 무자격증명 모델, 재연결·소멸 세션 계약.
- 설정 창 5개 섹션(단축키·herdr 상태·테마 액센트/폰트·에이전트 기본값·디바이스 관리).
- 디자인: 다크 전용 토큰 단일 정의 + 전 화면 적용, Raycast DESIGN.md(MIT 고지) 루트 추가 + AGENTS.md 참조.
- 배포: `modakbul-gongbang/hide` public 레포 신설, `v*` 태그 → GitHub Actions 빌드·zip·SHA-256 → draft 릴리스(publish는 사용자 수동), 공개 push 전 시크릿 스캔 + 사용자 최종 승인, zip 동봉 설치 안내.

Non-goals (각각 사용자 결정, 사용자 결과와 revisit 조건 포함):

- Developer ID 서명·공증: 비용(연 $99) 사유로 제외. 외부 사용자는 우클릭 열기/xattr 안내를 거친다. revisit: Apple Developer 계정 발급 시 (D-05).
- 내부 코드명 리네임(`Herdr*` 타입, `herdr-core` 크레이트, 로컬 디렉토리명): diff 대비 이득 없음. revisit: 다음 대규모 리팩토링 시 (D-11, D-26).
- gemini/cursor 에이전트 타일: 미검증 실행 경험을 외부 사용자에게 노출하지 않는다. revisit: 사용자 요청 시 (D-14, D-26).
- Intel(x86_64) mac 지원: 현 빌드가 arm64 전용. macOS 14+ / Apple Silicon만 지원한다 (D-33).
- Windows 원격 호스트: herdr SSH 원격 지원 매트릭스에서 제외 (D-41).
- 라이트 테마·토큰 전체 편집 UI: 다크 전용, 설정의 테마 조정은 액센트+폰트 크기만. revisit: 후속 요청 시 (D-15, D-31).
- 파일 내용 검색: Search는 에이전트+스페이스 탐색 전용 (D-30).
- 원격 호스트 herdr 자동 설치·자동 업그레이드: 안내만 한다 (D-34, D-37).
- hide 자체 세션 이력 저장: 상태의 주인은 herdr 데몬 (D-40).
- 기존 HerdrIDE dev 설치의 마이그레이션: 기존 설치는 개발자 본인 dev 빌드뿐이고 이관할 영속 데이터가 없다(UI 상태 저장이 /tmp 검증용 경로 기본). hide는 자체 영속 경로를 새로 정의하고, 옛 HerdrIDE.app은 수동 삭제한다. herdr 데몬 소유 상태는 앱 교체와 무관하게 유지된다 (D-47).
- PR 기반 개발 delivery: agents/config.json이 local 모드이고 사용자가 PR 워크플로를 요청하지 않았다. GitHub 레포·릴리스는 제품 배포 산출물(T11)이다.

### Product Completeness Contract

이 PRD는 축소 MVP가 아니라 외부 배포 가능한 완성 제품을 계약한다.
주 여정(설치→첫 실행→워크스페이스 등록→에이전트 실행→감시→원격 전환)과 빈 상태·실패·복구·보안·라이선스·배포 경계를 위 SC 카드와 R/AC가 전부 커버하며, 모든 의도적 축소는 위 non-goal에 revisit 조건과 함께 기록되어 있다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
필요한 자격은 이미 갖춰져 있다: gh CLI가 yansfil(modakbul-gongbang admin)로 인증되어 있고, mini SSH 키 인증이 ~/.ssh/config에 구성되어 있으며, ima2·herdr 릴리스 바이너리는 에이전트가 직접 접근할 수 있다.

### 4.2 Human Decisions Before PRD Approval

- 코드 공개 재확인: 이 저장소 전체가 `modakbul-gongbang/hide` **public** 레포로 올라간다는 것을 승인해 달라. 시크릿 스캔을 통과해도 코드·커밋 이력 자체가 공개된다 (실제 push 직전에 9.3에서 한 번 더 최종 승인을 받는다).
- mini 라이브 검증 경계 승인: 원격 검증은 실기기 mini에 전용 테스트 워크스페이스·pane을 생성했다가 정리하는 방식으로 진행한다. 기존 운영 세션(Hermes 게이트웨이 등)은 읽기만 하고 건드리지 않는다.
- Bypass permissions 노출 승인: 외부 배포물에 `--dangerously-skip-permissions` 실행 경로가 (기본 OFF + 상시 경고와 함께) 포함된다.
- delivery mode local 확인: PR 없이 로컬에서 완결하고, 레포 push는 배포 태스크의 결과물로만 발생한다.

### 4.3 Decision Traceability For Fidelity Review

Decision Register 전체(D-01~D-43)의 PRD 배치:

- D-01(fact, 현 빌드 ad-hoc 서명) → 문제 정의(2장)와 R1의 근거. context.
- D-02(fact, 단일 워크스페이스·3패널) → 5장 구조 변경의 출발점. context.
- D-03(fact, SidebarAgent 필드 존재) → R5 구현 기반, 가드레일 "기존 확장 우선". context.
- D-04(fact, mini 워크스페이스 조회 코드 존재) → R7 구현 기반. context.
- D-05(미서명 zip 배포, 공증은 후속) → R1, R10, AC17, non-goal(공증).
- D-06(워크스페이스=repo, 워크트리=체크아웃, pane 기반 자동 조립) → R3, AC5, SC1.
- D-07(체크아웃 선택=풀 컨텍스트 전환, 탭 추가 UI) → R4, AC6, SC1.
- D-08(Agents=에이전트 pane만, herdr-label 동일 상태·순서, favicon, 폴더, 점프) → R5, AC7, SC2.
- D-09(자동 git init 금지, 다이얼로그 체크박스 기본 ON, 컨텍스트 메뉴 init) → R6, AC8, 가드레일.
- D-10+D-24(풀 원격 컨텍스트 전환, 임의 SSH 호스트, remote.rs 포팅) → R7, AC11, SC6.
- D-11(표시명 hide, me.grab.hide, 내부 코드명·디렉토리명 유지, 원격 레포명 hide) → R1, AC1, non-goal(내부 리네임).
- D-12(아이콘 ima2 후보→사용자 선택) → T8, 9.3 인간 검증.
- D-13(상단 액션 3종, New Agent 모달·Search 팔레트 스크린샷 #2·#3 기준) → R6, AC9-AC10, SC3·SC5.
- D-14(claude+codex만) → R6, non-goal(gemini/cursor).
- D-15(다크 전용 토큰, DESIGN.md+AGENTS.md 참조) → R9, AC15, T1.
- D-16(assumption, 에이전트 favicon 정적 번들 - agent default 유지) → R5, T9. 여전히 assumption이다.
- D-17+D-37(herdr 확보 체인 3단계, 릴리스별 갱신되는 번들 고정 v0.8.2+SHA-256, 최소 버전=번들, 미달 시 번들 기동+안내) → R2, AC2-AC3, SC7.
- D-18(설정 5개 섹션, mini 하드코딩 제거) → R8, AC14, SC8.
- D-19(modakbul-gongbang/hide public+Releases, 시크릿 스캔+최종 승인 게이트, 로컬 디렉토리명 유지) → R10, AC16, 9.3.
- D-20(수명 기본값: 마지막 선택 복원·첫 실행 빈 상태·PetWindow 유지) → R11, AC18.
- D-21(bypass 기본 OFF, 상시 경고, 에이전트 1개 범위, 설정 기본값 변경 시 동일 경고) → R6·R8, AC9·AC14.
- D-22(디바이스 삭제=detach, 다이얼로그 문구) → R7, AC13, SC8.
- D-23(영속 등록+자동 발견, worktree만 순수 pane 자동) → R3, AC4.
- D-25(Space=워크스페이스+체크아웃 선택기, 빈 탭이면 새 탭 자동 생성) → R6, AC9.
- D-26(연기 항목 revisit 조건) → non-goal 목록.
- D-27(v* 태그→Actions→zip+SHA-256→draft, publish 수동, DESIGN.md MIT·로고 출처 표기 승인) → R10, AC16·AC20.
- D-28(SSH 무자격증명 모델, known_hosts 정책, 원인별 명시 실패) → R7, AC11, 가드레일.
- D-29(git init 실패→non-git 추가+원인 알림) → R6, AC8.
- D-30(Search 대상=에이전트+스페이스, Enter 동작) → R6, AC10.
- D-31(테마 조정=액센트+폰트만, 즉시 적용·자동 저장·Reset) → R8, AC14.
- D-32(실제 기본 브랜치명 라벨, main 하드코딩 금지) → R3, AC5.
- D-33(macOS 14+/arm64만) → non-goal(Intel), R1.
- D-34(원격 herdr 미설치 시 안내만) → R7, AC11.
- D-35(CLI 탐지, 미설치 타일 상태+링크, 인증은 CLI 위임) → R6, AC9.
- D-36(로고는 공식 브랜드 가이드라인 확인 후 사용, 불허 시 글리프 대체) → T9, AC20.
- D-38(CLI 버전 하한 없음, 로그인 셸 환경 상속) → R6, 가드레일.
- D-39(Workspace Remove=등록 해제만, 파일 미삭제 문구) → R3, AC13, SC4.
- D-40+D-42(재연결=herdr가 상태 주인·재attach, 소멸 세션은 제거·안내·재동기화) → R7, AC12, SC6.
- D-41(원격 플랫폼 macOS·Linux, 공식 설치 원라이너, Windows non-goal) → R7, AC11, non-goal.
- D-43(업그레이드 안내=설치 원라이너 재실행, brew 병기, 로컬·원격 동일) → R2, AC3.

거부·기각된 대안 (기각 상태 유지):

- 공증 배포(Q1-a): 비용 사유로 기각, 후속.
- 새 창 열기/파일트리만 전환(Q2-b·c): 풀 컨텍스트 전환에 밀려 기각.
- 자동(무언) git init(Q3 사용자 제안): 부작용 사유로 사용자 동의 하에 기각, 다이얼로그 명시 체크박스로 대체.
- 미니멀 설정(Q12-a): 본격 설정(b)에 밀려 기각.
- 원격 목록만(Q5-b·c): 풀 전환(a)에 밀려 기각.
- 4타일 노출(Q9-b·c): claude+codex 2타일에 밀려 기각.
- 라이트 병행/시스템 연동 테마(Q10-b·c): 다크 전용에 밀려 기각.
- 수동 zip 전달(Q13-a): GitHub Releases에 밀려 기각.
- herdrdev org(Q14): yansfil이 비멤버라 불가 판명. modakbul-gongbang으로 확정.
- 상단 New Terminal 액션(Q8 초기안): 사용자가 New Workspace로 대체.

Spec gate에서 확정된 사용자 결정 3건:

- 원격 연결 끊김 시 원격 컨텍스트에 머물며 끊김 상태·재연결 시도 표시, 로컬 전환은 수동 (사용자가 (b) 선택 - qa-log UX-06 카드의 "로컬로 안전 복귀" 문구를 이 결정으로 대체) → R7, AC12, SC6.
- 원격 라이브 검증은 mini(macOS arm64)로 한정, Linux는 지원 선언하되 이번 릴리스 실기기 미검증을 리스크로 명시 → V4, 10장.
- AC15의 "전 화면"을 유한 화면 인벤토리로 확정 → AC15.

gap-audit 사이클4에서 확정된 사용자 결정 2건:

- D-46(bypass 토글의 에이전트별 플래그 매핑: claude `--dangerously-skip-permissions`, codex `--dangerously-bypass-approvals-and-sandbox`, 공통 경고 문구) → R6, AC9.
- D-47(기존 HerdrIDE 설치 마이그레이션 없음, hide 자체 영속 경로 신설, 옛 dev 앱 수동 삭제) → non-goal, R11.
- D-44·D-45도 qa-log Decision Register에 정식 등록됨(위 spec gate 결정 3건과 동일 내용).

Principles intake: `~/projects/oh-my-principle` (commit 35ab76ca23d45e714f1630054855a8c8c4568d03)의 engineering/principles.md와 design/principles.md를 전문으로 읽었고, practices/test.md의 테스트 가격 원칙을 9장 테스트 편향에 반영했다.
practices/env.md는 이번 작업이 환경 변수 계약을 바꾸지 않아 적용하지 않았다(설정 저장은 앱 설정 파일).
적용 결과는 11장 가드레일과 AC19(원칙 1: 포팅이 obsolete로 만드는 `src/` 크레이트를 같은 변경에서 삭제)에 있다.
기존 학습 규칙 중 INV-herdr-unseen-token(허들 상태 토큰 매핑 불변식)이 R5의 상태 표시와 접촉하므로 가드레일에 명시했다.

## 5. Major Technical Structure Changes

- herdr-core 권한 확장: 단일 `--workspace-root` 모델을 멀티 워크스페이스(영속 등록 목록 + pane cwd 자동 발견 + git worktree 조립)와 디바이스(로컬/원격) 차원으로 확장한다. 스냅샷/이벤트 C ABI 계약(contracts/herdr-api.schema.json)이 워크스페이스·체크아웃·디바이스·탭 구조를 실어 나르도록 확장된다.
- 원격 채널 신설: retired `src/remote.rs`(3,550줄)의 SSH/mini 로직을 herdr-core로 포팅해 원격 herdr 데몬과의 pane·파일트리·워크스페이스 채널을 만든다. 포팅 완료와 같은 변경에서 retired `src/` 크레이트를 삭제한다.
- 영속 설정 저장소: 워크스페이스 등록 목록, 디바이스(SSH 호스트) 목록, 테마 조정값, 에이전트 기본 옵션, 마지막 선택 상태를 herdr-core persistence 계층에 저장한다.
- 번들 herdr: hide.app 안에 고정 버전 herdr 바이너리(+Apache-2.0 고지)를 동봉하고, 데몬 확보 체인(소켓→설치본→번들)과 최소 버전 검사를 런타임에 추가한다.
- 패키징 체계: release 빌드 스크립트(정식 Info.plist, 아이콘, /Applications 설치, zip 산출)가 dev 빌드 스크립트와 별도로 생긴다.
- 배포 파이프라인: `modakbul-gongbang/hide` public 레포 신설 + GitHub Actions(`v*` 태그 → 빌드·zip·SHA-256 → draft 릴리스).

## 6. Requirements

- R1. 브랜딩·번들: hide는 표시명 `hide`, bundle id `me.grab.hide`, 선택된 아이콘을 가진 release 번들로 조립되고, /Applications 설치 시 Spotlight에서 검색·실행된다. 지원 대상은 macOS 14+ / Apple Silicon이다.
- R2. herdr 확보 체인: 앱은 ①살아있는 소켓 ②설치본 ③번들 herdr 순서로 데몬을 확보한다. 번들은 릴리스별 고정 버전(현재 v0.8.2, SHA-256 기록)이고, 설치본·원격이 최소 버전 미달이면 공식 원라이너(brew 병기) 업그레이드 안내를 명시적으로 표시한다(자동 변경 없음).
- R3. 워크스페이스 모델: 워크스페이스는 영속 등록 목록 + 자동 발견(미등록 폴더의 pane, pane 소멸 시 제거)으로 구성된다. git repo는 기본 체크아웃(실제 기본 브랜치명 라벨) + pane이 있는 worktree 행으로 표시되고, non-git 폴더는 평평한 행이며 행 컨텍스트 메뉴에 'Initialize git repository' 액션이 있다. Remove는 등록 해제만 하며 파일을 삭제하지 않는다.
- R4. 풀 컨텍스트 전환: 체크아웃 선택 시 메인 영역의 탭 스트립·pane 그리드와 우측 파일트리가 함께 그 체크아웃으로 전환된다. 탭 추가 UI가 있고, 빈 체크아웃은 시작을 유도하는 빈 상태를 보인다.
- R5. Agents 감시 목록: 에이전트 pane만 herdr-label과 동일한 순서·상태 규칙(sortRank, 미확인 attention 토큰 구분)으로 표시되며, 행마다 에이전트 favicon·이름·상태·경과·폴더 라벨이 보이고 클릭 시 해당 위치로 점프·포커스한다. 일반 터미널은 표시되지 않는다.
- R6. 상단 액션: New Workspace(Finder 선택, git 없으면 init 체크박스 기본 ON, init 실패 시 non-git 추가+원인 알림), New Agent(Device→claude/codex 타일→Space(워크스페이스+체크아웃)→bypass 토글 기본 OFF+상시 경고, 토글은 선택 에이전트의 플래그로 번역(claude `--dangerously-skip-permissions`, codex `--dangerously-bypass-approvals-and-sandbox`)→Start, 빈 탭이면 새 탭 자동 생성, CLI 미설치 타일은 '설치 필요'+링크, 실행 환경은 로그인 셸 상속, 에이전트 인증은 각 CLI 자체 로그인에 위임하고 hide는 자격증명에 관여하지 않으며 CLI 버전 하한을 강제하지 않는다), Search(에이전트+스페이스 통합 팔레트, 뱃지, Enter 점프/전환, 매칭 없음 상태).
- R7. 원격 디바이스: 등록된 임의 SSH 호스트(macOS·Linux)에 대해 워크스페이스·pane·파일트리가 로컬과 동일 UX로 동작한다. 자격증명 저장·입력 없이 ssh-agent+known_hosts 정책을 따르고 실패는 원인별로 명시된다. 원격 herdr 미설치·버전 미달은 안내만 한다. 연결이 끊기면 원격 컨텍스트에 머물며 끊김 상태·재연결 시도를 표시하고(로컬 자동 복귀 없음, 전환은 사용자 수동), 재시작·재연결 시 재attach하며, 소멸 세션은 제거·안내·재동기화한다. 디바이스 삭제는 detach만이며 결과가 다이얼로그에 고지된다.
- R8. 설정: 설정 창은 pane 단축키, herdr 연결 상태/버전, 테마(액센트 컬러+폰트 크기: 즉시 적용·자동 저장·Reset), 에이전트 기본 옵션(bypass 기본값, 변경 시 동일 경고), 디바이스 관리(추가+연결 테스트+삭제)를 제공한다.
- R9. 다크 전용 디자인: 색·타이포·간격·라운드 토큰이 한 곳에 정의되고 전 화면이 이를 사용한다. 시스템이 라이트 모드여도 hide는 다크다. Raycast DESIGN.md가 MIT 고지와 함께 루트에 있고 AGENTS.md가 디자인 기준으로 참조한다.
- R10. 배포: `modakbul-gongbang/hide` public 레포에 `v*` 태그를 push하면 GitHub Actions가 release 빌드 zip과 SHA-256 체크섬을 가진 draft 릴리스를 만든다(publish는 사용자 수동, 실패 시 미발행). 최초 공개 push 전에 시크릿/개인정보 스캔 기록과 사용자 최종 승인이 있어야 한다. zip에는 우클릭 열기/xattr 설치 안내가 동봉된다.
- R11. 수명 기본값: 마지막 선택 디바이스/체크아웃이 재시작 시 복원되고, 첫 실행(등록 워크스페이스 없음)은 New Workspace를 유도하는 빈 상태를 보이며, PetWindow 등 기존 부가 기능은 그대로 동작한다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | /Applications에 설치된 hide.app이 Spotlight에서 "hide" 검색으로 실행되고, Dock·메뉴바·About에 소문자 `hide`와 선택된 아이콘이 표시되며 bundle id가 `me.grab.hide`다. (R1) | judged | 설치 후 Spotlight 검색·실행·About 화면 스크린샷과 번들 메타데이터 검사 |
| AC2 | herdr가 소켓·설치 경로 어디에도 없는 환경에서 hide를 실행하면 번들 herdr로 데몬이 기동되어 정상 동작하고, 설치본이 있으면 설치본이, 살아있는 소켓이 있으면 그 데몬이 우선된다. (R2) | judged | 격리 환경 실행의 데몬 프로세스·소켓 상태 증거와 세 우선순위 케이스별 확인 기록 |
| AC3 | 최소 버전 미달인 herdr(로컬 설치본 또는 원격)를 만나면 공식 원라이너(brew 병기) 업그레이드 안내가 표시되고, 로컬은 번들로 기동을 이어가며, 어떤 경우에도 침묵 실패가 없다. (R2) | judged | 구버전 herdr를 놓은 실행의 안내 문구 화면과 기동 결과 증거 |
| AC4 | New Workspace로 등록한 워크스페이스는 재시작 후에도 목록에 남고, 미등록 폴더에서 pane이 돌면 임시 항목으로 나타났다가 pane 소멸 시 사라진다. (R3) | judged | 등록→재시작→목록 화면과 임시 항목 등장·소멸 전후 UI 증거 |
| AC5 | pane이 있는 worktree만 부모 repo 아래 브랜치명 행으로 표시되고 마지막 pane 종료 시 사라지며(보던 중이면 기본 체크아웃으로 복귀), 기본 체크아웃 라벨은 저장소의 실제 기본 브랜치명이다. (R3, R4) | judged | worktree pane 생성·종료 전후의 사이드바 화면과 master 기본 브랜치 repo의 라벨 증거 |
| AC6 | 체크아웃 행을 클릭하면 메인 영역 탭·pane 구성과 우측 파일트리 루트가 그 체크아웃으로 함께 바뀌고, [+]로 새 탭이 추가되며, 탭 없는 체크아웃은 시작 유도 빈 상태를 보인다. (R4) | judged | 전환 전후의 탭·그리드·파일트리 스크린샷과 빈 체크아웃 상태 화면 |
| AC7 | 에이전트 2개+일반 터미널 3개가 섞인 구성에서 AGENTS 섹션에는 에이전트 2개만 favicon·상태·경과·폴더 라벨과 함께 herdr-label과 동일한 순서로 표시되고, 행 클릭 시 해당 체크아웃·탭으로 점프해 pane에 포커스가 간다. (R5) | judged | 혼합 pane 구성의 사이드바 화면과 점프 전후 포커스 상태 증거 |
| AC8 | git 없는 폴더의 New Workspace 확인 단계에 init 체크박스가 기본 ON으로 보이고, 해제하면 평평한 non-git 행으로 추가되며, init 실패 시 non-git으로 추가되면서 실패 원인 알림이 뜬다. 사용자 동의 없는 git init은 어떤 경로에도 없다. (R6) | judged | 체크 ON/OFF·init 실패 각 케이스의 다이얼로그·결과 행·알림 화면과 대상 폴더의 .git 유무 검사 |
| AC9 | New Agent 모달이 Device→타일(claude/codex, 미설치면 '설치 필요'+설치 링크)→Space(워크스페이스+체크아웃)→bypass 토글(기본 OFF, ON이면 위험 문구 상시 노출, 선택 에이전트의 플래그로 번역: claude `--dangerously-skip-permissions` / codex `--dangerously-bypass-approvals-and-sandbox`)→Start 순서로 동작하고, Start 시 대상 체크아웃의 활성 탭(없으면 자동 생성된 새 탭)에 pane이 생긴다. (R6) | judged | 모달 각 상태(토글 OFF/ON, 미설치 타일)와 Start 결과 pane·새 탭 생성 화면 |
| AC10 | Search 팔레트가 에이전트+스페이스만 검색하고(일반 터미널 제외), 행에 종류·워크스페이스·디바이스 뱃지가 보이며, ⏎가 에이전트→pane 점프·스페이스→체크아웃 전환으로 동작하고, 매칭 없음 상태와 esc 닫기가 있다. (R6) | judged | 필터·뱃지·Enter 두 동작·매칭 없음 각 상태의 팔레트 화면과 이동 결과 증거 |
| AC11 | 등록된 원격 디바이스 선택 시 원격 워크스페이스·pane·파일트리가 로컬과 동일 UX로 동작하고, SSH·인증·host key 실패는 원인별 문구로 표시되며, 앱 어디에도 비밀번호/패스프레이즈 입력·저장 UI가 없고, 원격 herdr 미설치·미달 시 설치/업그레이드 명령 안내만 표시된다. (R7) | judged | mini 원격 플로우 화면, 실패 케이스별 문구 증거, 자격증명 UI 부재 확인 |
| AC12 | 연결이 끊긴 동안 원격 컨텍스트에 끊김 상태와 재연결 시도가 표시되고 로컬 전환은 자동으로 일어나지 않으며, hide 재시작 또는 SSH 재연결 후 기존 원격 pane에 재attach되어 이력이 herdr 보존 범위만큼 보이고, 소멸한 세션의 행은 제거되며 "원격에서 세션이 종료되었습니다" 안내와 함께 목록이 재동기화된다. (R7) | judged | 강제 절단 중의 끊김 상태 화면, 재시작·재연결 후 재attach 증거, 세션 소멸 케이스의 전후 화면 |
| AC13 | Workspace Remove와 디바이스 삭제의 확인 다이얼로그가 각각 "파일은 삭제되지 않습니다"·"실행 중인 에이전트 N개는 원격에서 계속 실행되며 재추가 시 재연결됩니다"를 명시하고, 실제로 파일 삭제·원격 프로세스 종료가 일어나지 않는다. (R3, R7) | judged | 두 다이얼로그 화면, 실행 후 디스크 파일 불변·원격 에이전트 생존 증거 |
| AC14 | 설정 창 5개 섹션이 모두 동작한다: 액센트·폰트 변경이 즉시 적용되고 재시작 후 유지되며 Reset으로 복원된다. bypass 기본값 변경 시 동일 경고가 노출된다. 디바이스 추가의 연결 테스트가 실패 원인을 구분 표시하고 저장은 허용한다. (R8) | judged | 각 섹션 화면, 테마 변경 즉시 적용·재시작 유지·Reset 전후 증거, 연결 테스트 실패 문구 |
| AC15 | 시스템이 라이트 모드여도 다음 화면 인벤토리 전부가 다크 토큰으로 렌더된다: 메인 창(사이드바+탭·pane 그리드+파일트리+상태바), 빈 체크아웃 상태, 첫 실행 빈 상태, New Agent 모달, New Workspace 다이얼로그, Search 팔레트, 설정 창 5개 섹션, Remove·디바이스 삭제 다이얼로그. 색·타이포·간격·라운드 값이 단일 토큰 정의에서 오고, 루트에 Raycast DESIGN.md(MIT 출처 고지 포함)가 있으며 AGENTS.md가 이를 참조한다. (R9) | machine+gate:human | 라이트 모드 시스템에서 인벤토리 각 화면의 스크린샷, 토큰 단일 정의 검사, 사용자 taste 판정 |
| AC16 | `v*` 태그 push가 zip과 SHA-256 체크섬 파일을 가진 draft 릴리스를 만들고 publish는 수동으로 남으며, 최초 공개 push 이전 시점의 시크릿/개인정보 스캔 결과와 사용자 승인 기록이 남아 있다. (R10) | machine+gate:human | 스캔 기록, 사용자 승인 인용, Actions 실행과 draft 릴리스·체크섬 산출물 |
| AC17 | 릴리스 zip에 설치 안내(우클릭 열기 또는 xattr 한 줄)가 동봉되어 있고, 격리 속성이 있는 zip에서 안내 절차대로 실행이 가능하다. (R1, R10) | judged | 격리 속성 zip의 설치 절차 수행 기록과 실행 성공 화면 |
| AC18 | 재시작 시 마지막 선택 디바이스/체크아웃이 복원되고, 등록 워크스페이스가 없는 첫 실행에서는 New Workspace 유도 빈 상태가 보이며, PetWindow가 기존과 동일하게 동작한다. (R11) | judged | 재시작 전후 선택 상태 화면, 초기화된 상태의 첫 실행 화면, PetWindow 동작 확인 |
| AC19 | `src/remote.rs` 포팅이 완료된 동일 변경에서 retired `src/` 크레이트가 소스 트리와 빌드 그래프에서 제거되어 있다. (R7) | machine | 소스 트리에 `src/` 부재, 워크스페이스 멤버·빌드 그래프에 해당 크레이트 부재 검사 결과 |
| AC20 | herdr Apache-2.0 고지가 번들에 동봉되고, claude/codex 로고 에셋의 출처 표기와 브랜드 가이드라인 확인 결과가 기록되어 있다(불허 판정 시 자체 글리프로 대체되어 있다). (R2, R5, R10) | judged | 번들 내 고지 파일 목록, 가이드라인 확인 기록, 채택/대체 결정 근거 |

## 8. PRD-Level Tasks

- T1. 다크 전용 디자인 토큰 시스템을 정의하고 기존 전 화면에 적용하며, Raycast DESIGN.md(MIT 고지 포함)를 루트에 추가하고 AGENTS.md에 디자인 기준으로 참조를 넣는다. Covers R9. Depends on: none.
- T2. herdr-core에 워크스페이스/체크아웃 모델을 구현한다: 영속 등록 목록, pane cwd 자동 발견, git worktree 조립, 실제 기본 브랜치명 라벨, non-git 평행 행, Remove(등록 해제만), 마지막 선택 복원. C ABI 스냅샷/이벤트 계약 확장 포함. Covers R3, R11. Depends on: none.
- T3. 사이드바를 재설계한다: WORKSPACES 트리, AGENTS 감시 목록(favicon·상태·폴더·점프), 하단 디바이스 목록+설정 아이콘, 첫 실행 빈 상태. Covers R5, R11. Depends on: T1, T2.
- T4. 풀 컨텍스트 전환을 구현한다: 체크아웃 선택 → 탭 스트립+pane 그리드+파일트리 전환, 탭 추가, 빈 체크아웃 상태, worktree 소멸 시 복귀. Covers R4. Depends on: T2, T3.
- T5. 상단 액션 3종을 구현한다: New Workspace 다이얼로그(git init 체크박스·실패 처리), New Agent 모달(타일·CLI 탐지·Space 선택기·bypass 경고·빈 탭 자동 생성·로그인 셸 환경), Search 팔레트(통합 검색·뱃지·Enter 동작). Covers R6. Depends on: T4.
- T6. 설정 창을 구현한다: 단축키·herdr 상태/버전·테마(액센트+폰트, 즉시 적용·자동 저장·Reset)·에이전트 기본 옵션(bypass 기본값+경고)·디바이스 관리 UI(추가·연결 테스트·삭제 다이얼로그). Covers R8. Depends on: T3.
- T7. 원격 디바이스를 완성한다: `src/remote.rs` 로직을 herdr-core로 포팅해 원격 워크스페이스·pane·파일트리 채널을 구현하고, SSH 무자격증명 모델·원인별 실패 문구·미설치/미달 안내·재연결·소멸 세션 처리·detach 삭제를 연결하며, 같은 변경에서 retired `src/` 크레이트를 삭제한다. Covers R7. Depends on: T4, T6.
- T8. 브랜딩·패키징을 완성한다: Info.plist(표시명 hide, me.grab.hide), ima2 아이콘 후보 생성→사용자 선택→.icns 적용, release 빌드 스크립트(/Applications 설치·zip 산출), herdr 번들(버전 고정+SHA-256 기록+Apache-2.0 고지)과 데몬 확보 체인·최소 버전 검사. Covers R1, R2. Depends on: none.
- T9. 에이전트 로고 에셋을 확보한다: Anthropic/OpenAI 공식 브랜드 가이드라인을 확인해 허용 범위의 claude/codex 에셋을 출처 표기와 함께 번들하거나, 불허 시 자체 글리프를 제작해 대체하고 확인 결과를 기록한다. Covers R5. Depends on: none.
- T10. 자동 회귀 테스트를 갖춘다: 워크스페이스 조립(등록+발견+worktree+기본 브랜치), herdr 버전 비교·체인 선택 로직, 에이전트 상태 정렬·미확인 토큰 매핑(INV-herdr-unseen-token 준수), Search 필터·Enter 라우팅의 핵심 로직 테스트. Covers R2, R3, R5, R6 회귀 보호. Depends on: T5, T7.
- T11. 배포 파이프라인을 만든다: 시크릿/개인정보 스캔 실행·기록, `modakbul-gongbang/hide` public 레포 생성, 사용자 최종 승인 후 최초 push, `v*` 태그 → 빌드·zip·SHA-256 → draft 릴리스 GitHub Actions 워크플로, zip 동봉 설치 안내 문서. Covers R10. Depends on: T8, T10.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust·Swift 빌드와 기존 테스트 회귀 없음 | none |
| automated behavior | yes | 핵심 모델·라우팅·상태 매핑 로직의 회귀 보호 | none |
| app runtime | yes | 실행 앱에서의 주 사용자 플로우(스크린샷 증거) | 최종 UX·디자인 taste |
| remote live | yes | mini 실기기 SSH 원격 플로우 | 전용 테스트 워크스페이스 경계 승인 |
| release live | yes/blockable | 공개 레포·릴리스 파이프라인 실동작 | 공개 push 최종 승인, draft publish |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R11, AC19 | Rust(herdr-core)·Swift(macos) 빌드와 기존 테스트 스위트가 전부 통과하고 회귀가 없으며, 포팅 완료 후 retired `src/` 크레이트가 소스 트리와 빌드 그래프 어디에도 없다 | yes | no |
| V2 | automated behavior | R2, R3, R5, R6, AC2-AC5, AC7, AC10 | 워크스페이스 조립(등록·발견·worktree·기본 브랜치), herdr 체인·버전 비교, 에이전트 상태 정렬·미확인 토큰 매핑, Search 필터·Enter 라우팅이 자동 테스트로 커버되어 각 로직의 현실적인 회귀에서 실패한다 | yes | no |
| V3 | app runtime | R1, R3-R6, R8, R9, R11, AC1, AC4-AC10, AC13-AC15, AC18, SC1, SC2, SC3, SC4, SC5, SC8 | 실행 중인 hide 앱에서 로컬 주 플로우(전환·감시·상단 액션 3종·설정·다크 토큰·수명 기본값)가 각 SC 카드의 primary·failure·recovery 경로까지 스크린샷 증거로 확인된다 | yes | no |
| V4 | remote live | R7, AC11-AC13, SC6, SC8 | mini에 전용 테스트 워크스페이스·pane을 생성해 원격 전환·파일트리·재연결·소멸 세션·detach 삭제가 실기기에서 확인되고, 검증 후 생성물이 정리되며 기존 세션은 건드리지 않는다 | yes | no |
| V5 | app runtime | R1, R2, AC1-AC3, AC17, AC20, SC7 | 격리 환경(quarantine zip, herdr 비노출 PATH)에서 설치 안내 절차대로 실행되어 번들 herdr로 기동되고, 설치본·소켓 우선순위와 Spotlight 검색·고지문 동봉이 확인된다 | yes | no |
| V6 | release live | R10, AC16, AC20, SC7 | 시크릿 스캔 기록과 사용자 승인 후 public 레포에 push되고, v* 태그가 zip+SHA-256을 가진 draft 릴리스를 만들며 publish는 수동으로 남는다 | yes | yes |

### 9.3 Human Verification

- 앱 아이콘 선택: ima2 후보 중 최종 아이콘을 사용자가 고른다 (D-12).
- 다크 디자인 최종 taste: 토큰 적용 결과와 사이드바·모달·팔레트의 시각 품질을 사용자가 승인한다.
- 공개 push 최종 승인: 시크릿 스캔 결과를 확인한 뒤 `modakbul-gongbang/hide` public push를 사용자가 승인한다 (D-19). V6은 이 승인 전까지 blocked다.
- draft 릴리스 publish: 릴리스 공개는 사용자만 수행한다 (D-27).
- 타사 로고 포함 최종 확인: 공개 배포물에 claude/codex 로고(또는 대체 글리프)가 포함된 최종 형태를 사용자가 확인·승인한다 (D-27, D-36).

## 10. Risks And Open Decisions

- 원격 검증이 실기기 mini에 의존한다: mini가 다운이면 V4가 지연된다. 완화: 전용 워크스페이스만 생성·정리하고, 검증 창을 짧게 유지한다.
- Linux 원격 호스트는 지원 선언 대상이지만 이번 릴리스에서 실기기 검증이 없다(사용자 결정: 검증 가능한 Linux 호스트 부재). 실기기 검증은 mini(macOS arm64)로 한정하고, Linux 호스트 확보 시 후속 검증한다.
- remote.rs 포팅(3,550줄)이 이번 스코프에서 가장 큰 단일 작업이다: 포팅 중 발견되는 계약 불일치는 구조 변경 승인 없이 확장하지 말고 보고한다.
- 미서명 배포의 사용자 마찰: Gatekeeper 경고는 안내로만 완화된다. 공증은 non-goal로 기록된 후속이다.
- 타사 로고 번들: 가이드라인 불허 시 글리프 대체 경로가 계약되어 있다 (T9).
- 공개 레포 전환: push 이후 커밋 이력은 영구 공개다. 시크릿 스캔+사용자 승인 게이트가 유일한 방어선이므로 스캔 결과를 증거로 남긴다.
- herdr 스냅샷 스키마 확장은 herdr 데몬 버전과의 계약이다: 최소 버전 검사(R2)가 이 위험의 방어선이다.

Open decisions: 없음.
모든 P0/P1 결정이 qa-log Decision Register에서 resolved 상태다.

## 11. Implementation Guardrails

- 스코프 확장 금지: non-goal 목록(공증, 내부 리네임, gemini/cursor, Intel, Windows, 라이트 테마, 파일 검색, 원격 자동 설치, 자체 이력 저장)을 어떤 이유로도 열지 않는다.
- 사용자 승인 없는 공개 행위 금지: public 레포 생성 후에도 최초 push는 시크릿 스캔 결과와 함께 사용자 승인을 받은 뒤에만 한다. 릴리스 publish는 하지 않는다(draft까지만).
- 자격증명 취급 금지: 비밀번호·패스프레이즈·토큰의 입력 UI나 저장 경로를 만들지 않는다 (D-28).
- 파괴 동작 금지: Workspace Remove·디바이스 삭제가 파일 삭제나 원격 프로세스 종료를 일으키는 구현을 금지한다 (D-39, D-22). 파괴적·고객 가시 액션 앞에는 결과를 먼저 고지한다 (design/principles.md rule 6).
- 자동 git init 금지: 사용자가 다이얼로그에서 체크박스를 본 경우에만 init한다 (D-09).
- obsolete 삭제 의무: remote.rs 포팅이 완료되는 변경에서 retired `src/` 크레이트를 함께 삭제한다. 다른 작업에서도 대체된 코드·경로를 같은 변경에서 지운다 (engineering/principles.md rule 1).
- 침묵 실패 금지: 연결·인증·버전·init 실패는 원인과 함께 표시한다. 기본값·빈 값으로 실패를 덮지 않는다 (engineering rule 4, 10).
- 기존 자산 우선: SidebarAgent·persistence·RemoteWorkspaceSummary 등 기존 구현을 확장하고 병렬 구현을 만들지 않는다 (engineering rule 7). 화면 구조는 herdr-label 기준 레퍼런스(스크린샷 #1~#3)와 기존 패턴을 따른다 (design rule 5).
- 멱등성: 워크스페이스 등록·디바이스 추가·설치 스크립트·릴리스 워크플로의 두 번째 실행이 같은 상태로 수렴해야 한다 (engineering rule 11).
- 상태 매핑 불변식: 에이전트 상태 토큰 처리(미확인 attention 구분)는 INV-herdr-unseen-token을 지키고 관련 테스트를 green으로 유지한다. 상태 파싱 휴리스틱은 격리하고 구조적 신호를 우선한다 (engineering rule 13).
- 상태의 시각 인코딩: 연결 상태·에이전트 상태·디바이스 뱃지는 색·아이콘·뱃지로 먼저 표현하고 설명 문장은 최후 수단으로 쓴다. 의미를 가진 아이콘에는 라벨/툴팁을 단다 (design rule 7). 최빈 액션(에이전트 점프, 체크아웃 전환)은 클릭 1회다 (design rule 3).
- 테스트 가격 준수: 호출자가 관찰하는 결과를 구현 밖에서 정한 기대값으로 검증하고, 리팩토링에 깨지는 배선 테스트를 추가하지 않는다 (engineering rule 12, practices/test.md).
- 승인 없는 구조 변경 금지: 5장에 없는 서비스·스키마·외부 호출·아키텍처 변경이 필요해지면 멈추고 승인을 요청한다.

## 12. Implementation Result Report Contract

구현 에이전트는 다음을 보고한다:

- status: Done / Partially Done / Blocked.
- 사용자 가시 변경 요약(사이드바·전환·모달·팔레트·설정·브랜딩·설치 경험).
- 변경된 주요 모듈·C ABI 계약·데이터 형태(스냅샷 스키마, persistence 스키마)와 실제 선택한 파일/모듈 구조·책임 경계.
- 승인된 기술 구조(5장) 준수 여부와 이탈 내역.
- T1~T11 완료 상태와 R/AC/V 커버리지.
- 모드별 검증 증거: 빌드·테스트 로그, 로컬/패키징/원격 스크린샷, mini 검증의 생성·정리 기록, 릴리스 파이프라인 증거(레포 URL, 스캔 기록, 사용자 승인 인용, Actions 실행, draft 릴리스 URL·체크섬).
- 추가·수정된 자동 테스트와 각각이 보호하는 회귀 위험.
- 아이콘 선택 기록(후보→선택), 로고 가이드라인 확인 결과와 채택/대체 결정, 라이선스 고지 목록(herdr Apache-2.0, DESIGN.md MIT, 로고 출처).
- deviations와 남은 인간 리뷰(디자인 taste, publish), 미완·후속 후보(공증, 내부 리네임, gemini/cursor 등 non-goal 재확인 포함).
