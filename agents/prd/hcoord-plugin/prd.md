---
topic: "hcoord를 hide 저장소의 Herdr 플러그인으로 옮기고 부모 관계를 pane 토큰으로 기록"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "앱 설치만으로 로그인 때마다 뜨는 사용자 LaunchAgent를 등록하고, 에이전트에게 입력을 보내는 조율 데몬의 실행 파일과 소유 저장소를 바꾸며, Herdr pane에 남는 공용 토큰 규격을 새로 공개한다."
source_intake: "current conversation"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: hcoord를 hide 저장소의 Herdr 플러그인으로 옮기고 부모 관계를 pane 토큰으로 기록

## Goal

hide의 단일 사용자이자 개발자는 main checkout의 에이전트(Observer)에게 일을 맡기고, 그 에이전트가 worktree에 자식 에이전트를 띄운다.
지금은 "누가 누구를 띄웠나"가 hide가 읽는 Herdr `parent_pane` 토큰과 hcoord ledger 두 곳에 따로 적혀 서로 빠진 것이 있다 (2026-09-27 관찰: 22시간 된 impl-* 네 개는 hcoord만 부모를 알고, sidepanel 세 개는 토큰만 있다).
이 변경 후에는 hcoord가 부모 관계를 기록하는 표준 경로가 되어 자식 pane에 토큰을 남기고, hcoord가 없어도 Herdr를 쓰는 누구나 그 토큰을 읽는다.
hcoord는 sasu에서 나와 hide 저장소 `plugins/hcoord/`의 독립 패키지이자 Herdr 플러그인이 되고, Electron hide 앱을 설치하면 앱 안의 Node로 로그인 때마다 자동 실행된다.
사용자의 말: "결국 herdr 에서 hcoord 없어도 정보들은 잘 활용할 수 있게 해야돼", "hide 앱에서는 무조건 이 plugin이 설치된채로 진행되게 하긴 해야돼".

## Non-goals

