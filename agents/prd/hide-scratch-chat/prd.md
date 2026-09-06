---
topic: "hide Scratch 공간과 ⌘N 채팅 composer: 프로젝트에 묶이지 않은 에이전트 탭을 메시지 한 줄로 시작"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "사용자에게 보이는 새 진입 흐름(⌘N composer)과 내비게이터 섹션을 추가하고 기존 New Agent 시트를 대체하지만, 외부 효과는 운영자 자신의 로컬 herdr 서버에 워크스페이스와 pane을 만들고 운영자가 이미 쓰는 에이전트 CLI를 시작하는 것뿐이며 데이터 삭제, 자격 증명, 결제, 파괴적 동작이 없다."
source_intake: "agents/interview/hide-scratch-chat/qa-log.md"
created_at: "2026-09-06"
updated_at: "2026-09-06"
---

# PRD: hide Scratch 공간과 ⌘N 채팅 composer

## 1. Summary

⌘N을 누르면 채팅 입력창이 뜨고, 메시지를 쳐서 ⌘↩하면 프로젝트가 아닌 상설 Scratch 공간에 새 탭이 생겨 그 에이전트가 그 메시지로 시작된다.
Scratch는 hide가 관리하는 고정 폴더 하나를 cwd로 쓰는 herdr 워크스페이스이며, 그 안에 에이전트 탭과 터미널 탭이 여러 개 공존하고, 닫기는 다른 pane과 같이 ⌘W다.
왼쪽 Projects 뷰에는 맨 위 New chat 행과, Needs You/Done 아래 기본 접힌 Scratch 섹션이 생기며, Scratch의 pane은 Projects 트리에 절대 섞이지 않는다.

지금 ⌘N은 Device / Agent / Workspace checkout / bypass 폼(New Agent 시트)을 열고 checkout을 고르지 않으면 시작할 수 없다.
프로젝트에 속하지 않은 질문 하나를 던지려면 폴더를 먼저 골라야 하고, 임시 터미널을 열어두고 싶어도 어떤 프로젝트에 넣을지부터 정해야 한다.
그리고 core는 등록되지 않은 폴더의 pane을 주황색 "temporary" 워크스페이스로 Projects 트리에 섞어 보여준다.

이 PRD 뒤에는 composer가 New Agent 시트를 완전히 대체하고, Where 칩에서 프로젝트를 고르는 것으로 기존 프로젝트 시작도 같은 창에서 하며, 프로젝트 모드도 첫 메시지가 필수다.

Approval checklist:

- Scratch = 고정 폴더 하나(`~/Library/Application Support/hide/scratch/`)를 cwd로 쓰는 상설 비프로젝트 공간이라는 개념과, 세션별 폴더·닫기 시 삭제·고아 정리를 기각한 결정 (section 3, 4.3 D-05/D-11/D-17).
- composer가 기존 New Agent 시트를 완전히 대체하고 프로젝트 모드도 첫 메시지 필수가 되는 범위 (R2, R6, section 4.3 D-12).
- Scratch를 core의 별도 내비게이터 노드로 투영하고 temporary 워크스페이스 fallback에서 제외하는 구조 변경, ui_state에 마지막 에이전트·bypass 선택을 영속하는 변경 (section 5).
- 에이전트 탭 제목을 herdr pane 메타데이터 토큰에 적어 재시작을 넘기는 결정 (R5, section 5).
- 검증 모드: build/static, automated behavior, app runtime, 그리고 격리된 herdr 서버에서 실제 에이전트 세션을 정확히 하나 시작해 첫 메시지 전달을 증명하는 live herdr integration (section 9.1, 4.3 D-24).
- 배포 모드: pr. `agents/config.json`대로 worktree에서 구현하고 `main`을 base로 PR을 열어 CI를 지켜본다 (section 4.3).
- `review_profile: standard`와 그 근거 (frontmatter).

## 2. Problem, Goal, And Users

사용자는 hide의 단일 운영자다.
여러 프로젝트에서 코딩 에이전트를 돌리지만, 하루에도 여러 번 "이 에러 뭔 뜻이야", "이 문구 다듬어줘"처럼 어떤 프로젝트에도 속하지 않는 질문을 던지고 싶어 하며, 프로젝트에 속하지 않는 임시 터미널도 자주 열어둔다.

지금의 문제는 셋이다.

- ⌘N은 폼이다. Device, Agent, Workspace checkout, bypass를 채우는 시트가 뜨고 checkout이 없으면 Start가 비활성이다. 바로 물어보는 흐름이 아니다.
- 프로젝트 밖의 공간이 없다. 임시 터미널이나 임시 대화는 어떤 프로젝트 폴더를 빌려 열어야 하고, 그 폴더에 파일이 생기면 프로젝트가 더러워진다.
- 프로젝트 밖의 pane은 내비게이터에서 이상하게 보인다. core는 등록되지 않은 폴더의 pane을 폴더 이름을 단 주황색 "temporary" 워크스페이스로 Projects 트리에 섞어 넣는다.

목표는 운영자가 ⌘N, 메시지, ⌘↩ 세 동작으로 프로젝트에 속하지 않은 에이전트 대화를 시작하고, 열어둔 Scratch 탭들을 왼쳽 패널의 한 섹션에서 오가며, 닫기와 재시작이 다른 pane과 똑같이 동작하는 것이다.

### 2.1 User Scenarios

