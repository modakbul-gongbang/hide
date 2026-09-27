---
topic: "Agent sleep: 오래 쉰 에이전트 프로세스 재우기"
status: "ready"
human_approval: "approved"  # user 2026-09-27 verbatim: ㅇㅇㅇㅇ /implement ㄱㄱ
review_profile: "high-risk"
review_rationale: "hide가 처음으로 운영자가 띄운 에이전트 프로세스를 스스로 종료하며, 잘못 판단하면 진행 중인 작업이 끊기거나 Herdr가 전달한 프롬프트가 셸 명령으로 실행될 수 있다."
source_intake: "current conversation"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: Agent sleep: 오래 쉰 에이전트 프로세스 재우기

## Goal

Herdr pane에서 에이전트를 여러 개 돌리는 운영자에게, 하루 넘게 손대지 않은 에이전트가 쥐고 있는 메모리를 돌려준다.
설정을 켜면 hide는 정한 시간 동안 아무 변화도 없고 운영자가 보지도 않은 claude·codex 에이전트의 프로세스를 끝내고, pane과 대화는 그대로 두었다가 그 탭을 여는 순간 같은 대화를 이어서 띄운다.
지금 이 Mac에서 claude 18개가 4.6G를 쓰고 그중 10개가 22시간째 idle인 반면, hide 자신의 버퍼와 Herdr 서버는 합쳐 0.14G라서, 줄일 대상은 에이전트 프로세스 자체다.

## Non-goals

