---
topic: "hide-design-refactoring"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "기존 macOS 작업 UI와 읽기용 Git·계보 projection을 바꾸며, 파일 조작·권한·배포 경계를 확대하지 않고 로컬 격리 검증과 전달만 수행한다."
source_intake: "current conversation"
created_at: "2026-09-14"
updated_at: "2026-09-14"
---

# PRD: Hide Design Refactoring

## Goal

Hide 사용자가 자신이 책임지는 작업과 위임한 작업을 구분하고, compact한 전체 작업 화면에서 부모·자식 이동, 검사, 실제 입력 focus와 파일 Git 상태를 혼동 없이 사용할 수 있도록 최종 UI 인계 디자인을 실제 native 앱에 구현한다.
기준은 `5d364f0`의 `design/agent-workflow-review.md`와 `design/hide.pen`의 `Review / UI Handoff /`이며, 아래 최신 Agents·Overview 결정이 그 인계의 미결정 부분을 대체한다.

## Non-goals

- Project 전체 화면 재설계, New Agent/Quick Workspace 개편, Git 탭 제거, Sessions 종료 이력·retention은 제외한다; 기존 진입점과 live Sessions identity만 유지하며 별도 사용자 요청 때 재검토한다.
- 상태 네 그룹과 demand/activity/read/ownership, stall threshold를 새 정책으로 교체하지 않는다; 이번 변경은 ownership 기반 목록 범위이며 delegated 완료의 독립 Done 승격은 계속 금지한다.
- Herdr 또는 실행기 저장소·설치·API를 변경하지 않는다; 부모 정보를 보내지 않는 외부 생성 경로의 자동 분류는 해결됐다고 주장하지 않으며 해당 API와 adapter가 authoritative lineage를 전달할 수 있을 때 별도 연동으로 재검토한다.
- 운영자 앱·Pane·서버의 종료/이동, 설치 앱 덮어쓰기, push/PR/merge/배포는 하지 않는다; native QA에 운영자 앱 전환이 필요하면 구체적인 후보 빌드와 격리 준비를 끝내고 조율한다.
- 새 provider/네트워크 서비스, 별도 상태 저장소, 화면별 중복 control system, row별 Git 조회를 추가하지 않는다; engineering 원칙 2·5·7에 따라 기존 owner를 확장하고 마지막 소비자가 사라진 구현만 함께 제거한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 디자인 전용 단계에서 실제 Hide UI 구현으로 전환하고 최종 인계본 전체를 기준으로 삼는다; 오래된 Final/R2/R3를 복원하지 않는다. | 사용자: “이 UI와 히스토리 내용 바탕으로 꼼꼼하게 확인해서 그대로 Hide Design Refactoring 공사 해보자.”; 이전 “기존 Final을 정답으로 전제하지 마세요.” |
| D-02 | Agents는 내 작업 기본 + 전체 전환으로 구성하며 두 보기 모두 기존 상태 그룹을 유지한다. | 사용자 응답: “추천안: 내 작업 기본 + 전체 전환” |
| D-03 | 내 작업은 core가 최종 파생한 ownership != Delegated를 사용한다; hard escalation과 orphan의 사용자 복귀를 보존한다. | 기존 `docs/status-model.md`, `runtime_lineage_tests`; 가정: ‘대표 작업’은 depth 0 필터가 아니라 현재 사용자 책임을 뜻한다. |
| D-04 | 위임 관계는 authoritative parent identity로만 판단하고 생성 도구·이름·환경·Pane 근접성으로 추측하지 않는다. | 사용자: “그건 에이전트가 감시하는거지 내가 보는건 아닌데”; engineering 원칙 13; 현재 실행기는 parentLineage unavailable이고 pinned API에는 role env와 parent를 동시에 전달하는 생성 경로가 없다. |
| D-05 | 우측 Overview는 현재 Project 전체 task forest와 짧은 inspector를 보여준다; 현재 family만 보는 B안은 기본에서 제외한다. | 사용자 응답: “A안: 현재 Project 전체 작업”; “선택 항목을 살펴보는 것과 실제 Pane을 여는 동작은 분리합니다”라는 질문에 대한 선택. |
| D-06 | Projects/Agents, 소속과 위임 구분, compact 공통 slots와 surface별 variant, 고정 열과 이어지는 계층선을 채택한다. | 초기 범위 A와 최종 인계 12-15/18; 과한 여백·부유한 관계선·중복 요약 거부. |
| D-07 | Pane 첫 줄 28pt, 알려진 자식이 있을 때만 24pt 둘째 줄; 부모 복귀/자식 관계는 아이콘, zoom/restore는 우측 항상 표시한다. | 사용자: “zoom in/out도 기본으로 우측에 같이 붙어있으면 좋겠어”, “타이포강가 아니라 icon 같은걸로”; 최종 인계 10/11. |
| D-08 | shown Pane, 실제 terminal keyboard focus, inspection selection을 분리하며 unread weight를 관계 역할로 쓰지 않는다. | 사용자: “자식이 선택된것과 부모가 선택된게 명확하게 focus 나 그런게 구분”; 최종 인계 01/02/04/10. |
| D-09 | delegated child는 별도 탭을 열고 부모 복귀는 기존 authoritative 배치를 선택한다; 관계 모달은 검사 후 명시적으로 연다. | 초기 범위 B와 인계 06/17; `docs/ARCHITECTURE.md` child-tab 계약. |
| D-10 | terminal/agent/browser/file/diff의 owner와 기능을 보존하면서 헤더를 정리한다; capability 없는 agent controls를 공통으로 얹지 않는다. | 초기 기능 inventory 요청, 인계 11/16; 기존 file/diff는 중앙 editor tab이다. |
| D-11 | Explorer는 기존 22pt row, Seti 아이콘과 파일 조작을 유지하며 Git status slot과 정확한 데이터·갱신 경계를 추가한다. | 사용자: “File Explorer git 상태 추가하는거 정도까진 같이 남겨서 마무리”; 인계 03/19의 M/A/U/R/!, 실패·remote·dirty 계약. |
| D-12 | 필요한 tokens/control variants를 기존 shared owner에 추가하고 Pen master/ref와 채택된 Screen을 함께 갱신한다. | 초기 “재사용 master + 실제 ref 상태 변형”; repo AGENTS.md, DESIGN.md; 스크린샷은 구현 증거를 대신하지 않는다. |
| D-13 | 실제 native 화면과 관련 회귀·성능 경계를 검증하며 정적 통과를 미적 승인으로 보지 않는다. | 사용자: “검증 꼼꼼하게 하고!”; 초기 240/320/344/400pt 실측·render 검수와 단일 인스턴스 규칙. |
| D-14 | 격리 worktree에서 구현하고 로컬 commit까지만 전달한다; 프로젝트 config의 PR 기본값보다 대화의 push/PR 금지를 우선한다. | 이전 “본인 변경만 커밋. PR/push/제품 코드/설치/실행 변경은 금지”; 최신 요청은 제품 코드·격리 검증을 허용하지만 외부 전달을 요청하지 않음. 가정: 외부 전달 제한 유지. |
| D-15 | 디자인·엔지니어링 원칙 전체를 적용한다; 미정 구조는 D-02/D-05 사용자 선택으로 해소했다. | `oh-my-principle` commit `653c46267c79892316ab7e8ff91f3a9a7d1561fc`의 `design/principles.md`, `engineering/principles.md` 전문 확인. UI 관찰 규칙은 아래 행동, 구현 제한은 Non-goals/Technical structure에 반영; 결제·배포 등 해당하지 않는 예시는 새 범위로 추가하지 않는다. |
| D-16 | Agents 범위 선택은 기존 sidebar mode와 독립적으로 현재 앱 세션 동안 유지하고 재실행 기본값은 내 작업이다; 전체 탐색의 Search/Recent와 Project task forest는 이 필터로 숨기지 않는다. | 가정: 추가 영구 저장 schema 없이 선택 연속성을 보존하고 기본 인지부하를 줄인다; 현재 MRU·검색 정렬 보존 요청. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 좌측 Projects/Agents, 중앙 탭과 다중 Pane, 우측 Overview/Explorer/Changes/Git를 포함한 작업 화면이 최종 인계 구성과 밀도로 동작하고 기존 entrypoint가 사라지지 않는다. | D-01, D-06, D-10 |
| B2 | Agents를 처음 열면 내 작업이며 전체로 전환할 수 있다; 전환은 Pane/tab/read를 바꾸지 않고 현재 세션에서 유지된다. | D-02, D-16 |
| B3 | 내 작업은 최종 delegated 항목만 제외하고 operator와 escalated 항목은 표시한다; 전체에서는 모든 canonical agent를 기존 Needs You/Done/Working/Seen 순서로 보여준다. | D-02, D-03 |
| B4 | 위임 작업의 완료는 독립 Done이 되지 않으며 기존 hard stall 시 root와 대상 child의 Needs You 복귀, 부모가 사라진 orphan의 visible 복귀를 유지한다. | D-03, D-04 |
| B5 | Agents 필터·검색·퇴장 후 번호와 키보드 대상은 실제 표시 목록과 일치한다; 중복 ID·번호·count가 없고 숨긴 항목으로 선택이 이동하지 않는다. | D-02, D-06 |
| B6 | 내 작업이 비었지만 전체에 위임 작업이 있으면 필터된 빈 상태와 전체 전환을 제공한다; 정말 작업이 없는 상태, loading, 연결 단절은 구분한다. | D-02, D-15 |
| B7 | 부모 행의 canonical 상태 다음 보조 슬롯에 알려진 direct child total/working 요약과 관계 진입이 있다; 자식 0은 요약 없음, 1은 +0 없음, partial은 전체라고 주장하지 않고 단절 시 live 진행 수를 숨긴다. | D-06, D-09 |
| B8 | provider artwork·상태·title·위치 identity는 Search/Recent/live Sessions/관계/Overview에서 같은 ID와 canonical 상태를 사용하며 검색 순서·MRU·일반 surface identity를 보존한다. | D-06, D-10, D-16 |
| B9 | Project > Workspace 소속과 부모 > 자식 위임이 구분되고 cross-Workspace 자식만 필요한 위치를 보조 표시한다; 같은 agent를 두 진입점에서 접근해도 중복 집계하지 않는다. | D-06 |
| B10 | status 12pt/provider 16pt/title 12pt/보조 11pt의 고정 slots, 16pt disclosure와 18pt agent depth가 root/child/grandchild·여러 형제·마지막·접힘에서 정렬된다; 실제 row 첫 줄 anchor에 분기하고 마지막 elbow에서 선이 끝난다. | D-06, D-12 |
| B11 | sidebar 240/320/344/400pt에서 긴 한글·혼합 문장·비분리 영문은 최대 두 줄 후 tail truncation하고 고정 열·조상 rail이 깨지지 않는다; compact Search/Recent/Overview는 한 줄이며 full title/path는 동일 tooltip/accessibility help에 남는다. | D-06, D-13 |
| B12 | Workspace populated 본문은 펼침/접힘, 별도 24pt target은 Workspace 열기다; 펼침은 read/focus를 바꾸지 않으며 접힌 요약은 펼쳤을 때 숨기고 0/1/다수/missing/disconnected를 구분한다. | D-06 |
| B13 | tree Up/Down은 표시 항목 이동, Left/Right는 펼침/접힘과 부모·자식 이동이고 활성화는 해당 row action을 따른다; hover/keyboard focus/selection/read/unread가 구분되며 terminal이 responder일 때 tree 키가 입력을 가로채지 않는다. | D-06, D-08 |
| B14 | tab strip은 배치/문서 탐색, Pane header는 현재 identity/actions를 담당하며 같은 title/status/breadcrumb/Workspace를 양쪽에서 반복하지 않는다; stable tab name이 비어 있는 기존 저장 상태에는 결정적인 fallback을 사용하고 기존 사용자 이름은 보존한다. | D-07, D-10 |
| B15 | 자식 0인 root는 28pt 한 줄, 자식 0인 child는 같은 한 줄 안의 부모 복귀 아이콘으로 관계를 드러낸다; parent는 24pt child 줄, child이면서 parent는 상하 관계를 함께 표시한다. | D-07 |
| B16 | child 줄은 첫 direct child의 이름과 남은 +N, 관계 진입을 표시하며 +N은 실제 숨긴 수다; in-process subagent와 Pane child 수를 합치지 않는다. | D-07, D-09 |
| B17 | 240pt에서도 parent return/provider/status/zoom/overflow/close를 유지하며 title만 말줄임한다; 부모 이름은 480pt 미만 또는 현재 title 공간 부족 시 먼저 숨기고 resize/hover가 action 위치나 footprint를 흔들지 않는다. | D-07, D-13 |
| B18 | 우측 24pt zoom/restore, overflow, close 순서를 유지한다; zoom은 Pane 확대/기존 배치 복원이며 글자 크기를 바꾸지 않고 Herdr의 실제 geometry 응답을 따른다. | D-07, D-09 |
| B19 | 알려진 child가 없는 detected 미계측 agent는 첫 줄 안내 아이콘만 표시한다; unknown/partial/disconnected/미계측/confirmed zero를 구분하고 plain terminal/browser/file/diff에는 무의미한 agent unknown 영역이 없다. | D-07, D-10 |
| B20 | 일반 terminal/browser/file/diff는 자신의 title과 controls를 유지하고 agent 전용 controls가 누출되지 않는다; split/fork/ports/ancestor/sibling은 실제 존재하는 capability만 overflow에서 접근하며 tooltip과 키보드 접근이 가능하다. | D-10 |
| B21 | header 제목·배경은 Pane focus를 선택하고 controls는 자기 intent만 수행한다; selected header wash는 보여주는 Pane, 바깥 primary hairline은 실제 terminal responder를 나타내며 Overview 검사 시 wash는 남고 terminal outline은 꺼진다. | D-08 |
| B22 | unread/상태 색은 focus 또는 부모·자식 역할로 바뀌지 않는다; header Tab/Enter/Space와 tooltip/accessibility help가 동작하고 terminal 한국어 IME·화살표·일반 입력은 기존 responder로 전달된다. | D-08, D-13 |
| B23 | child 이름을 열면 기존 child 전용 탭으로 이동하고 부모 복귀는 현재 authoritative 부모 배치를 선택한다; operator Pane에 split하거나 배치를 재생성하지 않는다. | D-09 |
| B24 | 관계 모달과 노드 선택은 검사만 하며 Open만 실제 이동한다; Esc는 invocation control로 돌아가고 pending은 중복 실행을 막으며 실패·unavailable parent는 기존 배치를 유지하고 이유/재시도를 표시한다. | D-09 |
| B25 | Overview 작업 보기는 현재 Project의 전체 live task forest를 Workspace를 넘어 표시한다; root와 타 Workspace child에 위치를 표시하고 Git ancestry와 agent delegation을 별도 보기로 분리한다. | D-05 |
| B26 | Overview 행/Return은 inspection selection, Pane 아이콘은 중앙에서 보여주는 ID, outline은 row keyboard focus다; 명시적 Open/Return agent 또는 Workspace 열기만 Pane/tab/read를 바꾸며 현재 Pane 찾기와 검색은 검사로 남는다. | D-05, D-08 |
| B27 | Overview inspector는 선택 identity/status/Workspace/Open/Return/Workspace 상세를 제공한다; 기존 GitHub·disk·cleanup 진입점을 보존하고 Workspace 변경 파일 수를 agent 개인 성과로 표시하지 않는다. | D-05 |
| B28 | Overview는 loading/확인된 0/검색 없음/partial/remote unavailable/disconnected를 구분한다; 검색은 확인 가능한 조상 경로를 유지하고 no-results는 query clear와 기존 inspector를 유지하며 퇴장·이동 후 stale Open을 실행하지 않는다. | D-04, D-05 |
| B29 | 부모 정보 누락/순환/retired relation은 확인 가능한 관계만 표시하고 종료·독립 작업의 확정 증거로 꾸미지 않는다; 현재 core orphan 정책대로 visible 유지하며 모든 Herdr 생성 작업의 감독 관계를 알아냈다고 주장하지 않는다. | D-03, D-04 |
| B30 | Explorer는 기존 22pt 밀도와 Seti 아이콘·disclosure를 유지하고 오른쪽 12pt Git 문자 슬롯을 예약한다; clean에서도 filename 끝 열이 흔들리지 않고 240/320/344/400pt에서는 파일명만 truncation한다. | D-11 |
| B31 | M/Modified, A/Added, U/Untracked, R/Renamed, !/Conflict는 실제 정규화 데이터와 semantic color+문자로 표시한다; porcelain conflict U와 화면 Untracked U를 혼동하지 않으며 selected/hover/focus에도 판독된다. | D-11 |
| B32 | 실제 rename은 현재 경로로 Explorer에서 열리고 Changes/diff는 이전→현재 경로를 사용한다; 공백·한글 경로와 NUL rename record를 정확히 처리하고 conflict를 Modified로 축약하지 않는다. | D-11 |
| B33 | 폴더 변경 표시는 고유 descendant changed paths 전체에서 파생하며 lazy로 펼친 자식만으로 clean을 판정하지 않는다; 삭제 파일은 Changes의 D로 남고 Explorer에 가짜 row를 만들지 않으며 editor unsaved dirty는 Git M과 별개다. | D-11 |
| B34 | Explorer만 보여도 Git 상태가 갱신되고 Workspace 전환 후 다른 root의 badge를 재사용하지 않는다; Git loading/failure/stale는 파일 목록을 지우거나 clean으로 위장하지 않으며 기존 refresh로 재시도한다. | D-11 |
| B35 | non-Git와 unsupported remote는 Git 상태를 합성하지 않는다; 목록 자체의 loading/empty/read failure와 Git 조회 상태는 별도로 보이며 파일명·경로·상태 풀네임은 tooltip/accessibility help로 제공한다. | D-11 |
| B36 | Explorer 클릭/Enter/disclosure/inline rename/drag/context menu/Copy Path/Reveal/삭제 confirmation과 복구가 유지되고 Git badge는 별도 클릭을 가로채지 않는다; file editing·find/wrap·Markdown mode·diff·browser 주소/reload는 기존 의미를 유지한다. | D-10, D-11 |
| B37 | hover/focus/scroll/반복 키 입력으로 Git·disk·network I/O나 무변화 snapshot publication이 생기지 않는다; task forest·필터는 stable ID projection을 재사용하며 갱신 요청과 pending 작업은 기존 bounded reader 경계 안에 있다. | D-11, D-15 |
| B38 | 최종 native 화면에서 좁은 폭, 긴 한글·영문, 부모→자식→부모·zoom 복원·검사와 입력 focus·read를 실제 관찰할 수 있다; fixture 표본과 실제 provider/계측/연결 증거의 범위를 명확히 구분한다. | D-13 |
| B39 | 재사용 master와 ref 상태 시트, 채택된 전체 Screen, 최신 owner 문서가 실제 구현과 함께 갱신되어 다음 작업자가 오래된 Review 후보를 재조합할 필요가 없다; 이전 기록은 Git history에 남고 dangling ref가 없다. | D-01, D-12 |