- SC1. ⌘N으로 Scratch에 에이전트 탭 시작: 운영자가 어느 뷰에서든 ⌘N을 누르거나 Projects 뷰 맨 위 New chat 행을 누른다.
  Actors: 운영자.
  Primary path: composer 시트가 열리고 커서가 입력창에 있다. Where 칩은 Scratch, Agent 칩은 마지막에 쓴 에이전트, bypass는 마지막 선택이다. 질문을 치고 ⌘↩하면 Send가 진행 표시로 바뀌고 입력이 잠긴다. Scratch 워크스페이스가 없으면 만들고 있으면 그 안에 새 탭을 만들어 에이전트를 시작하고, 준비되면 첫 메시지를 보내고 제목을 pane 메타데이터에 적는다. 시트가 닫히고 새 pane에 포커스가 가며 에이전트가 답하기 시작한다. Scratch 헤더 개수가 1 늘어난다.
  Failure state: 입력이 비면 Send 비활성. 선택한 에이전트 CLI가 PATH에 없으면 Agent 칩 아래 경고와 설치 링크가 보이고 Send 비활성. 진행 중 ⌘N/⌘↩ 재입력은 무시되고 취소 버튼은 없다. 탭 생성 뒤 에이전트 시작이나 첫 메시지 전달이 실패하면(준비 대기 30초 초과 포함) 만든 탭은 셸 프롬프트 상태의 터미널 탭으로 Scratch에 남고 시트는 닫히며 알림 한 줄로 사유를 보인다. 탭 생성 자체가 실패하면 만든 것이 없고 알림만 보인다. Run on이 원격 디바이스면 제출 시 기존 원격 거부 안내가 나오고 아무것도 만들지 않는다. bypass가 켜져 있으면 칩 옆에 기존 경고 문구가 보인다.
  Recovery: 실패한 터미널 탭에서 직접 CLI를 치거나 ⌘W로 닫고 다시 ⌘N. Esc는 시트를 닫고 입력을 버린다.
  Reach: 격리된 herdr 서버(별도 소켓)에 붙은 dev 번들에서 ⌘N을 누른다. 실패 경로는 준비 대기 실패를 주입한 자동 테스트와, PATH에 없는 에이전트를 골라 본 런타임 캡처로 도달한다.

- SC2. 프로젝트에서 에이전트 시작(기존 시트 대체): 운영자가 composer의 Where 칩에서 프로젝트 checkout을 고르거나, 내비게이터의 Start agent here 또는 워크스페이스 행의 새 에이전트 버튼을 누른다.
  Actors: 운영자.
  Primary path: 진입점에서 온 경우 Where 칩에 그 checkout이 미리 선택돼 있다. 메시지를 치고 ⌘↩하면 그 checkout의 live workspace에 탭(없으면 workspace)이 생기고 에이전트가 첫 메시지를 받는다. 새 에이전트는 Projects 트리의 그 checkout 아래에 보이고 Scratch 섹션에는 나타나지 않는다.
  Failure state: 메시지 없이는 시작할 수 없다(Send 비활성). 존재하지 않는(missing) checkout은 고를 수 없다. 실패 처리는 SC1과 같다.
  Recovery: 메시지 없는 시작을 원하면 그 프로젝트 탭에서 ⌘T로 터미널을 열고 직접 CLI를 실행한다.
  Reach: 격리된 herdr 서버에 등록된 프로젝트 checkout 하나에서 Start agent here를 누른다. 제출 파이프라인은 Scratch와 같으므로 checkout cwd와 워크스페이스 id가 인자에 들어가는 것은 자동 테스트로, 실제 에이전트 시작은 D-24가 승인한 Scratch의 세션 하나로 증명한다.

- SC3. Scratch 섹션에서 탭 오가기, 터미널 열기, 닫기: 운영자가 Projects 뷰의 Scratch 헤더, 행, ⌘T, ⌘W를 쓴다.
  Actors: 운영자.
  Primary path: Scratch는 기본 접힘이고 헤더에 개수가 보인다(0이어도 헤더는 보인다). 펼치면 탭마다 한 줄: 에이전트 탭은 에이전트 아이콘, 첫 메시지 제목, 상태를, 터미널 탭은 기존 탭 라벨을 보인다. 행을 누르면 그 pane에 포커스가 간다. Scratch가 포커스된 채 ⌘T를 누르면 scratch 폴더를 cwd로 하는 터미널 탭이 Scratch에 생긴다. ⌘W는 다른 pane과 같이 닫고 아무것도 지우지 않는다. 펼침 상태와 제목은 hide 재시작 뒤에도 유지된다.
  Failure state: Scratch 에이전트가 입력을 기다리면 Needs You 섹션에도 올라오고, 그때 Projects 트리에는 중복되지 않는다. scratch 폴더 cwd의 pane은 Projects 트리에도 temporary 워크스페이스로도 나타나지 않는다.
  Recovery: 닫은 탭은 다시 ⌘N이나 ⌘T로 만든다. 파일은 scratch 폴더에 그대로 남아 있다.
  Reach: 격리된 herdr 서버에 에이전트 탭 둘과 터미널 탭 하나를 Scratch에 만든 뒤 셸을 재시작한다.

## 3. Scope And Non-Goals

범위: Scratch 공간의 core 투영과 고정 폴더, ⌘N composer 시트(칩 세 개 + 입력창 + Send), 제출 흐름(탭 생성, 에이전트 시작, 첫 메시지, 제목 기록)과 그 실패 경로, 기존 New Agent 시트의 삭제, Projects 뷰의 New chat 행과 Scratch 섹션, Scratch에서의 ⌘T, 마지막 에이전트와 bypass 선택의 영속, 메뉴와 도움말의 New Agent 문구를 New Chat으로 바꾸기.

비목표, 각각 의도된 제외:

- 세션별 폴더와 닫기 시 폴더 삭제, 고아 폴더 정리 (qa-log D-05 초기안, D-11, D-17 기각).
  Consequence: 에이전트가 scratch 폴더에 만든 파일은 사용자가 지우기 전까지 남는다.
  Rationale: 사용자가 "hide는 터미널 기반이라 닫는다는 개념이 애매하고, temp라 언제든 사라져도 된다"고 판단했다.
  Revisit: scratch 폴더가 실제로 불편할 만큼 커진다고 사용자가 말할 때.
- ⌘N 즉시 생성(모달 없음) (D-06 대안 B 기각).
  Consequence: 항상 ⌘↩ 한 번이 더 든다.
  Revisit: 없음. 사용자가 명시적으로 창 한 번 뜨는 방식을 골랐다.
