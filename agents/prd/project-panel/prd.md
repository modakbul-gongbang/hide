---
topic: "프로젝트 중심 좌우 패널: worktree 목록, GitHub PR 상태, 요약 카드"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "사용자 대면 UI와 로컬 gh/git 읽기 전용 조회가 중심이며, 유일한 파괴적 동작(worktree 삭제)은 기존 확인창을 재사용하고 앱은 토큰이나 원격 상태를 변경하지 않는다."
source_intake: "agents/interview/project-panel/qa-log.md"
created_at: "2026-09-03"
updated_at: "2026-09-03"
---

# PRD: 프로젝트 중심 좌우 패널: worktree 목록, GitHub PR 상태, 요약 카드

## 1. Summary

좌측 Projects 트리가 저장소의 **모든 worktree**를 checkout 행으로 보여주고(pane 유무와 무관), 각 행에 PR 상태 배지·커밋 안 된 변경 점·agent 수를 싣는다.
행을 선택하면 우측 패널 상단에 그 checkout의 **요약 카드**가 나타나 브랜치, PR base 대비 ahead/behind, 미push 커밋 수, 변경 파일 수, PR 상태(gh 조회)와 갱신 시각, agent/포트, 디스크 용량을 기호와 숫자 위주로 보여준다.
카드에서 PR을 브라우저로 열고, merged/closed 된 worktree는 같은 자리에서 정리한다.
PR 정보는 로컬 `gh` CLI로만 읽으며 앱은 토큰을 갖지 않는다.

이 변경은 "worktree마다 agent를 돌리고, 그 결과를 검토해 PR로 넘긴 뒤 머지됐는지 확인한다"는 흐름을 한 화면 안에서 끝내기 위한 것이다.

Approval checklist:

- 범위 경계: worktree 목록은 Projects 트리 안, 별도 Branches 뷰 없음. 앱 내 Commit/Create PR, worktree 생성, 브랜치 삭제, 토큰 소유, pushed 배지, 비GitHub 호스팅은 비목표 (3장).
- 구조 변경: core에 worktree 카탈로그·gh PR 조회·디스크 측정 리더 3개와 스냅샷 필드가 추가되고, 모두 런타임 잠금 밖에서 실행된다 (5장).
- 외부 도구: `gh` CLI 의존, 조회는 프로젝트당 5분 + agent 종료 시 + 수동 (R7, R9).
- 파괴적 동작: Remove worktree 버튼은 merged/closed에서만, agent 실행 중·변경 있음이면 비활성 (R5).
- 검증 모드: 자동 테스트 + 실제 GitHub 비공개 픽스처 저장소 `hide-e2e-fixture`에서 open/merged/closed e2e, review 상태는 고정 출력 테스트 (9장).
- 픽스처 저장소는 검증 후 유지 (4.1, 9.2).
- delivery mode: local (`agents/config.json` 기본값).

## 2. Problem, Goal, And Users

사용자는 한 저장소에서 worktree 여러 개를 만들고 각 worktree에 agent를 붙여 병렬로 일한다.
지금 사이드바는 pane이 있는 worktree만 보여주고, 우측 패널은 파일 트리와 변경 목록뿐이라 "어느 worktree에 검토할 게 있는지", "PR로 넘겼는지", "머지됐는지"를 알려면 터미널이나 브라우저로 나가야 한다.
그 결과 머지된 worktree가 정리되지 않고 쌓이고, 프로젝트 안에서 무엇이 진행 중인지 한눈에 보이지 않는다.

목표: 사용자가 좌측에서 검토할 worktree를 고르고, 우측 카드에서 상태를 확인한 뒤, PR을 열거나 정리하는 것까지 앱 안에서 끝낸다.

사용자: 이 앱의 단일 사용자(개발자). 역할 구분은 없다.

### 2.1 User Scenarios

- SC1. worktree 검토 후 PR 상태 확인: 사용자가 프로젝트를 펼쳐 worktree 행을 보고, 하나를 골라 카드에서 상태를 확인하고 PR을 연다.
  Actors: 사용자.
  Primary path: 프로젝트를 펼치면 모든 worktree가 행으로 보이고(pane 없는 행은 흐리게), 각 행에 PR 배지·dirty 점·agent 수가 있다. 행을 선택하면 우측 상단 카드에 브랜치, PR base 대비 ahead/behind, 미push 커밋 수, 변경 수, PR 배지와 갱신 시각, agent/포트, 용량과 가장 큰 하위 폴더가 기호·숫자 위주로 보인다. 첫 조회 중인 줄은 스피너다. PR 줄을 클릭하면 브라우저에서 PR이 열린다.
  Failure state: gh가 없거나 미로그인이면 PR 줄과 배지가 비고 카드에 `gh auth login` 안내 한 줄만 있다. gh 조회가 실패하거나 오프라인이면 마지막 성공값이 갱신 시각과 함께 남고 배지는 흐려지며 실패 사유는 카드에서만 보인다. git 저장소가 아닌 폴더 프로젝트는 브랜치/PR/변경 줄이 없고 이름·용량·agent/포트만 있다.
  Recovery: 카드의 새로고침 버튼, 그 checkout의 agent가 working에서 벗어날 때의 자동 재조회, 5분 주기 재조회.
  Reach: 픽스처 저장소에 worktree 3개(PR 없음, open PR, merged PR), `-u`로 push한 뒤 로컬 커밋 1개를 더한 브랜치 하나, 한 번도 push하지 않은 브랜치 하나를 만들고 앱에 등록한다. gh 로그아웃 상태와 네트워크 차단 상태는 각각 `gh auth logout`과 gh 호출 실패 주입으로 만든다.

