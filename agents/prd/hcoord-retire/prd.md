---
topic: "hcoord 은퇴: spawn과 계보 쓰기 이전, sasu 호출 교체, 원격 수신자 전달, 제거 (PRD B)"
status: "ready"
human_approval: "approved"  # user 2026-10-03 verbatim: ㅇㅇㅇ 둘다 approve~
review_profile: "high-risk"
review_rationale: "운영자 Mac과 기기의 LaunchAgent, 링크, kit 복사본, 홈 폴더를 실제로 치우는 되돌릴 수 없는 은퇴 동작을 수행하고, 원격 pane의 capability를 편지함 호출까지 넓혀 접근 경계를 바꾸며, 별도 저장소(sasu)의 호출 계약을 한 번에 교체하고, 계보 쓰기와 알림이 hided의 1초 갱신 경로와 키 입력 경로에 얹힌다."
source_intake: "agents/interview/hcoord-retire/qa-log.md"
created_at: "2026-10-04"
updated_at: "2026-10-04"
---

# PRD: hcoord 은퇴 (PRD B)

## Goal

hide를 쓰는 운영자와 그 에이전트들은 PRD A(`agents/prd/hcoord-delivery-v2/prd.md`)가 hided에 편지함, 전달, 감시를 만든 뒤에도 spawn과 계보 토큰 쓰기, sasu의 호출, 원격 기기의 편지 수신, 설치된 daemon을 위해 hcoord를 남겨 두고 있다.
이 PRD는 전체 4단계 중 3단계와 4단계로, 그 남은 일을 hide로 옮기고 hcoord를 완전히 걷어낸다(D-01).
이 변경 뒤 계보 토큰은 hided가 쓰고(D-09), 자식은 `hide agent spawn`으로 만들어지고 `hide request send --kind report`로 끝을 알리며(D-10, D-14), mini의 에이전트도 같은 편지함으로 편지를 받고(D-15), sasu는 hide만 쓰며(D-12), 이 Mac과 기기에 남은 hcoord 설치물은 kit이 한 릴리스 동안 치우고(D-17), 저장소의 hcoord 코드와 문서는 은퇴 코드와 이력만 남기고 사라진다(D-18).
이유는 운영자가 한 명이라 호환을 유지할 필요가 없고, 이중 시스템이 남아 있는 한 별도 daemon의 배포, 수명, 소음 비용이 계속되기 때문이다(D-12, D-13).

## Non-goals

