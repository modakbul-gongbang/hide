---
topic: "체크아웃마다 Herdr 워크트리 워크스페이스 하나"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "Hide가 만드는 모든 탭이 어느 Herdr 워크스페이스에 생기는지와 기기 체크아웃 id를 스냅샷 wire 전체에서 바꾸므로, 실수하면 운영자의 탭이 엉뚱한 워크스페이스에 생기거나 기기의 열린 상태가 끊긴다."
source_intake: "agents/interview/checkout-workspace-binding/qa-log.md"
created_at: "2026-09-29"
updated_at: "2026-09-29"
---

# PRD: 체크아웃마다 Herdr 워크트리 워크스페이스 하나

## Goal

Hide로 여러 체크아웃에서 에이전트를 돌리는 운영자에게, 체크아웃에서 만든 탭이 항상 그 체크아웃의 Herdr 워크스페이스에 생기고 다른 워크스페이스의 이름이 새어 나오지 않게 한다.
지금은 Hide가 "이 체크아웃의 워크스페이스"를 모르고 보이는 탭이 있는 워크스페이스를 재사용해서, herdr-ide main의 새 탭이 w8P에 생기고 그 라벨 `home-graph`가 탭과 에이전트 이름으로 보인다.
원격에서는 같은 폴더의 Herdr 워크스페이스 두 개가 체크아웃 두 줄(mac mini의 `main` 두 줄)로 보인다.
Herdr가 이미 체크아웃 경로마다 묶어 둔 워크스페이스를 소유자로 읽고, 새 탭은 그 안에만 만들며, 기기 체크아웃도 이 Mac처럼 경로로 식별한다.

## Non-goals

