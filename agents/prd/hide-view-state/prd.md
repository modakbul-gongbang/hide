---
topic: "Hide view state: hide-owned tab and focus selection, retained terminal views, one core initialization per launch"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "Moves the authority for visible tab and keyboard focus from herdr's round trip into the core, changes launch sequencing and snapshot delivery; no data, credential, billing, or destructive action, and the one live effect is focusing tabs inside a throwaway workspace."
source_intake: "current conversation"
created_at: "2026-09-04"
updated_at: "2026-09-04"
---

# PRD: Hide view state

## 1. Summary

herdr와 hide 사이의 권위 경계를 "토폴로지는 herdr, 뷰 상태는 hide"로 긋고, 그 경계 위에서 탭 전환, pane 포커스, 시작 시간을 네이티브 반응성으로 되돌린다.

코드에서 확인된 느림의 원인은 소켓 왕복 자체가 아니다.
탭을 전환하면 core가 현재 layout을 비우고(`None`), 셸은 균등 분할 격자를 임시로 그리며, 탭 identity가 바뀐 캔버스가 터미널 뷰를 통째로 새로 만들어 빈 버퍼로 시작하고, 새 뷰의 크기 보고가 herdr에 resize를 보내 그제야 전체 프레임이 다시 온다.
한 번의 전환에 빈 화면, 균등 격자, 실제 레이아웃의 세 단계가 지나가고 스크롤백은 매번 사라진다.
pane 포커스는 herdr의 확인 이벤트가 올 때까지 시각적으로 움직이지 않는다.
시작할 때는 core를 두 번 만들어 `session.snapshot`, 구독, git 카탈로그 구성을 두 번 하고, 첫 attach는 24x80으로 시작해 resize로 두 번째 전체 프레임을 부른다.

이 PRD 뒤에는 로컬 세션의 모든 탭 layout이 snapshot에 함께 있어 탭 전환이 클라이언트 선택이 되고, 터미널 뷰는 탭 사이에서 유지되며, 포커스는 클릭한 프레임에 옮겨 가고 herdr에는 비동기로 통보한다.
그려지는 기하는 언제나 herdr가 이미 적용한 layout이므로 PTY 크기를 두고 herdr와 다투지 않는다.
시작은 core 초기화 한 번으로 끝나고, 첫 attach는 뷰가 보고한 실제 크기로 한다.

Approval checklist:

- 권위 경계 자체: herdr가 pane 존재, 분할 기하, 줌, cwd, 에이전트 생명주기, PTY를 소유하고 hide가 보이는 탭, 키보드 포커스 pane, 패널 가시성, 글자 배율을 소유한다 (R1).
- 구조 변경: 로컬 snapshot이 탭마다 layout을 싣고, 터미널 뷰가 탭 전환에 유지되며, core 초기화가 한 번이 되는 것 (section 5).
- 줌, 분할, 닫기는 계속 herdr의 확인을 기다린다는 비목표와 그 근거 (section 3).
- 성능 예산과 측정 방법: 방문한 탭 전환과 포커스 이동은 한 프레임 안에, 시작은 첫 터미널 프레임까지의 시간을 기준선 대비 기록 (R7, AC10, AC11).
- 검증 모드: build/static, automated behavior, app runtime, performance measurement, 그리고 throwaway 워크스페이스에 한정된 live herdr integration 하나 (section 9.1).
- 배포 모드: local. 실행 브랜치에 커밋 하나, push와 PR 없음 (section 4.3).
- `review_profile: standard`와 그 근거 (frontmatter).

## 2. Problem, Goal, And Users

사용자는 Hide의 단일 운영자이며 herdr TUI를 병행하지 않는다.
그가 느끼는 문제는 "UI가 herdr를 그대로 따라가니 느리고, 로딩도 다 느리다"였다.

리서치로 확인된 사실은 다음과 같다.