- 사람 앞 request 화면(Inbox, ⌘K, 그래프), `request relay`와 `escalate`, 권한 증명, 초안 자동 정리, 감시 시각 표시, upstream 입력 가드 제안은 이번에 만들지 않는다. 결과: hcoord가 하던 부모 상위 전달과 30분 자동 사람 올림은 사라지고 사람 알림은 D-11의 두 경우뿐이다. 이 항목들은 추적 이슈 하나로 묶고 PRD B 구현 완료 때 다시 본다(D-06, D-11, D-20, D-23).
- sasu의 런 단위 전환 스위치, `sasu supervisor use hcoord`, `migrate-hcoord`, hcoord 백엔드 분기, `HCOORD_ANSWER`와 `HCOORD_RELAY` 프롬프트 분기는 만들지 않고 지운다. 결과: 두 시스템이 공존하는 기간이 없다(D-12).
- 옛 hcoord 장부를 새 hide가 읽거나 옮기는 코드와 테스트는 만들지 않는다. 결과: 전환 전에 활성 sasu 런, 열린 request, 활성 감시가 없어야 하고 그 시점은 운영자가 고른다. 열린 request만 이전하는 안(PRD A의 결정 10번)은 이 결정으로 대체되어 폐기되었다(D-02, D-13).
- 은퇴 동작은 옛 장부 폴더를 지우지 않고 롤백도 없다. 결과: 이름이 바뀐 폴더를 지우는 것은 운영자 몫이고 은퇴 중 실패해도 장부는 남아 잃는 것이 없다(D-13, D-24).
- hided 로그인 자동 시작은 추가하지 않는다. 결과: 재부팅 뒤 앱을 열기 전까지 hided와 감시가 없는 공백이 생기고 그동안 감시할 에이전트도 없다. 세 OS의 자동 시작 도구를 따로 다루게 되면 다시 본다(D-08, D-16).
- 기기 쪽에 편지를 쌓는 spool은 만들지 않고 mini에 hided를 상주시키지 않는다. 결과: 연결이 끊긴 동안 mini의 에이전트는 편지를 못 받고 편지는 이 Mac의 장부에서 대기한다(D-04, D-15).
- `hcoord` 이름의 호환 shim은 남기지 않는다. 결과: 스킬 등에 남은 `hcoord` 호출은 명령 없음 오류로 드러난다(D-17).
- 뺀 플래그 `--reconcile-pane`, `--resume-start`, `--session`과 원격용 자체 SSH spawn, 뺀 명령 `request relay`, `escalate`, `graph`, `events`는 만들지 않는다. 결과: 같은 `--intent` 재시도와 hide의 기기 시작 경로가 그 자리를 맡는다(D-10).
- Herdr를 수정하지 않고 Herdr 고유 기능을 막지 않는다(D-07).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 범위는 이행 3단계(spawn과 계보 토큰 쓰기를 hide로)와 4단계(sasu 호출 교체, mini 정리, 재부팅 뒤 감시 공백 판단, 명시적 완료 보고 설계, hcoord 제거)다. PRD A가 hided에 편지함, 전달, 감시를 만들고 hcoord와 공존한 뒤에 시작한다. 구현은 A를 운영해 본 뒤 시작하고 그때 B의 가정을 한 번 더 점검한다. | qa-log D-01, Q15에서 운영자가 재확인("추천대로"), PRD A 결정 41번 |
| D-02 | 열린 request만 새 장부로 옮기는 안(PRD A의 결정 10번)은 폐기되고 전환 전에 비워 두는 운영 전제와 옛 장부를 읽지 않는 D-13으로 대체되었다. | qa-log D-02(rejected), Q8 "ㅇㅇ" |
| D-03 | sasu의 hcoord 호출을 hide로 바꾸는 일은 hide 쪽이 모두 끝난 뒤 맨 마지막에 한다. | qa-log D-03, Q15 재확인 |
| D-04 | 원격 통일 대상은 hide가 보는 기본(운영) Herdr 서버 하나다. hcoord-test 세션과 e2e 잔재 서버는 소유를 확인한 뒤 hcoord 은퇴와 함께 정리한다. mini 감시는 이 Mac의 hided가 SSH로 붙는 현재 방식이고 mini에 hided를 상주시키지 않는다. | qa-log D-04, Q15 재확인 |
| D-05 | 명시적 완료 보고는 D-14로 설계한다(PRD A는 이를 B로 넘겼다). | qa-log D-05, Q9, PRD A 결정 42번 |
| D-06 | 사람 앞 request 화면, hcoord의 relay와 escalate 명령, 권한 증명, 초안 자동 정리는 이 PRD의 범위가 아니다. | qa-log D-06, Q15 재확인 |
| D-07 | hide는 Herdr 공개 API만 쓰고 Herdr를 수정하지 않으며 Herdr 고유 기능을 막지 않는다. 새 코드는 OS 중립 Rust이고 OS 분기는 hide-platform에만 둔다. Rust 재작성이며 hide CLI 하위명령으로 제공한다. | qa-log D-07, Q15 재확인 |
| D-08 | 재부팅 뒤 감시 공백을 허용한다(D-16으로 확정). | qa-log D-08, Q12 "ㅇㅇㅇ 허용", PRD A 결정 15번 |
| D-09 | 계보 토큰 계약(parent_pane, parent_machine, child_session, parent_session)은 그대로 두고 쓰는 주체만 hcoord에서 hided로 바꾼다. 읽는 쪽(wire.rs, sidebar.rs)은 바꾸지 않는다. hcoord의 5초(로컬)와 60초(원격) 재기록 폴링은 새 타이머 없이 hided가 이미 1초마다 하는 agent.list 갱신에 얹어 대체하고, 로컬의 5초 간격을 따로 보장하지 않으며 1초 갱신과 이벤트에 맡긴다. 장부의 계보와 토큰이 다른 pane만 쓰고 세션이 바뀐 pane은 지우며 시작과 재연결 때는 전체를 한 번 맞춘다. spawn으로 자식을 만들 때는 생성 직후 바로 쓴다. 틱당 비용은 바뀐 pane 수에 비례한다. | qa-log D-09, Q1 "ㅇㅇㅇ 그렇게 ㄱㄱㄱ" |
| D-10 | 명령 표면: agent register [--check], list, show, end는 유지(sasu와 Fork가 쓰는 형태)하고 watch는 PRD A의 hide watch에 assign(관찰자 바꾸기)을 더하며 `--interval`은 없다. agent spawn은 --parent --name --intent --kind --repo --branch [--path] --no-watch와 -- 뒤 네이티브 인자만 지원한다. --reconcile-pane, --resume-start, --session, 원격용 자체 SSH spawn은 뺀다(같은 --intent 재시도가 기존 자식을 이어서 고치고 원격 spawn은 hide의 기기 시작 경로를 쓴다). request relay, escalate, graph, events는 뺀다. status는 hide status로 대응한다. 에이전트 스킬이나 프롬프트가 뺀 플래그를 안내하는지는 구현 때 rg로 확인한다. | qa-log D-10, Q2 "ㅇㅇㅇㅇ 그렇게 하자꾸나!" |
| D-11 | hcoord 은퇴로 사라지는 사람 알림을 기존 hided 경로(휴대폰 Web Push, Herdr 알림 호출)를 재사용해 두 경우에만 유지한다: 부모가 응답하지 않는 감시 경고와 기한이 지난 미전달 편지. 같은 원인당 한 번이고 상한을 두며 새 화면은 만들지 않는다. 사람이 답하는 흐름은 후속 작업이다. | qa-log D-11, Q6 "ㅇㅇㅇ글래" |
| D-12 | sasu는 hide만 쓰고 hcoord 관련 레거시는 모두 제거한다. 런 단위 전환 스위치, supervisor use hcoord와 migrate-hcoord, hcoord 백엔드 분기, HCOORD_ANSWER와 HCOORD_RELAY 프롬프트 분기는 만들지 않고 지운다. 운영자 한 명이라 호환을 유지하지 않는다. sasu는 별도 저장소의 변경이고 hide의 새 명령 표면이 먼저 설치되어 있어야 호출할 수 있다. | qa-log D-12, Q7 "sasu는그냥 hide만써야지.. hcoord관련 레거시 다 제거할거야 어차피 나밖에 안써서 그냥 막 바꿔도 돼" |
| D-13 | 전환은 한 번에 하고 새 hide는 옛 hcoord 장부를 읽지 않는다. 전환 전에 활성 sasu 런과 열린 request와 활성 감시가 없는 상태가 운영 전제이며 운영자가 시점을 고른다. 이전 코드와 테스트는 만들지 않는다. 은퇴 단계가 옛 폴더를 지우지 않고 열린 항목이 없을 때만 hcoord.retired-날짜로 이름만 바꾸고 열린 것이 남아 있으면 감지해서 멈추고 알린다. 지우는 것은 운영자가 한다. | qa-log D-13, Q8 "ㅇㅇ" |
| D-14 | 자식의 완료 보고는 새 명령 없이 hide request send --kind report로 한다(sasu의 implement report가 내부에서 부른다). 감시는 그 보고 편지가 부모에게 전달됨으로 확정되는 순간 자동으로 끝나며 부모의 ack를 기다리지 않는다. 부모가 더 지켜보려면 hide watch start로 다시 건다. 같은 intent의 중복 전송은 한 통이다. 보낸 쪽이 감시 대상이 아니면 일반 편지로만 동작하고 보고 전에 자식이 종료하면 대상 이탈로 끝난다. | qa-log D-14, Q9 "ㅇㅇㅇㅇㅇ" |
| D-15 | 원격 기기(mini 등) 에이전트가 편지를 받는 경로는 기존 SSH reverse forward와 workspace-bridge를 편지함 호출(request, inbox, hook)까지 넓히는 것이다. 장부는 이 Mac의 hided 하나이고 기기 쪽 spool은 만들지 않는다. 초인종은 이 Mac의 hided가 SSH로 mini의 Herdr에 보낸다. 연결이 끊긴 동안 mini의 hook은 2초 상한 안에 편지 없이 끝나고 편지는 대기로 남는다. 원격 pane의 capability에 request와 inbox 계열을 허용하고 발신자와 종류는 그 capability가 가리키는 pane에서 정한다. 원격 hook의 접속 발견 방식은 기존 workspace-bridge와 맞춰 구현 때 확정한다. | qa-log D-15, Q11 "ㅇㅇㅇ 그러자", PRD A 결정 49번 |
| D-16 | hcoord LaunchAgent가 사라진 뒤 재부팅하면 앱을 열기 전까지 hided와 감시가 없는 공백을 허용하고 hided 로그인 자동 시작은 추가하지 않는다. 재부팅하면 Herdr 서버도 같이 죽어 감시할 에이전트가 없고 앱이 Herdr 서버와 hided를 함께 시작한다. | qa-log D-16, Q12 "ㅇㅇㅇ 허용" |
| D-17 | 제거 범위: plugins/hcoord 전체, pnpm workspace와 lock 항목, hide-kit의 hcoord 부품(hcoord.rs와 local, device, record, layout의 참조), 원격 payload와 kit.rs의 hcoord, fork.rs의 hcoord 호출(hide 내부 호출로 대체), 데스크톱 패키징과 env, cli와 e2e의 hcoord, 문서의 hcoord 항목, hcoord 때문에만 요구하던 기기의 Node 22.12 의존(구현 때 다른 용도가 없는지 확인). sasu 저장소의 hcoord 호출 코드와 문서도 지운다. 설치물은 kit의 hcoord 부품을 한 릴리스 동안만 은퇴 동작으로 바꿔 치운다: daemon 정지, com.hcoord.daemon LaunchAgent bootout과 plist 삭제, hide 소유일 때만 hcoord 링크 제거, kit 복사본 삭제, 열린 항목이 없을 때만 홈 폴더 이름 변경. 기기에도 같은 동작이 적용된다. 은퇴 코드는 모든 기기가 한 번씩 지난 뒤 다음 릴리스에서 제거하고 시점은 운영자가 정한다. 호환 shim은 남기지 않는다. | qa-log D-17, Q13 "ㅇㅇㅇㅇ" |
| D-18 | 완료 기준 11개: 1 spawn이 pane과 에이전트(필요 시 worktree)를 만들고 토큰이 쓰여 위임 관계가 화면에 나타나며 감시가 자동으로 걸림(--no-watch로 끔), 2 같은 intent 재시도는 기존 자식을 이어서 고침, 3 토큰은 hided가 쓰고 읽는 쪽 코드는 불변이며 토큰 계약 테스트가 통과하고 재기록은 새 타이머 없이 1초 agent.list 갱신에서 바뀐 pane만 처리, 4 register [--check], list, show, end가 sasu와 Fork가 쓰는 형태로 동작, 5 request send --kind report가 전달됨 확정 순간 감시 자동 종료, 6 부모 무응답 감시 경고와 기한 지난 미전달 편지는 사람에게 같은 원인당 한 번 알림과 상한, 7 mini 에이전트가 reverse forward로 편지를 받고(hook 2초 상한) 끊기면 대기하며 이중 전달 없음, 8 sasu가 hide만 써서 implement dispatch, block, report가 동작하고 코드와 문서에 hcoord 호출이 없음, 9 kit 은퇴 동작이 이 Mac과 기기에서 한 번씩 동작(열린 항목이 있으면 멈추고 알림), 10 rg hcoord 결과가 은퇴 코드와 이력 문서뿐, 11 새 작업은 락 밖과 상한을 지키고 키 입력 경로와 1초 갱신 비용이 PRD A 측정보다 늘지 않음. 증명은 격리 Herdr e2e, 상태 기계 단위 테스트, 성능 측정이다. | qa-log D-18, Q14 "ㅇㅇㅇㅇ" |
| D-19 | 사실: sasu가 부르는 hcoord 명령은 agent register [--check](machine, host-scope, session, instance, name, parent, project), agent list, show, end, watch start와 assign(--observer, --actor, --interval, --expected-generation), request send(kind block), status --json이다. sasu는 agent spawn을 쓰지 않고 pane은 직접 띄운 뒤 등록한다. sasu 참조는 약 180곳이고 런 상태에 hcoord 참여자 ID가 기록되며 hide의 Fork는 register와 계보 게시를 hcoord에 요청한다. | qa-log D-19(조사: sasu cli/src/implement/hcoord.ts, cli/src/supervisor/commands.ts, herdr-core/src/fork.rs) |
| D-20 | 사실: hcoord의 request relay는 부모가 자식의 request를 자기 상위로 넘기는 명령 계통 전달이고 안 닫히면 사람에게 올라간다. escalate는 답 없는 request를 사람에게 올리며 데몬이 기본 30분에 자동으로도 올린다. sasu의 implement escalate는 다른 기능(막힌 구현자 진단과 교체)이다. | qa-log D-20(조사: plugins/hcoord/src/hcoord/service.ts) |
| D-21 | 사실: hide 저장소의 hcoord 참조는 plugins/hcoord, hide-kit(hcoord.rs 672줄과 local, device, record, layout, process, labels, lib), herdr-core(remote/host.rs payload, kit.rs, fork.rs, runtime.rs, wire.rs, model.rs, sidebar.rs 주석과 테스트), 데스크톱 패키징과 env, cli, e2e, 문서와 CONTRIBUTING, pnpm workspace와 lock이다. contracts/snapshot-wire-enums.json과 design/hide-screens.pen에도 이름이 보여 구현 때 확인한다. | qa-log D-21(rg hcoord 조사) |
| D-22 | 사실: hook은 각 기기에서 런타임이 프롬프트 제출 때 실행하는 명령(hide-agent-hooks, kit이 기기에도 설치)이며 SessionStart hook이 hide workspace bootstrap으로 capability를 받는다. mini의 hide CLI는 SSH reverse forward와 workspace-bridge로 이 Mac의 hided에 붙지만 이는 Workspace 제어용이고 편지함에는 쓰이지 않는다. 원격 hook의 접속 발견 방식은 확인하지 못했다(조사 보고 기준, 직접 재확인하지 않음). | qa-log D-22(hide-agent-hooks/src/workspace_context.rs, docs/ARCHITECTURE.md) |
| D-23 | 후속 작업(relay와 escalate, 사람 앞 request 화면, 권한 증명, 초안 자동 정리, 감시 시각 표시, upstream 입력 가드 제안)은 추적 이슈 하나로 묶는다. 이슈는 운영자 지시로 미리보기 없이 바로 만든다. 재검토 시점은 PRD B 구현 완료다. | qa-log D-23, Q15 "추천대로. 근데 issue 스킬은 미리보기 안하고 걍 바로 올리도록 해. 나 자러가야되서 검토못하니까" |
| D-24 | 은퇴 순서와 복구: 사전 점검(활성 런, 열린 request, 활성 감시)이 하나라도 걸리면 아무것도 바꾸지 않고 이유와 다음 동작을 알리고 멈춘다. 이후 daemon 정지, LaunchAgent 제거, 링크와 kit 복사본 제거, 폴더 이름 변경 순으로 하며 각 단계는 이미 끝났는지 확인하는 멱등 동작이다. 중간에 실패하면 실패한 단계 이름을 진단 로그와 설정 화면에 남기고 다음 실행이 그 단계부터 이어간다. 롤백은 없고 장부는 지우지 않아 잃는 것이 없다. 연결되지 않은 기기는 다음 연결 때 같은 동작을 수행한다. | qa-log D-24, Q15 "추천대로" |
| D-25 | 편지 전달 장애는 PRD A의 결정 44번, 34번, 47번을 그대로 따른다. 전달됨은 hook이 출력한 뒤 확정 호출을 한 순간이고 확정 전에 중단되면 같은 id로 다시 주입된다. 완료 보고 감시는 보고 편지가 전달됨으로 확정될 때만 끝난다. | qa-log D-25, Q15 "추천대로" |
| D-26 | 미전달 기한: 편지가 대기 상태로 60분(PRD A의 request 기한과 같은 값)을 넘겨 전달되지 못하면 미전달로 바꾸고 운영자에게 알린다. 새 편지를 만들지 않고 장부와 CLI에만 상태를 표시한다. 만료 상태는 쓰지 않고 이 기한이 미전달 기준이다. 값은 60분으로 고정한다. | qa-log D-26, Q19 "60분" |
| D-27 | 두 단계 무활동 경고의 재시작: 첫 경고 시각과 경고 횟수를 장부에 저장한다. 두 번째 경고는 첫 경고 후 60분이다. hided가 재시작돼도 이어지고 활동이 생기면 초기화된다. PRD A의 결정 44번과 48번을 이 규칙으로 정합시킨다. | qa-log D-27, Q15 "추천대로" |
| D-28 | 사람 알림 상한: 키는 감시 대상(또는 편지 id)과 원인 종류다. 같은 키는 Push와 Herdr 두 채널을 합쳐 한 번만 보내고 최대 2회이며 활동이 다시 생기면 초기화된다. 한 채널이 실패하면 다른 채널로 보내고 둘 다 실패하면 진단 로그만 남기고 재시도하지 않는다. | qa-log D-28, Q15 "추천대로" |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | `hide agent spawn --parent here --name --intent --kind --repo --branch [--path]`를 실행하면 새 pane과 에이전트가 만들어지고(필요하면 worktree도) 계보 토큰이 생성 직후 바로 쓰여 hide 화면에 부모-자식 위임 관계가 나타나며 감시가 자동으로 걸린다. | D-09, D-10, D-18 |
| B2 | `--no-watch`로 spawn하면 감시가 걸리지 않는다. `--` 뒤의 인자는 네이티브 인자로 그대로 전달된다. | D-10, D-18 |
| B3 | 같은 부모가 같은 `--intent`로 spawn을 다시 실행하면 새 자식을 만들지 않고 기존 자식을 이어서 고친다(식별은 부모와 intent의 쌍이며 다른 부모의 같은 intent는 별개의 자식이다). 그래서 `--reconcile-pane`, `--resume-start`, `--session`은 없고 쓰면 알 수 없는 플래그 오류가 난다. | D-10, D-18 |
| B4 | 원격 기기에 자식을 띄울 때는 spawn 전용 SSH 경로 없이 hide가 이미 가진 기기 시작 경로를 쓴다. | D-10 |
| B5 | 계보 토큰의 네 키(parent_pane, parent_machine, child_session, parent_session) 계약은 그대로이고 읽는 쪽(wire.rs, sidebar.rs)은 바뀌지 않으며 기존 토큰 계약 테스트가 그대로 통과한다. 쓰는 주체만 hided다. | D-09, D-18 |
| B6 | 토큰 재기록은 새 타이머 없이 기존 1초 `agent.list` 갱신과 이벤트에서 일어나고 로컬에 5초 같은 별도 재기록 간격은 없다. 장부의 계보와 토큰이 다른 pane만 쓰고, 세션이 바뀐 pane의 토큰은 지우며, hided 시작과 재연결 때는 전체를 한 번 맞춘다. 틱당 비용은 바뀐 pane 수에 비례하고 토큰 쓰기는 Runtime 락 밖에서 한다. | D-09, D-18 |
| B7 | `hide agent register [--check]`, `list`, `show`, `end`가 sasu와 Fork가 쓰는 형태(machine, host-scope, session, instance, name, parent, project)로 동작한다. | D-10, D-18, D-19 |
| B8 | hide의 Fork가 register와 계보 게시를 hcoord 대신 hide 내부 호출로 한다. | D-17, D-19 |
| B9 | `hide watch assign`으로 감시의 관찰자를 바꿀 수 있고 주기 옵션 `--interval`은 없다. `hide status`가 hided의 응답 여부를 보여 준다. | D-10, D-19 |
| B10 | hide에는 `request relay`, `escalate`, `graph`, `events` 명령이 없다. 그 기능이 사라진 결과로 답 없는 request의 자동 사람 올림도 없다. | D-06, D-10, D-20 |
| B11 | 자식이 `hide request send --kind report`로 부모에게 완료 보고를 보내고 그 편지가 부모에게 전달됨으로 확정되는 순간 그 대상의 감시가 자동으로 끝난다. 부모의 ack는 기다리지 않는다. | D-14, D-18, D-25 |
| B12 | 확정 전에 중단되면 같은 편지 id로 다시 주입되고 감시는 끝나지 않으며 전달됨이 확정될 때만 끝난다. | D-25 |
| B13 | 같은 intent의 보고 중복 전송은 한 통이다. 보낸 쪽이 감시 대상이 아니면 일반 편지로만 동작하고, 보고 전에 자식이 종료하면 감시는 대상 이탈로 끝난다. 부모가 더 지켜보려면 `hide watch start`로 다시 건다. | D-14 |
| B14 | 사람에게 가는 알림은 두 경우뿐이다: 부모가 응답하지 않는 감시 경고와 기한이 지난 미전달 편지. 기존 휴대폰 Web Push와 Herdr 알림 호출을 쓰고 새 화면은 만들지 않는다. | D-11, D-18 |
| B15 | 알림 키는 감시 대상(또는 편지 id)과 원인 종류다. 같은 키는 두 채널을 합쳐 한 번만 보내고 최대 2회이며 활동이 다시 생기면 초기화된다. 한 채널이 실패하면 다른 채널로 보내고 둘 다 실패하면 진단 로그만 남기고 재시도하지 않는다. | D-11, D-28 |
| B16 | 편지가 대기 상태로 60분을 넘겨 전달되지 못하면 미전달이 되고 운영자에게 알림이 가며, 새 편지는 만들어지지 않고 장부와 `hide` CLI 조회에만 상태가 표시된다. `만료` 상태는 쓰이지 않는다. 기준은 PRD A의 request 기한과 같은 60분이다. | D-11, D-26 |
| B17 | 감시 대상이 20분 무활동이면 PRD A의 정의대로 부모에게 첫 경고가 가고, 같은 정지 상태가 이어지면 첫 경고 후 60분에 두 번째 경고가 간다(총 2회). 첫 경고 시각과 경고 횟수가 장부에 저장되어 hided가 재시작돼도 두 번째 경고 시점이 이어지며, 대상에 활동이 생기면 그 대상의 시각과 횟수가 모두 초기화되어 다음 무활동에서 첫 경고부터 다시 시작한다. | D-27 |
| B18 | mini의 에이전트가 기존 SSH reverse forward와 workspace-bridge를 넓힌 경로로 `hide request`, `hide inbox`, `UserPromptSubmit` hook을 호출해 이 Mac의 hided에 닿고 편지를 받는다. 장부는 이 Mac의 hided 하나이고 기기 쪽 spool은 없다. | D-15, D-18 |
| B19 | mini 에이전트에게 가는 초인종은 이 Mac의 hided가 SSH로 mini의 Herdr에 보낸다. 로컬과 같은 전달 코드 경로와 PRD A의 초인종 조건을 따른다. | D-15 |
| B20 | 연결이 끊긴 동안 mini의 hook은 2초 상한 안에 편지 없이 끝나 프롬프트 제출을 막지 않고, 편지는 장부에서 대기로 남으며, 연결이 돌아온 뒤 같은 편지가 이중 전달 없이 한 번 전달된다. | D-15, D-18, D-25 |
| B21 | 원격 pane의 capability가 request와 inbox 계열 호출을 허용하고, 편지의 발신자와 종류는 그 capability가 가리키는 pane에서 정해진다. | D-15 |
| B22 | sasu가 hide만 써서 `implement dispatch`, `implement block`, `implement report`가 동작한다. sasu가 쓰던 register, list, show, end, watch start와 assign, request send(kind block), status는 hide 명령으로 대응된다. | D-12, D-18, D-19 |
| B23 | sasu 코드와 문서에 hcoord 호출이 없고, 런 단위 전환 스위치, `supervisor use hcoord`, `migrate-hcoord`, hcoord 백엔드 분기, `HCOORD_ANSWER`와 `HCOORD_RELAY` 프롬프트 분기가 없다. | D-12, D-18 |
| B24 | sasu 변경은 sasu 저장소의 별도 변경이고 hide의 새 명령 표면이 설치된 뒤에 적용되며 hide 쪽이 모두 끝난 뒤 맨 마지막에 한다. | D-03, D-12 |
| B25 | 새 hide는 옛 hcoord 장부를 읽지 않고 장부 이전 코드와 테스트가 없다. | D-02, D-13 |
| B26 | 은퇴 동작을 실행하면 먼저 활성 런, 열린 request, 활성 감시를 점검하고 하나라도 있으면 아무것도 바꾸지 않고 이유와 다음 동작을 알리고 멈춘다. | D-13, D-18, D-24 |
| B27 | 점검을 통과하면 daemon 정지, `com.hcoord.daemon` LaunchAgent bootout과 plist 삭제, hide 소유일 때만 hcoord 링크 제거, kit 복사본 삭제, 열린 항목이 없을 때만 홈 폴더를 `hcoord.retired-날짜`로 이름 변경하는 순서로 진행되고 옛 폴더는 지워지지 않는다. | D-13, D-17, D-24 |
| B28 | 은퇴의 각 단계는 이미 끝났는지 확인하는 멱등 동작이라 다시 실행해도 같은 결과다. 중간에 실패하면 실패한 단계 이름이 진단 로그와 설정 화면에 남고 다음 실행이 그 단계부터 이어간다. 롤백은 없고 장부는 남아 있다. | D-24 |
| B29 | 은퇴 동작은 이 Mac과 기기에서 각각 한 번씩 동작한다. 연결되지 않은 기기는 다음 연결 때 같은 동작을 수행한다. | D-17, D-18, D-24 |
| B30 | 은퇴 동작과 hcoord 부품 코드는 한 릴리스 동안만 있고 모든 기기가 한 번씩 지난 뒤 다음 릴리스에서 제거되며 그 시점은 운영자가 정한다. `hcoord` 이름의 호환 shim은 없어 남은 호출은 명령 없음 오류로 드러난다. | D-17 |
| B31 | 은퇴와 함께 hcoord-test 세션과 e2e 잔재 서버가 소유 확인 뒤 정리되고, hide는 기본(운영) Herdr 서버 하나만 감시하며 mini는 이 Mac의 hided가 SSH로 붙어 본다. mini에 hided는 상주하지 않는다. | D-04 |
| B32 | 재부팅 뒤 hided 자동 시작은 없고 앱을 열면 앱이 Herdr 서버와 hided를 함께 시작하며, 그 전까지의 감시 공백은 허용된다. | D-08, D-16 |
| B33 | `plugins/hcoord/` 전체와 pnpm workspace와 lock의 hcoord 항목이 저장소에서 없다. | D-17, D-21 |
| B34 | hide-kit의 hcoord 부품(`hcoord.rs`와 local, device, record, layout의 참조), 원격 payload와 `kit.rs`의 hcoord, `fork.rs`와 `runtime.rs`와 `wire.rs`와 `model.rs`와 `sidebar.rs`의 hcoord 참조와 테스트가 은퇴 동작 부품을 빼고 없다. | D-17, D-21 |
| B35 | 데스크톱 패키징(`package.mjs`, `smoke-package.mjs`), `env.ts`, `cli.ts`, e2e의 hcoord fixture와 spec이 없다. | D-17, D-21 |
| B36 | `docs/README.md`의 hcoord 항목과 `UI_BEHAVIOR.md`, `BUILD.md`, `dev-runtime.md`, `status-model.md`, `CONTRIBUTING.md`의 hcoord 설명이 같은 변경에서 지워지고 sasu 저장소의 hcoord 문서도 지워진다. | D-17, D-21 |
| B37 | hcoord 때문에만 요구하던 기기의 Node 22.12 의존이 없다(다른 용도가 있으면 남기고 그 근거를 구현 기록에 남긴다). `contracts/snapshot-wire-enums.json`과 `design/hide-screens.pen`의 hcoord 이름은 확인 뒤 정리된다. | D-17, D-21 |
| B38 | 저장소에서 `rg hcoord`를 하면 결과가 은퇴 동작 코드와 이력 문서뿐이고 `plugins/hcoord/`, 패키징, e2e, 현행 문서에는 없다. | D-18 |
| B39 | 새 작업(토큰 재기록, 알림, 원격 편지함 호출, 은퇴 동작)은 Runtime 락 밖에서 하고 상한을 가지며, 키 입력 경로와 1초 갱신의 비용은 PRD A 측정보다 늘지 않는다. | D-18 |
| B40 | 새 코드는 OS 중립 Rust이고 OS 분기는 hide-platform에만 있으며 hide는 Herdr 공개 API만 쓰고 Herdr를 수정하지 않는다. 모든 새 명령은 `hide` CLI 하위명령이다. | D-07 |
| B41 | 편지 전달 장애 계약은 PRD A와 같다: 전달됨은 hook 출력 뒤 확정 호출 순간이고, 확정 전 중단은 같은 id로 재주입되며, hook은 2초 안에 응답이 없으면 편지 없이 끝난다. | D-15, D-25 |

