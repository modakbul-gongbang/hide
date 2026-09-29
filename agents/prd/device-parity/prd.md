---
topic: "기기 추가와 로컬 첫 설치가 같은 Hide 구성을 하나의 모듈로 설치"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "운영자의 에이전트 설정 파일, 실행 파일, 플러그인, LaunchAgent를 이 Mac과 이미 등록된 원격 기기에 묻지 않고 설치, 교체, 제거하므로 보안 경계와 운영 중인 기계에 영향이 있다."
source_intake: "agents/interview/device-parity/qa-log.md"
created_at: "2026-09-29"
updated_at: "2026-09-29"
---

# PRD: 기기 추가와 로컬 첫 설치가 같은 Hide 구성을 하나의 모듈로 설치

## Goal

hide를 처음 실행하거나 SSH 기기를 추가하면, 그 기계에 hide CLI, Claude Code와 Codex hook, agent-context-labels 플러그인, hcoord가 묻지 않고 모두 설치되어 기기 pane이 로컬 pane과 같은 경험을 준다.
지금은 hook은 이 Mac에만, 라벨 플러그인은 손으로만, hcoord는 이 Mac의 Electron 호스트에서만, helper는 기기 동의 뒤에만 설치되어, 기기에서 무엇이 되는지가 그 기기에 무엇을 손으로 깔았는지에 달려 있다.
하나의 설치 모듈(kit)이 대상 기계만 바꿔 같은 구성요소를 설치하고 상태를 보고하므로, 운영자는 "등록하면 다 된다"는 한 가지 규칙만 알면 된다.

## Non-goals

