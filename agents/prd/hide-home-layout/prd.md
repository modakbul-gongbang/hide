---
topic: "hide 소유 파일을 ~/.hide 아래로 모으기 (이 Mac과 기기, hcoord 포함)"
status: "ready"
human_approval: "approved"  # user 2026-10-02 verbatim: ㅇㅇㅇㅇ 그렇게 진행해~
review_profile: "high-risk"
review_rationale: "운영자의 살아 있는 hided 상태 폴더, hcoord ledger, 기기 helper 폴더와 동의 기록을 한 번에 옮기는 마이그레이션이고, 실패하거나 되돌리면 등록 프로젝트·라벨·요청 기록·기기 동의를 잃을 수 있다."
source_intake: "current conversation"
created_at: "2026-10-02"
updated_at: "2026-10-02"
---

# PRD: hide 소유 파일을 ~/.hide 아래로 모으기

## Goal

hide를 쓰는 운영자는 이 Mac과 SSH 기기에서 hide가 만든 파일이 `~/.local/state/hide`, `~/.local/share/hide`, `~/.hcoord`, `~/.hide`에 흩어져 있어, 무엇이 hide 것인지 한눈에 보거나 정리할 수 없다.
이 변경 뒤에는 hide가 소유한 파일이 모두 `~/.hide` 아래에 있고, 밖에는 다른 프로그램이 정해진 자리에서 읽어야 하는 것(Claude Code·Codex hook 항목, PATH의 링크)만 남는다.
hcoord는 hide 안에서만 쓰는 부품이 되어 같은 `~/.hide` 아래로 들어간다.
옮기는 동안 등록 프로젝트, 화면 상태, 라벨, 폰 페어링, hcoord 요청 기록, 기기 동의는 그대로 이어진다.

## Non-goals