- 깨어 있는 에이전트 수의 상한(메모리 상한)은 두지 않는다. 하루 안에 에이전트를 많이 만들면 모두 깨어 있다. 시간 기준으로도 메모리 압박이 계속되면 다시 검토한다.
- 프로세스를 끝내지 않고 멈추는 방식(SIGSTOP)은 쓰지 않는다. 메모리가 돌아오지 않는다. 멈추고도 메모리를 돌려받는 방법이 생기면 다시 검토한다.
- 자동으로 잠든 pane을 열었을 때 버튼을 눌러야 깨우는 방식(B안)은 보류한다. 열면 곧바로 깨우는 방식이 불필요한 깨우기를 반복시키는 것이 관찰되면 다시 검토한다.
- claude·codex 외의 에이전트(opencode 등)는 재우지 않는다. hide에 검증된 resume 명령이 없다. provider별 resume이 검증되면 추가한다.
- SSH 기기의 pane은 재우지 않는다. hide의 resume 경로가 로컬 전용이다. 기기 쪽 resume이 생기면 다시 검토한다.
- 동결된 Swift 앱에는 표시와 설정을 추가하지 않는다. 웹·데스크톱 앱이 재운 에이전트는 Swift 앱에서 셸로 보이고, 웹·데스크톱 앱에서 열어야 깨어난다. Swift 셸이 제거되면 이 비목표는 사라진다.
- 설정을 Never로 바꿔도 이미 잠든 에이전트를 한꺼번에 깨우지 않는다. 열 때 하나씩 깨어난다. 한꺼번에 깨우면 에이전트가 동시에 여러 개 새로 뜬다.
- hide 자신의 터미널 버퍼(최근 5개 탭 attach, 웹 xterm scrollback 0)와 Herdr scrollback 설정은 바꾸지 않는다. 이미 상한이 있다.
- 에이전트별 메모리 사용량(MB)은 표시하지 않는다. core가 만들어내는 숫자가 아니다(design 원칙 10). 측정 경로가 생기면 다시 검토한다.
- Herdr 자체에 hibernate API를 추가하거나 Herdr를 수정하지 않는다. 고정된 upstream Herdr 0.9.1이 제공하는 API만 쓴다.
- Herdr의 동작을 hide가 덮어쓰거나 막지 않는다. Herdr의 재시작 복원을 되돌리지 않고, Herdr의 pane 기록(session ref, 에이전트 권한)과 설정을 지우거나 바꾸지 않는다. Herdr가 hibernate나 종료 표시를 제공하게 되면 hide의 재우기·깨우기를 그쪽으로 옮긴다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 줄일 대상은 에이전트 프로세스다. hide의 attach 창(`ATTACHED_TAB_LIMIT` 5)과 웹 `scrollback: 0` 덕분에 hide 쪽 버퍼는 이미 상한이 있고, Herdr에는 idle hibernate 기능이 없다. | 대화 중 실측: claude 18개 footprint 4.6G, herdr 프로세스 9개 0.14G. `herdr api schema`에 hibernate·suspend 메서드 없음. 사용자: "ㅇㅇ 에이전트 프로세스 !!" |
| D-02 | 잠재우기는 에이전트 프로세스를 끝내고 pane과 session id를 남겨 두었다가, 같은 pane에서 provider의 resume으로 대화를 잇는 것이다. SIGSTOP은 기각한다. | 사용자: "꼭 종료하지 않고라도?"에 대해 SIGSTOP은 메모리를 돌려주지 않고, pty 실험에서 pane의 zsh가 터미널을 되찾는 것을 보여줬다. 종료 후 resume 안에 대해 사용자가 "개수 기반 괜찮은 것 같은데?"로 진행했다. |
| D-03 | 잠재우는 기준은 시간이다(선택지 중 24시간). 깨어 있는 수를 세는 개수 기준은 기각한다. | 사용자: "근데 이미 돌아가는것들이 10개면 10개는 해야하는거아냐? 내가 작업을 오래+많이 시킬수도 잇잖아", "흠~ 그럴거면 시간이 나을것같은데! 24시간 지난거 기준?" |
| D-04 | 시간은 "마지막 상태 변화"와 "운영자가 그 pane을 마지막으로 본 시각" 중 늦은 쪽에서 잰다. 상태 변화는 Herdr가 보고하는 에이전트 상태(demand·activity·completion)가 바뀌거나 에이전트가 새로 나타난 것이며, Herdr 재시작 복원으로 다시 나타난 것도 포함한다. | 기준 시각은 대화에서 제안한 규칙이고 사용자가 "ㅇㅇㅇ gen-prd"로 진행했다. 복원 포함은 D-20에 따름. |
| D-05 | Working 에이전트는 시간과 상관없이 재우지 않는다. Needs You, 아직 안 본 Done, Unknown 활동, 화면에 떠 있는 탭의 에이전트도 재우지 않는다. | Working은 사용자 우려("작업을 오래+많이 시킬수도")에서 나왔다. 나머지는 가정: close 보호 규칙과 같은 기준. |
| D-06 | 대상 provider는 claude와 codex다. Reopen Closed Tab의 resume 인자(claude `--resume <id>`, codex `resume <id>`)를 그대로 쓴다. | 가정. 조사: `herdr-core/src/recent_closed.rs:304-311`. opencode는 저장소에 resume 경로가 없다. |
| D-07 | 이 기기(로컬) pane만 재운다. | 가정. 조사: reopen·fork가 로컬 Herdr 연결만 쓴다(`runtime/editor.rs:2214-2220`). |
| D-08 | 위임된 자식 에이전트도 같은 규칙으로 잠든다. 이 사실은 설정 설명 한 문장으로만 안내한다. | 사용자: "위임된 자식도 재울것이다라고 그냥 안내를 해주면 될듯" |
| D-09 | hide는 잠든 pane에 대한 Herdr의 기록과 판단을 바꾸지 않는다. 에이전트가 끝난 뒤에도 Herdr가 그 pane을 idle 에이전트로 보고하는 동작은 그대로 두고, 그 결과(다른 에이전트가 보낸 프롬프트의 행방)는 구현 때 고정된 Herdr로 먼저 확인한다. | D-20. 조사: Herdr는 에이전트 프로세스가 끝나도 그 pane을 idle로 보고하고 session ref를 유지한다(`src/app/actions.rs:3072`, 로컬 Herdr checkout). |
| D-10 | 설정은 새 Performance 탭에 두고 기본은 꺼짐이다. 컨트롤은 선택 하나 `Sleep idle agents after [Never / 12 hours / 24 hours / 3 days]`이다. | 사용자: "그거 설정에서 켤 수 있게 해도 좋겠다 performance 세팅 같은거에서", "설정은 Performance로 하자 ㅇㅇ 앞으로 더 추가할듯". 선택지 구성은 가정(scratch board v5). |
| D-11 | 잠든 에이전트 행은 상태 마크만 달로 바꾸고 툴팁으로 상태를 말한다. 새 그룹을 만들지 않고 Seen에 남긴다. | 가정(scratch board v5). 사용자: "잠든 agent pane도 뭔가 sleep 되는게 보이면 좋겟는데" |
| D-12 | 잠든 에이전트의 탭을 열면(방문 확정) 곧바로 깨운다(A안). Recent Panels에서 modifier를 누른 채 훑는 프리뷰로는 깨우지 않는다. B안(버튼)은 보류한다. | 사용자: "A안이 나은것같기는 한데!". 프리뷰 제외는 가정: 확정 전 중간 프레임은 방문이 아니라는 기존 규칙(`docs/UI_BEHAVIOR.md` Recent navigation). |
| D-13 | 깨우기가 실패하면 pane에 이유와 Retry, Start new session을 보여준다. Reopen Closed Tab처럼 새 대화를 말없이 시작하지 않는다. | 가정: 잠든 에이전트를 연 운영자는 이전 대화를 기대한다. |
| D-14 | pane 메뉴에 수동 `Sleep agent`를 둔다. 설정이 꺼져 있어도 쓸 수 있다. | 가정(scratch board v5). |
| D-15 | hide를 다시 켜도 잠든 상태는 유지된다. Herdr 서버가 재시작하면서 Herdr가 에이전트를 되살리면 hide는 그것을 깨어난 것으로 받아들이고 다시 재우지 않으며, 시간은 그때부터 다시 잰다. | D-20. 조사: Herdr `resume_agents_on_restore`는 저장된 session ref가 있는 pane을 모두 되살린다(`src/persist/restore.rs:743-793`). |
| D-16 | 동결된 Swift 앱은 새 snapshot 필드를 무시하고 계속 동작해야 한다. 잠든 상태는 기존 group 옆의 선택 필드로 싣고, 새 group이나 transport 값은 만들지 않는다. | 가정. 조사: `waiting_on_descendants`가 같은 방식이다(`docs/status-model.md`). `macos/AGENTS.md`가 Swift 셸을 동결한다. |
| D-17 | 설정 행에 지금 잠든 이 기기 에이전트 수(`8 sleeping`)를 보여준다. 0이면 아무것도 보이지 않는다. | 가정: design 원칙 4(계산된 상태를 보여준다)와 9(가장 작은 형태). |
| D-18 | PR로 전달한다. base `main`, 브랜치 접두사 `gen-prd`, CI 확인을 따르고 worktree에서 구현한다. | `agents/config.json` delivery·worktree 설정. 사용자가 다른 방식을 요청하지 않았다. |
| D-19 | `~/projects/oh-my-principle` commit `654485f`의 engineering·design 원칙 전체와 `engineering/practices/process.md`를 읽고 반영했다. engineering 15(늘어나는 자원의 상한)는 옮기지 않는다: 에이전트는 hide가 작업 단위마다 띄운 자원이 아니라 운영자의 작업이고, 사용자가 개수 상한을 기각했다. process.md 1(단일 spawn helper)도 해당하지 않는다: 에이전트는 Herdr가 pane PTY에서 띄운다. | `sasu principles list --json`. 사용자 결정 D-03. |
| D-20 | Herdr에 맞서는 방어 코드를 넣지 않는다. Herdr가 보고하는 상태를 그대로 믿고, Herdr의 동작을 막거나 기록을 고치지 않는다. Herdr에 기대는 재우기·깨우기 동작은 한 경계에 모아서, Herdr가 나아지면 그 경계만 바꾸면 되게 한다. | 사용자: "음 그러게~ 우선 다 괜찮은데 너무 herdr를 제약을 주면서 하지말자. 나중에 개선되거나 할수도 있는데 애초에 코드를 너무 보수적으로 작성하면 나중에 문제생길 수 있을듯" |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 웹·데스크톱 앱 Settings에 Devices 다음으로 Performance 탭이 있다. 그 안의 Idle agents 그룹에는 `Sleep idle agents after` 선택 하나가 있고(Never, 12 hours, 24 hours, 3 days, 기본 Never), 설명 한 문장이 Working 에이전트·안 본 결과·화면의 탭은 잠들지 않고, 위임된 에이전트도 잠들며, 열면 대화가 이어진다고 말한다. | D-08, D-10 |
| B2 | 값을 바꾸면 재시작 없이 바로 적용되고, hide를 다시 켜도 유지된다. Never로 바꾸면 더 이상 재우지 않지만, 이미 잠든 에이전트는 열 때까지 잠든 채로 남는다. | D-10 |
| B3 | 이 기기에 잠든 에이전트가 있으면 설정 행에 `N sleeping`이 보이고, 없으면 아무것도 보이지 않는다. | D-17 |
| B4 | 시간이 설정되어 있으면, 로컬 claude·codex 에이전트는 마지막 상태 변화와 운영자가 마지막으로 그 pane을 본 시각 중 늦은 쪽에서 그 시간이 지난 뒤 1분 안에 잠든다. 어제 일을 시켰더라도 방금 그 pane을 봤다면 잠들지 않는다. | D-03, D-04 |
| B5 | 다음은 절대 잠들지 않는다: Working 에이전트(얼마나 오래 일하든), Needs You 에이전트, 아직 안 본 Done, Unknown 활동, 화면에 떠 있는 탭의 에이전트, 에이전트가 없는 pane, Herdr가 session id를 보고하지 않은 에이전트, claude·codex가 아닌 에이전트, SSH 기기의 pane. | D-05, D-06, D-07 |
| B6 | 위임된 자식 에이전트도 루트와 같은 규칙으로 잠든다. 자식이 아직 일하고 있어서 기다리는 루트는 Working이라 잠들지 않는다. | D-05, D-08 |
| B7 | 잠들면 그 에이전트 프로세스와, 그 프로세스가 pane의 foreground 작업으로 띄운 자식(MCP 서버 등)이 프로세스 목록에서 사라진다. pane, 그 셸, 탭 안의 위치와 split은 그대로이고, pane의 셸이나 다른 pane의 프로세스는 건드리지 않는다. | D-01, D-02 |
| B8 | 제한 시간 안에 프로세스가 끝나지 않으면 에이전트는 깨어 있는 그대로 쓸 수 있고 달 마크도 붙지 않는다. 실패는 진단 로그에만 남고, 다음 시도는 매 tick이 아니라 제한된 빈도로만 한다. | D-02, D-05 |
| B9 | 재우기와 깨우기는 Herdr의 설정과 pane 기록을 바꾸지 않는다. `resume_agents_on_restore` 설정, pane의 session ref, 에이전트 권한은 재우기 전과 같다. 잠든 자식에게 부모가 보낸 후속 요청에 답하려면 운영자가 그 자식을 열어 깨운다. | D-08, D-09, D-20 |
| B10 | Agents 목록, 사이드바, Project Overview의 에이전트 행에서 잠든 에이전트는 상태 마크 자리에 같은 크기의 달이 그려지고, 툴팁과 접근성 이름이 `Sleeping · resumes when opened`이다. Herdr가 더 이상 에이전트를 돌리지 않아도 그룹(Seen), 제목, provider 마크, 경과 시간, 순서, 자식 배지는 그대로다. Workspace 칩과 그룹 카운트에서는 idle로 센다. | D-11, D-16 |
| B11 | Recent Panels와 ⌘K 검색 행에서도 잠든 에이전트의 이름과 달 마크가 보인다. 행을 확정하면 깨어나고, modifier를 누른 채 훑는 프리뷰로는 깨어나지 않는다. | D-11, D-12 |
| B12 | 탭 방문을 확정하면(클릭, Recent Panels 확정, 검색, 단축키) 그 탭의 잠든 에이전트가 모두 깨어난다. 깨는 동안 pane 헤더에는 `☾ waking…`, 본문에는 `Waking…`과 `Resuming the conversation from <경과>`가 보이고, 입력한 키는 셸로 가지 않는다. 준비가 끝나면 같은 pane에서 같은 대화가 이어진 live 터미널이 보인다. | D-02, D-06, D-12 |
| B13 | 깨는 중에 다시 방문하거나 클릭하거나 Retry를 눌러도 에이전트가 두 개 뜨지 않는다. 재우기 판정이 진행 중인 깨우기와 겹치면 깨우기가 이긴다. 이미 잠든 에이전트를 다시 재우라는 요청은 아무 일도 하지 않는다. | D-02, D-12 |
| B14 | 깨우기가 실패하면(대화를 더 이어갈 수 없음, 작업 폴더가 없음, 제한 시간 안에 에이전트가 준비되지 않음) pane에 `Couldn’t resume this conversation`, 그 이유, Retry, `Start new session`이 보이고, 말없이 새 대화가 시작되지 않는다. Retry는 같은 resume을 다시 하고, Start new session은 같은 provider의 새 에이전트를 그 pane에서 시작한다. 그 pane에 다시 에이전트가 뜰 때까지 행의 달 마크는 남는다. | D-13 |
| B15 | pane 메뉴의 `Sleep agent`는 로컬 claude·codex 에이전트가 Working이 아니고, 요청(demand)이 없고, session id가 있을 때만 활성화된다. 비활성일 때는 툴팁이 이유를 말한다. 설정이 꺼져 있어도 쓸 수 있다. 화면에 떠 있는 pane을 직접 재우면, Wake agent를 누르거나 탭에 다시 들어올 때까지 그 pane 본문에 달, `Sleeping`, 마지막 progress 한 줄, `Wake agent`가 보인다. | D-14, D-05 |
| B16 | hide를 다시 켠 뒤에도 잠든 에이전트는 잠든 것으로 보이고, 열면 대화가 이어진다. Herdr 서버가 재시작하면서 에이전트를 되살리면 그 에이전트는 깨어 있는 것으로 보이고, hide는 다시 재우지 않으며, 시간은 그때부터 다시 잰다. | D-04, D-15, D-20 |
| B17 | 잠든 pane을 닫으면 다른 pane처럼 닫힌다. 그 pane의 잠든 기록과 시각 기록도 함께 사라지고, 무엇도 그것을 되살리지 않는다. | D-15 |
| B18 | 다른 경로로 에이전트가 다시 뜨면(운영자가 pane에 직접 입력, Herdr 복원) Herdr가 보고하는 즉시 깨어 있는 것으로 보이고, 시간도 그때부터 다시 잰다. | D-04, D-20 |
| B19 | 재우기, 깨우기, 각 실패는 pane id, provider, 결과와 이유를 담은 구조화 진단으로 남고, 대화 내용은 담지 않는다. 운영자가 손쓸 수 없는 실패(자동 재우기 실패)는 화면에 나타나지 않는다. | D-13 |
| B20 | 동결된 Swift 앱은 새 snapshot 필드가 있어도 디코딩에 실패하거나 화면이 멈추지 않고 계속 동작한다. | D-16 |

