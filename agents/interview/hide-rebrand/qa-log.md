---
topic: "hide 리브랜딩·패키징·사이드바 재설계"
status: "complete"
where: "brownfield"
selected_packs: "ux, compatibility, operation, verification, risk, provider"
created_at: "2026-08-29"
updated_at: "2026-08-30"
question_count: 24
normalization_policy: "transcript-sync-with-checkpoint-backfill"
normalization_checkpoint_every: 10
---

# Interview Log: hide 리브랜딩·패키징·사이드바 재설계

## Current Understanding

- herdr-ide macOS 앱을 'hide'(herdr+ide)로 리브랜딩하고 패키징해 Spotlight 검색과 외부 사용자 배포가 가능하게 한다
- 좌측 패널을 사이드 패널로 재설계: Workspace 섹션(+worktree를 branch처럼), Agents 섹션(herdr-label처럼 상태·순서 유지, claude/codex favicon, 폴더 표시), 최하단 설정 아이콘 + 디바이스(local/mac mini) 연결
- Raycast DESIGN.md를 다운받아 루트에 넣고 AGENTS.md에서 참조시키며 전반적 디자인 리팩토링 진행
- 우측 파일트리는 선택된 워크스페이스에 맞춰 전환

## Intake Cursor

- next_decision_id: D-48
- next_question: (owned by the live conversation until checkpoint)
- last_materiality_sweep: checkpoint 9
- outstanding_raw_entries: none
- next_checkpoint_at: Q34

## Transcript Sources

| Runtime | Session ID | Start ref |
| --- | --- | --- |
| claude | ea9f12f2-8227-44ca-b8a9-96e2e5955ab7 | de5881ea-b24d-447e-8bc9-849888338af4 |

## Decision Register