- `~/Library` 아래 macOS 표준 위치(`Application Support/hide-desktop` Electron 프로필, `Application Support/hide`의 `ai.json`·Project Memory DB·옛 Swift 앱 파일, Preferences, Saved State, Caches)는 옮기지 않는다. 결과: `~/Library/Application Support/hide`는 남는다. 다시 볼 조건: Project Memory의 웹 단계가 DB 위치를 정할 때(`docs/ARCHITECTURE.md`의 보류 항목) 또는 운영자가 요청할 때.
- 운영자의 작업 폴더인 `~/hide` Home은 옮기지 않는다. 결과: 보이는 작업 폴더는 그대로다. 다시 볼 조건: 운영자 요청.
- Herdr 소켓 옆 라벨 생성 잠금(`<socket>.hide-label-generator.lock`)은 그대로 둔다. 결과: HOME이 다른 hided끼리도 한 Herdr에서 생성자를 하나로 맞춘다(labels-in-hided D-10). 다시 볼 조건: 그 규칙이 바뀔 때.
- 운영자 소유 설정인 `~/.codex/config.toml`의 `writable_roots`는 hide가 고치지 않는다. 결과: Codex 샌드박스에서 hcoord outbox를 쓰려면 운영자가 새 경로를 한 번 넣어야 한다(Risks). 다시 볼 조건: Codex hook 설치가 그 설정을 맡게 될 때.
- 옛 경로를 계속 읽는 호환 경로는 두지 않는다. 결과: 옮긴 뒤 옛 빌드로 되돌리면 옛 빌드는 빈 상태로 시작한다(Risks의 되돌리기 절차). 다시 볼 조건: 없음.
- `/tmp`·`$TMPDIR`의 임시 소켓과 폴더, 다른 프로그램이 만든 폴더(`~/.claude/projects/...` 사용량 조사 기록 등)는 다루지 않는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | hide 소유 파일의 자리는 `~/.hide` 하나다: `state/`(hided 상태), `host-helper/`(기기 helper), `kit/`, `agent-hooks/`, `hcoord/`(hcoord 홈, 명령은 `hcoord/bin/hcoord`). | 사용자: "근데 저게 다 ~/.hide 같은거에 잘 모여있는거맞지?", 제안 뒤 "ㅇㅇㅇ" |
| D-02 | `~/.hide` 밖에 남는 것은 `~/.claude/settings.json`·`~/.codex/hooks.json`의 hide 항목과 PATH용 `~/.local/bin/hide`, 그리고 `~/.hide/hcoord/bin/hcoord`를 가리키는 symlink `~/.local/bin/hcoord`뿐이다. 두 링크 모두 이름이 비었거나 이미 hide 것일 때만 건다. | 사용자가 받아들인 설명("밖에 있어야 하는 것"); `hcoord` 링크는 사용자: "symlink로 해서 두면 되나?" |
| D-03 | `~/Library` 아래 macOS 표준 위치와 `~/hide` Home, Herdr 소켓 옆 잠금은 옮기지 않는다. | 가정: OS·다른 프로그램이 정한 자리이거나 보이는 작업 폴더이고, Project Memory 위치는 별도 보류 결정 |
| D-04 | 상태 폴더 기본값은 `~/.hide/state`다. `HIDE_STATE_DIR`은 계속 이긴다. `XDG_STATE_HOME`이 지정돼 있으면 지금처럼 `$XDG_STATE_HOME/hide`를 쓰고 옮기지 않는다. | 사용자: "지정되면 계속 따름 (Recommended)" |
| D-05 | 이 Mac의 옛 상태 폴더는 한 번, 통째로 이름을 바꿔 옮긴다. 옛 폴더에서 도는 hided는 먼저 멈추고, 새 폴더가 이미 있으면 합치지 않고 옛 폴더를 남긴 채 진단을 남긴다. 다시 실행해도 아무것도 하지 않는다. | 가정: 원자적 rename이 가장 단순하고 반쯤 옮겨진 상태가 없다(원칙 2, 11) |
| D-06 | 상태 폴더 안에 있어야 할 경로(라벨 생성 잠금 `label-generators/`, 기기의 `workspace-bridges/`)는 모두 상태 폴더에서 계산한다. HOME에 박힌 경로는 없앤다. | 가정: 지금 두 곳이 HOME을 직접 써서 격리 daemon도 운영자 HOME에 쓴다(원칙 13) |
| D-07 | hcoord는 hide 안에서만 쓰는 부품이다. 단독 Herdr 플러그인 배포(manifest, startup hook, 원격 shim 설치 스크립트, README의 단독 설치 안내)는 없앤다. | 사용자: "음 우선 hide 안에서만 쓰게 하는게 나을것 같아 깔끔하게" |
| D-08 | hcoord 홈 기본값은 `~/.hide/hcoord`다. `HCOORD_HOME`이 지정되면 계속 따르고 옮기지 않는다. hide 코드에 박힌 `~/.hcoord/bin/hcoord`는 모두 이 홈에서 계산한다. | D-07의 결과; 가정: 박힌 경로 세 곳(kit shim, fork lineage, 기기 identity) |
| D-09 | `~/.hcoord`는 kit의 hcoord 부품이 한 번 옮긴다: daemon을 멈추고, 폴더 이름을 바꾸고, 새 홈에서 같은 LaunchAgent 이름으로 다시 띄운다. ledger와 수동 정지 선택은 그대로 가고, 쓰레기 임시 파일과 죽은 표시 파일은 가져가지 않는다. 실패하면 옛 폴더와 옛 daemon을 그대로 둔다. | 가정: 마이그레이션 코드가 없고 ledger가 비면 조용히 빈 상태로 시작하므로 명시 단계가 필요 |
| D-10 | sasu는 자체 hcoord 사본과 shim 쓰기(`~/Library/pnpm/hcoord`, `~/.hcoord/bin/hcoord`)를 없애고 PATH의 `hcoord`(hide가 깐 것)를 부른다. sasu 저장소 변경이다. | 사용자가 받아들인 제안("sasu는 고정 경로 대신 hide가 깔아 둔 hcoord 명령을 찾게"); 조사: sasu가 고정 LaunchAgent 이름으로 hide의 daemon을 빼앗을 수 있음 |
| D-11 | 기기 helper 기본 위치는 `~/.hide/host-helper`, kit 상태와 섞이지 않는 전용 하위 폴더다. | 가정: kit 폴더와 섞으면 기기 제거 때 helper 폴더가 지워지지 않는다(조사) |
| D-12 | 옛 기본 helper 폴더(`~/.local/share/hide/host-helper`)로 받은 기기 동의는, command 폴더가 같고 `HIDE_HOST_HELPER_ROOT`가 지정되지 않았으면 다시 묻지 않고 새 기본 폴더로 옮겨 적는다. 다른 값으로 받은 동의는 그대로다. | 사용자: "다시 안 물음 (Recommended)" |
| D-13 | 옛 경로는 새 경로가 동작한 뒤 kit 패스가 지운다: 기기의 옛 helper 폴더와 옛 `workspace-bridges`, 이 Mac의 고아 폴더(`~/.local/state/hide-plugin-upgrade`, `~/.local/share/hide/agent-context-labels`), 비면 `~/.local/share/hide`. 옛 helper 폴더를 가리키는 `hide` 링크는 hide의 것으로 보고 다시 건다. `~/.local/state` 같은 공용 부모는 지우지 않는다. | 가정: 지금은 다른 helper 폴더를 지우는 코드가 없고, 옛 링크가 "hide 것이 아님"으로 막힌다(조사) |
| D-14 | Herdr에 `hide.hcoord` 플러그인이 등록돼 있으면 kit 패스가 라벨 플러그인처럼 등록을 뺀다. | 가정: 남은 startup hook이 같은 daemon 이름을 다른 빌드로 바꿀 수 있다 |
| D-15 | 순서는 층으로 쌓는다: 이 Mac 상태 폴더 → hcoord → 기기. 한 PR 안에서의 작업 순서다. | 원칙 3; 가정 |
| D-16 | 전달은 저장소 기본(PR, `agents/config.json` delivery.mode=pr)이다. herdr-ide PR과 sasu PR을 함께 머지하고, 이 Mac에는 sasu 설치를 먼저, 새 hide 앱 설치를 바로 뒤에 한다. 앞서 사용자가 고른 "main에 직접 push"는 node 경로 quick에 대한 답이었다. | 사용자: "PR (Recommended)"; 설치 순서는 가정: Risks의 sasu 순서 |
| D-17 | 원칙 반영: engineering principles(oh-my-principle 654485f)를 전부 읽었고 2·3·11·13·14·15를 Decisions와 Behaviors로 옮겼다. design 도메인은 화면 배치가 바뀌지 않고 Settings 문구의 경로 값만 바뀌어 해당 없음으로 본다. | `sasu principles list` |
| D-18 | spec gate는 qa-log 없이 대화에서 쓴 PRD라 건너뛴다. | gen-prd 규칙 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 새 앱을 설치하고 열면 등록 프로젝트, 탭과 화면 배치, 사이드바 라벨, 폰 페어링과 푸시, 브라우저 초안이 설치 전과 같다. | D-01, D-05 |
| B2 | 첫 실행 뒤 `~/.local/state/hide`는 없고 `~/.hide/state`에 같은 파일이 있다. 복사본이 두 곳에 남지 않는다. | D-04, D-05 |
| B3 | 옛 상태 폴더에서 돌던 옛 빌드 hided는 앱이 연결할 때 멈추고, 새 hided 하나만 남는다. `hide status`는 그 하나를 보고한다. | D-05 |
| B4 | 앱을 다시 열거나 hided가 다시 시작해도 아무것도 다시 옮기지 않는다. | D-05 |
| B5 | `~/.hide/state`가 이미 있는데 옛 폴더도 있으면 새 폴더로 시작하고, 옛 폴더는 손대지 않은 채 두 경로를 적은 진단을 남긴다. 화면에 경고는 띄우지 않는다. | D-05 |
| B6 | `HIDE_STATE_DIR`을 준 hided(격리 검증 등)는 그 폴더만 쓰고 아무것도 옮기지 않으며, 운영자 HOME에 라벨 잠금이나 bridge 폴더를 만들지 않는다. | D-04, D-06 |
| B7 | `XDG_STATE_HOME`이 지정된 기계는 지금처럼 `$XDG_STATE_HOME/hide`를 쓰고 옮기지 않는다. | D-04 |
| B8 | 이미 열려 있던 pane의 에이전트는 앱 교체 뒤에도 지금 앱 교체 때와 같은 방법으로 hide 명령을 다시 쓸 수 있고, 새 세션의 안내 문구는 새 경로를 적는다. | D-05 |
| B9 | 이 Mac에서 `~/.local/state/hide-plugin-upgrade`, `~/.local/share/hide/agent-context-labels`가 사라지고, 비게 된 `~/.local/share/hide`도 사라진다. `~/.local/state`와 `~/.local/share`는 남는다. | D-13 |
| B10 | hcoord 옮김 뒤 진행 중이던 요청, 참가자, 편지, 수동 정지 선택이 그대로 있고, 감독 중인 run은 잠깐 끊겼다가 같은 요청으로 이어진다. | D-08, D-09 |
| B11 | 옮긴 뒤 `~/.hcoord`는 없고, hcoord daemon은 `~/.hide/hcoord`를 홈으로, 같은 LaunchAgent 이름으로 하나만 돈다. 옛 이름의 daemon이 따로 남지 않는다. | D-08, D-09 |
| B12 | hcoord 옮김이 실패하면 옛 폴더와 옛 daemon이 그대로 동작하고, Settings의 hcoord 줄이 실패와 다시 시도 방법을 보인다. 반쯤 옮겨진 상태가 없다. | D-09 |
| B13 | `HCOORD_HOME`을 준 hcoord(격리 검증)는 그 홈을 그대로 쓰고 아무것도 옮기지 않는다. | D-08 |
| B14 | 터미널에서 `hcoord`를 치면 hide가 깐 hcoord가 돈다. `~/.local/bin/hcoord`는 그 이름이 비었거나 이미 hide 것일 때만 걸고, 다른 것이 있으면 손대지 않고 Settings에 이유를 보인다. | D-02, D-08 |
| B15 | hcoord를 hide 없이 따로 설치하는 안내와 수단(Herdr 플러그인 manifest, startup hook, 원격 shim 설치 스크립트)이 저장소에서 사라진다. | D-07 |
| B16 | Herdr에 `hide.hcoord` 플러그인이 등록돼 있던 기계는 kit 패스 뒤 등록이 빠지고, 결과는 진단 기록에 남는다. | D-14 |
| B17 | 새 세션의 SessionStart 안내는 위임 명령을 `hcoord agent spawn --parent here …`로 적는다. | D-08 |
| B18 | sasu의 implement·supervisor 명령은 hide가 깐 hcoord로 동작하고, sasu 설치는 hcoord shim을 쓰지 않는다. sasu가 hcoord daemon을 자기 빌드로 바꾸지 않는다. | D-10 |
| B19 | 기기는 새 앱과 연결된 뒤 kit 패스에서 `~/.hide/host-helper/current`의 helper로 돌고, 옛 `~/.local/share/hide/host-helper`와 비게 된 `~/.local/share/hide`가 사라진다. | D-11, D-13 |
| B20 | 옛 기본 폴더로 동의했던 기기는 Allow를 다시 묻지 않고 바로 연결된다. 다른 helper 폴더로 동의한 기기는 그 동의대로 연결된다. | D-12 |
| B21 | 기기의 `~/.local/bin/hide`가 새 helper 폴더를 가리키도록 다시 걸린다. | D-13 |
| B22 | 기기의 Claude Code·Codex hook 항목이 새 helper 폴더를 가리키고, 옮기는 동안에도 에이전트 턴에서 hook이 빠지지 않는다. | D-11, D-13 |
| B23 | 기기에서 원격 Workspace 명령이 동작한다: helper와 기기의 `hide` 명령이 같은 `~/.hide/state/workspace-bridges`를 쓰고, 옛 bridge 폴더는 사라진다. | D-06, D-13 |
| B24 | 기기의 hcoord도 B10~B12처럼 `~/.hide/hcoord`로 옮겨지고, 이 Mac에서 그 기기의 hcoord identity를 읽을 수 있다. | D-08, D-09 |
| B25 | 기기 옮김이 중간에 실패하면 helper는 동작하는 쪽 폴더로 계속 돌고, 그 기기의 kit 줄이 실패를 보이며, 다음 패스에서 마저 맞춰진다. | D-13, D-15 |
| B26 | Settings › Devices의 부품 위치, 기기 추가 때 보이는 설치 목록, 기기 제거 확인 문구가 새 경로를 적는다. | D-01, D-11 |
| B27 | 기기를 제거하면 `~/.hide/host-helper`가 지워지고, `~/.hide/kit`, `~/.hide/agent-hooks`, hcoord는 지금처럼 남는다. | D-11 |
| B28 | 문서(INSTALL, ARCHITECTURE, agent-hooks, VERIFICATION, BUILD, PERFORMANCE_TESTING, dev-runtime, AI_PROVIDERS, UI_BEHAVIOR, AGENTS, CONTRIBUTING, hcoord README)가 새 배치와 옮기지 않는 것, 되돌리기 절차를 적고, hcoord를 따로 설치할 수 있다는 말을 지운다. | D-01, D-03, D-07 |