- hide 화면은 바꾸지 않는다: Observer 행 둘째 줄의 자식 worktree 칩, Fork 버튼의 hcoord 경유, SessionStart 위임 안내는 다음 PRD다. 이 PRD 머지 후에도 사이드바 모양은 같고, 토큰이 채워진 자식만 기존 규칙대로 부모 밑에 선다. 재검토: 다음 PRD(hide 소비) 착수 시 (D-17).
- watch 담당자나 hcoord 참여자 이름은 토큰으로 기록하지 않는다. Observer 인계 뒤에도 자식은 만든 부모 밑에 보인다. 재검토: 다음 PRD에서 "맡고 있다"의 기준(생성 대 watch)을 정할 때 (D-07).
- sasu 저장소의 hcoord 코드 삭제와 sasu의 설치된 hcoord 호출 전환은 sasu 저장소의 별도 변경이다. 그때까지 sasu 사본은 동결되고 토큰을 쓰지 않는다. 재검토: 이 PRD 머지 직후 sasu에서 (D-15).
- npm 레지스트리 게시는 하지 않는다. hide 없이 쓰는 사람은 Herdr 플러그인 설치나 checkout에서 설치한다. 재검토: 사용자가 게시 계정과 이름을 정할 때 (D-13).
- hide가 다른 기기의 부모를 찾아 그 밑에 자식을 그리는 일은 다음 PRD다: 지금 hide 계보는 기기를 구분하지 않고 pane id로만 부모를 찾는다(`sidebar.rs::apply_lineage`). 이 PRD 뒤에도 원격 자식은 hide에서 잠시 부모 없는 행으로 보이지만, 필요한 토큰은 이 PRD부터 기록된다. 재검토: 다음 PRD 착수 시 (D-08, D-17).
- macOS 앱 소속 서비스 등록(SMAppService, Electron `agentService`)은 쓰지 않는다: ad-hoc 서명 앱에서 동작이 확인되지 않았고 공증은 선행 PRD도 제외했다. 로그인 항목에 "hide"로 보이지 않고, 앱을 지우면 LaunchAgent가 남아 README의 제거 명령으로 지운다. 재검토: 공증 배포가 목표가 될 때 (D-10).
- Swift 앱에는 hcoord를 넣지 않는다: 선행 PRD가 Swift 셸을 지우며, 그 전까지는 지금처럼 따로 설치한 hcoord를 쓴다 (D-06).
- hcoord의 명령, 옵션, 출력, ledger 형식, 원격 프로토콜, 상한(MAX_*)은 이 PRD가 명시한 것 외에 바꾸지 않고 새 조율 기능도 더하지 않는다. Windows 지원은 지금처럼 없다 (D-15).
- hide core와 hided는 hcoord ledger나 소켓을 읽지 않는다 (D-01).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 부모 관계를 기록하는 표준 경로는 hcoord 하나이고 기록은 자식 Herdr pane의 토큰에 남는다. hide는 토큰만 읽고 hcoord ledger를 직접 읽지 않는다. hide가 ledger를 계보 원천으로 읽는 안은 기각. | 사용자: "토큰 기록을 ㅇㅇ 하게 해야지. 결국 herdr 에서 hcoord 없어도 정보들은 잘 활용할 수 있게 해야돼", "너무 의존적이지 않으면서 가게하려고" |
| D-02 | hcoord는 sasu에서 나와 별도 패키지가 되고 Herdr 플러그인으로 설치된다. | 사용자: "hcoord는 별도 패키지로 빼긴 해야돼.. 그래서 herdr 쓰는 쪽에서 할 수 있게 하는게 맞는것같기는 해 Plugin으로 하자" |
| D-03 | 위치는 hide 저장소 `plugins/hcoord/`다(agent-context-labels와 같은 방식, 단독 설치 가능). 새 저장소와 hided·herdr-core 흡수(hided는 마지막 클라이언트 10분 뒤 종료, `docs/ARCHITECTURE.md:305`)는 기각. | 사용자: "안1로 가면 좋지..ㅇㅇ" |
| D-04 | hide 앱은 hcoord가 설치되고 데몬이 도는 상태를 스스로 만든다. | 사용자: "hide 앱에서는 무조건 이 plugin이 설치된채로 진행되게 하긴 해야돼" |
| D-05 | 앱 안의 hcoord는 Electron hide 앱에 이미 있는 Node로 돈다(`ELECTRON_RUN_AS_NODE=1`). 사용자 설치 Node(경로가 박혀 nvm 제거 시 중단, 없는 사용자 있음), Node 따로 번들(+112MB), bun 단일 실행 파일(2026-09-27 실측: `daemon stop` 후 10초 넘게 살아 있고 SIGTERM 3초 무시)은 기각. 같은 날 Electron 44 앱(Node 24.21)에서 데몬 시작·응답·즉시 종료를 확인했다. | 사용자: "그 설치된 node로 하게 하면안돼? 만약 넣으면 아예 새로설치해서 용량을 더 먹고" → 답변 뒤 "ㅇㅇㅇ 그렇게 해보자~" |
| D-06 | 구현은 `agents/prd/electron-app-swift-removal/prd.md`(Electron이 유일한 배포 앱, Resources에 hided·hide·herdr 내장, ad-hoc 서명, Herdr pin `contracts/herdr-bundle.json`)가 main에 머지된 뒤 그 위에서 한다. Swift 앱 기간에는 따로 설치한 hcoord를 그대로 쓴다. | 위 D-05 답변의 수용; 저장소 사실: 그 PRD의 D-03, D-06, D-08 |
| D-07 | 토큰 규격: hcoord가 자식을 spawn하거나 `--parent`로 register하면 자식 pane에 source `hcoord`로 `parent_pane=<부모 pane id>`를 쓴다. 토큰 이름은 hide의 기존 계약 그대로이고, 참여자가 떠나도(`agent end`) 지우지 않으며 pane과 함께 사라진다. 규격 문서는 `plugins/hcoord`가 소유하고 hide 문서는 인용한다. | 가정: 되돌릴 수 있음; 저장소 사실 `docs/status-model.md` "Where a parent comes from", Herdr CLI 문서(토큰은 source별, 재시작·live handoff 후에도 유지) |
| D-08 | 다른 기기의 자식도 부모 밑에 보이게 한다. 부모와 자식이 다른 기기(다른 Herdr 서버)면 hcoord가 자식 기기의 Herdr에 `parent_pane`과 함께 부모 기기의 고유 id `parent_machine=<machine id>`를 쓴다. machine id는 macOS의 IOPlatformUUID, Linux의 `/etc/machine-id`이며, 누구나 hcoord 없이 같은 값을 구할 수 있게 규격 문서가 계산법을 정한다. 같은 기기면 `parent_machine`을 쓰지 않는다. 호스트 이름(네트워크마다 바뀜)과 Herdr 저장 기기 이름(hide 기기 목록과 이름공간이 다름)은 기각. 처음 안(다른 기기면 기록하지 않음)은 사용자 요청으로 기각. | 사용자: "부모 밑에 다른 기기 자식은  안보여? ㅠㅠ 보이게 할 수는 없나!"; 식별자 선택은 가정 |
| D-09 | 토큰 기록 실패는 에이전트 시작을 되돌리지 않고, 명령 결과와 ledger event에 따로 보고한다. 같은 intent 재실행은 에이전트를 다시 띄우지 않고 토큰만 다시 쓴다. 데몬 시작 때 ledger에 부모가 있는 살아 있는 참여자 pane(다른 기기 포함)에 토큰이 없으면 채운다. | 가정: engineering 규칙 10·11; sasu `implement/herdr.ts` spawnImplementor의 비치명 기록 선례 |
| D-10 | 자동 실행은 hcoord의 기존 사용자 LaunchAgent(`com.hcoord.daemon`, RunAtLoad, 비정상 종료만 재시작, 수동 정지 표시)를 쓴다. 패키지된 앱은 열릴 때마다 그 LaunchAgent를 앱 실행 파일 + `ELECTRON_RUN_AS_NODE=1` + 번들 CLI로 맞춘다. 응답하는 데몬이 같거나 새 버전이면 건드리지 않고, 없거나 오래됐거나 버전을 말하지 않으면 교체하며, 수동 정지 표시가 있으면 켜지 않는다. SMAppService는 기각(Non-goals). | 가정; 저장소 사실 sasu `cli/src/hcoord/platform.ts` LaunchAgent, 현재 live plist가 nvm node + sasu dist를 가리킴 |
| D-11 | `hcoord daemon status --json`이 데몬의 hcoord 버전과 로컬 API 버전을 보고한다(D-10의 비교 근거). | 가정; 저장소 사실: 지금 status는 원격 protocol만 보고 |
| D-12 | 앱은 `~/.hcoord/bin/hcoord`(hcoord가 원격 호출에 이미 쓰는 고정 경로)를 앱 런타임으로 번들 CLI를 실행하는 스크립트로 맞춘다. PATH는 바꾸지 않는다. | 가정; 저장소 사실 sasu `scripts/install-local-skills.mjs:208-224` |
| D-13 | hide 없이 쓰는 사람은 `herdr plugin install <owner>/<repo>/plugins/hcoord`(build가 사용자 Node로 빌드, `[[startup]]`은 데몬을 확인하고 끝나는 한 번짜리 명령) 또는 checkout의 `herdr plugin link`로 설치한다. 원격 기기의 `~/.hcoord/bin/hcoord` 설치 명령은 hcoord 패키지가 제공한다. npm 게시는 Non-goals. | 가정; Herdr v0.9.1 plugins.mdx(startup은 감독되는 데몬이 아님) |
| D-14 | Herdr 호출: `HERDR_BIN_PATH`가 있으면 그 바이너리를 쓰고(앱은 번들 herdr를 넘김), 없으면 PATH의 `herdr`. 데몬 환경에 `HERDR_SOCKET_PATH`가 있으면 로컬 기본 서버는 그 소켓이다. | 가정; 관찰 2026-09-27: 격리 소켓을 줘도 hostScope "default"가 변수를 지워 라이브 pane을 읽음 |
| D-15 | 옮기는 범위: `cli/src/hcoord/*`, hcoord 단위·e2e 테스트와 helper, `docs/hcoord.md`, channel adapter 예제. hcoord가 쓰던 sasu 코드(Herdr 어댑터 일부, launchd 수렴)는 hcoord 안으로 복사하고 sasu에는 sasu가 쓰는 것만 남긴다. `sasu-on-hcoord` e2e는 sasu에 남는다. | 가정; 조사: `platform.ts`·`herdr.ts`·`remote.ts`가 `implement/herdr`·`support/launchd`를 import |
| D-16 | 검증: `plugins/hcoord`를 pnpm workspace에 넣고 typecheck·단위·e2e를 저장소 검증 명령과 CI의 필수 lane에서 돌린다. 지금은 어떤 lane도 `plugins/` 아래 TS를 보지 않는다. | 가정; 저장소 사실 `pnpm-workspace.yaml`, `.github/workflows/pr.yml` |
| D-17 | 이 PRD는 두 PRD 중 첫째다(hcoord 이전·토큰 기록·앱 번들과 자동 실행). hide 화면·Fork·SessionStart 안내, 그리고 hide가 (기기, pane id)로 부모를 찾아 다른 기기의 자식을 부모 밑에 그리고 기기 간 같은 pane id가 섞이지 않게 하는 일은 둘째 PRD다. | 대화: "PRD 1: hcoord를 옮기고, pane에 기록하게 하고, hide 앱에 넣기" 안내 뒤 사용자 "ㅇㅇㅇ 그렇게 해보자~" |
| D-18 | 배포는 `agents/config.json` mode pr(브랜치 접두 gen-prd, CI 감시)로 PR을 열고, 머지는 사용자 확인 뒤에 한다. 자동 머지 권한은 이 PRD에 주어지지 않았다. | 가정; `agents/config.json` |
| D-19 | 원칙 intake: oh-my-principle 654485f의 `engineering/principles.md` 전체와 `engineering/practices/process.md`를 읽었다. 규칙 14·process 실천은 데몬 소유자를 launchd로 두는 D-10과 B13·B14에, 규칙 4·10은 B5·B12·B15에, 규칙 11은 B5에, 규칙 13은 호스트 이름 대신 기기 고유 id를 쓰는 D-08에, 규칙 15는 기존 상한 유지(Non-goals)에, 규칙 1은 sasu 사본 제거를 전환 경로로 둔 Non-goals에 반영됐다. design/principles.md는 새 화면이 없어 번역하지 않았다. | 가정 |
| D-20 | `HCOORD_HOME`이 설정되면 LaunchAgent label과 plist 경로가 그 데이터 폴더에 묶인 별도 이름이 되어, 기본 설치의 `com.hcoord.daemon`과 겹치지 않는다(격리 검증과 나란한 설치를 위해). | 가정; 저장소 사실: 지금 label은 `com.hcoord.daemon` 고정(`platform.ts:9`) |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | `plugins/hcoord`에서 빌드한 `hcoord`가 sasu 판과 같은 명령·옵션·JSON 모양으로 동작하고(status, agent register·spawn·list·show·end, watch, request, inbox, graph, events, daemon start·stop·status), 지금 `~/.hcoord`의 ledger·outbox를 그대로 읽어 기존 참여자·watch·요청이 이어진다. | D-02, D-03, D-15 |
| B2 | `hcoord agent spawn --parent <P>`로 같은 서버에 자식을 띄우면(worktree spawn 포함) 자식 pane의 토큰에 source `hcoord`로 `parent_pane=<P의 pane id>`가 있고, hide 사이드바는 기존 규칙대로 그 자식을 P 밑에 둔다. | D-01, D-07 |
| B3 | 이미 떠 있는 pane을 `hcoord agent register --parent <P>`로 등록하면 같은 토큰이 쓰이고, 같은 pane·세션을 다시 등록해도 토큰이 같은 값으로 수렴한다. | D-07, D-09 |
| B4 | 이 Mac의 부모가 `--machine <원격>`으로 원격 기기에 자식을 띄우거나 등록하면, 원격 기기의 Herdr에서 자식 pane의 토큰에 `parent_pane=<부모 pane id>`와 `parent_machine=<이 Mac의 machine id>`가 있다. 같은 기기의 자식에는 `parent_machine`이 없다. 원격 토큰 쓰기가 실패하면 B5와 같이 보고된다. | D-08, D-09 |
| B5 | 토큰 쓰기가 Herdr 거부나 무응답으로 실패하면 에이전트는 떠 있고, 명령 결과가 토큰 기록 실패와 다음 행동(같은 intent로 재실행)을 말하고, ledger event로 남는다. 같은 intent로 재실행하면 새 pane이나 새 에이전트 없이 토큰만 쓰인다. | D-09 |
| B6 | 데몬이 시작되면 ledger에 부모가 기록된, 살아 있는 참여자 pane(원격 기기 포함, 연결된 기기만) 중 토큰이 없는 것에 토큰이 채워지고, 채운 수와 실패 수가 로그 event로 남는다. | D-09 |
| B7 | `agent end`로 참여자가 떠나도 자식 pane의 `parent_pane`은 남고, pane이 닫히면 Herdr와 함께 사라진다. | D-07 |
| B8 | hcoord가 설치되지 않은 머신에서도 `herdr agent list`의 토큰에서 부모 관계를 읽을 수 있고, `plugins/hcoord`의 토큰 규격 문서가 이름·값·작성자·기기 조건·수명과 machine id 계산법을 정하며, `hcoord daemon status --json`이 자기 기기의 machine id를 보이고, hide의 `docs/status-model.md`와 `docs/ARCHITECTURE.md`는 그 문서를 인용한다. | D-01, D-07 |
| B9 | `hcoord daemon status --json`이 데몬의 hcoord 버전과 로컬 API 버전을 보이고, 데몬이 꺼져 있으면 지금처럼 오래된 상태임을 표시해 보인다. | D-11 |
| B10 | 격리 HOME에서 패키지된 hide.app을 처음 열면, 시스템 Node 없이 `com.hcoord.daemon` LaunchAgent가 앱 실행 파일 + `ELECTRON_RUN_AS_NODE=1` + 번들 CLI를 가리키게 되고 `~/.hcoord/bin/hcoord status`가 응답한다. | D-04, D-05, D-10, D-12 |
| B11 | 같거나 새 버전의 데몬이 이미 응답하면 앱을 열어도 LaunchAgent와 데몬이 바뀌지 않는다. 데몬이 없거나, 오래됐거나, 버전을 말하지 않으면(지금의 sasu 데몬) 앱을 열 때 앱 쪽으로 교체되고, 교체 전 outbox의 letter는 잃지 않는다. | D-10, D-11 |
| B12 | 사용자가 `hcoord daemon stop`을 했으면 앱을 다시 열어도 데몬이 켜지지 않고, 그 사실이 hide 호스트 로그에 한 번 남는다. `hcoord daemon start`가 표시를 지우고 다시 켠다. 앱의 hcoord 준비가 실패해도 hide 창과 기존 기능은 그대로이고 실패는 로그에만 남는다. | D-04, D-10 |
| B13 | 로그인 뒤 앱을 열지 않아도 데몬이 뜨고, 데몬을 강제 종료하면 launchd가 다시 띄우며, `hcoord daemon stop`에는 바로 종료한다. | D-05, D-10 |
| B14 | 앱을 다른 위치로 옮기거나 새 버전으로 바꾼 뒤 열면 LaunchAgent와 `~/.hcoord/bin/hcoord`가 새 경로로 맞춰지고 데몬이 새 버전으로 다시 뜬다. | D-10, D-12 |
| B15 | `pnpm --dir desktop package` 결과 hide.app Resources에 빌드된 hcoord가 있다. 빌드 결과물이 없거나, Electron `RunAsNode` fuse가 꺼져 번들 hcoord를 실행할 수 없으면 패키징이 그 이유를 이름으로 말하며 실패하고 앱을 만들지 않는다. | D-04, D-05, D-06 |
| B16 | 번들된 hcoord는 앱이 넘긴 pinned herdr로 Herdr를 부르고, 데몬 환경의 `HERDR_SOCKET_PATH`를 로컬 기본 서버로 쓴다: 격리 소켓을 주면 라이브 서버의 pane을 읽지 않는다. | D-14 |
| B17 | hide 없이 Node가 있는 머신에서 `herdr plugin install <owner>/<repo>/plugins/hcoord`를 하면 build가 끝난 뒤 `hcoord`를 쓸 수 있고, Herdr가 시작할 때 `[[startup]]`이 데몬을 확인하고 바로 끝난다. Node가 없으면 build가 그 이유를 말하며 실패하고 플러그인이 등록되지 않는다. checkout의 `herdr plugin link`도 README대로 동작한다. | D-02, D-13 |
| B18 | 원격 기기에서 hcoord 패키지의 설치 명령으로 `~/.hcoord/bin/hcoord`를 만들면 HQ가 지금처럼 그 기기에 register·worktree spawn·letter 수거를 한다. | D-13, D-15 |
| B19 | hcoord의 typecheck·단위·e2e가 저장소 검증 명령과 CI 필수 lane에서 돌고, 실패하면 PR이 막힌다. | D-16 |
| B20 | `plugins/hcoord/README.md`가 세 설치 경로(hide 앱, Herdr 플러그인, checkout link), 데몬 시작·정지, 원격 설치, 앱을 지운 뒤 LaunchAgent 제거 명령을 설명하고, hide의 `docs/README.md` 소유 표와 `AGENTS.md` Repository Layout에 `plugins/hcoord`가 있다. | D-03, D-10, D-13 |
| B21 | `HCOORD_HOME`을 설정하고 `hcoord daemon start`를 하면 그 폴더 전용 label의 LaunchAgent가 생기고, 기본 `com.hcoord.daemon`과 `~/.hcoord`는 바뀌지 않는다. | D-20 |

