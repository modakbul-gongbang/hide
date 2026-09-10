---
topic: "hide-orchestrator"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "hide가 사용자의 전역 에이전트 설정 파일(~/.claude/settings.json, ~/.codex/hooks.json)을 첫 실행에 자동으로 수정하며, 그 배열에는 이미 다른 도구의 훅이 들어 있어 잘못 쓰면 사용자의 무관한 도구가 조용히 망가진다."
source_intake: "agents/interview/hide-orchestrator/qa-log.md"
created_at: "2026-09-10"
updated_at: "2026-09-10"
---

# PRD: hide-orchestrator

## Goal

hide 조작자는 한 저장소에서 여러 에이전트를 동시에 돌리고, 그중 하나(Observer)에게 나머지를 위임한다. 지금은 에이전트가 자식을 띄우면 herdr가 같은 탭에 pane을 split하므로 Observer가 화면 절반으로 줄고 자식이 늘수록 계속 쪼개진다. 그리고 pane이 없는 in-process subagent는 어디에도 나타나지 않아, 자식 다섯 개를 돌리는 pane과 정말 혼자 일하는 pane이 화면에서 똑같이 보인다. 이 변경 이후 캔버스는 항상 pane 하나를 유지하고, 파생된 자식은 숨은 탭으로 옮겨져 계보와 소유권(진함=내 일 / 흐림=위임됨)으로만 드러나며, 사람은 Observer가 못 푼 것과 15분 넘게 정체된 것에만 개입한다. 목표는 자식을 잘 보는 것이 아니라 안 봐도 되게 하는 것이다.

Approval checklist:

- hide가 첫 실행에 사용자의 전역 설정 파일을 자동으로 수정하는 것 (B25, B26, B29, Risks)
- 켤 때 이미 split으로 떠 있는 자식의 레이아웃을 hide가 정리하는 것 (B3)
- 자식의 질문·승인·에러·완료를 사람 화면으로 올리지 않는 것 (B15, B16)
- `docs/status-model.md`의 상태 축을 셋에서 넷으로 늘리고 `Done`의 의미를 루트로 한정하는 것 (B11, B16, Technical structure)
- 시각 판정을 사용자가 수행하는 것 - `human:` 증명 행이 있다 (D-34)
- delivery mode: `pr` (`agents/config.json`의 기존 설정, D-59)

## Non-goals