- 빈 입력으로 시작 (D-08 추천안 기각).
  Consequence: 메시지 없는 에이전트 시작은 ⌘T 터미널에서 직접 CLI를 치는 것으로만 가능하다.
  Revisit: 사용자가 요청할 때.
- 원격 디바이스에서의 에이전트 시작 (D-14).
  Consequence: Run on이 원격이면 기존과 같은 거부 안내가 나온다.
  Revisit: 원격 시작 PRD.
- 참고 이미지(Create worktree)의 세로 폼 레이아웃, Codex 앱의 Pinned와 Recents(끝난 대화 기록) (D-07, Q2).
  Consequence: 끝난 Scratch 대화는 기록으로 남지 않는다.
  Revisit: 사용자가 대화 기록을 원할 때.
- Scratch 탭 또는 에이전트의 개수 제한, 자원·비용·디스크 안내 (D-22).
  Consequence: herdr와 macOS가 감당하는 만큼 열 수 있고, 실패하면 알림 한 줄만 보인다.
  Revisit: 자원 고갈이 실제로 관찰될 때.
- 진행 중 취소, 실패 시 입력 메시지 보존, "시작 실패" 전용 상태 행 (D-16).
  Consequence: 실패하면 메시지를 다시 친다.
  Revisit: 사용자가 요청할 때.
- 여러 줄 입력 편집기 기능(마크다운 미리보기, 첨부). 입력창은 여러 줄 plain text다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
필요한 herdr 워크스페이스, 탭, pane, 메타데이터 토큰은 격리된 herdr 서버에서 herdr CLI로 만들 수 있고, 에이전트 CLI(claude)는 이 Mac에 이미 설치되어 있다.

### 4.2 Human Decisions Before PRD Approval

None required.
유일한 하드스톱급 결정(V4의 실제 에이전트 세션과 극소량 토큰 지출)은 2026-09-06 사용자가 "ㅇㅇㅇ"으로 승인했고 4.3에 기록했다. 나머지 결정은 qa-log의 사용자 결정이거나 4.3에 가정으로 표시된 되돌릴 수 있는 선택이다.

### 4.3 Decision Traceability For Fidelity Review

qa-log `agents/interview/hide-scratch-chat/qa-log.md`의 Decision Register를 이 PRD에 이렇게 옮겼다.

- D-01 (fact, ⌘N과 기존 시트 구성, 클레임된 단축키): R1, R6, section 5. ⌘N은 그대로 composer에 쓰고 다른 단축키는 건드리지 않는다.
- D-02 (fact, workspace/tab create 뒤 agent start 두 단계, cwd 임의): R3, section 5. 기존 launcher를 확장한다.
- D-03 (fact, 미등록 폴더 자동 발견과 temporary 플래그): R4, section 5. scratch 폴더는 이 경로에서 제외된다.
- D-04 (fact, DESIGN.md에 시트 지침 없음, 토큰 안에서 재해석): section 11, 9.3.
- D-05 (decision, Q1 -> Q15 재정의: 고정 폴더 하나): R4, AC4, 비목표. 세션별 폴더는 기각.
- D-06 (decision, 창 한 번 뜸, ⌘↩ 제출): R1, AC1. 즉시 생성은 비목표.
- D-07 (decision, 칩 세 개 + 입력창 composer, Where 기본 Scratch, 열리자마자 포커스): R1, R2, AC1, AC2, SC1. 세로 폼은 기각.
- D-08 (decision, 빈 입력 Send 비활성, 제출 시 agent start 뒤 agent prompt): R2, R3, AC2, AC3. 빈 채로 시작은 기각.
- D-09 (decision, Scratch 안에 여러 탭, 제출마다 새 탭, ⌘T는 터미널 탭): R3, R7, AC7, SC3. 단일 고정 공간은 기각.
- D-10 (decision, New chat 행, Scratch 섹션 위치·행 모양·기본 접힘·개수·기억·Projects 비노출·Needs You 승격): R5, R6, AC5, AC6, SC3.
- D-11 (rejected, 닫기 시 폴더 삭제와 확인): 비목표.
- D-12 (decision, 기존 시트 완전 대체, 프로젝트 모드도 메시지 필수, Start agent here는 composer): R6, AC8, SC2.
- D-13 (fact, agent prompt 존재, agent start readiness 기본 30초): R3, section 5, section 10.
- D-14 (fact, 원격 시작 거부 유지): R3, AC3, 비목표.
- D-15 (decision, bypass는 Agent 칩 메뉴 안, 마지막 선택 기억, 켜져 있으면 경고): R1, R8, AC9. 기존 시트의 "매번 초기화" 규칙은 사용자 결정으로 대체.
- D-16 (decision, 대기 중 잠김·취소 없음·재입력 무시, 실패 시 탭은 터미널로 남고 알림 한 줄, 원격은 거부): R3, AC3, SC1.
- D-17 (rejected, 고아 폴더 정리): 비목표.
- D-18 (decision, 제목은 첫 메시지 첫 줄 40자, pane 메타데이터에 영속, 터미널 탭은 기존 라벨): R5, AC5, AC6.
- D-19 (decision, 시트로 열림, Esc는 입력 버림, 마지막 에이전트 기억): R1, R8, AC1, AC9.
- D-20 (assumption, 검증 모드): section 9. 고아 정리 테스트는 제거된 상태로 반영.
- D-21 (decision, 목표 한 문장): section 1.
- D-22 (decision, 개수 제한 없음): 비목표.
- D-23 (fact, 재수화는 live pane에서, 별도 세션 파일 없음): R4, R5, AC6, section 5.
- D-24 (decision, 실제 에이전트 세션 정확히 하나, 최소 첫 메시지, 극소량 지출 승인, 캡처는 첫 메시지와 답만, 프로젝트 모드는 에이전트 시작 없이 인자와 cwd로 증명): V4, AC10, 9.2 부작용 경계, 9.3.
- 사용자가 기각한 추천안(기록 유지): 빈 입력으로 시작(Q8), 세션별 폴더(Q15에서 재정의), 닫기 시 삭제와 고아 정리(Q15), 매번 초기화되는 bypass(Q16 "bypass는 기본 켜지는거 옵션으로").
- 사용자 승인 (2026-09-06, 질문 "live 검증에서 격리된 herdr 서버에 실제 Claude 세션 하나를 띄우고 'reply with exactly ok' 수준의 최소 첫 메시지를 보내도 될까요?"): "ㅇㅇㅇ". V4는 실제 에이전트 세션을 시작하고 첫 메시지 전달을 자동 증거로 남긴다. 같은 답으로 `/please` 인수 해석(아래)에도 이의가 없었다.
- 사용자의 `/please` 인수 해석 (agent 가정): "worktree 파서 작업 쭉 진행"은 현재 대화의 기능을 worktree에서 끝까지 진행하라는 강조로 읽었다. 별도의 worktree 파서 기능은 대화에 없다. 이 해석이 틀리면 사용자가 거부한다.
- Agent 가정 (사용자 결정 아님): 제목 토큰의 이름과 40자 자르기 단위는 구현이 herdr 토큰 값 상한(80자) 안에서 정한다. 한글 40자가 상한을 넘으면 바이트가 아니라 문자 단위로 더 짧게 자르되 AC5의 "첫 줄 앞부분"이라는 관찰은 유지된다.
- Agent 가정 (사용자 결정 아님): Scratch 섹션의 접힘 상태는 기존 `collapsed_workspace_ids` 패턴을 재사용해 영속한다. 새 저장 파일은 만들지 않는다.
- Agent 가정 (사용자 결정 아님): 마지막 에이전트와 bypass 선택은 ui_state의 revisioned rest 섹션에 두 필드로 영속한다. 제출 때만 바뀌므로 Performance Guide의 rest 재전송 비용은 발생하지 않는다.
- Agent 가정 (사용자 결정 아님): composer의 첫 메시지는 `herdr agent prompt`로 보내고 `--wait`는 쓰지 않는다. 답이 오기까지 시트를 잡아두지 않기 위해서다. 전달 실패는 명령의 종료 상태로 판정한다.
- Agent 가정 (사용자 결정 아님): 메뉴 항목과 툴바 도움말의 "New Agent"는 "New Chat"으로 바뀌고 단축키 ⌘N은 그대로다.
- 배포 모드: `agents/config.json`의 `delivery.mode: pr`, `baseBranch: main`, `branchPrefix: gen-prd`, `worktree.enabled: true`, CI watch를 그대로 따른다. PR 배포의 근거는 이 config이며, 사용자의 인수 "worktree ... 쭉 진행"은 worktree에서 진행한다는 것만 말한다. 사용자가 PR 배포를 원하지 않으면 거부할 수 있다.
- 원칙 인테이크: `~/projects/oh-my-principle` 커밋 `35ab76ca23d45e714f1630054855a8c8c4568d03`에서 `engineering/principles.md`와 `design/principles.md`를 전문으로 읽었다. 적용 규칙은 section 11에 번역했다. engineering 규칙 6은 Codex 앱 사이드바를 설계 참조로 채택한 것(qa-log Q2)으로 충족되어 별도 guardrail로 두지 않았다. design 규칙 1은 Scratch 행이 기존 에이전트 행 컴포넌트를 재사용하는 것으로 충족되고, 규칙 6은 파괴적 동작이 없어 번역하지 않았다.
- 프로젝트 규칙 인테이크: `AGENTS.md`의 "Performance Guide", "Evidence Belongs Outside The Repository", "Design Reference", "Herdr API Contract"를 section 11에 번역했다. `rules relevant`는 이 PRD가 건드리는 경로에 걸린 인버리언트가 없다고 답했다.