- 기기 프로젝트의 Project Memory는 이번에 만들지 않는다(D-14, D-23). 기기 세션은 Memory 없이 동작한다. 별도 device-memory PRD에서 다시 다룬다.
- 패키지가 싣지 않는 플랫폼(다른 OS나 아키텍처)의 기기에는 설치하지 않는다(D-17). 그 기기는 보기와 조작만 된다. 그런 기기를 쓰겠다는 요청이 오면 다시 다룬다.
- Herdr 자체는 설치하거나 업데이트하지 않는다(D-10). 로컬은 번들 Herdr를 쓰고, 기기는 지금처럼 Herdr가 전제조건이다.
- 다른 도구의 hook 항목과 파일은 관리하지 않는다(D-15). Hide marker가 있는 항목만 쓰고 지운다.
- 기기를 삭제해도 hcoord daemon과 `~/.hcoord`는 지우지 않는다(D-16). 그 기계의 다른 도구가 쓴다.
- 원칙(engineering 4, 10): 실패한 구성요소를 빈 값이나 조용한 건너뛰기로 덮지 않는다. 실패는 구성요소 상태와 진단 로그로 전달한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-02 | 변경 전 현재 상태: 기기 helper 동의(계약 2)는 helper와 hide CLI만 digest 폴더에 올리고 `~/.local/bin/hide`를 링크하며, 동의 문구는 hook과 설정을 바꾸지 않는다고 약속한다. 패키지는 빌드 아키텍처 helper만 싣는다. 동의 범위와 설치 항목은 D-12, D-13이 대체한다. | 사실: `herdr-core/src/remote/host.rs:58-70,694-1021`, `web/src/settings.ts:249-257`, `desktop/scripts/package.mjs:63-68` |
| D-05 | 변경 전 현재 상태: 기기 pane의 hook 토큰은 Mac에 도착하지만 `RemoteHost`로 고정 판정되어 버려진다. 라벨 토큰은 이미 전달된다. 판정은 D-21이 대체한다. | 사실: `herdr-core/src/session_sync/replica.rs:397-403`, `hide-agent-hooks/src/diagnosis.rs:142-143` |
| D-06 | 제약(유지): 고정된 Herdr 0.9.1은 로컬 디렉터리 플러그인을 소켓 `plugin.link`/`plugin.unlink`로 등록하고, GitHub 설치와 삭제는 CLI만 제공한다. | 사실: `herdr api schema --json`, `herdr plugin --help` |
| D-07 | 제약(유지): Memory 저장소는 Mac에 있고, UserPromptSubmit은 100ms, SessionStart는 8초 안에 끝나며, 기기 hook은 PATH의 hide CLI로 bridge를 거쳐 Mac hided에 닿는다. | 사실: `hide-agent-hooks/src/memory.rs:70`, `docs/agent-hooks.md`, `docs/ARCHITECTURE.md:333` |
| D-08 | 처음 설치할 때(로컬 첫 실행과 기기 추가) helper, hide CLI, agent hook, 플러그인을 구성요소마다 묻지 않고 모두 설치한다. | 사용자: "처음 설치할 때 그냥 다 등록해줘도 괜찮아... 등록, helper, hooks, 플러그인 이거를 전부 다 잘해주면 좋겠어" |
| D-09 | 설치는 하나의 모듈로 만들어 로컬 첫 설치와 기기 추가가 같은 구성과 경험을 준다. | 사용자: "모듈화되어있어서 그냥 처음에 hide 설치해서 local에 있을때랑 디바이스 추가한거랑 동일한 경험" |
| D-10 | kit는 한 목록이다: hide CLI, hide-agent-hooks와 Claude Code/Codex hook 항목, 미리 빌드한 라벨 플러그인, hcoord. 기기에는 host helper가 더해진다. 대상 기계(로컬은 직접 파일, 기기는 helper 호출)만 바꿔 같은 단계를 실행하고, Herdr는 설치하지 않는다. | 가정: 위임 범위 안의 구조 선택 |
| D-11 | hook 명령은 기계별 고정 경로(기기는 helper root의 고정 링크, 로컬은 앱 번들)를 가리키고, 바이너리가 없으면 아무 일 없이 성공으로 끝나도록 감싼다. | 가정: 2026-09-10 hook 경로 사고, mini의 Orca hook 가드 선례 |
| D-12 | 기기 추가는 버튼 하나로 kit 전체에 동의하고, 'Add without files'는 없애며, 추가 폼이 설치 항목과 위치를 한 번 보여 준다. 동의 계약은 3이 된다. | 가정: D-08에서 파생 |
| D-13 | 계약 2로 허용된 기기(mini 등)는 다음 연결 때 묻지 않고 kit 전체를 설치하며, 다른 도구의 hook은 건드리지 않는다. 설정 화면에서 한 번 누르게 하는 안은 기각했다. | Q1 (사용자: "Q1. a") |
| D-14 | 기기 프로젝트의 Memory는 이번 범위에서 빼고 별도 PRD로 설계하며, Memory 텍스트를 기기로 복사하는 안과 이번에 함께 하는 안은 기각했다. | Q1 (사용자: "Q2. a로 ㅇㅇ 메모리는 우선 제외") |
| D-15 | 손으로 설치된 같은 id의 구성요소(GitHub 라벨 플러그인, 수동 hcoord, 예전 경로의 Hide hook)는 kit 버전으로 교체하고 플러그인 config 디렉터리와 hcoord 상태는 보존한다. 다른 도구의 항목은 건드리지 않는다. | 가정: 위임 범위 안의 호환 선택 |
| D-16 | 기기 삭제는 Hide hook 항목(marker 기준), 플러그인 link, Hide 소유 `~/.local/bin` 링크, helper root를 제거하고 hcoord는 남긴다. 연결이 없으면 등록만 지우고 남은 항목을 로그에 적는다. | 가정: mini의 sasu 파이프라인이 hcoord를 쓴다 |
| D-17 | 지원 기기는 패키지가 싣는 플랫폼이고, 다른 플랫폼 기기는 보기와 조작만 되며 '지원하지 않는 플랫폼'을 보인다. | 가정: D-02 |
| D-18 | 구성요소 하나가 실패해도 나머지는 설치되고, Settings의 기계 행이 구성요소별 상태와 이유를 보이며 상세는 진단 로그에 남긴다. 배너와 경고창은 없다. | 가정: design 원칙 13, 운영자의 조용한 화면 선호 |
| D-19 | 로컬 첫 실행도 같은 모듈로 kit 전체를 설치하고, 번들 밖 standalone hided는 이 Mac 설치를 거절하고 이유를 보고한다. | 가정: D-09 |
| D-20 | 앱이 업데이트되면 다음 실행과 연결에 각 기계의 kit를 묻지 않고 갱신하며(오래된 hook 포함), 운영자가 지운 구성요소는 다시 넣지 않는다. | 가정: D-08에서 파생 |
| D-21 | 기기 pane의 hook 판정은 고정 `RemoteHost` 대신 그 기기의 kit 상태와 pane 토큰으로 로컬과 같은 함수를 쓴다. | 가정: D-05 |
| D-22 | 검증은 이 Mac과 mini의 실제 HOME을 건드리지 않고, 기기 설치는 격리된 sshd와 private HOME의 desktop e2e로, 로컬 설치는 HOME fixture로 증명한다. | 가정: `docs/agent-hooks.md` Testing, PR #227의 격리 sshd |
| D-23 | 제약(유지, D-14로 범위 밖): Project Memory는 지금 이 Mac의 checkout에만 만들어지고, 기기 프로젝트에는 추출, 분석, 저장, 주입 어느 단계도 없다. | 사실: `herdr-core/src/runtime/memory.rs:59-66,1371-1390` |
| D-24 | 기기 삭제 확인은 제거할 항목과 남길 항목을 한 줄로 더해 한 번 확인받고, 취소하면 아무것도 바뀌지 않으며, 같은 기기를 다시 추가하면 kit 전체를 새로 설치한다. | 가정: gap-audit F9 경고 반영 |
| D-25 | 예전에 동의 없이 등록된 기기('Add without files')는 자동 설치하지 않고, 기기 행의 허용 버튼 한 번으로 kit 전체가 설치된다. | 가정: 운영자가 명시적으로 거절한 기기 |
| D-26 | kit는 기계마다 자기가 설치한 구성요소 기록을 남기고, 설치했던 구성요소가 사라지면 운영자가 지운 것으로 본다. kit 이전의 `installed-once` marker는 제거 기록이 아니다. | 가정: mini에 marker만 있고 Hide hook이 없다 |
| D-27 | hcoord 설치는 Electron 호스트에서 kit 모듈로 옮기고, Settings Agents 탭의 hook 설치/업데이트 흐름은 상태와 다시 설치로 바뀌며, 옛 경로는 같은 변경에서 지운다. | 가정: engineering 원칙 1 |
| D-28 | 전달은 `agents/config.json`의 PR 모드(base `main`, CI 확인)이고, 머지는 운영자 승인 후 한다. | 가정: 저장소 설정 |
| D-29 | 원칙 intake: `~/projects/oh-my-principle` 654485f의 engineering과 design 문서를 전부 읽고 적용했다. engineering 1, 4, 5, 10, 11, 14, 15와 design 5, 6, 9, 13이 Behaviors와 Non-goals로 옮겨졌고, 옮기지 않은 규칙은 이 변경에 해당하는 관찰 가능한 결과가 없다. | 사실: `sasu principles list` |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 설치된 hide.app을 처음 실행하면 묻는 창 없이 이 Mac에 kit가 설치된다: `~/.local/bin/hide` 링크, Claude Code와 Codex의 hook 항목, Herdr 플러그인 목록의 agent-context-labels, hcoord shim과 daemon. | D-08, D-09, D-10, D-19 |
| B2 | 설치 뒤 새로 시작한 Claude Code와 Codex pane은 repo checkout 없이도 task와 상태 라벨, 서브에이전트 수, SessionStart의 Workspace 안내를 받는다. | D-10 |
| B3 | 앱이나 helper를 지우거나 옮겨도 Claude Code와 Codex 세션의 턴은 hook 오류 없이 진행된다. 바이너리가 없는 hook은 아무 일도 하지 않고 성공으로 끝난다. | D-11 |
| B4 | 설치는 다른 도구의 hook 항목과 설정 키를 개수와 내용 그대로 두고, 같은 설치를 두 번 해도 설정 파일이 바뀌지 않는다. | D-10, D-15, D-29 |
| B5 | 손으로 설치된 같은 id의 라벨 플러그인, 수동 hcoord, 예전 앱 경로를 가리키는 Hide hook은 kit 버전으로 바뀌고, 플러그인 config와 hcoord 상태는 남는다. | D-15 |
| B6 | `~/.local/bin/hide`를 Hide가 아닌 파일이 차지했거나, 설정 파일을 읽을 수 없거나, Herdr가 꺼져 있거나 `plugin.link`를 거절하면 그 구성요소만 설치되지 않고 이유가 남는다. 읽지 못한 설정 파일에는 쓰지 않고, 나머지 구성요소는 설치된다. | D-18, D-29 |
| B7 | Settings > Devices에 This Mac 행과 각 기기 행이 같은 형태로 구성요소별 상태(설치됨, 오래됨, 지워짐, 실패와 이유)를 보이고, 다시 설치 버튼은 문제가 있는 행에만 있다. 배너와 경고창은 없고 상세는 진단 로그에 남는다. | D-18, D-29 |
| B8 | 다시 설치는 빠졌거나 오래됐거나 지워진 구성요소만 설치하고 맞는 구성요소는 건드리지 않으며, 진행 중에는 버튼이 진행 상태를 보인다. | D-18, D-26, D-29 |
| B9 | 운영자가 kit가 설치했던 hook 항목이나 플러그인을 직접 지우면 다음 실행이나 연결에서 다시 넣지 않고 '지워짐'으로 보이며, 다시 설치로만 돌아온다. | D-20, D-26 |
| B10 | 앱을 업데이트하면 다음 실행에 이 Mac의 kit가, 다음 연결에 각 기기의 kit가 묻지 않고 새 버전으로 바뀐다. 오래된 hook도 바뀌고, 지워짐 상태의 구성요소는 그대로 둔다. | D-20 |
| B11 | 번들 밖에서 실행한 hided(개발 빌드, 브라우저용 standalone)는 이 Mac kit를 설치하지 않고, This Mac 행이 설치된 앱에서 설치된다는 이유를 보인다. 운영자 설정 파일에 빌드 디렉터리 경로를 쓰지 않는다. | D-11, D-19 |
| B12 | Settings > Devices의 추가 폼은 추가 버튼 하나이고, 설치할 항목(helper와 hide CLI, Claude Code와 Codex hook, 라벨 플러그인, hcoord)과 위치를 한 번 보여 준다. 'Add without files' 선택지는 없다. | D-12, D-29 |
| B13 | 기기를 추가하면 연결 뒤 같은 kit 모듈로 helper와 kit 전체가 그 기기에 설치되고, 기기 행이 This Mac 행과 같은 구성요소 상태를 보인다. | D-09, D-10, D-12 |
| B14 | 그 기기에서 새로 시작한 에이전트 pane은 사이드바, Agents 목록, pane 헤더에서 로컬 pane과 같은 라벨, 서브에이전트 수, 계측 표시를 보이고, 'Hide does not install hooks on remote hosts' 표시는 사라진다. | D-05, D-21 |
| B15 | 기기 세션의 SessionStart 안내는 그 기기 checkout의 Workspace 명령을 담고, 안내된 hide 명령은 bridge를 거쳐 이 Mac의 hide에서 열린다. | D-07, D-10 |
| B16 | 기기 hook은 기기 자신의 고정 경로를 가리키므로, 이 Mac의 앱을 옮기거나 업데이트해도 기기 세션의 hook이 깨지지 않는다. | D-11 |
| B17 | 패키지에 없는 플랫폼의 기기는 보기와 조작이 지금처럼 되고, 기기 행이 '지원하지 않는 플랫폼'을 보이며 아무것도 설치하지 않는다. | D-17 |
| B18 | 설치가 SSH 실패나 연결 끊김으로 중간에 멈추면, 연결이 돌아왔을 때 남은 구성요소만 이어서 설치하고 같은 파일을 다시 올리지 않는다. | D-18, D-29 |
| B19 | 계약 2로 허용된 기기는 다음 연결 때 묻지 않고 kit 전체가 설치되고, 그 기기에 있던 다른 도구(sasu, Orca, oh-my-principle 등)의 hook 항목은 그대로다. | D-13, D-15 |
| B20 | kit 이전의 `~/.hide/agent-hooks/installed-once`만 있고 Hide hook이 없는 기기는 '지워짐'이 아니라 미설치로 보고 hook을 설치한다. | D-26 |
| B21 | 동의 없이 등록된 예전 기기는 자동 설치하지 않고, 기기 행의 허용 버튼 한 번으로 kit 전체가 설치된다. | D-25 |
| B22 | 기기 삭제 확인은 그 기기에서 제거할 항목(Hide hook 항목, 플러그인, Hide 소유 링크, helper)과 남길 항목(hcoord)을 한 줄로 말하고, 버튼은 결과를 이름으로 가지며 기본 선택이 없다. 취소하면 아무것도 바뀌지 않는다. | D-16, D-24, D-29 |
| B23 | 연결된 기기를 삭제하면 그 기기 설정 파일에서 Hide marker 항목만 사라지고 다른 도구의 항목은 그대로이며, Hide가 link한 플러그인, Hide 소유 `~/.local/bin` 링크, helper root가 사라지고 hcoord는 남는다. | D-16 |
| B24 | 삭제 순간 연결이 없거나 일부 제거가 실패하면 등록은 지워지고 남은 항목과 이유가 진단 로그에 남는다. 남은 hook은 세션을 깨지 않고, 같은 기기를 다시 추가하면 kit 전체가 새로 설치된다. | D-11, D-24 |
| B25 | 기기 세션의 SessionStart 안내에는 Memory capsule이 없고, UserPromptSubmit은 Memory를 주입하지 않고 바로 성공으로 끝나며, Memory 텍스트는 기기로 복사되지 않는다. | D-14, D-23 |
| B26 | bridge나 이 Mac의 hide가 응답하지 않으면 기기 세션은 Workspace 안내 없이 그대로 시작하고, 다음 세션에서 다시 시도한다. | D-07, D-18 |
| B27 | Settings > Agents의 hook 그룹은 설치와 업데이트 승인 흐름 대신 This Mac과 각 기기의 hook 상태와 다시 설치만 보이고, 기기에는 아무것도 설치하지 않는다는 문구는 사라진다. | D-21, D-27 |
| B28 | hcoord daemon과 라벨 플러그인 watcher는 기계마다 하나만 돌고 두 번 설치해도 늘지 않으며, 기기 연결이 끝나면 그 연결의 helper 프로세스도 끝난다. | D-10, D-27, D-29 |
| B29 | `docs/INSTALL.md`와 `docs/agent-hooks.md`는 첫 실행과 기기 추가 때 설치되는 kit를 같은 목록으로 설명하고, 기기 삭제와 운영자 제거 규칙을 적는다. | D-09, D-16, D-26 |