## Technical structure

Herdr의 pane/geometry/PTY·lifecycle ownership과 core의 navigation·canonical 상태 authority를 유지하며 Swift는 typed event와 snapshot을 사용한다.
Agents 범위와 표시/번호 projection은 같은 canonical visibility 집합을 사용하고 status derivation 자체는 교체하지 않는다; inspection은 실제 focus/read와 독립적인 기존 core 선택 모델을 확장한다.
Project task forest는 stable agent/parent/Workspace IDs와 detected/instrumentation/availability를 전달하는 live projection이며 기존 Git history와 분리한다; private 환경이나 pane display metadata를 새로운 ownership authority로 사용하지 않는다.
Explorer는 기존 background Changes reader를 확장해 rename source/destination과 conflict를 정규화하고 Explorer visibility를 request에 포함한다; NUL status parser와 diff 경계까지 갱신하며 blocking Git I/O는 runtime lock 밖에서 수행한다.
변경 결과는 해당 root/generation에만 적용하고 지연 응답은 다른 Workspace를 오염시키지 않는다; 각 입력당 계산·notification fan-out·pending bound를 기존 성능 owner에 명시한다.
새 외부 서비스나 별도 persistence schema는 도입하지 않으며 native component/tooltip/theme owner와 기존 state migration/fallback 경계를 재사용한다.