- SC2. 머지된 worktree 정리: 배지가 merged 또는 closed로 바뀐 worktree를 카드에서 삭제한다.
  Actors: 사용자.
  Primary path: 행이 흐려지고 배지가 `merged`(또는 `closed`)로 바뀐다. 카드에 `merged · N days ago`와 Remove worktree 버튼이 보인다. 버튼을 누르면 기존 확인창이 용량과 함께 뜨고, 확인하면 worktree가 삭제되고 행이 사라진다. 로컬 브랜치는 남는다.
  Failure state: 그 worktree에 agent가 실행 중이거나 커밋 안 된 변경이 있으면 버튼이 비활성화되고 이유가 한 줄로 보인다. 삭제 실패는 기존 오류 안내로 표시된다.
  Recovery: agent를 끝내거나 변경을 정리하면 버튼이 활성화된다.
  Reach: 픽스처의 merged worktree에 커밋 안 된 파일을 두어 비활성 상태를, agent를 그 worktree에서 실행해 두 번째 비활성 상태를 만들고, 둘을 치운 뒤 삭제한다. closed 케이스는 별도 PR을 닫아 만든다.

- SC3. pane 없는 worktree에서 작업 시작: 흐린 worktree 행을 선택해 터미널을 연다.
  Actors: 사용자.
  Primary path: 흐린 행을 선택하면 카드가 그 worktree를 보여주고 본문은 빈 상태와 Start new terminal 버튼이다. 버튼을 누르면 pane이 생기고 행이 정상 밝기로 바뀌며 agent 수가 갱신된다.
  Failure state: worktree 경로가 디스크에 없으면 행에 기존 `missing` 배지가 붙고 시작 버튼은 없다.
  Recovery: 기존 missing 처리(경로 복구 또는 등록 삭제).
  Reach: 픽스처 저장소에 git으로 worktree를 추가만 하고 pane은 열지 않은 상태.

## 3. Scope And Non-Goals

포함:

- Projects 트리의 checkout 행 확장: `git worktree list` 기반으로 저장소의 모든 worktree를 행으로 표시, pane 없는 행은 흐리게 + Start new terminal, 행 배지 3종(PR 상태, dirty 점, agent 수), merged/closed 행 흐림.
- Changes 섹션 개편: UNCOMMITTED / COMMITTED ON BRANCH 두 그룹, 파일 행에 디렉터리·줄 델타·상태 글자.
- 우측 패널 상단 요약 카드(선택된 checkout 기준): 두 줄 헤더(브랜치 + 총 델타, → base + ↑↓), 브랜치, PR base 대비 ahead/behind, 미push 커밋 수(`↑N`), 변경 파일 수, PR 번호·상태·갱신 시각, agent 수와 상태, 열린 포트, 용량과 가장 큰 하위 폴더, 새로고침, Open PR(브라우저), Remove worktree(merged/closed에서만).
- `gh` CLI를 통한 PR 조회와 갱신 규칙, 기준 브랜치(PR base, 없으면 저장소 기본 브랜치).
- gh 없음/미로그인, gh 실패/오프라인, 비-git 폴더, 첫 조회 중 로딩, worktree 경로 missing 상태.
- 디스크 용량 측정(선택 checkout 1개, 잠금 밖, 재선택/삭제 확인창에서 재측정).
- 검증용 비공개 GitHub 픽스처 저장소 생성과 e2e 시나리오.

비목표(의도적 제외, 사용자 결정):

- 앱 내 Commit / Create PR 버튼 (D-07). 사용자 결과: 커밋과 PR 생성은 agent나 터미널에서 한다. 재검토: 사용자가 앱에서 직접 커밋하고 싶다고 말할 때.
- worktree 생성 (D-24). 재검토: 사용자 요청 시.
- 로컬 브랜치 삭제 (D-08). worktree만 지우고 브랜치는 남긴다.
- 앱이 GitHub 토큰을 소유하거나 OAuth를 여는 것 (D-04, D-13).
- 좌측 사이드바의 별도 Branches 뷰 (D-05).
- `pushed` 배지 (D-10). 미push 여부는 카드의 `↑N`으로 본다.
- 비GitHub 원격(GitLab 등) 지원 (D-28, agent 가정). 이런 저장소는 gh 실패 상태로 보인다. 재검토: 다른 호스팅 요청 시.
- 커밋 로그/브랜치 그래프 같은 git 클라이언트 기능 (agent 가정, 인터뷰 Q 제안 단계에서 배제).
- worktree가 10개를 넘는 저장소의 트리 길이 대응(접기/필터) (D-20, deferred). 재검토: 긴 트리가 보고될 때.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

- 검증용 비공개 GitHub 저장소 `hide-e2e-fixture`를 만드는 것은 agent가 로그인된 `gh`로 수행하므로 사전 작업이 아니다. 대상 계정은 이 머신의 `gh auth status`가 보고하는 현재 로그인 계정(2026-09-03 확인: `yansfil`, 활성 계정)이며, 인터뷰는 계정을 따로 정하지 않았다. 저장소 생성이 그 계정에 영구 흔적을 남기므로, 사용자는 대상 계정과 저장소 이름·비공개 여부(D-18 기본값)를 PRD 승인으로 함께 승인한다. 이 항목은 사람만 줄 수 있는 계정 소유권 승인이다.