- 탭 전환: 탭 전환 핸들러가 `pane_layout`을 `None`으로 비우고 다음 publish를 기다린다. 그동안 셸은 균등 분할로 그린다. 캔버스는 활성 탭의 identity를 뷰 id로 쓰므로 전환마다 터미널 뷰가 파괴되고 재생성되어 버퍼가 비고, 새 뷰의 크기 보고가 resize를 보내야 herdr가 전체 프레임을 다시 보낸다. 로컬 snapshot에는 layout이 하나뿐이라 클라이언트 쪽 선택이 불가능하다. 원격 경로는 이미 모든 layout을 싣는다.
- pane 포커스: 클릭은 `pane.focus` 요청만 보내고 아무 것도 바꾸지 않는다. 확인 이벤트가 온 뒤에야 포커스 링과 first responder가 움직인다. 지연은 요청 응답 약 130ms, 이벤트 약 170ms로 측정된 바 있다.
- 시작: 셸이 core를 런타임 없이 한 번 만들고, 로그인 셸 PATH 조회와 버전 조사 뒤 core를 파괴하고 다시 만든다. 두 번의 `session.snapshot`, 두 번의 구독, 두 번의 git 카탈로그 구성이 든다. 첫 attach는 뷰가 크기를 보고하기 전에 24x80으로 시작한다.
- 갱신: 1초마다 `agent.list` 결과가 같아도 projection 전체를 lock 안에서 다시 계산한다. 변경 알림과 snapshot 읽기 사이에 합치기가 없어 알림 폭주가 곧 메인 스레드의 mutex 대기가 된다. delta의 복제와 JSON 직렬화가 mutex를 잡은 채 이뤄진다.
- 교훈: 2026-09-03의 커밋 9570a2a는 pane 닫기를 낙관적으로 그렸다가 되돌렸다. 예측한 트리는 맞았지만 셸이 그리는 layout이 PTY 크기를 결정하므로, herdr가 적용하지 않은 layout을 그리면 양쪽이 PTY 크기를 두고 다툰다. 이 PRD는 그 경계를 침범하지 않는다.

목표는 사용자의 입력이 화면에 한 프레임 안에 반영되고, 방문했던 탭이 스크롤백째 즉시 돌아오며, 시작이 한 번의 초기화로 끝나는 것이다.

### 2.1 User Scenarios

- SC1. 방문한 탭 사이의 전환: 운영자가 ⌥Tab이나 ⌘N으로 이미 봤던 탭으로 돌아간다.
  Actors: 운영자.
  Primary path: 키를 놓는 프레임에 스트립의 활성 표시와 캔버스가 함께 바뀌고, 각 pane은 마지막으로 봤던 스크롤백과 실제 분할 기하를 그대로 보인다. 빈 화면도 균등 격자도 지나가지 않는다.
  Failure state: herdr가 그 사이 그 탭의 layout을 바꿨다면 캔버스는 herdr가 마지막으로 보낸 layout을 그린다. 예측하지 않는다.
  Recovery: herdr의 `tab_focused`가 다른 탭을 가리키고 hide의 요청이 대기 중이지 않으면 hide는 herdr를 따라가고 그 사실을 진단으로 남긴다.
  Reach: 한 checkout에 탭 셋을 열고 각각을 한 번씩 방문한 셸.

- SC2. 처음 방문하는 탭: 운영자가 아직 열어보지 않은 탭을 고른다.
  Actors: 운영자.
  Primary path: 스트립의 활성 표시는 즉시 바뀌고, 캔버스는 herdr가 보낸 그 탭의 layout으로 분할된 채 각 pane의 시작 상태를 보이다가, attach가 끝나면 한 번의 전체 프레임으로 채워진다.
  Failure state: attach가 실패하면 그 pane에 실패 이유가 보이고 나머지 pane은 영향을 받지 않는다.
  Recovery: 같은 탭을 다시 고르면 attach된 pane은 스크롤백을 유지한 채 즉시 보인다.
  Reach: 아직 방문하지 않은 탭이 있는 셸.

- SC3. pane 포커스: 운영자가 다른 pane을 클릭하거나 단축키로 옮긴다.
  Actors: 운영자.
  Primary path: 클릭한 프레임에 포커스 링이 옮겨 가고 곧바로 타이핑한 글자가 그 pane으로 들어간다. herdr에는 비동기로 포커스가 통보된다.
  Failure state: herdr가 요청을 거부하거나 응답하지 않으면 hide의 포커스는 유지되고 거부가 진단으로 남으며, 다음 herdr 이벤트가 다른 pane을 가리키면 그때 따라간다.
  Recovery: herdr CLI로 밖에서 pane을 포커스하면 hide는 다음 이벤트에 그 pane으로 옮겨 간다.
  Reach: 한 탭에 pane 둘이 있는 셸과 herdr CLI.

- SC4. 시작: 운영자가 Hide를 켠다.
  Actors: 운영자.
  Primary path: 창이 뜨고 첫 터미널 프레임이 실제 pane 크기로 한 번에 그려진다. core는 한 번만 만들어지고 `session.snapshot`, 구독, 카탈로그 구성도 한 번씩이다.
  Failure state: herdr 바이너리를 찾지 못하면 그 사실이 상태 표시줄에 보이고 창은 뜬 채로 있다.
  Recovery: 시작 추적이 프로세스 시작부터 첫 프레임까지의 시간을 남겨 다음 회귀를 잡을 수 있다.
  Reach: herdr 서버가 떠 있는 상태에서 조립된 dev 번들을 실행.

- SC5. 유휴 상태: 운영자가 아무 것도 하지 않는다.
  Actors: 운영자.
  Primary path: 에이전트 목록이 변하지 않는 동안 projection은 다시 계산되지 않고, 메인 스레드는 mutex를 기다리지 않는다.
  Failure state: 알림이 폭주해도 메인 스레드의 snapshot 읽기는 프레임당 한 번이다.
  Recovery: 목록이 실제로 변하면 그 틱에 반영된다.
  Reach: pane 여럿이 attach된 셸을 1분간 방치.

