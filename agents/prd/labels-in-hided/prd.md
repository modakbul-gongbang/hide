---
topic: "에이전트 라벨 생성을 hided로 합치기"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "운영 중인 이 Mac과 device에서 플러그인과 상주 프로세스를 제거하고 상태 폴더를 지우는 설치 이전을 수행하며, device 대화 기록을 SSH로 가져와 이 Mac의 AI 로그인으로 분석한다."
source_intake: "current conversation"
created_at: "2026-10-01"
updated_at: "2026-10-01"
---

# PRD: 에이전트 라벨 생성을 hided로 합치기

## Goal

hide 사용자는 사이드바와 모든 화면에서 각 에이전트가 무슨 일을 하는지(task, progress, expected reply)를 라벨로 본다.
지금은 별도 Herdr 플러그인의 watcher 프로세스가 라벨을 만들어 Herdr 토큰으로 넘기는데, 따로 배포되고 따로 살아서 2026-09-17 프로세스 누수, 2026-09-27 handoff lock 경쟁, 2026-10-01 업데이트 후 옛 watcher가 남아 모든 라벨이 사라진 사고가 모두 이 수명 관리에서 났다.
이 변경 후 라벨은 hided가 직접 만들고 같은 버전의 core가 바로 보여주므로, 앱 업데이트나 Herdr 재시작이 라벨을 지우지 않고 이 Mac과 device의 pane이 같은 방식으로 라벨을 얻는다.
hided는 Herdr 서버와 생애주기를 같이해서 앱 창을 닫아도 라벨이 최신으로 유지되고, 앱을 업데이트하면 다른 빌드의 hided가 남지 않는다.

## Non-goals

