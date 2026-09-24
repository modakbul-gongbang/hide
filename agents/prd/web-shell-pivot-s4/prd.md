---
topic: "S4: 웹 Changes와 읽기 전용 diff"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "웹에서 Git diff 경로를 활성화하므로 삭제·rename·symlink 및 체크아웃 전환에서 등록된 프로젝트 경계와 읽기 전용 보장을 독립적으로 검증한다."
source_intake: "current conversation"
created_at: "2026-09-24"
updated_at: "2026-09-24"
---

# PRD: S4 Changes와 읽기 전용 diff

## Goal

S3의 파일 열기·편집·저장에 이어, 사용자가 웹 셸에서 현재 checkout의 미커밋 변경과 브랜치 변경을 골라 커밋 전 diff를 읽을 수 있게 한다.
기존 native History와 Rust Changes 모델을 웹으로 옮기는 완결된 슬라이스이며, 사용자가 요청한 단계별 구현·검증·PR·머지 반복의 첫 작업이다.
S3 PR #139와 아직 로컬에 남아 있는 S3 개정분의 후속 PR이 모두 병합된 결과를 기반으로 하며 새 Workspace 정보구조는 후속 S6에서 적용한다.

## Non-goals

- Git stage/unstage/discard/commit/push, fetch, merge 충돌 해결, 임의 revision 비교와 커밋 이력 브라우저는 추가하지 않는다.
  사용자는 기존 터미널에서 이 작업을 하며 별도 Git 작업 요청에서 재검토한다.
- 원격 checkout의 Git, 이미지·binary 전용 비교, 실제 내장 Browser는 제외한다.
  local-only와 Git의 텍스트 요약을 표시하고 원격/Browser 후속 계약에서 재검토한다.
- Main/Project Overview, Agents·Views 분리와 drag split, Project Memory/Sessions는 개정된 S6~S8의 범위이다.
  S4는 현재 S3의 tab/editor 구조를 사용하며 미래 기능의 가짜 버튼을 제공하지 않는다.
- Swift 삭제, 설치 앱 교체, 운영자 앱·pane·서버 조작과 실제 사용 전환의 대리 승인은 하지 않는다.
  Swift 삭제는 S10 별도 PRD 승인 뒤에만 가능하다.
- Git 상태를 웹에서 다시 계산하거나 별도 Git reader·문서 저장소·HTTP 파일 읽기 경계를 만들지 않는다.
  engineering 5·7·8에 따라 코어의 기존 읽기 모델과 preview 권위를 재사용한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 기존 Changes/diff 검토 범위를 S4 한 PR로 완결한다. S3 병합 결과를 먼저 반영하고 S5 이후 기능을 섞지 않는다. | 기존 웹 전환 우산 B11/D-07; 사용자 "방금 머지했거든? S3인가 뭔가?" 및 단계별 진행 요청 |