## 3. Scope And Non-Goals

범위: 권위 경계의 명문화, 로컬 snapshot의 탭별 layout, hide 소유의 보이는 탭과 포커스 pane, 터미널 뷰 유지, 한 번의 core 초기화, 실제 크기의 첫 attach, 알림 합치기와 lock 밖 직렬화, 변화 없는 틱의 재계산 생략, 그리고 전후 측정.

비목표, 각각 의도된 제외:

- 줌, 분할, pane 닫기, pane 크기 조절의 낙관적 반영.
  이들은 pane 기하를 바꾸고 기하는 PTY 크기를 결정한다.
  커밋 9570a2a가 그 다툼을 기록했다.
  Consequence: 이 동작들은 herdr의 확인까지 약 170ms를 기다린다.
  Revisit: 셸이 PTY 크기를 herdr가 적용한 layout에서만 정하도록 분리하는 별도 작업 뒤에.
- `agent.list` 폴링을 이벤트 구독으로 대체.
  herdr의 `pane.agent_status_changed`는 pane마다 구독해야 하고 토큰을 싣지 않는다.
  Consequence: 에이전트 상태 반영 지연은 최대 1초로 남는다.
  Revisit: herdr가 전역 구독을 제공할 때.
- herdr 서버가 보이지 않는 pane까지 렌더링하는 문제.
  다른 저장소의 몫이다.
  Consequence: 서버 CPU는 이 PRD로 줄지 않는다.
- 원격(mini) 컨텍스트.
  이미 모든 layout을 싣고 낙관적 포커스를 한다.
  Consequence: 원격 경로는 그대로다.
- 탭 순서와 목록 구조.
  `hide-chrome-and-tabs` PRD의 범위이며 이 PRD는 그 위에 얹힌다.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
측정과 검증에 필요한 것은 모두 로컬 저장소와 herdr 서버 안에 있다.

### 4.2 Human Decisions Before PRD Approval

- 권위 경계와 그 조정 규칙을 승인한다.
  hide의 요청이 대기 중이면 herdr의 이벤트는 그 요청의 확인으로 읽고, 대기 중인 요청 없이 herdr가 다른 값을 보내면 hide가 따라간다.
- 방문한 탭의 터미널 뷰를 살려 두는 메모리 대가를 승인한다.
  방문한 pane마다 터미널 뷰와 버퍼가 유지된다.
- 성능 예산을 승인한다.
  방문한 탭 전환과 포커스 이동은 한 프레임(16ms) 안에 그려지고, 따뜻한 시작의 첫 터미널 프레임은 기준선 측정값의 절반 이하이며 500ms를 넘지 않는다.

### 4.3 Decision Traceability For Fidelity Review

이 PRD는 인터뷰 qa-log 없이 대화만을 근거로 하므로 사용자의 결정을 여기에 그대로 남긴다.