- Herdr 자체 TUI의 task 줄, 상태 심볼, 주의 우선 정렬을 더 이상 제공하지 않는다. 사용자 `config.toml`의 `$task` 줄은 빈다. Herdr만으로 라벨이 필요해지면 다시 검토한다.
- 로그인 항목(LaunchAgent)으로 hided를 자동 시작하지 않는다. 재부팅 뒤나 터미널에서 Herdr만 먼저 띄운 경우에는 앱을 열어 hided가 뜰 때부터 라벨이 갱신되고 밀린 턴을 따라잡는다. 앱을 열기 전부터 라벨이 필요해지면 다시 검토한다.
- 오류(`×`) 상태를 새로 만들지 않는다. 손으로 설치하는 native hook 경로가 이 Mac에 설치돼 있지 않아 지금도 나타나지 않는다. StopFailure를 상태로 보고 싶어지면 `hide-agent-hooks` 확장으로 다룬다.
- 자동 요약 끄기와 포커스 pane 라벨 다시 만들기 액션을 제공하지 않는다. hide 안에서 이를 부르는 곳이 없다. 기기별로 AI 비용을 끌 필요가 생기면 Settings 항목으로 다시 검토한다.
- device에 남아 있던 기존 라벨은 가져오지 않는다. device pane은 첫 분석 전까지 provider 이름으로 보인다.
- 라벨 문맥 구성, 프롬프트, 응답 schema, task 유지 규칙, provider와 모델 정책을 바꾸지 않는다.
- Herdr binary, Herdr API, 사용자 `config.toml`, Herdr의 플러그인 config 폴더를 수정하지 않는다.
- 라벨 생성 실패를 알리는 새 배너, 경고창, 알림을 만들지 않는다(design/principles.md 13).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 라벨 생성(대화 기록 읽기, AI 분석, 결과 저장)을 hided core의 runtime 잠금 밖 작업으로 옮기고 `agent-context-labels` 플러그인과 watcher 프로세스를 없앤다. Herdr만 의존하는 독립 플러그인으로 만드는 안은 기각한다. | 사용자: "필요없어 hided로 합치는거 설계해줘", "없어 합치는걸로 PRD 써줘"(앱 없는 라벨, 외부 배포, device 데이터 지역성 중 필요한 것 없음). |
| D-02 | Herdr TUI용 라벨, 상태 심볼, 정렬(`agent.view.set`, `sort_rank`, 아이콘 토큰)은 제공하지 않는다. | 사용자: Herdr만으로 라벨이 필요한지 묻자 "필요없어". |
| D-03 | device pane 라벨은 이 Mac의 hided가 device helper로 대화 기록을 증분으로 읽어 이 Mac의 AI 로그인으로 만든다. device의 플러그인과 watcher도 제거한다. "라벨 없음"과 "device만 플러그인 유지"는 기각한다. | 사용자 선택: "이 Mac에서 생성 (Recommended)". |
| D-04 | 라벨은 Herdr 토큰으로 게시하지 않고 core 상태에 pane과 provider·native session reference 기준으로 저장하며, 화면은 현재 reference와 일치하는 라벨만 쓴다. `label_owner`·`status_owner`·generation 토큰 확인과 `LabelPublicationGuard`는 삭제하되 session-label-isolation PRD의 세션 경계 행동(새 세션, 재사용 pane, A→B→A, 같은 세션 복원)은 유지한다. | 가정: 생성과 소비가 한 프로세스가 되어 토큰 왕복과 프로세스 간 확인이 불필요하다(engineering 1, 2). 세션 경계는 agents/prd/session-label-isolation의 승인된 행동. |
| D-05 | 분석은 지금처럼 한 번에 하나, `hide-ai` router로 하며 Settings의 provider·model 선택을 따른다. 분석 문맥, `context_label` v3 schema, task 유지 규칙, 큰 tool 결과 줄 건너뛰기는 그대로 옮긴다. | 가정: 라벨 품질과 비용을 지금과 같게 유지한다(engineering 7). |
| D-06 | 질문 상태는 분석 결과의 attention에서, 승인 대기(blocked)·실행·완료·대기는 Herdr의 `agent_status`에서 얻는다. 손으로 설치하는 native hook 경로(`agent-hook.sh`, `hook-state.json`)는 삭제한다. | 가정: 조사 결과 이 경로를 설치하는 코드가 없고 이 Mac에도 설치돼 있지 않다. 대화에서 삭제를 설명했으며 사용자가 이후 범위를 바꾸지 않았다. |
| D-07 | 경과시간은 core가 관찰한 상태 변경 시각을 스냅샷에 싣고 web이 공유 1초 시계로 글자만 갱신한다. 경과시간 때문에 스냅샷을 다시 발행하지 않는다. 정렬도 같은 시각을 쓴다. | 사용자: "퍼포먼스나 불필요한 작업같은건 없는거지?"에 대해 이 세 가지를 수용 기준으로 넣자는 제안 뒤 "없어 합치는걸로 PRD 써줘". |
| D-08 | hided를 재시작해도 저장된 읽기 위치, 분석한 턴, 라벨, 상태 변경 시각으로 이어가며 새 턴이 없으면 AI를 부르지 않고 대화 기록을 처음부터 다시 읽지 않는다. 저장은 hided state 폴더의 `labels.json`이며 살아 있는 pane 범위로 제한한다. | D-07과 같은 근거(재시작마다 재분석하는 낭비 방지). |
| D-09 | 대화 기록 읽기는 그 pane의 agent 상태나 session reference가 바뀌었을 때와 밀린 내용이 남았을 때만, 새로 붙은 바이트만 기존 상한(1회 1 MiB, 줄 256 KiB) 안에서 한다. 세션 소유 확인은 reference나 파일 identity가 바뀔 때만 다시 한다. device도 같다. | D-07과 같은 근거. |
| D-10 | 같은 Herdr를 보는 hided가 여럿이어도 라벨 생성은 하나만 한다. 생성하지 않는 hided는 provider 이름을 보여주고 진단을 남긴다. | 가정: dev·e2e는 격리 Herdr를 쓰지만 잘못 붙은 경우 AI 요청이 중복되지 않게 한다(engineering 15). |
| D-11 | 첫 실행 때 이 Mac의 기존 플러그인 상태(`display-state.json`)에서 현재 세션과 소유가 일치하는 라벨, 분석한 턴, 상태 변경 시각만 한 번 가져온다. | 가정: 업그레이드 직후 모든 행이 provider 이름으로 비는 오늘의 증상과 전체 재분석 비용을 막는 이전 경로(engineering 1의 필요한 전환 경로). |
| D-12 | kit은 업그레이드 때 이 Mac과 각 device에서 플러그인 링크, kit 복사본, 플러그인 상태 폴더를 지우고 돌던 watcher를 한 번 종료한다. Herdr 서버, 에이전트 세션, Herdr 플러그인 config 폴더, 사용자 `config.toml`은 건드리지 않는다. | 가정: 옛 watcher가 남으면 AI 요청이 중복된다. kit의 기존 원칙(Herdr 플러그인 config 폴더 불가침). |
| D-13 | 자동 요약 토글과 `refresh-active-pane-task` 액션은 없앤다. | 가정: 조사 결과 hide UI, CLI, daemon 어디에서도 부르지 않는다. |
| D-14 | 함께 맞출 곳: hcoord의 `activity` 토큰 파싱(읽는 곳이 없어 삭제), agent sleep의 라벨 필드, Settings의 kit 구성요소 목록, 가짜 토큰을 쓰던 e2e fixture, 패키징·CI·검증 빌드 대상, 문서. | 읽기 전용 조사(소비자와 참조 목록). |
| D-15 | 구현은 이 Mac pane을 끝까지 먼저, 그다음 device pane 순서로 하되 하나의 PR로 배달한다(`agents/config.json`의 PR 모드, CI 감시). `quick/watcher-reexec` 브랜치는 이 변경으로 대체되어 배포하지 않는다. | 가정: engineering 3의 순서, 저장소 배달 설정. 대화에서 watcher-reexec 미배포를 추천했다. |
| D-16 | engineering/principles.md와 design/principles.md 전체를 읽고 engineering 1·2·3·4·7·8·9·10·12·13·14·15와 design 9·10·13을 적용한다. 새 목록·폼·파괴 UX가 없어 design의 나머지 규칙은 행동으로 옮기지 않는다. | oh-my-principle commit 654485f96b7764c759662d2c3e9e386ebc221cf6. |
| D-18 | hided는 연결된 앱·폰이 없어도 자기 Herdr 서버가 살아 있는 동안 계속 돈다. 10분 idle 종료는 연결된 클라이언트가 없고 Herdr 서버에도 닿지 않는 상태가 이어질 때만 적용한다. Herdr 재시작·live handoff로 서버가 돌아오면 계속 산다. 로컬 Herdr 없이 쓰는 hided와 폰 keep-alive 규칙은 그대로다. | 사용자: "hided가 근데 매번 켜져있게 하는건 어떨까? herdr랑 생애주기를 같이 유지하면 좋을 것 같은 느낌인데?", "ㅇㅇㅇㅇ 그렇게 추가하자". |
| D-19 | 앱이 hided에 연결할 때 떠 있는 hided의 빌드가 자기와 다르면 그 hided를 종료하고 자기 빌드로 새로 띄운다. 같은 빌드면 그대로 붙는다. Herdr 서버, pane, 에이전트 세션은 건드리지 않고, state 폴더가 다른 dev·e2e hided는 교체 대상이 아니다. | D-18과 같은 근거. 조사: `hide connect`가 떠 있는 hided에 빌드 확인 없이 붙어, 지금도 앱 교체 때 옛 hided를 수동으로 종료하고 있다. |
| D-20 | 앱 연결이 없는 hided의 idle CPU·RSS를 운영 규모에 가까운 조건에서 측정해 기록하고, 현재 watcher(75분에 CPU 94초)와 앱이 연결된 hided(약 14%)와 비교한다. 앱 연결이 없는 동안 hided는 화면용 스냅샷을 만들거나 보내지 않는다. | D-18과 같은 근거. 사용자: "퍼포먼스 이슈가잇나? cpu를 옴총먹어?" |
| D-17 | qa-log 없이 대화가 원천이므로 Spec Gate는 원천 문서 부재로 건너뛴다. human_approval은 사용자 검토 전까지 pending이다. | gen-prd 대화 원천 계약. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 이 Mac의 Claude·Codex pane은 첫 분석 뒤 사이드바, pane header, ⌘K, Recent Panels, lineage, Overview, 자동 탭 이름, phone에 지금과 같은 task 제목과 두 번째 줄(progress, expected reply)을 보여준다. | D-01, D-05 |
| B2 | 업그레이드 후 이 Mac과 device 어디에도 `hide-agent-context-labels` 프로세스, Herdr 플러그인 링크, kit 복사본, 플러그인 상태 폴더가 남지 않고, Herdr 서버와 에이전트 세션은 그대로 살아 있다. | D-01, D-12 |
| B3 | 앱을 업데이트하거나 Herdr 서버가 재시작되거나 handoff해도 라벨이 사라지지 않고, 라벨 생성기와 화면의 버전이 달라지는 상태가 생기지 않는다. | D-01, D-04 |
| B4 | 업그레이드 첫 실행에서 기존 라벨이 현재 세션과 일치하는 이 Mac pane은 라벨이 바로 보이고 AI 요청을 하지 않는다. 일치하지 않거나 기록이 없는 pane만 provider 이름으로 시작해 분석 후 채워진다. | D-11, D-04, D-08 |
| B5 | 새 세션, 재사용 pane, provider 변경, A→B→A 전환에서 이전 세션의 문구나 질문 상태가 어떤 화면에도 잠깐이라도 보이지 않는다. 같은 세션의 재연결과 hided 재시작에서는 유효 라벨이 복원된다. | D-04 |
| B6 | 분석이 질문이라고 판단한 agent는 Needs You에 `?`와 expected reply로 나타나고, 그 agent가 다시 일을 시작하면 질문 표시와 expected reply가 사라진다. | D-06 |
| B7 | 승인 대기(blocked), 실행, 완료, 대기 표시와 read·unread, descendant badge, waiting on children, 그룹 의미는 지금과 같다. 라벨이 없는 pane도 Herdr 상태로 올바른 그룹에 있다. | D-06 |
| B8 | 경과시간은 1초·1분·1시간·1일 단위로 지금처럼 바뀌고, 그 갱신 때문에 스냅샷이 다시 발행되거나 사이드바 전체가 다시 그려지지 않는다. | D-07 |
| B9 | 최근 활동 순 정렬은 core가 본 상태 변경 시각으로 지금과 같이 동작하고, hided 재시작 후에도 순서가 유지된다. | D-07, D-08 |
| B10 | 앱을 다시 켜 hided가 재시작돼도 새 턴이 없는 pane은 AI 요청을 하지 않고, 대화 기록은 저장된 읽기 위치 이후만 읽는다. | D-08, D-09 |
| B11 | 앱이 꺼진 동안 쌓인 턴은 다음 실행에서 밀린 내용을 상한 단위로 읽어 따라잡고, 그 pane의 라벨이 최신 턴 기준으로 갱신된다. | D-08, D-09 |
| B12 | 어떤 agent 상태도 바뀌지 않는 동안 라벨 작업은 대화 기록 읽기, 세션 재확인, Herdr 호출, AI 요청, 스냅샷 발행을 하지 않는다. | D-07, D-09 |
| B13 | device(예: mini) pane도 이 Mac pane과 같은 라벨, 두 번째 줄, 질문 상태, 세션 경계를 보여주며 라벨은 이 Mac의 AI 로그인으로 만들어진다. | D-03, D-04 |
| B14 | device 대화 기록은 그 pane 상태가 바뀌었거나 밀린 내용이 있을 때만 새로 붙은 부분을 상한 안에서 가져온다. device 연결이 끊기면 그 device pane 라벨 작업은 멈추고 마지막 유효 라벨을 유지했다가 재연결 후 이어간다. | D-03, D-09 |
| B15 | device helper가 새 대화 기록 읽기를 지원하지 않는 옛 버전이면 device pane은 provider 이름으로 보이고 진단이 남으며, 기존 device kit 재설치 표시와 재설치로 회복한다. | D-03, D-12 |
| B16 | AI provider가 없거나 로그인되지 않았거나 사용량 한도에 걸리면 라벨 없는 pane은 provider 이름과 실제 상태를 그대로 보이고 기존 라벨은 유지된다. 회복되면 대기 시간 뒤 또는 다음 턴에 라벨이 채워지며 새 경고는 나타나지 않는다. | D-05, D-16 |
| B17 | Settings에서 provider나 model을 바꾸면 다음 라벨 분석부터 그 선택을 쓰고, 진행 중인 분석은 끝까지 마친다. | D-05 |
| B18 | 같은 Herdr를 보는 hided가 둘 이상이어도 같은 턴에 대한 AI 요청은 한 번만 일어난다. | D-10 |
| B19 | 스크린샷처럼 한 줄이 상한을 넘는 tool 결과가 들어 있는 세션도 라벨이 계속 갱신된다. 대화 문장 자체가 상한을 넘는 세션은 그 세션 라벨을 보류하고 진단을 남긴다. | D-05, D-09 |
| B20 | 라벨 작업의 실패, 세션 불일치, 읽기 상한 초과는 pane·세션 상관 id와 함께 hided 진단 로그에 남고, 대화 내용, 비밀 값, 개인 경로는 남지 않는다. | D-16 |
| B21 | `labels.json`이 손상되거나 읽을 수 없으면 빈 라벨 상태로 시작해 진단을 남기고 다시 분석으로 회복하며, 잘못된 세션의 라벨을 쓰지 않는다. | D-08, D-04 |
| B22 | hided가 종료되거나 오류로 끝나면 진행 중이던 AI 요청과 provider 프로세스가 남지 않는다. | D-01, D-16 |
| B23 | Settings의 install kit 목록에서 라벨 플러그인 항목이 사라지고, 나머지 구성요소의 상태 확인, 설치, 재설치, device 제거는 그대로 동작한다. | D-12, D-14 |
| B24 | 앱 창을 닫고 폰 연결 없이 10분이 넘게 지나도 Herdr 서버가 살아 있으면 hided가 계속 돌아 라벨이 갱신되고, 앱을 다시 열면 따라잡기 없이 최신 라벨이 바로 보인다. | D-18 |
| B25 | Herdr 서버가 종료되어 돌아오지 않고 연결된 앱·폰도 없으면 hided가 10분 뒤 종료되며 AI 요청과 provider 프로세스가 남지 않는다. Herdr 재시작·live handoff처럼 서버가 돌아오면 hided는 종료되지 않는다. | D-18, D-01 |
| B26 | 새 빌드의 앱을 켜면 떠 있던 다른 빌드의 hided가 교체되어 화면과 데몬이 같은 빌드가 되고, Herdr 서버, pane, 에이전트 세션, 터미널 내용, 라벨은 그대로 이어진다. | D-19, D-08 |
| B27 | 같은 빌드의 앱을 다시 켜거나 창을 새로 열면 hided를 재시작하지 않고 그대로 붙는다. | D-19 |
| B28 | 다른 빌드의 hided가 종료되지 않거나 새 hided가 응답하지 않으면 앱은 그 옛 hided에 조용히 붙지 않고 기존 시작 실패 화면으로 알리며, 원인은 진단에 남는다. | D-19 |
| B29 | 앱 연결이 없는 동안 hided는 라벨 갱신에 필요한 일만 하고 화면용 스냅샷을 만들거나 보내지 않는다. | D-20, D-07 |
| B30 | 저장소 문서와 설치 안내가 라벨의 소유자를 hided core로 설명하고, 플러그인 설치·재시작·액션 안내와 Herdr 토큰 라벨 계약은 사라진다. | D-01, D-14 |