## Technical structure

- 현재 상태(이 변경이 대체한다): 로컬 첫 실행은 agent hook만 앱 번들 경로로 설치하고(`herdr-core/src/session_sync/coordinator.rs:662-693`, `hide-agent-hooks/src/install.rs:126-335`), 라벨 플러그인은 Hide가 설치하지 않으며 repo checkout 빌드로만 동작하고(`plugins/agent-context-labels/herdr-plugin.toml`), hcoord는 이 Mac의 Electron 호스트만 설치한다(`desktop/src/main/host.ts:308-332`).
- 새 공유 kit 모듈(Rust crate)이 구성요소 목록 하나를 소유하고, 구성요소마다 설치, 상태, 제거를 명시적인 HOME, Herdr 소켓, kit 원본 폴더를 받아 실행한다. 새 구성요소는 목록에 한 항목을 더하는 것으로 들어간다.
- 로컬 실행기는 hided의 coordinator 스레드에서 runtime mutex 밖에서 돌고, kit 원본은 앱 번들 `Contents/Resources`다. Electron 호스트의 hcoord 설치와 Settings의 hook 설치 승인 경로는 지운다.
- 기기 실행기는 `hide-host-helper`가 같은 crate를 링크해 새 helper 호출(kit 적용, 상태, 제거)로 수행한다. helper protocol 버전을 올리고, kit payload(helper, hide, hide-agent-hooks, 라벨 바이너리와 패키지용 manifest, hcoord)는 기존 digest 폴더 SFTP 설치로 올리며, hook이 가리킬 고정 링크를 둔다.
- `HostConsent` 계약은 3이 되고, 계약 2는 연결 때 자동으로 3이 되며, 동의가 없는 기기는 바뀌지 않는다.
- 기계마다 kit 기록 파일이 설치한 구성요소와 버전을 남겨 운영자 제거를 판단한다.
- 라벨 플러그인은 build 단계 없이 동봉 바이너리를 실행하는 패키지 manifest로 `plugin.link`하고, GitHub로 설치된 같은 id는 그 기계의 herdr CLI로 먼저 지운다. Herdr 계약(0.9.1)은 바뀌지 않는다.
- snapshot에 기계별 kit 상태(This Mac과 각 기기)가 더해져 wire enum 계약(`contracts/snapshot-wire-enums.json`)이 늘고, 기기 pane 판정은 replica가 전달한 토큰과 그 기기의 kit 상태를 쓴다.
- 패키징은 hide-agent-hooks와 라벨 바이너리를 기기 payload에도 싣는다. 데이터 이전은 없고 Memory 저장소는 바뀌지 않는다.