## Technical structure

- PRD A가 만든 hided의 장부, 편지 상태 기계, 감시, 초인종, hook 전달, 단일 내부 전달 함수 위에 얹는다. 새 저장소나 새 외부 서비스는 없다. 장부에는 두 단계 경고의 첫 경고 시각과 횟수, 미전달 상태 표시(60분 기준)가 더해진다(D-26, D-27).
- herdr-core에 계보 토큰 쓰기가 들어간다. 기존 1초 `agent.list` 갱신에 얹히고 읽는 쪽 wire.rs와 sidebar.rs는 불변이며, 토큰 쓰기는 락 밖이다(D-09). hide CLI에 `agent register/list/show/end/spawn`, `watch assign`, `status`, 그리고 `request send --kind report`가 더해지고 Fork는 hide 내부 호출을 쓴다(D-10, D-14, D-17).
- 감시 종료는 보고 편지의 전달됨 확정 사건에 묶인다. 사람 알림은 기존 hided의 Web Push와 Herdr 알림 호출을 재사용하고 키별 중복 제거와 상한을 갖는다(D-11, D-14, D-28).
- 원격 수신자 전달은 기존 SSH reverse forward와 workspace-bridge를 편지함 호출까지 넓히는 접근 경계의 변화다. 원격 pane capability에 request와 inbox 계열이 더해지고 장부는 이 Mac의 hided 하나이며 기기 쪽 저장소는 없다(D-15).
- hide-kit의 hcoord 부품은 한 릴리스 동안 멱등한 은퇴 동작(사전 점검, daemon 정지, LaunchAgent 제거, 링크와 kit 복사본 제거, 폴더 이름 변경)으로 바뀌고 다음 릴리스에서 지워진다. 기기에도 같은 동작이 적용된다(D-17, D-24).
- 저장소 제거 범위는 plugins/hcoord, pnpm workspace와 lock, hide-kit, herdr-core, 데스크톱 패키징과 env, cli, e2e, 문서다. sasu 저장소의 호출 코드와 문서 제거는 별도 저장소의 변경이며 hide의 새 명령 표면 설치 뒤에 적용한다(D-12, D-17, D-21).
- Herdr 계약은 바꾸지 않는다(D-07).