## Technical structure

- 경로 계산: hide 소유 기본 경로(`~/.hide/state`, `~/.hide/host-helper`, `~/.hide/hcoord`)를 한 곳에서 정하고, 상태 폴더 하위 경로(라벨 잠금, bridge)와 hcoord 명령 경로는 거기서 파생한다. 박힌 HOME 경로는 없앤다.
- 이 Mac 마이그레이션: `hide connect`가 옛 상태 폴더의 daemon을 찾아 멈춘 뒤, 기본 위치일 때만 옛 폴더를 새 위치로 rename한다. 이후는 새 폴더만 본다.
- hcoord: 기본 홈이 바뀌고, kit의 hcoord 부품이 stop → rename → shim 재작성 → ensure 순서의 일회 마이그레이션을 한다(이 Mac과 기기 공통). 단독 플러그인 배포물은 지운다.
- 기기: helper 기본 폴더가 바뀌고, 동의 기록은 옛 기본값이면 새 기본값으로 옮겨 적는다(재동의 없음). helper가 옛 기본 helper 폴더를 지울 수 있도록 helper 프로토콜에 그 작업이 들어가며 프로토콜 버전이 오른다.
- 외부 저장소: sasu에서 hcoord 사본·shim 설치를 제거하고 PATH의 `hcoord`를 부르게 바꾼다.
- 데이터 구조(ledger 형식, core-state 형식)는 바뀌지 않고 위치만 바뀐다. 동의 기록의 `helper_root` 값만 다시 쓰인다.