## 5. Major Technical Structure Changes

- Scratch가 core 내비게이터의 별도 노드가 된다.
  core는 scratch 폴더 경로를 한 곳에서 정하고 snapshot에 싣는다.
  세션 동기화는 cwd가 그 폴더 안인 herdr 워크스페이스와 pane을 Projects 목록이 아니라 Scratch 노드로 투영하며, 미등록 폴더를 temporary 워크스페이스로 만드는 fallback은 그 경로에 적용되지 않는다.
  Scratch 노드는 탭 목록(각 탭의 pane과 에이전트)과 펼침 상태를 담고, Projects 개수와 트리에서 제외된다.
  Needs You/Done 승격과 Agents 뷰는 에이전트 단위이므로 바뀌지 않는다.
- 에이전트 시작 파이프라인이 두 단계에서 네 단계로 늘어난다.
  탭 생성(workspace create 또는 tab create, cwd는 Scratch 폴더 또는 checkout) -> agent start(준비 대기) -> agent prompt(첫 메시지) -> pane report-metadata(제목 토큰).
  각 단계의 실패는 그 단계 이름과 사유를 담은 결과로 셸에 돌아오고, 탭 생성 뒤의 실패는 탭을 남긴 채 알림으로 표면화된다.
  기존 launcher와 인자 빌더를 확장하며 새 프로세스 실행 경로를 만들지 않는다.
- New Agent 시트와 그 draft 모델이 composer로 대체되고 삭제된다.
  composer의 상태는 Where(Scratch 또는 checkout id), 디바이스, 에이전트 종류, bypass, 입력 텍스트, 진행 여부다.
- ui_state에 마지막 에이전트 종류와 bypass 선택이 영속된다(revisioned rest 섹션, 기본값 로드, 스키마 버전 유지).
  Scratch 섹션의 접힘은 기존 `collapsed_workspace_ids`에 Scratch 노드 id로 들어간다.
- 에이전트 탭의 제목은 herdr pane 메타데이터 토큰이 진실이다.
  core는 agent.list의 토큰에서 제목을 읽어 Scratch 행에 싣고, hide는 별도 세션 파일을 두지 않는다.
- 스키마 버전, herdr 계약(`contracts/herdr-api.schema.json`), 인증, 결제, 배포 변경 없음. 새 서드파티 의존성 없음.