## Risks

- 이 PRD의 구현은 PRD A 구현이 끝나고 운영해 본 뒤에 시작하며, 그때 B의 가정(PRD A의 장부, 편지, 감시, hook 계약이 인터뷰 때 알던 모습 그대로인지)을 한 번 더 점검한다(D-01).
- PRD A와의 문구 차이: D-27의 경고 재시작 규칙은 PRD A의 구현 중 확정 항목을 채우는 값이다. 봉인된 PRD A 본문에는 결정 44번과 48번의 문구가 그대로 있다. 미전달 기한은 D-26이 PRD A의 request 기한 60분과 같은 값으로 정해 둘 사이에 차이가 없다(Q19). qa-log는 PRD A를 `gate reopen` 없이 고치지 않고 PRD B의 결정으로만 기록한다고 정했고, 경고 재시작 문구를 PRD A 승인 때 함께 반영할지는 그때 운영자가 정한다(Q15).
- 은퇴 동작은 운영자의 LaunchAgent, 링크, kit 복사본, 홈 폴더 이름을 바꾸는 파괴적 변경이다. 장부는 지우지 않고 사전 점검이 걸리면 아무것도 바꾸지 않으며 롤백이 없어 단계 멱등과 이어가기에 의지한다(D-13, D-24). 은퇴 검증은 격리 HOME과 격리 Herdr에서만 하고 `/Applications/hide.app`과 운영자의 pane, 서버는 건드리지 않으며, 이 Mac과 mini의 실제 은퇴 실행은 운영자가 고른 전환 시점에 운영자가 한다.
- mini는 무인 운영 중이다. mini에 대한 라이브 확인은 상태를 먼저 읽고 하며 프로세스를 먼저 종료하지 않고, 이 Mac이 꺼져 있으면 mini는 감시와 전달을 받지 못한다(D-04, D-15, D-16).
- 원격 편지함 호출은 원격 pane capability의 허용 범위를 넓힌다. 발신자와 종류는 capability가 가리키는 pane에서 정하고 세션 단위 증명과 위조 방지는 범위 밖이다(D-06, D-15).
- 두 채널이 모두 실패한 사람 알림은 진단 로그만 남고 재시도하지 않아 운영자가 모를 수 있다. 알고 받아들인 위험이다(D-28).
- sasu는 별도 저장소이고 참조가 약 180곳이라 변경이 크다. 호환을 유지하지 않으므로 sasu 변경과 hide 설치의 순서가 어긋나면 sasu의 호출이 실패한다(D-12, D-19).
- 구현 중 확정: 원격 hook이 이 Mac의 hided에 닿는 접속 발견 방식(포트나 환경값)은 기존 workspace-bridge와 맞춰 정한다(D-15, D-22).
- 구현 중 확정: 에이전트 스킬이나 프롬프트가 뺀 플래그(`--resume-start` 등)를 안내하는지 `rg`로 확인하고 고친다(D-10).
- 구현 중 확인: 기기의 Node 22.12 의존에 다른 용도가 있는지, `contracts/snapshot-wire-enums.json`과 `design/hide-screens.pen`의 hcoord 이름의 처리(D-17, D-21).
- 증명 경계: 격리 Herdr e2e, 상태 기계 단위 테스트, 성능 측정(키 입력 경로와 1초 갱신, `docs/PERFORMANCE_TESTING.md`)으로 한다. 라이브 증명은 정확히 식별한 후보 PID와 격리 서버에서만 한다(D-18).
- 운영자가 따로 해 줄 일: 전환 시점 고르기(활성 런, 열린 request, 활성 감시가 없을 때), 이름이 바뀐 `hcoord.retired-날짜` 폴더 삭제, 은퇴 코드를 다음 릴리스에서 지울 시점 결정(D-13, D-17). 자격 증명, 계정, 결제는 없다.
- 가정(에이전트 소유, 운영자가 거부할 수 있음): 같은 `--intent` 재시도의 식별은 PRD A의 편지 intent와 같은 방식으로 부모와 intent의 쌍이다. qa-log는 식별 범위를 따로 정하지 않았다(D-10).
- 후속 작업 이슈는 운영자 지시로 미리보기 없이 만들도록 정해져 있고(D-23) 이 PRD를 쓰는 과정에서는 만들지 않았다.
- 전달 방식: agents/config.json의 기본(PR 전달, worktree 실행)을 따르며 qa-log에 이를 바꾸는 결정은 없다. 이는 에이전트의 가정이다.
- 원칙 intake: 선언된 원칙 저장소(oh-my-principle, commit 654485f)의 engineering/principles.md와 design/principles.md를 전문 읽었다. engineering rule 1, 4, 10, 11, 14, 15는 제거, 멱등, 상한, 실패 경로 행(B3, B13, B15, B23, B28, B30, B39)으로 옮겼다. design은 설정 화면에 실패한 은퇴 단계 이름을 남기는 B28 한 곳에만 닿고 rule 13에 따라 운영자가 행동할 수 있는 단계 이름 한 줄 밖의 새 화면은 만들지 않는다. 프로젝트 규칙(CLAUDE.md의 락 밖, 상한, 틱 비용)이 우선한다. 이는 에이전트의 가정이다.