## Technical structure

- **core(herdr-core) 상태**: pane마다 시각 기록 두 가지(마지막 상태 변화 시각, 운영자가 마지막으로 본 시각)와 잠든 기록(provider, session id, 행 표시에 쓸 이름과 마지막 문장, 잠든 시각, sleeping·waking·failed 상태와 이유)을 둔다.
  - `core-state.json`에 `#[serde(default)]` 필드로 저장하므로 schema version은 올리지 않는다.
  - 기록은 read 기록과 같은 규칙으로, pane이 topology에서 빠질 때 지운다.
  - 설정 값은 `ui_state`의 기존 설정 이벤트 방식으로 저장한다.
- **판정**: 로컬 session coordinator가 이미 도는 `tick_async_operations(now_unix_ms)`(250ms)에 붙인다. 판정 자체는 1분에 한 번만 하고, 새 스레드나 타이머는 만들지 않는다. 기기 coordinator에서는 판정하지 않는다.
- **종료**: hide가 자신이 띄우지 않은 프로세스를 끝내는 첫 경로다. `docs/ARCHITECTURE.md`에 이 경계를 적는다.
  - runtime lock 밖의 worker에서 Herdr `pane.process_info`로 foreground 프로세스 그룹을 읽는다.
  - 셸의 그룹이 아닐 때만 종료 신호를 보내고, 셸이 다시 foreground가 된 것을 확인한다.
  - Herdr의 pane 기록과 에이전트 권한은 건드리지 않는다(B9).
  - Herdr 요청과 응답 타입은 `herdr-core/src/wire.rs`에서만 변환한다.