- **오케스트레이터·칸반·머지.** 결과: 작업 큐에서 worktree 배정과 PR 머지까지를 hide가 소유하는 일은 이번에 없다. 이번 산출물은 그 전신이어야 하며 Overview의 worktree 행이 카드가 되고 상태가 열이 된다. 재검토: 사용자가 C 단계를 시작할 때. (D-06)
- **에이전트 간 통신 간선.** 결과: `A<>B`가 서로 말했다는 사실은 어디에도 그려지지 않는다. 관측 채널 자체가 없다 - `SubagentStart`/`SubagentStop`은 파생만 알리고 통신은 어느 층에도 기록되지 않는다. 재검토: 런타임이 통신 훅을 노출할 때. (D-21)
- **별도의 에이전트 그래프 뷰 화면.** 결과: 계보는 사이드바 트리, 현재 경로는 pane 헤더 breadcrumb, 여러 worktree 현황은 Overview 행이 나눠 맡고 네 번째 화면은 없다. 재검토: 세 표면으로 담기지 않는 정보가 생길 때. (D-12)
- **cloud session과 원격 호스트의 subagent 관측.** 결과: 원격 pane은 영구히 미계측(`~`)으로 남는다. hide는 다른 머신의 파일시스템에 쓰지 않는다. 재검토: 원격 계측 부재가 실제로 아플 때. (D-28, D-49)
- **Observer와 자식을 나란히 보는 hide 기능.** 결과: 열람 경로는 전체 화면 교체 하나다. 사용자가 herdr에서 직접 split하는 길은 막지 않는다. 재검토: 두 pane 비교가 실제로 반복될 때. (D-23)
- **steer 전용 UI(자식 중단·재시작·프롬프트 전송, 다중 선택 일괄 조작).** 결과: 자식을 조작하려면 교체해서 그 터미널에 직접 입력한다. 자식이 열 개를 넘으면 왕복 비용이 아프다. 재검토: 일괄 조작이 필요한 상황이 관측될 때. (D-29)
- **정체 임계값 설정 항목.** 결과: 5분/15분이 고정이다. 재검토: 실제 운용에서 그 숫자가 틀렸다고 관측될 때. (D-41)
- **소유권을 상호작용 경계로 강제하는 것.** 결과: 위임된 pane에 사람이 들어가 입력해도 막지 않는다. 흐림은 정보이고 권한이 아니다. (D-56)
- **개별 in-process subagent를 노드로 그리는 것.** 결과: subagent는 개수로만 요약되고 신원과 개별 상태는 보이지 않는다. pane이 없어 사람이 가서 만질 수 없으므로 그릴 값이 없다. (D-11, D-50)
- **고아 자식의 자동 정리.** 결과: 부모가 죽어도 자식 프로세스는 남는다. 돌고 있는 작업을 죽이지 않는다. (D-52)
- **다섯 번째 상태 그룹이나 새 상태어.** `docs/status-model.md`의 4그룹과 대표 선정 순서를 그대로 쓴다. (D-04)
- **herdr fork 변경, 새 소켓 메서드, 새 이벤트 구독, 새 폴링.** pinned 0.8.2 계약만으로 성립한다. (D-08, D-33, `engineering/principles.md` rule 7)
- **`report-metadata` 값 위에 권한이나 제어를 세우는 것.** herdr가 그것을 display-only로 규정하므로 표시에만 쓴다. (D-27)
- **방문 히스토리 스택 같은 새 지속 상태.** breadcrumb는 lineage에서 파생된다. (D-18, `engineering/principles.md` rule 2)
- **아직 필요 없는 확장점.** 통신 간선을 태울 수 있는 빈 구조나 조절 가능한 임계값 설정을 미리 만들지 않는다. (`engineering/principles.md` rule 2)
- **뷰가 herdr나 파일시스템을 직접 호출하는 것.** 코어가 값과 상태 전이를 소유하고 Swift는 그리기와 입력을 소유한다. (`engineering/principles.md` rule 5)
- **새 UI 패턴 발명.** 칩·breadcrumb·명암·미계측 마크는 기존 `HideTheme` 토큰과 컴포넌트를 확장한다. (`design/principles.md` rule 5)
- **`lineage_collapsed`·`lineage_orphan`·Workspace 칩·PR 컨트롤과 나란한 두 번째 구현.** (`engineering/principles.md` rule 7)

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | herdr가 이미 `parent_agent_instance_id`를 노출하고 `apply_lineage`가 depth·child_pane_ids·orphan·collapsed를 계산하므로 계층과 접기의 기반은 이미 있다. | repo `herdr-core/src/sidebar.rs:404-511`, `DESIGN.md:817-819` |
| D-02 | sasu의 유일한 dispatch 경로가 `SASU_HERDR_ROLE=implementor`를 원자적으로 주입하므로 파생은 lineage 외에 pane 환경변수로도 식별된다. | `~/projects/sasu/skills/implement/references/observer-and-herdr.md` |
| D-03 | worktree 생성 진입점과 "작업 시작 = worktree 생성"은 이미 결정·구현되어 있다. | repo `agents/prd/hide-worktree-per-task/prd.md` D-04/D-07 |
| D-04 | 상태는 demand/activity/read 3축과 4그룹(Needs You / Done / Working / Seen)이며 `sidebar.rs`가 단독 소유한다. 다섯 번째 그룹이나 새 상태어를 만들지 않는다. | repo `docs/status-model.md` |
| D-05 | Workspace 행에 PR 아이콘과 CI rollup popover가 이미 있어 칸반의 리뷰·머지 데이터 경로 일부가 존재한다. | repo `DESIGN.md` Native Git and lineage tokens, `herdr-core/src/github.rs` |
| D-06 | 이번 사이클은 계층 표현·숨김과 현황 파악·steer까지다. 오케스트레이터·칸반·머지는 다음 사이클이며 이번 산출물이 그 전신이어야 한다. 기각: 계층 표현만, 칸반까지 한 번에. | qa-log Q1 |
| D-07 | hide가 못 보는 파생은 보고 규약으로 다룬다. 런타임이 무엇을 쓰든 자기 파생을 herdr pane metadata로 report하고 hide가 읽는다. 미계측 pane은 "혼자 일하는 pane"과 구분되게 표시한다. 기각: 관측 가능한 것만 그리기, pane 강제 - 둘 다 미계측과 진짜 단독을 구분 못 해 화면이 거짓말을 한다. | qa-log Q2 |
| D-08 | 보고 규약은 pinned herdr 0.8.2 계약만으로 성립한다. `pane.report_metadata`가 쓴 값이 `PaneInfo`의 같은 필드로 되읽히고, `source`/`applies_to_source`/`seq`는 여러 독립 보고자와 순서를 전제한 필드다. fork 변경 불필요. | repo `contracts/herdr-api.schema.json` |
| D-09 | Claude Code와 Codex가 같은 훅 어휘를 쓴다. Claude Code 2.1.266 바이너리에 `SubagentStart`/`SubagentStop`과 `agent_id`/`agent_type`/`subagent_type`/`session_id`/`teammate`가 있고, Codex의 선언 이벤트는 PascalCase까지 동일하다. | 라이브 확인 2026-09-09: `~/.local/share/claude/versions/2.1.266` 문자열 grep, `~/.codex/hooks.json` |
| D-10 | 보고 규약의 한계 넷: `tokens`는 최대 16키 문자열이라 요약만 담긴다, pane metadata push 이벤트가 계약에 없다, 훅은 설치된 세션에서만 뜨므로 미계측이 반드시 발생한다, 통신 간선은 훅 이벤트에 없다. | repo `contracts/herdr-api.schema.json`, `docs/PERFORMANCE_TESTING.md` |
| D-11 | 노드는 pane이다. in-process subagent는 독립 노드가 아니라 부모 pane의 배지·카운트로 요약된다. 기각: 에이전트 단위 노드 - 제어할 수 없는 노드를 그리게 되어 steer 목적과 어긋나고 새 저장소와 수집 경로를 동시에 요구한다. | qa-log Q3 |
| D-12 | 별도의 에이전트 그래프 뷰 화면을 만들지 않는다. 계보는 사이드바 lineage 트리, 현재 경로와 형제 선택은 pane 헤더 breadcrumb, 여러 worktree 현황은 Overview의 worktree 행이 맡는다. 기각: 전용 전체 탭 Orchestrator, Overview 안의 별도 그래프 영역. | qa-log Q4, Q17, gap-audit F2 |
| D-13 | Overview는 우측 패널의 첫 섹션이고 project-scoped이며 `HideTheme.Overview` 토큰으로 rail·노드·커밋 행·96pt worktree 행과 ahead/behind·pushed·disk 상세를 그린다. 레인 색은 레인 식별용이고 에이전트 상태색이 아니다. | repo `DESIGN.md:1087-1130`, `macos/Sources/HerdrMacOS/CheckoutOverview.swift` |
| D-14 | sasu dispatch는 `herdr pane split --direction right --env ... --no-focus`이므로 Implementor는 같은 탭의 형제 split이다. 탭은 늘지 않고 사이드바에서는 이미 자식으로 접히므로 쌓이는 표면은 캔버스 하나다. | repo `~/projects/sasu/cli/src/implement/herdr.ts:271` |
| D-15 | 자식 pane을 같은 탭의 split으로 두지 않고 별도 탭으로 옮겨 캔버스에서 치운다. Herdr가 split geometry와 PTY 크기를 계속 소유하므로 "hide가 그냥 안 그린다"는 자식 터미널 폭을 틀리게 만든다. 기각: Observer를 herdr zoom, sasu dispatch 규약 변경. | qa-log Q5 |
| D-16 | 자식을 볼 때 다시 split하지 않고 전체 화면을 교체한다. 자식이 별도 탭에 있으므로 pane select가 기존 `align_visible_tab_with_selected_pane`을 타서 새 메커니즘이 0이고, dispatch 직후 Observer는 대기라 화면을 반 나누는 것이 낭비다. 사용자 제안이 에이전트의 split 복원안을 대체했다. | qa-log Q6 |
| D-17 | 런타임 계약에 "focused checkout always draws the tab that holds the selected pane"이 있고 구현이 `runtime.rs:3332`다. `ui_state`에는 `selected_pane_id`만 있고 방문 히스토리 스택은 없다. | repo `herdr-core/src/runtime.rs:3332`, `AGENTS.md` Runtime Architecture |
| D-18 | 탐색은 pane 헤더 breadcrumb이고 방문 히스토리 스택을 만들지 않는다. 경로가 lineage에서 매번 파생되므로 저장 상태가 0이고 재시작·탭 전환·자식 종료에서 스택이 썩지 않는다. | qa-log Q7 |
| D-19 | 깊은 체인과 형제 선택은 모달이 아니라 breadcrumb 각 단계의 드롭다운으로 처리한다. 모달은 클릭을 하나 더 먹고 판단 근거인 뒤쪽 상태를 가린다. 사용자가 모달을 제안했으나 권고를 수용해 뒤집었다. | qa-log Q7 |
| D-20 | 부모 pane 헤더에 자식 상태 칩 줄을 상시 표시한다. `lineage_child_pane_ids`와 4그룹 계약을 그대로 쓰므로 새 데이터 경로가 없고, 넘치면 Workspace 칩의 `+N` 패턴을 재사용한다. | qa-log Q6, Q7 |
| D-21 | 에이전트 간 통신 간선은 non-goal이다. 관측 채널이 없으므로 억지로 그리면 D-07을 어긴다. 경계: pane 헤더는 계보, Overview는 지도. 기각: 런타임마다 통신 래퍼 심기(새 런타임마다 다시 써야 하므로 확장 요구와 어긋남), breadcrumb를 방문 경로로 정의해 간선 태우기(빈 확장점). | qa-log Q8 |
| D-22 | 자식 pane은 생성 즉시 자동으로 옮겨지고 사용자 조작이 필요 없다. 기각: 사용자가 접기 버튼을 누르는 안 - 자식이 뜰 때마다 조작이 필요하므로 원래 문제를 고치지 못한다. | qa-log Q9 |
| D-23 | 나란히 보기를 hide 기능으로 만들지 않고 교체 모델 하나만 남긴다. 기각: 자동으로 치우되 옆에 붙이기를 액션으로 남기는 안 - 열람 경로가 둘로 갈라져 둘 다 유지보수해야 한다. | qa-log Q9 |
| D-24 | pane 없는 in-process subagent도 관측 범위에 포함하므로 훅 설치가 필수 경로가 된다. pane 있는 자식은 herdr lineage가 이미 알아 훅이 불필요하다 - 파생 두 종류에 경로가 둘이다. | qa-log Q11 |
| D-25 | 훅은 hide 설치 시 자동으로 붙인다. 에이전트는 전역 설정을 조용히 바꾸는 위험과 이미 점유된 훅 배열을 근거로 원클릭을 권고했으나 사용자가 마찰 없는 자동 설치를 택하고 재확인했다. 자동이어도 append여야 하고 기존 훅을 보존해야 한다. | 사용자: "hook은 그냥 설치하게 해버리면 좋겠어. 그냥 설치시 자동으로 붙여버리자" (qa-log Q15) |
| D-26 | 훅 배열은 이미 점유되어 있다. 이 머신의 `~/.codex/hooks.json`은 `SubagentStart`에 `inject-principles.sh`와 orca `codex-hook.sh` 두 개를 달고 있다. 설치는 append여야 하고 덮어쓰면 남의 훅이 사라진다. | 라이브 확인 2026-09-09 `~/.codex/hooks.json` |
| D-27 | `herdr pane report-metadata`는 CLI로도 존재하므로 훅 스크립트가 한 줄이다. herdr 자신이 이를 display-only로 규정하므로 hide는 표시에만 쓴다. | 라이브 확인 2026-09-09 `herdr pane report-metadata --help` (herdr 0.8.2) |
| D-28 | 원격 호스트에는 훅을 설치하지 않는다. 기각: SSH로 원격 설정을 수정하는 안 - 실패·롤백·권한·프로필별 HOME 문제가 로컬과 완전히 다른 별개 기능이다. | qa-log Q14 |
| D-29 | steer 전용 UI를 만들지 않는다. 교체 모델이 기본형을 제공한다. 기각: 칩 컨텍스트 메뉴(1동작만 아낀다), Overview 다중 선택 일괄 조작(칸반이 오면 겹친다). | qa-log Q15 |
| D-30 | 미계측 마크와 자식 배지는 herdr가 에이전트를 감지한 pane에만 그린다. pane 하나 = 에이전트 하나다. 자리 제한(사이드바 행과 pane 헤더 둘뿐)은 D-60이 Overview를 세 번째 자리로 더하며 대체한다. | qa-log Q15, D-11/D-20/D-04에서 파생; 자리 제한은 D-60이 대체 |
| D-31 | 훅은 첫 실행 1회 자동 설치하고 이후 조용히 손대지 않는다. 낡은 훅은 Settings 진단이 사용자에게 물어본 뒤 재설치한다. 판정 근거는 `--source`에 심은 버전이다. 기각: 매 실행 무조건 복구 - 사용자가 영구히 못 지운다. | qa-log Q16 |
| D-32 | Overview의 worktree 행에 에이전트 한 줄만 추가하고 새 영역이나 패턴을 만들지 않는다. Overview의 고유 가치는 폭이며 사이드바가 담을 수 없는 "누가 무엇을 하는 중"이 들어간다. 기각: 별도 그래프 영역, Overview 미변경. | qa-log Q17 |
| D-33 | `agent.list`의 `AgentInfo`가 `tokens`·`state_labels`·`state_change_seq`·`revision`·`spawned_from_pane_id`를 이미 담고 hide가 이미 초당 1회 받으며 변화 없는 틱은 아무것도 publish하지 않으므로, 새 폴링과 새 이벤트 구독이 0이다. | repo `contracts/herdr-api.schema.json`, `AGENTS.md` Runtime Architecture |
| D-34 | 시각 판정은 사용자가 한다. 에이전트가 네이티브 실행과 스크린샷까지 하고 사용자가 보고 판정한다. `hide-worktree-per-task`에서는 위임했으나 이번에는 뒤집는다 - "화면이 안 쪼개진다"는 체감은 사용자만 판정할 수 있다. | qa-log Q18 |
| D-35 | 자식이 막혔을 때 1차 책임은 Observer이고 사람은 예외에만 개입한다. 자식의 질문은 사람 화면에 뜨지 않는다. 기각: 자식의 질문을 즉시 사람에게 올리기(자식 10개면 사람이 10번 개입해 위임이 실패), 설정으로 고르게 하기. | qa-log Q20 |
| D-36 | 계보가 소유권을 결정한다: lineage 루트 = 사람 것, 자식 = 위임됨. 표현은 명암이며 위임된 것은 흐리게 그리고 출처를 표시한다. `DESIGN.md`에 이미 emphasis 개념이 있어 새 패턴이 아니다. | qa-log Q22 |
| D-37 | 승격은 명암 변화로 표현하고 별도 알림 장치를 만들지 않는다. soft는 Working 안의 시각 변조이고 hard일 때만 그룹이 Needs You로 바뀌므로 4그룹 계약을 깨지 않는다. | qa-log Q22 |
| D-38 | 자식의 Done은 사람의 확인 대상이 아니다. 확인은 Observer의 일이며 사람에게 요구하면 위임이 아니라 일을 되돌려주는 것이다. 루트의 Done만 사람의 확인 대상이다. Q19의 "끝난 자식을 사람이 확인한다" 안은 Q20의 목적 재정의로 폐기되었다. | qa-log Q20의 목적 재정의, D-35/D-36과 정합 |
| D-39 | 다른 checkout의 자식은 사이드바에 두 번 나타나는 것을 유지하고 둘 다 흐림과 출처를 붙인다. 기각: 계보 위치에만 그리기 - 칩이 세는데 펼치면 없는 상태는 `status-model.md`가 막은 이미 고쳐진 버그다. 기각: 칩도 안 세기 - worktree를 작업 단위로 삼은 D-03과 어긋난다. | qa-log Q22 |
| D-40 | 숨기는 것은 캔버스와 탭 스트립뿐이고 사이드바는 그대로 보인다. 부모를 접으면 자식이 접히는 기존 `lineage_collapsed`가 그 역할을 이미 한다. | repo `docs/status-model.md`, `herdr-core/src/sidebar.rs` |
| D-41 | 정체 임계값은 고정 2단계다: soft 5분, hard 15분. 설정 항목을 만들지 않는다. 기각: 설정 조절(어떤 숫자가 맞는지 아직 모른다), soft 없이 hard만(방치 방지가 안 된다). | qa-log Q23 |
| D-42 | 정체 감지는 에이전트의 협조를 요구하지 않는 안전망이다. Observer의 자식 감시는 에이전트의 일이고 hide는 sasu의 존재를 모른다. hide가 보는 것은 pane과 시간뿐이므로 Observer가 훌륭하면 승격이 뜨지 않고 형편없으면 15분 뒤 사람에게 온다. | qa-log Q24, D-33/D-35에서 파생 |
| D-43 | 자식은 같은 Workspace 안의 새 탭으로 옮긴다. dispatch가 부모와 같은 `--cwd`를 쓰므로 경로가 같고, 새 Workspace를 만들면 같은 경로가 둘로 갈라진다. | qa-log Q25 |
| D-44 | hide를 새로 켤 때 이미 split으로 떠 있는 자식도 정리한다. 기각: 새로 뜨는 자식만 적용 - 켰을 때 쪼개져 있으면 기능이 도착하지 않은 것으로 읽힌다. 되돌리는 길이 이미 있으므로 레이아웃 손실이 아니다. | qa-log Q25 |
| D-45 | 옮기기 실패는 조용히 재시도하고 pane 헤더에 에러를 띄우지 않으며 상태바와 진단에만 남긴다. 에이전트는 pane 헤더 표시를 권고했으나 사용자가 소음을 우선해 뒤집었다. 실패가 caller-visible outcome으로 라우팅되므로 `engineering/principles.md` rule 4/10은 충족된다. | 사용자: "실패를 숨겨도 돼. 안되면 그냥 그 맨 아래에 에러사항 보이는 정도로만? 이게 불필요한 에러들을 다 안보여줘도 돼" (qa-log Q26) |
| D-46 | 훅 설치 실패 시 설치를 중단하고 파일을 건드리지 않으며 진단에 사유를 남긴다. 파싱 실패한 설정 파일은 읽기 실패로 취급하고 절대 쓰지 않는다 - append할 수 없으므로 손을 뗀다. 손상된 파일을 새로 쓰는 안은 남의 훅을 지우므로 금지. | qa-log Q26 |
| D-47 | 훅 관리를 워크스페이스의 별도 crate로 분리한다. 설정 파일 위치와 형식, 읽기-파싱-append-쓰기, 설치 상태 판정, 진단, 런타임 어댑터를 소유한다. 기각: `herdr-core` 안의 모듈 - 파일 I/O가 `Mutex<Runtime>` crate에 섞이고 최우선 검증이 코어와 FFI를 끌고 온다. | qa-log Q27 |
| D-48 | 진단은 doctor로 분리하고 crate API와 CLI bin 둘 다 노출한다. `herdr-core/src/bin/herdr-ide-fixture.rs` 선례가 있다. 훅이 잘못되면 화면은 `~`만 보여주므로 터미널 확인 경로가 필요하고, bin 출력이 그대로 증거가 된다. | qa-log Q28 |
| D-49 | 관측 대상은 로컬 herdr pane과 로컬 in-process subagent까지다. team mode 세션이 로컬 pane으로 뜨면 특별 취급 없이 평범한 pane으로 보인다. cloud session과 원격 subagent는 non-goal이다. | qa-log Q29 (gap-audit F1 번들 승인) |
| D-50 | in-process subagent는 working/done/blocked 카운트까지만 표현하고 개별 신원과 상태는 그리지 않으며 승격에 참여하지 않는다. 부모 자신의 상태만 승격된다. | qa-log Q29 (gap-audit F3 번들 승인) |
| D-51 | 정체 타이머의 상태 규칙: `Needs You` 진입 또는 `Working`인데 `state_change_seq`가 오르지 않을 때 시작, seq 상승 시 리셋, `Done`/stopped 자식과 released pane과 미계측 pane은 제외, 일시정지 상태는 없다. | qa-log Q29 (gap-audit F4 번들 승인) |
| D-52 | 부모가 종료·크래시하거나 계보를 잃으면 소유권이 사람에게 돌아온다: 흐림이 걷히고 그 자식이 자기 Workspace의 루트로 그려지며 escalation 책임도 사람이 된다. `lineage_orphan`과 `lineage_hint`가 이미 있다. 자동 정리는 하지 않는다. | qa-log Q29 (gap-audit F5 번들 승인) |
| D-53 | 훅 payload에서 카운트로 가는 계약: pane 식별은 `$HERDR_PANE_ID`, 훅 스크립트는 stateless, 카운터는 crate가 pane id 키로 로컬에 유지, `SessionStart`에서 리셋, `SubagentStop` 누락은 `SessionEnd`/`Stop`에서 정리, 어댑터가 못 주는 상태는 미계측 fallback이며 0으로 표시하지 않는다. | qa-log Q29 (gap-audit F6 번들 승인) |
| D-54 | 정체 시계의 권위 출처는 hide다. herdr가 타임스탬프를 주지 않으므로 hide가 "이 `state_change_seq`를 처음 본 시각"을 기억한다. hide 재시작 시 리셋하고, 서버 disconnect 동안 정지하며 재연결 시 재개한다. | qa-log Q29 (gap-audit F7 번들 승인) |
| D-55 | Overview는 Q17의 A안으로 확정한다: worktree 행에 에이전트 한 줄, 새 영역 없음. Q17의 응답이 짧은 긍정이었으므로 게이트가 명시적 확인을 요구했고 사용자가 확인했다. | qa-log Q29 (gap-audit F8 명시 확인) |
| D-56 | 소유권 표현은 조언이며 상호작용 경계가 아니다. 위임된 pane의 선택과 입력을 막지 않는다 - hard 승격 시 사람이 반드시 개입해야 하므로 막아두면 정작 필요한 순간에 들어갈 수 없다. | qa-log Q29 (gap-audit F9 번들 승인) |
| D-57 | hide가 설치한 훅 항목에 소유권 마커를 심고, Settings 진단의 제거 액션은 그 마커가 붙은 항목만 배열에서 빼낸다. 마커 없는 항목은 절대 건드리지 않는다. 이것이 D-31에 빠져 있던 되돌리기 경로를 채운다. | qa-log Q29 (gap-audit F10 번들 승인) |
| D-58 | 원칙 인테이크: `engineering/principles.md`와 `design/principles.md`를 커밋 `f9ce2da`에서 전문 읽었다. 두 도메인의 트리거가 모두 이 작업에 걸린다(코드·아키텍처·에러 경로 / 사용자가 일하는 화면). 관측 가능한 규칙은 Behaviors로, 관측 결과가 없는 구현 제약은 non-goal로 번역했다. 번역하지 않은 규칙: engineering rule 9(구조적 로그) - hide는 사용자 개인 머신의 데스크톱 앱이고 상관 ID 기반 로그 파이프라인이 없으며 이 변경이 그것을 도입하지 않으므로, 실패 전달은 rule 10의 caller-visible outcome(상태바·진단)으로만 만족시킨다. engineering rule 11(멱등 재시도)은 B25와 B29의 append/제거가 마커 기반으로 수렴하는 것으로 번역했다. | `sasu principles list --json`, 두 문서 전문 |
| D-59 | delivery mode는 `pr`이다(base `main`, prefix `gen-prd`, CI watch 켜짐, 최대 2회 수정 시도). 저장소의 기존 설정이며 이 PRD가 바꾸지 않는다. | repo `agents/config.json` |
| D-60 | 미계측 표시 자리를 셋으로 확장한다. Overview의 worktree 에이전트 줄이 세 번째 자리다. 이유: 그 줄이 비어 있을 때 "에이전트가 없다"와 "모른다"를 구분하지 못하면 D-07이 막으려던 오독이 그 화면에서 재현된다. 상위 원칙인 D-07이 D-30의 자리 제한을 이긴다. D-30의 나머지(에이전트가 감지된 pane에만 그린다)는 세 자리 모두에 유지된다. | qa-log D-60 (spec gate F1 번들 승인) |
| D-61 | "훅 설치 이전에 시작된 세션"을 별도 미계측 사유로 승인하고 판정 규칙을 확정한다: 설정 파일에 hide 훅 항목이 있는데 그 pane에 hide의 `source`가 없으면 그 세션은 설치 이전에 시작된 것이다. 두 관찰값의 조합이라 새 데이터나 새 계약이 없다. | qa-log D-61 (spec gate F2 번들 승인) |
| D-62 | 깊은 계보에서 승격은 계보 루트에 표시되고 툴팁이 어느 자손이 몇 분째 막혔는지 말한다. 중간 단계가 각자 다시 15분을 기다리게 하지 않는다. 기각: 한 단계씩 올라가는 안 - 깊이 3이면 사람이 45분 뒤에 알게 되는데 D-35의 목적은 개입 횟수를 줄이는 것이지 지연을 늘리는 것이 아니다. 사람이 보는 것은 진한 루트 목록이므로(D-36) 루트 표시가 정합한다. | qa-log D-62 (spec gate F3 번들 승인) |
| D-63 | pane 자식과 in-process subagent를 한 부모 아래에서 섞지 않는다. 칩 줄에는 pane 자식만 개별 칩으로, subagent는 줄 끝의 카운트 배지 하나로 따로 나타난다. 대표 상태 선정과 승격에는 pane 자식만 참여하고 두 종류의 개수를 합치지 않는다. 이유: 둘은 제어 가능성이 다르므로 합치면 사람이 "내가 만질 수 있는 것"을 셀 수 없다. | qa-log D-63 (spec gate F4 번들 승인) |
| D-64 | 미계측 사유는 위에서 아래로 처음 맞는 것을 쓰는 순서 규칙으로 판정한다: 설정 파일 읽기 실패, 원격 host scope, 훅 항목 없음, 훅은 있고 `source` 없음, `source` 버전 낮음. 어디에도 해당하지 않으면 단일 fallback "자식 정보를 알 수 없습니다"를 쓰며 사유를 추측하거나 빈 값으로 두지 않는다. | qa-log D-64 (spec gate F5 번들 승인) |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 에이전트가 자식 pane을 만들면 캔버스는 부모 pane의 전체 화면을 유지한다. 자식은 같은 Workspace 안의 새 탭으로 자동 이동하고 그 탭은 탭 스트립에 나타나지 않는다. 사용자 조작은 없다. | D-15, D-22, D-43 |
| B2 | 자식을 옮기는 데 실패하면 자식은 split으로 남고 pane 헤더에는 아무 에러도 나타나지 않는다. 실패 사실은 화면 하단 상태바와 Settings 진단에만 나타나고 hide는 조용히 재시도한다. 재시도가 성공하면 자동으로 정리된다. | D-45 |
| B3 | hide를 켤 때 herdr 세션에 이미 split으로 떠 있던 자식도 정리되어 캔버스가 pane 하나가 된다. 정리에 실패한 자식은 B2와 같은 경로로 남는다. | D-44, D-45 |
| B4 | 자식 pane은 사이드바에 계속 나타난다. 숨는 것은 캔버스와 탭 스트립뿐이다. 부모 행을 접으면 그 자식 행들이 접힌다. | D-40 |
| B5 | 자식이 있는 pane의 헤더에 자식 상태 칩 줄이 상시 나타난다. 칩 줄에는 pane 자식만 개별 칩으로 나타나고 각 칩이 기존 4그룹 상태 마크와 이름을 보인다. 부모 배지는 pane 자식들의 대표 상태를 기존 우선순위(Needs You > Done > Working > Seen, 그 안에서 Error > Approval > Question)로 보이며, in-process subagent는 이 대표 선정에 들어가지 않는다. | D-20, D-04, D-63 |
| B6 | 칩이 줄을 넘치면 Workspace 칩과 같은 `+N` 형태로 접힌다. | D-20 |
| B7 | 자식 칩을 클릭하면 화면 전체가 그 자식으로 교체된다. 화면은 분할되지 않고 breadcrumb가 그 자식까지의 경로를 보인다. 그 자식이 입력을 기다리고 있으면 그 자리에서 터미널에 직접 입력할 수 있다. | D-16, D-29 |
| B8 | 클릭한 자식이 그 사이 종료되어 pane이 사라졌으면 화면은 교체되지 않고 그 칩이 사라지며 breadcrumb는 부모에 머문다. | D-16 |
| B9 | breadcrumb의 상위 단계를 클릭하면 몇 단계 깊이든 한 번에 그 단계로 돌아온다. 경로는 저장되지 않고 계보에서 파생되므로 hide 재시작, 탭 전환, 자식 종료 뒤에도 어긋나지 않는다. | D-18, D-17 |
| B10 | breadcrumb 각 단계의 드롭다운이 그 층의 형제 목록을 보이고, 형제를 고르면 부모를 거치지 않고 바로 그 형제로 교체된다. 모달은 나타나지 않는다. | D-19 |
| B11 | 사이드바와 pane 헤더에서 계보 루트 에이전트는 진하게, 위임된 자식은 흐리게 나타나고 자식 행에는 위임 출처가 함께 나타난다. 사이드바를 훑으면 진한 것이 사람이 만질 대상이다. | D-36 |
| B12 | 부모와 다른 checkout에서 도는 자식은 사이드바 두 곳에 나타난다: 부모 밑의 계보 자식으로, 그리고 그 자식을 물리적으로 소유한 Workspace 밑의 행으로. 두 곳 모두 흐리고 출처를 보인다. 그 Workspace를 펼치면 칩이 세는 에이전트가 실제로 보인다. | D-39, D-36 |
| B13 | 부모가 종료·크래시하거나 계보를 잃어 자식이 고아가 되면 그 자식의 흐림이 걷히고 자기 Workspace의 루트로 나타나며, 그 뒤로는 사람이 승격 대상으로 받는다. hide는 고아 자식을 닫거나 정리하지 않는다. | D-52 |
| B14 | 위임된 흐린 pane도 선택할 수 있고 그 터미널에 입력할 수 있다. 흐림이 조작을 막지 않는다. | D-56 |
| B15 | 자식이 질문·승인·에러 상태가 되어도 사람 화면의 상태 그룹은 바뀌지 않는다. 그 demand는 부모 배지에 자식 상태로만 나타나고 사람의 Needs You를 만들지 않는다. | D-35 |
| B16 | 자식이 완료해도 사람의 Needs You가 생기지 않고 그 자식은 흐린 `Done`으로 남는다. 계보 루트 에이전트가 완료하면 기존대로 사람의 확인 대상이 된다. 완료한 자식의 칩을 누르면 그 출력을 볼 수 있다. | D-38, D-04 |
| B17 | 자식이 5분 넘게 정체하면 계보 루트의 칩에 흐린 정체 표시가 붙고 툴팁이 어느 자손이 몇 분째 무엇을 기다리는지 말한다. 상태 그룹은 `Working`에 머물고 사람을 부르지 않는다. | D-41, D-37, D-62 |
| B18 | 자식이 15분 넘게 정체하면 그 자식의 흐림이 걷히고 계보 루트의 상태 그룹이 `Needs You`로 바뀌어 사람에게 올라온다. 자손이 몇 단계 아래에 있어도 루트에 바로 나타나고 툴팁이 어느 자손이 몇 분째 무엇에 막혔는지 말한다. 중간 단계가 각자 다시 15분을 기다리게 하지 않는다. 별도 알림 창은 나타나지 않고 명암과 그룹 변화가 그 신호다. | D-41, D-37, D-62 |
| B19 | 정체 시계는 자식이 `Needs You`에 들어갈 때, 또는 `Working`인데 상태 변화가 멈출 때 시작하고, 상태가 변하면 리셋된다. 완료·정지한 자식, 릴리스된 pane, 미계측 pane은 정체로 세지 않는다. | D-51 |
| B20 | hide를 재시작하면 정체 시계가 처음부터 다시 세므로 켠 직후 승격이 쏟아지지 않는다. herdr 서버 연결이 끊긴 동안은 시계가 멈추고 재연결하면 이어서 센다. | D-54 |
| B21 | 에이전트가 감지된 pane인데 자식 정보를 알 수 없으면 자식 칩 자리에 흐린 미계측 마크가 나타나고, 툴팁이 다음 순서로 처음 맞는 사유를 말한다: 설정 파일을 읽을 수 없음, 원격 호스트라 설치하지 않음, 훅이 설치되지 않음, 훅은 있으나 이 세션이 설치 이전에 시작됨(재시작하면 계측된다는 안내를 함께), 훅이 오래됨. 툴팁에서 Settings 진단으로 갈 수 있다. | D-07, D-31, D-28, D-64 |
| B22 | 에이전트가 감지되지 않은 pane(셸, 편집기, 로그)에는 자식 칩도 미계측 마크도 나타나지 않는다. | D-30 |
| B23 | 계측된 세션에서 자식이 없으면 아무 칩도 나타나지 않는다. 그 빈 상태는 미계측과 다른 화면이고 "이 pane은 혼자 일한다"는 확인된 정보다. | D-07, D-30 |
| B24 | pane 없는 in-process subagent는 칩 줄 끝의 카운트 배지 하나로만 나타난다(작업 중·완료·막힘 카운트). 개별 subagent의 이름이나 상태는 나타나지 않고 사이드바 행도 생기지 않으며, pane 자식 개수와 합쳐지지도 않고, 그 상태로 사람에게 승격되지 않는다. | D-50, D-11, D-63 |
| B25 | hide 첫 실행에서 로컬 런타임의 훅이 자동으로 설치된다. 설치는 기존 훅 배열에 추가하는 방식이고, 그 배열에 이미 있던 다른 도구의 훅은 개수와 내용이 그대로 남는다. 같은 설치를 두 번 수행해도 항목이 중복되지 않는다. | D-25, D-26, D-31 |
| B26 | 훅 설치가 권한 부족이나 설정 파일 파싱 실패로 실패하면 그 파일은 전혀 변경되지 않고 Settings 진단이 사유를 보인다. 그 런타임의 pane들은 B21의 미계측으로 나타난다. | D-46 |
| B27 | Settings 진단이 런타임별 훅 상태를 보인다: 설치됨(현재 버전), 오래됨, 미설치, 그리고 실패 사유. 훅이 설치되어 있는데 어떤 pane에 hide의 `source`가 없으면 그 pane은 설치 이전에 시작된 세션으로 판정되어 재시작 안내를 받는다. 사용자가 그 화면을 열면 왜 어떤 pane이 미계측인지 알 수 있다. | D-31, D-48, D-61 |
| B28 | 훅이 없거나 오래되었을 때 진단이 재설치할지 물어보고, 사용자가 승인한 뒤에만 설치한다. hide가 매 실행에서 조용히 다시 붙이지는 않는다. | D-31 |
| B29 | Settings 진단의 제거 액션은 hide가 설치한 항목만 훅 배열에서 빼내고 다른 도구의 훅은 남긴다. 제거한 뒤에는 hide가 다시 붙이지 않는다. | D-57, D-31 |
| B30 | 별도 실행 파일이 같은 진단 결과를 터미널에 출력하므로 앱을 띄우지 않고 훅 상태를 확인할 수 있다. | D-48 |
| B31 | subagent 카운트는 세션이 시작할 때 그 pane 기준으로 초기화되고, 세션이 끝나면 남아 있던 카운트가 정리되므로 죽은 세션의 개수가 화면에 남지 않는다. | D-53 |
| B32 | 런타임 어댑터가 어떤 상태를 제공하지 못하면 그 pane은 B21의 미계측으로 나타난다. B21의 어느 사유에도 해당하지 않으면 툴팁이 단일 문구 "자식 정보를 알 수 없습니다"를 보인다. 알 수 없는 값을 0이나 빈 값으로 보이거나 사유를 추측해 보이지 않는다. | D-53, D-64, `engineering/principles.md` rule 4 |
| B33 | 원격 호스트의 에이전트 pane은 미계측으로 나타나고 툴팁이 hide가 원격에는 훅을 설치하지 않는다고 말한다. | D-28, D-49 |
| B34 | Overview의 worktree 행에 에이전트 한 줄이 나타난다: 그 worktree에 붙어 있는 에이전트와 지금 무엇을 하는 중인지. 기존 브랜치·ahead/behind·pushed·PR·CI 표시는 그대로 남는다. | D-32, D-55, D-13 |
| B35 | 에이전트가 없는 worktree 행은 그 줄이 비어 있고, 미계측이면 그 줄이 B21과 같은 마크와 사유를 보인다. 그래서 Overview에서도 "에이전트가 없다"와 "모른다"가 다른 화면이다. 기존 PR 조회 실패 표시는 그대로 쓴다. | D-32, D-60, `design/principles.md` rule 9 |
| B36 | 이 변경이 늘리는 herdr 요청, 이벤트 구독, 타이머, 주기 작업은 없다. 자식 요약과 정체 판정은 이미 초당 1회 오는 에이전트 목록에서 계산되고, 상태가 변하지 않은 주기는 아무 화면 갱신도 만들지 않는다. | D-33, D-08 |
| B37 | 자식 칩, 미계측 마크, 정체 표시, 소유권 명암은 색만으로 의미를 전하지 않는다. 각각 기호나 텍스트를 함께 가지고 툴팁과 동일한 접근성 설명을 제공하며, breadcrumb와 칩은 키보드로 도달하고 조작할 수 있다. | D-04, `design/principles.md` rule 7 |
| B38 | 자식 이름, 위임 출처, 정체 툴팁, Overview의 에이전트 줄은 한글 라벨과 긴 브랜치·에이전트 식별자가 섞인 실제 텍스트에서 잘리거나 줄바꿈이 깨지지 않고 읽힌다. | `design/principles.md` rule 12 |