- 사용자 요청 원문 (2026-09-03): "herdr layout 이게 herdr 을 태우고 있는데,, 뭔가 UI를 그대로 따라가게 하니 느린것같아.. tab 안에서 pane간의 관계 같은것도 그대로 따라가니 느리고,, native 반응성으로 가려면 좀 더 native하게 가면서 herdr api,socket을 적절하게 써야하는데 그 경계를 어떻게 잡아야할지 고민이다". R1, section 5.
- 사용자 요청 원문: "이 원칙대로 하면 단순하긴 하지만 반응성 관점에서 아쉬운것 같아. 기본적으로 로딩도 뭔가 다 느리고!". R5, R7, SC4.
- 에이전트 제안, 사용자가 수용한 것 (2026-09-03 논의): herdr 권위는 pane 존재, 분할 트리, cwd, 에이전트 생명주기, PTY; hide 권위는 보이는 탭, 키보드 포커스, 줌, 사이드바, 스크롤. 이 PRD는 줌을 hide 권위에서 제외했다. 리서치에서 줌이 layout 기하로만 전달되고 PTY 크기를 바꾼다는 사실이 확인되었기 때문이다. 이 제외는 비목표로 기록하고 4.2에 올렸다.
- 에이전트 제안, 사용자가 수용한 것: "로딩이 느리다"는 원인을 모르니 sample로 측정한 뒤 판단한다. R7, T1.
- 사용자 선택 (2026-09-04, 질문 "hide와 herdr TUI를 같이 쓰시나요?"): "hide만 쓴다". 옵션 설명은 "활성 탭, 포커스 pane, 줌은 hide가 즉시 반영하고 herdr에는 비동기로 통보. herdr가 다른 값을 주면 herdr를 따르되 사용자 입력 직후 짧은 유예를 둠". 수용: 즉시 반영과 비동기 통보(R2, R4), herdr 추종(R1). "짧은 유예"는 "hide의 요청이 대기 중인 동안"으로 구체화했다. 거부된 대안: herdr 포커스가 진실이고 hide는 항상 되돌림.
- 사용자 선택 (2026-09-04): "3개로 분할". 이 PRD는 항목 4를 담고, "sample 측정 포함"이 옵션 설명에 있었다. R7.
- 에이전트 가정 (사용자 결정 아님): 방문한 탭의 터미널 뷰를 살려 둔다. 줌이 이미 같은 방식(뷰 유지, 가시성만 전환)을 쓰고 테스트가 그것을 지킨다. R3.
- 에이전트 가정 (사용자 결정 아님): 성능 예산 수치. 한 프레임 기준은 60Hz 디스플레이의 16ms, 시작 500ms는 ADR-0001이 스파이크에서 기록한 따뜻한 시작 186ms를 근거로 여유를 둔 값이다. 4.2에 올렸다.
- 배포 모드: `agents/config.json`의 `delivery.mode: local`, `worktree.enabled: true`를 그대로 따른다.
- 원칙 인테이크: `~/projects/oh-my-principle` 커밋 `35ab76ca23d45e714f1630054855a8c8c4568d03`에서 `engineering/principles.md`와 `design/principles.md`를 전문으로 읽었다. 적용 규칙은 section 11에 번역했다. design 규칙은 이 PRD가 새 화면 구성을 만들지 않으므로 규칙 5(기존 패턴: 줌의 뷰 유지 방식을 탭 전환에 확장)만 번역했다.
- 프로젝트 규칙 인테이크: `AGENTS.md` "Performance Guide"의 네 규칙 전부, "Runtime Architecture"(셸은 권위를 갖지 않는다는 문장은 이 PRD로 "셸은 여전히 권위를 갖지 않되 core가 뷰 상태의 권위를 갖는다"로 갱신되어야 하며 T8이 문서를 고친다), "Herdr API Contract", `docs/dev-runtime.md`의 단일 인스턴스 규칙, `docs/architecture/adr-0001-native-surface-ownership.md`의 측정 증거 기준을 section 11에 번역했다.

## 5. Major Technical Structure Changes

- 권위 경계가 core의 모델에 명문화된다.
  herdr 권위: pane 존재, 분할 기하, 줌, cwd, 에이전트 생명주기, PTY.
  hide(core) 권위: checkout마다 보이는 탭, 키보드 포커스 pane, 패널 가시성, 글자 배율.
  hide 권위 항목은 core가 즉시 바꾸고 herdr에 비동기로 통보하며, 대기 중인 통보가 없을 때 herdr가 다른 값을 보내면 core가 따라간다.
- 로컬 snapshot이 세션의 모든 탭 layout을 tab id로 싣는다.
  `layout_updated`가 해당 탭의 항목만 갱신하고 탭 전환은 어떤 layout도 비우지 않는다.
  탭 전환 시 layout을 `None`으로 만드는 경로와 셸의 균등 분할 대체 경로는 삭제된다.
- 셸의 캔버스가 방문한 탭의 터미널 뷰를 유지한다.
  보이는 탭만 가시 상태이고 나머지는 숨김 상태로 남아 버퍼와 크기를 지킨다.
  탭 전환이 resize를 일으키지 않는다.
- 시작 순서가 바뀐다.
  런타임 해석(로그인 셸 PATH, 바이너리, 버전)이 core 생성 전에 끝나고 core는 한 번만 만들어진다.
  첫 attach는 뷰가 보고한 크기(없으면 마지막으로 알려진 크기)로 시작한다.
  시작 추적이 프로세스 시작부터 첫 터미널 프레임까지를 한 값으로 남긴다.
- snapshot 전달: 변경 알림은 메인 스레드에서 프레임당 한 번으로 합쳐지고, delta의 직렬화는 lock 밖에서 이뤄지며, `agent.list` 결과가 이전과 같은 틱은 projection을 다시 계산하지 않는다.
- 스키마, 저장소, 인증, 결제, 배포 변경 없음. 새 서드파티 의존성 없음.

## 6. Requirements

- R1. core는 hide 권위 항목(보이는 탭, 포커스 pane, 패널 가시성, 글자 배율)을 즉시 바꾸고, 그중 herdr가 알아야 하는 것(보이는 탭, 포커스 pane)은 비동기로 통보한다.
  통보가 대기 중인 동안 도착한 herdr의 같은 종류 이벤트는 그 통보의 확인으로 읽는다.
  대기 중인 통보 없이 herdr가 다른 탭이나 pane을 가리키면 core는 그것을 따르고 pane id와 출처를 담은 구조화된 진단을 남긴다.
  통보가 거부되거나 시간 안에 응답이 없으면 core의 값은 유지되고 그 사실이 진단으로 남는다.
  herdr 권위 항목(pane 존재, 분할 기하, 줌, cwd, 에이전트 생명주기)은 지금처럼 herdr의 이벤트만이 바꾼다.
