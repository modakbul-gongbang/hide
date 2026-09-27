---
topic: "Checkout-bound hide capability for daemon-run agents (Codex shared app-server)"
status: "ready"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "The change adds a second way to obtain a Workspace control credential (by caller cwd instead of pane ancestry), which is an access-boundary change in hided's authentication path."
source_intake: "agents/interview/checkout-capability/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: Checkout-bound hide capability for daemon-run agents

## Goal

Hide 안의 pane에서 사용자가 직접 친 `codex`(Codex 0.157, `daemon_auto_start` 켜짐)는 tool 명령과 hook을 launchd 아래 공용 `codex app-server --managed-daemon`에서 실행하므로, 그 명령이 부르는 bare `hide` Workspace 명령은 pane 조상 검증에 항상 실패한다.
이 변경 후에는 pane 조상 검증이 실패한 로컬 호출자에 대해 hided가 그 프로세스의 cwd를 직접 읽어 cwd가 속한 등록된 checkout에 묶인 capability를 발급하므로, Codex daemon 스레드에서도 Claude Code pane과 똑같이 `hide browser open <url> --reveal`이 자기 Workspace 옆 브라우저 패널에 열리고 view 조작과 닫기가 된다.
Codex 설정, 실행 옵션, CLI 표면, hook 안내문은 바뀌지 않는다.

## Non-goals