## Risks

- 운영 이전: 새 앱 설치가 이 Mac의 hided와 hcoord daemon을 멈추고 폴더를 옮긴다. 감독 중인 sasu run이 있으면 그동안 hcoord가 잠깐 끊긴다. 열린 run이 없을 때 설치한다.
- sasu 순서: 새 hide와 옛 sasu가 함께 있으면 옛 sasu의 hcoord 사본이 `~/.hcoord`를 다시 만들고 고정 LaunchAgent 이름으로 daemon을 빼앗을 수 있다. sasu를 먼저 설치하고 곧바로 새 hide 앱을 설치해서 막는다.
- PATH: `~/Library/pnpm/hcoord`(sasu가 쓴 것)가 PATH에서 앞서 있으면 sasu 설치가 그것을 지우기 전까지 `hcoord`가 sasu 빌드를 가리킨다.
- Codex 샌드박스: `~/.codex/config.toml`의 `writable_roots`에 `~/.hide/hcoord`를 넣기 전까지 Codex 에이전트의 hcoord 편지 쓰기가 막힌다. 운영자가 할 일(또는 설치하는 세션이 운영자 허락을 받아 한 줄 고침).
- 되돌리기: 옮긴 뒤 옛 앱(`.bak`)을 열면 옛 빌드는 빈 상태로 시작한다. 되돌리려면 앱과 hcoord daemon을 멈추고 `~/.hide/state` → `~/.local/state/hide`, `~/.hide/hcoord` → `~/.hcoord`로 되돌린 뒤 옛 앱을 연다. 이 절차를 INSTALL 문서에 적는다.
- 붙여 넣은 클립보드 이미지: 옮기기 전에 프롬프트에 붙인 이미지 경로는 옛 폴더를 가리킨다(24시간 수명이라 영향이 작다).
- 기기(mini)는 운영 기계다. 새 앱이 연결되면 kit 패스가 자동으로 helper·bridge·hcoord를 옮긴다. 이 Mac 설치를 승인하는 것이 mini 이전을 승인하는 것임을 설치 전에 운영자에게 알린다.
- 확인 범위: 모든 동작 확인은 격리 HOME·격리 Herdr·격리 sshd 기기에서 한다. 운영자의 앱, hided, Herdr, hcoord daemon, mini에는 구현 중 손대지 않는다.
- 구현 전에 운영자가 할 일은 없다. Codex `writable_roots` 한 줄과 mini 이전 승인은 설치 때의 일이다.