## Technical structure

- 새 패키지 `plugins/hcoord/`(TypeScript, pnpm workspace 구성원, `herdr-plugin.toml`, 명령 `hcoord`): sasu `cli/src/hcoord/*`와 그것이 쓰던 Herdr 어댑터·launchd 수렴 코드를 옮기고 복사한다. 새 파일은 토큰 쓰기 한 곳뿐이며 Herdr `pane.report_metadata`(source `hcoord`)를 부른다.
- 새 공용 규격: `plugins/hcoord`의 토큰 규격 문서. hide의 `wire.rs::lineage_parent`와 사이드바 계보는 바뀌지 않는다.
- desktop 패키징: Resources에 빌드된 hcoord를 넣고 `RunAsNode` fuse를 확인한다. desktop main이 앱을 열 때마다 hided와 독립적으로 LaunchAgent와 `~/.hcoord/bin/hcoord`를 맞춘다.
- 새 외부 효과: 사용자 LaunchAgent `~/Library/LaunchAgents/com.hcoord.daemon.plist`의 소유가 앱으로 넘어오고, `~/.hcoord/bin/hcoord`가 앱 런타임을 가리킨다. Herdr pane 토큰(source `hcoord`) 쓰기가 생긴다.
- CI: `plugins/hcoord`의 검증이 필수 lane에 더해진다.
- 바뀌지 않음: hide core·wire·sidebar, hided의 WS/HTTP 계약과 수명, 웹 셸 화면, hcoord ledger 형식과 원격 프로토콜.