- View의 에이전트별 소유자/identity는 넣지 않는다. 한 checkout의 capability 보유자는 그 checkout의 어떤 view든 select/split/move/close 할 수 있다. 다시 볼 조건: 한 checkout에서 두 에이전트가 서로의 view를 건드려 실제 충돌이 관찰될 때 (D-08).
- SSH 원격 장치의 호출자는 지금처럼 pane attestation만 쓴다. 원격 Codex daemon은 이 변경으로 동작하지 않는다. 다시 볼 조건: 원격 장치에서 같은 실패가 관찰될 때 (D-12).
- Codex daemon을 끄거나(`--no-daemon`, `daemon_auto_start=false`) Hide UI 실행 경로에 Codex 옵션을 붙이는 대안은 채택하지 않았다 (D-07).
- hided/src/server.rs, `hide` CLI 명령 표면, SessionStart hook 안내문, Action 목록은 바꾸지 않는다 (D-07).
- 엔지니어링 원칙 6(기성 해법 조사)은 이미 수행했다: 동일 문제를 만난 orca(#22873)와 upstream 이슈를 읽었고, cwd 기반 attestation은 그 조사 결과 위에서 고른 것이다. 번역하지 않은 원칙은 없다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Codex 0.157은 pane에서 친 `codex`를 launchd 아래 detach된 공용 daemon(ppid 1)에 붙이고, tool shell과 hook을 그 daemon에서 처음 뜬 pane의 env로 실행한다. 이 머신의 daemon은 삭제된 pane w9J:p17의 env를 들고 있다. | qa-log D-01: `ps`, `codex features list`, upstream README("per-client environment isolation is not provided") |
| D-02 | hided는 로컬 호출자를 peer pid의 조상이 Herdr가 보고한 pane shell pid인지로 증명하고, pane id는 호출자 env의 HERDR_PANE_ID에서 온다. daemon 호출자는 둘 다 실패한다. | qa-log D-02: hided/src/pane_auth.rs:438-449, workspace_cli.rs:35-40, codex-shared-report.md |
| D-03 | core의 `workspace_control_query`는 pane id를 checkout을 찾는 데만 쓰고, 이후 조회·액션·재시도 기록·reveal은 (device_id, checkout_path)로 동작한다. reveal은 checkout을 앞으로 가져올 뿐 pane을 포커스하지 않는다. | qa-log D-03: herdr-core/src/runtime/workspace_control.rs:596-641, :30-52 |
| D-04 | 기존 문서는 detach된 Codex tool이 hook이 발급한 reference prefix를 쓰리라 전제했지만, 0.157에서는 hook도 detach돼 reference가 발급되지 않는다. pane에서 발급한 reference를 수동으로 넘긴 실험은 모든 bare 명령이 성공했다. | qa-log D-04: docs/agent-hooks.md:39, codex-shared-report.md, capability-only.json |
| D-05 | pane attestation이 실패한 로컬 호출자에 대해 hided가 peer pid의 cwd를 직접 읽어(macOS `proc_pidinfo` PROC_PIDVNODEPATHINFO, Linux `/proc/<pid>/cwd`; 요청 JSON에서 받지 않음) cwd가 속한 등록·연결된 checkout에 묶인 capability를 발급한다. 그 capability의 validate는 checkout이 아직 등록돼 있고 장치가 연결돼 있는지만 본다. 등록된 checkout 밖이면 거절한다. | 사용자: "ㅇㅇㅇ 여튼 이게 최소한 변경으로 다 동작할 수 있다는거잖아? 이거 바로 /please 로 작업하고" (qa-log D-05) |
| D-06 | 최종 검증은 설치된 Hide 앱에서 (a) Claude Code pane, (b) daemon을 켠 채 사용자가 pane에서 직접 친 `codex` 각각이 bare `hide browser open <url> --reveal`로 자기 Workspace 옆에 view를 열고, view 조작(status / select / split 또는 move) 뒤 `hide view close`로 닫는 것까지 확인한다. 테스트용 pane과 view만 만들고 닫는다. | 사용자: "최종 검증으로 claude code, codex에서 가각ㄱ hide open browser 시키고 뭔가 간단한 조작같은것도 시키게 해보고 되는거 확인하고 닫는거까지 해보자!" (qa-log D-06) |
| D-07 | server.rs, CLI 표면, hook 안내문, Action 목록, Codex 옵션/config는 바꾸지 않는다. daemon 끄기 대안은 기각. | 사용자가 daemon 유지 경로("daemon켜도 처리되도록 최소한으로")를 고르고 위 목록을 수락 (qa-log D-07) |
| D-08 | 가정: view의 에이전트별 소유자는 비범위. 사용자의 identity 질문에 비범위로 답했고 반대는 없었으나 침묵은 동의가 아니므로 거부 가능한 위임 가정. | 가정: 위임 하의 agent default (qa-log D-08) |
| D-09 | 가정: checkout-bound capability는 기존 수명 규칙(8시간, 미claim 30초, 직접 CLI는 one-shot, pane capability와 합쳐 64개 cap)을 그대로 따르고, hook이 발급한 persistent checkout capability는 같은 장치·checkout의 반복 bootstrap에 재사용된다. | 가정: pane_auth.rs:27-33 상수 (qa-log D-09) |
| D-10 | 가정: pane 자손도 아니고 등록된 checkout 안도 아닌 호출자는 별도 이유 `checkout_not_registered`와 다음 행동을 받는다. | 가정: 엔지니어링 원칙 10 (qa-log D-10) |
| D-11 | 가정: 등록된 checkout이 중첩되면 cwd를 포함하는 가장 긴 등록 경로가 이기고, 비교는 canonical 경로로 한다. | 가정 (qa-log D-11) |
| D-12 | 가정: fallback은 로컬 장치 호출자에만 적용하고 SSH bridge 원격 호출자는 pane attestation만 유지한다. | 가정: pane_auth.rs attest_remote (qa-log D-12) |
| D-13 | 가정: checkout-bound 발급과 거절마다 구조화 진단 이벤트 하나(device, checkout id, peer pid, reason)를 남기고, 호출자 env 경로나 reference 바이트는 남기지 않으며 화면에는 아무것도 띄우지 않는다. | 가정: 엔지니어링 원칙 9·10, 디자인 원칙 13 (qa-log D-13) |
| D-14 | daemon 안에서 도는 Codex SessionStart hook이 스레드 cwd를 보는지(그래서 Workspace 안내문이 다시 뜨는지)는 관찰해 어느 쪽이든 기록한다. bare 명령은 그와 무관하게 동작해야 한다. | 사용자가 승인한 /please 인자: "Codex의 daemon 내 SessionStart hook cwd가 스레드 cwd인지도 검증 항목에 넣는다" (qa-log D-14) |
| D-15 | 같은 변경에서 docs/ARCHITECTURE.md의 Workspace CLI attestation 문단, docs/agent-hooks.md:39, docs/BROWSER_DISPLAYS.md의 "attested pane" 표현을 새 동작에 맞게 고친다. | CLAUDE.md, docs/README.md:35 (qa-log D-15) |
| D-16 | libc 0.2.189(Cargo.lock)가 `proc_vnodepathinfo`/`PROC_PIDVNODEPATHINFO`를 제공하므로 새 의존성은 없다. | Cargo.lock:1700 (qa-log D-16) |
| D-17 | 전달은 agents/config.json대로 pull request(base main, worktree). merge는 사용자의 명시 승인과 저장소 필수 검사 뒤에만. | agents/config.json; 사용자의 /please 요청 (qa-log D-17) |
| D-18 | 가정: peer의 cwd를 읽거나 canonicalize 할 수 없으면(프로세스 종료, proc_pidinfo 거부, 경로 소멸) `caller_unavailable`로 fail closed 하고, 요청이 보낸 경로나 다른 checkout을 시도하지 않는다. | 가정: 엔지니어링 원칙 4·10 (qa-log D-18) |
| D-19 | 원칙 intake: ~/projects/oh-my-principle engineering/principles.md(commit 654485f)를 전부 읽었다. 원칙 4·9·10·15는 B 행으로, 2·7·8은 구조 결정(기존 attestation 경로 확장, 새 crate/의존성 없음)으로 번역했다. | 엔지니어링 원칙 문서 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | daemon을 켠 채 pane에서 직접 친 `codex`의 tool shell이 bare `hide workspace info`를 실행하면 자기 pane의 checkout과 같은 device_id/workspace_id/checkout_id/checkout_path와 capability 목록이 성공 JSON으로 돌아온다. | D-01, D-02, D-05 |
| B2 | 같은 shell에서 bare `hide browser open <url> --reveal`을 실행하면 그 checkout의 Workspace 사이드 패널에 Browser View가 생기고, 결과 JSON은 pane 호출자와 같은 모양(context, view_id, area_id, load)이며, reveal은 오늘처럼 checkout을 앞으로 가져오고 패널을 열 뿐 pane 포커스는 바꾸지 않는다. | D-03, D-05, D-06 |
| B3 | 같은 shell에서 받은 view id로 `hide view status`, `view select`, `view split`/`view move`, `view close`를 실행하면 각각 pane 호출자와 같은 결과로 동작하고, close 뒤 `hide view list`에 그 view가 없다. | D-03, D-05, D-06 |
| B4 | Claude Code pane과 pane 안의 사용자 shell에서 실행한 bare `hide` 명령은 지금처럼 pane attestation을 먼저 통과해 pane-bound capability를 받고, pane이 닫히거나 재생성되면 지금처럼 `pane_changed`로 무효화된다. checkout fallback은 pane attestation이 성공하면 실행되지 않는다. | D-02, D-05 |
| B5 | pane 자손도 아니고 cwd가 등록·연결된 어떤 checkout 안에도 없는 호출자는 bootstrap에서 `checkout_not_registered`와 다음 행동("등록된 프로젝트 checkout 안에서 실행하거나 Hide를 다시 연결")을 받고, 아무것도 화면에 나타나지 않는다. | D-05, D-10 |
| B6 | hided가 peer의 cwd를 읽거나 canonicalize 할 수 없으면 bootstrap은 `caller_unavailable`로 거절되고, 요청 본문의 어떤 경로도 대신 쓰이지 않으며 다른 checkout으로 넘어가지 않는다. | D-18 |
| B7 | checkout-bound capability로 명령을 보낼 때 그 checkout이 등록 해제됐거나 장치 연결이 끊겼으면 명령은 `checkout_not_registered`로 거절되고, 웹/데스크톱 렌더러가 없으면 오늘처럼 `views_unavailable`이다. 다른 checkout에 액션이 적용되는 일은 없다. | D-05, D-09 |
| B8 | 등록된 checkout이 중첩된 경우 cwd를 포함하는 가장 긴 등록 경로의 checkout이 선택되고, symlink 경로의 cwd는 canonical 경로로 비교된다. | D-11 |
| B9 | checkout-bound capability는 8시간 뒤, 미claim 30초 뒤, one-shot 호출자 종료 뒤에 만료되고 pane capability와 합쳐 64개를 넘으면 `capability_limit`로 거절된다. hook이 발급한 persistent checkout capability는 같은 장치·checkout의 다음 bootstrap에 재사용된다. | D-09 |
| B10 | SSH bridge를 거친 원격 호출자는 이 변경 전과 같은 pane attestation만 받는다. | D-12 |
| B11 | checkout-bound 발급과 모든 거절은 hided 진단 로그에 구조화 이벤트 하나(event 이름, device, checkout id, peer pid, reason)로 남고, 호출자 env의 경로·reference 바이트·pane id는 남지 않는다. 알림, 배너, 시트는 없다. | D-13 |
| B12 | 실제 설치본 검증에서 Claude Code pane과 daemon-on `codex` 각각이 B2와 B3의 전체 순서(open --reveal, 조작, close)를 완료하고, 테스트가 만든 pane과 view만 정리되며 다른 pane·view·workspace는 닫히거나 움직이지 않는다. | D-06 |
| B13 | 같은 검증에서 daemon 안 Codex SessionStart hook이 Workspace 안내문을 냈는지, 그 hook의 cwd가 스레드 cwd였는지를 관찰해 결과가 기록된다. 안내문이 없어도 B1~B3은 성립해야 한다. | D-14 |
| B14 | docs/ARCHITECTURE.md, docs/agent-hooks.md, docs/BROWSER_DISPLAYS.md가 pane-bound와 checkout-bound 두 attestation 경로와 각 credential이 기록·재검사하는 항목을 설명하고, "모든 명령이 attested pane을 요구한다"는 문장이 남아 있지 않다. | D-15 |
| B15 | 기존 hided·herdr-core 테스트 스위트(`scripts/verify-cargo.sh test`, `lint`)가 통과하고, checkout-bound 발급·거절·validate 경로에 대한 테스트가 호출자가 관찰하는 결과(JSON reason, 발급 여부)를 단언한다. | D-05, D-18 |

## Technical structure

- hided `pane_auth`: `Attestation`이 pane-bound와 checkout-bound 두 형태를 가진다. `serve()`는 `attest_local`이 실패하면 peer pid의 cwd를 hided가 직접 읽어 core에 checkout 조회를 요청하고, `validate`는 형태별로 재검사한다. bootstrap 요청 형식은 바뀌지 않는다.
- herdr-core `workspace_control`: pane id 대신 (device_id, canonical cwd)로 checkout을 찾는 조회를 추가하고, 기존 `workspace_control_query`의 연결·views·layout 부분을 공유한다. 액션 재시도 기록의 pane 키는 checkout-bound 호출자에 대해 capability 고유 키를 쓴다.
- 새 crate, 새 의존성, 새 프로세스, 새 소켓, 새 CLI 명령은 없다. Codex 쪽 파일은 건드리지 않는다.
- 문서: docs/ARCHITECTURE.md, docs/agent-hooks.md, docs/BROWSER_DISPLAYS.md.

## Risks

- 접근 경계 완화: 같은 사용자 계정의 어떤 프로세스든 등록된 checkout 안에 cwd를 두면 그 checkout의 Workspace view를 조작할 수 있다. 효과는 UI view 열기·이동·닫기로 한정되고 파일 쓰기·프로세스 실행은 없으며, cwd는 커널에서 읽으므로 호출자가 고를 수 없다. 이것이 review_profile을 high-risk로 둔 이유이고 리뷰어 판단 항목이다.
- daemon hook cwd 미확인: hook이 스레드 cwd를 못 보면 Codex는 안내문 없이 bare 명령만 된다. B13이 관찰해 기록하고, 안내문 복구는 별도 결정으로 남긴다.
- 라이브 검증 경계: 사용자의 실행 중 Hide와 Herdr 위에서 테스트 pane을 새로 열어 진행한다. 기존 pane·view·workspace는 만들지 않은 것을 닫거나 옮기지 않으며, 검증 산출물은 agents/runs/checkout-capability/ 아래에만 둔다.
- Codex upstream이 daemon 기본값을 되돌려도 이 경로는 남는다. pane 밖에서 도는 어떤 로컬 런타임에도 같은 답이 되므로 stopgap이 아니다.
- 사용자가 구현 전에 할 일: 없다.