## Risks

- 모든 기계의 에이전트 설정 파일을 자동으로 고친다. marker 항목만 쓰고 지우기, 원자적 쓰기와 키 순서 보존, 바이너리 없음 가드, 다른 도구 항목 보존 테스트로 제한한다.
- 계약 2 자동 업그레이드는 운영 중인 mini에 hook 5개, 라벨 플러그인 교체, hcoord 채택을 일으킨다. Hermes gateway는 profile별 HOME으로 돌아 영향 밖이고, SessionStart는 최대 8초 예산이 세션 시작마다 더해진다.
- 실검증 안전 경계: 이 Mac과 mini의 실제 HOME, 설정 파일, `/Applications/hide.app`은 테스트 대상이 아니다. 기기 설치는 격리 sshd와 private HOME에서만 증명하고, mini 실제 업그레이드는 머지 뒤 운영자가 새 앱을 설치할 때 일어난다.
- 기기를 연결 없이 삭제하면 Hide 항목이 그 기기에 남는다. 가드 때문에 세션은 깨지지 않지만 파일은 남는다.
- 패키지는 빌드 아키텍처만 싣는다. 다른 아키텍처 Mac 기기는 '지원하지 않는 플랫폼'이 된다.
- 구현 전에 운영자가 할 일은 없다.