| ID | Kind | Area | Decision / fact | Priority | Source / owner | Status | PRD mapping / revisit |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D-01 | fact | packaging | 현재 빌드는 macos/scripts/build_dev_app.sh가 debug 바이너리를 macos/build/assembled/HerdrIDE.app으로 조립하고 ad-hoc 서명(codesign --sign -)만 수행. /Applications 설치·Spotlight 노출·외부 배포(공증) 없음 | P0 | repo macos/scripts/build_dev_app.sh:6-30 | resolved |  |
| D-02 | fact | architecture | 앱은 --workspace-root 런치 인자로 단일 워크스페이스만 열며(CoreBridge.swift:338-355), 레이아웃은 HSplitView 3패널(AgentsPanel/TerminalPanel/WorkbenchPanel, ShellView.swift:28-58). 파일트리는 navigator.root_path 기반 WorkbenchPanel | P0 | repo macos/Sources/HerdrMacOS/ShellView.swift:28, CoreBridge.swift:338 | resolved |  |
| D-03 | fact | sidebar-data | SidebarAgent가 이미 workspaceLabel, agentKind(claude/codex), state, symbol, sortRank, elapsed, activity를 core snapshot으로 전달받음(CoreBridge.swift:117-141) - 사이드바 재설계에 필요한 데이터 대부분 존재 | P1 | repo macos/Sources/HerdrMacOS/CoreBridge.swift:117 | resolved |  |
| D-04 | fact | devices | mini 원격 워크스페이스 조회 코드가 이미 존재: ssh mini herdr workspace list를 호출해 RemoteWorkspaceSummary 리스트를 로드(OperationalModels.swift:396-475) | P1 | repo macos/Sources/HerdrMacOS/OperationalModels.swift:396 | resolved |  |
| D-05 | decision | packaging | 배포는 미서명 zip: release 빌드 + 정식 번들(아이콘·Info.plist) + /Applications 설치로 Spotlight 노출, 외부엔 zip + 우클릭 열기/xattr 안내 동봉. Developer ID 서명·공증은 Apple Developer 계정 발급 후 후속 | P0 | user Q1 | resolved | R: packaging, revisit: Apple Developer 계정 발급 시 |
| D-06 | decision | architecture | 워크스페이스=git repo, 워크트리=체크아웃. 사이드바 계층: workspace 그룹 → main(항상 표시) + worktree(pane 존재 시만 표시, 마지막 pane 닫히면 제거). pane cwd를 git으로 해석해 자동 조립, 사용자 등록 관리 없음 | P0 | user Q2-Q3 (추천 수용) | resolved |  |
| D-07 | decision | architecture | 체크아웃 선택 = 풀 컨텍스트 전환: 메인 영역이 해당 체크아웃의 탭 스트립 + pane 그리드로 전환, 상단에 새 탭 추가 UI, 파일트리도 함께 전환 | P0 | user Q3 | resolved |  |
| D-08 | decision | sidebar | Agents 섹션은 에이전트 pane만 표시(일반 터미널은 그리드에만): herdr-label과 동일한 상태·순서, agent favicon, 폴더 라벨. 행 클릭 시 해당 워크스페이스→체크아웃→탭 점프 + pane 포커스 | P0 | user Q4 | resolved |  |
| D-09 | decision | git-init | 자동 git init 금지. git 없는 폴더는 worktree 계층 없이 평평한 워크스페이스 행. New Workspace 다이얼로그에 'Initialize git repository' 체크박스 기본 ON(명시적 노출), 컨텍스트 메뉴에도 init 액션 | P1 | user Q4+Q8 | resolved |  |
| D-10 | decision | remote | 디바이스는 풀 컨텍스트 전환: 원격 디바이스 선택 시 원격 워크스페이스·pane·파일트리가 로컬과 동일 UX로 동작. src/remote.rs의 SSH 로직 포팅 포함. 대상 디바이스 범위는 D-24(임의 SSH 호스트, mini는 기본 등록 사례) | P0 | user Q5 + Q17-4 | resolved | risk: 원격 검증에 mini 실기기 필요 |
| D-11 | decision | branding | 표시명 소문자 'hide', bundle id 'me.grab.hide'. 내부 코드명(Herdr* 타입, herdr-core 크레이트)과 로컬 디렉토리명 herdr-ide는 유지, 내부 리네임은 후속. GitHub 원격 레포명은 D-19에 따라 'hide' | P0 | user Q6 | resolved |  |
| D-12 | decision | branding | 앱 아이콘은 ima2로 후보 다수 생성 → 사용자 선택(인간 승인) → 1024px 규격으로 다듬어 .icns 적용 | P1 | user Q7 | resolved | human verification: 아이콘 선택 |
| D-13 | decision | sidebar-actions | 상단 액션 = New Workspace / New Agent / Search (New Terminal 제외). New Agent 모달: Device→Agent 타일→Space→'Bypass permissions' 토글(--dangerously-skip-permissions)→Start Agent (스크린샷 #2 레퍼런스). New Workspace: Finder 폴더 선택 + git init 체크박스. Search: 커맨드 팔레트 오버레이, 에이전트·스페이스 통합 검색, 행에 종류·워크스페이스·디바이스 뱃지, ↑↓/⏎ 네비 (스크린샷 #3 레퍼런스) | P0 | user Q8 (스크린샷 2·3) | resolved |  |
| D-14 | decision | agents | 런칭 시 claude + codex만 실제 지원, New Agent 타일도 2개만. gemini/cursor는 후속 | P1 | user Q9 | resolved |  |
| D-15 | decision | design | 다크 전용 테마(시스템 라이트 모드여도 다크). 디자인 토큰(색·타이포·간격·라운드) 단일 정의 후 전 화면 적용. Raycast DESIGN.md를 루트에 추가하고 AGENTS.md에서 디자인 기준으로 참조 | P1 | user Q10 | resolved |  |
| D-16 | assumption | assets | claude/codex favicon은 런타임 네트워크 페치가 아니라 정적 에셋으로 번들(오프라인·성능·안정성). 교체는 에셋 파일 교체로 | P2 | agent default | resolved |  |
| D-17 | decision | packaging | herdr 데몬 확보 순서(사용자 명시 승인): ①살아있는 소켓이 있으면 그 데몬 사용(세션 보존) ②설치된 herdr(~/.local/bin → /opt/homebrew/bin → /usr/local/bin)로 시작 ③없으면 hide.app에 번들된 herdr로 시작(Apache-2.0 고지 동봉). hide가 요구하는 최소 herdr 버전을 명시하고 미달 시 업그레이드 안내. 근거: herdr는 단일 19MB arm64 바이너리, 공개 레포 herdrdev/herdr Apache-2.0 | P0 | user Q11 제안 + Q18-3 체인 명시 승인 | resolved |  |
| D-18 | decision | settings | 설정 창은 본격 구성: 기존 pane 단축키 + herdr 연결 상태/버전 + 테마 토큰 조정 + 에이전트 기본 옵션(bypass permissions 기본값) + 디바이스(SSH 호스트) 관리. mini 하드코딩을 설정 기반 디바이스 목록으로 대체 | P1 | user Q12 | resolved |  |
| D-19 | decision | launch | 배포 채널: GitHub public 레포 modakbul-gongbang/hide 신규 생성(yansfil이 org admin, 이름 미점유 확인) + Releases 자동화로 zip 배포. 공개 push 직전 시크릿/개인정보 스캔 + 사용자 최종 확인 게이트 필수. 로컬 디렉토리명 herdr-ide는 유지 | P0 | user Q13-Q15 | resolved | human verification: 공개 push 최종 승인 |
| D-20 | decision | defaults | 명시 승인된 수명 기본값: ①마지막 선택 디바이스/체크아웃 재시작 시 복원(herdr-core persistence) ②첫 실행(워크스페이스 없음)은 New Workspace 유도 빈 상태 ③PetWindow 등 기존 부가 기능 유지 | P1 | user Q18-1 명시 승인 | resolved |  |
| D-21 | decision | permissions | Bypass permissions 토글: 기본 OFF. ON 시 토글 하단에 위험 문구(--dangerously-skip-permissions: 에이전트가 확인 없이 파일·명령 실행) 상시 노출. 적용 범위는 해당 모달로 시작하는 에이전트 1개. 설정에서 기본값 ON 가능하되 변경 시 동일 경고 | P0 | user Q17-1 | resolved |  |
| D-22 | decision | devices | 디바이스 삭제 = 로컬 등록 해제(detach)만. 원격 herdr 데몬·에이전트는 계속 실행. 삭제 확인 다이얼로그에 '실행 중인 에이전트 N개는 원격에서 계속 실행되며 재추가 시 재연결' 명시. 원격 작업을 종료시키는 파괴 동작 없음 | P0 | user Q17-2 | resolved |  |
| D-23 | decision | architecture | 워크스페이스 목록 수명: 영속 등록 목록(New Workspace 추가, 컨텍스트 메뉴 Remove 제거, 재시작 복원) + 자동 발견 보강(미등록 폴더에서 pane 실행 중이면 임시 표시, pane 소멸 시 제거). worktree 행만 순수 pane 기반 자동(D-06 유지) | P1 | user Q17-3 | resolved |  |
| D-24 | decision | remote | 원격 런칭 스코프 = 임의 SSH 호스트(설정에서 추가·관리). mini는 기본 등록되는 첫 사례일 뿐. D-10의 풀 컨텍스트 전환 UX는 모든 등록 디바이스에 동일 적용 | P1 | user Q17-4 | resolved |  |
| D-25 | decision | agent-start | New Agent의 Space = 선택 디바이스의 워크스페이스+체크아웃 선택기. Start 시 해당 체크아웃의 활성 탭에 pane 추가, 탭이 없으면 새 탭 자동 생성 | P1 | user Q17-5 | resolved |  |
| D-26 | decision | non-goals | 명시적 non-goal: 내부 코드명(Herdr*) 리네임은 다음 대규모 리팩토링 시 재검토, gemini/cursor 타일은 사용자 요청 시 재검토 | P1 | user Q17-6 | resolved |  |
| D-27 | decision | launch | 릴리스 자동화: v* git 태그 push → GitHub Actions 빌드·zip·SHA-256 체크섬 → draft 릴리스 생성, publish는 사용자 수동(실패 시 미발행=롤백 불요). 에셋 라이선스: awesome-design-md는 MIT(고지 포함 복사 허용, gh api로 확인), claude/codex 로고는 공식 배포 에셋을 출처 표기와 함께 번들 - 공개 배포물에 타사 로고 포함을 사용자가 인지·승인 | P1 | user Q17-7 + gh api 라이선스 확인 | resolved |  |
| D-28 | decision | security | SSH 보안 모델: hide는 자격증명 저장·입력 일절 없음. ~/.ssh/config alias + ssh-agent 키 인증만 사용, 비밀번호/패스프레이즈 UI 없음. host key 신뢰는 시스템 known_hosts 정책 그대로, 미신뢰·인증 실패는 원인과 함께 명시적 실패 | P0 | user Q18-2 | resolved |  |
| D-29 | decision | workspace | New Workspace의 git init 실패 시: 폴더는 non-git 워크스페이스로 추가하고 실패 원인을 알림으로 표시 | P1 | user Q18-4 | resolved |  |
| D-30 | decision | search | Search 대상은 에이전트 + 스페이스(워크스페이스/체크아웃)만, 일반 터미널 비검색. Enter: 에이전트 행→해당 pane 점프, 스페이스 행→해당 체크아웃으로 전환 | P1 | user Q18-5 | resolved |  |
| D-31 | decision | settings | 설정의 테마 조정은 액센트 컬러 + 폰트 크기 2개 컨트롤만: 즉시 적용, 자동 저장, Reset to defaults 버튼. 토큰 전체 편집은 후속 | P1 | user Q18-6 | resolved |  |
| D-32 | decision | git | 기본 브랜치가 main이 아닌 repo는 실제 기본 브랜치명(master 등)을 기본 체크아웃 행 라벨로 그대로 표시 - 'main' 하드코딩 금지 | P1 | user Q18-7 | resolved |  |
| D-33 | decision | compatibility | 지원 매트릭스: macOS 14+ / Apple Silicon(arm64)만. Intel은 명시적 non-goal (현 빌드도 arm64 전용) | P1 | user Q18-8 | resolved |  |
| D-34 | decision | remote | 원격 호스트에 herdr 미설치 시 자동 설치하지 않고 설치 명령 안내만 표시. mini는 이미 설치되어 있음 | P1 | user Q18-9 | resolved |  |
| D-35 | decision | agent-providers | claude/codex CLI는 PATH + 표준 설치 경로에서 탐지. 미설치면 New Agent 타일에 '설치 필요' 상태 + 설치 링크 표시. 인증은 각 CLI 자체 로그인에 위임, hide는 자격증명 무관여 | P1 | user Q18-10 | resolved |  |
| D-36 | decision | licensing | claude/codex 로고는 구현 단계에서 Anthropic/OpenAI 공식 브랜드 가이드라인 확인 후 허용 범위 에셋만 사용, 불허 시 자체 글리프로 대체 - PRD 검증 태스크로 포함 | P1 | user Q18-11 | resolved | V: 브랜드 가이드라인 확인 태스크 |
| D-37 | decision | herdr-runtime | herdr 버전 계약: 번들 herdr는 hide 릴리스마다 당시 최신 릴리스로 갱신되는 빌드 시점 고정(현재 v0.8.2, GitHub 릴리스 바이너리, SHA-256 기록). 최소 지원 버전 = 그 릴리스의 번들 버전. 설치본 미달 시 번들로 기동 + 업그레이드 안내, 원격 호스트 미달 시 업그레이드 명령 안내만(자동 변경 없음). 런타임 체인(D-17)이 소켓·설치본을 우선하므로 번들은 미설치 사용자용 바닥 | P1 | user Q19-1 (영구 고정 아님 설명 후 수용) | resolved |  |
| D-38 | decision | agent-providers | 에이전트 CLI 버전·환경: claude/codex 버전 하한 강제 없음(현행 안정판 기준). 실행 실패는 pane 출력으로 그대로 노출. 실행 환경은 사용자 로그인 셸 환경 상속(Finder 런치에서도 셸 PATH 로드, 현행 herdr pane 방식 유지) | P1 | user Q19-2 | resolved |  |
| D-39 | decision | workspace | Workspace Remove = 목록 등록 해제만, 디스크 파일 절대 삭제 안 함. 확인 다이얼로그에 '파일은 삭제되지 않습니다' 명시. 해제 후 해당 폴더에 pane이 남아 있으면 자동 발견(D-23)으로 임시 항목으로 계속 표시 | P0 | user Q19-3 | resolved |  |
| D-40 | decision | remote | 재연결 계약: 상태의 주인은 herdr 데몬. hide 재시작·SSH 재연결 시 기존 pane에 재attach, 출력 이력은 herdr 보존 범위만큼 복원. hide는 자체 세션 복제·저장 없음 | P1 | user Q19-4 | resolved |  |
| D-41 | decision | remote | 원격 SSH 호스트 지원 범위 = macOS·Linux(arm64/x86_64), herdr 공식 릴리스 플랫폼 그대로(gh 확인: v0.8.2에 macos/linux 각 아키텍처 바이너리 존재). herdr 미설치 호스트 안내 명령 = 공식 원라이너 'curl -fsSL https://herdr.dev/install.sh \| sh' (brew install herdr 병기). Windows 호스트는 non-goal | P1 | user Q20-1 + gh release/README 확인 | resolved |  |
| D-42 | decision | remote | 재attach 실패(원격 pane/세션 소멸) 처리: 복구 시도 없음. 소멸 pane 행은 목록에서 제거, '원격에서 세션이 종료되었습니다' 안내, 사이드바는 현재 원격 상태로 재동기화. herdr가 보존하지 않은 출력 이력은 소실로 명시(D-40과 일관: hide 자체 이력 저장 없음) | P1 | user Q20-2 | resolved |  |
| D-43 | decision | herdr-runtime | herdr 버전 미달 시 업그레이드 안내 명령 = 설치와 동일한 공식 원라이너 'curl -fsSL https://herdr.dev/install.sh \| sh' (brew 설치본이면 'brew upgrade herdr' 병기). 로컬·원격 호스트 동일 안내. 근거: install.sh가 최신 릴리스를 받아 기존 바이너리를 mv로 덮어쓰는 것을 확인(재실행=업그레이드) | P1 | user Q21 + install.sh 소스 확인 | resolved |  |
| D-44 | decision | remote | 원격 연결 끊김 시 로컬 자동 복귀 없음: 원격 컨텍스트에 머물며 '연결 끊김' 상태와 재연결 시도를 표시하고, 로컬 전환은 사용자가 직접 한다. UX-06 카드의 기존 '로컬로 안전 복귀' recovery를 이 결정으로 대체 (spec gate 질문 1에서 (b) 선택) | P1 | user (spec gate Q, '1. b') | resolved |  |
| D-45 | decision | verification | 원격 라이브 검증은 mini(macOS arm64)로 한정. Linux 호스트는 지원 선언 대상이나 이번 릴리스 실기기 미검증(검증 가능한 Linux 호스트 부재)을 리스크로 명시, 확보 시 후속 검증. AC15의 '전 화면'은 유한 화면 인벤토리(메인 창·빈 상태 2종·모달 2종·팔레트·설정 5섹션·삭제 다이얼로그)로 확정 | P1 | user (spec gate Q, '2-3은 ㅇㅇ') | resolved |  |
| D-46 | decision | permissions | Bypass permissions 토글의 에이전트별 플래그 매핑: claude → --dangerously-skip-permissions, codex → --dangerously-bypass-approvals-and-sandbox (로컬 codex CLI --help로 확인). 경고 문구는 공통('에이전트가 확인 없이 파일·명령을 실행합니다'), 토글 하나가 선택된 에이전트의 플래그로 번역됨 | P0 | user Q24 + codex --help 확인 | resolved |  |
| D-47 | decision | compatibility | 기존 HerdrIDE 설치 마이그레이션 없음: 기존 설치는 개발자 본인 dev 빌드뿐이고 UI 상태 저장이 /tmp 검증용 경로 기본이라 이관할 영속 데이터가 없음(CoreBridge.swift:349-350 확인). hide는 자체 영속 경로를 새로 정의하고 옛 HerdrIDE.app은 수동 삭제. herdr 데몬 소유 세션·워크스페이스 상태는 앱 교체와 무관하게 유지 | P1 | user Q24 + repo 확인 | resolved |  |

## Raw Q&A

### Q1: qa-log 생성 및 프리플라이트 사실 4건 등록 완료 (`agents/interview/hide-rebrand/qa-log.md
- decision_ids: D-05
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:09bece67-6964-4445-9887-641c47c626ad
- asked: qa-log 생성 및 프리플라이트 사실 4건 등록 완료 (`agents/interview/hide-rebrand/qa-log.md`). 인터뷰 시작합니다.
  
  **현재 이해 (2줄 요약)**
  - herdr-ide를 **hide**로 리브랜딩 + 정식 패키징(Spotlight 검색, 외부 배포 가능)
  - 사이드 패널 재설계(Workspace/worktree + Agents + 설정·디바이스), Raycast DESIGN.md 도입 + 디자인 리팩토링, 파일트리는 선택 워크스페이스 추종
  
  ---
  
  **Q1. 외부 배포의 목표 수준 - Apple Developer 계정으로 공증(notarization)까지 갈까요?**
  
  이 질문이 패키징 작업의 절반을 결정합니다. 현재 빌드는 ad-hoc 서명(`codesign --sign -`)이라 본인 Mac에서만 돌고, 외부인이 받으면 Gatekeeper가 차단합니다.
  
  | 선택지 | 외부인 경험 | 필요한 것 |
  |---|---|---|
  | **(a) Developer ID 서명 + 공증** | 다운로드 후 그냥 더블클릭으로 실행 | Apple Developer Program 계정 (연 $99), 서명 인증서 |
  | (b) 미서명 zip 배포 | "손상됨" 경고 → 우클릭 열기 or `xattr -d` 안내 필요 | 계정 불필요, 릴리스 빌드 + zip만 |
  
  **추천: (a)** - "외부사람도 쓸 수 있어야" 한다면 공증 없이는 실사용 배포가 사실상 어렵습니다. 다만 Apple Developer Program 계정이 있으신가요? (없다면 이번 스코프는 (b)로 하고 공증은 계정 발급 후 후속으로 미루는 것도 방법입니다.)
- recommended: none
- answer: ㅠㅠ 돈없어잉 우선 b로 가자. 그게 최선인거지?
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q2: 네, 그 상황에선 (b)가 최선입니다. 미서명이라도 완화 수단은 다 넣을 수 있어요: release 빌드 + 제대로 된 번들(아이
- decision_ids: D-06
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:8d207ef4-5327-45b8-87b9-af886ed17b1f
- asked: 네, 그 상황에선 (b)가 최선입니다. 미서명이라도 완화 수단은 다 넣을 수 있어요: release 빌드 + 제대로 된 번들(아이콘, Info.plist, /Applications 설치 → Spotlight 노출)로 만들고, 외부 배포용으론 zip + "우클릭 → 열기" 안내(또는 `xattr` 한 줄 스크립트)를 동봉하면 됩니다. 나중에 계정 생기면 서명 단계만 얹으면 되고 지금 작업이 버려지지 않습니다.
  
  ---
  
  **Q2. 사이드바의 Workspace 섹션 - 선택하면 무엇이 바뀌는 건가요?**
  
  이게 이번 작업에서 가장 큰 아키텍처 결정입니다. 현재 앱은 `--workspace-root` 인자로 **워크스페이스 하나만** 열고, 창 전체(터미널, 파일트리)가 거기에 묶여 있습니다. 스크린샷(herdr-label)처럼 여러 워크스페이스가 목록으로 보이려면:
  
  | 선택지 | 동작 | 규모 |
  |---|---|---|
  | **(a) 단일 창 안에서 전환** | herdr의 워크스페이스 목록을 다 보여주고, 클릭하면 파일트리 + 터미널 pane 컨텍스트가 통째로 그 워크스페이스로 전환 | 큼 - core가 멀티 워크스페이스를 알아야 함 |
  | (b) 목록은 보여주되, 다른 워크스페이스 클릭 시 새 창으로 열기 | 창 = 워크스페이스 1개 모델 유지 | 중간 |
  | (c) 클릭 시 파일트리만 전환, 터미널은 그대로 | 가장 작지만 "지금 어디를 보고 있나"가 어긋날 수 있음 | 작음 |
  
  **추천: (a)** - 3번 요구("파일트리는 선택된 워크스페이스에 맞춰")와 herdr-label의 멘탈 모델에 가장 부합합니다. 다만 구현 규모가 가장 큽니다. 어느 쪽인가요?
- recommended: none
- answer: 음 근데 워크스페이스에서 워크트리나 그런거가 다를 수도 있고.. 음 기준을 어케잡아야할지 모르겟네 어떻게 구조를 딱딱 쪼개지
- immediate_notes: 사용자가 워크스페이스/워크트리 계층 기준을 스스로 정하지 못해 구조 제안을 요청 → Q3에서 제안한 repo=워크스페이스, 체크아웃=선택단위 모델로 해소(D-06).
- needs_normalization: false

### Q3: 구조를 제가 제안해볼게요. 기준을 잡는 열쇠는 **"워크스페이스 = git 저장소, 워크트리 = 그 저장소의 체크아웃 하나"**로
- decision_ids: D-06, D-07
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:199334b0-f2ca-49bd-8f38-9bd905ad682b
- asked: 구조를 제가 제안해볼게요. 기준을 잡는 열쇠는 **"워크스페이스 = git 저장소, 워크트리 = 그 저장소의 체크아웃 하나"**로 두 층을 분리하는 겁니다.
  
  ```
  WORKSPACES
  ▾ 📁 herdr-ide              ← 워크스페이스 = repo (그룹 헤더)
      ● main                  ← 기본 체크아웃 (항상 존재)
      ⑂ fix-sidebar           ← git worktree (pane이 하나라도 거기서 돌면 자동 표시)
  ▾ 📁 sasu
      ● main
  ```
  
  동작 규칙은 세 줄로 정리됩니다:
  
  1. **그룹핑 기준**: herdr pane들의 cwd를 `git rev-parse`로 repo 루트로 해석해서 저장소 단위로 묶는다. worktree의 cwd는 자동으로 부모 repo 밑에 `⑂ 브랜치명`으로 달린다.
  2. **표시 기준**: 기본 체크아웃(main)은 워크스페이스가 보이면 항상 표시, worktree는 **pane이 하나라도 있을 때만** 나타나고 마지막 pane이 닫히면 사라진다. (말씀하신 "기본이 있을거고 + 하나라도 pane이 있다면" 그대로)
  3. **선택 단위**: 클릭하는 건 워크스페이스가 아니라 **체크아웃 한 줄**(main 또는 worktree). 선택하면 파일트리 루트가 그 체크아웃 경로로 전환된다.
  
  이렇게 하면 "워크스페이스마다 워크트리가 다를 수 있다"는 문제가 사라져요 - 워크트리는 정의상 항상 어떤 repo에 속하니, pane의 cwd만 보고 트리가 자동으로 조립됩니다. 사용자가 뭘 등록하거나 관리할 필요가 없습니다.
  
  ---
  
  **Q3. 이 구조로 가는 걸로 하고, 하나만 정하면 됩니다: 체크아웃을 선택했을 때 터미널 영역도 따라 바뀌나요?**
  
  - **(a) 파일트리만 전환** - 터미널 pane들은 지금처럼 전부 보이고, 선택은 "지금 어느 폴더를 브라우징하나"만 결정 *(추천: 구현이 명확하고, 에이전트 pane을 숨겨서 놓치는 일이 없음)*
  - (b) 파일트리 + 터미널 둘 다 전환 - 선택한 체크아웃에서 도는 pane만 터미널 영역에 표시 (진짜 "컨텍스트 전환", Q2의 (a)에 해당하는 큰 구조)
- recommended: none
- answer: git이 없는 레포면 우짜지 우선 git없으면 gitinit을 디폴트로 우선 박아버릴까? 
  그런데 이제 저거 브랜치 main을 눌렀다고 하자.그러면 걔가 이제 하나의 전체 창에 뜨고 거기서 pane관리하고 가능한거지? 그리고 tab 추가하는것도 위에 넣긴 해야됨!
  근데 만약 pane이 늘어나서 탭1에 pane 4개 거기서 2개는 claude, codex 같은거띄움 나머지 2개는 걍 새 창, 그리고 tab하나에는 그냥 터미널 1개만 띄워져 있어. 그런 상황에서는 어떻게 되는게맞을껏 깥아?
- immediate_notes: decision: 체크아웃 선택=풀 컨텍스트 전환(탭 스트립+pane 그리드), 새 탭 추가 UI 필요 / constraints: git 없는 폴더 처리 필요(사용자는 auto git init 제안) / unresolved follow-up: 탭1(에이전트2+터미널2)+탭2(터미널1) 시나리오의 표시 규칙 → Q4에서 해소 / source: user Q3 답변.
- needs_normalization: false

### Q4: 지금 답변으로 큰 구조가 확정됐네요: **체크아웃 선택 = 진짜 컨텍스트 전환** (선택하면 그 체크아웃의 pane/tab 세계가
- decision_ids: D-08
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:ba74addd-3322-4cac-a89b-60b490e99290
- asked: 지금 답변으로 큰 구조가 확정됐네요: **체크아웃 선택 = 진짜 컨텍스트 전환** (선택하면 그 체크아웃의 pane/tab 세계가 메인 영역 전체에 뜨고, 상단에 tab 스트립 + 새 탭 추가). Q3의 (b) 모델로 갑니다.
  
  말씀하신 상황(탭1: pane 4개 = claude + codex + 일반 터미널 2개, 탭2: 터미널 1개)은 이렇게 되는 게 맞다고 봅니다:
  
  ```
  main 선택 시 메인 영역:
  ┌─ [탭1] [탭2] [+] ────────────┐
  │ ┌────────┬────────┐          │   탭1 = 4-pane 그리드 그대로
  │ │ claude │ codex  │          │   (에이전트든 일반 터미널이든
  │ ├────────┼────────┤          │    pane은 pane, 차별 없이 표시)
  │ │ term   │ term   │          │
  │ └────────┴────────┘          │
  └──────────────────────────────┘
  
  사이드바 AGENTS 섹션:
    observer   ✳ claude · 📁 herdr-ide   ← 에이전트 pane만 올라옴
    codex      ⬤ codex  · 📁 herdr-ide   ← 일반 터미널 2개와 탭2의 터미널은 안 올라옴
  ```
  
  원칙은 한 줄입니다: **탭/pane 그리드는 "배치"이고, 사이드바 Agents는 "감시 목록"이다.** 일반 터미널은 그리드에만 존재하고, claude/codex 같은 에이전트 pane만 Agents 섹션에 상태·경과시간과 함께 올라옵니다(herdr-label과 동일). Agents 줄을 클릭하면 해당 워크스페이스 → 체크아웃 → 탭으로 점프해서 그 pane에 포커스가 갑니다. 그래서 "탭2에 터미널 1개"는 사이드바에 아무 흔적이 없고, 탭1의 에이전트 2개만 어느 화면에 있든 항상 보입니다.
  
  git 없는 폴더는 **auto `git init`은 안 하는 걸 추천**합니다. 사용자 폴더에 말없이 `.git`을 만드는 건 놀라운 부작용이고(Downloads 같은 데 열면 끔찍), worktree 기능이 필요 없는 폴더도 많아요. 대신: git 없는 폴더 = worktree 계층 없이 워크스페이스 한 줄로 평평하게 표시하고, 컨텍스트 메뉴에 "Initialize git repository" 액션을 명시적으로 두면 됩니다.
  
  ---
  
  **Q4. 위 두 가지, 이대로 확정할까요?** ① 에이전트 pane만 Agents 섹션에 올리고 일반 터미널은 그리드에만 (클릭 시 해당 위치로 점프) ② git 없는 폴더는 auto-init 없이 평평한 워크스페이스 + 명시적 init 메뉴
- recommended: none
- answer: ㅇㅇㅇ 그렇게 하자.
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q5: 좋습니다, 둘 다 확정. 다음으로 사이드바 최하단의 디바이스 부분입니다.
- decision_ids: D-10
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:a24d81ce-8157-4c6b-8a24-e22a4eea6553
- asked: 좋습니다, 둘 다 확정. 다음으로 사이드바 최하단의 디바이스 부분입니다.
  
  **Q5. 디바이스(local / mac mini) 항목을 눌렀을 때 어디까지 되어야 하나요?**
  
  참고로 현재 코드 상태: mini의 워크스페이스 목록을 SSH로 가져오는 코드는 이미 있지만, 원격 pane을 실제로 여는 쪽(`src/remote.rs`)은 아직 새 Swift 셸로 포팅이 안 된 상태입니다. 그래서 선택지별 규모 차이가 큽니다.
  
  - **(a) 풀 컨텍스트 전환**: mini 선택 → WORKSPACES 섹션이 mini의 워크스페이스들로 바뀌고, 체크아웃 선택하면 원격 pane·파일트리까지 로컬과 똑같이 동작 *(remote.rs 포팅 포함, 이번 스코프가 크게 늘어남)*
  - **(b) 목록+상태까지만**: 하단에 local/mini가 연결 상태 점과 함께 보이고, mini를 펼치면 원격 워크스페이스 목록은 보여주지만 열기는 아직 안 됨 - 원격 pane 동작은 후속 작업으로 분리 *(추천: 이번 릴리스는 사이드바 구조 + 로컬 경험을 완성하고, 원격은 별도 PRD로)*
  - (c) 아이콘/자리만 잡아두고 mini 연동은 전부 후속
  
  **추천은 (b)**입니다. 이번 작업(리브랜딩+패키징+사이드바+디자인)만 해도 규모가 상당해서, 원격 pane 포팅까지 묶으면 검증 범위가 두 배가 됩니다. 어떻게 할까요?
- recommended: none
- answer: A로 가야지! herdr 어차피 사용하면 기본적으로 많이 지원해줄 것 같은데~
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q6: 좋습니다, (a) 풀 원격 컨텍스트 전환까지 이번 스코프에 포함. `src/remote.rs` 포팅 + herdr 원격 연동이 정
- decision_ids: D-11
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:084bdf4e-b4c8-4e5c-8b43-32b28b5eb1ce
- asked: 좋습니다, (a) 풀 원격 컨텍스트 전환까지 이번 스코프에 포함. `src/remote.rs` 포팅 + herdr 원격 연동이 정식 요구사항으로 들어갑니다.
  
  **Q6. 리브랜딩 범위를 확정합시다.** 낮은 리스크라 묶어서 여쭤요, 추천대로면 "다 ㅇㅇ"로 답하셔도 됩니다:
  
  1. **표기**: 앱 이름을 소문자 `hide`로 (Dock, Spotlight, 메뉴바, About 전부) - Raycast/슬랙처럼 소문자 브랜드. 추천: `hide`
  2. **Bundle identifier**: 지금 정해야 합니다 (Spotlight/Gatekeeper 식별자). 추천: `me.hoyeon.hide` (혹은 선호 도메인 있으면 알려주세요)
  3. **코드 내부 이름**: Swift 타깃/타입의 `Herdr*` 접두어, `herdr-core` 크레이트명까지 전부 `Hide*`로 바꿀지, 아니면 **앱 껍데기(번들명·표시명·아이콘·Info.plist)만 hide로 하고 내부 코드명은 유지**할지. 추천: 후자 - herdr 멀티플렉서와의 연동 코드가 많아서 내부까지 리네임하면 diff만 커지고 얻는 게 없어요. 내부 리네임은 원하면 후속으로.
  4. **레포 이름** (`herdr-ide` → `hide`): 이번엔 유지 추천 (GitHub 리다이렉트, 로컬 경로, herdr 세션 라벨이 다 걸려 있어서 별도 작업으로 분리).
- recommended: none
- answer: me.grab.hide 로 가자 
  내부 리네임은 나중에
  레포 이름도 우선은 유지~
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q7: 확정: bundle id `me.grab.hide`, 표시명 `hide`, 내부 코드명·레포명은 유지.
- decision_ids: D-12
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:5eb72285-04d6-4850-a4ea-f865b257daed
- asked: 확정: bundle id `me.grab.hide`, 표시명 `hide`, 내부 코드명·레포명은 유지.
  
  **Q7. 앱 아이콘은 어떻게 만들까요?** Spotlight/Dock에 뜨려면 `.icns` 아이콘이 필요한데, 지금 번들에는 아이콘이 없습니다. 브랜드의 얼굴이라 taste 판단이 필요해요:
  
  - **(a) ima2로 후보 생성** - 제가 "hide" 컨셉(herdr + ide, 숨김/미니멀 모티프 등)으로 후보 여러 장을 생성해서 보여드리고, 고르신 걸 macOS 아이콘 규격(1024px, 라운드 사각형)으로 다듬어 적용 *(추천)*
  - (b) 직접 만들어둔 아이콘/로고가 이미 있다 - 파일 위치 알려주시면 그걸 사용
  - (c) 우선 심플한 글리프 아이콘(예: 어두운 배경에 "h" 모노그램)을 코드/SVG로 그려서 넣고, 진짜 아이콘은 나중에
- recommended: none
- answer: 앱 아이콘 ㅇㅇㅇㅇ ima2 후보 생성 고
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q8: 아이콘은 ima2 후보 생성으로 확정.
- decision_ids: D-09, D-13
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:c431b1d5-b726-464b-836d-63c322223de0
- asked: 아이콘은 ima2 후보 생성으로 확정.
  
  **Q8. 사이드바 상단 액션 - 스크린샷의 New Agent / New Terminal / Search 세 개도 이번 스코프인가요?**
  
  스크린샷 상단에 있는 세 버튼이 요청에 명시는 안 됐지만 "사이드 패널 식"의 일부로 보여서요. 추천 스코프:
  
  - **New Agent**: 클릭 → 에이전트 종류(claude/codex) + 대상 체크아웃 선택 → 현재 탭에 pane으로 실행
  - **New Terminal**: 현재 선택된 체크아웃에 일반 터미널 pane 추가
  - **Search**: 사이드바 목록(워크스페이스·에이전트) 필터링만 - 파일 내용 검색이 아니라 "저 목록에서 빨리 찾기" 용도
  
  이대로 셋 다 포함할까요, 아니면 일부는 빼거나 Search 범위를 다르게 볼까요? (파일 검색까지 원하시면 규모가 달라져서 확인이 필요합니다)
- recommended: none
- answer: ㅇㅇㅇ New Workspace, New Agent, Search정도로? 스크린샷은이거 참고 New Agent는  [Image #2] New Workspace는 그냥 폴더(finder) 선택하고 git 없으면 초기화할건지에 기본 설정되어있게 정도? Search는 [Image #3] 대충 이런 흐름으로 되려나? Agent 검색되는거 베이스로~
- immediate_notes: decision: 상단 액션 New Workspace/New Agent/Search, New Terminal 제외 / verified facts: 스크린샷 #2(New Agent 모달: Device·Agent 타일·Space·Bypass permissions 토글), #3(Search 팔레트: 통합 검색·디바이스 뱃지·키보드 네비)를 기준 레퍼런스로 채택 / constraints: New Workspace는 Finder 폴더 선택 + git init 체크박스 기본 ON / source: user Q8 답변+이미지 2·3.
- needs_normalization: false

### Q9: 좋습니다. 상단 액션은 **New Workspace / New Agent / Search**로 확정하고, 스크린샷 두 장을 기준
- decision_ids: D-14
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:8302f57c-52e3-408e-bb73-146f27dc7c5d
- asked: 좋습니다. 상단 액션은 **New Workspace / New Agent / Search**로 확정하고, 스크린샷 두 장을 기준 레퍼런스로 기록합니다:
  
  - **New Agent 모달**: Device 선택(Local/mini) → Agent 타일 → Space 선택 → Options(Bypass permissions 토글 = `--dangerously-skip-permissions`) → Start Agent
  - **New Workspace**: Finder 폴더 선택 + git 없으면 "git 초기화" 체크박스가 **기본 켜짐**으로 노출 (다이얼로그에 보이니 Q4의 "silent init 금지"와 충돌 없음 - 명시적 선택으로 처리)
  - **Search**: 커맨드 팔레트 오버레이, 에이전트·스페이스 통합 검색, 행에 `종류 · 워크스페이스 · 디바이스 뱃지`, ↑↓/⏎ 네비게이션
  
  **Q9. New Agent의 에이전트 타일 - 런칭 시점에 실제로 실행 가능해야 하는 건 어디까지인가요?**
  
  스크린샷에는 claude / codex / gemini / cursor 네 타일이 보이는데, 앞서 favicon 언급은 claude·codex 두 개였어요.
  
  - **(a) claude + codex만 실제 지원** - 타일도 두 개만. gemini/cursor는 나중에 추가 *(추천: 실행 커맨드·상태 파싱이 검증된 두 개만 정식으로, 미검증 타일을 보여주면 외부 사용자에게 깨진 경험)*
  - (b) 네 타일 다 보여주되 gemini/cursor는 "coming soon" 비활성
  - (c) 네 개 다 실제 실행 지원 (gemini CLI, cursor CLI 실행 커맨드 정의 + 상태 표시까지 이번에)
- recommended: none
- answer: 우선 claude, codex만!!!
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q10: claude + codex 두 개만 확정.
- decision_ids: D-15
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:b45ff3ce-92a3-4697-ba91-e399504e0cb7
- asked: claude + codex 두 개만 확정.
  
  **Q10. 디자인 리팩토링의 기준 테마 - 다크 전용으로 갈까요?**
  
  Raycast DESIGN.md를 루트에 넣고 AGENTS.md에서 참조시키는 건 확정 사항으로 기록했고, "리팩토링을 쭉 진행"의 방향을 정해야 합니다. 보여주신 스크린샷(herdr-label)은 전부 다크 톤인데, 현재 앱은 macOS 시스템 색(`windowBackgroundColor`)을 따라 라이트/다크가 같이 움직입니다.
  
  - **(a) 다크 전용** - 스크린샷 톤 그대로, 시스템이 라이트 모드여도 hide는 다크. 디자인 토큰(색·타이포·간격·라운드)을 한 파일로 정의하고 전 화면 적용 *(추천: 터미널 중심 앱은 다크 고정이 자연스럽고, 토큰 세트 하나만 다듬으면 되니 품질을 올리기 쉬움. Warp·Linear 초기도 이 전략)*
  - (b) 다크 우선으로 만들되 라이트 팔레트도 이번에 같이
  - (c) 시스템 라이트/다크 자동 연동 유지
- recommended: none
- answer: 다크 전용으로 우선 가버맂즈아 !
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q11: 체크포인트 1 기록 완료 (D-01~D-16, UX 카드 7장). 스윕에서 가장 큰 미결 갭이 하나 나왔습니다:
- decision_ids: D-17
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:6e44dce0-69c2-4310-af0d-1721a604a420
- asked: 체크포인트 1 기록 완료 (D-01~D-16, UX 카드 7장). 스윕에서 가장 큰 미결 갭이 하나 나왔습니다:
  
  **Q11. 외부 사용자에게 herdr 자체는 어떻게 전달되나요?**
  
  hide의 새 구조(워크스페이스·pane·에이전트·원격)는 전부 herdr 멀티플렉서 위에 서 있는데, 외부 사람은 herdr가 설치되어 있지 않습니다. hide.app만 zip으로 받으면 빈 껍데기가 돼요. 배포 형태를 정해야 합니다:
  
  - **(a) hide.app 안에 herdr 번들** - 앱 실행 시 내장 herdr 데몬을 자동 시작. 외부 사용자는 zip 하나로 끝 *(추천: "외부사람도 쓸 수 있어야"의 취지에 유일하게 부합. 단, herdr 바이너리를 앱에 포함할 수 있는 라이선스/빌드 체계 확인 필요)*
  - (b) herdr 별도 설치 안내 (brew/스크립트) - hide는 접속만
  - (c) 이번엔 herdr 있는 사람(= 사실상 본인·팀)까지만 실사용 대상으로 하고, self-contained화는 후속
  
  herdr가 현재 외부 배포 가능한 상태인지(공개 레포인지, 바이너리 릴리스가 있는지)도 알려주시면 판단이 정확해집니다.
- recommended: none
- answer: 음 리서치 머넞해볼래? 가장 베스트는 이미 깔아놨다면(지금ㄱ처럼) 그거 쓰고 없으면 바이너리 앱에 포함하는게 낫지 않으핚?
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q12: 리서치 결과가 사용자 제안을 그대로 뒷받침합니다:
- decision_ids: D-18
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:14872107-7500-4e11-8ec6-2fd911383828
- asked: 리서치 결과가 사용자 제안을 그대로 뒷받침합니다:
  
  - **herdr는 단일 바이너리**(19MB, arm64 Mach-O)이고, **공개 레포**(github.com/herdrdev/herdr)에 **Apache-2.0 라이선스** + 바이너리 릴리스(v0.8.2)까지 있습니다 → 앱에 번들하는 데 법적·기술적 장애물 없음 (고지문만 동봉하면 됨).
  - hide는 이미 `resolveHerdrBinaryPath()`로 설치된 herdr를 `~/.local/bin` → `/opt/homebrew/bin` → `/usr/local/bin` 순으로 찾고, 없으면 명시적으로 실패합니다. herdr-core도 `herdr_bin_path`를 받아 데몬을 띄우는 구조라, **번들 경로를 후보 마지막에 추가하는 것만으로** 제안하신 동작이 완성됩니다.
  
  그래서 이렇게 확정하겠습니다 (제안 + 리서치 종합):
  
  1. 살아있는 herdr 소켓이 있으면 그 데몬 사용 (기존 세션 보존)
  2. 소켓이 없고 설치된 herdr가 있으면 그것으로 시작
  3. 둘 다 없으면 **hide.app에 번들된 herdr**로 시작 (Apache-2.0 고지 동봉)
  4. 버전 비호환이면 침묵하지 않고 명시적 안내
  
  ---
  
  **Q12. 사이드바 최하단 settings 아이콘 - 안에 뭐가 들어가나요?**
  
  현재 앱에 있는 설정은 pane 단축키 설정 하나뿐입니다. 이번 스코프 추천:
  
  - **(a) 미니멀**: 기존 단축키 설정 + herdr 연결 상태/버전 표시 정도만 settings 창으로 묶기 *(추천: 설정 항목은 필요해질 때 늘리는 게 낫고, 이번 릴리스의 본체는 사이드바·패키징)*
  - (b) 본격 설정 화면: 테마 토큰 조정, 에이전트 기본 옵션(bypass permissions 기본값), 디바이스(SSH 호스트) 관리까지
  
  어느 쪽으로 할까요? (b)에서 특히 **디바이스 관리**(mini 외 SSH 호스트 추가)가 필요하면 알려주세요 - 지금은 mini가 하드코딩되어 있습니다.
- recommended: none
- answer: b까지 해야지 ㅇㅇㅇ
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q13: 확인 결과 하나 더: **herdr-ide 레포에는 git remote가 아예 없습니다** (로컬 전용). 외부 배포 채널이 곧
- decision_ids: D-19
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:7068d319-29c0-455f-a6d1-156549ba113e
- asked: 확인 결과 하나 더: **herdr-ide 레포에는 git remote가 아예 없습니다** (로컬 전용). 외부 배포 채널이 곧 "어디서 받게 할 것인가" 결정과 직결됩니다.
  
  **Q13. (마지막 질문일 것 같아요) 외부 사람은 hide.zip을 어디서 받나요?**
  
  - **(a) 우선 수동 전달** - 빌드 산출물 zip을 에어드랍/드라이브/슬랙으로 직접 공유. 배포 스크립트는 zip 만들기까지만 책임 *(추천: 레포를 GitHub에 올릴지 자체가 별도 결정이라, 이번 스코프는 "배포 가능한 zip 산출물"까지로 긋는 게 깔끔)*
  - (b) GitHub 공개 레포 + Releases - 이번에 레포를 GitHub에 올리고(공개 여부 결정 필요) 릴리스 자동화까지
  
  참고로 나머지 자잘한 것들은 이렇게 기본값으로 넣겠습니다 (거슬리는 것만 말씀해주세요): 마지막 선택한 디바이스/체크아웃은 재시작 시 복원, 첫 실행(워크스페이스 없음)은 New Workspace를 유도하는 빈 상태, PetWindow 등 기존 부가 기능은 그대로 유지.
- recommended: none
- answer: 우선 B로 할듯~
- immediate_notes: decision: 배포 채널은 GitHub 레포 + Releases(선택지 b). 고지된 P2 기본값 3종(선택 복원·첫 실행 빈 상태·PetWindow 유지)에 무이의 → D-20으로 기록 / source: user Q13 답변.
- needs_normalization: false

### Q14: (b) 확정 - GitHub 레포 + Releases 자동화까지 이번 스코프. 다만 코드를 공개로 올리는 건 되돌리기 어려운 외부
- decision_ids: D-19
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:eb37c0e3-b30a-49c4-88f9-80224c76d929
- asked: (b) 확정 - GitHub 레포 + Releases 자동화까지 이번 스코프. 다만 코드를 공개로 올리는 건 되돌리기 어려운 외부 공개라 하나만 정확히 확인합니다.
  
  **Q14. 레포를 어느 계정에, 어떤 공개 범위로 올릴까요?**
  
  - 위치: herdr가 있는 **herdrdev 조직**(`herdrdev/herdr-ide`) vs 개인 계정
  - 공개 범위: **public** (외부인이 Releases에서 zip을 받으려면 public이어야 합니다. private + 릴리스만 공유는 불가)
  
  추천: `herdrdev/herdr-ide` public - herdr 생태계가 한 조직에 모이고, 나중에 `hide`로 레포명 변경도 조직 안에서 리다이렉트됩니다. 코드 공개 전에 시크릿/개인정보 스캔은 제가 배포 작업 단계에서 필수로 넣겠습니다.
- recommended: none
- answer: herdrdev에 올릴수있어? 나 organization 초대안되어이쓴ㄴ데
- immediate_notes: herdrdev org 업로드 가능 여부 질문 → 로컬 확인 결과 yansfil은 herdrdev 비멤버(READ만)로 불가 판명, Q15에서 개인/타 조직 대안 제시 / D-19로 귀결.
- needs_normalization: false

### Q15: 확인 결과: 현재 gh 로그인 계정은 **yansfil**이고, herdrdev 조직에는 멤버가 아니라서(herdr 레포에 REA
- decision_ids: D-19
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:f1350314-4312-44ef-8a1b-bb27dce79e35
- asked: 확인 결과: 현재 gh 로그인 계정은 **yansfil**이고, herdrdev 조직에는 멤버가 아니라서(herdr 레포에 READ 권한만) **herdrdev에는 레포를 만들 수 없습니다.**
  
  **Q15. 그럼 이렇게 할까요?**
  
  - **(a) `yansfil/herdr-ide` public으로 우선 생성** - 지금 바로 가능. 나중에 herdrdev 초대받으면 GitHub transfer로 옮기면 되고, transfer는 기존 URL을 자동 리다이렉트해줘서 배포 링크가 안 깨집니다 *(추천)*
  - (b) herdrdev 관리자에게 초대를 먼저 받고 org에 생성 - 초대 전까지 배포 채널 작업이 블로킹됨
  
  (a)로 가면 이번 스코프가 외부 의존 없이 닫힙니다.
- recommended: none
- answer: modakbul_gongbang/hide 로 가자 ㅎ
- immediate_notes: Imported verbatim from the session transcript; semantic normalization is pending.
- needs_normalization: false

### Q16: 확인: yansfil 계정이 **modakbul-gongbang 조직의 admin**이고 `hide` 이름도 비어 있습니다. `m
- decision_ids: none
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:b068c4ef-b6cf-4d5f-8acc-28b4824927e4
- asked: 확인: yansfil 계정이 **modakbul-gongbang 조직의 admin**이고 `hide` 이름도 비어 있습니다. `modakbul-gongbang/hide` public 레포 + Releases로 확정합니다 (공개 push 직전에 시크릿/개인정보 스캔 + 최종 확인은 구현 단계에 필수로 넣습니다).
  
  이제 남은 재료가 없어 보여서 마감 확인입니다.
  
  **Q16 (마감). 합의된 목표를 한 문장으로 확인해주세요:**
  
  > herdr-ide macOS 앱을 다크 전용 디자인 토큰 기반으로 리브랜딩한 **hide**(번들 id `me.grab.hide`)로 패키징해 Spotlight에서 검색·실행되고 herdr 미설치 외부 사용자도 `modakbul-gongbang/hide` public GitHub Releases의 zip(herdr 번들 포함)으로 쓸 수 있게 하며, 사이드바를 [New Workspace/New Agent/Search 상단 액션 + Workspace(repo)→체크아웃(main/worktree) 트리 + Agents(claude·codex pane만) 감시 목록 + 하단 설정·디바이스(local/mini, 풀 원격 전환)]로 재설계하고, 체크아웃 선택 시 탭·pane 그리드와 우측 파일트리가 함께 전환되게 한다. (+ Raycast DESIGN.md 루트 추가·AGENTS.md 참조, 설정 창은 단축키·테마·에이전트 기본값·SSH 디바이스 관리 포함)
  
  이 문장대로 PRD로 넘기면 될까요? 빠졌거나 다르게 이해된 부분 있으면 지금 말씀해주세요.
- recommended: none
- answer: ㅇㅇ 레스고
- immediate_notes: 마감 확인: 한 문장 목표 재진술에 대한 명시적 동의("ㅇㅇ 레스고"). 새 결정 없음 - 인터뷰 closure 승인.
- needs_normalization: false

### Q17: 라이선스 확인 완료: **awesome-design-md는 MIT** → DESIGN.md를 루트에 복사하는 건 출처·라이선스 고
- decision_ids: D-21, D-22, D-23, D-24, D-25, D-26, D-27
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:2391070f-fc64-48db-9050-4db74f5aee2b
- asked: 라이선스 확인 완료: **awesome-design-md는 MIT** → DESIGN.md를 루트에 복사하는 건 출처·라이선스 고지만 포함하면 문제없습니다.
  
  게이트가 남긴 것 중 **사용자 결정이 필요한 항목 7개**입니다. 각각 추천을 달았으니 번호별로 "ㅇㅇ" 또는 수정만 주시면 됩니다:
  
  **1. (P0) Bypass permissions 토글의 기본값과 안전장치**
  추천: 기본 **OFF**. 켜면 토글 아래 위험 문구(`--dangerously-skip-permissions` - 에이전트가 확인 없이 파일·명령 실행) 상시 노출. 적용 범위는 그 모달로 시작하는 에이전트 1개뿐. 설정(D-18)에서 "기본값 ON"으로 바꿀 수 있지만 바꾸는 순간에도 같은 경고.
  
  **2. (P0) 설정에서 디바이스 삭제 시 실행 중인 원격 에이전트 처리**
  추천: 삭제 = **로컬 등록 해제일 뿐** (detach). 원격 herdr 데몬과 에이전트는 그대로 살아있고, 삭제 확인 다이얼로그에 "실행 중인 에이전트 N개는 mini에서 계속 실행되며, 디바이스를 다시 추가하면 재연결됩니다"를 명시. 원격 작업을 죽이는 파괴 동작은 없음.
  
  **3. (P1) 워크스페이스 목록의 수명 (New Workspace 등록 vs pane 자동 발견의 관계)**
  추천: 워크스페이스 목록 = **영속 등록 목록**(New Workspace로 추가, 컨텍스트 메뉴 Remove로 제거, 재시작 시 복원) **+ 자동 발견 보강**(등록 안 된 폴더에서 pane이 돌면 임시로 표시되고 pane이 없어지면 사라짐). worktree 행만 순수 pane 기반 자동(D-06 유지).
  
  **4. (P1) 원격 디바이스 런칭 스코프**
  추천: **임의 SSH 호스트 지원**이 정식 스코프 (mini는 기본 등록된 첫 사례일 뿐). D-10을 이렇게 일반화.
  
  **5. (P1) New Agent 모달의 Space 필드 의미와 빈 탭 처리**
  추천: Space = 선택한 디바이스의 **워크스페이스+체크아웃** 선택기. Start 시 그 체크아웃의 활성 탭에 pane 추가, 탭이 하나도 없으면 새 탭을 자동 생성.
  
  **6. (P1) 연기 항목의 revisit 조건 명문화**
  추천: 내부 코드명 리네임 → "다음 대규모 리팩토링 시 재검토", gemini/cursor 타일 → "사용자 요청 시" 를 non-goal로 기록.
  
  **7. (P1) Releases 자동화 운영 규칙 + 로고 에셋**
  추천: `v*` git 태그 push → GitHub Actions가 빌드·zip·SHA-256 체크섬 생성 → **draft 릴리스**로 만들고 사용자가 publish (실패 시 발행 안 됨 = 롤백 불요). claude/codex 로고는 공식 배포 에셋을 출처 표기와 함께 번들(식별 목적 사용) - 공개 배포물에 타사 로고가 포함된다는 점만 인지해주세요.
- recommended: none
- answer: ㅇㅇㅇ ㄱㄱ
- immediate_notes: 게이트 1차 BLOCK의 requiresHuman 7건을 추천안 배치로 제시, 사용자가 일괄 승인("ㅇㅇㅇ ㄱㄱ") → D-21~D-27로 확정. 라이선스(MIT)는 gh api로 사전 확인.
- needs_normalization: false

### Q18: 클로저 리뷰가 다시 BLOCK을 반환했고, 계약상 이 사이클은 여기서 종료입니다 (같은 사이클에서 세 번째 판정 호출 금지). 남
- decision_ids: D-17, D-20, D-28, D-29, D-30, D-31, D-32, D-33, D-34, D-35, D-36
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:a6bff6b8-1168-4e6f-aa71-92fcbbb1e11f
- asked: 클로저 리뷰가 다시 BLOCK을 반환했고, 계약상 이 사이클은 여기서 종료입니다 (같은 사이클에서 세 번째 판정 호출 금지). 남은 findings는 전부 사용자 결정 필요 항목이라, 마지막 배치로 정리했습니다. **각 항목에 추천을 달았으니 "전부 ㅇㅇ" 또는 번호별 수정으로 답해주시면, 그 답변을 근거로 게이트를 재개해 마지막 리뷰를 돌립니다.**
  
  **P0 세 개 (판정을 막고 있는 핵심):**
  
  1. **기본값 3종 명시 승인** - 앞서 제가 고지만 하고 넘어간 것들: ①마지막 선택 디바이스/체크아웃 재시작 시 복원 ②첫 실행은 New Workspace 유도 빈 상태 ③PetWindow 등 기존 부가 기능 유지. 이대로 확정?
  2. **SSH 보안 모델** - 추천: hide는 자격증명을 일절 저장·입력받지 않음. 사용자의 `~/.ssh/config` alias + ssh-agent 키 인증만 사용, 비밀번호/패스프레이즈 UI 없음. host key 신뢰는 시스템 `known_hosts` 정책 그대로 따르고, 미신뢰/인증 실패는 원인과 함께 명시적 실패.
  3. **herdr 확보 체인 명시 승인** - Q11에서 제안하신 "있으면 쓰고 없으면 번들"을 제가 3단계로 구체화한 것: ①살아있는 소켓 우선 ②설치된 herdr ③번들 herdr. 여기에 최소 요구 herdr 버전을 명시하고 낮으면 업그레이드 안내. 이 체인대로 확정?
  
  **P1 여덟 개 (추천대로면 그냥 ㅇㅇ):**
  
  4. New Workspace에서 git init 실패 시: 폴더는 non-git 워크스페이스로 추가되고 실패 원인 알림.
  5. Search 대상은 스크린샷대로 **에이전트 + 스페이스(워크스페이스/체크아웃)**만. Enter: 에이전트→pane 점프, 스페이스→체크아웃 전환. 일반 터미널은 비검색.
  6. 설정의 테마 조정은 이번엔 **액센트 컬러 + 폰트 크기만** (즉시 적용, 자동 저장, Reset to defaults 버튼). 토큰 전체 편집은 후속.
  7. 기본 브랜치가 main이 아닌 repo는 실제 브랜치명(master 등)을 그대로 라벨로 표시.
  8. 지원 대상: **macOS 14+ / Apple Silicon(arm64)** 만. Intel은 non-goal (현 빌드도 arm64 전용).
  9. 원격 호스트에 herdr가 없으면 자동 설치하지 않고 설치 명령 안내만 표시.
  10. claude/codex CLI는 PATH+표준 경로에서 탐지, 없으면 타일에 "설치 필요" 표시+링크. 인증은 각 CLI 자체 로그인에 위임 (hide는 무관여).
  11. claude/codex 로고는 구현 단계에서 공식 브랜드 가이드라인을 확인해 허용 범위 에셋만 사용, 불허 시 자체 글리프 대체 - 검증 태스크로 PRD에 포함.
- recommended: none
- answer: 1. ㅇㅇ PetWindow는 우선 유지 
  2. ㅇㅇㅇ 그렇게
  3. ㅇㅇ 그렇게 레스고
  4. ㅇㅇㅇ
  5. ㅇㅇ
  6. 오키 그정도만
  7. ㅇㅇㅇㅇ
  8. ㅇㅇ
  9. ㅇㅇㅇ 그렇게 ㄱㄱ
  10. ㅇㅇㅇ
  11. ㅇㅇㅇㅇ
- immediate_notes: 클로저 BLOCK의 findings 11건을 번호별 추천안으로 제시, 사용자가 1~11 전 항목을 개별 명시 승인. D-20을 assumption→user decision으로 승격, D-17 체인 명시 승인, D-28~D-36 신규 확정. 이 답변이 게이트 reopen 근거.
- needs_normalization: false

### Q19: 사이클 2의 1차 리뷰: **BLOCK이지만 findings가 14개 → 5개로 줄었습니다.** 검증 항목 1개는 제가 직접 고치
- decision_ids: D-37, D-38, D-39, D-40
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:b5f28190-0a15-402f-a4e9-2be26440f00d
- asked: 사이클 2의 1차 리뷰: **BLOCK이지만 findings가 14개 → 5개로 줄었습니다.** 검증 항목 1개는 제가 직접 고치고, 나머지 4개만 마지막으로 확인받겠습니다. 이번 답변 후 클로저 리뷰 1회로 마감됩니다.
  
  **1. herdr 버전 계약 구체화** - 추천: 번들 herdr = **v0.8.2 고정**(GitHub 릴리스 바이너리, SHA-256 기록). 최소 지원 버전 = 번들과 동일(0.8.2). 설치본이 그보다 낮으면 번들로 기동하고 업그레이드 안내. 원격 호스트가 미달이면 업그레이드 명령 안내만(자동 변경 없음).
  
  **2. 에이전트 CLI 버전·실행 환경** - 추천: claude/codex는 버전 하한을 강제하지 않고 현행 안정판 기준으로 지원. 실행 실패는 pane에 출력 그대로 노출. 실행 환경은 사용자 로그인 셸 환경을 상속(Finder 런치에서도 셸 PATH 로드 - 현행 herdr pane 방식 유지).
  
  **3. (P0) Workspace Remove의 의미** - 추천: Remove = **목록 등록 해제만, 디스크 파일은 절대 삭제하지 않음**. 해제 후에도 그 폴더에서 pane이 돌고 있으면 자동 발견(D-23)에 의해 임시 항목으로 계속 표시됨. 확인 다이얼로그에 "파일은 삭제되지 않습니다" 명시.
  
  **4. 재연결 계약** - 추천: 상태의 주인은 herdr 데몬. hide 재시작·SSH 재연결 시 기존 pane에 재attach하고, 출력 이력은 herdr가 보존하는 범위만큼 복원. hide가 자체적으로 세션을 복제·저장하지 않음.
  
  4개 다 추천대로면 "ㅇㅇ"만 주세요.
- recommended: none
- answer: 1. 음 우선 그렇게해야곘지? 근데 이거 herdr 버전 높아지는데 이거 고정만 되버리면 어떻게해? 원래 이런건가 
  2. ㅇㅇㅇ
  3. ㅇㅇㅇ 그렇게가야지 결국 개념이 그런거임
  4. ㅇㅇㅇ
- immediate_notes: 사이클2 1차 findings 4건 승인. 1번은 "영구 고정이냐" 질문 → 릴리스마다 갱신되는 빌드 시점 고정 + 런타임은 소켓·설치본 우선이라 번들은 바닥일 뿐이라는 설명 후 수용. 2·3·4는 즉시 승인.
- needs_normalization: false

### Q20: 확인 완료 - herdr는 macOS·Linux·Windows 릴리스 바이너리와 공식 설치 원라이너를 제공합니다. 마지막 2개입니
- decision_ids: D-41, D-42
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:f8ba49b7-a01d-4527-9ce4-2acf6b098b8e
- asked: 확인 완료 - herdr는 macOS·Linux·Windows 릴리스 바이너리와 공식 설치 원라이너를 제공합니다. 마지막 2개입니다:
  
  **1. 원격 호스트 지원 범위 + 설치 안내 명령** - 추천: 원격 SSH 호스트는 **macOS·Linux (arm64/x86_64)** 지원 - herdr 공식 릴리스가 있는 플랫폼 그대로. herdr 미설치 호스트에는 공식 설치 명령을 그대로 안내: `curl -fsSL https://herdr.dev/install.sh | sh` (brew 대안 병기). Windows 호스트는 non-goal.
  
  **2. 재attach 실패(원격 pane/세션이 이미 소멸) 처리** - 추천: 복구 시도 없음. 소멸한 pane 행은 목록에서 제거하고 "원격에서 세션이 종료되었습니다" 안내, 사이드바는 현재 원격 상태로 재동기화. herdr가 보존하지 않은 출력 이력은 소실됨을 명시 (hide는 자체 이력 저장을 하지 않으므로, D-40과 일관).
  
  둘 다 추천대로면 "ㅇㅇ" 주세요 - 그 답변으로 게이트 재개 후 최종 리뷰 돌립니다.
- recommended: none
- answer: ㅇㅇㅇ
- immediate_notes: 사이클2 클로저 잔여 2건(원격 플랫폼·설치 명령, 재attach 실패 처리)을 herdr 릴리스 확인 근거와 함께 제시, 즉시 승인 → D-41, D-42.
- needs_normalization: false

### Q21: 확인됨: herdr 공식 인스톨러는 최신 릴리스를 받아 기존 바이너리 위에 덮어쓰는 방식이라, **업그레이드 = 같은 원라이너 재
- decision_ids: D-43
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:1769886d-886b-4888-8866-c8d18e5462e4
- asked: 확인됨: herdr 공식 인스톨러는 최신 릴리스를 받아 기존 바이너리 위에 덮어쓰는 방식이라, **업그레이드 = 같은 원라이너 재실행**입니다.
  
  **마지막 확인 (이거 하나로 게이트 끝):** 버전 미달 시 안내하는 업그레이드 명령은 설치 때와 동일한 `curl -fsSL https://herdr.dev/install.sh | sh` (brew로 설치된 경우 `brew upgrade herdr` 병기), 로컬·원격 호스트 모두 동일 안내. 이대로 ㅇㅇ?
- recommended: none
- answer: ㅇㅇ
- immediate_notes: 사이클3 잔여 1건(업그레이드 명령): install.sh가 덮어쓰기 방식임을 소스로 확인한 뒤 동일 원라이너 재실행안을 제시, 즉시 승인 → D-43.
- needs_normalization: false

### Q22: **인터뷰 완료 - gap-audit 게이트 PASS(봉인), qa-log가 PRD 소스로 확정됐습니다.** 🎉
- decision_ids: D-44, D-45
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:2331ff67-fbef-4b89-9979-8059a5a72c91
- asked: **인터뷰 완료 - gap-audit 게이트 PASS(봉인), qa-log가 PRD 소스로 확정됐습니다.** 🎉
  
  최종 상태:
  - **`agents/interview/hide-rebrand/qa-log.md`** - status: complete, 질문 21개, Decision Register 43행(열린 P0/P1 없음), UX 시나리오 카드 8장, 근거 15건
  - 게이트는 3사이클 만에 통과: 1차 리뷰의 14건 → 사용자 승인 배치 2회로 해소 → 잔여 5건 → 2건 → 1건 → **PASS**
  
  **확정된 핵심 골격:**
  
  | 영역 | 결정 |
  |---|---|
  | 브랜딩 | `hide` / `me.grab.hide`, ima2 아이콘, 내부 코드명·로컬 디렉토리명은 유지 |
  | 패키징 | release 번들 + /Applications(Spotlight), 미서명 zip, herdr 소켓→설치본→번들(v0.8.2 고정, 릴리스마다 갱신) 폴백 체인 |
  | 배포 | `modakbul-gongbang/hide` public + `v*` 태그 → draft 릴리스 + 체크섬, 공개 전 시크릿 스캔·최종 승인 게이트 |
  | 사이드바 | New Workspace/New Agent/Search + Workspace(repo)→체크아웃(main/⑂worktree) + Agents(claude·codex pane만) + 설정·디바이스 |
  | 아키텍처 | 체크아웃 선택 = 풀 컨텍스트 전환(탭·pane·파일트리), 원격도 동일 UX(임의 SSH 호스트, 무자격증명 모델) |
  | 디자인 | 다크 전용 토큰, Raycast DESIGN.md(MIT) 루트 추가 + AGENTS.md 참조 |
  
  다음 단계:
  
  ```
  /gen-prd --context agents/interview/hide-rebrand/qa-log.md "hide 리브랜딩·패키징·사이드바 재설계"
  ```
  
  바로 진행할까요?
- recommended: none
- answer: <command-message>gen-prd</command-message>
  <command-name>/gen-prd</command-name>
  <command-args>--context agents/interview/hide-rebrand/qa-log.md "hide 리브랜딩·패키징·사이드바 재설계"</command-args>
- immediate_notes: spec gate 1차 findings 중 사용자 결정 3건을 질문: 끊김 동작 (b) 선택, mini 한정 검증 승인, AC15 화면 인벤토리 승인 → D-44, D-45.
- needs_normalization: false

### Q23: qa-log 기록 복원 4건 적용 완료. **판정관이 사용자 결정을 요구한 3건**입니다:
- decision_ids: none
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:6c8b28f1-5d99-496d-80fb-62e43a3fdcae
- asked: qa-log 기록 복원 4건 적용 완료. **판정관이 사용자 결정을 요구한 3건**입니다:
  
  **1. 원격 연결이 끊겼을 때 동작** - PRD의 "로컬로 안전 복귀"가 인터뷰에서 명시 승인된 적 없다는 지적입니다 (제가 UX 카드에 쓴 recovery였어요). 둘 중 확정해주세요:
  - **(a) 로컬로 자동 복귀** - 끊기면 즉시 로컬 컨텍스트로 전환, 재연결되면 원격 pane 재attach *(카드에 있던 안)*
  - (b) 원격 컨텍스트에 머물며 "연결 끊김" 상태 + 재연결 시도 표시, 로컬 전환은 사용자가 직접
  
  **2. 원격 플랫폼 검증 범위** - R7은 macOS·Linux SSH 호스트를 선언하는데 실기기 검증은 mini(macOS arm64)뿐입니다:
  - **(a) mini만 실기기 검증** - Linux는 지원 선언을 유지하되 "이번 릴리스에서 실기기 미검증"을 리스크로 명시 *(추천: 검증 가능한 Linux 호스트가 현재 없음)*
  - (b) Linux 호스트를 하나 확보해서 검증 추가 (있으면 호스트 알려주세요)
  
  **3. AC15 "전 화면 다크 렌더"의 화면 인벤토리 확정** - 검증 가능하도록 유한 목록으로 못박습니다: 메인 창(사이드바+탭·pane 그리드+파일트리+상태바), 빈 체크아웃 상태, 첫 실행 빈 상태, New Agent 모달, New Workspace 다이얼로그, Search 팔레트, 설정 창 5개 섹션, Remove·디바이스 삭제 다이얼로그. **이 목록 = AC15의 전부**로 확정할까요?
  
  1은 (a)/(b) 선택, 2·3은 추천대로면 "ㅇㅇ" 주세요.
- recommended: none
- answer: 1. b 2-3은 ㅇㅇ
- immediate_notes: gap-audit 재개·qa-log 정합 작업의 상태 보고 턴 - 새 결정 없음, D-44 반영 작업의 일부.
- needs_normalization: false

### Q24: 남은 2건은 사용자 결정입니다. 추천을 달았으니 확인해주세요:
- decision_ids: D-46, D-47
- route: mixed
- source_ref: claude:ea9f12f2-8227-44ca-b8a9-96e2e5955ab7:fd9dd5ac-5751-4401-a40c-6dcd6a8cb4cd
- asked: 남은 2건은 사용자 결정입니다. 추천을 달았으니 확인해주세요:
  
  **1. Bypass permissions 토글의 에이전트별 플래그 매핑** - 지금까지 `--dangerously-skip-permissions`(claude 플래그)만 기록되어 있는데 codex는 플래그가 다릅니다. 추천:
  - claude → `--dangerously-skip-permissions`
  - codex → `--dangerously-bypass-approvals-and-sandbox` (로컬 codex CLI에서 실제 플래그 확인 완료)
  - 경고 문구는 공통("에이전트가 확인 없이 파일·명령을 실행합니다"), 토글 하나가 선택된 에이전트에 맞는 플래그로 번역됨.
  
  **2. 기존 HerdrIDE 설치의 업그레이드/마이그레이션** - 추천: **마이그레이션 없음**. 근거: 기존 설치는 본인 dev 빌드뿐(외부 배포 이력 없음)이고, 현재 UI 상태 저장도 `/tmp` 검증용 경로가 기본이라 이관할 영속 데이터가 사실상 없습니다. hide가 자체 영속 경로(예: Application Support/me.grab.hide)를 새로 정의하고, 옛 dev 앱(HerdrIDE.app)은 수동 삭제하면 됩니다. herdr 데몬 쪽 세션·워크스페이스 상태는 herdr 소유라 앱 교체와 무관하게 유지됩니다.
  
  둘 다 추천대로면 "ㅇㅇ" 주세요 - 반영 후 클로저 리뷰 → spec gate 재개까지 한 번에 돌립니다.
- recommended: none
- answer: ㅇㅇㅇ
- immediate_notes: gap-audit 사이클4 findings 2건(에이전트별 bypass 플래그 매핑, 기존 설치 마이그레이션)을 codex --help·CoreBridge 경로 확인 근거와 함께 제시, 즉시 승인 → D-46, D-47.
- needs_normalization: false

## UX Scenario Cards

### UX-01: 체크아웃(워크스페이스/워크트리) 전환
- trigger: 사이드바 WORKSPACES에서 main 또는 ⑂worktree 행 클릭
- happy path: 메인 영역이 해당 체크아웃의 탭 스트립 + pane 그리드로 전환되고, 우측 파일트리 루트가 그 체크아웃 경로로 바뀐다. 상단 [+]로 새 탭 추가 가능
- state / failure: 체크아웃에 열린 탭이 없으면 빈 상태(새 탭/터미널 시작 유도). worktree는 마지막 pane이 닫히면 목록에서 사라짐 - 그 worktree를 보고 있었다면 부모 workspace의 main으로 복귀
- recovery: herdr 연결 끊김 시 사이드바에 연결 상태 문구(기존 emptyAgentsDescription 패턴 유지)
- proof: 실행 앱 스크린샷으로 전환 전/후 파일트리 루트와 탭 구성 비교 (peekaboo/screencapture)
- linked decisions: D-06, D-07

### UX-02: Agents 감시 목록
- trigger: claude/codex 에이전트 pane이 어느 체크아웃에서든 실행 중
- happy path: 사이드바 AGENTS 섹션에 herdr-label과 동일한 순서·상태로 행이 뜬다: favicon + 이름 + 상태 스피너/심볼 + 폴더 라벨. 행 클릭 시 해당 워크스페이스→체크아웃→탭으로 점프하고 pane 포커스
- state / failure: 일반 터미널 pane은 목록에 나타나지 않음. 에이전트 pane 종료 시 done 상태 표기 후 목록 규칙(sortRank)에 따라 정렬
- recovery: 점프 대상 pane이 이미 닫혔으면 해당 행 제거
- proof: 에이전트 2개+터미널 2개 탭 구성에서 AGENTS 섹션에 에이전트 2개만 표시되는 스크린샷. claude와 codex를 각각 실제 실행해 favicon·상태·폴더 라벨이 모두 뜨는지 확인
- linked decisions: D-08, D-14, D-16

### UX-03: New Agent 시작
- trigger: 사이드바 상단 New Agent 클릭
- happy path: 모달(스크린샷 #2): Device(Local/mini) → Agent 타일(claude/codex) → Space 선택 → Bypass permissions 토글(선택 에이전트의 플래그로 번역: claude --dangerously-skip-permissions / codex --dangerously-bypass-approvals-and-sandbox, D-46) → Start Agent. 시작되면 대상 체크아웃의 현재 탭에 pane 생성 + AGENTS 섹션에 등장
- state / failure: 선택한 디바이스가 연결 불가면 Start 비활성 + 원인 문구. CLI 미설치면 해당 타일에 '설치 필요' 상태 + 설치 링크(D-35). 에이전트 CLI 실행 실패 시 pane에 에러 출력이 그대로 보임
- recovery: 모달 Cancel로 무변경 종료
- proof: 모달로 claude·codex 각각 실제 시작해 pane + AGENTS 행 생성 확인. Bypass 토글 기본 OFF와 ON 시 경고 문구 노출 스크린샷. 탭 없는 체크아웃에서 시작 시 새 탭 자동 생성 확인
- linked decisions: D-13, D-14, D-21, D-25

### UX-04: New Workspace 추가
- trigger: 사이드바 상단 New Workspace 클릭
- happy path: Finder 폴더 선택 → git repo면 바로 WORKSPACES에 추가. git이 없으면 'Initialize git repository' 체크박스(기본 ON)가 보이는 확인 단계 후 추가
- state / failure: git 없는 폴더에서 체크박스를 끄고 추가하면 worktree 계층 없는 평평한 행. 이미 목록에 있는 폴더 선택 시 기존 행 선택으로 처리. Remove는 등록 해제만 - 파일 미삭제를 다이얼로그에 명시, pane이 남아 있으면 자동 발견으로 임시 표시 지속(D-39)
- recovery: 취소 시 무변경. init 실패(권한 등) 시 폴더는 non-git 워크스페이스로 추가되고 실패 원인 알림 표시(D-29)
- proof: git 없는 임시 폴더로 추가 플로우 실행, init 체크 ON/OFF 각각의 결과 확인
- linked decisions: D-09, D-13, D-23, D-29, D-39

### UX-05: Search 팔레트
- trigger: 사이드바 Search 클릭 (또는 단축키)
- happy path: 오버레이 팔레트(스크린샷 #3)에 에이전트 + 스페이스(워크스페이스/체크아웃) 통합 목록(일반 터미널 비검색, D-30), 타이핑으로 필터, 행에 종류·워크스페이스·디바이스 뱃지, ↑↓ 이동, ⏎는 에이전트→pane 점프·스페이스→체크아웃 전환, esc 닫기
- state / failure: 매칭 없음 상태 표시. 원격 항목은 디바이스 뱃지(mac-mini)로 구분
- recovery: esc로 무변경 닫기
- proof: 검색어 입력→필터→⏎로 해당 에이전트 pane 점프 확인. 행의 종류·워크스페이스·디바이스 뱃지(Local/원격)가 데이터와 일치하는지, 매칭 없음 상태 표시 확인
- linked decisions: D-13

### UX-06: 디바이스 전환 (local ↔ mini)
- trigger: 사이드바 최하단 디바이스 영역에서 mini 선택
- happy path: WORKSPACES가 mini의 워크스페이스 목록으로 전환, 체크아웃 선택 시 원격 pane·파일트리가 로컬과 동일 UX로 동작
- state / failure: SSH 불가·인증 실패·미신뢰 host key는 원인별 문구로 명시적 실패(D-28). 원격 herdr 미설치면 설치 명령 안내만 표시(D-34). 연결 상태 점(색)으로 상시 표시
- recovery: 연결 끊김 시 원격 컨텍스트에 머물며 '연결 끊김' 상태와 재연결 시도를 표시, 로컬 전환은 사용자 수동(D-44). 재연결 시 기존 pane에 재attach(D-40). 세션이 이미 소멸했으면 행 제거 + '원격에서 세션 종료' 안내 + 목록 재동기화, 복구 시도 없음(D-42)
- proof: 실제 mini에 SSH로 워크스페이스 목록 로드 + 원격 pane 입출력 + 원격 체크아웃 선택 시 파일트리 전환 스크린샷. SSH 강제 절단 후 상태 문구 표시와 재연결 시 pane 재attach 확인
- linked decisions: D-04, D-10, D-24, D-28, D-34, D-40, D-44

### UX-07: 외부 사용자 설치
- trigger: hide.zip 다운로드
- happy path: 압축 해제 → hide.app을 /Applications로 이동 → 우클릭 열기(동봉 안내) → 실행. Spotlight에서 "hide" 검색으로 실행 가능
- state / failure: 더블클릭 시 Gatekeeper "손상됨/확인 불가" 경고 - 동봉 README의 우클릭 열기 또는 xattr 한 줄로 해소
- recovery: 실패 시에도 앱/데이터 손상 없음
- proof: 격리 속성이 있는 zip을 새 사용자 계정(또는 xattr 복원)에서 열어 안내대로 실행되는지 확인 + Spotlight 검색 스크린샷. herdr 미설치 환경(PATH·설치 경로 격리)에서 번들 herdr로 데몬이 자동 기동되는지, 설치된 herdr가 있으면 그것이 우선되는지 폴백 체인 확인. 릴리스는 v* 태그로 draft 생성→체크섬 파일 존재→publish 전 시크릿 스캔 기록 확인
- linked decisions: D-05, D-11, D-12, D-17, D-19, D-27

### UX-08: 설정 - 디바이스(SSH 호스트) 관리
- trigger: 사이드바 최하단 settings 아이콘 → 설정 창 → Devices 섹션
- happy path: 등록된 디바이스 목록(local 고정 + SSH 호스트들)과 연결 상태 표시. 추가: 이름 + SSH 호스트(~/.ssh/config alias) 입력 → 연결 테스트 → 저장하면 사이드바 하단 목록에 반영. 자격증명은 저장·입력받지 않음: 키 인증은 ssh-agent, host key는 시스템 known_hosts 정책(D-28)
- state / failure: 연결 테스트 실패 시 원인(SSH 불가/원격 herdr 없음)을 구분해 표시하고 저장은 허용(나중에 연결될 수 있음). 삭제는 detach일 뿐(D-22): 다이얼로그에 '실행 중인 에이전트 N개는 원격에서 계속 실행, 재추가 시 재연결' 명시
- recovery: 삭제는 확인 후 실행, 설정은 되돌릴 수 있음(재추가)
- proof: 설정 창의 각 섹션(단축키·herdr 상태·테마·에이전트 기본값·디바이스)이 렌더되고 값 변경이 반영되는지 확인. 액센트 컬러·폰트 크기 변경이 즉시 화면에 적용되는지, 앱 재시작 후에도 유지되는지, Reset to defaults가 기본값으로 복원하는지 확인(D-31). mini를 삭제→다이얼로그의 detach 문구 확인→원격 에이전트가 mini에서 계속 실행 중임을 herdr agent list로 검증→재추가 후 재연결 확인
- linked decisions: D-18, D-10, D-22, D-24, D-28, D-31

## Evidence From Code, Docs, Or Research

- macos/scripts/build_dev_app.sh:6-30 - debug 빌드를 HerdrIDE.app으로 조립, ad-hoc 서명만 수행 (D-01)
- macos/Sources/HerdrMacOS/ShellView.swift:28-58 - HSplitView 3패널 구조 (D-02)
- macos/Sources/HerdrMacOS/CoreBridge.swift:117-141 - SidebarAgent 필드(workspaceLabel, agentKind, state, sortRank 등) (D-03)
- macos/Sources/HerdrMacOS/CoreBridge.swift:338-355 - --workspace-root 단일 워크스페이스 런치 (D-02)
- macos/Sources/HerdrMacOS/OperationalModels.swift:396-475 - mini 원격 워크스페이스 조회(ssh mini herdr workspace list) (D-04)
- AGENTS.md Repository Layout - src/remote.rs는 아직 미포팅 상태로 명시 (D-10 스코프 근거)
- 사용자 제공 스크린샷 #1(herdr-label 사이드바), #2(New Agent 모달), #3(Search 팔레트) - UI 기준 레퍼런스 (D-08, D-13)
- Raycast DESIGN.md 원본: https://github.com/VoltAgent/awesome-design-md/blob/main/design-md/raycast/DESIGN.md (D-15)
- which herdr / gh 조회 - herdr 0.8.2 단일 19MB arm64 바이너리(~/.local/bin), 공개 레포 herdrdev/herdr, Apache-2.0, v0.8.2 릴리스 존재 (D-17)
- macos/Sources/HerdrMacOS/CoreBridge.swift:662-676 - 소켓 기본 경로(~/.config/herdr/herdr.sock)와 herdr 바이너리 후보 체인 해석 (D-17)
- git remote -v - herdr-ide는 remote 없는 로컬 전용 레포 (D-19)
- gh api - yansfil은 herdrdev 비멤버(herdr에 READ만), modakbul-gongbang org admin(active), modakbul-gongbang/hide 이름 미점유 (D-19)
- gh api repos/VoltAgent/awesome-design-md - 라이선스 MIT 확인: DESIGN.md 복사는 라이선스 고지 포함 시 허용 (D-27)
- gh release view v0.8.2 -R herdrdev/herdr - macos/linux/windows 각 아키텍처 바이너리 제공, README 공식 설치 원라이너 curl -fsSL https://herdr.dev/install.sh | sh 확인 (D-41)
- https://herdr.dev/install.sh 소스 - 최신 릴리스를 받아 기존 바이너리를 mv로 덮어씀: 재실행이 곧 업그레이드 (D-43)

## Documented Domain Checks

- docs inspected:
- canonical terms:
- glossary or code conflicts:
- concrete scenarios tested:
- docs mutation:
- ADR candidate:

## Checkpoint And Sweep History

### Checkpoint 1
- after_question: Q10
- normalized_entries: Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10
- register_changes: D-05~D-15 사용자 결정 11건 + D-16 P2 기본값 등록: 미서명 zip 배포, 워크스페이스=repo/워크트리=체크아웃 모델, 풀 컨텍스트 전환, Agents=에이전트 pane만, git init 명시적, 원격(mini) 풀 지원, hide/me.grab.hide 브랜딩, ima2 아이콘, 상단 액션 3종+모달·팔레트 레퍼런스, claude+codex만, 다크 전용 토큰. UX-01~07 카드 작성
- reopened_decisions: 없음
- highest_remaining_gap: 외부 사용자에게 herdr 데몬/CLI 의존성을 어떻게 배포하나 - hide.app이 self-contained인지 미결

### Checkpoint 2
- after_question: Q16
- normalized_entries: Q11, Q12, Q13, Q14, Q15, Q16
- register_changes: D-17(herdr 데몬 확보 체인+번들), D-18(설정 본격: 단축키·테마·에이전트 기본값·SSH 디바이스 관리), D-19(modakbul-gongbang/hide public + Releases + 공개 전 시크릿 스캔·최종 승인 게이트), D-20(P2 기본값 3종), D-11 수정(원격 레포명 hide). UX-08 카드, 리서치 근거 4건 추가
- reopened_decisions: 없음
- highest_remaining_gap: 없음 - 마감 재진술에 사용자 동의 완료

### Checkpoint 3
- after_question: Q17
- normalized_entries: Q17
- register_changes: 게이트 1차 findings 해소: D-21(bypass 기본 OFF+경고), D-22(디바이스 삭제=detach), D-23(워크스페이스 영속 등록+자동 발견), D-24(임의 SSH 호스트 스코프, D-10 정합 수정), D-25(Space=워크스페이스+체크아웃, 빈 탭 자동 생성), D-26(연기 항목 non-goal+revisit), D-27(v* 태그→draft 릴리스+체크섬, MIT·로고 에셋 승인). UX-02/03/05/06/07/08 proof 보강
- reopened_decisions: 없음
- highest_remaining_gap: 없음

### Checkpoint 4
- after_question: Q18
- normalized_entries: Q18
- register_changes: 클로저 BLOCK findings 11건 전부 사용자 명시 승인으로 해소: D-20 승격(수명 기본값 3종), D-28(SSH 무자격증명 모델), D-17 체인 명시 승인+최소 버전, D-29(init 실패), D-30(Search 대상·Enter), D-31(테마=액센트+폰트), D-32(실제 기본 브랜치명), D-33(macOS14+/arm64), D-34(원격 herdr 안내만), D-35(CLI 탐지·인증 위임), D-36(로고 가이드라인 검증). UX-03/04/05/06/08 정합 수정
- reopened_decisions: 없음
- highest_remaining_gap: 없음

### Checkpoint 5
- after_question: Q19
- normalized_entries: Q19
- register_changes: 사이클2 1차 findings 해소: D-37(릴리스별 갱신되는 번들 버전 고정+최소 버전), D-38(CLI 버전 하한 없음·로그인 셸 환경 상속), D-39(Remove=등록 해제만·파일 미삭제), D-40(재연결=herdr가 상태 주인·재attach). UX-08 설정 proof에 즉시 적용·재시작 지속·Reset 검증 추가, UX-04 Remove 의미 반영
- reopened_decisions: 없음
- highest_remaining_gap: 없음

### Checkpoint 6
- after_question: Q20
- normalized_entries: Q20
- register_changes: 사이클2 클로저 잔여 해소: D-41(원격 플랫폼 macOS·Linux + 공식 설치 명령, Windows non-goal), D-42(소멸 세션은 복구 없이 제거·안내·재동기화). UX-06 recovery 반영, herdr 릴리스 근거 추가
- reopened_decisions: 없음
- highest_remaining_gap: 없음

### Checkpoint 7
- after_question: Q21
- normalized_entries: Q21
- register_changes: D-43(업그레이드 명령 = 설치 원라이너 재실행, brew 병기, 로컬·원격 동일)
- reopened_decisions: 없음
- highest_remaining_gap: 없음

### Checkpoint 8
- after_question: Q23
- normalized_entries: Q22, Q23
- register_changes: spec gate에서 발생한 사용자 결정 반영: D-44(끊김 시 원격 컨텍스트 유지·수동 전환, UX-06 recovery 대체), D-45(원격 검증 mini 한정 + AC15 화면 인벤토리)
- reopened_decisions: UX-06 recovery (D-44로 대체)
- highest_remaining_gap: 없음

### Checkpoint 9
- after_question: Q24
- normalized_entries: Q24
- register_changes: gap-audit 사이클4 findings 반영: D-46(에이전트별 bypass 플래그 매핑), D-47(기존 설치 마이그레이션 없음), D-41/D-43 파이프 이스케이프 수정. UX-03 카드에 플래그 매핑 반영
- reopened_decisions: 없음
- highest_remaining_gap: 없음

## Audit History

### Audit 1
- type: gap-audit-gate
- result: fail
- missing decision_ids: 워크스페이스 수명, SSH 스코프, bypass 기본값, 디바이스 삭제, Space 매핑, 연기 항목 revisit, 릴리스 운영, 에셋 라이선스, 검증 누락 (사이클1 1차 14건)
- unsupported assumptions: D-20(수명 기본값)이 명시 승인 없는 agent default
- UX or behavior gap: Search Enter 동작, 설정 테마 컨트롤, git init 실패 경로
- highest-risk blocker: bypass permissions 기본값·경고(P0), SSH 자격증명 모델(P0)
- final-blocking-question: Q17~Q18 배치로 사용자에게 전달
- PRD impact: D-21~D-36 확정으로 해소

### Audit 2
- type: gap-audit-gate
- result: fail
- missing decision_ids: herdr 버전 계약 구체값, CLI 버전·환경, Workspace Remove 의미, 재연결 계약 (사이클2 1차 5건 → 클로저 4건: 원격 플랫폼·설치/업그레이드 명령, 소멸 세션 처리)
- unsupported assumptions: 없음
- UX or behavior gap: 설정 proof의 즉시 적용·persistence·reset 검증
- highest-risk blocker: Workspace Remove가 파일 삭제로 오인될 여지(P0)
- final-blocking-question: Q19~Q21 배치로 사용자에게 전달
- PRD impact: D-37~D-43 확정으로 해소

### Audit 3
- type: gap-audit-gate
- result: pass
- missing decision_ids: 없음
- unsupported assumptions: 없음
- UX or behavior gap: 없음
- highest-risk blocker: 없음
- final-blocking-question: 없음
- PRD impact: 사이클3 라운드2 PASS·sealed - qa-log를 PRD 소스로 확정

### Audit 4
- type: gap-audit-gate
- result: pass
- missing decision_ids: 사이클4 1차에서 3건(설치 명령 파이프 표기 오류, 에이전트별 bypass 플래그 매핑, 기존 설치 마이그레이션) 발견 → D-46·D-47 확정과 표기 수정으로 해소
- unsupported assumptions: 없음
- UX or behavior gap: 없음
- highest-risk blocker: codex bypass 플래그 미정의(P0) - D-46으로 해소
- final-blocking-question: Q24 배치로 사용자에게 전달
- PRD impact: spec gate 파생 D-44·D-45 + 사이클4 D-46·D-47 반영, 클로저 라운드2 PASS·재봉인