| D-02 | 가정: 화면 이름은 현 native의 History, wire identity는 changes를 유지한다. 오른쪽 Explorer/History 선택과 숨김/복원은 현재 core UI 상태를 쓰고 diff는 중앙 editor tab에서 연다. 후속 S6의 독립 Tools로 재배치할 수 있도록 역할만 분리한다. | native RightPanel.swift, DESIGN.md History/preview 계약; 새 구조를 S6까지 미루는 계획 |
| D-03 | 가정: CodeMirror 6 기반 읽기 전용 unified patch를 표시한다. 우산의 @codemirror/merge 채택은 실제 입력 API를 확인하여 적용하며, 전체 old/new 문서를 요구해 기존 patch wire에 맞지 않으면 같은 CodeMirror 6의 patch 렌더링을 사용하고 이유를 기록한다. 누락된 문맥을 가짜 전체 파일로 복원하거나 전체 파일을 추가로 읽는 방식은 기각한다. | 기존 우산 D-03; core ChangesDiff가 text patch만 제공; engineering 2·6·7 |
| D-04 | 가정: uncommitted는 index와 working tree를 합쳐 HEAD와 비교하고, committed는 기존 resolved base와 HEAD의 merge-base 비교를 사용한다. base 탐색은 기존 local 다음 origin 참조 순서이며 fetch하지 않는다. path와 committed group을 함께 identity로 쓴다. | herdr-core changes.rs와 model.rs의 현행 의미 |
| D-05 | 가정: file/diff 공통 preview 한 개, Keep Open, 고정·닫기·focus·재정렬은 기존 core를 승계한다. diff에는 autosave, draft 복원, 편집 도구를 붙이지 않는다. 원본 file tab과 working/branch diff tab은 별도 identity이다. | 기존 EditorTabKind::Diff, changes_select 및 file_* 이벤트; DESIGN.md preview 계약 |
| D-06 | 가정: core의 오류·notice·root identity를 그대로 분류한다. 다른 checkout의 남은 delta를 새 checkout 내용으로 보이지 않게 하고, diff null과 성공한 빈 diff를 구분한다. 별도 refresh 이벤트나 가짜 상태 시각을 만들지 않는다. | snapshot_delta.rs와 projects.rs; engineering 4·10, design 9·10·13 |
| D-07 | 기존 256 KiB UTF-8 patch 표시 상한과 잘림 notice를 유지하며 오래된 render 자원을 해제한다. 기존 2초 due refresh는 완료 SLA가 아니다. 렌더/스크롤/행 클릭에 새 Git polling을 추가하지 않는다. | changes.rs의 MAX_DIFF_BYTES/refresh 의미; engineering 14·15 |
| D-08 | 등록 checkout, 루프백·토큰·Origin, readonly 경계를 유지한다. deleted/rename 경로는 파일 존재 검사를 무작정 강화하여 막지 않되 checkout 밖이나 다른 group 데이터를 노출하지 않는다. 새 보안 정책·권한 확대는 별도 승인 사항이다. | S1/S3 경계, AGENTS.md; native Git 읽기 동작 승계 |
| D-09 | 가정: 현재 토큰/Seti/툴팁 관례를 쓰고 file/diff 종류를 아이콘과 접근성 이름으로 구별한다. 해당 표면의 Pen component/state와 한글·긴 경로·좁은 창을 함께 검증하되 새 전체 레이아웃은 만들지 않는다. | 사용자 "각 탭의 경우 에이전트면 에이전트 아이콘 + 파일,브라우저나 그런거면 좀 다르게"; 개정안 D-11/D-13 |
| D-10 | 별도 worktree의 새 Implementor가 gpt-6-sol high로 구현한다. 매 단계 현재 HEAD 검증·독립 Fidelity/Code 및 Security 리뷰·PR·필수 CI를 통과한 뒤 보호 규칙을 지킨 merge commit을 허용하고 다음 S로 진행한다. 이전 문서 개정 턴의 문서-only/자동 merge 미허용은 이번 실행 요청이 대체하며 S10 삭제는 제외한다. | 사용자 "이때 $implement 하고 sol-6 high 띄워서 하게 하고 ㅇㅇ  매번 S별로 새거 띄우고 ㅇㅇ" 및 "S끝나면 PR올리고 머지하고 다음꺼 하고 그렇게 반복해서"; agents/config.json delivery.mode=pr |
| D-11 | engineering/design 원칙 commit 654485f96b7764c759662d2c3e9e386ebc221cf6를 읽었다. engineering 3·5·7·8은 기존 reader 위 완결 슬라이스, 4·9·10은 잘못된 빈 성공 금지·안전한 실패, 12·14·15는 관찰 기반 회귀·자원 상한에 적용한다. design 1~5·7~13은 목록→diff 흐름·실제 상태·작은 실패 표식·기존 구조·한글 검증에 적용한다. 파괴적 작업 자체가 없어 design 6은 새 동작을 만들지 않고 기존 보호를 유지한다. | principles intake; docs/ARCHITECTURE.md 및 DESIGN.md |
| D-12 | S4 상세 인터뷰 파일은 없으므로 현재 대화와 기존 S4 범위를 source로 사용한다. 상세 parity 선택은 위 가정이며 새 완성 PRD의 human_approval은 pending으로 유지하고, 사용자의 원문 실행 위임으로 start한다. | current conversation; gen-prd conversational authorization 규칙 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 오른쪽에서 Explorer와 History를 선택하고 숨겼다 열면 선택한 section을 유지한다. 다른 UI 설정·파일 draft·현재 terminal focus를 단순 section 전환으로 지우지 않는다. | D-02, D-05 |
| B2 | History는 현재 local checkout의 UNCOMMITTED와 COMMITTED ON BRANCH 그룹을 접고 펼쳐 보여주며 처음에는 펼친다. 실제 파일 개수와 사용 가능한 base branch만 표시하고 빈 그룹은 숨긴다. base를 구할 수 없으면 브랜치 그룹을 만들지 않는다. | D-04, D-06 |
| B3 | 행은 core의 상대 경로 순서로 파일 아이콘·이름·부모 경로·상태와 가능한 추가/삭제 수를 보여준다. binary/미계산 수를 0으로 꾸미지 않으며 M/A/D/U/R/!를 색만이 아닌 접근 가능한 의미로 구별한다. | D-04, D-09 |
| B4 | 단일 클릭/키보드 Enter로 working 또는 branch diff를 preview tab에 연다. 이미 열렸으면 해당 tab을 선택하고, 같은 파일의 두 diff와 원본 파일 tab을 혼동하지 않는다. tooltip과 접근성 이름은 전체 identity를 보존한다. | D-04, D-05, D-09 |
| B5 | 다음 preview는 기존 공통 slot을 교체하지만 dirty file preview는 잃지 않는다. Keep Open, tab 제목 더블클릭 및 기존 reorder 고정 규칙이 동작하며 diff 닫기는 Git이나 원본 파일을 바꾸지 않는다. 기존처럼 diff 닫기는 Reopen Closed에 넣지 않는다. | D-05 |
| B6 | diff는 편집할 수 없으며 unified header/hunk/context/add/delete를 구별하고 old/new line number를 정확히 표시한다. header에는 line number가 없고 context는 양쪽, 추가는 new, 삭제는 old만 증가한다. 공백과 줄바꿈을 보존하고 가로·세로로 스크롤하며 짧은 diff는 위쪽부터 보인다. | D-03, D-09 |
| B7 | 원본 에디터와 오가도 diff에는 자동저장·draft·쓰기 요청이 발생하지 않는다. 복사와 editor text scale은 사용할 수 있고 다른 문서의 draft·충돌 상태를 변경하지 않는다. | D-05, D-09 |
| B8 | workspace 없음, local clean checkout, remote local-only, Git 실패를 서로 구별한다. 읽기 실패를 No changes로 표시하지 않고 조작 가능한 실패는 해당 영역에서 재선택/다시 열기로 복구하며 진단 세부를 화면 전체 경고로 노출하지 않는다. | D-06, D-08, D-11 |
| B9 | 현재 tab/path/group의 diff를 기다릴 때만 Reading the diff를 표시한다. 파일이 선택 group에서 사라지면 tab을 남겨 unavailable과 닫기/재선택 경로를 제공한다. 읽기 notice를 받으면 무한 spinner 대신 그 사유를 표시한다. | D-05, D-06 |
| B10 | 삭제 파일도 해당 group에서 읽고, rename은 목적 파일 identity와 이전→새 경로를 tooltip/접근성으로 보여준다. untracked는 전체 추가 내용을 읽으며 conflict는 !와 가능한 Git patch를 보여주되 해결 editor는 제공하지 않는다. | D-04, D-08 |
| B11 | binary는 Git의 텍스트 요약, 성공한 빈 patch는 빈 읽기 캔버스로 표시하고 오류로 바꾸지 않는다. 256 KiB를 넘는 patch는 UTF-8 문자를 깨뜨리지 않는 기존 잘림 결과와 notice를 보여주며 전체 파일을 임의로 다시 읽지 않는다. | D-03, D-06, D-07 |
| B12 | checkout 또는 tab/group을 빠르게 바꾸어도 이전 diff나 늦게 도착한 delta를 새 선택으로 보이지 않는다. WS 재연결 후 현재 core snapshot을 따라가며 보이지 않는 탭을 위해 무한 작업이나 Git reader를 추가하지 않는다. | D-04, D-06, D-07 |
| B13 | 등록 checkout 밖 경로와 이탈 symlink, root 전환·등록 해제의 오래된 요청은 기존 hided/core 경계에서 거절된다. 삭제된 유효 tracked 경로와 정상 rename은 읽을 수 있다. Git 내용은 실행 가능한 HTML이 아니라 텍스트로 표시한다. | D-08 |
| B14 | 키보드로 section·group·행·tab을 선택하고 현재 선택/접힘/focus를 인식할 수 있다. 한글·혼합 문자·긴 경로가 좁은 지원 폭에서 겹치지 않으며 diff가 최소 내용 폭보다 길면 scroll로 읽는다. 색·크기·간격은 기존 token 권위와 Pen state에 맞는다. | D-09, D-11 |
| B15 | 변경 목록과 큰 patch를 열고 스크롤하거나 terminal로 돌아가도 terminal 입력/IME를 삼키지 않고 기존 S0 echo/frame 기준을 유지한다. hidden/unmounted diff의 editor/listener가 정리되며 렌더 중 Git 실행이나 동기 filesystem 읽기가 없다. | D-07, D-11 |
| B16 | 격리 checkout에서 파일 수정→History 선택→diff 검토→원본 편집→갱신된 diff 검토를 실제 웹 UI로 끝낸다. owning guide가 비교 기준·읽기 전용 경계·실패·상한을 설명하고 S3 파일 편집/뷰어/첨부 흐름은 그대로 사용할 수 있다. | D-01, D-04, D-05, D-11 |