- R2. 로컬 snapshot은 세션의 모든 탭 layout을 싣는다.
  탭 전환은 layout을 비우지 않고 보이는 탭만 바꾸며, 스트립의 활성 표시와 캔버스가 같은 프레임에 바뀐다.
  캔버스는 어떤 순간에도 herdr가 마지막으로 보낸 그 탭의 layout만 그리고, 균등 분할이나 예측한 기하를 그리지 않는다.
- R3. 방문한 탭의 터미널 뷰는 탭 전환에 유지된다.
  다시 방문한 탭은 스크롤백과 기하를 그대로 보이고, 전환은 resize를 일으키지 않으며, 숨겨진 pane의 PTY 크기는 숨겨진 동안 바뀌지 않는다.
  처음 방문하는 탭의 pane은 지금처럼 그때 attach된다.
- R4. 키보드 포커스 pane은 클릭이나 단축키의 프레임에 옮겨 가고 그 즉시 입력이 그 pane으로 간다.
  포커스 링과 first responder는 herdr의 확인을 기다리지 않는다.
- R5. 시작은 core 초기화 한 번으로 끝난다.
  `session.snapshot`, 이벤트 구독, 카탈로그 구성이 시작당 한 번씩이다.
  첫 attach는 뷰가 보고한 크기로 시작해 시작 시 pane마다 전체 프레임이 한 번만 온다.
  시작 추적이 프로세스 시작부터 첫 터미널 프레임까지의 시간을 남긴다.
- R6. 변경 알림은 메인 스레드에서 프레임당 한 번의 snapshot 읽기로 합쳐진다.
  delta의 복제는 lock 안에서, 직렬화는 lock 밖에서 이뤄진다.
  `agent.list` 결과가 직전과 같은 틱은 projection을 다시 계산하지 않는다.
- R7. 변경 전과 후를 같은 방법으로 측정한다.
  조립된 dev 번들의 단일 인스턴스에서 `/usr/bin/sample`로 앱과 herdr 서버를 잡고, 시작 추적과 전환 추적으로 시간을 읽는다.
  예산: 방문한 탭 전환과 포커스 이동은 입력부터 그려지기까지 16ms 이하, 따뜻한 시작의 첫 터미널 프레임은 기준선의 절반 이하이고 500ms 이하, 유휴 CPU는 기준선 이하다.
- R8. `AGENTS.md`의 "Runtime Architecture"와 "Performance Guide"가 새 경계와 알림 합치기 규칙을 말하도록 갱신된다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | 보이는 탭이나 포커스 pane을 바꾸는 core 이벤트 직후의 snapshot이 이미 새 값을 담고, herdr 통보가 대기 중인 동안 도착한 같은 종류의 herdr 이벤트는 값을 바꾸지 않으며, 대기 중인 통보 없이 도착한 다른 값의 herdr 이벤트는 값을 바꾸고 진단을 남긴다 | machine | - |
| AC2 | herdr 통보가 거부되거나 시간 초과되면 core의 값은 유지되고 진단이 남는다 | machine | - |
| AC3 | 로컬 snapshot이 세션의 모든 탭 layout을 tab id로 담고, 탭 전환 뒤에도 어떤 탭의 layout도 비어 있지 않으며, 한 탭의 `layout_updated`는 그 탭의 항목만 바꾼다 | machine | - |
| AC4 | 방문했던 탭으로 전환할 때 스트립의 활성 표시와 캔버스가 같은 프레임에 바뀌고, 그 사이에 빈 프레임이나 균등 분할 프레임이 없으며, 각 pane의 스크롤백이 전환 전과 같다 | judged | 실행 중인 앱에서 탭 셋을 각각 방문한 뒤 ⌥Tab 전환을 프레임 단위로 캡처하고, 전환 전후의 pane 스크롤백 캡처를 비교 |
| AC5 | 탭 전환은 herdr에 resize 요청을 보내지 않고, 숨겨진 pane의 크기는 숨겨진 동안 바뀌지 않는다 | machine | - |
| AC6 | 처음 방문하는 탭은 herdr가 보낸 layout대로 분할된 채 시작 상태를 보이다가 attach 뒤 한 번의 전체 프레임으로 채워지고, attach 실패는 그 pane에만 이유로 보인다 | judged | 실행 중인 앱에서 미방문 탭을 고르는 순간부터 첫 프레임까지를 캡처하고, herdr 바이너리 경로를 잘못 준 두 번째 실행에서 실패 표시를 캡처 |
| AC7 | pane을 클릭한 프레임에 포커스 링이 옮겨 가고 바로 이어 타이핑한 글자가 그 pane의 터미널에 들어간다 | judged | 실행 중인 앱에서 pane 둘 중 하나를 클릭하고 즉시 고정 문자열을 타이핑한 뒤, 클릭 프레임과 두 pane의 내용을 캡처 |
| AC8 | herdr CLI로 밖에서 다른 pane과 탭을 포커스하면 hide가 다음 이벤트에 그것을 따르고 진단을 남긴다 | judged | throwaway 워크스페이스에서 CLI 포커스 명령 전후의 hide 캡처와 진단 로그 |
| AC9 | 한 번의 시작에 core 생성, `session.snapshot`, 이벤트 구독, 카탈로그 구성이 각각 한 번이고, 시작 시 attach된 각 pane의 전체 프레임 수신이 한 번이다 | machine | - |
| AC10 | 방문한 탭 전환과 포커스 이동의 입력부터 그리기까지가 16ms 이하다 | machine | - |
| AC11 | 따뜻한 시작의 첫 터미널 프레임까지의 시간이 변경 전 기준선의 절반 이하이고 500ms 이하이며, 유휴 CPU가 기준선 이하다 | machine | - |
| AC12 | 알림 폭주 중 메인 스레드의 snapshot 읽기는 프레임당 한 번이고, delta 직렬화는 lock 밖에서 이뤄지며, `agent.list`가 같은 틱은 projection을 다시 계산하지 않는다 | machine | - |
| AC13 | 기준선과 변경 후의 sample 결과가 같은 조건(단일 인스턴스, dev 번들, 같은 pane 수)에서 기록되어 있고, 변경 후 메인 스레드의 mutex 대기 비율이 기준선 이하다 | judged | 변경 전후 각각의 sample 출력과 그 조건 기록을 나란히 둔 측정 보고 |
| AC14 | `AGENTS.md`의 Runtime Architecture와 Performance Guide가 core의 뷰 상태 권위와 알림 합치기를 서술한다 | machine | - |