## 6. Requirements

- R1. ⌘N, 메뉴의 New Chat, Projects 뷰 맨 위의 New chat 행은 composer 시트를 연다.
  시트는 위에 한 줄로 Where 칩, Run on 칩, Agent 칩을 붙이고 아래에 여러 줄 텍스트 입력, 우하단에 Send(⌘↩)를 둔다.
  열리자마자 키보드 포커스는 입력창에 있다.
  Where 칩의 기본값은 진입점이 프로젝트 문맥(Start agent here, 워크스페이스 행의 새 에이전트 버튼)이 아닌 한 항상 Scratch이며, 펼치면 존재하는 프로젝트 checkout 목록이 나온다.
  Agent 칩은 마지막에 쓴 에이전트 종류를 보이고, 그 메뉴 안에 bypass 토글이 있으며 토글이 켜져 있으면 칩 옆에 기존 경고 문구가 보인다.
  Esc는 시트를 닫고 입력을 버린다.
- R2. Send는 입력이 공백만이거나 선택한 에이전트 CLI가 PATH에 없을 때 비활성이다.
  CLI가 없으면 Agent 칩 아래에 경고와 설치 링크가 보인다.
  제출 뒤 결과가 돌아오기까지 Send는 진행 표시로 바뀌고 입력은 잠기며, 그 사이 ⌘N과 ⌘↩은 무시된다. 취소 버튼은 없다.
- R3. 제출은 Where에 따라 Scratch 폴더 또는 checkout을 cwd로 탭을 만들고(해당 herdr 워크스페이스가 없으면 workspace create, 있으면 tab create), 에이전트를 시작하고, 준비되면 첫 메시지를 그대로 보내고, 제목 토큰을 그 pane에 적는다.
  Scratch 폴더가 없으면 첫 제출 때 만든다.
  성공하면 시트가 닫히고 새 pane에 포커스가 간다.
  탭 생성 뒤 어느 단계가 실패하면 탭은 셸 프롬프트 상태의 터미널 탭으로 남고 시트는 닫히며 실패 단계와 사유가 알림 한 줄로 보인다. 탭 생성 자체가 실패하면 알림만 보인다.
  Run on이 원격 디바이스면 아무것도 만들지 않고 기존 원격 거부 안내를 보인다.
  단계별 결과는 구조화된 진단(단계 이름, 대상 pane id, 성공 여부)으로 남고 메시지 내용은 담지 않는다.
- R4. core는 scratch 폴더 경로를 한 곳에서 정해 snapshot에 싣고, cwd가 그 폴더 안인 herdr 워크스페이스와 pane을 Scratch 노드로 투영한다.
  그 pane은 Projects 목록, Projects 개수, temporary 워크스페이스 fallback 어디에도 나타나지 않는다.
  Scratch 노드는 herdr가 살려둔 pane에서 hide 재시작 뒤에도 다시 만들어진다.
- R5. Scratch 노드의 각 탭은 한 행이다.
  에이전트가 있는 탭은 기존 에이전트 행 컴포넌트로 에이전트 아이콘, 제목, 상태를 보이고 제목은 첫 메시지 첫 줄의 앞 40자 이내다.
  제목은 herdr pane 메타데이터 토큰에서 읽으며 토큰이 없으면 기존 탭 라벨을 보인다.
  에이전트가 없는 탭은 기존 탭 라벨을 보인다.
  행을 누르면 그 pane에 포커스가 간다.
- R6. Projects 뷰는 맨 위에 New chat 행, 그 아래 기존 Needs You/Done 섹션, 그 아래 Scratch 섹션, 그 아래 Projects 섹션 순서다.
  Scratch 섹션은 헤더에 개수를 보이고 개수가 0이어도 헤더는 보이며, 기본은 접힘이고 헤더를 누르면 펼쳐지며 그 상태는 재시작 뒤에도 유지된다.
  Needs You나 Done에 든 Scratch 에이전트는 Scratch 섹션에 중복되지 않는다(기존 Projects 트리와 같은 규칙).
  기존 New Agent 시트, 그 draft 모델, "New Agent" 문구는 삭제되고 메뉴와 툴바는 New Chat을 말한다.
- R7. Scratch 노드가 포커스된 상태의 ⌘T는 scratch 폴더를 cwd로 하는 터미널 탭을 Scratch에 만든다.
  ⌘W와 pane 닫기는 Scratch 탭에서도 다른 pane과 같이 동작하고 폴더나 파일을 지우지 않는다.
- R8. 마지막에 쓴 에이전트 종류와 bypass 선택은 ui_state에 영속되어 hide 재시작 뒤 composer의 기본값이 된다.
  bypass는 켜진 채 제출하면 기존과 같은 provider별 bypass 플래그가 agent start 인자에 붙는다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | ⌘N 또는 New chat 행으로 열린 시트에서 키보드 포커스가 입력창에 있고, Where 칩이 Scratch, Agent 칩이 마지막 에이전트를 보이며, 칩 세 줄이 한 줄에 붙어 있고 입력창이 그 아래에 있다 | judged | dev 번들에서 ⌘N 직후 캡처 한 장과, 문자를 치자 입력창에 나타나는 두 번째 캡처 |