## Technical structure

기존 core Changes reader → changes/editor snapshot → hided token-gated WS → React/CodeMirror 경계를 유지한다.
core가 목록·선택·preview tab을 소유하고 웹은 표시용 focus/스크롤만 가지며 별도 Git subprocess나 파일 읽기 API를 만들지 않는다.
기존 WS event/snapshot을 우선 재사용하고 shell의 누락된 diff type과 렌더링을 확장한다.
경계의 실제 결함이 나오면 같은 승인된 읽기 범위에서 거절/경로 검사를 고치고 새 권한은 열지 않는다.
새 영속 상태 schema나 Swift UX 변경은 이 슬라이스에 필요하지 않다.

## Risks

- S3 PR #139 병합 HEAD는 5d25eeed2bda1ee8a49255baa049767c68e01f2d이지만, local S3 HEAD 98acd3561823c9b99aed19ee497049cd6a14ed6f의 후속 17개 커밋은 아직 미병합이다.
  자동저장·오른쪽 Explorer·draft 보호·외부 실행 차단을 포함한 이 개정분을 먼저 별도 PR로 검증·병합한 뒤 S4를 시작한다.
  root의 로컬 S3 문서 commit은 보존하고 새 작업 branch에서 최신 origin/main을 합친 뒤 구현한다.