- **깨우기**: Reopen Closed Tab의 resume 인자 생성과 `agent.start` 경로를 쓰되, 새 pane이 아니라 같은 pane에서 시작한다.
- **Herdr 경계**: 종료와 resume 시작처럼 Herdr에 기대는 동작은 한 모듈에 모은다. Herdr가 hibernate나 종료 표시를 제공하면 그 모듈만 바꾼다. 판정 규칙과 화면은 그 모듈 밖에 둔다.
- **snapshot wire**: 에이전트 행과 pane projection에 잠든 상태를 선택 필드로 추가한다. 기존 group·symbol·transport 값은 바꾸지 않는다.
- **웹**: Performance 탭, 달 상태 마크, pane의 waking·sleeping·failed 본문, pane 메뉴 항목을 추가한다.
- **디자인**: `design/hide-screens.pen`의 `Screen / Settings`(`scripts/pen-screens.mjs`의 TABS)에 Performance 탭을 반영한다. 행·pane 시안의 원본은 scratch board `agents/runs/agent-sleep/design/board-v5.pen`이다.
- **문서**: `docs/status-model.md`의 상태 표에 한 행을 추가하고, `docs/UI_BEHAVIOR.md`의 Agent panes·Settings와 `docs/ARCHITECTURE.md`를 갱신한다.
- 새 의존성, Herdr 수정, SSH 기기 경로는 없다.