| AC2 | 입력이 비었거나 공백만일 때 Send가 비활성이고, 한 글자라도 들어가면 활성이며, PATH에 없는 에이전트를 고르면 경고와 설치 링크가 보이고 Send가 비활성이고, 제출이 진행 중인 동안은 Send가 진행 상태이고 입력이 잠기며 추가 제출 요청은 무시된다 | machine | - |
| AC3 | 제출 인자 순서가 탭 생성 -> agent start -> agent prompt(첫 메시지 그대로) -> report-metadata(제목 토큰)이고, agent start나 prompt 실패를 주입하면 만든 pane은 남고 실패 단계와 사유가 알림 문자열로 돌아오며, 원격 디바이스 선택 제출은 어떤 명령도 실행하지 않고 원격 거부 안내를 돌려준다 | machine | - |
| AC4 | cwd가 scratch 폴더 안인 herdr 워크스페이스와 pane은 snapshot의 Scratch 노드에만 나타나고 Projects 목록과 개수에 없으며 temporary 워크스페이스로 만들어지지 않고, 같은 pane 집합에서 snapshot을 두 번 만들어도 같은 결과다 | machine | - |
| AC5 | 제목 토큰이 있는 에이전트 pane의 Scratch 행 제목은 토큰 값이고, 토큰이 없는 pane과 에이전트 없는 탭은 탭 라벨이며, 첫 메시지 첫 줄에서 만든 제목은 40자를 넘지 않고 원문의 접두어다 | machine | - |
| AC6 | Scratch 섹션이 기본 접힘으로 개수를 보이고, 펼친 뒤 hide를 재시작해도 펼침과 각 행의 제목이 같으며, Needs You에 오른 Scratch 에이전트는 Scratch 섹션에 중복되지 않고, Projects 트리 어디에도 scratch 경로의 행이 없다 | judged | 격리된 herdr 서버에 에이전트 탭 둘과 터미널 탭 하나를 만든 뒤 접힘 상태, 펼친 상태, 재시작 뒤 상태의 사이드바 캡처 셋과 Projects 트리 캡처 |
| AC7 | Scratch가 포커스된 채 ⌘T로 만든 탭의 cwd가 scratch 폴더이고 그 탭이 Scratch 섹션에 나타나며, Scratch 탭을 ⌘W로 닫은 뒤 scratch 폴더와 그 안의 파일이 그대로 있다 | judged | 격리된 herdr 서버에서 ⌘T 뒤 herdr pane 정보의 cwd, 파일 하나를 만든 탭을 닫기 전후의 폴더 목록 |
| AC8 | Start agent here로 연 시트는 그 checkout이 Where에 선택된 채 열리고, 그 상태의 제출은 Scratch와 같은 단계 순서로 checkout cwd와 그 checkout의 herdr 워크스페이스 id를 인자에 실으며 그 pane은 Scratch 섹션에 나타나지 않고, 코드베이스에 기존 New Agent 시트와 그 draft 타입이 남아 있지 않다 | judged | 격리된 herdr 서버의 등록 checkout에서 Start agent here 뒤 시트 캡처, 프로젝트 모드 제출 인자(cwd, 워크스페이스 id, 단계 순서)를 단언한 자동 테스트 결과, 정적 검색 결과 |
| AC9 | bypass를 켜고 제출하면 agent start 인자에 claude와 codex 각각의 bypass 플래그가 붙고, 끄면 두 provider 모두 붙지 않으며, 켠 상태와 마지막 에이전트 종류는 ui_state 저장 뒤 다시 로드해도 같다 | machine | - |
| AC10 | 격리된 herdr 서버에서 composer로 Scratch 에이전트 탭을 만들면 pane의 cwd가 scratch 폴더이고, 에이전트가 첫 메시지를 받아 답했으며, pane 메타데이터에 제목 토큰이 있고, 이 실행이 만든 워크스페이스·탭·pane 밖의 것은 건드리지 않았다 | judged | 격리 소켓의 herdr 서버에서 제출 뒤 pane 정보(cwd, 토큰)와 pane 출력의 첫 메시지·응답 부분 캡처, 실행 전후의 pane 목록 비교 |

## 8. PRD-Level Tasks

- T1. core에 scratch 폴더 경로와 Scratch 노드 투영을 두고, cwd가 그 폴더 안인 워크스페이스와 pane을 temporary fallback과 Projects 목록에서 빼며, 제목 토큰을 읽어 행에 싣고, ui_state에 마지막 에이전트와 bypass 필드를 더한다. Covers R4, R5, R8, AC4, AC5, AC9. Depends on: none.
- T2. 에이전트 시작 파이프라인을 네 단계로 확장한다: cwd(Scratch 또는 checkout) 결정과 폴더 생성, agent start 뒤 agent prompt, 제목 토큰 기록, 단계별 실패 결과와 진단, 원격 거부 유지. Covers R3, R8, AC3, AC9. Depends on: T1.
- T3. composer 시트를 만들어 ⌘N, 메뉴, New chat 행, Start agent here, 워크스페이스 행 버튼이 그것을 열게 하고, 기존 New Agent 시트와 draft 모델과 "New Agent" 문구를 삭제한다. Covers R1, R2, R6, AC1, AC2, AC8, SC1, SC2. Depends on: T2.
- T4. Projects 뷰에 New chat 행과 Scratch 섹션(기본 접힘, 개수, 영속, 행 클릭 포커스, Needs You 중복 제외)을 넣고, Scratch 포커스 상태의 ⌘T가 scratch 폴더에 터미널 탭을 만들게 한다. Covers R5, R6, R7, AC6, AC7, SC3. Depends on: T1.
- T5. 검증 픽스처를 준비한다: 격리 소켓의 herdr 서버와 그에 붙는 dev 번들 실행 절차, Scratch에 에이전트 탭 둘과 터미널 탭 하나를 만드는 스크립트, 재시작 전후 비교 절차, 실행이 만든 것만 닫는 정리 절차. Covers SC1, SC2, SC3, AC10. Depends on: none.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust core와 Swift 셸의 빌드와 기존 테스트, 기존 시트 타입 부재 | none |
| automated behavior | yes | Scratch 투영, 제목 규칙, 제출 인자와 실패 경로, Send 활성 규칙, ui_state 영속의 회귀 | none |
| app runtime | yes | composer의 포커스와 레이아웃, Scratch 섹션의 접힘·펼침·재시작, Projects 비노출, ⌘T와 ⌘W | 최종 시각 판단 |
| live herdr integration | yes | 격리된 herdr 서버에서 실제 제출 한 번(에이전트 세션 하나)으로 cwd, 첫 메시지, 제목 토큰이 맞는 흐름 | D-24로 승인됨 |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R8, AC8 | Rust core와 Swift 셸이 깨끗이 빌드되고 기존 테스트가 통과하며, 셸 소스에 기존 New Agent 시트와 draft 타입이 남아 있지 않은 것이 정적으로 확인된다 | yes | no |
| V2 | automated behavior | R2, R3, R4, R5, R8, AC2, AC3, AC4, AC5, AC8, AC9, SC2 | 회귀 위험을 직접 겨냥한 테스트가 있다: scratch 경로의 pane이 다시 temporary 워크스페이스나 Projects 트리로 새는 것, 제목이 40자를 넘거나 토큰 없는 pane이 빈 제목을 보이는 것, 제출 인자 순서가 바뀌거나 실패가 조용히 성공으로 보이는 것, 원격 선택이 명령을 실행하는 것, 빈 입력에 Send가 켜지는 것, 제출 진행 중에 Send가 다시 활성되거나 두 번째 제출이 실행되는 것, bypass가 켜졌는데 claude 또는 codex의 플래그가 빠지거나 꺼졌는데 붙는 것, 프로젝트 모드 제출에 checkout cwd나 워크스페이스 id가 빠지는 것, 재시작 뒤 마지막 에이전트와 bypass가 기본값으로 돌아가는 것. 각 테스트는 snapshot 또는 셸 정책 함수의 출력을 단언한다 | yes | no |
| V3 | app runtime | R1, R6, R7, AC1, AC6, AC7, AC8, SC2, SC3 | 격리된 herdr 서버에 붙은 dev 번들에서 ⌘N 직후 입력창에 포커스가 있고 칩이 한 줄에 붙어 있으며, Start agent here로 연 시트에 그 checkout이 선택돼 있고, Scratch 섹션이 기본 접힘으로 개수를 보이고 펼침과 제목이 재시작을 넘기며, Projects 트리에 scratch 행이 없고, ⌘T가 scratch 폴더에 터미널 탭을 만들고, ⌘W가 파일을 지우지 않는다 | yes | no |
| V4 | live herdr integration | R3, R4, R5, AC10, SC1 | 격리된 herdr 서버에서 composer 제출 한 번으로 Scratch pane의 cwd가 scratch 폴더이고, 에이전트가 첫 메시지를 받아 답했으며, 제목 토큰이 pane 메타데이터에 있고, 이 실행이 만든 것만 건드렸다 | yes | no |