- 코어 diff tab의 editor.document는 null이 정상이다.
  S3의 파일 loading·autosave 경로를 그대로 재사용하면 spinner 또는 쓰기 회귀가 생기므로 별도로 확인한다.
- 기존 reader의 256 KiB 제한은 Git stdout을 읽은 후 적용되며 Git subprocess timeout/전체 changed-file 개수 상한은 없다.
  이것을 이미 bounded라고 주장하지 않고 기존 소유자에서 재현 가능한 S4 위험은 해결하며 범위 밖 개선은 근거와 함께 후속 항목으로 남긴다.
- 독립 Security 검토는 삭제 경로·rename·symlink·다른 root·HTML 실행 및 WS 경계에 집중한다.
  차단할 보안 결함, 필수 검증 실패, 새 권한이나 요구 삭제 필요 시 자동 merge하지 않고 Observer에 반환한다.
- Rust/Swift sealed 검증 외에도 저장소 web typecheck/lint/unit/build/e2e와 design-contract를 실행하고 실제 Herdr Browser pane + chromux 관찰을 남긴다.
  스크린샷·로그는 local agents/runs 아래에만 두며 사용자 계정·실사용 repo 내용은 QA fixture로 쓰지 않는다.
- 이 문서의 inline self-check와 readiness를 통과한 뒤 시작한다.
  S4 전용 qa-log가 없는 conversation-only 계약이라 spec gate는 source 부재로 생략하고 구현 후 독립 전체 계약 리뷰는 생략하지 않는다.
- 사용자가 실제 앱을 전환했다는 판단과 S10 삭제 승인은 남는다.
  야간 자동화는 검증 가능한 단계만 진행하며 모든 S가 7시간 내 끝난다고 약속하지 않는다.