### 4.2 Human Decisions Before PRD Approval

- 3장의 범위와 비목표 승인.
- 5장의 구조 변경(core 리더 3개, 스냅샷 필드, 우측 패널 카드 섹션) 승인.
- `gh` CLI 의존과 조회 규칙(5분 + agent 종료 + 수동, `--limit 200`) 승인.
- Remove worktree 노출·비활성 규칙 승인.
- 검증 모드 승인: 자동 테스트 + 실제 GitHub 픽스처 e2e(open/merged/closed), review는 고정 출력만.
- 픽스처 저장소 이름 `hide-e2e-fixture`, 비공개, 검증 후 유지 승인.
- delivery mode `local` 승인.

### 4.3 Decision Traceability For Fidelity Review

Decision Register (qa-log D#) 처리:

- D-01 (fact, 프로젝트는 저장소 단위, worktree는 pane이 있어야 행): 현재 사실. R1이 "모든 worktree" 규칙으로 바꾼다.
- D-02 (fact, 우측 패널은 Explorer/Changes만, checkout 경로 기준): 현재 사실. R4가 카드를 추가한다.
- D-03 (user, 핵심 흐름 = 검토 → PR → 머지 확인): 2장 목표, SC1, R1/R4/R7.
- D-04 (user, gh CLI만, 토큰 없음): R7, 비목표, G3.
- D-05 (user, worktree 목록은 Projects 트리 안, pane 없는 행 흐리게): R1, SC3, 비목표(Branches 뷰).
- D-06 (user가 추천 수락, 행 배지 3종, merged 흐림): R2, AC2.
- D-07 (user, 카드 구성, 보기 + Open PR까지, Commit/Create PR 제외): R4, R6, 비목표.
- D-08 (user, Remove worktree: merged/closed에서만, 실행 중·변경 있음 비활성, 브랜치 삭제 제외): R5, AC5, 비목표.
- D-09 (user, 용량 표시, 잠금 밖 1회 측정, 재선택/삭제창 재측정): R8, AC7, G4.
- D-10 (user가 추천 수락, gh pr list 1회/프로젝트, 5분 + agent 종료 + 수동, 배지 5값, pushed 없음, 실패 시 마지막 값): R7, R9, AC3, AC8.
- D-11 (user, 기준 브랜치 = PR base, 없으면 기본 브랜치, merged = PR MERGED, gh 불가 시 로컬 기본 브랜치 대비): R4, R7, AC4.
- D-12 (user, 용량 옆 가장 큰 하위 폴더): R8.
- D-13 (user, gh 없음/미로그인 표시): R9, AC8.
- D-14 (user, 비-git 폴더 카드): R4, AC9.
- D-15 (user, 오프라인/실패 stale 표시): R9, AC8.
- D-16 (user, 자동 테스트 + 실제 GitHub e2e): 9장.
- D-17, D-26 (user, 비공개 픽스처 저장소, 검증 후 유지): 4.1, T10, V5.
- D-18 (assumption, 이름 `hide-e2e-fixture`, 비공개): 4.2 승인 항목. 가정으로 유지.
- D-19 (assumption, 모든 조회는 잠금 밖): G4. 저장소 Performance Guide 사실.
- D-20 (assumption, deferred, 긴 트리): 비목표.
- D-21 (user 수락, `↑N` 미push 항목, pushed 배지 없음): R4, AC4.
- D-22 (user 수락, Remove 버튼은 merged/closed에서만): R5.
- D-23 (user 수락, 배지 매핑표): R7 매핑표, AC3.
- D-24 (user 수락, worktree 생성 비목표): 비목표.
- D-25 (user 수락, review 상태는 고정 출력만): V2, V5.
- D-27 (user, 텍스트 최소·기호/숫자 우선): R10, G1, 9.3.
- D-28 (assumption, GitHub만): 비목표.
- D-29 (assumption, 첫 조회 스피너): R9, AC8.
- D-30 (assumption, gh 필드와 `--limit 200`): R7. 게이트 참고를 반영해 `baseRefName,number`를 추가했다.
- D-31 (assumption, 경계 상태 증명 단계): V3, V4.
- 후속 사용자 요청(2026-09-03, 소스 컨트롤 패널 캡처를 참고로 제시): 카드 두 줄 헤더 배치(R4)와 Changes 두 그룹·파일 행 구성(R12, AC13, T8)으로 반영. 캡처의 Message 입력과 Publish Branch는 D-07 비목표에 따라 거절.

감사 게이트 P2 참고의 처리(agent 가정, 사용자 결정으로 승격하지 않음):

- `↑N` 두 지표 구분: PR base 대비는 `↑2 ↓0 main`처럼 base 브랜치 이름을 뒤에 붙이고, 미push는 `↑N origin`처럼 리모트 이름을 붙인다 (R4).
- 배지 tie-break: 같은 브랜치에 PR이 여럿이면 OPEN이 MERGED/CLOSED보다 우선, 남으면 최신 `updatedAt`. draft는 reviewDecision과 무관하게 `open` (R7).
- upstream 없는 브랜치: 미push 항목 자체를 생략한다 (R4).
- `↑N > 0` 증명 단계: V5에 `-u` push 후 로컬 커밋 1개를 더한 브랜치와, push한 적 없는 브랜치(항목 생략)를 포함한다.

사용자가 거절하거나 유보한 것: Branches 뷰(Q3 b), 우측 패널 전용 Worktrees 섹션(Q3 c), 앱 내 커밋/PR 생성(Q5), 브랜치 함께 삭제(Q6 c), 행마다 용량 표시(Q7 c), 수동 새로고침만(Q8 c), 선택 checkout만 갱신(Q8 a), pushed 배지(Q9), push 감지 트리거(Q9), 로컬 main 대비 merge-base 판정(Q10 b), herdr-ide 자체를 픽스처로 쓰기(Q13 a).

Principles intake: `~/projects/oh-my-principle` (commit `35ab76c`)의 `engineering/principles.md`와 `design/principles.md`를 전부 읽었다. 두 도메인 모두 이 작업에 해당한다(코드 변경 + 사용자가 작업하는 화면). 적용 규칙은 11장 가드레일과 AC에 옮겼다. 번역하지 않은 규칙: engineering 6(기성 해법 탐색)은 `gh`와 `git worktree`를 그대로 쓰는 것으로 이미 충족되어 별도 가드레일로 두지 않았다.

## 5. Major Technical Structure Changes

- core(`herdr-core`)에 잠금 밖 리더 3개가 추가된다: worktree 카탈로그 리더(`git worktree list --porcelain`, 브랜치·dirty·ahead/behind·미push), GitHub PR 리더(`gh pr list`, `gh repo view`, `gh auth status`), 디스크 용량 리더(선택 checkout 1개). 셋 다 기존 `ChangesReader`처럼 session_sync 틱에서 요청을 읽고 결과만 런타임에 전달한다.
- 워크스페이스 카탈로그(`build_catalog`)가 pane이 있는 worktree뿐 아니라 저장소의 모든 worktree를 checkout으로 생성한다. checkout에 `has_panes`, PR 상태, dirty, ahead/behind, 미push, missing 필드가 붙는다.
- 스냅샷에 카드 섹션이 추가된다: 선택 checkout의 요약(브랜치, 기준 브랜치, 카운트들, PR, agent/포트, 용량, gh 가용성과 마지막 성공 시각, 실패 사유). 자주 바뀌지 않으므로 revisioned `rest` 채널에 싣는다.
- 우측 패널에 카드 섹션이 Explorer/Changes 위에 추가된다. 새 우측 섹션 enum 값은 없고, 카드는 어느 섹션에서나 상단에 보인다.
- 갱신 트리거: agent 상태 전이(working → 그 외)를 core가 감지해 해당 프로젝트의 PR 리더 요청을 즉시 갱신한다.
- 외부 의존: 로컬 `gh` CLI(로그인 상태는 사용자 소유). 앱은 네트워크 호출을 직접 하지 않는다.
- 새 서비스, 스키마, 잡, 인증 경계 없음. 원격(mini) 프로젝트는 이번 범위에서 카드가 로컬 프로젝트에만 나타난다(원격 checkout 선택 시 카드 없음).

## 6. Requirements

- R1. 프로젝트를 펼치면 저장소의 모든 worktree(`git worktree list`)가 checkout 행으로 보인다. pane이 없는 행은 흐리게 표시되고, 선택하면 본문 빈 상태의 Start new terminal로 pane을 만들 수 있다. 경로가 없는 worktree는 `missing` 배지를 단다.
- R2. 각 checkout 행은 PR 상태 배지(있을 때), 커밋 안 된 변경이 있으면 점, agent 수(있을 때)만 싣는다. PR 상태가 merged 또는 closed인 행은 흐리게 표시된다.
- R3. 행 선택은 기존과 같이 그 checkout을 focus하고 우측 패널의 기준 경로를 바꾼다.
- R4. 우측 패널 상단 카드는 선택된 로컬 checkout에 대해 다음을 보여준다. 헤더는 두 줄이다: 1행 브랜치 이름(worktree 경로는 툴팁)과 오른쪽 끝에 기준 브랜치 대비 커밋된 변경만의 총 줄 수 델타 `+A -D`(`git diff --shortstat <base>...HEAD`, 작업 트리 변경은 제외, 초록/빨강), 2행 `→ <base>`와 오른쪽 끝에 `↑A ↓B`. 그 아래에 upstream이 있을 때 미push 커밋 `↑N <remote>`, 변경 파일 수(클릭하면 Changes 섹션), PR 번호와 배지와 마지막 갱신 시각(클릭하면 브라우저로 PR), agent 수와 상태 점, 열린 포트(클릭하면 브라우저), 용량과 가장 큰 하위 폴더. git 저장소가 아닌 폴더는 이름, 용량, agent/포트만 보인다.
- R5. 카드는 PR 상태가 merged 또는 closed일 때만 Remove worktree 버튼을 보인다. 그 worktree에 agent가 실행 중이거나 커밋 안 된 변경이 있으면 버튼이 비활성화되고 이유가 한 줄로 보인다. 누르면 기존 worktree 삭제 확인창(용량 포함)이 뜨고, 확인 시 worktree만 삭제되며 로컬 브랜치는 남는다. merged/closed가 아닌 worktree의 삭제는 기존 checkout 우클릭 메뉴 `Delete worktree…` 경로가 그대로 남아 있고 이 변경은 그 경로를 바꾸지 않는다.
- R6. 카드의 PR 항목과 포트는 시스템 기본 라우팅(기존 링크 열기 규칙)으로 브라우저를 연다.
- R7. PR 정보는 `gh pr list --state all --limit 200 --json number,headRefName,baseRefName,state,reviewDecision,isDraft,url,mergedAt,updatedAt`을 프로젝트(저장소)당 1회 호출해 worktree 브랜치에 매핑한다. 배지 매핑: `merged`=MERGED; `closed`=CLOSED; `review`=OPEN이고 draft가 아니며 reviewDecision이 REVIEW_REQUIRED/CHANGES_REQUESTED/APPROVED(셋은 색으로 구분); `open`=OPEN이고 리뷰 없음 또는 draft; 없음=해당 브랜치 PR 없음. 한 브랜치에 PR이 여럿이면 OPEN 우선, 남으면 최신 `updatedAt`. 기준 브랜치는 PR의 base, PR이 없으면 `gh repo view --json defaultBranchRef`의 기본 브랜치, gh 불가 시 로컬 기본 브랜치.
- R8. 용량은 선택 checkout 1개만, 카드가 열릴 때 백그라운드에서 측정하며 측정 중에는 `measuring…`을 보인다. 재선택하거나 삭제 확인창을 열 때 다시 잰다. 가장 큰 1단계 하위 폴더 이름과 크기를 함께 보인다.
- R9. 갱신과 상태: PR 조회는 프로젝트당 5분 주기, 그 프로젝트의 checkout에서 agent가 working에서 벗어날 때 즉시, 카드의 새로고침 버튼으로 즉시. 첫 조회 중인 항목은 스피너. gh가 없거나 미로그인이면 PR 항목과 배지를 비우고 카드에 `gh auth login` 안내 한 줄. 조회 실패나 오프라인이면 마지막 성공값을 `as of N min ago`와 함께 유지하고 배지는 흐리게, 실패 사유는 카드에서만 보인다. 실패와 빈 결과는 구조화 로그로 남긴다.
- R12. Changes 섹션은 두 그룹으로 나뉜다: `UNCOMMITTED N`(현재 `git status`, 기존 동작)과 `COMMITTED ON BRANCH N`(기준 브랜치 대비 `git diff --stat <base>...HEAD`의 파일). 각 그룹은 헤더에 이름·개수·접기를 가지며, 파일 행은 파일명(밝게)·디렉터리(흐리게)·오른쪽 `+N -M`(초록/빨강)·상태 글자 `A/M/D`(색)로 구성된다. 파일 클릭은 기존처럼 diff를 연다(COMMITTED 그룹은 base 대비 diff). 기준 브랜치가 없는 경우(비-git 폴더)에는 UNCOMMITTED 그룹만 보인다. gh가 불가해도 R7의 로컬 기본 브랜치 대비로 COMMITTED 그룹은 계속 보인다. 사용자가 제시한 소스 컨트롤 패널 캡처(2026-09-03)를 배치 참고로 삼되 Message 입력과 Publish Branch는 비목표(D-07)에 따라 가져오지 않는다.
- R10. 행과 카드의 텍스트는 최소로 하고 상태는 기호·색·숫자로 표시한다. 문장형 안내는 gh 미로그인/실패 사유 같은 예외 상태에 한 줄만 허용한다. 색과 기호에는 툴팁 또는 접근성 라벨이 붙는다.
- R11. worktree, PR, 용량 조회는 모두 런타임 뮤텍스 밖에서 실행되고 결과만 잠금 안으로 전달된다. 어떤 조회도 매 틱 서브프로세스를 만들지 않는다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | pane이 없는 worktree와 있는 worktree가 모두 프로젝트 아래 행으로 보이고, pane 없는 행만 흐리며, 경로가 없는 worktree는 missing 배지를 단다 | judged | 스크립트 실행: 픽스처 저장소에 worktree 3개(그중 1개는 경로 삭제)를 두고 앱에서 프로젝트를 펼친 화면 캡처 |
| AC2 | 행에는 PR 배지, dirty 점, agent 수 외의 텍스트가 없고, merged/closed 행은 흐리다 | judged | 스크립트 실행: open/merged/closed/없음 네 상태의 worktree 행 캡처와 접근성 라벨 목록 |
| AC3 | gh 출력의 state/reviewDecision/isDraft 조합이 R7 매핑표대로 다섯 배지 값으로 결정되고, 한 브랜치에 PR이 여럿이면 OPEN 우선·최신 updatedAt 순이다 | machine | - |
| AC4 | 카드의 ahead/behind와 총 줄 델타 `+A -D`는 PR base(없으면 기본 브랜치) 대비 커밋된 변경 기준이고, 미push 항목은 upstream이 있을 때만 `↑N <remote>`로 보이며 upstream에 push한 뒤 로컬 커밋 1개를 더한 브랜치에서 N이 1이고, upstream이 없는 브랜치에서는 항목이 없다 | judged | 스크립트 실행: base 대비 2 ahead·upstream 대비 1 미push 상태의 카드 캡처와 같은 시점의 `git rev-list --count`, `git diff --shortstat` 값; upstream 없는 브랜치의 카드 캡처 |
| AC5 | Remove worktree 버튼은 merged/closed 카드에만 있고, agent 실행 중이거나 변경이 있으면 비활성이며 이유가 한 줄로 보이고, 활성 상태에서 확인하면 worktree 폴더만 사라지고 로컬 브랜치는 남는다 | judged | 스크립트 실행: open PR 카드(버튼 없음) → merged + dirty(비활성) → merged + agent 실행(비활성) → 정리 후 활성 → 삭제 후 폴더 부재와 브랜치 존재 확인 |
| AC6 | 카드의 PR 항목을 클릭하면 그 PR URL이 브라우저로 열린다 | judged | 스크립트 실행: 클릭 후 열린 브라우저 탭의 URL 기록 |
| AC7 | 용량은 선택 checkout에 대해서만 측정되어 크기와 가장 큰 하위 폴더가 보이고, 측정 중에는 measuring 표시가 있다 | judged | 스크립트 실행: 큰 하위 폴더를 둔 worktree 선택 직후(measuring)와 측정 완료 후의 카드 캡처, 다른 checkout으로 옮겼을 때 그 checkout의 값이 따로 측정되는 캡처 |
| AC8 | gh가 없거나 미로그인이면 PR 항목과 배지가 비고 안내 한 줄만 보이며 ahead/behind는 로컬 기본 브랜치 대비로 계속 보이고, 조회 실패 시 마지막 값이 `as of N min ago`와 함께 남고 배지는 흐려지며, 새로고침·agent 종료·5분 주기 세 트리거가 각각 재조회를 일으킨다 | judged | 스크립트 실행: gh를 PATH에서 뺀 상태의 카드 캡처(안내 + 로컬 기본 브랜치 대비 카운트), gh 로그아웃 캡처, gh 실패 주입 후 캡처, 세 트리거 각각의 재조회 로그 |
| AC9 | git 저장소가 아닌 폴더 프로젝트의 카드에는 브랜치·PR·변경 항목이 없고 이름·용량·agent/포트만 있다 | judged | 스크립트 실행: 비-git 폴더 프로젝트를 선택한 카드 캡처 |
| AC10 | worktree·PR·용량 조회는 잠금 밖에서 실행되고 요청이 같으면 주기 안에서 다시 실행되지 않는다 | machine | - |
| AC11 | 흐린 worktree 행을 선택하고 Start new terminal을 누르면 그 worktree 경로에 pane이 생기고 행이 정상 밝기로 바뀐다 | judged | 스크립트 실행: pane 없는 worktree 선택 → 시작 → pane 생성 후 행과 헤더 경로 캡처 |
| AC12 | 기존 Explorer 동작이 카드 추가 후에도 그대로이며, 카드는 두 섹션 모두에서 상단에 보인다 | judged | 스크립트 실행: Explorer/Changes 각 섹션에서 카드가 상단에 있는 캡처와 Explorer 파일 열림 확인 |
| AC13 | Changes 섹션이 UNCOMMITTED와 COMMITTED ON BRANCH 두 그룹으로 나뉘어 각 그룹 헤더에 개수가 있고, 파일 행에 디렉터리·`+N -M`·상태 글자가 보이며, COMMITTED 그룹의 파일을 클릭하면 base 대비 diff가 열리고, 비-git 폴더에서는 UNCOMMITTED 그룹만 보이며, gh가 PATH에 없어도 로컬 기본 브랜치 대비 COMMITTED 그룹이 보인다 | judged | 스크립트 실행: 커밋 2개와 미커밋 파일 1개가 있는 worktree의 Changes 캡처, COMMITTED 파일 클릭 후 diff 캡처, 비-git 폴더의 Changes 캡처, gh를 PATH에서 뺀 상태의 Changes 캡처 |

## 8. PRD-Level Tasks

- T1. worktree 카탈로그 리더: `git worktree list --porcelain`으로 저장소의 worktree, 브랜치, missing 여부를 읽고, 각 worktree의 dirty 여부·기준 브랜치 대비 ahead/behind·upstream 대비 미push 수를 잠금 밖에서 계산해 스냅샷으로 전달한다. Covers R1, R4, R11, AC10.
- T2. 카탈로그 통합: `build_catalog`가 pane 없는 worktree도 checkout으로 만들고 `has_panes`, missing, dirty, 카운트 필드를 싣는다. 기존 pane 기반 checkout과 병합하며 id 규칙(경로 기반)은 유지한다. Covers R1, R2, AC1. Depends on: T1.
- T3. GitHub PR 리더: `gh auth status`, `gh repo view`, `gh pr list`를 프로젝트당 실행해 브랜치별 PR 상태로 매핑(매핑표, tie-break)하고, 가용성·마지막 성공 시각·실패 사유를 함께 전달한다. 5분 주기와 즉시 갱신 요청을 받는다. Covers R7, R9, AC3, AC8, AC10. Depends on: none.
- T4. 갱신 트리거: agent 상태 전이(working → 그 외)와 카드 새로고침 이벤트를 core가 PR 리더 요청 갱신으로 연결한다. Covers R9, AC8. Depends on: T3.
- T5. 디스크 용량 리더: 선택 checkout의 총 용량과 가장 큰 1단계 하위 폴더를 잠금 밖에서 측정하고, 재선택·삭제 확인창 열림에 재측정한다. Covers R8, R11, AC7, AC10. Depends on: none.
- T6. 사이드바 행: 모든 worktree 행 렌더링(흐림, missing, 배지 3종, merged/closed 흐림), 접근성 라벨과 툴팁, 텍스트 최소 규칙. Covers R1, R2, R3, R10, AC1, AC2, AC11. Depends on: T2.
- T7. 요약 카드: 우측 패널 상단 카드 렌더링(두 줄 헤더와 모든 항목, 스피너, stale/미로그인/비-git 상태, Open PR, 포트, 새로고침), 카드 스냅샷 필드를 `rest` 채널에 추가. Covers R4, R6, R9, R10, AC4, AC6, AC8, AC9, AC12. Depends on: T2, T3, T5.
- T8. Changes 두 그룹: 기존 `ChangesReader`를 확장해 base 대비 커밋된 파일 목록과 파일별 줄 델타를 읽고, Changes 뷰를 UNCOMMITTED/COMMITTED ON BRANCH 그룹과 새 파일 행 구성으로 바꾼다. Covers R12, R10, AC13. Depends on: T1.
- T9. Remove worktree: 카드 버튼의 노출·비활성 규칙, 기존 확인창에 용량 표시, 삭제 후 카탈로그 갱신. Covers R5, AC5. Depends on: T7.
- T10. 검증 픽스처: 로그인된 `gh`로 비공개 저장소 `hide-e2e-fixture`를 만들고(이미 있으면 재사용), worktree 3개(PR 없음/open/merged), closed PR 1개, `-u` push 후 로컬 커밋 1개를 더한 브랜치 1개, push한 적 없는 브랜치 1개, 큰 하위 폴더 1개, 비-git 폴더 프로젝트 1개를 준비하는 스크립트를 둔다. 저장소는 검증 후 유지한다. Covers 9.2 V5, AC1-AC9. Depends on: none.
- T11. 자동 테스트: gh 출력 파싱·매핑·tie-break, worktree porcelain 파싱과 카운트, `git diff --stat` 파싱과 그룹 분리, 리더 주기·요청 동일성, 카드/행 표시 정책(배지·흐림·버튼 노출·비활성 이유)에 대한 단위 테스트. Covers AC3, AC10, R2, R5, R7, R12. Depends on: T1, T3, T5, T6, T8, T9.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust/Swift 빌드와 기존 계약 체크 스크립트 | none |
| automated behavior | yes | 파싱·매핑·리더 주기·표시 정책 회귀 | none |
| browser/runtime | yes | 설치된 앱에서의 사이드바·카드·삭제 흐름 | 최종 UX 판단 |
| live external API | yes | 픽스처 저장소에서 실제 gh 조회와 PR 생명주기 | 계정 소유 승인(4.1) |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R12 | Rust와 Swift 빌드, 기존 계약 체크 스크립트(shortcut, right panel sections)가 회귀하지 않는다 | yes | no |
| V2 | automated behavior | R2, R5, R7, R9, R11, R12, AC3, AC10 | gh 출력의 매핑과 tie-break(review 상태 포함), worktree porcelain 파싱, diff --stat 파싱과 두 그룹 분리, 리더가 같은 요청을 주기 안에 재실행하지 않음, 행·카드 표시 정책이 고정 입력으로 검증된다. 보호하는 회귀: 매핑표 이탈, 잠금 안 서브프로세스, 배지/버튼 규칙 붕괴 | yes | no |
| V3 | browser/runtime | R1-R6, R8, R10, R12, AC1, AC2, AC4-AC7, AC9, AC11, AC12, AC13, SC1, SC2, SC3 | 설치된 dev 인스턴스에서 픽스처 프로젝트를 펼쳐 행 상태, 카드 항목, Open PR, Remove worktree의 노출·비활성·삭제, pane 없는 worktree 시작, 비-git 카드, Explorer/Changes 공존, Changes 두 그룹과 파일 행 구성이 스크린샷과 상태 기록으로 증명된다. 각 SC의 primary·failure·recovery를 모두 거친다 | yes | no |
| V4 | browser/runtime | R7, R9, AC8, SC1 failure/recovery | gh가 PATH에 없는 상태, gh 로그아웃 상태, gh 실패 주입 상태 각각에서 카드와 배지가 규칙대로 보이고(없음/미로그인 안내, 로컬 기본 브랜치 대비 ahead/behind, stale 표시), 새로고침·agent 종료·주기 세 트리거가 각각 재조회를 일으키는 것이 로그로 남는다 | yes | no |

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked | Allowed Side Effect | Sensitive Data Policy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| V5 | live external API | R7, AC3, AC4, AC5, SC1, SC2 | 픽스처 저장소에서 PR을 열고(open), 머지하고(merged), 다른 PR을 닫아(closed) 실제 gh 조회 결과가 배지와 카드에 반영되며, upstream에 push한 뒤 로컬 커밋 1개를 더한 브랜치에서 `↑1`이, push한 적 없는 브랜치에서 항목 생략이 증명된다. review 상태는 두 번째 계정이 없어 V2의 고정 출력으로만 증명한다 | yes | yes: gh 미로그인이나 네트워크 불가 시 | 사용자 계정의 비공개 픽스처 저장소에 브랜치·PR·머지 생성, 저장소는 유지 | gh 토큰은 앱과 증거에 절대 기록하지 않음, 계정명 외 개인정보 없음 |

### 9.3 Human Verification

- 카드와 행의 시각 밀도: 텍스트 최소·기호 우선 규칙(D-27)이 실제로 읽기 쉬운지, 배지 색 세 가지(review 세부)가 구분되는지의 최종 판단.
- Remove worktree 비활성 이유 문구와 gh 안내 한 줄의 어조.
- 4.1의 계정 소유 승인(픽스처 저장소 생성).

## 10. Risks And Open Decisions

- gh 출력 형식 변경: `--json` 필드 기반이라 안정적이나, 필드 누락 시 실패로 표면화하고(빈 값으로 넘어가지 않음) 카드에 사유를 남긴다.
- PR이 200개를 넘는 저장소: 오래된 PR이 누락되어 `없음`으로 보일 수 있다. 문서화하고 보고 시 재검토(D-30).
- worktree가 많은 저장소: `git status`류 조회가 worktree 수만큼 늘어난다. 조회는 잠금 밖·주기 제한이지만, 매우 많은 worktree(10개 초과)의 비용은 D-20 유보 항목과 함께 재검토.
- 용량 측정이 큰 트리(`node_modules`, `target`)에서 수 초 걸린다. 선택 1개로 제한하고 측정 중 표시로 대응.
- 삭제는 파괴적이다. 기존 확인창과 비활성 규칙으로 막고, 브랜치는 건드리지 않는다.
- 원격(mini) 프로젝트는 카드가 없다. 원격 worktree 관리는 범위 밖이며 필요 시 별도 PRD.
- 열린 결정 없음. 유보: D-20(긴 트리).

## 11. Implementation Guardrails

- G1. 행과 카드에 문장을 넣지 않는다. 상태는 색·기호·숫자로, 예외 상태만 한 줄 (design 7, D-27). 의미를 가진 색과 기호에는 툴팁 또는 접근성 라벨을 붙인다 (design 7 역방향).
- G2. 기존 패턴을 따른다: 행은 `CheckoutNavigatorRow`, 배지는 `SidebarBadge`, 카드 항목의 필은 pane 헤더의 포트 필, 확인창은 기존 worktree 삭제창, 색·간격·반경은 `HideTheme` 토큰 (design 5, DESIGN.md). 새 토큰이 필요하면 `HideTheme`에 추가하고 말한다.
- G3. 앱은 GitHub 토큰을 읽거나 저장하지 않고 네트워크를 직접 호출하지 않는다. 모든 GitHub 접근은 `gh` 서브프로세스다 (D-04).
- G4. 서브프로세스(git, gh, du)는 런타임 뮤텍스 밖에서만 실행하고 결과만 전달한다. 틱마다 새 서브프로세스를 만들지 않으며 요청 동일성 + 주기 캐시를 쓴다 (CLAUDE.md Performance Guide, D-19).
- G5. 실패는 표면화한다: gh 실패, 파싱 실패, 측정 실패는 빈 값으로 덮지 않고 사유를 스냅샷에 싣고 구조화 로그(`kind`, 프로젝트 id, 사유)를 남긴다. "PR 0개"와 "조회 실패"는 구분된다 (engineering 4, 9, 10).
- G6. 파괴적 동작 앞에 결과를 말한다: Remove worktree 확인창은 경로·용량·"브랜치는 남는다"를 보이고, 비활성일 때 이유를 보인다 (design 6).
- G7. 삭제와 갱신은 두 번 실행돼도 안전해야 한다: 이미 없는 worktree 삭제는 실패 사유로 끝나고, 중복 갱신 요청은 합쳐진다 (engineering 11).
- G8. 범위를 넓히지 않는다: Commit/Create PR, worktree 생성, 브랜치 삭제, 별도 사이드바 뷰, 원격 프로젝트 카드, 비GitHub 호스팅을 추가하지 않는다. 새 서비스·스키마·잡·외부 호출도 승인 없이 추가하지 않는다.
- G9. 이 변경으로 필요 없어지는 코드(pane 기반 checkout 생성 분기 등)는 같은 변경에서 지운다 (engineering 1). 새 리더는 `ChangesReader` 패턴을 확장하지, 옆에 다른 형태를 만들지 않는다 (engineering 7).
- G10. 테스트는 호출자가 관찰하는 결과(매핑 결과, 스냅샷 필드)를 고정 입력으로 검사하며, 실제 gh나 네트워크에 의존하는 단위 테스트를 만들지 않는다 (engineering 12).
- G11. 증거는 `agents/runs/<slug>/` 아래에만 두고 커밋하지 않는다 (CLAUDE.md).

## 12. Implementation Result Report Contract

구현 agent는 다음을 보고한다:

- status: `Done`, `Partially Done`, `Blocked`.
- 사용자에게 보이는 변경(행, 카드, 삭제 흐름, 상태 표시).
- 변경된 주요 모듈·스냅샷 필드·이벤트(리더 3개, 카탈로그, 카드 채널, 트리거).
- 구현 중 선택한 파일/모듈 구조와 각 책임 경계, 5장의 구조를 따랐는지.
- T1-T11 완료 상태.
- R/AC/V 커버리지와 모드별 검증 증거(빌드, 자동 테스트, 앱 스크린샷, 픽스처 PR URL과 gh 조회 기록).
- 추가·수정한 자동 테스트와 각각이 막는 회귀.
- 픽스처 저장소 이름·URL·유지 여부.
- 편차, 남은 사람 검토(9.3), 미완료 항목과 후속 후보(D-20 긴 트리, 원격 카드).
- delivery: local, 커밋 목록.
