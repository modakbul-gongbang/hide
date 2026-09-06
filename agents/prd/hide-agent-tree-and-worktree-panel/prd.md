---
topic: "Hide agent lineage tree in the sidebar and a Git worktree section in the right panel"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "Worktree deletion now closes Herdr panes first and may delete the local branch, which destroys operator work if the gate is wrong, and one task lands a commit in a second repository (sasu) through a delegated agent."
source_intake: "agents/interview/hide-agent-tree-and-worktree-panel/qa-log.md"
created_at: "2026-09-06"
updated_at: "2026-09-06"
---

# PRD: 사이드바 에이전트 계보 트리와 우측 패널 Git 섹션

## 1. Summary

두 가지를 바꾼다.

첫째, 사이드바의 프로젝트 트리가 Herdr의 에이전트 계보(`spawned_from_pane_id`)를 읽어 파생된 에이전트를 부모 아래에 들여쓴다.
sasu의 Observer 아래에 Implementor가, 그 아래에 escalate된 solver가 보인다.
계보의 단일 원천은 Herdr이며, 오늘 sasu의 dispatch가 `--from-pane`을 넘기지 않아 계보가 비어 있으므로 그 dispatch를 고치는 일도 이 PRD의 task다.

둘째, 우측 패널에 세 번째 섹션 **Git**을 추가한다.
포커스된 프로젝트의 모든 worktree(pane이 없는 것 포함)를 한 줄씩 보여 주고, 각 행에 병합 여부, 기준 브랜치 대비 ahead/behind, 변경 여부, push 여부, 디스크 용량, GitHub PR 상태를 붙인다.
행에서 열기, Finder에서 보기, 경로 복사, 기준 브랜치 지정, 삭제를 할 수 있다.
삭제는 하나의 core 규칙이 세 표면(카드, 사이드바 메뉴, Git 섹션)을 함께 지키며, 열린 pane을 Herdr로 먼저 닫고, 병합된 브랜치는 선택적으로 함께 지운다.

이 PRD는 `main`(caaa665) 위에 쓴다.
main에는 project-panel PRD가 이미 worktree 읽기, gh PR 읽기, 디스크 측정, checkout 카드를 실었으므로 이 PRD는 그 모듈을 확장하고, 인터뷰가 그 PRD의 결정을 뒤집은 지점을 3장과 4.3에 명시한다.

Approval checklist:

- 범위 경계: 계보 트리(R1-R2), sasu dispatch 수정의 위임 실행(R3), Git 섹션과 행 모델(R4-R10), 통합 삭제 게이트와 삭제 순서(R11-R12), 문서(R13). 비목표는 3장.
- project-panel PRD를 뒤집는 세 결정: 카드의 Remove 버튼이 merged/closed에서만 보이던 규칙을 통합 게이트로 대체, 로컬 브랜치 삭제 옵션 추가, pushed 상태 표시 (3장, 4.3).
- 구조 변경: core의 계보 투영과 접힘 상태, `WorktreeSnapshot` 확장과 행별 디스크 측정, `RightPanelSection::Git`, core가 계산하는 하나의 삭제 게이트, pane 닫기 확인 후 저장소 루트에서 실행하는 worktree 제거 (5장).
- 위임 실행: sasu 저장소의 dispatch 경로에 `--from-pane`을 넣는 작업은 `~/projects/sasu`에 연 Herdr pane의 claude(opus) 에이전트가 수행하고, 이 저장소의 receipt가 그 커밋을 기록한다 (T3, V6).
- 검증 모드: build/static, automated behavior, app runtime(스크린샷 판정, 2026-09-07 judge 쿼터 회복 뒤), live herdr integration(HERDR_SOCKET_PATH로 격리한 임시 서버), performance measurement, delegated repository change (9.1).
- 배포 모드: local. `agents/config.json`대로 실행 worktree에 커밋 하나, push와 PR 없음.
- `review_profile: high-risk`와 그 근거 (frontmatter).
- 4.3의 에이전트 가정 A-01부터 A-20까지. 사용자는 이를 사후에 거부할 수 있다.

## 2. Problem, Goal, And Users

사용자는 Hide의 단일 운영자다.
여러 sasu 실행이 동시에 돌면 Observer, Implementor, solver가 서로 다른 worktree에서 같은 높이의 행으로 나열되어 누가 누구를 낳았는지 읽을 수 없다.
worktree는 실행마다 하나씩 늘어나는데, Hide의 사이드바는 pane이 있는 worktree만 보여 주므로 pane이 닫힌 worktree는 시야에서 사라지고, 병합되었는지, push되었는지, 얼마나 큰지는 터미널에서 git을 쳐야 안다.
그래서 worktree가 쌓이고 관리가 안 된다.

목표는 두 가지다.
사이드바에서 에이전트의 파생 관계가 트리로 읽히고, 우측 패널 Git 섹션 한 화면에서 포커스된 프로젝트의 worktree 전부를 상태와 함께 보고 그 자리에서 정리할 수 있는 것이다.

리서치로 확인된 사실은 다음과 같다.

- Herdr 0.8.2는 spawner가 `herdr agent new --from-pane <ID>`를 넘길 때만 계보를 기록한다. core는 `agent.list`의 `spawned_from_pane_id`를 이미 파싱하고 투영(`herdr-core/src/sidebar.rs:461`)하지만 Swift 셸은 읽지 않는다. Hide 자체의 fork는 `--from-pane`을 넘긴다(`herdr-core/src/fork.rs:77`). sasu의 Observer dispatch(`~/.claude/skills/implement/references/observer-and-herdr.md:57`)는 넘기지 않는다.
- main에는 `herdr-core/src/worktrees.rs`(`git worktree list --porcelain`을 읽는 배경 리더, 10초 창), `herdr-core/src/github.rs`(운영자의 gh로 저장소당 한 번 `gh pr list`, 5분 창), `herdr-core/src/disk.rs`(선택된 checkout 하나의 du, 120초 창), `herdr-core/src/reader.rs`(`BackgroundRead`)와 우측 패널 `CheckoutSummaryCard`, 사이드바 checkout 행의 배지가 이미 있다. `runtime.rs:4183`의 `remove_offered`는 PR 배지가 merged/closed일 때만 카드에 Remove를 내놓는다.
- `RightPanelSection`은 `Explorer`, `Changes`의 닫힌 enum이고(`herdr-core/src/model.rs:656`), `scripts/check-right-panel-sections.sh`와 `ChangesPresentationTests`가 "정확히 두 섹션"을 단언한다.
- Herdr는 `worktree.list/create/open/remove`, `pane.close`, `workspace.close`, `pane.focus`를 제공하고, 핀된 계약(`contracts/herdr-api.schema.json`)에 계보 필드와 worktree 이벤트가 모두 있다.
- `git worktree remove <path>`는 디렉터리가 사라진 linked worktree에 대해 `--force` 없이 성공하고 브랜치를 남긴다. 기존 `GitWorktreeRemover`는 대상 디렉터리 안에서 `rev-parse`를 먼저 돌리므로 그 경우를 거부한다.

### 2.1 User Scenarios

- SC1. 사이드바에서 에이전트 트리 읽기: 메인 checkout의 Observer가 `--from-pane`으로 worktree에 Implementor를 띄우고, 나중에 Implementor가 solver를 escalate한다.
  Actors: 운영자.
  Primary path: 프로젝트 트리에 Observer가 보이고 그 아래에 Implementor가 worktree 이름 배지를 달고 들여쓰여 있으며, solver는 Implementor 아래에 들여쓰여 있다. 부모 행에는 접기 chevron이 있다. Implementor가 주의를 요구하면 raised Needs You 그룹에 "↳ from Observer" 힌트를 단 평평한 행 하나로 한 번만 나타난다.
  Failure state: Implementor가 도는 중에 Observer pane이 닫히면 Implementor 행은 자기 worktree checkout 아래로 내려가고 흐린 "↳ from <닫힌 부모 이름 또는 pane id>" 힌트를 단다. 접힌 부모는 자식이 주의를 요구해도 접힌 채다.
  Recovery: 되돌릴 것이 없다. 새 dispatch가 새 트리를 만든다. 다시 열거나 fork해도 계보를 재구성하지 않는다.
  Reach: 격리된 Herdr 서버에서 계보로 연결된 에이전트 셋을 두 checkout에 걸쳐 띄운 픽스처. 픽스처 준비는 T8이다.

- SC2. 포커스된 프로젝트의 worktree 살펴보기: 운영자가 우측 패널 헤더에서 Git을 고른다.
  Actors: 운영자.
  Primary path: 저장소의 모든 worktree가 행으로 보인다. primary worktree가 "main worktree" 배지를 달고 맨 위, 다음은 pane이 열린 것, 그 다음은 마지막 커밋 시각 내림차순이다. 각 행에 브랜치, 프로젝트 기준 상대 경로, pane 수, merged, ahead/behind, dirty, pushed와 fetch 시각, 디스크 용량과 "N min ago", PR 아이콘이 있다. 헤더는 "base: <branch>"를 보이고 새로고침 버튼은 용량과 PR 상태를 다시 잰다. 행의 컨텍스트 메뉴 "Set as base branch"는 그 프로젝트의 기준을 바꾸고 merged와 ahead/behind가 다시 계산된다.
  Failure state: 첫 계산 중에는 스피너와 "Reading worktrees"가 보인다. linked worktree가 없으면 main 행 아래에 흐린 "No linked worktrees yet"이 있다. gh가 없거나 로그아웃이거나 네트워크 실패거나 GitHub 원격이 없으면 PR 아이콘 자리가 그 범주를 말한다. 디스크에서 사라진 행은 "missing on disk"로 Delete만 내놓고 Open은 없다. detached 행은 짧은 SHA를 보이고 gh를 부르지 않으며 PR 자리는 "no branch" 툴팁이다. upstream이 없으면 "no upstream", upstream이 사라졌으면 "upstream gone"이다. du나 git 값이 실패하면 "—"와 툴팁의 원문 오류다. 원격(SSH) 컨텍스트는 Changes와 같은 local-only 안내다. main worktree 경로가 없으면 행 대신 "Repository unavailable: <path>" 하나만 보인다.
  Recovery: 새로고침이 gh와 du를 다시 시도한다. 로컬 상태는 worktree 이벤트나 HEAD 변화가 오면 다시 계산된다.
  Reach: 병합된 브랜치, squash 병합된 브랜치, push 안 된 브랜치, upstream이 지워진 브랜치, dirty worktree, detached worktree, 중첩 worktree, 디렉터리를 지운 worktree를 가진 임시 저장소. 만드는 일은 T8이다.