## Technical structure

herdr-core 안에 라벨 작업 모듈을 둔다.
대상 pane과 상태 변화는 세션 동기화 coordinator가 이미 받는 Herdr 상태(로컬 runtime과 device replica)에서 얻고, 대화 기록 읽기와 AI 분석은 runtime 잠금 밖 스레드에서 하며 결과만 잠금 안에서 짧게 반영한다.
로컬 대화 기록은 `hide-session`으로 읽고, device는 `hide-host-helper`에 세션 찾기와 증분 읽기 호출을 추가해 protocol 버전을 올린다.
라벨, 분석한 턴, 읽기 위치, 상태 변경 시각은 hided state 폴더의 `labels.json`에 잠금 밖 원자적 쓰기로 저장한다.
스냅샷의 제목, 두 번째 줄, 질문 상태는 core 라벨 저장소에서 계산하고, 경과시간 문자열 대신 상태 변경 시각을 싣는다. snapshot wire 계약이 바뀐다.
라벨 생성자는 같은 Herdr당 하나로 잠금 파일로 제한한다.
hided의 idle 종료 조건에 로컬 Herdr 서버 도달 여부를 더하고, 건강 확인 응답에 빌드 식별을 실어 `hide connect`가 다른 빌드의 hided를 교체한다.
`plugins/agent-context-labels` crate, kit의 labels 구성요소(로컬·device), 앱 번들 항목, 검증 빌드 대상, core의 토큰 확인 코드를 삭제하고, 기존 상태 가져오기와 설치 정리를 한 번 수행하는 이전 경로를 둔다.
구현 순서는 이 Mac pane을 끝까지, 그다음 device pane이다.