## 8. PRD-Level Tasks

- T1. 기준선을 측정한다: 단일 인스턴스 확인 뒤 sample로 앱과 herdr 서버를 잡고, 시작 시간, 탭 전환, 포커스 이동, 유휴 CPU를 기록한다. Covers R7, AC13. Depends on: none.
- T2. 로컬 snapshot이 모든 탭 layout을 싣게 하고, 탭 전환의 layout 비우기와 셸의 균등 분할 대체 경로를 삭제한다. Covers R2, AC3, SC1. Depends on: none.
- T3. core에 hide 권위 항목과 대기 중 통보 모델을 두고, 보이는 탭과 포커스 pane을 즉시 바꾸며 herdr에 비동기 통보하고, 확인, 추종, 거부, 시간 초과의 네 경로를 구현한다. Covers R1, R4, AC1, AC2, AC7, AC8, SC3. Depends on: T2.
- T4. 셸의 캔버스가 방문한 탭의 터미널 뷰를 가시성만 바꿔 유지하게 하고 전환이 resize를 내지 않게 한다. Covers R3, AC4, AC5, AC6, SC1, SC2. Depends on: T2.
- T5. 시작을 core 초기화 한 번으로 바꾸고 첫 attach가 뷰의 실제 크기로 시작하게 하며, 첫 터미널 프레임까지의 시작 추적을 남긴다. Covers R5, AC9, SC4. Depends on: none.
- T6. 변경 알림을 프레임당 한 번으로 합치고, delta 직렬화를 lock 밖으로 옮기며, 같은 `agent.list` 틱의 재계산을 생략한다. Covers R6, AC12, SC5. Depends on: none.
- T7. 변경 후를 T1과 같은 방법으로 측정해 예산과 비교한다. Covers R7, AC10, AC11, AC13. Depends on: T3, T4, T5, T6.
- T8. `AGENTS.md`의 Runtime Architecture와 Performance Guide를 새 경계에 맞게 고친다. Covers R8, AC14. Depends on: T3, T6.
- T9. 검증 픽스처를 준비한다: throwaway herdr 워크스페이스에 탭 셋과 pane 둘, CLI 포커스 스크립트, 잘못된 바이너리 경로로의 두 번째 실행. Covers SC1, SC2, SC3, SC4. Depends on: none.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | Rust core와 Swift 셸의 빌드, lock 밖 직렬화의 구조 검사, 문서 갱신 | none |
| automated behavior | yes | 권위 조정 규칙, layout 보존, 뷰 유지, 알림 합치기, 재계산 생략의 회귀 | none |
| app runtime | yes | 전환, 포커스, 시작의 사용자 흐름 | 최종 UX 판단 |
| performance measurement | yes | 기준선 대비 예산 | 예산 수치의 승인 |
| live herdr integration | yes | 밖에서 온 포커스 변화의 추종 | 이 PRD에서 승인된 격리 경계 |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | R1-R8, AC14 | Rust core와 Swift 셸이 깨끗이 빌드되고, 직렬화가 lock 안에 없다는 것이 기존 구조 검사 스크립트의 방식으로 정적으로 확인되며, 문서가 갱신되어 있다 | yes | no |
| V2 | automated behavior | R1, R2, R3, R5, R6, AC1, AC2, AC3, AC5, AC9, AC12 | 회귀 위험을 직접 겨냥한 테스트가 있다: 탭 전환이 다시 layout을 비우는 것, herdr 이벤트가 대기 중인 통보를 덮어쓰는 것, 대기 없는 herdr 이벤트가 무시되는 것, 시간 초과가 조용히 넘어가는 것, 탭 전환이 resize를 보내는 것, 시작이 core를 두 번 만드는 것, 같은 틱이 재계산되는 것. 각 테스트는 snapshot과 herdr로 나간 요청을 단언한다 | yes | no |
| V3 | app runtime | R2, R3, R4, AC4, AC6, AC7, SC1, SC2, SC3 | 조립된 dev 번들에서 방문한 탭 전환이 한 프레임에 스크롤백째 이뤄지고, 미방문 탭이 실제 layout으로 시작해 한 프레임으로 채워지며, 클릭 즉시 입력이 새 pane으로 간다 | yes | no |
| V4 | app runtime | R5, AC9, SC4 | 조립된 dev 번들의 시작이 한 번의 초기화로 첫 프레임에 이르고, 잘못된 바이너리 경로에서는 창이 뜬 채 이유가 보인다 | yes | no |
| V5 | performance measurement | R7, AC10, AC11, AC13, SC5 | 변경 전후가 같은 조건에서 측정되어 예산 안에 들고 mutex 대기가 줄었다는 것이 sample과 추적 값으로 보인다 | yes | no |
| V6 | live herdr integration | R1, AC8, SC3 | throwaway 워크스페이스에서 CLI로 바꾼 포커스와 탭을 hide가 따르고 진단을 남긴다 | yes | no |