- SC3. Git 섹션에서 worktree 삭제하기: 운영자가 행에서 Delete worktree…를 고른다.
  Actors: 운영자.
  Primary path: 확인창이 결과를 나열한다(ahead N unmerged, not pushed, N running agents, 용량). 병합된 브랜치면 "also delete local branch" 체크박스가 있다. 확인하면 worktree가 제거되고 행이 사라지며, 선택했다면 브랜치도 지워진다.
  Failure state: dirty, 중첩 worktree를 품은 부모, primary, 현재 기준 브랜치의 worktree는 버튼이 비활성이고 이유가 보인다. 기준을 바꾸면 보호도 새 기준의 worktree로 옮겨 간다. pane이 열려 있으면 버튼이 "Close N panes and delete"로 읽히고, Herdr가 닫기를 거부하면 아무것도 제거되지 않고 이유가 보인다. pane을 닫은 뒤 git worktree remove가 실패하면 메시지가 실패를 말하고 worktree는 그대로이며 pane은 닫힌 채다(대화상자가 미리 말한다). 제거는 됐는데 브랜치 삭제가 실패하면 행은 사라지고 브랜치가 남았다는 안내와 git의 이유가 보인다.
  Recovery: 거부된 삭제는 worktree를 그대로 둔다. 운영자가 막힌 이유를 풀고(커밋 또는 폐기, 중첩 worktree 먼저 삭제) 다시 시도한다.
  Reach: SC2의 임시 저장소와, 격리된 Herdr 서버에서 그중 한 worktree에 연 pane. T8.

- SC4. pane 없는 worktree 열기: 운영자가 pane이 없는 행에서 Open을 고른다.
  Actors: 운영자.
  Primary path: Hide가 Herdr `worktree.open`을 부르고 사이드바에 workspace와 pane이 나타나며 행의 pane 수가 1이 된다. 이미 열린 행에서 Open은 `last_activity`가 가장 최근인 pane을 포커스한다.
  Failure state: 새로고침과 클릭 사이에 디렉터리가 사라졌으면 Herdr가 거부하고, 행은 거부 문구를 인라인으로 보이며 missing on disk로 다시 읽힌다(Delete만). 이미 missing으로 알려진 행은 Open을 내놓지 않는다.
  Recovery: 새로고침하거나 prunable worktree를 삭제한다.
  Reach: 격리된 Herdr 서버와 pane 없는 worktree 하나, pane 둘이 열린 worktree 하나. T8.

## 3. Scope And Non-Goals

범위:

- 계보 트리: core 투영(부모 아래 중첩, 무제한 깊이, 고아 fallback, 접힘 상태 영속, raised 그룹의 평평한 힌트 행)과 Swift 렌더(들여쓰기, worktree 배지, chevron, 힌트).
- sasu의 Observer dispatch와 escalate dispatch가 `--from-pane "$HERDR_PANE_ID"`를 넘기도록 하는 변경을 위임 실행하고, 격리 서버에서 실제 dispatch 한 번으로 증명.
- 우측 패널 Git 섹션: 섹션 추가와 영속, 행 목록과 정렬, 행 모델과 특수 케이스, 기준 브랜치 지정, 갱신 계층, 상태 표시, 행 액션.
- 삭제: core가 계산하는 하나의 게이트(차단, 경고, pane 닫기)와 세 표면의 채택, pane 닫기 확인 후 저장소 루트에서 실행하는 제거, 선택적 브랜치 삭제, 부분 실패 안내, missing 행의 등록 해제.
- 문서: `AGENTS.md`에 Git 섹션의 갱신 계층 한 줄, 필요하면 `DESIGN.md`의 토큰 추가.

project-panel PRD를 뒤집는 결정(인터뷰가 나중이므로 이 PRD가 이긴다):

- 카드의 Remove worktree가 PR merged/closed에서만 보이던 규칙(그 PRD의 R5, D-08, D-22)은 통합 게이트(R11)로 대체된다. 병합되지 않은 worktree도 경고와 함께 지울 수 있다.
- 로컬 브랜치 삭제 제외(그 PRD의 D-08)는 D-14의 "also delete local branch" 체크박스로 대체된다.
- pushed 배지 없음(그 PRD의 D-10, D-21)은 Git 섹션 행의 pushed 슬롯(R6)으로 대체된다. 카드의 `↑N`은 그대로다.
- 용량은 선택된 checkout 하나만(그 PRD의 Q7 c 거절)에서 Git 섹션의 행별 용량으로 넓어진다. 단 R8의 갱신 계층이 비용을 묶는다.

비목표, 각각 의도된 제외:

- 계보의 두 번째 채널(pane metadata 토큰 규약)과 UI만의 추론 (D-09 거절안 B, C).
  Consequence: `--from-pane` 없이 띄운 에이전트는 평평하게 남는다.
  Revisit: 없음. Herdr가 계보의 단일 원천이다.
- push, merge, PR 생성, 브랜치나 worktree 생성, prune, `--force` (D-14, D-41).
  Consequence: 이 동작은 터미널에서 한다.
  Revisit: 사용자가 요청할 때.
- 모든 프로젝트를 한 번에 보는 인벤토리 (D-16 거절안 B, C).
  Consequence: 다른 프로젝트는 사이드바에서 포커스해서 본다.
- 원격(SSH) 컨텍스트에서의 git 읽기 (D-17).
  Consequence: 원격 프로젝트의 Git 섹션은 local-only 안내만 보인다.
  Revisit: 원격 git 읽기가 추가될 때.
- Herdr 버전 floor나 degraded 모드 (D-23).
  Consequence: 계약 불일치는 지금처럼 실행 시작에서 거부된다.
- escalate → solver 계보의 end-to-end 증명 (D-28, deferred).
  Consequence: 그 경로는 위임된 sasu 에이전트의 자체 테스트가 지킨다.
  Revisit: 실제 escalate에서 solver가 트리에 중첩되지 않는 첫 사례.
- gh 토큰 저장, 로그인 흐름, GitHub 외 호스팅 (D-31).
  Consequence: 로그아웃된 gh는 실패 범주로 보일 뿐이다.
- 브랜치만 다시 지우는 재시도 액션 (D-26).
  Consequence: 브랜치 삭제가 실패하면 터미널에서 지운다.
- 접힌 부모의 자동 펼침 (D-18).
  Consequence: 자식의 주의 요구는 raised 그룹이 보여 준다.
- `herdr-runtime-owned` 브랜치의 Herdr 핀 변경 세 커밋 (A-01).
  Consequence: 이 PRD의 실행 브랜치에는 그 커밋이 없다. 병합은 별도 작업이다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
gh는 이 Mac에 설치되어 운영자 계정으로 로그인되어 있고, Herdr 바이너리는 저장소가 번들하며, 검증용 픽스처와 격리 서버는 task(T8)가 만든다.

### 4.2 Human Decisions Before PRD Approval

None required.
모든 제품 결정은 qa-log의 Decision Register에 사용자 결정으로 있거나 4.3에 에이전트 소유의 되돌릴 수 있는 가정으로 기록되어 있다.
파괴적 동작(worktree 제거, 브랜치 삭제, pane 닫기)은 사용자가 Q7, Q12, Q15, Q16에서 직접 정한 규칙을 따르고, 실행은 매번 확인창 뒤에서만 일어난다.

### 4.3 Decision Traceability For Fidelity Review