## Technical structure

새 crate 하나가 워크스페이스에 추가된다. 각 런타임의 에이전트 설정 파일(`~/.claude/settings.json`, `~/.codex/hooks.json`)의 위치와 형식, 읽기·파싱·항목 추가·항목 제거, 설치 상태 판정, 진단 결과, 그리고 런타임별 어댑터를 그 crate가 단독으로 소유한다. 사용자의 전역 파일에 쓰는 유일한 코드 경로이며, 소유권 마커로 자기 항목만 식별해 남의 항목을 보존한다. 새 런타임 지원은 이 crate에 어댑터를 추가하는 일로 끝난다. 같은 crate의 진단을 CLI 실행 파일로도 노출한다(`herdr-core/src/bin/` 선례를 따른다).

`herdr-core`는 세 가지를 얻는다. 자식 pane의 탭 배치 전이(생성 시 이동, 시작 시 정리, 실패 시 재시도와 상태 보고), 계보에서 파생되는 소유권과 breadcrumb 경로, 그리고 정체 시계(`state_change_seq`를 처음 관찰한 시각의 기억과 두 임계 판정). 이 상태는 기존 `Mutex<Runtime>`과 기존 스냅샷 경로 안에 들어가고, 새 herdr 요청·이벤트 구독·타이머·주기 작업을 만들지 않는다. subagent 카운트는 새 crate가 pane id 키로 유지하고 코어는 스냅샷의 pane 토큰으로만 읽는다.