Live 모드의 부작용 경계:

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked | Allowed Side Effect | Sensitive Data Policy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| V6 | live herdr integration | R1, AC8, SC3 | 위와 같음 | yes | no | 이 실행이 만든 throwaway herdr 워크스페이스 하나와 그 안의 탭과 pane을 만들고 포커스하고 닫는다. 이 실행이 만들지 않은 pane, 탭, 워크스페이스는 닫거나 옮기거나 포커스하거나 프롬프트를 보내지 않는다 | 캡처에서 checkout 밖의 경로와 토큰을 가린다 |

### 9.3 Human Verification

- 성능 예산 수치의 승인 (4.2).
- 전환과 포커스의 체감이 운영자에게 "네이티브"로 느껴지는지, 그리고 유지된 터미널 뷰의 메모리 사용이 받아들일 만한지 운영자가 판단한다.

## 10. Risks And Open Decisions

- 낙관적 포커스와 herdr의 seen 의미가 어긋날 수 있다.
  herdr는 여전히 자기 이벤트로 seen을 판정하지만, `hide-agent-attention` PRD가 읽음을 hide 소유로 옮기므로 사용자에게 보이는 결과는 hide의 값이다.
- 모든 탭의 layout을 싣고 뷰를 유지하면 방문한 pane마다 메모리가 든다.
  4.2의 인간 결정이며 V5가 유휴 상태를 측정한다.
- core 초기화를 한 번으로 줄이면 창이 뜨기 전에 런타임 해석이 끝나야 한다.
  지금의 두 단계는 "Finder 실행이 느린 셸에 첫 창을 잃지 않도록" 도입되었다.
  T5는 창을 먼저 띄우되 core를 런타임 해석 뒤 한 번만 만드는 순서로 두 목적을 모두 지켜야 하며, 창이 뜨기까지의 시간이 늘면 보고한다.
- 예산 수치는 가정이다.
  T1의 기준선이 나온 뒤 수치가 비현실적이면 T7에서 보고하고 사람의 결정을 기다린다.
- `hide-chrome-and-tabs`가 먼저 끝나야 한다.
  이 PRD의 보이는 탭은 그 PRD의 통합 탭 목록 위에 정의된다.
- live 검증은 운영자의 herdr 서버에서 돈다. V6의 격리 경계가 완화책이고 section 11이 금지로 적는다.
- 측정 산출물과 캡처는 `agents/runs/hide-view-state/` 아래에만 두며 커밋하지 않는다.

## 11. Implementation Guardrails

운영자의 지침과 이 저장소의 규칙에서:

- 이 실행이 만들지 않은 herdr pane, 탭, 워크스페이스를 닫거나 옮기거나 포커스하거나 프롬프트를 보내지 않는다. 운영자의 실행 중인 Hide 인스턴스와 상호작용하지 않는다.
- section 6을 넘어 범위를 넓히지 않고, section 5를 넘어 구조를 바꾸지 않으며, 서드파티 의존성을 추가하지 않는다. 특히 줌, 분할, 닫기, 크기 조절을 낙관적으로 그리지 않는다.
- 숨겨진 사용자 흐름을 추가하지 않는다.
- 캔버스가 그리는 기하는 언제나 herdr가 이미 보낸 layout이다. 예측한 기하로 PTY 크기를 정하지 않는다 (커밋 9570a2a의 교훈).
- Herdr 통합은 `AGENTS.md` "Herdr API Contract"를 따른다: 공식 문서를 읽고 계약 스키마와 실제 바이너리로 확인하며 기존 호출부에서 추측하지 않는다.
- 성능은 `AGENTS.md` "Performance Guide"를 따른다: mutex를 subprocess, 블로킹 I/O, 큰 직렬화에 걸쳐 잡지 않고, 틱이나 이벤트 경로에서 subprocess를 부르지 않으며, 탭별 layout은 revisioned `rest`에 실리고, 성능 주장은 `docs/dev-runtime.md`의 단일 인스턴스 규칙 아래 sample로 증명한다.
- 측정 증거는 `docs/architecture/adr-0001-native-surface-ownership.md`의 기준을 따른다: 디버그 빌드, 소스 읽기, 단일 표본은 성능 증거가 아니다.
- 증거는 `AGENTS.md` "Evidence Belongs Outside The Repository"를 따른다: sample 출력, 추적, 캡처는 `agents/runs/hide-view-state/`에 두고 커밋하지 않는다.
- engineering/principles.md 규칙 1: 탭 전환의 layout 비우기, 균등 분할 대체, 두 번째 core 생성, 24x80 기본 attach 크기를 같은 변경에서 지운다.
- engineering/principles.md 규칙 2: 대기 중 통보 모델은 보이는 탭과 포커스 pane 둘에만 적용하고 일반 낙관적 상태 프레임워크를 만들지 않는다.
- engineering/principles.md 규칙 3: 측정(T1)이 먼저, layout 보존(T2)이 그 위에, 권위 이전(T3)과 뷰 유지(T4)가 그 위에, 재측정(T7)이 마지막이다.
- engineering/principles.md 규칙 4, 10: 통보 거부와 시간 초과, herdr의 예상 밖 값은 조용히 넘어가지 않고 진단으로 드러난다.
- engineering/principles.md 규칙 5: 권위 조정은 core의 runtime에, 뷰 유지는 셸의 캔버스에, 알림 합치기는 브리지에 머문다.
- engineering/principles.md 규칙 7: 뷰 유지는 줌이 이미 쓰는 가시성 전환 방식을 확장하고, 탭별 layout은 원격 경로가 이미 쓰는 형태를 따른다.
- engineering/principles.md 규칙 8: 뷰 상태의 권위를 core에 두는 것은 장기 결정이다.
- engineering/principles.md 규칙 9: 추종, 거부, 시간 초과 진단은 이벤트 이름, pane id, 출처를 담고 내용을 담지 않는다.
- engineering/principles.md 규칙 11: 같은 탭이나 pane으로의 반복 요청은 같은 상태로 수렴하고 중복 통보를 보내지 않는다.
- engineering/principles.md 규칙 12, 13: 테스트는 snapshot과 herdr로 나간 요청을 단언하고, 느림은 세 단계 렌더링이라는 부류를 없애는 방식으로 고친다.
- design/principles.md 규칙 5: 탭 전환의 뷰 유지는 줌이 쓰는 기존 패턴을 따른다.
- Git과 PR 귀속: 브랜치 이름, 커밋 메시지, 트레일러, 생성 텍스트 어디에도 에이전트, 모델, 벤더, 도구 이름을 쓰지 않는다.

## 12. Implementation Result Report Contract

보고 항목:

- status: `Done`, `Partially Done`, `Blocked`.
- 사용자에게 보이는 변화: 탭 전환, 포커스, 시작 각각의 전후 체감과 측정값.
- 바뀐 모듈과 새 모듈의 책임 경계, 실제로 고른 파일 구조.
- section 5의 구조를 따랐는지, 벗어난 곳과 이유.
- T1부터 T9까지의 완료 상태와 R/AC/V 커버리지.
- 모드별 검증 증거와 각 산출물이 있는 실행 디렉터리.
- T1과 T7에 대해: 측정 조건(인스턴스 확인, 번들 종류, pane 수)과 수치의 나란한 비교, 예산을 넘긴 항목과 그 이유.
- T5에 대해: 창이 뜨기까지의 시간이 변경 전후로 어떻게 달라졌는지.
- V6에 대해: 이 실행이 만든 herdr 워크스페이스, 탭, pane과 그 전부가 닫혔다는 확인, 그 밖의 어떤 것도 건드리지 않았다는 확인.
- 추가되거나 바뀐 자동 테스트와 각각이 막는 회귀.
- 삭제된 코드 경로의 목록.
- 이탈, 남은 인간 검토, 미완 항목과 후속 후보.