## Risks

- 외부 생성 계약 한계: 현재 Sasu는 role env를 원자적으로 넣기 위해 split 후 start하며 parentLineage unavailable을 반환한다; pinned AgentNew에는 parent만 있고 env가 없으며 AgentStart에는 parent가 없어 Hide만으로 supervisor 관계를 복원할 수 없다.
  이 경로의 worker는 계속 visible이고, 이번 완료는 authoritative lineage가 있는 위임 작업 분리까지다; 외부 Herdr API·Sasu adapter 변경은 후속 연동 범위이며 heuristic 숨김으로 대신하지 않는다.
- native QA는 정확한 bundle/PID/revision/core hash를 식별한 Hide 한 개와 private socket/config/state/workspace, remote 차단으로 수행한다; operator 앱이 실행 중이면 격리 준비 후 QA 전환을 조율하며 승인 없이 종료하거나 두 번째 앱을 띄우지 않는다.
- Peekaboo Screen Recording/Accessibility가 없으면 native 단계는 blocked로 보고한다; screenshot·기능 테스트가 없는 focus/resize/IME 결과를 PASS라고 쓰지 않는다.
- 관련 Rust/Swift required suites, invariant gates, `gen-pen.mjs`, `check-design-contract.mjs`, 실제 native flow와 영향을 받는 baseline/candidate idle·driven 비교를 수행한다; 최종 Fidelity/Code review와 fresh receipt를 완료 권위로 사용한다.
- 화면 미적 baseline은 최종 native 결과에 대한 사용자 판단을 남길 수 있지만 기능·입력·상태 검증 누락을 taste review로 바꾸지 않는다; Pen의 opacity 백분율과 폰트 대체는 native 값과 별도로 확인한다.
- 원본 Pen live state에 미저장 변경이 있을 수 있다; MCP로 먼저 확인하고 worktree 문서로 전환하기 전에 보존하며 .pen을 직접 읽거나 수정하지 않는다.
- 모든 실행 증거는 `agents/runs/hide-design-refactoring/` 아래에 두고 source에 커밋하지 않는다; 로컬 전달 제한과 shared config의 PR 기본값 충돌을 외부 push로 해결하지 않는다.
- 구현 전 남은 제품 구조 질문은 없다; 운영자 native 전환·권한 같은 환경 조건은 확인되는 시점에 구체적으로 보고하며 확인 전 승인됐다고 간주하지 않는다.