`sidebar.rs`가 상태의 단독 소유자라는 기존 구조를 유지하며, 소유권은 네 번째 파생 축으로 그 안에 들어간다. 새 상태 그룹이나 새 상태어는 만들지 않는다. Swift 쪽은 그리기와 입력만 담당하고 herdr나 파일시스템을 직접 호출하지 않는다.

herdr 계약 변경은 없다. pinned 0.8.2의 `pane.report_metadata`, `PaneInfo`, `agent.list`의 `AgentInfo`만 사용하며 fork 수정, 새 소켓 메서드, 스키마 변경이 없다. `report-metadata` 값은 herdr가 display-only로 규정하므로 표시에만 쓰고 권한이나 제어의 근거로 삼지 않는다.

문서 세 곳이 같은 변경에서 갱신된다. `docs/status-model.md`는 상태 축이 셋에서 넷으로 늘고 `Done`의 의미가 계보 루트로 한정되며 미계측 마크가 activity의 Unknown과 다른 뜻을 갖는다는 것을 기록한다. `AGENTS.md`는 Runtime Architecture에 자식 pane의 탭 배치와 숨김 규칙, Repository Layout에 새 crate를 더한다. `DESIGN.md`는 자식 칩 줄, breadcrumb, 소유권 명암, 미계측 마크의 토큰과 컴포넌트 귀속을 더한다. `docs/README.md`의 라우팅 표에 새 crate와 진단의 소유자를 더한다.