Live 모드의 부작용 경계:

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked | Allowed Side Effect | Sensitive Data Policy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| V4 | live herdr integration | R3, R4, R5, AC10, SC1 | 위와 같음 | yes | no | 별도 소켓(HERDR_SOCKET_PATH)과 별도 HOME으로 띄운 격리 herdr 서버에만 워크스페이스·탭·pane을 만들고, 그 서버에서 실제 에이전트 세션을 정확히 하나 시작해 "reply with exactly ok" 수준의 최소 메시지 하나를 보내며(D-24), 끝나면 그 서버와 그것이 만든 것을 모두 닫는다. 운영자의 기본 소켓 herdr 서버와 실행 중인 hide 인스턴스에는 어떤 명령도 보내지 않는다 | 캡처는 첫 메시지와 그 답 이상을 담지 않고 홈 경로와 호스트명을 가린다. 캡처는 gitignore된 `agents/runs/hide-scratch-chat/` 아래에만 두어 이 Mac의 운영자만 볼 수 있고 커밋되지 않으며, 운영자가 언제든 지울 수 있다 |

### 9.3 Human Verification

- composer의 칩 밀도, 입력창 크기, Send의 진행 표시가 `DESIGN.md` 토큰 안에서 자연스럽고 "바로 타이핑하는 창"으로 읽히는지 판단한다 (D-04, D-07).
- Scratch 섹션의 행이 프로젝트 트리의 행과 같은 언어로 읽히는지, 제목 40자가 사이드바 폭에서 잘리는 방식이 받아들일 만한지 판단한다.

## 10. Risks And Open Decisions

- herdr 토큰 값 상한은 80자다. 한글 40자가 인코딩에 따라 상한을 넘으면 구현이 문자 단위로 더 짧게 자른다(4.3 가정). T1에서 실제 바이너리로 상한의 단위를 확인해 결과를 보고한다.
- `agent start`의 준비 대기(기본 30초)는 에이전트 CLI의 시작 화면(신뢰 확인, 업데이트 안내)에 걸릴 수 있다. 이 경우 실패 경로(R3)로 탭이 터미널로 남고 사용자가 그 pane에서 직접 처리한다. 첫 메시지가 시작 화면에 먹히는 위험은 `agent start`의 readiness 성공 뒤에만 prompt를 보내는 순서로 줄인다.
- Scratch 노드의 id와 herdr 워크스페이스 id는 다르다. `tab.create --workspace`에는 herdr id를 넘겨야 하며 기존 도메인 경계(`HerdrLiveWorkspaceIdentity`)가 그 혼동을 막는다.
- 운영자가 이미 scratch 폴더 cwd로 pane을 열어둔 herdr 워크스페이스가 있다면 그것이 Scratch 노드로 흡수된다. 의도된 동작이다.
- live 검증은 운영자의 Mac에서 돈다. 격리 소켓과 별도 HOME이 완화책이고 section 11이 금지로 적는다. 과거 사고(2026-09-06): HOME만 바꾸고 소켓을 바꾸지 않은 e2e가 운영자의 live herdr에 붙었다.
- 실제 에이전트 세션 하나는 운영자 계정의 토큰을 극소량 쓴다 (D-24). 프로젝트 모드의 에이전트 시작은 같은 파이프라인이므로 별도 세션으로 증명하지 않는다.
- 스크린샷과 로그는 `agents/runs/hide-scratch-chat/` 아래에만 두며 커밋하지 않는다.

## 11. Implementation Guardrails

운영자의 지침과 이 저장소의 규칙에서:

- 이 실행이 만들지 않은 herdr pane, 탭, 워크스페이스를 닫거나 옮기거나 프롬프트를 보내지 않는다. live 검증은 `HERDR_SOCKET_PATH`와 별도 HOME으로 격리한 서버에서만 하고, 운영자의 기본 소켓 서버와 실행 중인 hide 인스턴스와 상호작용하지 않는다.
- section 6을 넘어 범위를 넓히지 않고, section 5를 넘어 구조를 바꾸지 않으며, 서드파티 의존성을 추가하지 않는다. 숨겨진 사용자 흐름을 추가하지 않는다.
- herdr 호출은 `AGENTS.md` "Herdr API Contract"를 따른다: 공식 CLI 참조와 소켓 API 문서를 읽고, `herdr api schema --json`과 `contracts/herdr-api.schema.json`으로 확인한 메서드와 필드만 쓴다. 메서드 이름이나 플래그를 기존 call site만 보고 추측하지 않는다.
- 성능은 `AGENTS.md` "Performance Guide"를 따른다: Scratch 투영과 제목 읽기는 lock 안에서 subprocess나 블로킹 I/O를 부르지 않고, 폴더 생성과 herdr CLI 실행은 기존 launcher처럼 lock 밖 detached 작업에서 하며, ui_state의 새 필드는 제출 때만 바뀐다.
- 증거는 `AGENTS.md` "Evidence Belongs Outside The Repository"를 따른다: 모든 캡처와 로그는 `agents/runs/hide-scratch-chat/`에 두고 커밋하지 않는다.
- 디자인은 `AGENTS.md` "Design Reference"와 `DESIGN.md`를 따른다: 칩, 입력창, 진행 표시의 색·radius·간격은 `HideTheme` 토큰에서 오고 새 값은 거기에 추가해 쓴다. 시트에 그림자를 두지 않고 hairline 보더를 쓴다.
- engineering/principles.md 규칙 1: New Agent 시트, draft 모델, "New Agent" 문구, temporary fallback의 scratch 경로 분기를 같은 변경에서 지우고 호환 경로를 남기지 않는다.
- engineering/principles.md 규칙 2: Scratch는 노드 하나와 폴더 하나다. 다중 scratch 공간이나 일반화된 "비프로젝트 공간" 추상을 만들지 않는다.
- engineering/principles.md 규칙 3: core 투영(T1)이 먼저, 파이프라인(T2)이 그 위에, composer(T3)와 섹션(T4)이 그 위에 얹힌다.
- engineering/principles.md 규칙 4, 10: 제출의 어느 단계 실패도 조용히 성공으로 보이지 않고 알림과 진단으로 드러난다. 폴더 생성 실패도 실패다.
- engineering/principles.md 규칙 5: 경로와 투영은 core에, 프로세스 실행은 기존 launcher에, 그리기는 셸의 composer와 행 컴포넌트에 머문다.
- engineering/principles.md 규칙 7: Scratch 행은 기존 에이전트 행 컴포넌트를, 접힘은 기존 `collapsed_workspace_ids`를, 영속은 기존 ui_state 저장 경로를, 원격 거부는 기존 안내를 재사용한다.
- engineering/principles.md 규칙 8: 제목의 진실은 herdr pane 메타데이터다. 메모리나 임시 파일에 두고 나중에 옮기지 않는다.
- engineering/principles.md 규칙 9: 제출 단계 진단은 단계 이름, pane id, 성공 여부만 담고 메시지 내용은 담지 않는다.
- engineering/principles.md 규칙 11: 같은 Scratch 워크스페이스에 두 번 제출해도 워크스페이스는 하나이고 탭만 늘어난다. 폴더 생성은 이미 있으면 성공이다.
- engineering/principles.md 규칙 12, 13: 테스트는 snapshot과 정책 함수의 출력을 단언하고, "scratch 경로가 새는 부류"를 경로 판정 한 곳으로 없앤다.
- design/principles.md 규칙 2, 3: 가장 잦은 동작(⌘N, 메시지, ⌘↩)이 세 동작이고 칩은 기본값이 채워져 있어 건드릴 필요가 없다.
- design/principles.md 규칙 4: Send 활성 여부, 진행 상태, Scratch 개수는 계산되어 보이고 사용자가 판단하지 않는다.
- design/principles.md 규칙 5: 섹션 라벨, 행, 배지, 시트 헤더는 사이드바와 기존 시트의 패턴을 쓴다.
- design/principles.md 규칙 7: 상태는 칩과 진행 표시와 아이콘으로 먼저 부호화하고, 설명 문장은 CLI 없음 경고와 실패 알림 한 줄에만 둔다.
- Git과 PR 귀속: 브랜치 이름, 커밋 메시지, 트레일러, PR 본문 어디에도 에이전트, 모델, 벤더, 도구 이름을 쓰지 않는다.

## 12. Implementation Result Report Contract

보고 항목:

- status: `Done`, `Partially Done`, `Blocked`.
- 사용자에게 보이는 변화를 세 문제(폼인 ⌘N, 프로젝트 밖 공간 부재, temporary 워크스페이스 노출) 각각에 대해.
- 바뀐 모듈과 새 모듈의 책임 경계, 실제로 고른 파일 구조.
- section 5의 구조를 따랐는지, 벗어난 곳과 이유.
- T1부터 T5까지의 완료 상태와 R/AC/V 커버리지.
- 모드별 검증 증거와 각 산출물이 있는 실행 디렉터리.
- T1에 대해: herdr 토큰 값 상한의 단위(문자/바이트)를 실제 바이너리에서 관찰한 결과와 최종 자르기 규칙.
- V4에 대해: 격리 서버의 소켓 경로, 이 실행이 만든 워크스페이스·탭·pane과 그 전부가 닫혔다는 확인, 운영자의 기본 서버에 어떤 명령도 보내지 않았다는 확인.
- 추가되거나 바뀐 자동 테스트와 각각이 막는 회귀.
- 삭제된 타입과 코드 경로의 목록(New Agent 시트, draft 모델, 문구).
- 배포 증거: 브랜치, PR URL, CI 상태, 재시도나 블록 상태.
- 이탈, 남은 인간 검토, 미완 항목과 후속 후보.