## Risks

- 선행 PRD(electron-app-swift-removal)가 머지되기 전에는 착수할 수 없다. 그 PRD의 Resources 구성이나 서명이 바뀌면 B10·B15를 그 결과에 맞춘다.
- sasu 사본과 겹치는 기간: sasu의 `hcoord daemon start`나 `install-local-skills.mjs`가 LaunchAgent와 `~/.hcoord/bin/hcoord`를 sasu 쪽으로 되돌릴 수 있다. 앱은 다음 실행 때 B11 규칙으로 되찾고, sasu 정리 변경이 이 경합을 끝낸다. 그 사이 sasu 사본은 고치지 않는다.
- 로그인 시 자동 시작은 hcoord 자체 지원 표에서도 아직 미검증이다. B13은 격리 label로 검증하고, 실제 로그인 재시작 관찰이 불가하면 그 사실을 결과에 적는다.
- ad-hoc 서명 앱이 LaunchAgent를 등록할 때 macOS가 "백그라운드 항목 추가됨" 알림을 띄울 수 있다(미확인). 알림 자체는 동작을 막지 않는다.
- machine id가 같은 기기가 둘일 수 있다(복제한 VM은 `/etc/machine-id`를 공유). 그 경우 다음 PRD의 hide 계보가 잘못된 기기의 pane을 부모로 볼 수 있으며, 규격 문서가 이 한계를 적는다. 원격 토큰 쓰기는 hcoord가 이미 쓰는 `herdr --machine <label>` 경로를 탄다.
- 두 source가 서로 다른 `parent_pane`을 쓰면 hide는 먼저 찾은 값을 쓴다(`wire.rs`, 문서화된 우선순위 없음). hcoord와 sasu·hide Fork는 같은 부모를 쓰므로 이 PRD 범위에서는 충돌하지 않는다.
- 실제 관찰의 안전 경계: 격리 HOME, 격리 `HCOORD_HOME`, 격리 Herdr 소켓, B21의 격리 label로만 앱과 데몬을 띄운다. 사용자의 라이브 hcoord 데몬, `~/.hcoord`, `com.hcoord.daemon` LaunchAgent, 라이브 hide·Herdr 창과 pane은 건드리지 않는다.
- 사용자가 미리 할 일: 없음. npm 게시를 원하면 그때 계정과 패키지 이름이 필요하다.