- 이미 다른 워크스페이스에 있는 탭(w8P의 main 탭 등)을 옮기지 않는다(D-08). 결과: 그 탭들은 닫힐 때까지 비소유 탭으로 남는다. 재검토: pane id와 계보를 보존하는 이동 수단이 생기고 운영자가 정리를 원할 때.
- Herdr 워크스페이스 이름을 바꾸지 않는다(D-08, D-09). 결과: Herdr 자체 UI에는 `home-graph` 같은 옛 라벨이 남지만 Hide 화면에는 나오지 않는다. 재검토: 운영자가 Herdr UI 정리를 원할 때.
- sasu implement dispatch와 hcoord agent spawn처럼 Hide 밖에서 일반 워크스페이스를 만드는 도구는 바꾸지 않는다(D-16). 결과: 그 도구가 만든 탭은 cwd로 체크아웃 아래 보이는 비소유 탭이다. 재검토: 이 변경이 배포된 뒤 그 도구들을 worktree.open으로 옮길 때.
- 보호된 닫기의 대체 셸과 위임된 자식의 전용 탭은 원래 워크스페이스에 둔다(D-16). 결과: 그 두 경우는 소유 규칙의 예외다. 재검토: 그 흐름을 다시 설계할 때.
- 모델 A 작업(앞 기기가 아닌 행의 기기 칩, 기기를 넘나드는 Ctrl+Tab·Opt+Tab과 원격 사용 기록)과 기기 사이드바 폴드는 별도 후속 작업이고, 기기 Inactive 폴드는 고르지 않았다(D-17). 결과: 원격 사이드바 폴드와 기기 구분은 지금과 같다. 재검토: 이 PRD가 배포된 뒤 후속 PRD에서.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Herdr 0.9.1은 워크트리 워크스페이스를 체크아웃 하나에 묶는다: `WorkspaceInfo.worktree.checkout_path`는 worktree.create/worktree.open만 설정하고 workspace.create는 설정하지 않는다. worktree.list는 체크아웃마다 `open_workspace_id`를, worktree.open은 `already_open`을 돌려준다. tab.create는 workspace_id·cwd·label을 받고, 워크스페이스 메타데이터 토큰 키는 `^[A-Za-z0-9_-]{1,32}$`, 한 번에 16개까지다. 일반 폴더 바인딩과 거부 코드는 스키마에 없다. | 사실: contracts/herdr-api.schema.json, herdr.dev CLI reference, 이 Mac과 mini의 herdr 0.9.1 |
| D-02 | 지금 로컬은 탭 첫 pane의 cwd로 체크아웃을 정하고 체크아웃 id가 경로 해시라서, 같은 폴더의 여러 워크스페이스는 이미 한 줄이다. 새 탭은 `reusable_session_workspace_id`(보이는 탭의 워크스페이스 우선)로 워크스페이스를 고르고, 없으면 `hide <checkout>` 라벨로 workspace.create 한다. 실제로 main의 새 탭은 묶이지 않은 w8P(`home-graph`)에 생기고, Herdr는 루트를 w9J에 묶어 두었다. | 사실: session.rs:242-271, projects.rs:2408-2461, events.rs:1546-1590, worktree_control.rs:847-903, 2026-09-29 live 조회 |
| D-03 | 에이전트 이름(identity_label)은 task가 없으면 Herdr 워크스페이스 라벨이고, 사이드바·Agents 행, pane 헤더, 팔레트, Recent Panels, 계보 칩, Overview에 쓰인다. 탭 이름은 Herdr 커스텀 라벨, 포커스된 에이전트 이름, 전경 프로세스, `Tab N` 순이다. | 사실: sidebar.rs:1263, model.rs:1132, docs/status-model.md:365, docs/UI_BEHAVIOR.md:54 |
| D-04 | 기기 체크아웃 id는 Herdr 워크스페이스 하나를 가리키고(`remote:<device>:checkout:<workspace>`), focus_workspace·create_tab·purpose는 id를 파싱해 워크스페이스를 찾는다. 그래서 mini의 w5N(묶임)과 w5T(안 묶임), w3Y와 w43이 각각 두 줄로 보인다. | 사실: replica.rs, device_catalog.rs:1-23·126, agents.rs:2095-2116, docs/ARCHITECTURE.md:763·770, 2026-09-29 mini 조회 |
| D-05 | 보호된 primary 닫기는 이미 워크스페이스의 worktree 출처를 읽고 같은 워크스페이스에 대체 셸을 둔 뒤 닫는다. 세션이 없는 체크아웃 열기는 이미 worktree.open을 쓰고, View 영역은 기기와 경로로 저장된다. | 사실: docs/ARCHITECTURE.md:512, projects.rs:700-745, workspace_views.rs:236 |
| D-06 | 체크아웃마다 소유 Herdr 워크스페이스는 하나이며, Herdr가 그 경로에 묶은 워크스페이스다. Hide는 이를 Herdr에서 읽고, 탭이 지금 어디 있는지로 추측하지 않는다. 이 Mac과 모든 SSH 기기에 같은 규칙이다. | 사용자: `/please` 호출이 직전 추천(체크아웃 하나 = Herdr 워크트리 워크스페이스 하나)을 수락 |
| D-07 | Hide가 git 체크아웃에 만드는 탭과 에이전트는 모두 소유 워크스페이스에 생긴다. 소유가 열려 있지 않으면 먼저 그 경로로 worktree.open 해서 하나를 연다(이미 열려 있으면 그것을 쓴다). 보이는 탭의 워크스페이스 재사용과 `hide <checkout>` 일반 워크스페이스 생성은 git 체크아웃에서 없앤다. | 사용자: 수락한 추천 2번 |
| D-08 | 비소유 워크스페이스에 이미 있는 탭은 pane cwd가 가리키는 체크아웃 아래 계속 보이고, 워크스페이스 사이로 옮기지 않으며(pane.move는 pane id를 바꿔 계보·읽음·슬립을 끊는다), 새 탭의 자리로 쓰지 않는다. | 사용자: 수락한 추천 3번 |
| D-09 | Herdr 워크스페이스 라벨은 화면 이름으로 쓰지 않는다. task가 없는 에이전트는 제공자 이름(Claude, Codex, 또는 Herdr가 보고한 종류)으로 부르고, 탭 이름 사다리는 워크스페이스 라벨 대신 그 이름을 쓴다. | 사용자: 수락한 추천 4번 |
| D-10 | 기기 체크아웃은 이 Mac처럼 경로로 식별한다. 한 기기 폴더에 탭이 있는 Herdr 워크스페이스는 모두 체크아웃 한 줄이다. 줄 자체의 명령(새 탭, purpose)은 소유 워크스페이스로 보내고, 탭·pane 명령은 그 탭이 있는 워크스페이스로 보낸다. | 사용자: 수락한 추천("같은 폴더 한 행으로"를 흡수) |
| D-11 | 일반 폴더는 Herdr가 묶지 못하므로, Hide가 그 폴더에서 workspace.create 하고 기기와 경로의 안정 digest를 담은 Hide 전용 메타데이터 토큰으로 표시한다. 소유는 그 표시를 가진 살아 있는 워크스페이스다. 표시가 사라지면 다음 탭이 새 소유를 만들고 표시한다. | 가정: /please 위임 아래 에이전트 결정, 거부 가능. 재검토: Herdr 폴더 바인딩, live handoff 후 토큰 소실 확인 |
| D-12 | primary 체크아웃이 Herdr의 primary 워크트리 워크스페이스에 묶이면, 링크된 워크트리가 열린 동안 마지막 탭을 못 닫는 Herdr 규칙이 그 소유에 적용된다. Hide는 기존 보호된 primary 닫기를 그대로 쓰고 새 화면 상태를 더하지 않는다. | 가정: /please 위임 아래 에이전트 결정, 거부 가능. 재검토: 기존 흐름이 회복하지 못하는 거부 관찰 |
| D-13 | 체크아웃 줄을 열 때(탭 생성이 아닐 때)는 어느 워크스페이스에 있든 그 체크아웃의 가장 최근 활성 탭을 앞으로 가져온다. 탭이 하나도 없는 체크아웃만 지금처럼 소유를 연다. | 가정: /please 위임 아래 에이전트 결정, 거부 가능 |
| D-14 | Hide가 여는 워크스페이스의 Herdr 라벨은 사이드바의 체크아웃 이름이다(링크된 워크트리는 브랜치, primary와 폴더는 프로젝트 이름). Herdr UI에서만 보인다. | 가정: /please 위임 아래 에이전트 결정, 거부 가능 |
| D-15 | 기기 체크아웃 id를 경로 기준으로 바꿔도 기기에서 열려 있던 것은 사라지지 않는다. View 영역은 기기와 경로로 저장되고, 열린 파일은 경로로 다시 찾으며, 기기 폴드는 아직 그리지 않는다. Herdr 워크스페이스가 없는 등록 기기 프로젝트도 같은 소유 규칙으로 연다. | 가정: /please 위임 아래 에이전트 결정, 거부 가능 |
| D-16 | Hide 밖의 도구(sasu dispatch, hcoord spawn)는 바꾸지 않는다. Hide가 직접 만드는 탭(새 탭, 새 에이전트, 에이전트 시작, 닫은 탭 다시 열기)은 모두 소유 규칙을 따르고, 보호된 닫기의 대체 셸과 위임된 자식의 전용 탭은 원래 워크스페이스에 남는다. | 가정: /please 위임 아래 에이전트 결정, 거부 가능 |
| D-17 | 이 PRD는 바인딩, 이름 대체값, 기기 같은 폴더 합치기만 다룬다. 모델 A와 기기 폴드는 후속이고, 기기 Inactive 폴드는 고르지 않았다. | 사용자: "우선 이거 진행해줘" (앞선 선택: 모델 A, 원격 폴드, 원격 최근 기록, 같은 폴더 합치기) |
| D-18 | 전달은 main 대상 GitHub PR이고 CI를 지켜보며, merge는 운영자의 명시적 승인 뒤에만 한다. | 사실: agents/config.json delivery, AGENTS.md |
| D-19 | 원칙 intake(oh-my-principle 654485f 전문): engineering #13(소유를 추측하지 말고 모델링), #7(Herdr 바인딩과 기존 worktree.open 경로 사용), #1(`reusable_session_workspace_id`와 일반 대체 경로 삭제), #4·#10(소유 열기 실패는 기존 실패 경로로, 다른 워크스페이스로 대체하지 않음), #11(반복·경합 요청은 already_open으로 소유 하나에 수렴). design #10(보증할 수 없는 이름을 그리지 않음), #13(새 화면 상태 없이 진단은 로그로). live 증명은 격리된 Herdr에서만 하고, 스냅샷마다 Herdr 호출이나 하위 프로세스를 더하지 않는다. 번역하지 않은 규칙은 없다. | 가정: 원칙 문서 전문 읽음, AGENTS.md Performance Guide |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 로컬 git 체크아웃에서 새 탭 단축키나 New tab here, New agent here, Overview의 에이전트 시작, Reopen Closed Tab으로 만든 탭은 Herdr가 그 체크아웃에 묶은 워크스페이스 안에 생기고 그 체크아웃 줄 아래 보인다. herdr-ide main의 기존 탭이 w8P에 있어도 새 탭은 w9J에 생긴다. | D-06, D-07, D-16 |
| B2 | 묶인 워크스페이스가 열려 있지 않은 체크아웃에 탭을 만들면 Hide가 그 경로로 워크스페이스 하나를 연 뒤 그 안에 탭을 만든다. 같은 체크아웃에 요청을 빠르게 두 번 보내면 워크스페이스는 하나만 생기고 탭은 두 개 생긴다. | D-07, D-19 |
| B3 | 소유 워크스페이스를 열지 못하면(폴더 없음, Herdr 거부, 연결 끊김, 응답 유실) 요청은 기존 탭 생성 실패 알림과 Retry로 끝나고, 다른 워크스페이스에는 아무것도 생기지 않는다. 진단 로그에 체크아웃과 이유가 남는다. | D-07, D-19 |
| B4 | 비소유 워크스페이스에 이미 있는 탭은 pane cwd의 체크아웃 아래 그대로 보이고 선택·닫기·분할이 지금처럼 동작한다. 그 탭의 pane id, 계보, 읽음, 슬립 상태는 유지되고, 그 워크스페이스가 새 탭의 자리로 쓰이지 않으며, 운영자가 닫으면 사라진다. | D-08 |
| B5 | 체크아웃 줄을 열면 어느 워크스페이스에 있든 그 체크아웃의 가장 최근 활성 탭이 앞으로 오고 아무것도 새로 만들지 않는다. 탭이 없는 체크아웃은 지금처럼 소유를 열어 보여준다. | D-13, D-05 |
| B6 | task가 아직 없는 에이전트는 사이드바·Agents 행, pane 헤더, 탭 이름, 팔레트, Recent Panels, 계보 칩, Overview 어디서나 제공자 이름(Claude, Codex, 또는 Herdr가 보고한 종류)으로 보이고, 첫 task가 오면 그 문장으로 바뀐다. Herdr 워크스페이스 라벨(`home-graph`, `web-shell-pivot-s3` 등)은 어디에도 이름으로 나오지 않는다. | D-03, D-09 |
| B7 | 탭 이름은 운영자가 Herdr에서 붙인 이름, 포커스된 pane의 에이전트 이름(task 또는 제공자), 전경 프로세스, `Tab N` 순으로 정해진다. 셸만 있는 새 탭은 프로세스 이름이나 `Tab N`이다. | D-03, D-09 |
| B8 | SSH 기기에서 한 폴더에 Herdr 워크스페이스가 여럿이면 체크아웃 한 줄로 보이고 그 줄이 모든 탭을 담는다. mini의 herdr-ide `main` 두 줄과 modakbul 두 줄은 각각 한 줄이 된다. | D-04, D-10 |
| B9 | 그 기기 줄에서 새 탭을 만들면 그 폴더에 묶인 워크스페이스(mini에서는 w5N)에 생긴다. 묶인 것이 없으면 그 기기의 Herdr에서 하나를 연 뒤 만든다. purpose 설정도 소유로 가고, 탭 포커스·분할·줌·닫기는 그 탭이 있는 워크스페이스로 간다. | D-06, D-07, D-10 |
| B10 | 기기 연결이 끊겼거나 목록을 확인하는 중이면 기존 기기 알림이 그대로 나온다. 기기에서 소유를 열지 못하면 기존 원격 제어 실패 알림으로 끝나고, 그 기기의 다른 워크스페이스에는 아무것도 생기지 않는다. | D-10, D-19 |
| B11 | 업그레이드 뒤에도 기기에서 열어 두었던 View 영역의 파일·Diff·브라우저 표시, 선택된 체크아웃과 탭이 그대로 남는다. 기기 체크아웃 id가 바뀐 것 때문에 사라지거나 다시 열어야 하는 것은 없다. | D-15 |
| B12 | Herdr 워크스페이스가 없는 등록 프로젝트(이 Mac이나 기기)를 열거나 그 줄에서 새 탭을 만들면, git 프로젝트는 그 경로에 묶인 워크스페이스를, 일반 폴더는 Hide가 표시한 워크스페이스를 하나 만들고 그 안에 탭을 연다. | D-07, D-11, D-15 |
| B13 | 일반 폴더 프로젝트는 첫 탭이 그 폴더에 표시된 워크스페이스 하나를 만들고 다음 탭도 거기에 생긴다. 그 워크스페이스가 닫힌 뒤의 다음 탭은 새 워크스페이스를 만들어 표시하고, 다른 워크스페이스의 탭은 cwd로 계속 보인다. | D-11 |
| B14 | Hide가 연 워크스페이스는 Herdr에서 체크아웃 이름(링크된 워크트리는 브랜치, primary와 폴더는 프로젝트 이름)으로 보인다. | D-14 |
| B15 | primary 체크아웃의 마지막 탭을 닫을 때 Herdr의 워크트리 그룹 규칙이 걸려도 기존 보호된 닫기가 같은 워크스페이스에 셸을 남기고 닫는다. 새로운 경고나 화면 상태는 없다. | D-05, D-12 |
| B16 | 스냅샷마다 Herdr 호출이나 하위 프로세스가 늘지 않는다. 소유는 이미 받는 세션 정보로 판단하고, 워크스페이스를 여는 호출은 소유가 없는 체크아웃에 탭을 만들 때만 한 번 일어난다. idle과 탭 전환 비용은 지금과 같다. | D-19 |