## Risks

- **idle로 보이지만 일하는 에이전트**: 백그라운드 셸이나 고아가 된 subagent가 있으면 잠들 때 함께 끝난다. 입력창에 보내지 않은 초안도 사라진다.
  - 한계: 기본값이 꺼짐이고, Working은 제외되며, 가장 짧은 선택지도 12시간이다. 대화 자체는 resume으로 돌아온다. 더 길게 둘지(3 days)나 끌지는 운영자가 정한다.
- **Herdr의 재시작 복원**: Herdr는 끝난 에이전트의 pane도 session ref가 있으면 재시작 때 되살린다. hide는 이를 막지 않으므로(D-20), Herdr 재시작 뒤에는 잠들었던 에이전트가 다시 깨어 있고 설정한 시간이 지나야 다시 잠든다. 그동안 메모리가 다시 쓰인다.
- **잠든 pane으로 가는 프롬프트**: Herdr가 끝난 에이전트의 pane을 idle 에이전트로 계속 보고하므로, 부모가 `herdr agent prompt`로 보낸 텍스트가 pane의 셸에 입력될 수 있다. 셸이 첫 단어를 명령으로 실행할 수 있다.
  - 구현 첫 단계에서 격리된 Herdr 서버로 실제 동작을 확인한다.
  - 셸로 들어간다면 방어 코드를 먼저 넣지 않고, 확인 결과를 가지고 사용자와 다시 정한다(담당: 사용자).
- **provider CLI 변경**: resume 인자나 동작이 바뀌면 깨우기가 실패한다. 이때는 B14로 드러나고 말없이 새 대화가 시작되지 않는다.
- **Swift 앱과의 공존 기간**: Swift 앱으로 보는 운영자에게 잠든 에이전트는 셸로 보인다(비목표). Swift 앱은 자기 상태 파일을 따로 쓰므로 그쪽에서는 이 기능이 켜지지 않는다.
- **판정 부하**: tick은 coordinator마다 돈다. 판정은 로컬 coordinator에서 1분에 한 번만 하고, 프로세스 신호와 Herdr 요청은 runtime lock 밖에서 한다.
- **실제 검증의 안전 경계**
  - 격리된 Herdr 서버(`HERDR_SOCKET_PATH`, 전용 HOME, `SHELL=/bin/zsh`)와 provider shim을 쓴다.
  - 실제 claude·codex resume은 새로 만든 일회용 세션으로만 확인한다.
  - 운영자의 기존 에이전트를 끝내거나 resume하지 않는다. 운영자의 앱, 창, Herdr 서버도 건드리지 않는다.
- 구현 전에 사용자가 따로 준비할 것은 없다.