사용자 결정(qa-log Decision Register, D#):

- D-09 (Q1, "herdr 자체 계보로 A"): 계보의 단일 원천은 Herdr의 `spawned_from_pane_id`. R1. 거절: 토큰 규약(B), UI만(C) → 비목표.
- D-10 (Q2, A): 프로젝트 트리에서 계보가 checkout을 이긴다. 다른 checkout의 자식은 부모 아래 들여쓰고 worktree 이름 배지를 단다. raised Needs You / Done 그룹은 평평한 행에 "↳ from <parent>" 힌트. R1, R2, AC1, AC4, SC1. 거절: 힌트만(B), raised 그룹 안 트리(C).
- D-11 (Q3, A): 세 번째 섹션 Git. Changes는 그대로. `RightPanelSection`에 Git 추가, 영속 유지. R4, AC8. 거절: Changes 개명(B), Explorer 안 블록(C).
- D-12 (Q4, A + PR 아이콘): 행 필드 전부와 gh PR 아이콘. R6, AC10, AC13. squash 병합은 감지하지 않으므로 ahead/behind를 옆에 둔다.
- D-13 (Q5, A): 세 갱신 계층, per tick 금지, mutex 밖. R8, AC12, AC25. 거절: 60초 주기(B), 수동만(C). 로컬 상태의 "섹션이 보일 때만" 조건은 A-03에서 조정했다.
- D-14 (Q6, A): 액션 Open, Delete worktree…(+ merged면 "also delete local branch"), Reveal in Finder, Copy path. R10, R12, AC18, AC21. 거절: New worktree…(B), Push/Prune(C) → 비목표.
- D-15 (Q7, A): 차단(dirty, 중첩, 기준 브랜치의 main worktree), 경고(unmerged, unpushed, running agents), "Close N panes and delete", 닫기 실패면 제거 없음. R11, R12, AC16, AC17. 거절: pane 열려 있으면 차단(B), 경고만 + --force(C).
- D-16 (Q8, A): 포커스된 프로젝트만. R4, AC8. 거절: 전체(B), 토글(C) → 비목표.
- D-17 (Q8에서 제시한 에이전트 기본값, 이의 없음, P2 가정): 원격(SSH) 컨텍스트의 Git 섹션은 Changes와 같은 local-only 안내. R4, AC8, 비목표. 이 항목은 사용자 결정이 아니라 가정으로 남는다.
- D-18 (Q9 수락): 고아는 자기 checkout 아래 + 흐린 힌트, 무제한 깊이, 깊이 3부터 들여쓰기 축소, chevron 상태 ui_state 영속, 자동 펼침 없음, 형제는 last_activity 순. R1, R2, AC2, AC3.
- D-19 (Q9 수락): pane 열린 것 먼저, 그다음 마지막 커밋 시각 내림차순. R5, AC9.
- D-20 (Q10, "B로 해. 근데 태스크 직접하기보단 해당 프로젝트에 pane 열어서 진행하게 해 claude opus로"): sasu 변경은 이 PRD의 task이며 `~/projects/sasu`에 연 Herdr pane의 claude(opus) 에이전트가 수행한다. Hide Implementor는 브리핑, 계보 검증, receipt에 sasu 커밋 기록. R3, T3, V6. 거절: 비목표 + 후속(A).
- D-21 (Q11 수락): 증명은 receipt의 sasu 커밋 기록과, 갱신된 skill로 실제 Observer → Implementor dispatch 한 번에서 `herdr agent list`의 `spawned_from_pane_id`와 Hide 사이드바의 중첩 행. AC6, AC7, V4, V6.
- D-22 (Q11 수락): 트리 들여쓰기와 배지, Git 섹션 밀도와 아이콘 가독성, 삭제 대화상자 문구는 스크린샷 판정. codex judge 쿼터가 2026-09-07에 회복되므로 구현이 먼저, 시각 판정은 그 뒤. AC5, AC15, AC23, V3(Required For Done: no), 9.3.
- D-24 (Q12-1 수락): 상태는 ChangesView 패턴. "Reading worktrees" 스피너, 새로고침 중 버튼 비활성 + 스피너, 실패 값 "—" + 툴팁 원문. R9, AC14. 빈 상태 문구는 D-35가 대체.
- D-25 (Q12-2 수락): detached는 짧은 SHA + "detached", bare 제외, upstream 없음 "no upstream", missing은 "missing on disk"에 Delete만, du 실패 "—". R6, AC10.
- D-26 (Q12-3 수락): 제거 후 브랜치 삭제 순서, 부분 실패 안내, 브랜치만 재시도 없음. R12, AC18.
- D-27 (Q12-4 수락): gh 읽기 전용, 프롬프트 없음, 타임아웃, PR 여럿이면 open 우선 후 최신, 실패 범주 넷. R6, AC13. 호출 단위는 A-05에서 main의 저장소당 한 번으로 조정했다.
- D-28 (Q12-5 수락, deferred): escalate → solver e2e는 유보. 비목표에 revisit 조건.
- D-29 (Q12-6 수락): 실제 dispatch 검증은 HERDR_SOCKET_PATH로 격리한 임시 서버와 일회용 픽스처 저장소에서, 만든 것은 전부 정리. V4의 부작용 경계, 11장.
- D-30 (gap-audit 발견의 에이전트 기본값, P2): 섹션 영속 round-trip, 갱신 계약의 계측 테스트, pane 닫힌 뒤 remove 실패의 fault-injection. AC8, AC12, AC17.
- D-31 (Q13-1 수락): 운영자의 gh 로그인만 쓰고 토큰을 저장하지 않으며 로그인 흐름이 없다. R6, 비목표.
- D-32 (Q13-2 수락): upstream이 있으나 remote-tracking ref가 없으면 "upstream gone", 삭제 경고는 not pushed로 취급. R6, R11, AC10.
- D-33 (Q13-3 수락): 차단 규칙이 우선, prunable 규칙은 linked만, main worktree 경로가 없으면 "Repository unavailable: <path>". R9, R11, AC14.
- D-34 (Q13 자유 답변 "브랜치를 default로 설정해두면 좋긴 하겠다", Q14 확인): 프로젝트별 기준 브랜치. 기본은 저장소 기본 브랜치, 행 메뉴 "Set as base branch", ui_state에 프로젝트 경로로 영속, 헤더 "base: <branch>", 없어진 브랜치는 기본으로 fallback하고 헤더가 말한다. R7, AC11.
- D-35 (Q15-1 수락): primary worktree가 항상 첫 행, "main worktree" 배지, Delete 비활성. 빈 상태 대신 흐린 "No linked worktrees yet". 섹션 전체 안내는 셋뿐. R5, R9, AC9.
- D-36 (Q15-2 수락): missing 행은 Delete만, Open 없음. Open 실패는 사라진 디렉터리 경우로 좁힘. R10, AC22.
- D-37 (Q15-3 수락): detached 행에는 "Set as base branch"가 없다. R7, AC11.
- D-38 (Q15-4 수락): 여러 pane이 열린 worktree의 Open은 `last_activity` 최신 pane 포커스. R10, AC21.
- D-39 (Q15-5 수락): 보호 worktree는 primary와 현재 기준 브랜치가 체크아웃된 worktree. 기준 변경이 보호를 옮긴다. R11, AC11, AC16.
- D-41 (Q16-1 수락): missing 행의 Delete는 저장소 루트에서 같은 `git worktree remove <path>`, prune과 --force 없음, 브랜치 유지, 실패면 git 메시지와 행 유지. R12, AC19.
- D-42 (Q16-2 수락): detached 행은 gh를 부르지 않고 PR 자리는 비운 채 "no branch" 툴팁. R6, AC13.

사실(qa-log의 D-01..D-08, D-23, D-40)과 main에서 확인한 변화:

- D-01, D-02, D-08: 계보는 `--from-pane`으로만 기록되고 sasu dispatch가 그것을 빼먹는다. 2장, R3의 근거.
- D-03: `RightPanelSection {Explorer, Changes}` 사실은 main에서도 유효(`model.rs:656`). 다만 main의 `scripts/check-right-panel-sections.sh`와 `ChangesPresentationTests`가 두 섹션을 단언하므로 T5가 함께 고친다.
- D-04: Herdr `worktree.list` 필드 사실. 인벤토리 원천은 A-17.
- D-05: "pane 없는 worktree는 보이지 않는다"는 사실은 `herdr-runtime-owned`에서 확인한 것이고 main에서는 **대체되었다**. main의 `WorktreeReader`가 `git worktree list --porcelain`으로 모든 worktree를 읽고 사이드바가 pane 없는 행을 흐리게 보인다(`HideUI.swift:1230-1320`). 삭제 흐름은 여전히 `GitWorktreeRemover`(대상 디렉터리 안 rev-parse)와 확인창이다. 이 PRD는 그 위에 쌓는다(A-02).
- D-06: 성능 규칙은 그대로다. main의 `BackgroundRead` 리더들이 이미 그 규칙을 지키는 형태이므로 새 값은 그 리더에 싣는다(A-02, A-04).
- D-07: 이 기계의 중첩 worktree와 base 브랜치 worktree 사실. 픽스처 모양의 근거(T8).
- D-23: 버전 floor 없음. 비목표.
- D-40: 디렉터리가 없는 linked worktree의 `git worktree remove` 동작. R12의 근거.

에이전트 가정 (사용자 결정이 아님, 되돌릴 수 있음, 사용자가 사후에 거부할 수 있음):

- A-01 실행 기준은 `main`(caaa665)이다. 사용자의 "가장 최신 기준으로 worktree 따서"를 main 팁으로 읽었다. 이 세션의 작업 트리가 있던 `herdr-runtime-owned`의 세 커밋(Herdr 핀을 preview로 옮기는 변경)은 포함하지 않는다. 실행 worktree는 main 팁에서 만든 spec worktree의 HEAD에서 갈라진다.
- A-02 D-03..D-06의 preflight는 뒤처진 브랜치에서 했고, main은 project-panel PRD로 worktree 리더, gh 리더, 디스크 리더, checkout 카드, 사이드바 배지를 이미 실었다. 이 PRD는 그 모듈을 확장하고 병렬 구현을 만들지 않는다(engineering 규칙 7).
- A-03 로컬 git 상태(merged, ahead/behind, dirty, pushed)는 D-13의 "Git 섹션이 보일 때만" 대신 main의 `WorktreeReader`가 이미 사이드바를 위해 도는 cadence(입력 변화 + 10초 창, worktree 이벤트 트리거)를 그대로 쓴다. 섹션 가시성으로 막아도 사이드바가 같은 값을 쓰므로 비용이 줄지 않는다. per tick 금지와 mutex 밖 실행은 그대로다.
- A-04 행별 디스크 용량은 Git 섹션이 보이는 동안 섹션이 열릴 때와 수동 새로고침에만 재고, 기존 디스크 워커에서 worktree 하나씩 순차로 잰다. 섹션이 숨겨진 동안은 어떤 worktree도 재지 않는다. 카드의 선택된 checkout 측정은 같은 워커의 항목을 읽는다.
- A-05 PR 상태는 D-27의 브랜치별 `gh pr list --head`가 아니라 main의 저장소당 한 번 `gh pr list --state all --limit 200`(5분 창, 수동 새로고침 트리거)을 그대로 쓰고 브랜치로 매핑한다. D-27의 나머지(읽기 전용, 프롬프트 없음, 타임아웃, 여럿이면 open 우선 후 최신, 실패 범주 넷)는 유지한다. main의 gh 리더는 "not installed"와 "not logged in"만 구분하므로 "network or rate limit"과 "no GitHub remote"를 더한다. detached 행은 매핑 대상이 없으므로 gh 호출이 없다(D-42 충족).
- A-06 삭제 게이트는 core가 worktree마다 한 번 계산하는 `WorktreeSnapshot`의 필드가 되고 카드, 사이드바 컨텍스트 메뉴, Git 섹션이 같은 값을 읽는다. main의 `remove_offered`(PR settled일 때만)와 `remove_blocked_reason`은 삭제된다(engineering 규칙 1). 카드와 사이드바 메뉴에 D-15의 규칙을 적용하는 것은 인터뷰가 Git 섹션만 말했으므로 가정이다.
- A-07 기준 브랜치 우선순위는 운영자가 지정한 값 > 그 브랜치의 PR base(main의 `bases`) > 저장소 기본 브랜치다. D-34는 기본을 저장소 기본 브랜치로 말했고 project-panel R7은 PR base를 먼저 봤다. PR이 있는 브랜치는 PR base가 실제 병합 대상이므로 그것을 지정값 다음에 둔다. ahead/behind와 merged는 이 순서로 고른 기준에 대해 계산한다.
- A-08 사이드바 checkout 행의 "primary" 배지 문구를 D-35의 "main worktree"로 통일한다. 같은 것을 두 이름으로 부르지 않기 위해서다(design 규칙 5).
- A-09 브랜치 삭제는 `git branch -d`(안전 삭제)뿐이다. 체크박스는 D-12의 merged 정의(worktree 팁이 기준의 조상)가 참일 때만 보인다. PR 배지가 merged인 것만으로는 보이지 않고, `-D`는 어떤 경우에도 쓰지 않는다. squash 병합된 브랜치는 merged가 거짓이므로 체크박스가 없고 터미널에서 지운다(비목표). git이 `-d`를 거부하면 D-26의 안내로 끝난다. 확인창이 이 규칙을 문장으로 미리 말한다. spec gate가 PR-merged 근거와 force 삭제를 인터뷰가 승인하지 않은 비가역 동작으로 지적해 승인된 정의로 좁혔다.
- A-10 merged 판정은 `git merge-base --is-ancestor <tip> <base>`다. squash 병합은 거짓이 되고 그 경우 옆의 ahead/behind와 PR 배지가 사실을 보완한다(D-12가 말한 한계).
- A-11 snapshot 채널: worktree 행, 기준 브랜치 지정, 접힘 상태는 드물게 바뀌므로 revisioned `rest`에 싣는다. 디스크 용량과 PR 상태의 시각 stamp는 측정이 끝날 때만 바뀐다. 매 틱 restamp되는 필드는 만들지 않는다(Performance Guide).
- A-12 계보 트리의 키는 pane id다. 자식의 `spawned_from_pane_id`와 pane id가 같은 에이전트 행이 부모다. 그 pane을 가진 행이 없으면 고아다. 접힘 상태는 부모 pane id로 ui_state에 남고 그 pane이 사라지면 정리된다.
- A-13 Implementor는 사용자의 말대로 codex `gpt-6-astra`, reasoning effort medium이다. sasu 위임 에이전트는 D-20대로 claude opus다. sasu의 dispatch 헬퍼가 herdr 0.8.2에서 동작하지 않으므로(memory `sasu-herdr-dispatch-adapter-stale`) 두 dispatch 모두 `herdr pane split --env SASU_HERDR_ROLE=implementor` + `herdr agent start`, 또는 `herdr agent new --from-pane`으로 손으로 한다. `agent new`는 `--env`가 없어 역할 마커를 넣을 수 없으므로 Implementor pane은 마커를 위해 split + start로 띄우고, 그 결과 이 실행의 Implementor 자체는 Herdr 계보를 갖지 않는다. 트리의 증명은 격리 서버의 픽스처(AC6)에서 온다.
- A-14 sasu 변경의 범위는 문서화된 Observer dispatch 경로(`observer-and-herdr.md`의 `herdr agent new` 명령과 그 명령을 조립하는 어댑터의 spawn 구멍)와 escalate dispatch 경로에 `--from-pane "$HERDR_PANE_ID"`를 넣는 것까지다. 뒤처진 어댑터를 0.8.2에 맞게 다시 쓰는 일은 이 PRD의 범위가 아니다.
- A-15 스크린샷 판정 AC(AC5, AC15, AC23)는 `Required For Done: no`, `Can Be Blocked: yes`다. codex judge 쿼터가 2026-09-07에 회복되기 전에는 판정할 수 없고 사용자가 자리에 없어 `park`를 승인할 수 없기 때문이다. 9.3이 같은 항목을 사람의 판정으로 든다.
- A-16 Reveal in Finder와 Copy path는 셸만의 동작이다. core 이벤트 없이 NSWorkspace와 NSPasteboard로 끝난다.
- A-17 worktree 인벤토리의 원천은 main의 `WorktreeReader`(`git worktree list --porcelain`)다. prunable, detached, bare를 이미 구분한다. pane 수와 "열림" 여부는 카탈로그의 checkout을 경로로 맞춰 얻고, Herdr의 `worktree_created/opened/removed` 이벤트와 카탈로그 변화가 리더를 다시 트리거한다. Herdr `worktree.list`는 인벤토리로 쓰지 않는다. 두 원천을 합치지 않기 위해서다.
- A-18 마지막 커밋 시각은 같은 배경 읽기 안에서 worktree마다 `git log -1 --format=%ct`로 읽는다. 중첩 여부는 다른 worktree의 경로가 이 경로 아래에 있는지로 판정한다.
- A-20 성능 측정 프로토콜(AC25)은 에이전트가 정한 값이다: load 14 미만(그 위에서는 sample이 심볼화하지 못한다는 Performance Guide의 사실), 20초 유휴, 3초 창 5개, mutex 대기 평균 차이 0.5 퍼센트포인트. 2026-09-04의 측정(유휴 0.28%, 구동 1.52%)에서 유휴 창의 자연 변동 폭을 넉넉히 덮는 값이다.
- A-19 Open의 대상이 pane 없는 worktree면 Herdr `worktree.open`을 부르고, 그 응답의 workspace가 카탈로그에 나타나면 행의 pane 수가 오른다. 거부 문구는 행에 인라인으로 보이고 다음 리더 읽기가 그 행을 missing으로 다시 분류한다.

배포: `agents/config.json`의 `delivery.mode: local`, `baseBranch: main`, `worktree.enabled: true`를 그대로 따른다.
실행 worktree에 커밋 하나, push와 PR 없음.
sasu 저장소의 커밋은 위임된 에이전트가 그 저장소의 관례대로 남기고 이 저장소의 receipt가 해시를 기록한다.

원칙 인테이크: `~/projects/oh-my-principle` 커밋 `35ab76ca23d45e714f1630054855a8c8c4568d03`에서 `engineering/principles.md`와 `design/principles.md`를 전문으로 읽었다.
적용 규칙은 11장에 번역했다.
design 규칙 1(목록은 읽기 뷰이고 데이터가 모양을 정한다)은 Git 섹션의 행 구성(R6)이 곧 그 적용이라 별도 guardrail로 번역하지 않았다.

프로젝트 규칙 인테이크: `AGENTS.md`의 Performance Guide, Herdr API Contract, Evidence Belongs Outside The Repository, Design Reference와 `agents/rules/invariants/INV-herdr-unseen-token.md`를 11장에 번역했다.
UX 카드 UX-01..UX-04는 SC1..SC4로 옮겼다.

## 5. Major Technical Structure Changes

- core의 사이드바 투영이 에이전트 행을 계보 트리로 낸다.
  각 에이전트 행이 부모 pane id, 깊이, 자식 목록, 고아 여부와 힌트 텍스트, 다른 checkout에서 도는지의 배지 텍스트를 싣는다.
  raised 그룹의 행은 평평한 채 힌트만 싣는다.
  접힘 상태는 `UiStateSnapshot`에 부모 pane id 집합으로 영속되고 셸 이벤트로 토글된다.
- `WorktreeSnapshot`이 확장된다: 기준의 조상인지(merged), upstream 상태(pushed / no upstream / upstream gone / ↑N), 마지막 커밋 시각, 중첩 worktree 보유 여부, 열린 pane 수, 디스크 용량과 측정 시각, PR 배지 참조, 삭제 게이트 판정(차단 이유 또는 경고 목록과 닫을 pane 수), 보호 여부.
  `ProjectWorktreesSnapshot`이 유효 기준 브랜치와 그 출처(지정, PR base, 기본, fallback 안내)를 싣는다.
- `DiskReader`가 경로 하나 대신 경로 목록을 받아 순차로 재고 항목마다 시각을 남긴다.
  요청은 Git 섹션의 가시성과 새로고침 이벤트에서만 만들어진다.
- `GithubReader`의 실패 범주가 넷으로 늘고, 항목이 worktree 행의 PR 슬롯으로 매핑된다.
- `RightPanelSection`에 `Git`이 추가되고 영속 파싱이 이를 받아들인다.
  `scripts/check-right-panel-sections.sh`와 `ChangesPresentationTests`의 "정확히 두 섹션" 단언이 세 섹션으로 바뀐다.
- `UiStateSnapshot`이 프로젝트 경로별 기준 브랜치 지정을 싣는다.
- 삭제 흐름의 순서가 바뀐다.
  셸의 삭제 요청은 core 이벤트가 되고, core가 Herdr에 pane/workspace 닫기를 요청해 이벤트로 확인한 뒤에야 셸이 저장소 루트에서 `git worktree remove <path>`를 실행하고, 선택했다면 브랜치를 지운다.
  `GitWorktreeRemover`는 대상 디렉터리 안의 `rev-parse` 대신 저장소 루트에서 동작한다.
  main의 `remove_offered` / `remove_blocked_reason` 규칙은 삭제된다.
- 외부 저장소 변경: `~/projects/sasu`의 dispatch 경로가 `--from-pane`을 넘긴다.
  이 저장소에는 코드 변경이 없고 receipt가 커밋 해시를 기록한다.
- 스키마, 인증, 결제, 배포 변경 없음. 새 서드파티 의존성 없음. Herdr 계약 변경 없음(핀된 계약의 기존 메서드와 필드만 쓴다).

## 6. Requirements

- R1. core는 `agent.list`의 `spawned_from_pane_id`로 에이전트 계보 트리를 투영한다.
  자식은 그 pane id를 가진 에이전트 행 아래에 중첩되며 checkout이 달라도 그렇다.
  깊이는 제한이 없다.
  부모 pane이 없으면 자식은 자기 checkout 아래로 내려가고 "↳ from <부모 이름 또는 pane id>" 힌트를 싣는다.
  형제는 기존 `last_activity` 순이다.
  부모 행의 접힘 상태는 `ui_state`에 영속되고, 자식이 주의를 요구해도 자동으로 펼쳐지지 않는다.
  raised Needs You / Done 그룹의 행은 평평하며 "↳ from <parent>" 힌트만 싣는다.
  이 투영은 읽음 기록과 demand 축을 바꾸지 않는다.
- R2. 사이드바는 그 트리를 들여쓰기로 그린다.
  깊이 3부터 들여쓰기 폭이 줄어든다.
  다른 checkout에서 도는 자식은 worktree 이름 배지를 단다.
  부모 행에는 chevron이 있고 클릭이 접힘을 토글한다.
  고아와 raised 행의 힌트는 흐린 색이다.
  색, 간격, 반경은 `HideTheme`의 토큰이다.
- R3. sasu의 Observer dispatch와 escalate dispatch가 `--from-pane "$HERDR_PANE_ID"`를 넘긴다.
  이 변경은 `~/projects/sasu`에 연 Herdr pane의 claude(opus) 에이전트가 그 저장소의 관례대로 구현하고 커밋한다.
  Hide Implementor는 브리프(변경 범위, 뒤처진 헬퍼 사실, 검증 방법)를 보내고, 격리 서버에서 실제 dispatch 한 번으로 계보를 확인하며, receipt에 sasu 커밋 해시와 그 저장소 테스트 결과를 기록한다.
- R4. 우측 패널 헤더가 Explorer, Changes, Git 세 섹션을 제공한다.
  선택된 섹션은 지금처럼 `ui_state`에 영속되고 재시작을 견디며, 저장된 값이 없으면 Explorer다.
  Git은 포커스된 프로젝트의 worktree만 보인다.
  원격(SSH) 컨텍스트에서는 Changes와 같은 local-only 안내만 보인다.
- R5. 행 목록은 primary worktree가 항상 첫 행이고 "main worktree" 배지를 단다.
  그 아래는 pane이 열린 worktree, 그다음은 마지막 커밋 시각 내림차순이다.
  bare 항목은 제외된다.
  linked worktree가 없으면 main 행 아래에 흐린 "No linked worktrees yet"이 있다.
- R6. 각 행은 브랜치(detached면 짧은 SHA와 "detached"), 프로젝트 기준 상대 경로, 열린 pane 수, merged(기준의 조상), 기준 대비 ahead/behind, dirty, pushed 슬롯(pushed / ↑N / no upstream / upstream gone, 마지막 fetch 시각 stamp), 디스크 용량과 "N min ago", PR 아이콘을 보인다.
  PR 아이콘은 open / merged / closed / 없음이며 운영자의 gh 로그인으로 저장소당 한 번 읽는다.
  gh 실패는 아이콘 자리가 not installed, not logged in, network or rate limit, no GitHub remote 중 하나로 말한다.
  detached 행은 PR 자리를 비우고 "no branch" 툴팁이며 gh 매핑 대상이 아니다.
  gh는 읽기 전용 명령만, 프롬프트가 비활성화된 환경과 타임아웃 아래에서 실행되며, Hide는 토큰이나 gh 설정을 저장하지 않고 로그인을 시작하지 않는다.
- R7. 기준 브랜치는 프로젝트별로 정해진다.
  우선순위는 운영자 지정 > PR base > 저장소 기본 브랜치다.
  행의 컨텍스트 메뉴 "Set as base branch"가 지정을 바꾸고 `ui_state`에 프로젝트 경로로 영속된다.
  detached 행에는 그 메뉴가 없다.
  헤더는 "base: <branch>"를 보이고, 지정된 브랜치가 없어졌으면 기본으로 돌아가며 헤더가 그 사실을 말한다.
  기준이 바뀌면 merged, ahead/behind, 보호 worktree가 다시 계산된다.
- R8. 갱신은 세 계층이다.
  로컬 git 상태는 기존 worktree 리더의 cadence(입력 변화와 창, worktree 이벤트 트리거)로, 디스크 용량과 PR 상태는 Git 섹션이 열릴 때와 헤더 새로고침에만 다시 잰다.
  섹션이 숨겨진 동안 디스크 측정은 없다.
  어떤 계층도 per tick으로 subprocess를 부르지 않고 runtime mutex 아래에서 I/O를 하지 않는다.
  각 값은 측정 시각 stamp를 갖는다.
- R9. 섹션 상태는 ChangesView 패턴이다.
  첫 계산 중 "Reading worktrees" 스피너, 새로고침 중 버튼 비활성과 스피너, 실패한 값은 "—"와 툴팁의 원문 오류.
  main worktree 경로가 없으면 행 대신 "Repository unavailable: <path>" 하나만 보인다.
  섹션 전체 안내는 이 셋(Reading, local-only, unavailable)뿐이다.
- R10. 행 액션은 Open, Reveal in Finder, Copy path, Set as base branch, Delete worktree…다.
  Open은 pane 없는 worktree에 Herdr `worktree.open`을 부르고, 이미 열린 worktree에서는 `last_activity` 최신 pane을 포커스한다.
  missing 행은 Open을 내놓지 않는다.
  디렉터리가 새로고침과 클릭 사이에 사라졌으면 Herdr의 거부 문구가 행에 인라인으로 보이고 행은 missing으로 다시 읽힌다.
- R11. 삭제 게이트는 core가 worktree마다 계산하는 하나의 판정이며 카드, 사이드바 컨텍스트 메뉴, Git 섹션이 같은 판정을 보인다.
  차단(버튼 비활성 + 이유): dirty, 중첩 worktree 보유, primary worktree, 현재 기준 브랜치가 체크아웃된 worktree, 원격 컨텍스트.
  경고(확인창에 나열): ahead N unmerged, not pushed(no upstream과 upstream gone 포함), N running agents, 용량.
  pane이 열려 있으면 버튼이 "Close N panes and delete"다.
  차단이 경고에 우선한다.
  기준 변경은 옛 기준 worktree의 보호를 풀고 새 기준에 건다.
- R12. 삭제 순서는 pane 닫기 → Herdr 확인 → 저장소 루트에서 `git worktree remove <path>` → 선택 시 브랜치 삭제다.
  Herdr가 닫기를 거부하면 아무것도 제거되지 않고 이유가 보인다.
  pane을 닫은 뒤 제거가 실패하면 worktree는 그대로, pane은 닫힌 채, 메시지가 실패를 말하고 재시도할 수 있다. 확인창이 이 결과를 미리 말한다.
  제거는 됐고 브랜치 삭제가 실패하면 행은 사라지고 "worktree removed, branch <name> remains: <git reason>" 안내가 보인다.
  브랜치 삭제 체크박스는 merged(팁이 기준의 조상)일 때만 보이고, 삭제는 `git branch -d` 안전 삭제뿐이며 `-D`는 쓰지 않는다(A-09).
  missing 행의 Delete는 같은 명령을 저장소 루트에서 실행하고 prune과 --force를 쓰지 않으며 브랜치를 남긴다.
- R13. `AGENTS.md`의 Runtime Architecture가 Git 섹션의 갱신 계층 한 줄을 얻고, 새 색이나 아이콘이 필요하면 `DESIGN.md`와 `HideTheme`에 토큰으로 더한다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | 두 checkout에 걸친 세 에이전트(부모, `spawned_from_pane_id`가 부모 pane인 자식, 자식 pane을 가리키는 손자) 픽스처의 투영에서 자식은 부모 아래 깊이 1, 손자는 깊이 2이고, 다른 checkout의 자식은 그 worktree 이름 배지를 가지며, 형제 순서는 `last_activity`다 | machine | - |
| AC2 | 부모 pane이 목록에서 사라지면 자식은 자기 checkout 아래 깊이 0으로 돌아가고 "↳ from <이름 또는 pane id>" 힌트를 갖는다 | machine | - |
| AC3 | 부모의 접힘 토글이 `ui_state`에 남아 snapshot round-trip과 재시작을 견디고, 접힌 동안 자식이 주의를 요구해도 접힘 값이 바뀌지 않으며, 부모 pane이 사라지면 그 항목이 정리된다 | machine | - |
| AC4 | 주의를 요구하는 자식이 raised Needs You 그룹에 깊이 0의 행 하나로 힌트와 함께 나타나고 프로젝트 트리의 중첩 행과 합쳐 두 번을 넘지 않으며, 기존 읽음 기록과 demand 축 테스트가 그대로 통과한다 | machine | - |
| AC5 | 조립된 dev 앱에서 세 단계 트리의 들여쓰기, worktree 배지, chevron, raised 그룹의 힌트 행이 한눈에 계층으로 읽힌다 | judged | 격리 서버의 세 에이전트 픽스처에서 사이드바 캡처 두 장: 펼친 상태와 부모를 접은 상태 |
| AC6 | 갱신된 sasu skill로 격리 서버에서 실제 Observer → Implementor dispatch를 한 번 하면 `herdr agent list`의 Implementor 항목에 Observer pane id가 `spawned_from_pane_id`로 있고 Hide 사이드바에서 Implementor가 Observer 아래에 중첩된다 | judged | 격리 서버에서 dispatch 실행 스크립트, 그 뒤의 agent list 출력, Hide의 사이드바 캡처와 core snapshot의 트리 항목 |
| AC7 | `~/projects/sasu`의 기록된 커밋에서 Observer dispatch 문서와 그 명령을 조립하는 코드, escalate dispatch 경로가 모두 `--from-pane`을 넘기고 그 저장소의 테스트가 통과한다 | machine | - |
| AC8 | 우측 패널 헤더의 섹션이 정확히 Explorer, Changes, Git이고, Git 선택이 `ui_state` round-trip과 재시작을 견디며, 저장된 값이 없으면 Explorer이고, 원격 컨텍스트에서 Git은 local-only 안내만 보인다 | machine | - |
| AC9 | 임시 저장소 픽스처에서 행은 primary("main worktree" 배지) → pane 열린 worktree → 마지막 커밋 시각 내림차순이고 bare가 없으며, linked worktree가 없는 저장소는 main 행 아래 "No linked worktrees yet"을 갖는다 | machine | - |
| AC10 | 픽스처의 각 행이 정확한 값을 갖는다: 병합된 브랜치 merged=참, squash 병합 브랜치 merged=거짓에 ahead>0, push 안 된 브랜치 ↑N, upstream 없는 브랜치 "no upstream", 원격 브랜치가 지워진 브랜치 "upstream gone", dirty worktree dirty=참, detached 행 짧은 SHA, 중첩 worktree를 품은 행 nested=참, 디렉터리를 지운 행 "missing on disk", 디스크 용량과 stamp가 측정 뒤 채워짐 | machine | - |
| AC11 | "Set as base branch"가 헤더의 base와 merged, ahead/behind를 새 기준으로 바꾸고 `ui_state`에 프로젝트 경로로 영속되며, 지정된 브랜치를 지우면 기본으로 돌아가고 헤더가 fallback을 말하고, detached 행에는 그 메뉴가 없으며, 보호가 옛 기준 worktree에서 새 기준 worktree로 옮겨 간다 | machine | - |
| AC12 | 계측된 실행에서 Git 섹션이 숨겨진 동안 디스크 측정이 0회이고, 섹션을 열면 worktree마다 한 번, 새로고침에 다시 한 번이며, gh는 저장소당 한 번과 새로고침에만 불리고, 로컬 상태는 worktree 이벤트나 HEAD 변화 없이 20초를 두어도 다시 읽히지 않으며, 각 값에 측정 시각이 있다 | machine | - |
| AC13 | gh가 PATH에 없음, 로그아웃, 네트워크 실패, GitHub 원격 없음의 네 경우에 PR 아이콘 자리가 각각 다른 범주 문구를 보이고, PR이 여럿인 브랜치는 open이 이기고 없으면 최신이며, detached 행의 PR 자리는 비어 있고 툴팁이 "no branch"다 | machine | - |
| AC14 | 첫 계산 중 "Reading worktrees" 스피너, 새로고침 중 버튼 비활성과 스피너, 실패한 값의 "—"와 툴팁 원문, main worktree 경로가 없을 때의 "Repository unavailable: <path>" 단일 안내가 각각 그 상태에서만 나타난다 | machine | - |
| AC15 | 조립된 dev 앱의 Git 섹션에서 여덟 행이 겹침 없이 한 화면에 들어가고 merged, dirty, pushed, PR 아이콘이 색과 모양만으로 구분된다 | judged | 픽스처 저장소를 포커스한 Git 섹션 캡처 한 장과, gh를 PATH에서 뺀 상태의 캡처 한 장 |
| AC16 | 삭제 게이트가 dirty, 중첩, primary, 기준 브랜치 worktree, 원격 컨텍스트 각각에서 이유와 함께 차단하고, unmerged, not pushed, running agents가 경고로 나열되며, pane이 열린 worktree의 버튼이 "Close N panes and delete"이고, 같은 worktree에 대해 카드, 사이드바 메뉴, Git 섹션이 같은 판정을 보인다 | machine | - |
| AC17 | Herdr가 pane 닫기를 거부하면 git 명령이 실행되지 않고 worktree가 그대로이며 이유가 보이고, 닫기 확인 뒤 `git worktree remove`가 실패하도록 주입하면 worktree 디렉터리와 등록이 그대로이고 메시지가 실패를 말하며 다시 시도할 수 있다 | machine | - |
| AC18 | 브랜치 삭제 체크박스는 merged(팁이 기준의 조상) 행에만 있고 PR 배지만 merged인 squash 병합 행에는 없으며, 선택하면 제거 뒤 `git branch -d`로 브랜치가 사라지고 `-D`는 실행되지 않으며, 브랜치 삭제가 실패하도록 주입하면 행은 사라지고 "branch <name> remains" 안내에 git의 이유가 있다 | machine | - |
| AC19 | 디렉터리를 지운 linked worktree의 Delete가 --force나 prune 없이 등록을 해제하고 브랜치를 남기며, primary가 아닌 missing 행에만 적용된다 | machine | - |
| AC20 | 격리 서버에서 pane이 열린 worktree를 Git 섹션에서 삭제하면 그 pane이 Herdr에서 닫히고 디렉터리가 사라지며 행이 없어지고, 이 실행이 만들지 않은 pane과 workspace는 그대로다 | judged | 삭제 전후의 `herdr pane list`와 `git worktree list` 출력, Git 섹션 캡처 |
| AC21 | 격리 서버에서 pane 없는 행의 Open이 workspace와 pane을 만들어 행의 pane 수가 1이 되고, pane 둘이 열린 행의 Open은 `last_activity`가 최신인 pane에 포커스를 둔다 | judged | Open 전후의 `herdr pane list`와 사이드바 캡처, 포커스된 pane id |
| AC22 | 새로고침 뒤 디렉터리를 지우고 Open을 누르면 Herdr의 거부 문구가 행에 인라인으로 보이고 다음 읽기에서 행이 missing on disk로 바뀌어 Open이 사라진다 | machine | - |
| AC23 | 삭제 확인창이 결과(unmerged, not pushed, running agents, 용량, pane 닫힘 뒤 실패 시의 상태)를 운영자가 확인 전에 읽을 수 있는 문장으로 보이고 브랜치 삭제 체크박스의 뜻이 분명하다 | judged | 경고가 셋 있는 merged 행과 pane이 열린 행 각각의 확인창 캡처 |
| AC24 | `AGENTS.md`의 Runtime Architecture가 Git 섹션의 세 갱신 계층을 한 줄로 서술하고, 새 색이나 아이콘이 있으면 `DESIGN.md`와 `HideTheme`에 토큰으로 있으며 뷰에 인라인 값이 없다 | machine | - |
| AC26 | gh 호출은 `gh pr list`와 `gh auth status`의 읽기 전용 명령뿐이고, 프롬프트가 비활성화된 환경과 타임아웃 아래에서 실행되며, 타임아웃이 지나면 프로세스가 종료되고 값은 "network or rate limit"로 남고, 앱은 어떤 경로에도 토큰이나 gh 설정을 쓰지 않으며 `gh auth login`을 포함한 어떤 쓰기 명령도 만들지 않는다 | machine | - |
| AC25 | 단일 인스턴스의 조립된 dev 앱을 시스템 load 14 미만에서 Git 섹션을 보인 채 20초 유휴로 두면 그 동안 앱 프로세스의 자식으로 새 git이나 du 프로세스가 0개 생기고, 3초 sample 창 5개에서 메인 스레드의 mutex 대기 표본 비율의 평균이 섹션을 숨긴 같은 프로토콜의 평균보다 0.5 퍼센트포인트 넘게 높지 않으며, 심볼화에 실패한 창은 세지 않고 다시 잰다 | judged | 표시/숨김 각각에 대해 load 기록, 20초 동안 1초 간격의 자식 프로세스 목록, 3초 sample 창 5개의 출력과 심볼화 실패 수, 두 평균의 표 |

## 8. PRD-Level Tasks

- T1. core 계보 투영: 에이전트 행에 부모 pane id, 깊이, 배지, 고아 힌트, raised 힌트를 싣고 접힘 상태를 `ui_state`에 영속하며, 회귀 테스트로 중첩, 고아 fallback, 접힘 round-trip, raised 평평함, 읽음 기록 불변을 잠근다. Covers R1, AC1, AC2, AC3, AC4, SC1. Depends on: none.
- T2. 사이드바 트리 렌더: 들여쓰기(깊이 3부터 축소), worktree 배지, chevron, 흐린 힌트를 `HideTheme` 토큰으로 그리고 "primary" 배지 문구를 "main worktree"로 통일한다. Covers R2, AC5, SC1. Depends on: T1.
- T3. sasu `--from-pane` 위임: `~/projects/sasu`에 Herdr pane을 열어 claude(opus) 에이전트에게 브리프(범위 A-14, 뒤처진 헬퍼 사실, `--from-pane "$HERDR_PANE_ID"`, 그 저장소의 테스트와 커밋 관례, 커밋 해시 회신)를 보내고, 커밋 해시와 테스트 결과를 받아 receipt에 기록한다. Covers R3, AC7. Depends on: none.
- T4. core worktree 모델 확장: merged, upstream 상태, 마지막 커밋 시각, 중첩, pane 수, 행별 디스크 용량과 stamp, PR 매핑과 네 실패 범주, 기준 브랜치 우선순위와 fallback, 삭제 게이트 판정과 보호를 `WorktreeSnapshot`과 `ProjectWorktreesSnapshot`에 싣고, main의 `remove_offered` 규칙을 지우며, 임시 저장소 픽스처로 모든 파생 값을 잠근다. Covers R6, R7, R8, R11, AC10, AC11, AC12, AC13, AC16, AC26, SC2. Depends on: none.
- T5. Git 섹션 추가: `RightPanelSection::Git`, 영속 파싱, 원격 안내, `scripts/check-right-panel-sections.sh`와 섹션 단언 테스트를 세 섹션으로 갱신한다. Covers R4, AC8. Depends on: none.
- T6. Git 섹션 뷰: 헤더(base, 새로고침), 행 목록과 정렬, 행 필드, 상태 셋, 컨텍스트 메뉴(Open, Reveal in Finder, Copy path, Set as base branch, Delete worktree…)와 Open의 인라인 거부. Covers R5, R9, R10, AC9, AC14, AC15, AC22, SC2, SC4. Depends on: T4, T5.
- T7. 삭제 흐름: core 이벤트로 pane 닫기와 Herdr 확인, 저장소 루트에서 동작하는 remover, 선택적 브랜치 삭제와 부분 실패 안내, missing 행의 등록 해제, 카드와 사이드바 메뉴의 게이트 채택, 확인창 문구. fault-injection 테스트로 닫기 거부, 제거 실패, 브랜치 삭제 실패를 잠근다. Covers R11, R12, AC17, AC18, AC19, AC23, SC3. Depends on: T4.
- T8. 격리 검증 픽스처와 실행: HERDR_SOCKET_PATH로 격리한 임시 Herdr 서버, 임시 저장소(병합, squash 병합, 미push, upstream 삭제, dirty, detached, 중첩, 디렉터리 삭제), 계보 에이전트 셋, pane이 열린 worktree, pane 없는 worktree를 만들고 AC5, AC6, AC15, AC20, AC21, AC23, AC25의 실행과 캡처를 `agents/runs/hide-agent-tree-and-worktree-panel/` 아래에 남기며 끝에 전부 정리한다. Covers AC6, AC20, AC21, AC25, SC1, SC3, SC4. Depends on: T2, T3, T6, T7.
- T9. 문서: `AGENTS.md` Runtime Architecture에 갱신 계층 한 줄, 필요한 토큰을 `DESIGN.md`와 `HideTheme`에 추가. Covers R13, AC24. Depends on: T6.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust core와 Swift 셸의 빌드, 섹션 검사 스크립트, Herdr 계약 검사, 문서 | none |
| automated behavior | yes | 계보 투영, worktree 파생 값, 게이트, 삭제 순서, 영속, 갱신 계약의 회귀 | none |
| app runtime | no/blockable | 트리, Git 섹션, 확인창의 시각 판정 | judge 백엔드 회복 뒤의 시각 판정 (D-22) |
| live herdr integration | yes | 격리 서버에서의 dispatch 계보, 삭제, 열기 | 이 PRD가 승인한 격리 경계 |
| performance measurement | yes | 섹션이 보이는 유휴 상태의 subprocess와 mutex 대기 | none |
| delegated repository change | yes | sasu 저장소의 `--from-pane` 커밋 | none |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R13, AC8, AC24 | Rust core와 Swift 셸이 깨끗이 빌드되고, 갱신된 섹션 검사 스크립트가 세 섹션을 통과하며, Herdr 계약 검사가 핀된 계약과 일치하고, 문서와 토큰이 갱신되어 있다 | yes | no |
| V2 | automated behavior | R1, R4-R12, AC1-AC4, AC8-AC14, AC16-AC19, AC22, AC26, SC2 | 회귀 위험을 직접 겨냥한 테스트가 있다: 자식이 평평해지는 것, 고아가 사라지는 것, 접힘이 자동으로 풀리는 것, 읽음 기록이 계보에 흔들리는 것, squash 병합이 merged로 읽히는 것, upstream gone이 pushed로 읽히는 것, 기준 변경 뒤 보호가 옛 worktree에 남는 것, 세 표면의 게이트가 갈라지는 것, 닫기 거부 뒤 git이 도는 것, 제거 실패 뒤 상태가 어긋나는 것, missing 행 삭제가 --force를 쓰는 것, 숨겨진 섹션이 du를 부르는 것, 섹션 영속이 깨지는 것, gh 호출이 읽기 전용과 프롬프트 비활성, 타임아웃의 경계를 벗어나거나 토큰을 남기는 것. 각 테스트는 snapshot과 실행된 명령줄, 환경, 파일 시스템 부작용을 단언한다 | yes | no |
| V3 | app runtime | R2, AC5, AC15, AC23, SC1 | 조립된 dev 앱의 캡처에서 트리 계층, Git 섹션 밀도와 아이콘 구분, 확인창 문구가 운영자에게 읽힌다 | no | yes |
| V4 | live herdr integration | R3, R10, R12, AC6, AC20, AC21, SC1, SC3, SC4 | 격리 서버에서 갱신된 sasu dispatch가 계보를 남겨 Hide가 중첩하고, pane이 열린 worktree 삭제가 pane을 닫고 디렉터리를 없애며, pane 없는 worktree의 Open이 pane을 만들고 다중 pane의 Open이 최신 pane을 포커스한다 | yes | no |
| V5 | performance measurement | R8, AC25 | AC25의 프로토콜(load 14 미만, 20초 유휴, 1초 간격 자식 프로세스 목록, 3초 sample 창 5개, 심볼화 실패 창 제외)로 표시/숨김을 재어 새 git/du 프로세스 0개와 mutex 대기 평균 차이 0.5 퍼센트포인트 이내를 보인다 | yes | no |
| V6 | delegated repository change | R3, AC7 | receipt에 기록된 sasu 커밋에서 Observer와 escalate dispatch 경로가 `--from-pane`을 넘기고 그 저장소의 테스트가 통과한 결과가 링크되어 있다 | yes | no |

Live 모드의 부작용 경계:

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked | Allowed Side Effect | Sensitive Data Policy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| V4 | live herdr integration | R3, R10, R12, AC6, AC20, AC21, SC1, SC3, SC4 | 위와 같음 | yes | no | HERDR_SOCKET_PATH로 격리한 임시 Herdr 서버 하나와 일회용 픽스처 저장소 안의 worktree, workspace, pane, 에이전트를 만들고 닫고 지운다. 운영자의 live 서버, 이 실행이 만들지 않은 pane과 workspace, 이 저장소의 worktree는 건드리지 않는다. 격리 서버와 픽스처는 끝에 전부 정리한다 | 캡처와 출력에서 홈 디렉터리 경로, 호스트명, gh 토큰을 가린다 |
| V6 | delegated repository change | R3, AC7 | 위와 같음 | yes | no | `~/projects/sasu`에 위임 에이전트의 커밋 하나. push, PR, 다른 저장소 변경 없음 | 없음 |

### 9.3 Human Verification

- 트리 들여쓰기와 배지, Git 섹션의 밀도와 아이콘 가독성, 삭제 확인창 문구의 시각 판정 (AC5, AC15, AC23).
  codex judge 쿼터가 2026-09-07에 회복된 뒤 judge 또는 운영자가 T8의 캡처로 판정한다.
- A-03(로컬 상태 갱신을 사이드바 cadence에 맡김), A-05(저장소당 한 번의 gh 조회), A-06(카드와 사이드바 메뉴에도 통합 게이트 적용), A-07(기준 브랜치 우선순위에서 PR base를 지정값 다음에 둠), A-20(성능 측정 프로토콜)의 사후 승인 또는 거부. spec gate가 이 다섯을 위임 실행 아래의 가정으로 기록했다.
- squash 병합된 브랜치의 삭제를 앱에서 허용할지는 사용자 결정으로 남는다. 이 PRD는 허용하지 않는다(A-09).

## 10. Risks And Open Decisions

- 삭제 게이트가 틀리면 운영자의 작업이 사라진다.
  완화: 차단이 경고에 우선하고, dirty와 중첩은 항상 차단이며, 브랜치 삭제는 안전 삭제가 먼저이고, 모든 경로가 확인창 뒤에 있으며, fault-injection 테스트(AC17, AC18)가 부분 실패를 잠근다.
- pane을 닫은 뒤 제거가 실패하면 pane은 닫힌 채 worktree만 남는다.
  사용자가 Q7에서 받아들인 결과이고 확인창이 미리 말한다.
- 행별 du는 큰 worktree에서 느리다.
  완화: 섹션이 보일 때 열기와 새로고침에만, 워커에서 순차로, stamp와 함께. V5가 유휴 비용을 잰다.
- gh 실패 범주 넷을 gh의 출력에서 구분하는 규칙은 gh 버전에 따라 달라질 수 있다.
  완화: 구분 불가면 원문 오류를 툴팁에 남기고 범주는 "network or rate limit"로 둔다. 구현 보고에 규칙을 적는다.
- sasu 위임이 실패하거나 커밋이 없으면 AC6과 AC7이 막힌다.
  같은 blocker 서명은 한 번만 자동 복구하고, 반복되면 Observer가 사용자에게 올린다.
- 이 실행의 Implementor pane 자체는 계보를 갖지 않는다(A-13).
  트리의 증명은 격리 서버 픽스처에서 온다.
- 스크린샷 판정은 2026-09-07 이후로 미뤄진다(A-15).
  receipt는 V3 없이 완료될 수 있고 9.3이 그 항목을 남긴다.
- `herdr-runtime-owned`의 Herdr 핀 변경과의 병합은 별도 작업이다(A-01).
- 증거는 `agents/runs/hide-agent-tree-and-worktree-panel/` 아래에만 두고 커밋하지 않는다.

## 11. Implementation Guardrails

운영자의 지침과 이 저장소의 규칙에서:

- 이 실행이 만들지 않은 Herdr pane, 탭, workspace를 닫거나 옮기거나 포커스하거나 프롬프트를 보내지 않는다. 운영자의 live 서버에 대한 검증은 없다. 모든 live 검증은 HERDR_SOCKET_PATH로 격리한 임시 서버에서 하고 끝에 정리한다(D-29, memory `hide-e2e-isolation-needs-socket-path`).
- 6장을 넘어 범위를 넓히지 않고, 5장을 넘어 구조를 바꾸지 않으며, 서드파티 의존성을 추가하지 않는다. 특히 push, merge, PR 생성, worktree 생성, prune, `--force`를 추가하지 않는다.
- 계보의 원천은 Herdr의 `spawned_from_pane_id`뿐이다. pane metadata 토큰이나 이름 규약으로 계보를 추론하지 않는다(D-09).
- `INV-herdr-unseen-token`: 계보 투영은 읽음 기록과 demand 축을 건드리지 않으며, `herdr-core/src/sidebar.rs`의 axis와 read record 테스트는 그대로 통과해야 한다.
- Herdr 통합은 `AGENTS.md` "Herdr API Contract"를 따른다: 공식 문서를 읽고, `herdr api schema --json`과 `contracts/herdr-api.schema.json`으로 확인하며, 기존 호출부에서 추측하지 않는다. `scripts/check-herdr-contract.sh`가 핀 자체의 문제로 실패하면(memory `herdr-pinned-release-is-protocol-20`) 핀을 고치지 말고 보고한다.
- 성능은 `AGENTS.md` "Performance Guide"를 따른다: runtime mutex를 subprocess, 블로킹 I/O, 큰 직렬화에 걸쳐 잡지 않고, per tick이나 per event 경로에서 git이나 du를 부르지 않으며, 새 snapshot 필드의 채널을 정하고(A-11) 매 틱 restamp되는 필드를 만들지 않으며, 성능 주장은 단일 인스턴스에서 짧은 sample 창 여러 개로 증명한다.
- 디자인은 `DESIGN.md`와 `HideTheme` 토큰을 따른다. 새 색, 반경, 간격은 토큰으로 추가하고 뷰에 인라인 값을 쓰지 않는다. 채도 높은 색은 chrome에 쓰지 않는다.
- 증거는 `AGENTS.md` "Evidence Belongs Outside The Repository"를 따른다: 캡처, sample 출력, 픽스처 로그는 `agents/runs/hide-agent-tree-and-worktree-panel/`에 두고 커밋하지 않으며, `agents/` 아래 어떤 것도 force-add하지 않는다.
- sasu 변경은 위임 에이전트만 한다. Hide Implementor는 `~/projects/sasu`의 파일을 직접 편집하지 않는다(D-20). 위임 범위는 A-14를 넘지 않는다.
- engineering/principles.md 규칙 1: main의 `remove_offered` / `remove_blocked_reason` 규칙, 대상 디렉터리 안에서 `rev-parse`하는 remover 경로, 단일 경로 `DiskRequest`, 두 섹션 단언을 같은 변경에서 지운다.
- engineering/principles.md 규칙 2, 5: 삭제 게이트는 core의 한 함수가 계산하고 세 표면은 읽기만 한다. 셸에 두 번째 판정을 두지 않는다.
- engineering/principles.md 규칙 3: T1/T4/T5가 먼저, 뷰(T2, T6)가 그 위에, 삭제(T7)가 그 위에, 격리 검증(T8)이 마지막이다.
- engineering/principles.md 규칙 4, 10: git, du, gh, Herdr의 실패는 조용히 넘기지 않고 값의 "—"와 툴팁, 행의 인라인 문구, 안내로 드러나며 원문 이유를 담는다.
- engineering/principles.md 규칙 7: 기존 `WorktreeReader`, `GithubReader`, `DiskReader`, `BackgroundRead`, `CheckoutCardPresentation`, `HideUI` 확인창을 확장하고 병렬 구현을 만들지 않는다.
- engineering/principles.md 규칙 8: 기준 브랜치 지정과 접힘 상태는 `ui_state`의 장기 필드다. 임시 저장소를 두지 않는다.
- engineering/principles.md 규칙 9: 삭제 흐름의 각 단계(닫기 요청, 확인, 제거, 브랜치 삭제)와 실패는 경로와 pane id를 담은 구조화된 진단으로 남긴다.
- engineering/principles.md 규칙 11: 같은 worktree에 대한 반복 삭제 요청, 반복 새로고침, 반복 Open은 같은 상태로 수렴하고 중복 subprocess를 만들지 않는다.
- engineering/principles.md 규칙 12, 13: 테스트는 snapshot 값과 실행된 명령을 단언하고, 게이트는 표면마다가 아니라 core에서 한 번 고친다.
- design/principles.md 규칙 3: 가장 잦은 동작(상태 읽기)은 클릭 없이 행에 있고, 삭제는 메뉴 한 번과 확인 한 번이다.
- design/principles.md 규칙 4: merged, ahead/behind, pushed, 보호 여부는 파생 상태로 보이며 운영자가 계산하지 않는다.
- design/principles.md 규칙 5: 상태 표시는 ChangesView 패턴을, 확인창은 기존 HideUI alert를, 배지는 사이드바의 기존 배지를 따른다. "primary"와 "main worktree"를 하나로 한다.
- design/principles.md 규칙 6: 파괴적 동작 전에 결과(unmerged, not pushed, running agents, 용량, pane 닫힘 뒤 실패 시의 상태)를 문장으로 말한다.
- design/principles.md 규칙 7: 상태는 색과 아이콘으로 먼저 보이고 문장은 툴팁과 안내에만 쓴다.
- Git과 PR 귀속: 브랜치 이름, 커밋 메시지, 트레일러, 생성 텍스트 어디에도 에이전트, 모델, 벤더, 도구 이름을 쓰지 않는다. 위임 에이전트의 sasu 커밋도 같다.

## 12. Implementation Result Report Contract

보고 항목:

- status: `Done`, `Partially Done`, `Blocked`.
- 사용자에게 보이는 변화: 트리, Git 섹션, 삭제 흐름 각각.
- 바뀐 모듈과 새 모듈의 책임 경계, 실제로 고른 파일 구조.
- 5장의 구조를 따랐는지, 벗어난 곳과 이유.
- T1부터 T9까지의 완료 상태와 R/AC/V 커버리지.
- 모드별 검증 증거와 각 산출물이 있는 실행 디렉터리.
- V6에 대해: sasu 커밋 해시, 그 저장소 테스트 결과의 링크, 위임 에이전트의 pane id.
- V4에 대해: 격리 서버의 소켓 경로, 만든 픽스처 저장소와 worktree, workspace, pane, 에이전트의 목록과 그 전부가 정리되었다는 확인, 운영자의 live 서버를 건드리지 않았다는 확인.
- V5에 대해: 단일 인스턴스 확인, 창 수, 심볼화 실패 창 수, 표시/숨김 각각의 수치.
- V3에 대해: 캡처 경로와 "judge 쿼터 회복 뒤 판정 대기" 상태.
- gh 실패 범주를 구분한 규칙.
- 추가되거나 바뀐 자동 테스트와 각각이 막는 회귀.
- 삭제된 코드 경로의 목록.
- 4.3의 가정 A-01..A-20 중 구현에서 실제로 의존한 것과 추가된 가정.
- 이탈, 남은 인간 검토(9.3), 미완 항목과 후속 후보.
- 배포 결과: 실행 worktree의 로컬 커밋 해시.