## Technical structure

- Core 모델: 로컬과 기기의 체크아웃이 소유 Herdr 워크스페이스 id를 갖는다. git 체크아웃은 세션에 이미 오는 워크스페이스별 worktree 바인딩(`wire.rs` 변환)에서, 일반 폴더는 Hide 메타데이터 토큰에서 얻는다. 탭은 계속 pane cwd로 체크아웃에 배치된다.
- 탭 생성: create_tab, agent start, reopen, 등록 프로젝트 열기가 하나의 "소유 확보 후 tab.create" 경로를 쓴다. 기존 worker에서 runtime mutex 밖으로 worktree.open(또는 일반 폴더의 workspace.create와 토큰 기록)을 부른다. `reusable_session_workspace_id`와 `hide <checkout>` 대체 경로는 삭제한다.
- 기기: replica와 device_catalog가 한 기기의 워크스페이스를 폴더 경로로 묶는다. 기기 체크아웃 id는 기기와 경로에서 만든 id로 바뀌며 더 이상 워크스페이스 하나를 가리키지 않는다. remote_control은 id를 파싱하지 않고 세션에서 소유 워크스페이스나 탭의 워크스페이스를 찾는다.
- 이름: `sidebar.rs`의 identity 사다리에서 워크스페이스 라벨을 빼고 제공자 이름을 넣는다. 탭 이름 사다리도 그 이름을 쓴다.
- Herdr 계약: 기존 메서드(worktree.open, tab.create, workspace.create, workspace.report-metadata)만 쓰고 스키마는 바꾸지 않는다. wire 변환은 `wire.rs`에만 둔다.
- 저장 파일 형식은 바뀌지 않는다. docs/ARCHITECTURE.md(Device catalogs, 소유 규칙), docs/status-model.md(identity 사다리), docs/UI_BEHAVIOR.md(탭 이름)를 같은 변경에서 고친다.

## Risks

- **일반 폴더 표시 토큰이 Herdr 재시작이나 live handoff에서 사라질 수 있다(D-11).** 영향은 다음 탭 생성 때 새 워크스페이스가 하나 더 생기는 데 그친다. 구현 중에 확인하고 결과를 기록한다.
- **worktree.open의 거부 코드가 스키마에 정의돼 있지 않다.** 모든 거부는 B3·B10의 기존 실패 경로로 가고, 다른 워크스페이스로 대체하지 않는다.
- **운영자의 Herdr에 체크아웃마다 워크스페이스가 늘어난다.** Hide는 체크아웃 단위로 보여주므로 화면은 늘지 않는다.
- **기기 체크아웃 id를 바꾸면서 옛 id로 저장된 기기 상태를 놓칠 수 있다.** B11이 경계이고, 구현은 옛 id를 쓰는 곳을 모두 점검한다.
- **live 증명 경계:** 격리된 Herdr 서버에서만 탭이나 워크스페이스를 만든다. 운영자의 Herdr와 mini의 Herdr는 읽기만 하고, `/Applications/hide.app`은 교체하지 않는다.
- 구현 전에 운영자에게 필요한 계정, 자격 증명, 비용은 없다.