## Risks

- crate 삭제, core 신규 작업, helper protocol, kit 이전이 한 번에 바뀐다. session-label-isolation의 회귀 테스트를 core 기준으로 옮겨 세션 경계를 계속 지킨다.
- device 대화 기록이 SSH로 이 Mac에 들어온다. 메모리에서만 쓰고 저장하지 않으며, AI 요청 문맥은 기존 마스킹(비밀, 이메일, 경로)을 그대로 거친다.
- device 라벨 몫만큼 이 Mac의 AI 사용량이 늘어난다. 전체 요청 수는 지금과 같다.
- 업그레이드 때 옛 watcher 종료가 실패하면 두 생성자가 잠시 공존해 AI 요청이 중복될 수 있다. kit 결과에 종료 여부를 남기고 다음 kit 확인에서 다시 정리한다.
- hided가 상시 상주하게 된다. 앱 연결이 없는 hided의 CPU·RSS는 아직 측정한 적이 없으므로 D-20 측정 결과가 현재 watcher보다 크게 나쁘면 구현을 멈추고 원인을 보고한다.
- 빌드 교체 때 WebSocket 연결과 터미널 attach가 잠깐 끊긴다. desktop host의 기존 재연결로 회복하는지 격리 환경에서 확인한다.
- e2e는 가짜 토큰 대신 합성 대화 기록과 결정적 provider로 라벨을 만들어야 한다. 운영 앱, 운영 Herdr 서버, 사용자 로그인은 쓰지 않고 격리 HOME·Herdr에서 확인한다.
- 이 Mac과 mini의 실제 설치 이전은 사용자가 요청한 세션에서만 수행한다. idle CPU와 스냅샷 발행 수를 현재 watcher(75분에 CPU 94초, 경과시간 토큰으로 초당 재발행)와 같은 조건에서 비교해 기록한다.
- 사용자가 구현 전에 해야 할 일은 없다.