## Risks

- **남의 훅을 지우는 것이 이 변경의 최대 위험이다.** 사용자의 `SubagentStart` 배열에는 이미 `oh-my-principle`과 orca의 훅이 들어 있고(D-26), 잘못 쓰면 hide와 무관한 도구가 조용히 망가진다. 경계: 파일에 쓰는 코드 경로가 새 crate 하나로 한정되고, 파싱 실패한 파일에는 절대 쓰지 않으며(B26), 제거는 소유권 마커가 붙은 항목만 대상으로 한다(B29). 기존 항목 보존이 최우선 자동 증명 항목이다.
- **hide가 사용자 동의 없이 전역 설정을 바꾼다.** 사용자가 마찰 없는 자동 설치를 명시적으로 두 번 택했고(D-25) 되돌리기 경로를 B29로 채웠다. 남는 위험: 사용자가 hide를 지우면 훅이 남는다. 이 PRD는 앱 제거 시 정리를 다루지 않는다.
- **자식의 demand를 사람에게 올리지 않는 것이 방치로 이어질 수 있다.** Observer가 자식의 질문을 못 알아채면 사람은 잘 돌고 있다고 믿는다. 경계: hide의 시계가 에이전트의 협조 없이 도는 안전망이며(D-42) 15분에 사람에게 올린다. 남는 위험: 5분/15분이 실제 작업 리듬에 맞는지는 운용 전에 알 수 없다(D-41의 재검토 조건).
- **켤 때 레이아웃을 정리하는 것이 놀라움을 준다.** 앱을 켰더니 화면 배치가 바뀐다. 사용자가 대가를 알고 택했고(D-44) 자식은 죽지 않고 칩으로 되돌아올 수 있다.
- **옮기기 실패가 화면에 이유를 남기지 않는다.** 화면은 쪼개진 채이고 사유는 하단에만 있다. 사용자가 소음을 우선해 명시적으로 택했다(D-45).
- **상태 모델 문서의 계약을 세 군데 바꾼다.** 상태 축 수, `Done`의 범위, 미계측과 Unknown의 구분이다. 문서를 같은 변경에서 갱신하지 않으면 이후 작업이 낡은 계약을 적용한다. ADR 후보로 기록되어 있다.
- **`report-metadata`가 display-only라는 herdr의 규정.** hide가 그 값 위에 제어를 세우면 herdr의 다음 버전에서 깨질 수 있다. 표시 전용 사용으로 한정한다(D-27).
- **미계측이 정상 상태로 남는다.** 훅 설치 이전에 시작된 세션은 재시작해야 계측된다. 사용자가 "설치했는데 왜 안 되지"로 헤매지 않도록 툴팁이 사유를 구분해 말하는 것이 B21의 존재 이유다.
- 열린 결정: 없다. gap-audit 게이트가 낸 10건 모두 사용자 승인으로 닫혔다(D-49~D-57).
- 사용자가 구현 전에 해줄 일: 없다. 계정, 자격증명, 구매, 외부 설정이 필요하지 않다. 구현 후 시각 판정을 위한 스크린샷 확인만 요청한다(D-34).
